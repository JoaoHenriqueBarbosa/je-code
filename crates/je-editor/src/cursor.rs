//! Porte de `src/vs/editor/common/cursor/cursor.ts`: o controlador de cursores do editor
//! ([`CursorsController`]), o executor de comandos ([`CommandExecutor`]) e o que eles usam (`CursorModelState`,
//! `AutoClosedAction`, `CompositionState`).
//!
//! Decisões de tradução:
//!
//! - O controlador é compartilhado e reentrante no original: o modelo dispara eventos de dentro de
//!   `pushEditOperations` e o view model chama `onModelContentChanged` de volta (é para isso que existem
//!   `_isHandling` e a ordem de `_validateAutoClosedActions`). Por isso os métodos têm `&self` e o estado fica em
//!   `Cell`/`RefCell`, como no resto do crate. Nenhum empréstimo de `RefCell` atravessa uma chamada ao modelo ou
//!   a um callback.
//! - `ICursorSimpleModel` é `Rc<dyn CursorSimpleModel>`, `ICoordinatesConverter` é `Rc<dyn ICoordinatesConverter>`
//!   e `ITextModel` é o `Rc<TextModel>`. `CursorContext` não é `Clone`: o controlador guarda um
//!   `Rc<CursorContext>` (o campo público `context` do original, lido por [`CursorsController::context`]) e entrega
//!   à coleção de cursores um contexto próprio montado dos mesmos `Rc`.
//! - `string | null | undefined` de `source` é `Option<&str>`; `source || 'keyboard'` trata a cadeia vazia como
//!   ausente. Textos digitados e colados são `&[u16]` (UTF-16), como as operações de edição. Parâmetros com valor
//!   padrão são obrigatórios (`cursorChangeReason`, `isBlock`, `editReason` de `executeCommands`).
//! - `type` é `r#type`. `executeCommand`/`executeCommands` recebem os comandos por valor (`Box<dyn Command>`).
//! - `states: PartialCursorState[] | null` de `setStates` é `Option<Vec<PartialCursorState>>`.
//! - `InternalModelContentChangeEvent | ModelInjectedTextChangedEvent` é o `ContentOrInjectedTextChangeEvent` de
//!   `view_model_types.rs` (a escolha do ramo é o `instanceof` do original).
//! - `editorCommon.ICursorState` não tem porte em `editor_common.rs`: [`SavedCursorState`] traz os mesmos campos,
//!   com `Option<Position>` para as propriedades que o original tolera ausentes ("Avoid missing properties on the
//!   literal"). Um número de linha ou de coluna zero é falso no original e também aqui.
//! - `command instanceof BaseTypeWithAutoClosingCommand` é `Command::as_type_with_auto_closing` (de
//!   `editor_common.rs`, implementado pelos dois comandos que estendem a base).
//! - `EditSources.cursor({ kind, detailedSource })` é `EditSources::cursor(kind, detailed_source)`.
//! - `Disposable` é o [`IDisposable`] do `je-base`: `dispose` solta os cursores e as ações de fechamento
//!   automático.
//! - `try { ... } catch (err) { onUnexpectedError(err) }` é `errors::catch` e `errors::on_unexpected_error`;
//!   `try { ... } finally { _isHandling = false }` de `onModelContentChanged` é o guarda [`HandlingGuard`];
//!   `console.warn` vai para `stderr`.
//! - `ICommandData` e `ICommandsData` do original têm o mesmo formato e viram uma struct só ([`CommandsData`]).
//!   Os `loserCursorsMap` (`{ [index: string]: boolean }`) são `BTreeSet<i32>`.
//! - `ICursorStateComputer` recebe as operações inversas e devolve `Option<Vec<Selection>>`, como em `model.rs`.

use std::cell::{Cell, RefCell};
use std::collections::BTreeSet;
use std::rc::Rc;

use je_base::JsString;
use je_base::char_code as cc;
use je_base::errors;
use je_base::js_string::substring;
use je_base::lifecycle::IDisposable;
use je_base::strings::{common_prefix_length, common_suffix_length, next_char_length};

use crate::coordinates_converter::ICoordinatesConverter;
use crate::cursor_collection::CursorCollection;
use crate::cursor_common::{
    ColumnSelectData, CursorConfiguration, CursorSimpleModel, CursorState, EditOperationResult,
    EditOperationResultOptions, EditOperationType, PartialCursorState,
};
use crate::cursor_context::CursorContext;
use crate::cursor_delete_operations::DeleteOperations;
use crate::cursor_events::CursorChangeReason;
use crate::cursor_type_edit_operations::{CompositionOperation, PasteOperation, TypeWithoutInterceptorsOperation};
use crate::cursor_type_operations::{CompositionOutcome, TypeOperations};
use crate::edit_operation::SingleEditOperation;
use crate::editor_common::{Command, CursorStateComputerData, EditOperationBuilder, ScrollType};
use crate::model::{
    ModelDecorationOptionsInit, ModelDeltaDecoration, SingleEditOperationIdentifier, TrackedRangeStickiness,
    ValidEditOperation,
};
use crate::position::Position;
use crate::range::Range;
use crate::selection::{Selection, SelectionDirection};
use crate::text_model::{ModelDecorationOptions, TextModel};
use crate::text_model_edit_source::{EditSources, TextModelEditSource};
use crate::text_model_events::RawContentChangedType;
use crate::view_events::{ViewCursorStateChangedEvent, ViewEvent, ViewRevealRangeRequestEvent};
use crate::view_model_event_dispatcher::{CursorStateChangedEvent, OutgoingViewModelEvent, ViewModelEventsCollector};
use crate::view_model_types::{ContentOrInjectedTextChangeEvent, VerticalRevealType};

/// `editorCommon.ICursorState`: o estado de um cursor, para salvar e restaurar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SavedCursorState {
    pub in_selection_mode: bool,
    pub selection_start: Option<Position>,
    pub position: Option<Position>,
}

/// O `finally { this._isHandling = false }` de `onModelContentChanged`: zera a flag ao sair, por qualquer caminho.
struct HandlingGuard<'a>(&'a Cell<bool>);

impl Drop for HandlingGuard<'_> {
    fn drop(&mut self) {
        self.0.set(false);
    }
}

/// `CursorState.fromModelSelections(selections)` já como `PartialCursorState[]`.
fn model_cursor_states(selections: &[Selection]) -> Vec<PartialCursorState> {
    CursorState::from_model_selections(selections).into_iter().map(PartialCursorState::Model).collect()
}

/// Os estados completos dos cursores como `PartialCursorState[]`.
fn full_cursor_states(states: Vec<CursorState>) -> Vec<PartialCursorState> {
    states.into_iter().map(PartialCursorState::Full).collect()
}

/// Um dos caracteres `)]}>'"` e a crase, os de fechamento do regex de `_findAutoClosingPairs`.
fn is_auto_closing_close_char(unit: u16) -> bool {
    matches!(u8::try_from(unit), Ok(b')' | b']' | b'}' | b'>' | b'\'' | b'"' | b'`'))
}

/// `haystack.lastIndexOf(needle, fromIndex)`: o último índice `<= fromIndex` (com piso 0) em que `needle` começa.
fn last_index_of_from(haystack: &[u16], needle: &[u16], from_index: i64) -> Option<usize> {
    if needle.len() > haystack.len() {
        return None;
    }
    let start = usize::try_from(from_index.max(0)).unwrap_or(0).min(haystack.len() - needle.len());
    (0..=start).rev().find(|&index| &haystack[index..index + needle.len()] == needle)
}

pub struct CursorsController {
    model: Rc<TextModel>,
    known_model_version_id: Cell<i64>,
    view_model: Rc<dyn CursorSimpleModel>,
    coordinates_converter: Rc<dyn ICoordinatesConverter>,
    context: RefCell<Rc<CursorContext>>,
    cursors: RefCell<CursorCollection>,

    has_focus: Cell<bool>,
    is_handling: Cell<bool>,
    composition_state: RefCell<Option<CompositionState>>,
    column_select_data: Cell<Option<ColumnSelectData>>,
    auto_closed_actions: RefCell<Vec<AutoClosedAction>>,
    prev_edit_operation_type: Cell<EditOperationType>,
}

