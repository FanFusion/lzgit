use ratatui::widgets::ListState;

#[derive(Clone, Debug)]
pub struct BranchEntry {
    pub name: String,
    pub is_current: bool,
    pub is_remote: bool,
    pub upstream: Option<String>,
    pub track: Option<String>,
}

#[derive(Clone, Debug)]
pub enum BranchListItem {
    Header(String),
    Branch { idx: usize, depth: usize },
}

#[derive(Clone, Debug)]
pub struct BranchUi {
    pub open: bool,
    pub query: String,
    pub branches: Vec<BranchEntry>,
    pub items: Vec<BranchListItem>,
    pub list_state: ListState,
    pub confirm_checkout: Option<String>,
    pub status: Option<String>,
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
            }
        } else {
            score += (30 - idx as i32).max(0);
        }

        last_match = Some(idx);
        pos = idx + ch.len_utf8();
    }

    Some(score)
}

impl BranchUi {
    pub fn new() -> Self {
        Self {
            open: false,
            query: String::new(),
            branches: Vec::new(),
            items: Vec::new(),
            list_state: ListState::default(),
            confirm_checkout: None,
            status: None,
        }
    }

    pub fn set_branches(&mut self, branches: Vec<BranchEntry>) {
        self.branches = branches;
        self.update_filtered();
    }

    pub fn update_filtered(&mut self) {
        let q = self.query.trim().to_lowercase();

        let prev_branch_idx = self
            .selected_branch()
            .map(|b| b.name)
            .and_then(|name| self.branches.iter().position(|b| b.name == name));

        let mut matches: Vec<(i32, usize)> = Vec::new();
        for (i, b) in self.branches.iter().enumerate() {
            if q.is_empty() {
                matches.push((0, i));
                continue;
            }

            if let Some(score) = fuzzy_score(b.name.to_lowercase().as_str(), q.as_str()) {
                matches.push((score, i));
            }
        }

        matches.sort_by(|a, b| {
            let ba = &self.branches[a.1];
            let bb = &self.branches[b.1];
            ba.is_remote
                .cmp(&bb.is_remote)
                .then_with(|| ba.name.cmp(&bb.name))
        });

        let mut locals: Vec<usize> = Vec::new();
        let mut remotes: Vec<usize> = Vec::new();
        for (_, i) in matches {
            if self.branches[i].is_remote {
                remotes.push(i);
            } else {
                locals.push(i);
            }
        }

        self.items.clear();

        if !locals.is_empty() {
            self.items.push(BranchListItem::Header("Local".to_string()));
            for idx in locals {
                let depth = self.branches[idx].name.matches('/').count();
                self.items.push(BranchListItem::Branch { idx, depth });
            }
        }

        if !remotes.is_empty() {
            self.items
                .push(BranchListItem::Header("Remote".to_string()));

            let mut current_remote: Option<String> = None;
            for idx in remotes {
                let name = self.branches[idx].name.as_str();
                let (remote, rest) = name.split_once('/').unwrap_or((name, ""));

                if current_remote.as_deref() != Some(remote) {
                    current_remote = Some(remote.to_string());
                    self.items
                        .push(BranchListItem::Header(format!("  {}", remote)));
                }

                let depth = if rest.is_empty() {
                    1
                } else {
                    1 + rest.matches('/').count()
                };
                self.items.push(BranchListItem::Branch { idx, depth });
            }
        }

        if self.items.is_empty() {
            self.list_state.select(None);
            return;
        }

        let mut desired = None;
        if let Some(prev_idx) = prev_branch_idx {
            for (i, item) in self.items.iter().enumerate() {
                if let BranchListItem::Branch { idx, .. } = item
                    && *idx == prev_idx
                {
                    desired = Some(i);
                    break;
                }
            }
        }

        if desired.is_none() {
            desired = self
                .items
                .iter()
                .position(|i| matches!(i, BranchListItem::Branch { .. }));
        }

        self.list_state.select(desired);
    }

    pub fn selected_branch(&self) -> Option<BranchEntry> {
        let sel = self.list_state.selected()?;
        match self.items.get(sel)? {
            BranchListItem::Branch { idx, .. } => self.branches.get(*idx).cloned(),
            BranchListItem::Header(_) => None,
        }
    }

    pub fn move_selection(&mut self, delta: i32) {
        if self.items.is_empty() {
            self.list_state.select(None);
            return;
        }

        let cur = self.list_state.selected().unwrap_or(0) as i32;
        let mut next = (cur + delta).clamp(0, self.items.len().saturating_sub(1) as i32);

        let step = if delta >= 0 { 1 } else { -1 };
        while let Some(item) = self.items.get(next as usize) {
            if matches!(item, BranchListItem::Branch { .. }) {
                break;
            }
            if next == 0 && step < 0 {
                break;
            }
            if next as usize == self.items.len().saturating_sub(1) && step > 0 {
                break;
            }
            next = (next + step).clamp(0, self.items.len().saturating_sub(1) as i32);
        }

        self.list_state.select(Some(next as usize));
    }
}
