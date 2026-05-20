use super::inline::pair_inline_spans;
use super::model::{GitDiffCell, GitDiffCellKind, GitDiffRow, InlineSpan};
use super::width::expand_tabs;

pub(crate) fn parse_hunk_header(line: &str) -> Option<(u32, u32)> {
    let trimmed = line.trim();
    let rest = trimmed.strip_prefix("@@")?.trim_start();
    let (range, _) = rest.split_once("@@")?;
    let mut it = range.split_whitespace();
    let old_tok = it.next()?;
    let new_tok = it.next()?;
    let old_start = old_tok.strip_prefix('-')?.split(',').next()?.parse().ok()?;
    let new_start = new_tok.strip_prefix('+')?.split(',').next()?.parse().ok()?;
    Some((old_start, new_start))
}

pub fn build_side_by_side_rows(lines: &[String]) -> Vec<GitDiffRow> {
    let mut rows = Vec::new();
    let mut old_line: Option<u32> = None;
    let mut new_line: Option<u32> = None;
    let mut pending_del: Vec<(Option<u32>, String)> = Vec::new();
    let mut pending_add: Vec<(Option<u32>, String)> = Vec::new();

    for line in lines {
        if is_meta_line(line) {
            flush_changes(&mut rows, &mut pending_del, &mut pending_add);
            rows.push(GitDiffRow::Meta(line.clone()));
            continue;
        }

        if line.starts_with("@@") {
            flush_changes(&mut rows, &mut pending_del, &mut pending_add);
            rows.push(GitDiffRow::Meta(line.clone()));
            if let Some((old, new)) = parse_hunk_header(line) {
                old_line = Some(old);
                new_line = Some(new);
            }
            continue;
        }

        let Some(first) = line.chars().next() else {
            continue;
        };

        match first {
            ' ' => {
                flush_changes(&mut rows, &mut pending_del, &mut pending_add);
                let text = expand_tabs(line.get(1..).unwrap_or(""));
                rows.push(GitDiffRow::Split {
                    old: GitDiffCell::new(old_line, text.clone(), GitDiffCellKind::Context),
                    new: GitDiffCell::new(new_line, text, GitDiffCellKind::Context),
                });
                if let Some(value) = old_line.as_mut() {
                    *value += 1;
                }
                if let Some(value) = new_line.as_mut() {
                    *value += 1;
                }
            }
            '-' => {
                let line_no = old_line;
                if let Some(value) = old_line.as_mut() {
                    *value += 1;
                }
                pending_del.push((line_no, expand_tabs(line.get(1..).unwrap_or(""))));
            }
            '+' => {
                let line_no = new_line;
                if let Some(value) = new_line.as_mut() {
                    *value += 1;
                }
                pending_add.push((line_no, expand_tabs(line.get(1..).unwrap_or(""))));
            }
            _ => {
                flush_changes(&mut rows, &mut pending_del, &mut pending_add);
                rows.push(GitDiffRow::Meta(line.clone()));
            }
        }
    }

    flush_changes(&mut rows, &mut pending_del, &mut pending_add);
    rows
}

fn is_meta_line(line: &str) -> bool {
    line.starts_with("diff --git ")
        || line.starts_with("index ")
        || line.starts_with("--- ")
        || line.starts_with("+++ ")
        || line.starts_with("rename ")
        || line.starts_with("new file ")
        || line.starts_with("deleted file ")
        || line.starts_with("similarity index ")
        || line.starts_with("Binary files ")
        || line.starts_with("\\ No newline")
}

fn flush_changes(
    rows: &mut Vec<GitDiffRow>,
    pending_del: &mut Vec<(Option<u32>, String)>,
    pending_add: &mut Vec<(Option<u32>, String)>,
) {
    if pending_del.is_empty() && pending_add.is_empty() {
        return;
    }

    let deletes: Vec<String> = pending_del.iter().map(|(_, text)| text.clone()).collect();
    let adds: Vec<String> = pending_add.iter().map(|(_, text)| text.clone()).collect();
    let (delete_spans, add_spans, pairs) = pair_inline_spans(&deletes, &adds);

    let mut delete_index = 0usize;
    let mut add_index = 0usize;
    for pair in pairs {
        while delete_index < pair.delete_index {
            rows.push(left_only_cell(&pending_del[delete_index], Vec::new()));
            delete_index += 1;
        }
        while add_index < pair.add_index {
            rows.push(right_only_cell(&pending_add[add_index], Vec::new()));
            add_index += 1;
        }

        rows.push(GitDiffRow::Split {
            old: GitDiffCell::with_spans(
                pending_del[pair.delete_index].0,
                pending_del[pair.delete_index].1.clone(),
                GitDiffCellKind::Delete,
                delete_spans[pair.delete_index].clone(),
            ),
            new: GitDiffCell::with_spans(
                pending_add[pair.add_index].0,
                pending_add[pair.add_index].1.clone(),
                GitDiffCellKind::Add,
                add_spans[pair.add_index].clone(),
            ),
        });
        delete_index = pair.delete_index + 1;
        add_index = pair.add_index + 1;
    }

    while delete_index < pending_del.len() {
        rows.push(left_only_cell(&pending_del[delete_index], Vec::new()));
        delete_index += 1;
    }
    while add_index < pending_add.len() {
        rows.push(right_only_cell(&pending_add[add_index], Vec::new()));
        add_index += 1;
    }

    pending_del.clear();
    pending_add.clear();
}

