use ratatui::{
    prelude::*,
    text::{Line, Span},
};
use std::{collections::HashMap, sync::OnceLock};
use syntect::{
    easy::HighlightLines,
    highlighting::{
        Color as SyntectColor, FontStyle, ScopeSelectors, StyleModifier, Theme, ThemeItem,
        ThemeSettings,
    },
    parsing::SyntaxSet,
    util::LinesWithEndings,
};

pub struct Highlighter {
    inner: HighlightLines<'static>,
}

/// Cache for highlighted lines to avoid re-highlighting on scroll
pub struct HighlightCache {
    /// Maps (line_number, bg_color) -> highlighted Line
    cache: HashMap<(usize, Color), Line<'static>>,
    /// File content split into lines for quick access
    lines: Vec<String>,
    /// File extension for syntax detection
    ext: String,
}

fn syntax_set() -> &'static SyntaxSet {
    static SET: OnceLock<SyntaxSet> = OnceLock::new();
    SET.get_or_init(SyntaxSet::load_defaults_newlines)
}

/// Create a vibrant Dracula-like theme for syntax highlighting
fn create_vibrant_theme() -> Theme {
    // Dracula-inspired vibrant colors
    let pink = SyntectColor { r: 255, g: 121, b: 198, a: 255 };      // #FF79C6 - keywords
    let cyan = SyntectColor { r: 139, g: 233, b: 253, a: 255 };      // #8BE9FD - functions/types
    let green = SyntectColor { r: 80, g: 250, b: 123, a: 255 };      // #50FA7B - strings
    let yellow = SyntectColor { r: 241, g: 250, b: 140, a: 255 };    // #F1FA8C - classes
    let orange = SyntectColor { r: 255, g: 184, b: 108, a: 255 };    // #FFB86C - numbers/constants
    let purple = SyntectColor { r: 189, g: 147, b: 249, a: 255 };    // #BD93F9 - variables
    let red = SyntectColor { r: 255, g: 85, b: 85, a: 255 };         // #FF5555 - errors
    let comment = SyntectColor { r: 98, g: 114, b: 164, a: 255 };    // #6272A4 - comments
    let fg = SyntectColor { r: 248, g: 248, b: 242, a: 255 };        // #F8F8F2 - foreground

    fn scope(s: &str) -> ScopeSelectors {
        s.parse().unwrap_or_default()
    }

    fn style(fg: SyntectColor) -> StyleModifier {
        StyleModifier {
            foreground: Some(fg),
            background: None,
            font_style: None,
        }
    }

    fn style_bold(fg: SyntectColor) -> StyleModifier {
        StyleModifier {
            foreground: Some(fg),
            background: None,
            font_style: Some(FontStyle::BOLD),
        }
    }

    fn style_italic(fg: SyntectColor) -> StyleModifier {
        StyleModifier {
            foreground: Some(fg),
            background: None,
            font_style: Some(FontStyle::ITALIC),
        }
    }

    Theme {
        name: Some("Vibrant".to_string()),
        author: None,
        settings: ThemeSettings {
            foreground: Some(fg),
            background: None,
            ..Default::default()
        },
        scopes: vec![
            // Comments - italic gray-blue
            ThemeItem { scope: scope("comment"), style: style_italic(comment) },
            // Strings - bright green
            ThemeItem { scope: scope("string"), style: style(green) },
            // Numbers - bright orange
            ThemeItem { scope: scope("constant.numeric"), style: style(orange) },
            // Constants - bright orange
            ThemeItem { scope: scope("constant"), style: style(orange) },
            ThemeItem { scope: scope("constant.language"), style: style(purple) },
            // Keywords - bright pink (bold)
            ThemeItem { scope: scope("keyword"), style: style_bold(pink) },
            ThemeItem { scope: scope("keyword.control"), style: style_bold(pink) },
            ThemeItem { scope: scope("keyword.operator"), style: style(pink) },
            ThemeItem { scope: scope("storage"), style: style_bold(pink) },
            ThemeItem { scope: scope("storage.type"), style: style_bold(cyan) },
            ThemeItem { scope: scope("storage.modifier"), style: style_bold(pink) },
            // Functions - bright cyan
            ThemeItem { scope: scope("entity.name.function"), style: style(cyan) },
            ThemeItem { scope: scope("support.function"), style: style(cyan) },
            ThemeItem { scope: scope("meta.function-call"), style: style(cyan) },
            // Types/Classes - bright yellow
            ThemeItem { scope: scope("entity.name.type"), style: style(yellow) },
            ThemeItem { scope: scope("entity.name.class"), style: style(yellow) },
            ThemeItem { scope: scope("support.type"), style: style(yellow) },
            ThemeItem { scope: scope("support.class"), style: style(yellow) },
            ThemeItem { scope: scope("entity.other.inherited-class"), style: style(yellow) },
            // Variables - bright purple
            ThemeItem { scope: scope("variable"), style: style(purple) },
            ThemeItem { scope: scope("variable.parameter"), style: style_italic(orange) },
            ThemeItem { scope: scope("variable.other"), style: style(fg) },
            // Punctuation - foreground
            ThemeItem { scope: scope("punctuation"), style: style(fg) },
            // Operators - pink
            ThemeItem { scope: scope("keyword.operator"), style: style(pink) },
            // Tags (HTML/XML) - pink
            ThemeItem { scope: scope("entity.name.tag"), style: style(pink) },
            ThemeItem { scope: scope("entity.other.attribute-name"), style: style(green) },
            // Markdown
            ThemeItem { scope: scope("markup.heading"), style: style_bold(purple) },
            ThemeItem { scope: scope("markup.bold"), style: style_bold(orange) },
            ThemeItem { scope: scope("markup.italic"), style: style_italic(yellow) },
            ThemeItem { scope: scope("markup.raw"), style: style(green) },
            ThemeItem { scope: scope("markup.underline.link"), style: style(cyan) },
            // Invalid/Error - red
            ThemeItem { scope: scope("invalid"), style: style(red) },
            // Rust specific
            ThemeItem { scope: scope("entity.name.lifetime"), style: style_italic(pink) },
            ThemeItem { scope: scope("entity.name.module"), style: style(cyan) },
            ThemeItem { scope: scope("support.macro"), style: style(cyan) },
        ],
    }
}

