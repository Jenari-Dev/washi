// SPDX-License-Identifier: GPL-2.0-or-later
//! Run the fake printer by hand: `washi-fake-printer [--chmp-port N] [--ipp-port N] [--asleep]
//! [--scan-pages N] [--scan-image FILE.jpg] [--roller] [--code CODE] [--firmware-update VERSION]
//! [--cleaning-types roller,platen,...] [--no-model-js]`.
//! Control commands are read from stdin (`sleep`, `wake`, `ink C 10`, `code 1300`, `log`, ...).
//! It only listens on 127.0.0.1.

use std::io::BufRead;

use washi_fake::FakePrinter;

fn main() {
    let mut chmp = 0u16;
    let mut ipp = 0u16;
    let mut asleep = false;
    let mut scan_pages: Option<usize> = None;
    let mut roller = false;
    let mut code: Option<String> = None;
    let mut firmware: Option<String> = None;
    let mut cleaning: Vec<String> = Vec::new();
    let mut model_js = true;
    let mut scan_image: Option<Vec<u8>> = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--chmp-port" => chmp = args.next().and_then(|v| v.parse().ok()).unwrap_or(0),
            "--ipp-port" => ipp = args.next().and_then(|v| v.parse().ok()).unwrap_or(0),
            "--asleep" => asleep = true,
            "--roller" => roller = true,
            "--code" => code = args.next(),
            "--firmware-update" => firmware = args.next(),
            "--cleaning-types" => {
                cleaning = args
                    .next()
                    .unwrap_or_default()
                    .split(',')
                    .filter(|t| !t.is_empty())
                    .map(str::to_string)
                    .collect();
            }
            "--no-model-js" => model_js = false,
            "--scan-image" => {
                scan_image = args.next().and_then(|p| std::fs::read(p).ok());
            }
            "--scan-pages" => scan_pages = args.next().and_then(|v| v.parse().ok()),
            other => {
                eprintln!(
                    "usage: washi-fake-printer [--chmp-port N] [--ipp-port N] [--asleep] [--scan-pages N] [--scan-image FILE] [--roller] [--code CODE] [--firmware-update VERSION] [--cleaning-types a,b] [--no-model-js]"
                );
                eprintln!("unknown argument {other:?}");
                std::process::exit(2);
            }
        }
    }
    let fake = FakePrinter::start_on(chmp, ipp);
    fake.set_asleep(asleep);
    fake.set_roller_cleaning(roller);
    if let Some(code) = code {
        fake.set_support_code(&code);
    }
    if let Some(version) = firmware {
        fake.offer_firmware_update(&version, 20_000);
    }
    let types: Vec<&str> = cleaning.iter().map(String::as_str).collect();
    fake.set_cleaning_types(&types);
    if !model_js {
        fake.set_model_js(None);
    }
    if let Some(jpeg) = scan_image {
        fake.set_scan_image(jpeg);
    }
    if let Some(n) = scan_pages {
        fake.set_scan_pages(n);
    }
    println!("CHMP 127.0.0.1:{}  IPP 127.0.0.1:{}", fake.chmp.port(), fake.ipp.port());
    for line in std::io::stdin().lock().lines().map_while(Result::ok) {
        println!("{}", fake.command(&line));
    }
    // stdin closed (e.g. started from a script): keep serving until killed.
    loop {
        std::thread::sleep(std::time::Duration::from_secs(3600));
    }
}
