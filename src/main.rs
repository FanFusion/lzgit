use arboard::Clipboard;
use base64::{Engine as _, engine::general_purpose};
use crossterm::{
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers,
        MouseButton, MouseEventKind,
    },
    execute,
    style::Print,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    prelude::*,
    widgets::{
        Block, BorderType, Borders, Clear, List, ListItem, ListState, Paragraph, Scrollbar,
        ScrollbarOrientation, ScrollbarState, Wrap,
    },
};
use ratatui_image::{StatefulImage, picker::Picker, protocol::StatefulProtocol};
use std::{
    cmp::Ordering,
    env,
    fs::{self},
    io::{self, Write},
    path::PathBuf,
    time::{Duration, Instant},
};

mod git;

use git::{
    GitDiffCellKind, GitDiffMode, GitDiffRow, GitSection, GitState, build_side_by_side_rows,
    pad_to_width, render_side_by_side_cell,
};

mod theme {
    use ratatui::style::Color;

    pub const BG: Color = Color::Rgb(30, 30, 46);
    pub const FG: Color = Color::Rgb(205, 214, 244);
    pub const ACCENT_PRIMARY: Color = Color::Rgb(203, 166, 247);
    pub const ACCENT_SECONDARY: Color = Color::Rgb(250, 179, 135);
    pub const ACCENT_TERTIARY: Color = Color::Rgb(137, 180, 250);
    pub const BORDER_INACTIVE: Color = Color::Rgb(88, 91, 112);
    pub const SELECTION_BG: Color = Color::Rgb(69, 71, 90);
    pub const DIR_COLOR: Color = Color::Rgb(137, 180, 250);
    pub const EXE_COLOR: Color = Color::Rgb(166, 227, 161);
    pub const SIZE_COLOR: Color = Color::Rgb(147, 153, 178);
    pub const BTN_BG: Color = Color::Rgb(243, 139, 168);
    pub const BTN_FG: Color = Color::Rgb(24, 24, 37);
    pub const MENU_BG: Color = Color::Rgb(49, 50, 68);

