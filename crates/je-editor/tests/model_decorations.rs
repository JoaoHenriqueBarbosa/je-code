//! Porte de `src/vs/editor/test/common/model/modelDecorations.test.ts` (suítes `Editor Model - Model Decorations`, `Decorations and editing` (com `insert`, `delete`, `replace short` e `replace long`, cada uma com `collapsed dec` e `non-collapsed dec`) e `deltaDecorations`), caso a caso e na mesma ordem.
//!
//! Cada `suite` do original vira um `mod`, inclusive as aninhadas.
//!
//! Notas de tradução:
//! - Os parâmetros com valor padrão do original (`ownerId = 0`, `filterOutValidation = false`, ...) são
//!   passados explícitos.
//! - O `setup`/`teardown` da primeira suíte vira [`with_this_model`], que cria o modelo, roda o caso e o
//!   descarta.
//! - `hoverMessage` do original aceita `IMarkdownString | IMarkdownString[]`; aqui é sempre
//!   `Vec<MarkdownString>`, então o `{ value: 'hello2' }` do caso `issue #4317` vira um vetor de um elemento.
//! - Em `testDeltaDecorations` o original confere de novo as decorações iniciais no fim (e não as novas);
//!   o porte mantém a mesma conferência.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::cell::Cell;
use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::JsString;
use je_base::lifecycle::IDisposable;
use je_editor::edit_operation::{EditOperation, SingleEditOperation};
use je_editor::model::{
    EndOfLineSequence, InjectedTextOptions, MarkdownString, ModelDecorationOptionsInit, ModelDeltaDecoration,
    TrackedRangeStickiness,
};
use je_editor::position::Position;
use je_editor::range::Range;
use je_editor::text_model::{ModelDecorationOptions, TextModel};
use support::test_text_model::*;

// --------- utils

/// `ILightWeightDecoration2`: o intervalo e o `className`.
type LightWeightDecoration2 = (Range, Option<String>);

fn model_has_decorations(model: &TextModel, decorations: &[(Range, &str)]) {
    let mut model_decorations: Vec<LightWeightDecoration2> = model
        .get_all_decorations(0, false, false)
        .into_iter()
        .map(|decoration| (decoration.range, decoration.options.class_name.clone()))
        .collect();
    model_decorations.sort_by(|a, b| Range::compare_ranges_using_starts(Some(&a.0), Some(&b.0)).cmp(&0));
    let expected: Vec<LightWeightDecoration2> =
        decorations.iter().map(|(range, class_name)| (*range, Some((*class_name).to_owned()))).collect();
    assert_eq!(model_decorations, expected);
}

fn model_has_decoration(
    model: &TextModel,
    start_line_number: i32,
    start_column: i32,
    end_line_number: i32,
    end_column: i32,
    class_name: &str,
) {
    model_has_decorations(
        model,
        &[(Range::new(start_line_number, start_column, end_line_number, end_column), class_name)],
    );
}

fn model_has_no_decorations(model: &TextModel) {
    assert_eq!(model.get_all_decorations(0, false, false).len(), 0, "Model has no decoration");
}

fn add_decoration(
    model: &TextModel,
    start_line_number: i32,
    start_column: i32,
    end_line_number: i32,
    end_column: i32,
    class_name: &str,
) -> String {
    model
        .change_decorations(
            |change_accessor| {
                change_accessor.add_decoration(
                    &Range::new(start_line_number, start_column, end_line_number, end_column),
                    ModelDecorationOptions::register(&ModelDecorationOptionsInit {
                        description: "test".to_owned(),
                        class_name: Some(class_name.to_owned()),
                        ..Default::default()
                    }),
                )
            },
            0,
        )
        .unwrap()
}

fn line_has_decorations(model: &TextModel, line_number: i32, decorations: &[(i32, i32, &str)]) {
    let line_decorations: Vec<(i32, i32, Option<String>)> = model
        .get_line_decorations(line_number, 0, false, false)
        .into_iter()
        .map(|decoration| (decoration.range.start_column, decoration.range.end_column, decoration.options.class_name.clone()))
        .collect();
    let expected: Vec<(i32, i32, Option<String>)> =
        decorations.iter().map(|(start, end, class_name)| (*start, *end, Some((*class_name).to_owned()))).collect();
    assert_eq!(line_decorations, expected, "Line decorations");
}

fn line_has_no_decorations(model: &TextModel, line_number: i32) {
    line_has_decorations(model, line_number, &[]);
}

fn line_has_decoration(model: &TextModel, line_number: i32, start: i32, end: i32, class_name: &str) {
    line_has_decorations(model, line_number, &[(start, end, class_name)]);
}

/// Um `SingleEditOperation` com texto, como o literal `{ range, text, forceMoveMarkers }` do original.
fn edit(range: Range, text: &str, force_move_markers: bool) -> SingleEditOperation {
    SingleEditOperation::new(range, Some(JsString::from(text)), force_move_markers)
}

mod editor_model_model_decorations {
    use super::*;

    const LINE1: &str = "My First Line";
    const LINE2: &str = "\t\tMy Second Line";
    const LINE3: &str = "    Third Line";
    const LINE4: &str = "";
    const LINE5: &str = "1";

    // --------- Model Decorations

    /// `setup` e `teardown` da suíte em volta do caso.
    fn with_this_model(test: impl FnOnce(&TextModel)) {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let text = format!("{LINE1}\r\n{LINE2}\n{LINE3}\n{LINE4}\r\n{LINE5}");
        let this_model = create_text_model(&text);
        test(&this_model);
        this_model.dispose();
    }

    #[test]
    fn single_character_decoration() {
        with_this_model(|this_model| {
            add_decoration(this_model, 1, 1, 1, 2, "myType");
            line_has_decoration(this_model, 1, 1, 2, "myType");
            line_has_no_decorations(this_model, 2);
            line_has_no_decorations(this_model, 3);
            line_has_no_decorations(this_model, 4);
            line_has_no_decorations(this_model, 5);
        });
    }

    #[test]
    fn injected_text_width_must_be_finite_and_non_negative() {
        with_this_model(|this_model| {
            let widths = [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.0, 0.0, 1.0];
            let new_decorations: Vec<ModelDeltaDecoration> = widths
                .iter()
                .map(|&width_in_em| ModelDeltaDecoration {
                    range: Range::new(1, 1, 1, 1),
                    options: ModelDecorationOptions::register(&ModelDecorationOptionsInit {
                        description: "test".to_owned(),
                        after: Some(InjectedTextOptions {
                            content: "x".to_owned(),
                            width_in_em: Some(width_in_em),
                            ..Default::default()
                        }),
                        ..Default::default()
                    }),
                })
                .collect();
            let decoration_ids = this_model.delta_decorations(&[], &new_decorations, 0);

            let actual: Vec<Option<f64>> = decoration_ids
                .iter()
                .map(|id| {
                    this_model
                        .get_decoration_options(id)
                        .and_then(|options| options.after.as_ref().and_then(|after| after.width_in_em))
                })
                .collect();
            assert_eq!(actual, vec![None, None, None, None, Some(0.0), Some(1.0)]);
        });
    }

    #[test]
    fn line_decoration() {
        with_this_model(|this_model| {
            add_decoration(this_model, 1, 1, 1, 14, "myType");
            line_has_decoration(this_model, 1, 1, 14, "myType");
            line_has_no_decorations(this_model, 2);
            line_has_no_decorations(this_model, 3);
            line_has_no_decorations(this_model, 4);
            line_has_no_decorations(this_model, 5);
        });
    }

    #[test]
    fn full_line_decoration() {
        with_this_model(|this_model| {
            add_decoration(this_model, 1, 1, 2, 1, "myType");

            let line1_decorations = this_model.get_line_decorations(1, 0, false, false);
            assert_eq!(line1_decorations.len(), 1);
            assert_eq!(line1_decorations[0].options.class_name.as_deref(), Some("myType"));

            let line2_decorations = this_model.get_line_decorations(1, 0, false, false);
            assert_eq!(line2_decorations.len(), 1);
            assert_eq!(line2_decorations[0].options.class_name.as_deref(), Some("myType"));

            line_has_no_decorations(this_model, 3);
            line_has_no_decorations(this_model, 4);
            line_has_no_decorations(this_model, 5);
        });
    }

    #[test]
    fn multiple_line_decoration() {
        with_this_model(|this_model| {
            add_decoration(this_model, 1, 2, 3, 2, "myType");

            let line1_decorations = this_model.get_line_decorations(1, 0, false, false);
            assert_eq!(line1_decorations.len(), 1);
            assert_eq!(line1_decorations[0].options.class_name.as_deref(), Some("myType"));

            let line2_decorations = this_model.get_line_decorations(1, 0, false, false);
            assert_eq!(line2_decorations.len(), 1);
            assert_eq!(line2_decorations[0].options.class_name.as_deref(), Some("myType"));

            let line3_decorations = this_model.get_line_decorations(1, 0, false, false);
            assert_eq!(line3_decorations.len(), 1);
            assert_eq!(line3_decorations[0].options.class_name.as_deref(), Some("myType"));

            line_has_no_decorations(this_model, 4);
            line_has_no_decorations(this_model, 5);
        });
    }

    // --------- removing, changing decorations

    #[test]
    fn decoration_gets_removed() {
        with_this_model(|this_model| {
            let dec_id = add_decoration(this_model, 1, 2, 3, 2, "myType");
            model_has_decoration(this_model, 1, 2, 3, 2, "myType");
            this_model.change_decorations(|change_accessor| change_accessor.remove_decoration(&dec_id), 0);
            model_has_no_decorations(this_model);
        });
    }

    #[test]
    fn decorations_get_removed() {
        with_this_model(|this_model| {
            let dec_id1 = add_decoration(this_model, 1, 2, 3, 2, "myType1");
            let dec_id2 = add_decoration(this_model, 1, 2, 3, 1, "myType2");
            model_has_decorations(
                this_model,
                &[(Range::new(1, 2, 3, 1), "myType2"), (Range::new(1, 2, 3, 2), "myType1")],
            );
            this_model.change_decorations(|change_accessor| change_accessor.remove_decoration(&dec_id1), 0);
            model_has_decorations(this_model, &[(Range::new(1, 2, 3, 1), "myType2")]);
            this_model.change_decorations(|change_accessor| change_accessor.remove_decoration(&dec_id2), 0);
            model_has_no_decorations(this_model);
        });
    }

