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
        Block, Borders, Clear, List, ListItem, ListState, Paragraph, Scrollbar,
        ScrollbarOrientation, ScrollbarState, Wrap,
    },
};
use ratatui_image::{StatefulImage, picker::Picker, protocol::StatefulProtocol};
use serde::{Deserialize, Serialize};
use std::{
    cmp::Ordering,
    collections::{BTreeSet, VecDeque},
    env,
    fs::{self},
    io::{self, Read as _, Write},
    path::PathBuf,
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};
use portable_pty::{CommandBuilder, NativePtySystem, PtySize, PtySystem};

const VERSION: &str = env!("CARGO_PKG_VERSION");

mod branch;
mod commit;
mod conflict;
mod git;
mod git_ops;
mod highlight;
mod openrouter;

use branch::{BranchListItem, BranchUi};
use commit::{CommitFocus, CommitState};
use conflict::{ConflictFile, ConflictResolution};
use git::{
    GitDiffCellKind, GitDiffMode, GitDiffRow, GitSection, GitState, build_side_by_side_rows,
    display_width, pad_to_width,
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
        Terminal,
    }

    impl Theme {
        pub fn label(self) -> &'static str {
            match self {
                Theme::Mocha => "Mocha",
                Theme::TokyoNightStorm => "Tokyo Night",
                Theme::GruvboxDarkHard => "Gruvbox",
                Theme::Nord => "Nord",
                Theme::Dracula => "Dracula",
                Theme::Terminal => "Terminal",
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
        pub diff_add_fg: Color,
        pub diff_del_fg: Color,
        pub diff_gutter_fg: Color,
    }

    fn tint(base: Color, overlay: Color, alpha: f32) -> Color {
        let (br, bg, bb) = match base {
            Color::Rgb(r, g, b) => (r, g, b),
            _ => return base,
        };
        let (or, og, ob) = match overlay {
            Color::Rgb(r, g, b) => (r, g, b),
            _ => return base,
        };

        let mix = |b: u8, o: u8| -> u8 {
            let b = b as f32;
            let o = o as f32;
            let v = b + (o - b) * alpha;
            v.round().clamp(0.0, 255.0) as u8
        };

        Color::Rgb(mix(br, or), mix(bg, og), mix(bb, ob))
    }

    pub fn palette(theme: Theme) -> Palette {
        let diff_alpha = 0.20;
        let hunk_alpha = 0.12;

        match theme {
            Theme::Mocha => {
                let bg = Color::Rgb(30, 30, 46);
                let fg = Color::Rgb(248, 248, 255);
                let accent_primary = Color::Rgb(203, 166, 247);
                let accent_secondary = Color::Rgb(250, 179, 135);
                let accent_tertiary = Color::Rgb(137, 180, 250);
                let border_inactive = Color::Rgb(120, 124, 150);
                let selection_bg = Color::Rgb(78, 82, 110);
                let dir_color = Color::Rgb(137, 180, 250);
                let exe_color = Color::Rgb(166, 227, 161);
                let size_color = Color::Rgb(147, 153, 178);
                let btn_bg = Color::Rgb(243, 139, 168);
                let btn_fg = Color::Rgb(24, 24, 37);
                let menu_bg = Color::Rgb(58, 60, 82);
                // Soft teal for additions, warm coral for deletions
                let diff_add_tint = Color::Rgb(148, 226, 213); // Catppuccin teal
                let diff_del_tint = Color::Rgb(243, 139, 168); // Catppuccin red/pink

                Palette {
                    bg,
                    fg,
                    accent_primary,
                    accent_secondary,
                    accent_tertiary,
                    border_inactive,
                    selection_bg,
                    dir_color,
                    exe_color,
                    size_color,
                    btn_bg,
                    btn_fg,
                    menu_bg,
                    diff_add_bg: tint(bg, diff_add_tint, diff_alpha),
                    diff_del_bg: tint(bg, diff_del_tint, diff_alpha),
                    diff_hunk_bg: tint(bg, accent_primary, hunk_alpha),
                    diff_add_fg: Color::Rgb(148, 226, 213), // Teal for + sign
                    diff_del_fg: Color::Rgb(243, 139, 168), // Red/pink for - sign
                    diff_gutter_fg: Color::Rgb(108, 112, 134), // Muted gray for line numbers
                }
            }
            Theme::TokyoNightStorm => {
                let bg = Color::Rgb(36, 40, 59);
                let fg = Color::Rgb(192, 202, 245);
                let accent_primary = Color::Rgb(122, 162, 247);
                let accent_secondary = Color::Rgb(255, 158, 100);
                let accent_tertiary = Color::Rgb(187, 154, 247);
                let border_inactive = Color::Rgb(65, 72, 104);
                let selection_bg = Color::Rgb(46, 60, 100);
                let dir_color = Color::Rgb(122, 162, 247);
                let exe_color = Color::Rgb(158, 206, 106);
                let size_color = Color::Rgb(86, 95, 137);
                let btn_bg = Color::Rgb(247, 118, 142);
                let btn_fg = Color::Rgb(24, 24, 37);
                let menu_bg = Color::Rgb(45, 49, 71);
                let diff_add_tint = Color::Rgb(115, 218, 202); // Tokyo Night cyan/teal
                let diff_del_tint = Color::Rgb(247, 118, 142); // Tokyo Night red

                Palette {
                    bg,
                    fg,
                    accent_primary,
                    accent_secondary,
                    accent_tertiary,
                    border_inactive,
                    selection_bg,
                    dir_color,
                    exe_color,
                    size_color,
                    btn_bg,
                    btn_fg,
                    menu_bg,
                    diff_add_bg: tint(bg, diff_add_tint, diff_alpha),
                    diff_del_bg: tint(bg, diff_del_tint, diff_alpha),
                    diff_hunk_bg: tint(bg, accent_primary, hunk_alpha),
                    diff_add_fg: Color::Rgb(115, 218, 202), // Cyan/teal for + sign
                    diff_del_fg: Color::Rgb(247, 118, 142), // Red for - sign
                    diff_gutter_fg: Color::Rgb(86, 95, 137), // Muted gray
                }
            }
            Theme::GruvboxDarkHard => {
                let bg = Color::Rgb(29, 32, 33);
                let fg = Color::Rgb(235, 219, 178);
                let accent_primary = Color::Rgb(250, 189, 47);
                let accent_secondary = Color::Rgb(214, 93, 14);
                let accent_tertiary = Color::Rgb(131, 165, 152);
                let border_inactive = Color::Rgb(80, 73, 69);
                let selection_bg = Color::Rgb(60, 56, 54);
                let dir_color = Color::Rgb(131, 165, 152);
                let exe_color = Color::Rgb(184, 187, 38);
                let size_color = Color::Rgb(146, 131, 116);
                let btn_bg = Color::Rgb(251, 73, 52);
                let btn_fg = Color::Rgb(29, 32, 33);
                let menu_bg = Color::Rgb(50, 48, 47);
                let diff_add_tint = Color::Rgb(142, 192, 124); // Gruvbox aqua/green
                let diff_del_tint = Color::Rgb(251, 73, 52); // Gruvbox red

                Palette {
                    bg,
                    fg,
                    accent_primary,
                    accent_secondary,
                    accent_tertiary,
                    border_inactive,
                    selection_bg,
                    dir_color,
                    exe_color,
                    size_color,
                    btn_bg,
                    btn_fg,
                    menu_bg,
                    diff_add_bg: tint(bg, diff_add_tint, diff_alpha),
                    diff_del_bg: tint(bg, diff_del_tint, diff_alpha),
                    diff_hunk_bg: tint(bg, accent_primary, hunk_alpha),
                    diff_add_fg: Color::Rgb(142, 192, 124), // Aqua for + sign
                    diff_del_fg: Color::Rgb(251, 73, 52), // Red for - sign
                    diff_gutter_fg: Color::Rgb(146, 131, 116), // Muted gray
                }
            }
            Theme::Nord => {
                let bg = Color::Rgb(46, 52, 64);
                let fg = Color::Rgb(216, 222, 233);
                let accent_primary = Color::Rgb(136, 192, 208);
                let accent_secondary = Color::Rgb(235, 203, 139);
                let accent_tertiary = Color::Rgb(180, 142, 173);
                let border_inactive = Color::Rgb(76, 86, 106);
                let selection_bg = Color::Rgb(67, 76, 94);
                let dir_color = Color::Rgb(129, 161, 193);
                let exe_color = Color::Rgb(163, 190, 140);
                let size_color = Color::Rgb(76, 86, 106);
                let btn_bg = Color::Rgb(191, 97, 106);
                let btn_fg = Color::Rgb(46, 52, 64);
                let menu_bg = Color::Rgb(59, 66, 82);
                let diff_add_tint = Color::Rgb(136, 192, 208); // Nord frost (cyan)
                let diff_del_tint = Color::Rgb(191, 97, 106); // Nord aurora red

                Palette {
                    bg,
                    fg,
                    accent_primary,
                    accent_secondary,
                    accent_tertiary,
                    border_inactive,
                    selection_bg,
                    dir_color,
                    exe_color,
                    size_color,
                    btn_bg,
                    btn_fg,
                    menu_bg,
                    diff_add_bg: tint(bg, diff_add_tint, diff_alpha),
                    diff_del_bg: tint(bg, diff_del_tint, diff_alpha),
                    diff_hunk_bg: tint(bg, accent_primary, hunk_alpha),
                    diff_add_fg: Color::Rgb(136, 192, 208), // Frost cyan for + sign
                    diff_del_fg: Color::Rgb(191, 97, 106), // Aurora red for - sign
                    diff_gutter_fg: Color::Rgb(76, 86, 106), // Muted gray
                }
            }
            Theme::Dracula => {
                let bg = Color::Rgb(40, 42, 54);
                let fg = Color::Rgb(248, 248, 242);
                let accent_primary = Color::Rgb(189, 147, 249);
                let accent_secondary = Color::Rgb(139, 233, 253);
                let accent_tertiary = Color::Rgb(255, 121, 198);
                let border_inactive = Color::Rgb(98, 114, 164);
                let selection_bg = Color::Rgb(68, 71, 90);
                let dir_color = Color::Rgb(139, 233, 253);
                let exe_color = Color::Rgb(80, 250, 123);
                let size_color = Color::Rgb(98, 114, 164);
                let btn_bg = Color::Rgb(255, 85, 85);
                let btn_fg = Color::Rgb(40, 42, 54);
                let menu_bg = Color::Rgb(68, 71, 90);
                let diff_add_tint = Color::Rgb(139, 233, 253); // Dracula cyan
                let diff_del_tint = Color::Rgb(255, 121, 198); // Dracula pink

                Palette {
                    bg,
                    fg,
                    accent_primary,
                    accent_secondary,
                    accent_tertiary,
                    border_inactive,
                    selection_bg,
                    dir_color,
                    exe_color,
                    size_color,
                    btn_bg,
                    btn_fg,
                    menu_bg,
                    diff_add_bg: tint(bg, diff_add_tint, diff_alpha),
                    diff_del_bg: tint(bg, diff_del_tint, diff_alpha),
                    diff_hunk_bg: tint(bg, accent_primary, hunk_alpha),
                    diff_add_fg: Color::Rgb(139, 233, 253), // Cyan for + sign
                    diff_del_fg: Color::Rgb(255, 121, 198), // Pink for - sign
                    diff_gutter_fg: Color::Rgb(98, 114, 164), // Muted gray
                }
            }
            Theme::Terminal => {
                // Clean dark theme inspired by OpenCode - pure grays, high contrast
                let bg = Color::Rgb(22, 22, 22);
                let fg = Color::Rgb(212, 212, 212);
                let accent_primary = Color::Rgb(97, 175, 239); // Bright blue
                let accent_secondary = Color::Rgb(229, 192, 123); // Orange/gold
                let accent_tertiary = Color::Rgb(198, 120, 221); // Purple
                let border_inactive = Color::Rgb(68, 68, 68);
                let selection_bg = Color::Rgb(55, 55, 55);
                let dir_color = Color::Rgb(97, 175, 239); // Blue for dirs
                let exe_color = Color::Rgb(152, 195, 121); // Green for executables
                let size_color = Color::Rgb(92, 99, 112); // Muted gray
                let btn_bg = Color::Rgb(224, 108, 117); // Red
                let btn_fg = Color::Rgb(22, 22, 22);
                let menu_bg = Color::Rgb(38, 38, 38);
                let diff_add_tint = Color::Rgb(86, 182, 194); // Cyan/teal
                let diff_del_tint = Color::Rgb(224, 108, 117); // Warm red/coral

                Palette {
                    bg,
                    fg,
                    accent_primary,
                    accent_secondary,
                    accent_tertiary,
                    border_inactive,
                    selection_bg,
                    dir_color,
                    exe_color,
                    size_color,
                    btn_bg,
                    btn_fg,
                    menu_bg,
                    diff_add_bg: tint(bg, diff_add_tint, diff_alpha),
                    diff_del_bg: tint(bg, diff_del_tint, diff_alpha),
                    diff_hunk_bg: tint(bg, accent_primary, hunk_alpha),
                    diff_add_fg: Color::Rgb(86, 182, 194), // Cyan for + sign
                    diff_del_fg: Color::Rgb(224, 108, 117), // Red for - sign
                    diff_gutter_fg: Color::Rgb(92, 99, 112), // Muted gray
                }
            }
        }
    }
}

const THEME_ORDER: [theme::Theme; 6] = [
    theme::Theme::Terminal,
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
    Terminal,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GitFooterAction {
    Stage,
    Unstage,
    Discard,
    Commit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BranchPickerMode {
    Checkout,
    LogView,
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
    SelectGitTreeItem(usize),
    ToggleGitTreeExpand,
    RevertHunk(usize),
    RevertBlock(usize),
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
    OpenLogBranchPicker,
    CloseBranchPicker,
    SelectBranch(usize),
    SelectLogBranch(usize),
    ConfirmLogBranchPicker,

    OpenAuthorPicker,
    CloseAuthorPicker,
    SelectAuthor(usize),
    BranchCheckout,
    ConfirmBranchCheckout,
    CancelBranchCheckout,

    OpenStashPicker,
    CloseStashPicker,
    SelectStash(usize),
    StashApply,
    StashPop,
    StashDrop,
    ConfirmStashAction,
    CancelStashAction,

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
    Delete,

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
struct DeleteConfirm {
    path: PathBuf,
    is_dir: bool,
}

struct TerminalState {
    parser: vt100::Parser,
    pty_writer: Option<Box<dyn Write + Send>>,
    pty_reader_rx: Option<mpsc::Receiver<Vec<u8>>>,
    active: bool,
}

impl TerminalState {
    fn new() -> Self {
        Self {
            parser: vt100::Parser::new(24, 80, 0),
            pty_writer: None,
            pty_reader_rx: None,
            active: false,
        }
    }

    fn spawn_shell(&mut self, cols: u16, rows: u16, cwd: &PathBuf) {
        if self.active {
            return;
        }

        let pty_system = NativePtySystem::default();
        let pair = match pty_system.openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        }) {
            Ok(p) => p,
            Err(_) => return,
        };

        let shell = env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string());
        let mut cmd = CommandBuilder::new(&shell);
        cmd.cwd(cwd);

        let _child = match pair.slave.spawn_command(cmd) {
            Ok(c) => c,
            Err(_) => return,
        };

        self.parser = vt100::Parser::new(rows, cols, 1000);
        self.pty_writer = Some(pair.master.take_writer().unwrap());

        // Read PTY output in background thread
        let mut reader = pair.master.try_clone_reader().unwrap();
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let mut buf = [0u8; 4096];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        if tx.send(buf[..n].to_vec()).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        });
        self.pty_reader_rx = Some(rx);
        self.active = true;
    }

    fn poll_output(&mut self) {
        if let Some(rx) = &self.pty_reader_rx {
            while let Ok(data) = rx.try_recv() {
                self.parser.process(&data);
            }
        }
    }

    fn write_input(&mut self, data: &[u8]) {
        if let Some(writer) = &mut self.pty_writer {
            let _ = writer.write_all(data);
            let _ = writer.flush();
        }
    }

    fn resize(&mut self, cols: u16, rows: u16) {
        self.parser.set_size(rows, cols);
    }
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
    git_left_width: Option<u16>,
    #[serde(default)]
    theme: Option<theme::Theme>,

    #[serde(default)]
    wrap_diff: Option<bool>,
    #[serde(default)]
    syntax_highlight: Option<bool>,

    #[serde(default)]
    git_side_by_side: Option<bool>,
    #[serde(default)]
    git_zoom_diff: Option<bool>,
    #[serde(default)]
    log_side_by_side: Option<bool>,

    #[serde(default)]
    log_zoom: Option<LogZoom>,
    #[serde(default)]
    log_detail_mode: Option<LogDetailMode>,
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
    Stash,
    Commands,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
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
    status: Option<String>,

    history_ref: Option<String>,

    subtab: LogSubTab,
    filter_query: String,
    filter_edit: bool,
    focus: LogPaneFocus,

    history: Vec<git_ops::CommitEntry>,
    reflog: Vec<git_ops::ReflogEntry>,
    stash: Vec<git_ops::StashEntry>,
    history_filtered: Vec<usize>,
    reflog_filtered: Vec<usize>,
    stash_filtered: Vec<usize>,

    detail_mode: LogDetailMode,
    diff_mode: GitDiffMode,
    zoom: LogZoom,

    diff_lines: Vec<String>,
    diff_scroll_y: u16,
    diff_scroll_x: u16,
    diff_generation: u64,
    diff_request_id: u64,

    files: Vec<git_ops::CommitFileChange>,
    files_hash: Option<String>,

    history_limit: usize,
    reflog_limit: usize,
    stash_limit: usize,

    history_state: ListState,
    reflog_state: ListState,
    stash_state: ListState,
    command_state: ListState,

    left_width: u16,
    inspect: InspectUi,

    files_state: ListState,
}

