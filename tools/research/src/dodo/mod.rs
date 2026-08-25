//! DODO V2 PMM, ported from the pinned upstream Solidity.
//!
//! Upstream repository: <https://github.com/DODOEX/contractV2>
//! Pinned commit: `8da3ee1ec50966fca9a2c80d424040c45c0f785e`
//!
//! Files read (through the read-only `mantle-propamm-contracts` vendor copy,
//! which documents the same pinned commit):
//! * `src/vendor/dodo/DecimalMath.sol`  -> [`decimal_math`]
//! * `src/vendor/dodo/DODOMath.sol`     -> [`dodo_math`]
//! * `src/vendor/dodo/PMMPricing.sol`   -> [`pmm_pricing`]
//!
//! The state-persistence rule in [`state`] follows
//! `mantle-propamm-contracts/src/MantlePropAmmPool.sol`.

pub mod decimal_math;
pub mod dodo_math;
pub mod pmm_pricing;
pub mod state;

pub use pmm_pricing::{PmmState, RState};
pub use state::{DodoPool, DodoQuery, DodoQueryInput};
