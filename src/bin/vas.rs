//! `vas`: a lookout over your rented vast.ai boxes, and over the offers to rent more. Not
//! affiliated with vast.ai.

use std::ffi::{OsStr, OsString};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use clap::{Parser, Subcommand, ValueEnum};
use vast_skopos::api::{Client, DEFAULT_BASE, SearchClient};
use vast_skopos::compact::json_lines;
use vast_skopos::config::{default_offers_file, read_flag_file};
use vast_skopos::key::ApiKey;
use vast_skopos::offer::{
    Offer, OfferChanges, OfferQuery, OfferSort, country_code, gpu_model, select,
};
use vast_skopos::probe::ssh_probe;
use vast_skopos::render::{
    TableOptions, offers_changes_note, offers_changes_only, offers_footer, offers_table,
    offers_table_marked, table,
};

/// A lookout over your rented vast.ai boxes: status, $/h, cost so far, credit, and (with --ssh)
/// what the GPUs are actually doing. Read-only. Not affiliated with vast.ai.
// EXPLICIT: these doc comments ARE the `--help` text, where backticks would print literally.
#[allow(clippy::doc_markdown)]
#[derive(Debug, Parser)]
#[command(name = "vas", version, about, args_conflicts_with_subcommands = true)]
struct Cli {
    /// `vas offers`, or none: the lookout.
    #[command(subcommand)]
    command: Option<Command>,
    /// The lookout's flags (refused with a subcommand).
    #[command(flatten)]
    look: LookArgs,
}

/// The subcommands.
#[derive(Debug, Subcommand)]
enum Command {
    /// The offers for rent that pass your constraints, cheapest first. Needs no API key: the
    /// search is public, and vas sends none.
    Offers(OffersArgs),
}

/// The lookout's flags.
// EXPLICIT: these doc comments ARE the `--help` text, where backticks would print literally.
#[allow(clippy::doc_markdown)]
#[derive(Debug, clap::Args)]
struct LookArgs {
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
    /// Also show each box's machine id and host id (what a rentals log or an AVOID list needs).
    #[arg(long)]
    wide: bool,
    /// JSON Lines instead of the table, for scripts and monitors: one object per box, then one for
    /// the money, each with a `kind` and an `at` (Unix seconds); the machine and host ids always.
    #[arg(long, conflicts_with = "wide")]
    compact: bool,
    /// Refresh every N seconds (at least 30) until Ctrl+C.
    #[arg(long, value_name = "SECONDS")]
    watch: Option<u64>,
}

/// The order of the offers.
#[derive(Debug, Clone, Copy, ValueEnum)]
enum Sort {
    /// The cheapest first.
    Price,
    /// The most reliable first.
    Reliability,
    /// The fastest upload first.
    Up,
}

impl From<Sort> for OfferSort {
    fn from(sort: Sort) -> Self {
        match sort {
            Sort::Price => Self::Price,
            Sort::Reliability => Self::Reliability,
            Sort::Up => Self::Up,
        }
    }
}

