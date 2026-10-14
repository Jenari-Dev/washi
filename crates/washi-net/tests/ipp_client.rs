// SPDX-License-Identifier: GPL-2.0-or-later
//! The IPP client against an in-process HTTP server with constructed responses.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use washi_net::ipp::{IppAttribute, IppGroup, IppMessage, IppOptions, IppValue, PrinterState, tag};
use washi_net::{IppClient, NetError};

#[derive(Default, Clone)]
struct Seen {
    request_line: String,
    content_type: String,
    host: String,
    body: Vec<u8>,
}

/// One-shot server: answers the first request with `reply(body_len)` bytes.
fn serve(reply: Vec<u8>) -> (SocketAddr, Arc<Mutex<Seen>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let seen = Arc::new(Mutex::new(Seen::default()));
    let s2 = seen.clone();
    thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut out = stream;
        let mut seen = Seen::default();
        reader.read_line(&mut seen.request_line).unwrap();
        let mut len = 0usize;
        loop {
            let mut h = String::new();
            reader.read_line(&mut h).unwrap();
            let h = h.trim_end();
            if h.is_empty() {
                break;
            }
            let (k, v) = h.split_once(':').unwrap();
            match k.to_ascii_lowercase().as_str() {
                "content-length" => len = v.trim().parse().unwrap(),
                "content-type" => seen.content_type = v.trim().to_string(),
                "host" => seen.host = v.trim().to_string(),
                _ => {}
            }
        }
        seen.body = vec![0; len];
        reader.read_exact(&mut seen.body).unwrap();
        *s2.lock().unwrap() = seen;
        let _ = out.write_all(&reply);
    });
    (addr, seen)
}

fn client(addr: SocketAddr) -> IppClient {
    IppClient::with_options(
        "127.0.0.1",
        IppOptions {
            port: addr.port(),
            io_timeout: Duration::from_secs(2),
            connect_timeout: Duration::from_secs(2),
            ..IppOptions::default()
        },
    )
}

fn message(status: u16, printer: Vec<IppAttribute>) -> Vec<u8> {
    IppMessage {
        version: (1, 1),
        code: status,
        request_id: 1,
        groups: vec![IppGroup::new(tag::PRINTER_ATTRIBUTES, printer)],
    }
    .encode()
}

fn http_length(body: &[u8]) -> Vec<u8> {
    let mut out = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/ipp\r\nContent-Length: {}\r\n\r\n",
        body.len()
    )
    .into_bytes();
    out.extend_from_slice(body);
    out
}

fn http_chunked(body: &[u8]) -> Vec<u8> {
    let mut out =
        b"HTTP/1.1 200 OK\r\nContent-Type: application/ipp\r\nTransfer-Encoding: chunked\r\n\r\n"
            .to_vec();
    for part in body.chunks(7) {
        out.extend_from_slice(format!("{:X}\r\n", part.len()).as_bytes());
        out.extend_from_slice(part);
        out.extend_from_slice(b"\r\n");
    }
    out.extend_from_slice(b"0\r\n\r\n");
    out
}

fn attrs() -> Vec<IppAttribute> {
    vec![
        IppAttribute::single("printer-state", IppValue::Enum(4)),
        IppAttribute::single(
            "printer-state-reasons",
            IppValue::Keyword("marker-supply-low-warning".into()),
        ),
        IppAttribute::new("marker-names", vec![IppValue::Name("Cyan".into())]),
        IppAttribute::new("marker-levels", vec![IppValue::Integer(10)]),
        IppAttribute::new("marker-low-levels", vec![IppValue::Integer(15)]),
    ]
}

#[test]
fn sends_exactly_one_get_printer_attributes_post() {
    let (addr, seen) = serve(http_length(&message(0, attrs())));
    let a = client(addr).status().unwrap();
    assert_eq!(a.state, Some(PrinterState::Processing));
    assert_eq!(a.state_reasons, ["marker-supply-low-warning"]);
    assert_eq!(a.markers[0].level, Some(10));

    let s = seen.lock().unwrap().clone();
    assert_eq!(s.request_line.trim_end(), "POST /ipp/print HTTP/1.1");
    assert_eq!(s.content_type, "application/ipp");
    assert_eq!(s.host, format!("127.0.0.1:{}", addr.port()));
    let req = IppMessage::decode(&s.body).unwrap();
    assert_eq!((req.version, req.code), ((1, 1), 0x000B), "Get-Printer-Attributes");
    let ops = req.group(tag::OPERATION_ATTRIBUTES).unwrap();
    let uri = &ops.get("printer-uri").unwrap().values[0];
    assert_eq!(uri.as_str(), Some(format!("ipp://127.0.0.1:{}/ipp/print", addr.port()).as_str()));
    let wanted: Vec<_> =
        ops.get("requested-attributes").unwrap().values.iter().filter_map(|v| v.as_str()).collect();
    assert!(wanted.contains(&"marker-levels") && wanted.contains(&"printer-state-reasons"));
    assert!(req.group(tag::JOB_ATTRIBUTES).is_none() && req.groups.len() == 1);
}

