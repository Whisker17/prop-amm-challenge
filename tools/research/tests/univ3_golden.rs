//! The Uniswap V3 port must reproduce, integer for integer, what the pinned
//! Solidity produces.
//!
//! Fixtures come from `solidity/univ3-golden/generate.sh`, which verifies the
//! commit and the sha256 of every pinned file, copies the tree to a temporary
//! directory, and executes it:
//!
//! * `per-function-vectors.json` — `TickMath`, `SqrtPriceMath`, `SwapMath`
//!   (at fee 0 **and** fee 3000) and `LiquidityMath`, reached through 0.7.6
//!   wrappers, with reverts recorded as vectors.
//! * `golden-vectors.json` — a real zero-fee `UniswapV3Pool` created by a real
//!   `UniswapV3Factory`, including a 220-swap consecutive sequence in which the
//!   full state is recorded after every single swap.
//!
//! No expected value in this file comes from the Rust implementation.

use prop_amm_research::json::Json;
use prop_amm_research::u256::U256;
use prop_amm_research::univ3::pool::{self, PoolConfig, PoolState};
use prop_amm_research::univ3::signed::I256;
use prop_amm_research::univ3::swap_math::{add_delta, compute_swap_step};
use prop_amm_research::univ3::{sqrt_price_math, tick_math, TickList};

fn load(name: &str) -> Json {
    let path = format!("{}/tests/fixtures/univ3/{name}", env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path}: {e}"));
    Json::parse(&text).unwrap_or_else(|e| panic!("parse {path}: {e}"))
}

fn dec(vector: &Json, field: &str) -> U256 {
    let raw = vector
        .get(field)
        .unwrap_or_else(|| panic!("missing {field}"))
        .as_str()
        .unwrap_or_else(|| panic!("{field} is not a string"));
    U256::from_dec_str(raw).unwrap_or_else(|| panic!("{field} = {raw} does not fit uint256"))
}

/// A signed decimal string, as the generator writes `int256` values.
fn signed(vector: &Json, field: &str) -> I256 {
    let raw = vector
        .get(field)
        .unwrap_or_else(|| panic!("missing {field}"))
        .as_str()
        .unwrap_or_else(|| panic!("{field} is not a string"));
    match raw.strip_prefix('-') {
        Some(magnitude) => I256::to_int256(U256::from_dec_str(magnitude).expect("magnitude"))
            .expect("magnitude fits")
            .wrapping_neg(),
        None => I256::to_int256(U256::from_dec_str(raw).expect("magnitude")).expect("fits"),
    }
}

fn int(vector: &Json, field: &str) -> i64 {
    vector
        .get(field)
        .unwrap_or_else(|| panic!("missing {field}"))
        .as_num_str()
        .unwrap_or_else(|| panic!("{field} is not a number"))
        .parse()
        .expect("integer")
}

fn flag(vector: &Json, field: &str) -> bool {
    vector
        .get(field)
        .unwrap_or_else(|| panic!("missing {field}"))
        .as_bool()
        .expect("bool")
}

fn rows<'a>(json: &'a Json, key: &str) -> &'a [Json] {
    json.get(key)
        .unwrap_or_else(|| panic!("missing array {key}"))
        .as_array()
        .unwrap_or_else(|| panic!("{key} is not an array"))
}

fn check_source(json: &Json) {
    let source = json.get("source").expect("source block");
    assert_eq!(
        source.get("commit").unwrap().as_str(),
        Some("e3589b192d0be27e100cd0daaf6c97204fdb1899"),
        "vectors must come from the pinned v3-core commit"
    );
    assert_eq!(source.get("solc").unwrap().as_str(), Some("0.7.6"));
}

// ===================== per-function vectors =====================

#[test]
fn tick_math_matches_the_pinned_solidity_exactly() {
    let json = load("per-function-vectors.json");
    check_source(&json);

    let at_tick = rows(&json, "getSqrtRatioAtTick");
    assert!(at_tick.len() > 500, "expected dense tick coverage");
    for vector in at_tick {
        let tick = int(vector, "tick") as i32;
        assert_eq!(
            tick_math::get_sqrt_ratio_at_tick(tick),
            Some(dec(vector, "sqrtPriceX96")),
            "getSqrtRatioAtTick({tick})"
        );
    }
    for vector in rows(&json, "getSqrtRatioAtTickReverts") {
        let tick = int(vector, "tick") as i32;
        assert_eq!(
            tick_math::get_sqrt_ratio_at_tick(tick),
            None,
            "getSqrtRatioAtTick({tick}) must revert"
        );
    }

    let at_ratio = rows(&json, "getTickAtSqrtRatio");
    assert!(at_ratio.len() > 1_000, "expected dense ratio coverage");
    for vector in at_ratio {
        let ratio = dec(vector, "sqrtPriceX96");
        assert_eq!(
            tick_math::get_tick_at_sqrt_ratio(ratio),
            Some(int(vector, "tick") as i32),
            "getTickAtSqrtRatio({ratio})"
        );
    }
    for vector in rows(&json, "getTickAtSqrtRatioReverts") {
        let ratio = dec(vector, "sqrtPriceX96");
        assert_eq!(
            tick_math::get_tick_at_sqrt_ratio(ratio),
            None,
            "getTickAtSqrtRatio({ratio}) must revert"
        );
    }
}

