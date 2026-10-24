//! `vscode.languages`: os diagnósticos (`createDiagnosticCollection`, `getDiagnostics`, `onDidChangeDiagnostics`), a lista e a troca de
//! linguagem (`getLanguages`, `setTextDocumentLanguage`), a pontuação de seletor (`match`) e os registros de provedor
//! (`registerCompletionItemProvider`, `registerHoverProvider`, `registerDefinitionProvider`, `registerDeclarationProvider`,
//! `registerImplementationProvider`, `registerTypeDefinitionProvider`, `registerCodeActionsProvider`,
//! `registerDocumentFormattingEditProvider`, `registerDocumentRangeFormattingEditProvider` e `registerDocumentSymbolProvider`), mais a
//! classe `vscode.DiagnosticCollection` (protótipo das coleções que o host entrega).
//!
//! É a ponte para o `ExtHostDiagnostics`, o `ExtHostLanguages` e o `ExtHostLanguageFeatures` portados (`extHostDiagnostics.ts`,
//! `extHostLanguages.ts`, `extHostLanguageFeatures.ts` e o literal `languages` de `extHost.api.impl.ts`). Cada grupo só entra em
//! `vscode.languages` se o binder registrou o cliente `MainThread*` de que ele depende (`MainThreadDiagnostics`, `MainThreadLanguages`,
//! `MainThreadLanguageFeatures`) e os documentos de texto (`MainThreadDocuments` e `MainThreadTextEditors`) estão ligados.
//!
//! Falta (o serviço do host tem, falta só a ponte): `registerReferenceProvider`, `registerRenameProvider`,
//! `registerSignatureHelpProvider`, `registerFoldingRangeProvider`, `registerCodeLensProvider`, `registerInlayHintsProvider`,
//! `registerDocumentLinkProvider`, `registerDocumentHighlightProvider`, `registerOnTypeFormattingEditProvider`,
//! `registerWorkspaceSymbolProvider` e os demais; `setLanguageConfiguration`, `createLanguageStatusItem`, `tokenAtPosition` e
//! `onDidChangeSyntaxHighlighting`; `DiagnosticCollection#[Symbol.iterator]`.
//!
//! Divergências:
//! - O `vscode` é um só para todas as extensões: o host não sabe qual delas chama, então os registros e as coleções usam a extensão
//!   `je.exthost` (a descrição que o binder não conhece), e a verificação das propostas de API (`checkProposedApiEnabled`) enxerga uma
//!   extensão sem propostas.
//! - O provedor chega por REFERÊNCIA (`NodeRunner::make_function_by_reference`), então os métodos de classe e o `this` do provedor
//!   funcionam. A consequência é que o host não consegue perguntar "o provedor tem este método?" (`typeof provider.resolveX ===
//!   'function'`): `resolveCompletionItem`, `resolveCodeAction` e `provideDocumentRangesFormattingEdits` não são ligados (os `has_*`
//!   do trait valem `false`), mesmo que o provedor os defina.
//! - O resultado do provedor passa pelo motor como cópia: a identidade dos itens que o provedor guarda (para o `resolve`) não existe.
//! - `HoverContext#previousHover` não é entregue (só `verbosityDelta`), porque o host só tem o `Hover` já convertido para o formato do
//!   editor, sem o objeto da extensão.
//! - `DiagnosticCollection#forEach` percorre um retrato das entradas; `thisArg` só vale se for função (o motor copia objetos comuns).
//! - As classes de valor (`MarkdownString`, `SnippetString`, `WorkspaceEdit`) e a lista do que falta nelas estão em `language_types`.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use je_base::cancellation::CancellationToken;
use je_base::errors::Error;
use je_base::promise::{MaybePromise, Promise};
use je_editor::language_selector::score;
use je_extensions::manifest::{ExtensionIdentifier, ExtensionManifest, TargetPlatform};
use je_extensions::scanner::ExtensionDescription;
use je_workbench::api::common::ext_host_diagnostics::{DiagnosticCollection, ExtHostDiagnostics};
use je_workbench::api::common::ext_host_document_data::TextDocument;
use je_workbench::api::common::ext_host_language_features::ExtHostLanguageFeatures;
use je_workbench::api::common::ext_host_language_features_api::{
    ApiCodeActionContext, ApiCodeActionProvider, ApiCompletionItemProvider, ApiCompletionResult, ApiDocumentFormattingEditProvider, ApiDocumentRangeFormattingEditProvider,
    ApiDocumentSymbolProvider, ApiDocumentSymbols, ApiHoverContext, ApiHoverProvider, ApiLocationLinkProvider, ApiLocationResult, ApiProviderResult, ApiRangeOrSelection,
    CodeActionDocumentation, CodeActionProviderMetadata, CommandOrCodeAction, DocumentSymbolProviderMetadata,
};
use je_workbench::api::common::ext_host_languages::ExtHostLanguages;
use je_workbench::api::common::ext_host_protocol::FormattingOptions;
use je_workbench::api::common::ext_host_type_converters::language_selector::LanguageSelector;
use je_workbench::api::common::ext_host_type_converters::{DocumentFilterInput, DocumentSelectorInput};
use je_workbench::api::common::ext_host_types::{CompletionContext, Diagnostic, Position, Range, TextEdit, VerboseHover};
use zjsc::api::embed_node::{ClassSpec, JsData, JsHandle, MethodCallback, NativeCallback, NodeRunner, CLASS_KEY};

