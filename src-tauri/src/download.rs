use std::time::{Duration, Instant};

use futures_util::StreamExt;
use serde::Serialize;

use crate::http_client::{build_client, HttpSettingsDto};

/// Progress event streamed over the `on_progress` channel.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadProgressEvent {
    pub event: &'static str,
    pub bytes_received: u64,
    pub content_length: Option<u64>,
    pub elapsed_ms: u64,
    pub current_mbps: f64,
}

/// Final result of a download speed test.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadSpeedResultDto {
    pub url: String,
    pub final_url: String,
    pub status_code: u16,
    pub content_length: Option<u64>,
    pub bytes_received: u64,
    pub total_time_ms: u64,
    pub time_to_first_byte_ms: Option<u64>,
    pub dns_resolution_ms: Option<u64>,
    pub tls_handshake_ms: Option<u64>,
    pub average_mbps: f64,
}

/// Errors that can occur while running a download speed test.
#[derive(Debug, thiserror::Error)]
pub enum DownloadError {
    #[error("URL must use http or https")]
    InvalidScheme,
    #[error("URL is invalid: {0}")]
    InvalidUrl(String),
    #[error("URL must include a host")]
    MissingHost,
    #[error("DNS resolution failed: {0}")]
    Dns(String),
    #[error("request failed: {0}")]
    Request(String),
}

impl DownloadError {
    fn kind(&self) -> &'static str {
        match self {
            Self::InvalidScheme => "invalid-scheme",
            Self::InvalidUrl(_) => "invalid-url",
            Self::MissingHost => "missing-host",
            Self::Dns(_) => "dns",
            Self::Request(_) => "request",
        }
    }
}

impl Serialize for DownloadError {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("DownloadError", 2)?;
        state.serialize_field("kind", self.kind())?;
        state.serialize_field("message", &self.to_string())?;
        state.end()
    }
}

pub(crate) fn parse_url(url: &str) -> Result<url::Url, DownloadError> {
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err(DownloadError::InvalidScheme);
    }
    let parsed = url::Url::parse(url).map_err(|e| {
        let message = e.to_string();
        if message.contains("empty host") {
            DownloadError::MissingHost
        } else {
            DownloadError::InvalidUrl(message)
        }
    })?;
    let host = parsed.host_str().unwrap_or("");
    if host.is_empty() {
        return Err(DownloadError::MissingHost);
    }
    Ok(parsed)
}

pub(crate) fn host_and_port(parsed: &url::Url) -> Result<(String, u16), DownloadError> {
    let host = parsed.host_str().unwrap_or("");
    if host.is_empty() {
        return Err(DownloadError::MissingHost);
    }
    let port = parsed
        .port_or_known_default()
        .ok_or_else(|| DownloadError::InvalidUrl("could not determine port".to_owned()))?;
    Ok((host.to_owned(), port))
}

pub(crate) fn mbps_from(bytes: u64, elapsed: Duration) -> f64 {
    let secs = elapsed.as_secs_f64();
    if secs <= 0.0 || bytes == 0 {
        return 0.0;
    }
    (bytes as f64) * 8.0 / secs / 1_000_000.0
}

