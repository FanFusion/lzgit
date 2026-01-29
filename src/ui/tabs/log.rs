//! Log tab rendering - commit history, reflog, stash views with diff display

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Margin, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Wrap},
};
use std::time::Instant;

use crate::git::{
    self, GitDiffCellKind, GitDiffMode, GitDiffRow, build_side_by_side_rows, display_width,
    pad_to_width,
};
use crate::git_ops;
use crate::highlight::{Highlighter, new_highlighter};
use crate::theme;
use crate::{App, AppAction, ClickZone, DiffRenderCacheKey, LogDetailMode, LogSubTab, LogZoom};

/// Render the Log tab content: subtab selector, commit list, and diff view
pub fn render_log_tab(
    app: &mut App,
    f: &mut Frame,
    content_area: Rect,
    zones: &mut Vec<ClickZone>,
) {
    let zoom = app.log_ui.zoom;

    let (subtab_area, list_area, diff_area) = match zoom {
        LogZoom::None => {
            let chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Length(app.log_ui.left_width),
                    Constraint::Min(0),
                ])
                .split(content_area);

            let left_area = chunks[0];
            let diff_area = chunks[1];
            let left_chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(1), Constraint::Min(0)])
                .split(left_area);
            (left_chunks[0], left_chunks[1], diff_area)
        }
        LogZoom::List => {
            let rows = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(1), Constraint::Min(0)])
                .split(content_area);
            (
                rows[0],
                rows[1],
                Rect::new(content_area.x, content_area.y, 0, 0),
            )
        }
        LogZoom::Diff => {
            let rows = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(1), Constraint::Min(0)])
                .split(content_area);
            (
                rows[0],
                Rect::new(content_area.x, content_area.y, 0, 0),
                rows[1],
            )
        }
    };

    if zoom == LogZoom::List {
        app.log_files_x = u16::MAX;
        app.log_diff_x = u16::MAX;
    } else {
        app.log_files_x = diff_area.x;
        app.log_diff_x = diff_area.x;
    }

    // Render subtab selector
    render_subtab_selector(app, f, subtab_area, zones);

    // Render list area (if not zoomed to diff)
    if zoom != LogZoom::Diff {
        render_log_list(app, f, list_area, zones);
    }

    // Render diff area (if not zoomed to list)
    if zoom != LogZoom::List {
        render_log_diff(app, f, diff_area, zones);
    }
}

/// Render the subtab selector (History, Reflog, Stash, Commands)
fn render_subtab_selector(app: &App, f: &mut Frame, subtab_area: Rect, zones: &mut Vec<ClickZone>) {
    let mut x = subtab_area.x;
    let max_x = subtab_area.x + subtab_area.width;
    for (label, subtab) in [
        (" History ", LogSubTab::History),
        (" Reflog ", LogSubTab::Reflog),
        (" Stash ", LogSubTab::Stash),
        (" Comm ", LogSubTab::Commands),
    ] {
        let w = label.len() as u16;
        // Skip if we've run out of horizontal space
        if x >= max_x {
            break;
        }
        // Clip width to not extend past subtab_area
        let clipped_w = w.min(max_x.saturating_sub(x));
        let active = app.log_ui.subtab == subtab;
        let style = if active {
            Style::default()
                .bg(app.palette.accent_primary)
                .fg(app.palette.btn_fg)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().bg(app.palette.bg).fg(app.palette.fg)
        };
        let rect = Rect::new(x, subtab_area.y, clipped_w, 1);
        f.render_widget(Paragraph::new(label).style(style), rect);
        zones.push(ClickZone {
            rect,
            action: AppAction::LogSwitch(subtab),
        });
        x += w + 1;
    }
}

