use prop_amm_shared::config::{HyperparameterVariance, SimulationConfig};
use prop_amm_shared::result::SimResult;

use crate::stats::{self, PairedStat};

/// One axis's position within its sampling range, split into equal-width thirds. This is a
/// bench-level reporting choice, not a `docs/DESIGN.md`-specified boundary: the design doc
/// fixes grid mode's three *representative* levels per axis (§2.3) but leaves regime
/// slicing's bin edges to the implementation. Equal-width thirds of each axis's own
/// `HyperparameterVariance` range is the natural, documented extension of that same 3-level
/// structure to a full partition (every sampled value lands in exactly one bin).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Tier {
    Low,
    Mid,
    High,
}

// WHI-1249: named once so `tier_of`'s classification and `tier_bounds`'s displayed
// numeric ranges cannot silently drift apart into two different partitions of the same
// `[min, max]` — both read these same two cutoff fractions instead of each hand-writing
// its own `1.0 / 3.0`/`2.0 / 3.0` literal.
const LOW_MID_CUTOFF_FRAC: f64 = 1.0 / 3.0;
const MID_HIGH_CUTOFF_FRAC: f64 = 2.0 / 3.0;

fn tier_of(value: f64, min: f64, max: f64) -> Tier {
    let frac = (value - min) / (max - min);
    if frac < LOW_MID_CUTOFF_FRAC {
        Tier::Low
    } else if frac < MID_HIGH_CUTOFF_FRAC {
        Tier::Mid
    } else {
        Tier::High
    }
}

/// WHI-1249: the `[lo, hi)` bounds `tier` actually covers, computed the same way `tier_of`
/// computes its fractional cutoffs — equal linear thirds of `[min, max]`. Display-only: it
/// does not feed back into `tier_of`/`classify_seed`, which remain the sole classification
/// logic. Exists because a caller (`tools/bench/src/commands/ceiling.rs`'s sigma-slice
/// report) previously labelled each tier with the nearest `config/bench.toml`
/// `[grid] gbm_sigma_levels` entry under the false assumption that the three grid levels
/// land one-per-tier; they do not (e.g. the sigma grid's `0.0010` falls in `Low`, not `Mid`,
/// for the default `gbm_sigma` range) — this reports the tier's own true numeric range
/// instead, which is correct by construction for any `[min, max]`.
pub fn tier_bounds(tier: Tier, min: f64, max: f64) -> (f64, f64) {
    let range = max - min;
    let low_mid = min + LOW_MID_CUTOFF_FRAC * range;
    let mid_high = min + MID_HIGH_CUTOFF_FRAC * range;
    match tier {
        Tier::Low => (min, low_mid),
        Tier::Mid => (low_mid, mid_high),
        Tier::High => (mid_high, max),
    }
}

/// WHI-1249: the single source of the sigma axis's default sampling range. `classify_seed`
/// (below) needs the *whole* `HyperparameterVariance` to reconstruct a sampled config, so it
/// cannot call this directly — but any caller that only wants the sigma bounds `tier_bounds`
/// partitions (e.g. `tools/bench/src/commands/ceiling.rs`'s sigma-slice report) should read
/// them from here rather than constructing its own `HyperparameterVariance::default()`, so the
/// range a label is computed over can't drift from the range `classify_seed` actually samples.
pub fn default_sigma_range() -> (f64, f64) {
    let variance = HyperparameterVariance::default();
    (variance.gbm_sigma_min, variance.gbm_sigma_max)
}

/// A simulation's regime, reconstructed from its sampled config (docs/DESIGN.md §2.3's three
/// grid axes) rather than the full continuous value — coarse enough to slice a report by,
/// fine enough to separate "cheap, thin, volatile" corners from calm ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Regime {
    pub fee: Tier,
    pub liquidity: Tier,
    pub sigma: Tier,
}

impl Regime {
    pub fn label(&self) -> String {
        format!(
            "fee={:?} liq={:?} sigma={:?}",
            self.fee, self.liquidity, self.sigma
        )
    }
}

fn classify(config: &SimulationConfig, variance: &HyperparameterVariance) -> Regime {
    Regime {
        fee: tier_of(
            config.norm_fee_bps as f64,
            variance.norm_fee_bps_min as f64,
            variance.norm_fee_bps_max as f64,
        ),
        liquidity: tier_of(
            config.norm_liquidity_mult,
            variance.norm_liquidity_mult_min,
            variance.norm_liquidity_mult_max,
        ),
        sigma: tier_of(
            config.gbm_sigma,
            variance.gbm_sigma_min,
            variance.gbm_sigma_max,
        ),
    }
}

/// Reconstructs `seed`'s regime the same way the ticket specifies: re-derive the sampled
/// config via `HyperparameterVariance::apply(base, seed)`. `SimResult` only carries
/// `(seed, submission_edge)` (`crates/shared/src/result.rs`), not the config that produced
/// it, so this is the only way back to "what regime was this simulation actually run under".
pub fn classify_seed(base: &SimulationConfig, seed: u64) -> Regime {
    let variance = HyperparameterVariance::default();
    let sampled = variance.apply(base, seed);
    classify(&sampled, &variance)
}

