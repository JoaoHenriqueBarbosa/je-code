//! Teste de unidade do `ExtHostEditors` (`extHostTextEditors.ts`; o oráculo não tem teste dele, então não é porte): o editor ativo e
//! os visíveis vêm do `ExtHostDocumentsAndEditors`, os `$accept*` do workbench disparam os eventos de `window.onDidChange*` e
//! `showTextDocument` resolve o editor pelo id que o `MainThreadTextEditors` devolve.

use std::any::Any;
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;

use je_base::lifecycle::IDisposable;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_base::JsString;
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
use je_workbench::api::common::ext_host_document_data::IExtHostDocumentSaveDelegate;
use je_workbench::api::common::ext_host_documents_and_editors::ExtHostDocumentsAndEditors;
use je_workbench::api::common::ext_host_editors::{ExtHostEditors, ShowTextDocumentArgument};
use je_workbench::api::common::ext_host_protocol::{
    ApplyEditsOptions, CreateDocumentOptions, DecorationOptionsDto, DocumentsAndEditorsDelta, EditorGroupColumn, EditorPropertiesChangeData, ExtHostDocumentsAndEditorsShape,
    ExtHostEditorsShape, IMainContext, IndentSizeValue, MainContext, MainThreadDocumentsShape, MainThreadTextEditorsShape, ModelAddedData, OpenDocumentOptions,
    ResolvedTextEditorConfiguration, SelectionChangeEvent, SnippetOptions, TextDocumentShowOptions, TextEditorAddData, TextEditorConfigurationUpdate,
};
use je_workbench::api::common::ext_host_types::{TextEditorRevealType, TextEditorSelectionChangeKind};

#[derive(Default)]
struct TestRpcProtocol {
    objects: RefCell<HashMap<&'static str, Rc<dyn Any>>>,
}

impl IMainContext for TestRpcProtocol {
    fn get_proxy_erased(&self, sid: &'static str) -> Rc<dyn Any> {
        self.objects.borrow().get(sid).cloned().unwrap_or_else(|| panic!("o proxy '{sid}' não foi registrado"))
    }

    fn set_erased(&self, sid: &'static str, value: Rc<dyn Any>) {
        self.objects.borrow_mut().insert(sid, value);
    }
}

struct UnusedDocuments;

impl IDisposable for UnusedDocuments {
    fn dispose(&self) {}
}

impl IExtHostDocumentSaveDelegate for UnusedDocuments {
    fn rpc_try_save_document(&self, _uri: &URI) -> Promise<bool> {
        panic!("não exercitado")
    }
}

impl MainThreadDocumentsShape for UnusedDocuments {
    fn rpc_try_create_document(&self, _options: Option<CreateDocumentOptions>) -> Promise<URI> {
        panic!("não exercitado")
    }

    fn rpc_try_open_document(&self, _uri: &URI, _options: Option<OpenDocumentOptions>) -> Promise<URI> {
        panic!("não exercitado")
    }
}

/// O `MainThreadTextEditors` que só responde `$tryShowTextDocument` (com o id que o teste escolheu) e registra as opções recebidas.
struct ShowOnlyEditors {
    show_result: RefCell<Option<String>>,
    shown: RefCell<Vec<(String, TextDocumentShowOptions)>>,
}

fn unexercised<T>(name: &str) -> T {
    panic!("o teste não exercita MainThreadTextEditors.{name}")
}

impl IDisposable for ShowOnlyEditors {
    fn dispose(&self) {}
}

impl MainThreadTextEditorsShape for ShowOnlyEditors {
    fn rpc_try_show_text_document(&self, resource: &URI, options: TextDocumentShowOptions) -> Promise<Option<String>> {
        self.shown.borrow_mut().push((resource.path().to_string_lossy(), options));
        Promise::resolved(self.show_result.borrow().clone())
    }

    fn rpc_register_text_editor_decoration_type(&self, _extension_id: &ExtensionIdentifier, _key: &str, _options: DecorationRenderOptions) {
        unexercised("$registerTextEditorDecorationType")
    }

    fn rpc_remove_text_editor_decoration_type(&self, _key: &str) {
        unexercised("$removeTextEditorDecorationType")
    }

    fn rpc_try_show_editor(&self, _id: &str, _position: EditorGroupColumn) -> Promise<()> {
        unexercised("$tryShowEditor")
    }

    fn rpc_try_hide_editor(&self, _id: &str) -> Promise<()> {
        unexercised("$tryHideEditor")
    }

