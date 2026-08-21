//! State-machine fidelity for the DODO PMM port.
//!
//! These properties come from the Solidity, not from the Rust port:
//!
//! * `MantlePropAmmPool.swap` writes a target back **only** when `RState`
//!   changes, and then only on the side matching the trade direction.
//! * `PMMPricing.adjustedTarget` is idempotent — the Mantle harness exposes
//!   `adjustedTargetTwice` specifically to pin that.
//! * `RState` only ever takes the three declared values, and the reserve /
//!   target relationship implied by each state holds.

use prop_amm_research::dodo::decimal_math::ONE;
use prop_amm_research::dodo::state::{DodoPool, DodoQueryInput};
use prop_amm_research::dodo::RState;
use prop_amm_research::u256::U256;
use prop_amm_research::wad::price_to_wad;

/// Small deterministic LCG so the test needs no RNG dependency.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0 >> 11
    }

    fn range(&mut self, span: u64) -> u64 {
        if span == 0 {
            0
        } else {
            self.next() % span
        }
    }
}

fn wad(units: u128) -> U256 {
    ONE.checked_mul(U256::from_u128(units)).unwrap()
}

fn k_values() -> Vec<U256> {
    vec![
        U256::ZERO,
        U256::from_u128(1_000_000_000_000_000),
        U256::from_u128(100_000_000_000_000_000),
        U256::from_u128(500_000_000_000_000_000),
        ONE,
    ]
}

#[test]
fn targets_move_only_on_an_r_state_transition_and_only_on_the_traded_side() {
    let mut transitions = 0usize;
    let mut non_transitions = 0usize;
    let mut states_seen = [false; 3];

    for (case, k) in k_values().into_iter().enumerate() {
        let mut rng = Lcg(0xD0D0_0000 + case as u64);
        let mut pool = DodoPool {
            i: price_to_wad(100.0).unwrap(),
            k,
            target_base: wad(100),
            target_quote: wad(10_000),
            r_state: RState::One,
            lp_fee_rate: U256::ZERO,
        };
        let mut base = wad(100);
        let mut quote = wad(10_000);

        for _ in 0..400 {
            // Re-publish a wandering guide price, as the harness does per step.
            let drift = 95 + rng.range(11); // 95 .. 105
            pool.i = price_to_wad(drift as f64).unwrap();

            let sell_base = rng.range(2) == 0;
            // Mix small, medium and large trades so the inventory repeatedly
            // crosses the balance point and exercises every R transition.
            let magnitude = match rng.range(4) {
                0 => 1_000_000_000_000_000u64,      // ~0.001
                1 => 100_000_000_000_000_000,       // ~0.1
                2 => 2_000_000_000_000_000_000,     // ~2
                _ => 9_000_000_000_000_000_000,     // ~9
            };
            let amount_in = if sell_base {
                U256::from_u128(1 + rng.range(magnitude) as u128)
            } else {
                U256::from_u128(1 + rng.range(magnitude) as u128)
                    .checked_mul(U256::from_u64(100))
                    .unwrap()
            };

            let Some(result) = pool.quote(base, quote, sell_base, amount_in) else {
                continue;
            };
            let output_reserve = if sell_base { quote } else { base };
            if result.amount_out.is_zero() || result.amount_out >= output_reserve {
                continue; // the pool would revert; the simulation skips these too
            }

            let before = pool;
            pool.commit(sell_base, &result);
            states_seen[pool.r_state.as_u8() as usize] = true;

            if result.new_r == before.r_state {
                non_transitions += 1;
                assert_eq!(
                    pool.target_base, before.target_base,
                    "target base moved without an R transition"
                );
                assert_eq!(
                    pool.target_quote, before.target_quote,
                    "target quote moved without an R transition"
                );
            } else {
                transitions += 1;
                assert_eq!(pool.r_state, result.new_r);
                if sell_base {
                    assert_eq!(pool.target_base, result.adjusted_b0);
                    assert_eq!(
                        pool.target_quote, before.target_quote,
                        "sell-base must not write the quote target"
                    );
                } else {
                    assert_eq!(pool.target_quote, result.adjusted_q0);
                    assert_eq!(
                        pool.target_base, before.target_base,
                        "sell-quote must not write the base target"
                    );
                }
            }

            if sell_base {
                base = base.checked_add(amount_in).unwrap();
                quote = quote.checked_sub(result.amount_out).unwrap();
            } else {
                quote = quote.checked_add(amount_in).unwrap();
                base = base.checked_sub(result.amount_out).unwrap();
            }
        }
    }

    assert!(transitions > 50, "expected many R transitions, saw {transitions}");
    assert!(
        non_transitions > 50,
        "expected many non-transitions, saw {non_transitions}"
    );
    // Random amounts essentially never land on the exact return-to-one amount,
    // so ONE is reached by the targeted case below instead.
    assert!(
        states_seen[RState::AboveOne.as_u8() as usize],
        "expected to visit ABOVE_ONE"
    );
    assert!(
        states_seen[RState::BelowOne.as_u8() as usize],
        "expected to visit BELOW_ONE"
    );
}

