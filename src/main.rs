use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, MouseEventKind, MouseButton},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState},
};
use std::{
    env, fs, io,
    path::PathBuf,
};

// Tokyo Night 配色
mod colors {
    use ratatui::style::Color;
    pub const BORDER: Color = Color::Rgb(122, 162, 247);
    pub const BORDER_DIM: Color = Color::Rgb(59, 66, 97);
    pub const TITLE: Color = Color::Rgb(125, 207, 255);
    pub const SELECTED_BG: Color = Color::Rgb(122, 162, 247);
    pub const SELECTED_FG: Color = Color::Rgb(26, 27, 38);
    pub const FILE: Color = Color::Rgb(192, 202, 245);
    pub const DIR: Color = Color::Rgb(122, 162, 247);
    pub const SYMLINK: Color = Color::Rgb(187, 154, 247);
    pub const EXEC: Color = Color::Rgb(158, 206, 106);
    pub const HIDDEN: Color = Color::Rgb(86, 95, 137);
    pub const SIZE: Color = Color::Rgb(86, 95, 137);
    pub const PATH: Color = Color::Rgb(224, 175, 104);
    pub const HELP_KEY: Color = Color::Rgb(158, 206, 106);
    pub const HELP: Color = Color::Rgb(86, 95, 137);
    pub const PREVIEW: Color = Color::Rgb(169, 177, 214);
}

#[derive(Clone)]
struct FileEntry {
    name: String,
    is_dir: bool,
    is_symlink: bool,
    is_exec: bool,
    is_hidden: bool,
    size: u64,
}

struct App {
    current_path: PathBuf,
    files: Vec<FileEntry>,
    list_state: ListState,
    preview_scroll: u16,
    should_quit: bool,
}

impl App {
    fn new(path: PathBuf) -> Self {
        let mut app = Self {
            current_path: path,
            files: Vec::new(),
            list_state: ListState::default(),
            preview_scroll: 0,
            should_quit: false,
        };
        app.load_files();
        if !app.files.is_empty() {
            app.list_state.select(Some(0));
        }
        app
    }

    fn load_files(&mut self) {
        self.files.clear();
        if let Ok(entries) = fs::read_dir(&self.current_path) {
            let mut items: Vec<FileEntry> = entries
                .filter_map(|e| e.ok())
                .map(|entry| {
                    let name = entry.file_name().to_string_lossy().to_string();
                    let metadata = entry.metadata().ok();
                    let file_type = entry.file_type().ok();
                    FileEntry {
                        is_hidden: name.starts_with('.'),
                        is_dir: file_type.map(|t| t.is_dir()).unwrap_or(false),
                        is_symlink: file_type.map(|t| t.is_symlink()).unwrap_or(false),
                        is_exec: metadata
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
                            .unwrap_or(false),
                        size: metadata.map(|m| m.len()).unwrap_or(0),
                        name,
                    }
                })
                .collect();

            items.sort_by(|a, b| {
                match (a.is_dir, b.is_dir) {
                    (true, false) => std::cmp::Ordering::Less,
                    (false, true) => std::cmp::Ordering::Greater,
                    _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
                }
            });

            self.files = items;
        }
        self.preview_scroll = 0;
    }

    fn selected_index(&self) -> Option<usize> {
        self.list_state.selected()
    }

    fn selected_file(&self) -> Option<&FileEntry> {
        self.selected_index().and_then(|i| self.files.get(i))
    }

    fn selected_path(&self) -> Option<PathBuf> {
        self.selected_file().map(|f| self.current_path.join(&f.name))
    }

    fn move_selection(&mut self, delta: i32) {
        if self.files.is_empty() {
            return;
        }
        let current = self.selected_index().unwrap_or(0) as i32;
        let new_index = (current + delta).clamp(0, self.files.len() as i32 - 1) as usize;
        self.list_state.select(Some(new_index));
        self.preview_scroll = 0;
    }

    fn select_index(&mut self, index: usize) {
        if index < self.files.len() {
            self.list_state.select(Some(index));
            self.preview_scroll = 0;
        }
    }