/// Groups `candidate`/`reference` (paired by index, same as `stats::paired_stat` expects) by
/// each seed's reconstructed regime, and computes a paired statistic per populated bin
/// (docs/DESIGN.md §2.3, §5: "report per-bin paired differences beside the headline"). Bins
/// are returned sorted by `Regime` for a stable report order.
pub fn slice_paired_stats(
    base: &SimulationConfig,
    candidate: &[SimResult],
    reference: &[SimResult],
) -> anyhow::Result<Vec<(Regime, PairedStat)>> {
    if candidate.len() != reference.len() {
        anyhow::bail!(
            "regime slicing requires equal-length batches: candidate={} reference={}",
            candidate.len(),
            reference.len()
        );
    }

    let mut bins: std::collections::BTreeMap<Regime, (Vec<SimResult>, Vec<SimResult>)> =
        std::collections::BTreeMap::new();

    for (i, (c, r)) in candidate.iter().zip(reference.iter()).enumerate() {
        if c.seed != r.seed {
            anyhow::bail!(
                "regime slicing unpaired at index {i}: candidate seed {} vs reference seed {}",
                c.seed,
                r.seed
            );
        }
        let regime = classify_seed(base, c.seed);
        let entry = bins.entry(regime).or_default();
        entry.0.push(c.clone());
        entry.1.push(r.clone());
    }

    bins.into_iter()
        .map(|(regime, (c, r))| Ok((regime, stats::paired_stat(&c, &r)?)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sim(seed: u64, edge: f64) -> SimResult {
        SimResult {
            seed,
            submission_edge: edge,
        }
    }

    #[test]
    fn tier_of_splits_range_into_equal_thirds() {
        assert_eq!(tier_of(0.0, 0.0, 90.0), Tier::Low);
        assert_eq!(tier_of(29.0, 0.0, 90.0), Tier::Low);
        assert_eq!(tier_of(30.0, 0.0, 90.0), Tier::Mid);
        assert_eq!(tier_of(59.0, 0.0, 90.0), Tier::Mid);
        assert_eq!(tier_of(60.0, 0.0, 90.0), Tier::High);
        assert_eq!(tier_of(89.999, 0.0, 90.0), Tier::High);
    }

    #[test]
    fn tier_bounds_matches_tier_of_at_the_default_sigma_range() {
        // WHI-1249: the regression this guards is exactly the one the audit found — a
        // caller assuming the three `config/bench.toml` `[grid] gbm_sigma_levels`
        // (0.0001, 0.0010, 0.0070) land one-per-tier against the default
        // `HyperparameterVariance` sigma range. `0.0010` must classify as `Low`, not
        // `Mid`, and `tier_bounds(Mid, ..)` must not contain it.
        let min = 0.0001_f64;
        let max = 0.007_f64;
        assert_eq!(tier_of(0.0010, min, max), Tier::Low);

        let (low_lo, low_hi) = tier_bounds(Tier::Low, min, max);
        let (mid_lo, mid_hi) = tier_bounds(Tier::Mid, min, max);
        let (high_lo, high_hi) = tier_bounds(Tier::High, min, max);

        assert!((low_lo - 0.0001).abs() < 1e-12);
        assert!((low_hi - 0.0024).abs() < 1e-9);
        assert!((mid_lo - 0.0024).abs() < 1e-9);
        assert!((mid_hi - 0.0047).abs() < 1e-9);
        assert!((high_lo - 0.0047).abs() < 1e-9);
        assert!((high_hi - 0.007).abs() < 1e-12);

        // The grid level 0.0010 falls inside tier_bounds(Low, ..), not tier_bounds(Mid, ..).
        assert!(low_lo <= 0.0010 && 0.0010 < low_hi);
        assert!(!(mid_lo <= 0.0010 && 0.0010 < mid_hi));

        // tier_of and tier_bounds must agree for every boundary-adjacent sample.
        for value in [0.0001, 0.0023, 0.0024, 0.0035, 0.0046, 0.0047, 0.006999] {
            let tier = tier_of(value, min, max);
            let (lo, hi) = tier_bounds(tier, min, max);
            assert!(
                lo <= value && value < hi,
                "value {value} classified as {tier:?} but its own bounds are [{lo}, {hi})"
            );
        }
    }

    #[test]
    fn every_seed_lands_in_exactly_one_bin_and_pooling_reproduces_the_headline_mean() {
        let base = SimulationConfig::default();
        let seeds: Vec<u64> = (1..=50).collect();
        // diffs = seed as f64, so the headline mean is trivially the mean of 1..=50.
        let candidate: Vec<SimResult> = seeds.iter().map(|&s| sim(s, s as f64)).collect();
        let reference: Vec<SimResult> = seeds.iter().map(|&s| sim(s, 0.0)).collect();

        let headline = stats::paired_stat(&candidate, &reference).unwrap();
        let sliced = slice_paired_stats(&base, &candidate, &reference).unwrap();

        let total_n: usize = sliced.iter().map(|(_, stat)| stat.n).sum();
        assert_eq!(
            total_n,
            seeds.len(),
            "every seed must land in exactly one bin"
        );

        let pooled_sum: f64 = sliced
            .iter()
            .map(|(_, stat)| stat.mean_diff * stat.n as f64)
            .sum();
        let pooled_mean = pooled_sum / total_n as f64;
        assert!(
            (pooled_mean - headline.mean_diff).abs() < 1e-9,
            "pooled mean {pooled_mean} should reproduce the headline mean {}",
            headline.mean_diff
        );
    }

    #[test]
    fn mismatched_seeds_error() {
        let base = SimulationConfig::default();
        let candidate = vec![sim(1, 1.0)];
        let reference = vec![sim(2, 1.0)];
        let err = slice_paired_stats(&base, &candidate, &reference).unwrap_err();
        assert!(
            err.to_string().contains("unpaired"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn mismatched_lengths_error() {
        let base = SimulationConfig::default();
        let candidate = vec![sim(1, 1.0), sim(2, 1.0)];
        let reference = vec![sim(1, 1.0)];
        let err = slice_paired_stats(&base, &candidate, &reference).unwrap_err();
        assert!(
            err.to_string().contains("equal-length"),
            "unexpected error: {err}"
        );
    }
}
