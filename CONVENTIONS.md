# vast-skopos Coding Conventions (Grit + Grit-MI Extensions)

This document describes the [Amphigraphic coding](https://github.com/PCfVW/Amphigraphic-Strict) conventions used in vast-skopos. It is a superset of
the [Grit — Strict Rust for AI-Assisted Development](https://github.com/PCfVW/Amphigraphic-Strict/tree/master/Grit), and follows
[`hf-fetch-model/CONVENTIONS.md`](https://github.com/mi-for-the-rust-of-us/hf-fetch-model/blob/main/CONVENTIONS.md) section for section. Its
last two sections are this crate's own: [the API key](#when-handling-the-api-key) and [talking to a box](#when-talking-to-a-box).

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
- Public structs that may gain new fields: `#[non_exhaustive]` (`Instance`,
  `Account`, `Probe`, `GpuReading`) — the API grows, and so will what `vas` shows.
  Every such struct has a constructor (`from_json`, a parser) for callers.
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
- **Never the API key**, in any error (next section).

---

## When Handling the API Key

The key spends the account's money. It must never leave the process except as
the `Authorization: Bearer` header of a request to the vast.ai API host.

- **`ApiKey` has a redacted `Debug` and no `Display`.** Its text is reachable only
  through `pub(crate) fn bearer`, used in exactly one place: the request header.
- **Never in an error, a log line, a table or a panic message.** Errors carry the
  endpoint PATH, never a full URL with a query, never a header.
- **Never parsed from a response.** `/api/v0/users/current` returns the key: only
  `credit` and `balance` are read from it (`Account::from_json`).
- **Never written to disk** by this crate, and never accepted on the command line
  (a `--key` flag would land in shell history): `VAST_API_KEY`, the `vastai` CLI's
  key files, or `--key-file`.
- **Tests use dummy keys** and assert that error text does not contain them.

## When Calling the vast.ai API

### Read-only

`vas` only ever issues `GET`. It never creates, starts, stops, labels or destroys
an instance: a monitor that can spend or destroy is a different tool, with a
different review.

### Lenient Reading

Rows are read field by field from `serde_json::Value` (`instance.rs`), never with a
derived `Deserialize` over the whole row: the API adds, drops and retypes fields,
and one unused field changing type must cost a dash in one cell, not the whole table.
Numbers accept a JSON number or a numeric string; a row without an integer `id` is skipped.

The endpoints and their pagination follow the official `vastai` CLI's source (cite the
function when changing one: `_fetch_all_instances_v1`, `show__user`).

## When Talking to a Box

- **The system `ssh`**, never an `ssh` library: the user's keys, agent and
  `~/.ssh/config` apply unchanged.
- **Arguments as arguments.** `Command::arg` for every local argument (a key path
  with a space stays one path); only the REMOTE command is a shell string, and only
  the user's own `--log` pattern is interpolated into it, unquoted on purpose so the
  box's shell expands the glob.
- **One attempt.** `BatchMode=yes`, `ConnectTimeout=10`, and no retry loop: a box
  that refuses is reported in its row, and what to do about it is a person's call,
  made from the dashboard.
- **Trust the box, not the dashboard** for GPU utilisation: the API's `gpu_util` is
  not shown, because it has been seen to read 0 % on boxes at 97-99 %.