    #[test]
    fn decoration_range_can_be_changed() {
        with_this_model(|this_model| {
            let dec_id = add_decoration(this_model, 1, 2, 3, 2, "myType");
            model_has_decoration(this_model, 1, 2, 3, 2, "myType");
            this_model.change_decorations(
                |change_accessor| change_accessor.change_decoration(&dec_id, &Range::new(1, 1, 1, 2)),
                0,
            );
            model_has_decoration(this_model, 1, 1, 1, 2, "myType");
        });
    }

    // --------- eventing

    /// Registra um listener de `onDidChangeDecorations` que só conta as chamadas.
    fn count_decoration_events(model: &TextModel) -> (Rc<Cell<i32>>, je_base::lifecycle::DisposableRef) {
        let listener_called = Rc::new(Cell::new(0));
        let counter = listener_called.clone();
        let disposable = model.on_did_change_decorations().listen(move |_e| counter.set(counter.get() + 1));
        (listener_called, disposable)
    }

    #[test]
    fn decorations_emit_event_on_add() {
        with_this_model(|this_model| {
            let (listener_called, disposable) = count_decoration_events(this_model);
            add_decoration(this_model, 1, 2, 3, 2, "myType");
            assert_eq!(listener_called.get(), 1, "listener called");
            disposable.dispose();
        });
    }

    #[test]
    fn decorations_emit_event_on_change() {
        with_this_model(|this_model| {
            let dec_id = add_decoration(this_model, 1, 2, 3, 2, "myType");
            let (listener_called, disposable) = count_decoration_events(this_model);
            this_model.change_decorations(
                |change_accessor| change_accessor.change_decoration(&dec_id, &Range::new(1, 1, 1, 2)),
                0,
            );
            assert_eq!(listener_called.get(), 1, "listener called");
            disposable.dispose();
        });
    }

    #[test]
    fn decorations_emit_event_on_remove() {
        with_this_model(|this_model| {
            let dec_id = add_decoration(this_model, 1, 2, 3, 2, "myType");
            let (listener_called, disposable) = count_decoration_events(this_model);
            this_model.change_decorations(|change_accessor| change_accessor.remove_decoration(&dec_id), 0);
            assert_eq!(listener_called.get(), 1, "listener called");
            disposable.dispose();
        });
    }

    #[test]
    fn decorations_emit_event_when_inserting_one_line_text_before_it() {
        with_this_model(|this_model| {
            add_decoration(this_model, 1, 2, 3, 2, "myType");

            let (listener_called, disposable) = count_decoration_events(this_model);

            this_model.apply_edits(&[EditOperation::insert(Position::new(1, 1), "Hallo ")]);
            assert_eq!(listener_called.get(), 1, "listener called");
            disposable.dispose();
        });
    }

    #[test]
    fn decorations_do_not_emit_event_on_no_op_delta_decorations() {
        with_this_model(|this_model| {
            let (listener_called, disposable) = count_decoration_events(this_model);

            this_model.delta_decorations(&[], &[], 0);
            this_model.change_decorations(
                |accessor| {
                    accessor.delta_decorations(&[], &[]);
                },
                0,
            );

            assert_eq!(listener_called.get(), 0, "listener not called");
            disposable.dispose();
        });
    }

    // --------- editing text & effects on decorations

    #[test]
    fn decorations_are_updated_when_inserting_one_line_text_before_it() {
        with_this_model(|this_model| {
            add_decoration(this_model, 1, 2, 3, 2, "myType");
            model_has_decoration(this_model, 1, 2, 3, 2, "myType");
            this_model.apply_edits(&[EditOperation::insert(Position::new(1, 1), "Hallo ")]);
            model_has_decoration(this_model, 1, 8, 3, 2, "myType");
        });
    }

    #[test]
    fn decorations_are_updated_when_inserting_one_line_text_before_it_2() {
        with_this_model(|this_model| {
            add_decoration(this_model, 1, 1, 3, 2, "myType");
            model_has_decoration(this_model, 1, 1, 3, 2, "myType");
            this_model.apply_edits(&[EditOperation::replace(Range::new(1, 1, 1, 1), Some(JsString::from("Hallo ")))]);
            model_has_decoration(this_model, 1, 1, 3, 2, "myType");
        });
    }

    #[test]
    fn decorations_are_updated_when_inserting_multiple_lines_text_before_it() {
        with_this_model(|this_model| {
            add_decoration(this_model, 1, 2, 3, 2, "myType");
            model_has_decoration(this_model, 1, 2, 3, 2, "myType");
            this_model.apply_edits(&[EditOperation::insert(Position::new(1, 1), "Hallo\nI'm inserting multiple\nlines")]);
            model_has_decoration(this_model, 3, 7, 5, 2, "myType");
        });
    }

    #[test]
    fn decorations_change_when_inserting_text_after_them() {
        with_this_model(|this_model| {
            add_decoration(this_model, 1, 2, 3, 2, "myType");
            model_has_decoration(this_model, 1, 2, 3, 2, "myType");
            this_model.apply_edits(&[EditOperation::insert(Position::new(3, 2), "Hallo")]);
            model_has_decoration(this_model, 1, 2, 3, 7, "myType");
        });
    }

    #[test]
    fn decorations_are_updated_when_inserting_text_inside() {
        with_this_model(|this_model| {
            add_decoration(this_model, 1, 2, 3, 2, "myType");
            model_has_decoration(this_model, 1, 2, 3, 2, "myType");
            this_model.apply_edits(&[EditOperation::insert(Position::new(1, 3), "Hallo ")]);
            model_has_decoration(this_model, 1, 2, 3, 2, "myType");
        });
    }

    #[test]
    fn decorations_are_updated_when_inserting_text_inside_2() {
        with_this_model(|this_model| {
            add_decoration(this_model, 1, 2, 3, 2, "myType");
            model_has_decoration(this_model, 1, 2, 3, 2, "myType");
            this_model.apply_edits(&[EditOperation::insert(Position::new(3, 1), "Hallo ")]);
            model_has_decoration(this_model, 1, 2, 3, 8, "myType");
        });
    }

    #[test]
    fn decorations_are_updated_when_inserting_text_inside_3() {
        with_this_model(|this_model| {
            add_decoration(this_model, 1, 1, 2, 16, "myType");
            model_has_decoration(this_model, 1, 1, 2, 16, "myType");
            this_model.apply_edits(&[EditOperation::insert(Position::new(2, 2), "\n")]);
            model_has_decoration(this_model, 1, 1, 3, 15, "myType");
        });
    }

    #[test]
    fn decorations_are_updated_when_inserting_multiple_lines_text_inside() {
        with_this_model(|this_model| {
            add_decoration(this_model, 1, 2, 3, 2, "myType");
            model_has_decoration(this_model, 1, 2, 3, 2, "myType");
            this_model.apply_edits(&[EditOperation::insert(Position::new(1, 3), "Hallo\nI'm inserting multiple\nlines")]);
            model_has_decoration(this_model, 1, 2, 5, 2, "myType");
        });
    }

    #[test]
    fn decorations_are_updated_when_deleting_one_line_text_before_it() {
        with_this_model(|this_model| {
            add_decoration(this_model, 1, 2, 3, 2, "myType");
            model_has_decoration(this_model, 1, 2, 3, 2, "myType");
            this_model.apply_edits(&[EditOperation::delete(Range::new(1, 1, 1, 2))]);
            model_has_decoration(this_model, 1, 1, 3, 2, "myType");
        });
    }

    #[test]
    fn decorations_are_updated_when_deleting_multiple_lines_text_before_it() {
        with_this_model(|this_model| {
            add_decoration(this_model, 2, 2, 3, 2, "myType");
            model_has_decoration(this_model, 2, 2, 3, 2, "myType");
            this_model.apply_edits(&[EditOperation::delete(Range::new(1, 1, 2, 1))]);
            model_has_decoration(this_model, 1, 2, 2, 2, "myType");
        });
    }

    #[test]
    fn decorations_are_updated_when_deleting_multiple_lines_text_before_it_2() {
        with_this_model(|this_model| {
            add_decoration(this_model, 2, 3, 3, 2, "myType");
            model_has_decoration(this_model, 2, 3, 3, 2, "myType");
            this_model.apply_edits(&[EditOperation::delete(Range::new(1, 1, 2, 2))]);
            model_has_decoration(this_model, 1, 2, 2, 2, "myType");
        });
    }

    #[test]
    fn decorations_are_updated_when_deleting_text_inside() {
        with_this_model(|this_model| {
            add_decoration(this_model, 1, 2, 4, 1, "myType");
            model_has_decoration(this_model, 1, 2, 4, 1, "myType");
            this_model.apply_edits(&[EditOperation::delete(Range::new(1, 3, 2, 1))]);
            model_has_decoration(this_model, 1, 2, 3, 1, "myType");
        });
    }

    #[test]
    fn decorations_are_updated_when_deleting_text_inside_2() {
        with_this_model(|this_model| {
            add_decoration(this_model, 1, 2, 4, 1, "myType");
            model_has_decoration(this_model, 1, 2, 4, 1, "myType");
            this_model.apply_edits(&[
                EditOperation::delete(Range::new(1, 1, 1, 2)),
                EditOperation::delete(Range::new(4, 1, 4, 1)),
            ]);
            model_has_decoration(this_model, 1, 1, 4, 1, "myType");
        });
    }

    #[test]
    fn decorations_are_updated_when_deleting_multiple_lines_text() {
        with_this_model(|this_model| {
            add_decoration(this_model, 1, 2, 4, 1, "myType");
            model_has_decoration(this_model, 1, 2, 4, 1, "myType");
            this_model.apply_edits(&[EditOperation::delete(Range::new(1, 1, 3, 1))]);
            model_has_decoration(this_model, 1, 1, 2, 1, "myType");
        });
    }

