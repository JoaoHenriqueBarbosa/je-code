//! Porte de `src/vs/workbench/services/extensions/browser/extensionUrlHandler.ts`: o tratador das URLs dirigidas a
//! extensões (`vscode://publisher.name/...`). Uma URL para uma extensão inativa é guardada, a extensão é ativada e a URL
//! é reaberta quando a extensão registra o seu tratador; a URL de uma extensão que nunca registra um tratador é
//! esquecida depois de cinco minutos. O serviço também garante que o usuário confirme a abertura de URLs de extensões.
//!
//! Notas de tradução:
//! - **`handleURLByExtension`** (só repassa a `handler.handleURL`) é a chamada direta, sem função própria.
//! - **`handleURL`** devolve um `Promise<bool>` (o corpo `async` do original é um `async_fn`), e os `await` são `.await`
//!   sobre `Promise`, que propaga a rejeição com `?` como o `await` relança.
//! - **`ExtensionUrlHandlerOverrideRegistry`** guarda os tratadores num `thread_local` (o `static handlers = new Set()`
//!   do original); a identidade do `Set` é a do `Rc`.
//! - **`ExtensionUrlBootstrapHandler`** tem o cache e o `disposable` estáticos num `thread_local`. O acesso a `cache`
//!   antes de existir um `disposable` (que no original lançaria `TypeError`) só devolve o cache.
//! - **`URI.revive(JSON.parse(urlToHandleValue))`** é o `revive_uri` do serializador de editores sem título.
//! - **`new Date().getTime()`** é o `date_now` do runtime; o `disposableWindowInterval(mainWindow, ...)` é o
//!   `set_interval` do runtime, desarmado no descarte.
//!
//! Fora do porte (e por quê):
//! - `registerWorkbenchContribution2(ExtensionUrlBootstrapHandler.ID, ...)`: o registro de contribuições do workbench
//!   (`common/contributions.ts`) ainda não foi portado; a classe está aqui e quem a instanciar passa o `IUrlService`.
//! - `ManageAuthorizedExtensionURIsAction` (`registerAction2` com `MenuId.CommandPalette`): o `Action2.run` do porte é
//!   síncrono e o original espera `quickInputService.pick`; entra com o porte do registro de contribuições.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use je_base::JsString;
use je_base::async_::{IntervalHandle, async_fn, clear_interval, set_interval};
use je_base::dynamic::{Dynamic, json_stringify};
use je_base::errors::is_cancellation_error;
use je_base::html_content::{MarkdownString, MarkdownStringOptions, TrustedOrOptions};
use je_base::lifecycle::{DisposableRef, IDisposable, combined_disposable, to_disposable};
use je_base::nls::localize;
use je_base::promise::Promise;
use je_base::runtime::{self, TimerId};
use je_base::strings::equals_ignore_case;
use je_base::uri::URI;
use je_extensions::manifest::ExtensionIdentifier;
use je_instantiation::{Injectable, InstantiationType, register_singleton, service_id, service_impl};
use je_platform::commands::commands::ICommandService;
use je_platform::configuration::service::IConfigurationService;
use je_platform::dialogs::dialogs::{BaseDialogOptions, Checkbox, Confirmation, CustomDialog, CustomDialogMarkdown, CustomDialogOptions, DialogDetail, IDialogService};
use je_platform::notification::notification::{INotificationService, NotificationMessage};
use je_platform::product::IProductService;
use je_platform::storage::storage::{StorageScope, StorageService, StorageTarget};
use je_platform::url::url_service::{IUrlHandler, IUrlService, OpenUrlOptions};
use je_storage::StorageValue;

use crate::services::extensions::extensions::{ActivationKind, IExtensionService};
use crate::services::host::host::IHostService;
use crate::services::search::query_builder::uri_to_dynamic;
use crate::services::untitled::untitled_text_editor_handler::revive_uri;

const FIVE_MINUTES: u64 = 5 * 60 * 1000;
const THIRTY_SECONDS: u64 = 30 * 1000;
const URL_TO_HANDLE: &str = "extensionUrlHandler.urlToHandle";
const USER_TRUSTED_EXTENSIONS_CONFIGURATION_KEY: &str = "extensions.confirmedUriHandlerExtensionIds";
const USER_TRUSTED_EXTENSIONS_STORAGE_KEY: &str = "extensionUrlHandler.confirmedExtensions";