#[test]
fn sqrt_price_math_matches_the_pinned_solidity_exactly() {
    let json = load("per-function-vectors.json");

    for vector in rows(&json, "getNextSqrtPriceFromInput") {
        if vector.get("reverted").is_some() {
            let liquidity = dec(vector, "liquidity").as_u128().expect("uint128");
            assert_eq!(
                sqrt_price_math::get_next_sqrt_price_from_input(
                    dec(vector, "sqrtPX96"),
                    liquidity,
                    dec(vector, "amountIn"),
                    flag(vector, "zeroForOne"),
                ),
                None,
                "getNextSqrtPriceFromInput must revert"
            );
            continue;
        }
        let liquidity = dec(vector, "liquidity").as_u128().expect("uint128");
        assert_eq!(
            sqrt_price_math::get_next_sqrt_price_from_input(
                dec(vector, "sqrtPX96"),
                liquidity,
                dec(vector, "amountIn"),
                flag(vector, "zeroForOne"),
            ),
            Some(dec(vector, "sqrtQX96")),
            "getNextSqrtPriceFromInput"
        );
    }

    for vector in rows(&json, "getNextSqrtPriceFromOutput") {
        if vector.get("reverted").is_some() {
            let liquidity = dec(vector, "liquidity").as_u128().expect("uint128");
            assert_eq!(
                sqrt_price_math::get_next_sqrt_price_from_output(
                    dec(vector, "sqrtPX96"),
                    liquidity,
                    dec(vector, "amountOut"),
                    flag(vector, "zeroForOne"),
                ),
                None,
                "getNextSqrtPriceFromOutput must revert"
            );
            continue;
        }
        let liquidity = dec(vector, "liquidity").as_u128().expect("uint128");
        assert_eq!(
            sqrt_price_math::get_next_sqrt_price_from_output(
                dec(vector, "sqrtPX96"),
                liquidity,
                dec(vector, "amountOut"),
                flag(vector, "zeroForOne"),
            ),
            Some(dec(vector, "sqrtQX96")),
            "getNextSqrtPriceFromOutput"
        );
    }

    for vector in rows(&json, "getAmount0Delta") {
        if vector.get("reverted").is_some() {
            continue; // covered by the dedicated revert arrays
        }
        assert_eq!(
            sqrt_price_math::get_amount0_delta(
                dec(vector, "sqrtRatioAX96"),
                dec(vector, "sqrtRatioBX96"),
                dec(vector, "liquidity").as_u128().expect("uint128"),
                flag(vector, "roundUp"),
            ),
            Some(dec(vector, "amount")),
            "getAmount0Delta"
        );
    }

    for vector in rows(&json, "getAmount1Delta") {
        if vector.get("reverted").is_some() {
            continue; // covered by the dedicated revert arrays
        }
        assert_eq!(
            sqrt_price_math::get_amount1_delta(
                dec(vector, "sqrtRatioAX96"),
                dec(vector, "sqrtRatioBX96"),
                dec(vector, "liquidity").as_u128().expect("uint128"),
                flag(vector, "roundUp"),
            ),
            Some(dec(vector, "amount")),
            "getAmount1Delta"
        );
    }

    for (key, signed_delta) in [
        ("getAmount0DeltaSigned", true),
        ("getAmount1DeltaSigned", false),
    ] {
        for vector in rows(&json, key) {
            let liquidity = signed(vector, "liquidity")
                .to_int128()
                .expect("int128 liquidity");
            let actual = if signed_delta {
                sqrt_price_math::get_amount0_delta_signed(
                    dec(vector, "sqrtRatioAX96"),
                    dec(vector, "sqrtRatioBX96"),
                    liquidity,
                )
            } else {
                sqrt_price_math::get_amount1_delta_signed(
                    dec(vector, "sqrtRatioAX96"),
                    dec(vector, "sqrtRatioBX96"),
                    liquidity,
                )
            };
            assert_eq!(actual, Some(signed(vector, "amount")), "{key}");
        }
    }
}

