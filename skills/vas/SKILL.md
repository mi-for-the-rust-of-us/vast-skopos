---
name: vas
description: Watch rented vast.ai GPU boxes with vas, the read-only lookout of the vast-skopos crate. It shows each box's status, cost, credit and runway, what the GPUs are doing (over ssh), a log's last or matching line, a wait for a line, and the offers to rent. Use it when someone asks how their vast.ai boxes or GPUs are doing, whether a card is idle, capped or shared, why a box is not running, to wait for a line in a training log, to keep the credit as a curve, or to find an offer like a box they had.
---

# vas: a lookout over rented vast.ai boxes

`vas` reads; it never acts. It sends only GET requests to the vast.ai API, and on a box it runs only
read-only commands (`nvidia-smi`, and `ls`, `tail`, `grep` and their helpers for a log) over the user's
own `ssh`. It never rents, starts, stops, labels or
destroys a box: that is the human's call. Say what you see and recommend; let the human act.

What it prints goes to stdout (the table, or JSON Lines); its notices (a wait, a warning, an error) go to
stderr. `vas -h` lists every flag in one line each; `vas --help` adds the detail, examples, the exit codes
and the environment variables.

## Which command answers which question

| The question | The command |
|---|---|
| How are my boxes doing, and costing? | `vas` |
| Is a box idle, paid for nothing? | `vas --ssh` (plain `vas` cannot tell) |
| Is a GPU capped or shared? | `vas --ssh` |
| What did each run write last? | `vas --ssh --log '/root/runs/*/train.log'` |
| Did a run print a given line once? | `vas --ssh --log '/root/runs/*/train.log' --grep 'Traceback'` |
| Why is this box not running? | `vas` (the line under the table) |
| Which machine is this box, to rent it again or avoid it? | `vas --wide` |
| Which offers pass the filters? | `vas offers --gpu RTX_5090 --verified --max-price 0.5` |

## Recipes

**How are my boxes doing?** Run `vas` once. Each row is a box: status, `$/H`, age, `COST~` (an estimate:
today's price times the hours since creation; the invoice is the reference), the `ssh` address. The last
line is the money: `credit $9.33 | burn $1.195/h over 2 running | ~7.8 h left at this rate`. For a
program, `vas --compact` (below).

**Is a GPU idle, capped or shared?** Run `vas --ssh`: `nvidia-smi` is read on each running box, once,
in parallel. Read `GPU% (ssh)`, `OTHER`, `POWER` and `CLOCK` (the marks, below). Never trust the
dashboard's utilisation: it has read 0 % on boxes at 97-99 %. One sample is not a diagnosis; a watch
(`vas --ssh --watch 60`) adds `TREND` and `CLOCK TREND`, the last six samples.

**Wait for a line, with a time limit.** `--until` waits for a line matching an extended regular
expression on any box, then exits 0 and says the line on stderr. It has no time limit of its own:
wrap it in `timeout`.

```sh
timeout 600 vas --ssh --log '/root/runs/*/train.log' --watch 30 --until 'initial loss|Traceback'
```

Then act on the exit code (below): 0 is the match, 124 is `timeout`'s limit. Read the exit code, not the
table: the matched line goes to stderr, and can land in the middle of the table on a terminal. While it
waits, the log cell shows the last line matching the pattern, so `(no line matches)` there means "not
yet". `--log-wait 300` gives up sooner (exit 3) when no box has a file matching the path (900 s by
default). `--watch 30` is a fair pace for a wait (at least 10).

**Follow one box of several.** `--instance` (repeatable) keeps only the boxes named: the rows, the
probes, the trend and `--until` follow them, while the money line stays the whole account's.

```sh
timeout 600 vas --ssh --log '/root/runs/*/train.log' --watch 30 --until 'initial loss' --instance 56533728
```

**Find an offer like the box we had.** `vas --wide` shows the box's `$/H`, its `MACHINE` and `HOST` ids,
its CPU, driver, DLPerf and PCIe. Then search with the same columns, keyless:

```sh
vas offers --gpu RTX_5090 --num-gpus 1 --verified --max-price 0.49 --exclude-machine 100682 --wide
vas offers --machine 100682 --wide
vas offers --gpu RTX_5090 --exclude-host 406325
```

