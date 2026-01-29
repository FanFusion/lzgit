//! Explorer tab rendering - Miller columns layout with zoom modes.

use ratatui::{
    prelude::*,
    widgets::{
        Block, Borders, List, ListItem, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState,
        Wrap,
    },
};
use ratatui_image::StatefulImage;
use std::fs;

use crate::{App, AppAction, ClickZone, ExplorerZoom, format_size, highlight};

/// Render the Explorer tab with configurable layout (z to cycle).
pub fn render_explorer_tab(
    app: &mut App,
    f: &mut Frame,
    content_area: Rect,
    click_zones: &mut Vec<ClickZone>,
) {
    match app.explorer_zoom {
        ExplorerZoom::ThreeColumn => {
            // Miller columns: Parent (25%) | Current (35%) | Preview (40%)
            let chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Percentage(25),
                    Constraint::Percentage(35),
                    Constraint::Percentage(40),
                ])
                .split(content_area);

            app.explorer_parent_x = chunks[0].x;
            app.explorer_current_x = chunks[1].x;
            app.explorer_preview_x = chunks[2].x;

            render_parent_pane(app, f, chunks[0], click_zones);
            render_file_list(app, f, chunks[1], click_zones);
            render_preview(app, f, chunks[2], click_zones);
        }
        ExplorerZoom::TwoColumn => {
            // Two columns: Current (40%) | Preview (60%)
            let chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Percentage(40),
                    Constraint::Percentage(60),
                ])
                .split(content_area);

            app.explorer_parent_x = 0;
            app.explorer_current_x = chunks[0].x;
            app.explorer_preview_x = chunks[1].x;

            render_file_list(app, f, chunks[0], click_zones);
            render_preview(app, f, chunks[1], click_zones);
        }
        ExplorerZoom::PreviewOnly => {
            // Full preview
            app.explorer_parent_x = 0;
            app.explorer_current_x = 0;
            app.explorer_preview_x = content_area.x;

            render_preview(app, f, content_area, click_zones);
        }
    }
}

/// Render the parent directory pane (left column).
fn render_parent_pane(app: &mut App, f: &mut Frame, area: Rect, click_zones: &mut Vec<ClickZone>) {
    let parent_path = app.current_path.parent();

    let title = if let Some(p) = parent_path {
        let name = p.file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "/".to_string());
        format!(" {} ", name)
    } else {
        " / ".to_string()
    };

    let parent_block = Block::default()
        .borders(Borders::ALL)
        .border_set(ratatui::symbols::border::PLAIN)
        .border_style(Style::default().fg(app.palette.border_inactive))
        .title(title);

    // Get parent directory entries
    let parent_entries: Vec<(String, bool, bool)> = if let Some(parent) = parent_path {
        fs::read_dir(parent)
            .ok()
            .map(|entries| {
                let mut items: Vec<_> = entries
                    .filter_map(|e| e.ok())
                    .map(|e| {
                        let name = e.file_name().to_string_lossy().to_string();
                        let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
                        let is_current = e.path() == app.current_path;
                        (name, is_dir, is_current)
                    })
                    .collect();
                items.sort_by(|a, b| {
                    match (a.1, b.1) {
                        (true, false) => std::cmp::Ordering::Less,
                        (false, true) => std::cmp::Ordering::Greater,
                        _ => a.0.to_lowercase().cmp(&b.0.to_lowercase()),
                    }
                });
                items
            })
            .unwrap_or_default()
    } else {
        vec![]
    };

    let items: Vec<ListItem> = parent_entries
        .iter()
        .map(|(name, is_dir, is_current)| {
            let icon = if *is_dir { "" } else { "󰈙" };
            let color = if *is_dir {
                app.palette.dir_color
            } else {
                app.palette.fg
            };

            let (text_style, item_style) = if *is_current {
                // Highlight current directory with background like Yazi
                (
                    Style::default().fg(app.palette.accent_primary).add_modifier(Modifier::BOLD),
                    Style::default().bg(app.palette.selection_bg),
                )
            } else {
                (Style::default().fg(color), Style::default())
            };

            ListItem::new(Line::from(vec![
                Span::styled(format!("{} ", icon), text_style),
                Span::styled(name.clone(), text_style),
            ])).style(item_style)
        })
        .collect();

    let list = List::new(items).block(parent_block);
    f.render_widget(list, area);

    // Add click zones for parent directory items
    let inner = area.inner(Margin { vertical: 1, horizontal: 1 });
    if let Some(parent) = parent_path {
        for (i, (name, is_dir, _)) in parent_entries.iter().enumerate() {
            if i >= inner.height as usize {
                break;
            }
            let entry_path = parent.join(name);
            let rect = Rect::new(inner.x, inner.y + i as u16, inner.width, 1);

            if *is_dir {
                click_zones.push(ClickZone {
                    rect,
                    action: AppAction::Navigate(entry_path),
                });
            }
        }
    }
}

