//! Porte de `src/vs/workbench/services/extensionManagement/electron-browser/extensionGalleryManifestService.ts`: o
//! serviço do manifesto da galeria no workbench do Electron. Sem marketplace privado configurado, o manifesto é o que o
//! produto declara (o serviço base); com ele, o manifesto é buscado na URL configurada, depois de conferir a conta que dá
//! acesso (`IExtensionGalleryAccountService`), e o estado de acesso (`available`, `requiresSignIn`, `accessDenied`,
//! `unavailable`) acompanha o resultado. O manifesto vigente é empurrado ao processo compartilhado e ao servidor remoto.
//!
//! Divergências:
//! - O `extends ExtensionGalleryManifestService` é o campo `base` (o serviço da plataforma), e `super.getExtensionGalleryManifest()`
//!   é o `get_extension_gallery_manifest` dele. `this._register` e `this._store` são o [`ExtensionGalleryManifestService::store`]
//!   dele, que o descarte do serviço descarta.
//! - As dependências por injeção entram numa struct, [`WorkbenchExtensionGalleryManifestServiceParts`]. Os recortes
//!   `ISharedProcessService` é o recorte de [`super::extension_gallery_contracts`]; o `IRemoteAgentService` e o `IHostService`
//!   são os dos módulos `services::remote` e `services::host`; o `IEnvironmentService` é o `EnvironmentService` da plataforma. `registerSingleton`
//!   não tem equivalente: quem monta o workbench cria o serviço com [`WorkbenchExtensionGalleryManifestService::new`].
//! - `this.extensionGalleryManifest !== manifest` compara objetos por identidade, e todo manifesto recebido é um objeto
//!   novo: o manifesto muda sempre que algum dos dois (o atual ou o novo) existe, e só `null` por `null` não muda.
//! - `getValue<string>(extensions.gallery.serviceUrl)` só vale como configurado quando é texto não vazio (o tipo do setting).
//! - A resposta do índice do serviço é lida como valor dinâmico e validada como o original (`isExtensionGalleryManifestResponse`).
//!   Do que o original copia sem olhar (`...extensionGalleryManifest`), ficam o que o tipo do porte declara: `resources`,
//!   `capabilities.extensionQuery` e, se vierem bem formados, `capabilities.signing` e `capabilities.extensions`.
//! - As funções `async` do original são [`async_fn`]; o que o original lança dentro delas é o `Err` que rejeita a promessa.
//!   O `await` de uma promessa cede como no JavaScript, e o trecho até o primeiro `await` roda na chamada.
//! - O serviço segura a si mesmo por referência fraca nas inscrições de evento: ele é descartado quando o último `Rc` some.

use je_platform::files::i_file_service::IFileService;
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::async_::async_fn;
use je_base::cancellation::{CancellationToken, CancellationTokenSource};
use je_base::dynamic::Dynamic;
use je_base::errors::Error;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{DisposableStore, IDisposable, MutableDisposable};
use je_base::marshalling::Marshalled;
use je_base::nls::localize;
use je_base::parts::request::{HeaderValue, IHeaders, IRequestOptions};
use je_base::promise::Promise;
use je_platform::configuration::service::IConfigurationService;
use je_platform::dialogs::dialogs::{BaseDialogOptions, Confirmation, IDialogService};
use je_platform::environment::environment::EnvironmentService;
use je_platform::extension_management::extension_gallery_manifest::{
    EXTENSION_GALLERY_AUTH_PROVIDER_CONFIG_KEY, EXTENSION_GALLERY_SERVICE_URL_CONFIG_KEY, ExtensionGalleryManifestCapabilities,
    ExtensionGalleryManifestResource, ExtensionGalleryManifestStatus, ExtensionQueryCapabilities, ExtensionQueryCapabilityValue,
    ExtensionsCapabilities, IExtensionGalleryManifest, IExtensionGalleryManifestService, SigningCapabilities,
};
use je_platform::extension_management::extension_gallery_manifest_service::ExtensionGalleryManifestService;
use je_platform::external_services::marketplace::resolve_marketplace_headers;
use je_platform::files::file_service::FileService;
use je_platform::log::{LogArg, LogMessage, LogServiceRef, Logger};
use je_platform::product::IProductService;
use je_platform::request::request::{IRequestService, as_json};
use je_platform::storage::StorageService;
use je_platform::telemetry::telemetry::ITelemetryService;

