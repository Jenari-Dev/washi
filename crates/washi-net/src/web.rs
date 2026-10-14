// SPDX-License-Identifier: GPL-2.0-or-later
//! The printer's own web server: the links its Remote UI publishes.
//!
//! The Remote UI loads `GET /JS_MDL/model.js`, a small script of `var g_...` settings. Four of
//! them are web addresses the printer itself offers for the people who use it: the ink shop
//! (`g_ink_url`), the manual (`g_help_url`), the help page of an error (`g_err_url`, which ends in
//! `ERR=NO` where the support code belongs) and the maker's support page (`g_sup_url`). Washi
//! reads those, and nothing else, from the file instead of guessing addresses: they are specific
//! to the model, and they are what the printer's own page links to. This is a read of a public
//! page of the printer, so it needs no [`Confirm`](crate::Confirm); nothing is sent anywhere else.
//!
//! The values come from the network, so they are checked before they are used: only `http` and
//! `https` addresses with a plain host name, no whitespace or control characters, and at most
//! 2048 bytes.
//!
//! The same web server serves the printer's own words and picture, which IPP only names
//! (`printer-strings-uri`, `printer-icons`, see [`crate::ipp::localization`]):
//! [`fetch_strings`] reads one `.strings` file and [`fetch_icon`] the best of the pictures. Both
//! ask the printer Washi already talks to, for the *path* in the address the printer gave
//! ([`local_path`]); the host in that address is never followed.

use std::collections::BTreeMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::error::NetError;
use crate::http::{Endpoint, request_once, uri_host};
use crate::strings::{MAX_STRINGS_BYTES, parse_strings};

/// Path of the script on the printer's web server.
pub const MODEL_JS_PATH: &str = "/JS_MDL/model.js";

/// The model script is a few kilobytes; anything bigger is not it.
const MODEL_JS_MAX: usize = 256 * 1024;
const MAX_URL: usize = 2048;

/// The addresses the printer offers (each is `None` when the script lacks it or it is not a
/// plain web address).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ModelLinks {
    /// Where to buy ink for this model (`g_ink_url`).
    pub ink: Option<String>,
    /// The manual (`g_help_url`).
    pub help: Option<String>,
    /// Help for an error; its `ERR` parameter says `NO` until [`ModelLinks::error_url`] fills in a
    /// support code (`g_err_url`).
    pub error: Option<String>,
    /// The maker's support page (`g_sup_url`).
    pub support: Option<String>,
}

impl ModelLinks {
    /// True when the script offered nothing usable.
    pub fn is_empty(&self) -> bool {
        self.ink.is_none() && self.help.is_none() && self.error.is_none() && self.support.is_none()
    }

    /// Parse the text of `model.js`.
    pub fn parse(script: &str) -> ModelLinks {
        let get = |name: &str| js_string_var(script, name).and_then(|v| clean_url(&v));
        ModelLinks {
            ink: get("g_ink_url"),
            help: get("g_help_url"),
            error: get("g_err_url"),
            support: get("g_sup_url"),
        }
    }

    /// The error help address for `code`: the `ERR` parameter of `g_err_url` set to the code.
    /// `None` without an address or for a code that is not letters and digits.
    pub fn error_url(&self, code: &str) -> Option<String> {
        let code = code.trim();
        if code.is_empty() || code.len() > 8 || !code.bytes().all(|b| b.is_ascii_alphanumeric()) {
            return None;
        }
        Some(set_query_param(self.error.as_deref()?, "ERR", &code.to_ascii_uppercase(), false))
    }

    /// The same links with the `LNG` parameter (`EN`, `JP`) set to `lang` wherever the address
    /// has one: the printer words it in its own display language, the person may prefer another.
    pub fn with_language(&self, lang: &str) -> ModelLinks {
        let set = |u: &Option<String>| u.as_deref().map(|u| set_query_param(u, "LNG", lang, true));
        ModelLinks {
            ink: set(&self.ink),
            help: set(&self.help),
            error: set(&self.error),
            support: set(&self.support),
        }
    }
}

/// `http://host[:port]` of a printer's web server, for display and for the links below it. The
/// port is left out when it is 80.
pub fn origin(host: &str, port: u16) -> String {
    if port == 80 {
        format!("http://{}", uri_host(host))
    } else {
        format!("http://{}:{port}", uri_host(host))
    }
}

