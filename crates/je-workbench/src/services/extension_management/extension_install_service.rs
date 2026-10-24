//! Porte dos contratos de instalação do `IExtensionManagementService`
//! (`src/vs/platform/extensionManagement/common/extensionManagement.ts`) que o recorte de
//! [`super::extension_management`] deixou de fora: `InstallExtensionEvent`, `UninstallExtensionEvent`,
//! `DidUpdateExtensionMetadata`, `InstallExtensionInfo`, `UninstallExtensionInfo`, `InstallExtensionSummary`,
//! `IExtensionManagementParticipant`, `AllowedExtensionsConfigValueType` e os métodos assíncronos de instalação,
//! remoção e atualização, mais os métodos do `IWorkbenchExtensionManagementService` que o `ExtensionsWorkbenchService`
//! chama ([`IWorkbenchExtensionInstallService`]).
//!
//! Notas de tradução:
//! - **Um só modelo de extensão local.** A extensão é a [`LocalExtension`] completa de `je-extensions`; as extensões
//!   devolvidas são [`ExtensionRef`] (`Rc`), porque o original as compara por identidade. Os parâmetros recebem
//!   `&LocalExtension`.
//! - **Subtrait.** O [`IExtensionManagementService`] reduzido continua como está, com os eventos de resultado; o trait
//!   [`IExtensionInstallService`] o estende com o resto da interface do original, para os chamadores atuais continuarem
//!   compilando. [`IProfileAwareExtensionManagementService::as_install_service`] liga os dois.
//! - **`InstallOptions`, `UninstallOptions`, `Metadata`** são os de `je-extensions`, com `profileLocation` como caminho
//!   de URI (texto). `context` e `downloadExtensionsLocally` do `InstallOptions` original não existem lá; entram com o
//!   serviço do workbench que os lê.
//! - **`canInstall`** devolve `Ok(())` para o `true` do original e `Err(mensagem)` para o `IMarkdownString` (o texto
//!   da mensagem), como o `can_install` síncrono.
//! - **`AllowedExtensionsConfigValueType`** é o mapa ordenado de `extensions.allowed` (valor `true`, `false`, texto ou
//!   lista de textos), como `serde_json::Map` (que preserva a ordem neste crate); o `AllowedExtensionsService` de
//!   `je-extensions` devolve o mesmo conteúdo num `IndexMap`.
//! - **`installGalleryExtensions`** devolve o `InstallExtensionResult` do workbench (extensão como `ExtensionRef`), o
//!   mesmo que o evento `onDidInstallExtensions`.
//! - **`InstallExtensionSource`** (`URI | IGalleryExtension`) mora em [`super::extension_management`], porque o
//!   resultado de instalação a usa; este módulo a reexporta.

use std::rc::Rc;

use je_base::cancellation::CancellationToken;
use je_base::event::Event;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_extensions::management::{
    GalleryExtension, InstallOperation, InstallOptions, LocalExtension, Metadata, ProductVersion, UninstallOptions,
};
use je_extensions::manifest::{ExtensionIdentifierInfo, ExtensionManifest, ExtensionType, TargetPlatform};
use je_instantiation::service_id;
use serde_json::{Map, Value};

pub use super::extension_management::InstallExtensionSource;
use super::extension_management::{
    ExtensionRef, IExtensionManagementServer, IExtensionManagementService, IResourceExtension, IWorkbenchExtensionManagementService,
    InstallExtensionResult,
};

/// `InstallExtensionEvent`.
#[derive(Clone, Debug)]
pub struct InstallExtensionEvent {
    pub identifier: ExtensionIdentifierInfo,
    pub source: InstallExtensionSource,
    pub profile_location: URI,
    pub application_scoped: Option<bool>,
    pub workspace_scoped: Option<bool>,
}

/// `UninstallExtensionEvent`.
#[derive(Clone, Debug)]
pub struct UninstallExtensionEvent {
    pub identifier: ExtensionIdentifierInfo,
    pub profile_location: URI,
    pub application_scoped: Option<bool>,
    pub workspace_scoped: Option<bool>,
}

/// `DidUpdateExtensionMetadata`.
#[derive(Clone)]
pub struct DidUpdateExtensionMetadata {
    pub profile_location: URI,
    pub local: ExtensionRef,
}

/// `InstallExtensionSummary`: as extensões que falharam, com as opções com que foram pedidas.
#[derive(Clone, Debug, Default)]
pub struct InstallExtensionSummary {
    pub failed: Vec<InstallExtensionFailure>,
}

/// Um item de `InstallExtensionSummary.failed`.
#[derive(Clone, Debug)]
pub struct InstallExtensionFailure {
    pub id: String,
    pub install_options: InstallOptions,
}

/// `InstallExtensionInfo`.
#[derive(Clone, Debug)]
pub struct InstallExtensionInfo {
    pub extension: GalleryExtension,
    pub options: InstallOptions,
}

/// `UninstallExtensionInfo`.
#[derive(Clone, Debug)]
pub struct UninstallExtensionInfo {
    pub extension: LocalExtension,
    pub options: Option<UninstallOptions>,
}

/// `IExtensionManagementParticipant`.
pub trait IExtensionManagementParticipant {
    fn post_install(&self, local: &LocalExtension, source: &InstallExtensionSource, options: &InstallOptions, token: CancellationToken) -> Promise<()>;
    fn post_uninstall(&self, local: &LocalExtension, options: &UninstallOptions, token: CancellationToken) -> Promise<()>;
}

