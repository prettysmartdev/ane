use crate::data::lsp::types::SemanticToken;

/// LSP tokens win over tree-sitter tokens on any overlapping character range.
/// Both inputs must be sorted by (line, start_col).
pub fn merge(ts: &[SemanticToken], lsp: &[SemanticToken]) -> Vec<SemanticToken> {
    let _timing =
        crate::commands::diagnostics::Timing::new_count("token_merge", ts.len() + lsp.len());
    let mut result = Vec::with_capacity(ts.len() + lsp.len());

    // Collapse LSP intervals into their union. This handles nested and
    // overlapping intervals without assuming their ends are monotonic.
    let mut intervals: Vec<(usize, usize, usize)> = Vec::new();
    for token in lsp {
        let end = token.start_col.saturating_add(token.length);
        if let Some(last) = intervals.last_mut()
            && last.0 == token.line
            && token.start_col < last.2
        {
            last.2 = last.2.max(end);
        } else {
            intervals.push((token.line, token.start_col, end));
        }
    }
    let mut i = 0;
    for token in ts {
        while i < intervals.len()
            && (intervals[i].0 < token.line
                || (intervals[i].0 == token.line && intervals[i].2 <= token.start_col))
        {
            i += 1;
        }
        let overlaps = intervals.get(i).is_some_and(|&(line, start, end)| {
            line == token.line
                && start < token.start_col.saturating_add(token.length)
                && end > token.start_col
        });
        if !overlaps {
            result.push(token.clone());
        }
    }

    result.extend_from_slice(lsp);
    result.sort_by_key(|t| (t.line, t.start_col));
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tok(line: usize, start_col: usize, length: usize, token_type: &str) -> SemanticToken {
        SemanticToken {
            line,
            start_col,
            length,
            token_type: token_type.to_string(),
        }
    }

    #[test]
    fn merge_lsp_wins_on_overlap() {
        // ts token at (line 3, col 5, len 4); lsp at (line 3, col 4, len 6)
        // lsp covers col 4-10, ts covers col 5-9 — they overlap, lsp wins
        let ts = vec![tok(3, 5, 4, "variable")];
        let lsp = vec![tok(3, 4, 6, "type")];
        let result = merge(&ts, &lsp);
        assert_eq!(result.len(), 1, "overlapping ts token should be dropped");
        assert_eq!(result[0].start_col, 4);
        assert_eq!(result[0].length, 6);
        assert_eq!(result[0].token_type, "type");
    }

    #[test]
    fn merge_non_overlapping_tokens_preserved() {
        // ts at col 0, lsp at col 10 — no overlap, both survive, sorted
        let ts = vec![tok(0, 0, 3, "keyword")];
        let lsp = vec![tok(0, 10, 5, "string")];
        let result = merge(&ts, &lsp);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].start_col, 0);
        assert_eq!(result[1].start_col, 10);
    }

    #[test]
    fn merge_lsp_only() {
        let lsp = vec![tok(0, 0, 4, "function"), tok(1, 2, 6, "type")];
        let result = merge(&[], &lsp);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].token_type, "function");
        assert_eq!(result[1].token_type, "type");
    }

    #[test]
    fn merge_ts_only() {
        let ts = vec![tok(0, 0, 2, "keyword"), tok(0, 3, 4, "variable")];
        let result = merge(&ts, &[]);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].token_type, "keyword");
        assert_eq!(result[1].token_type, "variable");
    }
    #[test]
    fn sweep_matches_overlap_oracle_for_nested_and_empty_intervals() {
        // Exercise sorted inputs with nested tokens, equal starts, touching
        // ends and empty ranges, rather than only disjoint happy paths.
        let ts: Vec<_> = (0..3)
            .flat_map(|line| {
                (0..12)
                    .flat_map(move |start| (0..8).map(move |length| tok(line, start, length, "ts")))
            })
            .collect();
        for seed in 0..40 {
            let mut lsp: Vec<_> = (0..30)
                .map(|i| {
                    tok(
                        (i + seed) % 3,
                        (i * 7 + seed) % 12,
                        (i * 11 + seed) % 9,
                        "lsp",
                    )
                })
                .collect();
            lsp.sort_by_key(|t| (t.line, t.start_col));
            let mut expected: Vec<_> = ts
                .iter()
                .filter(|t| {
                    !lsp.iter().any(|l| {
                        l.line == t.line
                            && l.start_col < t.start_col + t.length
                            && l.start_col + l.length > t.start_col
                    })
                })
                .cloned()
                .collect();
            expected.extend_from_slice(&lsp);
            expected.sort_by_key(|t| (t.line, t.start_col));
            let actual = merge(&ts, &lsp);
            let key = |tokens: &[SemanticToken]| {
                tokens
                    .iter()
                    .map(|t| (t.line, t.start_col, t.length, t.token_type.clone()))
                    .collect::<Vec<_>>()
            };
            assert_eq!(key(&actual), key(&expected), "seed {seed}");
        }
    }
}
