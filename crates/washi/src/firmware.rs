// SPDX-License-Identifier: GPL-2.0-or-later
//! `washi firmware` and `washi links`.

use anyhow::Result;
use washi_core::firmware::plan_update;
use washi_core::{Confirm, FirmwareProgress, FirmwareStatus, FirmwareUpdate, Language, WebLinks};

use crate::cli::{Cli, FirmwareCmd, LinkLang};
use crate::commands::{connect, print_json};
use crate::render;

pub fn firmware(cli: &Cli, cmd: Option<&FirmwareCmd>) -> Result<()> {
    match cmd {
        None | Some(FirmwareCmd::Status) => status(cli),
        Some(FirmwareCmd::Update) => update(cli),
    }
}

fn status(cli: &Cli) -> Result<()> {
    let p = connect(cli)?;
    // The snapshot kept from the last time the printer was on says what it offered then.
    let last = p.default_cache().and_then(|c| c.load());
    let s = p.firmware_status_with(last.as_ref())?;
    if cli.json {
        return print_json(&s);
    }
    print!("{}", render_status(&s));
    Ok(())
}

pub(crate) fn render_status(s: &FirmwareStatus) -> String {
    let verdict = if s.asleep && !(s.update == FirmwareUpdate::Available && !s.remembered) {
        // In auto power-off the printer does not check for updates, so its "no update" is no
        // answer. Say what was known from the last time it was on.
        let known = match s.update {
            FirmwareUpdate::Unavailable => "up to date",
            FirmwareUpdate::Available => "an update was available",
            FirmwareUpdate::Unknown => "not known yet",
        };
        format!(
            "the printer checks for updates while it is on; last known: {known} (wake it with \
             `washi power on` to check)"
        )
    } else {
        match s.update {
            FirmwareUpdate::Unavailable => "up to date (the printer reports no update)".into(),
            FirmwareUpdate::Available => "an update is available".into(),
            FirmwareUpdate::Unknown => {
                "the printer does not say whether an update is available".into()
            }
        }
    };
    format!("Firmware  {}\nUpdate    {verdict}\n", s.version)
}

fn update(cli: &Cli) -> Result<()> {
    if !cli.confirm {
        print!("{}", render::render_plan(&plan_update()));
        println!(
            "This is UNVERIFIED on a real printer. The update takes several minutes, cannot be \
             undone, and the printer restarts: do not switch it off."
        );
        return Ok(());
    }
    let p = connect(cli)?;
    // Tests against the fake printer poll faster than the minutes a real update takes.
    let mut opts = p.firmware_options().clone();
    if let Some(ms) =
        std::env::var("WASHI_FIRMWARE_POLL_MS").ok().and_then(|v| v.parse::<u64>().ok())
    {
        opts.poll_interval = std::time::Duration::from_millis(ms.max(5));
    }
    let report = p.update_firmware(Confirm::explicit(), &opts, &mut |ev| match ev {
        FirmwareProgress::Sent => {
            println!("  the printer accepted the update; do not switch it off")
        }
        FirmwareProgress::Restarting => println!("  the printer is restarting"),
        FirmwareProgress::Waiting { elapsed, restarted } => println!(
            "  [{}] waiting for the printer to {}",
            render::fmt_duration(elapsed),
            if restarted { "come back" } else { "start installing" }
        ),
        FirmwareProgress::Back => println!("  the printer is back"),
    })?;
    if !report.restarted {
        println!(
            "The printer did not restart, so nothing was installed (version {}).",
            report.after
        );
    } else if report.before == report.after {
        println!("The printer restarted; the firmware version is still {}.", report.after);
    } else {
        println!(
            "Firmware updated from {} to {} in {}.",
            report.before,
            report.after,
            render::fmt_duration(report.elapsed)
        );
    }
    Ok(())
}

pub fn links(cli: &Cli, code: Option<&str>, lang: LinkLang) -> Result<()> {
    let p = connect(cli)?;
    let lang = match lang {
        LinkLang::En => Language::En,
        LinkLang::Ja => Language::Ja,
    };
    // The printer serves its links only while it is awake: the copy kept from then fills in.
    let cache = washi_core::LocalizationCache::for_printer(p.host(), p.chmp_port());
    let l = p.web_links_kept(code, lang, cache.as_ref());
    if cli.json {
        return print_json(&l);
    }
    print!("{}", render_links(&l));
    Ok(())
}

pub(crate) fn render_links(l: &WebLinks) -> String {
    let row = |name: &str, link: &washi_core::Link| {
        let who = match link.source {
            washi_core::LinkSource::Maker => "maker",
            washi_core::LinkSource::Printer => "printer",
        };
        format!("{name:<14}{:<8}{}\n", who, link.url)
    };
    let mut o = String::new();
    o.push_str(&row("ink", &l.ink));
    o.push_str(&row("manual", &l.help));
    if let Some(e) = &l.error {
        o.push_str(&row("error help", e));
    }
    o.push_str(&row("support", &l.support));
    o.push_str(&row("printer page", &l.printer_page));
    o.push_str(&row("maintenance", &l.maintenance_page));
    if let (true, Some(at)) = (l.from_printer, l.saved_at) {
        o.push_str(&format!(
            "(from the printer, saved {}; it serves them only while it is on)\n",
            render::fmt_date(at)
        ));
    } else if !l.from_printer {
        o.push_str("(the printer offered no links of its own: these are its web page)\n");
    }
    o
}
