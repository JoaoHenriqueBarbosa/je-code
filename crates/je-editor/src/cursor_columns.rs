//! Porte de `src/vs/editor/common/core/cursorColumns.ts`.
//!
//! Uma coluna de uma posição é o vão entre dois caracteres vizinhos. As funções daqui trabalham com a
//! "coluna visível", uma aproximação grosseira da posição horizontal na tela. Com tabulação de 4:
//! ```txt
//! |<TAB>|<TAB>|T|ext
//! |     |     | \---- coluna = 4, coluna visível = 9
//! |     |     \------ coluna = 3, coluna visível = 8
//! |     \------------ coluna = 2, coluna visível = 4
//! \------------------ coluna = 1, coluna visível = 0
//! ```
//!
//! Colunas visíveis não funcionam bem com texto RTL, fonte de largura variável ou caracteres largos
//! demais, e valem tanto no modelo quanto no modelo de visão.
//!
//! A classe estática `CursorColumns` do original vira funções livres; `nextIndentTabStop` e
//! `prevIndentTabStop`, que no original só repassam para as versões `Render`, são reexportações delas.

use je_base::char_code as cc;
use je_base::strings::{self, CodePointIterator, GraphemeIterator};

const TAB: u32 = cc::Tab as u32;

fn next_visible_column(code_point: u32, visible_column: i32, tab_size: i32) -> i32 {
    if code_point == TAB {
        return next_render_tab_stop(visible_column, tab_size);
    }
    // `isFullWidthCharacter` recebe o code point; acima de 0xFFFF ele nunca é largo.
    let full_width = u16::try_from(code_point).is_ok_and(strings::is_full_width_character);
    if full_width || strings::is_emoji_imprecise(code_point) {
        return visible_column + 2;
    }
    visible_column + 1
}

/// `substring(0, column - 1)` com o fim limitado ao texto (coluna menor que 1 dá o texto vazio).
fn text_before_column(line_content: &[u16], column: i32) -> &[u16] {
    let text_len = usize::try_from(column - 1)
        .unwrap_or(0)
        .min(line_content.len());
    &line_content[..text_len]
}

/// `visibleColumnFromColumn`: a coluna visível de uma coluna.
pub fn visible_column_from_column(line_content: &[u16], column: i32, tab_size: i32) -> i32 {
    let text = text_before_column(line_content, column);
    let mut iterator = GraphemeIterator::new(text, 0);

    let mut result = 0;
    while !iterator.eol() {
        let code_point = strings::get_next_code_point(text, text.len() as i64, iterator.offset());
        iterator.next_grapheme_length();

        result = next_visible_column(code_point, result, tab_size);
    }

    result
}

/// `toStatusbarColumn`: o valor mostrado como "Col" na barra de status.
pub fn to_statusbar_column(line_content: &[u16], column: i32, tab_size: i32) -> i32 {
    let text = text_before_column(line_content, column);
    let mut iterator = CodePointIterator::new(text, 0);

    let mut result = 0;
    while !iterator.eol() {
        let code_point = iterator.next_code_point();

        if code_point == TAB {
            result = next_render_tab_stop(result, tab_size);
        } else {
            result += 1;
        }
    }

    result + 1
}

/// `columnFromVisibleColumn`: a coluna mais próxima de uma coluna visível.
pub fn column_from_visible_column(line_content: &[u16], visible_column: i32, tab_size: i32) -> i32 {
    if visible_column <= 0 {
        return 1;
    }

    let line_content_length = line_content.len() as i64;
    let mut iterator = GraphemeIterator::new(line_content, 0);

    let mut before_visible_column = 0;
    let mut before_column = 1;
    while !iterator.eol() {
        let code_point =
            strings::get_next_code_point(line_content, line_content_length, iterator.offset());
        iterator.next_grapheme_length();

        let after_visible_column = next_visible_column(code_point, before_visible_column, tab_size);
        let after_column = iterator.offset() as i32 + 1;

        if after_visible_column >= visible_column {
            let before_delta = visible_column - before_visible_column;
            let after_delta = after_visible_column - visible_column;
            return if after_delta < before_delta {
                after_column
            } else {
                before_column
            };
        }

        before_visible_column = after_visible_column;
        before_column = after_column;
    }

    // Percorreu o texto inteiro.
    line_content_length as i32 + 1
}

/// `nextRenderTabStop`. Trabalha com colunas a partir de 0 (e não de 1, como as colunas comuns).
pub fn next_render_tab_stop(visible_column: i32, tab_size: i32) -> i32 {
    visible_column + tab_size - visible_column % tab_size
}

/// `prevRenderTabStop`. Trabalha com colunas a partir de 0 (e não de 1, como as colunas comuns).
pub fn prev_render_tab_stop(column: i32, tab_size: i32) -> i32 {
    0.max(column - 1 - (column - 1) % tab_size)
}

/// `nextIndentTabStop`: o mesmo cálculo de [`next_render_tab_stop`], com o tamanho da indentação.
pub use next_render_tab_stop as next_indent_tab_stop;

/// `prevIndentTabStop`: o mesmo cálculo de [`prev_render_tab_stop`], com o tamanho da indentação.
pub use prev_render_tab_stop as prev_indent_tab_stop;
