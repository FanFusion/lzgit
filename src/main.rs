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
use serde::{Deserialize, Serialize};
use std::{
    cmp::Ordering,
    collections::VecDeque,
    env,
    fs::{self},
    io::{self, Write},
    path::PathBuf,
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

mod branch;
mod commit;
mod conflict;
mod git;
mod git_ops;
mod highlight;
mod openrouter;

use branch::BranchUi;
use commit::{CommitFocus, CommitState};
use conflict::{ConflictFile, ConflictResolution};
use git::{
    GitDiffCellKind, GitDiffMode, GitDiffRow, GitSection, GitState, build_side_by_side_rows,
    pad_to_width, render_side_by_side_cell,
};
use highlight::{Highlighter, new_highlighter};

mod theme {
    use ratatui::style::Color;
    use serde::{Deserialize, Serialize};

    #[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "kebab-case")]
    pub enum Theme {
        Mocha,
        TokyoNightStorm,
        GruvboxDarkHard,
        Nord,
        Dracula,
    }

    impl Theme {
        pub fn label(self) -> &'static str {
            match self {
                Theme::Mocha => "Mocha",
                Theme::TokyoNightStorm => "Tokyo Night",
                Theme::GruvboxDarkHard => "Gruvbox",
                Theme::Nord => "Nord",
                Theme::Dracula => "Dracula",
            }
        }
    }

    #[derive(Clone, Copy, Debug)]
    pub struct Palette {
        pub bg: Color,
        pub fg: Color,
        pub accent_primary: Color,
        pub accent_secondary: Color,
        pub accent_tertiary: Color,
        pub border_inactive: Color,
        pub selection_bg: Color,
        pub dir_color: Color,
        pub exe_color: Color,
        pub size_color: Color,
        pub btn_bg: Color,
        pub btn_fg: Color,
        pub menu_bg: Color,
        pub diff_add_bg: Color,
        pub diff_del_bg: Color,
        pub diff_hunk_bg: Color,
    }

    pub fn palette(theme: Theme) -> Palette {
        match theme {
            Theme::Mocha => Palette {
                bg: Color::Rgb(30, 30, 46),
                fg: Color::Rgb(205, 214, 244),
                accent_primary: Color::Rgb(203, 166, 247),
                accent_secondary: Color::Rgb(250, 179, 135),
                accent_tertiary: Color::Rgb(137, 180, 250),
                border_inactive: Color::Rgb(88, 91, 112),
                selection_bg: Color::Rgb(69, 71, 90),
                dir_color: Color::Rgb(137, 180, 250),
                exe_color: Color::Rgb(166, 227, 161),
                size_color: Color::Rgb(147, 153, 178),
                btn_bg: Color::Rgb(243, 139, 168),
                btn_fg: Color::Rgb(24, 24, 37),
                menu_bg: Color::Rgb(49, 50, 68),
                diff_add_bg: Color::Rgb(72, 104, 88),
                diff_del_bg: Color::Rgb(110, 70, 92),
                diff_hunk_bg: Color::Rgb(74, 78, 116),
            },
            Theme::TokyoNightStorm => Palette {
                bg: Color::Rgb(36, 40, 59),
                fg: Color::Rgb(192, 202, 245),
                accent_primary: Color::Rgb(122, 162, 247),
                accent_secondary: Color::Rgb(255, 158, 100),
                accent_tertiary: Color::Rgb(187, 154, 247),
                border_inactive: Color::Rgb(65, 72, 104),
                selection_bg: Color::Rgb(46, 60, 100),
                dir_color: Color::Rgb(122, 162, 247),
                exe_color: Color::Rgb(158, 206, 106),
                size_color: Color::Rgb(86, 95, 137),
                btn_bg: Color::Rgb(247, 118, 142),
                btn_fg: Color::Rgb(24, 24, 37),
                menu_bg: Color::Rgb(45, 49, 71),
                diff_add_bg: Color::Rgb(56, 83, 76),
                diff_del_bg: Color::Rgb(90, 60, 75),
                diff_hunk_bg: Color::Rgb(60, 65, 100),
            },
            Theme::GruvboxDarkHard => Palette {
                bg: Color::Rgb(29, 32, 33),
                fg: Color::Rgb(235, 219, 178),
                accent_primary: Color::Rgb(250, 189, 47),
                accent_secondary: Color::Rgb(214, 93, 14),
                accent_tertiary: Color::Rgb(131, 165, 152),
                border_inactive: Color::Rgb(80, 73, 69),
                selection_bg: Color::Rgb(60, 56, 54),
                dir_color: Color::Rgb(131, 165, 152),
                exe_color: Color::Rgb(184, 187, 38),
                size_color: Color::Rgb(146, 131, 116),
                btn_bg: Color::Rgb(251, 73, 52),
                btn_fg: Color::Rgb(29, 32, 33),
                menu_bg: Color::Rgb(50, 48, 47),
                diff_add_bg: Color::Rgb(54, 69, 54),
                diff_del_bg: Color::Rgb(78, 53, 53),
                diff_hunk_bg: Color::Rgb(69, 64, 74),
            },
            Theme::Nord => Palette {
                bg: Color::Rgb(46, 52, 64),
                fg: Color::Rgb(216, 222, 233),
                accent_primary: Color::Rgb(136, 192, 208),
                accent_secondary: Color::Rgb(235, 203, 139),
                accent_tertiary: Color::Rgb(180, 142, 173),
                border_inactive: Color::Rgb(76, 86, 106),
                selection_bg: Color::Rgb(67, 76, 94),
                dir_color: Color::Rgb(129, 161, 193),
                exe_color: Color::Rgb(163, 190, 140),
                size_color: Color::Rgb(76, 86, 106),
                btn_bg: Color::Rgb(191, 97, 106),
                btn_fg: Color::Rgb(46, 52, 64),
                menu_bg: Color::Rgb(59, 66, 82),
                diff_add_bg: Color::Rgb(58, 76, 74),
                diff_del_bg: Color::Rgb(87, 63, 72),
                diff_hunk_bg: Color::Rgb(67, 76, 94),
            },
            Theme::Dracula => Palette {
                bg: Color::Rgb(40, 42, 54),
                fg: Color::Rgb(248, 248, 242),
                accent_primary: Color::Rgb(189, 147, 249),
                accent_secondary: Color::Rgb(139, 233, 253),
                accent_tertiary: Color::Rgb(255, 121, 198),
                border_inactive: Color::Rgb(98, 114, 164),
                selection_bg: Color::Rgb(68, 71, 90),
                dir_color: Color::Rgb(139, 233, 253),
                exe_color: Color::Rgb(80, 250, 123),
                size_color: Color::Rgb(98, 114, 164),
                btn_bg: Color::Rgb(255, 85, 85),
                btn_fg: Color::Rgb(40, 42, 54),
                menu_bg: Color::Rgb(68, 71, 90),
                diff_add_bg: Color::Rgb(60, 92, 72),
                diff_del_bg: Color::Rgb(92, 60, 72),
                diff_hunk_bg: Color::Rgb(65, 68, 96),
            },
        }
    }
}

const THEME_ORDER: [theme::Theme; 5] = [
    theme::Theme::Mocha,
    theme::Theme::TokyoNightStorm,
    theme::Theme::GruvboxDarkHard,
    theme::Theme::Nord,
    theme::Theme::Dracula,
];

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
    OpenCommandPalette,
    Navigate(PathBuf),
    EnterDir,
    GoParent,
    Select(usize),
    SelectGitSection(GitSection),
    SelectGitFile(usize),
    ToggleCommitDrawer,
    FocusCommitMessage,
    GenerateCommitMessage,
    ConfirmDiscard,
    CancelDiscard,
    ClearGitLog,
    LogSwitch(LogSubTab),
    LogDetail(LogDetailMode),
    LogToggleZoom,
    LogInspect,
    LogCloseInspect,
    LogInspectCopyPrimary,
    LogInspectCopySecondary,
    LogFocusDiff,
    LogFocusFiles,
    LogAdjustLeft(i16),
    SelectLogItem(usize),
    SelectLogFile(usize),

    CloseOperationPopup,
    MergeContinue,
    MergeAbort,
    RebaseContinue,
    RebaseAbort,
    RebaseSkip,
    ConflictPrev,
    ConflictNext,
    ConflictUseOurs,
    ConflictUseTheirs,
    ConflictUseBoth,
    MarkResolved,
    OpenBranchPicker,
    CloseBranchPicker,
    SelectBranch(usize),
    BranchCheckout,
    ConfirmBranchCheckout,
    CancelBranchCheckout,
    GitFetch,
    GitPullRebase,
    GitPush,
    ToggleGitStage,
    GitStageAllVisible,
    GitUnstageAllVisible,
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
    selected: usize,
    options: Vec<(String, ContextCommand)>,
}

#[derive(Clone)]
enum ContextCommand {
    AddBookmark,
    RemoveBookmark,
    CopyPath,
    CopyRelPath,
    Rename,

    GitStage,
    GitUnstage,
    GitToggleStage,
    GitDiscard,
    GitStageAll,
    GitUnstageAll,
    GitOpenInExplorer,
    GitCopyPath,
    GitCopyRelPath,
    GitAddToGitignore,

