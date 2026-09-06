use std::fmt;
use std::str::FromStr;
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Record type requested for a low-level DNS query.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RecordTypeSpec {
    A,
    Aaaa,
    Mx,
    Txt,
    Ns,
    Soa,
    Cname,
    Srv,
    Caa,
    Ptr,
    Axfr,
    Other(u16),
}

impl RecordTypeSpec {
    const fn to_hickory(self) -> hickory_proto::rr::RecordType {
        use hickory_proto::rr::RecordType;
        match self {
            Self::A => RecordType::A,
            Self::Aaaa => RecordType::AAAA,
            Self::Mx => RecordType::MX,
            Self::Txt => RecordType::TXT,
            Self::Ns => RecordType::NS,
            Self::Soa => RecordType::SOA,
            Self::Cname => RecordType::CNAME,
            Self::Srv => RecordType::SRV,
            Self::Caa => RecordType::CAA,
            Self::Ptr => RecordType::PTR,
            Self::Axfr => RecordType::AXFR,
            Self::Other(code) => RecordType::Unknown(code),
        }
    }
}

impl fmt::Display for RecordTypeSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::A => "A",
            Self::Aaaa => "AAAA",
            Self::Mx => "MX",
            Self::Txt => "TXT",
            Self::Ns => "NS",
            Self::Soa => "SOA",
            Self::Cname => "CNAME",
            Self::Srv => "SRV",
            Self::Caa => "CAA",
            Self::Ptr => "PTR",
            Self::Axfr => "AXFR",
            Self::Other(code) => return write!(f, "TYPE{code}"),
        };
        f.write_str(s)
    }
}

impl FromStr for RecordTypeSpec {
    type Err = crate::dns::error::DnsError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Ok(match value {
            "A" => Self::A,
            "AAAA" => Self::Aaaa,
            "MX" => Self::Mx,
            "TXT" => Self::Txt,
            "NS" => Self::Ns,
            "SOA" => Self::Soa,
            "CNAME" => Self::Cname,
            "SRV" => Self::Srv,
            "CAA" => Self::Caa,
            "PTR" => Self::Ptr,
            "AXFR" => Self::Axfr,
            other => {
                let stripped = other.strip_prefix("TYPE").unwrap_or(other);
                let code = stripped
                    .parse::<u16>()
                    .map_err(|_| crate::dns::error::DnsError::InvalidInput(format!("unknown record type: {value}")))?;
                Self::Other(code)
            }
        })
    }
}

/// Per-query options for the low-level exchange.
#[derive(Debug, Clone)]
pub struct QueryOpts {
    pub rd: bool,
    pub timeout: Duration,
    pub edns_size: Option<u16>,
    pub edns_version: u8,
    pub dnssec_ok: bool,
    pub pinned_root_cert: Option<Vec<u8>>,
}

impl Default for QueryOpts {
    fn default() -> Self {
        Self {
            rd: true,
            timeout: Duration::from_secs(2),
            edns_size: None,
            edns_version: 0,
            dnssec_ok: false,
            pinned_root_cert: None,
        }
    }
}

/// One answer record in a [`QueryResultDto`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnswerDto {
    pub data: String,
    pub ttl: u32,
}

/// Result of a single low-level DNS query exchange.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryResultDto {
    pub query_name: String,
    pub record_type: String,
    pub rcode: String,
    pub header_rcode: u16,
    pub extended_rcode: u8,
    pub full_rcode: u16,
    pub answers: Vec<AnswerDto>,
    pub authority_nodata: bool,
    pub authority_soa: bool,
    pub ad_flag: bool,
    pub aa_flag: bool,
    pub ra_flag: bool,
    pub truncated: bool,
    pub latency_ms: u64,
    pub transport_used: String,
    pub response_bytes: usize,
    pub edns_present: bool,
    pub additional_glue: Vec<AnswerDto>,
}

/// Returns `true` when the response is an explicit NODATA indication:
/// NOERROR with an empty answer section and an SOA in the authority section.
pub fn is_nodata(result: &QueryResultDto) -> bool {
    result.rcode == "noerror" && result.answers.is_empty() && result.authority_soa
}

/// Returns `true` when the response rcode is NXDOMAIN.
pub fn is_nxdomain(result: &QueryResultDto) -> bool {
    result.rcode == "nxdomain"
}

/// Convert a hickory [`RecordType`] into our public record-type string.
pub fn record_type_name(rt: hickory_proto::rr::RecordType) -> String {
    RecordTypeSpec::from_str(&rt.to_string())
        .map_or_else(|_| rt.to_string(), |spec| spec.to_string())
}

pub(super) fn to_hickory_record_type(spec: RecordTypeSpec) -> hickory_proto::rr::RecordType {
    spec.to_hickory()
}
