//! Markdown region selection. Regions are heading paths, not filesystem paths.

use ce_protocol::CommonroomError;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Heading {
    pub title: String,
    pub level: usize,
    pub line: usize,
    pub index: usize,
    pub path: String,
}

pub fn headings(content: &str) -> Vec<Heading> {
    let mut found = Vec::new();
    let mut stack: Vec<(usize, String)> = Vec::new();
    for (index, line) in content.lines().enumerate() {
        let stripped = line.trim();
        if !stripped.starts_with('#') {
            continue;
        }
        let level = stripped.chars().take_while(|ch| *ch == '#').count();
        if !(1..=6).contains(&level) {
            continue;
        }
        let title = stripped[level..].trim().to_string();
        if title.is_empty() {
            continue;
        }
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
            line: index + 1,
            index,
            path,
        });
    }
    found
}

pub fn span(content: &str, section: &str) -> Option<(usize, usize)> {
    let lines: Vec<&str> = content.lines().collect();
    let found = headings(content);
    let matched = found.iter().find(|heading| {
        heading.title == section
            || heading.path == section
            || format!("section:{}", heading.title) == section
            || format!("section:{}", heading.path) == section
    })?;
    let mut end = lines.len();
    for heading in &found {
        if heading.index <= matched.index {
            continue;
        }
        if heading.level <= matched.level {
            end = heading.index;
            break;
        }
    }
    Some((matched.index, end))
}

pub fn replace_section(
    content: &str,
    section: &str,
    replacement: &str,
) -> Result<String, CommonroomError> {
    let (start, end) = span(content, section).ok_or_else(|| {
        ce_protocol::invalid_region(format!("section '{section}' not found"))
            .detail("region", section)
    })?;
    let lines: Vec<&str> = content.lines().collect();
    let replacement_lines: Vec<&str> = replacement.lines().collect();
    let mut new_lines: Vec<&str> = Vec::new();
    new_lines.extend_from_slice(&lines[..start]);
    new_lines.extend(replacement_lines);
    new_lines.extend_from_slice(&lines[end..]);
    let trailing = if content.ends_with('\n') || replacement.ends_with('\n') {
        "\n"
    } else {
        ""
    };
    Ok(format!("{}{trailing}", new_lines.join("\n")))
}

pub fn limit_text(text: &str, max_lines: Option<i64>, max_bytes: Option<i64>) -> String {
    let mut output = text.to_string();
    if let Some(max_lines) = max_lines {
        if max_lines > 0 {
            output = output
                .lines()
                .take(max_lines as usize)
                .collect::<Vec<_>>()
                .join("\n");
        }
    }
    if let Some(max_bytes) = max_bytes {
        if max_bytes > 0 {
            let bytes = output.as_bytes();
            let limit = max_bytes as usize;
            if bytes.len() > limit {
                let mut end = limit;
                while end > 0 && std::str::from_utf8(&bytes[..end]).is_err() {
                    end -= 1;
                }
                output = String::from_utf8_lossy(&bytes[..end]).into_owned();
            }
        }
    }
    output
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
}