use crate::api::commands::{promise_to_js, settle_js_result};
use crate::api::disposable::DisposableTable;
use crate::api::events::{event_function, EventTables};
use crate::api::language_types::{
    self, code_action_kind_data, code_action_kind_from_js, code_actions_from_js, command_from_js, completion_result_from_js, diagnostics_from_js,
    document_symbols_from_js, hover_from_js, location_result_from_js, text_edits_from_js,
};
use crate::api::text_editors::{TextEditorApi, TextObjects};
use crate::api::types::{position_data, range_data, selection_data};
use crate::api::uri::guarded;
use crate::api::workspace::glob_of;
use crate::api::HOST_EXTENSION;
use crate::convert::{api_to_js, field, is_nullish, js_to_error, opt_bool, opt_string, string_arg, uri_from_js, uri_to_js};

/// Os serviços do host que o namespace usa. Cada grupo de `vscode.languages` só existe quando o serviço dele existe.
pub struct LanguagesServices {
    /// Sem o cliente `MainThreadLanguages` não há `getLanguages` nem `setTextDocumentLanguage`.
    pub languages: Option<Rc<ExtHostLanguages>>,
    /// Sem o cliente `MainThreadDiagnostics` não há coleções, `getDiagnostics` nem `onDidChangeDiagnostics`.
    pub diagnostics: Option<Rc<ExtHostDiagnostics>>,
    /// Sem o cliente `MainThreadLanguageFeatures` não há `register*Provider`.
    pub features: Option<Rc<ExtHostLanguageFeatures>>,
    /// Os documentos (`TextDocument` do motor) e os objetos vivos: sempre presentes, porque os três serviços acima pedem documentos.
    pub text_editors: TextEditorApi,
}

/// As coleções de diagnósticos vivas, por id (o objeto JavaScript carrega só o id).
#[derive(Default)]
pub struct DiagnosticCollections {
    next: Cell<u64>,
    items: RefCell<HashMap<u64, Rc<DiagnosticCollection>>>,
}

impl DiagnosticCollections {
    fn add(&self, collection: Rc<DiagnosticCollection>) -> JsData {
        let id = self.next.get() + 1;
        self.next.set(id);
        self.items.borrow_mut().insert(id, collection);
        JsData::Object(vec![(CLASS_KEY.to_owned(), JsData::Str("DiagnosticCollection".to_owned())), ("_id".to_owned(), JsData::Num(id as f64))])
    }

    fn of(&self, this: &JsData) -> Result<Rc<DiagnosticCollection>, String> {
        match field(this, "_id") {
            Some(JsData::Num(id)) => self.items.borrow().get(&(*id as u64)).cloned().ok_or_else(|| "DiagnosticCollection: coleção desconhecida".to_owned()),
            _ => Err("DiagnosticCollection: this inválido".to_owned()),
        }
    }
}

