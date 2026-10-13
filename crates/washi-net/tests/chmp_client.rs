// SPDX-License-Identifier: GPL-2.0-or-later
//! CHMP client against an in-process fake printer.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use washi_ivec::{JobId, Request, Service};
use washi_net::{ChmpClient, ClientOptions, Confirm, NetError};

#[derive(Debug, Clone)]
struct Seen {
    method: String,
    headers: Vec<(String, String)>,
    body: String,
    conn: usize,
}

impl Seen {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }
}

#[derive(Default)]
struct Script {
    /// Number of GETs answered with an empty chunked body before the real reply.
    empty_gets: usize,
    /// Number of GETs answered with a DeviceBusy reply before the real reply.
    busy_gets: usize,
    /// Close the TCP connection after every completed command/reply pair (an idle timeout).
    drop_after_each: bool,
    /// Never produce a reply.
    never_reply: bool,
}

struct Fake {
    addr: SocketAddr,
    seen: Arc<Mutex<Vec<Seen>>>,
}

fn reply_xml(op: &str, detail: &str) -> String {
    let resp = if detail.is_empty() { "OK" } else { "NG" };
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<cmd xmlns:ivec=\"http://www.canon.com/ns/cmd/2008/07/common/\">\n\
         <ivec:contents>\n<ivec:operation>{op}Response</ivec:operation>\n<ivec:param_set servicetype=\"print\">\n\
         <ivec:response>{resp}</ivec:response>\n<ivec:response_detail>{detail}</ivec:response_detail>\n\
         <ivec:status>idle</ivec:status>\n</ivec:param_set>\n</ivec:contents>\n</cmd>"
    )
}

fn chunked(body: &str) -> Vec<u8> {
    let mut out = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nContent-Type: application/octet-stream\r\n\r\n".to_vec();
    // Two chunks to exercise reassembly.
    let (a, b) = body.split_at(body.len() / 2);
    for part in [a, b] {
        if !part.is_empty() {
            out.extend_from_slice(format!("{:X}\r\n{part}\r\n", part.len()).as_bytes());
        }
    }
    out.extend_from_slice(b"0\r\n\r\n");
    out
}

fn spawn_fake(script: Script) -> Fake {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let script = Arc::new(Mutex::new((script, 0usize))); // (script, GETs served for the current command)
    let seen2 = seen.clone();
    thread::spawn(move || {
        for (i, stream) in listener.incoming().enumerate() {
            let Ok(stream) = stream else { break };
            let conn_no = i + 1;
            let (seen, script) = (seen2.clone(), script.clone());
            thread::spawn(move || serve(stream, conn_no, seen, script));
        }
    });
    Fake { addr, seen }
}

fn serve(
    stream: TcpStream,
    conn_no: usize,
    seen: Arc<Mutex<Vec<Seen>>>,
    script: Arc<Mutex<(Script, usize)>>,
) {
    // Like the real printer, a reply belongs to the connection that carried the command.
    let mut pending: Option<String> = None;
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut out = stream;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap_or(0) == 0 {
            return;
        }
        let method = line.split(' ').next().unwrap().to_string();
        let mut headers = Vec::new();
        loop {
            let mut h = String::new();
            reader.read_line(&mut h).unwrap();
            let h = h.trim_end().to_string();
            if h.is_empty() {
                break;
            }
            let (k, v) = h.split_once(':').unwrap();
            headers.push((k.to_string(), v.trim().to_string()));
        }
        let len: usize = headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("content-length"))
            .map(|(_, v)| v.parse().unwrap())
            .unwrap_or(0);
        let mut body = vec![0; len];
        reader.read_exact(&mut body).unwrap();
        let body = String::from_utf8(body).unwrap();
        let closing = headers
            .iter()
            .any(|(k, v)| k.eq_ignore_ascii_case("connection") && v.eq_ignore_ascii_case("close"));
        seen.lock().unwrap().push(Seen {
            method: method.clone(),
            headers,
            body: body.clone(),
            conn: conn_no,
        });

        let mut guard = script.lock().unwrap();
        let (s, gets) = &mut *guard;
        let reply: Vec<u8> = if method == "POST" {
            if !body.is_empty() {
                *gets = 0;
                pending = Some(
                    body.split("<ivec:operation>")
                        .nth(1)
                        .and_then(|r| r.split("</ivec:operation>").next())
                        .unwrap_or("Unknown")
                        .to_string(),
                );
            }
            b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: Keep-Alive\r\n\r\n".to_vec()
        } else if let Some(op) = &pending {
            *gets += 1;
            if s.never_reply || *gets <= s.empty_gets {
                chunked("")
            } else if *gets <= s.empty_gets + s.busy_gets {
                chunked(&reply_xml(op, "DeviceBusy"))
            } else {
                chunked(&reply_xml(op, ""))
            }
        } else {
            b"HTTP/1.1 409 Conflict\r\nContent-Length: 0\r\n\r\n".to_vec()
        };
        // Idle timeout: the printer hangs up once a command/reply pair is complete.
        let drop_now = s.drop_after_each && method == "GET" && *gets > s.empty_gets + s.busy_gets;
        drop(guard);
        out.write_all(&reply).unwrap();
        if closing || drop_now {
            return;
        }
    }
}

