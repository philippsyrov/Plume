//! Local tool catalog + tool search (D86) — progressive disclosure scaffold.
//!
//! The stateless model behind `docs/TOOL_DISCLOSURE.md`: a small set of
//! **core** tools the prompt always shows, and a long tail of **optional**
//! tools the model retrieves by name/intent through a search verb so a
//! local model never pays the schema cost of an inventory it isn't using.
//!
//! Clean-room and Hermes-inspired: the *idea* (core stays direct, the tail
//! hides behind search) is borrowed; the ranking, tiers, and types are
//! Plume's own and deliberately simpler — a linear weighted substring/token
//! scan, no BM25, no index, no embeddings. The catalog is meant to be
//! rebuilt from live tool definitions each prompt assembly, so there is no
//! stale state to invalidate.
//!
//! **Scaffold only.** No prompt assembler consumes this yet, there is no
//! MCP integration, and nothing here executes a tool or authorizes one —
//! the catalog is a *presentation* concern (what the model may see), not an
//! *authorization* one (whether a tool may run, which is the approval /
//! allowlist gate's call). D92 wired a **read-only** IPC surface
//! (`tools.list` / `tools.search`) over [`builtin_catalog`]; the prompt
//! assembler that actually serializes these into a model turn is still a
//! later slice, hence `allow(dead_code)` on the unused helpers.

#![allow(dead_code)]

use serde::Serialize;

/// Whether a tool is always in the prompt or retrieved on demand.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ToolTier {
    /// Always serialized into the prompt, verbatim. Kept small on purpose.
    Core,
    /// Omitted by default; surfaced only when `search` matches it.
    Optional,
}

/// One parameter of a tool. Carried so search can match on parameter
/// names (a model often phrases a search by the argument it has in hand).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolParam {
    pub name: String,
    pub summary: String,
}

impl ToolParam {
    pub fn new(name: &str, summary: &str) -> Self {
        Self {
            name: name.to_string(),
            summary: summary.to_string(),
        }
    }
}

/// A tool's catalog entry: enough to rank it and to serialize its schema.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolSpec {
    pub name: String,
    pub summary: String,
    pub tier: ToolTier,
    pub params: Vec<ToolParam>,
}

impl ToolSpec {
    pub fn core(name: &str, summary: &str, params: Vec<ToolParam>) -> Self {
        Self {
            name: name.to_string(),
            summary: summary.to_string(),
            tier: ToolTier::Core,
            params,
        }
    }
    pub fn optional(name: &str, summary: &str, params: Vec<ToolParam>) -> Self {
        Self {
            name: name.to_string(),
            summary: summary.to_string(),
            tier: ToolTier::Optional,
            params,
        }
    }
}

/// A scored search result. Higher `score` ranks first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolSearchHit<'a> {
    pub spec: &'a ToolSpec,
    pub score: u32,
}

// Score weights, highest-signal first. Tuned only for ordering, not for
// any absolute meaning; the doc lists the same ladder.
const SCORE_NAME_EXACT: u32 = 100;
const SCORE_NAME_PREFIX: u32 = 60;
const SCORE_NAME_SUBSTR: u32 = 40;
const SCORE_SUMMARY_TOKEN: u32 = 20;
const SCORE_PARAM_MATCH: u32 = 10;

/// The stateless tool catalog. Construct it from the live tool set each
/// assembly; it owns no mutable state.
#[derive(Debug, Clone, Default)]
pub struct ToolCatalog {
    tools: Vec<ToolSpec>,
}

impl ToolCatalog {
    pub fn new(tools: Vec<ToolSpec>) -> Self {
        Self { tools }
    }

    pub fn len(&self) -> usize {
        self.tools.len()
    }
    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }

    /// All tools in declaration order (core and optional interleaved as
    /// declared). The `tools.list` IPC serializes this.
    pub fn specs(&self) -> &[ToolSpec] {
        &self.tools
    }

    /// The always-visible tools, in declaration order.
    pub fn core(&self) -> Vec<&ToolSpec> {
        self.tools
            .iter()
            .filter(|t| t.tier == ToolTier::Core)
            .collect()
    }

    /// The retrievable tools, in declaration order.
    pub fn optional(&self) -> Vec<&ToolSpec> {
        self.tools
            .iter()
            .filter(|t| t.tier == ToolTier::Optional)
            .collect()
    }

    /// Rank the **optional** tools against `query`, returning the top
    /// `limit` by descending score (ties broken by declaration order, which
    /// `sort_by_key` preserves as a stable sort). Core tools are never in the
    /// results — they are already in the prompt. A blank query or `limit`
    /// of 0 returns nothing.
    pub fn search(&self, query: &str, limit: usize) -> Vec<ToolSearchHit<'_>> {
        let q = query.trim().to_lowercase();
        if q.is_empty() || limit == 0 {
            return Vec::new();
        }
        let mut hits: Vec<ToolSearchHit<'_>> = self
            .tools
            .iter()
            .filter(|t| t.tier == ToolTier::Optional)
            .filter_map(|spec| {
                let score = score_spec(spec, &q);
                (score > 0).then_some(ToolSearchHit { spec, score })
            })
            .collect();
        // Stable sort by descending score keeps declaration order within a
        // score tier — deterministic results for the same catalog + query.
        hits.sort_by_key(|hit| std::cmp::Reverse(hit.score));
        hits.truncate(limit);
        hits
    }

    /// The set of specs to serialize for a turn: every core tool, then the
    /// search hits for `query` (if any). This is the one call the prompt
    /// assembler makes — the serialized tool surface stays ~constant
    /// (core + at most `limit`) regardless of how many optional tools exist.
    pub fn visible_specs(&self, query: &str, limit: usize) -> Vec<&ToolSpec> {
        let mut out = self.core();
        for hit in self.search(query, limit) {
            out.push(hit.spec);
        }
        out
    }
}

