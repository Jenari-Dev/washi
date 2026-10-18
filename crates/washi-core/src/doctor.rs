// SPDX-License-Identifier: GPL-2.0-or-later
//! The self-check behind `washi doctor` and the app's "Check setup": a list of short, read-only
//! checks, each answered with a tick, a cross or a dash, and a one-line hint.
//!
//! * `ok` (✓): works.
//! * `fail` (✗): something that Washi needs is wrong or unreachable.
//! * `skip` (–): optional, not applicable here, or not known yet. Never a problem by itself.
//!
//! The checks that belong to a printer (its management channel, IPP, eSCL, kept words and links)
//! and to this computer (helper programs, locales, paths) live here, so that the command line and
//! the daemon run the very same ones; the parts that need D-Bus (the daemon, the notification
//! service) are added by their callers. Every text is written in English and Japanese side by
//! side, in the language the caller asks for.
//!
//! Nothing here writes to the printer, and no serial number, hardware address or address of the
//! printer ever ends up in a text (see [`scrub`]).

use std::net::IpAddr;
use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::lang::Language;
use crate::localization::LocalizationCache;
use crate::printer::{Printer, resolve_ipv4, split_host_port};
use crate::tools::{Tools, run_capture};

/// The answer of one check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Ok,
    Fail,
    Skip,
}