/// A page of the printer's web server from the relative link its capabilities give
/// (`index.html?page=PAGE_UTIL`). `None` when the link is not a plain relative address.
pub fn page_url(host: &str, port: u16, relative: &str) -> Option<String> {
    let rel = relative.trim();
    let rel = rel.strip_prefix('/').unwrap_or(rel);
    let plain = |b: u8| b.is_ascii_alphanumeric() || b"-._~/?=&%+:,;".contains(&b);
    if rel.is_empty()
        || rel.starts_with('/')
        || rel.len() > 256
        || rel.contains("..")
        || !rel.bytes().all(plain)
    {
        return None;
    }
    // "scheme:" would make it absolute.
    if rel.split(['/', '?']).next().is_some_and(|first| first.contains(':')) {
        return None;
    }
    Some(format!("{}/{rel}", origin(host, port)))
}

/// Timeouts and language of the web reads.
#[derive(Debug, Clone)]
pub struct WebOptions {
    pub connect_timeout: Duration,
    pub io_timeout: Duration,
    /// Sent as `Accept-Language`. **The printer needs one**: asked without a language (a plain
    /// `GET` with no such header) it answers `404 A specification file is not found`, as it does
    /// for any file of its Remote UI that depends on the language. It then words the addresses for
    /// that language (`LNG=JP` for `ja`).
    pub accept_language: String,
}

impl Default for WebOptions {
    fn default() -> Self {
        WebOptions {
            connect_timeout: Duration::from_secs(5),
            io_timeout: Duration::from_secs(10),
            accept_language: "en".to_string(),
        }
    }
}

/// `GET /JS_MDL/model.js` and parse the links in it (read-only).
pub fn fetch_model_links(host: &str, port: u16, opts: &WebOptions) -> Result<ModelLinks, NetError> {
    let ep =
        Endpoint { host, port, connect_timeout: opts.connect_timeout, io_timeout: opts.io_timeout };
    let headers = [
        ("Host", host_header(host, port)),
        ("Accept", "*/*".to_string()),
        ("Accept-Language", opts.accept_language.clone()),
    ];
    let r = request_once(ep, "GET", MODEL_JS_PATH, &headers, &[], None, MODEL_JS_MAX)?;
    if r.status != 200 {
        return Err(NetError::Status { status: r.status });
    }
    Ok(ModelLinks::parse(&String::from_utf8_lossy(&r.body)))
}

fn host_header(host: &str, port: u16) -> String {
    if port == 80 { uri_host(host) } else { format!("{}:{port}", uri_host(host)) }
}

// ---- the printer's words and picture ---------------------------------------------------------------

/// Longest path (with query) that is requested from the printer.
const MAX_LOCAL_PATH: usize = 512;
/// Largest picture accepted (the printer's own are 10 and 80 KB).
pub const MAX_ICON_BYTES: usize = 2 * 1024 * 1024;
/// Largest picture edge accepted, in pixels (the printer's own are 128 and 512).
pub const MAX_ICON_EDGE: u32 = 2048;
/// Pictures looked at when a printer lists several.
const MAX_ICON_CANDIDATES: usize = 4;

/// The path (and query) of an address from the printer's attributes, to be requested from the
/// printer's own web server: `http://<anything>/strings/ja.txt` and `/strings/ja.txt` both give
/// `/strings/ja.txt`. The host and port of the address are dropped on purpose: a printer cannot
/// send the app to another machine. `None` for any other scheme (no TLS here), a fragment, `..`,
/// or characters outside the plain URL set.
pub fn local_path(uri: &str) -> Option<String> {
    let uri = uri.trim();
    let path = if uri.get(..7).is_some_and(|p| p.eq_ignore_ascii_case("http://")) {
        let rest = &uri[7..];
        let at = rest.find('/').filter(|at| *at > 0)?;
        &rest[at..]
    } else if uri.starts_with('/') && !uri.starts_with("//") {
        uri
    } else {
        return None;
    };
    let plain = |b: u8| b.is_ascii_alphanumeric() || b"-._~/%+:,;=@?&".contains(&b);
    let bare = path.split('?').next().unwrap_or(path);
    if path.len() > MAX_LOCAL_PATH || !path.bytes().all(plain) || bare.contains("..") {
        return None;
    }
    Some(path.to_string())
}

