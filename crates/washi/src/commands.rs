// SPDX-License-Identifier: GPL-2.0-or-later
//! Command implementations.

use std::io::Write as _;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use serde::Serialize;
use washi_core::ivec::{
    CleaningType, DeviceCapability, DeviceSettings, DeviceStatus, ReadOp, Service, redact,
};
use washi_core::{
    Config, Confirm, HostSource, MaintOp, Plan, Power, Printer, PrinterSnapshot, Progress,
    plan_power_off, plan_power_on, plan_settings, resolve_host, unix_now,
};

use crate::cli::{Cli, Cmd, ConfigCmd, MaintCmd, PowerCmd};
use crate::{firmware, jobs, render};

/// How long `washi power on` waits for the printer to come up, and how often it looks.
const WAKE_WAIT: Duration = Duration::from_secs(10);
const WAKE_POLL: Duration = Duration::from_secs(1);

pub fn run(cli: Cli) -> Result<()> {
    match &cli.cmd {
        Cmd::Discover { timeout } => discover(&cli, *timeout),
        Cmd::Status => status(&cli),
        Cmd::Watch { interval, count, no_daemon } => watch(&cli, *interval, *count, *no_daemon),
        Cmd::Info => info(&cli),
        Cmd::Settings => settings(&cli),
        Cmd::Caps { service } => caps(&cli, service),
        Cmd::Raw { op, service, no_redact } => raw(&cli, op, service, *no_redact),
        Cmd::Maint { cmd } => maint(&cli, cmd),
        Cmd::Set { cmd } => set(&cli, &cmd.to_settings()?),
        Cmd::Power { cmd: PowerCmd::Off } => power_off(&cli),
        Cmd::Power { cmd: PowerCmd::On } => power_on(&cli),
        Cmd::Firmware { cmd } => firmware::firmware(&cli, cmd.as_ref()),
        Cmd::Links { code, lang } => firmware::links(&cli, code.as_deref(), *lang),
        Cmd::Config { cmd } => config(cmd.as_ref()),
        Cmd::Scan(a) => jobs::scan(&cli, a),
        Cmd::Print(a) => jobs::print(&cli, a),
        Cmd::Disc(a) => jobs::disc_cmd(&cli, a),
        Cmd::Identify { action } => jobs::identify(&cli, *action),
        Cmd::Jobs => jobs::jobs(&cli),
        Cmd::Cancel { job } => jobs::cancel(&cli, *job),
        Cmd::Media { source, size } => jobs::media(&cli, *source, size.as_deref()),
        Cmd::Scanner => jobs::scanner(&cli),
        Cmd::Tools => jobs::tools(&cli),
        Cmd::Doctor => crate::doctor::doctor(&cli),
        Cmd::NotifyTest => notify_test(&cli),
    }
}

pub(crate) fn print_json(v: &impl Serialize) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(v)?);
    Ok(())
}

pub(crate) fn connect(cli: &Cli) -> Result<Printer> {
    let config = Config::load()?;
    let (target, source) = resolve_host(cli.host.as_deref(), &config, Duration::from_secs(4))?;
    if source == HostSource::Discovery {
        eprintln!("using discovered printer {target}");
    }
    let mut printer = Printer::from_target(&target);
    printer.set_trace(cli.verbose);
    printer.set_reconnect_per_request(cli.reconnect);
    if let Some(port) = cli.ipp_port {
        printer.set_ipp_port(port);
    }
    Ok(printer)
}

fn discover(cli: &Cli, timeout: u64) -> Result<()> {
    let found = washi_net::discover(Duration::from_secs(timeout))?;
    if cli.json {
        return print_json(&found);
    }
    if found.is_empty() {
        println!("No Canon CHMP printers found (waited {timeout}s).");
        return Ok(());
    }
    for p in found {
        println!("{}", p.name);
        println!("  host       {}", p.host);
        println!("  addresses  {}", p.addresses.join(", "));
        println!("  port       {}", p.port);
        if let Some(m) = &p.mpath {
            println!("  mpath      {m}");
        }
    }
    Ok(())
}