    #[test]
    fn decorations_are_updated_when_changing_eol() {
        with_this_model(|this_model| {
            add_decoration(this_model, 1, 2, 4, 1, "myType1");
            add_decoration(this_model, 1, 3, 4, 1, "myType2");
            add_decoration(this_model, 1, 4, 4, 1, "myType3");
            add_decoration(this_model, 1, 5, 4, 1, "myType4");
            add_decoration(this_model, 1, 6, 4, 1, "myType5");
            add_decoration(this_model, 1, 7, 4, 1, "myType6");
            add_decoration(this_model, 1, 8, 4, 1, "myType7");
            add_decoration(this_model, 1, 9, 4, 1, "myType8");
            add_decoration(this_model, 1, 10, 4, 1, "myType9");
            this_model.apply_edits(&[EditOperation::insert(Position::new(1, 1), "x")]);
            this_model.set_eol(EndOfLineSequence::CrLf);
            this_model.apply_edits(&[EditOperation::insert(Position::new(1, 1), "x")]);
            model_has_decorations(
                this_model,
                &[
                    (Range::new(1, 4, 4, 1), "myType1"),
                    (Range::new(1, 5, 4, 1), "myType2"),
                    (Range::new(1, 6, 4, 1), "myType3"),
                    (Range::new(1, 7, 4, 1), "myType4"),
                    (Range::new(1, 8, 4, 1), "myType5"),
                    (Range::new(1, 9, 4, 1), "myType6"),
                    (Range::new(1, 10, 4, 1), "myType7"),
                    (Range::new(1, 11, 4, 1), "myType8"),
                    (Range::new(1, 12, 4, 1), "myType9"),
                ],
            );
        });
    }

    #[test]
    fn an_apparently_simple_edit() {
        with_this_model(|this_model| {
            add_decoration(this_model, 1, 2, 4, 1, "myType1");
            this_model.apply_edits(&[EditOperation::replace(Range::new(1, 14, 2, 1), Some(JsString::from("x")))]);
            model_has_decorations(this_model, &[(Range::new(1, 2, 3, 1), "myType1")]);
        });
    }

    #[test]
    fn remove_all_decorations_with_owner_id_can_be_called_after_model_dispose() {
        with_this_model(|_this_model| {
            let model = create_text_model("asd");
            model.dispose();
            model.remove_all_decorations_with_owner_id(1);
        });
    }

    #[test]
    fn remove_all_decorations_with_owner_id_works() {
        with_this_model(|this_model| {
            this_model.delta_decorations(
                &[],
                &[ModelDeltaDecoration {
                    range: Range::new(1, 2, 4, 1),
                    options: ModelDecorationOptions::register(&ModelDecorationOptionsInit {
                        description: "test".to_owned(),
                        class_name: Some("myType1".to_owned()),
                        ..Default::default()
                    }),
                }],
                1,
            );
            this_model.remove_all_decorations_with_owner_id(1);
            model_has_no_decorations(this_model);
        });
    }
}

mod decorations_and_editing {
    use super::*;

    #[allow(clippy::too_many_arguments)]
    fn run_test_once(
        dec_range: Range,
        stickiness: u8,
        edit_range: Range,
        edit_text: &str,
        edit_force_move_markers: bool,
        expected_dec_range: Range,
        msg: &str,
    ) {
        let model = create_text_model(&["My First Line", "My Second Line", "Third Line"].join("\n"));

        let id = model
            .delta_decorations(
                &[],
                &[ModelDeltaDecoration {
                    range: dec_range,
                    options: ModelDecorationOptions::register(&ModelDecorationOptionsInit {
                        description: "test".to_owned(),
                        stickiness: Some(TrackedRangeStickiness::from_u8(stickiness)),
                        ..Default::default()
                    }),
                }],
                0,
            )
            .remove(0);
        model.apply_edits(&[edit(edit_range, edit_text, edit_force_move_markers)]);
        let actual = model.get_decoration_range(&id);
        assert_eq!(actual, Some(expected_dec_range), "{msg}");

        model.dispose();
    }

    /// `runTest`: as quatro aderências, sem e com `forceMoveMarkers`.
    fn run_test(dec_range: Range, edit_range: Range, edit_text: &str, expected_dec_range: [[Range; 4]; 2]) {
        run_test_once(dec_range, 0, edit_range, edit_text, false, expected_dec_range[0][0], "no-0-AlwaysGrowsWhenTypingAtEdges");
        run_test_once(dec_range, 1, edit_range, edit_text, false, expected_dec_range[0][1], "no-1-NeverGrowsWhenTypingAtEdges");
        run_test_once(dec_range, 2, edit_range, edit_text, false, expected_dec_range[0][2], "no-2-GrowsOnlyWhenTypingBefore");
        run_test_once(dec_range, 3, edit_range, edit_text, false, expected_dec_range[0][3], "no-3-GrowsOnlyWhenTypingAfter");

        run_test_once(dec_range, 0, edit_range, edit_text, true, expected_dec_range[1][0], "force-0-AlwaysGrowsWhenTypingAtEdges");
        run_test_once(dec_range, 1, edit_range, edit_text, true, expected_dec_range[1][1], "force-1-NeverGrowsWhenTypingAtEdges");
        run_test_once(dec_range, 2, edit_range, edit_text, true, expected_dec_range[1][2], "force-2-GrowsOnlyWhenTypingBefore");
        run_test_once(dec_range, 3, edit_range, edit_text, true, expected_dec_range[1][3], "force-3-GrowsOnlyWhenTypingAfter");
    }

    mod insert {
        use super::*;

        mod collapsed_dec {
            use super::*;

            #[test]
            fn before() {
                let _guard = ensure_no_disposables_are_leaked_in_test_suite();
                run_test(
                    Range::new(1, 4, 1, 4),
                    Range::new(1, 3, 1, 3),
                    "xx",
                    [
                        [Range::new(1, 6, 1, 6), Range::new(1, 6, 1, 6), Range::new(1, 6, 1, 6), Range::new(1, 6, 1, 6)],
                        [Range::new(1, 6, 1, 6), Range::new(1, 6, 1, 6), Range::new(1, 6, 1, 6), Range::new(1, 6, 1, 6)],
                    ],
                );
            }

            #[test]
            fn equal() {
                let _guard = ensure_no_disposables_are_leaked_in_test_suite();
                run_test(
                    Range::new(1, 4, 1, 4),
                    Range::new(1, 4, 1, 4),
                    "xx",
                    [
                        [Range::new(1, 4, 1, 6), Range::new(1, 6, 1, 6), Range::new(1, 4, 1, 4), Range::new(1, 6, 1, 6)],
                        [Range::new(1, 6, 1, 6), Range::new(1, 6, 1, 6), Range::new(1, 6, 1, 6), Range::new(1, 6, 1, 6)],
                    ],
                );
            }

            #[test]
            fn after() {
                let _guard = ensure_no_disposables_are_leaked_in_test_suite();
                run_test(
                    Range::new(1, 4, 1, 4),
                    Range::new(1, 5, 1, 5),
                    "xx",
                    [
                        [Range::new(1, 4, 1, 4), Range::new(1, 4, 1, 4), Range::new(1, 4, 1, 4), Range::new(1, 4, 1, 4)],
                        [Range::new(1, 4, 1, 4), Range::new(1, 4, 1, 4), Range::new(1, 4, 1, 4), Range::new(1, 4, 1, 4)],
                    ],
                );
            }
        }

        mod non_collapsed_dec {
            use super::*;

            #[test]
            fn before() {
                let _guard = ensure_no_disposables_are_leaked_in_test_suite();
                run_test(
                    Range::new(1, 4, 1, 9),
                    Range::new(1, 3, 1, 3),
                    "xx",
                    [
                        [Range::new(1, 6, 1, 11), Range::new(1, 6, 1, 11), Range::new(1, 6, 1, 11), Range::new(1, 6, 1, 11)],
                        [Range::new(1, 6, 1, 11), Range::new(1, 6, 1, 11), Range::new(1, 6, 1, 11), Range::new(1, 6, 1, 11)],
                    ],
                );
            }

            #[test]
            fn start() {
                let _guard = ensure_no_disposables_are_leaked_in_test_suite();
                run_test(
                    Range::new(1, 4, 1, 9),
                    Range::new(1, 4, 1, 4),
                    "xx",
                    [
                        [Range::new(1, 4, 1, 11), Range::new(1, 6, 1, 11), Range::new(1, 4, 1, 11), Range::new(1, 6, 1, 11)],
                        [Range::new(1, 6, 1, 11), Range::new(1, 6, 1, 11), Range::new(1, 6, 1, 11), Range::new(1, 6, 1, 11)],
                    ],
                );
            }

            #[test]
            fn inside() {
                let _guard = ensure_no_disposables_are_leaked_in_test_suite();
                run_test(
                    Range::new(1, 4, 1, 9),
                    Range::new(1, 5, 1, 5),
                    "xx",
                    [
                        [Range::new(1, 4, 1, 11), Range::new(1, 4, 1, 11), Range::new(1, 4, 1, 11), Range::new(1, 4, 1, 11)],
                        [Range::new(1, 4, 1, 11), Range::new(1, 4, 1, 11), Range::new(1, 4, 1, 11), Range::new(1, 4, 1, 11)],
                    ],
                );
            }

            #[test]
            fn end() {
                let _guard = ensure_no_disposables_are_leaked_in_test_suite();
                run_test(
                    Range::new(1, 4, 1, 9),
                    Range::new(1, 9, 1, 9),
                    "xx",
                    [
                        [Range::new(1, 4, 1, 11), Range::new(1, 4, 1, 9), Range::new(1, 4, 1, 9), Range::new(1, 4, 1, 11)],
                        [Range::new(1, 4, 1, 11), Range::new(1, 4, 1, 11), Range::new(1, 4, 1, 11), Range::new(1, 4, 1, 11)],
                    ],
                );
            }

