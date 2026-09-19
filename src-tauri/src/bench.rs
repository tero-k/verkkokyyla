//! HTTP benchmark diagnostics engine (Web Benchmark).
//!
//! Provides detailed per-request timings, repeated-run statistics, cold/warm
//! connection modes, protocol comparison, an optional concurrency test, and an
//! optional connection probe for real DNS/TCP/TLS phase measurements.
//!
//! reqwest 0.12 is intentionally retained as the HTTP client. Metrics that
//! reqwest cannot expose (per-request TCP/TLS split, TLS version of the actual
//! request, connection-reuse flag) are reported as `null` rather than fabricated.
//! The optional probe measures a parallel connection to the same host and is
//! clearly labeled as such in the result.

use std::collections::HashMap;
use std::fmt;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use chrono::Utc;
use futures_util::stream;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};

use crate::bench_stats::{summarize, MetricStats};
use crate::download::{host_and_port, parse_url};
use crate::http_client::{self, HttpSettingsDto, HttpVersion, IpFamily};

/// Unique ID for a benchmark result, roughly sortable by time.
fn make_benchmark_id() -> String {
    format!(
        "wb-{}-{:04x}",
        Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        rand::random::<u16>()
    )
}

fn now_rfc3339() -> String {
    Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

static RUN_COUNTER: AtomicU64 = AtomicU64::new(0);

fn make_run_id() -> String {
    format!("run-{:06x}", RUN_COUNTER.fetch_add(1, Ordering::SeqCst))
}

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// How the benchmark should treat transport connections.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ConnectionMode {
    /// Establish a fresh client (and therefore a fresh connection) for each run.
    #[default]
    Cold,
    /// Share a single client across runs; the first run is a labelled warmup.
    Warm,
}

/// Request payload from the frontend.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BenchmarkConfig {
    pub url: String,
    pub protocols: Vec<HttpVersion>,
    #[serde(default = "default_runs")]
    pub runs: u32,
    #[serde(default)]
    pub connection_mode: ConnectionMode,
    /// Optional concurrency-test level. Values such as 1, 5, 10, 25, 50 are
    /// recommended; any value in 1..=50 is accepted.
    pub concurrency: Option<u32>,
    /// Whether to run the optional companion connection probe.
    #[serde(default)]
    pub probe: bool,
    #[serde(default)]
    pub http_settings: HttpSettingsDto,
}

fn default_runs() -> u32 {
    10
}

impl BenchmarkConfig {
    fn sanitized(mut self) -> Self {
        self.runs = self.runs.clamp(1, 100);
        if let Some(level) = self.concurrency {
            self.concurrency = Some(level.clamp(1, 50));
        }
        self.http_settings = self.http_settings.sanitized();
        self
    }
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// High-level benchmark error returned only for invalid input before any request.
#[derive(Clone, Debug, PartialEq, Serialize, thiserror::Error)]
#[serde(rename_all = "camelCase")]
pub struct BenchmarkError {
    pub kind: String,
    pub message: String,
}

impl BenchmarkError {
    fn invalid_url(message: impl fmt::Display) -> Self {
        Self {
            kind: "invalid-url".to_owned(),
            message: message.to_string(),
        }
    }
}

impl fmt::Display for BenchmarkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.kind, self.message)
    }
}

/// Classifies a reqwest error into a structured error kind.
fn classify_reqwest_error(err: &reqwest::Error) -> (&'static str, String) {
    use std::error::Error as _;

    let message = err.to_string();

    if err.is_timeout() {
        return ("http-timeout", message);
    }

    if err.is_connect() {
        let mut source_chain: Vec<String> = Vec::new();
        let mut source = err.source();
        while let Some(s) = source {
            source_chain.push(s.to_string().to_lowercase());
            source = s.source();
        }
        let kind = if source_chain
            .iter()
            .any(|s| s.contains("dns") || s.contains("resolve"))
        {
            "dns"
        } else if source_chain.iter().any(|s| s.contains("refused")) {
            "connect-refused"
        } else if message.to_lowercase().contains("certificate")
            || source_chain.iter().any(|s| s.contains("certificate"))
        {
            "certificate"
        } else if message.to_lowercase().contains("tls")
            || source_chain.iter().any(|s| s.contains("tls"))
        {
            "tls"
        } else {
            "connection-timeout"
        };
        return (kind, message);
    }

    if err.is_request() {
        let lower = message.to_lowercase();
        if lower.contains("redirect") || lower.contains("too many redirects") {
            return ("redirect-loop", message);
        }
        if lower.contains("http/2") || lower.contains("protocol") {
            return ("protocol-negotiation", message);
        }
        return ("request", message);
    }

    if err.is_status() {
        return ("http-status", message);
    }

    ("request", message)
}

// ---------------------------------------------------------------------------
// Results
// ---------------------------------------------------------------------------

/// The result of one measured HTTP request.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BenchmarkRun {
    pub run_id: String,
    pub url: String,
    pub requested_protocol: String,
    pub negotiated_protocol: Option<String>,
    pub started_at: String,
    pub status_code: Option<u16>,
    pub dns_ms: Option<f64>,
    pub connect_ms: Option<f64>,
    pub tls_ms: Option<f64>,
    pub ttfb_ms: Option<f64>,
    pub download_ms: Option<f64>,
    pub total_ms: Option<f64>,
    pub response_bytes: u64,
    pub decoded_bytes: Option<u64>,
    pub transferred_bytes: Option<u64>,
    pub throughput_bytes_per_second: Option<f64>,
    pub remote_ip: Option<String>,
    pub ip_version: Option<String>,
    pub tls_version: Option<String>,
    pub alpn: Option<String>,
    /// reqwest cannot reliably tell us whether the connection was reused.
    pub connection_reused: Option<bool>,
    pub connection_mode: String,
    pub redirect_count: u32,
    pub redirects: Vec<String>,
    pub final_url: String,
    pub content_encoding: Option<String>,
    pub success: bool,
    pub error_type: Option<String>,
    pub error_message: Option<String>,
    pub is_warmup: bool,
}

