//! Porte de `src/vs/editor/common/model/utils.ts`.

use je_base::char_code;

/// `computeIndentLevel(line, tabSize)`. Devolve:
///  - -1 => a linha só tem espaço em branco
///  - senão => o nível de indentação
pub fn compute_indent_level(line: &[u16], tab_size: i32) -> i32 {
    let mut indent = 0;
    let mut i = 0;
    let len = line.len();

    while i < len {
        let ch_code = line[i];
        if ch_code == char_code::Space {
            indent += 1;
        } else if ch_code == char_code::Tab {
            indent = indent - indent % tab_size + tab_size;
        } else {
            break;
        }
        i += 1;
    }

    if i == len {
        return -1; // a linha só tem espaço em branco
    }

    indent
}
