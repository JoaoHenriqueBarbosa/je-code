//! Porte de `src/vs/workbench/services/extensionManagement/common/extensionManagement.ts`: os contratos do
//! gerenciamento de extensões na camada do workbench (`EnablementState`, `IExtensionManagementServer`,
//! `IExtensionManagementServerService`, `IWorkbenchExtensionManagementService` e
//! `IWorkbenchExtensionEnablementService`), mais os recortes dos contratos de plataforma que eles estendem.
//!
//! Recortes (cada um entra inteiro quando o serviço que ele descreve for portado):
//! - **`IExtension`** (`platform/extensions/common/extensions.ts`) e **`ILocalExtension`** são um só modelo: a
//!   [`LocalExtension`] completa de `je-extensions` ([`IExtension`] e [`ILocalExtension`] são apelidos dela). O recorte
//!   reduzido que existia aqui foi removido. A extensão é compartilhada por `Rc` ([`ExtensionRef`]), porque o original
//!   compara extensões por identidade (`Map<IExtension, ...>`, `indexOf`). Uma diferença que resta: o `location` da
//!   `LocalExtension` é um caminho (texto), e o original guarda um `URI` de cujo esquema o servidor depende. A função
//!   [`local_extension_uri`] devolve o `URI`: um `location` que já traz esquema (`vscode-remote://...`) é lido como
//!   URI, e um caminho puro vira `file`. Os caminhos que o scanner de `je-extensions` produz nunca têm `://`. Unificar
//!   de vez exige o `location` virar `URI` na `LocalExtension`, mudança de `je-extensions` que fica para quando o
//!   scanner for portado por inteiro.
//! - **`IExtensionManagementService`** (`platform/extensionManagement/common/extensionManagement.ts`) é o trait
//!   [`IExtensionManagementService`], só com os eventos de instalação e remoção, `getInstalled()` (sem os argumentos
//!   opcionais `type`, `profileLocation`, `productVersion` e `language`, que o gerenciador de habilitação não passa) e
//!   `getExtensionsControlManifest()`.
//! - **`DidUninstallExtensionOnServerEvent` e `DidChangeProfileForServerEvent`** (`DidUninstallExtensionEvent & { server }`
//!   e `DidChangeProfileEvent & { server }`) não são tipos à parte: [`DidUninstallExtensionEvent`] e
//!   [`DidChangeProfileEvent`] carregam o `server` como `Option`, que o serviço da plataforma deixa vazio e o do
//!   workbench preenche. Assim `IWorkbenchExtensionManagementService` é um subtrait de
//!   `IProfileAwareExtensionManagementService`, como no original.
//! - **`IGlobalExtensionEnablementService`** é o trait [`IGlobalExtensionEnablementService`]; a implementação é
//!   [`super::global_extension_enablement_service::GlobalExtensionEnablementService`].
//! - Do `IWorkbenchExtensionManagementService` ficam de fora a instalação, a atualização e a confiança em publishers
//!   (`installVSIX`, `installFromGallery`, `requestPublisherTrust`...): dependem de `IGalleryExtension`,
//!   `InstallOptions` e `IMarkdownString` ligados ao serviço de instalação, que o porte tem em `je-extensions` com
//!   outro formato. Entram com o serviço de gerenciamento do workbench.
//! - O `onProfileAwareDidUpdateExtensionMetadata` também fica de fora (precisa de `DidUpdateExtensionMetadata`).
//! - `DidChangeProfileEvent.added` e `removed` são listas de extensões compartilhadas ([`ExtensionRef`]).

use std::rc::Rc;

use je_base::event::Event;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_extensions::management::{ExtensionsControlManifest, GalleryExtension, InstallOperation, LocalExtension};
use je_extensions::manifest::{ExtensionIdentifierInfo, ExtensionManifest};
use je_instantiation::service_id;

/// `IExtension` (`platform/extensions/common/extensions.ts`): a extensão local completa de `je-extensions`.
pub type IExtension = LocalExtension;

/// `ILocalExtension`.
pub type ILocalExtension = LocalExtension;

/// Uma extensão compartilhada: o original passa o mesmo objeto de mão em mão e compara por identidade.
pub type ExtensionRef = Rc<LocalExtension>;

/// O `location` de uma extensão local como `URI` (ver a nota do módulo sobre o esquema).
pub fn local_extension_uri(extension: &LocalExtension) -> URI {
    if extension.location.contains("://") {
        URI::parse(extension.location.as_str(), false)
    } else {
        URI::file(extension.location.as_str(), je_platform::resources::host_platform())
    }
}

/// `URI | IGalleryExtension`: de onde a extensão vem (`source` dos eventos de instalação).
#[derive(Clone, Debug)]
pub enum InstallExtensionSource {
    Location(URI),
    Gallery(GalleryExtension),
}