impl BenchmarkRun {
    fn new(url: &str, requested: HttpVersion, mode: ConnectionMode, is_warmup: bool) -> Self {
        Self {
            run_id: make_run_id(),
            url: url.to_owned(),
            requested_protocol: http_version_label(requested).to_owned(),
            negotiated_protocol: None,
            started_at: now_rfc3339(),
            status_code: None,
            dns_ms: None,
            connect_ms: None,
            tls_ms: None,
            ttfb_ms: None,
            download_ms: None,
            total_ms: None,
            response_bytes: 0,
            decoded_bytes: None,
            transferred_bytes: None,
            throughput_bytes_per_second: None,
            remote_ip: None,
            ip_version: None,
            tls_version: None,
            alpn: None,
            connection_reused: None,
            connection_mode: connection_mode_str(mode).to_owned(),
            redirect_count: 0,
            redirects: Vec::new(),
            final_url: url.to_owned(),
            content_encoding: None,
            success: false,
            error_type: None,
            error_message: None,
            is_warmup,
        }
    }

    fn with_error(mut self, kind: &str, message: impl fmt::Display) -> Self {
        self.error_type = Some(kind.to_owned());
        self.error_message = Some(message.to_string());
        self
    }

    fn with_dns(mut self, ms: f64) -> Self {
        self.dns_ms = Some(ms);
        self
    }
}

/// Optional companion probe: a separate DNS+TCP+TLS connection to the same host.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionProbe {
    pub dns_ms: Option<f64>,
    pub connect_ms: Option<f64>,
    pub tls_ms: Option<f64>,
    pub tls_version: Option<String>,
    pub alpn: Option<String>,
    pub session_resumed: Option<bool>,
    pub remote_ip: Option<String>,
    pub ip_version: Option<String>,
    pub error: Option<String>,
}

impl ConnectionProbe {
    fn empty() -> Self {
        Self {
            dns_ms: None,
            connect_ms: None,
            tls_ms: None,
            tls_version: None,
            alpn: None,
            session_resumed: None,
            remote_ip: None,
            ip_version: None,
            error: None,
        }
    }
}

/// Statistics calculated across multiple runs for one metric.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MetricSummary {
    pub count: usize,
    pub min: f64,
    pub max: f64,
    pub average: f64,
    pub p50: f64,
    pub p90: f64,
    pub p95: f64,
    pub p99: f64,
    pub stddev: f64,
}

impl From<MetricStats> for MetricSummary {
    fn from(s: MetricStats) -> Self {
        Self {
            count: s.count,
            min: s.min,
            max: s.max,
            average: s.mean,
            p50: s.p50,
            p90: s.p90,
            p95: s.p95,
            p99: s.p99,
            stddev: s.stddev,
        }
    }
}

fn metric_summary_or_empty(stats: Option<MetricStats>) -> MetricSummary {
    stats.map(MetricSummary::from).unwrap_or_else(|| {
        let empty = MetricStats::empty();
        MetricSummary {
            count: 0,
            min: empty.min,
            max: empty.max,
            average: empty.mean,
            p50: empty.p50,
            p90: empty.p90,
            p95: empty.p95,
            p99: empty.p99,
            stddev: empty.stddev,
        }
    })
}

/// Summary for one requested protocol.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProtocolSummary {
    pub requested_protocol: String,
    pub negotiated_protocol: Option<String>,
    pub successful_runs: usize,
    pub failed_runs: usize,
    pub total_ms: MetricSummary,
    pub ttfb_ms: MetricSummary,
    pub download_ms: MetricSummary,
    pub dns_ms: MetricSummary,
    pub connect_ms: MetricSummary,
    pub tls_ms: MetricSummary,
    pub response_bytes: MetricSummary,
    pub throughput_bytes_per_second: MetricSummary,
    pub probe: Option<ConnectionProbe>,
    pub error_type: Option<String>,
    pub error_message: Option<String>,
}

/// Result of an optional concurrency test.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConcurrencySummary {
    pub concurrency: u32,
    pub total_requests: usize,
    pub successful_requests: usize,
    pub failed_requests: usize,
    pub requests_per_second: f64,
    pub average_latency_ms: f64,
    pub p50_latency_ms: f64,
    pub p95_latency_ms: f64,
    pub p99_latency_ms: f64,
    pub min_latency_ms: f64,
    pub max_latency_ms: f64,
    pub total_bytes: u64,
    pub aggregate_throughput_bytes_per_second: f64,
    pub requested_protocol: String,
    pub negotiated_protocol: Option<String>,
}

/// Final result returned to the frontend.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BenchmarkResult {
    pub benchmark_id: String,
    pub url: String,
    pub started_at: String,
    pub config: BenchmarkConfig,
    pub runs: Vec<BenchmarkRun>,
    pub summaries: Vec<ProtocolSummary>,
    pub concurrency: Option<ConcurrencySummary>,
    pub concurrency_error: Option<BenchmarkError>,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn http_version_label(v: HttpVersion) -> &'static str {
    match v {
        HttpVersion::Auto => "auto",
        HttpVersion::Http1_1 => "http1.1",
        HttpVersion::Http2 => "http2",
        HttpVersion::Http3 => "http3",
    }
}

fn connection_mode_str(mode: ConnectionMode) -> &'static str {
    match mode {
        ConnectionMode::Cold => "cold",
        ConnectionMode::Warm => "warm",
    }
}

fn negotiated_version_string(version: reqwest::Version) -> String {
    match version {
        reqwest::Version::HTTP_09 => "HTTP/0.9",
        reqwest::Version::HTTP_10 => "HTTP/1.0",
        reqwest::Version::HTTP_11 => "HTTP/1.1",
        reqwest::Version::HTTP_2 => "HTTP/2",
        reqwest::Version::HTTP_3 => "HTTP/3",
        _ => "unknown",
    }
    .to_owned()
}

fn most_common_negotiated_protocol(runs: &[&BenchmarkRun]) -> Option<String> {
    let mut counts = HashMap::new();
    for run in runs {
        if let Some(proto) = run.negotiated_protocol.as_deref() {
            *counts.entry(proto.to_owned()).or_insert(0usize) += 1;
        }
    }
    counts
        .into_iter()
        .max_by_key(|(_, count)| *count)
        .map(|(proto, _)| proto)
}

