// SPDX-License-Identifier: GPL-2.0-or-later
//! IPP client for the printer's `ipp://host:631/ipp/print` endpoint, plus the TLV codec.
//!
//! * [`IppMessage`] / [`IppValue`]: the RFC 8010 binary encoding (attribute groups, collections,
//!   out-of-band values). The decoder is bounds-checked and never panics on bad input.
//! * [`PrinterAttributes`]: the typed view Washi needs for status when the CHMP print service is
//!   asleep (state, ink markers, supplies, media, formats, ...).
//! * [`IppClient`]: reads (`Get-Printer-Attributes`, `Get-Job-Attributes`, `Get-Jobs`) are free.
//!   Everything that starts, changes or cancels device activity (`Print-Job`, `Validate-Job`,
//!   `Cancel-Job`, `Identify-Printer`) takes a [`Confirm`](crate::Confirm), see [`job`].
//! * [`media`]: the printer's `media-col-database` (which size / source / type / margin
//!   combinations exist, including borderless and the disc tray).

pub mod job;
pub mod localization;
pub mod media;

pub use job::{
    IdentifyAction, JobAttributes, JobState, Margins, MediaSpec, PrintCapabilities, PrintColorMode,
    PrintJob, Quality, Scaling, Sides, ValidateReply, WhichJobs,
};
pub use localization::{LOCALIZATION_ATTRIBUTES, PrinterLocalization, normalize_language};
pub use media::{MediaDatabase, MediaEntry};

use std::collections::BTreeMap;
use std::fmt;
use std::io::Read;
use std::time::Duration;

use serde::Serialize;
use thiserror::Error;

use crate::error::NetError;
use crate::http::{Endpoint, MAX_BODY, request_once, uri_host};

pub const DEFAULT_IPP_PORT: u16 = 631;
pub const DEFAULT_IPP_PATH: &str = "/ipp/print";

/// Operation ids (RFC 8011 section 5.2, PWG 5100.13).
pub mod op {
    pub const PRINT_JOB: u16 = 0x0002;
    pub const VALIDATE_JOB: u16 = 0x0004;
    pub const CANCEL_JOB: u16 = 0x0008;
    pub const GET_JOB_ATTRIBUTES: u16 = 0x0009;
    pub const GET_JOBS: u16 = 0x000A;
    pub const GET_PRINTER_ATTRIBUTES: u16 = 0x000B;
    pub const IDENTIFY_PRINTER: u16 = 0x003C;

    /// Operations that only read.
    pub fn is_read(code: u16) -> bool {
        matches!(code, GET_JOB_ATTRIBUTES | GET_JOBS | GET_PRINTER_ATTRIBUTES)
    }

    pub fn name(code: u16) -> String {
        match code {
            PRINT_JOB => "Print-Job".to_string(),
            0x0003 => "Print-URI".to_string(),
            VALIDATE_JOB => "Validate-Job".to_string(),
            0x0005 => "Create-Job".to_string(),
            0x0006 => "Send-Document".to_string(),
            CANCEL_JOB => "Cancel-Job".to_string(),
            GET_JOB_ATTRIBUTES => "Get-Job-Attributes".to_string(),
            GET_JOBS => "Get-Jobs".to_string(),
            GET_PRINTER_ATTRIBUTES => "Get-Printer-Attributes".to_string(),
            0x0039 => "Cancel-My-Jobs".to_string(),
            0x003B => "Close-Job".to_string(),
            IDENTIFY_PRINTER => "Identify-Printer".to_string(),
            other => format!("op-{other:#06x}"),
        }
    }
}
const OP_GET_PRINTER_ATTRIBUTES: u16 = op::GET_PRINTER_ATTRIBUTES;

/// The attributes `washi` asks for (a plain `all` is ~380 KB on the XK140, mostly
/// `media-col-database`).
pub const STATUS_ATTRIBUTES: &[&str] = &[
    "printer-state",
    "printer-state-reasons",
    "printer-state-message",
    "marker-names",
    "marker-colors",
    "marker-types",
    "marker-levels",
    "marker-low-levels",
    "marker-high-levels",
    "printer-supply",
    "printer-supply-description",
    "printer-firmware-string-version",
    "printer-make-and-model",
    "media-source-supported",
    "media-type-supported",
    "media-supported",
    "document-format-supported",
    "print-quality-supported",
    "printer-more-info",
    "identify-actions-supported",
];

/// Delimiter and value tags (RFC 8010 section 3.5).
pub mod tag {
    pub const OPERATION_ATTRIBUTES: u8 = 0x01;
    pub const JOB_ATTRIBUTES: u8 = 0x02;
    pub const END_OF_ATTRIBUTES: u8 = 0x03;
    pub const PRINTER_ATTRIBUTES: u8 = 0x04;
    pub const UNSUPPORTED_ATTRIBUTES: u8 = 0x05;

    pub const UNSUPPORTED: u8 = 0x10;
    pub const UNKNOWN: u8 = 0x12;
    pub const NO_VALUE: u8 = 0x13;
    pub const NOT_SETTABLE: u8 = 0x15;
    pub const DELETE_ATTRIBUTE: u8 = 0x16;
    pub const ADMIN_DEFINE: u8 = 0x17;

    pub const INTEGER: u8 = 0x21;
    pub const BOOLEAN: u8 = 0x22;
    pub const ENUM: u8 = 0x23;
    pub const OCTET_STRING: u8 = 0x30;
    pub const DATE_TIME: u8 = 0x31;
    pub const RESOLUTION: u8 = 0x32;
    pub const RANGE_OF_INTEGER: u8 = 0x33;
    pub const BEG_COLLECTION: u8 = 0x34;
    pub const TEXT_WITH_LANGUAGE: u8 = 0x35;
    pub const NAME_WITH_LANGUAGE: u8 = 0x36;
    pub const END_COLLECTION: u8 = 0x37;
    pub const TEXT: u8 = 0x41;
    pub const NAME: u8 = 0x42;
    pub const KEYWORD: u8 = 0x44;
    pub const URI: u8 = 0x45;
    pub const URI_SCHEME: u8 = 0x46;
    pub const CHARSET: u8 = 0x47;
    pub const NATURAL_LANGUAGE: u8 = 0x48;
    pub const MIME_MEDIA_TYPE: u8 = 0x49;
    pub const MEMBER_ATTR_NAME: u8 = 0x4A;
}

