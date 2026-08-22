use std::collections::{HashMap, HashSet};
use std::path::Path;

use clap::Args;
use prop_amm_shared::config::{HyperparameterVariance, SimulationConfig};
use serde::Deserialize;

use crate::search::validate_budget;

/// The checked-in config every command loads by default. `config/README.md`'s per-machine
/// override (`<name>.local.toml`, gitignored) takes precedence when present.
pub const DEFAULT_CONFIG_PATH: &str = "config/bench.toml";
const LOCAL_CONFIG_PATH: &str = "config/bench.local.toml";

fn default_stride() -> u64 {
    1
}

// `deny_unknown_fields` on both: a typo'd field name (`singel_use`) must fail loading, not
// silently fall back to a permissive default (`config/README.md`'s fail-fast rule) — that
// exact typo would otherwise leave `single_use` at its default `false` and make a
// single-use segment spendable with no `--i-am-spending-the-test-segment` flag at all.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
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

// Grid mode's axis levels and per-cell seed count (docs/DESIGN.md §2.3) are "ours,
// protocol-level" per §3.1's own bucket list ("seed segments, search budget, **grid
// levels**, sim counts") — they belong here, not as Rust consts, so a future change to the
// factorial is a config edit plus a §2 update, not a recompile. `deny_unknown_fields` for
// the same fail-fast reason as `RawSegment`. The grid's own *seed addressing* (the base
// offset and the `base + cell*1_000 + i` formula) stays in `grid.rs` — this file's own
// header comment already carves grid seeds out as "not a segment", and the formula itself
// is code, not a tunable value.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawGridConfig {
    norm_fee_bps_levels: Vec<u16>,
    norm_liquidity_mult_levels: Vec<f64>,
    gbm_sigma_levels: Vec<f64>,
    seeds_per_cell: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSearchConfig {
    max_points: usize,
}

// `bench fuzz`'s own sample counts (WHI-1212, docs/DESIGN.md §2.9's fidelity contract) —
// "ours, protocol-level" for the same reason grid's own axis levels are: a future change to
// how hard the gate hammers a candidate is a config edit, not a recompile.
// `deny_unknown_fields` for the same fail-fast reason as `RawSegment`/`RawGridConfig`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawFuzzConfig {
    dense_sweep_points: usize,
    seeds_per_regime: u64,
    golden_price_multipliers: Vec<f64>,
    moderate_max_input: f64,
    golden_max_iters: usize,
}

