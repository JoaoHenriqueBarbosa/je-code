//! Porte parcial de `src/vs/workbench/api/common/extHost.protocol.ts` e do que ele pede do RPC
//! (`proxyIdentifier.ts`), só o que `extHostBrowsers.ts` e `extHostCommands.ts` usam.
//!
//! Divergências:
//! - É um porte parcial: do arquivo original entram `BrowserTabDto`, `MainThreadBrowsersShape`,
//!   `ExtHostBrowsersShape`, `IMainContext`, `MainThreadCommandsShape`, `ExtHostCommandsShape`,
//!   `ICommandDto`, `ICommandMetadataDto`, `MainThreadTelemetryShape` (com o `IExtHostTelemetry` mínimo de
//!   `extHostTelemetry.ts`), `SerializableObjectWithBuffers` (de `proxyIdentifier.ts`) e os identificadores
//!   `MainContext.MainThreadBrowsers`, `MainThreadCommands`, `MainThreadTelemetry` e
//!   `ExtHostContext.ExtHostCommands`. O resto dos DTOs, das formas e dos identificadores entra junto de
//!   cada arquivo `extHost*.ts` que os pedir.
//! - `$executeCommand` devolve `unknown | undefined`: é [`Dynamic`], com `Dynamic::Undefined` no segundo
//!   caso; `args: unknown[] | SerializableObjectWithBuffers<unknown[]>` é [`CommandArgs`]. O `{ $ident?: string } &
//!   languages.Command` de `ICommandDto` é um struct com o `$ident` no campo `ident`.
//! - O prefixo `$` dos métodos de RPC vira `rpc_` (`$openBrowserTab` é `rpc_open_browser_tab`), porque
//!   `onDidOpenBrowserTab` (o evento da API) e `$onDidOpenBrowserTab` (o retorno da thread principal) ficariam
//!   com o mesmo nome em snake case.
//! - `IMainContext` é um trait sem genéricos (objeto de trait) com `get_proxy_erased`/`set_erased`; o
//!   `getProxy<T>(identifier)` e o `set<T, R extends T>(identifier, value)` genéricos do original são os
//!   métodos [`dyn IMainContext::get_proxy`](IMainContext) e [`dyn IMainContext::set`](IMainContext), que
//!   fazem o `downcast` do proxy guardado pelo `sid` do identificador. `Proxied<T>` é `Rc<T>` (a thread
//!   principal e a de extensões dividem o processo aqui, e o proxy de teste responde direto). `drain`,
//!   `assertRegistered`, `dispose` e a conexão remota do `IRPCProtocol` ficam para o porte do protocolo.
//! - `MainThreadBrowsersShape extends IDisposable` é o supertrait [`IDisposable`].
//! - `CDPRequest`, `CDPResponse` e `CDPEvent` (`platform/browserView/common/cdp/types.ts`) são definidos
//!   aqui até esse arquivo ser portado. Os `unknown` e `object` do protocolo são [`Dynamic`]. O tipo
//!   união `CDPResponse | CDPEvent` é [`CdpMessage`].
//! - `EditorGroupColumn` e `IEditorOptions` vêm de [`super::ext_host_type_converters`] e de
//!   [`je_platform::opener::editor_types::EditorOptions`].
//! - Regiões `Configuration`, `Workspace` e `MessageService` (de `extHostConfiguration.ts`, `extHostWorkspace.ts`):
//!   os `UriComponents` são [`URI`]; `IConfigurationInitData` é o `IConfigurationData` mais `configurationScopes`
//!   (struct com `data`); `IRawFileMatch2`, `ITextSearchComplete` e `IRelativePatternDto` (que moram em `search.ts`
//!   e em `extHost.protocol.ts`) ficam aqui, junto de quem os usa; `ITextSearchComplete.message` é sempre lista; o
//!   `$requestResourceTrust(ResourceTrustRequestOptionsDto)` usa o [`ResourceTrustRequestOptions`] da plataforma.
//! - Região `TreeViews` (de `extHostTreeViews.ts`): `ITreeItem`, `IRevealOptions` e `IViewBadge` são de
//!   `common::views`. O objeto `{ [treeItemHandle]: ITreeItem }` de `$refresh` é uma lista de pares na ordem de
//!   inserção ([`ItemsToRefresh`]); o `(number | ITreeItem)[]` de `$getChildren` é o struct [`TreeChildrenBatch`]
//!   (índice mais filhos); `DataTransferDTO.items` é uma lista de pares; os `UriComponents` são [`URI`].

use std::any::Any;
use std::marker::PhantomData;
use std::rc::Rc;

use je_base::cancellation::CancellationToken;
use je_base::buffer::VSBuffer;
use je_base::dynamic::Dynamic;
use je_base::errors::Error;
use je_base::lifecycle::IDisposable;
use je_base::promise::Promise;
use je_base::severity::Severity;
use je_base::uri::URI;
use je_editor::languages::StringOrMarkdown;
use je_extensions::manifest::ExtensionIdentifier;
use je_platform::commands::commands::CommandDescription;
use je_platform::configuration::registry::ConfigurationScope;
use je_platform::configuration::{ConfigurationChange, ConfigurationData, ConfigurationOverrides, ConfigurationTarget};
use je_platform::opener::editor_types::EditorOptions;
use je_platform::request::request::{AuthInfo, Credentials};
use je_platform::workspace_trust::{ResourceTrustRequestOptions, WorkspaceTrustRequestOptions};

use crate::common::views::{IRevealOptions, ITreeItem, IViewBadge};
use crate::services::search::query_builder::{IFileQueryBuilderOptions, ITextQueryBuilderOptions};
use crate::services::search::search::{ITextSearchResult, PatternInfo};
use crate::services::search::search_ext_types::TextSearchCompleteMessage2;

pub use super::ext_host_type_converters::EditorGroupColumn;

// #region Mensagens CDP

/// `CDPRequest`.
#[derive(Clone)]
pub struct CdpRequest {
    pub id: f64,
    pub method: String,
    pub params: Option<Dynamic>,
    pub session_id: Option<String>,
}

/// O `{ code: number; message: string }` de `CDPResponse.error`.
#[derive(Clone, Debug, PartialEq)]
pub struct CdpResponseError {
    pub code: f64,
    pub message: String,
}

/// `CDPResponse`.
#[derive(Clone)]
pub struct CdpResponse {
    pub id: f64,
    pub result: Option<Dynamic>,
    pub error: Option<CdpResponseError>,
    pub session_id: Option<String>,
}

/// `CDPEvent`.
#[derive(Clone)]
pub struct CdpEvent {
    pub method: String,
    pub params: Dynamic,
    pub session_id: Option<String>,
}

/// `CDPResponse | CDPEvent`.
#[derive(Clone)]
pub enum CdpMessage {
    Response(CdpResponse),
    Event(CdpEvent),
}

/// O objeto JavaScript só com as propriedades definidas (uma propriedade `undefined` não aparece).
fn defined_object(entries: impl IntoIterator<Item = (&'static str, Option<Dynamic>)>) -> Dynamic {
    Dynamic::object(entries.into_iter().filter_map(|(key, value)| value.map(|value| (key, value))))
}

impl CdpMessage {
    /// A mensagem como o `unknown` que a extensão recebe em `onDidReceiveMessage`.
    pub fn to_dynamic(&self) -> Dynamic {
        let session_id = |session_id: &Option<String>| session_id.as_deref().map(Dynamic::string);
        match self {
            Self::Response(response) => defined_object([
                ("id", Some(Dynamic::Number(response.id))),
                ("result", response.result.clone()),
                (
                    "error",
                    response.error.as_ref().map(|error| Dynamic::object([("code", Dynamic::Number(error.code)), ("message", Dynamic::string(&error.message))])),
                ),
                ("sessionId", session_id(&response.session_id)),
            ]),
            Self::Event(event) => defined_object([
                ("method", Some(Dynamic::string(&event.method))),
                ("params", Some(event.params.clone())),
                ("sessionId", session_id(&event.session_id)),
            ]),
        }
    }
}

// #endregion

// #region Proxies

/// `ProxyIdentifier<T>`: o identificador de um proxy de RPC, tipado pela forma `T`.
pub struct ProxyIdentifier<T: ?Sized + 'static> {
    pub sid: &'static str,
    marker: PhantomData<fn() -> Rc<T>>,
}

impl<T: ?Sized + 'static> ProxyIdentifier<T> {
    /// `createProxyIdentifier<T>(identifier)`.
    pub const fn new(sid: &'static str) -> Self {
        Self { sid, marker: PhantomData }
    }
}

/// `IMainContext`: de onde o lado das extensões pega o proxy de cada serviço da thread principal.
pub trait IMainContext {
    /// O proxy registrado em `sid`: um `Rc<T>` guardado como `Rc<dyn Any>` (ver [`dyn IMainContext::get_proxy`](IMainContext)).
    fn get_proxy_erased(&self, sid: &'static str) -> Rc<dyn Any>;

    /// Registra o objeto local `value` (um `Rc<T>` guardado como `Rc<dyn Any>`) em `sid`.
    fn set_erased(&self, sid: &'static str, value: Rc<dyn Any>);
}

impl<'a> dyn IMainContext + 'a {
    /// `getProxy<T>(identifier)`. Levanta um erro se o proxy guardado não tem a forma `T`.
    pub fn get_proxy<T: ?Sized + 'static>(&self, identifier: &ProxyIdentifier<T>) -> Rc<T> {
        self.get_proxy_erased(identifier.sid)
            .downcast_ref::<Rc<T>>()
            .cloned()
            .unwrap_or_else(|| panic!("o proxy '{}' não tem a forma pedida", identifier.sid))
    }

    /// `set<T, R extends T>(identifier, value)`: registra `value` e o devolve.
    pub fn set<T: ?Sized + 'static>(&self, identifier: &ProxyIdentifier<T>, value: Rc<T>) -> Rc<T> {
        self.set_erased(identifier.sid, Rc::new(value.clone()));
        value
    }
}

// #endregion

// #region Browsers

/// `BrowserTabDto`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BrowserTabDto {
    pub id: String,
    pub url: String,
    pub title: String,
    pub favicon: Option<String>,
}

/// `MainThreadBrowsersShape`.
pub trait MainThreadBrowsersShape: IDisposable {
    fn rpc_open_browser_tab(&self, url: &str, view_column: Option<EditorGroupColumn>, options: Option<EditorOptions>) -> Promise<BrowserTabDto>;
    fn rpc_close_browser_tab(&self, browser_id: &str) -> Promise<()>;
    fn rpc_start_cdp_session(&self, session_id: &str, browser_id: &str) -> Promise<()>;
    fn rpc_close_cdp_session(&self, session_id: &str) -> Promise<()>;
    fn rpc_send_cdp_message(&self, session_id: &str, message: CdpRequest) -> Promise<()>;
}

/// `ExtHostBrowsersShape`.
pub trait ExtHostBrowsersShape {
    fn rpc_on_did_open_browser_tab(&self, browser: BrowserTabDto);
    fn rpc_on_did_close_browser_tab(&self, browser_id: &str);
    fn rpc_on_did_change_active_browser_tab(&self, browser_id: Option<&str>);
    fn rpc_on_did_change_browser_tab_state(&self, browser: BrowserTabDto);
    fn rpc_on_cdp_session_message(&self, session_id: &str, message: CdpMessage);
    fn rpc_on_cdp_session_closed(&self, session_id: &str);
}

// #endregion

// #region Commands

/// `SerializableObjectWithBuffers<T>`: marca o objeto como contendo buffers, que o transporte serializa de
/// forma mais eficiente.
#[derive(Clone, Debug)]
pub struct SerializableObjectWithBuffers<T> {
    pub value: T,
}

impl<T> SerializableObjectWithBuffers<T> {
    pub fn new(value: T) -> Self {
        Self { value }
    }
}

/// `ICommandDto`: `{ $ident?: string } & languages.Command`. O `$ident` é o campo `ident`.
#[derive(Clone, Debug)]
pub struct ICommandDto {
    pub ident: Option<String>,
    pub id: String,
    pub title: String,
    pub tooltip: Option<String>,
    pub arguments: Option<Vec<Dynamic>>,
}

/// `unknown[] | SerializableObjectWithBuffers<unknown[]>`: os argumentos de `$executeCommand`.
#[derive(Clone, Debug)]
pub enum CommandArgs {
    Plain(Vec<Dynamic>),
    WithBuffers(SerializableObjectWithBuffers<Vec<Dynamic>>),
}

/// `MainThreadCommandsShape`. O retorno `unknown | undefined` de `$executeCommand` é um [`Dynamic`]
/// (`Dynamic::Undefined` quando não há valor).
pub trait MainThreadCommandsShape: IDisposable {
    fn rpc_register_command(&self, id: &str);
    fn rpc_unregister_command(&self, id: &str);
    fn rpc_fire_command_activation_event(&self, id: &str);
    fn rpc_execute_command(&self, id: &str, args: CommandArgs, retry: bool) -> Promise<Dynamic>;
    fn rpc_get_commands(&self) -> Promise<Vec<String>>;
}

/// Um item de `ICommandMetadataDto.args`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandMetadataArgDto {
    pub name: String,
    pub is_optional: Option<bool>,
    pub description: Option<String>,
}

/// `ICommandMetadataDto`. O `description` é `ILocalizedString | string`, ou seja, um `CommandDescription`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ICommandMetadataDto {
    pub description: CommandDescription,
    pub args: Option<Vec<CommandMetadataArgDto>>,
    pub returns: Option<String>,
}

/// `ExtHostCommandsShape`: o retorno de `$getContributedCommandMetadata` é `{ [id: string]: string | ICommandMetadataDto }`,
/// aqui uma lista de pares na ordem de inserção (o `string` do original nunca é produzido: o host sempre
/// guarda um `ICommandMetadata`).
pub trait ExtHostCommandsShape {
    fn rpc_execute_contributed_command(&self, id: &str, args: Vec<Dynamic>) -> Promise<Dynamic>;
    fn rpc_get_contributed_command_metadata(&self) -> Promise<Vec<(String, ICommandMetadataDto)>>;
}

// #endregion

// #region Documentos e editores de texto

pub use self::documents_and_editors_region::*;

/// Região de `extHost.protocol.ts` que `extHostTextEditor.ts`, `extHostDocumentsAndEditors.ts` e
/// `extHostDocuments.ts` usam: `MainThreadDocumentsShape`, `MainThreadTextEditorsShape`,
/// `ExtHostDocumentsShape`, `ExtHostDocumentsAndEditorsShape`, os DTOs deles e os identificadores
/// `MainContext.MainThreadDocuments`, `MainThreadTextEditors`, `ExtHostContext.ExtHostDocuments` e
/// `ExtHostDocumentsAndEditors`. Fica num módulo próprio (reexportado aqui) para as importações não
/// colidirem com as das outras regiões do arquivo.
///
/// Divergências:
/// - `UriComponents` é a [`URI`] (o `URI.revive` do original já está feito), e `IRange`, `ISelection` e
///   `ISingleEditOperation` são os tipos do editor ([`EditorRange`], [`EditorSelection`],
///   [`SingleEditOperation`]). O `EOL: string` de `IModelAddedData` e o `eol: string` do evento serializado são o
///   [`Eol`] do modelo (só há `'\n'` e `'\r\n'`), e o `versionId` é `i64`, como no espelho do modelo.
/// - `MainThreadDocumentsShape extends IDisposable, IExtHostDocumentSaveDelegate`: o `$trySaveDocument` é o
///   `rpc_try_save_document` do supertrait [`IExtHostDocumentSaveDelegate`].
/// - `newActiveEditor?: string | null` é `Option<Option<String>>` (`None` é `undefined`, `Some(None)` é `null`).
/// - `$getDiffInformation` devolve `IChange[]`: o [`LineChange`] sem `charChanges`. `$trySetDecorationsFast`
///   recebe os `number[]` como `Vec<f64>`. `$tryShowTextDocument`, `$tryHideEditor`, `$tryShowEditor` e os demais
///   métodos de `MainThreadTextEditorsShape` seguem a assinatura do original com os tipos acima.
/// - `ISnippetOptions` (o `opts` de `$tryInsertSnippet`, que o original anota como `IUndoStopOptions`, mas
///   recebe o `keepWhitespace` também) é [`SnippetOptions`].
/// - `TextEditorRevealType` do protocolo (valores `0` a `3`) é o enum de `ext_host_types`, de valores iguais.
/// - `IDecorationOptions`, `IDecorationInstanceRenderOptions` e `IThemeDecorationInstanceRenderOptions` (de
///   `editorCommon.ts`) são [`DecorationOptionsDto`], [`DecorationInstanceRenderOptionsDto`] e
///   [`ThemeDecorationInstanceRenderOptionsDto`]; `IDecorationRenderOptions` é o do editor.
/// - `IMarkdownString | IMarkdownString[]` é [`MarkdownStringOrArray`].
mod documents_and_editors_region {
    use je_base::dynamic::Dynamic;
    use je_base::html_content::IMarkdownString;
    use je_base::js_string::JsString;
    use je_base::lifecycle::IDisposable;
    use je_base::promise::Promise;
    use je_base::uri::URI;
    use je_editor::config::editor_options::cursor_style::TextEditorCursorStyle;
    use je_editor::config::editor_options::line_numbers::RenderLineNumbersType;
    use je_editor::diff::legacy_lines_diff_computer::LineChange;
    use je_editor::edit_operation::SingleEditOperation;
    use je_editor::model::{EndOfLineSequence, Eol};
    use je_editor::range::Range as EditorRange;
    use je_editor::selection::Selection as EditorSelection;
    use je_editor::services::decoration_render_options::{ContentDecorationRenderOptions, DecorationRenderOptions};
    use je_editor::text_model_events::ModelContentChange;
    use je_extensions::manifest::ExtensionIdentifier;

    use super::{EditorGroupColumn, ExtHostContext, MainContext, ProxyIdentifier};
    use crate::api::common::ext_host_document_data::IExtHostDocumentSaveDelegate;
    use crate::api::common::ext_host_types::TextEditorRevealType;

    // ---- configuração do editor

    /// `tabSize?: number | 'auto'`.
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub enum TabSizeValue {
        Size(f64),
        Auto,
    }

    /// `indentSize?: number | 'tabSize'` (e o `originalIndentSize: number | 'tabSize'`).
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub enum IndentSizeValue {
        Size(f64),
        TabSize,
    }

    /// `insertSpaces?: boolean | 'auto'`.
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub enum InsertSpacesValue {
        Bool(bool),
        Auto,
    }

    /// `ITextEditorConfigurationUpdate`: cada campo ausente é `undefined`.
    #[derive(Clone, Copy, Debug, Default, PartialEq)]
    pub struct TextEditorConfigurationUpdate {
        pub tab_size: Option<TabSizeValue>,
        pub indent_size: Option<IndentSizeValue>,
        pub insert_spaces: Option<InsertSpacesValue>,
        pub cursor_style: Option<TextEditorCursorStyle>,
        pub line_numbers: Option<RenderLineNumbersType>,
    }

    /// `IResolvedTextEditorConfiguration`.
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct ResolvedTextEditorConfiguration {
        pub tab_size: f64,
        pub indent_size: f64,
        pub original_indent_size: IndentSizeValue,
        pub insert_spaces: bool,
        pub cursor_style: TextEditorCursorStyle,
        pub line_numbers: RenderLineNumbersType,
    }

    /// `IUndoStopOptions`. O valor padrão do original (`{ undoStopBefore: true, undoStopAfter: true }`) é o
    /// [`Default`].
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct UndoStopOptions {
        pub undo_stop_before: bool,
        pub undo_stop_after: bool,
    }

    impl Default for UndoStopOptions {
        fn default() -> Self {
            Self { undo_stop_before: true, undo_stop_after: true }
        }
    }

    /// `IApplyEditsOptions`.
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct ApplyEditsOptions {
        pub set_end_of_line: Option<EndOfLineSequence>,
        pub undo_stop_before: bool,
        pub undo_stop_after: bool,
    }

    /// `ISnippetOptions`. O valor padrão do original é `{ undoStopBefore: true, undoStopAfter: true }`.
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct SnippetOptions {
        pub undo_stop_before: bool,
        pub undo_stop_after: bool,
        pub keep_whitespace: Option<bool>,
    }

    impl Default for SnippetOptions {
        fn default() -> Self {
            Self { undo_stop_before: true, undo_stop_after: true, keep_whitespace: None }
        }
    }

    /// `ITextDocumentShowOptions`.
    #[derive(Clone, Copy, Debug, Default, PartialEq)]
    pub struct TextDocumentShowOptions {
        pub position: Option<EditorGroupColumn>,
        pub preserve_focus: Option<bool>,
        pub pinned: Option<bool>,
        pub selection: Option<EditorRange>,
    }

    // ---- decorações

    /// `IMarkdownString | IMarkdownString[]`.
    #[derive(Clone, Debug)]
    pub enum MarkdownStringOrArray {
        One(IMarkdownString),
        Many(Vec<IMarkdownString>),
    }

    /// `IThemeDecorationInstanceRenderOptions`.
    #[derive(Clone, Debug, Default)]
    pub struct ThemeDecorationInstanceRenderOptionsDto {
        pub before: Option<ContentDecorationRenderOptions>,
        pub after: Option<ContentDecorationRenderOptions>,
    }

    /// `IDecorationInstanceRenderOptions`: as opções de tema (`base`) mais as variantes `light` e `dark`.
    #[derive(Clone, Debug, Default)]
    pub struct DecorationInstanceRenderOptionsDto {
        pub base: ThemeDecorationInstanceRenderOptionsDto,
        pub light: Option<ThemeDecorationInstanceRenderOptionsDto>,
        pub dark: Option<ThemeDecorationInstanceRenderOptionsDto>,
    }

    /// `IDecorationOptions`.
    #[derive(Clone, Debug)]
    pub struct DecorationOptionsDto {
        pub range: EditorRange,
        pub hover_message: Option<MarkdownStringOrArray>,
        pub render_options: Option<DecorationInstanceRenderOptionsDto>,
    }

    // ---- documentos

    /// O `options` de `$tryCreateDocument`: `{ language?: string; content?: string; encoding?: string }`.
    #[derive(Clone, Debug, Default, PartialEq, Eq)]
    pub struct CreateDocumentOptions {
        pub language: Option<String>,
        pub content: Option<String>,
        pub encoding: Option<String>,
    }

    /// O `options` de `$tryOpenDocument`: `{ encoding?: string }`.
    #[derive(Clone, Debug, Default, PartialEq, Eq)]
    pub struct OpenDocumentOptions {
        pub encoding: Option<String>,
    }

    /// `MainThreadDocumentsShape`.
    pub trait MainThreadDocumentsShape: IDisposable + IExtHostDocumentSaveDelegate {
        fn rpc_try_create_document(&self, options: Option<CreateDocumentOptions>) -> Promise<URI>;
        fn rpc_try_open_document(&self, uri: &URI, options: Option<OpenDocumentOptions>) -> Promise<URI>;
    }

    /// `MainThreadTextEditorsShape`.
    pub trait MainThreadTextEditorsShape: IDisposable {
        fn rpc_try_show_text_document(&self, resource: &URI, options: TextDocumentShowOptions) -> Promise<Option<String>>;
        fn rpc_register_text_editor_decoration_type(&self, extension_id: &ExtensionIdentifier, key: &str, options: DecorationRenderOptions);
        fn rpc_remove_text_editor_decoration_type(&self, key: &str);
        fn rpc_try_show_editor(&self, id: &str, position: EditorGroupColumn) -> Promise<()>;
        fn rpc_try_hide_editor(&self, id: &str) -> Promise<()>;
        fn rpc_try_set_options(&self, id: &str, options: TextEditorConfigurationUpdate) -> Promise<()>;
        fn rpc_try_set_decorations(&self, id: &str, key: &str, ranges: Vec<DecorationOptionsDto>) -> Promise<()>;
        fn rpc_try_set_decorations_fast(&self, id: &str, key: &str, ranges: Vec<f64>) -> Promise<()>;
        fn rpc_try_reveal_range(&self, id: &str, range: EditorRange, reveal_type: TextEditorRevealType) -> Promise<()>;
        fn rpc_try_set_selections(&self, id: &str, selections: Vec<EditorSelection>) -> Promise<()>;
        fn rpc_try_apply_edits(&self, id: &str, model_version_id: f64, edits: Vec<SingleEditOperation>, opts: ApplyEditsOptions) -> Promise<bool>;
        fn rpc_try_insert_snippet(&self, id: &str, model_version_id: f64, template: &str, selections: &[EditorRange], opts: SnippetOptions) -> Promise<bool>;
        fn rpc_get_diff_information(&self, id: &str) -> Promise<Vec<LineChange>>;
    }

    /// `IModelAddedData`.
    #[derive(Clone, Debug)]
    pub struct ModelAddedData {
        pub uri: URI,
        pub version_id: i64,
        pub lines: Vec<JsString>,
        pub eol: Eol,
        pub language_id: String,
        pub is_dirty: bool,
        pub encoding: String,
    }

    /// `ISerializedModelContentChangedEvent`. O `detailedReason: Record<string, unknown> | undefined` é um
    /// [`Dynamic`] (um objeto).
    #[derive(Clone, Debug)]
    pub struct SerializedModelContentChangedEvent {
        pub changes: Vec<ModelContentChange>,
        pub eol: Eol,
        pub version_id: i64,
        pub is_undoing: bool,
        pub is_redoing: bool,
        pub is_flush: bool,
        pub is_eol_change: bool,
        pub detailed_reason: Option<Dynamic>,
    }

    /// `ExtHostDocumentsShape`.
    pub trait ExtHostDocumentsShape {
        fn rpc_accept_model_language_changed(&self, uri: &URI, new_language_id: &str);
        fn rpc_accept_model_saved(&self, uri: &URI);
        fn rpc_accept_dirty_state_changed(&self, uri: &URI, is_dirty: bool);
        fn rpc_accept_encoding_changed(&self, uri: &URI, encoding: &str);
        fn rpc_accept_model_changed(&self, uri: &URI, e: SerializedModelContentChangedEvent, is_dirty: bool);
    }

    // ---- documentos e editores

    /// `ITextEditorAddData`. O `editorPosition: EditorGroupColumn | undefined` é um `Option`.
    #[derive(Clone, Debug)]
    pub struct TextEditorAddData {
        pub id: String,
        pub document_uri: URI,
        pub options: ResolvedTextEditorConfiguration,
        pub selections: Vec<EditorSelection>,
        pub visible_ranges: Vec<EditorRange>,
        pub editor_position: Option<EditorGroupColumn>,
    }

    /// `IDocumentsAndEditorsDelta`: cada campo ausente é `undefined`.
    #[derive(Clone, Debug, Default)]
    pub struct DocumentsAndEditorsDelta {
        pub removed_documents: Option<Vec<URI>>,
        pub added_documents: Option<Vec<ModelAddedData>>,
        pub removed_editors: Option<Vec<String>>,
        pub added_editors: Option<Vec<TextEditorAddData>>,
        pub new_active_editor: Option<Option<String>>,
    }

    /// `ExtHostDocumentsAndEditorsShape`.
    pub trait ExtHostDocumentsAndEditorsShape {
        fn rpc_accept_documents_and_editors_delta(&self, delta: DocumentsAndEditorsDelta);
    }

    // ---- identificadores

    impl MainContext {
        pub const MAIN_THREAD_DOCUMENTS: ProxyIdentifier<dyn MainThreadDocumentsShape> = ProxyIdentifier::new("MainThreadDocuments");
        pub const MAIN_THREAD_TEXT_EDITORS: ProxyIdentifier<dyn MainThreadTextEditorsShape> = ProxyIdentifier::new("MainThreadTextEditors");
    }

    impl ExtHostContext {
        pub const EXT_HOST_DOCUMENTS_AND_EDITORS: ProxyIdentifier<dyn ExtHostDocumentsAndEditorsShape> = ProxyIdentifier::new("ExtHostDocumentsAndEditors");
        pub const EXT_HOST_DOCUMENTS: ProxyIdentifier<dyn ExtHostDocumentsShape> = ProxyIdentifier::new("ExtHostDocuments");
    }
}

// #endregion

// #region BulkEdits, DocumentSaveParticipant e DocumentContentProviders

pub use self::bulk_edits_region::*;

