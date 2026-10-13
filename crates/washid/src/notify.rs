// SPDX-License-Identifier: GPL-2.0-or-later
//! Desktop notifications (`org.freedesktop.Notifications`) for ink, errors and the printer
//! coming back online. The decision logic ([`NotifyState`]) is pure and unit-tested; delivery is
//! behind the [`Notifier`] trait.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, PoisonError};

use washi_core::{
    InkSource, InkStatus, Language, NotificationConfig, Power, PrinterSnapshot, Severity,
};
use zbus::zvariant::Value;

use crate::messages;

/// One notification to show. `key` identifies the condition: a newer notification with the same
/// key replaces the older one on the desktop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notification {
    pub key: String,
    pub summary: String,
    pub body: String,
    pub icon: &'static str,
    /// Errors and empty tanks are urgent; the rest is normal.
    pub critical: bool,
}

pub trait Notifier: Send + Sync {
    fn send(&self, n: Notification);
}

/// What has already been announced for one printer. Conditions are announced when they appear
/// and re-armed when they clear, so polling every 30 s never repeats a notification.
#[derive(Debug, Default)]
pub struct NotifyState {
    ink: HashSet<String>,
    error: Option<String>,
    seen_offline: bool,
}

fn printer_label(s: &PrinterSnapshot, lang: Language) -> String {
    match s.device.as_ref().map(|d| d.model.trim()).filter(|m| !m.is_empty()) {
        Some(m) if m.to_ascii_lowercase().starts_with("canon") => m.to_string(),
        Some(m) => format!("Canon {m}"),
        None => messages::generic_printer(lang).to_string(),
    }
}

impl NotifyState {
    /// Look at a fresh snapshot and return the notifications that are new, worded in `lang`.
    /// The state is updated even when notifications are disabled, so switching them on later
    /// does not flood.
    pub fn evaluate(
        &mut self,
        cfg: &NotificationConfig,
        cur: &PrinterSnapshot,
        lang: Language,
    ) -> Vec<Notification> {
        let mut out = Vec::new();
        let label = printer_label(cur, lang);

        // Back online: reachable again after having been seen offline.
        if cur.power == Power::Offline {
            self.seen_offline = true;
        } else if self.seen_offline {
            self.seen_offline = false;
            out.push(Notification {
                key: "online".into(),
                summary: messages::back_online_title(lang, &label),
                body: messages::back_online_body(lang),
                icon: "printer",
                critical: false,
            });
            if !cfg.online {
                out.pop();
            }
        }

        // Ink: only trust levels that were actually read (not carried over from the cache).
        if matches!(cur.ink_source, InkSource::Ivec | InkSource::Ipp) {
            let mut active = HashSet::new();
            let mut fresh = Vec::new();
            for ink in &cur.inks {
                let name = messages::ink_name(lang, &ink.color);
                let (suffix, summary, body, critical) = match ink.status {
                    InkStatus::Low => (
                        "low",
                        messages::ink_low_title(lang, &name),
                        messages::ink_low_body(lang, &label, &name, ink.level),
                        false,
                    ),
                    InkStatus::Empty => (
                        "empty",
                        messages::ink_empty_title(lang, &name),
                        messages::ink_empty_body(lang, &label, &name),
                        true,
                    ),
                    InkStatus::Ok | InkStatus::Unknown => continue,
                };
                let key = format!("ink:{}:{suffix}", ink.color);
                if !self.ink.contains(&key) {
                    fresh.push(Notification {
                        key: key.clone(),
                        summary,
                        body,
                        icon: if critical { "dialog-error" } else { "dialog-warning" },
                        critical,
                    });
                }
                active.insert(key);
            }
            self.ink = active;
            if cfg.ink {
                out.extend(fresh);
            }
        }

        // Errors: only while the print service answers (otherwise the code is unknown).
        if cur.power == Power::On {
            let code = cur.support.as_ref().map(|s| s.code.clone());
            if code != self.error {
                // Notices (severity `info`, e.g. D109) are for the Status tab, not for a popup.
                if let (Some(sup), true) = (&cur.support, cfg.errors) {
                    if sup.severity != Severity::Info {
                        let critical = sup.severity == Severity::Error;
                        out.push(Notification {
                            key: "error".into(),
                            summary: messages::error_title(lang, &label, &sup.code),
                            body: messages::support_message(lang, &sup.code),
                            icon: if critical { "dialog-error" } else { "dialog-warning" },
                            critical,
                        });
                    }
                }
                self.error = code;
            }
        }

        if cfg.enabled { out } else { Vec::new() }
    }
}

