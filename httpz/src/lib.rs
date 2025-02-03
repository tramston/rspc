//! Code once, support every Rust webserver!
#![forbid(unsafe_code)]
#![warn(missing_docs, clippy::unwrap_used)]

mod endpoint;
mod error;
mod generic_endpoint;
mod request;
mod response;
mod server;
mod servers;

/// is the module containing code related to handling incoming websockets.
pub mod ws;

pub use endpoint::*;
pub use error::*;
pub use form_urlencoded;
pub use generic_endpoint::*;
pub use http;
pub use request::*;
pub use response::*;
pub use server::*;
pub use servers::*;
