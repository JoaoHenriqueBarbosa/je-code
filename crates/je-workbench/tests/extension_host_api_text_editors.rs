//! Teste de ponta a ponta dos editores e documentos de texto de `vscode.window` e `vscode.workspace` (não é porte do oráculo): uma
//! extensão de exemplo em JavaScript, ativada pelo host local (`AbstractExtensionService` + `LocalExtensionHost` + `RPCProtocol`), lê
//! `textDocuments`, `activeTextEditor` e `visibleTextEditors`, assina os eventos de documento e de editor, muda a seleção, aplica
//! edições, abre documentos e chama `showTextDocument`.
//!
//! O lado do workbench é o `MainThreadDocumentsAndEditors` REAL (com o `MainThreadDocuments` e o `MainThreadTextEditors` dentro dele),
//! ligado pelo binder do teste sobre o `ModelService` real, um `TestCodeEditorService` e os dublês das suítes
//! `mainThreadDocumentsAndEditors.test.ts` e `mainThreadEditors.test.ts` (`support/main_thread_support.rs`). Os documentos nascem de
//! modelos criados no `ModelService` e os editores de `create_test_code_editor`, como nessas suítes.
//!
//! - **Documentos**: `textDocuments` e as propriedades de `TextDocument` (`uri`, `fileName`, `languageId`, `version`, `lineCount`,
//!   `getText`, `isDirty`, `isUntitled`, `isClosed`, `encoding`), `lineAt`, `offsetAt`, `positionAt`, `validateRange`,
//!   `getWordRangeAtPosition`, `openTextDocument` (documento já conhecido, por `Uri` e por caminho) e os eventos de abertura e fechamento.
//! - **Editores**: `activeTextEditor`, `visibleTextEditors`, os eventos de editor ativo, visível e de seleção, `selection`, `edit` com
//!   `TextEditorEdit` (`insert`, `replace`, `delete`) e o erro de usar o construtor depois do `callback`.
//! - **`showTextDocument`**: as opções chegam ao `IEditorService` (por documento e por `Uri`) e a rejeição de quando nenhum editor abre.
//!
//! Fora deste teste: o `openTextDocument` de um recurso que ainda não tem modelo (pede o `ITextModelService` real, que o
//! `main_thread_documents` já cobre) e o `showTextDocument` que resolve um editor (pede um `IEditorPane` completo).

#[allow(dead_code)]
#[path = "../../je-base/tests/common/mod.rs"]
mod common;
#[path = "support/extension_host_api_harness.rs"]
mod harness;
#[allow(dead_code, unused_imports)]
#[path = "support/main_thread_support.rs"]
mod main_thread_support;

/// O suporte de editor do `je-editor/tests/support` que o `create_test_code_editor` precisa, no mesmo módulo pai (os arquivos se
/// referem uns aos outros por `super::`).
#[allow(dead_code, unused_imports)]
#[path = "../../je-editor/tests/support"]
mod editor_support {
    pub mod editor_test_services;
    pub mod test_clipboard_service;
    pub mod test_code_editor;
    pub mod test_code_editor_service;
    pub mod test_color_theme_service;
    pub mod test_configuration;
    pub mod test_instantiation_service;
    pub mod test_language_configuration_service;
    pub mod test_notification_service;
    pub mod test_text_model;
    pub mod test_theme_service;
}

use je_instantiation::Injectable;
use std::cell::RefCell;
use std::rc::Rc;

use editor_support::test_clipboard_service::TestClipboardService;
use editor_support::test_code_editor::{create_test_code_editor, TestCodeEditor, TestCodeEditorInstantiationOptions, I_CODE_EDITOR_SERVICE};
use editor_support::test_code_editor_service::TestCodeEditorService;
use editor_support::test_color_theme_service::TestThemeService;
use harness::{run_extension, Workbench};
use je_base::dynamic::Dynamic;
use je_base::lifecycle::IDisposable;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_editor::model_service::ModelService;
use je_editor::text_model::TextModel;
use je_editor::undo_redo_service::UndoRedoService;
use je_exthost::in_process::IMainThreadBinder;
use je_instantiation::ServiceCollection;
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::files::i_file_service::IFileService;
use je_platform::uri_identity::uri_identity_service::UriIdentityService;
use je_workbench::api::browser::main_thread_documents_and_editors::MainThreadDocumentsAndEditors;
use je_workbench::api::common::ext_host_protocol::IMainContext;
use je_workbench::common::editor::{EditorIdentifier, EditorInputOrUntyped, EditorInputRef, EditorInputWithOptions, EditorsOrder, IEditorPane, UntypedEditorInput};
use je_workbench::services::editor::editor_groups_service::PreferredGroup;
use je_workbench::services::editor::editor_service::{
    ActiveTextEditorControl, FindEditorOptions, IEditorService, SaveAllEditorsOptions, SaveEditorsOptions, SaveEditorsResult,
};
use je_workbench::test::browser::workbench_test_services::{TestEditorGroupsService, TestEditorService, TestEnvironmentService, TestPathService};
use je_workbench::test::common::workbench_test_services::{TestFileService, TestTextResourcePropertiesService};
use main_thread_support::{NoEventWorkingCopyFileService, StubPaneCompositePartService, StubQuickDiffModelService, StubTextFileService, UnusedTextModelService};

