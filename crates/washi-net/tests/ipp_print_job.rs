// SPDX-License-Identifier: GPL-2.0-or-later
//! IPP job operations against a scripted in-process server: the document follows the IPP
//! message in the same POST, replies map to typed results, refusals name the culprit.

mod common_server;

use std::io::Cursor;
use std::time::Duration;

use common_server::{http, serve};
use washi_net::ipp::{
    IdentifyAction, IppAttribute, IppError, IppGroup, IppMessage, IppOptions, IppValue, JobState,
    PrintJob, WhichJobs, tag,
};
use washi_net::{Confirm, IppClient, NetError};

fn client(addr: std::net::SocketAddr) -> IppClient {
    IppClient::with_options(
        "127.0.0.1",
        IppOptions {
            port: addr.port(),
            connect_timeout: Duration::from_secs(2),
            io_timeout: Duration::from_secs(2),
            job_timeout: Duration::from_secs(5),
            ..IppOptions::default()
        },
    )
}

fn reply(code: u16, groups: Vec<IppGroup>) -> Vec<u8> {
    let mut g = vec![IppGroup::new(
        tag::OPERATION_ATTRIBUTES,
        vec![
            IppAttribute::single("attributes-charset", IppValue::Charset("utf-8".into())),
            IppAttribute::single(
                "attributes-natural-language",
                IppValue::NaturalLanguage("en".into()),
            ),
        ],
    )];
    g.extend(groups);
    let body = IppMessage { version: (2, 0), code, request_id: 2, groups: g }.encode();
    http("200 OK", &[("Content-Type", "application/ipp")], &body)
}

fn job_group(id: i32, state: i32) -> IppGroup {
    IppGroup::new(
        tag::JOB_ATTRIBUTES,
        vec![
            IppAttribute::single("job-id", IppValue::Integer(id)),
            IppAttribute::single("job-state", IppValue::Enum(state)),
        ],
    )
}

fn job() -> PrintJob {
    PrintJob::new("test page", "image/pwg-raster")
}

#[test]
fn print_job_posts_the_message_and_the_document_together() {
    let (addr, seen) = serve(vec![reply(0, vec![job_group(77, 3)])]);
    let doc: Vec<u8> = (0..=255u8).cycle().take(200_000).collect();
    let got = client(addr)
        .print_job(&job(), &mut Cursor::new(doc.clone()), doc.len() as u64, Confirm::explicit())
        .unwrap();
    assert_eq!((got.id, got.state), (Some(77), Some(JobState::Pending)));

    let s = seen.lock().unwrap();
    assert_eq!((s[0].method.as_str(), s[0].path.as_str()), ("POST", "/ipp/print"));
    assert_eq!(s[0].header("content-type"), Some("application/ipp"));
    let (msg, used) = IppMessage::decode_prefix(&s[0].body).unwrap();
    assert_eq!(msg.code, 0x0002, "Print-Job");
    assert_eq!(s[0].body.len(), used + doc.len(), "Content-Length covers message + document");
    assert_eq!(&s[0].body[used..], &doc[..], "the document is untouched");
}

#[test]
fn validate_cancel_identify_and_reads() {
    let (addr, seen) = serve(vec![
        reply(0, vec![]),
        reply(0x0001, vec![]),
        reply(0, vec![]),
        reply(0, vec![job_group(5, 9)]),
        reply(0, vec![job_group(5, 5), job_group(6, 3)]),
    ]);
    let c = client(addr);
    let v = c.validate_job(&job(), Confirm::explicit()).unwrap();
    assert_eq!(v.status, 0);
    let ignored = c.validate_job(&job(), Confirm::explicit()).unwrap();
    assert_eq!(ignored.status_name, "successful-ok-ignored-or-substituted-attributes");
    c.cancel_job(5, "washi", Confirm::explicit()).unwrap();
    assert_eq!(c.get_job_attributes(5).unwrap().state, Some(JobState::Completed));
    let all = c.get_jobs(WhichJobs::NotCompleted).unwrap();
    assert_eq!(all.iter().filter_map(|j| j.id).collect::<Vec<_>>(), [5, 6]);

    let s = seen.lock().unwrap();
    let codes: Vec<u16> = s.iter().map(|r| IppMessage::decode(&r.body).unwrap().code).collect();
    assert_eq!(codes, [0x0004, 0x0004, 0x0008, 0x0009, 0x000A]);
}

#[test]
fn identify_sends_the_action() {
    let (addr, seen) = serve(vec![reply(0, vec![])]);
    client(addr).identify(IdentifyAction::Flash, "washi", Confirm::explicit()).unwrap();
    let m = IppMessage::decode(&seen.lock().unwrap()[0].body).unwrap();
    assert_eq!(m.code, 0x003C);
    let ops = m.group(tag::OPERATION_ATTRIBUTES).unwrap();
    assert_eq!(ops.get("identify-actions").unwrap().values, [IppValue::Keyword("flash".into())]);
}

#[test]
fn refusals_carry_the_status_and_the_unsupported_attributes() {
    let bad = IppGroup::new(
        tag::UNSUPPORTED_ATTRIBUTES,
        vec![IppAttribute::single(
            "media-col",
            IppValue::OutOfBand(washi_net::ipp::OutOfBand::Unsupported),
        )],
    );
    let (addr, _) = serve(vec![reply(0x040B, vec![bad])]);
    let e = client(addr)
        .print_job(&job(), &mut Cursor::new(vec![1, 2, 3]), 3, Confirm::explicit())
        .unwrap_err();
    assert!(matches!(e, NetError::Ipp(IppError::Rejected { code: 0x040B, .. })), "{e}");
    assert!(e.to_string().contains("media-col"), "{e}");
}

#[test]
fn media_database_is_requested_on_its_own() {
    let (addr, seen) = serve(vec![reply(
        0,
        vec![IppGroup::new(
            tag::PRINTER_ATTRIBUTES,
            vec![IppAttribute::single(
                "media-col-database",
                IppValue::Collection(vec![(
                    "media-size".into(),
                    vec![IppValue::Collection(vec![
                        ("x-dimension".into(), vec![IppValue::Integer(12000)]),
                        ("y-dimension".into(), vec![IppValue::Integer(12000)]),
                    ])],
                )]),
            )],
        )],
    )]);
    let db = client(addr).media_database().unwrap();
    assert_eq!(db.entries.len(), 1);
    let m = IppMessage::decode(&seen.lock().unwrap()[0].body).unwrap();
    let want = m.group(tag::OPERATION_ATTRIBUTES).unwrap().get("requested-attributes").unwrap();
    assert_eq!(want.values, [IppValue::Keyword("media-col-database".into())]);
}