/// Região de `extHost.protocol.ts` que `extHostDocumentSaveParticipant.ts` e `extHostDocumentContentProviders.ts`
/// usam: `IWorkspaceEditDto` (com os DTOs de arquivo, texto e célula), `MainThreadBulkEditsShape`,
/// `ExtHostDocumentSaveParticipantShape`, `MainThreadDocumentContentProvidersShape`,
/// `ExtHostDocumentContentProvidersShape` e os identificadores `MainContext.MainThreadBulkEdits`,
/// `MainThreadDocumentContentProviders`, `ExtHostContext.ExtHostDocumentContentProviders` e
/// `ExtHostDocumentSaveParticipant`. Fica num módulo próprio (reexportado aqui) para as importações não colidirem
/// com as das outras regiões do arquivo.
///
/// Divergências:
/// - `UriComponents` é a [`URI`], e o `string` de `$onVirtualDocumentChange` e o `string | null | undefined` de
///   `$provideTextDocumentContent` são [`JsString`] (o texto de um provedor é UTF-16 e pode carregar substitutos
///   isolados); `null` e `undefined` são ambos `None`. O `handle` é `u64`, como nas outras regiões.
/// - `IWorkspaceTextEditDto` (`Dto<languages.IWorkspaceTextEdit>`) é o próprio `IWorkspaceTextEdit` do editor, que
///   já guarda o `resource` como [`URI`]. `IWorkspaceFileEditDto` tem as opções próprias do protocolo: o `contents`
///   é `{ type: 'base64'; value: string } | { type: 'dataTransferItem'; id: string }`
///   ([`WorkspaceFileEditContentsDto`]) em vez da promessa de bytes do `languages.WorkspaceFileEditOptions`.
/// - `IWorkspaceCellEditDto` é o `IWorkspaceNotebookCellEdit` de `notebook_common`, cujo `Replace` carrega os
///   `ICellDto2` no lugar de `NotebookCellDataDto` (o mesmo conteúdo, com o estado de colapso a mais).
/// - `$tryApplyWorkspaceEdit(dto, undoRedoGroupId?, isRefactoring?)` recebe os dois opcionais como `Option`.
/// - `SaveReason` é o de `common::editor`.
mod bulk_edits_region {
    use je_base::js_string::JsString;
    use je_base::lifecycle::IDisposable;
    use je_base::promise::Promise;
    use je_base::uri::URI;
    use je_editor::code_action::languages::{IWorkspaceTextEdit, WorkspaceEditMetadata};

    use super::{ExtHostContext, MainContext, ProxyIdentifier, SerializableObjectWithBuffers};
    use crate::common::editor::SaveReason;
    use crate::contrib::notebook::common::notebook_common::IWorkspaceNotebookCellEdit;

    // ---- edições do workspace

    /// `IWorkspaceTextEditDto`: `Dto<languages.IWorkspaceTextEdit>`.
    pub type IWorkspaceTextEditDto = IWorkspaceTextEdit;

    /// O `contents` de `IWorkspaceFileEditDto.options`: `{ type: 'base64'; value: string } | { type: 'dataTransferItem'; id: string }`.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub enum WorkspaceFileEditContentsDto {
        Base64(String),
        DataTransferItem(String),
    }

    /// `Omit<languages.WorkspaceFileEditOptions, 'contents'> & { contents?: ... }`.
    #[derive(Clone, Debug, Default, PartialEq)]
    pub struct WorkspaceFileEditOptionsDto {
        pub overwrite: Option<bool>,
        pub ignore_if_not_exists: Option<bool>,
        pub ignore_if_exists: Option<bool>,
        pub recursive: Option<bool>,
        pub copy: Option<bool>,
        pub folder: Option<bool>,
        pub skip_trash_bin: Option<bool>,
        pub max_size: Option<f64>,
        pub contents: Option<WorkspaceFileEditContentsDto>,
    }

    /// `IWorkspaceFileEditDto`.
    #[derive(Clone, Default)]
    pub struct IWorkspaceFileEditDto {
        pub old_resource: Option<URI>,
        pub new_resource: Option<URI>,
        pub options: Option<WorkspaceFileEditOptionsDto>,
        pub metadata: Option<WorkspaceEditMetadata>,
    }

    /// Um item de `IWorkspaceEditDto.edits`: `IWorkspaceFileEditDto | IWorkspaceTextEditDto | IWorkspaceCellEditDto`.
    #[derive(Clone)]
    pub enum WorkspaceEditEntryDto {
        File(IWorkspaceFileEditDto),
        Text(IWorkspaceTextEditDto),
        Cell(IWorkspaceNotebookCellEdit),
    }

    /// `IWorkspaceEditDto`.
    #[derive(Clone, Default)]
    pub struct IWorkspaceEditDto {
        pub edits: Vec<WorkspaceEditEntryDto>,
    }

    // ---- formas

    /// `MainThreadBulkEditsShape`.
    pub trait MainThreadBulkEditsShape: IDisposable {
        fn rpc_try_apply_workspace_edit(
            &self,
            workspace_edit_dto: SerializableObjectWithBuffers<IWorkspaceEditDto>,
            undo_redo_group_id: Option<f64>,
            is_refactoring: Option<bool>,
        ) -> Promise<bool>;
    }

    /// `ExtHostDocumentSaveParticipantShape`.
    pub trait ExtHostDocumentSaveParticipantShape {
        fn rpc_participate_in_save(&self, resource: &URI, reason: SaveReason) -> Promise<Vec<bool>>;
    }

    /// `MainThreadDocumentContentProvidersShape`.
    pub trait MainThreadDocumentContentProvidersShape: IDisposable {
        fn rpc_register_text_content_provider(&self, handle: u64, scheme: &str);
        fn rpc_unregister_text_content_provider(&self, handle: u64);
        fn rpc_on_virtual_document_change(&self, uri: &URI, value: &JsString) -> Promise<()>;
    }

    /// `ExtHostDocumentContentProvidersShape`.
    pub trait ExtHostDocumentContentProvidersShape {
        fn rpc_provide_text_document_content(&self, handle: u64, uri: &URI) -> Promise<Option<JsString>>;
    }

    // ---- identificadores

    impl MainContext {
        pub const MAIN_THREAD_BULK_EDITS: ProxyIdentifier<dyn MainThreadBulkEditsShape> = ProxyIdentifier::new("MainThreadBulkEdits");
        pub const MAIN_THREAD_DOCUMENT_CONTENT_PROVIDERS: ProxyIdentifier<dyn MainThreadDocumentContentProvidersShape> =
            ProxyIdentifier::new("MainThreadDocumentContentProviders");
    }

    impl ExtHostContext {
        pub const EXT_HOST_DOCUMENT_CONTENT_PROVIDERS: ProxyIdentifier<dyn ExtHostDocumentContentProvidersShape> = ProxyIdentifier::new("ExtHostDocumentContentProviders");
        pub const EXT_HOST_DOCUMENT_SAVE_PARTICIPANT: ProxyIdentifier<dyn ExtHostDocumentSaveParticipantShape> = ProxyIdentifier::new("ExtHostDocumentSaveParticipant");
    }
}

// #endregion

// #region Telemetry

/// `MainThreadTelemetryShape`. O `$publicLog2<E, T>` genérico do original, que só checa o tipo dos dados em
/// tempo de compilação, é `rpc_public_log2` com os dados como [`Dynamic`].
pub trait MainThreadTelemetryShape: IDisposable {
    fn rpc_public_log(&self, event_name: &str, data: Option<Dynamic>);
    fn rpc_public_log2(&self, event_name: &str, data: Option<Dynamic>);
}

/// `IExtHostTelemetry`, só o que o host de comandos usa: `onExtensionError` devolve `true` se o erro foi
/// repassado ao registrador de telemetria da extensão. O resto de `ExtHostTelemetryShape` entra junto do
/// porte de `extHostTelemetry.ts`.
pub trait IExtHostTelemetry {
    fn on_extension_error(&self, extension: &ExtensionIdentifier, error: &Error) -> bool;
}

// #endregion

// #region Configuration

/// `IConfigurationInitData`: o `IConfigurationData` (em `data`) mais `configurationScopes`, os escopos
/// declarados por chave (`undefined` quando a chave não declara um).
#[derive(Clone)]
pub struct IConfigurationInitData {
    pub data: ConfigurationData,
    pub configuration_scopes: Vec<(String, Option<ConfigurationScope>)>,
}

/// `MainThreadConfigurationShape`. O `IConfigurationOverrides | undefined` é um `Option`; o `null` e o `undefined`
/// de `resource` são ambos `None` em [`ConfigurationOverrides`] (ver `ext_host_configuration`).
pub trait MainThreadConfigurationShape: IDisposable {
    fn rpc_update_configuration_option(
        &self,
        target: Option<ConfigurationTarget>,
        key: &str,
        value: Dynamic,
        overrides: Option<ConfigurationOverrides>,
        scope_to_language: Option<bool>,
    ) -> Promise<()>;
    fn rpc_remove_configuration_option(
        &self,
        target: Option<ConfigurationTarget>,
        key: &str,
        overrides: Option<ConfigurationOverrides>,
        scope_to_language: Option<bool>,
    ) -> Promise<()>;
}

/// `ExtHostConfigurationShape`.
pub trait ExtHostConfigurationShape {
    fn rpc_initialize_configuration(&self, data: IConfigurationInitData);
    fn rpc_accept_configuration_changed(&self, data: IConfigurationInitData, change: ConfigurationChange);
}

// #endregion

// #region Workspace

/// Uma pasta de `IWorkspaceData.folders` (`{ uri, name, index }`). O `UriComponents` do original é o próprio
/// [`URI`] (a conversão é da camada de RPC, como no resto deste arquivo).
#[derive(Clone, Debug)]
pub struct WorkspaceFolderDataDto {
    pub uri: URI,
    pub name: String,
    pub index: usize,
}

/// `IWorkspaceData` (que estende `IStaticWorkspaceData`, de `extensionHostProtocol.ts`).
#[derive(Clone, Debug, Default)]
pub struct IWorkspaceData {
    pub id: String,
    pub name: String,
    pub transient: Option<bool>,
    pub configuration: Option<URI>,
    pub is_untitled: Option<bool>,
    pub folders: Vec<WorkspaceFolderDataDto>,
}

/// `IRelativePatternDto`: `{ pattern, base, baseUri }`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IRelativePatternDto {
    pub pattern: String,
    pub base: String,
    pub base_uri: URI,
}

/// `TextSearchCompleteMessage` (de `searchExtTypes.ts`): o mesmo formato de [`TextSearchCompleteMessage2`].
pub type TextSearchCompleteMessage = TextSearchCompleteMessage2;

/// `ITextSearchComplete`. O `message?: TextSearchCompleteMessage | TextSearchCompleteMessage[]` é sempre uma lista
/// (um valor só é uma lista de um elemento).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ITextSearchComplete {
    pub limit_hit: Option<bool>,
    pub message: Option<Vec<TextSearchCompleteMessage>>,
}

/// `IRawFileMatch2` (`IFileMatch<UriComponents>` de `search.ts`).
#[derive(Clone, Debug)]
pub struct IRawFileMatch2 {
    pub resource: URI,
    pub results: Option<Vec<ITextSearchResult>>,
}

/// `{ saveAs: boolean }` de `$save`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SaveOptionsDto {
    pub save_as: bool,
}

/// `{ uri: UriComponents; name?: string }` de `$updateWorkspaceFolders`.
#[derive(Clone, Debug)]
pub struct WorkspaceFolderToAdd {
    pub uri: URI,
    pub name: Option<String>,
}

/// O `{ encoding: string }` opcional de `$resolveDecoding`, `$validateDetectedEncoding` e `$resolveEncoding`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EncodingOptionDto {
    pub encoding: String,
}

/// O retorno de `$resolveDecoding`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedDecodingDto {
    pub preferred_encoding: String,
    pub guess_encoding: bool,
    pub candidate_guess_encodings: Vec<String>,
}

/// O retorno de `$resolveEncoding`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedEncodingDto {
    pub encoding: String,
    pub add_bom: bool,
}

/// `EditSessionIdentityMatch` (`platform/workspace/common/editSessions.ts`).
pub use je_platform::edit_sessions::EditSessionIdentityMatch;

/// `MainThreadWorkspaceShape`. Os `UriComponents` do original são [`URI`]; o `string | undefined` é `Option<String>`.
pub trait MainThreadWorkspaceShape: IDisposable {
    fn rpc_start_file_search(&self, include_folder: Option<URI>, options: IFileQueryBuilderOptions, token: CancellationToken) -> Promise<Option<Vec<URI>>>;
    fn rpc_start_text_search(
        &self,
        query: PatternInfo,
        folder: Option<URI>,
        options: ITextQueryBuilderOptions,
        request_id: u64,
        token: CancellationToken,
    ) -> Promise<Option<ITextSearchComplete>>;
    fn rpc_check_exists(&self, folders: Vec<URI>, includes: Vec<String>, token: CancellationToken) -> Promise<bool>;
    fn rpc_save(&self, uri: &URI, options: SaveOptionsDto) -> Promise<Option<URI>>;
    fn rpc_save_all(&self, include_untitled: Option<bool>) -> Promise<bool>;
    fn rpc_update_workspace_folders(&self, extension_name: &str, index: f64, delete_count: f64, workspace_folders_to_add: Vec<WorkspaceFolderToAdd>) -> Promise<()>;
    fn rpc_resolve_proxy(&self, url: &str) -> Promise<Option<String>>;
    fn rpc_lookup_authorization(&self, auth_info: AuthInfo) -> Promise<Option<Credentials>>;
    fn rpc_lookup_kerberos_authorization(&self, url: &str) -> Promise<Option<String>>;
    fn rpc_load_certificates(&self) -> Promise<Vec<String>>;
    fn rpc_request_resource_trust(&self, options: ResourceTrustRequestOptions) -> Promise<Option<bool>>;
    fn rpc_request_workspace_trust(&self, options: Option<WorkspaceTrustRequestOptions>) -> Promise<Option<bool>>;
    fn rpc_is_resource_trusted(&self, resource: &URI) -> Promise<bool>;
    fn rpc_register_edit_session_identity_provider(&self, handle: u64, scheme: &str);
    fn rpc_unregister_edit_session_identity_provider(&self, handle: u64);
    fn rpc_register_canonical_uri_provider(&self, handle: u64, scheme: &str);
    fn rpc_unregister_canonical_uri_provider(&self, handle: u64);
    fn rpc_resolve_decoding(&self, resource: Option<URI>, options: Option<EncodingOptionDto>) -> Promise<ResolvedDecodingDto>;
    fn rpc_validate_detected_encoding(&self, resource: Option<URI>, detected_encoding: &str, options: Option<EncodingOptionDto>) -> Promise<String>;
    fn rpc_resolve_encoding(&self, resource: Option<URI>, options: Option<EncodingOptionDto>) -> Promise<ResolvedEncodingDto>;
}

/// `ExtHostWorkspaceShape`.
pub trait ExtHostWorkspaceShape {
    fn rpc_initialize_workspace(&self, workspace: Option<IWorkspaceData>, trusted: bool);
    fn rpc_accept_workspace_data(&self, workspace: Option<IWorkspaceData>);
    fn rpc_handle_text_search_result(&self, result: IRawFileMatch2, request_id: u64);
    fn rpc_on_did_grant_workspace_trust(&self);
    fn rpc_on_did_change_workspace_trusted_folders(&self);
    fn rpc_get_edit_session_identifier(&self, folder: URI, token: CancellationToken) -> Promise<Option<String>>;
    fn rpc_provide_edit_session_identity_match(&self, folder: URI, identity1: &str, identity2: &str, token: CancellationToken) -> Promise<Option<EditSessionIdentityMatch>>;
    fn rpc_on_will_create_edit_session_identity(&self, folder: URI, token: CancellationToken, timeout: f64) -> Promise<()>;
    fn rpc_provide_canonical_uri(&self, uri: URI, target_scheme: &str, token: CancellationToken) -> Promise<Option<URI>>;
}

// #endregion

// #region MessageService

/// A `source` de `MainThreadMessageOptions`: `{ identifier: ExtensionIdentifier; label: string }`.
#[derive(Clone, Debug)]
pub struct MainThreadMessageSource {
    pub identifier: ExtensionIdentifier,
    pub label: String,
}

/// `MainThreadMessageOptions`.
#[derive(Clone, Debug, Default)]
pub struct MainThreadMessageOptions {
    pub source: Option<MainThreadMessageSource>,
    pub modal: Option<bool>,
    pub detail: Option<String>,
    pub use_custom: Option<bool>,
}

/// Um item de `commands` de `$showMessage`: `{ title, isCloseAffordance, handle }`.
#[derive(Clone, Debug, PartialEq)]
pub struct MessageCommandDto {
    pub title: String,
    pub is_close_affordance: bool,
    pub handle: f64,
}

/// `MainThreadMessageServiceShape`. O retorno `number | undefined` é o `handle` do comando escolhido.
pub trait MainThreadMessageServiceShape: IDisposable {
    fn rpc_show_message(&self, severity: Severity, message: &str, options: MainThreadMessageOptions, commands: Vec<MessageCommandDto>) -> Promise<Option<f64>>;
}

// #endregion

// #region TreeViews

/// As opções de `$registerTreeViewDataProvider`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TreeViewRegistrationOptions {
    pub show_collapse_all: bool,
    pub can_select_many: bool,
    pub drop_mime_types: Vec<String>,
    pub drag_mime_types: Vec<String>,
    pub has_handle_drag: bool,
    pub has_handle_drop: bool,
    pub manually_manage_checkboxes: bool,
}

/// `{ [treeItemHandle: string]: ITreeItem }` de `$refresh`: as entradas na ordem em que o objeto as recebeu.
pub type ItemsToRefresh = Vec<(String, ITreeItem)>;

/// `{ item: ITreeItem; parentChain: ITreeItem[] }` de `$reveal`.
#[derive(Clone, Debug)]
pub struct RevealItemInfo {
    pub item: ITreeItem,
    pub parent_chain: Vec<ITreeItem>,
}

/// `MainThreadTreeViewsShape`. O `message: string | IMarkdownString` de `$setMessage` é [`StringOrMarkdown`] e o
/// `VSBuffer` de `$resolveDropFileData` é o de `je-base`.
pub trait MainThreadTreeViewsShape: IDisposable {
    fn rpc_register_tree_view_data_provider(&self, tree_view_id: &str, options: TreeViewRegistrationOptions) -> Promise<()>;
    fn rpc_refresh(&self, tree_view_id: &str, items_to_refresh: Option<ItemsToRefresh>) -> Promise<()>;
    fn rpc_reveal(&self, tree_view_id: &str, item_info: Option<RevealItemInfo>, options: IRevealOptions) -> Promise<()>;
    fn rpc_set_message(&self, tree_view_id: &str, message: StringOrMarkdown);
    fn rpc_set_title(&self, tree_view_id: &str, title: &str, description: Option<&str>);
    fn rpc_set_badge(&self, tree_view_id: &str, badge: Option<IViewBadge>);
    fn rpc_resolve_drop_file_data(&self, destination_view_id: &str, request_id: f64, data_item_id: &str) -> Promise<VSBuffer>;
    fn rpc_dispose_tree(&self, tree_view_id: &str) -> Promise<()>;
    fn rpc_log_resolve_tree_node_failure(&self, extension_id: &str);
}

/// `IDataTransferFileDTO`.
#[derive(Clone, Debug)]
pub struct DataTransferFileDto {
    pub id: String,
    pub name: String,
    pub uri: Option<URI>,
}

/// Um elemento de `DataTransferItemDTO.uriListData`: `string | UriComponents`.
#[derive(Clone, Debug)]
pub enum UriListPart {
    Text(String),
    Uri(URI),
}

/// `DataTransferItemDTO`.
#[derive(Clone, Debug)]
pub struct DataTransferItemDto {
    pub id: String,
    pub as_string: String,
    pub file_data: Option<DataTransferFileDto>,
    pub uri_list_data: Option<Vec<UriListPart>>,
}

/// `DataTransferDTO`: o `Array<readonly [type, DataTransferItemDTO]>` é uma lista de pares.
#[derive(Clone, Debug, Default)]
pub struct DataTransferDto {
    pub items: Vec<(String, DataTransferItemDto)>,
}

/// `CheckboxUpdate`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckboxUpdate {
    pub tree_item_handle: String,
    pub new_state: bool,
}

/// Um elemento de `$getChildren`: o `[index, ...children]` do original (`(number | ITreeItem)[]`, onde o primeiro
/// elemento é o índice do pai na lista pedida e o resto são os filhos dele).
#[derive(Clone, Debug)]
pub struct TreeChildrenBatch {
    pub index: usize,
    pub children: Vec<ITreeItem>,
}

/// `ExtHostTreeViewsShape`. Os métodos que lançam no original (`NoTreeViewError`) e não devolvem promessa lançam como
/// pânico com o [`Error`] (ver `ext_host_commands`); os que devolvem promessa a rejeitam.
pub trait ExtHostTreeViewsShape {
    fn rpc_get_children(&self, tree_view_id: &str, tree_item_handles: Option<Vec<String>>) -> Promise<Option<Vec<TreeChildrenBatch>>>;
    #[allow(clippy::too_many_arguments)]
    fn rpc_handle_drop(
        &self,
        destination_view_id: &str,
        request_id: f64,
        tree_data_transfer: DataTransferDto,
        target_handle: Option<String>,
        token: CancellationToken,
        operation_uuid: Option<String>,
        source_view_id: Option<String>,
        source_tree_item_handles: Option<Vec<String>>,
    ) -> Promise<()>;
    fn rpc_handle_drag(&self, source_view_id: &str, source_tree_item_handles: Vec<String>, operation_uuid: &str, token: CancellationToken) -> Promise<Option<DataTransferDto>>;
    fn rpc_release_drag_operation(&self, operation_uuid: &str);
    fn rpc_set_expanded(&self, tree_view_id: &str, tree_item_handle: &str, expanded: bool);
    fn rpc_set_selection_and_focus(&self, tree_view_id: &str, selection_handles: Vec<String>, focus_handle: &str);
    fn rpc_set_visible(&self, tree_view_id: &str, visible: bool);
    fn rpc_change_checkbox_state(&self, tree_view_id: &str, checkbox_updates: Vec<CheckboxUpdate>);
    fn rpc_has_resolve(&self, tree_view_id: &str) -> Promise<bool>;
    fn rpc_resolve(&self, tree_view_id: &str, tree_item_handle: &str, token: CancellationToken) -> Promise<Option<ITreeItem>>;
}

// #endregion

// #region Editores personalizados (webview)

pub use self::custom_editors_region::*;

/// Região de `extHost.protocol.ts` que `extHostCustomEditors.ts` usa: `MainThreadCustomEditorsShape`,
/// `ExtHostCustomEditorsShape`, os DTOs de webview e de editor personalizado e os identificadores
/// `MainContext.MainThreadCustomEditors` e `ExtHostContext.ExtHostCustomEditors`.
///
/// Divergências:
/// - `UriComponents` é a [`URI`]; `IRange` e `ISelection` são o `Range` e o `Selection` do editor; `VSBuffer` é o
///   [`VSBuffer`]. `WebviewHandle` é `String`.
/// - O tipo anônimo de `initData` de `$resolveCustomEditor` tem os mesmos campos de `CustomEditorDiffInitData`, e os
///   dois são o mesmo struct ([`CustomEditorResolveInitData`] é o alias).
/// - `enableCommandUris?: boolean | readonly string[]` é [`EnableCommandUris`]. Os campos opcionais são `Option`.
/// - `$resolveCustomTextEditorNavigation` devolve `{ selection: ISelection | undefined } | undefined`:
///   `Option<CustomTextEditorNavigationResult>`. O `$createCustomDocument` devolve [`CreateCustomDocumentResult`].
/// - `$disposeEdits` e `$disposeCustomTextEditorNavigation` lançam no original (documento inexistente): o `Err` é o
///   lançamento de `$disposeEdits`.
mod custom_editors_region {
    use je_base::buffer::VSBuffer;
    use je_base::cancellation::CancellationToken;
    use je_base::errors::Error;
    use je_base::lifecycle::IDisposable;
    use je_base::promise::Promise;
    use je_base::uri::URI;
    use je_editor::range::Range as EditorRange;
    use je_editor::selection::Selection as EditorSelection;
    use je_extensions::manifest::ExtensionIdentifier;

    use super::{EditorGroupColumn, ExtHostContext, MainContext, ProxyIdentifier};

    /// `WebviewHandle`.
    pub type WebviewHandle = String;

    /// `WebviewExtensionDescription`.
    #[derive(Clone, Debug)]
    pub struct WebviewExtensionDescription {
        pub id: ExtensionIdentifier,
        pub location: URI,
    }

    /// `IWebviewPortMapping`.
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct WebviewPortMapping {
        pub webview_port: f64,
        pub extension_host_port: f64,
    }

    /// `enableCommandUris?: boolean | readonly string[]`.
    #[derive(Clone, Debug, PartialEq)]
    pub enum EnableCommandUris {
        All(bool),
        Commands(Vec<String>),
    }

    /// `IWebviewContentOptions`: cada campo ausente é `undefined`.
    #[derive(Clone, Debug, Default, PartialEq)]
    pub struct WebviewContentOptions {
        pub enable_scripts: Option<bool>,
        pub enable_forms: Option<bool>,
        pub enable_command_uris: Option<EnableCommandUris>,
        pub local_resource_roots: Option<Vec<URI>>,
        pub port_mapping: Option<Vec<WebviewPortMapping>>,
    }

    /// `IWebviewPanelOptions`.
    #[derive(Clone, Copy, Debug, Default, PartialEq)]
    pub struct WebviewPanelOptions {
        pub enable_find_widget: Option<bool>,
        pub retain_context_when_hidden: Option<bool>,
    }

    /// `CustomEditorProviderCapabilities`.
    #[derive(Clone, Copy, Debug, Default, PartialEq)]
    pub struct CustomEditorProviderCapabilities {
        pub supports_move: Option<bool>,
        pub supports_navigation: Option<bool>,
        pub supports_inline_diff: Option<bool>,
        pub supports_side_by_side_diff: Option<bool>,
    }

    /// `CustomEditorDiffInitData`.
    #[derive(Clone, Debug, PartialEq)]
    pub struct CustomEditorDiffInitData {
        pub title: String,
        pub content_options: WebviewContentOptions,
        pub options: WebviewPanelOptions,
        pub active: bool,
    }

    /// O `initData` anônimo de `$resolveCustomEditor`, de mesma forma que [`CustomEditorDiffInitData`].
    pub type CustomEditorResolveInitData = CustomEditorDiffInitData;

    /// `CustomEditorSideBySideDiffWebviewHandles`.
    #[derive(Clone, Debug, PartialEq)]
    pub struct CustomEditorSideBySideDiffWebviewHandles {
        pub original: WebviewHandle,
        pub modified: WebviewHandle,
    }

    /// `CustomEditorSideBySideDiffInitData`.
    #[derive(Clone, Debug, PartialEq)]
    pub struct CustomEditorSideBySideDiffInitData {
        pub original: CustomEditorDiffInitData,
        pub modified: CustomEditorDiffInitData,
    }

    /// O `{ editable: boolean }` de `$createCustomDocument`.
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct CreateCustomDocumentResult {
        pub editable: bool,
    }

    /// O `{ selection: ISelection | undefined }` de `$resolveCustomTextEditorNavigation`.
    #[derive(Clone, Debug, PartialEq)]
    pub struct CustomTextEditorNavigationResult {
        pub selection: Option<EditorSelection>,
    }

    /// `MainThreadCustomEditorsShape`.
    #[allow(clippy::too_many_arguments)]
    pub trait MainThreadCustomEditorsShape: IDisposable {
        fn rpc_on_did_change_custom_text_editor_selection(&self, handle: &str, selection: Option<EditorSelection>);
        fn rpc_register_text_editor_provider(
            &self,
            extension: WebviewExtensionDescription,
            view_type: &str,
            options: WebviewPanelOptions,
            capabilities: CustomEditorProviderCapabilities,
            serialize_buffers_for_post_message: bool,
        );
        fn rpc_register_custom_editor_provider(
            &self,
            extension: WebviewExtensionDescription,
            view_type: &str,
            options: WebviewPanelOptions,
            capabilities: CustomEditorProviderCapabilities,
            supports_multiple_editors_per_document: bool,
            serialize_buffers_for_post_message: bool,
        );
        fn rpc_unregister_editor_provider(&self, view_type: &str);

        fn rpc_on_did_edit(&self, resource: &URI, view_type: &str, edit_id: u32, label: Option<&str>);
        fn rpc_on_content_change(&self, resource: &URI, view_type: &str);
    }

