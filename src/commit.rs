#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommitFocus {
    Message,
}

#[derive(Clone, Debug)]
pub struct CommitState {
    pub open: bool,
    pub focus: CommitFocus,
    pub message: String,
    pub cursor: usize,
    pub scroll_y: u16,
    pub status: Option<String>,
    pub busy: bool,
}

impl CommitState {
    pub fn new() -> Self {
        Self {
            open: false,
            focus: CommitFocus::Message,
            message: String::new(),
            cursor: 0,
            scroll_y: 0,
            status: None,
            busy: false,
        }
    }

    pub fn set_status<S: Into<String>>(&mut self, msg: S) {
        self.status = Some(msg.into());
    }

    pub fn move_left(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    pub fn move_right(&mut self) {
        let len = self.message.chars().count();
        if self.cursor < len {
            self.cursor += 1;
        }
    }

    pub fn move_home(&mut self) {
        let (line, _) = cursor_line_col(&self.message, self.cursor);
        self.cursor = cursor_to_index_in_line(&self.message, line, 0);
    }

    pub fn move_end(&mut self) {
        let (line, _) = cursor_line_col(&self.message, self.cursor);
        let line_len = line_length(&self.message, line);
        self.cursor = cursor_to_index_in_line(&self.message, line, line_len);
    }

    pub fn insert_char(&mut self, ch: char) {
        let byte = char_to_byte_index(&self.message, self.cursor);
        self.message.insert(byte, ch);
        self.cursor += 1;
    }

    pub fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let idx = self.cursor - 1;
        let b0 = char_to_byte_index(&self.message, idx);
        let b1 = char_to_byte_index(&self.message, self.cursor);
        if b0 < b1 {
            self.message.replace_range(b0..b1, "");
            self.cursor -= 1;
        }
    }

    pub fn delete(&mut self) {
        let len = self.message.chars().count();
        if self.cursor >= len {
            return;
        }
        let b0 = char_to_byte_index(&self.message, self.cursor);
        let b1 = char_to_byte_index(&self.message, self.cursor + 1);
        if b0 < b1 {
            self.message.replace_range(b0..b1, "");
        }
    }

    pub fn cursor_line_col(&self) -> (usize, usize) {
        cursor_line_col(&self.message, self.cursor)
    }

    pub fn ensure_cursor_visible(&mut self, view_height: usize) {
        if view_height == 0 {
            return;
        }
        let (line, _) = self.cursor_line_col();
        let cur = line as i64;
        let top = self.scroll_y as i64;
        let bottom = top + view_height as i64 - 1;

        if cur < top {
            self.scroll_y = line as u16;
        } else if cur > bottom {
            let new_top = (cur - (view_height as i64 - 1)).max(0);
            self.scroll_y = new_top as u16;
        }
    }
}

fn char_to_byte_index(s: &str, char_idx: usize) -> usize {
    if char_idx == 0 {
        return 0;
    }
    s.char_indices()
        .nth(char_idx)
        .map(|(i, _)| i)
        .unwrap_or_else(|| s.len())
}

fn cursor_line_col(s: &str, cursor: usize) -> (usize, usize) {
    let mut line = 0usize;
    let mut col = 0usize;
    for (i, ch) in s.chars().enumerate() {
        if i >= cursor {
            break;
        }
        if ch == '\n' {
            line += 1;
            col = 0;
        } else {
            col += 1;
        }
    }
    (line, col)
}

fn line_length(s: &str, line_index: usize) -> usize {
    s.lines()
        .nth(line_index)
        .map(|l| l.chars().count())
        .unwrap_or(0)
}

fn cursor_to_index_in_line(s: &str, target_line: usize, target_col: usize) -> usize {
    let mut idx = 0usize;
    let mut line = 0usize;
    let mut col = 0usize;

    for ch in s.chars() {
        if line == target_line && col == target_col {
            break;
        }

        if ch == '\n' {
            if line == target_line {
                break;
            }
            line += 1;
            col = 0;
            idx += 1;
            continue;
        }

        if line == target_line {
            col += 1;
        }
        idx += 1;
    }

    idx
}
