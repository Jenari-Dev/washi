// SPDX-License-Identifier: GPL-2.0-or-later
//! washid: the Washi daemon (session bus).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use clap::Parser;
use tracing::info;
use washi_core::Printer;
use washid::dbus::{self, Washi};
use washid::notify::DbusNotifier;
use washid::service::{Options, PrinterFactory, Service};

#[derive(Parser)]
#[command(
    version,
    about = "Washi daemon: printer status, maintenance and notifications over D-Bus"
)]
struct Args {
    /// Refuse every write (maintenance, settings). Status, discovery and polling still work.
    #[arg(long)]
    read_only: bool,
    /// Config file (default: ~/.config/washi/config.toml).
    #[arg(long)]
    config: Option<PathBuf>,
    /// Snapshot cache directory (default: ~/.cache/washi).
    #[arg(long)]
    cache_dir: Option<PathBuf>,
    /// State file with the known printers (default: ~/.local/state/washi/washid.json).
    #[arg(long)]
    state_file: Option<PathBuf>,
    /// A printer to know about (host name or address, `host:port`); repeatable.
    #[arg(long = "printer", value_name = "HOST")]
    printers: Vec<String>,
    /// Seconds between polls of the active printer (default: config, 30).
    #[arg(long, value_name = "SECONDS", value_parser = clap::value_parser!(u64).range(1..))]
    poll_interval: Option<u64>,
    /// Do not send desktop notifications.
    #[arg(long)]
    no_notify: bool,
    /// IPP port of every printer (default 631); for tests against a local fake printer.
    #[arg(long, hide = true, env = "WASHI_IPP_PORT", value_name = "PORT")]
    ipp_port: Option<u16>,
    /// Log more.
    #[arg(short, long)]
    verbose: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    tracing_subscriber::fmt()
        .with_target(false)
        .with_max_level(if args.verbose { tracing::Level::DEBUG } else { tracing::Level::INFO })
        .init();

    let ipp_port = args.ipp_port;
    let factory: PrinterFactory = Arc::new(move |host: &str| {
        let mut p = Printer::from_target(host);
        if let Some(port) = ipp_port {
            p.set_ipp_port(port);
        }
        p
    });
    let svc = Service::new(Options {
        read_only: args.read_only,
        poll_interval: args.poll_interval.map(Duration::from_secs),
        config_path: args.config,
        cache_dir: args.cache_dir,
        state_path: args.state_file,
        printers: args.printers,
        printer_factory: Some(factory),
        ..Options::default()
    })?;

    let conn = zbus::connection::Builder::session()?
        .name(dbus::BUS_NAME)?
        .serve_at(dbus::PATH, Washi::new(svc.clone()))?
        .build()
        .await
        .context("D-Bus: is another washid already running?")?;
    let iface = conn.object_server().interface::<_, Washi>(dbus::PATH).await?;
    info!(
        "serving {} on the session bus{} (poll every {}s)",
        dbus::BUS_NAME,
        if svc.read_only() { " (read-only)" } else { "" },
        svc.poll_interval().as_secs()
    );

    let forwarder =
        tokio::spawn(dbus::forward_events(svc.subscribe(), iface.signal_emitter().clone()));
    if !args.no_notify {
        svc.set_notifier(Arc::new(DbusNotifier::new(conn.clone())));
    }
    let poller = tokio::spawn(svc.clone().run_poller());
    if svc.list_printers().is_empty() {
        info!("no printer configured; browsing the network");
        let s = svc.clone();
        tokio::spawn(async move {
            if let Err(e) = s.discover().await {
                tracing::warn!("discovery failed: {e}");
            }
        });
    }

    // systemd stops us with SIGTERM; Ctrl+C when run by hand.
    let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    tokio::select! {
        r = tokio::signal::ctrl_c() => r?,
        _ = term.recv() => {}
    }
    info!("shutting down");
    poller.abort();
    forwarder.abort();
    Ok(())
}