/// Power, state, ink levels and paper in one view. A sleeping printer (auto power-off) is not an
/// error: ink comes from IPP and the rest from the cache. Only an unreachable printer exits 1.
fn status(cli: &Cli) -> Result<()> {
    let p = connect(cli)?;
    let cache = p.default_cache();
    let snap = p.snapshot(cache.as_ref());
    if cli.json {
        print_json(&snap)?;
    } else {
        print!("{}", render::render_snapshot(p.host(), &snap, unix_now()));
    }
    if snap.power == Power::Offline {
        bail!("the printer is offline: nothing answered on CHMP or IPP");
    }
    Ok(())
}

fn print_snapshot_line(cli: &Cli, snap: &PrinterSnapshot, started: Instant) -> Result<()> {
    if cli.json {
        println!("{}", serde_json::to_string(snap)?);
    } else {
        let t = started.elapsed().as_secs();
        let clock = format!("+{:02}:{:02}:{:02}", t / 3600, t / 60 % 60, t % 60);
        println!("{}", render::watch_line(snap, &clock));
    }
    std::io::stdout().flush()?;
    Ok(())
}

const DAEMON_NAME: &str = "io.github.Jenari_Dev.Washi";
const DAEMON_PATH: &str = "/io/github/Jenari_Dev/Washi";
const DAEMON_IFACE: &str = "io.github.Jenari_Dev.Washi1";

fn print_json_line(cli: &Cli, json: &str, started: Instant) -> Result<()> {
    if cli.json {
        println!("{}", json.trim());
        std::io::stdout().flush()?;
        return Ok(());
    }
    let snap: PrinterSnapshot =
        serde_json::from_str(json).context("unreadable snapshot from washid")?;
    print_snapshot_line(cli, &snap, started)
}

/// Stream snapshots from a running `washid`. `Ok(false)` means there is no daemon on the session
/// bus (or it does not know the printer) and the caller should poll directly.
fn watch_daemon(cli: &Cli, count: Option<u32>) -> Result<bool> {
    use futures_util::StreamExt;

    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
    rt.block_on(async {
        let Ok(conn) = zbus::Connection::session().await else { return Ok(false) };
        let bus = zbus::fdo::DBusProxy::new(&conn).await?;
        if !bus.name_has_owner(DAEMON_NAME.try_into()?).await? {
            return Ok(false);
        }
        let proxy = zbus::Proxy::new(&conn, DAEMON_NAME, DAEMON_PATH, DAEMON_IFACE).await?;
        let list: String = proxy.call("ListPrinters", &()).await?;
        let list: Vec<serde_json::Value> = serde_json::from_str(&list)?;
        let wanted = cli.host.as_deref().map(str::trim).filter(|h| !h.is_empty());
        let host = list.iter().find_map(|p| {
            let host = p["host"].as_str()?;
            let hit = match wanted {
                Some(w) => host.eq_ignore_ascii_case(w),
                None => p["active"].as_bool() == Some(true),
            };
            hit.then(|| host.to_string())
        });
        let Some(host) = host else {
            eprintln!("washid does not know this printer; polling it directly");
            return Ok(false);
        };

        // Subscribe first so that nothing between the first reading and the stream is lost.
        let mut changes = proxy.receive_signal("SnapshotChanged").await?;
        let started = Instant::now();
        let first: String = proxy.call("GetSnapshot", &(host.as_str(),)).await?;
        print_json_line(cli, &first, started)?;
        let mut n = 1u32;
        while count.is_none_or(|c| n < c) {
            let Some(msg) = changes.next().await else { break };
            let (h, json): (String, String) = msg.body().deserialize()?;
            if h == host {
                print_json_line(cli, &json, started)?;
                n += 1;
            }
        }
        Ok(true)
    })
}

