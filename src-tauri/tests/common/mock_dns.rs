use std::collections::BTreeMap;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use base64::{engine::general_purpose::STANDARD, Engine as _};
use hickory_server::net::runtime::TokioRuntimeProvider;
use hickory_server::proto::op::{Header, HeaderCounts, Metadata, ResponseCode};
use hickory_server::proto::rr::rdata::{NS, PTR, TXT};
use hickory_server::proto::rr::{LowerName, Name, RData, Record, RecordSet, RrKey};
use hickory_server::proto::serialize::txt::Parser;
use hickory_server::server::{Request, RequestHandler, ResponseHandler, ResponseInfo, Server};
use hickory_server::store::in_memory::InMemoryZoneHandler;
use hickory_server::zone_handler::{
    AxfrPolicy, Catalog, MessageResponseBuilder, ZoneHandler, ZoneType,
};
use rcgen::generate_simple_self_signed;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use rustls::ServerConfig;
use tokio::net::{TcpListener, UdpSocket};

pub type UdpAddr = SocketAddr;
pub type TcpAddr = SocketAddr;
pub type TlsAddr = SocketAddr;
pub type CertDer = Vec<u8>;

pub const FIXED_SERIAL: u32 = 2_026_082_203;

#[derive(Clone, Default)]
pub struct AnswersOverride {
    pub www_a: Option<Ipv4Addr>,
}

pub struct MockHandle {
    join: Option<tokio::task::JoinHandle<()>>,
    counter: Arc<AtomicUsize>,
}

impl MockHandle {
    pub fn query_count(&self) -> usize {
        self.counter.load(Ordering::SeqCst)
    }
}

impl Drop for MockHandle {
    fn drop(&mut self) {
        if let Some(join) = self.join.take() {
            join.abort();
        }
    }
}

pub async fn start_mock() -> (MockHandle, UdpAddr, TcpAddr, TlsAddr, CertDer) {
    start_mock_variant(FIXED_SERIAL, AnswersOverride::default()).await
}

pub async fn start_mock_variant(
    serial: u32,
    answers_override: AnswersOverride,
) -> (MockHandle, UdpAddr, TcpAddr, TlsAddr, CertDer) {
    let udp = UdpSocket::bind("127.0.0.1:0").await.expect("bind mock udp");
    let tcp = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock tcp");
    let tls = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock tls");
    let udp_addr = udp.local_addr().expect("udp addr");
    let tcp_addr = tcp.local_addr().expect("tcp addr");
    let tls_addr = tls.local_addr().expect("tls addr");
    let (tls_config, cert_der) = tls_config();

    let handler = MockHandler::new(serial, answers_override);
    let counter = Arc::clone(&handler.counter);
    let mut server = Server::new(handler);
    server.register_socket(udp);
    server.register_listener(tcp, Duration::from_secs(2), 65_535);
    server
        .register_tls_listener_with_tls_config(tls, Duration::from_secs(2), Arc::new(tls_config))
        .expect("register mock tls");

    let join = tokio::spawn(async move {
        let _ = server.block_until_done().await;
    });

    let handle = MockHandle {
        join: Some(join),
        counter,
    };
    (handle, udp_addr, tcp_addr, tls_addr, cert_der)
}

struct MockHandler {
    catalog: Catalog,
    counter: Arc<AtomicUsize>,
    drop_counter: AtomicUsize,
    axfr_records: Vec<Record>,
}

impl MockHandler {
    fn new(serial: u32, answers_override: AnswersOverride) -> Self {
        let origin = name("mock.test.");
        let records = zone_records(serial, answers_override);
        let axfr_records = records
            .values()
            .flat_map(|set| set.records_without_rrsigs().cloned())
            .collect::<Vec<_>>();
        let zone = InMemoryZoneHandler::<TokioRuntimeProvider>::new(
            origin.clone(),
            records,
            ZoneType::Primary,
            AxfrPolicy::AllowAll,
        )
        .expect("mock zone");
        let mut catalog = Catalog::new();
        catalog.upsert(
            LowerName::new(&origin),
            vec![Arc::new(zone) as Arc<dyn ZoneHandler>],
        );
        Self {
            catalog,
            counter: Arc::new(AtomicUsize::new(0)),
            drop_counter: AtomicUsize::new(0),
            axfr_records,
        }
    }
}

