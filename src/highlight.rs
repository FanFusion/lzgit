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

fn clamp01(v: f32) -> f32 {
    v.clamp(0.0, 1.0)
}

fn srgb_to_linear(c: u8) -> f32 {
    let c = c as f32 / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn luminance(color: Color) -> Option<f32> {
    let Color::Rgb(r, g, b) = color else {
        return None;
    };

    let r = srgb_to_linear(r);
    let g = srgb_to_linear(g);
    let b = srgb_to_linear(b);

    Some(0.2126 * r + 0.7152 * g + 0.0722 * b)
}

fn contrast_ratio(fg: Color, bg: Color) -> Option<f32> {
    let lf = luminance(fg)?;
    let lb = luminance(bg)?;

    let (l1, l2) = if lf >= lb { (lf, lb) } else { (lb, lf) };
    Some((l1 + 0.05) / (l2 + 0.05))
}

fn mix(a: u8, b: u8, alpha: f32) -> u8 {
    let a = a as f32;
    let b = b as f32;
    (a + (b - a) * alpha).round().clamp(0.0, 255.0) as u8
}

fn ensure_contrast(fg: Color, bg: Color) -> Color {
    let Color::Rgb(fr, fg_g, fb) = fg else {
        return fg;
    };
    let Color::Rgb(_, _, _) = bg else {
        return fg;
    };

    let Some(contrast) = contrast_ratio(fg, bg) else {
        return fg;
    };

    let target = 6.5;
    if contrast >= target {
        return fg;
    }

    let bg_l = luminance(bg).unwrap_or(0.0);
    let (tr, tg, tb) = if bg_l < 0.5 {
        (255u8, 255u8, 255u8)
    } else {
        (0u8, 0u8, 0u8)
    };

    let alpha = clamp01((target - contrast) / target);
    Color::Rgb(mix(fr, tr, alpha), mix(fg_g, tg, alpha), mix(fb, tb, alpha))
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
                let fg = ensure_contrast(
                    Color::Rgb(style.foreground.r, style.foreground.g, style.foreground.b),
                    bg,
                );
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
            let fg = ensure_contrast(
                Color::Rgb(style.foreground.r, style.foreground.g, style.foreground.b),
                bg,
            );
            spans.push(Span::styled(
                text.to_string(),
                Style::default().fg(fg).bg(bg),
            ));
        }
        Line::from(spans)
    }
}
