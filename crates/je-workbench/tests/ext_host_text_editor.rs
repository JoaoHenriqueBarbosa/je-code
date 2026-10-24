//! Porte de `src/vs/workbench/api/test/browser/extHostTextEditor.test.ts` (suítes `ExtHostTextEditor` e
//! `ExtHostTextEditorOptions`), caso a caso e na mesma ordem. Cada suíte é um módulo.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado no começo de cada caso. Os casos `async`
//!   rodam em `run_with_faked_timers`.
//! - `mock<MainThreadTextEditorsShape>()` com `override` e o literal com os membros `undefined!` viram o
//!   [`MockMainThreadTextEditors`]: cada método que o caso usa tem um fecho opcional, e os demais falham se
//!   forem chamados (o `undefined!` que lançaria `TypeError`). O `null!` do `proxy` da primeira suíte é o mesmo
//!   dublê sem fecho nenhum.
//! - O `doc` da primeira suíte é um `const` do módulo no original; cada caso monta o seu, que nenhum altera.
//! - `new Lazy(() => doc.document)` é `Lazy::new` sobre o `TextDocument` do documento.
//! - `assert.throws(() => ...)` vira `errors::catch(...).is_err()`. No caso `disposed editor`, o `null!` de
//!   `_acceptOptions(null!)` é uma configuração qualquer: o que lança é a asserção de editor descartado, que
//!   vem antes de qualquer leitura do argumento.
//! - `editor.value.edit(edit => { ... })` é `editor.value().edit(|edit| { ... }, None)`, com os métodos de
//!   `TextEditorEdit` devolvendo `Result` (o `unwrap` é o `throw` do original). `edit.setEndOfLine(1)` é
//!   `EndOfLine::LF` (valor `1`) e `new Range(0, 0, 1, 1)` é `Range::from_coordinates`.
//! - Na segunda suíte, `opts.value.tabSize = x` é `opts.set_tab_size(&x)`, e os valores que a extensão pode
//!   mandar (`'2'`, `null`, `-5`, `'hello'`) são `Dynamic`. `<any>'2'` e os demais casts não existem.
//!   `calls` guarda os `ITextEditorConfigurationUpdate` recebidos.
//! - `assertState` confere um campo por vez: o `deepStrictEqual` do original compara um objeto de cinco
//!   campos. `lineNumbers: RenderLineNumbersType.X` é conferido pelo valor numérico, porque `opts.value.lineNumbers`
//!   é um `TextEditorLineNumbersStyle` (os valores das duas enumerações coincidem).

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::JsString;
use je_base::dynamic::Dynamic;
use je_base::errors;
use je_base::lazy::Lazy;
use je_base::lifecycle::IDisposable;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_editor::config::editor_options::cursor_style::TextEditorCursorStyle;
use je_editor::config::editor_options::line_numbers::RenderLineNumbersType;
use je_editor::diff::legacy_lines_diff_computer::LineChange;
use je_editor::edit_operation::SingleEditOperation;
use je_editor::model::Eol;
use je_editor::range::Range as EditorRange;
use je_editor::selection::Selection as EditorSelection;
use je_editor::services::decoration_render_options::DecorationRenderOptions;
use je_extensions::manifest::ExtensionIdentifier;
use je_platform::log::NullLogService;
use je_workbench::api::common::ext_host_document_data::{ExtHostDocumentData, IExtHostDocumentSaveDelegate};
use je_workbench::api::common::ext_host_protocol::{
    ApplyEditsOptions, DecorationOptionsDto, EditorGroupColumn, IndentSizeValue, InsertSpacesValue, MainThreadTextEditorsShape,
    ResolvedTextEditorConfiguration, SnippetOptions, TabSizeValue, TextDocumentShowOptions, TextEditorConfigurationUpdate,
};
use je_workbench::api::common::ext_host_text_editor::{ExtHostTextEditor, ExtHostTextEditorOptions, TextEditorOptionsInput};
use je_workbench::api::common::ext_host_types::{EndOfLine, Range, TextEditorLineNumbersStyle, TextEditorRevealType};