impl LogUi {
    fn new() -> Self {
        Self {
            status: None,

            history_ref: None,

            subtab: LogSubTab::History,
            filter_query: String::new(),
            filter_edit: false,
            focus: LogPaneFocus::Commits,

            history: Vec::new(),
            reflog: Vec::new(),
            stash: Vec::new(),
            history_filtered: Vec::new(),
            reflog_filtered: Vec::new(),
            stash_filtered: Vec::new(),

            detail_mode: LogDetailMode::Diff,
            diff_mode: GitDiffMode::Unified,
            zoom: LogZoom::None,

            diff_lines: Vec::new(),
            diff_scroll_y: 0,
            diff_scroll_x: 0,
            diff_generation: 0,
            diff_request_id: 0,

            files: Vec::new(),
            files_hash: None,

            history_limit: 200,
            reflog_limit: 200,
            stash_limit: 200,

            history_state: ListState::default(),
            reflog_state: ListState::default(),
            stash_state: ListState::default(),
            command_state: ListState::default(),

            left_width: 44,
            inspect: InspectUi::new(),

            files_state: ListState::default(),
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
            LogSubTab::Stash => {}
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
            LogDetailMode::Files if self.subtab == LogSubTab::History => {
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
            LogSubTab::Stash => &self.stash_state,
            LogSubTab::Commands => &self.command_state,
        }
    }

    fn active_state_mut(&mut self) -> &mut ListState {
        match self.subtab {
            LogSubTab::History => &mut self.history_state,
            LogSubTab::Reflog => &mut self.reflog_state,
            LogSubTab::Stash => &mut self.stash_state,
            LogSubTab::Commands => &mut self.command_state,
        }
    }

    fn update_filtered(&mut self) {
        let prev_hist = self
            .history_state
            .selected()
            .and_then(|sel| self.history_filtered.get(sel).copied());
        let prev_reflog = self
            .reflog_state
            .selected()
            .and_then(|sel| self.reflog_filtered.get(sel).copied());
        let prev_stash = self
            .stash_state
            .selected()
            .and_then(|sel| self.stash_filtered.get(sel).copied());

        let parsed = parse_log_filter_query(self.filter_query.as_str());
        let author_tokens: Vec<String> = parsed.author.iter().map(|s| s.to_lowercase()).collect();
        let ref_tokens: Vec<String> = parsed.refs.iter().map(|s| s.to_lowercase()).collect();
        let tokens: Vec<String> = parsed.tokens.iter().map(|s| s.to_lowercase()).collect();
        let is_empty = author_tokens.is_empty() && ref_tokens.is_empty() && tokens.is_empty();

        let mut history_matches: Vec<(i32, usize)> = Vec::new();
        let mut reflog_matches: Vec<(i32, usize)> = Vec::new();
        let mut stash_matches: Vec<(i32, usize)> = Vec::new();

        for (i, e) in self.history.iter().enumerate() {
            if is_empty {
                history_matches.push((0, i));
                continue;
            }

            let author = e.author.to_lowercase();
            let refs = e.decoration.to_lowercase();
            let hay = format!("{} {} {}", e.short, e.subject, e.decoration).to_lowercase();

            let mut score = 0i32;
            let mut ok = true;

            for t in &author_tokens {
                if let Some(s) = token_score(author.as_str(), t.as_str()) {
                    score += s;
                } else {
                    ok = false;
                    break;
                }
            }
            if ok {
                for t in &ref_tokens {
                    if let Some(s) = token_score(refs.as_str(), t.as_str()) {
                        score += s;
                    } else {
                        ok = false;
                        break;
                    }
                }
            }
            if ok {
                for t in &tokens {
                    if let Some(s) = token_score(hay.as_str(), t.as_str()) {
                        score += s;
                    } else {
                        ok = false;
                        break;
                    }
                }
            }

            if ok {
                history_matches.push((score, i));
            }
        }

        for (i, e) in self.reflog.iter().enumerate() {
            if is_empty {
                reflog_matches.push((0, i));
                continue;
            }

            let refs = e.decoration.to_lowercase();
            let hay = format!("{} {} {}", e.selector, e.subject, e.decoration).to_lowercase();

            let mut score = 0i32;
            let mut ok = true;

            for t in &ref_tokens {
                if let Some(s) = token_score(refs.as_str(), t.as_str()) {
                    score += s;
                } else {
                    ok = false;
                    break;
                }
            }
            if ok {
                for t in &tokens {
                    if let Some(s) = token_score(hay.as_str(), t.as_str()) {
                        score += s;
                    } else {
                        ok = false;
                        break;
                    }
                }
            }

            if ok {
                reflog_matches.push((score, i));
            }
        }

        for (i, e) in self.stash.iter().enumerate() {
            if is_empty {
                stash_matches.push((0, i));
                continue;
            }

            if !author_tokens.is_empty() {
                continue;
            }

            let hay = format!("{} {}", e.selector, e.subject).to_lowercase();
            let mut score = 0i32;
            let mut ok = true;

            for t in &tokens {
                if let Some(s) = token_score(hay.as_str(), t.as_str()) {
                    score += s;
                } else {
                    ok = false;
                    break;
                }
            }

            if ok {
                stash_matches.push((score, i));
            }
        }

        history_matches.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
        reflog_matches.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
        stash_matches.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));

        self.history_filtered.clear();
        self.history_filtered
            .extend(history_matches.into_iter().map(|(_, i)| i));

        self.reflog_filtered.clear();
        self.reflog_filtered
            .extend(reflog_matches.into_iter().map(|(_, i)| i));

        self.stash_filtered.clear();
        self.stash_filtered
            .extend(stash_matches.into_iter().map(|(_, i)| i));

        if self.history_filtered.is_empty() {
            self.history_state.select(None);
        } else if let Some(prev) =
            prev_hist.and_then(|idx| self.history_filtered.iter().position(|i| *i == idx))
        {
            self.history_state.select(Some(prev));
        } else {
            self.history_state.select(Some(0));
        }

        if self.reflog_filtered.is_empty() {
            self.reflog_state.select(None);
        } else if let Some(prev) =
            prev_reflog.and_then(|idx| self.reflog_filtered.iter().position(|i| *i == idx))
        {
            self.reflog_state.select(Some(prev));
        } else {
            self.reflog_state.select(Some(0));
        }

