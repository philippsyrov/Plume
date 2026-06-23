//! Tests for the tool-catalog IPC (D92). Pin the wire shape and the
//! progressive-disclosure scoping the `tools.*` verbs promise.

use super::*;
use crate::agent::catalog::ToolTier;

// ─── tools.list ──────────────────────────────────────────────────────────

#[test]
fn list_returns_core_and_optional_with_tier() {
    let resp = list_response();
    assert!(!resp.tools.is_empty(), "catalog is non-empty");
    assert!(
        resp.tools.iter().any(|t| t.tier == ToolTier::Core),
        "has core tools"
    );
    assert!(
        resp.tools.iter().any(|t| t.tier == ToolTier::Optional),
        "has optional tools"
    );
}

#[test]
fn list_serializes_camel_case_with_tier_strings() {
    let resp = list_response();
    let v = serde_json::to_value(&resp).unwrap();
    let first = &v["tools"][0];
    // camelCase keys, tier as a lowercase discriminator, params present.
    assert!(first.get("name").is_some());
    assert!(first.get("summary").is_some());
    assert!(first.get("tier").is_some());
    assert!(first.get("params").is_some());
    // Every tier serializes to exactly "core" or "optional".
    for tool in v["tools"].as_array().unwrap() {
        let tier = tool["tier"].as_str().unwrap();
        assert!(tier == "core" || tier == "optional", "tier was {tier}");
    }
}

// ─── tools.search ────────────────────────────────────────────────────────

#[test]
fn search_returns_core_always_and_matches_only_optional() {
    // A query that hits an optional tool by name.
    let resp = search_response("github".to_string(), 10).expect("ok");
    assert!(!resp.core.is_empty(), "core is always returned");
    assert!(
        resp.core.iter().all(|t| t.tier == ToolTier::Core),
        "core list holds only core tools"
    );
    assert!(!resp.matched.is_empty(), "github matched something");
    assert!(
        resp.matched
            .iter()
            .all(|h| h.spec.tier == ToolTier::Optional),
        "matched holds ONLY optional tools — never a core tool"
    );
    assert!(
        resp.matched.iter().any(|h| h.spec.name == "github_open_pr"),
        "found github_open_pr"
    );
}

#[test]
fn search_for_a_core_name_yields_no_matches_but_still_returns_core() {
    // "patch_apply" is a CORE tool; search ranks only optional tools, so it
    // must not appear in `matched` (it's already in `core`).
    let resp = search_response("patch_apply".to_string(), 10).expect("ok");
    assert!(resp.matched.is_empty(), "core tools are never search hits");
    assert!(resp.core.iter().any(|t| t.name == "patch_apply"));
}

#[test]
fn search_respects_the_limit() {
    // "browser" hits two optional tools (browser_open, browser_click).
    let resp = search_response("browser".to_string(), 1).expect("ok");
    assert_eq!(resp.matched.len(), 1, "limit caps matched results");
}

#[test]
fn search_rejects_bad_limit() {
    assert!(matches!(
        search_response("x".to_string(), 0),
        Err(IpcError::BadArgument(_))
    ));
    assert!(matches!(
        search_response("x".to_string(), TOOLS_SEARCH_MAX_LIMIT + 1),
        Err(IpcError::BadArgument(_))
    ));
}

#[test]
fn search_rejects_oversize_query() {
    let big = "a".repeat(TOOLS_SEARCH_MAX_QUERY_BYTES + 1);
    assert!(matches!(
        search_response(big, 10),
        Err(IpcError::BadArgument(_))
    ));
}

#[test]
fn search_serializes_hit_with_spec_and_score() {
    let resp = search_response("huggingface".to_string(), 10).expect("ok");
    let v = serde_json::to_value(&resp).unwrap();
    assert_eq!(v["query"], "huggingface");
    assert!(v["core"].is_array());
    let hit = &v["matched"][0];
    assert!(hit["spec"]["name"].is_string());
    assert!(hit["score"].is_number());
}
