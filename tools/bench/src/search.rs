//! Coarse grid, then coordinate descent (docs/DESIGN.md §2.5). Generic over any number of
//! integer parameters so a future 2-4 parameter family reuses this unchanged —
//! `001-cpmm-fee`'s single `fee_bps` exercises the 1-dimensional case.

use crate::params::ParamSpec;

/// Hard cap on evaluation points per family (docs/DESIGN.md §2.5, §3.4) — not itself a
/// config value: a `[search]` section in `config/bench.toml` supplies the *chosen* budget,
/// validated at load time to never exceed this constant
/// (`config::BenchConfig::search_max_points`).
pub const MAX_SEARCH_POINTS: usize = 300;

#[derive(Debug, Clone)]
pub struct SearchOutcome {
    pub best: Vec<i128>,
    pub best_edge: f64,
    /// Every `(params, edge)` pair actually evaluated, in evaluation order. For a single
    /// parameter this is exactly the fee<->edge curve docs/DESIGN.md §2.8 requires be
    /// committed to `results/`.
    pub history: Vec<(Vec<i128>, f64)>,
    pub points_evaluated: usize,
    /// True if the search stopped because `points_evaluated` reached `budget`, rather than
    /// because coordinate descent converged on its own — "the search refuses to exceed 300
    /// evaluation points and says so": refusal is never exceeding the cap, and this flag is
    /// the "says so".
    pub budget_exhausted: bool,
}

/// Runs the search. `eval` is called with one candidate parameter vector (in `specs` order)
/// at a time and must return that point's edge; routing it through `params::rewrite_params`,
/// the fast compile path, and a **fixed** set of simulation configs is the caller's job
/// (`commands/fit.rs`) — common random numbers (docs/DESIGN.md §2.5: "every point in a
/// search is evaluated on the same screening seeds") is enforced by the caller reusing one
/// `Vec<SimulationConfig>` across every call, not by anything in this module.
///
/// Never evaluates more than `budget` points; an `eval` failure (e.g. the candidate failed
/// to compile) propagates immediately as an error.
pub fn coarse_grid_then_coordinate_descent(
    specs: &[ParamSpec],
    budget: usize,
    mut eval: impl FnMut(&[i128]) -> anyhow::Result<f64>,
) -> anyhow::Result<SearchOutcome> {
    if specs.is_empty() {
        anyhow::bail!("search requires at least one declared parameter");
    }
    if budget == 0 {
        anyhow::bail!("search budget must be at least 1");
    }

    let mut history: Vec<(Vec<i128>, f64)> = Vec::new();
    let mut points_evaluated = 0usize;

    // `None` once the budget is spent (not an error — running out of budget is an expected
    // stopping condition, not a failed evaluation). A genuine `eval` error still propagates.
    let mut try_eval = |point: &[i128]| -> anyhow::Result<Option<f64>> {
        if points_evaluated >= budget {
            return Ok(None);
        }
        let edge = eval(point)?;
        points_evaluated += 1;
        history.push((point.to_vec(), edge));
        Ok(Some(edge))
    };

    let grid_budget = (budget / 2).max(1).min(budget);
    let grid = coarse_grid(specs, grid_budget);

    let mut best_point: Option<Vec<i128>> = None;
    let mut best_edge = f64::NEG_INFINITY;
    let mut budget_exhausted = false;
    for point in &grid {
        match try_eval(point)? {
            Some(edge) => {
                if edge > best_edge {
                    best_edge = edge;
                    best_point = Some(point.clone());
                }
            }
            None => {
                budget_exhausted = true;
                break;
            }
        }
    }
    let mut best_point = best_point.ok_or_else(|| {
        anyhow::anyhow!("search budget ({budget}) is too small to evaluate even one grid point")
    })?;

    if !budget_exhausted {
        let mut step: Vec<i128> = specs.iter().map(|s| ((s.max - s.min) / 4).max(1)).collect();

        'cycles: loop {
            let mut any_dim_improved = false;
            for dim in 0..specs.len() {
                loop {
                    let mut moved = false;
                    for direction in [1i128, -1] {
                        let clamped = (best_point[dim] + direction * step[dim])
                            .clamp(specs[dim].min, specs[dim].max);
                        if clamped == best_point[dim] {
                            continue;
                        }
                        let mut candidate = best_point.clone();
                        candidate[dim] = clamped;

                        match try_eval(&candidate)? {
                            Some(edge) if edge > best_edge => {
                                best_edge = edge;
                                best_point = candidate;
                                moved = true;
                                any_dim_improved = true;
                                break;
                            }
                            Some(_) => {}
                            None => {
                                budget_exhausted = true;
                                break 'cycles;
                            }
                        }
                    }
                    if !moved {
                        if step[dim] <= 1 {
                            break;
                        }
                        step[dim] = (step[dim] / 2).max(1);
                    }
                }
            }
            if !any_dim_improved {
                break;
            }
        }
    }

    Ok(SearchOutcome {
        best: best_point,
        best_edge,
        history,
        points_evaluated,
        budget_exhausted,
    })
}

