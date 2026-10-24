//! O [`IExtensionInstallService`] sobre o [`ExtensionManagementService`] síncrono de `je-extensions`: a camada
//! assíncrona (`Promise`) e a de eventos que o workbench consome (`onInstallExtension`, `onDidInstallExtensions`,
//! `onUninstallExtension`, `onDidUninstallExtension`, `onDidUpdateExtensionMetadata`).
//!
//! A lógica de instalação, remoção e metadados vive uma vez só em `je-extensions`; este módulo converte os tipos
//! (`LocalExtension` em [`ExtensionRef`], caminhos em `URI`, erros em `Error`) e dispara os eventos que o serviço
//! síncrono não tem.
//!
//! Diferenças e o que falta:
//! - **`onInstallExtension` e `onUninstallExtension`** o serviço síncrono só dispara os eventos de resultado. Aqui o
//!   evento de início dispara antes da chamada, com a fonte (galeria ou VSIX) que o chamador deu. Os resultados de
//!   `onDidInstallExtensions` ganham a `source` pelo identificador da extensão pedida; as dependências instaladas junto
//!   (packs) saem sem `source`.
//! - **Síncrono por baixo.** A chamada ao serviço de `je-extensions` bloqueia e a `Promise` já volta assentada; o
//!   `CancellationToken` do original não tem onde ser lido.
//! - **Não existe em `je-extensions` e rejeita com `Unsupported`:** `zip` (gerar um VSIX de uma extensão instalada),
//!   `toggleApplicationScope`, `installExtensionsFromProfile` e `resetPinnedStateForAllUserExtensions` precisam do
//!   `copyExtension` e da varredura por perfil do `abstractExtensionManagementService`, ainda não portados por inteiro.
//!   `registerParticipant` guarda o participante, mas o serviço síncrono não o chama (o `postInstall` do original só roda
//!   no fluxo assíncrono de instalação).
//! - **`download`** usa o [`ExtensionsDownloader`]; `donot_verify_signature` vira o `verify_signature` invertido.
//! - **`getTargetPlatform`** é o da plataforma onde o serviço roda.

use std::cell::RefCell;
use std::rc::Rc;

use je_base::errors::Error;
use je_base::event::{Emitter, Event};
use je_base::promise::Promise;
use je_base::uri::URI;
use je_extensions::downloader::ExtensionsDownloader;
use je_extensions::gallery::GalleryApi;
use je_extensions::management::{
    ExtensionManagementError, ExtensionManagementErrorCode, ExtensionsControlManifest, GalleryExtension, InstallOperation, InstallOptions, LocalExtension, Metadata,
    ProductVersion, UninstallOptions,
};
use je_extensions::management_service::{DidUninstallExtensionEvent as PlatformDidUninstall, ExtensionManagementService, InstallExtensionResult as PlatformInstallResult};
use je_extensions::manifest::{ExtensionIdentifierInfo, ExtensionType, TargetPlatform};
use je_extensions::util::are_same_extensions;
use serde_json::Value;

use super::extension_install_service::{
    DidUpdateExtensionMetadata, IExtensionInstallService, IExtensionManagementParticipant, InstallExtensionEvent, InstallExtensionInfo, UninstallExtensionEvent,
    UninstallExtensionInfo,
};
use super::extension_management::{
    DidUninstallExtensionEvent, ExtensionRef, IExtensionManagementService, InstallExtensionResult, InstallExtensionSource,
};

/// O texto do código de um [`ExtensionManagementErrorCode`], que é o `name` do `ExtensionManagementError` do original.
fn code_name(code: ExtensionManagementErrorCode) -> String {
    format!("{code:?}")
}

/// Um `ExtensionManagementError` como o `Error` que a `Promise` rejeita: `name` é o código, e o código também fica na
/// propriedade `code`.
fn to_error(error: &ExtensionManagementError) -> Error {
    if error.code == ExtensionManagementErrorCode::Cancelled {
        return Error::cancellation();
    }
    let name = code_name(error.code);
    Error::named(name.clone(), error.message.clone()).with_prop("code", name)
}

fn unsupported(what: &str) -> Error {
    to_error(&ExtensionManagementError::new(format!("{what} is not supported by the extension management service"), ExtensionManagementErrorCode::Unsupported))
}

/// Assenta a chamada síncrona numa `Promise`.
fn settle<T: Clone + 'static>(call: impl FnOnce() -> Result<T, ExtensionManagementError>) -> Promise<T> {
    match call() {
        Ok(value) => Promise::resolved(value),
        Err(error) => Promise::rejected(to_error(&error)),
    }
}

/// O caminho de uma URI de arquivo, que é o que o serviço de `je-extensions` recebe.
fn uri_path(uri: &URI) -> String {
    uri.path().to_string_lossy()
}

