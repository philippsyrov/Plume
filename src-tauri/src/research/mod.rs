//! Session-owned, provider-neutral research artifact workflow.

pub mod budget;
pub mod model;

#[cfg(test)]
#[path = "model_tests.rs"]
mod model_tests;