/// Render the log list (history, reflog, stash, or commands)
fn render_log_list(app: &mut App, f: &mut Frame, list_area: Rect, zones: &mut Vec<ClickZone>) {
    let (title, items_len) = match app.log_ui.subtab {
        LogSubTab::History => (" History ", app.log_ui.history_filtered.len()),
        LogSubTab::Reflog => (" Reflog ", app.log_ui.reflog_filtered.len()),
        LogSubTab::Stash => (" Stash ", app.log_ui.stash_filtered.len()),
        LogSubTab::Commands => (" Commands ", app.git_log.len()),
    };

    let (list_title, border_color) = if app.log_ui.subtab != LogSubTab::Commands {
        let q = app.log_ui.filter_query.trim();
        let filter_label = if q.is_empty() {
            "filter: /".to_string()
        } else {
            format!("filter: {}", q)
        };
        let filter_style = if app.log_ui.filter_edit {
            Style::default()
                .fg(app.palette.accent_primary)
                .add_modifier(Modifier::BOLD)
        } else if !q.is_empty() {
            Style::default().fg(app.palette.accent_primary)
        } else {
            Style::default().fg(app.palette.size_color)
        };

        (
            Line::from(vec![
                Span::raw(format!("{}({})  ", title, items_len)),
                Span::styled(filter_label, filter_style),
            ]),
            if app.log_ui.filter_edit || !q.is_empty() {
                app.palette.accent_primary
            } else {
                app.palette.border_inactive
            },
        )
    } else {
        (
            Line::raw(format!("{}({})", title, items_len)),
            app.palette.border_inactive,
        )
    };

    let list_block = Block::default()
        .borders(Borders::ALL)
        .border_set(ratatui::symbols::border::PLAIN)
        .border_style(Style::default().fg(border_color))
        .title(list_title);

    let list_items: Vec<ListItem> = match app.log_ui.subtab {
        LogSubTab::History => app
            .log_ui
            .history_filtered
            .iter()
            .filter_map(|idx| app.log_ui.history.get(*idx))
            .map(|e| ListItem::new(log_history_line(e, app.palette)))
            .collect(),
        LogSubTab::Reflog => app
            .log_ui
            .reflog_filtered
            .iter()
            .filter_map(|idx| app.log_ui.reflog.get(*idx))
            .map(|e| ListItem::new(log_reflog_line(e, app.palette)))
            .collect(),
        LogSubTab::Stash => app
            .log_ui
            .stash_filtered
            .iter()
            .filter_map(|idx| app.log_ui.stash.get(*idx))
            .map(|e| ListItem::new(format!("{}  {}", e.selector, e.subject)))
            .collect(),
        LogSubTab::Commands => {
            let now = Instant::now();
            app.git_log
                .iter()
                .map(|e| {
                    let age = now.duration_since(e.when).as_secs();
                    let tag = if e.ok { "ok" } else { "err" };
                    ListItem::new(format!("[{tag}] +{age}s  {}", e.cmd))
                })
                .collect()
        }
    };

    let list = List::new(list_items)
        .block(list_block)
        .highlight_style(
            Style::default()
                .bg(app.palette.selection_bg)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▎ ");

    let selected_idx = match app.log_ui.subtab {
        LogSubTab::History => {
            f.render_stateful_widget(list, list_area, &mut app.log_ui.history_state);
            app.log_ui.history_state.selected().unwrap_or(0)
        }
        LogSubTab::Reflog => {
            f.render_stateful_widget(list, list_area, &mut app.log_ui.reflog_state);
            app.log_ui.reflog_state.selected().unwrap_or(0)
        }
        LogSubTab::Stash => {
            f.render_stateful_widget(list, list_area, &mut app.log_ui.stash_state);
            app.log_ui.stash_state.selected().unwrap_or(0)
        }
        LogSubTab::Commands => {
            f.render_stateful_widget(list, list_area, &mut app.log_ui.command_state);
            app.log_ui.command_state.selected().unwrap_or(0)
        }
    };

    // Scrollbar for list - use max scroll range so thumb reaches bottom
    let list_viewport_h = list_area.height.saturating_sub(2) as usize;
    let list_max_scroll = items_len.saturating_sub(list_viewport_h).max(1);
    if items_len > list_viewport_h {
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(Some("▴"))
            .end_symbol(Some("▾"))
            .track_symbol(Some("│"))
            .thumb_symbol("█");
        let mut scroll_state = ScrollbarState::new(list_max_scroll).position(selected_idx.min(list_max_scroll));
        f.render_stateful_widget(
            scrollbar,
            list_area.inner(Margin { vertical: 1, horizontal: 0 }),
            &mut scroll_state,
        );
    }

    let list_inner = list_area.inner(Margin {
        vertical: 1,
        horizontal: 1,
    });

    let offset = match app.log_ui.subtab {
        LogSubTab::History => app.log_ui.history_state.offset(),
        LogSubTab::Reflog => app.log_ui.reflog_state.offset(),
        LogSubTab::Stash => app.log_ui.stash_state.offset(),
        LogSubTab::Commands => app.log_ui.command_state.offset(),
    };

    let end = (offset + list_inner.height as usize).min(items_len);
    for (i, idx) in (offset..end).enumerate() {
        let rect = Rect::new(list_inner.x, list_inner.y + i as u16, list_inner.width, 1);
        zones.push(ClickZone {
            rect,
            action: AppAction::SelectLogItem(idx),
        });
    }
}

/// Render the diff/detail view
fn render_log_diff(app: &mut App, f: &mut Frame, diff_area: Rect, zones: &mut Vec<ClickZone>) {
    let files_mode =
        app.log_ui.detail_mode == LogDetailMode::Files && app.log_ui.subtab == LogSubTab::History;

    let mut diff_view_area = diff_area;
    if files_mode {
        // Use proportional width for sidebar, max 38, min 26
        let files_w = (diff_area.width / 3).clamp(26, 38);
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(files_w), Constraint::Min(0)])
            .split(diff_area);
        let files_area = chunks[0];
        diff_view_area = chunks[1];

        app.log_files_x = files_area.x;
        app.log_diff_x = diff_view_area.x;

        render_files_sidebar(app, f, files_area, zones);
    } else {
        app.log_files_x = diff_area.x;
        app.log_diff_x = diff_area.x;
    }

    render_diff_content(app, f, diff_view_area, zones);
}

