// SPDX-License-Identifier: GPL-2.0-or-later
use thiserror::Error;
use washi_ivec::IvecError;
use washi_net::NetError;
use washi_net::escl::EsclError;
use washi_net::ipp::IppError;

use crate::settings::Mismatch;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    #[error(transparent)]
    Net(#[from] NetError),
    #[error(transparent)]
    Ivec(#[from] IvecError),
    #[error("{0}")]
    Invalid(String),
    #[error("timed out: {0}")]
    Timeout(String),
    #[error("the printer did not keep the new settings: {}", fmt_mismatches(.0))]
    Mismatch(Vec<Mismatch>),
    #[error("config file {path}: {msg}")]
    Config { path: String, msg: String },
    #[error(
        "no printer found: pass --host, save one with `washi config set-host`, or make sure the printer is on the network"
    )]
    NoPrinter,
    #[error("{error} (additionally, closing the job failed: {cleanup})")]
    WithCleanupFailure { error: Box<Error>, cleanup: String },
    #[error("{context}: {source}")]
    Io {
        context: String,
        #[source]
        source: std::io::Error,
    },
    #[error("{0}")]
    Image(String),
    #[error("{tool}: {msg}")]
    Tool { tool: String, msg: String },
    #[error("cancelled")]
    Cancelled,
}

impl Error {
    pub fn io(context: impl Into<String>, source: std::io::Error) -> Error {
        Error::Io { context: context.into(), source }
    }

    pub fn tool(tool: impl Into<String>, msg: impl Into<String>) -> Error {
        Error::Tool { tool: tool.into(), msg: msg.into() }
    }
}

impl Error {
    /// A plain-language hint for well-known refusals.
    pub fn hint(&self) -> Option<&'static str> {
        match self {
            Error::Ivec(IvecError::Ng { detail, .. }) => match detail.as_str() {
                "Suspended" => Some(
                    "the printer is asleep (its print service is suspended); wake it with its power button or by printing something, then try again",
                ),
                "DeviceBusy" => Some("the printer is busy; wait for it to finish and try again"),
                "NotSupportService" => {
                    Some("this printer does not offer that service on this port")
                }
                _ => None,
            },
            Error::WithCleanupFailure { error, .. } => error.hint(),
            Error::Net(NetError::Ipp(IppError::Rejected { code: 0x040B | 0x040E, .. })) => Some(
                "the printer does not accept that paper size / type / tray / quality combination; run `washi media` to see what it offers",
            ),
            Error::Net(NetError::Ipp(IppError::Rejected { code: 0x040A, .. })) => Some(
                "the printer does not take that file format; PDF and Office files are rasterised by washi, images are sent as JPEG",
            ),
            Error::Net(NetError::Ipp(IppError::Rejected { code: 0x0506, .. })) => {
                Some("the printer is not accepting jobs right now (check its display for an error)")
            }
            Error::Net(NetError::Escl(EsclError::JobRejected { status: 409, .. })) => {
                Some("close the scanner cover, and wait until the printer is idle")
            }
            _ => None,
        }
    }
}

fn fmt_mismatches(m: &[Mismatch]) -> String {
    m.iter()
        .map(|m| format!("{} wanted {} but the printer reports {}", m.field, m.wanted, m.actual))
        .collect::<Vec<_>>()
        .join("; ")
}