/// `isExtensionId(value)`: `/^[a-z0-9][a-z0-9\-]*\.[a-z0-9][a-z0-9\-]*$/i`.
fn is_extension_id(value: &str) -> bool {
    let is_part = |part: &str| {
        let mut chars = part.chars();
        chars.next().is_some_and(|first| first.is_ascii_alphanumeric()) && chars.all(|c| c.is_ascii_alphanumeric() || c == '-')
    };
    match value.split_once('.') {
        Some((publisher, name)) => is_part(publisher) && is_part(name),
        None => false,
    }
}

/// `UserTrustedExtensionIdStorage`.
pub struct UserTrustedExtensionIdStorage {
    storage_service: Rc<dyn StorageService>,
}

impl UserTrustedExtensionIdStorage {
    pub fn new(storage_service: Rc<dyn StorageService>) -> Self {
        Self { storage_service }
    }

    pub fn extensions(&self) -> Vec<String> {
        let user_trusted_extension_ids_json = self.storage_service.get(USER_TRUSTED_EXTENSIONS_STORAGE_KEY, StorageScope::Profile, Some("[]"));

        user_trusted_extension_ids_json.and_then(|json| serde_json::from_str(&json).ok()).unwrap_or_default()
    }

    pub fn has(&self, id: &str) -> bool {
        self.extensions().iter().any(|extension| extension == id)
    }

    pub fn add(&self, id: &str) {
        let mut ids = self.extensions();
        ids.push(id.to_string());
        self.set(&ids);
    }

    pub fn set(&self, ids: &[String]) {
        let json = serde_json::to_string(ids).unwrap_or_else(|_| "[]".to_string());
        self.storage_service.store(USER_TRUSTED_EXTENSIONS_STORAGE_KEY, StorageValue::String(json), StorageScope::Profile, StorageTarget::Machine);
    }
}

/// `IExtensionContributedURLHandler`.
pub trait IExtensionContributedUrlHandler: IUrlHandler {
    fn extension_display_name(&self) -> String;
}

/// `IExtensionUrlHandler`.
pub trait IExtensionUrlHandler {
    fn register_extension_handler(&self, extension_id: &ExtensionIdentifier, handler: Rc<dyn IExtensionContributedUrlHandler>);
    fn unregister_extension_handler(&self, extension_id: &ExtensionIdentifier);
}

service_id!(pub I_EXTENSION_URL_HANDLER: dyn IExtensionUrlHandler = "extensionUrlHandler");

/// `IExtensionUrlHandlerOverride`.
pub trait IExtensionUrlHandlerOverride {
    fn can_handle_url(&self, uri: &URI) -> bool;
    fn handle_url(&self, uri: &URI) -> Promise<bool>;
}

thread_local! {
    static OVERRIDE_HANDLERS: RefCell<Vec<Rc<dyn IExtensionUrlHandlerOverride>>> = const { RefCell::new(Vec::new()) };
}

/// `ExtensionUrlHandlerOverrideRegistry`.
pub struct ExtensionUrlHandlerOverrideRegistry;

impl ExtensionUrlHandlerOverrideRegistry {
    /// `registerHandler(handler)`. O `Set` do original não duplica: registrar o mesmo tratador duas vezes é uma só.
    pub fn register_handler(handler: Rc<dyn IExtensionUrlHandlerOverride>) -> DisposableRef {
        OVERRIDE_HANDLERS.with(|handlers| {
            let mut handlers = handlers.borrow_mut();
            if !handlers.iter().any(|existing| Rc::ptr_eq(existing, &handler)) {
                handlers.push(handler.clone());
            }
        });

        to_disposable(move || OVERRIDE_HANDLERS.with(|handlers| handlers.borrow_mut().retain(|existing| !Rc::ptr_eq(existing, &handler))))
    }

    /// `getHandler(uri)`.
    pub fn get_handler(uri: &URI) -> Option<Rc<dyn IExtensionUrlHandlerOverride>> {
        let handlers: Vec<Rc<dyn IExtensionUrlHandlerOverride>> = OVERRIDE_HANDLERS.with(|handlers| handlers.borrow().clone());
        handlers.into_iter().find(|handler| handler.can_handle_url(uri))
    }
}