impl CursorsController {
    pub fn new(
        model: Rc<TextModel>,
        view_model: Rc<dyn CursorSimpleModel>,
        coordinates_converter: Rc<dyn ICoordinatesConverter>,
        cursor_config: Rc<CursorConfiguration>,
    ) -> Self {
        let new_context = || {
            CursorContext::new(
                model.clone(),
                view_model.clone(),
                coordinates_converter.clone(),
                cursor_config.clone(),
            )
        };
        let context = Rc::new(new_context());
        let cursors = CursorCollection::new(new_context());
        Self {
            known_model_version_id: Cell::new(model.get_version_id()),
            model: model.clone(),
            view_model: view_model.clone(),
            coordinates_converter: coordinates_converter.clone(),
            context: RefCell::new(context),
            cursors: RefCell::new(cursors),

            has_focus: Cell::new(false),
            is_handling: Cell::new(false),
            composition_state: RefCell::new(None),
            column_select_data: Cell::new(None),
            auto_closed_actions: RefCell::new(Vec::new()),
            prev_edit_operation_type: Cell::new(EditOperationType::Other),
        }
    }

    /// `context`.
    pub fn context(&self) -> Rc<CursorContext> {
        self.context.borrow().clone()
    }

    /// Um contexto novo sobre os mesmos objetos do controlador (o `CursorContext` não é `Clone`).
    fn new_context(&self, cursor_config: Rc<CursorConfiguration>) -> CursorContext {
        CursorContext::new(
            self.model.clone(),
            self.view_model.clone(),
            self.coordinates_converter.clone(),
            cursor_config,
        )
    }

    pub fn update_configuration(&self, cursor_config: Rc<CursorConfiguration>) {
        *self.context.borrow_mut() = Rc::new(self.new_context(cursor_config.clone()));
        let context = self.new_context(cursor_config);
        self.cursors.borrow_mut().update_context(context);
    }

    pub fn on_line_mapping_changed(&self, events_collector: &ViewModelEventsCollector) {
        if self.known_model_version_id.get() != self.model.get_version_id() {
            // Há eventos do modelo que ainda não recebi.
            //
            // Isso pode acontecer ao editar o modelo, quando o view model recebe os eventos de mudança primeiro
            // e emite os eventos de mapeamento de linhas, tudo antes de o cursor ter a chance de se recuperar
            // dos marcadores.
            //
            // O ouvinte de mudança do modelo será chamado logo e garantirá um estado válido do cursor lá.
            return;
        }
        // Garante um estado válido
        self.set_states(
            events_collector,
            Some("viewModel"),
            CursorChangeReason::NotSet,
            Some(full_cursor_states(self.get_cursor_states())),
        );
    }

    pub fn set_has_focus(&self, has_focus: bool) {
        self.has_focus.set(has_focus);
    }

    fn validate_auto_closed_actions(&self) {
        if self.auto_closed_actions.borrow().is_empty() {
            return;
        }
        let mut selections: Vec<Range> = self.cursors.borrow().get_selections().iter().map(|s| **s).collect();
        let actions = std::mem::take(&mut *self.auto_closed_actions.borrow_mut());
        let (valid_actions, invalid_actions): (Vec<AutoClosedAction>, Vec<AutoClosedAction>) =
            actions.into_iter().partition(|auto_closed_action| auto_closed_action.is_valid(&mut selections));
        // A liberação dispara eventos de decoração, cujos ouvintes podem reentrar no cursor.
        *self.auto_closed_actions.borrow_mut() = valid_actions;
        for mut auto_closed_action in invalid_actions {
            auto_closed_action.dispose();
        }
    }

    // ------ some getters/setters

    pub fn get_primary_cursor_state(&self) -> CursorState {
        self.cursors.borrow().get_primary_cursor()
    }

    pub fn get_last_added_cursor_index(&self) -> usize {
        self.cursors.borrow().get_last_added_cursor_index()
    }

    pub fn get_cursor_states(&self) -> Vec<CursorState> {
        self.cursors.borrow().get_all()
    }

    pub fn set_states(
        &self,
        events_collector: &ViewModelEventsCollector,
        source: Option<&str>,
        reason: CursorChangeReason,
        states: Option<Vec<PartialCursorState>>,
    ) -> bool {
        let mut states = states;
        let mut reached_max_cursor_count = false;
        let multi_cursor_limit = self.context().cursor_config.multi_cursor_limit;
        if let Some(states) = states.as_mut()
            && states.len() as f64 > multi_cursor_limit
        {
            states.truncate(multi_cursor_limit as usize);
            reached_max_cursor_count = true;
        }

        let old_state = CursorModelState::from(&self.model, self);

        {
            let mut cursors = self.cursors.borrow_mut();
            cursors.set_states(states.as_deref());
            cursors.normalize();
        }
        self.column_select_data.set(None);

        self.validate_auto_closed_actions();

        self.emit_state_changed_if_necessary(events_collector, source, reason, Some(old_state), reached_max_cursor_count)
    }

    pub fn set_cursor_column_select_data(&self, column_select_data: ColumnSelectData) {
        self.column_select_data.set(Some(column_select_data));
    }

    pub fn reveal_all(
        &self,
        events_collector: &ViewModelEventsCollector,
        source: Option<&str>,
        minimal_reveal: bool,
        vertical_type: VerticalRevealType,
        reveal_horizontal: bool,
        scroll_type: ScrollType,
    ) {
        let (reveal_view_range, reveal_view_selections) = {
            let cursors = self.cursors.borrow();
            let view_positions = cursors.get_view_positions();
            if view_positions.len() > 1 {
                (None, Some(cursors.get_view_selections()))
            } else {
                (Some(Range::from_positions(view_positions[0], view_positions[0])), None)
            }
        };

        events_collector.emit_view_event(ViewEvent::RevealRangeRequest(ViewRevealRangeRequestEvent::new(
            source.map(str::to_owned),
            minimal_reveal,
            reveal_view_range,
            reveal_view_selections,
            vertical_type,
            reveal_horizontal,
            scroll_type,
        )));
    }

    pub fn reveal_primary(
        &self,
        events_collector: &ViewModelEventsCollector,
        source: Option<&str>,
        minimal_reveal: bool,
        vertical_type: VerticalRevealType,
        reveal_horizontal: bool,
        scroll_type: ScrollType,
    ) {
        let primary_cursor = self.cursors.borrow().get_primary_cursor();
        let reveal_view_selections = vec![primary_cursor.view_state.selection];
        events_collector.emit_view_event(ViewEvent::RevealRangeRequest(ViewRevealRangeRequestEvent::new(
            source.map(str::to_owned),
            minimal_reveal,
            None,
            Some(reveal_view_selections),
            vertical_type,
            reveal_horizontal,
            scroll_type,
        )));
    }

    pub fn save_state(&self) -> Vec<SavedCursorState> {
        self.cursors
            .borrow()
            .get_selections()
            .iter()
            .map(|selection| SavedCursorState {
                in_selection_mode: !selection.is_empty(),
                selection_start: Some(Position::new(
                    selection.selection_start_line_number,
                    selection.selection_start_column,
                )),
                position: Some(Position::new(selection.position_line_number, selection.position_column)),
            })
            .collect()
    }

    pub fn restore_state(&self, events_collector: &ViewModelEventsCollector, states: &[SavedCursorState]) {
        let mut desired_selections: Vec<Selection> = Vec::with_capacity(states.len());

        for state in states {
            let mut position_line_number = 1;
            let mut position_column = 1;

            // Evita propriedades ausentes no literal
            if let Some(position) = state.position {
                if position.line_number != 0 {
                    position_line_number = position.line_number;
                }
                if position.column != 0 {
                    position_column = position.column;
                }
            }

            let mut selection_start_line_number = position_line_number;
            let mut selection_start_column = position_column;

            // Evita propriedades ausentes no literal
            if let Some(selection_start) = state.selection_start {
                if selection_start.line_number != 0 {
                    selection_start_line_number = selection_start.line_number;
                }
                if selection_start.column != 0 {
                    selection_start_column = selection_start.column;
                }
            }

            desired_selections.push(Selection::new(
                selection_start_line_number,
                selection_start_column,
                position_line_number,
                position_column,
            ));
        }

        self.set_states(
            events_collector,
            Some("restoreState"),
            CursorChangeReason::NotSet,
            Some(model_cursor_states(&desired_selections)),
        );
        self.reveal_all(
            events_collector,
            Some("restoreState"),
            false,
            VerticalRevealType::Simple,
            true,
            ScrollType::Immediate,
        );
    }

