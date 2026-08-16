use std::time::{Duration, Instant};

use futures_util::{StreamExt, stream};
use serde::Serialize;

use crate::download::{mbps_from, parse_url, DownloadError};
use crate::http_client::{build_client, HttpSettingsDto};

/// Resource type for a discovered subresource.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PageResourceType {
    Document,
    Stylesheet,
    Script,
    Image,
    Font,
    Media,
    Iframe,
    Other,
}

/// One row in the page waterfall.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PageResource {
    pub url: String,
    pub resource_type: PageResourceType,
    pub status_code: Option<u16>,
    pub content_length: Option<u64>,
    pub bytes_received: u64,
    pub start_offset_ms: u64,
    pub duration_ms: u64,
    pub time_to_first_byte_ms: Option<u64>,
    pub average_mbps: f64,
    pub error: Option<String>,
}

/// Progress event streamed during a full-page speed test.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PageProgressEvent {
    pub event: &'static str,
    pub completed_resources: u32,
    pub total_resources: u32,
    pub bytes_received: u64,
    pub elapsed_ms: u64,
    pub current_mbps: f64,
    pub resource: PageResource,
}

/// Final result of a full-page download speed test.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PageSpeedResultDto {
    pub url: String,
    pub final_url: String,
    pub status_code: u16,
    pub total_resources: u32,
    pub successful_resources: u32,
    pub failed_resources: u32,
    pub total_content_length: Option<u64>,
    pub total_bytes_received: u64,
    pub total_duration_ms: u64,
    pub time_to_first_byte_ms: Option<u64>,
    pub average_mbps: f64,
    pub resources: Vec<PageResource>,
    pub parse_error: Option<String>,
}

const MAX_CONCURRENT: usize = 6;
const PROGRESS_THROTTLE_MS: u64 = 100;

struct DiscoveredResource {
    url: url::Url,
    resource_type: PageResourceType,
}

impl PageResource {
    fn failed(url: String, resource_type: PageResourceType, error: String) -> Self {
        Self {
            url,
            resource_type,
            status_code: None,
            content_length: None,
            bytes_received: 0,
            start_offset_ms: 0,
            duration_ms: 0,
            time_to_first_byte_ms: None,
            average_mbps: 0.0,
            error: Some(error),
        }
    }
}

fn classify_resource(element: &scraper::ElementRef<'_>) -> PageResourceType {
    let name = element.value().name();
    match name {
        "script" => PageResourceType::Script,
        "img" | "source" => PageResourceType::Image,
        "video" | "audio" | "track" => PageResourceType::Media,
        "iframe" => PageResourceType::Iframe,
        "link" => {
            let rel = element
                .value()
                .attr("rel")
                .unwrap_or("")
                .to_ascii_lowercase();
            let as_attr = element.value().attr("as").unwrap_or("").to_ascii_lowercase();
            if rel.contains("stylesheet") {
                PageResourceType::Stylesheet
            } else if as_attr == "font" {
                PageResourceType::Font
            } else {
                PageResourceType::Other
            }
        }
        _ => PageResourceType::Other,
    }
}

fn extract_srcset_urls(srcset: &str) -> impl Iterator<Item = &str> {
    srcset.split(',').filter_map(|part| {
        let trimmed = part.trim();
        trimmed.split_whitespace().next()
    })
}

