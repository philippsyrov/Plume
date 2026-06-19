//! Tests for the tool catalog / progressive disclosure scaffold (D86).
//! Pin the tier split, the search ranking ladder, and the visible-set
//! composition the prompt assembler will depend on.

use super::*;

fn sample() -> ToolCatalog {
    ToolCatalog::new(vec![
        ToolSpec::core(
            "file_read",
            "Read a file from the project",
            vec![ToolParam::new("path", "project-relative path")],
        ),
        ToolSpec::core(
            "file_search",
            "Grep the project for a pattern",
            vec![ToolParam::new("pattern", "regex to search for")],
        ),
        ToolSpec::optional(
            "github_open_pr",
            "Open a pull request on GitHub",
            vec![
                ToolParam::new("title", "PR title"),
                ToolParam::new("branch", "head branch"),
            ],
        ),
        ToolSpec::optional(
            "hf_download",
            "Download a model from Hugging Face",
            vec![ToolParam::new("repo", "model repo id")],
        ),
        ToolSpec::optional(
            "browser_click",
            "Click an element in the browser",
            vec![ToolParam::new("selector", "CSS selector")],
        ),
    ])
}

#[test]
fn tiers_partition_the_catalog() {
    let cat = sample();
    let core: Vec<&str> = cat.core().iter().map(|t| t.name.as_str()).collect();
    let optional: Vec<&str> = cat.optional().iter().map(|t| t.name.as_str()).collect();
    assert_eq!(core, vec!["file_read", "file_search"]);
    assert_eq!(
        optional,
        vec!["github_open_pr", "hf_download", "browser_click"]
    );
}

#[test]
fn search_only_ranks_optional_tools() {
    let cat = sample();
    // "file" matches the two CORE tools by name, but search never returns
    // core tools — they're already in the prompt.
    let hits = cat.search("file", 10);
    assert!(
        hits.iter().all(|h| h.spec.tier == ToolTier::Optional),
        "search must never surface a core tool"
    );
}

#[test]
fn exact_name_outranks_substring_and_summary() {
    let cat = sample();
    let hits = cat.search("hf_download", 10);
    assert_eq!(hits[0].spec.name, "hf_download");
    assert_eq!(hits[0].score, SCORE_NAME_EXACT);
}

#[test]
fn search_matches_summary_and_param_names() {
    let cat = sample();
    // "selector" appears only as a param name of browser_click.
    let by_param = cat.search("selector", 10);
    assert_eq!(by_param.len(), 1);
    assert_eq!(by_param[0].spec.name, "browser_click");
    assert_eq!(by_param[0].score, SCORE_PARAM_MATCH);

    // "hugging" appears only in hf_download's summary.
    let by_summary = cat.search("hugging", 10);
    assert_eq!(by_summary.len(), 1);
    assert_eq!(by_summary[0].spec.name, "hf_download");
    assert_eq!(by_summary[0].score, SCORE_SUMMARY_TOKEN);
}

#[test]
fn multi_token_query_accumulates_score() {
    let cat = sample();
    // github_open_pr — summary "Open a pull request on GitHub".
    //   "github" → name prefix (60) + summary contains "github" (20)
    //   "pull"   → summary contains "pull" (20)
    let hits = cat.search("github pull", 10);
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].spec.name, "github_open_pr");
    assert_eq!(
        hits[0].score,
        SCORE_NAME_PREFIX + SCORE_SUMMARY_TOKEN + SCORE_SUMMARY_TOKEN
    );
}

#[test]
fn search_respects_limit_and_orders_by_score() {
    // Two optional tools both match "model" so limit + ordering show.
    let cat2 = ToolCatalog::new(vec![
        ToolSpec::optional("a_tool", "handles models and stuff", vec![]),
        ToolSpec::optional("model_sync", "sync models", vec![]),
    ]);
    let hits = cat2.search("model", 1);
    assert_eq!(hits.len(), 1, "limit caps results");
    // model_sync scores name-prefix (60) + summary (20) = 80; a_tool scores
    // summary only (20). model_sync ranks first.
    assert_eq!(hits[0].spec.name, "model_sync");
}

#[test]
fn blank_query_and_zero_limit_return_nothing() {
    let cat = sample();
    assert!(cat.search("", 10).is_empty());
    assert!(cat.search("   ", 10).is_empty());
    assert!(cat.search("file", 0).is_empty());
}

#[test]
fn unmatched_query_returns_nothing() {
    let cat = sample();
    assert!(cat.search("zzzznomatch", 10).is_empty());
}

#[test]
fn visible_specs_is_core_then_hits() {
    let cat = sample();
    let visible: Vec<&str> = cat
        .visible_specs("hugging", 10)
        .iter()
        .map(|t| t.name.as_str())
        .collect();
    // Core tools first (in declaration order), then the one search hit.
    assert_eq!(visible, vec!["file_read", "file_search", "hf_download"]);
}

#[test]
fn visible_specs_with_no_query_is_just_core() {
    let cat = sample();
    let visible: Vec<&str> = cat
        .visible_specs("", 10)
        .iter()
        .map(|t| t.name.as_str())
        .collect();
    assert_eq!(visible, vec!["file_read", "file_search"]);
}

#[test]
fn search_is_case_insensitive() {
    let cat = sample();
    let lower = cat.search("github", 10);
    let upper = cat.search("GITHUB", 10);
    assert_eq!(lower, upper);
    assert_eq!(lower[0].spec.name, "github_open_pr");
}