            #[test]
            fn after() {
                let _guard = ensure_no_disposables_are_leaked_in_test_suite();
                run_test(
                    Range::new(1, 4, 1, 9),
                    Range::new(1, 10, 1, 10),
                    "xx",
                    [
                        [Range::new(1, 4, 1, 9), Range::new(1, 4, 1, 9), Range::new(1, 4, 1, 9), Range::new(1, 4, 1, 9)],
                        [Range::new(1, 4, 1, 9), Range::new(1, 4, 1, 9), Range::new(1, 4, 1, 9), Range::new(1, 4, 1, 9)],
                    ],
                );
            }
        }
    }

    mod delete {
        use super::*;

        mod collapsed_dec {
            use super::*;

            /// `edit.end < range.start`
            #[test]
            fn edit_end_lt_range_start() {
                let _guard = ensure_no_disposables_are_leaked_in_test_suite();
                run_test(
                    Range::new(1, 4, 1, 4),
                    Range::new(1, 1, 1, 3),
                    "",
                    [
                        [Range::new(1, 2, 1, 2), Range::new(1, 2, 1, 2), Range::new(1, 2, 1, 2), Range::new(1, 2, 1, 2)],
                        [Range::new(1, 2, 1, 2), Range::new(1, 2, 1, 2), Range::new(1, 2, 1, 2), Range::new(1, 2, 1, 2)],
                    ],
                );
            }

            /// `edit.end <= range.start`
            #[test]
            fn edit_end_le_range_start() {
                let _guard = ensure_no_disposables_are_leaked_in_test_suite();
                run_test(
                    Range::new(1, 4, 1, 4),
                    Range::new(1, 2, 1, 4),
                    "",
                    [
                        [Range::new(1, 2, 1, 2), Range::new(1, 2, 1, 2), Range::new(1, 2, 1, 2), Range::new(1, 2, 1, 2)],
                        [Range::new(1, 2, 1, 2), Range::new(1, 2, 1, 2), Range::new(1, 2, 1, 2), Range::new(1, 2, 1, 2)],
                    ],
                );
            }

            /// `edit.start < range.start && edit.end > range.end`
            #[test]
            fn edit_start_lt_range_start_and_edit_end_gt_range_end() {
                let _guard = ensure_no_disposables_are_leaked_in_test_suite();
                run_test(
                    Range::new(1, 4, 1, 4),
                    Range::new(1, 3, 1, 5),
                    "",
                    [
                        [Range::new(1, 3, 1, 3), Range::new(1, 3, 1, 3), Range::new(1, 3, 1, 3), Range::new(1, 3, 1, 3)],
                        [Range::new(1, 3, 1, 3), Range::new(1, 3, 1, 3), Range::new(1, 3, 1, 3), Range::new(1, 3, 1, 3)],
                    ],
                );
            }

            /// `edit.start >= range.end`
            #[test]
            fn edit_start_ge_range_end() {
                let _guard = ensure_no_disposables_are_leaked_in_test_suite();
                run_test(
                    Range::new(1, 4, 1, 4),
                    Range::new(1, 4, 1, 6),
                    "",
                    [
                        [Range::new(1, 4, 1, 4), Range::new(1, 4, 1, 4), Range::new(1, 4, 1, 4), Range::new(1, 4, 1, 4)],
                        [Range::new(1, 4, 1, 4), Range::new(1, 4, 1, 4), Range::new(1, 4, 1, 4), Range::new(1, 4, 1, 4)],
                    ],
                );
            }

            /// `edit.start > range.end`
            #[test]
            fn edit_start_gt_range_end() {
                let _guard = ensure_no_disposables_are_leaked_in_test_suite();
                run_test(
                    Range::new(1, 4, 1, 4),
                    Range::new(1, 5, 1, 7),
                    "",
                    [
                        [Range::new(1, 4, 1, 4), Range::new(1, 4, 1, 4), Range::new(1, 4, 1, 4), Range::new(1, 4, 1, 4)],
                        [Range::new(1, 4, 1, 4), Range::new(1, 4, 1, 4), Range::new(1, 4, 1, 4), Range::new(1, 4, 1, 4)],
                    ],
                );
            }
        }

        mod non_collapsed_dec {
            use super::*;

            /// `edit.end < range.start`
            #[test]
            fn edit_end_lt_range_start() {
                let _guard = ensure_no_disposables_are_leaked_in_test_suite();
                run_test(
                    Range::new(1, 4, 1, 9),
                    Range::new(1, 1, 1, 3),
                    "",
                    [
                        [Range::new(1, 2, 1, 7), Range::new(1, 2, 1, 7), Range::new(1, 2, 1, 7), Range::new(1, 2, 1, 7)],
                        [Range::new(1, 2, 1, 7), Range::new(1, 2, 1, 7), Range::new(1, 2, 1, 7), Range::new(1, 2, 1, 7)],
                    ],
                );
            }

            /// `edit.end <= range.start`
            #[test]
            fn edit_end_le_range_start() {
                let _guard = ensure_no_disposables_are_leaked_in_test_suite();
                run_test(
                    Range::new(1, 4, 1, 9),
                    Range::new(1, 2, 1, 4),
                    "",
                    [
                        [Range::new(1, 2, 1, 7), Range::new(1, 2, 1, 7), Range::new(1, 2, 1, 7), Range::new(1, 2, 1, 7)],
                        [Range::new(1, 2, 1, 7), Range::new(1, 2, 1, 7), Range::new(1, 2, 1, 7), Range::new(1, 2, 1, 7)],
                    ],
                );
            }

            /// `edit.start < range.start && edit.end < range.end`
            #[test]
            fn edit_start_lt_range_start_and_edit_end_lt_range_end() {
                let _guard = ensure_no_disposables_are_leaked_in_test_suite();
                run_test(
                    Range::new(1, 4, 1, 9),
                    Range::new(1, 3, 1, 5),
                    "",
                    [
                        [Range::new(1, 3, 1, 7), Range::new(1, 3, 1, 7), Range::new(1, 3, 1, 7), Range::new(1, 3, 1, 7)],
                        [Range::new(1, 3, 1, 7), Range::new(1, 3, 1, 7), Range::new(1, 3, 1, 7), Range::new(1, 3, 1, 7)],
                    ],
                );
            }

            /// `edit.start < range.start && edit.end == range.end`
            #[test]
            fn edit_start_lt_range_start_and_edit_end_eq_range_end() {
                let _guard = ensure_no_disposables_are_leaked_in_test_suite();
                run_test(
                    Range::new(1, 4, 1, 9),
                    Range::new(1, 3, 1, 9),
                    "",
                    [
                        [Range::new(1, 3, 1, 3), Range::new(1, 3, 1, 3), Range::new(1, 3, 1, 3), Range::new(1, 3, 1, 3)],
                        [Range::new(1, 3, 1, 3), Range::new(1, 3, 1, 3), Range::new(1, 3, 1, 3), Range::new(1, 3, 1, 3)],
                    ],
                );
            }

            /// `edit.start < range.start && edit.end > range.end`
            #[test]
            fn edit_start_lt_range_start_and_edit_end_gt_range_end() {
                let _guard = ensure_no_disposables_are_leaked_in_test_suite();
                run_test(
                    Range::new(1, 4, 1, 9),
                    Range::new(1, 3, 1, 10),
                    "",
                    [
                        [Range::new(1, 3, 1, 3), Range::new(1, 3, 1, 3), Range::new(1, 3, 1, 3), Range::new(1, 3, 1, 3)],
                        [Range::new(1, 3, 1, 3), Range::new(1, 3, 1, 3), Range::new(1, 3, 1, 3), Range::new(1, 3, 1, 3)],
                    ],
                );
            }

            /// `edit.start == range.start && edit.end < range.end`
            #[test]
            fn edit_start_eq_range_start_and_edit_end_lt_range_end() {
                let _guard = ensure_no_disposables_are_leaked_in_test_suite();
                run_test(
                    Range::new(1, 4, 1, 9),
                    Range::new(1, 4, 1, 6),
                    "",
                    [
                        [Range::new(1, 4, 1, 7), Range::new(1, 4, 1, 7), Range::new(1, 4, 1, 7), Range::new(1, 4, 1, 7)],
                        [Range::new(1, 4, 1, 7), Range::new(1, 4, 1, 7), Range::new(1, 4, 1, 7), Range::new(1, 4, 1, 7)],
                    ],
                );
            }

            /// `edit.start == range.start && edit.end == range.end`
            #[test]
            fn edit_start_eq_range_start_and_edit_end_eq_range_end() {
                let _guard = ensure_no_disposables_are_leaked_in_test_suite();
                run_test(
                    Range::new(1, 4, 1, 9),
                    Range::new(1, 4, 1, 9),
                    "",
                    [
                        [Range::new(1, 4, 1, 4), Range::new(1, 4, 1, 4), Range::new(1, 4, 1, 4), Range::new(1, 4, 1, 4)],
                        [Range::new(1, 4, 1, 4), Range::new(1, 4, 1, 4), Range::new(1, 4, 1, 4), Range::new(1, 4, 1, 4)],
                    ],
                );
            }

            /// `edit.start == range.start && edit.end > range.end`
            #[test]
            fn edit_start_eq_range_start_and_edit_end_gt_range_end() {
                let _guard = ensure_no_disposables_are_leaked_in_test_suite();
                run_test(
                    Range::new(1, 4, 1, 9),
                    Range::new(1, 4, 1, 10),
                    "",
                    [
                        [Range::new(1, 4, 1, 4), Range::new(1, 4, 1, 4), Range::new(1, 4, 1, 4), Range::new(1, 4, 1, 4)],
                        [Range::new(1, 4, 1, 4), Range::new(1, 4, 1, 4), Range::new(1, 4, 1, 4), Range::new(1, 4, 1, 4)],
                    ],
                );
            }

            /// `edit.start > range.start && edit.start < range.end && edit.end < range.end`
            #[test]
            fn edit_start_gt_range_start_and_edit_start_lt_range_end_and_edit_end_lt_range_end() {
                let _guard = ensure_no_disposables_are_leaked_in_test_suite();
                run_test(
                    Range::new(1, 4, 1, 9),
                    Range::new(1, 5, 1, 7),
                    "",
                    [
                        [Range::new(1, 4, 1, 7), Range::new(1, 4, 1, 7), Range::new(1, 4, 1, 7), Range::new(1, 4, 1, 7)],
                        [Range::new(1, 4, 1, 7), Range::new(1, 4, 1, 7), Range::new(1, 4, 1, 7), Range::new(1, 4, 1, 7)],
                    ],
                );
            }

