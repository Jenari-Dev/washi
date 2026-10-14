// SPDX-License-Identifier: GPL-2.0-or-later
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use washi_ivec::{
    DeviceCapability, DeviceConfig, DeviceStatus, IvecError, MaintenanceCapability,
    MaintenanceStatus, PrintStatus, ReadOp, Request, Response, Service,
};
use washi_net::{
    ChmpClient, ClientOptions, EsclClient, EsclOptions, IppClient, IppOptions, NetError,
};

use crate::cache::SnapshotCache;
use crate::config::Config;
use crate::error::{Error, Result};
use crate::firmware::FirmwareOptions;
use crate::maintenance::MaintOptions;
use crate::snapshot::{Observation, PrintProbe, PrinterSnapshot};

/// Re-read the device information (`GetCapability device`) when the cached copy is older.
const DEVICE_INFO_MAX_AGE_SECS: u64 = 6 * 3600;

/// An unchanged snapshot is still written back this often, so `last_awake` stays roughly current.
const CACHE_REFRESH_SECS: u64 = 300;

/// Seconds since the Unix epoch.
pub fn unix_now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// A printer on the network. Cheap to create; every read opens (and closes) its own CHMP session.
#[derive(Debug, Clone)]
pub struct Printer {
    host: String,
    pub(crate) net: ClientOptions,
    pub(crate) maint: MaintOptions,
    firmware: FirmwareOptions,
    ipp: IppOptions,
    escl: EsclOptions,
    trace: bool,
}

impl Printer {
    pub fn new(host: impl Into<String>) -> Self {
        Self::with_options(host, ClientOptions::default())
    }

    /// Accepts `host`, `host:port`, `[v6]:port` or a bare IPv6 literal.
    pub fn from_target(target: &str) -> Self {
        let (host, port) = split_host_port(target);
        let mut net = ClientOptions::default();
        if let Some(p) = port {
            net.port = p;
        }
        Self::with_options(host, net)
    }

    pub fn with_options(host: impl Into<String>, net: ClientOptions) -> Self {
        // eSCL is served by the same web server as CHMP (port 80 on the printer).
        let escl = EsclOptions { port: net.port, ..EsclOptions::default() };
        Printer {
            host: host.into(),
            net,
            maint: MaintOptions::default(),
            firmware: FirmwareOptions::default(),
            ipp: IppOptions::default(),
            escl,
            trace: false,
        }
    }

    /// The CHMP port (80 unless the target said otherwise).
    pub fn chmp_port(&self) -> u16 {
        self.net.port
    }

    /// IPP settings (`ipp://host:631/ipp/print` by default).
    pub fn set_ipp_options(&mut self, opts: IppOptions) {
        self.ipp = opts;
    }

    pub fn set_ipp_port(&mut self, port: u16) {
        self.ipp.port = port;
    }

    /// The IPP client for this printer (reads are free; job operations need a [`Confirm`]).
    ///
    /// [`Confirm`]: crate::Confirm
    pub fn ipp(&self) -> IppClient {
        IppClient::with_options(self.host.clone(), self.ipp.clone())
    }

    pub fn ipp_options(&self) -> &IppOptions {
        &self.ipp
    }

    /// The eSCL scanner client (port 80, like CHMP, unless changed).
    pub fn escl(&self) -> EsclClient {
        EsclClient::with_options(self.host.clone(), self.escl.clone())
    }

    pub fn set_escl_options(&mut self, opts: EsclOptions) {
        self.escl = opts;
    }

    pub fn set_escl_port(&mut self, port: u16) {
        self.escl.port = port;
    }

    /// The snapshot cache file of this printer (`~/.cache/washi/<hash of host>.json`).
    pub fn default_cache(&self) -> Option<SnapshotCache> {
        SnapshotCache::for_printer(&self.host, self.net.port)
    }

    pub fn host(&self) -> &str {
        &self.host
    }

    /// Log HTTP-level exchanges (status codes and sizes, never bodies) to stderr.
    pub fn set_trace(&mut self, on: bool) {
        self.trace = on;
    }

    /// Open a new TCP connection for every HTTP request (the CHMP session is kept).
    pub fn set_reconnect_per_request(&mut self, on: bool) {
        self.net.reconnect_per_request = on;
    }

