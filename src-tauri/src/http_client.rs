use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::download::DownloadError;

/// HTTP protocol version selected by the user.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HttpVersion {
    Auto,
    #[serde(rename = "http1.1")]
    Http1_1,
    #[serde(rename = "http2")]
    Http2,
}

/// User-configurable HTTP client settings passed from the frontend.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HttpSettingsDto {
    pub version: HttpVersion,
    pub connect_timeout_sec: u64,
    pub request_timeout_sec: u64,
    pub follow_redirects: bool,
    pub max_redirects: u32,
    pub compression: bool,
    pub user_agent: String,
}

impl HttpSettingsDto {
    /// Clamp inputs to sensible ranges so a bad IPC payload cannot cause a panic.
    fn sanitized(self) -> Self {
        Self {
            version: self.version,
            connect_timeout_sec: self.connect_timeout_sec.clamp(1, 300),
            request_timeout_sec: self.request_timeout_sec.clamp(1, 300),
            follow_redirects: self.follow_redirects,
            max_redirects: self.max_redirects.clamp(0, 100),
            compression: self.compression,
            user_agent: self.user_agent,
        }
    }
}

/// Build a `reqwest::Client` from user-supplied HTTP settings.
/// When `settings.user_agent` is empty, `default_user_agent` is used.
pub fn build_client(
    settings: HttpSettingsDto,
    default_user_agent: &str,
) -> Result<reqwest::Client, DownloadError> {
    let settings = settings.sanitized();

    let mut builder = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(settings.connect_timeout_sec))
        .timeout(Duration::from_secs(settings.request_timeout_sec));

    builder = match settings.version {
        HttpVersion::Auto => builder,
        HttpVersion::Http1_1 => builder.http1_only(),
        HttpVersion::Http2 => builder.http2_prior_knowledge(),
    };

    builder = if settings.follow_redirects {
        builder.redirect(reqwest::redirect::Policy::limited(settings.max_redirects as usize))
    } else {
        builder.redirect(reqwest::redirect::Policy::none())
    };

    if !settings.compression {
        builder = builder.gzip(false).brotli(false).deflate(false);
    }

    let user_agent = if settings.user_agent.is_empty() {
        default_user_agent
    } else {
        &settings.user_agent
    };
    builder = builder.user_agent(user_agent);

    builder
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
            follow_redirects: true,
            max_redirects: 10,
            compression: true,
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
