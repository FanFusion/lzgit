use super::model::InlineSpan;

#[derive(Clone, Debug)]
struct Token {
    text: String,
    start: usize,
    end: usize,
    kind: TokenKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TokenKind {
    Word,
    Punctuation,
    Symbol,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LinePair {
    pub delete_index: usize,
    pub add_index: usize,
}

#[derive(Clone, Copy, Debug)]
struct TokenPair {
    old_index: usize,
    new_index: usize,
}

const MIN_INLINE_LINE_SIMILARITY: f64 = 0.45;
const LEADING_TOKEN_MATCH_BONUS: f64 = 0.5;
const MAX_COALESCE_GAP_BYTES: usize = 12;

pub fn pair_inline_spans(
    deletes: &[String],
    adds: &[String],
) -> (Vec<Vec<InlineSpan>>, Vec<Vec<InlineSpan>>, Vec<LinePair>) {
    let mut delete_spans = vec![Vec::new(); deletes.len()];
    let mut add_spans = vec![Vec::new(); adds.len()];
    let pairs = pair_changed_lines(deletes, adds);

    for pair in &pairs {
        let (old_spans, new_spans) =
            inline_spans(&deletes[pair.delete_index], &adds[pair.add_index]);
        delete_spans[pair.delete_index] = old_spans;
        add_spans[pair.add_index] = new_spans;
    }

    (delete_spans, add_spans, pairs)
}

pub fn pair_changed_lines(deletes: &[String], adds: &[String]) -> Vec<LinePair> {
    if deletes.is_empty() || adds.is_empty() {
        return Vec::new();
    }

    let mut scores = vec![vec![0.0; adds.len()]; deletes.len()];
    for (i, old) in deletes.iter().enumerate() {
        for (j, new) in adds.iter().enumerate() {
            scores[i][j] = line_similarity(old, new);
        }
    }

    best_line_pairs(&scores)
}

fn best_line_pairs(scores: &[Vec<f64>]) -> Vec<LinePair> {
    if scores.is_empty() || scores[0].is_empty() {
        return Vec::new();
    }

    let deletes = scores.len();
    let adds = scores[0].len();
    let mut dp = vec![vec![0.0_f64; adds + 1]; deletes + 1];

    for i in (0..deletes).rev() {
        for j in (0..adds).rev() {
            let mut best = dp[i + 1][j].max(dp[i][j + 1]);
            if scores[i][j] >= MIN_INLINE_LINE_SIMILARITY {
                best = best.max(scores[i][j] + dp[i + 1][j + 1]);
            }
            dp[i][j] = best;
        }
    }

    let mut pairs = Vec::new();
    let (mut i, mut j) = (0usize, 0usize);
    while i < deletes && j < adds {
        let pair_score = scores[i][j] + dp[i + 1][j + 1];
        if scores[i][j] >= MIN_INLINE_LINE_SIMILARITY
            && pair_score >= dp[i + 1][j]
            && pair_score >= dp[i][j + 1]
        {
            pairs.push(LinePair {
                delete_index: i,
                add_index: j,
            });
            i += 1;
            j += 1;
        } else if dp[i + 1][j] >= dp[i][j + 1] {
            i += 1;
        } else {
            j += 1;
        }
    }

    pairs
}

pub fn inline_spans(old_code: &str, new_code: &str) -> (Vec<InlineSpan>, Vec<InlineSpan>) {
    let old_tokens = tokenize_inline(old_code);
    let new_tokens = tokenize_inline(new_code);
    let (old_matched, new_matched, pairs) = token_matches(&old_tokens, &new_tokens);

    let mut old_spans = changed_spans(&old_tokens, &old_matched);
    let mut new_spans = changed_spans(&new_tokens, &new_matched);

    if !old_spans.is_empty() && new_spans.is_empty() {
        new_spans = counterpart_spans(&old_tokens, &new_tokens, &pairs, &old_spans, false);
    }
    if !new_spans.is_empty() && old_spans.is_empty() {
        old_spans = counterpart_spans(&old_tokens, &new_tokens, &pairs, &new_spans, true);
    }

    (old_spans, new_spans)
}

fn line_similarity(old_code: &str, new_code: &str) -> f64 {
    let mut old_tokens = tokenize_inline(old_code);
    let mut new_tokens = tokenize_inline(new_code);

    let old_words = similarity_tokens(&old_tokens);
    let new_words = similarity_tokens(&new_tokens);
    if !old_words.is_empty() && !new_words.is_empty() {
        old_tokens = old_words;
        new_tokens = new_words;
    }

    if old_tokens.is_empty() || new_tokens.is_empty() {
        return if old_code == new_code { 1.0 } else { 0.0 };
    }

    let common = token_lcs_matrix(&old_tokens, &new_tokens)[0][0];
    let mut similarity = (2 * common) as f64 / (old_tokens.len() + new_tokens.len()) as f64;
    if old_tokens[0].text == new_tokens[0].text {
        similarity = (similarity + LEADING_TOKEN_MATCH_BONUS).min(1.0);
    }
    similarity
}

fn similarity_tokens(tokens: &[Token]) -> Vec<Token> {
    tokens
        .iter()
        .filter(|token| token.kind == TokenKind::Word)
        .cloned()
        .collect()
}

fn token_matches(
    old_tokens: &[Token],
    new_tokens: &[Token],
) -> (Vec<bool>, Vec<bool>, Vec<TokenPair>) {
    let mut old_matched = vec![false; old_tokens.len()];
    let mut new_matched = vec![false; new_tokens.len()];
    let mut pairs = Vec::new();

    let mut prefix = 0usize;
    while prefix < old_tokens.len()
        && prefix < new_tokens.len()
        && old_tokens[prefix].text == new_tokens[prefix].text
    {
        old_matched[prefix] = true;
        new_matched[prefix] = true;
        pairs.push(TokenPair {
            old_index: prefix,
            new_index: prefix,
        });
        prefix += 1;
    }

    let mut suffix_pairs = Vec::new();
    let mut old_suffix = old_tokens.len();
    let mut new_suffix = new_tokens.len();
    while old_suffix > prefix
        && new_suffix > prefix
        && old_tokens[old_suffix - 1].text == new_tokens[new_suffix - 1].text
    {
        old_suffix -= 1;
        new_suffix -= 1;
        old_matched[old_suffix] = true;
        new_matched[new_suffix] = true;
        suffix_pairs.push(TokenPair {
            old_index: old_suffix,
            new_index: new_suffix,
        });
    }

    let middle_old = &old_tokens[prefix..old_suffix];
    let middle_new = &new_tokens[prefix..new_suffix];
    let dp = token_lcs_matrix(middle_old, middle_new);

    let (mut i, mut j) = (0usize, 0usize);
    while i < middle_old.len() && j < middle_new.len() {
        if middle_old[i].text == middle_new[j].text {
            let old_index = prefix + i;
            let new_index = prefix + j;
            old_matched[old_index] = true;
            new_matched[new_index] = true;
            pairs.push(TokenPair {
                old_index,
                new_index,
            });
            i += 1;
            j += 1;
        } else if dp[i + 1][j] >= dp[i][j + 1] {
            i += 1;
        } else {
            j += 1;
        }
    }

    for pair in suffix_pairs.into_iter().rev() {
        pairs.push(pair);
    }

    (old_matched, new_matched, pairs)
}

fn token_lcs_matrix(old_tokens: &[Token], new_tokens: &[Token]) -> Vec<Vec<usize>> {
    let mut dp = vec![vec![0; new_tokens.len() + 1]; old_tokens.len() + 1];

    for i in (0..old_tokens.len()).rev() {
        for j in (0..new_tokens.len()).rev() {
            if old_tokens[i].text == new_tokens[j].text {
                dp[i][j] = dp[i + 1][j + 1] + 1;
            } else {
                dp[i][j] = dp[i + 1][j].max(dp[i][j + 1]);
            }
        }
    }

    dp
}

fn changed_spans(tokens: &[Token], matched: &[bool]) -> Vec<InlineSpan> {
    let spans: Vec<InlineSpan> = tokens
        .iter()
        .zip(matched.iter())
        .filter_map(|(token, matched)| {
            if *matched {
                None
            } else {
                Some(InlineSpan {
                    start: token.start,
                    end: token.end,
                })
            }
        })
        .collect();
    merge_inline_spans(tokens, spans)
}

fn counterpart_spans(
    old_tokens: &[Token],
    new_tokens: &[Token],
    pairs: &[TokenPair],
    source_spans: &[InlineSpan],
    old_target: bool,
) -> Vec<InlineSpan> {
    let mut spans = Vec::new();
    for pair in pairs {
        let (source, target) = if old_target {
            (&new_tokens[pair.new_index], &old_tokens[pair.old_index])
        } else {
            (&old_tokens[pair.old_index], &new_tokens[pair.new_index])
        };
        if target.kind != TokenKind::Word || !token_overlaps_spans(source, source_spans) {
            continue;
        }
        spans.push(InlineSpan {
            start: target.start,
            end: target.end,
        });
    }

    let target_tokens = if old_target { old_tokens } else { new_tokens };
    merge_inline_spans(target_tokens, spans)
}

fn token_overlaps_spans(token: &Token, spans: &[InlineSpan]) -> bool {
    spans
        .iter()
        .any(|span| token.start < span.end && token.end > span.start)
}

fn merge_inline_spans(tokens: &[Token], spans: Vec<InlineSpan>) -> Vec<InlineSpan> {
    if spans.is_empty() {
        return Vec::new();
    }

    let mut merged = vec![spans[0]];
    for span in spans.into_iter().skip(1) {
        let last = merged.last_mut().expect("merged has one span");
        if span.start <= last.end || should_coalesce_inline_gap(tokens, last.end, span.start) {
            last.end = last.end.max(span.end);
        } else {
            merged.push(span);
        }
    }
    merged
}

fn should_coalesce_inline_gap(tokens: &[Token], start: usize, end: usize) -> bool {
    if end.saturating_sub(start) > MAX_COALESCE_GAP_BYTES {
        return false;
    }

    let mut gap_tokens = 0usize;
    for token in tokens {
        if token.start >= end {
            return true;
        }
        if token.end <= start {
            continue;
        }
        gap_tokens += 1;
        if gap_tokens > 2 {
            return false;
        }
    }
    true
}

fn tokenize_inline(text: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut iter = text.char_indices().peekable();

    while let Some((start, ch)) = iter.next() {
        if ch.is_whitespace() {
            continue;
        }

        let kind = token_kind(ch);
        let mut end = start + ch.len_utf8();
        if kind == TokenKind::Word {
            while let Some(&(next_start, next)) = iter.peek() {
                if next.is_whitespace() || token_kind(next) != kind {
                    break;
                }
                iter.next();
                end = next_start + next.len_utf8();
            }
        }

        tokens.push(Token {
            text: text[start..end].to_string(),
            start,
            end,
            kind,
        });
    }

    tokens
}

fn token_kind(ch: char) -> TokenKind {
    if ch.is_alphanumeric() || ch == '_' {
        TokenKind::Word
    } else if ch.is_ascii_punctuation() {
        TokenKind::Punctuation
    } else {
        TokenKind::Symbol
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span_texts<'a>(text: &'a str, spans: &[InlineSpan]) -> Vec<&'a str> {
        spans
            .iter()
            .map(|span| &text[span.start..span.end])
            .collect()
    }

    #[test]
    fn inline_spans_highlight_changed_word() {
        let old = "let color = \"red\";";
        let new = "let color = \"blue\";";
        let (old_spans, new_spans) = inline_spans(old, new);
        assert_eq!(span_texts(old, &old_spans), vec!["red"]);
        assert_eq!(span_texts(new, &new_spans), vec!["blue"]);
    }

    #[test]
    fn inline_spans_keep_common_prefix_suffix() {
        let old = "prefix call(old_value, keep); suffix";
        let new = "prefix call(new_value, keep); suffix";
        let (old_spans, new_spans) = inline_spans(old, new);
        assert_eq!(span_texts(old, &old_spans), vec!["old_value"]);
        assert_eq!(span_texts(new, &new_spans), vec!["new_value"]);
    }

    #[test]
    fn unrelated_lines_are_not_paired() {
        let old = vec!["alpha beta".to_string()];
        let new = vec!["one two".to_string()];
        assert!(pair_changed_lines(&old, &new).is_empty());
    }

    #[test]
    fn inline_spans_coalesce_small_gaps() {
        let old = "call(foo, bar)";
        let new = "call(baz, qux)";
        let (old_spans, new_spans) = inline_spans(old, new);
        assert_eq!(span_texts(old, &old_spans), vec!["foo, bar"]);
        assert_eq!(span_texts(new, &new_spans), vec!["baz, qux"]);
    }

    #[test]
    fn inline_spans_unicode_safe() {
        let old = "let 标题 = \"红色😀\";";
        let new = "let 标题 = \"蓝色😀\";";
        let (old_spans, new_spans) = inline_spans(old, new);
        for span in old_spans.iter().chain(new_spans.iter()) {
            assert!(old.is_char_boundary(span.start) || new.is_char_boundary(span.start));
            assert!(old.is_char_boundary(span.end) || new.is_char_boundary(span.end));
        }
        assert_eq!(span_texts(old, &old_spans), vec!["红色"]);
        assert_eq!(span_texts(new, &new_spans), vec!["蓝色"]);
    }

    #[test]
    fn pairs_replacements_around_inserted_line() {
        let old = vec!["foo := oldValue + 1".to_string(), "keep()".to_string()];
        let new = vec![
            "inserted()".to_string(),
            "foo := newValue + 1".to_string(),
            "keep()".to_string(),
        ];
        let pairs = pair_changed_lines(&old, &new);
        assert_eq!(
            pairs[0],
            LinePair {
                delete_index: 0,
                add_index: 1
            }
        );
        assert_eq!(
            pairs[1],
            LinePair {
                delete_index: 1,
                add_index: 2
            }
        );
    }
}
