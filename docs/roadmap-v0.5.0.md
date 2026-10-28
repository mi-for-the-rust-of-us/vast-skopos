# Roadmap: 0.5.0, a `vas` that is easy to pick up

**Not affiliated with vast.ai.** This page is the plan for 0.5.0, written on 2026-10-28 before 0.4.0 was
released. It is a plan, not a promise: what ships is what the [CHANGELOG](../CHANGELOG.md) says. The
earlier roadmaps ([0.2.0](roadmap-v0.2.0.md), [0.3.0](roadmap-v0.3.0.md)) are kept as they were written.

## Why

By 0.4.0, `vas` answers most questions a rental asks. It watches the boxes, says why one is not running,
reads the GPUs over `ssh`, waits for a line in a log, and finds offers. Each of those is documented
precisely. That precision is now the obstacle: there is a lot to read before the first useful command. The
same is true for an AI assistant driving `vas` for someone. 0.5.0 adds no new lookout. It makes what is
there easy to learn, for a person and for an agent.

## Where 0.4.0 stands (measured 2026-10-28, on the `next-0.4.0` branch)

- **`vas --help` is 684 words, and `vas -h` prints the same.** Every flag's help is a full paragraph; the one
  for `--until` alone holds five exit-code rules. `vas offers --help` is 501 words.
- **The help has no examples,** never names the environment variables (`VAST_API_KEY`, `VAST_URL`), and says
  the exit codes only inside `--until`.
- **The README is 7,900 words.** It opens with three release banners in a row (0.3.0, 0.2.0, 0.1.0), and
  0.4.0 would add a fourth. The FAQ has 39 questions (5,700 words), and the two tutorials have 4,200 words.
  A newcomer has to dig to find the three commands they need first.
- **For an AI assistant, the precision is an asset,** since an agent that reads `vas --help` gets the exact
  behaviour. What it lacks is the overall picture:
  - which command answers which question;
  - that `--compact` is the format meant for programs;
  - that `vas` only reads, and renting or destroying a box is the human's call;
  - how to read `POWER` (and its `<575`), `OTHER`, `UNREACHABLE` vs `SSH FAILED`, and `100%?`.

## The plan

### 1. Two levels of help

clap takes the first paragraph of each flag's doc comment for `-h`, and the whole comment for `--help`.

- **Each flag opens with a one-line summary,** so `vas -h` and `vas offers -h` fit on one screen. The
  detail stays in `--help`, unchanged.
- **`--help` ends with three blocks** (clap's `after_long_help`):
  - **examples:** "watch my boxes", "is any GPU idle or capped?", "wait for the first loss on one box",
    "find a verified RTX 5090 under $0.50/h";
  - **the exit codes,** in one table: 0, 1, 2, 3, and what each means for a one-shot, a watch and a wait;
  - **the environment variables:** `VAST_API_KEY`, `VAST_URL`, and where the key is otherwise looked for.
- **Tests:**
  - every example is accepted by the current flags, as `tests/readme.rs` checks the README's commands;
  - each `-h` line stays under a width, so it cannot grow back into a paragraph unnoticed;
  - the exit codes block uses the same constants as the code (`GAVE_UP`, `REFUSED`), as the `--until` help
    already does.

### 2. A skill for agents, shipped in the repository

- **The file:** `skills/vas/SKILL.md`, in the Agent Skills format (a `name` and a `description` in the front
  matter, then the instructions). It holds:
  - **when to reach for `vas`:** what it answers, and what it does not do (it never rents, stops or destroys
    a box);
  - **about six recipes,** one per question. "How are my boxes doing?", "Is a GPU idle, capped or shared?",
    "Wait for a line, with a time limit (`timeout`)", "Find an offer like the box we had", "Keep the credit
    as a curve", "Follow one box of several (`--instance`)";
  - **`--compact` whenever the output is parsed,** with its keys and the rule that a key is always present
    (`null` when not known);
  - **the exit codes,** and what an agent should do after each;
  - **how to read each mark:** `450/575W+power_cap`, `450W<575`, `OTHER`, `100%?`, `UNREACHABLE 4m` vs
    `SSH FAILED 4m`, the status line under the table, `(direct)`;
  - **the rules:**
    - never print or log the API key;
    - `vas` sends only GETs, and any action on a box is the human's;
    - a watch costs nothing, but the boxes cost every minute;
  - **the traps:** Git Bash rewriting `/root/...` (and `//root/...` or `MSYS_NO_PATHCONV=1` around it); a
    `--log` glob that matches several runs on one box.
- **A test,** like the README's: every `vas ...` line in the skill is accepted by the current flags, and
  every `--compact` key it names is one the code writes. The skill then cannot drift from the code.
- **Pointers:** the README and the end of `vas --help` say where the skill is.

### 3. A lighter README top

- **One "What's new" line** pointing to the CHANGELOG, in place of the stacked banners. The banners' stories
  move to the CHANGELOG or the dogfooding reports, where they already are in full.
- **A quick start of three commands** right after Install: `vas`, `vas --ssh --log ...`, `vas offers ...`,
  each with what it shows.
- **The long flag reference** moves lower, or into a page of its own (`docs/reference.md`), linked from the
  top.
- How much to cut is Éric's call; nothing is removed that a test or a link relies on.

### 4. Measure the learning curve

Not a guess but a test. A fresh agent gets only `vas --help` (and, in a second run, only the skill), a mock
vast.ai server and a fake `ssh` like the test suite's, and real tasks:

- "Is any box idle?" (one reads `100%?`, one is at work);
- "Is a card capped?" (a `450/575W+power_cap` and a `450W<575`);
- "Wait for box 2's first loss, at most ten minutes" (two boxes write the same log);
- "Find a cheaper verified machine like this one" (the offers' `--wide` against the box's);
- "Why is this box not running?" (a status line).

- **What is kept:** where the agent stumbles (a flag it missed, a mark it misread, an exit code it did not
  expect) is what to fix, in the help or in the skill. The runs are written up as a dogfooding report, the
  way the rentals are.
- **The bar for 0.5.0:** each task done right from the help alone, or from the skill alone, with no guessing
  of flags.

## Order of work

1. The two levels of help (1), since the skill and the README quote it.
2. The skill and its test (2).
3. The first measurement (4); the fixes it calls for go back into 1 and 2.
4. The README's top (3), last, once the help and the skill say what it can point to.

## Not in 0.5.0

- New lookouts, or new flags beyond what the help's examples need.
- The items still open from earlier versions stay where they are, to be seen at a rental when they turn up:
  - `UNREACHABLE` and the direct-route fallback on a real refusing proxy;
  - `100%?` on an idle box;
  - the `<575`-style mark on a board below its reference;
  - note 4 of the 2026-10-23 report (did `vas offers --watch` help the hunt?).
