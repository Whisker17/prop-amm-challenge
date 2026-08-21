//! Exactness tests for the hand-rolled `uint256` used by every Solidity port.
//!
//! Reference values in `tests/fixtures/u256/arith-vectors.json` were produced by
//! Python arbitrary-precision integers (see `tests/fixtures/u256/SOURCES.md`),
//! i.e. by an implementation completely independent of this crate.

use prop_amm_research::json::Json;
use prop_amm_research::u256::U256;

fn fixture() -> Json {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/u256/arith-vectors.json"
    );
    let text = std::fs::read_to_string(path).expect("read u256 fixture");
    Json::parse(&text).expect("parse u256 fixture")
}

fn hex(value: &Json) -> U256 {
    U256::from_hex_str(value.as_str().expect("hex string")).expect("hex fits in 256 bits")
}

fn opt_hex(value: &Json) -> Option<U256> {
    if value.is_null() {
        None
    } else {
        Some(hex(value))
    }
}

#[test]
fn matches_python_bigint_reference_for_every_operation() {
    let json = fixture();
    let cases = json.get("cases").unwrap().as_array().unwrap();
    assert!(cases.len() >= 1_500, "expected a broad vector set");

    for (idx, case) in cases.iter().enumerate() {
        let a = hex(case.get("a").unwrap());
        let b = hex(case.get("b").unwrap());
        let ctx = |op: &str| format!("case {idx} ({op}): a={a} b={b}");

        assert_eq!(
            a.checked_add(b),
            opt_hex(case.get("add").unwrap()),
            "{}",
            ctx("add")
        );
        assert_eq!(
            a.checked_sub(b),
            opt_hex(case.get("sub").unwrap()),
            "{}",
            ctx("sub")
        );
        assert_eq!(
            a.checked_mul(b),
            opt_hex(case.get("mul").unwrap()),
            "{}",
            ctx("mul")
        );
        assert_eq!(
            a.wrapping_mul(b),
            hex(case.get("wmul").unwrap()),
            "{}",
            ctx("wrapping_mul")
        );
        assert_eq!(
            a.checked_div(b),
            opt_hex(case.get("div").unwrap()),
            "{}",
            ctx("div")
        );
        assert_eq!(
            a.checked_rem(b),
            opt_hex(case.get("rem").unwrap()),
            "{}",
            ctx("rem")
        );

        // The DODO `_sqrt` Babylonian loop agrees with floor(sqrt(a)) for every
        // input except a = 2, where the upstream loop never executes and
        // returns 2. That quirk is preserved on purpose (see `dodo_math::sqrt`).
        let expected_sqrt = if a == U256::from_u64(2) {
            U256::from_u64(2)
        } else {
            hex(case.get("isqrt").unwrap())
        };
        assert_eq!(
            prop_amm_research::dodo::dodo_math::sqrt(a),
            expected_sqrt,
            "{}",
            ctx("sqrt")
        );
    }
}

#[test]
fn decimal_round_trip_is_exact() {
    let json = fixture();
    let cases = json.get("cases").unwrap().as_array().unwrap();
    for case in cases.iter().take(400) {
        let a = hex(case.get("a").unwrap());
        let text = a.to_string();
        assert_eq!(
            U256::from_dec_str(&text).unwrap(),
            a,
            "decimal round-trip {text}"
        );
    }
    assert_eq!(U256::from_dec_str("0").unwrap(), U256::ZERO);
    assert_eq!(
        U256::from_dec_str(
            "115792089237316195423570985008687907853269984665640564039457584007913129639935"
        )
        .unwrap(),
        U256::MAX
    );
    assert!(U256::from_dec_str(
        "115792089237316195423570985008687907853269984665640564039457584007913129639936"
    )
    .is_none());
    assert!(U256::from_dec_str("").is_none());
    assert!(U256::from_dec_str("12x").is_none());
}

#[test]
fn ordering_is_by_magnitude_not_limb_order() {
    // Regression guard: a derived `Ord` on little-endian limbs would compare the
    // least-significant limb first and get this backwards.
    let big = U256::from_dec_str("18446744073709551616").unwrap(); // 2^64
    let small = U256::from_u64(u64::MAX);
    assert!(big > small);
    assert!(small < big);
    assert_eq!(big.max(small), big);
}

#[test]
fn division_by_zero_is_rejected() {
    assert_eq!(U256::from_u64(7).checked_div(U256::ZERO), None);
    assert_eq!(U256::from_u64(7).checked_rem(U256::ZERO), None);
}
