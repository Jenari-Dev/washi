// SPDX-License-Identifier: GPL-2.0-or-later
//! What a printer's *support code* (`current_support_code` of IVEC `GetStatus print`, e.g. `1000`,
//! `1600`, `D109`) means, in short messages of Washi's own.
//!
//! The printer reports only the code. This module groups codes into families (a paper jam is
//! `13xx`, an empty tank is `1600`, ...) plus a few specific codes, and gives each family a
//! stable message key, a severity and a text in English and Japanese. The texts are written for
//! Washi: they say what to do, they are not Canon's wording, and the table only records which
//! codes belong together. A code that is in no family still gets a generic message that names
//! the code, so nothing is ever hidden.
//!
//! The desktop app words the same keys through its own translation catalog (`Fmt.supportMessage`
//! in `qml/Fmt.qml`); `po/check.py` makes sure it has a text for every key of this table.

use serde::{Deserialize, Serialize};

use crate::lang::Language;

/// How much attention a code asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// A notice; the printer works.
    Info,
    /// Printing may be possible, but something needs a look.
    #[default]
    Warning,
    /// The printer cannot go on until the problem is dealt with.
    Error,
}

impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Info => "info",
            Severity::Warning => "warning",
            Severity::Error => "error",
        }
    }
}

/// One family of support codes.
#[derive(Debug, PartialEq, Eq)]
pub struct SupportInfo {
    /// Stable key of the message (`support.paper-jam`); translation tables use it.
    pub key: &'static str,
    pub severity: Severity,
    /// English text. `{code}` stands for the code (only the unknown-code text has it).
    pub en: &'static str,
    /// Japanese text.
    pub ja: &'static str,
}

impl SupportInfo {
    /// The text in a language, with the code filled in.
    pub fn text(&self, lang: Language, code: &str) -> String {
        let t = match lang {
            Language::En => self.en,
            Language::Ja => self.ja,
        };
        t.replace("{code}", code)
    }
}

macro_rules! info {
    ($key:literal, $sev:ident, $en:literal, $ja:literal) => {
        SupportInfo { key: concat!("support.", $key), severity: Severity::$sev, en: $en, ja: $ja }
    };
}

