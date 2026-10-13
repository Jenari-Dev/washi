// SPDX-License-Identifier: GPL-2.0-or-later
use std::io::{BufReader, Write};
use std::net::TcpStream;
use std::thread::sleep;
use std::time::{Duration, Instant};

use uuid::Uuid;
use washi_ivec::{Request, Response};

use crate::error::NetError;
use crate::http::{HttpResponse, build_request, read_response};

/// IVEC control port for the `print`, `device` and `maintenance` services.
pub const DEFAULT_PATH: &str = "/canon/ij/command2/port1";
const CHMP_VERSION: &str = "1.3.0";

#[derive(Debug, Clone)]
pub struct ClientOptions {
    pub port: u16,
    pub path: String,
    pub connect_timeout: Duration,
    /// Socket read/write timeout for one HTTP exchange.
    pub io_timeout: Duration,
    /// Total budget for polling a reply to appear after a command was written.
    pub read_timeout: Duration,
    /// Open a new TCP connection for every HTTP request (the session GUID is kept).
    pub reconnect_per_request: bool,
    pub poll_initial: Duration,
    pub poll_max: Duration,
}

impl Default for ClientOptions {
    fn default() -> Self {
        ClientOptions {
            port: 80,
            path: DEFAULT_PATH.to_string(),
            connect_timeout: Duration::from_secs(5),
            io_timeout: Duration::from_secs(10),
            read_timeout: Duration::from_secs(20),
            reconnect_per_request: false,
            poll_initial: Duration::from_millis(50),
            poll_max: Duration::from_millis(400),
        }
    }
}

/// Proof that a human explicitly agreed to change the printer. There is deliberately no
/// `Default`, and it is only obtainable through [`Confirm::explicit`].
#[derive(Debug, Clone, Copy)]
pub struct Confirm(());

impl Confirm {
    /// Call this only on the code path that runs after an explicit user confirmation
    /// (`--confirm` on the command line, a dialog in a GUI).
    pub fn explicit() -> Confirm {
        Confirm(())
    }
}

type Conn = BufReader<TcpStream>;

/// One CHMP session with a printer. Connects lazily; the session is closed on [`close`] / drop.
///
/// [`close`]: ChmpClient::close
pub struct ChmpClient {
    host: String,
    opts: ClientOptions,
    session: String,
    conn: Option<Conn>,
    session_open: bool,
    trace: bool,
    polls: u32,
    connections: u32,
}

fn new_session() -> String {
    let mut buf = Uuid::encode_buffer();
    let id = Uuid::new_v4().hyphenated().encode_upper(&mut buf);
    format!("{{{id}}}")
}

impl ChmpClient {
    pub fn new(host: impl Into<String>) -> Self {
        Self::with_options(host, ClientOptions::default())
    }

    pub fn with_options(host: impl Into<String>, opts: ClientOptions) -> Self {
        ChmpClient {
            host: host.into(),
            opts,
            session: new_session(),
            conn: None,
            session_open: false,
            trace: false,
            polls: 0,
            connections: 0,
        }
    }

    /// Print one line per HTTP exchange to stderr (status, sizes; never bodies).
    pub fn set_trace(&mut self, on: bool) {
        self.trace = on;
    }

    pub fn session_id(&self) -> &str {
        &self.session
    }

    /// Read polls (`GET`) used by the most recent exchange.
    pub fn last_polls(&self) -> u32 {
        self.polls
    }

    /// TCP connections opened so far.
    pub fn connections_opened(&self) -> u32 {
        self.connections
    }

    /// Send a read-only request (`GetStatus` / `GetCapability` / `GetConfiguration`).
    pub fn query(&mut self, req: &Request) -> Result<Response, NetError> {
        if !req.is_read_only() {
            return Err(NetError::NotReadOnly(req.operation()));
        }
        self.exchange(req)
    }

    /// Send any request. Writes require a [`Confirm`].
    pub fn execute(&mut self, req: &Request, _confirm: Confirm) -> Result<Response, NetError> {
        self.exchange(req)
    }

    /// End the CHMP session. Safe to call repeatedly; a later request starts a new session.
    pub fn close(&mut self) {
        if !self.session_open {
            return;
        }
        self.drop_dead_connection();
        let _ = self.roundtrip("POST", &[], true);
        self.conn = None;
        self.session_open = false;
        self.session = new_session();
    }

    fn host_header(&self) -> String {
        let bare = self.host.split('%').next().unwrap_or(&self.host);
        let h = if bare.contains(':') { format!("[{bare}]") } else { bare.to_string() };
        if self.opts.port == 80 { h } else { format!("{h}:{}", self.opts.port) }
    }

    fn connect(&mut self) -> Result<Conn, NetError> {
        let s = crate::tcp::connect(
            &self.host,
            self.opts.port,
            self.opts.connect_timeout,
            self.opts.io_timeout,
        )?;
        self.connections += 1;
        if self.trace {
            eprintln!("[chmp] connected (connection #{})", self.connections);
        }
        Ok(BufReader::new(s))
    }

    /// Forget a reused connection the printer has closed in the meantime (idle timeout).
    fn drop_dead_connection(&mut self) {
        if let Some(c) = &self.conn {
            if !connection_alive(c.get_ref()) {
                if self.trace {
                    eprintln!("[chmp] connection was closed by the printer");
                }
                self.conn = None;
            }
        }
    }