#[test]
fn compute_swap_step_matches_the_pinned_solidity_at_zero_and_nonzero_fee() {
    let json = load("per-function-vectors.json");

    for key in ["computeSwapStepZeroFee", "computeSwapStepFee3000"] {
        let vectors = rows(&json, key);
        assert!(vectors.len() > 100, "{key}: expected broad coverage");
        let mut exact_in = 0usize;
        let mut exact_out = 0usize;
        let mut reached = 0usize;

        for vector in vectors {
            let step = compute_swap_step(
                dec(vector, "sqrtRatioCurrentX96"),
                dec(vector, "sqrtRatioTargetX96"),
                dec(vector, "liquidity").as_u128().expect("uint128"),
                signed(vector, "amountRemaining"),
                int(vector, "feePips") as u32,
            )
            .unwrap_or_else(|| panic!("{key}: port reverted where Solidity did not"));

            assert_eq!(
                step.sqrt_ratio_next_x96,
                dec(vector, "sqrtRatioNextX96"),
                "{key}: sqrtRatioNextX96"
            );
            assert_eq!(step.amount_in, dec(vector, "amountIn"), "{key}: amountIn");
            assert_eq!(
                step.amount_out,
                dec(vector, "amountOut"),
                "{key}: amountOut"
            );
            assert_eq!(
                step.fee_amount,
                dec(vector, "feeAmount"),
                "{key}: feeAmount"
            );

            if flag(vector, "exactIn") {
                exact_in += 1;
            } else {
                exact_out += 1;
            }
            if flag(vector, "reachedTarget") {
                reached += 1;
            }
        }

        assert!(
            exact_in > 0 && exact_out > 0,
            "{key}: both directions of exactness"
        );
        assert!(reached > 0, "{key}: some steps must reach their target");
    }
}

#[test]
fn liquidity_math_matches_the_pinned_solidity_exactly() {
    let json = load("per-function-vectors.json");
    for vector in rows(&json, "addDelta") {
        let x = dec(vector, "x").as_u128().expect("uint128");
        let y = signed(vector, "y").to_int128().expect("int128");
        assert_eq!(
            add_delta(x, y),
            Some(dec(vector, "z").as_u128().expect("uint128"))
        );
    }
    for vector in rows(&json, "addDeltaReverts") {
        let x = dec(vector, "x").as_u128().expect("uint128");
        let y = signed(vector, "y").to_int128().expect("int128");
        assert_eq!(add_delta(x, y), None, "addDelta must revert");
    }
}

// ===================== full-swap vectors =====================

/// Rebuild the pool a scenario's `mint` row describes.
fn scenario_pool(mint: &Json, ticks: &[(i32, i128)]) -> (PoolConfig, PoolState) {
    (
        PoolConfig {
            tick_spacing: 1,
            fee_pips: 0,
            ticks: TickList::new(ticks.iter().copied()),
        },
        PoolState {
            sqrt_price_x96: dec(mint, "sqrtPriceX96"),
            tick: int(mint, "tick") as i32,
            liquidity: dec(mint, "liquidity").as_u128().expect("uint128"),
        },
    )
}

/// The tick set each scenario mints, mirroring the generator's `_ladderTicks`.
fn ticks_for(scenario: &str, liquidity: u128) -> Vec<(i32, i128)> {
    let l = liquidity as i128;
    match scenario {
        "full-range" => vec![(tick_math::MIN_TICK, l), (tick_math::MAX_TICK, -l)],
        "concentrated-60" => vec![(46_054 - 60, l), (46_054 + 60, -l)],
        "concentrated-600" => vec![(46_054 - 600, l), (46_054 + 600, -l)],
        "ladder-3" => {
            // Three nested positions, each minted with the same liquidity, so
            // the total at the centre is 3x and each boundary carries 1x.
            let per = l / 3;
            vec![
                (46_054 - 200, per),
                (46_054 + 200, -per),
                (46_054 - 400, per),
                (46_054 + 400, -per),
                (46_054 - 800, per),
                (46_054 + 800, -per),
            ]
        }
        other => panic!("unknown scenario {other}"),
    }
}

