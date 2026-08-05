//! `nahida-agent` — the loop, and nothing else.
//!
//! This crate knows about turns, tools, and cancellation. It does not know what
//! a file is (that is `nahida-tools`) or what a terminal is (that is
//! `nahida-cli`). Tools plug in through the [`Tool`] trait; progress comes out
//! through [`AgentEvent`].

pub mod agent;
pub mod cancel;
pub mod event;
pub mod tool;

pub use agent::{Agent, AgentError, Outcome};
pub use cancel::Cancel;
pub use event::AgentEvent;
pub use tool::{Tool, ToolOutcome, optional_u64, optional_usize, required_str};
