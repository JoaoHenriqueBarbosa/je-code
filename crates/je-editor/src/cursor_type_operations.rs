//! Porte de `src/vs/editor/common/cursor/cursorTypeOperations.ts`: o que o editor faz ao digitar, ao terminar
//! uma composição de IME e ao indentar (a fachada `TypeOperations` e o `CompositionOutcome`).
//!
//! Notas de tradução:
//!
//! - A classe estática `TypeOperations` é a struct sem campos [`TypeOperations`] com as funções associadas, como
//!   `DeleteOperations`. Os métodos do original que só repassam para uma operação de `cursorTypeEditOperations.ts`
//!   (`shiftIndent`, `unshiftIndent`, `paste`, `tab`, `compositionType` e `typeWithoutInterceptors`) não existem
//!   aqui, porque o repasse não acrescenta nada (regra DRY do projeto): quem chamaria
//!   `TypeOperations.paste(...)` chama `PasteOperation::get_edits`, `TypeOperations.tab(...)` chama
//!   `TabOperation::get_commands`, `TypeOperations.compositionType(...)` chama
//!   `CompositionOperation::get_edits`, `TypeOperations.typeWithoutInterceptors(...)` chama
//!   `TypeWithoutInterceptorsOperation::get_edits` e `TypeOperations.shiftIndent/unshiftIndent` são as funções
//!   `shift_indent` e `unshift_indent` (todas em `cursor_type_edit_operations.rs`).
//! - `ICursorSimpleModel` é `&dyn CursorSimpleModel`, `ITextModel` é o [`TextModel`] concreto, `ICommand[]` é
//!   `Vec<Box<dyn Command>>` e `ch: string` é `&[u16]` (ver `cursor_common.rs`).
//! - `model: ICursorSimpleModel | null` e `selections: Selection[] | null` de `indent` são `Option`.
//! - `compositions: CompositionOutcome[] | null` é `Option<&[CompositionOutcome]>`.
//! - `prevEditOperationType` de `compositionEndWithInterceptors` não é lido no original; o parâmetro fica, com
//!   `_`, para a chamada seguir a assinatura de lá.
//! - A cadeia de `if (x !== undefined) return x` de `typeWithInterceptors` e de `compositionEndWithInterceptors` é
//!   `Option::or_else`, que também só avalia a operação seguinte se a anterior não editou nada.

use je_base::JsString;
use je_base::char_code as cc;

use crate::cursor_common::{
    CursorConfiguration, CursorSimpleModel, EditOperationResult, EditOperationResultOptions, EditOperationType, is_quote,
};
use crate::cursor_type_edit_operations::{
    AutoClosingOpenCharTypeOperation, AutoClosingOvertypeOperation, AutoClosingOvertypeWithInterceptorsOperation,
    AutoIndentOperation, CompositionEndOvertypeOperation, EnterOperation, InterceptorElectricCharOperation,
    SimpleCharacterTypeOperation, SurroundSelectionOperation, should_surround_char,
};
use crate::editor_common::Command;
use crate::position::Position;
use crate::range::Range;
use crate::selection::Selection;
use crate::shift_command::{ShiftCommand, ShiftCommandOpts};
use crate::surround_selection_command::CompositionSurroundSelectionCommand;
use crate::text_model::TextModel;

pub struct TypeOperations;

impl TypeOperations {
    pub fn indent(
        config: &CursorConfiguration,
        model: Option<&dyn CursorSimpleModel>,
        selections: Option<&[Selection]>,
    ) -> Vec<Box<dyn Command>> {
        let (Some(_), Some(selections)) = (model, selections) else {
            return Vec::new();
        };
        Self::shift_commands(config, selections, false)
    }

    pub fn outdent(
        config: &CursorConfiguration,
        _model: &dyn CursorSimpleModel,
        selections: &[Selection],
    ) -> Vec<Box<dyn Command>> {
        Self::shift_commands(config, selections, true)
    }

    /// Um `ShiftCommand` por seleção: o corpo comum de `indent` e `outdent`, que só diferem em `isUnshift`.
    fn shift_commands(config: &CursorConfiguration, selections: &[Selection], is_unshift: bool) -> Vec<Box<dyn Command>> {
        selections
            .iter()
            .map(|selection| {
                Box::new(ShiftCommand::new(
                    *selection,
                    ShiftCommandOpts {
                        is_unshift,
                        tab_size: config.tab_size,
                        indent_size: config.indent_size,
                        insert_spaces: config.insert_spaces,
                        use_tab_stops: config.use_tab_stops,
                        auto_indent: config.auto_indent,
                    },
                    config.language_configuration_service.clone(),
                )) as Box<dyn Command>
            })
            .collect()
    }

