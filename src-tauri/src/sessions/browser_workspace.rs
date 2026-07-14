//! Durable Browser-workspace domain records.
//!
//! Live WebKit views do not belong here. This module describes only the
//! bounded, restorable state owned by one persisted chat session. The
//! command layer supplies `scope`; physical local/project separation is
//! still enforced by choosing the correct session database.

use std::collections::HashSet;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};

use crate::browser::restoration::{admit_restorable_url, HISTORY_CAP};

use super::{now_ms, schema, store_lock, validation, SessionStoreError};

pub(super) const MIN_SPLIT_WIDTH_PX: i64 = 320;
pub(super) const MAX_SPLIT_WIDTH_PX: i64 = 1_600;
pub(super) const DEFAULT_SPLIT_WIDTH_PX: i64 = 560;
pub(super) const MAX_TABS: usize = 5;
pub(super) const MAX_HISTORY_ROWS: usize = HISTORY_CAP;

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum BrowserWorkspaceScope {
    Local,
    Project,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum BrowserLayoutMode {
    Split,
    Expanded,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum BrowserRestorationStatus {
    Blank,
    Restorable,
    ManualReopenRequired,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum BrowserWorkspaceRecovery {
    BrowserStateReset,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BrowserHistoryNavigation {
    New,
    Back,
    Forward,
    Reload,
    Restore,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BrowserHistoryRecord {
    pub position: usize,
    pub url: String,
    pub recorded_at_ms: i64,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BrowserTabRecord {
    pub id: String,
    pub position: usize,
    pub current_history_index: Option<usize>,
    pub manual_reopen_required: bool,
    pub restoration_status: BrowserRestorationStatus,
    pub history: Vec<BrowserHistoryRecord>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BrowserWorkspaceRecord {
    pub session_id: String,
    pub scope: BrowserWorkspaceScope,
    pub layout_mode: BrowserLayoutMode,
    pub split_width_px: i64,
    pub active_tab_id: Option<String>,
    pub tabs: Vec<BrowserTabRecord>,
    pub recovery: Option<BrowserWorkspaceRecovery>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrowserWorkspaceLoad {
    Missing,
    Ready(BrowserWorkspaceRecord),
    ResetCorrupt { reason: String },
}

pub(super) fn validate_split_width_px(value: i64) -> Result<i64, SessionStoreError> {
    if (MIN_SPLIT_WIDTH_PX..=MAX_SPLIT_WIDTH_PX).contains(&value) {
        Ok(value)
    } else {
        Err(SessionStoreError::Invalid(format!(
            "browser split width must be between {MIN_SPLIT_WIDTH_PX} and {MAX_SPLIT_WIDTH_PX} pixels"
        )))
    }
}

#[allow(dead_code)] // The integrated Browser runtime mints workspace handles in PR 2.
pub fn mint_workspace_id() -> String {
    mint_browser_id("bw_")
}

pub fn mint_tab_id() -> String {
    mint_browser_id("bt_")
}

fn mint_browser_id(prefix: &str) -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos() as u64)
        .unwrap_or(0);
    format!("{prefix}{nanos:016x}{:08x}{n:08x}", std::process::id())
}

/// Load one session's Browser descriptors. A malformed Browser subtree
/// is deleted and reported independently; it never makes the chat
/// transcript unreadable. A session absent from this physical database
/// remains a plain `NotFound`, which is the local/project scope fence.
pub fn load_browser_workspace(
    sessions_dir: &Path,
    session_id: &str,
    scope: BrowserWorkspaceScope,
) -> Result<BrowserWorkspaceLoad, SessionStoreError> {
    validation::validate_id(session_id)?;
    let lock = store_lock(sessions_dir);
    let _guard = lock.lock().expect("session store mutex poisoned");
    let mut conn = schema::open_connection(sessions_dir)?;
    require_session(&conn, session_id)?;
    match read_workspace(&conn, session_id, scope) {
        Ok(Some(record)) => Ok(BrowserWorkspaceLoad::Ready(record)),
        Ok(None) => Ok(BrowserWorkspaceLoad::Missing),
        Err(
            SessionStoreError::Corrupt(_)
            | SessionStoreError::Invalid(_)
            | SessionStoreError::Limit(_),
        ) => {
            let tx = conn
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(schema::storage("begin corrupt browser reset"))?;
            tx.execute(
                "DELETE FROM browser_workspaces WHERE session_id=?1",
                params![session_id],
            )
            .map_err(schema::storage("reset corrupt browser workspace"))?;
            tx.commit()
                .map_err(schema::storage("commit corrupt browser reset"))?;
            Ok(BrowserWorkspaceLoad::ResetCorrupt {
                reason: "invalidBrowserState".into(),
            })
        }
        Err(other) => Err(other),
    }
}

/// Atomically replace the complete normalized Browser subtree. The
/// incoming record is validated and bounded before the transaction, so
/// a rejected save cannot disturb the previous durable workspace.
pub fn replace_browser_workspace(
    sessions_dir: &Path,
    session_id: &str,
    scope: BrowserWorkspaceScope,
    record: &BrowserWorkspaceRecord,
) -> Result<BrowserWorkspaceRecord, SessionStoreError> {
    validation::validate_id(session_id)?;
    let normalized = normalize_for_save(session_id, scope, record)?;
    let lock = store_lock(sessions_dir);
    let _guard = lock.lock().expect("session store mutex poisoned");
    let mut conn = schema::open_connection(sessions_dir)?;
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(schema::storage("begin browser workspace replace"))?;
    require_session(&tx, session_id)?;
    write_workspace(&tx, &normalized)?;
    tx.commit()
        .map_err(schema::storage("commit browser workspace replace"))?;
    Ok(normalized)
}

fn write_workspace(
    tx: &rusqlite::Transaction<'_>,
    normalized: &BrowserWorkspaceRecord,
) -> Result<(), SessionStoreError> {
    tx.execute(
        "DELETE FROM browser_workspaces WHERE session_id=?1",
        params![normalized.session_id],
    )
    .map_err(schema::storage("clear browser workspace"))?;
    tx.execute(
        "INSERT INTO browser_workspaces
         (session_id,layout_mode,split_width_px,active_tab_id,updated_at_ms)
         VALUES (?1,?2,?3,?4,?5)",
        params![
            normalized.session_id,
            layout_to_sql(normalized.layout_mode),
            normalized.split_width_px,
            normalized.active_tab_id,
            now_ms()
        ],
    )
    .map_err(schema::storage("insert browser workspace"))?;
    for tab in &normalized.tabs {
        tx.execute(
            "INSERT INTO browser_tabs
             (id,session_id,position,current_history_index,manual_reopen_required)
             VALUES (?1,?2,?3,?4,?5)",
            params![
                tab.id,
                normalized.session_id,
                usize_to_i64(tab.position, "tab position")?,
                current_index_to_sql(tab.current_history_index)?,
                i64::from(tab.manual_reopen_required)
            ],
        )
        .map_err(schema::storage("insert browser tab"))?;
        for history in &tab.history {
            tx.execute(
                "INSERT INTO browser_history (tab_id,position,url,recorded_at_ms)
                 VALUES (?1,?2,?3,?4)",
                params![
                    tab.id,
                    usize_to_i64(history.position, "history position")?,
                    history.url,
                    history.recorded_at_ms
                ],
            )
            .map_err(schema::storage("insert browser history"))?;
        }
    }
    Ok(())
}

/// Commit one finished native top-level navigation into Plume's bounded
/// restoration history. The native back-forward list remains WebKit-owned;
/// Plume records only admitted URLs and an explicit current index.
pub(crate) fn commit_browser_navigation(
    sessions_dir: &Path,
    session_id: &str,
    scope: BrowserWorkspaceScope,
    tab_id: &str,
    raw_url: &str,
    navigation: BrowserHistoryNavigation,
) -> Result<BrowserWorkspaceRecord, SessionStoreError> {
    validation::validate_id(session_id)?;
    validate_browser_id(tab_id, "bt_")?;
    let admitted = admit_restorable_url(raw_url)
        .map_err(|_| SessionStoreError::Invalid("browser navigation URL is unsafe".into()))?;
    let lock = store_lock(sessions_dir);
    let _guard = lock.lock().expect("session store mutex poisoned");
    let mut conn = schema::open_connection(sessions_dir)?;
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(schema::storage("begin browser navigation commit"))?;
    require_session(&tx, session_id)?;
    let mut record = read_workspace(&tx, session_id, scope)?
        .ok_or_else(|| SessionStoreError::NotFound("browser workspace".into()))?;
    let tab = record
        .tabs
        .iter_mut()
        .find(|tab| tab.id == tab_id)
        .ok_or_else(|| SessionStoreError::NotFound(tab_id.into()))?;
    apply_navigation(tab, admitted, navigation)?;
    let normalized = normalize_for_save(session_id, scope, &record)?;
    write_workspace(&tx, &normalized)?;
    tx.commit()
        .map_err(schema::storage("commit browser navigation"))?;
    Ok(normalized)
}

fn apply_navigation(
    tab: &mut BrowserTabRecord,
    admitted: crate::browser::restoration::RestorableUrl,
    navigation: BrowserHistoryNavigation,
) -> Result<(), SessionStoreError> {
    match navigation {
        BrowserHistoryNavigation::New => {
            let keep = tab
                .current_history_index
                .map(|index| index.saturating_add(1))
                .unwrap_or(0)
                .min(tab.history.len());
            tab.history.truncate(keep);
            tab.history.push(BrowserHistoryRecord {
                position: tab.history.len(),
                url: admitted.value,
                recorded_at_ms: now_ms(),
            });
            if tab.history.len() > MAX_HISTORY_ROWS {
                let overflow = tab.history.len() - MAX_HISTORY_ROWS;
                tab.history.drain(..overflow);
            }
            tab.current_history_index = Some(tab.history.len() - 1);
            tab.manual_reopen_required |= admitted.manual_reopen_required;
        }
        BrowserHistoryNavigation::Back => {
            let current = tab.current_history_index.ok_or_else(|| {
                SessionStoreError::Invalid("blank browser tab cannot go back".into())
            })?;
            let target = current.checked_sub(1).ok_or_else(|| {
                SessionStoreError::Invalid("browser history has no previous entry".into())
            })?;
            require_navigation_target(tab, target, &admitted.value)?;
            tab.current_history_index = Some(target);
        }
        BrowserHistoryNavigation::Forward => {
            let current = tab.current_history_index.ok_or_else(|| {
                SessionStoreError::Invalid("blank browser tab cannot go forward".into())
            })?;
            let target = current.saturating_add(1);
            require_navigation_target(tab, target, &admitted.value)?;
            tab.current_history_index = Some(target);
        }
        BrowserHistoryNavigation::Reload | BrowserHistoryNavigation::Restore => {
            let current = tab.current_history_index.ok_or_else(|| {
                SessionStoreError::Invalid("blank browser tab has no page to reload".into())
            })?;
            require_navigation_target(tab, current, &admitted.value)?;
        }
    }
    for (position, history) in tab.history.iter_mut().enumerate() {
        history.position = position;
    }
    tab.restoration_status = if tab.history.is_empty() {
        BrowserRestorationStatus::Blank
    } else if tab.manual_reopen_required {
        BrowserRestorationStatus::ManualReopenRequired
    } else {
        BrowserRestorationStatus::Restorable
    };
    Ok(())
}

fn require_navigation_target(
    tab: &BrowserTabRecord,
    index: usize,
    admitted_url: &str,
) -> Result<(), SessionStoreError> {
    if tab.history.get(index).map(|row| row.url.as_str()) == Some(admitted_url) {
        Ok(())
    } else {
        Err(SessionStoreError::Invalid(
            "native browser history target does not match persisted state".into(),
        ))
    }
}

/// Replace any prior Browser subtree with one backend-minted blank tab.
/// Reset is an ordinary validated atomic replacement, so an unknown
/// session remains `NotFound` and a failure leaves the old state intact.
pub fn reset_browser_workspace(
    sessions_dir: &Path,
    session_id: &str,
    scope: BrowserWorkspaceScope,
) -> Result<BrowserWorkspaceRecord, SessionStoreError> {
    let tab_id = mint_tab_id();
    let workspace = BrowserWorkspaceRecord {
        session_id: session_id.into(),
        scope,
        layout_mode: BrowserLayoutMode::Split,
        split_width_px: DEFAULT_SPLIT_WIDTH_PX,
        active_tab_id: Some(tab_id.clone()),
        tabs: vec![BrowserTabRecord {
            id: tab_id,
            position: 0,
            current_history_index: None,
            manual_reopen_required: false,
            restoration_status: BrowserRestorationStatus::Blank,
            history: Vec::new(),
        }],
        recovery: None,
    };
    replace_browser_workspace(sessions_dir, session_id, scope, &workspace)
}

fn require_session(conn: &rusqlite::Connection, session_id: &str) -> Result<(), SessionStoreError> {
    let exists: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM chat_sessions WHERE id=?1)",
            params![session_id],
            |row| row.get(0),
        )
        .map_err(schema::storage("check browser session owner"))?;
    if exists {
        Ok(())
    } else {
        Err(SessionStoreError::NotFound(session_id.into()))
    }
}

fn normalize_for_save(
    session_id: &str,
    scope: BrowserWorkspaceScope,
    record: &BrowserWorkspaceRecord,
) -> Result<BrowserWorkspaceRecord, SessionStoreError> {
    normalize_record(session_id, scope, record, true)
}

fn validate_persisted_record(
    session_id: &str,
    scope: BrowserWorkspaceScope,
    record: &BrowserWorkspaceRecord,
) -> Result<BrowserWorkspaceRecord, SessionStoreError> {
    normalize_record(session_id, scope, record, false)
}

fn normalize_record(
    session_id: &str,
    scope: BrowserWorkspaceScope,
    record: &BrowserWorkspaceRecord,
    sanitize_incoming_urls: bool,
) -> Result<BrowserWorkspaceRecord, SessionStoreError> {
    if record.session_id != session_id || record.scope != scope {
        return Err(SessionStoreError::Invalid(
            "browser workspace identity does not match its session scope".into(),
        ));
    }
    if record.recovery.is_some() {
        return Err(SessionStoreError::Invalid(
            "browser recovery notices are output-only".into(),
        ));
    }
    validate_split_width_px(record.split_width_px)?;
    if record.tabs.len() > MAX_TABS {
        return Err(SessionStoreError::Limit(format!(
            "browser workspace exceeds the {MAX_TABS}-tab cap"
        )));
    }
    let mut normalized = record.clone();
    let mut ids = HashSet::with_capacity(normalized.tabs.len());
    for (position, tab) in normalized.tabs.iter_mut().enumerate() {
        validate_browser_id(&tab.id, "bt_")?;
        if !ids.insert(tab.id.clone()) {
            return Err(SessionStoreError::Invalid(
                "browser workspace contains duplicate tab ids".into(),
            ));
        }
        if tab.position != position {
            return Err(SessionStoreError::Invalid(
                "browser tab positions must be contiguous and ordered".into(),
            ));
        }
        if tab.history.len() > MAX_HISTORY_ROWS {
            let Some(current_index) = tab.current_history_index else {
                return Err(SessionStoreError::Invalid(
                    "non-empty browser history requires a current index".into(),
                ));
            };
            if current_index + 1 != tab.history.len() {
                return Err(SessionStoreError::Limit(
                    "over-cap Browser history can only be trimmed at its current tail".into(),
                ));
            }
            let overflow = tab.history.len() - MAX_HISTORY_ROWS;
            tab.history.drain(..overflow);
            tab.current_history_index = Some(current_index - overflow);
            for (history_position, history) in tab.history.iter_mut().enumerate() {
                history.position = history_position;
            }
        }
        if sanitize_incoming_urls {
            sanitize_tab_history(tab)?;
        }
        validate_tab(tab)?;
    }
    match (&normalized.active_tab_id, normalized.tabs.is_empty()) {
        (None, true) => {}
        (Some(active), false) if ids.contains(active) => {}
        _ => {
            return Err(SessionStoreError::Invalid(
                "browser active tab must identify one owned tab".into(),
            ));
        }
    }
    Ok(normalized)
}

fn sanitize_tab_history(tab: &mut BrowserTabRecord) -> Result<(), SessionStoreError> {
    if tab.history.is_empty() {
        return Ok(());
    }
    let current_index = tab.current_history_index.ok_or_else(|| {
        SessionStoreError::Invalid("non-empty browser history requires a current index".into())
    })?;
    if current_index >= tab.history.len() {
        return Err(SessionStoreError::Invalid(
            "browser current history index is outside its history".into(),
        ));
    }

    // Once a secret-bearing URL has been reduced, the sanitized value no
    // longer contains the evidence needed to derive this marker again. Keep a
    // prior conservative marker across ordinary load -> save cycles.
    let mut requires_manual_reopen = tab.manual_reopen_required;
    for history in &mut tab.history {
        let admitted = admit_restorable_url(&history.url).map_err(|_| {
            SessionStoreError::Invalid("browser history contains an unsafe URL".into())
        })?;
        history.url = admitted.value;
        requires_manual_reopen |= admitted.manual_reopen_required;
    }
    tab.manual_reopen_required = requires_manual_reopen;
    tab.restoration_status = if requires_manual_reopen {
        BrowserRestorationStatus::ManualReopenRequired
    } else {
        BrowserRestorationStatus::Restorable
    };
    Ok(())
}

fn validate_tab(tab: &BrowserTabRecord) -> Result<(), SessionStoreError> {
    if tab.history.is_empty() {
        if tab.current_history_index.is_some()
            || tab.manual_reopen_required
            || tab.restoration_status != BrowserRestorationStatus::Blank
        {
            return Err(SessionStoreError::Invalid(
                "an empty browser tab cannot carry current-page restoration state".into(),
            ));
        }
    } else {
        let Some(current_index) = tab.current_history_index else {
            return Err(SessionStoreError::Invalid(
                "non-empty browser history requires a current index".into(),
            ));
        };
        if current_index >= tab.history.len() {
            return Err(SessionStoreError::Invalid(
                "browser current history index is outside its history".into(),
            ));
        }
        let expected_status = if tab.manual_reopen_required {
            BrowserRestorationStatus::ManualReopenRequired
        } else {
            BrowserRestorationStatus::Restorable
        };
        if tab.restoration_status != expected_status {
            return Err(SessionStoreError::Invalid(
                "browser restoration status disagrees with its persisted marker".into(),
            ));
        }
    }
    for (position, history) in tab.history.iter().enumerate() {
        if history.position != position {
            return Err(SessionStoreError::Invalid(
                "browser history positions must be contiguous and ordered".into(),
            ));
        }
        let admitted = admit_restorable_url(&history.url).map_err(|_| {
            SessionStoreError::Invalid("browser history contains an unsafe URL".into())
        })?;
        if admitted.value != history.url || admitted.manual_reopen_required {
            return Err(SessionStoreError::Invalid(
                "browser history URL was not sanitized before persistence".into(),
            ));
        }
    }
    Ok(())
}

fn read_workspace(
    conn: &rusqlite::Connection,
    session_id: &str,
    scope: BrowserWorkspaceScope,
) -> Result<Option<BrowserWorkspaceRecord>, SessionStoreError> {
    let workspace = conn
        .query_row(
            "SELECT layout_mode,split_width_px,active_tab_id
             FROM browser_workspaces WHERE session_id=?1",
            params![session_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))
            },
        )
        .optional()
        .map_err(browser_read_error("read browser workspace"))?;
    let Some((layout, split_width_px, active_tab_id)) = workspace else {
        return Ok(None);
    };
    let mut tabs_stmt = conn
        .prepare(
            "SELECT id,position,current_history_index,manual_reopen_required
             FROM browser_tabs WHERE session_id=?1 ORDER BY position ASC",
        )
        .map_err(schema::storage("prepare browser tabs"))?;
    let tab_rows = tabs_stmt
        .query_map(params![session_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
            ))
        })
        .map_err(schema::storage("query browser tabs"))?;
    let mut tabs = Vec::new();
    for row in tab_rows {
        let (id, position, current_history_index, manual) =
            row.map_err(browser_read_error("read browser tab"))?;
        if !matches!(manual, 0 | 1) {
            return Err(SessionStoreError::Corrupt(
                "browser tab has a malformed restoration marker".into(),
            ));
        }
        let history = read_history(conn, &id)?;
        tabs.push(BrowserTabRecord {
            id,
            position: i64_to_usize(position, "tab position")?,
            current_history_index: current_index_from_sql(current_history_index)?,
            manual_reopen_required: manual == 1,
            restoration_status: if history.is_empty() {
                BrowserRestorationStatus::Blank
            } else if manual == 1 {
                BrowserRestorationStatus::ManualReopenRequired
            } else {
                BrowserRestorationStatus::Restorable
            },
            history,
        });
    }
    drop(tabs_stmt);
    let record = BrowserWorkspaceRecord {
        session_id: session_id.into(),
        scope,
        layout_mode: layout_from_sql(&layout)?,
        split_width_px,
        active_tab_id,
        tabs,
        recovery: None,
    };
    validate_persisted_record(session_id, scope, &record)
        .map(Some)
        .map_err(as_corrupt)
}