/// `vas offers`'s flags. A config file of the same flags is read first (`--config`); a single value
/// given twice is the last one, a list (`--exclude-*`) keeps every entry.
// EXPLICIT: these doc comments ARE the `--help` text, where backticks would print literally; and
// the switches of a command line are booleans, one per flag (`--verified`, `--no-verified`,
// `--changes`, `--no-config`), not a state to model.
#[allow(clippy::doc_markdown, clippy::struct_excessive_bools)]
#[derive(Debug, clap::Args)]
#[command(args_override_self = true)]
struct OffersArgs {
    /// The GPU model as vast.ai names it: RTX_5090 or "RTX 5090" (an underscore reads as a space).
    #[arg(long, value_name = "MODEL", value_parser = gpu)]
    gpu: Option<String>,
    /// The number of GPUs.
    #[arg(long, value_name = "N", value_parser = clap::value_parser!(u64).range(1..))]
    num_gpus: Option<u64>,
    /// The disk wanted, in GB: priced into $/h, and required free (unset: 5 GB priced in).
    #[arg(long, value_name = "GB", value_parser = clap::value_parser!(u32).range(1..))]
    disk: Option<u32>,
    /// The lowest CUDA version the host's driver must support (e.g. 12.8).
    #[arg(long, value_name = "VERSION", value_parser = positive)]
    cuda: Option<f64>,
    /// The lowest reliability, a fraction from 0 to 1 (0.99 for 99 %).
    #[arg(long, value_name = "FRACTION", value_parser = fraction)]
    reliability: Option<f64>,
    /// The lowest advertised upload, in Mb/s.
    #[arg(long, value_name = "MBIT_S", value_parser = non_negative)]
    inet_up: Option<f64>,
    /// The lowest advertised download, in Mb/s.
    #[arg(long, value_name = "MBIT_S", value_parser = non_negative)]
    inet_down: Option<f64>,
    /// The highest price, in $/h, storage included.
    #[arg(long, value_name = "DOLLARS", value_parser = positive)]
    max_price: Option<f64>,
    /// The shortest acceptable maximum rental, in days.
    #[arg(long, value_name = "DAYS", value_parser = positive)]
    min_days: Option<f64>,
    /// Only machines vast.ai has verified.
    #[arg(long, overrides_with = "no_verified")]
    verified: bool,
    /// Cancels --verified (e.g. the config file's).
    #[arg(long, overrides_with = "verified")]
    no_verified: bool,
    /// Leave out this country (two letters, e.g. CN). Repeatable; added to the config file's.
    #[arg(long, value_name = "CC", value_parser = country)]
    exclude_country: Vec<String>,
    /// Leave out this host. Repeatable; added to the config file's.
    #[arg(long, value_name = "HOST_ID")]
    exclude_host: Vec<u64>,
    /// Leave out this machine (a host may own several; `vas --wide` shows yours). Repeatable; added
    /// to the config file's.
    #[arg(long, value_name = "MACHINE_ID")]
    exclude_machine: Vec<u64>,
    /// Search again every N seconds (at least 30) until Ctrl+C: each look is the whole table, with a
    /// `+` on the offers that are new since the last one and the prices that moved, and what
    /// vanished listed under it.
    #[arg(long, value_name = "SECONDS")]
    watch: Option<u64>,
    /// With --watch: print only what moved since the last look (one line each), not the table.
    #[arg(long, requires = "watch")]
    changes: bool,
    /// Show at most N offers.
    #[arg(long, value_name = "N", default_value_t = 20, value_parser = clap::value_parser!(u64).range(1..))]
    top: u64,
    /// The order of the offers (and which ones vast.ai sends: it sends at most 64).
    #[arg(long, value_enum, default_value_t = Sort::Price)]
    sort: Sort,
    /// Read the flags of this file first (default: ~/.config/vast-skopos/offers, when it exists).
    #[arg(long, value_name = "FILE", conflicts_with = "no_config")]
    config: Option<PathBuf>,
    /// Read no config file.
    #[arg(long)]
    no_config: bool,
}

