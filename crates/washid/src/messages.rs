// SPDX-License-Identifier: GPL-2.0-or-later
//! The texts the daemon itself shows to people, in English and Japanese: desktop notifications,
//! the messages of D-Bus errors, and the progress and result messages of jobs. The app shows
//! these as they are, so they follow the language the app last told the daemon about
//! (`SetLanguage`, or `[ui] language` at start).
//!
//! Every message is a function that builds both languages side by side, so a change to one is
//! hard to make without seeing the other. The unit tests at the end check that no Japanese text
//! slipped into English and that every Japanese text really is Japanese.

use std::fmt;

use washi_core::ivec::{CleaningType, InkGroup, IvecError};
use washi_core::net::NetError;
use washi_core::net::escl::EsclError;
use washi_core::net::ipp::IppError;
use washi_core::{Error, Language, MaintOp};

/// A user-facing text in both languages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Text {
    en: String,
    ja: String,
}

impl Text {
    pub fn new(en: impl Into<String>, ja: impl Into<String>) -> Text {
        Text { en: en.into(), ja: ja.into() }
    }

    pub fn get(&self, lang: Language) -> &str {
        match lang {
            Language::En => &self.en,
            Language::Ja => &self.ja,
        }
    }

    pub fn en(&self) -> &str {
        &self.en
    }
}

/// The English text, which is also what logs and `Display` show.
impl fmt::Display for Text {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.en)
    }
}

// ---- ink and printers --------------------------------------------------------------------------

/// The ink colour as people say it.
pub fn ink_name(lang: Language, code: &str) -> String {
    match (lang, code) {
        (Language::En, c) => washi_core::ink_display_name(c).to_string(),
        (Language::Ja, "BK") => "ブラック".into(),
        (Language::Ja, "PGBK") => "顔料ブラック".into(),
        (Language::Ja, "C") => "シアン".into(),
        (Language::Ja, "M") => "マゼンタ".into(),
        (Language::Ja, "Y") => "イエロー".into(),
        (Language::Ja, "GY") => "グレー".into(),
        (Language::Ja, c) => c.to_string(),
    }
}

/// What to call a printer whose model is not known.
pub fn generic_printer(lang: Language) -> &'static str {
    match lang {
        Language::En => "Canon printer",
        Language::Ja => "Canonプリンター",
    }
}

// ---- notifications -----------------------------------------------------------------------------

pub fn back_online_title(lang: Language, label: &str) -> String {
    match lang {
        Language::En => format!("{label} is back online"),
        Language::Ja => format!("{label}がオンラインに戻りました"),
    }
}

pub fn back_online_body(lang: Language) -> String {
    match lang {
        Language::En => "The printer is reachable again.".into(),
        Language::Ja => "プリンターに再び接続できました。".into(),
    }
}

pub fn ink_low_title(lang: Language, name: &str) -> String {
    match lang {
        Language::En => format!("{name} ink is low"),
        Language::Ja => format!("{name}インクの残りが少なくなっています"),
    }
}

pub fn ink_low_body(lang: Language, label: &str, name: &str, level: Option<u8>) -> String {
    match (lang, level) {
        (Language::En, Some(l)) => {
            format!("{label}: the {name} tank is at {l}%. Have a replacement ready.")
        }
        (Language::En, None) => {
            format!("{label}: the {name} tank is running low. Have a replacement ready.")
        }
        (Language::Ja, Some(l)) => {
            format!("{label}：{name}のインクタンクの残量は{l}%です。交換用を用意してください。")
        }
        (Language::Ja, None) => {
            format!(
                "{label}：{name}のインクタンクの残りが少なくなっています。交換用を用意してください。"
            )
        }
    }
}

pub fn ink_empty_title(lang: Language, name: &str) -> String {
    match lang {
        Language::En => format!("{name} ink is empty"),
        Language::Ja => format!("{name}インクがなくなりました"),
    }
}

pub fn ink_empty_body(lang: Language, label: &str, name: &str) -> String {
    match lang {
        Language::En => format!("{label}: the {name} tank is empty. Replace it to keep printing."),
        Language::Ja => {
            format!("{label}：{name}のインクタンクが空です。印刷を続けるには交換してください。")
        }
    }
}

pub fn error_title(lang: Language, label: &str, code: &str) -> String {
    match lang {
        Language::En => format!("{label}: support code {code}"),
        Language::Ja => format!("{label}：サポート番号 {code}"),
    }
}

/// What a printer's support code means: the text of its family in `washi_core::support` (the
/// table holds both languages), or a generic line that names the code.
pub fn support_message(lang: Language, code: &str) -> String {
    washi_core::support::message(code, lang)
}

