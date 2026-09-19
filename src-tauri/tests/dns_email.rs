mod common;

use common::mock_dns::start_mock;
use verkkokyyla_lib::dns::client::{DnsProtocol, ResolverEndpointDto};
use verkkokyyla_lib::dns::email::{email_security_report, EmailSecurityVerdict};

fn endpoint(protocol: DnsProtocol, address: &str) -> ResolverEndpointDto {
    ResolverEndpointDto {
        name: "test".into(),
        protocol,
        address: address.into(),
    }
}

#[tokio::test]
async fn email_report_flags_good_configuration_as_pass() {
    let (handle, _udp, tcp, _tls, _cert) = start_mock().await;

    let report = email_security_report(
        &endpoint(DnsProtocol::Tcp, &tcp.to_string()),
        "email.mock.test.",
        &["default".to_string()],
    )
    .await
    .expect("email report should succeed");

    assert_eq!(report.domain, "email.mock.test");
    assert_eq!(report.spf.verdict, EmailSecurityVerdict::Pass);
    assert_eq!(report.spf.all_mechanism.as_deref(), Some("-all"));
    assert!(report.spf.record.is_some());
    assert!(report.spf.lookup_limit_ok);

    assert_eq!(report.dmarc.verdict, EmailSecurityVerdict::Pass);
    assert_eq!(report.dmarc.policy.as_deref(), Some("reject"));
    assert_eq!(report.dmarc.pct, Some(100));

    let default = report
        .dkim
        .into_iter()
        .find(|d| d.selector == "default")
        .expect("default selector present");
    assert_eq!(default.verdict, EmailSecurityVerdict::Pass);
    assert!(default.found);
    assert!(default.key_present);
    assert!(!default.revoked);
    assert!(
        default.key_bits_approx.unwrap_or(0) >= 2048,
        "expected 2048-bit-or-larger key approx, got {:?}",
        default.key_bits_approx
    );

    drop(handle);
}

#[tokio::test]
async fn email_report_flags_revoked_dkim_as_fail() {
    let (handle, _udp, tcp, _tls, _cert) = start_mock().await;

    let report = email_security_report(
        &endpoint(DnsProtocol::Tcp, &tcp.to_string()),
        "email.mock.test.",
        &["revoked".to_string()],
    )
    .await
    .expect("email report should succeed");

    let revoked = report
        .dkim
        .into_iter()
        .find(|d| d.selector == "revoked")
        .expect("revoked selector present");
    assert!(revoked.revoked);
    assert!(!revoked.key_present);
    assert_eq!(revoked.verdict, EmailSecurityVerdict::Fail);

    drop(handle);
}

#[tokio::test]
async fn email_report_for_missing_domain_fails_dmarc_and_dkim() {
    let (handle, _udp, tcp, _tls, _cert) = start_mock().await;

    let report = email_security_report(
        &endpoint(DnsProtocol::Tcp, &tcp.to_string()),
        "noemail.mock.test.",
        &["default".to_string()],
    )
    .await
    .expect("email report should still return report");

    assert_eq!(report.spf.verdict, EmailSecurityVerdict::Fail);
    assert_eq!(report.dmarc.verdict, EmailSecurityVerdict::Fail);
    assert!(
        report
            .dkim
            .iter()
            .all(|d| d.verdict == EmailSecurityVerdict::Fail),
        "missing DKIM records should fail"
    );

    drop(handle);
}