/// O JavaScript da extensão de exemplo.
const EXTENSION_SOURCE: &str = r#"
const vscode = require('vscode');
exports.activate = (context) => {
  globalThis.__activated = true;
  globalThis.__events = [];
  const log = (text) => globalThis.__events.push(text);
  const subscribe = (disposable) => context.subscriptions.push(disposable);
  const command = (id, handler) => subscribe(vscode.commands.registerCommand(id, handler));
  const pos = (position) => position.line + ',' + position.character;
  const describe = (selection) => pos(selection.anchor) + '-' + pos(selection.active);
  const describeRange = (range) => (range ? pos(range.start) + '-' + pos(range.end) : 'none');

  subscribe(vscode.window.onDidChangeActiveTextEditor((editor) => log('active:' + (editor ? editor.document.uri.path : 'none'))));
  subscribe(vscode.window.onDidChangeVisibleTextEditors((editors) => log('visible:' + editors.length)));
  subscribe(vscode.window.onDidChangeTextEditorSelection((event) => log('selection:' + event.selections.map(describe).join(';'))));
  subscribe(vscode.workspace.onDidOpenTextDocument((document) => log('open:' + document.uri.path)));
  subscribe(vscode.workspace.onDidCloseTextDocument((document) => log('close:' + document.uri.path)));
  subscribe(vscode.workspace.onDidChangeTextDocument((event) => log('change:' + event.document.uri.path + ':' + event.contentChanges.length)));

  command('t.documents', () => JSON.stringify(vscode.workspace.textDocuments.map((document) => ({
    path: document.uri.path,
    scheme: document.uri.scheme,
    fileName: document.fileName,
    languageId: document.languageId,
    version: document.version,
    lineCount: document.lineCount,
    text: document.getText(),
    isDirty: document.isDirty,
    isUntitled: document.isUntitled,
    isClosed: document.isClosed,
    encoding: document.encoding,
  }))));
  command('t.active', () => {
    const editor = vscode.window.activeTextEditor;
    if (!editor) { return 'none'; }
    return JSON.stringify({
      path: editor.document.uri.path,
      selection: describe(editor.selection),
      selections: editor.selections.length,
      viewColumn: String(editor.viewColumn),
      tabSize: typeof editor.options.tabSize,
      visible: vscode.window.visibleTextEditors.length,
    });
  });
  command('t.documentApi', () => {
    const document = vscode.window.activeTextEditor.document;
    const line = document.lineAt(1);
    return JSON.stringify({
      lineText: line.text,
      lineNumber: line.lineNumber,
      first: line.firstNonWhitespaceCharacterIndex,
      empty: line.isEmptyOrWhitespace,
      offset: document.offsetAt(new vscode.Position(1, 2)),
      position: pos(document.positionAt(11)),
      valid: describeRange(document.validateRange(new vscode.Range(0, 0, 9, 99))),
      word: describeRange(document.getWordRangeAtPosition(new vscode.Position(0, 7))),
      slice: document.getText(new vscode.Range(0, 6, 0, 8)),
      lineCount: document.lineCount,
    });
  });
  command('t.select', () => {
    const editor = vscode.window.activeTextEditor;
    editor.selection = new vscode.Selection(0, 2, 0, 6);
    return describe(editor.selection);
  });
  command('t.edit', async () => {
    const editor = vscode.window.activeTextEditor;
    const applied = await editor.edit((builder) => {
      builder.insert(new vscode.Position(0, 0), '>> ');
      builder.replace(new vscode.Range(0, 6, 0, 8), 'UM');
      builder.delete(new vscode.Range(1, 0, 1, 6));
    });
    return String(applied) + '|' + editor.document.getText();
  });
  command('t.editReuse', async () => {
    const editor = vscode.window.activeTextEditor;
    let saved;
    await editor.edit((builder) => { saved = builder; });
    try {
      saved.insert(new vscode.Position(0, 0), 'x');
      return 'sem erro';
    } catch (error) {
      return error.message;
    }
  });
  command('t.open', async () => {
    const byUri = await vscode.workspace.openTextDocument(vscode.Uri.file('/proj/a.txt'));
    const byPath = await vscode.workspace.openTextDocument('/proj/a.txt');
    return byUri.languageId + '|' + byUri.getText() + '|' + byPath.uri.path + '|' + vscode.workspace.textDocuments.length;
  });
  command('t.show', async () => {
    const document = vscode.window.activeTextEditor.document;
    const messages = [];
    try {
      await vscode.window.showTextDocument(document, { preserveFocus: true, selection: new vscode.Range(0, 1, 0, 3) });
      messages.push('sem erro');
    } catch (error) {
      messages.push(error.message);
    }
    try {
      await vscode.window.showTextDocument(vscode.Uri.file('/proj/a.txt'));
      messages.push('sem erro');
    } catch (error) {
      messages.push(error.message);
    }
    return messages.join('|');
  });
  command('t.decorations', () => {
    const editor = vscode.window.activeTextEditor;
    const type = vscode.window.createTextEditorDecorationType({
      backgroundColor: new vscode.ThemeColor('editor.findMatchBackground'),
      isWholeLine: true,
      overviewRulerLane: 4,
      before: { contentText: '>', color: '#ff0000' },
      light: { borderColor: '#000000' },
    });
    editor.setDecorations(type, [new vscode.Range(0, 0, 0, 5)]);
    const key = type.key;
    const withRanges = key.startsWith('TextEditorDecorationType') + '|' + typeof type.dispose;
    editor.setDecorations(type, [{ range: new vscode.Range(1, 0, 1, 3), hoverMessage: 'dica', renderOptions: { after: { contentText: '!' } } }]);
    editor.setDecorations(type, []);
    type.dispose();
    return withRanges;
  });
  command('t.insertSnippet', async () => {
    const editor = vscode.window.activeTextEditor;
    const snippet = new vscode.SnippetString('x').appendTabstop(1).appendText('$y');
    const results = [];
    results.push(String(await editor.insertSnippet(snippet)));
    results.push(String(await editor.insertSnippet(snippet, new vscode.Position(0, 0))));
    results.push(String(await editor.insertSnippet(snippet, [new vscode.Range(0, 0, 0, 1), new vscode.Position(1, 0)], { undoStopBefore: false })));
    try {
      await editor.insertSnippet('texto');
      results.push('sem erro');
    } catch (error) {
      results.push(error.message);
    }
    return results.join('|');
  });
};
"#;

