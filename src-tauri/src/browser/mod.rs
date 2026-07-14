//! Sandboxed Browser Phase A foundations.

pub mod evidence;
#[cfg(target_os = "macos")]
pub mod native_snapshot;
pub mod policy;
pub mod screenshot_evidence;
pub mod state;

#[cfg(test)]
mod authority_tests;

#[cfg(test)]
#[path = "evidence_tests.rs"]
mod evidence_tests;

#[cfg(test)]
#[path = "screenshot_evidence_tests.rs"]
mod screenshot_evidence_tests;
