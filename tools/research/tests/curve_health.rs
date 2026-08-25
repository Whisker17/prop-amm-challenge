//! Health checks for the boundary between the simulation's `f64`/nano ledger and
//! the curves' exact integer state.
//!
//! The DODO state machine relies on invariants that hold exactly on chain
//! (`ABOVE_ONE => Q >= Q0`, `BELOW_ONE => B >= B0`). The simulation ledger stores
//! reserves as `f64` and quantises them to nano, so in principle a reserve could
//! land one nano on the wrong side of a persisted target, which would make
//! `adjustedTarget` revert. These tests (a) document exactly what the adapter
//! does in that situation and (b) assert that it does not actually happen over
//! long runs.

use prop_amm_research::curves;
use prop_amm_research::dodo::decimal_math::ONE;
use prop_amm_research::dodo::RState;
use prop_amm_research::experiment::{self, BatchConfig, Competitor};
use prop_amm_research::strategies::strategy_by_id;
use prop_amm_research::u256::U256;
use prop_amm_shared::instruction::encode_swap_instruction;

const NANO: u64 = 1_000_000_000;

fn wad(units: u128) -> U256 {
    ONE.checked_mul(U256::from_u128(units)).unwrap()
}

#[test]
fn an_inconsistent_state_is_reported_as_a_revert_not_silently_repriced() {
    // Hand-build the pathological state: R = BELOW_ONE with B one nano *below*
    // the persisted base target. On chain this cannot happen; if the ledger ever
    // produced it, `adjustedTarget` would revert.
    let mut storage = curves::dodo_initial_storage(
        wad(100),
        U256::from_u128(100_000_000_000_000_000),
        wad(100),
        wad(10_000),
        U256::ZERO,
    );
    storage[curves::dodo_storage::R_STATE] = RState::BelowOne.as_u8();

    curves::reset_revert_count();
    // reserve_x is one nano below target_base (100 X).
    let reserve_x = 100 * NANO - 1;
    let data = encode_swap_instruction(1, NANO, reserve_x, 10_000 * NANO, &storage);
    let quote = curves::dodo_compute_swap(&data);

    assert_eq!(quote, 0, "a reverting curve must quote zero, never a guess");
    assert_eq!(
        curves::revert_count(),
        1,
        "the revert must be counted so a run cannot look healthy while it is not"
    );

    // The same state with B exactly at the target quotes normally.
    curves::reset_revert_count();
    let data = encode_swap_instruction(1, NANO, 100 * NANO, 10_000 * NANO, &storage);
    assert!(curves::dodo_compute_swap(&data) > 0);
    assert_eq!(curves::revert_count(), 0);
}

#[test]
fn long_runs_produce_no_curve_reverts() {
    // Debug-build runtime is the constraint here, so this covers the extremes of
    // the parameter table plus the two non-DODO families. The full benchmark
    // reports `curve_reverts` for all 17 strategies across every seed.
    let ids = [
        "dodo-k1000000000000000000",
        "dodo-k1000000000000000",
        "flashbots-c1",
        "flashbots-c1000",
        "univ2-zero-fee",
    ];
    let batch = BatchConfig {
        simulations: 2,
        steps: 1_500,
        seed_start: 0,
        seed_stride: 7,
        competitor: Competitor::Normalizer,
        workers: 1,
    };
    let configs = experiment::seed_configs(&batch);

    for id in ids {
        let strategy = strategy_by_id(id).expect("known strategy");
        for config in &configs {
            let metrics = experiment::run_single(&strategy, batch.competitor, config);
            assert_eq!(
                metrics.curve_revert_count, 0,
                "{id} seed {}: {} curve reverts",
                config.seed, metrics.curve_revert_count
            );
            assert!(
                metrics.retail_trade_count > 0,
                "{id} seed {}: the curve never traded, so the run proves nothing",
                config.seed
            );
        }
    }
}

#[test]
fn solo_runs_also_produce_no_curve_reverts() {
    let batch = BatchConfig {
        simulations: 1,
        steps: 1_500,
        seed_start: 3,
        seed_stride: 1,
        competitor: Competitor::None,
        workers: 1,
    };
    let configs = experiment::seed_configs(&batch);
    for id in ["dodo-k100000000000000000", "flashbots-c10"] {
        let strategy = strategy_by_id(id).expect("known strategy");
        let metrics = experiment::run_single(&strategy, batch.competitor, &configs[0]);
        assert_eq!(metrics.curve_revert_count, 0, "{id}");
        assert!(metrics.retail_trade_count > 0, "{id}");
    }
}