    pub fn on_model_content_changed(
        &self,
        events_collector: &ViewModelEventsCollector,
        event: &ContentOrInjectedTextChangeEvent,
    ) {
        match event {
            ContentOrInjectedTextChangeEvent::InjectedText(_) => {
                // Se os textos injetados mudam, as posições na visão de todos os cursores precisam ser atualizadas.
                if self.is_handling.get() {
                    // As posições na visão serão atualizadas quando o tratamento terminar
                    return;
                }
                // `setStates` pode remover marcadores, o que pode disparar uma mudança de decoração. Se há
                // decorações de texto injetado para aquela linha, `onModelContentChanged` é emitido de novo e
                // acontece uma recursão infinita. `_isHandling` impede isso.
                self.is_handling.set(true);
                let _reset = HandlingGuard(&self.is_handling);
                self.set_states(
                    events_collector,
                    Some("modelChange"),
                    CursorChangeReason::NotSet,
                    Some(full_cursor_states(self.get_cursor_states())),
                );
            }
            ContentOrInjectedTextChangeEvent::Content(event) => {
                let e = &event.raw_content_changed_event;
                self.known_model_version_id.set(e.version_id);
                if self.is_handling.get() {
                    return;
                }

                let had_flush_event = e.contains_event(RawContentChangedType::Flush);
                self.prev_edit_operation_type.set(EditOperationType::Other);

                if had_flush_event {
                    // um `model.setValue()` foi chamado
                    {
                        let mut cursors = self.cursors.borrow_mut();
                        cursors.dispose();
                        *cursors = CursorCollection::new(self.new_context(self.context().cursor_config.clone()));
                    }
                    self.validate_auto_closed_actions();
                    self.emit_state_changed_if_necessary(
                        events_collector,
                        Some("model"),
                        CursorChangeReason::ContentFlush,
                        None,
                        false,
                    );
                } else {
                    match e.resulting_selection.as_deref().filter(|selection| self.has_focus.get() && !selection.is_empty()) {
                        Some(resulting_selection) => {
                            let cursor_state = model_cursor_states(resulting_selection);
                            let reason = if e.is_undoing {
                                CursorChangeReason::Undo
                            } else if e.is_redoing {
                                CursorChangeReason::Redo
                            } else {
                                CursorChangeReason::RecoverFromMarkers
                            };
                            if self.set_states(events_collector, Some("modelChange"), reason, Some(cursor_state)) {
                                self.reveal_all(
                                    events_collector,
                                    Some("modelChange"),
                                    false,
                                    VerticalRevealType::Simple,
                                    true,
                                    ScrollType::Smooth,
                                );
                            }
                        }
                        None => {
                            let selections_from_markers = self.cursors.borrow().read_selection_from_markers();
                            self.set_states(
                                events_collector,
                                Some("modelChange"),
                                CursorChangeReason::RecoverFromMarkers,
                                Some(model_cursor_states(&selections_from_markers)),
                            );
                        }
                    }
                }
            }
        }
    }

    pub fn get_selection(&self) -> Selection {
        self.cursors.borrow().get_primary_cursor().model_state.selection
    }

    pub fn get_top_most_view_position(&self) -> Position {
        self.cursors.borrow().get_top_most_view_position()
    }

    pub fn get_bottom_most_view_position(&self) -> Position {
        self.cursors.borrow().get_bottom_most_view_position()
    }

    pub fn get_cursor_column_select_data(&self) -> ColumnSelectData {
        if let Some(column_select_data) = self.column_select_data.get() {
            return column_select_data;
        }
        let primary_cursor = self.cursors.borrow().get_primary_cursor();
        let view_selection_start = primary_cursor.view_state.selection_start.get_start_position();
        let view_position = primary_cursor.view_state.position;
        let cursor_config = self.context().cursor_config.clone();
        ColumnSelectData {
            is_real: false,
            from_view_line_number: view_selection_start.line_number,
            from_view_visual_column: cursor_config.visible_column_from_column(&*self.view_model, view_selection_start),
            to_view_line_number: view_position.line_number,
            to_view_visual_column: cursor_config.visible_column_from_column(&*self.view_model, view_position),
        }
    }

    pub fn get_selections(&self) -> Vec<Selection> {
        self.cursors.borrow().get_selections()
    }

    pub fn get_position(&self) -> Position {
        self.cursors.borrow().get_primary_cursor().model_state.position
    }

    pub fn set_selections(
        &self,
        events_collector: &ViewModelEventsCollector,
        source: Option<&str>,
        selections: &[Selection],
        reason: CursorChangeReason,
    ) {
        self.set_states(events_collector, source, reason, Some(model_cursor_states(selections)));
    }

    pub fn get_prev_edit_operation_type(&self) -> EditOperationType {
        self.prev_edit_operation_type.get()
    }