fn platform() -> je_base::path::Platform {
    if cfg!(windows) { je_base::path::Platform::Win32 } else { je_base::path::Platform::Posix }
}

/// O `proxy` do documento: nenhum caso o chama.
struct UnusedSaveDelegate;

impl IExtHostDocumentSaveDelegate for UnusedSaveDelegate {
    fn rpc_try_save_document(&self, _uri: &URI) -> Promise<bool> {
        panic!("o proxy não deveria ser chamado");
    }
}

/// O membro `undefined!` do dublê: chamá-lo é erro.
fn undefined<T>(name: &str) -> T {
    panic!("{name} é undefined");
}

type TrySetOptionsFn = Box<dyn Fn(&str, TextEditorConfigurationUpdate) -> Promise<()>>;
type TryApplyEditsFn = Box<dyn Fn() -> Promise<bool>>;

/// O dublê de `MainThreadTextEditorsShape`.
#[derive(Default)]
struct MockMainThreadTextEditors {
    try_set_options: Option<TrySetOptionsFn>,
    try_apply_edits: Option<TryApplyEditsFn>,
}

impl IDisposable for MockMainThreadTextEditors {
    fn dispose(&self) {}
}

impl MainThreadTextEditorsShape for MockMainThreadTextEditors {
    fn rpc_try_show_text_document(&self, _resource: &URI, _options: TextDocumentShowOptions) -> Promise<Option<String>> {
        undefined("$tryShowTextDocument")
    }

    fn rpc_register_text_editor_decoration_type(&self, _extension_id: &ExtensionIdentifier, _key: &str, _options: DecorationRenderOptions) {
        undefined("$registerTextEditorDecorationType")
    }

    fn rpc_remove_text_editor_decoration_type(&self, _key: &str) {
        undefined("$removeTextEditorDecorationType")
    }

    fn rpc_try_show_editor(&self, _id: &str, _position: EditorGroupColumn) -> Promise<()> {
        undefined("$tryShowEditor")
    }

    fn rpc_try_hide_editor(&self, _id: &str) -> Promise<()> {
        undefined("$tryHideEditor")
    }

    fn rpc_try_set_options(&self, id: &str, options: TextEditorConfigurationUpdate) -> Promise<()> {
        match &self.try_set_options {
            Some(try_set_options) => try_set_options(id, options),
            None => undefined("$trySetOptions"),
        }
    }

    fn rpc_try_set_decorations(&self, _id: &str, _key: &str, _ranges: Vec<DecorationOptionsDto>) -> Promise<()> {
        undefined("$trySetDecorations")
    }

    fn rpc_try_set_decorations_fast(&self, _id: &str, _key: &str, _ranges: Vec<f64>) -> Promise<()> {
        undefined("$trySetDecorationsFast")
    }

    fn rpc_try_reveal_range(&self, _id: &str, _range: EditorRange, _reveal_type: TextEditorRevealType) -> Promise<()> {
        undefined("$tryRevealRange")
    }

    fn rpc_try_set_selections(&self, _id: &str, _selections: Vec<EditorSelection>) -> Promise<()> {
        undefined("$trySetSelections")
    }

    fn rpc_try_apply_edits(&self, _id: &str, _model_version_id: f64, _edits: Vec<SingleEditOperation>, _opts: ApplyEditsOptions) -> Promise<bool> {
        match &self.try_apply_edits {
            Some(try_apply_edits) => try_apply_edits(),
            None => undefined("$tryApplyEdits"),
        }
    }

    fn rpc_try_insert_snippet(&self, _id: &str, _model_version_id: f64, _template: &str, _selections: &[EditorRange], _opts: SnippetOptions) -> Promise<bool> {
        undefined("$tryInsertSnippet")
    }

