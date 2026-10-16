//! Firmware version check from one read-only GET (`/api/core/firmware/info`).
//!
//! The firewall itself fetches and signature-verifies its release changelog
//! during its own update checks/cron. This app reads that cached list; it
//! never starts a firewall update check, downloads a changelog, or installs
//! anything. An empty or stale cache cannot prove that the firewall is current.

use semver::Version;

use crate::db::now_rfc3339;
use crate::opnsense::manager::OpnsenseManager;
use crate::opnsense::types::{
    OpnsenseChangelogEntryDto, OpnsenseManagerError, OpnsensePluginDto, OpnsenseVersionCheckDto,
};

/// Release series of an installed version: "26.7.6_3" → "26.7".
pub fn series_of(version: &str) -> Option<String> {
    let mut parts = version.trim().split('.');
    let major = parts.next()?;
    let minor = parts.next()?;
    if major.is_empty() || minor.is_empty() {
        return None;
    }
    Some(format!("{major}.{minor}"))
}

/// Parse a production OPNsense version. Package rebuild suffixes (`_3`)
/// do not change the upstream release version; RC/devel suffixes do not
/// count as production releases.
fn production_version(version: &str) -> Option<Version> {
    let trimmed = version.trim();
    let base = match trimmed.rsplit_once('_') {
        Some((base, revision))
            if !revision.is_empty() && revision.bytes().all(|b| b.is_ascii_digit()) =>
        {
            base
        }
        _ => trimmed,
    };
    let mut parts = base.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    match parts.next() {
        None => Some(Version::new(major, minor, 0)),
        Some(patch) if patch.chars().all(|c| c.is_ascii_digit()) => Version::parse(base).ok(),
        _ => None,
    }
}

#[derive(Clone, Debug, PartialEq)]
struct ChangelogEntry {
    version: String,
    date: Option<String>,
}

/// Latest production version in the cached changelog, and every entry
/// newer than `installed` (newest first).
fn evaluate(
    entries: Vec<ChangelogEntry>,
    installed: &Version,
) -> (Option<(Version, String)>, Vec<OpnsenseChangelogEntryDto>) {
    let mut ranked: Vec<(Version, ChangelogEntry)> = entries
        .into_iter()
        .filter_map(|entry| production_version(&entry.version).map(|version| (version, entry)))
        .collect();
    ranked.sort_by(|a, b| b.0.cmp(&a.0));
    let latest = ranked
        .first()
        .map(|(version, entry)| (version.clone(), entry.version.clone()));
    let newer = ranked
        .into_iter()
        .filter(|(version, _)| version > installed)
        .map(|(_, entry)| OpnsenseChangelogEntryDto {
            version: entry.version,
            date: entry.date,
        })
        .collect();
    (latest, newer)
}

#[derive(Clone)]
pub struct OpnsenseFirmwareManager {
    manager: OpnsenseManager,
}

impl OpnsenseFirmwareManager {
    pub fn new(manager: OpnsenseManager) -> Self {
        Self { manager }
    }

    /// Compare installed firmware with the firewall's cached release list.
    /// Missing cache/unsupported version → `state: "unknown"`, never a
    /// fabricated "up-to-date" result.
    pub async fn check(
        &self,
        profile_id: i64,
    ) -> Result<OpnsenseVersionCheckDto, OpnsenseManagerError> {
        let profile = self.manager.require_profile(profile_id).await?;
        let conn = self.manager.connection_for(&profile).await?;
        let api = (self.manager.api_factory())(conn).await?;
        let info = api.firmware_info().await?;

        let installed_version = info.product_version;
        let series = installed_version.as_deref().and_then(series_of);
        let plugins = info
            .plugins
            .into_iter()
            .map(|(name, version)| {
                let cached_upgrade_version = info
                    .cached_plugin_upgrades
                    .iter()
                    .find(|(plugin, _)| plugin == &name)
                    .map(|(_, version)| version.clone());
                OpnsensePluginDto {
                    name,
                    version,
                    cached_upgrade_version,
                }
            })
            .collect();

        let base = OpnsenseVersionCheckDto {
            state: "unknown".to_owned(),
            message: None,
            installed_version: installed_version.clone(),
            abi: info.abi,
            series: series.clone(),
            latest_version: None,
            checked_at: now_rfc3339(),
            changelog: Vec::new(),
            plugins,
        };

        let (Some(series), Some(installed)) = (
            series,
            installed_version.as_deref().and_then(production_version),
        ) else {
            return Ok(OpnsenseVersionCheckDto {
                message: Some(
                    "Installed firmware version missing or not a production release".to_owned(),
                ),
                ..base
            });
        };

        let entries = info
            .changelog
            .into_iter()
            .filter(|item| item.series.as_deref() == Some(&series))
            .map(|item| ChangelogEntry {
                version: item.version,
                date: item.date,
            })
            .collect();
        let (latest, newer) = evaluate(entries, &installed);
        let Some((latest_version, latest_name)) = latest else {
            return Ok(OpnsenseVersionCheckDto {
                message: Some(
                    "No cached release changelog for this series — run an update check in the OPNsense GUI"
                        .to_owned(),
                ),
                ..base
            });
        };
        Ok(OpnsenseVersionCheckDto {
            state: if installed >= latest_version {
                "up-to-date".to_owned()
            } else {
                "update-available".to_owned()
            },
            latest_version: Some(latest_name),
            changelog: newer,
            ..base
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(version: &str, date: Option<&str>) -> ChangelogEntry {
        ChangelogEntry {
            version: version.to_owned(),
            date: date.map(str::to_owned),
        }
    }

    #[test]
    fn series_derives_major_minor() {
        assert_eq!(series_of("25.7.3"), Some("25.7".to_owned()));
        assert_eq!(series_of("26.7.6_3"), Some("26.7".to_owned()));
        assert_eq!(series_of("junk"), None);
        assert_eq!(series_of(""), None);
    }

    #[test]
    fn production_versions_strip_pkg_revision_and_exclude_candidates() {
        assert_eq!(production_version("25.7"), Some(Version::new(25, 7, 0)));
        assert_eq!(production_version("26.7.6_3"), Some(Version::new(26, 7, 6)));
        assert_eq!(production_version("25.7.11"), Some(Version::new(25, 7, 11)));
        assert!(production_version("25.7.r1").is_none());
        assert!(production_version("26.1.b205").is_none());
        assert!(production_version("26.7.6_beta").is_none());
        assert!(production_version("").is_none());
    }

    #[test]
    fn evaluate_picks_latest_production_and_lists_newer() {
        let entries = vec![
            entry("26.7.5", None),
            entry("26.7.7", Some("2026-10-13")),
            entry("26.7.r1", None),
            entry("26.7.6", Some("2026-10-08")),
        ];
        let installed = production_version("26.7.6_3").unwrap();
        let (latest, newer) = evaluate(entries, &installed);
        assert_eq!(latest, Some((Version::new(26, 7, 7), "26.7.7".to_owned())));
        assert_eq!(newer.len(), 1);
        assert_eq!(newer[0].version, "26.7.7");
        assert_eq!(newer[0].date.as_deref(), Some("2026-10-13"));
    }
}