fn get_file(
    host: &str,
    port: u16,
    path: &str,
    accept: &str,
    opts: &WebOptions,
    max: usize,
) -> Result<Vec<u8>, NetError> {
    // Whatever the caller got the path from, it is a plain path of the printer's own server.
    if local_path(path).as_deref() != Some(path) {
        return Err(NetError::Http(format!("not a path on the printer: {path:?}")));
    }
    let ep =
        Endpoint { host, port, connect_timeout: opts.connect_timeout, io_timeout: opts.io_timeout };
    let headers = [
        ("Host", host_header(host, port)),
        ("Accept", accept.to_string()),
        ("Accept-Language", opts.accept_language.clone()),
    ];
    let r = request_once(ep, "GET", path, &headers, &[], None, max)?;
    if r.status != 200 {
        return Err(NetError::Status { status: r.status });
    }
    Ok(r.body)
}

/// `GET <path>` (a path from [`crate::ipp::PrinterLocalization::strings_path`]) and parse the
/// `.strings` file (read-only). `opts.accept_language` is the language the file is wanted in.
pub fn fetch_strings(
    host: &str,
    port: u16,
    path: &str,
    opts: &WebOptions,
) -> Result<BTreeMap<String, String>, NetError> {
    let body = get_file(host, port, path, "text/plain, */*", opts, MAX_STRINGS_BYTES)?;
    Ok(parse_strings(&body)?)
}

/// A picture of the printer (PNG).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrinterIcon {
    pub png: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

/// Width and height from the header of a PNG; `None` when the bytes do not start like one or
/// the size is zero or above [`MAX_ICON_EDGE`].
pub fn png_size(bytes: &[u8]) -> Option<(u32, u32)> {
    const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    if bytes.len() < 33 || bytes[..8] != SIGNATURE || bytes[8..16] != *b"\0\0\0\x0dIHDR" {
        return None;
    }
    let w = u32::from_be_bytes(bytes[16..20].try_into().ok()?);
    let h = u32::from_be_bytes(bytes[20..24].try_into().ok()?);
    ((1..=MAX_ICON_EDGE).contains(&w) && (1..=MAX_ICON_EDGE).contains(&h)).then_some((w, h))
}

/// Candidates, the ones whose name says they are big first (`printer_icon_large.png`).
fn icons_by_promise(paths: &[String]) -> Vec<&String> {
    let rank = |p: &String| {
        let name = p.rsplit('/').next().unwrap_or(p).to_ascii_lowercase();
        if ["large", "big", "xl", "512"].iter().any(|w| name.contains(w)) {
            0
        } else if ["small", "thumb", "mini", "128", "64"].iter().any(|w| name.contains(w)) {
            2
        } else {
            1
        }
    };
    let mut v: Vec<&String> = paths.iter().take(MAX_ICON_CANDIDATES * 2).collect();
    v.sort_by_key(|p| rank(p));
    v.truncate(MAX_ICON_CANDIDATES);
    v
}

/// The largest of the printer's pictures at `paths` (read-only). A picture that cannot be read
/// or is not a PNG of a sensible size is skipped; the error is the last one when none is usable.
pub fn fetch_icon(
    host: &str,
    port: u16,
    paths: &[String],
    opts: &WebOptions,
) -> Result<PrinterIcon, NetError> {
    let mut best: Option<PrinterIcon> = None;
    let mut last = NetError::Http("the printer lists no picture".to_string());
    for path in icons_by_promise(paths) {
        let png = match get_file(host, port, path, "image/png, */*", opts, MAX_ICON_BYTES) {
            Ok(b) => b,
            Err(e) => {
                last = e;
                continue;
            }
        };
        let Some((width, height)) = png_size(&png) else {
            last = NetError::Icon("not a PNG of a usable size".to_string());
            continue;
        };
        if best.as_ref().is_none_or(|b| {
            u64::from(width) * u64::from(height) > u64::from(b.width) * u64::from(b.height)
        }) {
            best = Some(PrinterIcon { png, width, height });
        }
        // A picture of this size is the big one; the others are only smaller copies.
        if best.as_ref().is_some_and(|b| b.width >= 512) {
            break;
        }
    }
    best.ok_or(last)
}

// ---- parsing -------------------------------------------------------------------------------------

fn is_ident(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'$'
}

