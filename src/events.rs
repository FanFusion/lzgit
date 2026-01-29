//! Event handling module for keyboard and mouse events.
//!
//! This module extracts the event handling code from the main loop into
//! dedicated functions for better organization and maintainability.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

use crate::commit::CommitFocus;
use crate::conflict::ConflictResolution;
use crate::git::{self, GitDiffMode};
use crate::{
    App, BranchPickerMode, GitFooterAction, LogDetailMode, LogPaneFocus, LogSubTab,
    LogZoom, StashConfirmAction, Tab, THEME_ORDER,
};

/// Result of handling a key event.
pub enum KeyEventResult {
    /// Continue the event loop normally
    Continue,
    /// Should quit the application
    Quit,
}

/// Handle a key press event.
///
/// Returns `KeyEventResult::Quit` if the application should exit.
pub fn handle_key_event(app: &mut App, key: KeyEvent) -> KeyEventResult {
    match key.code {
        KeyCode::Char('q') => return KeyEventResult::Quit,
        KeyCode::Char('1')
            if app.operation_popup.is_none()
                && !app.theme_picker.open
                && !app.command_palette.open
                && !app.stash_ui.open
                && app.stash_confirm.is_none()
                && !app.branch_ui.open
                && app.current_tab != Tab::Terminal =>
        {
            app.current_tab = Tab::Git;
            app.git.refresh(&app.current_path);
            app.update_git_operation();
        }
        KeyCode::Char('2')
            if app.operation_popup.is_none()
                && !app.theme_picker.open
                && !app.command_palette.open
                && !app.stash_ui.open
                && app.stash_confirm.is_none()
                && !app.branch_ui.open
                && app.current_tab != Tab::Terminal =>
        {
            app.current_tab = Tab::Log;
            app.refresh_log_data();
        }
        KeyCode::Char('3')
            if app.operation_popup.is_none()
                && !app.theme_picker.open
                && !app.command_palette.open
                && !app.stash_ui.open
                && app.stash_confirm.is_none()
                && !app.branch_ui.open
                && app.current_tab != Tab::Terminal =>
        {
            app.current_tab = Tab::Explorer;
        }
        KeyCode::Char('p')
            if key.modifiers.contains(KeyModifiers::CONTROL)
                && app.operation_popup.is_none()
                && app.discard_confirm.is_none()
                && app.stash_confirm.is_none()
                && !app.branch_ui.open
                && !app.author_ui.open
                && app.context_menu.is_none()
                && !app.log_ui.inspect.open =>
        {
            app.open_command_palette();
        }
        KeyCode::Char('T')
            if app.operation_popup.is_none()
                && app.discard_confirm.is_none()
                && app.stash_confirm.is_none()
                && !app.branch_ui.open
                && !app.author_ui.open
                && app.context_menu.is_none()
                && !app.log_ui.inspect.open =>
        {
            app.open_theme_picker();
        }
        KeyCode::Esc => {
            handle_escape(app);
        }
        _ => {
            handle_modal_or_tab_key(app, key);
        }
    }
    KeyEventResult::Continue
}

/// Handle the Escape key in various contexts.
fn handle_escape(app: &mut App) {
    app.context_menu = None;
    app.discard_confirm = None;
    app.update_confirm = None;
    app.quick_stash_confirm = false;
    app.new_branch_input = None;
    app.operation_popup = None;
    app.theme_picker.open = false;
    app.command_palette.open = false;

    if app.current_tab == Tab::Log && app.log_ui.filter_edit {
        if app.log_ui.filter_query.trim().is_empty() {
            app.log_ui.filter_edit = false;
        } else {
            app.log_ui.filter_query.clear();
            app.log_ui.update_filtered();
            app.refresh_log_diff();
        }
    } else {
        app.log_ui.filter_edit = false;
    }
    app.log_ui.inspect.close();

    if app.branch_ui.open {
        if app.branch_ui.confirm_checkout.is_some() {
            app.branch_ui.confirm_checkout = None;
        } else {
            app.close_branch_picker();
        }
    }
    if app.author_ui.open {
        app.close_author_picker();
    }
    if app.stash_ui.open {
        if app.stash_confirm.is_some() {
            app.stash_confirm = None;
        } else {
            app.close_stash_picker();
        }
    }
    if app.current_tab == Tab::Git {
        app.commit.open = false;
    }
}

/// Handle keys when a modal is open or route to tab-specific handlers.
fn handle_modal_or_tab_key(app: &mut App, key: KeyEvent) {
    if app.theme_picker.open {
        handle_theme_picker_key(app, key);
    } else if app.command_palette.open {
        handle_command_palette_key(app, key);
    } else if let Some(popup) = &mut app.operation_popup {
        match key.code {
            KeyCode::Esc | KeyCode::Enter => app.operation_popup = None,
            KeyCode::Char('j') | KeyCode::Down => popup.scroll_y = popup.scroll_y.saturating_add(3),
            KeyCode::Char('k') | KeyCode::Up => popup.scroll_y = popup.scroll_y.saturating_sub(3),
            _ => {}
        }
    } else if app.update_confirm.is_some() {
        handle_update_confirm_key(app, key);
    } else if app.quick_stash_confirm {
        handle_quick_stash_confirm_key(app, key);
    } else if app.new_branch_input.is_some() {
        handle_new_branch_input_key(app, key);
    } else if app.branch_ui.open {
        handle_branch_picker_key(app, key);
    } else {
        handle_tab_key(app, key);
    }
}

