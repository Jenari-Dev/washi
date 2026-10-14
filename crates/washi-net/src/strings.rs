// SPDX-License-Identifier: GPL-2.0-or-later
//! The `.strings` files of IPP Everywhere (PWG 5100.13 "printer-strings"): the printer's own
//! display names for the keywords it uses (`media-type.<keyword>`, `media-source.<keyword>`, ...),
//! worded for one language.
//!
//! The format is Apple's: `"key" = "value";` lines, `/* ... */` and `// ...` comments, the
//! escapes `\"`, `\\`, `\n`, `\t`, `\r` and `\Uxxxx`, and UTF-8 or UTF-16 (with a byte order
//! mark; UTF-16 without one is recognised by its zero bytes).
//!
//! The file comes from the network, so what is kept is limited: at most [`MAX_ENTRIES`] entries,
//! keys of plain ASCII, values of at most [`MAX_VALUE_CHARS`] characters, control and
//! bidirectional-formatting characters replaced or dropped, so that a value can be shown in a
//! one-line label as it is. A syntax error costs the entry it is in, not the whole file.

use std::collections::BTreeMap;

use thiserror::Error;

/// A strings file is a few kilobytes; anything bigger than this is not one.
pub const MAX_STRINGS_BYTES: usize = 512 * 1024;
/// Entries kept from one file.
pub const MAX_ENTRIES: usize = 4096;
/// Longest key (`media-type.com.canon-mtgr` is 26).
pub const MAX_KEY_CHARS: usize = 160;
/// Longest value: it is a name for a list, not a paragraph.
pub const MAX_VALUE_CHARS: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum StringsError {
    #[error("the file is too large to be a strings file ({0} bytes)")]
    TooLarge(usize),
    #[error("the file is not valid text")]
    Encoding,
    #[error("line {line}: not a \"key\" = \"value\"; entry")]
    Syntax { line: usize },
}

/// Text of the file as characters: UTF-8 (a BOM is dropped) or UTF-16 (BOM, or recognised by the
/// zero bytes of its first ASCII characters). Invalid sequences become U+FFFD; the file is not
/// refused for them.
fn decode(bytes: &[u8]) -> Vec<char> {
    let utf16 = |b: &[u8], le: bool| -> Vec<char> {
        let units = b.chunks_exact(2).map(|p| {
            if le { u16::from_le_bytes([p[0], p[1]]) } else { u16::from_be_bytes([p[0], p[1]]) }
        });
        char::decode_utf16(units).map(|r| r.unwrap_or('\u{FFFD}')).collect()
    };
    match bytes {
        [0xEF, 0xBB, 0xBF, rest @ ..] => String::from_utf8_lossy(rest).chars().collect(),
        [0xFF, 0xFE, rest @ ..] => utf16(rest, true),
        [0xFE, 0xFF, rest @ ..] => utf16(rest, false),
        // No BOM: a file of ASCII-range characters in UTF-16 has a zero byte next to each one.
        [a, 0, b, 0, ..] if *a != 0 && *b != 0 => utf16(bytes, true),
        [0, a, 0, b, ..] if *a != 0 && *b != 0 => utf16(bytes, false),
        _ => String::from_utf8_lossy(bytes).chars().collect(),
    }
}

/// A character that must not reach a label: controls (they become a space) and the characters
/// that reorder text (they are dropped).
fn is_bidi_control(c: char) -> bool {
    matches!(c, '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
}

/// Make a value fit a one-line label: control characters (line breaks included) become spaces,
/// text-reordering characters go, the ends are trimmed. `None` for an empty or overlong result.
fn clean_value(raw: &str) -> Option<String> {
    let s: String = raw
        .chars()
        .filter(|c| !is_bidi_control(*c))
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let s = s.trim();
    (!s.is_empty() && s.chars().count() <= MAX_VALUE_CHARS).then(|| s.to_string())
}

fn clean_key(raw: &str) -> Option<String> {
    let k = raw.trim();
    (!k.is_empty() && k.chars().count() <= MAX_KEY_CHARS && k.bytes().all(|b| b.is_ascii_graphic()))
        .then(|| k.to_string())
}

struct Lexer {
    c: Vec<char>,
    i: usize,
    line: usize,
}

impl Lexer {
    fn peek(&self) -> Option<char> {
        self.c.get(self.i).copied()
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.i += 1;
        if c == '\n' {
            self.line += 1;
        }
        Some(c)
    }

    /// Skip white space and comments. An unterminated `/*` runs to the end of the file.
    fn skip_blank(&mut self) {
        loop {
            match self.peek() {
                Some(c) if c.is_whitespace() || c == '\u{FEFF}' => {
                    self.bump();
                }
                Some('/') if self.c.get(self.i + 1) == Some(&'/') => {
                    while self.peek().is_some_and(|c| c != '\n') {
                        self.bump();
                    }
                }
                Some('/') if self.c.get(self.i + 1) == Some(&'*') => {
                    self.bump();
                    self.bump();
                    while let Some(c) = self.bump() {
                        if c == '*' && self.peek() == Some('/') {
                            self.bump();
                            break;
                        }
                    }
                }
                _ => return,
            }
        }
    }

    /// A quoted string (the opening quote is next) or an unquoted word.
    fn token(&mut self) -> Option<String> {
        match self.peek()? {
            '"' => {
                self.bump();
                let mut out = String::new();
                loop {
                    match self.bump()? {
                        '"' => return Some(out),
                        '\\' => out.push(self.escape()?),
                        c => out.push(c),
                    }
                }
            }
            c if is_bare(c) => {
                let mut out = String::new();
                while let Some(c) = self.peek().filter(|c| is_bare(*c)) {
                    out.push(c);
                    self.bump();
                }
                Some(out)
            }
            _ => None,
        }
    }

    /// The character after a backslash.
    fn escape(&mut self) -> Option<char> {
        Some(match self.bump()? {
            'n' => '\n',
            't' => '\t',
            'r' => '\r',
            '0' => '\0',
            'U' | 'u' => {
                let mut v = 0u32;
                for _ in 0..4 {
                    v = v * 16 + self.bump()?.to_digit(16)?;
                }
                char::from_u32(v).unwrap_or('\u{FFFD}')
            }
            // `\"`, `\\`, `\'` and anything else stand for the character itself.
            other => other,
        })
    }

    /// After a bad entry: continue behind the next `;` (or at the end).
    fn recover(&mut self) {
        while let Some(c) = self.bump() {
            if c == ';' {
                return;
            }
        }
    }
}

fn is_bare(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '$' | '/' | ':' | '.' | '-')
}

