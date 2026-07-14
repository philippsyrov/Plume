//! Sandboxed Browser Phase A foundations.

pub mod evidence;
#[cfg(target_os = "macos")]
pub mod native_snapshot;
pub mod policy;
#[allow(dead_code)] // Consumed by Browser workspace persistence in this campaign.
mod restoration;
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
