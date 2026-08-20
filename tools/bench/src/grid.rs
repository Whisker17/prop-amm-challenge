use prop_amm_shared::config::SimulationConfig;

use crate::config::GridConfig;

/// Grid mode's own seed addressing scheme — not a `config/bench.toml` segment (that file's
/// header says so explicitly), and not a tunable value either: it's the formula, not a
/// level or a count, so it stays code (docs/DESIGN.md §2.3). Cell `c` draws seeds
/// `GRID_SEED_BASE + c*1_000 + i`.
pub const GRID_SEED_BASE: u64 = 4_000_000;

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
    /// This cell's `seeds_per_cell` simulation configs. `retail_arrival_rate` and
    /// `retail_mean_size` are left at `base`'s own values untouched (docs/DESIGN.md §2.3:
    /// held at `SimulationConfig::default()` to keep the factorial readable) — only the
    /// three grid axes and the seed are overridden.
    pub fn configs(&self, base: &SimulationConfig, seeds_per_cell: u64) -> Vec<SimulationConfig> {
        (0..seeds_per_cell)
            .map(|i| SimulationConfig {
                seed: GRID_SEED_BASE + (self.index as u64) * 1_000 + i,
                norm_fee_bps: self.norm_fee_bps,
                norm_liquidity_mult: self.norm_liquidity_mult,
                gbm_sigma: self.gbm_sigma,
                ..base.clone()
            })
            .collect()
    }

    /// Human-readable axis values, for report tables and progress lines.
    pub fn label_axes(&self) -> String {
        format!(
            "fee={}bps liq={:.1}x sigma={:.4}",
            self.norm_fee_bps, self.norm_liquidity_mult, self.gbm_sigma
        )
    }
}

/// The cells of `config`'s factorial, in a fixed, documented enumeration order: fee outer,
/// liquidity middle, sigma inner (`index = fee_idx*n_liq*n_sigma + liq_idx*n_sigma +
/// sigma_idx`). The order itself is an implementation choice — what matters is that it's
/// fixed, so `index` (and therefore each cell's seed block) is stable across runs.
pub fn cells(config: &GridConfig) -> Vec<GridCell> {
    let n_liq = config.norm_liquidity_mult_levels.len();
    let n_sigma = config.gbm_sigma_levels.len();
    let mut out = Vec::with_capacity(config.norm_fee_bps_levels.len() * n_liq * n_sigma);
    for (fee_idx, &norm_fee_bps) in config.norm_fee_bps_levels.iter().enumerate() {
        for (liq_idx, &norm_liquidity_mult) in config.norm_liquidity_mult_levels.iter().enumerate()
        {
            for (sigma_idx, &gbm_sigma) in config.gbm_sigma_levels.iter().enumerate() {
                let index = fee_idx * (n_liq * n_sigma) + liq_idx * n_sigma + sigma_idx;
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

    fn sample_config() -> GridConfig {
        GridConfig {
            norm_fee_bps_levels: vec![30, 55, 80],
            norm_liquidity_mult_levels: vec![0.4, 1.0, 2.0],
            gbm_sigma_levels: vec![1e-4, 1e-3, 7e-3],
            seeds_per_cell: 40,
        }
    }

    #[test]
    fn cells_covers_the_full_27_cell_factorial() {
        let config = sample_config();
        let cells = cells(&config);
        assert_eq!(cells.len(), 27);

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
        let configs = cell.configs(&SimulationConfig::default(), 40);
        assert_eq!(configs.len(), 40);
        for (i, config) in configs.iter().enumerate() {
            assert_eq!(config.seed, 4_000_000 + 5 * 1_000 + i as u64);
        }
    }

    #[test]
    fn cell_configs_leave_retail_axes_at_base_defaults() {
        let base = SimulationConfig::default();
        let cell = cells(&sample_config())[0];
        let configs = cell.configs(&base, 40);
        for config in &configs {
            assert_eq!(config.retail_arrival_rate, base.retail_arrival_rate);
            assert_eq!(config.retail_mean_size, base.retail_mean_size);
        }
    }
}