/// The value of `var <name> = '...';` (single or double quotes, JavaScript escapes resolved).
fn js_string_var(script: &str, name: &str) -> Option<String> {
    let bytes = script.as_bytes();
    for (at, _) in script.match_indices(name) {
        if at > 0 && is_ident(bytes[at - 1]) {
            continue;
        }
        let mut i = at + name.len();
        if bytes.get(i).is_some_and(|b| is_ident(*b)) {
            continue;
        }
        let skip_space = |mut i: usize| {
            while bytes.get(i).is_some_and(|b| *b == b' ' || *b == b'\t') {
                i += 1;
            }
            i
        };
        i = skip_space(i);
        if bytes.get(i) != Some(&b'=') {
            continue;
        }
        i = skip_space(i + 1);
        let Some(&quote) = bytes.get(i).filter(|q| **q == b'\'' || **q == b'"') else { continue };
        if let Some(v) = read_js_string(&script[i + 1..], quote as char) {
            return Some(v);
        }
    }
    None
}

/// The contents of a JavaScript string whose opening quote was just consumed; `None` when it is
/// not closed on the same line.
fn read_js_string(rest: &str, quote: char) -> Option<String> {
    let mut out = String::new();
    let mut chars = rest.chars();
    while let Some(c) = chars.next() {
        match c {
            c if c == quote => return Some(out),
            '\n' | '\r' => return None,
            '\\' => match chars.next()? {
                'n' => out.push('\n'),
                't' => out.push('\t'),
                'x' => {
                    let hex: String = chars.by_ref().take(2).collect();
                    out.push(char::from_u32(u32::from_str_radix(&hex, 16).ok()?)?);
                }
                'u' => {
                    let hex: String = chars.by_ref().take(4).collect();
                    out.push(char::from_u32(u32::from_str_radix(&hex, 16).ok()?)?);
                }
                other => out.push(other),
            },
            c => out.push(c),
        }
    }
    None
}

/// What the Remote UI's own `urlDecode` does to these values: the HTML entities.
fn unescape_html(s: &str) -> String {
    s.replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&#x5c;", "\\")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&#38;", "&")
        .replace("&amp;", "&")
}

/// The address if it is a plain `http(s)` address that is safe to hand to a browser.
fn clean_url(raw: &str) -> Option<String> {
    let url = unescape_html(raw.trim());
    let lower = url.to_ascii_lowercase();
    let rest = lower.strip_prefix("https://").or_else(|| lower.strip_prefix("http://"))?;
    if url.len() > MAX_URL || !url.bytes().all(|b| b.is_ascii_graphic()) {
        return None;
    }
    if url.bytes().any(|b| matches!(b, b'"' | b'<' | b'>' | b'\\' | b'`' | b'{' | b'}' | b'|')) {
        return None;
    }
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host = authority.rsplit_once(':').map_or(authority, |(h, p)| {
        if p.bytes().all(|b| b.is_ascii_digit()) { h } else { authority }
    });
    // No user information (`a@b`), and a real host name.
    if host.is_empty()
        || authority.contains('@')
        || !host.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
        || !host.bytes().any(|b| b.is_ascii_alphabetic())
    {
        return None;
    }
    Some(url)
}

