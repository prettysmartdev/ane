use crate::data::lsp::types::SemanticToken;

/// LSP tokens win over tree-sitter tokens on any overlapping character range.
/// Both inputs must be sorted by (line, start_col).
pub fn merge(ts: &[SemanticToken], lsp: &[SemanticToken]) -> Vec<SemanticToken> {
    let mut result = Vec::with_capacity(ts.len() + lsp.len());

    for ts_tok in ts {
        let overlaps_lsp = lsp.iter().any(|l| {
            l.line == ts_tok.line
                && l.start_col < ts_tok.start_col + ts_tok.length
                && l.start_col + l.length > ts_tok.start_col
        });
        if !overlaps_lsp {
            result.push(ts_tok.clone());
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
}