/// `{ timestamp: number; uri: URI }`.
#[derive(Clone)]
struct BufferedUri {
    timestamp: u64,
    uri: URI,
}

/// Concatena trechos UTF-16 num `JsString`.
fn js_concat(parts: &[&[u16]]) -> JsString {
    let mut result = JsString::from("");
    for part in parts {
        result.push_units(part);
    }
    result
}

/// `ExtensionUrlHandler`: trata URLs dirigidas a extensões.
pub struct ExtensionUrlHandler {
    this: Weak<ExtensionUrlHandler>,

    extension_service: Rc<dyn IExtensionService>,
    dialog_service: Rc<dyn IDialogService>,
    command_service: Rc<dyn ICommandService>,
    host_service: Rc<dyn IHostService>,
    storage_service: Rc<dyn StorageService>,
    configuration_service: Rc<dyn IConfigurationService>,
    notification_service: Rc<dyn INotificationService>,
    product_service: Rc<dyn IProductService>,

    extension_handlers: RefCell<HashMap<String, Rc<dyn IExtensionContributedUrlHandler>>>,
    uri_buffer: RefCell<HashMap<String, Vec<BufferedUri>>>,
    user_trusted_extensions_storage: UserTrustedExtensionIdStorage,
    disposable: RefCell<Option<DisposableRef>>,
}

impl Injectable for ExtensionUrlHandler {
    type Args = ();
    type Deps = (
        Rc<dyn IUrlService>,
        Rc<dyn IExtensionService>,
        Rc<dyn IDialogService>,
        Rc<dyn ICommandService>,
        Rc<dyn IHostService>,
        Rc<dyn StorageService>,
        Rc<dyn IConfigurationService>,
        Rc<dyn INotificationService>,
        Rc<dyn IProductService>,
    );

    fn create(
        (): Self::Args,
        (url_service, extension_service, dialog_service, command_service, host_service, storage_service, configuration_service, notification_service, product_service): Self::Deps,
    ) -> Rc<Self> {
        let this = Rc::new_cyclic(|this| Self {
            this: this.clone(),
            extension_service,
            dialog_service,
            command_service,
            host_service,
            storage_service: storage_service.clone(),
            configuration_service,
            notification_service,
            product_service,
            extension_handlers: RefCell::new(HashMap::new()),
            uri_buffer: RefCell::new(HashMap::new()),
            user_trusted_extensions_storage: UserTrustedExtensionIdStorage::new(storage_service),
            disposable: RefCell::new(None),
        });

        let weak = this.this.clone();
        let interval: IntervalHandle = set_interval(
            move || {
                if let Some(this) = weak.upgrade() {
                    this.garbage_collect();
                }
            },
            THIRTY_SECONDS,
        );
        let interval = to_disposable(move || clear_interval(&interval));

        let url_to_handle_value = this.storage_service.get(URL_TO_HANDLE, StorageScope::Workspace, None);
        if let Some(url_to_handle_value) = url_to_handle_value.filter(|value| !value.is_empty()) {
            this.storage_service.remove(URL_TO_HANDLE, StorageScope::Workspace);
            let parsed: serde_json::Value = serde_json::from_str(&url_to_handle_value).unwrap_or(serde_json::Value::Null);
            let _ = this.handle_url_impl(revive_uri(&parsed), Some(OpenUrlOptions { trusted: Some(true), original_url: None }));
        }

        let cache = ExtensionUrlBootstrapHandler::cache();
        let weak = this.this.clone();
        let drain_timeout: TimerId = runtime::set_timeout(
            move || {
                if let Some(this) = weak.upgrade() {
                    for (uri, option) in cache {
                        let _ = this.handle_url_impl(uri, option);
                    }
                }
            },
            0,
        );

        let handler: Rc<dyn IUrlHandler> = this.clone();
        *this.disposable.borrow_mut() = Some(combined_disposable(vec![
            url_service.register_handler(handler),
            interval,
            to_disposable(move || runtime::clear_timeout(drain_timeout)),
        ]));

        this
    }
}

service_impl!(ExtensionUrlHandler => dyn IExtensionUrlHandler, disposable);

