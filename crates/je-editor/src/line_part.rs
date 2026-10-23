//! Porte de `src/vs/editor/common/viewLayout/linePart.ts`.

/// `LinePartMetadata` (enum constante no original): bits do `metadata` de um [`LinePart`].
pub mod line_part_metadata {
    pub const IS_WHITESPACE: i32 = 1;
    pub const PSEUDO_BEFORE: i32 = 2;
    pub const PSEUDO_AFTER: i32 = 4;

    pub const IS_WHITESPACE_MASK: i32 = 0b001;
    pub const PSEUDO_BEFORE_MASK: i32 = 0b010;
    pub const PSEUDO_AFTER_MASK: i32 = 0b100;
}

/// Um trecho de uma linha com o mesmo tipo de token.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinePart {
    /// Índice do último caractere deste token (exclusivo).
    pub end_index: i32,
    pub r#type: String,
    pub metadata: i32,
    pub contains_rtl: bool,
}

impl LinePart {
    pub fn new(end_index: i32, r#type: impl Into<String>, metadata: i32, contains_rtl: bool) -> Self {
        Self {
            end_index,
            r#type: r#type.into(),
            metadata,
            contains_rtl,
        }
    }

    pub fn is_whitespace(&self) -> bool {
        self.metadata & line_part_metadata::IS_WHITESPACE_MASK != 0
    }

    pub fn is_pseudo_after(&self) -> bool {
        self.metadata & line_part_metadata::PSEUDO_AFTER_MASK != 0
    }
}