    fn rpc_get_diff_information(&self, _id: &str) -> Promise<Vec<LineChange>> {
        undefined("$getDiffInformation")
    }
}

/// `{ cursorStyle: TextEditorCursorStyle.Line, insertSpaces: true, lineNumbers: 1, tabSize: 4, indentSize: 4,
/// originalIndentSize: 'tabSize' }`.
fn editor_configuration() -> ResolvedTextEditorConfiguration {
    ResolvedTextEditorConfiguration {
        cursor_style: TextEditorCursorStyle::Line,
        insert_spaces: true,
        line_numbers: RenderLineNumbersType::On,
        tab_size: 4.0,
        indent_size: 4.0,
        original_indent_size: IndentSizeValue::TabSize,
    }
}

mod ext_host_text_editor {
    use super::*;

    /// O `doc` do módulo.
    fn doc() -> ExtHostDocumentData {
        ExtHostDocumentData::new(
            Rc::new(UnusedSaveDelegate),
            URI::file("", platform()),
            vec![JsString::from("aaaa bbbb+cccc abc")],
            Eol::Lf,
            1,
            "text",
            false,
            "utf8",
        )
    }

    /// `new ExtHostTextEditor(id, proxy, new NullLogService(), new Lazy(() => doc.document), [], { ... }, [], 1)`.
    fn new_editor(id: &str, proxy: MockMainThreadTextEditors, doc: &ExtHostDocumentData) -> ExtHostTextEditor {
        let document = doc.document();
        ExtHostTextEditor::new(id, Rc::new(proxy), NullLogService::shared(), Lazy::new(move || document), vec![], &editor_configuration(), vec![], Some(1.0))
    }

    #[test]
    fn disposed_editor() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let doc = doc();
        let editor = new_editor("fake", MockMainThreadTextEditors::default(), &doc);

        let _document = editor.value().document();
        editor.accept_view_column(3.0);
        assert_eq!(Some(3.0), editor.value().view_column());

        editor.dispose();

        assert!(errors::catch(|| editor.accept_view_column(2.0)).is_err());
        assert_eq!(Some(3.0), editor.value().view_column());

        let _document = editor.value().document();
        assert!(errors::catch(|| editor.accept_options(&editor_configuration())).is_err());
        assert!(errors::catch(|| editor.accept_selections(vec![])).is_err());
    }

    #[test]
    fn api_bug_register_text_editor_command_clears_redo_stack_even_if_no_edits_are_made_55163() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        run_with_faked_timers(async move {
            let doc = doc();
            let apply_count = Rc::new(Cell::new(0));
            let counter = apply_count.clone();
            let editor = new_editor(
                "edt1",
                MockMainThreadTextEditors {
                    try_apply_edits: Some(Box::new(move || {
                        counter.set(counter.get() + 1);
                        Promise::resolved(true)
                    })),
                    ..MockMainThreadTextEditors::default()
                },
                &doc,
            );

            editor.value().edit(|_edit| {}, None).await.unwrap();
            assert_eq!(apply_count.get(), 0);

            editor
                .value()
                .edit(
                    |edit| {
                        edit.set_end_of_line(EndOfLine::LF).unwrap();
                    },
                    None,
                )
                .await
                .unwrap();
            assert_eq!(apply_count.get(), 1);

            editor
                .value()
                .edit(
                    |edit| {
                        edit.delete(Range::from_coordinates(0.0, 0.0, 1.0, 1.0).unwrap()).unwrap();
                    },
                    None,
                )
                .await
                .unwrap();
            assert_eq!(apply_count.get(), 2);
        });
    }
}

mod ext_host_text_editor_options {
    use super::*;

    type Calls = Rc<RefCell<Vec<TextEditorConfigurationUpdate>>>;

