use ratatui::widgets::ListState;

#[derive(Clone, Debug)]
pub struct BranchEntry {
    pub name: String,
    pub is_current: bool,
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
        let q = self.query.to_lowercase();
        self.filtered.clear();
        for (i, b) in self.branches.iter().enumerate() {
            if q.is_empty() || b.name.to_lowercase().contains(&q) {
                self.filtered.push(i);
            }
        }

        let sel = self.list_state.selected().unwrap_or(0);
        if self.filtered.is_empty() {
            self.list_state.select(None);
        } else if sel >= self.filtered.len() {
            self.list_state.select(Some(0));
        }
    }

    pub fn selected_branch_name(&self) -> Option<String> {
        let sel = self.list_state.selected()?;
        let idx = *self.filtered.get(sel)?;
        self.branches.get(idx).map(|b| b.name.clone())
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
