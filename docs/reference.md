# The `vas` reference

**Not affiliated with vast.ai.** Every flag of `vas` and `vas offers`, every column and mark they print,
`--compact`'s JSON Lines, the offers' config file and their watch, in full. The [README](../README.md) has
the quick start; `vas -h` prints the flags in one line each, and `vas --help` adds the detail, examples, how
to read the table, the exit codes and the environment variables; the [FAQ](FAQ.md) answers the questions real
rentals asked.

- [Binary (`vas`)](#binary-vas): the flags, `--ssh` and its columns (`GPU% (ssh)`, `VRAM`, `OTHER`, `POWER`,
  `CLOCK`, the trends, the log), the status line, the money line, rate limits, the credit history
- [`vas --compact`: JSON Lines](#vas---compact-json-lines)
- [`vas offers`](#vas-offers): the flags, the config file, the footer, which machine is which, verified or not
  - [Watching the offers](#watching-the-offers)

## Binary (`vas`)

| | |
|---|---|
| `vas` | the table and the money line |
| `vas --ssh` | also probe every running box over `ssh`, once each, in parallel |
| `vas --ssh --log PATH_OR_GLOB` | also print, for the newest match of the pattern on each box, its last two path components and its last non-blank line (`s0/train.log: ...`, so a glob whose varying part is among the last two components says which run wrote it); `(empty)` when the file has no line to show, `(not a file)` when the newest match is a directory, `(no file matches)` when nothing matched, `?` when the box's answer was cut short. The pattern is expanded by the box's shell, so quote it locally (and quote a part with spaces again inside, `'"/root/my runs"/*/train.log'`); the line is shown without control characters, at most 120 characters, the path at most 60 |
| `vas --ssh --log PATH_OR_GLOB` from Git Bash | Git Bash rewrites an argument that looks like a POSIX path before a Windows program sees it: `/root/runs/s1/run.log` arrives as `C:/Program Files/Git/root/runs/s1/run.log`. `vas` reads it as typed again, and says so on stderr; a path that starts with another drive (`/tmp/...`, mapped to your temp folder) is refused with exit 2: write it `//tmp/...`, or set `MSYS_NO_PATHCONV=1`. A path with a `*` is not rewritten |
| `vas --ssh --log PATH_OR_GLOB --grep PATTERN` | the last line MATCHING `PATTERN` (an extended regular expression, `grep -E`, case-sensitive) instead of the last line: `--grep 'initial loss\|Traceback'` catches a line printed once and buried by the lines after it. Searched in the file's last 64 MiB within 10 s (`?` when the search runs past them, or the file cannot be read), each carriage-return segment of a progress bar a line of its own, empty lines left out, and each line's first 4096 bytes (a line is shown from its head, cut at 120 characters); `(no line matches)` when none does, `(grep refused the pattern)` when `grep` cannot read it, `(a blank line)` when the line that matched is made of spaces only. The pattern is quoted for the box's shell: nothing in it is expanded |
| `vas --ssh --log PATH_OR_GLOB --watch 30 --until PATTERN` | wait for a line matching `PATTERN` (read as `--grep` reads it, which it replaces) on any box, then exit `0` and say the line on stderr (`vas: --until: box 57641653 matched: ...`): a script can wait on a run without a hand-written `ssh` loop. It exits `2` at once when a box's `grep` refuses the pattern and no other box was read (no look could match), `3` when no box could be read for 2 looks in a row (every probe failed or gave no readable log, `?`, or none could be made) or the boxes answered but none has had a file matching the `--log` path for `--log-wait` seconds (a warning on stderr after 2 looks: is the path right?), and `1` when the first look fails or its stdout is closed before a match (`vas ... \| head` is not a match); a box meant to run but not running (`loading`, say) keeps the wait going for 10 looks at most since a box was last read, and a look whose instances request failed does not count. No time limit on the whole wait (`--log-wait` bounds only a missing file): `timeout 3600 vas ...` gives one |
| `vas ... --until PATTERN --log-wait SECONDS` | how long a wait goes on while the boxes answer but none has a file matching the `--log` path, counted from the first look that found none: 900 (15 minutes) by default. A run may write its log late (a setup first, say); a wrong path must not keep a script waiting while the boxes cost. The time runs while any box answers without the file and none is read with it, whatever the other boxes say (`loading`, `?`, a failed `ssh`, a refused pattern); a box read with the file starts it afresh |
| `vas --ssh -i ~/.ssh/id_ed25519` | use this private key (default: `ssh`'s own choice, agent and `~/.ssh/config`) |
| `vas --wide` | also show each box's direct `ssh` address past the proxy (`DIRECT`: its public IP and the host port mapped to its port 22, from the API's `public_ipaddr` and `ports`), its machine id and host id (what a rentals log, `--exclude-machine` and `--exclude-host` need), then its CPU, driver, DLPerf and PCIe bandwidth (`CPU`, `DRIVER`, `DLPERF`, `PCIE GB/s`: what tells it from another machine of its host, as `vas offers --wide` shows them) |
| `vas --compact` | JSON Lines instead of the table, for scripts and monitors ([below](#vas---compact-json-lines)); not with `--wide` |
| `vas --watch 60` | refresh every 60 s (at least 10) until Ctrl+C |
| `vas --instance ID` | only this box, repeatable: the rows, the probes, the trend and `--until` follow the boxes named. The money line stays the whole account's, since the credit is spent by every box, and says how many boxes are shown (`... \| 1 of 3 boxes shown`); `--credit-log` writes the whole account's money too. An id the account lacks is said on stderr, once, and again only when that changes |
| `vas --credit-log FILE` | append one CSV row per look to `FILE`: the credit as a curve ([below](#the-credit-history)) |
| `vas --key-file FILE` | read the key from `FILE` (otherwise `VAST_API_KEY`, then the `vastai` CLI's files: [the README](../README.md#the-api-key)) |
| `vas --skill` | print the skill for AI agents, and exit ([the README](../README.md#a-skill-for-ai-agents)); alone, with no other flag |
| `vas offers [FLAGS]` | the offers for rent that pass the flags, cheapest first; no key needed ([below](#vas-offers)) |

`--ssh` uses your system `ssh` with `BatchMode=yes`, `ConnectTimeout=10` and keep-alives, gives
each attempt 30 s in all (and `nvidia-smi` 20 s on the box), and makes **one attempt per route** per box per
refresh: vast.ai's proxy (the `SSH` column) and, only when the proxy never reached the box, the box's direct
address (`DIRECT` under `--wide`). A box that refuses is reported in its row, never retried in a loop.
Readings that came by the direct route say so (`(direct) s0/train.log: ...`); a box that neither route
reached says both reasons: `UNREACHABLE 4m: Connection refused (direct 79.117.26.24:20674: Connection
refused)`. A direct address can be closed by design (on 2026-10-26 one host sat behind a router that does
not forward it), so its refusal alone says nothing against the box. The row says what
happened, how old the box is, and `ssh`'s own last error line: `UNREACHABLE 4m: ssh: connect to host
ssh3.vast.ai port 32456: Connection refused` when `ssh` never reached the box (refused, timed out, no route,
a name that does not resolve, closed or reset before the session, nothing back within 30 s), `SSH FAILED 4m: ...`
when it did and was turned away (a refused key, a changed host key), gave no GPU reading, or began to answer
and did not finish within 30 s (a hung `nvidia-smi`). The age is what the choice
between waiting and destroying rests on: three rentals (2026-10-17, 10-18, 10-23) met boxes that read
`running` at 4 minutes and refused every connection, and were destroyed unused. Host keys are accepted on first use (`StrictHostKeyChecking=accept-new`: trust on first use, a changed key
is refused), since vast.ai's `ssh` proxies are reached by port. Each probe has an `ssh` session of its own
(`ControlMaster=no`, `ControlPath=none`), whatever `~/.ssh/config` says about shared connections.

`$/H` is the API's `dph_total`: the GPU and the box's disk, not its bandwidth (vast.ai's API reference
shows `dph_total` = `dph_base` + `storage_total_cost`, and a rental measured it:
[2026-11-08](dogfooding/2026-11-08.md#still-open)). `COST~` is an estimate: today's `$/h` times the
hours since creation. It counts too much for a box that was stopped a while (vast.ai then bills only its
storage), too little for one with much traffic (bandwidth is billed per byte), and prices can change; the
invoice is the reference.

**A box that is not running yet** says why in vast.ai's own words, on a line under the table (before the
money line): `57340012 loading: 46b4edaa8d47: Pull complete`, the message cut at 120 characters. On
2026-10-24 two boxes read `loading -> running` at 2 minutes of age while that message named the image's
layers arriving: progress, not a stuck box. A running box's message is not shown; `--compact` carries the
message of every box as `status_msg`.

**The probe columns.** Under `--ssh`, in order: `GPU% (ssh)`, `VRAM`, `OTHER`, `POWER`, `CLOCK`, then, under
`--watch`, `TREND` and `CLOCK TREND`, and `LAST LOG / SSH ERROR` last; each is explained below.

**An idle counter.** A card has read 100 % with almost no memory used and no process three times (2026-10-15,
10-19 and 10-26) while nothing held it. So a GPU that reads 99 % or more with under 1 GiB in use and no compute
process listed shows `100%?` in `GPU% (ssh)`, with a line under the table: `57787711 gpu 0: 100% with no memory
held and no process (an idle counter?)`. It is marked only when the process list came back whole: an unknown
list is not "no process". `--compact` carries each GPU's `processes` (`null` when unknown).

**The GPU trend.** One `nvidia-smi` sample is not a diagnosis (it has read 100 % with 0 MiB used while
nothing held the card). Under `vas --ssh --watch N`, the table gains a `TREND` column after `CLOCK`: the
last six samples of each box, oldest first, as the mean utilisation of its GPUs (`97 99 0 98 99 99`),
kept in memory for the length of the watch; `--compact` carries the same numbers as `probe.util_trend`. A
probe that failed adds nothing and keeps the history (one dropped `ssh` is not the run stopping), and a box
that stops or leaves is forgotten. A single look has no history, and no such column.

<a id="the-power-cap-and-the-clock"></a>

**The power cap.** A card's power limit can be set below what it ships with. Seen on 2026-10-18 on two
rented RTX 5090s: a limit of 400 W against the 575 W default, `SW Power Cap: Active`, and the SM clock held
at about 1,700 MHz of a possible 3,090: the run goes slower than the model suggests, and the dashboard's
utilisation (100 %) does not show it. Under `--ssh`, the `POWER` column (after `OTHER`) says the limit, the
default when the limit is lower, and what holds the clocks down: `400/575W+power_cap`; `575W` is a card at
its default. The words are `power_cap`, `thermal`, `slowdown` and `power_brake`; an idle GPU, application
clocks and sync boost are not restraints and are not shown. A board whose maximum (`power.max_limit`) is below
its model's reference board power reads `450W<575`. The board itself is then the cap, which neither its limit
nor its default shows, since both are 450 W (seen 2026-10-26). The references are NVIDIA's figures for the
RTX 5090 (575 W), 5080 (360), 4090 (450), 4080 (320) and 3090 (350); another model gets no mark.

**What `+power_cap` does and does not say.** It says the card is at its power limit, which **every card
reaches under full load**: on 2026-10-19 a default-limit RTX 4060 Ti (`165W`) read `165W+power_cap` and about
2,565 of 3,105 MHz as soon as a matrix-multiply loop ran on it, and went back to `165W` when the loop ended.
That is normal. What marked the 2026-10-18 boxes as unusual was the limit **below the default**
(`400/575W`) together with a clock far under the maximum: compare the two numbers and the clock, not the
word alone. `--compact` carries, per GPU,
`power_w` (the draw now), `power_limit_w`, `power_default_w`, `sm_mhz`, `sm_max_mhz` and `throttle` (a list
of those words: empty when nothing holds the card, `null` when the box did not say). The power is asked in a
second `nvidia-smi` query, so a driver that lacks a field costs the power cells (`-`) and nothing else; the
reasons that hold the clocks down are a third, asked as `clocks_event_reasons` and, failing that, as the older
`clocks_throttle_reasons`, so a driver that knows only one name still gives the power and the clock.
A GPU the main query cannot read whole (one `[N/A]` among its fields, as with MIG slices) is left out of the
table, not shown half-read.

<a id="someone-else-on-the-gpu"></a>

**Someone else on the GPU.** On 2026-10-18 a rented RTX 5090 held 24,809 MiB while its only visible process, the
training, held 8,720: about 16 GiB belonged to something outside the container (what, and why, was not determined:
another user of the card, or the host), which also took roughly half of the GPU's time (ten one-second `nvidia-smi pmon` samples gave the training 24-82 % of
the SM, while the card read 100 %). Its twin on another machine, same workload, added up to the MiB. Under
`--ssh`, the `OTHER` column (after `VRAM`) says that memory, per GPU (`15.7G`), or `-` when nothing notable is
held (the driver's own few MiB are not shown: the threshold is 1 GiB) or the box did not say. It is the
device's memory in use minus what the compute processes the box lists hold, so nothing is concluded unless
that list was read whole: a figure that is not a number, a missing bus id, or NO process listed on the GPU
gives `-` (a run that has just restarted, or a container that cannot see its own processes, lists none while
the memory is still its own; the gap is then unknown, not "all someone else's"). Under `--watch`, a
line says when it appears or goes: `! 56533728 gpu 0: memory held outside the container 0.0 -> 15.7 GiB`.
`--compact` carries the raw gap as `other_mem_mib` per GPU (`null` when unknown) and the change as an `event`
object (`other_mib_was`, `other_mib_now`). `vas` cannot name the process (it is outside the box): report the
machine to vast.ai with the figures (`vas offers --exclude-machine ID` keeps it out of the next search).

The clock itself is the `CLOCK` column (`1702/3090`: the SM clock now over the highest the card reaches, in
MHz), and under `--watch` a `CLOCK TREND` column after `TREND` keeps its last six samples, the mean of the box's
GPUs: a card that fell from 3000 to 1700 MHz reads `3000 2900 1700 1700 1700 1700`, while its utilisation
still reads `100`. `--compact` carries them as `probe.clock_trend` (`null` in a one-shot).

A card that reaches its limit and leaves it (a load that starts and stops, a cap that is intermittent) makes a
watch say `power_cap began` and `power_cap ended` at every look it flips: that is what the card did, and
the line is information, not an accusation (see above).

A cap can also arrive mid-run. Under `vas --ssh --watch N`, a line under the money says when a GPU's limit,
its throttle reasons or its other users change from one look to the next: `! 56533728 gpu 0: power limit 575 -> 400 W, power_cap
began` (and `... ended`). The first look of a box is the baseline, a reading the box did not give changes
nothing, and a box that stops starts afresh. `--compact` prints each change as an object before the money
one: `{"kind":"event","at":...,"id":...,"gpu":0,"limit_was":575,"limit_now":400,"began":["power_cap"],"ended":[]}`.

**vast.ai rate-limits its API**, per endpoint, and says so in `X-RateLimit-*` headers (on 2026-10-18: 30
calls for the offer search, ONE for the instances list). `vas` reads them. A look refused with a 429 that
clears within 15 s waits it out once and says so (`vas: rate-limited: waiting 3 s, then trying again`), so
two runs in a row do not fail; a longer wait is named and ends the run (`vast.ai rate-limited
/api/v1/instances/: try again in 120 s`); and a `--watch` whose budget is spent leaves at least until the
reset between two looks, whatever N says (`vast.ai's request budget renews in 5 s: the next look waits for
it`). The library only reports (`api::RateLimit`, `Client::budget_wait_secs`, `SkoposError::RateLimited`)
and never waits by itself.

**The burn counts what may bill.** The money line's burn adds the boxes the API reads as `running` AND
the boxes it was asked to run (`intended_status`) that read otherwise (`offline`, `loading`...):
`burn $1.940/h over 3 running + 1 offline (meant to run, billing unknown)`. Whether such a box bills
is not known from here, and a burn that leaves it out overstates the runway; the `~H h left` figure
uses the larger burn. A stopped box is not counted, though vast.ai still bills its storage: `vas`
does not show that cost yet.

<a id="the-credit-history"></a>

**The credit history.** `vas --credit-log credit.csv` appends one row per look to the file, so a watch (or a
cron job running `vas` every hour) leaves a curve of the account's money behind:

```text
unix_time,credit,burn_per_hour,running,unsure
1792328430,10.5100,0.4910,1,0
1792328460,10.5000,0.4910,1,0
```

The header is written only when the file is new or empty; the file is only ever appended to, so a log
survives restarts, and `credit` is empty when the account did not answer. `unsure` counts the boxes meant to
run that read otherwise, and the burn counts them, so compare the fall of the credit with `burn_per_hour`: a
fall well below it, while `unsure` is above zero, says those boxes do not bill; a fall above it says
something else does (vast.ai bills the storage of a stopped box, which `vas` does not count). A look whose
instances request failed writes no row. Nothing is
written unless you name the file, the API key is never in it, and a file that cannot be written is said
once on stderr (`failed to write the credit log ...`) while the look goes on and the log stops trying for
the rest of the run (on Windows, a file open in Excel is such a file: close it, then restart the watch).

## `vas --compact`: JSON Lines

One object per box, then (under `--ssh --watch`) one per change of a GPU's power limit, throttle reasons or memory held outside the container, then one for the money,
each on one line, each with a `kind` (`box`, `event` or `money`) and `at` (Unix seconds, the same on every
line of a look), so a line read alone says what it is and when; the money object closes a look. The
top-level keys of each `kind` never change: what is not known is `null`, not a missing key (`probe` is
described below: it is `null`, an error object, or the readings). Keys come out in alphabetical order, as `serde_json` writes them by default (`kind` is not first: filter on it with
`jq` or `grep '"kind":"money"'`). Money is rounded to ten-thousandths of a dollar (`credit`, `burn`, `cost`, `dph`):
about two seconds of a $0.19/h box, and vast.ai sends the credit with float noise (`12.059765171599793`).

```text
$ vas --compact --ssh --log '/root/runs/*/train.log'
{"age_s":8100,"at":1792250000,"cost":2.025,"cpu_name":"AMD EPYC 7K62 48-Core Processor","direct_host":"180.189.55.38","direct_port":55814,"dlperf":199.2,"dph":0.9,"driver_version":"580.142","gpu_name":"RTX 5090","host_id":406325,"id":56362434,"intended":"running","kind":"box","label":"me36 box 2","machine_id":100682,"num_gpus":1,"pcie_bw":26.9,"power_reference_w":575,"probe":{"clock_trend":null,"gpus":[{"index":0,"mem_total_mib":32607,"mem_used_mib":29400,"other_mem_mib":10,"power_default_w":575,"power_limit_w":575,"power_max_w":600,"power_w":560,"processes":1,"sm_max_mhz":3090,"sm_mhz":2900,"throttle":[],"util_pct":99}],"log":{"file":"/root/runs/s0/train.log","line":"e41 loss 0.3","state":"line"},"route":"proxy","util_trend":null},"ssh_host":"ssh6.vast.ai","ssh_port":36000,"status":"running","status_msg":null,"where":"South Korea, KR"}
{"at":1792250000,"boxes":1,"burn":0.9,"credit":12.93,"hours_left":14.4,"kind":"money","running":1,"shown":1,"unsure":{}}
```

`probe` is `null` for a box not probed (no `--ssh`, or not running), `{"error": "...", "error_kind": "unreachable", "direct_error": null}`
when the probe failed (`error_kind` is `unreachable` when `ssh` never reached the box, else `failed`;
`direct_error` is why the direct address failed too, after the proxy, `null` when it was not tried; a box with no
proxy address is probed at its direct address only, and `error` then says why),
else `gpus` (per GPU: `util_pct`, `mem_used_mib`, `mem_total_mib`, `other_mem_mib`, `power_w`,
`power_limit_w`, `power_default_w`, `power_max_w`, `sm_mhz`, `sm_max_mhz`, `throttle` and `processes`, each
`null` when the box did not say), `log`, `route` (`proxy`, or `direct` when the readings came past the proxy), `util_trend` and
`clock_trend` (`null` in a one-shot, else the last GPU samples of a `--watch`,
see above); `log` is `null` without `--log`, else an object whose `state` is `line`, `empty`,
`not_a_file`, `no_match` or `unreadable`, and under `--grep` also `no_matching_line` or `pattern_refused`
(each with its `file`). `status_msg` is vast.ai's own message on the box, as the API
gives it whatever the status (`null` when it gives none); `cpu_name`, `driver_version`, `dlperf` and
`pcie_bw` are always there, like the ids (what `--wide` shows), as is `power_reference_w` (the model's reference
board power, `null` for a model not in the table), and so are `direct_host` and `direct_port`, the
box's direct `ssh` address past the proxy (`null` when the API gives none). `unsure` counts, by state, the boxes meant to run that read
otherwise (the burn, above). The money is always the whole account's; `boxes` counts the account's boxes and
`shown` those printed, fewer under `--instance`. With `--ssh --watch N`, a change of a GPU's power limit, throttle reasons or memory held outside the container is an
`event` object, between the boxes and the money (see the power cap, above). With `--watch N`, a look is printed every N seconds with no separator line (the table form of `--watch`
stamps each look with `== vas, unix time N`; JSON lines would not parse with it): the `at`
tells looks apart. `jq` does the rest, for example `vas --compact | jq -r 'select(.kind=="money").hours_left'`.

## `vas offers`

| Flag | |
|---|---|
| `--gpu MODEL` | the GPU model as vast.ai names it: `RTX_5090` or `"RTX 5090"` (an underscore reads as a space) |
| `--num-gpus N` | the number of GPUs |
| `--disk GB` | the disk wanted: priced into `$/h` and required free (unset, 5 GB is priced in: the `vastai` CLI's default) |
| `--cuda VERSION` | the lowest CUDA version the host's driver must support (`12.8`) |
| `--reliability FRACTION` | the lowest reliability, from 0 to 1 (`0.99`; `99` is refused) |
| `--inet-up MBIT_S`, `--inet-down MBIT_S` | the lowest ADVERTISED bandwidth (the host's own test, not your route: see the [2026-10-16 report](https://github.com/mi-for-the-rust-of-us/vast-skopos/blob/main/docs/dogfooding/2026-10-16.md)) |
| `--max-price DOLLARS`, `--min-days DAYS` | the highest `$/h` (storage included), the shortest acceptable maximum rental |
| `--verified`, `--no-verified` | only machines vast.ai has verified; `--no-verified` cancels a config file's `--verified` |
| `--exclude-deverified`, `--no-exclude-deverified` | leave out the machines vast.ai has DEVERIFIED (verified once, then found failing its checks) and keep the unverified ones (new, or under evaluation), which `--verified` hides too ([below](#verified-unverified-deverified)); the second flag cancels a config file's first |
| `--host ID`, `--machine ID` | only this host or this machine: what a host rented from before offers now, or whether the machine you had is for rent again (`vas --wide` shows your boxes' ids); repeatable, and an `--exclude-*` of the same id still leaves it out |
| `--exclude-country CC`, `--exclude-host ID`, `--exclude-machine ID` | leave out a country (two letters), a host or a machine (a host may own several; `vas --wide` shows yours); repeatable |
| `--top N` (20), `--sort price\|reliability\|up` | how many rows, in which order |
| `--wide` | (the table's; `--changes` prints none) also show what tells one machine from another: `MACHINE` after `HOST`, then `CPU`, `DRIVER`, `DLPERF` (vast.ai's benchmark score), `DLPERF/$` (per `$/h` shown, storage included), `PCIE GB/s` and `LIMIT W` (the GPU power limit the listing gives: a listed figure, not a reading of the card, which `vas --ssh`'s `POWER` is) ([below](#is-this-the-machine-we-had)) |
| `--watch SECONDS`, `--changes` | search again every N seconds (at least 20) until Ctrl+C, each look the whole table marked with what moved; `--changes` prints only what moved ([below](#watching-the-offers)) |
| `--config FILE`, `--no-config` | read the flags of `FILE` first, or of no file |

**The config file** holds the same flags, one per line (`#` lines are comments, one pair of quotes
around a value is removed), and is read FIRST: the one `--config` names, else
`$XDG_CONFIG_HOME/vast-skopos/offers`, else `~/.config/vast-skopos/offers`. On the command line, a single value
replaces the file's, and a list adds to it (`--exclude-country RU` keeps the file's `CN`). A missing
default file is no file; a missing `--config` file, an unreadable file or a line that is not a flag
stops the run, since going on would silently drop the exclusions it holds. With no constraint at
all, `vas offers` lists every offer rentable now, and says so on stderr.

```text
# ~/.config/vast-skopos/offers: askesis's renting filters
--gpu RTX_5090
--num-gpus 1
--disk 50
--reliability 0.99
--min-days 2
# driver >= 580, which vast.ai shows as CUDA 13
--cuda 13
--inet-up 200
--exclude-country CN
--exclude-host 362355
--verified
```

**What the footer says.** vast.ai answers at most **64 rows** per search, whatever it is asked, and
still says `truncated: false`; its order decides which 64 come back, so `--sort` is sent to the
server too. When 64 rows come back, the footer says the list may be cut: narrow the search. Every
constraint is also **re-checked on each row**: a row whose field contradicts one is dropped (and
counted in the footer); a row MISSING the field is kept, with a dash. The footer also names every
constraint applied (a config file never acts unseen), the storage priced into `$/h` and the time of
the search (UTC): the list is a snapshot of a market where offers vanish within minutes.

<a id="is-this-the-machine-we-had"></a>

**Is this the machine we had?** One host can own several machines, and a search shows the host, not the
machine. `vas offers --wide` adds what tells them apart (the machine id, the CPU, the driver), and
`vas --wide` shows the same for a rented box, so the two can be set side by side; `--host ID` or
`--machine ID` narrows a search to one of them. DLPerf is shown as a number and nothing more: on
2026-10-25 two South Korean RTX 5090s read DLPerf 199.2, the same as an uncapped card, and were both capped
by their host at 500 of 600 W. `LIMIT W` may say more: the same day, another machine of that host listed
500 W, and RTX 5090 offers listed anything from 400 to 575 W. One host is not a rule; the card's own
reading (`vas --ssh`, `POWER`) is the one that says what the run gets.

<a id="verified-unverified-deverified"></a>

**Verified, unverified, deverified.** vast.ai's documentation defines three stages: *verified* machines
passed its automated checks (reliability, network, operational health, performance) and are monitored;
*unverified* ones are new or under evaluation, "not a judgment of quality"; *deverified* ones were verified
and no longer meet the requirements. None of the three checks speed or a power cap. `--verified` hides the
last two; `--exclude-deverified` hides only the deverified, so a new machine that passes your own checks
(`vas --ssh`'s `POWER`, a speed check) can still be rented. The search does not filter on the `verification`
field it shows (probed 2026-10-25: `neq`, `in` and `notin` all came back with deverified rows), so `vas`
sends `vericode <= 1` (0 is unverified, 1 verified, 2 and up deverified), and re-checks `verification` on
each row as usual.

### Watching the offers

**vast.ai's answers vary.** Measured on 2026-10-18: fifteen identical searches, 2.5 s apart, saw 129
machines (64 per answer), and only 30 of them in every answer; the others came and went, and one machine
could answer under two or three offer ids. A single search is a sample of the market, not the market:
`vas offers` ends with a line saying so (`repeat the search, or use --watch, to see more`; a watch says it once,
`--changes` not at all). Comparing one
answer with the next, offer by offer, would report that sampling as a market in turmoil, so a watch
reads the machines over several looks instead.

`vas offers --watch 60` searches again every minute (at least 20 s) until Ctrl+C. Each look is the whole
table and its footer. A machine is its machine id and its GPU count, not its offer id, and the table gains
a `SEEN` column, before `WHERE`: in how many looks each machine appeared, out of the looks since it was
first seen (`15/15` is a dependable listing, `3/9` one that flickers; why it flickers is not known).

- The first three looks only **learn** the market; nothing is marked or reported before the fourth.
- After that, `+` marks a machine that was absent from the previous three looks (or never seen), a price
  that moved reads `0.594 (was 0.575)`, and a machine is **gone** only after it has been missing for five
  looks in a row, said once.

Under the footer, what the table cannot show:

```text
since the last look: 1 new, 1 repriced, 1 gone
- 46579456  1x RTX 5090  0.554 $/h  California, US
```

`--changes` prints only what moved, one line each, for a log or a monitor: a line per look while it learns,
then one line per look (`no change`, or the counts followed by a line per offer: `+` new, `~` repriced,
`-` gone), none left out. Under the table, at most ten gone offers are listed, the rest counted.

A few details keep the notes honest:

- "Gone" is measured against the whole list the search kept, not the rows shown, so an offer that sank
  below the last row is not gone. With `--top 3`, a new offer that is not among the three rows has no `+`:
  the note counts it (`1 new (1 below the table)`).
- vast.ai answers at most 64 rows per search: when it does, a change in who entered or left may only be an
  offer crossing that line, and the note says so.
- A machine listed under two offer ids at DIFFERENT prices (the same GPU count) counts once, by its cheaper
  row; if one look shows only the cheap id and the next only the dearer one, that reads as a price that moved.
  The table can show a machine twice (two rows), and `SEEN` counts it once.
- A search that fails ends a watch whose FIRST look failed (a wrong `VAST_URL` or key would otherwise loop on
  stderr forever); after one look has succeeded, a failure is only said on stderr, and the next look reads
  `since the last successful look`. The lookout's `vas --watch` shares the first two rules (a failed first
  look ends it, a failure after a success is said and the watch goes on) but prints no such note.
- A `--watch` under the floor (10 s for `vas`, 20 s for `vas offers`) is raised to it, and `vas` says so on
  stderr. The floor is a courtesy to the service: vast.ai's own limits are read and obeyed whatever it says.
  `VAS_WATCH_FLOOR_MS` (milliseconds) replaces both floors: it exists for the test suite, and setting it
  removes the courtesy and nothing else.
- A table every few seconds scrolls past unread in a terminal: for a person, `--changes` (one line per look and
  per change) reads best; `vas --compact --watch N` is for scripts.
- A config file cannot hold `--watch` or `--changes`: a file of filters must not turn a run into a loop.

