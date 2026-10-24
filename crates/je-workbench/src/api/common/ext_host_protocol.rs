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
