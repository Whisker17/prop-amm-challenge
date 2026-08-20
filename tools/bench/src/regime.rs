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

fn tier_of(value: f64, min: f64, max: f64) -> Tier {
    let frac = (value - min) / (max - min);
    if frac < 1.0 / 3.0 {
        Tier::Low
    } else if frac < 2.0 / 3.0 {
        Tier::Mid
    } else {
        Tier::High
    }
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