/// O que `open_editor_in` recebeu: o recurso, `preserveFocus` e a seleção (`TextEditorSelection`).
#[derive(Debug, PartialEq)]
struct OpenedEditor {
    resource: String,
    preserve_focus: Option<bool>,
    selection: Option<(f64, f64, Option<f64>, Option<f64>)>,
}

/// O `IEditorService` das suítes (`TestEditorService`) que grava os pedidos de `open_editor_in` e responde que nenhum editor abriu.
struct RecordingEditorService {
    inner: TestEditorService,
    opened: RefCell<Vec<OpenedEditor>>,
}

impl IEditorService for RecordingEditorService {
    fn visible_editor_panes(&self) -> Vec<Rc<dyn IEditorPane>> {
        self.inner.visible_editor_panes()
    }

    fn active_editor(&self) -> Option<EditorInputOrUntyped> {
        self.inner.active_editor()
    }

    fn active_text_editor_control(&self) -> Option<ActiveTextEditorControl> {
        self.inner.active_text_editor_control()
    }

    fn active_text_editor_language_id(&self) -> Option<String> {
        self.inner.active_text_editor_language_id()
    }

    fn open_editor(&self, editor: UntypedEditorInput) -> Promise<Option<Rc<dyn IEditorPane>>> {
        self.inner.open_editor(editor)
    }

