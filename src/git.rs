use ratatui::widgets::ListState;
use std::{
    collections::BTreeSet,
    io,
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GitSection {
    All,
    Working,
    Staged,
    Untracked,
    Conflicts,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GitDiffMode {
    SideBySide,
    Unified,
}

#[derive(Clone, Debug)]
pub struct GitFileEntry {
    pub path: String,
    pub x: char,
    pub y: char,
    pub is_untracked: bool,
    pub is_conflict: bool,
    pub renamed_from: Option<String>,
}

#[derive(Clone, Debug)]
pub struct GitState {
    pub repo_root: Option<PathBuf>,
    pub branch: String,
    pub ahead: u32,
    pub behind: u32,

    pub section: GitSection,
    pub entries: Vec<GitFileEntry>,
    pub filtered: Vec<usize>,
    pub list_state: ListState,
    pub selected_paths: BTreeSet<String>,
    pub selection_anchor: Option<usize>,

    pub diff_mode: GitDiffMode,
    pub diff_lines: Vec<String>,
    pub diff_scroll_y: u16,
    pub diff_scroll_x: u16,
}

impl GitState {
    pub fn new() -> Self {
        Self {
            repo_root: None,
            branch: String::new(),
            ahead: 0,
            behind: 0,
            section: GitSection::All,
            entries: Vec::new(),
            filtered: Vec::new(),
            list_state: ListState::default(),
            selected_paths: BTreeSet::new(),
            selection_anchor: None,
            diff_mode: GitDiffMode::SideBySide,
            diff_lines: Vec::new(),
            diff_scroll_y: 0,
            diff_scroll_x: 0,
        }
    }

    pub fn refresh(&mut self, current_path: &Path) {
        self.repo_root = None;
        self.branch.clear();
        self.ahead = 0;
        self.behind = 0;
        self.entries.clear();
        self.filtered.clear();
        self.list_state.select(None);
        self.selected_paths.clear();
        self.selection_anchor = None;
        self.diff_lines.clear();
        self.diff_scroll_y = 0;
        self.diff_scroll_x = 0;

        let cwd = if current_path.exists() {
            current_path
        } else {
            Path::new("/")
        };

        let root = Command::new("git")
            .arg("-C")
            .arg(cwd)
            .args(["rev-parse", "--show-toplevel"])
            .output()
            .ok()
            .and_then(|o| {
                if o.status.success() {
                    Some(o.stdout)
                } else {
                    None
                }
            })
            .and_then(|b| String::from_utf8(b).ok())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .map(PathBuf::from);

        let Some(root) = root else {
            self.list_state.select(None);
            return;
        };
        self.repo_root = Some(root);

        let out = run_git(cwd, &["status", "--porcelain=v1", "-z", "-b"]);
        let Ok(out) = out else {
            self.list_state.select(None);
            return;
        };
        if !out.status.success() {
            self.list_state.select(None);
            return;
        }

        let items: Vec<&[u8]> = out
            .stdout
            .split(|b| *b == 0)
            .filter(|s| !s.is_empty())
            .collect();
        let mut i = 0;
        while i < items.len() {
            let s = String::from_utf8_lossy(items[i]).to_string();
            if let Some(branch_line) = s.strip_prefix("## ") {
                self.parse_status_v1_branch_line(&format!("## {}", branch_line));
                i += 1;
                continue;
            }

            if s.len() >= 3 {
                let x = s.chars().nth(0).unwrap_or(' ');
                let y = s.chars().nth(1).unwrap_or(' ');

                if &s[0..2] == "??" {
                    let path = s[3..].to_string();
                    self.entries.push(GitFileEntry {
                        path,
                        x: '?',
                        y: '?',
                        is_untracked: true,
                        is_conflict: false,
                        renamed_from: None,
                    });
                    i += 1;
                    continue;
                }

                let status = &s[0..1];
                if status == "R" || status == "C" {
                    let from_path = s[3..].to_string();
                    let to_path = if i + 1 < items.len() {
                        String::from_utf8_lossy(items[i + 1]).to_string()
                    } else {
                        String::new()
                    };
                    let is_conflict = is_conflict_status(x, y);
                    self.entries.push(GitFileEntry {
                        path: if to_path.is_empty() {
                            from_path.clone()
                        } else {
                            to_path
                        },
                        x,
                        y,
                        is_untracked: false,
                        is_conflict,
                        renamed_from: Some(from_path),
                    });
                    i += 2;
                    continue;
                }

                let path = s[3..].to_string();
                let is_conflict = is_conflict_status(x, y);
                self.entries.push(GitFileEntry {
                    path,
                    x,
                    y,
                    is_untracked: false,
                    is_conflict,
                    renamed_from: None,
                });
            }
            i += 1;
        }

        self.update_filtered();
        self.update_diff_lines(cwd);
    }

    pub fn set_section(&mut self, section: GitSection, current_path: &Path) {
        self.section = section;
        self.update_filtered();
        self.update_diff_lines(current_path);
    }

    pub fn select_filtered(&mut self, idx: usize, current_path: &Path) {
        self.list_state.select(Some(idx));
        self.diff_scroll_y = 0;
        self.diff_scroll_x = 0;
        self.update_diff_lines(current_path);
    }

    pub fn selected_entry(&self) -> Option<&GitFileEntry> {
        let sel = self.list_state.selected()?;
        let abs = *self.filtered.get(sel)?;
        self.entries.get(abs)
    }

    fn update_filtered(&mut self) {
        self.filtered.clear();

        for (idx, e) in self.entries.iter().enumerate() {
            let staged = e.x != ' ' && e.x != '?';
            let unstaged = e.y != ' ' && e.y != '?';
            let keep = match self.section {
                GitSection::All => true,
                GitSection::Working => unstaged && !e.is_conflict && !e.is_untracked,
                GitSection::Staged => staged && !e.is_conflict && !e.is_untracked,
                GitSection::Untracked => e.is_untracked,
                GitSection::Conflicts => e.is_conflict,
            };
            if keep {
                self.filtered.push(idx);
            }
        }

        if self.section == GitSection::All {
            self.filtered.sort_by(|a, b| {
                let ea = &self.entries[*a];
                let eb = &self.entries[*b];
                let pa = entry_sort_key(ea);
                let pb = entry_sort_key(eb);
                pa.cmp(&pb)
            });
        }

        let selected = self.list_state.selected().unwrap_or(0);
        if self.filtered.is_empty() {
            self.list_state.select(None);
        } else if selected >= self.filtered.len() {
            self.list_state.select(Some(0));
        }
    }

    fn update_diff_lines(&mut self, current_path: &Path) {
        self.diff_lines.clear();
        let Some(entry) = self.selected_entry() else {
            return;
        };

        if entry.is_untracked {
            self.diff_lines.push("Untracked file".to_string());
            return;
        }

        let staged = entry.x != ' ' && entry.x != '?';
        let args: Vec<&str> = if staged {
            vec!["diff", "--cached", "--", entry.path.as_str()]
        } else {
            vec!["diff", "--", entry.path.as_str()]
        };

        let cwd = if current_path.exists() {
            current_path
        } else {
            Path::new("/")
        };

        let out = run_git(cwd, &args);
        let Ok(out) = out else {
            self.diff_lines.push("Failed to run git diff".to_string());
            return;
        };
        if !out.status.success() {
            self.diff_lines.push("git diff failed".to_string());
            return;
        }

        let text = String::from_utf8_lossy(&out.stdout);
        if text.trim().is_empty() {
            self.diff_lines.push("No diff".to_string());
        } else {
            self.diff_lines.extend(text.lines().map(|l| l.to_string()));
        }
    }

    fn parse_status_v1_branch_line(&mut self, line: &str) {
        let rest = line.trim_start_matches("## ").trim();
        if rest.is_empty() {
            self.branch.clear();
            self.ahead = 0;
            self.behind = 0;
            return;
        }

        let (head, ab_part) = if let Some((left, right)) = rest.rsplit_once('[') {
            (left.trim(), Some(right.trim_end_matches(']').trim()))
        } else {
            (rest, None)
        };

        let branch = head.split("...").next().unwrap_or(head).trim().to_string();
        self.branch = branch;
        self.ahead = 0;
        self.behind = 0;

        let Some(ab_part) = ab_part else {
            return;
        };
        for item in ab_part.split(',').map(|s| s.trim()) {
            if let Some(v) = item.strip_prefix("ahead ") {
                self.ahead = v.parse::<u32>().unwrap_or(0);
            } else if let Some(v) = item.strip_prefix("behind ") {
                self.behind = v.parse::<u32>().unwrap_or(0);
            }
        }
    }
}

fn run_git(cwd: &Path, args: &[&str]) -> io::Result<std::process::Output> {
    Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GCM_INTERACTIVE", "never")
        .env("GIT_PAGER", "cat")
        .env("PAGER", "cat")
        .env("GIT_EDITOR", ":")
        .env("EDITOR", ":")
        .env("GIT_SEQUENCE_EDITOR", ":")
        .env("GIT_MERGE_AUTOEDIT", "no")
        .output()
}

fn is_conflict_status(x: char, y: char) -> bool {
    matches!(
        (x, y),
        ('U', 'U') | ('A', 'A') | ('D', 'D') | ('A', 'U') | ('U', 'A') | ('D', 'U') | ('U', 'D')
    )
}

fn entry_sort_key(e: &GitFileEntry) -> (u8, String) {
    if e.is_conflict {
        return (0, e.path.clone());
    }
    if e.is_untracked {
        return (3, e.path.clone());
    }

    let staged = e.x != ' ' && e.x != '?';
    let unstaged = e.y != ' ' && e.y != '?';

    if staged {
        (1, e.path.clone())
    } else if unstaged {
        (2, e.path.clone())
    } else {
        (4, e.path.clone())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GitDiffCellKind {
    Context,
    Delete,
    Add,
    Empty,
}

#[derive(Clone, Debug)]
pub struct GitDiffCell {
    pub line_no: Option<u32>,
    pub text: String,
    pub kind: GitDiffCellKind,
}

#[derive(Clone, Debug)]
pub enum GitDiffRow {
    Meta(String),
    Split { old: GitDiffCell, new: GitDiffCell },
}

pub fn slice_chars(s: &str, start: usize, max_len: usize) -> String {
    if max_len == 0 {
        return String::new();
    }
    s.chars().skip(start).take(max_len).collect()
}

pub fn truncate_to_width(s: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    s.chars().take(width).collect()
}

pub fn pad_to_width(mut s: String, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    let len = s.chars().count();
    if len >= width {
        return truncate_to_width(&s, width);
    }
    s.push_str(&" ".repeat(width - len));
    s
}

pub fn render_side_by_side_cell(cell: &GitDiffCell, width: usize, scroll_x: usize) -> String {
    const GUTTER: usize = 6;
    if width == 0 {
        return String::new();
    }

    let marker = match cell.kind {
        GitDiffCellKind::Add => '+',
        GitDiffCellKind::Delete => '-',
        _ => ' ',
    };

    let gutter = if let Some(n) = cell.line_no {
        format!("{:>4}{} ", n, marker)
    } else {
        "      ".to_string()
    };

    if width <= GUTTER {
        return truncate_to_width(&gutter, width);
    }

    let code_w = width - GUTTER;
    let code = slice_chars(&cell.text, scroll_x, code_w);
    format!("{}{}", gutter, pad_to_width(code, code_w))
}

fn parse_hunk_header(line: &str) -> Option<(u32, u32)> {
    let trimmed = line.trim();
    let Some(rest) = trimmed.strip_prefix("@@") else {
        return None;
    };
    let rest = rest.trim_start();
    let Some((range, _)) = rest.split_once("@@") else {
        return None;
    };
    let mut it = range.trim().split_whitespace();
    let old_tok = it.next()?;
    let new_tok = it.next()?;

    let old_start = old_tok.strip_prefix('-')?.split(',').next()?.parse().ok()?;
    let new_start = new_tok.strip_prefix('+')?.split(',').next()?.parse().ok()?;

    Some((old_start, new_start))
}

pub fn build_side_by_side_rows(lines: &[String]) -> Vec<GitDiffRow> {
    let mut rows = Vec::new();

    let mut old_line: Option<u32> = None;
    let mut new_line: Option<u32> = None;

    let mut pending_del: Vec<(u32, String)> = Vec::new();
    let mut pending_add: Vec<(u32, String)> = Vec::new();

    let flush = |rows: &mut Vec<GitDiffRow>,
                 pending_del: &mut Vec<(u32, String)>,
                 pending_add: &mut Vec<(u32, String)>| {
        let n = pending_del.len().max(pending_add.len());
        for i in 0..n {
            let old = if let Some((ln, t)) = pending_del.get(i) {
                GitDiffCell {
                    line_no: Some(*ln),
                    text: t.clone(),
                    kind: GitDiffCellKind::Delete,
                }
            } else {
                GitDiffCell {
                    line_no: None,
                    text: String::new(),
                    kind: GitDiffCellKind::Empty,
                }
            };
            let new = if let Some((ln, t)) = pending_add.get(i) {
                GitDiffCell {
                    line_no: Some(*ln),
                    text: t.clone(),
                    kind: GitDiffCellKind::Add,
                }
            } else {
                GitDiffCell {
                    line_no: None,
                    text: String::new(),
                    kind: GitDiffCellKind::Empty,
                }
            };
            rows.push(GitDiffRow::Split { old, new });
        }
        pending_del.clear();
        pending_add.clear();
    };

    for line in lines {
        if line.starts_with("diff --git ")
            || line.starts_with("index ")
            || line.starts_with("--- ")
            || line.starts_with("+++ ")
            || line.starts_with("rename ")
            || line.starts_with("new file ")
            || line.starts_with("deleted file ")
            || line.starts_with("similarity index ")
            || line.starts_with("Binary files ")
            || line.starts_with("\\ No newline")
        {
            flush(&mut rows, &mut pending_del, &mut pending_add);
            rows.push(GitDiffRow::Meta(line.clone()));
            continue;
        }

        if line.starts_with("@@") {
            flush(&mut rows, &mut pending_del, &mut pending_add);
            rows.push(GitDiffRow::Meta(line.clone()));
            if let Some((o, n)) = parse_hunk_header(line) {
                old_line = Some(o);
                new_line = Some(n);
            }
            continue;
        }

        let Some(first) = line.chars().next() else {
            continue;
        };

        match first {
            ' ' => {
                flush(&mut rows, &mut pending_del, &mut pending_add);
                let o = old_line;
                let n = new_line;
                let text = line.get(1..).unwrap_or("").to_string();
                rows.push(GitDiffRow::Split {
                    old: GitDiffCell {
                        line_no: o,
                        text: text.clone(),
                        kind: GitDiffCellKind::Context,
                    },
                    new: GitDiffCell {
                        line_no: n,
                        text,
                        kind: GitDiffCellKind::Context,
                    },
                });
                if let Some(v) = old_line.as_mut() {
                    *v += 1;
                }
                if let Some(v) = new_line.as_mut() {
                    *v += 1;
                }
            }
            '-' => {
                if let Some(v) = old_line.as_mut() {
                    let ln = *v;
                    *v += 1;
                    pending_del.push((ln, line.get(1..).unwrap_or("").to_string()));
                } else {
                    pending_del.push((0, line.get(1..).unwrap_or("").to_string()));
                }
            }
            '+' => {
                if let Some(v) = new_line.as_mut() {
                    let ln = *v;
                    *v += 1;
                    pending_add.push((ln, line.get(1..).unwrap_or("").to_string()));
                } else {
                    pending_add.push((0, line.get(1..).unwrap_or("").to_string()));
                }
            }
            _ => {
                flush(&mut rows, &mut pending_del, &mut pending_add);
                rows.push(GitDiffRow::Meta(line.clone()));
            }
        }
    }

    flush(&mut rows, &mut pending_del, &mut pending_add);
    rows
}