/// Run a download speed test against `url` and stream progress via `on_progress`.
pub async fn run_download_speed_test<F>(
    url: &str,
    settings: HttpSettingsDto,
    on_progress: F,
) -> Result<DownloadSpeedResultDto, DownloadError>
where
    F: Fn(DownloadProgressEvent) + Send + Sync + 'static,
{
    let parsed = parse_url(url)?;
    let (host, port) = host_and_port(&parsed)?;

    let dns_start = Instant::now();
    let _ = tokio::net::lookup_host((host, port))
        .await
        .map_err(|e| DownloadError::Dns(e.to_string()))?
        .collect::<Vec<_>>();
    let dns_resolution_ms = Some(dns_start.elapsed().as_millis() as u64);

    let client = build_client(settings, "verkkokyyla/0.1.0 download-speed-test")?;

    let request = client
        .get(parsed.as_str())
        .header("Cache-Control", "no-cache");

    let started = Instant::now();
    let response = request
        .send()
        .await
        .map_err(|e| DownloadError::Request(e.to_string()))?;

    let status_code = response.status().as_u16();
    let final_url = response.url().to_string();
    let content_length = response.content_length();

    let mut bytes_received: u64 = 0;
    let mut time_to_first_byte_ms: Option<u64> = None;
    let mut stream = response.bytes_stream();
    let mut last_progress = Instant::now();

    while let Some(chunk_result) = stream.next().await {
        let chunk = chunk_result.map_err(|e| DownloadError::Request(e.to_string()))?;
        if time_to_first_byte_ms.is_none() {
            time_to_first_byte_ms = Some(started.elapsed().as_millis() as u64);
        }
        bytes_received += chunk.len() as u64;

        if last_progress.elapsed() >= Duration::from_millis(100) {
            let elapsed = started.elapsed();
            on_progress(DownloadProgressEvent {
                event: "progress",
                bytes_received,
                content_length,
                elapsed_ms: elapsed.as_millis() as u64,
                current_mbps: mbps_from(bytes_received, elapsed),
            });
            last_progress = Instant::now();
        }
    }

    let total_elapsed = started.elapsed();
    on_progress(DownloadProgressEvent {
        event: "progress",
        bytes_received,
        content_length,
        elapsed_ms: total_elapsed.as_millis() as u64,
        current_mbps: mbps_from(bytes_received, total_elapsed),
    });

    Ok(DownloadSpeedResultDto {
        url: url.to_owned(),
        final_url,
        status_code,
        content_length,
        bytes_received,
        total_time_ms: total_elapsed.as_millis() as u64,
        time_to_first_byte_ms,
        dns_resolution_ms,
        tls_handshake_ms: None,
        average_mbps: mbps_from(bytes_received, total_elapsed),
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tokio::time::sleep;

    use crate::http_client::{HttpSettingsDto, HttpVersion, IpFamily};

    fn default_settings() -> HttpSettingsDto {
        HttpSettingsDto {
            version: HttpVersion::Auto,
            connect_timeout_sec: 10,
            request_timeout_sec: 60,
            read_timeout_sec: 0,
            follow_redirects: true,
            max_redirects: 10,
            compression: true,
            ip_family: IpFamily::Auto,
            user_agent: String::new(),
        }
    }

    async fn local_server(response: Vec<u8>) -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind to random port");
        let port = listener.local_addr().unwrap().port();

        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 1024];
            let _ = stream.read(&mut buf).await;
            let _ = stream.write_all(&response).await;
            let _ = stream.shutdown().await;
        });
        sleep(Duration::from_millis(200)).await;

        port
    }

    fn http_response_ok(body: &[u8]) -> Vec<u8> {
        let mut response = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\n",
            body.len()
        )
        .into_bytes();
        response.extend_from_slice(body);
        response
    }

    #[tokio::test]
    async fn valid_http_download_returns_stats() {
        let body = b"0123456789".repeat(100);
        let port = local_server(http_response_ok(&body)).await;
        let url = format!("http://127.0.0.1:{port}/");

        let progress_count = Arc::new(AtomicU64::new(0));
        let counter = Arc::clone(&progress_count);
        let result = run_download_speed_test(&url, default_settings(), move |_event| {
            counter.fetch_add(1, Ordering::SeqCst);
        })
        .await
        .unwrap();

        assert_eq!(result.url, url);
        assert_eq!(result.status_code, 200);
        assert_eq!(result.bytes_received, body.len() as u64);
        assert_eq!(result.content_length, Some(body.len() as u64));
        assert!(result.total_time_ms > 0);
        assert!(result.time_to_first_byte_ms.is_some());
        assert!(result.dns_resolution_ms.is_some());
        assert!(result.tls_handshake_ms.is_none());
        assert!(result.average_mbps.is_finite());
        assert!(result.average_mbps > 0.0);
        assert!(progress_count.load(Ordering::SeqCst) >= 1);
    }

    #[tokio::test]
    async fn missing_scheme_is_invalid() {
        let err = run_download_speed_test("example.com/path", default_settings(), |_event| {})
            .await
            .unwrap_err();
        assert!(matches!(err, DownloadError::InvalidScheme));
    }

    #[tokio::test]
    async fn ftp_scheme_is_invalid() {
        let err =
            run_download_speed_test("ftp://example.com/file", default_settings(), |_event| {})
                .await
                .unwrap_err();
        assert!(matches!(err, DownloadError::InvalidScheme));
    }

    #[tokio::test]
    async fn missing_host_is_rejected() {
        let err = run_download_speed_test("http:///", default_settings(), |_event| {})
            .await
            .unwrap_err();
        assert!(matches!(err, DownloadError::MissingHost));
    }

    #[tokio::test]
    async fn non_2xx_status_is_preserved() {
        let response = b"HTTP/1.1 404 Not Found\r\nContent-Length: 5\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\nhello".to_vec();
        let port = local_server(response).await;
        let url = format!("http://127.0.0.1:{port}/missing");

        let result = run_download_speed_test(&url, default_settings(), |_event| {})
            .await
            .unwrap();
        assert_eq!(result.status_code, 404);
        assert_eq!(result.bytes_received, 5);
        assert!(result.average_mbps.is_finite());
    }

    #[tokio::test]
    async fn chunked_response_reports_full_body() {
        let response = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\n5\r\nhello\r\n0\r\n\r\n".to_vec();
        let port = local_server(response).await;
        let url = format!("http://127.0.0.1:{port}/chunked");

        let result = run_download_speed_test(&url, default_settings(), |_event| {})
            .await
            .unwrap();
        assert_eq!(result.status_code, 200);
        assert_eq!(result.bytes_received, 5);
        assert_eq!(result.content_length, None);
        assert!(result.average_mbps.is_finite());
    }
}