/// Render the file/folder list with icons.
fn render_file_list(app: &mut App, f: &mut Frame, area: Rect, click_zones: &mut Vec<ClickZone>) {
    let list_block = Block::default()
        .borders(Borders::ALL)
        .border_set(ratatui::symbols::border::PLAIN)
        .border_style(Style::default().fg(app.palette.accent_primary))
        .title(format!(" Files ({}) ", app.files.len()));

    let items: Vec<ListItem> = app
        .files
        .iter()
        .map(|file| {
            // File type icons and colors (like Yazi)
            let ext = file.path.extension()
                .and_then(|e| e.to_str())
                .unwrap_or("");

            let (icon, color) = if file.is_dir {
                ("", app.palette.dir_color)
            } else if file.is_symlink {
                ("", Color::Cyan)
            } else if file.is_exec {
                ("󰆍", app.palette.exe_color)
            } else {
                match ext {
                    // Rust
                    "rs" => ("", Color::Rgb(255, 140, 90)),  // Orange
                    // Python
                    "py" => ("", Color::Rgb(80, 200, 120)),  // Green
                    // JavaScript/TypeScript
                    "js" | "jsx" => ("", Color::Rgb(240, 220, 80)),  // Yellow
                    "ts" | "tsx" => ("", Color::Rgb(80, 160, 240)),  // Blue
                    // Web
                    "html" | "htm" => ("", Color::Rgb(230, 120, 80)),  // Orange
                    "css" | "scss" | "sass" => ("", Color::Rgb(80, 160, 240)),  // Blue
                    // Config
                    "json" => ("", Color::Rgb(200, 180, 100)),  // Yellow
                    "toml" | "yaml" | "yml" => ("", Color::Rgb(180, 140, 200)),  // Purple
                    // Docs
                    "md" | "txt" => ("", Color::Rgb(180, 180, 180)),  // Gray
                    // Shell
                    "sh" | "bash" | "zsh" => ("", Color::Rgb(80, 200, 120)),  // Green
                    // Images
                    "png" | "jpg" | "jpeg" | "gif" | "svg" | "webp" => ("", Color::Rgb(200, 120, 200)),
                    // Archives
                    "zip" | "tar" | "gz" | "7z" | "rar" => ("", Color::Rgb(200, 160, 80)),
                    // Default
                    _ => ("󰈙", app.palette.fg),
                }
            };

            let name_span = Span::styled(&file.name, Style::default().fg(color));
            let icon_span = Span::styled(format!("{} ", icon), Style::default().fg(color));
            let mut spans = vec![icon_span, name_span];

            if !file.is_dir {
                spans.push(Span::styled(
                    format!(" ({})", format_size(file.size)),
                    Style::default().fg(app.palette.size_color),
                ));
            }

            ListItem::new(Line::from(spans))
        })
        .collect();

    let list = List::new(items)
        .block(list_block)
        .highlight_style(
            Style::default()
                .bg(app.palette.selection_bg)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▎ ");

    f.render_stateful_widget(list, area, &mut app.list_state.clone());

    let list_inner = area.inner(Margin {
        vertical: 1,
        horizontal: 1,
    });
    let start_index = app.list_state.offset();
    let end_index = (start_index + list_inner.height as usize).min(app.files.len());

    for (i, idx) in (start_index..end_index).enumerate() {
        let rect = Rect::new(list_inner.x, list_inner.y + i as u16, list_inner.width, 1);
        click_zones.push(ClickZone {
            rect,
            action: AppAction::Select(idx),
        });
    }

    // Scrollbar
    let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
        .begin_symbol(Some("▴"))
        .end_symbol(Some("▾"))
        .track_symbol(Some("│"))
        .thumb_symbol("║");
    let mut scroll_state =
        ScrollbarState::new(app.files.len()).position(app.selected_index().unwrap_or(0));
    f.render_stateful_widget(
        scrollbar,
        area.inner(Margin {
            vertical: 1,
            horizontal: 0,
        }),
        &mut scroll_state,
    );
}

/// Render the preview panel (syntax highlighted text, image, or directory listing).
fn render_preview(app: &mut App, f: &mut Frame, area: Rect, click_zones: &mut Vec<ClickZone>) {
    app.explorer_preview_x = area.x;

    if let Some(state) = &mut app.image_state {
        let image = StatefulImage::new();
        f.render_stateful_widget(image, area, state);
    } else {
        let preview_text = if let Some(err) = &app.preview_error {
            err.clone()
        } else if app.preview_loading {
            "Loading...".to_string()
        } else if let Some(file) = app.selected_file() {
            if file.is_dir {
                // Directories are not async-loaded - just list entries
                if let Ok(entries) = fs::read_dir(&file.path) {
                    entries
                        .take(20)
                        .map(|e| {
                            e.ok()
                                .map(|x| x.file_name().to_string_lossy().into_owned())
                                .unwrap_or_default()
                        })
                        .collect::<Vec<_>>()
                        .join("\n")
                } else {
                    String::new()
                }
            } else {
                // Use async-loaded preview content
                app.preview_content.clone().unwrap_or_default()
            }
        } else {
            String::new()
        };

        // Use preview text directly - no truncation for infinite scroll
        let preview_limited = preview_text;

        // Extract file info first to avoid borrow issues
        let file_ext = app
            .selected_file()
            .filter(|f| !f.is_dir)
            .and_then(|f| f.path.extension())
            .and_then(|s| s.to_str())
            .map(|s| s.to_string());

        // Syntax highlighting with increased limit for larger files
        let total_lines = preview_limited.lines().count();
        let line_num_width = total_lines.to_string().len().max(3); // At least 3 chars for line numbers

        let base_lines: Vec<Line> = if app.syntax_highlight && preview_limited.len() < 500_000 {
            if let Some(ext) = file_ext {
                // Check if cache needs to be initialized or updated
                let cache_needs_update = app.highlight_cache.as_ref().map_or(true, |cache| {
                    // Update if content changed or extension changed
                    cache.line_count() != total_lines
                });

                if cache_needs_update {
                    // Initialize/update cache with new content
                    app.highlight_cache = Some(highlight::HighlightCache::new(
                        preview_limited.clone(),
                        ext.clone(),
                    ));
                }

                // Get all highlighted lines (cache handles efficiency internally)
                if let Some(cache) = &mut app.highlight_cache {
                    cache.get_highlighted_range(0, total_lines, app.palette.bg)
                } else {
                    // Fallback to non-highlighted
                    preview_limited.lines().map(Line::raw).collect()
                }
            } else {
                // No extension or directory
                preview_limited.lines().map(Line::raw).collect()
            }
        } else {
            // No highlighting
            preview_limited.lines().map(Line::raw).collect()
        };

        // Add line numbers to each line (like Yazi/bat)
        let line_num_style = Style::default().fg(app.palette.line_num_color);
        let lines: Vec<Line> = base_lines
            .into_iter()
            .enumerate()
            .map(|(i, mut line)| {
                let num = format!("{:>width$} │ ", i + 1, width = line_num_width);
                let mut new_spans = vec![Span::styled(num, line_num_style)];
                new_spans.extend(line.spans.drain(..));
                Line::from(new_spans)
            })
            .collect();

        // Calculate total lines for display and scroll clamping
        let line_count = lines.len();

        let title = if app.preview_loading {
            " Preview (loading...) ".to_string()
        } else {
            format!(" Preview ({} lines) ", line_count)
        };

        let p_block = Block::default()
            .borders(Borders::ALL)
            .border_set(ratatui::symbols::border::PLAIN)
            .border_style(Style::default().fg(app.palette.border_inactive))
            .title(title);

        // Clamp scroll to keep content visible (can't scroll past last line)
        let visible_height = area.height.saturating_sub(2) as usize; // Account for border
        let max_scroll = line_count.saturating_sub(visible_height);
        let clamped_scroll = app.preview_scroll_offset.min(max_scroll);

        // Update the actual offset so scrolling stops at the end
        if clamped_scroll != app.preview_scroll_offset {
            app.preview_scroll_offset = clamped_scroll;
        }

        let para = Paragraph::new(lines)
            .block(p_block)
            .wrap(Wrap { trim: false })
            .scroll((clamped_scroll as u16, 0));

        f.render_widget(para, area);

        // Scrollbar for preview - use max_scroll as range so thumb reaches bottom
        if line_count > visible_height {
            let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .begin_symbol(Some("▴"))
                .end_symbol(Some("▾"))
                .track_symbol(Some("│"))
                .thumb_symbol("█");
            let mut scroll_state = ScrollbarState::new(max_scroll.max(1)).position(clamped_scroll);
            f.render_stateful_widget(
                scrollbar,
                area.inner(Margin { vertical: 1, horizontal: 0 }),
                &mut scroll_state,
            );
        }
    }

    click_zones.push(ClickZone {
        rect: area,
        action: AppAction::None,
    });
}