/// Sends notifications through the session bus. Delivery happens on the Tokio runtime so the
/// polling thread never waits for the notification daemon; failures are logged at debug level.
pub struct DbusNotifier {
    conn: zbus::Connection,
    handle: tokio::runtime::Handle,
    /// `key` -> notification id, so a repeated condition replaces its earlier notification.
    ids: Arc<Mutex<HashMap<String, u32>>>,
}

#[zbus::proxy(
    interface = "org.freedesktop.Notifications",
    default_service = "org.freedesktop.Notifications",
    default_path = "/org/freedesktop/Notifications"
)]
trait Notifications {
    #[allow(clippy::too_many_arguments)]
    fn notify(
        &self,
        app_name: &str,
        replaces_id: u32,
        app_icon: &str,
        summary: &str,
        body: &str,
        actions: &[&str],
        hints: HashMap<&str, Value<'_>>,
        expire_timeout: i32,
    ) -> zbus::Result<u32>;
}

impl DbusNotifier {
    /// Must be called from inside the Tokio runtime.
    pub fn new(conn: zbus::Connection) -> Self {
        DbusNotifier {
            conn,
            handle: tokio::runtime::Handle::current(),
            ids: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

impl Notifier for DbusNotifier {
    fn send(&self, n: Notification) {
        let (conn, ids) = (self.conn.clone(), self.ids.clone());
        self.handle.spawn(async move {
            let replaces = ids.lock().unwrap_or_else(PoisonError::into_inner).get(&n.key).copied();
            let proxy = match NotificationsProxy::new(&conn).await {
                Ok(p) => p,
                Err(e) => {
                    tracing::debug!("no notification service: {e}");
                    return;
                }
            };
            let mut hints: HashMap<&str, Value<'_>> = HashMap::new();
            hints.insert("urgency", Value::U8(if n.critical { 2 } else { 1 }));
            hints.insert("desktop-entry", Value::from("io.github.Jenari_Dev.Washi"));
            let timeout = if n.critical { 0 } else { 10_000 };
            match proxy
                .notify(
                    "Washi",
                    replaces.unwrap_or(0),
                    n.icon,
                    &n.summary,
                    &n.body,
                    &[],
                    hints,
                    timeout,
                )
                .await
            {
                Ok(id) => {
                    ids.lock().unwrap_or_else(PoisonError::into_inner).insert(n.key.clone(), id);
                    tracing::info!("notification sent: {}", n.key);
                }
                Err(e) => tracing::debug!("notification failed: {e}"),
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use washi_core::{DeviceInfo, InkLevel, Support};

    use super::*;

    const EN: Language = Language::En;
    const JA: Language = Language::Ja;

    fn ink(color: &str, level: u8, status: InkStatus) -> InkLevel {
        InkLevel {
            color: color.into(),
            model: None,
            level: Some(level),
            status,
            rgb: None,
            order: None,
        }
    }

    fn snap(power: Power, inks: Vec<InkLevel>) -> PrinterSnapshot {
        PrinterSnapshot {
            power,
            inks,
            ink_source: InkSource::Ivec,
            device: Some(DeviceInfo { model: "XK140 series".into(), ..Default::default() }),
            ..Default::default()
        }
    }

    fn all_ok() -> Vec<InkLevel> {
        ["M", "BK", "Y", "PGBK", "C"].map(|c| ink(c, 60, InkStatus::Ok)).into()
    }

    fn keys(v: &[Notification]) -> Vec<&str> {
        v.iter().map(|n| n.key.as_str()).collect()
    }

    #[test]
    fn low_ink_is_announced_once_and_rearmed_after_a_refill() {
        let cfg = NotificationConfig::default();
        let mut st = NotifyState::default();
        assert!(st.evaluate(&cfg, &snap(Power::On, all_ok()), EN).is_empty());

        let mut low = all_ok();
        low[4] = ink("C", 10, InkStatus::Low);
        let first = st.evaluate(&cfg, &snap(Power::On, low.clone()), EN);
        assert_eq!(keys(&first), ["ink:C:low"]);
        assert!(first[0].summary.contains("Cyan") && first[0].body.contains("10%"));
        assert!(first[0].body.contains("Canon XK140 series"), "{}", first[0].body);
        assert!(!first[0].critical);
        // Polling again changes nothing.
        for _ in 0..3 {
            assert!(st.evaluate(&cfg, &snap(Power::On, low.clone()), EN).is_empty());
        }
        // Empty is a new condition and urgent.
        let mut empty = all_ok();
        empty[4] = ink("C", 0, InkStatus::Empty);
        let e = st.evaluate(&cfg, &snap(Power::On, empty.clone()), EN);
        assert_eq!(keys(&e), ["ink:C:empty"]);
        assert!(e[0].critical);
        assert!(st.evaluate(&cfg, &snap(Power::On, empty), EN).is_empty());
        // Refill, then low again: announced again.
        assert!(st.evaluate(&cfg, &snap(Power::On, all_ok()), EN).is_empty());
        assert_eq!(keys(&st.evaluate(&cfg, &snap(Power::On, low), EN)), ["ink:C:low"]);
    }

    #[test]
    fn several_tanks_notify_separately() {
        let mut st = NotifyState::default();
        let mut inks = all_ok();
        inks[0] = ink("M", 10, InkStatus::Low);
        inks[1] = ink("BK", 0, InkStatus::Empty);
        let n = st.evaluate(&NotificationConfig::default(), &snap(Power::On, inks), EN);
        let mut k = keys(&n);
        k.sort_unstable();
        assert_eq!(k, ["ink:BK:empty", "ink:M:low"]);
    }

    #[test]
    fn stale_cached_levels_do_not_change_what_was_announced() {
        let cfg = NotificationConfig::default();
        let mut st = NotifyState::default();
        let mut low = all_ok();
        low[4] = ink("C", 10, InkStatus::Low);
        assert_eq!(st.evaluate(&cfg, &snap(Power::On, low.clone()), EN).len(), 1);
        // The printer drops off the network: cached levels must neither clear nor re-fire.
        let mut off = snap(Power::Offline, low.clone());
        off.ink_source = InkSource::Cache;
        assert!(st.evaluate(&cfg, &off, EN).is_empty());
        // Back online (via IPP, still low): the "back online" notice, but not the ink again.
        let mut back = snap(Power::Sleeping, low);
        back.ink_source = InkSource::Ipp;
        assert_eq!(keys(&st.evaluate(&cfg, &back, EN)), ["online"]);
    }

    #[test]
    fn errors_are_announced_per_new_code() {
        let cfg = NotificationConfig::default();
        let mut st = NotifyState::default();
        let mut s = snap(Power::On, all_ok());
        assert!(st.evaluate(&cfg, &s, EN).is_empty());
        s.support = Support::from_code("1300");
        let n = st.evaluate(&cfg, &s, EN);
        assert_eq!(keys(&n), ["error"]);
        assert!(n[0].summary.contains("1300") && n[0].body.contains("jammed"));
        assert!(n[0].critical);
        assert!(st.evaluate(&cfg, &s, EN).is_empty(), "same code, no repeat");
        s.support = Support::from_code("1688");
        assert_eq!(st.evaluate(&cfg, &s, EN).len(), 1, "a different code is new");
        // Sleeping reports no code: the previous one stays remembered, nothing fires.
        let sleeping = snap(Power::Sleeping, all_ok());
        assert!(st.evaluate(&cfg, &sleeping, EN).is_empty());
        s.support = None;
        assert!(st.evaluate(&cfg, &s, EN).is_empty());
        s.support = Support::from_code("1688");
        assert_eq!(st.evaluate(&cfg, &s, EN).len(), 1, "cleared and then back: announced again");
    }

    #[test]
    fn severity_decides_the_urgency_and_notices_stay_quiet() {
        let cfg = NotificationConfig::default();
        let mut st = NotifyState::default();
        let mut s = snap(Power::On, all_ok());
        s.support = Support::from_code("1000");
        let n = st.evaluate(&cfg, &s, EN);
        assert_eq!(keys(&n), ["error"]);
        assert!(!n[0].critical && n[0].icon == "dialog-warning", "paper missing is a warning");
        assert!(n[0].body.contains("rear tray"), "{}", n[0].body);
        s.support = Support::from_code("1300");
        assert!(st.evaluate(&cfg, &s, EN)[0].critical, "a jam is an error");
        s.support = Support::from_code("D109");
        assert!(st.evaluate(&cfg, &s, EN).is_empty(), "an information code is no popup");
    }

    #[test]
    fn back_online_only_after_being_offline() {
        let cfg = NotificationConfig::default();
        let mut st = NotifyState::default();
        // Starting up with a reachable printer, sleeping or not, is not "coming back".
        assert!(st.evaluate(&cfg, &snap(Power::Sleeping, all_ok()), EN).is_empty());
        assert!(st.evaluate(&cfg, &snap(Power::On, all_ok()), EN).is_empty());
        assert!(st.evaluate(&cfg, &snap(Power::Offline, vec![]), EN).is_empty());
        assert!(st.evaluate(&cfg, &snap(Power::Offline, vec![]), EN).is_empty());
        let n = st.evaluate(&cfg, &snap(Power::Sleeping, all_ok()), EN);
        assert_eq!(keys(&n), ["online"]);
        assert!(n[0].summary.contains("Canon XK140 series is back online"));
        assert!(st.evaluate(&cfg, &snap(Power::On, all_ok()), EN).is_empty());
        // Started while offline.
        let mut st2 = NotifyState::default();
        assert!(st2.evaluate(&cfg, &snap(Power::Offline, vec![]), EN).is_empty());
        assert_eq!(keys(&st2.evaluate(&cfg, &snap(Power::On, all_ok()), EN)), ["online"]);
    }

    #[test]
    fn switches_in_the_config() {
        let mut low = all_ok();
        low[0] = ink("M", 10, InkStatus::Low);
        let mut err = snap(Power::On, low.clone());
        err.support = Support::from_code("1300");

        let off = NotificationConfig { enabled: false, ..Default::default() };
        let mut st = NotifyState::default();
        assert!(st.evaluate(&off, &err, EN).is_empty());
        // Enabled later: what was already known is not announced retroactively.
        assert!(st.evaluate(&NotificationConfig::default(), &err, EN).is_empty());

        let only_errors =
            NotificationConfig { ink: false, online: false, ..NotificationConfig::default() };
        let mut st = NotifyState::default();
        assert_eq!(keys(&st.evaluate(&only_errors, &err, EN)), ["error"]);
        let mut st = NotifyState::default();
        let no_errors = NotificationConfig { errors: false, ..NotificationConfig::default() };
        assert_eq!(keys(&st.evaluate(&no_errors, &err, EN)), ["ink:M:low"]);
        let mut off = snap(Power::Offline, low.clone());
        off.ink_source = InkSource::Cache;
        st.evaluate(&no_errors, &off, EN);
        let no_online = NotificationConfig { online: false, ..NotificationConfig::default() };
        assert!(st.evaluate(&no_online, &snap(Power::On, low), EN).is_empty());
    }

    #[test]
    fn label_falls_back_without_device_info() {
        let mut s = snap(Power::On, all_ok());
        s.device = None;
        assert_eq!(printer_label(&s, EN), "Canon printer");
        assert_eq!(printer_label(&s, JA), "Canonプリンター");
        s.device = Some(DeviceInfo { model: "Canon PIXMA X".into(), ..Default::default() });
        assert_eq!(printer_label(&s, EN), "Canon PIXMA X");
    }

    #[test]
    fn the_same_conditions_are_worded_in_japanese() {
        let cfg = NotificationConfig::default();
        let mut st = NotifyState::default();
        let mut low = all_ok();
        low[4] = ink("C", 10, InkStatus::Low);
        let n = st.evaluate(&cfg, &snap(Power::On, low), JA);
        assert_eq!(keys(&n), ["ink:C:low"], "the keys do not depend on the language");
        assert!(n[0].summary.contains("シアン") && n[0].body.contains("10%"), "{:?}", n[0]);
        assert!(n[0].body.contains("Canon XK140 series"), "the model name stays as it is");

        let mut s = snap(Power::On, all_ok());
        s.support = Support::from_code("1300");
        let n = st.evaluate(&cfg, &s, JA);
        assert_eq!(keys(&n), ["error"]);
        assert!(n[0].summary.contains("1300") && n[0].body.contains("紙"), "{:?}", n[0]);

        let mut st = NotifyState::default();
        st.evaluate(&cfg, &snap(Power::Offline, vec![]), JA);
        let n = st.evaluate(&cfg, &snap(Power::On, all_ok()), JA);
        assert_eq!(n[0].summary, "Canon XK140 seriesがオンラインに戻りました");
    }
}