    pub const DIFF_ADD_BG: Color = Color::Rgb(72, 104, 88);
    pub const DIFF_DEL_BG: Color = Color::Rgb(110, 70, 92);
    pub const DIFF_HUNK_BG: Color = Color::Rgb(74, 78, 116);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tab {
    Explorer,
    Git,
    Log,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GitFooterAction {
    Stage,
    Unstage,
    Discard,
    Commit,
}

#[derive(Clone, Debug, PartialEq)]
enum AppAction {
    SwitchTab(Tab),
    RefreshGit,
    Navigate(PathBuf),
    EnterDir,
    GoParent,
    Select(usize),
    SelectGitSection(GitSection),
    SelectGitFile(usize),
    ToggleCommitDrawer,
    GitFooter(GitFooterAction),
    ToggleHidden,
    Quit,
    None,
    ContextMenuAction(usize),
}

#[derive(Clone)]
struct ClickZone {
    rect: Rect,
    action: AppAction,
}

#[derive(Clone)]
struct FileEntry {
    name: String,
    path: PathBuf,
    is_dir: bool,
    is_symlink: bool,
    is_exec: bool,
    is_hidden: bool,
    size: u64,
}

struct ContextMenu {
    x: u16,
    y: u16,
    options: Vec<(String, MenuAction)>,
}

#[derive(Clone)]
enum MenuAction {
    AddBookmark,
    RemoveBookmark,
    CopyPath,
    CopyRelPath,
    Rename,
}

struct App {
    current_path: PathBuf,
    files: Vec<FileEntry>,
    list_state: ListState,
    preview_scroll: u16,
    should_quit: bool,
    show_hidden: bool,

    current_tab: Tab,

    git: GitState,
    commit_drawer_open: bool,

    explorer_preview_x: u16,
    git_diff_x: u16,

    zones: Vec<ClickZone>,
    last_click: Option<(Instant, usize)>,
    bookmarks: Vec<(String, PathBuf)>,

    context_menu: Option<ContextMenu>,

    picker: Picker,
    image_state: Option<StatefulProtocol>,
    current_image_path: Option<PathBuf>,
    preview_error: Option<String>,
    status_message: Option<(String, Instant)>,
    status_ttl: Duration,

    pending_clipboard: Option<String>,
    bookmarks_path: Option<PathBuf>,
}

impl App {
    fn new(start_path: PathBuf, picker: Picker) -> Self {
        let mut app = Self {
            current_path: start_path,
            files: Vec::new(),
            list_state: ListState::default(),
            preview_scroll: 0,
            should_quit: false,
            show_hidden: false,

            current_tab: Tab::Explorer,

            git: GitState::new(),
            commit_drawer_open: false,

            explorer_preview_x: 0,
            git_diff_x: 0,

            zones: Vec::new(),
            last_click: None,
            bookmarks: vec![
                ("Root".to_string(), PathBuf::from("/")),
                (
                    "Home".to_string(),
                    env::home_dir().unwrap_or_else(|| PathBuf::from("/")),
                ),
                ("Tmp".to_string(), PathBuf::from("/tmp")),
                ("Bin".to_string(), PathBuf::from("/usr/bin")),
            ],
            context_menu: None,
            picker,
            image_state: None,
            current_image_path: None,
            preview_error: None,
            status_message: None,
            status_ttl: Duration::from_secs(2),
            pending_clipboard: None,
            bookmarks_path: bookmarks_file_path(),
        };
        app.load_persisted_bookmarks();
        app.load_files();
        if !app.files.is_empty() {
            app.list_state.select(Some(0));
            app.update_preview();
        }
        app.git.refresh(&app.current_path);
        app
    }

    fn refresh_git_state(&mut self) {
        self.git.refresh(&self.current_path);
    }

    fn load_files(&mut self) {
        self.files.clear();
        let read_path = if self.current_path.exists() {
            self.current_path.clone()
        } else {
            PathBuf::from("/")
        };

        if let Ok(entries) = fs::read_dir(&read_path) {
            let mut items: Vec<FileEntry> = entries
                .filter_map(|e| e.ok())
                .map(|entry| {
                    let path = entry.path();
                    let name = entry.file_name().to_string_lossy().to_string();
                    let metadata = entry.metadata().ok();
                    let file_type = entry.file_type().ok();

                    let is_dir = file_type.map(|t| t.is_dir()).unwrap_or(false);
                    let is_symlink = file_type.map(|t| t.is_symlink()).unwrap_or(false);
                    let is_hidden = name.starts_with('.');

                    let is_exec = metadata
                        .as_ref()
                        .map(|m| {
                            #[cfg(unix)]
                            {
                                use std::os::unix::fs::PermissionsExt;
                                m.permissions().mode() & 0o111 != 0
                            }
                            #[cfg(not(unix))]
                            false
                        })
                        .unwrap_or(false);

                    let size = metadata.as_ref().map(|m| m.len()).unwrap_or(0);

                    FileEntry {
                        name,
                        path,
                        is_dir,
                        is_symlink,
                        is_exec,
                        is_hidden,
                        size,
                    }
                })
                .filter(|f| self.show_hidden || !f.is_hidden)
                .collect();

            items.sort_by(|a, b| match (a.is_dir, b.is_dir) {
                (true, false) => Ordering::Less,
                (false, true) => Ordering::Greater,
                _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            });

            self.files = items;
        }
        self.preview_scroll = 0;
        self.update_preview();
    }

    fn selected_index(&self) -> Option<usize> {
        self.list_state.selected()
    }

    fn selected_file(&self) -> Option<&FileEntry> {
        self.selected_index().and_then(|i| self.files.get(i))
    }

    fn is_ssh_session() -> bool {
        env::var_os("SSH_CONNECTION").is_some() || env::var_os("SSH_TTY").is_some()
    }

    fn set_status<S: Into<String>>(&mut self, msg: S) {
        self.status_message = Some((msg.into(), Instant::now()));
    }

    fn maybe_expire_status(&mut self) {
        let should_clear = self
            .status_message
            .as_ref()
            .is_some_and(|(_, t)| t.elapsed() >= self.status_ttl);
        if should_clear {
            self.status_message = None;
        }
    }

    fn request_copy_to_clipboard<S: Into<String>>(&mut self, text: S) {
        self.pending_clipboard = Some(text.into());
    }

    fn take_pending_clipboard(&mut self) -> Option<String> {
        self.pending_clipboard.take()
    }

    fn load_persisted_bookmarks(&mut self) {
        let Some(path) = self.bookmarks_path.clone() else {
            return;
        };

        let data = fs::read_to_string(&path).ok();
        let Some(data) = data else {
            return;
        };

        for line in data.lines() {
            let mut parts = line.splitn(2, '\t');
            let name = parts.next().unwrap_or("").trim();
            let path_str = parts.next().unwrap_or("").trim();
            if name.is_empty() || path_str.is_empty() {
                continue;
            }

            let p = PathBuf::from(path_str);
            if !self.bookmarks.iter().any(|(_, existing)| existing == &p) {
                self.bookmarks.push((name.to_string(), p));
            }
        }
    }

    fn save_persisted_bookmarks(&mut self) {
        let Some(path) = self.bookmarks_path.clone() else {
            self.set_status("Cannot save favorites: no config dir");
            return;
        };

        let default_paths = default_bookmark_paths();
        let mut lines = Vec::new();
        for (name, p) in &self.bookmarks {
            if default_paths.iter().any(|d| d == p) {
                continue;
            }
            lines.push(format!("{}\t{}", name, p.to_string_lossy()));
        }
        let content = lines.join("\n");

        if let Some(parent) = path.parent()
            && let Err(e) = fs::create_dir_all(parent)
        {
            self.set_status(format!("Cannot save favorites: {}", e));
            return;
        }

        let tmp = path.with_extension("tmp");
        if fs::write(&tmp, content).is_err() || fs::rename(&tmp, &path).is_err() {
            let _ = fs::remove_file(&tmp);
            self.set_status("Failed to save favorites");
        }
    }

    fn update_preview(&mut self) {
        self.preview_error = None;

        let Some(file) = self.selected_file() else {
            self.image_state = None;
            self.current_image_path = None;
            return;
        };

        if file.is_dir {
            self.image_state = None;
            self.current_image_path = None;
            return;
        }

        let path = file.path.clone();
        let is_image = path
            .extension()
            .and_then(|s| s.to_str())
            .map(|ext| ext.to_lowercase())
            .is_some_and(|ext| {
                matches!(
                    ext.as_str(),
                    "png" | "jpg" | "jpeg" | "gif" | "bmp" | "webp"
                )
            });

        if !is_image {
            self.image_state = None;
            self.current_image_path = None;
            return;
        }

        if self.current_image_path.as_ref() == Some(&path) {
            return;
        }

        match image::ImageReader::open(&path)
            .and_then(|r| r.with_guessed_format())
            .and_then(|r| r.decode().map_err(std::io::Error::other))
        {
            Ok(dyn_img) => {
                let proto = self.picker.new_resize_protocol(dyn_img);
                self.image_state = Some(proto);
                self.current_image_path = Some(path);
            }
            Err(e) => {
                self.preview_error = Some(format!("Image Error: {}", e));
                self.image_state = None;
                self.current_image_path = None;
            }
        }
    }

    fn navigate_to(&mut self, path: PathBuf) {
        if let Ok(canonical) = path.canonicalize() {
            self.current_path = canonical;
            self.load_files();
            self.list_state
                .select(if self.files.is_empty() { None } else { Some(0) });
        } else if path.exists() {
            self.current_path = path;
            self.load_files();
            self.list_state
                .select(if self.files.is_empty() { None } else { Some(0) });
        }
        self.update_preview();
    }

    fn enter_selected(&mut self) {
        if let Some(file) = self.selected_file().cloned()
            && file.is_dir
        {
            self.navigate_to(file.path);
        }
    }

    fn go_parent(&mut self) {
        if let Some(parent) = self.current_path.parent() {
            let old_name = self
                .current_path
                .file_name()
                .map(|n| n.to_string_lossy().to_string());
            let parent_path = parent.to_path_buf();
            self.navigate_to(parent_path);

            if let Some(name) = old_name
                && let Some(idx) = self.files.iter().position(|f| f.name == name)
            {
                self.list_state.select(Some(idx));
            }
        }
        self.update_preview();
    }

    fn handle_click(&mut self, row: u16, col: u16) {
        if self.context_menu.is_some() {
            let mut hit_menu = false;
            for zone in self.zones.iter().rev() {
                if row >= zone.rect.y
                    && row < zone.rect.y + zone.rect.height
                    && col >= zone.rect.x
                    && col < zone.rect.x + zone.rect.width
                {
                    if let AppAction::ContextMenuAction(_) = zone.action {
                        hit_menu = true;
                    }
                    break;
                }
            }

            if !hit_menu {
                self.context_menu = None;
                return;
            }
        }

        let mut action = AppAction::None;

        for zone in self.zones.iter().rev() {
            if row >= zone.rect.y
                && row < zone.rect.y + zone.rect.height
                && col >= zone.rect.x
                && col < zone.rect.x + zone.rect.width
            {
                action = zone.action.clone();
                break;
            }
        }

        match action {
            AppAction::SwitchTab(tab) => {
                self.current_tab = tab;
                self.context_menu = None;
                if tab == Tab::Git {
                    self.refresh_git_state();
                }
            }
            AppAction::RefreshGit => {
                self.refresh_git_state();
                self.set_status("Git refreshed");
            }
            AppAction::Navigate(path) => self.navigate_to(path),
            AppAction::EnterDir => self.enter_selected(),
            AppAction::GoParent => self.go_parent(),
            AppAction::Select(idx) => {
                let now = Instant::now();
                let is_double_click = if let Some((last_time, last_idx)) = self.last_click {
                    idx == last_idx && now.duration_since(last_time) < Duration::from_millis(400)
                } else {
                    false
                };

                self.list_state.select(Some(idx));
                self.update_preview();
                self.preview_scroll = 0;

                if is_double_click {
                    self.enter_selected();
                    self.last_click = None;
                } else {
                    self.last_click = Some((now, idx));
                }
            }
            AppAction::SelectGitSection(section) => {
                self.git.set_section(section, &self.current_path);
            }
            AppAction::SelectGitFile(idx) => {
                self.git.select_filtered(idx, &self.current_path);
            }
            AppAction::ToggleCommitDrawer => {
                self.commit_drawer_open = !self.commit_drawer_open;
            }
            AppAction::GitFooter(action) => match action {
                GitFooterAction::Stage => self.set_status("TODO: stage"),
                GitFooterAction::Unstage => self.set_status("TODO: unstage"),
                GitFooterAction::Discard => self.set_status("TODO: discard"),
                GitFooterAction::Commit => self.set_status("TODO: commit"),
            },
            AppAction::ToggleHidden => {
                self.show_hidden = !self.show_hidden;
                self.load_files();
            }
            AppAction::Quit => self.should_quit = true,
            AppAction::ContextMenuAction(idx) => {
                self.execute_menu_action(idx);
            }
            AppAction::None => {}
        }
    }

    fn open_context_menu(&mut self, row: u16, col: u16, file_idx: Option<usize>) {
        if let Some(idx) = file_idx {
            self.list_state.select(Some(idx));
            self.update_preview();
        }

        let mut options = vec![
            (" 📋 Copy Path ".to_string(), MenuAction::CopyPath),
            (
                " 📄 Copy Relative Path ".to_string(),
                MenuAction::CopyRelPath,
            ),
        ];

        let current_path = if let Some(idx) = self.selected_index() {
            if let Some(f) = self.files.get(idx) {
                if f.is_dir {
                    f.path.clone()
                } else {
                    self.current_path.clone()
                }
            } else {
                self.current_path.clone()
            }
        } else {
            self.current_path.clone()
        };

        let is_bookmarked = self.bookmarks.iter().any(|(_, p)| p == &current_path);
        if is_bookmarked {
            options.push((
                " 🚫 Remove Bookmark ".to_string(),
                MenuAction::RemoveBookmark,
            ));
        } else {
            options.push((" 🔖 Add Bookmark ".to_string(), MenuAction::AddBookmark));
        }

        options.push((" ✏️  Rename (TODO) ".to_string(), MenuAction::Rename));

        self.context_menu = Some(ContextMenu {
            x: col,
            y: row,
            options,
        });
    }

    fn execute_menu_action(&mut self, action_idx: usize) {
        if let Some(menu) = &self.context_menu
            && let Some((_, action)) = menu.options.get(action_idx)
        {
            match action {
                MenuAction::CopyPath => {
                    if let Some(file) = self.selected_file() {
                        self.request_copy_to_clipboard(file.path.to_string_lossy().to_string());
                    }
                }
                MenuAction::CopyRelPath => {
                    if let Some(file) = self.selected_file() {
                        let rel = file
                            .path
                            .strip_prefix(&self.current_path)
                            .ok()
                            .map(|p| p.to_string_lossy().to_string())
                            .or_else(|| {
                                file.path
                                    .file_name()
                                    .map(|s| s.to_string_lossy().to_string())
                            })
                            .unwrap_or_else(|| file.path.to_string_lossy().to_string());
                        self.request_copy_to_clipboard(rel);
                    }
                }
                MenuAction::AddBookmark => {
                    let target = if let Some(file) = self.selected_file() {
                        if file.is_dir {
                            file.path.clone()
                        } else {
                            self.current_path.clone()
                        }
                    } else {
                        self.current_path.clone()
                    };
                    let name = target
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or("Root".to_string());
                    if !self.bookmarks.iter().any(|(_, p)| p == &target) {
                        self.bookmarks.push((name, target));
                        self.save_persisted_bookmarks();
                    }
                }
                MenuAction::RemoveBookmark => {
                    let target = if let Some(file) = self.selected_file() {
                        if file.is_dir {
                            file.path.clone()
                        } else {
                            self.current_path.clone()
                        }
                    } else {
                        self.current_path.clone()
                    };
                    self.bookmarks.retain(|(_, p)| p != &target);
                    self.save_persisted_bookmarks();
                }
                MenuAction::Rename => {}
            }
        }
        self.context_menu = None;
    }
}

fn osc52_sequence(text: &str) -> String {
    let encoded = general_purpose::STANDARD.encode(text.as_bytes());
    format!("\x1b]52;c;{}\x07", encoded)
}

fn in_tmux() -> bool {
    env::var_os("TMUX").is_some()
        || env::var_os("TERM").is_some_and(|t| t.to_string_lossy().starts_with("tmux"))
}

fn tmux_passthrough(seq: &str) -> String {
    let escaped = seq.replace('\x1b', "\x1b\x1b");
    format!("\x1bPtmux;{}\x1b\\", escaped)
}

fn emit_osc52<W: Write>(w: &mut W, text: &str) -> io::Result<()> {
    let seq = osc52_sequence(text);
    let out = if in_tmux() {
        tmux_passthrough(&seq)
    } else {
        seq
    };
    execute!(w, Print(out))?;
    w.flush()
}

fn try_set_system_clipboard(text: &str) -> Result<(), String> {
    let mut cb = Clipboard::new().map_err(|e| e.to_string())?;
    cb.set_text(text.to_string()).map_err(|e| e.to_string())
}

fn bookmarks_file_path() -> Option<PathBuf> {
    let home = env::home_dir()?;
    let base = env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".config"));
    Some(base.join("te").join("bookmarks.tsv"))
}