    pub fn set_maint_options(&mut self, opts: MaintOptions) {
        self.maint = opts;
    }

    /// Timing of the firmware update wait (see [`Printer::update_firmware`]).
    pub fn set_firmware_options(&mut self, opts: FirmwareOptions) {
        self.firmware = opts;
    }

    pub fn firmware_options(&self) -> &FirmwareOptions {
        &self.firmware
    }

    /// A fresh CHMP client (new session).
    pub fn client(&self) -> ChmpClient {
        let mut c = ChmpClient::with_options(self.host.clone(), self.net.clone());
        c.set_trace(self.trace);
        c
    }

    /// Raw read: the parsed reply plus its XML, whatever the service answers.
    pub fn raw(&self, op: ReadOp, service: Service) -> Result<Response> {
        let mut c = self.client();
        let req = Request::Get { op, service };
        Ok(c.query(&req)?)
    }

    pub fn print_status(&self) -> Result<PrintStatus> {
        Ok(PrintStatus::from_response(&self.raw(ReadOp::GetStatus, Service::Print)?)?)
    }

    /// Is the print service answering, i.e. the printer is not in auto power-off? One read-only
    /// `GetStatus print`: `NG Suspended` and an unreachable printer are "no".
    pub fn print_service_awake(&self) -> bool {
        matches!(probe_print(self.print_status()), PrintProbe::Awake(_) | PrintProbe::Busy)
    }

    /// After a wake request: look at `GetStatus print` right away and then every `interval`,
    /// until it no longer says `Suspended` (true) or `timeout` has passed (false). Read-only.
    pub fn wait_until_awake(&self, timeout: Duration, interval: Duration) -> bool {
        let start = Instant::now();
        loop {
            if self.print_service_awake() {
                return true;
            }
            if start.elapsed() >= timeout {
                return false;
            }
            std::thread::sleep(interval);
        }
    }

    pub fn device_status(&self) -> Result<DeviceStatus> {
        Ok(DeviceStatus::from_response(&self.raw(ReadOp::GetStatus, Service::Device)?)?)
    }

    pub fn maintenance_status(&self) -> Result<MaintenanceStatus> {
        Ok(MaintenanceStatus::from_response(&self.raw(ReadOp::GetStatus, Service::Maintenance)?)?)
    }

    pub fn device_config(&self) -> Result<DeviceConfig> {
        Ok(DeviceConfig::from_response(&self.raw(ReadOp::GetConfiguration, Service::Device)?)?)
    }

    pub fn device_capability(&self) -> Result<DeviceCapability> {
        Ok(DeviceCapability::from_response(&self.raw(ReadOp::GetCapability, Service::Device)?)?)
    }

    pub fn maintenance_capability(&self) -> Result<MaintenanceCapability> {
        Ok(MaintenanceCapability::from_response(
            &self.raw(ReadOp::GetCapability, Service::Maintenance)?,
        )?)
    }
}

/// A short, identifier-free reason for a transport failure (no host names or addresses).
fn net_reason(e: &NetError) -> &'static str {
    match e {
        NetError::Connect { source, .. } => match source.kind() {
            std::io::ErrorKind::ConnectionRefused => "connection refused",
            std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock => "connection timed out",
            _ => "cannot connect",
        },
        NetError::Timeout(_) => "no reply in time",
        NetError::Io(_) => "connection lost",
        _ => "unexpected reply",
    }
}

fn probe_print(result: Result<PrintStatus>) -> PrintProbe {
    match result {
        Ok(ps) => PrintProbe::Awake(ps),
        Err(Error::Ivec(IvecError::Ng { detail, .. })) => match detail.as_str() {
            "Suspended" => PrintProbe::Suspended,
            "DeviceBusy" => PrintProbe::Busy,
            other => PrintProbe::Refused(other.to_string()),
        },
        Err(Error::Net(
            e @ (NetError::Connect { .. } | NetError::Timeout(_) | NetError::Io(_)),
        )) => PrintProbe::Unreachable(net_reason(&e).to_string()),
        Err(Error::Net(NetError::Status { status })) => {
            PrintProbe::Refused(format!("HTTP {status}"))
        }
        Err(_) => PrintProbe::Refused("unreadable reply".to_string()),
    }
}

