//! Git tab rendering - staged/unstaged changes tree view and diff view

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Margin, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, Borders, List, ListItem, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState,
        Wrap,
    },
};

use crate::git::{
    self, FlatNodeType, GitDiffCellKind, GitDiffMode, GitDiffRow, GitSection,
    build_side_by_side_rows, display_width, pad_to_width,
};
use crate::highlight::{Highlighter, new_highlighter};
use crate::{App, AppAction, ClickZone, DiffRenderCacheKey};

/// Render the Git tab content: tree view on left, diff on right
pub fn render_git_tab(
    app: &mut App,
    f: &mut Frame,
    content_area: Rect,
    zones: &mut Vec<ClickZone>,
) {
    app.ensure_conflicts_loaded();

    let (tree_area, diff_area) = if app.git_zoom_diff {
        let diff_area = content_area;
        app.git_diff_x = diff_area.x;
        (Rect::new(0, 0, 0, 0), diff_area)
    } else {
        let content_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(app.git_left_width), Constraint::Min(0)])
            .split(content_area);

        let tree_area = content_chunks[0];
        let diff_area = content_chunks[1];
        app.git_diff_x = diff_area.x;

        (tree_area, diff_area)
    };

    // Render tree view
    render_tree_view(app, f, tree_area, zones);

    // Determine which view to render on the right
    let in_conflict_view = app.git.selected_tree_entry().is_some_and(|e| e.is_conflict);

    if in_conflict_view {
        render_conflict_view(app, f, diff_area, zones);
    } else if app.git.show_full_file {
        render_full_file_view(app, f, diff_area);
    } else {
        render_diff_view(app, f, diff_area, zones);
    }
}