/// An evenly spaced integer grid over `specs`' declared ranges, sized to spend roughly
/// `budget` points total. For one dimension this is `budget` points spanning `[min, max]`;
/// for `n` dimensions each axis gets about `budget^(1/n)` points and the Cartesian product is
/// truncated to `budget` if rounding pushes it slightly over.
fn coarse_grid(specs: &[ParamSpec], budget: usize) -> Vec<Vec<i128>> {
    let n_dims = specs.len();
    let points_per_dim = if n_dims == 1 {
        budget.max(2)
    } else {
        ((budget as f64).powf(1.0 / n_dims as f64).floor() as usize).max(2)
    };

    let axes: Vec<Vec<i128>> = specs
        .iter()
        .map(|s| linspace_int(s.min, s.max, points_per_dim))
        .collect();

    let mut product: Vec<Vec<i128>> = vec![vec![]];
    for axis in &axes {
        let mut next = Vec::with_capacity(product.len() * axis.len());
        for prefix in &product {
            for &v in axis {
                let mut p = prefix.clone();
                p.push(v);
                next.push(p);
            }
        }
        product = next;
    }
    product.truncate(budget.max(1));
    product
}

/// `n` integers evenly spaced across `[min, max]` inclusive, deduplicated (a narrow range
/// with `n` larger than its integer count just returns every integer in it once).
fn linspace_int(min: i128, max: i128, n: usize) -> Vec<i128> {
    if min == max || n <= 1 {
        return vec![min];
    }
    let span = max - min;
    let available: usize = usize::try_from(span.saturating_add(1)).unwrap_or(usize::MAX);
    let n = n.min(available).max(2);

    let mut points: Vec<i128> = (0..n)
        .map(|i| {
            let frac = i as f64 / (n - 1) as f64;
            min + (span as f64 * frac).round() as i128
        })
        .collect();
    points.dedup();
    points
}

