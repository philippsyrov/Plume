use serde::Serialize;

#[derive(Debug, Clone)]
pub struct SkillInput {
    pub slug: String,
    pub name: String,
    pub description: String,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillMetadata {
    pub slug: String,
    pub name: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillInvalid {
    pub slug: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillIndex {
    pub skills: Vec<SkillMetadata>,
    pub invalid: Vec<SkillInvalid>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillDocument {
    pub slug: String,
    pub name: String,
    pub description: String,
    pub body: String,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillPreview {
    pub slug: String,
    pub content: String,
    pub exists: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillApplyResponse {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skill: Option<SkillMetadata>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct SkillsError(pub String);
