//! D92: read-only tool-catalog IPC (`tools.list` / `tools.search`).
//!
//! A thin, stateless surface over [`crate::agent::catalog::builtin_catalog`]
//! that lets a caller (a future prompt assembler, or a UI) see the agent's
//! tool catalog and exercise progressive disclosure: core tools are always
//! listed; optional tools are reached through search. See
//! `docs/TOOL_DISCLOSURE.md`.
//!
//! **Read-only and unprivileged.** Like `session.*`, these verbs touch no
//! disk and run nothing, so they are not trust-gated. Listing or finding a
//! tool grants *visibility*, never permission to execute it — that stays
//! the approval / allowlist gate's call when (a later slice) an executor
//! lands. No MCP, no command execution, no file writes here.

use serde::{Deserialize, Serialize};

use crate::agent::catalog::{builtin_catalog, ToolSpec};
use crate::error::{IpcError, IpcRequest};

/// Upper bound on `tools.search` results, mirroring how `memory.search`
/// caps its limit rather than silently clamping — keeps the caller honest.
pub const TOOLS_SEARCH_MAX_LIMIT: usize = 50;
/// Max query length accepted (bytes), same ceiling as `memory.search`.
pub const TOOLS_SEARCH_MAX_QUERY_BYTES: usize = 256;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EmptyPayload {}

/// `tools.list` result: every tool in the catalog, each carrying its
/// `tier` so the caller can split core vs optional itself.
#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ToolListResponse {
    pub tools: Vec<ToolSpec>,
}

#[tauri::command]
pub async fn tools_list(req: IpcRequest<EmptyPayload>) -> Result<ToolListResponse, IpcError> {
    req.check_version()?;
    Ok(list_response())
}

/// Sync core of `tools.list`, exercised directly by tests.
fn list_response() -> ToolListResponse {
    ToolListResponse {
        tools: builtin_catalog().specs().to_vec(),
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ToolsSearchPayload {
    pub query: String,
    pub limit: usize,
}

/// One ranked search hit (owned wire form of `catalog::ToolSearchHit`).
#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ToolSearchHitWire {
    pub spec: ToolSpec,
    pub score: u32,
}

/// `tools.search` result. `core` is **always** returned (those tools are
/// already in the prompt); `matched` is the ranked, capped set of
/// **optional** tools the query hit — never a core tool. This is the
/// scoping the progressive-disclosure design promises.
#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ToolSearchResponse {
    pub query: String,
    pub core: Vec<ToolSpec>,
    pub matched: Vec<ToolSearchHitWire>,
}

#[tauri::command]
pub async fn tools_search(
    req: IpcRequest<ToolsSearchPayload>,
) -> Result<ToolSearchResponse, IpcError> {
    req.check_version()?;
    let ToolsSearchPayload { query, limit } = req.payload;
    search_response(query, limit)
}

/// Sync core of `tools.search`, exercised directly by tests.
fn search_response(query: String, limit: usize) -> Result<ToolSearchResponse, IpcError> {
    if limit == 0 || limit > TOOLS_SEARCH_MAX_LIMIT {
        return Err(IpcError::BadArgument(format!(
            "limit must be between 1 and {TOOLS_SEARCH_MAX_LIMIT}; got {limit}"
        )));
    }
    if query.len() > TOOLS_SEARCH_MAX_QUERY_BYTES {
        return Err(IpcError::BadArgument(format!(
            "query exceeds {TOOLS_SEARCH_MAX_QUERY_BYTES} bytes"
        )));
    }

    let catalog = builtin_catalog();
    let core: Vec<ToolSpec> = catalog.core().into_iter().cloned().collect();
    let matched: Vec<ToolSearchHitWire> = catalog
        .search(&query, limit)
        .into_iter()
        .map(|hit| ToolSearchHitWire {
            spec: hit.spec.clone(),
            score: hit.score,
        })
        .collect();

    Ok(ToolSearchResponse {
        query,
        core,
        matched,
    })
}

#[cfg(test)]
#[path = "tools_tests.rs"]
mod tests;
