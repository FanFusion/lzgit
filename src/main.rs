use arboard::Clipboard;
use base64::{Engine as _, engine::general_purpose};
use crossterm::{
    event::{
        DisableMouseCapture, EnableMouseCapture, Event, EventStream, KeyCode, KeyEventKind,
        KeyModifiers, MouseButton, MouseEventKind,
    },
    execute,
    style::Print,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use futures::StreamExt;
use portable_pty::{CommandBuilder, NativePtySystem, PtySize, PtySystem};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
};
use ratatui_image::{picker::Picker, protocol::StatefulProtocol};
use serde::{Deserialize, Serialize};
use std::{
    cmp::Ordering,
    collections::{BTreeSet, VecDeque},
    env,
    fs::{self},
    io::{self, Read as _, Write},
    path::PathBuf,
    sync::{Arc, mpsc},
    thread,
    time::{Duration, Instant},
};
use tokio::sync::mpsc as tokio_mpsc;
use tokio_util::sync::CancellationToken;

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Compare two version strings (e.g., "0.4.1" vs "0.3.7")
/// Returns true if `new` is newer than `current`
fn is_newer_version(new: &str, current: &str) -> bool {
    let parse = |v: &str| -> Vec<u32> { v.split('.').filter_map(|s| s.parse().ok()).collect() };
    let new_parts = parse(new);
    let cur_parts = parse(current);

    for i in 0..new_parts.len().max(cur_parts.len()) {
        let n = new_parts.get(i).copied().unwrap_or(0);
        let c = cur_parts.get(i).copied().unwrap_or(0);
        if n > c {
            return true;
        }
        if n < c {
            return false;
        }
    }
    false
}

mod branch;
mod commit;
mod conflict;
mod git;
mod git_diff_loader;
mod git_ops;
mod highlight;
mod openrouter;
mod preview_cache;
mod preview_loader;
mod ui;