    /// `ExtHostCustomEditorsShape`.
    #[allow(clippy::too_many_arguments)]
    pub trait ExtHostCustomEditorsShape {
        fn rpc_resolve_custom_text_editor_navigation(&self, handle: &str, view_type: &str, resource: &URI, token: CancellationToken) -> Promise<Option<CustomTextEditorNavigationResult>>;
        fn rpc_dispose_custom_text_editor_navigation(&self, handle: &str);
        fn rpc_reveal_custom_text_editor_range(&self, handle: &str, range: EditorRange, selection: Option<EditorSelection>, preserve_focus: bool, token: CancellationToken) -> Promise<()>;
        fn rpc_capture_custom_text_editor_view_state(&self, handle: &str, state_id: u32) -> Promise<()>;
        fn rpc_restore_custom_text_editor_view_state(&self, handle: &str, state_id: u32, token: CancellationToken) -> Promise<()>;
        fn rpc_release_custom_text_editor_view_state(&self, handle: &str, state_id: u32);
        fn rpc_resolve_custom_editor(
            &self,
            resource: &URI,
            new_webview_handle: &str,
            view_type: &str,
            init_data: CustomEditorResolveInitData,
            position: EditorGroupColumn,
            cancellation: CancellationToken,
        ) -> Promise<()>;
        fn rpc_resolve_custom_editor_inline_diff(
            &self,
            original_resource: &URI,
            modified_resource: &URI,
            new_webview_handle: &str,
            view_type: &str,
            init_data: CustomEditorDiffInitData,
            position: EditorGroupColumn,
            cancellation: CancellationToken,
        ) -> Promise<()>;
        fn rpc_resolve_custom_editor_side_by_side_diff(
            &self,
            original_resource: &URI,
            modified_resource: &URI,
            webview_handles: CustomEditorSideBySideDiffWebviewHandles,
            view_type: &str,
            init_data: CustomEditorSideBySideDiffInitData,
            position: EditorGroupColumn,
            cancellation: CancellationToken,
        ) -> Promise<()>;
        fn rpc_create_custom_document(
            &self,
            resource: &URI,
            view_type: &str,
            backup_id: Option<String>,
            untitled_document_data: Option<VSBuffer>,
            cancellation: CancellationToken,
        ) -> Promise<CreateCustomDocumentResult>;
        fn rpc_dispose_custom_document(&self, resource: &URI, view_type: &str) -> Promise<()>;

        fn rpc_undo(&self, resource: &URI, view_type: &str, edit_id: u32, is_dirty: bool) -> Promise<()>;
        fn rpc_redo(&self, resource: &URI, view_type: &str, edit_id: u32, is_dirty: bool) -> Promise<()>;
        fn rpc_revert(&self, resource: &URI, view_type: &str, cancellation: CancellationToken) -> Promise<()>;
        fn rpc_dispose_edits(&self, resource: &URI, view_type: &str, edit_ids: Vec<u32>) -> Result<(), Error>;

        fn rpc_on_save(&self, resource: &URI, view_type: &str, cancellation: CancellationToken) -> Promise<()>;
        fn rpc_on_save_as(&self, resource: &URI, view_type: &str, target_resource: &URI, cancellation: CancellationToken) -> Promise<()>;

        fn rpc_backup(&self, resource: &URI, view_type: &str, cancellation: CancellationToken) -> Promise<String>;

        fn rpc_on_move_custom_editor(&self, handle: &str, new_resource: &URI, view_type: &str) -> Promise<()>;
    }

    impl MainContext {
        pub const MAIN_THREAD_CUSTOM_EDITORS: ProxyIdentifier<dyn MainThreadCustomEditorsShape> = ProxyIdentifier::new("MainThreadCustomEditors");
    }

    impl ExtHostContext {
        pub const EXT_HOST_CUSTOM_EDITORS: ProxyIdentifier<dyn ExtHostCustomEditorsShape> = ProxyIdentifier::new("ExtHostCustomEditors");
    }
}

// #endregion

/// `MainContext`: os identificadores dos proxies da thread principal.
pub struct MainContext;

impl MainContext {
    pub const MAIN_THREAD_BROWSERS: ProxyIdentifier<dyn MainThreadBrowsersShape> = ProxyIdentifier::new("MainThreadBrowsers");
    pub const MAIN_THREAD_COMMANDS: ProxyIdentifier<dyn MainThreadCommandsShape> = ProxyIdentifier::new("MainThreadCommands");
    pub const MAIN_THREAD_CONFIGURATION: ProxyIdentifier<dyn MainThreadConfigurationShape> = ProxyIdentifier::new("MainThreadConfiguration");
    pub const MAIN_THREAD_MESSAGE_SERVICE: ProxyIdentifier<dyn MainThreadMessageServiceShape> = ProxyIdentifier::new("MainThreadMessageService");
    pub const MAIN_THREAD_TELEMETRY: ProxyIdentifier<dyn MainThreadTelemetryShape> = ProxyIdentifier::new("MainThreadTelemetry");
    pub const MAIN_THREAD_TREE_VIEWS: ProxyIdentifier<dyn MainThreadTreeViewsShape> = ProxyIdentifier::new("MainThreadTreeViews");
    pub const MAIN_THREAD_WORKSPACE: ProxyIdentifier<dyn MainThreadWorkspaceShape> = ProxyIdentifier::new("MainThreadWorkspace");
}

/// `ExtHostContext`: os identificadores dos proxies do lado das extensões.
pub struct ExtHostContext;

impl ExtHostContext {
    pub const EXT_HOST_COMMANDS: ProxyIdentifier<dyn ExtHostCommandsShape> = ProxyIdentifier::new("ExtHostCommands");
    pub const EXT_HOST_CONFIGURATION: ProxyIdentifier<dyn ExtHostConfigurationShape> = ProxyIdentifier::new("ExtHostConfiguration");
    pub const EXT_HOST_TREE_VIEWS: ProxyIdentifier<dyn ExtHostTreeViewsShape> = ProxyIdentifier::new("ExtHostTreeViews");
    pub const EXT_HOST_WORKSPACE: ProxyIdentifier<dyn ExtHostWorkspaceShape> = ProxyIdentifier::new("ExtHostWorkspace");
}

// #region File system events

pub use self::file_system_event_region::*;

/// `FileSystemEvents`, `SourceTargetPair`, `IWillRunFileOperationParticipation`, `MainThreadFileSystemEventServiceShape`,
/// `ExtHostFileSystemEventServiceShape` e `ExtHostFileSystemInfoShape`.
///
/// Divergências: as `UriComponents` já chegam revividas como [`URI`]; o `session: number | undefined` de
/// `FileSystemEvents` é `Option<f64>`; o `capabilities: number | null` de `$acceptProviderInfos` é `Option` das capacidades.
mod file_system_event_region {
    use je_base::cancellation::CancellationToken;
    use je_base::lifecycle::IDisposable;
    use je_base::promise::Promise;
    use je_base::uri::URI;
    use je_platform::files::files::{FileOperation, FileSystemProviderCapabilities, WatchOptions};

    use super::{ExtHostContext, IWorkspaceEditDto, MainContext, ProxyIdentifier};

    /// `FileSystemEvents`.
    #[derive(Clone, Debug, Default)]
    pub struct FileSystemEvents {
        pub session: Option<f64>,
        pub created: Vec<URI>,
        pub changed: Vec<URI>,
        pub deleted: Vec<URI>,
    }

    /// `SourceTargetPair`.
    #[derive(Clone, Debug)]
    pub struct SourceTargetPair {
        pub source: Option<URI>,
        pub target: URI,
    }

    /// `IWillRunFileOperationParticipation`.
    #[derive(Clone)]
    pub struct IWillRunFileOperationParticipation {
        pub edit: IWorkspaceEditDto,
        pub extension_names: Vec<String>,
    }

    /// `MainThreadFileSystemEventServiceShape`.
    pub trait MainThreadFileSystemEventServiceShape: IDisposable {
        fn rpc_watch(&self, extension_id: &str, session: f64, resource: &URI, opts: WatchOptions, correlate: bool);
        fn rpc_unwatch(&self, session: f64);
    }

    /// `ExtHostFileSystemEventServiceShape`.
    pub trait ExtHostFileSystemEventServiceShape {
        fn rpc_on_file_event(&self, events: FileSystemEvents);
        fn rpc_on_will_run_file_operation(
            &self,
            operation: FileOperation,
            files: Vec<SourceTargetPair>,
            timeout: f64,
            token: CancellationToken,
        ) -> Promise<Option<IWillRunFileOperationParticipation>>;
        fn rpc_on_did_run_file_operation(&self, operation: FileOperation, files: Vec<SourceTargetPair>);
    }

    /// `ExtHostFileSystemInfoShape`.
    pub trait ExtHostFileSystemInfoShape {
        fn rpc_accept_provider_infos(&self, uri: &URI, capabilities: Option<FileSystemProviderCapabilities>);
    }

    impl MainContext {
        pub const MAIN_THREAD_FILE_SYSTEM_EVENT_SERVICE: ProxyIdentifier<dyn MainThreadFileSystemEventServiceShape> =
            ProxyIdentifier::new("MainThreadFileSystemEventService");
    }

    impl ExtHostContext {
        pub const EXT_HOST_FILE_SYSTEM_EVENT_SERVICE: ProxyIdentifier<dyn ExtHostFileSystemEventServiceShape> = ProxyIdentifier::new("ExtHostFileSystemEventService");
        pub const EXT_HOST_FILE_SYSTEM_INFO: ProxyIdentifier<dyn ExtHostFileSystemInfoShape> = ProxyIdentifier::new("ExtHostFileSystemInfo");
    }
}

// #endregion

// #region Progress (bloco próprio: `extHostProgress.ts` e `mainThreadProgress.ts`)

pub use self::progress_region::*;

/// Região de `extHost.protocol.ts` que `extHostProgress.ts` e `mainThreadProgress.ts` usam: `IProgressOptionsDto`,
/// `IProgressStepDto`, `MainThreadProgressShape`, `ExtHostProgressShape` e os identificadores
/// `MainContext.MainThreadProgress` e `ExtHostContext.ExtHostProgress`.
///
/// Divergências:
/// - `IProgressOptionsDto extends Omit<IProgressOptions, 'title' | 'legacyExtensionLinkParsing' | 'telemetry'>`: o
///   struct tem só os campos que sobram (`location`, `source`, `total`, `cancellable`, `buttons`) mais o `title`
///   como texto e o `telemetryId`. Os campos de capacidade (`legacyExtensionLinkParsing`, `telemetry`) não existem no
///   DTO, o que o compilador garante. `IProgressStepDto extends Omit<IProgressStep, 'message'>` é o struct com o
///   `message` como texto.
/// - O `handle: number` é `u32` (o contador de `ExtHostProgress`), e o `extensionId?: string` é `Option<&str>`.
mod progress_region {
    use je_base::lifecycle::IDisposable;
    use je_base::promise::Promise;
    use je_platform::notification::notification::NotificationSourceOrText;
    use je_platform::notification::notification_telemetry::NotificationTelemetryId;
    use je_platform::progress::progress::{ProgressCancellable, ProgressLocationId};

    use super::ProxyIdentifier;

    /// `IProgressOptionsDto`.
    #[derive(Clone, Debug)]
    pub struct IProgressOptionsDto {
        pub location: ProgressLocationId,
        pub title: Option<String>,
        pub telemetry_id: Option<NotificationTelemetryId>,
        pub source: Option<NotificationSourceOrText>,
        pub total: Option<f64>,
        pub cancellable: Option<ProgressCancellable>,
        pub buttons: Option<Vec<String>>,
    }

    impl IProgressOptionsDto {
        /// O DTO só com o local, o resto ausente.
        pub fn new(location: impl Into<ProgressLocationId>) -> Self {
            Self { location: location.into(), title: None, telemetry_id: None, source: None, total: None, cancellable: None, buttons: None }
        }
    }

    /// `IProgressStepDto`.
    #[derive(Clone, Debug, Default, PartialEq)]
    pub struct IProgressStepDto {
        pub message: Option<String>,
        pub increment: Option<f64>,
        pub total: Option<f64>,
    }

    /// `MainThreadProgressShape`.
    pub trait MainThreadProgressShape: IDisposable {
        fn rpc_start_progress(&self, handle: u32, options: IProgressOptionsDto, extension_id: Option<&str>) -> Promise<()>;
        fn rpc_progress_report(&self, handle: u32, message: IProgressStepDto);
        fn rpc_progress_end(&self, handle: u32);
    }

    /// `ExtHostProgressShape`.
    pub trait ExtHostProgressShape {
        fn rpc_accept_progress_canceled(&self, handle: u32);
    }

    impl super::MainContext {
        pub const MAIN_THREAD_PROGRESS: ProxyIdentifier<dyn MainThreadProgressShape> = ProxyIdentifier::new("MainThreadProgress");
    }

    impl super::ExtHostContext {
        pub const EXT_HOST_PROGRESS: ProxyIdentifier<dyn ExtHostProgressShape> = ProxyIdentifier::new("ExtHostProgress");
    }
}

// #endregion

// #region ManagedSockets

pub use self::managed_sockets_region::*;

/// `MainThreadManagedSocketsShape` e `ExtHostManagedSocketsShape` (de `extHost.protocol.ts`, usadas por
/// `mainThreadManagedSockets.ts`) e os identificadores `MainContext.MainThreadManagedSockets` e
/// `ExtHostContext.ExtHostManagedSockets`.
///
/// Divergências: `socketFactoryId` e `socketId` (`number`) são `u64`, como os demais identificadores de registro do protocolo;
/// o `error: string | undefined` de `$onDidManagedSocketClose` é `Option<String>`.
mod managed_sockets_region {
    use je_base::buffer::VSBuffer;
    use je_base::lifecycle::IDisposable;
    use je_base::promise::Promise;

    use super::{ExtHostContext, MainContext, ProxyIdentifier};

    /// `MainThreadManagedSocketsShape`.
    pub trait MainThreadManagedSocketsShape: IDisposable {
        fn rpc_register_socket_factory(&self, socket_factory_id: u64) -> Promise<()>;
        fn rpc_unregister_socket_factory(&self, socket_factory_id: u64) -> Promise<()>;
        fn rpc_on_did_managed_socket_have_data(&self, socket_id: u64, data: VSBuffer);
        fn rpc_on_did_managed_socket_close(&self, socket_id: u64, error: Option<String>);
        fn rpc_on_did_managed_socket_end(&self, socket_id: u64);
    }

    /// `ExtHostManagedSocketsShape`.
    pub trait ExtHostManagedSocketsShape {
        fn rpc_open_remote_socket(&self, socket_factory_id: u64) -> Promise<u64>;
        fn rpc_remote_socket_write(&self, socket_id: u64, buffer: VSBuffer);
        fn rpc_remote_socket_end(&self, socket_id: u64);
        fn rpc_remote_socket_drain(&self, socket_id: u64) -> Promise<()>;
    }

    impl MainContext {
        pub const MAIN_THREAD_MANAGED_SOCKETS: ProxyIdentifier<dyn MainThreadManagedSocketsShape> = ProxyIdentifier::new("MainThreadManagedSockets");
    }

    impl ExtHostContext {
        pub const EXT_HOST_MANAGED_SOCKETS: ProxyIdentifier<dyn ExtHostManagedSocketsShape> = ProxyIdentifier::new("ExtHostManagedSockets");
    }
}

// #endregion

// #region Comments

pub use self::comments_region::*;

/// Região de `extHost.protocol.ts` que `extHostComments.ts` usa: `CommentProviderFeatures`, `CommentChanges`,
/// `CommentThreadChanges`, `MainThreadCommentsShape`, `ExtHostCommentsShape` e os identificadores
/// `MainContext.MainThreadComments` e `ExtHostContext.ExtHostComments`. Entram junto os tipos de `languages.ts` que
/// esses DTOs citam (`CommentReaction`, `CommentAuthorInformation`, `CommentOptions`, `CommentThreadRevealOptions`,
/// `CommentingRangeResourceHint`).
///
/// Divergências:
/// - Os handles (`number`) são `u32`, como o de `ExtHostProgress`; os `UriComponents` são [`URI`] e o `IRange` é o
///   [`EditorRange`] do editor. O `IRange | ICellRange | undefined` de `$createCommentThread` é só `IRange`: o host
///   de extensões nunca produz `ICellRange`.
/// - Os enums `languages.CommentMode`, `CommentState`, `CommentThreadCollapsibleState`, `CommentThreadState` e
///   `CommentThreadApplicability` têm os mesmos valores dos da API (`types.*`), e o original só os converte por um
///   `switch` identidade; aqui o DTO usa os enums de [`ext_host_types`](crate::api::common::ext_host_types).
/// - `languages.Comment` (o `comments: languages.Comment[]` de `$createCommentThread` e o `comment` de
///   `$toggleReaction`) e `CommentChanges` têm a mesma forma e são o struct [`CommentChanges`]. O retorno
///   `languages.CommentThread | undefined` de `$createCommentThread` é o modelo da thread principal e o host nunca o
///   lê: é `()`.
/// - `CommentThreadChanges` é o `Partial<...>` do original: cada campo é um `Option` (a chave existe ou não). Onde o
///   valor também pode ser `undefined` (`range`, `label`) ou `null` (`contextValue`), o campo é `Option<Option<_>>`:
///   o `Some(None)` é a chave presente sem valor, e o `contextValue` com `Some(None)` é o `null` (limpar).
/// - O `canReply: boolean | CommentAuthorInformation` é [`CommentCanReply`], com o `Rc` para a comparação de identidade
///   que o original faz (`this._canReply !== state`).
/// - `$provideCommentingRanges` devolve `{ ranges, fileComments } | undefined`, o struct [`CommentingRangesDto`]; o
///   `{ commentThreadHandle; uniqueIdInThread? }` de `$setActiveComment` é [`ActiveCommentInfo`].
mod comments_region {
    use std::rc::Rc;

    use je_base::cancellation::CancellationToken;
    use je_base::lifecycle::IDisposable;
    use je_base::promise::Promise;
    use je_base::uri::URI;
    use je_editor::languages::StringOrMarkdown;
    use je_editor::range::Range as EditorRange;
    use je_extensions::manifest::ExtensionIdentifier;

    use super::{ExtHostContext, MainContext, ProxyIdentifier};
    use crate::api::common::ext_host_types::{CommentMode, CommentState, CommentThreadApplicability, CommentThreadCollapsibleState, CommentThreadState};

    /// `languages.CommentAuthorInformation` (e o `vscode.CommentAuthorInformation`, que tem a mesma forma).
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct CommentAuthorInformation {
        pub name: String,
        pub icon_path: Option<URI>,
    }

    /// `languages.CommentReaction`.
    #[derive(Clone, Debug, Default, PartialEq)]
    pub struct CommentReaction {
        pub label: Option<String>,
        pub icon_path: Option<URI>,
        pub count: Option<f64>,
        pub has_reacted: Option<bool>,
        pub can_edit: Option<bool>,
        pub reactors: Option<Vec<String>>,
    }

    /// `languages.CommentOptions`.
    #[derive(Clone, Debug, Default, PartialEq, Eq)]
    pub struct CommentOptions {
        pub prompt: Option<String>,
        pub place_holder: Option<String>,
    }

    /// `languages.CommentThreadRevealOptions`.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct CommentThreadRevealOptions {
        pub preserve_focus: bool,
        pub focus_reply: bool,
    }

    /// `languages.CommentingRangeResourceHint`.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct CommentingRangeResourceHint {
        pub schemes: Vec<String>,
    }

    /// `boolean | languages.CommentAuthorInformation`.
    #[derive(Clone, Debug)]
    pub enum CommentCanReply {
        Bool(bool),
        Author(Rc<CommentAuthorInformation>),
    }

    impl CommentCanReply {
        /// O `===` do original: os booleanos por valor, os objetos por identidade.
        pub fn same(&self, other: &CommentCanReply) -> bool {
            match (self, other) {
                (Self::Bool(a), Self::Bool(b)) => a == b,
                (Self::Author(a), Self::Author(b)) => Rc::ptr_eq(a, b),
                _ => false,
            }
        }
    }

    /// `CommentProviderFeatures`.
    #[derive(Clone, Debug, Default)]
    pub struct CommentProviderFeatures {
        pub reaction_group: Option<Vec<CommentReaction>>,
        pub reaction_handler: Option<bool>,
        pub options: Option<CommentOptions>,
    }

    /// `CommentChanges` (e `languages.Comment`).
    #[derive(Clone, Debug)]
    pub struct CommentChanges {
        pub unique_id_in_thread: u32,
        pub body: StringOrMarkdown,
        pub user_name: String,
        pub user_icon_path: Option<URI>,
        pub context_value: Option<String>,
        pub comment_reactions: Option<Vec<CommentReaction>>,
        pub label: Option<String>,
        pub mode: Option<CommentMode>,
        pub state: Option<CommentState>,
        pub timestamp: Option<String>,
    }

    /// `CommentThreadChanges`.
    #[derive(Clone, Debug, Default)]
    pub struct CommentThreadChanges {
        pub range: Option<Option<EditorRange>>,
        pub label: Option<Option<String>>,
        pub context_value: Option<Option<String>>,
        pub comments: Option<Vec<CommentChanges>>,
        pub collapse_state: Option<CommentThreadCollapsibleState>,
        pub can_reply: Option<CommentCanReply>,
        pub state: Option<CommentThreadState>,
        pub applicability: Option<CommentThreadApplicability>,
        pub is_template: Option<bool>,
    }

    /// O `{ ranges: IRange[]; fileComments: boolean }` de `$provideCommentingRanges`.
    #[derive(Clone, Debug)]
    pub struct CommentingRangesDto {
        pub ranges: Vec<EditorRange>,
        pub file_comments: bool,
    }

    /// O `{ commentThreadHandle: number; uniqueIdInThread?: number }` de `$setActiveComment`.
    #[derive(Clone, Copy, Debug)]
    pub struct ActiveCommentInfo {
        pub comment_thread_handle: u32,
        pub unique_id_in_thread: Option<u32>,
    }

    /// `MainThreadCommentsShape`.
    pub trait MainThreadCommentsShape: IDisposable {
        fn rpc_register_comment_controller(&self, handle: u32, id: &str, label: &str, extension_id: &str);
        fn rpc_unregister_comment_controller(&self, handle: u32);
        fn rpc_update_comment_controller_features(&self, handle: u32, features: CommentProviderFeatures);
        #[allow(clippy::too_many_arguments)]
        fn rpc_create_comment_thread(
            &self,
            handle: u32,
            comment_thread_handle: u32,
            thread_id: &str,
            resource: &URI,
            range: Option<EditorRange>,
            comments: Vec<CommentChanges>,
            extension_id: &ExtensionIdentifier,
            is_template: bool,
            editor_id: Option<&str>,
        );
        fn rpc_update_comment_thread(&self, handle: u32, comment_thread_handle: u32, thread_id: &str, resource: &URI, changes: CommentThreadChanges);
        fn rpc_delete_comment_thread(&self, handle: u32, comment_thread_handle: u32);
        fn rpc_update_commenting_ranges(&self, handle: u32, resource_hints: Option<CommentingRangeResourceHint>);
        fn rpc_reveal_comment_thread(&self, handle: u32, comment_thread_handle: u32, comment_unique_id_in_thread: u32, options: CommentThreadRevealOptions) -> Promise<()>;
        fn rpc_hide_comment_thread(&self, handle: u32, comment_thread_handle: u32);
    }

    /// `ExtHostCommentsShape`. `$deleteCommentThread` é o único método síncrono; os demais são `async` no original e
    /// rejeitam em vez de lançar.
    pub trait ExtHostCommentsShape {
        fn rpc_create_comment_thread_template(&self, comment_controller_handle: u32, uri: &URI, range: Option<EditorRange>, editor_id: Option<&str>) -> Promise<()>;
        fn rpc_update_comment_thread_template(&self, comment_controller_handle: u32, thread_handle: u32, range: EditorRange) -> Promise<()>;
        fn rpc_update_comment_thread(&self, comment_controller_handle: u32, thread_handle: u32, changes: CommentThreadChanges) -> Promise<()>;
        fn rpc_delete_comment_thread(&self, comment_controller_handle: u32, comment_thread_handle: u32);
        fn rpc_provide_commenting_ranges(&self, comment_controller_handle: u32, uri: &URI, token: CancellationToken) -> Promise<Option<CommentingRangesDto>>;
        fn rpc_toggle_reaction(&self, comment_controller_handle: u32, thread_handle: u32, uri: &URI, comment: CommentChanges, reaction: CommentReaction) -> Promise<()>;
        fn rpc_set_active_comment(&self, controller_handle: u32, comment_info: Option<ActiveCommentInfo>) -> Promise<()>;
    }

    impl MainContext {
        pub const MAIN_THREAD_COMMENTS: ProxyIdentifier<dyn MainThreadCommentsShape> = ProxyIdentifier::new("MainThreadComments");
    }

    impl ExtHostContext {
        pub const EXT_HOST_COMMENTS: ProxyIdentifier<dyn ExtHostCommentsShape> = ProxyIdentifier::new("ExtHostComments");
    }
}

// #endregion

// #region QuickOpen

pub use self::quick_open_region::*;

/// `MainThreadQuickOpenShape`, `ExtHostQuickOpenShape`, os DTOs de transferência (`TransferQuickPickItem`,
/// `TransferQuickInputButton`, `TransferQuickInput`, `IInputBoxOptions`, `IconPathDto`) e os identificadores
/// `MainContext.MainThreadQuickOpen` e `ExtHostContext.ExtHostQuickOpen` (de `extHost.protocol.ts`, usados por
/// `mainThreadQuickOpen.ts`).
///
/// Divergências:
/// - `UriComponents` é [`URI`] e `IconPathDto` (`UriComponents | { light, dark } | ThemeIcon`) é o enum [`IconPathDto`].
/// - `TransferQuickPickItem` e `TransferQuickInputButton` estendem `IQuickPickItem` e `IQuickInputButton`: os campos
///   que o `MainThreadQuickOpen` preenche (`iconPath`, `iconClass`, `iconClasses`) estão no struct. `label` é
///   `Option<String>` porque o original o preenche com `??=` quando só há `resourceUri`. O `toggle?: { checked }` é o
///   [`QuickInputButtonToggle`] compartilhado, que o widget escreve.
/// - `handle` de item é `u32`, de botão é `i64` (o botão de voltar da API tem `handle` -1); `instance` e `sessionId`
///   são `u32`. O `handle` ausente do evento de botão (o botão do serviço, que não é um `TransferQuickInputButton`) é
///   `None`.
/// - `TransferQuickInput` tem o `[key: string]: any` que o `$createOrUpdate` percorre com `for ... in`: [`params`](TransferQuickInput::params)
///   guarda os pares na ordem de inserção, cada um como [`TransferQuickInputParam`]; as chaves que o host de extensões
///   envia são variantes tipadas e o resto é [`TransferQuickInputParam::Other`] (que o `MainThreadQuickOpen` ignora, pois
///   o `input[param] = value` do original só pendura uma propriedade solta). `type` é o `kind`; `id` é o `id`.
/// - `IPickOptions<TransferQuickPickItem>` é o [`IPickOptions`] sem genérico; o retorno de `$show` (`number | number[] |
///   undefined`) é [`QuickOpenShowResult`] dentro de `Option`.
/// - O `$validateInput` devolve o [`InputValidationResult`] da plataforma (`string | { content, severity } | null | undefined`).
mod quick_open_region {
    use std::rc::Rc;

    use je_base::cancellation::CancellationToken;
    use je_base::dynamic::Dynamic;
    use je_base::errors::Error;
    use je_base::lifecycle::IDisposable;
    use je_base::promise::Promise;
    use je_base::severity::Severity;
    use je_base::themables::ThemeIcon;
    use je_base::uri::URI;
    use je_platform::quickinput::quick_input::{
        IPickOptions, IQuickPickSeparator, InputValidationResult, QuickInputButtonLocation, QuickInputButtonToggle, QuickInputIconPath,
    };

    use super::{ExtHostContext, MainContext, ProxyIdentifier};

    /// `IconPathDto`.
    #[derive(Clone, Debug)]
    pub enum IconPathDto {
        Uri(URI),
        Themed { light: URI, dark: URI },
        ThemeIcon(ThemeIcon),
    }

