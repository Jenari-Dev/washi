// SPDX-License-Identifier: GPL-2.0-or-later
//! The slice of HTTP/1.1 that CHMP needs: build a request, parse a response
//! (`Content-Length`, `chunked`, or empty).

use std::io::{BufRead, BufReader, Read, Write};
use std::time::Duration;

use crate::error::NetError;

/// Upper bound for a reply body. The largest observed IVEC reply is about 30 KB; IPP replies
/// (`media-col-database`) reach ~370 KB.
pub const MAX_BODY: usize = 8 * 1024 * 1024;
/// Upper bound for a scanned page (a 1200 dpi A4 JPEG can be tens of megabytes).
pub const MAX_SCAN_BODY: usize = 512 * 1024 * 1024;
const MAX_LINE: usize = 16 * 1024;
const MAX_HEADERS: usize = 100;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,
    /// Header names lower-cased.
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl HttpResponse {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }

    /// The server announced that it will close the connection.
    pub fn closes_connection(&self) -> bool {
        self.header("connection").is_some_and(|v| v.to_ascii_lowercase().contains("close"))
    }
}

/// Format a request head plus body.
pub fn build_request(method: &str, path: &str, headers: &[(&str, String)], body: &[u8]) -> Vec<u8> {
    let mut out = format!("{method} {path} HTTP/1.1\r\n").into_bytes();
    for (k, v) in headers {
        out.extend_from_slice(format!("{k}: {v}\r\n").as_bytes());
    }
    out.extend_from_slice(b"\r\n");
    out.extend_from_slice(body);
    out
}

fn read_line(r: &mut impl BufRead) -> Result<String, NetError> {
    let mut buf = Vec::new();
    let n = r.by_ref().take(MAX_LINE as u64).read_until(b'\n', &mut buf)?;
    if n == 0 {
        return Err(NetError::Io(std::io::ErrorKind::UnexpectedEof.into()));
    }
    if buf.last() != Some(&b'\n') {
        return Err(NetError::Http("header line too long or truncated".into()));
    }
    while matches!(buf.last(), Some(b'\n' | b'\r')) {
        buf.pop();
    }
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

fn read_exact_vec(
    r: &mut impl BufRead,
    n: usize,
    into: &mut Vec<u8>,
    max: usize,
) -> Result<(), NetError> {
    if into.len().saturating_add(n) > max {
        return Err(NetError::Http("reply body too large".into()));
    }
    // Grow with the data actually received: a lying Content-Length must not preallocate.
    let got = r.by_ref().take(n as u64).read_to_end(into)?;
    if got != n {
        return Err(NetError::Io(std::io::ErrorKind::UnexpectedEof.into()));
    }
    Ok(())
}

/// Read one response (interim `1xx` replies are skipped); bodies up to [`MAX_BODY`].
pub fn read_response(r: &mut impl BufRead) -> Result<HttpResponse, NetError> {
    read_response_limited(r, MAX_BODY)
}

/// Like [`read_response`] with a caller-chosen body limit (scanned pages are large).
pub fn read_response_limited(
    r: &mut impl BufRead,
    max_body: usize,
) -> Result<HttpResponse, NetError> {
    let mut status_line = read_line(r)?;
    // Skip interim 1xx responses.
    let (status, mut headers) = loop {
        let mut parts = status_line.splitn(3, ' ');
        let version = parts.next().unwrap_or_default();
        if !version.starts_with("HTTP/") {
            return Err(NetError::Http(format!("bad status line {status_line:?}")));
        }
        let status: u16 = parts
            .next()
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| NetError::Http(format!("bad status line {status_line:?}")))?;
        let mut headers = Vec::new();
        loop {
            let line = read_line(r)?;
            if line.is_empty() {
                break;
            }
            if headers.len() >= MAX_HEADERS {
                return Err(NetError::Http("too many headers".into()));
            }
            let (k, v) = line
                .split_once(':')
                .ok_or_else(|| NetError::Http(format!("bad header line {line:?}")))?;
            headers.push((k.trim().to_ascii_lowercase(), v.trim().to_string()));
        }
        if (100..200).contains(&status) && status != 101 {
            status_line = read_line(r)?;
            continue;
        }
        break (status, headers);
    };

    let mut body = Vec::new();
    let find = |headers: &[(String, String)], name: &str| -> Option<String> {
        headers.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone())
    };
    let no_body = status == 204 || status == 304;
    if no_body {
        // nothing to read
    } else if find(&headers, "transfer-encoding")
        .is_some_and(|v| v.to_ascii_lowercase().contains("chunked"))
    {
        loop {
            let line = read_line(r)?;
            let size_str = line.split(';').next().unwrap_or_default().trim();
            let size = usize::from_str_radix(size_str, 16)
                .map_err(|_| NetError::Http(format!("bad chunk size {size_str:?}")))?;
            if size == 0 {
                // Trailers, then the final blank line.
                loop {
                    let t = read_line(r)?;
                    if t.is_empty() {
                        break;
                    }
                    if let Some((k, v)) = t.split_once(':') {
                        headers.push((k.trim().to_ascii_lowercase(), v.trim().to_string()));
                    }
                }
                break;
            }
            read_exact_vec(r, size, &mut body, max_body)?;
            let crlf = read_line(r)?;
            if !crlf.is_empty() {
                return Err(NetError::Http("missing CRLF after chunk".into()));
            }
        }
    } else if let Some(len) = find(&headers, "content-length") {
        let len: usize = len
            .trim()
            .parse()
            .map_err(|_| NetError::Http(format!("bad Content-Length {len:?}")))?;
        read_exact_vec(r, len, &mut body, max_body)?;
    } else if find(&headers, "connection").is_some_and(|v| v.to_ascii_lowercase().contains("close"))
    {
        let n = r.by_ref().take(max_body as u64 + 1).read_to_end(&mut body)?;
        if n > max_body {
            return Err(NetError::Http("reply body too large".into()));
        }
    }
    // Otherwise: keep-alive reply without framing headers = empty body.
    Ok(HttpResponse { status, headers, body })
}