/// Collections nested deeper than this are rejected (stack safety on hostile input).
const MAX_DEPTH: usize = 16;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum IppError {
    #[error("IPP message is truncated")]
    Truncated,
    #[error("malformed IPP message: {0}")]
    Malformed(String),
    #[error("the printer answered IPP status {code:#06x} ({name})")]
    Status { code: u16, name: &'static str },
    #[error("the IPP reply carries no printer attributes")]
    NoPrinterAttributes,
    #[error("the IPP reply carries no job attributes")]
    NoJobAttributes,
    #[error("{tag:?} is not a language tag")]
    BadLanguage { tag: String },
    #[error("the printer refused the request: {name} ({code:#06x}){detail}")]
    Rejected { code: u16, name: &'static str, detail: String },
}

/// Human-readable name of an IPP status code (RFC 8011 section 5.4).
pub fn status_name(code: u16) -> &'static str {
    match code {
        0x0000 => "successful-ok",
        0x0001 => "successful-ok-ignored-or-substituted-attributes",
        0x0002 => "successful-ok-conflicting-attributes",
        0x0400 => "client-error-bad-request",
        0x0401 => "client-error-forbidden",
        0x0402 => "client-error-not-authenticated",
        0x0403 => "client-error-not-authorized",
        0x0404 => "client-error-not-possible",
        0x0405 => "client-error-timeout",
        0x0406 => "client-error-not-found",
        0x0407 => "client-error-gone",
        0x0408 => "client-error-request-entity-too-large",
        0x0409 => "client-error-request-value-too-long",
        0x040A => "client-error-document-format-not-supported",
        0x040B => "client-error-attributes-or-values-not-supported",
        0x040C => "client-error-uri-scheme-not-supported",
        0x040D => "client-error-charset-not-supported",
        0x040E => "client-error-conflicting-attributes",
        0x0500 => "server-error-internal-error",
        0x0501 => "server-error-operation-not-supported",
        0x0502 => "server-error-service-unavailable",
        0x0503 => "server-error-version-not-supported",
        0x0504 => "server-error-device-error",
        0x0505 => "server-error-temporary-error",
        0x0506 => "server-error-not-accepting-jobs",
        0x0507 => "server-error-busy",
        0x0508 => "server-error-job-canceled",
        _ => "unknown",
    }
}

// ---------------------------------------------------------------------------------------------
// Values
// ---------------------------------------------------------------------------------------------

/// Out-of-band value kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutOfBand {
    Unsupported,
    Unknown,
    NoValue,
    NotSettable,
    DeleteAttribute,
    AdminDefine,
}

impl OutOfBand {
    fn from_tag(t: u8) -> Option<OutOfBand> {
        Some(match t {
            tag::UNSUPPORTED => OutOfBand::Unsupported,
            tag::UNKNOWN => OutOfBand::Unknown,
            tag::NO_VALUE => OutOfBand::NoValue,
            tag::NOT_SETTABLE => OutOfBand::NotSettable,
            tag::DELETE_ATTRIBUTE => OutOfBand::DeleteAttribute,
            tag::ADMIN_DEFINE => OutOfBand::AdminDefine,
            _ => return None,
        })
    }

    fn tag(self) -> u8 {
        match self {
            OutOfBand::Unsupported => tag::UNSUPPORTED,
            OutOfBand::Unknown => tag::UNKNOWN,
            OutOfBand::NoValue => tag::NO_VALUE,
            OutOfBand::NotSettable => tag::NOT_SETTABLE,
            OutOfBand::DeleteAttribute => tag::DELETE_ATTRIBUTE,
            OutOfBand::AdminDefine => tag::ADMIN_DEFINE,
        }
    }
}

/// One IPP attribute value.
#[derive(Debug, Clone, PartialEq)]
pub enum IppValue {
    Integer(i32),
    Boolean(bool),
    Enum(i32),
    OctetString(Vec<u8>),
    /// RFC 3339 style text, e.g. `1970-01-02T23:55:25.0Z`.
    DateTime(String),
    /// `units` is 3 (dots per inch) or 4 (dots per centimetre).
    Resolution {
        x: i32,
        y: i32,
        units: i8,
    },
    Range {
        lower: i32,
        upper: i32,
    },
    Collection(Vec<(String, Vec<IppValue>)>),
    Text(String),
    Name(String),
    Keyword(String),
    Uri(String),
    UriScheme(String),
    Charset(String),
    NaturalLanguage(String),
    MimeMediaType(String),
    OutOfBand(OutOfBand),
    /// A value tag this decoder does not know, kept verbatim.
    Other {
        tag: u8,
        data: Vec<u8>,
    },
}

impl IppValue {
    /// The text of any string-like value (text, name, keyword, uri, ...).
    pub fn as_str(&self) -> Option<&str> {
        match self {
            IppValue::Text(s)
            | IppValue::Name(s)
            | IppValue::Keyword(s)
            | IppValue::Uri(s)
            | IppValue::UriScheme(s)
            | IppValue::Charset(s)
            | IppValue::NaturalLanguage(s)
            | IppValue::MimeMediaType(s) => Some(s),
            _ => None,
        }
    }

    /// Integer or enum value.
    pub fn as_i32(&self) -> Option<i32> {
        match self {
            IppValue::Integer(v) | IppValue::Enum(v) => Some(*v),
            _ => None,
        }
    }

    /// Octet strings that hold text (e.g. `printer-supply`).
    pub fn as_text_lossy(&self) -> Option<String> {
        match self {
            IppValue::OctetString(b) => Some(String::from_utf8_lossy(b).into_owned()),
            other => other.as_str().map(str::to_string),
        }
    }
}

impl fmt::Display for IppValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IppValue::Integer(v) | IppValue::Enum(v) => write!(f, "{v}"),
            IppValue::Boolean(v) => write!(f, "{v}"),
            IppValue::OctetString(b) => write!(f, "{}", String::from_utf8_lossy(b)),
            IppValue::DateTime(s) => f.write_str(s),
            IppValue::Resolution { x, y, units } => {
                write!(f, "{x}x{y}{}", if *units == 4 { "dpcm" } else { "dpi" })
            }
            IppValue::Range { lower, upper } => write!(f, "{lower}-{upper}"),
            IppValue::Collection(m) => write!(f, "{{{} members}}", m.len()),
            IppValue::OutOfBand(o) => write!(f, "{o:?}"),
            IppValue::Other { tag, data } => write!(f, "tag {tag:#04x} ({} bytes)", data.len()),
            other => f.write_str(other.as_str().unwrap_or_default()),
        }
    }
}

/// A named attribute with one or more values.
#[derive(Debug, Clone, PartialEq)]
pub struct IppAttribute {
    pub name: String,
    pub values: Vec<IppValue>,
}

impl IppAttribute {
    pub fn new(name: impl Into<String>, values: Vec<IppValue>) -> Self {
        IppAttribute { name: name.into(), values }
    }

    pub fn single(name: impl Into<String>, value: IppValue) -> Self {
        IppAttribute::new(name, vec![value])
    }

    pub fn keywords(name: impl Into<String>, words: &[&str]) -> Self {
        IppAttribute::new(name, words.iter().map(|w| IppValue::Keyword((*w).to_string())).collect())
    }
}

/// An attribute group (operation, printer, job, ...), identified by its delimiter tag.
#[derive(Debug, Clone, PartialEq)]
pub struct IppGroup {
    pub tag: u8,
    pub attributes: Vec<IppAttribute>,
}

