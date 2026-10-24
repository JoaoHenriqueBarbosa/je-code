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

use std::any::Any;
use std::marker::PhantomData;
use std::rc::Rc;

use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::errors::Error;
use je_base::lifecycle::IDisposable;
use je_base::promise::Promise;
use je_base::severity::Severity;
use je_base::uri::URI;
use je_extensions::manifest::ExtensionIdentifier;
use je_platform::commands::commands::CommandDescription;
use je_platform::configuration::registry::ConfigurationScope;
use je_platform::configuration::{ConfigurationChange, ConfigurationData, ConfigurationOverrides, ConfigurationTarget};
use je_platform::opener::editor_types::EditorOptions;
use je_platform::request::request::{AuthInfo, Credentials};
use je_platform::workspace_trust::{ResourceTrustRequestOptions, WorkspaceTrustRequestOptions};

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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditSessionIdentityMatch {
    Complete = 100,
    Partial = 50,
    None = 0,
}

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
#[derive(Clone, Debug, PartialEq, Eq)]
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

/// `MainContext`: os identificadores dos proxies da thread principal.
pub struct MainContext;

impl MainContext {
    pub const MAIN_THREAD_BROWSERS: ProxyIdentifier<dyn MainThreadBrowsersShape> = ProxyIdentifier::new("MainThreadBrowsers");
    pub const MAIN_THREAD_COMMANDS: ProxyIdentifier<dyn MainThreadCommandsShape> = ProxyIdentifier::new("MainThreadCommands");
    pub const MAIN_THREAD_CONFIGURATION: ProxyIdentifier<dyn MainThreadConfigurationShape> = ProxyIdentifier::new("MainThreadConfiguration");
    pub const MAIN_THREAD_MESSAGE_SERVICE: ProxyIdentifier<dyn MainThreadMessageServiceShape> = ProxyIdentifier::new("MainThreadMessageService");
    pub const MAIN_THREAD_TELEMETRY: ProxyIdentifier<dyn MainThreadTelemetryShape> = ProxyIdentifier::new("MainThreadTelemetry");
    pub const MAIN_THREAD_WORKSPACE: ProxyIdentifier<dyn MainThreadWorkspaceShape> = ProxyIdentifier::new("MainThreadWorkspace");
}

/// `ExtHostContext`: os identificadores dos proxies do lado das extensões.
pub struct ExtHostContext;

impl ExtHostContext {
    pub const EXT_HOST_COMMANDS: ProxyIdentifier<dyn ExtHostCommandsShape> = ProxyIdentifier::new("ExtHostCommands");
    pub const EXT_HOST_CONFIGURATION: ProxyIdentifier<dyn ExtHostConfigurationShape> = ProxyIdentifier::new("ExtHostConfiguration");
    pub const EXT_HOST_WORKSPACE: ProxyIdentifier<dyn ExtHostWorkspaceShape> = ProxyIdentifier::new("ExtHostWorkspace");
}
