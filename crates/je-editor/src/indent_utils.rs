//! Porte de `src/vs/editor/contrib/indentation/common/indentUtils.ts`: contagem de espaços de uma
//! indentação (`getSpaceCnt`) e geração de uma indentação com a quantidade de espaços pedida
//! (`generateIndent`).

use je_base::JsString;

const TAB: u16 = 9;
const SPACE: u16 = 32;

/// `getSpaceCnt(str, tabSize)`: cada tab vale `tab_size` espaços.
pub fn get_space_cnt(str: &[u16], tab_size: i32) -> i32 {
    str.iter().map(|&unit| if unit == TAB { tab_size } else { 1 }).sum()
}

/// `generateIndent(spacesCnt, tabSize, insertSpaces)`.
pub fn generate_indent(spaces_cnt: i32, tab_size: i32, insert_spaces: bool) -> JsString {
    let mut spaces_cnt = spaces_cnt.max(0);

    let mut result = JsString::new();
    if !insert_spaces {
        let tabs_cnt = spaces_cnt.div_euclid(tab_size);
        spaces_cnt = spaces_cnt.rem_euclid(tab_size);
        for _ in 0..tabs_cnt {
            result.push_unit(TAB);
        }
    }

    for _ in 0..spaces_cnt {
        result.push_unit(SPACE);
    }

    result
}
