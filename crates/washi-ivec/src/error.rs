// SPDX-License-Identifier: GPL-2.0-or-later
use thiserror::Error;

#[derive(Debug, Error)]
pub enum IvecError {
    #[error("malformed XML: {0}")]
    Xml(String),
    #[error("unexpected reply structure: {0}")]
    Structure(String),
    #[error("printer refused {operation}: {detail}")]
    Ng { operation: String, detail: String },
    #[error("invalid value for {field}: {reason}")]
    Invalid { field: &'static str, reason: String },
}

impl IvecError {
    pub(crate) fn invalid(field: &'static str, reason: impl Into<String>) -> Self {
        IvecError::Invalid { field, reason: reason.into() }
    }
}