/// Set (or, with `only_if_present`, replace) one query parameter.
fn set_query_param(url: &str, key: &str, value: &str, only_if_present: bool) -> String {
    let (base, fragment) = match url.split_once('#') {
        Some((b, f)) => (b, Some(f)),
        None => (url, None),
    };
    let (path, query) = base.split_once('?').unwrap_or((base, ""));
    let mut found = false;
    let mut params: Vec<String> = query
        .split('&')
        .filter(|p| !p.is_empty())
        .map(|p| {
            if p.split('=').next() == Some(key) {
                found = true;
                format!("{key}={value}")
            } else {
                p.to_string()
            }
        })
        .collect();
    if !found && !only_if_present {
        params.push(format!("{key}={value}"));
    }
    let mut out = path.to_string();
    if !params.is_empty() {
        out.push('?');
        out.push_str(&params.join("&"));
    }
    if let Some(f) = fragment {
        out.push('#');
        out.push_str(f);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = include_str!("../tests/fixtures/model-sample.js");

    #[test]
    fn reads_the_four_links_and_nothing_else() {
        let l = ModelLinks::parse(SAMPLE);
        assert_eq!(
            l.ink.as_deref(),
            Some("https://links.example.org/go?KIND=INK&RES=1&DEV=Sample+series&LNG=EN")
        );
        assert_eq!(
            l.help.as_deref(),
            Some(
                "https://links.example.org/go?KIND=HELP&RES=1&DEV=Sample+series&LNG=EN&PAGE=a%2Fb.html"
            )
        );
        assert_eq!(
            l.error.as_deref(),
            Some("https://links.example.org/go?KIND=ERR&RES=1&DEV=Sample+series&LNG=EN&ERR=NO")
        );
        assert_eq!(l.support.as_deref(), Some("http://example.org/support/"));
        assert!(!l.is_empty());
    }

    #[test]
    fn the_error_address_takes_the_support_code() {
        let l = ModelLinks::parse(SAMPLE);
        let u = l.error_url("1300").unwrap();
        assert!(u.ends_with("&ERR=1300"), "{u}");
        assert!(l.error_url("d109").unwrap().ends_with("&ERR=D109"));
        assert_eq!(l.error_url(""), None);
        assert_eq!(l.error_url("13 00"), None);
        assert_eq!(l.error_url("1300&x=y"), None);
        // A parameter in the middle is replaced; a missing one is appended.
        let mid = ModelLinks {
            error: Some("https://a.example/x?ERR=NO&LNG=EN".into()),
            ..Default::default()
        };
        assert_eq!(mid.error_url("1600").unwrap(), "https://a.example/x?ERR=1600&LNG=EN");
        let none = ModelLinks { error: Some("https://a.example/x".into()), ..Default::default() };
        assert_eq!(none.error_url("1600").unwrap(), "https://a.example/x?ERR=1600");
        assert_eq!(ModelLinks::default().error_url("1600"), None);
    }

    #[test]
    fn the_language_parameter_follows_the_app() {
        let l = ModelLinks::parse(SAMPLE).with_language("JP");
        assert!(l.ink.as_deref().unwrap().contains("&LNG=JP"));
        assert!(l.error.as_deref().unwrap().ends_with("&ERR=NO"), "only LNG changes");
        // An address without LNG is not given one.
        assert_eq!(l.support.as_deref(), Some("http://example.org/support/"));
    }

    #[test]
    fn script_escapes_and_entities() {
        let js = r#"var g_ink_url = "http://a.example/p?x=1&y=2\x26z=3&#38;w=4&amp;v=5\/q";"#;
        assert_eq!(
            ModelLinks::parse(js).ink.as_deref(),
            Some("http://a.example/p?x=1&y=2&z=3&w=4&v=5/q")
        );
        let js = "var g_ink_url='https://a.example/' ;var g_help_url\t=\t'https://b.example/'";
        let l = ModelLinks::parse(js);
        assert_eq!(l.ink.as_deref(), Some("https://a.example/"));
        assert_eq!(l.help.as_deref(), Some("https://b.example/"));
        // Not a variable of that name.
        assert!(
            ModelLinks::parse(
                "var xg_ink_url = 'https://a.example/'; var g_ink_url2 = 'https://b.example/';"
            )
            .is_empty()
        );
    }

    #[test]
    fn unsafe_values_are_dropped() {
        for bad in [
            "javascript:alert(1)",
            "file:///etc/passwd",
            "ftp://a.example/",
            "dummy",
            "",
            "https://",
            "https:///path",
            "https://user@evil.example/",
            "https://good.example@evil.example/x",
            "https://a.example/with space",
            "https://a.example/\"quote",
            "https://a.example/<x>",
            "https://a.example/back\\\\slash",
            "https://203.0.113.7/",
            "https://ex ample.org/",
        ] {
            let js = format!("var g_ink_url = '{bad}';");
            assert_eq!(ModelLinks::parse(&js).ink, None, "{bad:?}");
        }
        let long = format!("var g_ink_url = 'https://a.example/{}';", "x".repeat(MAX_URL));
        assert_eq!(ModelLinks::parse(&long).ink, None);
        // A value with a raw line break is not a string.
        assert!(ModelLinks::parse("var g_ink_url = 'https://a.example/\n';").is_empty());
        // Ports are fine.
        let ok = ModelLinks::parse("var g_ink_url = 'http://a.example:8080/x';");
        assert_eq!(ok.ink.as_deref(), Some("http://a.example:8080/x"));
    }

    #[test]
    fn pages_of_the_printer_from_relative_links() {
        assert_eq!(
            page_url("printer.local", 80, "index.html?page=PAGE_UTIL").as_deref(),
            Some("http://printer.local/index.html?page=PAGE_UTIL")
        );
        assert_eq!(
            page_url("127.0.0.1", 18080, "/index.html").as_deref(),
            Some("http://127.0.0.1:18080/index.html")
        );
        assert_eq!(
            page_url("fe80::1%eth0", 80, "x.html").as_deref(),
            Some("http://[fe80::1]/x.html")
        );
        for bad in
            ["", "http://evil.example/", "//evil.example/", "javascript:x", "../x", "a b", "a<b"]
        {
            assert_eq!(page_url("p.local", 80, bad), None, "{bad:?}");
        }
        assert_eq!(origin("p.local", 80), "http://p.local");
        assert_eq!(origin("p.local", 8080), "http://p.local:8080");
    }

    /// The first 33 bytes of a PNG of this size (all the size check looks at).
    pub(crate) fn png_head(w: u32, h: u32) -> Vec<u8> {
        let mut b = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 13];
        b.extend(b"IHDR");
        b.extend(w.to_be_bytes());
        b.extend(h.to_be_bytes());
        b.extend([8, 6, 0, 0, 0, 0, 0, 0, 0]);
        b
    }

    #[test]
    fn only_the_path_of_an_address_is_kept() {
        assert_eq!(
            local_path("http://printer.example./strings/ja.txt").as_deref(),
            Some("/strings/ja.txt")
        );
        assert_eq!(
            local_path("HTTP://203.0.113.9:8080/icon/a.png?v=2").as_deref(),
            Some("/icon/a.png?v=2")
        );
        assert_eq!(local_path("  /strings/en.txt ").as_deref(), Some("/strings/en.txt"));
        assert_eq!(local_path("http://[2001:db8::1]/x/y.png").as_deref(), Some("/x/y.png"));
        for bad in [
            "",
            "http://printer.example.",
            "http://printer.example",
            "http:///no-host",
            "https://printer.example./strings/ja.txt",
            "ftp://printer.example./strings/ja.txt",
            "//printer.example./strings/ja.txt",
            "strings/ja.txt",
            "http://p/../etc/passwd",
            "/a/../b",
            "/a/b#frag",
            "/a b",
            "/a\r\nHost: evil",
            "/\u{65e5}\u{672c}.txt",
            "/a\"b",
        ] {
            assert_eq!(local_path(bad), None, "{bad:?}");
        }
        assert_eq!(local_path(&format!("/{}", "a".repeat(MAX_LOCAL_PATH))), None);
        assert!(local_path(&format!("/{}", "a".repeat(MAX_LOCAL_PATH - 1))).is_some());
    }

    #[test]
    fn png_sizes_are_read_from_the_header() {
        assert_eq!(png_size(&png_head(512, 256)), Some((512, 256)));
        assert_eq!(png_size(&png_head(0, 10)), None);
        assert_eq!(png_size(&png_head(10, MAX_ICON_EDGE + 1)), None);
        assert_eq!(
            png_size(&png_head(MAX_ICON_EDGE, MAX_ICON_EDGE)),
            Some((MAX_ICON_EDGE, MAX_ICON_EDGE))
        );
        assert_eq!(png_size(b"GIF89a"), None);
        assert_eq!(png_size(&png_head(8, 8)[..20]), None);
        let mut not_ihdr = png_head(8, 8);
        not_ihdr[12..16].copy_from_slice(b"IDAT");
        assert_eq!(png_size(&not_ihdr), None);
    }

    #[test]
    fn the_big_looking_pictures_are_tried_first() {
        let p = |s: &str| s.to_string();
        let paths =
            [p("/i/small.png"), p("/i/icon.png"), p("/i/icon_large.png"), p("/i/x_128.png")];
        let order: Vec<&str> = icons_by_promise(&paths).into_iter().map(String::as_str).collect();
        assert_eq!(order, ["/i/icon_large.png", "/i/icon.png", "/i/small.png", "/i/x_128.png"]);
        let many: Vec<String> = (0..20).map(|i| format!("/i/{i}.png")).collect();
        assert_eq!(icons_by_promise(&many).len(), MAX_ICON_CANDIDATES);
    }
}