// ---- jobs --------------------------------------------------------------------------------------

/// What a running job is called in the "busy" message and in the snapshot's `activity`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobKind {
    Maintenance,
    Scan,
    Print,
    DiscLabel,
}

impl JobKind {
    pub fn text(self) -> Text {
        match self {
            JobKind::Maintenance => Text::new("maintenance", "メンテナンス"),
            JobKind::Scan => Text::new("scan", "スキャン"),
            JobKind::Print => Text::new("print", "印刷"),
            JobKind::DiscLabel => Text::new("disc label", "ディスクレーベル"),
        }
    }
}

/// `kind: label`, as the snapshot's `activity` says it.
pub fn activity(lang: Language, kind: JobKind, label: &Text) -> String {
    let kind = kind.text();
    match lang {
        Language::En => format!("{}: {}", kind.en(), label.en()),
        Language::Ja => format!("{}：{}", kind.get(lang), label.get(lang)),
    }
}

/// `A4 at 300 dpi`.
pub fn scan_label(size: &str, dpi: u32) -> Text {
    Text::new(format!("{size} at {dpi} dpi"), format!("{size}、{dpi} dpi"))
}

/// What a maintenance operation is called.
pub fn maint_label(op: &MaintOp) -> Text {
    let group = |g: InkGroup| match g {
        InkGroup::All => "すべて",
        InkGroup::Group1 => "グループ1",
        InkGroup::Group2 => "グループ2",
    };
    match op {
        MaintOp::Clean { kind: CleaningType::Roller, .. } => {
            Text::new(op.label(), "給紙ローラークリーニング")
        }
        MaintOp::Clean { kind: CleaningType::Platen, .. } => {
            Text::new(op.label(), "底面クリーニング")
        }
        MaintOp::Clean { kind, group: g } => {
            let kind_ja = match kind {
                CleaningType::Deep => "強力クリーニング",
                _ => "クリーニング",
            };
            Text::new(op.label(), format!("{kind_ja}（インクグループ：{}）", group(*g)))
        }
        MaintOp::NozzleCheck => Text::new(op.label(), "ノズルチェック"),
        MaintOp::AlignAuto => Text::new(op.label(), "プリントヘッド位置調整（自動）"),
        MaintOp::AlignCheck => Text::new(op.label(), "位置調整の確認"),
        MaintOp::AlignManual(id) => {
            Text::new(op.label(), format!("プリントヘッド位置調整（手動）のパターン {id}"))
        }
        MaintOp::RollerClean => Text::new(op.label(), "給紙ローラークリーニング"),
    }
}

/// What the printer is doing during a maintenance job, from its `jobinfo`
/// (`maintenance_operation`, `type`); `None` when it says nothing useful.
pub fn job_phrase(lang: Language, operation: &str, ty: &str) -> String {
    let (en, ja) = match (operation, ty) {
        ("TestPrint", "nozzle_check") => {
            ("Printing the nozzle check pattern", "ノズルチェックパターンを印刷中")
        }
        ("TestPrint", "auto_registration") => ("Alignment in progress", "位置調整を実行中"),
        ("TestPrint", "manual_registration") => {
            ("Printing the alignment pattern", "位置調整パターンを印刷中")
        }
        ("TestPrint", "regi_check") => ("Printing the alignment check", "位置調整の確認を印刷中"),
        ("TestPrint", _) => ("Printing the test pattern", "テストパターンを印刷中"),
        ("Cleaning", "deep") => ("Deep cleaning in progress", "強力クリーニング中"),
        ("Cleaning", "roller") => {
            ("Cleaning the paper feed rollers", "給紙ローラーをクリーニング中")
        }
        ("Cleaning", "platen") => ("Cleaning the bottom plate", "底面をクリーニング中"),
        ("Cleaning", _) => ("Cleaning in progress", "クリーニング中"),
        ("RollerCleaning", _) => {
            ("Cleaning the paper feed rollers", "給紙ローラーをクリーニング中")
        }
        _ => ("Working", "処理中"),
    };
    match lang {
        Language::En => en.to_string(),
        Language::Ja => ja.to_string(),
    }
}

/// The printer's own words for what it is doing (`printing`, `processing`, ...).
fn printer_state(lang: Language, status: &str) -> String {
    if lang == Language::En {
        return status.to_string();
    }
    match status {
        "idle" => "待機中",
        "printing" => "印刷中",
        "processing" => "処理中",
        "working" => "動作中",
        "pending" => "待機中",
        "completed" => "完了",
        "canceled" | "cancelled" => "取り消し済み",
        "aborted" => "中断",
        "stopped" | "processing-stopped" => "停止中",
        "pending-held" => "保留中",
        other => other,
    }
    .to_string()
}

