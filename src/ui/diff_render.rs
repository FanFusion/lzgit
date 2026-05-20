//! Shared comview-inspired diff rendering for Git and History tabs.

use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

use crate::diff::{
    GitDiffCell, GitDiffCellKind, GitDiffRow, InlineSpan, UnifiedDiffRow, UnifiedRowKind,
    build_side_by_side_rows, build_unified_rows, display_width, slice_chars,
};
use crate::git::GitDiffMode;
use crate::highlight::{Highlighter, new_highlighter};
use crate::theme;

const GUTTER_WIDTH: usize = 6;

pub(crate) struct DiffRenderConfig<'a> {
    pub palette: theme::Palette,
    pub mode: GitDiffMode,
    pub content_width: usize,
    pub wrap: bool,
    pub syntax_highlight: bool,
    pub scroll_x: usize,
    pub initial_path: Option<&'a str>,
    pub header_lines: &'a [String],
    pub diff_lines: &'a [String],
    pub include_side_titles: bool,
}

pub(crate) fn render_diff(config: DiffRenderConfig<'_>) -> Vec<Line<'static>> {
    let mut out = render_header_lines(config.header_lines, config.content_width, config.palette);
    if !config.header_lines.is_empty() && !config.diff_lines.is_empty() {
        out.push(Line::raw(""));
    }

    let mut diff_lines = match config.mode {
        GitDiffMode::Unified => render_unified_diff(&config),
        GitDiffMode::SideBySide => render_side_by_side_diff(&config),
    };
    out.append(&mut diff_lines);
    out
}

fn render_header_lines(
    header_lines: &[String],
    content_width: usize,
    palette: theme::Palette,
) -> Vec<Line<'static>> {
    let mut out = Vec::new();
    for line in header_lines {
        if line.starts_with('─') {
            out.push(Line::from(vec![Span::styled(
                "─".repeat(content_width),
                Style::default().fg(palette.border_inactive),
            )]));
        } else if out.is_empty() && !line.is_empty() {
            out.push(Line::from(vec![Span::styled(
                line.clone(),
                Style::default()
                    .fg(palette.accent_primary)
                    .add_modifier(Modifier::BOLD),
            )]));
        } else if is_commit_meta_line(line) {
            let (label, value) = split_label(line);
            out.push(Line::from(vec![
                Span::styled(label, Style::default().fg(palette.border_inactive)),
                Span::styled(value, Style::default().fg(palette.accent_secondary)),
            ]));
        } else {
            out.push(Line::from(vec![Span::styled(
                line.clone(),
                Style::default().fg(palette.fg),
            )]));
        }
    }
    out
}