    /// O `setup`: as opções do editor e a lista `calls` do `$trySetOptions` do `mockProxy`.
    fn setup() -> (Rc<ExtHostTextEditorOptions>, Calls) {
        let calls: Calls = Rc::default();
        let recorded = calls.clone();
        let proxy = MockMainThreadTextEditors {
            try_set_options: Some(Box::new(move |id, options| {
                assert_eq!(id, "1");
                recorded.borrow_mut().push(options);
                Promise::resolved(())
            })),
            ..MockMainThreadTextEditors::default()
        };
        let opts = ExtHostTextEditorOptions::new(
            Rc::new(proxy),
            "1",
            &ResolvedTextEditorConfiguration {
                tab_size: 4.0,
                indent_size: 4.0,
                original_indent_size: IndentSizeValue::TabSize,
                insert_spaces: false,
                cursor_style: TextEditorCursorStyle::Line,
                line_numbers: RenderLineNumbersType::On,
            },
            NullLogService::shared(),
        );
        (opts, calls)
    }

    fn assert_state(
        opts: &ExtHostTextEditorOptions,
        tab_size: f64,
        indent_size: f64,
        insert_spaces: bool,
        cursor_style: TextEditorCursorStyle,
        line_numbers: RenderLineNumbersType,
    ) {
        assert_eq!(opts.tab_size(), tab_size);
        assert_eq!(opts.indent_size(), indent_size);
        assert_eq!(opts.insert_spaces(), insert_spaces);
        assert_eq!(opts.cursor_style(), cursor_style);
        assert_eq!(opts.line_numbers().value(), line_numbers as i32);
    }

    fn tab_size(tab_size: TabSizeValue) -> TextEditorConfigurationUpdate {
        TextEditorConfigurationUpdate { tab_size: Some(tab_size), ..Default::default() }
    }

    fn indent_size(indent_size: IndentSizeValue) -> TextEditorConfigurationUpdate {
        TextEditorConfigurationUpdate { indent_size: Some(indent_size), ..Default::default() }
    }

    fn insert_spaces(insert_spaces: InsertSpacesValue) -> TextEditorConfigurationUpdate {
        TextEditorConfigurationUpdate { insert_spaces: Some(insert_spaces), ..Default::default() }
    }