fn discover_page_resources(base: &url::Url, html: &str) -> Vec<DiscoveredResource> {
    let document = scraper::Html::parse_document(html);
    let mut seen = std::collections::HashSet::new();
    let mut resources = Vec::new();

    let selectors = [
        ("link[href]", "href"),
        ("script[src]", "src"),
        ("img[src],source[src]", "src"),
        ("img[srcset],source[srcset]", "srcset"),
        ("video[src],audio[src],track[src]", "src"),
        ("iframe[src]", "src"),
        ("embed[src]", "src"),
        ("object[data]", "data"),
    ];

    for (selector_str, attr) in selectors {
        let selector = match scraper::Selector::parse(selector_str) {
            Ok(s) => s,
            Err(_) => continue,
        };

        for element in document.select(&selector) {
            let attr_values: Vec<String> = if attr == "srcset" {
                element
                    .value()
                    .attr("srcset")
                    .map(|s| extract_srcset_urls(s).map(String::from).collect())
                    .unwrap_or_default()
            } else {
                element
                    .value()
                    .attr(attr)
                    .map(|s| vec![s.to_owned()])
                    .unwrap_or_default()
            };

            let resource_type = classify_resource(&element);

            for raw in attr_values {
                let raw = raw.trim();
                if raw.is_empty() || raw.starts_with("data:") || raw.starts_with("javascript:") {
                    continue;
                }
                let joined = match base.join(raw) {
                    Ok(u) => u,
                    Err(_) => continue,
                };
                if joined.scheme() != "http" && joined.scheme() != "https" {
                    continue;
                }
                if !seen.insert(joined.as_str().to_owned()) {
                    continue;
                }
                resources.push(DiscoveredResource {
                    url: joined,
                    resource_type: resource_type.clone(),
                });
            }
        }
    }

    resources
}

async fn fetch_page_resource(
    client: &reqwest::Client,
    discovered: DiscoveredResource,
    page_started: Instant,
) -> PageResource {
    let started = Instant::now();
    let mut bytes_received: u64 = 0;
    let mut time_to_first_byte_ms: Option<u64> = None;

    let request = client
        .get(discovered.url.as_str())
        .header("Cache-Control", "no-cache");

    let response = match request.send().await {
        Ok(r) => r,
        Err(e) => {
            let duration_ms = started.elapsed().as_millis() as u64;
            return PageResource {
                url: discovered.url.to_string(),
                resource_type: discovered.resource_type,
                status_code: None,
                content_length: None,
                bytes_received: 0,
                start_offset_ms: page_started.elapsed().as_millis() as u64,
                duration_ms,
                time_to_first_byte_ms: None,
                average_mbps: 0.0,
                error: Some(e.to_string()),
            };
        }
    };

    let status_code = response.status().as_u16();
    let content_length = response.content_length();
    let mut stream = response.bytes_stream();

    while let Some(chunk_result) = stream.next().await {
        match chunk_result {
            Ok(chunk) => {
                if time_to_first_byte_ms.is_none() {
                    time_to_first_byte_ms = Some(started.elapsed().as_millis() as u64);
                }
                bytes_received += chunk.len() as u64;
            }
            Err(e) => {
                let duration_ms = started.elapsed().as_millis() as u64;
                return PageResource {
                    url: discovered.url.to_string(),
                    resource_type: discovered.resource_type,
                    status_code: Some(status_code),
                    content_length,
                    bytes_received,
                    start_offset_ms: page_started.elapsed().as_millis() as u64,
                    duration_ms,
                    time_to_first_byte_ms,
                    average_mbps: mbps_from(bytes_received, started.elapsed()),
                    error: Some(e.to_string()),
                };
            }
        }
    }

    let duration = started.elapsed();
    let error = if status_code >= 400 {
        Some(format!("HTTP {status_code}"))
    } else {
        None
    };
    PageResource {
        url: discovered.url.to_string(),
        resource_type: discovered.resource_type,
        status_code: Some(status_code),
        content_length,
        bytes_received,
        start_offset_ms: page_started.elapsed().as_millis() as u64,
        duration_ms: duration.as_millis() as u64,
        time_to_first_byte_ms,
        average_mbps: mbps_from(bytes_received, duration),
        error,
    }
}

