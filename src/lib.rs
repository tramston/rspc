//! rspc: A blazingly fast and easy to use TRPC-like server for Rust.
//!
//! Checkout the official docs <https://rspc.dev>
//!
#![forbid(unsafe_code)]
#![warn(
    clippy::all,
    clippy::cargo,
    clippy::unwrap_used,
    clippy::panic,
    clippy::todo,
    clippy::panic_in_result_fn,
    // missing_docs
)]
#![cfg_attr(docsrs, feature(doc_cfg))]

#[cfg(feature = "alpha")]
#[cfg_attr(docsrs, doc(cfg(feature = "alpha")))]
// #[deprecated = "Being removed in `v1.0.0`. This will be in the root of the crate."] // TODO
pub mod alpha;
// #[deprecated = "Being removed in `v1.0.0`. This will be in the root of the crate."] // TODO
pub(crate) mod alpha_stable;
mod config;
mod error;
mod middleware;
mod resolver_result;
mod router;
mod router_builder;
mod selection;

pub use config::*;
pub use error::*;
pub use middleware::*;
pub use resolver_result::*;
pub use router::*;
pub use router_builder::*;

pub mod integrations;
pub mod internal;
