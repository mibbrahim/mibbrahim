//! hsnipe — pump.fun paper/live trading bot.
//!
//!   hsnipe [run]                         stream from Yellowstone gRPC (paper books + optional live)
//!   hsnipe replay FILE [FILE...]         replay recorded/backfilled events through the same rules
//!   hsnipe backfill --hours 24 --out F   fetch history over RPC into an event file
//!   hsnipe backfill --from S --to S --out F
//!   hsnipe simtest [--slot S] [--n 3]    simulate cloned buy/sell txs on mainnet (no funds move)
//!   hsnipe config                        print the effective configuration
//!
//! Strategy is picked with HS_MODE=powerful|powerbot|gymer; every threshold is an env var.

mod backfill;
mod coin;
mod config;
mod decode;
mod engine;
mod grpc;
mod live;
mod logger;
mod model;
mod paper;
mod price;
mod pump;
mod rpc;
mod selftest;
mod strategy;
mod tx;
mod types;

use anyhow::Context;
use config::Config;
use decode::Decoder;
use engine::{Engine, Msg, PoolWatch};
use logger::Logger;
use model::Event;
use std::io::BufRead;
use std::sync::Arc;
use std::time::{Duration, Instant};

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("run");
    match cmd {
        "run" => run(),
        "replay" => replay(&args[1..]),
        "backfill" => backfill_cmd(&args[1..]),
        "simtest" => {
            let a = &args[1..];
            let get = |k: &str| a.iter().position(|x| x == k).and_then(|i| a.get(i + 1)).cloned();
            let url = get("--rpc").unwrap_or_else(|| std::env::var("HS_RPC_URL").unwrap_or_else(|_| "https://api.mainnet-beta.solana.com".into()));
            let slot = get("--slot").and_then(|s| s.parse().ok());
            let n = get("--n").and_then(|s| s.parse().ok()).unwrap_or(3);
            tokio::runtime::Builder::new_multi_thread().enable_all().build()?.block_on(selftest::run(&url, slot, n))
        }
        "config" => {
            let cfg = Config::load()?;
            println!("{}", serde_json::to_string_pretty(&cfg)?);
            Ok(())
        }
        "-h" | "--help" | "help" => {
            println!("{}", include_str!("main.rs").lines().skip(2).take(8).map(|l| l.trim_start_matches("//!")).collect::<Vec<_>>().join("\n"));
            Ok(())
        }
        other => anyhow::bail!("unknown command {other} (run | replay | backfill | config)"),
    }
}

fn run() -> anyhow::Result<()> {
    let cfg = Arc::new(Config::load()?);
    if cfg.grpc_urls.is_empty() {
        anyhow::bail!("set HS_GRPC_URLS (comma-separated Yellowstone endpoints)");
    }
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(4).enable_all().build()?;
    rt.block_on(async move {
        let record = cfg.record.then(|| Duration::from_secs(cfg.record_rotate_min * 60));
        let (log, log_thread) = Logger::start(&cfg.log_dir, record)?;
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Msg>();
        let live = if cfg.live.enabled { Some(live::Live::new(cfg.clone(), log.clone(), tx.clone())?) } else { None };
        let wallet = live.as_ref().map(|l| l.wallet);
        eprintln!(
            "hsnipe {} | paper delays {:?} size {} SOL | live: {} | logs -> {}",
            cfg.mode.name(),
            cfg.paper.delays,
            cfg.paper.size_sol,
            match (&live, cfg.live.simulate) {
                (Some(l), true) => format!("SIMULATE as {}", l.wallet),
                (Some(l), false) => format!("ON as {}", l.wallet),
                _ => "off".into(),
            },
            cfg.log_dir
        );
        let pool_watch = PoolWatch::new();
        let decoder = Decoder { wallet, capture_templates: live.is_some() };
        for (i, url) in cfg.grpc_urls.iter().enumerate() {
            let gc = grpc::GrpcCfg {
                url: url.clone(),
                x_token: cfg.grpc_x_token.clone(),
                src: i as u8,
                idle_timeout: Duration::from_secs(cfg.stream_idle_timeout_s),
                wallet,
            };
            tokio::spawn(grpc::run(gc, decoder.clone(), tx.clone(), pool_watch.clone()));
        }
        tokio::spawn(price::run(tx.clone(), Duration::from_secs(cfg.price_refresh_s)));
        {
            let tx = tx.clone();
            let every = Duration::from_millis(cfg.tick_ms);
            tokio::spawn(async move {
                let mut iv = tokio::time::interval(every);
                iv.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                loop {
                    iv.tick().await;
                    if tx.send(Msg::Tick).is_err() {
                        return;
                    }
                }
            });
        }
        let mut engine = Engine::new(cfg.clone(), log.clone(), live, false);
        engine.pool_watch = Some(pool_watch);
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
        loop {
            tokio::select! {
                m = rx.recv() => match m {
                    Some(m) => engine.on_msg(m),
                    None => break,
                },
                _ = tokio::signal::ctrl_c() => break,
                _ = term.recv() => break,
            }
        }
        eprintln!("shutting down");
        engine.write_summary(true);
        log.stop();
        let _ = log_thread.join();
        Ok::<(), anyhow::Error>(())
    })
}

