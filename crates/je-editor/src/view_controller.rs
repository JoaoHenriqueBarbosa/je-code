//! Porte de `src/vs/editor/browser/view/viewController.ts`: traduz os gestos do mouse (cliques, arrastos, teclas
//! modificadoras) e os eventos de teclado em comandos de navegação do editor (`CoreNavigationCommands`), e
//! entrega a digitação, a colagem e a composição ao delegado de comandos.
//!
//! Representação:
//! - `IMouseDispatchData` é o [`MouseDispatchData`] e `ICommandDelegate` o trait [`ICommandDelegate`]
//!   (`multicursorText: string[] | null` é `Option<&[JsString]>`, `mode: string | null` é `Option<&str>`, e `type` virou
//!   `type_text`, por ser palavra reservada).
//! - `CoreNavigationCommands.X.runCoreEditorCommand(viewModel, args)` é `core_navigation_commands::instance().x`
//!   com a struct de argumentos do comando; as sete funções que só mudam o comando e usam `_usualArgs` são
//!   [`ViewController::run_usual`], que recebe o comando como seletor (o original repete o mesmo corpo em cada uma).
//! - `IKeyboardEvent` e `IMouseWheelEvent` são os parâmetros de tipo `K` e `W` do [`ViewUserInputEvents`] (ver o
//!   módulo dele).
//!
//! Divergências:
//! - `IEditorConfiguration` não é um campo: as opções (`multiCursorModifier`, `columnSelection`...) são lidas das
//!   opções calculadas do `viewModel`, que são as da mesma configuração.
//! - `paste`, `type`, `compositionType`, `compositionStart`, `compositionEnd`, `cut` e os `emit*` do original só
//!   repassam a chamada ao `commandDelegate` e ao `userInputEvents`, e função de repasse não existe neste repositório:
//!   quem as chama usa [`ViewController::command_delegate`] e [`ViewController::user_input_events`] diretamente.
//! - `Position.with(undefined, column - 1)` é `Position::new(lineNumber, column - 1)`.

use std::rc::Rc;

use je_base::JsString;
use je_base::dynamic::Dynamic;
use je_base::js_string::substring;
use je_base::platform::Platform;
use je_base::strings::contains_rtl;

use crate::code_editor_widget::EditorViewModel;
use crate::config::editor_options::registry::{
    COLUMN_SELECTION, DOUBLE_CLICK_SELECTS_BLOCK, MULTI_CURSOR_MODIFIER, SCROLL_ON_MIDDLE_CLICK, SELECTION_CLIPBOARD,
};
use crate::core_commands::{
    ColumnSelectCommandOptions, CoreEditorCommand, CreateCursorCommandOptions, MoveCommandOptions,
    NavigationCommandRevealType, SetSelectionCommandOptions, core_navigation_commands,
};
use crate::core_commands::core_navigation_commands::CoreNavigationCommands;
use crate::cursor_common::CursorSimpleModel;
use crate::encoded_token_attributes::standard_token_type;
use crate::line_tokens::IViewLineTokens;
use crate::position::Position;
use crate::selection::Selection;
use crate::text_model::TextModel;
use crate::view_model_types::IViewModel;
use crate::view_user_input_events::ViewUserInputEvents;

/// `IMouseDispatchData`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MouseDispatchData {
    pub position: Position,
    /// Desired mouse column (e.g. when position.column gets clamped to text length -- clicking after text on a line).
    pub mouse_column: i32,
    pub reveal_type: NavigationCommandRevealType,
    pub started_on_line_numbers: bool,

    pub in_selection_mode: bool,
    pub mouse_down_count: i32,
    pub alt_key: bool,
    pub ctrl_key: bool,
    pub meta_key: bool,
    pub shift_key: bool,

    pub left_button: bool,
    pub middle_button: bool,
    pub on_injected_text: bool,
}

/// `ICommandDelegate`.
pub trait ICommandDelegate {
    fn paste(&self, text: &JsString, paste_on_new_line: bool, multicursor_text: Option<&[JsString]>, mode: Option<&str>, is_block: bool);
    fn type_text(&self, text: &JsString);
    fn composition_type(&self, text: &JsString, replace_prev_char_cnt: i32, replace_next_char_cnt: i32, position_delta: i32);
    fn start_composition(&self);
    fn end_composition(&self);
    fn cut(&self);
}

