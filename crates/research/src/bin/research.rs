//! Research-only CLI. Separate binary from `prop-amm`, so the challenge CLI's
//! behaviour is untouched.
//!
//! ```text
//! research equilibrium                 # mid price at the unified initial state
//! research quote-matrix                # quotes across order sizes
//! research bench --simulations 20 --steps 1000
//! ```

use std::path::PathBuf;
use std::time::Instant;

use clap::{Parser, Subcommand};

use prop_amm_research::experiment::{self, BatchConfig, Competitor};
use prop_amm_research::metrics::StrategySummary;
use prop_amm_research::paired;
use prop_amm_research::probe;
use prop_amm_research::report::{self, RunMeta};
use prop_amm_research::strategies::{self, Strategy};
use prop_amm_research::wad::price_to_wad;

#[derive(Parser)]
#[command(
    name = "research",
    about = "Oracle-aware AMM curve comparison benchmark (research only)"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Print the mid price of every curve at the unified initial state.
    Equilibrium {
        /// Fair price to publish.
        #[arg(long, default_value = "100")]
        price: f64,
        #[arg(long, default_value = "100")]
        reserve_x: f64,
        #[arg(long, default_value = "10000")]
        reserve_y: f64,
        /// Optional directory for `mid-price.json`.
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Print the quote matrix across several order sizes.
    QuoteMatrix {
        #[arg(long, default_value = "100")]
        price: f64,
        #[arg(long, default_value = "100")]
        reserve_x: f64,
        #[arg(long, default_value = "10000")]
        reserve_y: f64,
        /// Optional directory for `quote-matrix.csv` / `.json`.
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Run the paired benchmark.
    Bench {
        #[arg(long, default_value = "20")]
        simulations: u32,
        #[arg(long, default_value = "1000")]
        steps: u32,
        #[arg(long, default_value = "0")]
        seed_start: u64,
        #[arg(long, default_value = "1")]
        seed_stride: u64,
        /// `paired` (vs the challenge normalizer) or `solo` (no competitor).
        #[arg(long, default_value = "paired")]
        mode: String,
        /// 0 = all available cores.
        #[arg(long, default_value = "0")]
        workers: usize,
        /// Restrict to specific strategy ids (repeatable).
        #[arg(long)]
        strategy: Vec<String>,
        /// Output directory for JSON / CSV / Markdown.
        #[arg(long, default_value = "research-out")]
        out: PathBuf,
    },
}

fn selected_strategies(filter: &[String]) -> anyhow::Result<Vec<Strategy>> {
    if filter.is_empty() {
        return Ok(strategies::all_strategies());
    }
    let mut selected = Vec::new();
    for id in filter {
        let strategy = strategies::strategy_by_id(id)
            .ok_or_else(|| anyhow::anyhow!("unknown strategy id `{id}`"))?;
        selected.push(strategy);
    }
    Ok(selected)
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Equilibrium {
            price,
            reserve_x,
            reserve_y,
            out,
        } => equilibrium(price, reserve_x, reserve_y, out),
        Command::QuoteMatrix {
            price,
            reserve_x,
            reserve_y,
            out,
        } => quote_matrix(price, reserve_x, reserve_y, out),
        Command::Bench {
            simulations,
            steps,
            seed_start,
            seed_stride,
            mode,
            workers,
            strategy,
            out,
        } => bench(
            simulations,
            steps,
            seed_start,
            seed_stride,
            &mode,
            workers,
            &strategy,
            out,
        ),
    }
}

