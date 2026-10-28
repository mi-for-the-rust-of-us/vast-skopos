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
- **The help has no examples.** It names `VAST_API_KEY` once, inside `--key-file`, and never `VAST_URL` or
  `XDG_CONFIG_HOME`. It says the exit codes only inside `--until`; `vas offers --help` never mentions them.
- **The README is 7,900 words.** It opens with three release banners in a row (0.3.0, 0.2.0, 0.1.0), and
  0.4.0 will add a fourth. Its "Try it" section, with a `vas --ssh` and a `vas offers` sample, comes after
  80 lines of "Usage". The FAQ has 39 questions (5,700 words), and the two tutorials have 4,200 words. A
  newcomer has to dig to find the three commands they need first.
- **For an AI assistant, the precision is an asset,** since an agent that reads `vas --help` gets the exact
  behaviour. What it lacks is the overall picture:
  - which command answers which question;
  - that `--compact` is the format meant for programs;
  - that `vas` only reads, and renting or destroying a box is the human's call;
  - how to read `POWER` (and its `<575`), `OTHER`, `UNREACHABLE` vs `SSH FAILED`, and `100%?`.

## The plan

### 1. Two levels of help

clap takes the first paragraph of each flag's doc comment for `-h`, and the whole comment for `--help`.
Today every flag's comment is a single paragraph, which is why the two print the same.

- **Each flag opens with a one-line summary, then a blank line,** so `vas -h` and `vas offers -h` fit on one
  screen. The detail stays in `--help`, unchanged.
- **`--help` ends with three blocks** (clap's `after_long_help`):
  - **examples:** "watch my boxes", "is any GPU idle or capped?", "wait for the first loss on one box",
    "find a verified RTX 5090 under $0.50/h";
  - **the exit codes,** in one table: 0, 1, 2, 3, and what each means for a one-shot, a watch and a wait;
  - **the environment variables:** `VAST_API_KEY`, `VAST_URL` (and that the key is then sent there),
    `XDG_CONFIG_HOME`; where the key is otherwise looked for (the `vastai` CLI's files) and where `vas
    offers` finds its config file. `VAS_WATCH_FLOOR_MS` is the tests' seam and stays undocumented.
- **Tests,** in the binary's own test module, since `Cli` is private to it:
  - every example parses (`Cli::try_parse_from`). `tests/readme.rs` already holds the README's samples to the
    columns and keys the code prints; this holds the help's commands to its flags;
  - each flag's short help stays one line long, so it cannot grow back into a paragraph unnoticed;
  - the exit codes block is pinned to the code's constants (`GAVE_UP`, `REFUSED`) by a test, as the
    `--until` help already is (`the_help_of_until_says_the_looks_and_the_exit_code_the_code_uses`).

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
    - a watch spends no credit (a few API calls per look), but the boxes cost every minute;
  - **the traps:**
    - Git Bash rewriting `/root/...` (`vas` repairs it and says so; `//root/...` or `MSYS_NO_PATHCONV=1`
      avoid it);
    - a `--log` glob that matches several runs on one box: only the newest file is read.
- **A test,** in the binary's test module like the help's: every `vas ...` line in the skill parses with the
  current flags, and every `--compact` key it names is one the code writes (as `tests/readme.rs` checks the
  README's). The skill then cannot drift from the code.
- **Pointers:** the README and the end of `vas --help` say where the skill is.
- **Open, to settle while writing it:** how the skill reaches an agent. Copying the folder into a project's
  `.claude/skills/` or the user's `~/.claude/skills/` works for Claude Code today. Other agents may read the
  file directly. The crate's package includes `skills/` (`Cargo.toml` excludes only `.github/`, `deny.toml`
  and two `docs/` folders), so it ships with the source.

### 3. A lighter README top

- **One "What's new" line** pointing to the CHANGELOG, in place of the stacked banners. The banners' stories
  move to the CHANGELOG or the dogfooding reports, where they already are in full.
- **A quick start of three commands** right after Install: `vas`, `vas --ssh --log ...`, `vas offers ...`,
  each with what it shows. "Try it" already has the samples; it moves up and gains the third command.
  `tests/readme.rs` finds its sample by the `$ vas --ssh` line wherever it stands, so the move costs the test
  nothing.
- **The long flag reference** moves lower, or into a page of its own (`docs/reference.md`), linked from the
  top.
- How much to cut is Éric's call; nothing is removed that a test or a link relies on.

### 4. Measure the learning curve

Not a guess but a test. A fresh agent gets only `vas --help` (and, in a second run, only the skill), a mock
vast.ai server and a fake `ssh`, and real tasks. `VAST_URL` points both `vas` and `vas offers` at the mock,
and the fake `ssh` comes first on the `PATH`, as in `tests/until_cli.rs`. The test suite's mock lives in
`tests/common` and serves only a test, so the harness is part of the work: a small mock that keeps running,
its fixtures, and the fake `ssh` (Linux or WSL, like the tests that use one). Nothing in it reaches vast.ai
or a real box.

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

- New lookouts or new flags. One exception may come from the skill's open question: a flag that prints the
  skill, if copying a folder proves too awkward. It would be decided then.
- The items still open from earlier versions stay where they are, to be seen at a rental when they turn up:
  - `UNREACHABLE` and the direct-route fallback on a real refusing proxy;
  - `100%?` on an idle box;
  - the `<575`-style mark on a board below its reference;
  - note 4 of the 2026-10-23 report (did `vas offers --watch` help the hunt?).
