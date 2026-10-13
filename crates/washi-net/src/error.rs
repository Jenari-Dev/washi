// SPDX-License-Identifier: GPL-2.0-or-later
use std::io;
use std::time::Duration;

use thiserror::Error;
use washi_ivec::IvecError;

#[derive(Debug, Error)]
pub enum NetError {
    #[error("cannot reach {host}: {source}")]
    Connect {
        host: String,
        #[source]
        source: io::Error,
    },
    #[error("I/O error talking to the printer: {0}")]
    Io(#[from] io::Error),
    #[error("malformed HTTP response: {0}")]
    Http(String),
    #[error("the printer answered HTTP {status}")]
    Status { status: u16 },
    #[error("no usable reply from the printer within {0:?}")]
    Timeout(Duration),
    #[error("{0} changes the printer; it must go through execute() with a Confirm")]
    NotReadOnly(&'static str),
    #[error(transparent)]
    Ivec(#[from] IvecError),
    #[error("mDNS discovery failed: {0}")]
    Mdns(String),
    #[error(transparent)]
    Ipp(#[from] crate::ipp::IppError),
    #[error(transparent)]
    Escl(#[from] crate::escl::EsclError),
}
