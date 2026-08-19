//! Container analysis: what a probe works out, as data rather than as output.
//!
//! [`probe_report`] returns a [`ProbeReport`]; rendering it is the caller's job.
//! [`sampling`] holds the scan-plan helpers a deep probe uses to read a
//! representative subset of a large pyramid.

mod report;
pub mod sampling;

pub use report::*;