fn open_events(path: &str) -> anyhow::Result<Box<dyn BufRead>> {
    let f = std::fs::File::open(path).with_context(|| format!("open {path}"))?;
    Ok(if path.ends_with(".gz") {
        Box::new(std::io::BufReader::with_capacity(1 << 20, flate2::read::MultiGzDecoder::new(f)))
    } else {
        Box::new(std::io::BufReader::with_capacity(1 << 20, f))
    })
}

fn replay(files: &[String]) -> anyhow::Result<()> {
    if files.is_empty() {
        anyhow::bail!("usage: hsnipe replay events.jsonl[.gz] [more files...]");
    }
    let mut cfg = Config::load()?;
    cfg.record = false;
    cfg.live.enabled = false;
    let cfg = Arc::new(cfg);
    let (log, log_thread) = Logger::start(&cfg.log_dir, None)?;
    let mut engine = Engine::new(cfg.clone(), log.clone(), None, true);
    let t0 = Instant::now();
    let (mut n, mut bad) = (0u64, 0u64);
    for f in files {
        for line in open_events(f)?.lines() {
            let line = match line {
                Ok(l) => l,
                Err(e) => {
                    // e.g. the last recorder file of a crashed run is cut short
                    eprintln!("{f}: stopped reading at {e}");
                    break;
                }
            };
            if line.is_empty() {
                continue;
            }
            match serde_json::from_str::<Event>(&line) {
                Ok(Event::Own(_)) => {}
                Ok(ev) => {
                    engine.on_event(&ev, None);
                    n += 1;
                }
                Err(_) => bad += 1,
            }
        }
    }
    engine.write_summary(true);
    log.stop();
    let _ = log_thread.join();
    eprintln!("replayed {n} events ({bad} unparsable) in {:.1}s -> {}/summary.json", t0.elapsed().as_secs_f64(), cfg.log_dir);
    println!("{}", serde_json::to_string(&engine.paper.summary())?);
    Ok(())
}

fn backfill_cmd(args: &[String]) -> anyhow::Result<()> {
    let get = |k: &str| args.iter().position(|a| a == k).and_then(|i| args.get(i + 1)).cloned();
    let out = get("--out").unwrap_or_else(|| "backfill.jsonl.gz".into());
    let url = get("--rpc").unwrap_or_else(|| std::env::var("HS_RPC_URL").unwrap_or_else(|_| "https://api.mainnet-beta.solana.com".into()));
    let conc: usize = get("--concurrency").and_then(|v| v.parse().ok()).unwrap_or(8);
    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build()?;
    rt.block_on(async {
        let (from, to) = match (get("--from"), get("--to"), get("--hours")) {
            (Some(a), Some(b), _) => (a.parse()?, b.parse()?),
            (_, _, Some(h)) => {
                let tip = rpc::Rpc::new(&url).get_slot("confirmed").await?;
                let h: f64 = h.parse()?;
                (tip - (h * 3600.0 / 0.4) as u64, tip)
            }
            _ => anyhow::bail!("usage: hsnipe backfill (--hours H | --from SLOT --to SLOT) [--out FILE] [--rpc URL] [--concurrency N]"),
        };
        backfill::run(&url, from, to, &out, conc).await
    })
}
