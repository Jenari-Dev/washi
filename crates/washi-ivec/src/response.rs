// SPDX-License-Identifier: GPL-2.0-or-later
use serde::Serialize;

use crate::error::IvecError;
use crate::xml::Node;

/// A parsed IVEC reply: the common header plus the raw `param_set` for the typed views.
#[derive(Debug, Clone, Serialize)]
pub struct Response {
    /// e.g. `GetStatusResponse`.
    pub operation: String,
    /// The `servicetype` attribute of the param set (`unknown` when the printer could not
    /// resolve the service).
    pub service: String,
    /// `ivec:response` was `OK`.
    pub ok: bool,
    /// `ivec:response_detail` (empty on success), e.g. `NotSupportService`, `DataError`.
    pub detail: String,
    #[serde(skip)]
    param_set: Node,
    #[serde(skip)]
    raw: String,
}

impl Response {
    pub fn parse(xml: &str) -> Result<Response, IvecError> {
        let root = Node::parse(xml)?;
        if root.name != "cmd" {
            return Err(IvecError::Structure(format!(
                "root element is <{}>, expected <cmd>",
                root.name
            )));
        }
        let contents = root
            .child("contents")
            .ok_or_else(|| IvecError::Structure("missing <contents>".into()))?;
        let operation = contents.child_text("operation").unwrap_or_default().to_string();
        let param_set = contents
            .child("param_set")
            .ok_or_else(|| IvecError::Structure("missing <param_set>".into()))?
            .clone();
        let service = param_set.attr("servicetype").unwrap_or_default().to_string();
        let ok = param_set.child_text("response").is_some_and(|r| r.eq_ignore_ascii_case("OK"));
        let detail = param_set.child_text("response_detail").unwrap_or_default().to_string();
        Ok(Response { operation, service, ok, detail, param_set, raw: xml.to_string() })
    }

    /// The `param_set` element (header fields included).
    pub fn param_set(&self) -> &Node {
        &self.param_set
    }

    /// The reply exactly as received. May contain device identifiers; see [`crate::redact`].
    pub fn raw(&self) -> &str {
        &self.raw
    }

    /// The printer says it is busy (`DeviceBusy` and similar).
    pub fn is_busy(&self) -> bool {
        !self.ok && self.detail.to_ascii_lowercase().contains("busy")
    }

    /// `Ok(self)` when the reply is `OK`, otherwise an [`IvecError::Ng`].
    pub fn require_ok(&self) -> Result<&Response, IvecError> {
        if self.ok {
            Ok(self)
        } else {
            Err(IvecError::Ng {
                operation: self.operation.trim_end_matches("Response").to_string(),
                detail: if self.detail.is_empty() { "NG".to_string() } else { self.detail.clone() },
            })
        }
    }

    /// Leaf values of the reply below the header, for generic display.
    pub fn flatten(&self) -> Vec<(String, String)> {
        self.param_set
            .flatten()
            .into_iter()
            .filter(|(k, _)| k != "response" && k != "response_detail")
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE_NG: &str = include_str!("../tests/fixtures/GetStatus-eis.xml");

    #[test]
    fn ng_reply() {
        let r = Response::parse(FIXTURE_NG).unwrap();
        assert_eq!(r.operation, "GetStatusResponse");
        assert_eq!(r.service, "eis");
        assert!(!r.ok);
        assert_eq!(r.detail, "DataError");
        assert!(!r.is_busy());
        let err = r.require_ok().unwrap_err().to_string();
        assert!(err.contains("GetStatus") && err.contains("DataError"), "{err}");
    }

    #[test]
    fn busy_detection_and_structure_errors() {
        let busy = FIXTURE_NG.replace("DataError", "DeviceBusy");
        assert!(Response::parse(&busy).unwrap().is_busy());
        assert!(Response::parse("<x/>").is_err());
        assert!(Response::parse("<cmd/>").is_err());
        assert!(Response::parse("<cmd><contents/></cmd>").is_err());
    }

    #[test]
    fn unsupported_service_reply() {
        let r = Response::parse(include_str!("../tests/fixtures/GetStatus-scan.xml")).unwrap();
        assert_eq!(r.service, "unknown");
        assert_eq!(r.detail, "NotSupportService");
        assert!(!r.ok);
    }
}
