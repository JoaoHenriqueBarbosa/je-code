//! Porte de `src/vs/editor/test/common/model/editableTextModelTestUtils.ts`: aplicar edições num modelo
//! acompanhado por um [`MirrorTextModel`] e conferir que os dois ficam iguais, além do mapa entre
//! deslocamento e posição.
//!
//! Notas de tradução:
//! - As operações inversas que `applyEdits(edits, true)` devolve são [`ValidEditOperation`]; no
//!   TypeScript elas valem como `ISingleEditOperation` por tipagem estrutural (sem `forceMoveMarkers`).
//!   Aqui [`to_single_edit`] faz essa conversão explícita, com `force_move_markers: false`.
//! - O `simplifyEdit` do original compara `range`, `text` e `forceMoveMarkers || false`: é o que
//!   [`simplify_edit`] devolve.
//! - O `console.warn` do original vira `eprintln!`.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::JsString;
use je_base::lifecycle::IDisposable;
use je_editor::edit_operation::SingleEditOperation;
use je_editor::mirror_text_model::MirrorTextModel;
use je_editor::model::{EndOfLinePreference, EndOfLineSequence, ValidEditOperation};
use je_editor::position::Position;
use je_editor::range::Range;
use je_editor::text_model::TextModel;

use super::test_text_model::create_text_model;

/// Uma operação inversa usada de novo como entrada de `applyEdits`.
pub fn to_single_edit(edit: &ValidEditOperation) -> SingleEditOperation {
    SingleEditOperation {
        identifier: edit.identifier,
        ..SingleEditOperation::new(edit.range, Some(edit.text.clone()), false)
    }
}

/// `simplifyEdit(edit)`.
pub fn simplify_edit(edit: &SingleEditOperation) -> (Range, Option<JsString>, bool) {
    (edit.range, edit.text.clone(), edit.force_move_markers)
}

/// `testApplyEditsWithSyncedModels(original, edits, expected, inputEditsAreInvalid)`.
pub fn test_apply_edits_with_synced_models(
    original: &[&str],
    edits: &[SingleEditOperation],
    expected: &[&str],
    input_edits_are_invalid: bool,
) {
    let original_str = JsString::from(original.join("\n").as_str());
    let expected_str = JsString::from(expected.join("\n").as_str());

    assert_synced_models(
        &original.join("\n"),
        |model, assert_mirror_models| {
            // Aplica as edições e recolhe as inversas
            let inverse_edits: Vec<SingleEditOperation> =
                model.apply_edits_computing_undo(edits).iter().map(to_single_edit).collect();

            // Confere que as edições produziram o resultado esperado
            assert_eq!(model.get_value_with(EndOfLinePreference::Lf, false), expected_str);

            assert_mirror_models();

            // Aplica as edições inversas
            let inverse_inverse_edits: Vec<SingleEditOperation> =
                model.apply_edits_computing_undo(&inverse_edits).iter().map(to_single_edit).collect();

            // Confere que as inversas devolveram o modelo ao estado original
            assert_eq!(model.get_value_with(EndOfLinePreference::Lf, false), original_str);

            if !input_edits_are_invalid {
                // Confere que a inversa da inversa são as edições originais
                assert_eq!(
                    inverse_inverse_edits.iter().map(simplify_edit).collect::<Vec<_>>(),
                    edits.iter().map(simplify_edit).collect::<Vec<_>>()
                );
            }

            assert_mirror_models();
        },
        None,
    );
}

/// `AssertDocumentLineMappingDirection`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum AssertDocumentLineMappingDirection {
    OffsetToPosition,
    PositionToOffset,
}

fn assert_one_direction_line_mapping(model: &TextModel, direction: AssertDocumentLineMappingDirection, msg: &str) {
    let all_text = model.get_value();
    let all_text = all_text.units();

    let (mut line, mut column, mut previous_is_carriage_return) = (1, 1, false);
    for offset in 0..=all_text.len() {
        // O sistema de coordenadas de posição não expressa a posição entre `\r` e `\n`
        let position = Position::new(line, column + if previous_is_carriage_return { -1 } else { 0 });

        if direction == AssertDocumentLineMappingDirection::OffsetToPosition {
            let actual_position = model.get_position_at(offset as i64);
            assert_eq!(
                actual_position.to_string(),
                position.to_string(),
                "{msg} - getPositionAt mismatch for offset {offset}"
            );
        } else {
            // O sistema de coordenadas de posição não expressa a posição entre `\r` e `\n`
            let expected_offset = offset - usize::from(previous_is_carriage_return);
            let actual_offset = model.get_offset_at(position);
            assert_eq!(actual_offset, expected_offset, "{msg} - getOffsetAt mismatch for position {position}");
        }

        // `charAt` além do fim devolve '', que não é `\n` nem `\r`.
        let ch = all_text.get(offset).copied();
        if ch == Some(u16::from(b'\n')) {
            line += 1;
            column = 1;
        } else {
            column += 1;
        }

        previous_is_carriage_return = ch == Some(u16::from(b'\r'));
    }
}

/// `assertLineMapping(model, msg)`.
pub fn assert_line_mapping(model: &TextModel, msg: &str) {
    assert_one_direction_line_mapping(model, AssertDocumentLineMappingDirection::PositionToOffset, msg);
    assert_one_direction_line_mapping(model, AssertDocumentLineMappingDirection::OffsetToPosition, msg);
}

/// O `setup` de `assertSyncedModels`: prepara o modelo antes de o espelho nascer.
pub type ModelSetup<'a> = &'a dyn Fn(&Rc<TextModel>);

/// `assertSyncedModels(text, callback, setup)`.
pub fn assert_synced_models(
    text: &str,
    callback: impl FnOnce(&Rc<TextModel>, &dyn Fn()),
    setup: Option<ModelSetup<'_>>,
) {
    let model = create_text_model(text);
    model.set_eol(EndOfLineSequence::Lf);
    assert_line_mapping(&model, "model");

    if let Some(setup) = setup {
        setup(&model);
        assert_line_mapping(&model, "model");
    }

    let mirror_model2 = Rc::new(RefCell::new(MirrorTextModel::new(
        None,
        model.get_lines_content(),
        model.get_eol(),
        model.get_version_id(),
    )));
    let mirror_model2_prev_version_id = Rc::new(Cell::new(model.get_version_id()));

    let disposable = {
        let mirror_model2 = mirror_model2.clone();
        let mirror_model2_prev_version_id = mirror_model2_prev_version_id.clone();
        model.on_did_change_content().listen(move |e| {
            let version_id = e.version_id;
            if version_id < mirror_model2_prev_version_id.get() {
                eprintln!("Model version id did not advance between edits (2)");
            }
            mirror_model2_prev_version_id.set(version_id);
            mirror_model2.borrow_mut().on_events(e);
        })
    };

    let assert_mirror_models = || {
        assert_line_mapping(&model, "model");
        assert_eq!(mirror_model2.borrow().get_text(), model.get_value(), "mirror model 2 text OK");
        assert_eq!(mirror_model2.borrow().version(), model.get_version_id(), "mirror model 2 version OK");
    };

    callback(&model, &assert_mirror_models);

    disposable.dispose();
    model.dispose();
    mirror_model2.borrow_mut().dispose();
}