fn render_unified_diff(config: &DiffRenderConfig<'_>) -> Vec<Line<'static>> {
    let mut out = Vec::new();
    let rows = build_unified_rows(config.diff_lines);
    let mut first_file = true;
    let mut old_hl = highlighter_for(config.syntax_highlight, config.initial_path);
    let mut new_hl = highlighter_for(config.syntax_highlight, config.initial_path);

    for row in rows {
        match row.kind {
            UnifiedRowKind::File => {
                if !first_file {
                    out.push(Line::raw(""));
                    out.push(separator_line(config.content_width, config.palette));
                }
                first_file = false;
                if let Some(path) = row.file_path.as_deref() {
                    old_hl = highlighter_for(config.syntax_highlight, Some(path));
                    new_hl = highlighter_for(config.syntax_highlight, Some(path));
                }
                out.push(file_header_line(
                    &row.text,
                    config.content_width,
                    config.palette,
                ));
            }
            UnifiedRowKind::Hunk => {
                out.push(Line::raw(""));
                out.push(hunk_header_line(
                    &row.text,
                    config.content_width,
                    config.palette,
                ));
            }
            UnifiedRowKind::Meta | UnifiedRowKind::NoNewline | UnifiedRowKind::Preamble => {
                out.push(meta_line(&row.text, config.content_width, config.palette));
            }
            UnifiedRowKind::Blank => out.push(Line::raw("")),
            UnifiedRowKind::Context | UnifiedRowKind::Add | UnifiedRowKind::Delete => {
                out.push(render_unified_code_row(
                    &row,
                    config.content_width,
                    config.palette,
                    config.syntax_highlight,
                    &mut old_hl,
                    &mut new_hl,
                ));
            }
        }
    }

    out
}

fn render_side_by_side_diff(config: &DiffRenderConfig<'_>) -> Vec<Line<'static>> {
    let total_w = config.content_width;
    if total_w < 33 {
        return vec![
            Line::from(vec![Span::styled(
                "Window too narrow for side-by-side view",
                Style::default().fg(config.palette.accent_secondary),
            )]),
            Line::from(vec![Span::styled(
                "Press 's' to switch to unified mode, or widen the window",
                Style::default().fg(config.palette.border_inactive),
            )]),
        ];
    }

    let left_w = total_w.saturating_sub(1) / 2;
    let right_w = total_w.saturating_sub(1).saturating_sub(left_w);
    let mut out = Vec::new();

    if config.include_side_titles {
        out.push(Line::from(vec![
            Span::styled(
                fill_text(" Old ", left_w),
                Style::default()
                    .fg(config.palette.fg)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("│", separator_style(config.palette)),
            Span::styled(
                fill_text(" New ", right_w),
                Style::default()
                    .fg(config.palette.fg)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
    }

    let rows = build_side_by_side_rows(config.diff_lines);
    let mut first_file = true;
    let mut old_hl = highlighter_for(config.syntax_highlight, config.initial_path);
    let mut new_hl = highlighter_for(config.syntax_highlight, config.initial_path);

    for row in rows {
        match row {
            GitDiffRow::Meta(text) if text.starts_with("diff --git") => {
                if !first_file {
                    out.push(Line::raw(""));
                    out.push(separator_line(total_w, config.palette));
                }
                first_file = false;
                let path = diff_git_path(&text).unwrap_or(text.as_str());
                old_hl = highlighter_for(config.syntax_highlight, Some(path));
                new_hl = highlighter_for(config.syntax_highlight, Some(path));
                out.push(file_header_line(&text, total_w, config.palette));
            }
            GitDiffRow::Meta(text) if text.starts_with("@@") => {
                out.push(Line::raw(""));
                out.push(hunk_header_line(&text, total_w, config.palette));
            }
            GitDiffRow::Meta(text) => out.push(meta_line(&text, total_w, config.palette)),
            GitDiffRow::Split { old, new } => {
                let old_lines = render_side_cell_lines(
                    &old,
                    left_w,
                    config.scroll_x,
                    config.wrap,
                    config.palette,
                    config.syntax_highlight,
                    &mut old_hl,
                    Side::Left,
                );
                let new_lines = render_side_cell_lines(
                    &new,
                    right_w,
                    config.scroll_x,
                    config.wrap,
                    config.palette,
                    config.syntax_highlight,
                    &mut new_hl,
                    Side::Right,
                );
                let count = old_lines.len().max(new_lines.len());
                for i in 0..count {
                    let mut spans = old_lines
                        .get(i)
                        .cloned()
                        .unwrap_or_else(|| blank_cell(left_w, config.palette));
                    spans.push(Span::styled("│", separator_style(config.palette)));
                    spans.extend(
                        new_lines
                            .get(i)
                            .cloned()
                            .unwrap_or_else(|| blank_cell(right_w, config.palette)),
                    );
                    out.push(Line::from(spans));
                }
            }
        }
    }

    out
}

#[derive(Clone, Copy)]
enum Side {
    Left,
    Right,
}

fn render_side_cell_lines(
    cell: &GitDiffCell,
    width: usize,
    scroll_x: usize,
    wrap: bool,
    palette: theme::Palette,
    syntax_highlight: bool,
    highlighter: &mut Option<Highlighter>,
    side: Side,
) -> Vec<Vec<Span<'static>>> {
    if width == 0 {
        return vec![Vec::new()];
    }
    if width <= GUTTER_WIDTH {
        return vec![gutter_spans(cell, side, palette.bg, palette)];
    }

    let bg = cell_bg(cell.kind, palette);
    let inline_bg = inline_bg(cell.kind, palette);
    let code_w = width - GUTTER_WIDTH;
    let slices = code_slices(&cell.text, code_w, scroll_x, wrap);
    let syntax_spans = syntax_spans_for_cell(cell, bg, syntax_highlight, highlighter);
    let mut out = Vec::new();

    for (index, slice) in slices.iter().enumerate() {
        let mut spans = if index == 0 {
            gutter_spans(cell, side, bg, palette)
        } else {
            empty_gutter(bg, palette)
        };
        let code_spans = styled_code_slice(
            &cell.text,
            &syntax_spans,
            &cell.inline_spans,
            inline_bg,
            slice.start_col,
            slice.width,
            bg,
            palette.fg,
        );
        spans.extend(code_spans);
        if slice.truncated {
            spans.push(Span::styled(
                "→",
                Style::default().fg(palette.border_inactive).bg(bg),
            ));
        }
        pad_spans_to_width(&mut spans, width, Style::default().bg(bg));
        out.push(spans);
    }

    if out.is_empty() {
        out.push(blank_cell(width, palette));
    }
    out
}

#[derive(Clone, Copy)]
struct CodeSlice {
    start_col: usize,
    width: usize,
    truncated: bool,
}

fn code_slices(text: &str, code_w: usize, scroll_x: usize, wrap: bool) -> Vec<CodeSlice> {
    if code_w == 0 {
        return vec![CodeSlice {
            start_col: 0,
            width: 0,
            truncated: false,
        }];
    }
    let text_w = display_width(text).max(1);
    if !wrap {
        let remaining = text_w.saturating_sub(scroll_x);
        let truncated = remaining > code_w;
        let width = if truncated && code_w > 1 {
            code_w - 1
        } else {
            code_w
        };
        return vec![CodeSlice {
            start_col: scroll_x,
            width,
            truncated,
        }];
    }

    let lines = text_w.div_ceil(code_w).max(1);
    (0..lines)
        .map(|line| CodeSlice {
            start_col: line * code_w,
            width: code_w,
            truncated: false,
        })
        .collect()
}

fn syntax_spans_for_cell(
    cell: &GitDiffCell,
    bg: Color,
    syntax_highlight: bool,
    highlighter: &mut Option<Highlighter>,
) -> Vec<Span<'static>> {
    let base = Style::default().fg(Color::Reset).bg(bg);
    if !syntax_highlight || cell.kind == GitDiffCellKind::Empty || cell.text.trim().is_empty() {
        return vec![Span::styled(cell.text.clone(), base)];
    }
    if let Some(highlighter) = highlighter.as_mut() {
        highlighter.highlight_line(&cell.text, bg).spans
    } else {
        vec![Span::styled(cell.text.clone(), base)]
    }
}

fn render_unified_code_row(
    row: &UnifiedDiffRow,
    content_width: usize,
    palette: theme::Palette,
    syntax_highlight: bool,
    old_hl: &mut Option<Highlighter>,
    new_hl: &mut Option<Highlighter>,
) -> Line<'static> {
    let bg = unified_row_bg(row.kind, palette);
    let marker_fg = match row.kind {
        UnifiedRowKind::Add => palette.diff_add_fg,
        UnifiedRowKind::Delete => palette.diff_del_fg,
        _ => palette.diff_gutter_fg,
    };
    let mut spans = vec![Span::styled(
        row.marker.unwrap_or(' ').to_string(),
        Style::default().fg(marker_fg).bg(bg),
    )];

    let highlighter = match row.kind {
        UnifiedRowKind::Delete => old_hl,
        UnifiedRowKind::Add => new_hl,
        UnifiedRowKind::Context => {
            let old_syntax = syntax_spans_for_text(&row.code, bg, syntax_highlight, old_hl);
            let _ = syntax_spans_for_text(&row.code, bg, syntax_highlight, new_hl);
            let code_spans = styled_code_slice_from_syntax(
                &row.code,
                &old_syntax,
                &row.inline_spans,
                inline_bg_for_unified(row.kind, palette),
                0,
                content_width.saturating_sub(1),
                bg,
                palette.fg,
            );
            spans.extend(code_spans);
            pad_spans_to_width(&mut spans, content_width, Style::default().bg(bg));
            return Line::from(spans);
        }
        _ => old_hl,
    };

    let syntax = syntax_spans_for_text(&row.code, bg, syntax_highlight, highlighter);
    spans.extend(styled_code_slice_from_syntax(
        &row.code,
        &syntax,
        &row.inline_spans,
        inline_bg_for_unified(row.kind, palette),
        0,
        content_width.saturating_sub(1),
        bg,
        palette.fg,
    ));
    pad_spans_to_width(&mut spans, content_width, Style::default().bg(bg));
    Line::from(spans)
}

fn syntax_spans_for_text(
    text: &str,
    bg: Color,
    syntax_highlight: bool,
    highlighter: &mut Option<Highlighter>,
) -> Vec<Span<'static>> {
    if !syntax_highlight || text.trim().is_empty() {
        return vec![Span::styled(
            text.to_string(),
            Style::default().fg(Color::Reset).bg(bg),
        )];
    }
    if let Some(highlighter) = highlighter.as_mut() {
        highlighter.highlight_line(text, bg).spans
    } else {
        vec![Span::styled(
            text.to_string(),
            Style::default().fg(Color::Reset).bg(bg),
        )]
    }
}

fn styled_code_slice(
    code: &str,
    syntax_spans: &[Span<'_>],
    inline_spans: &[InlineSpan],
    inline_bg: Option<Color>,
    start_col: usize,
    width: usize,
    bg: Color,
    fallback_fg: Color,
) -> Vec<Span<'static>> {
    styled_code_slice_from_syntax(
        code,
        syntax_spans,
        inline_spans,
        inline_bg,
        start_col,
        width,
        bg,
        fallback_fg,
    )
}

fn styled_code_slice_from_syntax(
    code: &str,
    syntax_spans: &[Span<'_>],
    inline_spans: &[InlineSpan],
    inline_bg: Option<Color>,
    start_col: usize,
    width: usize,
    bg: Color,
    fallback_fg: Color,
) -> Vec<Span<'static>> {
    if width == 0 {
        return Vec::new();
    }

    let style_ranges = syntax_style_ranges(syntax_spans, bg, fallback_fg);
    let end_col = start_col + width;
    let mut col = 0usize;
    let mut out: Vec<Span<'static>> = Vec::new();

    for (byte, ch) in code.char_indices() {
        let ch_width = crate::diff::width::char_width(ch);
        let next_col = col + ch_width;
        if next_col <= start_col {
            col = next_col;
            continue;
        }
        if col >= end_col || next_col > end_col {
            break;
        }

        let mut style = style_for_byte(&style_ranges, byte, bg, fallback_fg);
        if inline_spans
            .iter()
            .any(|span| byte < span.end && byte + ch.len_utf8() > span.start)
        {
            if let Some(inline_bg) = inline_bg {
                style = style.bg(inline_bg);
            }
        }
        if ch == '\t' {
            append_span(&mut out, Span::styled("    ".to_string(), style));
        } else {
            append_span(&mut out, Span::styled(ch.to_string(), style));
        }
        col = next_col;
    }

    let current = spans_width(&out);
    if current < width {
        append_span(
            &mut out,
            Span::styled(" ".repeat(width - current), Style::default().bg(bg)),
        );
    }
    out
}

fn syntax_style_ranges(
    spans: &[Span<'_>],
    bg: Color,
    fallback_fg: Color,
) -> Vec<(usize, usize, Style)> {
    let mut ranges = Vec::new();
    let mut offset = 0usize;
    for span in spans {
        let text = span.content.as_ref();
        let mut style = span.style;
        if style.bg.is_none() {
            style = style.bg(bg);
        }
        if style.fg.is_none() {
            style = style.fg(fallback_fg);
        }
        ranges.push((offset, offset + text.len(), style));
        offset += text.len();
    }
    ranges
}

fn style_for_byte(
    ranges: &[(usize, usize, Style)],
    byte: usize,
    bg: Color,
    fallback_fg: Color,
) -> Style {
    ranges
        .iter()
        .find(|(start, end, _)| byte >= *start && byte < *end)
        .map(|(_, _, style)| *style)
        .unwrap_or_else(|| Style::default().fg(fallback_fg).bg(bg))
}

fn gutter_spans(
    cell: &GitDiffCell,
    side: Side,
    bg: Color,
    palette: theme::Palette,
) -> Vec<Span<'static>> {
    let marker = match (side, cell.kind) {
        (Side::Left, GitDiffCellKind::Delete) => '-',
        (Side::Right, GitDiffCellKind::Add) => '+',
        _ => ' ',
    };
    let line = cell
        .line_no
        .map(|n| format!("{:>4}", n))
        .unwrap_or_else(|| "    ".to_string());
    let marker_fg = match marker {
        '+' => palette.diff_add_fg,
        '-' => palette.diff_del_fg,
        _ => palette.diff_gutter_fg,
    };
    vec![
        Span::styled(line, Style::default().fg(palette.diff_gutter_fg).bg(bg)),
        Span::styled(marker.to_string(), Style::default().fg(marker_fg).bg(bg)),
        Span::styled(" ", Style::default().fg(palette.diff_gutter_fg).bg(bg)),
    ]
}

fn empty_gutter(bg: Color, palette: theme::Palette) -> Vec<Span<'static>> {
    vec![Span::styled(
        " ".repeat(GUTTER_WIDTH),
        Style::default().fg(palette.diff_gutter_fg).bg(bg),
    )]
}

fn blank_cell(width: usize, palette: theme::Palette) -> Vec<Span<'static>> {
    vec![Span::styled(
        " ".repeat(width),
        Style::default().bg(palette.bg),
    )]
}

fn file_header_line(text: &str, width: usize, palette: theme::Palette) -> Line<'static> {
    let path = diff_git_path(text).unwrap_or(text);
    let (dir, file) = split_dir_file(path);
    let mut spans = vec![Span::styled(
        format!("📄 {}", file),
        Style::default()
            .fg(palette.accent_primary)
            .add_modifier(Modifier::BOLD),
    )];
    if !dir.is_empty() {
        spans.push(Span::styled(
            format!("  {}", dir),
            Style::default().fg(palette.border_inactive),
        ));
    }
    pad_spans_to_width(&mut spans, width, Style::default().bg(palette.bg));
    Line::from(spans)
}

