use ratatui::{
    prelude::*,
    text::{Line, Span},
};
use std::sync::OnceLock;
use syntect::{
    easy::HighlightLines,
    highlighting::{Theme, ThemeSet},
    parsing::SyntaxSet,
    util::LinesWithEndings,
};

pub struct Highlighter {
    inner: HighlightLines<'static>,
}

fn syntax_set() -> &'static SyntaxSet {
    static SET: OnceLock<SyntaxSet> = OnceLock::new();
    SET.get_or_init(SyntaxSet::load_defaults_newlines)
}

fn theme() -> &'static Theme {
    static THEME: OnceLock<Theme> = OnceLock::new();
    THEME.get_or_init(|| {
        let ts = ThemeSet::load_defaults();
        ts.themes
            .get("base16-ocean.dark")
            .or_else(|| ts.themes.values().next())
            .cloned()
            .unwrap_or_default()
    })
}

pub fn is_supported_extension(ext: &str) -> bool {
    matches!(
        ext,
        "py" | "js" | "jsx" | "ts" | "tsx" | "json" | "toml" | "rs" | "md"
    )
}

pub fn new_highlighter(ext: &str) -> Option<Highlighter> {
    if !is_supported_extension(ext) {
        return None;
    }
    let syntax = syntax_set().find_syntax_by_extension(ext)?;
    Some(Highlighter {
        inner: HighlightLines::new(syntax, theme()),
    })
}

pub fn highlight_text(text: &str, ext: &str, bg: Color) -> Option<Vec<Line<'static>>> {
    let mut hl = new_highlighter(ext)?;
    Some(hl.highlight_lines(text, bg))
}

impl Highlighter {
    pub fn highlight_lines(&mut self, text: &str, bg: Color) -> Vec<Line<'static>> {
        let mut out = Vec::new();
        for raw in LinesWithEndings::from(text) {
            let line = raw.trim_end_matches(['\n', '\r']);
            out.push(self.highlight_line(line, bg));
        }
        if out.is_empty() {
            out.push(Line::raw(""));
        }
        out
    }

    pub fn highlight_line(&mut self, line: &str, bg: Color) -> Line<'static> {
        let ranges = self
            .inner
            .highlight_line(line, syntax_set())
            .unwrap_or_default();
        if ranges.is_empty() {
            return Line::from(Span::styled(line.to_string(), Style::default().bg(bg)));
        }

        let spans: Vec<Span<'static>> = ranges
            .into_iter()
            .map(|(style, text)| {
                let fg = Color::Rgb(style.foreground.r, style.foreground.g, style.foreground.b);
                Span::styled(text.to_string(), Style::default().fg(fg).bg(bg))
            })
            .collect();
        Line::from(spans)
    }

    pub fn highlight_diff_code_with_prefix(
        &mut self,
        prefix: &str,
        code: &str,
        prefix_style: Style,
        bg: Color,
    ) -> Line<'static> {
        let mut spans = Vec::new();
        spans.push(Span::styled(prefix.to_string(), prefix_style.bg(bg)));

        let ranges = self
            .inner
            .highlight_line(code, syntax_set())
            .unwrap_or_default();
        if ranges.is_empty() {
            spans.push(Span::styled(code.to_string(), Style::default().bg(bg)));
            return Line::from(spans);
        }

        for (style, text) in ranges {
            let fg = Color::Rgb(style.foreground.r, style.foreground.g, style.foreground.b);
            spans.push(Span::styled(
                text.to_string(),
                Style::default().fg(fg).bg(bg),
            ));
        }
        Line::from(spans)
    }
}
