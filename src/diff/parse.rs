use super::side_by_side::parse_hunk_header;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ParsedDiffDocument {
    pub preamble: Vec<String>,
    pub files: Vec<ParsedDiffFile>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ParsedDiffFile {
    pub header: Vec<String>,
    pub old_name: Option<String>,
    pub new_name: Option<String>,
    pub hunks: Vec<ParsedDiffHunk>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedDiffHunk {
    pub header: String,
    pub old_start: u32,
    pub new_start: u32,
    pub lines: Vec<ParsedDiffLine>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParsedDiffLineKind {
    Context,
    Add,
    Delete,
    NoNewline,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedDiffLine {
    pub kind: ParsedDiffLineKind,
    pub old_line: Option<u32>,
    pub new_line: Option<u32>,
    pub text: String,
}

pub fn parse_unified_diff(lines: &[String]) -> ParsedDiffDocument {
    let mut doc = ParsedDiffDocument::default();
    let mut current_file: Option<ParsedDiffFile> = None;
    let mut current_hunk: Option<ParsedDiffHunk> = None;
    let mut old_line = 0u32;
    let mut new_line = 0u32;

    for line in lines {
        if line.starts_with("diff --git ") {
            finish_hunk(&mut current_file, &mut current_hunk);
            finish_file(&mut doc, &mut current_file);
            current_file = Some(ParsedDiffFile {
                header: vec![line.clone()],
                ..ParsedDiffFile::default()
            });
            continue;
        }

        if current_file.is_none() {
            doc.preamble.push(line.clone());
            continue;
        }

        if line.starts_with("@@") {
            finish_hunk(&mut current_file, &mut current_hunk);
            if let Some((old, new)) = parse_hunk_header(line) {
                old_line = old;
                new_line = new;
                current_hunk = Some(ParsedDiffHunk {
                    header: line.clone(),
                    old_start: old,
                    new_start: new,
                    lines: Vec::new(),
                });
            } else {
                current_file
                    .as_mut()
                    .expect("checked above")
                    .header
                    .push(line.clone());
            }
            continue;
        }

        if let Some(hunk) = current_hunk.as_mut() {
            match line.chars().next() {
                Some('+') => {
                    hunk.lines.push(ParsedDiffLine {
                        kind: ParsedDiffLineKind::Add,
                        old_line: None,
                        new_line: Some(new_line),
                        text: line.get(1..).unwrap_or("").to_string(),
                    });
                    new_line += 1;
                    continue;
                }
                Some('-') => {
                    hunk.lines.push(ParsedDiffLine {
                        kind: ParsedDiffLineKind::Delete,
                        old_line: Some(old_line),
                        new_line: None,
                        text: line.get(1..).unwrap_or("").to_string(),
                    });
                    old_line += 1;
                    continue;
                }
                Some('\\') => {
                    hunk.lines.push(ParsedDiffLine {
                        kind: ParsedDiffLineKind::NoNewline,
                        old_line: None,
                        new_line: None,
                        text: line.clone(),
                    });
                    continue;
                }
                _ => {
                    let text = line.strip_prefix(' ').unwrap_or(line).to_string();
                    hunk.lines.push(ParsedDiffLine {
                        kind: ParsedDiffLineKind::Context,
                        old_line: Some(old_line),
                        new_line: Some(new_line),
                        text,
                    });
                    old_line += 1;
                    new_line += 1;
                    continue;
                }
            }
        }

        let file = current_file.as_mut().expect("checked above");
        if let Some(name) = line.strip_prefix("--- ") {
            file.old_name = Some(name.to_string());
        }
        if let Some(name) = line.strip_prefix("+++ ") {
            file.new_name = Some(name.to_string());
        }
        file.header.push(line.clone());
    }

    finish_hunk(&mut current_file, &mut current_hunk);
    finish_file(&mut doc, &mut current_file);
    doc
}

fn finish_hunk(file: &mut Option<ParsedDiffFile>, hunk: &mut Option<ParsedDiffHunk>) {
    if let (Some(file), Some(hunk)) = (file.as_mut(), hunk.take()) {
        file.hunks.push(hunk);
    }
}

fn finish_file(doc: &mut ParsedDiffDocument, file: &mut Option<ParsedDiffFile>) {
    if let Some(file) = file.take() {
        doc.files.push(file);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_file_hunk_and_no_newline_marker() {
        let doc = parse_unified_diff(&[
            "commit abc".to_string(),
            "diff --git a/a.rs b/a.rs".to_string(),
            "--- a/a.rs".to_string(),
            "+++ b/a.rs".to_string(),
            "@@ -1,2 +1,2 @@".to_string(),
            " same".to_string(),
            "-old".to_string(),
            "+new".to_string(),
            "\\ No newline at end of file".to_string(),
        ]);
        assert_eq!(doc.preamble, vec!["commit abc"]);
        assert_eq!(doc.files.len(), 1);
        assert_eq!(doc.files[0].old_name.as_deref(), Some("a/a.rs"));
        assert_eq!(doc.files[0].new_name.as_deref(), Some("b/a.rs"));
        assert_eq!(doc.files[0].hunks[0].old_start, 1);
        assert_eq!(doc.files[0].hunks[0].new_start, 1);
        assert_eq!(
            doc.files[0].hunks[0].lines[1].kind,
            ParsedDiffLineKind::Delete
        );
        assert_eq!(
            doc.files[0].hunks[0].lines[3].kind,
            ParsedDiffLineKind::NoNewline
        );
    }
}