/// O que o namespace `languages` guarda de cada chamada que devolve um objeto vivo para a extensão.
#[derive(Default)]
pub struct LanguagesState {
    pub collections: Rc<DiagnosticCollections>,
}

/// A extensão que o host usa nos registros (ver as divergências do módulo).
fn host_extension() -> Rc<ExtensionDescription> {
    Rc::new(ExtensionDescription {
        id: HOST_EXTENSION.to_owned(),
        identifier: ExtensionIdentifier::new(HOST_EXTENSION),
        is_builtin: false,
        is_user_builtin: false,
        is_under_development: false,
        extension_location: String::new(),
        uuid: None,
        target_platform: TargetPlatform::Undefined,
        publisher_display_name: None,
        pre_release: false,
        manifest: ExtensionManifest { name: "exthost".to_owned(), publisher: "je".to_owned(), version: "1.0.0".to_owned(), ..ExtensionManifest::default() },
    })
}

// --- seletores

/// O valor do argumento por referência como dado: o objeto é copiado, o resto fica como veio.
fn plain(runner: &NodeRunner, value: &JsData) -> JsData {
    match value {
        JsData::Handle(handle) => runner.export(handle),
        other => other.clone(),
    }
}

/// `vscode.DocumentSelector`: texto, `DocumentFilter` ou uma lista deles.
fn selector_from(value: &JsData) -> Result<DocumentSelectorInput, String> {
    match value {
        JsData::Str(language) => Ok(DocumentSelectorInput::Language(language.clone())),
        JsData::Array(items) => Ok(DocumentSelectorInput::Array(items.iter().map(selector_from).collect::<Result<Vec<_>, _>>()?)),
        JsData::Object(_) => Ok(DocumentSelectorInput::Filter(DocumentFilterInput {
            language: opt_string(field(value, "language")),
            scheme: opt_string(field(value, "scheme")),
            pattern: match field(value, "pattern") {
                Some(pattern) if !is_nullish(Some(pattern)) => Some(glob_of(pattern)?),
                _ => None,
            },
            exclusive: opt_bool(field(value, "exclusive")),
            notebook_type: opt_string(field(value, "notebookType")),
        })),
        _ => Err("o seletor precisa ser texto, um DocumentFilter ou uma lista deles".to_owned()),
    }
}

// --- provedores

/// Um provedor JavaScript: o objeto (por referência, com o `this` e os métodos da classe) e o que a conversão precisa.
#[derive(Clone)]
struct JsProvider {
    runner: NodeRunner,
    provider: Rc<JsHandle>,
    objects: Rc<TextObjects>,
    events: Rc<EventTables>,
}

/// O resultado de um método do provedor (valor, ou promessa que o `then` adota) como `ProviderResult<T>`; uma conversão que falha é uma
/// promessa rejeitada, como o `throw` síncrono do original.
fn settle<T: Clone + 'static>(runner: &NodeRunner, result: JsData, convert: fn(&JsData) -> Result<Option<T>, Error>) -> ApiProviderResult<T> {
    match settle_js_result(runner, result) {
        MaybePromise::Value(value) => match convert(&crate::convert::dynamic_to_js(&value)) {
            Ok(converted) => MaybePromise::Value(converted),
            Err(error) => MaybePromise::Promise(Promise::rejected(error)),
        },
        MaybePromise::Promise(promise) => MaybePromise::Promise(promise.then_result(move |outcome| convert(&crate::convert::dynamic_to_js(&outcome?)))),
    }
}

impl JsProvider {
    /// `provider[method](...args)`: o resultado convertido por `convert`. Uma exceção do provedor é uma promessa rejeitada.
    fn invoke<T: Clone + 'static>(&self, method: &str, args: Vec<JsData>, convert: fn(&JsData) -> Result<Option<T>, Error>) -> ApiProviderResult<T> {
        match self.runner.call_method(&self.provider, method, &args) {
            Ok(result) => settle(&self.runner, result, convert),
            Err(message) => MaybePromise::Promise(Promise::rejected(Error::new(message))),
        }
    }

    fn document(&self, document: &TextDocument) -> JsData {
        self.objects.document_to_js(document)
    }

    fn token(&self, token: CancellationToken) -> JsData {
        self.events.token_to_js(token)
    }
}