    pub fn set_prev_edit_operation_type(&self, r#type: EditOperationType) {
        self.prev_edit_operation_type.set(r#type);
    }

    // ------ auxiliary handling logic

    fn push_auto_closed_action(&self, auto_closed_characters_ranges: &[Range], auto_closed_enclosing_ranges: &[Range]) {
        let decoration = |range: &Range, options: ModelDecorationOptionsInit| ModelDeltaDecoration {
            range: *range,
            options: ModelDecorationOptions::create_dynamic(&options),
        };
        let auto_closed_characters_delta_decorations: Vec<ModelDeltaDecoration> = auto_closed_characters_ranges
            .iter()
            .map(|range| {
                decoration(
                    range,
                    ModelDecorationOptionsInit {
                        description: "auto-closed-character".to_owned(),
                        inline_class_name: Some("auto-closed-character".to_owned()),
                        stickiness: Some(TrackedRangeStickiness::NeverGrowsWhenTypingAtEdges),
                        ..Default::default()
                    },
                )
            })
            .collect();
        let auto_closed_enclosing_delta_decorations: Vec<ModelDeltaDecoration> = auto_closed_enclosing_ranges
            .iter()
            .map(|range| {
                decoration(
                    range,
                    ModelDecorationOptionsInit {
                        description: "auto-closed-enclosing".to_owned(),
                        stickiness: Some(TrackedRangeStickiness::NeverGrowsWhenTypingAtEdges),
                        ..Default::default()
                    },
                )
            })
            .collect();

        let auto_closed_characters_decorations =
            self.model.delta_decorations(&[], &auto_closed_characters_delta_decorations, 0);
        let auto_closed_enclosing_decorations =
            self.model.delta_decorations(&[], &auto_closed_enclosing_delta_decorations, 0);
        self.auto_closed_actions.borrow_mut().push(AutoClosedAction::new(
            self.model.clone(),
            auto_closed_characters_decorations,
            auto_closed_enclosing_decorations,
        ));
    }

    fn execute_edit_operation(&self, op_result: Option<EditOperationResult>, edit_reason: &TextModelEditSource) {
        let Some(mut op_result) = op_result else {
            // Nada a executar
            return;
        };

        if op_result.should_push_stack_element_before {
            self.model.push_stack_element();
        }

        let selections = self.get_selections();
        let result = CommandExecutor::execute_commands(&self.model, &selections, &mut op_result.commands, edit_reason);
        let applied = result.is_some();
        if let Some(result) = result {
            // Os comandos foram aplicados corretamente
            self.interpret_command_result(Some(result));

            // Confere os caracteres fechados automaticamente
            let mut auto_closed_characters_ranges: Vec<Range> = Vec::new();
            let mut auto_closed_enclosing_ranges: Vec<Range> = Vec::new();

            for command in op_result.commands.iter().flatten() {
                if let Some(base) = command.as_type_with_auto_closing()
                    && let (Some(enclosing_range), Some(close_character_range)) =
                        (base.enclosing_range, base.close_character_range)
                {
                    auto_closed_characters_ranges.push(close_character_range);
                    auto_closed_enclosing_ranges.push(enclosing_range);
                }
            }

            if !auto_closed_characters_ranges.is_empty() {
                self.push_auto_closed_action(&auto_closed_characters_ranges, &auto_closed_enclosing_ranges);
            }

            self.prev_edit_operation_type.set(op_result.r#type);
        }

        if op_result.should_push_stack_element_after {
            self.model.push_stack_element();
        }

        if applied && let Some(follow_up) = &op_result.follow_up {
            self.execute_edit_operation(follow_up(&self.get_selections()), edit_reason);
        }
    }

    fn interpret_command_result(&self, cursor_state: Option<Vec<Selection>>) {
        let cursor_state = match cursor_state {
            Some(cursor_state) if !cursor_state.is_empty() => cursor_state,
            _ => self.cursors.borrow().read_selection_from_markers(),
        };

        self.column_select_data.set(None);
        let mut cursors = self.cursors.borrow_mut();
        cursors.set_selections(&cursor_state);
        cursors.normalize();
    }

    // -----------------------------------------------------------------------------------------------------------
    // ----- emitting events

    fn emit_state_changed_if_necessary(
        &self,
        events_collector: &ViewModelEventsCollector,
        source: Option<&str>,
        reason: CursorChangeReason,
        old_state: Option<CursorModelState>,
        reached_max_cursor_count: bool,
    ) -> bool {
        let new_state = CursorModelState::from(&self.model, self);
        if new_state.equals(old_state.as_ref()) {
            return false;
        }

        let (selections, view_selections) = {
            let cursors = self.cursors.borrow();
            (cursors.get_selections(), cursors.get_view_selections())
        };

        // Deixa a visão receber o evento primeiro.
        events_collector.emit_view_event(ViewEvent::CursorStateChanged(ViewCursorStateChangedEvent::new(
            view_selections,
            selections.clone(),
            reason,
        )));

        // Só depois de a visão ser avisada, o resto do mundo fica sabendo...
        let model_state_changed = match &old_state {
            None => true,
            Some(old_state) => {
                old_state.cursor_state.len() != new_state.cursor_state.len()
                    || new_state
                        .cursor_state
                        .iter()
                        .zip(&old_state.cursor_state)
                        .any(|(new_cursor_state, old_cursor_state)| {
                            !new_cursor_state.model_state.equals(&old_cursor_state.model_state)
                        })
            }
        };
        if model_state_changed {
            let old_selections: Option<Vec<Selection>> = old_state
                .as_ref()
                .map(|old_state| old_state.cursor_state.iter().map(|s| s.model_state.selection).collect());
            let old_model_version_id = old_state.as_ref().map_or(0, |old_state| old_state.model_version_id);
            events_collector.emit_outgoing_event(OutgoingViewModelEvent::CursorStateChanged(
                CursorStateChangedEvent::new(
                    old_selections,
                    selections,
                    old_model_version_id,
                    new_state.model_version_id,
                    source.filter(|source| !source.is_empty()).unwrap_or("keyboard").to_owned(),
                    reason,
                    reached_max_cursor_count,
                ),
            ));
        }

        true
    }

    // -----------------------------------------------------------------------------------------------------------
    // ----- handlers beyond this point

    /// `_findAutoClosingPairs`: para cada edição, os índices de abertura e de fechamento do par dentro do texto
    /// dela, ou `None` se alguma edição não é um par de fechamento automático.
    fn find_auto_closing_pairs(&self, edits: &[SingleEditOperation]) -> Option<Vec<(usize, usize)>> {
        if edits.is_empty() {
            return None;
        }

        let context = self.context();
        let close_single_char = &context.cursor_config.auto_closing_pairs.auto_closing_pairs_close_single_char;
        let mut indices: Vec<(usize, usize)> = Vec::with_capacity(edits.len());
        for edit in edits {
            let text = edit.text.as_ref().filter(|text| !text.is_empty())?;
            let units = text.units();
            if units.contains(&cc::LineFeed) {
                return None;
            }

            // `/([)\]}>'"`])([^)\]}>'"`]*)$/`: o último caractere de fechamento do texto, com só texto sem
            // caractere de fechamento depois dele.
            let close_char_index = units.iter().rposition(|&unit| is_auto_closing_close_char(unit))?;
            let close_char = &units[close_char_index..=close_char_index];

            let auto_closing_pairs_candidates = close_single_char.get(close_char)?;
            if auto_closing_pairs_candidates.len() != 1 {
                return None;
            }

            let open_char: Vec<u16> = auto_closing_pairs_candidates[0].open.encode_utf16().collect();
            let open_char_index = last_index_of_from(units, &open_char, close_char_index as i64 - 1)?;

            indices.push((open_char_index, close_char_index));
        }

        Some(indices)
    }

    pub fn execute_edits(
        &self,
        events_collector: &ViewModelEventsCollector,
        source: Option<&str>,
        edits: Vec<SingleEditOperation>,
        cursor_state_computer: &mut dyn FnMut(&[ValidEditOperation]) -> Option<Vec<Selection>>,
        reason: TextModelEditSource,
    ) {
        let mut edits = edits;
        let mut auto_closing_indices: Option<Vec<(usize, usize)>> = None;
        if source == Some("snippet") {
            auto_closing_indices = self.find_auto_closing_pairs(&edits);
        }

        if auto_closing_indices.is_some() {
            edits[0].is_tracked = true;
        }
        let mut auto_closed_characters_ranges: Vec<Range> = Vec::new();
        let mut auto_closed_enclosing_ranges: Vec<Range> = Vec::new();
        let selections = {
            let mut computer = |undo_edits: &[ValidEditOperation]| -> Option<Vec<Selection>> {
                if let Some(auto_closing_indices) = &auto_closing_indices {
                    for (i, &(open_char_inner_index, close_char_inner_index)) in auto_closing_indices.iter().enumerate()
                    {
                        let undo_edit = &undo_edits[i];
                        let line_number = undo_edit.range.start_line_number;
                        let open_char_index = undo_edit.range.start_column - 1 + open_char_inner_index as i32;
                        let close_char_index = undo_edit.range.start_column - 1 + close_char_inner_index as i32;

                        auto_closed_characters_ranges.push(Range::new(
                            line_number,
                            close_char_index + 1,
                            line_number,
                            close_char_index + 2,
                        ));
                        auto_closed_enclosing_ranges.push(Range::new(
                            line_number,
                            open_char_index + 1,
                            line_number,
                            close_char_index + 2,
                        ));
                    }
                }
                let selections = cursor_state_computer(undo_edits);
                if selections.is_some() {
                    // Não recupera a seleção a partir dos marcadores porque sabemos qual ela deve ser.
                    self.is_handling.set(true);
                }

                selections
            };
            self.model.push_edit_operations(Some(self.get_selections()), &edits, Some(&mut computer), None, Some(reason))
        };
        if let Some(selections) = selections {
            self.is_handling.set(false);
            self.set_selections(events_collector, source, &selections, CursorChangeReason::NotSet);
        }
        if !auto_closed_characters_ranges.is_empty() {
            self.push_auto_closed_action(&auto_closed_characters_ranges, &auto_closed_enclosing_ranges);
        }
    }

    fn execute_edit(
        &self,
        callback: impl FnOnce(),
        events_collector: &ViewModelEventsCollector,
        source: Option<&str>,
        cursor_change_reason: CursorChangeReason,
    ) {
        if self.context().cursor_config.read_only {
            // não dá para editar em modo somente leitura...
            return;
        }

        let old_state = CursorModelState::from(&self.model, self);
        self.cursors.borrow_mut().stop_tracking_selections();
        self.is_handling.set(true);

        let outcome = errors::catch(|| {
            self.cursors.borrow_mut().ensure_valid_state();
            callback();
        });
        if let Err(err) = outcome {
            errors::on_unexpected_error(&err);
        }

        self.is_handling.set(false);
        self.cursors.borrow_mut().start_tracking_selections();
        self.validate_auto_closed_actions();
        if self.emit_state_changed_if_necessary(events_collector, source, cursor_change_reason, Some(old_state), false) {
            self.reveal_all(events_collector, source, false, VerticalRevealType::Simple, true, ScrollType::Smooth);
        }
    }

    pub fn get_auto_closed_characters(&self) -> Vec<Range> {
        AutoClosedAction::get_all_auto_closed_characters(&self.auto_closed_actions.borrow())
    }

    pub fn start_composition(&self, _events_collector: &ViewModelEventsCollector) {
        *self.composition_state.borrow_mut() = Some(CompositionState::new(&self.model, &self.get_selections()));
    }

    pub fn end_composition(&self, events_collector: &ViewModelEventsCollector, source: Option<&str>) {
        let reason = EditSources::cursor("compositionEnd", source);

        let composition_outcome = self
            .composition_state
            .borrow()
            .as_ref()
            .and_then(|composition_state| composition_state.deduce_outcome(&self.model, &self.get_selections()));
        *self.composition_state.borrow_mut() = None;

        self.execute_edit(
            || {
                if source == Some("keyboard") {
                    // a composição terminou, vê se é preciso completar automaticamente
                    let context = self.context();
                    self.execute_edit_operation(
                        TypeOperations::composition_end_with_interceptors(
                            self.prev_edit_operation_type.get(),
                            &context.cursor_config,
                            &self.model,
                            composition_outcome.as_deref(),
                            &self.get_selections(),
                            &self.get_auto_closed_characters(),
                        ),
                        &reason,
                    );
                }
            },
            events_collector,
            source,
            CursorChangeReason::NotSet,
        );
    }

    pub fn r#type(&self, events_collector: &ViewModelEventsCollector, text: &[u16], source: Option<&str>) {
        let reason = EditSources::cursor("type", source);

        self.execute_edit(
            || {
                let context = self.context();
                if source == Some("keyboard") {
                    // Se o evento vem direto do teclado, procura caracteres elétricos e enter

                    let len = text.len();
                    let mut offset = 0;
                    while offset < len {
                        let char_length = next_char_length(text, offset);
                        let chr = &text[offset..(offset + char_length).min(len)];

                        // Aqui é preciso interpretar cada caractere digitado individualmente
                        let is_doing_composition = self.composition_state.borrow().is_some();
                        self.execute_edit_operation(
                            Some(TypeOperations::type_with_interceptors(
                                is_doing_composition,
                                self.prev_edit_operation_type.get(),
                                &context.cursor_config,
                                &self.model,
                                &self.get_selections(),
                                &self.get_auto_closed_characters(),
                                chr,
                            )),
                            &reason,
                        );

                        offset += char_length;
                    }
                } else {
                    self.execute_edit_operation(
                        Some(TypeWithoutInterceptorsOperation::get_edits(
                            self.prev_edit_operation_type.get(),
                            &self.get_selections(),
                            text,
                        )),
                        &reason,
                    );
                }
            },
            events_collector,
            source,
            CursorChangeReason::NotSet,
        );
    }

    pub fn composition_type(
        &self,
        events_collector: &ViewModelEventsCollector,
        text: &[u16],
        replace_prev_char_cnt: i32,
        replace_next_char_cnt: i32,
        position_delta: i32,
        source: Option<&str>,
    ) {
        let reason = EditSources::cursor("compositionType", source);

        if text.is_empty() && replace_prev_char_cnt == 0 && replace_next_char_cnt == 0 {
            // esta edição não faz nada
            if position_delta != 0 {
                // mas ainda quer mover o cursor
                let new_selections: Vec<Selection> = self
                    .get_selections()
                    .iter()
                    .map(|selection| {
                        let position = selection.get_position();
                        Selection::new(
                            position.line_number,
                            position.column + position_delta,
                            position.line_number,
                            position.column + position_delta,
                        )
                    })
                    .collect();
                self.set_selections(events_collector, source, &new_selections, CursorChangeReason::NotSet);
            }
            return;
        }
        self.execute_edit(
            || {
                let context = self.context();
                self.execute_edit_operation(
                    Some(CompositionOperation::get_edits(
                        self.prev_edit_operation_type.get(),
                        &context.cursor_config,
                        &self.model,
                        &self.get_selections(),
                        text,
                        replace_prev_char_cnt,
                        replace_next_char_cnt,
                        position_delta,
                    )),
                    &reason,
                );
            },
            events_collector,
            source,
            CursorChangeReason::NotSet,
        );
    }

    pub fn paste(
        &self,
        events_collector: &ViewModelEventsCollector,
        text: &[u16],
        paste_on_new_line: bool,
        multicursor_text: Option<&[JsString]>,
        source: Option<&str>,
        is_block: bool,
    ) {
        let reason = EditSources::cursor("paste", source);

        self.execute_edit(
            || {
                let context = self.context();
                self.execute_edit_operation(
                    Some(PasteOperation::get_edits(
                        &context.cursor_config,
                        &*self.model,
                        &self.get_selections(),
                        text,
                        paste_on_new_line,
                        multicursor_text.unwrap_or(&[]),
                        is_block,
                    )),
                    &reason,
                );
            },
            events_collector,
            source,
            CursorChangeReason::Paste,
        );
    }

    pub fn cut(&self, events_collector: &ViewModelEventsCollector, source: Option<&str>) {
        let reason = EditSources::cursor("cut", source);
        self.execute_edit(
            || {
                let context = self.context();
                let mut selections = self.get_selections();
                self.execute_edit_operation(
                    Some(DeleteOperations::cut(&context.cursor_config, &*self.model, &mut selections)),
                    &reason,
                );
            },
            events_collector,
            source,
            CursorChangeReason::NotSet,
        );
    }

    pub fn execute_command(
        &self,
        events_collector: &ViewModelEventsCollector,
        command: Box<dyn Command>,
        source: Option<&str>,
    ) {
        let reason = EditSources::cursor("executeCommand", source);

        self.execute_edit(
            || {
                self.cursors.borrow_mut().kill_secondary_cursors();

                self.execute_edit_operation(Some(Self::commands_result(vec![command])), &reason);
            },
            events_collector,
            source,
            CursorChangeReason::NotSet,
        );
    }

    pub fn execute_commands(
        &self,
        events_collector: &ViewModelEventsCollector,
        commands: Vec<Box<dyn Command>>,
        source: Option<&str>,
    ) {
        let reason = EditSources::cursor("executeCommands", source);

        self.execute_edit(
            || {
                self.execute_edit_operation(Some(Self::commands_result(commands)), &reason);
            },
            events_collector,
            source,
            CursorChangeReason::NotSet,
        );
    }

    /// `new EditOperationResult(EditOperationType.Other, commands, { shouldPushStackElementBefore: false,
    /// shouldPushStackElementAfter: false })`, o resultado de `executeCommand` e de `executeCommands`.
    fn commands_result(commands: Vec<Box<dyn Command>>) -> EditOperationResult {
        EditOperationResult::new(
            EditOperationType::Other,
            commands.into_iter().map(Some).collect(),
            EditOperationResultOptions {
                should_push_stack_element_before: false,
                should_push_stack_element_after: false,
                follow_up: None,
            },
        )
    }
}

impl IDisposable for CursorsController {
    fn dispose(&self) {
        self.cursors.borrow_mut().dispose();
        let auto_closed_actions = std::mem::take(&mut *self.auto_closed_actions.borrow_mut());
        for mut auto_closed_action in auto_closed_actions {
            auto_closed_action.dispose();
        }
    }
}

/// Um retrato do cursor e do modelo.
struct CursorModelState {
    model_version_id: i64,
    cursor_state: Vec<CursorState>,
}

impl CursorModelState {
    fn from(model: &TextModel, cursor: &CursorsController) -> Self {
        Self { model_version_id: model.get_version_id(), cursor_state: cursor.get_cursor_states() }
    }

