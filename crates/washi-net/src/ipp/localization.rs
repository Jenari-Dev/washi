// SPDX-License-Identifier: GPL-2.0-or-later
//! What a printer says about its own words and picture (IPP Everywhere, PWG 5100.13):
//!
//! * `printer-strings-uri`: a `.strings` file with the printer's display names for the keywords it
//!   uses, for the language of the request (`attributes-natural-language`),
//! * `printer-strings-languages-supported`: the languages it has such files for,
//! * `printer-icons`: pictures of the printer (PNG, 128 and 512 pixels on Canon's printers).
//!
//! `Get-Printer-Attributes` for these is a read. The addresses the printer returns are only used
//! for their *path*: the app then asks the printer it already talks to (see
//! [`crate::web::local_path`]), never a host the reply names.

use serde::Serialize;

use super::{
    IppAttribute, IppClient, IppError, IppGroup, IppMessage, OP_GET_PRINTER_ATTRIBUTES,
    status_name, tag,
};
use crate::error::NetError;
use crate::web::local_path;

/// The attributes asked for: the strings and picture addresses, the languages, and the firmware
/// version (a new firmware may come with new words and a new picture).
pub const LOCALIZATION_ATTRIBUTES: &[&str] = &[
    "printer-strings-uri",
    "printer-strings-languages-supported",
    "printer-icons",
    "printer-firmware-string-version",
];

/// A language tag as it goes into a request and a file name: letters, digits and hyphens, in
/// lower case (`en`, `ja`, `zh-cn`). `None` for anything else.
pub fn normalize_language(tag: &str) -> Option<String> {
    let t = tag.trim().replace('_', "-").to_ascii_lowercase();
    let ok = !t.is_empty()
        && t.len() <= 35
        && !t.starts_with('-')
        && !t.ends_with('-')
        && !t.contains("--")
        && t.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    ok.then_some(t)
}

/// The first subtag (`zh-cn` -> `zh`).
fn primary(tag: &str) -> &str {
    tag.split('-').next().unwrap_or(tag)
}

/// The localisation attributes of one printer, as asked for one language.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct PrinterLocalization {
    /// The language the request was made in (normalised).
    pub language: String,
    /// `printer-strings-uri` as the printer wrote it (`None` when it has none for the language).
    pub strings_uri: Option<String>,
    /// `printer-strings-languages-supported`.
    pub languages_supported: Vec<String>,
    /// `printer-icons`, as the printer wrote them.
    pub icons: Vec<String>,
    pub firmware_version: Option<String>,
}

impl PrinterLocalization {
    /// Build from a decoded `Get-Printer-Attributes` reply to a request made in `language`.
    /// A printer that has none of the attributes gives an empty value, not an error.
    pub fn from_message(msg: &IppMessage, language: &str) -> Result<PrinterLocalization, IppError> {
        if msg.code >= 0x0100 {
            return Err(IppError::Status { code: msg.code, name: status_name(msg.code) });
        }
        let g = msg.group(tag::PRINTER_ATTRIBUTES).ok_or(IppError::NoPrinterAttributes)?;
        let strings = |name: &str| -> Vec<String> {
            g.get(name)
                .map(|a| {
                    a.values
                        .iter()
                        .filter_map(|v| v.as_str())
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect()
                })
                .unwrap_or_default()
        };
        Ok(PrinterLocalization {
            language: normalize_language(language).unwrap_or_default(),
            strings_uri: strings("printer-strings-uri").into_iter().next(),
            languages_supported: strings("printer-strings-languages-supported"),
            icons: strings("printer-icons"),
            firmware_version: strings("printer-firmware-string-version").into_iter().next(),
        })
    }

    /// Does the printer list `language` (exactly, or by its first subtag when `language` has
    /// none: `ja` for `ja-jp`)? Returns the way the printer spells it.
    fn supported_spelling(&self, language: &str) -> Option<&str> {
        let want = normalize_language(language)?;
        let has_region = want.contains('-');
        let norm = |s: &str| normalize_language(s).unwrap_or_default();
        self.languages_supported
            .iter()
            .find(|s| norm(s) == want)
            .or_else(|| {
                self.languages_supported.iter().find(|s| !has_region && primary(&norm(s)) == want)
            })
            .map(String::as_str)
    }

