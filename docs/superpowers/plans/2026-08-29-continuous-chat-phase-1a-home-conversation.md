# Phase 1A — Durable Home conversation

Slice plan for the first half of campaign Phase 1. Companion to the
[campaign plan](2026-08-27-continuous-chat-folder-grants-campaign.md) and the
[design](../specs/2026-08-27-continuous-chat-folder-grants-design.md).

## Why this is two slices, not one

Campaign Phase 1 carries six checkboxes covering two separable outcomes: a
durable Home conversation, and a durable storage cap. They share a store but
nothing else — Home is identity and routing, the cap is a refusal path with its
own recovery surface. Shipping them together would produce one diff spanning a
schema migration, startup routing, a frontend surface, an error path, and a
packaged smoke, with no reviewable middle.

- **1A (this plan)** — Home identity, routing, relaunch restoration.
- **1B** — the storage cap: warn, refuse appends, recover through review,
  export, and explicit deletion.

1B lands second because its refusal path has to name the store Home actually
uses. Neither slice ships a consumer authority change.

## Verified baseline

Measured on this head, not assumed:

- `SCHEMA_VERSION` is `6` (`src-tauri/src/sessions/schema.rs:24`). Migrations run
  `v1→v6`, each in one transaction that stamps `PRAGMA user_version` last, so a
  crash mid-migration leaves the old version and the next open retries.
- The local store is app-private and resolved once at startup:
  `AppState.local_sessions_dir` is `<app-data>/sessions`
  (`src-tauri/src/commands/project.rs`), explicitly *not* derived from any open
  project.
- `sessions::create` already refuses past `validation::MAX_SESSIONS` with
  `SessionStoreError::Limit` rather than evicting. **This is the precedent 1B
  extends** — the store already fails closed on a cap instead of trimming.
- Fourteen `sessions_*` / `session_*` IPC verbs exist
  (`src-tauri/src/app_commands.rs:84`). Phase 1A adds none.

## What Home is

One backend-owned conversation in the app-private local store, with a stable
identity that survives relaunch. It is an ordinary `chat_sessions` row — not a
parallel store — so fork, rewind, archive, deletion, accepted-turn manifests,
Browser ownership, cancellation, and streaming boundaries all keep working
through the paths that already handle them.

What makes it Home is a single marker the backend owns and the frontend cannot
set.

## Task 1 — Home identity in the store

**Schema v7 migration.** Add a `home` marker to `chat_sessions`:

```sql
ALTER TABLE chat_sessions ADD COLUMN is_home INTEGER NOT NULL DEFAULT 0;
CREATE UNIQUE INDEX chat_sessions_home_idx ON chat_sessions(is_home) WHERE is_home = 1;
```

The partial unique index is the load-bearing part: it makes "at most one Home"
a database invariant rather than a convention every call site has to remember.
A second Home cannot be inserted even by a bug.

Follow the existing migration shape exactly — one transaction, `user_version`
stamped last.

**`sessions::home(sessions_dir) -> Result<SessionSummary, SessionStoreError>`.**
Returns the Home row, creating it on first call. Idempotent under concurrency:
the whole read-or-create runs inside the existing store lock plus one
transaction, so two simultaneous callers cannot produce two Homes — one inserts,
the other reads what it inserted.

Home is exempt from `MAX_SESSIONS`. The cap exists to stop unbounded session
growth; refusing to create the one conversation the app opens into would make a
full store unusable rather than merely full.

**Tests (write first, watch fail):**

- creates on first call, returns the same id on the second;
- concurrent callers get one id, and the store holds exactly one Home row;
- Home survives close and reopen of the connection;
- a direct insert of a second `is_home = 1` row is refused by the index;
- Home is returned by `load`, `list`, `save_transcript`, `fork`, and `rollback`
  like any other session — no special-casing leaks into those paths;
- Home is creatable when the store is at `MAX_SESSIONS`.

## Task 2 — Routing startup and no-folder chat to Home

**Backend.** `sessions_load` and `sessions_list` keep their current contracts.
Add nothing to the IPC surface: the frontend asks for Home through the existing
`sessions_load` verb with a backend-resolved id, obtained at startup.

The exact wire shape is settled in implementation, but the invariant is binding:
**the frontend never supplies the Home id from its own storage.** It receives it
from the backend each launch. A caller-supplied Home id would be a caller-chosen
conversation identity, which is the same mistake as a caller-supplied filesystem
root.

**Frontend.** Startup with no open project routes to Home instead of an empty
chat. The existing local/project session APIs and stores stay reachable and
unchanged — Projects remains a working compatibility path for this whole phase.

**Tests:**

- startup with no project renders Home with its prior chronology;
- a message sent with no folder open lands in Home and is there after reload;
- opening a project still routes to project sessions, unchanged;
- closing a project returns to Home, not to an empty chat;
- an existing local session created before this slice still loads.

## Task 3 — Relaunch restoration and packaged smoke

Repeated relaunch returns to the same visible chronology without opening or
trusting a folder. Packaged smoke per `docs/SMOKE_TESTING.md`, because startup
routing and a native window are exactly what a headless suite cannot prove.

Record the smoke evidence in `docs/history/continuous-chat-campaign.md` against
this slice's exact head.

## Not in this slice

- The storage cap and its refusal path — 1B.
- Compaction, memory proposals, folder grants, run leases — Phases 2, 3, 4, 6.
- Removing Projects from navigation — Phase 5.
- Any change to what a conversation is allowed to do. Home is a place to talk,
  not a new authority.

## Slice verification

1. Focused Rust tests for `sessions::home` and the v7 migration.
2. Focused frontend tests for startup routing.
3. `cd src-tauri && cargo test` — no existing test changed.
4. `npm run test`.
5. `PLUME_FULL_VERIFY=1 ./scripts/verify.sh` — no failures.
6. Packaged smoke for relaunch chronology.
7. `docs/IPC_CONTRACT.md`, `docs/ARCHITECTURE.md`, `docs/STATE_OWNERSHIP.md`,
   and `docs/FEATURE_INVENTORY.md` updated to this exact head.
8. Findings-only exact-head review; resolve every important finding.
9. Pre-commit and gitleaks; GitHub verify green before merge.

`docs/FEATURE_INVENTORY.md` gains a record here — unlike Phase 0, this slice
ships behaviour a user can see.