fn ip_version_from_addr(addr: &SocketAddr) -> &'static str {
    if addr.is_ipv4() {
        "ipv4"
    } else {
        "ipv6"
    }
}

fn content_encoding_from_headers(headers: &reqwest::header::HeaderMap) -> Option<String> {
    headers
        .get(reqwest::header::CONTENT_ENCODING)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_lowercase())
}

/// Resolve a host and filter by address family, returning the addresses plus the
/// DNS lookup time in milliseconds.
async fn resolve_host(
    host: &str,
    port: u16,
    family: IpFamily,
) -> Result<(Vec<SocketAddr>, f64), BenchmarkError> {
    let start = Instant::now();
    let iter = tokio::net::lookup_host((host, port))
        .await
        .map_err(|e| BenchmarkError {
            kind: "dns".to_owned(),
            message: e.to_string(),
        })?;
    let dns_ms = start.elapsed().as_secs_f64() * 1000.0;

    let addrs: Vec<SocketAddr> = iter
        .filter(|a| match family {
            IpFamily::Auto => true,
            IpFamily::Ipv4 => a.is_ipv4(),
            IpFamily::Ipv6 => a.is_ipv6(),
        })
        .collect();

    if addrs.is_empty() {
        return Err(BenchmarkError {
            kind: "dns".to_owned(),
            message: format!("no matching addresses found for {}", host),
        });
    }

    Ok((addrs, dns_ms))
}

// ---------------------------------------------------------------------------
// Redirect logging
// ---------------------------------------------------------------------------

fn make_redirect_policy(
    follow: bool,
    max_redirects: u32,
    log: Arc<Mutex<Vec<String>>>,
) -> reqwest::redirect::Policy {
    if !follow {
        return reqwest::redirect::Policy::none();
    }

    let max = max_redirects as usize;
    reqwest::redirect::Policy::custom(move |attempt: reqwest::redirect::Attempt| {
        {
            let mut log = log.lock().expect("redirect log mutex poisoned");
            log.push(attempt.url().to_string());
        }

        if attempt.previous().len() >= max {
            attempt.error("too many redirects")
        } else {
            attempt.follow()
        }
    })
}

// ---------------------------------------------------------------------------
// Connection probe
// ---------------------------------------------------------------------------

fn build_tls_connector() -> Result<tokio_rustls::TlsConnector, String> {
    let roots = rustls::RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let config = rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    Ok(tokio_rustls::TlsConnector::from(Arc::new(config)))
}

async fn run_connection_probe(
    host: &str,
    port: u16,
    addrs: &[SocketAddr],
    connect_timeout: Duration,
    is_https: bool,
) -> ConnectionProbe {
    let mut probe = ConnectionProbe::empty();

    let dns_start = Instant::now();
    let selected =
        match tokio::time::timeout(connect_timeout, tokio::net::lookup_host((host, port))).await {
            Ok(Ok(mut iter)) => iter.next(),
            _ => None,
        };
    probe.dns_ms = Some(dns_start.elapsed().as_secs_f64() * 1000.0);

    let Some(addr) = selected.or_else(|| addrs.first().copied()) else {
        probe.error = Some("no address to probe".to_owned());
        return probe;
    };

    let connect_start = Instant::now();
    let stream =
        match tokio::time::timeout(connect_timeout, tokio::net::TcpStream::connect(addr)).await {
            Ok(Ok(s)) => s,
            Ok(Err(e)) => {
                probe.connect_ms = Some(connect_start.elapsed().as_secs_f64() * 1000.0);
                probe.error = Some(e.to_string());
                return probe;
            }
            Err(_) => {
                probe.connect_ms = Some(connect_start.elapsed().as_secs_f64() * 1000.0);
                probe.error = Some("probe connect timed out".to_owned());
                return probe;
            }
        };
    probe.connect_ms = Some(connect_start.elapsed().as_secs_f64() * 1000.0);
    probe.remote_ip = Some(addr.ip().to_string());
    probe.ip_version = Some(ip_version_from_addr(&addr).to_owned());

    if !is_https {
        return probe;
    }

    let connector = match build_tls_connector() {
        Ok(c) => c,
        Err(e) => {
            probe.error = Some(e);
            return probe;
        }
    };

    let server_name = match rustls::pki_types::ServerName::try_from(host.to_owned()) {
        Ok(name) => name,
        Err(e) => {
            probe.error = Some(format!("invalid server name: {e}"));
            return probe;
        }
    };

    let tls_start = Instant::now();
    match tokio::time::timeout(connect_timeout, connector.connect(server_name, stream)).await {
        Ok(Ok(tls)) => {
            probe.tls_ms = Some(tls_start.elapsed().as_secs_f64() * 1000.0);
            let (_tcp, conn) = tls.get_ref();
            probe.tls_version = conn.protocol_version().map(|v| format!("{:?}", v));
            probe.alpn = conn
                .alpn_protocol()
                .map(|b| String::from_utf8_lossy(b).to_string());
            // rustls does not expose session resumption reliably.
            probe.session_resumed = None;
        }
        Ok(Err(e)) => {
            probe.tls_ms = Some(tls_start.elapsed().as_secs_f64() * 1000.0);
            probe.error = Some(e.to_string());
        }
        Err(_) => {
            probe.tls_ms = Some(tls_start.elapsed().as_secs_f64() * 1000.0);
            probe.error = Some("probe TLS timed out".to_owned());
        }
    }

    probe
}

// ---------------------------------------------------------------------------
// Single request execution
// ---------------------------------------------------------------------------

fn build_benchmark_client(
    settings: &HttpSettingsDto,
    host: &str,
    addrs: &[SocketAddr],
    redirect_log: Arc<Mutex<Vec<String>>>,
) -> Result<reqwest::Client, reqwest::Error> {
    let policy = make_redirect_policy(
        settings.follow_redirects,
        settings.max_redirects,
        redirect_log,
    );
    let mut builder = http_client::base_builder(settings.clone())
        .redirect(policy)
        .resolve_to_addrs(host, addrs);
    builder = http_client::apply_user_agent(builder, settings, "verkkokyyla/0.1.0 web-benchmark");
    builder.build()
}