/// Parse a strings file into `key -> text`. A later entry replaces an earlier one with the same
/// key. An empty file is an empty map; a file whose every entry is malformed is an error.
pub fn parse_strings(bytes: &[u8]) -> Result<BTreeMap<String, String>, StringsError> {
    if bytes.len() > MAX_STRINGS_BYTES {
        return Err(StringsError::TooLarge(bytes.len()));
    }
    let mut lx = Lexer { c: decode(bytes), i: 0, line: 1 };
    let mut out = BTreeMap::new();
    let mut first_error: Option<usize> = None;
    loop {
        lx.skip_blank();
        if lx.peek().is_none() {
            break;
        }
        let line = lx.line;
        let entry = (|| {
            let key = lx.token()?;
            lx.skip_blank();
            (lx.bump()? == '=').then_some(())?;
            lx.skip_blank();
            let value = lx.token()?;
            lx.skip_blank();
            (lx.bump()? == ';').then_some(())?;
            Some((key, value))
        })();
        match entry {
            Some((k, v)) => {
                if let (Some(k), Some(v)) = (clean_key(&k), clean_value(&v)) {
                    if out.len() < MAX_ENTRIES || out.contains_key(&k) {
                        out.insert(k, v);
                    }
                }
            }
            None => {
                first_error.get_or_insert(line);
                lx.recover();
            }
        }
    }
    match (out.is_empty(), first_error) {
        (true, Some(line)) => Err(StringsError::Syntax { line }),
        _ => Ok(out),
    }
}