fn hunk_header_line(text: &str, width: usize, palette: theme::Palette) -> Line<'static> {
    let (range, context) = split_hunk_header(text);
    let mut spans = vec![Span::styled(
        range,
        Style::default()
            .fg(palette.accent_secondary)
            .bg(palette.diff_hunk_bg)
            .add_modifier(Modifier::BOLD),
    )];
    if !context.is_empty() {
        spans.push(Span::styled(
            context,
            Style::default()
                .fg(palette.border_inactive)
                .bg(palette.diff_hunk_bg),
        ));
    }
    pad_spans_to_width(&mut spans, width, Style::default().bg(palette.diff_hunk_bg));
    Line::from(spans)
}

fn meta_line(text: &str, width: usize, palette: theme::Palette) -> Line<'static> {
    let mut spans = if let Some((path, rest)) = diff_stat_parts(text) {
        diff_stat_spans(path, rest, palette)
    } else {
        metadata_spans(text, palette)
    };
    pad_spans_to_width(&mut spans, width, Style::default().bg(palette.bg));
    Line::from(spans)
}

fn separator_line(width: usize, palette: theme::Palette) -> Line<'static> {
    Line::from(vec![Span::styled(
        "─".repeat(width),
        Style::default().fg(palette.border_inactive),
    )])
}

