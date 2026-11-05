//! Minimal CalDAV/VTODO client.
//!
//! Built to back a `RemoteTaskProvider` implementation for Radicale (and any
//! other RFC 4791 server): discovery, VTODO CRUD guarded by ETags, and
//! incremental sync through `sync-collection`.

pub mod client;
pub mod error;
pub mod ical;
pub mod vtodo;
pub mod xml;

/// Helpers for integration tests against a real server.
#[doc(hidden)]
pub mod testing;

pub use client::{CalDavClient, Calendar, RemoteVtodo, SyncChanges};
pub use error::{CalDavError, Result};
pub use vtodo::{Vtodo, VtodoStatus};
