# vast-skopos Coding Conventions (Grit + Grit-MI Extensions)

This document describes the [Amphigraphic coding](https://github.com/PCfVW/Amphigraphic-Strict) conventions used in vast-skopos. It is a superset of
the [Grit — Strict Rust for AI-Assisted Development](https://github.com/PCfVW/Amphigraphic-Strict/tree/master/Grit), and follows
[`hf-fetch-model/CONVENTIONS.md`](https://github.com/mi-for-the-rust-of-us/hf-fetch-model/blob/main/CONVENTIONS.md) section for section. Its
own sections are [writing a file](#when-writing-a-file), [the API key](#when-handling-the-api-key), [calling the vast.ai API](#when-calling-the-vastai-api) and [talking to a box](#when-talking-to-a-box).

## Trigger Checklist

**Before writing any line of code, check which triggers apply.**

| You are about to... | Check these rules |
|---|---|
| Write a `///` or `//!` comment | [Backtick hygiene](#backtick-hygiene), [field-level docs](#field-level-docs), [intra-doc link safety](#intra-doc-link-safety) |
| Write a `pub fn` or `pub const fn` | [`const fn`](#const-fn), [`#[must_use]`](#must_use-policy), [pass by value](#pass-by-value-vs-reference) |
| Write a `pub fn` returning `Result<T>` | [`# Errors` section](#errors-doc-section) |
| Write a `pub enum` or `pub struct` | [`#[non_exhaustive]`](#non_exhaustive-policy) or [`// EXHAUSTIVE:`](#exhaustive-annotation) |
| Write an `as` cast | [`// CAST:`](#cast-annotation) |
| Write `slice[i]` or `slice[a..b]` | [`// INDEX:`](#index-annotation) |
| Write `.as_str()`, `.to_owned()` | [`// BORROW:`](#borrow-annotation) |
| Write an `unsafe` block | [`// SAFETY:`](#safety-annotation) (none expected: `unsafe_code = "forbid"`) |
| Write `Box<dyn T>` or `&dyn T` | [`// TRAIT_OBJECT:`](#trait_object-annotation) |
| Write a `match` or `if let` | [Control-flow rules](#if-let-vs-match), [`// EXPLICIT:`](#explicit-annotation) if no-op arm |
| Write error strings | [Error message wording](#error-message-wording) |
| Add `#[allow(clippy::...)]` for a lint newer than the MSRV | [MSRV lint guard](#msrv-lint-guard) |
| Push, or open a pull request | [The local gates](#the-local-gates) |
| Write to a file | [Writing a file](#when-writing-a-file) |
| Touch the API key, a header, an error or any output | [The API key](#when-handling-the-api-key) |
| Call the vast.ai API | [Read-only](#read-only), [lenient reading](#lenient-reading) |
| Run `ssh` or anything on a box | [Talking to a box](#when-talking-to-a-box) |

---

## When Writing Doc Comments (`///`, `//!`)

### Backtick Hygiene

All identifiers, types, trait names, field names, crate names, endpoint paths
and command names in doc comments must be wrapped in backticks so that rustdoc
renders them as inline code and Clippy's `doc_markdown` lint passes.

Applies to: struct/enum/field names, method names (`fn foo`), types
(`Vec<T>`, `Option<f64>`), crate names (`ureq`, `serde_json`), endpoints
(`/api/v1/instances/`), commands (`ssh`, `nvidia-smi`, `vas`), and environment
variables (`VAST_API_KEY`).

> ✅ `` /// Reads one row of `/api/v1/instances/` into an [`Instance`]. ``
> ❌ `/// Reads one row of /api/v1/instances/ into an Instance.`

The one exception is `clap`'s argument docs in `src/bin/vas.rs`: they ARE the
`--help` text, where backticks would print literally. That struct carries
`#[allow(clippy::doc_markdown)]` with an `// EXPLICIT:` comment.

### Intra-Doc Link Safety

Rustdoc intra-doc links must resolve under all feature-flag combinations
(`cargo doc --all-features --no-deps` with `RUSTDOCFLAGS="-D warnings"` in CI).

1. **Feature-gated items** use plain backtick text, not link syntax.
2. **Cross-module links** to items re-exported at the crate root (`SkoposError`)
   use the bare form: `` [`SkoposError::Ssh`] ``, never
   `` [`SkoposError::Ssh`](crate::SkoposError::Ssh) ``.

### Field-Level Docs

Every field of every `pub` struct must carry a `///` doc comment describing:
1. what the field represents,
2. its unit (dollars, dollars per hour, Unix seconds, MiB, percent) or valid range,
3. when it can be `None` (every `Instance` field but `id` can, and says so once on the struct).

Private fields of `pub` structs carry a `///` comment too (`missing_docs_in_private_items`).

> Example:
> ```rust
> pub struct GpuReading {
>     /// Utilisation, percent (0-100), as `nvidia-smi` samples it.
>     pub util_pct: u32,
>     /// Memory used, MiB.
>     pub mem_used_mib: u64,
> }
> ```

### `# Errors` Doc Section

All public fallible methods (`-> Result<T>`) must include an `# Errors` section
in their doc comment. Each bullet uses the format:

    /// # Errors
    /// Returns [`SkoposError::Http`] if a request cannot be made.
    /// Returns [`SkoposError::Status`] if vast.ai answers with a non-success status.

Rules:
- Start each bullet with `Returns` followed by the variant in rustdoc link
  syntax, e.g., `` [`SkoposError::Http`] ``.
- Follow with `if` (condition), `on` (event), or `when` (circumstance).
- Use the concrete variant name, not the generic `SkoposError`.
- One bullet per distinct error path.

---

## When Writing Function Signatures

### `const fn`

Declare a function `const fn` when **all** of the following hold:
1. The body contains no heap allocation, I/O, or `dyn` dispatch.
2. All called functions are themselves `const fn`.
3. There are no trait-method calls that are not yet `const`.

When in doubt, annotate and let the compiler reject it — do not omit `const`
preemptively. (Clippy's nursery `missing_const_for_fn` is on.)

### `#[must_use]` Policy

All public functions and methods that return a value and have no side effects
must be annotated `#[must_use]`: constructors (`ApiKey::new`, `Client::new`),
readers (`Instance::from_json`), and pure queries (`Instance::cost_so_far`,
`render::table`). The `clippy::must_use_candidate` lint enforces this at `warn`.

### Pass by Value vs Reference

| Type | Rule |
|---|---|
| `Copy` type ≤ 2 words (`u16`, `f64`, `bool`, small `enum`) | Pass by value |
| `Copy` type > 2 words | Pass by reference |
| Non-`Copy`, not mutated | Pass by `&T`, `&str`, `&Path` or `&[T]` |
| Non-`Copy`, mutated | Pass by `&mut T` |
| Owned, consumed by callee | Pass by value (move semantics: `Client::new(key)`) |
| `&mut T` not actually mutated in body | Change to `&T` |

---

## When Writing Public Enums and Structs

### `#[non_exhaustive]` Policy

- Public enums that may gain new variants: `#[non_exhaustive]` (`SkoposError`).
- Public structs that may gain new fields: `#[non_exhaustive]` (e.g. `Instance`, `Account`,
  `Offer`, `OfferQuery`, `OfferPage`, `OfferChanges`, `Probe`, `TableOptions`, `Runway`) — the API
  grows, and so will what `vas` shows. A caller builds one with its constructor (`from_json`,
  `Default`, `new`, a parser) and, for a query or an options struct, by assigning fields after
  `default()`; a type a downstream crate has no way to build (`GpuReading`, today) is a known gap.
- Internal dispatch enums matched exhaustively by this crate:
  `#[allow(clippy::exhaustive_enums)] // EXHAUSTIVE: <reason>`.

---

## When Writing Expressions

These annotations are required **on or immediately before** the line where
the pattern occurs. Apply them as you write the line, not in a review pass.

### CAST Annotation

`// CAST: <from> → <to>, <reason>` — required on every `as` cast between numeric types. Prefer `From`/`Into` for
lossless conversions and `TryFrom`/`TryInto` for fallible ones (`u16::try_from(port)`).
Use `as` only when truncation is the deliberate intent.
> Example: `// CAST: f64 → u64, a non-negative age in seconds, truncated to whole seconds on purpose`

### INDEX Annotation

`// INDEX: <reason>` — required on every direct slice index (`slice[i]`, `slice[a..b]`) that cannot
be replaced by an iterator. Library code has none: `indexing_slicing = "deny"`.

### BORROW Annotation

`// BORROW: <what is converted>` — required on explicit `.as_str()`, `.as_bytes()`, `.to_owned()` conversions (Grit Rule 2).
> Example: `// BORROW: explicit to_owned -- the key outlives the file buffer it was read from`

### TRAIT_OBJECT Annotation

`// TRAIT_OBJECT: <reason>` — required on every `Box<dyn Trait>` or `&dyn Trait` usage. None expected.

---

## When Writing `unsafe`

### SAFETY Annotation

`// SAFETY: <invariants>` — required on every `unsafe` block or function.

vast-skopos is `unsafe_code = "forbid"`; no `unsafe` is expected. If it ever
becomes necessary, follow the candle-mi pattern: single dedicated module,
feature-gated, with a `// SAFETY:` comment on every block.

---

## When Writing Control Flow

### `if let` vs `match`

| Situation | Preferred form |
|---|---|
| Testing a single variant, no binding needed | `matches!(expr, Pat)` |
| Testing a single variant, binding needed | `if let Pat(x) = expr { … }` |
| Two or more variants with different bodies | `match expr { … }` |
| Exhaustive dispatch over an enum | `match expr { … }` (never `if let` chains) |

Matches over `serde_json::Value` name every variant (`Value::Null | Value::Bool(_) | ...`)
rather than `_`: `wildcard_enum_match_arm = "deny"`.

### EXPLICIT Annotation

`// EXPLICIT: <reason>` — required when a match arm is intentionally a no-op, when an imperative
loop is used instead of an iterator chain for a stateful computation, or when a lint is allowed
because its remedy would change behaviour.
> Example: `// EXPLICIT: collected so every probe is SPAWNED before any is joined; a lazy chain would run them one after another.`

### EXHAUSTIVE Annotation

`// EXHAUSTIVE: <reason>` — required on `#[allow(clippy::exhaustive_enums)]`.

---

## When Writing Error Strings

### Error Message Wording

Error strings carried by `SkoposError` variants follow two patterns:

- **External failures** (I/O, network, a process): `"failed to <verb>: {e}"`
  > Example: `"failed to read the API key file {path}: {source}"`
- **Validation failures** (status, shape, absence): `"<noun> <problem> (<context>)"`
  > Example: `"vast.ai returned status {status} for {endpoint}{hint}"`

Rules:
- Use lowercase, no trailing period.
- Include the offending value (the endpoint, `host:port`, the status) and, for the
  common statuses, a short hint (`401`: is the key right?).
- Wrap external errors with `: {e}`, not `.to_string()`.
- **Never the API key**, in any error ([The API key](#when-handling-the-api-key)).

---

## When Suppressing a Lint, and Before a Push

### MSRV Lint Guard

`Cargo.toml`'s `[lints.rust]` sets `unknown_lints = "allow"` (after hypomnesis's `lib.rs` guard), so
an `#[allow(clippy::newer_lint)]` naming a lint the MSRV toolchain (1.88) does not know cannot fail
the MSRV build under `-D warnings`. It lives in `Cargo.toml`, not `lib.rs`, because the integration
tests and `vas` are crates of their own.

The price: a misspelt lint name in an `allow` is not reported. Check a suppression by changing
`#[allow]` to `#[expect]` for a moment, on stable AND on the MSRV: some lints fire on only one
toolchain. (2026-10-16, the first CI run: clippy 1.88's `doc_markdown` flagged a bare `PowerShell`
that stable's does not, and all three MSRV jobs failed.)

### The Local Gates

Before a push, each command alone, its exit code read on its own (never through a pipe, which
reports the pipe's last command):

```sh
cargo fmt --check
cargo check --locked --no-default-features
cargo clippy --locked --all-targets --no-default-features -- -D warnings
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo +1.88 clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-features
cargo +1.88 test --locked --all-features
RUSTDOCFLAGS="-D warnings" cargo doc --all-features --no-deps
```

A test file gated on a platform (`tests/remote.rs`, `#![cfg(target_os = "linux")]`) compiles to
nothing elsewhere, so no Windows or macOS gate lints or runs it: lint it with the gate lifted (on a
copy, restored and checked by its checksum), and let Linux CI run it.

---

## When Writing a File

The crate reads files (the key, the config file) and writes exactly one kind: the credit log
(`vas --credit-log FILE`, `creditlog::CreditLog`). A second kind of file is a change of rule, with
its own review, not a convenience.

- **Only when asked, and only where asked.** Nothing is written unless the user names the file; no
  default path, no cache, no state kept between runs.
- **Append-only.** The file is opened to append and never truncated, rewritten or deleted; a header
  is written only when the file is new or empty, so a log survives restarts and a user's own edits.
- **No secret.** A row holds the time, the credit and the burn: never the key, an endpoint or a
  header ([The API key](#when-handling-the-api-key)).
- **Appended whole, last line first.** A row goes out in one write, on a line of its own (a file whose
  last line has no newline gets one first). Two `vas` processes on one NEW file may both write the
  header: that race is accepted. A change of columns is a new file, never a rewrite of an old one.
- **A failure is said once.** A file that cannot be written is named on stderr one time
  (`failed to write the credit log FILE: ...`); the look goes on and the log stops trying. A
  lookout does not die because a disk is full.

## When Handling the API Key

The key spends the account's money. It must never leave the process except as
the `Authorization: Bearer` header of a request to the vast.ai API host.

- **`ApiKey` has a redacted `Debug` and no `Display`.** Its text is reachable only
  through `pub(crate) fn bearer`, used in exactly one place: the request header.
- **Never in an error, a log line, a table or a panic message.** Errors carry the
  endpoint PATH, never a full URL with a query, never a header.
- **Never parsed from a response.** `/api/v0/users/current/` returns the key: only
  `credit` and `balance` are read from it (`Account::from_json`).
- **Never written to disk** by this crate (not even in the credit log), and never accepted on the command line
  (a `--key` flag would land in shell history): `VAST_API_KEY`, the `vastai` CLI's
  key files, or `--key-file`.
- **Tests use dummy keys** and assert that error text does not contain them.
- **What needs no key gets none.** The offer search is public: `SearchClient` holds no key and
  sends no `Authorization` header (`get_json(.., bearer: None, ..)`), and `vas offers` never
  looks for one. The mock tests assert the header is absent.

## When Calling the vast.ai API

### Read-only

`vas` only ever issues `GET`. It never creates, starts, stops, labels or destroys
an instance: a monitor that can spend or destroy is a different tool, with a
different review. The offer search is the `GET /api/v0/bundles/?q=` form, the
predecessor the official CLI keeps commented out beside its `POST`: it is still
answered, and keeps `vas` to `GET`. If it goes (404/405, hinted), moving to `POST`
is a change of rule, with its own review, not a silent fallback.

### Rate Limits

vast.ai budgets the calls of each endpoint per window and says so in `X-RateLimit-Limit`, `-Remaining`
and `-Reset` (seen 2026-10-18: 30 for the offer search, ONE for the instances list). The library
REPORTS: the clients keep what each answer said (`api::RateLimit`, `budget_wait_secs`), and a 429 is
`SkoposError::RateLimited` with the wait; it never sleeps and never retries (`Client::instances_with`
asks the caller what to do when the budget is spent BETWEEN two pages, and the caller sleeps, if it
does). The binary decides: a wait of at most `PATIENCE_SECS`, said on stderr, before each further page
of the instances list and once before the one retry of a look a 429 refused; a longer wait is named
and ends the run; a watch leaves at least the budget's reset between two looks, and never more than
`MAX_BUDGET_WAIT_SECS` (900 s: a longer reset is a mis-scaled header, said on stderr), and never less
than its floor (10 s for the lookout, 20 s for `vas offers`: a courtesy, since the limits above are
obeyed anyway). No loop of retries, ever: a refused call is a person's decision, as for a refused
`ssh`.

### Lenient Reading

Rows are read field by field from `serde_json::Value` (`instance.rs`, `offer.rs`),
never with a derived `Deserialize` over the whole row: the API adds, drops and
retypes fields, and one unused field changing type must cost a dash in one cell, not
the whole table. Numbers accept a JSON number or a numeric string and must be finite;
an integer may be written `5`, `5.0` or `"5"`. Text loses its control and invisible
characters (`instance::is_hidden`: it comes from other people's hosts and reaches a
terminal, where a bidirectional override can reorder a row), and a cell of it is capped
(`render::TEXT_WIDTH`). That covers EVERYTHING a box or a host sends: the API's text,
and the probe's log lines, file names and `ssh` error line (`probe::strip`); a reading
from a box is untrusted arithmetic too (sum in `u64`, saturate, never overflow).
A row without an integer `id` is skipped; the offers search counts the rows it skipped
(the footer says so), the instances list does not.

The endpoints and their pagination follow the official `vastai` CLI's source (cite the
function when changing one: `_fetch_all_instances_v1`, `show__user`, `search__offers`).

### Adding a `vas offers` Constraint

The server answers at most 64 rows and may ignore a filter it does not know, so a
constraint lives in SIX places, kept in step:

1. an `OfferQuery` field (documented with its unit);
2. `OfferQuery::is_identity`;
3. `OfferQuery::to_search_json`: sent to the server, in the server's unit (days become seconds);
4. `OfferQuery::admits`: re-checked on each row with the SAME semantics and unit; a
   field the row has that contradicts it drops the row, a missing field keeps it;
5. `OfferQuery::summary`: the footer names it, so a config file never acts unseen;
6. the flag in `src/bin/vas.rs` (its value parser refuses non-finite and out-of-range
   values) and `OffersArgs::query`.

A test decodes the `q` the mock received and compares it whole, and `tests/offer.rs`
gives the new constraint its row in `VIOLATIONS`: a contradicting value drops the row,
the field missing keeps it.

### The Config File

A config file holds deny-lists, so it never fails open: a missing default file is no
file, but a missing `--config` file, an unreadable file or a malformed line stops the
run before any request. Each line becomes ONE word, `--name=value`, so clap reads a
value starting with `-` as a value; `--config`, `--no-config`, `--watch`, `--changes`, `--help` and
`--version` are refused inside it (a file of filters must not choose the file, turn a run into a
loop, or print and stop).

## When Talking to a Box

- **The system `ssh`**, never an `ssh` library: the user's keys, agent and
  `~/.ssh/config` apply unchanged.
- **Arguments as arguments.** `Command::arg` for every local argument (a key path
  with a space stays one path); only the REMOTE command is a shell string, and only
  the user's own `--log` pattern is interpolated into it, unquoted on purpose so the
  box's shell expands the glob.
- **One attempt.** `BatchMode=yes`, `ConnectTimeout=10`, and no retry loop: a box
  that refuses is reported in its row, and what to do about it is a person's call,
  made from the dashboard. One session per probe too (`ControlMaster=no`,
  `ControlPath=none`): a shared master of the user's config would hold the pipes open.
- **One question per `nvidia-smi` query, when a driver may lack the field.** The
  utilisation and memory, the power and clocks, the clock event reasons (asked under
  both names, `||`), the GPUs' bus ids and the compute processes each have their own
  query and section: a driver that lacks a field costs that reading and nothing else.
  Every query is bounded by `timeout` on the box, and an error `nvidia-smi` prints
  (on stdout) is a line the parsers skip.
- **Trust the box, not the dashboard** for GPU utilisation: the API's `gpu_util` is
  not shown, because it has been seen to read 0 % on boxes at 97-99 %.
