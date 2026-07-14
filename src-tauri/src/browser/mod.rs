//! Sandboxed Browser Phase A foundations.

pub mod evidence;
pub mod policy;
pub mod state;

#[cfg(test)]
mod authority_tests;

#[cfg(test)]
#[path = "evidence_tests.rs"]
mod evidence_tests;