/// Checks whether a 1-D curve (parameter value -> edge) is single-peaked, within
/// `tolerance`: sorted by parameter, edges non-decrease up to some peak then non-increase,
/// where a change smaller than `tolerance` is treated as noise rather than a direction
/// change. Only meaningful for a single free parameter — this is the self-check
/// docs/DESIGN.md §2.8 requires of `001-cpmm-fee`'s fee<->edge response; a multi-parameter
/// family has no single curve to check this way.
pub fn is_unimodal_1d(curve: &[(i128, f64)], tolerance: f64) -> bool {
    let mut sorted = curve.to_vec();
    sorted.sort_by_key(|(p, _)| *p);
    sorted.dedup_by_key(|(p, _)| *p);

    let mut seen_decrease = false;
    for w in sorted.windows(2) {
        let (_, a) = w[0];
        let (_, b) = w[1];
        if b + tolerance < a {
            seen_decrease = true;
        } else if b > a + tolerance && seen_decrease {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(name: &str, min: i128, max: i128) -> ParamSpec {
        ParamSpec {
            name: name.to_string(),
            ty: "u128".to_string(),
            min,
            max,
            current: min,
        }
    }

    #[test]
    fn finds_the_peak_of_a_clean_1d_quadratic() {
        let specs = vec![spec("x", 0, 500)];
        let outcome =
            coarse_grid_then_coordinate_descent(&specs, 60, |p| Ok(-((p[0] - 77).pow(2)) as f64))
                .unwrap();
        assert_eq!(outcome.best, vec![77]);
        assert_eq!(outcome.best_edge, 0.0);
        assert!(!outcome.budget_exhausted);
        assert!(outcome.points_evaluated <= 60);
    }

    #[test]
    fn finds_the_peak_of_a_clean_2d_quadratic() {
        let specs = vec![spec("x", 0, 200), spec("y", 0, 200)];
        let outcome = coarse_grid_then_coordinate_descent(&specs, 150, |p| {
            Ok(-(((p[0] - 40).pow(2) + (p[1] - 160).pow(2)) as f64))
        })
        .unwrap();
        assert_eq!(outcome.best, vec![40, 160]);
        assert!(outcome.points_evaluated <= 150);
    }

    #[test]
    fn never_exceeds_the_budget() {
        let specs = vec![spec("x", 0, 500)];
        let outcome =
            coarse_grid_then_coordinate_descent(&specs, 5, |p| Ok(-((p[0] - 77).pow(2)) as f64))
                .unwrap();
        assert!(outcome.points_evaluated <= 5);
        assert!(outcome.budget_exhausted);
    }

    #[test]
    fn a_single_point_budget_still_returns_that_point() {
        let specs = vec![spec("x", 0, 500)];
        let outcome = coarse_grid_then_coordinate_descent(&specs, 1, |p| Ok(p[0] as f64)).unwrap();
        assert_eq!(outcome.points_evaluated, 1);
        assert!(outcome.budget_exhausted);
    }

    #[test]
    fn propagates_an_eval_error_immediately() {
        let specs = vec![spec("x", 0, 500)];
        let err =
            coarse_grid_then_coordinate_descent(&specs, 10, |_| anyhow::bail!("build failed"))
                .unwrap_err();
        assert!(err.to_string().contains("build failed"));
    }

    #[test]
    fn empty_specs_is_rejected() {
        let err = coarse_grid_then_coordinate_descent(&[], 10, |_| Ok(0.0)).unwrap_err();
        assert!(err.to_string().contains("at least one"));
    }

    #[test]
    fn zero_budget_is_rejected() {
        let specs = vec![spec("x", 0, 500)];
        let err = coarse_grid_then_coordinate_descent(&specs, 0, |_| Ok(0.0)).unwrap_err();
        assert!(err.to_string().contains("at least 1"));
    }

    #[test]
    fn linspace_deduplicates_a_narrow_range() {
        assert_eq!(linspace_int(0, 2, 10), vec![0, 1, 2]);
    }

    #[test]
    fn linspace_handles_a_degenerate_range() {
        assert_eq!(linspace_int(5, 5, 10), vec![5]);
    }

    #[test]
    fn unimodal_curve_passes() {
        let curve: Vec<(i128, f64)> = (0i128..=10)
            .map(|x| (x, -((x - 5).pow(2)) as f64))
            .collect();
        assert!(is_unimodal_1d(&curve, 1e-9));
    }

    #[test]
    fn multi_modal_curve_fails() {
        // Two separated peaks at x=2 and x=8.
        let curve: Vec<(i128, f64)> = (0i128..=10)
            .map(|x| {
                let a = -((x - 2).pow(2)) as f64;
                let b = -((x - 8).pow(2)) as f64;
                (x, a.max(b))
            })
            .collect();
        assert!(!is_unimodal_1d(&curve, 1e-9));
    }

    #[test]
    fn unimodal_check_tolerates_noise_within_bound() {
        // Monotonically increasing except a tiny wiggle well inside tolerance.
        let curve = vec![(0, 0.0), (1, 1.0), (2, 0.999_999), (3, 2.0), (4, 1.5)];
        assert!(is_unimodal_1d(&curve, 1e-3));
    }
}