impl Printer {
    /// Ask the printer what it can say right now. Read-only: `GetStatus print`, and when that
    /// works or the printer merely sleeps also `GetStatus device` (and `GetCapability device`
    /// when `prev` has no recent device information); IPP `Get-Printer-Attributes` whenever the
    /// print service cannot provide ink levels. Never fails: problems become the
    /// [`Observation`]'s probe result and warnings.
    pub fn observe(&self, prev: Option<&PrinterSnapshot>, now: u64) -> Observation {
        let print = probe_print(self.print_status());
        let mut warnings = Vec::new();

        let ivec_reachable = !matches!(print, PrintProbe::Unreachable(_));
        let device_status = if ivec_reachable {
            match self.device_status() {
                Ok(d) => Some(d),
                Err(e) => {
                    warnings.push(format!("GetStatus device failed: {}", short(&e)));
                    None
                }
            }
        } else {
            None
        };
        let need_cap = ivec_reachable
            && prev.is_none_or(|p| {
                p.device.is_none() || now.saturating_sub(p.last_updated) > DEVICE_INFO_MAX_AGE_SECS
            });
        let device_cap = if need_cap {
            match self.device_capability() {
                Ok(c) => Some(c),
                Err(e) => {
                    warnings.push(format!("GetCapability device failed: {}", short(&e)));
                    None
                }
            }
        } else {
            None
        };

        let ink_from_ivec = matches!(&print, PrintProbe::Awake(ps) if !ps.inks.is_empty());
        let ipp = if ink_from_ivec {
            None
        } else {
            match self.ipp().status() {
                Ok(a) => Some(a),
                Err(e) => {
                    if ivec_reachable {
                        warnings.push(format!("IPP status failed: {}", short_net(&e)));
                    }
                    None
                }
            }
        };
        Observation {
            print,
            device_status,
            device_cap,
            device_cap_current: ivec_reachable && !need_cap,
            ipp,
            warnings,
        }
    }

    /// Observe and merge with the cache (if any), then update the cache when something changed.
    pub fn snapshot(&self, cache: Option<&SnapshotCache>) -> PrinterSnapshot {
        self.snapshot_with(cache, false)
    }

    /// Like [`Printer::snapshot`], but reads the device information (`GetCapability device`:
    /// model, firmware version) again even if the cached copy is recent. For "check for
    /// updates" and for after a firmware update.
    pub fn snapshot_fresh(&self, cache: Option<&SnapshotCache>) -> PrinterSnapshot {
        self.snapshot_with(cache, true)
    }

    fn snapshot_with(&self, cache: Option<&SnapshotCache>, reread_info: bool) -> PrinterSnapshot {
        let prev = cache.and_then(SnapshotCache::load);
        let now = unix_now();
        let obs = if reread_info {
            let mut stale = prev.clone();
            if let Some(p) = stale.as_mut() {
                p.device = None;
            }
            self.observe(stale.as_ref(), now)
        } else {
            self.observe(prev.as_ref(), now)
        };
        let mut snap = PrinterSnapshot::compose(obs, prev.as_ref(), now);
        // Display only; a failed lookup keeps the address the cache remembers.
        if let Some(a) = resolve_ipv4(&self.host, Duration::from_millis(1500)) {
            snap.address = Some(a);
        }
        if let Some(c) = cache {
            let stale = |p: &PrinterSnapshot| {
                snap.last_updated.saturating_sub(p.last_updated) >= CACHE_REFRESH_SECS
            };
            if prev.as_ref().is_none_or(|p| !p.content_eq(&snap) || stale(p)) {
                let _ = c.store(&snap);
            }
        }
        snap
    }
}

fn short(e: &Error) -> String {
    match e {
        Error::Net(n) => short_net(n),
        Error::Ivec(IvecError::Ng { detail, .. }) => detail.clone(),
        _ => "unreadable reply".to_string(),
    }
}

fn short_net(e: &NetError) -> String {
    match e {
        NetError::Status { status } => format!("HTTP {status}"),
        NetError::Ipp(i) => i.to_string(),
        other => net_reason(other).to_string(),
    }
}

