# Tauri / Rust Desktop Rules

## Dependency discipline

- Do not run install commands without approval.
- Do not add npm or Rust dependencies unless the existing stack cannot solve the problem cleanly.
- Prefer project-native scripts and existing config.

## Tauri boundaries

- Frontend code asks the backend for filesystem, process, and shell actions.
- Rust backend validates paths, command arguments, and process ownership.
- Never expose a broad "run any command" frontend API without an approval layer.

## Verification

- Run `./scripts/verify.sh` before handoff if files changed.
- If verification fails because dependencies are not installed, report that clearly instead of installing them.
