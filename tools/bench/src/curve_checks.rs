//! Mirrors `crates/sim/src/curve_checks.rs::submission_shape_violation` — **the authority;
//! check there first if anything here drifts** (same drift convention `fast_compile.rs`
//! already uses for `compile.rs`). That function is private to `prop-amm-sim` (`mod
//! curve_checks;`, not `pub mod`), so `tools/bench` cannot import it and instead keeps its
//! own copy, used by `bench fuzz` (WHI-1212) to run the exact same shape check the
//! simulator panics on, but ahead of a frozen search rather than during one.

use prop_amm_shared::nano::{f64_to_nano, nano_to_f64};

// Finite differences involve two endpoints; allow a few nanos of ambiguity from endpoint
// quantization and round-trip noise.
const QUOTE_DELTA_UNCERTAINTY_NANO: u64 = 4;
// Adjacent sample x-values that differ by <= 4 nanos are effectively the same grid point for
// shape-check purposes.
const INPUT_MERGE_EPS_NANO: u64 = 4;
// Keep runtime shape checks aligned with validator granularity (`CONCAVITY_DELTA_NANO`).
const MIN_CONCAVITY_DX_NANO: u64 = 1_000_000;

/// Returns `Some(message)` describing the first monotonicity or concavity violation found in
/// `points`, or `None` if the (cleaned, sorted) curve is monotone and concave throughout.
/// Byte-for-byte identical logic to `crates/sim/src/curve_checks.rs`'s private
/// `submission_shape_violation` — see that file if this one needs to change.
pub fn submission_shape_violation(points: &[(f64, f64)], min_input: f64) -> Option<String> {
    let min_input_nano = f64_to_nano(min_input);
    let mut sorted: Vec<(u64, u64)> = points
        .iter()
        .copied()
        .filter(|(input, output)| {
            input.is_finite() && output.is_finite() && *input > min_input && *output >= 0.0
        })
        .map(|(input, output)| (f64_to_nano(input), f64_to_nano(output)))
        .filter(|(input, _)| *input > min_input_nano)
        .collect();
    sorted.sort_by_key(|(input, _)| *input);

    let mut cleaned: Vec<(u64, u64)> = Vec::with_capacity(sorted.len());
    for (input, output) in sorted {
        if let Some((prev_input, prev_output)) = cleaned.last_mut() {
            if input.saturating_sub(*prev_input) <= INPUT_MERGE_EPS_NANO {
                if output > *prev_output {
                    *prev_output = output;
                }
                continue;
            }
        }
        cleaned.push((input, output));
    }

    for window in cleaned.windows(2) {
        let (in_a, out_a) = window[0];
        let (in_b, out_b) = window[1];
        if in_b > in_a && out_b.saturating_add(QUOTE_DELTA_UNCERTAINTY_NANO) < out_a {
            return Some(format!(
                "monotonicity violated: input {in_a:.6} -> output {out_a:.6}, \
                 input {in_b:.6} -> output {out_b:.6}",
                in_a = nano_to_f64(in_a),
                out_a = nano_to_f64(out_a),
                in_b = nano_to_f64(in_b),
                out_b = nano_to_f64(out_b),
            ));
        }
    }

    let mut prev_segment: Option<(u64, u64)> = None; // (dy, dx)
    for window in cleaned.windows(2) {
        let (in_a, out_a) = window[0];
        let (in_b, out_b) = window[1];
        let dx = in_b - in_a;
        if dx == 0 {
            continue;
        }
        if dx < MIN_CONCAVITY_DX_NANO {
            continue;
        }
        let dy = out_b.saturating_sub(out_a);
        if let Some((prev_dy, prev_dx)) = prev_segment {
            let prev_upper = prev_dy.saturating_add(QUOTE_DELTA_UNCERTAINTY_NANO);
            let curr_lower = dy.saturating_sub(QUOTE_DELTA_UNCERTAINTY_NANO);
            let lhs = (prev_upper as u128) * (dx as u128);
            let rhs = (curr_lower as u128) * (prev_dx as u128);
            if rhs > lhs {
                let prev_slope = prev_dy as f64 / prev_dx as f64;
                let slope = dy as f64 / dx as f64;
                return Some(format!(
                    "concavity violated: slope rose from {prev:.9} to {slope:.9} \
                     between inputs {in_a:.6} and {in_b:.6}",
                    prev = prev_slope,
                    slope = slope,
                    in_a = nano_to_f64(in_a),
                    in_b = nano_to_f64(in_b),
                ));
            }
        }
        prev_segment = Some((dy, dx));
    }

    None
}

#[cfg(test)]
mod tests {
    use super::submission_shape_violation;

    const MIN_INPUT: f64 = 1e-3;