    fn find_editors(&self, resource: &URI, options: Option<FindEditorOptions>) -> Vec<EditorIdentifier> {
        self.inner.find_editors(resource, options)
    }

    fn save(&self, editors: Vec<EditorIdentifier>, options: Option<SaveEditorsOptions>) -> Promise<SaveEditorsResult> {
        self.inner.save(editors, options)
    }

    fn save_all(&self, options: Option<SaveAllEditorsOptions>) -> Promise<SaveEditorsResult> {
        self.inner.save_all(options)
    }

    fn get_editors(&self, order: EditorsOrder) -> Vec<EditorIdentifier> {
        self.inner.get_editors(order)
    }

    fn open_editors(&self, editors: Vec<EditorInputWithOptions>) -> Promise<Vec<Rc<dyn IEditorPane>>> {
        self.inner.open_editors(editors)
    }

    fn is_visible(&self, editor: &EditorInputRef) -> bool {
        self.inner.is_visible(editor)
    }

    fn open_editor_in(&self, editor: UntypedEditorInput, _group: Option<PreferredGroup>) -> Promise<Option<Rc<dyn IEditorPane>>> {
        let options = editor.options.as_ref();
        self.opened.borrow_mut().push(OpenedEditor {
            resource: editor.resource.as_ref().map(|resource| resource.to_string(false).to_string_lossy()).unwrap_or_default(),
            preserve_focus: options.and_then(|options| options.base.preserve_focus),
            selection: options
                .and_then(|options| options.selection.as_ref())
                .map(|selection| (selection.start_line_number, selection.start_column, selection.end_line_number, selection.end_column)),
        });
        Promise::resolved(None)
    }
}

/// Liga o `MainThreadDocumentsAndEditors` (real, com o `MainThreadDocuments` e o `MainThreadTextEditors`) ao host.
struct Binder {
    model_service: Rc<ModelService>,
    code_editor_service: Rc<TestCodeEditorService>,
    editor_service: Rc<RecordingEditorService>,
    file_service: Rc<TestFileService>,
}

impl IMainThreadBinder for Binder {
    fn bind(&self, context: Rc<dyn IMainContext>) -> Vec<Rc<dyn IDisposable>> {
        let file_service: Rc<dyn IFileService> = self.file_service.clone();
        let uri_identity_service = UriIdentityService::create((), (file_service.clone(),));
        let documents_and_editors = MainThreadDocumentsAndEditors::new(
            &context,
            self.model_service.clone(),
            Rc::new(StubTextFileService),
            self.editor_service.clone(),
            self.code_editor_service.registry().clone(),
            file_service,
            Rc::new(UnusedTextModelService),
            TestEditorGroupsService::new(None),
            Rc::new(StubPaneCompositePartService),
            TestEnvironmentService::new(),
            Rc::new(NoEventWorkingCopyFileService),
            uri_identity_service,
            Rc::new(TestClipboardService::new()),
            TestPathService::new(None, None),
            TestConfigurationService::new(None),
            Rc::new(StubQuickDiffModelService),
        );
        let client: Rc<dyn IDisposable> = documents_and_editors;
        vec![client]
    }
}

/// O que o teste usa do lado do workbench.
struct Observed {
    model_service: Rc<ModelService>,
    code_editor_service: Rc<TestCodeEditorService>,
    editor_service: Rc<RecordingEditorService>,
}

fn setup(_workbench: &Workbench) -> (Rc<dyn IMainThreadBinder>, Observed) {
    let config_service = TestConfigurationService::new(None);
    let _ = config_service.set_user_configuration("editor", Dynamic::object([("detectIndentation", Dynamic::Bool(false))]), None);
    let configuration_service: Rc<dyn IConfigurationService> = config_service;
    let model_service = ModelService::new(configuration_service.clone(), TestTextResourcePropertiesService::new(configuration_service), UndoRedoService::for_tests());
    let code_editor_service = Rc::new(TestCodeEditorService::new(TestThemeService::new_default()));
    let editor_service = Rc::new(RecordingEditorService { inner: TestEditorService, opened: RefCell::new(Vec::new()) });
    let binder = Binder {
        model_service: model_service.clone(),
        code_editor_service: code_editor_service.clone(),
        editor_service: editor_service.clone(),
        file_service: TestFileService::new(),
    };
    (Rc::new(binder), Observed { model_service, code_editor_service, editor_service })
}