            /// `edit.start > range.start && edit.start < range.end && edit.end == range.end`
            #[test]
            fn edit_start_gt_range_start_and_edit_start_lt_range_end_and_edit_end_eq_range_end() {
                let _guard = ensure_no_disposables_are_leaked_in_test_suite();
                run_test(
                    Range::new(1, 4, 1, 9),
                    Range::new(1, 5, 1, 9),
                    "",
                    [
                        [Range::new(1, 4, 1, 5), Range::new(1, 4, 1, 5), Range::new(1, 4, 1, 5), Range::new(1, 4, 1, 5)],
                        [Range::new(1, 4, 1, 5), Range::new(1, 4, 1, 5), Range::new(1, 4, 1, 5), Range::new(1, 4, 1, 5)],
                    ],
                );
            }

            /// `edit.start > range.start && edit.start < range.end && edit.end > range.end`
            #[test]
            fn edit_start_gt_range_start_and_edit_start_lt_range_end_and_edit_end_gt_range_end() {
                let _guard = ensure_no_disposables_are_leaked_in_test_suite();
                run_test(
                    Range::new(1, 4, 1, 9),
                    Range::new(1, 5, 1, 10),
                    "",
                    [
                        [Range::new(1, 4, 1, 5), Range::new(1, 4, 1, 5), Range::new(1, 4, 1, 5), Range::new(1, 4, 1, 5)],
                        [Range::new(1, 4, 1, 5), Range::new(1, 4, 1, 5), Range::new(1, 4, 1, 5), Range::new(1, 4, 1, 5)],
                    ],
                );
            }

            /// `edit.start == range.end`
            #[test]
            fn edit_start_eq_range_end() {
                let _guard = ensure_no_disposables_are_leaked_in_test_suite();
                run_test(
                    Range::new(1, 4, 1, 9),
                    Range::new(1, 9, 1, 11),
                    "",
                    [
                        [Range::new(1, 4, 1, 9), Range::new(1, 4, 1, 9), Range::new(1, 4, 1, 9), Range::new(1, 4, 1, 9)],
                        [Range::new(1, 4, 1, 9), Range::new(1, 4, 1, 9), Range::new(1, 4, 1, 9), Range::new(1, 4, 1, 9)],
                    ],
                );
            }

            /// `edit.start > range.end`
            #[test]
            fn edit_start_gt_range_end() {
                let _guard = ensure_no_disposables_are_leaked_in_test_suite();
                run_test(
                    Range::new(1, 4, 1, 9),
                    Range::new(1, 10, 1, 11),
                    "",
                    [
                        [Range::new(1, 4, 1, 9), Range::new(1, 4, 1, 9), Range::new(1, 4, 1, 9), Range::new(1, 4, 1, 9)],
                        [Range::new(1, 4, 1, 9), Range::new(1, 4, 1, 9), Range::new(1, 4, 1, 9), Range::new(1, 4, 1, 9)],
                    ],
                );
            }
        }
    }

    mod replace_short {
        use super::*;

        mod collapsed_dec {
            use super::*;

            /// `edit.end < range.start`
            #[test]
            fn edit_end_lt_range_start() {
                let _guard = ensure_no_disposables_are_leaked_in_test_suite();
                run_test(
                    Range::new(1, 4, 1, 4),
                    Range::new(1, 1, 1, 3),
                    "c",
                    [
                        [Range::new(1, 3, 1, 3), Range::new(1, 3, 1, 3), Range::new(1, 3, 1, 3), Range::new(1, 3, 1, 3)],
                        [Range::new(1, 3, 1, 3), Range::new(1, 3, 1, 3), Range::new(1, 3, 1, 3), Range::new(1, 3, 1, 3)],
                    ],
                );
            }

            /// `edit.end <= range.start`
            #[test]
            fn edit_end_le_range_start() {
                let _guard = ensure_no_disposables_are_leaked_in_test_suite();
                run_test(
                    Range::new(1, 4, 1, 4),
                    Range::new(1, 2, 1, 4),
                    "c",
                    [
                        [Range::new(1, 3, 1, 3), Range::new(1, 3, 1, 3), Range::new(1, 3, 1, 3), Range::new(1, 3, 1, 3)],
                        [Range::new(1, 3, 1, 3), Range::new(1, 3, 1, 3), Range::new(1, 3, 1, 3), Range::new(1, 3, 1, 3)],
                    ],
                );
            }

            /// `edit.start < range.start && edit.end > range.end`
            #[test]
            fn edit_start_lt_range_start_and_edit_end_gt_range_end() {
                let _guard = ensure_no_disposables_are_leaked_in_test_suite();
                run_test(
                    Range::new(1, 4, 1, 4),
                    Range::new(1, 3, 1, 5),
                    "c",
                    [
                        [Range::new(1, 4, 1, 4), Range::new(1, 4, 1, 4), Range::new(1, 4, 1, 4), Range::new(1, 4, 1, 4)],
                        [Range::new(1, 4, 1, 4), Range::new(1, 4, 1, 4), Range::new(1, 4, 1, 4), Range::new(1, 4, 1, 4)],
                    ],
                );
            }

            /// `edit.start >= range.end`
            #[test]
            fn edit_start_ge_range_end() {
                let _guard = ensure_no_disposables_are_leaked_in_test_suite();
                run_test(
                    Range::new(1, 4, 1, 4),
                    Range::new(1, 4, 1, 6),
                    "c",
                    [
                        [Range::new(1, 4, 1, 4), Range::new(1, 4, 1, 4), Range::new(1, 4, 1, 4), Range::new(1, 4, 1, 4)],
                        [Range::new(1, 5, 1, 5), Range::new(1, 5, 1, 5), Range::new(1, 5, 1, 5), Range::new(1, 5, 1, 5)],
                    ],
                );
            }

            /// `edit.start > range.end`
            #[test]
            fn edit_start_gt_range_end() {
                let _guard = ensure_no_disposables_are_leaked_in_test_suite();
                run_test(
                    Range::new(1, 4, 1, 4),
                    Range::new(1, 5, 1, 7),
                    "c",
                    [
                        [Range::new(1, 4, 1, 4), Range::new(1, 4, 1, 4), Range::new(1, 4, 1, 4), Range::new(1, 4, 1, 4)],
                        [Range::new(1, 4, 1, 4), Range::new(1, 4, 1, 4), Range::new(1, 4, 1, 4), Range::new(1, 4, 1, 4)],
                    ],
                );
            }
        }

        mod non_collapsed_dec {
            use super::*;

            /// `edit.end < range.start`
            #[test]
            fn edit_end_lt_range_start() {
                let _guard = ensure_no_disposables_are_leaked_in_test_suite();
                run_test(
                    Range::new(1, 4, 1, 9),
                    Range::new(1, 1, 1, 3),
                    "c",
                    [
                        [Range::new(1, 3, 1, 8), Range::new(1, 3, 1, 8), Range::new(1, 3, 1, 8), Range::new(1, 3, 1, 8)],
                        [Range::new(1, 3, 1, 8), Range::new(1, 3, 1, 8), Range::new(1, 3, 1, 8), Range::new(1, 3, 1, 8)],
                    ],
                );
            }

            /// `edit.end <= range.start`
            #[test]
            fn edit_end_le_range_start() {
                let _guard = ensure_no_disposables_are_leaked_in_test_suite();
                run_test(
                    Range::new(1, 4, 1, 9),
                    Range::new(1, 2, 1, 4),
                    "c",
                    [
                        [Range::new(1, 3, 1, 8), Range::new(1, 3, 1, 8), Range::new(1, 3, 1, 8), Range::new(1, 3, 1, 8)],
                        [Range::new(1, 3, 1, 8), Range::new(1, 3, 1, 8), Range::new(1, 3, 1, 8), Range::new(1, 3, 1, 8)],
                    ],
                );
            }

            /// `edit.start < range.start && edit.end < range.end`
            #[test]
            fn edit_start_lt_range_start_and_edit_end_lt_range_end() {
                let _guard = ensure_no_disposables_are_leaked_in_test_suite();
                run_test(
                    Range::new(1, 4, 1, 9),
                    Range::new(1, 3, 1, 5),
                    "c",
                    [
                        [Range::new(1, 4, 1, 8), Range::new(1, 4, 1, 8), Range::new(1, 4, 1, 8), Range::new(1, 4, 1, 8)],
                        [Range::new(1, 4, 1, 8), Range::new(1, 4, 1, 8), Range::new(1, 4, 1, 8), Range::new(1, 4, 1, 8)],
                    ],
                );
            }

            /// `edit.start < range.start && edit.end == range.end`
            #[test]
            fn edit_start_lt_range_start_and_edit_end_eq_range_end() {
                let _guard = ensure_no_disposables_are_leaked_in_test_suite();
                run_test(
                    Range::new(1, 4, 1, 9),
                    Range::new(1, 3, 1, 9),
                    "c",
                    [
                        [Range::new(1, 4, 1, 4), Range::new(1, 4, 1, 4), Range::new(1, 4, 1, 4), Range::new(1, 4, 1, 4)],
                        [Range::new(1, 4, 1, 4), Range::new(1, 4, 1, 4), Range::new(1, 4, 1, 4), Range::new(1, 4, 1, 4)],
                    ],
                );
            }

            /// `edit.start < range.start && edit.end > range.end`
            #[test]
            fn edit_start_lt_range_start_and_edit_end_gt_range_end() {
                let _guard = ensure_no_disposables_are_leaked_in_test_suite();
                run_test(
                    Range::new(1, 4, 1, 9),
                    Range::new(1, 3, 1, 10),
                    "c",
                    [
                        [Range::new(1, 4, 1, 4), Range::new(1, 4, 1, 4), Range::new(1, 4, 1, 4), Range::new(1, 4, 1, 4)],
                        [Range::new(1, 4, 1, 4), Range::new(1, 4, 1, 4), Range::new(1, 4, 1, 4), Range::new(1, 4, 1, 4)],
                    ],
                );
            }