/// Um comando de navegação que recebe os `MoveCommandOptions` de `_usualArgs`.
type UsualCommand = Rc<CoreEditorCommand<MoveCommandOptions>>;

/// `ViewController`.
pub struct ViewController<K, W> {
    view_model: Rc<EditorViewModel>,
    user_input_events: Rc<ViewUserInputEvents<K, W>>,
    command_delegate: Rc<dyn ICommandDelegate>,
}

impl<K, W> ViewController<K, W> {
    pub fn new(
        view_model: Rc<EditorViewModel>,
        user_input_events: Rc<ViewUserInputEvents<K, W>>,
        command_delegate: Rc<dyn ICommandDelegate>,
    ) -> Self {
        Self { view_model, user_input_events, command_delegate }
    }

    /// O `commandDelegate` (ver "Divergências" no topo do módulo).
    pub fn command_delegate(&self) -> &dyn ICommandDelegate {
        &*self.command_delegate
    }

    /// O `userInputEvents` (ver "Divergências" no topo do módulo).
    pub fn user_input_events(&self) -> &ViewUserInputEvents<K, W> {
        &self.user_input_events
    }

    pub fn set_selection(&self, model_selection: Selection) {
        core_navigation_commands::instance().set_selection.run_core_editor_command(
            &self.view_model,
            &SetSelectionCommandOptions { source: Some("keyboard".to_string()), selection: Some(model_selection) },
        );
    }

    fn validate_view_column(&self, view_position: Position) -> Position {
        let min_column = self.view_model.get_line_min_column(view_position.line_number);
        if view_position.column < min_column {
            return Position::new(view_position.line_number, min_column);
        }
        view_position
    }

    fn has_multicursor_modifier(&self, data: &MouseDispatchData) -> bool {
        match self.view_model.computed_editor_options().get(MULTI_CURSOR_MODIFIER).as_str() {
            "altKey" => data.alt_key,
            "ctrlKey" => data.ctrl_key,
            "metaKey" => data.meta_key,
            _ => false,
        }
    }

    fn has_non_multicursor_modifier(&self, data: &MouseDispatchData) -> bool {
        match self.view_model.computed_editor_options().get(MULTI_CURSOR_MODIFIER).as_str() {
            "altKey" => data.ctrl_key || data.meta_key,
            "ctrlKey" => data.alt_key || data.meta_key,
            "metaKey" => data.ctrl_key || data.alt_key,
            _ => false,
        }
    }

    /// Selects content inside brackets if the position is right after an opening bracket or right before a closing
    /// bracket. `pos` é a posição no modelo.
    fn try_select_bracket_content(model: &TextModel, pos: Position) -> Option<Selection> {
        // Try to find bracket match if we're right after an opening bracket.
        if pos.column > 1 {
            let pair = model.bracket_pairs().match_bracket(Position::new(pos.line_number, pos.column - 1), None);
            if let Some((opening, closing)) = pair {
                if opening.get_end_position() == pos {
                    return Some(Selection::from_positions(opening.get_end_position(), closing.get_start_position()));
                }
            }
        }

        // Try to find bracket match if we're right before a closing bracket.
        if pos.column <= CursorSimpleModel::get_line_max_column(model, pos.line_number) {
            let pair = model.bracket_pairs().match_bracket(pos, None);
            if let Some((opening, closing)) = pair {
                if closing.get_start_position() == pos {
                    return Some(Selection::from_positions(opening.get_end_position(), closing.get_start_position()));
                }
            }
        }

        None
    }

