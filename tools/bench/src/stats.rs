use prop_amm_shared::result::SimResult;

/// Two-tailed 95% critical t-values for df = 1..=29 (index 0 -> df 1). The protocol's real
/// segments are 1,000 sims (df=999), always past the normal-approximation threshold below;
/// this table exists so a small custom segment isn't silently over-confident about its
/// interval (docs/DESIGN.md §2.2's "t critical value" note).
const T_TABLE_DF_1_TO_29: [f64; 29] = [
    12.706, 4.303, 3.182, 2.776, 2.571, 2.447, 2.365, 2.306, 2.262, 2.228, 2.201, 2.179, 2.160,
    2.145, 2.131, 2.120, 2.110, 2.101, 2.093, 2.086, 2.080, 2.074, 2.069, 2.064, 2.060, 2.056,
    2.052, 2.048, 2.045,
];

const NORMAL_APPROX_Z: f64 = 1.959964;

fn t_critical(df: usize) -> f64 {
    match df {
        0 => f64::NAN,
        1..=29 => T_TABLE_DF_1_TO_29[df - 1],
        _ => NORMAL_APPROX_Z,
    }
}

/// The paired-by-seed comparison of a candidate against a reference (docs/DESIGN.md §2.1):
/// mean difference, standard error, and a t-interval, computed from per-seed edges rather
/// than the two aggregate `avg_edge` values.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PairedStat {
    pub n: usize,
    pub mean_diff: f64,
    pub std_error: f64,
    /// NaN when `n < 2` — a single-seed sample cannot estimate a variance, so the interval
    /// collapses to the point estimate (`ci_low == ci_high == mean_diff`) rather than lying
    /// about a confidence level it can't support.
    pub t_critical: f64,
    pub ci_low: f64,
    pub ci_high: f64,
}

/// Computes the paired statistic between two equal-length, seed-aligned batches. Both must
/// come from the *same* `Vec<SimulationConfig>` run against candidate and reference in turn
/// (docs/DESIGN.md §2.6) — a seed mismatch at any index means that pairing broke, which is a
/// correctness bug this function refuses to paper over.
pub fn paired_stat(candidate: &[SimResult], reference: &[SimResult]) -> anyhow::Result<PairedStat> {
    if candidate.len() != reference.len() {
        anyhow::bail!(
            "paired comparison requires equal-length batches: candidate={} reference={}",
            candidate.len(),
            reference.len()
        );
    }
    let n = candidate.len();
    if n == 0 {
        anyhow::bail!("paired comparison requires at least one seed");
    }

    let diffs: Vec<f64> = candidate
        .iter()
        .zip(reference.iter())
        .enumerate()
        .map(|(i, (c, r))| -> anyhow::Result<f64> {
            if c.seed != r.seed {
                anyhow::bail!(
                    "paired comparison unpaired at index {i}: candidate seed {} vs reference seed {}",
                    c.seed,
                    r.seed
                );
            }
            Ok(c.submission_edge - r.submission_edge)
        })
        .collect::<anyhow::Result<Vec<f64>>>()?;

    let mean_diff = diffs.iter().sum::<f64>() / n as f64;

    let (std_error, df) = if n >= 2 {
        let variance = diffs.iter().map(|d| (d - mean_diff).powi(2)).sum::<f64>() / (n - 1) as f64;
        (variance.sqrt() / (n as f64).sqrt(), n - 1)
    } else {
        (0.0, 0)
    };

    let t = t_critical(df);
    let margin = if df == 0 { 0.0 } else { t * std_error };

    Ok(PairedStat {
        n,
        mean_diff,
        std_error,
        t_critical: t,
        ci_low: mean_diff - margin,
        ci_high: mean_diff + margin,
    })
}

/// The median of `values` (average of the two middle elements on an even-length input) —
/// used by WHI-1225's Probe A kill rule ("median `count / n_steps > 0.9` on the low-sigma
/// tercile"), which the mean would answer a different, more outlier-sensitive question than
/// the issue's own wording asks. Does not mutate `values`; sorts a local copy.
pub fn median(values: &[f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).expect("median input must not contain NaN"));
    let n = sorted.len();
    Some(if n % 2 == 1 {
        sorted[n / 2]
    } else {
        (sorted[n / 2 - 1] + sorted[n / 2]) / 2.0
    })
}