/// `registerSingleton(IExtensionUrlHandler, ExtensionUrlHandler, InstantiationType.Eager)`.
pub fn register() {
    register_singleton::<ExtensionUrlHandler>(I_EXTENSION_URL_HANDLER, InstantiationType::Eager);
}

impl IUrlHandler for ExtensionUrlHandler {
    fn handle_url(&self, uri: &URI, options: Option<&OpenUrlOptions>) -> Promise<bool> {
        self.this.upgrade().expect("o tratador de URLs de extensão foi descartado").handle_url_impl(uri.clone(), options.cloned())
    }
}

impl IExtensionUrlHandler for ExtensionUrlHandler {
    fn register_extension_handler(&self, extension_id: &ExtensionIdentifier, handler: Rc<dyn IExtensionContributedUrlHandler>) {
        let key = ExtensionIdentifier::to_key(&extension_id.value);
        self.extension_handlers.borrow_mut().insert(key.clone(), handler.clone());

        let uris = self.uri_buffer.borrow_mut().remove(&key).unwrap_or_default();

        for BufferedUri { uri, .. } in uris {
            let _ = handler.handle_url(&uri, None);
        }
    }

    fn unregister_extension_handler(&self, extension_id: &ExtensionIdentifier) {
        self.extension_handlers.borrow_mut().remove(&ExtensionIdentifier::to_key(&extension_id.value));
    }
}

impl ExtensionUrlHandler {
    /// `handleURL(uri, options?)`.
    fn handle_url_impl(self: Rc<Self>, uri: URI, options: Option<OpenUrlOptions>) -> Promise<bool> {
        async_fn(async move {
            let authority = uri.authority().to_string_lossy();
            if !is_extension_id(&authority) {
                return Ok(false);
            }

            let override_handler = ExtensionUrlHandlerOverrideRegistry::get_handler(&uri);
            let extension_id = authority;

            let initial_handler = self.extension_handlers.borrow().get(&ExtensionIdentifier::to_key(&extension_id)).cloned();
            let extension_display_name: String;
            let mut extension_installed = initial_handler.is_some();

            match &initial_handler {
                None => {
                    // The extension is not yet activated, so let's check if it is installed and enabled
                    let extension = self.extension_service.get_extension(&extension_id).await?;
                    extension_installed = extension.is_some();
                    if extension.is_none() && override_handler.is_none() {
                        self.clone().handle_unhandled_url(uri, extension_id, options).await?;
                        return Ok(true);
                    }
                    extension_display_name = extension.and_then(|extension| extension.display_name).unwrap_or_else(|| extension_id.clone());
                }
                Some(initial_handler) => {
                    extension_display_name = initial_handler.extension_display_name();
                }
            }

            let trusted = options.as_ref().and_then(|options| options.trusted).unwrap_or(false)
                || self
                    .product_service
                    .trusted_extension_protocol_handlers()
                    .iter()
                    .any(|value| equals_ignore_case(&value.encode_utf16().collect::<Vec<u16>>(), &extension_id.encode_utf16().collect::<Vec<u16>>()))
                || self.did_user_trust_extension(&ExtensionIdentifier::to_key(&extension_id));

            if !trusted {
                let uri_string = uri.to_string(false);
                let mut uri_label = uri_string.clone();

                if uri_label.len() > 40 {
                    let units = uri_label.units();
                    uri_label = js_concat(&[&units[..30], &"...".encode_utf16().collect::<Vec<u16>>(), &units[units.len() - 5..]]);
                }

                let markdown = MarkdownString::new(
                    Some(js_concat(&[
                        &"<div title=\"".encode_utf16().collect::<Vec<u16>>(),
                        uri_string.units(),
                        &"\" aria-label='".encode_utf16().collect::<Vec<u16>>(),
                        uri_string.units(),
                        &"'>".encode_utf16().collect::<Vec<u16>>(),
                        uri_label.units(),
                        &"</div>".encode_utf16().collect::<Vec<u16>>(),
                    ])),
                    TrustedOrOptions::Options(MarkdownStringOptions { support_html: Some(true), ..MarkdownStringOptions::default() }),
                );

                let result = self
                    .dialog_service
                    .confirm(Confirmation {
                        base: BaseDialogOptions {
                            checkbox: Some(Checkbox { label: localize("rememberConfirmUrl", "Do not ask me again for this extension", &[]), checked: None }),
                            custom: Some(CustomDialog::Options(CustomDialogOptions {
                                markdown_details: Some(vec![CustomDialogMarkdown { markdown, classes: None }]),
                                ..CustomDialogOptions::default()
                            })),
                            ..BaseDialogOptions::new(localize("confirmUrl", "Allow '{0}' extension to open this URI?", &[&extension_display_name]))
                        },
                        primary_button: Some(localize("open", "&&Open", &[])),
                        cancel_button: None,
                    })
                    .await?;

                if !result.confirmed {
                    return Ok(true);
                }

                if result.checkbox_checked == Some(true) {
                    self.user_trusted_extensions_storage.add(&ExtensionIdentifier::to_key(&extension_id));
                }
            }

            if let Some(override_handler) = override_handler {
                let handled = override_handler.handle_url(&uri).await?;
                if handled {
                    return Ok(handled);
                }
            }

            if !extension_installed {
                let options = OpenUrlOptions { trusted: Some(true), ..options.unwrap_or_default() };
                self.clone().handle_unhandled_url(uri, extension_id, Some(options)).await?;
                return Ok(true);
            }

            let handler = self.extension_handlers.borrow().get(&ExtensionIdentifier::to_key(&extension_id)).cloned();

            if let Some(handler) = handler {
                if initial_handler.is_none() {
                    // forward it directly
                    return handler.handle_url(&uri, options.as_ref()).await;
                }

                // let the ExtensionUrlHandler instance handle this
                return Ok(false);
            }

            // collect URI for eventual extension activation
            let timestamp = runtime::date_now();
            self.uri_buffer.borrow_mut().entry(ExtensionIdentifier::to_key(&extension_id)).or_default().push(BufferedUri { timestamp, uri });

            // activate the extension using ActivationKind.Immediate because URI handling might be part
            // of resolving authorities (via authentication extensions)
            self.extension_service.activate_by_event(&format!("onUri:{}", ExtensionIdentifier::to_key(&extension_id)), Some(ActivationKind::Immediate)).await?;
            Ok(true)
        })
    }