fn formatting_options_to_js(options: &FormattingOptions) -> JsData {
    JsData::Object(vec![("tabSize".to_owned(), JsData::Num(options.tab_size as f64)), ("insertSpaces".to_owned(), JsData::Bool(options.insert_spaces))])
}

struct CompletionProvider(JsProvider);

impl ApiCompletionItemProvider for CompletionProvider {
    fn provide_completion_items(&self, document: &TextDocument, position: &Position, token: CancellationToken, context: &CompletionContext) -> ApiProviderResult<ApiCompletionResult> {
        let context = JsData::Object(vec![
            ("triggerKind".to_owned(), JsData::Num(f64::from(context.trigger_kind.value()))),
            ("triggerCharacter".to_owned(), context.trigger_character.clone().map_or(JsData::Undefined, JsData::Str)),
        ]);
        self.0.invoke("provideCompletionItems", vec![self.0.document(document), position_data(*position), self.0.token(token), context], completion_result_from_js)
    }
}

struct HoverProvider(JsProvider);

impl ApiHoverProvider for HoverProvider {
    fn provide_hover(&self, document: &TextDocument, position: &Position, token: CancellationToken, context: Option<&ApiHoverContext>) -> ApiProviderResult<VerboseHover> {
        let mut args = vec![self.0.document(document), position_data(*position), self.0.token(token)];
        if let Some(context) = context {
            args.push(JsData::Object(vec![("verbosityDelta".to_owned(), JsData::Num(context.verbosity_delta))]));
        }
        self.0.invoke("provideHover", args, hover_from_js)
    }
}

/// Os quatro provedores de localização têm a mesma forma; só muda o método do provedor.
struct LocationLinkProvider {
    provider: JsProvider,
    method: &'static str,
}

impl ApiLocationLinkProvider for LocationLinkProvider {
    fn provide_location_links(&self, document: &TextDocument, position: &Position, token: CancellationToken) -> ApiProviderResult<ApiLocationResult> {
        self.provider.invoke(self.method, vec![self.provider.document(document), position_data(*position), self.provider.token(token)], location_result_from_js)
    }
}

struct CodeActionProvider(JsProvider);

impl ApiCodeActionProvider for CodeActionProvider {
    fn provide_code_actions(
        &self,
        document: &TextDocument,
        range_or_selection: &ApiRangeOrSelection,
        context: &ApiCodeActionContext,
        token: CancellationToken,
    ) -> ApiProviderResult<Vec<CommandOrCodeAction>> {
        let range = match range_or_selection {
            ApiRangeOrSelection::Range(range) => range_data(*range),
            ApiRangeOrSelection::Selection(selection) => selection_data(*selection),
        };
        let context = JsData::Object(vec![
            ("diagnostics".to_owned(), JsData::Array(context.diagnostics.iter().map(api_to_js).collect())),
            ("only".to_owned(), context.only.as_ref().map_or(JsData::Undefined, code_action_kind_data)),
            ("triggerKind".to_owned(), JsData::Num(f64::from(context.trigger_kind.value()))),
        ]);
        self.0.invoke("provideCodeActions", vec![self.0.document(document), range, context, self.0.token(token)], code_actions_from_js)
    }
}

struct DocumentFormattingProvider(JsProvider);

impl ApiDocumentFormattingEditProvider for DocumentFormattingProvider {
    fn provide_document_formatting_edits(&self, document: &TextDocument, options: &FormattingOptions, token: CancellationToken) -> ApiProviderResult<Vec<TextEdit>> {
        self.0.invoke("provideDocumentFormattingEdits", vec![self.0.document(document), formatting_options_to_js(options), self.0.token(token)], text_edits_from_js)
    }
}

struct DocumentRangeFormattingProvider(JsProvider);

impl ApiDocumentRangeFormattingEditProvider for DocumentRangeFormattingProvider {
    fn provide_document_range_formatting_edits(&self, document: &TextDocument, range: &Range, options: &FormattingOptions, token: CancellationToken) -> ApiProviderResult<Vec<TextEdit>> {
        self.0.invoke(
            "provideDocumentRangeFormattingEdits",
            vec![self.0.document(document), range_data(*range), formatting_options_to_js(options), self.0.token(token)],
            text_edits_from_js,
        )
    }
}

