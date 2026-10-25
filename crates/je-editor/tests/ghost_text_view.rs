//! Porte de `src/vs/editor/contrib/inlineCompletions/test/browser/ghostTextView.test.ts` (suíte `GhostTextView`), caso
//! a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o `LeakGuard` criado no começo de cada caso.
//! - `createInsertionViewContext` monta o editor de teste com `lineHeight` 20, minimapa desligado e
//!   `scrollBeyondLastLine` falso, com 200 linhas `line NNN content`. O `InlineEditsInsertionView` (a slice de inline
//!   edits, ainda sem porte) entra pelo que ele faz com o `GhostTextView`: uma `GhostText` de uma parte com a
//!   `InlineDecoration` `modified-background` por linha, e as opções `extraClasses: ['inline-edit']`,
//!   `isClickable` e `shouldKeepCursorStable` verdadeiros. O recorte de prefixo (`getPrefixTrim`) só muda as colunas
//!   das decorações, não a geometria que os casos medem, e vale zero aqui.
//! - O `CodeEditorWidget` do porte não tem `View`: `editor.layout(...)`, `editor.render(true)` e o
//!   `requestAnimationFrame` não existem, e a geometria é a do `ViewModel`, síncrona. A view zone é a do
//!   `editor.changeViewZones` do widget (a parte `ViewZones`, que a transforma numa whitespace do layout com altura
//!   `heightInLines * lineHeight`, depois da linha `afterLineNumber`), como no original.
//! - `context.input.set(undefined, undefined)` é `input.set(None, None)`; `assert.deepStrictEqual` sobre os dois
//!   campos é a comparação das tuplas.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite};
use je_base::observable::SettableExt;
use je_base::dynamic::Dynamic;
use je_base::lifecycle::to_disposable;
use je_base::observable::base::IObservable;
use je_base::observable::observables::const_observable::const_observable;
use je_base::observable::observables::derived::derived;
use je_base::observable::observables::observable_value::{ObservableValueOptions, observable_value_opts};
use je_base::observable::debug_name::DebugNameOptions;
use je_editor::code_editor_widget::ICodeEditor;
use je_editor::edits::text_edit::{TextEdit, TextReplacement};
use je_editor::inline_completions::ghost_text::{GhostText, GhostTextOrReplacement, GhostTextPart};
use je_editor::inline_completions::inline_completion_view_data::InlineCompletionViewData;
use je_editor::inline_completions::view::ghost_text_view::{
    GhostTextView, GhostTextViewOptions, IGhostTextWidgetData, GhostTextWidgetWarning,
};
use je_editor::inline_decorations::{InlineDecoration, InlineDecorationType};
use je_editor::language_service::I_LANGUAGE_SERVICE;
use je_editor::position::Position;
use je_editor::range::Range;
use je_editor::text_model_edit_source::{EditSources, InlineCompletionAcceptData};
use support::test_code_editor::{
    TestCodeEditor, create_code_editor_services, instantiate_test_code_editor, TestCodeEditorCreationOptions,
};
use support::test_text_model::create_text_model;

/// A `GhostTextView` não lê o aviso nem a telemetria do inline edit: o `handleInlineCompletionShown` do
/// `InlineEditsInsertionView` é um no-op e o `warning` é `undefined`.
struct InsertionWidgetData {
    ghost_text: GhostTextOrReplacement,
}

impl IGhostTextWidgetData for InsertionWidgetData {
    fn ghost_text(&self) -> &GhostTextOrReplacement {
        &self.ghost_text
    }

    fn warning(&self) -> Option<&GhostTextWidgetWarning> {
        None
    }

    fn handle_inline_completion_shown(&self, _view_data: &InlineCompletionViewData) {
        // This is a no-op for the insertion view, as it is handled by the InlineEditsView.
    }
}

/// O `{ lineNumber, startColumn, text, editorType }` do `input` (o `editorType` não é lido pela view).
struct InsertionInput {
    line_number: i32,
    start_column: i32,
    text: String,
}

/// O retorno de `createInsertionViewContext`.
struct InsertionViewContext {
    editor: Rc<TestCodeEditor>,
    input: je_base::observable::base::ISettableObservable<Option<Rc<InsertionInput>>>,
    text_model: Rc<je_editor::text_model::TextModel>,
    line_height: f64,
    insertion_text: String,
    inserted_line_count: usize,
    insertion_range: Range,
}

