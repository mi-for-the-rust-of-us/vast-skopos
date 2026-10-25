# Frequently asked questions

**Not affiliated with vast.ai.** `vas` is an independent, read-only tool. Everything here was observed
on real rentals or read from the code; where something is a guess, it says so. It describes the code as
it is now, on its way to **0.3.0**: 0.2.0 has no status line under the table, no `UNREACHABLE`, no
`--grep` or `--until`, no `--exclude-deverified`, `--host`, `--machine` or `--wide` on `vas offers`; 0.1.0
has no `--credit-log`, `--wide`, `--compact`, `--exclude-machine`, probe columns beyond utilisation and
memory, `--watch` on `vas offers` or rate-limit handling.

A few words used below:

- **the lookout**: `vas` on its own, which watches the boxes you already rent; `vas offers` searches the market;
- **a look**: one refresh (one search, or one look at your boxes);
- **burn**: what your running boxes cost per hour, added up;
- **host and machine**: a host owns hardware and may own several machines;
- **SM clock**: the speed of the GPU's compute cores, in MHz; **power cap**: a ceiling on the watts the card
  may draw, which lowers its speed.

- [What `vas` does and does not do](#what-vas-does-and-does-not-do)
- [The key and the files](#the-key-and-the-files)
- [Searching for offers](#searching-for-offers)
- [The table and the money](#the-table-and-the-money)
- [Looking inside a box](#looking-inside-a-box)
- [When a rental is slow](#when-a-rental-is-slow)
- [Rate limits and watching](#rate-limits-and-watching)
- [Windows, Linux, macOS](#windows-linux-macos)

## What `vas` does and does not do

### Can `vas` rent, stop or destroy a box?

No, and it never will: it only issues `GET` requests. To rent the offer you chose, take its `ID` to the
dashboard or to the official CLI (`vastai create instance`). To stop or destroy a box, use the dashboard
or `vastai`. A monitor that can spend money or destroy work is a different tool with a different review;
what to do about a box stays your decision. See [`vas` and `vastai`](comparison-with-vastai.md).

### Does `vas` write anything to my disk?

`vas` itself writes only one thing, and only if you ask: `vas --credit-log FILE` appends a row per look to
the file you name (see [the credit log](#what-does---credit-log-write)). It keeps no cache, no state, and no
copy of your key. (`vas --ssh` uses your system `ssh`, which records a new box's host key in your
`~/.ssh/known_hosts`, as any first `ssh` to a box does.)

### Does it work on Windows, Linux and macOS?

Yes: CI builds and tests on all three, on the minimum supported Rust version (1.88) and on stable (the
tests that fake the box's `nvidia-smi` run on Linux only). `vas --ssh`
uses the `ssh` program of your system, so it must be on your `PATH` (recent Windows ships an OpenSSH client).

## The key and the files

### Where does `vas` look for my API key?

In this order: the `VAST_API_KEY` environment variable, `$XDG_CONFIG_HOME/vastai/vast_api_key`,
`~/.config/vastai/vast_api_key`, then `~/.vast_api_key`: the places the official `vastai` CLI uses. Or
name a file with `--key-file FILE`. There is no `--key` flag on purpose: a key typed on the command line
lands in your shell history. `vas offers` needs no key at all.

### Which permissions should the key have?

If you can, a read-only key: on vast.ai, User Read and Instances Read, nothing else. `vas` reads the
instance list and your credit and nothing more.

### I saved the key on Windows and `vas` says it found none

Two usual causes:

- **Notepad appended `.txt`**: the file is called `vast_api_key.txt`. `vas` notices and names it in the
  error: rename the file.
- **The file is not one line of plain text.** A key is one word. A second line, a space, or a control
  character is refused when the file is read, with a message that does not show the key. PowerShell 5.1's
  `>` writes UTF-16 with a byte-order mark: that is fine, `vas` reads UTF-8 (with or without a mark) and
  UTF-16 with a mark. A UTF-16 file cut in the middle of a character is refused rather than half-read.

A relative `XDG_CONFIG_HOME` is ignored, as the XDG specification says.

### Why does the credit show `?`?

The account request failed, and `vas` said why on stderr (`vas: no credit shown: ...`). The table is still
printed. A rate limit is a likely reason (see [rate limits](#rate-limits-and-watching)); a key the API
refuses would be another, and the message names the status.

### What exit codes does `vas` use?

`0` for success (a closed pipe, such as `vas --watch 30 | head`, ends quietly with `0`; so does a
`--until` wait that found its line), `1` when a look fails or a key file or config file cannot be read or is
malformed (and when the stdout of a `--until` wait is closed before a match), `2` when a flag or a value is
refused, on the command line or in a config file (an unknown flag, a value out of range, flags that
conflict), or when a box's `grep` refuses a `--until` pattern, and `3` when a `--until` wait gave up: no box
could be read for 2 looks in a row.

## Searching for offers

### Why does `vas offers` need no key?

The offer search is public. `vas offers` sends no `Authorization` header and never looks for a key.

### Why only 64 offers?

vast.ai answers at most 64 rows per search, whatever you ask, and still says `truncated: false`. When 64
rows come back, the footer says the list may be cut: narrow the search (more filters, or `--sort`, which
is sent to the server too, so the 64 you get are the best ones in your order).

### Why does the list change from one search to the next?

Because vast.ai's search answers a **sample**, not the whole market. Measured on 2026-10-18: fifteen
identical searches, 2.5 s apart, saw 129 different machines (64 per answer) and only 30 of them in every
answer; the others came and went, and one machine could show up under two or three offer ids. Why the
answers differ is not known (a guess, not a finding: a machine is listed only while its host's report is
fresh). So one search is a snapshot of a sample, and every `vas offers` ends with a line saying so.

`vas offers --watch` therefore does not compare two answers. It identifies an offer by its **machine** and
its GPU count (not by its offer id), learns the market for the first three looks, and reports a machine as
new only when it was absent from the previous three looks, and as gone only after five looks in a row
without it. The `SEEN` column says in how many looks each machine appeared (`15/15` is a dependable
listing, `3/9` one that flickers).

### What is the difference between a machine id and a host id?

A host is a person or company that rents out hardware; a host may own several machines. `vas --wide`
shows both ids of your boxes. `vas offers --exclude-machine ID` leaves out one machine, `--exclude-host ID`
all of a host's machines; `--machine ID` and `--host ID` keep only them. All four can be repeated, they add
to the ones in your config file, and an id both kept and excluded is left out.

### Is this the machine I had?

`vas offers` shows the host, and one host may list several machines that look alike. `vas offers --wide`
adds what tells them apart: `MACHINE`, `CPU` and `DRIVER` (and `DLPERF`, `DLPERF/$`, `PCIE GB/s`, `LIMIT W`);
`vas --wide` shows the same `CPU`, `DRIVER`, `DLPERF` and `PCIE GB/s` for your rented boxes, so the two can be
set side by side. To see only one host's or one machine's offers: `vas offers --host 406325 --wide`. On
2026-10-25 that answered:

```text
ID        GPU          VRAM  $/H    CUDA  DISK  UP/DOWN Mb/s  REL %  MAX DAYS  HOST    MACHINE  VERIFICATION  CPU  DRIVER      DLPERF  DLPERF/$  PCIE GB/s  LIMIT W  WHERE
48989513  1x RTX 5090  32G   0.496  13.0  401G  182/213       99.65  6.1       406325  148820   deverified    -    580.178.04  11.0    22        47.2       500      South Korea, KR
1 offer, cheapest first | hosts 406325 | $/h includes 5 GB of storage | searched 13:27:11 UTC
```

A third machine of the host whose two rented boxes were running beside it: same driver (`580.178.04`), its
own id, deverified, and a DLPerf of 11.

### Can DLPerf or `LIMIT W` tell me that a host has capped the card?

DLPerf cannot, as far as seen: on 2026-10-25 two RTX 5090s of one host read DLPerf 199.2, the same as an
uncapped card, and `vas --ssh` read both capped at 500 of 600 W. `LIMIT W` (vast.ai's `gpu_max_power`, the
power limit the listing gives) may: a third machine of that host listed 500 W the same day, and RTX 5090 offers
listed anything from 400 to 575 W. That is one host, not a rule. The card's own reading, `vas --ssh`'s
`POWER`, is the one that says what the run gets: read it on every box you rent.

### Verified, unverified, deverified: which should I filter?

vast.ai's documentation defines three stages. **Verified** machines passed its automated checks
(reliability, network, operational health, performance) and are monitored. **Unverified** ones are new or
under evaluation: "not a judgment of quality". **Deverified** ones were verified and no longer meet the
requirements. None of the three checks speed or a power cap.

`--verified` keeps only the first, so it hides new machines too. `--exclude-deverified` hides only the
third. On 2026-10-24, an unverified box ran at full power and as fast as the fastest measured, a verified one
was capped at 475 W, and a deverified one never started three instances in 17 minutes: for a rental,
`--exclude-deverified` plus your own checks (`vas --ssh`'s `POWER`, a speed check) was the better rule. The
search ignores a filter on the `verification` field it shows, so `vas` sends `vericode <= 1` (0 is
unverified, 1 verified, 2 and up deverified, as probed on 2026-10-25) and re-checks each row; the footer
says `not deverified`. In a config file, `--no-exclude-deverified` on the command line cancels it.

### The price in the table differs from the one I expected

`$/h` includes the storage you ask for: `--disk GB` (5 GB when you do not say, the official `vastai` CLI's
default). The footer says how much storage is priced in; compare like with like.

### Why is a row shown that does not match my `--min-days` (or another filter)?

Every constraint is sent to the server and **re-checked on each row**. A row whose field contradicts a
constraint is dropped (and counted in the footer). A row that is *missing* the field is kept, with a dash:
the offer might qualify, and hiding it would hide a candidate.

### Where does the config file go, and what is in it?

The flags of `vas offers`, one per line, in `$XDG_CONFIG_HOME/vast-skopos/offers` or
`~/.config/vast-skopos/offers`, or the file `--config FILE` names. `#` lines are comments. It is read
first; on the command line a single value replaces the file's, and a list (`--exclude-country`, ...) adds
to it. `--no-config` reads no file. A file cannot hold `--watch` or `--changes`: a file of filters must not
turn a run into a loop. A missing `--config` file, an unreadable file or a line that is not a flag stops
the run, because going on would silently drop the exclusions the file holds.

## The table and the money

### What does `COST~` mean?

An estimate: today's `$/h` times the hours since the box was created. Prices can change, and vast.ai bills
the storage of a **stopped** box, which `vas` does not show (yet). The invoice is the reference.

### What does the burn count, and what does "billing unknown" mean?

The burn adds the boxes the API reads as `running` **and** the boxes you asked to run (their intended
status) that read otherwise (`offline`, `loading`, ...): `burn $1.940/h over 3 running + 1 offline (meant
to run, billing unknown)`. Whether such a box bills is not known from the API, and a burn that left it out
would overstate how long your credit lasts, so `vas` counts it and names it. A stopped box is not counted.

### A box stays `loading` for minutes: is it stuck?

Read the line under the table: for every box that is not running, `vas` prints vast.ai's own status message,
`57340012 loading: 46b4edaa8d47: Pull complete`. On 2026-10-24 two boxes read `loading -> running` at 2
minutes of age while that message named the image's layers arriving: they were progressing, and both read
`running` at the next look. A box whose message does not move for many minutes is another matter (three
boxes on one deverified machine read `rebooting -> running` for up to 17 minutes that day). `--compact`
carries every box's message as `status_msg`.

### What does `--credit-log` write?

One CSV row per look, to the file you name:

```text
unix_time,credit,burn_per_hour,running,unsure
1792328430,10.5100,0.4910,1,0
```

The header is written only when the file is new or empty. The file is only ever appended to, so it survives
restarts, and your own notes in it are kept (a row always starts on a new line). `credit` is empty when the
account did not answer; `unsure` counts the boxes meant to run that read otherwise. The burn already counts
those boxes, so compare the **fall** of the credit with `burn_per_hour`: a fall well below it while
`unsure` is above zero says those boxes do not bill; a fall above it says something else does (vast.ai
bills the storage of stopped boxes).

If the file cannot be written (a wrong folder, or a file open in Excel on Windows, which locks it), `vas`
says so once on stderr and stops trying for the rest of the run; the look goes on. A look whose instance
list failed writes no row. Nothing in the file is secret: no key, no endpoint.

### The credit goes down in steps, not smoothly

Seen once, on five rows (a guess, not a finding): the credit read `10.3378`, then `10.3330`, then `10.3280`,
steps of about $0.005, roughly 35 seconds of burn, with flat stretches between. It looks as though vast.ai
refreshes the credit in billing ticks, so a watch shorter than a tick often shows the same credit twice.
A longer `--credit-log` will tell whether the ticks are regular.

## Looking inside a box

### What does `vas --ssh` need?

`ssh` on your `PATH`, your key accepted by the box (your public key added to your vast.ai account: test
with a plain `ssh` first), and `nvidia-smi` on the box (GPU instances have it). `vas` connects as `root`,
to the box's `ssh` endpoint shown in the `SSH` column, **once per look and per box**, with `BatchMode`
(it never asks a question), a 30-second deadline, and never a retry loop. Use `-i FILE` for a particular
private key. Host keys are accepted on first use (a changed key is refused). Each probe has an `ssh` session
of its own, whatever your `~/.ssh/config` says about shared connections.

### The `SSH` column shows a port that differs from `ssh_port` in the API

For a box launched with Jupyter, vast.ai's `ssh` proxy listens on the port **plus one**; `vas` applies the
dashboard's own rule, so the address in the `SSH` column is the one to use, not the raw `ssh_port`.

### A box shows an error instead of GPU numbers

The probe failed, and the last cell says which way, how old the box is, and why in `ssh`'s own words:

- `UNREACHABLE 4m: ssh: connect to host ssh3.vast.ai port 32456: Connection refused`: `ssh` never reached
  the box (refused, timed out, no route, a name that does not resolve, the connection closed or reset before
  the session, or nothing back within 30 s);
- `SSH FAILED 4m: root@ssh3.vast.ai: Permission denied (publickey).`: it reached the box and was turned away
  (a refused key, a changed host key), or the box gave no GPU reading, or it began to answer and did not
  finish within 30 s (`the box answered, but the probe did not finish within 30 s`: a hung `nvidia-smi`, say).

Nothing is retried: what to do about the box is your call. A box that is not `running`, or has no endpoint,
is not probed (`-`). `--compact` says the same as `error_kind`: `unreachable` or `failed`.

### A box reads `UNREACHABLE`: should I wait or destroy it?

The age is there for that choice. Three rentals (2026-10-17, 10-18 and 10-23) met boxes that read `running`
at about 4 minutes while their `ssh` address refused every connection; each was destroyed unused, for a few
cents. A box of a minute or two may still be starting: look again. If it still refuses after several minutes,
try the direct address the dashboard gives (its public IP and the port mapped to 22) before giving up on it.
Before you destroy it, run `vas --wide` once and note its `MACHINE`: `vas offers --exclude-machine ID` keeps
it out of your next search. `SSH FAILED` is different: the box answered, so a refused key is fixed on your
side (is your public key on your vast.ai account?), not by waiting.

### Why not trust the dashboard's GPU utilisation?

On 2026-10-15 it read 0 % on boxes running at 97-99 %. `vas` shows what `nvidia-smi` says on the box, and
not the API's figure. (One `nvidia-smi` sample is not a diagnosis either: it has read 100 % with 0 MiB used
while nothing held the card, which is why a watch keeps a short history in `TREND`. Seen again on 2026-10-19 on
an idle rented RTX 5090: 100 %, 2 MiB used, 129 W, 2,925 MHz, and no process listed by `nvidia-smi pmon`. A
real load draws far more power and holds memory; read `VRAM` and `POWER` beside `GPU%`.)

### What does `--log` show?

For the newest file matching the path or glob on each box: its last two path components (so a glob over
sequential runs says which run wrote it) and its last non-blank line, cleaned of control characters and
cut at 120 characters. `(no file matches)` when nothing matched, `(not a file)` when the newest match is
a directory, `(empty)` for a file with no line to show, and `?` when the box's answer was cut short. Quote
the glob locally (`'/root/runs/*/train.log'`): the box's shell expands it. A long progress bar that is
rewritten in place for hours shows its final state.

With `--grep PATTERN`, the cell shows the last line that MATCHES instead (an extended regular expression,
`grep -E`, case-sensitive). On 2026-10-25, a run's log ended in an epoch line, and `--grep 'archived|resumed'`
found the archive line well above it:

```text
$ vas --ssh --log '/root/src/askesis/reference/canvas/data/runs/*/run.log' --grep 'archived|resumed'
... LAST LOG / SSH ERROR
... me38_c12o_s0_d1/run.log: archived /root/src/askesis/reference/canvas/data/runs/me38_c12o_s0_d1/canvas_ema_e55.safetensors
```

The search covers the file's last 64 MiB, within 10 s on the box (`?` when it runs past them, or when the
file cannot be read), each carriage-return segment of a progress bar as a line of its own, empty lines left
out, and each line's first 4096 bytes: a long line is shown from its head. `(no line matches)` when none does,
`(grep refused the pattern)` when `grep` cannot read it (`(` alone, say), `(a blank line)` when the line that
matched is made of spaces only. The pattern is quoted for the box's shell: nothing in it is expanded, so `$`, quotes and backquotes
are only characters to match.

### How do I wait for a line in a log, without a loop of my own?

`vas --ssh --log PATH --watch 30 --until PATTERN` looks every 30 s and exits `0` as soon as a box's log has a
line matching `PATTERN` (read as `--grep` reads it), saying which box and which line on stderr. It exits `2` at
once when a box's `grep` refuses the pattern and no other box was read (no later look could match), `3` when
no box could be read for 2 looks in a row (every probe failed or gave no readable log, or none could be made),
and `1` when the first look fails or its stdout is closed before a match; a box meant to run but not running
(`loading`, say) keeps the wait going for 10 looks at most since a box was last read, and a look whose
instances request failed does not count. A pattern
can wait for success or failure at once: `--until 'initial loss|Traceback'`, then read the line it printed.
There is no time limit of its own: `timeout 3600 vas ...` gives one. See the tutorial
[Waiting for a line](tutorials/waiting-for-a-line.md).

## When a rental is slow

### The GPU reads 100 % but the run is slower than it should be

Two things can hold a busy card back, and `vas --ssh` (0.2.0) shows both.

**A power limit below the default.** A card's power limit can be set below what it ships with. Seen on 2026-10-18 on two
rented RTX 5090s: a limit of 400 W against the default 575 W (set on the box; by whom, and why, is not
known), `SW Power Cap: Active`, and the SM clock held at about 1,700 MHz out of a possible 3,090, while the
card read 100 %. In the table:

```text
POWER               CLOCK
400/575W+power_cap  1717/3090
```

`POWER` is the limit set on the card, followed by the default when the limit is lower, and by what holds the
clocks down at this moment. The words are `power_cap`, `thermal`, `slowdown` and `power_brake` (an idle card, application
clocks and sync boost are not restraints and are not shown). `CLOCK` is the SM clock now over the highest
the card reaches. Under `--watch`, `CLOCK TREND` keeps the last six samples, and a line under the money
says when a limit or a reason changes between two looks:

```text
! 56533728 gpu 0: power limit 575 -> 400 W, power_cap began
```

Three things to know. First, **`+power_cap` alone is normal under full load**: it says the card is at its power
limit, which every card reaches. On 2026-10-19 a default-limit RTX 4060 Ti (`165W`) read `165W+power_cap` and
about 2,565 of 3,105 MHz while a matrix-multiply loop ran on it, and `165W` again afterwards, with the lines
`! ... power_cap began` and `power_cap ended` under the money. What was unusual on 2026-10-18 was the limit
**below the default** (`400/575W`) together with a clock far under the maximum: compare the limit with the
default, and the clock with its maximum, not the word alone. Second, a load that starts and stops, or a cap that
is intermittent, makes the watch say `began` and `ended` at each flip: that is what the card did. And the **same cap does not mean the same speed**: on those two boxes both were at
`400/575W`, one ran at about 1,700 MHz and the other at about 2,100 (different silicon, cooling and
workload: the cause was not determined). The limit alone does not tell the speed; the clock history does.

**Someone else on the GPU.** On one of those boxes the card held 24,809 MiB while the only process of the
box, the training, held 8,720: about 16 GiB belonged to something outside the container, and in ten one-second
`nvidia-smi pmon` samples the training got 24 to 82 % of the card's compute (about 50 % on average) while
the card read 100 %. Its twin, on another machine
with the same workload, added up to the MiB. `vas --ssh` shows this as the `OTHER` column: the device's
memory in use minus what the compute processes the box lists hold (`15.7G`), or `-` when nothing notable is
held (under 1 GiB: the driver's own few MiB) or when the box cannot say. Under `--watch`, a line says when it
appears or goes:

```text
! 56533728 gpu 0: memory held outside the container 0.0 -> 15.7 GiB
```

`vas` cannot name the process: it is outside the box. A GPU with **no** process listed shows `-`, not "all
of it is someone else's": a run that has just restarted lists none while the memory is still its own.

### What should I do about it?

You decide; `vas` only shows it. What was done in the case above: the figures were gathered (`nvidia-smi` on
the box, and `lsof /dev/nvidia*` showing that nothing of the box's own holds the memory), and reported to
vast.ai support with the instance id, the machine id and the host id (`vas --wide` shows the last two). The
outcome is not recorded here. You can also keep that machine out of your next search with `vas offers
--exclude-machine ID`; whether to ask for a credit or a move is between you and vast.ai.

### `OTHER` shows `-` but I am sure someone is there

`-` means "nothing notable, or unknown". The memory held must reach 1 GiB, the box must have listed the
processes (an old driver, or `[N/A]` figures, give no reading), and at least one process must be listed on
that GPU. A GPU that cannot be read whole by the main query (one `[N/A]` among its fields, as with MIG
slices) is left out of the table, not shown half-read.

## Rate limits and watching

### vast.ai says I am rate-limited

vast.ai budgets each endpoint's calls per window and says so in `X-RateLimit-*` headers (seen on
2026-10-18: 30 calls for the offer search, and **one** for the instances list, whose window was about three
seconds). `vas` reads them. A look refused with a wait of 15 seconds or less is waited out once, aloud
(`vas: rate-limited: waiting 3 s, then trying again`); a longer wait is named and ends the run (`vast.ai
rate-limited /api/v1/instances/: try again in 120 s`). An account with more than 25 instances needs a
second page of the list, and `vas` waits for the budget between the pages. A `--watch` never looks again
before its budget has renewed, and never waits more than 15 minutes for it.

### Why can't `--watch` go faster than every 10 s (every 20 s for `vas offers`)?

A `--watch` under the floor is raised to it, and `vas` says so on stderr. The floor is a courtesy to the
service (vast.ai's own limits are obeyed whatever it says), and a table every few seconds scrolls past
unread in a terminal. For a person watching offers, `--changes` (one line per look and per change) reads
best; for a script, `vas --compact --watch N` prints JSON Lines. A screen that redraws in place, and
faster offers, are waiting for a text interface (see the [roadmap](roadmap-v0.2.0.md)).

### Why is nothing marked in the first looks of `vas offers --watch`?

It is learning the market: nothing is marked or reported before the fourth look (see
[above](#why-does-the-list-change-from-one-search-to-the-next)). Each look still prints: the table, or with
`--changes` a line saying it is learning (`searched 13:01:30 UTC: 33 offers (learning the market: changes are
reported from look 4)`). With the 20-second floor, the learning takes about a minute.

## Windows, Linux, macOS

### `vas --watch 30 | Select-Object -First 3` ends by itself?

Yes: when the reader of the pipe goes away, `vas` notices at its next write and ends quietly (exit `0`): a
closed pipe is tested, and this exact command was observed to end the same way on 2026-10-18. It does not run
on in the background.

### Does PowerShell's `>` break my config file?

No: a config file may be UTF-8 (with or without a byte-order mark) or UTF-16 with a mark, like the key file.
A line that is not a flag stops the run and names the file and the line.
