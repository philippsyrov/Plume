//! Session-owned, provider-neutral research artifact workflow.

pub mod budget;
pub mod bundle;
pub mod citations;
pub mod context;
pub mod evidence;
pub mod export;
pub mod markdown;
pub mod model;
pub mod run;
pub(crate) mod run_registry;

#[cfg(test)]
#[path = "evidence_tests.rs"]
mod evidence_tests;

#[cfg(test)]
#[path = "export_tests.rs"]
mod export_tests;

#[cfg(test)]
#[path = "context_tests.rs"]
mod context_tests;

#[cfg(test)]
#[path = "citations_tests.rs"]
mod citations_tests;

#[cfg(test)]
#[path = "bundle_tests.rs"]
mod bundle_tests;

#[cfg(test)]
#[path = "markdown_tests.rs"]
mod markdown_tests;

#[cfg(test)]
#[path = "model_tests.rs"]
mod model_tests;

#[cfg(test)]
#[path = "run_tests.rs"]
mod run_tests;