    /// The path of the strings file for `language` on the printer's own web server, or `None` when
    /// the printer has no file in that language.
    ///
    /// The address the printer returns for the language is used when its file name is the
    /// language (`/strings/ja.txt`). A printer that gives one address whatever the language is
    /// asked for (its file is in another language, or has no language in its name) is used
    /// through the language list instead: when `language` is among
    /// `printer-strings-languages-supported`, the file is `<directory of the address>/<language>.<extension>`.
    /// A language that is not listed has no file.
    pub fn strings_path(&self, language: &str) -> Option<String> {
        let want = normalize_language(language)?;
        let uri = self.strings_uri.as_deref()?;
        let path = local_path(uri)?;
        // The query string of the address does not belong to a name made from the language list.
        let bare = path.split_once('?').map_or(path.as_str(), |(p, _)| p);
        let (dir, file) = bare.rsplit_once('/')?;
        let (stem, ext) = file.split_once('.').unwrap_or((file, "txt"));
        let stem = normalize_language(stem).unwrap_or_default();
        let named = stem == want || (!want.contains('-') && primary(&stem) == want);
        if named {
            return Some(path);
        }
        let spelled = self.supported_spelling(&want)?;
        Some(format!("{dir}/{spelled}.{ext}"))
    }

    /// The paths of the printer's pictures on its own web server.
    pub fn icon_paths(&self) -> Vec<String> {
        self.icons.iter().filter_map(|u| local_path(u)).collect()
    }
}