    /// `TransferQuickInputButton`.
    #[derive(Clone, Debug)]
    pub struct TransferQuickInputButton {
        pub handle: i64,
        pub icon_path_dto: IconPathDto,
        pub toggle: Option<Rc<QuickInputButtonToggle>>,
        pub tooltip: Option<String>,
        pub always_visible: Option<bool>,
        pub location: Option<QuickInputButtonLocation>,
        pub label: Option<String>,
        pub secondary: Option<bool>,
        pub icon_path: Option<QuickInputIconPath>,
        pub icon_class: Option<String>,
    }

    impl TransferQuickInputButton {
        /// O botão só com o `handle` e o ícone, o resto ausente.
        pub fn new(handle: i64, icon_path_dto: IconPathDto) -> Self {
            Self {
                handle,
                icon_path_dto,
                toggle: None,
                tooltip: None,
                always_visible: None,
                location: None,
                label: None,
                secondary: None,
                icon_path: None,
                icon_class: None,
            }
        }
    }

    /// `TransferQuickPickItem`.
    #[derive(Clone, Debug, Default)]
    pub struct TransferQuickPickItem {
        pub handle: u32,
        pub label: Option<String>,
        pub icon_path_dto: Option<IconPathDto>,
        pub description: Option<String>,
        pub detail: Option<String>,
        pub picked: Option<bool>,
        pub always_show: Option<bool>,
        pub buttons: Option<Vec<TransferQuickInputButton>>,
        pub resource_uri: Option<URI>,
        pub icon_path: Option<QuickInputIconPath>,
        pub icon_class: Option<String>,
        /// A propriedade `iconClasses` que o `MainThreadQuickOpen` define (um getter preguiçoso no original).
        pub icon_classes: Option<Vec<String>>,
    }

    /// `TransferQuickPickItemOrSeparator`.
    #[derive(Clone, Debug)]
    pub enum TransferQuickPickItemOrSeparator {
        Item(TransferQuickPickItem),
        Separator(IQuickPickSeparator),
    }

    /// O `type?: 'quickPick' | 'inputBox'` de `BaseTransferQuickInput`.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum TransferQuickInputKind {
        QuickPick,
        InputBox,
    }

    /// Uma chave de `TransferQuickInput` com o valor dela (`null` do original é `None`).
    #[derive(Clone, Debug)]
    pub enum TransferQuickInputParam {
        Title(Option<String>),
        Step(Option<u32>),
        TotalSteps(Option<u32>),
        Enabled(bool),
        Busy(bool),
        Visible(bool),
        IgnoreFocusOut(bool),
        Value(String),
        ValueSelection(Option<(u32, u32)>),
        Placeholder(Option<String>),
        Prompt(Option<String>),
        Password(bool),
        ValidationMessage(Option<String>),
        Severity(Severity),
        Buttons(Vec<TransferQuickInputButton>),
        Items(Option<Vec<TransferQuickPickItemOrSeparator>>),
        ActiveItems(Vec<u32>),
        SelectedItems(Vec<u32>),
        CanSelectMany(bool),
        MatchOnDescription(bool),
        MatchOnDetail(bool),
        SortByLabel(bool),
        KeepScrollPosition(bool),
        /// Qualquer outra chave do `[key: string]: any`.
        Other(String, Dynamic),
    }

    /// `TransferQuickInput` (`TransferQuickPick | TransferInputBox`).
    #[derive(Clone, Debug)]
    pub struct TransferQuickInput {
        pub id: u32,
        pub kind: Option<TransferQuickInputKind>,
        /// Os pares na ordem de inserção (sem `id` e `type`).
        pub params: Vec<TransferQuickInputParam>,
    }

    /// `IInputBoxOptions`.
    #[derive(Clone, Debug, Default)]
    pub struct IInputBoxOptions {
        pub title: Option<String>,
        pub value: Option<String>,
        pub value_selection: Option<(u32, u32)>,
        pub prompt: Option<String>,
        pub place_holder: Option<String>,
        pub password: Option<bool>,
        pub ignore_focus_out: Option<bool>,
    }

    /// O `number | number[]` que `$show` devolve.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub enum QuickOpenShowResult {
        One(u32),
        Many(Vec<u32>),
    }

    /// `MainThreadQuickOpenShape`.
    pub trait MainThreadQuickOpenShape: IDisposable {
        fn rpc_show(&self, instance: u32, options: IPickOptions, token: CancellationToken) -> Promise<Option<QuickOpenShowResult>>;
        fn rpc_set_items(&self, instance: u32, items: Vec<TransferQuickPickItemOrSeparator>) -> Promise<()>;
        fn rpc_set_error(&self, instance: u32, error: Error) -> Promise<()>;
        fn rpc_input(&self, options: Option<IInputBoxOptions>, validate_input: bool, token: CancellationToken) -> Promise<Option<String>>;
        fn rpc_create_or_update(&self, params: TransferQuickInput) -> Promise<()>;
        fn rpc_dispose(&self, id: u32) -> Promise<()>;
    }

    /// `ExtHostQuickOpenShape`.
    pub trait ExtHostQuickOpenShape {
        fn rpc_on_item_selected(&self, handle: u32);
        fn rpc_validate_input(&self, input: &str) -> Promise<InputValidationResult>;
        fn rpc_on_did_change_active(&self, session_id: u32, handles: Vec<u32>);
        fn rpc_on_did_change_selection(&self, session_id: u32, handles: Vec<u32>);
        fn rpc_on_did_accept(&self, session_id: u32);
        fn rpc_on_did_change_value(&self, session_id: u32, value: &str);
        fn rpc_on_did_trigger_button(&self, session_id: u32, handle: Option<i64>, checked: Option<bool>);
        fn rpc_on_did_trigger_item_button(&self, session_id: u32, item_handle: Option<u32>, button_handle: Option<i64>, checked: Option<bool>);
        fn rpc_on_did_hide(&self, session_id: u32);
    }

    impl MainContext {
        pub const MAIN_THREAD_QUICK_OPEN: ProxyIdentifier<dyn MainThreadQuickOpenShape> = ProxyIdentifier::new("MainThreadQuickOpen");
    }

    impl ExtHostContext {
        pub const EXT_HOST_QUICK_OPEN: ProxyIdentifier<dyn ExtHostQuickOpenShape> = ProxyIdentifier::new("ExtHostQuickOpen");
    }
}

// #endregion

// #region EditorTabs

pub use self::editor_tabs_region::*;

/// Região de `extHost.protocol.ts` que `extHostEditorTabs.ts` usa: `TabInputKind`, `TabModelOperationKind`, os DTOs de
/// entrada de aba (`AnyInputDto` e seus membros), `IEditorTabDto`, `IEditorTabGroupDto`, `TabOperation`,
/// `MainThreadEditorTabsShape`, `IExtHostEditorTabsShape` e os identificadores `MainContext.MainThreadEditorTabs` e
/// `ExtHostContext.ExtHostEditorTabs`.
///
/// Divergências:
/// - Os `UriComponents` são [`URI`] (já revividos); `AnyInputDto` é um enum com um struct por membro do original
///   (`UnknownInputDto` é a variante [`AnyInputDto::Unknown`] e o `TabInputDto` do terminal, [`AnyInputDto::Terminal`]);
///   [`AnyInputDto::kind`] devolve o `kind` do original.
/// - `groupId` é `u32` e os índices de `TabOperation` são `usize`; `viewColumn` é [`EditorGroupColumn`].
/// - `ExtHostEditorTabsShape` é o `IExtHostEditorTabsShape`; os métodos síncronos que lançam erro (`throw`) levantam
///   pânico com o [`Error`](je_base::errors::Error), como o resto do porte.
mod editor_tabs_region {
    use je_base::lifecycle::IDisposable;
    use je_base::promise::Promise;
    use je_base::uri::URI;

    use super::{EditorGroupColumn, ExtHostContext, MainContext, ProxyIdentifier};

    /// `TabInputKind`.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum TabInputKind {
        UnknownInput,
        TextInput,
        TextDiffInput,
        TextMergeInput,
        NotebookInput,
        NotebookDiffInput,
        CustomEditorInput,
        WebviewEditorInput,
        TerminalEditorInput,
        InteractiveEditorInput,
        ChatEditorInput,
        MultiDiffEditorInput,
    }

    /// `TabModelOperationKind`.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum TabModelOperationKind {
        TabOpen,
        TabClose,
        TabUpdate,
        TabMove,
    }

    /// `TextInputDto`.
    #[derive(Clone, Debug)]
    pub struct TextInputDto {
        pub uri: URI,
    }

    /// `TextDiffInputDto`.
    #[derive(Clone, Debug)]
    pub struct TextDiffInputDto {
        pub original: URI,
        pub modified: URI,
    }

    /// `TextMergeInputDto`.
    #[derive(Clone, Debug)]
    pub struct TextMergeInputDto {
        pub base: URI,
        pub input1: URI,
        pub input2: URI,
        pub result: URI,
    }

    /// `NotebookInputDto`.
    #[derive(Clone, Debug)]
    pub struct NotebookInputDto {
        pub notebook_type: String,
        pub uri: URI,
    }

    /// `NotebookDiffInputDto`.
    #[derive(Clone, Debug)]
    pub struct NotebookDiffInputDto {
        pub notebook_type: String,
        pub original: URI,
        pub modified: URI,
    }

    /// `CustomInputDto`.
    #[derive(Clone, Debug)]
    pub struct CustomInputDto {
        pub view_type: String,
        pub uri: URI,
    }

    /// `WebviewInputDto`.
    #[derive(Clone, Debug)]
    pub struct WebviewInputDto {
        pub view_type: String,
    }

    /// `InteractiveEditorInputDto`.
    #[derive(Clone, Debug)]
    pub struct InteractiveEditorInputDto {
        pub uri: URI,
        pub input_box_uri: URI,
    }

    /// `MultiDiffEditorInputDto`.
    #[derive(Clone, Debug)]
    pub struct MultiDiffEditorInputDto {
        pub diff_editors: Vec<TextDiffInputDto>,
    }

    /// `AnyInputDto`.
    #[derive(Clone, Debug)]
    pub enum AnyInputDto {
        Unknown,
        Text(TextInputDto),
        TextDiff(TextDiffInputDto),
        MultiDiff(MultiDiffEditorInputDto),
        TextMerge(TextMergeInputDto),
        Notebook(NotebookInputDto),
        NotebookDiff(NotebookDiffInputDto),
        Custom(CustomInputDto),
        Webview(WebviewInputDto),
        Interactive(InteractiveEditorInputDto),
        Chat,
        Terminal,
    }

    impl AnyInputDto {
        /// O `kind` do DTO.
        pub fn kind(&self) -> TabInputKind {
            match self {
                Self::Unknown => TabInputKind::UnknownInput,
                Self::Text(_) => TabInputKind::TextInput,
                Self::TextDiff(_) => TabInputKind::TextDiffInput,
                Self::MultiDiff(_) => TabInputKind::MultiDiffEditorInput,
                Self::TextMerge(_) => TabInputKind::TextMergeInput,
                Self::Notebook(_) => TabInputKind::NotebookInput,
                Self::NotebookDiff(_) => TabInputKind::NotebookDiffInput,
                Self::Custom(_) => TabInputKind::CustomEditorInput,
                Self::Webview(_) => TabInputKind::WebviewEditorInput,
                Self::Interactive(_) => TabInputKind::InteractiveEditorInput,
                Self::Chat => TabInputKind::ChatEditorInput,
                Self::Terminal => TabInputKind::TerminalEditorInput,
            }
        }
    }

    /// `IEditorTabDto`.
    #[derive(Clone, Debug)]
    pub struct EditorTabDto {
        pub id: String,
        pub label: String,
        pub input: AnyInputDto,
        pub editor_id: Option<String>,
        pub is_active: bool,
        pub is_pinned: bool,
        pub is_preview: bool,
        pub is_dirty: bool,
    }

    /// `IEditorTabGroupDto`.
    #[derive(Clone, Debug)]
    pub struct EditorTabGroupDto {
        pub is_active: bool,
        pub view_column: EditorGroupColumn,
        pub tabs: Vec<EditorTabDto>,
        pub group_id: u32,
    }

    /// `TabOperation`.
    #[derive(Clone, Debug)]
    pub struct TabOperation {
        pub kind: TabModelOperationKind,
        pub index: usize,
        pub tab_dto: EditorTabDto,
        pub group_id: u32,
        pub old_index: Option<usize>,
    }

    /// `MainThreadEditorTabsShape`.
    pub trait MainThreadEditorTabsShape: IDisposable {
        fn rpc_move_tab(&self, tab_id: &str, index: usize, view_column: EditorGroupColumn, preserve_focus: Option<bool>);
        fn rpc_close_tab(&self, tab_ids: Vec<String>, preserve_focus: Option<bool>) -> Promise<bool>;
        fn rpc_close_group(&self, group_ids: Vec<u32>, preserve_focus: Option<bool>) -> Promise<bool>;
    }

    /// `IExtHostEditorTabsShape`.
    pub trait ExtHostEditorTabsShape {
        /// Aceita um modelo novo inteiro.
        fn rpc_accept_editor_tab_model(&self, tab_groups: Vec<EditorTabGroupDto>);
        /// Só quando uma propriedade do grupo muda (não as abas dentro dele).
        fn rpc_accept_tab_group_update(&self, group_dto: EditorTabGroupDto);
        /// Quando uma aba é adicionada, removida ou atualizada.
        fn rpc_accept_tab_operation(&self, operation: TabOperation);
    }

    impl MainContext {
        pub const MAIN_THREAD_EDITOR_TABS: ProxyIdentifier<dyn MainThreadEditorTabsShape> = ProxyIdentifier::new("MainThreadEditorTabs");
    }

    impl ExtHostContext {
        pub const EXT_HOST_EDITOR_TABS: ProxyIdentifier<dyn ExtHostEditorTabsShape> = ProxyIdentifier::new("ExtHostEditorTabs");
    }
}

// #endregion

// #region Diagnostics

pub use self::diagnostics_region::*;

/// `IMarkerChange`, `MainThreadDiagnosticsShape`, `ExtHostDiagnosticsShape` e os identificadores
/// `MainContext.MainThreadDiagnostics` e `ExtHostContext.ExtHostDiagnostics` (de `extHost.protocol.ts`, usados por
/// `extHostDiagnostics.ts`).
///
/// Divergências:
/// - `IMarkerChange` é uma tupla `[uri, markers | undefined, isTruncated?]` no original: aqui é um struct com
///   os três campos, `markers` ausente (`None`) sendo o `undefined` que `delete` envia. Os `UriComponents` são [`URI`] e
///   o `IMarkerData` é o [`MarkerData`] da plataforma.
/// - `$acceptMarkersChange(data: [UriComponents, IMarkerData[]][])` recebe uma lista de pares.
mod diagnostics_region {
    use je_base::lifecycle::IDisposable;
    use je_base::uri::URI;
    use je_platform::markers::markers::MarkerData;

    use super::ProxyIdentifier;

    /// `IMarkerChange`.
    #[derive(Clone, Debug)]
    pub struct IMarkerChange {
        pub resource: URI,
        pub markers: Option<Vec<MarkerData>>,
        pub is_truncated: Option<bool>,
    }

    /// `MainThreadDiagnosticsShape`.
    pub trait MainThreadDiagnosticsShape: IDisposable {
        fn rpc_change_many(&self, owner: &str, entries: Vec<IMarkerChange>);
        fn rpc_clear(&self, owner: &str);
    }

    /// `ExtHostDiagnosticsShape`.
    pub trait ExtHostDiagnosticsShape {
        fn rpc_accept_markers_change(&self, data: Vec<(URI, Vec<MarkerData>)>);
    }

    impl super::MainContext {
        pub const MAIN_THREAD_DIAGNOSTICS: ProxyIdentifier<dyn MainThreadDiagnosticsShape> = ProxyIdentifier::new("MainThreadDiagnostics");
    }

    impl super::ExtHostContext {
        pub const EXT_HOST_DIAGNOSTICS: ProxyIdentifier<dyn ExtHostDiagnosticsShape> = ProxyIdentifier::new("ExtHostDiagnostics");
    }
}

// #endregion

// #region ExtHostTelemetry

pub use self::ext_host_telemetry_region::*;

/// `ExtHostTelemetryShape` e `ExtHostContext.ExtHostTelemetry` (de `extHost.protocol.ts`, usados por
/// `extHostTelemetry.ts`).
///
/// Divergências: o `TelemetryLevel` é o da plataforma; o `productConfig?: { usage, error }` de
/// `$initializeTelemetryLevel` é [`TelemetryProductConfig`] dentro de `Option`.
mod ext_host_telemetry_region {
    use je_platform::telemetry::TelemetryLevel;

    use super::{ExtHostContext, ProxyIdentifier};

    /// O `{ usage: boolean; error: boolean }` de `productConfig`.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct TelemetryProductConfig {
        pub usage: bool,
        pub error: bool,
    }

    /// `ExtHostTelemetryShape`.
    pub trait ExtHostTelemetryShape {
        fn rpc_initialize_telemetry_level(&self, level: TelemetryLevel, supports_telemetry: bool, product_config: Option<TelemetryProductConfig>);
        fn rpc_on_did_change_telemetry_level(&self, level: TelemetryLevel);
    }

    impl ExtHostContext {
        pub const EXT_HOST_TELEMETRY: ProxyIdentifier<dyn ExtHostTelemetryShape> = ProxyIdentifier::new("ExtHostTelemetry");
    }
}

// #endregion

// #region StatusBar

pub use self::status_bar_region::*;

/// `MainThreadStatusBarShape`, `ExtHostStatusBarShape`, `StatusBarItemDto` e os identificadores
/// `MainContext.MainThreadStatusBar` e `ExtHostContext.ExtHostStatusBar` (de `extHost.protocol.ts`, usados por
/// `extHostStatusBar.ts` e `mainThreadStatusBar.ts`).
///
/// Divergências:
/// - `color: string | ThemeColor` é [`StatusBarColor`]. O `backgroundColor` do `$setEntry` (`string | ThemeColor` no
///   protocolo, sempre `ThemeColor` no host de extensões e no `MainThreadStatusBar`) é `Option<ThemeColor>`.
/// - `tooltip: IMarkdownString | string | undefined` é `Option<StringOrMarkdown>`; o `StatusBarItemDto.tooltip` é só
///   `string`. `$provideTooltip` devolve `Option<StringOrMarkdown>`.
/// - `priority` é `Option<f64>` (o host troca `Infinity` por `Number.MAX_VALUE` porque o JSON não o carrega).
mod status_bar_region {
    use je_base::cancellation::CancellationToken;
    use je_base::lifecycle::IDisposable;
    use je_base::promise::Promise;
    use je_base::themables::ThemeColor;
    use je_editor::languages::StringOrMarkdown;
    use je_platform::accessibility::IAccessibilityInformation;

    use super::{ExtHostContext, ICommandDto, MainContext, ProxyIdentifier};

    /// `string | ThemeColor`.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub enum StatusBarColor {
        String(String),
        ThemeColor(ThemeColor),
    }

    /// `StatusBarItemDto`.
    #[derive(Clone, Debug, PartialEq)]
    pub struct StatusBarItemDto {
        pub entry_id: String,
        pub align_left: bool,
        pub priority: Option<f64>,
        pub name: String,
        pub text: String,
        pub tooltip: Option<String>,
        pub command: Option<String>,
        pub accessibility_information: Option<IAccessibilityInformation>,
    }

    /// `MainThreadStatusBarShape`.
    pub trait MainThreadStatusBarShape: IDisposable {
        #[allow(clippy::too_many_arguments)]
        fn rpc_set_entry(
            &self,
            id: &str,
            status_id: &str,
            extension_id: Option<&str>,
            status_name: &str,
            text: &str,
            tooltip: Option<StringOrMarkdown>,
            has_tooltip_provider: bool,
            command: Option<ICommandDto>,
            color: Option<StatusBarColor>,
            background_color: Option<ThemeColor>,
            align_left: bool,
            priority: Option<f64>,
            accessibility_information: Option<IAccessibilityInformation>,
        );
        fn rpc_dispose_entry(&self, id: &str);
    }

    /// `ExtHostStatusBarShape`.
    pub trait ExtHostStatusBarShape {
        fn rpc_accept_static_entries(&self, added: Vec<StatusBarItemDto>);
        fn rpc_provide_tooltip(&self, entry_id: &str, cancellation: CancellationToken) -> Promise<Option<StringOrMarkdown>>;
    }

    impl MainContext {
        pub const MAIN_THREAD_STATUS_BAR: ProxyIdentifier<dyn MainThreadStatusBarShape> = ProxyIdentifier::new("MainThreadStatusBar");
    }

    impl ExtHostContext {
        pub const EXT_HOST_STATUS_BAR: ProxyIdentifier<dyn ExtHostStatusBarShape> = ProxyIdentifier::new("ExtHostStatusBar");
    }
}

// #endregion

// #region Notebook (DTOs de dados dos conversores)

pub use self::notebook_data_dto_region::*;

/// `NotebookOutputItemDto`, `NotebookOutputDto`, `NotebookCellDataDto` e `NotebookDataDto` (de `extHost.protocol.ts`,
/// usados por `NotebookData`, `NotebookCellData` e `NotebookCellOutput` de `extHostTypeConverters.ts`).
///
/// Divergências:
/// - `metadata?: Record<string, any>` e os metadados do notebook são [`Dynamic`] (o `NotebookCellMetadata` e o
///   `NotebookDocumentMetadata` de `notebook_common`). O `valueBytes: VSBuffer` é o [`VSBuffer`].
/// - `NotebookDataDto.metadata` é obrigatório no original (`Object.create(null)` quando a extensão não tem): é um
///   [`Dynamic`] objeto, vazio nesse caso.
mod notebook_data_dto_region {
    use je_base::buffer::VSBuffer;
    use je_base::dynamic::Dynamic;

    use crate::contrib::notebook::common::notebook_common::{CellKind, NotebookCellInternalMetadata, NotebookCellMetadata, NotebookDocumentMetadata};

    /// `NotebookOutputItemDto`.
    #[derive(Clone, Debug)]
    pub struct NotebookOutputItemDto {
        pub mime: String,
        pub value_bytes: VSBuffer,
    }

    /// `NotebookOutputDto`.
    #[derive(Clone, Debug)]
    pub struct NotebookOutputDto {
        pub items: Vec<NotebookOutputItemDto>,
        pub output_id: String,
        pub metadata: Option<Dynamic>,
    }

    /// `NotebookCellDataDto`.
    #[derive(Clone, Debug)]
    pub struct NotebookCellDataDto {
        pub source: String,
        pub language: String,
        pub mime: Option<String>,
        pub cell_kind: CellKind,
        pub outputs: Vec<NotebookOutputDto>,
        pub metadata: Option<NotebookCellMetadata>,
        pub internal_metadata: Option<NotebookCellInternalMetadata>,
    }

    /// `NotebookDataDto`.
    #[derive(Clone, Debug)]
    pub struct NotebookDataDto {
        pub cells: Vec<NotebookCellDataDto>,
        pub metadata: NotebookDocumentMetadata,
    }
}

// #endregion

// #region Timeline

pub use self::timeline_region::*;

/// `MainThreadTimelineShape`, `ExtHostTimelineShape` e os identificadores `MainContext.MainThreadTimeline` e
/// `ExtHostContext.ExtHostTimeline` (de `extHost.protocol.ts`, usados por `extHostTimeline.ts` e
/// `mainThreadTimeline.ts`).
///
/// Divergências:
/// - Os `UriComponents` são [`URI`] (os dois lados dividem o processo e o `revive` é a identidade), e o
///   `Promise<Dto<Timeline> | undefined>` de `$getTimeline` é `Promise<Option<Timeline>>`.
mod timeline_region {
    use je_base::cancellation::CancellationToken;
    use je_base::lifecycle::IDisposable;
    use je_base::promise::Promise;
    use je_base::uri::URI;

    use super::{ExtHostContext, MainContext, ProxyIdentifier};
    use crate::contrib::timeline::common::timeline::{Timeline, TimelineOptions, TimelineProviderDescriptor, WorkspaceTimelineProviderDescriptor};

    /// `MainThreadTimelineShape`.
    pub trait MainThreadTimelineShape: IDisposable {
        fn rpc_register_timeline_provider(&self, provider: TimelineProviderDescriptor);
        fn rpc_unregister_timeline_provider(&self, source: &str);
        fn rpc_register_workspace_timeline_provider(&self, provider: WorkspaceTimelineProviderDescriptor);
        fn rpc_unregister_workspace_timeline_provider(&self, source: &str);

        /// `uris`: the files or folders whose timelines changed, or `None` if all of them changed.
        fn rpc_emit_timeline_change_event(&self, source: &str, uris: Option<Vec<URI>>);
        fn rpc_emit_workspace_timeline_change_event(&self, source: &str);
    }

    /// `ExtHostTimelineShape`.
    pub trait ExtHostTimelineShape {
        /// `uri`: the file or folder to get the timeline for, or `None` for the workspace timeline.
        fn rpc_get_timeline(&self, source: &str, uri: Option<URI>, options: TimelineOptions, token: CancellationToken) -> Promise<Option<Timeline>>;
    }

    impl MainContext {
        pub const MAIN_THREAD_TIMELINE: ProxyIdentifier<dyn MainThreadTimelineShape> = ProxyIdentifier::new("MainThreadTimeline");
    }

    impl ExtHostContext {
        pub const EXT_HOST_TIMELINE: ProxyIdentifier<dyn ExtHostTimelineShape> = ProxyIdentifier::new("ExtHostTimeline");
    }
}

// #endregion

// #region Testing

pub use self::testing_region::*;

/// `ITestControllerPatch`, `MainThreadTestingShape`, `ExtHostTestingShape` e os identificadores
/// `MainContext.MainThreadTesting` e `ExtHostContext.ExtHostTesting` (de `extHost.protocol.ts`, usados por
/// `extHostTesting.ts`).
///
/// Divergências:
/// - `Partial<ITestRunProfile>` é o [`PartialTestRunProfile`] e `TestControllerCapability` é o `u32` de bits do
///   `testTypes`. `Record<string, number[]>` de `$setDefaultRunProfiles` é uma lista de pares na ordem de inserção.
/// - As formas `Serialized` do original são os tipos `*Serialized` do `testTypes` (`TestsDiffOpSerialized`,
///   `ITestItemSerialized`, `ITestMessageSerialized`, `IFileCoverageSerialized`). `CoverageDetails.Serialized` é o
///   mesmo dado de [`CoverageDetails`], e `ILocationDto` é o [`LocationDto`]. Os `UriComponents` são [`URI`].
/// - `$markTestRetired(testIds: string[] | undefined)` é `Option<Vec<String>>`; `$appendOutputToRun` e os
///   `$runTests` seguem o `rpc_` do resto do protocolo.
mod testing_region {
    use je_base::buffer::VSBuffer;
    use je_base::cancellation::CancellationToken;
    use je_base::promise::Promise;
    use je_base::uri::URI;
    use je_editor::position::Position;

    use super::{ExtHostContext, MainContext, ProxyIdentifier};
    use crate::api::common::ext_host_type_converters::LocationDto;
    use crate::contrib::testing::common::test_types::{
        CoverageDetails, ExtensionRunTestsRequest, ICallProfileRunHandler, IFileCoverageSerialized, IStartControllerTests, IStartControllerTestsResult,
        ISerializedTestResults, ITestItemSerialized, ITestMessageSerialized, ITestRunProfile, ITestRunTask, PartialTestRunProfile,
        ResolvedTestRunRequest, TestControllerCapability, TestMessageFollowupRequest, TestMessageFollowupResponse, TestResultState, TestsDiffOpSerialized,
    };

    /// `ITestControllerPatch`.
    #[derive(Clone, Debug, Default, PartialEq)]
    pub struct ITestControllerPatch {
        pub label: Option<String>,
        pub capabilities: Option<TestControllerCapability>,
    }

    /// `MainThreadTestingShape`.
    pub trait MainThreadTestingShape {
        // --- test lifecycle:

        /// Registers that there's a test controller with the given ID
        fn rpc_register_test_controller(&self, controller_id: &str, label: &str, capability: TestControllerCapability);
        /// Updates the label of an existing test controller.
        fn rpc_update_controller(&self, controller_id: &str, patch: ITestControllerPatch);
        /// Diposes of the test controller with the given ID
        fn rpc_unregister_test_controller(&self, controller_id: &str);
        /// Requests tests published to VS Code.
        fn rpc_subscribe_to_diffs(&self);
        /// Stops requesting tests published to VS Code.
        fn rpc_unsubscribe_from_diffs(&self);
        /// Publishes that new tests were available on the given source.
        fn rpc_publish_diff(&self, controller_id: &str, diff: Vec<TestsDiffOpSerialized>);
        /// Gets coverage details from a test result.
        fn rpc_get_coverage_details(&self, result_id: &str, task_index: usize, uri: &URI, token: CancellationToken) -> Promise<Vec<CoverageDetails>>;

