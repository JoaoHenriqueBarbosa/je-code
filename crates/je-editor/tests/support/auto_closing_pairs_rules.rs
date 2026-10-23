//! Porte de `src/vs/editor/test/common/modes/supports/autoClosingPairsRules.ts`: os pares de fechamento
//! automático de linguagens de exemplo.

use je_editor::language_configuration::{AutoClosingPair, AutoClosingPairConditional};

pub fn javascript_auto_closing_pairs_rules() -> Vec<AutoClosingPairConditional> {
    vec![
        AutoClosingPairConditional::new("{", "}"),
        AutoClosingPairConditional::new("[", "]"),
        AutoClosingPairConditional::new("(", ")"),
        AutoClosingPairConditional::new("'", "'").not_in(&["string", "comment"]),
        AutoClosingPairConditional::new("\"", "\"").not_in(&["string"]),
        AutoClosingPairConditional::new("`", "`").not_in(&["string", "comment"]),
        AutoClosingPairConditional::new("/**", " */").not_in(&["string"]),
    ]
}

pub fn latex_auto_closing_pairs_rules() -> Vec<AutoClosingPair> {
    vec![
        AutoClosingPair::new("\\left(", "\\right)"),
        AutoClosingPair::new("\\left[", "\\right]"),
        AutoClosingPair::new("\\left\\{", "\\right\\}"),
        AutoClosingPair::new("\\bigl(", "\\bigr)"),
        AutoClosingPair::new("\\bigl[", "\\bigr]"),
        AutoClosingPair::new("\\bigl\\{", "\\bigr\\}"),
        AutoClosingPair::new("\\Bigl(", "\\Bigr)"),
        AutoClosingPair::new("\\Bigl[", "\\Bigr]"),
        AutoClosingPair::new("\\Bigl\\{", "\\Bigr\\}"),
        AutoClosingPair::new("\\biggl(", "\\biggr)"),
        AutoClosingPair::new("\\biggl[", "\\biggr]"),
        AutoClosingPair::new("\\biggl\\{", "\\biggr\\}"),
        AutoClosingPair::new("\\Biggl(", "\\Biggr)"),
        AutoClosingPair::new("\\Biggl[", "\\Biggr]"),
        AutoClosingPair::new("\\Biggl\\{", "\\Biggr\\}"),
        AutoClosingPair::new("\\(", "\\)"),
        AutoClosingPair::new("\\[", "\\]"),
        AutoClosingPair::new("\\{", "\\}"),
        AutoClosingPair::new("{", "}"),
        AutoClosingPair::new("[", "]"),
        AutoClosingPair::new("(", ")"),
        AutoClosingPair::new("`", "'"),
    ]
}