async fn finish_response(
    response: reqwest::Response,
    started: Instant,
    redirect_log: Arc<Mutex<Vec<String>>>,
    compression_enabled: bool,
    mut run: BenchmarkRun,
) -> BenchmarkRun {
    let ttfb_ms = started.elapsed().as_secs_f64() * 1000.0;
    run.ttfb_ms = Some(ttfb_ms);
    run.status_code = Some(response.status().as_u16());
    run.final_url = response.url().to_string();
    run.negotiated_protocol = Some(negotiated_version_string(response.version()));
    run.remote_ip = response.remote_addr().map(|a| a.to_string());
    run.ip_version = response
        .remote_addr()
        .map(|a| ip_version_from_addr(&a).to_owned());
    run.content_encoding = content_encoding_from_headers(response.headers());

    let mut bytes_received: u64 = 0;
    let mut stream = response.bytes_stream();
    while let Some(chunk_result) = stream.next().await {
        match chunk_result {
            Ok(chunk) => bytes_received += chunk.len() as u64,
            Err(e) => {
                let (kind, message) = classify_reqwest_error(&e);
                run.total_ms = Some(started.elapsed().as_secs_f64() * 1000.0);
                run.download_ms = run
                    .total_ms
                    .zip(run.ttfb_ms)
                    .map(|(total, ttfb)| (total - ttfb).max(0.0));
                run.response_bytes = bytes_received;
                run.decoded_bytes = Some(bytes_received);
                run.transferred_bytes = if compression_enabled {
                    None
                } else {
                    Some(bytes_received)
                };
                run.throughput_bytes_per_second = run.total_ms.and_then(|ms| {
                    let secs = ms / 1000.0;
                    if secs > 0.0 {
                        Some(bytes_received as f64 / secs)
                    } else {
                        None
                    }
                });
                run.redirects = {
                    let log = redirect_log.lock().expect("redirect log mutex poisoned");
                    log.clone()
                };
                run.redirect_count = run.redirects.len() as u32;
                return run.with_error(kind, message);
            }
        }
    }

    let total_ms = started.elapsed().as_secs_f64() * 1000.0;
    let download_ms = (total_ms - ttfb_ms).max(0.0);
    let secs = total_ms / 1000.0;
    let throughput = if secs > 0.0 {
        Some(bytes_received as f64 / secs)
    } else {
        None
    };

    run.total_ms = Some(total_ms);
    run.download_ms = Some(download_ms);
    run.response_bytes = bytes_received;
    run.decoded_bytes = Some(bytes_received);
    run.transferred_bytes = if compression_enabled {
        None
    } else {
        Some(bytes_received)
    };
    run.throughput_bytes_per_second = throughput;
    run.redirects = {
        let log = redirect_log.lock().expect("redirect log mutex poisoned");
        log.clone()
    };
    run.redirect_count = run.redirects.len() as u32;
    run.success = true;
    run
}

struct BenchmarkRequestContext<'a> {
    url: &'a str,
    parsed: &'a url::Url,
    host: &'a str,
    port: u16,
    settings: &'a HttpSettingsDto,
}

async fn run_single_request(
    context: &BenchmarkRequestContext<'_>,
    version: HttpVersion,
    mode: ConnectionMode,
    is_warmup: bool,
) -> BenchmarkRun {
    let run = BenchmarkRun::new(context.url, version, mode, is_warmup);

    if version == HttpVersion::Http3 {
        return run.with_error(
            "http3-unsupported",
            "HTTP/3 is not supported by the current networking stack",
        );
    }

    let (addrs, dns_ms) =
        match resolve_host(context.host, context.port, context.settings.ip_family).await {
            Ok(v) => v,
            Err(e) => {
                return run
                    .with_error(&e.kind, e.message)
                    .with_dns(dns_ms_placeholder())
            }
        };

    let redirect_log = Arc::new(Mutex::new(Vec::<String>::new()));
    let client = match build_benchmark_client(
        context.settings,
        context.host,
        &addrs,
        Arc::clone(&redirect_log),
    ) {
        Ok(c) => c,
        Err(e) => return run.with_dns(dns_ms).with_error("request", e),
    };

    let started = Instant::now();
    match client
        .get(context.parsed.as_str())
        .header("Cache-Control", "no-cache")
        .send()
        .await
    {
        Ok(response) => {
            let run = run.with_dns(dns_ms);
            finish_response(
                response,
                started,
                redirect_log,
                context.settings.compression,
                run,
            )
            .await
        }
        Err(e) => {
            let (kind, message) = classify_reqwest_error(&e);
            run.with_dns(dns_ms).with_error(kind, message)
        }
    }
}

fn dns_ms_placeholder() -> f64 {
    0.0
}

async fn run_single_warm_request(
    client: &reqwest::Client,
    url: &url::Url,
    version: HttpVersion,
    mode: ConnectionMode,
    is_warmup: bool,
    compression_enabled: bool,
    redirect_log: Arc<Mutex<Vec<String>>>,
) -> BenchmarkRun {
    let run = BenchmarkRun::new(url.as_str(), version, mode, is_warmup);
    let started = Instant::now();
    match client
        .get(url.as_str())
        .header("Cache-Control", "no-cache")
        .send()
        .await
    {
        Ok(response) => {
            finish_response(response, started, redirect_log, compression_enabled, run).await
        }
        Err(e) => {
            let (kind, message) = classify_reqwest_error(&e);
            run.with_error(kind, message)
        }
    }
}

// ---------------------------------------------------------------------------
// Repeated runs per protocol
// ---------------------------------------------------------------------------