/// Handle keys when theme picker is open.
fn handle_theme_picker_key(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Char('j') | KeyCode::Down => app.move_theme_picker(1),
        KeyCode::Char('k') | KeyCode::Up => app.move_theme_picker(-1),
        KeyCode::Enter => app.apply_theme_picker_selection(),
        KeyCode::Char(ch) if ('1'..='5').contains(&ch) => {
            let idx = ch.to_digit(10).unwrap_or(1).saturating_sub(1) as usize;
            if idx < THEME_ORDER.len() {
                app.theme_picker.list_state.select(Some(idx));
                app.apply_theme_picker_selection();
            }
        }
        _ => {}
    }
}

/// Handle keys when command palette is open.
fn handle_command_palette_key(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Char('j') | KeyCode::Down => app.move_command_palette(1),
        KeyCode::Char('k') | KeyCode::Up => app.move_command_palette(-1),
        KeyCode::Enter => app.run_command_palette_selection(),
        _ => {}
    }
}

/// Handle keys when update confirmation is shown.
fn handle_update_confirm_key(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
            app.confirm_update();
        }
        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
            app.update_confirm = None;
        }
        _ => {}
    }
}

/// Handle keys when quick stash confirmation is shown.
fn handle_quick_stash_confirm_key(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
            app.quick_stash_confirm = false;
            app.start_operation_job("git stash", true);
        }
        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
            app.quick_stash_confirm = false;
        }
        _ => {}
    }
}

/// Handle keys when new branch input is active.
fn handle_new_branch_input_key(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => {
            app.new_branch_input = None;
        }
        KeyCode::Enter => {
            if let Some(name) = app.new_branch_input.take() {
                let name = name.trim();
                if !name.is_empty() {
                    let cmd = format!("git checkout -b {}", name);
                    app.start_operation_job(&cmd, true);
                }
            }
        }
        KeyCode::Backspace => {
            if let Some(ref mut input) = app.new_branch_input {
                input.pop();
            }
        }
        KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            if let Some(ref mut input) = app.new_branch_input {
                input.push(ch);
            }
        }
        _ => {}
    }
}

/// Handle keys when branch picker is open.
fn handle_branch_picker_key(app: &mut App, key: KeyEvent) {
    if app.branch_ui.confirm_checkout.is_some() {
        match key.code {
            KeyCode::Enter => app.branch_checkout_selected(true),
            KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => {
                app.branch_ui.confirm_checkout = None;
            }
            _ => {}
        }
    } else {
        match key.code {
            KeyCode::Esc => app.close_branch_picker(),
            KeyCode::Enter => match app.branch_picker_mode {
                BranchPickerMode::Checkout => app.branch_checkout_selected(false),
                BranchPickerMode::LogView => app.confirm_log_branch_picker(),
            },
            KeyCode::Char('j') | KeyCode::Down => app.branch_ui.move_selection(1),
            KeyCode::Char('k') | KeyCode::Up => app.branch_ui.move_selection(-1),
            KeyCode::PageDown => app.branch_ui.move_selection(10),
            KeyCode::PageUp => app.branch_ui.move_selection(-10),
            KeyCode::Backspace => {
                app.branch_ui.query.pop();
                app.branch_ui.update_filtered();
            }
            KeyCode::Char(ch)
                if !key.modifiers.contains(KeyModifiers::CONTROL)
                    && !key.modifiers.contains(KeyModifiers::ALT) =>
            {
                app.branch_ui.query.push(ch);
                app.branch_ui.update_filtered();
            }
            _ => {}
        }
    }
}

/// Handle keys based on current tab.
fn handle_tab_key(app: &mut App, key: KeyEvent) {
    match app.current_tab {
        Tab::Explorer => handle_explorer_key(app, key),
        Tab::Git => handle_git_key(app, key),
        Tab::Log => handle_log_key(app, key),
        Tab::Terminal => handle_terminal_key(app, key),
    }
}