    fn equals(&self, other: Option<&CursorModelState>) -> bool {
        let Some(other) = other else {
            return false;
        };
        self.model_version_id == other.model_version_id
            && self.cursor_state.len() == other.cursor_state.len()
            && self.cursor_state.iter().zip(&other.cursor_state).all(|(a, b)| a.equals(b))
    }
}

struct AutoClosedAction {
    model: Rc<TextModel>,

    auto_closed_characters_decorations: Vec<String>,
    auto_closed_enclosing_decorations: Vec<String>,
}

impl AutoClosedAction {
    fn get_all_auto_closed_characters(auto_closed_actions: &[AutoClosedAction]) -> Vec<Range> {
        auto_closed_actions
            .iter()
            .flat_map(AutoClosedAction::get_auto_closed_characters_ranges)
            .collect()
    }

    fn new(
        model: Rc<TextModel>,
        auto_closed_characters_decorations: Vec<String>,
        auto_closed_enclosing_decorations: Vec<String>,
    ) -> Self {
        Self { model, auto_closed_characters_decorations, auto_closed_enclosing_decorations }
    }

    fn dispose(&mut self) {
        let model = self.model.clone();
        let characters_decorations = &mut self.auto_closed_characters_decorations;
        let enclosing_decorations = &mut self.auto_closed_enclosing_decorations;
        model.change_decorations(
            |accessor| {
                *characters_decorations = accessor.delta_decorations(characters_decorations, &[]);
                *enclosing_decorations = accessor.delta_decorations(enclosing_decorations, &[]);
            },
            0,
        );
    }