/// O `URI` de um caminho de pasta ou arquivo local.
fn path_uri(path: &str) -> URI {
    URI::file(path, je_platform::resources::host_platform())
}

/// A fonte de uma instalação pendente, para dar a `source` ao resultado.
struct PendingSource {
    identifier: ExtensionIdentifierInfo,
    source: InstallExtensionSource,
}

/// O [`IExtensionInstallService`] sobre o [`ExtensionManagementService`] (ver a nota do módulo).
pub struct ExtensionInstallService {
    service: Rc<ExtensionManagementService>,
    /// O cliente do marketplace que o serviço síncrono usa: o manifesto de controle (extensões maliciosas e obsoletas).
    gallery: Rc<dyn GalleryApi>,
    downloader: Rc<ExtensionsDownloader>,
    /// `userDataProfilesService.currentProfile.extensionsResource`.
    current_profile_location: URI,
    prefer_pre_releases: bool,
    on_install_extension: Rc<Emitter<InstallExtensionEvent>>,
    on_did_install_extensions: Rc<Emitter<Vec<InstallExtensionResult>>>,
    on_uninstall_extension: Rc<Emitter<UninstallExtensionEvent>>,
    on_did_uninstall_extension: Rc<Emitter<DidUninstallExtensionEvent>>,
    on_did_update_extension_metadata: Rc<Emitter<DidUpdateExtensionMetadata>>,
    pending_sources: Rc<RefCell<Vec<PendingSource>>>,
    participants: RefCell<Vec<Rc<dyn IExtensionManagementParticipant>>>,
    /// As inscrições nos eventos do serviço síncrono: vivem tanto quanto este serviço.
    _subscriptions: Vec<je_base::lifecycle::DisposableRef>,
}

impl ExtensionInstallService {
    /// `prefer_pre_releases` é `preferPreReleases` (a configuração `extensions.autoUpdate.preRelease`... lida por quem
    /// monta o serviço); `current_profile_location` o `extensionsResource` do perfil corrente.
    pub fn new(
        service: Rc<ExtensionManagementService>,
        gallery: Rc<dyn GalleryApi>,
        downloader: Rc<ExtensionsDownloader>,
        current_profile_location: URI,
        prefer_pre_releases: bool,
    ) -> Rc<Self> {
        let on_did_install_extensions = Emitter::<Vec<InstallExtensionResult>>::new();
        let on_did_uninstall_extension = Emitter::<DidUninstallExtensionEvent>::new();
        let on_did_update_extension_metadata = Emitter::<DidUpdateExtensionMetadata>::new();
        let pending_sources: Rc<RefCell<Vec<PendingSource>>> = Rc::new(RefCell::new(Vec::new()));

        let mut subscriptions = Vec::new();
        {
            let emitter = on_did_install_extensions.clone();
            let pending = pending_sources.clone();
            subscriptions.push(service.on_did_install_extensions().listen(move |results| {
                let converted = results.iter().map(|result| Self::to_install_result(result, &pending.borrow())).collect();
                emitter.fire(converted);
            }));
        }
        {
            let emitter = on_did_uninstall_extension.clone();
            subscriptions.push(service.on_did_uninstall_extension().listen(move |event| emitter.fire(Self::to_did_uninstall(event))));
        }
        {
            let emitter = on_did_update_extension_metadata.clone();
            subscriptions.push(service.on_did_update_extension_metadata().listen(move |(local, profile_location)| {
                emitter.fire(DidUpdateExtensionMetadata { profile_location: path_uri(profile_location), local: Rc::new(local.clone()) });
            }));
        }

        Rc::new(Self {
            service,
            gallery,
            downloader,
            current_profile_location,
            prefer_pre_releases,
            on_install_extension: Emitter::new(),
            on_did_install_extensions,
            on_uninstall_extension: Emitter::new(),
            on_did_uninstall_extension,
            on_did_update_extension_metadata,
            pending_sources,
            participants: RefCell::new(Vec::new()),
            _subscriptions: subscriptions,
        })
    }

    fn to_install_result(result: &PlatformInstallResult, pending: &[PendingSource]) -> InstallExtensionResult {
        let source = pending.iter().find(|pending| are_same_extensions(&pending.identifier, &result.identifier)).map(|pending| pending.source.clone());
        InstallExtensionResult {
            identifier: result.identifier.clone(),
            operation: result.operation,
            source,
            profile_location: Some(path_uri(&result.profile_location)),
            local: result.local.clone().map(Rc::new),
            workspace_scoped: result.local.as_ref().map(|local| local.is_workspace_scoped),
        }
    }

