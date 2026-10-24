//! Porte de `src/vs/workbench/contrib/preferences/common/smartSnippetInserter.ts`: descobre onde inserir um
//! snippet de atalho (ou de configuração) num arquivo JSON com um array no topo (`keybindings.json`), e o que
//! acrescentar antes e depois para o texto continuar válido.
//!
//! Notas de tradução:
//! - `ITextModel` é o [`TextModel`]; `getLineCount`, `getLineMaxColumn` vêm do trait [`CursorSimpleModel`], pelo
//!   qual o modelo atende o `ICursorSimpleModel` (ver `text_model.rs`).
//! - O `prepend` e o `append` do resultado são sempre um dos literais `''`, `','`, `'\n['` ou `']'`, então são
//!   `&'static str`.
//! - O `enum State` local do `insertSnippet` é [`State`]; o `checkRangeStatus`, que fecha sobre `scanner`,
//!   `arrayLevel`, `objLevel` e as variáveis de estado, vira [`RangeTracker`], que recebe o deslocamento do
//!   token do scanner por argumento. `lastValidPos = -1` é `None`.
//! - Os deslocamentos são em unidades UTF-16, como no scanner e no `getValueLengthInRange` do original.

use je_base::json::{Scanner, SyntaxKind};
use je_editor::cursor_common::CursorSimpleModel;
use je_editor::position::Position;
use je_editor::range::Range;
use je_editor::text_model::TextModel;

/// `InsertSnippetResult`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InsertSnippetResult {
    pub position: Position,
    pub prepend: &'static str,
    pub append: &'static str,
}

/// `<INVALID> [ <BEFORE_OBJECT> { <INVALID> } <AFTER_OBJECT>, <BEFORE_OBJECT> { <INVALID> } <AFTER_OBJECT> ] <INVALID>`
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Invalid,
    AfterObject,
    BeforeObject,
}

/// As variáveis que o `checkRangeStatus` do original fecha: profundidade de array e de objeto, o estado atual
/// e a última posição válida.
struct RangeTracker {
    array_level: i32,
    obj_level: i32,
    current_state: State,
    last_valid_pos: Option<usize>,
    last_valid_state: State,
}

impl RangeTracker {
    fn new() -> Self {
        Self { array_level: 0, obj_level: 0, current_state: State::Invalid, last_valid_pos: None, last_valid_state: State::Invalid }
    }

    /// `checkRangeStatus(pos, state)`; `token_offset` é o `scanner.getTokenOffset()` do momento.
    fn check_range_status(&mut self, pos: usize, state: State, token_offset: usize) {
        if state != State::Invalid && self.array_level == 1 && self.obj_level == 0 {
            self.current_state = state;
            self.last_valid_pos = Some(pos);
            self.last_valid_state = state;
        } else if self.current_state != State::Invalid {
            self.current_state = State::Invalid;
            self.last_valid_pos = Some(token_offset);
        }
    }
}

pub struct SmartSnippetInserter;

impl SmartSnippetInserter {
    fn has_open_brace(scanner: &mut Scanner<'_>) -> bool {
        while scanner.scan() != SyntaxKind::EOF {
            let kind = scanner.token();

            if kind == SyntaxKind::OpenBraceToken {
                return true;
            }
        }

        false
    }

    fn offset_to_position(model: &TextModel, offset: usize) -> Position {
        let mut offset_before_line = 0;
        let eol_length = model.get_eol().len();
        let line_count = model.get_line_count();
        for line_number in 1..=line_count {
            let line_total_length = model.get_line_length(line_number) + eol_length;
            let offset_after_line = offset_before_line + line_total_length;

            if offset_after_line > offset {
                return Position::new(line_number, (offset - offset_before_line + 1) as i32);
            }
            offset_before_line = offset_after_line;
        }
        Position::new(line_count, model.get_line_max_column(line_count))
    }

    pub fn insert_snippet(model: &TextModel, position: Position) -> InsertSnippetResult {
        let desired_position = model.get_value_length_in_range(&Range::new(1, 1, position.line_number, position.column));

        let value = model.get_value();
        let mut scanner = Scanner::new(value.units(), false);
        let mut tracker = RangeTracker::new();

        while scanner.scan() != SyntaxKind::EOF {
            let current_pos = scanner.position();
            let kind = scanner.token();

            let mut good_kind = false;
            match kind {
                SyntaxKind::OpenBracketToken => {
                    good_kind = true;
                    tracker.array_level += 1;
                    tracker.check_range_status(current_pos, State::BeforeObject, scanner.token_offset());
                }
                SyntaxKind::CloseBracketToken => {
                    good_kind = true;
                    tracker.array_level -= 1;
                    tracker.check_range_status(current_pos, State::Invalid, scanner.token_offset());
                }
                SyntaxKind::CommaToken => {
                    good_kind = true;
                    tracker.check_range_status(current_pos, State::BeforeObject, scanner.token_offset());
                }
                SyntaxKind::OpenBraceToken => {
                    good_kind = true;
                    tracker.obj_level += 1;
                    tracker.check_range_status(current_pos, State::Invalid, scanner.token_offset());
                }
                SyntaxKind::CloseBraceToken => {
                    good_kind = true;
                    tracker.obj_level -= 1;
                    tracker.check_range_status(current_pos, State::AfterObject, scanner.token_offset());
                }
                SyntaxKind::Trivia | SyntaxKind::LineBreakTrivia => good_kind = true,
                _ => {}
            }

            if current_pos >= desired_position && (tracker.current_state != State::Invalid || tracker.last_valid_pos.is_some()) {
                let (accept_position, accept_state) = match (tracker.current_state, tracker.last_valid_pos) {
                    (State::Invalid, Some(last_valid_pos)) => (last_valid_pos, tracker.last_valid_state),
                    (current_state, _) => (if good_kind { current_pos } else { scanner.token_offset() }, current_state),
                };

                if accept_state == State::AfterObject {
                    return InsertSnippetResult { position: Self::offset_to_position(model, accept_position), prepend: ",", append: "" };
                }
                scanner.set_position(accept_position);
                return InsertSnippetResult {
                    position: Self::offset_to_position(model, accept_position),
                    prepend: "",
                    append: if Self::has_open_brace(&mut scanner) { "," } else { "" },
                };
            }
        }

        // no valid position found!
        let model_line_count = model.get_line_count();
        InsertSnippetResult {
            position: Position::new(model_line_count, model.get_line_max_column(model_line_count)),
            prepend: "\n[",
            append: "]",
        }
    }
}