    /// Selects content inside a string if the position is right after an opening quote or right before a closing
    /// quote. `pos` é a posição no modelo.
    fn try_select_string_content(model: &TextModel, pos: Position) -> Option<Selection> {
        let Position { line_number, column } = pos;
        let tokens = model.tokenization();

        // Ensure we have accurate tokens for the line.
        if !tokens.has_accurate_tokens_for_line(line_number) {
            if tokens.is_cheap_to_tokenize(line_number) {
                tokens.force_tokenization(line_number);
            } else {
                return None;
            }
        }

        // Expand to the contiguous run of string tokens (StandardTokenType.String) around the click position.
        let line_tokens = tokens.get_line_tokens(line_number);
        let mut start_index = line_tokens.find_token_index_at_offset(column - 1);
        let mut end_index = start_index;
        while start_index > 0 && line_tokens.get_standard_token_type(start_index - 1) == standard_token_type::STRING {
            start_index -= 1;
        }
        while end_index + 1 < line_tokens.get_count()
            && line_tokens.get_standard_token_type(end_index + 1) == standard_token_type::STRING
        {
            end_index += 1;
        }

        // Verify the click is after starting or before closing quote.
        let token_start = line_tokens.get_start_offset(start_index);
        let token_end = line_tokens.get_end_offset(end_index);
        if column != token_start + 2 && column != token_end {
            return None;
        }

        // Verify the token looks like a complete quoted string (quote ... quote).
        let line_content = CursorSimpleModel::get_line_content(model, line_number);
        let units = line_content.units();
        // `charAt` devolve a string vazia fora do intervalo: nunca é aspa.
        let char_at = |index: i32| usize::try_from(index).ok().and_then(|index| units.get(index)).copied();
        let first_char = char_at(token_start);
        if !matches!(first_char, Some(0x22 | 0x27 | 0x60)) {
            return None;
        }
        if char_at(token_end - 1) != first_char {
            return None;
        }

        // Skip if string contains RTL characters.
        let content = substring(units, i64::from(token_start) + 1, i64::from(token_end) - 1);
        if contains_rtl(content) {
            return None;
        }

        Some(Selection::new(line_number, token_start + 2, line_number, token_end))
    }

    pub fn dispatch_mouse(&self, data: &MouseDispatchData) {
        let options = self.view_model.computed_editor_options();
        let selection_clipboard_is_on = Platform::detect().is_linux && *options.get(SELECTION_CLIPBOARD);
        let column_selection = *options.get(COLUMN_SELECTION);
        let scroll_on_middle_click = *options.get(SCROLL_ON_MIDDLE_CLICK);
        if data.middle_button && !selection_clipboard_is_on {
            if scroll_on_middle_click {
                // nothing to do here, handled in the contribution
            } else {
                self.column_select(data.position, data.mouse_column, data.in_selection_mode);
            }
        } else if data.started_on_line_numbers {
            // If the dragging started on the gutter, then have operations work on the entire line
            if self.has_multicursor_modifier(data) {
                if data.in_selection_mode {
                    self.run_usual(|c| &c.last_cursor_line_select, data.position, data.reveal_type);
                } else {
                    self.create_cursor(data.position, true);
                }
            } else if data.in_selection_mode {
                self.run_usual(|c| &c.line_select_drag, data.position, data.reveal_type);
            } else {
                self.run_usual(|c| &c.line_select, data.position, data.reveal_type);
            }
        } else if data.mouse_down_count >= 4 {
            self.select_all();
        } else if data.mouse_down_count == 3 {
            if self.has_multicursor_modifier(data) {
                if data.in_selection_mode {
                    self.run_usual(|c| &c.last_cursor_line_select_drag, data.position, data.reveal_type);
                } else {
                    self.run_usual(|c| &c.last_cursor_line_select, data.position, data.reveal_type);
                }
            } else if data.in_selection_mode {
                self.run_usual(|c| &c.line_select_drag, data.position, data.reveal_type);
            } else {
                self.run_usual(|c| &c.line_select, data.position, data.reveal_type);
            }
        } else if data.mouse_down_count == 2 {
            if !data.on_injected_text {
                if self.has_multicursor_modifier(data) {
                    self.run_usual(|c| &c.last_cursor_word_select, data.position, data.reveal_type);
                } else if data.in_selection_mode {
                    self.run_usual(|c| &c.word_select_drag, data.position, data.reveal_type);
                } else {
                    let mut selection: Option<Selection> = None;
                    if *options.get(DOUBLE_CLICK_SELECTS_BLOCK) {
                        let model = self.view_model.model();
                        let model_pos = self.convert_view_to_model_position(data.position);
                        selection = Self::try_select_bracket_content(&model, model_pos)
                            .or_else(|| Self::try_select_string_content(&model, model_pos));
                    }
                    match selection {
                        Some(selection) => self.select(selection),
                        None => self.run_usual(|c| &c.word_select, data.position, data.reveal_type),
                    }
                }
            }
        } else if self.has_multicursor_modifier(data) {
            if !self.has_non_multicursor_modifier(data) {
                if data.shift_key {
                    self.column_select(data.position, data.mouse_column, true);
                } else if data.in_selection_mode {
                    // Do multi-cursor operations only when purely alt is pressed
                    self.run_usual(|c| &c.last_cursor_move_to_select, data.position, data.reveal_type);
                } else {
                    self.create_cursor(data.position, false);
                }
            }
        } else if data.in_selection_mode {
            if data.alt_key || column_selection {
                self.column_select(data.position, data.mouse_column, true);
            } else {
                self.run_usual(|c| &c.move_to_select, data.position, data.reveal_type);
            }
        } else {
            self.move_to(data.position, data.reveal_type);
        }
    }

