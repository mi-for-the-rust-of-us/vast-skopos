# From filters to a running box

A walk through one rental with `vas` (vast-skopos 0.3): you write down what you want, search the market
without a key, watch it for a few minutes, rent a box **outside `vas`**, then watch the box until you are
done with it. Steps 1 to 5 take about twenty minutes; steps 7 to 11 last as long as your rental.

**Not affiliated with vast.ai.** `vas` is read-only: it never rents, starts, stops or destroys anything.
Renting (step 6) happens in the vast.ai console or with the official `vastai` CLI, and **that step starts
billing**.

A few words used below:

- **a look**: one refresh of what `vas` shows (one search, or one look at your boxes);
- **the lookout**: `vas` on its own, which watches the boxes you already rent (`vas offers` searches the market);
- **burn**: what your running boxes cost per hour, added up; the credit divided by it is how many hours you
  have left;
- **host and machine**: a host is whoever owns the hardware and may own several machines.

About the examples: they are output from real rentals, trimmed, except where a note says otherwise. Commands
have a PowerShell spelling where it differs.

## 0. What you need

- **Rust**, to install `vas`: `cargo` is Rust's package manager (get it from <https://rustup.rs>). There are no
  pre-built binaries.
- For steps 1 to 5, nothing else: the offer search needs **no account and no key**.
- For steps 6 to 11: a vast.ai account with some credit, an API key (step 7), the `ssh` program of your system
  (`ssh -V` prints its version; recent Windows ships one), and your **public ssh key added to your vast.ai
  account** (in the console's account settings; see vast.ai's documentation). Test that a plain `ssh` to a
  box works before blaming `vas`.

## 1. Install

```sh
cargo install vast-skopos --locked
vas --help
```

`vas --help` lists the flags of the lookout; `vas offers --help` lists those of the offer search. To update,
run `cargo install vast-skopos --locked --force`; to remove it, `cargo uninstall vast-skopos` (your key file and your
filters file are not touched). `vas` prints what you asked for on **stdout** and its notices (a wait, a
warning, an error) on **stderr**.

## 2. Say what you want, once

You will search many times; write the filters down once. Create the folder and the file
`~/.config/vast-skopos/offers` (on Windows, `C:\Users\YOU\.config\vast-skopos\offers`; or set `XDG_CONFIG_HOME`):

```sh
mkdir -p ~/.config/vast-skopos                                  # bash
New-Item -ItemType Directory -Force ~/.config/vast-skopos       # PowerShell
```

One flag per line:

```text
# my renting filters
--gpu RTX_5090
--num-gpus 1
--disk 50
--reliability 0.99
--min-days 2
# driver >= 580, which vast.ai shows as CUDA 13
--cuda 13
--inet-up 200
--exclude-country CN
--exclude-deverified
```

What each line asks:

