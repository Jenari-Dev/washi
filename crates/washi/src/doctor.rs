// SPDX-License-Identifier: GPL-2.0-or-later
//! `washi doctor`: a read-only self-check. It looks at the three programs (this command, the
//! `washid` service, the `washi-app` window), at how the service gets started, at the printer
//! and at the optional helpers, and prints a tick, a cross or a dash for each with a hint. The
//! checks of the printer and of the computer are the ones the service runs for the app's
//! "Check setup" (they live in `washi_core::doctor`). Nothing is written to the printer, and
//! no serial number, hardware address or address of the printer is printed.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::Result;
use washi_core::doctor::{
    Check, Group, Report, computer_groups, group_title, mdns_check, no_printer_check,
    notification_check, printer_checks, scrub,
};
use washi_core::tools::run_capture;
use washi_core::{
    Config, HostSource, Language, LocalizationCache, Printer, SnapshotCache, Tools, resolve_host,
};

use crate::cli::Cli;
use crate::commands::print_json;

const LANG: Language = Language::En;
const DAEMON_NAME: &str = "io.github.Jenari_Dev.Washi";
const DAEMON_PATH: &str = "/io/github/Jenari_Dev/Washi";
const DAEMON_IFACE: &str = "io.github.Jenari_Dev.Washi1";
const CLI_VERSION: &str = env!("CARGO_PKG_VERSION");

/// What the session bus says about the service and the notification server.
#[derive(Debug, Default)]
pub struct BusFacts {
    /// Why there is no session bus (`None`: there is one).
    pub no_bus: Option<String>,
    pub running: bool,
    pub version: Option<String>,
    pub read_only: bool,
    pub activatable: bool,
    /// The notification server's name, or why it did not answer.
    pub notifications: Option<Result<String, String>>,
}

async fn probe_bus_async() -> BusFacts {
    let mut f = BusFacts::default();
    let conn = match tokio::time::timeout(Duration::from_secs(3), zbus::Connection::session()).await
    {
        Ok(Ok(c)) => c,
        Ok(Err(e)) => {
            f.no_bus = Some(e.to_string());
            return f;
        }
        Err(_) => {
            f.no_bus = Some("the session bus did not answer".into());
            return f;
        }
    };
    let Ok(bus) = zbus::fdo::DBusProxy::new(&conn).await else {
        f.no_bus = Some("the session bus does not answer".into());
        return f;
    };
    if let Ok(name) = DAEMON_NAME.try_into() {
        f.running = bus.name_has_owner(name).await.unwrap_or(false);
    }
    f.activatable = bus
        .list_activatable_names()
        .await
        .map(|names| names.iter().any(|n| n.as_str() == DAEMON_NAME))
        .unwrap_or(false);
    if f.running {
        if let Ok(p) = zbus::Proxy::new(&conn, DAEMON_NAME, DAEMON_PATH, DAEMON_IFACE).await {
            f.version = tokio::time::timeout(
                Duration::from_secs(3),
                p.call::<_, _, String>("Version", &()),
            )
            .await
            .ok()
            .and_then(Result::ok);
            f.read_only = p.get_property::<bool>("ReadOnly").await.unwrap_or(false);
        }
    }
    f.notifications = Some(
        match zbus::Proxy::new(
            &conn,
            "org.freedesktop.Notifications",
            "/org/freedesktop/Notifications",
            "org.freedesktop.Notifications",
        )
        .await
        {
            Ok(p) => {
                match tokio::time::timeout(
                    Duration::from_secs(3),
                    p.call::<_, _, (String, String, String, String)>("GetServerInformation", &()),
                )
                .await
                {
                    Ok(Ok((name, _vendor, _version, _spec))) => Ok(name),
                    Ok(Err(e)) => Err(e.to_string()),
                    Err(_) => Err("the notification service did not answer in time".into()),
                }
            }
            Err(e) => Err(e.to_string()),
        },
    );
    f
}

fn probe_bus() -> BusFacts {
    match tokio::runtime::Builder::new_current_thread().enable_all().build() {
        Ok(rt) => rt.block_on(probe_bus_async()),
        Err(e) => BusFacts { no_bus: Some(e.to_string()), ..BusFacts::default() },
    }
}

/// `systemctl --user` answer for the unit: `Some(state)` such as `enabled` / `disabled`, `None`
/// when systemd cannot be asked (no systemctl, no user manager).
fn unit_state() -> Option<String> {
    let mut cmd = std::process::Command::new("systemctl");
    cmd.args(["--user", "is-enabled", "washid.service"]);
    let out = run_capture(cmd, Duration::from_secs(3), "systemctl").ok()?;
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let err = String::from_utf8_lossy(&out.stderr);
    if text.is_empty() && (err.contains("Failed to connect") || err.contains("No medium")) {
        return None;
    }
    Some(if text.is_empty() { "not found".to_string() } else { text })
}

/// Where the app program may be: next to this one, else on the `PATH`.
fn find_app() -> Option<PathBuf> {
    let beside = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(|d| d.join("washi-app")))
        .filter(|p| p.is_file());
    beside.or_else(|| {
        washi_core::tools::find_in_path("washi-app", &std::env::var_os("PATH").unwrap_or_default())
    })
}

/// `washi-app --version` prints `washi-app X.Y.Z` before any window opens.
fn app_version(exe: &Path) -> Option<String> {
    let mut cmd = std::process::Command::new(exe);
    cmd.arg("--version");
    let out = run_capture(cmd, Duration::from_secs(5), "washi-app").ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    text.split_whitespace()
        .last()
        .filter(|v| v.chars().next().is_some_and(|c| c.is_ascii_digit()))
        .map(str::to_string)
}