The first finds cheaper verified machines like it (`--max-price` is in $/h, storage included) and leaves
its own out; the second asks whether that machine is for rent again; the third leaves a whole host out.
A host that capped one card may cap the others: no flag filters on the power limit, and `LIMIT W` is the
limit the listing gives, not a reading. Only `vas --ssh` reads the card, once the box is rented.

The offers table: `$/H` (storage included, 5 GB unless `--disk`), `REL %` (reliability), `MAX DAYS` (the
longest rental), `VERIFICATION` (`verified`, `unverified` or `deverified`), and under `--wide`
`MACHINE`, `CPU`, `DRIVER`, `DLPERF` (vast.ai's benchmark score), `DLPERF/$`, `PCIE GB/s` and `LIMIT W`. The
footer names the constraints in force; `N dropped here` counts the rows vast.ai sent that a constraint,
re-checked on each row, refused (it may ignore a filter), which is expected. vast.ai answers at most 64
rows and its answers vary from one search to the next: the footer says so. `vas offers --watch 60` reads
the market over several looks (the first three only learn).

**Keep the credit as a curve.** `vas --credit-log credit.csv --watch 300` appends one row per look
(`unix_time,credit,burn_per_hour,running,unsure`); the file is only ever appended to, and never holds
the key.

## `--compact`: JSON Lines for programs

Whenever you parse the output, use `vas --compact` (it cannot be used with `--wide`, and it carries what
`--wide` shows). Each line is one object: one per box (`kind` is `box`), under `--ssh --watch` one per
change of a GPU's power (`event`), then one for the money (`money`), which closes a look. Every key is
always there: what is not known is `null`, never a missing key. Keys come in alphabetical order. Filter on
the kind: `vas --compact | jq -r 'select(.kind=="money").hours_left'`.

<!-- keys: every name in backticks between this line and the closing comment is a key that --compact writes (a test checks it) -->
- A box: `kind`, `at` (Unix seconds, the same on every line of a look), `id`, `label`, `status`,
  `intended`, `status_msg` (vast.ai's own message), `gpu_name`, `num_gpus`, `dph` ($/h), `age_s`, `cost`,
  `where`, `ssh_host`, `ssh_port`, `direct_host`, `direct_port`, `machine_id`, `host_id`, `cpu_name`,
  `driver_version`, `dlperf`, `pcie_bw`, `power_reference_w`, `probe`.
- `probe` is null (not probed), an error (`error`, `error_kind`, `direct_error`), or the readings:
  `gpus`, `log`, `route`, `util_trend`, `clock_trend` (the trends are null in a one-shot).
- Each of `gpus`: `index`, `util_pct`, `mem_used_mib`, `mem_total_mib`, `other_mem_mib`, `power_w`,
  `power_limit_w`, `power_default_w`, `power_max_w`, `sm_mhz`, `sm_max_mhz`, `throttle`, `processes`.
- `log` (null without a log path): `state`, `file`, `line`.
- The money: `kind`, `at`, `credit`, `burn`, `hours_left`, `running`, `unsure`, `boxes`, `shown`.
<!-- /keys -->

`error_kind` is `unreachable` or `failed`; `route` is `proxy` or `direct`; the log's `state` is `line`,
`empty`, `not_a_file`, `no_match` or `unreadable`, and under `--grep` also `no_matching_line` or
`pattern_refused`. `throttle` is a list of words (empty when nothing holds the card).

## The exit codes, and what to do after each

| Code | Means | Then |
|---|---|---|
| 0 | The look was printed; or `--until` matched (the line is on stderr); or stdout was closed | Read the output, or the matched line |
| 1 | A look failed (a one-shot's, or a watch's first); no API key; a wait's stdout closed | Read stderr: the key, the network, `VAST_URL`. Do not loop on it |
| 2 | A flag or value was refused, before any request; or a box's `grep` refused the `--until` pattern | Fix the command or the pattern; do not retry it unchanged |
| 3 | `--until` gave up: no box could be read for 2 looks in a row, or none had a file matching `--log` for `--log-wait` seconds | Check the path and the boxes (`vas --ssh --log '/root/runs/*/train.log'`), then tell the human |
| 124 | `timeout` ended the wait | The line did not come in time: say so, with the last lines |

`vas offers` uses 0, 1 and 2 only. A watch without `--until` runs until Ctrl+C.

## How to read each mark

- `GPU% (ssh)` `100%?`: a card at 99 % or more that holds under 1 GiB and lists no process. That is
  likely an idle card's stuck counter, not work; a line under the table says so.
- `POWER` `400/575W+power_cap`: a power limit of 400 W set below the card's 575 W default, and the
  clocks held by it. `CLOCK` then reads low (`1717/3090` MHz): the run is slower than the model suggests.
- `POWER` `575W+power_cap`: the card is at its default limit under full load. That is normal.
  Compare the two numbers and the clock, not the word. A bare `575W` is a card at its default with
  nothing holding its clocks; an idle card's low `CLOCK` (`210/3090`) is then rest, not a cap. Other
  words than `power_cap`: `thermal`, `slowdown`, `power_brake`.
- `POWER` `450W<575`: the board's maximum is below its model's reference, so the board itself is the cap.
- `CLOCK` `1717/3090`: the SM clock now over the highest the card reaches, in MHz.
- `OTHER` `15.7G`: GPU memory held outside the container (someone else, or the host). `-` means
  nothing notable, or not known.
- `UNREACHABLE 4m: ...`: `ssh` never reached the box (refused, timed out). The age is what the choice
  between waiting and destroying rests on: a box of a minute or two may still be starting. After several
  minutes, tell the human; destroying it is their call. Before they destroy it, note its `MACHINE` from
  `vas --wide`.
- `SSH FAILED 4m: ...`: `ssh` reached the box and was turned away (a refused key), or got no GPU reading.
  Waiting will not fix a refused key: the public key must be on the vast.ai account.
- `(direct) s0/train.log: ...`: the readings came by the box's direct address, past the proxy.
- The line under the table, `57340012 loading: 46b4edaa8d47: Pull complete`, is vast.ai's own message
  on a box that is not running: here, the image arriving. That is progress, not a stuck box.
- The log cell, `s0/train.log: step 200 loss 1.8843`: the newest file matching `--log` (its last two path
  components: here the run is `s0`) and its last non-blank line, or under `--grep` or `--until` its last
  line matching the pattern. `(no line matches)`: no line matches the pattern (yet); `(empty)`: the file
  has no line to show; `(no file matches)`: nothing matches the path on that box; `(not a file)`: the
  newest match is a folder; `?`: the box's answer was cut short.
- `! 56533728 gpu 0: power limit 575 -> 400 W, power_cap began`: under `--ssh --watch`, a change since
  the last look.
- The money line's `+ 1 offline (meant to run, billing unknown)`: a box asked to run that reads
  otherwise. It is counted in the burn, since it may bill.

## The rules

- Never print, log or echo the API key (`VAST_API_KEY`, `~/.config/vastai/vast_api_key`). `vas` never
  shows it; do not either. There is no `--key` flag, on purpose.
- `vas` sends only GETs. Any action on a box (rent, stop, destroy, relabel) is the human's.
- A watch spends no credit (a few API calls per look), but the boxes cost every minute they run. Say so
  when a box sits idle or unreachable.
- Do not run `vas` in a tight loop: use `--watch N` (at least 10 s; 20 s for `vas offers`). vast.ai
  rate-limits its API, and `vas` already waits for its budget.

## The traps

- Git Bash rewrites an argument that looks like a POSIX path: `/root/run.log` reaches `vas` as
  `C:/Program Files/Git/root/run.log`. `vas` reads it as typed again and says so on stderr; a path
  under another drive is refused (exit 2). `//root/run.log` or `MSYS_NO_PATHCONV=1` avoids the rewrite.
- A `--log` glob that matches several runs on one box reads only the newest file. Name the run in
  the path when the newest is not the one wanted.
- Quote the glob locally (`'/root/runs/*/train.log'`): the box's shell expands it, not yours.
- `--until` matches on any box. When several boxes write the same log, add `--instance`.