/// Handle keys in Explorer tab.
fn handle_explorer_key(app: &mut App, key: KeyEvent) {
    if app.delete_confirm.is_some() {
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => app.confirm_delete(),
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                app.delete_confirm = None;
            }
            _ => {}
        }
        return;
    }

    match key.code {
        KeyCode::Char('h') | KeyCode::Backspace | KeyCode::Left => app.go_parent(),
        KeyCode::Char('l') | KeyCode::Enter | KeyCode::Right => app.enter_selected(),
        KeyCode::Char('j') | KeyCode::Down => {
            let i = app.selected_index().unwrap_or(0);
            if i + 1 < app.files.len() {
                app.list_state.select(Some(i + 1));
                app.update_preview();
                app.preview_scroll = 0;
            }
        }
        KeyCode::Char('k') | KeyCode::Up => {
            let i = app.selected_index().unwrap_or(0);
            if i > 0 {
                app.list_state.select(Some(i - 1));
                app.update_preview();
                app.preview_scroll = 0;
            }
        }
        KeyCode::Char('.') => {
            app.show_hidden = !app.show_hidden;
            app.load_files();
        }
        KeyCode::Char('g') => {
            app.list_state.select(Some(0));
            app.update_preview();
            app.preview_scroll = 0;
        }
        KeyCode::Char('G') => {
            if !app.files.is_empty() {
                app.list_state.select(Some(app.files.len() - 1));
                app.update_preview();
                app.preview_scroll = 0;
            }
        }
        KeyCode::Char('i') => app.add_selected_to_gitignore(),
        KeyCode::Char('r') => {
            app.load_files();
            app.set_status("Refreshed");
        }
        KeyCode::Char('d') | KeyCode::Delete => {
            app.show_delete_confirm();
        }
        KeyCode::Char('e') => {
            app.open_selected_in_editor();
        }
        KeyCode::Char('H') => {
            app.syntax_highlight = !app.syntax_highlight;
            app.set_status(if app.syntax_highlight {
                "Syntax highlight: on"
            } else {
                "Syntax highlight: off"
            });
        }
        KeyCode::Char('R') => {
            app.auto_refresh = !app.auto_refresh;
            app.set_status(if app.auto_refresh {
                "Auto-refresh: on"
            } else {
                "Auto-refresh: off"
            });
        }
        _ => {}
    }
}

/// Handle keys in Git tab.
fn handle_git_key(app: &mut App, key: KeyEvent) {
    if app.discard_confirm.is_some() {
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => app.confirm_discard(),
            KeyCode::Char('n') | KeyCode::Char('N') => {
                app.discard_confirm = None;
            }
            _ => {}
        }
        return;
    }

    if app.stash_ui.open {
        handle_stash_ui_key(app, key);
        return;
    }

    if app.commit.open {
        handle_commit_editor_key(app, key);
        return;
    }

    // Main Git tab keys
    match key.code {
        KeyCode::Char(' ') => app.toggle_stage_for_selection(),
        KeyCode::Char('A') => app.stage_all_visible(),
        KeyCode::Char('U') => app.unstage_all_visible(),
        KeyCode::Char('a') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.select_all_git_filtered();
        }
        KeyCode::Char('z')
            if key.modifiers.contains(KeyModifiers::CONTROL)
                && !key.modifiers.contains(KeyModifiers::SHIFT) =>
        {
            app.undo_revert();
        }
        KeyCode::Char('z') | KeyCode::Char('Z')
            if key.modifiers.contains(KeyModifiers::CONTROL)
                && key.modifiers.contains(KeyModifiers::SHIFT) =>
        {
            app.redo_revert();
        }
        KeyCode::Char('y') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.redo_revert();
        }
        KeyCode::Char('r') => app.refresh_git_state(),
        KeyCode::Char('i') => app.add_selected_to_gitignore(),
        KeyCode::Char('w') => {
            app.wrap_diff = !app.wrap_diff;
            app.set_status(if app.wrap_diff {
                "Diff wrap: on"
            } else {
                "Diff wrap: off"
            });
        }
        KeyCode::Char('H') => {
            app.syntax_highlight = !app.syntax_highlight;
            app.set_status(if app.syntax_highlight {
                "Syntax highlight: on"
            } else {
                "Syntax highlight: off"
            });
        }
        KeyCode::Char('F') => app.toggle_full_file_view(),
        KeyCode::Char('B') => app.open_branch_picker(),
        KeyCode::Char('z') => {
            app.quick_stash_confirm = true;
        }
        KeyCode::Char('N') => {
            app.new_branch_input = Some(String::new());
        }
        KeyCode::Char('c') => {
            app.commit.open = true;
            app.commit.focus = CommitFocus::Message;
        }
        KeyCode::Char('n')
            if app
                .git
                .selected_tree_entry()
                .is_some_and(|e| e.is_conflict) =>
        {
            app.change_conflict_block(1)
        }
        KeyCode::Char('p')
            if app
                .git
                .selected_tree_entry()
                .is_some_and(|e| e.is_conflict) =>
        {
            app.change_conflict_block(-1)
        }
        KeyCode::Char('o')
            if app
                .git
                .selected_tree_entry()
                .is_some_and(|e| e.is_conflict) =>
        {
            app.apply_conflict_resolution(ConflictResolution::Ours)
        }
        KeyCode::Char('t')
            if app
                .git
                .selected_tree_entry()
                .is_some_and(|e| e.is_conflict) =>
        {
            app.apply_conflict_resolution(ConflictResolution::Theirs)
        }
        KeyCode::Char('b')
            if app
                .git
                .selected_tree_entry()
                .is_some_and(|e| e.is_conflict) =>
        {
            app.apply_conflict_resolution(ConflictResolution::Both)
        }
        KeyCode::Char('a')
            if app
                .git
                .selected_tree_entry()
                .is_some_and(|e| e.is_conflict) =>
        {
            app.mark_conflict_resolved()
        }
        KeyCode::Char('s') => {
            app.git.diff_mode = match app.git.diff_mode {
                GitDiffMode::Unified => GitDiffMode::SideBySide,
                GitDiffMode::SideBySide => GitDiffMode::Unified,
            };
        }
        KeyCode::Char('[') => app.adjust_git_left_width(-2),
        KeyCode::Char(']') => app.adjust_git_left_width(2),
        KeyCode::Left => {
            if let Some(item) = app.git.selected_tree_item() {
                use git::FlatNodeType;
                if item.node_type == FlatNodeType::Section
                    || item.node_type == FlatNodeType::Directory
                {
                    app.git.collapse_tree_item();
                } else {
                    app.git.diff_scroll_x = app.git.diff_scroll_x.saturating_sub(4);
                }
            } else {
                app.git.diff_scroll_x = app.git.diff_scroll_x.saturating_sub(4);
            }
        }
        KeyCode::Right => {
            if let Some(item) = app.git.selected_tree_item() {
                use git::FlatNodeType;
                if item.node_type == FlatNodeType::Section
                    || item.node_type == FlatNodeType::Directory
                {
                    app.git.expand_tree_item();
                } else {
                    app.git.diff_scroll_x = app.git.diff_scroll_x.saturating_add(4);
                }
            } else {
                app.git.diff_scroll_x = app.git.diff_scroll_x.saturating_add(4);
            }
        }
        KeyCode::Char('j') | KeyCode::Down => {
            app.git.tree_move_down();
            app.request_git_diff_update();
        }
        KeyCode::Char('k') | KeyCode::Up => {
            app.git.tree_move_up();
            app.request_git_diff_update();
        }
        KeyCode::Char('g') => {
            app.git.tree_goto_first();
            app.request_git_diff_update();
        }
        KeyCode::Char('G') => {
            app.git.tree_goto_last();
            app.request_git_diff_update();
        }
        KeyCode::Enter => {
            app.git.toggle_tree_expand();
        }
        _ => {}
    }
}