fn split_hunk_header(text: &str) -> (String, String) {
    if let Some(start) = text.find(" @@") {
        let split = start + 3;
        if split < text.len() {
            return (text[..split].to_string(), text[split..].to_string());
        }
    }
    (text.to_string(), String::new())
}

fn split_dir_file(path: &str) -> (&str, &str) {
    match path.rfind('/') {
        Some(index) => (&path[..index + 1], &path[index + 1..]),
        None => ("", path),
    }
}

fn diff_git_path(line: &str) -> Option<&str> {
    line.strip_prefix("diff --git a/")?.split(" b/").next()
}

fn highlighter_for(syntax_highlight: bool, path: Option<&str>) -> Option<Highlighter> {
    if !syntax_highlight {
        return None;
    }
    path.and_then(|path| std::path::Path::new(path).extension())
        .and_then(|ext| ext.to_str())
        .and_then(new_highlighter)
}

fn cell_bg(kind: GitDiffCellKind, palette: theme::Palette) -> Color {
    match kind {
        GitDiffCellKind::Delete => palette.diff_del_bg,
        GitDiffCellKind::Add => palette.diff_add_bg,
        _ => palette.bg,
    }
}

fn unified_row_bg(kind: UnifiedRowKind, palette: theme::Palette) -> Color {
    match kind {
        UnifiedRowKind::Delete => palette.diff_del_bg,
        UnifiedRowKind::Add => palette.diff_add_bg,
        _ => palette.bg,
    }
}