fn theme() -> &'static Theme {
    static THEME: OnceLock<Theme> = OnceLock::new();
    THEME.get_or_init(create_vibrant_theme)
}

pub fn is_supported_extension(ext: &str) -> bool {
    matches!(
        ext,
        // Rust
        "rs" |
        // Python
        "py" |
        // JavaScript/TypeScript
        "js" | "jsx" | "ts" | "tsx" | "mjs" | "cjs" |
        // Web
        "html" | "htm" | "css" | "scss" | "sass" | "less" |
        // Config/Data
        "json" | "toml" | "yaml" | "yml" | "xml" |
        // Shell
        "sh" | "bash" | "zsh" | "fish" |
        // Go
        "go" |
        // C/C++
        "c" | "h" | "cpp" | "hpp" | "cc" | "hh" |
        // Java/Kotlin
        "java" | "kt" | "kts" |
        // Ruby
        "rb" | "erb" |
        // PHP
        "php" |
        // Lua
        "lua" |
        // SQL
        "sql" |
        // Markdown
        "md" | "markdown" |
        // Makefile
        "makefile" | "mk" |
        // Docker
        "dockerfile" |
        // Vim
        "vim" |
        // Diff/Patch
        "diff" | "patch"
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

/// Highlight only a specific range of lines from the text
/// This is the optimized version that only processes visible lines
pub fn highlight_text_range(
    text: &str,
    ext: &str,
    bg: Color,
    start_line: usize,
    num_lines: usize,
) -> Option<Vec<Line<'static>>> {
    let mut hl = new_highlighter(ext)?;
    Some(hl.highlight_lines_range(text, bg, start_line, num_lines))
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

    /// Highlight only a range of lines from the text
    /// This is optimized for rendering only visible lines
    pub fn highlight_lines_range(
        &mut self,
        text: &str,
        bg: Color,
        start_line: usize,
        num_lines: usize,
    ) -> Vec<Line<'static>> {
        let all_lines: Vec<&str> = text.lines().collect();

        // Calculate the actual range to highlight
        let start = start_line.min(all_lines.len());
        let end = (start + num_lines).min(all_lines.len());

        if start >= all_lines.len() {
            return vec![Line::raw("")];
        }

        let mut out = Vec::new();
        for &line in &all_lines[start..end] {
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

impl HighlightCache {
    /// Create a new cache for the given text and extension
    pub fn new(text: String, ext: String) -> Self {
        let lines = text.lines().map(|s| s.to_string()).collect();
        Self {
            cache: HashMap::new(),
            lines,
            ext,
        }
    }

    /// Get highlighted lines for a specific range, using cache when possible
    pub fn get_highlighted_range(
        &mut self,
        start_line: usize,
        num_lines: usize,
        bg: Color,
    ) -> Vec<Line<'static>> {
        // Return empty if out of bounds
        if start_line >= self.lines.len() {
            return vec![Line::raw("")];
        }

        let end_line = (start_line + num_lines).min(self.lines.len());
        let mut result = Vec::new();

        // Check if we need to create a highlighter
        let mut highlighter = None;

        for line_num in start_line..end_line {
            let cache_key = (line_num, bg);

            // Try to get from cache first
            if let Some(cached_line) = self.cache.get(&cache_key) {
                result.push(cached_line.clone());
            } else {
                // Need to highlight this line
                if highlighter.is_none() {
                    highlighter = new_highlighter(&self.ext);
                }

                let line = if let Some(hl) = &mut highlighter {
                    hl.highlight_line(&self.lines[line_num], bg)
                } else {
                    // No highlighter available, return plain text
                    Line::from(Span::styled(
                        self.lines[line_num].clone(),
                        Style::default().bg(bg),
                    ))
                };

                // Cache the result
                self.cache.insert(cache_key, line.clone());
                result.push(line);
            }
        }

        if result.is_empty() {
            result.push(Line::raw(""));
        }
        result
    }

    /// Clear the cache (e.g., when bg color changes or file content changes)
    pub fn clear_cache(&mut self) {
        self.cache.clear();
    }

    /// Get total number of lines
    pub fn line_count(&self) -> usize {
        self.lines.len()
    }
}