            /// `edit.start == range.start && edit.end < range.end`
            #[test]
            fn edit_start_eq_range_start_and_edit_end_lt_range_end() {
                let _guard = ensure_no_disposables_are_leaked_in_test_suite();
                run_test(
                    Range::new(1, 4, 1, 9),
                    Range::new(1, 4, 1, 6),
                    "c",
                    [
                        [Range::new(1, 4, 1, 8), Range::new(1, 4, 1, 8), Range::new(1, 4, 1, 8), Range::new(1, 4, 1, 8)],
                        [Range::new(1, 5, 1, 8), Range::new(1, 5, 1, 8), Range::new(1, 5, 1, 8), Range::new(1, 5, 1, 8)],
                    ],
                );
            }

            /// `edit.start == range.start && edit.end == range.end`
            #[test]
            fn edit_start_eq_range_start_and_edit_end_eq_range_end() {
                let _guard = ensure_no_disposables_are_leaked_in_test_suite();
                run_test(
                    Range::new(1, 4, 1, 9),
                    Range::new(1, 4, 1, 9),
                    "c",
                    [
                        [Range::new(1, 4, 1, 5), Range::new(1, 4, 1, 5), Range::new(1, 4, 1, 5), Range::new(1, 4, 1, 5)],
                        [Range::new(1, 5, 1, 5), Range::new(1, 5, 1, 5), Range::new(1, 5, 1, 5), Range::new(1, 5, 1, 5)],
                    ],
                );
            }

            /// `edit.start == range.start && edit.end > range.end`
            #[test]
            fn edit_start_eq_range_start_and_edit_end_gt_range_end() {
                let _guard = ensure_no_disposables_are_leaked_in_test_suite();
                run_test(
                    Range::new(1, 4, 1, 9),
                    Range::new(1, 4, 1, 10),
                    "c",
                    [
                        [Range::new(1, 4, 1, 5), Range::new(1, 4, 1, 5), Range::new(1, 4, 1, 5), Range::new(1, 4, 1, 5)],
                        [Range::new(1, 5, 1, 5), Range::new(1, 5, 1, 5), Range::new(1, 5, 1, 5), Range::new(1, 5, 1, 5)],
                    ],
                );
            }

            /// `edit.start > range.start && edit.start < range.end && edit.end < range.end`
            #[test]
            fn edit_start_gt_range_start_and_edit_start_lt_range_end_and_edit_end_lt_range_end() {
                let _guard = ensure_no_disposables_are_leaked_in_test_suite();
                run_test(
                    Range::new(1, 4, 1, 9),
                    Range::new(1, 5, 1, 7),
                    "c",
                    [
                        [Range::new(1, 4, 1, 8), Range::new(1, 4, 1, 8), Range::new(1, 4, 1, 8), Range::new(1, 4, 1, 8)],
                        [Range::new(1, 4, 1, 8), Range::new(1, 4, 1, 8), Range::new(1, 4, 1, 8), Range::new(1, 4, 1, 8)],
                    ],
                );
            }

            /// `edit.start > range.start && edit.start < range.end && edit.end == range.end`
            #[test]
            fn edit_start_gt_range_start_and_edit_start_lt_range_end_and_edit_end_eq_range_end() {
                let _guard = ensure_no_disposables_are_leaked_in_test_suite();
                run_test(
                    Range::new(1, 4, 1, 9),
                    Range::new(1, 5, 1, 9),
                    "c",
                    [
                        [Range::new(1, 4, 1, 6), Range::new(1, 4, 1, 6), Range::new(1, 4, 1, 6), Range::new(1, 4, 1, 6)],
                        [Range::new(1, 4, 1, 6), Range::new(1, 4, 1, 6), Range::new(1, 4, 1, 6), Range::new(1, 4, 1, 6)],
                    ],
                );
            }

            /// `edit.start > range.start && edit.start < range.end && edit.end > range.end`
            #[test]
            fn edit_start_gt_range_start_and_edit_start_lt_range_end_and_edit_end_gt_range_end() {
                let _guard = ensure_no_disposables_are_leaked_in_test_suite();
                run_test(
                    Range::new(1, 4, 1, 9),
                    Range::new(1, 5, 1, 10),
                    "c",
                    [
                        [Range::new(1, 4, 1, 6), Range::new(1, 4, 1, 6), Range::new(1, 4, 1, 6), Range::new(1, 4, 1, 6)],
                        [Range::new(1, 4, 1, 6), Range::new(1, 4, 1, 6), Range::new(1, 4, 1, 6), Range::new(1, 4, 1, 6)],
                    ],
                );
            }

            /// `edit.start == range.end`
            #[test]
            fn edit_start_eq_range_end() {
                let _guard = ensure_no_disposables_are_leaked_in_test_suite();
                run_test(
                    Range::new(1, 4, 1, 9),
                    Range::new(1, 9, 1, 11),
                    "c",
                    [
                        [Range::new(1, 4, 1, 9), Range::new(1, 4, 1, 9), Range::new(1, 4, 1, 9), Range::new(1, 4, 1, 9)],
                        [Range::new(1, 4, 1, 10), Range::new(1, 4, 1, 10), Range::new(1, 4, 1, 10), Range::new(1, 4, 1, 10)],
                    ],
                );
            }

            /// `edit.start > range.end`
            #[test]
            fn edit_start_gt_range_end() {
                let _guard = ensure_no_disposables_are_leaked_in_test_suite();
                run_test(
                    Range::new(1, 4, 1, 9),
                    Range::new(1, 10, 1, 11),
                    "c",
                    [
                        [Range::new(1, 4, 1, 9), Range::new(1, 4, 1, 9), Range::new(1, 4, 1, 9), Range::new(1, 4, 1, 9)],
                        [Range::new(1, 4, 1, 9), Range::new(1, 4, 1, 9), Range::new(1, 4, 1, 9), Range::new(1, 4, 1, 9)],
                    ],
                );
            }
        }
    }

    mod replace_long {
        use super::*;

        mod collapsed_dec {
            use super::*;

            /// `edit.end < range.start`
            #[test]
            fn edit_end_lt_range_start() {
                let _guard = ensure_no_disposables_are_leaked_in_test_suite();
                run_test(
                    Range::new(1, 4, 1, 4),
                    Range::new(1, 1, 1, 3),
                    "cccc",
                    [
                        [Range::new(1, 6, 1, 6), Range::new(1, 6, 1, 6), Range::new(1, 6, 1, 6), Range::new(1, 6, 1, 6)],
                        [Range::new(1, 6, 1, 6), Range::new(1, 6, 1, 6), Range::new(1, 6, 1, 6), Range::new(1, 6, 1, 6)],
                    ],
                );
            }

            /// `edit.end <= range.start`
            #[test]
            fn edit_end_le_range_start() {
                let _guard = ensure_no_disposables_are_leaked_in_test_suite();
                run_test(
                    Range::new(1, 4, 1, 4),
                    Range::new(1, 2, 1, 4),
                    "cccc",
                    [
                        [Range::new(1, 4, 1, 6), Range::new(1, 6, 1, 6), Range::new(1, 4, 1, 4), Range::new(1, 6, 1, 6)],
                        [Range::new(1, 6, 1, 6), Range::new(1, 6, 1, 6), Range::new(1, 6, 1, 6), Range::new(1, 6, 1, 6)],
                    ],
                );
            }

            /// `edit.start < range.start && edit.end > range.end`
            #[test]
            fn edit_start_lt_range_start_and_edit_end_gt_range_end() {
                let _guard = ensure_no_disposables_are_leaked_in_test_suite();
                run_test(
                    Range::new(1, 4, 1, 4),
                    Range::new(1, 3, 1, 5),
                    "cccc",
                    [
                        [Range::new(1, 4, 1, 4), Range::new(1, 4, 1, 4), Range::new(1, 4, 1, 4), Range::new(1, 4, 1, 4)],
                        [Range::new(1, 7, 1, 7), Range::new(1, 7, 1, 7), Range::new(1, 7, 1, 7), Range::new(1, 7, 1, 7)],
                    ],
                );
            }

            /// `edit.start >= range.end`
            #[test]
            fn edit_start_ge_range_end() {
                let _guard = ensure_no_disposables_are_leaked_in_test_suite();
                run_test(
                    Range::new(1, 4, 1, 4),
                    Range::new(1, 4, 1, 6),
                    "cccc",
                    [
                        [Range::new(1, 4, 1, 4), Range::new(1, 4, 1, 4), Range::new(1, 4, 1, 4), Range::new(1, 4, 1, 4)],
                        [Range::new(1, 8, 1, 8), Range::new(1, 8, 1, 8), Range::new(1, 8, 1, 8), Range::new(1, 8, 1, 8)],
                    ],
                );
            }

            /// `edit.start > range.end`
            #[test]
            fn edit_start_gt_range_end() {
                let _guard = ensure_no_disposables_are_leaked_in_test_suite();
                run_test(
                    Range::new(1, 4, 1, 4),
                    Range::new(1, 5, 1, 7),
                    "cccc",
                    [
                        [Range::new(1, 4, 1, 4), Range::new(1, 4, 1, 4), Range::new(1, 4, 1, 4), Range::new(1, 4, 1, 4)],
                        [Range::new(1, 4, 1, 4), Range::new(1, 4, 1, 4), Range::new(1, 4, 1, 4), Range::new(1, 4, 1, 4)],
                    ],
                );
            }
        }

        mod non_collapsed_dec {
            use super::*;

            /// `edit.end < range.start`
            #[test]
            fn edit_end_lt_range_start() {
                let _guard = ensure_no_disposables_are_leaked_in_test_suite();
                run_test(
                    Range::new(1, 4, 1, 9),
                    Range::new(1, 1, 1, 3),
                    "cccc",
                    [
                        [Range::new(1, 6, 1, 11), Range::new(1, 6, 1, 11), Range::new(1, 6, 1, 11), Range::new(1, 6, 1, 11)],
                        [Range::new(1, 6, 1, 11), Range::new(1, 6, 1, 11), Range::new(1, 6, 1, 11), Range::new(1, 6, 1, 11)],
                    ],
                );
            }