/// `createInsertionViewContext(store)`.
fn create_insertion_view_context(store: &LeakGuard) -> InsertionViewContext {
    let line_height = 20.0;
    let options: TestCodeEditorCreationOptions = Dynamic::object([
        ("lineHeight", Dynamic::from(line_height)),
        ("minimap", Dynamic::object([("enabled", Dynamic::from(false))])),
        ("scrollBeyondLastLine", Dynamic::from(false)),
    ]);
    let services = create_code_editor_services(&store.store(), None);
    let text_model = create_text_model(
        &(0..200).map(|index| format!("line {:03} content", index + 1)).collect::<Vec<_>>().join("\n"),
    );
    let owned = text_model.clone();
    store.add(to_disposable(move || owned.dispose()));
    let editor = instantiate_test_code_editor(&services, Some(text_model.clone()), &options);
    store.add_rc(editor.clone());

    let target_line_number = 100;
    let target_column = 5;
    let insertion_text = "\ninserted 1\ninserted 2\ninserted 3\ninserted 4\ninserted 5\ninserted 6\ninserted 7".to_owned();
    let inserted_line_count = insertion_text.split('\n').count() - 1;
    let insertion_range = Range::new(target_line_number, target_column, target_line_number, target_column);
    let input = observable_value_opts::<Option<Rc<InsertionInput>>, ()>(
        ObservableValueOptions { debug: DebugNameOptions::named("input"), equals_fn: None, lazy: false },
        None,
    );

    let ghost_text_data: IObservable<Option<Rc<dyn IGhostTextWidgetData>>> = {
        let input = input.to_observable();
        derived(move |reader| {
            let state = input.read(reader)?;
            let inline_decorations: Vec<InlineDecoration> = state
                .text
                .split('\n')
                .enumerate()
                .map(|(index, line)| {
                    InlineDecoration::new(
                        Range::new(index as i32 + 1, 1, index as i32 + 1, line.encode_utf16().count() as i32 + 1),
                        "modified-background",
                        InlineDecorationType::Regular,
                    )
                })
                .collect();
            let ghost_text = GhostText::new(
                state.line_number,
                vec![GhostTextPart::new(state.start_column, state.text.as_str(), false, inline_decorations)],
            );
            Some(Rc::new(InsertionWidgetData { ghost_text: GhostTextOrReplacement::GhostText(ghost_text) }) as Rc<dyn IGhostTextWidgetData>)
        })
    };
    let view = GhostTextView::new(
        editor.widget().clone(),
        ghost_text_data,
        GhostTextViewOptions {
            extra_classes: vec!["inline-edit".to_owned()],
            is_clickable: true,
            should_keep_cursor_stable: true,
            min_reserved_line_count: Some(const_observable(0)),
            ..GhostTextViewOptions::default()
        },
        services.get(I_LANGUAGE_SERVICE).language_id_codec(),
    );
    store.add_rc(view);

    editor.set_position(Position::new(target_line_number, target_column), Some("test"));
    editor.set_scroll_top(editor.get_top_for_line_number(target_line_number - 22, Some(true)), None);
    input.set(
        Some(Rc::new(InsertionInput { line_number: target_line_number, start_column: target_column, text: insertion_text.clone() })),
        None,
    );

    InsertionViewContext { editor, input, text_model, line_height, insertion_text, inserted_line_count, insertion_range }
}

#[test]
fn accepting_a_multi_line_insertion_preserves_the_viewport() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let context = create_insertion_view_context(&store);
    let scroll_top = context.editor.get_scroll_top();
    let anchor_top = context.editor.get_top_for_line_number(80, Some(true)) - scroll_top;

    let input = Rc::clone(&context.input);
    store.add(context.editor.on_did_change_model_content().listen(move |_| input.set(None, None)));
    context.editor.edit(
        &TextEdit::new(vec![TextReplacement::new(context.insertion_range, context.insertion_text.as_str())]),
        EditSources::inline_completion_accept(&InlineCompletionAcceptData {
            nes: true,
            request_uuid: "ghost-text-view-test".to_owned(),
            language_id: context.text_model.get_language_id().to_string(),
            provider_id: None,
            correlation_id: None,
        }),
    );

    assert_eq!(
        (context.editor.get_scroll_top(), context.editor.get_top_for_line_number(80, Some(true)) - context.editor.get_scroll_top()),
        (scroll_top, anchor_top),
    );
}

#[test]
fn dismissing_a_multi_line_insertion_keeps_the_cursor_stable() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let context = create_insertion_view_context(&store);
    context.editor.set_position(Position::new(110, 1), Some("test"));
    let scroll_top = context.editor.get_scroll_top();
    let cursor_top = context.editor.get_top_for_line_number(110, Some(true)) - scroll_top;

    context.input.set(None, None);

    assert_eq!(
        (
            context.editor.get_scroll_top() - scroll_top,
            context.editor.get_top_for_line_number(110, Some(true)) - context.editor.get_scroll_top(),
        ),
        (-(context.inserted_line_count as f64) * context.line_height, cursor_top),
    );
}
