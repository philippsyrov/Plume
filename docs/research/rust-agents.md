```research-metadata
{
  "family": "rust-agents",
  "sourceDate": "2026-07-13",
  "hygiene": "behavior-report-only",
  "sources": ["../CLAUDE_CODE_REFERENCE_NOTES.md", "https://github.com/graniet/claude-code-rs"],
  "refreshTrigger": "Completed license and provenance audit of a Rust agent reference"
}
```

# Rust Agent References

## Observed behavior

Public behavior reports and Rust reimplementation claims provide leads about
tool contracts, permissions, compaction, hooks, background work, diagnostics,
skills, orchestration, and recovery. A Rust rewrite or public repository is not
automatically clean-room, compatible with Plume's license, or safe to copy.

## Plume adaptation

Compare candidates by license, provenance claim, architecture, safety model,
recovery, memory, tools, provider support, and test quality. Recreate useful
behavior from public contracts in original Tauri/Rust designs only after the
source-hygiene judgment is explicit.

## Already shipped overlap

Plume already has Rust-owned trust gates, path-safe prompt reads, atomic patch
apply/revert, supervised MLX processes, persisted sessions, bounded memory,
and tested controller scaffolding. These are repo-native implementations, not
evidence that an external Rust agent has been integrated.

## Remaining gap

A pinned comparison table and dedicated license/provenance audit are still
needed before any external Rust agent can move beyond behavior-level research.
The real bounded executor and recovery loop also remain unshipped.

## Rejected or deferred

Do not copy leaked Claude Code source, translate proprietary implementation
text, vendor ambiguously derived code, or treat a clean-room claim as proof.
GPL or otherwise incompatible code remains observe-only unless a deliberate
licensing decision changes that boundary.
