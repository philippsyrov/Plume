use std::path::Path;

use rusqlite::TransactionBehavior;

use super::{
    normalize_for_save, read_workspace, require_session, schema, store_lock, write_workspace,
    BrowserWorkspaceRecord, BrowserWorkspaceScope, SessionStoreError,
};

/// Atomically save the frontend-owned workspace shape without allowing a
/// stale renderer snapshot to overwrite newer native navigation state.
/// Existing tab history and restoration fields are backend-owned; tab
/// membership/order, selection, and layout remain frontend-owned.
pub fn merge_browser_workspace_from_frontend(
    sessions_dir: &Path,
    session_id: &str,
    scope: BrowserWorkspaceScope,
    record: &BrowserWorkspaceRecord,
) -> Result<BrowserWorkspaceRecord, SessionStoreError> {
    super::validation::validate_id(session_id)?;
    let mut normalized = normalize_for_save(session_id, scope, record)?;
    let lock = store_lock(sessions_dir);
    let _guard = lock.lock().expect("session store mutex poisoned");
    let mut conn = schema::open_connection(sessions_dir)?;
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(schema::storage("begin browser workspace merge"))?;
    require_session(&tx, session_id)?;
    if let Some(current) = read_workspace(&tx, session_id, scope)? {
        for tab in &mut normalized.tabs {
            let Some(current_tab) = current.tabs.iter().find(|candidate| candidate.id == tab.id)
            else {
                continue;
            };
            tab.current_history_index = current_tab.current_history_index;
            tab.manual_reopen_required = current_tab.manual_reopen_required;
            tab.restoration_status = current_tab.restoration_status;
            tab.history.clone_from(&current_tab.history);
        }
    }
    write_workspace(&tx, &normalized)?;
    tx.commit()
        .map_err(schema::storage("commit browser workspace merge"))?;
    Ok(normalized)
}