use branch::{BranchListItem, BranchUi};
use commit::{CommitFocus, CommitState};
use conflict::{ConflictFile, ConflictResolution};
use git::{GitDiffMode, GitSection, GitState, display_width};

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
        pub line_num_color: Color,
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
                    line_num_color: Color::Rgb(88, 91, 112), // Muted gray for line numbers
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
                    line_num_color: Color::Rgb(88, 91, 112), // Muted gray for line numbers
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
                    line_num_color: Color::Rgb(88, 91, 112), // Muted gray for line numbers
                    btn_bg,
                    btn_fg,
                    menu_bg,
                    diff_add_bg: tint(bg, diff_add_tint, diff_alpha),
                    diff_del_bg: tint(bg, diff_del_tint, diff_alpha),
                    diff_hunk_bg: tint(bg, accent_primary, hunk_alpha),
                    diff_add_fg: Color::Rgb(142, 192, 124), // Aqua for + sign
                    diff_del_fg: Color::Rgb(251, 73, 52),   // Red for - sign
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
                    line_num_color: Color::Rgb(88, 91, 112), // Muted gray for line numbers
                    btn_bg,
                    btn_fg,
                    menu_bg,
                    diff_add_bg: tint(bg, diff_add_tint, diff_alpha),
                    diff_del_bg: tint(bg, diff_del_tint, diff_alpha),
                    diff_hunk_bg: tint(bg, accent_primary, hunk_alpha),
                    diff_add_fg: Color::Rgb(136, 192, 208), // Frost cyan for + sign
                    diff_del_fg: Color::Rgb(191, 97, 106),  // Aurora red for - sign
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
                    line_num_color: Color::Rgb(88, 91, 112), // Muted gray for line numbers
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
                    line_num_color: Color::Rgb(88, 91, 112), // Muted gray for line numbers
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
pub(crate) enum Tab {
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
pub(crate) struct GitLogEntry {
    pub(crate) when: Instant,
    pub(crate) cmd: String,
    pub(crate) ok: bool,
    pub(crate) detail: Option<String>,
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
pub(crate) enum LogSubTab {
    History,
    Reflog,
    Stash,
    Commands,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum LogDetailMode {
    Diff,
    Files,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LogPaneFocus {
    Commits,
    Files,
    Diff,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum LogZoom {
    None,
    List,
    Diff,
}

/// Explorer view zoom modes
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub(crate) enum ExplorerZoom {
    #[default]
    ThreeColumn,  // Parent | Current | Preview
    TwoColumn,    // Current | Preview
    PreviewOnly,  // Full preview
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

pub(crate) struct LogUi {
    pub(crate) status: Option<String>,

    pub(crate) history_ref: Option<String>,

    pub(crate) subtab: LogSubTab,
    pub(crate) filter_query: String,
    pub(crate) filter_edit: bool,
    pub(crate) focus: LogPaneFocus,

    pub(crate) history: Vec<git_ops::CommitEntry>,
    pub(crate) reflog: Vec<git_ops::ReflogEntry>,
    pub(crate) stash: Vec<git_ops::StashEntry>,
    pub(crate) history_filtered: Vec<usize>,
    pub(crate) reflog_filtered: Vec<usize>,
    pub(crate) stash_filtered: Vec<usize>,

    pub(crate) detail_mode: LogDetailMode,
    pub(crate) diff_mode: GitDiffMode,
    pub(crate) zoom: LogZoom,

    pub(crate) diff_lines: Vec<String>,
    pub(crate) diff_scroll_y: u16,
    pub(crate) diff_scroll_x: u16,
    pub(crate) diff_generation: u64,
    pub(crate) diff_request_id: u64,

    pub(crate) files: Vec<git_ops::CommitFileChange>,
    pub(crate) files_hash: Option<String>,

    pub(crate) history_limit: usize,
    pub(crate) reflog_limit: usize,
    pub(crate) stash_limit: usize,

    pub(crate) history_state: ListState,
    pub(crate) reflog_state: ListState,
    pub(crate) stash_state: ListState,
    pub(crate) command_state: ListState,

    pub(crate) left_width: u16,
    inspect: InspectUi,

    pub(crate) files_state: ListState,
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
    NewBranch,
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
    (CommandId::NewBranch, "Git: new branch…"),
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
pub(crate) struct DiffRenderCacheKey {
    pub(crate) theme: theme::Theme,
    pub(crate) generation: u64,
    pub(crate) mode: GitDiffMode,
    pub(crate) width: u16,
    pub(crate) wrap: bool,
    pub(crate) syntax_highlight: bool,
    pub(crate) scroll_x: u16,
}

pub(crate) struct DiffRenderCache {
    pub(crate) key: Option<DiffRenderCacheKey>,
    pub(crate) lines: Vec<Line<'static>>,
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

pub(crate) struct App {
    pub(crate) current_path: PathBuf, // Explorer's current directory (changes with navigation)
    pub(crate) startup_path: PathBuf, // Initial directory (fixed, used for Git)
    pub(crate) files: Vec<FileEntry>,
    pub(crate) list_state: ListState,
    pub(crate) preview_scroll: u16,
    pub(crate) preview_scroll_offset: usize, // Independent scroll offset for preview panel
    pub(crate) should_quit: bool,
    pub(crate) show_hidden: bool,

    pub(crate) current_tab: Tab,

    pub(crate) git: GitState,
    pub(crate) git_operation: Option<GitOperation>,
    pub(crate) branch_ui: BranchUi,
    pub(crate) branch_picker_mode: BranchPickerMode,
    pub(crate) author_ui: AuthorUi,
    pub(crate) stash_ui: StashUi,
    pub(crate) stash_confirm: Option<(StashConfirmAction, String)>,
    pub(crate) conflict_ui: ConflictUi,
    pub(crate) commit: CommitState,
    pub(crate) pending_job: Option<PendingJob>,
    pub(crate) git_refresh_job: Option<PendingJob>,
    pub(crate) git_refresh_request_id: u64,
    pub(crate) git_diff_loader: git_diff_loader::GitDiffLoader,
    pub(crate) git_diff_cancel_token: Option<CancellationToken>,
    pub(crate) git_diff_result_rx: tokio_mpsc::Receiver<git_diff_loader::GitDiffResult>,
    pub(crate) log_diff_job: Option<PendingJob>,
    pub(crate) discard_confirm: Option<DiscardConfirm>,
    pub(crate) delete_confirm: Option<DeleteConfirm>,
    pub(crate) operation_popup: Option<OperationPopup>,
    pub(crate) theme_picker: ThemePickerUi,
    pub(crate) command_palette: CommandPaletteUi,
    pub(crate) git_log: VecDeque<GitLogEntry>,
    pub(crate) log_ui: LogUi,
    pub(crate) terminal: TerminalState,

    pub(crate) wrap_diff: bool,
    pub(crate) syntax_highlight: bool,
    pub(crate) git_zoom_diff: bool,
    pub(crate) explorer_zoom: ExplorerZoom,
    pub(crate) git_left_width: u16,

    pub(crate) theme: theme::Theme,
    pub(crate) palette: theme::Palette,

    pub(crate) git_diff_cache: DiffRenderCache,
    pub(crate) log_diff_cache: DiffRenderCache,

    pub(crate) explorer_parent_x: u16,
    pub(crate) explorer_current_x: u16,
    pub(crate) explorer_preview_x: u16,
    pub(crate) git_diff_x: u16,
    pub(crate) log_files_x: u16,
    pub(crate) log_diff_x: u16,

    pub(crate) zones: Vec<ClickZone>,
    pub(crate) last_click: Option<(Instant, usize)>,
    pub(crate) bookmarks: Vec<(String, PathBuf)>,

    // Auto-refresh
    pub(crate) last_dir_check: Instant,
    pub(crate) dir_mtime: Option<std::time::SystemTime>,
    pub(crate) auto_refresh: bool,

    // Update confirmation
    pub(crate) update_confirm: Option<String>, // Some(new_version) when update available
    pub(crate) update_in_progress: bool,
    pub(crate) spinner_frame: usize,

    // Quick stash confirmation
    pub(crate) quick_stash_confirm: bool,
    pub(crate) new_branch_input: Option<String>,

    pub(crate) context_menu: Option<ContextMenu>,
    pub(crate) pending_menu_action: Option<(usize, bool)>,

    pub(crate) picker: Picker,
    pub(crate) image_state: Option<StatefulProtocol>,
    pub(crate) current_image_path: Option<PathBuf>,
    pub(crate) preview_error: Option<String>,
    pub(crate) status_message: Option<(String, Instant)>,
    pub(crate) status_ttl: Duration,

    pub(crate) pending_clipboard: Option<String>,
    pub(crate) bookmarks_path: Option<PathBuf>,
    pub(crate) ui_settings_path: Option<PathBuf>,
    pub(crate) needs_full_redraw: bool,

    // Undo/Redo for file operations (revert)
    pub(crate) undo_stack: Vec<UndoEntry>,
    pub(crate) redo_stack: Vec<UndoEntry>,

    // Preview cache (kept for potential future use with async loader)
    #[allow(dead_code)]
    pub(crate) preview_cache: Arc<preview_cache::PreviewCache>,

    // Async preview loading
    pub(crate) preview_loader: preview_loader::PreviewLoader,
    pub(crate) preview_cancel_token: Option<CancellationToken>,
    pub(crate) preview_result_rx: tokio_mpsc::Receiver<preview_loader::PreviewResult>,
    pub(crate) preview_content: Option<String>,
    pub(crate) preview_loading: bool,

    // Preloading for adjacent files
    pub(crate) preload_cancel_tokens: Vec<CancellationToken>,
    pub(crate) preloaded_paths: BTreeSet<PathBuf>,

    // Syntax highlighting cache for visible lines only
    pub(crate) highlight_cache: Option<highlight::HighlightCache>,
}

/// Represents a file change that can be undone/redone
#[derive(Clone, Debug)]
struct UndoEntry {
    /// Description of the operation
    description: String,
    /// File path (absolute)
    file_path: PathBuf,
    /// Content before the operation
    old_content: String,
    /// Content after the operation
    new_content: String,
}

impl App {
    fn new(
        start_path: PathBuf,
        picker: Picker,
        preview_loader: preview_loader::PreviewLoader,
        preview_result_rx: tokio_mpsc::Receiver<preview_loader::PreviewResult>,
        git_diff_loader: git_diff_loader::GitDiffLoader,
        git_diff_result_rx: tokio_mpsc::Receiver<git_diff_loader::GitDiffResult>,
    ) -> Self {
        let mut app = Self {
            current_path: start_path.clone(),
            startup_path: start_path,
            files: Vec::new(),
            list_state: ListState::default(),
            preview_scroll: 0,
            preview_scroll_offset: 0,
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
            git_diff_loader,
            git_diff_cancel_token: None,
            git_diff_result_rx,
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
            explorer_zoom: ExplorerZoom::ThreeColumn,
            git_left_width: 40,

            theme: theme::Theme::Terminal,
            palette: theme::palette(theme::Theme::Terminal),

            git_diff_cache: DiffRenderCache::new(),
            log_diff_cache: DiffRenderCache::new(),

            explorer_parent_x: 0,
            explorer_current_x: 0,
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
            update_in_progress: false,
            spinner_frame: 0,
            quick_stash_confirm: false,
            new_branch_input: None,
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
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            preview_cache: Arc::new(preview_cache::PreviewCache::new(256)),

            preview_loader,
            preview_cancel_token: None,
            preview_result_rx,
            preview_content: None,
            preview_loading: false,

            preload_cancel_tokens: Vec::new(),
            preloaded_paths: BTreeSet::new(),

            highlight_cache: None,
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
        // Cancel any pending git diff request
        if let Some(token) = self.git_diff_cancel_token.take() {
            token.cancel();
        }

        self.git.diff_request_id = self.git.diff_request_id.wrapping_add(1);
        let request_id = self.git.diff_request_id;

        self.git.diff_scroll_y = 0;
        self.git.diff_scroll_x = 0;
        // Reset full file view when selection changes
        self.git.show_full_file = false;
        self.git.full_file_content = None;
        self.git.full_file_scroll_y = 0;

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

        // Use async git diff loader
        let cancel_token = self.git_diff_loader.request_diff(
            repo_root,
            path,
            is_untracked,
            staged,
            request_id,
        );
        self.git_diff_cancel_token = Some(cancel_token);
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

    fn toggle_full_file_view(&mut self) {
        self.git.show_full_file = !self.git.show_full_file;

        if self.git.show_full_file {
            // Load the full file content
            let Some(repo_root) = self.git.repo_root.clone() else {
                self.git.full_file_content = Some("Not a git repository".to_string());
                return;
            };

            let Some(entry) = self.git.selected_tree_entry().cloned() else {
                self.git.full_file_content = Some("No file selected".to_string());
                return;
            };

            let file_path = repo_root.join(&entry.path);
            match std::fs::read_to_string(&file_path) {
                Ok(content) => {
                    self.git.full_file_content = Some(content);
                    self.git.full_file_scroll_y = 0;
                    self.git_diff_cache.invalidate();
                }
                Err(e) => {
                    // Try to read as binary
                    if file_path.exists() {
                        self.git.full_file_content =
                            Some(format!("Binary file or read error: {}", e));
                    } else {
                        self.git.full_file_content =
                            Some(format!("File not found: {}", entry.path));
                    }
                }
            }
            self.set_status("Full file view (press F to return to diff)");
        } else {
            self.git.full_file_content = None;
            self.git_diff_cache.invalidate();
            self.set_status("Diff view");
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

    fn handle_git_diff_result(&mut self, result: git_diff_loader::GitDiffResult) {
        use git_diff_loader::GitDiffResult;

        match result {
            GitDiffResult::Ready { request_id, lines } => {
                // Ignore stale results
                if request_id != self.git.diff_request_id {
                    return;
                }
                self.git.set_diff_lines(lines);
                self.git.diff_generation = self.git.diff_generation.wrapping_add(1);
                self.git_diff_cache.invalidate();
            }
            GitDiffResult::Error { request_id, error } => {
                // Ignore stale results
                if request_id != self.git.diff_request_id {
                    return;
                }
                self.git.set_diff_lines(vec![error]);
                self.git.diff_generation = self.git.diff_generation.wrapping_add(1);
                self.git_diff_cache.invalidate();
            }
            GitDiffResult::Cancelled => {
                // Cancelled requests are ignored
            }
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

                if cmd.starts_with("update lzgit ") {
                    self.update_in_progress = false;
                    match &result {
                        Ok(()) => {
                            self.set_status("Update complete! Please restart lzgit.");
                        }
                        Err(e) => {
                            self.set_status(format!("Update failed: {}", e));
                        }
                    }
                }

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
                let name = confirm
                    .path
                    .file_name()
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

        // Save undo entry before writing
        self.undo_stack.push(UndoEntry {
            description: format!("Revert change in {}", block.file_path),
            file_path: file_path.clone(),
            old_content: content.clone(),
            new_content: new_content.clone(),
        });
        // Clear redo stack when new action is performed
        self.redo_stack.clear();
        // Limit undo stack size to 50 entries
        if self.undo_stack.len() > 50 {
            self.undo_stack.remove(0);
        }

        // Write the file
        if let Err(e) = std::fs::write(&file_path, &new_content) {
            self.set_status(format!("Failed to write file: {}", e));
            // Remove the undo entry since write failed
            self.undo_stack.pop();
            return;
        }

        self.set_status("Reverted (Ctrl+Z to undo)");
        self.refresh_git_state();
    }

    /// Undo the last revert operation
    fn undo_revert(&mut self) {
        let Some(entry) = self.undo_stack.pop() else {
            self.set_status("Nothing to undo");
            return;
        };

        // Write the old content back
        if let Err(e) = std::fs::write(&entry.file_path, &entry.old_content) {
            self.set_status(format!("Undo failed: {}", e));
            // Put the entry back since we couldn't undo
            self.undo_stack.push(entry);
            return;
        }

        // Move to redo stack
        self.redo_stack.push(entry);
        // Limit redo stack size
        if self.redo_stack.len() > 50 {
            self.redo_stack.remove(0);
        }

        self.set_status("Undone (Ctrl+Shift+Z to redo)");
        self.refresh_git_state();
    }

    /// Redo the last undone operation
    fn redo_revert(&mut self) {
        let Some(entry) = self.redo_stack.pop() else {
            self.set_status("Nothing to redo");
            return;
        };

        // Write the new content
        if let Err(e) = std::fs::write(&entry.file_path, &entry.new_content) {
            self.set_status(format!("Redo failed: {}", e));
            // Put the entry back since we couldn't redo
            self.redo_stack.push(entry);
            return;
        }

        // Move back to undo stack
        self.undo_stack.push(entry);

        self.set_status("Redone (Ctrl+Z to undo)");
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
            _ if cmd.starts_with("update lzgit ") => {
                let version = cmd.strip_prefix("update lzgit ").unwrap_or("").to_string();
                self.start_git_job(cmd.to_string(), false, false, move || {
                    // Download pre-built binary from GitHub Releases
                    let platform = if cfg!(target_os = "linux") && cfg!(target_arch = "x86_64") {
                        "linux-x86_64"
                    } else if cfg!(target_os = "linux") && cfg!(target_arch = "aarch64") {
                        "linux-aarch64"
                    } else if cfg!(target_os = "macos") && cfg!(target_arch = "x86_64") {
                        "macos-x86_64"
                    } else if cfg!(target_os = "macos") && cfg!(target_arch = "aarch64") {
                        "macos-aarch64"
                    } else {
                        return Err("Unsupported platform".to_string());
                    };

                    if version.is_empty() {
                        return Err("No version specified".to_string());
                    }

                    let url = format!(
                        "https://github.com/FanFusion/lzgit/releases/download/v{}/lzgit-{}",
                        version, platform
                    );

                    let resp = ureq::AgentBuilder::new()
                        .timeout(std::time::Duration::from_secs(120))
                        .build()
                        .get(&url)
                        .call()
                        .map_err(|e| format!("Download failed ({}): {}", url, e))?;

                    if resp.status() != 200 {
                        return Err(format!("HTTP {} from {}", resp.status(), url));
                    }

                    use std::io::Read;
                    let mut bytes = Vec::new();
                    resp.into_reader()
                        .read_to_end(&mut bytes)
                        .map_err(|e| format!("Read failed: {}", e))?;

                    let home =
                        std::env::var_os("HOME").ok_or_else(|| "HOME not set".to_string())?;

                    // Install to both ~/.cargo/bin and ~/.local/bin
                    let cargo_bin = std::path::PathBuf::from(&home).join(".cargo/bin/lzgit");
                    let local_bin = std::path::PathBuf::from(&home).join(".local/bin/lzgit");

                    for bin_path in [&cargo_bin, &local_bin] {
                        if let Some(parent) = bin_path.parent() {
                            let _ = std::fs::create_dir_all(parent);
                        }

                        // Write to temp file first, then rename (handles "text file busy")
                        let temp_path = bin_path.with_extension("new");
                        std::fs::write(&temp_path, &bytes)
                            .map_err(|e| format!("Write {:?}: {}", temp_path, e))?;

                        #[cfg(unix)]
                        {
                            use std::os::unix::fs::PermissionsExt;
                            let _ = std::fs::set_permissions(
                                &temp_path,
                                std::fs::Permissions::from_mode(0o755),
                            );
                        }

                        // Remove old file first (works even if running), then rename
                        let _ = std::fs::remove_file(bin_path);
                        std::fs::rename(&temp_path, bin_path)
                            .map_err(|e| format!("Rename {:?}: {}", bin_path, e))?;
                    }

                    Ok(())
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

    /// Get the file entries adjacent to the current selection (prev and next).
    /// Returns (prev_file, next_file), where either can be None if at boundaries.
    fn adjacent_files(&self) -> (Option<&FileEntry>, Option<&FileEntry>) {
        let Some(idx) = self.selected_index() else {
            return (None, None);
        };

        let prev = if idx > 0 {
            self.files.get(idx - 1)
        } else {
            None
        };

        let next = self.files.get(idx + 1);

        (prev, next)
    }

    /// Check if a file should be preloaded.
    /// Skip directories, images, and very large files.
    fn should_preload(&self, file: &FileEntry) -> bool {
        if file.is_dir {
            return false;
        }

        // Skip image files
        if let Some(ext) = file.path.extension().and_then(|s| s.to_str()) {
            let ext_lower = ext.to_lowercase();
            if matches!(
                ext_lower.as_str(),
                "png" | "jpg" | "jpeg" | "gif" | "bmp" | "webp"
            ) {
                return false;
            }
        }

        // Skip very large files (> 5MB)
        if let Ok(metadata) = fs::metadata(&file.path) {
            if metadata.len() > 5 * 1024 * 1024 {
                return false;
            }
        }

        true
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
            CommandId::NewBranch => {
                self.new_branch_input = Some(String::new());
            }
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

        // Fetch VERSION file from raw.githubusercontent.com (no API rate limit)
        let result: Result<String, String> = (|| {
            let resp = ureq::AgentBuilder::new()
                .timeout(std::time::Duration::from_secs(10))
                .build()
                .get("https://raw.githubusercontent.com/FanFusion/lzgit/main/VERSION")
                .call()
                .map_err(|e| format!("Network error: {}", e))?;

            let latest = resp
                .into_string()
                .map_err(|e| format!("Read error: {}", e))?
                .trim()
                .to_string();

            Ok(latest)
        })();

        match result {
            Ok(latest) => {
                if latest == VERSION {
                    self.set_status(&format!("You're up to date! (v{})", VERSION));
                } else if is_newer_version(&latest, VERSION) {
                    // Only show update if latest is actually newer
                    self.update_confirm = Some(latest);
                } else {
                    // Current version is newer (dev build or unreleased)
                    self.set_status(&format!("You're up to date! (v{} > v{})", VERSION, latest));
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
            self.update_in_progress = true;
            self.start_operation_job(&format!("update lzgit {}", new_version), false);
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
        self.preview_scroll_offset = 0; // Reset preview scroll when changing files

        // Cancel any pending preview load
        if let Some(token) = self.preview_cancel_token.take() {
            token.cancel();
        }
        self.preview_loader.cancel_current();

        let Some(file) = self.selected_file() else {
            self.image_state = None;
            self.current_image_path = None;
            self.preview_content = None;
            self.preview_loading = false;
            self.highlight_cache = None;
            return;
        };

        if file.is_dir {
            self.image_state = None;
            self.current_image_path = None;
            self.preview_content = None;
            self.preview_loading = false;
            self.highlight_cache = None;
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

        if is_image {
            // Handle image files synchronously (as before)
            self.preview_content = None;
            self.preview_loading = false;
            self.highlight_cache = None;

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
        } else {
            // Handle text files asynchronously
            self.image_state = None;
            self.current_image_path = None;

            // Check cache first for instant display
            if let Some(cached) = self.preview_cache.get(&path) {
                self.preview_loading = false;
                if cached.is_binary {
                    self.preview_content = None;
                    self.preview_error = Some("Binary file".to_string());
                    self.highlight_cache = None;
                } else {
                    let mut display_content = cached.text.clone();
                    if cached.truncated {
                        display_content.push_str("\n\n... (file truncated, too large to preview)");
                    }
                    self.preview_content = Some(display_content);
                    self.preview_error = None;
                    // Clear highlight cache when content changes
                    self.highlight_cache = None;
                }
                // Trigger preloading for adjacent files after using cache
                self.preload_adjacent_files();
            } else {
                // Not in cache, request async load
                self.preview_loading = true;
                self.preview_content = None;
                self.highlight_cache = None;

                // Request async preview load
                let cancel_token = self.preview_loader.request_preview_sync(path);
                self.preview_cancel_token = Some(cancel_token);
            }
        }
    }

    /// Preload previews for files adjacent to the current selection.
    /// This provides instant navigation when moving between files.
    fn preload_adjacent_files(&mut self) {
        // Cancel any existing preload operations
        for token in self.preload_cancel_tokens.drain(..) {
            token.cancel();
        }
        self.preloaded_paths.clear();

        let (prev, next) = self.adjacent_files();

        // Collect paths to preload first to avoid borrow issues
        let mut paths_to_preload = Vec::new();

        // Check previous file
        if let Some(file) = prev {
            if self.should_preload(file) && !self.preview_cache.get(&file.path).is_some() {
                paths_to_preload.push(file.path.clone());
            }
        }

        // Check next file
        if let Some(file) = next {
            if self.should_preload(file) && !self.preview_cache.get(&file.path).is_some() {
                paths_to_preload.push(file.path.clone());
            }
        }

        // Now preload the collected paths
        for path in paths_to_preload {
            let cancel_token = self.preview_loader.request_preview_sync(path.clone());
            self.preload_cancel_tokens.push(cancel_token);
            self.preloaded_paths.insert(path);
        }
    }

    /// Handle a preview result from the async loader.
    fn handle_preview_result(&mut self, result: preview_loader::PreviewResult) {
        use preview_loader::PreviewResult;

        self.preview_loading = false;

        match result {
            PreviewResult::Ready {
                path,
                content,
                truncated,
            } => {
                // Store in cache for future instant access
                let cache_content = preview_cache::PreviewContent {
                    text: content.clone(),
                    is_binary: false,
                    truncated,
                };
                self.preview_cache.insert(path.clone(), cache_content);

                let mut display_content = content;
                if truncated {
                    display_content.push_str("\n\n... (file truncated, too large to preview)");
                }
                self.preview_content = Some(display_content);
                self.preview_error = None;
                // Clear highlight cache when content changes
                self.highlight_cache = None;

                // Trigger preloading for adjacent files after successful load
                self.preload_adjacent_files();
            }
            PreviewResult::Partial {
                path: _,
                content,
                start_line: _,
                lines_loaded: _,
                has_more_before,
                has_more_after,
            } => {
                // Don't cache partial results as they're not complete
                let mut display_content = String::new();
                if has_more_before {
                    display_content.push_str("... (scroll up for more)\n\n");
                }
                display_content.push_str(&content);
                if has_more_after {
                    display_content.push_str("\n\n... (scroll down for more)");
                }
                self.preview_content = Some(display_content);
                self.preview_error = None;
                // Clear highlight cache when content changes
                self.highlight_cache = None;

                // Also trigger preloading for partial results
                self.preload_adjacent_files();
            }
            PreviewResult::Binary { path } => {
                // Store binary flag in cache
                let cache_content = preview_cache::PreviewContent {
                    text: String::new(),
                    is_binary: true,
                    truncated: false,
                };
                self.preview_cache.insert(path, cache_content);

                self.preview_content = None;
                self.preview_error = Some("Binary file".to_string());
                self.highlight_cache = None;
            }
            PreviewResult::Error { path: _, error } => {
                self.preview_content = None;
                self.preview_error = Some(error);
                self.highlight_cache = None;
            }
            PreviewResult::Cancelled => {
                // Ignore cancelled results, a new preview request should be pending
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
                                        let (a, b) = if anchor <= idx {
                                            (anchor, idx)
                                        } else {
                                            (idx, anchor)
                                        };
                                        self.git.selected_paths.clear();
                                        for i in a..=b {
                                            if let Some(item) = self.git.flat_tree.get(i) {
                                                if item.node_type == FlatNodeType::File {
                                                    if let Some(e_idx) = item.entry_idx {
                                                        if let Some(e) = self.git.entries.get(e_idx)
                                                        {
                                                            self.git
                                                                .selected_paths
                                                                .insert(e.path.clone());
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

    fn toggle_explorer_zoom(&mut self) {
        self.explorer_zoom = match self.explorer_zoom {
            ExplorerZoom::ThreeColumn => ExplorerZoom::TwoColumn,
            ExplorerZoom::TwoColumn => ExplorerZoom::PreviewOnly,
            ExplorerZoom::PreviewOnly => ExplorerZoom::ThreeColumn,
        };
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

pub(crate) fn format_size(size: u64) -> String {
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
            ui::tabs::render_explorer_tab(app, f, content_area, &mut zones);
        }
        Tab::Git => {
            ui::tabs::render_git_tab(app, f, content_area, &mut zones);
        }
        Tab::Log => {
            ui::tabs::render_log_tab(app, f, content_area, &mut zones);
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
                app.terminal
                    .spawn_shell(inner.width, inner.height, &app.current_path);
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
                        if cell.bold() {
                            style = style.add_modifier(Modifier::BOLD);
                        }
                        spans.push(Span::styled(
                            if ch.is_empty() {
                                " ".to_string()
                            } else {
                                ch.to_string()
                            },
                            style,
                        ));
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
    } else if app.current_tab == Tab::Git
        && app.git.selected_tree_entry().is_some_and(|e| e.is_conflict)
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
                    let hint = "Ctrl+P menu  T theme  z stash  N new branch";
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

        let title = if confirm.is_dir {
            " Delete Folder "
        } else {
            " Delete File "
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .border_set(ratatui::symbols::border::PLAIN)
            .border_style(Style::default().fg(app.palette.diff_del_fg))
            .title(title);
        f.render_widget(block.clone(), modal);

        let inner = modal.inner(Margin {
            vertical: 1,
            horizontal: 2,
        });

        let name = confirm
            .path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| confirm.path.display().to_string());

        let mut lines = Vec::new();
        lines.push(Line::raw(format!("Delete: {}", name)));
        if confirm.is_dir {
            lines.push(Line::styled(
                "(including all contents)",
                Style::default().fg(app.palette.border_inactive),
            ));
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

        let inner = modal.inner(Margin {
            vertical: 1,
            horizontal: 2,
        });

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

        let inner = modal.inner(Margin {
            vertical: 1,
            horizontal: 2,
        });

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

    if let Some(ref input) = app.new_branch_input {
        let w = area.width.min(50).saturating_sub(2).max(40);
        let h = 7u16.min(area.height.saturating_sub(2)).max(6);
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
            .title(" New Branch ");
        f.render_widget(block.clone(), modal);

        let inner = modal.inner(Margin {
            vertical: 1,
            horizontal: 2,
        });
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(1),
            ])
            .split(inner);

        f.render_widget(
            Paragraph::new("Enter branch name:").style(Style::default().fg(app.palette.fg)),
            rows[0],
        );

        let input_style = Style::default()
            .fg(app.palette.fg)
            .bg(app.palette.selection_bg);
        let display_input = format!("{}_", input);
        f.render_widget(Paragraph::new(display_input).style(input_style), rows[1]);

        f.render_widget(
            Paragraph::new("Enter to create · Esc to cancel")
                .style(Style::default().fg(app.palette.border_inactive)),
            rows[2],
        );
    }

    if app.update_in_progress {
        let spinner_chars = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
        let spinner = spinner_chars[app.spinner_frame % spinner_chars.len()];
        let text = format!(" {} Updating... ", spinner);
        let w = text.len() as u16 + 2;
        let x = area.x + area.width.saturating_sub(w + 1);
        let y = area.y + area.height.saturating_sub(2);
        let rect = Rect::new(x, y, w, 1);

        f.render_widget(
            Paragraph::new(text).style(
                Style::default()
                    .fg(app.palette.bg)
                    .bg(app.palette.accent_primary),
            ),
            rect,
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

#[tokio::main]
async fn main() -> io::Result<()> {
    let _ = dotenvy::dotenv();

    // Handle --version / -V
    if let Some(arg) = env::args().nth(1) {
        if arg == "--version" || arg == "-V" {
            println!("lzgit {}", VERSION);
            return Ok(());
        }
    }

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

    // Create async preview loader
    let (preview_loader, preview_result_rx) = preview_loader::PreviewLoader::new();

    // Create async git diff loader
    let (git_diff_loader, git_diff_result_rx) = git_diff_loader::GitDiffLoader::new();

    let mut app = App::new(
        start_path,
        picker,
        preview_loader,
        preview_result_rx,
        git_diff_loader,
        git_diff_result_rx,
    );

    // Create event stream for async terminal event handling
    let mut event_stream = EventStream::new();

    loop {
        let mut zones = Vec::new();
        app.tick_pending_menu_action();
        app.poll_pending_job();
        app.poll_git_refresh_job();
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

        if app.update_in_progress {
            app.spinner_frame = app.spinner_frame.wrapping_add(1);
        }

        // Use tokio::select! to handle terminal events and preview results concurrently
        let poll_timeout = tokio::time::sleep(Duration::from_millis(100));
        tokio::pin!(poll_timeout);

        tokio::select! {
            // Handle preview loader results
            Some(result) = app.preview_result_rx.recv() => {
                app.handle_preview_result(result);
            }
            // Handle git diff loader results
            Some(result) = app.git_diff_result_rx.recv() => {
                app.handle_git_diff_result(result);
            }
            // Handle terminal events
            Some(event_result) = event_stream.next() => {
                if let Ok(event) = event_result {
                    match event {
                Event::Key(key) if key.kind == KeyEventKind::Press => match key.code {
                    KeyCode::Char('q') => app.should_quit = true,
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
                        } else if app.new_branch_input.is_some() {
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
                                    // Preview scroll controls (must be before general Up/Down)
                                    KeyCode::Up if key.modifiers.contains(KeyModifiers::CONTROL) => {
                                        app.preview_scroll_offset = app.preview_scroll_offset.saturating_sub(1);
                                    }
                                    KeyCode::Down if key.modifiers.contains(KeyModifiers::CONTROL) => {
                                        app.preview_scroll_offset = app.preview_scroll_offset.saturating_add(1);
                                    }
                                    KeyCode::PageUp if key.modifiers.contains(KeyModifiers::CONTROL) => {
                                        app.preview_scroll_offset = app.preview_scroll_offset.saturating_sub(10);
                                    }
                                    KeyCode::PageDown if key.modifiers.contains(KeyModifiers::CONTROL) => {
                                        app.preview_scroll_offset = app.preview_scroll_offset.saturating_add(10);
                                    }
                                    // File list navigation
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
                                    KeyCode::Char('z') => {
                                        app.toggle_explorer_zoom();
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
                                            KeyCode::Char('z')
                                                if key
                                                    .modifiers
                                                    .contains(KeyModifiers::CONTROL)
                                                    && !key.modifiers.contains(KeyModifiers::SHIFT) =>
                                            {
                                                app.undo_revert();
                                            }
                                            KeyCode::Char('z') | KeyCode::Char('Z')
                                                if key
                                                    .modifiers
                                                    .contains(KeyModifiers::CONTROL)
                                                    && key.modifiers.contains(KeyModifiers::SHIFT) =>
                                            {
                                                app.redo_revert();
                                            }
                                            KeyCode::Char('y')
                                                if key
                                                    .modifiers
                                                    .contains(KeyModifiers::CONTROL) =>
                                            {
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
                                        // Preview pane - scroll preview
                                        app.preview_scroll_offset = app.preview_scroll_offset.saturating_add(3);
                                    } else if mouse.column >= app.explorer_current_x {
                                        // Current directory pane - scroll file list
                                        let i = app.selected_index().unwrap_or(0);
                                        if i + 3 < app.files.len() {
                                            app.list_state.select(Some(i + 3));
                                            app.update_preview();
                                        } else {
                                            app.list_state
                                                .select(Some(app.files.len().saturating_sub(1)));
                                            app.update_preview();
                                        }
                                    }
                                    // Parent pane (left) - no scroll action for now
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
                                        } else if app.git.show_full_file {
                                            app.git.full_file_scroll_y =
                                                app.git.full_file_scroll_y.saturating_add(3);
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
                                        // Preview pane - scroll preview
                                        app.preview_scroll_offset = app.preview_scroll_offset.saturating_sub(3);
                                    } else if mouse.column >= app.explorer_current_x {
                                        // Current directory pane - scroll file list
                                        let i = app.selected_index().unwrap_or(0);
                                        if i >= 3 {
                                            app.list_state.select(Some(i - 3));
                                            app.update_preview();
                                        } else {
                                            app.list_state.select(Some(0));
                                            app.update_preview();
                                        }
                                    }
                                    // Parent pane (left) - no scroll action for now
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
                                        } else if app.git.show_full_file {
                                            app.git.full_file_scroll_y =
                                                app.git.full_file_scroll_y.saturating_sub(3);
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
            }
            // Timeout - allows background polling to continue
            _ = &mut poll_timeout => {}
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
