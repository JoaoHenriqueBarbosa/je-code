//! `vscode.languages`: os diagnósticos (`createDiagnosticCollection`, `getDiagnostics`, `onDidChangeDiagnostics`), a lista e a troca de
//! linguagem (`getLanguages`, `setTextDocumentLanguage`), a pontuação de seletor (`match`) e os registros de provedor
//! (`registerCompletionItemProvider`, `registerHoverProvider`, `registerDefinitionProvider`, `registerDeclarationProvider`,
//! `registerImplementationProvider`, `registerTypeDefinitionProvider`, `registerCodeActionsProvider`,
//! `registerDocumentFormattingEditProvider`, `registerDocumentRangeFormattingEditProvider` e `registerDocumentSymbolProvider`; os demais
//! estão em `language_providers`), mais a classe `vscode.DiagnosticCollection` (protótipo das coleções que o host entrega).
//!
//! É a ponte para o `ExtHostDiagnostics`, o `ExtHostLanguages` e o `ExtHostLanguageFeatures` portados (`extHostDiagnostics.ts`,
//! `extHostLanguages.ts`, `extHostLanguageFeatures.ts` e o literal `languages` de `extHost.api.impl.ts`). Cada grupo só entra em
//! `vscode.languages` se o binder registrou o cliente `MainThread*` de que ele depende (`MainThreadDiagnostics`, `MainThreadLanguages`,
//! `MainThreadLanguageFeatures`) e os documentos de texto (`MainThreadDocuments` e `MainThreadTextEditors`) estão ligados.
//!
//! Falta (o serviço do host tem, falta só a ponte): `tokenAtPosition` e `onDidChangeSyntaxHighlighting`;
//! `DiagnosticCollection#[Symbol.iterator]`. `setLanguageConfiguration` está em `language_providers` (a leitura da configuração, em
//! `language_configuration`) e `createLanguageStatusItem` aqui, com a classe em `language_status`.
//!
//! Divergências:
//! - O `vscode` é um só para todas as extensões: o host não sabe qual delas chama, então os registros e as coleções usam a extensão
//!   `je.exthost` (a descrição que o binder não conhece), e a verificação das propostas de API (`checkProposedApiEnabled`) enxerga uma
//!   extensão sem propostas.
//! - O provedor chega por REFERÊNCIA (`NodeRunner::make_function_by_reference`), então os métodos de classe e o `this` do provedor
//!   funcionam, e os `has_*` do trait (`typeof provider.resolveX === 'function'`) são respondidos lendo o próprio objeto do provedor
//!   ([`JsProvider::has_method`]), como no original. Os eventos (`onDidChangeCodeLenses`...) são assinados no objeto do provedor.
//! - O resultado do provedor passa pelo motor como cópia: a identidade dos itens que o provedor guarda (para o `resolve`) não existe.
//!   O host guarda, por item devolvido, a cópia original ([`Originals`]) e a entrega ao `resolveX` do provedor; o provedor que altera o
//!   item no lugar e devolve `undefined` perde a alteração (o original também usa o item devolvido, não o alterado, quando devolve outro).
//! - `HoverContext#previousHover` é o `Hover` que o `_hoverMap` do adaptador guarda por id, relido como objeto da extensão
//!   (`language_types::hover_data`, com o `range` que o adaptador preencheu); a identidade do objeto original não existe.
//! - `DiagnosticCollection#forEach` percorre um retrato das entradas; `thisArg` só vale se for função (o motor copia objetos comuns).
//! - As classes de valor (`MarkdownString`, `SnippetString`, `WorkspaceEdit`) e a lista do que falta nelas estão em `language_types`.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use je_base::cancellation::CancellationToken;
use je_base::errors::{on_unexpected_error, Error};
use je_base::event::{Event, Listener};
use je_base::lifecycle::{plain_disposable, DisposableRef};
use je_base::promise::{MaybePromise, Promise};
use je_editor::language_selector::score;
use je_extensions::manifest::{ExtensionIdentifier, ExtensionManifest, TargetPlatform};
use je_extensions::scanner::ExtensionDescription;
use je_workbench::api::common::ext_host_diagnostics::{DiagnosticCollection, ExtHostDiagnostics};
use je_workbench::api::common::ext_host_document_data::TextDocument;
use je_workbench::api::common::ext_host_language_features::ExtHostLanguageFeatures;
use je_workbench::api::common::ext_host_language_features_api::{
    ApiCodeActionContext, ApiCodeActionProvider, ApiCodeActionRef, ApiCompletionItemProvider, ApiCompletionResult, ApiDocumentFormattingEditProvider,
    ApiDocumentRangeFormattingEditProvider, ApiDocumentSymbolProvider, ApiDocumentSymbols, ApiHoverContext, ApiHoverProvider, ApiLocationLinkProvider, ApiLocationResult,
    ApiProviderResult, ApiRangeOrSelection, CodeActionDocumentation, CodeActionProviderMetadata, CommandOrCodeAction, CompletionItemRef, DocumentSymbolProviderMetadata,
};
use je_workbench::api::common::ext_host_languages::ExtHostLanguages;
use je_workbench::api::common::ext_host_protocol::FormattingOptions;
use je_workbench::api::common::ext_host_type_converters::language_selector::LanguageSelector;
use je_workbench::api::common::ext_host_type_converters::{DocumentFilterInput, DocumentSelectorInput};
use je_workbench::api::common::ext_host_types::{CodeAction, CompletionContext, CompletionItem, Diagnostic, Position, Range, TextEdit, VerboseHover};
use zjsc::api::embed_node::{ClassSpec, JsData, JsHandle, MethodCallback, NativeCallback, NodeRunner, CLASS_KEY};

