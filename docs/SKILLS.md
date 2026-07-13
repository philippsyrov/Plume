# Project skills

Plume can store a small, project-local library of user-authored skill documents
under `.plume/skills/<slug>/SKILL.md`. This is progressive disclosure: the list
operation returns only each skill's slug, name, and description; full Markdown
is read only when the user opens one.

This first version is deliberately inert. A saved skill is not added to chat
context, advertised as a tool, granted permissions, or executed. Creating one
always follows preview then explicit apply, and apply never overwrites an
existing slug.

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
