//! Porte de `src/vs/editor/common/textModelBracketPairs.ts`: os tipos de resultado do
//! `BracketPairsTextModelPart` (`BracketInfo`, `BracketPairInfo`, `BracketPairWithMinIndentationInfo`,
//! `IFoundBracket`).
//!
//! Divergência: `BracketPairWithMinIndentationInfo` estende `BracketPairInfo` no original; aqui há um tipo só,
//! [`BracketPairInfo`], com `min_visible_column_indentation` (`-1` quando não foi pedido), e
//! [`BracketPairWithMinIndentationInfo`] é um alias dele.

use crate::bracket_pairs_text_model_part::bracket_pairs_tree::ast::AstNodeRef;
use crate::language_brackets_configuration::{BracketKind, ClosingBracketKind, OpeningBracketKind};
use crate::range::Range;

/// `IFoundBracket`.
#[derive(Clone, Debug, PartialEq)]
pub struct FoundBracket {
    pub range: Range,
    pub bracket_info: BracketKind,
}

/// `BracketInfo`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BracketInfo {
    pub range: Range,
    /// Nível a partir de 0
    pub nesting_level: i64,
    pub nesting_level_of_equal_bracket_type: i64,
    pub is_invalid: bool,
}

/// `BracketPairInfo` (e `BracketPairWithMinIndentationInfo`).
#[derive(Clone)]
pub struct BracketPairInfo {
    pub range: Range,
    pub opening_bracket_range: Range,
    pub closing_bracket_range: Option<Range>,
    /// A partir de 0
    pub nesting_level: i64,
    pub nesting_level_of_equal_bracket_type: i64,
    bracket_pair_node: AstNodeRef,
    /// `-1` se não foi pedido; senão o tamanho da menor indentação dentro do par, em colunas visíveis.
    pub min_visible_column_indentation: i64,
}

/// `BracketPairWithMinIndentationInfo`.
pub type BracketPairWithMinIndentationInfo = BracketPairInfo;

impl BracketPairInfo {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        range: Range,
        opening_bracket_range: Range,
        closing_bracket_range: Option<Range>,
        nesting_level: i64,
        nesting_level_of_equal_bracket_type: i64,
        bracket_pair_node: AstNodeRef,
        min_visible_column_indentation: i64,
    ) -> Self {
        Self {
            range,
            opening_bracket_range,
            closing_bracket_range,
            nesting_level,
            nesting_level_of_equal_bracket_type,
            bracket_pair_node,
            min_visible_column_indentation,
        }
    }

    /// `openingBracketInfo`.
    pub fn opening_bracket_info(&self) -> OpeningBracketKind {
        match &*self
            .bracket_pair_node
            .as_pair()
            .expect("é um par")
            .opening()
            .bracket_info
        {
            BracketKind::Opening(info) => info.clone(),
            BracketKind::Closing(_) => unreachable!("a abertura de um par é uma abertura"),
        }
    }

    /// `closingBracketInfo`.
    pub fn closing_bracket_info(&self) -> Option<ClosingBracketKind> {
        self.bracket_pair_node
            .as_pair()
            .expect("é um par")
            .closing()
            .map(|c| match &*c.bracket_info {
                BracketKind::Closing(info) => info.clone(),
                BracketKind::Opening(_) => unreachable!("o fechamento de um par é um fechamento"),
            })
    }
}
