//! Offline, aggregate-only calibration replay. Never refreshes or contacts providers.
#[path = "dip_check_replay/mod.rs"]
mod replay;

use std::{path::PathBuf, process::ExitCode};
use ticker_tape_tally_board_backend::{clock::Clock, db, demo, ledger_analysis};

#[tokio::main]
async fn main() -> ExitCode {
    log::set_max_level(log::LevelFilter::Off);
    // Domain/repository panics may carry data. No panic payload or location escapes.
    std::panic::set_hook(Box::new(|_| {
        eprintln!("Replay failed; no ledger details are disclosed.")
    }));
    match run().await {
        Ok(output) => {
            print!("{output}");
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<String, &'static str> {
    replay::validate_parameters()?;
    let mut demo_mode = false;
    let mut truncated = false;
    let mut path = None;
    let mut as_of = None;
    let mut args = std::env::args_os().skip(1);
    const USAGE: &str = "Usage: dip_check_replay (--demo [--truncated] | --ledger <scratch snapshot>) [--as-of YYYY-MM-DD]";
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--demo") if !demo_mode => demo_mode = true,
            Some("--truncated") if !truncated => truncated = true,
            Some("--ledger") if path.is_none() => {
                path = Some(PathBuf::from(args.next().ok_or(USAGE)?))
            }
            Some("--as-of") if as_of.is_none() => {
                as_of = Some(
                    args.next()
                        .and_then(|a| a.to_str().and_then(|s| s.parse().ok()))
                        .ok_or(USAGE)?,
                )
            }
            Some(s) if s.starts_with('-') => return Err(USAGE),
            _ if path.is_none() => path = Some(PathBuf::from(arg)),
            _ => return Err(USAGE),
        }
    }
    if demo_mode == path.is_some() || (truncated && !demo_mode) {
        return Err(USAGE);
    }
    if !demo_mode && as_of.is_none() {
        return Err("A prepared-copy replay requires --as-of set to its preparation refresh date.");
    }
    let today = as_of.unwrap_or_else(|| Clock::System.today());
    let inputs = if demo_mode {
        let pool = db::memory_pool()
            .await
            .map_err(|_| "Cannot create demo fixture.")?;
        let seeded = demo::seed(&pool, today)
            .await
            .map_err(|_| "Cannot seed demo fixture.");
        let inputs = match seeded {
            Ok(_) => ledger_analysis::replay_inputs::load(&pool, today).await,
            Err(message) => Err(message),
        };
        pool.close().await;
        replay::demo_fixture(inputs?, today, truncated)?
    } else {
        let pool = ledger_analysis::open_scratch_ledger(&path.ok_or(USAGE)?).await?;
        let loaded = ledger_analysis::replay_inputs::load(&pool, today).await;
        pool.close().await;
        loaded?
    };
    replay::run(&inputs, today, !demo_mode)
}
