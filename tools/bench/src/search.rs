//! Coarse grid, then coordinate descent (docs/DESIGN.md §2.5). Generic over any number of
//! integer parameters so a future 2-4 parameter family reuses this unchanged —
//! `001-cpmm-fee`'s single `fee_bps` exercises the 1-dimensional case.

use std::collections::HashMap;

use crate::params::ParamSpec;

/// Hard cap on evaluation points per family (docs/DESIGN.md §2.5, §3.4) — not itself a
/// config value: a `[search]` section in `config/bench.toml` supplies the *chosen* budget,
/// validated at load time to never exceed this constant
/// (`config::BenchConfig::search_max_points`).
pub const MAX_SEARCH_POINTS: usize = 300;

/// Validates an evaluation-point budget against the protocol's bounds (docs/DESIGN.md
/// §2.5, §3.4): at least 1, at most [`MAX_SEARCH_POINTS`]. One shared check for every
/// surface a budget can come from — `config::BenchConfig::parse`'s `[search] max_points`,
/// `commands/fit.rs`'s `--max-points` CLI override, and this module's own entry point —
/// so the bound and its wording live in exactly one place (WHI-1205).
pub fn validate_budget(budget: usize, label: &str) -> anyhow::Result<()> {
    if budget == 0 {
        anyhow::bail!("{label} must be at least 1");
    }
    if budget > MAX_SEARCH_POINTS {
        anyhow::bail!(
            "{label} ({budget}) exceeds the protocol's hard cap of {MAX_SEARCH_POINTS} \
             (docs/DESIGN.md §2.5, §3.4)"
        );
    }
    Ok(())
}

/// The outcome of evaluating one search point (docs/DESIGN.md §2.5, WHI-1213). A candidate
/// whose output violates the runtime shape check (`crates/sim/src/curve_checks.rs`) panics
/// mid-simulation — `crates/sim` is upstream-owned (§3.2), so the panic site itself can
/// never be fixed here; `commands/fit.rs` instead catches it (`std::panic::catch_unwind`
/// around the batch run) and reports it as `Invalid` rather than letting it abort the whole
/// `bench fit` process.
///
/// Deliberately not folded into `f64` (e.g. as `f64::NEG_INFINITY`): that would contaminate
/// the single-peak self-check's tolerance/scale math (docs/DESIGN.md §2.8, which takes the
/// max absolute edge over the curve) with an infinite value. It is also deliberately not
/// silently skipped: skipping would spend no budget and let coordinate descent step back
/// into the same panicking region for free, whereas an `Invalid` point still consumes one
/// of the 300 evaluation points and is memoized like a valid one.
#[derive(Debug, Clone)]
pub enum PointOutcome {
    /// The point compiled and simulated cleanly; this is its average edge.
    Valid(f64),
    /// The point panicked during simulation (a caught, not propagated, panic). The `String`
    /// is the panic message, for the `results/` snapshot's "Invalid points" section.
    Invalid(String),
}

#[derive(Debug, Clone)]
pub struct SearchOutcome {
    pub best: Vec<i128>,
    pub best_edge: f64,
    /// Every *distinct* point that produced a **valid** edge, in first-seen order — an
    /// `Invalid` point is never added here (it has no edge to report as a curve sample); a
    /// coordinate-descent revisit of an already-measured point is answered from the cache
    /// (see `coarse_grid_then_coordinate_descent`'s `memo`) and does not add a second entry
    /// either. For a single parameter this is exactly the fee<->edge curve docs/DESIGN.md
    /// §2.8 requires be committed to `results/` — holes left by `Invalid` points are just
    /// absent from it, not present with some sentinel edge.
    pub history: Vec<(Vec<i128>, f64)>,
    /// Every *distinct* point that panicked, in first-seen order, with its panic message —
    /// docs/DESIGN.md §2.5/WHI-1213's "listed explicitly in the `results/` snapshot".
    pub invalid: Vec<(Vec<i128>, String)>,
    /// Count of distinct points actually compiled and simulated (valid **and** invalid) —
    /// never exceeds `budget`, and never double-counts a cached revisit. An invalid point
    /// still consumed a compile and a simulation, so it counts against the budget the same
    /// as a valid one (docs/DESIGN.md §2.5/WHI-1213).
    pub points_evaluated: usize,
    /// True if the search stopped because `points_evaluated` reached `budget`, rather than
    /// because coordinate descent converged on its own — "the search refuses to exceed 300
    /// evaluation points and says so": refusal is never exceeding the cap, and this flag is
    /// the "says so".
    pub budget_exhausted: bool,
}