    LogCopySha,
    LogCheckoutDetached,
    LogCopySubject,
    LogCopyCommand,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DiscardMode {
    Worktree,
    Untracked,
    AllChanges,
}

#[derive(Clone, Debug)]
struct DiscardItem {
    path: String,
    mode: DiscardMode,
}

#[derive(Clone, Debug)]
struct DiscardConfirm {
    items: Vec<DiscardItem>,
}

#[derive(Clone, Debug)]
struct GitLogEntry {
    when: Instant,
    cmd: String,
    ok: bool,
    detail: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct PersistedUiSettings {
    #[serde(default)]
    log_left_width: Option<u16>,
    #[serde(default)]
    theme: Option<theme::Theme>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GitOperation {
    Merge,
    Rebase,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LogSubTab {
    History,
    Reflog,
    Commands,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LogDetailMode {
    Diff,
    Files,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LogPaneFocus {
    Commits,
    Files,
    Diff,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LogZoom {
    None,
    List,
    Diff,
}

struct InspectUi {
    open: bool,
    title: String,
    body: String,
    scroll_y: u16,
}

impl InspectUi {
    fn new() -> Self {
        Self {
            open: false,
            title: String::new(),
            body: String::new(),
            scroll_y: 0,
        }
    }

    fn close(&mut self) {
        self.open = false;
        self.title.clear();
        self.body.clear();
        self.scroll_y = 0;
    }
}

struct LogUi {
    subtab: LogSubTab,
    detail_mode: LogDetailMode,
    focus: LogPaneFocus,
    zoom: LogZoom,

    diff_mode: GitDiffMode,

    left_width: u16,

    history: Vec<git_ops::CommitEntry>,
    reflog: Vec<git_ops::ReflogEntry>,
    files: Vec<git_ops::CommitFileChange>,
    files_hash: Option<String>,

    history_state: ListState,
    reflog_state: ListState,
    command_state: ListState,
    files_state: ListState,

    diff_lines: Vec<String>,
    diff_scroll_y: u16,
    diff_scroll_x: u16,

    inspect: InspectUi,
    status: Option<String>,
}

impl LogUi {
    fn new() -> Self {
        Self {
            subtab: LogSubTab::History,
            detail_mode: LogDetailMode::Diff,
            focus: LogPaneFocus::Commits,
            zoom: LogZoom::None,

            diff_mode: GitDiffMode::Unified,

            left_width: 56,

            history: Vec::new(),
            reflog: Vec::new(),
            files: Vec::new(),
            files_hash: None,

            history_state: ListState::default(),
            reflog_state: ListState::default(),
            command_state: ListState::default(),
            files_state: ListState::default(),

            diff_lines: Vec::new(),
            diff_scroll_y: 0,
            diff_scroll_x: 0,

            inspect: InspectUi::new(),
            status: None,
        }
    }

    fn set_subtab(&mut self, subtab: LogSubTab) {
        if self.subtab == subtab {
            return;
        }

        self.subtab = subtab;
        self.focus = LogPaneFocus::Commits;
        self.diff_scroll_y = 0;
        self.diff_scroll_x = 0;

        match self.subtab {
            LogSubTab::History => {}
            LogSubTab::Reflog => {}
            LogSubTab::Commands => {
                self.command_state.select(Some(0));
            }
        }
    }

    fn set_detail_mode(&mut self, mode: LogDetailMode) {
        if self.detail_mode == mode {
            return;
        }
        self.detail_mode = mode;
        self.diff_scroll_y = 0;
        self.diff_scroll_x = 0;

        match mode {
            LogDetailMode::Files if self.subtab != LogSubTab::Commands => {
                self.focus = LogPaneFocus::Files;
            }
            LogDetailMode::Diff => {
                self.focus = LogPaneFocus::Diff;
            }
            _ => {}
        }

        if mode != LogDetailMode::Files {
            self.files.clear();
            self.files_hash = None;
            self.files_state.select(None);
        }
    }

    fn active_state(&self) -> &ListState {
        match self.subtab {
            LogSubTab::History => &self.history_state,
            LogSubTab::Reflog => &self.reflog_state,
            LogSubTab::Commands => &self.command_state,
        }
    }

    fn active_state_mut(&mut self) -> &mut ListState {
        match self.subtab {
            LogSubTab::History => &mut self.history_state,
            LogSubTab::Reflog => &mut self.reflog_state,
            LogSubTab::Commands => &mut self.command_state,
        }
    }
}

struct OperationPopup {
    title: String,
    body: String,
    ok: bool,
    scroll_y: u16,
}

impl OperationPopup {
    fn new(title: String, body: String, ok: bool) -> Self {
        Self {
            title,
            body,
            ok,
            scroll_y: 0,
        }
    }
}

struct ThemePickerUi {
    open: bool,
    list_state: ListState,
}

impl ThemePickerUi {
    fn new() -> Self {
        Self {
            open: false,
            list_state: ListState::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CommandId {
    ToggleHidden,
    ToggleWrapDiff,
    ToggleSyntaxHighlight,
    SelectTheme,
    RefreshGit,
    GitFetch,
    GitPullRebase,
    GitPush,
    OpenBranchPicker,
    ClearGitLog,
    Quit,
}

const COMMAND_PALETTE_ITEMS: &[(CommandId, &str)] = &[
    (CommandId::ToggleHidden, "Toggle hidden files"),
    (CommandId::ToggleWrapDiff, "Toggle diff wrap"),
    (CommandId::ToggleSyntaxHighlight, "Toggle syntax highlight"),
    (CommandId::SelectTheme, "Select theme…"),
    (CommandId::RefreshGit, "Git: refresh status"),
    (CommandId::OpenBranchPicker, "Checkout branch…"),
    (CommandId::GitFetch, "Git: fetch --prune"),
    (CommandId::GitPullRebase, "Git: pull --rebase"),
    (CommandId::GitPush, "Git: push"),
    (CommandId::ClearGitLog, "Clear git command log"),
    (CommandId::Quit, "Quit"),
];

struct CommandPaletteUi {
    open: bool,
    list_state: ListState,
}

impl CommandPaletteUi {
    fn new() -> Self {
        Self {
            open: false,
            list_state: ListState::default(),
        }
    }
}

enum JobResult {
    Git {
        cmd: String,
        result: Result<(), String>,
        refresh: bool,
        close_commit: bool,
    },
    Ai {
        result: Result<String, String>,
    },
}

struct PendingJob {
    rx: mpsc::Receiver<JobResult>,
}

struct ConflictUi {
    path: Option<String>,
    file: Option<ConflictFile>,
    selected_block: usize,
    scroll_y: u16,
}

impl ConflictUi {
    fn new() -> Self {
        Self {
            path: None,
            file: None,
            selected_block: 0,
            scroll_y: 0,
        }
    }

    fn reset(&mut self) {
        self.path = None;
        self.file = None;
        self.selected_block = 0;
        self.scroll_y = 0;
    }
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
    git_operation: Option<GitOperation>,
    branch_ui: BranchUi,
    conflict_ui: ConflictUi,
    commit: CommitState,
    pending_job: Option<PendingJob>,
    discard_confirm: Option<DiscardConfirm>,
    operation_popup: Option<OperationPopup>,
    theme_picker: ThemePickerUi,
    command_palette: CommandPaletteUi,
    git_log: VecDeque<GitLogEntry>,
    log_ui: LogUi,

    wrap_diff: bool,
    syntax_highlight: bool,

    theme: theme::Theme,
    palette: theme::Palette,

    explorer_preview_x: u16,
    git_diff_x: u16,
    log_files_x: u16,
    log_diff_x: u16,

    zones: Vec<ClickZone>,
    last_click: Option<(Instant, usize)>,
    bookmarks: Vec<(String, PathBuf)>,

    context_menu: Option<ContextMenu>,
    pending_menu_action: Option<(usize, bool)>,

    picker: Picker,
    image_state: Option<StatefulProtocol>,
    current_image_path: Option<PathBuf>,
    preview_error: Option<String>,
    status_message: Option<(String, Instant)>,
    status_ttl: Duration,

    pending_clipboard: Option<String>,
    bookmarks_path: Option<PathBuf>,
    ui_settings_path: Option<PathBuf>,
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
            git_operation: None,
            branch_ui: BranchUi::new(),
            conflict_ui: ConflictUi::new(),
            commit: CommitState::new(),
            pending_job: None,
            discard_confirm: None,
            operation_popup: None,
            theme_picker: ThemePickerUi::new(),
            command_palette: CommandPaletteUi::new(),
            git_log: VecDeque::new(),
            log_ui: LogUi::new(),

            wrap_diff: true,
            syntax_highlight: true,

            theme: theme::Theme::Mocha,
            palette: theme::palette(theme::Theme::Mocha),

            explorer_preview_x: 0,
            git_diff_x: 0,
            log_files_x: 0,
            log_diff_x: 0,

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
            pending_menu_action: None,
            picker,
            image_state: None,
            current_image_path: None,
            preview_error: None,
            status_message: None,
            status_ttl: Duration::from_secs(2),
            pending_clipboard: None,
            bookmarks_path: bookmarks_file_path(),
            ui_settings_path: ui_settings_file_path(),
        };
        app.load_persisted_bookmarks();
        app.load_persisted_ui_settings();
        app.load_files();
        if !app.files.is_empty() {
            app.list_state.select(Some(0));
            app.update_preview();
        }
        app.git.refresh(&app.current_path);
        app.update_git_operation();
        app
    }

    fn refresh_git_state(&mut self) {
        self.git.refresh(&self.current_path);
        self.update_git_operation();
        self.conflict_ui.reset();
    }

    fn update_git_operation(&mut self) {
        self.git_operation = None;
        let Some(repo_root) = self.git.repo_root.clone() else {
            return;
        };

        if git_ops::rebase_in_progress(&repo_root).unwrap_or(false) {
            self.git_operation = Some(GitOperation::Rebase);
            return;
        }

        if git_ops::merge_head_exists(&repo_root).unwrap_or(false) {
            self.git_operation = Some(GitOperation::Merge);
        }
    }

    fn open_branch_picker(&mut self) {
        self.context_menu = None;
        self.commit.open = false;

        let Some(repo_root) = self.git.repo_root.clone() else {
            self.set_status("Not a git repository");
            return;
        };

        match git_ops::list_branches(&repo_root) {
            Ok(branches) => {
                self.branch_ui.open = true;
                self.branch_ui.query.clear();
                self.branch_ui.confirm_checkout = None;
                self.branch_ui.status = None;
                self.branch_ui.set_branches(branches);
            }
            Err(e) => {
                self.set_status(e);
            }
        }
    }

    fn close_branch_picker(&mut self) {
        self.branch_ui.open = false;
        self.branch_ui.query.clear();
        self.branch_ui.filtered.clear();
        self.branch_ui.branches.clear();
        self.branch_ui.confirm_checkout = None;
        self.branch_ui.status = None;
        self.branch_ui.list_state.select(None);
    }

    fn branch_checkout_selected(&mut self, force: bool) {
        let Some(repo_root) = self.git.repo_root.clone() else {
            self.branch_ui.status = Some("Not a git repository".to_string());
            return;
        };

        let Some(branch) = self.branch_ui.selected_branch() else {
            self.branch_ui.status = Some("No branch selected".to_string());
            return;
        };
        let name = branch.name.clone();

        if !force {
            match git_ops::is_dirty(&repo_root) {
                Ok(true) => {
                    self.branch_ui.confirm_checkout = Some(name);
                    return;
                }
                Ok(false) => {}
                Err(e) => {
                    self.branch_ui.status = Some(e);
                    return;
                }
            }
        }

        let cmd = if branch.is_remote {
            format!("git checkout --track {}", name)
        } else {
            format!("git checkout {}", name)
        };
        self.start_git_job(cmd, true, false, move || {
            git_ops::checkout_branch_entry(&repo_root, &branch)
        });
        self.close_branch_picker();
    }

    fn ensure_conflicts_loaded(&mut self) {
        let Some(entry) = self.git.selected_entry() else {
            self.conflict_ui.reset();
            return;
        };

        if !entry.is_conflict {
            self.conflict_ui.reset();
            return;
        }

        if self.conflict_ui.path.as_deref() == Some(entry.path.as_str())
            && self.conflict_ui.file.is_some()
        {
            return;
        }

        let Some(repo_root) = self.git.repo_root.clone() else {
            self.conflict_ui.reset();
            return;
        };

        let abs = repo_root.join(&entry.path);
        match conflict::load_conflicts(&abs) {
            Ok(file) => {
                self.conflict_ui.path = Some(entry.path.clone());
                self.conflict_ui.file = Some(file);
                self.conflict_ui.selected_block = 0;
                self.conflict_ui.scroll_y = 0;
            }
            Err(e) => {
                self.conflict_ui.path = Some(entry.path.clone());
                self.conflict_ui.file = None;
                self.conflict_ui.selected_block = 0;
                self.conflict_ui.scroll_y = 0;
                self.set_status(e);
            }
        }
    }

    fn push_git_log(&mut self, cmd: String, result: &Result<(), String>) {
        let ok = result.is_ok();
        let detail = result.as_ref().err().cloned();
        self.git_log.push_front(GitLogEntry {
            when: Instant::now(),
            cmd,
            ok,
            detail,
        });
        while self.git_log.len() > 200 {
            self.git_log.pop_back();
        }

        if self.log_ui.subtab == LogSubTab::Commands
            && self.log_ui.command_state.selected().is_none()
        {
            self.log_ui.command_state.select(Some(0));
            self.refresh_log_diff();
        }
    }

    fn refresh_log_data(&mut self) {
        self.log_ui.status = None;

        let Some(repo_root) = self.git.repo_root.clone() else {
            self.log_ui.history.clear();
            self.log_ui.reflog.clear();
            self.log_ui.history_state.select(None);
            self.log_ui.reflog_state.select(None);
            self.refresh_log_diff();
            return;
        };

        match git_ops::list_history(&repo_root, 200) {
            Ok(items) => {
                self.log_ui.history = items;
                if self.log_ui.history.is_empty() {
                    self.log_ui.history_state.select(None);
                } else if self
                    .log_ui
                    .history_state
                    .selected()
                    .map(|i| i >= self.log_ui.history.len())
                    .unwrap_or(true)
                {
                    self.log_ui.history_state.select(Some(0));
                }
            }
            Err(e) => {
                self.log_ui.status = Some(e);
                self.log_ui.history.clear();
                self.log_ui.history_state.select(None);
            }
        }

        match git_ops::list_reflog(&repo_root, 200) {
            Ok(items) => {
                self.log_ui.reflog = items;
                if self.log_ui.reflog.is_empty() {
                    self.log_ui.reflog_state.select(None);
                } else if self
                    .log_ui
                    .reflog_state
                    .selected()
                    .map(|i| i >= self.log_ui.reflog.len())
                    .unwrap_or(true)
                {
                    self.log_ui.reflog_state.select(Some(0));
                }
            }
            Err(e) => {
                self.log_ui.status = Some(e);
                self.log_ui.reflog.clear();
                self.log_ui.reflog_state.select(None);
            }
        }

        self.refresh_log_diff();
    }

    fn refresh_log_diff(&mut self) {
        self.log_ui.diff_lines.clear();

        match self.log_ui.subtab {
            LogSubTab::History => {
                let Some(repo_root) = self.git.repo_root.clone() else {
                    self.log_ui
                        .diff_lines
                        .push("Not a git repository".to_string());
                    return;
                };
                let Some(sel) = self.log_ui.history_state.selected() else {
                    self.log_ui.diff_lines.push("No commits".to_string());
                    return;
                };
                let Some(entry) = self.log_ui.history.get(sel) else {
                    return;
                };
                let hash = entry.hash.clone();

                self.refresh_log_commit_view(&repo_root, &hash);
            }
            LogSubTab::Reflog => {
                let Some(repo_root) = self.git.repo_root.clone() else {
                    self.log_ui
                        .diff_lines
                        .push("Not a git repository".to_string());
                    return;
                };
                let Some(sel) = self.log_ui.reflog_state.selected() else {
                    self.log_ui.diff_lines.push("No reflog entries".to_string());
                    return;
                };
                let Some(entry) = self.log_ui.reflog.get(sel) else {
                    return;
                };
                let hash = entry.hash.clone();

                self.refresh_log_commit_view(&repo_root, &hash);
            }
            LogSubTab::Commands => {
                let Some(sel) = self.log_ui.command_state.selected() else {
                    self.log_ui.diff_lines.push("No commands".to_string());
                    return;
                };
                let Some(entry) = self.git_log.get(sel) else {
                    return;
                };
                self.log_ui
                    .diff_lines
                    .push(format!("Command: {}", entry.cmd));
                self.log_ui
                    .diff_lines
                    .push(format!("Result: {}", if entry.ok { "OK" } else { "Error" }));
                self.log_ui.diff_lines.push(String::new());
                if let Some(detail) = entry.detail.as_deref() {
                    if detail.trim().is_empty() {
                        self.log_ui.diff_lines.push("(no output)".to_string());
                    } else {
                        self.log_ui
                            .diff_lines
                            .extend(detail.lines().map(|l| l.to_string()));
                    }
                } else {
                    self.log_ui.diff_lines.push("(no output)".to_string());
                }
            }
        }
    }

    fn refresh_log_commit_view(&mut self, repo_root: &PathBuf, hash: &str) {
        match self.log_ui.detail_mode {
            LogDetailMode::Diff => match git_ops::show_commit(repo_root, hash) {
                Ok(text) => {
                    self.log_ui
                        .diff_lines
                        .extend(text.lines().map(|l| l.to_string()));
                }
                Err(e) => {
                    self.log_ui
                        .diff_lines
                        .push(format!("git show failed: {}", e));
                }
            },
            LogDetailMode::Files => {
                let needs_reload = self
                    .log_ui
                    .files_hash
                    .as_deref()
                    .map(|h| h != hash)
                    .unwrap_or(true);

                if needs_reload {
                    self.log_ui.files_hash = Some(hash.to_string());
                    match git_ops::list_commit_files(repo_root, hash) {
                        Ok(files) => {
                            self.log_ui.files = files;
                            if self.log_ui.files.is_empty() {
                                self.log_ui.files_state.select(None);
                            } else {
                                self.log_ui.files_state.select(Some(0));
                            }
                        }
                        Err(e) => {
                            self.log_ui.files.clear();
                            self.log_ui.files_state.select(None);
                            self.log_ui
                                .diff_lines
                                .push(format!("git show failed: {}", e));
                            return;
                        }
                    }
                }

                let Some(sel) = self.log_ui.files_state.selected() else {
                    self.log_ui.diff_lines.push("No files".to_string());
                    return;
                };
                let Some(file) = self.log_ui.files.get(sel) else {
                    return;
                };

                match git_ops::show_commit_file_diff(repo_root, hash, &file.path) {
                    Ok(text) => {
                        if text.trim().is_empty() {
                            self.log_ui.diff_lines.push("(no diff)".to_string());
                        } else {
                            self.log_ui
                                .diff_lines
                                .extend(text.lines().map(|l| l.to_string()));
                        }
                    }
                    Err(e) => {
                        self.log_ui
                            .diff_lines
                            .push(format!("git show failed: {}", e));
                    }
                }
            }
        }
    }

    fn active_log_len(&self) -> usize {
        match self.log_ui.subtab {
            LogSubTab::History => self.log_ui.history.len(),
            LogSubTab::Reflog => self.log_ui.reflog.len(),
            LogSubTab::Commands => self.git_log.len(),
        }
    }

    fn set_log_subtab(&mut self, subtab: LogSubTab) {
        self.log_ui.inspect.close();
        self.log_ui.set_subtab(subtab);

        if self.log_ui.subtab == LogSubTab::Commands {
            if self.git_log.is_empty() {
                self.log_ui.command_state.select(None);
            } else if self
                .log_ui
                .command_state
                .selected()
                .map(|i| i >= self.git_log.len())
                .unwrap_or(true)
            {
                self.log_ui.command_state.select(Some(0));
            }
        } else {
            if self.log_ui.subtab == LogSubTab::History && !self.log_ui.history.is_empty() {
                if self
                    .log_ui
                    .history_state
                    .selected()
                    .map(|i| i >= self.log_ui.history.len())
                    .unwrap_or(true)
                {
                    self.log_ui.history_state.select(Some(0));
                }
            }
            if self.log_ui.subtab == LogSubTab::Reflog && !self.log_ui.reflog.is_empty() {
                if self
                    .log_ui
                    .reflog_state
                    .selected()
                    .map(|i| i >= self.log_ui.reflog.len())
                    .unwrap_or(true)
                {
                    self.log_ui.reflog_state.select(Some(0));
                }
            }
        }

        self.refresh_log_diff();
    }

    fn select_log_item(&mut self, idx: usize) {
        if idx >= self.active_log_len() {
            return;
        }
        self.log_ui.active_state_mut().select(Some(idx));
        self.log_ui.focus = LogPaneFocus::Commits;
        self.log_ui.diff_scroll_y = 0;
        self.log_ui.diff_scroll_x = 0;
        self.refresh_log_diff();
    }

    fn select_log_file(&mut self, idx: usize) {
        if idx >= self.log_ui.files.len() {
            return;
        }
        self.log_ui.files_state.select(Some(idx));
        self.log_ui.focus = LogPaneFocus::Files;
        self.log_ui.diff_scroll_y = 0;
        self.log_ui.diff_scroll_x = 0;
        self.refresh_log_diff();
    }

    fn move_log_file_selection(&mut self, delta: i32) {
        let len = self.log_ui.files.len();
        if len == 0 {
            self.log_ui.files_state.select(None);
            return;
        }

        let cur = self.log_ui.files_state.selected().unwrap_or(0) as i32;
        let next = (cur + delta).clamp(0, len.saturating_sub(1) as i32);
        self.select_log_file(next as usize);
    }

    fn move_log_selection(&mut self, delta: i32) {
        let len = self.active_log_len();
        if len == 0 {
            self.log_ui.active_state_mut().select(None);
            return;
        }

        let cur = self.log_ui.active_state().selected().unwrap_or(0) as i32;
        let next = (cur + delta).clamp(0, len.saturating_sub(1) as i32);
        self.select_log_item(next as usize);
    }

    fn start_git_job<F>(&mut self, cmd: String, refresh: bool, close_commit: bool, f: F)
    where
        F: FnOnce() -> Result<(), String> + Send + 'static,
    {
        if self.pending_job.is_some() {
            self.set_status("Busy");
            return;
        }

        let (tx, rx) = mpsc::channel();
        self.pending_job = Some(PendingJob { rx });

        thread::spawn(move || {
            let result = f();
            let _ = tx.send(JobResult::Git {
                cmd,
                result,
                refresh,
                close_commit,
            });
        });
    }

    fn start_ai_job<F>(&mut self, f: F)
    where
        F: FnOnce() -> Result<String, String> + Send + 'static,
    {
        if self.pending_job.is_some() {
            self.commit.set_status("Busy");
            return;
        }

        let (tx, rx) = mpsc::channel();
        self.pending_job = Some(PendingJob { rx });

        thread::spawn(move || {
            let result = f();
            let _ = tx.send(JobResult::Ai { result });
        });
    }

    fn poll_pending_job(&mut self) {
        let mut done: Option<JobResult> = None;
        if let Some(job) = &self.pending_job {
            match job.rx.try_recv() {
                Ok(msg) => done = Some(msg),
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => {
                    done = Some(JobResult::Ai {
                        result: Err("Background job disconnected".to_string()),
                    });
                }
            }
        }

        if let Some(msg) = done {
            self.pending_job = None;
            self.handle_job_result(msg);
        }
    }

    fn handle_job_result(&mut self, msg: JobResult) {
        match msg {
            JobResult::Git {
                cmd,
                result,
                refresh,
                close_commit,
            } => {
                self.push_git_log(cmd.clone(), &result);

                if refresh {
                    self.refresh_git_state();
                    if self.current_tab == Tab::Log {
                        self.refresh_log_data();
                    }
                }

                if close_commit {
                    self.commit.busy = false;
                }

                let wants_popup = !close_commit
                    && matches!(
                        cmd.as_str(),
                        "git fetch --prune" | "git pull --rebase" | "git push"
                    );

                let popup = if wants_popup {
                    let (ok, body) = match &result {
                        Ok(()) => (true, "Success".to_string()),
                        Err(e) => (false, e.clone()),
                    };
                    Some(OperationPopup::new(cmd.clone(), body, ok))
                } else {
                    None
                };

                match result {
                    Ok(()) => {
                        if close_commit {
                            self.commit.open = false;
                            self.commit.message.clear();
                            self.commit.cursor = 0;
                            self.commit.scroll_y = 0;
                            self.commit.set_status("Committed");
                            self.set_status("Commit succeeded");
                        } else {
                            let msg = if cmd.starts_with("git add") {
                                "Staged"
                            } else if cmd.starts_with("git restore --staged -- ") {
                                "Unstaged"
                            } else if cmd.starts_with("git restore --staged --worktree") {
                                "Discarded"
                            } else if cmd.starts_with("git restore -- ") {
                                "Discarded"
                            } else if cmd.starts_with("git clean") {
                                "Deleted"
                            } else {
                                "Done"
                            };
                            self.set_status(msg);
                        }
                    }
                    Err(e) => {
                        if close_commit {
                            self.commit.set_status(e.clone());
                            self.set_status("Commit failed");
                        } else {
                            self.set_status(e);
                        }
                    }
                }

                if let Some(popup) = popup {
                    self.operation_popup = Some(popup);
                }
            }
            JobResult::Ai { result } => {
                self.commit.busy = false;
                match result {
                    Ok(msg) => {
                        self.commit.message = msg;
                        self.commit.cursor = self.commit.message.chars().count();
                        self.commit.scroll_y = 0;
                        self.commit.set_status("AI message generated");
                    }
                    Err(e) => {
                        self.commit.set_status(e);
                    }
                }
            }
        }
    }

    fn handle_git_footer(&mut self, action: GitFooterAction) {
        if self.git.repo_root.is_none() {
            self.set_status("Not a git repository");
            return;
        }

        if self.pending_job.is_some() {
            self.set_status("Busy");
            return;
        }

        match action {
            GitFooterAction::Stage => {
                let Some(repo_root) = self.git.repo_root.clone() else {
                    self.set_status("Not a git repository");
                    return;
                };

                let mut paths: Vec<String> = if self.git.selected_paths.is_empty() {
                    self.git
                        .selected_entry()
                        .map(|e| vec![e.path.clone()])
                        .unwrap_or_default()
                } else {
                    self.git.selected_paths.iter().cloned().collect()
                };

                if paths.is_empty() {
                    self.set_status("No selection");
                    return;
                }

                paths.sort();

                let cmd = if paths.len() == 1 {
                    format!("git add -- {}", paths[0])
                } else {
                    format!("git add ({})", paths.len())
                };

                self.start_git_job(cmd, true, false, move || {
                    git_ops::stage_paths(&repo_root, &paths)
                });
            }
            GitFooterAction::Unstage => {
                let Some(repo_root) = self.git.repo_root.clone() else {
                    self.set_status("Not a git repository");
                    return;
                };

                let paths: Vec<String> = if self.git.selected_paths.is_empty() {
                    self.git
                        .selected_entry()
                        .map(|e| vec![e.path.clone()])
                        .unwrap_or_default()
                } else {
                    self.git.selected_paths.iter().cloned().collect()
                };

                if paths.is_empty() {
                    self.set_status("No selection");
                    return;
                }

                let mut staged_paths: Vec<String> = Vec::new();
                for p in paths {
                    if let Some(e) = self.git.entries.iter().find(|e| e.path == p) {
                        let staged = e.x != ' ' && e.x != '?';
                        if staged {
                            staged_paths.push(p);
                        }
                    }
                }

                if staged_paths.is_empty() {
                    self.set_status("Nothing staged in selection");
                    return;
                }

                staged_paths.sort();

                let cmd = if staged_paths.len() == 1 {
                    format!("git restore --staged -- {}", staged_paths[0])
                } else {
                    format!("git restore --staged ({})", staged_paths.len())
                };

                self.start_git_job(cmd, true, false, move || {
                    git_ops::unstage_paths(&repo_root, &staged_paths)
                });
            }
            GitFooterAction::Discard => {
                let paths = self.selected_git_paths();
                if paths.is_empty() {
                    self.set_status("No selection");
                    return;
                }

                let mut items: Vec<DiscardItem> = Vec::new();
                for p in paths {
                    if let Some(entry) = self.git.entries.iter().find(|e| e.path == p) {
                        if entry.is_conflict {
                            self.set_status("Cannot discard conflicts");
                            return;
                        }

                        let staged = entry.x != ' ' && entry.x != '?';
                        let mode = if entry.is_untracked {
                            DiscardMode::Untracked
                        } else if staged {
                            DiscardMode::AllChanges
                        } else {
                            DiscardMode::Worktree
                        };

                        items.push(DiscardItem { path: p, mode });
                    }
                }

                if items.is_empty() {
                    self.set_status("No selection");
                    return;
                }

                self.discard_confirm = Some(DiscardConfirm { items });
            }
            GitFooterAction::Commit => {
                if !self.commit.open {
                    self.commit.open = true;
                    self.commit.focus = CommitFocus::Message;
                    return;
                }

                let Some(repo_root) = self.git.repo_root.clone() else {
                    self.commit.set_status("Not a git repository");
                    return;
                };
                match git_ops::has_staged_changes(&repo_root) {
                    Ok(true) => {}
                    Ok(false) => {
                        self.commit.set_status("No staged changes");
                        return;
                    }
                    Err(e) => {
                        self.commit.set_status(e);
                        return;
                    }
                }

                let msg = self.commit.message.clone();
                if msg.trim().is_empty() {
                    self.commit.set_status("Empty commit message");
                    return;
                }

                self.commit.busy = true;
                let cmd = "git commit".to_string();
                self.start_git_job(cmd, true, true, move || {
                    git_ops::commit_message(&repo_root, &msg)
                });
            }
        }
    }

    fn toggle_stage_for_selection(&mut self) {
        if self.pending_job.is_some() {
            self.set_status("Busy");
            return;
        }

        let paths: Vec<String> = if self.git.selected_paths.is_empty() {
            self.git
                .selected_entry()
                .map(|e| vec![e.path.clone()])
                .unwrap_or_default()
        } else {
            self.git.selected_paths.iter().cloned().collect()
        };

        if paths.is_empty() {
            self.set_status("No selection");
            return;
        }

        let mut staged_count = 0usize;
        let mut known = 0usize;
        for p in &paths {
            if let Some(e) = self.git.entries.iter().find(|e| &e.path == p) {
                known += 1;
                let staged = e.x != ' ' && e.x != '?';
                if staged {
                    staged_count += 1;
                }
            }
        }

        if known > 0 && staged_count == known {
            self.handle_git_footer(GitFooterAction::Unstage);
        } else {
            self.handle_git_footer(GitFooterAction::Stage);
        }
    }

    fn select_all_git_filtered(&mut self) {
        self.git.selected_paths.clear();
        for abs in &self.git.filtered {
            if let Some(e) = self.git.entries.get(*abs) {
                self.git.selected_paths.insert(e.path.clone());
            }
        }
        self.git.selection_anchor = Some(0);
        if !self.git.filtered.is_empty() {
            self.git.list_state.select(Some(0));
        }
    }

    fn stage_all_visible(&mut self) {
        self.git.selected_paths.clear();
        for abs in &self.git.filtered {
            if let Some(e) = self.git.entries.get(*abs) {
                if !e.is_conflict {
                    self.git.selected_paths.insert(e.path.clone());
                }
            }
        }
        self.handle_git_footer(GitFooterAction::Stage);
    }

    fn unstage_all_visible(&mut self) {
        self.git.selected_paths.clear();
        for abs in &self.git.filtered {
            if let Some(e) = self.git.entries.get(*abs) {
                let staged = e.x != ' ' && e.x != '?';
                if staged {
                    self.git.selected_paths.insert(e.path.clone());
                }
            }
        }
        self.handle_git_footer(GitFooterAction::Unstage);
    }

    fn start_ai_generate(&mut self) {
        if !self.commit.open {
            self.commit.open = true;
        }

        let Some(repo_root) = self.git.repo_root.clone() else {
            self.commit.set_status("Not a git repository");
            return;
        };

        match git_ops::has_staged_changes(&repo_root) {
            Ok(true) => {}
            Ok(false) => {
                self.commit.set_status("No staged changes");
                return;
            }
            Err(e) => {
                self.commit.set_status(e);
                return;
            }
        }

        self.commit.busy = true;
        self.commit.set_status("Generating...");

        self.start_ai_job(move || {
            let cfg = openrouter::OpenRouterConfig::from_env()?;
            let diff = git_ops::staged_diff(&repo_root)?;
            openrouter::generate_commit_message(&cfg, &diff)
        });
    }

    fn confirm_discard(&mut self) {
        if self.pending_job.is_some() {
            self.set_status("Busy");
            return;
        }

        let Some(confirm) = self.discard_confirm.take() else {
            return;
        };
        let Some(repo_root) = self.git.repo_root.clone() else {
            self.set_status("Not a git repository");
            return;
        };

        let items = confirm.items;
        let n = items.len();
        let cmd = format!("discard ({})", n);

        self.start_git_job(cmd, true, false, move || {
            for item in items {
                let res = match item.mode {
                    DiscardMode::Worktree => git_ops::discard_worktree_path(&repo_root, &item.path),
                    DiscardMode::Untracked => {
                        git_ops::discard_untracked_path(&repo_root, &item.path)
                    }
                    DiscardMode::AllChanges => {
                        git_ops::discard_all_changes_path(&repo_root, &item.path)
                    }
                };
                if let Err(e) = res {
                    return Err(format!("{}: {}", item.path, e));
                }
            }
            Ok(())
        });
    }

    fn start_operation_job(&mut self, cmd: &str, refresh: bool) {
        let Some(repo_root) = self.git.repo_root.clone() else {
            self.set_status("Not a git repository");
            return;
        };

        if self.pending_job.is_some() {
            self.set_status("Busy");
            return;
        }

        self.set_status(format!("Running: {}", cmd));

        match cmd {
            "git merge --continue" => {
                self.start_git_job(cmd.to_string(), refresh, false, move || {
                    git_ops::merge_continue(&repo_root)
                });
            }
            "git merge --abort" => {
                self.start_git_job(cmd.to_string(), refresh, false, move || {
                    git_ops::merge_abort(&repo_root)
                });
            }
            "git rebase --continue" => {
                self.start_git_job(cmd.to_string(), refresh, false, move || {
                    git_ops::rebase_continue(&repo_root)
                });
            }
            "git rebase --abort" => {
                self.start_git_job(cmd.to_string(), refresh, false, move || {
                    git_ops::rebase_abort(&repo_root)
                });
            }
            "git rebase --skip" => {
                self.start_git_job(cmd.to_string(), refresh, false, move || {
                    git_ops::rebase_skip(&repo_root)
                });
            }
            "git fetch --prune" => {
                self.start_git_job(cmd.to_string(), refresh, false, move || {
                    git_ops::fetch_prune(&repo_root)
                });
            }
            "git pull --rebase" => {
                self.start_git_job(cmd.to_string(), refresh, false, move || {
                    git_ops::pull_rebase(&repo_root)
                });
            }
            "git push" => {
                self.start_git_job(cmd.to_string(), refresh, false, move || {
                    git_ops::push(&repo_root)
                });
            }
            _ => {
                self.set_status("Unknown operation");
            }
        }
    }

    fn change_conflict_block(&mut self, delta: i32) {
        self.ensure_conflicts_loaded();
        let Some(file) = self.conflict_ui.file.as_ref() else {
            self.set_status("No conflicts loaded");
            return;
        };
        if file.blocks.is_empty() {
            self.set_status("No conflict markers found");
            return;
        }

        let cur = self.conflict_ui.selected_block as i32;
        let next = (cur + delta).clamp(0, file.blocks.len().saturating_sub(1) as i32);
        self.conflict_ui.selected_block = next as usize;
        self.conflict_ui.scroll_y = 0;
    }

    fn apply_conflict_resolution(&mut self, resolution: ConflictResolution) {
        if self.pending_job.is_some() {
            self.set_status("Busy");
            return;
        }

        self.ensure_conflicts_loaded();
        let Some(repo_root) = self.git.repo_root.clone() else {
            self.set_status("Not a git repository");
            return;
        };
        let Some(rel) = self.conflict_ui.path.clone() else {
            self.set_status("No conflict file selected");
            return;
        };

        let abs = repo_root.join(&rel);
        let idx = self.conflict_ui.selected_block;
        match conflict::apply_conflict_resolution(&abs, idx, resolution) {
            Ok(()) => {
                self.git.refresh(&self.current_path);
                self.update_git_operation();
                self.conflict_ui.path = None;
                self.ensure_conflicts_loaded();
                self.set_status("Conflict applied");
            }
            Err(e) => {
                self.set_status(e);
            }
        }
    }

    fn mark_conflict_resolved(&mut self) {
        let Some(entry) = self.git.selected_entry() else {
            self.set_status("No selection");
            return;
        };
        let Some(repo_root) = self.git.repo_root.clone() else {
            self.set_status("Not a git repository");
            return;
        };

        let path = entry.path.clone();
        let cmd = format!("git add -- {}", path);
        self.start_git_job(cmd, true, false, move || {
            git_ops::stage_path(&repo_root, &path)
        });
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

    fn set_theme(&mut self, theme: theme::Theme) {
        self.theme = theme;
        self.palette = theme::palette(theme);
    }

    fn open_theme_picker(&mut self) {
        if self.theme_picker.open {
            self.theme_picker.open = false;
            return;
        }

        self.context_menu = None;
        self.pending_menu_action = None;
        self.command_palette.open = false;

        let current = THEME_ORDER
            .iter()
            .position(|t| *t == self.theme)
            .unwrap_or(0);
        self.theme_picker.open = true;
        self.theme_picker.list_state.select(Some(current));
    }

    fn close_theme_picker(&mut self) {
        self.theme_picker.open = false;
    }

    fn move_theme_picker(&mut self, delta: i32) {
        let len = THEME_ORDER.len();
        if len == 0 {
            self.theme_picker.list_state.select(None);
            return;
        }

        let cur = self.theme_picker.list_state.selected().unwrap_or(0) as i32;
        let next = (cur + delta).rem_euclid(len as i32) as usize;
        self.theme_picker.list_state.select(Some(next));
    }

    fn apply_theme_picker_selection(&mut self) {
        let Some(idx) = self.theme_picker.list_state.selected() else {
            return;
        };
        let Some(theme) = THEME_ORDER.get(idx).copied() else {
            return;
        };

        self.set_theme(theme);
        self.save_persisted_ui_settings();
        self.set_status(format!("Theme: {}", theme.label()));
        self.close_theme_picker();
    }

    fn open_command_palette(&mut self) {
        if self.operation_popup.is_some()
            || self.discard_confirm.is_some()
            || self.branch_ui.open
            || self.log_ui.inspect.open
        {
            return;
        }

        if self.command_palette.open {
            self.command_palette.open = false;
            return;
        }

        self.context_menu = None;
        self.pending_menu_action = None;
        self.theme_picker.open = false;

        self.command_palette.open = true;
        self.command_palette.list_state.select(Some(0));
    }

    fn close_command_palette(&mut self) {
        self.command_palette.open = false;
    }

    fn move_command_palette(&mut self, delta: i32) {
        let len = COMMAND_PALETTE_ITEMS.len();
        if len == 0 {
            self.command_palette.list_state.select(None);
            return;
        }

        let cur = self.command_palette.list_state.selected().unwrap_or(0) as i32;
        let next = (cur + delta).rem_euclid(len as i32) as usize;
        self.command_palette.list_state.select(Some(next));
    }

    fn run_command_palette_selection(&mut self) {
        let Some(idx) = self.command_palette.list_state.selected() else {
            return;
        };
        let Some((cmd, _)) = COMMAND_PALETTE_ITEMS.get(idx).copied() else {
            return;
        };
        self.close_command_palette();
        self.run_command(cmd);
    }

    fn run_command(&mut self, cmd: CommandId) {
        match cmd {
            CommandId::ToggleHidden => {
                self.show_hidden = !self.show_hidden;
                self.load_files();
                self.set_status(if self.show_hidden {
                    "Hidden files: shown"
                } else {
                    "Hidden files: hidden"
                });
            }
            CommandId::ToggleWrapDiff => {
                self.wrap_diff = !self.wrap_diff;
                self.set_status(if self.wrap_diff {
                    "Diff wrap: on"
                } else {
                    "Diff wrap: off"
                });
            }
            CommandId::ToggleSyntaxHighlight => {
                self.syntax_highlight = !self.syntax_highlight;
                self.set_status(if self.syntax_highlight {
                    "Syntax highlight: on"
                } else {
                    "Syntax highlight: off"
                });
            }
            CommandId::SelectTheme => {
                self.open_theme_picker();
            }
            CommandId::RefreshGit => {
                self.refresh_git_state();
                self.set_status("Git refreshed");
            }
            CommandId::GitFetch => self.start_operation_job("git fetch --prune", true),
            CommandId::GitPullRebase => self.start_operation_job("git pull --rebase", true),
            CommandId::GitPush => self.start_operation_job("git push", true),
            CommandId::OpenBranchPicker => self.open_branch_picker(),
            CommandId::ClearGitLog => {
                self.git_log.clear();
                self.log_ui.command_state.select(None);
                self.log_ui.diff_lines.clear();
                self.set_status("Commands cleared");
            }
            CommandId::Quit => self.should_quit = true,
        }
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

    fn tick_pending_menu_action(&mut self) {
        let Some((idx, armed)) = self.pending_menu_action else {
            return;
        };

        if armed {
            self.pending_menu_action = None;
            self.execute_menu_action(idx);
        } else {
            self.pending_menu_action = Some((idx, true));
        }
    }

    fn update_context_menu_hover(&mut self, row: u16, col: u16) {
        let Some(menu) = &mut self.context_menu else {
            return;
        };

        let width = 30u16;
        let height = menu.options.len() as u16 + 2;

        if col < menu.x || col >= menu.x + width {
            return;
        }
        if row <= menu.y || row >= menu.y + height - 1 {
            return;
        }

        let idx = (row - menu.y - 1) as usize;
        if idx < menu.options.len() {
            menu.selected = idx;
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

    fn load_persisted_ui_settings(&mut self) {
        let Some(path) = self.ui_settings_path.clone() else {
            return;
        };

        let data = fs::read_to_string(&path).ok();
        let Some(data) = data else {
            return;
        };

        let settings: PersistedUiSettings = match serde_json::from_str(&data) {
            Ok(s) => s,
            Err(_) => return,
        };

        if let Some(w) = settings.log_left_width {
            self.log_ui.left_width = w.clamp(32, 90);
        }

        if let Some(theme) = settings.theme {
            self.set_theme(theme);
        }
    }

    fn save_persisted_ui_settings(&mut self) {
        let Some(path) = self.ui_settings_path.clone() else {
            return;
        };

        let settings = PersistedUiSettings {
            log_left_width: Some(self.log_ui.left_width),
            theme: Some(self.theme),
        };

        let content = match serde_json::to_string(&settings) {
            Ok(s) => s,
            Err(_) => return,
        };

        if let Some(parent) = path.parent() {
            if fs::create_dir_all(parent).is_err() {
                return;
            }
        }

        let tmp = path.with_extension("tmp");
        if fs::write(&tmp, content).is_err() || fs::rename(&tmp, &path).is_err() {
            let _ = fs::remove_file(&tmp);
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

    fn handle_click(&mut self, row: u16, col: u16, modifiers: KeyModifiers) {
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
                self.pending_menu_action = None;
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
                } else if tab == Tab::Log {
                    self.refresh_log_data();
                }
            }
            AppAction::RefreshGit => {
                self.refresh_git_state();
                self.set_status("Git refreshed");
            }
            AppAction::OpenCommandPalette => {
                self.open_command_palette();
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
                self.git.selected_paths.clear();
                self.git.selection_anchor = None;
            }
            AppAction::SelectGitFile(idx) => {
                self.git.select_filtered(idx, &self.current_path);

                let Some(abs) = self.git.filtered.get(idx).copied() else {
                    return;
                };
                let Some(entry) = self.git.entries.get(abs) else {
                    return;
                };

                if modifiers.contains(KeyModifiers::SHIFT) {
                    let anchor = self.git.selection_anchor.unwrap_or(idx);
                    let (a, b) = if anchor <= idx {
                        (anchor, idx)
                    } else {
                        (idx, anchor)
                    };
                    self.git.selected_paths.clear();
                    for i in a..=b {
                        if let Some(abs) = self.git.filtered.get(i).copied()
                            && let Some(e) = self.git.entries.get(abs)
                        {
                            self.git.selected_paths.insert(e.path.clone());
                        }
                    }
                } else if modifiers.contains(KeyModifiers::CONTROL) {
                    if self.git.selected_paths.contains(&entry.path) {
                        self.git.selected_paths.remove(&entry.path);
                    } else {
                        self.git.selected_paths.insert(entry.path.clone());
                    }
                    self.git.selection_anchor = Some(idx);
                } else {
                    self.git.selected_paths.clear();
                    self.git.selected_paths.insert(entry.path.clone());
                    self.git.selection_anchor = Some(idx);
                }
            }
            AppAction::ToggleCommitDrawer => {
                self.commit.open = !self.commit.open;
                if self.commit.open {
                    self.commit.focus = CommitFocus::Message;
                }
            }
            AppAction::FocusCommitMessage => {
                self.commit.focus = CommitFocus::Message;
            }
            AppAction::GenerateCommitMessage => {
                self.start_ai_generate();
            }
            AppAction::ConfirmDiscard => {
                self.confirm_discard();
            }
            AppAction::CancelDiscard => {
                self.discard_confirm = None;
            }
            AppAction::ClearGitLog => {
                self.git_log.clear();
                self.log_ui.command_state.select(None);
                self.log_ui.diff_lines.clear();
                self.set_status("Commands cleared");
            }
            AppAction::LogSwitch(subtab) => {
                self.set_log_subtab(subtab);
            }
            AppAction::LogDetail(mode) => {
                self.log_ui.inspect.close();
                self.log_ui.set_detail_mode(mode);
                self.refresh_log_diff();
            }
            AppAction::LogToggleZoom => {
                self.toggle_log_zoom();
            }
            AppAction::LogInspect => {
                self.open_log_inspect();
            }
            AppAction::LogCloseInspect => {
                self.log_ui.inspect.close();
            }
            AppAction::LogInspectCopyPrimary => {
                if let Some(s) = self
                    .selected_log_hash()
                    .or_else(|| self.selected_log_command())
                {
                    self.request_copy_to_clipboard(s);
                }
            }
            AppAction::LogInspectCopySecondary => {
                if let Some(s) = self.selected_log_subject() {
                    self.request_copy_to_clipboard(s);
                } else if !self.log_ui.inspect.body.is_empty() {
                    self.request_copy_to_clipboard(self.log_ui.inspect.body.clone());
                }
            }
            AppAction::LogFocusDiff => {
                self.log_ui.focus = LogPaneFocus::Diff;
            }
            AppAction::LogFocusFiles => {
                self.log_ui.focus = LogPaneFocus::Files;
            }
            AppAction::LogAdjustLeft(delta) => {
                self.adjust_log_left_width(delta);
            }
            AppAction::SelectLogItem(idx) => {
                self.select_log_item(idx);
            }
            AppAction::SelectLogFile(idx) => {
                self.select_log_file(idx);
            }
            AppAction::CloseOperationPopup => {
                self.operation_popup = None;
            }
            AppAction::MergeContinue => self.start_operation_job("git merge --continue", true),
            AppAction::MergeAbort => self.start_operation_job("git merge --abort", true),
            AppAction::RebaseContinue => self.start_operation_job("git rebase --continue", true),
            AppAction::RebaseAbort => self.start_operation_job("git rebase --abort", true),
            AppAction::RebaseSkip => self.start_operation_job("git rebase --skip", true),
            AppAction::ConflictPrev => self.change_conflict_block(-1),
            AppAction::ConflictNext => self.change_conflict_block(1),
            AppAction::ConflictUseOurs => self.apply_conflict_resolution(ConflictResolution::Ours),
            AppAction::ConflictUseTheirs => {
                self.apply_conflict_resolution(ConflictResolution::Theirs)
            }
            AppAction::ConflictUseBoth => self.apply_conflict_resolution(ConflictResolution::Both),
            AppAction::MarkResolved => self.mark_conflict_resolved(),
            AppAction::OpenBranchPicker => self.open_branch_picker(),
            AppAction::CloseBranchPicker => self.close_branch_picker(),
            AppAction::SelectBranch(idx) => {
                self.branch_ui.list_state.select(Some(idx));
            }
            AppAction::BranchCheckout => self.branch_checkout_selected(false),
            AppAction::ConfirmBranchCheckout => self.branch_checkout_selected(true),
            AppAction::CancelBranchCheckout => {
                self.branch_ui.confirm_checkout = None;
            }
            AppAction::GitFetch => self.start_operation_job("git fetch --prune", true),
            AppAction::GitPullRebase => self.start_operation_job("git pull --rebase", true),
            AppAction::GitPush => self.start_operation_job("git push", true),
            AppAction::ToggleGitStage => self.toggle_stage_for_selection(),
            AppAction::GitStageAllVisible => self.stage_all_visible(),
            AppAction::GitUnstageAllVisible => self.unstage_all_visible(),
            AppAction::GitFooter(action) => {
                self.handle_git_footer(action);
            }
            AppAction::ToggleHidden => {
                self.show_hidden = !self.show_hidden;
                self.load_files();
            }
            AppAction::Quit => self.should_quit = true,
            AppAction::ContextMenuAction(idx) => {
                if let Some(menu) = &mut self.context_menu {
                    menu.selected = idx;
                }
                self.pending_menu_action = Some((idx, false));
            }
            AppAction::None => {}
        }
    }

    fn handle_context_click(&mut self, row: u16, col: u16, modifiers: KeyModifiers) {
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
            AppAction::Select(idx) => {
                self.list_state.select(Some(idx));
                self.update_preview();
                self.preview_scroll = 0;
            }
            AppAction::SelectGitSection(section) => {
                self.git.set_section(section, &self.current_path);
                self.git.selected_paths.clear();
                self.git.selection_anchor = None;
            }
            AppAction::SelectGitFile(idx) => {
                self.git.select_filtered(idx, &self.current_path);

                let Some(abs) = self.git.filtered.get(idx).copied() else {
                    return;
                };
                let Some(entry) = self.git.entries.get(abs) else {
                    return;
                };

                if modifiers.contains(KeyModifiers::SHIFT) {
                    let anchor = self.git.selection_anchor.unwrap_or(idx);
                    let (a, b) = if anchor <= idx {
                        (anchor, idx)
                    } else {
                        (idx, anchor)
                    };
                    self.git.selected_paths.clear();
                    for i in a..=b {
                        if let Some(abs) = self.git.filtered.get(i).copied()
                            && let Some(e) = self.git.entries.get(abs)
                        {
                            self.git.selected_paths.insert(e.path.clone());
                        }
                    }
                } else if modifiers.contains(KeyModifiers::CONTROL) {
                    if self.git.selected_paths.contains(&entry.path) {
                        self.git.selected_paths.remove(&entry.path);
                    } else {
                        self.git.selected_paths.insert(entry.path.clone());
                    }
                    self.git.selection_anchor = Some(idx);
                } else {
                    self.git.selected_paths.clear();
                    self.git.selected_paths.insert(entry.path.clone());
                    self.git.selection_anchor = Some(idx);
                }
            }
            AppAction::SelectLogItem(idx) => {
                self.select_log_item(idx);
            }
            _ => {}
        }
    }

    fn open_context_menu(&mut self, row: u16, col: u16) {
        let mut options: Vec<(String, ContextCommand)> = Vec::new();

        match self.current_tab {
            Tab::Explorer => {
                options.push((" 📋 Copy Path ".to_string(), ContextCommand::CopyPath));
                options.push((
                    " 📄 Copy Relative Path ".to_string(),
                    ContextCommand::CopyRelPath,
                ));

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
                        ContextCommand::RemoveBookmark,
                    ));
                } else {
                    options.push((" 🔖 Add Bookmark ".to_string(), ContextCommand::AddBookmark));
                }

                options.push((" ✏️  Rename (TODO) ".to_string(), ContextCommand::Rename));

                if self.git.repo_root.is_some() {
                    options.push((
                        " 🙈 Add to .gitignore ".to_string(),
                        ContextCommand::GitAddToGitignore,
                    ));
                }
            }
            Tab::Git => {
                let paths = self.selected_git_paths();

                options.push((
                    " ✅ Toggle Stage ".to_string(),
                    ContextCommand::GitToggleStage,
                ));
                options.push((" + Stage ".to_string(), ContextCommand::GitStage));
                options.push((" - Unstage ".to_string(), ContextCommand::GitUnstage));

                let discard_label = if paths.len() == 1 {
                    " ↩ Discard… ".to_string()
                } else {
                    format!(" ↩ Discard… ({}) ", paths.len())
                };
                options.push((discard_label, ContextCommand::GitDiscard));

                options.push((" Stage All ".to_string(), ContextCommand::GitStageAll));
                options.push((" Unstage All ".to_string(), ContextCommand::GitUnstageAll));

                let ignore_label = if paths.len() <= 1 {
                    " 🙈 Add to .gitignore ".to_string()
                } else {
                    format!(" 🙈 Add to .gitignore ({}) ", paths.len())
                };
                options.push((ignore_label, ContextCommand::GitAddToGitignore));

                options.push((" 📋 Copy Path ".to_string(), ContextCommand::GitCopyPath));
                options.push((
                    " 📄 Copy Relative Path ".to_string(),
                    ContextCommand::GitCopyRelPath,
                ));
                options.push((
                    " 📂 Open In Explorer ".to_string(),
                    ContextCommand::GitOpenInExplorer,
                ));
            }
            Tab::Log => match self.log_ui.subtab {
                LogSubTab::History => {
                    let Some(entry) = self
                        .log_ui
                        .history_state
                        .selected()
                        .and_then(|i| self.log_ui.history.get(i))
                    else {
                        return;
                    };

                    options.push((" 📋 Copy SHA ".to_string(), ContextCommand::LogCopySha));
                    options.push((
                        " 📋 Copy Subject ".to_string(),
                        ContextCommand::LogCopySubject,
                    ));
                    options.push((
                        format!(" ⎇ Checkout {}… ", entry.short),
                        ContextCommand::LogCheckoutDetached,
                    ));
                }
                LogSubTab::Reflog => {
                    let Some(entry) = self
                        .log_ui
                        .reflog_state
                        .selected()
                        .and_then(|i| self.log_ui.reflog.get(i))
                    else {
                        return;
                    };

                    options.push((" 📋 Copy SHA ".to_string(), ContextCommand::LogCopySha));
                    options.push((
                        " 📋 Copy Subject ".to_string(),
                        ContextCommand::LogCopySubject,
                    ));
                    options.push((
                        format!(" ⎇ Checkout {}… ", entry.selector),
                        ContextCommand::LogCheckoutDetached,
                    ));
                }
                LogSubTab::Commands => {
                    let Some(entry) = self
                        .log_ui
                        .command_state
                        .selected()
                        .and_then(|i| self.git_log.get(i))
                    else {
                        return;
                    };
                    options.push((
                        " 📋 Copy Command ".to_string(),
                        ContextCommand::LogCopyCommand,
                    ));
                    let _ = entry;
                }
            },
        }

        self.context_menu = Some(ContextMenu {
            x: col,
            y: row,
            selected: 0,
            options,
        });
    }

    fn execute_menu_action(&mut self, action_idx: usize) {
        if let Some(menu) = &self.context_menu
            && let Some((_, action)) = menu.options.get(action_idx)
        {
            match action {
                ContextCommand::CopyPath => {
                    if let Some(file) = self.selected_file() {
                        self.request_copy_to_clipboard(file.path.to_string_lossy().to_string());
                    }
                }
                ContextCommand::CopyRelPath => {
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
                ContextCommand::AddBookmark => {
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
                ContextCommand::RemoveBookmark => {
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
                ContextCommand::Rename => {}
                ContextCommand::GitStage => self.handle_git_footer(GitFooterAction::Stage),
                ContextCommand::GitUnstage => self.handle_git_footer(GitFooterAction::Unstage),
                ContextCommand::GitToggleStage => self.toggle_stage_for_selection(),
                ContextCommand::GitDiscard => self.handle_git_footer(GitFooterAction::Discard),
                ContextCommand::GitStageAll => self.stage_all_visible(),
                ContextCommand::GitUnstageAll => self.unstage_all_visible(),
                ContextCommand::GitOpenInExplorer => self.open_selected_git_path_in_explorer(),
                ContextCommand::GitCopyPath => self.copy_selected_git_path(true),
                ContextCommand::GitCopyRelPath => self.copy_selected_git_path(false),
                ContextCommand::GitAddToGitignore => self.add_selected_to_gitignore(),
                ContextCommand::LogCopySha => {
                    if let Some(hash) = self.selected_log_hash() {
                        self.request_copy_to_clipboard(hash);
                    }
                }
                ContextCommand::LogCopySubject => {
                    if let Some(s) = self.selected_log_subject() {
                        self.request_copy_to_clipboard(s);
                    }
                }
                ContextCommand::LogCopyCommand => {
                    if let Some(s) = self.selected_log_command() {
                        self.request_copy_to_clipboard(s);
                    }
                }
                ContextCommand::LogCheckoutDetached => {
                    self.checkout_detached_selected_log();
                }
            }
        }
        self.context_menu = None;
    }

    fn selected_git_paths(&self) -> Vec<String> {
        if !self.git.selected_paths.is_empty() {
            return self.git.selected_paths.iter().cloned().collect();
        }
        self.git
            .selected_entry()
            .map(|e| vec![e.path.clone()])
            .unwrap_or_default()
    }

    fn selected_log_hash(&self) -> Option<String> {
        match self.log_ui.subtab {
            LogSubTab::History => self
                .log_ui
                .history_state
                .selected()
                .and_then(|i| self.log_ui.history.get(i))
                .map(|e| e.hash.clone()),
            LogSubTab::Reflog => self
                .log_ui
                .reflog_state
                .selected()
                .and_then(|i| self.log_ui.reflog.get(i))
                .map(|e| e.hash.clone()),
            LogSubTab::Commands => None,
        }
    }

    fn selected_log_subject(&self) -> Option<String> {
        match self.log_ui.subtab {
            LogSubTab::History => self
                .log_ui
                .history_state
                .selected()
                .and_then(|i| self.log_ui.history.get(i))
                .map(|e| e.subject.clone()),
            LogSubTab::Reflog => self
                .log_ui
                .reflog_state
                .selected()
                .and_then(|i| self.log_ui.reflog.get(i))
                .map(|e| e.subject.clone()),
            LogSubTab::Commands => None,
        }
    }

    fn selected_log_command(&self) -> Option<String> {
        if self.log_ui.subtab != LogSubTab::Commands {
            return None;
        }
        let sel = self.log_ui.command_state.selected()?;
        let entry = self.git_log.get(sel)?;
        Some(entry.cmd.clone())
    }

    fn checkout_detached_selected_log(&mut self) {
        if self.pending_job.is_some() {
            self.set_status("Busy");
            return;
        }

        let Some(repo_root) = self.git.repo_root.clone() else {
            self.set_status("Not a git repository");
            return;
        };

        let Some(hash) = self.selected_log_hash() else {
            self.set_status("No selection");
            return;
        };

        match git_ops::is_dirty(&repo_root) {
            Ok(true) => {
                self.set_status("Working tree dirty; checkout blocked");
                return;
            }
            Ok(false) => {}
            Err(e) => {
                self.set_status(e);
                return;
            }
        }

        let short = hash.chars().take(7).collect::<String>();
        let cmd = format!("git checkout --detach {}", short);
        self.start_git_job(cmd, true, false, move || {
            git_ops::checkout_detached(&repo_root, &hash)
        });
    }

    fn open_log_inspect(&mut self) {
        let (title, body) = match self.log_ui.subtab {
            LogSubTab::History => {
                let Some(sel) = self.log_ui.history_state.selected() else {
                    self.set_status("No selection");
                    return;
                };
                let Some(e) = self.log_ui.history.get(sel) else {
                    self.set_status("No selection");
                    return;
                };

                let mut body = String::new();
                body.push_str("SHA: ");
                body.push_str(&e.hash);
                body.push('\n');
                body.push_str("Date: ");
                body.push_str(&e.date);
                body.push('\n');
                body.push_str("Author: ");
                body.push_str(&e.author);
                body.push('\n');
                body.push('\n');
                body.push_str("Subject:\n");
                body.push_str(&e.subject);
                body.push('\n');

                (format!("Inspect {}", e.short), body)
            }
            LogSubTab::Reflog => {
                let Some(sel) = self.log_ui.reflog_state.selected() else {
                    self.set_status("No selection");
                    return;
                };
                let Some(e) = self.log_ui.reflog.get(sel) else {
                    self.set_status("No selection");
                    return;
                };

                let mut body = String::new();
                body.push_str("SHA: ");
                body.push_str(&e.hash);
                body.push('\n');
                body.push_str("Selector: ");
                body.push_str(&e.selector);
                body.push('\n');
                body.push('\n');
                body.push_str("Subject:\n");
                body.push_str(&e.subject);
                body.push('\n');

                (format!("Inspect {}", e.selector), body)
            }
            LogSubTab::Commands => {
                let Some(sel) = self.log_ui.command_state.selected() else {
                    self.set_status("No selection");
                    return;
                };
                let Some(e) = self.git_log.get(sel) else {
                    self.set_status("No selection");
                    return;
                };

                let mut body = String::new();
                body.push_str("Command:\n");
                body.push_str(&e.cmd);
                body.push('\n');
                body.push('\n');
                body.push_str("Output:\n");
                if let Some(d) = e.detail.as_deref() {
                    body.push_str(d);
                    if !d.ends_with('\n') {
                        body.push('\n');
                    }
                } else {
                    body.push_str("(no output)\n");
                }

                ("Inspect Command".to_string(), body)
            }
        };

        self.log_ui.inspect.open = true;
        self.log_ui.inspect.scroll_y = 0;
        self.log_ui.inspect.title = title;
        self.log_ui.inspect.body = body;
        self.context_menu = None;
    }

    fn toggle_log_zoom(&mut self) {
        let next = match self.log_ui.zoom {
            LogZoom::None => LogZoom::Diff,
            LogZoom::Diff => LogZoom::List,
            LogZoom::List => LogZoom::None,
        };
        self.log_ui.zoom = next;

        match next {
            LogZoom::Diff => self.log_ui.focus = LogPaneFocus::Diff,
            LogZoom::List => self.log_ui.focus = LogPaneFocus::Commits,
            LogZoom::None => {}
        }
    }

    fn cycle_log_focus(&mut self) {
        let files_mode = self.log_ui.detail_mode == LogDetailMode::Files
            && self.log_ui.subtab != LogSubTab::Commands;

        match self.log_ui.zoom {
            LogZoom::List => {
                self.log_ui.focus = LogPaneFocus::Commits;
            }
            LogZoom::Diff => {
                if files_mode {
                    self.log_ui.focus = match self.log_ui.focus {
                        LogPaneFocus::Files => LogPaneFocus::Diff,
                        _ => LogPaneFocus::Files,
                    };
                } else {
                    self.log_ui.focus = LogPaneFocus::Diff;
                }
            }
            LogZoom::None => {
                if files_mode {
                    self.log_ui.focus = match self.log_ui.focus {
                        LogPaneFocus::Commits => LogPaneFocus::Files,
                        LogPaneFocus::Files => LogPaneFocus::Diff,
                        LogPaneFocus::Diff => LogPaneFocus::Commits,
                    };
                } else {
                    self.log_ui.focus = match self.log_ui.focus {
                        LogPaneFocus::Diff => LogPaneFocus::Commits,
                        _ => LogPaneFocus::Diff,
                    };
                }
            }
        }
    }

    fn adjust_log_left_width(&mut self, delta: i16) {
        let cur = self.log_ui.left_width as i16;
        let next = (cur + delta).clamp(32, 90);
        self.log_ui.left_width = next as u16;
    }

    fn copy_selected_git_path(&mut self, absolute: bool) {
        let paths = self.selected_git_paths();
        let Some(first) = paths.first() else {
            self.set_status("No selection");
            return;
        };

        if absolute {
            let Some(root) = self.git.repo_root.clone() else {
                self.set_status("Not a git repository");
                return;
            };
            let p = root.join(first);
            self.request_copy_to_clipboard(p.to_string_lossy().to_string());
        } else {
            self.request_copy_to_clipboard(first.clone());
        }
    }

    fn open_selected_git_path_in_explorer(&mut self) {
        let paths = self.selected_git_paths();
        let Some(first) = paths.first() else {
            self.set_status("No selection");
            return;
        };
        let Some(root) = self.git.repo_root.clone() else {
            self.set_status("Not a git repository");
            return;
        };

        let abs = root.join(first);
        let Some(parent) = abs.parent() else {
            return;
        };

        self.current_tab = Tab::Explorer;
        self.navigate_to(parent.to_path_buf());
        self.load_files();

        if let Some(name) = abs.file_name().map(|s| s.to_string_lossy().to_string())
            && let Some(idx) = self.files.iter().position(|f| f.name == name)
        {
            self.list_state.select(Some(idx));
            self.update_preview();
        }
    }

    fn add_selected_to_gitignore(&mut self) {
        if self.git.repo_root.is_none() {
            self.git.refresh(&self.current_path);
        }

        let Some(repo_root) = self.git.repo_root.clone() else {
            self.set_status("Not a git repository");
            return;
        };

        let mut patterns: Vec<String> = match self.current_tab {
            Tab::Explorer => {
                let Some(file) = self.selected_file() else {
                    self.set_status("No selection");
                    return;
                };

                let Ok(rel) = file.path.strip_prefix(&repo_root) else {
                    self.set_status("Selection not in repo");
                    return;
                };

                let mut p = rel.to_string_lossy().to_string();
                if file.is_dir && !p.ends_with('/') {
                    p.push('/');
                }
                vec![p]
            }
            Tab::Git => self.selected_git_paths(),
            Tab::Log => {
                self.set_status("Not available here");
                return;
            }
        };

        if patterns.is_empty() {
            self.set_status("No selection");
            return;
        }

        for p in patterns.iter_mut() {
            let is_dir = repo_root.join(p.as_str()).is_dir();
            if is_dir && !p.ends_with('/') {
                p.push('/');
            }
        }

        patterns.sort();
        patterns.dedup();

        match git_ops::add_to_gitignore(&repo_root, &patterns) {
            Ok(0) => {
                self.set_status("Already ignored");
            }
            Ok(n) => {
                self.set_status(format!("Added {} to .gitignore", n));
                self.refresh_git_state();
            }
            Err(e) => {
                self.set_status(e);
            }
        }
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

fn ui_settings_file_path() -> Option<PathBuf> {
    let home = env::home_dir()?;
    let base = env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".config"));
    Some(base.join("te").join("ui.json"))
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

    f.render_widget(Block::default().bg(app.palette.bg), area);

    let main_layout = if app.current_tab == Tab::Git {
        let commit_h = if app.commit.open { 11 } else { 1 };
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

    let top_block = Block::default().borders(Borders::BOTTOM).border_style(
        Style::default()
            .fg(app.palette.border_inactive)
            .bg(app.palette.bg),
    );
    f.render_widget(top_block.clone(), top_bar);

    let tabs_y = top_bar.y;
    let mut tab_x = top_bar.x + 1;
    for (label, tab) in [
        (" Explorer ", Tab::Explorer),
        (" Git ", Tab::Git),
        (" Git Log ", Tab::Log),
    ] {
        let width = label.len() as u16;
        let is_active = app.current_tab == tab;
        let style = if is_active {
            Style::default()
                .bg(app.palette.accent_primary)
                .fg(app.palette.btn_fg)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().bg(app.palette.bg).fg(app.palette.fg)
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
                    Style::default().fg(app.palette.accent_secondary).bold(),
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
                        .fg(app.palette.accent_primary)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(app.palette.fg)
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
                            Style::default().fg(app.palette.border_inactive),
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
            let op = match app.git_operation {
                Some(GitOperation::Rebase) => "  REBASE ",
                Some(GitOperation::Merge) => "  MERGE ",
                None => "",
            };

            let width = top_bar.width.saturating_sub(2);
            let base_x = top_bar.x + 2;

            let mut spans: Vec<Span> = Vec::new();
            spans.push(Span::raw(" Repo: "));
            spans.push(Span::raw(repo.clone()));
            spans.push(Span::raw("   "));
            spans.push(Span::raw("Branch: "));

            let branch_text = format!("{} ▼", branch);
            let branch_prefix_len = " Repo: ".len() + repo.len() + "   ".len() + "Branch: ".len();
            let branch_x = base_x.saturating_add(branch_prefix_len as u16);
            let branch_w = branch_text.len() as u16;

            spans.push(Span::styled(
                branch_text.clone(),
                Style::default()
                    .fg(app.palette.accent_secondary)
                    .add_modifier(Modifier::BOLD),
            ));
            zones.push(ClickZone {
                rect: Rect::new(branch_x, second_row_y, branch_w, 1),
                action: AppAction::OpenBranchPicker,
            });

            spans.push(Span::raw(format!(
                "   ↑{} ↓{}{}   ",
                app.git.ahead, app.git.behind, op
            )));

            f.render_widget(
                Paragraph::new(Line::from(spans)).style(Style::default().fg(app.palette.fg)),
                Rect::new(base_x, second_row_y, width, 1),
            );

            let refresh_label = "[Refresh]";
            let refresh_x = base_x + width.saturating_sub(refresh_label.len() as u16);
            let refresh_rect = Rect::new(refresh_x, second_row_y, refresh_label.len() as u16, 1);
            zones.push(ClickZone {
                rect: refresh_rect,
                action: AppAction::RefreshGit,
            });

            let enabled = app.pending_job.is_none();
            let mut cursor = refresh_x.saturating_sub(2);

            if let Some(op) = app.git_operation {
                let buttons: Vec<(&str, AppAction, Color)> = match op {
                    GitOperation::Merge => vec![
                        (
                            "[Continue]",
                            AppAction::MergeContinue,
                            app.palette.accent_tertiary,
                        ),
                        ("[Abort]", AppAction::MergeAbort, app.palette.btn_bg),
                    ],
                    GitOperation::Rebase => vec![
                        (
                            "[Continue]",
                            AppAction::RebaseContinue,
                            app.palette.accent_tertiary,
                        ),
                        (
                            "[Skip]",
                            AppAction::RebaseSkip,
                            app.palette.accent_secondary,
                        ),
                        ("[Abort]", AppAction::RebaseAbort, app.palette.btn_bg),
                    ],
                };

                for (label, action, bg) in buttons.into_iter().rev() {
                    let w = label.len() as u16;
                    if cursor <= top_bar.x + 2 + w {
                        break;
                    }
                    let x = cursor.saturating_sub(w);
                    let rect = Rect::new(x, second_row_y, w, 1);
                    let style = Style::default()
                        .bg(if enabled {
                            bg
                        } else {
                            app.palette.border_inactive
                        })
                        .fg(if enabled {
                            app.palette.btn_fg
                        } else {
                            app.palette.fg
                        })
                        .add_modifier(Modifier::BOLD);
                    f.render_widget(Paragraph::new(label).style(style), rect);
                    if enabled {
                        zones.push(ClickZone { rect, action });
                    }
                    cursor = x.saturating_sub(1);
                }
            }

            if app.git.repo_root.is_some() {
                for (label, action, bg) in [
                    ("[Push]", AppAction::GitPush, app.palette.accent_secondary),
                    (
                        "[Pull]",
                        AppAction::GitPullRebase,
                        app.palette.accent_tertiary,
                    ),
                    ("[Fetch]", AppAction::GitFetch, app.palette.accent_primary),
                ] {
                    let w = label.len() as u16;
                    if cursor <= top_bar.x + 2 + w {
                        break;
                    }
                    let x = cursor.saturating_sub(w);
                    let rect = Rect::new(x, second_row_y, w, 1);
                    let style = Style::default()
                        .bg(if enabled {
                            bg
                        } else {
                            app.palette.border_inactive
                        })
                        .fg(if enabled {
                            app.palette.btn_fg
                        } else {
                            app.palette.fg
                        })
                        .add_modifier(Modifier::BOLD);
                    f.render_widget(Paragraph::new(label).style(style), rect);
                    if enabled {
                        zones.push(ClickZone { rect, action });
                    }
                    cursor = x.saturating_sub(1);
                }
            }
        }
        Tab::Log => {
            let sub = match app.log_ui.subtab {
                LogSubTab::History => "History",
                LogSubTab::Reflog => "Reflog",
                LogSubTab::Commands => "Commands",
            };
            let label = format!(" Git Log: {} ", sub);
            let width = label.len() as u16;
            f.render_widget(
                Paragraph::new(label).style(Style::default().fg(app.palette.fg)),
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
                .border_style(Style::default().fg(app.palette.border_inactive))
                .title(" Places ")
                .title_style(Style::default().fg(app.palette.accent_tertiary));
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
                        .fg(app.palette.accent_secondary)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(app.palette.fg)
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
                .border_style(Style::default().fg(app.palette.accent_primary))
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
                        app.palette.dir_color
                    } else if file.is_exec {
                        app.palette.exe_color
                    } else {
                        app.palette.fg
                    };

                    let name_span = Span::styled(&file.name, Style::default().fg(color));
                    let mut spans = vec![Span::raw(format!("{} ", icon)), name_span];

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

                let lines: Vec<Line> = if app.syntax_highlight {
                    app.selected_file()
                        .and_then(|f| {
                            if f.is_dir {
                                return None;
                            }
                            f.path.extension().and_then(|s| s.to_str()).and_then(|ext| {
                                highlight::highlight_text(&preview_text, ext, app.palette.bg)
                            })
                        })
                        .unwrap_or_else(|| preview_text.lines().map(Line::raw).collect())
                } else {
                    preview_text.lines().map(Line::raw).collect()
                };
                let p_block = Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(app.palette.border_inactive))
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
            app.ensure_conflicts_loaded();

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
                .border_style(Style::default().fg(app.palette.accent_primary))
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

            let all_count = app.git.entries.len();
            let sections = [
                (GitSection::All, format!(" All ({}) ", all_count)),
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
                        .bg(app.palette.selection_bg)
                        .add_modifier(Modifier::BOLD)
                        .fg(app.palette.fg)
                } else {
                    Style::default().fg(app.palette.fg)
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
                .border_style(Style::default().fg(app.palette.border_inactive))
                .title(" Files ");

            let file_items: Vec<ListItem> = app
                .git
                .filtered
                .iter()
                .filter_map(|abs| app.git.entries.get(*abs))
                .map(|e| {
                    let is_selected = app.git.selected_paths.contains(&e.path);

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
                        "M" => Style::default().fg(app.palette.accent_secondary),
                        "A" => Style::default().fg(app.palette.exe_color),
                        "D" => Style::default().fg(app.palette.btn_bg),
                        "??" => Style::default().fg(app.palette.accent_tertiary),
                        _ => Style::default().fg(app.palette.fg),
                    };

                    let checkbox = if is_selected { "▣ " } else { "□ " };

                    let mut spans = vec![
                        Span::styled(checkbox, Style::default().fg(app.palette.border_inactive)),
                        Span::styled(format!("{:>2} ", status), status_style),
                        Span::styled(e.path.as_str(), Style::default().fg(app.palette.fg)),
                    ];
                    if let Some(from) = &e.renamed_from {
                        spans.push(Span::styled(
                            format!(" (from {})", from),
                            Style::default().fg(app.palette.border_inactive),
                        ));
                    }

                    let mut item = ListItem::new(Line::from(spans));
                    if is_selected {
                        item = item.style(Style::default().bg(app.palette.menu_bg));
                    }
                    item
                })
                .collect();

            let files_list = List::new(file_items)
                .block(files_block)
                .highlight_style(
                    Style::default()
                        .bg(app.palette.selection_bg)
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

            let in_conflict_view = app.git.selected_entry().is_some_and(|e| e.is_conflict);

            if in_conflict_view {
                let title = app
                    .conflict_ui
                    .path
                    .as_deref()
                    .map(|p| format!(" Conflicts: {} ", p))
                    .unwrap_or_else(|| " Conflicts ".to_string());

                let block = Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
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
                let title_style = Style::default()
                    .fg(app.palette.fg)
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
                        format!(" Ours ({}/{}) ", cur.min(n.max(1)), n),
                        " Theirs ".to_string(),
                    )
                } else {
                    (0, " Ours ".to_string(), " Theirs ".to_string())
                };

                let header = Line::from(vec![
                    Span::styled(pad_to_width(ours_title, left_w), title_style),
                    Span::styled("│", sep_style),
                    Span::styled(pad_to_width(theirs_title, right_w), title_style),
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
                        for i in 0..n {
                            let left = block.ours.get(i).cloned().unwrap_or_default();
                            let right = block.theirs.get(i).cloned().unwrap_or_default();

                            let left = pad_to_width(
                                git::slice_chars(&left, app.git.diff_scroll_x as usize, left_w),
                                left_w,
                            );
                            let right = pad_to_width(
                                git::slice_chars(&right, app.git.diff_scroll_x as usize, right_w),
                                right_w,
                            );

                            content_lines.push(Line::from(vec![
                                Span::styled(left, Style::default().fg(app.palette.fg)),
                                Span::styled("│", sep_style),
                                Span::styled(right, Style::default().fg(app.palette.fg)),
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
            } else {
                let mode_label = match app.git.diff_mode {
                    GitDiffMode::SideBySide => "SxS",
                    GitDiffMode::Unified => "Unified",
                };
                let diff_block = Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(app.palette.border_inactive))
                    .title(format!(" Diff ({}) ", mode_label));

                let diff_lines: Vec<Line> = if app.git.repo_root.is_none() {
                    vec![Line::raw("Not a git repository")]
                } else if app.git.diff_lines.is_empty() {
                    vec![Line::raw("No selection")]
                } else {
                    match app.git.diff_mode {
                        GitDiffMode::Unified => {
                            let ext = app
                                .git
                                .selected_entry()
                                .and_then(|e| std::path::Path::new(e.path.as_str()).extension())
                                .and_then(|s| s.to_str());

                            let mut highlighter: Option<Highlighter> = if app.syntax_highlight {
                                ext.and_then(new_highlighter)
                            } else {
                                None
                            };

                            let mut out = Vec::new();
                            for l in &app.git.diff_lines {
                                let t = l.as_str();
                                if t.starts_with("@@") {
                                    out.push(Line::from(vec![Span::styled(
                                        t.to_string(),
                                        Style::default()
                                            .fg(app.palette.btn_fg)
                                            .bg(app.palette.diff_hunk_bg),
                                    )]));
                                    continue;
                                }

                                if t.starts_with("diff --git") {
                                    out.push(Line::from(vec![Span::styled(
                                        t.to_string(),
                                        Style::default().fg(app.palette.accent_primary),
                                    )]));
                                    continue;
                                }

                                if t.starts_with("index ")
                                    || t.starts_with("--- ")
                                    || t.starts_with("+++ ")
                                    || t.starts_with("rename ")
                                {
                                    out.push(Line::from(vec![Span::styled(
                                        t.to_string(),
                                        Style::default().fg(app.palette.border_inactive),
                                    )]));
                                    continue;
                                }

                                let (prefix, code) =
                                    t.split_at(t.chars().next().map(|c| c.len_utf8()).unwrap_or(0));
                                let (bg, is_code) = match prefix {
                                    "+" if !t.starts_with("+++") => (app.palette.diff_add_bg, true),
                                    "-" if !t.starts_with("---") => (app.palette.diff_del_bg, true),
                                    " " => (app.palette.bg, true),
                                    _ => (app.palette.bg, false),
                                };

                                if is_code {
                                    if let Some(hl) = highlighter.as_mut() {
                                        out.push(hl.highlight_diff_code_with_prefix(
                                            prefix,
                                            code,
                                            Style::default().fg(app.palette.fg),
                                            bg,
                                        ));
                                    } else {
                                        out.push(Line::from(vec![Span::styled(
                                            t.to_string(),
                                            Style::default().fg(app.palette.fg).bg(bg),
                                        )]));
                                    }
                                } else {
                                    out.push(Line::from(vec![Span::styled(
                                        t.to_string(),
                                        Style::default().fg(app.palette.fg).bg(bg),
                                    )]));
                                }
                            }
                            out
                        }
                        GitDiffMode::SideBySide => {
                            let inner_w = diff_area.width.saturating_sub(2) as usize;
                            let sep_w = 1usize;
                            let left_w = inner_w.saturating_sub(sep_w) / 2;
                            let right_w = inner_w.saturating_sub(sep_w).saturating_sub(left_w);

                            let mut out = Vec::new();
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

                            let rows = build_side_by_side_rows(&app.git.diff_lines);
                            for row in rows {
                                match row {
                                    GitDiffRow::Meta(t) => {
                                        let style = if t.starts_with("@@") {
                                            Style::default()
                                                .fg(app.palette.accent_tertiary)
                                                .bg(app.palette.diff_hunk_bg)
                                        } else if t.starts_with("diff --git") {
                                            Style::default().fg(app.palette.accent_primary)
                                        } else {
                                            Style::default().fg(app.palette.border_inactive)
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
                                            GitDiffCellKind::Delete => Style::default()
                                                .fg(app.palette.fg)
                                                .bg(app.palette.diff_del_bg),
                                            GitDiffCellKind::Context => Style::default()
                                                .fg(app.palette.fg)
                                                .bg(app.palette.bg),
                                            GitDiffCellKind::Add => Style::default()
                                                .fg(app.palette.fg)
                                                .bg(app.palette.bg),
                                            GitDiffCellKind::Empty => Style::default()
                                                .fg(app.palette.border_inactive)
                                                .bg(app.palette.bg),
                                        };
                                        let new_style = match new.kind {
                                            GitDiffCellKind::Add => Style::default()
                                                .fg(app.palette.fg)
                                                .bg(app.palette.diff_add_bg),
                                            GitDiffCellKind::Context => Style::default()
                                                .fg(app.palette.fg)
                                                .bg(app.palette.bg),
                                            GitDiffCellKind::Delete => Style::default()
                                                .fg(app.palette.fg)
                                                .bg(app.palette.bg),
                                            GitDiffCellKind::Empty => Style::default()
                                                .fg(app.palette.border_inactive)
                                                .bg(app.palette.bg),
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

                let wrap = app.git.diff_mode == GitDiffMode::Unified && app.wrap_diff;

                let viewport_h = diff_area.height.saturating_sub(2) as usize;
                let max_y = if viewport_h == 0 {
                    0
                } else if wrap {
                    app.git
                        .diff_lines
                        .iter()
                        .map(|l| {
                            let w = (diff_area.width.saturating_sub(2).max(1)) as usize;
                            let chars = l.chars().count().max(1);
                            (chars + w - 1) / w
                        })
                        .sum::<usize>()
                        .saturating_sub(viewport_h)
                } else {
                    match app.git.diff_mode {
                        GitDiffMode::Unified => app.git.diff_lines.len().saturating_sub(viewport_h),
                        GitDiffMode::SideBySide => build_side_by_side_rows(&app.git.diff_lines)
                            .len()
                            .saturating_sub(viewport_h),
                    }
                };
                app.git.diff_scroll_y = app.git.diff_scroll_y.min(max_y as u16);

                let x_scroll = if app.git.diff_mode == GitDiffMode::Unified && !wrap {
                    app.git.diff_scroll_x
                } else {
                    0
                };
                let mut diff_para = Paragraph::new(diff_lines)
                    .block(diff_block)
                    .scroll((app.git.diff_scroll_y, x_scroll));
                if wrap {
                    diff_para = diff_para.wrap(Wrap { trim: false });
                }

                f.render_widget(diff_para, diff_area);
            }
        }
        Tab::Log => {
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

            let mut x = subtab_area.x;
            for (label, subtab) in [
                (" History ", LogSubTab::History),
                (" Reflog ", LogSubTab::Reflog),
                (" Commands ", LogSubTab::Commands),
            ] {
                let w = label.len() as u16;
                let active = app.log_ui.subtab == subtab;
                let style = if active {
                    Style::default()
                        .bg(app.palette.accent_primary)
                        .fg(app.palette.btn_fg)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().bg(app.palette.bg).fg(app.palette.fg)
                };
                let rect = Rect::new(x, subtab_area.y, w, 1);
                f.render_widget(Paragraph::new(label).style(style), rect);
                zones.push(ClickZone {
                    rect,
                    action: AppAction::LogSwitch(subtab),
                });
                x += w + 1;
            }

            if zoom != LogZoom::Diff {
                let (title, items_len) = match app.log_ui.subtab {
                    LogSubTab::History => (" History ", app.log_ui.history.len()),
                    LogSubTab::Reflog => (" Reflog ", app.log_ui.reflog.len()),
                    LogSubTab::Commands => (" Commands ", app.git_log.len()),
                };

                let list_block = Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(app.palette.border_inactive))
                    .title(format!("{}({}) ", title, items_len));

                let list_items: Vec<ListItem> = match app.log_ui.subtab {
                    LogSubTab::History => app
                        .log_ui
                        .history
                        .iter()
                        .map(|e| {
                            ListItem::new(format!(
                                "{}  {}  {}: {}",
                                e.short, e.date, e.author, e.subject
                            ))
                        })
                        .collect(),
                    LogSubTab::Reflog => app
                        .log_ui
                        .reflog
                        .iter()
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

                match app.log_ui.subtab {
                    LogSubTab::History => {
                        f.render_stateful_widget(list, list_area, &mut app.log_ui.history_state)
                    }
                    LogSubTab::Reflog => {
                        f.render_stateful_widget(list, list_area, &mut app.log_ui.reflog_state)
                    }
                    LogSubTab::Commands => {
                        f.render_stateful_widget(list, list_area, &mut app.log_ui.command_state)
                    }
                }

                let list_inner = list_area.inner(Margin {
                    vertical: 1,
                    horizontal: 1,
                });

                let offset = match app.log_ui.subtab {
                    LogSubTab::History => app.log_ui.history_state.offset(),
                    LogSubTab::Reflog => app.log_ui.reflog_state.offset(),
                    LogSubTab::Commands => app.log_ui.command_state.offset(),
                };

                let end = (offset + list_inner.height as usize).min(items_len);
                for (i, idx) in (offset..end).enumerate() {
                    let rect =
                        Rect::new(list_inner.x, list_inner.y + i as u16, list_inner.width, 1);
                    zones.push(ClickZone {
                        rect,
                        action: AppAction::SelectLogItem(idx),
                    });
                }
            }

            if zoom != LogZoom::List {
                let files_mode = app.log_ui.detail_mode == LogDetailMode::Files
                    && app.log_ui.subtab != LogSubTab::Commands;

                let mut diff_view_area = diff_area;
                if files_mode {
                    let chunks = Layout::default()
                        .direction(Direction::Horizontal)
                        .constraints([Constraint::Length(36), Constraint::Min(0)])
                        .split(diff_area);
                    let files_area = chunks[0];
                    diff_view_area = chunks[1];

                    app.log_files_x = files_area.x;
                    app.log_diff_x = diff_view_area.x;

                    let file_block = Block::default()
                        .borders(Borders::ALL)
                        .border_type(BorderType::Rounded)
                        .border_style(Style::default().fg(app.palette.border_inactive))
                        .title(format!(" Files ({}) ", app.log_ui.files.len()));

                    let file_items: Vec<ListItem> = app
                        .log_ui
                        .files
                        .iter()
                        .map(|f| {
                            let s = if let Some(old) = f.old_path.as_deref() {
                                format!("{}  {} -> {}", f.status, old, f.path)
                            } else {
                                format!("{}  {}", f.status, f.path)
                            };
                            ListItem::new(s)
                        })
                        .collect();

                    let file_list = List::new(file_items)
                        .block(file_block)
                        .highlight_style(
                            Style::default()
                                .bg(app.palette.selection_bg)
                                .add_modifier(Modifier::BOLD),
                        )
                        .highlight_symbol("▎ ");

                    f.render_stateful_widget(file_list, files_area, &mut app.log_ui.files_state);

                    zones.push(ClickZone {
                        rect: files_area,
                        action: AppAction::LogFocusFiles,
                    });

                    let list_inner = files_area.inner(Margin {
                        vertical: 1,
                        horizontal: 1,
                    });

                    let items_len = app.log_ui.files.len();
                    let offset = app.log_ui.files_state.offset();
                    let end = (offset + list_inner.height as usize).min(items_len);
                    for (i, idx) in (offset..end).enumerate() {
                        let rect =
                            Rect::new(list_inner.x, list_inner.y + i as u16, list_inner.width, 1);
                        zones.push(ClickZone {
                            rect,
                            action: AppAction::SelectLogFile(idx),
                        });
                    }
                } else {
                    app.log_files_x = diff_area.x;
                    app.log_diff_x = diff_area.x;
                }

                let diff_area = diff_view_area;

                let diff_title = match app.log_ui.subtab {
                    LogSubTab::History => match app.log_ui.detail_mode {
                        LogDetailMode::Diff => " Commit Diff ",
                        LogDetailMode::Files => " Changed Files ",
                    },
                    LogSubTab::Reflog => match app.log_ui.detail_mode {
                        LogDetailMode::Diff => " Reflog Diff ",
                        LogDetailMode::Files => " Changed Files ",
                    },
                    LogSubTab::Commands => " Command Output ",
                };

                let diff_block = Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(app.palette.border_inactive))
                    .title(diff_title);

                let diff_lines: Vec<Line> = match app.log_ui.diff_mode {
                    GitDiffMode::Unified => {
                        let mut out = Vec::new();
                        let mut highlighter: Option<Highlighter> = None;

                        for l in &app.log_ui.diff_lines {
                            let t = l.as_str();

                            if app.syntax_highlight {
                                if let Some(p) = t.strip_prefix("+++ b/") {
                                    let ext = std::path::Path::new(p)
                                        .extension()
                                        .and_then(|s| s.to_str());
                                    highlighter = ext.and_then(new_highlighter);
                                }
                            }

                            if t.starts_with("@@") {
                                out.push(Line::from(vec![Span::styled(
                                    t.to_string(),
                                    Style::default()
                                        .fg(app.palette.btn_fg)
                                        .bg(app.palette.diff_hunk_bg),
                                )]));
                                continue;
                            }

                            if t.starts_with("diff --git") {
                                out.push(Line::from(vec![Span::styled(
                                    t.to_string(),
                                    Style::default().fg(app.palette.accent_primary),
                                )]));
                                continue;
                            }

                            if t.starts_with("index ")
                                || t.starts_with("--- ")
                                || t.starts_with("+++ ")
                                || t.starts_with("rename ")
                            {
                                out.push(Line::from(vec![Span::styled(
                                    t.to_string(),
                                    Style::default().fg(app.palette.border_inactive),
                                )]));
                                continue;
                            }

                            let (prefix, code) =
                                t.split_at(t.chars().next().map(|c| c.len_utf8()).unwrap_or(0));
                            let (bg, is_code) = match prefix {
                                "+" if !t.starts_with("+++") => (app.palette.diff_add_bg, true),
                                "-" if !t.starts_with("---") => (app.palette.diff_del_bg, true),
                                " " => (app.palette.bg, true),
                                _ => (app.palette.bg, false),
                            };

                            if is_code {
                                if let Some(hl) = highlighter.as_mut() {
                                    out.push(hl.highlight_diff_code_with_prefix(
                                        prefix,
                                        code,
                                        Style::default().fg(app.palette.fg),
                                        bg,
                                    ));
                                } else {
                                    out.push(Line::from(vec![Span::styled(
                                        t.to_string(),
                                        Style::default().fg(app.palette.fg).bg(bg),
                                    )]));
                                }
                            } else {
                                out.push(Line::from(vec![Span::styled(
                                    t.to_string(),
                                    Style::default().fg(app.palette.fg).bg(bg),
                                )]));
                            }
                        }

                        out
                    }
                    GitDiffMode::SideBySide => {
                        let rows = build_side_by_side_rows(&app.log_ui.diff_lines);
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

                        for r in rows {
                            match r {
                                GitDiffRow::Meta(t) => {
                                    let style = if t.starts_with("@@") {
                                        Style::default()
                                            .fg(app.palette.btn_fg)
                                            .bg(app.palette.diff_hunk_bg)
                                    } else if t.starts_with("+") {
                                        Style::default()
                                            .fg(app.palette.fg)
                                            .bg(app.palette.diff_add_bg)
                                    } else if t.starts_with("-") {
                                        Style::default()
                                            .fg(app.palette.fg)
                                            .bg(app.palette.diff_del_bg)
                                    } else if t.starts_with("diff --git") {
                                        Style::default().fg(app.palette.accent_primary)
                                    } else {
                                        Style::default().fg(app.palette.border_inactive)
                                    };
                                    out.push(Line::from(vec![Span::styled(t, style)]));
                                }
                                GitDiffRow::Split { old, new } => {
                                    let old_cell = render_side_by_side_cell(
                                        &old,
                                        left_w,
                                        app.log_ui.diff_scroll_x as usize,
                                    );
                                    let new_cell = render_side_by_side_cell(
                                        &new,
                                        right_w,
                                        app.log_ui.diff_scroll_x as usize,
                                    );

                                    let old_style = match old.kind {
                                        GitDiffCellKind::Delete => Style::default()
                                            .fg(app.palette.fg)
                                            .bg(app.palette.diff_del_bg),
                                        GitDiffCellKind::Context => {
                                            Style::default().fg(app.palette.fg).bg(app.palette.bg)
                                        }
                                        GitDiffCellKind::Add => {
                                            Style::default().fg(app.palette.fg).bg(app.palette.bg)
                                        }
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
                };

                let wrap = app.log_ui.diff_mode == GitDiffMode::Unified && app.wrap_diff;

                let viewport_h = diff_area.height.saturating_sub(2) as usize;
                let max_y = if viewport_h == 0 {
                    0
                } else if wrap {
                    app.log_ui
                        .diff_lines
                        .iter()
                        .map(|l| {
                            let w = (diff_area.width.saturating_sub(2).max(1)) as usize;
                            let chars = l.chars().count().max(1);
                            (chars + w - 1) / w
                        })
                        .sum::<usize>()
                        .saturating_sub(viewport_h)
                } else {
                    match app.log_ui.diff_mode {
                        GitDiffMode::Unified => {
                            app.log_ui.diff_lines.len().saturating_sub(viewport_h)
                        }
                        GitDiffMode::SideBySide => build_side_by_side_rows(&app.log_ui.diff_lines)
                            .len()
                            .saturating_sub(viewport_h),
                    }
                };
                app.log_ui.diff_scroll_y = app.log_ui.diff_scroll_y.min(max_y as u16);

                let x_scroll = if app.log_ui.diff_mode == GitDiffMode::Unified && !wrap {
                    app.log_ui.diff_scroll_x
                } else {
                    0
                };
                let mut diff_para = Paragraph::new(diff_lines)
                    .block(diff_block)
                    .scroll((app.log_ui.diff_scroll_y, x_scroll));
                if wrap {
                    diff_para = diff_para.wrap(Wrap { trim: false });
                }

                f.render_widget(diff_para, diff_area);
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
        }
    }

    if let Some(commit_area) = commit_area {
        if app.commit.open {
            let commit_block = Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(app.palette.accent_primary))
                .title(" Commit ");
            f.render_widget(commit_block.clone(), commit_area);

            let inner = commit_area.inner(Margin {
                vertical: 1,
                horizontal: 1,
            });

            let rows = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(1),
                    Constraint::Length(5),
                    Constraint::Length(1),
                    Constraint::Length(1),
                    Constraint::Length(1),
                ])
                .split(inner);

            let model =
                env::var("OPENROUTER_MODEL").unwrap_or_else(|_| "openai/gpt-5.2".to_string());
            let header = Paragraph::new(format!("Message    AI: {}", model)).style(
                Style::default()
                    .fg(app.palette.fg)
                    .add_modifier(Modifier::BOLD),
            );
            f.render_widget(header, rows[0]);

            let input_border = if app.commit.focus == CommitFocus::Message {
                app.palette.accent_primary
            } else {
                app.palette.border_inactive
            };
            let input_block = Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(input_border))
                .title(" Commit Message ");

            let input_inner = rows[1].inner(Margin {
                vertical: 1,
                horizontal: 1,
            });
            app.commit
                .ensure_cursor_visible(input_inner.height as usize);

            let input_lines: Vec<Line> = if app.commit.message.is_empty() {
                vec![Line::from(Span::styled(
                    "Type commit message...",
                    Style::default().fg(app.palette.border_inactive),
                ))]
            } else {
                app.commit.message.lines().map(Line::raw).collect()
            };

            let input = Paragraph::new(input_lines)
                .block(input_block)
                .wrap(Wrap { trim: false })
                .scroll((app.commit.scroll_y, 0));
            f.render_widget(input, rows[1]);

            zones.push(ClickZone {
                rect: rows[1],
                action: AppAction::FocusCommitMessage,
            });

            if app.commit.focus == CommitFocus::Message {
                let (line, col) = app.commit.cursor_line_col();
                let rel_y = (line as i64 - app.commit.scroll_y as i64).max(0) as u16;
                let cursor_y = input_inner.y.saturating_add(rel_y);
                let cursor_x = input_inner
                    .x
                    .saturating_add(col as u16)
                    .min(input_inner.x + input_inner.width.saturating_sub(1));
                if cursor_y >= input_inner.y && cursor_y < input_inner.y + input_inner.height {
                    f.set_cursor_position((cursor_x, cursor_y));
                }
            }

            let status_text = app.commit.status.as_deref().unwrap_or(if app.commit.busy {
                "Working..."
            } else {
                ""
            });
            f.render_widget(
                Paragraph::new(status_text).style(Style::default().fg(app.palette.fg)),
                rows[2],
            );

            let mut x = rows[3].x;
            for (label, action, color, enabled) in [
                (
                    " AI Generate ",
                    AppAction::GenerateCommitMessage,
                    app.palette.accent_tertiary,
                    !app.commit.busy,
                ),
                (
                    " Commit ",
                    AppAction::GitFooter(GitFooterAction::Commit),
                    app.palette.accent_secondary,
                    !app.commit.busy,
                ),
                (
                    " Close ",
                    AppAction::ToggleCommitDrawer,
                    app.palette.btn_bg,
                    true,
                ),
            ] {
                let w = label.len() as u16;
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
                let rect = Rect::new(x, rows[3].y, w, 1);
                f.render_widget(Paragraph::new(label).style(style), rect);
                if enabled {
                    zones.push(ClickZone { rect, action });
                }
                x += w + 2;
            }

            f.render_widget(
                Paragraph::new("Ctrl+G AI  Ctrl+Enter commit  Esc close")
                    .style(Style::default().fg(app.palette.border_inactive)),
                rows[4],
            );
        } else {
            let sep = Block::default()
                .borders(Borders::TOP)
                .border_style(Style::default().fg(app.palette.border_inactive));
            f.render_widget(sep, commit_area);

            let label = " Commit ▸ ";
            let w = label.len().min(commit_area.width as usize) as u16;
            f.render_widget(
                Paragraph::new(label).style(
                    Style::default()
                        .fg(app.palette.fg)
                        .add_modifier(Modifier::BOLD),
                ),
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
        .border_style(Style::default().fg(app.palette.border_inactive));
    f.render_widget(footer_block, footer_area);

    let btn_y = footer_area.y + 1;
    let mut btn_x = footer_area.x + 2;

    let mut buttons: Vec<(String, AppAction, Color, bool)> = Vec::new();
    match app.current_tab {
        Tab::Explorer => {
            buttons.push((
                " Menu (^P) ".to_string(),
                AppAction::OpenCommandPalette,
                app.palette.accent_primary,
                true,
            ));
            buttons.push((
                " ⬅ Back (h) ".to_string(),
                AppAction::GoParent,
                app.palette.accent_primary,
                true,
            ));
            buttons.push((
                " ⏎ Enter (l) ".to_string(),
                AppAction::EnterDir,
                app.palette.accent_secondary,
                true,
            ));
            buttons.push((
                " 👁 Hidden (.) ".to_string(),
                AppAction::ToggleHidden,
                app.palette.accent_tertiary,
                true,
            ));
            buttons.push((
                " ✖ Quit (q) ".to_string(),
                AppAction::Quit,
                app.palette.btn_bg,
                true,
            ));
        }
        Tab::Git => {
            buttons.push((
                " Menu (^P) ".to_string(),
                AppAction::OpenCommandPalette,
                app.palette.accent_primary,
                true,
            ));

            let enabled = app.pending_job.is_none() && !app.commit.busy && !app.branch_ui.open;
            let in_conflict_view = app.git.selected_entry().is_some_and(|e| e.is_conflict);

            if in_conflict_view {
                buttons.push((
                    " < Prev (p) ".to_string(),
                    AppAction::ConflictPrev,
                    app.palette.accent_tertiary,
                    enabled,
                ));
                buttons.push((
                    " Next (n) > ".to_string(),
                    AppAction::ConflictNext,
                    app.palette.accent_tertiary,
                    enabled,
                ));
                buttons.push((
                    " Ours (o) ".to_string(),
                    AppAction::ConflictUseOurs,
                    app.palette.accent_primary,
                    enabled,
                ));
                buttons.push((
                    " Theirs (t) ".to_string(),
                    AppAction::ConflictUseTheirs,
                    app.palette.accent_secondary,
                    enabled,
                ));
                buttons.push((
                    " Both (b) ".to_string(),
                    AppAction::ConflictUseBoth,
                    app.palette.accent_tertiary,
                    enabled,
                ));
                buttons.push((
                    " Mark (a) ".to_string(),
                    AppAction::MarkResolved,
                    app.palette.exe_color,
                    enabled,
                ));
                buttons.push((
                    " ✎ Commit… ".to_string(),
                    AppAction::ToggleCommitDrawer,
                    app.palette.accent_primary,
                    true,
                ));
            } else {
                buttons.push((
                    " ␠ Toggle ".to_string(),
                    AppAction::ToggleGitStage,
                    app.palette.accent_primary,
                    enabled,
                ));
                buttons.push((
                    " + Stage ".to_string(),
                    AppAction::GitFooter(GitFooterAction::Stage),
                    app.palette.accent_secondary,
                    enabled,
                ));
                buttons.push((
                    " - Unstage ".to_string(),
                    AppAction::GitFooter(GitFooterAction::Unstage),
                    app.palette.accent_tertiary,
                    enabled,
                ));
                buttons.push((
                    " ↩ Discard ".to_string(),
                    AppAction::GitFooter(GitFooterAction::Discard),
                    app.palette.btn_bg,
                    enabled,
                ));
                buttons.push((
                    " + All (A) ".to_string(),
                    AppAction::GitStageAllVisible,
                    app.palette.accent_secondary,
                    enabled,
                ));
                buttons.push((
                    " - All (U) ".to_string(),
                    AppAction::GitUnstageAllVisible,
                    app.palette.accent_tertiary,
                    enabled,
                ));
                buttons.push((
                    " Branch (B) ".to_string(),
                    AppAction::OpenBranchPicker,
                    app.palette.accent_tertiary,
                    enabled,
                ));
                buttons.push((
                    " ✎ Commit… ".to_string(),
                    AppAction::ToggleCommitDrawer,
                    app.palette.accent_primary,
                    true,
                ));
            }

            buttons.push((
                " ✖ Quit (q) ".to_string(),
                AppAction::Quit,
                app.palette.btn_bg,
                true,
            ));
        }
        Tab::Log => {
            buttons.push((
                " Menu (^P) ".to_string(),
                AppAction::OpenCommandPalette,
                app.palette.accent_primary,
                true,
            ));
            buttons.push((
                " History (h) ".to_string(),
                AppAction::LogSwitch(LogSubTab::History),
                app.palette.accent_tertiary,
                true,
            ));
            buttons.push((
                " Reflog (r) ".to_string(),
                AppAction::LogSwitch(LogSubTab::Reflog),
                app.palette.accent_tertiary,
                true,
            ));
            buttons.push((
                " Cmd (c) ".to_string(),
                AppAction::LogSwitch(LogSubTab::Commands),
                app.palette.accent_tertiary,
                true,
            ));
            buttons.push((
                " Diff (d) ".to_string(),
                AppAction::LogDetail(LogDetailMode::Diff),
                app.palette.accent_primary,
                app.log_ui.subtab != LogSubTab::Commands,
            ));
            buttons.push((
                " Changed (f) ".to_string(),
                AppAction::LogDetail(LogDetailMode::Files),
                app.palette.accent_primary,
                app.log_ui.subtab != LogSubTab::Commands,
            ));
            buttons.push((
                " Inspect (i) ".to_string(),
                AppAction::LogInspect,
                app.palette.accent_secondary,
                true,
            ));
            buttons.push((
                " Zoom (z) ".to_string(),
                AppAction::LogToggleZoom,
                app.palette.accent_tertiary,
                true,
            ));
            buttons.push((
                " < ([) ".to_string(),
                AppAction::LogAdjustLeft(-2),
                app.palette.btn_bg,
                app.log_ui.zoom == LogZoom::None,
            ));
            buttons.push((
                " > (]) ".to_string(),
                AppAction::LogAdjustLeft(2),
                app.palette.btn_bg,
                app.log_ui.zoom == LogZoom::None,
            ));
            buttons.push((
                " Clear Cmd (x) ".to_string(),
                AppAction::ClearGitLog,
                app.palette.btn_bg,
                app.log_ui.subtab == LogSubTab::Commands,
            ));
            buttons.push((
                " ✖ Quit (q) ".to_string(),
                AppAction::Quit,
                app.palette.btn_bg,
                true,
            ));
        }
    }

    let available = footer_area.width.saturating_sub(4);
    loop {
        let total: u16 = buttons
            .iter()
            .map(|(label, _, _, _)| label.len() as u16 + 2)
            .sum();
        if total <= available || buttons.len() <= 1 {
            break;
        }
        let drop_idx = buttons.len().saturating_sub(2);
        buttons.remove(drop_idx);
    }

    for (label, action, color, enabled) in buttons {
        let width = label.len() as u16;
        if btn_x + width >= footer_area.x + footer_area.width {
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
        let btn_style = Style::default().bg(bg).fg(fg).add_modifier(Modifier::BOLD);

        f.render_widget(
            Paragraph::new(label.as_str()).style(btn_style),
            Rect::new(btn_x, btn_y, width, 1),
        );

        if enabled {
            zones.push(ClickZone {
                rect: Rect::new(btn_x, btn_y, width, 1),
                action,
            });
        }

        btn_x += width + 2;
    }

    if let Some((msg, _)) = app.status_message.as_ref() {
        let used = btn_x.saturating_sub(footer_area.x);
        let available = footer_area.width.saturating_sub(used).saturating_sub(2);
        if available > 0 {
            f.render_widget(
                Paragraph::new(msg.as_str()).style(Style::default().fg(app.palette.fg)),
                Rect::new(btn_x, btn_y, available, 1),
            );
        }
    } else if app.current_tab == Tab::Git && app.git.selected_entry().is_some_and(|e| e.is_conflict)
    {
        let hint = "Conflicts: n/p block  o/t/b apply  a stage";
        let used = btn_x.saturating_sub(footer_area.x);
        let available = footer_area.width.saturating_sub(used).saturating_sub(2);
        if available > 0 {
            let w = hint.len().min(available as usize) as u16;
            f.render_widget(
                Paragraph::new(hint).style(Style::default().fg(app.palette.border_inactive)),
                Rect::new(btn_x, btn_y, w, 1),
            );
        }
    } else {
        let hint = "Ctrl+P menu  T theme";
        let used = btn_x.saturating_sub(footer_area.x);
        let available = footer_area.width.saturating_sub(used).saturating_sub(2);
        if available > 0 {
            let w = hint.len().min(available as usize) as u16;
            f.render_widget(
                Paragraph::new(hint).style(Style::default().fg(app.palette.border_inactive)),
                Rect::new(btn_x, btn_y, w, 1),
            );
        }
    }

    if app.branch_ui.open {
        let w = area.width.min(84).saturating_sub(2).max(50);
        let h = area.height.min(20).saturating_sub(2).max(10);
        let x = area.x + (area.width.saturating_sub(w)) / 2;
        let y = area.y + (area.height.saturating_sub(h)) / 2;
        let modal = Rect::new(x, y, w, h);

        zones.push(ClickZone {
            rect: area,
            action: AppAction::CloseBranchPicker,
        });

        f.render_widget(Clear, modal);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(app.palette.accent_primary))
            .title(" Checkout Branch ");
        f.render_widget(block.clone(), modal);

        let inner = modal.inner(Margin {
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

        let query = Paragraph::new(format!("Filter: {}", app.branch_ui.query))
            .style(Style::default().fg(app.palette.fg));
        f.render_widget(query, rows[0]);

        let list_items: Vec<ListItem> = app
            .branch_ui
            .filtered
            .iter()
            .map(|idx| {
                let b = &app.branch_ui.branches[*idx];
                let cur = if b.is_current { "* " } else { "  " };
                let kind = if b.is_remote { "[R] " } else { "[L] " };
                let mut s = format!("{}{}{}", cur, kind, b.name);
                if let Some(up) = &b.upstream {
                    s.push_str("  ");
                    s.push_str(up);
                }
                if let Some(tr) = &b.track {
                    s.push_str("  ");
                    s.push_str(tr);
                }
                ListItem::new(s)
            })
            .collect();

        let list = List::new(list_items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(app.palette.border_inactive))
                    .title(" Branches "),
            )
            .highlight_style(
                Style::default()
                    .bg(app.palette.accent_primary)
                    .fg(app.palette.btn_fg)
                    .add_modifier(Modifier::BOLD),
            );
        f.render_stateful_widget(list, rows[1], &mut app.branch_ui.list_state);

        let list_inner = rows[1].inner(Margin {
            vertical: 1,
            horizontal: 1,
        });
        let start = app.branch_ui.list_state.offset();
        let end = (start + list_inner.height as usize).min(app.branch_ui.filtered.len());
        for (i, idx) in (start..end).enumerate() {
            let rect = Rect::new(list_inner.x, list_inner.y + i as u16, list_inner.width, 1);
            zones.push(ClickZone {
                rect,
                action: AppAction::SelectBranch(idx),
            });
        }

        let mut x = rows[2].x;
        for (label, action, color) in [
            (
                " Checkout ",
                AppAction::BranchCheckout,
                app.palette.accent_secondary,
            ),
            (" Close ", AppAction::CloseBranchPicker, app.palette.btn_bg),
        ] {
            let w = label.len() as u16;
            let rect = Rect::new(x, rows[2].y, w, 1);
            let style = Style::default()
                .bg(color)
                .fg(app.palette.btn_fg)
                .add_modifier(Modifier::BOLD);
            f.render_widget(Paragraph::new(label).style(style), rect);
            zones.push(ClickZone { rect, action });
            x += w + 2;
        }

        if let Some(msg) = app.branch_ui.status.as_deref() {
            f.render_widget(
                Paragraph::new(msg).style(Style::default().fg(app.palette.btn_bg)),
                Rect::new(
                    rows[2].x + 30,
                    rows[2].y,
                    rows[2].width.saturating_sub(30),
                    1,
                ),
            );
        }

        if let Some(pending) = app.branch_ui.confirm_checkout.as_deref() {
            let w = modal.width.min(70).saturating_sub(2).max(40);
            let h = 7u16.min(modal.height.saturating_sub(2)).max(7);
            let x = modal.x + (modal.width.saturating_sub(w)) / 2;
            let y = modal.y + (modal.height.saturating_sub(h)) / 2;
            let confirm = Rect::new(x, y, w, h);

            f.render_widget(Clear, confirm);

            let block = Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(app.palette.btn_bg))
                .title(" Uncommitted Changes ");
            f.render_widget(block.clone(), confirm);

            let inner = confirm.inner(Margin {
                vertical: 1,
                horizontal: 2,
            });

            let text = vec![
                Line::raw("Working tree has changes."),
                Line::raw(""),
                Line::raw(format!("Checkout `{}` anyway?", pending)),
            ];
            f.render_widget(
                Paragraph::new(text).style(Style::default().fg(app.palette.fg)),
                Rect::new(
                    inner.x,
                    inner.y,
                    inner.width,
                    inner.height.saturating_sub(1),
                ),
            );

            let by = inner.y + inner.height.saturating_sub(1);
            let mut bx = inner.x;
            for (label, action, color) in [
                (
                    " Checkout ",
                    AppAction::ConfirmBranchCheckout,
                    app.palette.accent_secondary,
                ),
                (
                    " Cancel ",
                    AppAction::CancelBranchCheckout,
                    app.palette.btn_bg,
                ),
            ] {
                let w = label.len() as u16;
                let rect = Rect::new(bx, by, w, 1);
                let style = Style::default()
                    .bg(color)
                    .fg(app.palette.btn_fg)
                    .add_modifier(Modifier::BOLD);
                f.render_widget(Paragraph::new(label).style(style), rect);
                zones.push(ClickZone { rect, action });
                bx += w + 2;
            }
        }
    }

    if let Some(menu) = &app.context_menu {
        let width = 30u16.min(area.width.saturating_sub(2));
        let height = (menu.options.len() as u16 + 2).min(area.height.saturating_sub(2));
        if width < 6 || height < 4 {
        } else {
            let max_x = area
                .x
                .saturating_add(area.width)
                .saturating_sub(width)
                .saturating_sub(1);
            let max_y = area
                .y
                .saturating_add(area.height)
                .saturating_sub(height)
                .saturating_sub(1);

            let menu_area = Rect::new(menu.x.min(max_x), menu.y.min(max_y), width, height);

            f.render_widget(Clear, menu_area);

            let block = Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(app.palette.accent_secondary))
                .bg(app.palette.menu_bg);

            f.render_widget(block.clone(), menu_area);

            let inner = menu_area.inner(Margin {
                vertical: 1,
                horizontal: 1,
            });

            let visible = (inner.height as usize).min(menu.options.len());
            for (i, (label, _)) in menu.options.iter().take(visible).enumerate() {
                let item_area = Rect::new(inner.x, inner.y + i as u16, inner.width, 1);

                let is_selected = i == menu.selected;
                let style = if is_selected {
                    Style::default()
                        .bg(app.palette.selection_bg)
                        .fg(app.palette.fg)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(app.palette.fg)
                };

                f.render_widget(Paragraph::new(label.as_str()).style(style), item_area);

                zones.push(ClickZone {
                    rect: item_area,
                    action: AppAction::ContextMenuAction(i),
                });
            }
        }
    }

    if app.discard_confirm.is_none()
        && !app.branch_ui.open
        && app.context_menu.is_none()
        && !app.log_ui.inspect.open
        && app.operation_popup.is_none()
    {
        if app.command_palette.open {
            let w = area.width.min(56).saturating_sub(2).max(32);
            let desired_h = COMMAND_PALETTE_ITEMS.len() as u16 + 6;
            let h = desired_h.min(area.height.saturating_sub(2)).max(10);
            let x = area.x + (area.width.saturating_sub(w)) / 2;
            let y = area.y + (area.height.saturating_sub(h)) / 2;
            let modal = Rect::new(x, y, w, h);

            f.render_widget(Clear, modal);

            let block = Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(app.palette.accent_primary))
                .title(" Menu (Ctrl+P) ");
            f.render_widget(block.clone(), modal);

            let inner = modal.inner(Margin {
                vertical: 1,
                horizontal: 2,
            });

            let rows = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(0), Constraint::Length(1)])
                .split(inner);

            let list_items: Vec<ListItem> = COMMAND_PALETTE_ITEMS
                .iter()
                .map(|(_, label)| ListItem::new(format!("  {}", label)))
                .collect();

            let list = List::new(list_items)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_type(BorderType::Rounded)
                        .border_style(Style::default().fg(app.palette.border_inactive))
                        .title(" Commands "),
                )
                .highlight_style(
                    Style::default()
                        .bg(app.palette.accent_primary)
                        .fg(app.palette.btn_fg)
                        .add_modifier(Modifier::BOLD),
                );
            f.render_stateful_widget(list, rows[0], &mut app.command_palette.list_state);

            let hint = "j/k move  Enter run  Esc close";
            f.render_widget(
                Paragraph::new(hint).style(Style::default().fg(app.palette.border_inactive)),
                rows[1],
            );
        }

        if app.theme_picker.open {
            let w = 35u16.min(area.width.saturating_sub(2)).max(30);
            let h = 11u16.min(area.height.saturating_sub(2)).max(9);
            let x = area.x + (area.width.saturating_sub(w)) / 2;
            let y = area.y + (area.height.saturating_sub(h)) / 2;
            let modal = Rect::new(x, y, w, h);

            f.render_widget(Clear, modal);

            let block = Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(app.palette.accent_primary))
                .title(" Select Theme ");
            f.render_widget(block.clone(), modal);

            let inner = modal.inner(Margin {
                vertical: 1,
                horizontal: 2,
            });

            let rows = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(0), Constraint::Length(1)])
                .split(inner);

            let list_items: Vec<ListItem> = THEME_ORDER
                .iter()
                .enumerate()
                .map(|(i, t)| {
                    let current = if *t == app.theme { "*" } else { " " };
                    ListItem::new(format!("{} {} {}", current, i + 1, t.label()))
                })
                .collect();

            let list = List::new(list_items)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_type(BorderType::Rounded)
                        .border_style(Style::default().fg(app.palette.border_inactive))
                        .title(" Themes "),
                )
                .highlight_style(
                    Style::default()
                        .bg(app.palette.accent_primary)
                        .fg(app.palette.btn_fg)
                        .add_modifier(Modifier::BOLD),
                );
            f.render_stateful_widget(list, rows[0], &mut app.theme_picker.list_state);

            let hint = "j/k move  Enter apply  1-5 quick  Esc";
            f.render_widget(
                Paragraph::new(hint).style(Style::default().fg(app.palette.border_inactive)),
                rows[1],
            );
        }
    }

    if app.discard_confirm.is_none() && app.log_ui.inspect.open {
        zones.push(ClickZone {
            rect: area,
            action: AppAction::LogCloseInspect,
        });

        let w = area.width.min(90).saturating_sub(2).max(50);
        let h = area.height.min(18).saturating_sub(2).max(8);
        let x = area.x + (area.width.saturating_sub(w)) / 2;
        let y = area.y + (area.height.saturating_sub(h)) / 2;
        let modal = Rect::new(x, y, w, h);

        f.render_widget(Clear, modal);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(app.palette.accent_secondary))
            .title(app.log_ui.inspect.title.as_str());
        f.render_widget(block.clone(), modal);

        let inner = modal.inner(Margin {
            vertical: 1,
            horizontal: 2,
        });

        let body_h = inner.height.saturating_sub(1);
        let body_area = Rect::new(inner.x, inner.y, inner.width, body_h);
        let buttons_y = inner.y + body_h;

        let para = Paragraph::new(app.log_ui.inspect.body.as_str())
            .wrap(Wrap { trim: false })
            .scroll((app.log_ui.inspect.scroll_y, 0));
        f.render_widget(para, body_area);

        let primary_label = match app.log_ui.subtab {
            LogSubTab::Commands => " Copy Cmd (y) ".to_string(),
            _ => " Copy SHA (y) ".to_string(),
        };
        let secondary_label = match app.log_ui.subtab {
            LogSubTab::Commands => " Copy Output (Y) ".to_string(),
            _ => " Copy Subject (Y) ".to_string(),
        };

        let mut bx = inner.x;
        for (label, action, color) in [
            (
                primary_label.as_str(),
                AppAction::LogInspectCopyPrimary,
                app.palette.accent_primary,
            ),
            (
                secondary_label.as_str(),
                AppAction::LogInspectCopySecondary,
                app.palette.accent_tertiary,
            ),
            (" Close ", AppAction::LogCloseInspect, app.palette.btn_bg),
        ] {
            let bw = label.len() as u16;
            let rect = Rect::new(bx, buttons_y, bw, 1);
            let style = Style::default()
                .bg(color)
                .fg(app.palette.btn_fg)
                .add_modifier(Modifier::BOLD);
            f.render_widget(Paragraph::new(label).style(style), rect);
            zones.push(ClickZone { rect, action });
            bx += bw + 2;
        }
    }

    if app.discard_confirm.is_none() && !app.log_ui.inspect.open {
        if let Some(popup) = &app.operation_popup {
            zones.push(ClickZone {
                rect: area,
                action: AppAction::CloseOperationPopup,
            });

            let w = area.width.min(90).saturating_sub(2).max(44);
            let h = area.height.min(14).saturating_sub(2).max(7);
            let x = area.x + (area.width.saturating_sub(w)) / 2;
            let y = area.y + (area.height.saturating_sub(h)) / 2;
            let modal = Rect::new(x, y, w, h);

            f.render_widget(Clear, modal);

            let border = if popup.ok {
                app.palette.accent_secondary
            } else {
                app.palette.btn_bg
            };
            let block = Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(border))
                .title(popup.title.as_str());
            f.render_widget(block.clone(), modal);

            let inner = modal.inner(Margin {
                vertical: 1,
                horizontal: 2,
            });

            let body_h = inner.height.saturating_sub(1);
            let body_area = Rect::new(inner.x, inner.y, inner.width, body_h);
            let buttons_y = inner.y + body_h;

            let para = Paragraph::new(popup.body.as_str())
                .wrap(Wrap { trim: false })
                .scroll((popup.scroll_y, 0));
            f.render_widget(para, body_area);

            let label = " Close (Esc) ";
            let bw = label.len() as u16;
            let rect = Rect::new(inner.x, buttons_y, bw, 1);
            let style = Style::default()
                .bg(app.palette.btn_bg)
                .fg(app.palette.btn_fg)
                .add_modifier(Modifier::BOLD);
            f.render_widget(Paragraph::new(label).style(style), rect);
            zones.push(ClickZone {
                rect,
                action: AppAction::CloseOperationPopup,
            });
        }
    }

    if let Some(confirm) = &app.discard_confirm {
        let w = area.width.min(70).saturating_sub(2).max(40);
        let h = 9u16.min(area.height.saturating_sub(2)).max(7);
        let x = area.x + (area.width.saturating_sub(w)) / 2;
        let y = area.y + (area.height.saturating_sub(h)) / 2;
        let modal = Rect::new(x, y, w, h);

        zones.push(ClickZone {
            rect: area,
            action: AppAction::CancelDiscard,
        });

        f.render_widget(Clear, modal);

        let n = confirm.items.len();
        let title = if n == 1 {
            match confirm.items[0].mode {
                DiscardMode::Worktree => " Discard Changes ",
                DiscardMode::Untracked => " Delete Untracked ",
                DiscardMode::AllChanges => " Discard All Changes ",
            }
        } else {
            " Discard "
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(app.palette.btn_bg))
            .title(title);
        f.render_widget(block.clone(), modal);

        let inner = modal.inner(Margin {
            vertical: 1,
            horizontal: 2,
        });

        let mut work = 0usize;
        let mut all = 0usize;
        let mut untracked = 0usize;
        for item in &confirm.items {
            match item.mode {
                DiscardMode::Worktree => work += 1,
                DiscardMode::Untracked => untracked += 1,
                DiscardMode::AllChanges => all += 1,
            }
        }

        let mut lines = Vec::new();
        if n == 1 {
            lines.push(Line::raw(format!("File: {}", confirm.items[0].path)));
        } else {
            lines.push(Line::raw(format!("Files: {}", n)));
        }
        lines.push(Line::raw(""));
        if work > 0 {
            lines.push(Line::raw(format!("Revert unstaged: {}", work)));
        }
        if all > 0 {
            lines.push(Line::raw(format!("Reset staged+unstaged: {}", all)));
        }
        if untracked > 0 {
            lines.push(Line::raw(format!("Delete untracked: {}", untracked)));
        }
        lines.push(Line::raw(""));
        lines.push(Line::raw("Confirm? (y/n)"));

        let text_h = inner.height.saturating_sub(2);
        f.render_widget(
            Paragraph::new(lines)
                .style(Style::default().fg(app.palette.fg))
                .wrap(Wrap { trim: false }),
            Rect::new(inner.x, inner.y, inner.width, text_h),
        );

        let buttons_y = inner.y + inner.height.saturating_sub(1);
        let mut bx = inner.x;
        for (label, action, color) in [
            (" Discard ", AppAction::ConfirmDiscard, app.palette.btn_bg),
            (
                " Cancel ",
                AppAction::CancelDiscard,
                app.palette.border_inactive,
            ),
        ] {
            let bw = label.len() as u16;
            let style = Style::default()
                .bg(color)
                .fg(app.palette.btn_fg)
                .add_modifier(Modifier::BOLD);
            let rect = Rect::new(bx, buttons_y, bw, 1);
            f.render_widget(Paragraph::new(label).style(style), rect);
            zones.push(ClickZone { rect, action });
            bx += bw + 2;
        }
    }

    zones
}

fn main() -> io::Result<()> {
    let _ = dotenvy::dotenv();

    let start_path = env::args()
        .nth(1)
        .map(PathBuf::from)
        .or_else(|| env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("/"));

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
        app.tick_pending_menu_action();
        app.poll_pending_job();
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
                    KeyCode::Char('1')
                        if app.operation_popup.is_none()
                            && !app.theme_picker.open
                            && !app.command_palette.open =>
                    {
                        app.current_tab = Tab::Explorer;
                    }
                    KeyCode::Char('2')
                        if app.operation_popup.is_none()
                            && !app.theme_picker.open
                            && !app.command_palette.open =>
                    {
                        app.current_tab = Tab::Git;
                        app.git.refresh(&app.current_path);
                        app.update_git_operation();
                    }
                    KeyCode::Char('3')
                        if app.operation_popup.is_none()
                            && !app.theme_picker.open
                            && !app.command_palette.open =>
                    {
                        app.current_tab = Tab::Log;
                        app.refresh_log_data();
                    }
                    KeyCode::Char('p')
                        if key.modifiers.contains(KeyModifiers::CONTROL)
                            && app.operation_popup.is_none()
                            && app.discard_confirm.is_none()
                            && !app.branch_ui.open
                            && app.context_menu.is_none()
                            && !app.log_ui.inspect.open =>
                    {
                        app.open_command_palette();
                    }
                    KeyCode::Char('T')
                        if app.operation_popup.is_none()
                            && app.discard_confirm.is_none()
                            && !app.branch_ui.open
                            && app.context_menu.is_none()
                            && !app.log_ui.inspect.open =>
                    {
                        app.open_theme_picker();
                    }
                    KeyCode::Esc => {
                        app.context_menu = None;
                        app.discard_confirm = None;
                        app.operation_popup = None;
                        app.theme_picker.open = false;
                        app.command_palette.open = false;
                        app.log_ui.inspect.close();
                        if app.branch_ui.open {
                            if app.branch_ui.confirm_checkout.is_some() {
                                app.branch_ui.confirm_checkout = None;
                            } else {
                                app.close_branch_picker();
                            }
                        }
                        if app.current_tab == Tab::Git {
                            app.commit.open = false;
                        }
                    }
                    _ => {
                        if app.theme_picker.open {
                            match key.code {
                                KeyCode::Char('j') | KeyCode::Down => app.move_theme_picker(1),
                                KeyCode::Char('k') | KeyCode::Up => app.move_theme_picker(-1),
                                KeyCode::Enter => app.apply_theme_picker_selection(),
                                KeyCode::Char(ch) if ('1'..='5').contains(&ch) => {
                                    let idx =
                                        ch.to_digit(10).unwrap_or(1).saturating_sub(1) as usize;
                                    if idx < THEME_ORDER.len() {
                                        app.theme_picker.list_state.select(Some(idx));
                                        app.apply_theme_picker_selection();
                                    }
                                }
                                _ => {}
                            }
                        } else if app.command_palette.open {
                            match key.code {
                                KeyCode::Char('j') | KeyCode::Down => app.move_command_palette(1),
                                KeyCode::Char('k') | KeyCode::Up => app.move_command_palette(-1),
                                KeyCode::Enter => app.run_command_palette_selection(),
                                _ => {}
                            }
                        } else if let Some(popup) = &mut app.operation_popup {
                            match key.code {
                                KeyCode::Esc | KeyCode::Enter => app.operation_popup = None,
                                KeyCode::Char('j') | KeyCode::Down => {
                                    popup.scroll_y = popup.scroll_y.saturating_add(3)
                                }
                                KeyCode::Char('k') | KeyCode::Up => {
                                    popup.scroll_y = popup.scroll_y.saturating_sub(3)
                                }
                                _ => {}
                            }
                        } else {
                            match app.current_tab {
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
                                    KeyCode::Char('i') => app.add_selected_to_gitignore(),
                                    KeyCode::Char('H') => {
                                        app.syntax_highlight = !app.syntax_highlight;
                                        app.set_status(if app.syntax_highlight {
                                            "Syntax highlight: on"
                                        } else {
                                            "Syntax highlight: off"
                                        });
                                    }
                                    _ => {}
                                },
                                Tab::Git => {
                                    if app.discard_confirm.is_some() {
                                        match key.code {
                                            KeyCode::Char('y') | KeyCode::Char('Y') => {
                                                app.confirm_discard()
                                            }
                                            KeyCode::Char('n') | KeyCode::Char('N') => {
                                                app.discard_confirm = None;
                                            }
                                            _ => {}
                                        }
                                    } else if app.branch_ui.open {
                                        if app.branch_ui.confirm_checkout.is_some() {
                                            match key.code {
                                                KeyCode::Enter => {
                                                    app.branch_checkout_selected(true)
                                                }
                                                KeyCode::Esc
                                                | KeyCode::Char('n')
                                                | KeyCode::Char('N') => {
                                                    app.branch_ui.confirm_checkout = None;
                                                }
                                                _ => {}
                                            }
                                        } else {
                                            match key.code {
                                                KeyCode::Esc => app.close_branch_picker(),
                                                KeyCode::Enter => {
                                                    app.branch_checkout_selected(false)
                                                }
                                                KeyCode::Char('j') | KeyCode::Down => {
                                                    app.branch_ui.move_selection(1)
                                                }
                                                KeyCode::Char('k') | KeyCode::Up => {
                                                    app.branch_ui.move_selection(-1)
                                                }
                                                KeyCode::Backspace => {
                                                    app.branch_ui.query.pop();
                                                    app.branch_ui.update_filtered();
                                                }
                                                KeyCode::Char(ch)
                                                    if !key
                                                        .modifiers
                                                        .contains(KeyModifiers::CONTROL)
                                                        && !key
                                                            .modifiers
                                                            .contains(KeyModifiers::ALT) =>
                                                {
                                                    app.branch_ui.query.push(ch);
                                                    app.branch_ui.update_filtered();
                                                }
                                                _ => {}
                                            }
                                        }
                                    } else if app.commit.open {
                                        if key.modifiers.contains(KeyModifiers::CONTROL)
                                            && matches!(
                                                key.code,
                                                KeyCode::Char('g') | KeyCode::Char('G')
                                            )
                                        {
                                            app.start_ai_generate();
                                        } else if key.modifiers.contains(KeyModifiers::CONTROL)
                                            && key.code == KeyCode::Enter
                                        {
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
                                                    if !key
                                                        .modifiers
                                                        .contains(KeyModifiers::CONTROL)
                                                        && !key
                                                            .modifiers
                                                            .contains(KeyModifiers::ALT) =>
                                                {
                                                    app.commit.insert_char(ch);
                                                }
                                                _ => {}
                                            }
                                        }
                                    } else {
                                        match key.code {
                                            KeyCode::Char(' ') => app.toggle_stage_for_selection(),
                                            KeyCode::Char('A') => app.stage_all_visible(),
                                            KeyCode::Char('U') => app.unstage_all_visible(),
                                            KeyCode::Char('a')
                                                if key
                                                    .modifiers
                                                    .contains(KeyModifiers::CONTROL) =>
                                            {
                                                app.select_all_git_filtered();
                                            }
                                            KeyCode::Char('r') => app.refresh_git_state(),
                                            KeyCode::Char('i') => app.add_selected_to_gitignore(),
                                            KeyCode::Char('w') => {
                                                app.wrap_diff = !app.wrap_diff;
                                                app.set_status(if app.wrap_diff {
                                                    "Diff wrap: on (unified only)"
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
                                            KeyCode::Char('c') => {
                                                app.commit.open = true;
                                                app.commit.focus = CommitFocus::Message;
                                            }
                                            KeyCode::Char('n')
                                                if app
                                                    .git
                                                    .selected_entry()
                                                    .is_some_and(|e| e.is_conflict) =>
                                            {
                                                app.change_conflict_block(1)
                                            }
                                            KeyCode::Char('p')
                                                if app
                                                    .git
                                                    .selected_entry()
                                                    .is_some_and(|e| e.is_conflict) =>
                                            {
                                                app.change_conflict_block(-1)
                                            }
                                            KeyCode::Char('o')
                                                if app
                                                    .git
                                                    .selected_entry()
                                                    .is_some_and(|e| e.is_conflict) =>
                                            {
                                                app.apply_conflict_resolution(
                                                    ConflictResolution::Ours,
                                                )
                                            }
                                            KeyCode::Char('t')
                                                if app
                                                    .git
                                                    .selected_entry()
                                                    .is_some_and(|e| e.is_conflict) =>
                                            {
                                                app.apply_conflict_resolution(
                                                    ConflictResolution::Theirs,
                                                )
                                            }
                                            KeyCode::Char('b')
                                                if app
                                                    .git
                                                    .selected_entry()
                                                    .is_some_and(|e| e.is_conflict) =>
                                            {
                                                app.apply_conflict_resolution(
                                                    ConflictResolution::Both,
                                                )
                                            }
                                            KeyCode::Char('a')
                                                if app
                                                    .git
                                                    .selected_entry()
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

                                            KeyCode::Left => {
                                                app.git.diff_scroll_x =
                                                    app.git.diff_scroll_x.saturating_sub(4);
                                            }
                                            KeyCode::Right => {
                                                app.git.diff_scroll_x =
                                                    app.git.diff_scroll_x.saturating_add(4);
                                            }
                                            KeyCode::Char('j') | KeyCode::Down => {
                                                let i = app.git.list_state.selected().unwrap_or(0);
                                                if i + 1 < app.git.filtered.len() {
                                                    app.git
                                                        .select_filtered(i + 1, &app.current_path);
                                                }
                                            }
                                            KeyCode::Char('k') | KeyCode::Up => {
                                                let i = app.git.list_state.selected().unwrap_or(0);
                                                if i > 0 {
                                                    app.git
                                                        .select_filtered(i - 1, &app.current_path);
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
                                        }
                                    }
                                }
                                Tab::Log => {
                                    if app.log_ui.inspect.open {
                                        match key.code {
                                            KeyCode::Esc | KeyCode::Enter => {
                                                app.log_ui.inspect.close()
                                            }
                                            KeyCode::Char('j') | KeyCode::Down => {
                                                app.log_ui.inspect.scroll_y =
                                                    app.log_ui.inspect.scroll_y.saturating_add(3)
                                            }
                                            KeyCode::Char('k') | KeyCode::Up => {
                                                app.log_ui.inspect.scroll_y =
                                                    app.log_ui.inspect.scroll_y.saturating_sub(3)
                                            }
                                            KeyCode::Char('y') => {
                                                if let Some(s) = app
                                                    .selected_log_hash()
                                                    .or_else(|| app.selected_log_command())
                                                {
                                                    app.request_copy_to_clipboard(s);
                                                }
                                            }
                                            KeyCode::Char('Y') => {
                                                if let Some(s) = app.selected_log_subject() {
                                                    app.request_copy_to_clipboard(s);
                                                } else {
                                                    app.request_copy_to_clipboard(
                                                        app.log_ui.inspect.body.clone(),
                                                    );
                                                }
                                            }
                                            _ => {}
                                        }
                                    } else {
                                        match key.code {
                                            KeyCode::Char('h') => {
                                                app.set_log_subtab(LogSubTab::History)
                                            }
                                            KeyCode::Char('r') => {
                                                app.set_log_subtab(LogSubTab::Reflog)
                                            }
                                            KeyCode::Char('c') => {
                                                app.set_log_subtab(LogSubTab::Commands)
                                            }
                                            KeyCode::Char('x')
                                                if app.log_ui.subtab == LogSubTab::Commands =>
                                            {
                                                app.git_log.clear();
                                                app.log_ui.command_state.select(None);
                                                app.refresh_log_diff();
                                                app.set_status("Log cleared");
                                            }
                                            KeyCode::Char('d')
                                                if app.log_ui.subtab != LogSubTab::Commands =>
                                            {
                                                let next = match app.log_ui.detail_mode {
                                                    LogDetailMode::Diff => LogDetailMode::Files,
                                                    LogDetailMode::Files => LogDetailMode::Diff,
                                                };
                                                app.log_ui.set_detail_mode(next);
                                                app.refresh_log_diff();
                                            }
                                            KeyCode::Char('f')
                                                if app.log_ui.subtab != LogSubTab::Commands =>
                                            {
                                                app.log_ui.set_detail_mode(LogDetailMode::Files);
                                                app.refresh_log_diff();
                                            }
                                            KeyCode::Char('i') => app.open_log_inspect(),
                                            KeyCode::Char('z') => app.toggle_log_zoom(),
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
                                                    "Diff wrap: on (unified only)"
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
                                            KeyCode::Left => {
                                                app.log_ui.diff_scroll_x =
                                                    app.log_ui.diff_scroll_x.saturating_sub(4)
                                            }
                                            KeyCode::Right => {
                                                app.log_ui.diff_scroll_x =
                                                    app.log_ui.diff_scroll_x.saturating_add(4)
                                            }
                                            KeyCode::Char('j') | KeyCode::Down => match app
                                                .log_ui
                                                .focus
                                            {
                                                LogPaneFocus::Commits => app.move_log_selection(1),
                                                LogPaneFocus::Files => {
                                                    app.move_log_file_selection(1)
                                                }
                                                LogPaneFocus::Diff => {
                                                    app.log_ui.diff_scroll_y =
                                                        app.log_ui.diff_scroll_y.saturating_add(1)
                                                }
                                            },
                                            KeyCode::Char('k') | KeyCode::Up => match app
                                                .log_ui
                                                .focus
                                            {
                                                LogPaneFocus::Commits => app.move_log_selection(-1),
                                                LogPaneFocus::Files => {
                                                    app.move_log_file_selection(-1)
                                                }
                                                LogPaneFocus::Diff => {
                                                    app.log_ui.diff_scroll_y =
                                                        app.log_ui.diff_scroll_y.saturating_sub(1)
                                                }
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
                                                LogPaneFocus::Diff => {
                                                    app.log_ui.diff_scroll_y = u16::MAX
                                                }
                                            },
                                            _ => {}
                                        }
                                    }
                                }
                            }
                        }
                    }
                },
                Event::Mouse(mouse) => match mouse.kind {
                    MouseEventKind::Moved => {
                        app.update_context_menu_hover(mouse.row, mouse.column);
                    }
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
                            if app.branch_ui.open {
                                app.branch_ui.move_selection(3);
                            } else if mouse.column >= app.git_diff_x {
                                if mouse.modifiers.contains(KeyModifiers::SHIFT) {
                                    app.git.diff_scroll_x = app.git.diff_scroll_x.saturating_add(4);
                                } else if app.git.selected_entry().is_some_and(|e| e.is_conflict) {
                                    app.conflict_ui.scroll_y =
                                        app.conflict_ui.scroll_y.saturating_add(3);
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
                        Tab::Log => {
                            let files_mode = app.log_ui.detail_mode == LogDetailMode::Files
                                && app.log_ui.subtab != LogSubTab::Commands
                                && app.log_ui.zoom != LogZoom::List;

                            if app.log_ui.inspect.open {
                                app.log_ui.inspect.scroll_y =
                                    app.log_ui.inspect.scroll_y.saturating_add(3);
                            } else if mouse.column >= app.log_diff_x {
                                app.log_ui.focus = LogPaneFocus::Diff;
                                if mouse.modifiers.contains(KeyModifiers::SHIFT) {
                                    app.log_ui.diff_scroll_x =
                                        app.log_ui.diff_scroll_x.saturating_add(4);
                                } else {
                                    app.log_ui.diff_scroll_y =
                                        app.log_ui.diff_scroll_y.saturating_add(3);
                                }
                            } else if files_mode && mouse.column >= app.log_files_x {
                                app.log_ui.focus = LogPaneFocus::Files;
                                app.move_log_file_selection(3);
                            } else {
                                app.log_ui.focus = LogPaneFocus::Commits;
                                app.move_log_selection(3);
                            }
                        }
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
                            if app.branch_ui.open {
                                app.branch_ui.move_selection(-3);
                            } else if mouse.column >= app.git_diff_x {
                                if mouse.modifiers.contains(KeyModifiers::SHIFT) {
                                    app.git.diff_scroll_x = app.git.diff_scroll_x.saturating_sub(4);
                                } else if app.git.selected_entry().is_some_and(|e| e.is_conflict) {
                                    app.conflict_ui.scroll_y =
                                        app.conflict_ui.scroll_y.saturating_sub(3);
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
                        Tab::Log => {
                            let files_mode = app.log_ui.detail_mode == LogDetailMode::Files
                                && app.log_ui.subtab != LogSubTab::Commands
                                && app.log_ui.zoom != LogZoom::List;

                            if app.log_ui.inspect.open {
                                app.log_ui.inspect.scroll_y =
                                    app.log_ui.inspect.scroll_y.saturating_sub(3);
                            } else if mouse.column >= app.log_diff_x {
                                app.log_ui.focus = LogPaneFocus::Diff;
                                if mouse.modifiers.contains(KeyModifiers::SHIFT) {
                                    app.log_ui.diff_scroll_x =
                                        app.log_ui.diff_scroll_x.saturating_sub(4);
                                } else {
                                    app.log_ui.diff_scroll_y =
                                        app.log_ui.diff_scroll_y.saturating_sub(3);
                                }
                            } else if files_mode && mouse.column >= app.log_files_x {
                                app.log_ui.focus = LogPaneFocus::Files;
                                app.move_log_file_selection(-3);
                            } else {
                                app.log_ui.focus = LogPaneFocus::Commits;
                                app.move_log_selection(-3);
                            }
                        }
                    },
                    MouseEventKind::Down(MouseButton::Left) => {
                        app.handle_click(mouse.row, mouse.column, mouse.modifiers);
                    }
                    MouseEventKind::Down(MouseButton::Right) => {
                        app.context_menu = None;
                        app.pending_menu_action = None;
                        app.handle_context_click(mouse.row, mouse.column, mouse.modifiers);
                        app.open_context_menu(mouse.row, mouse.column);
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
    app.save_persisted_ui_settings();

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    Ok(())
}