    fn to_did_uninstall(event: &PlatformDidUninstall) -> DidUninstallExtensionEvent {
        DidUninstallExtensionEvent {
            identifier: event.identifier.clone(),
            profile_location: Some(path_uri(&event.profile_location)),
            error: event.error.map(code_name),
            workspace_scoped: None,
            server: None,
        }
    }

    /// O perfil do pedido: o `profileLocation` das opções ou o corrente.
    fn profile_of(&self, profile_location: Option<&String>) -> URI {
        profile_location.map_or_else(|| self.current_profile_location.clone(), |location| path_uri(location))
    }

    /// Dispara `onInstallExtension` e registra a fonte para o resultado.
    fn begin_install(&self, identifier: &ExtensionIdentifierInfo, source: InstallExtensionSource, options: &InstallOptions) {
        self.pending_sources.borrow_mut().push(PendingSource { identifier: identifier.clone(), source: source.clone() });
        self.on_install_extension.fire(InstallExtensionEvent {
            identifier: identifier.clone(),
            source,
            profile_location: self.profile_of(options.profile_location.as_ref()),
            application_scoped: options.is_application_scoped,
            workspace_scoped: options.is_workspace_scoped,
        });
    }

    fn end_install(&self, identifier: &ExtensionIdentifierInfo) {
        self.pending_sources.borrow_mut().retain(|pending| !are_same_extensions(&pending.identifier, identifier));
    }

    fn begin_uninstall(&self, extension: &LocalExtension, options: &UninstallOptions) {
        self.on_uninstall_extension.fire(UninstallExtensionEvent {
            identifier: extension.identifier.clone(),
            profile_location: self.profile_of(options.profile_location.as_ref()),
            application_scoped: Some(extension.is_application_scoped),
            workspace_scoped: Some(extension.is_workspace_scoped),
        });
    }

    fn profile_text(profile_location: Option<&URI>) -> Option<String> {
        profile_location.map(uri_path)
    }
}

impl IExtensionManagementService for ExtensionInstallService {
    fn on_did_install_extensions(&self) -> Event<Vec<InstallExtensionResult>> {
        self.on_did_install_extensions.event()
    }

    fn on_did_uninstall_extension(&self) -> Event<DidUninstallExtensionEvent> {
        self.on_did_uninstall_extension.event()
    }

    fn get_installed(&self) -> Promise<Vec<ExtensionRef>> {
        self.get_installed_filtered(None, None, None, None)
    }

    fn get_extensions_control_manifest(&self) -> Promise<ExtensionsControlManifest> {
        match self.gallery.get_extensions_control_manifest() {
            Ok(manifest) => Promise::resolved(manifest),
            Err(error) => Promise::rejected(Error::new(error.message)),
        }
    }
}

impl IExtensionInstallService for ExtensionInstallService {
    fn prefer_pre_releases(&self) -> bool {
        self.prefer_pre_releases
    }

    fn on_install_extension(&self) -> Event<InstallExtensionEvent> {
        self.on_install_extension.event()
    }

    fn on_uninstall_extension(&self) -> Event<UninstallExtensionEvent> {
        self.on_uninstall_extension.event()
    }

    fn on_did_update_extension_metadata(&self) -> Event<DidUpdateExtensionMetadata> {
        self.on_did_update_extension_metadata.event()
    }

    fn zip(&self, _extension: &LocalExtension) -> Promise<URI> {
        Promise::rejected(unsupported("zip"))
    }

    fn get_manifest(&self, vsix: &URI) -> Promise<Value> {
        settle(|| self.service.get_manifest(&uri_path(vsix)))
    }

    fn install(&self, vsix: &URI, options: InstallOptions) -> Promise<ExtensionRef> {
        let location = InstallExtensionSource::Location(vsix.clone());
        let identifier = self.service.get_manifest(&uri_path(vsix)).ok().map(|manifest| {
            let publisher = manifest.get("publisher").and_then(Value::as_str).unwrap_or_default();
            let name = manifest.get("name").and_then(Value::as_str).unwrap_or_default();
            ExtensionIdentifierInfo::new(je_extensions::util::get_gallery_extension_id(Some(publisher), name))
        });
        if let Some(identifier) = &identifier {
            self.begin_install(identifier, location, &options);
        }
        let result = settle(|| self.service.install(&uri_path(vsix), &options).map(Rc::new));
        if let Some(identifier) = &identifier {
            self.end_install(identifier);
        }
        result
    }

    fn can_install(&self, extension: &GalleryExtension) -> Promise<Result<(), String>> {
        Promise::resolved(self.service.can_install(extension))
    }

