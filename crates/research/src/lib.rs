//! Research-only crate: oracle-aware AMM curve comparison benchmark.
//!
//! This crate exists to compare curve designs under identical external quotes,
//! identical initial capital, identical order flow and identical seeds. It is
//! **not** part of the challenge submission path: it adds no behaviour to
//! `prop-amm-shared`, `prop-amm-executor`, `prop-amm-sim` or the `prop-amm` CLI,
//! and it reuses the simulation's price process, retail trader, arbitrageur and
//! order router without modification.
//!
//! Ground rules enforced throughout:
//!
//! * Curve maths are literal ports of pinned Solidity sources. Integer operation
//!   order, `floor`/`ceil` choices, special-case branches and state transitions
//!   are preserved; formulas are never rewritten or simplified.
//! * No `f64` inside any curve. The oracle price is quantised to a WAD integer
//!   exactly once, at the boundary ([`wad`]), and everything below that boundary
//!   is fixed-point integer arithmetic on [`u256::U256`].

pub mod curves;
pub mod dodo;
pub mod experiment;
pub mod flashbots;
pub mod json;
pub mod metrics;
pub mod probe;
pub mod report;
pub mod strategies;
pub mod u256;
pub mod univ2;
pub mod wad;
