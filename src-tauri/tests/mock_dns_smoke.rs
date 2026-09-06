mod common;

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use common::mock_dns::{start_mock, start_mock_variant, AnswersOverride, FIXED_SERIAL};
use hickory_server::proto::op::{Message, Query, ResponseCode};
use hickory_server::proto::rr::{Name, RData, RecordType};
use hickory_server::proto::serialize::binary::{BinEncodable, BinEncoder};
use rustls::pki_types::{CertificateDer, ServerName};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpStream, UdpSocket};
use tokio_rustls::TlsConnector;

#[tokio::test]
async fn mock_dns_exercises_front_loaded_behaviors() {
    let (handle, udp, tcp, tls, cert_der) = start_mock().await;

    let a = udp_query(udp, "www.mock.test.", RecordType::A, Duration::from_secs(1))
        .await
        .expect("udp a");
    assert_eq!(a.response_code, ResponseCode::NoError);
    assert!(a
        .answers
        .iter()
        .any(|record| matches!(record.data, RData::A(_))));

    let aaaa = udp_query(
        udp,
        "www.mock.test.",
        RecordType::AAAA,
        Duration::from_secs(1),
    )
    .await
    .expect("udp aaaa");
    assert!(aaaa
        .answers
        .iter()
        .any(|record| matches!(record.data, RData::AAAA(_))));

    assert_has_answer(udp, "mock.test.", RecordType::MX).await;
    assert_has_answer(udp, "mock.test.", RecordType::TXT).await;
    assert_has_answer(udp, "mock.test.", RecordType::NS).await;

    let soa = udp_query(udp, "mock.test.", RecordType::SOA, Duration::from_secs(1))
        .await
        .expect("soa");
    assert!(soa.answers.iter().any(|record| match &record.data {
        RData::SOA(soa) => soa.serial == FIXED_SERIAL,
        _ => false,
    }));

    let cname = udp_query(
        udp,
        "alias.mock.test.",
        RecordType::CNAME,
        Duration::from_secs(1),
    )
    .await
    .expect("cname");
    assert!(cname
        .answers
        .iter()
        .any(|record| matches!(record.data, RData::CNAME(_))));

    let loop_one = udp_query(
        udp,
        "loop1.mock.test.",
        RecordType::CNAME,
        Duration::from_secs(1),
    )
    .await
    .expect("loop1");
    let loop_two = udp_query(
        udp,
        "loop2.mock.test.",
        RecordType::CNAME,
        Duration::from_secs(1),
    )
    .await
    .expect("loop2");
    assert!(loop_one
        .answers
        .iter()
        .any(|record| record.to_string().contains("loop2.mock.test")));
    assert!(loop_two
        .answers
        .iter()
        .any(|record| record.to_string().contains("loop1.mock.test")));

    assert_has_answer(udp, "anything.wild.mock.test.", RecordType::A).await;

    let delegation = udp_query(
        udp,
        "host.sub.mock.test.",
        RecordType::A,
        Duration::from_secs(1),
    )
    .await
    .expect("delegation");
    assert!(delegation
        .authorities
        .iter()
        .any(|record| matches!(record.data, RData::NS(_))));
    assert!(delegation
        .additionals
        .iter()
        .any(|record| matches!(record.data, RData::A(_))));

    let ptr = udp_query(
        udp,
        "10.2.0.192.in-addr.arpa.",
        RecordType::PTR,
        Duration::from_secs(1),
    )
    .await
    .expect("ptr");
    assert!(ptr
        .answers
        .iter()
        .any(|record| matches!(record.data, RData::PTR(_))));
    let broken_forward = udp_query(
        udp,
        "broken-ptr.mock.test.",
        RecordType::A,
        Duration::from_secs(1),
    )
    .await
    .expect("broken forward");
    assert!(!broken_forward
        .answers
        .iter()
        .any(|record| record.to_string().contains("192.0.2.10")));

    let big_udp = udp_query(
        udp,
        "big.mock.test.",
        RecordType::TXT,
        Duration::from_secs(1),
    )
    .await
    .expect("big udp");
    assert!(big_udp.truncation);
    let big_tcp = tcp_query(
        tcp,
        "big.mock.test.",
        RecordType::TXT,
        Duration::from_secs(1),
    )
    .await
    .expect("big tcp");
    assert!(!big_tcp.truncation);
    assert!(big_tcp.answers.len() >= 1);

    let fail = udp_query(
        udp,
        "fail.mock.test.",
        RecordType::A,
        Duration::from_secs(1),
    )
    .await
    .expect("servfail");
    assert_eq!(fail.response_code, ResponseCode::ServFail);

    let slow = udp_query(
        udp,
        "slow.mock.test.",
        RecordType::A,
        Duration::from_millis(100),
    )
    .await;
    assert!(slow.is_err());

    let absent = udp_query(
        udp,
        "absent.mock.test.",
        RecordType::A,
        Duration::from_secs(1),
    )
    .await
    .expect("nxdomain");
    assert_eq!(absent.response_code, ResponseCode::NXDomain);

    let nodata = udp_query(
        udp,
        "www.mock.test.",
        RecordType::MX,
        Duration::from_secs(1),
    )
    .await
    .expect("nodata");
    assert_eq!(nodata.response_code, ResponseCode::NoError);
    assert!(nodata.answers.is_empty());

    let axfr = tcp_query(tcp, "mock.test.", RecordType::AXFR, Duration::from_secs(1))
        .await
        .expect("axfr");
    assert!(
        axfr.answers.len() > 10,
        "axfr rcode {:?} answers {} authorities {} additionals {}",
        axfr.response_code,
        axfr.answers.len(),
        axfr.authorities.len(),
        axfr.additionals.len()
    );

    let dot = tls_query(
        tls,
        &cert_der,
        "www.mock.test.",
        RecordType::A,
        Duration::from_secs(2),
    )
    .await
    .expect("dot");
    assert_eq!(dot.response_code, ResponseCode::NoError);

    let misses = dropped_queries(udp).await;
    assert!(
        (5..=15).contains(&misses),
        "expected 5-15 drops, got {misses}"
    );

    let before = handle.query_count();
    let _ = udp_query(udp, "www.mock.test.", RecordType::A, Duration::from_secs(1)).await;
    assert!(handle.query_count() > before);

    let (_variant, variant_udp, _, _, _) = start_mock_variant(
        FIXED_SERIAL + 99,
        AnswersOverride {
            www_a: Some("198.51.100.77".parse().expect("variant ip")),
        },
    )
    .await;
    let variant = udp_query(
        variant_udp,
        "www.mock.test.",
        RecordType::A,
        Duration::from_secs(1),
    )
    .await
    .expect("variant a");
    assert!(variant
        .answers
        .iter()
        .any(|record| record.to_string().contains("198.51.100.77")));
}