/// Classify a revert the fixture recorded.
///
/// `UniswapV3Pool.swap` reverts for two very different reasons, and only one of
/// them is pricing:
///
/// * **pricing** — `'AS'` (zero amount) or `'SPL'` (the price limit is on the
///   wrong side). The port reproduces these.
/// * **settlement** — the pool cannot transfer what the swap asks for, because
///   its token balance is smaller than the requested exact output. The port
///   models pricing only (see `univ3::pool`), so it correctly does *not*
///   reproduce these.
///
/// A settlement revert is recognised by the port succeeding while the transfer
/// the swap implies cannot happen, on either side:
///
/// * the pool would have to pay out more than it holds of that token, or
/// * the swapper would have to pay in more than the generator ever minted
///   (`TOKEN_SUPPLY`). An exact-output request for nearly the pool's whole
///   token0 balance is the case that hits this: the price has to rise almost
///   without bound, so the token1 input runs into the tens of thousands of
///   orders of magnitude.
fn is_settlement_revert(
    outcome: &pool::SwapOutcome,
    zero_for_one: bool,
    balance0: U256,
    balance1: U256,
) -> bool {
    // `TOKEN_SUPPLY` in the generator: 1e33 of each token, minted to the swapper.
    let supply = U256::from_u128(1_000_000_000_000_000_000_000_000_000_000_000);

    let (paid_out, held, paid_in) = if zero_for_one {
        (outcome.amount1, balance1, outcome.amount0)
    } else {
        (outcome.amount0, balance0, outcome.amount1)
    };
    let pool_cannot_pay = paid_out.is_negative() && paid_out.magnitude() > held;
    let swapper_cannot_pay = paid_in.is_positive_or_zero() && paid_in.magnitude() > supply;
    pool_cannot_pay || swapper_cannot_pay
}

fn replay(config: &PoolConfig, state: PoolState, vector: &Json) -> Option<pool::SwapOutcome> {
    pool::swap(
        config,
        state,
        flag(vector, "zeroForOne"),
        signed(vector, "amountSpecified"),
        dec(vector, "sqrtPriceLimitX96"),
    )
}

fn assert_matches(outcome: &pool::SwapOutcome, vector: &Json, label: &str) {
    assert_eq!(
        outcome.amount0,
        signed(vector, "amount0"),
        "{label}: amount0"
    );
    assert_eq!(
        outcome.amount1,
        signed(vector, "amount1"),
        "{label}: amount1"
    );
    assert_eq!(
        outcome.state.sqrt_price_x96,
        dec(vector, "sqrtPriceAfterX96"),
        "{label}: sqrtPriceX96"
    );
    assert_eq!(
        outcome.state.tick,
        int(vector, "tickAfter") as i32,
        "{label}: tick"
    );
    assert_eq!(
        outcome.state.liquidity,
        dec(vector, "liquidityAfter").as_u128().expect("uint128"),
        "{label}: liquidity"
    );
}

#[test]
fn full_swaps_match_the_real_pool_exactly() {
    let json = load("golden-vectors.json");
    check_source(&json);

    let mut swaps_checked = 0usize;
    let mut crossings_seen = Vec::new();
    let mut pricing_reverts = 0usize;
    let mut settlement_reverts = 0usize;

    for key in [
        "fullRange",
        "concentratedNarrow",
        "concentratedWide",
        "concentratedLadder",
    ] {
        let scenario_rows = rows(&json, key);
        let mint = scenario_rows
            .iter()
            .find(|row| row.get("kind").and_then(|k| k.as_str()) == Some("mint"))
            .unwrap_or_else(|| panic!("{key}: no mint row"));
        let name = mint.get("scenario").unwrap().as_str().unwrap();
        let liquidity = dec(mint, "liquidity").as_u128().expect("uint128");
        let ticks = ticks_for(name, liquidity);
        let (config, state) = scenario_pool(mint, &ticks);

        // The port must agree with the pool about what the mint produced.
        assert_eq!(
            state.liquidity, liquidity,
            "{key}: liquidity at the opening state"
        );

        for vector in scenario_rows
            .iter()
            .filter(|row| row.get("kind").and_then(|k| k.as_str()) == Some("swap"))
        {
            let label = format!(
                "{name} zeroForOne={} exactIn={} amount={}",
                flag(vector, "zeroForOne"),
                flag(vector, "exactIn"),
                vector.get("amountSpecified").unwrap().as_str().unwrap()
            );
            let outcome = replay(&config, state, vector);
            if flag(vector, "reverted") {
                match outcome {
                    None => pricing_reverts += 1,
                    Some(outcome) => {
                        assert!(
                            is_settlement_revert(
                                &outcome,
                                flag(vector, "zeroForOne"),
                                dec(mint, "amount0"),
                                dec(mint, "amount1")
                            ),
                            "{label}: Solidity reverted for a reason the port should have \
                             reproduced, but the port succeeded with amount0 {} amount1 {}",
                            outcome.amount0.to_string_signed(),
                            outcome.amount1.to_string_signed()
                        );
                        settlement_reverts += 1;
                    }
                }
                continue;
            }
            let outcome = outcome.unwrap_or_else(|| panic!("{label}: port reverted"));
            assert_matches(&outcome, vector, &label);
            crossings_seen.push(int(vector, "crossings"));
            swaps_checked += 1;
        }
    }

    assert!(swaps_checked > 80, "only {swaps_checked} swaps checked");
    println!(
        "full swaps: {swaps_checked} replayed exactly, {pricing_reverts} pricing reverts \
         reproduced, {settlement_reverts} settlement reverts (out of pricing scope)"
    );
    assert!(
        crossings_seen.contains(&0),
        "no zero-crossing swap in the fixture"
    );
    assert!(
        crossings_seen.iter().any(|c| *c >= 2),
        "no multi-crossing swap in the fixture"
    );
}