    fn get_auto_closed_characters_ranges(&self) -> Vec<Range> {
        self.auto_closed_characters_decorations
            .iter()
            .filter_map(|decoration| self.model.get_decoration_range(decoration))
            .collect()
    }

    /// `isValid(selections)`: ordena `selections` no lugar, como o original.
    fn is_valid(&self, selections: &mut [Range]) -> bool {
        let mut enclosing_ranges: Vec<Range> = Vec::new();
        for decoration in &self.auto_closed_enclosing_decorations {
            if let Some(decoration_range) = self.model.get_decoration_range(decoration) {
                enclosing_ranges.push(decoration_range);
                if decoration_range.start_line_number != decoration_range.end_line_number {
                    // Para de rastrear se o intervalo ficar de várias linhas...
                    return false;
                }
            }
        }
        let by_starts = |a: &Range, b: &Range| Range::compare_ranges_using_starts(Some(a), Some(b)).cmp(&0);
        enclosing_ranges.sort_by(by_starts);

        selections.sort_by(by_starts);

        for (i, selection) in selections.iter().enumerate() {
            if i >= enclosing_ranges.len() {
                return false;
            }
            if !enclosing_ranges[i].strict_contains_range(selection) {
                return false;
            }
        }

        true
    }
}

/// `IExecContext`: o modelo, as seleções antes da execução e os intervalos rastreados criados pelos comandos
/// (com a direção da seleção que cada um rastreia).
struct ExecContext<'a> {
    model: &'a TextModel,
    selections_before: &'a [Selection],
    tracked_ranges: Vec<String>,
    tracked_ranges_direction: Vec<SelectionDirection>,
}

/// `ICommandData` e `ICommandsData`: as operações de edição de um ou de vários comandos.
struct CommandsData {
    operations: Vec<SingleEditOperation>,
    had_tracked_edit_operation: bool,
}

/// O `IEditOperationBuilder` que `_getEditOperationsFromCommand` monta para um comando.
struct CommandEditOperationBuilder<'a> {
    model: &'a TextModel,
    major_identifier: i32,
    inserts_auto_whitespace: bool,
    operations: Vec<SingleEditOperation>,
    operation_minor: i32,
    had_tracked_edit_operation: bool,
    tracked_ranges: &'a mut Vec<String>,
    tracked_ranges_direction: &'a mut Vec<SelectionDirection>,
}

impl EditOperationBuilder for CommandEditOperationBuilder<'_> {
    fn add_edit_operation(&mut self, range: Range, text: Option<JsString>, force_move_markers: bool) {
        if range.is_empty() && text.as_ref().is_some_and(JsString::is_empty) {
            // Este comando quer adicionar um no-op => não, obrigado
            return;
        }
        self.operations.push(SingleEditOperation {
            identifier: Some(SingleEditOperationIdentifier { major: self.major_identifier, minor: self.operation_minor }),
            range,
            text,
            force_move_markers,
            is_auto_whitespace_edit: self.inserts_auto_whitespace,
            is_tracked: false,
        });
        self.operation_minor += 1;
    }

    fn add_tracked_edit_operation(&mut self, range: Range, text: Option<JsString>, force_move_markers: bool) {
        self.had_tracked_edit_operation = true;
        self.add_edit_operation(range, text, force_move_markers);
    }

    fn track_selection(&mut self, selection: Selection, track_previous_on_empty: Option<bool>) -> String {
        let stickiness = if selection.is_empty() {
            match track_previous_on_empty {
                Some(true) => TrackedRangeStickiness::GrowsOnlyWhenTypingBefore,
                Some(false) => TrackedRangeStickiness::GrowsOnlyWhenTypingAfter,
                None => {
                    // Tenta prender no texto ao redor
                    let max_line_column = self.model.get_line_max_column(selection.start_line_number);
                    if selection.start_column == max_line_column {
                        TrackedRangeStickiness::GrowsOnlyWhenTypingBefore
                    } else {
                        TrackedRangeStickiness::GrowsOnlyWhenTypingAfter
                    }
                }
            }
        } else {
            TrackedRangeStickiness::NeverGrowsWhenTypingAtEdges
        };

        let l = self.tracked_ranges.len();
        let id = self
            .model
            .set_tracked_range(None, Some(&*selection), stickiness)
            .expect("criar um intervalo rastreado devolve o id");
        self.tracked_ranges.push(id);
        self.tracked_ranges_direction.push(selection.get_direction());
        l.to_string()
    }
}

/// O `ICursorStateComputerData` que o executor entrega a `computeCursorState` de um comando.
struct CommandCursorStateData<'a> {
    model: &'a TextModel,
    operations: &'a [ValidEditOperation],
    tracked_ranges: &'a [String],
    tracked_ranges_direction: &'a [SelectionDirection],
}

impl CursorStateComputerData for CommandCursorStateData<'_> {
    fn get_inverse_edit_operations(&self) -> Vec<ValidEditOperation> {
        self.operations.to_vec()
    }

    fn get_tracked_selection(&self, id: &str) -> Selection {
        let idx: usize = id.parse().expect("o id de seleção rastreada é um número");
        let range = self.model.get_tracked_range(&self.tracked_ranges[idx]).expect("o intervalo rastreado existe");
        Selection::from_range(&range, self.tracked_ranges_direction[idx])
    }
}

pub struct CommandExecutor;

impl CommandExecutor {
    /// `executeCommands(model, selectionsBefore, commands, editReason)`: o `editReason` padrão do original é
    /// `EditSources.unknown({ name: 'executeCommands' })`.
    pub fn execute_commands(
        model: &TextModel,
        selections_before: &[Selection],
        commands: &mut [Option<Box<dyn Command>>],
        edit_reason: &TextModelEditSource,
    ) -> Option<Vec<Selection>> {
        let mut ctx = ExecContext {
            model,
            selections_before,
            tracked_ranges: Vec::new(),
            tracked_ranges_direction: Vec::new(),
        };

        let result = Self::inner_execute_commands(&mut ctx, commands, edit_reason);

        for tracked_range in &ctx.tracked_ranges {
            ctx.model.set_tracked_range(
                Some(tracked_range),
                None,
                TrackedRangeStickiness::AlwaysGrowsWhenTypingAtEdges,
            );
        }

        result
    }