pub fn job_started(lang: Language) -> String {
    match lang {
        Language::En => "The printer accepted the job.".into(),
        Language::Ja => "プリンターがジョブを受け付けました。".into(),
    }
}

pub fn command_accepted(lang: Language, op: &MaintOp) -> String {
    match lang {
        Language::En => format!("The printer started: {}.", op.label()),
        Language::Ja => format!("プリンターが開始しました：{}。", maint_label(op).get(lang)),
    }
}

/// A progress line while the printer works: `phrase` (what `jobinfo` says), or else the state.
pub fn waiting(
    lang: Language,
    secs: u64,
    phrase: Option<String>,
    status: &str,
    support_code: &str,
) -> String {
    let mut m = match (phrase, lang) {
        (Some(p), Language::En) => format!("{p}… ({secs}s)"),
        (Some(p), Language::Ja) => format!("{p}…（{secs}秒）"),
        (None, Language::En) => format!("{secs}s: the printer is {status}"),
        (None, Language::Ja) => {
            format!("{secs}秒：プリンターは{}です", printer_state(lang, status))
        }
    };
    if !support_code.is_empty() {
        match lang {
            Language::En => m.push_str(&format!(", support code {support_code}")),
            Language::Ja => m.push_str(&format!("、サポート番号 {support_code}")),
        }
    }
    m
}

pub fn job_ended(lang: Language) -> String {
    match lang {
        Language::En => "The printer is idle again.".into(),
        Language::Ja => "プリンターは待機状態に戻りました。".into(),
    }
}

pub fn closing_job(lang: Language) -> String {
    match lang {
        Language::En => "Closing the job.".into(),
        Language::Ja => "ジョブを終了しています。".into(),
    }
}

pub fn maintenance_done(lang: Language, op: &MaintOp, secs: u64) -> String {
    match lang {
        Language::En => format!("{} finished in {secs}s.", op.label()),
        Language::Ja => format!("{}が完了しました（{secs}秒）。", maint_label(op).get(lang)),
    }
}

// ---- firmware ------------------------------------------------------------------------------------

/// What the firmware update is called as a job.
pub fn firmware_label() -> Text {
    Text::new("firmware update", "ファームウェアの更新")
}

pub fn firmware_sent(lang: Language) -> String {
    match lang {
        Language::En => "The printer accepted the update. It downloads and installs the firmware by itself: do not switch it off.".into(),
        Language::Ja => "プリンターが更新を受け付けました。ファームウェアのダウンロードとインストールはプリンター自身が行います。電源を切らないでください。".into(),
    }
}

pub fn firmware_restarting(lang: Language) -> String {
    match lang {
        Language::En => {
            "The printer is restarting to finish the update. This takes a few minutes.".into()
        }
        Language::Ja => "更新を完了するため、プリンターが再起動しています。数分かかります。".into(),
    }
}

pub fn firmware_waiting(lang: Language, secs: u64, restarted: bool) -> String {
    let (m, s) = (secs / 60, secs % 60);
    match (lang, restarted) {
        (Language::En, false) => {
            format!("Waiting for the printer to start installing… ({m} min {s} s)")
        }
        (Language::En, true) => format!("Waiting for the printer to come back… ({m} min {s} s)"),
        (Language::Ja, false) => {
            format!("プリンターがインストールを始めるのを待っています…（{m}分{s}秒）")
        }
        (Language::Ja, true) => format!("プリンターが戻るのを待っています…（{m}分{s}秒）"),
    }
}

pub fn firmware_back(lang: Language) -> String {
    match lang {
        Language::En => "The printer is back. Reading its firmware version…".into(),
        Language::Ja => {
            "プリンターが戻りました。ファームウェアのバージョンを読み取っています…".into()
        }
    }
}

/// The result line of a finished update (it did not fail).
pub fn firmware_done(lang: Language, r: &washi_core::FirmwareReport) -> String {
    let (before, after) = (r.before.as_str(), r.after.as_str());
    match (lang, r.restarted, before == after) {
        (Language::En, false, _) => format!(
            "The printer did not restart, so nothing was installed. The firmware version is {after}."
        ),
        (Language::Ja, false, _) => format!(
            "プリンターが再起動しなかったため、何もインストールされていません。ファームウェアのバージョンは {after} です。"
        ),
        (Language::En, true, true) => {
            format!("The printer restarted, but its firmware version is still {after}.")
        }
        (Language::Ja, true, true) => format!(
            "プリンターは再起動しましたが、ファームウェアのバージョンは {after} のままです。"
        ),
        (Language::En, true, false) => {
            format!("The firmware was updated from {before} to {after}.")
        }
        (Language::Ja, true, false) => {
            format!("ファームウェアを {before} から {after} に更新しました。")
        }
    }
}