/// `IResourceExtension`: uma extensão que vive num recurso (uma pasta do workspace ou um VSIX), sem passar pela galeria
/// nem pelo gerenciador de instalação. O campo `type: 'resource'` do original é o próprio tipo.
#[derive(Clone, Debug)]
pub struct IResourceExtension {
    pub identifier: ExtensionIdentifierInfo,
    pub location: URI,
    pub manifest: ExtensionManifest,
    pub readme_uri: Option<URI>,
    pub changelog_uri: Option<URI>,
}

/// `InstallExtensionResult`: o resultado de uma instalação (`local` é a extensão instalada, quando houve).
#[derive(Clone)]
pub struct InstallExtensionResult {
    pub identifier: ExtensionIdentifierInfo,
    pub operation: InstallOperation,
    pub source: Option<InstallExtensionSource>,
    pub profile_location: Option<URI>,
    pub local: Option<ExtensionRef>,
    /// `workspaceScoped`.
    pub workspace_scoped: Option<bool>,
}

/// `DidUninstallExtensionEvent` (com o `server` do `DidUninstallExtensionOnServerEvent`).
#[derive(Clone)]
pub struct DidUninstallExtensionEvent {
    pub identifier: ExtensionIdentifierInfo,
    pub profile_location: Option<URI>,
    /// A mensagem do erro, se a remoção falhou.
    pub error: Option<String>,
    /// `workspaceScoped`.
    pub workspace_scoped: Option<bool>,
    /// O servidor de onde a extensão saiu (só o serviço do workbench preenche).
    pub server: Option<Rc<IExtensionManagementServer>>,
}

/// `DidChangeProfileEvent` (com o `server` do `DidChangeProfileForServerEvent`).
#[derive(Clone)]
pub struct DidChangeProfileEvent {
    pub added: Vec<ExtensionRef>,
    pub removed: Vec<ExtensionRef>,
    /// O servidor cujo perfil mudou (só o serviço do workbench preenche).
    pub server: Option<Rc<IExtensionManagementServer>>,
}

/// `IExtensionManagementService`, reduzido ao que o gerenciador de habilitação lê (ver a nota do módulo).
pub trait IExtensionManagementService {
    fn on_did_install_extensions(&self) -> Event<Vec<InstallExtensionResult>>;
    fn on_did_uninstall_extension(&self) -> Event<DidUninstallExtensionEvent>;
    fn get_installed(&self) -> Promise<Vec<ExtensionRef>>;
    fn get_extensions_control_manifest(&self) -> Promise<ExtensionsControlManifest>;
}

service_id!(pub I_EXTENSION_MANAGEMENT_SERVICE: dyn IExtensionManagementService = "extensionManagementService");

/// `IProfileAwareExtensionManagementService`.
pub trait IProfileAwareExtensionManagementService: IExtensionManagementService {
    fn on_profile_aware_did_install_extensions(&self) -> Event<Vec<InstallExtensionResult>>;
    fn on_profile_aware_did_uninstall_extension(&self) -> Event<DidUninstallExtensionEvent>;
    fn on_did_change_profile(&self) -> Event<DidChangeProfileEvent>;

    /// Este mesmo serviço como o contrato completo de instalação, para quem instala, atualiza e remove (o original
    /// chama `installFromGallery`, `updateMetadata`... no `IExtensionManagementService` do servidor; aqui o contrato
    /// reduzido não os traz, e o dublê que não instala devolve `None`).
    fn as_install_service(self: Rc<Self>) -> Option<Rc<dyn super::extension_install_service::IExtensionInstallService>> {
        None
    }
}

/// `IExtensionManagementServer`.
pub struct IExtensionManagementServer {
    pub id: String,
    pub label: String,
    pub extension_management_service: Rc<dyn IProfileAwareExtensionManagementService>,
}

/// `ExtensionInstallLocation`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExtensionInstallLocation {
    Local = 1,
    Remote,
    Web,
}

/// `IExtensionManagementServerService`.
pub trait IExtensionManagementServerService {
    fn local_extension_management_server(&self) -> Option<Rc<IExtensionManagementServer>>;
    fn remote_extension_management_server(&self) -> Option<Rc<IExtensionManagementServer>>;
    fn web_extension_management_server(&self) -> Option<Rc<IExtensionManagementServer>>;
    fn get_extension_management_server(&self, extension: &IExtension) -> Option<Rc<IExtensionManagementServer>>;
    fn get_extension_install_location(&self, extension: &IExtension) -> Option<ExtensionInstallLocation>;
}

service_id!(pub I_EXTENSION_MANAGEMENT_SERVER_SERVICE: dyn IExtensionManagementServerService = "extensionManagementServerService");