static NO_PAPER_REAR: SupportInfo = info!(
    "no-paper-rear",
    Warning,
    "The rear tray is out of paper. Load some, then press OK on the printer.",
    "後トレイの用紙がなくなりました。用紙をセットしてから、プリンター本体のOKボタンを押してください。"
);
static NO_PAPER_CASSETTE: SupportInfo = info!(
    "no-paper-cassette",
    Warning,
    "The cassette is out of paper. Load some, then press OK on the printer.",
    "カセットの用紙がなくなりました。用紙をセットしてから、プリンター本体のOKボタンを押してください。"
);
static PAPER_FEED: SupportInfo = info!(
    "paper-feed",
    Warning,
    "The paper could not be fed, or it does not match the paper settings. Check the paper and the paper settings on the printer.",
    "用紙を送れなかったか、用紙の設定と合っていません。用紙と、プリンター本体の用紙設定を確認してください。"
);
static COVER: SupportInfo = info!(
    "cover",
    Warning,
    "A cover or tray is in the wrong position. Close the cover, or open the output tray, as the printer shows.",
    "カバーまたはトレイの位置が正しくありません。プリンターの表示に従って、カバーを閉じるか、排紙トレイを開いてください。"
);
static PAPER_JAM: SupportInfo = info!(
    "paper-jam",
    Error,
    "Paper is jammed. Remove it carefully; switch the printer off only if its display tells you to.",
    "用紙がつまっています。慎重に取り除いてください。プリンターの電源を切るのは、画面で指示されたときだけにしてください。"
);
static DUPLEX_SIZE: SupportInfo = info!(
    "duplex-size",
    Warning,
    "This paper size cannot be printed on both sides. Print on one side or choose another size.",
    "この用紙サイズは両面印刷できません。片面印刷にするか、別のサイズを選んでください。"
);
static HEAD_MISSING: SupportInfo = info!(
    "head-missing",
    Error,
    "The print head is not recognized. Reseat it, then restart the printer.",
    "プリントヘッドが認識されません。取り付け直してから、プリンターを再起動してください。"
);
static TANK_UNRECOGNIZED: SupportInfo = info!(
    "tank-unrecognized",
    Error,
    "An ink tank is not recognized. Press it in until it clicks.",
    "インクタンクが認識されません。カチッと音がするまで押し込んでください。"
);
static TANK_WRONG_SLOT: SupportInfo = info!(
    "tank-wrong-slot",
    Error,
    "An ink tank is in the wrong slot. Match the tanks to the colour labels.",
    "インクタンクの取り付け位置が違います。色のラベルに合わせて入れ直してください。"
);
static INK_EMPTY: SupportInfo = info!(
    "ink-empty",
    Error,
    "An ink tank is empty. Replace it to keep printing.",
    "インクタンクが空です。印刷を続けるには交換してください。"
);
static TANK_MISSING: SupportInfo = info!(
    "tank-missing",
    Error,
    "An ink tank is missing. Install it.",
    "インクタンクが取り付けられていません。取り付けてください。"
);
static INK_LEVEL_UNKNOWN: SupportInfo = info!(
    "ink-level-unknown",
    Warning,
    "The level of an ink tank cannot be determined, or it may have run out. Check the tanks and the printer's display; printing may still be possible.",
    "インクタンクの残量を判別できないか、インクが切れている可能性があります。タンクと、プリンターの画面に出ている内容を見てください。印刷は続けられる場合があります。"
);
static ABSORBER_NEARLY_FULL: SupportInfo = info!(
    "absorber-nearly-full",
    Warning,
    "The ink absorber is nearly full. Printing continues for now; plan to have the printer serviced.",
    "インク吸収体がもうすぐいっぱいになります。当面は印刷できますが、点検を依頼する準備をしてください。"
);
static HEAD_PROTECTIVE_TAPE: SupportInfo = info!(
    "head-protective-tape",
    Error,
    "Packing material may still be on the print head holder. Remove it.",
    "プリントヘッドホルダーに保護材が残っている可能性があります。取り除いてください。"
);
static AUTO_ALIGN_FAILED: SupportInfo = info!(
    "auto-align-failed",
    Warning,
    "Automatic print head alignment did not succeed. Run a nozzle check, clean the print head if needed, then try again.",
    "プリントヘッドの自動位置調整に失敗しました。ノズルチェックを行い、必要ならクリーニングしてから、もう一度試してください。"
);
static ALIGN_SHEET_UNREADABLE: SupportInfo = info!(
    "align-sheet-unreadable",
    Warning,
    "The alignment sheet could not be read. Make sure it lies correctly on the glass, then try again.",
    "位置調整シートを読み取れませんでした。原稿台に正しく置かれているか確認して、もう一度試してください。"
);
static PRINT_DATA: SupportInfo = info!(
    "print-data",
    Warning,
    "This job cannot be printed. Cancel it and try again.",
    "このジョブは印刷できません。キャンセルして、もう一度試してください。"
);
static HARDWARE_ERROR: SupportInfo = info!(
    "hardware-error",
    Error,
    "The printer reported a hardware error. Switch it off and on again; if the code comes back, contact the maker's support.",
    "プリンター本体の故障が報告されました。電源を切って入れ直してください。同じ番号が再び表示される場合は、メーカーのサポートに連絡してください。"
);
static ABSORBER_FULL: SupportInfo = info!(
    "absorber-full",
    Error,
    "The ink absorber has filled up. The printer needs service by the maker.",
    "インク吸収体がいっぱいです。メーカーによる点検が必要です。"
);
static NOTICE: SupportInfo = info!(
    "notice",
    Info,
    "The printer has a notice for you. See its display, or use the help link for details.",
    "プリンターからお知らせがあります。詳しくは、プリンターの表示かヘルプのリンクを確認してください。"
);
static UNKNOWN: SupportInfo = info!(
    "unknown",
    Warning,
    "The printer reports support code {code}. See the printer's display, or use the help link.",
    "プリンターがサポート番号 {code} を報告しています。プリンターの表示か、ヘルプのリンクを確認してください。"
);

/// How a rule recognises a code (the code is upper-cased and trimmed first).
enum Pat {
    /// Exactly one of these.
    Codes(&'static [&'static str]),
    /// Four decimal digits within an inclusive range.
    Range(u16, u16),
    /// Four hexadecimal digits that start with this.
    Prefix(&'static str),
}

