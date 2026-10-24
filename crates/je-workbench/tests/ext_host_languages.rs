//! Teste de unidade do `ExtHostLanguages` (`extHostLanguages.ts`; o oráculo não tem teste dele, então não é porte) contra um dublê do
//! `MainThreadLanguagesShape` que registra as chamadas.

use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use je_base::errors::Error;
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::promise::Promise;
use je_base::severity::Severity;
use je_base::uri::URI;
use je_base::JsString;
use je_editor::model::Eol;
use je_editor::position::Position as EditorPosition;
use je_editor::range::Range as EditorRange;
use je_extensions::manifest::{ExtensionIdentifier, ExtensionManifest, TargetPlatform};
use je_extensions::scanner::ExtensionDescription;
use je_platform::log::NullLogService;
use je_workbench::api::common::ext_host_document_data::IExtHostDocumentSaveDelegate;
use je_workbench::api::common::ext_host_documents::ExtHostDocuments;
use je_workbench::api::common::ext_host_documents_and_editors::ExtHostDocumentsAndEditors;
use je_workbench::api::common::ext_host_languages::ExtHostLanguages;
use je_workbench::api::common::ext_host_protocol::{
    CreateDocumentOptions, DocumentsAndEditorsDelta, ExtHostDocumentsAndEditorsShape, ExtHostLanguagesShape, ICommandDto, ILanguageStatus, IMainContext, ISyntaxHighlightingResultDto,
    MainContext, MainThreadDocumentsShape, MainThreadLanguagesShape, ModelAddedData, OpenDocumentOptions, TokenAtPositionDto,
};
use je_workbench::api::common::ext_host_type_converters::language_selector::DocumentSelectorInput;
use je_workbench::api::common::ext_host_type_converters::ICommandsConverter;
use je_workbench::api::common::ext_host_types::{Command, LanguageStatusSeverity, Position, StandardTokenType};

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

/// O `ICommandsConverter` de comandos sem argumentos: `toInternal` só copia `id` e `title`.
struct PlainCommands;

impl ICommandsConverter for PlainCommands {
    fn from_internal(&self, _command: &ICommandDto) -> Option<Command> {
        None
    }

    fn to_internal(&self, command: Option<&Command>, _disposables: &DisposableStore) -> Result<Option<ICommandDto>, Error> {
        Ok(command.map(|command| ICommandDto { ident: None, id: command.command.clone(), title: command.title.clone(), tooltip: None, arguments: None }))
    }
}

/// O `MainThreadLanguagesShape` que registra as chamadas; `token` é o que `$tokensAtPosition` devolve.
#[derive(Default)]
struct RecordingLanguages {
    calls: RefCell<Vec<String>>,
    statuses: RefCell<Vec<(u32, ILanguageStatus)>>,
    token: RefCell<Option<TokenAtPositionDto>>,
    change_error: RefCell<Option<String>>,
}

impl IDisposable for RecordingLanguages {
    fn dispose(&self) {}
}

impl MainThreadLanguagesShape for RecordingLanguages {
    fn rpc_change_language(&self, resource: URI, language_id: &str) -> Promise<()> {
        self.calls.borrow_mut().push(format!("changeLanguage {} {language_id}", resource.path()));
        match self.change_error.borrow().clone() {
            Some(message) => Promise::rejected(Error::new(message)),
            None => Promise::resolved(()),
        }
    }

    fn rpc_tokens_at_position(&self, _resource: URI, _position: EditorPosition) -> Promise<Option<TokenAtPositionDto>> {
        Promise::resolved(self.token.borrow().clone())
    }

    fn rpc_compute_full_syntax_highlighting(&self, source: &str, language_id: &str) -> Promise<ISyntaxHighlightingResultDto> {
        self.calls.borrow_mut().push(format!("highlight {source} {language_id}"));
        Promise::resolved(ISyntaxHighlightingResultDto { tokens: Vec::new(), color_map: vec!["#fff".to_owned()] })
    }