// `bench estimator-probe`'s own pre-registered kill-condition thresholds and floor sweep
// (WHI-1225, docs/DESIGN.md §3.1) — "ours, protocol-level" for the same reason `[fuzz]`'s own
// sample counts are: a future change to either threshold, or to the floor sweep, is a config
// edit, not a recompile. `deny_unknown_fields` for the same fail-fast reason as the other Raw*
// structs.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawEstimatorProbeConfig {
    low_sigma_count_ratio_kill_threshold: f64,
    decompression_kill_threshold_pct: f64,
    floor_bps_sweep: Vec<u64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawBenchConfig {
    #[serde(default)]
    segments: HashMap<String, RawSegment>,
    #[serde(default)]
    search: Option<RawSearchConfig>,
    grid: Option<RawGridConfig>,
    fuzz: Option<RawFuzzConfig>,
    estimator_probe: Option<RawEstimatorProbeConfig>,
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

/// Grid mode's factorial axes and per-cell seed count (docs/DESIGN.md §2.3), loaded from
/// `config/bench.toml`'s `[grid]` table.
#[derive(Debug, Clone)]
pub struct GridConfig {
    pub norm_fee_bps_levels: Vec<u16>,
    pub norm_liquidity_mult_levels: Vec<f64>,
    pub gbm_sigma_levels: Vec<f64>,
    pub seeds_per_cell: u64,
}

/// `bench fuzz`'s sample counts (WHI-1212), loaded from `config/bench.toml`'s `[fuzz]`
/// table.
#[derive(Debug, Clone)]
pub struct FuzzConfig {
    pub dense_sweep_points: usize,
    pub seeds_per_regime: u64,
    pub golden_price_multipliers: Vec<f64>,
    pub moderate_max_input: f64,
    pub golden_max_iters: usize,
}

/// `bench estimator-probe`'s kill-condition thresholds and floor sweep (WHI-1225), loaded
/// from `config/bench.toml`'s `[estimator_probe]` table.
#[derive(Debug, Clone)]
pub struct EstimatorProbeConfig {
    pub low_sigma_count_ratio_kill_threshold: f64,
    pub decompression_kill_threshold_pct: f64,
    pub floor_bps_sweep: Vec<u64>,
}

#[derive(Debug, Clone)]
pub struct BenchConfig {
    segments: HashMap<String, Segment>,
    search_max_points: usize,
    grid: Option<GridConfig>,
    fuzz: Option<FuzzConfig>,
    estimator_probe: Option<EstimatorProbeConfig>,
}

impl BenchConfig {
    /// Loads `config/bench.local.toml` if it exists, else `config/bench.toml`
    /// (`config/README.md`'s per-machine-override convention).
    pub fn load_default() -> anyhow::Result<Self> {
        let local = Path::new(LOCAL_CONFIG_PATH);
        if local.exists() {
            Self::load(local)
        } else {
            Self::load(Path::new(DEFAULT_CONFIG_PATH))
        }
    }

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
            // Fail at load time if this segment's own `seeds()` would overflow u64 —
            // better than a silent wraparound the first time someone actually runs it.
            (seg.count - 1)
                .checked_mul(seg.stride)
                .and_then(|max_offset| seg.start.checked_add(max_offset))
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "segment `{name}` overflows u64 seed arithmetic \
                         (start {} + (count {} - 1) * stride {})",
                        seg.start,
                        seg.count,
                        seg.stride
                    )
                })?;
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

        let search = raw
            .search
            .ok_or_else(|| anyhow::anyhow!("bench config declares no [search] section"))?;
        validate_budget(search.max_points, "[search] max_points")?;

        let grid = raw.grid.map(|g| validate_grid(&g)).transpose()?;
        let fuzz = raw.fuzz.map(|f| validate_fuzz(&f)).transpose()?;
        let estimator_probe = raw
            .estimator_probe
            .map(|e| validate_estimator_probe(&e))
            .transpose()?;

        Ok(Self {
            segments,
            search_max_points: search.max_points,
            grid,
            fuzz,
            estimator_probe,
        })
    }

    /// The evaluation-point budget a search may spend (docs/DESIGN.md §2.5), as configured
    /// in `config/bench.toml`'s `[search]` section — always `1..=MAX_SEARCH_POINTS`, enforced
    /// at load time in `parse`.
    pub fn search_max_points(&self) -> usize {
        self.search_max_points
    }

    pub fn segment(&self, name: &str) -> anyhow::Result<&Segment> {
        self.segments.get(name).ok_or_else(|| {
            anyhow::anyhow!(
                "unknown segment `{name}`; known segments: {}",
                self.segment_names()
            )
        })
    }

    /// Grid mode's config, if `config/bench.toml` declares a `[grid]` table. Fails only when
    /// a caller actually needs it (grid mode itself) — `compare`/`anchor`/`l1` don't, and
    /// shouldn't have to satisfy a requirement they never touch.
    pub fn grid(&self) -> anyhow::Result<&GridConfig> {
        self.grid
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("bench config declares no [grid] table"))
    }

    /// `bench fuzz`'s config, if `config/bench.toml` declares a `[fuzz]` table. Fails only
    /// when a caller actually needs it (`bench fuzz` itself), same rationale as `grid()`.
    pub fn fuzz(&self) -> anyhow::Result<&FuzzConfig> {
        self.fuzz
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("bench config declares no [fuzz] table"))
    }

    /// `bench estimator-probe`'s config, if `config/bench.toml` declares an
    /// `[estimator_probe]` table. Fails only when a caller actually needs it (`bench
    /// estimator-probe` itself), same rationale as `grid()`/`fuzz()`.
    pub fn estimator_probe(&self) -> anyhow::Result<&EstimatorProbeConfig> {
        self.estimator_probe
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("bench config declares no [estimator_probe] table"))
    }

    fn segment_names(&self) -> String {
        let mut names: Vec<&str> = self.segments.keys().map(String::as_str).collect();
        names.sort_unstable();
        names.join(", ")
    }
}