/// Score one spec against a lowercased query. `0` means no match (filtered
/// out). The query may be multiple whitespace tokens; each token can
/// contribute, so "read file" scores a tool named `file_read` on both.
fn score_spec(spec: &ToolSpec, query: &str) -> u32 {
    let name = spec.name.to_lowercase();
    let summary = spec.summary.to_lowercase();
    let mut score = 0u32;

    for token in query.split_whitespace() {
        if token.is_empty() {
            continue;
        }
        if name == token {
            score += SCORE_NAME_EXACT;
        } else if name.starts_with(token) {
            score += SCORE_NAME_PREFIX;
        } else if name.contains(token) {
            score += SCORE_NAME_SUBSTR;
        }
        if summary.contains(token) {
            score += SCORE_SUMMARY_TOKEN;
        }
        if spec
            .params
            .iter()
            .any(|p| p.name.to_lowercase().contains(token))
        {
            score += SCORE_PARAM_MATCH;
        }
    }
    score
}

/// Plume's built-in tool catalog (D92). The concrete core / optional split
/// `docs/TOOL_DISCLOSURE.md` describes, as data — no execution, no MCP, no
/// authorization. The `tools.*` IPC reads this; a prompt assembler will
/// later serialize the visible subset into a model turn.
///
/// Core stays deliberately small (a coding turn's constant companions);
/// the long tail is Optional and reached through search. Names are stable
/// identifiers a future executor will map to real handlers — listing one
/// here grants the model *visibility*, never permission to run it.
pub fn builtin_catalog() -> ToolCatalog {
    ToolCatalog::new(vec![
        // ── Core: always serialized into the prompt ──────────────────────
        ToolSpec::core(
            "file_read",
            "Read a project file's contents.",
            vec![ToolParam::new("path", "project-relative path to read")],
        ),
        ToolSpec::core(
            "file_search",
            "Search the project for a regex or literal pattern.",
            vec![
                ToolParam::new("pattern", "regex or literal to search for"),
                ToolParam::new("glob", "optional path glob to filter files"),
            ],
        ),
        ToolSpec::core(
            "patch_validate",
            "Validate a unified diff against the project without writing.",
            vec![ToolParam::new("diff", "the unified diff to validate")],
        ),
        ToolSpec::core(
            "patch_apply",
            "Apply a validated unified diff inside the file allowlist.",
            vec![ToolParam::new("diff", "the unified diff to apply")],
        ),
        ToolSpec::core(
            "patch_revert",
            "Revert a previously applied patch via its checkpoint.",
            vec![ToolParam::new("checkpoint", "checkpoint id from apply")],
        ),
        ToolSpec::core(
            "memory_search",
            "Search the project's local memory store.",
            vec![ToolParam::new(
                "query",
                "substring to search remembered notes",
            )],
        ),
        ToolSpec::core(
            "memory_remember",
            "Add a note to the project's local memory.",
            vec![ToolParam::new(
                "text",
                "the note to remember (secrets redacted)",
            )],
        ),
        ToolSpec::core(
            "run_verifier",
            "Run an allowlisted verifier command (e.g. the test suite).",
            vec![ToolParam::new("command", "allowlisted argv to run")],
        ),
        ToolSpec::core("stop", "Stop the current agent run.", vec![]),
        // ── Optional: omitted by default, reached through search ──────────
        ToolSpec::optional(
            "github_open_pr",
            "Open a pull request on GitHub for the current branch.",
            vec![
                ToolParam::new("title", "pull request title"),
                ToolParam::new("body", "pull request description"),
            ],
        ),
        ToolSpec::optional(
            "github_list_issues",
            "List open issues on the GitHub repository.",
            vec![ToolParam::new("query", "optional search filter")],
        ),
        ToolSpec::optional(
            "huggingface_download",
            "Download a model snapshot from Hugging Face into the model dir.",
            vec![ToolParam::new("repo", "the model repo id to download")],
        ),
        ToolSpec::optional(
            "browser_open",
            "Open a URL in a controlled browser surface.",
            vec![ToolParam::new("url", "the URL to open")],
        ),
        ToolSpec::optional(
            "browser_click",
            "Click an element in the controlled browser surface.",
            vec![ToolParam::new("selector", "CSS selector of the element")],
        ),
        ToolSpec::optional(
            "computer_screenshot",
            "Capture a screenshot of the controlled computer-use target.",
            vec![],
        ),
    ])
}

#[cfg(test)]
#[path = "catalog_tests.rs"]
mod tests;
