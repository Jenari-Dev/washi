// SPDX-License-Identifier: GPL-2.0-or-later
//! Optional helper programs, detected at run time: poppler's `pdftoppm` / `pdfinfo` (PDF
//! rasterising for printing), LibreOffice's `soffice` (office documents) and `tesseract` (OCR).
//! Each can be overridden with an environment variable (`WASHI_PDFTOPPM`, `WASHI_PDFINFO`,
//! `WASHI_SOFFICE`, `WASHI_TESSERACT`); a missing tool only disables the features that need it.

use std::ffi::OsStr;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::error::{Error, Result};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Tools {
    pub pdftoppm: Option<PathBuf>,
    pub pdfinfo: Option<PathBuf>,
    pub soffice: Option<PathBuf>,
    pub tesseract: Option<PathBuf>,
}

fn is_executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    p.metadata().is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

/// First executable called `name` in the `PATH`-style list `path_var`.
pub fn find_in_path(name: &str, path_var: &OsStr) -> Option<PathBuf> {
    std::env::split_paths(path_var).map(|d| d.join(name)).find(|p| is_executable(p))
}

fn resolve(env_var: &str, names: &[&str], path_var: &OsStr) -> Option<PathBuf> {
    if let Some(v) = std::env::var_os(env_var).filter(|v| !v.is_empty()) {
        let p = PathBuf::from(v);
        return is_executable(&p).then_some(p);
    }
    names.iter().find_map(|n| find_in_path(n, path_var))
}

impl Tools {
    /// Look in `$PATH`.
    pub fn detect() -> Tools {
        let path = std::env::var_os("PATH").unwrap_or_default();
        Tools::detect_in(&path)
    }

    /// Look in a given `PATH`-style list (environment overrides still apply).
    pub fn detect_in(path_var: &OsStr) -> Tools {
        Tools {
            pdftoppm: resolve("WASHI_PDFTOPPM", &["pdftoppm"], path_var),
            pdfinfo: resolve("WASHI_PDFINFO", &["pdfinfo"], path_var),
            soffice: resolve("WASHI_SOFFICE", &["soffice", "libreoffice"], path_var),
            tesseract: resolve("WASHI_TESSERACT", &["tesseract"], path_var),
        }
    }

    /// PDF rasterising needs both poppler programs.
    pub fn can_print_pdf(&self) -> bool {
        self.pdftoppm.is_some() && self.pdfinfo.is_some()
    }

    pub fn can_convert_office(&self) -> bool {
        self.soffice.is_some()
    }

    pub fn can_ocr(&self) -> bool {
        self.tesseract.is_some()
    }

    /// A one-line-per-tool report for `washi doctor`-style output.
    pub fn report(&self) -> Vec<(&'static str, &'static str, Option<&Path>)> {
        vec![
            ("pdftoppm", "PDF printing (poppler)", self.pdftoppm.as_deref()),
            ("pdfinfo", "PDF page counts (poppler)", self.pdfinfo.as_deref()),
            ("soffice", "Office documents (LibreOffice)", self.soffice.as_deref()),
            ("tesseract", "OCR", self.tesseract.as_deref()),
        ]
    }
}

/// Run a helper program to completion and capture its output. Both pipes are drained on
/// threads (a rasterised page is 100 MB), and the process is killed after `timeout`.
pub fn run_capture(mut cmd: Command, timeout: Duration, what: &str) -> Result<Output> {
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    // A program that was written an instant ago can be "text file busy" while another thread
    // is still forking; that clears within milliseconds.
    let mut attempts = 0;
    let mut child = loop {
        match cmd.spawn() {
            Ok(c) => break c,
            Err(e) if e.raw_os_error() == Some(26) && attempts < 20 => {
                attempts += 1;
                std::thread::sleep(Duration::from_millis(25));
            }
            Err(e) => return Err(Error::tool(what, format!("cannot start: {e}"))),
        }
    };
    fn drain<R: Read + Send + 'static>(r: Option<R>) -> std::thread::JoinHandle<Vec<u8>> {
        std::thread::spawn(move || {
            let mut v = Vec::new();
            if let Some(mut r) = r {
                let _ = r.read_to_end(&mut v);
            }
            v
        })
    }
    let (out, err) = (drain(child.stdout.take()), drain(child.stderr.take()));
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) if started.elapsed() > timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(Error::tool(
                    what,
                    format!("no answer within {}s, stopped", timeout.as_secs()),
                ));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(e) => return Err(Error::tool(what, format!("wait failed: {e}"))),
        }
    };
    Ok(Output {
        status,
        stdout: out.join().unwrap_or_default(),
        stderr: err.join().unwrap_or_default(),
    })
}

/// The last non-empty line of a helper's stderr, for error messages.
pub fn stderr_hint(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr)
        .lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("no message")
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    fn fake_tool(dir: &Path, name: &str, mode: u32) {
        let p = dir.join(name);
        std::fs::write(&p, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(mode)).unwrap();
    }

    #[test]
    fn detects_executables_only() {
        let dir = tempfile::tempdir().unwrap();
        fake_tool(dir.path(), "pdftoppm", 0o755);
        fake_tool(dir.path(), "pdfinfo", 0o644); // not executable
        fake_tool(dir.path(), "libreoffice", 0o755);
        let t = Tools::detect_in(dir.path().as_os_str());
        // Environment overrides are process-wide; only assert what the temp PATH decides when
        // none is set.
        if std::env::var_os("WASHI_PDFTOPPM").is_none() {
            assert_eq!(t.pdftoppm.as_deref(), Some(dir.path().join("pdftoppm").as_path()));
        }
        if std::env::var_os("WASHI_PDFINFO").is_none() {
            assert!(t.pdfinfo.is_none() && !t.can_print_pdf());
        }
        if std::env::var_os("WASHI_SOFFICE").is_none() {
            assert!(t.can_convert_office(), "libreoffice is accepted as soffice");
        }
        if std::env::var_os("WASHI_TESSERACT").is_none() {
            assert!(!t.can_ocr());
        }
        assert_eq!(t.report().len(), 4);
        let o = run_capture(Command::new("sh"), Duration::from_secs(5), "sh").unwrap();
        assert!(o.status.success());
        let mut c = Command::new("sh");
        c.args(["-c", "echo out; echo oops >&2; exit 3"]);
        let o = run_capture(c, Duration::from_secs(5), "sh").unwrap();
        assert_eq!((o.status.code(), String::from_utf8_lossy(&o.stdout).trim()), (Some(3), "out"));
        assert_eq!(stderr_hint(&o), "oops");
        let mut slow = Command::new("sleep");
        slow.arg("5");
        let e = run_capture(slow, Duration::from_millis(100), "sleep").unwrap_err();
        assert!(e.to_string().contains("stopped"), "{e}");
        assert!(
            run_capture(Command::new("/nonexistent/tool"), Duration::from_secs(1), "x").is_err()
        );
        assert!(find_in_path("definitely-not-here", dir.path().as_os_str()).is_none());
    }
}