impl Pat {
    fn matches(&self, code: &str) -> bool {
        match self {
            Pat::Codes(list) => list.contains(&code),
            Pat::Range(lo, hi) => {
                code.len() == 4
                    && code.bytes().all(|b| b.is_ascii_digit())
                    && code.parse::<u16>().is_ok_and(|n| (*lo..=*hi).contains(&n))
            }
            Pat::Prefix(p) => {
                code.len() == 4
                    && code.bytes().all(|b| b.is_ascii_hexdigit())
                    && code.starts_with(p)
            }
        }
    }
}

/// The families, most specific first: the first rule that matches wins.
static RULES: &[(Pat, &SupportInfo)] = &[
    (Pat::Codes(&["1000"]), &NO_PAPER_REAR),
    (Pat::Codes(&["1003"]), &NO_PAPER_CASSETTE),
    (Pat::Range(1000, 1099), &PAPER_FEED),
    (Pat::Range(1200, 1299), &COVER),
    (Pat::Codes(&["1310"]), &DUPLEX_SIZE),
    (Pat::Range(1300, 1399), &PAPER_JAM),
    (Pat::Codes(&["1401", "1403", "1405"]), &HEAD_MISSING),
    (Pat::Range(1410, 1419), &TANK_UNRECOGNIZED),
    (Pat::Range(1470, 1476), &TANK_UNRECOGNIZED),
    (Pat::Codes(&["1485", "1486"]), &TANK_WRONG_SLOT),
    (Pat::Codes(&["1600"]), &INK_EMPTY),
    (Pat::Codes(&["1660"]), &TANK_MISSING),
    (Pat::Range(1680, 1689), &INK_LEVEL_UNKNOWN),
    (Pat::Codes(&["1700", "1701"]), &ABSORBER_NEARLY_FULL),
    (Pat::Codes(&["1890"]), &HEAD_PROTECTIVE_TAPE),
    (Pat::Codes(&["2500"]), &AUTO_ALIGN_FAILED),
    (Pat::Codes(&["2900", "2901"]), &ALIGN_SHEET_UNREADABLE),
    (Pat::Codes(&["4100", "4102", "4103"]), &PRINT_DATA),
    (Pat::Prefix("5B"), &ABSORBER_FULL),
    (Pat::Codes(&["5011", "5012", "5050", "5100", "5200", "5400", "5700"]), &HARDWARE_ERROR),
    (Pat::Prefix("6"), &HARDWARE_ERROR),
    (Pat::Prefix("B"), &HARDWARE_ERROR),
    (Pat::Prefix("C"), &HARDWARE_ERROR),
    (Pat::Prefix("D"), &NOTICE),
];

/// Every family once, for tests and for the translation check.
pub fn families() -> Vec<&'static SupportInfo> {
    let mut all: Vec<&'static SupportInfo> = Vec::new();
    for info in RULES.iter().map(|(_, info)| *info).chain([&UNKNOWN]) {
        if !all.iter().any(|a| a.key == info.key) {
            all.push(info);
        }
    }
    all
}

/// The family of a support code; codes in no family get the generic "unknown" entry.
pub fn classify(code: &str) -> &'static SupportInfo {
    let code = code.trim().to_ascii_uppercase();
    RULES.iter().find(|(pat, _)| pat.matches(&code)).map_or(&UNKNOWN, |(_, info)| info)
}

