//! The Rust DODO port must reproduce every pinned upstream golden vector
//! integer-for-integer.
//!
//! Fixtures are verbatim copies from the read-only `mantle-propamm-contracts`
//! repository (see `tests/fixtures/dodo/SOURCES.md`); no expected value in this
//! file is produced by the Rust implementation.

use prop_amm_research::dodo::decimal_math::ONE;
use prop_amm_research::dodo::state::DodoQueryInput;
use prop_amm_research::dodo::RState;
use prop_amm_research::json::Json;
use prop_amm_research::u256::U256;

fn vectors() -> Json {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/dodo/golden-vectors.json"
    );
    let text = std::fs::read_to_string(path).expect("read dodo golden vectors");
    Json::parse(&text).expect("parse dodo golden vectors")
}

fn field(vector: &Json, name: &str) -> U256 {
    let raw = vector
        .get(name)
        .unwrap_or_else(|| panic!("missing field {name}"))
        .as_num_str()
        .unwrap_or_else(|| panic!("field {name} is not a number"));
    U256::from_dec_str(raw).unwrap_or_else(|| panic!("field {name} does not fit in uint256"))
}

fn u64_field(vector: &Json, name: &str) -> u64 {
    vector
        .get(name)
        .unwrap_or_else(|| panic!("missing field {name}"))
        .as_u64()
        .unwrap_or_else(|| panic!("field {name} is not a small integer"))
}

fn input_of(vector: &Json) -> DodoQueryInput {
    DodoQueryInput {
        i: field(vector, "i"),
        k: field(vector, "k"),
        b: field(vector, "B"),
        q: field(vector, "Q"),
        b0: field(vector, "B0"),
        q0: field(vector, "Q0"),
        r_state: RState::from_u8(u64_field(vector, "R") as u8).expect("valid RState"),
        sell_base: vector.get("sellBase").unwrap().as_bool().expect("sellBase"),
        amount_in: field(vector, "amountIn"),
        lp_fee_rate: field(vector, "lpFeeRate"),
    }
}

#[test]
fn port_matches_every_pinned_upstream_vector_exactly() {
    let json = vectors();
    let count = json.get("count").unwrap().as_u64().unwrap();
    assert_eq!(count, 19, "the pinned fixture carries 19 vectors");

    let all = json.get("vectors").unwrap().as_array().unwrap();
    assert_eq!(all.len() as u64, count);

    for vector in all {
        let name = vector.get("name").unwrap().as_str().unwrap();
        let input = input_of(vector);
        let result = input
            .query_with_fee()
            .unwrap_or_else(|| panic!("{name}: query reverted"));

        assert_eq!(
            result.gross_amount_out,
            field(vector, "grossAmountOut"),
            "{name}: grossAmountOut"
        );
        assert_eq!(
            result.lp_fee_amount,
            field(vector, "lpFeeAmount"),
            "{name}: lpFeeAmount"
        );
        assert_eq!(result.amount_out, field(vector, "amountOut"), "{name}: amountOut");
        assert_eq!(
            U256::from_u64(result.new_r.as_u8() as u64),
            field(vector, "newR"),
            "{name}: newR"
        );
        assert_eq!(result.adjusted_b0, field(vector, "adjustedB0"), "{name}: adjustedB0");
        assert_eq!(result.adjusted_q0, field(vector, "adjustedQ0"), "{name}: adjustedQ0");
        assert_eq!(
            input.mid_price().unwrap_or_else(|| panic!("{name}: midPrice reverted")),
            field(vector, "midPrice"),
            "{name}: midPrice"
        );
    }
}