#[tokio::test]
async fn mock_dns_closed_port_times_out_quickly() {
    let socket = UdpSocket::bind("127.0.0.1:0")
        .await
        .expect("reserve udp port");
    let closed = socket.local_addr().expect("reserved addr");
    drop(socket);
    let (_handle, _, _, _, _) = start_mock().await;
    let result = udp_query(
        closed,
        "www.mock.test.",
        RecordType::A,
        Duration::from_secs(3),
    )
    .await;
    assert!(result.is_err());
}

async fn assert_has_answer(addr: SocketAddr, name: &str, record_type: RecordType) {
    let response = udp_query(addr, name, record_type, Duration::from_secs(1))
        .await
        .expect("query");
    assert_eq!(response.response_code, ResponseCode::NoError);
    assert!(!response.answers.is_empty());
}

async fn dropped_queries(addr: SocketAddr) -> usize {
    let mut misses = 0;
    for _ in 0..100 {
        if udp_query(
            addr,
            "drop10.mock.test.",
            RecordType::A,
            Duration::from_millis(100),
        )
        .await
        .is_err()
        {
            misses += 1;
        }
    }
    misses
}

async fn udp_query(
    addr: SocketAddr,
    name: &str,
    record_type: RecordType,
    timeout: Duration,
) -> Result<Message, Box<dyn std::error::Error + Send + Sync>> {
    let socket = UdpSocket::bind("127.0.0.1:0").await?;
    let request = request_bytes(name, record_type)?;
    socket.send_to(&request, addr).await?;
    let mut buf = [0u8; 4096];
    let (len, _) = tokio::time::timeout(timeout, socket.recv_from(&mut buf)).await??;
    Ok(Message::from_vec(&buf[..len])?)
}

async fn tcp_query(
    addr: SocketAddr,
    name: &str,
    record_type: RecordType,
    timeout: Duration,
) -> Result<Message, Box<dyn std::error::Error + Send + Sync>> {
    let stream = tokio::time::timeout(timeout, TcpStream::connect(addr)).await??;
    framed_query(stream, name, record_type, timeout).await
}

async fn tls_query(
    addr: SocketAddr,
    cert_der: &[u8],
    name: &str,
    record_type: RecordType,
    timeout: Duration,
) -> Result<Message, Box<dyn std::error::Error + Send + Sync>> {
    let mut roots = rustls::RootCertStore::empty();
    roots.add(CertificateDer::from(cert_der.to_vec()))?;
    let config = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()?
    .with_root_certificates(roots)
    .with_no_client_auth();
    let connector = TlsConnector::from(Arc::new(config));
    let stream = tokio::time::timeout(timeout, TcpStream::connect(addr)).await??;
    let server_name = ServerName::try_from("localhost")?;
    let tls = tokio::time::timeout(timeout, connector.connect(server_name, stream)).await??;
    framed_query(tls, name, record_type, timeout).await
}

async fn framed_query<S>(
    mut stream: S,
    name: &str,
    record_type: RecordType,
    timeout: Duration,
) -> Result<Message, Box<dyn std::error::Error + Send + Sync>>
where
    S: AsyncReadExt + AsyncWriteExt + Unpin,
{
    let request = request_bytes(name, record_type)?;
    stream
        .write_all(&(request.len() as u16).to_be_bytes())
        .await?;
    stream.write_all(&request).await?;
    let mut len = [0u8; 2];
    tokio::time::timeout(timeout, stream.read_exact(&mut len)).await??;
    let mut response = vec![0u8; u16::from_be_bytes(len) as usize];
    tokio::time::timeout(timeout, stream.read_exact(&mut response)).await??;
    Ok(Message::from_vec(&response)?)
}

fn request_bytes(
    name: &str,
    record_type: RecordType,
) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
    let mut message = Message::query();
    message.add_query(Query::query(Name::parse(name, None)?, record_type));
    let mut bytes = Vec::with_capacity(512);
    let mut encoder = BinEncoder::new(&mut bytes);
    message.emit(&mut encoder)?;
    Ok(bytes)
}