pub fn pages_scanned(lang: Language, n: usize) -> String {
    match (lang, n) {
        (Language::En, 1) => "1 page scanned.".into(),
        (Language::En, n) => format!("{n} pages scanned."),
        (Language::Ja, n) => format!("{n}ページをスキャンしました。"),
    }
}

pub fn print_preflight(lang: Language) -> String {
    match lang {
        Language::En => "Checking what the printer accepts.".into(),
        Language::Ja => "プリンターが受け付ける内容を確認しています。".into(),
    }
}

pub fn print_ready(lang: Language, job: usize, jobs: usize) -> String {
    match lang {
        Language::En => format!("Job {job} of {jobs} is ready to send."),
        Language::Ja => format!("ジョブ {job}/{jobs} の送信準備ができました。"),
    }
}

pub fn print_preparing(lang: Language, job: usize, jobs: usize, side: u32, sides: u32) -> String {
    match lang {
        Language::En => format!("Preparing job {job} of {jobs}: sheet side {side} of {sides}."),
        Language::Ja => format!("ジョブ {job}/{jobs} を準備中（用紙の面 {side}/{sides}）。"),
    }
}

pub fn print_validated(lang: Language, ignored: &[String]) -> String {
    match (lang, ignored.is_empty()) {
        (Language::En, true) => "The printer accepts the job.".into(),
        (Language::En, false) => {
            format!("The printer accepts the job but will ignore: {}.", ignored.join(", "))
        }
        (Language::Ja, true) => "プリンターがジョブを受け付けました。".into(),
        (Language::Ja, false) => {
            format!(
                "プリンターはジョブを受け付けますが、次の指定は無視されます：{}。",
                ignored.join("、")
            )
        }
    }
}

pub fn print_sending(lang: Language, bytes: u64) -> String {
    match lang {
        Language::En => format!("Sending {bytes} bytes."),
        Language::Ja => format!("{bytes}バイトを送信しています。"),
    }
}

pub fn print_submitted(lang: Language, id: &str) -> String {
    match lang {
        Language::En => format!("The printer took the job (id {id})."),
        Language::Ja => format!("プリンターがジョブを受け取りました（ID {id}）。"),
    }
}

pub fn print_state(
    lang: Language,
    id: &str,
    state: &str,
    pages_done: Option<u32>,
    reasons: &[&str],
) -> String {
    let st = printer_state(lang, state);
    match lang {
        Language::En => format!(
            "Job {id}: {st}{}{}.",
            pages_done.filter(|n| *n > 0).map_or(String::new(), |n| format!(", {n} page(s) done")),
            if reasons.is_empty() { String::new() } else { format!(" ({})", reasons.join(", ")) }
        ),
        Language::Ja => format!(
            "ジョブ {id}：{st}{}{}。",
            pages_done.filter(|n| *n > 0).map_or(String::new(), |n| format!("、{n}ページ完了")),
            if reasons.is_empty() {
                String::new()
            } else {
                format!("（{}）", reasons.join("、"))
            }
        ),
    }
}

pub fn print_finished(lang: Language, id: &str, state: &str) -> String {
    match lang {
        Language::En => format!("Job {id} {state}."),
        Language::Ja => format!("ジョブ {id}：{}。", printer_state(lang, state)),
    }
}

/// The result of a finished print job: `file: state` for each file.
pub fn print_result(lang: Language, items: &[(String, &str)]) -> String {
    let sep = if lang == Language::En { "; " } else { "、" };
    items
        .iter()
        .map(|(label, state)| match lang {
            Language::En => format!("{label}: {state}"),
            Language::Ja => format!("{label}：{}", printer_state(lang, state)),
        })
        .collect::<Vec<_>>()
        .join(sep)
}

pub fn disc_result(lang: Language, state: &str) -> String {
    match lang {
        Language::En => format!("disc label: {state}"),
        Language::Ja => format!("ディスクレーベル：{}", printer_state(lang, state)),
    }
}

pub fn disc_label_name() -> Text {
    Text::new("disc label", "ディスクレーベル")
}

// ---- errors ------------------------------------------------------------------------------------

pub fn read_only(lang: Language, what: &str) -> String {
    match lang {
        Language::En => format!("washid is running with --read-only: {what} is disabled"),
        Language::Ja => format!(
            "washid が読み取り専用モード（--read-only）で動作しているため、{what} は使えません。"
        ),
    }
}