struct DocumentSymbolProvider(JsProvider);

impl ApiDocumentSymbolProvider for DocumentSymbolProvider {
    fn provide_document_symbols(&self, document: &TextDocument, token: CancellationToken) -> ApiProviderResult<ApiDocumentSymbols> {
        self.0.invoke("provideDocumentSymbols", vec![self.0.document(document), self.0.token(token)], document_symbols_from_js)
    }
}

/// `metadata` de `registerCodeActionsProvider`: `providedCodeActionKinds` e `documentation`.
fn code_action_metadata_from_js(value: &JsData) -> Result<CodeActionProviderMetadata, Error> {
    let provided_code_action_kinds = match field(value, "providedCodeActionKinds") {
        Some(JsData::Array(kinds)) => Some(kinds.iter().filter_map(code_action_kind_from_js).collect()),
        _ => None,
    };
    let documentation = match field(value, "documentation") {
        Some(JsData::Array(entries)) => Some(
            entries
                .iter()
                .map(|entry| -> Result<CodeActionDocumentation, Error> {
                    let kind = field(entry, "kind").and_then(code_action_kind_from_js).ok_or_else(|| Error::new("documentation.kind must be a CodeActionKind"))?;
                    let command = command_from_js(field(entry, "command").ok_or_else(|| Error::new("documentation.command must be a Command"))?)?;
                    Ok(CodeActionDocumentation { kind, command })
                })
                .collect::<Result<Vec<_>, Error>>()?,
        ),
        _ => None,
    };
    Ok(CodeActionProviderMetadata { provided_code_action_kinds, documentation })
}

/// Tudo que um `register*Provider` precisa além do provedor.
struct Registrar {
    runner: NodeRunner,
    features: Rc<ExtHostLanguageFeatures>,
    extension: Rc<ExtensionDescription>,
    objects: Rc<TextObjects>,
    events: Rc<EventTables>,
    table: Rc<DisposableTable>,
}

impl Registrar {
    /// Os argumentos `(selector, provider, ...rest)` de um `register*Provider`: lê o seletor, monta o provedor e devolve o `Disposable`
    /// do registro.
    fn register(&self, args: &[JsData], build: impl FnOnce(&ExtHostLanguageFeatures, &ExtensionDescription, &DocumentSelectorInput, JsProvider, &[JsData]) -> je_base::lifecycle::DisposableRef) -> Result<JsData, String> {
        let selector = selector_from(&plain(&self.runner, args.first().ok_or("o seletor é obrigatório")?))?;
        let Some(JsData::Handle(provider)) = args.get(1) else { return Err("o provedor precisa ser um objeto".to_owned()) };
        let provider = JsProvider { runner: self.runner.clone(), provider: provider.clone(), objects: self.objects.clone(), events: self.events.clone() };
        let registration = build(&*self.features, &*self.extension, &selector, provider, args.get(2..).unwrap_or(&[]));
        Ok(self.table.to_js(registration))
    }
}

