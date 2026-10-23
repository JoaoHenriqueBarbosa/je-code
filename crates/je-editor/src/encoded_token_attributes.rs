//! Porte de `src/vs/editor/common/encodedTokenAttributes.ts`: os metadados de um token, empacotados em 32 bits.
//!
//! Os `const enum` do original viram constantes `u32`/`i32` e o `TokenMetadata` vira um conjunto de funções
//! associadas, com os mesmos nomes (em snake_case).

/// `LanguageId`.
pub mod language_id {
    pub const NULL: u32 = 0;
    pub const PLAIN_TEXT: u32 = 1;
}

/// `FontStyle`.
pub mod font_style {
    pub const NOT_SET: i32 = -1;
    pub const NONE: u32 = 0;
    pub const ITALIC: u32 = 1;
    pub const BOLD: u32 = 2;
    pub const UNDERLINE: u32 = 4;
    pub const STRIKETHROUGH: u32 = 8;
}

/// `ColorId`.
pub mod color_id {
    pub const NONE: u32 = 0;
    pub const DEFAULT_FOREGROUND: u32 = 1;
    pub const DEFAULT_BACKGROUND: u32 = 2;
}

/// `StandardTokenType`.
pub mod standard_token_type {
    pub const OTHER: u32 = 0;
    pub const COMMENT: u32 = 1;
    pub const STRING: u32 = 2;
    pub const REG_EX: u32 = 3;
}

/// `MetadataConsts`.
pub mod metadata_consts {
    pub const LANGUAGEID_MASK: u32 = 0b0000_0000_0000_0000_0000_0000_1111_1111;
    pub const TOKEN_TYPE_MASK: u32 = 0b0000_0000_0000_0000_0000_0011_0000_0000;
    pub const BALANCED_BRACKETS_MASK: u32 = 0b0000_0000_0000_0000_0000_0100_0000_0000;
    pub const FONT_STYLE_MASK: u32 = 0b0000_0000_0000_0000_0111_1000_0000_0000;
    pub const FOREGROUND_MASK: u32 = 0b0000_0000_1111_1111_1000_0000_0000_0000;
    pub const BACKGROUND_MASK: u32 = 0b1111_1111_0000_0000_0000_0000_0000_0000;

    pub const ITALIC_MASK: u32 = 0b0000_0000_0000_0000_0000_1000_0000_0000;
    pub const BOLD_MASK: u32 = 0b0000_0000_0000_0000_0001_0000_0000_0000;
    pub const UNDERLINE_MASK: u32 = 0b0000_0000_0000_0000_0010_0000_0000_0000;
    pub const STRIKETHROUGH_MASK: u32 = 0b0000_0000_0000_0000_0100_0000_0000_0000;

    pub const SEMANTIC_USE_ITALIC: u32 = 0b0000_0000_0000_0000_0000_0000_0000_0001;
    pub const SEMANTIC_USE_BOLD: u32 = 0b0000_0000_0000_0000_0000_0000_0000_0010;
    pub const SEMANTIC_USE_UNDERLINE: u32 = 0b0000_0000_0000_0000_0000_0000_0000_0100;
    pub const SEMANTIC_USE_STRIKETHROUGH: u32 = 0b0000_0000_0000_0000_0000_0000_0000_1000;
    pub const SEMANTIC_USE_FOREGROUND: u32 = 0b0000_0000_0000_0000_0000_0000_0001_0000;
    pub const SEMANTIC_USE_BACKGROUND: u32 = 0b0000_0000_0000_0000_0000_0000_0010_0000;

    pub const LANGUAGEID_OFFSET: u32 = 0;
    pub const TOKEN_TYPE_OFFSET: u32 = 8;
    pub const BALANCED_BRACKETS_OFFSET: u32 = 10;
    pub const FONT_STYLE_OFFSET: u32 = 11;
    pub const FOREGROUND_OFFSET: u32 = 15;
    pub const BACKGROUND_OFFSET: u32 = 24;
}

use metadata_consts as mc;

/// `ITokenPresentation`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TokenPresentation {
    pub foreground: u32,
    pub italic: bool,
    pub bold: bool,
    pub underline: bool,
    pub strikethrough: bool,
}

/// `TokenMetadata`: leitores dos campos do metadado de um token.
pub struct TokenMetadata;

impl TokenMetadata {
    pub fn get_language_id(metadata: u32) -> u32 {
        (metadata & mc::LANGUAGEID_MASK) >> mc::LANGUAGEID_OFFSET
    }

    pub fn get_token_type(metadata: u32) -> u32 {
        (metadata & mc::TOKEN_TYPE_MASK) >> mc::TOKEN_TYPE_OFFSET
    }

    pub fn contains_balanced_brackets(metadata: u32) -> bool {
        (metadata & mc::BALANCED_BRACKETS_MASK) != 0
    }

    pub fn get_font_style(metadata: u32) -> u32 {
        (metadata & mc::FONT_STYLE_MASK) >> mc::FONT_STYLE_OFFSET
    }

    pub fn get_foreground(metadata: u32) -> u32 {
        (metadata & mc::FOREGROUND_MASK) >> mc::FOREGROUND_OFFSET
    }

    pub fn get_background(metadata: u32) -> u32 {
        (metadata & mc::BACKGROUND_MASK) >> mc::BACKGROUND_OFFSET
    }

    pub fn get_class_name_from_metadata(metadata: u32) -> String {
        let foreground = Self::get_foreground(metadata);
        let mut class_name = format!("mtk{foreground}");
        let style = Self::get_font_style(metadata);
        if style & font_style::ITALIC != 0 {
            class_name.push_str(" mtki");
        }
        if style & font_style::BOLD != 0 {
            class_name.push_str(" mtkb");
        }
        if style & font_style::UNDERLINE != 0 {
            class_name.push_str(" mtku");
        }
        if style & font_style::STRIKETHROUGH != 0 {
            class_name.push_str(" mtks");
        }
        class_name
    }

    pub fn get_inline_style_from_metadata(metadata: u32, color_map: &[String]) -> String {
        let foreground = Self::get_foreground(metadata) as usize;
        let style = Self::get_font_style(metadata);
        let mut result = format!("color: {};", color_map.get(foreground).map_or("undefined", String::as_str));
        if style & font_style::ITALIC != 0 {
            result.push_str("font-style: italic;");
        }
        if style & font_style::BOLD != 0 {
            result.push_str("font-weight: bold;");
        }
        let mut text_decoration = String::new();
        if style & font_style::UNDERLINE != 0 {
            text_decoration.push_str(" underline");
        }
        if style & font_style::STRIKETHROUGH != 0 {
            text_decoration.push_str(" line-through");
        }
        if !text_decoration.is_empty() {
            result.push_str(&format!("text-decoration:{text_decoration};"));
        }
        result
    }

    pub fn get_presentation_from_metadata(metadata: u32) -> TokenPresentation {
        let style = Self::get_font_style(metadata);
        TokenPresentation {
            foreground: Self::get_foreground(metadata),
            italic: style & font_style::ITALIC != 0,
            bold: style & font_style::BOLD != 0,
            underline: style & font_style::UNDERLINE != 0,
            strikethrough: style & font_style::STRIKETHROUGH != 0,
        }
    }
}