pub fn unknown_printer(lang: Language, key: &str) -> String {
    match lang {
        Language::En => {
            format!("unknown printer {key:?}: call Discover() or SetActivePrinter() first")
        }
        Language::Ja => format!(
            "プリンター「{key}」は登録されていません。先にプリンターを検索するか、選択してください。"
        ),
    }
}

pub fn no_printer(lang: Language) -> String {
    match lang {
        Language::En => "no printer is configured: call Discover() or SetActivePrinter()".into(),
        Language::Ja => {
            "プリンターが設定されていません。先にプリンターを検索するか、選択してください。".into()
        }
    }
}

pub fn not_confirmed(lang: Language, what: &str) -> String {
    match lang {
        Language::En => format!(
            "{what} writes to the printer: pass \"confirm\": true after the user has agreed"
        ),
        Language::Ja => format!(
            "{what} はプリンターに書き込みます。ユーザーの同意を得てから \"confirm\": true を渡してください。"
        ),
    }
}

pub fn invalid_args(detail: &str) -> Text {
    Text::new(detail, format!("指定内容が正しくありません：{detail}"))
}

pub fn busy_maintenance() -> Text {
    Text::new("a maintenance job is running", "メンテナンスを実行中です。")
}

pub fn busy_printer() -> Text {
    Text::new("the printer is busy", "プリンターが他の処理を実行中です。")
}

pub fn busy_with(activity: &Text) -> Text {
    Text::new(
        format!("a {} is running", activity.en()),
        format!("ほかの処理（{}）を実行中です。", activity.get(Language::Ja)),
    )
}

pub fn busy_with_here(activity: &Text) -> Text {
    Text::new(
        format!("a {} is running on this printer", activity.en()),
        format!("このプリンターで次の処理を実行中です：{}", activity.get(Language::Ja)),
    )
}

pub fn busy_job_still_running(id: &str, activity: &Text) -> Text {
    Text::new(
        format!("{id} ({}) is still running on this printer", activity.en()),
        format!("{id}（{}）がこのプリンターでまだ実行中です。", activity.get(Language::Ja)),
    )
}

fn hint(e: &Error) -> Option<Text> {
    let t = |en: &'static str, ja: &'static str| Some(Text::new(en, ja));
    match e {
        Error::Ivec(IvecError::Ng { detail, .. }) => match detail.as_str() {
            "Suspended" => t(
                "the printer is asleep (its print service is suspended); wake it with its power button or by printing something, then try again",
                "プリンターがスリープ中です（印刷サービスが停止しています）。電源ボタンを押すか、何か印刷してから、もう一度お試しください。",
            ),
            "DeviceBusy" => t(
                "the printer is busy; wait for it to finish and try again",
                "プリンターが使用中です。終わるまで待ってから、もう一度お試しください。",
            ),
            "NotSupportService" => t(
                "this printer does not offer that service on this port",
                "このプリンターは、このポートではその機能を提供していません。",
            ),
            _ => None,
        },
        Error::WithCleanupFailure { error, .. } => hint(error),
        Error::Net(NetError::Ipp(IppError::Rejected { code: 0x040B | 0x040E, .. })) => t(
            "the printer does not accept that paper size / type / tray / quality combination; run `washi media` to see what it offers",
            "用紙サイズ、用紙の種類、給紙元、印刷品質の組み合わせがプリンターで受け付けられません。`washi media` で対応内容を確認してください。",
        ),
        Error::Net(NetError::Ipp(IppError::Rejected { code: 0x040A, .. })) => t(
            "the printer does not take that file format; PDF and Office files are rasterised by washi, images are sent as JPEG",
            "プリンターがこのファイル形式に対応していません。PDF と Office のファイルは Washi が画像に変換し、画像は JPEG で送ります。",
        ),
        Error::Net(NetError::Ipp(IppError::Rejected { code: 0x0506, .. })) => t(
            "the printer is not accepting jobs right now (check its display for an error)",
            "プリンターは今ジョブを受け付けていません。本体の表示でエラーを確認してください。",
        ),
        Error::Net(NetError::Escl(EsclError::JobRejected { status: 409, .. })) => t(
            "close the scanner cover, and wait until the printer is idle",
            "原稿台カバーを閉じ、プリンターが待機状態になるまで待ってください。",
        ),
        _ => None,
    }
}