| Flag | Meaning |
|---|---|
| `--gpu RTX_5090` | the GPU model as vast.ai names it (an underscore reads as a space) |
| `--num-gpus 1` | one GPU on the box |
| `--disk 50` | 50 GB of disk wanted: it is **priced into `$/h`** and required free |
| `--reliability 0.99` | the host's reliability, from 0 to 1 (`99` is refused, to save you a mistake) |
| `--min-days 2` | the offer must allow at least a two-day rental |
| `--cuda 13` | the host's driver must support CUDA 13 |
| `--inet-up 200` | at least 200 Mbit/s of **advertised** upload (the host's own test, not your route) |
| `--exclude-country CN` | leave a country out |
| `--exclude-deverified` | leave out the machines vast.ai has deverified (verified once, then found failing its checks), and keep the unverified ones, which are new or under evaluation; `--verified` would hide those too ([FAQ](../FAQ.md#verified-unverified-deverified-which-should-i-filter)) |

The file is read first; the command line adds to it or overrides it. [README](../../README.md#vas-offers) lists
every flag.

## 3. Search

```sh
vas offers --top 20
```

```text
ID        GPU          VRAM  $/H    CUDA  DISK   UP/DOWN Mb/s  REL %  MAX DAYS  HOST    VERIFICATION  WHERE
52190788  1x RTX 5090  32G   0.516  13.0  543G   551/618       99.75  13.1      406325  verified      South Korea, KR
48215695  1x RTX 5090  32G   0.587  13.2  577G   718/766       99.25  104.8     403004  verified      South Korea, KR
47558703  1x RTX 5090  32G   0.610  13.0  365G   906/774       99.21  117.1     229807  verified      Estonia, EE
40077669  1x RTX 5090  32G   0.685  13.2  650G   768/841       99.93  30.0      269644  verified      Bulgaria, BG
52803262  1x RTX 5090  32G   0.689  13.2  170G   924/928       99.20  69.7      691937  verified      Georgia, US
...
20 of 33 offers shown, cheapest first | RTX 5090, 1 GPU, disk >= 50 GB, CUDA >= 13, reliability >= 99 %, up >= 200 Mb/s, >= 2 days, verified, not in CN | $/h includes 50 GB of storage | searched 13:01:30 UTC
vast.ai's answers vary from one search to the next; repeat the search, or use --watch, to see more.
```

(Five of the twenty rows, from a search on 2026-10-19, with `--verified` in the file at the time.) How to
read it:

- **`ID`** is the offer id: what you rent. **`HOST`** is the host that owns the hardware.
- **`$/H` includes the storage you asked for**; the footer says how much (here 50 GB).
- **`MAX DAYS`** is the longest rental the host allows; **`UP/DOWN`** is what the host advertises, not what you
  will get.
- **The footer** names every constraint applied: that is how you check that your file was read. It also
  gives the time of the search, and two honest notes: vast.ai answers at most **64 rows** per search (it says
  so when it does: narrow the search), and its answers **vary** from one search to the next.
- If nothing matches, `vas` prints `(no offers)` and the footer. If it says there is no constraint, your file
  was not found.

Every constraint is sent to the server *and* re-checked on each row. A row that is missing a field is kept,
with a dash, rather than hidden.

## 4. Watch the market for a few minutes

The market moves, and one search is a sample of it, not all of it: fifteen identical searches 2.5 s apart,
measured, saw 129 machines and only 30 of them in every answer. So watch instead of searching once:

```sh
vas offers --watch 30 --changes
```

`--watch 30` searches again every 30 seconds (at least 20: a faster request is raised, and said). `--changes`
prints one line per look and one per change, which reads better in a terminal than a whole table every thirty
seconds. The first three looks only **learn** the market, so at first the lines say so and nothing is reported:

```text
searched 13:01:30 UTC: 33 offers (learning the market: changes are reported from look 4)
```

From the fourth look on, a line names what changed (`+` a new machine, `~` a price that moved, `-` a machine
that is gone). Without `--changes`, each look is the whole table with two more columns: a mark column, and
**`SEEN`**, which says in how many looks each machine appeared: `3/9` means it showed up in 3 of the 9 looks
since you first saw it, `15/15` in every one. Press Ctrl+C when you have seen enough. The exact rules, and why
a plain comparison of two searches would mislead, are in the
[FAQ](../FAQ.md#why-does-the-list-change-from-one-search-to-the-next).

## 5. Choose, and keep the ones you do not want out

Choose with the whole row, not the price alone: a low `REL %`, a short `MAX DAYS` or a modest `UP/DOWN` can
be the trade-off for a low price, and the right trade-off depends on your run.

To keep a host out of every later search, use the id from the `HOST` column; to keep one machine out, use its
machine id, which `vas --wide` shows for the boxes you have rented:

```sh
vas offers --exclude-host 406325
vas offers --exclude-machine 123456
```

(Both can be repeated, and they add to the lists in your file.)

One host may list several machines that look alike in the table. `--wide` adds what tells them apart, the
machine id, the CPU and the driver, with vast.ai's benchmark score (`DLPERF`, and per dollar) and the power
limit the listing gives (`LIMIT W`); `--host ID` or `--machine ID` keeps only one of them:

```sh
vas offers --wide --host 406325
```

(A listed figure is not a reading of the card: step 8 reads the card itself. `LIMIT W` did match the cap of one
host, 406325, on two days; whether it does for others is not known yet.)

## 6. Rent it, outside `vas`

**This step starts billing.** Check the price, the disk size and the image first.

Take the `ID` of the offer to the vast.ai console, or to the official CLI: `vastai create instance ID` with an
image and a disk size (run `vastai create instance --help` for its options). A minute or so later the box
appears in `vas`, typically `loading` and then `running`. `vas` cannot do any of this, by design.

While a box is not running, `vas` prints vast.ai's own message for it under the table:
`57340012 loading: 46b4edaa8d47: Pull complete` is the image arriving, layer by layer: wait for it.

## 7. Give `vas` a key

The lookout needs your API key. Create one in the vast.ai console with the least it needs: **User Read and
Instances Read**, nothing else. `vas` finds it where the `vastai` CLI keeps it: the file
`~/.config/vastai/vast_api_key` (one line, the key) or the `VAST_API_KEY` environment variable, or you name a
file with `--key-file FILE`. There is no `--key` flag, on purpose: a key typed on the command line lands in
your shell history.

Prefer the file. Typing `export VAST_API_KEY=...` puts the key in your shell's history too (and in PowerShell,
`$env:VAST_API_KEY = "..."` lasts for that window only). Then:

```sh
vas
```

```text
ID        LABEL  GPU          WHERE              STATUS   $/H    AGE     COST~  SSH
56533728  -      1x RTX 5090  United States, US  running  0.491  4h 26m  2.18   ssh5.vast.ai:24016
credit $9.33 | burn $0.491/h over 1 running | ~19.0 h left at this rate
```

(One box of a real rental; the money line is recomputed for that one box.) `COST~` is an estimate (today's
`$/h` times the age; the invoice is the reference). The last line is the number to watch: your credit, the
burn, and the hours the credit lasts at that burn. A box you asked to run that reads `offline` or `loading`
is counted in the burn and named, because whether it bills is not known from the API. If the money line shows
`credit ?`, the account request failed and `vas` said why on stderr.

## 8. Ask the box itself

The dashboard's GPU figure is not trusted (it has read 0 % on boxes at 97-99 %): `--ssh` asks each running box,
over your system `ssh`, once per look, what its GPUs are doing; `--log` adds the last line of the newest file
matching a path on the box. Quote the glob so that the box's shell, not yours, expands it:

```sh
vas --ssh --log '/root/runs/*/train.log'
```

`vas` connects as `root` to the address in the `SSH` column, never retries, and only **reads**: `nvidia-smi`
queries and, with `--log`, `ls` and `tail` on your log file. Add `-i FILE` to name a private key.

A healthy row looks like this (illustrative: a card at its default limit, free of other users):

```text
GPU% (ssh)  VRAM       OTHER  POWER  CLOCK      LAST LOG / SSH ERROR
98%         29.4/32G   -      575W   2900/3090  s0/train.log: e41 loss 0.3
```

- **`GPU% (ssh)`, `VRAM`**: utilisation and memory, as `nvidia-smi` reports them on the box.
- **`OTHER`**: GPU memory held by processes **outside your container**; `-` is the healthy answer.
- **`POWER`**: the power limit set on the card (`575W` is an RTX 5090 at its default). When the limit is
  lower than the default, the default follows (`400/575W`). `+power_cap` says the card is at its power limit
  right now, which every card is under full load: it is a limit **below the default** and a clock far under
  its maximum that point at a setting on the box. A board limited in its own firmware shows no default,
  but `vas` marks it with its model's reference: `450W<575` is an RTX 5090 board that cannot go above 450 W,
  against the model's 575 ([FAQ](../FAQ.md#the-gpu-reads-100--but-the-run-is-slower-than-it-should-be)).
- **`GPU% (ssh)` on a box nobody uses yet** can read `100%`: an idle counter. `vas` marks it `100%?`, with a
  line under the table, when the card holds no memory and lists no process: nobody is on the card.
- **`CLOCK`**: the speed of the GPU's compute cores (the SM clock) now, over the highest it reaches, in MHz.
- **`LAST LOG`**: the newest file matching `--log`, named by its last two path components, and its last
  non-blank line (`(no file matches)`, `(not a file)`, `(empty)` or `?` otherwise).

If the last column shows an error instead, it says which way the probe failed, how old the box is, and
`ssh`'s own words. `UNREACHABLE 4m: ...` means `ssh` never reached the box (refused, timed out): a box of a
minute or two may still be starting, so look again; one that still refuses after several minutes may never
come up. `SSH FAILED 4m: ...` means the box answered and turned `ssh` away: a refused key (your public key is
not on your account) is fixed on your side, not by waiting. `vas` retries nothing. More in the
[FAQ](../FAQ.md#a-box-reads-unreachable-should-i-wait-or-destroy-it).

Run `vas --wide` once on every box you rent, even one you destroy at once: it shows the box's `MACHINE` (what
`--exclude-machine` takes) and the `CPU`, `DRIVER`, `DLPERF` and `PCIE GB/s` that tell it from its host's other
machines.

## 9. Keep watching, and keep the credit curve

```sh
vas --ssh --log '/root/runs/*/train.log' --watch 60 --credit-log credit.csv
```

Every 60 seconds (at least 10) the table is printed again, with two more columns, `TREND` and `CLOCK TREND`
(the last six utilisation and clock samples of each box, oldest first: `99 99 98 0 0` may be a run that
stalled, or one that is restarting), and **lines under the money** when something changes between two looks.
Their format, as the code prints it (not captured live):

```text
! 56533728 gpu 0: power limit 575 -> 400 W, power_cap began
! 56533728 gpu 0: memory held outside the container 0.0 -> 15.7 GiB
```

`--credit-log credit.csv` appends one row per look (`unix_time,credit,burn_per_hour,running,unsure`), so the
credit becomes a curve you can plot or compare with the burn. It is the only file `vas` itself ever writes,
and only because you named it. Stop with Ctrl+C. (For a script or a monitor, `--compact` prints the same
readings as JSON Lines: see the [README](../../README.md#vas---compact-json-lines).)

To wait for one line of the log (the first loss of a stage, say) rather than watch, see
[Waiting for a line](waiting-for-a-line.md): `--until` ends the watch when the line appears.

## 10. When something looks wrong

- **The GPU reads 100 % but the run is slow.** Look at `POWER` and `CLOCK`: a limit below the default, a
  number below the model's reference (a board limited in its firmware), or a thermal reason, with a clock far
  under its maximum (by whom, and why, is not something `vas` can tell), and the clock history shows how much it
  costs you. Real example, power-capped boxes: [FAQ](../FAQ.md#the-gpu-reads-100--but-the-run-is-slower-than-it-should-be).
- **`OTHER` shows gigabytes.** Something outside your container is using the card (probably another user of
  the machine); `vas` cannot name it. The [FAQ](../FAQ.md#what-should-i-do-about-it) says what to gather and
  who to ask.
- **A box shows `UNREACHABLE` or `SSH FAILED`.** See step 8.
- **A box stays `loading`.** Read vast.ai's message under the table (step 6): a message that moves is
  progress.
- **The credit runs down faster than the burn says.** Compare the credit log's fall with `burn_per_hour`
  ([FAQ](../FAQ.md#what-does---credit-log-write)).

## 11. Finish

**Copy your results off the box first.** Then stop or destroy it in the console or with `vastai`; `vas`
will not. They are not the same: destroying deletes the box's disk and everything on it, while stopping keeps
the disk, and vast.ai bills the storage of a stopped box. `vas` then shows the box as `exited`, or no longer
lists it. The last rows of your credit log show how the credit fell while `vas` was watching: that is not an
invoice.

## Where next

- [Waiting for a line](waiting-for-a-line.md): `--grep` and `--until`, instead of a loop of your own.
- [FAQ](../FAQ.md): the questions that came up on real rentals.
- [README](../../README.md): every flag, the JSON format, the key and file rules.
- [`vas` and `vastai`](../comparison-with-vastai.md): which tool for which job.
- [Roadmap](../roadmap-v0.3.0.md): what 0.3.0 contains and what may come after.
