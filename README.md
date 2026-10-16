# vast-skopos

**Not affiliated with vast.ai.** An independent, read-only tool that reads the vast.ai API with
your own key, and your rented boxes over your own `ssh`.

`vas` is a lookout (σκοπός) over the boxes you rent on [vast.ai](https://vast.ai): one table, for
every instance, its status, price, age and cost so far, and its `ssh` endpoint; under it, your
credit, what your running boxes burn per hour, and how many hours that credit buys at that rate.
With `--ssh`, it also asks each running box directly what its GPUs are doing, and with `--log`
what the run last wrote.

```text
$ vas --ssh --log '/root/runs/*/train.log'
ID        LABEL  GPU          WHERE  STATUS   $/H    AGE    COST~  SSH                 GPU% (ssh)  VRAM       LAST LOG / SSH ERROR
56053340  -      1x RTX 5090  US     running  0.643  2h 0m  1.29   ssh4.vast.ai:23484  98%         29.4/32G   s0/train.log: [ 796.4s] layer 4: ...
56053638  -      1x RTX 5090  US     exited   0.608  10m    0.10   ssh4.vast.ai:37038  -           -          -
credit $5.94 | burn $0.643/h over 1 running | ~9.2 h left at this rate
```

## Why

The dashboard's GPU utilisation has been seen to read **0 % on boxes running at 97-99 %**, and an
instance can show "open" long after its run ended. `vas` therefore takes the GPU reading from the
box itself (`nvidia-smi`, over `ssh`) and never shows the API's `gpu_util`. The money line is
there because a rental is a budget: how long the credit lasts at the current burn is the number
to watch.

## Install

```sh
cargo install vast-skopos      # installs the `vas` binary
```

Library only (no `clap`): `cargo add vast-skopos --no-default-features`.

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

**Use a read-only key if you can.** `vas` only ever issues `GET` requests: it never creates,
starts, stops or destroys anything.

## Usage

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
refresh: a box that refuses is reported in its row (with `ssh`'s own last
error line), never retried in a loop. Host keys are accepted on first use
(`StrictHostKeyChecking=accept-new`), since vast.ai's `ssh` proxies are reached by port.

`COST~` is an estimate: today's `$/h` times the hours since creation. vast.ai bills storage while an
instance is stopped and prices can change; the invoice is the reference.

## Endpoints

The two endpoints read, as the official `vastai` CLI reads them:

- `GET /api/v1/instances/`, paginated (`limit`, `order_by`, then `after_token`);
- `GET /api/v0/users/current`, of which only `credit` and `balance` are read.

The base URL is `https://console.vast.ai`, or `VAST_URL` when set. Rows are read field by field,
so a field the API adds, drops or retypes costs a dash in one cell rather than the whole table.

## `vas` and `vastai`

`vastai`, Vast.ai's own CLI and SDK, rents and manages boxes; `vas` only watches them, read-only.
They are complements: [`docs/comparison-with-vastai.md`](docs/comparison-with-vastai.md) compares
them row by row.

## Conventions

[`CONVENTIONS.md`](CONVENTIONS.md): Grit + Grit-MI, as in
[hf-fetch-model](https://github.com/mi-for-the-rust-of-us/hf-fetch-model), with this crate's own
rules for the API key and for talking to a box.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option.

vast.ai is a trademark of its owner, used here only to say what this tool works with.
