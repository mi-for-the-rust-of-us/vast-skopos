//! `vas`: a lookout over your rented vast.ai boxes. Not affiliated with vast.ai.

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use clap::Parser;
use vast_skopos::api::Client;
use vast_skopos::key::ApiKey;
use vast_skopos::probe::ssh_probe;
use vast_skopos::render::table;

/// A lookout over your rented vast.ai boxes: status, $/h, cost so far, credit, and (with --ssh)
/// what the GPUs are actually doing. Read-only. Not affiliated with vast.ai.
// EXPLICIT: these doc comments ARE the `--help` text, where backticks would print literally.
#[allow(clippy::doc_markdown)]
#[derive(Debug, Parser)]
#[command(name = "vas", version, about)]
struct Args {
    /// Also ask every running box over ssh (once each): GPU utilisation and memory from nvidia-smi.
    #[arg(long)]
    ssh: bool,
    /// With --ssh: for the newest file matching this path or glob on each box, print its last two
    /// path components and its last non-blank line ("(empty)", "(not a file)", "(no file matches)").
    #[arg(long, value_name = "PATH_OR_GLOB", requires = "ssh", value_parser = non_blank)]
    log: Option<String>,
    /// With --ssh: the private key to use (default: ssh's own choice, agent and ~/.ssh/config).
    #[arg(long, short = 'i', value_name = "FILE", requires = "ssh")]
    identity: Option<PathBuf>,
    /// Read the API key from this file (default: VAST_API_KEY, then the vastai CLI's files).
    #[arg(long, value_name = "FILE")]
    key_file: Option<PathBuf>,
    /// Refresh every N seconds (at least 30) until Ctrl+C.
    #[arg(long, value_name = "SECONDS")]
    watch: Option<u64>,
}

/// `--log`'s pattern, refused when blank: an unset shell variable would make `ls` list `.`.
fn non_blank(pattern: &str) -> Result<String, String> {
    if pattern.trim().is_empty() {
        Err(String::from(
            "the pattern is blank (an unset variable?): name the log file or glob",
        ))
    } else {
        // BORROW: explicit to_owned -- clap stores the parsed value owned
        Ok(pattern.to_owned())
    }
}

/// Unix seconds now.
fn now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64())
}

/// One look: the API, then the probes in parallel, then the table.
fn look(client: &Client, args: &Args) -> Result<String, vast_skopos::SkoposError> {
    let instances = client.instances()?;
    let account = match client.account() {
        Ok(a) => Some(a),
        Err(e) => {
            eprintln!("vas: no credit shown: {e}");
            None
        }
    };
    let probes: Vec<_> = if args.ssh {
        std::thread::scope(|scope| {
            // EXPLICIT: collected so every probe is SPAWNED before any is joined; a lazy chain would
            // run them one after another.
            #[allow(clippy::needless_collect)]
            let handles: Vec<_> = instances
                .iter()
                .map(|i| {
                    scope.spawn(|| {
                        let (host, port) = i.ssh_endpoint().filter(|_| i.is_running())?;
                        Some(ssh_probe(
                            host,
                            port,
                            args.identity.as_deref(),
                            args.log.as_deref(),
                        ))
                    })
                })
                .collect();
            handles
                .into_iter()
                .map(|h| {
                    h.join().unwrap_or_else(|_| {
                        Some(Err(vast_skopos::SkoposError::Ssh {
                            target: String::from("?"),
                            message: String::from("the probe thread panicked"),
                        }))
                    })
                })
                .collect()
        })
    } else {
        Vec::new()
    };
    Ok(table(
        &instances,
        &probes,
        args.ssh,
        account.as_ref(),
        now(),
    ))
}

fn main() -> ExitCode {
    let args = Args::parse();
    let key = args
        .key_file
        .as_deref()
        .map_or_else(ApiKey::discover, ApiKey::from_file);
    let key = match key {
        Ok(k) => k,
        Err(e) => {
            eprintln!("vas: {e}");
            return ExitCode::FAILURE;
        }
    };
    let client = Client::new(key);
    if client.base() != vast_skopos::api::DEFAULT_BASE {
        let plain = if client.base().starts_with("https://") {
            ""
        } else {
            ", NOT over https"
        };
        eprintln!(
            "vas: VAST_URL is set: the API key is sent to {}{plain}",
            client.base()
        );
    }
    loop {
        match look(&client, &args) {
            Ok(text) => {
                if args.watch.is_some() {
                    let stamp = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .map_or(0, |d| d.as_secs());
                    println!("== vas, unix time {stamp}");
                }
                print!("{text}");
            }
            Err(e) => {
                eprintln!("vas: {e}");
                if args.watch.is_none() {
                    return ExitCode::FAILURE;
                }
            }
        }
        let Some(every) = args.watch else {
            return ExitCode::SUCCESS;
        };
        std::thread::sleep(Duration::from_secs(every.max(30)));
    }
}
