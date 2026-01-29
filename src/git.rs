use ratatui::widgets::ListState;
use std::{
    collections::{BTreeMap, BTreeSet},
    io,
    path::{Path, PathBuf},
    process::Command,
};
use unicode_width::UnicodeWidthChar;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum GitSection {
    Staged,
    Working,
    Untracked,
    Conflicts,
}

/// Type of node in the flattened tree view
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlatNodeType {
    Section,
    Directory,
    File,
}

/// Represents a single visible item in the tree view
#[derive(Clone, Debug)]
pub struct FlatTreeItem {
    pub depth: usize,
    pub node_type: FlatNodeType,
    pub expanded: bool,
    pub entry_idx: Option<usize>,
    pub name: String,
    pub path: String,
    pub section: GitSection,
}

/// Internal tree node for building the hierarchy
#[derive(Clone, Debug)]
enum TreeNode {
    Section {
        kind: GitSection,
        expanded: bool,
        children: Vec<TreeNode>,
    },
    Directory {
        name: String,
        path: String,
        expanded: bool,
        children: Vec<TreeNode>,
    },
    File {
        name: String,
        entry_idx: usize,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GitDiffMode {
    SideBySide,
    Unified,
}

/// A hunk in a diff, used for partial staging/reverting
#[derive(Clone, Debug)]
pub struct DiffHunk {
    /// Display row index where this hunk starts (unified mode)
    pub display_row: usize,
    /// Display row index in side-by-side mode
    pub sbs_display_row: usize,
    /// The raw diff lines for this hunk (header + content)
    pub lines: Vec<String>,
}

/// A change block - consecutive deleted/added lines that can be reverted together
#[derive(Clone, Debug)]
pub struct ChangeBlock {
    /// Display row in side-by-side view (first row of the block)
    pub display_row: usize,
    /// File path this block belongs to
    pub file_path: String,
    /// Starting line number in the NEW file (1-indexed)
    pub new_start: u32,
    /// Lines in the NEW file (additions to remove)
    pub new_lines: Vec<String>,
    /// Lines from the OLD file (deletions to restore)
    pub old_lines: Vec<String>,
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

    // Tree view state
    tree: Vec<TreeNode>,
    pub flat_tree: Vec<FlatTreeItem>,
    pub tree_state: ListState,
    section_expanded: BTreeMap<GitSection, bool>,
    dir_expanded: BTreeSet<String>,

    pub diff_mode: GitDiffMode,
    pub diff_lines: Vec<String>,
    pub diff_hunks: Vec<DiffHunk>,
    pub change_blocks: Vec<ChangeBlock>,
    pub diff_scroll_y: u16,
    pub diff_scroll_x: u16,
    pub diff_generation: u64,
    pub diff_request_id: u64,

    /// Show full file content instead of diff
    pub show_full_file: bool,
    pub full_file_content: Option<String>,
    pub full_file_scroll_y: u16,
}

impl GitState {
    pub fn new() -> Self {
        let mut section_expanded = BTreeMap::new();
        section_expanded.insert(GitSection::Staged, true);
        section_expanded.insert(GitSection::Working, true);
        section_expanded.insert(GitSection::Untracked, true);
        section_expanded.insert(GitSection::Conflicts, true);

        Self {
            repo_root: None,
            branch: String::new(),
            ahead: 0,
            behind: 0,
            section: GitSection::Working,
            entries: Vec::new(),
            filtered: Vec::new(),
            list_state: ListState::default(),
            selected_paths: BTreeSet::new(),
            selection_anchor: None,
            tree: Vec::new(),
            flat_tree: Vec::new(),
            tree_state: ListState::default(),
            section_expanded,
            dir_expanded: BTreeSet::new(),
            diff_mode: GitDiffMode::SideBySide,
            diff_lines: Vec::new(),
            diff_hunks: Vec::new(),
            change_blocks: Vec::new(),
            diff_scroll_y: 0,
            diff_scroll_x: 0,
            diff_generation: 0,
            diff_request_id: 0,
            show_full_file: false,
            full_file_content: None,
            full_file_scroll_y: 0,
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
        self.diff_generation = 0;
        self.diff_request_id = 0;

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
        self.repo_root = Some(root.clone());

        let out = run_git(&root, &["status", "--porcelain=v1", "-z", "-b"]);
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
                    // Check if it's a directory (ends with / or is actually a directory)
                    let full_path = root.join(&path);
                    if full_path.is_dir() {
                        // Expand untracked directory - list all files recursively
                        fn collect_untracked_files(
                            dir: &std::path::Path,
                            base: &std::path::Path,
                            entries: &mut Vec<GitFileEntry>,
                        ) {
                            if let Ok(read_dir) = std::fs::read_dir(dir) {
                                for entry in read_dir.filter_map(|e| e.ok()) {
                                    let entry_path = entry.path();
                                    if entry_path.is_dir() {
                                        collect_untracked_files(&entry_path, base, entries);
                                    } else {
                                        if let Ok(rel) = entry_path.strip_prefix(base) {
                                            entries.push(GitFileEntry {
                                                path: rel.to_string_lossy().to_string(),
                                                x: '?',
                                                y: '?',
                                                is_untracked: true,
                                                is_conflict: false,
                                                renamed_from: None,
                                            });
                                        }
                                    }
                                }
                            }
                        }
                        collect_untracked_files(&full_path, &root, &mut self.entries);
                    } else {
                        self.entries.push(GitFileEntry {
                            path,
                            x: '?',
                            y: '?',
                            is_untracked: true,
                            is_conflict: false,
                            renamed_from: None,
                        });
                    }
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
        self.build_tree();
    }

    pub fn set_section(&mut self, section: GitSection) {
        self.section = section;
        self.update_filtered();
    }

    pub fn select_filtered(&mut self, idx: usize) {
        self.list_state.select(Some(idx));
        self.diff_scroll_y = 0;
        self.diff_scroll_x = 0;
    }

    /// Set diff lines and parse hunks for revert functionality
    pub fn set_diff_lines(&mut self, lines: Vec<String>) {
        self.diff_lines = lines;
        self.parse_hunks();
    }

    /// Parse hunks from diff_lines, tracking display row positions
    fn parse_hunks(&mut self) {
        self.diff_hunks.clear();

        let mut current_hunk_lines: Vec<String> = Vec::new();
        let mut current_hunk_start_display: usize = 0;
        let mut file_header: Vec<String> = Vec::new();
        let mut in_hunk = false;
        let mut display_row: usize = 0;

        for line in &self.diff_lines {
            // Skip meta lines for display row counting (they're filtered in unified view)
            let is_meta =
                line.starts_with("index ") || line.starts_with("--- ") || line.starts_with("+++ ");

            if line.starts_with("diff --git ") {
                // Save previous hunk if any
                if in_hunk && !current_hunk_lines.is_empty() {
                    let mut full_hunk = file_header.clone();
                    full_hunk.extend(current_hunk_lines.drain(..));
                    self.diff_hunks.push(DiffHunk {
                        display_row: current_hunk_start_display,
                        sbs_display_row: 0,
                        lines: full_hunk,
                    });
                }

                // Start new file
                file_header.clear();
                file_header.push(line.clone());
                in_hunk = false;
                display_row += 1; // diff --git line is shown
            } else if line.starts_with("index ")
                || line.starts_with("--- ")
                || line.starts_with("+++ ")
            {
                file_header.push(line.clone());
                // These are not displayed in unified view
            } else if line.starts_with("@@") {
                // Save previous hunk if any
                if in_hunk && !current_hunk_lines.is_empty() {
                    let mut full_hunk = file_header.clone();
                    full_hunk.extend(current_hunk_lines.drain(..));
                    self.diff_hunks.push(DiffHunk {
                        display_row: current_hunk_start_display,
                        sbs_display_row: 0,
                        lines: full_hunk,
                    });
                }

                // Start new hunk
                current_hunk_start_display = display_row;
                current_hunk_lines.clear();
                current_hunk_lines.push(line.clone());
                in_hunk = true;
                display_row += 1;
            } else if in_hunk {
                current_hunk_lines.push(line.clone());
                if !is_meta {
                    display_row += 1;
                }
            } else if !is_meta {
                display_row += 1;
            }
        }

        // Don't forget last hunk
        if in_hunk && !current_hunk_lines.is_empty() {
            let mut full_hunk = file_header.clone();
            full_hunk.extend(current_hunk_lines);
            self.diff_hunks.push(DiffHunk {
                display_row: current_hunk_start_display,
                sbs_display_row: 0,
                lines: full_hunk,
            });
        }

        // Calculate side-by-side display rows
        self.compute_sbs_hunk_rows();
    }

    /// Compute side-by-side display row indices and change blocks
    fn compute_sbs_hunk_rows(&mut self) {
        use crate::git::build_side_by_side_rows;

        self.change_blocks.clear();
        let rows = build_side_by_side_rows(&self.diff_lines);
        let mut hunk_idx = 0;
        // Track display row matching the actual rendering output
        let mut row_idx = 1usize; // Start at 1 for title row
        let mut first_file = true;

        // Track current file path and line numbers
        let mut current_file_path = String::new();
        let mut new_line: u32 = 1;

        // For grouping consecutive changes
        struct PendingBlock {
            display_row: usize,
            file_path: String,
            new_start: u32,
            old_lines: Vec<String>,
            new_lines: Vec<String>,
        }

        let mut current_block: Option<PendingBlock> = None;

        for row in &rows {
            match row {
                GitDiffRow::Meta(t) => {
                    if t.starts_with("diff --git") {
                        // Finish any pending block
                        if let Some(block) = current_block.take() {
                            if !block.old_lines.is_empty() || !block.new_lines.is_empty() {
                                self.change_blocks.push(ChangeBlock {
                                    display_row: block.display_row,
                                    file_path: block.file_path,
                                    new_start: block.new_start,
                                    new_lines: block.new_lines,
                                    old_lines: block.old_lines,
                                });
                            }
                        }

                        // Extract file path from "diff --git a/path b/path"
                        current_file_path = t
                            .strip_prefix("diff --git a/")
                            .and_then(|s| s.split(" b/").next())
                            .unwrap_or("")
                            .to_string();

                        if !first_file {
                            row_idx += 2;
                        }
                        first_file = false;
                        row_idx += 1;
                    } else if t.starts_with("index ")
                        || t.starts_with("--- ")
                        || t.starts_with("+++ ")
                    {
                        // Skipped in rendering
                    } else if t.starts_with("@@") {
                        // Finish any pending block
                        if let Some(block) = current_block.take() {
                            if !block.old_lines.is_empty() || !block.new_lines.is_empty() {
                                self.change_blocks.push(ChangeBlock {
                                    display_row: block.display_row,
                                    file_path: block.file_path,
                                    new_start: block.new_start,
                                    new_lines: block.new_lines,
                                    old_lines: block.old_lines,
                                });
                            }
                        }

                        if let Some((_, n)) = parse_hunk_header(t) {
                            new_line = n;
                        }

                        row_idx += 1; // Empty line
                        if hunk_idx < self.diff_hunks.len() {
                            self.diff_hunks[hunk_idx].sbs_display_row = row_idx;
                            hunk_idx += 1;
                        }
                        row_idx += 1; // Hunk header
                    } else {
                        row_idx += 1;
                    }
                }
                GitDiffRow::Split { old, new } => {
                    match (old.kind, new.kind) {
                        (GitDiffCellKind::Context, GitDiffCellKind::Context) => {
                            // Context line - finalize any pending block
                            if let Some(block) = current_block.take() {
                                if !block.old_lines.is_empty() || !block.new_lines.is_empty() {
                                    self.change_blocks.push(ChangeBlock {
                                        display_row: block.display_row,
                                        file_path: block.file_path,
                                        new_start: block.new_start,
                                        new_lines: block.new_lines,
                                        old_lines: block.old_lines,
                                    });
                                }
                            }
                            if new.line_no.is_some() {
                                new_line += 1;
                            }
                        }
                        (GitDiffCellKind::Delete, _) | (_, GitDiffCellKind::Add) => {
                            if current_block.is_none() {
                                current_block = Some(PendingBlock {
                                    display_row: row_idx,
                                    file_path: current_file_path.clone(),
                                    new_start: new_line,
                                    old_lines: Vec::new(),
                                    new_lines: Vec::new(),
                                });
                            }

                            if let Some(ref mut block) = current_block {
                                if old.kind == GitDiffCellKind::Delete {
                                    block.old_lines.push(old.text.clone());
                                }
                                if new.kind == GitDiffCellKind::Add {
                                    block.new_lines.push(new.text.clone());
                                    new_line += 1;
                                }
                            }
                        }
                        _ => {
                            if new.line_no.is_some() {
                                new_line += 1;
                            }
                        }
                    }
                    row_idx += 1;
                }
            }
        }

        // Finish any pending block
        if let Some(block) = current_block.take() {
            if !block.old_lines.is_empty() || !block.new_lines.is_empty() {
                self.change_blocks.push(ChangeBlock {
                    display_row: block.display_row,
                    file_path: block.file_path,
                    new_start: block.new_start,
                    new_lines: block.new_lines,
                    old_lines: block.old_lines,
                });
            }
        }
    }

    /// Get the hunk index at or before a given display row
    pub fn hunk_at_display_row(&self, row: usize) -> Option<usize> {
        let mut result = None;
        for (i, hunk) in self.diff_hunks.iter().enumerate() {
            if hunk.display_row <= row {
                result = Some(i);
            } else {
                break;
            }
        }
        result
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
                GitSection::Working => unstaged && !e.is_conflict && !e.is_untracked,
                GitSection::Staged => staged && !e.is_conflict && !e.is_untracked,
                GitSection::Untracked => e.is_untracked,
                GitSection::Conflicts => e.is_conflict,
            };
            if keep {
                self.filtered.push(idx);
            }
        }

        self.filtered.sort_by(|a, b| {
            let ea = &self.entries[*a];
            let eb = &self.entries[*b];
            let pa = entry_sort_key(ea);
            let pb = entry_sort_key(eb);
            pa.cmp(&pb)
        });

        let selected = self.list_state.selected().unwrap_or(0);
        if self.filtered.is_empty() {
            self.list_state.select(None);
        } else if selected >= self.filtered.len() {
            self.list_state.select(Some(0));
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

    /// Build the tree structure from entries
    pub fn build_tree(&mut self) {
        self.tree.clear();

        // Group entries by section
        let mut staged_entries: Vec<usize> = Vec::new();
        let mut working_entries: Vec<usize> = Vec::new();
        let mut untracked_entries: Vec<usize> = Vec::new();
        let mut conflict_entries: Vec<usize> = Vec::new();

        for (idx, e) in self.entries.iter().enumerate() {
            if e.is_conflict {
                conflict_entries.push(idx);
            } else if e.is_untracked {
                untracked_entries.push(idx);
            } else {
                let staged = e.x != ' ' && e.x != '?';
                let unstaged = e.y != ' ' && e.y != '?';
                if staged {
                    staged_entries.push(idx);
                }
                if unstaged {
                    working_entries.push(idx);
                }
            }
        }

        // Helper to build directory hierarchy from file list
        let build_section = |entries: &[usize],
                             all_entries: &[GitFileEntry],
                             dir_expanded: &BTreeSet<String>,
                             section: GitSection|
         -> Vec<TreeNode> {
            if entries.is_empty() {
                return Vec::new();
            }

            // Build a tree of directories
            #[derive(Default)]
            struct DirNode {
                children: BTreeMap<String, DirNode>,
                files: Vec<(String, usize)>, // (filename, entry_idx)
            }

            let mut root = DirNode::default();

            for &idx in entries {
                let path = &all_entries[idx].path;
                let parts: Vec<&str> = path.split('/').collect();

                if parts.len() == 1 {
                    // File in root
                    root.files.push((parts[0].to_string(), idx));
                } else {
                    // File in a subdirectory
                    let mut current = &mut root;
                    for (i, part) in parts.iter().enumerate() {
                        if i == parts.len() - 1 {
                            // This is the file
                            current.files.push((part.to_string(), idx));
                        } else {
                            // This is a directory
                            current = current.children.entry(part.to_string()).or_default();
                        }
                    }
                }
            }

            // Convert DirNode to TreeNode recursively
            fn convert_dir(
                name: &str,
                path: &str,
                node: DirNode,
                dir_expanded: &BTreeSet<String>,
                section: GitSection,
            ) -> TreeNode {
                let mut children = Vec::new();

                // Add subdirectories first (sorted)
                for (child_name, child_node) in node.children {
                    let child_path = if path.is_empty() {
                        child_name.clone()
                    } else {
                        format!("{}/{}", path, child_name)
                    };
                    children.push(convert_dir(
                        &child_name,
                        &child_path,
                        child_node,
                        dir_expanded,
                        section,
                    ));
                }

                // Then add files (sorted by name)
                let mut files: Vec<_> = node.files;
                files.sort_by(|a, b| a.0.cmp(&b.0));
                for (file_name, entry_idx) in files {
                    children.push(TreeNode::File {
                        name: file_name,
                        entry_idx,
                    });
                }

                // Full path for collapsed tracking (directories are expanded by default)
                let full_path = format!("{:?}:{}", section, path);
                // Expanded by default unless explicitly collapsed
                let expanded = !dir_expanded.contains(&full_path);

                TreeNode::Directory {
                    name: name.to_string(),
                    path: path.to_string(),
                    expanded,
                    children,
                }
            }

            // Convert root children to tree nodes
            let mut result = Vec::new();

            // Add subdirectories
            for (child_name, child_node) in root.children {
                result.push(convert_dir(
                    &child_name,
                    &child_name,
                    child_node,
                    dir_expanded,
                    section,
                ));
            }

            // Add root-level files
            let mut files: Vec<_> = root.files;
            files.sort_by(|a, b| a.0.cmp(&b.0));
            for (file_name, entry_idx) in files {
                result.push(TreeNode::File {
                    name: file_name,
                    entry_idx,
                });
            }

            result
        };

        // Build sections in order: Staged, Changes, Untracked, Conflicts
        if !staged_entries.is_empty() {
            let children = build_section(
                &staged_entries,
                &self.entries,
                &self.dir_expanded,
                GitSection::Staged,
            );
            self.tree.push(TreeNode::Section {
                kind: GitSection::Staged,
                expanded: *self
                    .section_expanded
                    .get(&GitSection::Staged)
                    .unwrap_or(&true),
                children,
            });
        }

        if !working_entries.is_empty() {
            let children = build_section(
                &working_entries,
                &self.entries,
                &self.dir_expanded,
                GitSection::Working,
            );
            self.tree.push(TreeNode::Section {
                kind: GitSection::Working,
                expanded: *self
                    .section_expanded
                    .get(&GitSection::Working)
                    .unwrap_or(&true),
                children,
            });
        }

        if !untracked_entries.is_empty() {
            let children = build_section(
                &untracked_entries,
                &self.entries,
                &self.dir_expanded,
                GitSection::Untracked,
            );
            self.tree.push(TreeNode::Section {
                kind: GitSection::Untracked,
                expanded: *self
                    .section_expanded
                    .get(&GitSection::Untracked)
                    .unwrap_or(&true),
                children,
            });
        }

        if !conflict_entries.is_empty() {
            let children = build_section(
                &conflict_entries,
                &self.entries,
                &self.dir_expanded,
                GitSection::Conflicts,
            );
            self.tree.push(TreeNode::Section {
                kind: GitSection::Conflicts,
                expanded: *self
                    .section_expanded
                    .get(&GitSection::Conflicts)
                    .unwrap_or(&true),
                children,
            });
        }

        self.flatten_tree();
    }

    /// Flatten tree into visible items respecting expand/collapse state
    pub fn flatten_tree(&mut self) {
        self.flat_tree.clear();

        fn flatten_node(
            node: &TreeNode,
            depth: usize,
            section: GitSection,
            out: &mut Vec<FlatTreeItem>,
        ) {
            match node {
                TreeNode::Section {
                    kind,
                    expanded,
                    children,
                } => {
                    out.push(FlatTreeItem {
                        depth,
                        node_type: FlatNodeType::Section,
                        expanded: *expanded,
                        entry_idx: None,
                        name: match kind {
                            GitSection::Staged => "Staged Changes".to_string(),
                            GitSection::Working => "Changes".to_string(),
                            GitSection::Untracked => "Untracked".to_string(),
                            GitSection::Conflicts => "Conflicts".to_string(),
                        },
                        path: String::new(),
                        section: *kind,
                    });

                    if *expanded {
                        for child in children {
                            flatten_node(child, depth + 1, *kind, out);
                        }
                    }
                }
                TreeNode::Directory {
                    name,
                    path,
                    expanded,
                    children,
                } => {
                    out.push(FlatTreeItem {
                        depth,
                        node_type: FlatNodeType::Directory,
                        expanded: *expanded,
                        entry_idx: None,
                        name: name.clone(),
                        path: path.clone(),
                        section,
                    });

                    if *expanded {
                        for child in children {
                            flatten_node(child, depth + 1, section, out);
                        }
                    }
                }
                TreeNode::File { name, entry_idx } => {
                    out.push(FlatTreeItem {
                        depth,
                        node_type: FlatNodeType::File,
                        expanded: false,
                        entry_idx: Some(*entry_idx),
                        name: name.clone(),
                        path: String::new(),
                        section,
                    });
                }
            }
        }

        for node in &self.tree {
            flatten_node(node, 0, GitSection::Working, &mut self.flat_tree);
        }

        // Preserve selection if possible
        let current_sel = self.tree_state.selected();
        if self.flat_tree.is_empty() {
            self.tree_state.select(None);
        } else if let Some(sel) = current_sel {
            if sel >= self.flat_tree.len() {
                self.tree_state.select(Some(self.flat_tree.len() - 1));
            }
        } else {
            // Select first file if available
            for (i, item) in self.flat_tree.iter().enumerate() {
                if item.node_type == FlatNodeType::File {
                    self.tree_state.select(Some(i));
                    break;
                }
            }
        }
    }

    /// Toggle expand/collapse for the currently selected tree item
    pub fn toggle_tree_expand(&mut self) {
        let Some(sel) = self.tree_state.selected() else {
            return;
        };
        let Some(item) = self.flat_tree.get(sel) else {
            return;
        };

        match item.node_type {
            FlatNodeType::Section => {
                let section = item.section;
                let current = *self.section_expanded.get(&section).unwrap_or(&true);
                self.section_expanded.insert(section, !current);
                self.rebuild_tree_structure();
            }
            FlatNodeType::Directory => {
                let key = format!("{:?}:{}", item.section, item.path);
                if self.dir_expanded.contains(&key) {
                    self.dir_expanded.remove(&key);
                } else {
                    self.dir_expanded.insert(key);
                }
                self.rebuild_tree_structure();
            }
            FlatNodeType::File => {
                // No action for files
            }
        }
    }

    /// Expand the currently selected tree item
    pub fn expand_tree_item(&mut self) {
        let Some(sel) = self.tree_state.selected() else {
            return;
        };
        let Some(item) = self.flat_tree.get(sel) else {
            return;
        };

        match item.node_type {
            FlatNodeType::Section => {
                let section = item.section;
                if !*self.section_expanded.get(&section).unwrap_or(&true) {
                    self.section_expanded.insert(section, true);
                    self.rebuild_tree_structure();
                }
            }
            FlatNodeType::Directory => {
                // dir_expanded now stores COLLAPSED paths, so remove to expand
                let key = format!("{:?}:{}", item.section, item.path);
                if self.dir_expanded.contains(&key) {
                    self.dir_expanded.remove(&key);
                    self.rebuild_tree_structure();
                }
            }
            FlatNodeType::File => {}
        }
    }

    /// Collapse the currently selected tree item
    pub fn collapse_tree_item(&mut self) {
        let Some(sel) = self.tree_state.selected() else {
            return;
        };
        let Some(item) = self.flat_tree.get(sel) else {
            return;
        };

        match item.node_type {
            FlatNodeType::Section => {
                let section = item.section;
                if *self.section_expanded.get(&section).unwrap_or(&true) {
                    self.section_expanded.insert(section, false);
                    self.rebuild_tree_structure();
                }
            }
            FlatNodeType::Directory => {
                // dir_expanded now stores COLLAPSED paths, so insert to collapse
                let key = format!("{:?}:{}", item.section, item.path);
                if !self.dir_expanded.contains(&key) {
                    self.dir_expanded.insert(key);
                    self.rebuild_tree_structure();
                }
            }
            FlatNodeType::File => {
                // For files, collapse parent directory or go to parent
                if item.depth > 1 {
                    // Find parent directory and select it
                    for i in (0..sel).rev() {
                        if let Some(parent) = self.flat_tree.get(i) {
                            if parent.depth < item.depth
                                && parent.node_type == FlatNodeType::Directory
                            {
                                self.tree_state.select(Some(i));
                                return;
                            }
                        }
                    }
                }
            }
        }
    }

    fn rebuild_tree_structure(&mut self) {
        // Update the tree nodes with current expansion state
        fn update_section(
            node: &mut TreeNode,
            section_expanded: &BTreeMap<GitSection, bool>,
            dir_expanded: &BTreeSet<String>,
        ) {
            match node {
                TreeNode::Section {
                    kind,
                    expanded,
                    children,
                } => {
                    *expanded = *section_expanded.get(kind).unwrap_or(&true);
                    for child in children {
                        update_section(child, section_expanded, dir_expanded);
                    }
                }
                TreeNode::Directory {
                    path,
                    expanded,
                    children,
                    ..
                } => {
                    // dir_expanded contains COLLAPSED paths, so negate the check
                    let collapsed_any = dir_expanded
                        .iter()
                        .any(|k| k.ends_with(&format!(":{}", path)));
                    *expanded = !collapsed_any;
                    for child in children {
                        update_section(child, section_expanded, dir_expanded);
                    }
                }
                TreeNode::File { .. } => {}
            }
        }

        for node in &mut self.tree {
            update_section(node, &self.section_expanded, &self.dir_expanded);
        }

        self.flatten_tree();
    }

    /// Select the next item in tree view
    pub fn tree_move_down(&mut self) {
        let current = self.tree_state.selected().unwrap_or(0);
        if current + 1 < self.flat_tree.len() {
            self.tree_state.select(Some(current + 1));
            self.diff_scroll_y = 0;
            self.diff_scroll_x = 0;
        }
    }

    /// Select the previous item in tree view
    pub fn tree_move_up(&mut self) {
        let current = self.tree_state.selected().unwrap_or(0);
        if current > 0 {
            self.tree_state.select(Some(current - 1));
            self.diff_scroll_y = 0;
            self.diff_scroll_x = 0;
        }
    }

    /// Get the currently selected tree item
    pub fn selected_tree_item(&self) -> Option<&FlatTreeItem> {
        self.tree_state
            .selected()
            .and_then(|i| self.flat_tree.get(i))
    }

    /// Get the entry for the currently selected tree item (if it's a file)
    pub fn selected_tree_entry(&self) -> Option<&GitFileEntry> {
        self.selected_tree_item()
            .and_then(|item| item.entry_idx)
            .and_then(|idx| self.entries.get(idx))
    }

    /// Get paths of all selected items in tree view
    /// Supports files, directories (all files under it), and sections (all files in section)
    pub fn selected_tree_paths(&self) -> Vec<String> {
        if !self.selected_paths.is_empty() {
            return self.selected_paths.iter().cloned().collect();
        }

        let Some(item) = self.selected_tree_item() else {
            return Vec::new();
        };

        match item.node_type {
            FlatNodeType::File => {
                if let Some(idx) = item.entry_idx {
                    if let Some(e) = self.entries.get(idx) {
                        return vec![e.path.clone()];
                    }
                }
                Vec::new()
            }
            FlatNodeType::Directory => {
                // Collect all files under this directory in the same section
                let dir_path = &item.path;
                let section = item.section;
                let prefix = format!("{}/", dir_path);

                self.entries
                    .iter()
                    .filter(|e| {
                        let e_section = Self::entry_section(e);
                        e_section == section && (e.path.starts_with(&prefix) || e.path == *dir_path)
                    })
                    .map(|e| e.path.clone())
                    .collect()
            }
            FlatNodeType::Section => {
                // Collect all files in this section
                let section = item.section;
                self.entries
                    .iter()
                    .filter(|e| Self::entry_section(e) == section)
                    .map(|e| e.path.clone())
                    .collect()
            }
        }
    }

    /// Determine which section an entry belongs to
    fn entry_section(entry: &GitFileEntry) -> GitSection {
        if entry.is_conflict {
            GitSection::Conflicts
        } else if entry.x == '?' {
            GitSection::Untracked
        } else if entry.x != ' ' && entry.x != '?' {
            // Has staged changes
            if entry.y != ' ' && entry.y != '?' {
                // Also has unstaged changes - entry appears in both sections
                // For toggle purposes, treat as staged if we're looking from staged view
                GitSection::Staged
            } else {
                GitSection::Staged
            }
        } else {
            GitSection::Working
        }
    }

    /// Select tree item at index
    pub fn select_tree(&mut self, idx: usize) {
        if idx < self.flat_tree.len() {
            self.tree_state.select(Some(idx));
            self.diff_scroll_y = 0;
            self.diff_scroll_x = 0;
        }
    }

    /// Go to first item
    pub fn tree_goto_first(&mut self) {
        if !self.flat_tree.is_empty() {
            self.tree_state.select(Some(0));
            self.diff_scroll_y = 0;
            self.diff_scroll_x = 0;
        }
    }

    /// Go to last item
    pub fn tree_goto_last(&mut self) {
        if !self.flat_tree.is_empty() {
            self.tree_state.select(Some(self.flat_tree.len() - 1));
            self.diff_scroll_y = 0;
            self.diff_scroll_x = 0;
        }
    }

    /// Select a file by path (searches all sections)
    /// Returns true if found and selected
    pub fn select_by_path(&mut self, path: &str) -> bool {
        for (i, item) in self.flat_tree.iter().enumerate() {
            if item.node_type == FlatNodeType::File {
                if let Some(entry_idx) = item.entry_idx {
                    if let Some(entry) = self.entries.get(entry_idx) {
                        if entry.path == path {
                            self.tree_state.select(Some(i));
                            self.diff_scroll_y = 0;
                            self.diff_scroll_x = 0;
                            return true;
                        }
                    }
                }
            }
        }
        false
    }

    /// Remember current selection path for restoration after refresh
    pub fn selected_path(&self) -> Option<String> {
        self.selected_tree_entry().map(|e| e.path.clone())
    }

    /// Count files in each section
    pub fn section_counts(&self) -> (usize, usize, usize, usize) {
        let mut staged = 0;
        let mut working = 0;
        let mut untracked = 0;
        let mut conflicts = 0;

        for e in &self.entries {
            if e.is_conflict {
                conflicts += 1;
            } else if e.is_untracked {
                untracked += 1;
            } else {
                let s = e.x != ' ' && e.x != '?';
                let u = e.y != ' ' && e.y != '?';
                if s {
                    staged += 1;
                }
                if u {
                    working += 1;
                }
            }
        }

        (staged, working, untracked, conflicts)
    }

    /// Expand all directories
    pub fn expand_all(&mut self) {
        // Expand all sections
        self.section_expanded.insert(GitSection::Staged, true);
        self.section_expanded.insert(GitSection::Working, true);
        self.section_expanded.insert(GitSection::Untracked, true);
        self.section_expanded.insert(GitSection::Conflicts, true);

        // dir_expanded stores COLLAPSED paths, so clear it to expand all
        self.dir_expanded.clear();

        self.rebuild_tree_structure();
    }

    /// Collapse all directories
    pub fn collapse_all(&mut self) {
        // dir_expanded stores COLLAPSED paths, so add all dir paths
        fn collect_dirs(node: &TreeNode, section: GitSection, out: &mut BTreeSet<String>) {
            match node {
                TreeNode::Section { kind, children, .. } => {
                    for child in children {
                        collect_dirs(child, *kind, out);
                    }
                }
                TreeNode::Directory { path, children, .. } => {
                    out.insert(format!("{:?}:{}", section, path));
                    for child in children {
                        collect_dirs(child, section, out);
                    }
                }
                TreeNode::File { .. } => {}
            }
        }

        self.dir_expanded.clear();
        for node in &self.tree {
            collect_dirs(node, GitSection::Working, &mut self.dir_expanded);
        }

        self.rebuild_tree_structure();
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

pub fn display_width(s: &str) -> usize {
    s.chars()
        .map(|ch| {
            if ch == '\t' {
                4
            } else {
                UnicodeWidthChar::width(ch).unwrap_or(0)
            }
        })
        .sum()
}

pub fn slice_chars(s: &str, start: usize, max_len: usize) -> String {
    if max_len == 0 {
        return String::new();
    }

    let mut out = String::new();
    let mut col = 0usize;
    let mut taken = 0usize;

    for ch in s.chars() {
        let w = if ch == '\t' {
            4
        } else {
            UnicodeWidthChar::width(ch).unwrap_or(0)
        };

        if col + w <= start {
            col += w;
            continue;
        }

        if taken + w > max_len {
            break;
        }

        out.push(ch);
        taken += w;
        col += w;

        if taken >= max_len {
            break;
        }
    }

    out
}

pub fn truncate_to_width(s: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }

    let mut out = String::new();
    let mut wsum = 0usize;

    for ch in s.chars() {
        let w = if ch == '\t' {
            4
        } else {
            UnicodeWidthChar::width(ch).unwrap_or(0)
        };
        if wsum + w > width {
            break;
        }
        out.push(ch);
        wsum += w;
        if wsum >= width {
            break;
        }
    }

    out
}

pub fn pad_to_width(mut s: String, width: usize) -> String {
    if width == 0 {
        return String::new();
    }

    let w = display_width(&s);
    if w >= width {
        return truncate_to_width(&s, width);
    }

    s.push_str(&" ".repeat(width - w));
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

fn wrap_text_to_width(s: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![String::new()];
    }

    if display_width(s) <= width {
        return vec![s.to_string()];
    }

    let width_of = |ch: char| -> usize {
        if ch == '\t' {
            4
        } else {
            UnicodeWidthChar::width(ch).unwrap_or(0)
        }
    };

    let items: Vec<(usize, char, usize)> = s
        .char_indices()
        .map(|(i, ch)| (i, ch, width_of(ch)))
        .collect();

    let mut out = Vec::new();
    let mut i = 0usize;

    while i < items.len() {
        let start_byte = items[i].0;
        let mut wsum = 0usize;
        let mut j = i;
        let mut last_break: Option<usize> = None;

        while j < items.len() {
            let (_b, ch, w) = items[j];

            if ch.is_whitespace() || matches!(ch, '-' | '/' | ',' | '.' | ';' | ':') {
                last_break = Some(j + 1);
            }

            if wsum > 0 && wsum + w > width {
                break;
            }

            wsum += w;
            j += 1;

            if wsum >= width {
                break;
            }
        }

        if j >= items.len() {
            let seg = s[start_byte..].trim_end_matches(|c: char| c.is_whitespace());
            out.push(seg.to_string());
            break;
        }

        let split_j = last_break.filter(|b| *b > i && *b <= j).unwrap_or(j);
        let end_byte = items.get(split_j).map(|t| t.0).unwrap_or(s.len());
        let seg = s[start_byte..end_byte].trim_end_matches(|c: char| c.is_whitespace());
        out.push(seg.to_string());

        i = split_j;
        while i < items.len() && items[i].1.is_whitespace() {
            i += 1;
        }
    }

    if out.is_empty() {
        out.push(String::new());
    }

    out
}

pub fn render_side_by_side_cell_lines(
    cell: &GitDiffCell,
    width: usize,
    scroll_x: usize,
    wrap: bool,
) -> Vec<String> {
    const GUTTER: usize = 6;

    if width == 0 {
        return vec![String::new()];
    }

    if !wrap {
        return vec![render_side_by_side_cell(cell, width, scroll_x)];
    }

    let marker = match cell.kind {
        GitDiffCellKind::Add => '+',
        GitDiffCellKind::Delete => '-',
        _ => ' ',
    };

    let gutter_first = if let Some(n) = cell.line_no {
        format!("{:>4}{} ", n, marker)
    } else {
        "      ".to_string()
    };

    if width <= GUTTER {
        return vec![truncate_to_width(&gutter_first, width)];
    }

    let code_w = width - GUTTER;

    let indent_bytes = cell
        .text
        .char_indices()
        .take_while(|(_i, c)| c.is_whitespace())
        .last()
        .map(|(i, c)| i + c.len_utf8())
        .unwrap_or(0);

    let (indent, mut rest) = cell.text.split_at(indent_bytes);
    let mut indent_w = display_width(indent);

    if indent_w >= code_w {
        rest = cell.text.as_str();
        indent_w = 0;
    }

    let avail = code_w.saturating_sub(indent_w);
    let mut out = Vec::new();

    let mut lines = wrap_text_to_width(rest, avail);
    if lines.is_empty() {
        lines.push(String::new());
    }

    for (idx, seg) in lines.into_iter().enumerate() {
        let gutter = if idx == 0 {
            gutter_first.clone()
        } else {
            "      ".to_string()
        };

        let code = if indent_w > 0 {
            format!("{}{}", indent, seg)
        } else {
            seg
        };

        out.push(format!("{}{}", gutter, pad_to_width(code, code_w)));
    }

    out
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
