# Roadmap: what 0.3.0 is, and what may come next

**Not affiliated with vast.ai.** This page says what `vas` 0.3.0 contains, what it deliberately does not,
and which ideas are on the table for later. The later part is a list of **candidates, not promises**:
each one says why it is wanted and what is still open. The authority for what shipped is the
[CHANGELOG](../CHANGELOG.md); [the 0.2.0 roadmap](roadmap-v0.2.0.md) is kept as it was written.

## What 0.3.0 contains

0.3.0 answers the questions the rentals of 2026-10-22 to 10-26 kept asking: wait for this box, or destroy
it? Is this offer the machine we had? Did the run print its line yet? The
[2026-10-23 report](https://github.com/mi-for-the-rust-of-us/vast-skopos/blob/main/docs/dogfooding/2026-10-23.md) lists the notes it comes from, and the
[2026-10-26 report](https://github.com/mi-for-the-rust-of-us/vast-skopos/blob/main/docs/dogfooding/2026-10-26.md) its check on two rented boxes. In the order a rental goes:

**Choosing a box**
- **`--exclude-deverified`**, beside `--verified`: it leaves out the machines vast.ai has deverified and keeps
  the unverified ones (new, or under evaluation), which `--verified` hides too.
- **`--host ID` and `--machine ID`** keep only a host's or a machine's offers.
- **`--wide`** shows what tells one machine from another: `MACHINE`, `CPU`, `DRIVER`, `DLPERF`, `DLPERF/$`,
  `PCIE GB/s` and `LIMIT W`, the power limit the listing gives. `vas --wide` shows the same `CPU`, `DRIVER`,
  `DLPERF` and `PCIE GB/s` for a rented box.

**Waiting for a box**
- **vast.ai's message for a box that is not running**, on a line under the table:
  `57340012 loading: 46b4edaa8d47: Pull complete` is a box progressing, not a stuck one.
- **A failed probe says which way it failed, and how old the box is**: `UNREACHABLE 4m: …` when `ssh` never
  reached it, `SSH FAILED 4m: …` when it did and was turned away.

**Waiting for a run**
- **`--grep PATTERN`** shows the last line of the log that matches, not the last line.
- **`--until PATTERN`**, with `--watch`, ends the watch when a box's log has a matching line. Its exit codes
  are made for a script: 0 a match, 2 a pattern the box's `grep` refuses, 3 no box readable for 2 looks, 1
  the first look failed or stdout was closed.

**Robustness**: two independent reviews before the release (14 findings, then 7, all fixed), each new test
shown to fail when its feature is broken, CI on Linux, macOS and Windows (MSRV 1.88 and stable), and two live
checks on rented RTX 5090s.

For library users 0.3.0 is a breaking release (the 0.x convention): the changes are listed as BREAKING in the
CHANGELOG.

## What 0.3.0 deliberately does not do

- **It does not rent, start, stop or destroy anything**, and will not: `vas` issues only `GET` requests.
  `UNREACHABLE` and the status line help you decide; the dashboard or `vastai` acts.
- **It does not mark an offer as "likely capped".** DLPerf saw some power limits and missed others, and
  `LIMIT W` matched a host's cap on one host only: neither is yet a rule. The card's own reading, `vas --ssh`'s
  `POWER`, is the one that says what a run gets.
- **It does not retry.** `--until` waits by looking again, every `--watch` seconds, each look one attempt.

## Known limits

- **Not yet seen live:** the status line under the table, and `UNREACHABLE` / `SSH FAILED` on a box that
  really refuses. Both are tested, but both checks on real boxes came when the boxes already ran and
  answered. The `!` line of a power change in a watch (0.2.0) has also only been seen on test data.
- **On some `ssh` builds a refused port reads `SSH FAILED …: no GPU reading (ssh exit 255)`**, not
  `UNREACHABLE`: GitHub's Windows runner's `ssh` exits 255 and says nothing, and `vas` does not guess.
- **A board limited in its firmware reads as uncapped while it is idle** (`450W`, no default after it). Under
  load it shows (`+power_cap` with a clock below its peers'); before the run, compare the number with the
  model's reference (575 W for an RTX 5090).
- **`--until` ends on the first box that matches**; there is no flag to wait on one box only.
- **`--grep` searches the last 64 MiB of a file** and each line's first 4096 bytes, and leaves empty lines out.
- And, from 0.2.0: why the search flickers is not known; an empty search answer counts as a look; `COST~` is
  an estimate, and the storage of stopped boxes is not shown.

## Candidates for later

Not a priority order, and none promised. *Since written (2026-10-28):* candidates 1, 2, 3 and 5, and the
library half of 8, are built for the next version, and its CHANGELOG's Unreleased section says how; 4 has a
second host's agreement (2026-10-28).

1. **The direct route (note 1b).** The instance row carries it: `public_ipaddr` and the host port mapped to 22
   (`ports["22/tcp"]`). Show it, and probe it when the proxy refuses. *Open:* on 2026-10-26 it was open on one
   box and refused on the other (a host behind a home router), so a refused direct route must not read as a
   dead box.
2. **A reference board power per GPU model** (RTX 5090: 575 W, NVIDIA's figure), to mark a card whose maximum
   (`power.max_limit`, not yet in the probe's query) is below it: `450W<575`. *Open:* the table of models,
   and what to do with add-in boards above the reference (`500/600W`).
3. **An idle counter told apart from a busy card.** A card has read `100%` with no memory held and no process
   three times; it could print `100%?`, or a note under the table. *Open:* whether the probe's memory figure
   is enough, or the process list is needed.
4. **`LIMIT W` as an early warning**, if it keeps matching the cards' caps on more hosts than 406325.
5. **`--until` on one box** (an `--instance ID`), for runs that share a log path.
6. **A history of the boxes seen** (note 3): instance, machine and host, first and last status, kept without
   anyone having to remember to run `--wide`. *Open:* it would be the second file `vas` writes, a change of
   rule with its own review.
7. **A count of the offers a filter hid** (note 6): `3 deverified offers hidden`. *Open:* the count needs the
   unfiltered answer, which a server-side filter does not give.
8. **Still open from 0.2.0:** a text interface for the watch, a quiet watch, a measured upload (`vas check`),
   the storage cost of stopped boxes, a process's share of the GPU against the card's utilisation, and, for
   library users, which helpers are promised and a way to build a `GpuReading` outside the crate. See
   [the 0.2.0 roadmap](roadmap-v0.2.0.md) for what each needs.

## How to influence it

The roadmap comes from real rentals. If something bit you on yours, [open an issue](https://github.com/mi-for-the-rust-of-us/vast-skopos/issues)
with the figures: the command, the output, and what you expected.
