# `vas` and the `vastai` CLI/SDK

**Not affiliated with vast.ai.** This page compares `vas` (this crate) with
[`vastai`](https://pypi.org/project/vastai/), the CLI and SDK published by Vast.ai. Facts about
`vastai` come from its PyPI page (read 2026-10-16: version 1.8.3, released 2026-10-02) and from the
source of its `vast.py` (the `vast-ai/vast-cli` repository, read 2026-10-15/16). Where a cell says
"not verified", we did not check it.

**In one sentence:** `vastai` is the full tool to rent and manage boxes; `vas` is a read-only
lookout that tells you what your rented boxes are actually doing and what they cost. They are
complements, not alternatives: rent with `vastai` (or the dashboard), watch with `vas`.

| | `vastai` (Vast.ai) | `vas` (vast-skopos) |
|---|---|---|
| **Purpose** | "CLI and SDK for Vast.ai GPU Cloud Service": the whole lifecycle | A lookout over boxes you already rent |
| **Language, install** | Python ≥ 3.10, `pip install vastai` | Rust, one static binary, `cargo install vast-skopos` |
| **License** | MIT | MIT OR Apache-2.0 |
| **Search offers** | yes (`vastai search offers`) | no |
| **Create / stop / destroy instances** | yes | **no, by design**: `vas` only ever issues `GET` |
| **Serverless endpoints** | yes (a serverless client) | no |
| **SDK (library use)** | yes, Python | yes, a small Rust library (`api`, `instance`, `probe`, `render`) |
| **List instances** | yes (`show instances`, paginated `/api/v1/instances/`) | yes, the same endpoint and pagination, read field by field so an API change costs one cell, not the table |
| **Account credit** | yes (`show user`) | yes, plus the **burn rate** of the running boxes and the **hours of runway** it buys |
| **Cost so far per box** | the dashboard and invoices | an estimate per row (today's `$/h` × age), labelled `COST~` |
| **GPU utilisation** | the API's `gpu_util` field | **read on the box** over `ssh` (`nvidia-smi`), with VRAM. The API's field is not shown: it read 0 % on boxes running at 97-99 % (2026-10-15) |
| **What the run is doing** | `vastai logs` exists for container logs (not verified in detail) | `--log <glob>`: the newest matching file on each box, named by its last two path components (so a glob over sequential runs says which run), and its last non-blank line, cleaned (no control characters, at most 120 characters); `(empty)`, `(not a file)` or `(no file matches)` otherwise |
| **Watching** | one-shot commands | `--watch N`: the table every N seconds (at least 30) |
| **ssh address** | `ssh-url`: a mapped `22/tcp` port when present, else the proxy at `ssh_port`, or `ssh_port + 1` for Jupyter-launched instances | the proxy rule (`+1` for Jupyter), matching the dashboard's command |
| **ssh probing** | n/a | the system `ssh`, `BatchMode`, a 30 s deadline per box, **one attempt**, never a retry loop |
| **API key storage** | `vastai set api-key KEY` writes it to `~/.config/vastai/vast_api_key` (legacy `~/.vast_api_key`); also `VAST_API_KEY` or `--api-key` | **reads** the same files and `VAST_API_KEY`, or `--key-file`; never writes the key, and there is **no `--key` flag** (it would land in shell history) |
| **Key hygiene** | the key is printed by nothing that we saw, but the CLI accepts it on the command line | redacted `Debug`, no `Display`, never in an error; the `users/current` answer, which contains the key, is never parsed beyond `credit` and `balance` |
| **HTTP redirects** | followed (`requests`' default) | **never** followed, so the key is never re-sent elsewhere. This is why `vas` requests the canonical `/api/v0/users/current/`: the bare path answers 301 |
| **Least-privilege key** | needs Write scopes for its write commands | works with **User Read + Instances Read** only |
| **Maturity** | Vast.ai's own, actively released (1.8.3) | 0.1.0, unreleased, dogfooded on one rental (2026-10-16) |

## When to use which

- **Renting, resizing, stopping, destroying:** `vastai` or the dashboard. `vas` cannot, and will not.
- **"Are my runs actually training, and how long does my credit last?"** `vas --ssh --log ... --watch 300`:
  one table, measured on the boxes, with the runway.
- **Least privilege:** give `vas` a read-only key (User Read + Instances Read). Keep the key with
  Write scopes for `vastai`.

## What `vas` learned from `vastai`'s source

The endpoints and their pagination, the key file locations, and the Jupyter `+1` ssh-port rule all
come from reading `vast.py` (the functions `_fetch_all_instances_v1`, `show__user`, `_ssh_url`). Two
of them were found the hard way, by dogfooding (`docs/dogfooding/2026-10-16.md`): the redirect on
`users/current` and the `+1` port.