    fn rpc_set_language_status(&self, handle: u32, status: ILanguageStatus) {
        self.statuses.borrow_mut().push((handle, status));
    }

    fn rpc_remove_language_status(&self, handle: u32) {
        self.calls.borrow_mut().push(format!("removeStatus {handle}"));
    }
}

fn extension() -> ExtensionDescription {
    ExtensionDescription {
        id: "pub.ext".to_owned(),
        identifier: ExtensionIdentifier::new("pub.ext"),
        is_builtin: false,
        is_user_builtin: false,
        is_under_development: false,
        extension_location: "file:///ext".to_owned(),
        uuid: None,
        target_platform: TargetPlatform::Undefined,
        publisher_display_name: None,
        pre_release: false,
        manifest: ExtensionManifest {
            name: "ext".to_owned(),
            display_name: Some("Minha Extensão".to_owned()),
            publisher: "pub".to_owned(),
            version: "1.0.0".to_owned(),
            ..ExtensionManifest::default()
        },
    }
}

fn uri() -> URI {
    URI::parse("file:///a.txt", false)
}

struct Fixture {
    languages: Rc<ExtHostLanguages>,
    proxy: Rc<RecordingLanguages>,
}

fn fixture(with_document: bool) -> Fixture {
    let rpc: Rc<dyn IMainContext> = Rc::new(TestRpcProtocol::default());
    let documents_shape: Rc<dyn MainThreadDocumentsShape> = Rc::new(UnusedDocuments);
    rpc.set(&MainContext::MAIN_THREAD_DOCUMENTS, documents_shape);
    let proxy = Rc::new(RecordingLanguages::default());
    let languages_shape: Rc<dyn MainThreadLanguagesShape> = proxy.clone();
    rpc.set(&MainContext::MAIN_THREAD_LANGUAGES, languages_shape);

    let documents_and_editors = Rc::new(ExtHostDocumentsAndEditors::new(rpc.clone(), NullLogService::shared()));
    if with_document {
        documents_and_editors.rpc_accept_documents_and_editors_delta(DocumentsAndEditorsDelta {
            added_documents: Some(vec![ModelAddedData {
                uri: uri(),
                version_id: 1,
                lines: vec![JsString::from("alfa beta")],
                eol: Eol::Lf,
                language_id: "plaintext".to_owned(),
                is_dirty: false,
                encoding: "utf8".to_owned(),
            }]),
            ..DocumentsAndEditorsDelta::default()
        });
    }
    let documents = ExtHostDocuments::new(&*rpc, documents_and_editors);
    let languages = ExtHostLanguages::new(&*rpc, documents, Rc::new(PlainCommands));
    Fixture { languages, proxy }
}

fn pump() {
    for _ in 0..16 {
        je_base::runtime::turn();
    }
}

#[test]
fn language_ids_come_from_the_workbench_and_are_returned_as_a_copy() {
    let fixture = fixture(false);
    assert!(fixture.languages.get_languages().is_empty());
    fixture.languages.rpc_accept_language_ids(vec!["rust".to_owned(), "json".to_owned()]);
    assert_eq!(fixture.languages.get_languages(), vec!["rust".to_owned(), "json".to_owned()]);
}

#[test]
fn syntax_highlighting_theme_changes_fire_and_the_full_highlighting_is_forwarded() {
    let fixture = fixture(false);
    let fired = Rc::new(RefCell::new(0));
    let counter = fired.clone();
    let _subscription = fixture.languages.on_did_change_syntax_highlighting().listen(move |()| *counter.borrow_mut() += 1);
    fixture.languages.rpc_accept_syntax_highlighting_theme_changed();
    assert_eq!(*fired.borrow(), 1);
    let result = fixture.languages.compute_full_syntax_highlighting("let x", "rust");
    pump();
    assert_eq!(result.result().expect("assentou").expect("sem erro").color_map, vec!["#fff".to_owned()]);
    assert_eq!(fixture.proxy.calls.borrow()[0], "highlight let x rust");
}