    fn enter_selected(&mut self) {
        if let Some(file) = self.selected_file() {
            if file.is_dir {
                let new_path = self.current_path.join(&file.name);
                if let Ok(canonical) = new_path.canonicalize() {
                    self.current_path = canonical;
                    self.load_files();
                    self.list_state.select(if self.files.is_empty() { None } else { Some(0) });
                }
            }
        }
    }

    fn go_parent(&mut self) {
        if let Some(parent) = self.current_path.parent() {
            let old_name = self.current_path.file_name().map(|n| n.to_string_lossy().to_string());
            self.current_path = parent.to_path_buf();
            self.load_files();
            if let Some(name) = old_name {
                if let Some(idx) = self.files.iter().position(|f| f.name == name) {
                    self.list_state.select(Some(idx));
                }
            }
        }
    }

    fn get_preview(&self) -> Vec<String> {
        let Some(path) = self.selected_path() else {
            return vec![];
        };

        if let Some(file) = self.selected_file() {
            if file.is_dir {
                if let Ok(entries) = fs::read_dir(&path) {
                    let mut items: Vec<String> = entries
                        .filter_map(|e| e.ok())
                        .map(|e| {
                            let name = e.file_name().to_string_lossy().to_string();
                            let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
                            if is_dir {
                                format!(" {}", name)
                            } else {
                                format!(" {}", name)
                            }
                        })
                        .collect();
                    items.sort();
                    return items;
                }
            } else {
                if file.size > 100_000 {
                    return vec!["File too large to preview".to_string()];
                }
                if let Ok(content) = fs::read_to_string(&path) {
                    return content.lines().map(String::from).collect();
                } else {
                    return vec!["Binary file".to_string()];
                }
            }
        }
        vec![]
    }
}

fn get_icon(file: &FileEntry) -> &'static str {
    if file.is_dir { " " }
    else if file.is_symlink { " " }
    else {
        match file.name.rsplit('.').next() {
            Some("rs") => " ",
            Some("ts" | "tsx") => " ",
            Some("js" | "jsx") => " ",
            Some("py") => " ",
            Some("go") => " ",
            Some("json") => " ",
            Some("toml" | "yaml" | "yml") => " ",
            Some("md") => " ",
            Some("sh" | "bash") => " ",
            Some("lock") => " ",
            _ => " ",
        }
    }
}

fn get_color(file: &FileEntry) -> Color {
    if file.is_dir { colors::DIR }
    else if file.is_symlink { colors::SYMLINK }
    else if file.is_hidden { colors::HIDDEN }
    else if file.is_exec { colors::EXEC }
    else { colors::FILE }
}

fn format_size(size: u64) -> String {
    if size < 1024 { format!("{}B", size) }
    else if size < 1024 * 1024 { format!("{:.1}K", size as f64 / 1024.0) }
    else if size < 1024 * 1024 * 1024 { format!("{:.1}M", size as f64 / (1024.0 * 1024.0)) }
    else { format!("{:.1}G", size as f64 / (1024.0 * 1024.0 * 1024.0)) }
}