/// Handle keys when stash UI is open in Git tab.
fn handle_stash_ui_key(app: &mut App, key: KeyEvent) {
    if app.stash_confirm.is_some() {
        match key.code {
            KeyCode::Enter => app.confirm_stash_action(),
            KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => {
                app.stash_confirm = None;
            }
            _ => {}
        }
    } else {
        match key.code {
            KeyCode::Esc => app.close_stash_picker(),
            KeyCode::Enter => app.stash_apply_selected(),
            KeyCode::Char('a') => app.stash_apply_selected(),
            KeyCode::Char('p') => {
                app.stash_ui.status = None;
                if let Some(sel) = app.stash_ui.selected_stash() {
                    app.open_stash_confirm(StashConfirmAction::Pop, sel.selector.clone());
                } else {
                    app.set_stash_status("No stash selected");
                }
            }
            KeyCode::Char('d') => {
                app.stash_ui.status = None;
                if let Some(sel) = app.stash_ui.selected_stash() {
                    app.open_stash_confirm(StashConfirmAction::Drop, sel.selector.clone());
                } else {
                    app.set_stash_status("No stash selected");
                }
            }
            KeyCode::Char('j') | KeyCode::Down => app.stash_ui.move_selection(1),
            KeyCode::Char('k') | KeyCode::Up => app.stash_ui.move_selection(-1),
            KeyCode::Backspace => {
                app.stash_ui.query.pop();
                app.stash_ui.update_filtered();
            }
            KeyCode::Char(ch)
                if !key.modifiers.contains(KeyModifiers::CONTROL)
                    && !key.modifiers.contains(KeyModifiers::ALT) =>
            {
                app.stash_ui.query.push(ch);
                app.stash_ui.update_filtered();
            }
            _ => {}
        }
    }
}

/// Handle keys when commit editor is open.
fn handle_commit_editor_key(app: &mut App, key: KeyEvent) {
    if key.modifiers.contains(KeyModifiers::CONTROL)
        && matches!(key.code, KeyCode::Char('g') | KeyCode::Char('G'))
    {
        app.start_ai_generate();
    } else if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Enter {
        app.handle_git_footer(GitFooterAction::Commit);
    } else if !app.commit.busy {
        match key.code {
            KeyCode::Left => app.commit.move_left(),
            KeyCode::Right => app.commit.move_right(),
            KeyCode::Home => app.commit.move_home(),
            KeyCode::End => app.commit.move_end(),
            KeyCode::Backspace => app.commit.backspace(),
            KeyCode::Delete => app.commit.delete(),
            KeyCode::Enter => app.commit.insert_char('\n'),
            KeyCode::Char(ch)
                if !key.modifiers.contains(KeyModifiers::CONTROL)
                    && !key.modifiers.contains(KeyModifiers::ALT) =>
            {
                app.commit.insert_char(ch);
            }
            _ => {}
        }
    }
}