async fn fetch_document(
    client: &reqwest::Client,
    parsed: &url::Url,
    page_started: Instant,
) -> Result<(PageResource, String, url::Url), DownloadError> {
    let started = Instant::now();
    let mut bytes_received: u64 = 0;
    let mut time_to_first_byte_ms: Option<u64> = None;

    let request = client
        .get(parsed.as_str())
        .header("Cache-Control", "no-cache");

    let response = request
        .send()
        .await
        .map_err(|e| DownloadError::Request(e.to_string()))?;

    let status_code = response.status().as_u16();
    let final_url = response.url().clone();
    let content_length = response.content_length();

    let mut body = Vec::new();
    let mut stream = response.bytes_stream();

    while let Some(chunk_result) = stream.next().await {
        let chunk = chunk_result.map_err(|e| DownloadError::Request(e.to_string()))?;
        if time_to_first_byte_ms.is_none() {
            time_to_first_byte_ms = Some(started.elapsed().as_millis() as u64);
        }
        bytes_received += chunk.len() as u64;
        body.extend_from_slice(&chunk);
    }

    let duration = started.elapsed();
    let resource = PageResource {
        url: parsed.to_string(),
        resource_type: PageResourceType::Document,
        status_code: Some(status_code),
        content_length,
        bytes_received,
        start_offset_ms: page_started.elapsed().as_millis() as u64,
        duration_ms: duration.as_millis() as u64,
        time_to_first_byte_ms,
        average_mbps: mbps_from(bytes_received, duration),
        error: None,
    };

    let html = String::from_utf8_lossy(&body).to_string();
    Ok((resource, html, final_url))
}