/// The `--segment`/`--i-am-spending-the-test-segment` pair every segment-taking subcommand
/// needs. `#[command(flatten)]` this into a command's own `Args` struct so the single-use
/// guard (docs/DESIGN.md §2.2) is enforced identically everywhere `--segment` exists,
/// including subcommands WHI-1194/1195 add later — never a per-command reimplementation.
#[derive(Args, Debug)]
pub struct SegmentSelector {
    /// Seed segment to evaluate on (train|validation|test|observation, per config/bench.toml).
    #[arg(long, default_value = "observation")]
    pub segment: String,
    /// Required to select a single-use segment (the test segment).
    #[arg(long)]
    pub i_am_spending_the_test_segment: bool,
}

impl SegmentSelector {
    /// Resolves `--segment` against `config` and enforces the single-use guard. Returns the
    /// segment name and the segment itself.
    pub fn resolve<'s, 'c>(
        &'s self,
        config: &'c BenchConfig,
    ) -> anyhow::Result<(&'s str, &'c Segment)> {
        let segment = config.segment(&self.segment)?;

        if segment.single_use && !self.i_am_spending_the_test_segment {
            anyhow::bail!(
                "segment `{}` is single-use; pass --i-am-spending-the-test-segment to spend it",
                self.segment
            );
        }

        Ok((self.segment.as_str(), segment))
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

fn validate_grid(raw: &RawGridConfig) -> anyhow::Result<GridConfig> {
    if raw.norm_fee_bps_levels.is_empty() {
        anyhow::bail!("grid config's norm_fee_bps_levels is empty");
    }
    if raw.norm_liquidity_mult_levels.is_empty() {
        anyhow::bail!("grid config's norm_liquidity_mult_levels is empty");
    }
    if raw.gbm_sigma_levels.is_empty() {
        anyhow::bail!("grid config's gbm_sigma_levels is empty");
    }
    if raw.seeds_per_cell == 0 {
        anyhow::bail!("grid config's seeds_per_cell is 0");
    }

    Ok(GridConfig {
        norm_fee_bps_levels: raw.norm_fee_bps_levels.clone(),
        norm_liquidity_mult_levels: raw.norm_liquidity_mult_levels.clone(),
        gbm_sigma_levels: raw.gbm_sigma_levels.clone(),
        seeds_per_cell: raw.seeds_per_cell,
    })
}

fn validate_fuzz(raw: &RawFuzzConfig) -> anyhow::Result<FuzzConfig> {
    if raw.dense_sweep_points < 2 {
        anyhow::bail!("fuzz config's dense_sweep_points must be at least 2");
    }
    if raw.seeds_per_regime == 0 {
        anyhow::bail!("fuzz config's seeds_per_regime is 0");
    }
    if raw.golden_price_multipliers.is_empty() {
        anyhow::bail!("fuzz config's golden_price_multipliers is empty");
    }
    if raw
        .golden_price_multipliers
        .iter()
        .any(|m| *m <= 0.0 || !m.is_finite())
    {
        anyhow::bail!("fuzz config's golden_price_multipliers must all be finite and positive");
    }
    if !raw.moderate_max_input.is_finite() || raw.moderate_max_input <= 0.0 {
        anyhow::bail!("fuzz config's moderate_max_input must be finite and positive");
    }
    // A value too close to `MIN_INPUT` collapses every dense-sweep grid (`linear_grid` etc.
    // in `fuzz.rs` clamp their span to a tiny epsilon) to effectively one point instead of a
    // real sweep — fail loudly here (`AGENTS.md` § Runtime configuration's fail-fast rule)
    // rather than let the gate silently probe almost nothing.
    if raw.moderate_max_input <= crate::fuzz::MIN_INPUT * 10.0 {
        anyhow::bail!(
            "fuzz config's moderate_max_input ({}) is too close to the minimum tradable \
             input ({}) to produce a real sweep",
            raw.moderate_max_input,
            crate::fuzz::MIN_INPUT,
        );
    }
    if raw.golden_max_iters == 0 {
        anyhow::bail!("fuzz config's golden_max_iters is 0");
    }

    Ok(FuzzConfig {
        dense_sweep_points: raw.dense_sweep_points,
        seeds_per_regime: raw.seeds_per_regime,
        moderate_max_input: raw.moderate_max_input,
        golden_max_iters: raw.golden_max_iters,
        golden_price_multipliers: raw.golden_price_multipliers.clone(),
    })
}

fn validate_estimator_probe(raw: &RawEstimatorProbeConfig) -> anyhow::Result<EstimatorProbeConfig> {
    if !raw.low_sigma_count_ratio_kill_threshold.is_finite()
        || !(0.0..=1.0).contains(&raw.low_sigma_count_ratio_kill_threshold)
    {
        anyhow::bail!(
            "estimator_probe config's low_sigma_count_ratio_kill_threshold must be in [0, 1]"
        );
    }
    if !raw.decompression_kill_threshold_pct.is_finite()
        || raw.decompression_kill_threshold_pct < 0.0
    {
        anyhow::bail!(
            "estimator_probe config's decompression_kill_threshold_pct must be finite and \
             non-negative"
        );
    }
    if raw.floor_bps_sweep.is_empty() {
        anyhow::bail!("estimator_probe config's floor_bps_sweep is empty");
    }
    let mut sorted = raw.floor_bps_sweep.clone();
    sorted.sort_unstable();
    if sorted != raw.floor_bps_sweep {
        anyhow::bail!("estimator_probe config's floor_bps_sweep must be sorted ascending");
    }

    Ok(EstimatorProbeConfig {
        low_sigma_count_ratio_kill_threshold: raw.low_sigma_count_ratio_kill_threshold,
        decompression_kill_threshold_pct: raw.decompression_kill_threshold_pct,
        floor_bps_sweep: raw.floor_bps_sweep.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::MAX_SEARCH_POINTS;

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

[search]
max_points = 300
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
    fn seed_arithmetic_overflow_fails_at_load() {
        // TOML integers are i64-range, so u64::MAX itself isn't a valid literal — use
        // i64::MAX for both start and stride instead, which is representable but still
        // overflows u64 seed arithmetic once count*stride is added on top.
        let text = r#"
[segments.a]
start = 9223372036854775807
count = 3
stride = 9223372036854775807
"#;
        let err = BenchConfig::parse(text).unwrap_err();
        assert!(
            err.to_string().contains("overflows u64 seed arithmetic"),
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

    #[test]
    fn unknown_field_is_rejected() {
        let text = r#"
[segments.a]
start = 0
count = 5
singel_use = true
"#;
        assert!(BenchConfig::parse(text).is_err());
    }

    /// Loads the actual shipped `config/bench.toml` (not a synthetic string) so a wrong
    /// `start`/`count`/flag in the real file fails `cargo test`, not just a manual run.
    #[test]
    fn shipped_config_loads_and_matches_design_doc_segmentation() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let cfg = BenchConfig::load(&repo_root.join(DEFAULT_CONFIG_PATH)).unwrap();

        let observation = cfg.segment("observation").unwrap();
        assert_eq!(observation.start, 0);
        assert_eq!(observation.count, 1000);
        assert!(!observation.single_use);
        assert!(!observation.decision_input);

        let train = cfg.segment("train").unwrap();
        assert_eq!(train.start, 1_000_000);
        assert_eq!(train.count, 1000);

        let screening = cfg.segment("screening").unwrap();
        assert_eq!(screening.start, 1_000_000);
        assert_eq!(screening.count, 200);
        assert_eq!(screening.subset_of.as_deref(), Some("train"));

        let validation = cfg.segment("validation").unwrap();
        assert_eq!(validation.start, 2_000_000);
        assert_eq!(validation.count, 1000);

        let test = cfg.segment("test").unwrap();
        assert_eq!(test.start, 3_000_000);
        assert_eq!(test.count, 1000);
        assert!(test.single_use);

        assert_eq!(cfg.search_max_points(), 300);
    }

    #[test]
    fn missing_search_section_fails() {
        let text = r#"
[segments.a]
start = 0
count = 5
"#;
        let err = BenchConfig::parse(text).unwrap_err();
        assert!(
            err.to_string().contains("no [search] section"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn zero_max_points_fails() {
        let text = r#"
[segments.a]
start = 0
count = 5

[search]
max_points = 0
"#;
        let err = BenchConfig::parse(text).unwrap_err();
        assert!(
            err.to_string().contains("at least 1"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn max_points_above_the_protocol_cap_fails() {
        let text = format!(
            r#"
[segments.a]
start = 0
count = 5

[search]
max_points = {}
"#,
            MAX_SEARCH_POINTS + 1
        );
        let err = BenchConfig::parse(&text).unwrap_err();
        assert!(
            err.to_string().contains("exceeds the protocol's hard cap"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn max_points_at_the_protocol_cap_is_allowed() {
        let text = format!(
            r#"
[segments.a]
start = 0
count = 5

[search]
max_points = {MAX_SEARCH_POINTS}
"#
        );
        let cfg = BenchConfig::parse(&text).unwrap();
        assert_eq!(cfg.search_max_points(), MAX_SEARCH_POINTS);
    }

    /// Loads the actual shipped `config/bench.toml`'s `[grid]` table and checks it against
    /// docs/DESIGN.md §2.3's own axis table, plus that grid mode's seed range (computed from
    /// the loaded config, not a hardcoded copy of it) stays clear of every declared segment.
    #[test]
    fn shipped_config_declares_grid_matching_design_doc_axes() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let cfg = BenchConfig::load(&repo_root.join(DEFAULT_CONFIG_PATH)).unwrap();
        let grid = cfg.grid().unwrap();

        assert_eq!(grid.norm_fee_bps_levels, vec![30, 55, 80]);
        assert_eq!(grid.norm_liquidity_mult_levels, vec![0.4, 1.0, 2.0]);
        assert_eq!(grid.gbm_sigma_levels, vec![1e-4, 1e-3, 7e-3]);
        assert_eq!(grid.seeds_per_cell, 40);

        let n_cells = grid.norm_fee_bps_levels.len()
            * grid.norm_liquidity_mult_levels.len()
            * grid.gbm_sigma_levels.len();
        // Imported, not re-typed: a `grid.rs` change to the seed base must fail this test
        // too, not just silently widen (or shrink) the range it's checking.
        let grid_seed_base = crate::grid::GRID_SEED_BASE;
        let grid_max_seed =
            grid_seed_base + (n_cells as u64 - 1) * 1_000 + (grid.seeds_per_cell - 1);

        for name in ["observation", "train", "screening", "validation", "test"] {
            let segment = cfg.segment(name).unwrap();
            let segment_max_seed = segment.start + (segment.count - 1) * segment.stride;
            assert!(
                grid_seed_base > segment_max_seed || segment.start > grid_max_seed,
                "grid's seed range [{grid_seed_base}, {grid_max_seed}] must not overlap \
                 segment `{name}`'s [{}, {segment_max_seed}]",
                segment.start,
            );
        }
    }

    #[test]
    fn grid_accessor_fails_when_no_grid_table_declared() {
        let cfg = BenchConfig::parse(sample_valid()).unwrap();
        let err = cfg.grid().unwrap_err();
        assert!(
            err.to_string().contains("no [grid] table"),
            "unexpected error: {err}"
        );
    }

    /// Loads the actual shipped `config/bench.toml`'s `[fuzz]` table (WHI-1212).
    #[test]
    fn shipped_config_declares_fuzz_table() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let cfg = BenchConfig::load(&repo_root.join(DEFAULT_CONFIG_PATH)).unwrap();
        let fuzz = cfg.fuzz().unwrap();

        assert!(fuzz.dense_sweep_points >= 2);
        assert!(fuzz.seeds_per_regime >= 1);
        assert!(!fuzz.golden_price_multipliers.is_empty());
        assert!(fuzz
            .golden_price_multipliers
            .iter()
            .all(|m| m.is_finite() && *m > 0.0));
        assert!(fuzz.moderate_max_input.is_finite() && fuzz.moderate_max_input > 0.0);
        assert!(fuzz.golden_max_iters >= 1);
    }

    #[test]
    fn fuzz_accessor_fails_when_no_fuzz_table_declared() {
        let cfg = BenchConfig::parse(sample_valid()).unwrap();
        let err = cfg.fuzz().unwrap_err();
        assert!(
            err.to_string().contains("no [fuzz] table"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn fuzz_table_parses_and_validates() {
        let text = format!(
            "{}\n[fuzz]\ndense_sweep_points = 200\nseeds_per_regime = 3\ngolden_price_multipliers = [0.5, 1.0, 2.0]\nmoderate_max_input = 50000.0\ngolden_max_iters = 20\n",
            sample_valid()
        );
        let cfg = BenchConfig::parse(&text).unwrap();
        let fuzz = cfg.fuzz().unwrap();
        assert_eq!(fuzz.dense_sweep_points, 200);
        assert_eq!(fuzz.seeds_per_regime, 3);
        assert_eq!(fuzz.golden_price_multipliers, vec![0.5, 1.0, 2.0]);
        assert_eq!(fuzz.moderate_max_input, 50_000.0);
        assert_eq!(fuzz.golden_max_iters, 20);
    }

    #[test]
    fn fuzz_table_rejects_zero_golden_max_iters() {
        let text = format!(
            "{}\n[fuzz]\ndense_sweep_points = 200\nseeds_per_regime = 3\ngolden_price_multipliers = [1.0]\nmoderate_max_input = 50000.0\ngolden_max_iters = 0\n",
            sample_valid()
        );
        let err = BenchConfig::parse(&text).unwrap_err();
        assert!(
            err.to_string().contains("golden_max_iters is 0"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn fuzz_table_rejects_too_few_dense_sweep_points() {
        let text = format!(
            "{}\n[fuzz]\ndense_sweep_points = 1\nseeds_per_regime = 3\ngolden_price_multipliers = [1.0]\nmoderate_max_input = 50000.0\ngolden_max_iters = 20\n",
            sample_valid()
        );
        let err = BenchConfig::parse(&text).unwrap_err();
        assert!(
            err.to_string().contains("dense_sweep_points"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn fuzz_table_rejects_zero_seeds_per_regime() {
        let text = format!(
            "{}\n[fuzz]\ndense_sweep_points = 200\nseeds_per_regime = 0\ngolden_price_multipliers = [1.0]\nmoderate_max_input = 50000.0\ngolden_max_iters = 20\n",
            sample_valid()
        );
        let err = BenchConfig::parse(&text).unwrap_err();
        assert!(
            err.to_string().contains("seeds_per_regime is 0"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn fuzz_table_rejects_empty_golden_price_multipliers() {
        let text = format!(
            "{}\n[fuzz]\ndense_sweep_points = 200\nseeds_per_regime = 3\ngolden_price_multipliers = []\nmoderate_max_input = 50000.0\ngolden_max_iters = 20\n",
            sample_valid()
        );
        let err = BenchConfig::parse(&text).unwrap_err();
        assert!(
            err.to_string()
                .contains("golden_price_multipliers is empty"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn fuzz_table_rejects_non_positive_golden_price_multiplier() {
        let text = format!(
            "{}\n[fuzz]\ndense_sweep_points = 200\nseeds_per_regime = 3\ngolden_price_multipliers = [1.0, 0.0]\nmoderate_max_input = 50000.0\ngolden_max_iters = 20\n",
            sample_valid()
        );
        let err = BenchConfig::parse(&text).unwrap_err();
        assert!(
            err.to_string().contains("finite and positive"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn fuzz_table_rejects_zero_moderate_max_input() {
        let text = format!(
            "{}\n[fuzz]\ndense_sweep_points = 200\nseeds_per_regime = 3\ngolden_price_multipliers = [1.0]\nmoderate_max_input = 0.0\ngolden_max_iters = 20\n",
            sample_valid()
        );
        let err = BenchConfig::parse(&text).unwrap_err();
        assert!(
            err.to_string()
                .contains("moderate_max_input must be finite and positive"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn fuzz_table_rejects_a_moderate_max_input_too_close_to_min_input() {
        let text = format!(
            "{}\n[fuzz]\ndense_sweep_points = 200\nseeds_per_regime = 3\ngolden_price_multipliers = [1.0]\nmoderate_max_input = 0.001\ngolden_max_iters = 20\n",
            sample_valid()
        );
        let err = BenchConfig::parse(&text).unwrap_err();
        assert!(
            err.to_string()
                .contains("too close to the minimum tradable input"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn fuzz_table_rejects_missing_moderate_max_input() {
        let text = format!(
            "{}\n[fuzz]\ndense_sweep_points = 200\nseeds_per_regime = 3\ngolden_price_multipliers = [1.0]\n",
            sample_valid()
        );
        assert!(BenchConfig::parse(&text).is_err());
    }

    #[test]
    fn grid_table_parses_and_validates() {
        let text = format!(
            "{}\n[grid]\nnorm_fee_bps_levels = [30, 55, 80]\nnorm_liquidity_mult_levels = [0.4, 1.0, 2.0]\ngbm_sigma_levels = [0.0001, 0.001, 0.007]\nseeds_per_cell = 40\n",
            sample_valid()
        );
        let cfg = BenchConfig::parse(&text).unwrap();
        let grid = cfg.grid().unwrap();
        assert_eq!(grid.norm_fee_bps_levels, vec![30, 55, 80]);
        assert_eq!(grid.seeds_per_cell, 40);
    }

    #[test]
    fn grid_table_rejects_empty_levels() {
        let text = format!(
            "{}\n[grid]\nnorm_fee_bps_levels = []\nnorm_liquidity_mult_levels = [1.0]\ngbm_sigma_levels = [0.001]\nseeds_per_cell = 40\n",
            sample_valid()
        );
        let err = BenchConfig::parse(&text).unwrap_err();
        assert!(
            err.to_string().contains("norm_fee_bps_levels is empty"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn grid_table_rejects_zero_seeds_per_cell() {
        let text = format!(
            "{}\n[grid]\nnorm_fee_bps_levels = [30]\nnorm_liquidity_mult_levels = [1.0]\ngbm_sigma_levels = [0.001]\nseeds_per_cell = 0\n",
            sample_valid()
        );
        let err = BenchConfig::parse(&text).unwrap_err();
        assert!(
            err.to_string().contains("seeds_per_cell is 0"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn segment_selector_blocks_single_use_without_spend_flag() {
        let cfg = BenchConfig::parse(sample_valid()).unwrap();
        let selector = SegmentSelector {
            segment: "test".to_string(),
            i_am_spending_the_test_segment: false,
        };
        let err = selector.resolve(&cfg).unwrap_err();
        assert!(
            err.to_string().contains("single-use"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn segment_selector_allows_single_use_with_spend_flag() {
        let cfg = BenchConfig::parse(sample_valid()).unwrap();
        let selector = SegmentSelector {
            segment: "test".to_string(),
            i_am_spending_the_test_segment: true,
        };
        assert!(selector.resolve(&cfg).is_ok());
    }

    #[test]
    fn segment_selector_allows_non_single_use_without_spend_flag() {
        let cfg = BenchConfig::parse(sample_valid()).unwrap();
        let selector = SegmentSelector {
            segment: "observation".to_string(),
            i_am_spending_the_test_segment: false,
        };
        assert!(selector.resolve(&cfg).is_ok());
    }

    #[test]
    fn segment_selector_unknown_segment_fails() {
        let cfg = BenchConfig::parse(sample_valid()).unwrap();
        let selector = SegmentSelector {
            segment: "nope".to_string(),
            i_am_spending_the_test_segment: false,
        };
        assert!(selector.resolve(&cfg).is_err());
    }

    /// Loads the actual shipped `config/bench.toml`'s `[estimator_probe]` table (WHI-1225).
    #[test]
    fn shipped_config_declares_estimator_probe_table() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let cfg = BenchConfig::load(&repo_root.join(DEFAULT_CONFIG_PATH)).unwrap();
        let probe = cfg.estimator_probe().unwrap();

        assert_eq!(probe.low_sigma_count_ratio_kill_threshold, 0.9);
        assert_eq!(probe.decompression_kill_threshold_pct, 15.0);
        assert_eq!(probe.floor_bps_sweep, vec![0, 10, 20, 25, 30, 40, 60]);
    }

    #[test]
    fn estimator_probe_accessor_fails_when_no_table_declared() {
        let cfg = BenchConfig::parse(sample_valid()).unwrap();
        let err = cfg.estimator_probe().unwrap_err();
        assert!(
            err.to_string().contains("no [estimator_probe] table"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn estimator_probe_table_parses_and_validates() {
        let text = format!(
            "{}\n[estimator_probe]\nlow_sigma_count_ratio_kill_threshold = 0.9\ndecompression_kill_threshold_pct = 15.0\nfloor_bps_sweep = [0, 25, 60]\n",
            sample_valid()
        );
        let cfg = BenchConfig::parse(&text).unwrap();
        let probe = cfg.estimator_probe().unwrap();
        assert_eq!(probe.low_sigma_count_ratio_kill_threshold, 0.9);
        assert_eq!(probe.decompression_kill_threshold_pct, 15.0);
        assert_eq!(probe.floor_bps_sweep, vec![0, 25, 60]);
    }

    #[test]
    fn estimator_probe_table_rejects_a_ratio_threshold_outside_zero_one() {
        let text = format!(
            "{}\n[estimator_probe]\nlow_sigma_count_ratio_kill_threshold = 1.5\ndecompression_kill_threshold_pct = 15.0\nfloor_bps_sweep = [0]\n",
            sample_valid()
        );
        let err = BenchConfig::parse(&text).unwrap_err();
        assert!(
            err.to_string()
                .contains("low_sigma_count_ratio_kill_threshold"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn estimator_probe_table_rejects_a_negative_decompression_threshold() {
        let text = format!(
            "{}\n[estimator_probe]\nlow_sigma_count_ratio_kill_threshold = 0.9\ndecompression_kill_threshold_pct = -1.0\nfloor_bps_sweep = [0]\n",
            sample_valid()
        );
        let err = BenchConfig::parse(&text).unwrap_err();
        assert!(
            err.to_string().contains("decompression_kill_threshold_pct"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn estimator_probe_table_rejects_an_empty_floor_sweep() {
        let text = format!(
            "{}\n[estimator_probe]\nlow_sigma_count_ratio_kill_threshold = 0.9\ndecompression_kill_threshold_pct = 15.0\nfloor_bps_sweep = []\n",
            sample_valid()
        );
        let err = BenchConfig::parse(&text).unwrap_err();
        assert!(
            err.to_string().contains("floor_bps_sweep is empty"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn estimator_probe_table_rejects_an_unsorted_floor_sweep() {
        let text = format!(
            "{}\n[estimator_probe]\nlow_sigma_count_ratio_kill_threshold = 0.9\ndecompression_kill_threshold_pct = 15.0\nfloor_bps_sweep = [10, 0, 30]\n",
            sample_valid()
        );
        let err = BenchConfig::parse(&text).unwrap_err();
        assert!(
            err.to_string().contains("sorted ascending"),
            "unexpected error: {err}"
        );
    }
}