fn inline_bg(kind: GitDiffCellKind, palette: theme::Palette) -> Option<Color> {
    match kind {
        GitDiffCellKind::Delete => Some(palette.diff_del_inline_bg),
        GitDiffCellKind::Add => Some(palette.diff_add_inline_bg),
        _ => None,
    }
}

fn inline_bg_for_unified(kind: UnifiedRowKind, palette: theme::Palette) -> Option<Color> {
    match kind {
        UnifiedRowKind::Delete => Some(palette.diff_del_inline_bg),
        UnifiedRowKind::Add => Some(palette.diff_add_inline_bg),
        _ => None,
    }
}

fn separator_style(palette: theme::Palette) -> Style {
    Style::default().fg(palette.border_inactive).bg(palette.bg)
}

fn fill_text(text: &str, width: usize) -> String {
    let mut out = slice_chars(text, 0, width);
    let w = display_width(&out);
    if w < width {
        out.push_str(&" ".repeat(width - w));
    }
    out
}

fn pad_spans_to_width(spans: &mut Vec<Span<'static>>, width: usize, style: Style) {
    let current = spans_width(spans);
    if current < width {
        append_span(spans, Span::styled(" ".repeat(width - current), style));
    }
}

fn spans_width(spans: &[Span<'_>]) -> usize {
    spans
        .iter()
        .map(|span| display_width(span.content.as_ref()))
        .sum()
}

fn append_span(spans: &mut Vec<Span<'static>>, span: Span<'static>) {
    if span.content.is_empty() {
        return;
    }
    if let Some(last) = spans.last_mut() {
        if last.style == span.style {
            let mut text = last.content.to_string();
            text.push_str(span.content.as_ref());
            *last = Span::styled(text, last.style);
            return;
        }
    }
    spans.push(span);
}

fn is_commit_meta_line(line: &str) -> bool {
    matches!(
        line.split_once(':').map(|(key, _)| key),
        Some("Author" | "Date" | "AuthorDate" | "Commit" | "CommitDate")
    )
}

fn split_label(line: &str) -> (String, String) {
    if let Some((label, value)) = line.split_once(':') {
        (format!("{}: ", label), value.trim_start().to_string())
    } else {
        (String::new(), line.to_string())
    }
}

fn diff_stat_parts(line: &str) -> Option<(&str, &str)> {
    let trimmed = line.trim_start();
    let (path, rest) = trimmed.split_once(" | ")?;
    if rest.chars().any(|ch| ch == '+' || ch == '-') {
        Some((path, rest))
    } else {
        None
    }
}

fn diff_stat_spans(path: &str, rest: &str, palette: theme::Palette) -> Vec<Span<'static>> {
    let mut spans = vec![
        Span::styled(path.to_string(), Style::default().fg(palette.fg)),
        Span::styled(" | ", Style::default().fg(palette.border_inactive)),
    ];
    for ch in rest.chars() {
        let fg = match ch {
            '+' => palette.diff_add_fg,
            '-' => palette.diff_del_fg,
            _ => palette.border_inactive,
        };
        spans.push(Span::styled(ch.to_string(), Style::default().fg(fg)));
    }
    spans
}

