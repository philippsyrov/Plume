```research-metadata
{
  "family": "qoder-notion",
  "sourceDate": "2026-07-13",
  "hygiene": "official-public",
  "sources": ["https://docs.qoder.com/", "https://qoder.com/en", "https://www.notion.com/help/custom-agents"],
  "refreshTrigger": "Meaningful upstream knowledge, agent, or permission-model release"
}
```

# Qoder And Notion

## Observed behavior

Qoder's public product and documentation present a task workspace plus a
Knowledge Engine and Repo Wiki. Notion's official custom-agent documentation
describes Chat, Activity, and Settings surfaces, explicit resource permissions,
triggers, logs, version history, and revert. These are capability observations,
not proof of either product's private implementation.

## Plume adaptation

Use a calm Knowledge workspace over Plume's own bounded memories and curated
topics. Context placement must be explicit: a user drags or chooses a typed
reference, the visible shelf names its source, scope, and provenance, and the
backend resolves and rechecks it at send time. Drag/drop must never smuggle in
frontend-supplied prompt text or trigger hidden retrieval. Removing an item
removes it from the next send.

## Already shipped overlap

Plume ships redacted memory entries, curated topic reads, exact prompt-context
manifests, organization-only memory links, persisted sessions, and reversible
patch application. It does not yet ship the Knowledge workspace or explicit
context shelf.

## Remaining gap

Read-only topic navigation, backlinks, unlinked-memory views, explicit
`Use in chat` and drag/drop provenance, blocked-source recovery, and persisted
turn-level source placement remain to be built.

## Rejected or deferred

Do not copy either product's branding or Electron bundle. Do not grant ambient
authority because a page, topic, or memory is linked. Automatic or semantic
retrieval waits for visible preview, evaluation evidence, and explicit user
control.