/// Handle keys in Log tab.
fn handle_log_key(app: &mut App, key: KeyEvent) {
    if app.author_ui.open {
        handle_author_picker_key(app, key);
        return;
    }

    if app.stash_confirm.is_some() {
        match key.code {
            KeyCode::Enter => app.confirm_stash_action(),
            KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => {
                app.stash_confirm = None;
            }
            _ => {}
        }
        return;
    }

    if app.log_ui.inspect.open {
        handle_log_inspect_key(app, key);
        return;
    }

    // Main Log tab keys
    match key.code {
        KeyCode::Char('/') if app.log_ui.subtab != LogSubTab::Commands => {
            app.log_ui.filter_edit = !app.log_ui.filter_edit;
            app.log_ui.focus = LogPaneFocus::Commits;
        }
        KeyCode::Enter if app.log_ui.filter_edit => {
            app.log_ui.filter_edit = false;
        }
        KeyCode::Enter if app.log_ui.subtab == LogSubTab::Stash => {
            app.stash_apply_log_selected();
        }
        KeyCode::Backspace if app.log_ui.filter_edit => {
            app.log_ui.filter_query.pop();
            app.log_ui.update_filtered();
            app.refresh_log_diff();
        }
        KeyCode::Char('u') | KeyCode::Char('l')
            if app.log_ui.subtab != LogSubTab::Commands
                && key.modifiers.contains(KeyModifiers::CONTROL) =>
        {
            app.log_ui.filter_query.clear();
            app.log_ui.update_filtered();
            app.refresh_log_diff();
        }
        KeyCode::Char(ch) if app.log_ui.filter_edit => {
            if !key.modifiers.contains(KeyModifiers::CONTROL)
                && !key.modifiers.contains(KeyModifiers::ALT)
            {
                app.log_ui.filter_query.push(ch);
                app.log_ui.update_filtered();
                app.refresh_log_diff();
            }
        }
        KeyCode::Char('r') => app.set_log_subtab(LogSubTab::Reflog),
        KeyCode::Char('R') => {
            app.refresh_git_state();
        }
        KeyCode::Char('h') => app.set_log_subtab(LogSubTab::History),
        KeyCode::Char('t') => app.set_log_subtab(LogSubTab::Stash),
        KeyCode::Char('c') => app.set_log_subtab(LogSubTab::Commands),
        KeyCode::Char('x') if app.log_ui.subtab == LogSubTab::Commands => {
            app.git_log.clear();
            app.log_ui.command_state.select(None);
            app.refresh_log_diff();
            app.set_status("Log cleared");
        }
        KeyCode::Char('a') if app.log_ui.subtab == LogSubTab::Stash => {
            app.stash_apply_log_selected();
        }
        KeyCode::Char('p') if app.log_ui.subtab == LogSubTab::Stash => {
            app.open_stash_confirm_log_selected(StashConfirmAction::Pop);
        }
        KeyCode::Char('d') if app.log_ui.subtab == LogSubTab::Stash => {
            app.open_stash_confirm_log_selected(StashConfirmAction::Drop);
        }
        KeyCode::Char('d') if app.log_ui.subtab == LogSubTab::History => {
            let next = match app.log_ui.detail_mode {
                LogDetailMode::Diff => LogDetailMode::Files,
                LogDetailMode::Files => LogDetailMode::Diff,
            };
            app.log_ui.set_detail_mode(next);
            app.refresh_log_diff();
        }
        KeyCode::Char('f') if app.log_ui.subtab == LogSubTab::History => {
            app.log_ui.set_detail_mode(LogDetailMode::Files);
            app.refresh_log_diff();
        }
        KeyCode::Char('F') if app.log_ui.subtab == LogSubTab::History => {
            let next = match app.log_ui.detail_mode {
                LogDetailMode::Diff => LogDetailMode::Files,
                LogDetailMode::Files => LogDetailMode::Diff,
            };
            app.log_ui.set_detail_mode(next);
            app.refresh_log_diff();
        }
        KeyCode::Char('i') => {
            if app.log_ui.inspect.open {
                app.log_ui.inspect.close();
            } else {
                app.open_log_inspect();
            }
        }
        KeyCode::Char('L') if app.log_ui.subtab != LogSubTab::Commands => {
            app.load_more_log_data();
        }
        KeyCode::Char('z') => {
            if app.current_tab == Tab::Git {
                app.git_zoom_diff = !app.git_zoom_diff;
                app.save_persisted_ui_settings();
            } else {
                app.toggle_log_zoom();
            }
        }
        KeyCode::Tab => app.cycle_log_focus(),
        KeyCode::Char('[') => app.adjust_log_left_width(-2),
        KeyCode::Char(']') => app.adjust_log_left_width(2),
        KeyCode::Char('s') => {
            app.log_ui.diff_mode = match app.log_ui.diff_mode {
                GitDiffMode::Unified => GitDiffMode::SideBySide,
                GitDiffMode::SideBySide => GitDiffMode::Unified,
            };
            app.log_ui.focus = LogPaneFocus::Diff;
        }
        KeyCode::Char('w') => {
            app.wrap_diff = !app.wrap_diff;
            app.set_status(if app.wrap_diff {
                "Diff wrap: on"
            } else {
                "Diff wrap: off"
            });
        }
        KeyCode::Char('H') => {
            app.syntax_highlight = !app.syntax_highlight;
            app.set_status(if app.syntax_highlight {
                "Syntax highlight: on"
            } else {
                "Syntax highlight: off"
            });
        }
        KeyCode::Char('B') => app.open_branch_picker(),
        KeyCode::Char('A') if app.log_ui.subtab != LogSubTab::Commands => {
            app.open_author_picker();
        }
        KeyCode::Left => app.log_ui.diff_scroll_x = app.log_ui.diff_scroll_x.saturating_sub(4),
        KeyCode::Right => app.log_ui.diff_scroll_x = app.log_ui.diff_scroll_x.saturating_add(4),
        KeyCode::Char('j') | KeyCode::Down => match app.log_ui.focus {
            LogPaneFocus::Commits => app.move_log_selection(1),
            LogPaneFocus::Files => app.move_log_file_selection(1),
            LogPaneFocus::Diff => app.log_ui.diff_scroll_y = app.log_ui.diff_scroll_y.saturating_add(1),
        },
        KeyCode::Char('k') | KeyCode::Up => match app.log_ui.focus {
            LogPaneFocus::Commits => app.move_log_selection(-1),
            LogPaneFocus::Files => app.move_log_file_selection(-1),
            LogPaneFocus::Diff => app.log_ui.diff_scroll_y = app.log_ui.diff_scroll_y.saturating_sub(1),
        },
        KeyCode::Char('g') => match app.log_ui.focus {
            LogPaneFocus::Commits => app.select_log_item(0),
            LogPaneFocus::Files => app.select_log_file(0),
            LogPaneFocus::Diff => app.log_ui.diff_scroll_y = 0,
        },
        KeyCode::Char('G') => match app.log_ui.focus {
            LogPaneFocus::Commits => {
                let n = app.active_log_len();
                if n > 0 {
                    app.select_log_item(n - 1);
                }
            }
            LogPaneFocus::Files => {
                let n = app.log_ui.files.len();
                if n > 0 {
                    app.select_log_file(n - 1);
                }
            }
            LogPaneFocus::Diff => app.log_ui.diff_scroll_y = u16::MAX,
        },
        _ => {}
    }
}