/// Follow the printer: through `washid` when it runs (no extra load on the printer), else by
/// polling it directly. A line is printed whenever something changes.
fn watch(cli: &Cli, interval: u64, count: Option<u32>, no_daemon: bool) -> Result<()> {
    if !no_daemon && watch_daemon(cli, count)? {
        return Ok(());
    }
    let p = connect(cli)?;
    let cache = p.default_cache();
    let started = Instant::now();
    let mut last: Option<PrinterSnapshot> = None;
    let mut n = 0u32;
    loop {
        let snap = p.snapshot(cache.as_ref());
        if last.as_ref().is_none_or(|l| !l.content_eq(&snap)) {
            print_snapshot_line(cli, &snap, started)?;
        }
        last = Some(snap);
        n += 1;
        if count.is_some_and(|c| n >= c) {
            return Ok(());
        }
        std::thread::sleep(Duration::from_secs(interval));
    }
}

/// `washi notify-test`: ask the running `washid` to show a sample notification through the same
/// code that shows the real ones. Does not touch the printer. If the service is not running but
/// D-Bus can start it, D-Bus does.
fn notify_test(cli: &Cli) -> Result<()> {
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
    let reply = rt.block_on(async {
        let conn = zbus::Connection::session()
            .await
            .context("no D-Bus session bus: run this inside your desktop session")?;
        let proxy = zbus::Proxy::new(&conn, DAEMON_NAME, DAEMON_PATH, DAEMON_IFACE).await?;
        let call = proxy.call::<_, _, ()>("SendTestNotification", &());
        match tokio::time::timeout(Duration::from_secs(20), call).await {
            Err(_) => bail!("washid did not answer in time"),
            Ok(Ok(())) => Ok(None),
            Ok(Err(zbus::Error::MethodError(name, msg, _))) => {
                let name = name.as_str().to_string();
                let text = msg.unwrap_or_default();
                if name.ends_with(".Error.Notification") {
                    Ok(Some(text))
                } else if name.contains("ServiceUnknown") || name.contains("NameHasNoOwner") {
                    bail!(
                        "washid is not running (start it with `washid`, `systemctl --user start washid`, or open the app)"
                    )
                } else {
                    bail!("{name}: {text}")
                }
            }
            Ok(Err(e)) => bail!("{e}"),
        }
    })?;
    match reply {
        None if cli.json => print_json(&serde_json::json!({ "sent": true }))?,
        None => println!("Sent: a test notification should be on your screen now."),
        Some(why) if cli.json => {
            print_json(&serde_json::json!({ "sent": false, "error": why }))?;
            bail!("{why}");
        }
        Some(why) => bail!("{why}"),
    }
    Ok(())
}

#[derive(Serialize)]
struct InfoJson<'a> {
    manufacturer: &'a str,
    model: &'a str,
    firmware: &'a str,
    services: &'a [String],
    counters: &'a DeviceStatus,
}

fn info(cli: &Cli) -> Result<()> {
    let p = connect(cli)?;
    let cap = p.device_capability()?;
    let st = p.device_status()?;
    if cli.json {
        return print_json(&InfoJson {
            manufacturer: &cap.manufacturer,
            model: &cap.model,
            firmware: &cap.firmver,
            services: &cap.services,
            counters: &st,
        });
    }
    let asleep = !p.print_service_awake();
    print!("{}", render::render_info(&cap, &st, asleep));
    Ok(())
}

#[derive(Serialize)]
struct SettingsJson<'a> {
    current: &'a washi_core::ivec::DeviceConfig,
    allowed: &'a DeviceCapability,
}

fn settings(cli: &Cli) -> Result<()> {
    let p = connect(cli)?;
    let cfg = p.device_config()?;
    let cap = p.device_capability()?;
    if cli.json {
        return print_json(&SettingsJson { current: &cfg, allowed: &cap });
    }
    print!("{}", render::render_settings(&cfg, &cap));
    Ok(())
}

