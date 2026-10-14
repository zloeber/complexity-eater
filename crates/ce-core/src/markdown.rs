//! Markdown region selection. Regions are ATX heading paths, not filesystem paths.
//!
//! Setext headings (`===` / `---`) are not regions. Fenced code and lines
//! indented four or more spaces are not headings. An ATX heading needs a space
//! after the hash run, so `#tag` is not a region.

use ce_protocol::{invalid_region, CommonroomError};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Heading {
    pub title: String,
    pub level: usize,
    pub line: usize,
    pub index: usize,
    pub path: String,
    pub start: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SectionSpan {
    pub start: usize,
    pub end: usize,
    pub heading: Heading,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LocateError {
    NotFound,
    Ambiguous(Vec<String>),
}

pub fn headings(content: &str) -> Vec<Heading> {
    let mut found = Vec::new();
    let mut stack: Vec<(usize, String)> = Vec::new();
    let mut fence: Option<(char, usize)> = None;
    let bytes = content.as_bytes();
    let mut cursor = 0;
    let mut line_index = 0;
    while cursor < bytes.len() {
        let start = cursor;
        while cursor < bytes.len() && bytes[cursor] != b'\n' {
            cursor += 1;
        }
        let mut end = cursor;
        if end > start && bytes[end - 1] == b'\r' {
            end -= 1;
        }
        let line = &content[start..end];
        if let Some((marker, count)) = fence {
            if closing_fence(line, marker, count) {
                fence = None;
            }
        } else if let Some(opened) = opening_fence(line) {
            fence = Some(opened);
        } else if let Some((level, title)) = atx_heading(line) {
            while stack.last().is_some_and(|(current, _)| *current >= level) {
                stack.pop();
            }
            stack.push((level, title.clone()));
            let path = stack
                .iter()
                .map(|(_, name)| name.as_str())
                .collect::<Vec<_>>()
                .join("/");
            found.push(Heading {
                title,
                level,
                line: line_index + 1,
                index: line_index,
                path,
                start,
            });
        }
        if cursor < bytes.len() && bytes[cursor] == b'\n' {
            cursor += 1;
        }
        line_index += 1;
    }
    found
}

pub fn locate(content: &str, section: &str) -> Result<SectionSpan, LocateError> {
    let found = headings(content);
    let name = section.strip_prefix("section:").unwrap_or(section);
    let path_hits: Vec<usize> = found
        .iter()
        .enumerate()
        .filter(|(_, heading)| heading.path == name)
        .map(|(index, _)| index)
        .collect();
    let chosen = if path_hits.len() == 1 {
        path_hits[0]
    } else if path_hits.len() > 1 {
        return Err(LocateError::Ambiguous(
            path_hits
                .into_iter()
                .map(|index| found[index].path.clone())
                .collect(),
        ));
    } else {
        let title_hits: Vec<usize> = found
            .iter()
            .enumerate()
            .filter(|(_, heading)| heading.title == name)
            .map(|(index, _)| index)
            .collect();
        if title_hits.len() == 1 {
            title_hits[0]
        } else if title_hits.len() > 1 {
            return Err(LocateError::Ambiguous(
                title_hits
                    .into_iter()
                    .map(|index| found[index].path.clone())
                    .collect(),
            ));
        } else {
            return Err(LocateError::NotFound);
        }
    };
    let heading = found[chosen].clone();
    let end = found
        .iter()
        .skip(chosen + 1)
        .find(|item| item.level <= heading.level)
        .map(|item| item.start)
        .unwrap_or(content.len());
    Ok(SectionSpan {
        start: heading.start,
        end,
        heading,
    })
}

pub fn locate_error(section: &str, err: LocateError) -> CommonroomError {
    match err {
        LocateError::NotFound => {
            invalid_region(format!("section '{section}' not found")).detail("region", section)
        }
        LocateError::Ambiguous(candidates) => {
            invalid_region(format!("section '{section}' matches more than one heading"))
                .detail("region", section)
                .detail("candidates", candidates)
        }
    }
}

pub fn replace_section(
    content: &str,
    section: &str,
    replacement: &str,
) -> Result<String, CommonroomError> {
    let span = locate(content, section).map_err(|err| locate_error(section, err))?;
    let mut updated =
        String::with_capacity(content.len() - (span.end - span.start) + replacement.len());
    updated.push_str(&content[..span.start]);
    updated.push_str(replacement);
    updated.push_str(&content[span.end..]);
    Ok(updated)
}

/// Text owned by each heading, up to the next heading of any level.
pub fn own_regions(content: &str) -> Vec<(String, String)> {
    let found = headings(content);
    found
        .iter()
        .enumerate()
        .map(|(index, heading)| {
            let end = found
                .get(index + 1)
                .map(|next| next.start)
                .unwrap_or(content.len());
            (
                heading.path.clone(),
                content[heading.start..end].to_string(),
            )
        })
        .collect()
}

pub fn regions_overlap(before: &str, after: &str, target_path: &str) -> bool {
    if target_path == "workspace" || target_path == "*" {
        return true;
    }
    let changed = changed_paths(before, after);
    changed.iter().any(|path| paths_overlap(path, target_path))
}

pub fn changed_paths(before: &str, after: &str) -> Vec<String> {
    let left = own_regions(before);
    let right = own_regions(after);
    let mut changed = Vec::new();
    for (path, text) in &left {
        match right.iter().find(|(other, _)| other == path) {
            Some((_, other)) if other == text => {}
            _ => changed.push(path.clone()),
        }
    }
    for (path, _) in &right {
        if left.iter().all(|(other, _)| other != path) {
            changed.push(path.clone());
        }
    }
    changed
}

pub fn paths_overlap(left: &str, right: &str) -> bool {
    left == right
        || left.starts_with(&format!("{right}/"))
        || right.starts_with(&format!("{left}/"))
}

pub fn limit_text(text: &str, max_lines: Option<i64>, max_bytes: Option<i64>) -> (String, bool) {
    let mut output = text.to_string();
    let mut truncated = false;
    if let Some(max_lines) = max_lines.filter(|value| *value > 0) {
        let lines: Vec<&str> = output.lines().collect();
        if lines.len() > max_lines as usize {
            truncated = true;
            output = lines[..max_lines as usize].join("\n");
        }
    }
    if let Some(max_bytes) = max_bytes.filter(|value| *value > 0) {
        let limit = max_bytes as usize;
        if output.len() > limit {
            truncated = true;
            let bytes = output.as_bytes();
            let mut end = limit;
            while end > 0 && std::str::from_utf8(&bytes[..end]).is_err() {
                end -= 1;
            }
            output = String::from_utf8_lossy(&bytes[..end]).into_owned();
        }
    }
    (output, truncated)
}

pub fn unified_diff(old: &str, new: &str, from_file: &str, to_file: &str) -> String {
    let diff = similar::TextDiff::from_lines(old, new);
    let mut output = Vec::new();
    diff.unified_diff()
        .context_radius(3)
        .header(from_file, to_file)
        .to_writer(&mut output)
        .ok();
    let output = String::from_utf8_lossy(&output).into_owned();
    if output.starts_with("---") && !output.contains("\n@@") && old == new {
        return String::new();
    }
    output
}

fn opening_fence(line: &str) -> Option<(char, usize)> {
    let indent = line.chars().take_while(|ch| *ch == ' ').count();
    if indent > 3 || line.starts_with('\t') {
        return None;
    }
    let rest = &line[indent..];
    let marker = rest.chars().next()?;
    if marker != '`' && marker != '~' {
        return None;
    }
    let count = rest.chars().take_while(|ch| *ch == marker).count();
    if count < 3 {
        return None;
    }
    let info = rest[count..].trim();
    if marker == '`' && info.contains('`') {
        return None;
    }
    Some((marker, count))
}

fn closing_fence(line: &str, marker: char, count: usize) -> bool {
    let indent = line.chars().take_while(|ch| *ch == ' ').count();
    if indent > 3 || line.starts_with('\t') {
        return false;
    }
    let rest = &line[indent..];
    let got = rest.chars().take_while(|ch| *ch == marker).count();
    if got < count {
        return false;
    }
    rest[got..].chars().all(|ch| ch == ' ' || ch == '\t')
}

fn atx_heading(line: &str) -> Option<(usize, String)> {
    if line.starts_with('\t') {
        return None;
    }
    let indent = line.chars().take_while(|ch| *ch == ' ').count();
    if indent > 3 {
        return None;
    }
    let rest = &line[indent..];
    if !rest.starts_with('#') {
        return None;
    }
    let level = rest.chars().take_while(|ch| *ch == '#').count();
    if !(1..=6).contains(&level) {
        return None;
    }
    let after = &rest[level..];
    if !after.is_empty() && !after.starts_with(' ') && !after.starts_with('\t') {
        return None;
    }
    let mut title = after.trim().to_string();
    if title.ends_with('#') {
        let stripped = title.trim_end_matches('#').trim_end();
        if stripped.is_empty() || title.trim_end_matches('#').ends_with([' ', '\t']) {
            title = stripped.trim().to_string();
        }
    }
    if title.is_empty() {
        return None;
    }
    Some((level, title))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heading_paths_and_section_replacement() {
        let content = "# Architecture\n\n## Goals\n\nship\n";
        let updated = replace_section(content, "Goals", "## Goals\n\nship the room\n").unwrap();
        assert!(updated.contains("# Architecture"));
        assert!(updated.contains("ship the room"));
        assert!(!updated.contains("\nship\n"));
    }

    #[test]
    fn fenced_hashes_and_tags_are_not_headings() {
        let content = "# Doc\n\n## Setup\n\nRun:\n\n```bash\n# install deps\nnpm install\n```\n\nThen configure.\n#todo check this\n\n## Next\n\nstay\n";
        let paths: Vec<_> = headings(content)
            .into_iter()
            .map(|item| item.path)
            .collect();
        assert_eq!(paths, vec!["Doc", "Doc/Setup", "Doc/Next"]);
        let setup = locate(content, "Setup").unwrap();
        let slice = &content[setup.start..setup.end];
        assert!(slice.contains("# install deps"));
        assert!(slice.contains("```"));
        let updated = replace_section(content, "Setup", "## Setup\n\nready\n").unwrap();
        assert!(!updated.contains("# install deps"));
        assert!(!updated.contains("npm install"));
        assert!(updated.contains("## Next\n\nstay\n"));
    }

    #[test]
    fn ambiguous_titles_list_paths() {
        let content = "# Doc\n\n## Alpha\n\n### Notes\n\na\n\n## Beta\n\n### Notes\n\nb\n";
        let err = locate(content, "Notes").unwrap_err();
        assert_eq!(
            err,
            LocateError::Ambiguous(vec!["Doc/Alpha/Notes".into(), "Doc/Beta/Notes".into()])
        );
        assert!(locate(content, "Doc/Beta/Notes").is_ok());
    }

    #[test]
    fn replacing_a_read_section_is_byte_identical() {
        let content = "# Doc\n\n## Plan\n\n- step one\n- step two\n\n## Tail\n\nkeep\n";
        let span = locate(content, "Plan").unwrap();
        let slice = &content[span.start..span.end];
        let updated = replace_section(content, "Plan", slice).unwrap();
        assert_eq!(updated, content);
        let edited = slice.replace("- step two\n", "- step two\n- step three\n");
        let updated = replace_section(content, "Plan", &edited).unwrap();
        assert!(updated.contains("- step three\n"));
        assert!(updated.contains("\n## Tail\n"));
        assert!(updated.contains("keep\n"));
    }
}