/// As funções `register*Provider`, cada uma por referência.
fn provider_members(runner: &NodeRunner, registrar: &Rc<Registrar>) -> Vec<(String, JsData)> {
    let member = |name: &str, length: u32, body: NativeCallback| (name.to_owned(), JsData::Handle(Rc::new(runner.make_function_by_reference(name, length, body))));
    let location = |name: &str, method: &'static str, register: fn(&ExtHostLanguageFeatures, &ExtensionDescription, &DocumentSelectorInput, Rc<dyn ApiLocationLinkProvider>) -> je_base::lifecycle::DisposableRef| {
        let registrar = registrar.clone();
        member(
            name,
            2,
            Rc::new(move |args| registrar.register(args, |features, extension, selector, provider, _| register(features, extension, selector, Rc::new(LocationLinkProvider { provider, method })))),
        )
    };
    let completion = {
        let registrar = registrar.clone();
        member(
            "registerCompletionItemProvider",
            3,
            Rc::new(move |args| {
                registrar.register(args, |features, extension, selector, provider, rest| {
                    let trigger_characters = rest.iter().filter_map(|character| opt_string(Some(character))).collect();
                    features.register_completion_item_provider(extension, selector, Rc::new(CompletionProvider(provider)), trigger_characters)
                })
            }),
        )
    };
    let hover = {
        let registrar = registrar.clone();
        member(
            "registerHoverProvider",
            2,
            Rc::new(move |args| registrar.register(args, |features, extension, selector, provider, _| features.register_hover_provider(extension, selector, Rc::new(HoverProvider(provider))))),
        )
    };
    let code_actions = {
        let (registrar, runner) = (registrar.clone(), runner.clone());
        member(
            "registerCodeActionsProvider",
            3,
            Rc::new(move |args| {
                let metadata = match args.get(2) {
                    Some(metadata) if !is_nullish(Some(metadata)) => Some(code_action_metadata_from_js(&plain(&runner, metadata)).map_err(|error| error.message)?),
                    _ => None,
                };
                registrar.register(args, |features, extension, selector, provider, _| {
                    features.register_code_action_provider(extension, selector, Rc::new(CodeActionProvider(provider)), metadata.as_ref())
                })
            }),
        )
    };
    let formatting = {
        let registrar = registrar.clone();
        member(
            "registerDocumentFormattingEditProvider",
            2,
            Rc::new(move |args| {
                registrar.register(args, |features, extension, selector, provider, _| {
                    features.register_document_formatting_edit_provider(extension, selector, Rc::new(DocumentFormattingProvider(provider)))
                })
            }),
        )
    };
    let range_formatting = {
        let registrar = registrar.clone();
        member(
            "registerDocumentRangeFormattingEditProvider",
            2,
            Rc::new(move |args| {
                registrar.register(args, |features, extension, selector, provider, _| {
                    features.register_document_range_formatting_edit_provider(extension, selector, Rc::new(DocumentRangeFormattingProvider(provider)))
                })
            }),
        )
    };
    let symbols = {
        let (registrar, runner) = (registrar.clone(), runner.clone());
        member(
            "registerDocumentSymbolProvider",
            3,
            Rc::new(move |args| {
                let metadata = match args.get(2) {
                    Some(metadata) if !is_nullish(Some(metadata)) => Some(DocumentSymbolProviderMetadata { label: opt_string(field(&plain(&runner, metadata), "label")) }),
                    _ => None,
                };
                registrar.register(args, |features, extension, selector, provider, _| {
                    features.register_document_symbol_provider(extension, selector, Rc::new(DocumentSymbolProvider(provider)), metadata.as_ref())
                })
            }),
        )
    };
    vec![
        completion,
        hover,
        location("registerDefinitionProvider", "provideDefinition", ExtHostLanguageFeatures::register_definition_provider),
        location("registerDeclarationProvider", "provideDeclaration", ExtHostLanguageFeatures::register_declaration_provider),
        location("registerImplementationProvider", "provideImplementation", ExtHostLanguageFeatures::register_implementation_provider),
        location("registerTypeDefinitionProvider", "provideTypeDefinition", ExtHostLanguageFeatures::register_type_definition_provider),
        code_actions,
        formatting,
        range_formatting,
        symbols,
    ]
}

// --- diagnósticos