fn metadata_spans(text: &str, palette: theme::Palette) -> Vec<Span<'static>> {
    let style = Style::default();
    if let Some(path) = text.strip_prefix("--- ") {
        return prefixed_metadata_spans("--- ", path, palette.diff_del_fg, palette);
    }
    if let Some(path) = text.strip_prefix("+++ ") {
        return prefixed_metadata_spans("+++ ", path, palette.diff_add_fg, palette);
    }
    if let Some(rest) = text.strip_prefix("new file mode ") {
        return vec![
            Span::styled("new file mode ", style.fg(palette.diff_add_fg)),
            Span::styled(rest.to_string(), style.fg(palette.fg)),
        ];
    }
    if let Some(rest) = text.strip_prefix("deleted file mode ") {
        return vec![
            Span::styled("deleted file mode ", style.fg(palette.diff_del_fg)),
            Span::styled(rest.to_string(), style.fg(palette.fg)),
        ];
    }
    if let Some(path) = text.strip_prefix("rename from ") {
        return prefixed_metadata_spans("rename from ", path, palette.diff_del_fg, palette);
    }
    if let Some(path) = text.strip_prefix("rename to ") {
        return prefixed_metadata_spans("rename to ", path, palette.diff_add_fg, palette);
    }
    if text.starts_with("Binary files ") || text.starts_with("similarity index ") {
        return vec![Span::styled(
            text.to_string(),
            style.fg(palette.accent_secondary),
        )];
    }
    if text.starts_with("\\ No newline") {
        return vec![Span::styled(
            text.to_string(),
            style.fg(palette.accent_secondary),
        )];
    }
    if text.starts_with("index ") {
        return vec![Span::styled(
            text.to_string(),
            style.fg(palette.diff_gutter_fg),
        )];
    }

    vec![Span::styled(
        text.to_string(),
        style.fg(palette.border_inactive),
    )]
}