/// Render the tree view panel (left side)
fn render_tree_view(app: &mut App, f: &mut Frame, tree_area: Rect, zones: &mut Vec<ClickZone>) {
    let (staged, working, untracked, conflicts) = app.git.section_counts();
    let total = staged + working + untracked + conflicts;
    let tree_block = Block::default()
        .borders(Borders::ALL)
        .border_set(ratatui::symbols::border::PLAIN)
        .border_style(Style::default().fg(app.palette.accent_primary))
        .title(format!(" Git ({}) ", total));
    f.render_widget(tree_block.clone(), tree_area);

    let tree_inner = tree_area.inner(Margin {
        vertical: 1,
        horizontal: 1,
    });

    // Build tree items for rendering
    let tree_items: Vec<ListItem> = app
        .git
        .flat_tree
        .iter()
        .map(|item| {
            let indent = "  ".repeat(item.depth);

            match item.node_type {
                FlatNodeType::Section => {
                    // Section header with expand/collapse and count
                    let arrow = if item.expanded { "▾" } else { "▸" };
                    let count = match item.section {
                        GitSection::Staged => staged,
                        GitSection::Working => working,
                        GitSection::Untracked => untracked,
                        GitSection::Conflicts => conflicts,
                    };
                    let label = format!("{}{} {} ({})", indent, arrow, item.name, count);
                    // Conflicts section gets red/warning color
                    let section_color = if item.section == GitSection::Conflicts {
                        app.palette.diff_del_fg
                    } else {
                        app.palette.accent_secondary
                    };
                    ListItem::new(Line::from(vec![Span::styled(
                        label,
                        Style::default()
                            .fg(section_color)
                            .add_modifier(Modifier::BOLD),
                    )]))
                }
                FlatNodeType::Directory => {
                    // Directory with expand/collapse
                    let arrow = if item.expanded { "▾" } else { "▸" };
                    let label = format!("{}{}  {}/", indent, arrow, item.name);
                    ListItem::new(Line::from(vec![Span::styled(
                        label,
                        Style::default().fg(app.palette.dir_color),
                    )]))
                }
                FlatNodeType::File => {
                    // File entry with status
                    if let Some(entry_idx) = item.entry_idx {
                        if let Some(e) = app.git.entries.get(entry_idx) {
                            let is_selected = app.git.selected_paths.contains(&e.path);

                            // Determine status code based on section
                            let status = match item.section {
                                GitSection::Staged => e.x.to_string(),
                                GitSection::Working => e.y.to_string(),
                                GitSection::Untracked => "?".to_string(),
                                GitSection::Conflicts => format!("{}{}", e.x, e.y),
                            };

                            // Conflict files get red styling
                            let status_style = if item.section == GitSection::Conflicts {
                                Style::default().fg(app.palette.diff_del_fg)
                            } else {
                                match status.chars().next().unwrap_or(' ') {
                                    'M' => Style::default().fg(app.palette.accent_secondary),
                                    'A' => Style::default().fg(app.palette.exe_color),
                                    'D' => Style::default().fg(app.palette.btn_bg),
                                    '?' => Style::default().fg(app.palette.accent_tertiary),
                                    'U' => Style::default().fg(app.palette.btn_bg),
                                    _ => Style::default().fg(app.palette.fg),
                                }
                            };

                            let checkbox = if is_selected { "▣" } else { "□" };

                            let mut spans = vec![
                                Span::raw(indent.clone()),
                                Span::styled(
                                    format!("{} ", checkbox),
                                    Style::default().fg(app.palette.border_inactive),
                                ),
                                Span::styled(format!("{} ", status), status_style),
                                Span::styled(&item.name, Style::default().fg(app.palette.fg)),
                            ];

                            if let Some(from) = &e.renamed_from {
                                let base = from.rsplit('/').next().unwrap_or(from);
                                spans.push(Span::styled(
                                    format!(" <- {}", base),
                                    Style::default().fg(app.palette.border_inactive),
                                ));
                            }

                            let mut list_item = ListItem::new(Line::from(spans));
                            if is_selected {
                                list_item =
                                    list_item.style(Style::default().bg(app.palette.menu_bg));
                            }
                            return list_item;
                        }
                    }
                    // Fallback
                    ListItem::new(Line::from(vec![Span::raw(format!(
                        "{}  {}",
                        indent, item.name
                    ))]))
                }
            }
        })
        .collect();

    let tree_list = List::new(tree_items)
        .highlight_style(
            Style::default()
                .bg(app.palette.selection_bg)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▎");

    f.render_stateful_widget(tree_list, tree_inner, &mut app.git.tree_state.clone());

    // Add click zones for tree items
    let start_index = app.git.tree_state.offset();
    let end_index = (start_index + tree_inner.height as usize).min(app.git.flat_tree.len());
    for (i, idx) in (start_index..end_index).enumerate() {
        let rect = Rect::new(tree_inner.x, tree_inner.y + i as u16, tree_inner.width, 1);
        zones.push(ClickZone {
            rect,
            action: AppAction::SelectGitTreeItem(idx),
        });
    }

    // Scrollbar for tree
    if app.git.flat_tree.len() > tree_inner.height as usize {
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(Some("▴"))
            .end_symbol(Some("▾"))
            .track_symbol(Some("│"))
            .thumb_symbol("║");
        let mut scroll_state = ScrollbarState::new(app.git.flat_tree.len())
            .position(app.git.tree_state.selected().unwrap_or(0));
        f.render_stateful_widget(
            scrollbar,
            tree_area.inner(Margin {
                vertical: 1,
                horizontal: 0,
            }),
            &mut scroll_state,
        );
    }
}

/// Render the conflict resolution view
fn render_conflict_view(app: &mut App, f: &mut Frame, diff_area: Rect, zones: &mut Vec<ClickZone>) {
    let title = app
        .conflict_ui
        .path
        .as_deref()
        .map(|p| format!(" Conflicts: {} ", p))
        .unwrap_or_else(|| " Conflicts ".to_string());

    let block = Block::default()
        .borders(Borders::ALL)
        .border_set(ratatui::symbols::border::PLAIN)
        .border_style(Style::default().fg(app.palette.border_inactive))
        .title(title);
    f.render_widget(block.clone(), diff_area);

    let inner = diff_area.inner(Margin {
        vertical: 1,
        horizontal: 1,
    });

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(inner);

    let sep_style = Style::default().fg(app.palette.border_inactive);
    let ours_header_style = Style::default()
        .fg(app.palette.diff_add_fg)
        .bg(app.palette.diff_add_bg)
        .add_modifier(Modifier::BOLD);
    let theirs_header_style = Style::default()
        .fg(app.palette.accent_primary)
        .bg(app.palette.diff_hunk_bg)
        .add_modifier(Modifier::BOLD);

    let inner_w = rows[0].width as usize;
    let sep_w = 1usize;
    let left_w = inner_w.saturating_sub(sep_w) / 2;
    let right_w = inner_w.saturating_sub(sep_w).saturating_sub(left_w);

    let (count, ours_title, theirs_title) = if let Some(file) = &app.conflict_ui.file {
        let n = file.blocks.len();
        let cur = app.conflict_ui.selected_block + 1;
        (
            n,
            format!(" ◀ Ours ({}/{}) ", cur.min(n.max(1)), n),
            " Theirs ▶ ".to_string(),
        )
    } else {
        (0, " ◀ Ours ".to_string(), " Theirs ▶ ".to_string())
    };

    let header = Line::from(vec![
        Span::styled(pad_to_width(ours_title, left_w), ours_header_style),
        Span::styled("│", sep_style),
        Span::styled(pad_to_width(theirs_title, right_w), theirs_header_style),
    ]);
    f.render_widget(Paragraph::new(header), rows[0]);

    let mut content_lines: Vec<Line> = Vec::new();
    if let Some(file) = &app.conflict_ui.file {
        if file.blocks.is_empty() {
            content_lines.push(Line::raw("No conflict markers found"));
        } else {
            let idx = app.conflict_ui.selected_block.min(file.blocks.len() - 1);
            let block = &file.blocks[idx];
            let n = block.ours.len().max(block.theirs.len());

            let gutter_style = Style::default().fg(app.palette.diff_gutter_fg);
            let ours_style = Style::default()
                .fg(app.palette.diff_add_fg)
                .bg(app.palette.diff_add_bg);
            let theirs_style = Style::default()
                .fg(app.palette.accent_primary)
                .bg(app.palette.diff_hunk_bg);
            let empty_ours_style = Style::default().bg(app.palette.diff_add_bg);
            let empty_theirs_style = Style::default().bg(app.palette.diff_hunk_bg);

            let gutter_w = 4usize;
            let content_left_w = left_w.saturating_sub(gutter_w);
            let content_right_w = right_w.saturating_sub(gutter_w);

            for i in 0..n {
                let has_left = i < block.ours.len();
                let has_right = i < block.theirs.len();
                let left = block.ours.get(i).cloned().unwrap_or_default();
                let right = block.theirs.get(i).cloned().unwrap_or_default();

                let left_ln = if has_left {
                    format!("{:>3} ", i + 1)
                } else {
                    "    ".to_string()
                };
                let right_ln = if has_right {
                    format!("{:>3} ", i + 1)
                } else {
                    "    ".to_string()
                };

                let left = pad_to_width(
                    git::slice_chars(&left, app.git.diff_scroll_x as usize, content_left_w),
                    content_left_w,
                );
                let right = pad_to_width(
                    git::slice_chars(&right, app.git.diff_scroll_x as usize, content_right_w),
                    content_right_w,
                );

                let left_style = if has_left {
                    ours_style
                } else {
                    empty_ours_style
                };
                let right_style = if has_right {
                    theirs_style
                } else {
                    empty_theirs_style
                };

                content_lines.push(Line::from(vec![
                    Span::styled(left_ln, gutter_style),
                    Span::styled(left, left_style),
                    Span::styled("│", sep_style),
                    Span::styled(right_ln, gutter_style),
                    Span::styled(right, right_style),
                ]));
            }
        }
    } else {
        content_lines.push(Line::raw("Failed to load conflict file"));
    }

    let para = Paragraph::new(content_lines)
        .scroll((app.conflict_ui.scroll_y, 0))
        .wrap(Wrap { trim: false });
    f.render_widget(para, rows[1]);

    zones.push(ClickZone {
        rect: rows[1],
        action: AppAction::None,
    });

    let enabled = !app.commit.busy && app.pending_job.is_none();
    let mut x = rows[2].x;
    for (label, action, color) in [
        (
            " < Prev ",
            AppAction::ConflictPrev,
            app.palette.accent_tertiary,
        ),
        (
            " Next > ",
            AppAction::ConflictNext,
            app.palette.accent_tertiary,
        ),
        (
            " Ours ",
            AppAction::ConflictUseOurs,
            app.palette.accent_primary,
        ),
        (
            " Theirs ",
            AppAction::ConflictUseTheirs,
            app.palette.accent_secondary,
        ),
        (
            " Both ",
            AppAction::ConflictUseBoth,
            app.palette.accent_tertiary,
        ),
        (
            " Mark Resolved ",
            AppAction::MarkResolved,
            app.palette.exe_color,
        ),
    ] {
        let w = label.len() as u16;
        if x + w > rows[2].x + rows[2].width {
            break;
        }
        let bg = if enabled {
            color
        } else {
            app.palette.border_inactive
        };
        let fg = if enabled {
            app.palette.btn_fg
        } else {
            app.palette.fg
        };
        let style = Style::default().bg(bg).fg(fg).add_modifier(Modifier::BOLD);
        let rect = Rect::new(x, rows[2].y, w, 1);
        f.render_widget(Paragraph::new(label).style(style), rect);
        if enabled {
            zones.push(ClickZone { rect, action });
        }
        x += w + 1;
    }

    if count == 0 {
        let msg = "No conflicts";
        let w = msg.len().min(rows[2].width as usize) as u16;
        f.render_widget(
            Paragraph::new(msg).style(Style::default().fg(app.palette.border_inactive)),
            Rect::new(rows[2].x, rows[2].y, w, 1),
        );
    }
}

/// Render the full file view (when F key is pressed)
fn render_full_file_view(app: &mut App, f: &mut Frame, diff_area: Rect) {
    let file_name = app
        .git
        .selected_tree_entry()
        .map(|e| e.path.as_str())
        .unwrap_or("File");
    let diff_block = Block::default()
        .borders(Borders::ALL)
        .border_set(ratatui::symbols::border::PLAIN)
        .border_style(Style::default().fg(app.palette.border_inactive))
        .title(format!(" {} (F=diff) ", file_name));

    let content = app.git.full_file_content.as_deref().unwrap_or("No content");

    // Simple line rendering without syntax highlight for performance
    let lines: Vec<Line> = content.lines().map(Line::raw).collect();

    let lines_len = lines.len();
    let viewport_h = diff_area.height.saturating_sub(2) as usize;
    let max_scroll = lines_len.saturating_sub(viewport_h);
    let scroll_y = (app.git.full_file_scroll_y as usize).min(max_scroll);

    let para = Paragraph::new(lines)
        .block(diff_block)
        .scroll((scroll_y as u16, 0));
    f.render_widget(para, diff_area);

    // Scrollbar
    if lines_len > viewport_h {
        let sb_area = Rect::new(
            diff_area.x + diff_area.width.saturating_sub(1),
            diff_area.y + 1,
            1,
            diff_area.height.saturating_sub(2),
        );
        let mut sb_state = ScrollbarState::new(lines_len)
            .position(scroll_y)
            .viewport_content_length(viewport_h);
        f.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .begin_symbol(None)
                .end_symbol(None)
                .track_symbol(Some("│"))
                .thumb_symbol("█"),
            sb_area,
            &mut sb_state,
        );
    }
}