fn ui(frame: &mut Frame, app: &mut App) {
    let area = frame.area();

    let main_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(0),
            Constraint::Length(2),
        ])
        .split(area);

    // 标题栏
    let title = Paragraph::new(Line::from(vec![
        Span::styled("  Explorer ", Style::default().fg(colors::TITLE).bold()),
        Span::styled(app.current_path.to_string_lossy(), Style::default().fg(colors::PATH)),
    ]));
    frame.render_widget(title, main_layout[0]);

    // 主体分栏
    let content_layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
        .split(main_layout[1]);

    // 文件列表
    let items: Vec<ListItem> = app
        .files
        .iter()
        .map(|file| {
            let icon = get_icon(file);
            let size_str = if file.is_dir {
                "     ".to_string()
            } else {
                format!("{:>6}", format_size(file.size))
            };
            let content = Line::from(vec![
                Span::raw(icon),
                Span::raw(&file.name),
                Span::styled(format!(" {}", size_str), Style::default().fg(colors::SIZE)),
            ]);
            ListItem::new(content).style(Style::default().fg(get_color(file)))
        })
        .collect();

    let file_list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(ratatui::widgets::BorderType::Rounded)
                .border_style(Style::default().fg(colors::BORDER))
                .title(Span::styled(
                    format!(" Files ({}) ", app.files.len()),
                    Style::default().fg(colors::TITLE),
                )),
        )
        .highlight_style(
            Style::default()
                .bg(colors::SELECTED_BG)
                .fg(colors::SELECTED_FG)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol(" ▸ ");

    frame.render_stateful_widget(file_list, content_layout[0], &mut app.list_state);

    // 滚动条
    let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
        .begin_symbol(Some("▲"))
        .end_symbol(Some("▼"));
    let mut scrollbar_state = ScrollbarState::new(app.files.len())
        .position(app.selected_index().unwrap_or(0));
    frame.render_stateful_widget(
        scrollbar,
        content_layout[0].inner(Margin { vertical: 1, horizontal: 0 }),
        &mut scrollbar_state,
    );

    // 预览面板
    let preview_lines = app.get_preview();
    let preview_height = content_layout[1].height.saturating_sub(2) as usize;
    let visible_preview: Vec<Line> = preview_lines
        .iter()
        .skip(app.preview_scroll as usize)
        .take(preview_height)
        .map(|line| Line::from(Span::styled(line.as_str(), Style::default().fg(colors::PREVIEW))))
        .collect();

    let preview = Paragraph::new(visible_preview)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(ratatui::widgets::BorderType::Rounded)
                .border_style(Style::default().fg(colors::BORDER_DIM))
                .title(Span::styled(" Preview ", Style::default().fg(colors::TITLE))),
        );
    frame.render_widget(preview, content_layout[1]);

    // 帮助栏
    let help = Line::from(vec![
        Span::styled(" ↑↓/jk", Style::default().fg(colors::HELP_KEY)),
        Span::styled(" nav  ", Style::default().fg(colors::HELP)),
        Span::styled("Enter/l", Style::default().fg(colors::HELP_KEY)),
        Span::styled(" open  ", Style::default().fg(colors::HELP)),
        Span::styled("Backspace/h", Style::default().fg(colors::HELP_KEY)),
        Span::styled(" back  ", Style::default().fg(colors::HELP)),
        Span::styled("g/G", Style::default().fg(colors::HELP_KEY)),
        Span::styled(" top/end  ", Style::default().fg(colors::HELP)),
        Span::styled("q", Style::default().fg(colors::HELP_KEY)),
        Span::styled(" quit", Style::default().fg(colors::HELP)),
    ]);
    frame.render_widget(Paragraph::new(help), main_layout[2]);
}

fn main() -> io::Result<()> {
    let start_path = env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| env::current_dir().unwrap_or_default());

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new(start_path);

    loop {
        terminal.draw(|f| ui(f, &mut app))?;

        match event::read()? {
            Event::Key(key) if key.kind == KeyEventKind::Press => {
                match key.code {
                    KeyCode::Char('q') => app.should_quit = true,
                    KeyCode::Up | KeyCode::Char('k') => app.move_selection(-1),
                    KeyCode::Down | KeyCode::Char('j') => app.move_selection(1),
                    KeyCode::Enter | KeyCode::Char('l') => app.enter_selected(),
                    KeyCode::Backspace | KeyCode::Char('h') => app.go_parent(),
                    KeyCode::Char('g') => app.select_index(0),
                    KeyCode::Char('G') => app.select_index(app.files.len().saturating_sub(1)),
                    KeyCode::PageUp => app.move_selection(-10),
                    KeyCode::PageDown => app.move_selection(10),
                    _ => {}
                }
            }
            Event::Mouse(mouse) => {
                match mouse.kind {
                    MouseEventKind::ScrollUp => app.move_selection(-3),
                    MouseEventKind::ScrollDown => app.move_selection(3),
                    MouseEventKind::Down(MouseButton::Left) => {
                        let list_top = 3u16;
                        let list_height = terminal.size()?.height.saturating_sub(6);
                        if mouse.row >= list_top && mouse.row < list_top + list_height {
                            let offset = app.list_state.offset();
                            let clicked = offset + (mouse.row - list_top) as usize;
                            if clicked < app.files.len() {
                                app.select_index(clicked);
                            }
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }

        if app.should_quit {
            break;
        }
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture)?;
    Ok(())
}