/// The text for a code in a language.
pub fn message(code: &str, lang: Language) -> String {
    classify(code).text(lang, code.trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: &str) -> &'static str {
        classify(code).key
    }

    #[test]
    fn specific_codes_come_before_their_families() {
        assert_eq!(key("1000"), "support.no-paper-rear");
        assert_eq!(key("1003"), "support.no-paper-cassette");
        assert_eq!(key("1001"), "support.paper-feed");
        assert_eq!(key("1099"), "support.paper-feed");
        assert_eq!(key("1310"), "support.duplex-size");
        assert_eq!(key("1300"), "support.paper-jam");
        assert_eq!(key("1303"), "support.paper-jam");
        assert_eq!(key("1313"), "support.paper-jam");
        assert_eq!(key("1350"), "support.paper-jam");
    }

    #[test]
    fn ink_and_head_families() {
        for c in ["1401", "1403", "1405"] {
            assert_eq!(key(c), "support.head-missing", "{c}");
        }
        for c in ["1410", "1415", "1419", "1470", "1476"] {
            assert_eq!(key(c), "support.tank-unrecognized", "{c}");
        }
        assert_eq!(key("1420"), "support.unknown");
        assert_eq!(key("1477"), "support.unknown");
        assert_eq!(key("1485"), "support.tank-wrong-slot");
        assert_eq!(key("1600"), "support.ink-empty");
        assert_eq!(key("1660"), "support.tank-missing");
        for c in ["1680", "1686", "1688", "1689"] {
            assert_eq!(key(c), "support.ink-level-unknown", "{c}");
        }
        assert_eq!(key("1700"), "support.absorber-nearly-full");
        assert_eq!(key("1890"), "support.head-protective-tape");
    }

    #[test]
    fn alignment_and_data_codes() {
        assert_eq!(key("2500"), "support.auto-align-failed");
        assert_eq!(key("2900"), "support.align-sheet-unreadable");
        assert_eq!(key("2901"), "support.align-sheet-unreadable");
        assert_eq!(key("4102"), "support.print-data");
        assert_eq!(key("4101"), "support.unknown");
    }

    #[test]
    fn hardware_errors_and_the_absorber_full_code() {
        for c in ["5011", "5200", "5700", "6000", "6A3F", "6FFF", "B200", "C000", "cfff"] {
            assert_eq!(key(c), "support.hardware-error", "{c}");
        }
        for c in ["5B00", "5B01", "5b02", "5B1F"] {
            assert_eq!(key(c), "support.absorber-full", "{c}");
        }
        // Not 4-character hex codes.
        for c in ["6", "60000", "6XYZ", "B2", ""] {
            assert_eq!(key(c), "support.unknown", "{c:?}");
        }
    }

    #[test]
    fn d_codes_are_notices() {
        assert_eq!(key("D109"), "support.notice");
        assert_eq!(key("d001"), "support.notice");
        assert_eq!(classify("D109").severity, Severity::Info);
    }

    #[test]
    fn severity_follows_the_family() {
        assert_eq!(classify("1000").severity, Severity::Warning);
        assert_eq!(classify("1300").severity, Severity::Error);
        assert_eq!(classify("1600").severity, Severity::Error);
        assert_eq!(classify("1688").severity, Severity::Warning);
        assert_eq!(classify("5B00").severity, Severity::Error);
        assert_eq!(classify("9999").severity, Severity::Warning, "unknown codes are warnings");
    }

    #[test]
    fn unknown_codes_name_themselves_in_both_languages() {
        let en = message("9ABC", Language::En);
        let ja = message("9ABC", Language::Ja);
        assert!(en.contains("9ABC") && !en.contains("{code}"), "{en}");
        assert!(ja.contains("9ABC") && ja.contains("サポート番号"), "{ja}");
        // Whitespace around the code is not part of it.
        assert!(message("  9ABC ", Language::En).contains("code 9ABC."));
    }

    #[test]
    fn every_family_has_a_key_and_texts_in_both_languages() {
        let all = families();
        assert!(all.len() >= 20, "{} families", all.len());
        let mut keys = std::collections::BTreeSet::new();
        for f in &all {
            assert!(f.key.starts_with("support."), "{}", f.key);
            assert!(keys.insert(f.key), "duplicate key {}", f.key);
            assert!(f.en.is_ascii(), "English text of {} has non-ASCII characters", f.key);
            assert!(f.en.ends_with('.') && f.ja.ends_with('。'), "{}", f.key);
            assert!(
                f.ja.chars().any(|c| ('\u{3040}'..='\u{30ff}').contains(&c)),
                "{} has no Japanese text",
                f.key
            );
            assert!(!f.en.is_empty() && !f.ja.contains("  "), "{}", f.key);
        }
    }

    #[test]
    fn japanese_uses_the_standard_support_number_wording() {
        for f in families() {
            assert!(!f.ja.contains("サポートコード"), "{}", f.key);
        }
    }
}
