//! `vast-skopos`: a lookout (σκοπός) over your rented vast.ai boxes, and over the offers to rent more.
//!
//! **Not affiliated with vast.ai.** An independent tool that reads the public vast.ai API with
//! your own key, and your boxes over your own `ssh`. It only ever reads: it never rents, starts,
//! stops or destroys anything.
//!
//! `vas`: one table per call: for every instance, its status, `$/h`, age and cost so far, and its
//! `ssh` endpoint; under it, the account's credit, the burn rate and the hours that credit buys at
//! that rate. With `--ssh`, each running box is also asked directly, once, what its GPUs are doing
//! (`nvidia-smi`) and, with `--log`, which log it read and that log's last non-blank line: the
//! dashboard's GPU utilisation is not trusted, because it has been seen to read 0 % on boxes at
//! 97-99 %. `--compact` prints the same readings as JSON Lines, and `--watch` repeats the look
//! (until a log line matches, with `--until`).
//!
//! `vas offers`: the offers for rent that pass your constraints (cheapest first by default), with no key
//! ([`api::SearchClient`]); with `--watch`, the list again every so often, marked with what moved.
//!
//! The library half is what `vas` is built from:
//!
//! - [`key`]: where the API key is found (and that it is never printed);
//! - [`api`]: the three endpoints read (`/api/v1/instances/`, paginated, and
//!   `/api/v0/users/current/`, with the key; the offer search, without);
//! - [`instance`]: the lenient readers of their JSON;
//! - [`offer`]: the offers for rent, and the search `vas offers` sends;
//! - [`market`]: what a watch makes of searches that do not agree with each other;
//! - [`probe`]: the `ssh` probe and its `nvidia-smi` parser;
//! - [`render`]: the tables;
//! - [`runway`]: the credit, the burn and the hours they last, as the money line and `--compact` read them;
//! - [`compact`]: the same readings as JSON Lines, for scripts and monitors;
//! - [`trend`]: the last GPU utilisation and clock samples of each box, and what changed in their power, kept by a watch;
//! - [`until`]: whether a look found the line `vas --until` waits for;
//! - [`creditlog`]: the credit history, one CSV row per look, in the file `--credit-log` names;
//! - [`error`]: [`SkoposError`] and the [`Result`] alias every fallible call returns.

pub mod api;
pub mod compact;
#[doc(hidden)]
pub mod config;
pub mod creditlog;
pub mod error;
pub mod instance;
pub mod key;
pub mod market;
pub mod offer;
pub mod probe;
pub mod render;
pub mod runway;
pub mod trend;
pub mod until;

pub use error::{Result, SkoposError};

// The README's examples are compiled with the tests (`cargo test --doc`), so they cannot rot.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;
