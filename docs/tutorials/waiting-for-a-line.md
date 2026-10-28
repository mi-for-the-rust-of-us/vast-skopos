# Waiting for a line

A run prints the line you need once (the first loss of a stage, a final score, a `Traceback`), and then
other lines bury it. This tutorial replaces the loop you would write for that (`ssh` every two minutes,
`grep`, stop after a few failures) with two flags of `vas` (vast-skopos 0.4): `--grep` to see the line,
`--until` to wait for it. It assumes a rented box that `vas --ssh` can already read (see
[From filters to a running box](from-filters-to-a-running-box.md), steps 7 and 8).

**Not affiliated with vast.ai.** `vas` only reads: on the box, `nvidia-smi`, `ls`, `head`, `tail`, `tr`, `cut`
and `grep`, under `timeout` (and a `sh` of its own for the search).

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
- The search covers the file's **last 64 MiB**, within 10 s on the box (`?` when it runs past them, or when
  the file cannot be read). Each carriage-return segment of a progress bar is a line of its own, so a bar's
  `50%` state can be found; empty lines are left out; and each line's first 4096 bytes are searched: a long
  line is shown from its head.
- When no line matches, the cell says `(no line matches)`; when `grep` cannot read the pattern (`(` alone),
  `(grep refused the pattern)`; when the line that matched is made of spaces only, `(a blank line)`.
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

**The first box ends the wait, not every box.** On 2026-10-26, two runs started four minutes apart, and a wait
for epoch 10's archive line (`--watch 60 --until 'archived .*_e10\.|Traceback'`) ended after 8 looks
(09:18:58 to 09:26:28 UTC) on the first box, while the second still read `(no line matches)`:

```text
... me38_l12_s0_d1/run.log: archived /root/src/askesis/reference/canvas/data/runs/me38_l12_s0_d1/canvas_ema_e10.safetensors
... me38_l12_s1_d1/run.log: (no line matches)
vas: --until: box 57787711 matched: archived /root/src/askesis/reference/canvas/data/runs/me38_l12_s0_d1/canvas_ema_e10.safetensors
```

To wait for one box, name it with `--instance` (repeatable). Its line then ends the wait, whatever the other
boxes print, and the money line stays the account's:

```sh
vas --ssh --log '/root/src/askesis/reference/canvas/data/runs/*/run.log' --watch 60 \
    --until 'archived .*_e10\.' --instance 57787806
```

To wait for every box, start one wait per box, each with its own `--instance`. In 0.3.0, which has no
`--instance`, start a new wait after each match (the line on stderr names the box), or give each run a log path of
its own and wait on each path.

The rules, so a script can rely on them:

| Exit code | When |
|---|---|
| `0` | a box's log has a matching line; `vas: --until: box ID matched: LINE` on stderr |
| `3` | no box could be read for **2 looks in a row**: every probe failed or gave no readable log (`?`), or none could be made; or the boxes answered but none has had a file matching the `--log` path for **`--log-wait` seconds**, 900 (15 minutes) by default (a warning on stderr comes after 2 looks: `is the path right?`) |
| `2` | at once, when a box's `grep` refuses the pattern and no other box was read (no look could match: `vas: --until: grep on box ID refused the pattern ...`); or a flag was refused (`--until` needs `--log` and `--watch`, and replaces `--grep`) |
| `1` | the first look failed (a wrong key, say), or stdout was closed before a match (`vas ... \| head` is not a match); after a look has succeeded, a failed instances request is only said, and does not count |

A box meant to run but not running yet (`loading`) keeps the wait going, for **10 looks** at most, counted since
a box was last read: you can start the wait right after renting, and a box whose host went `offline` does not
hold it forever (after the ten, its looks count as looks with nothing to read). A box read without a match yet keeps it going too, and so does a box with no file matching the path yet, for
`--log-wait` seconds (15 minutes by default; `--log-wait 1800` for a run that sets up for half an hour first):
a run may write its log late, but a path that matches nothing on any box must not keep a script waiting while
the boxes cost. The time runs while any box answers without the file and none is read with it, whatever the
other boxes say (`loading`, `?`, a failed `ssh`, a refused pattern); a box read with the file starts it afresh.

**From Git Bash, check the path.** Git Bash rewrites `/root/...` into `C:/Program Files/Git/root/...` before
`vas` sees it. Since 0.4.0, `vas` reads it as typed again and says so. In 0.3.0 such a wait reads `(no file
matches)` on every box and waits forever (2026-10-26): use a glob, `//root/...` or `MSYS_NO_PATHCONV=1`
([FAQ](../FAQ.md#--log-with-an-exact-path-says-no-file-matches-but-the-file-is-there)).

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