fn client(f: &Fake, tweak: impl FnOnce(&mut ClientOptions)) -> ChmpClient {
    let mut o = ClientOptions {
        port: f.addr.port(),
        read_timeout: Duration::from_secs(5),
        poll_initial: Duration::from_millis(5),
        poll_max: Duration::from_millis(20),
        ..ClientOptions::default()
    };
    tweak(&mut o);
    ChmpClient::with_options("127.0.0.1", o)
}

fn seen(f: &Fake) -> Vec<Seen> {
    f.seen.lock().unwrap().clone()
}

#[test]
fn query_sends_chmp_headers_and_reads_with_the_same_session() {
    let f = spawn_fake(Script::default());
    let mut c = client(&f, |_| {});
    let r = c.query(&Request::get_status(Service::Print)).unwrap();
    assert!(r.ok);
    assert_eq!(r.service, "print");
    assert_eq!(r.operation, "GetStatusResponse");
    drop(c);

    let s = seen(&f);
    assert_eq!(s[0].method, "POST");
    assert_eq!(s[0].header("content-type"), Some("application/octet-stream"));
    assert_eq!(s[0].header("x-chmp-version"), Some("1.3.0"));
    assert_eq!(s[0].header("content-length"), Some(s[0].body.len().to_string().as_str()));
    assert_eq!(s[0].header("connection"), Some("Keep-Alive"));
    assert!(s[0].body.contains("<ivec:operation>GetStatus</ivec:operation>"));
    let sid = s[0].header("x-chmp-session").unwrap().to_string();
    assert_eq!(sid.len(), 38);
    assert!(sid.starts_with('{') && sid.ends_with('}'));
    assert!(
        sid.chars().all(|ch| ch == '{'
            || ch == '}'
            || ch == '-'
            || ch.is_ascii_digit()
            || ('A'..='F').contains(&ch)),
        "{sid}"
    );
    assert_eq!(s[1].method, "GET");
    assert_eq!(s[1].header("x-chmp-session"), Some(sid.as_str()));
    // Session close on drop.
    let last = s.last().unwrap();
    assert_eq!((last.method.as_str(), last.body.as_str()), ("POST", ""));
    assert_eq!(last.header("content-length"), Some("0"));
    assert_eq!(last.header("connection"), Some("Close"));
    assert_eq!(last.header("x-chmp-session"), Some(sid.as_str()));
}

#[test]
fn polls_while_the_reply_body_is_empty() {
    let f = spawn_fake(Script { empty_gets: 3, ..Script::default() });
    let mut c = client(&f, |_| {});
    let r = c.query(&Request::get_status(Service::Print)).unwrap();
    assert!(r.ok);
    assert_eq!(c.last_polls(), 4);
}

#[test]
fn polls_while_the_printer_reports_busy() {
    let f = spawn_fake(Script { busy_gets: 2, ..Script::default() });
    let mut c = client(&f, |_| {});
    let r = c.query(&Request::get_status(Service::Print)).unwrap();
    assert!(r.ok, "{r:?}");
    assert_eq!(c.last_polls(), 3);
}

#[test]
fn times_out_when_no_reply_ever_comes() {
    let f = spawn_fake(Script { never_reply: true, ..Script::default() });
    let mut c = client(&f, |o| o.read_timeout = Duration::from_millis(150));
    let err = c.query(&Request::get_status(Service::Print)).unwrap_err();
    assert!(matches!(err, NetError::Timeout(_)), "{err:?}");
}