/// Handle keys when author picker is open in Log tab.
fn handle_author_picker_key(app: &mut App, key: KeyEvent) {
    if app.author_ui.filtered.is_empty() {
        match key.code {
            KeyCode::Esc => app.close_author_picker(),
            KeyCode::Backspace => {
                app.author_ui.query.pop();
                app.author_ui.update_filtered();
            }
            KeyCode::Char(ch)
                if !key.modifiers.contains(KeyModifiers::CONTROL)
                    && !key.modifiers.contains(KeyModifiers::ALT) =>
            {
                app.author_ui.query.push(ch);
                app.author_ui.update_filtered();
            }
            _ => {}
        }
    } else {
        match key.code {
            KeyCode::Esc => app.close_author_picker(),
            KeyCode::Enter => app.confirm_author_picker(),
            KeyCode::Down | KeyCode::Char('j') => app.author_ui.move_selection(1),
            KeyCode::Up | KeyCode::Char('k') => app.author_ui.move_selection(-1),
            KeyCode::PageDown => app.author_ui.move_selection(10),
            KeyCode::PageUp => app.author_ui.move_selection(-10),
            KeyCode::Backspace => {
                app.author_ui.query.pop();
                app.author_ui.update_filtered();
            }
            KeyCode::Char(ch)
                if !key.modifiers.contains(KeyModifiers::CONTROL)
                    && !key.modifiers.contains(KeyModifiers::ALT) =>
            {
                app.author_ui.query.push(ch);
                app.author_ui.update_filtered();
            }
            _ => {}
        }
    }
}

/// Handle keys when log inspect is open.
fn handle_log_inspect_key(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc | KeyCode::Enter => app.log_ui.inspect.close(),
        KeyCode::Down => {
            app.move_log_selection(1);
            app.open_log_inspect();
        }
        KeyCode::Up => {
            app.move_log_selection(-1);
            app.open_log_inspect();
        }
        KeyCode::PageDown => {
            app.log_ui.inspect.scroll_y = app.log_ui.inspect.scroll_y.saturating_add(10)
        }
        KeyCode::PageUp => {
            app.log_ui.inspect.scroll_y = app.log_ui.inspect.scroll_y.saturating_sub(10)
        }
        KeyCode::Char('j') => {
            app.log_ui.inspect.scroll_y = app.log_ui.inspect.scroll_y.saturating_add(3)
        }
        KeyCode::Char('k') => {
            app.log_ui.inspect.scroll_y = app.log_ui.inspect.scroll_y.saturating_sub(3)
        }
        KeyCode::Char('y') => {
            if let Some(s) = app
                .selected_log_hash()
                .or_else(|| app.selected_log_command())
            {
                app.request_copy_to_clipboard(s);
            }
            app.log_ui.inspect.close();
        }
        KeyCode::Char('Y') => {
            if let Some(s) = app.selected_log_subject() {
                app.request_copy_to_clipboard(s);
            } else {
                app.request_copy_to_clipboard(app.log_ui.inspect.body.clone());
            }
            app.log_ui.inspect.close();
        }
        _ => {}
    }
}