impl IppGroup {
    pub fn new(tag: u8, attributes: Vec<IppAttribute>) -> Self {
        IppGroup { tag, attributes }
    }

    pub fn get(&self, name: &str) -> Option<&IppAttribute> {
        self.attributes.iter().find(|a| a.name == name)
    }
}

/// A whole IPP request or response. `code` is the operation id in a request and the status code
/// in a response.
#[derive(Debug, Clone, PartialEq)]
pub struct IppMessage {
    pub version: (u8, u8),
    pub code: u16,
    pub request_id: u32,
    pub groups: Vec<IppGroup>,
}

impl IppMessage {
    /// First group with this delimiter tag.
    pub fn group(&self, tag: u8) -> Option<&IppGroup> {
        self.groups.iter().find(|g| g.tag == tag)
    }

    /// `status-message` from the operation attributes, if the printer sent one.
    pub fn status_message(&self) -> Option<String> {
        let a = self.group(tag::OPERATION_ATTRIBUTES)?.get("status-message")?;
        a.values.first()?.as_str().map(str::to_string)
    }

    /// Serialise to the wire format.
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(256);
        out.push(self.version.0);
        out.push(self.version.1);
        out.extend_from_slice(&self.code.to_be_bytes());
        out.extend_from_slice(&self.request_id.to_be_bytes());
        for g in &self.groups {
            out.push(g.tag);
            for a in &g.attributes {
                for (i, v) in a.values.iter().enumerate() {
                    let name = if i == 0 { a.name.as_str() } else { "" };
                    encode_value(&mut out, name, v);
                }
            }
        }
        out.push(tag::END_OF_ATTRIBUTES);
        out
    }

    /// Parse a message. Bytes after `end-of-attributes` (document data) are ignored.
    pub fn decode(bytes: &[u8]) -> Result<IppMessage, IppError> {
        Self::decode_prefix(bytes).map(|(m, _)| m)
    }

    /// Parse a message and report how many bytes it occupied; what follows is document data
    /// (a `Print-Job` request).
    pub fn decode_prefix(bytes: &[u8]) -> Result<(IppMessage, usize), IppError> {
        let mut r = Reader { b: bytes, pos: 0 };
        let version = (r.u8()?, r.u8()?);
        let code = r.u16()?;
        let request_id = r.u32()?;
        let mut groups: Vec<IppGroup> = Vec::new();
        loop {
            let t = r.u8()?;
            if t == tag::END_OF_ATTRIBUTES {
                break;
            }
            if t <= 0x0F {
                groups.push(IppGroup { tag: t, attributes: Vec::new() });
                continue;
            }
            // A value tag.
            let name_len = r.u16()? as usize;
            let name = String::from_utf8_lossy(r.take(name_len)?).into_owned();
            let value = decode_value(&mut r, t, 0)?;
            let group = groups
                .last_mut()
                .ok_or_else(|| IppError::Malformed("attribute before any group".into()))?;
            if name.is_empty() {
                // An additional value of the previous attribute.
                let prev = group.attributes.last_mut().ok_or_else(|| {
                    IppError::Malformed("additional value without an attribute".into())
                })?;
                prev.values.push(value);
            } else {
                group.attributes.push(IppAttribute { name, values: vec![value] });
            }
        }
        Ok((IppMessage { version, code, request_id, groups }, r.pos))
    }

    /// All groups with this delimiter tag (`Get-Jobs` returns one job group per job).
    pub fn groups_with(&self, tag: u8) -> impl Iterator<Item = &IppGroup> {
        self.groups.iter().filter(move |g| g.tag == tag)
    }

    /// `status-message` plus the names in the `unsupported-attributes` group, formatted for an
    /// error text (" - message; unsupported: a, b").
    pub(crate) fn rejection_detail(&self) -> String {
        let mut detail = String::new();
        if let Some(m) = self.status_message().filter(|m| !m.is_empty()) {
            detail.push_str(&format!(" - {m}"));
        }
        let unsupported: Vec<&str> = self
            .groups_with(tag::UNSUPPORTED_ATTRIBUTES)
            .flat_map(|g| g.attributes.iter().map(|a| a.name.as_str()))
            .collect();
        if !unsupported.is_empty() {
            detail.push_str(&format!("; unsupported: {}", unsupported.join(", ")));
        }
        detail
    }
}

struct Reader<'a> {
    b: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], IppError> {
        let end = self.pos.checked_add(n).ok_or(IppError::Truncated)?;
        let s = self.b.get(self.pos..end).ok_or(IppError::Truncated)?;
        self.pos = end;
        Ok(s)
    }

    fn u8(&mut self) -> Result<u8, IppError> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16, IppError> {
        let b = self.take(2)?;
        Ok(u16::from_be_bytes([b[0], b[1]]))
    }

    fn u32(&mut self) -> Result<u32, IppError> {
        let b = self.take(4)?;
        Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }
}

fn be_i32(b: &[u8]) -> i32 {
    i32::from_be_bytes([b[0], b[1], b[2], b[3]])
}

