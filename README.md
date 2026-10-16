# vast-skopos

[![CI](https://github.com/mi-for-the-rust-of-us/vast-skopos/actions/workflows/ci.yml/badge.svg)](https://github.com/mi-for-the-rust-of-us/vast-skopos/actions/workflows/ci.yml)
[![MSRV](https://img.shields.io/badge/MSRV-1.88-blue.svg)](https://www.rust-lang.org)
[![license](https://img.shields.io/badge/license-MIT_OR_Apache--2.0-blue.svg)](#license)
[![unsafe: forbid](https://img.shields.io/badge/unsafe-forbid-success.svg)](https://github.com/rust-secure-code/safety-dance/)
[![API: read-only](https://img.shields.io/badge/vast.ai_API-read--only_(GET)-informational.svg)](#why-read-only)

**σκοπός** — *Your rented vast.ai boxes, watched.* **Not affiliated with vast.ai:** an independent,
read-only tool that reads the vast.ai API with your own key, and your boxes over your own `ssh`.

> 🆕 **`0.1.0` (unreleased) watched its first rental, then learned to name the run it reads.** `vas` watched askesis's Me35 rental (three RTX 5090s, six runs, 2026-10-16); its first minutes fixed what that rental found: the account endpoint's `301` (no redirect is ever followed, so the canonical `/api/v0/users/current/` is requested) and the `ssh` proxy port of Jupyter-launched boxes (`ssh_port + 1`, the dashboard's own rule). For Me36's four sequential runs per box, `--log` now names the run it read (`me36_ab_s0_d1/train.log: …`) and says `(no file matches)`, `(not a file)`, `(empty)` or `?` where it once printed a bare `-`; a key saved by Notepad as `vast_api_key.txt` is named in the error. See [`CHANGELOG.md`](CHANGELOG.md) and [`docs/dogfooding/2026-10-16.md`](docs/dogfooding/2026-10-16.md).

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
> - **`no vast.ai API key found`?** → [The API key](#the-api-key) — where `vas` looks, and why there is no `--key` flag.
>
> Every flag is in `vas --help`; how `vas` differs from Vast.ai's own CLI is in [`docs/comparison-with-vastai.md`](docs/comparison-with-vastai.md).

## Install

Not yet on crates.io. From a clone:

```sh
cargo install --path . --locked      # installs the `vas` binary
```

Once published, `cargo install vast-skopos`. As a library, without the CLI (no `clap`):

```toml
[dependencies]
vast-skopos = { version = "0.1", default-features = false }
```

## Usage

```rust
use vast_skopos::api::Client;
use vast_skopos::key::ApiKey;
use vast_skopos::probe::ssh_probe;

fn main() -> vast_skopos::Result<()> {
    // The key, by the official `vastai` CLI's search order; it is never printed.
    let client = Client::new(ApiKey::discover()?);
    println!("credit {:?}", client.account()?.credit);
    for i in client.instances()?.iter().filter(|i| i.is_running()) {
        println!("{} {:?} at {:?} $/h", i.id, i.gpu_name, i.dph_total);
        if let Some((host, port)) = i.ssh_endpoint() {
            // One attempt over the system `ssh`; the GPU as the box itself reads it.
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

`render::table` and `render::money_line` print what `vas` prints.

## Try it

With your key where the `vastai` CLI keeps it ([The API key](#the-api-key)):

```text
$ vas --ssh --log '/root/runs/*/train.log'
ID        LABEL  GPU          WHERE  STATUS   $/H    AGE    COST~  SSH                 GPU% (ssh)  VRAM       LAST LOG / SSH ERROR
56053340  -      1x RTX 5090  US     running  0.643  2h 0m  1.29   ssh4.vast.ai:23484  98%         29.4/32G   s0/train.log: [ 796.4s] layer 4: ...
56053638  -      1x RTX 5090  US     exited   0.608  10m    0.10   ssh4.vast.ai:37038  -           -          -
credit $5.94 | burn $0.643/h over 1 running | ~9.2 h left at this rate
```

## Binary (`vas`)

| | |
|---|---|
| `vas` | the table and the money line |
| `vas --ssh` | also probe every running box over `ssh`, once each, in parallel |
| `vas --ssh --log PATH_OR_GLOB` | also print, for the newest match of the pattern on each box, its last two path components and its last non-blank line (`s0/train.log: ...`, so a glob whose varying part is among the last two components says which run wrote it); `(empty)` when the file has no line to show, `(not a file)` when the newest match is a directory, `(no file matches)` when nothing matched, `?` when the box's answer was cut short. The pattern is expanded by the box's shell, so quote it locally (and quote a part with spaces again inside, `'"/root/my runs"/*/train.log'`); the line is shown without control characters, at most 120 characters, the path at most 60 |
| `vas --ssh -i ~/.ssh/id_ed25519` | use this private key (default: `ssh`'s own choice, agent and `~/.ssh/config`) |
| `vas --watch 60` | refresh every 60 s (at least 30) until Ctrl+C |
| `vas --key-file FILE` | read the key from `FILE` |

`--ssh` uses your system `ssh` with `BatchMode=yes`, `ConnectTimeout=10` and keep-alives, gives
each probe 30 s in all (and `nvidia-smi` 20 s on the box), and makes **one attempt** per box per
refresh: a box that refuses is reported in its row (with `ssh`'s own last error line), never
retried in a loop. Host keys are accepted on first use (`StrictHostKeyChecking=accept-new`), since
vast.ai's `ssh` proxies are reached by port.

`COST~` is an estimate: today's `$/h` times the hours since creation. vast.ai bills storage while an
instance is stopped and prices can change; the invoice is the reference.

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

### `vas` and `vastai`

`vastai`, Vast.ai's own CLI and SDK, rents and manages boxes; `vas` only watches them, read-only.
They are complements: [`docs/comparison-with-vastai.md`](docs/comparison-with-vastai.md) compares
them row by row.

#### Why read-only?

`vas` only ever issues `GET` requests: it never creates, starts, stops, labels or destroys an
instance. A monitor that can spend or destroy is a different tool, with a different review; what to
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
| [`vas` and `vastai`](docs/comparison-with-vastai.md) | Vast.ai's own CLI/SDK and `vas`, compared row by row: scope, read-only by design, the GPU read on the box, the runway, key hygiene, redirects, the `ssh` port rule |
| [Dogfooding, 2026-10-16](docs/dogfooding/2026-10-16.md) | The first real rental (Me35, three RTX 5090s): what broke in the first minute, what helped, and the next features |
| [CONVENTIONS](CONVENTIONS.md) | The rules every line follows, the API-key and box rules included |
| [CHANGELOG](CHANGELOG.md) | Release history |

## Used by

- **askesis** (private) — PCfVW's research on planning inside a model, whose models are trained on
  rented vast.ai GPUs. `vas` watched its Me35 rental (three RTX 5090s, six runs, 2026-10-16), and
  its dogfooding notes drove `0.1.0`'s fixes; `--log` naming the run was written for Me36's four
  sequential runs per box.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT License](LICENSE-MIT) at your option.

vast.ai is a trademark of its owner, used here only to say what this tool works with.

## Development

- Exclusively developed with [Claude Code](https://claude.com/product/claude-code) (dev)
- Git workflow managed with [Fork](https://fork.dev/)
- All code follows [CONVENTIONS.md](CONVENTIONS.md), derived from [Amphigraphic-Strict](https://github.com/PCfVW/Amphigraphic-Strict)'s [Grit](https://github.com/PCfVW/Amphigraphic-Strict/tree/master/Grit) — a strict Rust subset designed to improve AI coding accuracy — and following [hf-fetch-model](https://github.com/mi-for-the-rust-of-us/hf-fetch-model)'s conventions section for section.
