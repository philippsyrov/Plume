//! D66: full-text session search over one scope's database.
//!
//! Rides the v2 FTS5 indexes (`titles_fts`, `messages_fts` — see
//! `schema.rs`). Scope separation is structural, exactly as for every
//! other store operation: this module receives a sessions *directory*
//! and can only ever see that one database — local and project rows
//! cannot meet in a single query because they never share a file.
//!
//! The user's query is treated as literal text, never as FTS5 query
//! syntax: each whitespace-separated term is quoted (with `"` doubled)
//! and given a `*` prefix-match suffix, so `NEAR(`, `OR`, `-`, or an
//! unbalanced quote search for those characters instead of erroring or
//! changing the query semantics.

use std::path::Path;

use rusqlite::params;
use serde::Serialize;

use super::{schema, store_lock, validation, SessionStoreError};

/// Results are bounded: at most this many sessions per search, however
/// many rows matched. The IPC layer may ask for fewer, never more.
pub(super) const MAX_SEARCH_RESULTS: usize = 20;

/// Content rows scanned per query before folding to sessions. A session
/// matches once per matching message; scanning a bounded, ranked prefix
/// keeps the fold cheap while leaving plenty of distinct sessions to
/// fill `MAX_SEARCH_RESULTS`.
const CONTENT_SCAN_LIMIT: usize = 200;

/// Snippet highlight markers. Private-use code points so they cannot
/// collide with meaningful transcript text; the frontend splits on
/// them to render highlights and NEVER shows them raw.
pub const SNIPPET_START: char = '\u{E000}';
pub const SNIPPET_END: char = '\u{E001}';

/// Tokens of context around a match inside a snippet (FTS5 `snippet()`
/// column budget).
const SNIPPET_TOKENS: i64 = 12;

/// One search result. `snippet` is present when the transcript matched
/// (with [`SNIPPET_START`]/[`SNIPPET_END`] around matched terms) and
/// absent for a title-only match — the title itself is the evidence.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub id: String,
    pub title: String,
    pub updated_at_ms: i64,
    pub archived_at_ms: Option<i64>,
    pub match_kind: SearchMatchKind,
    pub snippet: Option<String>,
}

/// What matched. `Title` also covers "title AND content" — the title
/// match is the stronger signal and such a hit still carries the
/// content snippet.
#[derive(Debug, Clone, Copy, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum SearchMatchKind {
    Title,
    Content,
}

/// Search one scope's database for sessions whose title or transcript
/// matches `raw_query`. Archived sessions are included (search is a
/// finding aid over ALL history) and identifiable via `archived_at_ms`.
/// Title matches order before content-only matches; within each group,
/// FTS rank (bm25) then recency then id keep the order total.
pub fn search(
    sessions_dir: &Path,
    raw_query: &str,
    limit: Option<usize>,
) -> Result<Vec<SearchHit>, SessionStoreError> {
    let match_expr = validation::build_fts_match(raw_query)?;
    let limit = match limit {
        None => MAX_SEARCH_RESULTS,
        Some(n) if (1..=MAX_SEARCH_RESULTS).contains(&n) => n,
        Some(n) => {
            return Err(SessionStoreError::Invalid(format!(
                "search limit {n} is outside 1..={MAX_SEARCH_RESULTS}"
            )));
        }
    };

    let lock = store_lock(sessions_dir);
    let _guard = lock.lock().expect("session store mutex poisoned");
    let conn = schema::open_connection(sessions_dir)?;

    // Title matches, best rank first.
    let mut stmt = conn
        .prepare(
            "SELECT s.id, s.title, s.updated_at_ms, s.archived_at_ms
             FROM titles_fts
             JOIN chat_sessions s ON s.rowid = titles_fts.rowid
             WHERE titles_fts MATCH ?1
             ORDER BY bm25(titles_fts), s.updated_at_ms DESC, s.id DESC
             LIMIT ?2",
        )
        .map_err(schema::storage("prepare title search"))?;
    let title_rows = stmt
        .query_map(params![match_expr, limit as i64], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, Option<i64>>(3)?,
            ))
        })
        .map_err(schema::storage("query title search"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(schema::storage("read title search row"))?;

    // Content matches, best rank first; folded to one row per session
    // below (a session matches once per matching message).
    let mut stmt = conn
        .prepare(&format!(
            "SELECT s.id, s.title, s.updated_at_ms, s.archived_at_ms,
                    snippet(messages_fts, 0, '{SNIPPET_START}', '{SNIPPET_END}', '…', {SNIPPET_TOKENS})
             FROM messages_fts
             JOIN chat_messages m ON m.rowid = messages_fts.rowid
             JOIN chat_sessions s ON s.id = m.session_id
             WHERE messages_fts MATCH ?1
             ORDER BY bm25(messages_fts), s.updated_at_ms DESC, s.id DESC
             LIMIT {CONTENT_SCAN_LIMIT}"
        ))
        .map_err(schema::storage("prepare content search"))?;
    let content_rows = stmt
        .query_map(params![match_expr], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, Option<i64>>(3)?,
                row.get::<_, String>(4)?,
            ))
        })
        .map_err(schema::storage("query content search"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(schema::storage("read content search row"))?;

    // Fold: best (first, thanks to rank order) snippet per session.
    let mut snippets: Vec<(String, String, i64, Option<i64>, String)> = Vec::new();
    for (id, title, updated, archived, snippet) in content_rows {
        if !snippets.iter().any(|(seen, ..)| *seen == id) {
            snippets.push((id, title, updated, archived, snippet));
        }
    }

    // Merge: title hits first (carrying a content snippet when the
    // transcript matched too), then content-only hits, capped.
    let mut hits: Vec<SearchHit> = Vec::new();
    for (id, title, updated_at_ms, archived_at_ms) in title_rows {
        let snippet = snippets
            .iter()
            .find(|(sid, ..)| *sid == id)
            .map(|(.., s)| s.clone());
        hits.push(SearchHit {
            id,
            title,
            updated_at_ms,
            archived_at_ms,
            match_kind: SearchMatchKind::Title,
            snippet,
        });
    }
    for (id, title, updated_at_ms, archived_at_ms, snippet) in snippets {
        if hits.len() >= limit {
            break;
        }
        if hits.iter().any(|h| h.id == id) {
            continue;
        }
        hits.push(SearchHit {
            id,
            title,
            updated_at_ms,
            archived_at_ms,
            match_kind: SearchMatchKind::Content,
            snippet: Some(snippet),
        });
    }
    hits.truncate(limit);
    Ok(hits)
}
