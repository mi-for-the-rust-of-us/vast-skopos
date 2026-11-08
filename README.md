# vast-skopos

[![CI](https://github.com/mi-for-the-rust-of-us/vast-skopos/actions/workflows/ci.yml/badge.svg)](https://github.com/mi-for-the-rust-of-us/vast-skopos/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/vast-skopos.svg)](https://crates.io/crates/vast-skopos)
[![docs.rs](https://docs.rs/vast-skopos/badge.svg)](https://docs.rs/vast-skopos)
[![MSRV](https://img.shields.io/badge/MSRV-1.88-blue.svg)](https://www.rust-lang.org)
[![license](https://img.shields.io/badge/license-MIT_OR_Apache--2.0-blue.svg)](#license)
[![unsafe: forbid](https://img.shields.io/badge/unsafe-forbid-success.svg)](https://github.com/rust-secure-code/safety-dance/)
[![API: read-only](https://img.shields.io/badge/vast.ai_API-read--only_(GET)-informational.svg)](#why-read-only)

**σκοπός** — *Your rented vast.ai boxes, watched.* **Not affiliated with vast.ai:** an independent,
read-only tool that reads the vast.ai API with your own key, and your boxes over your own `ssh`.

> 🆕 **What's new in `0.5.0`:** `vas -h` fits on one screen and `vas --help` ends with examples, how to read the table, the exit codes and the environment; `vas --skill` prints a skill that teaches an AI agent to use `vas`; and this README is a third of its length, its reference now [a page of its own](docs/reference.md). Every release, and what each rental taught it, is in [`CHANGELOG.md`](CHANGELOG.md) and the [dogfooding reports](#documentation).

## Table of Contents

- [Install](#install)
- [Quick start](#quick-start)
- [A skill for AI agents](#a-skill-for-ai-agents)
- [The API key](#the-api-key)
- [`vas` and `vastai`](#vas-and-vastai)
- [As a library](#as-a-library)
- [Capabilities](#capabilities)
- [Feature Flags](#feature-flags)
- [Documentation](#documentation): the [reference](docs/reference.md) (every flag, column and mark), the FAQ, the tutorials
- [Used by](#used-by)
- [License](#license)
- [Development](#development)

> **New to vast-skopos?**
> - **What are my boxes doing, and costing, right now?** → [`vas`](docs/reference.md#binary-vas) — one table: status, `$/h`, age, cost so far, `ssh` endpoint, and the money line.
> - **Is the GPU really busy?** (the dashboard has read 0 % on boxes at 97-99 %) → [`vas --ssh`](docs/reference.md#binary-vas) — `nvidia-smi` read on each running box, over your own `ssh`.
> - **Which run is each box on, and what did it last write?** → [`vas --ssh --log '/root/runs/*/train.log'`](docs/reference.md#binary-vas) — the newest matching file, named, and its last non-blank line.
> - **How long will my credit last?** → the money line: `credit | burn $/h over N running | ~H h left at this rate`.
> - **A long rental to watch?** → `vas --ssh --log … --watch 120` — the table every two minutes, until Ctrl+C.
> - **Which boxes could I rent that pass my filters?** → [`vas offers`](docs/reference.md#vas-offers) — GPU, CUDA, disk, reliability, upload, countries and hosts left out, cheapest first; no key needed, and the filters can live in a config file.
> - **`no vast.ai API key found`?** → [The API key](#the-api-key) — where `vas` looks, and why there is no `--key` flag.
> - **The GPU reads 100 % but the run is slow?** → the `POWER`, `CLOCK` and `OTHER` columns of `vas --ssh` — [the FAQ](docs/FAQ.md#when-a-rental-is-slow) says what they mean.
> - **First rental?** → [From filters to a running box](docs/tutorials/from-filters-to-a-running-box.md), a walk through one rental, step by step.
> - **An AI assistant drives `vas` for you?** → [`vas --skill`](#a-skill-for-ai-agents) prints a skill that teaches it `vas`: which command answers which question, how to read each mark, the exit codes, and that renting or destroying a box stays your call.
>
> Every flag is in `vas -h`, one line each, and `vas --help` adds the detail, examples, how to read the table, the exit codes and the environment variables; how `vas` differs from Vast.ai's own CLI is in [`docs/comparison-with-vastai.md`](docs/comparison-with-vastai.md).

## Install

```sh
cargo install vast-skopos --locked   # installs the `vas` binary
```

`--locked` uses the dependency versions the crate was tested with (without it, `cargo install` resolves
the newest compatible ones). From a clone: `cargo install --path . --locked`. As a library, without the CLI (no `clap`):

```toml
[dependencies]
vast-skopos = { version = "0.5", default-features = false }
```

## Quick start

With your key where the `vastai` CLI keeps it ([The API key](#the-api-key)), three commands.

**`vas`: what your boxes are doing, and costing.** One row per box, then the money line: the credit, what
the boxes burn, and how long the credit lasts at that rate.

```text
$ vas
ID        LABEL  GPU          WHERE              STATUS   $/H    AGE     COST~  SSH
56533728  -      1x RTX 5090  United States, US  running  0.491  4h 26m  2.18   ssh5.vast.ai:24016
56558627  -      1x RTX 5090  Texas, US          running  0.704  51m     0.61   ssh2.vast.ai:20294
credit $9.33 | burn $1.195/h over 2 running | ~7.8 h left at this rate
```

A box that is not running yet says why under the table, in vast.ai's own words (`57340012 loading:
46b4edaa8d47: Pull complete`: the image arriving, not a stuck box).

**`vas --ssh`: what the GPUs are really doing,** read on each running box by `nvidia-smi`, over your own `ssh`
(the dashboard's utilisation has read 0 % on boxes at 97-99 %):

```text
$ vas --ssh
ID        LABEL  GPU          WHERE              STATUS   $/H    AGE     COST~  SSH                 GPU% (ssh)  VRAM      OTHER  POWER               CLOCK      LAST LOG / SSH ERROR
56533728  -      1x RTX 5090  United States, US  running  0.491  4h 26m  2.18   ssh5.vast.ai:24016  100%        24.2/32G  15.7G  400/575W+power_cap  1717/3090  -
56558627  -      1x RTX 5090  Texas, US          running  0.704  51m     0.61   ssh2.vast.ai:20294  99%         8.6/32G   -      400/575W+power_cap  2100/3090  -
credit $9.33 | burn $1.195/h over 2 running | ~7.8 h left at this rate
```

(Two rented RTX 5090s, 2026-10-18, one look of a watch; the `OTHER` of the first box is its reading minutes
later.) Both cards are capped by their hosts at 400 W against a 575 W default, and the first holds 15.7 GiB
that no process of its own accounts for: see [the power cap](docs/reference.md#the-power-cap-and-the-clock)
and [someone else on the GPU](docs/reference.md#someone-else-on-the-gpu). `--log '/root/runs/*/train.log'`
fills the last column with each run's last line, `--watch 60` refreshes the table every minute, and `--until`
waits for a line ([the tutorial](docs/tutorials/waiting-for-a-line.md)).

**`vas offers`: what you could rent, with no key at all.** The offers the author rents by (captured
2026-10-16: a search today also names its time and ends with the line saying that vast.ai's answers vary):

```text
$ vas offers --gpu RTX_5090 --num-gpus 1 --cuda 12.8 --disk 50 --reliability 0.99 --inet-up 200 --exclude-country CN --exclude-host 362355 --verified --top 5
ID        GPU          VRAM  $/H    CUDA  DISK   UP/DOWN Mb/s  REL %  MAX DAYS  HOST    VERIFICATION  WHERE
50584160  1x RTX 5090  32G   0.775  13.0  1022G  2004/2835     99.31  145.7     86295   verified      Estonia, EE
51905774  1x RTX 5090  32G   0.890  13.2  1309G  2242/2966     99.57  76.9      270760  verified      Denmark, DK
47238961  1x RTX 5090  32G   0.954  13.2  2391G  1745/1778     99.34  115.4     567249  verified      Hungary, HU
45618801  1x RTX 5090  32G   0.980  13.0  668G   6756/8514     99.46  60.8      256524  verified      California, US
52725385  1x RTX 5090  32G   1.014  13.2  271G   272/304       99.37  9.5       154140  verified      South Africa, ZA
5 of 17 offers shown, cheapest first | RTX 5090, 1 GPU, disk >= 50 GB, CUDA >= 12.8, reliability >= 99 %, up >= 200 Mb/s, verified, not in CN, not hosts 362355 | $/h includes 50 GB of storage
```

The filters you always use can live in a [config file](docs/reference.md#vas-offers).

**Next.** `vas -h` lists every flag in one line each; `vas --help` adds the detail, examples, how to read the
table, the exit codes and the environment variables. [`docs/reference.md`](docs/reference.md) has everything
in full: each column and mark, `--compact`'s JSON Lines for scripts, the offers' flags, config file and
watch. [The FAQ](docs/FAQ.md) answers what real rentals asked, and the
[first tutorial](docs/tutorials/from-filters-to-a-running-box.md) walks through one rental.

## A skill for AI agents

An AI assistant that reads `vas --help` gets every flag exactly, but not the overall picture. `vas --skill`
prints a skill, in the [Agent Skills](https://agentskills.io) format, that gives it: which command answers
which question, `--compact` whenever the output is parsed (and its keys), the exit codes and what to do after
each, how to read each mark (`400/575W+power_cap`, `450W<575`, `100%?`, `UNREACHABLE` against `SSH FAILED`),
and the rules (never print the key; `vas` only reads, and renting or destroying a box is yours to do). It is
[`skills/vas/SKILL.md`](skills/vas/SKILL.md), built into the binary, so an installed `vas` prints the skill of
its own flags; the tests parse every `vas` command in it and check every `--compact` key it names.

For [Claude Code](https://code.claude.com/docs/en/skills), in a project (or under `~/.claude/skills/` for every project):

```sh
mkdir -p .claude/skills/vas && vas --skill > .claude/skills/vas/SKILL.md
```

From Windows PowerShell 5.1, whose `>` writes UTF-16, write
`vas --skill | Out-File -Encoding ascii .claude\skills\vas\SKILL.md` (the skill is plain ASCII). Other agents
can be pointed at the file itself.

## The API key

`vas` finds your key where the official `vastai` CLI keeps it, in this order:

1. the `VAST_API_KEY` environment variable;
2. `$XDG_CONFIG_HOME/vastai/vast_api_key`;
3. `~/.config/vastai/vast_api_key`;
4. `~/.vast_api_key` (the CLI's legacy location);

or from `--key-file FILE`. There is deliberately no `--key` flag: a key typed on the command line
lands in your shell history. The key is only ever sent, as a `Bearer` header, to the API base:
`https://console.vast.ai`, or `VAST_URL` when you set it, in which case `vas` warns on stderr
where the key is going. No redirect is ever followed. The key is redacted from every debug
output and never appears in an error message. A key file may be UTF-8 (with or without a
byte-order mark) or UTF-16 with one, as Windows PowerShell writes it. When no key file is found
but a searched one exists with `.txt` appended (Windows Notepad's habit: `vast_api_key.txt`), the
error names it and says to rename it.

**Use a read-only key if you can** (on vast.ai: User Read and Instances Read, nothing else).

`vas offers` needs no key, and never looks for one: the offer search is public, and is sent
without any `Authorization` header.

## `vas` and `vastai`

`vastai`, Vast.ai's own CLI and SDK, rents and manages boxes; `vas` only watches them, read-only.
They are complements: [`docs/comparison-with-vastai.md`](docs/comparison-with-vastai.md) compares
them row by row.

### Why read-only?

`vas` only ever issues `GET` requests: it never creates, starts, stops, labels or destroys an
instance. `vas offers` searches, and rents nothing: to rent the offer you chose, take its `ID` to the
dashboard or to `vastai create instance`. A monitor that can spend or destroy is a different tool, with a different review; what to
do about a box (stop it, destroy it, top up the credit) stays a person's decision, taken in the
dashboard or with `vastai`.


## As a library

The lookout, as a table, in a few lines (the examples below are compiled by the tests, not run: they
need your key, your boxes or the network):

```rust,no_run
use vast_skopos::api::Client;
use vast_skopos::key::ApiKey;
use vast_skopos::probe::{LogQuery, probe_all};
use vast_skopos::render::{TableOptions, table};

fn main() -> vast_skopos::Result<()> {
    // The key, by the official `vastai` CLI's search order; it is never printed.
    let client = Client::new(ApiKey::discover()?);
    let instances = client.instances()?;
    let account = client.account().ok();
    // One `ssh` attempt per running box, all at once: what the GPUs do, and the newest log's last line.
    let probes = probe_all(&instances, None, Some(&LogQuery::new("/root/runs/*/train.log")));
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64());
    let options = TableOptions::new().ssh(true);
    print!("{}", table(&instances, &probes, account.as_ref(), options, now));
    Ok(())
}
```

Or the readings one by one, over your own `ssh` (one attempt, never retried):

```rust,no_run
use vast_skopos::api::Client;
use vast_skopos::key::ApiKey;
use vast_skopos::probe::{LogQuery, ssh_probe};

fn main() -> vast_skopos::Result<()> {
    let client = Client::new(ApiKey::discover()?);
    println!("credit {:?}", client.account()?.credit);
    for i in client.instances()?.iter().filter(|i| i.is_running()) {
        println!("{} {:?} at {:?} $/h", i.id, i.gpu_name, i.dph_total);
        if let Some((host, port)) = i.ssh_endpoint() {
            // The GPU as the box itself reads it, not as the dashboard does.
            let probe = ssh_probe(host, port, None, Some(&LogQuery::new("/root/runs/*/train.log")))?;
            for g in &probe.gpus {
                println!("  GPU {}: {} %, {} MiB used", g.index, g.util_pct, g.mem_used_mib);
            }
            println!("  log: {:?}", probe.log);
        }
    }
    Ok(())
}
```

`render::table` and `render::money_line` print what `vas` prints, and `compact::json_lines` the same
readings as JSON Lines.

The offer search needs no key:

```rust,no_run
use vast_skopos::api::SearchClient;
use vast_skopos::offer::{OfferQuery, OfferSort, select};
use vast_skopos::render::{OfferColumns, offers_footer, offers_table};

fn main() -> vast_skopos::Result<()> {
    let mut q = OfferQuery::default(); // the identity: every offer rentable now
    q.gpu = Some("RTX 5090".to_owned());
    q.reliability = Some(0.99);
    q.exclude_countries = vec!["CN".to_owned()];
    let page = SearchClient::new().offers(&q, OfferSort::Price)?;
    // Every constraint was sent; `select` re-checks each row and sorts again.
    let kept = select(page.offers.clone(), &q, OfferSort::Price);
    let (top, now) = (20, 0.0); // `now`: Unix seconds, named in the footer
    // What `vas offers` prints: the table, then the footer (which says when vast.ai's 64-row cap
    // may have cut the list).
    print!("{}{}", offers_table(&kept, top, OfferColumns::Plain), offers_footer(&page, &kept, top, &q, OfferSort::Price, now));
    Ok(())
}
```

`market::MarketWatch` remembers the machines over the looks of a watch and says what moved
(`record`) and in how many looks each appeared (`seen`), and `render::offers_table_marked` prints the
table of a watch.

## Capabilities

| What | Where it is read | Notes |
|------|------------------|-------|
| Instances: status, GPU, `$/h`, age, label | vast.ai API, `GET /api/v1/instances/` (paginated: `limit`, `order_by`, then `after_token`) | read field by field: a field the API adds, drops or retypes costs a dash in one cell, not the table |
| Credit and burn rate | vast.ai API, `GET /api/v0/users/current/` | only `credit` and `balance` are read (the response also holds the key: never kept or printed) |
| `ssh` endpoint | the instance's `ssh_host` and `ssh_port` | the proxy rule of the official CLI: `ssh_port + 1` for Jupyter-launched instances, as the dashboard shows |
| GPU utilisation and VRAM | the box itself, `nvidia-smi` over the system `ssh` | the API's `gpu_util` is never shown: it has read 0 % on boxes running at 97-99 % |
| What the run last wrote | the box itself, the newest match of `--log` | at most 4096 bytes come back; both `ssh` pipes are drained while it runs |
| Cost so far | an estimate, `$/h` × age | the invoice is the reference |
| Offers for rent | vast.ai API, `GET /api/v0/bundles/?q=<JSON>`, sent **without a key** | at most 64 rows per search (said in the footer when reached); every constraint sent AND re-checked on each row; host-written text loses its control and invisible characters (bidirectional overrides, zero-width), and a cell keeps at most 40 characters |

The base URL is `https://console.vast.ai`, or `VAST_URL` when set. The endpoints and their
pagination follow the official `vastai` CLI's source. An instance can show "open" long after its
run ended, which is why the GPU reading comes from the box; the money line is there because a
rental is a budget, and how long the credit lasts at the current burn is the number to watch.

## Feature Flags

| Feature | Default | Description |
|---------|---------|-------------|
| `cli` | yes | Build the `vas` binary (pulls `clap`). Library-only consumers use `--no-default-features`. |

## Documentation

| Doc | |
|-----|---|
| [Reference](docs/reference.md) | Every flag of `vas` and `vas offers`, every column and mark (`POWER`, `OTHER`, `100%?`, `UNREACHABLE`...), the status and money lines, `--compact`'s JSON Lines, the offers' config file, footer and watch |
| [From filters to a running box](docs/tutorials/from-filters-to-a-running-box.md) | A tutorial: from a filters file to a running box, and watching it until you are done |
| [Waiting for a line](docs/tutorials/waiting-for-a-line.md) | A tutorial: `--grep` to see the line a run printed once, `--until` to wait for it in place of a loop of your own, and the exit codes a script reads |
| [FAQ](docs/FAQ.md) | The questions that came up on real rentals: the key and the files, why the offer list changes, which machine is which, verified or not, the money line, what `--ssh` reads, a box that is loading or unreachable, waiting for a line, a slow rental (power cap, someone else on the GPU), rate limits |
| [Roadmap](docs/roadmap-v0.5.0.md) | What 0.5.0 contains (two levels of help, `vas --skill`, a lighter README, the learning curve measured) and the plan it was built from |
| [Draft for 0.6.0](docs/roadmap-v0.6.0.md) | The candidates so far: what a box costs now, stopped boxes included (measured at a rental on 2026-11-08), and the items still open from earlier versions |
| [Roadmap of 0.4.0](docs/roadmap-v0.4.0.md) | What 0.4.0 contains and does not, the known limits (what is still to be seen live: the direct-route fallback, `100%?`, the `<575` mark), and the candidates for later (ease of use, `LIMIT W` as a warning, a history of the boxes) |
| [Roadmap of 0.3.0](docs/roadmap-v0.3.0.md) | What 0.3.0 contains and does not, the known limits (what is still to be seen live), and the candidates for later (the direct route, a reference board power, an idle counter told apart, ...) |
| [Roadmap of 0.2.0](docs/roadmap-v0.2.0.md) | What 0.2.0 contains and does not, the known limits, and the candidates for later (a text interface, a measured upload, ...) |
| [`vas` and `vastai`](docs/comparison-with-vastai.md) | Vast.ai's own CLI/SDK and `vas`, compared row by row: scope, read-only by design, the GPU read on the box, the runway, the offer search, key hygiene, redirects, the `ssh` port rule |
| [Dogfooding, 2026-10-16](https://github.com/mi-for-the-rust-of-us/vast-skopos/blob/main/docs/dogfooding/2026-10-16.md) | The first real rental (Me35, three RTX 5090s): what broke in the first minute, what helped, the next features, and `vas offers` probed and built for the next rental |
| [Dogfooding, 2026-10-17](https://github.com/mi-for-the-rust-of-us/vast-skopos/blob/main/docs/dogfooding/2026-10-17.md) | `vas offers`' first rental (Me36, five 5090s for three slots): what worked, seven notes for the next versions (offers that vanish, a burn line that can mislead, machine ids, a compact mode, labels, a two-size uplink measure for `vas check`, a credit history) |
| [Dogfooding, 2026-10-18](https://github.com/mi-for-the-rust-of-us/vast-skopos/blob/main/docs/dogfooding/2026-10-18.md) | `vas offers --watch` on the live market: boxes going away and arriving by the second, and the idea of a TUI for the watch |
| [Dogfooding, 2026-10-23](https://github.com/mi-for-the-rust-of-us/vast-skopos/blob/main/docs/dogfooding/2026-10-23.md) | Seven runs chained on one RTX 5090, then the hunts of 10-24 and 10-25: the cost of a 500 W cap, and the notes behind 0.3.0 (the status message, `UNREACHABLE` with the age, `--grep` and `--until`, `--wide` and `--host` on the offers, unverified is not deverified) |
| [Dogfooding, 2026-10-26](https://github.com/mi-for-the-rust-of-us/vast-skopos/blob/main/docs/dogfooding/2026-10-26.md) | Two RTX 5090s for Me41: a board limited to 450 W in its firmware that reads as uncapped, what 575, 500 and 450 W cost per step, an idle card reading 100 %, and the third check of 0.3.0 (the direct route found, and closed on one box) |
| [Dogfooding, 2026-10-28](https://github.com/mi-for-the-rust-of-us/vast-skopos/blob/main/docs/dogfooding/2026-10-28.md) | A test rental of two RTX 3080s, to see `vas` itself on real boxes: the status line while loading (and a box destroyed for a message that stayed), `SSH FAILED`, the `!` power line, `DIRECT`, a wait on one box with `--instance`, and a line-break bug found and fixed |
| [Dogfooding, 2026-11-08](https://github.com/mi-for-the-rust-of-us/vast-skopos/blob/main/docs/dogfooding/2026-11-08.md) | The learning curve measured: two fresh agents, one with only the help and one with only the skill, five tasks against a mock, and what they stumbled on |
| [Review of the code since 0.1.0, 2026-10-18](https://github.com/mi-for-the-rust-of-us/vast-skopos/blob/main/docs/audits/2026-10-18-review.md) | Three fresh readers, finding by finding: what was fixed, what was accepted, what is deferred |
| [CONVENTIONS](CONVENTIONS.md) | The rules every line follows, the API-key and box rules included |
| [CHANGELOG](CHANGELOG.md) | Release history |

## Used by

- **askesis** (private) — PCfVW's research on planning inside a model, whose models are trained on
  rented vast.ai GPUs. `vas` watched its Me35 rental (three RTX 5090s, six runs, 2026-10-16), and
  its dogfooding notes drove `0.1.0`'s fixes; `--log` naming the run was written for Me36's four
  sequential runs per box, and `vas offers` for renting Me36's boxes by askesis's filters.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT License](LICENSE-MIT) at your option.

vast.ai is a trademark of its owner, used here only to say what this tool works with.

## Development

- Exclusively developed with [Claude Code](https://claude.com/product/claude-code) (dev)
- Git workflow managed with [Fork](https://fork.dev/)
- All code follows [CONVENTIONS.md](CONVENTIONS.md), derived from [Amphigraphic-Strict](https://github.com/PCfVW/Amphigraphic-Strict)'s [Grit](https://github.com/PCfVW/Amphigraphic-Strict/tree/master/Grit) — a strict Rust subset designed to improve AI coding accuracy — and following [hf-fetch-model](https://github.com/mi-for-the-rust-of-us/hf-fetch-model)'s conventions section for section.