fn lossy(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

/// `textWithLanguage` / `nameWithLanguage`: `[len][language][len][text]`. Returns the text.
fn with_language(data: &[u8]) -> Option<String> {
    let mut r = Reader { b: data, pos: 0 };
    let lang_len = r.u16().ok()? as usize;
    r.take(lang_len).ok()?;
    let text_len = r.u16().ok()? as usize;
    Some(lossy(r.take(text_len).ok()?))
}

fn format_datetime(d: &[u8]) -> String {
    let year = u16::from_be_bytes([d[0], d[1]]);
    let sign = if d[8] == b'-' { '-' } else { '+' };
    format!(
        "{year:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{}{sign}{:02}:{:02}",
        d[2], d[3], d[4], d[5], d[6], d[7], d[9], d[10]
    )
    .replace("+00:00", "Z")
}

fn decode_scalar(t: u8, data: &[u8]) -> IppValue {
    let other = || IppValue::Other { tag: t, data: data.to_vec() };
    match t {
        tag::INTEGER if data.len() == 4 => IppValue::Integer(be_i32(data)),
        tag::ENUM if data.len() == 4 => IppValue::Enum(be_i32(data)),
        tag::BOOLEAN if data.len() == 1 => IppValue::Boolean(data[0] != 0),
        tag::OCTET_STRING => IppValue::OctetString(data.to_vec()),
        tag::DATE_TIME if data.len() == 11 => IppValue::DateTime(format_datetime(data)),
        tag::RESOLUTION if data.len() == 9 => IppValue::Resolution {
            x: be_i32(&data[0..4]),
            y: be_i32(&data[4..8]),
            units: data[8] as i8,
        },
        tag::RANGE_OF_INTEGER if data.len() == 8 => {
            IppValue::Range { lower: be_i32(&data[0..4]), upper: be_i32(&data[4..8]) }
        }
        tag::TEXT_WITH_LANGUAGE => with_language(data).map_or_else(other, IppValue::Text),
        tag::NAME_WITH_LANGUAGE => with_language(data).map_or_else(other, IppValue::Name),
        tag::TEXT => IppValue::Text(lossy(data)),
        tag::NAME => IppValue::Name(lossy(data)),
        tag::KEYWORD => IppValue::Keyword(lossy(data)),
        tag::URI => IppValue::Uri(lossy(data)),
        tag::URI_SCHEME => IppValue::UriScheme(lossy(data)),
        tag::CHARSET => IppValue::Charset(lossy(data)),
        tag::NATURAL_LANGUAGE => IppValue::NaturalLanguage(lossy(data)),
        tag::MIME_MEDIA_TYPE => IppValue::MimeMediaType(lossy(data)),
        t => match OutOfBand::from_tag(t) {
            Some(o) => IppValue::OutOfBand(o),
            None => other(),
        },
    }
}

/// Read the value-length and value that follow a (already consumed) name for tag `t`.
fn decode_value(r: &mut Reader<'_>, t: u8, depth: usize) -> Result<IppValue, IppError> {
    let len = r.u16()? as usize;
    let data = r.take(len)?;
    if t == tag::BEG_COLLECTION {
        if depth >= MAX_DEPTH {
            return Err(IppError::Malformed("collections nested too deeply".into()));
        }
        return Ok(IppValue::Collection(decode_collection(r, depth + 1)?));
    }
    Ok(decode_scalar(t, data))
}

/// Members of a collection, after its `begCollection` value, up to `endCollection`.
fn decode_collection(
    r: &mut Reader<'_>,
    depth: usize,
) -> Result<Vec<(String, Vec<IppValue>)>, IppError> {
    let mut members: Vec<(String, Vec<IppValue>)> = Vec::new();
    loop {
        let t = r.u8()?;
        let name_len = r.u16()? as usize;
        r.take(name_len)?; // must be empty inside collections; ignored
        match t {
            tag::END_COLLECTION => {
                let len = r.u16()? as usize;
                r.take(len)?;
                return Ok(members);
            }
            tag::MEMBER_ATTR_NAME => {
                let len = r.u16()? as usize;
                members.push((lossy(r.take(len)?), Vec::new()));
            }
            t if t > 0x0F => {
                let value = decode_value(r, t, depth)?;
                members
                    .last_mut()
                    .ok_or_else(|| IppError::Malformed("collection value without a name".into()))?
                    .1
                    .push(value);
            }
            t => return Err(IppError::Malformed(format!("tag {t:#04x} inside a collection"))),
        }
    }
}

fn put(out: &mut Vec<u8>, t: u8, name: &str, data: &[u8]) {
    out.push(t);
    out.extend_from_slice(&(name.len() as u16).to_be_bytes());
    out.extend_from_slice(name.as_bytes());
    out.extend_from_slice(&(data.len() as u16).to_be_bytes());
    out.extend_from_slice(data);
}

fn encode_value(out: &mut Vec<u8>, name: &str, v: &IppValue) {
    match v {
        IppValue::Integer(i) => put(out, tag::INTEGER, name, &i.to_be_bytes()),
        IppValue::Enum(i) => put(out, tag::ENUM, name, &i.to_be_bytes()),
        IppValue::Boolean(b) => put(out, tag::BOOLEAN, name, &[u8::from(*b)]),
        IppValue::OctetString(b) => put(out, tag::OCTET_STRING, name, b),
        IppValue::DateTime(s) => put(out, tag::DATE_TIME, name, &encode_datetime(s)),
        IppValue::Resolution { x, y, units } => {
            let mut d = Vec::with_capacity(9);
            d.extend_from_slice(&x.to_be_bytes());
            d.extend_from_slice(&y.to_be_bytes());
            d.push(*units as u8);
            put(out, tag::RESOLUTION, name, &d);
        }
        IppValue::Range { lower, upper } => {
            let mut d = Vec::with_capacity(8);
            d.extend_from_slice(&lower.to_be_bytes());
            d.extend_from_slice(&upper.to_be_bytes());
            put(out, tag::RANGE_OF_INTEGER, name, &d);
        }
        IppValue::Collection(members) => {
            put(out, tag::BEG_COLLECTION, name, &[]);
            for (member, values) in members {
                put(out, tag::MEMBER_ATTR_NAME, "", member.as_bytes());
                for v in values {
                    encode_value(out, "", v);
                }
            }
            put(out, tag::END_COLLECTION, "", &[]);
        }
        IppValue::Text(s) => put(out, tag::TEXT, name, s.as_bytes()),
        IppValue::Name(s) => put(out, tag::NAME, name, s.as_bytes()),
        IppValue::Keyword(s) => put(out, tag::KEYWORD, name, s.as_bytes()),
        IppValue::Uri(s) => put(out, tag::URI, name, s.as_bytes()),
        IppValue::UriScheme(s) => put(out, tag::URI_SCHEME, name, s.as_bytes()),
        IppValue::Charset(s) => put(out, tag::CHARSET, name, s.as_bytes()),
        IppValue::NaturalLanguage(s) => put(out, tag::NATURAL_LANGUAGE, name, s.as_bytes()),
        IppValue::MimeMediaType(s) => put(out, tag::MIME_MEDIA_TYPE, name, s.as_bytes()),
        IppValue::OutOfBand(o) => put(out, o.tag(), name, &[]),
        IppValue::Other { tag, data } => put(out, *tag, name, data),
    }
}

/// Inverse of [`format_datetime`] for the formats it produces; anything else encodes as zeros.
fn encode_datetime(s: &str) -> Vec<u8> {
    let parse = || -> Option<Vec<u8>> {
        let s = s.strip_suffix('Z').map(|t| format!("{t}+00:00")).unwrap_or_else(|| s.to_string());
        let (date, rest) = s.split_once('T')?;
        let mut dp = date.split('-');
        let (y, mo, d) = (dp.next()?, dp.next()?, dp.next()?);
        let tz_at = rest.rfind(['+', '-'])?;
        let (time, tz) = rest.split_at(tz_at);
        let (hms, deci) = time.split_once('.').unwrap_or((time, "0"));
        let mut tp = hms.split(':');
        let (h, mi, se) = (tp.next()?, tp.next()?, tp.next()?);
        let (tzh, tzm) = tz[1..].split_once(':')?;
        let mut out = Vec::with_capacity(11);
        out.extend_from_slice(&y.parse::<u16>().ok()?.to_be_bytes());
        for v in [mo, d, h, mi, se, deci] {
            out.push(v.parse().ok()?);
        }
        out.push(tz.as_bytes()[0]);
        out.push(tzh.parse().ok()?);
        out.push(tzm.parse().ok()?);
        Some(out)
    };
    parse().unwrap_or_else(|| vec![0; 11])
}

// ---------------------------------------------------------------------------------------------
// Typed view
// ---------------------------------------------------------------------------------------------

/// `printer-state` (RFC 8011).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrinterState {
    Idle,
    Processing,
    Stopped,
    Unknown(i32),
}