/// The IPv4 address a host name resolves to on this computer (mDNS names need `nss-mdns`), or
/// `None` when it does not resolve within `timeout`. An IPv4 literal is returned as it is. The
/// lookup runs on its own thread so that a slow resolver cannot stall the caller.
pub fn resolve_ipv4(host: &str, timeout: Duration) -> Option<String> {
    use std::net::{IpAddr, ToSocketAddrs};
    let host = host.trim().trim_end_matches('.');
    if host.is_empty() {
        return None;
    }
    if let Ok(ip) = host.parse::<IpAddr>() {
        return match ip {
            IpAddr::V4(v4) => Some(v4.to_string()),
            IpAddr::V6(_) => None,
        };
    }
    let (tx, rx) = std::sync::mpsc::channel();
    let name = host.to_string();
    std::thread::spawn(move || {
        let found = (name.as_str(), 0u16)
            .to_socket_addrs()
            .ok()
            .and_then(|mut it| it.find(|a| a.is_ipv4()).map(|a| a.ip().to_string()));
        let _ = tx.send(found);
    });
    rx.recv_timeout(timeout).ok().flatten()
}

/// Split `host:port` and `[v6]:port`; plain names and bare IPv6 literals pass through.
pub fn split_host_port(target: &str) -> (String, Option<u16>) {
    let target = target.trim();
    if let Some(rest) = target.strip_prefix('[') {
        if let Some((host, tail)) = rest.split_once(']') {
            return (host.to_string(), tail.strip_prefix(':').and_then(|p| p.parse().ok()));
        }
    }
    if target.matches(':').count() == 1 {
        if let Some((host, port)) = target.split_once(':') {
            if let Ok(p) = port.parse() {
                return (host.to_string(), Some(p));
            }
        }
    }
    (target.to_string(), None)
}

/// Where the host used for a command came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostSource {
    Flag,
    Config,
    Discovery,
}

/// `--host`, else the saved default, else the first printer found by mDNS.
pub fn resolve_host(
    explicit: Option<&str>,
    config: &Config,
    discovery_timeout: Duration,
) -> Result<(String, HostSource)> {
    if let Some(h) = explicit.map(str::trim).filter(|h| !h.is_empty()) {
        return Ok((h.to_string(), HostSource::Flag));
    }
    if let Some(h) = config.printer.host.as_deref().map(str::trim).filter(|h| !h.is_empty()) {
        return Ok((h.to_string(), HostSource::Config));
    }
    match washi_net::discover_first(discovery_timeout).map_err(Error::from)? {
        Some(p) => Ok((p.connect_host(), HostSource::Discovery)),
        None => Err(Error::NoPrinter),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_port_splitting() {
        assert_eq!(split_host_port("printer.local"), ("printer.local".into(), None));
        assert_eq!(split_host_port("127.0.0.1:8080"), ("127.0.0.1".into(), Some(8080)));
        assert_eq!(split_host_port("[fe80::1%eth0]:631"), ("fe80::1%eth0".into(), Some(631)));
        assert_eq!(split_host_port("fe80::1%eth0"), ("fe80::1%eth0".into(), None));
        assert_eq!(split_host_port("[::1]"), ("::1".into(), None));
    }

    #[test]
    fn addresses_resolve_without_a_network_for_literals() {
        let t = Duration::from_millis(200);
        assert_eq!(resolve_ipv4("192.0.2.9", t).as_deref(), Some("192.0.2.9"));
        assert_eq!(resolve_ipv4("127.0.0.1", t).as_deref(), Some("127.0.0.1"));
        assert_eq!(resolve_ipv4("fe80::1", t), None);
        assert_eq!(resolve_ipv4("  ", t), None);
    }

    #[test]
    fn host_resolution_order_without_network() {
        let mut cfg = Config::default();
        cfg.printer.host = Some("from-config.local".into());
        let t = Duration::from_millis(1);
        assert_eq!(
            resolve_host(Some("flag.local"), &cfg, t).unwrap(),
            ("flag.local".to_string(), HostSource::Flag)
        );
        assert_eq!(
            resolve_host(None, &cfg, t).unwrap(),
            ("from-config.local".to_string(), HostSource::Config)
        );
        assert_eq!(
            resolve_host(Some("  "), &cfg, t).unwrap().1,
            HostSource::Config,
            "blank --host falls through"
        );
    }
}
