// SPDX-License-Identifier: GPL-2.0-or-later
//! The snapshot cache: `~/.cache/washi/<printer-key>.json`.
//!
//! `<printer-key>` is the 64-bit FNV-1a hash of the (normalised) host name in hex: stable across
//! Rust versions, and the file name carries no host name, address or device identifier. The file
//! holds the last [`PrinterSnapshot`], which is what lets a sleeping or offline printer still show
//! ink tank models, trays and counters from its last awake read.

use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::snapshot::PrinterSnapshot;

const CACHE_VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
struct CacheFile {
    version: u32,
    snapshot: PrinterSnapshot,
}

/// Where one printer's last snapshot lives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotCache {
    path: PathBuf,
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// The cache key of a printer: a hash of the host name (and of the port when it is not 80).
pub fn printer_key(host: &str, port: u16) -> String {
    let mut norm = host.trim().trim_end_matches('.').to_ascii_lowercase();
    if port != 80 {
        norm.push_str(&format!(":{port}"));
    }
    format!("{:016x}", fnv1a64(norm.as_bytes()))
}

impl SnapshotCache {
    /// A cache stored at exactly this file.
    pub fn at(path: impl Into<PathBuf>) -> Self {
        SnapshotCache { path: path.into() }
    }

    /// `<dir>/<printer-key>.json`.
    pub fn in_dir(dir: &Path, host: &str, port: u16) -> Self {
        SnapshotCache { path: dir.join(format!("{}.json", printer_key(host, port))) }
    }

    /// `~/.cache/washi/<printer-key>.json` (`XDG_CACHE_HOME` is honoured); `None` if the system
    /// has no cache directory.
    pub fn for_printer(host: &str, port: u16) -> Option<Self> {
        Some(Self::in_dir(&Self::default_dir()?, host, port))
    }

    /// `~/.cache/washi`.
    pub fn default_dir() -> Option<PathBuf> {
        dirs::cache_dir().map(|d| d.join("washi"))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The stored snapshot. A missing, unreadable, corrupt or other-version file is `None`.
    pub fn load(&self) -> Option<PrinterSnapshot> {
        let text = std::fs::read_to_string(&self.path).ok()?;
        let file: CacheFile = serde_json::from_str(&text).ok()?;
        (file.version == CACHE_VERSION).then_some(file.snapshot)
    }

    /// Write atomically (temp file + rename); the directory is created with mode 0700.
    pub fn store(&self, snapshot: &PrinterSnapshot) -> io::Result<()> {
        if let Some(dir) = self.path.parent() {
            use std::os::unix::fs::DirBuilderExt;
            std::fs::DirBuilder::new().recursive(true).mode(0o700).create(dir)?;
        }
        let text = serde_json::to_string_pretty(&CacheFile {
            version: CACHE_VERSION,
            snapshot: snapshot.clone(),
        })
        .map_err(io::Error::other)?;
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, &self.path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot::Power;

    #[test]
    fn key_is_a_stable_hash_without_the_host_name() {
        let k = printer_key("printer.local", 80);
        assert_eq!(k.len(), 16);
        assert!(k.chars().all(|c| c.is_ascii_hexdigit()));
        assert!(!k.contains("printer") && !k.contains("local"));
        // FNV-1a test vectors pin the algorithm (and so the file names) across versions.
        assert_eq!(fnv1a64(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a64(b"a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(fnv1a64(b"foobar"), 0x8594_4171_f739_67e8);
        // Normalisation: case, trailing dot, whitespace; the default port is not part of it.
        assert_eq!(k, printer_key(" PrInTeR.LOCAL. ", 80));
        assert_ne!(k, printer_key("printer.local", 8080));
        assert_ne!(k, printer_key("other.local", 80));
    }

    #[test]
    fn store_and_load_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let cache = SnapshotCache::in_dir(&dir.path().join("washi"), "p.local", 80);
        assert_eq!(cache.load(), None);
        let snap =
            PrinterSnapshot { power: Power::Sleeping, last_updated: 7, ..Default::default() };
        cache.store(&snap).unwrap();
        assert_eq!(cache.load(), Some(snap.clone()));
        let name = cache.path().file_name().unwrap().to_string_lossy().into_owned();
        assert!(name.ends_with(".json") && !name.contains("p.local"), "{name}");
        // Overwrite, and no temp file is left behind.
        let snap2 = PrinterSnapshot { last_updated: 8, ..snap };
        cache.store(&snap2).unwrap();
        assert_eq!(cache.load(), Some(snap2));
        let files: Vec<_> = std::fs::read_dir(cache.path().parent().unwrap()).unwrap().collect();
        assert_eq!(files.len(), 1);
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(cache.path().parent().unwrap()).unwrap().permissions().mode();
        assert_eq!(mode & 0o077, 0, "cache dir is private: {mode:o}");
    }

    #[test]
    fn corrupt_or_foreign_files_are_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let cache = SnapshotCache::at(dir.path().join("x.json"));
        std::fs::write(cache.path(), "not json").unwrap();
        assert_eq!(cache.load(), None);
        std::fs::write(cache.path(), r#"{"version": 99, "snapshot": {}}"#).unwrap();
        assert_eq!(cache.load(), None);
        std::fs::write(cache.path(), r#"{"version": 1, "snapshot": {"power": "on"}}"#).unwrap();
        assert_eq!(cache.load().unwrap().power, Power::On);
        // Truncated file (e.g. power loss): ignored, replaced on the next store.
        std::fs::write(cache.path(), r#"{"version": 1, "snap"#).unwrap();
        assert_eq!(cache.load(), None);
        cache.store(&PrinterSnapshot::default()).unwrap();
        assert!(cache.load().is_some());
    }
}
