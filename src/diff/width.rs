use unicode_width::UnicodeWidthChar;

pub fn char_width(ch: char) -> usize {
    if ch == '\t' {
        4
    } else {
        UnicodeWidthChar::width(ch).unwrap_or(0)
    }
}

pub fn display_width(s: &str) -> usize {
    s.chars().map(char_width).sum()
}

pub fn slice_chars(s: &str, start: usize, max_len: usize) -> String {
    if max_len == 0 {
        return String::new();
    }

    let mut out = String::new();
    let mut col = 0usize;
    let mut taken = 0usize;

    for ch in s.chars() {
        let w = char_width(ch);
        if col + w <= start {
            col += w;
            continue;
        }
        if taken + w > max_len {
            break;
        }
        if ch == '\t' {
            out.push_str("    ");
        } else {
            out.push(ch);
        }
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
        let w = char_width(ch);
        if wsum + w > width {
            break;
        }
        if ch == '\t' {
            out.push_str("    ");
        } else {
            out.push(ch);
        }
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

#[cfg(test)]
pub fn byte_index_for_col(s: &str, target_col: usize) -> usize {
    if target_col == 0 {
        return 0;
    }

    let mut col = 0usize;
    for (byte, ch) in s.char_indices() {
        let w = char_width(ch);
        if col + w > target_col {
            return byte;
        }
        col += w;
    }
    s.len()
}

#[cfg(test)]
pub fn col_for_byte(s: &str, target_byte: usize) -> usize {
    let mut col = 0usize;
    for (byte, ch) in s.char_indices() {
        if byte >= target_byte {
            break;
        }
        col += char_width(ch);
    }
    col
}

pub fn expand_tabs(s: &str) -> String {
    if s.contains('\t') {
        s.replace('\t', "    ")
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slice_chars_is_utf8_safe() {
        assert_eq!(slice_chars("ab中文cd", 2, 4), "中文");
        assert_eq!(truncate_to_width("😀abc", 2), "😀");
    }

    #[test]
    fn byte_col_conversions_are_boundary_safe() {
        let s = "a中😀z";
        let byte = byte_index_for_col(s, 2);
        assert!(s.is_char_boundary(byte));
        assert!(col_for_byte(s, byte) <= 2);
    }
}
