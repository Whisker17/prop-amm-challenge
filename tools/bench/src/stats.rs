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
        let variance =
            diffs.iter().map(|d| (d - mean_diff).powi(2)).sum::<f64>() / (n - 1) as f64;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn sim(seed: u64, edge: f64) -> SimResult {
        SimResult { seed, submission_edge: edge }
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
        assert!(err.to_string().contains("unpaired"), "unexpected error: {err}");
    }

    #[test]
    fn mismatched_lengths_error() {
        let candidate = vec![sim(1, 1.0), sim(2, 1.0)];
        let reference = vec![sim(1, 1.0)];
        let err = paired_stat(&candidate, &reference).unwrap_err();
        assert!(err.to_string().contains("equal-length"), "unexpected error: {err}");
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
}