    /// `handleUnhandledURL(uri, extensionId, options?)`.
    fn handle_unhandled_url(self: Rc<Self>, uri: URI, extension_id: String, options: Option<OpenUrlOptions>) -> Promise<()> {
        async_fn(async move {
            let install_options = Dynamic::object([
                (
                    "justification",
                    Dynamic::object([
                        (
                            "reason",
                            Dynamic::string(&format!("{}\n{}", localize("installDetail", "This extension wants to open a URI:", &[]), uri.to_string(false))),
                        ),
                        ("action", Dynamic::string(&localize("openUri", "Open URI", &[]))),
                    ]),
                ),
                ("enable", Dynamic::Bool(true)),
                ("installPreReleaseVersion", Dynamic::Bool(self.product_service.quality() != Some("stable"))),
            ]);
            let installed = self.command_service.execute_command("workbench.extensions.installExtension", vec![Dynamic::string(&extension_id), install_options]).await;
            if let Err(error) = installed {
                if !is_cancellation_error(&error) {
                    self.notification_service.error(vec![NotificationMessage::Error(error)]);
                }
                return Ok(());
            }

            let extension = self.extension_service.get_extension(&extension_id).await?;

            if extension.is_some() {
                let options = OpenUrlOptions { trusted: Some(true), ..options.unwrap_or_default() };
                self.clone().handle_url_impl(uri, Some(options)).await?;
            }
            /* Extension cannot be added and require window reload */
            else {
                let result = self
                    .dialog_service
                    .confirm(Confirmation {
                        base: BaseDialogOptions::new(localize(
                            "reloadAndHandle",
                            "Extension '{0}' is not loaded. Would you like to reload the window to load the extension and open the URL?",
                            &[&extension_id],
                        )),
                        primary_button: Some(localize("reloadAndOpen", "&&Reload Window and Open", &[])),
                        cancel_button: None,
                    })
                    .await?;

                if !result.confirmed {
                    return Ok(());
                }

                let serialized = json_stringify(&uri_to_dynamic(&uri)).unwrap_or_default();
                self.storage_service.store(URL_TO_HANDLE, StorageValue::String(serialized), StorageScope::Workspace, StorageTarget::Machine);
                self.host_service.reload(None).await?;
            }
            Ok(())
        })
    }