fn caps(cli: &Cli, service: &str) -> Result<()> {
    let svc: Service = service.parse()?;
    let p = connect(cli)?;
    match svc {
        Service::Device => {
            let cap = p.device_capability()?;
            if cli.json {
                return print_json(&cap);
            }
            print!("{}", render::render_cap_device(&cap));
            Ok(())
        }
        Service::Maintenance => {
            let cap = p.maintenance_capability()?;
            if cli.json {
                return print_json(&cap);
            }
            print!("{}", render::render_cap_maintenance(&cap));
            Ok(())
        }
        other => {
            // No typed view yet: show the reply as flattened `path = value` lines.
            let resp = p.raw(ReadOp::GetCapability, other)?;
            resp.require_ok()?;
            let rows = resp.flatten();
            if cli.json {
                let objs: Vec<_> = rows
                    .iter()
                    .map(|(k, v)| serde_json::json!({ "path": k, "value": v }))
                    .collect();
                return print_json(&objs);
            }
            for (k, v) in rows {
                println!("{k} = {v}");
            }
            Ok(())
        }
    }
}

fn raw(cli: &Cli, op: &str, service: &str, no_redact: bool) -> Result<()> {
    let op: ReadOp = op.parse()?;
    let svc: Service = service.parse()?;
    let p = connect(cli)?;
    let resp = p.raw(op, svc)?;
    let text = if no_redact { resp.raw().to_string() } else { redact(resp.raw())? };
    println!("{}", text.trim_end());
    Ok(())
}

fn maint_op(cmd: &MaintCmd) -> MaintOp {
    match cmd {
        MaintCmd::Clean { deep, roller, platen, group } => MaintOp::Clean {
            kind: if *roller {
                CleaningType::Roller
            } else if *platen {
                CleaningType::Platen
            } else if *deep {
                CleaningType::Deep
            } else {
                CleaningType::Regular
            },
            group: (*group).into(),
        },
        MaintCmd::NozzleCheck => MaintOp::NozzleCheck,
        MaintCmd::Align { check: true, .. } => MaintOp::AlignCheck,
        MaintCmd::Align { manual: Some(id), .. } => MaintOp::AlignManual(*id),
        MaintCmd::Align { .. } => MaintOp::AlignAuto,
        MaintCmd::RollerClean => MaintOp::RollerClean,
    }
}

fn dry_run(plan: &Plan) {
    print!("{}", render::render_plan(plan));
}

fn maint(cli: &Cli, cmd: &MaintCmd) -> Result<()> {
    let op = maint_op(cmd);
    let plan = op.plan()?;
    if !cli.confirm {
        dry_run(&plan);
        return Ok(());
    }
    let p = connect(cli)?;
    println!("Printer {}: {}", p.host(), op.label());
    let report = p.run_maintenance(&op, Confirm::explicit(), &mut |ev| match ev {
        Progress::JobStarted => println!("  job started"),
        Progress::CommandAccepted { operation } => println!("  printer accepted: {operation}"),
        Progress::Waiting { elapsed, status, support_code, job } => {
            let mut line = format!("  [{}] printer is {status}", render::fmt_duration(elapsed));
            if !support_code.is_empty() {
                line.push_str(&format!(", support code {support_code}"));
            }
            if job.is_some() {
                line.push_str(", job listed");
            }
            println!("{line}");
        }
        Progress::JobEnded => println!("  printer is idle again"),
        Progress::Ending => println!("  closing job"),
    })?;
    println!("Done: {} ({}).", report.operation, render::fmt_duration(report.elapsed));
    Ok(())
}