        // --- test run configurations:

        /// Called when a new test run configuration is available
        fn rpc_publish_test_run_profile(&self, config: ITestRunProfile);
        /// Updates an existing test run configuration
        fn rpc_update_test_run_config(&self, controller_id: &str, config_id: u32, update: PartialTestRunProfile);
        /// Removes a previously-published test run config
        fn rpc_remove_test_profile(&self, controller_id: &str, config_id: u32);

        // --- test run handling:

        /// Request by an extension to run tests.
        fn rpc_run_tests(&self, req: ResolvedTestRunRequest, token: CancellationToken) -> Promise<String>;
        /// Adds tests to the run. The tests are given in descending depth. The first
        /// item will be a previously-known test, or a test root.
        fn rpc_add_tests_to_run(&self, controller_id: &str, run_id: &str, tests: Vec<ITestItemSerialized>);
        /// Updates the state of a test run in the given run.
        fn rpc_update_test_state_in_run(&self, run_id: &str, task_id: &str, test_id: &str, state: TestResultState, duration: Option<f64>);
        /// Appends a message to a test in the run.
        fn rpc_append_test_messages_in_run(&self, run_id: &str, task_id: &str, test_id: &str, messages: Vec<ITestMessageSerialized>);
        /// Appends raw output to the test run..
        fn rpc_append_output_to_run(&self, run_id: &str, task_id: &str, output: VSBuffer, location: Option<LocationDto>, test_id: Option<String>);
        /// Triggered when coverage is added to test results.
        fn rpc_append_coverage(&self, run_id: &str, task_id: &str, coverage: IFileCoverageSerialized);
        /// Signals a task in a test run started.
        fn rpc_started_test_run_task(&self, run_id: &str, task: ITestRunTask);
        /// Signals a task in a test run ended.
        fn rpc_finished_test_run_task(&self, run_id: &str, task_id: &str);
        /// Start a new extension-provided test run.
        fn rpc_started_extension_test_run(&self, req: ExtensionRunTestsRequest);
        /// Signals that an extension-provided test run finished.
        fn rpc_finished_extension_test_run(&self, run_id: &str);
        /// Marks a test (or controller) as retired in all results.
        fn rpc_mark_test_retired(&self, test_ids: Option<Vec<String>>);
    }

    /// `ExtHostTestingShape`.
    pub trait ExtHostTestingShape {
        fn rpc_run_controller_tests(&self, req: Vec<IStartControllerTests>, token: CancellationToken) -> Promise<Vec<IStartControllerTestsResult>>;
        fn rpc_start_continuous_run(&self, req: Vec<ICallProfileRunHandler>, token: CancellationToken) -> Promise<Vec<IStartControllerTestsResult>>;
        fn rpc_cancel_extension_test_run(&self, run_id: Option<&str>, task_id: Option<&str>);
        /// Handles a diff of tests, as a result of a subscribeToDiffs() call
        fn rpc_accept_diff(&self, diff: Vec<TestsDiffOpSerialized>);
        /// Expands a test item's children, by the given number of levels.
        fn rpc_expand_test(&self, test_id: &str, levels: f64) -> Promise<()>;
        /// Requests coverage details for a test run. Errors if not available.
        fn rpc_get_coverage_details(&self, coverage_id: &str, test_id: Option<&str>, token: CancellationToken) -> Promise<Vec<CoverageDetails>>;
        /// Disposes resources associated with a test run.
        fn rpc_dispose_run(&self, run_id: &str);
        /// Configures a test run config.
        fn rpc_configure_run_profile(&self, controller_id: &str, config_id: u32);
        /// Asks the controller to refresh its tests
        fn rpc_refresh_tests(&self, controller_id: &str, token: CancellationToken) -> Promise<()>;
        /// Ensures any pending test diffs are flushed
        fn rpc_sync_tests(&self) -> Promise<()>;
        /// Sets the active test run profiles
        fn rpc_set_default_run_profiles(&self, profiles: Vec<(String, Vec<u32>)>);
        fn rpc_get_tests_related_to_code(&self, uri: &URI, position: Position, token: CancellationToken) -> Promise<Vec<String>>;
        fn rpc_get_code_related_to_test(&self, test_id: &str, token: CancellationToken) -> Promise<Vec<LocationDto>>;

        // --- test results:

        /// Publishes that a test run finished.
        fn rpc_publish_test_results(&self, results: Vec<ISerializedTestResults>);
        /// Requests followup actions for a test (failure) message
        fn rpc_provide_test_followups(&self, req: TestMessageFollowupRequest, token: CancellationToken) -> Promise<Vec<TestMessageFollowupResponse>>;
        /// Actions a followup actions for a test (failure) message
        fn rpc_execute_test_followup(&self, id: u32) -> Promise<()>;
        /// Disposes followup actions for a test (failure) message
        fn rpc_dispose_test_followups(&self, id: Vec<u32>);
    }

    impl MainContext {
        pub const MAIN_THREAD_TESTING: ProxyIdentifier<dyn MainThreadTestingShape> = ProxyIdentifier::new("MainThreadTesting");
    }

    impl ExtHostContext {
        pub const EXT_HOST_TESTING: ProxyIdentifier<dyn ExtHostTestingShape> = ProxyIdentifier::new("ExtHostTesting");
    }
}

// #endregion

// #region SCM

pub use self::scm_region::*;

/// Região de `extHost.protocol.ts` que `extHostSCM.ts` usa: `SCMProviderFeatures`, `SCMActionButtonDto`, `SCMGroupFeatures`,
/// `SCMRawResource` e as junções dele, os DTOs de histórico e de artefatos, `MainThreadSCMShape`, `ExtHostSCMShape` e os
/// identificadores `MainContext.MainThreadSCM` e `ExtHostContext.ExtHostSCM`.
///
/// Divergências:
/// - Os handles (`number`) são `u32` e os `UriComponents` são [`URI`]. O `IconPathDto` é o de `quick_open_region`
///   (`UriComponents | { light, dark } | ThemeIcon`), que tem a forma exata de `SCMHistoryItemRefDto.icon` e de
///   `SCMArtifactDto.icon`.
/// - `SCMProviderFeatures` é um struct com um `Option` por chave (a chave existe ou não, como o objeto parcial do original).
///   Onde a chave pode existir com o valor `undefined` (`count`, `commitTemplate`, `contextValue`, `quickDiffLabel`,
///   `secondaryQuickDiffLabel`, `acceptInputCommand`) ou com `null` (`actionButton`, `activeRepositoryName`), o campo é
///   `Option<Option<_>>`: `Some(None)` é a chave presente sem valor (`undefined`) e, nos dois campos que aceitam `null`,
///   é o `null`.
/// - As tuplas `SCMRawResource`, `SCMRawResourceSplice` e `SCMRawResourceSplices` são structs com os mesmos campos, na ordem
///   do original; as `icons: [light, dark]` são uma tupla. `UriComponents | ThemeIcon` é [`SCMIconDto`]. O `strikeThrough`
///   e o `faded` de um recurso sem `decorations` são `false` (o original manda `undefined`, que a thread principal lê como falso).
/// - `ICommandDto & { shortTitle?: string }` de `SCMActionButtonDto.command` é [`SCMActionButtonCommandDto`]. O `languages.Command`
///   de `acceptInputCommand` é o [`ICommandDto`] (o `toInternal` do conversor produz o `ident` junto).
/// - `InputValidationType` (de `contrib/scm/common/scm.ts`) e `ISCMHistoryOptions` (de `contrib/scm/common/history.ts`) ficam
///   aqui, junto de quem os cita. O `limit: number | { id?: string }` é [`SCMHistoryLimit`].
/// - O `tooltip: IMarkdownString | Array<IMarkdownString> | undefined` de `SCMHistoryItemDto` é [`SCMHistoryItemTooltipDto`].
/// - `$onInputBoxValueChange` devolve `void` no `ExtHostSCMShape` (o `ExtHostSCM` do original devolve uma promessa resolvida, que
///   ninguém lê): aqui devolve `()`. O `[string | IMarkdownString, number]` de `$validateInput` é uma tupla com o
///   [`StringOrMarkdown`] e o valor numérico do `InputValidationType`.
mod scm_region {
    use je_base::cancellation::CancellationToken;
    use je_base::html_content::IMarkdownString;
    use je_base::lifecycle::IDisposable;
    use je_base::promise::Promise;
    use je_base::themables::ThemeIcon;
    use je_base::uri::URI;
    use je_editor::languages::StringOrMarkdown;

    use super::{ExtHostContext, ICommandDto, IconPathDto, MainContext, ProxyIdentifier};

    /// `InputValidationType`.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum InputValidationType {
        Error = 0,
        Warning = 1,
        Information = 2,
    }

    /// `UriComponents | ThemeIcon`: um dos lados de `SCMRawResource.icons`.
    #[derive(Clone, Debug)]
    pub enum SCMIconDto {
        Uri(URI),
        ThemeIcon(ThemeIcon),
    }

    /// `SCMActionButtonDto.command`: `ICommandDto & { shortTitle?: string }`.
    #[derive(Clone, Debug)]
    pub struct SCMActionButtonCommandDto {
        pub command: ICommandDto,
        pub short_title: Option<String>,
    }

    /// `SCMActionButtonDto`.
    #[derive(Clone, Debug)]
    pub struct SCMActionButtonDto {
        pub command: SCMActionButtonCommandDto,
        pub secondary_commands: Option<Vec<Vec<ICommandDto>>>,
        pub enabled: bool,
    }

    /// `SCMProviderFeatures` (ver as divergências da região).
    #[derive(Clone, Debug, Default)]
    pub struct SCMProviderFeatures {
        pub has_artifact_provider: Option<bool>,
        pub has_history_provider: Option<bool>,
        pub has_quick_diff_provider: Option<bool>,
        pub quick_diff_label: Option<Option<String>>,
        pub has_secondary_quick_diff_provider: Option<bool>,
        pub secondary_quick_diff_label: Option<Option<String>>,
        pub count: Option<Option<f64>>,
        pub commit_template: Option<Option<String>>,
        pub accept_input_command: Option<Option<ICommandDto>>,
        pub action_button: Option<Option<SCMActionButtonDto>>,
        pub status_bar_commands: Option<Vec<ICommandDto>>,
        pub context_value: Option<Option<String>>,
        pub active_repository_name: Option<Option<String>>,
    }

    /// `SCMGroupFeatures`.
    #[derive(Clone, Debug, Default, PartialEq, Eq)]
    pub struct SCMGroupFeatures {
        pub hide_when_empty: Option<bool>,
        pub context_value: Option<String>,
    }

    /// `SCMRawResource`.
    #[derive(Clone, Debug)]
    pub struct SCMRawResource {
        pub handle: u32,
        pub resource_uri: URI,
        /// `[light, dark]`.
        pub icons: (Option<SCMIconDto>, Option<SCMIconDto>),
        pub tooltip: String,
        pub strike_through: bool,
        pub faded: bool,
        pub context_value: String,
        pub command: Option<ICommandDto>,
        pub multi_file_diff_editor_original_uri: Option<URI>,
        pub multi_file_diff_editor_modified_uri: Option<URI>,
    }

    /// `SCMRawResourceSplice`: `[start, deleteCount, resources]`.
    #[derive(Clone, Debug)]
    pub struct SCMRawResourceSplice {
        pub start: usize,
        pub delete_count: usize,
        pub to_insert: Vec<SCMRawResource>,
    }

    /// `SCMRawResourceSplices`: `[handle, splices]`.
    #[derive(Clone, Debug)]
    pub struct SCMRawResourceSplices {
        pub handle: u32,
        pub splices: Vec<SCMRawResourceSplice>,
    }

    /// Um grupo de `$registerGroups`: `[handle, id, label, features, multiDiffEditorEnableViewChanges]`.
    pub type SCMGroupRegistration = (u32, String, String, SCMGroupFeatures, bool);

    /// `SCMHistoryItemRefDto`.
    #[derive(Clone, Debug)]
    pub struct SCMHistoryItemRefDto {
        pub id: String,
        pub name: String,
        pub revision: Option<String>,
        pub category: Option<String>,
        pub description: Option<String>,
        pub icon: Option<IconPathDto>,
    }

    /// `SCMHistoryItemRefsChangeEventDto`.
    #[derive(Clone, Debug)]
    pub struct SCMHistoryItemRefsChangeEventDto {
        pub added: Vec<SCMHistoryItemRefDto>,
        pub modified: Vec<SCMHistoryItemRefDto>,
        pub removed: Vec<SCMHistoryItemRefDto>,
        pub silent: bool,
    }

    /// `SCMHistoryItemDto.statistics`.
    #[derive(Clone, Debug)]
    pub struct SCMHistoryItemStatisticsDto {
        pub files: f64,
        pub insertions: f64,
        pub deletions: f64,
    }

    /// `IMarkdownString | Array<IMarkdownString>`: o `tooltip` de um item de histórico.
    #[derive(Clone, Debug)]
    pub enum SCMHistoryItemTooltipDto {
        One(IMarkdownString),
        Many(Vec<IMarkdownString>),
    }

    /// `SCMHistoryItemDto`.
    #[derive(Clone, Debug)]
    pub struct SCMHistoryItemDto {
        pub id: String,
        pub parent_ids: Vec<String>,
        pub subject: String,
        pub message: String,
        pub display_id: Option<String>,
        pub author: Option<String>,
        pub author_icon: Option<IconPathDto>,
        pub author_email: Option<String>,
        pub timestamp: Option<f64>,
        pub statistics: Option<SCMHistoryItemStatisticsDto>,
        pub references: Option<Vec<SCMHistoryItemRefDto>>,
        pub tooltip: Option<SCMHistoryItemTooltipDto>,
    }

    /// `SCMHistoryItemChangeDto`.
    #[derive(Clone, Debug)]
    pub struct SCMHistoryItemChangeDto {
        pub uri: URI,
        pub original_uri: Option<URI>,
        pub modified_uri: Option<URI>,
    }

    /// `SCMArtifactGroupDto`.
    #[derive(Clone, Debug)]
    pub struct SCMArtifactGroupDto {
        pub id: String,
        pub name: String,
        pub icon: Option<IconPathDto>,
        pub supports_folders: Option<bool>,
    }

    /// `SCMArtifactDto`.
    #[derive(Clone, Debug)]
    pub struct SCMArtifactDto {
        pub id: String,
        pub name: String,
        pub description: Option<String>,
        pub icon: Option<IconPathDto>,
        pub timestamp: Option<f64>,
        pub command: Option<ICommandDto>,
    }

    /// `ISCMHistoryOptions.limit`: `number | { id?: string }`.
    #[derive(Clone, Debug, PartialEq)]
    pub enum SCMHistoryLimit {
        Count(f64),
        Id(Option<String>),
    }

    /// `ISCMHistoryOptions` (e `vscode.SourceControlHistoryOptions`, que tem a mesma forma).
    #[derive(Clone, Debug, Default, PartialEq)]
    pub struct ISCMHistoryOptions {
        pub skip: Option<f64>,
        pub limit: Option<SCMHistoryLimit>,
        pub history_item_refs: Option<Vec<String>>,
        pub filter_text: Option<String>,
    }

    /// `MainThreadSCMShape`.
    pub trait MainThreadSCMShape: IDisposable {
        #[allow(clippy::too_many_arguments)]
        fn rpc_register_source_control(
            &self,
            handle: u32,
            parent_handle: Option<u32>,
            id: &str,
            label: &str,
            root_uri: Option<URI>,
            icon_path: Option<IconPathDto>,
            is_hidden: Option<bool>,
            input_box_document_uri: URI,
        ) -> Promise<()>;
        fn rpc_update_source_control(&self, handle: u32, features: SCMProviderFeatures) -> Promise<()>;
        fn rpc_unregister_source_control(&self, handle: u32) -> Promise<()>;

        fn rpc_register_groups(&self, source_control_handle: u32, groups: Vec<SCMGroupRegistration>, splices: Vec<SCMRawResourceSplices>) -> Promise<()>;
        fn rpc_update_group(&self, source_control_handle: u32, handle: u32, features: SCMGroupFeatures) -> Promise<()>;
        fn rpc_update_group_label(&self, source_control_handle: u32, handle: u32, label: &str) -> Promise<()>;
        fn rpc_unregister_group(&self, source_control_handle: u32, handle: u32) -> Promise<()>;

        fn rpc_splice_resource_states(&self, source_control_handle: u32, splices: Vec<SCMRawResourceSplices>) -> Promise<()>;

        fn rpc_set_input_box_value(&self, source_control_handle: u32, value: &str) -> Promise<()>;
        fn rpc_set_input_box_placeholder(&self, source_control_handle: u32, placeholder: &str) -> Promise<()>;
        fn rpc_set_input_box_enablement(&self, source_control_handle: u32, enabled: bool) -> Promise<()>;
        fn rpc_set_input_box_visibility(&self, source_control_handle: u32, visible: bool) -> Promise<()>;
        fn rpc_show_validation_message(&self, source_control_handle: u32, message: StringOrMarkdown, validation_type: InputValidationType) -> Promise<()>;
        fn rpc_set_validation_provider_is_enabled(&self, source_control_handle: u32, enabled: bool) -> Promise<()>;

        fn rpc_on_did_change_history_provider_current_history_item_refs(
            &self,
            source_control_handle: u32,
            history_item_ref: Option<SCMHistoryItemRefDto>,
            history_item_remote_ref: Option<SCMHistoryItemRefDto>,
            history_item_base_ref: Option<SCMHistoryItemRefDto>,
        ) -> Promise<()>;
        fn rpc_on_did_change_history_provider_history_item_refs(&self, source_control_handle: u32, history_item_refs: SCMHistoryItemRefsChangeEventDto) -> Promise<()>;

        fn rpc_on_did_change_artifacts(&self, source_control_handle: u32, groups: Vec<String>) -> Promise<()>;
    }

    /// `ExtHostSCMShape`.
    pub trait ExtHostSCMShape {
        fn rpc_provide_original_resource(&self, source_control_handle: u32, uri: &URI, token: CancellationToken) -> Promise<Option<URI>>;
        fn rpc_provide_secondary_original_resource(&self, source_control_handle: u32, uri: &URI, token: CancellationToken) -> Promise<Option<URI>>;
        fn rpc_on_input_box_value_change(&self, source_control_handle: u32, value: &str);
        fn rpc_execute_resource_command(&self, source_control_handle: u32, group_handle: u32, handle: u32, preserve_focus: bool) -> Promise<()>;
        fn rpc_validate_input(&self, source_control_handle: u32, value: &str, cursor_position: f64) -> Promise<Option<(StringOrMarkdown, f64)>>;
        fn rpc_set_selected_source_control(&self, selected_source_control_handle: Option<u32>) -> Promise<()>;
        fn rpc_provide_history_item_refs(&self, source_control_handle: u32, history_item_refs: Option<Vec<String>>, token: CancellationToken) -> Promise<Option<Vec<SCMHistoryItemRefDto>>>;
        fn rpc_provide_history_items(&self, source_control_handle: u32, options: ISCMHistoryOptions, token: CancellationToken) -> Promise<Option<Vec<SCMHistoryItemDto>>>;
        fn rpc_provide_history_item_changes(
            &self,
            source_control_handle: u32,
            history_item_id: &str,
            history_item_parent_id: Option<&str>,
            token: CancellationToken,
        ) -> Promise<Option<Vec<SCMHistoryItemChangeDto>>>;
        fn rpc_resolve_history_item(&self, source_control_handle: u32, history_item_id: &str, token: CancellationToken) -> Promise<Option<SCMHistoryItemDto>>;
        fn rpc_resolve_history_item_chat_context(&self, source_control_handle: u32, history_item_id: &str, token: CancellationToken) -> Promise<Option<String>>;
        fn rpc_resolve_history_item_change_range_chat_context(
            &self,
            source_control_handle: u32,
            history_item_id: &str,
            history_item_parent_id: &str,
            path: &str,
            token: CancellationToken,
        ) -> Promise<Option<String>>;
        fn rpc_resolve_history_item_refs_common_ancestor(&self, source_control_handle: u32, history_item_refs: Vec<String>, token: CancellationToken) -> Promise<Option<String>>;

        fn rpc_provide_artifact_groups(&self, source_control_handle: u32, token: CancellationToken) -> Promise<Option<Vec<SCMArtifactGroupDto>>>;
        fn rpc_provide_artifacts(&self, source_control_handle: u32, group: &str, token: CancellationToken) -> Promise<Option<Vec<SCMArtifactDto>>>;
    }

    impl MainContext {
        pub const MAIN_THREAD_SCM: ProxyIdentifier<dyn MainThreadSCMShape> = ProxyIdentifier::new("MainThreadSCM");
    }

    impl ExtHostContext {
        pub const EXT_HOST_SCM: ProxyIdentifier<dyn ExtHostSCMShape> = ProxyIdentifier::new("ExtHostSCM");
    }
}

// #endregion

// #region LanguageFeatures

pub use self::language_features_region::*;

/// Região de `extHost.protocol.ts` que `extHostLanguageFeatures.ts` usa: `IDocumentFilterDto`, `ILocationDto`,
/// `ILocationLinkDto`, `HoverWithId`, `ICodeLensDto`, `ICodeLensListDto`, `MainThreadLanguageFeaturesShape`,
/// `ExtHostLanguageFeaturesShape` e os identificadores `MainContext.MainThreadLanguageFeatures` e
/// `ExtHostContext.ExtHostLanguageFeatures`.
///
/// PARTES 1 e 2 (somente o que os adaptadores já portados pedem): `DocumentSymbol`, `CodeLens`, `Definition`,
/// `Declaration`, `Implementation`, `TypeDefinition`, `Hover`, `DocumentHighlight`, `MultiDocumentHighlight`,
/// `Reference`, e (parte 2) expressão avaliável, valores embutidos, edição ligada, ação de código, colar, formatação
/// (documento, intervalo, ao digitar), símbolos do workspace, renomeação, novos nomes de símbolo, tokens semânticos,
/// completar, assinatura, dicas embutidas, links, cor, dobra, seleção, hierarquias de chamada e de tipo, e soltar.
/// PARTE 3: `$setLanguageConfiguration` (com `ILanguageConfigurationDto` e as regras), `$setWordDefinitions` e
/// `$acceptInlineCompletionsUnificationState`. Ainda pendentes: as completações em linha (`$registerInlineCompletionsSupport`,
/// `$provideInlineCompletions` e os `$handleInlineCompletion*`), que dependem de tipos do editor sem porte.
///
/// Divergências da parte 2:
/// - Os DTOs que o original tira de `languages.ts` e o editor ainda não tem são definidos aqui (`FormattingOptions`,
///   `InlineValueDto`, `EvaluatableExpressionDto`, `InlayHintLabelDto`...); os que o editor já tem são os dele
///   (`TextEdit` de `code_action::languages`, `CompletionContext`, `SignatureInformation`, `FoldingRange`,
///   `SelectionRange`, `SymbolKind`...). `InlayHintKind` e `NewSymbolNameTag` são os enums da API (mesmos valores).
/// - `ISuggestDataDto` e `ISuggestResultDto` são structs com campos nomeados no lugar das chaves de uma letra
///   (`ISuggestDataDtoField.label = 'a'`), que existem só para encurtar o JSON na rede; o `x` é `cache_id`.
/// - `IWorkspaceEditDto & { rejectReason?: string }` é [`RenameEditsDto`] e `RenameLocation & Rejection` é
///   [`RenameLocationDto`]: o objeto que o original monta sem o outro lado (`edits: undefined!`) tem o lado ausente `None`.
/// - `$provideCodeActions(rangeOrSelection: IRange | ISelection)` é o enum [`RangeOrSelection`].
/// - `CustomCodeAction._isSynthetic` é o campo `is_synthetic` de [`ICodeActionDto`] (o objeto do original o carrega junto).
/// - `$provideDocumentSemanticTokens` devolve `VSBuffer | null`: `Option<VSBuffer>`.
/// - `ICallHierarchyItemDto` e `ITypeHierarchyItemDto` (mesma forma) são o [`HierarchyItemDto`] dos conversores.
/// - `insertText: string | { snippet: string }` dos DTOs de colar e soltar é [`DropPasteInsertTextDto`].
///
/// Divergências:
/// - Os handles (`number`) são `u32` e os `UriComponents` são [`URI`] (os dois lados dividem o processo e o `revive` é a
///   identidade). O `IRange` e o `IPosition` são os tipos do editor.
/// - O `$serialized: true` de `IDocumentFilterDto` é sempre verdadeiro e não existe como campo. O
///   `pattern?: string | IRelativePattern` é o [`GlobPatternDto`] do conversor de `GlobPattern`.
/// - `ILocationDto` (`Dto<languages.Location>`) é o [`LocationDto`] dos conversores, de `uri` e `range` opcionais.
///   `ILocationLinkDto` (`Dto<languages.LocationLink>`) tem `uri` e `range` obrigatórios, como o `languages.LocationLink`.
/// - `languages.Hover` é [`HoverDto`] e `HoverWithId extends languages.Hover` é o [`HoverWithId`], com os mesmos campos mais
///   o `id`. `languages.HoverContext<{ id: number }>` é [`HoverContextDto`].
/// - `CachedSessionItem<Dto<languages.CodeLens>>` é [`ICodeLensDto`] (o `command` do `languages.CodeLens` já chega como
///   `ICommandDto` depois do `toInternal`) e `CachedSession<{ lenses }>` é [`ICodeLensListDto`]; `ChainedCacheId` é uma tupla.
/// - `Dto<languages.MultiDocumentHighlight[]>` é uma lista de [`MultiDocumentHighlightDto`].
/// - `ILanguageConfigurationDto`: `comments`, `brackets` e as ações de `IOnEnterRuleDto` são os [`CommentRule`], [`CharacterPair`] e
///   [`EnterAction`] do editor (mesmas formas); o `__electricCharacterSupport` e o `__characterPairSupport` ficam com os campos
///   que o original declara (`brackets?: any` é um [`Dynamic`]).
/// - `$emitCodeLensEvent(eventHandle, event?: any)` leva o evento como `Option<Dynamic>`.
mod language_features_region {
    use je_base::buffer::VSBuffer;
    use je_base::cancellation::CancellationToken;
    use je_base::dynamic::Dynamic;
    use je_base::html_content::IMarkdownString;
    use je_base::lifecycle::IDisposable;
    use je_base::promise::Promise;
    use je_base::uri::URI;
    use je_editor::code_action::languages::{CodeActionContext, TextEdit as EditorTextEdit};
    use je_editor::folding::syntax_range_provider::{FoldingContext, FoldingRange as EditorFoldingRange};
    use je_editor::language_configuration::{CharacterPair, CommentRule, EnterAction};
    use je_editor::language_providers::{
        DocumentHighlight, DocumentSymbol, SelectionRange as EditorSelectionRange, SymbolKind as EditorSymbolKind, SymbolTag as EditorSymbolTag,
    };
    use je_editor::languages::{CompletionContext, CompletionItemKind as EditorCompletionItemKind, CompletionItemRange, CompletionItemTag, CompletionLabel, StringOrMarkdown};
    use je_editor::parameter_hints::languages::{SignatureHelpTriggerKind, SignatureInformation as EditorSignatureInformation};
    use je_editor::position::Position as EditorPosition;
    use je_editor::range::Range as EditorRange;
    use je_editor::selection::Selection as EditorSelection;
    use je_extensions::manifest::ExtensionIdentifier;
    use je_platform::markers::markers::MarkerData;

    use super::{DataTransferDto, ExtHostContext, ICommandDto, IWorkspaceEditDto, MainContext, ProxyIdentifier};
    use crate::services::inline_completions::inline_completions_unification::IInlineCompletionsUnificationState;
    use crate::api::common::ext_host_type_converters::{HierarchyItemDto, IncomingCallDto, LocationDto, OutgoingCallDto};
    use crate::api::common::ext_host_types::{DocumentPasteTriggerKind, InlayHintKind, NewSymbolNameTag, SemanticTokensLegend};
    use crate::api::common::ext_host_workspace::GlobPatternDto;