    fn assert_valid(points: &[(f64, f64)], context: &str) {
        if let Some(err) = submission_shape_violation(points, MIN_INPUT) {
            panic!("{context}: unexpected shape violation: {err}");
        }
    }

    #[test]
    fn accepts_simple_concave_curve() {
        let points: Vec<(f64, f64)> = (1..120)
            .map(|i| {
                let x = i as f64 * 0.25;
                (x, (1.0 + x).ln())
            })
            .collect();
        assert_valid(&points, "ln(1+x)");
    }

    #[test]
    fn accepts_unsorted_and_duplicate_inputs() {
        let mut points = vec![
            (0.1, 0.0953102),
            (0.2, 0.1823216),
            (0.2, 0.1823216),
            (0.4, 0.3364722),
            (0.8, 0.5877866),
            (1.6, 0.9555114),
            (3.2, 1.4350845),
            (6.4, 2.0014800),
        ];
        points.reverse();
        assert_valid(&points, "unsorted duplicates");
    }

    #[test]
    fn accepts_staircase_from_quantization() {
        let points: Vec<(f64, f64)> = (1..300)
            .map(|i| {
                let x = i as f64 * 0.05;
                let y = ((1.0 + x).ln() * 1_000_000.0).floor() / 1_000_000.0;
                (x, y)
            })
            .collect();
        assert_valid(&points, "quantized staircase");
    }

    #[test]
    fn rejects_non_monotone_curve() {
        let points = vec![(0.1, 1.0), (0.2, 1.1), (0.3, 1.05), (0.4, 1.2)];
        let err = submission_shape_violation(&points, MIN_INPUT).expect("expected violation");
        assert!(err.contains("monotonicity"), "unexpected error: {err}");
    }

    #[test]
    fn rejects_non_concave_curve() {
        let points = vec![(0.1, 0.1), (0.2, 0.18), (0.3, 0.31), (0.4, 0.45)];
        let err = submission_shape_violation(&points, MIN_INPUT).expect("expected violation");
        assert!(err.contains("concavity"), "unexpected error: {err}");
    }

    #[test]
    fn rejects_inverted_kink_beyond_a_small_probe_window() {
        // A curve that is a straight line up to x=200 (would pass a 10-point probe capped
        // at 200), then kinks to a *steeper* slope afterwards (WHI-1207's failure shape:
        // concavity violated far beyond the small window `prop-amm validate` samples).
        let mut points: Vec<(f64, f64)> = (1..=200).map(|x| (x as f64, x as f64 * 2.0)).collect();
        for x in 201..=400 {
            let x = x as f64;
            points.push((x, 400.0 + (x - 200.0) * 5.0));
        }
        let err = submission_shape_violation(&points, MIN_INPUT).expect("expected violation");
        assert!(err.contains("concavity"), "unexpected error: {err}");
    }

    /// Ported from `crates/sim/src/curve_checks.rs`'s own
    /// `exposes_false_positive_from_cancellation_prone_concave_curve` — a regression pin for
    /// a subtle nano-quantization false-positive the checker itself once needed guarding
    /// against. Kept in sync with the upstream test by inspection at each upstream sync,
    /// same as the mirrored function above (see `docs/DEFERRED_ISSUES.md` for the residual
    /// drift risk this doesn't fully close).
    #[test]
    fn exposes_false_positive_from_cancellation_prone_concave_curve() {
        // f(x) = sqrt(C + x) - sqrt(C) is monotone and concave for C > 0:
        // f'(x) = 1 / (2*sqrt(C+x)) > 0, f''(x) = -1 / (4*(C+x)^(3/2)) < 0.
        // With large C, naive evaluation suffers cancellation and can create flat-then-jump
        // artifacts that trip the discrete slope-rise check.
        let c: f64 = 1e16;
        let xs = [
            0.9628366933867734,
            0.9828747494989979,
            1.0029128056112224,
            1.0229508617234468,
        ];

        let naive_points: Vec<(f64, f64)> = xs
            .iter()
            .map(|x| (*x, (c + *x).sqrt() - c.sqrt()))
            .collect();
        let err = submission_shape_violation(&naive_points, MIN_INPUT).expect(
            "expected checker to flag cancellation-prone evaluation despite legal underlying shape",
        );
        assert!(err.contains("concavity"), "unexpected error: {err}");

        // Equivalent stable form: sqrt(C+x)-sqrt(C) = x / (sqrt(C+x)+sqrt(C)).
        let stable_points: Vec<(f64, f64)> = xs
            .iter()
            .map(|x| (*x, *x / ((c + *x).sqrt() + c.sqrt())))
            .collect();
        assert_valid(
            &stable_points,
            "stable algebraic form of same legal concave/monotone curve",
        );
    }
}