/// Runs the search. `eval` is called with one candidate parameter vector (in `specs` order)
/// at a time and must return that point's outcome; routing it through `params::rewrite_params`,
/// the fast compile path, and a **fixed** set of simulation configs — including catching a
/// shape-check panic into `PointOutcome::Invalid` — is the caller's job (`commands/fit.rs`).
/// Common random numbers (docs/DESIGN.md §2.5: "every point in a search is evaluated on the
/// same screening seeds") is enforced by the caller reusing one `Vec<SimulationConfig>`
/// across every call, not by anything in this module.
///
/// Never evaluates more than `budget` points; an `eval` failure (e.g. the candidate failed
/// to compile — a genuine error, distinct from a caught runtime panic) propagates
/// immediately as an error.
pub fn coarse_grid_then_coordinate_descent(
    specs: &[ParamSpec],
    budget: usize,
    mut eval: impl FnMut(&[i128]) -> anyhow::Result<PointOutcome>,
) -> anyhow::Result<SearchOutcome> {
    if specs.is_empty() {
        anyhow::bail!("search requires at least one declared parameter");
    }
    validate_budget(budget, "search budget")?;

    let mut history: Vec<(Vec<i128>, f64)> = Vec::new();
    let mut invalid: Vec<(Vec<i128>, String)> = Vec::new();
    let mut points_evaluated = 0usize;
    // Coordinate descent revisits points the grid phase (or an earlier descent step) already
    // measured — e.g. clamping two different step sizes to the same boundary value. A cache
    // keyed on the exact parameter vector answers those for free (whether the point was valid
    // or invalid): the budget counts *distinct* points actually compiled and simulated, not
    // evaluation *attempts* (docs/DESIGN.md §2.5's "hard cap 300 evaluation points" —
    // re-asking a fast-path build for a value it already measured is not a new evaluation).
    let mut memo: HashMap<Vec<i128>, PointOutcome> = HashMap::new();

    // `None` once the budget is spent (not an error — running out of budget is an expected
    // stopping condition, not a failed evaluation). A genuine `eval` error still propagates.
    let mut try_eval = |point: &[i128]| -> anyhow::Result<Option<PointOutcome>> {
        // A revisit is not a new curve sample either — it's already in `history`/`invalid`
        // from the first time this exact point was measured.
        if let Some(outcome) = memo.get(point) {
            return Ok(Some(outcome.clone()));
        }
        if points_evaluated >= budget {
            return Ok(None);
        }
        let outcome = eval(point)?;
        points_evaluated += 1;
        memo.insert(point.to_vec(), outcome.clone());
        match &outcome {
            PointOutcome::Valid(edge) => history.push((point.to_vec(), *edge)),
            PointOutcome::Invalid(reason) => invalid.push((point.to_vec(), reason.clone())),
        }
        Ok(Some(outcome))
    };

    // Half the budget locates the region (coarse grid), half refines it (coordinate descent)
    // — an even split with no data yet to weight it by; §2.5 fixes the *total* budget but
    // leaves how a family spends it inside that total to the implementation.
    let grid_budget = (budget / 2).max(1);
    let grid = coarse_grid(specs, grid_budget);

    let mut best_point: Option<Vec<i128>> = None;
    let mut best_edge = f64::NEG_INFINITY;
    let mut budget_exhausted = false;
    // Tracked separately from `invalid` (rather than reading `invalid.len()` below) since
    // `try_eval` captures `invalid` mutably for as long as it's still callable — including
    // the coordinate-descent loop further down — so an intervening read of `invalid` itself
    // would conflict with that borrow.
    let mut grid_invalid_count = 0usize;
    for point in &grid {
        match try_eval(point)? {
            Some(PointOutcome::Valid(edge)) => {
                if edge > best_edge {
                    best_edge = edge;
                    best_point = Some(point.clone());
                }
            }
            Some(PointOutcome::Invalid(_)) => {
                grid_invalid_count += 1;
            }
            None => {
                budget_exhausted = true;
                break;
            }
        }
    }
    let mut best_point = best_point.ok_or_else(|| {
        if grid_invalid_count == 0 {
            anyhow::anyhow!("search budget ({budget}) is too small to evaluate even one grid point")
        } else {
            anyhow::anyhow!(
                "every evaluated grid point ({grid_invalid_count}) was invalid (a caught \
                 shape-check panic) — no valid point exists yet to start coordinate \
                 descent from"
            )
        }
    })?;

    if !budget_exhausted {
        // A quarter of each dimension's range: coarse enough that the first couple of
        // step-halvings still cross the grid's own spacing (so descent explores past its
        // immediate grid neighbors, not just between them), but small enough that a
        // reasonably-behaved family converges well inside the remaining budget.
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
                            Some(PointOutcome::Valid(edge)) if edge > best_edge => {
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
        invalid,
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
            indent: String::new(),
        }
    }

    #[test]
    fn validate_budget_rejects_zero() {
        let err = validate_budget(0, "some budget").unwrap_err();
        assert!(err.to_string().contains("some budget must be at least 1"));
    }

    #[test]
    fn validate_budget_rejects_above_the_cap() {
        let err = validate_budget(MAX_SEARCH_POINTS + 1, "some budget").unwrap_err();
        assert!(err.to_string().contains("exceeds the protocol's hard cap"));
    }

    #[test]
    fn validate_budget_allows_the_full_range() {
        assert!(validate_budget(1, "some budget").is_ok());
        assert!(validate_budget(MAX_SEARCH_POINTS, "some budget").is_ok());
    }

    #[test]
    fn finds_the_peak_of_a_clean_1d_quadratic() {
        let specs = vec![spec("x", 0, 500)];
        let outcome = coarse_grid_then_coordinate_descent(&specs, 60, |p| {
            Ok(PointOutcome::Valid(-((p[0] - 77).pow(2)) as f64))
        })
        .unwrap();
        assert_eq!(outcome.best, vec![77]);
        assert_eq!(outcome.best_edge, 0.0);
        assert!(!outcome.budget_exhausted);
        assert!(outcome.points_evaluated <= 60);
        assert!(outcome.invalid.is_empty());
    }

    #[test]
    fn finds_the_peak_of_a_clean_2d_quadratic() {
        let specs = vec![spec("x", 0, 200), spec("y", 0, 200)];
        let outcome = coarse_grid_then_coordinate_descent(&specs, 150, |p| {
            Ok(PointOutcome::Valid(
                -(((p[0] - 40).pow(2) + (p[1] - 160).pow(2)) as f64),
            ))
        })
        .unwrap();
        assert_eq!(outcome.best, vec![40, 160]);
        assert!(outcome.points_evaluated <= 150);
    }

    #[test]
    fn never_exceeds_the_budget() {
        let specs = vec![spec("x", 0, 500)];
        let outcome = coarse_grid_then_coordinate_descent(&specs, 5, |p| {
            Ok(PointOutcome::Valid(-((p[0] - 77).pow(2)) as f64))
        })
        .unwrap();
        assert!(outcome.points_evaluated <= 5);
        assert!(outcome.budget_exhausted);
    }

    #[test]
    fn a_single_point_budget_still_returns_that_point() {
        let specs = vec![spec("x", 0, 500)];
        let outcome = coarse_grid_then_coordinate_descent(&specs, 1, |p| {
            Ok(PointOutcome::Valid(p[0] as f64))
        })
        .unwrap();
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
        let err = coarse_grid_then_coordinate_descent(&[], 10, |_| Ok(PointOutcome::Valid(0.0)))
            .unwrap_err();
        assert!(err.to_string().contains("at least one"));
    }

    #[test]
    fn zero_budget_is_rejected() {
        let specs = vec![spec("x", 0, 500)];
        let err = coarse_grid_then_coordinate_descent(&specs, 0, |_| Ok(PointOutcome::Valid(0.0)))
            .unwrap_err();
        assert!(err.to_string().contains("at least 1"));
    }

    #[test]
    fn budget_above_the_protocol_cap_is_rejected() {
        // This entry point's own bound-checking widened from zero-only to the full
        // `validate_budget` range (WHI-1205's shared check) — covered separately by
        // `validate_budget`'s own tests, but this confirms the widening actually reached
        // this call site too.
        let specs = vec![spec("x", 0, 500)];
        let err = coarse_grid_then_coordinate_descent(&specs, MAX_SEARCH_POINTS + 1, |_| {
            Ok(PointOutcome::Valid(0.0))
        })
        .unwrap_err();
        assert!(err.to_string().contains("exceeds the protocol's hard cap"));
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
    fn revisiting_an_already_measured_point_does_not_re_invoke_eval_or_spend_budget() {
        use std::cell::RefCell;
        use std::rc::Rc;

        // Grid points [0, 3, 7, 10] then descent from 3: the very next step (±2 from the new
        // best 5) lands back on 7 and 3, both already measured by the grid phase. Without
        // memoization those two revisits would consume 2 more of the 8-point budget on top of
        // the 3 genuinely new descent evaluations, for 9 total — over budget. With it, the
        // search converges to the true peak (x=5) using only 7 distinct evaluations.
        let specs = vec![spec("x", 0, 10)];
        let calls = Rc::new(RefCell::new(0usize));
        let calls_inner = Rc::clone(&calls);
        let outcome = coarse_grid_then_coordinate_descent(&specs, 8, move |p| {
            *calls_inner.borrow_mut() += 1;
            Ok(PointOutcome::Valid(-((p[0] - 5).pow(2)) as f64))
        })
        .unwrap();

        assert_eq!(outcome.best, vec![5]);
        assert_eq!(outcome.best_edge, 0.0);
        assert_eq!(outcome.points_evaluated, 7);
        assert!(!outcome.budget_exhausted);
        assert_eq!(
            *calls.borrow(),
            7,
            "a cached revisit must not re-invoke eval"
        );
    }

    #[test]
    fn an_invalid_point_consumes_budget_and_is_never_chosen_as_best() {
        // Every point with x >= 5 is invalid (simulating a panicking parameter sub-region);
        // the true unconstrained peak at x=77 is deep in that region, so the search must
        // settle on the best *valid* point instead of chasing the invalid peak.
        let specs = vec![spec("x", 0, 10)];
        let outcome = coarse_grid_then_coordinate_descent(&specs, 10, |p| {
            if p[0] >= 5 {
                Ok(PointOutcome::Invalid(format!("point {} panicked", p[0])))
            } else {
                Ok(PointOutcome::Valid(-((p[0] - 77).pow(2)) as f64))
            }
        })
        .unwrap();

        assert!(
            outcome.best[0] < 5,
            "best point {:?} should be valid",
            outcome.best
        );
        assert!(
            outcome.history.iter().all(|(p, _)| p[0] < 5),
            "history must contain only valid points: {:?}",
            outcome.history
        );
        assert!(
            outcome.invalid.iter().all(|(p, _)| p[0] >= 5),
            "invalid must contain only invalid points: {:?}",
            outcome.invalid
        );
        assert!(!outcome.invalid.is_empty());
        assert_eq!(
            outcome.points_evaluated,
            outcome.history.len() + outcome.invalid.len(),
            "every evaluated point is either valid or invalid, never both or neither"
        );
    }

    #[test]
    fn a_revisited_invalid_point_does_not_re_invoke_eval_or_spend_budget() {
        use std::cell::RefCell;
        use std::rc::Rc;

        // Same peak-at-5 shape and grid trajectory as
        // `revisiting_an_already_measured_point_does_not_re_invoke_eval_or_spend_budget`
        // above (grid points [0, 3, 7, 10], descent revisits 7 while stepping from the new
        // best 5), except x=7 is invalid instead of just a low-scoring valid point — it is
        // visited twice (once by the grid phase, once as a descent candidate) but must only
        // ever invoke `eval` once and appear once in `invalid`.
        let specs = vec![spec("x", 0, 10)];
        let calls = Rc::new(RefCell::new(0usize));
        let calls_inner = Rc::clone(&calls);
        let outcome = coarse_grid_then_coordinate_descent(&specs, 8, move |p| {
            *calls_inner.borrow_mut() += 1;
            if p[0] == 7 {
                Ok(PointOutcome::Invalid("point 7 panicked".to_string()))
            } else {
                Ok(PointOutcome::Valid(-((p[0] - 5).pow(2)) as f64))
            }
        })
        .unwrap();

        assert_eq!(outcome.best, vec![5]);
        assert_eq!(outcome.best_edge, 0.0);
        assert_eq!(outcome.points_evaluated, 7);
        assert_eq!(
            outcome.invalid,
            vec![(vec![7], "point 7 panicked".to_string())]
        );
        assert_eq!(
            *calls.borrow(),
            7,
            "a cached revisit of an invalid point must not re-invoke eval"
        );
    }

    #[test]
    fn all_invalid_grid_points_report_a_distinct_error_from_budget_too_small() {
        let specs = vec![spec("x", 0, 500)];
        let err = coarse_grid_then_coordinate_descent(&specs, 10, |p| {
            Ok(PointOutcome::Invalid(format!("point {} panicked", p[0])))
        })
        .unwrap_err();
        assert!(
            err.to_string().contains("was invalid"),
            "unexpected error: {err}"
        );
        assert!(!err.to_string().contains("too small"));
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