    /// `IDocumentFilterDto`.
    #[derive(Clone, Debug, Default, PartialEq, Eq)]
    pub struct IDocumentFilterDto {
        pub language: Option<String>,
        pub scheme: Option<String>,
        pub pattern: Option<GlobPatternDto>,
        pub exclusive: Option<bool>,
        pub notebook_type: Option<String>,
        pub is_builtin: Option<bool>,
    }

    /// `CacheId`.
    pub type CacheId = u32;

    /// `ChainedCacheId`.
    pub type ChainedCacheId = (CacheId, CacheId);

    /// `ILocationDto`.
    pub type ILocationDto = LocationDto;

    /// `ILocationLinkDto`.
    #[derive(Clone, Debug, PartialEq)]
    pub struct ILocationLinkDto {
        pub origin_selection_range: Option<EditorRange>,
        pub uri: URI,
        pub range: EditorRange,
        pub target_selection_range: Option<EditorRange>,
    }

    /// `languages.Hover`.
    #[derive(Clone, Debug)]
    pub struct HoverDto {
        pub contents: Vec<IMarkdownString>,
        pub range: Option<EditorRange>,
        pub can_increase_verbosity: Option<bool>,
        pub can_decrease_verbosity: Option<bool>,
    }

    /// `HoverWithId`: o [`HoverDto`] mais o `id` do hover.
    #[derive(Clone, Debug)]
    pub struct HoverWithId {
        pub hover: HoverDto,
        pub id: u32,
    }

    /// O `{ id: number }` que `languages.HoverContext<{ id: number }>` usa como `previousHover`.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct HoverIdDto {
        pub id: u32,
    }

    /// `languages.HoverVerbosityRequest<{ id: number }>`.
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct HoverVerbosityRequestDto {
        pub verbosity_delta: f64,
        pub previous_hover: HoverIdDto,
    }

    /// `languages.HoverContext<{ id: number }>`.
    #[derive(Clone, Copy, Debug, PartialEq, Default)]
    pub struct HoverContextDto {
        pub verbosity_request: Option<HoverVerbosityRequestDto>,
    }

    /// `languages.ReferenceContext`.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct ReferenceContext {
        pub include_declaration: bool,
    }

    /// `Dto<languages.MultiDocumentHighlight>`.
    #[derive(Clone, Debug, PartialEq)]
    pub struct MultiDocumentHighlightDto {
        pub uri: URI,
        pub highlights: Vec<DocumentHighlight>,
    }

    /// `ICodeLensDto`.
    #[derive(Clone, Debug)]
    pub struct ICodeLensDto {
        pub cache_id: Option<ChainedCacheId>,
        pub range: EditorRange,
        pub id: Option<String>,
        pub command: Option<ICommandDto>,
    }

    /// `ICodeLensListDto`.
    #[derive(Clone, Debug)]
    pub struct ICodeLensListDto {
        pub cache_id: Option<CacheId>,
        pub lenses: Vec<ICodeLensDto>,
    }

    // ---- parte 2: valores embutidos, expressão avaliável, edição ligada

    /// `languages.EvaluatableExpression`.
    #[derive(Clone, Debug, PartialEq)]
    pub struct EvaluatableExpressionDto {
        pub range: EditorRange,
        pub expression: Option<String>,
    }

    /// `languages.InlineValue`: `InlineValueText | InlineValueVariableLookup | InlineValueExpression`.
    #[derive(Clone, Debug, PartialEq)]
    pub enum InlineValueDto {
        Text { range: EditorRange, text: String },
        Variable { range: EditorRange, variable_name: Option<String>, case_sensitive_lookup: bool },
        Expression { range: EditorRange, expression: Option<String> },
    }

    /// `IInlineValueContextDto`.
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct IInlineValueContextDto {
        pub frame_id: f64,
        pub stopped_location: EditorRange,
    }

    /// `IRegExpDto`.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct IRegExpDto {
        pub pattern: String,
        pub flags: Option<String>,
    }

    /// `ILinkedEditingRangesDto`.
    #[derive(Clone, Debug, PartialEq)]
    pub struct ILinkedEditingRangesDto {
        pub ranges: Vec<EditorRange>,
        pub word_pattern: Option<IRegExpDto>,
    }

    // ---- parte 3: configuração de linguagem e definições de palavra

    /// `IIndentationRuleDto`.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct IIndentationRuleDto {
        pub decrease_indent_pattern: IRegExpDto,
        pub increase_indent_pattern: IRegExpDto,
        pub indent_next_line_pattern: Option<IRegExpDto>,
        pub un_indented_line_pattern: Option<IRegExpDto>,
    }

    /// `IOnEnterRuleDto`.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct IOnEnterRuleDto {
        pub before_text: IRegExpDto,
        pub after_text: Option<IRegExpDto>,
        pub previous_line_text: Option<IRegExpDto>,
        pub action: EnterAction,
    }

    /// `IAutoClosingPairDto`.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct IAutoClosingPairDto {
        pub open: String,
        pub close: String,
        pub not_in: Option<Vec<String>>,
        pub before_text: Option<IRegExpDto>,
    }

    /// O `docComment` de `ILanguageConfigurationDto.__electricCharacterSupport`. O `scope` e o `lineStart` só existem na
    /// API (a thread principal lê `open` e `close`).
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct ElectricCharacterDocCommentDto {
        pub scope: String,
        pub open: String,
        pub line_start: String,
        pub close: Option<String>,
    }

    /// O `__electricCharacterSupport` (obsoleto) de `ILanguageConfigurationDto`. O `brackets?: any` é um [`Dynamic`] que
    /// ninguém lê.
    #[derive(Clone, Default)]
    pub struct ElectricCharacterSupportDto {
        pub brackets: Option<Dynamic>,
        pub doc_comment: Option<ElectricCharacterDocCommentDto>,
    }

    /// O par de `ILanguageConfigurationDto.__characterPairSupport.autoClosingPairs`.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct CharacterPairSupportPairDto {
        pub open: String,
        pub close: String,
        pub not_in: Option<Vec<String>>,
    }

    /// O `__characterPairSupport` (obsoleto) de `ILanguageConfigurationDto`.
    #[derive(Clone, Debug, Default, PartialEq, Eq)]
    pub struct CharacterPairSupportDto {
        pub auto_closing_pairs: Vec<CharacterPairSupportPairDto>,
    }

    /// `ILanguageConfigurationDto`.
    #[derive(Clone, Default)]
    pub struct ILanguageConfigurationDto {
        pub comments: Option<CommentRule>,
        pub brackets: Option<Vec<CharacterPair>>,
        pub word_pattern: Option<IRegExpDto>,
        pub indentation_rules: Option<IIndentationRuleDto>,
        pub on_enter_rules: Option<Vec<IOnEnterRuleDto>>,
        pub electric_character_support: Option<ElectricCharacterSupportDto>,
        pub character_pair_support: Option<CharacterPairSupportDto>,
        pub auto_closing_pairs: Option<Vec<IAutoClosingPairDto>>,
    }

    /// `ILanguageWordDefinitionDto`.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct ILanguageWordDefinitionDto {
        pub language_id: String,
        pub regex_source: String,
        pub regex_flags: String,
    }

    // ---- parte 2: ações de código e colar

    /// `IRange | ISelection`.
    #[derive(Clone)]
    pub enum RangeOrSelection {
        Range(EditorRange),
        Selection(EditorSelection),
    }

    /// `ICodeActionDto`. O `is_synthetic` é o `_isSynthetic` de `CustomCodeAction`.
    #[derive(Clone, Default)]
    pub struct ICodeActionDto {
        pub is_synthetic: Option<bool>,
        pub cache_id: Option<ChainedCacheId>,
        pub title: String,
        pub edit: Option<IWorkspaceEditDto>,
        pub diagnostics: Option<Vec<MarkerData>>,
        pub command: Option<ICommandDto>,
        pub kind: Option<String>,
        pub is_preferred: Option<bool>,
        pub is_ai: Option<bool>,
        pub disabled: Option<String>,
        pub ranges: Option<Vec<EditorRange>>,
    }

    /// `ICodeActionListDto`.
    #[derive(Clone)]
    pub struct ICodeActionListDto {
        pub cache_id: CacheId,
        pub actions: Vec<ICodeActionDto>,
    }

    /// O elemento de `ICodeActionProviderMetadataDto.documentation`.
    #[derive(Clone, Debug)]
    pub struct CodeActionDocumentationDto {
        pub kind: String,
        pub command: ICommandDto,
    }

    /// `ICodeActionProviderMetadataDto`.
    #[derive(Clone, Debug, Default)]
    pub struct ICodeActionProviderMetadataDto {
        pub provided_kinds: Option<Vec<String>>,
        pub documentation: Option<Vec<CodeActionDocumentationDto>>,
    }

    /// O `{ edit?: IWorkspaceEditDto; command?: ICommandDto }` de `$resolveCodeAction`.
    #[derive(Clone, Default)]
    pub struct ResolvedCodeActionDto {
        pub edit: Option<IWorkspaceEditDto>,
        pub command: Option<ICommandDto>,
    }

    /// `string | { snippet: string }`: o `insertText` das edições de colar e de soltar.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub enum DropPasteInsertTextDto {
        Text(String),
        Snippet(String),
    }

    /// `{ value: string }`: o `kind` de `IPasteEditDto`.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct EditKindDto {
        pub value: String,
    }

    /// `IPasteEditProviderMetadataDto`.
    #[derive(Clone, Debug, Default)]
    pub struct IPasteEditProviderMetadataDto {
        pub supports_copy: bool,
        pub supports_paste: bool,
        pub supports_resolve: bool,
        pub provided_paste_edit_kinds: Option<Vec<String>>,
        pub copy_mime_types: Option<Vec<String>>,
        pub paste_mime_types: Option<Vec<String>>,
    }

    /// `IDocumentPasteContextDto`.
    #[derive(Clone, Debug)]
    pub struct IDocumentPasteContextDto {
        pub only: Option<String>,
        pub trigger_kind: DocumentPasteTriggerKind,
    }

    /// `IPasteEditDto`.
    #[derive(Clone)]
    pub struct IPasteEditDto {
        pub cache_id: Option<ChainedCacheId>,
        pub title: String,
        pub kind: Option<EditKindDto>,
        pub insert_text: DropPasteInsertTextDto,
        pub additional_edit: Option<IWorkspaceEditDto>,
        pub yield_to: Option<Vec<String>>,
    }

    /// O `{ insertText?; additionalEdit? }` de `$resolvePasteEdit`.
    #[derive(Clone, Default)]
    pub struct ResolvedPasteEditDto {
        pub insert_text: Option<DropPasteInsertTextDto>,
        pub additional_edit: Option<IWorkspaceEditDto>,
    }

    /// `IDocumentDropEditProviderMetadata`.
    #[derive(Clone, Debug, Default)]
    pub struct IDocumentDropEditProviderMetadata {
        pub supports_resolve: bool,
        pub drop_mime_types: Vec<String>,
        pub provided_drop_kinds: Option<Vec<String>>,
    }

    /// `IDocumentDropEditDto`.
    #[derive(Clone)]
    pub struct IDocumentDropEditDto {
        pub cache_id: Option<ChainedCacheId>,
        pub title: String,
        pub kind: Option<String>,
        pub insert_text: DropPasteInsertTextDto,
        pub additional_edit: Option<IWorkspaceEditDto>,
        pub yield_to: Option<Vec<String>>,
    }

    /// O `{ additionalEdit? }` de `$resolveDropEdit`.
    #[derive(Clone, Default)]
    pub struct ResolvedDropEditDto {
        pub additional_edit: Option<IWorkspaceEditDto>,
    }

    // ---- parte 2: formatação, símbolos do workspace, renomeação

    /// `languages.FormattingOptions`.
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct FormattingOptions {
        pub tab_size: f64,
        pub insert_spaces: bool,
    }

    /// `IWorkspaceSymbolDto`: `CachedSessionItem<Dto<IWorkspaceSymbol>>`.
    #[derive(Clone, Debug)]
    pub struct IWorkspaceSymbolDto {
        pub cache_id: Option<ChainedCacheId>,
        pub name: String,
        pub container_name: Option<String>,
        pub kind: EditorSymbolKind,
        pub tags: Option<Vec<EditorSymbolTag>>,
        pub location: ILocationDto,
    }

    /// `IWorkspaceSymbolsDto`: `CachedSession<{ symbols: IWorkspaceSymbolDto[] }>`.
    #[derive(Clone, Debug, Default)]
    pub struct IWorkspaceSymbolsDto {
        pub cache_id: Option<CacheId>,
        pub symbols: Vec<IWorkspaceSymbolDto>,
    }

    /// `IWorkspaceEditDto & languages.Rejection` (ver a nota do módulo).
    #[derive(Clone, Default)]
    pub struct RenameEditsDto {
        pub workspace_edit: Option<IWorkspaceEditDto>,
        pub reject_reason: Option<String>,
    }

    /// `languages.RenameLocation & languages.Rejection` (ver a nota do módulo).
    #[derive(Clone, Debug, Default, PartialEq)]
    pub struct RenameLocationDto {
        pub range: Option<EditorRange>,
        pub text: Option<String>,
        pub reject_reason: Option<String>,
    }

    /// `languages.NewSymbolNameTriggerKind`.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum LanguagesNewSymbolNameTriggerKind {
        Invoke = 0,
        Automatic = 1,
    }

    /// `languages.NewSymbolName`.
    #[derive(Clone, Debug, PartialEq)]
    pub struct NewSymbolNameDto {
        pub new_symbol_name: String,
        pub tags: Option<Vec<NewSymbolNameTag>>,
    }

    // ---- parte 2: completar

    /// O `{ insert: IRange; replace: IRange }` de `ISuggestResultDtoField.defaultRanges`.
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct ISuggestDefaultRangesDto {
        pub insert: EditorRange,
        pub replace: EditorRange,
    }

    /// `ISuggestDataDto` (ver a nota do módulo). `cache_id` é o `x`.
    #[derive(Clone)]
    pub struct ISuggestDataDto {
        pub cache_id: Option<ChainedCacheId>,
        pub label: CompletionLabel,
        pub kind: Option<EditorCompletionItemKind>,
        pub detail: Option<String>,
        pub documentation: Option<StringOrMarkdown>,
        pub sort_text: Option<String>,
        pub filter_text: Option<String>,
        pub preselect: Option<bool>,
        pub insert_text: Option<String>,
        /// Máscara de `completion_item_insert_text_rule`.
        pub insert_text_rules: Option<u32>,
        pub range: Option<CompletionItemRange>,
        pub commit_characters: Option<String>,
        pub additional_text_edits: Option<Vec<EditorTextEdit>>,
        pub kind_modifier: Option<Vec<CompletionItemTag>>,
        pub command_ident: Option<String>,
        pub command_id: Option<String>,
        pub command_arguments: Option<Vec<Dynamic>>,
    }

    /// `ISuggestResultDto`. `cache_id` é o `x`.
    #[derive(Clone)]
    pub struct ISuggestResultDto {
        pub cache_id: Option<CacheId>,
        pub default_ranges: ISuggestDefaultRangesDto,
        pub completions: Vec<ISuggestDataDto>,
        pub is_incomplete: Option<bool>,
        pub duration: f64,
    }

    // ---- parte 2: assinatura, dicas embutidas, links, cor

    /// `ISignatureHelpProviderMetadataDto`.
    #[derive(Clone, Debug, Default, PartialEq, Eq)]
    pub struct ISignatureHelpProviderMetadataDto {
        pub trigger_characters: Vec<String>,
        pub retrigger_characters: Vec<String>,
    }

    /// `ISignatureHelpDto`.
    #[derive(Clone, Debug)]
    pub struct ISignatureHelpDto {
        pub id: CacheId,
        pub signatures: Vec<EditorSignatureInformation>,
        pub active_signature: i32,
        pub active_parameter: i32,
    }

    /// `ISignatureHelpContextDto`.
    #[derive(Clone, Debug)]
    pub struct ISignatureHelpContextDto {
        pub trigger_kind: SignatureHelpTriggerKind,
        pub trigger_character: Option<String>,
        pub is_retrigger: bool,
        pub active_signature_help: Option<ISignatureHelpDto>,
    }

    /// `languages.InlayHintLabelPart`.
    #[derive(Clone, Debug)]
    pub struct InlayHintLabelPartDto {
        pub label: String,
        pub tooltip: Option<StringOrMarkdown>,
        pub location: Option<ILocationDto>,
        pub command: Option<ICommandDto>,
    }

    /// `string | languages.InlayHintLabelPart[]`.
    #[derive(Clone, Debug)]
    pub enum InlayHintLabelDto {
        Text(String),
        Parts(Vec<InlayHintLabelPartDto>),
    }

    /// `IInlayHintDto`: `CachedSessionItem<Dto<languages.InlayHint>>`.
    #[derive(Clone)]
    pub struct IInlayHintDto {
        pub cache_id: Option<ChainedCacheId>,
        pub label: InlayHintLabelDto,
        pub tooltip: Option<StringOrMarkdown>,
        pub position: EditorPosition,
        pub text_edits: Option<Vec<EditorTextEdit>>,
        pub kind: Option<InlayHintKind>,
        pub padding_left: Option<bool>,
        pub padding_right: Option<bool>,
    }

    /// `IInlayHintsDto`: `CachedSession<{ hints: IInlayHintDto[] }>`.
    #[derive(Clone)]
    pub struct IInlayHintsDto {
        pub cache_id: Option<CacheId>,
        pub hints: Vec<IInlayHintDto>,
    }

    /// `ILinkDto`: `CachedSessionItem<Dto<languages.ILink>>`. O `url` é sempre `URI` (`DocumentLink.from` passa o `target`).
    #[derive(Clone, Debug)]
    pub struct ILinkDto {
        pub cache_id: Option<ChainedCacheId>,
        pub range: EditorRange,
        pub url: Option<URI>,
        pub tooltip: Option<String>,
    }

    /// `ILinksListDto`: `CachedSession<{ links: ILinkDto[] }>`.
    #[derive(Clone, Debug)]
    pub struct ILinksListDto {
        pub cache_id: Option<CacheId>,
        pub links: Vec<ILinkDto>,
    }

    /// `IRawColorInfo`.
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct IRawColorInfo {
        pub color: [f64; 4],
        pub range: EditorRange,
    }

    /// `languages.IColorPresentation`.
    #[derive(Clone)]
    pub struct IColorPresentationDto {
        pub label: String,
        pub text_edit: Option<EditorTextEdit>,
        pub additional_text_edits: Option<Vec<EditorTextEdit>>,
    }

    /// `ICallHierarchyItemDto`.
    pub type ICallHierarchyItemDto = HierarchyItemDto;

    /// `ITypeHierarchyItemDto`.
    pub type ITypeHierarchyItemDto = HierarchyItemDto;

    /// `IIncomingCallDto`.
    pub type IIncomingCallDto = IncomingCallDto;

    /// `IOutgoingCallDto`.
    pub type IOutgoingCallDto = OutgoingCallDto;

    /// `MainThreadLanguageFeaturesShape` (parte 1, ver a nota do módulo).
    pub trait MainThreadLanguageFeaturesShape: IDisposable {
        fn rpc_unregister(&self, handle: u32);
        fn rpc_register_document_symbol_provider(&self, handle: u32, selector: Vec<IDocumentFilterDto>, label: &str);
        fn rpc_register_code_lens_support(&self, handle: u32, selector: Vec<IDocumentFilterDto>, event_handle: Option<u32>);
        fn rpc_emit_code_lens_event(&self, event_handle: u32, event: Option<Dynamic>);
        fn rpc_register_definition_support(&self, handle: u32, selector: Vec<IDocumentFilterDto>);
        fn rpc_register_declaration_support(&self, handle: u32, selector: Vec<IDocumentFilterDto>);
        fn rpc_register_implementation_support(&self, handle: u32, selector: Vec<IDocumentFilterDto>);
        fn rpc_register_type_definition_support(&self, handle: u32, selector: Vec<IDocumentFilterDto>);
        fn rpc_register_hover_provider(&self, handle: u32, selector: Vec<IDocumentFilterDto>);
        fn rpc_register_document_highlight_provider(&self, handle: u32, selector: Vec<IDocumentFilterDto>);
        fn rpc_register_multi_document_highlight_provider(&self, handle: u32, selector: Vec<IDocumentFilterDto>);
        fn rpc_register_reference_support(&self, handle: u32, selector: Vec<IDocumentFilterDto>);
        fn rpc_register_evaluatable_expression_provider(&self, handle: u32, selector: Vec<IDocumentFilterDto>);
        fn rpc_register_inline_values_provider(&self, handle: u32, selector: Vec<IDocumentFilterDto>, event_handle: Option<u32>);
        fn rpc_emit_inline_values_event(&self, event_handle: u32, event: Option<Dynamic>);
        fn rpc_register_linked_editing_range_provider(&self, handle: u32, selector: Vec<IDocumentFilterDto>);
        fn rpc_register_code_action_support(
            &self,
            handle: u32,
            selector: Vec<IDocumentFilterDto>,
            metadata: ICodeActionProviderMetadataDto,
            display_name: &str,
            extension_id: &str,
            supports_resolve: bool,
        );
        fn rpc_register_paste_edit_provider(&self, handle: u32, selector: Vec<IDocumentFilterDto>, metadata: IPasteEditProviderMetadataDto);
        fn rpc_register_document_formatting_support(&self, handle: u32, selector: Vec<IDocumentFilterDto>, extension_id: &ExtensionIdentifier, display_name: &str);
        fn rpc_register_range_formatting_support(
            &self,
            handle: u32,
            selector: Vec<IDocumentFilterDto>,
            extension_id: &ExtensionIdentifier,
            display_name: &str,
            support_ranges: bool,
        );
        fn rpc_register_on_type_formatting_support(
            &self,
            handle: u32,
            selector: Vec<IDocumentFilterDto>,
            auto_format_trigger_characters: Vec<String>,
            extension_id: &ExtensionIdentifier,
        );
        fn rpc_register_navigate_type_support(&self, handle: u32, supports_resolve: bool);
        fn rpc_register_rename_support(&self, handle: u32, selector: Vec<IDocumentFilterDto>, supports_resolve_initial_values: bool);
        fn rpc_register_new_symbol_names_provider(&self, handle: u32, selector: Vec<IDocumentFilterDto>);
        fn rpc_register_document_semantic_tokens_provider(
            &self,
            handle: u32,
            selector: Vec<IDocumentFilterDto>,
            legend: SemanticTokensLegend,
            event_handle: Option<u32>,
        );
        fn rpc_emit_document_semantic_tokens_event(&self, event_handle: u32);
        fn rpc_register_document_range_semantic_tokens_provider(
            &self,
            handle: u32,
            selector: Vec<IDocumentFilterDto>,
            legend: SemanticTokensLegend,
            event_handle: Option<u32>,
        );
        fn rpc_emit_document_range_semantic_tokens_event(&self, event_handle: u32);
        fn rpc_register_completions_provider(
            &self,
            handle: u32,
            selector: Vec<IDocumentFilterDto>,
            trigger_characters: Vec<String>,
            supports_resolve_details: bool,
            extension_id: &ExtensionIdentifier,
        );
        fn rpc_register_signature_help_provider(&self, handle: u32, selector: Vec<IDocumentFilterDto>, metadata: ISignatureHelpProviderMetadataDto);
        fn rpc_register_inlay_hints_provider(
            &self,
            handle: u32,
            selector: Vec<IDocumentFilterDto>,
            supports_resolve: bool,
            event_handle: Option<u32>,
            display_name: Option<&str>,
        );
        fn rpc_emit_inlay_hints_event(&self, event_handle: u32);
        fn rpc_register_document_link_provider(&self, handle: u32, selector: Vec<IDocumentFilterDto>, supports_resolve: bool);
        fn rpc_register_document_color_provider(&self, handle: u32, selector: Vec<IDocumentFilterDto>);
        fn rpc_register_folding_range_provider(&self, handle: u32, selector: Vec<IDocumentFilterDto>, extension_id: &ExtensionIdentifier, event_handle: Option<u32>);
        fn rpc_emit_folding_range_event(&self, event_handle: u32, event: Option<Dynamic>);
        fn rpc_register_selection_range_provider(&self, handle: u32, selector: Vec<IDocumentFilterDto>);
        fn rpc_register_call_hierarchy_provider(&self, handle: u32, selector: Vec<IDocumentFilterDto>);
        fn rpc_register_type_hierarchy_provider(&self, handle: u32, selector: Vec<IDocumentFilterDto>);
        fn rpc_register_document_on_drop_edit_provider(&self, handle: u32, selector: Vec<IDocumentFilterDto>, metadata: Option<IDocumentDropEditProviderMetadata>);
        fn rpc_resolve_paste_file_data(&self, handle: u32, request_id: u32, data_id: &str) -> Promise<VSBuffer>;
        fn rpc_resolve_document_on_drop_file_data(&self, handle: u32, request_id: u32, data_id: &str) -> Promise<VSBuffer>;
        fn rpc_set_language_configuration(&self, handle: u32, language_id: &str, configuration: ILanguageConfigurationDto);
    }

    /// `ExtHostLanguageFeaturesShape` (parte 1, ver a nota do módulo).
    pub trait ExtHostLanguageFeaturesShape {
        fn rpc_provide_document_symbols(&self, handle: u32, resource: &URI, token: CancellationToken) -> Promise<Option<Vec<DocumentSymbol>>>;
        fn rpc_provide_code_lenses(&self, handle: u32, resource: &URI, token: CancellationToken) -> Promise<Option<ICodeLensListDto>>;
        fn rpc_resolve_code_lens(&self, handle: u32, symbol: ICodeLensDto, token: CancellationToken) -> Promise<Option<ICodeLensDto>>;
        fn rpc_release_code_lenses(&self, handle: u32, id: u32);
        fn rpc_provide_definition(&self, handle: u32, resource: &URI, position: EditorPosition, token: CancellationToken) -> Promise<Vec<ILocationLinkDto>>;
        fn rpc_provide_declaration(&self, handle: u32, resource: &URI, position: EditorPosition, token: CancellationToken) -> Promise<Vec<ILocationLinkDto>>;
        fn rpc_provide_implementation(&self, handle: u32, resource: &URI, position: EditorPosition, token: CancellationToken) -> Promise<Vec<ILocationLinkDto>>;
        fn rpc_provide_type_definition(&self, handle: u32, resource: &URI, position: EditorPosition, token: CancellationToken) -> Promise<Vec<ILocationLinkDto>>;
        fn rpc_provide_hover(
            &self,
            handle: u32,
            resource: &URI,
            position: EditorPosition,
            context: Option<HoverContextDto>,
            token: CancellationToken,
        ) -> Promise<Option<HoverWithId>>;
        fn rpc_release_hover(&self, handle: u32, id: u32);
        fn rpc_provide_document_highlights(&self, handle: u32, resource: &URI, position: EditorPosition, token: CancellationToken) -> Promise<Option<Vec<DocumentHighlight>>>;
        fn rpc_provide_multi_document_highlights(
            &self,
            handle: u32,
            resource: &URI,
            position: EditorPosition,
            other_models: Vec<URI>,
            token: CancellationToken,
        ) -> Promise<Option<Vec<MultiDocumentHighlightDto>>>;
        fn rpc_provide_references(
            &self,
            handle: u32,
            resource: &URI,
            position: EditorPosition,
            context: ReferenceContext,
            token: CancellationToken,
        ) -> Promise<Option<Vec<ILocationDto>>>;
        fn rpc_provide_evaluatable_expression(&self, handle: u32, resource: &URI, position: EditorPosition, token: CancellationToken) -> Promise<Option<EvaluatableExpressionDto>>;
        fn rpc_provide_inline_values(
            &self,
            handle: u32,
            resource: &URI,
            range: EditorRange,
            context: IInlineValueContextDto,
            token: CancellationToken,
        ) -> Promise<Option<Vec<InlineValueDto>>>;
        fn rpc_provide_linked_editing_ranges(&self, handle: u32, resource: &URI, position: EditorPosition, token: CancellationToken) -> Promise<Option<ILinkedEditingRangesDto>>;
        fn rpc_provide_code_actions(
            &self,
            handle: u32,
            resource: &URI,
            range_or_selection: RangeOrSelection,
            context: CodeActionContext,
            token: CancellationToken,
        ) -> Promise<Option<ICodeActionListDto>>;
        fn rpc_resolve_code_action(&self, handle: u32, id: ChainedCacheId, token: CancellationToken) -> Promise<ResolvedCodeActionDto>;
        fn rpc_release_code_actions(&self, handle: u32, cache_id: u32);
        fn rpc_prepare_document_paste(
            &self,
            handle: u32,
            uri: &URI,
            ranges: Vec<EditorRange>,
            data_transfer: DataTransferDto,
            token: CancellationToken,
        ) -> Promise<Option<DataTransferDto>>;
        fn rpc_provide_paste_edits(
            &self,
            handle: u32,
            request_id: u32,
            uri: &URI,
            ranges: Vec<EditorRange>,
            data_transfer: DataTransferDto,
            context: IDocumentPasteContextDto,
            token: CancellationToken,
        ) -> Promise<Option<Vec<IPasteEditDto>>>;
        fn rpc_resolve_paste_edit(&self, handle: u32, id: ChainedCacheId, token: CancellationToken) -> Promise<ResolvedPasteEditDto>;
        fn rpc_release_paste_edits(&self, handle: u32, cache_id: u32);
        fn rpc_provide_document_formatting_edits(
            &self,
            handle: u32,
            resource: &URI,
            options: FormattingOptions,
            token: CancellationToken,
        ) -> Promise<Option<Vec<EditorTextEdit>>>;
        fn rpc_provide_document_range_formatting_edits(
            &self,
            handle: u32,
            resource: &URI,
            range: EditorRange,
            options: FormattingOptions,
            token: CancellationToken,
        ) -> Promise<Option<Vec<EditorTextEdit>>>;
        fn rpc_provide_document_ranges_formatting_edits(
            &self,
            handle: u32,
            resource: &URI,
            ranges: Vec<EditorRange>,
            options: FormattingOptions,
            token: CancellationToken,
        ) -> Promise<Option<Vec<EditorTextEdit>>>;
        fn rpc_provide_on_type_formatting_edits(
            &self,
            handle: u32,
            resource: &URI,
            position: EditorPosition,
            ch: &str,
            options: FormattingOptions,
            token: CancellationToken,
        ) -> Promise<Option<Vec<EditorTextEdit>>>;
        fn rpc_provide_workspace_symbols(&self, handle: u32, search: &str, token: CancellationToken) -> Promise<IWorkspaceSymbolsDto>;
        fn rpc_resolve_workspace_symbol(&self, handle: u32, symbol: IWorkspaceSymbolDto, token: CancellationToken) -> Promise<Option<IWorkspaceSymbolDto>>;
        fn rpc_release_workspace_symbols(&self, handle: u32, id: u32);
        fn rpc_provide_rename_edits(
            &self,
            handle: u32,
            resource: &URI,
            position: EditorPosition,
            new_name: &str,
            token: CancellationToken,
        ) -> Promise<Option<RenameEditsDto>>;
        fn rpc_resolve_rename_location(&self, handle: u32, resource: &URI, position: EditorPosition, token: CancellationToken) -> Promise<Option<RenameLocationDto>>;
        fn rpc_supports_automatic_new_symbol_names_trigger_kind(&self, handle: u32) -> Promise<Option<bool>>;
        fn rpc_provide_new_symbol_names(
            &self,
            handle: u32,
            resource: &URI,
            range: EditorRange,
            trigger_kind: LanguagesNewSymbolNameTriggerKind,
            token: CancellationToken,
        ) -> Promise<Option<Vec<NewSymbolNameDto>>>;
        fn rpc_provide_document_semantic_tokens(&self, handle: u32, resource: &URI, previous_result_id: u32, token: CancellationToken) -> Promise<Option<VSBuffer>>;
        fn rpc_release_document_semantic_tokens(&self, handle: u32, semantic_coloring_result_id: u32);
        fn rpc_provide_document_range_semantic_tokens(&self, handle: u32, resource: &URI, range: EditorRange, token: CancellationToken) -> Promise<Option<VSBuffer>>;
        fn rpc_provide_completion_items(
            &self,
            handle: u32,
            resource: &URI,
            position: EditorPosition,
            context: CompletionContext,
            token: CancellationToken,
        ) -> Promise<Option<ISuggestResultDto>>;
        fn rpc_resolve_completion_item(&self, handle: u32, id: ChainedCacheId, token: CancellationToken) -> Promise<Option<ISuggestDataDto>>;
        fn rpc_release_completion_items(&self, handle: u32, id: u32);
        fn rpc_provide_signature_help(
            &self,
            handle: u32,
            resource: &URI,
            position: EditorPosition,
            context: ISignatureHelpContextDto,
            token: CancellationToken,
        ) -> Promise<Option<ISignatureHelpDto>>;
        fn rpc_release_signature_help(&self, handle: u32, id: u32);
        fn rpc_provide_inlay_hints(&self, handle: u32, resource: &URI, range: EditorRange, token: CancellationToken) -> Promise<Option<IInlayHintsDto>>;
        fn rpc_resolve_inlay_hint(&self, handle: u32, id: ChainedCacheId, token: CancellationToken) -> Promise<Option<IInlayHintDto>>;
        fn rpc_release_inlay_hints(&self, handle: u32, id: u32);
        fn rpc_provide_document_links(&self, handle: u32, resource: &URI, token: CancellationToken) -> Promise<Option<ILinksListDto>>;
        fn rpc_resolve_document_link(&self, handle: u32, id: ChainedCacheId, token: CancellationToken) -> Promise<Option<ILinkDto>>;
        fn rpc_release_document_links(&self, handle: u32, id: u32);
        fn rpc_provide_document_colors(&self, handle: u32, resource: &URI, token: CancellationToken) -> Promise<Vec<IRawColorInfo>>;
        fn rpc_provide_color_presentations(
            &self,
            handle: u32,
            resource: &URI,
            color_info: IRawColorInfo,
            token: CancellationToken,
        ) -> Promise<Option<Vec<IColorPresentationDto>>>;
        fn rpc_provide_folding_ranges(&self, handle: u32, resource: &URI, context: FoldingContext, token: CancellationToken) -> Promise<Option<Vec<EditorFoldingRange>>>;
        fn rpc_provide_selection_ranges(
            &self,
            handle: u32,
            resource: &URI,
            positions: Vec<EditorPosition>,
            token: CancellationToken,
        ) -> Promise<Vec<Vec<EditorSelectionRange>>>;
        fn rpc_prepare_call_hierarchy(&self, handle: u32, resource: &URI, position: EditorPosition, token: CancellationToken) -> Promise<Option<Vec<ICallHierarchyItemDto>>>;
        fn rpc_provide_call_hierarchy_incoming_calls(
            &self,
            handle: u32,
            session_id: &str,
            item_id: &str,
            token: CancellationToken,
        ) -> Promise<Option<Vec<IIncomingCallDto>>>;
        fn rpc_provide_call_hierarchy_outgoing_calls(
            &self,
            handle: u32,
            session_id: &str,
            item_id: &str,
            token: CancellationToken,
        ) -> Promise<Option<Vec<IOutgoingCallDto>>>;
        fn rpc_release_call_hierarchy(&self, handle: u32, session_id: &str);
        fn rpc_prepare_type_hierarchy(&self, handle: u32, resource: &URI, position: EditorPosition, token: CancellationToken) -> Promise<Option<Vec<ITypeHierarchyItemDto>>>;
        fn rpc_provide_type_hierarchy_supertypes(
            &self,
            handle: u32,
            session_id: &str,
            item_id: &str,
            token: CancellationToken,
        ) -> Promise<Option<Vec<ITypeHierarchyItemDto>>>;
        fn rpc_provide_type_hierarchy_subtypes(
            &self,
            handle: u32,
            session_id: &str,
            item_id: &str,
            token: CancellationToken,
        ) -> Promise<Option<Vec<ITypeHierarchyItemDto>>>;
        fn rpc_release_type_hierarchy(&self, handle: u32, session_id: &str);
        fn rpc_provide_document_on_drop_edits(
            &self,
            handle: u32,
            request_id: u32,
            resource: &URI,
            position: EditorPosition,
            data_transfer: DataTransferDto,
            token: CancellationToken,
        ) -> Promise<Option<Vec<IDocumentDropEditDto>>>;
        fn rpc_resolve_drop_edit(&self, handle: u32, id: ChainedCacheId, token: CancellationToken) -> Promise<ResolvedDropEditDto>;
        fn rpc_release_document_on_drop_edits(&self, handle: u32, cache_id: u32);
        fn rpc_set_word_definitions(&self, word_definitions: Vec<ILanguageWordDefinitionDto>);
        fn rpc_accept_inline_completions_unification_state(&self, state: IInlineCompletionsUnificationState);
    }

    impl MainContext {
        pub const MAIN_THREAD_LANGUAGE_FEATURES: ProxyIdentifier<dyn MainThreadLanguageFeaturesShape> = ProxyIdentifier::new("MainThreadLanguageFeatures");
    }

    impl ExtHostContext {
        pub const EXT_HOST_LANGUAGE_FEATURES: ProxyIdentifier<dyn ExtHostLanguageFeaturesShape> = ProxyIdentifier::new("ExtHostLanguageFeatures");
    }
}