#[test]
fn fixture_still_covers_every_required_branch() {
    // Mirrors the coverage assertions in `test/unit/DodoPmmGolden.t.sol` so that a
    // future fixture refresh cannot silently drop a branch this port relies on.
    let json = vectors();
    let all = json.get("vectors").unwrap().as_array().unwrap();

    let mut r_state_mask = 0u32;
    let mut direction_mask = 0u32;
    let mut boundary_mask = 0u32;
    let mut mid_price_coverage = 0u32;
    let mut saw_k_zero = false;
    let mut saw_k_one = false;
    let mut saw_k_interior = false;
    let mut saw_adjusted_b0 = false;
    let mut saw_adjusted_q0 = false;
    let mut saw_one_unit = false;
    let mut saw_zero_fee = false;
    let mut saw_interior_fee = false;
    let mut saw_max_fee = false;
    let mut saw_one_unit_fee_rounding = false;
    let mut saw_above_sell_base_k_zero = false;
    let mut saw_below_sell_quote_k_zero = false;

    for vector in all {
        let r_state = u64_field(vector, "R") as u32;
        let k = field(vector, "k");
        let sell_base = vector.get("sellBase").unwrap().as_bool().unwrap();
        let boundary = u64_field(vector, "boundaryRelation") as u32;
        let new_r = u64_field(vector, "newR") as u32;
        let lp_fee_rate = field(vector, "lpFeeRate");
        let gross = field(vector, "grossAmountOut");
        let fee = field(vector, "lpFeeAmount");
        let net = field(vector, "amountOut");

        let k_class = if k.is_zero() {
            0
        } else if k == ONE {
            2
        } else {
            1
        };
        mid_price_coverage |= 1 << (r_state * 3 + k_class);
        r_state_mask |= 1 << r_state;
        direction_mask |= if sell_base { 1 } else { 2 };
        if boundary != 0 {
            boundary_mask |= 1 << (boundary - 1);
        }
        saw_k_zero |= k.is_zero();
        saw_k_one |= k == ONE;
        saw_k_interior |= !k.is_zero() && k < ONE;
        saw_adjusted_b0 |= field(vector, "B0") != field(vector, "adjustedB0");
        saw_adjusted_q0 |= field(vector, "Q0") != field(vector, "adjustedQ0");
        saw_one_unit |= vector.get("oneUnit").unwrap().as_bool().unwrap();
        saw_zero_fee |= lp_fee_rate.is_zero();
        saw_interior_fee |= !lp_fee_rate.is_zero() && lp_fee_rate < ONE.checked_sub(U256::ONE).unwrap();
        saw_max_fee |= lp_fee_rate == ONE.checked_sub(U256::ONE).unwrap();
        saw_one_unit_fee_rounding |=
            gross == U256::ONE && !lp_fee_rate.is_zero() && fee.is_zero() && net == U256::ONE;
        saw_above_sell_base_k_zero |=
            r_state == RState::AboveOne.as_u8() as u32 && sell_base && k.is_zero() && new_r == r_state;
        saw_below_sell_quote_k_zero |=
            r_state == RState::BelowOne.as_u8() as u32 && !sell_base && k.is_zero() && new_r == r_state;

        assert_eq!(net, gross.checked_sub(fee).unwrap(), "amountOut = gross - fee");
    }

    assert_eq!(r_state_mask, 7, "all three R states");
    assert_eq!(direction_mask, 3, "both trade directions");
    assert_eq!(boundary_mask, 7, "below / equal / above return-to-one");
    assert_eq!(mid_price_coverage, 0x1ff, "R state x K mid-price matrix");
    assert!(saw_k_zero, "K = 0");
    assert!(saw_k_one, "K = 1e18");
    assert!(saw_k_interior, "interior K");
    assert!(saw_adjusted_b0, "adjusted B0");
    assert!(saw_adjusted_q0, "adjusted Q0");
    assert!(saw_one_unit, "one-unit rounding");
    assert!(saw_zero_fee, "zero fee");
    assert!(saw_interior_fee, "interior fee");
    assert!(saw_max_fee, "maximum-valid fee boundary");
    assert!(saw_one_unit_fee_rounding, "one-unit fee rounding");
    assert!(saw_above_sell_base_k_zero, "ABOVE_ONE sell-base K=0 GeneralIntegrate");
    assert!(saw_below_sell_quote_k_zero, "BELOW_ONE sell-quote K=0 GeneralIntegrate");
}

#[test]
fn boundary_vectors_transition_exactly_as_the_solidity_state_machine() {
    let json = vectors();
    for vector in json.get("vectors").unwrap().as_array().unwrap() {
        let boundary = u64_field(vector, "boundaryRelation");
        if boundary == 0 {
            continue;
        }
        let name = vector.get("name").unwrap().as_str().unwrap();
        let sell_base = vector.get("sellBase").unwrap().as_bool().unwrap();
        let r_state = u64_field(vector, "R") as u8;
        let expected = if sell_base {
            assert_eq!(r_state, RState::AboveOne.as_u8(), "{name}: sell-base boundary starts ABOVE_ONE");
            match boundary {
                1 => RState::AboveOne,
                2 => RState::One,
                _ => RState::BelowOne,
            }
        } else {
            assert_eq!(r_state, RState::BelowOne.as_u8(), "{name}: sell-quote boundary starts BELOW_ONE");
            match boundary {
                1 => RState::BelowOne,
                2 => RState::One,
                _ => RState::AboveOne,
            }
        };
        let result = input_of(vector).query_with_fee().expect("query");
        assert_eq!(result.new_r, expected, "{name}: boundary transition");
    }
}