#[test]
fn persistent_mode_reuses_one_connection() {
    let f = spawn_fake(Script::default());
    let mut c = client(&f, |_| {});
    c.query(&Request::get_status(Service::Print)).unwrap();
    c.query(&Request::get_capability(Service::Device)).unwrap();
    assert_eq!(c.connections_opened(), 1);
    drop(c);
    assert!(seen(&f).iter().all(|s| s.conn == 1));
}

#[test]
fn reconnect_per_request_mode_keeps_the_session() {
    let f = spawn_fake(Script::default());
    let mut c = client(&f, |o| o.reconnect_per_request = true);
    c.query(&Request::get_status(Service::Print)).unwrap();
    c.query(&Request::get_configuration(Service::Device)).unwrap();
    let sid = c.session_id().to_string();
    // One connection per command/reply pair; the close is a third.
    assert_eq!(c.connections_opened(), 2);
    drop(c);
    let s = seen(&f);
    assert!(s.iter().all(|x| x.header("x-chmp-session") == Some(sid.as_str())));
    let conns: std::collections::BTreeSet<_> = s.iter().map(|x| x.conn).collect();
    assert_eq!(conns.len(), 3, "{s:?}");
    // The command and its reply always share a connection.
    for pair in s.windows(2).filter(|w| w[0].method == "POST" && w[1].method == "GET") {
        assert_eq!(pair[0].conn, pair[1].conn);
    }
}

#[test]
fn survives_a_connection_the_printer_dropped() {
    let f = spawn_fake(Script { drop_after_each: true, ..Script::default() });
    let mut c = client(&f, |_| {});
    for _ in 0..3 {
        assert!(c.query(&Request::get_status(Service::Print)).unwrap().ok);
    }
}

#[test]
fn a_dropped_connection_is_replaced_before_a_write_is_sent() {
    let f = spawn_fake(Script { drop_after_each: true, ..Script::default() });
    let mut c = client(&f, |_| {});
    let job = JobId::standard();
    c.execute(&Request::start_job(Service::Device, job.clone()), Confirm::explicit()).unwrap();
    std::thread::sleep(Duration::from_millis(100)); // let the printer's close arrive
    c.execute(&Request::end_job(Service::Device, job), Confirm::explicit()).unwrap();
    drop(c);
    let writes: Vec<_> =
        seen(&f).into_iter().filter(|s| s.method == "POST" && !s.body.is_empty()).collect();
    assert_eq!(writes.len(), 2, "each write must be sent exactly once: {writes:?}");
}

#[test]
fn query_refuses_writes_without_touching_the_network() {
    let f = spawn_fake(Script::default());
    let mut c = client(&f, |_| {});
    for req in [
        Request::start_job(Service::Maintenance, JobId::standard()),
        Request::power_off(),
        Request::update_firmware(),
        Request::end_job(Service::Device, JobId::standard()),
    ] {
        let err = c.query(&req).unwrap_err();
        assert!(matches!(err, NetError::NotReadOnly(_)), "{err:?}");
    }
    drop(c);
    assert!(seen(&f).is_empty(), "nothing may reach the printer");
}

#[test]
fn execute_sends_writes_when_confirmed() {
    let f = spawn_fake(Script::default());
    let mut c = client(&f, |_| {});
    let r = c
        .execute(&Request::start_job(Service::Maintenance, JobId::standard()), Confirm::explicit())
        .unwrap();
    assert!(r.ok);
    drop(c);
    let s = seen(&f);
    assert!(s[0].body.contains("<ivec:operation>StartJob</ivec:operation>"));
    assert!(s[0].body.contains("<ivec:jobID>00000002</ivec:jobID>"));
}

#[test]
fn connection_failure_is_a_clear_error() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let mut c = ChmpClient::with_options(
        "127.0.0.1",
        ClientOptions {
            port,
            connect_timeout: Duration::from_millis(500),
            ..ClientOptions::default()
        },
    );
    let err = c.query(&Request::get_status(Service::Print)).unwrap_err();
    assert!(matches!(err, NetError::Connect { .. }), "{err:?}");
}
