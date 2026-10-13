// SPDX-License-Identifier: GPL-2.0-or-later
//! IVEC request envelopes.
//!
//! Every operation Washi can send is a [`Request`] variant; the only way to get XML on the wire
//! is [`Request::to_xml`]. Free-form text values are XML-escaped.

use std::fmt;
use std::str::FromStr;

use serde::Serialize;

use crate::error::IvecError;
use crate::settings::DeviceSettings;
use crate::xml::escape;

pub const NS_IVEC: &str = "http://www.canon.com/ns/cmd/2008/07/common/";
pub const NS_VCN: &str = "http://www.canon.com/ns/cmd/2008/07/canon/";

/// IVEC `servicetype`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub enum Service {
    Print,
    Device,
    Maintenance,
    Eis,
    Pli,
    Scan,
    Other(String),
}

impl Service {
    pub fn as_str(&self) -> &str {
        match self {
            Service::Print => "print",
            Service::Device => "device",
            Service::Maintenance => "maintenance",
            Service::Eis => "eis",
            Service::Pli => "pli",
            Service::Scan => "scan",
            Service::Other(s) => s,
        }
    }
}

impl fmt::Display for Service {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Service {
    type Err = IvecError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim().to_ascii_lowercase();
        Ok(match s.as_str() {
            "print" => Service::Print,
            "device" => Service::Device,
            "maintenance" => Service::Maintenance,
            "eis" => Service::Eis,
            "pli" => Service::Pli,
            "scan" => Service::Scan,
            other => {
                if other.is_empty()
                    || !other.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
                {
                    return Err(IvecError::invalid(
                        "service",
                        format!("bad service name {other:?}"),
                    ));
                }
                Service::Other(other.to_string())
            }
        })
    }
}

/// Read-only operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum ReadOp {
    GetStatus,
    GetCapability,
    GetConfiguration,
}

impl ReadOp {
    pub fn as_str(self) -> &'static str {
        match self {
            ReadOp::GetStatus => "GetStatus",
            ReadOp::GetCapability => "GetCapability",
            ReadOp::GetConfiguration => "GetConfiguration",
        }
    }
}

impl fmt::Display for ReadOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for ReadOp {
    type Err = IvecError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "getstatus" | "status" => Ok(ReadOp::GetStatus),
            "getcapability" | "capability" => Ok(ReadOp::GetCapability),
            "getconfiguration" | "configuration" => Ok(ReadOp::GetConfiguration),
            other => {
                Err(IvecError::invalid("operation", format!("{other:?} is not a read operation")))
            }
        }
    }
}

/// Eight-character job identifier. Canon PRINT and the Windows driver use `00000002`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct JobId(String);

impl JobId {
    pub fn new(id: &str) -> Result<Self, IvecError> {
        if id.is_empty() || id.len() > 16 || !id.bytes().all(|b| b.is_ascii_alphanumeric()) {
            return Err(IvecError::invalid(
                "jobID",
                format!("{id:?} is not a short alphanumeric id"),
            ));
        }
        Ok(JobId(id.to_string()))
    }