#[test]
fn the_exact_return_to_one_amount_lands_on_state_one() {
    // ABOVE_ONE means B < B0 and Q > Q0; paying exactly `B0 - B` in base returns
    // the pool to ONE (`PMMPricing.sellBaseToken` case 2.2).
    let mut pool = DodoPool {
        i: price_to_wad(100.0).unwrap(),
        k: U256::from_u128(100_000_000_000_000_000),
        target_base: wad(100),
        target_quote: wad(10_000),
        r_state: RState::AboveOne,
        lp_fee_rate: U256::ZERO,
    };
    let base = wad(90);
    let quote = wad(11_200);
    let (adjusted_b0, adjusted_q0) = pool
        .input(base, quote, true, U256::ONE)
        .adjusted_target()
        .expect("adjusted target");
    let back_to_one = adjusted_b0.checked_sub(base).expect("B0 > B in ABOVE_ONE");

    let result = pool
        .quote(base, quote, true, back_to_one)
        .expect("exact boundary quote");
    assert_eq!(result.new_r, RState::One, "exact boundary must return to ONE");
    assert_eq!(
        result.amount_out,
        quote.checked_sub(adjusted_q0).unwrap(),
        "the payout is exactly the spare quote"
    );

    pool.commit(true, &result);
    assert_eq!(pool.r_state, RState::One);
    assert_eq!(pool.target_base, adjusted_b0, "sell-base writes the base target");
    assert_eq!(pool.target_quote, wad(10_000), "sell-base leaves the quote target");
}

#[test]
fn adjusted_target_is_idempotent() {
    // Mirrors `DodoPmmHarness.adjustedTargetTwice`.
    for (case, k) in k_values().into_iter().enumerate() {
        let mut rng = Lcg(0xADD0_0000 + case as u64);
        for _ in 0..200 {
            let base = wad(50 + rng.range(150) as u128);
            let quote = wad(5_000 + rng.range(15_000) as u128);
            let target_base = wad(50 + rng.range(150) as u128);
            let target_quote = wad(5_000 + rng.range(15_000) as u128);
            let r_state = if base > target_base {
                RState::BelowOne
            } else if base < target_base {
                RState::AboveOne
            } else {
                RState::One
            };
            // Keep the state self-consistent: ABOVE_ONE needs Q >= Q0,
            // BELOW_ONE needs B >= B0.
            let (quote, target_quote) = match r_state {
                RState::AboveOne => (quote.max(target_quote), quote.min(target_quote)),
                RState::BelowOne => (quote.min(target_quote), quote.max(target_quote)),
                RState::One => (quote, quote),
            };

            let input = DodoQueryInput {
                i: price_to_wad(100.0).unwrap(),
                k,
                b: base,
                q: quote,
                b0: target_base,
                q0: target_quote,
                r_state,
                sell_base: true,
                amount_in: U256::ONE,
                lp_fee_rate: U256::ZERO,
            };
            let Some((first_b0, first_q0)) = input.adjusted_target() else {
                continue;
            };
            let second = DodoQueryInput {
                b0: first_b0,
                q0: first_q0,
                ..input
            };
            let Some((second_b0, second_q0)) = second.adjusted_target() else {
                panic!("second adjustedTarget reverted");
            };
            assert_eq!(first_b0, second_b0, "adjustedTarget is not idempotent on B0");
            assert_eq!(first_q0, second_q0, "adjustedTarget is not idempotent on Q0");
        }
    }
}

#[test]
fn zero_fee_never_reduces_the_gross_output() {
    let pool = DodoPool {
        i: price_to_wad(100.0).unwrap(),
        k: U256::from_u128(100_000_000_000_000_000),
        target_base: wad(100),
        target_quote: wad(10_000),
        r_state: RState::One,
        lp_fee_rate: U256::ZERO,
    };
    for units in [1u128, 5, 20] {
        let result = pool.quote(wad(100), wad(10_000), true, wad(units)).unwrap();
        assert_eq!(result.lp_fee_amount, U256::ZERO);
        assert_eq!(result.amount_out, result.gross_amount_out);
    }
}
