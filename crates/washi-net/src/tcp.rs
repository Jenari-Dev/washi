// SPDX-License-Identifier: GPL-2.0-or-later
//! Shared TCP connect helper.

use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use crate::error::NetError;

/// Resolve `host`, try IPv4 addresses first (link-local IPv6 needs a scope and is the usual
/// source of slow failures), and return a connected stream with read/write timeouts set.
pub(crate) fn connect(
    host: &str,
    port: u16,
    connect_timeout: Duration,
    io_timeout: Duration,
) -> Result<TcpStream, NetError> {
    let fail = |source| NetError::Connect { host: host.to_string(), source };
    let mut addrs: Vec<_> = (host, port).to_socket_addrs().map_err(fail)?.collect();
    addrs.sort_by_key(|a| a.is_ipv6());
    let mut last = None;
    for addr in addrs {
        match TcpStream::connect_timeout(&addr, connect_timeout) {
            Ok(s) => {
                s.set_read_timeout(Some(io_timeout))?;
                s.set_write_timeout(Some(io_timeout))?;
                let _ = s.set_nodelay(true);
                return Ok(s);
            }
            Err(e) => last = Some(e),
        }
    }
    Err(fail(last.unwrap_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::NotFound, "name resolved to no addresses")
    })))
}
