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
pub struct BranchUi {
    pub open: bool,
    pub query: String,
    pub branches: Vec<BranchEntry>,
    pub filtered: Vec<usize>,
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
            filtered: Vec::new(),
            list_state: ListState::default(),
            confirm_checkout: None,
            status: None,
        }
    }

    pub fn set_branches(&mut self, branches: Vec<BranchEntry>) {
        self.branches = branches;
        self.update_filtered();
        if self.filtered.is_empty() {
            self.list_state.select(None);
        } else {
            self.list_state.select(Some(0));
        }
    }

    pub fn update_filtered(&mut self) {
        let q = self.query.trim().to_lowercase();

        let prev = self
            .list_state
            .selected()
            .and_then(|sel| self.filtered.get(sel).copied());

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

    pub fn selected_branch(&self) -> Option<BranchEntry> {
        let sel = self.list_state.selected()?;
        let idx = *self.filtered.get(sel)?;
        self.branches.get(idx).cloned()
    }

    pub fn move_selection(&mut self, delta: i32) {
        if self.filtered.is_empty() {
            self.list_state.select(None);
            return;
        }
        let cur = self.list_state.selected().unwrap_or(0) as i32;
        let next = (cur + delta).clamp(0, self.filtered.len().saturating_sub(1) as i32);
        self.list_state.select(Some(next as usize));
    }
}