fn default_bookmark_paths() -> Vec<PathBuf> {
    vec![
        PathBuf::from("/"),
        env::home_dir().unwrap_or_else(|| PathBuf::from("/")),
        PathBuf::from("/tmp"),
        PathBuf::from("/usr/bin"),
    ]
}

fn format_size(size: u64) -> String {
    if size < 1024 {
        format!("{}B", size)
    } else if size < 1024 * 1024 {
        format!("{:.1}K", size as f64 / 1024.0)
    } else if size < 1024 * 1024 * 1024 {
        format!("{:.1}M", size as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.1}G", size as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

fn draw_ui(f: &mut Frame, app: &mut App) -> Vec<ClickZone> {
    let mut zones = Vec::new();
    let area = f.area();

    f.render_widget(Block::default().bg(theme::BG), area);

    let main_layout = if app.current_tab == Tab::Git {
        let commit_h = if app.commit_drawer_open { 7 } else { 1 };
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(0),
                Constraint::Length(commit_h),
                Constraint::Length(3),
            ])
            .split(area)
    } else {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(0),
                Constraint::Length(3),
            ])
            .split(area)
    };

    let top_bar = main_layout[0];
    let content_area = main_layout[1];
    let (commit_area, footer_area) = if app.current_tab == Tab::Git {
        (Some(main_layout[2]), main_layout[3])
    } else {
        (None, main_layout[2])
    };

    let top_block = Block::default()
        .borders(Borders::BOTTOM)
        .border_style(Style::default().fg(theme::BORDER_INACTIVE).bg(theme::BG));
    f.render_widget(top_block.clone(), top_bar);

    let tabs_y = top_bar.y;
    let mut tab_x = top_bar.x + 1;
    for (label, tab) in [
        (" Explorer ", Tab::Explorer),
        (" Git ", Tab::Git),
        (" Log ", Tab::Log),
    ] {
        let width = label.len() as u16;
        let is_active = app.current_tab == tab;
        let style = if is_active {
            Style::default()
                .bg(theme::ACCENT_PRIMARY)
                .fg(theme::BTN_FG)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().bg(theme::BG).fg(theme::FG)
        };
        f.render_widget(
            Paragraph::new(label).style(style),
            Rect::new(tab_x, tabs_y, width, 1),
        );
        zones.push(ClickZone {
            rect: Rect::new(tab_x, tabs_y, width, 1),
            action: AppAction::SwitchTab(tab),
        });
        tab_x += width + 1;
    }

    let second_row_y = top_bar.y + 1;

    match app.current_tab {
        Tab::Explorer => {
            let mut breadcrumb_x = top_bar.x + 2;
            let breadcrumb_y = second_row_y;

            let home_txt = " 🏠 Home ";
            let home_width = home_txt.len() as u16;
            f.render_widget(
                Paragraph::new(Span::styled(
                    home_txt,
                    Style::default().fg(theme::ACCENT_SECONDARY).bold(),
                )),
                Rect::new(breadcrumb_x, breadcrumb_y, home_width, 1),
            );
            zones.push(ClickZone {
                rect: Rect::new(breadcrumb_x, breadcrumb_y, home_width, 1),
                action: AppAction::Navigate(env::home_dir().unwrap_or_else(|| PathBuf::from("/"))),
            });
            breadcrumb_x += home_width;

            let path_str = app.current_path.to_string_lossy();
            let components: Vec<&str> = path_str
                .split(std::path::MAIN_SEPARATOR)
                .filter(|s| !s.is_empty())
                .collect();

            let mut acc_path = PathBuf::from("/");

            f.render_widget(
                Paragraph::new(Span::raw(" / ")),
                Rect::new(breadcrumb_x, breadcrumb_y, 3, 1),
            );
            breadcrumb_x += 3;

            for (i, part) in components.iter().enumerate() {
                if cfg!(windows) && i == 0 {
                    acc_path = PathBuf::from(part);
                } else {
                    acc_path.push(part);
                }

                let label = format!(" {} ", part);
                let width = label.len() as u16;

                if breadcrumb_x + width > top_bar.width - 2 {
                    break;
                }

                let style = if i == components.len() - 1 {
                    Style::default()
                        .fg(theme::ACCENT_PRIMARY)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(theme::FG)
                };

                f.render_widget(
                    Paragraph::new(Span::styled(&label, style)),
                    Rect::new(breadcrumb_x, breadcrumb_y, width, 1),
                );

                zones.push(ClickZone {
                    rect: Rect::new(breadcrumb_x, breadcrumb_y, width, 1),
                    action: AppAction::Navigate(acc_path.clone()),
                });

                breadcrumb_x += width;
                if i < components.len() - 1 {
                    f.render_widget(
                        Paragraph::new(Span::styled(
                            " › ",
                            Style::default().fg(theme::BORDER_INACTIVE),
                        )),
                        Rect::new(breadcrumb_x, breadcrumb_y, 3, 1),
                    );
                    breadcrumb_x += 3;
                }
            }
        }
        Tab::Git => {
            let repo = app
                .git
                .repo_root
                .as_ref()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|| "(not a git repo)".to_string());
            let branch = if app.git.branch.is_empty() {
                "(unknown)".to_string()
            } else {
                app.git.branch.clone()
            };
            let label = format!(
                " Repo: {}   Branch: {} ▼   ↑{} ↓{}   [Refresh] ",
                repo, branch, app.git.ahead, app.git.behind
            );
            let width = top_bar.width.saturating_sub(2);
            f.render_widget(
                Paragraph::new(label.as_str()).style(Style::default().fg(theme::FG)),
                Rect::new(top_bar.x + 2, second_row_y, width, 1),
            );

            let refresh_label = "[Refresh]";
            let refresh_x = top_bar.x + 2 + width.saturating_sub(refresh_label.len() as u16);
            let refresh_rect = Rect::new(refresh_x, second_row_y, refresh_label.len() as u16, 1);
            zones.push(ClickZone {
                rect: refresh_rect,
                action: AppAction::RefreshGit,
            });
        }
        Tab::Log => {
            let label = " Log (TODO) ";
            let width = label.len() as u16;
            f.render_widget(
                Paragraph::new(label).style(Style::default().fg(theme::FG)),
                Rect::new(top_bar.x + 2, second_row_y, width, 1),
            );
        }
    }
    match app.current_tab {
        Tab::Explorer => {
            let content_chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Length(20),
                    Constraint::Percentage(40),
                    Constraint::Percentage(40),
                ])
                .split(content_area);

            let sidebar_area = content_chunks[0];
            let sidebar_block = Block::default()
                .borders(Borders::RIGHT)
                .border_style(Style::default().fg(theme::BORDER_INACTIVE))
                .title(" Places ")
                .title_style(Style::default().fg(theme::ACCENT_TERTIARY));
            f.render_widget(sidebar_block, sidebar_area);

            let mut place_y = sidebar_area.y + 1;
            for (name, target) in &app.bookmarks {
                let is_active = if target.as_path() == std::path::Path::new("/") {
                    app.current_path.as_path() == std::path::Path::new("/")
                } else {
                    app.current_path.starts_with(target)
                };

                let style = if is_active {
                    Style::default()
                        .fg(theme::ACCENT_SECONDARY)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(theme::FG)
                };

                let label = format!("  {}", name);
                f.render_widget(
                    Paragraph::new(label).style(style),
                    Rect::new(sidebar_area.x, place_y, sidebar_area.width - 1, 1),
                );

                zones.push(ClickZone {
                    rect: Rect::new(sidebar_area.x, place_y, sidebar_area.width - 1, 1),
                    action: AppAction::Navigate(target.clone()),
                });
                place_y += 2;
            }

            let list_area = content_chunks[1];
            let list_block = Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(theme::ACCENT_PRIMARY))
                .title(format!(" Files ({}) ", app.files.len()));

            let items: Vec<ListItem> = app
                .files
                .iter()
                .map(|file| {
                    let icon = if file.is_dir {
                        ""
                    } else if file.is_exec {
                        "󰆍"
                    } else if file.is_symlink {
                        ""
                    } else if file.name.ends_with(".rs") {
                        ""
                    } else {
                        "󰈙"
                    };

                    let color = if file.is_dir {
                        theme::DIR_COLOR
                    } else if file.is_exec {
                        theme::EXE_COLOR
                    } else {
                        theme::FG
                    };

                    let name_span = Span::styled(&file.name, Style::default().fg(color));
                    let mut spans = vec![Span::raw(format!("{} ", icon)), name_span];

                    if !file.is_dir {
                        spans.push(Span::styled(
                            format!(" ({})", format_size(file.size)),
                            Style::default().fg(theme::SIZE_COLOR),
                        ));
                    }

                    ListItem::new(Line::from(spans))
                })
                .collect();

            let list = List::new(items)
                .block(list_block)
                .highlight_style(
                    Style::default()
                        .bg(theme::SELECTION_BG)
                        .add_modifier(Modifier::BOLD),
                )
                .highlight_symbol("▎ ");

            f.render_stateful_widget(list, list_area, &mut app.list_state.clone());

            let list_inner = list_area.inner(Margin {
                vertical: 1,
                horizontal: 1,
            });
            let start_index = app.list_state.offset();
            let end_index = (start_index + list_inner.height as usize).min(app.files.len());

            for (i, idx) in (start_index..end_index).enumerate() {
                let rect = Rect::new(list_inner.x, list_inner.y + i as u16, list_inner.width, 1);
                zones.push(ClickZone {
                    rect,
                    action: AppAction::Select(idx),
                });
            }

            let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .begin_symbol(Some("▴"))
                .end_symbol(Some("▾"))
                .track_symbol(Some("│"))
                .thumb_symbol("║");
            let mut scroll_state =
                ScrollbarState::new(app.files.len()).position(app.selected_index().unwrap_or(0));
            f.render_stateful_widget(
                scrollbar,
                list_area.inner(Margin {
                    vertical: 1,
                    horizontal: 0,
                }),
                &mut scroll_state,
            );

            let preview_area = content_chunks[2];
            app.explorer_preview_x = preview_area.x;

            if let Some(state) = &mut app.image_state {
                let image = StatefulImage::new();
                f.render_stateful_widget(image, preview_area, state);
            } else {
                let preview_text = if let Some(err) = &app.preview_error {
                    err.clone()
                } else if let Some(file) = app.selected_file() {
                    if file.is_dir {
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
                        // Read first 2KB instead of checking strict size limit
                        if let Ok(file_handle) = fs::File::open(&file.path) {
                            use std::io::Read;
                            let mut reader = std::io::BufReader::new(file_handle);
                            let mut buffer = [0; 2048];
                            if let Ok(n) = reader.read(&mut buffer) {
                                if n == 0 {
                                    "Empty file".to_string()
                                } else {
                                    String::from_utf8_lossy(&buffer[..n]).to_string()
                                }
                            } else {
                                "Error reading file".to_string()
                            }
                        } else {
                            "Could not open file".to_string()
                        }
                    }
                } else {
                    String::new()
                };

                let lines: Vec<Line> = preview_text.lines().map(Line::raw).collect();
                let p_block = Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(theme::BORDER_INACTIVE))
                    .title(" Preview ");

                let para = Paragraph::new(lines)
                    .block(p_block)
                    .wrap(Wrap { trim: false })
                    .scroll((app.preview_scroll, 0));

                f.render_widget(para, preview_area);
            }

            zones.push(ClickZone {
                rect: preview_area,
                action: AppAction::None,
            });
        }
        Tab::Git => {
            let content_chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Length(32), Constraint::Min(0)])
                .split(content_area);

            let left_area = content_chunks[0];
            let diff_area = content_chunks[1];
            app.git_diff_x = diff_area.x;

            let left_chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(9), Constraint::Min(0)])
                .split(left_area);

            let sections_area = left_chunks[0];
            let files_area = left_chunks[1];

            let sections_block = Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(theme::ACCENT_PRIMARY))
                .title(" Changes ");
            f.render_widget(sections_block.clone(), sections_area);

            let section_inner = sections_area.inner(Margin {
                vertical: 1,
                horizontal: 1,
            });

            let mut counts = (0usize, 0usize, 0usize, 0usize);
            for e in &app.git.entries {
                let staged = e.x != ' ' && e.x != '?';
                let unstaged = e.y != ' ' && e.y != '?';
                if e.is_conflict {
                    counts.3 += 1;
                }
                if e.is_untracked {
                    counts.2 += 1;
                }
                if staged && !e.is_conflict && !e.is_untracked {
                    counts.1 += 1;
                }
                if unstaged && !e.is_conflict && !e.is_untracked {
                    counts.0 += 1;
                }
            }

            let sections = [
                (GitSection::Working, format!(" Working ({}) ", counts.0)),
                (GitSection::Staged, format!(" Staged ({}) ", counts.1)),
                (GitSection::Untracked, format!(" Untracked ({}) ", counts.2)),
                (GitSection::Conflicts, format!(" Conflicts ({}) ", counts.3)),
            ];

            for (i, (sec, label)) in sections.iter().enumerate() {
                if i as u16 >= section_inner.height {
                    break;
                }
                let is_active = app.git.section == *sec;
                let style = if is_active {
                    Style::default()
                        .bg(theme::SELECTION_BG)
                        .add_modifier(Modifier::BOLD)
                        .fg(theme::FG)
                } else {
                    Style::default().fg(theme::FG)
                };
                let rect = Rect::new(
                    section_inner.x,
                    section_inner.y + i as u16,
                    section_inner.width,
                    1,
                );
                f.render_widget(Paragraph::new(label.as_str()).style(style), rect);
                zones.push(ClickZone {
                    rect,
                    action: AppAction::SelectGitSection(*sec),
                });
            }

            let files_block = Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(theme::BORDER_INACTIVE))
                .title(" Files ");

            let file_items: Vec<ListItem> = app
                .git
                .filtered
                .iter()
                .filter_map(|abs| app.git.entries.get(*abs))
                .map(|e| {
                    let status = if e.is_untracked {
                        "??".to_string()
                    } else if e.is_conflict {
                        format!("{}{}", e.x, e.y)
                    } else {
                        let staged = e.x != ' ' && e.x != '?';
                        let unstaged = e.y != ' ' && e.y != '?';
                        match (staged, unstaged) {
                            (true, false) => e.x.to_string(),
                            (false, true) => e.y.to_string(),
                            (true, true) => format!("{}{}", e.x, e.y),
                            (false, false) => "".to_string(),
                        }
                    };

                    let status_style = match status.as_str() {
                        "M" => Style::default().fg(theme::ACCENT_SECONDARY),
                        "A" => Style::default().fg(theme::EXE_COLOR),
                        "D" => Style::default().fg(theme::BTN_BG),
                        "??" => Style::default().fg(theme::ACCENT_TERTIARY),
                        _ => Style::default().fg(theme::FG),
                    };

                    let mut spans = vec![
                        Span::styled(format!("{:>2} ", status), status_style),
                        Span::styled(e.path.as_str(), Style::default().fg(theme::FG)),
                    ];
                    if let Some(from) = &e.renamed_from {
                        spans.push(Span::styled(
                            format!(" (from {})", from),
                            Style::default().fg(theme::BORDER_INACTIVE),
                        ));
                    }
                    ListItem::new(Line::from(spans))
                })
                .collect();

            let files_list = List::new(file_items)
                .block(files_block)
                .highlight_style(
                    Style::default()
                        .bg(theme::SELECTION_BG)
                        .add_modifier(Modifier::BOLD),
                )
                .highlight_symbol("▎ ");

            f.render_stateful_widget(files_list, files_area, &mut app.git.list_state.clone());

            let files_inner = files_area.inner(Margin {
                vertical: 1,
                horizontal: 1,
            });
            let start_index = app.git.list_state.offset();
            let end_index = (start_index + files_inner.height as usize).min(app.git.filtered.len());
            for (i, idx) in (start_index..end_index).enumerate() {
                let rect = Rect::new(
                    files_inner.x,
                    files_inner.y + i as u16,
                    files_inner.width,
                    1,
                );
                zones.push(ClickZone {
                    rect,
                    action: AppAction::SelectGitFile(idx),
                });
            }

            let mode_label = match app.git.diff_mode {
                GitDiffMode::SideBySide => "SxS",
                GitDiffMode::Unified => "Unified",
            };
            let diff_block = Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(theme::BORDER_INACTIVE))
                .title(format!(" Diff ({}) ", mode_label));

            let diff_lines: Vec<Line> = if app.git.repo_root.is_none() {
                vec![Line::raw("Not a git repository")]
            } else if app.git.diff_lines.is_empty() {
                vec![Line::raw("No selection")]
            } else {
                match app.git.diff_mode {
                    GitDiffMode::Unified => app
                        .git
                        .diff_lines
                        .iter()
                        .map(|l| Line::raw(l.as_str()))
                        .collect(),
                    GitDiffMode::SideBySide => {
                        let inner_w = diff_area.width.saturating_sub(2) as usize;
                        let sep_w = 1usize;
                        let left_w = inner_w.saturating_sub(sep_w) / 2;
                        let right_w = inner_w.saturating_sub(sep_w).saturating_sub(left_w);

                        let mut out = Vec::new();
                        let title_style =
                            Style::default().fg(theme::FG).add_modifier(Modifier::BOLD);
                        let sep_style = Style::default().fg(theme::BORDER_INACTIVE);

                        let left_title = pad_to_width(" Old ".to_string(), left_w);
                        let right_title = pad_to_width(" New ".to_string(), right_w);
                        out.push(Line::from(vec![
                            Span::styled(left_title, title_style),
                            Span::styled("│", sep_style),
                            Span::styled(right_title, title_style),
                        ]));

                        let rows = build_side_by_side_rows(&app.git.diff_lines);
                        for row in rows {
                            match row {
                                GitDiffRow::Meta(t) => {
                                    let style = if t.starts_with("@@") {
                                        Style::default()
                                            .fg(theme::ACCENT_TERTIARY)
                                            .bg(theme::DIFF_HUNK_BG)
                                    } else if t.starts_with("diff --git") {
                                        Style::default().fg(theme::ACCENT_PRIMARY)
                                    } else {
                                        Style::default().fg(theme::BORDER_INACTIVE)
                                    };
                                    out.push(Line::from(vec![Span::styled(t, style)]));
                                }
                                GitDiffRow::Split { old, new } => {
                                    let old_cell = render_side_by_side_cell(
                                        &old,
                                        left_w,
                                        app.git.diff_scroll_x as usize,
                                    );
                                    let new_cell = render_side_by_side_cell(
                                        &new,
                                        right_w,
                                        app.git.diff_scroll_x as usize,
                                    );

                                    let old_style = match old.kind {
                                        GitDiffCellKind::Delete => {
                                            Style::default().fg(theme::FG).bg(theme::DIFF_DEL_BG)
                                        }
                                        GitDiffCellKind::Context => {
                                            Style::default().fg(theme::FG).bg(theme::BG)
                                        }
                                        GitDiffCellKind::Add => {
                                            Style::default().fg(theme::FG).bg(theme::BG)
                                        }
                                        GitDiffCellKind::Empty => Style::default()
                                            .fg(theme::BORDER_INACTIVE)
                                            .bg(theme::BG),
                                    };
                                    let new_style = match new.kind {
                                        GitDiffCellKind::Add => {
                                            Style::default().fg(theme::FG).bg(theme::DIFF_ADD_BG)
                                        }
                                        GitDiffCellKind::Context => {
                                            Style::default().fg(theme::FG).bg(theme::BG)
                                        }
                                        GitDiffCellKind::Delete => {
                                            Style::default().fg(theme::FG).bg(theme::BG)
                                        }
                                        GitDiffCellKind::Empty => Style::default()
                                            .fg(theme::BORDER_INACTIVE)
                                            .bg(theme::BG),
                                    };

                                    out.push(Line::from(vec![
                                        Span::styled(old_cell, old_style),
                                        Span::styled("│", sep_style),
                                        Span::styled(new_cell, new_style),
                                    ]));
                                }
                            }
                        }

                        out
                    }
                }
            };

            let x_scroll = if app.git.diff_mode == GitDiffMode::Unified {
                app.git.diff_scroll_x
            } else {
                0
            };
            let diff_para = Paragraph::new(diff_lines)
                .block(diff_block)
                .scroll((app.git.diff_scroll_y, x_scroll));

            f.render_widget(diff_para, diff_area);
        }
        Tab::Log => {
            let block = Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(theme::BORDER_INACTIVE))
                .title(" Log ");
            f.render_widget(block, content_area);
            f.render_widget(
                Paragraph::new("TODO")
                    .style(Style::default().fg(theme::FG))
                    .block(Block::default()),
                content_area.inner(Margin {
                    vertical: 1,
                    horizontal: 1,
                }),
            );
        }
    }

    if let Some(commit_area) = commit_area {
        if app.commit_drawer_open {
            let commit_block = Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(theme::ACCENT_PRIMARY))
                .title(" Commit ");
            f.render_widget(commit_block.clone(), commit_area);

            let inner = commit_area.inner(Margin {
                vertical: 1,
                horizontal: 1,
            });
            let msg = Paragraph::new("Message: (TODO)")
                .style(Style::default().fg(theme::FG))
                .wrap(Wrap { trim: false });
            f.render_widget(msg, Rect::new(inner.x, inner.y, inner.width, 1));

            let buttons_y = commit_area.y + commit_area.height.saturating_sub(2);
            let mut x = commit_area.x + 2;
            for (label, action, color) in [
                (
                    " Commit ",
                    AppAction::GitFooter(GitFooterAction::Commit),
                    theme::ACCENT_SECONDARY,
                ),
                (" Amend ", AppAction::None, theme::ACCENT_TERTIARY),
                (" Close ", AppAction::ToggleCommitDrawer, theme::BTN_BG),
            ] {
                let w = label.len() as u16;
                let style = Style::default()
                    .bg(color)
                    .fg(theme::BTN_FG)
                    .add_modifier(Modifier::BOLD);
                f.render_widget(
                    Paragraph::new(label).style(style),
                    Rect::new(x, buttons_y, w, 1),
                );
                zones.push(ClickZone {
                    rect: Rect::new(x, buttons_y, w, 1),
                    action,
                });
                x += w + 2;
            }
        } else {
            let sep = Block::default()
                .borders(Borders::TOP)
                .border_style(Style::default().fg(theme::BORDER_INACTIVE));
            f.render_widget(sep, commit_area);

            let label = " Commit ▸ ";
            let w = label.len().min(commit_area.width as usize) as u16;
            f.render_widget(
                Paragraph::new(label)
                    .style(Style::default().fg(theme::FG).add_modifier(Modifier::BOLD)),
                Rect::new(commit_area.x + 2, commit_area.y, w, 1),
            );
            zones.push(ClickZone {
                rect: Rect::new(commit_area.x, commit_area.y, commit_area.width, 1),
                action: AppAction::ToggleCommitDrawer,
            });
        }
    }

    let footer_block = Block::default()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(theme::BORDER_INACTIVE));
    f.render_widget(footer_block, footer_area);

    let btn_y = footer_area.y + 1;
    let mut btn_x = footer_area.x + 2;

    let mut buttons: Vec<(String, AppAction, Color)> = Vec::new();
    match app.current_tab {
        Tab::Explorer => {
            buttons.push((
                " ⬅ Back (h) ".to_string(),
                AppAction::GoParent,
                theme::ACCENT_PRIMARY,
            ));
            buttons.push((
                " ⏎ Enter (l) ".to_string(),
                AppAction::EnterDir,
                theme::ACCENT_SECONDARY,
            ));
            buttons.push((
                " 👁 Hidden (.) ".to_string(),
                AppAction::ToggleHidden,
                theme::ACCENT_TERTIARY,
            ));
            buttons.push((" ✖ Quit (q) ".to_string(), AppAction::Quit, theme::BTN_BG));
        }
        Tab::Git => {
            buttons.push((
                " + Stage ".to_string(),
                AppAction::GitFooter(GitFooterAction::Stage),
                theme::ACCENT_SECONDARY,
            ));
            buttons.push((
                " - Unstage ".to_string(),
                AppAction::GitFooter(GitFooterAction::Unstage),
                theme::ACCENT_TERTIARY,
            ));
            buttons.push((
                " ↩ Discard ".to_string(),
                AppAction::GitFooter(GitFooterAction::Discard),
                theme::BTN_BG,
            ));
            buttons.push((
                " ✎ Commit… ".to_string(),
                AppAction::ToggleCommitDrawer,
                theme::ACCENT_PRIMARY,
            ));
            buttons.push((" ✖ Quit (q) ".to_string(), AppAction::Quit, theme::BTN_BG));
        }
        Tab::Log => {
            buttons.push((" ✖ Quit (q) ".to_string(), AppAction::Quit, theme::BTN_BG));
        }
    }

    for (label, action, color) in buttons {
        let width = label.len() as u16;
        let btn_style = Style::default()
            .bg(color)
            .fg(theme::BTN_FG)
            .add_modifier(Modifier::BOLD);

        f.render_widget(
            Paragraph::new(label.as_str()).style(btn_style),
            Rect::new(btn_x, btn_y, width, 1),
        );

        zones.push(ClickZone {
            rect: Rect::new(btn_x, btn_y, width, 1),
            action,
        });

        btn_x += width + 2;
    }

    if let Some((msg, _)) = app.status_message.as_ref() {
        let used = btn_x.saturating_sub(footer_area.x);
        let available = footer_area.width.saturating_sub(used).saturating_sub(2);
        if available > 0 {
            f.render_widget(
                Paragraph::new(msg.as_str()).style(Style::default().fg(theme::FG)),
                Rect::new(btn_x, btn_y, available, 1),
            );
        }
    }

    if let Some(menu) = &app.context_menu {
        let width = 30;
        let height = menu.options.len() as u16 + 2;

        let area = Rect::new(
            menu.x.min(area.width - width - 1),
            menu.y.min(area.height - height - 1),
            width,
            height,
        );

        f.render_widget(Clear, area);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(theme::ACCENT_SECONDARY))
            .bg(theme::MENU_BG);

        f.render_widget(block.clone(), area);

        let inner = area.inner(Margin {
            vertical: 1,
            horizontal: 1,
        });

        for (i, (label, _)) in menu.options.iter().enumerate() {
            let item_area = Rect::new(inner.x, inner.y + i as u16, inner.width, 1);

            let style = Style::default().fg(theme::FG);

            f.render_widget(Paragraph::new(label.as_str()).style(style), item_area);

            zones.push(ClickZone {
                rect: item_area,
                action: AppAction::ContextMenuAction(i),
            });
        }
    }

    zones
}