#[test]
fn chunked_replies_are_reassembled() {
    let (addr, _) = serve(http_chunked(&message(0, attrs())));
    assert_eq!(client(addr).status().unwrap().markers.len(), 1);
}

#[test]
fn ipp_error_status_is_reported_by_name() {
    let (addr, _) = serve(http_length(&message(0x0507, vec![])));
    let err = client(addr).status().unwrap_err();
    assert!(matches!(err, NetError::Ipp(_)));
    assert!(err.to_string().contains("server-error-busy"), "{err}");
    // The raw call still returns the reply for callers that want the status themselves.
    let (addr, _) = serve(http_length(&message(0x0400, vec![])));
    assert_eq!(client(addr).get_printer_attributes_raw(&[]).unwrap().code, 0x0400);
}

#[test]
fn http_errors_and_garbage_are_errors() {
    let (addr, _) =
        serve(b"HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\n\r\n".to_vec());
    assert!(matches!(client(addr).status().unwrap_err(), NetError::Status { status: 500 }));
    let (addr, _) = serve(http_length(b"<html>not ipp</html>"));
    assert!(matches!(client(addr).status().unwrap_err(), NetError::Ipp(_)));
    let (addr, _) = serve(http_length(&[]));
    assert!(client(addr).status().is_err());
}

#[test]
fn refused_connection_names_the_host() {
    let port = {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    let c = IppClient::with_options(
        "127.0.0.1",
        IppOptions { port, connect_timeout: Duration::from_millis(300), ..IppOptions::default() },
    );
    assert!(matches!(c.status().unwrap_err(), NetError::Connect { .. }));
}

#[test]
fn the_localization_request_carries_the_language_and_asks_for_the_three_attributes() {
    let reply = message(
        0,
        vec![
            IppAttribute::single(
                "printer-strings-uri",
                IppValue::Uri("http://printer.example./strings/ja.txt".into()),
            ),
            IppAttribute::new(
                "printer-strings-languages-supported",
                vec![IppValue::Keyword("ja".into()), IppValue::Keyword("en".into())],
            ),
            IppAttribute::new(
                "printer-icons",
                vec![IppValue::Uri("http://printer.example./icon/big.png".into())],
            ),
            IppAttribute::single("printer-firmware-string-version", IppValue::Text("2.0".into())),
        ],
    );
    let (addr, seen) = serve(http_length(&reply));
    let l = client(addr).localization("JA").unwrap();
    assert_eq!(l.language, "ja");
    assert_eq!(l.strings_path("ja").as_deref(), Some("/strings/ja.txt"));
    assert_eq!(l.icon_paths(), ["/icon/big.png"]);
    assert_eq!(l.firmware_version.as_deref(), Some("2.0"));

    let req = IppMessage::decode(&seen.lock().unwrap().body).unwrap();
    assert_eq!(req.code, 0x000B, "a read: Get-Printer-Attributes");
    let ops = req.group(tag::OPERATION_ATTRIBUTES).unwrap();
    assert_eq!(ops.get("attributes-natural-language").unwrap().values[0].as_str(), Some("ja"));
    let wanted: Vec<_> =
        ops.get("requested-attributes").unwrap().values.iter().filter_map(|v| v.as_str()).collect();
    assert_eq!(
        wanted,
        [
            "printer-strings-uri",
            "printer-strings-languages-supported",
            "printer-icons",
            "printer-firmware-string-version"
        ]
    );
}

#[test]
fn a_bad_language_never_reaches_the_wire() {
    let port = {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    let c = IppClient::with_options("127.0.0.1", IppOptions { port, ..IppOptions::default() });
    for bad in ["", "ja\r\nX: y", "not a tag"] {
        let e = c.localization(bad).unwrap_err();
        assert!(matches!(e, NetError::Ipp(_)), "{bad:?}: {e}");
    }
}
