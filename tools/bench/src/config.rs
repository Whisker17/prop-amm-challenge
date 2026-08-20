use std::collections::{HashMap, HashSet};
use std::path::Path;

use clap::Args;
use prop_amm_shared::config::{HyperparameterVariance, SimulationConfig};
use serde::Deserialize;

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
struct RawBenchConfig {
    #[serde(default)]
    segments: HashMap<String, RawSegment>,
    grid: Option<RawGridConfig>,
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

#[derive(Debug, Clone)]
pub struct BenchConfig {
    segments: HashMap<String, Segment>,
    grid: Option<GridConfig>,
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

        let grid = raw.grid.map(|g| validate_grid(&g)).transpose()?;

        Ok(Self { segments, grid })
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
        let grid_seed_base = 4_000_000_u64;
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
}
