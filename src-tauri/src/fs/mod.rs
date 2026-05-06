//! Display filesystem reads. Display ≠ prompt: see
//! `docs/ARCHITECTURE.md` § Display reads vs prompt reads. The prompt
//! path lives elsewhere and produces `RedactedContent` through its own
//! chokepoint.

pub mod list;
pub mod policy;
pub mod read;

pub use list::{list_dir, resolve, FileEntry};
pub use read::{read_file, FileContent};
