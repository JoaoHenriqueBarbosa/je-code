//! Porte de `src/vs/platform/userDataSync/common/content.ts`: edição de texto JSON e deslocamentos de
//! linha. Deslocamentos em unidades UTF-16, como no original.

use je_base::errors::{self, Error};
use je_base::json::Value;
use je_base::json_edit::{EditSegment, apply_edit, set_property};
use je_base::json_formatter::FormattingOptions;

/// `edit`: aplica ao texto a primeira edição que o `setProperty` calcula (`value` `None` é `undefined`,
/// remover). Se o `setProperty` lançar, o erro sobe como no original.
pub fn edit(content: &str, original_path: &[EditSegment], value: Option<Value>, formatting_options: &FormattingOptions) -> String {
    let text: Vec<u16> = content.encode_utf16().collect();
    let edits = match set_property(&text, original_path, value, formatting_options, None) {
        Ok(edits) => edits,
        Err(e) => errors::throw(Error::new(&e.to_string())),
    };
    match edits.first() {
        Some(edit) => String::from_utf16_lossy(&apply_edit(&text, edit)),
        None => content.to_string(),
    }
}

/// `content.charAt(index)`: `None` fora do texto (a string vazia do original).
fn char_at(content: &[u16], index: i64) -> Option<u16> {
    usize::try_from(index).ok().and_then(|i| content.get(i)).copied()
}

/// `getLineStartOffset`.
pub fn get_line_start_offset(content: &[u16], eol: &[u16], at_offset: usize) -> usize {
    let last = eol.last().copied();
    let mut line_starting_offset = at_offset as i64;
    while line_starting_offset >= 0 {
        if char_at(content, line_starting_offset) == last && eol.len() == 1 {
            return (line_starting_offset + 1) as usize;
        }
        line_starting_offset -= 1;
        if eol.len() == 2 && line_starting_offset >= 0 && char_at(content, line_starting_offset) == Some(eol[0]) {
            return (line_starting_offset + 2) as usize;
        }
    }
    0
}

/// `getLineEndOffset`. Se nenhuma quebra de linha aparece até o fim do texto, devolve o último índice
/// (o laço do original não terminaria nesse caso).
pub fn get_line_end_offset(content: &[u16], eol: &[u16], at_offset: usize) -> usize {
    let last = eol.last().copied();
    let mut line_end_offset = at_offset as i64;
    while line_end_offset <= content.len() as i64 {
        if char_at(content, line_end_offset) == last && eol.len() == 1 {
            return line_end_offset as usize;
        }
        line_end_offset += 1;
        if eol.len() == 2 && char_at(content, line_end_offset) == Some(eol[1]) {
            return line_end_offset as usize;
        }
    }
    content.len().saturating_sub(1)
}