async fn run_protocol_benchmark(
    context: &BenchmarkRequestContext<'_>,
    version: HttpVersion,
    mode: ConnectionMode,
    runs: u32,
    run_probe: bool,
) -> (Vec<BenchmarkRun>, Option<ConnectionProbe>) {
    let mut all_runs = Vec::new();
    // The per-protocol `version` must override the global HTTP settings version,
    // otherwise every requested protocol would use the same negotiated stack.
    let protocol_settings = HttpSettingsDto {
        version,
        ..context.settings.clone()
    };

    let protocol_context = BenchmarkRequestContext {
        settings: &protocol_settings,
        ..*context
    };

    // Resolve once for the probe and warm-mode shared client.
    let resolved = match resolve_host(context.host, context.port, protocol_settings.ip_family).await
    {
        Ok((addrs, _)) => addrs,
        Err(e) => {
            let run =
                BenchmarkRun::new(context.url, version, mode, false).with_error(&e.kind, e.message);
            all_runs.push(run);
            return (all_runs, None);
        }
    };

    let probe = if run_probe {
        let probe = run_connection_probe(
            context.host,
            context.port,
            &resolved,
            Duration::from_secs(protocol_settings.connect_timeout_sec),
            context.parsed.scheme() == "https",
        )
        .await;
        Some(probe)
    } else {
        None
    };

    match mode {
        ConnectionMode::Cold => {
            for _ in 0..runs {
                let run = run_single_request(&protocol_context, version, mode, false).await;
                all_runs.push(run);
            }
        }
        ConnectionMode::Warm => {
            let redirect_log = Arc::new(Mutex::new(Vec::<String>::new()));
            let client = match build_benchmark_client(
                &protocol_settings,
                context.host,
                &resolved,
                Arc::clone(&redirect_log),
            ) {
                Ok(c) => Arc::new(c),
                Err(e) => {
                    for _ in 0..=runs {
                        let run = BenchmarkRun::new(context.url, version, mode, false)
                            .with_error("request", e.to_string());
                        all_runs.push(run);
                    }
                    return (all_runs, probe);
                }
            };

            for i in 0..=runs {
                let is_warmup = i == 0;
                {
                    let mut log = redirect_log.lock().expect("redirect log mutex poisoned");
                    log.clear();
                }
                let run = run_single_warm_request(
                    &client,
                    context.parsed,
                    version,
                    mode,
                    is_warmup,
                    protocol_settings.compression,
                    Arc::clone(&redirect_log),
                )
                .await;
                all_runs.push(run);
            }
        }
    }

    (all_runs, probe)
}

// ---------------------------------------------------------------------------
// Aggregates
// ---------------------------------------------------------------------------

fn summarize_protocol_runs(
    version: HttpVersion,
    runs: &[BenchmarkRun],
    probe: Option<ConnectionProbe>,
) -> ProtocolSummary {
    let requested_protocol = http_version_label(version).to_owned();
    let measured: Vec<&BenchmarkRun> = runs.iter().filter(|r| r.success && !r.is_warmup).collect();
    let negotiated_protocol = most_common_negotiated_protocol(&measured);

    if measured.is_empty() {
        let first_error = runs.iter().find(|r| r.error_type.is_some()).map(|r| {
            (
                r.error_type.clone().unwrap(),
                r.error_message.clone().unwrap_or_default(),
            )
        });
        return ProtocolSummary {
            requested_protocol,
            negotiated_protocol,
            successful_runs: 0,
            failed_runs: runs.iter().filter(|r| !r.success && !r.is_warmup).count(),
            total_ms: metric_summary_or_empty(None),
            ttfb_ms: metric_summary_or_empty(None),
            download_ms: metric_summary_or_empty(None),
            dns_ms: metric_summary_or_empty(None),
            connect_ms: metric_summary_or_empty(None),
            tls_ms: metric_summary_or_empty(None),
            response_bytes: metric_summary_or_empty(None),
            throughput_bytes_per_second: metric_summary_or_empty(None),
            probe,
            error_type: first_error.as_ref().map(|(k, _)| k.clone()),
            error_message: first_error.map(|(_, m)| m),
        };
    }

    let extract = |f: fn(&BenchmarkRun) -> Option<f64>| {
        let samples: Vec<f64> = measured.iter().filter_map(|r| f(r)).collect();
        summarize(&samples)
    };

    let response_bytes_samples: Vec<f64> =
        measured.iter().map(|r| r.response_bytes as f64).collect();

    ProtocolSummary {
        requested_protocol,
        negotiated_protocol,
        successful_runs: measured.len(),
        failed_runs: runs.iter().filter(|r| !r.success && !r.is_warmup).count(),
        total_ms: metric_summary_or_empty(extract(|r| r.total_ms)),
        ttfb_ms: metric_summary_or_empty(extract(|r| r.ttfb_ms)),
        download_ms: metric_summary_or_empty(extract(|r| r.download_ms)),
        dns_ms: metric_summary_or_empty(extract(|r| r.dns_ms)),
        connect_ms: metric_summary_or_empty(extract(|r| r.connect_ms)),
        tls_ms: metric_summary_or_empty(extract(|r| r.tls_ms)),
        response_bytes: metric_summary_or_empty(summarize(&response_bytes_samples)),
        throughput_bytes_per_second: metric_summary_or_empty(extract(|r| {
            r.throughput_bytes_per_second
        })),
        probe,
        error_type: None,
        error_message: None,
    }
}

// ---------------------------------------------------------------------------
// Concurrency test
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
struct ConcurrencyRequestResult {
    success: bool,
    latency_ms: f64,
    bytes: u64,
    negotiated: String,
}