    /// The id Canon PRINT uses for maintenance and settings jobs.
    pub fn standard() -> Self {
        JobId("00000002".to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for JobId {
    fn default() -> Self {
        JobId::standard()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum CleaningType {
    Regular,
    Deep,
    /// Paper feed roller cleaning as a `Cleaning` type (models that list `roller`). **Unverified**:
    /// the XK140 does not offer it, the request has no ink group.
    Roller,
    /// Bottom plate cleaning (models that list `platen`). **Unverified**, like `Roller`.
    Platen,
}

impl CleaningType {
    pub fn as_str(self) -> &'static str {
        match self {
            CleaningType::Regular => "regular",
            CleaningType::Deep => "deep",
            CleaningType::Roller => "roller",
            CleaningType::Platen => "platen",
        }
    }

    /// The types Washi knows by name; anything else a printer lists (`choke`, ...) is ignored.
    pub fn parse(s: &str) -> Option<CleaningType> {
        match s.trim().to_ascii_lowercase().as_str() {
            "regular" => Some(CleaningType::Regular),
            "deep" => Some(CleaningType::Deep),
            "roller" => Some(CleaningType::Roller),
            "platen" => Some(CleaningType::Platen),
            _ => None,
        }
    }

    /// Print head cleanings pick an ink group; the roller and the bottom plate do not.
    pub fn has_ink_group(self) -> bool {
        matches!(self, CleaningType::Regular | CleaningType::Deep)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum InkGroup {
    All,
    Group1,
    Group2,
}

impl InkGroup {
    pub fn as_str(self) -> &'static str {
        match self {
            InkGroup::All => "all",
            InkGroup::Group1 => "group1",
            InkGroup::Group2 => "group2",
        }
    }
}

impl FromStr for InkGroup {
    type Err = IvecError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "all" => Ok(InkGroup::All),
            "1" | "group1" => Ok(InkGroup::Group1),
            "2" | "group2" => Ok(InkGroup::Group2),
            other => {
                Err(IvecError::invalid("inkgroup", format!("expected all, 1 or 2, got {other:?}")))
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum TestPrintKind {
    NozzleCheck,
    AutoRegistration,
    /// Print the manual alignment pattern sheet; the id is the `setregistration` id (1..=3).
    ManualRegistration(u8),
    RegiCheck,
}

impl TestPrintKind {
    pub fn type_str(self) -> &'static str {
        match self {
            TestPrintKind::NozzleCheck => "nozzle_check",
            TestPrintKind::AutoRegistration => "auto_registration",
            TestPrintKind::ManualRegistration(_) => "manual_registration",
            TestPrintKind::RegiCheck => "regi_check",
        }
    }
}

/// `VendorCmd` payloads. Service counters (`ResetCounter`, `ClearHRC`, `ClearRRC`) are
/// deliberately not representable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum VendorCommand {
    /// Firmware update from Canon's server.
    UpdateFRom,
}

impl VendorCommand {
    pub fn as_str(self) -> &'static str {
        match self {
            VendorCommand::UpdateFRom => "UpdateFRom",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    Get {
        op: ReadOp,
        service: Service,
    },
    StartJob {
        service: Service,
        job: JobId,
        bidi: bool,
        username: Option<String>,
        computername: Option<String>,
        job_description: Option<String>,
    },
    SetJobConfiguration {
        service: Service,
        job: JobId,
        datetime: String,
    },
    Cleaning {
        job: JobId,
        kind: CleaningType,
        group: InkGroup,
    },
    TestPrint {
        job: JobId,
        kind: TestPrintKind,
    },
    RollerCleaning {
        job: JobId,
    },
    /// Device settings update (service `device`).
    SetConfiguration {
        job: JobId,
        settings: DeviceSettings,
    },
    EndJob {
        service: Service,
        job: JobId,
    },
    CancelJob {
        service: Service,
        job: JobId,
    },
    PowerOff,
    /// Wake a printer that is in auto power-off (device service, no job). **Unverified** on a
    /// real printer: the string comes from the Windows driver's service-command library
    /// (`PowerOn`), which sends it without a job bracket like `PowerOff`.
    PowerOn,
    VendorCmd(VendorCommand),
}

impl Request {
    pub fn get_status(service: Service) -> Self {
        Request::Get { op: ReadOp::GetStatus, service }
    }

    pub fn get_capability(service: Service) -> Self {
        Request::Get { op: ReadOp::GetCapability, service }
    }

    pub fn get_configuration(service: Service) -> Self {
        Request::Get { op: ReadOp::GetConfiguration, service }
    }

    pub fn start_job(service: Service, job: JobId) -> Self {
        Request::StartJob {
            service,
            job,
            bidi: true,
            username: None,
            computername: None,
            job_description: None,
        }
    }

    pub fn end_job(service: Service, job: JobId) -> Self {
        Request::EndJob { service, job }
    }

    pub fn cancel_job(service: Service, job: JobId) -> Self {
        Request::CancelJob { service, job }
    }

    pub fn cleaning(job: JobId, kind: CleaningType, group: InkGroup) -> Self {
        Request::Cleaning { job, kind, group }
    }

    pub fn test_print(job: JobId, kind: TestPrintKind) -> Self {
        Request::TestPrint { job, kind }
    }

    pub fn roller_cleaning(job: JobId) -> Self {
        Request::RollerCleaning { job }
    }

    pub fn set_device_settings(job: JobId, settings: DeviceSettings) -> Self {
        Request::SetConfiguration { job, settings }
    }

    pub fn power_off() -> Self {
        Request::PowerOff
    }

    /// See [`Request::PowerOn`]: unverified.
    pub fn power_on() -> Self {
        Request::PowerOn
    }

    pub fn update_firmware() -> Self {
        Request::VendorCmd(VendorCommand::UpdateFRom)
    }

    /// True for `GetStatus` / `GetCapability` / `GetConfiguration` only.
    pub fn is_read_only(&self) -> bool {
        matches!(self, Request::Get { .. })
    }

    pub fn operation(&self) -> &'static str {
        match self {
            Request::Get { op, .. } => op.as_str(),
            Request::StartJob { .. } => "StartJob",
            Request::SetJobConfiguration { .. } => "SetJobConfiguration",
            Request::Cleaning { .. } => "Cleaning",
            Request::TestPrint { .. } => "TestPrint",
            Request::RollerCleaning { .. } => "RollerCleaning",
            Request::SetConfiguration { .. } => "SetConfiguration",
            Request::EndJob { .. } => "EndJob",
            Request::CancelJob { .. } => "CancelJob",
            Request::PowerOff => "PowerOff",
            Request::PowerOn => "PowerOn",
            Request::VendorCmd(_) => "VendorCmd",
        }
    }

    pub fn service(&self) -> Service {
        match self {
            Request::Get { service, .. }
            | Request::StartJob { service, .. }
            | Request::SetJobConfiguration { service, .. }
            | Request::EndJob { service, .. }
            | Request::CancelJob { service, .. } => service.clone(),
            Request::Cleaning { .. }
            | Request::TestPrint { .. }
            | Request::RollerCleaning { .. } => Service::Maintenance,
            Request::SetConfiguration { .. }
            | Request::PowerOff
            | Request::PowerOn
            | Request::VendorCmd(_) => Service::Device,
        }
    }

    fn params(&self) -> String {
        let job = |j: &JobId| format!("<ivec:jobID>{}</ivec:jobID>", escape(j.as_str()));
        match self {
            Request::Get { .. } | Request::PowerOff | Request::PowerOn => String::new(),
            Request::StartJob { job: j, bidi, username, computername, job_description, .. } => {
                let mut x = job(j);
                x.push_str(&format!("<ivec:bidi>{}</ivec:bidi>", if *bidi { "1" } else { "0" }));
                for (tag, v) in [
                    ("username", username),
                    ("computername", computername),
                    ("job_description", job_description),
                ] {
                    if let Some(v) = v {
                        x.push_str(&format!("<ivec:{tag}>{}</ivec:{tag}>", escape(v)));
                    }
                }
                x
            }
            Request::SetJobConfiguration { job: j, datetime, .. } => {
                format!("{}<ivec:datetime>{}</ivec:datetime>", job(j), escape(datetime))
            }
            Request::Cleaning { job: j, kind, group } => {
                let mut x = format!("{}<ivec:type>{}</ivec:type>", job(j), kind.as_str());
                if kind.has_ink_group() {
                    x.push_str(&format!("<ivec:inkgroup>{}</ivec:inkgroup>", group.as_str()));
                }
                x
            }
            Request::TestPrint { job: j, kind } => {
                let mut x = format!("{}<ivec:type>{}</ivec:type>", job(j), kind.type_str());
                if let TestPrintKind::ManualRegistration(id) = kind {
                    x.push_str(&format!("<ivec:id>{id}</ivec:id>"));
                }
                x
            }
            Request::RollerCleaning { job: j } => job(j),
            Request::SetConfiguration { job: j, settings } => {
                format!("{}{}", job(j), settings.to_xml_fragment())
            }
            Request::EndJob { job: j, .. } | Request::CancelJob { job: j, .. } => job(j),
            Request::VendorCmd(cmd) => {
                format!("<vcn:ijoperation>{}</vcn:ijoperation>", cmd.as_str())
            }
        }
    }

    /// The exact request body: XML declaration, `cmd` envelope with both namespaces,
    /// `ivec:operation`, and one `ivec:param_set`. Compact (no added whitespace).
    pub fn to_xml(&self) -> String {
        format!(
            "<?xml version=\"1.0\" encoding=\"utf-8\" ?>\
             <cmd xmlns:ivec=\"{NS_IVEC}\" xmlns:vcn=\"{NS_VCN}\">\
             <ivec:contents><ivec:operation>{op}</ivec:operation>\
             <ivec:param_set servicetype=\"{svc}\">{params}</ivec:param_set>\
             </ivec:contents></cmd>",
            op = self.operation(),
            svc = escape(self.service().as_str()),
            params = self.params(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xml::Node;

    fn parsed(r: &Request) -> Node {
        Node::parse(&r.to_xml()).expect("request XML must be well-formed")
    }

    #[test]
    fn get_envelope_matches_known_good_template() {
        let x = Request::get_status(Service::Print).to_xml();
        assert_eq!(
            x,
            "<?xml version=\"1.0\" encoding=\"utf-8\" ?>\
             <cmd xmlns:ivec=\"http://www.canon.com/ns/cmd/2008/07/common/\" \
             xmlns:vcn=\"http://www.canon.com/ns/cmd/2008/07/canon/\">\
             <ivec:contents><ivec:operation>GetStatus</ivec:operation>\
             <ivec:param_set servicetype=\"print\"></ivec:param_set></ivec:contents></cmd>"
        );
    }

    #[test]
    fn every_request_is_well_formed_with_expected_operation() {
        let job = JobId::standard();
        let reqs = vec![
            Request::get_capability(Service::Device),
            Request::get_configuration(Service::Maintenance),
            Request::start_job(Service::Maintenance, job.clone()),
            Request::SetJobConfiguration {
                service: Service::Maintenance,
                job: job.clone(),
                datetime: "20260101120000".into(),
            },
            Request::cleaning(job.clone(), CleaningType::Deep, InkGroup::Group2),
            Request::test_print(job.clone(), TestPrintKind::NozzleCheck),
            Request::test_print(job.clone(), TestPrintKind::AutoRegistration),
            Request::test_print(job.clone(), TestPrintKind::ManualRegistration(2)),
            Request::test_print(job.clone(), TestPrintKind::RegiCheck),
            Request::roller_cleaning(job.clone()),
            Request::set_device_settings(
                job.clone(),
                DeviceSettings { drylevel: Some(2), ..Default::default() },
            ),
            Request::end_job(Service::Device, job.clone()),
            Request::cancel_job(Service::Maintenance, job.clone()),
            Request::power_off(),
            Request::power_on(),
            Request::update_firmware(),
        ];
        for r in reqs {
            let n = parsed(&r);
            assert_eq!(n.name, "cmd");
            let contents = n.child("contents").unwrap();
            assert_eq!(contents.child_text("operation"), Some(r.operation()));
            let ps = contents.child("param_set").unwrap();
            assert_eq!(ps.attr("servicetype"), Some(r.service().as_str()));
        }
    }

    #[test]
    fn only_get_is_read_only() {
        assert!(Request::get_status(Service::Print).is_read_only());
        assert!(!Request::power_off().is_read_only());
        assert!(!Request::power_on().is_read_only());
        assert!(!Request::start_job(Service::Device, JobId::standard()).is_read_only());
        assert!(!Request::update_firmware().is_read_only());
    }

    #[test]
    fn start_job_and_maintenance_params() {
        let job = JobId::standard();
        let n = parsed(&Request::start_job(Service::Maintenance, job.clone()));
        let ps = n.find(&["contents", "param_set"]).unwrap();
        assert_eq!(ps.child_text("jobID"), Some("00000002"));
        assert_eq!(ps.child_text("bidi"), Some("1"));

        let n = parsed(&Request::cleaning(job.clone(), CleaningType::Regular, InkGroup::All));
        let ps = n.find(&["contents", "param_set"]).unwrap();
        assert_eq!(ps.child_text("type"), Some("regular"));
        assert_eq!(ps.child_text("inkgroup"), Some("all"));

        // The roller and the bottom plate have no ink group.
        for (kind, name) in [(CleaningType::Roller, "roller"), (CleaningType::Platen, "platen")] {
            let n = parsed(&Request::cleaning(job.clone(), kind, InkGroup::All));
            let ps = n.find(&["contents", "param_set"]).unwrap();
            assert_eq!(ps.child_text("type"), Some(name));
            assert!(ps.child("inkgroup").is_none(), "{name}");
            assert_eq!(ps.child_text("jobID"), Some("00000002"));
        }
        assert_eq!(CleaningType::parse("Roller"), Some(CleaningType::Roller));
        assert_eq!(CleaningType::parse("choke"), None);

        let n = parsed(&Request::test_print(job.clone(), TestPrintKind::ManualRegistration(3)));
        let ps = n.find(&["contents", "param_set"]).unwrap();
        assert_eq!(ps.child_text("type"), Some("manual_registration"));
        assert_eq!(ps.child_text("id"), Some("3"));

        let n = parsed(&Request::test_print(job, TestPrintKind::NozzleCheck));
        assert!(n.find(&["contents", "param_set"]).unwrap().child("id").is_none());
    }

    #[test]
    fn values_are_escaped() {
        let mut r = Request::start_job(Service::Maintenance, JobId::standard());
        if let Request::StartJob { username, job_description, .. } = &mut r {
            *username = Some("a<b>&\"c\"".into());
            *job_description = Some("]]><x/>".into());
        }
        let xml = r.to_xml();
        assert!(!xml.contains("<x/>"));
        let n = Node::parse(&xml).unwrap();
        let ps = n.find(&["contents", "param_set"]).unwrap();
        assert_eq!(ps.child_text("username"), Some("a<b>&\"c\""));
        assert_eq!(ps.child_text("job_description"), Some("]]><x/>"));
        assert!(JobId::new("00 02").is_err());
        assert!(JobId::new("").is_err());
        assert!("a\"b".parse::<Service>().is_err());
    }

    #[test]
    fn parse_helpers() {
        assert_eq!("GetStatus".parse::<ReadOp>().unwrap(), ReadOp::GetStatus);
        assert!("StartJob".parse::<ReadOp>().is_err());
        assert_eq!("1".parse::<InkGroup>().unwrap(), InkGroup::Group1);
        assert_eq!("Print".parse::<Service>().unwrap(), Service::Print);
        assert!(matches!("foo".parse::<Service>().unwrap(), Service::Other(_)));
    }
}
