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

> 🆕 **`0.2.0` adds what a dashboard does not show.** On two rented RTX 5090s, `vas --ssh` found a power limit of 400 W against the card's 575 W default (the SM clock held at about 1,700 of 3,090 MHz while the card read 100 %), and, on one of them, about 16 GiB of GPU memory and about half of the GPU's compute time used by something outside the container (what, and why, was not determined). It now shows `POWER`, `CLOCK` and `OTHER` columns and, in a watch, says when they change; `--credit-log` keeps the credit as a curve; `vas offers --watch` reads machines over several looks (vast.ai's search answers a flickering sample) with a `SEEN` column; `--compact` prints JSON Lines; and vast.ai's rate limits are read and obeyed. See [`CHANGELOG.md`](CHANGELOG.md) and [`docs/dogfooding/2026-10-18.md`](https://github.com/mi-for-the-rust-of-us/vast-skopos/blob/main/docs/dogfooding/2026-10-18.md).

> 🚀 **`0.1.0` watched its first rental, then learned to name the run it reads.** `vas` watched askesis's Me35 rental (three RTX 5090s, six runs, 2026-10-16); its first minutes fixed what that rental found: the account endpoint's `301` (no redirect is ever followed, so the canonical `/api/v0/users/current/` is requested) and the `ssh` proxy port of Jupyter-launched boxes (`ssh_port + 1`, the dashboard's own rule). For Me36's four sequential runs per box, `--log` now names the run it read (`me36_ab_s0_d1/train.log: …`) and says `(no file matches)`, `(not a file)`, `(empty)` or `?` where it once printed a bare `-`; a key saved by Notepad as `vast_api_key.txt` is named in the error. And before the next rental, **`vas offers`** lists the offers that pass your renting filters, keyless, with every constraint re-checked on each row and vast.ai's 64-row cap said aloud. See [`CHANGELOG.md`](CHANGELOG.md) and [`docs/dogfooding/2026-10-16.md`](https://github.com/mi-for-the-rust-of-us/vast-skopos/blob/main/docs/dogfooding/2026-10-16.md).

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
> - **The GPU reads 100 % but the run is slow?** → the `POWER`, `CLOCK` and `OTHER` columns of `vas --ssh` — [the FAQ](docs/FAQ.md#when-a-rental-is-slow) says what they mean.
> - **First rental?** → [From filters to a running box](docs/tutorials/from-filters-to-a-running-box.md), a walk through one rental, step by step.
>
> Every flag is in `vas --help`; how `vas` differs from Vast.ai's own CLI is in [`docs/comparison-with-vastai.md`](docs/comparison-with-vastai.md).

## Install

```sh
cargo install vast-skopos --locked   # installs the `vas` binary
```

`--locked` uses the dependency versions the crate was tested with (without it, `cargo install` resolves
the newest compatible ones). From a clone: `cargo install --path . --locked`. As a library, without the CLI (no `clap`):

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

## Try it

With your key where the `vastai` CLI keeps it ([The API key](#the-api-key)):

```text
$ vas --ssh
ID        LABEL  GPU          WHERE              STATUS   $/H    AGE     COST~  SSH                 GPU% (ssh)  VRAM      OTHER  POWER               CLOCK      LAST LOG / SSH ERROR
56533728  -      1x RTX 5090  United States, US  running  0.491  4h 26m  2.18   ssh5.vast.ai:24016  100%        24.2/32G  15.7G  400/575W+power_cap  1717/3090  -
56558627  -      1x RTX 5090  Texas, US          running  0.704  51m     0.61   ssh2.vast.ai:20294  99%         8.6/32G   -      400/575W+power_cap  2100/3090  -
credit $9.33 | burn $1.195/h over 2 running | ~7.8 h left at this rate
```

(Two rented RTX 5090s, 2026-10-18, one look of a watch; the `OTHER` of the first box is its reading minutes
later. Both cards are capped by their hosts at 400 W against a 575 W default, and the first holds 15.7 GiB
that no process of its own accounts for: see [the power cap](#the-power-cap-and-the-clock) and
[someone else on the GPU](#someone-else-on-the-gpu) below. `--log PATH_OR_GLOB` fills the last column.)

And with no key at all, the offers the author rents by (captured 2026-10-16: a search today also names its time and ends with the line saying that vast.ai's answers vary):

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
| `vas --ssh --log PATH_OR_GLOB --grep PATTERN` | the last line MATCHING `PATTERN` (an extended regular expression, `grep -E`, case-sensitive) instead of the last line: `--grep 'initial loss\|Traceback'` catches a line printed once and buried by the lines after it. Searched in the file's last 64 MiB, each carriage-return segment of a progress bar a line of its own; `(no line matches)` when none does, `(grep refused the pattern)` when `grep` cannot read it. The pattern is quoted for the box's shell: nothing in it is expanded |
| `vas --ssh -i ~/.ssh/id_ed25519` | use this private key (default: `ssh`'s own choice, agent and `~/.ssh/config`) |
| `vas --wide` | also show each box's machine id and host id (what a rentals log, `--exclude-machine` and `--exclude-host` need), then its CPU, driver, DLPerf and PCIe bandwidth (`CPU`, `DRIVER`, `DLPERF`, `PCIE GB/s`: what tells it from another machine of its host, as `vas offers --wide` shows them) |
| `vas --compact` | JSON Lines instead of the table, for scripts and monitors ([below](#vas---compact-json-lines)); not with `--wide` |
| `vas --watch 60` | refresh every 60 s (at least 10) until Ctrl+C |
| `vas --credit-log FILE` | append one CSV row per look to `FILE`: the credit as a curve ([below](#the-credit-history)) |
| `vas --key-file FILE` | read the key from `FILE` |
| `vas offers [FLAGS]` | the offers for rent that pass the flags, cheapest first; no key needed ([below](#vas-offers)) |

`--ssh` uses your system `ssh` with `BatchMode=yes`, `ConnectTimeout=10` and keep-alives, gives
each probe 30 s in all (and `nvidia-smi` 20 s on the box), and makes **one attempt** per box per
refresh: a box that refuses is reported in its row, never retried in a loop. The row says what
happened, how old the box is, and `ssh`'s own last error line: `UNREACHABLE 4m: ssh: connect to host
ssh3.vast.ai port 32456: Connection refused` when `ssh` never reached the box (refused, timed out, no route,
a name that does not resolve, closed before the session, no answer within 30 s), `SSH FAILED 4m: ...` when it
did and was turned away (a refused key, a changed host key, no GPU reading). The age is what the choice
between waiting and destroying rests on: three rentals (2026-10-17, 10-18, 10-23) met boxes that read
`running` at 4 minutes and refused every connection, and were destroyed unused. Host keys are accepted on first use (`StrictHostKeyChecking=accept-new`: trust on first use, a changed key
is refused), since vast.ai's `ssh` proxies are reached by port. Each probe has an `ssh` session of its own
(`ControlMaster=no`, `ControlPath=none`), whatever `~/.ssh/config` says about shared connections.

`COST~` is an estimate: today's `$/h` times the hours since creation. vast.ai bills storage while an
instance is stopped and prices can change; the invoice is the reference.

**A box that is not running yet** says why in vast.ai's own words, on a line under the table (before the
money line): `57340012 loading: 46b4edaa8d47: Pull complete`, the message cut at 120 characters. On
2026-10-24 two boxes read `loading -> running` at 2 minutes of age while that message named the image's
layers arriving: progress, not a stuck box. A running box's message is not shown; `--compact` carries the
message of every box as `status_msg`.

**The probe columns.** Under `--ssh`, in order: `GPU% (ssh)`, `VRAM`, `OTHER`, `POWER`, `CLOCK`, then, under
`--watch`, `TREND` and `CLOCK TREND`, and `LAST LOG / SSH ERROR` last; each is explained below.

**The GPU trend.** One `nvidia-smi` sample is not a diagnosis (it has read 100 % with 0 MiB used while
nothing held the card). Under `vas --ssh --watch N`, the table gains a `TREND` column after `CLOCK`: the
last six samples of each box, oldest first, as the mean utilisation of its GPUs (`97 99 0 98 99 99`),
kept in memory for the length of the watch; `--compact` carries the same numbers as `probe.util_trend`. A
probe that failed adds nothing and keeps the history (one dropped `ssh` is not the run stopping), and a box
that stops or leaves is forgotten. A single look has no history, and no such column.

<a id="the-power-cap-and-the-clock"></a>

**The power cap.** A card's power limit can be set below what it ships with. Seen on 2026-10-18 on two
rented RTX 5090s: a limit of 400 W against the 575 W default, `SW Power Cap: Active`, and the SM clock held
at about 1,700 MHz of a possible 3,090: the run goes slower than the model suggests, and the dashboard's
utilisation (100 %) does not show it. Under `--ssh`, the `POWER` column (after `OTHER`) says the limit, the
default when the limit is lower, and what holds the clocks down: `400/575W+power_cap`; `575W` is a card at
its default. The words are `power_cap`, `thermal`, `slowdown` and `power_brake`; an idle GPU, application
clocks and sync boost are not restraints and are not shown.

**What `+power_cap` does and does not say.** It says the card is at its power limit, which **every card
reaches under full load**: on 2026-10-19 a default-limit RTX 4060 Ti (`165W`) read `165W+power_cap` and about
2,565 of 3,105 MHz as soon as a matrix-multiply loop ran on it, and went back to `165W` when the loop ended.
That is normal. What marked the 2026-10-18 boxes as unusual was the limit **below the default**
(`400/575W`) together with a clock far under the maximum: compare the two numbers and the clock, not the
word alone. `--compact` carries, per GPU,
`power_w` (the draw now), `power_limit_w`, `power_default_w`, `sm_mhz`, `sm_max_mhz` and `throttle` (a list
of those words: empty when nothing holds the card, `null` when the box did not say). The power is asked in a
second `nvidia-smi` query, so a driver that lacks a field costs the power cells (`-`) and nothing else; the
reasons that hold the clocks down are a third, asked as `clocks_event_reasons` and, failing that, as the older
`clocks_throttle_reasons`, so a driver that knows only one name still gives the power and the clock.
A GPU the main query cannot read whole (one `[N/A]` among its fields, as with MIG slices) is left out of the
table, not shown half-read.

<a id="someone-else-on-the-gpu"></a>

**Someone else on the GPU.** On 2026-10-18 a rented RTX 5090 held 24,809 MiB while its only visible process, the
training, held 8,720: about 16 GiB belonged to something outside the container (what, and why, was not determined:
another user of the card, or the host), which also took roughly half of the GPU's time (ten one-second `nvidia-smi pmon` samples gave the training 24-82 % of
the SM, while the card read 100 %). Its twin on another machine, same workload, added up to the MiB. Under
`--ssh`, the `OTHER` column (after `VRAM`) says that memory, per GPU (`15.7G`), or `-` when nothing notable is
held (the driver's own few MiB are not shown: the threshold is 1 GiB) or the box did not say. It is the
device's memory in use minus what the compute processes the box lists hold, so nothing is concluded unless
that list was read whole: a figure that is not a number, a missing bus id, or NO process listed on the GPU
gives `-` (a run that has just restarted, or a container that cannot see its own processes, lists none while
the memory is still its own; the gap is then unknown, not "all someone else's"). Under `--watch`, a
line says when it appears or goes: `! 56533728 gpu 0: memory held outside the container 0.0 -> 15.7 GiB`.
`--compact` carries the raw gap as `other_mem_mib` per GPU (`null` when unknown) and the change as an `event`
object (`other_mib_was`, `other_mib_now`). `vas` cannot name the process (it is outside the box): report the
machine to vast.ai with the figures (`vas offers --exclude-machine ID` keeps it out of the next search).

The clock itself is the `CLOCK` column (`1702/3090`: the SM clock now over the highest the card reaches, in
MHz), and under `--watch` a `CLOCK TREND` column after `TREND` keeps its last six samples, the mean of the box's
GPUs: a card that fell from 3000 to 1700 MHz reads `3000 2900 1700 1700 1700 1700`, while its utilisation
still reads `100`. `--compact` carries them as `probe.clock_trend` (`null` in a one-shot).

A card that reaches its limit and leaves it (a load that starts and stops, a cap that is intermittent) makes a
watch say `power_cap began` and `power_cap ended` at every look it flips: that is what the card did, and
the line is information, not an accusation (see above).

A cap can also arrive mid-run. Under `vas --ssh --watch N`, a line under the money says when a GPU's limit,
its throttle reasons or its other users change from one look to the next: `! 56533728 gpu 0: power limit 575 -> 400 W, power_cap
began` (and `... ended`). The first look of a box is the baseline, a reading the box did not give changes
nothing, and a box that stops starts afresh. `--compact` prints each change as an object before the money
one: `{"kind":"event","at":...,"id":...,"gpu":0,"limit_was":575,"limit_now":400,"began":["power_cap"],"ended":[]}`.

**vast.ai rate-limits its API**, per endpoint, and says so in `X-RateLimit-*` headers (on 2026-10-18: 30
calls for the offer search, ONE for the instances list). `vas` reads them. A look refused with a 429 that
clears within 15 s waits it out once and says so (`vas: rate-limited: waiting 3 s, then trying again`), so
two runs in a row do not fail; a longer wait is named and ends the run (`vast.ai rate-limited
/api/v1/instances/: try again in 120 s`); and a `--watch` whose budget is spent leaves at least until the
reset between two looks, whatever N says (`vast.ai's request budget renews in 5 s: the next look waits for
it`). The library only reports (`api::RateLimit`, `Client::budget_wait_secs`, `SkoposError::RateLimited`)
and never waits by itself.

**The burn counts what may bill.** The money line's burn adds the boxes the API reads as `running` AND
the boxes it was asked to run (`intended_status`) that read otherwise (`offline`, `loading`...):
`burn $1.940/h over 3 running + 1 offline (meant to run, billing unknown)`. Whether such a box bills
is not known from here, and a burn that leaves it out overstates the runway; the `~H h left` figure
uses the larger burn. A stopped box is not counted, though vast.ai still bills its storage: `vas`
does not show that cost yet.

<a id="the-credit-history"></a>

**The credit history.** `vas --credit-log credit.csv` appends one row per look to the file, so a watch (or a
cron job running `vas` every hour) leaves a curve of the account's money behind:

```text
unix_time,credit,burn_per_hour,running,unsure
1792328430,10.5100,0.4910,1,0
1792328460,10.5000,0.4910,1,0
```

The header is written only when the file is new or empty; the file is only ever appended to, so a log
survives restarts, and `credit` is empty when the account did not answer. `unsure` counts the boxes meant to
run that read otherwise, and the burn counts them, so compare the fall of the credit with `burn_per_hour`: a
fall well below it, while `unsure` is above zero, says those boxes do not bill; a fall above it says
something else does (vast.ai bills the storage of a stopped box, which `vas` does not count). A look whose
instances request failed writes no row. Nothing is
written unless you name the file, the API key is never in it, and a file that cannot be written is said
once on stderr (`failed to write the credit log ...`) while the look goes on and the log stops trying for
the rest of the run (on Windows, a file open in Excel is such a file: close it, then restart the watch).

### `vas --compact`: JSON Lines

One object per box, then (under `--ssh --watch`) one per change of a GPU's power, then one for the money,
each on one line, each with a `kind` (`box`, `event` or `money`) and `at` (Unix seconds, the same on every
line of a look), so a line read alone says what it is and when; the money object closes a look. The
top-level keys of each `kind` never change: what is not known is `null`, not a missing key (`probe` is
described below: it is `null`, an error object, or the readings). Keys come out in alphabetical order, as `serde_json` writes them by default (`kind` is not first: filter on it with
`jq` or `grep '"kind":"money"'`). Money is rounded to ten-thousandths of a dollar (`credit`, `burn`, `cost`, `dph`):
about two seconds of a $0.19/h box, and vast.ai sends the credit with float noise (`12.059765171599793`).

```text
$ vas --compact --ssh --log '/root/runs/*/train.log'
{"age_s":8100,"at":1792250000,"cost":2.025,"cpu_name":"AMD EPYC 7K62 48-Core Processor","dlperf":199.2,"dph":0.9,"driver_version":"580.142","gpu_name":"RTX 5090","host_id":406325,"id":56362434,"intended":"running","kind":"box","label":"me36 box 2","machine_id":100682,"num_gpus":1,"pcie_bw":26.9,"probe":{"clock_trend":null,"gpus":[{"index":0,"mem_total_mib":32607,"mem_used_mib":29400,"other_mem_mib":10,"power_default_w":575,"power_limit_w":575,"power_w":560,"sm_max_mhz":3090,"sm_mhz":2900,"throttle":[],"util_pct":99}],"log":{"file":"/root/runs/s0/train.log","line":"e41 loss 0.3","state":"line"},"util_trend":null},"ssh_host":"ssh6.vast.ai","ssh_port":36000,"status":"running","status_msg":null,"where":"South Korea, KR"}
{"at":1792250000,"burn":0.9,"credit":12.93,"hours_left":14.4,"kind":"money","running":1,"unsure":{}}
```

`probe` is `null` for a box not probed (no `--ssh`, or not running), `{"error": "...", "error_kind": "unreachable"}` when the
probe failed (`error_kind` is `unreachable` when `ssh` never reached the box, else `failed`),
else `gpus` (per GPU: `util_pct`, `mem_used_mib`, `mem_total_mib`, `other_mem_mib`, `power_w`,
`power_limit_w`, `power_default_w`, `sm_mhz`, `sm_max_mhz` and `throttle`, each `null` when the box did not
say), `log`, `util_trend` and `clock_trend` (`null` in a one-shot, else the last GPU samples of a `--watch`,
see above); `log` is `null` without `--log`, else an object whose `state` is `line`, `empty`,
`not_a_file`, `no_match` or `unreadable`, and under `--grep` also `no_matching_line` or `pattern_refused`
(each with its `file`). `status_msg` is vast.ai's own message on the box, as the API
gives it whatever the status (`null` when it gives none); `cpu_name`, `driver_version`, `dlperf` and
`pcie_bw` are always there, like the ids (what `--wide` shows). `unsure` counts, by state, the boxes meant to run that read
otherwise (the burn, above). With `--ssh --watch N`, a change of a GPU's power limit or throttle reasons is an
`event` object, between the boxes and the money (see the power cap, above). With `--watch N`, a look is printed every N seconds with no separator line (the table form of `--watch`
stamps each look with `== vas, unix time N`; JSON lines would not parse with it): the `at`
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
| `--exclude-deverified`, `--no-exclude-deverified` | leave out the machines vast.ai has DEVERIFIED (verified once, then found failing its checks) and keep the unverified ones (new, or under evaluation), which `--verified` hides too ([below](#verified-unverified-deverified)); the second flag cancels a config file's first |
| `--host ID`, `--machine ID` | only this host or this machine: what a host rented from before offers now, or whether the machine you had is for rent again (`vas --wide` shows your boxes' ids); repeatable, and an `--exclude-*` of the same id still leaves it out |
| `--exclude-country CC`, `--exclude-host ID`, `--exclude-machine ID` | leave out a country (two letters), a host or a machine (a host may own several; `vas --wide` shows yours); repeatable |
| `--top N` (20), `--sort price\|reliability\|up` | how many rows, in which order |
| `--wide` | also show what tells one machine from another: `MACHINE` after `HOST`, then `CPU`, `DRIVER`, `DLPERF` (vast.ai's benchmark score), `DLPERF/$` (per `$/h` shown, storage included), `PCIE GB/s` and `MAX W` (the card's maximum power, NOT the limit a host sets: only `vas --ssh`'s `POWER` reads that) ([below](#is-this-the-machine-we-had)) |
| `--watch SECONDS`, `--changes` | search again every N seconds (at least 20) until Ctrl+C, each look the whole table marked with what moved; `--changes` prints only what moved ([below](#watching-the-offers)) |
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

<a id="is-this-the-machine-we-had"></a>

**Is this the machine we had?** One host can own several machines, and a search shows the host, not the
machine. `vas offers --wide` adds what tells them apart (the machine id, the CPU, the driver), and
`vas --wide` shows the same for a rented box, so the two can be set side by side; `--host ID` or
`--machine ID` narrows a search to one of them. DLPerf is shown as a number and nothing more: on
2026-10-25 two South Korean RTX 5090s read DLPerf 199.2, the same as an uncapped card, and were both capped
by their host at 500 of 600 W. Only the card's own reading (`vas --ssh`, `POWER`) shows a cap.

<a id="verified-unverified-deverified"></a>

**Verified, unverified, deverified.** vast.ai's documentation defines three stages: *verified* machines
passed its automated checks (reliability, network, operational health, performance) and are monitored;
*unverified* ones are new or under evaluation, "not a judgment of quality"; *deverified* ones were verified
and no longer meet the requirements. None of the three checks speed or a power cap. `--verified` hides the
last two; `--exclude-deverified` hides only the deverified, so a new machine that passes your own checks
(`vas --ssh`'s `POWER`, a speed check) can still be rented. The search does not filter on the `verification`
field it shows (probed 2026-10-25: `neq`, `in` and `notin` all came back with deverified rows), so `vas`
sends `vericode <= 1` (0 is unverified, 1 verified, 2 and up deverified), and re-checks `verification` on
each row as usual.

#### Watching the offers

**vast.ai's answers vary.** Measured on 2026-10-18: fifteen identical searches, 2.5 s apart, saw 129
machines (64 per answer), and only 30 of them in every answer; the others came and went, and one machine
could answer under two or three offer ids. A single search is a sample of the market, not the market:
`vas offers` ends with a line saying so (`repeat the search, or use --watch, to see more`; a watch says it once,
`--changes` not at all). Comparing one
answer with the next, offer by offer, would report that sampling as a market in turmoil, so a watch
reads the machines over several looks instead.

`vas offers --watch 60` searches again every minute (at least 20 s) until Ctrl+C. Each look is the whole
table and its footer. A machine is its machine id and its GPU count, not its offer id, and the table gains
a `SEEN` column, before `WHERE`: in how many looks each machine appeared, out of the looks since it was
first seen (`15/15` is a dependable listing, `3/9` one that flickers; why it flickers is not known).

- The first three looks only **learn** the market; nothing is marked or reported before the fourth.
- After that, `+` marks a machine that was absent from the previous three looks (or never seen), a price
  that moved reads `0.594 (was 0.575)`, and a machine is **gone** only after it has been missing for five
  looks in a row, said once.

Under the footer, what the table cannot show:

```text
since the last look: 1 new, 1 repriced, 1 gone
- 46579456  1x RTX 5090  0.554 $/h  California, US
```

`--changes` prints only what moved, one line each, for a log or a monitor: a line per look while it learns,
then one line per look (`no change`, or the counts followed by a line per offer: `+` new, `~` repriced,
`-` gone), none left out. Under the table, at most ten gone offers are listed, the rest counted.

A few details keep the notes honest:

- "Gone" is measured against the whole list the search kept, not the rows shown, so an offer that sank
  below the last row is not gone. With `--top 3`, a new offer that is not among the three rows has no `+`:
  the note counts it (`1 new (1 below the table)`).
- vast.ai answers at most 64 rows per search: when it does, a change in who entered or left may only be an
  offer crossing that line, and the note says so.
- A machine listed under two offer ids at DIFFERENT prices (the same GPU count) counts once, by its cheaper
  row; if one look shows only the cheap id and the next only the dearer one, that reads as a price that moved.
  The table can show a machine twice (two rows), and `SEEN` counts it once.
- A search that fails ends a watch whose FIRST look failed (a wrong `VAST_URL` or key would otherwise loop on
  stderr forever); after one look has succeeded, a failure is only said on stderr, and the next look reads
  `since the last successful look`. The lookout's `vas --watch` shares the first two rules (a failed first
  look ends it, a failure after a success is said and the watch goes on) but prints no such note.
- A `--watch` under the floor (10 s for `vas`, 20 s for `vas offers`) is raised to it, and `vas` says so on
  stderr. The floor is a courtesy to the service: vast.ai's own limits are read and obeyed whatever it says.
  `VAS_WATCH_FLOOR_MS` (milliseconds) replaces both floors: it exists for the test suite, and setting it
  removes the courtesy and nothing else.
- A table every few seconds scrolls past unread in a terminal: for a person, `--changes` (one line per look and
  per change) reads best; `vas --compact --watch N` is for scripts.
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
| [From filters to a running box](docs/tutorials/from-filters-to-a-running-box.md) | A tutorial: from a filters file to a running box, and watching it until you are done |
| [FAQ](docs/FAQ.md) | The questions that came up on real rentals: the key and the files, why the offer list changes, the money line, what `--ssh` reads, a slow rental (power cap, someone else on the GPU), rate limits |
| [Roadmap](docs/roadmap-v0.2.0.md) | What 0.2.0 contains and does not, the known limits, and the candidates for later (a text interface, a measured upload, ...) |
| [`vas` and `vastai`](docs/comparison-with-vastai.md) | Vast.ai's own CLI/SDK and `vas`, compared row by row: scope, read-only by design, the GPU read on the box, the runway, the offer search, key hygiene, redirects, the `ssh` port rule |
| [Dogfooding, 2026-10-16](https://github.com/mi-for-the-rust-of-us/vast-skopos/blob/main/docs/dogfooding/2026-10-16.md) | The first real rental (Me35, three RTX 5090s): what broke in the first minute, what helped, the next features, and `vas offers` probed and built for the next rental |
| [Dogfooding, 2026-10-17](https://github.com/mi-for-the-rust-of-us/vast-skopos/blob/main/docs/dogfooding/2026-10-17.md) | `vas offers`' first rental (Me36, five 5090s for three slots): what worked, seven notes for the next versions (offers that vanish, a burn line that can mislead, machine ids, a compact mode, labels, a two-size uplink measure for `vas check`, a credit history) |
| [Dogfooding, 2026-10-18](https://github.com/mi-for-the-rust-of-us/vast-skopos/blob/main/docs/dogfooding/2026-10-18.md) | `vas offers --watch` on the live market: boxes going away and arriving by the second, and the idea of a TUI for the watch |
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
