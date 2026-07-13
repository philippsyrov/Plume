use super::{SkillDocument, SkillInput, SkillsError, MAX_BODY_BYTES, MAX_FILE_BYTES};

pub(super) fn valid_slug(slug: &str) -> bool {
    if slug.is_empty() || slug.len() > 48 || !slug.is_ascii() {
        return false;
    }
    slug.split('-').all(|part| {
        !part.is_empty()
            && part
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
    })
}

fn validate_single_line(value: &str, label: &str, max_chars: usize) -> Result<(), SkillsError> {
    if value.is_empty() {
        return Err(SkillsError(format!("{label} must not be empty")));
    }
    if value.contains('\n') || value.contains('\r') {
        return Err(SkillsError(format!("{label} must be one line")));
    }
    if value.chars().count() > max_chars {
        return Err(SkillsError(format!(
            "{label} exceeds {max_chars} characters"
        )));
    }
    Ok(())
}

pub(super) fn canonical(input: &SkillInput) -> Result<String, SkillsError> {
    if !valid_slug(&input.slug) {
        return Err(SkillsError(
            "slug must match [a-z0-9]+(?:-[a-z0-9]+)* and be at most 48 bytes".into(),
        ));
    }
    validate_single_line(&input.name, "name", 80)?;
    validate_single_line(&input.description, "description", 240)?;
    if input.body.trim().is_empty() {
        return Err(SkillsError("body must contain Markdown".into()));
    }
    if input.body.len() > MAX_BODY_BYTES {
        return Err(SkillsError(format!("body exceeds {MAX_BODY_BYTES} bytes")));
    }
    let name = serde_json::to_string(&input.name).map_err(|e| SkillsError(e.to_string()))?;
    let description =
        serde_json::to_string(&input.description).map_err(|e| SkillsError(e.to_string()))?;
    let content = format!(
        "---\nname: {name}\ndescription: {description}\n---\n\n{}",
        input.body
    );
    if content.len() > MAX_FILE_BYTES {
        return Err(SkillsError(format!(
            "canonical SKILL.md exceeds {MAX_FILE_BYTES} bytes"
        )));
    }
    Ok(content)
}

pub(super) fn parse(slug: &str, bytes: &[u8]) -> Result<SkillDocument, SkillsError> {
    if bytes.len() > MAX_FILE_BYTES {
        return Err(SkillsError(format!(
            "SKILL.md exceeds {MAX_FILE_BYTES} bytes"
        )));
    }
    let content = std::str::from_utf8(bytes)
        .map_err(|_| SkillsError("SKILL.md is not valid UTF-8".into()))?;
    let mut lines = content.split_inclusive('\n');
    if lines.next() != Some("---\n") {
        return Err(SkillsError("frontmatter must start with ---".into()));
    }
    let mut name = None;
    let mut description = None;
    let mut consumed = 4usize;
    loop {
        let line = lines
            .next()
            .ok_or_else(|| SkillsError("frontmatter is not closed".into()))?;
        consumed += line.len();
        if line == "---\n" {
            break;
        }
        let line = line
            .strip_suffix('\n')
            .ok_or_else(|| SkillsError("frontmatter lines must end with newline".into()))?;
        let (key, raw) = line
            .split_once(": ")
            .ok_or_else(|| SkillsError("malformed frontmatter field".into()))?;
        let value: String = serde_json::from_str(raw)
            .map_err(|_| SkillsError(format!("{key} must be a JSON-quoted string")))?;
        match key {
            "name" if name.is_none() => name = Some(value),
            "description" if description.is_none() => description = Some(value),
            "name" | "description" => return Err(SkillsError(format!("duplicate {key} field"))),
            _ => return Err(SkillsError(format!("unknown frontmatter field {key}"))),
        }
    }
    if content.as_bytes().get(consumed) != Some(&b'\n') {
        return Err(SkillsError(
            "frontmatter must be followed by one blank line".into(),
        ));
    }
    let body = &content[consumed + 1..];
    let input = SkillInput {
        slug: slug.into(),
        name: name.ok_or_else(|| SkillsError("missing name field".into()))?,
        description: description.ok_or_else(|| SkillsError("missing description field".into()))?,
        body: body.into(),
    };
    let rebuilt = canonical(&input)?;
    if rebuilt != content {
        return Err(SkillsError("SKILL.md is not in canonical form".into()));
    }
    Ok(SkillDocument {
        slug: slug.into(),
        name: input.name,
        description: input.description,
        body: input.body,
        content: content.into(),
    })
}