/// Handle keys in Terminal tab.
fn handle_terminal_key(app: &mut App, key: KeyEvent) {
    let bytes: Vec<u8> = match key.code {
        KeyCode::Char(c) => {
            if key.modifiers.contains(KeyModifiers::CONTROL) {
                // Ctrl+letter -> 1-26
                let code = c.to_ascii_lowercase() as u8;
                if code >= b'a' && code <= b'z' {
                    vec![code - b'a' + 1]
                } else {
                    vec![]
                }
            } else {
                let mut buf = [0u8; 4];
                c.encode_utf8(&mut buf).as_bytes().to_vec()
            }
        }
        KeyCode::Enter => vec![b'\r'],
        KeyCode::Backspace => vec![127],
        KeyCode::Tab => vec![b'\t'],
        KeyCode::Esc => vec![27],
        KeyCode::Up => b"\x1b[A".to_vec(),
        KeyCode::Down => b"\x1b[B".to_vec(),
        KeyCode::Right => b"\x1b[C".to_vec(),
        KeyCode::Left => b"\x1b[D".to_vec(),
        KeyCode::Home => b"\x1b[H".to_vec(),
        KeyCode::End => b"\x1b[F".to_vec(),
        KeyCode::PageUp => b"\x1b[5~".to_vec(),
        KeyCode::PageDown => b"\x1b[6~".to_vec(),
        KeyCode::Delete => b"\x1b[3~".to_vec(),
        KeyCode::Insert => b"\x1b[2~".to_vec(),
        _ => vec![],
    };
    if !bytes.is_empty() {
        app.terminal.write_input(&bytes);
    }
}

/// Result of handling a mouse event.
pub enum MouseEventResult {
    /// Continue the event loop normally
    Continue,
    /// Skip to next iteration (equivalent to `continue` in the loop)
    Skip,
}

/// Handle a mouse event.
///
/// Returns `MouseEventResult::Skip` if the caller should skip to the next loop iteration.
pub fn handle_mouse_event(app: &mut App, mouse: MouseEvent) -> MouseEventResult {
    match mouse.kind {
        MouseEventKind::Moved => {
            app.update_context_menu_hover(mouse.row, mouse.column);
        }
        MouseEventKind::ScrollDown => {
            handle_scroll_down(app, mouse);
        }
        MouseEventKind::ScrollUp => {
            handle_scroll_up(app, mouse);
        }
        MouseEventKind::Down(MouseButton::Left) => {
            app.handle_click(mouse.row, mouse.column, mouse.modifiers);
        }
        MouseEventKind::Down(MouseButton::Right) => {
            return handle_right_click(app, mouse);
        }
        _ => {}
    }
    MouseEventResult::Continue
}

/// Handle scroll down event.
fn handle_scroll_down(app: &mut App, mouse: MouseEvent) {
    if app.theme_picker.open {
        app.move_theme_picker(3);
    } else if app.command_palette.open {
        app.move_command_palette(3);
    } else if app.stash_ui.open {
        app.stash_ui.move_selection(3);
    } else if app.branch_ui.open {
        app.branch_ui.move_selection(3);
    } else if app.author_ui.open {
        app.author_ui.move_selection(3);
    } else {
        match app.current_tab {
            Tab::Explorer => {
                if mouse.column >= app.explorer_preview_x {
                    app.preview_scroll = app.preview_scroll.saturating_add(3);
                } else {
                    let i = app.selected_index().unwrap_or(0);
                    if i + 3 < app.files.len() {
                        app.list_state.select(Some(i + 3));
                        app.update_preview();
                        app.preview_scroll = 0;
                    } else {
                        app.list_state.select(Some(app.files.len().saturating_sub(1)));
                        app.update_preview();
                    }
                }
            }
            Tab::Git => {
                handle_git_scroll_down(app, mouse);
            }
            Tab::Log => {
                handle_log_scroll_down(app, mouse);
            }
            Tab::Terminal => {
                // Terminal handles scrollback internally
            }
        }
    }
}

/// Handle scroll down in Git tab.
fn handle_git_scroll_down(app: &mut App, mouse: MouseEvent) {
    if app.branch_ui.open {
        app.branch_ui.move_selection(3);
    } else if mouse.column >= app.git_diff_x {
        if mouse.modifiers.contains(KeyModifiers::SHIFT) {
            app.git.diff_scroll_x = app.git.diff_scroll_x.saturating_add(4);
        } else if app
            .git
            .selected_tree_entry()
            .is_some_and(|e| e.is_conflict)
        {
            app.conflict_ui.scroll_y = app.conflict_ui.scroll_y.saturating_add(3);
        } else if app.git.show_full_file {
            app.git.full_file_scroll_y = app.git.full_file_scroll_y.saturating_add(3);
        } else {
            app.git.diff_scroll_y = app.git.diff_scroll_y.saturating_add(3);
        }
    } else {
        let i = app.git.list_state.selected().unwrap_or(0);
        let next = (i + 3).min(app.git.filtered.len().saturating_sub(1));
        if app.git.filtered.is_empty() {
            app.git.list_state.select(None);
        } else {
            app.git.select_filtered(next);
            app.request_git_diff_update();
        }
    }
}

