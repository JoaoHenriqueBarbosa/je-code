//! Porte de `src/vs/editor/common/core/misc/indentation.ts`.

use je_base::JsString;
use je_base::char_code as cc;
use je_base::strings;

use crate::cursor_columns::next_indent_tab_stop;

fn normalize_indentation_from_whitespace(
    str: &[u16],
    indent_size: i32,
    insert_spaces: bool,
) -> JsString {
    let mut spaces_cnt = 0;
    for &char_code in str {
        if char_code == cc::Tab {
            spaces_cnt = next_indent_tab_stop(spaces_cnt, indent_size);
        } else {
            spaces_cnt += 1;
        }
    }

    let mut result = Vec::new();
    if !insert_spaces {
        let tabs_cnt = spaces_cnt / indent_size;
        spaces_cnt %= indent_size;
        result.resize(tabs_cnt as usize, cc::Tab);
    }

    result.resize(result.len() + spaces_cnt as usize, cc::Space);

    JsString::from_units(result)
}

/// `normalizeIndentation`: reescreve a indentação de `str` (os espaços e tabulações do começo) com
/// tabulações ou espaços, conforme `insert_spaces`, e mantém o resto.
pub fn normalize_indentation(str: &[u16], indent_size: i32, insert_spaces: bool) -> JsString {
    let first_non_whitespace_index = strings::first_non_whitespace_index(str).unwrap_or(str.len());
    let mut result = normalize_indentation_from_whitespace(
        &str[..first_non_whitespace_index],
        indent_size,
        insert_spaces,
    );
    result.push_units(&str[first_non_whitespace_index..]);
    result
}