use crate::api::commands::{promise_to_js, settle_js_result};
use crate::api::disposable::DisposableTable;
use crate::api::events::{event_function, EventTables};
use crate::api::js_object::JsObjects;
use crate::api::language_provider_types::{self, SemanticTokensBuilders};
use crate::api::language_providers;
use crate::api::language_status::{self, LanguageStatusItems};
use crate::api::language_types::{
    self, code_action_kind_data, code_action_kind_from_js, code_action_raw_entries, code_action_ref_from_js, code_actions_from_js, command_from_js,
    completion_item_ref_from_js, completion_raw_items, completion_result_from_js, diagnostics_from_js, document_symbols_from_js, hover_data, hover_from_js, location_result_from_js,
    text_edits_from_js,
};
use crate::api::text_editors::{TextEditorApi, TextObjects};
use crate::api::types::{position_data, range_data, selection_data};
use crate::api::uri::guarded;
use crate::api::workspace::glob_of;
use crate::api::workspace_edit::WorkspaceEditObjects;
use crate::api::HOST_EXTENSION;
use crate::convert::{api_to_js, exported, field, is_nullish, js_to_error, opt_bool, opt_string, string_arg, uri_from_js, uri_to_js};

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
    /// As `WorkspaceEdit` que as extensões criaram: `provideRenameEdits` e `CodeAction#edit` devolvem uma delas.
    pub workspace_edits: Rc<WorkspaceEditObjects>,
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
    pub builders: Rc<SemanticTokensBuilders>,
    pub status_items: Rc<LanguageStatusItems>,
}