    /// Muito parecido com digitar, mas o caractere já está no buffer de texto!
    pub fn composition_end_with_interceptors(
        _prev_edit_operation_type: EditOperationType,
        config: &CursorConfiguration,
        model: &TextModel,
        compositions: Option<&[CompositionOutcome]>,
        selections: &[Selection],
        auto_closed_characters: &[Range],
    ) -> Option<EditOperationResult> {
        // `None` é não ter conseguido deduzir o que a composição fez
        let compositions = compositions?;

        let mut inserted_text: Option<&JsString> = None;
        for composition in compositions {
            match inserted_text {
                None => inserted_text = Some(&composition.inserted_text),
                // nem todas as seleções concordam sobre o que foi digitado
                Some(text) if *text != composition.inserted_text => return None,
                Some(_) => {}
            }
        }

        let Some(inserted_text) = inserted_text.filter(|text| text.len() == 1) else {
            // só interessa o caso em que um único caractere foi inserido
            return CompositionEndOvertypeOperation::get_edits(config, compositions);
        };

        let ch = inserted_text.units();

        let has_deletion = compositions.iter().any(|composition| !composition.deleted_text.is_empty());

        if has_deletion {
            // Confere se isso poderia ter sido envolver a seleção
            if !should_surround_char(config, ch) {
                return None;
            }
            let close_character = config.surrounding_pairs.get(ch)?;

            let is_typing_a_quote_character = is_quote(ch);

            for composition in compositions {
                if composition.deleted_selection_start != 0
                    || composition.deleted_selection_end != composition.deleted_text.len() as i32
                {
                    // apagou mais texto do que o selecionado, então não podia ser envolver a seleção
                    return None;
                }
                let deleted_text = composition.deleted_text.units();
                // `/^[ \t]+$/.test(deletedText)`
                if !deleted_text.is_empty() && deleted_text.iter().all(|&unit| unit == cc::Space || unit == cc::Tab) {
                    // o texto apagado era só espaço em branco
                    return None;
                }
                if is_typing_a_quote_character && is_quote(deleted_text) {
                    // o texto apagado era uma aspa
                    return None;
                }
            }

            let mut positions: Vec<Position> = Vec::with_capacity(selections.len());
            for selection in selections {
                if !selection.is_empty() {
                    return None;
                }
                positions.push(selection.get_position());
            }

            if positions.len() != compositions.len() {
                return None;
            }

            let commands: Vec<Option<Box<dyn Command>>> = positions
                .iter()
                .zip(compositions)
                .map(|(position, composition)| {
                    Some(Box::new(CompositionSurroundSelectionCommand::new(
                        *position,
                        composition.deleted_text.clone(),
                        close_character.clone(),
                    )) as Box<dyn Command>)
                })
                .collect();
            return Some(EditOperationResult::new(
                EditOperationType::TypingOther,
                commands,
                EditOperationResultOptions {
                    should_push_stack_element_before: true,
                    should_push_stack_element_after: false,
                    follow_up: None,
                },
            ));
        }

        AutoClosingOvertypeWithInterceptorsOperation::get_edits(config, model, selections, auto_closed_characters, ch)
            .or_else(|| AutoClosingOpenCharTypeOperation::get_edits(config, model, selections, ch, true, false))
            .or_else(|| CompositionEndOvertypeOperation::get_edits(config, compositions))
    }

    pub fn type_with_interceptors(
        is_doing_composition: bool,
        prev_edit_operation_type: EditOperationType,
        config: &CursorConfiguration,
        model: &TextModel,
        selections: &[Selection],
        auto_closed_characters: &[Range],
        ch: &[u16],
    ) -> EditOperationResult {
        EnterOperation::get_edits(config, model, selections, ch, is_doing_composition)
            .or_else(|| AutoIndentOperation::get_edits(config, model, selections, ch, is_doing_composition))
            .or_else(|| {
                AutoClosingOvertypeOperation::get_edits(
                    prev_edit_operation_type,
                    config,
                    model,
                    selections,
                    auto_closed_characters,
                    ch,
                )
            })
            .or_else(|| {
                AutoClosingOpenCharTypeOperation::get_edits(config, model, selections, ch, false, is_doing_composition)
            })
            .or_else(|| SurroundSelectionOperation::get_edits(config, model, selections, ch, is_doing_composition))
            .or_else(|| {
                InterceptorElectricCharOperation::get_edits(
                    prev_edit_operation_type,
                    config,
                    model,
                    selections,
                    ch,
                    is_doing_composition,
                )
            })
            .unwrap_or_else(|| {
                SimpleCharacterTypeOperation::get_edits(
                    config,
                    prev_edit_operation_type,
                    selections,
                    ch,
                    is_doing_composition,
                )
            })
    }
}

/// `CompositionOutcome`: o que uma composição de IME apagou e inseriu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompositionOutcome {
    pub deleted_text: JsString,
    pub deleted_selection_start: i32,
    pub deleted_selection_end: i32,
    pub inserted_text: JsString,
    pub inserted_selection_start: i32,
    pub inserted_selection_end: i32,
    pub inserted_text_range: Range,
}