#[test]
fn change_language_returns_the_document_or_fails_when_it_is_unknown() {
    let fixture = fixture(true);
    let document = fixture.languages.change_language(&uri(), "rust");
    pump();
    assert_eq!(document.result().expect("assentou").expect("sem erro").uri().path().to_string_lossy(), "/a.txt");
    assert_eq!(fixture.proxy.calls.borrow()[0], "changeLanguage /a.txt rust");

    let missing = fixture.languages.change_language(&URI::parse("file:///outro.txt", false), "rust");
    pump();
    assert_eq!(missing.result().expect("assentou").expect_err("rejeita").message, "document 'file:///outro.txt' NOT found");

    *fixture.proxy.change_error.borrow_mut() = Some("Unknown language id: x".to_owned());
    let failed = fixture.languages.change_language(&uri(), "x");
    pump();
    assert_eq!(failed.result().expect("assentou").expect_err("rejeita").message, "Unknown language id: x");
}

#[test]
fn token_at_position_falls_back_to_the_word_range_when_the_result_is_missing_or_bogus() {
    let fixture = fixture(true);
    let document = fixture.languages.change_language(&uri(), "rust");
    pump();
    let document = document.result().expect("assentou").expect("sem erro");
    let position = Position::new(0.0, 1.0).expect("posição");

    let none = fixture.languages.token_at_position(&document, &position);
    pump();
    let none = none.result().expect("assentou").expect("sem erro");
    assert_eq!((none.token_type, none.range.start().character(), none.range.end().character()), (StandardTokenType::Other, 0.0, 4.0), "o padrão é a palavra sob o cursor");

    *fixture.proxy.token.borrow_mut() = Some(TokenAtPositionDto { token_type: 2, range: EditorRange::new(1, 1, 1, 5) });
    let found = fixture.languages.token_at_position(&document, &position);
    pump();
    assert_eq!(found.result().expect("assentou").expect("sem erro").token_type, StandardTokenType::String);

    *fixture.proxy.token.borrow_mut() = Some(TokenAtPositionDto { token_type: 2, range: EditorRange::new(1, 6, 1, 10) });
    let bogus = fixture.languages.token_at_position(&document, &position);
    pump();
    assert_eq!(bogus.result().expect("assentou").expect("sem erro").token_type, StandardTokenType::Other, "o intervalo não contém a posição");
}

#[test]
fn language_status_items_batch_updates_enforce_unique_ids_and_release_on_dispose() {
    let fixture = fixture(false);
    let selector = DocumentSelectorInput::Language("rust".to_owned());
    let item = fixture.languages.create_language_status_item(&extension(), "estado", selector.clone()).expect("cria");
    item.set_text("pronto");
    item.set_detail(Some("tudo certo".to_owned()));
    item.set_severity(LanguageStatusSeverity::Warning);
    item.set_command(Some(Command::new("Abrir", "x.abrir")));
    item.set_busy(true);
    assert!(fixture.proxy.statuses.borrow().is_empty(), "o envio é adiado para o laço seguinte");
    pump();
    {
        let statuses = fixture.proxy.statuses.borrow();
        assert_eq!(statuses.len(), 1, "as mudanças do mesmo laço viram um envio só");
        let (handle, status) = &statuses[0];
        assert_eq!(*handle, 0);
        assert_eq!((status.id.as_str(), status.label.as_str(), status.detail.as_str(), status.source.as_str()), ("pub.ext/estado", "pronto", "tudo certo", "Minha Extensão"));
        assert_eq!((status.severity, status.busy, status.command.as_ref().map(|command| command.id.as_str())), (Severity::Warning, true, Some("x.abrir")));
        assert!(status.selector.is_some());
    }

    assert_eq!(fixture.languages.create_language_status_item(&extension(), "estado", selector.clone()).err().expect("erro").message, "LanguageStatusItem with id 'estado' ALREADY exists");

    item.dispose();
    assert!(fixture.proxy.calls.borrow().iter().any(|call| call == "removeStatus 0"));
    assert!(fixture.languages.create_language_status_item(&extension(), "estado", selector).is_ok(), "o id volta a ficar livre");
}