fn left_only_cell(line: &(Option<u32>, String), spans: Vec<InlineSpan>) -> GitDiffRow {
    GitDiffRow::Split {
        old: GitDiffCell::with_spans(line.0, line.1.clone(), GitDiffCellKind::Delete, spans),
        new: GitDiffCell::empty(),
    }
}

fn right_only_cell(line: &(Option<u32>, String), spans: Vec<InlineSpan>) -> GitDiffRow {
    GitDiffRow::Split {
        old: GitDiffCell::empty(),
        new: GitDiffCell::with_spans(line.0, line.1.clone(), GitDiffCellKind::Add, spans),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_lines(body: &[&str]) -> Vec<String> {
        let mut lines = vec![
            "diff --git a/main.rs b/main.rs".to_string(),
            "--- a/main.rs".to_string(),
            "+++ b/main.rs".to_string(),
            "@@ -10,3 +20,4 @@ fn demo() {".to_string(),
        ];
        lines.extend(body.iter().map(|line| line.to_string()));
        lines
    }

    #[test]
    fn side_by_side_pairs_similar_replacement_rows() {
        let rows = build_side_by_side_rows(&sample_lines(&[
            "-foo := oldValue + 1",
            "+foo := newValue + 1",
        ]));
        let paired = rows
            .iter()
            .find_map(|row| match row {
                GitDiffRow::Split { old, new }
                    if old.kind == GitDiffCellKind::Delete && new.kind == GitDiffCellKind::Add =>
                {
                    Some((old, new))
                }
                _ => None,
            })
            .expect("paired row");
        assert_eq!(paired.0.line_no, Some(10));
        assert_eq!(paired.1.line_no, Some(20));
        assert!(!paired.0.inline_spans.is_empty());
        assert!(!paired.1.inline_spans.is_empty());
    }

    #[test]
    fn side_by_side_handles_inserted_line_between_replacements() {
        let rows = build_side_by_side_rows(&sample_lines(&[
            "-foo := oldValue + 1",
            "-keep()",
            "+inserted()",
            "+foo := newValue + 1",
            "+keep()",
        ]));
        let split_rows: Vec<_> = rows
            .iter()
            .filter_map(|row| match row {
                GitDiffRow::Split { old, new } => {
                    Some((old.kind, new.kind, old.text.as_str(), new.text.as_str()))
                }
                _ => None,
            })
            .collect();
        assert!(
            split_rows
                .iter()
                .any(|row| row.1 == GitDiffCellKind::Add && row.3 == "inserted()")
        );
        assert!(split_rows.iter().any(|row| row.0 == GitDiffCellKind::Delete
            && row.1 == GitDiffCellKind::Add
            && row.2.contains("oldValue")
            && row.3.contains("newValue")));
    }

    #[test]
    fn side_by_side_leaves_unrelated_changes_unpaired() {
        let rows = build_side_by_side_rows(&sample_lines(&["-alpha beta", "+one two"]));
        let split_rows: Vec<_> = rows
            .iter()
            .filter(|row| matches!(row, GitDiffRow::Split { .. }))
            .collect();
        assert_eq!(split_rows.len(), 2);
        assert!(
            matches!(split_rows[0], GitDiffRow::Split { old, new } if old.kind == GitDiffCellKind::Delete && new.kind == GitDiffCellKind::Empty)
        );
        assert!(
            matches!(split_rows[1], GitDiffRow::Split { old, new } if old.kind == GitDiffCellKind::Empty && new.kind == GitDiffCellKind::Add)
        );
    }
}