impl OffersArgs {
    /// The constraints, as the search's query.
    fn query(&self) -> OfferQuery {
        let mut q = OfferQuery::default();
        q.gpu.clone_from(&self.gpu);
        q.num_gpus = self.num_gpus;
        q.disk = self.disk;
        q.cuda = self.cuda;
        q.reliability = self.reliability;
        q.inet_up = self.inet_up;
        q.inet_down = self.inet_down;
        q.max_price = self.max_price;
        q.min_days = self.min_days;
        q.verified = self.verified;
        q.exclude_countries.clone_from(&self.exclude_country);
        q.exclude_hosts.clone_from(&self.exclude_host);
        q.exclude_machines.clone_from(&self.exclude_machine);
        q
    }
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

/// A finite number (`NaN` and `inf` parse as `f64`, and are refused).
fn finite(text: &str) -> Result<f64, String> {
    text.trim()
        .parse::<f64>()
        .ok()
        .filter(|v| v.is_finite())
        .ok_or_else(|| String::from("not a number"))
}

/// A number above 0.
fn positive(text: &str) -> Result<f64, String> {
    let v = finite(text)?;
    if v > 0.0 {
        Ok(v)
    } else {
        Err(String::from("must be above 0"))
    }
}

/// A number at least 0.
fn non_negative(text: &str) -> Result<f64, String> {
    let v = finite(text)?;
    if v >= 0.0 {
        Ok(v)
    } else {
        Err(String::from("must be at least 0"))
    }
}

/// A fraction from 0 to 1.
fn fraction(text: &str) -> Result<f64, String> {
    let v = finite(text)?;
    if (0.0..=1.0).contains(&v) {
        Ok(v)
    } else {
        Err(String::from("a fraction from 0 to 1: 0.99 for 99 %"))
    }
}

/// `--gpu`'s model ([`gpu_model`]).
fn gpu(name: &str) -> Result<String, String> {
    gpu_model(name).ok_or_else(|| {
        String::from("blank or quoted: name the model as vast.ai does, e.g. RTX_5090")
    })
}

/// `--exclude-country`'s code ([`country_code`]).
fn country(code: &str) -> Result<String, String> {
    country_code(code).ok_or_else(|| String::from("two letters, e.g. CN"))
}

/// Writes `text` to stdout; whether it was written. A closed pipe ends the output quietly, where
/// `print!` would panic.
fn emit(text: &str) -> bool {
    let mut out = std::io::stdout().lock();
    out.write_all(text.as_bytes())
        .and_then(|()| out.flush())
        .is_ok()
}

/// The pause between two looks of a `--watch`: the N asked, but at least 30 s, so no loop of looks
/// hammers a public service. `VAS_WATCH_FLOOR_MS` replaces that floor (the tests' seam: a test of a
/// second look cannot wait half a minute); unset or not a number, the floor is 30 s.
fn watch_pause(every: u64) -> Duration {
    let floor = std::env::var("VAS_WATCH_FLOOR_MS")
        .ok()
        .and_then(|ms| ms.trim().parse::<u64>().ok())
        .map_or(Duration::from_secs(30), Duration::from_millis);
    Duration::from_secs(every).max(floor)
}

/// The notice that a `--watch` shorter than the floor was raised to it; `who` names the command.
fn floor_notice(who: &str, every: u64) -> Option<String> {
    let pause = watch_pause(every);
    (pause > Duration::from_secs(every)).then(|| {
        format!(
            "{who}: --watch {every} is raised to {} s, the shortest pause between two looks (no loop hammers vast.ai)",
            pause.as_secs_f64()
        )
    })
}

/// Whether a failed look ends the run. A one-shot does; so does a watch whose first look fails,
/// since a wrong `VAST_URL` or key would loop on stderr forever. Once a look has succeeded, a
/// failure is a hiccup in a long watch, and the next look is tried.
const fn ends_on_failure(watching: bool, succeeded_before: bool) -> bool {
    !watching || !succeeded_before
}

/// Unix seconds now.
fn now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64())
}

/// One look: the API, then the probes in parallel, then the table.
fn look(client: &Client, args: &LookArgs) -> Result<String, vast_skopos::SkoposError> {
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
    Ok(if args.compact {
        json_lines(&instances, &probes, account.as_ref(), now())
    } else {
        table(
            &instances,
            &probes,
            account.as_ref(),
            TableOptions::new().ssh(args.ssh).wide(args.wide),
            now(),
        )
    })
}

