# Waiting for a line

A run prints the line you need once (the first loss of a stage, a final score, a `Traceback`), and then
other lines bury it. This tutorial replaces the loop you would write for that (`ssh` every two minutes,
`grep`, stop after a few failures) with two flags of `vas` (vast-skopos 0.3): `--grep` to see the line,
`--until` to wait for it. It assumes a rented box that `vas --ssh` can already read (see
[From filters to a running box](from-filters-to-a-running-box.md), steps 7 and 8).

**Not affiliated with vast.ai.** `vas` only reads: on the box, `nvidia-smi`, `ls`, `tail`, `tr` and `grep`.

About the examples: they are output from a real rental (two RTX 5090s, 2026-10-25), trimmed. Commands have a
PowerShell spelling where it differs.

## 1. The last line is rarely the one you want

`--log` shows the last line of the newest file matching a path or a glob:

```sh
vas --ssh --log '/root/src/askesis/reference/canvas/data/runs/*/run.log'
```

```text
... LAST LOG / SSH ERROR
... me38_c12o_s0_d1/run.log: epoch  57 | train 0.1451 | val(ema) 0.1506
... me38_c12o_s1_d1/run.log: epoch  57 | train 0.1906 | val(ema) 0.1500
```

That is the right line for "is it still training?". It is the wrong one for "where did it resume?" or
"did it archive?": those lines came earlier.

## 2. See the last line that matches: `--grep`

```sh
vas --ssh --log '/root/src/askesis/reference/canvas/data/runs/*/run.log' --grep 'archived|resumed'
```

```text
... me38_c12o_s0_d1/run.log: archived /root/src/askesis/reference/canvas/data/runs/me38_c12o_s0_d1/canvas_ema_e55.safetensors
... me38_c12o_s1_d1/run.log: archived /root/src/askesis/reference/canvas/data/runs/me38_c12o_s1_d1/canvas_ema_e55.safetensors
```

- The pattern is an **extended regular expression** (`grep -E`), case-sensitive: `|` is "or", `+` is "one
  or more", `.` is any character (write `\.` for a dot).
- The search covers the file's **last 64 MiB**, and each carriage-return segment of a progress bar is a line
  of its own, so a bar's `50%` state can be found.
- When no line matches, the cell says `(no line matches)`; when `grep` cannot read the pattern (`(` alone),
  `(grep refused the pattern)`.
- The pattern is quoted for the box's shell: nothing in it is expanded, so `$`, quotes and backquotes are
  characters to match, never commands. (The path is the opposite: the box's shell expands the glob, so quote
  it on your side.)

## 3. Wait for it: `--until`

`--until PATTERN` reads the log as `--grep PATTERN` does, every `--watch` seconds, and **exits** at the first
look where a box's log has a matching line. The run above stops at epoch 60:

```sh
vas --ssh --log '/root/src/askesis/reference/canvas/data/runs/*/run.log' --watch 30 --until 'epoch +60 '
```

Each look prints the whole table, as any `--watch` does; the cell says `(no line matches)` until the line is
there. Nine looks later (13:25 to 13:30 UTC), the last table and, on stderr, the line that ended the wait:

```text
== vas, unix time 1792934997
... GPU% (ssh)  ...  TREND              ...  LAST LOG / SSH ERROR
... 60%         ...  99 99 99 99 99 60  ...  me38_c12o_s0_d1/run.log: epoch  60 | train 0.1145 | val(ema) 0.1501
... 65%         ...  99 99 99 99 99 65  ...  me38_c12o_s1_d1/run.log: epoch  60 | train 0.1319 | val(ema) 0.1495
credit $5.66 | burn $1.024/h over 2 running | ~5.5 h left at this rate
vas: --until: box 57641653 matched: epoch  60 | train 0.1145 | val(ema) 0.1501
```

Both boxes reached epoch 60 in the same look; the first box in the table is the one named. (The `TREND` falling
to `60` is the same moment seen from the GPU: the run is done.)

The rules, so a script can rely on them:

| Exit code | When |
|---|---|
| `0` | a box's log has a matching line; `vas: --until: box ID matched: LINE` on stderr |
| `3` | no box could be read for **2 looks in a row**: every probe failed, or no box is running or starting |
| `1` | the first look failed (a wrong key, say); after a look has succeeded, a failed instances request is only said, and does not count |
| `2` | a flag was refused: `--until` needs `--log` and `--watch`, and replaces `--grep` |

A box that is still starting (`loading`, meant to run) keeps the wait going: you can start the wait right after
renting. A box read without a match yet keeps it going too, even before its log file exists.

## 4. Wait for success or failure at once

The `|` of a regular expression lets one wait end on either line:

```sh
vas --ssh --log '/root/runs/*/run.log' --watch 60 --until 'initial loss|Traceback'
```

Then read the line it printed to know which one it was.

## 5. In a script

`vas` has no time limit of its own; the system's `timeout` gives one (bash, and Git Bash on Windows):

```sh
timeout 3600 vas --ssh --log '/root/runs/*/run.log' --watch 60 --until 'initial loss|Traceback' > watch.log
case $? in
  0)   echo "the line is there" ;;
  3)   echo "no box could be read: look at the dashboard" ;;
  124) echo "an hour, and no line" ;;
  *)   echo "vas failed" ;;
esac
```

PowerShell has no `timeout` command; the exit code is in `$LASTEXITCODE`:

```powershell
vas --ssh --log '/root/runs/*/run.log' --watch 60 --until 'initial loss|Traceback' > watch.log
switch ($LASTEXITCODE) { 0 { "the line is there" } 3 { "no box could be read" } default { "vas failed" } }
```

The tables go to stdout (`watch.log` here) and the matched line to stderr. With `--compact`, each look is JSON
Lines instead, and each box's log reading is an object (`state` `line` with the `line`, or `no_matching_line`):
`vas --compact ... | jq -r 'select(.kind=="box") | .probe.log.line'`.

## Where next

- [FAQ](../FAQ.md#how-do-i-wait-for-a-line-in-a-log-without-a-loop-of-my-own): the same rules, in short.
- [From filters to a running box](from-filters-to-a-running-box.md): a whole rental with `vas`.
- [README](../../README.md): every flag.
