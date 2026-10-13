// SPDX-License-Identifier: GPL-2.0-or-later
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use mdns_sd::{ServiceDaemon, ServiceEvent};
use serde::Serialize;

use crate::error::NetError;

const SERVICE: &str = "_canon-chmp._tcp.local.";

/// A printer advertising `_canon-chmp._tcp`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DiscoveredPrinter {
    /// Service instance name.
    pub name: String,
    /// mDNS host name without the trailing dot (e.g. `printer.local`).
    pub host: String,
    /// Addresses as text; link-local IPv6 carries its `%interface` scope.
    pub addresses: Vec<String>,
    pub port: u16,
    /// TXT `mpath`: the CHMP command path advertised by the printer.
    pub mpath: Option<String>,
}

impl DiscoveredPrinter {
    /// What to connect to: an IPv4 address if one was advertised, otherwise the host name,
    /// otherwise the first address.
    pub fn connect_host(&self) -> String {
        let v4 = self.addresses.iter().find(|a| a.parse::<std::net::Ipv4Addr>().is_ok());
        if let Some(a) = v4 {
            return a.clone();
        }
        if !self.host.is_empty() {
            return self.host.clone();
        }
        self.addresses.first().cloned().unwrap_or_default()
    }
}

fn browse(timeout: Duration, first_only: bool) -> Result<Vec<DiscoveredPrinter>, NetError> {
    let daemon = ServiceDaemon::new().map_err(|e| NetError::Mdns(e.to_string()))?;
    let rx = daemon.browse(SERVICE).map_err(|e| NetError::Mdns(e.to_string()))?;
    let deadline = Instant::now() + timeout;
    let mut found: BTreeMap<String, DiscoveredPrinter> = BTreeMap::new();
    while let Some(remaining) = deadline.checked_duration_since(Instant::now()) {
        match rx.recv_timeout(remaining) {
            Ok(ServiceEvent::ServiceResolved(info)) => {
                let fullname = info.get_fullname().to_string();
                let name =
                    fullname.strip_suffix(&format!(".{SERVICE}")).unwrap_or(&fullname).to_string();
                let entry = found.entry(fullname).or_insert_with(|| DiscoveredPrinter {
                    name,
                    host: info.get_hostname().trim_end_matches('.').to_string(),
                    addresses: Vec::new(),
                    port: info.get_port(),
                    mpath: info.get_property_val_str("mpath").map(str::to_string),
                });
                for a in info.get_addresses() {
                    let a = a.to_string();
                    if !entry.addresses.contains(&a) {
                        entry.addresses.push(a);
                    }
                }
                entry.addresses.sort();
                if first_only {
                    break;
                }
            }
            Ok(_) => {}
            Err(_) => break,
        }
    }
    let _ = daemon.shutdown();
    Ok(found.into_values().collect())
}

/// Browse `_canon-chmp._tcp` for `timeout` and return everything that resolved.
pub fn discover(timeout: Duration) -> Result<Vec<DiscoveredPrinter>, NetError> {
    browse(timeout, false)
}

/// Return as soon as the first printer resolves (or `None` after `timeout`).
pub fn discover_first(timeout: Duration) -> Result<Option<DiscoveredPrinter>, NetError> {
    Ok(browse(timeout, true)?.into_iter().next())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(host: &str, addrs: &[&str]) -> DiscoveredPrinter {
        DiscoveredPrinter {
            name: "n".into(),
            host: host.into(),
            addresses: addrs.iter().map(|s| s.to_string()).collect(),
            port: 80,
            mpath: None,
        }
    }

    #[test]
    fn connect_host_preference() {
        assert_eq!(p("a.local", &["fe80::1%eth0", "192.0.2.7"]).connect_host(), "192.0.2.7");
        assert_eq!(p("a.local", &["fe80::1%eth0"]).connect_host(), "a.local");
        assert_eq!(p("", &["fe80::1%eth0"]).connect_host(), "fe80::1%eth0");
    }
}