#[async_trait::async_trait]
impl RequestHandler for MockHandler {
    async fn handle_request<R: ResponseHandler, T: hickory_server::net::runtime::Time>(
        &self,
        request: &Request,
        response_handle: R,
    ) -> ResponseInfo {
        self.counter.fetch_add(1, Ordering::SeqCst);
        let Ok(info) = request.request_info() else {
            return self
                .catalog
                .handle_request::<R, T>(request, response_handle)
                .await;
        };
        let qname = info.query.name().to_string();
        if info.query.query_type() == hickory_server::proto::rr::RecordType::AXFR {
            return axfr_response(request, response_handle, &self.axfr_records).await;
        }
        match qname.as_str() {
            "fail.mock.test." => {
                error_response(request, response_handle, ResponseCode::ServFail).await
            }
            "slow.mock.test." => {
                tokio::time::sleep(Duration::from_millis(750)).await;
                self.catalog
                    .handle_request::<R, T>(request, response_handle)
                    .await
            }
            "drop10.mock.test." => {
                if self.drop_counter.fetch_add(1, Ordering::SeqCst) % 10 == 0 {
                    no_response(request)
                } else {
                    self.catalog
                        .handle_request::<R, T>(request, response_handle)
                        .await
                }
            }
            "10.2.0.192.in-addr.arpa." => ptr_response(request, response_handle).await,
            "sub.mock.test."
                if info.query.query_type() == hickory_server::proto::rr::RecordType::NS =>
            {
                sub_ns_response(request, response_handle).await
            }
            "host.sub.mock.test." => delegation_response(request, response_handle).await,
            "big.mock.test." if request.protocol() == hickory_server::net::xfer::Protocol::Udp => {
                truncated_response(request, response_handle).await
            }
            "big.mock.test." => big_response(request, response_handle).await,
            _ => {
                self.catalog
                    .handle_request::<R, T>(request, response_handle)
                    .await
            }
        }
    }
}

async fn axfr_response<R: ResponseHandler>(
    request: &Request,
    response_handle: R,
    records: &[Record],
) -> ResponseInfo {
    let mut metadata = Metadata::response_from_request(&request.metadata);
    metadata.authoritative = true;
    let response = MessageResponseBuilder::from_message_request(request).build(
        metadata,
        records.iter(),
        std::iter::empty::<&Record>(),
        std::iter::empty::<&Record>(),
        std::iter::empty::<&Record>(),
    );
    send_or_servfail(request, response_handle, response).await
}

async fn big_response<R: ResponseHandler>(request: &Request, response_handle: R) -> ResponseInfo {
    let chunks = (0..12).map(|_| "x".repeat(200)).collect::<Vec<_>>();
    let txt = Record::from_rdata(name("big.mock.test."), 60, RData::TXT(TXT::new(chunks)));
    let mut metadata = Metadata::response_from_request(&request.metadata);
    metadata.authoritative = true;
    let response = MessageResponseBuilder::from_message_request(request).build(
        metadata,
        std::iter::once(&txt),
        std::iter::empty::<&Record>(),
        std::iter::empty::<&Record>(),
        std::iter::empty::<&Record>(),
    );
    send_or_servfail(request, response_handle, response).await
}

async fn ptr_response<R: ResponseHandler>(request: &Request, response_handle: R) -> ResponseInfo {
    let ptr = Record::from_rdata(
        name("10.2.0.192.in-addr.arpa."),
        60,
        RData::PTR(PTR(name("broken-ptr.mock.test."))),
    );
    let mut metadata = Metadata::response_from_request(&request.metadata);
    metadata.authoritative = true;
    let response = MessageResponseBuilder::from_message_request(request).build(
        metadata,
        std::iter::once(&ptr),
        std::iter::empty::<&Record>(),
        std::iter::empty::<&Record>(),
        std::iter::empty::<&Record>(),
    );
    send_or_servfail(request, response_handle, response).await
}