/// `IWorkbenchExtensionManagementService`, reduzido ao que o gerenciador de habilitação lê (ver a nota do módulo).
pub trait IWorkbenchExtensionManagementService: IProfileAwareExtensionManagementService {
    fn get_installed_workspace_extensions(&self, include_invalid: bool) -> Promise<Vec<ExtensionRef>>;
}

service_id!(pub I_WORKBENCH_EXTENSION_MANAGEMENT_SERVICE: dyn IWorkbenchExtensionManagementService = "extensionManagementService");

/// `EnablementState`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EnablementState {
    DisabledByTrustRequirement,
    DisabledByExtensionKind,
    DisabledByEnvironment,
    EnabledByEnvironment,
    DisabledByMalicious,
    DisabledByVirtualWorkspace,
    DisabledByInvalidExtension,
    DisabledByAllowlist,
    DisabledByExtensionDependency,
    /// Temporary TODO@benibenj remove when unification transition is complete
    DisabledByUnification,
    DisabledGlobally,
    DisabledWorkspace,
    EnabledGlobally,
    EnabledWorkspace,
}

/// O `{ trusted?: boolean }` de `workspaceTypeOverrides` em `getEnablementStates`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WorkspaceTypeOverrides {
    pub trusted: Option<bool>,
}

/// `IWorkbenchExtensionEnablementService`.
pub trait IWorkbenchExtensionEnablementService {
    /// Event to listen on for extension enablement changes.
    fn on_enablement_changed(&self) -> Event<Vec<ExtensionRef>>;

    /// Returns the enablement state for the given extension.
    fn get_enablement_state(&self, extension: &IExtension) -> EnablementState;

    /// Returns the enablement states for the given extensions.
    fn get_enablement_states(&self, extensions: &[ExtensionRef], workspace_type_overrides: WorkspaceTypeOverrides) -> Vec<EnablementState>;

    /// Returns the enablement states for the dependencies of the given extension.
    fn get_dependencies_enablement_states(&self, extension: &IExtension) -> Vec<(ExtensionRef, EnablementState)>;

    /// Returns `true` if the enablement can be changed.
    fn can_change_enablement(&self, extension: &IExtension) -> bool;

    /// Returns `true` if the enablement can be changed.
    fn can_change_workspace_enablement(&self, extension: &IExtension) -> bool;

    /// Returns `true` if the given extension is enabled.
    fn is_enabled(&self, extension: &IExtension) -> bool;

    /// Returns `true` if the given enablement state is enabled enablement state.
    fn is_enabled_enablement_state(&self, enablement_state: EnablementState) -> bool;

    /// Returns `true` if the given extension identifier is disabled globally. Extensions can be disabled globally or
    /// in workspace or both. If an extension is disabled in both then enablement state shows only workspace.
    fn is_disabled_globally(&self, extension: &IExtension) -> bool;

    /// Enable or disable the given extension. If the state is a workspace one then enablement is done for workspace,
    /// otherwise globally.
    ///
    /// Returns a promise that resolves to a boolean per extension. If it resolves to `true` then the change requires a
    /// restart to take effect. The promise is rejected if the enablement is requested for the workspace and there is
    /// no workspace.
    fn set_enablement(&self, extensions: Vec<ExtensionRef>, state: EnablementState) -> Promise<Vec<bool>>;

    /// Updates the enablement state of the extensions when workspace trust changes.
    fn update_extensions_enablements_when_workspace_trust_changes(&self) -> Promise<()>;
}

service_id!(pub I_WORKBENCH_EXTENSION_ENABLEMENT_SERVICE: dyn IWorkbenchExtensionEnablementService = "extensionEnablementService");

/// `{ readonly extensions: IExtensionIdentifier[]; readonly source?: string }`: o que
/// `IGlobalExtensionEnablementService.onDidChangeEnablement` emite.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DidChangeEnablementEvent {
    pub extensions: Vec<ExtensionIdentifierInfo>,
    pub source: Option<String>,
}

/// `IGlobalExtensionEnablementService`: as extensões desabilitadas em todos os workspaces do perfil.
pub trait IGlobalExtensionEnablementService {
    fn on_did_change_enablement(&self) -> Event<DidChangeEnablementEvent>;

    fn get_disabled_extensions(&self) -> Vec<ExtensionIdentifierInfo>;
    fn get_disabled_extensions_async(&self) -> Promise<Vec<ExtensionIdentifierInfo>>;

    fn enable_extension(&self, extension: &ExtensionIdentifierInfo, source: Option<&str>) -> Promise<bool>;
    fn disable_extension(&self, extension: &ExtensionIdentifierInfo, source: Option<&str>) -> Promise<bool>;
}

service_id!(pub I_GLOBAL_EXTENSION_ENABLEMENT_SERVICE: dyn IGlobalExtensionEnablementService = "IGlobalExtensionEnablementService");