/// A extensão que o host usa nos registros (ver as divergências do módulo).
pub fn host_extension() -> Rc<ExtensionDescription> {
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

/// `vscode.DocumentSelector`: texto, `DocumentFilter` ou uma lista deles.
pub(crate) fn selector_from(value: &JsData) -> Result<DocumentSelectorInput, String> {
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

/// Os valores como o provedor os devolveu (cópias do motor), por objeto que o adaptador guarda por referência: o `resolveX` do provedor
/// recebe a cópia original, porque o motor não entrega o objeto do provedor com a identidade dele. Uma entrada morre com o objeto (o
/// adaptador solta o cache e o `Weak` deixa de ter dono).
pub(crate) struct Originals<T> {
    items: RefCell<Vec<(Weak<RefCell<T>>, JsData)>>,
}

impl<T> Default for Originals<T> {
    fn default() -> Self {
        Self { items: RefCell::new(Vec::new()) }
    }
}

impl<T> Originals<T> {
    /// Guarda a cópia original de cada objeto (descarta antes as entradas cujo objeto já morreu).
    pub(crate) fn remember<'a>(&self, pairs: impl IntoIterator<Item = (&'a Rc<RefCell<T>>, &'a JsData)>) where T: 'a {
        let mut items = self.items.borrow_mut();
        items.retain(|(weak, _)| weak.strong_count() > 0);
        items.extend(pairs.into_iter().map(|(item, value)| (Rc::downgrade(item), value.clone())));
    }

    /// A cópia original de `item`, se o provedor o devolveu.
    pub(crate) fn of(&self, item: &Rc<RefCell<T>>) -> Option<JsData> {
        let target = Rc::downgrade(item);
        self.items.borrow().iter().find(|(weak, _)| weak.ptr_eq(&target)).map(|(_, value)| value.clone())
    }
}

/// Um provedor JavaScript: o objeto (por referência, com o `this` e os métodos da classe) e o que a conversão precisa.
#[derive(Clone)]
pub(crate) struct JsProvider {
    pub(crate) provider: Rc<JsHandle>,
    pub(crate) objects: Rc<TextObjects>,
    pub(crate) events: Rc<EventTables>,
    pub(crate) js: JsObjects,
    pub(crate) table: Rc<DisposableTable>,
    pub(crate) edits: Rc<WorkspaceEditObjects>,
}

/// O resultado de um método do provedor (valor, ou promessa que o `then` adota) como `ProviderResult<T>`; uma conversão que falha é uma
/// promessa rejeitada, como o `throw` síncrono do original.
fn settle<T: Clone + 'static>(runner: &NodeRunner, result: JsData, convert: impl Fn(&JsData) -> Result<Option<T>, Error> + 'static) -> ApiProviderResult<T> {
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
    pub(crate) fn invoke<T: Clone + 'static>(
        &self,
        method: &str,
        args: Vec<JsData>,
        convert: impl Fn(&JsData) -> Result<Option<T>, Error> + 'static,
    ) -> ApiProviderResult<T> {
        let runner = self.js.runner();
        match runner.call_method(&self.provider, method, &args) {
            Ok(result) => settle(runner, result, convert),
            Err(message) => MaybePromise::Promise(Promise::rejected(Error::new(message))),
        }
    }

    /// `provider[method](original, token)` de um `resolveX`: `original` é a cópia que o provedor devolveu antes (ver [`Originals`]); um
    /// objeto que este provedor não devolveu não tem original, e o resultado é "sem resolução".
    pub(crate) fn resolve<T: 'static>(
        &self,
        method: &str,
        originals: &Originals<T>,
        item: &Rc<RefCell<T>>,
        token: CancellationToken,
        convert: impl Fn(&JsData) -> Result<Option<Rc<RefCell<T>>>, Error> + 'static,
    ) -> ApiProviderResult<Rc<RefCell<T>>> {
        let Some(original) = originals.of(item) else { return MaybePromise::Value(None) };
        self.invoke(method, vec![original, self.token(token)], convert)
    }

    pub(crate) fn document(&self, document: &TextDocument) -> JsData {
        self.objects.document_to_js(document)
    }

    pub(crate) fn token(&self, token: CancellationToken) -> JsData {
        self.events.token_to_js(token)
    }

    /// `typeof provider[name] === 'function'`.
    pub(crate) fn has_method(&self, name: &str) -> bool {
        self.js.is_function(&JsData::Handle(self.provider.clone()), name)
    }

    /// O evento `provider[name]` (`onDidChangeCodeLenses`...), se o provedor o tem: cada disparo dele é um disparo do `Event<()>` do host, e
    /// o `Disposable` da escuta desfaz a assinatura no provedor.
    pub(crate) fn change_event(&self, name: &str) -> Option<Event<()>> {
        let target = JsData::Handle(self.provider.clone());
        let JsData::Handle(event) = self.js.property(&target, name) else { return None };
        if !self.js.is_function(&target, name) {
            return None;
        }
        let (js, table) = (self.js.clone(), self.table.clone());
        Some(Event::new(move |listener: Listener<()>| -> DisposableRef {
            match js.listen(&event, move |_| listener(&())) {
                Ok(Some(id)) => {
                    let table = table.clone();
                    plain_disposable(move || table.dispose(id))
                }
                Ok(None) => plain_disposable(|| {}),
                Err(message) => {
                    on_unexpected_error(&Error::new(message));
                    plain_disposable(|| {})
                }
            }
        }))
    }
}