    fn inner_execute_commands(
        ctx: &mut ExecContext<'_>,
        commands: &mut [Option<Box<dyn Command>>],
        edit_reason: &TextModelEditSource,
    ) -> Option<Vec<Selection>> {
        if Self::array_is_empty(commands) {
            return None;
        }

        let commands_data = Self::get_edit_operations(ctx, commands);
        if commands_data.operations.is_empty() {
            return None;
        }

        let raw_operations = commands_data.operations;

        let loser_cursors_map = Self::get_loser_cursor_map(&raw_operations);
        if loser_cursors_map.contains(&0) {
            // Estes comandos estão muito bagunçados
            eprintln!("Ignoring commands");
            return None;
        }

        // Remove as operações dos cursores perdedores
        let mut filtered_operations: Vec<SingleEditOperation> = raw_operations
            .into_iter()
            .filter(|operation| !loser_cursors_map.contains(&Self::major_of(operation)))
            .collect();

        // TODO@Alex: find a better way to do this.
        // dá ao modelo a dica de que as operações de edição são rastreadas
        if commands_data.had_tracked_edit_operation && !filtered_operations.is_empty() {
            filtered_operations[0].is_tracked = true;
        }

        let ctx: &ExecContext<'_> = ctx;
        let model = ctx.model;
        let mut compute_cursor_selections = |inverse_edit_operations: &[ValidEditOperation]| -> Option<Vec<Selection>> {
            let mut grouped_inverse_edit_operations: Vec<Vec<ValidEditOperation>> =
                vec![Vec::new(); ctx.selections_before.len()];
            for op in inverse_edit_operations {
                let Some(identifier) = op.identifier else {
                    // talvez edições automáticas de aparar espaço em branco
                    continue;
                };
                grouped_inverse_edit_operations[identifier.major as usize].push(op.clone());
            }
            let mut cursor_selections: Vec<Selection> = Vec::with_capacity(ctx.selections_before.len());
            for i in 0..ctx.selections_before.len() {
                if grouped_inverse_edit_operations[i].is_empty() {
                    cursor_selections.push(ctx.selections_before[i]);
                    continue;
                }
                grouped_inverse_edit_operations[i]
                    .sort_by_key(|op| op.identifier.expect("a operação inversa tem identificador").minor);
                let data = CommandCursorStateData {
                    model,
                    operations: &grouped_inverse_edit_operations[i],
                    tracked_ranges: &ctx.tracked_ranges,
                    tracked_ranges_direction: &ctx.tracked_ranges_direction,
                };
                let command = commands[i].as_mut().expect("o comando da operação inversa existe");
                cursor_selections.push(command.compute_cursor_state(model, &data));
            }
            Some(cursor_selections)
        };
        let selections_after = model.push_edit_operations(
            Some(ctx.selections_before.to_vec()),
            &filtered_operations,
            Some(&mut compute_cursor_selections),
            None,
            Some(edit_reason.clone()),
        );
        let mut selections_after = selections_after.unwrap_or_else(|| ctx.selections_before.to_vec());

        // Extrai os cursores perdedores, em ordem decrescente
        let losing_cursors: Vec<i32> = loser_cursors_map.iter().rev().copied().collect();

        // Remove os cursores perdedores
        for losing_cursor in losing_cursors {
            if (losing_cursor as usize) < selections_after.len() {
                selections_after.remove(losing_cursor as usize);
            }
        }

        Some(selections_after)
    }

    /// `operation.identifier!.major`.
    fn major_of(operation: &SingleEditOperation) -> i32 {
        operation.identifier.expect("a operação de edição tem identificador").major
    }

    fn array_is_empty(commands: &[Option<Box<dyn Command>>]) -> bool {
        commands.iter().all(Option::is_none)
    }

    fn get_edit_operations(ctx: &mut ExecContext<'_>, commands: &mut [Option<Box<dyn Command>>]) -> CommandsData {
        let mut operations: Vec<SingleEditOperation> = Vec::new();
        let mut had_tracked_edit_operation = false;

        for (i, command) in commands.iter_mut().enumerate() {
            if let Some(command) = command {
                let r = Self::get_edit_operations_from_command(ctx, i as i32, &mut **command);
                operations.extend(r.operations);
                had_tracked_edit_operation = had_tracked_edit_operation || r.had_tracked_edit_operation;
            }
        }
        CommandsData { operations, had_tracked_edit_operation }
    }

    fn get_edit_operations_from_command(
        ctx: &mut ExecContext<'_>,
        major_identifier: i32,
        command: &mut dyn Command,
    ) -> CommandsData {
        // Este método age como uma transação: se o comando falha, tudo o que ele fez é ignorado
        let model = ctx.model;
        let mut edit_operation_builder = CommandEditOperationBuilder {
            model,
            major_identifier,
            inserts_auto_whitespace: command.inserts_auto_whitespace(),
            operations: Vec::new(),
            operation_minor: 0,
            had_tracked_edit_operation: false,
            tracked_ranges: &mut ctx.tracked_ranges,
            tracked_ranges_direction: &mut ctx.tracked_ranges_direction,
        };

        let outcome = errors::catch(|| command.get_edit_operations(model, &mut edit_operation_builder));
        if let Err(e) = outcome {
            // TODO@Alex use notification service if this should be user facing
            // e.friendlyMessage = nls.localize('corrupt.commands', "Unexpected exception while executing command.");
            errors::on_unexpected_error(&e);
            return CommandsData { operations: Vec::new(), had_tracked_edit_operation: false };
        }

        CommandsData {
            operations: edit_operation_builder.operations,
            had_tracked_edit_operation: edit_operation_builder.had_tracked_edit_operation,
        }
    }

    fn get_loser_cursor_map(operations: &[SingleEditOperation]) -> BTreeSet<i32> {
        // Isto é destrutivo no array
        let mut operations: Vec<SingleEditOperation> = operations.to_vec();

        // Ordena as operações com a última primeiro
        operations.sort_by(|a, b| {
            // Note o sinal de menos!
            (-Range::compare_ranges_using_ends(&a.range, &b.range)).cmp(&0)
        });

        // As operações não podem se sobrepor!
        let mut loser_cursors_map: BTreeSet<i32> = BTreeSet::new();

        let mut i: isize = 1;
        while i < operations.len() as isize {
            let previous_op = &operations[(i - 1) as usize];
            let current_op = &operations[i as usize];

            if previous_op.range.get_start_position().is_before(&current_op.range.get_end_position()) {
                let previous_major = Self::major_of(previous_op);
                let current_major = Self::major_of(current_op);

                let loser_major = if previous_major > current_major {
                    // previousOp perde a batalha
                    previous_major
                } else {
                    current_major
                };

                loser_cursors_map.insert(loser_major);

                let mut j: isize = 0;
                while j < operations.len() as isize {
                    if Self::major_of(&operations[j as usize]) == loser_major {
                        operations.remove(j as usize);
                        if j < i {
                            i -= 1;
                        }
                    } else {
                        j += 1;
                    }
                }

                if i > 0 {
                    i -= 1;
                }
            }
            i += 1;
        }

        loser_cursors_map
    }
}

/// `CompositionLineState`.
struct CompositionLineState {
    text: JsString,
    line_number: i32,
    start_selection_offset: i32,
    end_selection_offset: i32,
}

/// `CompositionState`.
struct CompositionState {
    original: Option<Vec<CompositionLineState>>,
}

impl CompositionState {
    fn capture(text_model: &TextModel, selections: &[Selection]) -> Option<Vec<CompositionLineState>> {
        let mut result: Vec<CompositionLineState> = Vec::with_capacity(selections.len());
        for selection in selections {
            if selection.start_line_number != selection.end_line_number {
                return None;
            }
            let line_number = selection.start_line_number;
            result.push(CompositionLineState {
                text: text_model.get_line_content(line_number),
                line_number,
                start_selection_offset: selection.start_column - 1,
                end_selection_offset: selection.end_column - 1,
            });
        }
        Some(result)
    }

    fn new(text_model: &TextModel, selections: &[Selection]) -> Self {
        Self { original: Self::capture(text_model, selections) }
    }

    /// Devolve o texto inserido durante esta composição.
    /// Se a composição mudou texto que já existia (isto é, não foi uma inserção pura), devolve `None`.
    fn deduce_outcome(&self, text_model: &TextModel, selections: &[Selection]) -> Option<Vec<CompositionOutcome>> {
        let original = self.original.as_ref()?;
        let current = Self::capture(text_model, selections)?;
        if original.len() != current.len() {
            return None;
        }
        Some(original.iter().zip(&current).map(|(original, current)| Self::deduce_line_outcome(original, current)).collect())
    }

