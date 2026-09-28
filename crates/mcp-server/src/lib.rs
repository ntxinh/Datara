//! MCP (Model Context Protocol) server exposing Datara's shared
//! [`DatabaseService`] over stdio. The GUI's `datara mcp-serve` subcommand
//! wires storage, secrets, and the MSSQL driver, then calls
//! [`serve_stdio`].
//!
//! MCP calls reuse the service's pooled `ConnectionId`-keyed sessions — the
//! same pooling design the GUI uses, not a second driver stack. (Separate
//! process, so pools aren't literally shared; connection caching applies
//! per-process.)

mod server;
mod tools;

pub use server::{serve_stdio, DataraMcp};