/// Host part for a URI or `Host` header: IPv6 literals in brackets, zone id dropped.
pub(crate) fn uri_host(host: &str) -> String {
    let bare = host.split('%').next().unwrap_or(host);
    if bare.contains(':') { format!("[{bare}]") } else { bare.to_string() }
}

/// Where and how long to talk to a printer's HTTP server.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Endpoint<'a> {
    pub host: &'a str,
    pub port: u16,
    pub connect_timeout: Duration,
    /// Socket read/write timeout (a scan or a print job may keep the socket quiet for a while).
    pub io_timeout: Duration,
}

impl Endpoint<'_> {
    /// `Host` header value (port omitted when it is the scheme default `default_port`).
    pub fn host_header(&self, default_port: u16) -> String {
        if self.port == default_port {
            uri_host(self.host)
        } else {
            format!("{}:{}", uri_host(self.host), self.port)
        }
    }
}

/// One HTTP exchange on a fresh connection (`Connection: close`). `streamed` is an optional
/// body tail that is read from a `Read` and sent after `body` (large print jobs); its length
/// must be known up front because the request carries a `Content-Length`.
pub(crate) fn request_once(
    ep: Endpoint<'_>,
    method: &str,
    path: &str,
    headers: &[(&str, String)],
    body: &[u8],
    streamed: Option<(&mut dyn Read, u64)>,
    max_body: usize,
) -> Result<HttpResponse, NetError> {
    let stream = crate::tcp::connect(ep.host, ep.port, ep.connect_timeout, ep.io_timeout)?;
    let tail_len = streamed.as_ref().map_or(0, |(_, n)| *n);
    let mut all: Vec<(&str, String)> = headers.to_vec();
    all.push(("Connection", "close".to_string()));
    if !body.is_empty() || tail_len > 0 || method == "POST" {
        all.push(("Content-Length", (body.len() as u64 + tail_len).to_string()));
    }
    let head = build_request(method, path, &all, body);
    let mut reader = BufReader::new(stream);
    let sent = (|| -> Result<(), NetError> {
        reader.get_mut().write_all(&head)?;
        if let Some((src, n)) = streamed {
            let mut buf = vec![0u8; 64 * 1024];
            let mut left = n;
            while left > 0 {
                let want = buf.len().min(usize::try_from(left).unwrap_or(usize::MAX));
                let got = src.read(&mut buf[..want])?;
                if got == 0 {
                    return Err(NetError::Io(std::io::ErrorKind::UnexpectedEof.into()));
                }
                reader.get_mut().write_all(&buf[..got])?;
                left -= got as u64;
            }
        }
        reader.get_mut().flush()?;
        Ok(())
    })();
    match sent {
        Ok(()) => read_response_limited(&mut reader, max_body),
        // A printer that rejects a job early answers and closes while we are still sending:
        // prefer its answer over our broken pipe.
        Err(e) => read_response_limited(&mut reader, max_body).map_err(|_| e),
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    fn parse(raw: &str) -> Result<HttpResponse, NetError> {
        read_response(&mut Cursor::new(raw.as_bytes().to_vec()))
    }

    #[test]
    fn content_length_body() {
        let r = parse("HTTP/1.1 200 OK\r\nContent-Length: 5\r\nX-CHMP-Session: {A}\r\n\r\nhello")
            .unwrap();
        assert_eq!(r.status, 200);
        assert_eq!(r.body, b"hello");
        assert_eq!(r.header("x-chmp-session"), Some("{A}"));
    }

    #[test]
    fn chunked_body_with_extensions_and_trailers() {
        let r = parse(
            "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5;ext=1\r\nhello\r\n6\r\n world\r\n0\r\nX-T: 1\r\n\r\n",
        )
        .unwrap();
        assert_eq!(r.body, b"hello world");
        assert_eq!(r.header("x-t"), Some("1"));
    }

    #[test]
    fn empty_chunked_body() {
        let r = parse("HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n0\r\n\r\n").unwrap();
        assert!(r.body.is_empty());
    }

    #[test]
    fn no_framing_keepalive_means_empty() {
        let r = parse("HTTP/1.1 200 OK\r\nConnection: Keep-Alive\r\n\r\n").unwrap();
        assert!(r.body.is_empty());
        assert!(!r.closes_connection());
    }

    #[test]
    fn close_delimited_body() {
        let r = parse("HTTP/1.0 200 OK\r\nConnection: close\r\n\r\nrest").unwrap();
        assert_eq!(r.body, b"rest");
        assert!(r.closes_connection());
    }

    #[test]
    fn skips_interim_and_handles_204() {
        let r = parse("HTTP/1.1 100 Continue\r\n\r\nHTTP/1.1 204 No Content\r\n\r\n").unwrap();
        assert_eq!(r.status, 204);
    }

    #[test]
    fn rejects_garbage_and_truncation() {
        assert!(parse("garbage\r\n\r\n").is_err());
        assert!(parse("HTTP/1.1 200 OK\r\nContent-Length: 10\r\n\r\nshort").is_err());
        assert!(parse("HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\nzz\r\n").is_err());
        assert!(parse("").is_err());
    }

    #[test]
    fn request_format() {
        let b = build_request("POST", "/x", &[("Content-Length", "2".into())], b"hi");
        assert_eq!(b, b"POST /x HTTP/1.1\r\nContent-Length: 2\r\n\r\nhi");
    }
}