/// Render the diff view (unified or side-by-side)
fn render_diff_view(app: &mut App, f: &mut Frame, diff_area: Rect, zones: &mut Vec<ClickZone>) {
    let mode_label = match app.git.diff_mode {
        GitDiffMode::SideBySide => "SxS",
        GitDiffMode::Unified => "Unified",
    };
    let diff_block = Block::default()
        .borders(Borders::ALL)
        .border_set(ratatui::symbols::border::PLAIN)
        .border_style(Style::default().fg(app.palette.border_inactive))
        .title(format!(" Diff ({}) ", mode_label));

    let cache_width = diff_area.width.saturating_sub(2).max(1);
    let cache_scroll_x = if app.git.diff_mode == GitDiffMode::SideBySide && !app.wrap_diff {
        app.git.diff_scroll_x
    } else {
        0
    };
    let cache_key = DiffRenderCacheKey {
        theme: app.theme,
        generation: app.git.diff_generation,
        mode: app.git.diff_mode,
        width: cache_width,
        wrap: app.wrap_diff,
        syntax_highlight: app.syntax_highlight,
        scroll_x: cache_scroll_x,
    };

    let diff_lines: Vec<Line> = if app.git_diff_cache.key == Some(cache_key) {
        app.git_diff_cache.lines.clone()
    } else {
        let computed: Vec<Line> = if app.git.repo_root.is_none() {
            vec![Line::raw("Not a git repository")]
        } else if app.git.diff_lines.is_empty() {
            vec![Line::raw("No selection")]
        } else {
            match app.git.diff_mode {
                GitDiffMode::Unified => render_unified_diff(app, diff_area),
                GitDiffMode::SideBySide => render_side_by_side_diff(app, diff_area),
            }
        };
        app.git_diff_cache.key = Some(cache_key);
        app.git_diff_cache.lines = computed.clone();
        computed
    };

    let wrap_unified = app.git.diff_mode == GitDiffMode::Unified && app.wrap_diff;

    let viewport_h = diff_area.height.saturating_sub(2) as usize;
    let total_lines = diff_lines.len();
    let max_y = if viewport_h == 0 {
        0
    } else if wrap_unified {
        app.git
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
    // Clamp to u16::MAX to avoid overflow, then clamp to max_y
    let max_y_u16 = max_y.min(u16::MAX as usize) as u16;
    app.git.diff_scroll_y = app.git.diff_scroll_y.min(max_y_u16);

    let x_scroll = if app.git.diff_mode == GitDiffMode::Unified && !wrap_unified {
        app.git.diff_scroll_x
    } else {
        0
    };
    let mut diff_para = Paragraph::new(diff_lines)
        .block(diff_block)
        .scroll((app.git.diff_scroll_y, x_scroll));
    if wrap_unified {
        diff_para = diff_para.wrap(Wrap { trim: false });
    }

    f.render_widget(diff_para, diff_area);

    // Scrollbar for diff
    let total_lines = if wrap_unified {
        app.git
            .diff_lines
            .iter()
            .map(|l| {
                let w = (diff_area.width.saturating_sub(2).max(1)) as usize;
                let cols = display_width(l).max(1);
                (cols + w - 1) / w
            })
            .sum::<usize>()
    } else {
        app.git_diff_cache.lines.len()
    };
    // Scrollbar - use max_y as range so thumb reaches bottom when content ends
    let max_scroll_y = total_lines.saturating_sub(viewport_h).max(1);
    if total_lines > viewport_h {
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(Some("▴"))
            .end_symbol(Some("▾"))
            .track_symbol(Some("│"))
            .thumb_symbol("█");
        let mut scroll_state = ScrollbarState::new(max_scroll_y).position(app.git.diff_scroll_y as usize);
        f.render_stateful_widget(
            scrollbar,
            diff_area.inner(Margin { vertical: 1, horizontal: 0 }),
            &mut scroll_state,
        );
    }

    // Render revert buttons for visible changes
    render_revert_buttons(app, f, diff_area, zones);
}

/// Render unified diff lines
fn render_unified_diff(app: &App, diff_area: Rect) -> Vec<Line<'static>> {
    let ext = app
        .git
        .selected_tree_entry()
        .and_then(|e| std::path::Path::new(e.path.as_str()).extension())
        .and_then(|s| s.to_str());

    let mut highlighter: Option<Highlighter> = if app.syntax_highlight {
        ext.and_then(new_highlighter)
    } else {
        None
    };

    let content_w = diff_area.width.saturating_sub(2).max(1) as usize;

    let mut out = Vec::new();
    for l in &app.git.diff_lines {
        let t = l.as_str();
        if t.starts_with("@@") {
            out.push(Line::from(vec![Span::styled(
                pad_to_width(t.to_string(), content_w),
                Style::default()
                    .fg(app.palette.fg)
                    .bg(app.palette.diff_hunk_bg)
                    .add_modifier(Modifier::BOLD),
            )]));
            continue;
        }

        if t.starts_with("diff --git") {
            // Extract clean path from "diff --git a/path b/path"
            let full_path = t
                .strip_prefix("diff --git a/")
                .and_then(|s| s.split(" b/").next())
                .unwrap_or(t);

            // Update highlighter for this file's extension
            if app.syntax_highlight {
                let ext = std::path::Path::new(full_path)
                    .extension()
                    .and_then(|s| s.to_str());
                highlighter = ext.and_then(new_highlighter);
            }

            // Show filename first, then directory
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

        // Skip verbose meta lines (index, ---, +++)
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
        let (bg, is_code) = match prefix {
            "+" if !t.starts_with("+++") => (app.palette.diff_add_bg, true),
            "-" if !t.starts_with("---") => (app.palette.diff_del_bg, true),
            " " => (app.palette.bg, true),
            _ => (app.palette.bg, false),
        };

        let fill = content_w.saturating_sub(display_width(t));

        if is_code {
            if let Some(hl) = highlighter.as_mut() {
                let mut line = hl.highlight_diff_code_with_prefix(
                    prefix,
                    code,
                    Style::default().fg(app.palette.fg),
                    bg,
                );
                if fill > 0 {
                    line.spans
                        .push(Span::styled(" ".repeat(fill), Style::default().bg(bg)));
                }
                out.push(line);
            } else {
                out.push(Line::from(vec![Span::styled(
                    pad_to_width(t.to_string(), content_w),
                    Style::default().fg(app.palette.fg).bg(bg),
                )]));
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

/// Render side-by-side diff lines
fn render_side_by_side_diff(app: &App, diff_area: Rect) -> Vec<Line<'static>> {
    let inner_w = diff_area.width.saturating_sub(2) as usize;
    let sep_w = 1usize;
    let left_w = inner_w.saturating_sub(sep_w) / 2;
    let right_w = inner_w.saturating_sub(sep_w).saturating_sub(left_w);

    let mut out = Vec::new();

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

    let title_style = Style::default()
        .fg(app.palette.fg)
        .add_modifier(Modifier::BOLD);
    let sep_style = Style::default().fg(app.palette.border_inactive);

    let left_title = pad_to_width(" Old ".to_string(), left_w);
    let right_title = pad_to_width(" New ".to_string(), right_w);
    out.push(Line::from(vec![
        Span::styled(left_title, title_style),
        Span::styled("│", sep_style),
        Span::styled(right_title, title_style),
    ]));

    let wrap_cells = app.wrap_diff;
    let scroll_x = if wrap_cells {
        0
    } else {
        app.git.diff_scroll_x as usize
    };

    let cell_lines = |cell: &git::GitDiffCell, width: usize| -> Vec<String> {
        git::render_side_by_side_cell_lines(cell, width, scroll_x, wrap_cells)
    };

    let empty_left = " ".repeat(left_w);
    let empty_right = " ".repeat(right_w);

    let mut hl_old: Option<Highlighter> = None;
    let mut hl_new: Option<Highlighter> = None;
    if app.syntax_highlight {
        let ext = app
            .git
            .selected_tree_entry()
            .and_then(|e| std::path::Path::new(e.path.as_str()).extension())
            .and_then(|s| s.to_str());
        hl_old = ext.and_then(new_highlighter);
        hl_new = ext.and_then(new_highlighter);
    }

    let rows = build_side_by_side_rows(&app.git.diff_lines);
    let mut first_file = true;
    for row in rows {
        match row {
            GitDiffRow::Meta(t) => {
                // Hunk header with spacing
                if t.starts_with("@@") {
                    out.push(Line::from(vec![Span::raw("")]));
                    out.push(Line::from(vec![Span::styled(
                        pad_to_width(t, inner_w),
                        Style::default()
                            .fg(app.palette.accent_secondary)
                            .bg(app.palette.diff_hunk_bg)
                            .add_modifier(Modifier::BOLD),
                    )]));
                    continue;
                }

                // File header - show filename first, then directory
                if t.starts_with("diff --git") {
                    if !first_file {
                        out.push(Line::from(vec![Span::raw("")]));
                        out.push(Line::from(vec![Span::styled(
                            "─".repeat(inner_w),
                            Style::default().fg(app.palette.border_inactive),
                        )]));
                    }
                    first_file = false;
                    let full_path = t
                        .strip_prefix("diff --git a/")
                        .and_then(|s| s.split(" b/").next())
                        .unwrap_or(t.as_str());

                    // Update highlighters for this file's extension
                    if app.syntax_highlight {
                        let ext = std::path::Path::new(full_path)
                            .extension()
                            .and_then(|s| s.to_str());
                        hl_old = ext.and_then(new_highlighter);
                        hl_new = ext.and_then(new_highlighter);
                    }

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
                    pad_to_width(t, inner_w),
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
                        // Line number in gray
                        spans.push(Span::styled(
                            line_num.to_string(),
                            Style::default().fg(app.palette.diff_gutter_fg).bg(old_bg),
                        ));
                        // Marker (-) in diff color
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
                        // Line number in gray
                        spans.push(Span::styled(
                            line_num.to_string(),
                            Style::default().fg(app.palette.diff_gutter_fg).bg(new_bg),
                        ));
                        // Marker (+) in diff color
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

/// Render revert buttons for visible changes
fn render_revert_buttons(app: &App, f: &mut Frame, diff_area: Rect, zones: &mut Vec<ClickZone>) {
    let diff_inner = diff_area.inner(Margin {
        vertical: 1,
        horizontal: 1,
    });
    let scroll_y = app.git.diff_scroll_y as usize;
    let viewport_h = diff_inner.height as usize;

    if app.git.diff_mode == GitDiffMode::SideBySide {
        // In side-by-side mode, show buttons at each change block (middle gutter)
        // Must match the layout in content rendering: left_w = (inner_w - sep_w) / 2
        let inner_w = diff_area.width.saturating_sub(2) as usize;
        let sep_w = 1usize;
        let left_w = inner_w.saturating_sub(sep_w) / 2;
        let btn_x = diff_area.x + 1 + left_w as u16; // Middle gutter position (on the | separator)

        for (block_idx, block) in app.git.change_blocks.iter().enumerate() {
            if block.display_row >= scroll_y && block.display_row < scroll_y + viewport_h {
                let screen_y = diff_inner.y + (block.display_row - scroll_y) as u16;
                let btn_rect = Rect::new(btn_x, screen_y, 1, 1);

                // Draw the revert button (arrow in middle gutter)
                let btn_style = Style::default()
                    .fg(app.palette.accent_secondary)
                    .add_modifier(Modifier::BOLD);
                f.render_widget(Paragraph::new("→").style(btn_style), btn_rect);

                // Register click zone (slightly wider for easier clicking)
                let click_rect = Rect::new(btn_x.saturating_sub(1), screen_y, 3, 1);
                zones.push(ClickZone {
                    rect: click_rect,
                    action: AppAction::RevertBlock(block_idx),
                });
            }
        }
    } else {
        // In unified mode, show buttons at hunk headers
        let btn_x = diff_area.x + diff_area.width.saturating_sub(4);

        for (hunk_idx, hunk) in app.git.diff_hunks.iter().enumerate() {
            if hunk.display_row >= scroll_y && hunk.display_row < scroll_y + viewport_h {
                let screen_y = diff_inner.y + (hunk.display_row - scroll_y) as u16;
                let btn_rect = Rect::new(btn_x, screen_y, 3, 1);

                let btn_style = Style::default()
                    .fg(app.palette.accent_secondary)
                    .bg(app.palette.diff_hunk_bg)
                    .add_modifier(Modifier::BOLD);
                f.render_widget(Paragraph::new(" ↩ ").style(btn_style), btn_rect);

                zones.push(ClickZone {
                    rect: btn_rect,
                    action: AppAction::RevertHunk(hunk_idx),
                });
            }
        }
    }
}