/// Average (fractional) rank of each value in `values`, tied values sharing the mean of the
/// ranks they'd occupy — the standard tie-handling `spearman_rank_correlation` needs so equal
/// `sigma_hat` values (a real possibility at the estimator's integer-`isqrt` resolution) don't
/// bias the correlation from an arbitrary tie-break order.
fn fractional_ranks(values: &[f64]) -> Vec<f64> {
    let mut indexed: Vec<(usize, f64)> = values.iter().copied().enumerate().collect();
    indexed.sort_by(|a, b| {
        a.1.partial_cmp(&b.1)
            .expect("ranks input must not contain NaN")
    });

    let mut ranks = vec![0.0; values.len()];
    let mut i = 0;
    while i < indexed.len() {
        let mut j = i;
        while j + 1 < indexed.len() && indexed[j + 1].1 == indexed[i].1 {
            j += 1;
        }
        // Ranks are 1-based; positions i..=j (0-based) occupy ranks (i+1)..=(j+1).
        let avg_rank = ((i + 1) + (j + 1)) as f64 / 2.0;
        for slot in indexed.iter().take(j + 1).skip(i) {
            ranks[slot.0] = avg_rank;
        }
        i = j + 1;
    }
    ranks
}

/// Spearman's rank correlation between two equal-length, paired series — used by WHI-1225's
/// Probe A kill (ii) to compare `sigma_hat_old`/`sigma_hat_new` against `true_sigma` without
/// assuming either estimator is linear in the truth (only monotone). Returns `None` for
/// fewer than 2 points or when either series has zero rank variance (a constant series has no
/// defined correlation, not a `0.0` one).
pub fn spearman_rank_correlation(a: &[f64], b: &[f64]) -> Option<f64> {
    if a.len() != b.len() || a.len() < 2 {
        return None;
    }
    let ranks_a = fractional_ranks(a);
    let ranks_b = fractional_ranks(b);
    let n = ranks_a.len() as f64;

    let mean_a = ranks_a.iter().sum::<f64>() / n;
    let mean_b = ranks_b.iter().sum::<f64>() / n;

    let mut cov = 0.0;
    let mut var_a = 0.0;
    let mut var_b = 0.0;
    for i in 0..ranks_a.len() {
        let da = ranks_a[i] - mean_a;
        let db = ranks_b[i] - mean_b;
        cov += da * db;
        var_a += da * da;
        var_b += db * db;
    }
    if var_a == 0.0 || var_b == 0.0 {
        return None;
    }
    Some(cov / (var_a.sqrt() * var_b.sqrt()))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    fn sim(seed: u64, edge: f64) -> SimResult {
        SimResult {
            seed,
            submission_edge: edge,
        }
    }

    /// A `PairedStat` with a caller-chosen `n`/`mean_diff` and a degenerate (point) interval
    /// — for tests elsewhere in `tools/bench` that need a `PairedStat` to format or aggregate
    /// without recomputing one from raw `SimResult`s. Shared here rather than each of
    /// `commands/grid.rs` and `commands/compare.rs` redeclaring the same literal.
    pub(crate) fn sample_stat(n: usize, mean_diff: f64) -> PairedStat {
        PairedStat {
            n,
            mean_diff,
            std_error: 0.0,
            t_critical: f64::NAN,
            ci_low: mean_diff,
            ci_high: mean_diff,
        }
    }

    #[test]
    fn hand_computed_paired_stat() {
        let candidate = vec![sim(1, 10.0), sim(2, 12.0), sim(3, 11.0)];
        let reference = vec![sim(1, 9.0), sim(2, 9.0), sim(3, 9.0)];
        let stat = paired_stat(&candidate, &reference).unwrap();

        assert_eq!(stat.n, 3);
        assert!((stat.mean_diff - 2.0).abs() < 1e-12);
        assert!((stat.std_error - (1.0_f64 / 3.0).sqrt()).abs() < 1e-9);
        assert!((stat.t_critical - 4.303).abs() < 1e-9);
        let margin = stat.t_critical * stat.std_error;
        assert!((stat.ci_low - (2.0 - margin)).abs() < 1e-9);
        assert!((stat.ci_high - (2.0 + margin)).abs() < 1e-9);
    }

    #[test]
    fn identical_inputs_give_zero_mean_and_zero_width_interval() {
        let batch = vec![sim(1, 5.0), sim(2, 6.0), sim(3, 7.0)];
        let stat = paired_stat(&batch, &batch).unwrap();
        assert_eq!(stat.mean_diff, 0.0);
        assert_eq!(stat.ci_low, 0.0);
        assert_eq!(stat.ci_high, 0.0);
    }

    #[test]
    fn mismatched_seeds_error() {
        let candidate = vec![sim(1, 1.0)];
        let reference = vec![sim(2, 1.0)];
        let err = paired_stat(&candidate, &reference).unwrap_err();
        assert!(
            err.to_string().contains("unpaired"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn mismatched_lengths_error() {
        let candidate = vec![sim(1, 1.0), sim(2, 1.0)];
        let reference = vec![sim(1, 1.0)];
        let err = paired_stat(&candidate, &reference).unwrap_err();
        assert!(
            err.to_string().contains("equal-length"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn empty_batches_error() {
        assert!(paired_stat(&[], &[]).is_err());
    }

    #[test]
    fn single_seed_gives_degenerate_interval() {
        let candidate = vec![sim(1, 5.0)];
        let reference = vec![sim(1, 3.0)];
        let stat = paired_stat(&candidate, &reference).unwrap();
        assert_eq!(stat.mean_diff, 2.0);
        assert_eq!(stat.ci_low, 2.0);
        assert_eq!(stat.ci_high, 2.0);
        assert!(stat.t_critical.is_nan());
    }

    #[test]
    fn t_table_boundary_df_29_and_30() {
        assert!((t_critical(29) - 2.045).abs() < 1e-9);
        assert!((t_critical(30) - NORMAL_APPROX_Z).abs() < 1e-9);
        assert!((t_critical(999) - NORMAL_APPROX_Z).abs() < 1e-9);
    }

    #[test]
    fn t_table_df_1() {
        assert!((t_critical(1) - 12.706).abs() < 1e-9);
    }

    #[test]
    fn median_of_empty_is_none() {
        assert_eq!(median(&[]), None);
    }

    #[test]
    fn median_odd_length_is_the_middle_element() {
        assert_eq!(median(&[3.0, 1.0, 2.0]), Some(2.0));
    }

    #[test]
    fn median_even_length_averages_the_two_middle_elements() {
        assert_eq!(median(&[1.0, 2.0, 3.0, 4.0]), Some(2.5));
    }

    #[test]
    fn median_does_not_require_pre_sorted_input() {
        assert_eq!(median(&[5.0, 1.0, 4.0, 2.0, 3.0]), Some(3.0));
    }

    #[test]
    fn fractional_ranks_break_ties_with_the_average_rank() {
        // Values 10,10,20 -> ranks 1,2,3 for the sorted order, but the tied 10s share rank
        // 1.5 each; the untied 20 keeps rank 3.
        let ranks = fractional_ranks(&[10.0, 20.0, 10.0]);
        assert_eq!(ranks, vec![1.5, 3.0, 1.5]);
    }

    #[test]
    fn spearman_of_a_perfectly_monotone_pair_is_one() {
        let a = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let b = vec![10.0, 20.0, 30.0, 40.0, 50.0];
        assert!((spearman_rank_correlation(&a, &b).unwrap() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn spearman_of_a_perfectly_inverse_monotone_pair_is_negative_one() {
        let a = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let b = vec![50.0, 40.0, 30.0, 20.0, 10.0];
        assert!((spearman_rank_correlation(&a, &b).unwrap() - (-1.0)).abs() < 1e-9);
    }

    #[test]
    fn spearman_is_robust_to_a_nonlinear_but_monotone_transform() {
        // b = a^3 is nonlinear (Pearson would be < 1) but strictly monotone in a — Spearman
        // must still read exactly 1.0, which is the whole reason WHI-1225's kill (ii) uses it
        // rather than a linear correlation.
        let a: Vec<f64> = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let b: Vec<f64> = a.iter().map(|x| x * x * x).collect();
        assert!((spearman_rank_correlation(&a, &b).unwrap() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn spearman_requires_at_least_two_points() {
        assert_eq!(spearman_rank_correlation(&[1.0], &[1.0]), None);
        assert_eq!(spearman_rank_correlation(&[], &[]), None);
    }

    #[test]
    fn spearman_requires_equal_length_series() {
        assert_eq!(spearman_rank_correlation(&[1.0, 2.0], &[1.0]), None);
    }

    #[test]
    fn spearman_is_none_for_a_constant_series() {
        assert_eq!(
            spearman_rank_correlation(&[1.0, 1.0, 1.0], &[1.0, 2.0, 3.0]),
            None
        );
    }
}