    #[test]
    fn can_set_tab_size_to_the_same_value() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let (opts, calls) = setup();
        opts.set_tab_size(&Dynamic::Number(4.0));
        assert_state(&opts, 4.0, 4.0, false, TextEditorCursorStyle::Line, RenderLineNumbersType::On);
        assert!(calls.borrow().is_empty());
    }

    #[test]
    fn can_change_tab_size_to_positive_integer() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let (opts, calls) = setup();
        opts.set_tab_size(&Dynamic::Number(1.0));
        assert_state(&opts, 1.0, 4.0, false, TextEditorCursorStyle::Line, RenderLineNumbersType::On);
        assert_eq!(*calls.borrow(), vec![tab_size(TabSizeValue::Size(1.0))]);
    }

    #[test]
    fn can_change_tab_size_to_positive_float() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let (opts, calls) = setup();
        opts.set_tab_size(&Dynamic::Number(2.3));
        assert_state(&opts, 2.0, 4.0, false, TextEditorCursorStyle::Line, RenderLineNumbersType::On);
        assert_eq!(*calls.borrow(), vec![tab_size(TabSizeValue::Size(2.0))]);
    }

    #[test]
    fn can_change_tab_size_to_a_string_number() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let (opts, calls) = setup();
        opts.set_tab_size(&Dynamic::string("2"));
        assert_state(&opts, 2.0, 4.0, false, TextEditorCursorStyle::Line, RenderLineNumbersType::On);
        assert_eq!(*calls.borrow(), vec![tab_size(TabSizeValue::Size(2.0))]);
    }

    #[test]
    fn tab_size_can_request_indentation_detection() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let (opts, calls) = setup();
        opts.set_tab_size(&Dynamic::string("auto"));
        assert_state(&opts, 4.0, 4.0, false, TextEditorCursorStyle::Line, RenderLineNumbersType::On);
        assert_eq!(*calls.borrow(), vec![tab_size(TabSizeValue::Auto)]);
    }

    #[test]
    fn ignores_invalid_tab_size_1() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let (opts, calls) = setup();
        opts.set_tab_size(&Dynamic::Null);
        assert_state(&opts, 4.0, 4.0, false, TextEditorCursorStyle::Line, RenderLineNumbersType::On);
        assert!(calls.borrow().is_empty());
    }

    #[test]
    fn ignores_invalid_tab_size_2() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let (opts, calls) = setup();
        opts.set_tab_size(&Dynamic::Number(-5.0));
        assert_state(&opts, 4.0, 4.0, false, TextEditorCursorStyle::Line, RenderLineNumbersType::On);
        assert!(calls.borrow().is_empty());
    }

    #[test]
    fn ignores_invalid_tab_size_3() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let (opts, calls) = setup();
        opts.set_tab_size(&Dynamic::string("hello"));
        assert_state(&opts, 4.0, 4.0, false, TextEditorCursorStyle::Line, RenderLineNumbersType::On);
        assert!(calls.borrow().is_empty());
    }

    #[test]
    fn ignores_invalid_tab_size_4() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let (opts, calls) = setup();
        opts.set_tab_size(&Dynamic::string("-17"));
        assert_state(&opts, 4.0, 4.0, false, TextEditorCursorStyle::Line, RenderLineNumbersType::On);
        assert!(calls.borrow().is_empty());
    }

    #[test]
    fn can_set_indent_size_to_the_same_value() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let (opts, calls) = setup();
        opts.set_indent_size(&Dynamic::Number(4.0));
        assert_state(&opts, 4.0, 4.0, false, TextEditorCursorStyle::Line, RenderLineNumbersType::On);
        assert_eq!(*calls.borrow(), vec![indent_size(IndentSizeValue::Size(4.0))]);
    }

    #[test]
    fn can_change_indent_size_to_positive_integer() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let (opts, calls) = setup();
        opts.set_indent_size(&Dynamic::Number(1.0));
        assert_state(&opts, 4.0, 1.0, false, TextEditorCursorStyle::Line, RenderLineNumbersType::On);
        assert_eq!(*calls.borrow(), vec![indent_size(IndentSizeValue::Size(1.0))]);
    }

    #[test]
    fn can_change_indent_size_to_positive_float() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let (opts, calls) = setup();
        opts.set_indent_size(&Dynamic::Number(2.3));
        assert_state(&opts, 4.0, 2.0, false, TextEditorCursorStyle::Line, RenderLineNumbersType::On);
        assert_eq!(*calls.borrow(), vec![indent_size(IndentSizeValue::Size(2.0))]);
    }

    #[test]
    fn can_change_indent_size_to_a_string_number() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let (opts, calls) = setup();
        opts.set_indent_size(&Dynamic::string("2"));
        assert_state(&opts, 4.0, 2.0, false, TextEditorCursorStyle::Line, RenderLineNumbersType::On);
        assert_eq!(*calls.borrow(), vec![indent_size(IndentSizeValue::Size(2.0))]);
    }

    #[test]
    fn indent_size_can_request_to_use_tab_size() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let (opts, calls) = setup();
        opts.set_indent_size(&Dynamic::string("tabSize"));
        assert_state(&opts, 4.0, 4.0, false, TextEditorCursorStyle::Line, RenderLineNumbersType::On);
        assert_eq!(*calls.borrow(), vec![indent_size(IndentSizeValue::TabSize)]);
    }

    #[test]
    fn indent_size_cannot_request_indentation_detection() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let (opts, calls) = setup();
        opts.set_indent_size(&Dynamic::string("auto"));
        assert_state(&opts, 4.0, 4.0, false, TextEditorCursorStyle::Line, RenderLineNumbersType::On);
        assert!(calls.borrow().is_empty());
    }

    #[test]
    fn ignores_invalid_indent_size_1() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let (opts, calls) = setup();
        opts.set_indent_size(&Dynamic::Null);
        assert_state(&opts, 4.0, 4.0, false, TextEditorCursorStyle::Line, RenderLineNumbersType::On);
        assert!(calls.borrow().is_empty());
    }

    #[test]
    fn ignores_invalid_indent_size_2() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let (opts, calls) = setup();
        opts.set_indent_size(&Dynamic::Number(-5.0));
        assert_state(&opts, 4.0, 4.0, false, TextEditorCursorStyle::Line, RenderLineNumbersType::On);
        assert!(calls.borrow().is_empty());
    }

    #[test]
    fn ignores_invalid_indent_size_3() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let (opts, calls) = setup();
        opts.set_indent_size(&Dynamic::string("hello"));
        assert_state(&opts, 4.0, 4.0, false, TextEditorCursorStyle::Line, RenderLineNumbersType::On);
        assert!(calls.borrow().is_empty());
    }

    #[test]
    fn ignores_invalid_indent_size_4() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let (opts, calls) = setup();
        opts.set_indent_size(&Dynamic::string("-17"));
        assert_state(&opts, 4.0, 4.0, false, TextEditorCursorStyle::Line, RenderLineNumbersType::On);
        assert!(calls.borrow().is_empty());
    }

    #[test]
    fn can_set_insert_spaces_to_the_same_value() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let (opts, calls) = setup();
        opts.set_insert_spaces(&Dynamic::Bool(false));
        assert_state(&opts, 4.0, 4.0, false, TextEditorCursorStyle::Line, RenderLineNumbersType::On);
        assert!(calls.borrow().is_empty());
    }

    #[test]
    fn can_set_insert_spaces_to_boolean() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let (opts, calls) = setup();
        opts.set_insert_spaces(&Dynamic::Bool(true));
        assert_state(&opts, 4.0, 4.0, true, TextEditorCursorStyle::Line, RenderLineNumbersType::On);
        assert_eq!(*calls.borrow(), vec![insert_spaces(InsertSpacesValue::Bool(true))]);
    }

    #[test]
    fn can_set_insert_spaces_to_false_string() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let (opts, calls) = setup();
        opts.set_insert_spaces(&Dynamic::string("false"));
        assert_state(&opts, 4.0, 4.0, false, TextEditorCursorStyle::Line, RenderLineNumbersType::On);
        assert!(calls.borrow().is_empty());
    }

    #[test]
    fn can_set_insert_spaces_to_truey() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let (opts, calls) = setup();
        opts.set_insert_spaces(&Dynamic::string("hello"));
        assert_state(&opts, 4.0, 4.0, true, TextEditorCursorStyle::Line, RenderLineNumbersType::On);
        assert_eq!(*calls.borrow(), vec![insert_spaces(InsertSpacesValue::Bool(true))]);
    }

    #[test]
    fn insert_spaces_can_request_indentation_detection() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let (opts, calls) = setup();
        opts.set_insert_spaces(&Dynamic::string("auto"));
        assert_state(&opts, 4.0, 4.0, false, TextEditorCursorStyle::Line, RenderLineNumbersType::On);
        assert_eq!(*calls.borrow(), vec![insert_spaces(InsertSpacesValue::Auto)]);
    }

    #[test]
    fn can_set_cursor_style_to_same_value() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let (opts, calls) = setup();
        opts.set_cursor_style(TextEditorCursorStyle::Line);
        assert_state(&opts, 4.0, 4.0, false, TextEditorCursorStyle::Line, RenderLineNumbersType::On);
        assert!(calls.borrow().is_empty());
    }

    #[test]
    fn can_change_cursor_style() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let (opts, calls) = setup();
        opts.set_cursor_style(TextEditorCursorStyle::Block);
        assert_state(&opts, 4.0, 4.0, false, TextEditorCursorStyle::Block, RenderLineNumbersType::On);
        assert_eq!(*calls.borrow(), vec![TextEditorConfigurationUpdate { cursor_style: Some(TextEditorCursorStyle::Block), ..Default::default() }]);
    }

    #[test]
    fn can_set_line_numbers_to_same_value() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let (opts, calls) = setup();
        opts.set_line_numbers(TextEditorLineNumbersStyle::On);
        assert_state(&opts, 4.0, 4.0, false, TextEditorCursorStyle::Line, RenderLineNumbersType::On);
        assert!(calls.borrow().is_empty());
    }

    #[test]
    fn can_change_line_numbers() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let (opts, calls) = setup();
        opts.set_line_numbers(TextEditorLineNumbersStyle::Off);
        assert_state(&opts, 4.0, 4.0, false, TextEditorCursorStyle::Line, RenderLineNumbersType::Off);
        assert_eq!(*calls.borrow(), vec![TextEditorConfigurationUpdate { line_numbers: Some(RenderLineNumbersType::Off), ..Default::default() }]);
    }

    #[test]
    fn can_do_bulk_updates_0() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let (opts, calls) = setup();
        opts.assign(&TextEditorOptionsInput {
            tab_size: Some(Dynamic::Number(4.0)),
            indent_size: Some(Dynamic::Number(4.0)),
            insert_spaces: Some(Dynamic::Bool(false)),
            cursor_style: Some(TextEditorCursorStyle::Line),
            line_numbers: Some(TextEditorLineNumbersStyle::On),
        });
        assert_state(&opts, 4.0, 4.0, false, TextEditorCursorStyle::Line, RenderLineNumbersType::On);
        assert_eq!(*calls.borrow(), vec![indent_size(IndentSizeValue::Size(4.0))]);
    }

    #[test]
    fn can_do_bulk_updates_1() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let (opts, calls) = setup();
        opts.assign(&TextEditorOptionsInput { tab_size: Some(Dynamic::string("auto")), insert_spaces: Some(Dynamic::Bool(true)), ..Default::default() });
        assert_state(&opts, 4.0, 4.0, true, TextEditorCursorStyle::Line, RenderLineNumbersType::On);
        assert_eq!(
            *calls.borrow(),
            vec![TextEditorConfigurationUpdate {
                tab_size: Some(TabSizeValue::Auto),
                insert_spaces: Some(InsertSpacesValue::Bool(true)),
                ..Default::default()
            }]
        );
    }

    #[test]
    fn can_do_bulk_updates_2() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let (opts, calls) = setup();
        opts.assign(&TextEditorOptionsInput { tab_size: Some(Dynamic::Number(3.0)), insert_spaces: Some(Dynamic::string("auto")), ..Default::default() });
        assert_state(&opts, 3.0, 4.0, false, TextEditorCursorStyle::Line, RenderLineNumbersType::On);
        assert_eq!(
            *calls.borrow(),
            vec![TextEditorConfigurationUpdate {
                tab_size: Some(TabSizeValue::Size(3.0)),
                insert_spaces: Some(InsertSpacesValue::Auto),
                ..Default::default()
            }]
        );
    }

    #[test]
    fn can_do_bulk_updates_3() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let (opts, calls) = setup();
        opts.assign(&TextEditorOptionsInput {
            cursor_style: Some(TextEditorCursorStyle::Block),
            line_numbers: Some(TextEditorLineNumbersStyle::Relative),
            ..Default::default()
        });
        assert_state(&opts, 4.0, 4.0, false, TextEditorCursorStyle::Block, RenderLineNumbersType::Relative);
        assert_eq!(
            *calls.borrow(),
            vec![TextEditorConfigurationUpdate {
                cursor_style: Some(TextEditorCursorStyle::Block),
                line_numbers: Some(RenderLineNumbersType::Relative),
                ..Default::default()
            }]
        );
    }
}
