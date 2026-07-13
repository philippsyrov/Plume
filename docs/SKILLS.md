# Project skills

Plume can store a small, project-local library of user-authored skill documents
under `.plume/skills/<slug>/SKILL.md`. This is progressive disclosure: the list
operation returns only each skill's slug, name, and description; full Markdown
is read only when the user opens one.

This first version is deliberately inert. A saved skill is not added to chat
context, advertised as a tool, granted permissions, or executed. Creating one
always follows preview then explicit apply, and apply never overwrites an
existing slug.

## Promote a project chat into a draft

`skills.promotionContext({ sessionId })` returns the eligible persisted messages
with their original transcript indexes and a snapshot token. A subsequent
`skills.promotePreview({ sessionId, entryIndexes, snapshotToken })` can turn selected,
completed messages from a persisted project chat into an editable draft. The
operation is preview-only: it writes neither the source session nor a skill.
It works for live or archived project sessions, but never reads local chats.
The trusted project and its `.plume/sessions` store are resolved server-side;
the payload cannot provide a root or scope.

The SHA-256 snapshot token covers the session title and complete serialized
transcript, including excluded entries and message metadata. Preview reloads
the source and refuses a stale token before selecting anything, so a transcript
replacement, reorder, or title rename cannot silently change what an index
means. The token is an integrity marker, not a secret or authorization grant.

The request accepts 1–20 unique zero-based transcript indexes. Plume restores
transcript order, rejects out-of-range, cancelled, and error entries, and runs
the prompt redactor over every selected message again. The body starts with an
HTML provenance comment naming the project session and human-readable 1-based
entry numbers, followed by quoted User/Assistant evidence. Generated name,
description, slug, body, and canonical file must pass the same limits as a
manual draft. The user must still edit/review the result and explicitly apply
it through the normal create-only skill flow.

## File shape

Each canonical file has strict frontmatter followed by Markdown:

```markdown
---
name: "Explain tests"
description: "Explains a focused test failure."
---

# Steps

Read the failure first.
```

The two values are JSON-quoted strings. Unknown, duplicate, unquoted, or
malformed frontmatter is rejected and shown in `skills.list().invalid`; invalid
entries are never silently hidden.

Storage is project-only and requires the currently open project to be trusted.
Slugs are lowercase ASCII words separated by single hyphens. Limits are 50
skills, 80 characters for name, 240 characters for description, 12 KiB for the
body, and 16 KiB for the complete file. On Unix, storage operations require an
absolute canonical project root, open `/`, and walk every normal root component
with descriptor-relative `openat(O_DIRECTORY | O_NOFOLLOW)`. The store then
uses `mkdirat`/`openat`/`linkat`/`unlinkat` through that held directory chain.
Even an intermediate-ancestor pathname swap cannot redirect a read or write.
The errno-backed directory iterator covers the libc ABIs used by Apple
(macOS/iOS/tvOS/watchOS/visionOS), Linux/Android, FreeBSD/DragonFly,
OpenBSD/NetBSD, Solaris/illumos, AIX, Haiku, Hurd, and Redox. Non-Unix platforms
return an explicit unsupported error until an equivalent descriptor-safe
implementation exists.

Creation writes and fsyncs an exclusively created sibling temporary file, then
atomically installs it with `linkat`, which fails instead of clobbering an
existing `SKILL.md`. It fsyncs the slug directory after the final link, unlinks
the temporary name, and fsyncs the directory again before reporting success.
Every newly-created directory is followed by an fsync of its parent. Symlinked
storage components and pre-existing hardlinked skill files are refused. Lists
and loads share the writer lock, so they cannot observe a half-created skill
directory or temporary file. If apply fails before linking the final file, it
removes the newly-created slug with descriptor-relative `unlinkat(AT_REMOVEDIR)`
and fsyncs the skills directory, allowing a clean retry. A non-empty slug is
never removed, so an externally appeared final file is preserved; the returned
error says explicitly when cleanup failed or the slug may remain reserved.