fn read_history(
    conn: &rusqlite::Connection,
    tab_id: &str,
) -> Result<Vec<BrowserHistoryRecord>, SessionStoreError> {
    let mut stmt = conn
        .prepare(
            "SELECT position,url,recorded_at_ms FROM browser_history
             WHERE tab_id=?1 ORDER BY position ASC",
        )
        .map_err(schema::storage("prepare browser history"))?;
    let rows = stmt
        .query_map(params![tab_id], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })
        .map_err(schema::storage("query browser history"))?;
    let mut history = Vec::new();
    for row in rows {
        let (position, url, recorded_at_ms) =
            row.map_err(browser_read_error("read browser history"))?;
        history.push(BrowserHistoryRecord {
            position: i64_to_usize(position, "history position")?,
            url,
            recorded_at_ms,
        });
    }
    if history.len() > MAX_HISTORY_ROWS {
        return Err(SessionStoreError::Corrupt(format!(
            "browser history exceeds the {MAX_HISTORY_ROWS}-row cap"
        )));
    }
    Ok(history)
}

fn validate_browser_id(id: &str, prefix: &str) -> Result<(), SessionStoreError> {
    let suffix = id.strip_prefix(prefix).unwrap_or_default();
    if suffix.len() == 32 && suffix.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(SessionStoreError::Invalid(
            "malformed browser-owned id".into(),
        ))
    }
}