async fn sub_ns_response<R: ResponseHandler>(
    request: &Request,
    response_handle: R,
) -> ResponseInfo {
    let ns = Record::from_rdata(
        name("sub.mock.test."),
        60,
        RData::NS(NS(name("ns.sub.mock.test."))),
    );
    let glue = Record::from_rdata(
        name("ns.sub.mock.test."),
        60,
        RData::A(Ipv4Addr::new(192, 0, 2, 60).into()),
    );
    let mut metadata = Metadata::response_from_request(&request.metadata);
    metadata.authoritative = true;
    let response = MessageResponseBuilder::from_message_request(request).build(
        metadata,
        std::iter::once(&ns),
        std::iter::empty::<&Record>(),
        std::iter::empty::<&Record>(),
        std::iter::once(&glue),
    );
    send_or_servfail(request, response_handle, response).await
}

async fn delegation_response<R: ResponseHandler>(
    request: &Request,
    response_handle: R,
) -> ResponseInfo {
    let ns = Record::from_rdata(
        name("sub.mock.test."),
        60,
        RData::NS(NS(name("ns.sub.mock.test."))),
    );
    let glue = Record::from_rdata(
        name("ns.sub.mock.test."),
        60,
        RData::A(Ipv4Addr::new(192, 0, 2, 60).into()),
    );
    let mut metadata = Metadata::response_from_request(&request.metadata);
    metadata.authoritative = true;
    let response = MessageResponseBuilder::from_message_request(request).build(
        metadata,
        std::iter::empty::<&Record>(),
        std::iter::once(&ns),
        std::iter::empty::<&Record>(),
        std::iter::once(&glue),
    );
    send_or_servfail(request, response_handle, response).await
}

async fn error_response<R: ResponseHandler>(
    request: &Request,
    response_handle: R,
    code: ResponseCode,
) -> ResponseInfo {
    let response =
        MessageResponseBuilder::from_message_request(request).error_msg(&request.metadata, code);
    send_or_servfail(request, response_handle, response).await
}

async fn truncated_response<R: ResponseHandler>(
    request: &Request,
    response_handle: R,
) -> ResponseInfo {
    let mut metadata = Metadata::response_from_request(&request.metadata);
    metadata.authoritative = true;
    metadata.truncation = true;
    let response = MessageResponseBuilder::from_message_request(request).build_no_records(metadata);
    send_or_servfail(request, response_handle, response).await
}

async fn send_or_servfail<'q, 'a, R, A, N, S, D>(
    request: &Request,
    mut response_handle: R,
    response: hickory_server::zone_handler::MessageResponse<'q, 'a, A, N, S, D>,
) -> ResponseInfo
where
    R: ResponseHandler,
    A: Iterator<Item = &'a Record> + Send + 'a,
    N: Iterator<Item = &'a Record> + Send + 'a,
    S: Iterator<Item = &'a Record> + Send + 'a,
    D: Iterator<Item = &'a Record> + Send + 'a,
{
    match response_handle.send_response(response).await {
        Ok(info) => info,
        Err(_) => no_response(request),
    }
}

fn no_response(request: &Request) -> ResponseInfo {
    let mut metadata = Metadata::response_from_request(&request.metadata);
    metadata.response_code = ResponseCode::ServFail;
    ResponseInfo::from(Header {
        metadata,
        counts: HeaderCounts::default(),
    })
}

