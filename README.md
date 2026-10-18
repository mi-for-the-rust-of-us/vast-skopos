# vast-skopos

[![CI](https://github.com/mi-for-the-rust-of-us/vast-skopos/actions/workflows/ci.yml/badge.svg)](https://github.com/mi-for-the-rust-of-us/vast-skopos/actions/workflows/ci.yml)
[![MSRV](https://img.shields.io/badge/MSRV-1.88-blue.svg)](https://www.rust-lang.org)
[![license](https://img.shields.io/badge/license-MIT_OR_Apache--2.0-blue.svg)](#license)
[![unsafe: forbid](https://img.shields.io/badge/unsafe-forbid-success.svg)](https://github.com/rust-secure-code/safety-dance/)
[![API: read-only](https://img.shields.io/badge/vast.ai_API-read--only_(GET)-informational.svg)](#why-read-only)

**σκοπός** — *Your rented vast.ai boxes, watched.* **Not affiliated with vast.ai:** an independent,
read-only tool that reads the vast.ai API with your own key, and your boxes over your own `ssh`.

> 🆕 **`0.1.0` watched its first rental, then learned to name the run it reads.** `vas` watched askesis's Me35 rental (three RTX 5090s, six runs, 2026-10-16); its first minutes fixed what that rental found: the account endpoint's `301` (no redirect is ever followed, so the canonical `/api/v0/users/current/` is requested) and the `ssh` proxy port of Jupyter-launched boxes (`ssh_port + 1`, the dashboard's own rule). For Me36's four sequential runs per box, `--log` now names the run it read (`me36_ab_s0_d1/train.log: …`) and says `(no file matches)`, `(not a file)`, `(empty)` or `?` where it once printed a bare `-`; a key saved by Notepad as `vast_api_key.txt` is named in the error. And before the next rental, **`vas offers`** lists the offers that pass your renting filters, keyless, with every constraint re-checked on each row and vast.ai's 64-row cap said aloud. See [`CHANGELOG.md`](CHANGELOG.md) and [`docs/dogfooding/2026-10-16.md`](https://github.com/mi-for-the-rust-of-us/vast-skopos/blob/main/docs/dogfooding/2026-10-16.md).

## Table of Contents

- [Install](#install)
- [Usage](#usage)
- [Try it](#try-it)
- [Binary (`vas`)](#binary-vas)
- [Capabilities](#capabilities)
- [Feature Flags](#feature-flags)
- [Documentation](#documentation)
- [Used by](#used-by)
- [License](#license)
- [Development](#development)

> **New to vast-skopos?**
> - **What are my boxes doing, and costing, right now?** → [`vas`](#binary-vas) — one table: status, `$/h`, age, cost so far, `ssh` endpoint, and the money line.
> - **Is the GPU really busy?** (the dashboard has read 0 % on boxes at 97-99 %) → [`vas --ssh`](#binary-vas) — `nvidia-smi` read on each running box, over your own `ssh`.
> - **Which run is each box on, and what did it last write?** → [`vas --ssh --log '/root/runs/*/train.log'`](#binary-vas) — the newest matching file, named, and its last non-blank line.
> - **How long will my credit last?** → the money line: `credit | burn $/h over N running | ~H h left at this rate`.
> - **A long rental to watch?** → `vas --ssh --log … --watch 120` — the table every two minutes, until Ctrl+C.
> - **Which boxes could I rent that pass my filters?** → [`vas offers`](#vas-offers) — GPU, CUDA, disk, reliability, upload, countries and hosts left out, cheapest first; no key needed, and the filters can live in a config file.
> - **`no vast.ai API key found`?** → [The API key](#the-api-key) — where `vas` looks, and why there is no `--key` flag.
>
> Every flag is in `vas --help`; how `vas` differs from Vast.ai's own CLI is in [`docs/comparison-with-vastai.md`](docs/comparison-with-vastai.md).

## Install

```sh
cargo install vast-skopos            # installs the `vas` binary
```

From a clone: `cargo install --path . --locked`. As a library, without the CLI (no `clap`):

```toml
[dependencies]
vast-skopos = { version = "0.2", default-features = false }
```

## Usage

The lookout, as a table, in a few lines (the examples below are compiled by the tests, not run: they
need your key, your boxes or the network):

```rust,no_run
use vast_skopos::api::Client;
use vast_skopos::key::ApiKey;
use vast_skopos::probe::probe_all;
use vast_skopos::render::{TableOptions, table};

fn main() -> vast_skopos::Result<()> {
    // The key, by the official `vastai` CLI's search order; it is never printed.
    let client = Client::new(ApiKey::discover()?);
    let instances = client.instances()?;
    let account = client.account().ok();
    // One `ssh` attempt per running box, all at once: what the GPUs do, and the newest log's last line.
    let probes = probe_all(&instances, None, Some("/root/runs/*/train.log"));
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
use vast_skopos::probe::ssh_probe;

fn main() -> vast_skopos::Result<()> {
    let client = Client::new(ApiKey::discover()?);
    println!("credit {:?}", client.account()?.credit);
    for i in client.instances()?.iter().filter(|i| i.is_running()) {
        println!("{} {:?} at {:?} $/h", i.id, i.gpu_name, i.dph_total);
        if let Some((host, port)) = i.ssh_endpoint() {
            // The GPU as the box itself reads it, not as the dashboard does.
            let probe = ssh_probe(host, port, None, Some("/root/runs/*/train.log"))?;
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
use vast_skopos::render::{offers_footer, offers_table};

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
    print!("{}{}", offers_table(&kept, top), offers_footer(&page, &kept, top, &q, OfferSort::Price, now));
    Ok(())
}
```

`OfferChanges::between(previous, current)` says what moved between two looks, and
`render::offers_table_marked` prints the table of a watch.

## Try it

With your key where the `vastai` CLI keeps it ([The API key](#the-api-key)):

```text
$ vas --ssh --log '/root/runs/*/train.log'
ID        LABEL  GPU          WHERE  STATUS   $/H    AGE    COST~  SSH                 GPU% (ssh)  VRAM       LAST LOG / SSH ERROR
56053340  -      1x RTX 5090  US     running  0.643  2h 0m  1.29   ssh4.vast.ai:23484  98%         29.4/32G   s0/train.log: [ 796.4s] layer 4: ...
56053638  -      1x RTX 5090  US     exited   0.608  10m    0.10   ssh4.vast.ai:37038  -           -          -
credit $5.94 | burn $0.643/h over 1 running | ~9.2 h left at this rate
```

And with no key at all, the offers askesis rents by (captured 2026-10-16, 20:34, before the footer named the time of the search):

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

## Binary (`vas`)

| | |
|---|---|
| `vas` | the table and the money line |
| `vas --ssh` | also probe every running box over `ssh`, once each, in parallel |
| `vas --ssh --log PATH_OR_GLOB` | also print, for the newest match of the pattern on each box, its last two path components and its last non-blank line (`s0/train.log: ...`, so a glob whose varying part is among the last two components says which run wrote it); `(empty)` when the file has no line to show, `(not a file)` when the newest match is a directory, `(no file matches)` when nothing matched, `?` when the box's answer was cut short. The pattern is expanded by the box's shell, so quote it locally (and quote a part with spaces again inside, `'"/root/my runs"/*/train.log'`); the line is shown without control characters, at most 120 characters, the path at most 60 |
| `vas --ssh -i ~/.ssh/id_ed25519` | use this private key (default: `ssh`'s own choice, agent and `~/.ssh/config`) |
| `vas --wide` | also show each box's machine id and host id (what a rentals log, `--exclude-machine` and `--exclude-host` need) |
| `vas --compact` | JSON Lines instead of the table, for scripts and monitors ([below](#vas---compact-json-lines)); not with `--wide` |
| `vas --watch 60` | refresh every 60 s (at least 30) until Ctrl+C |
| `vas --key-file FILE` | read the key from `FILE` |
| `vas offers [FLAGS]` | the offers for rent that pass the flags, cheapest first; no key needed ([below](#vas-offers)) |

`--ssh` uses your system `ssh` with `BatchMode=yes`, `ConnectTimeout=10` and keep-alives, gives
each probe 30 s in all (and `nvidia-smi` 20 s on the box), and makes **one attempt** per box per
refresh: a box that refuses is reported in its row (with `ssh`'s own last error line), never
retried in a loop. Host keys are accepted on first use (`StrictHostKeyChecking=accept-new`), since
vast.ai's `ssh` proxies are reached by port.

`COST~` is an estimate: today's `$/h` times the hours since creation. vast.ai bills storage while an
instance is stopped and prices can change; the invoice is the reference.

**The burn counts what may bill.** The money line's burn adds the boxes the API reads as `running` AND
the boxes it was asked to run (`intended_status`) that read otherwise (`offline`, `loading`...):
`burn $1.940/h over 3 running + 1 offline (meant to run, billing unknown)`. Whether such a box bills
is not known from here, and a burn that leaves it out overstates the runway; the `~H h left` figure
uses the larger burn. A stopped box is not counted, though vast.ai still bills its storage: `vas`
does not show that cost yet.

### `vas --compact`: JSON Lines

One object per box, then one for the money, each on one line, each with a `kind` (`box` or `money`) and
`at` (Unix seconds, the same on every line of a look), so a line read alone says what it is and when.
The keys of an object never change: what is not known is `null`, not a missing key. Keys come out in alphabetical order, as `serde_json` writes them by default (`kind` is not first: filter on it with
`jq` or `grep '"kind":"money"'`). Money is rounded to ten-thousandths of a dollar (`credit`, `burn`, `cost`):
about two seconds of a $0.19/h box, and vast.ai sends the credit with float noise (`12.059765171599793`).

```text
$ vas --compact --ssh --log '/root/runs/*/train.log'
{"age_s":8100,"at":1792250000,"cost":2.025,"dph":0.9,"gpu_name":"RTX 5090","host_id":406325,"id":56362434,"intended":"running","kind":"box","label":"me36 box 2","machine_id":100682,"num_gpus":1,"probe":{"gpus":[{"index":0,"mem_total_mib":32607,"mem_used_mib":29400,"util_pct":99}],"log":{"file":"/root/runs/s0/train.log","line":"e41 loss 0.3","state":"line"}},"ssh_host":"ssh6.vast.ai","ssh_port":36000,"status":"running","where":"South Korea, KR"}
{"at":1792250000,"burn":0.9,"credit":12.93,"hours_left":14.4,"kind":"money","running":1,"unsure":{}}
```

`probe` is `null` for a box not probed (no `--ssh`, or not running), `{"error": "..."}` when the probe failed,
else `gpus` and `log`; `log` is `null` without `--log`, else an object whose `state` is `line`, `empty`,
`not_a_file`, `no_match` or `unreadable`. `unsure` counts, by state, the boxes meant to run that read
otherwise (the burn, above). With `--watch N`, a look is printed every N seconds with no separator line: the `at`
tells looks apart. `jq` does the rest, for example `vas --compact | jq -r 'select(.kind=="money").hours_left'`.

### `vas offers`

| Flag | |
|---|---|
| `--gpu MODEL` | the GPU model as vast.ai names it: `RTX_5090` or `"RTX 5090"` (an underscore reads as a space) |
| `--num-gpus N` | the number of GPUs |
| `--disk GB` | the disk wanted: priced into `$/h` and required free (unset, 5 GB is priced in: the `vastai` CLI's default) |
| `--cuda VERSION` | the lowest CUDA version the host's driver must support (`12.8`) |
| `--reliability FRACTION` | the lowest reliability, from 0 to 1 (`0.99`; `99` is refused) |
| `--inet-up MBIT_S`, `--inet-down MBIT_S` | the lowest ADVERTISED bandwidth (the host's own test, not your route: see the dogfooding note) |
| `--max-price DOLLARS`, `--min-days DAYS` | the highest `$/h` (storage included), the shortest acceptable maximum rental |
| `--verified`, `--no-verified` | only machines vast.ai has verified; `--no-verified` cancels a config file's `--verified` |
| `--exclude-country CC`, `--exclude-host ID`, `--exclude-machine ID` | leave out a country (two letters), a host or a machine (a host may own several; `vas --wide` shows yours); repeatable |
| `--top N` (20), `--sort price\|reliability\|up` | how many rows, in which order |
| `--watch SECONDS`, `--changes` | search again every N seconds (at least 30) until Ctrl+C, each look the whole table marked with what moved; `--changes` prints only what moved ([below](#watching-the-offers)) |
| `--config FILE`, `--no-config` | read the flags of `FILE` first, or of no file |

**The config file** holds the same flags, one per line (`#` lines are comments, one pair of quotes
around a value is removed), and is read FIRST: the one `--config` names, else
`$XDG_CONFIG_HOME/vast-skopos/offers`, else `~/.config/vast-skopos/offers`. On the command line, a single value
replaces the file's, and a list adds to it (`--exclude-country RU` keeps the file's `CN`). A missing
default file is no file; a missing `--config` file, an unreadable file or a line that is not a flag
stops the run, since going on would silently drop the exclusions it holds. With no constraint at
all, `vas offers` lists every offer rentable now, and says so on stderr.

```text
# ~/.config/vast-skopos/offers: askesis's renting filters
--gpu RTX_5090
--num-gpus 1
--disk 50
--reliability 0.99
--min-days 2
# driver >= 580, which vast.ai shows as CUDA 13
--cuda 13
--inet-up 200
--exclude-country CN
--exclude-host 362355
--verified
```

**What the footer says.** vast.ai answers at most **64 rows** per search, whatever it is asked, and
still says `truncated: false`; its order decides which 64 come back, so `--sort` is sent to the
server too. When 64 rows come back, the footer says the list may be cut: narrow the search. Every
constraint is also **re-checked on each row**: a row whose field contradicts one is dropped (and
counted in the footer); a row MISSING the field is kept, with a dash. The footer also names every
constraint applied (a config file never acts unseen), the storage priced into `$/h` and the time of
the search (UTC): the list is a snapshot of a market where offers vanish within minutes.

#### Watching the offers

Offers vanish within minutes, so `vas offers --watch 60` searches again every minute (at least 30 s) until
Ctrl+C. Each look is the whole table and its footer, with the new offers marked `+` in a first column, a
price that moved read `0.594 (was 0.575)`, and under the footer what the table cannot show:

```text
since the last look: 1 new, 1 repriced, 1 gone
- 46579456  1x RTX 5090  0.554 $/h  California, US
```

`--changes` prints only what moved, one line each, for a log or a monitor: a first line for the baseline,
then one line per look (`no change`, or the counts followed by a line per offer: `+` new, `~` repriced,
`-` gone), none left out. Under the table, at most ten gone offers are listed, the rest counted.

A few details keep the notes honest:

- "Gone" is measured against the whole list the search kept, not the rows shown, so an offer that sank
  below the last row is not gone. With `--top 3`, a new offer that is not among the three rows has no `+`:
  the note counts it (`1 new (1 below the table)`).
- vast.ai answers at most 64 rows per search: when it does, a change in who entered or left may only be an
  offer crossing that line, and the note says so.
- A search that fails ends a watch whose FIRST look failed (a wrong `VAST_URL` or key would otherwise loop on
  stderr forever); after one look has succeeded, a failure is only said on stderr, and the next look reads
  `since the last successful look`. The lookout's `vas --watch` does the same.
- A `--watch` under 30 s is raised to 30 s, and `vas` says so on stderr.
- A config file cannot hold `--watch` or `--changes`: a file of filters must not turn a run into a loop.

### The API key

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

### `vas` and `vastai`

`vastai`, Vast.ai's own CLI and SDK, rents and manages boxes; `vas` only watches them, read-only.
They are complements: [`docs/comparison-with-vastai.md`](docs/comparison-with-vastai.md) compares
them row by row.

#### Why read-only?

`vas` only ever issues `GET` requests: it never creates, starts, stops, labels or destroys an
instance. `vas offers` searches, and rents nothing: to rent the offer you chose, take its `ID` to the
dashboard or to `vastai create instance`. A monitor that can spend or destroy is a different tool, with a different review; what to
do about a box (stop it, destroy it, top up the credit) stays a person's decision, taken in the
dashboard or with `vastai`.

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
| [`vas` and `vastai`](docs/comparison-with-vastai.md) | Vast.ai's own CLI/SDK and `vas`, compared row by row: scope, read-only by design, the GPU read on the box, the runway, the offer search, key hygiene, redirects, the `ssh` port rule |
| [Dogfooding, 2026-10-16](https://github.com/mi-for-the-rust-of-us/vast-skopos/blob/main/docs/dogfooding/2026-10-16.md) | The first real rental (Me35, three RTX 5090s): what broke in the first minute, what helped, the next features, and `vas offers` probed and built for the next rental |
| [Dogfooding, 2026-10-17](https://github.com/mi-for-the-rust-of-us/vast-skopos/blob/main/docs/dogfooding/2026-10-17.md) | `vas offers`' first rental (Me36, five 5090s for three slots): what worked, seven notes for the next versions (offers that vanish, a burn line that can mislead, machine ids, a compact mode, labels, a two-size uplink measure for `vas check`, a credit history) |
| [Dogfooding, 2026-10-18](https://github.com/mi-for-the-rust-of-us/vast-skopos/blob/main/docs/dogfooding/2026-10-18.md) | `vas offers --watch` on the live market: boxes going away and arriving by the second, and the idea of a TUI for the watch |
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