/// O URI de um arquivo do projeto de exemplo (`/proj/<name>`).
fn project_file(name: &str) -> URI {
    URI::file(format!("/proj/{name}"), je_platform::resources::host_platform())
}

/// Um modelo no `ModelService` do arquivo `name` do projeto de exemplo (vira um `TextDocument` na extensão).
fn create_document(observed: &Observed, name: &str, text: &str) -> Rc<TextModel> {
    observed.model_service.create_model(text, None, Some(&project_file(name)), false)
}

/// Um editor de código sobre `model` que tem o foco de texto (vira o `activeTextEditor`).
fn create_focused_editor(observed: &Observed, model: &Rc<TextModel>) -> Rc<TestCodeEditor> {
    let service_collection = ServiceCollection::new();
    service_collection.set(I_CODE_EDITOR_SERVICE, observed.code_editor_service.clone());
    create_test_code_editor(
        Some(model.clone()),
        TestCodeEditorInstantiationOptions { options: Dynamic::object([("hasTextFocus", Dynamic::Bool(true))]), service_collection: Some(service_collection) },
    )
}

/// `JSON.stringify(globalThis.__events)`, o que os ouvintes da extensão registraram.
fn events(workbench: &Workbench) -> String {
    workbench.probe("JSON.stringify(globalThis.__events)")
}

fn run(workbench: &Workbench, command: &str) -> Dynamic {
    workbench.execute(command, Vec::new())
}

#[test]
fn text_documents_follow_the_models_of_the_workbench() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, observed| {
        assert_eq!(run(workbench, "t.documents"), Dynamic::string("[]"));

        let model = create_document(&observed, "a.txt", "linha um\nlinha dois");
        let document = concat!(
            r#"{"path":"/proj/a.txt","scheme":"file","fileName":"/proj/a.txt","languageId":"plaintext","version":1,"lineCount":2,"#,
            r#""text":"linha um\nlinha dois","isDirty":false,"isUntitled":false,"isClosed":false,"encoding":"utf8"}"#,
        );
        assert_eq!(run(workbench, "t.documents"), Dynamic::string(&format!("[{document}]")));

        model.dispose();
        assert_eq!(run(workbench, "t.documents"), Dynamic::string("[]"));
        assert_eq!(events(workbench), r#"["open:/proj/a.txt","close:/proj/a.txt"]"#, "abrir e fechar o modelo dispara os dois eventos");
    });
}

#[test]
fn document_api_reads_lines_offsets_positions_and_words() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, observed| {
        let model = create_document(&observed, "a.txt", "linha um\nlinha dois");
        let editor = create_focused_editor(&observed, &model);

        assert_eq!(
            run(workbench, "t.documentApi"),
            Dynamic::string(concat!(
                r#"{"lineText":"linha dois","lineNumber":1,"first":0,"empty":false,"offset":11,"position":"1,2","#,
                r#""valid":"0,0-1,10","word":"0,6-0,8","slice":"um","lineCount":2}"#,
            ))
        );

        editor.dispose();
        model.dispose();
    });
}