    fn rpc_try_set_options(&self, _id: &str, _options: TextEditorConfigurationUpdate) -> Promise<()> {
        unexercised("$trySetOptions")
    }

    fn rpc_try_set_decorations(&self, _id: &str, _key: &str, _ranges: Vec<DecorationOptionsDto>) -> Promise<()> {
        unexercised("$trySetDecorations")
    }

    fn rpc_try_set_decorations_fast(&self, _id: &str, _key: &str, _ranges: Vec<f64>) -> Promise<()> {
        unexercised("$trySetDecorationsFast")
    }

    fn rpc_try_reveal_range(&self, _id: &str, _range: EditorRange, _reveal_type: TextEditorRevealType) -> Promise<()> {
        unexercised("$tryRevealRange")
    }

    fn rpc_try_set_selections(&self, _id: &str, _selections: Vec<EditorSelection>) -> Promise<()> {
        unexercised("$trySetSelections")
    }

    fn rpc_try_apply_edits(&self, _id: &str, _model_version_id: f64, _edits: Vec<SingleEditOperation>, _opts: ApplyEditsOptions) -> Promise<bool> {
        unexercised("$tryApplyEdits")
    }

    fn rpc_try_insert_snippet(&self, _id: &str, _model_version_id: f64, _template: &str, _selections: &[EditorRange], _opts: SnippetOptions) -> Promise<bool> {
        unexercised("$tryInsertSnippet")
    }

    fn rpc_get_diff_information(&self, _id: &str) -> Promise<Vec<LineChange>> {
        unexercised("$getDiffInformation")
    }
}

fn configuration() -> ResolvedTextEditorConfiguration {
    ResolvedTextEditorConfiguration {
        cursor_style: TextEditorCursorStyle::Line,
        insert_spaces: true,
        line_numbers: RenderLineNumbersType::On,
        tab_size: 4.0,
        indent_size: 4.0,
        original_indent_size: IndentSizeValue::TabSize,
    }
}

fn uri() -> URI {
    URI::parse("file:///a.txt", false)
}

struct Fixture {
    editors: Rc<ExtHostEditors>,
    documents_and_editors: Rc<ExtHostDocumentsAndEditors>,
    proxy: Rc<ShowOnlyEditors>,
}

/// Um documento e um editor (`ed1`) ativo, como o workbench os anunciaria.
fn fixture() -> Fixture {
    let rpc: Rc<dyn IMainContext> = Rc::new(TestRpcProtocol::default());
    let documents: Rc<UnusedDocuments> = Rc::new(UnusedDocuments);
    let documents_shape: Rc<dyn MainThreadDocumentsShape> = documents;
    rpc.set(&MainContext::MAIN_THREAD_DOCUMENTS, documents_shape);
    let proxy = Rc::new(ShowOnlyEditors { show_result: RefCell::new(None), shown: RefCell::new(Vec::new()) });
    let editors_shape: Rc<dyn MainThreadTextEditorsShape> = proxy.clone();
    rpc.set(&MainContext::MAIN_THREAD_TEXT_EDITORS, editors_shape);

    let documents_and_editors = Rc::new(ExtHostDocumentsAndEditors::new(rpc.clone(), NullLogService::shared()));
    documents_and_editors.rpc_accept_documents_and_editors_delta(DocumentsAndEditorsDelta {
        added_documents: Some(vec![ModelAddedData {
            uri: uri(),
            version_id: 1,
            lines: vec![JsString::from("linha um"), JsString::from("linha dois")],
            eol: Eol::Lf,
            language_id: "plaintext".to_owned(),
            is_dirty: false,
            encoding: "utf8".to_owned(),
        }]),
        added_editors: Some(vec![TextEditorAddData {
            id: "ed1".to_owned(),
            document_uri: uri(),
            options: configuration(),
            selections: vec![EditorSelection::new(1, 1, 1, 1)],
            visible_ranges: vec![EditorRange::new(1, 1, 2, 1)],
            editor_position: Some(0.0),
        }]),
        new_active_editor: Some(Some("ed1".to_owned())),
        ..DocumentsAndEditorsDelta::default()
    });
    let editors = ExtHostEditors::new(&*rpc, documents_and_editors.clone());
    Fixture { editors, documents_and_editors, proxy }
}

fn pump() {
    for _ in 0..16 {
        je_base::runtime::turn();
    }
}

