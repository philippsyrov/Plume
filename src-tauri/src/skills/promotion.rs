use serde::Serialize;
use sha2::{Digest, Sha256};

use super::{parser, SkillInput, SkillsError};
use crate::sessions::{self, EntryRole, TranscriptEntry};

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillPromotionContextEntry {
    pub index: u32,
    pub role: EntryRole,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillPromotionContext {
    pub session_id: String,
    pub title: String,
    pub snapshot_token: String,
    pub entries: Vec<SkillPromotionContextEntry>,
    pub excluded_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillPromotionDraft {
    pub slug: String,
    pub name: String,
    pub description: String,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillPromotionSource {
    pub session_id: String,
    pub title: String,
    pub entry_indexes: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillPromotionPreview {
    pub draft: SkillPromotionDraft,
    pub source: SkillPromotionSource,
    pub redaction_count: usize,
}

#[derive(Debug, thiserror::Error)]
pub enum SkillPromotionError {
    #[error(transparent)]
    Session(#[from] sessions::SessionStoreError),
    #[error(transparent)]
    Skill(#[from] SkillsError),
    #[error("session changed since the promotion context was loaded")]
    SnapshotMismatch,
}

pub fn promotion_context(
    session: &sessions::SessionRecord,
) -> Result<SkillPromotionContext, SkillPromotionError> {
    let snapshot_token = snapshot_token(session)?;
    let entries = session
        .entries
        .iter()
        .enumerate()
        .filter_map(|(index, entry)| match entry {
            TranscriptEntry::Message { message, .. } => Some(SkillPromotionContextEntry {
                index: index as u32,
                role: message.role,
                content: message.content.clone(),
            }),
            TranscriptEntry::Cancelled { .. } | TranscriptEntry::Error { .. } => None,
        })
        .collect::<Vec<_>>();
    Ok(SkillPromotionContext {
        session_id: session.id.clone(),
        title: session.title.clone(),
        snapshot_token,
        excluded_count: session.entries.len() - entries.len(),
        entries,
    })
}

pub fn promote_preview(
    session: &sessions::SessionRecord,
    entry_indexes: &[u32],
    expected_snapshot_token: &str,
) -> Result<SkillPromotionPreview, SkillPromotionError> {
    if entry_indexes.is_empty() || entry_indexes.len() > 20 {
        return Err(
            SkillsError("entryIndexes must contain between 1 and 20 indexes".into()).into(),
        );
    }
    let mut indexes = entry_indexes.to_vec();
    indexes.sort_unstable();
    if indexes.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(SkillsError("entryIndexes must not contain duplicates".into()).into());
    }

    let current_token = snapshot_token(session)?;
    if !same_token(&current_token, expected_snapshot_token) {
        return Err(SkillPromotionError::SnapshotMismatch);
    }
    let name = suggested_name(&session.title);
    let slug = suggested_slug(&session.title);
    let description = format!(
        "Workflow draft promoted from project chat “{}”.",
        session.title
    );
    let entry_numbers = indexes
        .iter()
        .map(|index| (u64::from(*index) + 1).to_string())
        .collect::<Vec<_>>()
        .join(", ");
    let mut body = format!(
        "<!-- Plume provenance: project session {}; entries {} -->\n# Workflow draft\n\n> Review this transcript evidence and edit it into clear, reusable steps before saving.\n",
        session.id, entry_numbers
    );
    let mut redaction_count = 0usize;
    for index in &indexes {
        let entry = session
            .entries
            .get(*index as usize)
            .ok_or_else(|| SkillsError(format!("entry index {index} is out of range")))?;
        let (role, content) = match entry {
            TranscriptEntry::Message { message, .. } => {
                let role = match message.role {
                    EntryRole::User => "User",
                    EntryRole::Assistant => "Assistant",
                };
                (role, &message.content)
            }
            TranscriptEntry::Cancelled { .. } | TranscriptEntry::Error { .. } => {
                return Err(
                    SkillsError(format!("entry index {index} is not a completed message")).into(),
                )
            }
        };
        let (redacted, spans) = crate::prompts::redact::redact(content);
        redaction_count += spans.len();
        body.push_str(&format!(
            "\n## {role}\n\n{}\n",
            blockquote(&escape_html(&redacted))
        ));
    }

    let input = SkillInput {
        slug: slug.clone(),
        name: name.clone(),
        description: description.clone(),
        body: body.clone(),
    };
    parser::canonical(&input)?;
    Ok(SkillPromotionPreview {
        draft: SkillPromotionDraft {
            slug,
            name,
            description,
            body,
        },
        source: SkillPromotionSource {
            session_id: session.id.clone(),
            title: session.title.clone(),
            entry_indexes: indexes,
        },
        redaction_count,
    })
}

fn snapshot_token(session: &sessions::SessionRecord) -> Result<String, SkillsError> {
    let bytes = serde_json::to_vec(&(session.title.as_str(), &session.entries))
        .map_err(|error| SkillsError(format!("serialize session snapshot: {error}")))?;
    Ok(format!("sha256:{:x}", Sha256::digest(bytes)))
}

fn same_token(actual: &str, expected: &str) -> bool {
    actual.len() == expected.len()
        && actual
            .as_bytes()
            .iter()
            .zip(expected.as_bytes())
            .fold(0u8, |difference, (left, right)| difference | (left ^ right))
            == 0
}

fn suggested_name(title: &str) -> String {
    let suffix = " workflow";
    let keep = 80 - suffix.chars().count();
    let mut base = title.chars().take(keep).collect::<String>();
    while base.ends_with(char::is_whitespace) {
        base.pop();
    }
    format!("{base}{suffix}")
}

fn suggested_slug(title: &str) -> String {
    let mut slug = String::new();
    let mut pending_dash = false;
    for ch in title.chars() {
        if ch.is_ascii_alphanumeric() {
            if pending_dash && !slug.is_empty() && slug.len() < 48 {
                slug.push('-');
            }
            pending_dash = false;
            if slug.len() < 48 {
                slug.push(ch.to_ascii_lowercase());
            }
        } else {
            pending_dash = true;
        }
        if slug.len() >= 48 {
            break;
        }
    }
    while slug.ends_with('-') {
        slug.pop();
    }
    if slug.is_empty() {
        "chat-workflow".into()
    } else {
        slug
    }
}

fn blockquote(content: &str) -> String {
    content
        .split('\n')
        .map(|line| {
            if line.is_empty() {
                ">".into()
            } else {
                format!("> {line}")
            }
        })
        .collect::<Vec<String>>()
        .join("\n")
}

fn escape_html(content: &str) -> String {
    content
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sessions::{self, EntryMessage, EntryRole, TranscriptEntry};

    fn temp_dir() -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "plume-skill-promotion-{}",
            crate::project::mint_id()
        ))
    }

    fn message(role: EntryRole, content: &str) -> TranscriptEntry {
        TranscriptEntry::Message {
            message: EntryMessage {
                role,
                content: content.into(),
            },
            model_used: None,
            duration_ms: None,
            attachment_rel_path: None,
            attachment_line_range: None,
            stats: None,
            sent_in_mode: None,
            context_sources: None,
        }
    }

    fn preview(
        dir: &std::path::Path,
        session_id: &str,
        indexes: &[u32],
    ) -> Result<SkillPromotionPreview, SkillPromotionError> {
        let session = sessions::load(dir, session_id).unwrap();
        let token = promotion_context(&session).unwrap().snapshot_token;
        promote_preview(&session, indexes, &token)
    }

    #[test]
    fn canonicalizes_order_redacts_and_preserves_source() {
        let dir = temp_dir();
        let session = sessions::create(&dir, Some("Déploy API 🚀")).unwrap();
        let secret = "sk-abcdefghijklmnopqrstuvwxyz";
        let entries = vec![
            message(EntryRole::User, &format!("first\n{secret}")),
            message(EntryRole::Assistant, "second\nline"),
        ];
        sessions::save_transcript(&dir, &session.id, &entries, true).unwrap();

        let preview = preview(&dir, &session.id, &[1, 0]).unwrap();
        assert_eq!(preview.source.entry_indexes, vec![0, 1]);
        assert_eq!(preview.source.title, "Déploy API 🚀");
        assert_eq!(preview.draft.slug, "d-ploy-api");
        assert_eq!(preview.draft.name, "Déploy API 🚀 workflow");
        assert!(preview.draft.body.starts_with(&format!(
            "<!-- Plume provenance: project session {}; entries 1, 2 -->\n# Workflow draft",
            session.id
        )));
        assert!(preview.draft.body.contains("> first\n> [REDACTED:api-key]"));
        assert!(preview
            .draft
            .body
            .contains("## Assistant\n\n> second\n> line"));
        assert!(!preview.draft.body.contains(secret));
        assert_eq!(preview.redaction_count, 1);
        assert_eq!(sessions::load(&dir, &session.id).unwrap().entries, entries);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn rejects_invalid_selections_and_non_messages() {
        let dir = temp_dir();
        let session = sessions::create(&dir, None).unwrap();
        sessions::save_transcript(
            &dir,
            &session.id,
            &[
                message(EntryRole::User, "ok"),
                TranscriptEntry::Error {
                    message: "bad".into(),
                },
            ],
            true,
        )
        .unwrap();
        for indexes in [vec![], vec![0, 0], vec![2], vec![1]] {
            assert!(preview(&dir, &session.id, &indexes).is_err(), "{indexes:?}");
        }
        assert!(preview(&dir, &session.id, &(0..21).collect::<Vec<_>>()).is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn archived_sessions_are_allowed_and_unicode_only_title_falls_back() {
        let dir = temp_dir();
        let session = sessions::create(&dir, Some("你好 🚀")).unwrap();
        sessions::save_transcript(
            &dir,
            &session.id,
            &[message(EntryRole::User, "hello")],
            true,
        )
        .unwrap();
        sessions::set_archived(&dir, &session.id, true).unwrap();
        let preview = preview(&dir, &session.id, &[0]).unwrap();
        assert_eq!(preview.draft.slug, "chat-workflow");
        assert_eq!(preview.source.title, "你好 🚀");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn generated_draft_obeys_existing_body_cap() {
        let dir = temp_dir();
        let session = sessions::create(&dir, None).unwrap();
        sessions::save_transcript(
            &dir,
            &session.id,
            &[message(EntryRole::User, &"x".repeat(13_000))],
            true,
        )
        .unwrap();
        assert!(preview(&dir, &session.id, &[0])
            .unwrap_err()
            .to_string()
            .contains("body exceeds"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn response_uses_camel_case_and_escapes_transcript_html() {
        let dir = temp_dir();
        let session = sessions::create(&dir, Some("Wire")).unwrap();
        sessions::save_transcript(
            &dir,
            &session.id,
            &[message(EntryRole::User, "<script>x</script>")],
            true,
        )
        .unwrap();
        let preview = preview(&dir, &session.id, &[0]).unwrap();
        let wire = serde_json::to_value(&preview).unwrap();
        assert_eq!(wire["source"]["sessionId"], session.id);
        assert_eq!(wire["source"]["entryIndexes"], serde_json::json!([0]));
        assert_eq!(wire["redactionCount"], 0);
        assert!(wire["draft"]["body"]
            .as_str()
            .unwrap()
            .contains("&lt;script&gt;x&lt;/script&gt;"));
        assert!(wire.get("redaction_count").is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn snapshot_token_rejects_replaced_reordered_or_renamed_source() {
        let dir = temp_dir();
        let session = sessions::create(&dir, Some("Original")).unwrap();
        let original = vec![
            message(EntryRole::User, "one"),
            message(EntryRole::Assistant, "two"),
        ];
        sessions::save_transcript(&dir, &session.id, &original, true).unwrap();
        let loaded = sessions::load(&dir, &session.id).unwrap();
        let context = promotion_context(&loaded).unwrap();
        assert_eq!(context.entries.len(), 2);
        assert!(promote_preview(&loaded, &[0], &context.snapshot_token).is_ok());

        let reordered = vec![original[1].clone(), original[0].clone()];
        sessions::save_transcript(&dir, &session.id, &reordered, true).unwrap();
        let changed = sessions::load(&dir, &session.id).unwrap();
        assert!(matches!(
            promote_preview(&changed, &[0], &context.snapshot_token),
            Err(SkillPromotionError::SnapshotMismatch)
        ));

        sessions::save_transcript(&dir, &session.id, &original, true).unwrap();
        sessions::rename(&dir, &session.id, "Renamed").unwrap();
        let renamed = sessions::load(&dir, &session.id).unwrap();
        assert!(matches!(
            promote_preview(&renamed, &[0], &context.snapshot_token),
            Err(SkillPromotionError::SnapshotMismatch)
        ));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn promotion_context_keeps_original_indexes_and_counts_excluded_entries() {
        let dir = temp_dir();
        let session = sessions::create(&dir, None).unwrap();
        sessions::save_transcript(
            &dir,
            &session.id,
            &[
                message(EntryRole::User, "one"),
                TranscriptEntry::Cancelled {
                    partial: "partial".into(),
                    model_used: None,
                    duration_ms: None,
                },
                message(EntryRole::Assistant, "three"),
            ],
            true,
        )
        .unwrap();
        let loaded = sessions::load(&dir, &session.id).unwrap();
        let context = promotion_context(&loaded).unwrap();
        assert_eq!(context.entries[0].index, 0);
        assert_eq!(context.entries[1].index, 2);
        assert_eq!(context.excluded_count, 1);
        assert!(context.snapshot_token.starts_with("sha256:"));
        let _ = std::fs::remove_dir_all(dir);
    }
}