#[test]
fn active_and_visible_text_editors_follow_the_editors_of_the_workbench() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, observed| {
        assert_eq!(run(workbench, "t.active"), Dynamic::string("none"));

        let model = create_document(&observed, "a.txt", "linha um\nlinha dois");
        let editor = create_focused_editor(&observed, &model);

        assert_eq!(
            run(workbench, "t.active"),
            Dynamic::string(r#"{"path":"/proj/a.txt","selection":"0,0-0,0","selections":1,"viewColumn":"undefined","tabSize":"number","visible":1}"#)
        );
        let seen = events(workbench);
        assert!(seen.contains(r#""visible:1""#), "o editor novo muda os editores visíveis: {seen}");
        assert!(seen.contains(r#""active:/proj/a.txt""#), "o editor com foco vira o ativo: {seen}");

        editor.dispose();
        assert_eq!(run(workbench, "t.active"), Dynamic::string("none"));
        let seen = events(workbench);
        assert!(seen.contains(r#""active:none""#), "sem editor o ativo é undefined: {seen}");
        assert!(seen.contains(r#""visible:0""#), "o editor fechado sai dos visíveis: {seen}");
        model.dispose();
    });
}

#[test]
fn setting_the_selection_reaches_the_editor_and_comes_back_as_an_event() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, observed| {
        let model = create_document(&observed, "a.txt", "linha um\nlinha dois");
        let editor = create_focused_editor(&observed, &model);

        assert_eq!(run(workbench, "t.select"), Dynamic::string("0,2-0,6"));
        let seen = events(workbench);
        assert!(seen.contains(r#""selection:0,2-0,6""#), "o editor devolve a seleção por onDidChangeTextEditorSelection: {seen}");
        assert_eq!(
            run(workbench, "t.active"),
            Dynamic::string(r#"{"path":"/proj/a.txt","selection":"0,2-0,6","selections":1,"viewColumn":"undefined","tabSize":"number","visible":1}"#)
        );

        editor.dispose();
        model.dispose();
    });
}

#[test]
fn edit_applies_insert_replace_and_delete_to_the_model_and_the_document() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, observed| {
        let model = create_document(&observed, "a.txt", "linha um\nlinha dois");
        let editor = create_focused_editor(&observed, &model);

        assert_eq!(run(workbench, "t.edit"), Dynamic::string("true|>> linha UM\ndois"));
        let seen = events(workbench);
        assert!(seen.contains(r#""change:/proj/a.txt:"#), "a edição volta como onDidChangeTextDocument: {seen}");

        editor.dispose();
        model.dispose();
    });
}

#[test]
fn text_editor_edit_throws_when_used_after_the_callback() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, observed| {
        let model = create_document(&observed, "a.txt", "linha um");
        let editor = create_focused_editor(&observed, &model);

        assert_eq!(run(workbench, "t.editReuse"), Dynamic::string("Edit is only valid while callback runs"));

        editor.dispose();
        model.dispose();
    });
}

#[test]
fn open_text_document_returns_a_known_document_by_uri_and_by_path() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, observed| {
        let model = create_document(&observed, "a.txt", "linha um\nlinha dois");

        assert_eq!(run(workbench, "t.open"), Dynamic::string("plaintext|linha um\nlinha dois|/proj/a.txt|1"));

        model.dispose();
    });
}

#[test]
fn decoration_types_and_set_decorations_reach_the_main_thread_editor() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, observed| {
        let model = create_document(&observed, "a.txt", "linha um\nlinha dois");
        let editor = create_focused_editor(&observed, &model);

        assert_eq!(run(workbench, "t.decorations"), Dynamic::string("true|function"), "o tipo recebe uma chave gerada e expõe dispose");
        // a última chamada limpou a lista: nada fica no modelo
        let leftover = model.get_all_decorations(0, false, false).len();
        assert_eq!(leftover, 0, "setDecorations com lista vazia remove as decorações do tipo");

        editor.dispose();
        model.dispose();
    });
}

#[test]
fn insert_snippet_validates_its_arguments_and_asks_the_main_thread_editor() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, observed| {
        let model = create_document(&observed, "a.txt", "linha um\nlinha dois");
        let editor = create_focused_editor(&observed, &model);

        // o editor de teste não tem o `SnippetController2` (`contributions: []`), então o main thread responde `false` a cada pedido
        assert_eq!(
            run(workbench, "t.insertSnippet"),
            Dynamic::string("false|false|false|insertSnippet: esperava um SnippetString"),
            "location ausente, Position e lista de Range e Position chegam ao main thread, e a string pura é recusada na ponte"
        );

        editor.dispose();
        model.dispose();
    });
}

#[test]
fn show_text_document_sends_the_options_to_the_editor_service_and_rejects_when_no_editor_opens() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, observed| {
        let model = create_document(&observed, "a.txt", "linha um\nlinha dois");
        let editor = create_focused_editor(&observed, &model);

        let refused = r#"Could NOT open editor for "file:///proj/a.txt"."#;
        assert_eq!(run(workbench, "t.show"), Dynamic::string(&format!("{refused}|{refused}")));

        assert_eq!(
            *observed.editor_service.opened.borrow(),
            vec![
                OpenedEditor { resource: "file:///proj/a.txt".to_owned(), preserve_focus: Some(true), selection: Some((1.0, 2.0, Some(1.0), Some(4.0))) },
                OpenedEditor { resource: "file:///proj/a.txt".to_owned(), preserve_focus: Some(false), selection: None },
            ],
            "o documento e a Uri chegam como o mesmo recurso, com as opções de cada chamada"
        );

        editor.dispose();
        model.dispose();
    });
}