/// Handle scroll down in Log tab.
fn handle_log_scroll_down(app: &mut App, mouse: MouseEvent) {
    let files_mode = app.log_ui.detail_mode == LogDetailMode::Files
        && app.log_ui.subtab == LogSubTab::History
        && app.log_ui.zoom != LogZoom::List;

    if app.log_ui.inspect.open {
        app.log_ui.inspect.scroll_y = app.log_ui.inspect.scroll_y.saturating_add(3);
    } else if mouse.column >= app.log_diff_x {
        app.log_ui.focus = LogPaneFocus::Diff;
        if mouse.modifiers.contains(KeyModifiers::SHIFT) {
            app.log_ui.diff_scroll_x = app.log_ui.diff_scroll_x.saturating_add(4);
        } else {
            app.log_ui.diff_scroll_y = app.log_ui.diff_scroll_y.saturating_add(3);
        }
    } else if files_mode && mouse.column >= app.log_files_x {
        app.log_ui.focus = LogPaneFocus::Files;
        app.move_log_file_selection(3);
    } else {
        app.log_ui.focus = LogPaneFocus::Commits;
        app.move_log_selection(3);
    }
}

/// Handle scroll up event.
fn handle_scroll_up(app: &mut App, mouse: MouseEvent) {
    if app.theme_picker.open {
        app.move_theme_picker(-3);
    } else if app.command_palette.open {
        app.move_command_palette(-3);
    } else if app.stash_ui.open {
        app.stash_ui.move_selection(-3);
    } else if app.branch_ui.open {
        app.branch_ui.move_selection(-3);
    } else if app.author_ui.open {
        app.author_ui.move_selection(-3);
    } else {
        match app.current_tab {
            Tab::Explorer => {
                if mouse.column >= app.explorer_preview_x {
                    app.preview_scroll = app.preview_scroll.saturating_sub(3);
                } else {
                    let i = app.selected_index().unwrap_or(0);
                    if i >= 3 {
                        app.list_state.select(Some(i - 3));
                        app.update_preview();
                        app.preview_scroll = 0;
                    } else {
                        app.list_state.select(Some(0));
                        app.update_preview();
                    }
                }
            }
            Tab::Git => {
                handle_git_scroll_up(app, mouse);
            }
            Tab::Log => {
                handle_log_scroll_up(app, mouse);
            }
            Tab::Terminal => {
                // Terminal handles scrollback internally
            }
        }
    }
}

/// Handle scroll up in Git tab.
fn handle_git_scroll_up(app: &mut App, mouse: MouseEvent) {
    if app.branch_ui.open {
        app.branch_ui.move_selection(-3);
    } else if mouse.column >= app.git_diff_x {
        if mouse.modifiers.contains(KeyModifiers::SHIFT) {
            app.git.diff_scroll_x = app.git.diff_scroll_x.saturating_sub(4);
        } else if app
            .git
            .selected_tree_entry()
            .is_some_and(|e| e.is_conflict)
        {
            app.conflict_ui.scroll_y = app.conflict_ui.scroll_y.saturating_sub(3);
        } else if app.git.show_full_file {
            app.git.full_file_scroll_y = app.git.full_file_scroll_y.saturating_sub(3);
        } else {
            app.git.diff_scroll_y = app.git.diff_scroll_y.saturating_sub(3);
        }
    } else {
        let i = app.git.list_state.selected().unwrap_or(0);
        if i >= 3 {
            app.git.select_filtered(i - 3);
            app.request_git_diff_update();
        } else if !app.git.filtered.is_empty() {
            app.git.select_filtered(0);
            app.request_git_diff_update();
        }
    }
}

/// Handle scroll up in Log tab.
fn handle_log_scroll_up(app: &mut App, mouse: MouseEvent) {
    let files_mode = app.log_ui.detail_mode == LogDetailMode::Files
        && app.log_ui.subtab == LogSubTab::History
        && app.log_ui.zoom != LogZoom::List;

    if app.log_ui.inspect.open {
        app.log_ui.inspect.scroll_y = app.log_ui.inspect.scroll_y.saturating_sub(3);
    } else if mouse.column >= app.log_diff_x {
        app.log_ui.focus = LogPaneFocus::Diff;
        if mouse.modifiers.contains(KeyModifiers::SHIFT) {
            app.log_ui.diff_scroll_x = app.log_ui.diff_scroll_x.saturating_sub(4);
        } else {
            app.log_ui.diff_scroll_y = app.log_ui.diff_scroll_y.saturating_sub(3);
        }
    } else if files_mode && mouse.column >= app.log_files_x {
        app.log_ui.focus = LogPaneFocus::Files;
        app.move_log_file_selection(-3);
    } else {
        app.log_ui.focus = LogPaneFocus::Commits;
        app.move_log_selection(-3);
    }
}

/// Handle right click event.
fn handle_right_click(app: &mut App, mouse: MouseEvent) -> MouseEventResult {
    if app.theme_picker.open {
        app.theme_picker.open = false;
        return MouseEventResult::Skip;
    }
    if app.command_palette.open {
        app.command_palette.open = false;
        return MouseEventResult::Skip;
    }
    if app.stash_ui.open {
        if app.stash_confirm.is_some() {
            app.stash_confirm = None;
        } else {
            app.close_stash_picker();
        }
        return MouseEventResult::Skip;
    }

    app.context_menu = None;
    app.pending_menu_action = None;
    app.handle_context_click(mouse.row, mouse.column, mouse.modifiers);
    app.open_context_menu(mouse.row, mouse.column);
    MouseEventResult::Continue
}