    fn install_from_gallery(&self, extension: &GalleryExtension, options: InstallOptions) -> Promise<ExtensionRef> {
        self.begin_install(&extension.identifier, InstallExtensionSource::Gallery(extension.clone()), &options);
        let result = settle(|| self.service.install_from_gallery(extension, &options).map(Rc::new));
        self.end_install(&extension.identifier);
        result
    }

    fn install_gallery_extensions(&self, extensions: Vec<InstallExtensionInfo>) -> Promise<Vec<InstallExtensionResult>> {
        for info in &extensions {
            self.begin_install(&info.extension.identifier, InstallExtensionSource::Gallery(info.extension.clone()), &info.options);
        }
        let pairs: Vec<(GalleryExtension, InstallOptions)> = extensions.iter().map(|info| (info.extension.clone(), info.options.clone())).collect();
        let result = settle(|| {
            self.service.install_gallery_extensions(&pairs).map(|results| {
                let pending = self.pending_sources.borrow();
                results.iter().map(|result| Self::to_install_result(result, &pending)).collect::<Vec<_>>()
            })
        });
        for info in &extensions {
            self.end_install(&info.extension.identifier);
        }
        result
    }

    fn install_from_location(&self, location: &URI, profile_location: &URI) -> Promise<ExtensionRef> {
        settle(|| self.service.install_from_location(&uri_path(location), &uri_path(profile_location)).map(Rc::new))
    }

    fn install_extensions_from_profile(&self, _extensions: Vec<ExtensionIdentifierInfo>, _from_profile_location: &URI, _to_profile_location: &URI) -> Promise<Vec<ExtensionRef>> {
        Promise::rejected(unsupported("installExtensionsFromProfile"))
    }

    fn uninstall(&self, extension: &LocalExtension, options: UninstallOptions) -> Promise<()> {
        self.begin_uninstall(extension, &options);
        settle(|| self.service.uninstall(extension, &options))
    }

    fn uninstall_extensions(&self, extensions: Vec<UninstallExtensionInfo>) -> Promise<()> {
        for info in &extensions {
            self.begin_uninstall(&info.extension, &info.options.clone().unwrap_or_default());
        }
        settle(|| {
            for info in &extensions {
                self.service.uninstall(&info.extension, &info.options.clone().unwrap_or_default())?;
            }
            Ok(())
        })
    }

    fn toggle_application_scope(&self, _extension: &LocalExtension, _from_profile_location: &URI) -> Promise<ExtensionRef> {
        Promise::rejected(unsupported("toggleApplicationScope"))
    }

    fn get_installed_filtered(
        &self,
        extension_type: Option<ExtensionType>,
        profile_location: Option<&URI>,
        product_version: Option<ProductVersion>,
        language: Option<&str>,
    ) -> Promise<Vec<ExtensionRef>> {
        let profile_location = Self::profile_text(profile_location);
        settle(|| {
            self.service
                .get_installed(extension_type, profile_location.as_deref(), product_version, language)
                .map(|installed| installed.into_iter().map(Rc::new).collect())
        })
    }

    fn copy_extensions(&self, from_profile_location: &URI, to_profile_location: &URI) -> Promise<()> {
        settle(|| self.service.copy_extensions(&uri_path(from_profile_location), &uri_path(to_profile_location)))
    }

    fn update_metadata(&self, local: &LocalExtension, metadata: Metadata, profile_location: Option<&URI>) -> Promise<ExtensionRef> {
        let profile_location = Self::profile_text(profile_location);
        settle(|| self.service.update_metadata(local, &metadata, profile_location.as_deref()).map(Rc::new))
    }

    fn reset_pinned_state_for_all_user_extensions(&self, _pinned: bool) -> Promise<()> {
        Promise::rejected(unsupported("resetPinnedStateForAllUserExtensions"))
    }

    fn download(&self, extension: &GalleryExtension, operation: InstallOperation, donot_verify_signature: bool) -> Promise<URI> {
        settle(|| {
            self.downloader
                .download(extension, operation, !donot_verify_signature, Some(self.service.target_platform()))
                .map(|downloaded| path_uri(&downloaded.location))
        })
    }

    fn register_participant(&self, participant: Rc<dyn IExtensionManagementParticipant>) {
        self.participants.borrow_mut().push(participant);
    }

    fn get_target_platform(&self) -> Promise<TargetPlatform> {
        Promise::resolved(self.service.target_platform())
    }

    fn clean_up(&self) -> Promise<()> {
        self.service.clean_up();
        Promise::resolved(())
    }
}

/// Os participantes registrados (o serviço síncrono não os chama, ver a nota do módulo), para quem monta o fluxo
/// assíncrono de instalação consultá-los.
impl ExtensionInstallService {
    pub fn participants(&self) -> Vec<Rc<dyn IExtensionManagementParticipant>> {
        self.participants.borrow().clone()
    }
}
