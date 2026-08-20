use prop_amm_shared::config::SimulationConfig;

/// Grid mode's factorial axes (docs/DESIGN.md §2.3): endpoints + a representative midpoint
/// of each of upstream's own sampling ranges (`HyperparameterVariance::default()`).
pub const NORM_FEE_BPS_LEVELS: [u16; 3] = [30, 55, 80];
pub const NORM_LIQUIDITY_MULT_LEVELS: [f64; 3] = [0.4, 1.0, 2.0];
pub const GBM_SIGMA_LEVELS: [f64; 3] = [1e-4, 1e-3, 7e-3];

pub const SEEDS_PER_CELL: u64 = 40;
/// Grid mode's own seed addressing scheme — not a `config/bench.toml` segment (that file's
/// header says so explicitly). Cell `c` draws seeds `GRID_SEED_BASE + c*1_000 + i`.
pub const GRID_SEED_BASE: u64 = 4_000_000;

pub const N_CELLS: usize =
    NORM_FEE_BPS_LEVELS.len() * NORM_LIQUIDITY_MULT_LEVELS.len() * GBM_SIGMA_LEVELS.len();

/// One corner of the fragility matrix. `index` is this cell's position in `cells()`'s fixed
/// enumeration order (fee outer, liquidity middle, sigma inner) — the seed formula's `c`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GridCell {
    pub index: usize,
    pub norm_fee_bps: u16,
    pub norm_liquidity_mult: f64,
    pub gbm_sigma: f64,
}

impl GridCell {
    /// This cell's `SEEDS_PER_CELL` simulation configs. `retail_arrival_rate` and
    /// `retail_mean_size` are left at `base`'s own values untouched (docs/DESIGN.md §2.3:
    /// held at `SimulationConfig::default()` to keep the factorial readable) — only the
    /// three grid axes and the seed are overridden.
    pub fn configs(&self, base: &SimulationConfig) -> Vec<SimulationConfig> {
        (0..SEEDS_PER_CELL)
            .map(|i| SimulationConfig {
                seed: GRID_SEED_BASE + (self.index as u64) * 1_000 + i,
                norm_fee_bps: self.norm_fee_bps,
                norm_liquidity_mult: self.norm_liquidity_mult,
                gbm_sigma: self.gbm_sigma,
                ..base.clone()
            })
            .collect()
    }
}

/// The 27 cells, in a fixed, documented enumeration order: fee outer, liquidity middle,
/// sigma inner (`index = fee_idx*9 + liquidity_idx*3 + sigma_idx`). The order itself is an
/// implementation choice — what matters is that it's fixed, so `index` (and therefore each
/// cell's seed block) is stable across runs.
pub fn cells() -> Vec<GridCell> {
    let mut out = Vec::with_capacity(N_CELLS);
    for (fee_idx, &norm_fee_bps) in NORM_FEE_BPS_LEVELS.iter().enumerate() {
        for (liq_idx, &norm_liquidity_mult) in NORM_LIQUIDITY_MULT_LEVELS.iter().enumerate() {
            for (sigma_idx, &gbm_sigma) in GBM_SIGMA_LEVELS.iter().enumerate() {
                let index = fee_idx * (NORM_LIQUIDITY_MULT_LEVELS.len() * GBM_SIGMA_LEVELS.len())
                    + liq_idx * GBM_SIGMA_LEVELS.len()
                    + sigma_idx;
                out.push(GridCell {
                    index,
                    norm_fee_bps,
                    norm_liquidity_mult,
                    gbm_sigma,
                });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn cells_covers_the_full_27_cell_factorial() {
        let cells = cells();
        assert_eq!(cells.len(), 27);
        assert_eq!(N_CELLS, 27);

        let indices: HashSet<usize> = cells.iter().map(|c| c.index).collect();
        let expected: HashSet<usize> = (0..27).collect();
        assert_eq!(indices, expected);

        let combos: HashSet<(u16, u64, u64)> = cells
            .iter()
            .map(|c| {
                (
                    c.norm_fee_bps,
                    c.norm_liquidity_mult.to_bits(),
                    c.gbm_sigma.to_bits(),
                )
            })
            .collect();
        assert_eq!(combos.len(), 27, "every axis combination must be distinct");
    }

    #[test]
    fn cell_seeds_follow_the_documented_formula() {
        let cell = GridCell {
            index: 5,
            norm_fee_bps: 55,
            norm_liquidity_mult: 1.0,
            gbm_sigma: 1e-3,
        };
        let configs = cell.configs(&SimulationConfig::default());
        assert_eq!(configs.len(), 40);
        for (i, config) in configs.iter().enumerate() {
            assert_eq!(config.seed, 4_000_000 + 5 * 1_000 + i as u64);
        }
    }

    #[test]
    fn cell_configs_leave_retail_axes_at_base_defaults() {
        let base = SimulationConfig::default();
        let cell = cells()[0];
        let configs = cell.configs(&base);
        for config in &configs {
            assert_eq!(config.retail_arrival_rate, base.retail_arrival_rate);
            assert_eq!(config.retail_mean_size, base.retail_mean_size);
        }
    }

    #[test]
    fn grid_seed_range_is_disjoint_from_every_declared_segment() {
        // config/bench.toml's declared segments top out at test = 3_000_000..=3_000_999.
        // Grid's own block starts at 4_000_000 and runs for 27*1_000 seeds — comfortably
        // clear, but this pins that invariant rather than leaving it to eyeballing the
        // constants across two files.
        let declared_segments_max = 3_000_999_u64;
        let grid_min_seed = GRID_SEED_BASE;
        let grid_max_seed = GRID_SEED_BASE + (N_CELLS as u64 - 1) * 1_000 + (SEEDS_PER_CELL - 1);
        assert!(grid_min_seed > declared_segments_max);
        assert!(grid_max_seed >= grid_min_seed);
    }
}