            /// `edit.end <= range.start`
            #[test]
            fn edit_end_le_range_start() {
                let _guard = ensure_no_disposables_are_leaked_in_test_suite();
                run_test(
                    Range::new(1, 4, 1, 9),
                    Range::new(1, 2, 1, 4),
                    "cccc",
                    [
                        [Range::new(1, 4, 1, 11), Range::new(1, 6, 1, 11), Range::new(1, 4, 1, 11), Range::new(1, 6, 1, 11)],
                        [Range::new(1, 6, 1, 11), Range::new(1, 6, 1, 11), Range::new(1, 6, 1, 11), Range::new(1, 6, 1, 11)],
                    ],
                );
            }

            /// `edit.start < range.start && edit.end < range.end`
            #[test]
            fn edit_start_lt_range_start_and_edit_end_lt_range_end() {
                let _guard = ensure_no_disposables_are_leaked_in_test_suite();
                run_test(
                    Range::new(1, 4, 1, 9),
                    Range::new(1, 3, 1, 5),
                    "cccc",
                    [
                        [Range::new(1, 4, 1, 11), Range::new(1, 4, 1, 11), Range::new(1, 4, 1, 11), Range::new(1, 4, 1, 11)],
                        [Range::new(1, 7, 1, 11), Range::new(1, 7, 1, 11), Range::new(1, 7, 1, 11), Range::new(1, 7, 1, 11)],
                    ],
                );
            }

            /// `edit.start < range.start && edit.end == range.end`
            #[test]
            fn edit_start_lt_range_start_and_edit_end_eq_range_end() {
                let _guard = ensure_no_disposables_are_leaked_in_test_suite();
                run_test(
                    Range::new(1, 4, 1, 9),
                    Range::new(1, 3, 1, 9),
                    "cccc",
                    [
                        [Range::new(1, 4, 1, 7), Range::new(1, 4, 1, 7), Range::new(1, 4, 1, 7), Range::new(1, 4, 1, 7)],
                        [Range::new(1, 7, 1, 7), Range::new(1, 7, 1, 7), Range::new(1, 7, 1, 7), Range::new(1, 7, 1, 7)],
                    ],
                );
            }

            /// `edit.start < range.start && edit.end > range.end`
            #[test]
            fn edit_start_lt_range_start_and_edit_end_gt_range_end() {
                let _guard = ensure_no_disposables_are_leaked_in_test_suite();
                run_test(
                    Range::new(1, 4, 1, 9),
                    Range::new(1, 3, 1, 10),
                    "cccc",
                    [
                        [Range::new(1, 4, 1, 7), Range::new(1, 4, 1, 7), Range::new(1, 4, 1, 7), Range::new(1, 4, 1, 7)],
                        [Range::new(1, 7, 1, 7), Range::new(1, 7, 1, 7), Range::new(1, 7, 1, 7), Range::new(1, 7, 1, 7)],
                    ],
                );
            }

            /// `edit.start == range.start && edit.end < range.end`
            #[test]
            fn edit_start_eq_range_start_and_edit_end_lt_range_end() {
                let _guard = ensure_no_disposables_are_leaked_in_test_suite();
                run_test(
                    Range::new(1, 4, 1, 9),
                    Range::new(1, 4, 1, 6),
                    "cccc",
                    [
                        [Range::new(1, 4, 1, 11), Range::new(1, 4, 1, 11), Range::new(1, 4, 1, 11), Range::new(1, 4, 1, 11)],
                        [Range::new(1, 8, 1, 11), Range::new(1, 8, 1, 11), Range::new(1, 8, 1, 11), Range::new(1, 8, 1, 11)],
                    ],
                );
            }

            /// `edit.start == range.start && edit.end == range.end`
            #[test]
            fn edit_start_eq_range_start_and_edit_end_eq_range_end() {
                let _guard = ensure_no_disposables_are_leaked_in_test_suite();
                run_test(
                    Range::new(1, 4, 1, 9),
                    Range::new(1, 4, 1, 9),
                    "cccc",
                    [
                        [Range::new(1, 4, 1, 8), Range::new(1, 4, 1, 8), Range::new(1, 4, 1, 8), Range::new(1, 4, 1, 8)],
                        [Range::new(1, 8, 1, 8), Range::new(1, 8, 1, 8), Range::new(1, 8, 1, 8), Range::new(1, 8, 1, 8)],
                    ],
                );
            }

            /// `edit.start == range.start && edit.end > range.end`
            #[test]
            fn edit_start_eq_range_start_and_edit_end_gt_range_end() {
                let _guard = ensure_no_disposables_are_leaked_in_test_suite();
                run_test(
                    Range::new(1, 4, 1, 9),
                    Range::new(1, 4, 1, 10),
                    "cccc",
                    [
                        [Range::new(1, 4, 1, 8), Range::new(1, 4, 1, 8), Range::new(1, 4, 1, 8), Range::new(1, 4, 1, 8)],
                        [Range::new(1, 8, 1, 8), Range::new(1, 8, 1, 8), Range::new(1, 8, 1, 8), Range::new(1, 8, 1, 8)],
                    ],
                );
            }

            /// `edit.start > range.start && edit.start < range.end && edit.end < range.end`
            #[test]
            fn edit_start_gt_range_start_and_edit_start_lt_range_end_and_edit_end_lt_range_end() {
                let _guard = ensure_no_disposables_are_leaked_in_test_suite();
                run_test(
                    Range::new(1, 4, 1, 9),
                    Range::new(1, 5, 1, 7),
                    "cccc",
                    [
                        [Range::new(1, 4, 1, 11), Range::new(1, 4, 1, 11), Range::new(1, 4, 1, 11), Range::new(1, 4, 1, 11)],
                        [Range::new(1, 4, 1, 11), Range::new(1, 4, 1, 11), Range::new(1, 4, 1, 11), Range::new(1, 4, 1, 11)],
                    ],
                );
            }

            /// `edit.start > range.start && edit.start < range.end && edit.end == range.end`
            #[test]
            fn edit_start_gt_range_start_and_edit_start_lt_range_end_and_edit_end_eq_range_end() {
                let _guard = ensure_no_disposables_are_leaked_in_test_suite();
                run_test(
                    Range::new(1, 4, 1, 9),
                    Range::new(1, 5, 1, 9),
                    "cccc",
                    [
                        [Range::new(1, 4, 1, 9), Range::new(1, 4, 1, 9), Range::new(1, 4, 1, 9), Range::new(1, 4, 1, 9)],
                        [Range::new(1, 4, 1, 9), Range::new(1, 4, 1, 9), Range::new(1, 4, 1, 9), Range::new(1, 4, 1, 9)],
                    ],
                );
            }

            /// `edit.start > range.start && edit.start < range.end && edit.end > range.end`
            #[test]
            fn edit_start_gt_range_start_and_edit_start_lt_range_end_and_edit_end_gt_range_end() {
                let _guard = ensure_no_disposables_are_leaked_in_test_suite();
                run_test(
                    Range::new(1, 4, 1, 9),
                    Range::new(1, 5, 1, 10),
                    "cccc",
                    [
                        [Range::new(1, 4, 1, 9), Range::new(1, 4, 1, 9), Range::new(1, 4, 1, 9), Range::new(1, 4, 1, 9)],
                        [Range::new(1, 4, 1, 9), Range::new(1, 4, 1, 9), Range::new(1, 4, 1, 9), Range::new(1, 4, 1, 9)],
                    ],
                );
            }

            /// `edit.start == range.end`
            #[test]
            fn edit_start_eq_range_end() {
                let _guard = ensure_no_disposables_are_leaked_in_test_suite();
                run_test(
                    Range::new(1, 4, 1, 9),
                    Range::new(1, 9, 1, 11),
                    "cccc",
                    [
                        [Range::new(1, 4, 1, 9), Range::new(1, 4, 1, 9), Range::new(1, 4, 1, 9), Range::new(1, 4, 1, 9)],
                        [Range::new(1, 4, 1, 13), Range::new(1, 4, 1, 13), Range::new(1, 4, 1, 13), Range::new(1, 4, 1, 13)],
                    ],
                );
            }

            /// `edit.start > range.end`
            #[test]
            fn edit_start_gt_range_end() {
                let _guard = ensure_no_disposables_are_leaked_in_test_suite();
                run_test(
                    Range::new(1, 4, 1, 9),
                    Range::new(1, 10, 1, 11),
                    "cccc",
                    [
                        [Range::new(1, 4, 1, 9), Range::new(1, 4, 1, 9), Range::new(1, 4, 1, 9), Range::new(1, 4, 1, 9)],
                        [Range::new(1, 4, 1, 9), Range::new(1, 4, 1, 9), Range::new(1, 4, 1, 9), Range::new(1, 4, 1, 9)],
                    ],
                );
            }
        }
    }

}

mod delta_decorations {
    use super::*;

    /// `ILightWeightDecoration`.
    #[derive(Clone, Debug, PartialEq)]
    struct LightWeightDecoration {
        id: String,
        range: Range,
    }

    fn decoration(id: &str, start_line_number: i32, start_column: i32, end_line_number: i32, end_colum: i32) -> LightWeightDecoration {
        LightWeightDecoration {
            id: id.to_owned(),
            range: Range::new(start_line_number, start_column, end_line_number, end_colum),
        }
    }

    fn to_model_delta_decoration(dec: &LightWeightDecoration) -> ModelDeltaDecoration {
        ModelDeltaDecoration {
            range: dec.range,
            options: ModelDecorationOptions::register(&ModelDecorationOptionsInit {
                description: "test".to_owned(),
                class_name: Some(dec.id.clone()),
                ..Default::default()
            }),
        }
    }

    /// `strcmp`: a comparação de cadeias do JavaScript (por unidade UTF-16; os ids aqui são ASCII).
    fn strcmp(a: &str, b: &str) -> std::cmp::Ordering {
        a.cmp(b)
    }