/// The Japanese headline of an error. Low-level details (operating system messages, protocol
/// names) are kept as they are in parentheses: they are what a bug report needs.
fn headline_ja(e: &Error) -> String {
    match e {
        Error::Net(NetError::Connect { source, .. }) => {
            format!("プリンターに接続できません（{source}）")
        }
        Error::Net(NetError::Io(io)) => format!("プリンターとの通信でエラーが発生しました（{io}）"),
        Error::Net(NetError::Http(m)) => format!("プリンターの応答を解釈できませんでした（{m}）"),
        Error::Net(NetError::Status { status }) => {
            format!("プリンターが HTTP {status} を返しました")
        }
        Error::Net(NetError::Timeout(_)) => "プリンターから時間内に応答がありませんでした".into(),
        Error::Net(NetError::NotReadOnly(what)) => {
            format!("{what} はプリンターを変更するため、確認なしでは実行できません")
        }
        Error::Net(NetError::Mdns(m)) => {
            format!("ネットワーク上のプリンターを検索できませんでした（{m}）")
        }
        Error::Net(NetError::Ivec(i)) | Error::Ivec(i) => ivec_ja(i),
        Error::Net(NetError::Ipp(IppError::Rejected { name, .. })) => {
            format!("プリンターが要求を受け付けませんでした（{name}）")
        }
        Error::Net(NetError::Ipp(other)) => {
            format!("プリンターから想定外の応答がありました（{other}）")
        }
        Error::Net(NetError::Escl(EsclError::Busy)) => "スキャナーが使用中です".into(),
        Error::Net(NetError::Escl(EsclError::JobRejected { status, .. })) => {
            format!("スキャナーがスキャンを開始できませんでした（HTTP {status}）")
        }
        Error::Net(NetError::Escl(EsclError::NoLocation)) => {
            "スキャナーからジョブの場所が返されませんでした".into()
        }
        Error::Net(NetError::Escl(EsclError::Invalid(m))) => {
            format!("スキャンの設定が正しくありません（{m}）")
        }
        Error::Net(NetError::Escl(EsclError::Parse(m))) => {
            format!("スキャナーの応答を解釈できませんでした（{m}）")
        }
        Error::Invalid(m) => format!("指定内容が正しくありません：{m}"),
        Error::Timeout(m) => format!("時間内に完了しませんでした（{m}）"),
        Error::Mismatch(_) => format!("プリンターが新しい設定を保持しませんでした（{e}）"),
        Error::Config { path, msg } => format!("設定ファイルを読み込めません（{path}: {msg}）"),
        Error::NoPrinter => {
            "対象のプリンターを検出できませんでした。ホスト名を指定するか、プリンターがネットワークにつながっているか確認してください".into()
        }
        Error::WithCleanupFailure { error, cleanup } => {
            format!("{}（さらに、ジョブの終了処理にも失敗しました：{cleanup}）", headline_ja(error))
        }
        Error::Io { context, source } => {
            format!("ファイルを処理できませんでした（{context}: {source}）")
        }
        Error::Image(m) => format!("画像を処理できませんでした（{m}）"),
        Error::Tool { tool, msg } => format!("外部ツール {tool} がエラーを返しました（{msg}）"),
        Error::Cancelled => "中止しました".into(),
    }
}

fn ivec_ja(e: &IvecError) -> String {
    match e {
        IvecError::Xml(m) | IvecError::Structure(m) => {
            format!("プリンターの応答を解釈できませんでした（{m}）")
        }
        IvecError::Ng { operation, detail } => {
            format!("プリンターが {operation} を受け付けませんでした（{detail}）")
        }
        IvecError::Invalid { field, reason } => {
            format!("{field} の値が正しくありません（{reason}）")
        }
    }
}

/// The message of an error that came from the printer or the libraries: the English text with
/// the plain-language hint (as the CLI shows it), and its Japanese counterpart.
pub fn core_error(e: &Error) -> Text {
    let h = hint(e);
    let en = match &h {
        Some(h) => format!("{e} ({})", h.en()),
        None => e.to_string(),
    };
    let ja = match &h {
        Some(h) => format!("{}。{}", headline_ja(e), h.get(Language::Ja)),
        None => format!("{}。", headline_ja(e)),
    };
    Text::new(en, ja)
}

// ---- language ----------------------------------------------------------------------------------

pub fn bad_language(value: &str) -> Text {
    Text::new(
        format!("unknown language {value:?}: use \"en\", \"ja\" or \"system\""),
        format!(
            "言語「{value}」には対応していません。\"en\"、\"ja\"、\"system\" のいずれかを指定してください。"
        ),
    )
}

#[cfg(test)]
mod tests {
    use washi_core::ivec::IvecError;

    use super::*;

    fn has_kana_or_kanji(s: &str) -> bool {
        s.chars().any(|c| matches!(c, '\u{3040}'..='\u{30FF}' | '\u{4E00}'..='\u{9FFF}'))
    }

