#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GitDiffCellKind {
    Context,
    Delete,
    Add,
    Empty,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InlineSpan {
    pub start: usize,
    pub end: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitDiffCell {
    pub line_no: Option<u32>,
    pub text: String,
    pub kind: GitDiffCellKind,
    pub inline_spans: Vec<InlineSpan>,
}

impl GitDiffCell {
    pub fn new(line_no: Option<u32>, text: String, kind: GitDiffCellKind) -> Self {
        Self {
            line_no,
            text,
            kind,
            inline_spans: Vec::new(),
        }
    }

    pub fn with_spans(
        line_no: Option<u32>,
        text: String,
        kind: GitDiffCellKind,
        inline_spans: Vec<InlineSpan>,
    ) -> Self {
        Self {
            line_no,
            text,
            kind,
            inline_spans,
        }
    }

    pub fn empty() -> Self {
        Self::new(None, String::new(), GitDiffCellKind::Empty)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GitDiffRow {
    Meta(String),
    Split { old: GitDiffCell, new: GitDiffCell },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnifiedRowKind {
    File,
    Meta,
    Hunk,
    Context,
    Add,
    Delete,
    NoNewline,
    Blank,
    Preamble,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnifiedDiffRow {
    pub kind: UnifiedRowKind,
    pub text: String,
    pub marker: Option<char>,
    pub code: String,
    pub file_path: Option<String>,
    pub inline_spans: Vec<InlineSpan>,
}

impl UnifiedDiffRow {
    pub fn meta(kind: UnifiedRowKind, text: String) -> Self {
        Self {
            kind,
            text,
            marker: None,
            code: String::new(),
            file_path: None,
            inline_spans: Vec::new(),
        }
    }

    pub fn code(kind: UnifiedRowKind, marker: char, code: String, spans: Vec<InlineSpan>) -> Self {
        Self {
            kind,
            text: format!("{}{}", marker, code),
            marker: Some(marker),
            code,
            file_path: None,
            inline_spans: spans,
        }
    }
}