use super::extension_gallery_account::{ExtensionGalleryAccountStatus, IExtensionGalleryAccountService};
use super::extension_gallery_contracts::ISharedProcessService;
use crate::services::host::host::IHostService;
use crate::services::remote::remote_agent_service::IRemoteAgentService;

/// O nome do canal do manifesto da galeria, no processo compartilhado e no servidor remoto.
pub const EXTENSION_GALLERY_MANIFEST_CHANNEL: &str = "extensionGalleryManifest";

/// `value?.[key]`: `undefined` quando o valor é `undefined` ou `null`.
fn prop(value: &Dynamic, key: &str) -> Dynamic {
    if value.is_nullish() { Dynamic::Undefined } else { value.get_str(key) }
}

/// A verdade JavaScript de um valor (o `!value` do original).
fn is_falsy(value: &Dynamic) -> bool {
    match value {
        Dynamic::Undefined | Dynamic::Null | Dynamic::Bool(false) => true,
        Dynamic::Number(number) => *number == 0.0 || number.is_nan(),
        Dynamic::BigInt(number) => *number == 0,
        Dynamic::String(text) => text.to_string_lossy().is_empty(),
        _ => false,
    }
}

fn is_string(value: &Dynamic) -> bool {
    matches!(value, Dynamic::String(_))
}

fn is_number(value: &Dynamic) -> bool {
    matches!(value, Dynamic::Number(_))
}

/// `isExtensionQueryCapabilityValue`.
fn is_extension_query_capability_value(candidate: &Dynamic) -> bool {
    is_string(&prop(candidate, "name")) && is_number(&prop(candidate, "value"))
}

/// `isExtensionQueryCapabilityValues`: ausente, ou uma lista só de valores de capacidade.
fn is_extension_query_capability_values(candidate: &Dynamic) -> bool {
    match candidate {
        Dynamic::Undefined => true,
        Dynamic::Array(items) => items.to_vec().iter().all(is_extension_query_capability_value),
        _ => false,
    }
}

/// `isExtensionGalleryManifestResponse`.
fn is_extension_gallery_manifest_response(candidate: &Dynamic) -> bool {
    let version = prop(candidate, "version");
    let resources = prop(candidate, "resources");
    let extension_query = prop(&prop(candidate, "capabilities"), "extensionQuery");

    let resources_are_valid = match &resources {
        Dynamic::Array(items) => items.to_vec().iter().all(|resource| !is_falsy(resource) && is_string(&prop(resource, "id")) && is_string(&prop(resource, "type"))),
        _ => false,
    };

    (version.is_undefined() || is_string(&version))
        && resources_are_valid
        && extension_query.type_of() == "object"
        && !extension_query.is_null()
        && is_extension_query_capability_values(&prop(&extension_query, "filtering"))
        && is_extension_query_capability_values(&prop(&extension_query, "sorting"))
        && is_extension_query_capability_values(&prop(&extension_query, "flags"))
}

fn text_of(value: &Dynamic) -> String {
    match value {
        Dynamic::String(text) => text.to_string_lossy(),
        _ => String::new(),
    }
}

fn bool_of(value: &Dynamic) -> Option<bool> {
    match value {
        Dynamic::Bool(flag) => Some(*flag),
        _ => None,
    }
}

/// A lista de valores de capacidade já validada por [`is_extension_query_capability_values`].
fn capability_values_of(value: &Dynamic) -> Option<Vec<ExtensionQueryCapabilityValue>> {
    let Dynamic::Array(items) = value else { return None };
    Some(
        items
            .to_vec()
            .iter()
            .map(|item| ExtensionQueryCapabilityValue {
                name: text_of(&prop(item, "name")),
                value: match prop(item, "value") {
                    Dynamic::Number(number) => number,
                    _ => f64::NAN,
                },
            })
            .collect(),
    )
}

