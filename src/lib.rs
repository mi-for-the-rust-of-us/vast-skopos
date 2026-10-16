//! `vast-skopos`: a lookout (σκοπός) over your rented vast.ai boxes.
//!
//! **Not affiliated with vast.ai.** An independent tool that reads the public vast.ai API with
//! your own key, and your boxes over your own `ssh`.
//!
//! One table per call: for every instance, its status, `$/h`, age and cost so far, and its `ssh`
//! endpoint; under it, the account's credit, the burn rate and the hours that credit buys at that
//! rate. With `--ssh`, each running box is also asked directly, once, what its GPUs are doing
//! (`nvidia-smi`) and, with `--log`, which log it read and that log's last non-blank line: the
//! dashboard's GPU utilisation is not trusted, because it has been seen to read 0 % on boxes at
//! 97-99 %.
//!
//! The library half is what `vas` is built from:
//!
//! - [`key`]: where the API key is found (and that it is never printed);
//! - [`api`]: the two endpoints read (`/api/v1/instances/`, paginated, and
//!   `/api/v0/users/current`);
//! - [`instance`]: the lenient readers of their JSON;
//! - [`probe`]: the `ssh` probe and its `nvidia-smi` parser;
//! - [`render`]: the table.

pub mod api;
pub mod error;
pub mod instance;
pub mod key;
pub mod probe;
pub mod render;

pub use error::{Result, SkoposError};