fn describe_changes(s: &DeviceSettings) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(v) = s.autopoweron {
        out.push(format!("auto power on = {v}"));
    }
    for (name, p) in [
        ("auto power off", &s.autopoweroff),
        ("auto power off (no network)", &s.autopoweroff_standalone),
    ] {
        if let Some(p) = p {
            out.push(match (p.mode, p.time) {
                (washi_core::ivec::OnOff::On, Some(t)) => format!("{name} = {t} min"),
                _ => format!("{name} = off"),
            });
        }
    }
    if let Some(q) = &s.silentmode {
        out.push(match (&q.start, &q.end) {
            (Some(a), Some(b)) => format!("quiet mode = {a}-{b}"),
            _ => format!("quiet mode = {}", q.kind.as_str().to_ascii_lowercase()),
        });
    }
    if let Some(v) = &s.papergap {
        let on = v.eq_ignore_ascii_case("wide");
        out.push(format!("prevent paper abrasion = {}", if on { "on" } else { "off" }));
    }
    if let Some(v) = s.drylevel {
        out.push(format!("dry level = {v}"));
    }
    if let Some(v) = s.auto_printer_rom_update {
        out.push(format!("auto firmware update = {v}"));
    }
    if let Some(v) = s.mqtt_connection {
        out.push(format!("cloud link = {v}"));
    }
    for (id, items) in &s.setregistration {
        let list: Vec<String> = items.iter().map(|(k, v)| format!("{k}={v}")).collect();
        out.push(format!("head alignment set {id}: {}", list.join(" ")));
    }
    out
}

fn set(cli: &Cli, changes: &DeviceSettings) -> Result<()> {
    let plan = plan_settings(changes)?;
    if !cli.confirm {
        dry_run(&plan);
        return Ok(());
    }
    let p = connect(cli)?;
    let what = describe_changes(changes).join(", ");
    let report = p.apply_device_settings(changes, Confirm::explicit())?;
    if report.changed {
        println!("Printer {}: set {what} (verified by read-back).", p.host());
    } else {
        println!("Printer {}: already {what}; nothing was written.", p.host());
    }
    Ok(())
}

fn power_off(cli: &Cli) -> Result<()> {
    if !cli.confirm {
        dry_run(&plan_power_off());
        return Ok(());
    }
    let p = connect(cli)?;
    p.power_off(Confirm::explicit())?;
    println!("Printer {}: power off sent.", p.host());
    Ok(())
}

fn power_on(cli: &Cli) -> Result<()> {
    if !cli.confirm {
        dry_run(&plan_power_on());
        return Ok(());
    }
    let p = connect(cli)?;
    let was_asleep = !p.print_service_awake();
    p.power_on(Confirm::explicit())?;
    // The printer takes a few seconds to come up: look at the print status until it stops
    // saying `Suspended`.
    if p.wait_until_awake(WAKE_WAIT, WAKE_POLL) {
        if was_asleep {
            println!("Printer woke up ({}).", p.host());
        } else {
            println!("Printer {}: power on sent; it was already awake.", p.host());
        }
    } else {
        println!(
            "Printer {}: power on sent, but it still reports auto power-off after {} s; \
             check `washi status` in a moment.",
            p.host(),
            WAKE_WAIT.as_secs()
        );
    }
    Ok(())
}

fn config(cmd: Option<&ConfigCmd>) -> Result<()> {
    let path = Config::default_path().context("no configuration directory on this system")?;
    let mut cfg = Config::load()?;
    match cmd {
        None => {
            println!("config file   {}", path.display());
            println!(
                "default host  {}",
                cfg.printer
                    .host
                    .as_deref()
                    .unwrap_or("(none; the first discovered printer is used)")
            );
        }
        Some(ConfigCmd::SetHost { host }) => {
            cfg.set_host(host)?;
            cfg.save()?;
            println!("default host saved: {host}");
        }
        Some(ConfigCmd::ClearHost) => {
            if cfg.printer.host.take().is_none() {
                bail!("no default host is saved");
            }
            cfg.save()?;
            println!("default host cleared");
        }
    }
    Ok(())
}