fn layout_to_sql(layout: BrowserLayoutMode) -> &'static str {
    match layout {
        BrowserLayoutMode::Split => "split",
        BrowserLayoutMode::Expanded => "expanded",
    }
}

fn layout_from_sql(raw: &str) -> Result<BrowserLayoutMode, SessionStoreError> {
    match raw {
        "split" => Ok(BrowserLayoutMode::Split),
        "expanded" => Ok(BrowserLayoutMode::Expanded),
        _ => Err(SessionStoreError::Corrupt(
            "browser workspace has an unknown layout mode".into(),
        )),
    }
}

fn usize_to_i64(value: usize, label: &str) -> Result<i64, SessionStoreError> {
    i64::try_from(value)
        .map_err(|_| SessionStoreError::Invalid(format!("browser {label} exceeds SQLite range")))
}

fn current_index_to_sql(value: Option<usize>) -> Result<i64, SessionStoreError> {
    match value {
        Some(index) => usize_to_i64(index, "current history index"),
        None => Ok(-1),
    }
}

fn current_index_from_sql(value: i64) -> Result<Option<usize>, SessionStoreError> {
    match value {
        -1 => Ok(None),
        value => i64_to_usize(value, "current history index").map(Some),
    }
}

fn i64_to_usize(value: i64, label: &str) -> Result<usize, SessionStoreError> {
    usize::try_from(value)
        .map_err(|_| SessionStoreError::Corrupt(format!("browser {label} is negative")))
}

fn as_corrupt(error: SessionStoreError) -> SessionStoreError {
    match error {
        SessionStoreError::Invalid(message) | SessionStoreError::Limit(message) => {
            SessionStoreError::Corrupt(message)
        }
        other => other,
    }
}

fn browser_read_error(context: &'static str) -> impl Fn(rusqlite::Error) -> SessionStoreError {
    move |error| match error {
        rusqlite::Error::InvalidColumnType(..)
        | rusqlite::Error::FromSqlConversionFailure(..)
        | rusqlite::Error::IntegralValueOutOfRange(..) => {
            SessionStoreError::Corrupt(format!("{context}: malformed persisted field"))
        }
        other => SessionStoreError::Storage(format!("{context}: {other}")),
    }
}