fn define_collection_class(runner: &NodeRunner, collections: &Rc<DiagnosticCollections>) {
    let method = |name: &str, length: u32, body: Rc<dyn Fn(&JsData, &Rc<DiagnosticCollection>, &[JsData]) -> Result<JsData, String>>| -> (String, u32, MethodCallback) {
        let collections = collections.clone();
        (name.to_owned(), length, Rc::new(move |this, args| body(this, &collections.of(this)?, args)))
    };
    let for_each_runner = runner.clone();
    let methods: Vec<(String, u32, MethodCallback)> = vec![
        method(
            "set",
            2,
            Rc::new(|_, collection, args| {
                match args.first() {
                    // `set(entries)`: cada entrada é `[uri, diagnostics | undefined]`
                    Some(JsData::Array(entries)) => {
                        let mut parsed: Vec<(je_base::uri::URI, Option<Vec<Option<Diagnostic>>>)> = Vec::with_capacity(entries.len());
                        for entry in entries {
                            let JsData::Array(pair) = entry else { return Err("DiagnosticCollection.set: cada entrada precisa ser [uri, diagnostics]".to_owned()) };
                            let uri = uri_from_js(pair.first().ok_or("DiagnosticCollection.set: falta a Uri")?)?;
                            let diagnostics = match pair.get(1) {
                                Some(JsData::Array(items)) => Some(diagnostics_from_js(items).map_err(|error| error.message)?),
                                _ => None,
                            };
                            parsed.push((uri, diagnostics));
                        }
                        guarded(|| collection.set_many(parsed))?;
                    }
                    // `set(uri, diagnostics?)`
                    Some(uri @ JsData::Object(_)) => {
                        let uri = uri_from_js(uri)?;
                        let diagnostics = match args.get(1) {
                            Some(JsData::Array(items)) => Some(diagnostics_from_js(items).map_err(|error| error.message)?),
                            _ => None,
                        };
                        guarded(|| collection.set(&uri, diagnostics))?;
                    }
                    // `set(undefined)` limpa a coleção
                    _ => guarded(|| collection.clear())?,
                }
                Ok(JsData::Undefined)
            }),
        ),
        method(
            "get",
            1,
            Rc::new(|_, collection, args| {
                let uri = uri_from_js(args.first().ok_or("DiagnosticCollection.get: falta a Uri")?)?;
                Ok(JsData::Array(guarded(|| collection.get(&uri))?.iter().map(api_to_js).collect()))
            }),
        ),
        method(
            "has",
            1,
            Rc::new(|_, collection, args| {
                let uri = uri_from_js(args.first().ok_or("DiagnosticCollection.has: falta a Uri")?)?;
                Ok(JsData::Bool(guarded(|| collection.has(&uri))?))
            }),
        ),
        method(
            "delete",
            1,
            Rc::new(|_, collection, args| {
                let uri = uri_from_js(args.first().ok_or("DiagnosticCollection.delete: falta a Uri")?)?;
                guarded(|| collection.delete(&uri))?;
                Ok(JsData::Undefined)
            }),
        ),
        method(
            "clear",
            0,
            Rc::new(|_, collection, _| {
                guarded(|| collection.clear())?;
                Ok(JsData::Undefined)
            }),
        ),
        method(
            "forEach",
            2,
            Rc::new(move |this, collection, args| {
                let Some(JsData::Handle(callback)) = args.first() else { return Err("DiagnosticCollection.forEach: o callback precisa ser função".to_owned()) };
                for (uri, diagnostics) in guarded(|| collection.entries())? {
                    let call_args = vec![uri_to_js(&uri), JsData::Array(diagnostics.iter().map(api_to_js).collect()), this.clone()];
                    match args.get(1) {
                        Some(this_arg @ JsData::Handle(_)) => {
                            let mut full = vec![this_arg.clone()];
                            full.extend(call_args);
                            for_each_runner.call_method(callback, "call", &full).map(|_| ())?;
                        }
                        _ => for_each_runner.call_data_throwing(callback, &call_args).map(|_| ()).map_err(|thrown| js_to_error(&thrown).message)?,
                    }
                }
                Ok(JsData::Undefined)
            }),
        ),
        method(
            "dispose",
            0,
            Rc::new(|_, collection, _| {
                guarded(|| collection.dispose())?;
                Ok(JsData::Undefined)
            }),
        ),
    ];
    let name_collections = collections.clone();
    let getters: Vec<(String, MethodCallback)> = vec![(
        "name".to_owned(),
        Rc::new(move |this, _| {
            let collection = name_collections.of(this)?;
            Ok(JsData::Str(guarded(|| collection.name().to_owned())?))
        }),
    )];
    runner.define_class(ClassSpec {
        name: "DiagnosticCollection".to_owned(),
        length: 0,
        construct: Rc::new(|_| Err("DiagnosticCollection não tem construtor público: use languages.createDiagnosticCollection".to_owned())),
        methods,
        getters,
        statics: vec![],
        parent: None,
    });
}

