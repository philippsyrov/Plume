//! Session-owned, provider-neutral research artifact workflow.

pub mod budget;
pub mod context;
pub mod evidence;
pub mod model;

#[cfg(test)]
#[path = "evidence_tests.rs"]
mod evidence_tests;

#[cfg(test)]
#[path = "context_tests.rs"]
mod context_tests;

#[cfg(test)]
#[path = "model_tests.rs"]
mod model_tests;