    fn both(f: impl Fn(Language) -> String) -> (String, String) {
        (f(Language::En), f(Language::Ja))
    }

    /// English texts are English; Japanese texts are Japanese; parameters made it into both.
    fn check(en_ja: (String, String), params: &[&str]) {
        let (en, ja) = en_ja;
        assert!(!en.is_empty() && !has_kana_or_kanji(&en), "English: {en}");
        assert!(has_kana_or_kanji(&ja), "Japanese: {ja}");
        for p in params {
            assert!(en.contains(p), "{p} missing from {en}");
            assert!(ja.contains(p), "{p} missing from {ja}");
        }
        // Japanese typography: no ASCII sentence punctuation, no space before Japanese text
        // other than inside a Latin phrase.
        assert!(!ja.contains(". ") && !ja.ends_with('.'), "ASCII full stop in {ja}");
        assert!(!ja.contains("  "), "double space in {ja}");
    }

    #[test]
    fn notifications_are_translated() {
        check(both(|l| back_online_title(l, "Canon XK140 series")), &["Canon XK140 series"]);
        check(both(back_online_body), &[]);
        check(both(|l| ink_low_title(l, &ink_name(l, "C"))), &[]);
        check(
            both(|l| ink_low_body(l, "Canon XK140 series", &ink_name(l, "C"), Some(10))),
            &["10%"],
        );
        check(both(|l| ink_low_body(l, "Canon XK140 series", &ink_name(l, "PGBK"), None)), &[]);
        check(both(|l| ink_empty_title(l, &ink_name(l, "Y"))), &[]);
        check(both(|l| ink_empty_body(l, "Canon XK140 series", &ink_name(l, "M"))), &[]);
        check(both(|l| error_title(l, "Canon XK140 series", "1300")), &["1300"]);
        for code in ["1000", "1300", "1303", "1600", "1688", "1890", "5B00", "D109", "9999"] {
            check(both(|l| support_message(l, code)), &[]);
        }
        assert!(support_message(Language::En, "1300").starts_with("Paper is jammed."));
        assert!(support_message(Language::Ja, "1300").starts_with("用紙がつまっています"));
        assert!(support_message(Language::En, "9999").contains("9999"));
        assert!(support_message(Language::Ja, "9999").contains("サポート番号 9999"));
        assert_eq!(generic_printer(Language::En), "Canon printer");
    }

    #[test]
    fn firmware_messages_are_translated() {
        use washi_core::FirmwareReport;
        let report = |restarted, before: &str, after: &str| FirmwareReport {
            before: before.into(),
            after: after.into(),
            restarted,
            elapsed: std::time::Duration::from_secs(300),
        };
        check(both(firmware_sent), &[]);
        check(both(firmware_restarting), &[]);
        check(both(firmware_back), &[]);
        check(both(|l| firmware_waiting(l, 200, false)), &["3", "20"]);
        check(both(|l| firmware_waiting(l, 200, true)), &["3", "20"]);
        check(both(|l| firmware_done(l, &report(true, "1.060", "1.061"))), &["1.060", "1.061"]);
        check(both(|l| firmware_done(l, &report(true, "1.060", "1.060"))), &["1.060"]);
        check(both(|l| firmware_done(l, &report(false, "1.060", "1.060"))), &["1.060"]);
        assert!(has_kana_or_kanji(firmware_label().get(Language::Ja)));
        assert_eq!(
            firmware_done(Language::En, &report(true, "1.060", "1.061")),
            "The firmware was updated from 1.060 to 1.061."
        );
    }