/// `AllowedExtensionsConfigValueType`: `IStringDictionary<boolean | string | string[]>`.
pub type AllowedExtensionsConfigValueType = Map<String, Value>;

/// O resto de `IExtensionManagementService` (ver a nota do módulo sobre o subtrait).
pub trait IExtensionInstallService: IExtensionManagementService {
    /// `preferPreReleases`.
    fn prefer_pre_releases(&self) -> bool;

    fn on_install_extension(&self) -> Event<InstallExtensionEvent>;
    fn on_uninstall_extension(&self) -> Event<UninstallExtensionEvent>;
    fn on_did_update_extension_metadata(&self) -> Event<DidUpdateExtensionMetadata>;

    fn zip(&self, extension: &LocalExtension) -> Promise<URI>;
    fn get_manifest(&self, vsix: &URI) -> Promise<Value>;
    fn install(&self, vsix: &URI, options: InstallOptions) -> Promise<ExtensionRef>;
    fn can_install(&self, extension: &GalleryExtension) -> Promise<Result<(), String>>;
    fn install_from_gallery(&self, extension: &GalleryExtension, options: InstallOptions) -> Promise<ExtensionRef>;
    fn install_gallery_extensions(&self, extensions: Vec<InstallExtensionInfo>) -> Promise<Vec<InstallExtensionResult>>;
    fn install_from_location(&self, location: &URI, profile_location: &URI) -> Promise<ExtensionRef>;
    fn install_extensions_from_profile(&self, extensions: Vec<ExtensionIdentifierInfo>, from_profile_location: &URI, to_profile_location: &URI) -> Promise<Vec<ExtensionRef>>;
    fn uninstall(&self, extension: &LocalExtension, options: UninstallOptions) -> Promise<()>;
    fn uninstall_extensions(&self, extensions: Vec<UninstallExtensionInfo>) -> Promise<()>;
    fn toggle_application_scope(&self, extension: &LocalExtension, from_profile_location: &URI) -> Promise<ExtensionRef>;
    /// `getInstalled` com os argumentos opcionais do original (o do trait reduzido não os recebe).
    fn get_installed_filtered(
        &self,
        extension_type: Option<ExtensionType>,
        profile_location: Option<&URI>,
        product_version: Option<ProductVersion>,
        language: Option<&str>,
    ) -> Promise<Vec<ExtensionRef>>;
    fn copy_extensions(&self, from_profile_location: &URI, to_profile_location: &URI) -> Promise<()>;
    /// `updateMetadata(local, metadata, profileLocation?)`: sem o perfil, o corrente.
    fn update_metadata(&self, local: &LocalExtension, metadata: Metadata, profile_location: Option<&URI>) -> Promise<ExtensionRef>;
    fn reset_pinned_state_for_all_user_extensions(&self, pinned: bool) -> Promise<()>;

    fn download(&self, extension: &GalleryExtension, operation: InstallOperation, donot_verify_signature: bool) -> Promise<URI>;

    fn register_participant(&self, participant: Rc<dyn IExtensionManagementParticipant>);
    fn get_target_platform(&self) -> Promise<TargetPlatform>;

    fn clean_up(&self) -> Promise<()>;
}

service_id!(pub I_EXTENSION_INSTALL_SERVICE: dyn IExtensionInstallService = "extensionManagementService");

/// O que o `ExtensionsWorkbenchService` chama no `IWorkbenchExtensionManagementService` além do contrato de instalação
/// da plataforma: a escolha de servidores, as extensões de recurso (pasta do workspace ou VSIX) e a instalação num
/// conjunto de servidores.
pub trait IWorkbenchExtensionInstallService: IExtensionInstallService + IWorkbenchExtensionManagementService {
    /// `getInstallableServers`.
    fn get_installable_servers(&self, extension: &GalleryExtension) -> Promise<Vec<Rc<IExtensionManagementServer>>>;
    /// `getExtensions(locations)`: as extensões de recurso que vivem nas pastas dadas.
    fn get_resource_extensions(&self, locations: Vec<URI>) -> Promise<Vec<IResourceExtension>>;
    /// `canInstall(IResourceExtension)`.
    fn can_install_resource(&self, extension: &IResourceExtension) -> Promise<Result<(), String>>;
    /// `installResourceExtension`.
    fn install_resource_extension(&self, extension: &IResourceExtension, options: InstallOptions) -> Promise<ExtensionRef>;
    /// `installVSIX(vsix, manifest, options)`.
    fn install_vsix(&self, vsix: &URI, manifest: ExtensionManifest, options: InstallOptions) -> Promise<ExtensionRef>;
    /// `updateFromGallery(gallery, existing, options)`.
    fn update_from_gallery(&self, gallery: &GalleryExtension, existing: &LocalExtension, options: InstallOptions) -> Promise<ExtensionRef>;
    /// `installFromGallery(gallery, options, servers)`: com `servers` vazio ou ausente o serviço escolhe.
    fn install_from_gallery_in_servers(&self, gallery: &GalleryExtension, options: InstallOptions, servers: Option<Vec<Rc<IExtensionManagementServer>>>) -> Promise<ExtensionRef>;
}

service_id!(pub I_WORKBENCH_EXTENSION_INSTALL_SERVICE: dyn IWorkbenchExtensionInstallService = "extensionManagementService");