fn dkim_key_zone_value() -> String {
    let der = build_rsa_spki_2048();
    let b64 = STANDARD.encode(&der);
    b64.chars()
        .collect::<Vec<_>>()
        .chunks(255)
        .map(|c| format!("\"{}\"", c.iter().collect::<String>()))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Build a minimal, valid RSA SubjectPublicKeyInfo DER with a 2048-bit modulus.
fn build_rsa_spki_2048() -> Vec<u8> {
    // RSA encryption OID 1.2.840.113549.1.1.1 + NULL parameters
    let algorithm_identifier = vec![
        0x30, 0x0d, 0x06, 0x09, 0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x01, 0x05,
        0x00,
    ];

    // Modulus: 256 bytes with high bit set, prefixed with 0x00 to keep integer positive.
    let modulus = std::iter::once(0x00)
        .chain(std::iter::repeat_n(0xab, 256))
        .collect::<Vec<_>>();
    let modulus_int = wrap_integer(&modulus);

    // Public exponent 65537.
    let exponent = wrap_integer(&[0x01, 0x00, 0x01]);

    let rsa_public_key = wrap_sequence([modulus_int, exponent].concat());
    let subject_public_key = wrap_bit_string(&rsa_public_key);

    wrap_sequence([algorithm_identifier, subject_public_key].concat())
}

fn wrap_sequence(content: Vec<u8>) -> Vec<u8> {
    let mut out = vec![0x30];
    out.extend(der_length(content.len()));
    out.extend(content);
    out
}

fn wrap_integer(content: &[u8]) -> Vec<u8> {
    let mut out = vec![0x02];
    out.extend(der_length(content.len()));
    out.extend(content);
    out
}

fn wrap_bit_string(content: &[u8]) -> Vec<u8> {
    let mut out = vec![0x03];
    out.extend(der_length(content.len() + 1));
    out.push(0x00); // unused bits
    out.extend(content);
    out
}

fn der_length(len: usize) -> Vec<u8> {
    if len < 0x80 {
        vec![len as u8]
    } else if len <= 0xff {
        vec![0x81, len as u8]
    } else if len <= 0xffff {
        vec![0x82, (len >> 8) as u8, len as u8]
    } else if len <= 0xffffff {
        vec![0x83, (len >> 16) as u8, (len >> 8) as u8, len as u8]
    } else {
        vec![0x84, (len >> 24) as u8, (len >> 16) as u8, (len >> 8) as u8, len as u8]
    }
}

fn zone_records(serial: u32, answers_override: AnswersOverride) -> BTreeMap<RrKey, RecordSet> {
    let www = answers_override
        .www_a
        .unwrap_or(Ipv4Addr::new(192, 0, 2, 10));
    let dkim_key = dkim_key_zone_value();
    let zone = format!(
        r#"$ORIGIN mock.test.
$TTL 60
@ IN SOA ns1.mock.test. hostmaster.mock.test. {serial} 3600 600 86400 60
@ IN NS ns1.mock.test.
@ IN NS ns2.mock.test.
@ IN MX 10 mail.mock.test.
@ IN TXT "v=spf1 -all"
ns1 IN A 192.0.2.53
ns2 IN A 192.0.2.54
mail IN A 192.0.2.25
www IN A {www}
www IN AAAA 2001:db8::10
alias IN CNAME www.mock.test.
loop1 IN CNAME loop2.mock.test.
loop2 IN CNAME loop1.mock.test.
*.wild IN A 192.0.2.99
sub IN NS ns.sub.mock.test.
ns.sub IN A 192.0.2.60
broken-ptr IN A 192.0.2.201
10.2.0.192.in-addr.arpa. IN PTR broken-ptr.mock.test.
big IN TXT "placeholder"
drop10 IN A 192.0.2.110
slow IN A 192.0.2.120
email IN TXT "v=spf1 -all"
default._domainkey.email IN TXT "v=DKIM1; k=rsa; p=" {dkim_key}
revoked._domainkey.email IN TXT "v=DKIM1; k=rsa; p="
_dmarc.email IN TXT "v=DMARC1; p=reject; rua=mailto:dmarc@mock.test; pct=100"
"#,
    );
    Parser::new(zone, None, Some(name("mock.test.")))
        .parse()
        .expect("parse mock zone")
        .1
}

fn name(value: &str) -> Name {
    Name::parse(value, None).expect("valid dns name")
}

fn tls_config() -> (ServerConfig, Vec<u8>) {
    let certified =
        generate_simple_self_signed(vec!["localhost".to_owned(), "mock.test".to_owned()])
            .expect("generate mock cert");
    let cert_der = certified.cert.der().to_vec();
    let key_der =
        PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(certified.key_pair.serialize_der()));
    let cert_chain = vec![CertificateDer::from(cert_der.clone())];
    let mut config =
        ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_safe_default_protocol_versions()
            .expect("protocol versions")
            .with_no_client_auth()
            .with_single_cert(cert_chain, key_der)
            .expect("server cert");
    config.alpn_protocols = vec![b"dot".to_vec()];
    (config, cert_der)
}
