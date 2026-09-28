//! MCP (Model Context Protocol) server exposing Datara's shared
//! [`DatabaseService`] over stdio. The GUI's `datara mcp-serve` subcommand
//! wires storage, secrets, and the MSSQL driver, then calls
//! [`serve_stdio`].
//!
//! Sessions are the service's pooled `ConnectionId`-keyed sessions, shared
//! with the GUI process when both run — no second driver stack.

mod server;
mod tools;

pub use server::{serve_stdio, DataraMcp};
