//! IPv6 zone-id (scope) parsing: `fe80::1%3` -> (`Ipv6Addr`, scope_id `3`).
//!
//! Rust's `IpAddr`/`Ipv6Addr` carry no scope, so the pair is threaded
//! separately to the engines. Numeric zones (`%<index>`) work on every
//! platform; named zones (`%eth0`) resolve via `if_nametoindex` on POSIX only.

use std::fmt;
use std::net::Ipv6Addr;

/// Failure modes of [`parse_ipv6_with_scope`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ParseError {
    /// The address part is not a valid IPv6 address.
    InvalidAddress(String),
    /// The zone part is neither a valid `u32` index nor a resolvable name.
    InvalidZoneIndex(String),
    /// A named zone was given on a platform without `if_nametoindex`.
    NamedZoneUnsupported(String),
    /// POSIX `if_nametoindex` returned 0 (no such interface).
    #[cfg(unix)]
    UnknownInterface(String),
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseError::InvalidAddress(input) => {
                write!(f, "invalid IPv6 address: {input:?}")
            }
            ParseError::InvalidZoneIndex(zone) => {
                write!(f, "invalid zone index: {zone:?}")
            }
            ParseError::NamedZoneUnsupported(zone) => {
                write!(f, "named zone {zone:?} is only supported on POSIX")
            }
            #[cfg(unix)]
            ParseError::UnknownInterface(name) => {
                write!(f, "unknown interface: {name:?}")
            }
        }
    }
}

impl std::error::Error for ParseError {}

/// Parse an IPv6 address with an optional `%zone` suffix.
///
/// Plain addresses without `%` yield scope 0. `%<digits>` yields the numeric
/// interface index on all platforms. `%<name>` resolves via
/// `if_nametoindex` on POSIX and is an error elsewhere.
pub fn parse_ipv6_with_scope(input: &str) -> Result<(Ipv6Addr, u32), ParseError> {
    parse_impl(input, name_to_index)
}

/// Parsing core with the name->index lookup injected (test seam).
fn parse_impl(
    input: &str,
    resolve_name: impl Fn(&str) -> Result<u32, ParseError>,
) -> Result<(Ipv6Addr, u32), ParseError> {
    let (addr_str, zone) = match input.split_once('%') {
        Some((a, z)) => (a, Some(z)),
        None => (input, None),
    };
    let addr: Ipv6Addr = addr_str
        .parse()
        .map_err(|_| ParseError::InvalidAddress(input.to_owned()))?;
    match zone {
        None => Ok((addr, 0)),
        Some(z) if !z.is_empty() && z.bytes().all(|b| b.is_ascii_digit()) => {
            let index = z
                .parse::<u32>()
                .map_err(|_| ParseError::InvalidZoneIndex(z.to_owned()))?;
            Ok((addr, index))
        }
        Some("") => Err(ParseError::InvalidZoneIndex(String::new())),
        Some(z) => Ok((addr, resolve_name(z)?)),
    }
}

/// POSIX: resolve an interface name to its index via `if_nametoindex`.
#[cfg(unix)]
fn name_to_index(name: &str) -> Result<u32, ParseError> {
    use std::ffi::CString;
    let c_name =
        CString::new(name).map_err(|_| ParseError::UnknownInterface(name.to_owned()))?;
    // SAFETY: `c_name` is a valid NUL-terminated string that outlives the call.
    let index = unsafe { libc::if_nametoindex(c_name.as_ptr()) };
    if index == 0 {
        Err(ParseError::UnknownInterface(name.to_owned()))
    } else {
        Ok(index)
    }
}

/// Non-POSIX: named zones are unsupported (Windows uses numeric indices).
#[cfg(not(unix))]
fn name_to_index(name: &str) -> Result<u32, ParseError> {
    Err(ParseError::NamedZoneUnsupported(name.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fe80_1() -> Ipv6Addr {
        "fe80::1".parse().expect("literal")
    }

    // Given a plain IPv6 literal without a zone,
    // When parsed,
    // Then the scope id is 0.
    #[test]
    fn plain_ipv6_yields_scope_zero() {
        assert_eq!(parse_ipv6_with_scope("fe80::1"), Ok((fe80_1(), 0)));
        assert_eq!(
            parse_ipv6_with_scope("::1"),
            Ok((Ipv6Addr::LOCALHOST, 0))
        );
    }

    // Given a numeric zone suffix,
    // When parsed,
    // Then the scope id is the parsed index.
    #[test]
    fn numeric_zone_yields_index() {
        assert_eq!(parse_ipv6_with_scope("fe80::1%3"), Ok((fe80_1(), 3)));
        assert_eq!(
            parse_ipv6_with_scope("fe80::1%4294967295"),
            Ok((fe80_1(), u32::MAX))
        );
    }

    // Given malformed inputs,
    // When parsed,
    // Then each fails with the specific error variant.
    #[test]
    fn malformed_inputs_fail_with_specific_errors() {
        assert_eq!(
            parse_ipv6_with_scope("not-an-address"),
            Err(ParseError::InvalidAddress("not-an-address".to_owned()))
        );
        assert_eq!(
            parse_ipv6_with_scope("fe80::1%"),
            Err(ParseError::InvalidZoneIndex(String::new()))
        );
        // Index overflowing u32.
        assert_eq!(
            parse_ipv6_with_scope("fe80::1%4294967296"),
            Err(ParseError::InvalidZoneIndex("4294967296".to_owned()))
        );
        // A zone on a non-IPv6 address part.
        assert!(matches!(
            parse_ipv6_with_scope("1.2.3.4%3"),
            Err(ParseError::InvalidAddress(_))
        ));
    }

    // Given a named zone (POSIX path),
    // When parsed through the injected seam,
    // Then the resolver's index is returned, and resolver errors propagate.
    #[cfg(unix)]
    #[test]
    fn named_zone_resolves_via_if_nametoindex_seam() {
        let seam = |name: &str| {
            if name == "eth0" {
                Ok(7)
            } else {
                Err(ParseError::UnknownInterface(name.to_owned()))
            }
        };
        assert_eq!(parse_impl("fe80::1%eth0", seam), Ok((fe80_1(), 7)));
        assert_eq!(
            parse_impl("fe80::1%wlan9", seam),
            Err(ParseError::UnknownInterface("wlan9".to_owned()))
        );
    }

    // Given a named zone that does not exist,
    // When parsed through the real if_nametoindex,
    // Then it fails with UnknownInterface.
    #[cfg(unix)]
    #[test]
    fn unknown_named_zone_errors_via_real_if_nametoindex() {
        assert_eq!(
            parse_ipv6_with_scope("fe80::1%definitely_not_an_iface_zzz"),
            Err(ParseError::UnknownInterface(
                "definitely_not_an_iface_zzz".to_owned()
            ))
        );
    }

    // Given a named zone on Windows,
    // When parsed,
    // Then it fails with NamedZoneUnsupported (names are a POSIX feature).
    #[cfg(not(unix))]
    #[test]
    fn named_zone_unsupported_off_posix() {
        assert_eq!(
            parse_ipv6_with_scope("fe80::1%eth0"),
            Err(ParseError::NamedZoneUnsupported("eth0".to_owned()))
        );
    }
}