/// Render the files sidebar (in Files mode)
fn render_files_sidebar(
    app: &mut App,
    f: &mut Frame,
    files_area: Rect,
    zones: &mut Vec<ClickZone>,
) {
    // Get selected commit info for sidebar header
    let commit_info: Option<(&str, &str, &str)> = app
        .log_ui
        .history
        .get(app.log_ui.history_state.selected().unwrap_or(0))
        .map(|e| (e.subject.as_str(), e.short.as_str(), e.author.as_str()));

    let file_block = Block::default()
        .borders(Borders::ALL)
        .border_set(ratatui::symbols::border::PLAIN)
        .border_style(Style::default().fg(app.palette.border_inactive))
        .title(format!(" Files ({}) ", app.log_ui.files.len()));

    // Render sidebar block
    f.render_widget(file_block.clone(), files_area);
    let inner = files_area.inner(Margin {
        vertical: 1,
        horizontal: 1,
    });

    // Render commit info header, get remaining area for file list
    let list_area = if let Some((subject, hash, author)) = commit_info {
        let max_w = inner.width as usize;
        let subj_display: String = if subject.chars().count() > max_w {
            subject
                .chars()
                .take(max_w.saturating_sub(1))
                .collect::<String>()
                + "…"
        } else {
            subject.to_string()
        };
        let subject_line = Line::from(vec![Span::styled(
            subj_display,
            Style::default()
                .fg(app.palette.fg)
                .add_modifier(Modifier::BOLD),
        )]);
        let meta_line = Line::from(vec![
            Span::styled(
                hash.to_string(),
                Style::default().fg(app.palette.accent_primary),
            ),
            Span::styled(
                format!(" {}", author),
                Style::default().fg(app.palette.border_inactive),
            ),
        ]);
        let sep_line = Line::from(vec![Span::styled(
            "─".repeat(max_w),
            Style::default().fg(app.palette.border_inactive),
        )]);
        let header = Paragraph::new(vec![subject_line, meta_line, sep_line]);
        f.render_widget(header, Rect::new(inner.x, inner.y, inner.width, 3));
        Rect::new(
            inner.x,
            inner.y + 3,
            inner.width,
            inner.height.saturating_sub(3),
        )
    } else {
        inner
    };

    let file_items: Vec<ListItem> = app
        .log_ui
        .files
        .iter()
        .map(|file| {
            // Show filename first, then line stats, then directory in gray
            let (dir, filename) = match file.path.rfind('/') {
                Some(i) => (&file.path[..i + 1], &file.path[i + 1..]),
                None => ("", file.path.as_str()),
            };
            let status_color = match file.status.as_str() {
                "M" => app.palette.accent_secondary, // Modified
                "A" => app.palette.diff_add_fg,      // Added
                "D" => app.palette.diff_del_fg,      // Deleted
                "R" => app.palette.accent_primary,   // Renamed
                _ => app.palette.fg,
            };
            let mut spans = vec![
                Span::styled(
                    format!("{} ", file.status),
                    Style::default().fg(status_color),
                ),
                Span::styled(filename.to_string(), Style::default().fg(app.palette.fg)),
            ];
            // Add line change stats
            if let (Some(adds), Some(dels)) = (file.additions, file.deletions) {
                spans.push(Span::raw(" "));
                if adds > 0 {
                    spans.push(Span::styled(
                        format!("+{}", adds),
                        Style::default().fg(app.palette.diff_add_fg),
                    ));
                }
                if dels > 0 {
                    if adds > 0 {
                        spans.push(Span::raw(" "));
                    }
                    spans.push(Span::styled(
                        format!("-{}", dels),
                        Style::default().fg(app.palette.diff_del_fg),
                    ));
                }
            }
            if !dir.is_empty() {
                spans.push(Span::styled(
                    format!(" {}", dir.trim_end_matches('/')),
                    Style::default().fg(app.palette.border_inactive),
                ));
            }
            ListItem::new(Line::from(spans))
        })
        .collect();

    let file_list = List::new(file_items)
        .highlight_style(
            Style::default()
                .bg(app.palette.selection_bg)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▎ ");

    f.render_stateful_widget(file_list, list_area, &mut app.log_ui.files_state);

    zones.push(ClickZone {
        rect: files_area,
        action: AppAction::LogFocusFiles,
    });

    let list_inner = list_area;

    let items_len = app.log_ui.files.len();
    let offset = app.log_ui.files_state.offset();
    let end = (offset + list_inner.height as usize).min(items_len);
    for (i, idx) in (offset..end).enumerate() {
        let rect = Rect::new(list_inner.x, list_inner.y + i as u16, list_inner.width, 1);
        zones.push(ClickZone {
            rect,
            action: AppAction::SelectLogFile(idx),
        });
    }
}

/// Render the diff content panel
fn render_diff_content(app: &mut App, f: &mut Frame, diff_area: Rect, zones: &mut Vec<ClickZone>) {
    let diff_title = match app.log_ui.subtab {
        LogSubTab::History => match app.log_ui.detail_mode {
            LogDetailMode::Diff => " Commit Diff ",
            LogDetailMode::Files => " Changed Files ",
        },
        LogSubTab::Reflog => match app.log_ui.detail_mode {
            LogDetailMode::Diff => " Reflog ",
            LogDetailMode::Files => " Reflog ",
        },
        LogSubTab::Stash => " Stash ",
        LogSubTab::Commands => " Command Output ",
    };

    let diff_block = Block::default()
        .borders(Borders::ALL)
        .border_set(ratatui::symbols::border::PLAIN)
        .border_style(Style::default().fg(app.palette.border_inactive))
        .title(diff_title);

    let cache_width = diff_area.width.saturating_sub(2).max(1);
    let cache_scroll_x = if app.log_ui.diff_mode == GitDiffMode::Unified && !app.wrap_diff {
        app.log_ui.diff_scroll_x
    } else {
        0
    };
    let cache_key = DiffRenderCacheKey {
        theme: app.theme,
        generation: app.log_ui.diff_generation,
        mode: app.log_ui.diff_mode,
        width: cache_width,
        wrap: app.wrap_diff,
        syntax_highlight: app.syntax_highlight,
        scroll_x: cache_scroll_x,
    };

    let diff_lines: Vec<Line> = if app.log_diff_cache.key == Some(cache_key) {
        app.log_diff_cache.lines.clone()
    } else {
        // Separate header lines (before first diff --git) from diff lines
        let diff_start = app
            .log_ui
            .diff_lines
            .iter()
            .position(|l| l.starts_with("diff --git "))
            .unwrap_or(app.log_ui.diff_lines.len());
        let header_lines = &app.log_ui.diff_lines[..diff_start];
        let diff_only_lines = &app.log_ui.diff_lines[diff_start..];

        let computed: Vec<Line> = match app.log_ui.diff_mode {
            GitDiffMode::Unified => {
                render_log_unified_diff(app, diff_area, header_lines, diff_only_lines)
            }
            GitDiffMode::SideBySide => {
                render_log_side_by_side_diff(app, diff_area, header_lines, diff_only_lines)
            }
        };

        app.log_diff_cache.key = Some(cache_key);
        app.log_diff_cache.lines = computed.clone();
        computed
    };

    let wrap_unified = app.log_ui.diff_mode == GitDiffMode::Unified && app.wrap_diff;

    let viewport_h = diff_area.height.saturating_sub(2) as usize;
    let total_lines = diff_lines.len();
    let max_y = if viewport_h == 0 {
        0
    } else if wrap_unified {
        app.log_ui
            .diff_lines
            .iter()
            .map(|l| {
                let w = (diff_area.width.saturating_sub(2).max(1)) as usize;
                let cols = display_width(l).max(1);
                (cols + w - 1) / w
            })
            .sum::<usize>()
            .saturating_sub(viewport_h)
    } else {
        total_lines.saturating_sub(viewport_h)
    };
    // Clamp to u16::MAX to avoid overflow
    let max_y_u16 = max_y.min(u16::MAX as usize) as u16;
    app.log_ui.diff_scroll_y = app.log_ui.diff_scroll_y.min(max_y_u16);

    let x_scroll = if app.log_ui.diff_mode == GitDiffMode::Unified && !wrap_unified {
        app.log_ui.diff_scroll_x
    } else {
        0
    };
    let mut diff_para = Paragraph::new(diff_lines)
        .block(diff_block)
        .scroll((app.log_ui.diff_scroll_y, x_scroll));
    if wrap_unified {
        diff_para = diff_para.wrap(Wrap { trim: false });
    }

    f.render_widget(diff_para, diff_area);

    // Scrollbar for diff
    let total_lines = if wrap_unified {
        app.log_ui
            .diff_lines
            .iter()
            .map(|l| {
                let w = (diff_area.width.saturating_sub(2).max(1)) as usize;
                let cols = display_width(l).max(1);
                (cols + w - 1) / w
            })
            .sum::<usize>()
    } else {
        app.log_ui.diff_lines.len()
    };
    // Scrollbar - use max scroll range so thumb reaches bottom
    let max_scroll_y = total_lines.saturating_sub(viewport_h).max(1);
    if total_lines > viewport_h {
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(Some("▴"))
            .end_symbol(Some("▾"))
            .track_symbol(Some("│"))
            .thumb_symbol("█");
        let mut scroll_state = ScrollbarState::new(max_scroll_y).position(app.log_ui.diff_scroll_y as usize);
        f.render_stateful_widget(
            scrollbar,
            diff_area.inner(Margin { vertical: 1, horizontal: 0 }),
            &mut scroll_state,
        );
    }

    zones.push(ClickZone {
        rect: diff_area,
        action: AppAction::LogFocusDiff,
    });

    if let Some(msg) = app.log_ui.status.as_deref() {
        zones.push(ClickZone {
            rect: diff_area,
            action: AppAction::None,
        });
        let s = format!("Status: {}", msg);
        f.render_widget(
            Paragraph::new(s).style(Style::default().fg(app.palette.btn_bg)),
            Rect::new(
                diff_area.x + 2,
                diff_area.y + 1,
                diff_area.width.saturating_sub(4),
                1,
            ),
        );
    }
}

/// Render unified diff for log view
fn render_log_unified_diff(
    app: &App,
    diff_area: Rect,
    header_lines: &[String],
    diff_only_lines: &[String],
) -> Vec<Line<'static>> {
    let mut out = Vec::new();
    let mut highlighter: Option<Highlighter> = None;

    let content_w = diff_area.width.saturating_sub(2).max(1) as usize;

    // Render commit header as styled text
    for l in header_lines {
        let t = l.as_str();
        // Skip separator line
        if t.starts_with("─") {
            out.push(Line::from(vec![Span::styled(
                "─".repeat(content_w),
                Style::default().fg(app.palette.border_inactive),
            )]));
            continue;
        }
        // Subject line (first non-empty)
        if out.is_empty() && !t.is_empty() {
            out.push(Line::from(vec![Span::styled(
                t.to_string(),
                Style::default()
                    .fg(app.palette.accent_primary)
                    .add_modifier(Modifier::BOLD),
            )]));
            continue;
        }
        // Body/meta lines
        out.push(Line::from(vec![Span::styled(
            t.to_string(),
            Style::default().fg(app.palette.fg),
        )]));
    }
    // Add spacing after header if there was content
    if !header_lines.is_empty() && !diff_only_lines.is_empty() {
        out.push(Line::from(vec![Span::raw("")]));
    }

    let mut first_file = true;
    for l in diff_only_lines {
        let t = l.as_str();

        if app.syntax_highlight {
            if let Some(p) = t.strip_prefix("+++ b/") {
                let ext = std::path::Path::new(p).extension().and_then(|s| s.to_str());
                highlighter = ext.and_then(new_highlighter);
            }
        }

        // Hunk header with spacing
        if t.starts_with("@@") {
            // Add blank line before hunk for visual separation
            out.push(Line::from(vec![Span::raw("")]));
            out.push(Line::from(vec![Span::styled(
                pad_to_width(t.to_string(), content_w),
                Style::default()
                    .fg(app.palette.accent_secondary)
                    .bg(app.palette.diff_hunk_bg)
                    .add_modifier(Modifier::BOLD),
            )]));
            continue;
        }

        // File header with separator - show filename first
        if t.starts_with("diff --git") {
            // Add blank lines before new file (except first)
            if !first_file {
                out.push(Line::from(vec![Span::raw("")]));
                out.push(Line::from(vec![Span::styled(
                    "─".repeat(content_w),
                    Style::default().fg(app.palette.border_inactive),
                )]));
            }
            first_file = false;
            let full_path = t
                .strip_prefix("diff --git a/")
                .and_then(|s| s.split(" b/").next())
                .unwrap_or(t);
            let (dir, filename) = match full_path.rfind('/') {
                Some(i) => (&full_path[..i + 1], &full_path[i + 1..]),
                None => ("", full_path),
            };
            let mut spans = vec![Span::styled(
                format!("📄 {}", filename),
                Style::default()
                    .fg(app.palette.accent_primary)
                    .add_modifier(Modifier::BOLD),
            )];
            if !dir.is_empty() {
                spans.push(Span::styled(
                    format!("  {}", dir),
                    Style::default().fg(app.palette.border_inactive),
                ));
            }
            out.push(Line::from(spans));
            continue;
        }

        // Skip verbose meta lines
        if t.starts_with("index ") || t.starts_with("--- ") || t.starts_with("+++ ") {
            continue;
        }

        if t.starts_with("rename ") {
            out.push(Line::from(vec![Span::styled(
                pad_to_width(t.to_string(), content_w),
                Style::default().fg(app.palette.accent_secondary),
            )]));
            continue;
        }

        let (prefix, code) = t.split_at(t.chars().next().map(|c| c.len_utf8()).unwrap_or(0));
        let (bg, prefix_fg, is_code) = match prefix {
            "+" if !t.starts_with("+++") => {
                (app.palette.diff_add_bg, app.palette.diff_add_fg, true)
            }
            "-" if !t.starts_with("---") => {
                (app.palette.diff_del_bg, app.palette.diff_del_fg, true)
            }
            " " => (app.palette.bg, app.palette.diff_gutter_fg, true),
            _ => (app.palette.bg, app.palette.fg, false),
        };

        let fill = content_w.saturating_sub(display_width(t));

        if is_code {
            if let Some(hl) = highlighter.as_mut() {
                let mut line = hl.highlight_diff_code_with_prefix(
                    prefix,
                    code,
                    Style::default().fg(prefix_fg),
                    bg,
                );
                if fill > 0 {
                    line.spans
                        .push(Span::styled(" ".repeat(fill), Style::default().bg(bg)));
                }
                out.push(line);
            } else {
                // Without syntax highlight, still color the prefix
                let mut spans = vec![
                    Span::styled(prefix.to_string(), Style::default().fg(prefix_fg).bg(bg)),
                    Span::styled(code.to_string(), Style::default().fg(app.palette.fg).bg(bg)),
                ];
                if fill > 0 {
                    spans.push(Span::styled(" ".repeat(fill), Style::default().bg(bg)));
                }
                out.push(Line::from(spans));
            }
        } else {
            out.push(Line::from(vec![Span::styled(
                pad_to_width(t.to_string(), content_w),
                Style::default().fg(app.palette.fg).bg(bg),
            )]));
        }
    }

    out
}