fn washi_group(bus: &BusFacts, unit: Option<&str>, app: Option<&str>, app_found: bool) -> Group {
    let mut v = Vec::new();
    v.push(Check::ok("washi.cli", "washi (this command)", CLI_VERSION));

    // The service.
    let label = "Service (washid) running";
    v.push(match (&bus.no_bus, bus.running) {
        (Some(why), _) => Check::skip(
            "washi.service",
            label,
            format!("no session bus ({})", scrub(why, "")),
            "run this inside your desktop session (a session D-Bus is needed to talk to the service)",
        ),
        (None, true) => Check::ok(
            "washi.service",
            label,
            match (&bus.version, bus.read_only) {
                (Some(v), true) => format!("version {v}, read-only"),
                (Some(v), false) => format!("version {v}"),
                (None, _) => "it did not report a version".to_string(),
            },
        ),
        (None, false) => Check::skip(
            "washi.service",
            label,
            "not running",
            "the app starts it on demand when D-Bus activation is set up (see below); the command line works without it",
        ),
    });

    let label = "D-Bus activation (starts the service on demand)";
    v.push(if bus.no_bus.is_some() {
        Check::skip("washi.activation", label, "not checked", "")
    } else if bus.activatable {
        Check::ok("washi.activation", label, "installed")
    } else {
        Check::skip(
            "washi.activation",
            label,
            "not installed",
            "install io.github.Jenari_Dev.Washi.service into a dbus-1/services folder, or enable the systemd unit below",
        )
    });

    let label = "systemd user unit (washid.service)";
    v.push(match unit {
        None => Check::skip("washi.unit", label, "systemd cannot be asked here", ""),
        Some("enabled" | "enabled-runtime") => Check::ok("washi.unit", label, "enabled"),
        Some(other) => Check::skip(
            "washi.unit",
            label,
            other.to_string(),
            "to start the service with your session: systemctl --user enable --now washid",
        ),
    });

    let startable = bus.running
        || bus.activatable
        || matches!(unit, Some("enabled" | "enabled-runtime"))
        || bus.no_bus.is_some();
    if !startable {
        v.push(Check::fail(
            "washi.startable",
            "The service can be started",
            "it is not running, not activatable and not enabled",
            "the app needs the service: enable the systemd unit or install the D-Bus activation file",
        ));
    }

    // The app.
    let label = "App (washi-app)";
    v.push(match (app_found, app) {
        (true, Some(v)) => Check::ok("washi.app", label, format!("version {v}")),
        (true, None) => {
            Check::skip("washi.app", label, "found, but it did not report a version", "")
        }
        (false, _) => Check::skip(
            "washi.app",
            label,
            "not found next to this command or on the PATH",
            "the app is a separate program; the command line works without it",
        ),
    });

    // The three versions agree.
    let label = "Versions agree";
    let mut seen = vec![("washi", CLI_VERSION.to_string())];
    if let Some(v) = &bus.version {
        seen.push(("washid", v.clone()));
    }
    if let Some(v) = app {
        seen.push(("washi-app", v.to_string()));
    }
    let all = seen.iter().map(|(n, v)| format!("{n} {v}")).collect::<Vec<_>>().join(", ");
    v.push(if seen.len() < 2 {
        Check::skip("washi.versions", label, "only this command could be asked", "")
    } else if seen.iter().all(|(_, v)| *v == seen[0].1) {
        Check::ok("washi.versions", label, all)
    } else {
        Check::fail(
            "washi.versions",
            label,
            all,
            "restart the service after an update (systemctl --user restart washid) and close the old app window",
        )
    });
    Group::new("washi", group_title(LANG, "washi"), v)
}

/// Which printer is looked at, and how it was chosen.
fn printer_group(cli: &Cli) -> Group {
    let title = group_title(LANG, "printer");
    let config = Config::load().unwrap_or_default();
    let (target, source) = match resolve_host(cli.host.as_deref(), &config, Duration::from_secs(4))
    {
        Ok(x) => x,
        Err(_) => return Group::new("printer", title, vec![no_printer_check(LANG)]),
    };
    let how = match source {
        HostSource::Flag => "given with --host",
        HostSource::Config => "from the saved settings",
        HostSource::Discovery => "found by discovery",
    };
    let mut p = Printer::from_target(&target);
    if let Some(port) = cli.ipp_port {
        p.set_ipp_port(port);
    }
    let mut checks = vec![Check::ok("printer.host", "Printer to check", how)];
    checks.push(mdns_check(LANG, &target, Duration::from_secs(3)));
    let cache = LocalizationCache::for_printer(p.host(), p.chmp_port());
    checks.extend(printer_checks(LANG, &p, cache.as_ref()));
    Group::new("printer", title, checks)
}

pub fn doctor(cli: &Cli) -> Result<()> {
    let bus = probe_bus();
    let unit = unit_state();
    let app_exe = find_app();
    let app = app_exe.as_deref().and_then(app_version);

    let mut groups = vec![washi_group(&bus, unit.as_deref(), app.as_deref(), app_exe.is_some())];
    groups.push(printer_group(cli));
    let notification = match &bus.notifications {
        Some(Ok(name)) => notification_check(LANG, Ok(name)),
        Some(Err(why)) => notification_check(LANG, Err(why)),
        None => notification_check(LANG, Err("no session bus")),
    };
    let config_path = Config::default_path();
    let cache_dir = SnapshotCache::default_dir();
    groups.extend(computer_groups(
        LANG,
        notification,
        config_path.as_deref(),
        cache_dir.as_deref(),
        &Tools::detect(),
    ));
    let report = Report::new("washi", LANG, groups);
    if cli.json {
        return print_json(&report);
    }
    print!("{}", report.render_text());
    Ok(())
}
