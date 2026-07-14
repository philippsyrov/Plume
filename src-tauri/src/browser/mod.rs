//! Sandboxed Browser Phase A foundations.

pub mod evidence;
#[allow(dead_code)] // Local capture commands consume this foundation in PR 2.
pub(crate) mod local_evidence;
#[cfg(target_os = "macos")]
pub mod native_snapshot;
pub mod policy;
#[allow(dead_code)] // The Browser persistence IPC consumes this module in Task 5.
pub(crate) mod restoration;
pub mod screenshot_evidence;
#[cfg(unix)]
mod screenshot_store_unix;
pub mod state;

#[cfg(test)]
mod authority_tests;

#[cfg(test)]
#[path = "evidence_tests.rs"]
mod evidence_tests;

#[cfg(test)]
#[path = "screenshot_evidence_tests.rs"]
mod screenshot_evidence_tests;

#[cfg(test)]
#[path = "restoration_tests.rs"]
mod restoration_tests;

#[cfg(test)]
#[path = "local_evidence_tests.rs"]
mod local_evidence_tests;
