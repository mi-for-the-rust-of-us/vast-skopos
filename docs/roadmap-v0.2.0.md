# Roadmap: what 0.2.0 is, and what may come next

**Not affiliated with vast.ai.** This page says what `vas` 0.2.0 contains, what it deliberately does not,
and which ideas are on the table for later. The later part is a list of **candidates, not promises**:
each one says why it is wanted and what is still open. The authority for what shipped is the
[CHANGELOG](../CHANGELOG.md).

## What 0.2.0 contains

0.2.0 grew out of real rentals (the dogfooding notes in the repository record each one). In the order a
rental goes:

**Choosing a box**
- `vas offers --watch` reads the market **by machine** over several looks, with a `SEEN` column, because
  vast.ai's search answers a flickering sample, not the whole market.
- `--exclude-machine` beside `--exclude-host`; `--wide` shows your boxes' machine and host ids.
- Every search ends with a line saying that vast.ai's answers vary.

**Watching a box**
- `POWER`, `CLOCK` and `OTHER` columns under `--ssh`: the power limit and what holds the clocks down, the
  SM clock, and GPU memory held by processes outside your container.
- Under `--watch`: `TREND` and `CLOCK TREND` (the last six samples), and a line when a power limit, a
  throttle reason or an outside user of the GPU appears or goes.
- `--credit-log FILE`: one CSV row per look, so the credit becomes a curve.
- `--compact`: the same readings as JSON Lines for scripts and monitors, with an `event` object for each
  change.
- The burn counts boxes meant to run that read `offline` or `loading`, and names them.

**Playing fair with vast.ai**
- vast.ai's rate limits are read from its headers and obeyed: a short wait is sat out once, aloud; a long
  one is named; a watch never looks before its budget has renewed. The library reports, and never sleeps.
- `--watch` floors: 10 s for the lookout, 20 s for `vas offers`.

**Robustness**: two review rounds and a five-part audit before the release found and fixed many edge cases
(the CHANGELOG lists them); every fix has a test that fails without it.

For library users 0.2.0 is a breaking release (the 0.x convention): the changes are listed as BREAKING in the
CHANGELOG.

## What 0.2.0 deliberately does not do

- **It does not rent, start, stop or destroy anything**, and will not: `vas` issues only `GET` requests. A
  monitor that can spend money or destroy work is a different tool with a different review.
- **It does not name the process** that uses your GPU from outside the container: it cannot see it. It
  shows that something is there, and how much.
- **It is not a text interface yet.** A table every few seconds in a terminal scrolls past unread, which is
  why `--watch` has a floor and why `--changes` exists.

## Known limits

- **Why the search flickers is not known.** A guess (not a finding): a machine is listed only while its
  host's report is fresh. The watch is built to be honest about the flicker, not to explain it.
- **An empty search answer counts as a look.** Five empty answers in a row would report every machine as
  gone, then every machine as new. That is consistent, and would be noisy if vast.ai answered `[]` under load.
- **`COST~` is an estimate**, and vast.ai bills the storage of stopped boxes, which `vas` does not show.
- **A cap, or an outside user, is set or caused outside `vas`, which cannot fix it or say by whom.** It shows
  it, and gives you the figures to report it.
- **`--ssh` needs `nvidia-smi` on the box**, your key accepted by it, and `ssh` on your `PATH`.

## Candidates for later

Not a priority order, and none promised.

1. **A text interface for the watch.** Redrawn in place instead of scrolling; colour that means something
   (`+` new, a price down in green and up in red); rows that linger for a few looks after an offer vanishes,
   because the point of watching is to see what you just missed; keys to pause and to follow an offer; the
   GPU alerts staying visible beside the latest state. *Open:* whether it belongs in `vas` behind a cargo
   feature (to keep the default install small) or in a sibling binary, and how fast it may poll: a 5 s
   refresh of `vas offers` may be allowed once there is a screen that can show it, because showing
   real-time, meaningful changes is a display problem before it is a polling one.
2. **A quiet watch.** Print only on change, redrawn in place: the terminal's answer without a full
   interface. *Open:* it has to keep the `!` lines of the GPU alerts visible.
3. **A measured upload (`vas check`).** A two-size measure of a box's real uplink, since the advertised
   figure is the host's own test. *Open:* it needs somewhere to send bytes without a write token; a
   download-only first version is possible.
4. **The storage cost of stopped boxes.** vast.ai bills it, `vas` does not show it. *Open:* it needs the
   field name from the raw row of a stopped box.
5. **A process's share of the GPU against the card's utilisation.** The strongest evidence of an outside
   user so far was a training getting 24-82 % of the compute while the card read 100 %. It needs a few
   seconds of sampling per look, so it is not in the probe.
6. **For library users: which helpers are promised** (`probe::strip`, `render::age`, ...) and which stay
   hidden (un-hiding later is free, hiding later is a break), and a way to build a `GpuReading` outside the
   crate, so that downstream tests can drive a trend.

## How to influence it

The roadmap comes from real rentals. If something bit you on yours, [open an issue](https://github.com/mi-for-the-rust-of-us/vast-skopos/issues)
with the figures: the command, the output, and what you expected.