impl PrinterState {
    fn from_enum(v: i32) -> PrinterState {
        match v {
            3 => PrinterState::Idle,
            4 => PrinterState::Processing,
            5 => PrinterState::Stopped,
            other => PrinterState::Unknown(other),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            PrinterState::Idle => "idle",
            PrinterState::Processing => "processing",
            PrinterState::Stopped => "stopped",
            PrinterState::Unknown(_) => "unknown",
        }
    }
}

impl Serialize for PrinterState {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

/// `print-quality-supported` entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrintQuality {
    Draft,
    Normal,
    High,
    Other(i32),
}

impl PrintQuality {
    fn from_enum(v: i32) -> PrintQuality {
        match v {
            3 => PrintQuality::Draft,
            4 => PrintQuality::Normal,
            5 => PrintQuality::High,
            other => PrintQuality::Other(other),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            PrintQuality::Draft => "draft",
            PrintQuality::Normal => "normal",
            PrintQuality::High => "high",
            PrintQuality::Other(_) => "other",
        }
    }
}

impl Serialize for PrintQuality {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

/// One marker (ink tank) from the parallel `marker-*` attributes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Marker {
    /// e.g. `Cyan`, `Black(PGBK)`.
    pub name: String,
    /// `#RRGGBB` swatch colour.
    pub color: Option<String>,
    /// e.g. `ink-cartridge`.
    pub kind: Option<String>,
    /// Percent remaining; `None` when the printer reports a negative "unknown" code.
    pub level: Option<u8>,
    pub low_level: Option<u8>,
    pub high_level: Option<u8>,
}

/// One `printer-supply` entry (`key=value;` pairs) with its description.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Supply {
    pub description: Option<String>,
    pub fields: BTreeMap<String, String>,
}

impl Supply {
    pub fn field(&self, key: &str) -> Option<&str> {
        self.fields.get(key).map(String::as_str)
    }
}

/// The attributes Washi reads from `Get-Printer-Attributes`. Everything is optional or a
/// possibly empty list: a printer may omit any of them.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct PrinterAttributes {
    pub state: Option<PrinterState>,
    pub state_reasons: Vec<String>,
    pub state_message: Option<String>,
    pub markers: Vec<Marker>,
    pub supplies: Vec<Supply>,
    pub firmware_version: Option<String>,
    pub make_and_model: Option<String>,
    pub media_sources: Vec<String>,
    pub media_types: Vec<String>,
    pub media_supported: Vec<String>,
    pub document_formats: Vec<String>,
    pub print_qualities: Vec<PrintQuality>,
    pub more_info: Option<String>,
    pub identify_actions: Vec<String>,
}