fn prefixed_metadata_spans(
    prefix: &'static str,
    value: &str,
    accent: Color,
    palette: theme::Palette,
) -> Vec<Span<'static>> {
    vec![
        Span::styled(
            prefix,
            Style::default().fg(accent).add_modifier(Modifier::BOLD),
        ),
        Span::styled(value.to_string(), Style::default().fg(palette.fg)),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inline_overlay_preserves_foreground_and_changes_background() {
        let spans = vec![Span::styled(
            "let color = red;",
            Style::default().fg(Color::Blue).bg(Color::Black),
        )];
        let rendered = styled_code_slice_from_syntax(
            "let color = red;",
            &spans,
            &[InlineSpan { start: 12, end: 15 }],
            Some(Color::Red),
            0,
            16,
            Color::Black,
            Color::White,
        );
        let red_span = rendered
            .iter()
            .find(|span| span.content.contains("red"))
            .unwrap();
        assert_eq!(red_span.style.fg, Some(Color::Blue));
        assert_eq!(red_span.style.bg, Some(Color::Red));
    }

    #[test]
    fn side_by_side_renderer_outputs_separator_and_inline_style() {
        let lines = vec![
            "diff --git a/main.rs b/main.rs".to_string(),
            "@@ -1 +1 @@".to_string(),
            "-let color = \"red\";".to_string(),
            "+let color = \"blue\";".to_string(),
        ];
        let rendered = render_diff(DiffRenderConfig {
            palette: theme::palette(theme::Theme::Terminal),
            mode: GitDiffMode::SideBySide,
            content_width: 80,
            wrap: false,
            syntax_highlight: false,
            scroll_x: 0,
            initial_path: Some("main.rs"),
            header_lines: &[],
            diff_lines: &lines,
            include_side_titles: true,
        });
        let text: String = rendered
            .iter()
            .flat_map(|line| line.spans.iter())
            .map(|span| span.content.as_ref())
            .collect();
        assert!(text.contains('│'));
        assert!(text.contains("red"));
        assert!(text.contains("blue"));
        assert!(
            rendered
                .iter()
                .flat_map(|line| line.spans.iter())
                .any(|span| span.style.bg
                    == Some(theme::palette(theme::Theme::Terminal).diff_del_inline_bg))
        );
    }
}
