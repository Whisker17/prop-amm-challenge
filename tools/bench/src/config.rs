use std::collections::{HashMap, HashSet};
use std::path::Path;

use prop_amm_shared::config::{HyperparameterVariance, SimulationConfig};
use serde::Deserialize;

fn default_stride() -> u64 {
    1
}

#[derive(Debug, Clone, Deserialize)]
struct RawSegment {
    start: u64,
    count: u64,
    #[serde(default = "default_stride")]
    stride: u64,
    #[serde(default)]
    single_use: bool,
    #[serde(default)]
    decision_input: bool,
    subset_of: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct RawBenchConfig {
    #[serde(default)]
    segments: HashMap<String, RawSegment>,
}

/// A named, contiguous-stride block of seeds, plus the protocol flags that govern how it
/// may be used (`docs/DESIGN.md` §2.2, §3.1).
#[derive(Debug, Clone)]
pub struct Segment {
    pub start: u64,
    pub count: u64,
    pub stride: u64,
    pub single_use: bool,
    pub decision_input: bool,
    pub subset_of: Option<String>,
}

impl Segment {
    pub fn seeds(&self) -> Vec<u64> {
        (0..self.count)
            .map(|i| self.start + i * self.stride)
            .collect()
    }

    /// Distribution mode (docs/DESIGN.md §2.3): each seed's full regime via
    /// `HyperparameterVariance::apply`, over `base` with the caller's step count.
    pub fn sim_configs(&self, base: &SimulationConfig) -> Vec<SimulationConfig> {
        let variance = HyperparameterVariance::default();
        self.seeds()
            .into_iter()
            .map(|seed| variance.apply(base, seed))
            .collect()
    }
}

#[derive(Debug, Clone)]
pub struct BenchConfig {
    segments: HashMap<String, Segment>,
}

impl BenchConfig {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let text = std::fs::read_to_string(path).map_err(|e| {
            anyhow::anyhow!("failed to read bench config {}: {}", path.display(), e)
        })?;
        Self::parse(&text)
    }

    pub fn parse(text: &str) -> anyhow::Result<Self> {
        let raw: RawBenchConfig = toml::from_str(text)
            .map_err(|e| anyhow::anyhow!("failed to parse bench config: {e}"))?;

        if raw.segments.is_empty() {
            anyhow::bail!("bench config declares no segments");
        }

        let mut segments = HashMap::with_capacity(raw.segments.len());
        for (name, seg) in &raw.segments {
            if seg.count == 0 {
                anyhow::bail!("segment `{name}` has count = 0");
            }
            if seg.stride == 0 {
                anyhow::bail!("segment `{name}` has stride = 0");
            }
            if let Some(parent) = &seg.subset_of {
                if !raw.segments.contains_key(parent) {
                    anyhow::bail!(
                        "segment `{name}` declares subset_of `{parent}`, which is not a declared segment"
                    );
                }
            }
            segments.insert(
                name.clone(),
                Segment {
                    start: seg.start,
                    count: seg.count,
                    stride: seg.stride,
                    single_use: seg.single_use,
                    decision_input: seg.decision_input,
                    subset_of: seg.subset_of.clone(),
                },
            );
        }

        validate_disjoint(&segments)?;

        Ok(Self { segments })
    }

    pub fn segment(&self, name: &str) -> anyhow::Result<&Segment> {
        self.segments.get(name).ok_or_else(|| {
            anyhow::anyhow!(
                "unknown segment `{name}`; known segments: {}",
                self.segment_names()
            )
        })
    }

    fn segment_names(&self) -> String {
        let mut names: Vec<&str> = self.segments.keys().map(String::as_str).collect();
        names.sort_unstable();
        names.join(", ")
    }
}

