//! `vas`: a lookout over your rented vast.ai boxes, and over the offers to rent more. Not
//! affiliated with vast.ai.

use std::ffi::OsString;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use clap::{Parser, Subcommand, ValueEnum};
use vast_skopos::api::{Client, DEFAULT_BASE, SearchClient};
use vast_skopos::compact::json_lines_of;
use vast_skopos::config::{default_offers_file, read_flag_file};
use vast_skopos::creditlog::CreditLog;
use vast_skopos::instance::Instance;
use vast_skopos::key::ApiKey;
use vast_skopos::market::MarketWatch;
use vast_skopos::offer::{OfferChanges, OfferQuery, OfferSort, country_code, gpu_model, select};
use vast_skopos::probe::{LogQuery, probe_all};
use vast_skopos::render::{
    OfferColumns, TableOptions, offers_changes_note, offers_changes_only, offers_footer,
    offers_table, offers_table_marked, offers_variance_note, table_of,
};
use vast_skopos::runway::Runway;
use vast_skopos::trend::Trend;
use vast_skopos::until::{
    End, NO_FILE_SECS, NO_FILE_WARN_LOOKS, STARTING_LOOKS, UNREAD_LOOKS, Verdict, Wait, verdict,
};

/// What `vas --help` prints after the flags (`-h` leaves it out): examples, the exit codes and the
/// environment variables. The tests parse every example and pin the exit codes to [`REFUSED`],
/// [`GAVE_UP`] and the looks `vas` counts.
const LOOKOUT_AFTER_HELP: &str = "\
Examples:
  Watch my boxes, a look every two minutes:
    vas --watch 120
  Is any GPU idle, capped or shared? (Reading the table, below, says how to tell)
    vas --ssh
  Wait for the first loss on box 56533728, at most ten minutes. The wait has no time limit of its
  own: timeout gives one, and exits 124 when it is reached.
    timeout 600 vas --ssh --log '/root/runs/*/train.log' --watch 30 --until 'initial loss' --instance 56533728
  Find a verified RTX 5090 under $0.50/h (no key needed; vas offers --help says more):
    vas offers --gpu RTX_5090 --verified --max-price 0.5
  The same readings for a program, one JSON object per line:
    vas --compact --ssh

Reading the table (stdout; notices, a wait's match among them, go to stderr):
  STATUS      loading -> running: what the box is, then what it was asked to be. A box not running
              has vast.ai's own message on a line under the table: 46b4edaa8d47: Pull complete is
              the image arriving, progress.
  COST~       An estimate: today's $/h times the hours since the box was created.
  GPU% (ssh)  100%? is likely an idle card's stuck counter: 99 % or more, under 1 GiB held and no
              process (a line under the table says so).
  OTHER       GPU memory held outside the container (1 GiB or more); - when none, or not known.
  POWER       400/575W+power_cap: a limit of 400 W set below the card's 575 W default, and the
              clocks held by it (other reasons: thermal, slowdown, power_brake). 575W+power_cap alone
              is a card at its limit under full load: normal. 450W<575: the board's maximum is
              below its model's reference, so the board itself is the cap.
  CLOCK       The SM clock now over the highest it reaches, in MHz: 1717/3090 is held down.
  Last cell   The log's run and line (s0/train.log: ...; under --grep or --until, the last line
              matching, and (no line matches) while none does), or why the probe failed:
              UNREACHABLE 4m, ssh never reached the box (the age says whether to wait); SSH FAILED
              4m, it was turned away (a refused key) or got no reading. (direct): read past the
              proxy. ?: the answer was cut short.
  The money   credit | burn over the boxes that bill or may (meant to run, billing unknown) |
              the hours the credit buys at that rate.

Exit codes:
  0  The look was printed (a one-shot), the line was matched (--until), or stdout was closed
     (vas --watch 60 | head). A watch without --until runs until Ctrl+C.
  1  A look failed: a one-shot's, or a watch's first (a later one is tried again at the next look).
     Or no API key was found (or its file could not be read), or a wait's stdout was closed before
     a match.
  2  A flag or value was refused, before any request (a --log that is not a path on a Linux box,
     too); or a box's grep refused the --until pattern.
  3  A wait gave up: no box could be read for 2 looks in a row, or none had a file matching --log
     for --log-wait seconds.

Environment:
  VAST_API_KEY     The API key, unless --key-file names a file. Else the vastai CLI's files:
                   $XDG_CONFIG_HOME/vastai/vast_api_key, ~/.config/vastai/vast_api_key,
                   ~/.vast_api_key. Never on the command line, never printed.
  VAST_URL         Another API host than https://console.vast.ai (a mock, a proxy): the API key is
                   then sent there, and vas says so on stderr.
  XDG_CONFIG_HOME  Where the key file (vastai/vast_api_key) and vas offers' config file
                   (vast-skopos/offers) are looked for first, when it is an absolute path; then
                   ~/.config.

AI agents:
  vas --skill prints a skill that teaches an agent vas: which command answers which question, how
  to read each mark, the exit codes, and the rules (Agent Skills format; skills/vas/SKILL.md in the
  source). For Claude Code: mkdir -p .claude/skills/vas && vas --skill > .claude/skills/vas/SKILL.md
  (from Windows PowerShell 5.1, whose > writes UTF-16: vas --skill | Out-File -Encoding ascii
  .claude\\skills\\vas\\SKILL.md, the folder made first).";

/// The skill for AI agents (`vas --skill`): `skills/vas/SKILL.md`, embedded at build time so an
/// installed `vas` hands over the skill of its own flags. The tests parse every `vas` command in it.
const SKILL: &str = include_str!("../../skills/vas/SKILL.md");

/// What `vas offers --help` prints after the flags, as [`LOOKOUT_AFTER_HELP`] does for the lookout.
const OFFERS_AFTER_HELP: &str = "\
Examples:
  A verified RTX 5090 under $0.50/h, with 50 GB of disk:
    vas offers --gpu RTX_5090 --verified --max-price 0.5 --disk 50
  A cheaper verified machine like box 56533728, which vas --wide shows at $0.491/h on machine 100682
  (compare the CPU, DLPERF and PCIE GB/s columns with its own):
    vas offers --gpu RTX_5090 --num-gpus 1 --verified --max-price 0.49 --exclude-machine 100682 --wide
  Is the machine I had for rent again? (vas --wide shows its id)
    vas offers --machine 100682 --wide
  Watch two-GPU RTX 4090 offers, printing only what moved:
    vas offers --gpu RTX_4090 --num-gpus 2 --watch 60 --changes
  The filters you always use go in the config file, one flag per line: --exclude-country CN

Reading the footer:
  How many offers (N of M shown when --top cuts), the order | the constraints in force | $/h
  includes N GB of storage (--disk) | when it searched. Then, when they apply: N dropped here
  (vast.ai sent rows that a constraint, re-checked on each row, refuses: it may ignore a filter),
  and that it answers at most 64 rows. vast.ai's answers vary from one search to the next: repeat
  it, or --watch, to see more.

Exit codes:
  0  The offers were printed (a one-shot), or stdout was closed. A watch runs until Ctrl+C.
  1  A search failed: a one-shot's, or a watch's first (a later one is tried again at the next
     look). Or the config file is missing (--config) or unreadable, or holds a line that is not a
     flag or a flag a file may not hold (--watch, --changes, --config...).
  2  A flag or value was refused, on the command line or in the config file (an unknown flag, a
     value out of range), before any request.

Environment:
  VAST_URL         Another host than https://console.vast.ai (a mock): searched instead. No key is
                   ever sent, there or anywhere.
  XDG_CONFIG_HOME  The config file is $XDG_CONFIG_HOME/vast-skopos/offers when it is an absolute
                   path and the file exists, else ~/.config/vast-skopos/offers.";

/// The command line: the lookout's flags, or the `offers` subcommand. (`--help` opens with `about`,
/// which is Cargo.toml's `description`: one place to say what `vas` is.)
#[derive(Debug, Parser)]
#[command(
    name = "vas",
    version,
    about,
    args_conflicts_with_subcommands = true,
    after_long_help = LOOKOUT_AFTER_HELP
)]
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
    /// The offers for rent that pass your constraints; no API key needed.
    ///
    /// Cheapest first, unless you sort otherwise. The search is public, and vas sends no key.
    #[command(after_long_help = OFFERS_AFTER_HELP)]
    Offers(OffersArgs),
}

