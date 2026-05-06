//! Path + command validation. The single chokepoint everything that
//! touches the filesystem must pass through.
//!
//! See `docs/SAFETY.md`. Slice B ships only the path module; command
//! approval and the secret redactor land later.

pub mod path;
