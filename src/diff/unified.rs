use super::inline::pair_inline_spans;
use super::model::{UnifiedDiffRow, UnifiedRowKind};
use super::width::expand_tabs;

pub fn build_unified_rows(lines: &[String]) -> Vec<UnifiedDiffRow> {
    let mut rows = Vec::new();
    let mut pending_del: Vec<String> = Vec::new();
    let mut pending_add: Vec<String> = Vec::new();

    for line in lines {
        if line.starts_with("diff --git ") {
            flush_changes(&mut rows, &mut pending_del, &mut pending_add);
            let mut row = UnifiedDiffRow::meta(UnifiedRowKind::File, line.clone());
            row.file_path = diff_git_path(line).map(str::to_string);
            rows.push(row);
            continue;
        }

        if is_metadata_line(line) {
            flush_changes(&mut rows, &mut pending_del, &mut pending_add);
            rows.push(UnifiedDiffRow::meta(UnifiedRowKind::Meta, line.clone()));
            continue;
        }

        if line.starts_with("@@") {
            flush_changes(&mut rows, &mut pending_del, &mut pending_add);
            rows.push(UnifiedDiffRow::meta(UnifiedRowKind::Hunk, line.clone()));
            continue;
        }

        if line.starts_with("\\ No newline") {
            flush_changes(&mut rows, &mut pending_del, &mut pending_add);
            rows.push(UnifiedDiffRow::meta(
                UnifiedRowKind::NoNewline,
                line.clone(),
            ));
            continue;
        }

        let Some(first) = line.chars().next() else {
            flush_changes(&mut rows, &mut pending_del, &mut pending_add);
            rows.push(UnifiedDiffRow::meta(UnifiedRowKind::Blank, String::new()));
            continue;
        };

        match first {
            ' ' => {
                flush_changes(&mut rows, &mut pending_del, &mut pending_add);
                rows.push(UnifiedDiffRow::code(
                    UnifiedRowKind::Context,
                    ' ',
                    expand_tabs(line.get(1..).unwrap_or("")),
                    Vec::new(),
                ));
            }
            '-' => pending_del.push(expand_tabs(line.get(1..).unwrap_or(""))),
            '+' => pending_add.push(expand_tabs(line.get(1..).unwrap_or(""))),
            _ => {
                flush_changes(&mut rows, &mut pending_del, &mut pending_add);
                rows.push(UnifiedDiffRow::meta(UnifiedRowKind::Preamble, line.clone()));
            }
        }
    }

    flush_changes(&mut rows, &mut pending_del, &mut pending_add);
    rows
}

fn flush_changes(
    rows: &mut Vec<UnifiedDiffRow>,
    deletes: &mut Vec<String>,
    adds: &mut Vec<String>,
) {
    if deletes.is_empty() && adds.is_empty() {
        return;
    }

    let (delete_spans, add_spans, _) = pair_inline_spans(deletes, adds);
    for (index, line) in deletes.iter().enumerate() {
        rows.push(UnifiedDiffRow::code(
            UnifiedRowKind::Delete,
            '-',
            line.clone(),
            delete_spans.get(index).cloned().unwrap_or_default(),
        ));
    }
    for (index, line) in adds.iter().enumerate() {
        rows.push(UnifiedDiffRow::code(
            UnifiedRowKind::Add,
            '+',
            line.clone(),
            add_spans.get(index).cloned().unwrap_or_default(),
        ));
    }

    deletes.clear();
    adds.clear();
}

fn is_metadata_line(line: &str) -> bool {
    line.starts_with("index ")
        || line.starts_with("--- ")
        || line.starts_with("+++ ")
        || line.starts_with("rename ")
        || line.starts_with("new file ")
        || line.starts_with("deleted file ")
        || line.starts_with("similarity index ")
        || line.starts_with("Binary files ")
}

pub fn diff_git_path(line: &str) -> Option<&str> {
    line.strip_prefix("diff --git a/")?.split(" b/").next()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unified_rows_carry_inline_spans() {
        let lines = vec![
            "diff --git a/main.rs b/main.rs".to_string(),
            "@@ -1 +1 @@".to_string(),
            "-let color = \"red\";".to_string(),
            "+let color = \"blue\";".to_string(),
        ];
        let rows = build_unified_rows(&lines);
        let delete = rows
            .iter()
            .find(|row| row.kind == UnifiedRowKind::Delete)
            .unwrap();
        let add = rows
            .iter()
            .find(|row| row.kind == UnifiedRowKind::Add)
            .unwrap();
        assert_eq!(
            &delete.code[delete.inline_spans[0].start..delete.inline_spans[0].end],
            "red"
        );
        assert_eq!(
            &add.code[add.inline_spans[0].start..add.inline_spans[0].end],
            "blue"
        );
    }

    #[test]
    fn unified_rows_classify_metadata() {
        let rows = build_unified_rows(&[
            "diff --git a/a b/a".to_string(),
            "new file mode 100644".to_string(),
            "Binary files /dev/null and b/a differ".to_string(),
        ]);
        assert_eq!(rows[0].kind, UnifiedRowKind::File);
        assert_eq!(rows[1].kind, UnifiedRowKind::Meta);
        assert_eq!(rows[2].kind, UnifiedRowKind::Meta);
    }
}
