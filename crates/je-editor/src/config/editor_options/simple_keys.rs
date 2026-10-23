//! Chaves tipadas das opções simples do registro (`editorOptions.ts`, `EditorOptions`, linhas 6120 a 6950)
//! que não têm arquivo próprio: o computador de `layoutInfo` as lê, e o registro as reutiliza.
//!
//! Os valores seguem o tipo que o registro do original usa:
//!
//! - `EditorBooleanOption`: `glyphMargin`, `folding`, `scrollBeyondLastLine`, `wordWrapIndicator` (`Opt<bool>`).
//! - `EditorIntOption`: `lineNumbersMinChars` e `wordWrapColumn` (`Opt<f64>`, o `number` do JavaScript).
//! - `EditorStringEnumOption` com união de strings: `wordWrap`, `wordWrapOverride1`, `wordWrapOverride2` e
//!   `showFoldingControls`, que aqui viram `enum` (via `string_enum!`). O registro precisa usar
//!   `EditorEnumOption<...>` com esses tipos (e não `EditorStringEnumOption`, cujo valor é `String`),
//!   senão o `get` da chave tipada entra em pânico.

use crate::config::editor_options::{EditorOption, Opt};

string_enum! {
    /// `'off' | 'on' | 'wordWrapColumn' | 'bounded'`.
    pub enum WordWrap {
        Off => "off",
        On => "on",
        WordWrapColumn => "wordWrapColumn",
        Bounded => "bounded",
    }
}

string_enum! {
    /// `'off' | 'on' | 'inherit'` (`wordWrapOverride1` e `wordWrapOverride2`).
    pub enum WordWrapOverride {
        Off => "off",
        On => "on",
        Inherit => "inherit",
    }
}

string_enum! {
    /// `'always' | 'never' | 'mouseover'`.
    pub enum ShowFoldingControls {
        Always => "always",
        Never => "never",
        Mouseover => "mouseover",
    }
}

/// Chave tipada de `glyphMargin` (padrão `true`).
pub const GLYPH_MARGIN: Opt<bool> = Opt::new(EditorOption::GlyphMargin);

/// Chave tipada de `folding` (padrão `true`).
pub const FOLDING: Opt<bool> = Opt::new(EditorOption::Folding);

/// Chave tipada de `scrollBeyondLastLine` (padrão `true`).
pub const SCROLL_BEYOND_LAST_LINE: Opt<bool> = Opt::new(EditorOption::ScrollBeyondLastLine);

/// Chave tipada de `wordWrapIndicator` (padrão `false`).
pub const WORD_WRAP_INDICATOR: Opt<bool> = Opt::new(EditorOption::WordWrapIndicator);

/// Chave tipada de `lineNumbersMinChars` (inteiro de 1 a 300, padrão 5).
pub const LINE_NUMBERS_MIN_CHARS: Opt<f64> = Opt::new(EditorOption::LineNumbersMinChars);

/// Chave tipada de `wordWrapColumn` (inteiro de 1 a `MAX_SAFE_SMALL_INTEGER`, padrão 80).
pub const WORD_WRAP_COLUMN: Opt<f64> = Opt::new(EditorOption::WordWrapColumn);

/// Chave tipada de `wordWrap` (padrão `off`).
pub const WORD_WRAP: Opt<WordWrap> = Opt::new(EditorOption::WordWrap);

/// Chave tipada de `wordWrapOverride1` (padrão `inherit`).
pub const WORD_WRAP_OVERRIDE_1: Opt<WordWrapOverride> = Opt::new(EditorOption::WordWrapOverride1);

/// Chave tipada de `wordWrapOverride2` (padrão `inherit`).
pub const WORD_WRAP_OVERRIDE_2: Opt<WordWrapOverride> = Opt::new(EditorOption::WordWrapOverride2);

/// Chave tipada de `showFoldingControls` (padrão `mouseover`).
pub const SHOW_FOLDING_CONTROLS: Opt<ShowFoldingControls> = Opt::new(EditorOption::ShowFoldingControls);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_point_at_their_options() {
        assert_eq!(GLYPH_MARGIN.id(), EditorOption::GlyphMargin);
        assert_eq!(FOLDING.id(), EditorOption::Folding);
        assert_eq!(SCROLL_BEYOND_LAST_LINE.id(), EditorOption::ScrollBeyondLastLine);
        assert_eq!(WORD_WRAP_INDICATOR.id(), EditorOption::WordWrapIndicator);
        assert_eq!(LINE_NUMBERS_MIN_CHARS.id(), EditorOption::LineNumbersMinChars);
        assert_eq!(WORD_WRAP_COLUMN.id(), EditorOption::WordWrapColumn);
        assert_eq!(WORD_WRAP.id(), EditorOption::WordWrap);
        assert_eq!(WORD_WRAP_OVERRIDE_1.id(), EditorOption::WordWrapOverride1);
        assert_eq!(WORD_WRAP_OVERRIDE_2.id(), EditorOption::WordWrapOverride2);
        assert_eq!(SHOW_FOLDING_CONTROLS.id(), EditorOption::ShowFoldingControls);
    }

    #[test]
    fn enums_follow_the_original_lists() {
        assert_eq!(WordWrap::ALLOWED, ["off", "on", "wordWrapColumn", "bounded"]);
        assert_eq!(WordWrapOverride::ALLOWED, ["off", "on", "inherit"]);
        assert_eq!(ShowFoldingControls::ALLOWED, ["always", "never", "mouseover"]);
        assert_eq!(WordWrap::from_text("bounded"), Some(WordWrap::Bounded));
        assert_eq!(WordWrapOverride::from_text("x"), None);
    }
}