    #[test]
    fn job_messages_are_translated() {
        let clean = MaintOp::Clean { kind: CleaningType::Deep, group: InkGroup::All };
        for op in [
            MaintOp::NozzleCheck,
            MaintOp::AlignAuto,
            MaintOp::AlignCheck,
            MaintOp::AlignManual(2),
            MaintOp::RollerClean,
            MaintOp::Clean { kind: CleaningType::Roller, group: InkGroup::All },
            MaintOp::Clean { kind: CleaningType::Platen, group: InkGroup::All },
            clean.clone(),
        ] {
            let t = maint_label(&op);
            assert_eq!(t.en(), op.label());
            assert!(has_kana_or_kanji(t.get(Language::Ja)), "{t}");
            check(both(|l| command_accepted(l, &op)), &[]);
            check(both(|l| maintenance_done(l, &op, 21)), &["21"]);
        }
        assert!(maint_label(&clean).get(Language::Ja).starts_with("強力クリーニング"));
        for (o, t) in [
            ("TestPrint", "nozzle_check"),
            ("TestPrint", "auto_registration"),
            ("TestPrint", "manual_registration"),
            ("TestPrint", "regi_check"),
            ("TestPrint", "other"),
            ("Cleaning", "deep"),
            ("Cleaning", "regular"),
            ("Cleaning", "roller"),
            ("Cleaning", "platen"),
            ("RollerCleaning", ""),
            ("Mystery", ""),
        ] {
            check(both(|l| job_phrase(l, o, t)), &[]);
            check(both(|l| waiting(l, 8, Some(job_phrase(l, o, t)), "", "")), &["8"]);
        }
        check(both(|l| waiting(l, 8, None, "printing", "1300")), &["8", "1300"]);
        check(both(job_started), &[]);
        check(both(job_ended), &[]);
        check(both(closing_job), &[]);
        check(both(|l| pages_scanned(l, 3)), &["3"]);
        check(both(print_preflight), &[]);
        check(both(|l| print_ready(l, 1, 2)), &["1", "2"]);
        check(both(|l| print_preparing(l, 1, 2, 1, 2)), &["1", "2"]);
        check(both(|l| print_validated(l, &[])), &[]);
        check(both(|l| print_validated(l, &["copies".to_string()])), &["copies"]);
        check(both(|l| print_sending(l, 123)), &["123"]);
        check(both(|l| print_submitted(l, "42")), &["42"]);
        check(
            both(|l| print_state(l, "42", "processing", Some(2), &["media-low"])),
            &["42", "media-low"],
        );
        check(both(|l| print_finished(l, "42", "completed")), &["42"]);
        check(both(|l| print_result(l, &[("pic.png".to_string(), "completed")])), &["pic.png"]);
        check(both(|l| disc_result(l, "completed")), &[]);
        check(
            both(|l| activity(l, JobKind::Maintenance, &maint_label(&MaintOp::NozzleCheck))),
            &[],
        );
        assert_eq!(
            activity(Language::En, JobKind::Scan, &scan_label("A4", 300)),
            "scan: A4 at 300 dpi"
        );
    }

    #[test]
    fn errors_are_translated() {
        check(both(|l| read_only(l, "RunMaintenance")), &["RunMaintenance"]);
        check(both(|l| unknown_printer(l, "p.local")), &["p.local"]);
        check(both(no_printer), &[]);
        check(both(|l| not_confirmed(l, "Wake")), &["Wake"]);
        for t in [
            invalid_args("x is not valid"),
            busy_maintenance(),
            busy_printer(),
            busy_with(&Text::new("scan: A4 at 300 dpi", "スキャン：A4、300 dpi")),
            busy_with_here(&Text::new("scan: A4 at 300 dpi", "スキャン：A4、300 dpi")),
            busy_job_still_running("job-1", &Text::new("x", "x")),
            bad_language("fr"),
        ] {
            check((t.en().to_string(), t.get(Language::Ja).to_string()), &[]);
        }
        let ng = Error::Ivec(IvecError::Ng {
            operation: "GetStatus".into(),
            detail: "Suspended".into(),
        });
        let t = core_error(&ng);
        assert!(t.en().contains("asleep") && t.en().contains("Suspended"), "{t}");
        assert!(
            t.get(Language::Ja).contains("スリープ") && t.get(Language::Ja).contains("Suspended")
        );
        let cancelled = core_error(&Error::Cancelled);
        assert_eq!(cancelled.en(), "cancelled");
        assert!(has_kana_or_kanji(cancelled.get(Language::Ja)));
        let invalid = core_error(&Error::Invalid("bad".into()));
        assert_eq!(invalid.en(), "bad");
        assert!(invalid.get(Language::Ja).contains("bad"));
        let timeout = core_error(&Error::Net(NetError::Timeout(std::time::Duration::from_secs(1))));
        assert!(has_kana_or_kanji(timeout.get(Language::Ja)));
        let escl = core_error(&Error::Net(NetError::Escl(EsclError::JobRejected {
            status: 409,
            hint: "x",
        })));
        assert!(
            escl.get(Language::Ja).contains("409") && escl.get(Language::Ja).contains("カバー")
        );
    }

    #[test]
    fn japanese_ink_names() {
        for (code, ja) in
            [("BK", "ブラック"), ("C", "シアン"), ("M", "マゼンタ"), ("Y", "イエロー")]
        {
            assert_eq!(ink_name(Language::Ja, code), ja);
        }
        assert_eq!(ink_name(Language::En, "PGBK"), "Pigment black");
        assert_eq!(ink_name(Language::Ja, "XX"), "XX");
    }
}