// #endregion

// #region Notebook (controlador, documentos e editores)

pub use self::notebook_region::*;

/// Os DTOs e as formas de `extHost.protocol.ts` que `extHostNotebook.ts`, `extHostNotebookDocument.ts`,
/// `extHostNotebookEditor.ts` e `extHostNotebookDocuments.ts` usam (os de kernel entram com `extHostNotebookKernels.ts`).
///
/// Divergências:
/// - Os `UriComponents` são [`URI`]. Os handles de célula (`number`) são `i64`, como os de `notebook_common`; os de
///   registro (serializador, provedor de barra de status) são `u32`.
/// - `NotebookCellDto` e `NotebookRawContentEventDto` não são os de `notebook_common` (`IMainCellDto`, que tem `url` e
///   saídas `IOutputDto`): os do protocolo têm `uri` e saídas [`NotebookOutputDto`]. A união discriminada por `kind` é um
///   `enum`, sem o campo `kind`.
/// - `INotebookCellStatusBarEntryDto` (`Dto<INotebookCellStatusBarItem>`) guarda só o que `NotebookStatusBarItem.from`
///   preenche: o comando já é o [`ICommandDto`] convertido, e o `tooltip` é texto.
/// - `Promise<string>` de `$tryShowNotebookDocument` é um `String` (vazio no lugar de `undefined`/falsy).
/// - `INotebookPartialFileStatsWithMetadata | files.FileOperationError` de `$saveNotebook` é [`NotebookSaveResult`].
/// - `newActiveEditor?: string | null` é `Option<Option<String>>` (`None` ausente, `Some(None)` é `null`), como em
///   [`DocumentsAndEditorsDelta`]. `$searchInNotebooks` não está em [`ExtHostNotebookShape`]: depende de `ExtHostSearch`,
///   `CellSearchModel` e `searchNotebookHelpers`, que não existem no porte.
mod notebook_region {
    use je_base::buffer::VSBuffer;
    use je_base::cancellation::CancellationToken;
    use je_base::dynamic::Dynamic;
    use je_base::lifecycle::IDisposable;
    use je_base::promise::Promise;
    use je_base::uri::URI;
    use je_platform::accessibility::IAccessibilityInformation;
    use je_platform::files::files::{FileOperationError, WriteFileOptions};

    use super::{
        EditorGroupColumn, ExtHostContext, ICommandDto, MainContext, NotebookDataDto, NotebookOutputDto, NotebookOutputItemDto, ProxyIdentifier,
        SerializableObjectWithBuffers,
    };
    use crate::api::common::ext_host_type_converters::CellRange;
    use crate::api::common::ext_host_types::NotebookEditorRevealType;
    use crate::contrib::notebook::common::notebook_common::{
        CellKind, CellStatusbarAlignment, INotebookContributionData, NotebookCellInternalMetadata, NotebookCellMetadata, NotebookCellTextModelSplice,
        NotebookDocumentMetadata, NotebookExtensionDescription, TransientOptions,
    };

    /// `INotebookDocumentShowOptions`.
    #[derive(Clone, Debug, Default)]
    pub struct INotebookDocumentShowOptions {
        pub position: Option<EditorGroupColumn>,
        pub preserve_focus: Option<bool>,
        pub pinned: Option<bool>,
        pub selections: Option<Vec<CellRange>>,
        pub label: Option<String>,
    }

    /// `INotebookCellStatusBarEntryDto`.
    #[derive(Clone, Debug)]
    pub struct INotebookCellStatusBarEntryDto {
        pub alignment: CellStatusbarAlignment,
        pub priority: Option<f64>,
        pub text: String,
        pub tooltip: Option<String>,
        pub command: Option<ICommandDto>,
        pub accessibility_information: Option<IAccessibilityInformation>,
    }

    /// `INotebookCellStatusBarListDto`.
    #[derive(Clone, Debug)]
    pub struct INotebookCellStatusBarListDto {
        pub items: Vec<INotebookCellStatusBarEntryDto>,
        pub cache_id: u32,
    }

    /// O argumento de `$tryCreateNotebook`: `{ viewType: string; content?: NotebookDataDto }`.
    #[derive(Clone, Debug)]
    pub struct NotebookCreateOptions {
        pub view_type: String,
        pub content: Option<NotebookDataDto>,
    }

    /// `MainThreadNotebookShape`.
    pub trait MainThreadNotebookShape: IDisposable {
        fn rpc_register_notebook_serializer(
            &self,
            handle: u32,
            extension: NotebookExtensionDescription,
            view_type: &str,
            options: TransientOptions,
            registration: Option<INotebookContributionData>,
        );
        fn rpc_unregister_notebook_serializer(&self, handle: u32);

        fn rpc_register_notebook_cell_status_bar_item_provider(&self, handle: u32, event_handle: Option<u32>, view_type: &str) -> Promise<()>;
        fn rpc_unregister_notebook_cell_status_bar_item_provider(&self, handle: u32, event_handle: Option<u32>) -> Promise<()>;
        fn rpc_emit_cell_status_bar_event(&self, event_handle: u32);
    }

    /// `MainThreadNotebookEditorsShape`.
    pub trait MainThreadNotebookEditorsShape: IDisposable {
        fn rpc_try_show_notebook_document(&self, uri: &URI, view_type: &str, options: INotebookDocumentShowOptions) -> Promise<String>;
        fn rpc_try_reveal_range(&self, id: &str, range: CellRange, reveal_type: NotebookEditorRevealType) -> Promise<()>;
        fn rpc_try_set_selections(&self, id: &str, range: Vec<CellRange>) -> Promise<()>;
    }

    /// `MainThreadNotebookDocumentsShape`.
    pub trait MainThreadNotebookDocumentsShape: IDisposable {
        fn rpc_try_create_notebook(&self, options: NotebookCreateOptions) -> Promise<URI>;
        fn rpc_try_open_notebook(&self, uri: &URI) -> Promise<URI>;
        fn rpc_try_save_notebook(&self, uri: &URI) -> Promise<bool>;
    }

    /// `NotebookCellDto`.
    #[derive(Clone, Debug)]
    pub struct NotebookCellDto {
        pub handle: i64,
        pub uri: URI,
        pub eol: String,
        pub source: Vec<String>,
        pub language: String,
        pub mime: Option<String>,
        pub cell_kind: CellKind,
        pub outputs: Vec<NotebookOutputDto>,
        pub metadata: Option<NotebookCellMetadata>,
        pub internal_metadata: Option<NotebookCellInternalMetadata>,
    }

    /// `INotebookModelAddedData`.
    #[derive(Clone, Debug)]
    pub struct INotebookModelAddedData {
        pub uri: URI,
        pub version_id: i64,
        pub cells: Vec<NotebookCellDto>,
        pub view_type: String,
        pub metadata: Option<NotebookDocumentMetadata>,
    }

    /// `INotebookEditorAddData`.
    #[derive(Clone, Debug)]
    pub struct INotebookEditorAddData {
        pub id: String,
        pub document_uri: URI,
        pub selections: Vec<CellRange>,
        pub visible_ranges: Vec<CellRange>,
        pub view_column: Option<f64>,
        pub view_type: String,
    }

    /// `INotebookDocumentsAndEditorsDelta`.
    #[derive(Clone, Debug, Default)]
    pub struct INotebookDocumentsAndEditorsDelta {
        pub removed_documents: Option<Vec<URI>>,
        pub added_documents: Option<Vec<INotebookModelAddedData>>,
        pub removed_editors: Option<Vec<String>>,
        pub added_editors: Option<Vec<INotebookEditorAddData>>,
        pub new_active_editor: Option<Option<String>>,
        pub visible_editors: Option<Vec<String>>,
    }

    /// `INotebookDocumentPropertiesChangeData`.
    #[derive(Clone, Debug, Default)]
    pub struct INotebookDocumentPropertiesChangeData {
        pub metadata: Option<NotebookDocumentMetadata>,
    }

    /// `INotebookPartialFileStatsWithMetadata`: o `IFileStatWithMetadata` sem `resource` e `children`.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct INotebookPartialFileStatsWithMetadata {
        pub name: String,
        pub is_file: bool,
        pub is_directory: bool,
        pub is_symbolic_link: bool,
        pub mtime: i64,
        pub ctime: i64,
        pub size: u64,
        pub readonly: bool,
        pub locked: bool,
        pub executable: bool,
        pub etag: String,
    }

    /// `INotebookPartialFileStatsWithMetadata | files.FileOperationError`: o retorno de `$saveNotebook`.
    #[derive(Clone, Debug)]
    pub enum NotebookSaveResult {
        Stats(INotebookPartialFileStatsWithMetadata),
        Error(FileOperationError),
    }

    /// `NotebookRawContentEventDto`.
    #[derive(Clone, Debug)]
    pub enum NotebookRawContentEventDto {
        ModelChange { changes: Vec<NotebookCellTextModelSplice<NotebookCellDto>> },
        Move { index: usize, length: usize, new_idx: usize },
        Output { index: usize, outputs: Vec<NotebookOutputDto> },
        OutputItem { index: usize, output_id: String, output_items: Vec<NotebookOutputItemDto>, append: bool },
        ChangeCellLanguage { index: usize, language: String },
        ChangeCellMime { index: usize, mime: Option<String> },
        ChangeCellMetadata { index: usize, metadata: NotebookCellMetadata },
        ChangeCellInternalMetadata { index: usize, internal_metadata: NotebookCellInternalMetadata },
        ChangeCellContent { index: usize },
    }

    /// `NotebookCellsChangedEventDto`.
    #[derive(Clone, Debug)]
    pub struct NotebookCellsChangedEventDto {
        pub raw_events: Vec<NotebookRawContentEventDto>,
        pub version_id: i64,
    }

    /// `ExtHostNotebookDocumentsAndEditorsShape`.
    pub trait ExtHostNotebookDocumentsAndEditorsShape {
        fn rpc_accept_document_and_editors_delta(&self, delta: SerializableObjectWithBuffers<INotebookDocumentsAndEditorsDelta>);
    }

    /// `ExtHostNotebookShape`.
    pub trait ExtHostNotebookShape: ExtHostNotebookDocumentsAndEditorsShape {
        fn rpc_provide_notebook_cell_status_bar_items(&self, handle: u32, uri: &URI, index: usize, token: CancellationToken) -> Promise<Option<INotebookCellStatusBarListDto>>;
        fn rpc_release_notebook_cell_status_bar_items(&self, id: u32);

        fn rpc_data_to_notebook(&self, handle: u32, data: VSBuffer, token: CancellationToken) -> Promise<SerializableObjectWithBuffers<NotebookDataDto>>;
        fn rpc_notebook_to_data(&self, handle: u32, data: SerializableObjectWithBuffers<NotebookDataDto>, token: CancellationToken) -> Promise<VSBuffer>;
        fn rpc_save_notebook(&self, handle: u32, uri: &URI, version_id: i64, options: WriteFileOptions, token: CancellationToken) -> Promise<NotebookSaveResult>;
    }

    /// `ExtHostNotebookDocumentsShape`. O `newMetadata?` é `Option<Dynamic>`.
    pub trait ExtHostNotebookDocumentsShape {
        fn rpc_accept_model_changed(&self, uri: &URI, event: SerializableObjectWithBuffers<NotebookCellsChangedEventDto>, is_dirty: bool, new_metadata: Option<Dynamic>);
        fn rpc_accept_dirty_state_changed(&self, uri: &URI, is_dirty: bool);
        fn rpc_accept_model_saved(&self, uri: &URI);
    }

    impl MainContext {
        pub const MAIN_THREAD_NOTEBOOK: ProxyIdentifier<dyn MainThreadNotebookShape> = ProxyIdentifier::new("MainThreadNotebook");
        pub const MAIN_THREAD_NOTEBOOK_DOCUMENTS: ProxyIdentifier<dyn MainThreadNotebookDocumentsShape> = ProxyIdentifier::new("MainThreadNotebookDocumentsShape");
        pub const MAIN_THREAD_NOTEBOOK_EDITORS: ProxyIdentifier<dyn MainThreadNotebookEditorsShape> = ProxyIdentifier::new("MainThreadNotebookEditorsShape");
    }

    impl ExtHostContext {
        pub const EXT_HOST_NOTEBOOK: ProxyIdentifier<dyn ExtHostNotebookShape> = ProxyIdentifier::new("ExtHostNotebook");
        pub const EXT_HOST_NOTEBOOK_DOCUMENTS: ProxyIdentifier<dyn ExtHostNotebookDocumentsShape> = ProxyIdentifier::new("ExtHostNotebookDocuments");
    }
}

// #endregion

// #region Webviews (painéis, views e mensagens)

pub use self::webviews_region::*;

/// Região de `extHost.protocol.ts` que `extHostWebview.ts`, `extHostWebviewPanels.ts`, `extHostWebviewView.ts` e
/// `extHostWebviewMessaging.ts` usam (e os `MainThreadWebviews*` do workbench): `MainThreadWebviewsShape`,
/// `MainThreadWebviewPanelsShape`, `MainThreadWebviewViewsShape`, `ExtHostWebviewsShape`, `ExtHostWebviewPanelsShape`,
/// `ExtHostWebviewViewsShape`, os DTOs e os identificadores `MainContext.MainThreadWebviews*` e
/// `ExtHostContext.ExtHostWebviews*`. `WebviewHandle`, `WebviewExtensionDescription`, `IWebviewContentOptions` e
/// `IWebviewPanelOptions` já vêm da região dos editores personalizados.
///
/// Divergências:
/// - `UriComponents` é a [`URI`]; `VSBuffer` é o [`VSBuffer`]; o `any` do estado (`state`) é [`Dynamic`].
/// - `$postMessage(handle, value, ...buffers)` recebe os buffers num `Vec`.
/// - `WebviewPanelViewStateData` (um `Record` por handle) é um `Vec` de pares na ordem de inserção das chaves, que é a
///   ordem que `Object.keys` devolve.
/// - `IWebviewIconPath` (`ThemeIcon | { light, dark }`) é o enum [`WebviewIconPathDto`]. O `initData` anônimo de
///   `$deserializeWebviewPanel` é [`WebviewDeserializeInitData`].
/// - `WebviewMessageArrayBufferViewType` (um `const enum` numérico) é um enum com os mesmos valores, e a referência a um
///   `ArrayBuffer` é [`WebviewMessageArrayBufferReference`].
/// - `WebviewEditorCapabilities` (que o `extHost.protocol.ts` declara e nenhum arquivo deste porte lê) não entra.
mod webviews_region {
    use je_base::buffer::VSBuffer;
    use je_base::dynamic::Dynamic;
    use je_base::lifecycle::IDisposable;
    use je_base::promise::Promise;
    use je_base::themables::ThemeIcon;
    use je_base::uri::URI;

    use super::{
        EditorGroupColumn, ExtHostContext, MainContext, ProxyIdentifier, SerializableObjectWithBuffers, WebviewContentOptions, WebviewExtensionDescription,
        WebviewHandle, WebviewPanelOptions,
    };
    use crate::common::views::IViewBadge;

    /// `WebviewPanelShowOptions`.
    #[derive(Clone, Copy, Debug, Default, PartialEq)]
    pub struct WebviewPanelShowOptions {
        pub view_column: Option<EditorGroupColumn>,
        pub preserve_focus: Option<bool>,
    }

    /// `IWebviewIconPath`: `ThemeIcon | { light: UriComponents; dark: UriComponents }`.
    #[derive(Clone, Debug, PartialEq)]
    pub enum WebviewIconPathDto {
        ThemeIcon(ThemeIcon),
        LightDark { light: URI, dark: URI },
    }

    /// `IWebviewInitData`.
    #[derive(Clone, Debug, PartialEq)]
    pub struct WebviewInitData {
        pub title: String,
        pub webview_options: WebviewContentOptions,
        pub panel_options: WebviewPanelOptions,
        pub serialize_buffers_for_post_message: bool,
    }

    /// O valor de cada chave de `WebviewPanelViewStateData`.
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct WebviewPanelViewState {
        pub active: bool,
        pub visible: bool,
        pub position: EditorGroupColumn,
    }

    /// `WebviewPanelViewStateData`: um `Record<handle, ...>`, na ordem de inserção.
    pub type WebviewPanelViewStateData = Vec<(WebviewHandle, WebviewPanelViewState)>;

    /// O `initData` anônimo de `$deserializeWebviewPanel`.
    #[derive(Clone, Debug)]
    pub struct WebviewDeserializeInitData {
        pub title: String,
        pub state: Dynamic,
        pub webview_options: WebviewContentOptions,
        pub panel_options: WebviewPanelOptions,
        pub active: bool,
    }

    /// O `options` de `$registerWebviewViewProvider`: `{ retainContextWhenHidden?; serializeBuffersForPostMessage }`.
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct WebviewViewProviderOptions {
        pub retain_context_when_hidden: Option<bool>,
        pub serialize_buffers_for_post_message: bool,
    }