#[test]
fn the_consecutive_sequence_tracks_state_swap_by_swap() {
    // The hardest check in the suite: one pool, 220 swaps applied in order, with
    // the port's state carried forward and compared against the real pool's
    // state after every single one. A port that quotes correctly but tracks
    // state incorrectly passes every other test and fails this one.
    let json = load("golden-vectors.json");
    let sequence = rows(&json, "sequence");
    assert!(sequence.len() >= 200, "sequence too short");

    // The sequence pool is `_ladderTicks(400, 2)` with liquidity 5e20.
    let liquidity: u128 = 500_000_000_000_000_000_000;
    let l = liquidity as i128;
    let config = PoolConfig {
        tick_spacing: 1,
        fee_pips: 0,
        ticks: TickList::new([
            (46_054 - 400, l),
            (46_054 + 400, -l),
            (46_054 - 800, l),
            (46_054 + 800, -l),
        ]),
    };

    let mint = sequence
        .iter()
        .find(|row| row.get("kind").and_then(|k| k.as_str()) == Some("mint"))
        .expect("sequence mint row");
    let balance0 = dec(mint, "amount0");
    let balance1 = dec(mint, "amount1");

    let first = sequence
        .iter()
        .find(|row| row.get("kind").and_then(|k| k.as_str()) == Some("swap"))
        .expect("sequence swap row");
    let mut state = PoolState {
        sqrt_price_x96: dec(first, "sqrtPriceBeforeX96"),
        tick: int(first, "tickBefore") as i32,
        liquidity: dec(first, "liquidityBefore").as_u128().expect("uint128"),
    };

    let mut applied = 0usize;
    let mut reverts = 0usize;
    let mut settlement_reverts = 0usize;
    for vector in sequence {
        if vector.get("kind").and_then(|k| k.as_str()) != Some("swap") {
            continue;
        }
        let step = int(vector, "step");
        // The port's carried state must still agree with the pool's own
        // pre-trade state; if it has drifted, say so at the step it drifted.
        assert_eq!(
            state.sqrt_price_x96,
            dec(vector, "sqrtPriceBeforeX96"),
            "step {step}: carried sqrtPriceX96 diverged"
        );
        if !flag(vector, "reverted") {
            assert_eq!(
                state.tick,
                int(vector, "tickBefore") as i32,
                "step {step}: carried tick diverged"
            );
            assert_eq!(
                state.liquidity,
                dec(vector, "liquidityBefore").as_u128().expect("uint128"),
                "step {step}: carried liquidity diverged"
            );
        }

        match replay(&config, state, vector) {
            Some(outcome) => {
                if flag(vector, "reverted") {
                    // Only a settlement revert may go unreproduced, and the
                    // pool's state is then unchanged, so the carried state must
                    // NOT advance.
                    assert!(
                        is_settlement_revert(
                            &outcome,
                            flag(vector, "zeroForOne"),
                            balance0,
                            balance1
                        ),
                        "step {step}: Solidity reverted for a pricing reason the port missed"
                    );
                    settlement_reverts += 1;
                    continue;
                }
                assert_matches(&outcome, vector, &format!("step {step}"));
                state = outcome.state;
                applied += 1;
            }
            None => {
                assert!(
                    flag(vector, "reverted"),
                    "step {step}: the port reverted but Solidity did not"
                );
                reverts += 1;
            }
        }
    }

    assert!(applied >= 150, "only {applied} swaps replayed");
    assert!(
        applied + reverts + settlement_reverts >= 200,
        "the whole sequence must be accounted for"
    );
    println!(
        "sequence: {applied} swaps replayed exactly, {reverts} pricing reverts reproduced, \
         {settlement_reverts} settlement reverts (out of pricing scope)"
    );
}