pub async fn run_concurrency_benchmark(
    url: &str,
    host: &str,
    port: u16,
    settings: &HttpSettingsDto,
    level: u32,
) -> Result<ConcurrencySummary, BenchmarkError> {
    if settings.version == HttpVersion::Http3 {
        return Err(BenchmarkError {
            kind: "http3-unsupported".to_owned(),
            message: "HTTP/3 is not supported for concurrency tests".to_owned(),
        });
    }

    let (addrs, _dns_ms) = resolve_host(host, port, settings.ip_family).await?;
    let redirect_log = Arc::new(Mutex::new(Vec::<String>::new()));
    let client = build_benchmark_client(settings, host, &addrs, Arc::clone(&redirect_log))
        .map_err(|e| BenchmarkError {
            kind: "request".to_owned(),
            message: e.to_string(),
        })?;

    let total_requests = (level * 10) as usize;
    let requested_protocol = http_version_label(settings.version).to_owned();
    let urls = vec![url.to_owned(); total_requests];

    let started_all = Instant::now();
    let results: Vec<ConcurrencyRequestResult> = stream::iter(urls)
        .map(|u| {
            let client = client.clone();
            async move {
                let start = Instant::now();
                match client
                    .get(&u)
                    .header("Cache-Control", "no-cache")
                    .send()
                    .await
                {
                    Ok(response) => {
                        let negotiated = negotiated_version_string(response.version());
                        let mut bytes: u64 = 0;
                        let mut stream = response.bytes_stream();
                        while let Some(chunk) = stream.next().await {
                            match chunk {
                                Ok(c) => bytes += c.len() as u64,
                                Err(_) => {
                                    return ConcurrencyRequestResult {
                                        success: false,
                                        latency_ms: start.elapsed().as_secs_f64() * 1000.0,
                                        bytes,
                                        negotiated,
                                    };
                                }
                            }
                        }
                        ConcurrencyRequestResult {
                            success: true,
                            latency_ms: start.elapsed().as_secs_f64() * 1000.0,
                            bytes,
                            negotiated,
                        }
                    }
                    Err(_) => ConcurrencyRequestResult {
                        success: false,
                        latency_ms: start.elapsed().as_secs_f64() * 1000.0,
                        bytes: 0,
                        negotiated: "unknown".to_owned(),
                    },
                }
            }
        })
        .buffer_unordered(level as usize)
        .collect()
        .await;
    let total_duration_s = started_all.elapsed().as_secs_f64();

    let successful: Vec<&ConcurrencyRequestResult> = results.iter().filter(|r| r.success).collect();
    let failed = results.len() - successful.len();
    let total_bytes: u64 = successful.iter().map(|r| r.bytes).sum();

    let latencies: Vec<f64> = results.iter().map(|r| r.latency_ms).collect();
    let latency_stats = summarize(&latencies);

    let negotiated_protocol = successful
        .first()
        .map(|r| r.negotiated.clone())
        .or_else(|| {
            results
                .iter()
                .find(|r| !r.negotiated.is_empty() && r.negotiated != "unknown")
                .map(|r| r.negotiated.clone())
        });

    let requests_per_second = if total_duration_s > 0.0 {
        results.len() as f64 / total_duration_s
    } else {
        0.0
    };
    let aggregate_throughput = if total_duration_s > 0.0 {
        total_bytes as f64 / total_duration_s
    } else {
        0.0
    };

    Ok(ConcurrencySummary {
        concurrency: level,
        total_requests: results.len(),
        successful_requests: successful.len(),
        failed_requests: failed,
        requests_per_second,
        average_latency_ms: latency_stats.map(|s| s.mean).unwrap_or(0.0),
        p50_latency_ms: latency_stats.map(|s| s.p50).unwrap_or(0.0),
        p95_latency_ms: latency_stats.map(|s| s.p95).unwrap_or(0.0),
        p99_latency_ms: latency_stats.map(|s| s.p99).unwrap_or(0.0),
        min_latency_ms: latency_stats.map(|s| s.min).unwrap_or(0.0),
        max_latency_ms: latency_stats.map(|s| s.max).unwrap_or(0.0),
        total_bytes,
        aggregate_throughput_bytes_per_second: aggregate_throughput,
        requested_protocol,
        negotiated_protocol,
    })
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Run a Web Benchmark with the supplied configuration.
pub async fn run_benchmark(config: BenchmarkConfig) -> Result<BenchmarkResult, BenchmarkError> {
    let config = config.sanitized();
    let parsed = parse_url(&config.url).map_err(|e| BenchmarkError::invalid_url(e.to_string()))?;
    let (host, port) =
        host_and_port(&parsed).map_err(|e| BenchmarkError::invalid_url(e.to_string()))?;

    let mut runs = Vec::new();
    let mut summaries = Vec::new();

    let request_context = BenchmarkRequestContext {
        url: &config.url,
        parsed: &parsed,
        host: &host,
        port,
        settings: &config.http_settings,
    };

    for &version in &config.protocols {
        let (protocol_runs, probe) = run_protocol_benchmark(
            &request_context,
            version,
            config.connection_mode,
            config.runs,
            config.probe,
        )
        .await;
        summaries.push(summarize_protocol_runs(version, &protocol_runs, probe));
        runs.extend(protocol_runs);
    }

    let (concurrency, concurrency_error) = if let Some(level) = config.concurrency {
        match run_concurrency_benchmark(&config.url, &host, port, &config.http_settings, level)
            .await
        {
            Ok(summary) => (Some(summary), None),
            Err(e) => (None, Some(e)),
        }
    } else {
        (None, None)
    };

    Ok(BenchmarkResult {
        benchmark_id: make_benchmark_id(),
        url: config.url.clone(),
        started_at: now_rfc3339(),
        config,
        runs,
        summaries,
        concurrency,
        concurrency_error,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tokio::time::Duration;

    fn default_settings() -> HttpSettingsDto {
        HttpSettingsDto {
            version: HttpVersion::Auto,
            connect_timeout_sec: 5,
            request_timeout_sec: 5,
            read_timeout_sec: 0,
            follow_redirects: true,
            max_redirects: 10,
            compression: false,
            ip_family: IpFamily::Auto,
            user_agent: String::new(),
        }
    }

    fn http_response(status: &str, content_type: &str, body: &[u8]) -> Vec<u8> {
        let mut response = format!(
            "HTTP/1.1 {status}\r\nContent-Length: {}\r\nContent-Type: {content_type}\r\nConnection: close\r\n\r\n",
            body.len()
        )
        .into_bytes();
        response.extend_from_slice(body);
        response
    }

    type TestHttpHandler = Arc<dyn Fn(&str) -> Option<Vec<u8>> + Send + Sync>;

    async fn http_server(handler: TestHttpHandler) -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind to random port");
        let port = listener.local_addr().unwrap().port();

        tokio::spawn(async move {
            loop {
                let (mut stream, _) = match listener.accept().await {
                    Ok(v) => v,
                    Err(_) => break,
                };
                let handler = Arc::clone(&handler);
                tokio::spawn(async move {
                    let mut buf = [0u8; 2048];
                    let n = stream.read(&mut buf).await.unwrap_or(0);
                    let request = String::from_utf8_lossy(&buf[..n]);
                    let first_line = request.lines().next().unwrap_or("");
                    match handler(first_line) {
                        Some(response) => {
                            let _ = stream.write_all(&response).await;
                            let _ = stream.shutdown().await;
                        }
                        None => {
                            // Keep the connection open so the client times out.
                            tokio::time::sleep(Duration::from_secs(60)).await;
                        }
                    }
                });
            }
        });

        port
    }

    fn benchmark_config(url: String) -> BenchmarkConfig {
        BenchmarkConfig {
            url,
            protocols: vec![HttpVersion::Http1_1],
            runs: 1,
            connection_mode: ConnectionMode::Cold,
            concurrency: None,
            probe: false,
            http_settings: default_settings(),
        }
    }

    #[tokio::test]
    async fn successful_http11_request() {
        let body = b"hello benchmark".to_vec();
        let response = http_response("200 OK", "text/plain", &body);
        let handler = Arc::new(move |_: &str| Some(response.clone()));
        let port = http_server(handler).await;

        let result = run_benchmark(benchmark_config(format!("http://127.0.0.1:{port}/"))).await;
        let result = result.expect("benchmark should succeed");
        assert_eq!(result.summaries.len(), 1);
        let summary = &result.summaries[0];
        assert_eq!(summary.requested_protocol, "http1.1");
        assert_eq!(summary.negotiated_protocol, Some("HTTP/1.1".to_owned()));
        assert_eq!(summary.successful_runs, 1);
        assert!(summary.total_ms.average > 0.0);

        let run = &result.runs[0];
        assert_eq!(run.status_code, Some(200));
        assert_eq!(run.response_bytes, body.len() as u64);
        assert_eq!(run.negotiated_protocol, Some("HTTP/1.1".to_owned()));
        assert!(run.remote_ip.is_some());
        assert_eq!(run.error_type, None);
    }

    #[tokio::test]
    async fn auto_protocol_summary_reports_negotiated_version() {
        let body = b"hello benchmark".to_vec();
        let response = http_response("200 OK", "text/plain", &body);
        let handler = Arc::new(move |_: &str| Some(response.clone()));
        let port = http_server(handler).await;

        let config = BenchmarkConfig {
            protocols: vec![HttpVersion::Auto],
            ..benchmark_config(format!("http://127.0.0.1:{port}/"))
        };
        let result = run_benchmark(config)
            .await
            .expect("benchmark should succeed");
        assert_eq!(result.summaries.len(), 1);
        let summary = &result.summaries[0];
        assert_eq!(summary.requested_protocol, "auto");
        assert_eq!(summary.negotiated_protocol, Some("HTTP/1.1".to_owned()));
        assert_eq!(summary.successful_runs, 1);

        let run = &result.runs[0];
        assert_eq!(run.requested_protocol, "auto");
        assert_eq!(run.negotiated_protocol, Some("HTTP/1.1".to_owned()));
    }

    #[tokio::test]
    async fn protocols_use_requested_version_not_settings_version() {
        let body = b"hello benchmark".to_vec();
        let response = http_response("200 OK", "text/plain", &body);
        let handler = Arc::new(move |_: &str| Some(response.clone()));
        let port = http_server(handler).await;

        let config = BenchmarkConfig {
            protocols: vec![HttpVersion::Http1_1, HttpVersion::Http2],
            runs: 1,
            ..benchmark_config(format!("http://127.0.0.1:{port}/"))
        };
        let result = run_benchmark(config)
            .await
            .expect("benchmark should succeed");
        assert_eq!(result.summaries.len(), 2);

        let h1 = result
            .summaries
            .iter()
            .find(|s| s.requested_protocol == "http1.1")
            .expect("http1.1 summary");
        assert_eq!(h1.successful_runs, 1);
        assert_eq!(h1.negotiated_protocol, Some("HTTP/1.1".to_owned()));

        let h2 = result
            .summaries
            .iter()
            .find(|s| s.requested_protocol == "http2")
            .expect("http2 summary");
        // The test server only speaks HTTP/1.1, so an HTTP/2 prior-knowledge
        // request must fail rather than silently fall back to the settings version.
        assert_eq!(h2.successful_runs, 0);
        assert!(h2.error_type.is_some());
    }

    #[tokio::test]
    async fn http3_request_reports_unsupported() {
        let config = BenchmarkConfig {
            protocols: vec![HttpVersion::Http3],
            ..benchmark_config("http://127.0.0.1:1/".to_owned())
        };
        let result = run_benchmark(config)
            .await
            .expect("benchmark should return result");
        assert_eq!(result.summaries.len(), 1);
        let summary = &result.summaries[0];
        assert_eq!(summary.requested_protocol, "http3");
        assert_eq!(summary.successful_runs, 0);
        assert_eq!(summary.error_type.as_deref(), Some("http3-unsupported"));
    }

    #[tokio::test]
    async fn invalid_url_rejected() {
        let config = benchmark_config("not-a-url".to_owned());
        let err = run_benchmark(config)
            .await
            .expect_err("invalid url should fail");
        assert_eq!(err.kind, "invalid-url");
    }

    #[tokio::test]
    async fn dns_failure_is_structured() {
        let config = BenchmarkConfig {
            protocols: vec![HttpVersion::Http1_1],
            http_settings: HttpSettingsDto {
                connect_timeout_sec: 1,
                request_timeout_sec: 2,
                ..default_settings()
            },
            ..benchmark_config("http://verkkokyyla-test.invalid/".to_owned())
        };
        let result = tokio::time::timeout(Duration::from_secs(5), run_benchmark(config))
            .await
            .expect("benchmark should not hang")
            .expect("benchmark should return result");
        assert_eq!(result.summaries.len(), 1);
        let summary = &result.summaries[0];
        assert_eq!(summary.successful_runs, 0);
        assert_eq!(summary.error_type.as_deref(), Some("dns"));
    }

    #[tokio::test]
    async fn redirects_are_followed_and_counted() {
        let response = http_response("200 OK", "text/plain", b"final");
        let handler = Arc::new(move |first_line: &str| {
            if first_line.starts_with("GET /redirect") {
                Some("HTTP/1.1 302 Found\r\nLocation: /final\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string().into_bytes())
            } else {
                Some(response.clone())
            }
        });
        let port = http_server(handler).await;

        let result = run_benchmark(benchmark_config(format!(
            "http://127.0.0.1:{port}/redirect"
        )))
        .await
        .expect("benchmark should succeed");
        let run = &result.runs[0];
        assert_eq!(run.redirect_count, 1);
        assert!(run.final_url.contains("/final"));
        assert_eq!(run.status_code, Some(200));
    }

    #[tokio::test]
    async fn redirects_can_be_disabled() {
        let handler = Arc::new(|first_line: &str| {
            if first_line.starts_with("GET /redirect") {
                Some("HTTP/1.1 302 Found\r\nLocation: /final\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string().into_bytes())
            } else {
                Some(http_response("200 OK", "text/plain", b"final"))
            }
        });
        let port = http_server(handler).await;

        let config = BenchmarkConfig {
            http_settings: HttpSettingsDto {
                follow_redirects: false,
                ..default_settings()
            },
            ..benchmark_config(format!("http://127.0.0.1:{port}/redirect"))
        };
        let result = run_benchmark(config)
            .await
            .expect("benchmark should succeed");
        let run = &result.runs[0];
        assert_eq!(run.redirect_count, 0);
        assert_eq!(run.status_code, Some(302));
    }

    #[tokio::test]
    async fn repeated_runs_produce_statistics() {
        let body = b"x".repeat(100);
        let response = http_response("200 OK", "text/plain", &body);
        let handler = Arc::new(move |_: &str| Some(response.clone()));
        let port = http_server(handler).await;

        let config = BenchmarkConfig {
            runs: 5,
            ..benchmark_config(format!("http://127.0.0.1:{port}/"))
        };
        let result = run_benchmark(config)
            .await
            .expect("benchmark should succeed");
        let summary = &result.summaries[0];
        assert_eq!(summary.successful_runs, 5);
        assert_eq!(summary.total_ms.count, 5);
        assert!(summary.total_ms.min > 0.0);
        assert!(summary.total_ms.max >= summary.total_ms.min);
        assert!(summary.total_ms.p50 >= summary.total_ms.min);
        assert!(summary.total_ms.p95 >= summary.total_ms.p50);
        assert_eq!(result.runs.len(), 5);
    }

    #[tokio::test]
    async fn warm_mode_labels_warmup_and_measures() {
        let body = b"x".repeat(100);
        let response = http_response("200 OK", "text/plain", &body);
        let handler = Arc::new(move |_: &str| Some(response.clone()));
        let port = http_server(handler).await;

        let config = BenchmarkConfig {
            runs: 3,
            connection_mode: ConnectionMode::Warm,
            ..benchmark_config(format!("http://127.0.0.1:{port}/"))
        };
        let result = run_benchmark(config)
            .await
            .expect("benchmark should succeed");
        let warmups = result.runs.iter().filter(|r| r.is_warmup).count();
        let measured = result
            .runs
            .iter()
            .filter(|r| !r.is_warmup && r.success)
            .count();
        assert_eq!(warmups, 1);
        assert_eq!(measured, 3);

        let summary = &result.summaries[0];
        assert_eq!(summary.successful_runs, 3);
        assert_eq!(summary.failed_runs, 0);
    }

    #[tokio::test]
    async fn concurrency_test_completes() {
        let body = b"ok";
        let response = http_response("200 OK", "text/plain", body);
        let handler = Arc::new(move |_: &str| Some(response.clone()));
        let port = http_server(handler).await;

        let config = BenchmarkConfig {
            protocols: vec![HttpVersion::Auto],
            runs: 1,
            concurrency: Some(5),
            ..benchmark_config(format!("http://127.0.0.1:{port}/"))
        };
        let result = run_benchmark(config)
            .await
            .expect("benchmark should succeed");
        let c = result
            .concurrency
            .expect("concurrency summary should be present");
        assert_eq!(c.concurrency, 5);
        assert_eq!(c.total_requests, 50);
        assert_eq!(c.failed_requests, 0);
        assert!(c.requests_per_second > 0.0);
        assert!(c.aggregate_throughput_bytes_per_second > 0.0);
    }

    #[tokio::test]
    async fn timeout_is_reported() {
        let handler = Arc::new(|_: &str| {
            // Return nothing so the server keeps the connection open.
            None
        });
        let port = http_server(handler).await;

        let config = BenchmarkConfig {
            http_settings: HttpSettingsDto {
                request_timeout_sec: 1,
                connect_timeout_sec: 1,
                ..default_settings()
            },
            ..benchmark_config(format!("http://127.0.0.1:{port}/"))
        };
        let result = tokio::time::timeout(Duration::from_secs(5), run_benchmark(config))
            .await
            .expect("benchmark should not hang")
            .expect("benchmark should return result");
        let run = &result.runs[0];
        assert!(!run.success);
        assert_eq!(run.error_type.as_deref(), Some("http-timeout"));
    }

    #[tokio::test]
    async fn response_sizes_are_reported() {
        for size in [10, 10_000, 100_000] {
            let body = vec![b'x'; size];
            let response = http_response("200 OK", "text/plain", &body);
            let handler = Arc::new(move |_: &str| Some(response.clone()));
            let port = http_server(handler).await;

            let config = benchmark_config(format!("http://127.0.0.1:{port}/"));
            let result = run_benchmark(config)
                .await
                .expect("benchmark should succeed");
            assert_eq!(result.runs[0].response_bytes, size as u64);
        }
    }

    #[tokio::test]
    async fn compression_flag_controls_transferred_bytes() {
        let body = b"compressed-ish body";
        let response = http_response("200 OK", "text/plain", body);
        let handler = Arc::new(move |_: &str| Some(response.clone()));
        let port = http_server(handler).await;

        let off_config = BenchmarkConfig {
            http_settings: HttpSettingsDto {
                compression: false,
                ..default_settings()
            },
            ..benchmark_config(format!("http://127.0.0.1:{port}/"))
        };
        let off = run_benchmark(off_config).await.unwrap();
        assert_eq!(off.runs[0].transferred_bytes, Some(body.len() as u64));

        let on_config = BenchmarkConfig {
            http_settings: HttpSettingsDto {
                compression: true,
                ..default_settings()
            },
            ..benchmark_config(format!("http://127.0.0.1:{port}/"))
        };
        let on = run_benchmark(on_config).await.unwrap();
        assert_eq!(on.runs[0].transferred_bytes, None);
        assert_eq!(on.runs[0].decoded_bytes, Some(body.len() as u64));
    }
}
