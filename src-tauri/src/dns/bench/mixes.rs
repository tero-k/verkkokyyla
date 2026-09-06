use rand::Rng;
use rand::distributions::{Distribution, WeightedIndex};
use rand::rngs::StdRng;
use serde::{Deserialize, Serialize};

const TOP1000_TXT: &str = include_str!("top1000.txt");

/// Source of query names for a benchmark profile.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum QueryMix {
    /// Weighted random draw from an embedded top-1000 domain list.
    PopularWeighted,
    /// Unique random labels under a base domain.
    UniqueLabels { base: String },
    /// A blend of popular and unique-label draws.
    Mixed { ratio: f64 },
}

impl QueryMix {
    /// Generate the next query name. The caller supplies two seeded RNGs so that
    /// deterministic tests can drive the same mix source.
    pub fn next_name(&mut self, popular_rng: &mut StdRng, unique_rng: &mut StdRng) -> String {
        match self {
            Self::PopularWeighted => popular_name(popular_rng),
            Self::UniqueLabels { base } => unique_name(base, unique_rng),
            Self::Mixed { ratio } => {
                if popular_rng.gen::<f64>() < *ratio {
                    popular_name(popular_rng)
                } else {
                    unique_name("mock.test", unique_rng)
                }
            }
        }
    }

    /// Total number of domains in the embedded popular list.
    pub fn popular_count() -> usize {
        popular_domains().len()
    }
}

fn popular_domains() -> Vec<&'static str> {
    TOP1000_TXT
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect()
}

fn popular_name(rng: &mut StdRng) -> String {
    let list = popular_domains();
    let n = list.len();
    let dist = WeightedIndex::new((1..=n).rev()).expect("non-empty list");
    list[dist.sample(rng)].to_owned()
}

fn unique_name(base: &str, rng: &mut StdRng) -> String {
    let dist = rand::distributions::Uniform::new_inclusive(b'a', b'z');
    let label: String = dist
        .sample_iter(rng)
        .take(12)
        .map(|b| b as char)
        .collect();
    format!("{label}.{base}")
}

#[cfg(test)]
mod tests {
    use super::*;

    use rand::SeedableRng;

    #[test]
    fn embedded_list_has_exactly_1000_entries() {
        assert_eq!(QueryMix::popular_count(), 1000);
    }

    #[test]
    fn mixed_ratio_is_deterministic_with_seed() {
        let mut popular_rng = StdRng::seed_from_u64(1);
        let mut unique_rng = StdRng::seed_from_u64(2);
        let mut mix = QueryMix::Mixed { ratio: 0.5 };

        let mut popular = 0;
        let mut unique = 0;
        for _ in 0..200 {
            let name = mix.next_name(&mut popular_rng, &mut unique_rng);
            if name.ends_with(".mock.test") && name.len() > 12 {
                unique += 1;
            } else {
                popular += 1;
            }
        }

        assert!(popular >= 80 && popular <= 120, "expected ~100 popular, got {popular}");
        assert!(unique >= 80 && unique <= 120, "expected ~100 unique, got {unique}");
    }
}
