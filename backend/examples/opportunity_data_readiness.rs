//! Offline aggregate evidence-readiness measurement.
#[path = "opportunity_data_readiness/mod.rs"]
mod readiness;

use std::path::PathBuf;
use ticker_tape_tally_board_backend::{clock::Clock, db, demo, ledger_analysis};

#[tokio::main]
async fn main() -> std::process::ExitCode {
    // Repositories can log identifying decode context. This offline process must not.
    log::set_max_level(log::LevelFilter::Off);
    match run().await {
        Ok(output) => {
            print!("{output}");
            std::process::ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("{message}");
            std::process::ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<String, &'static str> {
    let mut demo_mode = false;
    let mut path = None;
    let clock = Clock::System;
    let mut as_of = None;
    let mut args = std::env::args_os().skip(1);
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--demo") if !demo_mode => demo_mode = true,
            Some("--as-of") => {
                let date = args.next().ok_or("--as-of requires YYYY-MM-DD.")?;
                as_of = Some(date.to_str().and_then(|s| s.parse().ok()).ok_or("Invalid --as-of date; use YYYY-MM-DD.")?);
            }
            Some("--ledger") if path.is_none() => path = Some(PathBuf::from(args.next().ok_or("--ledger requires a scratch snapshot path.")?)),
            Some(s) if s.starts_with('-') => return Err("Usage: opportunity_data_readiness (--demo | --ledger <scratch snapshot>) [--as-of YYYY-MM-DD]"),
            _ if path.is_none() => path = Some(PathBuf::from(arg)),
            _ => return Err("Specify exactly one scratch ledger path or --demo."),
        }
    }
    if demo_mode == path.is_some() {
        return Err("Specify exactly one scratch ledger path or --demo.");
    }
    let demo_clock = as_of.map(Clock::fixed).unwrap_or_else(|| clock.clone());
    let pool = if demo_mode {
        let pool = db::memory_pool()
            .await
            .map_err(|_| "Cannot create in-memory demo fixture.")?;
        demo::seed(&pool, demo_clock.today())
            .await
            .map_err(|_| "Cannot seed in-memory demo fixture.")?;
        pool
    } else {
        ledger_analysis::open_scratch_ledger(&path.expect("validated path")).await?
    };
    let date = if demo_mode {
        Ok(demo_clock.today())
    } else {
        readiness::ledger_measurement_date(&pool, &clock, as_of).await
    };
    let measured = match date {
        Ok(date) => {
            // The offset is safe metadata, and remains visible even if inventory loading fails.
            print!("{}", readiness::date_offset(&clock, date));
            if demo_mode {
                readiness::measure(&pool, &demo_clock).await
            } else {
                readiness::measure_as_of(&pool, &clock, date).await
            }
        }
        Err(message) => {
            pool.close().await;
            return Err(message);
        }
    };
    pool.close().await;
    measured.map(|summary| summary.render())
}
