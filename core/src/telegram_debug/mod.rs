//! Experimental, local-only capture of Telegram Desktop 7.2.5 diagnostic logs.
//!
//! This never launches or authenticates a messenger client. The source is a
//! user-selected directory; only one explicitly bound account and peer are
//! allowed. Diagnostic transport records do not prove complete chat history or
//! application acceptance. No production connector is registered by this module.

pub mod capture;
pub mod parser;