/// O manifesto de uma resposta já validada: `{ ...extensionGalleryManifest, version: extensionGalleryManifest.version ?? '' }`.
fn manifest_of_response(response: &Dynamic) -> IExtensionGalleryManifest {
    let resources = match prop(response, "resources") {
        Dynamic::Array(items) => items
            .to_vec()
            .iter()
            .map(|resource| ExtensionGalleryManifestResource { id: text_of(&prop(resource, "id")), r#type: text_of(&prop(resource, "type")) })
            .collect(),
        _ => Vec::new(),
    };

    let capabilities = prop(response, "capabilities");
    let extension_query = prop(&capabilities, "extensionQuery");
    let signing = prop(&capabilities, "signing");
    let extensions = prop(&capabilities, "extensions");

    IExtensionGalleryManifest {
        version: text_of(&prop(response, "version")),
        resources,
        capabilities: ExtensionGalleryManifestCapabilities {
            extension_query: ExtensionQueryCapabilities {
                filtering: capability_values_of(&prop(&extension_query, "filtering")),
                sorting: capability_values_of(&prop(&extension_query, "sorting")),
                flags: capability_values_of(&prop(&extension_query, "flags")),
            },
            signing: bool_of(&prop(&signing, "allPublicRepositorySigned"))
                .map(|all_public_repository_signed| SigningCapabilities { all_public_repository_signed, all_private_repository_signed: bool_of(&prop(&signing, "allPrivateRepositorySigned")) }),
            extensions: if extensions.type_of() == "object" && !extensions.is_null() {
                Some(ExtensionsCapabilities {
                    include_public_extensions: bool_of(&prop(&extensions, "includePublicExtensions")),
                    include_private_extensions: bool_of(&prop(&extensions, "includePrivateExtensions")),
                })
            } else {
                None
            },
        },
    }
}

/// As dependências por injeção do [`WorkbenchExtensionGalleryManifestService`].
pub struct WorkbenchExtensionGalleryManifestServiceParts {
    pub product_service: Rc<dyn IProductService>,
    pub environment_service: Rc<dyn EnvironmentService>,
    pub file_service: Rc<dyn IFileService>,
    pub telemetry_service: Rc<dyn ITelemetryService>,
    pub storage_service: Rc<dyn StorageService>,
    pub remote_agent_service: Rc<dyn IRemoteAgentService>,
    pub shared_process_service: Rc<dyn ISharedProcessService>,
    pub configuration_service: Rc<dyn IConfigurationService>,
    pub request_service: Rc<dyn IRequestService>,
    pub gallery_account_service: Rc<dyn IExtensionGalleryAccountService>,
    pub log_service: LogServiceRef,
    pub dialog_service: Rc<dyn IDialogService>,
    pub host_service: Rc<dyn IHostService>,
}

/// `WorkbenchExtensionGalleryManifestService`.
pub struct WorkbenchExtensionGalleryManifestService {
    /// O `super`: o serviço base da plataforma.
    base: Rc<ExtensionGalleryManifestService>,
    /// `this._store`.
    store: Rc<DisposableStore>,
    this: Weak<WorkbenchExtensionGalleryManifestService>,

    common_headers_promise: Promise<IHeaders>,
    extension_gallery_manifest: RefCell<Option<IExtensionGalleryManifest>>,
    on_did_change_extension_gallery_manifest: Rc<Emitter<Option<IExtensionGalleryManifest>>>,
    current_status: Cell<ExtensionGalleryManifestStatus>,
    on_did_change_extension_gallery_manifest_status: Rc<Emitter<ExtensionGalleryManifestStatus>>,
    extension_gallery_manifest_promise: RefCell<Option<Promise<()>>>,
    resolution_token_source: Rc<MutableDisposable<CancellationTokenSource>>,

    product_service: Rc<dyn IProductService>,
    telemetry_service: Rc<dyn ITelemetryService>,
    configuration_service: Rc<dyn IConfigurationService>,
    request_service: Rc<dyn IRequestService>,
    gallery_account_service: Rc<dyn IExtensionGalleryAccountService>,
    log_service: LogServiceRef,
    dialog_service: Rc<dyn IDialogService>,
    host_service: Rc<dyn IHostService>,
}

impl WorkbenchExtensionGalleryManifestService {
    pub fn new(parts: WorkbenchExtensionGalleryManifestServiceParts) -> Rc<Self> {
        let WorkbenchExtensionGalleryManifestServiceParts {
            product_service,
            environment_service,
            file_service,
            telemetry_service,
            storage_service,
            remote_agent_service,
            shared_process_service,
            configuration_service,
            request_service,
            gallery_account_service,
            log_service,
            dialog_service,
            host_service,
        } = parts;

        let base = ExtensionGalleryManifestService::new(product_service.clone());
        let store = base.store().clone();
        let on_did_change_extension_gallery_manifest = Emitter::new();
        store.add(on_did_change_extension_gallery_manifest.clone());
        let on_did_change_extension_gallery_manifest_status = Emitter::new();
        store.add(on_did_change_extension_gallery_manifest_status.clone());
        let resolution_token_source = MutableDisposable::<CancellationTokenSource>::new();
        store.add(resolution_token_source.clone());

        let common_headers_promise = resolve_marketplace_headers(
            product_service.version(),
            &*product_service,
            &*environment_service,
            &*configuration_service,
            &file_service,
            Some(storage_service),
            telemetry_service.clone(),
        );

        let this = Rc::new_cyclic(|this| Self {
            base,
            store,
            this: this.clone(),
            common_headers_promise,
            extension_gallery_manifest: RefCell::new(None),
            on_did_change_extension_gallery_manifest,
            current_status: Cell::new(ExtensionGalleryManifestStatus::Unavailable),
            on_did_change_extension_gallery_manifest_status,
            extension_gallery_manifest_promise: RefCell::new(None),
            resolution_token_source,
            product_service,
            telemetry_service,
            configuration_service,
            request_service,
            gallery_account_service,
            log_service,
            dialog_service,
            host_service,
        });

        let mut channels = vec![shared_process_service.get_channel(EXTENSION_GALLERY_MANIFEST_CHANNEL)];
        if let Some(remote_connection) = remote_agent_service.get_connection() {
            channels.push(remote_connection.get_channel(EXTENSION_GALLERY_MANIFEST_CHANNEL));
        }
        let update_channels: Rc<dyn Fn(&Option<IExtensionGalleryManifest>)> = {
            let log_service = this.log_service.clone();
            Rc::new(move |manifest: &Option<IExtensionGalleryManifest>| {
                log_service.trace(&format!("[Marketplace] Updating channels with manifest {}", if manifest.is_some() { "available" } else { "unavailable" }));
                for channel in &channels {
                    channel.call("setExtensionGalleryManifest", Marshalled::Array(vec![manifest.as_ref().map_or(Marshalled::Null, IExtensionGalleryManifest::to_marshalled)]), CancellationToken::None);
                }
            })
        };
        let weak = Rc::downgrade(&this);
        // `getExtensionGalleryManifest().then(...).catch(...)`: o `catch` também pega o erro do `then`.
        let _ = this.get_extension_gallery_manifest().then_result(move |result| {
            let Some(this) = weak.upgrade() else { return Ok(()) };
            match result {
                Ok(manifest) => {
                    if this.store.is_disposed() {
                        this.log_service.trace("[Marketplace] Store is already disposed, skipping channel initialization");
                        return Ok(());
                    }
                    update_channels(&manifest);
                    let on_manifest_changed = update_channels.clone();
                    this.store.add(this.on_did_change_extension_gallery_manifest.event().listen(move |manifest| on_manifest_changed(manifest)));
                }
                Err(error) => {
                    this.log_service.error_args(&LogMessage::from("[Marketplace] Error during initial gallery manifest bootstrap"), &[LogArg::from(&error)]);
                }
            }
            Ok(())
        });
        this
    }

    fn get_manifest_once(self: &Rc<Self>) -> Promise<()> {
        let existing = self.extension_gallery_manifest_promise.borrow().clone();
        match existing {
            Some(promise) => promise,
            None => {
                let promise = self.do_get_extension_gallery_manifest();
                *self.extension_gallery_manifest_promise.borrow_mut() = Some(promise.clone());
                promise
            }
        }
    }

    fn do_get_extension_gallery_manifest(self: &Rc<Self>) -> Promise<()> {
        let this = self.clone();
        async_fn(async move {
            let has_default_service_url = this.product_service.extensions_gallery().is_some_and(|gallery| !gallery.service_url.is_empty());
            if !has_default_service_url {
                return Ok(());
            }

            let configured_service_url = match this.configuration_service.get_value(Some(EXTENSION_GALLERY_SERVICE_URL_CONFIG_KEY), None) {
                Dynamic::String(url) => Some(url.to_string_lossy()).filter(|url| !url.is_empty()),
                _ => None,
            };
            if let Some(configured_service_url) = configured_service_url {
                this.log_service.trace_args("[Marketplace] Private marketplace configured, checking access and fetching manifest", &[LogArg::from(configured_service_url.as_str())]);
                let weak = Rc::downgrade(&this);
                let url = configured_service_url.clone();
                this.store.add(this.gallery_account_service.on_did_change_account().listen(move |_| {
                    if let Some(this) = weak.upgrade() {
                        let _ = this.handle_marketplace_account_access(url.clone());
                    }
                }));
                this.handle_marketplace_account_access(configured_service_url).await?;
            } else {
                let default_extension_gallery_manifest = this.base.get_extension_gallery_manifest().await?;
                this.update(default_extension_gallery_manifest, None);
            }

            let weak = Rc::downgrade(&this);
            this.store.add(this.configuration_service.on_did_change_configuration().listen(move |e| {
                let Some(this) = weak.upgrade() else { return };
                if e.affects_configuration(EXTENSION_GALLERY_SERVICE_URL_CONFIG_KEY, None) {
                    let _ = this.request_restart(localize(
                        "extensionGalleryManifestService.accountChange",
                        "{0} is now configured to a different Marketplace. Please restart to apply the changes.",
                        &[this.product_service.name_long()],
                    ));
                } else if e.affects_configuration(EXTENSION_GALLERY_AUTH_PROVIDER_CONFIG_KEY, None) {
                    let _ = this.request_restart(localize(
                        "extensionGalleryManifestService.configurationChange",
                        "The Extensions Marketplace configuration has changed. Please restart to apply the changes.",
                        &[],
                    ));
                }
            }));
            Ok(())
        })
    }

    /// `beginResolution`: cancela a resolução em andamento e devolve o token da nova.
    fn begin_resolution(&self) -> CancellationToken {
        if let Some(current) = self.resolution_token_source.value() {
            current.cancel();
        }
        let source = Rc::new(CancellationTokenSource::new(None));
        let token = source.token();
        self.resolution_token_source.set(Some(source));
        token
    }

    fn handle_marketplace_account_access(self: &Rc<Self>, configured_service_url: String) -> Promise<()> {
        let this = self.clone();
        async_fn(async move {
            let token = this.begin_resolution();
            if let Err(error) = this.resolve_marketplace_account_access(&configured_service_url, &token).await {
                if token.is_cancellation_requested() {
                    return Ok(());
                }
                this.log_service.error_args(&LogMessage::from("[Marketplace] Error handling marketplace account access"), &[LogArg::from(&error)]);
                this.update(None, Some(ExtensionGalleryManifestStatus::RequiresSignIn));
            }
            Ok(())
        })
    }

    /// O `try` de `handleMarketplaceAccountAccess`: o `Err` é o que o `catch` externo trata.
    async fn resolve_marketplace_account_access(self: &Rc<Self>, configured_service_url: &str, token: &CancellationToken) -> Result<(), Error> {
        let account = self.gallery_account_service.get_account().await?;
        if token.is_cancellation_requested() {
            return Ok(());
        }
        if account.is_none() {
            // A transient failure to resolve the account is not a sign-out - Unknown means we could
            // not tell - so it must not retract a marketplace the user already has.
            if self.gallery_account_service.account_status() == ExtensionGalleryAccountStatus::Unknown && self.current_status.get() == ExtensionGalleryManifestStatus::Available {
                return Ok(());
            }
            self.log_service.debug("[Marketplace] Enterprise marketplace configured but user not signed in");
            self.update(None, Some(ExtensionGalleryManifestStatus::RequiresSignIn));
            return Ok(());
        }

        match self.gallery_account_service.account_status() {
            ExtensionGalleryAccountStatus::Unknown => {
                self.log_service.debug("[Marketplace] User signed in but account status is unknown");
                self.update(None, Some(ExtensionGalleryManifestStatus::RequiresSignIn));
            }
            ExtensionGalleryAccountStatus::Ineligible => {
                self.log_service.debug("[Marketplace] User signed in but lacks access to private marketplace");
                self.update(None, Some(ExtensionGalleryManifestStatus::AccessDenied));
            }
            ExtensionGalleryAccountStatus::SignedOut => {
                self.log_service.debug("[Marketplace] User signed out");
                self.update(None, Some(ExtensionGalleryManifestStatus::RequiresSignIn));
            }
            ExtensionGalleryAccountStatus::Eligible => match self.get_extension_gallery_manifest_from_service_url(configured_service_url.to_owned(), token.clone()).await {
                Ok(manifest) => {
                    if token.is_cancellation_requested() {
                        return Ok(());
                    }
                    self.update(Some(manifest), None);
                    self.telemetry_service.public_log2("galleryservice:custom:marketplace", None);
                }
                Err(error) => {
                    if token.is_cancellation_requested() {
                        return Ok(());
                    }
                    self.log_service.error_args(&LogMessage::from("[Marketplace] Error fetching manifest from custom marketplace"), &[LogArg::from(&error)]);
                    self.update(None, Some(ExtensionGalleryManifestStatus::AccessDenied));
                }
            },
        }
        Ok(())
    }

    fn update(&self, manifest: Option<IExtensionGalleryManifest>, status: Option<ExtensionGalleryManifestStatus>) {
        self.log_service.debug(&format!("[Marketplace] Updating manifest {}", if manifest.is_some() { "available" } else { "unavailable" }));
        // `this.extensionGalleryManifest !== manifest`: ver a nota do módulo.
        if self.extension_gallery_manifest.borrow().is_some() || manifest.is_some() {
            *self.extension_gallery_manifest.borrow_mut() = manifest.clone();
            self.on_did_change_extension_gallery_manifest.fire(manifest);
        }
        let status = status.unwrap_or_else(|| {
            if self.extension_gallery_manifest.borrow().is_some() { ExtensionGalleryManifestStatus::Available } else { ExtensionGalleryManifestStatus::Unavailable }
        });
        self.update_status(status);
    }

    fn update_status(&self, status: ExtensionGalleryManifestStatus) {
        if self.current_status.get() != status {
            self.current_status.set(status);
            self.on_did_change_extension_gallery_manifest_status.fire(status);
        }
    }

    fn request_restart(self: &Rc<Self>, message: String) -> Promise<()> {
        let this = self.clone();
        async_fn(async move {
            let confirmation = this
                .dialog_service
                .confirm(Confirmation { base: BaseDialogOptions::new(message), primary_button: Some(localize("restart", "&&Restart", &[])), cancel_button: None })
                .await?;
            if confirmation.confirmed {
                this.host_service.restart().await?;
            }
            Ok(())
        })
    }

    fn get_extension_gallery_manifest_from_service_url(self: &Rc<Self>, url: String, token: CancellationToken) -> Promise<IExtensionGalleryManifest> {
        let this = self.clone();
        async_fn(async move {
            let mut headers = this.common_headers_promise.clone().await?;
            headers.insert("Content-Type".to_owned(), HeaderValue::One("application/json".to_owned()));
            headers.insert("Accept-Encoding".to_owned(), HeaderValue::One("gzip".to_owned()));

            let result = async {
                let context = this
                    .request_service
                    .request(
                        IRequestOptions {
                            r#type: Some("GET".to_owned()),
                            url: Some(url),
                            headers: Some(headers),
                            call_site: "extensionGalleryManifestService.fetchManifest".to_owned(),
                            ..IRequestOptions::default()
                        },
                        token,
                    )
                    .await?;

                let extension_gallery_manifest = as_json(&context).await?;

                let Some(extension_gallery_manifest) = extension_gallery_manifest.filter(|manifest| !is_falsy(manifest)) else {
                    return Err(Error::new("Unable to retrieve extension gallery manifest."));
                };

                if !is_extension_gallery_manifest_response(&extension_gallery_manifest) {
                    return Err(Error::new("Service index response is not a valid extension gallery manifest."));
                }

                Ok::<IExtensionGalleryManifest, Error>(manifest_of_response(&extension_gallery_manifest))
            }
            .await;

            if let Err(error) = &result {
                this.log_service.error_args(&LogMessage::from("[Marketplace] Error retrieving extension gallery manifest"), &[LogArg::from(error)]);
            }
            result
        })
    }
}

impl IDisposable for WorkbenchExtensionGalleryManifestService {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl IExtensionGalleryManifestService for WorkbenchExtensionGalleryManifestService {
    fn extension_gallery_manifest_status(&self) -> ExtensionGalleryManifestStatus {
        self.current_status.get()
    }

    fn on_did_change_extension_gallery_manifest_status(&self) -> Event<ExtensionGalleryManifestStatus> {
        self.on_did_change_extension_gallery_manifest_status.event()
    }

    fn on_did_change_extension_gallery_manifest(&self) -> Event<Option<IExtensionGalleryManifest>> {
        self.on_did_change_extension_gallery_manifest.event()
    }

    fn get_extension_gallery_manifest(&self) -> Promise<Option<IExtensionGalleryManifest>> {
        let this = self.this.upgrade().expect("o serviço do manifesto da galeria está vivo");
        let manifest_promise = this.get_manifest_once();
        async_fn(async move {
            manifest_promise.await?;
            let manifest = this.extension_gallery_manifest.borrow().clone();
            Ok(manifest)
        })
    }
}