fn main() -> io::Result<()> {
    let start_path = env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| env::current_dir().unwrap());

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;

    let picker = if App::is_ssh_session() {
        Picker::halfblocks()
    } else {
        Picker::from_query_stdio().unwrap_or_else(|_| Picker::halfblocks())
    };

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new(start_path, picker);

    loop {
        let mut zones = Vec::new();
        app.maybe_expire_status();
        terminal.draw(|f| {
            zones = draw_ui(f, &mut app);
        })?;
        app.zones = zones;

        if let Some(state) = &mut app.image_state
            && let Some(Err(e)) = state.last_encoding_result()
        {
            app.preview_error = Some(format!("Image Error: {}", e));
            app.image_state = None;
            app.current_image_path = None;
        }

        if event::poll(Duration::from_millis(100))? {
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => match key.code {
                    KeyCode::Char('q') => app.should_quit = true,
                    KeyCode::Char('1') => app.current_tab = Tab::Explorer,
                    KeyCode::Char('2') => {
                        app.current_tab = Tab::Git;
                        app.git.refresh(&app.current_path);
                    }
                    KeyCode::Char('3') => app.current_tab = Tab::Log,
                    KeyCode::Esc => {
                        app.context_menu = None;
                        if app.current_tab == Tab::Git {
                            app.commit_drawer_open = false;
                        }
                    }
                    _ => match app.current_tab {
                        Tab::Explorer => match key.code {
                            KeyCode::Char('h') | KeyCode::Backspace | KeyCode::Left => {
                                app.go_parent()
                            }
                            KeyCode::Char('l') | KeyCode::Enter | KeyCode::Right => {
                                app.enter_selected()
                            }
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
                            _ => {}
                        },
                        Tab::Git => match key.code {
                            KeyCode::Char('r') => app.refresh_git_state(),
                            KeyCode::Char('s') => app.git.diff_mode = GitDiffMode::SideBySide,
                            KeyCode::Char('u') => app.git.diff_mode = GitDiffMode::Unified,
                            KeyCode::Left => {
                                app.git.diff_scroll_x = app.git.diff_scroll_x.saturating_sub(4);
                            }
                            KeyCode::Right => {
                                app.git.diff_scroll_x = app.git.diff_scroll_x.saturating_add(4);
                            }
                            KeyCode::Char('j') | KeyCode::Down => {
                                let i = app.git.list_state.selected().unwrap_or(0);
                                if i + 1 < app.git.filtered.len() {
                                    app.git.select_filtered(i + 1, &app.current_path);
                                }
                            }
                            KeyCode::Char('k') | KeyCode::Up => {
                                let i = app.git.list_state.selected().unwrap_or(0);
                                if i > 0 {
                                    app.git.select_filtered(i - 1, &app.current_path);
                                }
                            }
                            KeyCode::Char('g') => {
                                if !app.git.filtered.is_empty() {
                                    app.git.select_filtered(0, &app.current_path);
                                }
                            }
                            KeyCode::Char('G') => {
                                if !app.git.filtered.is_empty() {
                                    app.git.select_filtered(
                                        app.git.filtered.len() - 1,
                                        &app.current_path,
                                    );
                                }
                            }
                            _ => {}
                        },
                        Tab::Log => {}
                    },
                },
                Event::Mouse(mouse) => match mouse.kind {
                    MouseEventKind::ScrollDown => match app.current_tab {
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
                                    app.list_state
                                        .select(Some(app.files.len().saturating_sub(1)));
                                    app.update_preview();
                                }
                            }
                        }
                        Tab::Git => {
                            if mouse.column >= app.git_diff_x {
                                if mouse.modifiers.contains(KeyModifiers::SHIFT) {
                                    app.git.diff_scroll_x = app.git.diff_scroll_x.saturating_add(4);
                                } else {
                                    app.git.diff_scroll_y = app.git.diff_scroll_y.saturating_add(3);
                                }
                            } else {
                                let i = app.git.list_state.selected().unwrap_or(0);
                                let next = (i + 3).min(app.git.filtered.len().saturating_sub(1));
                                if app.git.filtered.is_empty() {
                                    app.git.list_state.select(None);
                                } else {
                                    app.git.select_filtered(next, &app.current_path);
                                }
                            }
                        }
                        Tab::Log => {}
                    },
                    MouseEventKind::ScrollUp => match app.current_tab {
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
                            if mouse.column >= app.git_diff_x {
                                if mouse.modifiers.contains(KeyModifiers::SHIFT) {
                                    app.git.diff_scroll_x = app.git.diff_scroll_x.saturating_sub(4);
                                } else {
                                    app.git.diff_scroll_y = app.git.diff_scroll_y.saturating_sub(3);
                                }
                            } else {
                                let i = app.git.list_state.selected().unwrap_or(0);
                                if i >= 3 {
                                    app.git.select_filtered(i - 3, &app.current_path);
                                } else if !app.git.filtered.is_empty() {
                                    app.git.select_filtered(0, &app.current_path);
                                }
                            }
                        }
                        Tab::Log => {}
                    },
                    MouseEventKind::Down(MouseButton::Left) => {
                        app.handle_click(mouse.row, mouse.column);
                    }
                    MouseEventKind::Down(MouseButton::Right) => {
                        let mut hit_idx = None;
                        for zone in &app.zones {
                            if let AppAction::Select(idx) = zone.action
                                && mouse.row >= zone.rect.y
                                && mouse.row < zone.rect.y + zone.rect.height
                                && mouse.column >= zone.rect.x
                                && mouse.column < zone.rect.x + zone.rect.width
                            {
                                hit_idx = Some(idx);
                                break;
                            }
                        }

                        if hit_idx.is_some() {
                            app.handle_click(mouse.row, mouse.column);
                            app.open_context_menu(mouse.row, mouse.column, hit_idx);
                        } else {
                            app.open_context_menu(mouse.row, mouse.column, None);
                        }
                    }
                    _ => {}
                },
                _ => {}
            }
        }

        if let Some(text) = app.take_pending_clipboard() {
            let osc52_result = emit_osc52(terminal.backend_mut(), &text);
            let is_ssh = App::is_ssh_session();
            let mut system_result = Ok(());
            if !is_ssh {
                system_result = try_set_system_clipboard(&text);
            }

            match (osc52_result, system_result) {
                (Ok(_), Ok(_)) => {
                    if is_ssh {
                        app.set_status(if in_tmux() {
                            "Copied (OSC52/tmux)"
                        } else {
                            "Copied (OSC52)"
                        });
                    } else {
                        app.set_status("Copied");
                    }
                }
                (Ok(_), Err(e)) => {
                    app.set_status(format!("Copied (OSC52); clipboard error: {}", e));
                }
                (Err(e), Ok(_)) => {
                    app.set_status(format!("Clipboard set; OSC52 error: {}", e));
                }
                (Err(e1), Err(e2)) => {
                    app.set_status(format!("Copy failed: {}; {}", e1, e2));
                }
            }
        }

        if app.should_quit {
            break;
        }
    }

    app.save_persisted_bookmarks();

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    Ok(())
}