/// The lookout's flags. Each doc comment opens with a one-line summary, which is `-h`; the whole
/// comment is `--help`.
// EXPLICIT: these doc comments ARE the `--help` text, where backticks would print literally; and
// the switches of a command line are booleans, one per flag (`--ssh`, `--wide`, `--compact`,
// `--skill`), not a state to model.
#[allow(clippy::doc_markdown, clippy::struct_excessive_bools)]
#[derive(Debug, clap::Args)]
struct LookArgs {
    /// Also read each running box's GPUs over ssh, with nvidia-smi.
    ///
    /// Asks every running box over ssh (once each, and once more at its direct address when the
    /// proxy never reached it): from nvidia-smi, the GPUs' utilisation, memory, power limit, clock
    /// and throttle reasons, and the memory held by anyone else.
    #[arg(long)]
    ssh: bool,
    /// With --ssh: the last line of the newest file matching this path or glob.
    ///
    /// For the newest file matching this path or glob on each box, print its last two path
    /// components and its last non-blank line ("(empty)", "(not a file)", "(no file matches)", or
    /// "?" when the box's answer was cut short). From Git Bash, a /root/... path that the shell
    /// rewrote as C:/Program Files/Git/root/... is read as typed again, and said.
    #[arg(long, value_name = "PATH_OR_GLOB", requires = "ssh", value_parser = non_blank)]
    log: Option<String>,
    /// With --log: the last line matching this regular expression instead.
    ///
    /// The last line MATCHING this extended regular expression (grep -E, case-sensitive), not the
    /// last line: "initial loss|Traceback". Searched in the file's last 64 MiB within 10 s ("?" past
    /// them), each carriage-return segment a line, empty lines left out, each line's first 4096
    /// bytes; "(no line matches)" when none does, "(grep refused the pattern)" when it is not one,
    /// "(a blank line)" for a line of spaces only. Quoted for the box's shell: nothing in it is
    /// expanded.
    #[arg(long, value_name = "PATTERN", requires = "log", value_parser = grep_pattern)]
    grep: Option<String>,
    /// With --log and --watch: exit 0 once a line matches this pattern.
    ///
    /// The pattern is read as --grep reads it (which it replaces), on any box; the line that matched
    /// is said on stderr. Exits 2 at once when a box's grep refuses the pattern (and no other box
    /// was read), 3 when no box could be read for 2 looks in a row (every probe failed or gave no
    /// readable log, or none could be made) or no box has had a file matching --log for --log-wait
    /// seconds (a warning comes after 2 looks), and 1 when the first look fails or stdout is closed
    /// before a match. A box meant to run but not running (loading, say) keeps the wait going for
    /// 10 looks at most since a box was last read.
    ///
    /// No time limit on the whole wait (--log-wait bounds only a missing file): timeout 600 vas ...
    /// gives one, and exits 124 when it is reached. It matches on any box: when several write the
    /// same log, --instance names the one to wait on.
    #[arg(
        long,
        value_name = "PATTERN",
        requires_all = ["log", "watch"],
        conflicts_with = "grep",
        value_parser = grep_pattern
    )]
    until: Option<String>,
    /// With --until: give up after this long without a file matching --log.
    ///
    /// How long the wait goes on while the boxes answer but none has a file matching --log, counted
    /// from the first look that found none (a run may write its log late; a wrong path must not keep
    /// a script waiting while the boxes cost). Not a limit on the whole wait: timeout gives one.
    #[arg(
        long,
        value_name = "SECONDS",
        default_value_t = NO_FILE_SECS,
        requires = "until",
        value_parser = clap::value_parser!(u64).range(1..)
    )]
    log_wait: u64,
    /// With --ssh: the private key to use.
    ///
    /// Default: ssh's own choice, agent and ~/.ssh/config.
    #[arg(long, short = 'i', value_name = "FILE", requires = "ssh")]
    identity: Option<PathBuf>,
    /// Read the API key from this file.
    ///
    /// Default: VAST_API_KEY, then the vastai CLI's files (see Environment below).
    #[arg(long, value_name = "FILE")]
    key_file: Option<PathBuf>,
    /// Also show each box's direct ssh address, its ids, CPU, driver and PCIe.
    ///
    /// Each box's direct ssh address, past the proxy (DIRECT: its public IP and the port mapped to
    /// 22), its machine id and host id (what a rentals log, --exclude-machine and --exclude-host
    /// need), then its CPU, driver, DLPerf and PCIe bandwidth (what tells it from another machine,
    /// as vas offers --wide shows them).
    #[arg(long)]
    wide: bool,
    /// JSON Lines instead of the table, for programs: every key, always.
    ///
    /// For scripts and monitors: one object per box, then one for the money, each with a kind and
    /// an at (Unix seconds); every key always (null when not known), the machine and host ids, the
    /// CPU, driver, DLPerf, PCIe and vast.ai's status message among them. Not with --wide: it
    /// carries what --wide shows.
    #[arg(long, conflicts_with = "wide")]
    compact: bool,
    /// Append the credit and the burn to this CSV file, one row per look.
    ///
    /// The columns: unix_time,credit,burn_per_hour,running,unsure (a header when the file is new or
    /// empty). The credit as a curve; never the API key.
    #[arg(long, value_name = "FILE", value_parser = non_blank_path)]
    credit_log: Option<PathBuf>,
    /// Refresh every N seconds (at least 10) until Ctrl+C.
    #[arg(long, value_name = "SECONDS")]
    watch: Option<u64>,
    /// Only this box (repeatable): its row, its probe and --until follow it.
    ///
    /// The rows, the probes, the trend and --until follow the boxes named. The money line stays the
    /// whole account's (the credit is spent by every box) and says how many boxes are shown;
    /// --credit-log writes the whole account's money too. An id that is not in the account is said
    /// on stderr.
    #[arg(long, value_name = "ID")]
    instance: Vec<u64>,
    /// Print the skill that teaches an AI agent to use vas, then exit.
    ///
    /// The skill is a Markdown file in the Agent Skills format, the source's skills/vas/SKILL.md as
    /// this vas was built with: which command answers which question, how to read each mark, the
    /// exit codes and the rules. Needs no API key, and sends nothing. Alone: with any other flag, it
    /// is refused.
    #[arg(long, exclusive = true)]
    skill: bool,
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
// `--exclude-deverified`, `--no-exclude-deverified`, `--changes`, `--no-config`), not a state to model.
#[allow(clippy::doc_markdown, clippy::struct_excessive_bools)]
#[derive(Debug, clap::Args)]
#[command(args_override_self = true)]
struct OffersArgs {
    /// The GPU model as vast.ai names it: RTX_5090 or "RTX 5090".
    ///
    /// An underscore reads as a space.
    #[arg(long, value_name = "MODEL", value_parser = gpu)]
    gpu: Option<String>,
    /// The number of GPUs.
    #[arg(long, value_name = "N", value_parser = clap::value_parser!(u64).range(1..))]
    num_gpus: Option<u64>,
    /// The disk wanted, in GB (unset: 5 GB priced in).
    ///
    /// Priced into $/h, and required free.
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
    ///
    /// At most 36500: a hundred years.
    #[arg(long, value_name = "DAYS", value_parser = days)]
    min_days: Option<f64>,
    /// Only machines vast.ai has verified.
    #[arg(long, overrides_with = "no_verified")]
    verified: bool,
    /// Cancels --verified (e.g. the config file's).
    #[arg(long, overrides_with = "verified")]
    no_verified: bool,
    /// Leave out the deverified machines, and keep the unverified ones.
    ///
    /// Leaves out the machines vast.ai has deverified (verified once, then found failing its
    /// checks) and keeps the unverified ones (new, or under evaluation), which --verified hides as
    /// well.
    #[arg(long, overrides_with = "no_exclude_deverified")]
    exclude_deverified: bool,
    /// Cancels --exclude-deverified (e.g. the config file's).
    #[arg(long, overrides_with = "exclude_deverified")]
    no_exclude_deverified: bool,
    /// Only this host (repeatable).
    ///
    /// E.g. one rented from before, to see what it offers now. Added to the config file's.
    #[arg(long, value_name = "HOST_ID")]
    host: Vec<u64>,
    /// Only this machine (repeatable).
    ///
    /// Is the machine rented before for rent again? vas --wide shows yours. Added to the config
    /// file's.
    #[arg(long, value_name = "MACHINE_ID")]
    machine: Vec<u64>,
    /// Leave out this country, two letters: CN (repeatable).
    ///
    /// Added to the config file's.
    #[arg(long, value_name = "CC", value_parser = country)]
    exclude_country: Vec<String>,
    /// Leave out this host (repeatable).
    ///
    /// Added to the config file's.
    #[arg(long, value_name = "HOST_ID")]
    exclude_host: Vec<u64>,
    /// Leave out this machine (repeatable).
    ///
    /// A host may own several; vas --wide shows yours. Added to the config file's.
    #[arg(long, value_name = "MACHINE_ID")]
    exclude_machine: Vec<u64>,
    /// Search again every N seconds (at least 20) until Ctrl+C.
    ///
    /// Each look is the whole table with a SEEN column (in how many looks each machine appeared).
    /// vast.ai's answers vary, so the first three looks only learn; after them a new machine is
    /// marked with a plus sign, a moved price is shown beside its old one, and a machine missing
    /// for five looks is listed under the table.
    #[arg(long, value_name = "SECONDS")]
    watch: Option<u64>,
    /// With --watch: print only what moved since the last look, not the table.
    ///
    /// One line each.
    #[arg(long, requires = "watch")]
    changes: bool,
    /// Also show what tells one machine from another: CPU, DLPerf, PCIe...
    ///
    /// The table's (--changes prints none): MACHINE (after HOST), then CPU, DRIVER, DLPERF
    /// (vast.ai's benchmark score), DLPERF/$ (per $/h shown), PCIE GB/s and LIMIT W (the GPU power
    /// limit the listing gives, not a reading of the card: vas --ssh's POWER reads that).
    #[arg(long)]
    wide: bool,
    /// Show at most N offers.
    #[arg(long, value_name = "N", default_value_t = 20, value_parser = clap::value_parser!(u64).range(1..))]
    top: u64,
    /// The order of the offers.
    ///
    /// It also chooses which ones vast.ai sends: it sends at most 64.
    #[arg(long, value_enum, default_value_t = Sort::Price)]
    sort: Sort,
    /// Read the flags of this file first.
    ///
    /// Default: $XDG_CONFIG_HOME/vast-skopos/offers, else ~/.config/vast-skopos/offers, when one
    /// exists.
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
        // The query owns the flags it was built from.
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
        q.exclude_deverified = self.exclude_deverified;
        q.exclude_countries.clone_from(&self.exclude_country);
        q.only_hosts.clone_from(&self.host);
        q.only_machines.clone_from(&self.machine);
        q.exclude_hosts.clone_from(&self.exclude_host);
        q.exclude_machines.clone_from(&self.exclude_machine);
        q
    }
}

