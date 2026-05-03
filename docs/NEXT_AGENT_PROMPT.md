# Next Agent Prompt

Use this prompt when handing Plume to Claude Code, Codex, or another coding
agent.

```text
You are working on Plume at:
/Users/philippsyrov/Desktop/CS Projects/Plume

First do:
pwd
sed -n '1,220p' AGENTS.md
sed -n '1,220p' docs/DEPENDENCY_ISOLATION.md
sed -n '1,220p' docs/DEVELOPMENT.md
git status --short

Rules:
- AGENTS.md is the project authority. Do not create CLAUDE.md.
- Do not install anything globally.
- Do not run npm install, cargo fetch, pip install, brew install, cargo install,
  rustup, or xcode-select unless the user explicitly approves it in this chat.
- When dependency commands are approved, run them through:
  ./scripts/dev-env.sh <command>
- Keep all dependency/model caches project-local:
  node_modules/, .cargo-home/, src-tauri/target/, .venv/, .cache/, .local/,
  plume-models/
- Before writing files, confirm you are still in:
  /Users/philippsyrov/Desktop/CS Projects/Plume
- If you need GitHub, check auth first with gh auth status. If auth is broken,
  stop and report it instead of creating paths on Desktop or guessing.
- Run ./scripts/verify.sh before handoff.

Current repo state:
- Git is initialized.
- `~/scripts/setup-tauri-project.sh` has already been run once.
- The shared CI, pre-commit hook, `.claude/`, `.agents/`, and `.gitattributes`
  bootstrap files already exist.
- Do not rerun the bootstrap unless you are intentionally checking drift.

Next useful tasks:
1. Add a package-lock.json only after approved dependency install:
   ./scripts/dev-env.sh npm install
2. Fetch Rust crates only after approved dependency fetch:
   ./scripts/dev-env.sh bash -lc 'cd src-tauri && cargo fetch'
3. If setting up MLX-LM, create .venv and install inside it:
   python3 -m venv .venv
   ./scripts/dev-env.sh bash -lc '. .venv/bin/activate && python -m pip install --upgrade pip mlx-lm'

Do not pretend the app builds until Node deps and Rust crates are actually
installed and ./scripts/verify.sh or the repo-native build proves it.
```

GitHub repo setup, if the remote is not already present:

```bash
cd "/Users/philippsyrov/Desktop/CS Projects/Plume"
gh auth status
./scripts/verify.sh
gh repo create plume --private --source=. --remote=origin --push
```