fn equilibrium(
    price: f64,
    reserve_x: f64,
    reserve_y: f64,
    out: Option<PathBuf>,
) -> anyhow::Result<()> {
    let expected =
        price_to_wad(price).ok_or_else(|| anyhow::anyhow!("price {price} cannot be quantised"))?;
    println!("published price (WAD): {expected}");
    println!("inventory: reserveX = {reserve_x}, reserveY = {reserve_y}\n");
    println!("{:<28} {:<12} mid price (WAD)", "strategy", "parameter");

    let mut rows = Vec::new();
    let mut all_equal = true;
    for strategy in strategies::all_strategies() {
        let mid = probe::mid_price_wad(&strategy, price, reserve_x, reserve_y)
            .ok_or_else(|| anyhow::anyhow!("{}: mid price reverted", strategy.id))?;
        all_equal &= mid == expected;
        println!("{:<28} {:<12} {mid}", strategy.id, strategy.parameter);
        rows.push((strategy.id.clone(), mid));
    }
    println!(
        "\nall curves agree with the published price: {}",
        if all_equal { "yes" } else { "NO" }
    );

    if let Some(dir) = out {
        std::fs::create_dir_all(&dir)?;
        let path = dir.join("mid-price.json");
        std::fs::write(&path, report::mid_price_json(&rows, expected))?;
        println!("wrote {}", path.display());
    }
    if !all_equal {
        anyhow::bail!("mid prices disagree at the equilibrium point");
    }
    Ok(())
}