/// O objeto `vscode.languages`.
pub fn namespace(runner: &NodeRunner, services: &LanguagesServices, table: &Rc<DisposableTable>, events: &Rc<EventTables>, state: &LanguagesState) -> JsData {
    let mut members: Vec<(String, JsData)> = Vec::new();
    let function = |name: &str, length: u32, body: NativeCallback| (name.to_owned(), JsData::Handle(Rc::new(runner.make_function(name, length, body))));
    let objects = services.text_editors.objects.clone();

    let match_objects = objects.clone();
    members.push(function(
        "match",
        2,
        Rc::new(move |args| {
            let selector = selector_from(args.first().ok_or("match: falta o seletor")?)?;
            let document = match_objects.document_of(args.get(1).ok_or("match: falta o documento")?)?;
            let selector = LanguageSelector::from(Some(&selector));
            Ok(JsData::Num(f64::from(score(selector.as_ref(), document.uri(), &document.language_id(), true, None, None))))
        }),
    ));

    if let Some(languages) = &services.languages {
        let (get_runner, get_languages) = (runner.clone(), languages.clone());
        members.push(function(
            "getLanguages",
            0,
            Rc::new(move |_| Ok(promise_to_js(&get_runner, Promise::resolved(get_languages.get_languages()), |ids| JsData::Array(ids.iter().cloned().map(JsData::Str).collect())))),
        ));
        let (set_runner, set_languages, set_objects) = (runner.clone(), languages.clone(), objects.clone());
        members.push(function(
            "setTextDocumentLanguage",
            2,
            Rc::new(move |args| {
                let document = set_objects.document_of(args.first().ok_or("setTextDocumentLanguage: falta o documento")?)?;
                let language_id = string_arg(args, 1, "setTextDocumentLanguage: o languageId")?;
                let changed = set_languages.change_language(document.uri(), &language_id);
                let result_objects = set_objects.clone();
                Ok(promise_to_js(&set_runner, changed, move |document| result_objects.document_to_js(document)))
            }),
        ));
    }

    if let Some(diagnostics) = &services.diagnostics {
        let (create_diagnostics, create_collections) = (diagnostics.clone(), state.collections.clone());
        members.push(function(
            "createDiagnosticCollection",
            1,
            Rc::new(move |args| {
                let name = opt_string(args.first());
                let collection = guarded(|| create_diagnostics.create_diagnostic_collection(&ExtensionIdentifier::new(HOST_EXTENSION), name.as_deref()))?;
                Ok(create_collections.add(collection))
            }),
        ));
        let get_diagnostics = diagnostics.clone();
        members.push(function(
            "getDiagnostics",
            1,
            Rc::new(move |args| match args.first() {
                Some(uri @ JsData::Object(_)) => {
                    let diagnostics = get_diagnostics.get_diagnostics(&uri_from_js(uri)?);
                    Ok(JsData::Array(diagnostics.iter().map(api_to_js).collect()))
                }
                _ => Ok(JsData::Array(
                    get_diagnostics
                        .get_all_diagnostics()
                        .iter()
                        .map(|(uri, diagnostics)| JsData::Array(vec![uri_to_js(uri), JsData::Array(diagnostics.iter().map(api_to_js).collect())]))
                        .collect(),
                )),
            }),
        ));
        members.push((
            "onDidChangeDiagnostics".to_owned(),
            event_function(runner, table, "onDidChangeDiagnostics", diagnostics.on_did_change_diagnostics.clone(), |event| {
                JsData::Object(vec![("uris".to_owned(), JsData::Array(event.uris.iter().map(uri_to_js).collect()))])
            }),
        ));
    }

    if let Some(features) = &services.features {
        let registrar = Rc::new(Registrar {
            runner: runner.clone(),
            features: features.clone(),
            extension: host_extension(),
            objects,
            events: events.clone(),
            table: table.clone(),
        });
        members.extend(provider_members(runner, &registrar));
    }
    JsData::Object(members)
}

/// As classes deste módulo: as de valor (`language_types`) e o protótipo de `DiagnosticCollection`.
pub fn classes(runner: &NodeRunner, state: &LanguagesState) -> Vec<(String, JsData)> {
    define_collection_class(runner, &state.collections);
    language_types::classes(runner)
}
