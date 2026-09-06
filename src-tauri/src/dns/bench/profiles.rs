use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::dns::bench::mixes::QueryMix;
use crate::dns::query::RecordTypeSpec;

/// A reusable benchmark profile.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BenchmarkProfile {
    pub name: String,
    pub concurrency: usize,
    pub qps_limit: Option<usize>,
    pub duration_seconds: u64,
    pub query_name: String,
    pub record_type: RecordTypeSpec,
    pub mix: QueryMix,
    pub cache_bust: bool,
}

impl BenchmarkProfile {
    /// Total offered queries for this profile.
    pub fn total_queries(&self) -> usize {
        match self.qps_limit {
            Some(qps) => qps * self.duration_seconds as usize,
            None => self.duration_seconds as usize * 100,
        }
    }

    pub fn interval(&self) -> Option<Duration> {
        self.qps_limit.map(|qps| Duration::from_secs_f64(1.0 / qps.max(1) as f64))
    }

    pub fn preset_quick(query_name: impl Into<String>) -> Self {
        Self {
            name: "Quick probe".to_string(),
            concurrency: 4,
            qps_limit: Some(10),
            duration_seconds: 5,
            query_name: query_name.into(),
            record_type: RecordTypeSpec::A,
            mix: QueryMix::UniqueLabels {
                base: "mock.test".to_string(),
            },
            cache_bust: true,
        }
    }

    pub fn preset_stress(query_name: impl Into<String>) -> Self {
        Self {
            name: "Stress".to_string(),
            concurrency: 32,
            qps_limit: Some(100),
            duration_seconds: 20,
            query_name: query_name.into(),
            record_type: RecordTypeSpec::A,
            mix: QueryMix::PopularWeighted,
            cache_bust: false,
        }
    }

    pub fn preset_cache_bust(query_name: impl Into<String>) -> Self {
        Self {
            name: "Cache-bust".to_string(),
            concurrency: 8,
            qps_limit: Some(50),
            duration_seconds: 10,
            query_name: query_name.into(),
            record_type: RecordTypeSpec::A,
            mix: QueryMix::UniqueLabels {
                base: "mock.test".to_string(),
            },
            cache_bust: true,
        }
    }
}