fn quote_matrix(
    price: f64,
    reserve_x: f64,
    reserve_y: f64,
    out: Option<PathBuf>,
) -> anyhow::Result<()> {
    let strategies = strategies::all_strategies();
    let buy_sizes = probe::default_buy_sizes_y();
    let sell_sizes = probe::default_sell_sizes_x();
    let rows = probe::quote_matrix(
        &strategies,
        price,
        reserve_x,
        reserve_y,
        &buy_sizes,
        &sell_sizes,
    );

    for side in ["buy_x", "sell_x"] {
        let sizes: &[f64] = if side == "buy_x" {
            &buy_sizes
        } else {
            &sell_sizes
        };
        println!(
            "\n=== {} (fair price {price}, reserveX {reserve_x}, reserveY {reserve_y}) ===",
            if side == "buy_x" {
                "spend Y, receive X — slippage in bps"
            } else {
                "spend X, receive Y — slippage in bps"
            }
        );
        print!("{:<28}", "strategy");
        for size in sizes {
            print!("{size:>12}");
        }
        println!();
        for strategy in &strategies {
            print!("{:<28}", strategy.id);
            for size in sizes {
                let row = rows
                    .iter()
                    .find(|r| r.strategy_id == strategy.id && r.side == side && r.input == *size);
                match row {
                    Some(row) if row.slippage_bps.is_finite() => {
                        print!("{:>12.2}", row.slippage_bps)
                    }
                    _ => print!("{:>12}", "n/a"),
                }
            }
            println!();
        }
    }

    if let Some(dir) = out {
        std::fs::create_dir_all(&dir)?;
        let csv_path = dir.join("quote-matrix.csv");
        std::fs::write(&csv_path, report::quote_matrix_csv(&rows))?;
        let json_path = dir.join("quote-matrix.json");
        std::fs::write(
            &json_path,
            report::quote_matrix_json(&rows, price, reserve_x, reserve_y),
        )?;
        println!("\nwrote {}", csv_path.display());
        println!("wrote {}", json_path.display());
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn bench(
    simulations: u32,
    steps: u32,
    seed_start: u64,
    seed_stride: u64,
    mode: &str,
    workers: usize,
    strategy_filter: &[String],
    out: PathBuf,
) -> anyhow::Result<()> {
    let competitor = Competitor::parse(mode)
        .ok_or_else(|| anyhow::anyhow!("unknown mode `{mode}` (expected `paired` or `solo`)"))?;
    let strategies = selected_strategies(strategy_filter)?;
    let batch = BatchConfig {
        simulations,
        steps,
        seed_start,
        seed_stride,
        competitor,
        workers,
    };

    println!(
        "running {} strategies x {} simulations x {} steps (mode {}, workers {})",
        strategies.len(),
        simulations,
        steps,
        competitor.as_str(),
        if workers == 0 {
            rayon::current_num_threads()
        } else {
            workers
        }
    );

    let start = Instant::now();
    let results = experiment::run_all(&strategies, &batch)?;
    let elapsed = start.elapsed().as_secs_f64();

    let mut summaries: Vec<StrategySummary> = Vec::new();
    let mut all_runs = Vec::new();
    for (strategy, runs) in &results {
        summaries.push(StrategySummary::from_runs(
            &strategy.id,
            strategy.family.as_str(),
            &strategy.parameter,
            runs,
        ));
        all_runs.extend(runs.iter().cloned());
    }

    let flashbots_minus_dodo = paired::flashbots_minus_dodo(&results);
    let versus_univ2 = paired::versus_univ2(&results);
    let attributions = paired::attribute_versus_univ2(&results);

    let meta = RunMeta::from_batch(&batch, elapsed);
    let mut written = report::write_all(
        &out,
        &meta,
        &summaries,
        &all_runs,
        &flashbots_minus_dodo,
        &versus_univ2,
        &attributions,
    )?;

    // Keep each result directory self-contained: the static probes describe the
    // curves the numbers came from.
    let mid_rows: Vec<(String, prop_amm_research::u256::U256)> = strategies
        .iter()
        .filter_map(|strategy| {
            probe::mid_price_wad(strategy, meta.initial_price, meta.initial_x, meta.initial_y)
                .map(|mid| (strategy.id.clone(), mid))
        })
        .collect();
    if let Some(expected) = price_to_wad(meta.initial_price) {
        let path = out.join("mid-price.json");
        std::fs::write(&path, report::mid_price_json(&mid_rows, expected))?;
        written.push(path);
    }
    let quote_rows = probe::quote_matrix(
        &strategies,
        meta.initial_price,
        meta.initial_x,
        meta.initial_y,
        &probe::default_buy_sizes_y(),
        &probe::default_sell_sizes_x(),
    );
    let path = out.join("quote-matrix.csv");
    std::fs::write(&path, report::quote_matrix_csv(&quote_rows))?;
    written.push(path);
    let path = out.join("quote-matrix.json");
    std::fs::write(
        &path,
        report::quote_matrix_json(
            &quote_rows,
            meta.initial_price,
            meta.initial_x,
            meta.initial_y,
        ),
    )?;
    written.push(path);

    println!(
        "\n{:<28} {:>14} {:>14} {:>14} {:>10}",
        "strategy", "netEdge mean", "retailEdge", "arbEdge", "net>0 rate"
    );
    let mut ranked: Vec<&StrategySummary> = summaries.iter().collect();
    ranked.sort_by(|a, b| {
        b.net_edge
            .mean
            .partial_cmp(&a.net_edge.mean)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    for summary in ranked {
        println!(
            "{:<28} {:>14.4} {:>14.4} {:>14.4} {:>9.1}%",
            summary.strategy_id,
            summary.net_edge.mean,
            summary.retail_edge.mean,
            summary.arbitrage_edge.mean,
            summary.positive_net_rate * 100.0
        );
    }
    let net_deltas: Vec<&paired::PairedDelta> = flashbots_minus_dodo
        .iter()
        .filter(|d| d.metric == "netEdge")
        .collect();
    if !net_deltas.is_empty() {
        println!(
            "\npaired netEdge delta (Flashbots - DODO), per pairing row, 95% CI on per-seed differences:"
        );
        println!(
            "{:<10} {:>12} {:>26} {:>8} {:>10} verdict",
            "pairing", "mean", "95% CI", "t", "win rate"
        );
        for delta in net_deltas {
            println!(
                "{:<10} {:>12.5} {:>12.5} {:>12.5} {:>8.2} {:>9.1}% {}",
                delta
                    .pairing_index
                    .map(|i| format!("#{i}"))
                    .unwrap_or_else(|| "-".to_string()),
                delta.mean,
                delta.ci95_low,
                delta.ci95_high,
                delta.t_stat,
                delta.paired_win_rate * 100.0,
                if delta.is_significant() {
                    "significant"
                } else {
                    "not distinguishable"
                }
            );
        }
    }

    let total_reverts: u64 = summaries.iter().map(|s| s.total_curve_reverts).sum();
    if total_reverts == 0 {
        println!(
            "\ncurve reverts: 0 (the ported pricing functions never hit a revert branch; \
this is not a claim that a full on-chain swap would succeed \u{2014} pool-level guards are out of scope)"
        );
    } else {
        println!(
            "\nWARNING: {total_reverts} curve reverts — the ledger and a curve's integer state disagreed; \
             see the curve_reverts column in summary.csv before trusting these numbers"
        );
    }
    println!("finished in {elapsed:.1}s");
    for path in written {
        println!("wrote {}", path.display());
    }
    Ok(())
}