/// The key of a keyword's name in a strings file: `media-type` + `stationery` is
/// `media-type.stationery`.
pub fn string_key(attribute: &str, keyword: &str) -> String {
    format!("{attribute}.{keyword}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utf16(s: &str, le: bool, bom: bool) -> Vec<u8> {
        let mut out = Vec::new();
        if bom {
            out.extend(if le { [0xFF, 0xFE] } else { [0xFE, 0xFF] });
        }
        for u in s.encode_utf16() {
            out.extend(if le { u.to_le_bytes() } else { u.to_be_bytes() });
        }
        out
    }

    const SAMPLE: &str = "/* Sample strings for the tests: made up, not a printer's. */\r\n\
        \"media-type.sample-one\" = \"Sample Photo Paper\";\r\n\
        // a line comment\r\n\
        \"media-type.sample-two\"=\"Sample \\\"Quoted\\\" \\\\ Paper\";\r\n\
        \"media-source.sample-tray\"\t=\t\"Sample Tray\" ;\n";

    #[test]
    fn reads_quoted_pairs_comments_and_escapes() {
        let m = parse_strings(SAMPLE.as_bytes()).unwrap();
        assert_eq!(m.len(), 3);
        assert_eq!(m["media-type.sample-one"], "Sample Photo Paper");
        assert_eq!(m["media-type.sample-two"], r#"Sample "Quoted" \ Paper"#);
        assert_eq!(m["media-source.sample-tray"], "Sample Tray");
        assert_eq!(string_key("media-type", "sample-one"), "media-type.sample-one");
    }

    #[test]
    fn utf8_with_and_without_bom_and_japanese_text() {
        let text = "\"media-type.x\" = \"サンプル用紙 光沢\";\n";
        let plain = parse_strings(text.as_bytes()).unwrap();
        assert_eq!(plain["media-type.x"], "サンプル用紙 光沢");
        let mut bom = vec![0xEF, 0xBB, 0xBF];
        bom.extend(text.as_bytes());
        assert_eq!(parse_strings(&bom).unwrap(), plain);
    }

    #[test]
    fn utf16_with_a_bom_either_way_round_and_without() {
        let text = "\"media-type.x\" = \"サンプル用紙 \\U0041\";\n\"k2\" = \"𝄞 clef\";";
        let want = parse_strings(text.as_bytes()).unwrap();
        assert_eq!(want["media-type.x"], "サンプル用紙 A");
        assert_eq!(want["k2"], "𝄞 clef");
        for (le, bom) in [(true, true), (false, true), (true, false), (false, false)] {
            assert_eq!(
                parse_strings(&utf16(text, le, bom)).unwrap(),
                want,
                "utf-16 le={le} bom={bom}"
            );
        }
    }

    #[test]
    fn comments_may_hide_entries_and_stray_text_is_skipped() {
        let text = "/* \"a\" = \"hidden\"; */ \"a\" = \"shown\"; // \"b\" = \"hidden\";\n\
                    \"c\" = \"also /* not a comment */ shown\";\n/* unterminated";
        let m = parse_strings(text.as_bytes()).unwrap();
        assert_eq!(m["a"], "shown");
        assert_eq!(m["c"], "also /* not a comment */ shown");
        assert!(!m.contains_key("b"));
    }

    #[test]
    fn unquoted_words_and_the_remaining_escapes() {
        let m = parse_strings(b"media-type.bare = plain;\n\"k\" = \"a\\tb\\nc\\'d\";").unwrap();
        assert_eq!(m["media-type.bare"], "plain");
        // Line breaks and tabs cannot stay in a one-line label: they become spaces.
        assert_eq!(m["k"], "a b c'd");
    }

    #[test]
    fn a_bad_entry_costs_only_itself() {
        let text = "\"a\" = \"1\";\n\"broken\" \"2\";\n\"b\" = \"3\";\n= \"x\";\n\"c\" = \"unterminated;\n";
        let m = parse_strings(text.as_bytes()).unwrap();
        assert_eq!(m["a"], "1");
        assert_eq!(m["b"], "3");
        assert!(!m.contains_key("broken") && !m.contains_key("c"));
        // Nothing usable at all is an error with the line of the first problem.
        assert_eq!(
            parse_strings(b"\n\nthis is not a strings file"),
            Err(StringsError::Syntax { line: 3 })
        );
        // An empty file, or comments only, is simply empty.
        assert!(parse_strings(b"").unwrap().is_empty());
        assert!(parse_strings(b"/* nothing here */\n").unwrap().is_empty());
    }

    #[test]
    fn later_entries_win_and_values_are_made_safe() {
        let text = "\"k\" = \"first\"; \"k\" = \"  second  \";\n\
                    \"ctl\" = \"a\\u0001b\\u202Ec\\u200Fd\\u0085e\";\n\
                    \"empty\" = \"   \"; \"blank\" = \"\";";
        let m = parse_strings(text.as_bytes()).unwrap();
        assert_eq!(m["k"], "second");
        assert_eq!(m["ctl"], "a bcd e", "controls become spaces, reordering marks go");
        assert!(!m.contains_key("empty") && !m.contains_key("blank"));
    }

    #[test]
    fn limits_hold_for_hostile_files() {
        // Too big for a strings file.
        let big = vec![b' '; MAX_STRINGS_BYTES + 1];
        assert_eq!(parse_strings(&big), Err(StringsError::TooLarge(MAX_STRINGS_BYTES + 1)));
        // Overlong keys and values and keys with spaces or non-ASCII characters are dropped.
        let long_key = "k".repeat(MAX_KEY_CHARS + 1);
        let long_val = "v".repeat(MAX_VALUE_CHARS + 1);
        let text = format!(
            "\"{long_key}\" = \"x\"; \"k\" = \"{long_val}\"; \"with space\" = \"x\"; \"キー\" = \"x\"; \"ok\" = \"{}\";",
            "v".repeat(MAX_VALUE_CHARS)
        );
        let m = parse_strings(text.as_bytes()).unwrap();
        assert_eq!(m.keys().collect::<Vec<_>>(), ["ok"]);
        // Only so many entries are kept.
        let many: String = (0..MAX_ENTRIES + 50).map(|i| format!("\"k{i}\" = \"v\";\n")).collect();
        assert_eq!(parse_strings(many.as_bytes()).unwrap().len(), MAX_ENTRIES);
    }

    #[test]
    fn garbage_never_panics() {
        let base = utf16(SAMPLE, true, true);
        for cut in 0..base.len() {
            let _ = parse_strings(&base[..cut]);
        }
        for i in 0..SAMPLE.len() {
            for flip in [0x01u8, 0x80, 0xFF] {
                let mut b = SAMPLE.as_bytes().to_vec();
                b[i] ^= flip;
                let _ = parse_strings(&b);
            }
        }
        let _ = parse_strings(&[0xFF, 0xFE, 0x22]); // odd trailing byte
        let _ = parse_strings(&[0x22, 0x5C, 0x55, 0x44]); // backslash-U cut short
        let _ = parse_strings(b"\"a\" = \"\\UD800\";"); // a lone surrogate
    }
}