pub(crate) fn formatting_options_to_js(options: &FormattingOptions) -> JsData {
    JsData::Object(vec![("tabSize".to_owned(), JsData::Num(options.tab_size as f64)), ("insertSpaces".to_owned(), JsData::Bool(options.insert_spaces))])
}

struct CompletionProvider {
    js: JsProvider,
    originals: Rc<Originals<CompletionItem>>,
}

impl CompletionProvider {
    fn new(js: JsProvider) -> Self {
        Self { js, originals: Rc::default() }
    }
}

fn completion_refs(result: &ApiCompletionResult) -> &[CompletionItemRef] {
    match result {
        ApiCompletionResult::Items(items) => items,
        ApiCompletionResult::List(list) => &list.items,
    }
}

impl ApiCompletionItemProvider for CompletionProvider {
    fn provide_completion_items(&self, document: &TextDocument, position: &Position, token: CancellationToken, context: &CompletionContext) -> ApiProviderResult<ApiCompletionResult> {
        let context = JsData::Object(vec![
            ("triggerKind".to_owned(), JsData::Num(f64::from(context.trigger_kind.value()))),
            ("triggerCharacter".to_owned(), context.trigger_character.clone().map_or(JsData::Undefined, JsData::Str)),
        ]);
        let originals = self.originals.clone();
        self.js.invoke("provideCompletionItems", vec![self.js.document(document), position_data(*position), self.js.token(token), context], move |value| {
            let result = completion_result_from_js(value)?;
            if let Some(result) = &result {
                originals.remember(completion_refs(result).iter().zip(completion_raw_items(value)));
            }
            Ok(result)
        })
    }

    fn has_resolve_completion_item(&self) -> bool {
        self.js.has_method("resolveCompletionItem")
    }

    fn resolve_completion_item(&self, item: &CompletionItemRef, token: CancellationToken) -> ApiProviderResult<CompletionItemRef> {
        self.js.resolve("resolveCompletionItem", &self.originals, item, token, completion_item_ref_from_js)
    }
}

struct HoverProvider(JsProvider);