    /// `WebviewMessageArrayBufferViewType`.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum WebviewMessageArrayBufferViewType {
        Int8Array = 1,
        Uint8Array = 2,
        Uint8ClampedArray = 3,
        Int16Array = 4,
        Uint16Array = 5,
        Int32Array = 6,
        Uint32Array = 7,
        Float32Array = 8,
        Float64Array = 9,
        BigInt64Array = 10,
        BigUint64Array = 11,
    }

    impl WebviewMessageArrayBufferViewType {
        /// O valor numérico que trafega no JSON.
        pub fn value(self) -> u8 {
            self as u8
        }

        /// O tipo que o número nomeia, ou `None` (o `default: throw new Error('Unknown array buffer view type')`).
        pub fn from_value(value: f64) -> Option<Self> {
            if value.fract() != 0.0 {
                return None;
            }
            Some(match value as i64 {
                1 => Self::Int8Array,
                2 => Self::Uint8Array,
                3 => Self::Uint8ClampedArray,
                4 => Self::Int16Array,
                5 => Self::Uint16Array,
                6 => Self::Int32Array,
                7 => Self::Uint32Array,
                8 => Self::Float32Array,
                9 => Self::Float64Array,
                10 => Self::BigInt64Array,
                11 => Self::BigUint64Array,
                _ => return None,
            })
        }
    }

    /// O `view` de `WebviewMessageArrayBufferReference`.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct WebviewMessageArrayBufferView {
        pub view_type: WebviewMessageArrayBufferViewType,
        pub byte_length: usize,
        pub byte_offset: usize,
    }

    /// `WebviewMessageArrayBufferReference` (o `$$vscode_array_buffer_reference$$: true` é implícito).
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct WebviewMessageArrayBufferReference {
        pub index: usize,
        pub view: Option<WebviewMessageArrayBufferView>,
    }

    /// `MainThreadWebviewsShape`.
    pub trait MainThreadWebviewsShape: IDisposable {
        fn rpc_set_html(&self, handle: &str, value: &str);
        fn rpc_set_options(&self, handle: &str, options: WebviewContentOptions);
        fn rpc_post_message(&self, handle: &str, value: &str, buffers: Vec<VSBuffer>) -> Promise<bool>;
    }

    /// `MainThreadWebviewPanelsShape`.
    pub trait MainThreadWebviewPanelsShape: IDisposable {
        fn rpc_create_webview_panel(
            &self,
            extension: WebviewExtensionDescription,
            handle: &str,
            view_type: &str,
            init_data: WebviewInitData,
            show_options: WebviewPanelShowOptions,
        );
        fn rpc_dispose_webview(&self, handle: &str);
        fn rpc_reveal(&self, handle: &str, show_options: WebviewPanelShowOptions);
        fn rpc_set_title(&self, handle: &str, value: &str);
        fn rpc_set_icon_path(&self, handle: &str, value: Option<WebviewIconPathDto>);

        fn rpc_register_serializer(&self, view_type: &str, options: WebviewSerializerOptions);
        fn rpc_unregister_serializer(&self, view_type: &str);
    }

    /// O `options` de `$registerSerializer`: `{ serializeBuffersForPostMessage: boolean }`.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct WebviewSerializerOptions {
        pub serialize_buffers_for_post_message: bool,
    }

    /// `MainThreadWebviewViewsShape`.
    pub trait MainThreadWebviewViewsShape: IDisposable {
        fn rpc_register_webview_view_provider(&self, extension: WebviewExtensionDescription, view_type: &str, options: WebviewViewProviderOptions);
        fn rpc_unregister_webview_view_provider(&self, view_type: &str);

        fn rpc_set_webview_view_title(&self, handle: &str, value: Option<&str>);
        fn rpc_set_webview_view_description(&self, handle: &str, value: Option<&str>);
        fn rpc_set_webview_view_badge(&self, handle: &str, badge: Option<IViewBadge>);

        fn rpc_show(&self, handle: &str, preserve_focus: bool);
    }

    /// `ExtHostWebviewsShape`.
    pub trait ExtHostWebviewsShape {
        fn rpc_on_message(&self, handle: &str, json_serialized_message: &str, buffers: SerializableObjectWithBuffers<Vec<VSBuffer>>);
        fn rpc_on_missing_csp(&self, handle: &str, extension_id: &str);
    }

    /// `ExtHostWebviewPanelsShape`.
    pub trait ExtHostWebviewPanelsShape {
        fn rpc_on_did_change_webview_panel_view_states(&self, new_state: WebviewPanelViewStateData);
        fn rpc_on_did_dispose_webview_panel(&self, handle: &str) -> Promise<()>;
        fn rpc_deserialize_webview_panel(&self, new_webview_handle: &str, view_type: &str, init_data: WebviewDeserializeInitData, position: EditorGroupColumn) -> Promise<()>;
    }

    /// `ExtHostWebviewViewsShape`.
    pub trait ExtHostWebviewViewsShape {
        fn rpc_resolve_webview_view(
            &self,
            webview_handle: &str,
            view_type: &str,
            title: Option<String>,
            state: Dynamic,
            cancellation: je_base::cancellation::CancellationToken,
        ) -> Promise<()>;
        fn rpc_on_did_change_webview_view_visibility(&self, webview_handle: &str, visible: bool);
        fn rpc_dispose_webview_view(&self, webview_handle: &str);
    }

    impl MainContext {
        pub const MAIN_THREAD_WEBVIEWS: ProxyIdentifier<dyn MainThreadWebviewsShape> = ProxyIdentifier::new("MainThreadWebviews");
        pub const MAIN_THREAD_WEBVIEW_PANELS: ProxyIdentifier<dyn MainThreadWebviewPanelsShape> = ProxyIdentifier::new("MainThreadWebviewPanels");
        pub const MAIN_THREAD_WEBVIEW_VIEWS: ProxyIdentifier<dyn MainThreadWebviewViewsShape> = ProxyIdentifier::new("MainThreadWebviewViews");
    }

    impl ExtHostContext {
        pub const EXT_HOST_WEBVIEWS: ProxyIdentifier<dyn ExtHostWebviewsShape> = ProxyIdentifier::new("ExtHostWebviews");
        pub const EXT_HOST_WEBVIEW_PANELS: ProxyIdentifier<dyn ExtHostWebviewPanelsShape> = ProxyIdentifier::new("ExtHostWebviewPanels");
        pub const EXT_HOST_WEBVIEW_VIEWS: ProxyIdentifier<dyn ExtHostWebviewViewsShape> = ProxyIdentifier::new("ExtHostWebviewViews");
    }
}

// #endregion

// #region ExtHostEditors (propriedades, posição e diff dos editores de texto)

pub use self::editors_region::*;

/// Região de `extHost.protocol.ts` que `mainThreadEditor.ts` e `mainThreadEditors.ts` usam do lado do host:
/// `ExtHostEditorsShape`, `IEditorPropertiesChangeData`, `ISelectionChangeEvent`, `ITextEditorPositionData`,
/// `ITextEditorChange`, `ITextEditorDiffInformation` (de `platform/editor/common/editor.ts`) e o identificador
/// `ExtHostContext.ExtHostEditors`.
///
/// Divergências:
/// - `ITextEditorPositionData` (`{ [id: string]: EditorGroupColumn }`) é um [`BTreeMap`] por id: a comparação do
///   `objects.equals` do original não depende da ordem das chaves, e o mapa ordenado também não.
/// - `ITextEditorChange` (uma tupla de quatro números) é `[i32; 4]`, e o `documentVersion` é `i64`, como o
///   `versionId` do modelo.
/// - `ISelectionChangeEvent.source?` é `Option<String>`. `IEditorPropertiesChangeData` guarda cada campo
///   `null` do original como `None`.
/// - `original: UriComponents | undefined` do DTO e `original: URI | undefined` do `ITextEditorDiffInformation` da
///   plataforma são o mesmo [`ITextEditorDiffInformation`], com [`URI`] já revivida.
mod editors_region {
    use std::collections::BTreeMap;

    use je_base::uri::URI;
    use je_editor::range::Range as EditorRange;
    use je_editor::selection::Selection as EditorSelection;

    use super::documents_and_editors_region::ResolvedTextEditorConfiguration;
    use super::{EditorGroupColumn, ExtHostContext, ProxyIdentifier};

    /// `ISelectionChangeEvent`.
    #[derive(Clone, Debug, PartialEq)]
    pub struct SelectionChangeEvent {
        pub selections: Vec<EditorSelection>,
        pub source: Option<String>,
    }

    /// `IEditorPropertiesChangeData`.
    #[derive(Clone, Debug, PartialEq)]
    pub struct EditorPropertiesChangeData {
        pub options: Option<ResolvedTextEditorConfiguration>,
        pub selections: Option<SelectionChangeEvent>,
        pub visible_ranges: Option<Vec<EditorRange>>,
    }

    /// `ITextEditorPositionData`.
    pub type TextEditorPositionData = BTreeMap<String, EditorGroupColumn>;

    /// `ITextEditorChange`: `[originalStartLineNumber, originalEndLineNumberExclusive, modifiedStartLineNumber,
    /// modifiedEndLineNumberExclusive]`.
    pub type TextEditorChange = [i32; 4];

    /// `ITextEditorDiffInformation`.
    #[derive(Clone, Debug, PartialEq)]
    pub struct ITextEditorDiffInformation {
        pub document_version: i64,
        pub original: Option<URI>,
        pub modified: URI,
        pub changes: Vec<TextEditorChange>,
    }

    /// `ExtHostEditorsShape`.
    pub trait ExtHostEditorsShape {
        fn rpc_accept_editor_properties_changed(&self, id: &str, props: EditorPropertiesChangeData);
        fn rpc_accept_editor_position_data(&self, data: TextEditorPositionData);
        fn rpc_accept_editor_diff_information(&self, id: &str, diff_information: Option<Vec<ITextEditorDiffInformation>>);
    }

    impl ExtHostContext {
        pub const EXT_HOST_EDITORS: ProxyIdentifier<dyn ExtHostEditorsShape> = ProxyIdentifier::new("ExtHostEditors");
    }
}

// #endregion

// #region NotebookKernels

pub use self::notebook_kernels_region::*;

/// Os DTOs e as formas de `extHost.protocol.ts` que `extHostNotebookKernels.ts` usa (`INotebookKernelDto2`,
/// `ICellExecuteUpdateDto`, `ICellExecutionCompleteDto`, `VariablesResult`, `MainThreadNotebookKernelsShape` e
/// `ExtHostNotebookKernelsShape`).
///
/// Divergências:
/// - Os `UriComponents` são [`URI`]; os handles de kernel e de execução (`number`) são `u32`, o de célula é `i64`, como em
///   [`NotebookCellDto`].
/// - `$updateKernel(handle, data: Partial<INotebookKernelDto2>)` recebe o [`INotebookKernelDto2`] inteiro: o controlador
///   sempre manda o objeto completo (um `Partial` com todos os campos).
/// - `ICellExecuteUpdateDto` (`ICellExecuteOutputEditDto | ICellExecuteOutputItemEditDto | ICellExecutionStateUpdateDto`,
///   união discriminada por `editType`) é o `enum` [`ICellExecuteUpdateDto`]; o `editType` de cada variante é
///   [`ICellExecuteUpdateDto::edit_type`] e os structs não o repetem. `ICellExecutionStateUpdateDto` é o
///   [`ICellExecutionStateUpdate`] e `ICellExecutionCompleteDto` é o [`ICellExecutionComplete`] (os dois `extends` vazios).
/// - O `$updateNotebookPriority(handle, uri, value: number | undefined)` recebe o `NotebookControllerAffinity2`.
/// - `VariablesResult` leva também `interfaces`: o tipo declarado no original não tem o campo, mas o `$provideVariables`
///   o manda (o objeto não é checado contra o tipo).
/// - O `'named' | 'indexed'` de `$provideVariables` é [`VariablesRequestKindDto`]; o `message: any` é [`Dynamic`].
mod notebook_kernels_region {
    use je_base::cancellation::CancellationToken;
    use je_base::dynamic::Dynamic;
    use je_base::lifecycle::IDisposable;
    use je_base::promise::Promise;
    use je_base::uri::URI;
    use je_extensions::manifest::ExtensionIdentifier;

    use super::{ExtHostContext, MainContext, NotebookOutputDto, NotebookOutputItemDto, ProxyIdentifier, SerializableObjectWithBuffers};
    use crate::api::common::ext_host_types::NotebookControllerAffinity2;
    use crate::contrib::notebook::common::notebook_common::INotebookKernelSourceAction;
    use crate::contrib::notebook::common::notebook_execution_service::CellExecutionUpdateType;
    use crate::contrib::notebook::common::notebook_execution_state_service::{ICellExecutionComplete, ICellExecutionStateUpdate};

    /// `{ uri: UriComponents; provides: readonly string[] }`: um item de `INotebookKernelDto2.preloads`.
    #[derive(Clone, Debug)]
    pub struct NotebookRendererScriptDto {
        pub uri: URI,
        pub provides: Vec<String>,
    }

    /// `INotebookKernelDto2`.
    #[derive(Clone, Debug)]
    pub struct INotebookKernelDto2 {
        pub id: String,
        pub notebook_type: String,
        pub extension_id: ExtensionIdentifier,
        pub extension_location: URI,
        pub label: String,
        pub detail: Option<String>,
        pub description: Option<String>,
        pub supported_languages: Option<Vec<String>>,
        pub supports_interrupt: Option<bool>,
        pub supports_execution_order: Option<bool>,
        pub preloads: Option<Vec<NotebookRendererScriptDto>>,
        pub has_variable_provider: Option<bool>,
    }

    /// `ICellExecuteOutputEditDto`: o `editType` é [`CellExecutionUpdateType::Output`].
    #[derive(Clone, Debug)]
    pub struct ICellExecuteOutputEditDto {
        pub cell_handle: i64,
        pub append: Option<bool>,
        pub outputs: Vec<NotebookOutputDto>,
    }

    /// `ICellExecuteOutputItemEditDto`: o `editType` é [`CellExecutionUpdateType::OutputItems`].
    #[derive(Clone, Debug)]
    pub struct ICellExecuteOutputItemEditDto {
        pub append: Option<bool>,
        pub output_id: String,
        pub items: Vec<NotebookOutputItemDto>,
    }

    /// `ICellExecuteUpdateDto`.
    #[derive(Clone, Debug)]
    pub enum ICellExecuteUpdateDto {
        Output(ICellExecuteOutputEditDto),
        OutputItems(ICellExecuteOutputItemEditDto),
        ExecutionState(ICellExecutionStateUpdate),
    }

    impl ICellExecuteUpdateDto {
        /// O `editType` da atualização.
        pub fn edit_type(&self) -> CellExecutionUpdateType {
            match self {
                Self::Output(_) => CellExecutionUpdateType::Output,
                Self::OutputItems(_) => CellExecutionUpdateType::OutputItems,
                Self::ExecutionState(_) => CellExecutionUpdateType::ExecutionState,
            }
        }
    }

    /// `ICellExecutionCompleteDto`.
    pub type ICellExecutionCompleteDto = ICellExecutionComplete;

    /// `VariablesResult`.
    #[derive(Clone, Debug)]
    pub struct VariablesResult {
        pub id: u32,
        pub name: String,
        pub value: String,
        pub r#type: Option<String>,
        pub interfaces: Option<Vec<String>>,
        pub language: Option<String>,
        pub expression: Option<String>,
        pub has_named_children: bool,
        pub indexed_children_count: usize,
        pub extension_id: String,
    }

    /// `'named' | 'indexed'`: o `kind` de `$provideVariables`.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum VariablesRequestKindDto {
        Named,
        Indexed,
    }

    /// `MainThreadNotebookKernelsShape`.
    pub trait MainThreadNotebookKernelsShape: IDisposable {
        fn rpc_post_message(&self, handle: u32, editor_id: Option<&str>, message: Dynamic) -> Promise<bool>;
        fn rpc_add_kernel(&self, handle: u32, data: INotebookKernelDto2) -> Promise<()>;
        fn rpc_update_kernel(&self, handle: u32, data: INotebookKernelDto2);
        fn rpc_remove_kernel(&self, handle: u32);
        fn rpc_update_notebook_priority(&self, handle: u32, uri: &URI, value: Option<NotebookControllerAffinity2>);

        fn rpc_create_execution(&self, handle: u32, controller_id: &str, uri: &URI, cell_handle: i64);
        fn rpc_update_execution(&self, handle: u32, data: SerializableObjectWithBuffers<Vec<ICellExecuteUpdateDto>>);
        fn rpc_complete_execution(&self, handle: u32, data: SerializableObjectWithBuffers<ICellExecutionCompleteDto>);

        fn rpc_create_notebook_execution(&self, handle: u32, controller_id: &str, uri: &URI);
        fn rpc_begin_notebook_execution(&self, handle: u32);
        fn rpc_complete_notebook_execution(&self, handle: u32);

        fn rpc_add_kernel_detection_task(&self, handle: u32, notebook_type: &str) -> Promise<()>;
        fn rpc_remove_kernel_detection_task(&self, handle: u32);

        fn rpc_add_kernel_source_action_provider(&self, handle: u32, event_handle: u32, notebook_type: &str) -> Promise<()>;
        fn rpc_remove_kernel_source_action_provider(&self, handle: u32, event_handle: u32);
        fn rpc_emit_notebook_kernel_source_actions_change_event(&self, event_handle: u32);
        fn rpc_receive_variable(&self, request_id: &str, variable: VariablesResult);
        fn rpc_variables_updated(&self, notebook_uri: &URI);
    }

    /// `ExtHostNotebookKernelsShape`.
    pub trait ExtHostNotebookKernelsShape {
        fn rpc_accept_notebook_association(&self, handle: u32, uri: &URI, value: bool);
        fn rpc_execute_cells(&self, handle: u32, uri: &URI, handles: Vec<i64>) -> Promise<()>;
        fn rpc_cancel_cells(&self, handle: u32, uri: &URI, handles: Vec<i64>) -> Promise<()>;
        fn rpc_accept_kernel_message_from_renderer(&self, handle: u32, editor_id: &str, message: Dynamic);
        fn rpc_provide_kernel_source_actions(&self, handle: u32, token: CancellationToken) -> Promise<Vec<INotebookKernelSourceAction>>;
        fn rpc_provide_variables(
            &self,
            handle: u32,
            request_id: &str,
            notebook_uri: &URI,
            parent_id: Option<u32>,
            kind: VariablesRequestKindDto,
            start: usize,
            token: CancellationToken,
        ) -> Promise<()>;
    }

    impl MainContext {
        pub const MAIN_THREAD_NOTEBOOK_KERNELS: ProxyIdentifier<dyn MainThreadNotebookKernelsShape> = ProxyIdentifier::new("MainThreadNotebookKernels");
    }

    impl ExtHostContext {
        pub const EXT_HOST_NOTEBOOK_KERNELS: ProxyIdentifier<dyn ExtHostNotebookKernelsShape> = ProxyIdentifier::new("ExtHostNotebookKernels");
    }
}

// #endregion

// #region Decorations

pub use self::decorations_region::*;

/// Região de `extHost.protocol.ts` que `extHostDecorations.ts` usa: `MainThreadDecorationsShape`, `ExtHostDecorationsShape`,
/// `DecorationRequest`, `DecorationData`, `DecorationReply` e os identificadores `MainContext.MainThreadDecorations` e
/// `ExtHostContext.ExtHostDecorations`.
///
/// Divergências:
/// - `UriComponents` é a [`URI`] e o `handle` (`number`) é `u32`.
/// - `DecorationData` (`[boolean, string, string | ThemeIcon, ThemeColor]`, a tupla `[propagate, tooltip, badge, color]`) é um
///   struct com os quatro campos na mesma ordem, todos opcionais como os da `FileDecoration` de que vêm; o `badge`
///   (`string | ThemeIcon`) é o [`FileDecorationBadge`].
/// - `DecorationReply` (`{ [id: number]: DecorationData }`) é um [`BTreeMap`] por `id`.
/// - `$onDidChange(handle, resources: UriComponents[] | null)` recebe `None` no lugar de `null`.
mod decorations_region {
    use std::collections::BTreeMap;

    use je_base::cancellation::CancellationToken;
    use je_base::lifecycle::IDisposable;
    use je_base::promise::Promise;
    use je_base::themables::ThemeColor;
    use je_base::uri::URI;

    use super::{ExtHostContext, MainContext, ProxyIdentifier};
    use crate::api::common::ext_host_types::FileDecorationBadge;

    /// `DecorationRequest`.
    #[derive(Clone, Debug)]
    pub struct DecorationRequest {
        pub id: u32,
        pub uri: URI,
    }

    /// `DecorationData`: `[propagate, tooltip, badge, color]`.
    #[derive(Clone)]
    pub struct DecorationData {
        pub propagate: Option<bool>,
        pub tooltip: Option<String>,
        pub badge: Option<FileDecorationBadge>,
        pub color: Option<ThemeColor>,
    }

    /// `DecorationReply`.
    pub type DecorationReply = BTreeMap<u32, DecorationData>;

    /// `MainThreadDecorationsShape`.
    pub trait MainThreadDecorationsShape: IDisposable {
        fn rpc_register_decoration_provider(&self, handle: u32, label: &str);
        fn rpc_unregister_decoration_provider(&self, handle: u32);
        fn rpc_on_did_change(&self, handle: u32, resources: Option<Vec<URI>>);
    }

    /// `ExtHostDecorationsShape`.
    pub trait ExtHostDecorationsShape {
        fn rpc_provide_decorations(&self, handle: u32, requests: Vec<DecorationRequest>, token: CancellationToken) -> Promise<DecorationReply>;
    }

    impl MainContext {
        pub const MAIN_THREAD_DECORATIONS: ProxyIdentifier<dyn MainThreadDecorationsShape> = ProxyIdentifier::new("MainThreadDecorations");
    }

    impl ExtHostContext {
        pub const EXT_HOST_DECORATIONS: ProxyIdentifier<dyn ExtHostDecorationsShape> = ProxyIdentifier::new("ExtHostDecorations");
    }
}

// #endregion

// #region Errors e ExtensionService (host de extensões)

pub use self::extension_host_errors_region::*;

/// `MainThreadErrorsShape` e `MainThreadExtensionServiceShape` com os identificadores `MainContext.MainThreadErrors` e
/// `MainContext.MainThreadExtensionService`.
///
/// Divergências: `$onUnexpectedError(err: any | SerializedError)` recebe o [`SerializedError`] (o ramo `any` do original só
/// existe porque o chamador pode passar qualquer valor); `Dto<IExtensionDescription>` é o `ExtensionDescription`;
/// `performance.PerformanceMark` é o `PerformanceMark` do `je-base`; as `UriComponents` já chegam revividas como [`URI`].
mod extension_host_errors_region {
    use je_base::errors::SerializedError;
    use je_base::lifecycle::IDisposable;
    use je_base::performance::PerformanceMark;
    use je_base::promise::Promise;
    use je_base::uri::URI;
    use je_extensions::manifest::ExtensionIdentifier;
    use je_extensions::scanner::ExtensionDescription;

    use super::{MainContext, ProxyIdentifier};
    use crate::services::extensions::extension_activation::{ExtensionActivationReason, MissingExtensionDependency};

    /// `MainThreadErrorsShape`.
    pub trait MainThreadErrorsShape: IDisposable {
        fn rpc_on_unexpected_error(&self, err: SerializedError);
    }

    /// `MainThreadExtensionServiceShape`.
    pub trait MainThreadExtensionServiceShape: IDisposable {
        fn rpc_get_extension(&self, extension_id: &str) -> Promise<Option<ExtensionDescription>>;
        fn rpc_activate_extension(&self, extension_id: &ExtensionIdentifier, reason: &ExtensionActivationReason) -> Promise<()>;
        fn rpc_on_will_activate_extension(&self, extension_id: &ExtensionIdentifier) -> Promise<()>;
        fn rpc_on_did_activate_extension(
            &self,
            extension_id: &ExtensionIdentifier,
            code_loading_time: f64,
            activate_call_time: f64,
            activate_resolved_time: f64,
            activation_reason: &ExtensionActivationReason,
        );
        fn rpc_on_extension_activation_error(
            &self,
            extension_id: &ExtensionIdentifier,
            error: SerializedError,
            missing_extension_dependency: Option<MissingExtensionDependency>,
        ) -> Promise<()>;
        fn rpc_on_extension_runtime_error(&self, extension_id: &ExtensionIdentifier, error: SerializedError);
        fn rpc_set_performance_marks(&self, marks: Vec<PerformanceMark>) -> Promise<()>;
        fn rpc_as_browser_uri(&self, uri: &URI) -> Promise<URI>;
    }

    impl MainContext {
        pub const MAIN_THREAD_ERRORS: ProxyIdentifier<dyn MainThreadErrorsShape> = ProxyIdentifier::new("MainThreadErrors");
        pub const MAIN_THREAD_EXTENSION_SERVICE: ProxyIdentifier<dyn MainThreadExtensionServiceShape> =
            ProxyIdentifier::new("MainThreadExtensionService");
    }
}

// #endregion

// #region Authentication (DTOs de sessão)

pub use self::authentication_region::*;

/// Região de `extHost.protocol.ts` que `mainThreadAuthentication.ts` e `extHostAuthentication.ts` usam para as sessões:
/// `Dto<AuthenticationSession>` e `Dto<AuthenticationSessionAccount>` (o `Dto<T>` de `proxyIdentifier.ts` troca cada `URI` por
/// `UriComponents`). As formas `MainThreadAuthenticationShape` e `ExtHostAuthenticationShape` e os identificadores
/// entram junto do porte do restante desses dois arquivos (ver o cabeçalho de `api/browser/main_thread_authentication.rs`).
mod authentication_region {
    use je_base::uri::UriComponents;

    /// `Dto<AuthenticationSessionAccount>`: `icon` chega como componentes e é revivido no destino.
    #[derive(Clone, Debug, PartialEq)]
    pub struct AuthenticationSessionAccountDto {
        pub label: String,
        pub id: String,
        pub icon: Option<UriComponents>,
    }

    /// `Dto<AuthenticationSession>`: `account.icon` e `authorizationServer` chegam como componentes.
    #[derive(Clone, Debug, PartialEq)]
    pub struct AuthenticationSessionDto {
        pub id: String,
        pub access_token: String,
        pub account: AuthenticationSessionAccountDto,
        pub scopes: Vec<String>,
        pub id_token: Option<String>,
        pub expires_after: Option<f64>,
        pub authorization_server: Option<UriComponents>,
    }
}

// #endregion

// #region Debug

pub use super::ext_host_debug_protocol::*;

// #endregion

// #region Share

pub use self::share_region::*;

/// `MainThreadShareShape`, `ExtHostShareShape` e os identificadores `MainContext.MainThreadShare` e `ExtHostContext.ExtHostShare` (de
/// `extHost.protocol.ts`, usados por `mainThreadShare.ts`).
///
/// Divergências:
/// - `IShareableItemDto` (`{ resourceUri: UriComponents, selection?: IRange }`) é o [`IShareableItem`] do serviço: o `UriComponents`
///   é a [`URI`] (os dois lados dividem o processo e o `revive` é a identidade) e o `Selection` do original já é um `Range`.
/// - O `Promise<UriComponents | string | undefined>` de `$provideShare` é `Promise<Option<ShareResult>>`.
/// - `IDocumentFilterDto[]` é o `Vec<`[`IDocumentFilterDto`]`>` já portado para os provedores de linguagem.
mod share_region {
    use je_base::cancellation::CancellationToken;
    use je_base::lifecycle::IDisposable;
    use je_base::promise::Promise;

    use super::{ExtHostContext, IDocumentFilterDto, MainContext, ProxyIdentifier};
    use crate::contrib::share::common::share::{IShareableItem, ShareResult};

    /// `MainThreadShareShape`.
    pub trait MainThreadShareShape: IDisposable {
        fn rpc_register_share_provider(&self, handle: u32, selector: Vec<IDocumentFilterDto>, id: &str, label: &str, priority: i32);
        fn rpc_unregister_share_provider(&self, handle: u32);
    }

    /// `ExtHostShareShape`.
    pub trait ExtHostShareShape {
        fn rpc_provide_share(&self, handle: u32, item: IShareableItem, token: CancellationToken) -> Promise<Option<ShareResult>>;
    }

    impl MainContext {
        pub const MAIN_THREAD_SHARE: ProxyIdentifier<dyn MainThreadShareShape> = ProxyIdentifier::new("MainThreadShare");
    }

    impl ExtHostContext {
        pub const EXT_HOST_SHARE: ProxyIdentifier<dyn ExtHostShareShape> = ProxyIdentifier::new("ExtHostShare");
    }
}

// #endregion

// #region UriOpeners

pub use self::uri_openers_region::*;

/// Região de `extHost.protocol.ts` que `extHostUriOpener.ts` e `mainThreadUriOpeners.ts` usam: `MainThreadUriOpenersShape`,
/// `ExtHostUriOpenersShape` e os identificadores `MainContext.MainThreadUriOpeners` e `ExtHostContext.ExtHostUriOpeners`.
///
/// Divergências:
/// - `UriComponents` é a [`URI`] e o `viewColumn?: ViewColumn` do contexto de `$openUri` é o `ViewColumn` do `platform/editor`.
///   O objeto `{ resolvedUri, sourceUri, viewColumn? }` é [`OpenUriContextDto`].
/// - `readonly string[]` de `$registerUriOpener` é `&[String]`.
mod uri_openers_region {
    use je_base::cancellation::CancellationToken;
    use je_base::lifecycle::IDisposable;
    use je_base::promise::Promise;
    use je_base::uri::URI;
    use je_editor::languages::ExternalUriOpenerPriority;
    use je_extensions::manifest::ExtensionIdentifier;
    use je_platform::opener::editor_types::ViewColumn;

    use super::{ExtHostContext, MainContext, ProxyIdentifier};

    /// O `context` de `$openUri`: `{ resolvedUri: UriComponents; sourceUri: UriComponents; viewColumn?: ViewColumn }`.
    #[derive(Clone, Debug, PartialEq)]
    pub struct OpenUriContextDto {
        pub resolved_uri: URI,
        pub source_uri: URI,
        pub view_column: Option<ViewColumn>,
    }

    /// `MainThreadUriOpenersShape`.
    pub trait MainThreadUriOpenersShape: IDisposable {
        fn rpc_register_uri_opener(&self, id: &str, schemes: &[String], extension_id: &ExtensionIdentifier, label: &str) -> Promise<()>;
        fn rpc_unregister_uri_opener(&self, id: &str) -> Promise<()>;
    }

    /// `ExtHostUriOpenersShape`.
    pub trait ExtHostUriOpenersShape {
        fn rpc_can_open_uri(&self, id: &str, uri: &URI, token: CancellationToken) -> Promise<ExternalUriOpenerPriority>;
        fn rpc_open_uri(&self, id: &str, context: OpenUriContextDto, token: CancellationToken) -> Promise<()>;
    }

    impl MainContext {
        pub const MAIN_THREAD_URI_OPENERS: ProxyIdentifier<dyn MainThreadUriOpenersShape> = ProxyIdentifier::new("MainThreadUriOpeners");
    }

    impl ExtHostContext {
        pub const EXT_HOST_URI_OPENERS: ProxyIdentifier<dyn ExtHostUriOpenersShape> = ProxyIdentifier::new("ExtHostUriOpeners");
    }
}

// #endregion