/// `vas` without a subcommand: the lookout, once or every `--watch` seconds.
fn lookout(args: &LookArgs) -> ExitCode {
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
    if client.base() != DEFAULT_BASE {
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
    if let Some(every) = args.watch
        && let Some(note) = floor_notice("vas", every)
    {
        eprintln!("{note}");
    }
    let mut succeeded = false;
    loop {
        match look(&client, args) {
            Ok(text) => {
                succeeded = true;
                // JSON Lines carry their own `at`: a stamp line would not parse.
                let stamp = if args.watch.is_some() && !args.compact {
                    let secs = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .map_or(0, |d| d.as_secs());
                    format!("== vas, unix time {secs}\n")
                } else {
                    String::new()
                };
                if !emit(&format!("{stamp}{text}")) {
                    // The reader is gone (`vas --watch 60 | head`): nothing more to show.
                    return ExitCode::SUCCESS;
                }
            }
            Err(e) => {
                eprintln!("vas: {e}");
                if ends_on_failure(args.watch.is_some(), succeeded) {
                    return ExitCode::FAILURE;
                }
            }
        }
        let Some(every) = args.watch else {
            return ExitCode::SUCCESS;
        };
        std::thread::sleep(watch_pause(every));
    }
}

/// The `offers` flags again, the config file's first and the command line's after; on failure,
/// the exit code (the error already printed).
fn with_file(path: &Path) -> Result<OffersArgs, ExitCode> {
    let words = read_flag_file(path).map_err(|e| {
        eprintln!("vas offers: {e}");
        ExitCode::FAILURE
    })?;
    let command_line: Vec<OsString> = std::env::args_os().collect();
    // The first parse accepted `offers` with no `vas` flag before it: what follows is its own
    // (the first word, the program, is not searched).
    let after = command_line
        .iter()
        .skip(1)
        .position(|a| a.as_os_str() == OsStr::new("offers"))
        .map_or(command_line.len(), |at| at + 2);
    let mut merged = vec![OsString::from("vas"), OsString::from("offers")];
    merged.extend(words.into_iter().map(OsString::from));
    merged.extend(command_line.into_iter().skip(after));
    match Cli::try_parse_from(merged) {
        Ok(Cli {
            command: Some(Command::Offers(args)),
            ..
        }) => Ok(args),
        Ok(Cli { command: None, .. }) => {
            eprintln!("vas offers: the flags of {} lost `offers`", path.display());
            Err(ExitCode::FAILURE)
        }
        Err(e) => {
            let _ = e.print();
            if e.use_stderr() {
                eprintln!(
                    "vas offers: the flags of {} were read first (--no-config skips it)",
                    path.display()
                );
            }
            Err(ExitCode::from(u8::try_from(e.exit_code()).unwrap_or(2)))
        }
    }
}

/// `vas offers`: the config file's flags, then the command line's; one search; the table.
fn offers(args: OffersArgs) -> ExitCode {
    let file = if args.no_config {
        None
    } else {
        args.config.clone().or_else(default_offers_file)
    };
    let args = match file.as_deref() {
        None => args,
        Some(path) => match with_file(path) {
            Ok(merged) => merged,
            Err(code) => return code,
        },
    };
    let query = args.query();
    let sort = OfferSort::from(args.sort);
    if let Some(path) = &file {
        eprintln!(
            "vas offers: the flags of {} first, then the command line's",
            path.display()
        );
    }
    if query.is_identity() {
        eprintln!(
            "vas offers: no constraint given (neither a config file nor a flag sets one): every offer rentable now, {}",
            sort.words()
        );
    }
    let client = SearchClient::new();
    if client.base() != DEFAULT_BASE {
        eprintln!("vas offers: VAST_URL is set: searching {}", client.base());
    }
    if let Some(every) = args.watch
        && let Some(note) = floor_notice("vas offers", every)
    {
        eprintln!("{note}");
    }
    let top = usize::try_from(args.top).unwrap_or(usize::MAX);
    let nothing = OfferChanges::default();
    let mut previous: Option<Vec<Offer>> = None;
    // Whether a search failed since the last look that was printed.
    let mut missed = false;
    loop {
        match client.offers(&query, sort) {
            Ok(page) => {
                // BORROW: explicit clone -- the footer reads the page again
                let kept = select(page.offers.clone(), &query, sort);
                let at = now();
                let moved = previous.as_deref().map(|p| OfferChanges::between(p, &kept));
                let text = match (args.watch, args.changes) {
                    (_, true) => offers_changes_only(moved.as_ref(), &kept, &page, missed, at),
                    (None, false) => format!(
                        "{}{}",
                        offers_table(&kept, top),
                        offers_footer(&page, &kept, top, &query, sort, at)
                    ),
                    (Some(_), false) => {
                        // The first look has nothing to mark; a blank line parts the looks.
                        let gap = if previous.is_some() { "\n" } else { "" };
                        let shown = kept.get(..top).unwrap_or(&kept);
                        let note = moved
                            .as_ref()
                            .map(|m| offers_changes_note(m, shown, &page, missed))
                            .unwrap_or_default();
                        format!(
                            "{gap}{}{}{note}",
                            offers_table_marked(&kept, top, moved.as_ref().unwrap_or(&nothing)),
                            offers_footer(&page, &kept, top, &query, sort, at)
                        )
                    }
                };
                // A closed pipe (`vas offers | head -3`) is the reader's choice, not a failure.
                if !emit(&text) {
                    return ExitCode::SUCCESS;
                }
                previous = Some(kept);
                missed = false;
            }
            Err(e) => {
                eprintln!("vas offers: {e}");
                if ends_on_failure(args.watch.is_some(), previous.is_some()) {
                    return ExitCode::FAILURE;
                }
                missed = true;
            }
        }
        let Some(every) = args.watch else {
            return ExitCode::SUCCESS;
        };
        std::thread::sleep(watch_pause(every));
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Some(Command::Offers(args)) => offers(args),
        None => lookout(&cli.look),
    }
}