    /// `_usualArgs(viewPosition, revealType)`.
    fn usual_args(&self, view_position: Position, reveal_type: NavigationCommandRevealType) -> MoveCommandOptions {
        let view_position = self.validate_view_column(view_position);
        MoveCommandOptions {
            source: Some("mouse".to_string()),
            position: Some(self.convert_view_to_model_position(view_position)),
            view_position: Some(view_position),
            reveal_type: Some(reveal_type),
        }
    }

    /// `X.runCoreEditorCommand(this.viewModel, this._usualArgs(viewPosition, revealType))` para o comando que `pick`
    /// escolhe (`_moveToSelect`, `_wordSelect`, `_lineSelect`...).
    fn run_usual(
        &self,
        pick: impl Fn(&CoreNavigationCommands) -> &UsualCommand,
        view_position: Position,
        reveal_type: NavigationCommandRevealType,
    ) {
        let commands = core_navigation_commands::instance();
        pick(&commands).run_core_editor_command(&self.view_model, &self.usual_args(view_position, reveal_type));
    }

    pub fn move_to(&self, view_position: Position, reveal_type: NavigationCommandRevealType) {
        self.run_usual(|c| &c.move_to, view_position, reveal_type);
    }

    fn column_select(&self, view_position: Position, mouse_column: i32, do_column_select: bool) {
        let view_position = self.validate_view_column(view_position);
        core_navigation_commands::instance().column_select.run_core_editor_command(
            &self.view_model,
            &ColumnSelectCommandOptions {
                source: Some("mouse".to_string()),
                position: Some(self.convert_view_to_model_position(view_position)),
                view_position: Some(view_position),
                mouse_column: Some(mouse_column),
                do_column_select,
            },
        );
    }

    fn create_cursor(&self, view_position: Position, whole_line: bool) {
        let view_position = self.validate_view_column(view_position);
        core_navigation_commands::instance().create_cursor.run_core_editor_command(
            &self.view_model,
            &CreateCursorCommandOptions {
                options: MoveCommandOptions {
                    source: Some("mouse".to_string()),
                    position: Some(self.convert_view_to_model_position(view_position)),
                    view_position: Some(view_position),
                    reveal_type: None,
                },
                whole_line,
            },
        );
    }

    fn select(&self, selection: Selection) {
        core_navigation_commands::instance().set_selection.run_core_editor_command(
            &self.view_model,
            &SetSelectionCommandOptions { source: Some("mouse".to_string()), selection: Some(selection) },
        );
    }

    fn select_all(&self) {
        core_navigation_commands::instance()
            .select_all
            .run_core_editor_command(&self.view_model, &Dynamic::object([("source", Dynamic::from("mouse"))]));
    }

    // ----------------------

    fn convert_view_to_model_position(&self, view_position: Position) -> Position {
        self.view_model.coordinates_converter().convert_view_position_to_model_position(view_position)
    }
}