#[test]
fn active_and_visible_editors_come_from_the_documents_and_editors_state() {
    let fixture = fixture();
    let active = fixture.editors.get_active_text_editor().expect("há editor ativo");
    assert_eq!(active.view_column(), Some(1.0));
    assert_eq!(fixture.editors.get_visible_text_editors().len(), 1);
}

#[test]
fn selection_changes_update_the_editor_and_fire_with_the_converted_kind() {
    let fixture = fixture();
    let seen = Rc::new(RefCell::new(Vec::new()));
    let sink = seen.clone();
    let _subscription = fixture.editors.on_did_change_text_editor_selection().listen(move |event| {
        sink.borrow_mut().push((event.selections.len(), event.kind, event.selections[0].active().line()));
    });
    fixture.editors.rpc_accept_editor_properties_changed(
        "ed1",
        EditorPropertiesChangeData {
            options: None,
            selections: Some(SelectionChangeEvent { selections: vec![EditorSelection::new(2, 3, 2, 3)], source: Some("keyboard".to_owned()) }),
            visible_ranges: None,
        },
    );
    assert_eq!(*seen.borrow(), vec![(1, Some(TextEditorSelectionChangeKind::Keyboard), 1.0)], "a linha da API começa em zero");
    assert_eq!(fixture.editors.get_active_text_editor().expect("ativo").selections()[0].active().character(), 2.0);
}

#[test]
fn option_changes_fire_with_the_converted_line_numbers_style() {
    let fixture = fixture();
    let seen = Rc::new(RefCell::new(Vec::new()));
    let sink = seen.clone();
    let _subscription = fixture.editors.on_did_change_text_editor_options().listen(move |event| sink.borrow_mut().push((event.options.tab_size, event.line_numbers.value())));
    let mut options = configuration();
    options.tab_size = 8.0;
    options.line_numbers = RenderLineNumbersType::Relative;
    fixture.editors.rpc_accept_editor_properties_changed("ed1", EditorPropertiesChangeData { options: Some(options), selections: None, visible_ranges: None });
    assert_eq!(*seen.borrow(), vec![(8.0, 2)]);
}

#[test]
fn position_data_fires_the_view_column_event_only_when_the_column_changes() {
    let fixture = fixture();
    let seen = Rc::new(RefCell::new(Vec::new()));
    let sink = seen.clone();
    let _subscription = fixture.editors.on_did_change_text_editor_view_column().listen(move |event| sink.borrow_mut().push(event.view_column));
    fixture.editors.rpc_accept_editor_position_data(BTreeMap::from([("ed1".to_owned(), 0.0)]));
    assert!(seen.borrow().is_empty(), "a coluna continua a mesma");
    fixture.editors.rpc_accept_editor_position_data(BTreeMap::from([("ed1".to_owned(), 1.0)]));
    assert_eq!(*seen.borrow(), vec![2.0]);
}

#[test]
fn accepting_changes_for_an_unknown_editor_throws() {
    let fixture = fixture();
    let outcome = je_base::errors::catch(|| {
        fixture.editors.rpc_accept_editor_properties_changed("fantasma", EditorPropertiesChangeData { options: None, selections: None, visible_ranges: None });
    });
    assert_eq!(outcome.expect_err("lança").message, "unknown text editor");
}

#[test]
fn show_text_document_resolves_the_editor_the_workbench_names() {
    let fixture = fixture();
    let document = fixture.documents_and_editors.get_document(&uri()).expect("documento").document();
    *fixture.proxy.show_result.borrow_mut() = Some("ed1".to_owned());
    let shown = fixture.editors.show_text_document(&document, ShowTextDocumentArgument::Column(2.0), Some(true));
    pump();
    assert!(shown.result().expect("assentou").is_ok());
    assert_eq!(fixture.proxy.shown.borrow()[0].1.position, Some(1.0), "ViewColumn.Two é a coluna 1 do grupo");
    assert_eq!(fixture.proxy.shown.borrow()[0].1.preserve_focus, Some(true));

    *fixture.proxy.show_result.borrow_mut() = Some("outro".to_owned());
    let stale = fixture.editors.show_text_document(&document, ShowTextDocumentArgument::Undefined, None);
    pump();
    assert!(stale.result().expect("assentou").expect_err("rejeita").message.contains("because another editor opened in the meantime"));

    *fixture.proxy.show_result.borrow_mut() = None;
    let none = fixture.editors.show_text_document(&document, ShowTextDocumentArgument::Undefined, None);
    pump();
    assert!(none.result().expect("assentou").expect_err("rejeita").message.ends_with("a.txt\"."));
}