/// Render side-by-side diff for log view
fn render_log_side_by_side_diff(
    app: &App,
    diff_area: Rect,
    header_lines: &[String],
    diff_only_lines: &[String],
) -> Vec<Line<'static>> {
    let rows = build_side_by_side_rows(diff_only_lines);
    let mut out = Vec::new();
    let inner = diff_area.inner(Margin {
        vertical: 1,
        horizontal: 1,
    });
    let total_w = inner.width as usize;
    let sep_style = Style::default()
        .fg(app.palette.border_inactive)
        .bg(app.palette.bg);
    let left_w = total_w.saturating_sub(1) / 2;
    let right_w = total_w.saturating_sub(1) - left_w;

    // If columns are too narrow, show message instead of garbled text
    if left_w < 16 {
        out.push(Line::from(vec![Span::styled(
            "Window too narrow for side-by-side view",
            Style::default().fg(app.palette.accent_secondary),
        )]));
        out.push(Line::from(vec![Span::styled(
            "Press 's' to switch to unified mode, or widen the window",
            Style::default().fg(app.palette.border_inactive),
        )]));
        return out;
    }

    // Render commit header as styled text first
    for l in header_lines {
        let t = l.as_str();
        // Separator line
        if t.starts_with("─") {
            out.push(Line::from(vec![Span::styled(
                "─".repeat(total_w),
                Style::default().fg(app.palette.border_inactive),
            )]));
            continue;
        }
        // Subject line (first non-empty)
        if out.is_empty() && !t.is_empty() {
            out.push(Line::from(vec![Span::styled(
                t.to_string(),
                Style::default()
                    .fg(app.palette.accent_primary)
                    .add_modifier(Modifier::BOLD),
            )]));
            continue;
        }
        // Body/meta lines
        out.push(Line::from(vec![Span::styled(
            t.to_string(),
            Style::default().fg(app.palette.fg),
        )]));
    }
    // Add spacing after header
    if !header_lines.is_empty() && !diff_only_lines.is_empty() {
        out.push(Line::from(vec![Span::raw("")]));
    }

    let wrap_cells = app.wrap_diff;
    let scroll_x = if wrap_cells {
        0
    } else {
        app.log_ui.diff_scroll_x as usize
    };

    let cell_lines = |cell: &git::GitDiffCell, width: usize| -> Vec<String> {
        git::render_side_by_side_cell_lines(cell, width, scroll_x, wrap_cells)
    };

    let empty_left = " ".repeat(left_w);
    let empty_right = " ".repeat(right_w);

    let mut hl_old: Option<Highlighter> = None;
    let mut hl_new: Option<Highlighter> = None;
    let mut first_file = true;

    for r in rows {
        match r {
            GitDiffRow::Meta(t) => {
                if app.syntax_highlight {
                    if let Some(p) = t.strip_prefix("+++ b/") {
                        let ext = std::path::Path::new(p).extension().and_then(|s| s.to_str());
                        hl_old = ext.and_then(new_highlighter);
                        hl_new = ext.and_then(new_highlighter);
                    }
                }

                // Hunk header with spacing
                if t.starts_with("@@") {
                    out.push(Line::from(vec![Span::raw("")]));
                    out.push(Line::from(vec![Span::styled(
                        pad_to_width(t, total_w),
                        Style::default()
                            .fg(app.palette.accent_secondary)
                            .bg(app.palette.diff_hunk_bg)
                            .add_modifier(Modifier::BOLD),
                    )]));
                    continue;
                }

                // File header with separator - show filename first
                if t.starts_with("diff --git") {
                    if !first_file {
                        out.push(Line::from(vec![Span::raw("")]));
                        out.push(Line::from(vec![Span::styled(
                            "─".repeat(total_w),
                            Style::default().fg(app.palette.border_inactive),
                        )]));
                    }
                    first_file = false;
                    let full_path = t
                        .strip_prefix("diff --git a/")
                        .and_then(|s| s.split(" b/").next())
                        .unwrap_or(t.as_str());
                    let (dir, filename) = match full_path.rfind('/') {
                        Some(i) => (&full_path[..i + 1], &full_path[i + 1..]),
                        None => ("", full_path),
                    };
                    let mut spans = vec![Span::styled(
                        format!("📄 {}", filename),
                        Style::default()
                            .fg(app.palette.accent_primary)
                            .add_modifier(Modifier::BOLD),
                    )];
                    if !dir.is_empty() {
                        spans.push(Span::styled(
                            format!("  {}", dir),
                            Style::default().fg(app.palette.border_inactive),
                        ));
                    }
                    out.push(Line::from(spans));
                    continue;
                }

                // Skip verbose meta lines
                if t.starts_with("index ") || t.starts_with("--- ") || t.starts_with("+++ ") {
                    continue;
                }

                // Other meta lines (rename, etc.)
                out.push(Line::from(vec![Span::styled(
                    pad_to_width(t, total_w),
                    Style::default().fg(app.palette.accent_secondary),
                )]));
            }
            GitDiffRow::Split { old, new } => {
                let old_style = match old.kind {
                    GitDiffCellKind::Delete => Style::default()
                        .fg(app.palette.fg)
                        .bg(app.palette.diff_del_bg),
                    GitDiffCellKind::Context => {
                        Style::default().fg(app.palette.fg).bg(app.palette.bg)
                    }
                    GitDiffCellKind::Add => Style::default().fg(app.palette.fg).bg(app.palette.bg),
                    GitDiffCellKind::Empty => Style::default()
                        .fg(app.palette.border_inactive)
                        .bg(app.palette.bg),
                };
                let new_style = match new.kind {
                    GitDiffCellKind::Add => Style::default()
                        .fg(app.palette.fg)
                        .bg(app.palette.diff_add_bg),
                    GitDiffCellKind::Context => {
                        Style::default().fg(app.palette.fg).bg(app.palette.bg)
                    }
                    GitDiffCellKind::Delete => {
                        Style::default().fg(app.palette.fg).bg(app.palette.bg)
                    }
                    GitDiffCellKind::Empty => Style::default()
                        .fg(app.palette.border_inactive)
                        .bg(app.palette.bg),
                };

                let old_lines = cell_lines(&old, left_w);
                let new_lines = cell_lines(&new, right_w);
                let n = old_lines.len().max(new_lines.len());

                for i in 0..n {
                    let old_cell = old_lines
                        .get(i)
                        .cloned()
                        .unwrap_or_else(|| empty_left.clone());
                    let new_cell = new_lines
                        .get(i)
                        .cloned()
                        .unwrap_or_else(|| empty_right.clone());
                    let old_bg = match old.kind {
                        GitDiffCellKind::Delete => app.palette.diff_del_bg,
                        GitDiffCellKind::Context | GitDiffCellKind::Add => app.palette.bg,
                        GitDiffCellKind::Empty => app.palette.bg,
                    };
                    let new_bg = match new.kind {
                        GitDiffCellKind::Add => app.palette.diff_add_bg,
                        GitDiffCellKind::Context | GitDiffCellKind::Delete => app.palette.bg,
                        GitDiffCellKind::Empty => app.palette.bg,
                    };

                    let old_cell = pad_to_width(old_cell, left_w);
                    let new_cell = pad_to_width(new_cell, right_w);

                    let (old_gutter, old_code) = old_cell.split_at(old_cell.len().min(6));
                    let (new_gutter, new_code) = new_cell.split_at(new_cell.len().min(6));

                    let mut spans: Vec<Span> = Vec::new();

                    // Render old gutter with colored line number and marker
                    if old_gutter.len() >= 5 {
                        let (line_num, marker_space) = old_gutter.split_at(4);
                        let (marker, space) = if marker_space.len() >= 2 {
                            marker_space.split_at(1)
                        } else {
                            (marker_space, "")
                        };
                        spans.push(Span::styled(
                            line_num.to_string(),
                            Style::default().fg(app.palette.diff_gutter_fg).bg(old_bg),
                        ));
                        let marker_fg = if marker.trim() == "-" {
                            app.palette.diff_del_fg
                        } else {
                            app.palette.diff_gutter_fg
                        };
                        spans.push(Span::styled(
                            format!("{}{}", marker, space),
                            Style::default().fg(marker_fg).bg(old_bg),
                        ));
                    } else {
                        spans.push(Span::styled(
                            old_gutter.to_string(),
                            Style::default().fg(app.palette.diff_gutter_fg).bg(old_bg),
                        ));
                    }

                    if app.syntax_highlight
                        && old.kind != GitDiffCellKind::Empty
                        && !old_code.trim().is_empty()
                    {
                        if let Some(hl) = hl_old.as_mut() {
                            spans.extend(hl.highlight_line(old_code, old_bg).spans);
                        } else {
                            spans.push(Span::styled(old_code.to_string(), old_style));
                        }
                    } else {
                        spans.push(Span::styled(old_code.to_string(), old_style));
                    }

                    spans.push(Span::styled("│", sep_style));

                    // Render new gutter with colored line number and marker
                    if new_gutter.len() >= 5 {
                        let (line_num, marker_space) = new_gutter.split_at(4);
                        let (marker, space) = if marker_space.len() >= 2 {
                            marker_space.split_at(1)
                        } else {
                            (marker_space, "")
                        };
                        spans.push(Span::styled(
                            line_num.to_string(),
                            Style::default().fg(app.palette.diff_gutter_fg).bg(new_bg),
                        ));
                        let marker_fg = if marker.trim() == "+" {
                            app.palette.diff_add_fg
                        } else {
                            app.palette.diff_gutter_fg
                        };
                        spans.push(Span::styled(
                            format!("{}{}", marker, space),
                            Style::default().fg(marker_fg).bg(new_bg),
                        ));
                    } else {
                        spans.push(Span::styled(
                            new_gutter.to_string(),
                            Style::default().fg(app.palette.diff_gutter_fg).bg(new_bg),
                        ));
                    }

                    if app.syntax_highlight
                        && new.kind != GitDiffCellKind::Empty
                        && !new_code.trim().is_empty()
                    {
                        if let Some(hl) = hl_new.as_mut() {
                            spans.extend(hl.highlight_line(new_code, new_bg).spans);
                        } else {
                            spans.push(Span::styled(new_code.to_string(), new_style));
                        }
                    } else {
                        spans.push(Span::styled(new_code.to_string(), new_style));
                    }

                    out.push(Line::from(spans));
                }
            }
        }
    }

    out
}