/// Every pair of segments must have disjoint seed sets, except a pair where one declares
/// `subset_of` the other — that declared relationship is checked, not merely trusted, so a
/// stale or wrong `subset_of` still fails loudly rather than silently widening the exemption.
fn validate_disjoint(segments: &HashMap<String, Segment>) -> anyhow::Result<()> {
    let seed_sets: HashMap<&str, HashSet<u64>> = segments
        .iter()
        .map(|(name, seg)| (name.as_str(), seg.seeds().into_iter().collect()))
        .collect();

    let mut names: Vec<&str> = segments.keys().map(String::as_str).collect();
    names.sort_unstable();

    for (i, a) in names.iter().enumerate() {
        for b in &names[i + 1..] {
            let a_declares_b = segments[*a].subset_of.as_deref() == Some(b);
            let b_declares_a = segments[*b].subset_of.as_deref() == Some(a);

            if a_declares_b || b_declares_a {
                let (child, parent) = if a_declares_b { (a, b) } else { (b, a) };
                if !seed_sets[*child].is_subset(&seed_sets[*parent]) {
                    anyhow::bail!(
                        "segment `{child}` declares subset_of `{parent}`, but its seeds are not \
                         actually a subset of `{parent}`'s"
                    );
                }
                continue;
            }

            if !seed_sets[*a].is_disjoint(&seed_sets[*b]) {
                anyhow::bail!(
                    "segments `{a}` and `{b}` overlap; segments must be disjoint unless one \
                     declares subset_of the other"
                );
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_valid() -> &'static str {
        r#"
[segments.observation]
start = 0
count = 10

[segments.train]
start = 1000
count = 10

[segments.screening]
start = 1000
count = 3
subset_of = "train"

[segments.validation]
start = 2000
count = 10

[segments.test]
start = 3000
count = 10
single_use = true
decision_input = true
"#
    }

    #[test]
    fn loads_valid_config() {
        let cfg = BenchConfig::parse(sample_valid()).unwrap();
        assert_eq!(
            cfg.segment("observation").unwrap().seeds(),
            (0..10).collect::<Vec<_>>()
        );
        assert!(cfg.segment("test").unwrap().single_use);
        assert!(!cfg.segment("observation").unwrap().single_use);
        assert!(!cfg.segment("observation").unwrap().decision_input);
        assert!(cfg.segment("test").unwrap().decision_input);
    }

    #[test]
    fn screening_is_exempt_as_declared_subset_of_train() {
        let cfg = BenchConfig::parse(sample_valid()).unwrap();
        assert_eq!(
            cfg.segment("screening").unwrap().seeds(),
            vec![1000, 1001, 1002]
        );
    }

    #[test]
    fn overlapping_segments_without_subset_of_fail() {
        let text = r#"
[segments.a]
start = 0
count = 10

[segments.b]
start = 5
count = 10
"#;
        let err = BenchConfig::parse(text).unwrap_err();
        assert!(
            err.to_string().contains("overlap"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn declared_subset_that_is_not_actually_a_subset_fails() {
        let text = r#"
[segments.train]
start = 1000
count = 10

[segments.screening]
start = 999
count = 5
subset_of = "train"
"#;
        let err = BenchConfig::parse(text).unwrap_err();
        assert!(
            err.to_string().contains("not actually a subset"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn unknown_subset_of_target_fails() {
        let text = r#"
[segments.screening]
start = 0
count = 5
subset_of = "train"
"#;
        let err = BenchConfig::parse(text).unwrap_err();
        assert!(
            err.to_string().contains("not a declared segment"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn zero_count_fails() {
        let text = r#"
[segments.a]
start = 0
count = 0
"#;
        let err = BenchConfig::parse(text).unwrap_err();
        assert!(
            err.to_string().contains("count = 0"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn zero_stride_fails() {
        let text = r#"
[segments.a]
start = 0
count = 5
stride = 0
"#;
        let err = BenchConfig::parse(text).unwrap_err();
        assert!(
            err.to_string().contains("stride = 0"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn unknown_segment_lookup_fails() {
        let cfg = BenchConfig::parse(sample_valid()).unwrap();
        assert!(cfg.segment("nope").is_err());
    }

    #[test]
    fn sim_configs_matches_hyperparameter_variance_directly() {
        let cfg = BenchConfig::parse(sample_valid()).unwrap();
        let segment = cfg.segment("observation").unwrap();
        let base = SimulationConfig::default();
        let configs = segment.sim_configs(&base);

        assert_eq!(configs.len(), segment.seeds().len());
        let expected = HyperparameterVariance::default().apply(&base, 3);
        assert_eq!(configs[3].seed, expected.seed);
        assert!((configs[3].gbm_sigma - expected.gbm_sigma).abs() < 1e-15);
    }

    #[test]
    fn empty_config_fails() {
        let err = BenchConfig::parse("").unwrap_err();
        assert!(
            err.to_string().contains("no segments"),
            "unexpected error: {err}"
        );
    }
}
