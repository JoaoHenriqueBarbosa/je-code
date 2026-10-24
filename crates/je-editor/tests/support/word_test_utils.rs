//! Porte de `src/vs/editor/contrib/wordOperations/test/browser/wordTestUtils.ts`: `deserializePipePositions`,
//! `serializePipePositions` e `testRepeatedActionAndExtractPositions`.
//!
//! Os textos são lidos em unidades UTF-16 (o `charAt` e o `length` do original), então a coluna de uma posição conta
//! unidades UTF-16 como a do editor. `withTestCodeEditor` é o de [`super::test_code_editor`] e o `ITestCodeEditor`
//! é o [`TestCodeEditor`].

use je_editor::code_editor_widget::ICodeEditor;
use je_editor::position::Position;

use super::test_code_editor::{ITestCodeEditor, TestCodeEditorInstantiationOptions, with_test_code_editor};

const PIPE: u16 = b'|' as u16;
const LINE_FEED: u16 = b'\n' as u16;

/// `deserializePipePositions(text)`: tira os `|` do texto e devolve o texto e as posições onde estavam.
pub fn deserialize_pipe_positions(text: &str) -> (String, Vec<Position>) {
    let mut result_text: Vec<u16> = Vec::new();
    let mut line_number = 1;
    let mut char_index = 0;
    let mut positions = Vec::new();
    for unit in text.encode_utf16() {
        if unit == LINE_FEED {
            result_text.push(unit);
            line_number += 1;
            char_index = 0;
            continue;
        }
        if unit == PIPE {
            positions.push(Position::new(line_number, char_index + 1));
        } else {
            result_text.push(unit);
            char_index += 1;
        }
    }
    (String::from_utf16_lossy(&result_text), positions)
}

/// `serializePipePositions(text, positions)`: o inverso, e ordena `positions` como o original.
pub fn serialize_pipe_positions(text: &str, mut positions: Vec<Position>) -> String {
    positions.sort_by(|a, b| Position::compare(a, b).cmp(&0));
    let mut positions = positions.into_iter().peekable();
    let mut result_text: Vec<u16> = Vec::new();
    let mut line_number = 1;
    let mut char_index = 0;
    for unit in text.encode_utf16() {
        if positions.next_if(|position| position.line_number == line_number && position.column == char_index + 1).is_some() {
            result_text.push(PIPE);
        }
        result_text.push(unit);
        if unit == LINE_FEED {
            line_number += 1;
            char_index = 0;
        } else {
            char_index += 1;
        }
    }
    if positions.next_if(|position| position.line_number == line_number && position.column == char_index + 1).is_some() {
        result_text.push(PIPE);
    }
    assert!(positions.peek().is_none(), "Unexpected left over positions!!!");
    String::from_utf16_lossy(&result_text)
}

/// `testRepeatedActionAndExtractPositions(text, initialPosition, action, record, stopCondition, options)`.
pub fn test_repeated_action_and_extract_positions(
    text: &str,
    initial_position: Position,
    action: impl Fn(&ITestCodeEditor),
    record: impl Fn(&ITestCodeEditor) -> Position,
    stop_condition: impl Fn(&ITestCodeEditor) -> bool,
    options: TestCodeEditorInstantiationOptions,
) -> Vec<Position> {
    let mut actual_stops: Vec<Position> = Vec::new();
    with_test_code_editor(text, options, |editor, _, _| {
        editor.set_position(initial_position, None);
        let editor: &ITestCodeEditor = editor;
        loop {
            action(editor);
            actual_stops.push(record(editor));
            if stop_condition(editor) {
                break;
            }

            if actual_stops.len() > 1000 {
                panic!("Endless loop detected involving position {}!", editor.get_position().expect("o editor tem o modelo"));
            }
        }
    });
    actual_stops
}