        if self.stash_filtered.is_empty() {
            self.stash_state.select(None);
        } else if let Some(prev) =
            prev_stash.and_then(|idx| self.stash_filtered.iter().position(|i| *i == idx))
        {
            self.stash_state.select(Some(prev));
        } else {
            self.stash_state.select(Some(0));
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
    OpenAuthorPicker,
    OpenStashPicker,
    ClearGitLog,
    QuickStash,
    CheckUpdate,
    Quit,
}

const COMMAND_PALETTE_ITEMS: &[(CommandId, &str)] = &[
    (CommandId::ToggleHidden, "Toggle hidden files"),
    (CommandId::ToggleWrapDiff, "Toggle diff wrap"),
    (CommandId::ToggleSyntaxHighlight, "Toggle syntax highlight"),
    (CommandId::SelectTheme, "Select theme…"),
    (CommandId::RefreshGit, "Git: refresh status"),
    (CommandId::OpenBranchPicker, "Checkout branch…"),
    (CommandId::OpenAuthorPicker, "Filter by author…"),
    (CommandId::OpenStashPicker, "Stash…"),
    (CommandId::GitFetch, "Git: fetch --prune"),
    (CommandId::GitPullRebase, "Git: pull --rebase"),
    (CommandId::GitPush, "Git: push"),
    (CommandId::ClearGitLog, "Clear git command log"),
    (CommandId::QuickStash, "Git: stash changes"),
    (CommandId::CheckUpdate, "Check for updates"),
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StashConfirmAction {
    Pop,
    Drop,
}

struct StashUi {
    open: bool,
    query: String,
    stashes: Vec<git_ops::StashEntry>,
    filtered: Vec<usize>,
    list_state: ListState,
    confirm: Option<(StashConfirmAction, String)>,
    status: Option<String>,
}

impl StashUi {
    fn new() -> Self {
        Self {
            open: false,
            query: String::new(),
            stashes: Vec::new(),
            filtered: Vec::new(),
            list_state: ListState::default(),
            confirm: None,
            status: None,
        }
    }

    fn selected_stash(&self) -> Option<&git_ops::StashEntry> {
        let sel = self.list_state.selected()?;
        let idx = *self.filtered.get(sel)?;
        self.stashes.get(idx)
    }

    fn update_filtered(&mut self) {
        let prev = self
            .list_state
            .selected()
            .and_then(|sel| self.filtered.get(sel).copied());

        let query = self.query.trim().to_lowercase();
        let tokens: Vec<String> = query
            .split_whitespace()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        let mut matches: Vec<(i32, usize)> = Vec::new();
        for (i, s) in self.stashes.iter().enumerate() {
            if tokens.is_empty() {
                matches.push((0, i));
                continue;
            }

            let hay = format!("{} {}", s.selector, s.subject).to_lowercase();
            let mut score = 0i32;
            let mut ok = true;

            for t in &tokens {
                if let Some(s) = token_score(hay.as_str(), t.as_str()) {
                    score += s;
                } else {
                    ok = false;
                    break;
                }
            }

            if ok {
                matches.push((score, i));
            }
        }

        matches.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
        self.filtered.clear();
        self.filtered.extend(matches.into_iter().map(|(_, i)| i));

        if self.filtered.is_empty() {
            self.list_state.select(None);
            return;
        }

        if let Some(prev) = prev.and_then(|idx| self.filtered.iter().position(|i| *i == idx)) {
            self.list_state.select(Some(prev));
        } else {
            self.list_state.select(Some(0));
        }
    }

    fn move_selection(&mut self, delta: i32) {
        let len = self.filtered.len();
        if len == 0 {
            self.list_state.select(None);
            return;
        }

        let cur = self.list_state.selected().unwrap_or(0) as i32;
        let next = (cur + delta).clamp(0, len.saturating_sub(1) as i32);
        self.list_state.select(Some(next as usize));
    }
}

struct AuthorUi {
    open: bool,
    query: String,
    authors: Vec<String>,
    filtered: Vec<usize>,
    list_state: ListState,
    status: Option<String>,
}

impl AuthorUi {
    fn new() -> Self {
        Self {
            open: false,
            query: String::new(),
            authors: Vec::new(),
            filtered: Vec::new(),
            list_state: ListState::default(),
            status: None,
        }
    }

    fn set_authors(&mut self, authors: Vec<String>) {
        self.query.clear();
        self.authors = authors;
        self.update_filtered();
    }

    fn selected_author(&self) -> Option<&str> {
        let sel = self.list_state.selected()?;
        let idx = *self.filtered.get(sel)?;
        self.authors.get(idx).map(|s| s.as_str())
    }

    fn update_filtered(&mut self) {
        let prev = self
            .list_state
            .selected()
            .and_then(|sel| self.filtered.get(sel).copied());

        let query = self.query.trim().to_lowercase();
        let tokens: Vec<String> = query
            .split_whitespace()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        let mut matches: Vec<(i32, usize)> = Vec::new();
        for (i, s) in self.authors.iter().enumerate() {
            if tokens.is_empty() {
                matches.push((0, i));
                continue;
            }

            let hay = s.to_lowercase();
            let mut score = 0i32;
            let mut ok = true;

            for t in &tokens {
                if let Some(s) = token_score(hay.as_str(), t.as_str()) {
                    score += s;
                } else {
                    ok = false;
                    break;
                }
            }

            if ok {
                matches.push((score, i));
            }
        }

        matches.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
        self.filtered.clear();
        self.filtered.extend(matches.into_iter().map(|(_, i)| i));

        if self.filtered.is_empty() {
            self.list_state.select(None);
            return;
        }

        if let Some(prev) = prev.and_then(|idx| self.filtered.iter().position(|i| *i == idx)) {
            self.list_state.select(Some(prev));
        } else {
            self.list_state.select(Some(0));
        }
    }

    fn move_selection(&mut self, delta: i32) {
        let len = self.filtered.len();
        if len == 0 {
            self.list_state.select(None);
            return;
        }

        let cur = self.list_state.selected().unwrap_or(0) as i32;
        let next = (cur + delta).clamp(0, len.saturating_sub(1) as i32);
        self.list_state.select(Some(next as usize));
    }
}

struct LogDiffJobOutput {
    diff_lines: Vec<String>,
    files_hash: Option<String>,
    files: Option<Vec<git_ops::CommitFileChange>>,
    files_selected: Option<usize>,
}

struct GitRefreshJobOutput {
    repo_root: Option<PathBuf>,
    branch: String,
    ahead: u32,
    behind: u32,
    entries: Vec<git::GitFileEntry>,
}

enum JobResult {
    Git {
        cmd: String,
        result: Result<(), String>,
        refresh: bool,
        close_commit: bool,
    },
    GitRefresh {
        request_id: u64,
        current_path: PathBuf,
        result: Result<GitRefreshJobOutput, String>,
    },
    GitDiff {
        request_id: u64,
        result: Result<Vec<String>, String>,
    },
    Ai {
        result: Result<String, String>,
    },
    LogReload {
        history_limit: usize,
        reflog_limit: usize,
        stash_limit: usize,
        history: Result<Vec<git_ops::CommitEntry>, String>,
        reflog: Result<Vec<git_ops::ReflogEntry>, String>,
        stash: Result<Vec<git_ops::StashEntry>, String>,
    },
    LogDiff {
        request_id: u64,
        result: Result<LogDiffJobOutput, String>,
    },
    LogHistory {
        limit: usize,
        result: Result<Vec<git_ops::CommitEntry>, String>,
    },
    LogReflog {
        limit: usize,
        result: Result<Vec<git_ops::ReflogEntry>, String>,
    },
    LogStash {
        limit: usize,
        result: Result<Vec<git_ops::StashEntry>, String>,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DiffRenderCacheKey {
    theme: theme::Theme,
    generation: u64,
    mode: GitDiffMode,
    width: u16,
    wrap: bool,
    syntax_highlight: bool,
    scroll_x: u16,
}

struct DiffRenderCache {
    key: Option<DiffRenderCacheKey>,
    lines: Vec<Line<'static>>,
}

impl DiffRenderCache {
    fn new() -> Self {
        Self {
            key: None,
            lines: Vec::new(),
        }
    }

    fn invalidate(&mut self) {
        self.key = None;
        self.lines.clear();
    }
}

struct App {
    current_path: PathBuf,      // Explorer's current directory (changes with navigation)
    startup_path: PathBuf,      // Initial directory (fixed, used for Git)
    files: Vec<FileEntry>,
    list_state: ListState,
    preview_scroll: u16,
    should_quit: bool,
    show_hidden: bool,

    current_tab: Tab,

    git: GitState,
    git_operation: Option<GitOperation>,
    branch_ui: BranchUi,
    branch_picker_mode: BranchPickerMode,
    author_ui: AuthorUi,
    stash_ui: StashUi,
    stash_confirm: Option<(StashConfirmAction, String)>,
    conflict_ui: ConflictUi,
    commit: CommitState,
    pending_job: Option<PendingJob>,
    git_refresh_job: Option<PendingJob>,
    git_refresh_request_id: u64,
    git_diff_job: Option<PendingJob>,
    log_diff_job: Option<PendingJob>,
    discard_confirm: Option<DiscardConfirm>,
    delete_confirm: Option<DeleteConfirm>,
    operation_popup: Option<OperationPopup>,
    theme_picker: ThemePickerUi,
    command_palette: CommandPaletteUi,
    git_log: VecDeque<GitLogEntry>,
    log_ui: LogUi,
    terminal: TerminalState,

    wrap_diff: bool,
    syntax_highlight: bool,
    git_zoom_diff: bool,
    git_left_width: u16,

    theme: theme::Theme,
    palette: theme::Palette,

    git_diff_cache: DiffRenderCache,
    log_diff_cache: DiffRenderCache,

    explorer_preview_x: u16,
    git_diff_x: u16,
    log_files_x: u16,
    log_diff_x: u16,

    zones: Vec<ClickZone>,
    last_click: Option<(Instant, usize)>,
    bookmarks: Vec<(String, PathBuf)>,

    // Auto-refresh
    last_dir_check: Instant,
    dir_mtime: Option<std::time::SystemTime>,
    auto_refresh: bool,

    // Update confirmation
    update_confirm: Option<String>, // Some(new_version) when update available

    // Quick stash confirmation
    quick_stash_confirm: bool,

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
    needs_full_redraw: bool,
}

impl App {
    fn new(start_path: PathBuf, picker: Picker) -> Self {
        let mut app = Self {
            current_path: start_path.clone(),
            startup_path: start_path,
            files: Vec::new(),
            list_state: ListState::default(),
            preview_scroll: 0,
            should_quit: false,
            show_hidden: false,

            current_tab: Tab::Git,

            git: GitState::new(),
            git_operation: None,
            branch_ui: BranchUi::new(),
            branch_picker_mode: BranchPickerMode::Checkout,
            author_ui: AuthorUi::new(),
            stash_ui: StashUi::new(),
            stash_confirm: None,
            conflict_ui: ConflictUi::new(),
            commit: CommitState::new(),
            pending_job: None,
            git_refresh_job: None,
            git_refresh_request_id: 0,
            git_diff_job: None,
            log_diff_job: None,
            discard_confirm: None,
            delete_confirm: None,
            operation_popup: None,
            theme_picker: ThemePickerUi::new(),
            command_palette: CommandPaletteUi::new(),
            git_log: VecDeque::new(),
            log_ui: LogUi::new(),
            terminal: TerminalState::new(),

            wrap_diff: true,
            syntax_highlight: true,
            git_zoom_diff: false,
            git_left_width: 40,

            theme: theme::Theme::Mocha,
            palette: theme::palette(theme::Theme::Mocha),

            git_diff_cache: DiffRenderCache::new(),
            log_diff_cache: DiffRenderCache::new(),

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
            last_dir_check: Instant::now(),
            dir_mtime: None,
            auto_refresh: true,
            update_confirm: None,
            quick_stash_confirm: false,
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
            needs_full_redraw: false,
        };
        app.load_persisted_bookmarks();
        app.load_persisted_ui_settings();
        app.load_files();
        if !app.files.is_empty() {
            app.list_state.select(Some(0));
            app.update_preview();
        }
        app.git.refresh(&app.startup_path);
        app.update_git_operation();
        // Load diff for initially selected file
        if app.git.selected_tree_entry().is_some() {
            app.request_git_diff_update();
        }
        app
    }

    fn refresh_git_state(&mut self) {
        self.start_git_refresh_job();
    }

    fn start_git_refresh_job(&mut self) {
        if self.git_refresh_job.is_some() {
            self.set_status("Busy");
            return;
        }

        self.git_refresh_request_id = self.git_refresh_request_id.wrapping_add(1);
        let request_id = self.git_refresh_request_id;
        let startup_path = self.startup_path.clone();

        let (tx, rx) = mpsc::channel();
        self.git_refresh_job = Some(PendingJob { rx });

        thread::spawn(move || {
            let result = (|| -> Result<GitRefreshJobOutput, String> {
                let mut git = GitState::new();
                git.refresh(&startup_path);
                Ok(GitRefreshJobOutput {
                    repo_root: git.repo_root,
                    branch: git.branch,
                    ahead: git.ahead,
                    behind: git.behind,
                    entries: git.entries,
                })
            })();

            let _ = tx.send(JobResult::GitRefresh {
                request_id,
                current_path: startup_path,
                result,
            });
        });
    }

    fn request_git_diff_update(&mut self) {
        self.git.diff_request_id = self.git.diff_request_id.wrapping_add(1);
        let request_id = self.git.diff_request_id;

        self.git.diff_scroll_y = 0;
        self.git.diff_scroll_x = 0;

        let Some(repo_root) = self.git.repo_root.clone() else {
            self.git.diff_lines.clear();
            self.git.diff_generation = self.git.diff_generation.wrapping_add(1);
            self.git_diff_cache.invalidate();
            return;
        };

        let Some(entry) = self.git.selected_tree_entry().cloned() else {
            self.git.diff_lines.clear();
            self.git.diff_generation = self.git.diff_generation.wrapping_add(1);
            self.git_diff_cache.invalidate();
            return;
        };

        self.git.diff_lines = vec!["Loading diff…".to_string()];
        self.git.diff_generation = self.git.diff_generation.wrapping_add(1);
        self.git_diff_cache.invalidate();

        let path = entry.path;
        let is_untracked = entry.is_untracked;
        let staged = entry.x != ' ' && entry.x != '?';

        let (tx, rx) = mpsc::channel();
        self.git_diff_job = Some(PendingJob { rx });
        thread::spawn(move || {
            let result: Result<Vec<String>, String> = if is_untracked {
                // For untracked files, read the content and format as a diff
                let file_path = repo_root.join(&path);
                if file_path.is_dir() {
                    // List directory contents for untracked directories
                    match std::fs::read_dir(&file_path) {
                        Ok(entries) => {
                            let mut diff_lines = vec![
                                format!("Untracked directory: {}/", path),
                                "".to_string(),
                            ];
                            for entry in entries.filter_map(|e| e.ok()) {
                                let name = entry.file_name().to_string_lossy().to_string();
                                let prefix = if entry.path().is_dir() { "  " } else { "  + " };
                                diff_lines.push(format!("{}{}", prefix, name));
                            }
                            Ok(diff_lines)
                        }
                        Err(e) => Ok(vec![format!("Cannot read directory: {}", e)]),
                    }
                } else {
                    match std::fs::read_to_string(&file_path) {
                        Ok(content) => {
                            let lines: Vec<&str> = content.lines().collect();
                            let line_count = lines.len();
                            let mut diff_lines = vec![
                                format!("diff --git a/{} b/{}", path, path),
                                "new file mode 100644".to_string(),
                                "--- /dev/null".to_string(),
                                format!("+++ b/{}", path),
                                format!("@@ -0,0 +1,{} @@", line_count),
                            ];
                            for line in lines {
                                diff_lines.push(format!("+{}", line));
                            }
                            Ok(diff_lines)
                        }
                        Err(e) => Ok(vec![format!("Cannot read file: {}", e)]),
                    }
                }
            } else {
                match git_ops::diff_path(&repo_root, path.as_str(), staged) {
                    Ok(text) => {
                        if text.trim().is_empty() {
                            Ok(vec!["No diff".to_string()])
                        } else {
                            Ok(text.lines().map(|l| l.to_string()).collect())
                        }
                    }
                    Err(e) => Err(format!("git diff failed: {}", e)),
                }
            };

            let _ = tx.send(JobResult::GitDiff { request_id, result });
        });
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
        self.branch_picker_mode = BranchPickerMode::Checkout;
        self.open_branch_picker_internal();
    }

    fn open_log_branch_picker(&mut self) {
        self.branch_picker_mode = BranchPickerMode::LogView;
        self.open_branch_picker_internal();
    }

    fn open_branch_picker_internal(&mut self) {
        self.context_menu = None;
        self.commit.open = false;

        let Some(repo_root) = self.git.repo_root.clone() else {
            self.set_status("Not a git repository");
            return;
        };

        match git_ops::list_branches(&repo_root) {
            Ok(branches) => {
                self.branch_ui.open = true;
                self.author_ui.open = false;
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
        self.branch_ui.items.clear();
        self.branch_ui.branches.clear();

        self.branch_ui.confirm_checkout = None;
        self.branch_ui.status = None;
        self.branch_ui.list_state.select(None);
    }

    fn confirm_log_branch_picker(&mut self) {
        let Some(branch) = self.branch_ui.selected_branch() else {
            self.set_status("No branch selected");
            return;
        };

        if !branch.is_remote && branch.is_current {
            self.log_ui.history_ref = None;
        } else {
            self.log_ui.history_ref = Some(branch.name);
        }

        self.refresh_log_data();
        self.close_branch_picker();
    }

    fn open_stash_picker(&mut self) {
        self.context_menu = None;
        self.commit.open = false;
        self.branch_ui.open = false;

        let Some(repo_root) = self.git.repo_root.clone() else {
            self.set_status("Not a git repository");
            return;
        };

        match git_ops::list_stashes(&repo_root, 200) {
            Ok(stashes) => {
                self.stash_confirm = None;
                self.stash_ui.open = true;
                self.stash_ui.query.clear();
                self.stash_ui.status = None;
                self.stash_ui.confirm = None;
                self.stash_ui.stashes = stashes;
                self.stash_ui.update_filtered();
            }
            Err(e) => {
                self.set_status(e);
            }
        }
    }

    fn close_stash_picker(&mut self) {
        self.stash_confirm = None;
        self.stash_ui.open = false;
        self.stash_ui.query.clear();
        self.stash_ui.stashes.clear();
        self.stash_ui.filtered.clear();
        self.stash_ui.list_state.select(None);
        self.stash_ui.confirm = None;
        self.stash_ui.status = None;
    }

    fn open_author_picker(&mut self) {
        self.context_menu = None;
        self.commit.open = false;
        self.branch_ui.open = false;
        self.stash_ui.open = false;

        if self.git.repo_root.is_none() {
            self.set_status("Not a git repository");
            return;
        }

        let mut unique = BTreeSet::new();
        for e in &self.log_ui.history {
            let a = e.author.trim();
            if !a.is_empty() {
                unique.insert(a.to_string());
            }
        }

        let authors: Vec<String> = unique.into_iter().collect();
        if authors.is_empty() {
            self.set_status("No authors loaded");
            return;
        }

        self.author_ui.open = true;
        self.author_ui.set_authors(authors);
    }

    fn close_author_picker(&mut self) {
        self.author_ui.open = false;
        self.author_ui.query.clear();
        self.author_ui.authors.clear();
        self.author_ui.filtered.clear();
        self.author_ui.list_state.select(None);
        self.author_ui.status = None;
    }

    fn confirm_author_picker(&mut self) {
        let Some(author) = self.author_ui.selected_author().map(str::to_string) else {
            self.set_status("No author selected");
            return;
        };

        self.set_filter_author(author.as_str());
        self.log_ui.update_filtered();
        self.refresh_log_diff();
        self.close_author_picker();
    }

    fn set_filter_author(&mut self, author: &str) {
        let author_token = if author.chars().any(|c| c.is_whitespace()) {
            format!("@\"{}\"", author)
        } else {
            format!("@{}", author)
        };

        let tokens = split_query_tokens(self.log_ui.filter_query.as_str());
        let mut out: Vec<String> = Vec::new();
        for t in tokens {
            let tt = t.trim();
            if tt.starts_with('@') {
                continue;
            }
            if tt.starts_with("author:") || tt.starts_with("a:") {
                continue;
            }
            out.push(tt.to_string());
        }
        out.push(author_token);
        self.log_ui.filter_query = out.join(" ");
    }

    fn set_stash_status<S: Into<String>>(&mut self, msg: S) {
        let msg = msg.into();
        if self.stash_ui.open {
            self.stash_ui.status = Some(msg);
        } else {
            self.set_status(msg);
        }
    }

    fn stash_apply_selector(&mut self, selector: String) -> bool {
        if self.pending_job.is_some() {
            self.set_stash_status("Busy");
            return false;
        }

        let Some(repo_root) = self.git.repo_root.clone() else {
            self.set_stash_status("Not a git repository");
            return false;
        };

        let cmd = format!("git stash apply {}", selector);
        self.start_git_job(cmd, true, false, move || {
            git_ops::stash_apply(&repo_root, &selector)
        });
        true
    }

    fn stash_apply_log_selected(&mut self) {
        let Some(entry) = self.selected_stash_entry() else {
            self.set_status("No stash selected");
            return;
        };

        let _ = self.stash_apply_selector(entry.selector.clone());
    }

    fn open_stash_confirm(&mut self, action: StashConfirmAction, selector: String) {
        if self.pending_job.is_some() {
            self.set_stash_status("Busy");
            return;
        }

        if self.git.repo_root.is_none() {
            self.set_stash_status("Not a git repository");
            return;
        }

        self.stash_confirm = Some((action, selector));
    }

    fn open_stash_confirm_log_selected(&mut self, action: StashConfirmAction) {
        let Some(entry) = self.selected_stash_entry() else {
            self.set_status("No stash selected");
            return;
        };

        self.open_stash_confirm(action, entry.selector.clone());
    }

    fn stash_apply_selected(&mut self) {
        self.stash_ui.status = None;

        let Some(sel) = self.stash_ui.selected_stash() else {
            self.set_stash_status("No stash selected");
            return;
        };

        if self.stash_apply_selector(sel.selector.clone()) {
            if self.stash_ui.open {
                self.close_stash_picker();
            }
        }
    }

    fn confirm_stash_action(&mut self) {
        self.stash_ui.status = None;
        if self.pending_job.is_some() {
            self.set_stash_status("Busy");
            return;
        }

        let Some(repo_root) = self.git.repo_root.clone() else {
            self.set_stash_status("Not a git repository");
            return;
        };

        let Some((action, selector)) = self.stash_confirm.take() else {
            return;
        };

        match action {
            StashConfirmAction::Pop => {
                let rr = repo_root.clone();
                let sel = selector.clone();
                let cmd = format!("git stash pop {}", sel);
                self.start_git_job(cmd, true, false, move || git_ops::stash_pop(&rr, &sel));
            }
            StashConfirmAction::Drop => {
                let rr = repo_root.clone();
                let sel = selector.clone();
                let cmd = format!("git stash drop {}", sel);
                self.start_git_job(cmd, true, false, move || git_ops::stash_drop(&rr, &sel));
            }
        }

        self.close_stash_picker();
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
        let Some(entry) = self.git.selected_tree_entry() else {
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
        self.log_diff_cache.invalidate();

        let Some(repo_root) = self.git.repo_root.clone() else {
            self.log_ui.history.clear();
            self.log_ui.reflog.clear();
            self.log_ui.stash.clear();
            self.log_ui.history_filtered.clear();
            self.log_ui.reflog_filtered.clear();
            self.log_ui.stash_filtered.clear();
            self.log_ui.history_state.select(None);
            self.log_ui.reflog_state.select(None);
            self.log_ui.stash_state.select(None);
            self.refresh_log_diff();
            return;
        };

        if self.pending_job.is_some() {
            self.set_status("Busy");
            return;
        }

        let history_limit = self.log_ui.history_limit;
        let reflog_limit = self.log_ui.reflog_limit;
        let stash_limit = self.log_ui.stash_limit;
        let history_ref = self.log_ui.history_ref.clone();

        let (tx, rx) = mpsc::channel();
        self.pending_job = Some(PendingJob { rx });

        thread::spawn(move || {
            let history = git_ops::list_history(&repo_root, history_limit, history_ref.as_deref());
            let reflog = git_ops::list_reflog(&repo_root, reflog_limit);
            let stash = git_ops::list_stashes(&repo_root, stash_limit);
            let _ = tx.send(JobResult::LogReload {
                history_limit,
                reflog_limit,
                stash_limit,
                history,
                reflog,
                stash,
            });
        });
    }

    fn load_more_log_data(&mut self) {
        if self.pending_job.is_some() {
            self.set_status("Busy");
            return;
        }

        let Some(repo_root) = self.git.repo_root.clone() else {
            self.set_status("Not a git repository");
            return;
        };

        let (variant, limit) = match self.log_ui.subtab {
            LogSubTab::History => ("history", self.log_ui.history_limit.saturating_add(200)),
            LogSubTab::Reflog => ("reflog", self.log_ui.reflog_limit.saturating_add(200)),
            LogSubTab::Stash => ("stash", self.log_ui.stash_limit.saturating_add(200)),
            LogSubTab::Commands => {
                self.set_status("No more to load");
                return;
            }
        };

        let history_ref = self.log_ui.history_ref.clone();

        let (tx, rx) = mpsc::channel();
        self.pending_job = Some(PendingJob { rx });

        match variant {
            "history" => {
                thread::spawn(move || {
                    let result = git_ops::list_history(&repo_root, limit, history_ref.as_deref());
                    let _ = tx.send(JobResult::LogHistory { limit, result });
                });
            }
            "reflog" => {
                thread::spawn(move || {
                    let result = git_ops::list_reflog(&repo_root, limit);
                    let _ = tx.send(JobResult::LogReflog { limit, result });
                });
            }
            "stash" => {
                thread::spawn(move || {
                    let result = git_ops::list_stashes(&repo_root, limit);
                    let _ = tx.send(JobResult::LogStash { limit, result });
                });
            }
            _ => unreachable!(),
        }
    }

    fn maybe_load_more_log_data(&mut self) {
        if self.pending_job.is_some() {
            return;
        }

        let sel = self.log_ui.active_state().selected().unwrap_or(0);
        let active_len = self.active_log_len();
        if active_len == 0 {
            return;
        }

        let prefetch_start_idx = active_len.saturating_sub(10);
        if sel < prefetch_start_idx {
            return;
        }

        match self.log_ui.subtab {
            LogSubTab::History => {
                if !self.log_ui.history.is_empty()
                    && self.log_ui.history.len() == self.log_ui.history_limit
                {
                    self.load_more_log_data();
                }
            }
            LogSubTab::Reflog => {
                if !self.log_ui.reflog.is_empty()
                    && self.log_ui.reflog.len() == self.log_ui.reflog_limit
                {
                    self.load_more_log_data();
                }
            }
            LogSubTab::Stash => {
                if !self.log_ui.stash.is_empty()
                    && self.log_ui.stash.len() == self.log_ui.stash_limit
                {
                    self.load_more_log_data();
                }
            }
            LogSubTab::Commands => {}
        }
    }

    fn refresh_log_diff(&mut self) {
        self.log_ui.diff_request_id = self.log_ui.diff_request_id.wrapping_add(1);
        let request_id = self.log_ui.diff_request_id;

        self.log_ui.diff_scroll_y = 0;
        self.log_ui.diff_scroll_x = 0;

        self.log_ui.diff_lines = vec!["Loading diff…".to_string()];
        self.log_ui.diff_generation = self.log_ui.diff_generation.wrapping_add(1);
        self.log_diff_cache.invalidate();

        match self.log_ui.subtab {
            LogSubTab::History => {
                let Some(repo_root) = self.git.repo_root.clone() else {
                    self.log_ui.diff_lines = vec!["Not a git repository".to_string()];
                    self.log_ui.diff_generation = self.log_ui.diff_generation.wrapping_add(1);
                    self.log_diff_cache.invalidate();
                    return;
                };
                let Some(entry) = self.selected_history_entry() else {
                    self.log_ui.diff_lines = vec!["No commits".to_string()];
                    self.log_ui.diff_generation = self.log_ui.diff_generation.wrapping_add(1);
                    self.log_diff_cache.invalidate();
                    return;
                };

                let hash = entry.hash.clone();
                let detail_mode = self.log_ui.detail_mode;

                let wanted_file: Option<String> = if detail_mode == LogDetailMode::Files
                    && self.log_ui.files_hash.as_deref() == Some(hash.as_str())
                {
                    self.log_ui
                        .files_state
                        .selected()
                        .and_then(|sel| self.log_ui.files.get(sel))
                        .map(|f| f.path.clone())
                } else {
                    None
                };

                let (tx, rx) = mpsc::channel();
                self.log_diff_job = Some(PendingJob { rx });
                thread::spawn(move || {
                    let result: Result<LogDiffJobOutput, String> = match detail_mode {
                        LogDetailMode::Diff => {
                            match git_ops::show_commit(&repo_root, hash.as_str()) {
                                Ok(text) => Ok(LogDiffJobOutput {
                                    diff_lines: if text.trim().is_empty() {
                                        vec!["(no diff)".to_string()]
                                    } else {
                                        text.lines().map(|l| l.to_string()).collect()
                                    },
                                    files_hash: None,
                                    files: None,
                                    files_selected: None,
                                }),
                                Err(e) => Err(format!("git show failed: {}", e)),
                            }
                        }
                        LogDetailMode::Files => {
                            match git_ops::list_commit_files(&repo_root, hash.as_str()) {
                                Ok(files) => {
                                    if files.is_empty() {
                                        Ok(LogDiffJobOutput {
                                            diff_lines: vec!["No files".to_string()],
                                            files_hash: Some(hash.clone()),
                                            files: Some(files),
                                            files_selected: None,
                                        })
                                    } else {
                                        let selected_idx =
                                            wanted_file.as_deref().and_then(|wanted| {
                                                files.iter().position(|f| f.path.as_str() == wanted)
                                            });
                                        let idx = selected_idx.unwrap_or(0);
                                        let file = files
                                            .get(idx)
                                            .map(|f| f.path.clone())
                                            .unwrap_or_default();

                                        match git_ops::show_commit_file_diff(
                                            &repo_root,
                                            hash.as_str(),
                                            &file,
                                        ) {
                                            Ok(diff_text) => Ok(LogDiffJobOutput {
                                                diff_lines: if diff_text.trim().is_empty() {
                                                    vec!["(no diff)".to_string()]
                                                } else {
                                                    diff_text
                                                        .lines()
                                                        .map(|l| l.to_string())
                                                        .collect()
                                                },
                                                files_hash: Some(hash.clone()),
                                                files: Some(files),
                                                files_selected: Some(idx),
                                            }),
                                            Err(e) => Err(format!("git show failed: {}", e)),
                                        }
                                    }
                                }
                                Err(e) => Err(format!("git show failed: {}", e)),
                            }
                        }
                    };

                    let _ = tx.send(JobResult::LogDiff { request_id, result });
                });
            }
            LogSubTab::Reflog => {
                self.log_ui.diff_lines = vec!["Reflog is list-only; use Inspect (i)".to_string()];
                self.log_ui.diff_generation = self.log_ui.diff_generation.wrapping_add(1);
                self.log_diff_cache.invalidate();
            }
            LogSubTab::Stash => {
                let Some(entry) = self.selected_stash_entry() else {
                    self.log_ui.diff_lines = vec!["No stashes".to_string()];
                    self.log_ui.diff_generation = self.log_ui.diff_generation.wrapping_add(1);
                    self.log_diff_cache.invalidate();
                    return;
                };

                let selector = entry.selector.clone();
                let subject = entry.subject.clone();

                self.log_ui.diff_lines = vec![
                    selector,
                    String::new(),
                    subject,
                    String::new(),
                    "Keys: a/apply  p/pop  d/drop  Enter=apply".to_string(),
                ];
                self.log_ui.diff_generation = self.log_ui.diff_generation.wrapping_add(1);
                self.log_diff_cache.invalidate();
            }
            LogSubTab::Commands => {
                let Some(sel) = self.log_ui.command_state.selected() else {
                    self.log_ui.diff_lines = vec!["No commands".to_string()];
                    self.log_ui.diff_generation = self.log_ui.diff_generation.wrapping_add(1);
                    self.log_diff_cache.invalidate();
                    return;
                };
                let Some(entry) = self.git_log.get(sel) else {
                    return;
                };

                let mut lines = Vec::new();
                lines.push(format!("Command: {}", entry.cmd));
                lines.push(format!("Result: {}", if entry.ok { "OK" } else { "Error" }));
                lines.push(String::new());

                if let Some(detail) = entry.detail.as_deref() {
                    if detail.trim().is_empty() {
                        lines.push("(no output)".to_string());
                    } else {
                        lines.extend(detail.lines().map(|l| l.to_string()));
                    }
                } else {
                    lines.push("(no output)".to_string());
                }

                self.log_ui.diff_lines = lines;
                self.log_ui.diff_generation = self.log_ui.diff_generation.wrapping_add(1);
                self.log_diff_cache.invalidate();
            }
        }
    }

    fn active_log_len(&self) -> usize {
        match self.log_ui.subtab {
            LogSubTab::History => self.log_ui.history_filtered.len(),
            LogSubTab::Reflog => self.log_ui.reflog_filtered.len(),
            LogSubTab::Stash => self.log_ui.stash_filtered.len(),
            LogSubTab::Commands => self.git_log.len(),
        }
    }

    fn set_log_subtab(&mut self, subtab: LogSubTab) {
        self.log_ui.inspect.close();
        self.log_ui.set_subtab(subtab);

        if self.log_ui.subtab == LogSubTab::Reflog {
            self.log_ui.zoom = LogZoom::List;
            self.log_ui.focus = LogPaneFocus::Commits;
        }

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
            self.log_ui.update_filtered();

            if self.log_ui.subtab == LogSubTab::History && !self.log_ui.history_filtered.is_empty()
            {
                if self
                    .log_ui
                    .history_state
                    .selected()
                    .map(|i| i >= self.log_ui.history_filtered.len())
                    .unwrap_or(true)
                {
                    self.log_ui.history_state.select(Some(0));
                }
            }
            if self.log_ui.subtab == LogSubTab::Reflog && !self.log_ui.reflog_filtered.is_empty() {
                if self
                    .log_ui
                    .reflog_state
                    .selected()
                    .map(|i| i >= self.log_ui.reflog_filtered.len())
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

        let prev = self.log_ui.active_state().selected();
        if prev == Some(idx) {
            self.maybe_load_more_log_data();
            return;
        }

        self.log_ui.active_state_mut().select(Some(idx));
        self.log_ui.focus = LogPaneFocus::Commits;
        self.log_ui.diff_scroll_y = 0;
        self.log_ui.diff_scroll_x = 0;
        self.refresh_log_diff();
        self.maybe_load_more_log_data();
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
        if next == cur {
            self.maybe_load_more_log_data();
            return;
        }
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

    fn poll_git_refresh_job(&mut self) {
        let mut done: Option<JobResult> = None;
        if let Some(job) = &self.git_refresh_job {
            match job.rx.try_recv() {
                Ok(msg) => done = Some(msg),
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => {
                    done = Some(JobResult::GitRefresh {
                        request_id: self.git_refresh_request_id,
                        current_path: self.current_path.clone(),
                        result: Err("Git refresh job disconnected".to_string()),
                    });
                }
            }
        }

        if let Some(msg) = done {
            self.git_refresh_job = None;
            self.handle_job_result(msg);
        }
    }

    fn poll_git_diff_job(&mut self) {
        let mut done: Option<JobResult> = None;
        if let Some(job) = &self.git_diff_job {
            match job.rx.try_recv() {
                Ok(msg) => done = Some(msg),
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => {
                    done = Some(JobResult::GitDiff {
                        request_id: self.git.diff_request_id,
                        result: Err("Diff job disconnected".to_string()),
                    });
                }
            }
        }

        if let Some(msg) = done {
            self.git_diff_job = None;
            self.handle_job_result(msg);
        }
    }

    fn poll_log_diff_job(&mut self) {
        let mut done: Option<JobResult> = None;
        if let Some(job) = &self.log_diff_job {
            match job.rx.try_recv() {
                Ok(msg) => done = Some(msg),
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => {
                    done = Some(JobResult::LogDiff {
                        request_id: self.log_ui.diff_request_id,
                        result: Err("Diff job disconnected".to_string()),
                    });
                }
            }
        }

        if let Some(msg) = done {
            self.log_diff_job = None;
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
            JobResult::GitRefresh {
                request_id,
                current_path,
                result,
            } => {
                if request_id != self.git_refresh_request_id {
                    return;
                }

                // Remember current selection before refresh
                let prev_selected_path = self.git.selected_path();

                match result {
                    Ok(out) => {
                        self.git.repo_root = out.repo_root;
                        self.git.branch = out.branch;
                        self.git.ahead = out.ahead;
                        self.git.behind = out.behind;
                        self.git.entries = out.entries;
                        self.git.filtered.clear();
                        self.git.list_state.select(None);
                        self.git.selected_paths.clear();
                        self.git.selection_anchor = None;
                        let current_section = self.git.section;
                        self.git.set_section(current_section);
                        self.update_git_operation();

                        // Clear tree selection before rebuild
                        self.git.tree_state.select(None);

                        // Rebuild tree view
                        self.git.build_tree();

                        // Try to restore selection by path (file may have moved sections)
                        let found = if let Some(ref path) = prev_selected_path {
                            self.git.select_by_path(path)
                        } else {
                            false
                        };

                        // If not found, select first file
                        if !found && !self.git.flat_tree.is_empty() {
                            for (i, item) in self.git.flat_tree.iter().enumerate() {
                                if item.node_type == git::FlatNodeType::File {
                                    self.git.tree_state.select(Some(i));
                                    break;
                                }
                            }
                        }

                        // Update diff for new selection
                        if self.git.selected_tree_entry().is_some() {
                            self.request_git_diff_update();
                        } else {
                            self.git.diff_lines.clear();
                            self.git.diff_generation = self.git.diff_generation.wrapping_add(1);
                            self.git_diff_cache.invalidate();
                        }
                    }
                    Err(e) => {
                        self.set_status(e);
                        self.git.diff_lines.clear();
                        self.git.diff_generation = self.git.diff_generation.wrapping_add(1);
                        self.git_diff_cache.invalidate();
                    }
                }

                if self.current_path == current_path {
                    self.set_status("Git refreshed");
                }
            }
            JobResult::GitDiff { request_id, result } => {
                if request_id != self.git.diff_request_id {
                    return;
                }

                match result {
                    Ok(lines) => self.git.set_diff_lines(lines),
                    Err(e) => self.git.set_diff_lines(vec![e]),
                }

                self.git.diff_generation = self.git.diff_generation.wrapping_add(1);
                self.git_diff_cache.invalidate();
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
            JobResult::LogReload {
                history_limit,
                reflog_limit,
                stash_limit,
                history,
                reflog,
                stash,
            } => {
                self.log_ui.status = None;
                self.log_ui.history_limit = history_limit;
                self.log_ui.reflog_limit = reflog_limit;
                self.log_ui.stash_limit = stash_limit;

                let mut first_err: Option<String> = None;

                match history {
                    Ok(items) => self.log_ui.history = items,
                    Err(e) => {
                        if first_err.is_none() {
                            first_err = Some(e.clone());
                        }
                        self.log_ui.history.clear();
                    }
                }

                match reflog {
                    Ok(items) => self.log_ui.reflog = items,
                    Err(e) => {
                        if first_err.is_none() {
                            first_err = Some(e.clone());
                        }
                        self.log_ui.reflog.clear();
                    }
                }

                match stash {
                    Ok(items) => self.log_ui.stash = items,
                    Err(e) => {
                        if first_err.is_none() {
                            first_err = Some(e.clone());
                        }
                        self.log_ui.stash.clear();
                    }
                }

                self.log_ui.status = first_err;
                self.log_ui.update_filtered();
                self.refresh_log_diff();
            }
            JobResult::LogDiff { request_id, result } => {
                if request_id != self.log_ui.diff_request_id {
                    return;
                }

                match result {
                    Ok(out) => {
                        self.log_ui.diff_lines = out.diff_lines;
                        if let Some(files) = out.files {
                            self.log_ui.files = files;
                            self.log_ui.files_hash = out.files_hash;
                            self.log_ui
                                .files_state
                                .select(out.files_selected.or(Some(0)));
                        }
                    }
                    Err(e) => {
                        self.log_ui.diff_lines = vec![e];
                    }
                }

                self.log_ui.diff_generation = self.log_ui.diff_generation.wrapping_add(1);
                self.log_diff_cache.invalidate();
            }
            JobResult::LogHistory { limit, result } => {
                self.log_ui.status = None;
                self.log_ui.history_limit = limit;
                match result {
                    Ok(items) => self.log_ui.history = items,
                    Err(e) => self.log_ui.status = Some(e),
                }
                self.log_ui.update_filtered();
                self.refresh_log_diff();
            }
            JobResult::LogReflog { limit, result } => {
                self.log_ui.status = None;
                self.log_ui.reflog_limit = limit;
                match result {
                    Ok(items) => self.log_ui.reflog = items,
                    Err(e) => self.log_ui.status = Some(e),
                }
                self.log_ui.update_filtered();
                self.refresh_log_diff();
            }
            JobResult::LogStash { limit, result } => {
                self.log_ui.status = None;
                self.log_ui.stash_limit = limit;
                match result {
                    Ok(items) => self.log_ui.stash = items,
                    Err(e) => self.log_ui.status = Some(e),
                }
                self.log_ui.update_filtered();
                self.refresh_log_diff();
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

                let mut paths: Vec<String> = self.git.selected_tree_paths();

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

                let paths: Vec<String> = self.git.selected_tree_paths();

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

        let paths: Vec<String> = self.git.selected_tree_paths();

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

    fn show_delete_confirm(&mut self) {
        let Some(file) = self.selected_file().cloned() else {
            self.set_status("No selection");
            return;
        };
        self.delete_confirm = Some(DeleteConfirm {
            path: file.path.clone(),
            is_dir: file.is_dir,
        });
    }

    fn confirm_delete(&mut self) {
        let Some(confirm) = self.delete_confirm.take() else {
            return;
        };

        let result = if confirm.is_dir {
            fs::remove_dir_all(&confirm.path)
        } else {
            fs::remove_file(&confirm.path)
        };

        match result {
            Ok(_) => {
                let name = confirm.path.file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| confirm.path.display().to_string());
                self.set_status(format!("Deleted: {}", name));
                self.load_files();
            }
            Err(e) => {
                self.set_status(format!("Delete failed: {}", e));
            }
        }
    }

    fn revert_hunk(&mut self, hunk_idx: usize) {
        if self.pending_job.is_some() {
            self.set_status("Busy");
            return;
        }

        let Some(repo_root) = self.git.repo_root.clone() else {
            self.set_status("Not a git repository");
            return;
        };

        let Some(hunk) = self.git.diff_hunks.get(hunk_idx) else {
            self.set_status("Invalid hunk");
            return;
        };

        // Build patch content from hunk lines
        let patch_content = hunk.lines.join("\n") + "\n";

        self.start_git_job("revert hunk".to_string(), true, false, move || {
            git_ops::apply_patch_reverse(&repo_root, &patch_content)
        });
    }

    fn revert_block(&mut self, block_idx: usize) {
        let Some(repo_root) = self.git.repo_root.clone() else {
            self.set_status("Not a git repository");
            return;
        };

        let Some(block) = self.git.change_blocks.get(block_idx).cloned() else {
            self.set_status("Invalid block");
            return;
        };

        // Direct file manipulation: replace new_lines with old_lines
        let file_path = repo_root.join(&block.file_path);
        let new_start = block.new_start as usize;
        let new_lines = block.new_lines.clone();
        let old_lines = block.old_lines.clone();

        // Read the file
        let content = match std::fs::read_to_string(&file_path) {
            Ok(c) => c,
            Err(e) => {
                self.set_status(format!("Failed to read file: {}", e));
                return;
            }
        };

        let lines: Vec<&str> = content.lines().collect();

        // Calculate the range to replace (1-indexed to 0-indexed)
        let start_idx = new_start.saturating_sub(1);
        let end_idx = start_idx + new_lines.len();

        if end_idx > lines.len() {
            self.set_status("Line numbers out of range");
            return;
        }

        // Build new content: lines before + old_lines + lines after
        let mut new_content = String::new();
        for line in &lines[..start_idx] {
            new_content.push_str(line);
            new_content.push('\n');
        }
        for line in &old_lines {
            new_content.push_str(line);
            new_content.push('\n');
        }
        for line in &lines[end_idx..] {
            new_content.push_str(line);
            new_content.push('\n');
        }

        // Handle trailing newline
        if !content.ends_with('\n') && new_content.ends_with('\n') {
            new_content.pop();
        }

        // Write the file
        if let Err(e) = std::fs::write(&file_path, &new_content) {
            self.set_status(format!("Failed to write file: {}", e));
            return;
        }

        self.set_status("Reverted");
        self.refresh_git_state();
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
        let Some(entry) = self.git.selected_tree_entry() else {
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

            if read_path.parent().is_some() {
                items.insert(
                    0,
                    FileEntry {
                        name: "..".to_string(),
                        path: read_path.clone(),
                        is_dir: true,
                        is_symlink: false,
                        is_exec: false,
                        is_hidden: false,
                        size: 0,
                    },
                );
            }

            self.files = items;
        }
        self.preview_scroll = 0;
        self.update_preview();
        // Update directory modification time
        self.dir_mtime = fs::metadata(&self.current_path)
            .ok()
            .and_then(|m| m.modified().ok());
    }

    fn check_auto_refresh(&mut self) {
        if !self.auto_refresh {
            return;
        }
        // Only check every second
        if self.last_dir_check.elapsed() < Duration::from_secs(1) {
            return;
        }
        self.last_dir_check = Instant::now();

        // Get current mtime of directory
        let current_mtime = fs::metadata(&self.current_path)
            .ok()
            .and_then(|m| m.modified().ok());

        // If mtime changed, refresh
        if current_mtime != self.dir_mtime {
            let selected_name = self.selected_file().map(|f| f.name.clone());
            self.load_files();
            // Try to restore selection
            if let Some(name) = selected_name {
                if let Some(idx) = self.files.iter().position(|f| f.name == name) {
                    self.list_state.select(Some(idx));
                }
            }
        }
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
        self.git_diff_cache.invalidate();
        self.log_diff_cache.invalidate();
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
            || self.stash_ui.open
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
            CommandId::OpenAuthorPicker => self.open_author_picker(),
            CommandId::OpenStashPicker => self.open_stash_picker(),
            CommandId::ClearGitLog => {
                self.git_log.clear();
                self.log_ui.command_state.select(None);
                self.log_ui.diff_lines.clear();
                self.set_status("Commands cleared");
            }
            CommandId::QuickStash => {
                self.start_operation_job("git stash", true);
            }
            CommandId::CheckUpdate => {
                self.check_for_updates();
            }
            CommandId::Quit => self.should_quit = true,
        }
    }

    fn check_for_updates(&mut self) {
        self.set_status("Checking for updates...");

        // Query crates.io API for latest version
        let result: Result<String, String> = (|| {
            let resp = ureq::get("https://crates.io/api/v1/crates/lzgit")
                .set("User-Agent", "lzgit")
                .call()
                .map_err(|e| format!("Network error: {}", e))?;

            let json: serde_json::Value = resp.into_json()
                .map_err(|e| format!("Parse error: {}", e))?;

            let latest = json["crate"]["max_version"]
                .as_str()
                .ok_or("Could not get version")?;

            Ok(latest.to_string())
        })();

        match result {
            Ok(latest) => {
                if latest == VERSION {
                    self.set_status(&format!("You're up to date! (v{})", VERSION));
                } else {
                    // Show update confirmation dialog
                    self.update_confirm = Some(latest);
                }
            }
            Err(e) => {
                self.set_status(&format!("Update check failed: {}", e));
            }
        }
    }

    fn confirm_update(&mut self) {
        if let Some(new_version) = self.update_confirm.take() {
            self.set_status(&format!("Updating to v{}...", new_version));
            self.start_operation_job("cargo install lzgit --force", false);
        }
    }

    fn maybe_expire_status(&mut self) -> bool {
        let should_clear = self
            .status_message
            .as_ref()
            .is_some_and(|(_, t)| t.elapsed() >= self.status_ttl);
        if should_clear {
            self.status_message = None;
        }
        should_clear
    }

    fn tick_pending_menu_action(&mut self) -> bool {
        let Some((idx, armed)) = self.pending_menu_action else {
            return false;
        };

        if armed {
            self.pending_menu_action = None;
            self.execute_menu_action(idx);
            true
        } else {
            self.pending_menu_action = Some((idx, true));
            false
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
        if let Some(w) = settings.git_left_width {
            self.git_left_width = w.clamp(32, 90);
        }

        if let Some(theme) = settings.theme {
            self.set_theme(theme);
        }

        if let Some(wrap) = settings.wrap_diff {
            self.wrap_diff = wrap;
        }
        if let Some(syntax) = settings.syntax_highlight {
            self.syntax_highlight = syntax;
        }

        if let Some(side) = settings.git_side_by_side {
            self.git.diff_mode = if side {
                GitDiffMode::SideBySide
            } else {
                GitDiffMode::Unified
            };
        }
        if let Some(z) = settings.git_zoom_diff {
            self.git_zoom_diff = z;
        }

        if let Some(side) = settings.log_side_by_side {
            self.log_ui.diff_mode = if side {
                GitDiffMode::SideBySide
            } else {
                GitDiffMode::Unified
            };
        }

        if let Some(z) = settings.log_zoom {
            self.log_ui.zoom = z;
        }

        if let Some(m) = settings.log_detail_mode {
            self.log_ui.detail_mode = m;
        }
    }

    fn save_persisted_ui_settings(&mut self) {
        let Some(path) = self.ui_settings_path.clone() else {
            return;
        };

        let settings = PersistedUiSettings {
            log_left_width: Some(self.log_ui.left_width),
            git_left_width: Some(self.git_left_width),
            theme: Some(self.theme),
            wrap_diff: Some(self.wrap_diff),
            syntax_highlight: Some(self.syntax_highlight),
            git_side_by_side: Some(self.git.diff_mode == GitDiffMode::SideBySide),
            git_zoom_diff: Some(self.git_zoom_diff),
            log_side_by_side: Some(self.log_ui.diff_mode == GitDiffMode::SideBySide),
            log_zoom: Some(self.log_ui.zoom),
            log_detail_mode: Some(self.log_ui.detail_mode),
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
            if file.name == ".." {
                self.go_parent();
            } else {
                self.navigate_to(file.path);
            }
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

    fn open_selected_in_editor(&mut self) {
        let Some(file) = self.selected_file() else {
            return;
        };
        if file.is_dir {
            return;
        }

        let editor = env::var("EDITOR").ok().filter(|s| !s.trim().is_empty());
        let cmd = editor.unwrap_or_else(|| "vim".to_string());

        // Properly leave TUI mode
        let _ = disable_raw_mode();
        let _ = execute!(
            io::stdout(),
            LeaveAlternateScreen,
            DisableMouseCapture,
            crossterm::cursor::Show
        );
        let _ = io::stdout().flush();

        // Run editor
        let status = std::process::Command::new(cmd.as_str())
            .arg(file.path.as_os_str())
            .stdin(std::process::Stdio::inherit())
            .stdout(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::inherit())
            .status();

        // Restore TUI mode - order matters!
        let _ = enable_raw_mode();
        let _ = execute!(
            io::stdout(),
            EnterAlternateScreen,
            crossterm::terminal::Clear(crossterm::terminal::ClearType::All),
            crossterm::terminal::Clear(crossterm::terminal::ClearType::Purge),
            crossterm::cursor::MoveTo(0, 0),
            crossterm::cursor::Hide,
            EnableMouseCapture
        );
        let _ = io::stdout().flush();

        match status {
            Ok(s) if s.success() => self.set_status("Editor closed"),
            Ok(_) => self.set_status("Editor exited with error"),
            Err(e) => self.set_status(format!("Editor failed: {}", e)),
        }

        // Request full terminal redraw after editor
        self.needs_full_redraw = true;
        self.load_files();
        self.update_preview();
    }

    fn handle_click(&mut self, row: u16, col: u16, modifiers: KeyModifiers) {
        if self.theme_picker.open || self.command_palette.open {
            self.context_menu = None;
            self.pending_menu_action = None;

            let (tw, th) = crossterm::terminal::size().unwrap_or((0, 0));
            let area = Rect::new(0, 0, tw, th);

            if self.command_palette.open {
                let w = area.width.min(56).saturating_sub(2).max(32);
                let desired_h = COMMAND_PALETTE_ITEMS.len() as u16 + 6;
                let h = desired_h.min(area.height.saturating_sub(2)).max(10);
                let x = area.x + (area.width.saturating_sub(w)) / 2;
                let y = area.y + (area.height.saturating_sub(h)) / 2;
                let modal = Rect::new(x, y, w, h);

                if col < modal.x
                    || col >= modal.x + modal.width
                    || row < modal.y
                    || row >= modal.y + modal.height
                {
                    self.command_palette.open = false;
                    return;
                }

                let inner = modal.inner(Margin {
                    vertical: 1,
                    horizontal: 2,
                });
                let rows = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([Constraint::Min(0), Constraint::Length(1)])
                    .split(inner);

                let list_inner = rows[0].inner(Margin {
                    vertical: 1,
                    horizontal: 1,
                });

                if row >= list_inner.y && row < list_inner.y + list_inner.height {
                    let offset = self.command_palette.list_state.offset();
                    let idx = offset + (row - list_inner.y) as usize;
                    if idx < COMMAND_PALETTE_ITEMS.len() {
                        let was_selected = self.command_palette.list_state.selected() == Some(idx);
                        self.command_palette.list_state.select(Some(idx));
                        if was_selected {
                            self.run_command_palette_selection();
                        }
                    }
                }
                return;
            }

            if self.theme_picker.open {
                let w = 35u16.min(area.width.saturating_sub(2)).max(30);
                let h = 11u16.min(area.height.saturating_sub(2)).max(9);
                let x = area.x + (area.width.saturating_sub(w)) / 2;
                let y = area.y + (area.height.saturating_sub(h)) / 2;
                let modal = Rect::new(x, y, w, h);

                if col < modal.x
                    || col >= modal.x + modal.width
                    || row < modal.y
                    || row >= modal.y + modal.height
                {
                    self.theme_picker.open = false;
                    return;
                }

                let inner = modal.inner(Margin {
                    vertical: 1,
                    horizontal: 2,
                });
                let rows = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([Constraint::Min(0), Constraint::Length(1)])
                    .split(inner);

                let list_inner = rows[0].inner(Margin {
                    vertical: 1,
                    horizontal: 1,
                });

                if row >= list_inner.y && row < list_inner.y + list_inner.height {
                    let offset = self.theme_picker.list_state.offset();
                    let idx = offset + (row - list_inner.y) as usize;
                    if idx < THEME_ORDER.len() {
                        let was_selected = self.theme_picker.list_state.selected() == Some(idx);
                        self.theme_picker.list_state.select(Some(idx));
                        if was_selected {
                            self.apply_theme_picker_selection();
                        }
                    }
                }
                return;
            }
        }

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
                    self.start_git_refresh_job();
                } else if tab == Tab::Log {
                    self.refresh_log_data();
                }
            }
            AppAction::RefreshGit => {
                self.start_git_refresh_job();
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
                self.git.set_section(section);
                self.git.selected_paths.clear();
                self.git.selection_anchor = None;
                self.request_git_diff_update();
            }
            AppAction::SelectGitFile(idx) => {
                self.git.select_filtered(idx);
                self.request_git_diff_update();

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
            AppAction::SelectGitTreeItem(idx) => {
                self.git.select_tree(idx);

                // Handle selection based on item type
                if let Some(item) = self.git.flat_tree.get(idx) {
                    use git::FlatNodeType;
                    match item.node_type {
                        FlatNodeType::Section | FlatNodeType::Directory => {
                            // Toggle expand/collapse on click
                            self.git.toggle_tree_expand();
                        }
                        FlatNodeType::File => {
                            // Handle file selection with modifiers
                            if let Some(entry_idx) = item.entry_idx {
                                if let Some(entry) = self.git.entries.get(entry_idx) {
                                    if modifiers.contains(KeyModifiers::SHIFT) {
                                        let anchor = self.git.selection_anchor.unwrap_or(idx);
                                        let (a, b) = if anchor <= idx { (anchor, idx) } else { (idx, anchor) };
                                        self.git.selected_paths.clear();
                                        for i in a..=b {
                                            if let Some(item) = self.git.flat_tree.get(i) {
                                                if item.node_type == FlatNodeType::File {
                                                    if let Some(e_idx) = item.entry_idx {
                                                        if let Some(e) = self.git.entries.get(e_idx) {
                                                            self.git.selected_paths.insert(e.path.clone());
                                                        }
                                                    }
                                                }
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
                                        self.git.selection_anchor = Some(idx);
                                    }
                                    self.request_git_diff_update();
                                }
                            }
                        }
                    }
                }
            }
            AppAction::ToggleGitTreeExpand => {
                self.git.toggle_tree_expand();
            }
            AppAction::RevertHunk(hunk_idx) => {
                self.revert_hunk(hunk_idx);
            }
            AppAction::RevertBlock(block_idx) => {
                self.revert_block(block_idx);
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
                if self.log_ui.inspect.open {
                    self.log_ui.inspect.close();
                } else {
                    self.open_log_inspect();
                }
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
                self.log_ui.inspect.close();
            }
            AppAction::LogInspectCopySecondary => {
                if let Some(s) = self.selected_log_subject() {
                    self.request_copy_to_clipboard(s);
                } else if !self.log_ui.inspect.body.is_empty() {
                    self.request_copy_to_clipboard(self.log_ui.inspect.body.clone());
                }
                self.log_ui.inspect.close();
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
            AppAction::OpenLogBranchPicker => self.open_log_branch_picker(),
            AppAction::CloseBranchPicker => self.close_branch_picker(),
            AppAction::SelectBranch(idx) => {
                self.branch_ui.list_state.select(Some(idx));
            }
            AppAction::SelectLogBranch(idx) => {
                let was_selected = self.branch_ui.list_state.selected() == Some(idx);
                self.branch_ui.list_state.select(Some(idx));
                if was_selected {
                    self.confirm_log_branch_picker();
                }
            }
            AppAction::ConfirmLogBranchPicker => self.confirm_log_branch_picker(),
            AppAction::OpenAuthorPicker => self.open_author_picker(),
            AppAction::CloseAuthorPicker => self.close_author_picker(),
            AppAction::SelectAuthor(idx) => {
                let was_selected = self.author_ui.list_state.selected() == Some(idx);
                self.author_ui.list_state.select(Some(idx));
                if was_selected {
                    self.confirm_author_picker();
                }
            }
            AppAction::BranchCheckout => self.branch_checkout_selected(false),
            AppAction::ConfirmBranchCheckout => self.branch_checkout_selected(true),
            AppAction::CancelBranchCheckout => {
                self.branch_ui.confirm_checkout = None;
            }
            AppAction::OpenStashPicker => self.open_stash_picker(),
            AppAction::CloseStashPicker => self.close_stash_picker(),
            AppAction::SelectStash(idx) => {
                self.stash_ui.list_state.select(Some(idx));
            }
            AppAction::StashApply => self.stash_apply_selected(),
            AppAction::StashPop => {
                self.stash_ui.status = None;
                let Some(sel) = self.stash_ui.selected_stash() else {
                    self.set_stash_status("No stash selected");
                    return;
                };
                self.open_stash_confirm(StashConfirmAction::Pop, sel.selector.clone());
            }
            AppAction::StashDrop => {
                self.stash_ui.status = None;
                let Some(sel) = self.stash_ui.selected_stash() else {
                    self.set_stash_status("No stash selected");
                    return;
                };
                self.open_stash_confirm(StashConfirmAction::Drop, sel.selector.clone());
            }
            AppAction::ConfirmStashAction => self.confirm_stash_action(),
            AppAction::CancelStashAction => {
                self.stash_confirm = None;
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
                self.git.set_section(section);
                self.git.selected_paths.clear();
                self.git.selection_anchor = None;
                self.request_git_diff_update();
            }
            AppAction::SelectGitFile(idx) => {
                self.git.select_filtered(idx);
                self.request_git_diff_update();

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
                options.push((" 🗑️  Delete ".to_string(), ContextCommand::Delete));

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
                    if self.selected_history_entry().is_none() {
                        return;
                    }

                    options.push((" 📋 Copy SHA ".to_string(), ContextCommand::LogCopySha));
                    options.push((
                        " 📋 Copy Subject ".to_string(),
                        ContextCommand::LogCopySubject,
                    ));
                }
                LogSubTab::Reflog => {
                    if self.selected_reflog_entry().is_none() {
                        return;
                    }

                    options.push((" 📋 Copy SHA ".to_string(), ContextCommand::LogCopySha));
                    options.push((
                        " 📋 Copy Subject ".to_string(),
                        ContextCommand::LogCopySubject,
                    ));
                }
                LogSubTab::Stash => {
                    let Some(_entry) = self.selected_stash_entry() else {
                        return;
                    };
                    options.push((" 📋 Copy Selector ".to_string(), ContextCommand::LogCopySha));
                    options.push((
                        " 📋 Copy Subject ".to_string(),
                        ContextCommand::LogCopySubject,
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
            Tab::Terminal => return, // No context menu for terminal
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
                ContextCommand::Delete => self.show_delete_confirm(),
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
            }
        }
        self.context_menu = None;
    }

    fn selected_git_paths(&self) -> Vec<String> {
        self.git.selected_tree_paths()
    }

    fn selected_history_entry(&self) -> Option<&git_ops::CommitEntry> {
        let sel = self.log_ui.history_state.selected()?;
        let idx = *self.log_ui.history_filtered.get(sel)?;
        self.log_ui.history.get(idx)
    }

    fn selected_reflog_entry(&self) -> Option<&git_ops::ReflogEntry> {
        let sel = self.log_ui.reflog_state.selected()?;
        let idx = *self.log_ui.reflog_filtered.get(sel)?;
        self.log_ui.reflog.get(idx)
    }

    fn selected_stash_entry(&self) -> Option<&git_ops::StashEntry> {
        let sel = self.log_ui.stash_state.selected()?;
        let idx = *self.log_ui.stash_filtered.get(sel)?;
        self.log_ui.stash.get(idx)
    }

    fn selected_log_hash(&self) -> Option<String> {
        match self.log_ui.subtab {
            LogSubTab::History => self.selected_history_entry().map(|e| e.hash.clone()),
            LogSubTab::Reflog => self.selected_reflog_entry().map(|e| e.hash.clone()),
            LogSubTab::Stash => self.selected_stash_entry().map(|e| e.selector.clone()),
            LogSubTab::Commands => self
                .log_ui
                .command_state
                .selected()
                .and_then(|i| self.git_log.get(i))
                .map(|e| e.cmd.clone()),
        }
    }

    fn selected_log_subject(&self) -> Option<String> {
        match self.log_ui.subtab {
            LogSubTab::History => self.selected_history_entry().map(|e| e.subject.clone()),
            LogSubTab::Reflog => self.selected_reflog_entry().map(|e| e.subject.clone()),
            LogSubTab::Stash => self.selected_stash_entry().map(|e| e.subject.clone()),
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

    fn open_log_inspect(&mut self) {
        let (title, body) = match self.log_ui.subtab {
            LogSubTab::History => {
                let Some(e) = self.selected_history_entry() else {
                    self.set_status("No selection");
                    return;
                };

                let title = format!("Inspect {}", e.short);

                let body = if let Some(repo_root) = self.git.repo_root.clone() {
                    match git_ops::show_commit_header(&repo_root, &e.hash) {
                        Ok(text) => text,
                        Err(err) => {
                            let mut out = String::new();
                            out.push_str("git show failed: ");
                            out.push_str(&err);
                            out.push('\n');
                            out.push('\n');
                            out.push_str("SHA: ");
                            out.push_str(&e.hash);
                            out.push('\n');
                            let badges = git_decoration_tokens(&e.decoration)
                                .into_iter()
                                .take(8)
                                .map(|t| format!("[{}]", t))
                                .collect::<Vec<_>>()
                                .join(" ");
                            if !badges.is_empty() {
                                out.push_str("Refs: ");
                                out.push_str(&badges);
                                out.push('\n');
                            }
                            out.push_str("Date: ");
                            out.push_str(&e.date);
                            out.push('\n');
                            out.push_str("Author: ");
                            out.push_str(&e.author);
                            out.push('\n');
                            out.push('\n');
                            out.push_str("Subject:\n");
                            out.push_str(&e.subject);
                            out.push('\n');
                            out
                        }
                    }
                } else {
                    let mut out = String::new();
                    out.push_str("SHA: ");
                    out.push_str(&e.hash);
                    out.push('\n');
                    let badges = git_decoration_tokens(&e.decoration)
                        .into_iter()
                        .take(8)
                        .map(|t| format!("[{}]", t))
                        .collect::<Vec<_>>()
                        .join(" ");
                    if !badges.is_empty() {
                        out.push_str("Refs: ");
                        out.push_str(&badges);
                        out.push('\n');
                    }
                    out.push_str("Date: ");
                    out.push_str(&e.date);
                    out.push('\n');
                    out.push_str("Author: ");
                    out.push_str(&e.author);
                    out.push('\n');
                    out.push('\n');
                    out.push_str("Subject:\n");
                    out.push_str(&e.subject);
                    out.push('\n');
                    out
                };

                (title, body)
            }
            LogSubTab::Reflog => {
                let Some(e) = self.selected_reflog_entry() else {
                    self.set_status("No selection");
                    return;
                };

                let title = format!("Inspect {}", e.selector);

                let body = if let Some(repo_root) = self.git.repo_root.clone() {
                    match git_ops::show_commit_header(&repo_root, &e.hash) {
                        Ok(text) => text,
                        Err(err) => {
                            let mut out = String::new();
                            out.push_str("git show failed: ");
                            out.push_str(&err);
                            out.push('\n');
                            out.push('\n');
                            out.push_str("SHA: ");
                            out.push_str(&e.hash);
                            out.push('\n');
                            out.push_str("Selector: ");
                            out.push_str(&e.selector);
                            out.push('\n');
                            out.push('\n');
                            out.push_str("Subject:\n");
                            out.push_str(&e.subject);
                            out.push('\n');
                            out
                        }
                    }
                } else {
                    let mut out = String::new();
                    out.push_str("SHA: ");
                    out.push_str(&e.hash);
                    out.push('\n');
                    out.push_str("Selector: ");
                    out.push_str(&e.selector);
                    out.push('\n');
                    let badges = git_decoration_tokens(&e.decoration)
                        .into_iter()
                        .take(8)
                        .map(|t| format!("[{}]", t))
                        .collect::<Vec<_>>()
                        .join(" ");
                    if !badges.is_empty() {
                        out.push_str("Refs: ");
                        out.push_str(&badges);
                        out.push('\n');
                    }
                    out.push('\n');
                    out.push_str("Subject:\n");
                    out.push_str(&e.subject);
                    out.push('\n');
                    out
                };

                (title, body)
            }
            LogSubTab::Stash => {
                let Some(e) = self.selected_stash_entry() else {
                    self.set_status("No selection");
                    return;
                };

                let mut body = String::new();
                body.push_str("Selector: ");
                body.push_str(&e.selector);
                body.push('\n');
                body.push('\n');
                body.push_str("Message:\n");
                body.push_str(&e.subject);
                body.push('\n');
                body.push('\n');
                body.push_str("Keys: a/apply  p/pop  d/drop");
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
            LogZoom::List => {
                self.log_ui.focus = LogPaneFocus::Commits;
                self.log_ui.inspect.close();
            }
            LogZoom::None => {}
        }
    }

    fn cycle_log_focus(&mut self) {
        let files_mode = self.log_ui.detail_mode == LogDetailMode::Files
            && self.log_ui.subtab == LogSubTab::History;

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

    fn adjust_git_left_width(&mut self, delta: i16) {
        let cur = self.git_left_width as i16;
        let next = (cur + delta).clamp(32, 90);
        self.git_left_width = next as u16;
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
            Tab::Log | Tab::Terminal => {
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

#[derive(Default, Debug)]
struct LogFilterQuery {
    author: Vec<String>,
    refs: Vec<String>,
    tokens: Vec<String>,
}

fn split_query_tokens(input: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;

    for ch in input.chars() {
        match quote {
            Some(q) => {
                cur.push(ch);
                if ch == q {
                    quote = None;
                }
            }
            None => {
                if ch == '"' || ch == '\'' {
                    quote = Some(ch);
                    cur.push(ch);
                } else if ch.is_whitespace() {
                    let t = cur.trim();
                    if !t.is_empty() {
                        out.push(t.to_string());
                    }
                    cur.clear();
                } else {
                    cur.push(ch);
                }
            }
        }
    }

    let t = cur.trim();
    if !t.is_empty() {
        out.push(t.to_string());
    }

    out
}

fn parse_log_filter_query(input: &str) -> LogFilterQuery {
    let mut q = LogFilterQuery::default();

    for raw in split_query_tokens(input) {
        let t = raw.trim();
        if t.is_empty() {
            continue;
        }

        fn strip_quotes(s: &str) -> &str {
            let s = s.trim();
            if s.len() >= 2 {
                if let Some(rest) = s.strip_prefix('"').and_then(|x| x.strip_suffix('"')) {
                    return rest;
                }
                if let Some(rest) = s.strip_prefix('\'').and_then(|x| x.strip_suffix('\'')) {
                    return rest;
                }
            }
            s
        }

        if let Some(rest) = t.strip_prefix('@') {
            let rest = strip_quotes(rest);
            if !rest.is_empty() {
                q.author.push(rest.to_string());
            }
            continue;
        }

        if let Some(rest) = t.strip_prefix("author:").or_else(|| t.strip_prefix("a:")) {
            let rest = strip_quotes(rest);
            if !rest.is_empty() {
                q.author.push(rest.to_string());
            }
            continue;
        }

        if let Some(rest) = t.strip_prefix("ref:").or_else(|| t.strip_prefix("tag:")) {
            let rest = strip_quotes(rest);
            if !rest.is_empty() {
                q.refs.push(rest.to_string());
            }
            continue;
        }

        q.tokens.push(t.to_string());
    }

    q
}

fn fuzzy_score(haystack: &str, needle: &str) -> Option<i32> {
    let n = needle.trim();
    if n.is_empty() {
        return Some(0);
    }

    let mut score: i32 = 0;
    let mut last_match: Option<usize> = None;
    let mut pos = 0usize;

    for ch in n.chars() {
        let mut found_at: Option<usize> = None;
        for (i, hc) in haystack[pos..].char_indices() {
            if hc == ch {
                found_at = Some(pos + i);
                break;
            }
        }
        let idx = found_at?;

        score += 10;
        if let Some(prev) = last_match {
            if idx == prev + 1 {
                score += 15;
            } else {
                let gap = idx.saturating_sub(prev + 1) as i32;
                score -= gap.min(30);
            }
        } else {
            score += (30 - idx as i32).max(0);
        }

        last_match = Some(idx);
        pos = idx + ch.len_utf8();
    }

    Some(score)
}

fn token_score(haystack: &str, token: &str) -> Option<i32> {
    let t = token.trim();
    if t.is_empty() {
        return Some(0);
    }

    if haystack.contains(t) {
        return Some(200 + (t.chars().count() as i32) * 5);
    }

    let score = fuzzy_score(haystack, t)?;
    let len = t.chars().count() as i32;

    if len >= 4 && score < len * 10 {
        return None;
    }

    Some(score)
}

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

fn draw_ui(f: &mut Frame, app: &mut App) -> Vec<ClickZone> {
    let mut zones = Vec::new();
    let area = f.area();

    f.render_widget(Block::default().bg(app.palette.bg), area);

    let main_layout = if app.current_tab == Tab::Git {
        let commit_h = if app.commit.open { 11 } else { 1 };
        let footer_h = if app.git_zoom_diff { 0 } else { 3 };
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(0),
                Constraint::Length(commit_h),
                Constraint::Length(footer_h),
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
        (" Git ", Tab::Git),
        (" History ", Tab::Log),
        (" Explorer ", Tab::Explorer),
        (" Terminal ", Tab::Terminal),
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

            let refresh_icon = "⟳";
            spans.push(Span::raw(format!(
                "   ↑{} ↓{}{}  ",
                app.git.ahead, app.git.behind, op
            )));
            spans.push(Span::styled(
                format!(" {} ", refresh_icon),
                Style::default()
                    .fg(app.palette.btn_fg)
                    .bg(app.palette.accent_secondary)
                    .add_modifier(Modifier::BOLD),
            ));

            f.render_widget(
                Paragraph::new(Line::from(spans)).style(Style::default().fg(app.palette.fg)),
                Rect::new(base_x, second_row_y, width, 1),
            );

            let enabled = app.pending_job.is_none();

            let refresh_prefix = format!(
                " Repo: {}   Branch: {}   ↑{} ↓{}{}  ",
                repo, branch_text, app.git.ahead, app.git.behind, op
            );
            let refresh_x = base_x + display_width(refresh_prefix.as_str()) as u16;
            let refresh_rect = Rect::new(refresh_x, second_row_y, 3, 1);
            if enabled {
                zones.push(ClickZone {
                    rect: refresh_rect,
                    action: AppAction::RefreshGit,
                });
            }

            let mut cursor = base_x + width;

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
                LogSubTab::Stash => "Stash",
                LogSubTab::Commands => "Commands",
            };

            let branch = if app.git.branch.is_empty() {
                "(unknown)".to_string()
            } else {
                app.git.branch.clone()
            };

            let width = top_bar.width.saturating_sub(2);
            let base_x = top_bar.x + 2;

            let mut spans: Vec<Span> = Vec::new();
            spans.push(Span::raw(format!(" History: {}   ", sub)));
            spans.push(Span::raw("View: "));

            let view_ref = app.log_ui.history_ref.as_deref().unwrap_or_else(|| {
                if branch.is_empty() {
                    "HEAD"
                } else {
                    branch.as_str()
                }
            });

            let branch_text = format!("{} ▼", view_ref);
            let branch_prefix_len = format!(" History: {}   View: ", sub).len();
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
                action: AppAction::OpenLogBranchPicker,
            });

            spans.push(Span::raw(format!(
                "   (current: {})",
                if branch.is_empty() {
                    "HEAD"
                } else {
                    branch.as_str()
                }
            )));

            f.render_widget(
                Paragraph::new(Line::from(spans)).style(Style::default().fg(app.palette.fg)),
                Rect::new(base_x, second_row_y, width, 1),
            );
        }
        Tab::Terminal => {
            // Show terminal title
            let title = " Terminal (shell) ";
            f.render_widget(
                Paragraph::new(title).style(Style::default().fg(app.palette.accent_secondary)),
                Rect::new(top_bar.x + 2, second_row_y, title.len() as u16, 1),
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
                .border_set(ratatui::symbols::border::PLAIN)
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
                .border_set(ratatui::symbols::border::PLAIN)
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
                    .border_set(ratatui::symbols::border::PLAIN)
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
                    use git::FlatNodeType;
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
                            ListItem::new(Line::from(vec![
                                Span::styled(label, Style::default()
                                    .fg(section_color)
                                    .add_modifier(Modifier::BOLD)),
                            ]))
                        }
                        FlatNodeType::Directory => {
                            // Directory with expand/collapse
                            let arrow = if item.expanded { "▾" } else { "▸" };
                            let label = format!("{}{}  {}/", indent, arrow, item.name);
                            ListItem::new(Line::from(vec![
                                Span::styled(label, Style::default().fg(app.palette.dir_color)),
                            ]))
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
                                        Span::styled(format!("{} ", checkbox), Style::default().fg(app.palette.border_inactive)),
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
                                        list_item = list_item.style(Style::default().bg(app.palette.menu_bg));
                                    }
                                    return list_item;
                                }
                            }
                            // Fallback
                            ListItem::new(Line::from(vec![
                                Span::raw(format!("{}  {}", indent, item.name)),
                            ]))
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
                let rect = Rect::new(
                    tree_inner.x,
                    tree_inner.y + i as u16,
                    tree_inner.width,
                    1,
                );
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
                let mut scroll_state =
                    ScrollbarState::new(app.git.flat_tree.len()).position(app.git.tree_state.selected().unwrap_or(0));
                f.render_stateful_widget(
                    scrollbar,
                    tree_area.inner(Margin {
                        vertical: 1,
                        horizontal: 0,
                    }),
                    &mut scroll_state,
                );
            }

            let in_conflict_view = app.git.selected_tree_entry().is_some_and(|e| e.is_conflict);

            if in_conflict_view {
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
                    .border_set(ratatui::symbols::border::PLAIN)
                    .border_style(Style::default().fg(app.palette.border_inactive))
                    .title(format!(" Diff ({}) ", mode_label));

                let cache_width = diff_area.width.saturating_sub(2).max(1);
                let cache_scroll_x =
                    if app.git.diff_mode == GitDiffMode::SideBySide && !app.wrap_diff {
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
                            GitDiffMode::Unified => {
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
                                        // Show filename first, then directory
                                        let (dir, filename) = match full_path.rfind('/') {
                                            Some(i) => (&full_path[..i+1], &full_path[i+1..]),
                                            None => ("", full_path),
                                        };
                                        let mut spans = vec![
                                            Span::styled(
                                                format!("📄 {}", filename),
                                                Style::default()
                                                    .fg(app.palette.accent_primary)
                                                    .add_modifier(Modifier::BOLD),
                                            ),
                                        ];
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
                                    if t.starts_with("index ")
                                        || t.starts_with("--- ")
                                        || t.starts_with("+++ ")
                                    {
                                        continue;
                                    }

                                    if t.starts_with("rename ") {
                                        out.push(Line::from(vec![Span::styled(
                                            pad_to_width(t.to_string(), content_w),
                                            Style::default().fg(app.palette.accent_secondary),
                                        )]));
                                        continue;
                                    }

                                    let (prefix, code) = t.split_at(
                                        t.chars().next().map(|c| c.len_utf8()).unwrap_or(0),
                                    );
                                    let (bg, is_code) = match prefix {
                                        "+" if !t.starts_with("+++") => {
                                            (app.palette.diff_add_bg, true)
                                        }
                                        "-" if !t.starts_with("---") => {
                                            (app.palette.diff_del_bg, true)
                                        }
                                        " " => (app.palette.bg, true),
                                        _ => (app.palette.bg, false),
                                    };

                                    let fill = content_w.saturating_sub(git::display_width(t));

                                    if is_code {
                                        if let Some(hl) = highlighter.as_mut() {
                                            let mut line = hl.highlight_diff_code_with_prefix(
                                                prefix,
                                                code,
                                                Style::default().fg(app.palette.fg),
                                                bg,
                                            );
                                            if fill > 0 {
                                                line.spans.push(Span::styled(
                                                    " ".repeat(fill),
                                                    Style::default().bg(bg),
                                                ));
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
                            GitDiffMode::SideBySide => {
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
                                    out
                                } else {

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

                                let cell_lines =
                                    |cell: &git::GitDiffCell, width: usize| -> Vec<String> {
                                        git::render_side_by_side_cell_lines(
                                            cell, width, scroll_x, wrap_cells,
                                        )
                                    };

                                let empty_left = " ".repeat(left_w);
                                let empty_right = " ".repeat(right_w);

                                let mut hl_old: Option<Highlighter> = None;
                                let mut hl_new: Option<Highlighter> = None;
                                if app.syntax_highlight {
                                    let ext = app
                                        .git
                                        .selected_tree_entry()
                                        .and_then(|e| {
                                            std::path::Path::new(e.path.as_str()).extension()
                                        })
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
                                                let (dir, filename) = match full_path.rfind('/') {
                                                    Some(i) => (&full_path[..i+1], &full_path[i+1..]),
                                                    None => ("", full_path),
                                                };
                                                let mut spans = vec![
                                                    Span::styled(
                                                        format!("📄 {}", filename),
                                                        Style::default()
                                                            .fg(app.palette.accent_primary)
                                                            .add_modifier(Modifier::BOLD),
                                                    ),
                                                ];
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
                                            if t.starts_with("index ")
                                                || t.starts_with("--- ")
                                                || t.starts_with("+++ ")
                                            {
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
                                                    GitDiffCellKind::Delete => {
                                                        app.palette.diff_del_bg
                                                    }
                                                    GitDiffCellKind::Context
                                                    | GitDiffCellKind::Add => app.palette.bg,
                                                    GitDiffCellKind::Empty => app.palette.bg,
                                                };
                                                let new_bg = match new.kind {
                                                    GitDiffCellKind::Add => app.palette.diff_add_bg,
                                                    GitDiffCellKind::Context
                                                    | GitDiffCellKind::Delete => app.palette.bg,
                                                    GitDiffCellKind::Empty => app.palette.bg,
                                                };

                                                let old_cell = pad_to_width(old_cell, left_w);
                                                let new_cell = pad_to_width(new_cell, right_w);

                                                let (old_gutter, old_code) =
                                                    old_cell.split_at(old_cell.len().min(6));
                                                let (new_gutter, new_code) =
                                                    new_cell.split_at(new_cell.len().min(6));

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
                                                        spans.extend(
                                                            hl.highlight_line(old_code, old_bg)
                                                                .spans,
                                                        );
                                                    } else {
                                                        spans.push(Span::styled(
                                                            old_code.to_string(),
                                                            old_style,
                                                        ));
                                                    }
                                                } else {
                                                    spans.push(Span::styled(
                                                        old_code.to_string(),
                                                        old_style,
                                                    ));
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
                                                        spans.extend(
                                                            hl.highlight_line(new_code, new_bg)
                                                                .spans,
                                                        );
                                                    } else {
                                                        spans.push(Span::styled(
                                                            new_code.to_string(),
                                                            new_style,
                                                        ));
                                                    }
                                                } else {
                                                    spans.push(Span::styled(
                                                        new_code.to_string(),
                                                        new_style,
                                                    ));
                                                }

                                                out.push(Line::from(spans));
                                            }
                                        }
                                    }
                                }

                                out
                                }
                            }
                        }
                    };
                    app.git_diff_cache.key = Some(cache_key);
                    app.git_diff_cache.lines = computed.clone();
                    computed
                };

                let wrap_unified = app.git.diff_mode == GitDiffMode::Unified && app.wrap_diff;

                let viewport_h = diff_area.height.saturating_sub(2) as usize;
                let max_y = if viewport_h == 0 {
                    0
                } else if wrap_unified {
                    app.git
                        .diff_lines
                        .iter()
                        .map(|l| {
                            let w = (diff_area.width.saturating_sub(2).max(1)) as usize;
                            let cols = git::display_width(l).max(1);
                            (cols + w - 1) / w
                        })
                        .sum::<usize>()
                        .saturating_sub(viewport_h)
                } else {
                    diff_lines.len().saturating_sub(viewport_h)
                };
                app.git.diff_scroll_y = app.git.diff_scroll_y.min(max_y as u16);

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

                // Render revert buttons for visible changes
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
                    let btn_x = diff_area.x + 1 + left_w as u16; // Middle gutter position (on the │ separator)

                    for (block_idx, block) in app.git.change_blocks.iter().enumerate() {
                        if block.display_row >= scroll_y && block.display_row < scroll_y + viewport_h {
                            let screen_y = diff_inner.y + (block.display_row - scroll_y) as u16;
                            let btn_rect = Rect::new(btn_x, screen_y, 1, 1);

                            // Draw the revert button (arrow in middle gutter)
                            let btn_style = Style::default()
                                .fg(app.palette.accent_secondary)
                                .add_modifier(Modifier::BOLD);
                            f.render_widget(
                                Paragraph::new("→").style(btn_style),
                                btn_rect,
                            );

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
                            f.render_widget(
                                Paragraph::new(" ↩ ").style(btn_style),
                                btn_rect,
                            );

                            zones.push(ClickZone {
                                rect: btn_rect,
                                action: AppAction::RevertHunk(hunk_idx),
                            });
                        }
                    }
                }
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

            if zoom != LogZoom::Diff {
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

                match app.log_ui.subtab {
                    LogSubTab::History => {
                        f.render_stateful_widget(list, list_area, &mut app.log_ui.history_state)
                    }
                    LogSubTab::Reflog => {
                        f.render_stateful_widget(list, list_area, &mut app.log_ui.reflog_state)
                    }
                    LogSubTab::Stash => {
                        f.render_stateful_widget(list, list_area, &mut app.log_ui.stash_state)
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
                    LogSubTab::Stash => app.log_ui.stash_state.offset(),
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
                    && app.log_ui.subtab == LogSubTab::History;

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
                    let inner = files_area.inner(Margin { vertical: 1, horizontal: 1 });

                    // Render commit info header, get remaining area for file list
                    let list_area = if let Some((subject, hash, author)) = commit_info {
                        let max_w = inner.width as usize;
                        let subj_display: String = if subject.chars().count() > max_w {
                            subject.chars().take(max_w.saturating_sub(1)).collect::<String>() + "…"
                        } else {
                            subject.to_string()
                        };
                        let subject_line = Line::from(vec![Span::styled(
                            subj_display,
                            Style::default().fg(app.palette.fg).add_modifier(Modifier::BOLD),
                        )]);
                        let meta_line = Line::from(vec![
                            Span::styled(hash.to_string(), Style::default().fg(app.palette.accent_primary)),
                            Span::styled(format!(" {}", author), Style::default().fg(app.palette.border_inactive)),
                        ]);
                        let sep_line = Line::from(vec![Span::styled(
                            "─".repeat(max_w),
                            Style::default().fg(app.palette.border_inactive),
                        )]);
                        let header = Paragraph::new(vec![subject_line, meta_line, sep_line]);
                        f.render_widget(header, Rect::new(inner.x, inner.y, inner.width, 3));
                        Rect::new(inner.x, inner.y + 3, inner.width, inner.height.saturating_sub(3))
                    } else {
                        inner
                    };

                    let file_items: Vec<ListItem> = app
                        .log_ui
                        .files
                        .iter()
                        .map(|f| {
                            // Show filename first, then line stats, then directory in gray
                            let (dir, filename) = match f.path.rfind('/') {
                                Some(i) => (&f.path[..i+1], &f.path[i+1..]),
                                None => ("", f.path.as_str()),
                            };
                            let status_color = match f.status.as_str() {
                                "M" => app.palette.accent_secondary,  // Modified
                                "A" => app.palette.diff_add_fg,       // Added
                                "D" => app.palette.diff_del_fg,       // Deleted
                                "R" => app.palette.accent_primary,    // Renamed
                                _ => app.palette.fg,
                            };
                            let mut spans = vec![
                                Span::styled(format!("{} ", f.status), Style::default().fg(status_color)),
                                Span::styled(filename.to_string(), Style::default().fg(app.palette.fg)),
                            ];
                            // Add line change stats
                            if let (Some(adds), Some(dels)) = (f.additions, f.deletions) {
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
                let cache_scroll_x =
                    if app.log_ui.diff_mode == GitDiffMode::Unified && !app.wrap_diff {
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
                    let diff_start = app.log_ui.diff_lines.iter()
                        .position(|l| l.starts_with("diff --git "))
                        .unwrap_or(app.log_ui.diff_lines.len());
                    let header_lines = &app.log_ui.diff_lines[..diff_start];
                    let diff_only_lines = &app.log_ui.diff_lines[diff_start..];

                    let computed: Vec<Line> = match app.log_ui.diff_mode {
                        GitDiffMode::Unified => {
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
                                        let ext = std::path::Path::new(p)
                                            .extension()
                                            .and_then(|s| s.to_str());
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
                                        Some(i) => (&full_path[..i+1], &full_path[i+1..]),
                                        None => ("", full_path),
                                    };
                                    let mut spans = vec![
                                        Span::styled(
                                            format!("📄 {}", filename),
                                            Style::default()
                                                .fg(app.palette.accent_primary)
                                                .add_modifier(Modifier::BOLD),
                                        ),
                                    ];
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
                                if t.starts_with("index ")
                                    || t.starts_with("--- ")
                                    || t.starts_with("+++ ")
                                {
                                    continue;
                                }

                                if t.starts_with("rename ") {
                                    out.push(Line::from(vec![Span::styled(
                                        pad_to_width(t.to_string(), content_w),
                                        Style::default().fg(app.palette.accent_secondary),
                                    )]));
                                    continue;
                                }

                                let (prefix, code) =
                                    t.split_at(t.chars().next().map(|c| c.len_utf8()).unwrap_or(0));
                                let (bg, prefix_fg, is_code) = match prefix {
                                    "+" if !t.starts_with("+++") => (app.palette.diff_add_bg, app.palette.diff_add_fg, true),
                                    "-" if !t.starts_with("---") => (app.palette.diff_del_bg, app.palette.diff_del_fg, true),
                                    " " => (app.palette.bg, app.palette.diff_gutter_fg, true),
                                    _ => (app.palette.bg, app.palette.fg, false),
                                };

                                let fill = content_w.saturating_sub(git::display_width(t));

                                if is_code {
                                    if let Some(hl) = highlighter.as_mut() {
                                        let mut line = hl.highlight_diff_code_with_prefix(
                                            prefix,
                                            code,
                                            Style::default().fg(prefix_fg),
                                            bg,
                                        );
                                        if fill > 0 {
                                            line.spans.push(Span::styled(
                                                " ".repeat(fill),
                                                Style::default().bg(bg),
                                            ));
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
                        GitDiffMode::SideBySide => {
                            // Only pass diff lines to side-by-side parser (not commit header)
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
                                out
                            } else {

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

                            let cell_lines =
                                |cell: &git::GitDiffCell, width: usize| -> Vec<String> {
                                    git::render_side_by_side_cell_lines(
                                        cell, width, scroll_x, wrap_cells,
                                    )
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
                                                let ext = std::path::Path::new(p)
                                                    .extension()
                                                    .and_then(|s| s.to_str());
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
                                                Some(i) => (&full_path[..i+1], &full_path[i+1..]),
                                                None => ("", full_path),
                                            };
                                            let mut spans = vec![
                                                Span::styled(
                                                    format!("📄 {}", filename),
                                                    Style::default()
                                                        .fg(app.palette.accent_primary)
                                                        .add_modifier(Modifier::BOLD),
                                                ),
                                            ];
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
                                        if t.starts_with("index ")
                                            || t.starts_with("--- ")
                                            || t.starts_with("+++ ")
                                        {
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
                                                GitDiffCellKind::Context | GitDiffCellKind::Add => {
                                                    app.palette.bg
                                                }
                                                GitDiffCellKind::Empty => app.palette.bg,
                                            };
                                            let new_bg = match new.kind {
                                                GitDiffCellKind::Add => app.palette.diff_add_bg,
                                                GitDiffCellKind::Context
                                                | GitDiffCellKind::Delete => app.palette.bg,
                                                GitDiffCellKind::Empty => app.palette.bg,
                                            };

                                            let old_cell = pad_to_width(old_cell, left_w);
                                            let new_cell = pad_to_width(new_cell, right_w);

                                            let (old_gutter, old_code) =
                                                old_cell.split_at(old_cell.len().min(6));
                                            let (new_gutter, new_code) =
                                                new_cell.split_at(new_cell.len().min(6));

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
                                                    spans.extend(
                                                        hl.highlight_line(old_code, old_bg).spans,
                                                    );
                                                } else {
                                                    spans.push(Span::styled(
                                                        old_code.to_string(),
                                                        old_style,
                                                    ));
                                                }
                                            } else {
                                                spans.push(Span::styled(
                                                    old_code.to_string(),
                                                    old_style,
                                                ));
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
                                                    spans.extend(
                                                        hl.highlight_line(new_code, new_bg).spans,
                                                    );
                                                } else {
                                                    spans.push(Span::styled(
                                                        new_code.to_string(),
                                                        new_style,
                                                    ));
                                                }
                                            } else {
                                                spans.push(Span::styled(
                                                    new_code.to_string(),
                                                    new_style,
                                                ));
                                            }

                                            out.push(Line::from(spans));
                                        }
                                    }
                                }
                            }

                            out
                            }
                        }
                    };

                    app.log_diff_cache.key = Some(cache_key);
                    app.log_diff_cache.lines = computed.clone();
                    computed
                };

                let wrap_unified = app.log_ui.diff_mode == GitDiffMode::Unified && app.wrap_diff;

                let viewport_h = diff_area.height.saturating_sub(2) as usize;
                let max_y = if viewport_h == 0 {
                    0
                } else if wrap_unified {
                    app.log_ui
                        .diff_lines
                        .iter()
                        .map(|l| {
                            let w = (diff_area.width.saturating_sub(2).max(1)) as usize;
                            let cols = git::display_width(l).max(1);
                            (cols + w - 1) / w
                        })
                        .sum::<usize>()
                        .saturating_sub(viewport_h)
                } else {
                    diff_lines.len().saturating_sub(viewport_h)
                };
                app.log_ui.diff_scroll_y = app.log_ui.diff_scroll_y.min(max_y as u16);

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
        Tab::Terminal => {
            // Poll terminal output
            app.terminal.poll_output();

            let term_block = Block::default()
                .borders(Borders::ALL)
                .border_set(ratatui::symbols::border::PLAIN)
                .border_style(Style::default().fg(app.palette.border_inactive))
                .title(" Terminal ");
            let inner = term_block.inner(content_area);
            f.render_widget(term_block, content_area);

            // Spawn shell if not active (use inner dimensions)
            if !app.terminal.active {
                app.terminal.spawn_shell(inner.width, inner.height, &app.current_path);
            }

            // Render terminal screen
            let screen = app.terminal.parser.screen();
            let rows = screen.size().0.min(inner.height);
            let cols = screen.size().1.min(inner.width);
            let mut lines: Vec<Line> = Vec::new();
            for row in 0..rows {
                let mut spans: Vec<Span> = Vec::new();
                for col in 0..cols {
                    let cell = screen.cell(row, col);
                    if let Some(cell) = cell {
                        let ch = cell.contents();
                        let fg = match cell.fgcolor() {
                            vt100::Color::Default => app.palette.fg,
                            vt100::Color::Idx(i) => idx_to_color(i),
                            vt100::Color::Rgb(r, g, b) => Color::Rgb(r, g, b),
                        };
                        let bg = match cell.bgcolor() {
                            vt100::Color::Default => app.palette.bg,
                            vt100::Color::Idx(i) => idx_to_color(i),
                            vt100::Color::Rgb(r, g, b) => Color::Rgb(r, g, b),
                        };
                        let mut style = Style::default().fg(fg).bg(bg);
                        if cell.bold() { style = style.add_modifier(Modifier::BOLD); }
                        spans.push(Span::styled(if ch.is_empty() { " ".to_string() } else { ch.to_string() }, style));
                    } else {
                        spans.push(Span::raw(" "));
                    }
                }
                lines.push(Line::from(spans));
            }
            f.render_widget(Paragraph::new(lines), inner);
        }
    }

    if let Some(commit_area) = commit_area {
        if app.commit.open {
            let commit_block = Block::default()
                .borders(Borders::ALL)
                .border_set(ratatui::symbols::border::PLAIN)
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
                .border_set(ratatui::symbols::border::PLAIN)
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
                .border_set(ratatui::symbols::border::PLAIN)
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
        .border_set(ratatui::symbols::border::PLAIN)
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
            buttons.push((
                " Stash (S) ".to_string(),
                AppAction::OpenStashPicker,
                app.palette.accent_secondary,
                app.pending_job.is_none() && !app.commit.busy,
            ));

            let enabled = app.pending_job.is_none() && !app.commit.busy && !app.branch_ui.open;
            let in_conflict_view = app.git.selected_tree_entry().is_some_and(|e| e.is_conflict);

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
        Tab::Terminal => {
            buttons.push((
                " Type to interact with shell ".to_string(),
                AppAction::None,
                app.palette.border_inactive,
                false,
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
    } else if app.current_tab == Tab::Git && app.git.selected_tree_entry().is_some_and(|e| e.is_conflict)
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
        let used = btn_x.saturating_sub(footer_area.x);
        let available = footer_area.width.saturating_sub(used).saturating_sub(2);
        if available > 0 {
            match app.current_tab {
                Tab::Explorer => {
                    let hint = "Ctrl+P menu  T theme  r refresh";
                    let w = hint.len().min(available as usize) as u16;
                    f.render_widget(
                        Paragraph::new(hint)
                            .style(Style::default().fg(app.palette.border_inactive)),
                        Rect::new(btn_x, btn_y, w, 1),
                    );
                }
                Tab::Git => {
                    let hint = "Ctrl+P menu  T theme  z stash";
                    let w = hint.len().min(available as usize) as u16;
                    f.render_widget(
                        Paragraph::new(hint)
                            .style(Style::default().fg(app.palette.border_inactive)),
                        Rect::new(btn_x, btn_y, w, 1),
                    );
                }
                Tab::Log => {
                    let prefix = "/ filter  ";
                    let author = "@author ▼";
                    let suffix = "  ref:tag  Ctrl+U clear";

                    let mut spans: Vec<Span> = Vec::new();
                    spans.push(Span::raw(prefix));
                    spans.push(Span::styled(
                        author,
                        Style::default().fg(app.palette.accent_tertiary),
                    ));
                    spans.push(Span::raw(suffix));

                    let line = Line::from(spans);
                    f.render_widget(
                        Paragraph::new(line)
                            .style(Style::default().fg(app.palette.border_inactive)),
                        Rect::new(btn_x, btn_y, available, 1),
                    );

                    let author_x = btn_x.saturating_add(prefix.len() as u16);
                    let author_w = author.len() as u16;
                    if author_x + author_w <= btn_x + available {
                        zones.push(ClickZone {
                            rect: Rect::new(author_x, btn_y, author_w, 1),
                            action: AppAction::OpenAuthorPicker,
                        });
                    }
                }
                Tab::Terminal => {
                    let hint = "Ctrl+P menu  T theme";
                    let w = hint.len().min(available as usize) as u16;
                    f.render_widget(
                        Paragraph::new(hint)
                            .style(Style::default().fg(app.palette.border_inactive)),
                        Rect::new(btn_x, btn_y, w, 1),
                    );
                }
            }
        }
    }

    if app.author_ui.open {
        let w = area.width.min(74).saturating_sub(2).max(46);
        let h = area.height.min(18).saturating_sub(2).max(10);
        let x = area.x + (area.width.saturating_sub(w)) / 2;
        let y = area.y + (area.height.saturating_sub(h)) / 2;
        let modal = Rect::new(x, y, w, h);

        zones.push(ClickZone {
            rect: area,
            action: AppAction::CloseAuthorPicker,
        });

        f.render_widget(Clear, modal);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_set(ratatui::symbols::border::PLAIN)
            .border_style(Style::default().fg(app.palette.btn_bg))
            .title(" Author ");
        f.render_widget(block.clone(), modal);

        let inner = modal.inner(Margin {
            vertical: 1,
            horizontal: 2,
        });

        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Min(0)])
            .split(inner);

        let query = Paragraph::new(format!("Filter: {}", app.author_ui.query))
            .style(Style::default().fg(app.palette.fg));
        f.render_widget(query, rows[0]);

        let items: Vec<ListItem> = app
            .author_ui
            .filtered
            .iter()
            .filter_map(|idx| app.author_ui.authors.get(*idx))
            .map(|a| ListItem::new(a.clone()))
            .collect();

        let list = List::new(items)
            .highlight_style(
                Style::default()
                    .bg(app.palette.selection_bg)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("▶ ");

        f.render_stateful_widget(list, rows[1], &mut app.author_ui.list_state);

        let list_inner = rows[1].inner(Margin {
            vertical: 0,
            horizontal: 0,
        });

        if list_inner.height > 0 {
            let offset = app.author_ui.list_state.offset();
            let end = (offset + list_inner.height as usize).min(app.author_ui.filtered.len());
            for (row_idx, _idx) in app.author_ui.filtered[offset..end].iter().enumerate() {
                let rect = Rect::new(
                    list_inner.x,
                    list_inner.y + row_idx as u16,
                    list_inner.width,
                    1,
                );
                zones.push(ClickZone {
                    rect,
                    action: AppAction::SelectAuthor(offset + row_idx),
                });
            }
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

        let title = match app.branch_picker_mode {
            BranchPickerMode::Checkout => " Checkout Branch ",
            BranchPickerMode::LogView => " View Branch ",
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_set(ratatui::symbols::border::PLAIN)
            .border_style(Style::default().fg(app.palette.accent_primary))
            .title(title);
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
            .items
            .iter()
            .map(|item| match item {
                BranchListItem::Header(t) => ListItem::new(Span::styled(
                    t.clone(),
                    Style::default()
                        .fg(app.palette.accent_tertiary)
                        .add_modifier(Modifier::BOLD),
                )),
                BranchListItem::Branch { idx, depth } => {
                    let b = &app.branch_ui.branches[*idx];
                    let cur = if b.is_current { "* " } else { "  " };
                    let kind = if b.is_remote { "[R] " } else { "[L] " };

                    let indent = "  ".repeat((*depth).min(6));
                    let mut s = format!("{}{}{}{}", cur, kind, indent, b.name);
                    if let Some(up) = &b.upstream {
                        s.push_str("  ");
                        s.push_str(up);
                    }
                    if let Some(tr) = &b.track {
                        s.push_str("  ");
                        s.push_str(tr);
                    }
                    ListItem::new(s)
                }
            })
            .collect();

        let list = List::new(list_items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_set(ratatui::symbols::border::PLAIN)
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
        let end = (start + list_inner.height as usize).min(app.branch_ui.items.len());
        for (i, idx) in (start..end).enumerate() {
            let rect = Rect::new(list_inner.x, list_inner.y + i as u16, list_inner.width, 1);
            let selectable = matches!(
                app.branch_ui.items.get(idx),
                Some(BranchListItem::Branch { .. })
            );
            if !selectable {
                continue;
            }
            let action = if app.branch_picker_mode == BranchPickerMode::LogView {
                AppAction::SelectLogBranch(idx)
            } else {
                AppAction::SelectBranch(idx)
            };
            zones.push(ClickZone { rect, action });
        }

        let buttons: Vec<(&str, AppAction, Color)> = match app.branch_picker_mode {
            BranchPickerMode::Checkout => vec![
                (
                    " Checkout ",
                    AppAction::BranchCheckout,
                    app.palette.accent_secondary,
                ),
                (" Close ", AppAction::CloseBranchPicker, app.palette.btn_bg),
            ],
            BranchPickerMode::LogView => vec![
                (
                    " View ",
                    AppAction::ConfirmLogBranchPicker,
                    app.palette.accent_secondary,
                ),
                (" Close ", AppAction::CloseBranchPicker, app.palette.btn_bg),
            ],
        };

        let mut x = rows[2].x;
        for (label, action, color) in buttons {
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

        if app.branch_picker_mode == BranchPickerMode::Checkout
            && let Some(pending) = app.branch_ui.confirm_checkout.as_deref()
        {
            let w = modal.width.min(70).saturating_sub(2).max(40);
            let h = 7u16.min(modal.height.saturating_sub(2)).max(7);
            let x = modal.x + (modal.width.saturating_sub(w)) / 2;
            let y = modal.y + (modal.height.saturating_sub(h)) / 2;
            let confirm = Rect::new(x, y, w, h);

            f.render_widget(Clear, confirm);

            let block = Block::default()
                .borders(Borders::ALL)
                .border_set(ratatui::symbols::border::PLAIN)
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

    if app.stash_ui.open {
        zones.push(ClickZone {
            rect: area,
            action: AppAction::CloseStashPicker,
        });

        let w = area.width.min(96).saturating_sub(2).max(60);
        let h = area.height.min(22).saturating_sub(2).max(12);
        let x = area.x + (area.width.saturating_sub(w)) / 2;
        let y = area.y + (area.height.saturating_sub(h)) / 2;
        let modal = Rect::new(x, y, w, h);

        f.render_widget(Clear, modal);

        let title = if app.stash_ui.query.trim().is_empty() {
            " Stash (S) ".to_string()
        } else {
            format!(" Stash (S)  filter: {} ", app.stash_ui.query.trim())
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_set(ratatui::symbols::border::PLAIN)
            .border_style(Style::default().fg(app.palette.accent_primary))
            .title(title);
        f.render_widget(block.clone(), modal);

        let inner = modal.inner(Margin {
            vertical: 1,
            horizontal: 2,
        });

        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Min(0),
                Constraint::Length(1),
            ])
            .split(inner);

        let filter_hint = "Type to filter  Backspace delete  Ctrl+U clear";
        let filter_style = if app.stash_ui.query.trim().is_empty() {
            Style::default().fg(app.palette.border_inactive)
        } else {
            Style::default().fg(app.palette.accent_primary)
        };
        f.render_widget(Paragraph::new(filter_hint).style(filter_style), rows[0]);

        let list_items: Vec<ListItem> = app
            .stash_ui
            .filtered
            .iter()
            .filter_map(|idx| app.stash_ui.stashes.get(*idx))
            .map(|s| ListItem::new(format!("{}  {}", s.selector, s.subject)))
            .collect();

        let list = List::new(list_items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_set(ratatui::symbols::border::PLAIN)
                    .border_style(Style::default().fg(app.palette.border_inactive))
                    .title(format!(" Stashes ({}) ", app.stash_ui.filtered.len())),
            )
            .highlight_style(
                Style::default()
                    .bg(app.palette.selection_bg)
                    .fg(app.palette.fg)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("▎ ");

        f.render_stateful_widget(list, rows[1], &mut app.stash_ui.list_state);

        let list_inner = rows[1].inner(Margin {
            vertical: 1,
            horizontal: 1,
        });
        let start = app.stash_ui.list_state.offset();
        let end = (start + list_inner.height as usize).min(app.stash_ui.filtered.len());
        for (i, idx) in (start..end).enumerate() {
            let rect = Rect::new(list_inner.x, list_inner.y + i as u16, list_inner.width, 1);
            zones.push(ClickZone {
                rect,
                action: AppAction::SelectStash(idx),
            });
        }

        let mut bx = rows[2].x;
        for (label, action, color) in [
            (
                " Apply (a) ",
                AppAction::StashApply,
                app.palette.accent_secondary,
            ),
            (" Pop (p) ", AppAction::StashPop, app.palette.accent_primary),
            (" Drop (d) ", AppAction::StashDrop, app.palette.btn_bg),
            (" Close ", AppAction::CloseStashPicker, app.palette.menu_bg),
        ] {
            let bw = label.len() as u16;
            let rect = Rect::new(bx, rows[2].y, bw, 1);
            let style = Style::default()
                .bg(color)
                .fg(app.palette.btn_fg)
                .add_modifier(Modifier::BOLD);
            f.render_widget(Paragraph::new(label).style(style), rect);
            zones.push(ClickZone { rect, action });
            bx += bw + 2;
        }

        if let Some(msg) = app.stash_ui.status.as_deref() {
            f.render_widget(
                Paragraph::new(msg).style(Style::default().fg(app.palette.btn_bg)),
                Rect::new(
                    rows[2].x + 48,
                    rows[2].y,
                    rows[2].width.saturating_sub(48),
                    1,
                ),
            );
        }

        if let Some((action, selector)) = app.stash_confirm.as_ref() {
            let w = modal.width.min(70).saturating_sub(2).max(44);
            let h = 7u16.min(modal.height.saturating_sub(2)).max(7);
            let x = modal.x + (modal.width.saturating_sub(w)) / 2;
            let y = modal.y + (modal.height.saturating_sub(h)) / 2;
            let confirm = Rect::new(x, y, w, h);

            f.render_widget(Clear, confirm);

            let block = Block::default()
                .borders(Borders::ALL)
                .border_set(ratatui::symbols::border::PLAIN)
                .border_style(Style::default().fg(app.palette.btn_bg))
                .title(" Confirm ");
            f.render_widget(block.clone(), confirm);

            let inner = confirm.inner(Margin {
                vertical: 1,
                horizontal: 2,
            });

            let verb = match action {
                StashConfirmAction::Pop => "pop",
                StashConfirmAction::Drop => "drop",
            };

            let text = vec![
                Line::raw(format!("About to {} {}", verb, selector)),
                Line::raw(""),
                Line::raw("Continue?"),
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
            let mut cx = inner.x;
            for (label, action, color) in [
                (
                    " Confirm ",
                    AppAction::ConfirmStashAction,
                    app.palette.accent_secondary,
                ),
                (" Cancel ", AppAction::CancelStashAction, app.palette.btn_bg),
            ] {
                let bw = label.len() as u16;
                let rect = Rect::new(cx, by, bw, 1);
                let style = Style::default()
                    .bg(color)
                    .fg(app.palette.btn_fg)
                    .add_modifier(Modifier::BOLD);
                f.render_widget(Paragraph::new(label).style(style), rect);
                zones.push(ClickZone { rect, action });
                cx += bw + 2;
            }
        }
    }

    if !app.stash_ui.open
        && app.stash_confirm.is_some()
        && app.discard_confirm.is_none()
        && !app.log_ui.inspect.open
        && app.operation_popup.is_none()
    {
        zones.push(ClickZone {
            rect: area,
            action: AppAction::CancelStashAction,
        });

        let w = area.width.min(70).saturating_sub(2).max(44);
        let h = 7u16.min(area.height.saturating_sub(2)).max(7);
        let x = area.x + (area.width.saturating_sub(w)) / 2;
        let y = area.y + (area.height.saturating_sub(h)) / 2;
        let confirm = Rect::new(x, y, w, h);

        if let Some((action, selector)) = app.stash_confirm.as_ref() {
            f.render_widget(Clear, confirm);

            let block = Block::default()
                .borders(Borders::ALL)
                .border_set(ratatui::symbols::border::PLAIN)
                .border_style(Style::default().fg(app.palette.btn_bg))
                .title(" Confirm ");
            f.render_widget(block.clone(), confirm);

            let inner = confirm.inner(Margin {
                vertical: 1,
                horizontal: 2,
            });

            let verb = match action {
                StashConfirmAction::Pop => "pop",
                StashConfirmAction::Drop => "drop",
            };

            let text = vec![
                Line::raw(format!("About to {} {}", verb, selector)),
                Line::raw(""),
                Line::raw("Continue?"),
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
            let mut cx = inner.x;
            for (label, action, color) in [
                (
                    " Confirm ",
                    AppAction::ConfirmStashAction,
                    app.palette.accent_secondary,
                ),
                (" Cancel ", AppAction::CancelStashAction, app.palette.btn_bg),
            ] {
                let bw = label.len() as u16;
                let rect = Rect::new(cx, by, bw, 1);
                let style = Style::default()
                    .bg(color)
                    .fg(app.palette.btn_fg)
                    .add_modifier(Modifier::BOLD);
                f.render_widget(Paragraph::new(label).style(style), rect);
                zones.push(ClickZone { rect, action });
                cx += bw + 2;
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
                .border_set(ratatui::symbols::border::PLAIN)
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
                .border_set(ratatui::symbols::border::PLAIN)
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
                        .border_set(ratatui::symbols::border::PLAIN)
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
                .border_set(ratatui::symbols::border::PLAIN)
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
                        .border_set(ratatui::symbols::border::PLAIN)
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
        let h = area.height.saturating_sub(4).min(28).max(12);
        let x = area.x + (area.width.saturating_sub(w)) / 2;
        let y = area.y + (area.height.saturating_sub(h)) / 2;
        let modal = Rect::new(x, y, w, h);

        f.render_widget(Clear, modal);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_set(ratatui::symbols::border::PLAIN)
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
                .border_set(ratatui::symbols::border::PLAIN)
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
            .border_set(ratatui::symbols::border::PLAIN)
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

    // Delete confirmation dialog (Explorer tab)
    if let Some(confirm) = &app.delete_confirm {
        let w = area.width.min(60).saturating_sub(2).max(40);
        let h = 7u16.min(area.height.saturating_sub(2)).max(5);
        let x = area.x + (area.width.saturating_sub(w)) / 2;
        let y = area.y + (area.height.saturating_sub(h)) / 2;
        let modal = Rect::new(x, y, w, h);

        zones.push(ClickZone {
            rect: area,
            action: AppAction::None, // Click outside does nothing
        });

        f.render_widget(Clear, modal);

        let title = if confirm.is_dir { " Delete Folder " } else { " Delete File " };
        let block = Block::default()
            .borders(Borders::ALL)
            .border_set(ratatui::symbols::border::PLAIN)
            .border_style(Style::default().fg(app.palette.diff_del_fg))
            .title(title);
        f.render_widget(block.clone(), modal);

        let inner = modal.inner(Margin { vertical: 1, horizontal: 2 });

        let name = confirm.path.file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| confirm.path.display().to_string());

        let mut lines = Vec::new();
        lines.push(Line::raw(format!("Delete: {}", name)));
        if confirm.is_dir {
            lines.push(Line::styled("(including all contents)", Style::default().fg(app.palette.border_inactive)));
        }
        lines.push(Line::raw(""));
        lines.push(Line::raw("Confirm? (y/n)"));

        f.render_widget(
            Paragraph::new(lines).style(Style::default().fg(app.palette.fg)),
            inner,
        );
    }

    // Update confirmation dialog
    if let Some(new_version) = &app.update_confirm {
        let w = area.width.min(55).saturating_sub(2).max(40);
        let h = 7u16.min(area.height.saturating_sub(2)).max(5);
        let x = area.x + (area.width.saturating_sub(w)) / 2;
        let y = area.y + (area.height.saturating_sub(h)) / 2;
        let modal = Rect::new(x, y, w, h);

        zones.push(ClickZone {
            rect: area,
            action: AppAction::None,
        });

        f.render_widget(Clear, modal);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_set(ratatui::symbols::border::PLAIN)
            .border_style(Style::default().fg(app.palette.accent_primary))
            .title(" Update Available ");
        f.render_widget(block.clone(), modal);

        let inner = modal.inner(Margin { vertical: 1, horizontal: 2 });

        let lines = vec![
            Line::raw(format!("New version: v{} -> v{}", VERSION, new_version)),
            Line::raw(""),
            Line::raw("Update now? (y/n)"),
        ];

        f.render_widget(
            Paragraph::new(lines).style(Style::default().fg(app.palette.fg)),
            inner,
        );
    }

    // Quick stash confirmation dialog
    if app.quick_stash_confirm {
        let w = area.width.min(45).saturating_sub(2).max(35);
        let h = 6u16.min(area.height.saturating_sub(2)).max(5);
        let x = area.x + (area.width.saturating_sub(w)) / 2;
        let y = area.y + (area.height.saturating_sub(h)) / 2;
        let modal = Rect::new(x, y, w, h);

        zones.push(ClickZone {
            rect: area,
            action: AppAction::None,
        });

        f.render_widget(Clear, modal);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_set(ratatui::symbols::border::PLAIN)
            .border_style(Style::default().fg(app.palette.accent_primary))
            .title(" Stash Changes ");
        f.render_widget(block.clone(), modal);

        let inner = modal.inner(Margin { vertical: 1, horizontal: 2 });

        let lines = vec![
            Line::raw("Stash all changes?"),
            Line::raw(""),
            Line::raw("(y/n)"),
        ];

        f.render_widget(
            Paragraph::new(lines).style(Style::default().fg(app.palette.fg)),
            inner,
        );
    }

    zones
}

fn idx_to_color(idx: u8) -> Color {
    match idx {
        0 => Color::Black,
        1 => Color::Red,
        2 => Color::Green,
        3 => Color::Yellow,
        4 => Color::Blue,
        5 => Color::Magenta,
        6 => Color::Cyan,
        7 => Color::White,
        8 => Color::DarkGray,
        9 => Color::LightRed,
        10 => Color::LightGreen,
        11 => Color::LightYellow,
        12 => Color::LightBlue,
        13 => Color::LightMagenta,
        14 => Color::LightCyan,
        15 => Color::Gray,
        n => Color::Indexed(n),
    }
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
        app.poll_git_refresh_job();
        app.poll_git_diff_job();
        app.poll_log_diff_job();
        app.maybe_expire_status();
        // Auto-refresh explorer when directory changes
        if app.current_tab == Tab::Explorer {
            app.check_auto_refresh();
        }
        // Force full terminal refresh if needed (e.g., after external editor)
        if app.needs_full_redraw {
            app.needs_full_redraw = false;
            terminal.clear()?;
        }
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
                            && !app.command_palette.open
                            && !app.stash_ui.open
                            && app.stash_confirm.is_none()
                            && !app.branch_ui.open =>
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
                            && !app.branch_ui.open =>
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
                            && !app.branch_ui.open =>
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
                        app.context_menu = None;
                        app.discard_confirm = None;
                        app.update_confirm = None;
                        app.quick_stash_confirm = false;
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
                        } else if app.update_confirm.is_some() {
                            match key.code {
                                KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
                                    app.confirm_update();
                                }
                                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                                    app.update_confirm = None;
                                }
                                _ => {}
                            }
                        } else if app.quick_stash_confirm {
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
                        } else if app.branch_ui.open {
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
                                        BranchPickerMode::Checkout => {
                                            app.branch_checkout_selected(false)
                                        }
                                        BranchPickerMode::LogView => {
                                            app.confirm_log_branch_picker();
                                        }
                                    },
                                    KeyCode::Char('j') | KeyCode::Down => {
                                        app.branch_ui.move_selection(1)
                                    }
                                    KeyCode::Char('k') | KeyCode::Up => {
                                        app.branch_ui.move_selection(-1)
                                    }
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
                        } else {
                            match app.current_tab {
                                Tab::Explorer => if app.delete_confirm.is_some() {
                                    match key.code {
                                        KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
                                            app.confirm_delete()
                                        }
                                        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                                            app.delete_confirm = None;
                                        }
                                        _ => {}
                                    }
                                } else { match key.code {
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
                                }},
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
                                    } else if app.stash_ui.open {
                                        if app.stash_confirm.is_some() {
                                            match key.code {
                                                KeyCode::Enter => app.confirm_stash_action(),
                                                KeyCode::Esc
                                                | KeyCode::Char('n')
                                                | KeyCode::Char('N') => {
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
                                                    if let Some(sel) = app.stash_ui.selected_stash()
                                                    {
                                                        app.open_stash_confirm(
                                                            StashConfirmAction::Pop,
                                                            sel.selector.clone(),
                                                        );
                                                    } else {
                                                        app.set_stash_status("No stash selected");
                                                    }
                                                }
                                                KeyCode::Char('d') => {
                                                    app.stash_ui.status = None;
                                                    if let Some(sel) = app.stash_ui.selected_stash()
                                                    {
                                                        app.open_stash_confirm(
                                                            StashConfirmAction::Drop,
                                                            sel.selector.clone(),
                                                        );
                                                    } else {
                                                        app.set_stash_status("No stash selected");
                                                    }
                                                }
                                                KeyCode::Char('j') | KeyCode::Down => {
                                                    app.stash_ui.move_selection(1)
                                                }
                                                KeyCode::Char('k') | KeyCode::Up => {
                                                    app.stash_ui.move_selection(-1)
                                                }
                                                KeyCode::Backspace => {
                                                    app.stash_ui.query.pop();
                                                    app.stash_ui.update_filtered();
                                                }
                                                KeyCode::Char(ch)
                                                    if !key
                                                        .modifiers
                                                        .contains(KeyModifiers::CONTROL)
                                                        && !key
                                                            .modifiers
                                                            .contains(KeyModifiers::ALT) =>
                                                {
                                                    app.stash_ui.query.push(ch);
                                                    app.stash_ui.update_filtered();
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
                                            KeyCode::Char('z') => {
                                                app.quick_stash_confirm = true;
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
                                                app.apply_conflict_resolution(
                                                    ConflictResolution::Ours,
                                                )
                                            }
                                            KeyCode::Char('t')
                                                if app
                                                    .git
                                                    .selected_tree_entry()
                                                    .is_some_and(|e| e.is_conflict) =>
                                            {
                                                app.apply_conflict_resolution(
                                                    ConflictResolution::Theirs,
                                                )
                                            }
                                            KeyCode::Char('b')
                                                if app
                                                    .git
                                                    .selected_tree_entry()
                                                    .is_some_and(|e| e.is_conflict) =>
                                            {
                                                app.apply_conflict_resolution(
                                                    ConflictResolution::Both,
                                                )
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
                                                // Collapse or scroll diff
                                                if let Some(item) = app.git.selected_tree_item() {
                                                    use git::FlatNodeType;
                                                    if item.node_type == FlatNodeType::Section || item.node_type == FlatNodeType::Directory {
                                                        app.git.collapse_tree_item();
                                                    } else {
                                                        app.git.diff_scroll_x =
                                                            app.git.diff_scroll_x.saturating_sub(4);
                                                    }
                                                } else {
                                                    app.git.diff_scroll_x =
                                                        app.git.diff_scroll_x.saturating_sub(4);
                                                }
                                            }
                                            KeyCode::Right => {
                                                // Expand or scroll diff
                                                if let Some(item) = app.git.selected_tree_item() {
                                                    use git::FlatNodeType;
                                                    if item.node_type == FlatNodeType::Section || item.node_type == FlatNodeType::Directory {
                                                        app.git.expand_tree_item();
                                                    } else {
                                                        app.git.diff_scroll_x =
                                                            app.git.diff_scroll_x.saturating_add(4);
                                                    }
                                                } else {
                                                    app.git.diff_scroll_x =
                                                        app.git.diff_scroll_x.saturating_add(4);
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
                                                // Toggle expand/collapse for sections/directories
                                                app.git.toggle_tree_expand();
                                            }
                                            _ => {}
                                        }
                                    }
                                }
                                Tab::Log => {
                                    if app.author_ui.open {
                                        if app.author_ui.filtered.is_empty() {
                                            match key.code {
                                                KeyCode::Esc => app.close_author_picker(),
                                                KeyCode::Backspace => {
                                                    app.author_ui.query.pop();
                                                    app.author_ui.update_filtered();
                                                }
                                                KeyCode::Char(ch)
                                                    if !key
                                                        .modifiers
                                                        .contains(KeyModifiers::CONTROL)
                                                        && !key
                                                            .modifiers
                                                            .contains(KeyModifiers::ALT) =>
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
                                                KeyCode::Down | KeyCode::Char('j') => {
                                                    app.author_ui.move_selection(1)
                                                }
                                                KeyCode::Up | KeyCode::Char('k') => {
                                                    app.author_ui.move_selection(-1)
                                                }
                                                KeyCode::PageDown => {
                                                    app.author_ui.move_selection(10)
                                                }
                                                KeyCode::PageUp => {
                                                    app.author_ui.move_selection(-10)
                                                }
                                                KeyCode::Backspace => {
                                                    app.author_ui.query.pop();
                                                    app.author_ui.update_filtered();
                                                }
                                                KeyCode::Char(ch)
                                                    if !key
                                                        .modifiers
                                                        .contains(KeyModifiers::CONTROL)
                                                        && !key
                                                            .modifiers
                                                            .contains(KeyModifiers::ALT) =>
                                                {
                                                    app.author_ui.query.push(ch);
                                                    app.author_ui.update_filtered();
                                                }
                                                _ => {}
                                            }
                                        }
                                    } else if app.stash_confirm.is_some() {
                                        match key.code {
                                            KeyCode::Enter => app.confirm_stash_action(),
                                            KeyCode::Esc
                                            | KeyCode::Char('n')
                                            | KeyCode::Char('N') => {
                                                app.stash_confirm = None;
                                            }
                                            _ => {}
                                        }
                                    } else if app.log_ui.inspect.open {
                                        match key.code {
                                            KeyCode::Esc | KeyCode::Enter => {
                                                app.log_ui.inspect.close()
                                            }
                                            KeyCode::Down => {
                                                app.move_log_selection(1);
                                                app.open_log_inspect();
                                            }
                                            KeyCode::Up => {
                                                app.move_log_selection(-1);
                                                app.open_log_inspect();
                                            }
                                            KeyCode::PageDown => {
                                                app.log_ui.inspect.scroll_y =
                                                    app.log_ui.inspect.scroll_y.saturating_add(10)
                                            }
                                            KeyCode::PageUp => {
                                                app.log_ui.inspect.scroll_y =
                                                    app.log_ui.inspect.scroll_y.saturating_sub(10)
                                            }
                                            KeyCode::Char('j') => {
                                                app.log_ui.inspect.scroll_y =
                                                    app.log_ui.inspect.scroll_y.saturating_add(3)
                                            }
                                            KeyCode::Char('k') => {
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
                                                app.log_ui.inspect.close();
                                            }
                                            KeyCode::Char('Y') => {
                                                if let Some(s) = app.selected_log_subject() {
                                                    app.request_copy_to_clipboard(s);
                                                } else {
                                                    app.request_copy_to_clipboard(
                                                        app.log_ui.inspect.body.clone(),
                                                    );
                                                }
                                                app.log_ui.inspect.close();
                                            }
                                            _ => {}
                                        }
                                    } else {
                                        match key.code {
                                            KeyCode::Char('/')
                                                if app.log_ui.subtab != LogSubTab::Commands =>
                                            {
                                                app.log_ui.filter_edit = !app.log_ui.filter_edit;
                                                app.log_ui.focus = LogPaneFocus::Commits;
                                            }
                                            KeyCode::Enter if app.log_ui.filter_edit => {
                                                app.log_ui.filter_edit = false;
                                            }
                                            KeyCode::Enter
                                                if app.log_ui.subtab == LogSubTab::Stash =>
                                            {
                                                app.stash_apply_log_selected();
                                            }
                                            KeyCode::Backspace if app.log_ui.filter_edit => {
                                                app.log_ui.filter_query.pop();
                                                app.log_ui.update_filtered();
                                                app.refresh_log_diff();
                                            }
                                            KeyCode::Char('u') | KeyCode::Char('l')
                                                if app.log_ui.subtab != LogSubTab::Commands
                                                    && key
                                                        .modifiers
                                                        .contains(KeyModifiers::CONTROL) =>
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
                                            KeyCode::Char('r') => {
                                                app.set_log_subtab(LogSubTab::Reflog)
                                            }
                                            KeyCode::Char('R') => {
                                                app.refresh_git_state();
                                            }
                                            KeyCode::Char('h') => {
                                                app.set_log_subtab(LogSubTab::History)
                                            }
                                            KeyCode::Char('t') => {
                                                app.set_log_subtab(LogSubTab::Stash)
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
                                            KeyCode::Char('a')
                                                if app.log_ui.subtab == LogSubTab::Stash =>
                                            {
                                                app.stash_apply_log_selected();
                                            }
                                            KeyCode::Char('p')
                                                if app.log_ui.subtab == LogSubTab::Stash =>
                                            {
                                                app.open_stash_confirm_log_selected(
                                                    StashConfirmAction::Pop,
                                                );
                                            }
                                            KeyCode::Char('d')
                                                if app.log_ui.subtab == LogSubTab::Stash =>
                                            {
                                                app.open_stash_confirm_log_selected(
                                                    StashConfirmAction::Drop,
                                                );
                                            }
                                            KeyCode::Char('d')
                                                if app.log_ui.subtab == LogSubTab::History =>
                                            {
                                                let next = match app.log_ui.detail_mode {
                                                    LogDetailMode::Diff => LogDetailMode::Files,
                                                    LogDetailMode::Files => LogDetailMode::Diff,
                                                };
                                                app.log_ui.set_detail_mode(next);
                                                app.refresh_log_diff();
                                            }
                                            KeyCode::Char('f')
                                                if app.log_ui.subtab == LogSubTab::History =>
                                            {
                                                app.log_ui.set_detail_mode(LogDetailMode::Files);
                                                app.refresh_log_diff();
                                            }
                                            KeyCode::Char('F')
                                                if app.log_ui.subtab == LogSubTab::History =>
                                            {
                                                // Toggle Files panel visibility
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
                                            KeyCode::Char('L')
                                                if app.log_ui.subtab != LogSubTab::Commands =>
                                            {
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
                                            KeyCode::Char('A')
                                                if app.log_ui.subtab != LogSubTab::Commands =>
                                            {
                                                app.open_author_picker();
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
                                Tab::Terminal => {
                                    // Forward key input to the terminal
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
                            }
                        }
                    }
                },
                Event::Mouse(mouse) => match mouse.kind {
                    MouseEventKind::Moved => {
                        app.update_context_menu_hover(mouse.row, mouse.column);
                    }
                    MouseEventKind::ScrollDown => {
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
                                            app.git.diff_scroll_x =
                                                app.git.diff_scroll_x.saturating_add(4);
                                        } else if app
                                            .git
                                            .selected_tree_entry()
                                            .is_some_and(|e| e.is_conflict)
                                        {
                                            app.conflict_ui.scroll_y =
                                                app.conflict_ui.scroll_y.saturating_add(3);
                                        } else {
                                            app.git.diff_scroll_y =
                                                app.git.diff_scroll_y.saturating_add(3);
                                        }
                                    } else {
                                        let i = app.git.list_state.selected().unwrap_or(0);
                                        let next =
                                            (i + 3).min(app.git.filtered.len().saturating_sub(1));
                                        if app.git.filtered.is_empty() {
                                            app.git.list_state.select(None);
                                        } else {
                                            app.git.select_filtered(next);
                                            app.request_git_diff_update();
                                        }
                                    }
                                }
                                Tab::Log => {
                                    let files_mode = app.log_ui.detail_mode == LogDetailMode::Files
                                        && app.log_ui.subtab == LogSubTab::History
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
                                Tab::Terminal => {
                                    // Terminal handles scrollback internally
                                }
                            }
                        }
                    }
                    MouseEventKind::ScrollUp => {
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
                                    if app.branch_ui.open {
                                        app.branch_ui.move_selection(-3);
                                    } else if mouse.column >= app.git_diff_x {
                                        if mouse.modifiers.contains(KeyModifiers::SHIFT) {
                                            app.git.diff_scroll_x =
                                                app.git.diff_scroll_x.saturating_sub(4);
                                        } else if app
                                            .git
                                            .selected_tree_entry()
                                            .is_some_and(|e| e.is_conflict)
                                        {
                                            app.conflict_ui.scroll_y =
                                                app.conflict_ui.scroll_y.saturating_sub(3);
                                        } else {
                                            app.git.diff_scroll_y =
                                                app.git.diff_scroll_y.saturating_sub(3);
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
                                Tab::Log => {
                                    let files_mode = app.log_ui.detail_mode == LogDetailMode::Files
                                        && app.log_ui.subtab == LogSubTab::History
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
                                Tab::Terminal => {
                                    // Terminal handles scrollback internally
                                }
                            }
                        }
                    }
                    MouseEventKind::Down(MouseButton::Left) => {
                        app.handle_click(mouse.row, mouse.column, mouse.modifiers);
                    }
                    MouseEventKind::Down(MouseButton::Right) => {
                        if app.theme_picker.open {
                            app.theme_picker.open = false;
                            continue;
                        }
                        if app.command_palette.open {
                            app.command_palette.open = false;
                            continue;
                        }
                        if app.stash_ui.open {
                            if app.stash_confirm.is_some() {
                                app.stash_confirm = None;
                            } else {
                                app.close_stash_picker();
                            }
                            continue;
                        }

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