    /// One HTTP request/response on the current connection (opened on demand). Never retried
    /// here: the printer ties a reply to the connection that carried the command (a `GET` from
    /// another connection is answered with `409`), so recovery restarts the whole exchange.
    fn roundtrip(
        &mut self,
        method: &str,
        body: &[u8],
        closing: bool,
    ) -> Result<HttpResponse, NetError> {
        if self.conn.is_none() {
            let c = self.connect()?;
            self.conn = Some(c);
        }
        let mut headers: Vec<(&str, String)> = vec![("Host", self.host_header())];
        if method == "POST" {
            headers.push(("Content-Type", "application/octet-stream".to_string()));
            headers.push(("Content-Length", body.len().to_string()));
        }
        headers.push(("X-CHMP-Version", CHMP_VERSION.to_string()));
        headers.push(("X-CHMP-Session", self.session.clone()));
        headers.push(("Connection", if closing { "Close" } else { "Keep-Alive" }.to_string()));
        let bytes = build_request(method, &self.opts.path, &headers, body);

        let conn = self.conn.as_mut().expect("connection was just established");
        let result = (|| {
            conn.get_mut().write_all(&bytes)?;
            conn.get_mut().flush()?;
            read_response(conn)
        })();
        match result {
            Ok(resp) => {
                if self.trace {
                    eprintln!(
                        "[chmp] {method} -> HTTP {} ({} body bytes)",
                        resp.status,
                        resp.body.len()
                    );
                }
                dump_bodies(method, body, &resp.body);
                if closing || resp.closes_connection() {
                    self.conn = None;
                }
                Ok(resp)
            }
            Err(e) => {
                self.conn = None;
                Err(e)
            }
        }
    }

    /// One command and its reply, both on the same connection. A reused connection that turns
    /// out to be dead is replaced and a read-only exchange is repeated once; a write is never
    /// sent twice.
    fn exchange(&mut self, req: &Request) -> Result<Response, NetError> {
        self.session_open = true;
        if self.opts.reconnect_per_request {
            self.conn = None;
        }
        self.drop_dead_connection();
        let reused = self.conn.is_some();
        let mut result = self.exchange_once(req);
        if matches!(result, Err(NetError::Io(_) | NetError::Http(_)))
            && reused
            && req.is_read_only()
        {
            if self.trace {
                eprintln!("[chmp] stale connection, repeating the read on a new one");
            }
            result = self.exchange_once(req);
        }
        if self.opts.reconnect_per_request || result.is_err() {
            self.conn = None;
        }
        result
    }

    fn exchange_once(&mut self, req: &Request) -> Result<Response, NetError> {
        self.polls = 0;
        let xml = req.to_xml();
        let written = self.roundtrip("POST", xml.as_bytes(), false)?;
        if !(200..300).contains(&written.status) {
            return Err(NetError::Status { status: written.status });
        }
        self.read_reply()
    }

    /// Poll the read side until a non-empty, non-busy reply arrives or the budget runs out.
    fn read_reply(&mut self) -> Result<Response, NetError> {
        let started = Instant::now();
        let deadline = started + self.opts.read_timeout;
        let mut delay = self.opts.poll_initial;
        let mut busy: Option<Response> = None;
        loop {
            self.polls += 1;
            let resp = self.roundtrip("GET", &[], false)?;
            match resp.status {
                200 => {
                    let text = String::from_utf8_lossy(&resp.body);
                    if !text.trim().is_empty() {
                        let parsed = Response::parse(&text)?;
                        if !parsed.is_busy() {
                            return Ok(parsed);
                        }
                        busy = Some(parsed);
                    }
                }
                // Reply not ready yet.
                202 | 204 | 423 | 503 => {}
                status => return Err(NetError::Status { status }),
            }
            let now = Instant::now();
            if now >= deadline {
                return match busy {
                    Some(r) => Ok(r),
                    None => Err(NetError::Timeout(self.opts.read_timeout)),
                };
            }
            sleep(delay.min(deadline - now));
            delay = (delay * 2).min(self.opts.poll_max);
        }
    }
}

/// Protocol debugging: with `WASHI_TRACE_DIR=<dir>` set, every command body and reply body is
/// written to `<dir>/NNNN-{req,reply}.xml`. Off by default; the files can contain device
/// identifiers, so they are for local analysis only.
fn dump_bodies(method: &str, sent: &[u8], received: &[u8]) {
    use std::sync::atomic::{AtomicU32, Ordering};
    static SEQ: AtomicU32 = AtomicU32::new(0);
    let Some(dir) = std::env::var_os("WASHI_TRACE_DIR") else { return };
    let dir = std::path::PathBuf::from(dir);
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    if !sent.is_empty() {
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let _ = std::fs::write(dir.join(format!("{n:04}-{method}-req.xml")), sent);
    }
    if !received.is_empty() {
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let _ = std::fs::write(dir.join(format!("{n:04}-{method}-reply.xml")), received);
    }
}

/// True if the peer has not closed the connection and sent nothing unexpected.
fn connection_alive(stream: &TcpStream) -> bool {
    if stream.set_nonblocking(true).is_err() {
        return false;
    }
    let mut probe = [0u8; 1];
    let alive = match stream.peek(&mut probe) {
        Ok(_) => false, // EOF, or stray data from an earlier exchange
        Err(e) => e.kind() == std::io::ErrorKind::WouldBlock,
    };
    alive && stream.set_nonblocking(false).is_ok()
}

impl Drop for ChmpClient {
    fn drop(&mut self) {
        self.close();
    }
}

impl std::fmt::Debug for ChmpClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChmpClient")
            .field("host", &self.host)
            .field("session_open", &self.session_open)
            .finish_non_exhaustive()
    }
}