    fn read_model_decorations(model: &TextModel, ids: &[String]) -> Vec<LightWeightDecoration> {
        ids.iter()
            .map(|id| LightWeightDecoration {
                range: model.get_decoration_range(id).unwrap(),
                id: model.get_decoration_options(id).unwrap().class_name.clone().unwrap(),
            })
            .collect()
    }

    fn test_delta_decorations(text: &[&str], mut decorations: Vec<LightWeightDecoration>, mut new_decorations: Vec<LightWeightDecoration>) {
        let model = create_text_model(&text.join("\n"));

        // Add initial decorations & assert they are added
        let initial_ids = model.delta_decorations(&[], &decorations.iter().map(to_model_delta_decoration).collect::<Vec<_>>(), 0);
        let mut actual_decorations = read_model_decorations(&model, &initial_ids);

        assert_eq!(initial_ids.len(), decorations.len(), "returns expected cnt of ids");
        assert_eq!(initial_ids.len(), model.get_all_decorations(0, false, false).len(), "does not leak decorations");
        actual_decorations.sort_by(|a, b| strcmp(&a.id, &b.id));
        decorations.sort_by(|a, b| strcmp(&a.id, &b.id));
        assert_eq!(actual_decorations, decorations);

        let new_ids = model.delta_decorations(&initial_ids, &new_decorations.iter().map(to_model_delta_decoration).collect::<Vec<_>>(), 0);
        let mut actual_new_decorations = read_model_decorations(&model, &new_ids);

        assert_eq!(new_ids.len(), new_decorations.len(), "returns expected cnt of ids");
        assert_eq!(new_ids.len(), model.get_all_decorations(0, false, false).len(), "does not leak decorations");
        actual_new_decorations.sort_by(|a, b| strcmp(&a.id, &b.id));
        new_decorations.sort_by(|a, b| strcmp(&a.id, &b.id));
        // O original confere `actualDecorations` contra `decorations` de novo (e não as novas).
        assert_eq!(actual_decorations, decorations);

        model.dispose();
    }

    fn range(start_line_number: i32, start_column: i32, end_line_number: i32, end_column: i32) -> Range {
        Range::new(start_line_number, start_column, end_line_number, end_column)
    }

    #[test]
    fn result_respects_input() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let model = create_text_model(&["Hello world,", "How are you?"].join("\n"));

        let ids = model.delta_decorations(
            &[],
            &[
                to_model_delta_decoration(&decoration("a", 1, 1, 1, 12)),
                to_model_delta_decoration(&decoration("b", 2, 1, 2, 13)),
            ],
            0,
        );

        assert_eq!(model.get_decoration_range(&ids[0]), Some(range(1, 1, 1, 12)));
        assert_eq!(model.get_decoration_range(&ids[1]), Some(range(2, 1, 2, 13)));

        model.dispose();
    }

    #[test]
    fn delta_decorations_1() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_delta_decorations(
            &["This is a text", "That has multiple lines", "And is very friendly", "Towards testing"],
            vec![
                decoration("a", 1, 1, 1, 2),
                decoration("b", 1, 1, 1, 15),
                decoration("c", 1, 1, 2, 1),
                decoration("d", 1, 1, 2, 24),
                decoration("e", 2, 1, 2, 24),
                decoration("f", 2, 1, 4, 16),
            ],
            vec![
                decoration("x", 1, 1, 1, 2),
                decoration("b", 1, 1, 1, 15),
                decoration("c", 1, 1, 2, 1),
                decoration("d", 1, 1, 2, 24),
                decoration("e", 2, 1, 2, 21),
                decoration("f", 2, 17, 4, 16),
            ],
        );
    }

    #[test]
    fn delta_decorations_2() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_delta_decorations(
            &["This is a text", "That has multiple lines", "And is very friendly", "Towards testing"],
            vec![
                decoration("a", 1, 1, 1, 2),
                decoration("b", 1, 2, 1, 3),
                decoration("c", 1, 3, 1, 4),
                decoration("d", 1, 4, 1, 5),
                decoration("e", 1, 5, 1, 6),
            ],
            vec![
                decoration("a", 1, 2, 1, 3),
                decoration("b", 1, 3, 1, 4),
                decoration("c", 1, 4, 1, 5),
                decoration("d", 1, 5, 1, 6),
            ],
        );
    }

    #[test]
    fn delta_decorations_3() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_delta_decorations(
            &["This is a text", "That has multiple lines", "And is very friendly", "Towards testing"],
            vec![
                decoration("a", 1, 1, 1, 2),
                decoration("b", 1, 2, 1, 3),
                decoration("c", 1, 3, 1, 4),
                decoration("d", 1, 4, 1, 5),
                decoration("e", 1, 5, 1, 6),
            ],
            vec![],
        );
    }

    /// `issue #4317: editor.setDecorations doesn't update the hover message`
    #[test]
    fn issue_4317_editor_set_decorations_doesnt_update_the_hover_message() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();

        let model = create_text_model("Hello world!");

        let hover = |value: &str| ModelDeltaDecoration {
            range: Range::new(1, 1, 100, 1),
            options: ModelDecorationOptions::register(&ModelDecorationOptionsInit {
                description: "test".to_owned(),
                hover_message: Some(vec![MarkdownString { value: value.to_owned() }]),
                ..Default::default()
            }),
        };

        let mut ids = model.delta_decorations(&[], &[hover("hello1")], 0);

        ids = model.delta_decorations(&ids, &[hover("hello2")], 0);

        let actual_decoration = model.get_decoration_options(&ids[0]);

        assert_eq!(
            actual_decoration.unwrap().hover_message,
            Some(vec![MarkdownString { value: "hello2".to_owned() }])
        );

        model.dispose();
    }

    /// `model doesn't get confused with individual tracked ranges`
    #[test]
    fn model_doesnt_get_confused_with_individual_tracked_ranges() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let model = create_text_model(&["Hello world,", "How are you?"].join("\n"));

        let tracked_range_id = model.change_decorations(
            |change_acessor| {
                change_acessor.add_decoration(
                    &Range::new(1, 1, 1, 1),
                    ModelDecorationOptions::register(&ModelDecorationOptionsInit {
                        description: "test".to_owned(),
                        stickiness: Some(TrackedRangeStickiness::AlwaysGrowsWhenTypingAtEdges),
                        ..Default::default()
                    }),
                )
            },
            0,
        );
        model.change_decorations(|change_accessor| change_accessor.remove_decoration(tracked_range_id.as_deref().unwrap()), 0);

        let mut ids = model.delta_decorations(
            &[],
            &[
                to_model_delta_decoration(&decoration("a", 1, 1, 1, 12)),
                to_model_delta_decoration(&decoration("b", 2, 1, 2, 13)),
            ],
            0,
        );

        assert_eq!(model.get_decoration_range(&ids[0]), Some(range(1, 1, 1, 12)));
        assert_eq!(model.get_decoration_range(&ids[1]), Some(range(2, 1, 2, 13)));

        ids = model.delta_decorations(
            &ids,
            &[
                to_model_delta_decoration(&decoration("a", 1, 1, 1, 12)),
                to_model_delta_decoration(&decoration("b", 2, 1, 2, 13)),
            ],
            0,
        );

        assert_eq!(model.get_decoration_range(&ids[0]), Some(range(1, 1, 1, 12)));
        assert_eq!(model.get_decoration_range(&ids[1]), Some(range(2, 1, 2, 13)));

        model.dispose();
    }

    /// `issue #16922: Clicking on link doesn't seem to do anything`
    #[test]
    fn issue_16922_clicking_on_link_doesnt_seem_to_do_anything() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let model = create_text_model(&["Hello world,", "How are you?", "Fine.", "Good."].join("\n"));

        let dec = |range: Range, class_name: &str| ModelDeltaDecoration {
            range,
            options: ModelDecorationOptions::register(&ModelDecorationOptionsInit {
                description: "test".to_owned(),
                class_name: Some(class_name.to_owned()),
                ..Default::default()
            }),
        };
        model.delta_decorations(
            &[],
            &[
                dec(Range::new(1, 1, 1, 1), "1"),
                dec(Range::new(1, 13, 1, 13), "2"),
                dec(Range::new(2, 1, 2, 1), "3"),
                dec(Range::new(2, 1, 2, 4), "4"),
                dec(Range::new(2, 8, 2, 13), "5"),
                dec(Range::new(3, 1, 4, 6), "6"),
                dec(Range::new(1, 1, 3, 6), "x1"),
                dec(Range::new(2, 5, 2, 8), "x2"),
                dec(Range::new(1, 1, 2, 8), "x3"),
                dec(Range::new(2, 5, 3, 1), "x4"),
            ],
            0,
        );

        let in_range = model.get_decorations_in_range(&Range::new(2, 6, 2, 6), 0, false, false, false, false);

        let mut in_range_class_names: Vec<Option<String>> = in_range.iter().map(|d| d.options.class_name.clone()).collect();
        in_range_class_names.sort();
        assert_eq!(
            in_range_class_names,
            vec![Some("x1".to_owned()), Some("x2".to_owned()), Some("x3".to_owned()), Some("x4".to_owned())]
        );

        model.dispose();
    }

    /// `issue #41492: URL highlighting persists after pasting over url`
    #[test]
    fn issue_41492_url_highlighting_persists_after_pasting_over_url() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();

        let model = create_text_model(&["My First Line"].join("\n"));

        let id = model
            .delta_decorations(
                &[],
                &[ModelDeltaDecoration {
                    range: Range::new(1, 2, 1, 14),
                    options: ModelDecorationOptions::register(&ModelDecorationOptionsInit {
                        description: "test".to_owned(),
                        stickiness: Some(TrackedRangeStickiness::NeverGrowsWhenTypingAtEdges),
                        collapse_on_replace_edit: true,
                        ..Default::default()
                    }),
                }],
                0,
            )
            .remove(0);
        model.apply_edits(&[edit(Range::new(1, 1, 1, 14), "Some new text that is longer than the previous one", false)]);
        let actual = model.get_decoration_range(&id);
        assert_eq!(actual, Some(Range::new(1, 1, 1, 1)));

        model.dispose();
    }
}