impl ApiHoverProvider for HoverProvider {
    fn provide_hover(&self, document: &TextDocument, position: &Position, token: CancellationToken, context: Option<&ApiHoverContext>) -> ApiProviderResult<VerboseHover> {
        let mut args = vec![self.0.document(document), position_data(*position), self.0.token(token)];
        if let Some(context) = context {
            args.push(JsData::Object(vec![
                ("verbosityDelta".to_owned(), JsData::Num(context.verbosity_delta)),
                ("previousHover".to_owned(), hover_data(&context.previous_hover)),
            ]));
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

struct CodeActionProvider {
    js: JsProvider,
    originals: Rc<Originals<CodeAction>>,
}

impl CodeActionProvider {
    fn new(js: JsProvider) -> Self {
        Self { js, originals: Rc::default() }
    }
}

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
        let (originals, edits) = (self.originals.clone(), self.js.edits.clone());
        self.js.invoke("provideCodeActions", vec![self.js.document(document), range, context, self.js.token(token)], move |value| {
            let actions = code_actions_from_js(value, &edits)?;
            if let (Some(actions), JsData::Array(entries)) = (&actions, value) {
                originals.remember(code_action_raw_entries(entries).zip(actions.iter()).filter_map(|(raw, action)| match action {
                    CommandOrCodeAction::CodeAction(action) => Some((action, raw)),
                    CommandOrCodeAction::Command(_) => None,
                }));
            }
            Ok(actions)
        })
    }

    fn has_resolve_code_action(&self) -> bool {
        self.js.has_method("resolveCodeAction")
    }

    fn resolve_code_action(&self, code_action: &ApiCodeActionRef, token: CancellationToken) -> ApiProviderResult<ApiCodeActionRef> {
        let edits = self.js.edits.clone();
        self.js.resolve("resolveCodeAction", &self.originals, code_action, token, move |value| code_action_ref_from_js(value, &edits))
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

    fn has_provide_document_ranges_formatting_edits(&self) -> bool {
        self.0.has_method("provideDocumentRangesFormattingEdits")
    }

    fn provide_document_ranges_formatting_edits(
        &self,
        document: &TextDocument,
        ranges: &[Range],
        options: &FormattingOptions,
        token: CancellationToken,
    ) -> ApiProviderResult<Vec<TextEdit>> {
        let ranges = JsData::Array(ranges.iter().map(|range| range_data(*range)).collect());
        self.0.invoke("provideDocumentRangesFormattingEdits", vec![self.0.document(document), ranges, formatting_options_to_js(options), self.0.token(token)], text_edits_from_js)
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
pub(crate) struct Registrar {
    pub(crate) runner: NodeRunner,
    pub(crate) features: Rc<ExtHostLanguageFeatures>,
    pub(crate) extension: Rc<ExtensionDescription>,
    pub(crate) objects: Rc<TextObjects>,
    pub(crate) events: Rc<EventTables>,
    pub(crate) table: Rc<DisposableTable>,
    pub(crate) js: JsObjects,
    pub(crate) edits: Rc<WorkspaceEditObjects>,
}

impl Registrar {
    /// O provedor JavaScript (o objeto por referência) com o que a conversão precisa.
    pub(crate) fn js_provider(&self, provider: &Rc<JsHandle>) -> JsProvider {
        JsProvider {
            provider: provider.clone(),
            objects: self.objects.clone(),
            events: self.events.clone(),
            js: self.js.clone(),
            table: self.table.clone(),
            edits: self.edits.clone(),
        }
    }

    /// Os argumentos `(selector, provider, ...rest)` de um `register*Provider`: lê o seletor, monta o provedor e devolve o `Disposable`
    /// do registro.
    pub(crate) fn register(
        &self,
        args: &[JsData],
        build: impl FnOnce(&ExtHostLanguageFeatures, &ExtensionDescription, &DocumentSelectorInput, JsProvider, &[JsData]) -> DisposableRef,
    ) -> Result<JsData, String> {
        let selector = selector_from(&exported(&self.runner, args.first().ok_or("o seletor é obrigatório")?))?;
        let Some(JsData::Handle(provider)) = args.get(1) else { return Err("o provedor precisa ser um objeto".to_owned()) };
        let provider = self.js_provider(provider);
        let registration = build(&*self.features, &*self.extension, &selector, provider, args.get(2..).unwrap_or(&[]));
        Ok(self.table.to_js(registration))
    }
}

/// Uma função `register*Provider` por referência.
pub(crate) fn member(
    runner: &NodeRunner,
    registrar: &Rc<Registrar>,
    name: &str,
    length: u32,
    body: impl Fn(&Registrar, &[JsData]) -> Result<JsData, String> + 'static,
) -> (String, JsData) {
    let registrar = registrar.clone();
    let callback: NativeCallback = Rc::new(move |args| body(&registrar, args));
    (name.to_owned(), JsData::Handle(Rc::new(runner.make_function_by_reference(name, length, callback))))
}

/// `register*Provider(selector, provider)`, sem argumentos além do provedor.
pub(crate) fn simple(
    runner: &NodeRunner,
    registrar: &Rc<Registrar>,
    name: &str,
    register: impl Fn(&ExtHostLanguageFeatures, &ExtensionDescription, &DocumentSelectorInput, JsProvider) -> DisposableRef + 'static,
) -> (String, JsData) {
    member(runner, registrar, name, 2, move |registrar, args| {
        registrar.register(args, |features, extension, selector, provider, _| register(features, extension, selector, provider))
    })
}

/// As funções `register*Provider`, cada uma por referência.
fn provider_members(runner: &NodeRunner, registrar: &Rc<Registrar>) -> Vec<(String, JsData)> {
    let location = |name: &str, method: &'static str, register: fn(&ExtHostLanguageFeatures, &ExtensionDescription, &DocumentSelectorInput, Rc<dyn ApiLocationLinkProvider>) -> DisposableRef| {
        simple(runner, registrar, name, move |features, extension, selector, provider| register(features, extension, selector, Rc::new(LocationLinkProvider { provider, method })))
    };
    let mut members = vec![
        member(runner, registrar, "registerCompletionItemProvider", 3, |registrar, args| {
            registrar.register(args, |features, extension, selector, provider, rest| {
                let trigger_characters = rest.iter().filter_map(|character| opt_string(Some(character))).collect();
                features.register_completion_item_provider(extension, selector, Rc::new(CompletionProvider::new(provider)), trigger_characters)
            })
        }),
        simple(runner, registrar, "registerHoverProvider", |features, extension, selector, provider| {
            features.register_hover_provider(extension, selector, Rc::new(HoverProvider(provider)))
        }),
        location("registerDefinitionProvider", "provideDefinition", ExtHostLanguageFeatures::register_definition_provider),
        location("registerDeclarationProvider", "provideDeclaration", ExtHostLanguageFeatures::register_declaration_provider),
        location("registerImplementationProvider", "provideImplementation", ExtHostLanguageFeatures::register_implementation_provider),
        location("registerTypeDefinitionProvider", "provideTypeDefinition", ExtHostLanguageFeatures::register_type_definition_provider),
        member(runner, registrar, "registerCodeActionsProvider", 3, |registrar, args| {
            let metadata = match args.get(2) {
                Some(metadata) if !is_nullish(Some(metadata)) => Some(code_action_metadata_from_js(&exported(&registrar.runner, metadata)).map_err(|error| error.message)?),
                _ => None,
            };
            registrar.register(args, |features, extension, selector, provider, _| {
                features.register_code_action_provider(extension, selector, Rc::new(CodeActionProvider::new(provider)), metadata.as_ref())
            })
        }),
        simple(runner, registrar, "registerDocumentFormattingEditProvider", |features, extension, selector, provider| {
            features.register_document_formatting_edit_provider(extension, selector, Rc::new(DocumentFormattingProvider(provider)))
        }),
        simple(runner, registrar, "registerDocumentRangeFormattingEditProvider", |features, extension, selector, provider| {
            features.register_document_range_formatting_edit_provider(extension, selector, Rc::new(DocumentRangeFormattingProvider(provider)))
        }),
        member(runner, registrar, "registerDocumentSymbolProvider", 3, |registrar, args| {
            let metadata = match args.get(2) {
                Some(metadata) if !is_nullish(Some(metadata)) => Some(DocumentSymbolProviderMetadata { label: opt_string(field(&exported(&registrar.runner, metadata), "label")) }),
                _ => None,
            };
            registrar.register(args, |features, extension, selector, provider, _| {
                features.register_document_symbol_provider(extension, selector, Rc::new(DocumentSymbolProvider(provider)), metadata.as_ref())
            })
        }),
    ];
    members.extend(language_providers::members(runner, registrar));
    members
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
        let (status_languages, status_items, status_extension) = (languages.clone(), state.status_items.clone(), host_extension());
        members.push(function(
            "createLanguageStatusItem",
            2,
            Rc::new(move |args| {
                let id = string_arg(args, 0, "createLanguageStatusItem: o id")?;
                status_items.create(&status_languages, &status_extension, &id, args.get(1).ok_or("createLanguageStatusItem: falta o seletor")?)
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
            js: JsObjects::new(runner),
            edits: services.workspace_edits.clone(),
        });
        members.extend(provider_members(runner, &registrar));
    }
    JsData::Object(members)
}

/// As classes deste módulo: as de valor (`language_types` e `language_provider_types`) e o protótipo de `DiagnosticCollection`.
pub fn classes(runner: &NodeRunner, state: &LanguagesState) -> Vec<(String, JsData)> {
    define_collection_class(runner, &state.collections);
    language_status::define_class(runner, &state.status_items);
    let mut classes = language_types::classes(runner);
    classes.extend(language_provider_types::classes(runner, &state.builders));
    classes
}