/// Run a full-page speed test against `url` and stream progress via `on_progress`.
pub async fn run_page_speed_test<F>(
    url: &str,
    settings: HttpSettingsDto,
    on_progress: F,
) -> Result<PageSpeedResultDto, DownloadError>
where
    F: Fn(PageProgressEvent) + Send + Sync + 'static,
{
    let parsed = parse_url(url)?;
    let client = build_client(settings, "verkkokyyla/0.1.0 page-speed-test")?;

    let page_started = Instant::now();
    let mut last_progress = Instant::now();

    // Fetch and parse the main document.
    let (document_resource, html, final_url) = match fetch_document(&client, &parsed, page_started).await {
        Ok((res, html, final_url)) => (res, html, final_url),
        Err(e) => {
            let duration_ms = page_started.elapsed().as_millis() as u64;
            let resource = PageResource::failed(
                parsed.to_string(),
                PageResourceType::Document,
                e.to_string(),
            );
            return Ok(PageSpeedResultDto {
                url: url.to_owned(),
                final_url: parsed.to_string(),
                status_code: 0,
                total_resources: 1,
                successful_resources: 0,
                failed_resources: 1,
                total_content_length: None,
                total_bytes_received: 0,
                total_duration_ms: duration_ms,
                time_to_first_byte_ms: None,
                average_mbps: 0.0,
                resources: vec![resource],
                parse_error: None,
            });
        }
    };

    on_progress(PageProgressEvent {
        event: "progress",
        completed_resources: 1,
        total_resources: 1,
        bytes_received: document_resource.bytes_received,
        elapsed_ms: page_started.elapsed().as_millis() as u64,
        current_mbps: mbps_from(document_resource.bytes_received, page_started.elapsed()),
        resource: document_resource.clone(),
    });

    let base = url::Url::parse(final_url.as_str()).unwrap_or(parsed);
    let discovered = discover_page_resources(&base, &html);
    let total_resources = 1 + discovered.len() as u32;
    let mut completed_resources = 1u32;
    let mut successful_resources = 1u32;
    let mut failed_resources = 0u32;
    let mut total_bytes_received = document_resource.bytes_received;
    let mut total_content_length: Option<u64> = document_resource.content_length;

    let mut resources = vec![document_resource];

    let fetches = stream::iter(discovered)
        .map(|res| fetch_page_resource(&client, res, page_started))
        .buffer_unordered(MAX_CONCURRENT);

    futures_util::pin_mut!(fetches);

    while let Some(resource) = fetches.next().await {
        completed_resources += 1;
        total_bytes_received += resource.bytes_received;
        if let Some(len) = resource.content_length {
            total_content_length = total_content_length.map(|t| t + len);
        } else {
            total_content_length = None;
        }
        if resource.error.is_some() {
            failed_resources += 1;
        } else {
            successful_resources += 1;
        }
        resources.push(resource.clone());

        if last_progress.elapsed() >= Duration::from_millis(PROGRESS_THROTTLE_MS)
            || completed_resources == total_resources
        {
            on_progress(PageProgressEvent {
                event: "progress",
                completed_resources,
                total_resources,
                bytes_received: total_bytes_received,
                elapsed_ms: page_started.elapsed().as_millis() as u64,
                current_mbps: mbps_from(total_bytes_received, page_started.elapsed()),
                resource,
            });
            last_progress = Instant::now();
        }
    }

    let total_time = page_started.elapsed();
    let page_time_to_first_byte = resources[0].time_to_first_byte_ms;
    Ok(PageSpeedResultDto {
        url: url.to_owned(),
        final_url: final_url.to_string(),
        status_code: resources[0].status_code.unwrap_or(0),
        total_resources,
        successful_resources,
        failed_resources,
        total_content_length,
        total_bytes_received,
        total_duration_ms: total_time.as_millis() as u64,
        time_to_first_byte_ms: page_time_to_first_byte,
        average_mbps: mbps_from(total_bytes_received, total_time),
        resources,
        parse_error: None,
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::TcpListener;
    use tokio::time::sleep;

    use crate::http_client::{HttpSettingsDto, HttpVersion};

    fn default_settings() -> HttpSettingsDto {
        HttpSettingsDto {
            version: HttpVersion::Auto,
            connect_timeout_sec: 10,
            request_timeout_sec: 60,
            follow_redirects: true,
            max_redirects: 10,
            compression: true,
            user_agent: String::new(),
        }
    }

    fn http_response(status: &str, body: &[u8]) -> Vec<u8> {
        format!(
            "HTTP/1.1 {status}\r\nContent-Length: {}\r\nContent-Type: text/html\r\nConnection: close\r\n\r\n",
            body.len()
        )
        .into_bytes()
        .into_iter()
        .chain(body.iter().copied())
        .collect()
    }

    async fn start_server(
        handler: Arc<dyn Fn(&str) -> Vec<u8> + Send + Sync>,
    ) -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        tokio::spawn(async move {
            loop {
                let (stream, _) = match listener.accept().await {
                    Ok(conn) => conn,
                    Err(_) => break,
                };
                let handler = Arc::clone(&handler);
                tokio::spawn(async move {
                    let mut reader = BufReader::new(stream);
                    let mut request_line = String::new();
                    let _ = reader.read_line(&mut request_line).await;

                    let mut line = String::new();
                    loop {
                        line.clear();
                        if reader.read_line(&mut line).await.unwrap_or(0) == 0 {
                            break;
                        }
                        if line == "\r\n" || line == "\n" {
                            break;
                        }
                    }

                    let path = request_line
                        .split_whitespace()
                        .nth(1)
                        .unwrap_or("/")
                        .to_owned();

                    let response = handler(&path);
                    let mut stream = reader.into_inner();
                    let _ = stream.write_all(&response).await;
                    let _ = stream.shutdown().await;
                });
            }
        });

        sleep(Duration::from_millis(50)).await;
        port
    }

    #[tokio::test]
    async fn page_with_subresources_aggregates_stats() {
        let html = br#"<!doctype html>
<html>
<head><link rel="stylesheet" href="/style.css"></head>
<body><img src="/image.png"><script src="/app.js"></script></body>
</html>"#;
        let handler = Arc::new(move |path: &str| {
            let body: &[u8] = match path {
                "/" => html,
                "/style.css" => b"body { color: red }",
                "/image.png" => &[0u8; 512],
                "/app.js" => b"console.log('hi');",
                _ => b"not found",
            };
            let status = if path == "/" || path == "/style.css" || path == "/image.png" || path == "/app.js" {
                "200 OK"
            } else {
                "404 Not Found"
            };
            http_response(status, body)
        });
        let port = start_server(handler).await;
        let url = format!("http://127.0.0.1:{port}/");

        let progress_events = Arc::new(AtomicU32::new(0));
        let counter = Arc::clone(&progress_events);
        let result = run_page_speed_test(&url, default_settings(), move |_event| {
            counter.fetch_add(1, Ordering::SeqCst);
        })
        .await
        .unwrap();

        assert_eq!(result.total_resources, 4);
        assert_eq!(result.successful_resources, 4);
        assert_eq!(result.failed_resources, 0);
        assert!(result.total_bytes_received > html.len() as u64);
        assert!(result.average_mbps.is_finite());
        assert!(progress_events.load(Ordering::SeqCst) >= 1);

        let types: Vec<_> = result
            .resources
            .iter()
            .map(|r| r.resource_type.clone())
            .collect();
        assert!(types.contains(&PageResourceType::Document));
        assert!(types.contains(&PageResourceType::Stylesheet));
        assert!(types.contains(&PageResourceType::Image));
        assert!(types.contains(&PageResourceType::Script));
    }

    #[tokio::test]
    async fn duplicate_resources_are_deduplicated() {
        let html = br#"<html><script src="/a.js"></script><script src="/a.js"></script></html>"#;
        let handler = Arc::new(move |path: &str| {
            let body: &[u8] = match path {
                "/" => html,
                "/a.js" => b"// script",
                _ => b"not found",
            };
            http_response("200 OK", body)
        });
        let port = start_server(handler).await;
        let url = format!("http://127.0.0.1:{port}/");

        let result = run_page_speed_test(&url, default_settings(), |_event| {}).await.unwrap();
        assert_eq!(result.total_resources, 2);
        assert_eq!(result.resources.len(), 2);
    }

    #[tokio::test]
    async fn missing_subresource_is_reported_but_does_not_fail_run() {
        let html = br#"<html><link rel="stylesheet" href="/missing.css"></html>"#;
        let handler = Arc::new(move |path: &str| {
            let body: &[u8] = match path {
                "/" => html,
                _ => b"not found",
            };
            let status = if path == "/" { "200 OK" } else { "404 Not Found" };
            http_response(status, body)
        });
        let port = start_server(handler).await;
        let url = format!("http://127.0.0.1:{port}/");

        let result = run_page_speed_test(&url, default_settings(), |_event| {}).await.unwrap();
        assert_eq!(result.total_resources, 2);
        assert_eq!(result.successful_resources, 1);
        assert_eq!(result.failed_resources, 1);

        let missing = result
            .resources
            .iter()
            .find(|r| r.url.ends_with("/missing.css"))
            .unwrap();
        assert!(missing.error.is_some());
    }

    #[tokio::test]
    async fn relative_urls_resolve_against_document_url() {
        let html = br#"<html><img src="/pic.jpg"></html>"#;
        let handler = Arc::new(move |path: &str| {
            let body: &[u8] = match path {
                "/" => html,
                "/pic.jpg" => &[0u8; 64],
                _ => b"not found",
            };
            let status = if path == "/" || path == "/pic.jpg" {
                "200 OK"
            } else {
                "404 Not Found"
            };
            http_response(status, body)
        });
        let port = start_server(handler).await;
        let url = format!("http://127.0.0.1:{port}/");

        let result = run_page_speed_test(&url, default_settings(), |_event| {}).await.unwrap();
        assert_eq!(result.total_resources, 2);
        assert!(result
            .resources
            .iter()
            .any(|r| r.url == format!("http://127.0.0.1:{port}/pic.jpg")));
    }

    #[tokio::test]
    async fn invalid_url_fails_fast() {
        let err = run_page_speed_test("not-a-url", default_settings(), |_event| {})
            .await
            .unwrap_err();
        assert!(matches!(err, DownloadError::InvalidScheme));
    }
}
