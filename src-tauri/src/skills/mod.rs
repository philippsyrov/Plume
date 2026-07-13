//! Project-local manual skill library.
//!
//! Skills are inert Markdown documents. This module stores and inspects
//! them; it never adds them to prompts, tools, approvals, or execution.

mod parser;
mod promotion;
mod store;
mod types;
#[cfg(unix)]
mod unix;

pub use promotion::{
    promote_preview, promotion_context, SkillPromotionContext, SkillPromotionError,
    SkillPromotionPreview,
};
pub use store::{apply, list, load, preview};
pub use types::{
    SkillApplyResponse, SkillDocument, SkillIndex, SkillInput, SkillInvalid, SkillMetadata,
    SkillPreview, SkillsError,
};

pub const MAX_SKILLS: usize = 50;
pub const MAX_BODY_BYTES: usize = 12 * 1024;
pub const MAX_FILE_BYTES: usize = 16 * 1024;

#[cfg(test)]
mod tests;