    /// `garbageCollect()`: forget about all uris buffered more than 5 minutes ago.
    fn garbage_collect(&self) {
        let now = runtime::date_now();
        let mut uri_buffer: HashMap<String, Vec<BufferedUri>> = HashMap::new();

        for (extension_id, uris) in self.uri_buffer.borrow().iter() {
            let uris: Vec<BufferedUri> = uris.iter().filter(|buffered| now - buffered.timestamp < FIVE_MINUTES).cloned().collect();

            if !uris.is_empty() {
                uri_buffer.insert(extension_id.clone(), uris);
            }
        }

        *self.uri_buffer.borrow_mut() = uri_buffer;
    }

    /// `didUserTrustExtension(id)`.
    fn did_user_trust_extension(&self, id: &str) -> bool {
        if self.user_trusted_extensions_storage.has(id) {
            return true;
        }

        self.get_confirmed_trusted_extension_ids_from_configuration().iter().any(|trusted| trusted == id)
    }

    /// `getConfirmedTrustedExtensionIdsFromConfiguration()`.
    fn get_confirmed_trusted_extension_ids_from_configuration(&self) -> Vec<String> {
        match self.configuration_service.get_value(Some(USER_TRUSTED_EXTENSIONS_CONFIGURATION_KEY), None) {
            Dynamic::Array(ids) => (0..ids.len())
                .filter_map(|index| match ids.get(index) {
                    Dynamic::String(id) => Some(id.to_string_lossy()),
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        }
    }
}

impl IDisposable for ExtensionUrlHandler {
    fn dispose(&self) {
        if let Some(disposable) = self.disposable.borrow_mut().take() {
            disposable.dispose();
        }
        self.extension_handlers.borrow_mut().clear();
        self.uri_buffer.borrow_mut().clear();
    }
}

thread_local! {
    static BOOTSTRAP_CACHE: RefCell<Vec<(URI, Option<OpenUrlOptions>)>> = const { RefCell::new(Vec::new()) };
    static BOOTSTRAP_DISPOSABLE: RefCell<Option<DisposableRef>> = const { RefCell::new(None) };
}

/// `ExtensionUrlBootstrapHandler`: trata as URLs antes de o `ExtensionUrlHandler` ser instanciado
/// (https://github.com/microsoft/vscode/issues/73101).
pub struct ExtensionUrlBootstrapHandler;

impl ExtensionUrlBootstrapHandler {
    /// `ExtensionUrlBootstrapHandler.ID`.
    pub const ID: &'static str = "workbench.contrib.extensionUrlBootstrapHandler";

    /// `static get cache()`: desarma o registro e esvazia o cache.
    pub fn cache() -> Vec<(URI, Option<OpenUrlOptions>)> {
        if let Some(disposable) = BOOTSTRAP_DISPOSABLE.with(|disposable| disposable.borrow_mut().take()) {
            disposable.dispose();
        }

        BOOTSTRAP_CACHE.with(|cache| std::mem::take(&mut *cache.borrow_mut()))
    }
}

impl Injectable for ExtensionUrlBootstrapHandler {
    type Args = ();
    type Deps = (Rc<dyn IUrlService>,);

    fn create((): Self::Args, (url_service,): Self::Deps) -> Rc<Self> {
        let this = Rc::new(Self);
        let handler: Rc<dyn IUrlHandler> = this.clone();
        let disposable = url_service.register_handler(handler);
        BOOTSTRAP_DISPOSABLE.with(|slot| *slot.borrow_mut() = Some(disposable));
        this
    }
}

impl IUrlHandler for ExtensionUrlBootstrapHandler {
    fn handle_url(&self, uri: &URI, options: Option<&OpenUrlOptions>) -> Promise<bool> {
        if !is_extension_id(&uri.authority().to_string_lossy()) {
            return Promise::resolved(false);
        }

        BOOTSTRAP_CACHE.with(|cache| cache.borrow_mut().push((uri.clone(), options.cloned())));
        Promise::resolved(true)
    }
}
