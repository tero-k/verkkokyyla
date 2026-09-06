use std::path::PathBuf;

use thiserror::Error;
use tokio::sync::OnceCell;

const RANDOMIZED_VENDOR: &str = "Randomized / locally administered";
const MANUF_BYTES: &[u8] = include_bytes!("../../resources/manuf");
static OUI_DB: OnceCell<OuiDatabase> = OnceCell::const_new();

#[derive(Debug, Error)]
pub enum OuiError {
    #[error("invalid MAC address")]
    InvalidMac,
    #[error("invalid manuf prefix")]
    InvalidPrefix,
    #[error("invalid UTF-8 in bundled manuf data: {0}")]
    Utf8(#[from] std::str::Utf8Error),
}

#[derive(Debug, Clone)]
pub struct OuiDatabase {
    entries: Vec<OuiEntry>,
}

#[derive(Debug, Clone)]
struct OuiEntry {
    bits: usize,
    prefix: Vec<u8>,
    vendor: String,
}

pub async fn lookup_vendor(mac: &str) -> Result<Option<String>, OuiError> {
    let db = OUI_DB
        .get_or_try_init(|| async { OuiDatabase::parse(std::str::from_utf8(MANUF_BYTES)?) })
        .await?;
    db.lookup(mac)
}

pub fn bundled_manuf_path(resource_dir: Option<PathBuf>) -> PathBuf {
    resource_dir
        .map(|dir| dir.join("manuf"))
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("resources")
                .join("manuf")
        })
}

impl OuiDatabase {
    pub fn parse(input: &str) -> Result<Self, OuiError> {
        let mut entries = Vec::new();
        for line in input.lines() {
            if let Some(entry) = parse_line(line)? {
                entries.push(entry);
            }
        }
        entries.sort_by_key(|entry| std::cmp::Reverse(entry.bits));
        Ok(Self { entries })
    }

    pub fn lookup(&self, mac: &str) -> Result<Option<String>, OuiError> {
        let bytes = parse_mac(mac)?;
        if bytes[0] & 0x02 != 0 {
            return Ok(Some(RANDOMIZED_VENDOR.to_owned()));
        }
        Ok(self
            .entries
            .iter()
            .find(|entry| prefix_matches(&bytes, entry))
            .map(|entry| entry.vendor.clone()))
    }
}

fn parse_line(line: &str) -> Result<Option<OuiEntry>, OuiError> {
    let content = line.split('#').next().unwrap_or_default().trim();
    if content.is_empty() {
        return Ok(None);
    }
    let mut parts = content.split_whitespace();
    let Some(prefix) = parts.next() else {
        return Ok(None);
    };
    let Some(vendor) = parts.next() else {
        return Ok(None);
    };
    let (bytes, bits) = parse_prefix(prefix)?;
    Ok(Some(OuiEntry {
        bits,
        prefix: bytes,
        vendor: vendor.to_owned(),
    }))
}

fn parse_prefix(prefix: &str) -> Result<(Vec<u8>, usize), OuiError> {
    let (raw, explicit_bits) = match prefix.split_once('/') {
        Some((raw, bits)) => (
            raw,
            Some(bits.parse::<usize>().map_err(|_| OuiError::InvalidPrefix)?),
        ),
        None => (prefix, None),
    };
    let mut hex = hex_chars(raw);
    if hex.is_empty() || hex.len() % 2 != 0 {
        hex.push('0');
    }
    let bits = explicit_bits.unwrap_or_else(|| hex_chars(raw).len() * 4);
    let bytes = parse_hex_pairs(&hex).ok_or(OuiError::InvalidPrefix)?;
    Ok((bytes, bits))
}

fn parse_mac(mac: &str) -> Result<[u8; 6], OuiError> {
    let hex = hex_chars(mac);
    if hex.len() != 12 {
        return Err(OuiError::InvalidMac);
    }
    let bytes = parse_hex_pairs(&hex).ok_or(OuiError::InvalidMac)?;
    <[u8; 6]>::try_from(bytes.as_slice()).map_err(|_| OuiError::InvalidMac)
}

fn hex_chars(input: &str) -> String {
    input.chars().filter(|ch| ch.is_ascii_hexdigit()).collect()
}

fn parse_hex_pairs(hex: &str) -> Option<Vec<u8>> {
    hex.as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            std::str::from_utf8(pair)
                .ok()
                .and_then(|s| u8::from_str_radix(s, 16).ok())
        })
        .collect()
}

fn prefix_matches(mac: &[u8; 6], entry: &OuiEntry) -> bool {
    let full_bytes = entry.bits / 8;
    let partial_bits = entry.bits % 8;
    if mac.get(..full_bytes) != Some(entry.prefix.get(..full_bytes).unwrap_or(&[])) {
        return false;
    }
    if partial_bits == 0 {
        return true;
    }
    let mask = 0xFFu8 << (8 - partial_bits);
    mac.get(full_bytes)
        .zip(entry.prefix.get(full_bytes))
        .is_some_and(|(a, b)| (a & mask) == (b & mask))
}
