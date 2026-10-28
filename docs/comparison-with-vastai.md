# `vas` and the `vastai` CLI/SDK

**Not affiliated with vast.ai.** This page compares `vas` (this crate) with
[`vastai`](https://pypi.org/project/vastai/), the CLI and SDK published by Vast.ai. Facts about
`vastai` come from its PyPI page (read 2026-10-16: version 1.8.3, released 2026-10-02; read again
2026-10-24, and again 2026-10-28: **version 1.9.1, released 2026-10-23**) and from the source of its `vast.py` (the
`vast-ai/vast-cli` repository, read 2026-10-15/16; the cells marked *(2026-10-24)* come from its master
at `42c3a47`, read that day, not checked against the 1.9.1 wheel). Where a cell says "not verified", we
did not check it.

On the `vas` side, a cell marked *(0.4.0)* describes what is built on the way to 0.4.0 but not released yet; the rest
is in 0.3.0, released on 2026-10-26.

**In one sentence:** `vastai` is the full tool to rent and manage boxes; `vas` is a read-only
lookout that tells you what your rented boxes are actually doing and what they cost. They are
complements, not alternatives: rent with `vastai` (or the dashboard), watch with `vas`.

| | `vastai` (Vast.ai) | `vas` (vast-skopos) |
|---|---|---|
| **Purpose** | "CLI and SDK for Vast.ai GPU Cloud Service": the whole lifecycle | A lookout over boxes you already rent |
| **Language, install** | Python ≥ 3.10, `pip install vastai` | Rust, one static binary, `cargo install vast-skopos` |
| **License** | MIT | MIT OR Apache-2.0 |
| **Search offers** | yes (`vastai search offers`: a query language over every offer field, sent as a `POST` to `/api/v0/bundles/`, with the key) | yes, narrower and keyless: `vas offers` with the flags a rental is chosen by (GPU, CUDA, disk, reliability, bandwidth, price, duration, verified, countries and hosts left out), from a config file of flags and the command line; `--host` and `--machine` keep only one host's or machine's offers. The legacy `GET` form, no `Authorization` header. Every constraint is re-checked on each row, and the footer says when vast.ai's 64-row cap was reached (it answers `truncated: false` all the same) |
| **DLPerf in the offers** *(2026-10-24)* | yes: `DLP` (vast.ai's deep-learning performance score) and `DLP/$` are default columns of `search offers` | yes, under `vas offers --wide`: `DLPERF` and `DLPERF/$` (per `$/h` shown, storage included), as plain numbers. DLPerf is no cap detector: it saw a 475 W cap (2026-10-24) and a 450 W board (10-26), and missed one host's 500 W cap on three boxes (10-25, 10-26) |
| **Identifying a machine in the offers** *(2026-10-24)* | yes: `mach_id`, `host_id`, `NV Driver`, `PCIE`, `cpu_ghz` and `vCPUs` are default columns; every field with `--raw` | under `vas offers --wide`: `MACHINE`, `CPU`, `DRIVER`, `PCIE GB/s`; `vas --wide` shows the same for a rented box, so the two can be set side by side. `--host` / `--machine` keep, `--exclude-host` / `--exclude-machine` leave out |
| **Verification statuses** *(2026-10-24)* | three, `verified`, `unverified` and `deverified`, coloured in `show instances-v1`, which filters on them (`--verification`); offers by query (`verified=False`) | the offer search's `VERIFICATION` column shows all three. `--verified` keeps only the verified; `--exclude-deverified` hides only the deverified and keeps the new, unverified machines (sent as `vericode <= 1`: the search ignores a filter on `verification`, probed 2026-10-25) |
| **Create / stop / destroy instances** | yes | **no, by design**: `vas` only ever issues `GET` |
| **Serverless endpoints** | yes (a serverless client) | no |
| **SDK (library use)** | yes, Python | yes, a small Rust library (`key`, `api`, `instance`, `offer`, `market`, `probe`, `render`, `runway`, `compact`, `trend`, `until`, `creditlog`). *(0.4.0)* The helpers `vas` uses for what it prints are documented and promised, and `GpuReading::new` builds a reading by hand, to drive a trend or a table without a box |
| **List instances** | yes (`show instances`, paginated `/api/v1/instances/`); *(2026-10-24)* also `show instances-v1`, with filters (`--status`, `--label`, `--gpu-name`, `--verification`), `--order-by` and `--cols` | yes, the same endpoint and pagination, read field by field so an API change costs one cell, not the table. *(0.4.0)* `--instance ID` keeps the boxes named, while the money line stays the whole account's |
| **vast.ai's status message** *(2026-10-24)* | yes: `status_msg`, in `show instances-v1 -v`'s columns, cut to 40 characters | yes, for each box that is not running, on a line under the table, cut at 120 characters; every box's in `--compact`. It tells a box that is progressing (`Pull complete`) from a stuck one, which `STATUS` alone (`loading`, `rebooting`, `?`) cannot: on 2026-10-28 a box whose message stayed on `Verifying Checksum` for five minutes was destroyed for it. *(0.4.0)* The message's line breaks read as spaces |
| **Account credit** | yes (`show user`) | yes, plus the **burn rate** of the running boxes and the **hours of runway** it buys |
| **Cost so far per box** | the dashboard and invoices | an estimate per row (today's `$/h` × age), labelled `COST~` |
| **GPU utilisation** | the API's `gpu_util` field | **read on the box** over `ssh` (`nvidia-smi`), with VRAM. The API's field is not shown: it read 0 % on boxes running at 97-99 % (2026-10-15). *(0.4.0)* A card that reads busy with no memory held and no process reads `100%?`: an idle counter, seen three times |
| **GPU power limit** *(2026-10-24)* | a field of the offer query language, `gpu_max_power` ("GPU power limit (watts)"): filterable (`'gpu_max_power>=575'`), not a default column. It comes from the listing, not from the card, and it has matched the card's own limit on two hosts: host 406325 listed 500 W, the cap read on its rented RTX 5090s (2026-10-25 and 10-26), and host 102063 listed 370 W for a machine whose rented RTX 3080 read a 370 W limit (2026-10-28). Not settled for every host. `vas offers --wide` shows it as `LIMIT W` | **read on the box** over `ssh` (`nvidia-smi`): the enforced limit against the default, the throttle reasons, the clocks (`POWER`, `CLOCK`). It caught a 500 W cap (2026-10-23) and a 475/600 W one (2026-10-24); a cap cost 8-15 % per step and was not priced in. *(0.4.0)* A board whose maximum is below its model's reference reads `450W<575`, which an idle look would otherwise show as uncapped (2026-10-26) |
| **What the run is doing** | `vastai logs` exists for container logs (not verified in detail) | `--log <glob>`: the newest matching file on each box, named by its last two path components (so a glob over sequential runs says which run), and its last non-blank line, cleaned (no control characters, cut at 120 characters); `(empty)`, `(not a file)` or `(no file matches)` otherwise; with `--grep PATTERN`, the last line MATCHING an extended regular expression instead (`(no line matches)` when none does) |
| **Watching** | one-shot commands | `--watch N`: the table every N seconds (at least 10, `vas offers`: 20); `--until PATTERN` ends the watch when a box's log has a matching line (exit 0), with exit codes a script can wait on. *(0.4.0)* `--instance ID` waits on the boxes named; a path that matches no file on any box gives up after `--log-wait` seconds (15 minutes by default) instead of waiting forever |
| **ssh address** | `ssh-url`: a mapped `22/tcp` port when present, else the proxy at `ssh_port`, or `ssh_port + 1` for Jupyter-launched instances | the proxy first (`+1` for Jupyter), matching the dashboard's command. *(0.4.0)* The mapped `22/tcp` port is shown as `DIRECT` under `--wide`, and tried once when the proxy never reached the box: the opposite order from `ssh-url`, since a direct port can be closed by design (one was, behind a home router, on 2026-10-26) |
| **ssh probing** | n/a | the system `ssh`, `BatchMode`, a 30 s deadline per box, **one attempt** (*(0.4.0)* per route), never a retry loop. A failure says which way it failed and how old the box is: `UNREACHABLE 4m: ...` or `SSH FAILED 4m: ...` |
| **API key storage** | `vastai set api-key KEY` writes it to `~/.config/vastai/vast_api_key` (legacy `~/.vast_api_key`); also `VAST_API_KEY` or `--api-key` | **reads** the same files and `VAST_API_KEY`, or `--key-file`; never writes the key, and there is **no `--key` flag** (it would land in shell history) |
| **Key hygiene** | the key is printed by nothing that we saw, but the CLI accepts it on the command line | redacted `Debug`, no `Display`, never in an error; the `users/current` answer, which contains the key, is never parsed beyond `credit` and `balance` |
| **HTTP redirects** | followed (`requests`' default) | **never** followed, so the key is never re-sent elsewhere. This is why `vas` requests the canonical `/api/v0/users/current/`: the bare path answers 301 |
| **Least-privilege key** | needs Write scopes for its write commands | works with **User Read + Instances Read** only |
| **Maturity** | Vast.ai's own, actively released (1.9.1 on 2026-10-23; 1.8.3 on 2026-10-02) | 0.3.0, released on 2026-10-26 (0.2.0 on 2026-10-19, 0.1.0 on 2026-10-17), with 0.4.0 under way; dogfooded on real rentals (Me35, Me36, the RTX 3090 and RTX 5090 rentals of 2026-10-17 and 18, askesis's Me38 rentals of 2026-10-22 to 25, Me41 on 2026-10-26), and checked on a test rental of two RTX 3080s on 2026-10-28 |

## When to use which

- **Choosing what to rent:** `vas offers`, with your filters in its config file, then rent the `ID`
  you chose with `vastai create instance` or the dashboard.
- **"Is the card I just rented capped?"** `vas --ssh` as soon as the box runs, before deploying:
  `POWER` reads the enforced limit against the board's maximum. On 2026-10-24 it chose between two
  RTX 5090s the listing showed as identical (575 W against 475/600 W), and the uncapped one then
  matched the fastest step time measured so far.
- **Renting, resizing, stopping, destroying:** `vastai` or the dashboard. `vas` cannot, and will not.
- **"Are my runs actually training, and how long does my credit last?"** `vas --ssh --log ... --watch 300`:
  one table, measured on the boxes, with the runway.
- **"Tell me when this run prints its first loss."** `vas --ssh --log ... --watch 60 --until 'initial loss'`,
  with *(0.4.0)* `--instance ID` to wait on one box of several.
- **Least privilege:** give `vas` a read-only key (User Read + Instances Read). Keep the key with
  Write scopes for `vastai`.

## What `vas` learned from `vastai`'s source

The endpoints and their pagination, the key file locations, and the Jupyter `+1` ssh-port rule all
come from reading `vast.py` (the functions `_fetch_all_instances_v1`, `show__user`, `_ssh_url`,
`search__offers`, whose default storage of 5 GiB `vas offers` prices in too). Two of them were found
the hard way, by dogfooding (`docs/dogfooding/2026-10-16.md`): the redirect on `users/current` and
the `+1` port. The offer search's 64-row cap was found by probing it (same note).
