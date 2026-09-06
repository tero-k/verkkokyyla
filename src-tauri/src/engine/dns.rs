//! Target resolution: literal IPs (incl. scoped IPv6) short-circuit without
//! DNS; hostnames go through `tokio::net::lookup_host` with a family rule.
//!
//! Family rule: `Auto` prefers the first IPv4 answer and falls back to IPv6;
//! `V4`/`V6` select only answers of that family. ALL answers plus the
//! selected one are returned so the UI can display both.

use std::net::{IpAddr, Ipv4Addr};

use tokio::net::lookup_host;

use super::zone::parse_ipv6_with_scope;
use super::EngineError;

/// Address-family selector for resolution.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Family {
    /// Prefer the first IPv4 answer; fall back to IPv6.
    Auto,
    V4,
    V6,
}

/// Outcome of resolving one user-supplied target.
#[derive(Clone, Debug, PartialEq)]
pub struct ResolveResult {
    /// Every address answer (one element for literal inputs).
    pub answers: Vec<IpAddr>,
    /// The address the engine will probe.
    pub selected: IpAddr,
    /// IPv6 zone/scope id from a `%zone` suffix; 0 when absent.
    pub scope_id: u32,
}

/// Resolve a user-supplied target (hostname, IPv4, or IPv6 with optional
/// `%zone`) into concrete addresses according to `family`.
pub async fn resolve_target(input: &str, family: Family) -> Result<ResolveResult, EngineError> {
    // Literal IPv4 short-circuit: no DNS.
    if let Ok(v4) = input.parse::<Ipv4Addr>() {
        return match family {
            Family::Auto | Family::V4 => Ok(ResolveResult {
                answers: vec![IpAddr::V4(v4)],
                selected: IpAddr::V4(v4),
                scope_id: 0,
            }),
            Family::V6 => Err(EngineError::NoAnswer {
                input: input.to_owned(),
                family,
            }),
        };
    }
    // Literal IPv6 (optionally scoped) short-circuit: no DNS. Hostnames can
    // contain neither ':' nor '%', so anything with either is a (possibly
    // malformed) literal and never reaches the resolver.
    if input.contains(':') || input.contains('%') {
        let (v6, scope_id) = parse_ipv6_with_scope(input).map_err(EngineError::Parse)?;
        return match family {
            Family::Auto | Family::V6 => Ok(ResolveResult {
                answers: vec![IpAddr::V6(v6)],
                selected: IpAddr::V6(v6),
                scope_id,
            }),
            Family::V4 => Err(EngineError::NoAnswer {
                input: input.to_owned(),
                family,
            }),
        };
    }
    // DNS path.
    let answers: Vec<IpAddr> = lookup_host((input, 0))
        .await
        .map_err(|source| EngineError::Resolve {
            input: input.to_owned(),
            source,
        })?
        .map(|sock_addr| sock_addr.ip())
        .collect();
    let selected = select(&answers, family).ok_or_else(|| EngineError::NoAnswer {
        input: input.to_owned(),
        family,
    })?;
    Ok(ResolveResult {
        answers,
        selected,
        scope_id: 0,
    })
}

/// Family selection over the answer set. Auto prefers IPv4, falls back to v6.
fn select(answers: &[IpAddr], family: Family) -> Option<IpAddr> {
    let first_v4 = answers.iter().find(|a| a.is_ipv4()).copied();
    let first_v6 = answers.iter().find(|a| a.is_ipv6()).copied();
    match family {
        Family::Auto => first_v4.or(first_v6),
        Family::V4 => first_v4,
        Family::V6 => first_v6,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, Ipv6Addr};

    // Given a literal IPv4 input,
    // When resolved under Auto/V4,
    // Then DNS is never consulted and the address is the only answer.
    #[tokio::test]
    async fn literal_ipv4_short_circuits_dns() {
        let v4 = IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1));
        for family in [Family::Auto, Family::V4] {
            let result = resolve_target("192.0.2.1", family)
                .await
                .expect("literal resolves");
            assert_eq!(result.answers, vec![v4]);
            assert_eq!(result.selected, v4);
            assert_eq!(result.scope_id, 0);
        }
    }

    // Given a literal IPv6 input (plain and scoped),
    // When resolved under Auto/V6,
    // Then the address (and scope) are returned without DNS.
    #[tokio::test]
    async fn literal_ipv6_with_and_without_scope_short_circuits_dns() {
        let v6 = IpAddr::V6(Ipv6Addr::LOCALHOST);
        let plain = resolve_target("::1", Family::Auto).await.expect("literal");
        assert_eq!(plain.answers, vec![v6]);
        assert_eq!(plain.selected, v6);
        assert_eq!(plain.scope_id, 0);

        let scoped = resolve_target("fe80::1%3", Family::V6)
            .await
            .expect("literal");
        assert_eq!(scoped.selected, IpAddr::V6("fe80::1".parse().expect("lit")));
        assert_eq!(scoped.scope_id, 3);
    }

    // Given a literal whose family mismatches the selector,
    // When resolved,
    // Then NoAnswer is returned (no silent cross-family fallback).
    #[tokio::test]
    async fn literal_family_mismatch_is_an_error() {
        assert!(matches!(
            resolve_target("192.0.2.1", Family::V6).await,
            Err(EngineError::NoAnswer {
                family: Family::V6,
                ..
            })
        ));
        assert!(matches!(
            resolve_target("::1", Family::V4).await,
            Err(EngineError::NoAnswer {
                family: Family::V4,
                ..
            })
        ));
    }

    // Given "localhost" (resolves via the hosts file, no network),
    // When resolved under Auto,
    // Then answers are non-empty, the selected address is one of them, and
    // IPv4 is selected when present.
    #[tokio::test]
    async fn localhost_resolves_and_auto_prefers_ipv4() {
        let result = resolve_target("localhost", Family::Auto)
            .await
            .expect("localhost resolves via hosts file");
        assert!(!result.answers.is_empty());
        assert!(result.answers.contains(&result.selected));
        if result.answers.iter().any(IpAddr::is_ipv4) {
            assert!(result.selected.is_ipv4(), "Auto must prefer IPv4");
        }
    }

    // Given a hostname with a space (invalid),
    // When resolved,
    // Then the lookup fails with a typed Resolve error (no panic).
    #[tokio::test]
    async fn invalid_hostname_fails_lookup() {
        assert!(matches!(
            resolve_target("exa mple.com", Family::Auto).await,
            Err(EngineError::Resolve { .. })
        ));
    }
}