fn strings(g: &IppGroup, name: &str) -> Vec<String> {
    g.get(name)
        .map(|a| a.values.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
        .unwrap_or_default()
}

fn first_string(g: &IppGroup, name: &str) -> Option<String> {
    g.get(name)?.values.iter().find_map(|v| v.as_str().map(str::to_string))
}

fn ints(g: &IppGroup, name: &str) -> Vec<i32> {
    g.get(name).map(|a| a.values.iter().filter_map(IppValue::as_i32).collect()).unwrap_or_default()
}

/// A level code: 0..=100 is a percentage, negative values mean unknown / some left.
fn percent(v: Option<&i32>) -> Option<u8> {
    v.and_then(|v| u8::try_from(*v).ok()).filter(|v| *v <= 100)
}

fn parse_supply(raw: &str) -> BTreeMap<String, String> {
    raw.split(';')
        .filter_map(|kv| {
            let (k, v) = kv.split_once('=')?;
            let k = k.trim();
            (!k.is_empty()).then(|| (k.to_string(), v.trim().to_string()))
        })
        .collect()
}

impl PrinterAttributes {
    /// Build from a decoded `Get-Printer-Attributes` response. A non-successful IPP status is an
    /// error.
    pub fn from_message(msg: &IppMessage) -> Result<PrinterAttributes, IppError> {
        if msg.code >= 0x0100 {
            return Err(IppError::Status { code: msg.code, name: status_name(msg.code) });
        }
        let g = msg.group(tag::PRINTER_ATTRIBUTES).ok_or(IppError::NoPrinterAttributes)?;

        let names = strings(g, "marker-names");
        let colors = strings(g, "marker-colors");
        let kinds = strings(g, "marker-types");
        let levels = ints(g, "marker-levels");
        let lows = ints(g, "marker-low-levels");
        let highs = ints(g, "marker-high-levels");
        let markers = names
            .iter()
            .enumerate()
            .map(|(i, name)| Marker {
                name: name.clone(),
                color: colors.get(i).filter(|c| !c.is_empty()).cloned(),
                kind: kinds.get(i).filter(|c| !c.is_empty()).cloned(),
                level: percent(levels.get(i)),
                low_level: percent(lows.get(i)),
                high_level: percent(highs.get(i)),
            })
            .collect();

        let descriptions = strings(g, "printer-supply-description");
        let supplies = g
            .get("printer-supply")
            .map(|a| {
                a.values
                    .iter()
                    .enumerate()
                    .filter_map(|(i, v)| {
                        Some(Supply {
                            description: descriptions.get(i).cloned(),
                            fields: parse_supply(&v.as_text_lossy()?),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();

        Ok(PrinterAttributes {
            state: g
                .get("printer-state")
                .and_then(|a| a.values.first())
                .and_then(IppValue::as_i32)
                .map(PrinterState::from_enum),
            state_reasons: strings(g, "printer-state-reasons"),
            state_message: first_string(g, "printer-state-message").filter(|s| !s.is_empty()),
            markers,
            supplies,
            firmware_version: first_string(g, "printer-firmware-string-version")
                .filter(|s| !s.is_empty()),
            make_and_model: first_string(g, "printer-make-and-model"),
            media_sources: strings(g, "media-source-supported"),
            media_types: strings(g, "media-type-supported"),
            media_supported: strings(g, "media-supported"),
            document_formats: strings(g, "document-format-supported"),
            print_qualities: ints(g, "print-quality-supported")
                .into_iter()
                .map(PrintQuality::from_enum)
                .collect(),
            more_info: first_string(g, "printer-more-info"),
            identify_actions: strings(g, "identify-actions-supported"),
        })
    }
}

// ---------------------------------------------------------------------------------------------
// Client
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct IppOptions {
    pub port: u16,
    pub path: String,
    pub connect_timeout: Duration,
    /// Socket read/write timeout.
    pub io_timeout: Duration,
    /// Socket timeout while a `Print-Job` document is being sent and processed (the printer
    /// may sit on the connection while it digests a large raster).
    pub job_timeout: Duration,
}

impl Default for IppOptions {
    fn default() -> Self {
        IppOptions {
            port: DEFAULT_IPP_PORT,
            path: DEFAULT_IPP_PATH.to_string(),
            connect_timeout: Duration::from_secs(3),
            io_timeout: Duration::from_secs(10),
            job_timeout: Duration::from_secs(180),
        }
    }
}

/// IPP client: one request per HTTP connection.
#[derive(Debug, Clone)]
pub struct IppClient {
    host: String,
    opts: IppOptions,
}

impl IppClient {
    pub fn new(host: impl Into<String>) -> Self {
        Self::with_options(host, IppOptions::default())
    }

    pub fn with_options(host: impl Into<String>, opts: IppOptions) -> Self {
        IppClient { host: host.into(), opts }
    }

    pub fn options(&self) -> &IppOptions {
        &self.opts
    }

    /// `ipp://host:port/path`, the `printer-uri` operation attribute.
    pub fn printer_uri(&self) -> String {
        format!("ipp://{}:{}{}", uri_host(&self.host), self.opts.port, self.opts.path)
    }

    /// `charset`, `language` and `printer-uri`: the operation attributes every request starts
    /// with.
    pub(crate) fn base_operation_attributes(&self) -> Vec<IppAttribute> {
        self.base_operation_attributes_in("en")
    }

    /// The same for a request made in another natural language (`ja`): a printer words some
    /// of its answers, and the address of its strings file, for it.
    pub(crate) fn base_operation_attributes_in(&self, language: &str) -> Vec<IppAttribute> {
        vec![
            IppAttribute::single("attributes-charset", IppValue::Charset("utf-8".into())),
            IppAttribute::single(
                "attributes-natural-language",
                IppValue::NaturalLanguage(language.into()),
            ),
            IppAttribute::single("printer-uri", IppValue::Uri(self.printer_uri())),
        ]
    }

    /// The encoded `Get-Printer-Attributes` request body for `requested` attributes.
    pub fn encode_request(&self, requested: &[&str]) -> Vec<u8> {
        let mut ops = self.base_operation_attributes();
        if !requested.is_empty() {
            ops.push(IppAttribute::keywords("requested-attributes", requested));
        }
        IppMessage {
            version: (1, 1),
            code: OP_GET_PRINTER_ATTRIBUTES,
            request_id: 1,
            groups: vec![IppGroup::new(tag::OPERATION_ATTRIBUTES, ops)],
        }
        .encode()
    }

    /// POST one IPP message (plus an optional document that follows it) and decode the reply,
    /// whatever its IPP status. `io_timeout` is the socket timeout for this exchange.
    pub(crate) fn exchange(
        &self,
        msg: &IppMessage,
        document: Option<(&mut dyn Read, u64)>,
        io_timeout: Duration,
    ) -> Result<IppMessage, NetError> {
        let body = msg.encode();
        let ep = Endpoint {
            host: &self.host,
            port: self.opts.port,
            connect_timeout: self.opts.connect_timeout,
            io_timeout,
        };
        let headers = [
            ("Host", format!("{}:{}", uri_host(&self.host), self.opts.port)),
            ("Content-Type", "application/ipp".to_string()),
            ("Accept", "application/ipp".to_string()),
        ];
        let resp = request_once(ep, "POST", &self.opts.path, &headers, &body, document, MAX_BODY)?;
        if resp.status != 200 {
            return Err(NetError::Status { status: resp.status });
        }
        Ok(IppMessage::decode(&resp.body)?)
    }

    /// Send `Get-Printer-Attributes` and return the decoded reply, whatever its IPP status.
    pub fn get_printer_attributes_raw(&self, requested: &[&str]) -> Result<IppMessage, NetError> {
        let mut ops = self.base_operation_attributes();
        if !requested.is_empty() {
            ops.push(IppAttribute::keywords("requested-attributes", requested));
        }
        let msg = IppMessage {
            version: (1, 1),
            code: OP_GET_PRINTER_ATTRIBUTES,
            request_id: 1,
            groups: vec![IppGroup::new(tag::OPERATION_ATTRIBUTES, ops)],
        };
        self.exchange(&msg, None, self.opts.io_timeout)
    }

    /// `Get-Printer-Attributes` for `requested` (empty = the printer's default set).
    pub fn get_printer_attributes(
        &self,
        requested: &[&str],
    ) -> Result<PrinterAttributes, NetError> {
        let msg = self.get_printer_attributes_raw(requested)?;
        Ok(PrinterAttributes::from_message(&msg)?)
    }

    /// The attributes Washi shows ([`STATUS_ATTRIBUTES`]).
    pub fn status(&self) -> Result<PrinterAttributes, NetError> {
        self.get_printer_attributes(STATUS_ATTRIBUTES)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kw(s: &str) -> IppValue {
        IppValue::Keyword(s.into())
    }

    fn name(s: &str) -> IppValue {
        IppValue::Name(s.into())
    }

    /// A response shaped like the XK140's (values from a recorded `Get-Printer-Attributes`).
    pub(crate) fn sample_response() -> IppMessage {
        let attrs = vec![
            IppAttribute::single("printer-state", IppValue::Enum(3)),
            IppAttribute::single("printer-state-reasons", kw("none")),
            IppAttribute::single("printer-state-message", IppValue::Text(String::new())),
            IppAttribute::new(
                "marker-names",
                ["Magenta", "Black(BK)", "Yellow", "Black(PGBK)", "Cyan"].map(name).into(),
            ),
            IppAttribute::new(
                "marker-colors",
                ["#F200FF", "#000000", "#FFDA00", "#000000", "#00CFFF"].map(name).into(),
            ),
            IppAttribute::new("marker-types", vec![kw("ink-cartridge"); 5]),
            IppAttribute::new("marker-levels", [60, 80, 70, 60, 60].map(IppValue::Integer).into()),
            IppAttribute::new("marker-low-levels", vec![IppValue::Integer(15); 5]),
            IppAttribute::new("marker-high-levels", vec![IppValue::Integer(100); 5]),
            IppAttribute::new(
                "printer-supply",
                [
                    "type=inkCartridge;maxcapacity=100;level=-1;colorantname=magenta;",
                    "type=inkCartridge;maxcapacity=100;level=-1;colorantname=black;",
                ]
                .map(|s| IppValue::OctetString(s.as_bytes().to_vec()))
                .into(),
            ),
            IppAttribute::new(
                "printer-supply-description",
                ["Canon Magenta Ink Tank", "Canon Black Ink Tank"]
                    .map(|s| IppValue::Text(s.into()))
                    .into(),
            ),
            IppAttribute::single("printer-firmware-string-version", IppValue::Text("1.060".into())),
            IppAttribute::single(
                "printer-make-and-model",
                IppValue::Text("Canon XK140 series".into()),
            ),
            IppAttribute::keywords("media-source-supported", &["auto", "main", "rear", "disc"]),
            IppAttribute::keywords(
                "media-type-supported",
                &["stationery", "photographic", "com.canon-mtdisca"],
            ),
            IppAttribute::keywords("media-supported", &["iso_a4_210x297mm", "na_letter_8.5x11in"]),
            IppAttribute::new(
                "document-format-supported",
                ["application/octet-stream", "image/jpeg", "image/urf", "image/pwg-raster"]
                    .map(|s| IppValue::MimeMediaType(s.into()))
                    .into(),
            ),
            IppAttribute::new("print-quality-supported", [3, 4, 5].map(IppValue::Enum).into()),
            IppAttribute::single(
                "printer-more-info",
                IppValue::Uri("http://printer.example./index.html?page=PAGE_AAP".into()),
            ),
            IppAttribute::keywords("identify-actions-supported", &["flash", "sound"]),
        ];
        IppMessage {
            version: (1, 1),
            code: 0,
            request_id: 1,
            groups: vec![
                IppGroup::new(
                    tag::OPERATION_ATTRIBUTES,
                    vec![
                        IppAttribute::single(
                            "attributes-charset",
                            IppValue::Charset("utf-8".into()),
                        ),
                        IppAttribute::single(
                            "attributes-natural-language",
                            IppValue::NaturalLanguage("en".into()),
                        ),
                    ],
                ),
                IppGroup::new(tag::PRINTER_ATTRIBUTES, attrs),
            ],
        }
    }

    #[test]
    fn request_bytes_are_exact() {
        let c = IppClient::new("printer.local");
        let got = c.encode_request(&["printer-state", "marker-levels"]);
        let mut want: Vec<u8> = vec![0x01, 0x01, 0x00, 0x0B, 0, 0, 0, 1, 0x01];
        let mut put_attr = |t: u8, n: &str, v: &str| {
            want.push(t);
            want.extend((n.len() as u16).to_be_bytes());
            want.extend(n.as_bytes());
            want.extend((v.len() as u16).to_be_bytes());
            want.extend(v.as_bytes());
        };
        put_attr(0x47, "attributes-charset", "utf-8");
        put_attr(0x48, "attributes-natural-language", "en");
        put_attr(0x45, "printer-uri", "ipp://printer.local:631/ipp/print");
        put_attr(0x44, "requested-attributes", "printer-state");
        put_attr(0x44, "", "marker-levels");
        want.push(0x03);
        assert_eq!(got, want);
        // Operation id 0x000B = Get-Printer-Attributes, nothing else is encodable.
        assert_eq!(&got[2..4], &[0x00, 0x0B]);
    }

    #[test]
    fn printer_uri_formats_hosts() {
        assert_eq!(IppClient::new("a.local").printer_uri(), "ipp://a.local:631/ipp/print");
        assert_eq!(IppClient::new("fe80::1%eth0").printer_uri(), "ipp://[fe80::1]:631/ipp/print");
        let o = IppOptions { port: 8631, ..IppOptions::default() };
        assert_eq!(
            IppClient::with_options("192.0.2.5", o).printer_uri(),
            "ipp://192.0.2.5:8631/ipp/print"
        );
    }

    #[test]
    fn xk140_style_response_parses() {
        let bytes = sample_response().encode();
        let msg = IppMessage::decode(&bytes).unwrap();
        assert_eq!(msg, sample_response());
        let a = PrinterAttributes::from_message(&msg).unwrap();
        assert_eq!(a.state, Some(PrinterState::Idle));
        assert_eq!(a.state_reasons, ["none"]);
        assert_eq!(a.state_message, None, "empty text is dropped");
        assert_eq!(a.markers.len(), 5);
        let bk = &a.markers[1];
        assert_eq!(bk.name, "Black(BK)");
        assert_eq!(bk.color.as_deref(), Some("#000000"));
        assert_eq!(bk.kind.as_deref(), Some("ink-cartridge"));
        assert_eq!((bk.level, bk.low_level, bk.high_level), (Some(80), Some(15), Some(100)));
        assert_eq!(a.markers[4].name, "Cyan");
        assert_eq!(a.markers[4].level, Some(60));
        assert_eq!(a.supplies.len(), 2);
        assert_eq!(a.supplies[0].description.as_deref(), Some("Canon Magenta Ink Tank"));
        assert_eq!(a.supplies[0].field("colorantname"), Some("magenta"));
        assert_eq!(a.supplies[1].field("type"), Some("inkCartridge"));
        assert_eq!(a.firmware_version.as_deref(), Some("1.060"));
        assert_eq!(a.make_and_model.as_deref(), Some("Canon XK140 series"));
        assert_eq!(a.media_sources, ["auto", "main", "rear", "disc"]);
        assert!(a.media_types.contains(&"com.canon-mtdisca".to_string()));
        assert_eq!(a.media_supported.len(), 2);
        assert_eq!(a.document_formats[2], "image/urf");
        assert_eq!(
            a.print_qualities,
            [PrintQuality::Draft, PrintQuality::Normal, PrintQuality::High]
        );
        assert!(a.more_info.as_deref().unwrap().contains("PAGE_AAP"));
        assert_eq!(a.identify_actions, ["flash", "sound"]);
    }

    #[test]
    fn negative_and_missing_marker_levels_are_unknown() {
        let mut m = sample_response();
        let g = m.groups.iter_mut().find(|g| g.tag == tag::PRINTER_ATTRIBUTES).unwrap();
        for a in &mut g.attributes {
            if a.name == "marker-levels" {
                a.values = [-1, -3, 101, 0].map(IppValue::Integer).into();
            }
        }
        let a = PrinterAttributes::from_message(&m).unwrap();
        let levels: Vec<_> = a.markers.iter().map(|m| m.level).collect();
        assert_eq!(levels, [None, None, None, Some(0), None]);
    }

    #[test]
    fn error_status_and_missing_group() {
        let mut m = sample_response();
        m.code = 0x0400;
        let err = PrinterAttributes::from_message(&m).unwrap_err();
        assert_eq!(err, IppError::Status { code: 0x0400, name: "client-error-bad-request" });
        assert!(err.to_string().contains("0x0400"), "{err}");
        m.code = 0x0001; // successful-ok-ignored-or-substituted-attributes is still success
        assert!(PrinterAttributes::from_message(&m).is_ok());
        m.groups.retain(|g| g.tag != tag::PRINTER_ATTRIBUTES);
        assert_eq!(PrinterAttributes::from_message(&m), Err(IppError::NoPrinterAttributes));
    }

    #[test]
    fn every_value_type_round_trips_including_nested_collections() {
        let coll = IppValue::Collection(vec![
            (
                "media-size".into(),
                vec![IppValue::Collection(vec![
                    ("x-dimension".into(), vec![IppValue::Integer(21000)]),
                    ("y-dimension".into(), vec![IppValue::Integer(29700)]),
                ])],
            ),
            ("media-type".into(), vec![kw("stationery"), kw("photographic")]),
            ("media-source".into(), vec![kw("main")]),
        ]);
        let m = IppMessage {
            version: (2, 0),
            code: 0x0001,
            request_id: 0xDEAD_BEEF,
            groups: vec![
                IppGroup::new(
                    tag::OPERATION_ATTRIBUTES,
                    vec![IppAttribute::single(
                        "status-message",
                        IppValue::Text("successful-ok".into()),
                    )],
                ),
                IppGroup::new(
                    tag::PRINTER_ATTRIBUTES,
                    vec![
                        IppAttribute::single("b", IppValue::Boolean(true)),
                        IppAttribute::single("b2", IppValue::Boolean(false)),
                        IppAttribute::single("i", IppValue::Integer(-42)),
                        IppAttribute::single("e", IppValue::Enum(5)),
                        IppAttribute::single("o", IppValue::OctetString(vec![0, 1, 255])),
                        IppAttribute::single(
                            "d",
                            IppValue::DateTime("1970-01-02T23:55:25.0Z".into()),
                        ),
                        IppAttribute::single(
                            "r",
                            IppValue::Resolution { x: 600, y: 600, units: 3 },
                        ),
                        IppAttribute::single("g", IppValue::Range { lower: 16, upper: 19200 }),
                        IppAttribute::new("c", vec![coll.clone(), coll]),
                        IppAttribute::single("t", IppValue::Text("héllo".into())),
                        IppAttribute::single("n", name("x")),
                        IppAttribute::single("u", IppValue::Uri("ipp://h/p".into())),
                        IppAttribute::single("s", IppValue::UriScheme("ipp".into())),
                        IppAttribute::single("nl", IppValue::NaturalLanguage("en".into())),
                        IppAttribute::single("m", IppValue::MimeMediaType("image/urf".into())),
                        IppAttribute::single("nv", IppValue::OutOfBand(OutOfBand::NoValue)),
                        IppAttribute::single("un", IppValue::OutOfBand(OutOfBand::Unknown)),
                        IppAttribute::single("x", IppValue::Other { tag: 0x7F, data: vec![9] }),
                    ],
                ),
            ],
        };
        assert_eq!(IppMessage::decode(&m.encode()).unwrap(), m);
        assert_eq!(m.status_message().as_deref(), Some("successful-ok"));
    }

    #[test]
    fn language_tagged_text_and_name_are_decoded() {
        let mut bytes = vec![0x01, 0x01, 0, 0, 0, 0, 0, 1, 0x04];
        let mut put_lang = |t: u8, n: &str, lang: &str, text: &str| {
            bytes.push(t);
            bytes.extend((n.len() as u16).to_be_bytes());
            bytes.extend(n.as_bytes());
            let len = 2 + lang.len() + 2 + text.len();
            bytes.extend((len as u16).to_be_bytes());
            bytes.extend((lang.len() as u16).to_be_bytes());
            bytes.extend(lang.as_bytes());
            bytes.extend((text.len() as u16).to_be_bytes());
            bytes.extend(text.as_bytes());
        };
        put_lang(0x35, "printer-info", "en", "Hello");
        put_lang(0x36, "printer-name", "en", "Name");
        bytes.push(0x03);
        let m = IppMessage::decode(&bytes).unwrap();
        let g = m.group(tag::PRINTER_ATTRIBUTES).unwrap();
        assert_eq!(g.get("printer-info").unwrap().values, [IppValue::Text("Hello".into())]);
        assert_eq!(g.get("printer-name").unwrap().values, [name("Name")]);
    }

    #[test]
    fn data_after_end_of_attributes_is_ignored() {
        let mut bytes = sample_response().encode();
        bytes.extend_from_slice(b"%PDF-trailing document bytes");
        assert!(IppMessage::decode(&bytes).is_ok());
    }

    #[test]
    fn truncated_and_corrupt_input_never_panics() {
        let bytes = sample_response().encode();
        for cut in 0..bytes.len() {
            assert!(IppMessage::decode(&bytes[..cut]).is_err(), "prefix {cut} decoded");
        }
        // Bit flips: only require "no panic".
        for i in 0..bytes.len() {
            for flip in [0x01u8, 0x80, 0xFF] {
                let mut b = bytes.clone();
                b[i] ^= flip;
                let _ = IppMessage::decode(&b);
            }
        }
        // Attribute before any group, orphan additional value, bad collection nesting.
        assert!(IppMessage::decode(&[1, 1, 0, 0, 0, 0, 0, 1, 0x44, 0, 1, b'a', 0, 0]).is_err());
        let orphan = [1, 1, 0, 0, 0, 0, 0, 1, 0x04, 0x44, 0, 0, 0, 0, 0x03];
        assert!(IppMessage::decode(&orphan).is_err());
        let mut deep = vec![1, 1, 0, 0, 0, 0, 0, 1, 0x04, 0x34, 0, 1, b'c', 0, 0];
        for _ in 0..40 {
            deep.extend([0x4A, 0, 0, 0, 1, b'm', 0x34, 0, 0, 0, 0]);
        }
        assert!(matches!(IppMessage::decode(&deep), Err(IppError::Malformed(_))));
    }

    #[test]
    fn malformed_scalars_are_kept_not_fatal() {
        // An integer with a 2-byte payload decodes as `Other`, not an error.
        let bytes = [1, 1, 0, 0, 0, 0, 0, 1, 0x04, 0x21, 0, 1, b'i', 0, 2, 0, 5, 0x03];
        let m = IppMessage::decode(&bytes).unwrap();
        let v = &m.group(tag::PRINTER_ATTRIBUTES).unwrap().get("i").unwrap().values[0];
        assert_eq!(*v, IppValue::Other { tag: 0x21, data: vec![0, 5] });
    }

    #[test]
    fn supply_strings_parse_tolerantly() {
        let f = parse_supply("type=inkCartridge;maxcapacity=100;level=-1;colorantname=cyan;");
        assert_eq!(f["colorantname"], "cyan");
        assert_eq!(f["level"], "-1");
        assert!(parse_supply(";;=;novalue;").is_empty());
    }

    #[test]
    fn status_names() {
        assert_eq!(status_name(0), "successful-ok");
        assert_eq!(status_name(0x0507), "server-error-busy");
        assert_eq!(status_name(0x9999), "unknown");
    }

    #[test]
    fn datetime_format() {
        let raw = [0x07, 0xB2, 1, 2, 23, 55, 25, 0, b'+', 0, 0];
        assert_eq!(format_datetime(&raw), "1970-01-02T23:55:25.0Z");
        let m = IppMessage {
            version: (1, 1),
            code: 0,
            request_id: 1,
            groups: vec![IppGroup::new(
                tag::PRINTER_ATTRIBUTES,
                vec![IppAttribute::single(
                    "t",
                    IppValue::DateTime("2026-10-13T08:05:09.3-05:30".into()),
                )],
            )],
        };
        assert_eq!(IppMessage::decode(&m.encode()).unwrap(), m);
    }
}