// Helper functions for decoration rendering

fn git_decoration_tokens(decoration: &str) -> Vec<String> {
    let deco = decoration.trim();
    if deco.is_empty() {
        return Vec::new();
    }

    let mut text = deco;
    if let Some(stripped) = text.strip_prefix('(').and_then(|s| s.strip_suffix(')')) {
        text = stripped;
    }

    let mut out = Vec::new();
    for token in text.split(", ") {
        let t = token.trim();
        if t.is_empty() {
            continue;
        }

        if let Some(rest) = t.strip_prefix("HEAD -> ") {
            out.push("HEAD".to_string());
            if !rest.trim().is_empty() {
                out.push(rest.trim().to_string());
            }
            continue;
        }

        if let Some(rest) = t.strip_prefix("tag: ") {
            if !rest.trim().is_empty() {
                out.push(format!("tag:{}", rest.trim()));
            }
            continue;
        }

        out.push(t.to_string());
    }

    out
}

fn git_decoration_spans(decoration: &str, palette: theme::Palette) -> Vec<Span<'static>> {
    let tokens = git_decoration_tokens(decoration);
    if tokens.is_empty() {
        return Vec::new();
    }

    let max = 4usize;
    let extra = tokens.len().saturating_sub(max);
    let show = tokens.into_iter().take(max);

    let mut spans = Vec::new();
    for token in show {
        spans.push(Span::raw(" "));

        let (label, style) = if token == "HEAD" {
            (
                token,
                Style::default()
                    .fg(palette.accent_primary)
                    .add_modifier(Modifier::BOLD),
            )
        } else if let Some(rest) = token.strip_prefix("tag:") {
            (
                format!("tag:{}", rest),
                Style::default().fg(palette.accent_secondary),
            )
        } else if token.starts_with("origin/") {
            (token, Style::default().fg(palette.size_color))
        } else {
            (token, Style::default().fg(palette.accent_tertiary))
        };

        spans.push(Span::styled(format!("[{}]", label), style));
    }

    if extra > 0 {
        spans.push(Span::raw(" "));
        spans.push(Span::styled(
            format!("[+{}]", extra),
            Style::default().fg(palette.size_color),
        ));
    }

    spans
}

