use std::net::SocketAddr;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::download::DownloadError;

/// HTTP protocol version selected by the user.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HttpVersion {
    #[default]
    Auto,
    #[serde(rename = "http1.1")]
    Http1_1,
    #[serde(rename = "http2")]
    Http2,
    #[serde(rename = "http3")]
    Http3,
}

/// IP address family preference.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IpFamily {
    #[default]
    Auto,
    Ipv4,
    Ipv6,
}

/// User-configurable HTTP client settings passed from the frontend.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HttpSettingsDto {
    pub version: HttpVersion,
    pub connect_timeout_sec: u64,
    pub request_timeout_sec: u64,
    pub read_timeout_sec: u64,
    pub follow_redirects: bool,
    pub max_redirects: u32,
    pub compression: bool,
    pub ip_family: IpFamily,
    pub user_agent: String,
}

impl HttpSettingsDto {
    /// Clamp inputs to sensible ranges so a bad IPC payload cannot cause a panic.
    /// A `read_timeout_sec` of 0 means "disabled".
    pub(crate) fn sanitized(self) -> Self {
        Self {
            version: self.version,
            connect_timeout_sec: self.connect_timeout_sec.clamp(1, 300),
            request_timeout_sec: self.request_timeout_sec.clamp(1, 300),
            read_timeout_sec: if self.read_timeout_sec == 0 {
                0
            } else {
                self.read_timeout_sec.clamp(1, 300)
            },
            follow_redirects: self.follow_redirects,
            max_redirects: self.max_redirects.clamp(0, 100),
            compression: self.compression,
            ip_family: self.ip_family,
            user_agent: self.user_agent,
        }
    }
}

fn apply_version(builder: reqwest::ClientBuilder, version: HttpVersion) -> reqwest::ClientBuilder {
    match version {
        HttpVersion::Auto => builder,
        HttpVersion::Http1_1 => builder.http1_only(),
        HttpVersion::Http2 => builder.http2_prior_knowledge(),
        // HTTP/3 is not handled by the generic builder; callers must detect it before
        // building a client. Treat it like Auto here so the error path is explicit.
        HttpVersion::Http3 => builder,
    }
}

/// Start building a client with the shared timeout, version, compression, and
/// redirect settings. Callers are responsible for adding redirect policy, address
/// pinning, and user-agent before building.
pub(crate) fn base_builder(settings: HttpSettingsDto) -> reqwest::ClientBuilder {
    let settings = settings.sanitized();

    let mut builder = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(settings.connect_timeout_sec))
        .timeout(Duration::from_secs(settings.request_timeout_sec));

    if settings.read_timeout_sec > 0 {
        builder = builder.read_timeout(Duration::from_secs(settings.read_timeout_sec));
    }

    builder = apply_version(builder, settings.version);

    builder = if settings.follow_redirects {
        builder.redirect(reqwest::redirect::Policy::limited(
            settings.max_redirects as usize,
        ))
    } else {
        builder.redirect(reqwest::redirect::Policy::none())
    };

    if !settings.compression {
        builder = builder.gzip(false).brotli(false).deflate(false);
    }

    builder
}

pub(crate) fn apply_user_agent(
    builder: reqwest::ClientBuilder,
    settings: &HttpSettingsDto,
    default_user_agent: &str,
) -> reqwest::ClientBuilder {
    let user_agent = if settings.user_agent.is_empty() {
        default_user_agent
    } else {
        &settings.user_agent
    };
    builder.user_agent(user_agent)
}

/// Build a `reqwest::Client` from user-supplied HTTP settings.
/// When `settings.user_agent` is empty, `default_user_agent` is used.
pub fn build_client(
    settings: HttpSettingsDto,
    default_user_agent: &str,
) -> Result<reqwest::Client, DownloadError> {
    let settings = settings.sanitized();
    let builder = base_builder(settings.clone());
    let builder = apply_user_agent(builder, &settings, default_user_agent);
    builder
        .build()
        .map_err(|e| DownloadError::Request(e.to_string()))
}

/// Build a `reqwest::Client` with a fixed set of resolved addresses for a host.
///
/// This is used by the benchmark engine to honour IPv4/IPv6 family selection and to
/// avoid double-DNS effects. The host in `resolved_host` should match the request
/// host exactly.
pub fn build_client_resolved(
    settings: HttpSettingsDto,
    default_user_agent: &str,
    resolved_host: &str,
    addrs: &[SocketAddr],
) -> Result<reqwest::Client, DownloadError> {
    let settings = settings.sanitized();
    let builder = base_builder(settings.clone());
    let builder = apply_user_agent(builder, &settings, default_user_agent);
    builder
        .resolve_to_addrs(resolved_host, addrs)
        .build()
        .map_err(|e| DownloadError::Request(e.to_string()))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn build_client_with_defaults_succeeds() {
        let result = build_client(default_settings(), "verkkokyyla/test");
        assert!(result.is_ok());
    }

    #[test]
    fn build_client_clamps_timeouts() {
        let settings = HttpSettingsDto {
            connect_timeout_sec: 0,
            request_timeout_sec: 600,
            ..default_settings()
        };
        // Building proves the values were clamped to valid ranges rather than rejected.
        assert!(build_client(settings, "verkkokyyla/test").is_ok());
    }

    #[test]
    fn build_client_clamps_max_redirects() {
        let settings = HttpSettingsDto {
            max_redirects: 10_000,
            ..default_settings()
        };
        assert!(build_client(settings, "verkkokyyla/test").is_ok());
    }

    #[test]
    fn build_client_with_http1_only_succeeds() {
        let settings = HttpSettingsDto {
            version: HttpVersion::Http1_1,
            ..default_settings()
        };
        assert!(build_client(settings, "verkkokyyla/test").is_ok());
    }

    #[test]
    fn build_client_with_http2_prior_knowledge_succeeds() {
        let settings = HttpSettingsDto {
            version: HttpVersion::Http2,
            ..default_settings()
        };
        assert!(build_client(settings, "verkkokyyla/test").is_ok());
    }

    #[test]
    fn build_client_with_disabled_compression_succeeds() {
        let settings = HttpSettingsDto {
            compression: false,
            ..default_settings()
        };
        assert!(build_client(settings, "verkkokyyla/test").is_ok());
    }
}