/// `--credit-log`'s file, refused when blank: an unset shell variable would name a file that is not one.
fn non_blank_path(path: &str) -> Result<PathBuf, String> {
    if path.trim().is_empty() {
        Err(String::from(
            "the file name is blank (an unset variable?): name the CSV file",
        ))
    } else {
        Ok(PathBuf::from(path))
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

/// `--grep`'s pattern, refused when blank (an unset variable?) or holding a control character: a
/// newline would make two patterns of it on the box.
fn grep_pattern(pattern: &str) -> Result<String, String> {
    if pattern.trim().is_empty() {
        Err(String::from(
            "the pattern is blank (an unset variable?): name what the line holds",
        ))
    } else if pattern.chars().any(char::is_control) {
        Err(String::from(
            "the pattern holds a control character (a newline?): one line's pattern only",
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

/// A number of days above 0 and at most a hundred years: more would overflow the seconds sent to
/// the server (`1e305` days is not a rental).
fn days(text: &str) -> Result<f64, String> {
    let v = positive(text)?;
    if v <= 36_500.0 {
        Ok(v)
    } else {
        Err(String::from("must be at most 36500 (a hundred years)"))
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
    // BORROW: explicit as_bytes -- the writer takes bytes
    out.write_all(text.as_bytes())
        .and_then(|()| out.flush())
        .is_ok()
}

/// The shortest pause, in seconds, between two looks of the lookout's `--watch`: at 10 s a watch
/// that probes boxes still opens one `ssh` session per box about every 14 s. (vast.ai's own limits
/// are read and obeyed whatever this says: the instances list allows one call per window of about
/// 3 s, seen 2026-10-18.)
const LOOKOUT_FLOOR_SECS: u64 = 10;

/// The shortest pause, in seconds, between two looks of `vas offers --watch`. A table every few
/// seconds scrolls past unread in a terminal (`--changes` reads better); under 20 s waits for a
/// screen that redraws in place.
const OFFERS_FLOOR_SECS: u64 = 20;

/// The pause between two looks of a `--watch`: the N asked, but at least `floor_secs`, so no loop
/// of looks hammers a public service. `VAS_WATCH_FLOOR_MS` replaces that floor (the tests' seam: a
/// test of a second look cannot wait that long); unset or not a number, the floor stands.
fn watch_pause(every: u64, floor_secs: u64) -> Duration {
    let floor = std::env::var("VAS_WATCH_FLOOR_MS")
        .ok()
        .and_then(|ms| ms.trim().parse::<u64>().ok())
        .map_or_else(|| Duration::from_secs(floor_secs), Duration::from_millis);
    Duration::from_secs(every).max(floor)
}

/// The notice that a `--watch` shorter than the floor was raised to it; `who` names the command.
fn floor_notice(who: &str, every: u64, floor_secs: u64) -> Option<String> {
    let pause = watch_pause(every, floor_secs);
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

/// The longest wait a rate limit may ask of ONE look before `vas` gives up instead of waiting: vast.ai
/// budgets whole windows of calls (ONE per window for the instances list, seen 2026-10-18), and a
/// person running `vas` twice in a row should not meet an error for a wait of a few seconds.
const PATIENCE_SECS: f64 = 15.0;

/// Added to every wait for vast.ai's reset: its clock and this machine's differ by a little.
const MARGIN_SECS: f64 = 1.0;

/// The longest a watch waits for a request budget to renew. vast.ai's windows are seconds long
/// (seen 2026-10-18); a reset further away than this is a mis-scaled header (milliseconds, say) or a
/// server that wants the watch gone, and a watch that slept for it would look hung.
const MAX_BUDGET_WAIT_SECS: f64 = 900.0;

/// Sits out `wait` seconds of a rate limit, and the margin, after saying so on stderr:
/// `{who}: rate-limited: waiting N s{then}`. The callers bound `wait` by [`PATIENCE_SECS`].
fn sit_out(who: &str, wait: f64, then: &str) {
    eprintln!(
        "{who}: rate-limited: waiting {:.0} s{then}",
        wait.ceil().max(1.0)
    );
    std::thread::sleep(Duration::from_secs_f64(wait + MARGIN_SECS));
}

/// How long to wait for a budget that renews in `wait_secs`: that and a margin, never more than
/// [`MAX_BUDGET_WAIT_SECS`] (nor a panic on an absurd or non-finite value).
fn budget_pause(wait_secs: f64) -> Duration {
    let wait = if wait_secs.is_finite() {
        wait_secs
    } else {
        0.0
    };
    Duration::from_secs_f64((wait + MARGIN_SECS).clamp(0.0, MAX_BUDGET_WAIT_SECS))
}

/// The wait a rate-limit refusal asks for, when it is short enough to sit out.
fn short_rate_wait<T>(result: &Result<T, vast_skopos::SkoposError>) -> Option<f64> {
    if let Err(vast_skopos::SkoposError::RateLimited {
        retry_after: Some(wait),
        ..
    }) = result
        && *wait <= PATIENCE_SECS
    {
        Some(*wait)
    } else {
        None
    }
}

/// How a look ends the run, when it does (`vas --until`): the exit code, and the line said on stderr
/// after the look's text.
struct Stop {
    /// The exit code.
    code: ExitCode,
    /// What is said, after `who: `.
    said: String,
}

/// The exit code of a wait that gave up: no box could be read for [`UNREAD_LOOKS`] looks in a row.
const GAVE_UP: u8 = 3;

/// The exit code of a refused value: a flag clap refuses (its own code, which a test pins to this),
/// a `--log` that cannot be a path on the box, and a wait whose pattern a box's `grep` refused.
const REFUSED: u8 = 2;

/// Runs `look` once, or every `--watch` seconds until Ctrl+C or a look's [`Stop`]; the exit code.
///
/// `who` names the command in what it says on stderr; `floor_secs` is the shortest pause the watch
/// accepts between two looks; `on_closed` is what a closed stdout means, when it is not the quiet
/// end of a run whose reader has gone (a `--until` wait: said, and exit 1). `look(missed)` is one
/// look: its text and, when it ends the run, its stop; or why it failed, `missed` being whether a
/// look failed since the last one that was printed. `budget()`
/// is how many seconds vast.ai's answers say to leave it alone (see `Client::budget_wait_secs`): a
/// look refused for a short rate limit waits it out ONCE, aloud, and a watch leaves at least that
/// long between two looks. A closed pipe (`vas --watch 60 | head`) ends the run quietly: the reader
/// is gone.
fn run_looks(
    who: &str,
    watch: Option<u64>,
    floor_secs: u64,
    on_closed: Option<&str>,
    budget: impl Fn() -> Option<f64>,
    mut look: impl FnMut(bool) -> Result<(String, Option<Stop>), vast_skopos::SkoposError>,
) -> ExitCode {
    if let Some(every) = watch
        && let Some(note) = floor_notice(who, every, floor_secs)
    {
        eprintln!("{note}");
    }
    let (mut succeeded, mut missed) = (false, false);
    loop {
        let mut result = look(missed);
        if let Some(wait) = short_rate_wait(&result) {
            sit_out(who, wait, ", then trying again");
            result = look(missed);
        }
        match result {
            Ok((text, stop)) => {
                let written = emit(&text);
                // A look that ends the run ends it whether its text reached the reader or not.
                if let Some(Stop { code, said }) = stop {
                    eprintln!("{who}: {said}");
                    return code;
                }
                if !written {
                    return on_closed.map_or(ExitCode::SUCCESS, |note| {
                        eprintln!("{who}: {note}");
                        ExitCode::FAILURE
                    });
                }
                (succeeded, missed) = (true, false);
            }
            Err(e) => {
                eprintln!("{who}: {e}");
                if ends_on_failure(watch.is_some(), succeeded) {
                    return ExitCode::FAILURE;
                }
                missed = true;
            }
        }
        let Some(every) = watch else {
            return ExitCode::SUCCESS;
        };
        let mut pause = watch_pause(every, floor_secs);
        if let Some(wait) = budget() {
            let waited = budget_pause(wait);
            if waited > pause {
                if wait + MARGIN_SECS > MAX_BUDGET_WAIT_SECS {
                    eprintln!(
                        "{who}: vast.ai's request budget renews in {wait:.0} s, more than a watch waits: the next look waits {:.0} s",
                        waited.as_secs_f64()
                    );
                } else {
                    eprintln!(
                        "{who}: vast.ai's request budget renews in {:.0} s: the next look waits for it",
                        waited.as_secs_f64()
                    );
                }
                pause = waited;
            }
        }
        std::thread::sleep(pause);
    }
}

/// One look: the instances, then the account while the probes run (asking `log` of each box, when
/// given), then the table; and, under `--until`, what the look says to the wait.
///
/// The account request needs nothing from the probes, which take seconds: the two overlap. With a
/// `trend` (a watch that probes), this look's GPU samples join the history, and the table and the
/// JSON show it. With a `credit_log`, this look's credit and burn are appended to it.
fn look(
    client: &Client,
    args: &LookArgs,
    log: Option<&LogQuery>,
    trend: Option<&mut Trend>,
    credit_log: Option<&mut CreditLog>,
    missing_said: &mut Vec<u64>,
) -> Result<(String, Option<Verdict>), vast_skopos::SkoposError> {
    // The list pages by 25 and its budget is one call per window: between two pages a short wait is
    // sat out (said aloud), a long one ends the look like any other rate limit.
    let all = client.instances_with(|wait| {
        let sit = wait <= PATIENCE_SECS;
        if sit {
            sit_out("vas", wait, " before the next page of instances");
        }
        sit
    })?;
    // `--instance`: the boxes named, in the API's order; the money stays the whole account's.
    let named: Vec<Instance>;
    let instances: &[Instance] = if args.instance.is_empty() {
        &all
    } else {
        named = all
            .iter()
            .filter(|i| args.instance.contains(&i.id))
            // BORROW: explicit clone -- the rows shown are kept beside the whole list
            .cloned()
            .collect();
        &named
    };
    let missing = missing_ids(&args.instance, &all);
    if missing != *missing_said {
        eprintln!("vas: {}", missing_note(&missing, all.len()));
        *missing_said = missing;
    }
    let (account, probes) = std::thread::scope(|scope| {
        let account = scope.spawn(|| client.account());
        let probes = if args.ssh {
            probe_all(instances, args.identity.as_deref(), log)
        } else {
            Vec::new()
        };
        (account.join(), probes)
    });
    let account = match account {
        Ok(Ok(a)) => Some(a),
        Ok(Err(e)) => {
            eprintln!("vas: no credit shown: {e}");
            None
        }
        Err(_) => {
            eprintln!("vas: no credit shown: the account request panicked");
            None
        }
    };
    // One time for the whole look: the log's row, the table and the JSON all carry it.
    let at = now();
    if let Some(log) = credit_log
        && let Some(failure) = log.record(&Runway::of(&all, account.as_ref()), at)
    {
        eprintln!("vas: {failure}; the credit log is not written any more");
    }
    let trend: Option<&Trend> = trend.map(|history| {
        history.record(instances, &probes);
        &*history
    });
    let waited = args.until.is_some().then(|| verdict(instances, &probes));
    let text = if args.compact {
        json_lines_of(instances, &all, &probes, trend, account.as_ref(), at)
    } else {
        let mut options = TableOptions::new().ssh(args.ssh).wide(args.wide);
        if let Some(history) = trend {
            options = options.trend(history);
        }
        table_of(instances, &all, &probes, account.as_ref(), options, at)
    };
    Ok((text, waited))
}

/// The ids `named` (`--instance`) that no box of `all` has, sorted, each once.
fn missing_ids(named: &[u64], all: &[Instance]) -> Vec<u64> {
    let mut missing: Vec<u64> = named
        .iter()
        .copied()
        .filter(|id| !all.iter().any(|i| i.id == *id))
        .collect();
    missing.sort_unstable();
    missing.dedup();
    missing
}

/// What is said when the ids `--instance` named that the account lacks change to `missing`, the
/// account holding `boxes` boxes: said when the set changes, so a watch does not repeat it.
fn missing_note(missing: &[u64], boxes: usize) -> String {
    match missing {
        [] => String::from("--instance: every box named is in the account now"),
        [id] => format!("--instance: no box {id} in the account (it has {boxes})"),
        ids => {
            let ids: Vec<String> = ids.iter().map(u64::to_string).collect();
            format!(
                "--instance: no boxes {} in the account (it has {boxes})",
                ids.join(", ")
            )
        }
    }
}

/// How a wait's end ends the run: exit 0 on a match, [`REFUSED`] on a refused pattern, [`GAVE_UP`]
/// when no box could be read or none had the file for `log_wait` seconds, each said on stderr.
fn wait_stop(end: End, log_wait: u64) -> Stop {
    match end {
        End::Matched { id, line } => Stop {
            code: ExitCode::SUCCESS,
            // As the table says it: a line of spaces only is empty once cleaned.
            said: if line.is_empty() {
                format!("--until: box {id} matched: (a blank line)")
            } else {
                format!("--until: box {id} matched: {line}")
            },
        },
        End::Refused { id } => Stop {
            code: ExitCode::from(REFUSED),
            said: format!(
                "--until: grep on box {id} refused the pattern (not an extended regular expression it reads, or a grep without -a): no look can match"
            ),
        },
        End::NoFile => Stop {
            code: ExitCode::from(GAVE_UP),
            said: format!(
                "--until: no box has had a file matching --log for {log_wait} s (--log-wait): giving up (is the path right?)"
            ),
        },
        // `End` is `#[non_exhaustive]` and the binary is another crate: an end it does not know is
        // read as a wait that gave up.
        End::GaveUp | _ => Stop {
            code: ExitCode::from(GAVE_UP),
            said: format!(
                "--until: no box could be read for {UNREAD_LOOKS} looks in a row (every probe failed or none could be made; a box not running keeps a wait for {STARTING_LOOKS} looks at most): giving up"
            ),
        },
    }
}

/// Git Bash's root as a Windows path (`C:/Program Files/Git/`), when `vas` was started from Git
/// Bash: it sets `MSYSTEM`, and `EXEPATH` (see [`root_of_exepath`]); `None` otherwise.
fn git_bash_root() -> Option<String> {
    std::env::var_os("MSYSTEM").filter(|v| !v.is_empty())?;
    root_of_exepath(&std::env::var("EXEPATH").ok()?)
}

/// Git Bash's root, with `/` separators and one at the end, from its `EXEPATH`: the folder of the
/// program that started the shell. That is the root itself from the Start menu's `git-bash.exe`
/// (`C:\Program Files\Git`), its `bin` from an editor's terminal, which starts `bin\bash.exe`
/// (`C:\Program Files\Git\bin`, seen 2026-10-28 in VS Code), or `usr\bin`. `None` when it is
/// blank.
fn root_of_exepath(exepath: &str) -> Option<String> {
    let exe = exepath.replace('\\', "/");
    let exe = exe.trim_end_matches('/');
    let root = exe
        .strip_suffix("/usr/bin")
        .or_else(|| exe.strip_suffix("/bin"))
        .unwrap_or(exe);
    (!root.trim().is_empty()).then(|| format!("{root}/"))
}

/// What became of a path on the box (or a pattern) on its way to `vas`.
#[derive(Debug, PartialEq, Eq)]
enum ForTheBox {
    /// As typed.
    As,
    /// Git Bash had rewritten it under its root: this is the path as typed.
    Repaired(String),
    /// A Windows path, which no Linux box can have.
    Refused,
    /// A pattern, kept as typed, that starts with a drive letter under Git Bash: it may have been
    /// a POSIX path that Git Bash rewrote outside its root.
    Doubted,
}

/// Whether `text` starts with a Windows drive (`C:/`, `D:\`).
const fn starts_with_drive(text: &str) -> bool {
    matches!(text.as_bytes(), [letter, b':', b'/' | b'\\', ..] if letter.is_ascii_alphabetic())
}

/// `typed` as the box should read it. Git Bash rewrites an argument that looks like a POSIX path
/// (`/root/run.log`) into a Windows one under its own root (`C:/Program Files/Git/root/run.log`)
/// before a Windows program sees it (seen 2026-10-26: `--log` then matched no file); when `root` (see
/// [`git_bash_root`]) starts `typed`, it is taken off again. A path that still starts with a drive
/// letter (`D:/run.log`, or `/tmp/...`, which Git Bash maps outside its root) cannot be a path on a
/// Linux box: refused when `paths_only` (the `--log` path); a pattern is kept, but doubted under
/// Git Bash.
fn for_the_box(typed: &str, root: Option<&str>, paths_only: bool) -> ForTheBox {
    if let Some(root) = root
        && let Some(head) = typed.get(..root.len())
        && head.eq_ignore_ascii_case(root)
    {
        // `head` ended on a character boundary of `typed`, so the rest starts on one.
        let rest = typed.get(root.len()..).unwrap_or_default();
        return ForTheBox::Repaired(format!("/{rest}"));
    }
    match (starts_with_drive(typed), paths_only, root) {
        (true, true, _) => ForTheBox::Refused,
        (true, false, Some(_)) => ForTheBox::Doubted,
        (false, _, _) | (true, false, None) => ForTheBox::As,
    }
}

/// The `--log` path and the `--grep` / `--until` pattern as the box should read them, each said on
/// stderr when Git Bash had rewritten it; on a `--log` that cannot be a path on the box, the exit
/// code [`REFUSED`] (said too), before any request.
fn typed_for_the_box(args: &LookArgs) -> Result<(Option<String>, Option<String>), ExitCode> {
    let root = git_bash_root();
    let take = |flag: &str, typed: &str, paths_only: bool| match for_the_box(
        typed,
        root.as_deref(),
        paths_only,
    ) {
        // BORROW: explicit to_owned -- the query owns what it sends
        ForTheBox::As => Ok(typed.to_owned()),
        ForTheBox::Repaired(read) => {
            eprintln!("vas: {flag}: Git Bash rewrote {read} as {typed}; read as {read}");
            Ok(read)
        }
        ForTheBox::Refused => {
            let help = if root.is_some() {
                ": from Git Bash, write it //root/... or set MSYS_NO_PATHCONV=1"
            } else {
                ""
            };
            eprintln!(
                "vas: {flag} {typed} is not a path on the box (a Linux path starts with /){help}"
            );
            Err(ExitCode::from(REFUSED))
        }
        ForTheBox::Doubted => {
            eprintln!(
                "vas: {flag}: {typed} is sent as typed; if it began with / Git Bash rewrote it: write it //... or set MSYS_NO_PATHCONV=1"
            );
            // BORROW: explicit to_owned -- the query owns what it sends
            Ok(typed.to_owned())
        }
    };
    let path = args
        .log
        .as_deref()
        .map(|p| take("--log", p, true))
        .transpose()?;
    let pattern = match (args.grep.as_deref(), args.until.as_deref()) {
        (Some(p), _) => Some(take("--grep", p, false)?),
        (None, Some(p)) => Some(take("--until", p, false)?),
        (None, None) => None,
    };
    Ok((path, pattern))
}

/// `vas` without a subcommand: the lookout, once or every `--watch` seconds.
fn lookout(args: &LookArgs) -> ExitCode {
    let (path, pattern) = match typed_for_the_box(args) {
        Ok(typed) => typed,
        Err(code) => return code,
    };
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
    // The GPU samples of the watch, in memory: one look says little, the last few say more.
    let mut trend = Trend::default();
    let tracking = args.ssh && args.watch.is_some();
    let mut credit_log = args.credit_log.as_deref().map(CreditLog::new);
    // `--until` asks for the line it waits for as `--grep` would (the two are refused together).
    let log = path.map(|path| {
        let query = LogQuery::new(path);
        match pattern {
            Some(pattern) => query.grep(pattern),
            None => query,
        }
    });
    let mut wait = Wait::default().no_file_secs(args.log_wait);
    let mut missing_said: Vec<u64> = Vec::new();
    run_looks(
        "vas",
        args.watch,
        LOOKOUT_FLOOR_SECS,
        args.until
            .is_some()
            .then_some("--until: stdout was closed before a match"),
        || client.budget_wait_secs(),
        |_| {
            let (text, waited) = look(
                &client,
                args,
                log.as_ref(),
                tracking.then_some(&mut trend),
                credit_log.as_mut(),
                &mut missing_said,
            )?;
            let stop = waited
                .and_then(|v| wait.record(v))
                .map(|end| wait_stop(end, args.log_wait));
            if stop.is_none() && wait.warns_of_no_file() {
                eprintln!(
                    "vas: --until: no box has a file matching --log ({NO_FILE_WARN_LOOKS} looks): is the path right? The wait gives up after {} s without one (--log-wait)",
                    args.log_wait
                );
            }
            // JSON Lines carry their own `at`: a stamp line would not parse.
            let text = if args.watch.is_some() && !args.compact {
                let secs = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_or(0, |d| d.as_secs());
                format!("== vas, unix time {secs}\n{text}")
            } else {
                text
            };
            Ok((text, stop))
        },
    )
}

/// The `offers` flags again, the config file's first and the command line's after; on failure,
/// the exit code (the error already printed).
fn with_file(path: &Path) -> Result<OffersArgs, ExitCode> {
    let words = read_flag_file(path).map_err(|e| {
        eprintln!("vas offers: {e}");
        ExitCode::FAILURE
    })?;
    // The first parse accepted `offers` with no `vas` flag before it: what follows the first
    // `offers` (after the program name) is the command line's own.
    let own = std::env::args_os()
        .skip(1)
        .skip_while(|a| a != "offers")
        .skip(1);
    let mut merged = vec![OsString::from("vas"), OsString::from("offers")];
    merged.extend(words.into_iter().map(OsString::from));
    merged.extend(own);
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
            Err(ExitCode::from(
                u8::try_from(e.exit_code()).unwrap_or(REFUSED),
            ))
        }
    }
}

/// The flags `vas offers` runs with: the config file's merged under the command line's, and the
/// file that was read, if any; on failure, the exit code (the error already printed).
fn resolve(args: OffersArgs) -> Result<(OffersArgs, Option<PathBuf>), ExitCode> {
    let file = if args.no_config {
        None
    } else {
        // BORROW: explicit clone -- the file read is kept beside the merged arguments
        args.config.clone().or_else(default_offers_file)
    };
    let Some(path) = file.as_deref() else {
        return Ok((args, None));
    };
    let merged = with_file(path)?;
    Ok((merged, file))
}

/// `vas offers`: the config file's flags, then the command line's; one search, or one every
/// `--watch` seconds; the table, or with `--changes` only what moved.
fn offers(args: OffersArgs) -> ExitCode {
    let (args, file) = match resolve(args) {
        Ok(resolved) => resolved,
        Err(code) => return code,
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
    let top = usize::try_from(args.top).unwrap_or(usize::MAX);
    let columns = if args.wide {
        OfferColumns::Wide
    } else {
        OfferColumns::Plain
    };
    let nothing = OfferChanges::default();
    let mut market = MarketWatch::default();
    run_looks(
        "vas offers",
        args.watch,
        OFFERS_FLOOR_SECS,
        None,
        || client.budget_wait_secs(),
        |missed| {
            let page = client.offers(&query, sort)?;
            // BORROW: explicit clone -- the footer reads the page again
            let kept = select(page.offers.clone(), &query, sort);
            let at = now();
            let moved = market.record(&kept);
            let text = if args.changes {
                offers_changes_only(&page, moved.as_ref(), &kept, missed, at)
            } else {
                // A blank line parts the looks of a watch.
                let gap = if market.looks() > 1 { "\n" } else { "" };
                let table = if args.watch.is_some() {
                    offers_table_marked(
                        &kept,
                        top,
                        columns,
                        moved.as_ref().unwrap_or(&nothing),
                        &market,
                    )
                } else {
                    offers_table(&kept, top, columns)
                };
                let footer = offers_footer(&page, &kept, top, &query, sort, at);
                // One search is a sample: a one-shot says so every time, a watch on its first look.
                let variance = if args.watch.is_none() || market.looks() == 1 {
                    offers_variance_note(args.watch.is_some())
                } else {
                    String::new()
                };
                let shown = kept.get(..top).unwrap_or(&kept);
                let note = moved
                    .as_ref()
                    .map(|m| offers_changes_note(&page, m, shown, missed))
                    .unwrap_or_default();
                format!("{gap}{table}{footer}{variance}{note}")
            };
            Ok((text, None))
        },
    )
}

/// The skill as `vas --skill` prints it: with LF line ends whatever the checkout's, so every `vas`
/// prints the same file.
fn skill_text() -> String {
    SKILL.replace("\r\n", "\n")
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Some(Command::Offers(args)) => offers(args),
        // A closed pipe ends the output quietly, as for a look: the reader has gone.
        None if cli.look.skill => {
            emit(&skill_text());
            ExitCode::SUCCESS
        }
        None => lookout(&cli.look),
    }
}

#[cfg(test)]
// EXPLICIT: test code -- a failed unwrap, or a panic naming the example that failed, IS the failed
// assertion.
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use std::time::Duration;

    use clap::{CommandFactory, Parser};
    use vast_skopos::market::{ABSENT_FOR_GONE, LEARNING_LOOKS};
    use vast_skopos::until::UNREAD_LOOKS;

    use super::{
        Cli, ForTheBox, GAVE_UP, LOOKOUT_FLOOR_SECS, MARGIN_SECS, MAX_BUDGET_WAIT_SECS,
        OFFERS_FLOOR_SECS, REFUSED, budget_pause, for_the_box,
    };
    use vast_skopos::until::{NO_FILE_SECS, NO_FILE_WARN_LOOKS, STARTING_LOOKS};

    const ROOT: Option<&str> = Some("C:/Program Files/Git/");

    #[test]
    fn the_ids_named_that_the_account_lacks_are_found_sorted_and_once() {
        use super::{missing_ids, missing_note};
        use vast_skopos::instance::Instance;
        let all: Vec<Instance> = [56_362_434_u64, 56_363_476]
            .iter()
            .map(|id| Instance::from_json(&serde_json::json!({ "id": id })).unwrap())
            .collect();
        assert_eq!(missing_ids(&[], &all), Vec::<u64>::new());
        assert_eq!(
            missing_ids(&[56_363_476, 56_362_434], &all),
            Vec::<u64>::new()
        );
        assert_eq!(missing_ids(&[9, 1, 56_363_476, 9], &all), [1, 9]);
        assert_eq!(
            missing_ids(&[5], &[]),
            [5],
            "an empty account lacks every box"
        );
        assert_eq!(
            missing_note(&[1], 2),
            "--instance: no box 1 in the account (it has 2)"
        );
        assert_eq!(
            missing_note(&[1, 9], 2),
            "--instance: no boxes 1, 9 in the account (it has 2)"
        );
        assert_eq!(
            missing_note(&[], 3),
            "--instance: every box named is in the account now"
        );
    }

    #[test]
    fn a_path_git_bash_rewrote_under_its_root_is_read_as_typed_again() {
        // As reproduced on 2026-10-26: `--log /root/.../run.log` reached vas rewritten.
        let typed = "C:/Program Files/Git/root/src/askesis/reference/canvas/data/runs/me38_l12_s1_d1/run.log";
        assert_eq!(
            for_the_box(typed, ROOT, true),
            ForTheBox::Repaired(String::from(
                "/root/src/askesis/reference/canvas/data/runs/me38_l12_s1_d1/run.log"
            ))
        );
        assert_eq!(
            for_the_box("c:/program files/git/root/x.log", ROOT, true),
            ForTheBox::Repaired(String::from("/root/x.log")),
            "a drive letter's case does not matter"
        );
        assert_eq!(
            for_the_box("C:/Program Files/Git/", ROOT, true),
            ForTheBox::Repaired(String::from("/"))
        );
        // A pattern under the root is repaired too, never refused.
        assert_eq!(
            for_the_box("C:/Program Files/Git/epoch", ROOT, false),
            ForTheBox::Repaired(String::from("/epoch"))
        );
    }

    #[test]
    fn git_bash_s_root_is_read_from_exepath_whichever_program_started_the_shell() {
        use super::root_of_exepath;
        for exepath in [
            "C:\\Program Files\\Git\\bin",
            "C:\\Program Files\\Git\\usr\\bin\\",
            "C:\\Program Files\\Git",
            "C:/Program Files/Git/",
        ] {
            assert_eq!(root_of_exepath(exepath).as_deref(), ROOT, "{exepath}");
        }
        for blank in ["", "  ", "/", "/bin", "\\usr\\bin"] {
            assert_eq!(root_of_exepath(blank), None, "{blank:?}");
        }
    }

    #[test]
    fn a_windows_path_elsewhere_is_refused_as_a_log_path_and_kept_as_a_pattern() {
        // `/tmp/...` is mapped outside Git Bash's root (the user's temp folder): no repair is safe.
        let temp = "C:/Users/me/AppData/Local/Temp/me38_speedcheck/g12_long.log";
        assert_eq!(for_the_box(temp, ROOT, true), ForTheBox::Refused);
        assert_eq!(
            for_the_box("D:\\runs\\x.log", None, true),
            ForTheBox::Refused
        );
        // A pattern is the user's: kept, but doubted under Git Bash only.
        assert_eq!(for_the_box(temp, ROOT, false), ForTheBox::Doubted);
        assert_eq!(for_the_box(temp, None, false), ForTheBox::As);
        // What a box can have is left alone, with or without Git Bash.
        for path in [
            "/root/runs/*/run.log",
            "//root/x.log",
            "runs/x.log",
            "C:x",
            "loss|C:/",
        ] {
            assert_eq!(for_the_box(path, ROOT, true), ForTheBox::As, "{path}");
            assert_eq!(for_the_box(path, None, true), ForTheBox::As, "{path}");
        }
    }

    #[test]
    fn a_budget_is_waited_for_with_a_margin_and_never_longer_than_a_watch_waits() {
        assert_eq!(
            budget_pause(2.0),
            Duration::from_secs_f64(2.0 + MARGIN_SECS)
        );
        assert_eq!(budget_pause(-5.0), Duration::ZERO, "a reset in the past");
        let most = Duration::from_secs_f64(MAX_BUDGET_WAIT_SECS);
        assert_eq!(budget_pause(MAX_BUDGET_WAIT_SECS), most);
        assert_eq!(budget_pause(1.79e12), most, "a reset in milliseconds");
        assert_eq!(budget_pause(f64::MAX), most, "no panic on an absurd value");
        assert_eq!(
            budget_pause(f64::INFINITY),
            Duration::from_secs_f64(MARGIN_SECS)
        );
        assert_eq!(budget_pause(f64::NAN), Duration::from_secs_f64(MARGIN_SECS));
    }

    /// The lookout's command (`None`) or `vas offers`'s.
    fn command_of(subcommand: Option<&str>) -> clap::Command {
        let command = Cli::command();
        match subcommand {
            Some(name) => command.find_subcommand(name).unwrap().clone(),
            None => command,
        }
    }

    /// The `--help` text of the flag `id` of the lookout (`None`) or of `vas offers`: its whole doc
    /// comment (a flag of one paragraph has no long help, and prints its short one).
    fn long_help(subcommand: Option<&str>, id: &str) -> String {
        command_of(subcommand)
            .get_arguments()
            .find(|a| a.get_id() == id)
            .and_then(|a| a.get_long_help().or_else(|| a.get_help()))
            .unwrap()
            .to_string()
    }

    fn word(n: usize) -> &'static str {
        [
            "zero", "one", "two", "three", "four", "five", "six", "seven",
        ]
        .get(n)
        .copied()
        .unwrap_or("many")
    }

    #[test]
    fn the_help_of_each_watch_says_the_floor_and_the_looks_the_code_uses() {
        let lookout = long_help(None, "watch");
        assert!(
            lookout.contains(&format!("at least {LOOKOUT_FLOOR_SECS})")),
            "{lookout}"
        );
        let offers = long_help(Some("offers"), "watch");
        assert!(
            offers.contains(&format!("at least {OFFERS_FLOOR_SECS})")),
            "{offers}"
        );
        assert!(
            offers.contains(&format!("first {} looks only learn", word(LEARNING_LOOKS))),
            "{offers}"
        );
        assert!(
            offers.contains(&format!("missing for {} looks", word(ABSENT_FOR_GONE))),
            "{offers}"
        );
    }

    #[test]
    fn the_help_of_until_says_the_looks_and_the_exit_code_the_code_uses() {
        let help = long_help(None, "until");
        assert!(
            help.contains(&format!(
                "Exits {REFUSED} at once when a box's grep refuses the pattern (and no other box was read), {GAVE_UP} when no box could be read for {UNREAD_LOOKS} looks in a row"
            )),
            "{help}"
        );
        assert!(
            help.contains(&format!(
                "for {STARTING_LOOKS} looks at most since a box was last read"
            )),
            "{help}"
        );
        assert!(
            help.contains(&format!(
                "no box has had a file matching --log for --log-wait seconds (a warning comes after {NO_FILE_WARN_LOOKS} looks)"
            )),
            "{help}"
        );
        let command = Cli::command();
        let log_wait = command
            .get_arguments()
            .find(|a| a.get_id() == "log_wait")
            .unwrap();
        assert_eq!(
            log_wait.get_default_values(),
            [NO_FILE_SECS.to_string().as_str()]
        );
        assert_eq!(
            NO_FILE_SECS,
            15 * 60,
            "15 minutes, as the reference and the FAQ say"
        );
    }

    #[test]
    fn the_helps_name_what_the_ssh_probe_reads_and_the_states_of_a_log() {
        let help = |name: &str| long_help(None, name);
        let ssh = help("ssh");
        for what in [
            "utilisation",
            "memory",
            "power",
            "clock",
            "throttle",
            "anyone else",
        ] {
            assert!(ssh.contains(what), "--ssh help lacks {what}: {ssh}");
        }
        assert!(help("log").contains("\"?\""), "{}", help("log"));
        let offers = command_of(Some("offers"));
        let about = offers.get_long_about().unwrap().to_string();
        assert!(about.contains("unless you sort otherwise"), "{about}");
        assert!(
            offers
                .get_about()
                .unwrap()
                .to_string()
                .contains("no API key")
        );
    }

    /// The words of one shell command line, as `sh` splits them: blanks part words, and single or
    /// double quotes keep a blank inside one (enough for the examples, which use no escapes).
    fn shell_words(line: &str) -> Vec<String> {
        let (mut words, mut word, mut quote, mut open) = (Vec::new(), String::new(), None, false);
        // EXPLICIT: a loop, not an iterator chain: the open quote and the word being built are state
        // carried from one character to the next.
        for c in line.chars() {
            match (quote, c) {
                (Some(q), c) if c == q => quote = None,
                (Some(_), c) => word.push(c),
                (None, '\'' | '"') => (quote, open) = (Some(c), true),
                (None, c) if c.is_whitespace() => {
                    if open {
                        words.push(std::mem::take(&mut word));
                        open = false;
                    }
                }
                (None, c) => {
                    word.push(c);
                    open = true;
                }
            }
        }
        assert_eq!(quote, None, "a quote is left open in {line}");
        if open {
            words.push(word);
        }
        words
    }

    /// The `vas` command of a shell line: from its `vas` word up to the first `|`, `&&`, `;`, `>`
    /// or `2>` (what follows is another program's, or a file); what comes before the `vas` word (a
    /// `timeout N`, say) is dropped. `None` when the line runs no `vas`.
    fn vas_command(line: &str) -> Option<Vec<String>> {
        let words = shell_words(line);
        let from = words.iter().position(|w| w == "vas")?;
        Some(
            words
                .into_iter()
                .skip(from)
                // BORROW: explicit as_str -- a word is matched against the shell's operators
                .take_while(|w| !matches!(w.as_str(), "|" | "&&" | ";" | ">" | ">>" | "2>"))
                .collect(),
        )
    }

    #[test]
    fn shell_words_are_split_as_sh_splits_them() {
        assert_eq!(
            shell_words("timeout 3600 vas --log '/root/runs/*/x.log' --until \"a b\" | head"),
            [
                "timeout",
                "3600",
                "vas",
                "--log",
                "/root/runs/*/x.log",
                "--until",
                "a b",
                "|",
                "head"
            ]
        );
        assert_eq!(shell_words("vas --until ''"), ["vas", "--until", ""]);
        assert_eq!(
            vas_command("timeout 60 vas --compact | jq .").unwrap(),
            ["vas", "--compact"]
        );
        assert_eq!(vas_command("jq -r .kind"), None);
    }

    /// The commands under `Examples:` in an after-help block: its lines indented by four spaces.
    fn examples(block: &str) -> Vec<Vec<String>> {
        let examples = block
            .split("\n\n")
            .find(|part| part.starts_with("Examples:"))
            .unwrap();
        examples
            .lines()
            .filter(|l| l.starts_with("    "))
            .map(|l| vas_command(l).unwrap_or_else(|| panic!("no vas in {l}")))
            .collect()
    }

    #[test]
    fn every_example_of_the_helps_parses_with_the_flags_vas_has() {
        use super::{LOOKOUT_AFTER_HELP, OFFERS_AFTER_HELP};
        // As many as each block shows: an example the parser stopped seeing would leave the count.
        for (block, count) in [(LOOKOUT_AFTER_HELP, 5), (OFFERS_AFTER_HELP, 4)] {
            let commands = examples(block);
            assert_eq!(commands.len(), count, "{commands:?}");
            for command in commands {
                if let Err(e) = Cli::try_parse_from(&command) {
                    panic!("{command:?} does not parse: {e}");
                }
            }
        }
        // The empty test: a flag `vas` lacks fails the parse, so the check above can fail.
        assert!(Cli::try_parse_from(["vas", "--no-such-flag"]).is_err());
    }

    #[test]
    fn each_short_help_is_one_line_and_the_detail_stays_in_the_long_help() {
        /// The longest a flag's `-h` summary may grow: past it, it is a paragraph again.
        const SUMMARY: usize = 72;
        for subcommand in [None, Some("offers")] {
            let command = command_of(subcommand);
            for arg in command.get_arguments() {
                // BORROW: explicit as_str -- the id is matched against clap's own flags
                let id = arg.get_id().as_str();
                if matches!(id, "help" | "version") {
                    continue;
                }
                let short = arg.get_help().unwrap().to_string();
                assert!(
                    !short.contains('\n') && short.chars().count() <= SUMMARY,
                    "{subcommand:?} --{id}: the -h summary is {} characters: {short}",
                    short.chars().count()
                );
            }
        }
        let offers = command_of(Some("offers"));
        assert!(offers.get_about().unwrap().to_string().chars().count() <= SUMMARY);
        // `-h` is the summaries; `--help` adds the details and the blocks after them.
        let short = Cli::command().render_help().to_string();
        let long = Cli::command().render_long_help().to_string();
        assert!(!short.contains("Exit codes:") && long.contains("Exit codes:"));
        assert!(
            !short.contains("throttle reasons") && long.contains("throttle reasons"),
            "the detail of --ssh is in --help only"
        );
        assert!(short.contains("see more with '--help'"), "{short}");
    }

    #[test]
    fn the_exit_codes_and_the_environment_of_the_helps_are_the_code_s() {
        use super::{LOOKOUT_AFTER_HELP, OFFERS_AFTER_HELP};
        use vast_skopos::api::DEFAULT_BASE;
        let lookout = LOOKOUT_AFTER_HELP;
        assert!(
            lookout.contains(&format!(
                "\n  {REFUSED}  A flag or value was refused, before any request"
            )) && lookout.contains("or a box's grep refused the --until pattern"),
            "{lookout}"
        );
        // A refused flag is clap's code, which `vas`'s own refusals use too (`REFUSED`).
        let refused = Cli::try_parse_from(["vas", "--no-such-flag"]).unwrap_err();
        assert_eq!(refused.exit_code(), i32::from(REFUSED));
        assert!(
            lookout.contains(&format!(
                "\n  {GAVE_UP}  A wait gave up: no box could be read for {UNREAD_LOOKS} looks in a row"
            )),
            "{lookout}"
        );
        // `vas offers` never waits: no code 3 there.
        assert!(!OFFERS_AFTER_HELP.contains(&format!("\n  {GAVE_UP}  ")));
        for block in [lookout, OFFERS_AFTER_HELP] {
            assert!(
                block.contains("\n  0  ") && block.contains("\n  1  ") && block.contains("\n  2  ")
            );
            assert!(block.contains(&format!("than {DEFAULT_BASE} ")), "{block}");
            assert!(block.contains("XDG_CONFIG_HOME") && block.contains("VAST_URL"));
        }
        // The key files the help names, in the order of src/key.rs's module docs; the files the code
        // searches end with the same two names (the order itself is `key::candidate_files`'s).
        assert!(lookout.contains("VAST_API_KEY"));
        let named = [
            "$XDG_CONFIG_HOME/vastai/vast_api_key",
            "~/.config/vastai/vast_api_key",
            "~/.vast_api_key",
        ];
        let at: Vec<usize> = named.iter().map(|n| lookout.find(n).unwrap()).collect();
        assert!(at.is_sorted(), "{named:?} in the help's order");
        let searched = vast_skopos::key::candidate_files();
        let last = searched.last().unwrap();
        assert!(last.ends_with(".vast_api_key"), "{last:?}");
        assert!(
            searched.iter().any(|p| p.ends_with("vastai/vast_api_key")),
            "{searched:?}"
        );
    }

    /// The `vas` commands of the skill: each line of a fenced block that runs `vas`, and each
    /// inline code span that starts with `vas`.
    fn skill_commands(skill: &str) -> Vec<Vec<String>> {
        let mut fenced = false;
        let mut commands = Vec::new();
        // EXPLICIT: a loop, not an iterator chain: whether a line is inside a fence is state carried
        // from one line to the next.
        for line in skill.lines() {
            if line.starts_with("```") {
                fenced = !fenced;
            } else if fenced {
                commands.extend(vas_command(line));
            } else {
                // The odd pieces between backticks are the inline code spans.
                for span in line.split('`').skip(1).step_by(2) {
                    if span == "vas" || span.starts_with("vas ") {
                        commands.extend(vas_command(span));
                    }
                }
            }
        }
        commands
    }

    #[test]
    fn every_vas_command_of_the_skill_parses_with_the_flags_vas_has() {
        let skill = super::skill_text();
        let commands = skill_commands(&skill);
        assert!(commands.len() >= 20, "{} commands", commands.len());
        for command in commands {
            match Cli::try_parse_from(&command) {
                Ok(_) => {}
                // `vas -h` and `vas --help` are named, and print rather than parse.
                Err(e) if e.kind() == clap::error::ErrorKind::DisplayHelp => {}
                Err(e) => panic!("{command:?} does not parse: {e}"),
            }
        }
        // The empty test: a span with a flag `vas` lacks is found and fails.
        let wrong = skill_commands("Run `vas --sssh` here.");
        assert_eq!(wrong, [["vas", "--sssh"]]);
        assert!(Cli::try_parse_from(wrong.first().unwrap()).is_err());
    }

    #[test]
    fn vas_skill_prints_the_skill_alone_and_its_front_matter_is_an_agent_skill_s() {
        // ASCII, so PowerShell 5.1's `Out-File -Encoding ascii` (the README's) saves it exactly; and
        // printed with LF line ends whatever the checkout's (tests/look_cli.rs runs `vas --skill`).
        assert!(super::SKILL.is_ascii());
        let skill = super::skill_text();
        assert!(!skill.contains('\r'));
        let cli = Cli::try_parse_from(["vas", "--skill"]).unwrap();
        assert!(cli.look.skill && cli.command.is_none());
        for other in [
            &["vas", "--skill", "--ssh"][..],
            &["vas", "--compact", "--skill"],
            &["vas", "offers", "--skill"],
        ] {
            assert!(Cli::try_parse_from(other).is_err(), "{other:?}");
        }
        // The Agent Skills format: front matter with a `name` (lowercase, hyphens, at most 64) and a
        // `description` (at most 1024 characters), then the instructions.
        let front = skill
            .strip_prefix("---\n")
            .and_then(|rest| rest.split_once("\n---\n"))
            .map(|(front, _)| front)
            .unwrap();
        let field = |name: &str| {
            front
                .lines()
                .find_map(|l| l.strip_prefix(name)?.strip_prefix(": "))
                .unwrap()
        };
        assert_eq!(field("name"), "vas");
        let description = field("description");
        assert!(
            (100..=1024).contains(&description.chars().count()),
            "{description}"
        );
    }

    #[test]
    fn the_marks_the_help_and_the_skill_teach_are_the_ones_the_table_prints() {
        use super::LOOKOUT_AFTER_HELP;
        use vast_skopos::instance::Instance;
        use vast_skopos::probe::parse_output;
        use vast_skopos::render::{TableOptions, table};
        // What the probe reads on a box capped at 400 W with someone else on its GPU, on a 450 W
        // board waiting for a line (`--grep`), and on an idle card.
        let read = |util_mem: &str, power: &str, events: &str, apps: &str, max: &str, log: &str| {
            Some(Ok(parse_output(&format!(
                "0, {util_mem}, 32607\n---vas-power---\n0, {power}, 1717, 3090\n\
                 ---vas-throttle---\n0, {events}\n---vas-apps---\n0, 00000000:C1:00.0\n\
                 ---vas-apps-by-gpu---\n{apps}vas-apps-ok\n---vas-power-max---\n0, {max}\n{log}"
            ))))
        };
        let no_line = "---vas-log---\nvas-grep-file:/r/s0/train.log\nvas-grep-exit:1\n";
        let failed = |message: &str| {
            Some(Err(vast_skopos::SkoposError::Ssh {
                target: String::from("h:1"),
                message: String::from(message),
            }))
        };
        let probes = [
            read(
                "99, 24809",
                "399.9, 400, 575",
                "0x4",
                "00000000:C1:00.0, 8720\n",
                "600",
                "",
            ),
            read(
                "98, 20480",
                "441.2, 450, 450",
                "0x0",
                "00000000:C1:00.0, 20060\n",
                "450",
                no_line,
            ),
            read("100, 2", "31.4, 575, 575", "0x1", "", "600", ""),
            failed("ssh: connect to host h port 1: Connection refused"),
            failed("root@h: Permission denied (publickey)."),
            None,
        ];
        let rtx = |id: u64, status: &str| {
            Instance::from_json(&serde_json::json!({"id": id, "actual_status": status,
                "intended_status": "running", "gpu_name": "RTX 5090", "num_gpus": 1,
                "status_msg": "46b4edaa8d47: Pull complete"}))
            .unwrap()
        };
        let boxes = [
            rtx(1, "running"),
            rtx(2, "running"),
            rtx(3, "running"),
            rtx(4, "running"),
            rtx(5, "running"),
            rtx(6, "loading"),
        ];
        let printed = table(&boxes, &probes, None, TableOptions::new().ssh(true), 0.0);
        let skill = super::skill_text();
        let pull = "loading: 46b4edaa8d47: Pull complete";
        // Each mark the table prints here, whether --help teaches it, and whether the skill does.
        for (mark, in_help, in_skill) in [
            ("400/575W+power_cap", true, true),
            ("450W<575", true, true),
            ("100%?", true, true),
            ("1717/3090", true, true),
            ("15.7G", false, true),
            ("UNREACHABLE", true, true),
            ("SSH FAILED", true, true),
            ("(no line matches)", true, true),
            ("loading -> running", true, false),
            (pull, false, true),
        ] {
            assert!(printed.contains(mark), "the table lacks {mark}: {printed}");
            if in_help {
                assert!(
                    LOOKOUT_AFTER_HELP.contains(mark),
                    "--help does not teach {mark}"
                );
            }
            if in_skill {
                assert!(skill.contains(mark), "the skill does not teach {mark}");
            }
        }
    }
}