impl IppClient {
    /// `Get-Printer-Attributes` for the strings and picture addresses, the languages the printer
    /// has strings in, and its firmware version, asked in `language` (a read).
    pub fn localization(&self, language: &str) -> Result<PrinterLocalization, NetError> {
        let lang = normalize_language(language)
            .ok_or_else(|| IppError::BadLanguage { tag: language.chars().take(40).collect() })?;
        let mut ops = self.base_operation_attributes_in(&lang);
        ops.push(IppAttribute::keywords("requested-attributes", LOCALIZATION_ATTRIBUTES));
        let msg = IppMessage {
            version: (1, 1),
            code: OP_GET_PRINTER_ATTRIBUTES,
            request_id: 1,
            groups: vec![IppGroup::new(tag::OPERATION_ATTRIBUTES, ops)],
        };
        let reply = self.exchange(&msg, None, self.opts.io_timeout)?;
        Ok(PrinterLocalization::from_message(&reply, &lang)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipp::IppValue;

    fn loc(uri: Option<&str>, supported: &[&str]) -> PrinterLocalization {
        PrinterLocalization {
            language: "ja".into(),
            strings_uri: uri.map(str::to_string),
            languages_supported: supported.iter().map(|s| (*s).to_string()).collect(),
            icons: vec![
                "http://printer.example./icon/small.png".into(),
                "http://printer.example./icon/large.png".into(),
            ],
            firmware_version: Some("9.9".into()),
        }
    }

    fn reply(attrs: Vec<IppAttribute>) -> IppMessage {
        IppMessage {
            version: (1, 1),
            code: 0,
            request_id: 1,
            groups: vec![IppGroup::new(tag::PRINTER_ATTRIBUTES, attrs)],
        }
    }

    #[test]
    fn language_tags_are_normalised_or_refused() {
        assert_eq!(normalize_language("ja").as_deref(), Some("ja"));
        assert_eq!(normalize_language(" ZH_cn ").as_deref(), Some("zh-cn"));
        for bad in ["", "-", "ja-", "-ja", "a--b", "ja/../x", "ja\r\nX: y", "日本語", "a b"] {
            assert_eq!(normalize_language(bad), None, "{bad:?}");
        }
        assert_eq!(normalize_language(&"a".repeat(36)), None);
    }

    #[test]
    fn the_attributes_are_read_from_a_reply() {
        let m = reply(vec![
            IppAttribute::single(
                "printer-strings-uri",
                IppValue::Uri("http://printer.example./strings/ja.txt".into()),
            ),
            IppAttribute::new(
                "printer-strings-languages-supported",
                ["ja", "en", "zh-cn"].map(|s| IppValue::Keyword(s.into())).into(),
            ),
            IppAttribute::new(
                "printer-icons",
                ["http://printer.example./icon/a.png", "http://printer.example./icon/b.png"]
                    .map(|s| IppValue::Uri(s.into()))
                    .into(),
            ),
            IppAttribute::single("printer-firmware-string-version", IppValue::Text(" 1.2 ".into())),
        ]);
        let l = PrinterLocalization::from_message(&m, "JA").unwrap();
        assert_eq!(l.language, "ja");
        assert_eq!(l.strings_uri.as_deref(), Some("http://printer.example./strings/ja.txt"));
        assert_eq!(l.languages_supported, ["ja", "en", "zh-cn"]);
        assert_eq!(l.icon_paths(), ["/icon/a.png", "/icon/b.png"]);
        assert_eq!(l.firmware_version.as_deref(), Some("1.2"));
        assert_eq!(l.strings_path("ja").as_deref(), Some("/strings/ja.txt"));
    }

    #[test]
    fn a_printer_without_any_of_them_gives_an_empty_value() {
        let l = PrinterLocalization::from_message(&reply(vec![]), "en").unwrap();
        assert_eq!(l.strings_uri, None);
        assert!(l.languages_supported.is_empty() && l.icons.is_empty());
        assert_eq!(l.strings_path("en"), None);
        // An empty uri value (a language the printer has nothing for) is none.
        let m =
            reply(vec![IppAttribute::single("printer-strings-uri", IppValue::Uri(String::new()))]);
        assert_eq!(PrinterLocalization::from_message(&m, "xx").unwrap().strings_uri, None);
        // An error status is an error; so is a reply without printer attributes.
        let mut bad = reply(vec![]);
        bad.code = 0x0400;
        assert!(PrinterLocalization::from_message(&bad, "en").is_err());
        bad.code = 0;
        bad.groups.clear();
        assert_eq!(
            PrinterLocalization::from_message(&bad, "en"),
            Err(IppError::NoPrinterAttributes)
        );
    }

    #[test]
    fn the_address_that_follows_the_language_is_used_as_it_is() {
        let l = loc(Some("http://printer.example./strings/ja.txt"), &["en", "ja"]);
        assert_eq!(l.strings_path("ja").as_deref(), Some("/strings/ja.txt"));
        // A regional file name still is the language.
        let l = loc(Some("http://printer.example./s/ja-JP.strings"), &[]);
        assert_eq!(l.strings_path("ja").as_deref(), Some("/s/ja-JP.strings"));
        // The host in the address is never used.
        let l = loc(Some("http://elsewhere.example:8080/strings/ja.txt"), &[]);
        assert_eq!(l.strings_path("ja").as_deref(), Some("/strings/ja.txt"));
    }

    #[test]
    fn an_address_that_does_not_change_with_the_language_is_derived_from_the_list() {
        // The printer answered a Japanese request with its English file.
        let l = loc(Some("http://printer.example./strings/en.txt"), &["en", "ja", "ZH-CN"]);
        assert_eq!(l.strings_path("ja").as_deref(), Some("/strings/ja.txt"));
        assert_eq!(l.strings_path("zh-cn").as_deref(), Some("/strings/ZH-CN.txt"), "its spelling");
        assert_eq!(l.strings_path("en").as_deref(), Some("/strings/en.txt"));
        // Not listed: no file, whatever the address says.
        assert_eq!(l.strings_path("fr"), None);
        // The extension follows the address; a query string is not carried over.
        let l = loc(Some("http://printer.example./s/en.strings?v=3"), &["en", "ja"]);
        assert_eq!(l.strings_path("ja").as_deref(), Some("/s/ja.strings"));
        // A file whose name says nothing about a language is used only through the list.
        let l = loc(Some("http://printer.example./s/printer.txt"), &["ja"]);
        assert_eq!(l.strings_path("ja").as_deref(), Some("/s/ja.txt"));
        assert_eq!(l.strings_path("en"), None);
        // A region matches the listed language only when it is listed itself.
        let l = loc(Some("http://printer.example./s/en.txt"), &["ja", "en"]);
        assert_eq!(l.strings_path("ja-jp"), None);
        let l = loc(Some("http://printer.example./s/en.txt"), &["ja-jp"]);
        assert_eq!(l.strings_path("ja").as_deref(), Some("/s/ja-jp.txt"));
    }

    #[test]
    fn unusable_addresses_give_no_file() {
        for uri in ["https://printer.example./strings/ja.txt", "ftp://x/ja.txt", "ja.txt", ""] {
            let l = loc(Some(uri), &["ja"]);
            assert_eq!(l.strings_path("ja"), None, "{uri:?}");
        }
        assert_eq!(loc(Some("http://p./a/ja.txt"), &["ja"]).strings_path("not a tag"), None);
    }
}