    fn deduce_line_outcome(original: &CompositionLineState, current: &CompositionLineState) -> CompositionOutcome {
        let original_text = original.text.units();
        let current_text = current.text.units();
        let common_prefix = original
            .start_selection_offset
            .min(current.start_selection_offset)
            .min(common_prefix_length(original_text, current_text) as i32);
        let common_suffix = (original_text.len() as i32 - original.end_selection_offset)
            .min(current_text.len() as i32 - current.end_selection_offset)
            .min(common_suffix_length(original_text, current_text) as i32);
        let deleted_text = substring(
            original_text,
            i64::from(common_prefix),
            original_text.len() as i64 - i64::from(common_suffix),
        );
        let inserted_text_start_offset = common_prefix;
        let inserted_text_end_offset = current_text.len() as i32 - common_suffix;
        let inserted_text = substring(
            current_text,
            i64::from(inserted_text_start_offset),
            i64::from(inserted_text_end_offset),
        );
        let inserted_text_range = Range::new(
            current.line_number,
            inserted_text_start_offset + 1,
            current.line_number,
            inserted_text_end_offset + 1,
        );
        CompositionOutcome {
            deleted_text: JsString::from(deleted_text),
            deleted_selection_start: original.start_selection_offset - common_prefix,
            deleted_selection_end: original.end_selection_offset - common_prefix,
            inserted_text: JsString::from(inserted_text),
            inserted_selection_start: current.start_selection_offset - common_prefix,
            inserted_selection_end: current.end_selection_offset - common_prefix,
            inserted_text_range,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cursor_context::test_support::context_for;
    use crate::replace_command::ReplaceCommand;

    fn model_for(text: &str) -> Rc<TextModel> {
        context_for(text, true).model.clone()
    }

    fn insert(line_number: i32, column: i32, text: &str) -> Option<Box<dyn Command>> {
        Some(Box::new(ReplaceCommand::new(
            Range::new(line_number, column, line_number, column),
            JsString::from(text),
            None,
        )))
    }

    fn delete(range: Range) -> Option<Box<dyn Command>> {
        Some(Box::new(ReplaceCommand::new(range, JsString::new(), None)))
    }

    fn run(
        model: &TextModel,
        selections_before: &[Selection],
        mut commands: Vec<Option<Box<dyn Command>>>,
    ) -> Option<Vec<Selection>> {
        CommandExecutor::execute_commands(
            model,
            selections_before,
            &mut commands,
            &EditSources::unknown(Some("executeCommands")),
        )
    }

    /// Insere `text` em `(1, 1)` e rastreia `selection`, devolvendo a seleção rastreada como o novo cursor.
    struct InsertAndTrack {
        text: JsString,
        selection: Selection,
        id: Option<String>,
    }

    impl Command for InsertAndTrack {
        fn get_edit_operations(&mut self, _model: &TextModel, builder: &mut dyn EditOperationBuilder) {
            builder.add_edit_operation(Range::new(1, 1, 1, 1), Some(self.text.clone()), false);
            self.id = Some(builder.track_selection(self.selection, None));
        }

        fn compute_cursor_state(&mut self, _model: &TextModel, helper: &dyn CursorStateComputerData) -> Selection {
            helper.get_tracked_selection(self.id.as_deref().expect("o id foi criado em get_edit_operations"))
        }
    }

    #[test]
    fn executes_a_single_command_and_returns_the_cursor_after_the_inserted_text() {
        let model = model_for("abc");
        let result = run(&model, &[Selection::new(1, 2, 1, 2)], vec![insert(1, 2, "XY")]);
        // A operação inversa de `[1,2 -> 1,2]` com `XY` cobre `[1,2 -> 1,4]`, e o cursor fica no fim dela.
        assert_eq!(result, Some(vec![Selection::new(1, 4, 1, 4)]));
        assert_eq!(model.get_line_content(1), JsString::from("aXYbc"));
    }

    #[test]
    fn executes_one_command_per_cursor_in_the_resulting_document_coordinates() {
        let model = model_for("one two");
        let result = run(
            &model,
            &[Selection::new(1, 1, 1, 1), Selection::new(1, 5, 1, 5)],
            vec![insert(1, 1, "A"), insert(1, 5, "A")],
        );
        // Depois de `Aone Atwo`: o primeiro `A` ocupa `[1,1 -> 1,2]` e o segundo `[1,6 -> 1,7]`.
        assert_eq!(result, Some(vec![Selection::new(1, 2, 1, 2), Selection::new(1, 7, 1, 7)]));
        assert_eq!(model.get_line_content(1), JsString::from("Aone Atwo"));
    }

    #[test]
    fn returns_none_without_touching_the_model_when_there_is_no_command() {
        let model = model_for("abc");
        let version_id = model.get_version_id();
        assert_eq!(run(&model, &[Selection::new(1, 1, 1, 1)], vec![None]), None);
        assert_eq!(run(&model, &[Selection::new(1, 1, 1, 1)], Vec::new()), None);
        assert_eq!(model.get_version_id(), version_id);
        assert_eq!(model.get_line_content(1), JsString::from("abc"));
    }

    #[test]
    fn returns_none_when_every_command_is_a_no_op() {
        let model = model_for("abc");
        // Um comando que só insere texto vazio num intervalo vazio não gera operação.
        assert_eq!(run(&model, &[Selection::new(1, 1, 1, 1)], vec![insert(1, 1, "")]), None);
        assert_eq!(model.get_line_content(1), JsString::from("abc"));
    }

    #[test]
    fn overlapping_commands_drop_the_cursor_with_the_highest_index() {
        let model = model_for("abcdef");
        let result = run(
            &model,
            &[Selection::new(1, 1, 1, 3), Selection::new(1, 2, 1, 4)],
            vec![delete(Range::new(1, 1, 1, 3)), delete(Range::new(1, 2, 1, 4))],
        );
        // O cursor 1 perde (índice maior); só o comando 0 roda e o cursor 1 sai da lista.
        assert_eq!(result, Some(vec![Selection::new(1, 1, 1, 1)]));
        assert_eq!(model.get_line_content(1), JsString::from("cdef"));
    }

    #[test]
    fn tracked_selection_follows_the_edit_and_keeps_its_direction() {
        let model = model_for("abcdef");
        let command = |selection| -> Option<Box<dyn Command>> {
            Some(Box::new(InsertAndTrack { text: JsString::from("XY"), selection, id: None }))
        };
        let result = run(&model, &[Selection::new(1, 5, 1, 6)], vec![command(Selection::new(1, 5, 1, 6))]);
        // `e` (colunas 5 a 6) vai para as colunas 7 a 8 depois de inserir `XY` no começo.
        assert_eq!(result, Some(vec![Selection::new(1, 7, 1, 8)]));
        assert_eq!(model.get_line_content(1), JsString::from("XYabcdef"));

        // Seleção para trás: a posição fica antes da âncora.
        let model = model_for("abcdef");
        let result = run(&model, &[Selection::new(1, 6, 1, 5)], vec![command(Selection::new(1, 6, 1, 5))]);
        assert_eq!(result, Some(vec![Selection::new(1, 8, 1, 7)]));
    }

    #[test]
    fn auto_closing_close_chars_and_last_index_of_follow_the_original() {
        for unit in ")]}>'\"`".encode_utf16() {
            assert!(is_auto_closing_close_char(unit));
        }
        for unit in "({[<a ".encode_utf16() {
            assert!(!is_auto_closing_close_char(unit));
        }
        let haystack: Vec<u16> = "a(b(c)".encode_utf16().collect();
        let open: Vec<u16> = "(".encode_utf16().collect();
        // `'a(b(c)'.lastIndexOf('(', 4)` é 3; com `fromIndex` 2 é 1; com `-1` só olha o índice 0.
        assert_eq!(last_index_of_from(&haystack, &open, 4), Some(3));
        assert_eq!(last_index_of_from(&haystack, &open, 2), Some(1));
        assert_eq!(last_index_of_from(&haystack, &open, -1), None);
        assert_eq!(last_index_of_from(&haystack, &open, 100), Some(3));
    }
}