impl Status {
    /// ✓, ✗ or –.
    pub fn symbol(self) -> &'static str {
        match self {
            Status::Ok => "\u{2713}",
            Status::Fail => "\u{2717}",
            Status::Skip => "\u{2013}",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Check {
    /// A stable name (`printer.chmp`), for scripts and tests; not shown.
    pub id: String,
    pub status: Status,
    pub label: String,
    /// What was found, in a few words (may be empty).
    pub detail: String,
    /// What to do about it, in one line (may be empty).
    pub hint: String,
}

impl Check {
    fn new(id: &str, status: Status, label: String, detail: String, hint: String) -> Check {
        Check { id: id.to_string(), status, label, detail, hint }
    }

    pub fn ok(id: &str, label: impl Into<String>, detail: impl Into<String>) -> Check {
        Check::new(id, Status::Ok, label.into(), detail.into(), String::new())
    }

    pub fn fail(
        id: &str,
        label: impl Into<String>,
        detail: impl Into<String>,
        hint: impl Into<String>,
    ) -> Check {
        Check::new(id, Status::Fail, label.into(), detail.into(), hint.into())
    }

    pub fn skip(
        id: &str,
        label: impl Into<String>,
        detail: impl Into<String>,
        hint: impl Into<String>,
    ) -> Check {
        Check::new(id, Status::Skip, label.into(), detail.into(), hint.into())
    }

    /// The same check with a hint (for a ✓ that still has something to say).
    pub fn with_hint(mut self, hint: impl Into<String>) -> Check {
        self.hint = hint.into();
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Group {
    pub id: String,
    pub title: String,
    pub checks: Vec<Check>,
}

impl Group {
    pub fn new(id: &str, title: impl Into<String>, checks: Vec<Check>) -> Group {
        Group { id: id.to_string(), title: title.into(), checks }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Summary {
    pub ok: usize,
    pub fail: usize,
    pub skip: usize,
}

/// The whole report, as it is printed (`--json`) and as the daemon's `Doctor()` returns it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Report {
    /// Who made it: `washi` (the command line) or `washid`.
    pub source: String,
    /// The language of the texts: `en` or `ja`.
    pub language: String,
    pub groups: Vec<Group>,
    pub summary: Summary,
}

impl Report {
    pub fn new(source: &str, lang: Language, groups: Vec<Group>) -> Report {
        let mut summary = Summary::default();
        for c in groups.iter().flat_map(|g| &g.checks) {
            match c.status {
                Status::Ok => summary.ok += 1,
                Status::Fail => summary.fail += 1,
                Status::Skip => summary.skip += 1,
            }
        }
        Report { source: source.to_string(), language: lang.code().to_string(), groups, summary }
    }

    /// The check with this id, if any.
    pub fn check(&self, id: &str) -> Option<&Check> {
        self.groups.iter().flat_map(|g| &g.checks).find(|c| c.id == id)
    }

    /// The report as people read it in a terminal.
    pub fn render_text(&self) -> String {
        let mut o = String::new();
        for g in &self.groups {
            o.push_str(&g.title);
            o.push('\n');
            for c in &g.checks {
                o.push_str(&format!("  {} {}", c.status.symbol(), c.label));
                if !c.detail.is_empty() {
                    o.push_str(&format!(": {}", c.detail));
                }
                o.push('\n');
                if !c.hint.is_empty() {
                    o.push_str(&format!("      {}\n", c.hint));
                }
            }
            o.push('\n');
        }
        let lang = if self.language == "ja" { Language::Ja } else { Language::En };
        let s = &self.summary;
        o.push_str(&match lang {
            Language::En => format!(
                "{} checks passed, {} problem{}, {} optional or not known.\n",
                s.ok,
                s.fail,
                if s.fail == 1 { "" } else { "s" },
                s.skip
            ),
            Language::Ja => format!(
                "問題なし {} 件、問題あり {} 件、任意または未確認 {} 件。\n",
                s.ok, s.fail, s.skip
            ),
        });
        o
    }
}

/// The text for `lang`.
pub fn tr(lang: Language, en: &str, ja: &str) -> String {
    match lang {
        Language::En => en.to_string(),
        Language::Ja => ja.to_string(),
    }
}

// ---- keeping private things out of the texts ---------------------------------------------------------

/// `text` without the printer's host name and without anything that looks like an IPv4 address
/// or a hardware address (replaced by `<printer>` / `<address>`).
pub fn scrub(text: &str, host: &str) -> String {
    let mut out = text.to_string();
    let (name, _) = split_host_port(host);
    if !name.is_empty() {
        let lower = out.to_ascii_lowercase();
        let needle = name.to_ascii_lowercase();
        let mut res = String::with_capacity(out.len());
        let mut at = 0;
        while let Some(i) = lower[at..].find(&needle) {
            res.push_str(&out[at..at + i]);
            res.push_str("<printer>");
            at += i + needle.len();
        }
        res.push_str(&out[at..]);
        out = res;
    }
    // Words made of digits and dots/colons/hex: IPv4 (a.b.c.d[:port]) and MAC addresses.
    let mut res = String::with_capacity(out.len());
    let mut word = String::new();
    let flush = |word: &mut String, res: &mut String| {
        if looks_like_address(word) {
            res.push_str("<address>");
        } else {
            res.push_str(word);
        }
        word.clear();
    };
    for c in out.chars() {
        if c.is_ascii_hexdigit() || c == '.' || c == ':' {
            word.push(c);
        } else {
            flush(&mut word, &mut res);
            res.push(c);
        }
    }
    flush(&mut word, &mut res);
    res
}

fn looks_like_address(w: &str) -> bool {
    let w = w.trim_matches(|c| c == '.' || c == ':');
    // a.b.c.d with an optional :port
    let v4 = w.split(':').next().unwrap_or("");
    let parts: Vec<&str> = v4.split('.').collect();
    if parts.len() == 4
        && parts.iter().all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
    {
        return true;
    }
    // aa:bb:cc:dd:ee:ff
    let hex: Vec<&str> = w.split(':').collect();
    hex.len() == 6 && hex.iter().all(|p| p.len() == 2 && p.bytes().all(|b| b.is_ascii_hexdigit()))
}

fn short_path(p: &Path) -> String {
    match dirs::home_dir() {
        Some(h) => match p.strip_prefix(&h) {
            Ok(rest) => format!("~/{}", rest.display()),
            Err(_) => p.display().to_string(),
        },
        None => p.display().to_string(),
    }
}

// ---- the computer: files, helper programs, locales ------------------------------------------------------

/// Where the settings and the kept copies live. `config` and `cache` are the files and folders in
/// use (`None` when this system has none).
pub fn files_checks(lang: Language, config: Option<&Path>, cache: Option<&Path>) -> Vec<Check> {
    let mut v = Vec::new();
    let label = tr(lang, "Settings file", "設定ファイル");
    match config {
        None => v.push(Check::skip(
            "files.config",
            label,
            tr(
                lang,
                "this system has no configuration folder",
                "この環境には設定フォルダーがありません",
            ),
            "",
        )),
        Some(p) => {
            let shown = short_path(p);
            match crate::Config::load_from(p) {
                Err(e) => v.push(Check::fail(
                    "files.config",
                    label,
                    format!("{shown}: {}", scrub(&e.to_string(), "")),
                    tr(
                        lang,
                        "fix or delete the file; Washi then starts with its defaults",
                        "ファイルを直すか削除してください。削除すると初期設定で起動します",
                    ),
                )),
                Ok(_) if p.exists() => v.push(Check::ok("files.config", label, shown)),
                Ok(_) => v.push(Check::skip(
                    "files.config",
                    label,
                    format!(
                        "{shown} ({})",
                        tr(
                            lang,
                            "not created yet, defaults are used",
                            "まだ作られていません。初期設定を使います"
                        )
                    ),
                    "",
                )),
            }
        }
    }
    let label = tr(lang, "Cache folder", "キャッシュフォルダー");
    match cache {
        None => v.push(Check::skip(
            "files.cache",
            label,
            tr(
                lang,
                "this system has no cache folder",
                "この環境にはキャッシュフォルダーがありません",
            ),
            "",
        )),
        Some(d) => {
            let shown = short_path(d);
            if d.is_dir() {
                let writable = std::fs::metadata(d).is_ok_and(|m| !m.permissions().readonly());
                if writable {
                    v.push(Check::ok("files.cache", label, shown));
                } else {
                    v.push(Check::fail(
                        "files.cache",
                        label,
                        shown,
                        tr(
                            lang,
                            "the folder is read-only, so nothing the printer says can be kept",
                            "フォルダーが読み取り専用のため、プリンターから読み取った内容を保存できません",
                        ),
                    ));
                }
            } else {
                v.push(Check::skip(
                    "files.cache",
                    label,
                    format!(
                        "{shown} ({})",
                        tr(lang, "created when needed", "必要になったときに作られます")
                    ),
                    "",
                ));
            }
        }
    }
    v
}

/// The optional helper programs (poppler, tesseract and its languages, LibreOffice). Missing ones
/// only switch off the features that need them.
pub fn tools_checks(lang: Language, tools: &Tools) -> Vec<Check> {
    let mut v = Vec::new();
    let label = tr(lang, "PDF printing (pdftoppm, pdfinfo)", "PDFの印刷（pdftoppm、pdfinfo）");
    v.push(match (&tools.pdftoppm, &tools.pdfinfo) {
        (Some(_), Some(_)) => Check::ok("tools.poppler", label, tr(lang, "found", "あります")),
        (a, b) => {
            let missing: Vec<&str> = [(a, "pdftoppm"), (b, "pdfinfo")]
                .into_iter()
                .filter(|(p, _)| p.is_none())
                .map(|(_, n)| n)
                .collect();
            Check::skip(
                "tools.poppler",
                label,
                format!("{}: {}", tr(lang, "missing", "見つかりません"), missing.join(", ")),
                tr(
                    lang,
                    "install poppler (poppler-utils) to print PDF files; photos work without it",
                    "PDFを印刷するには poppler（poppler-utils）を入れてください。写真は不要です",
                ),
            )
        }
    });

    let label = tr(lang, "Text recognition (tesseract)", "文字認識（tesseract）");
    v.push(match &tools.tesseract {
        None => Check::skip(
            "tools.tesseract",
            label,
            tr(lang, "missing", "見つかりません"),
            tr(
                lang,
                "install tesseract with the English and Japanese data to make searchable scans",
                "検索できるスキャンを作るには、英語と日本語のデータ付きで tesseract を入れてください",
            ),
        ),
        Some(exe) => {
            let langs = tesseract_languages(exe);
            let want = ["eng", "jpn"];
            let missing: Vec<&str> =
                want.iter().copied().filter(|w| !langs.iter().any(|l| l == w)).collect();
            let found = if langs.is_empty() {
                tr(lang, "found", "あります")
            } else {
                format!("{}: {}", tr(lang, "languages", "言語"), langs.join(", "))
            };
            let c = Check::ok("tools.tesseract", label, found);
            if !langs.is_empty() && !missing.is_empty() {
                c.with_hint(format!(
                    "{}: {}",
                    tr(
                        lang,
                        "install the language data to recognise these",
                        "次の言語を認識するには言語データを入れてください"
                    ),
                    missing.join(", ")
                ))
            } else {
                c
            }
        }
    });

    let label = tr(lang, "Office documents (soffice)", "Office文書（soffice）");
    v.push(if tools.soffice.is_some() {
        Check::ok("tools.soffice", label, tr(lang, "found", "あります"))
    } else {
        Check::skip(
            "tools.soffice",
            label,
            tr(lang, "missing", "見つかりません"),
            tr(
                lang,
                "install LibreOffice to print Word, Excel and PowerPoint files",
                "Word・Excel・PowerPointを印刷するには LibreOffice を入れてください",
            ),
        )
    });
    v
}

/// The languages `tesseract --list-langs` names (empty when it cannot be asked).
fn tesseract_languages(exe: &Path) -> Vec<String> {
    let mut cmd = std::process::Command::new(exe);
    cmd.arg("--list-langs");
    let Ok(out) = run_capture(cmd, Duration::from_secs(5), "tesseract") else {
        return Vec::new();
    };
    let text = format!(
        "{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    text.lines()
        .map(str::trim)
        .filter(|l| {
            !l.is_empty()
                && *l != "osd"
                && !l.starts_with("List of")
                && l.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        })
        .map(str::to_string)
        .collect()
}

/// Are the English and Japanese locales installed (`locale -a`)? The app's own English and
/// Japanese texts do not need them; the system's dates, numbers and other programs do.
pub fn locale_checks(lang: Language) -> Vec<Check> {
    let names = std::process::Command::new("locale")
        .arg("-a")
        .stdin(std::process::Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| {
            String::from_utf8_lossy(&o.stdout).lines().map(str::to_string).collect::<Vec<_>>()
        });
    locale_checks_from(lang, names.as_deref())
}

/// [`locale_checks`] for a given list of locale names (`None`: `locale -a` could not be run).
pub fn locale_checks_from(lang: Language, names: Option<&[String]>) -> Vec<Check> {
    let label_en = tr(lang, "English locale", "英語のロケール");
    let label_ja = tr(lang, "Japanese locale", "日本語のロケール");
    let Some(names) = names else {
        let why =
            tr(lang, "`locale -a` is not available here", "この環境では `locale -a` を使えません");
        return vec![
            Check::skip("locale.en", label_en, why.clone(), ""),
            Check::skip("locale.ja", label_ja, why, ""),
        ];
    };
    let norm = |s: &str| s.to_ascii_lowercase().replace("utf-8", "utf8");
    let find = |pred: &dyn Fn(&str) -> bool| names.iter().find(|n| pred(&norm(n))).cloned();
    let en = find(&|n| n.starts_with("en_") && n.contains("utf8"))
        .or_else(|| find(&|n| n.starts_with("c.utf8")));
    let ja = find(&|n| n.starts_with("ja_jp") && n.contains("utf8"));
    vec![
        match en {
            Some(n) => Check::ok("locale.en", label_en, n),
            None => Check::skip(
                "locale.en",
                label_en,
                tr(lang, "none installed", "入っていません"),
                tr(
                    lang,
                    "Washi's English works anyway; generate en_US.UTF-8 for system dates and numbers",
                    "Washiの英語表示はそのまま使えます。システムの日付・数値用には en_US.UTF-8 を生成してください",
                ),
            ),
        },
        match ja {
            Some(n) => Check::ok("locale.ja", label_ja, n),
            None => Check::skip(
                "locale.ja",
                label_ja,
                tr(lang, "ja_JP.UTF-8 is not installed", "ja_JP.UTF-8 が入っていません"),
                tr(
                    lang,
                    "Washi's Japanese works anyway; generate ja_JP.UTF-8 so other programs can use Japanese too",
                    "Washiの日本語表示はそのまま使えます。他のプログラムでも日本語を使うなら ja_JP.UTF-8 を生成してください",
                ),
            ),
        },
    ]
}

// ---- the printer ----------------------------------------------------------------------------------------

fn norm_host(h: &str) -> String {
    h.trim().trim_end_matches('.').to_ascii_lowercase()
}

/// Does the printer answer mDNS (`_canon-chmp._tcp`) within `timeout`? Matches the printer by its
/// host name or address. Never a failure: a printer with a fixed address needs no mDNS.
pub fn mdns_check(lang: Language, host: &str, timeout: Duration) -> Check {
    let label = tr(lang, "Found by mDNS discovery", "mDNSで見つかる");
    let found = match washi_net::discover(timeout) {
        Ok(f) => f,
        Err(e) => {
            return Check::skip(
                "printer.mdns",
                label,
                format!(
                    "{}: {}",
                    tr(lang, "discovery could not run", "探索を実行できませんでした"),
                    scrub(&e.to_string(), host)
                ),
                tr(
                    lang,
                    "not needed when the printer is set up by its host name or address",
                    "ホスト名やアドレスで登録してあれば不要です",
                ),
            );
        }
    };
    let (name, _) = split_host_port(host);
    let want = norm_host(&name);
    let ip = resolve_ipv4(&name, Duration::from_secs(1));
    let seen = found.iter().any(|p| {
        norm_host(&p.host) == want
            || p.addresses.iter().any(|a| {
                a.parse::<IpAddr>().is_ok()
                    && (a.eq_ignore_ascii_case(&name) || ip.as_deref() == Some(a.as_str()))
            })
    });
    if seen {
        Check::ok("printer.mdns", label, tr(lang, "yes", "はい"))
    } else {
        Check::skip(
            "printer.mdns",
            label,
            if found.is_empty() {
                tr(lang, "no Canon printer answered", "Canonプリンターからの応答はありませんでした")
            } else {
                tr(
                    lang,
                    "other printers answered, not this one",
                    "別のプリンターからのみ応答がありました",
                )
            },
            tr(
                lang,
                "fine with a fixed address; otherwise check that the printer and this computer share a network",
                "固定のアドレスなら問題ありません。そうでなければプリンターとこのPCが同じネットワークにあるか確認してください",
            ),
        )
    }
}

/// What the printer answers to the read-only requests Washi relies on, and what of it is kept.
/// `cache` is where the printer's words, picture and links are kept.
pub fn printer_checks(
    lang: Language,
    p: &Printer,
    cache: Option<&LocalizationCache>,
) -> Vec<Check> {
    let host = p.host().to_string();
    let bad = |e: &dyn std::fmt::Display| scrub(&e.to_string(), &host);
    let mut v = Vec::new();

    // CHMP: the management channel (also answers while the printer sleeps).
    let label = tr(lang, "Management channel (CHMP)", "管理チャンネル（CHMP）");
    v.push(match p.device_capability() {
        Ok(cap) => {
            let model = cap.model.trim();
            let detail = if model.is_empty() {
                tr(lang, "answered", "応答あり")
            } else {
                format!("{model}, {} {}", tr(lang, "firmware", "ファームウェア"), cap.firmver)
            };
            Check::ok("printer.chmp", label, detail)
        }
        Err(e) => Check::fail(
            "printer.chmp",
            label,
            format!("{}: {}", tr(lang, "no answer", "応答なし"), bad(&e)),
            tr(
                lang,
                "check that the printer is switched on and on the same network as this computer",
                "プリンターの電源が入っていて、このPCと同じネットワークにあるか確認してください",
            ),
        ),
    });

    // IPP: printing and ink levels.
    let label = tr(lang, "Printing channel (IPP)", "印刷チャンネル（IPP）");
    v.push(match p.ipp().status() {
        Ok(a) => {
            let state = a.state.map(|s| s.as_str().to_string());
            let detail = match state {
                Some(s) => format!(
                    "{}: {s}; {} {}",
                    tr(lang, "state", "状態"),
                    a.markers.len(),
                    tr(lang, "ink tanks reported", "個のインクタンクを取得")
                ),
                None => tr(lang, "answered", "応答あり"),
            };
            Check::ok("printer.ipp", label, detail)
        }
        Err(e) => Check::fail(
            "printer.ipp",
            label,
            format!("{}: {}", tr(lang, "no answer", "応答なし"), bad(&e)),
            tr(
                lang,
                "printing and ink levels need port 631 of the printer to be reachable",
                "印刷とインク残量の取得には、プリンターのポート631に接続できる必要があります",
            ),
        ),
    });

    // eSCL: scanning.
    let label = tr(lang, "Scanning channel (eSCL)", "スキャンチャンネル（eSCL）");
    v.push(match p.escl().capabilities() {
        Ok(c) => Check::ok(
            "printer.escl",
            label,
            format!(
                "{} {}",
                tr(
                    lang,
                    "scanner capabilities read, version",
                    "スキャナーの機能を取得、バージョン"
                ),
                c.version
            ),
        ),
        Err(e) => Check::fail(
            "printer.escl",
            label,
            format!("{}: {}", tr(lang, "no answer", "応答なし"), bad(&e)),
            tr(
                lang,
                "scanning is unavailable until the printer's web server answers",
                "プリンターのWebサーバーが応答するまでスキャンは使えません",
            ),
        ),
    });

    // The printer's own words and picture, kept from earlier reads.
    let label = tr(lang, "Printer's own names kept", "プリンター固有の名称を保存済み");
    let kept_en = cache.and_then(|c| c.strings("en")).is_some();
    let kept_ja = cache.and_then(|c| c.strings("ja")).is_some();
    let note_later = tr(
        lang,
        "Washi reads them from the printer in the background once the app has shown it",
        "アプリがプリンターを表示すると、Washiがバックグラウンドで読み取ります",
    );
    v.push(if kept_en || kept_ja {
        let langs: Vec<&str> = [(kept_en, "en"), (kept_ja, "ja")]
            .into_iter()
            .filter(|(k, _)| *k)
            .map(|(_, l)| l)
            .collect();
        Check::ok("printer.strings", label, langs.join(", "))
    } else {
        Check::skip(
            "printer.strings",
            label,
            tr(lang, "not kept yet", "まだ保存されていません"),
            note_later.clone(),
        )
    });
    let label = tr(lang, "Printer picture kept", "プリンターの画像を保存済み");
    v.push(match cache.and_then(LocalizationCache::icon) {
        Some(i) if i.path.is_some() => Check::ok("printer.icon", label, tr(lang, "yes", "はい")),
        Some(_) => Check::ok(
            "printer.icon",
            label,
            tr(lang, "the printer offers none", "このプリンターは画像を提供していません"),
        ),
        None => Check::skip(
            "printer.icon",
            label,
            tr(lang, "not kept yet", "まだ保存されていません"),
            note_later,
        ),
    });

    // The web links (ink shop, manual, support): only served while the printer is awake.
    let label =
        tr(lang, "Web links (ink, manual, support)", "Webリンク（インク、マニュアル、サポート）");
    let src = p.web_sources_kept(lang, cache);
    v.push(match (&src.model, src.saved_at) {
        (Some(_), None) => Check::ok("printer.links", label, tr(lang, "read from the printer just now", "いまプリンターから読み取りました")),
        (Some(_), Some(_)) => Check::ok(
            "printer.links",
            label,
            tr(
                lang,
                "kept from an earlier read (the printer serves them only while it is on)",
                "以前に読み取った内容を使用中（プリンターは電源が入っている間だけ提供します）",
            ),
        ),
        (None, _) => Check::skip(
            "printer.links",
            label,
            tr(lang, "none available, the printer's own page is used", "取得できないため、プリンター自身のページを使います"),
            tr(
                lang,
                "the printer gives them out only while it is awake; open the app while it is on",
                "プリンターは電源が入っている間だけ提供します。電源が入っているときにアプリを開いてください",
            ),
        ),
    });
    v
}

/// The titles of the groups every report uses.
pub fn group_title(lang: Language, id: &str) -> String {
    match id {
        "washi" => tr(lang, "Washi", "Washi"),
        "service" => tr(lang, "Washi service (washid)", "Washiサービス（washid）"),
        "printer" => tr(lang, "Printer", "プリンター"),
        "desktop" => tr(lang, "Desktop", "デスクトップ"),
        "tools" => tr(lang, "Helper programs (optional)", "補助プログラム（任意）"),
        "files" => tr(lang, "Files", "ファイル"),
        other => other.to_string(),
    }
}

/// The groups about this computer: the desktop (the notification service the caller checked, and
/// the locales), the helper programs and the files.
pub fn computer_groups(
    lang: Language,
    notification: Check,
    config: Option<&Path>,
    cache: Option<&Path>,
    tools: &Tools,
) -> Vec<Group> {
    let mut desktop = vec![notification];
    desktop.extend(locale_checks(lang));
    vec![
        Group::new("desktop", group_title(lang, "desktop"), desktop),
        Group::new("tools", group_title(lang, "tools"), tools_checks(lang, tools)),
        Group::new("files", group_title(lang, "files"), files_checks(lang, config, cache)),
    ]
}

/// The notification service check, from what the caller found: the name of the server that
/// answered `GetServerInformation`, or why nothing did.
pub fn notification_check(lang: Language, found: Result<&str, &str>) -> Check {
    let label = tr(lang, "Desktop notification service", "デスクトップの通知サービス");
    match found {
        Ok(name) => Check::ok("desktop.notifications", label, name),
        Err(why) => Check::fail(
            "desktop.notifications",
            label,
            scrub(why, ""),
            tr(
                lang,
                "Washi's notifications need a notification daemon (org.freedesktop.Notifications); most desktops have one",
                "Washiの通知には通知デーモン（org.freedesktop.Notifications）が必要です。多くのデスクトップに入っています",
            ),
        ),
    }
}

/// The notification check of a daemon that was started with `--no-notify`.
pub fn notification_off_check(lang: Language) -> Check {
    Check::skip(
        "desktop.notifications",
        tr(lang, "Desktop notification service", "デスクトップの通知サービス"),
        tr(
            lang,
            "washid sends no notifications (--no-notify)",
            "washid は通知を送らない設定です（--no-notify）",
        ),
        tr(
            lang,
            "start washid without --no-notify to get them",
            "通知を受け取るには --no-notify を付けずに washid を起動してください",
        ),
    )
}

/// Shown instead of the printer checks when there is no printer to look at.
pub fn no_printer_check(lang: Language) -> Check {
    Check::fail(
        "printer.none",
        tr(lang, "A printer to check", "確認するプリンター"),
        tr(
            lang,
            "none is set up and none was found",
            "設定されたプリンターも、見つかったプリンターもありません",
        ),
        tr(
            lang,
            "switch the printer on, then run `washi discover`, or save one with `washi config set-host HOST`",
            "プリンターの電源を入れて `washi discover` を実行するか、`washi config set-host HOST` で登録してください",
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_and_the_host_never_reach_a_text() {
        let t = scrub(
            "connect to 192.0.2.68:80 failed; Printer.Local refused; mac 00:1e:8f:aa:bb:cc, v1.060, at 12:30",
            "printer.local",
        );
        assert!(!t.contains("192.0.2") && !t.to_lowercase().contains("printer.local"), "{t}");
        assert!(!t.contains("00:1e:8f"), "{t}");
        assert!(t.contains("v1.060") && t.contains("12:30"), "versions and clocks stay: {t}");
        assert!(t.contains("<printer>") && t.contains("<address>"), "{t}");
        assert_eq!(scrub("plain", ""), "plain");
    }

    #[test]
    fn no_printer_is_a_problem_with_a_way_out() {
        for lang in [Language::En, Language::Ja] {
            let c = no_printer_check(lang);
            assert_eq!((c.id.as_str(), c.status), ("printer.none", Status::Fail));
            assert!(c.hint.contains("washi discover") && c.hint.contains("set-host"), "{c:?}");
        }
    }

    #[test]
    fn locales_are_found_by_name() {
        let names: Vec<String> =
            ["C", "C.utf8", "en_US.utf8", "ja_JP.utf8", "POSIX"].map(String::from).into();
        let v = locale_checks_from(Language::En, Some(&names));
        assert_eq!((v[0].status, v[0].detail.as_str()), (Status::Ok, "en_US.utf8"));
        assert_eq!((v[1].status, v[1].detail.as_str()), (Status::Ok, "ja_JP.utf8"));
        let none: Vec<String> = ["C", "POSIX", "ja_JP.eucJP"].map(String::from).into();
        let v = locale_checks_from(Language::En, Some(&none));
        assert_eq!(v[0].status, Status::Skip, "no UTF-8 English locale at all");
        assert_eq!(v[1].status, Status::Skip, "EUC-JP is not the UTF-8 Japanese locale");
        assert!(!v[1].hint.is_empty());
        let v = locale_checks_from(Language::Ja, None);
        assert!(v.iter().all(|c| c.status == Status::Skip && !c.detail.is_ascii()));
        let c_utf8 = locale_checks_from(Language::En, Some(&["C.UTF-8".to_string()]));
        assert_eq!(c_utf8[0].status, Status::Ok, "C.UTF-8 is a usable English locale");
    }

    #[test]
    fn missing_tools_are_optional_and_say_how_to_get_them() {
        let none = Tools::default();
        let v = tools_checks(Language::En, &none);
        assert_eq!(v.len(), 3);
        assert!(v.iter().all(|c| c.status == Status::Skip && !c.hint.is_empty()), "{v:?}");
        let ja = tools_checks(Language::Ja, &none);
        assert!(ja.iter().all(|c| !c.label.is_ascii() && !c.hint.is_empty()));
        let some = Tools {
            pdftoppm: Some("/usr/bin/pdftoppm".into()),
            pdfinfo: Some("/usr/bin/pdfinfo".into()),
            ..Tools::default()
        };
        assert_eq!(tools_checks(Language::En, &some)[0].status, Status::Ok);
        let half = Tools { pdftoppm: Some("/usr/bin/pdftoppm".into()), ..Tools::default() };
        assert!(tools_checks(Language::En, &half)[0].detail.contains("pdfinfo"));
    }

    #[test]
    fn files_are_reported_without_the_home_folder() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("config.toml");
        let v = files_checks(Language::En, Some(&cfg), Some(&dir.path().join("cache")));
        assert_eq!((v[0].status, v[1].status), (Status::Skip, Status::Skip), "{v:?}");
        std::fs::write(&cfg, "[ui]\nlanguage = \"ja\"\n").unwrap();
        std::fs::create_dir(dir.path().join("cache")).unwrap();
        let v = files_checks(Language::En, Some(&cfg), Some(&dir.path().join("cache")));
        assert_eq!((v[0].status, v[1].status), (Status::Ok, Status::Ok), "{v:?}");
        std::fs::write(&cfg, "this is [not toml").unwrap();
        let v = files_checks(Language::En, Some(&cfg), None);
        assert_eq!((v[0].status, v[1].status), (Status::Fail, Status::Skip));
        assert!(!v[0].hint.is_empty());
    }

    #[test]
    fn the_report_counts_and_prints_every_check() {
        let g = Group::new(
            "g",
            "Group",
            vec![
                Check::ok("a", "Fine", "yes"),
                Check::fail("b", "Broken", "no", "do this"),
                Check::skip("c", "Optional", "", ""),
            ],
        );
        let r = Report::new("washi", Language::En, vec![g]);
        assert_eq!(r.summary, Summary { ok: 1, fail: 1, skip: 1 });
        let t = r.render_text();
        assert!(t.contains("\u{2713} Fine: yes") && t.contains("\u{2717} Broken: no"), "{t}");
        assert!(t.contains("      do this") && t.contains("\u{2013} Optional"), "{t}");
        assert!(t.contains("1 problem,"), "{t}");
        assert_eq!(r.check("b").unwrap().status, Status::Fail);
        let json = serde_json::to_string(&r).unwrap();
        assert!(json.contains("\"status\":\"fail\""), "{json}");
        let back: Report = serde_json::from_str(&json).unwrap();
        assert_eq!(back, r);
        let ja = Report::new("washid", Language::Ja, vec![]);
        assert!(ja.render_text().contains("問題あり"));
    }
}