fn log_history_line(e: &git_ops::CommitEntry, palette: theme::Palette) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = Vec::new();

    // Subject first - most important info
    spans.push(Span::styled(
        e.subject.clone(),
        Style::default().fg(palette.fg),
    ));

    // Decoration (tags/branches) if any
    let dec_spans = git_decoration_spans(e.decoration.as_str(), palette);
    if !dec_spans.is_empty() {
        spans.push(Span::raw(" "));
        spans.extend(dec_spans);
    }

    // Hash at the end, dimmed
    spans.push(Span::raw("  "));
    spans.push(Span::styled(
        e.short.clone(),
        Style::default().fg(palette.size_color),
    ));

    Line::from(spans)
}

fn log_reflog_line(e: &git_ops::ReflogEntry, palette: theme::Palette) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = Vec::new();

    // Subject first
    spans.push(Span::styled(
        e.subject.clone(),
        Style::default().fg(palette.fg),
    ));

    // Decoration if any
    let dec_spans = git_decoration_spans(e.decoration.as_str(), palette);
    if !dec_spans.is_empty() {
        spans.push(Span::raw(" "));
        spans.extend(dec_spans);
    }

    // Selector at the end, dimmed
    spans.push(Span::raw("  "));
    spans.push(Span::styled(
        e.selector.clone(),
        Style::default().fg(palette.size_color),
    ));

    Line::from(spans)
}
