//! Porte de `src/vs/workbench/contrib/externalUriOpener/common/externalUriOpenerService.ts`: o `ExternalUriOpenerService`, que
//! escolhe (pela configuração `workbench.externalUriOpeners`, pela prioridade que cada opener declara ou perguntando ao
//! usuário) o opener externo contribuído por extensões que abre um endereço `http` ou `https`, e se registra no
//! `IOpenerService` como opener externo.
//!
//! Divergências:
//! - Os decoradores `@IOpenerService`, `@IConfigurationService`, `@ILogService`, `@IPreferencesService` e `@IQuickInputService`
//!   são argumentos de [`ExternalUriOpenerService::new`]. O `IPreferencesService` é o recorte reduzido de
//!   `services::preferences::preferences` (só `openUserSettings`). `_serviceBrand` não existe; o identificador é
//!   [`I_EXTERNAL_URI_OPENER_SERVICE`].
//! - `AsyncIterable<IExternalUriOpener>` de [`IExternalOpenerProvider::get_openers`] é a `Promise` da lista completa dos
//!   openers: o `for await` do `getAllOpenersForUri` só os consome até o fim, e o que ele faz com cada um (guardar no
//!   `Map` pelo id) é o mesmo depois de a lista chegar.
//! - `IExternalUriOpener.id` e `label` são métodos; `canOpen` devolve a `Promise` do [`ExternalUriOpenerPriority`] do editor.
//!   O `ctx` de `openExternalUri` (`{ sourceUri, viewColumn? }`) é [`ExternalUriOpenerContext`], e o de `getOpener`
//!   (`{ sourceUri, preferredOpenerId? }`) é [`GetOpenerContext`]; o `ctx` de `openExternal` é o [`ExternalOpenerContext`]
//!   do `IOpenerService`, que tem os três campos.
//! - O estado que os métodos `async` usam (provedores, serviços) fica num [`Core`] compartilhado por `Rc`, como no
//!   `OpenerService`, porque cada `async fn` do original vira uma função cujo futuro precisa ser dono do que lê.
//!   Cada método `async` do original é um `async fn` e a interface expõe a `Promise` por [`async_fn`].
//! - `Map<string, IExternalUriOpener>` é um [`IndexMap`] (`set` num id existente mantém a posição, como o `Map`), e o
//!   `LinkedList` de provedores é o [`LinkedList`] de `je_base`.
//! - `PickItem` (`IQuickPickItem & { opener?: IExternalUriOpener | 'configureDefault' }`): o campo `opener` vai no `extra` do
//!   item, num objeto com a carga nativa [`OpenerPick`] (`undefined`, `'configureDefault'` ou o opener).
//! - `Object.entries(config)` percorre os pares do objeto dinâmico da configuração (`|| {}` cobre qualquer valor que não
//!   seja objeto), e um id que não é texto nunca casa com um opener (o `Map.get` de uma chave que não é texto não acha nada).
//! - `isWeb` é `Platform::detect().is_web`; `toString()` de um `URI` é `to_string(false)`.

use std::cell::RefCell;
use std::rc::Rc;

use indexmap::IndexMap;
use je_base::JsString;
use je_base::async_::async_fn;
use je_base::cancellation::CancellationToken;
use je_base::dynamic::{Dynamic, ObjectRef};
use je_base::errors::Error;
use je_base::lifecycle::{Disposable, DisposableRef, IDisposable, plain_disposable};
use je_base::linked_list::LinkedList;
use je_base::nls::localize;
use je_base::platform::Platform;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_editor::languages::ExternalUriOpenerPriority;
use je_instantiation::service_id;
use je_platform::configuration::service::IConfigurationService;
use je_platform::log::{LogServiceRef, Logger};
use je_platform::opener::editor_types::ViewColumn;
use je_platform::opener::{ExternalOpenerContext, IExternalOpener, IOpenerService};
use je_platform::quickinput::quick_input::{IPickOptions, IQuickInputService, IQuickItem, IQuickPickItem, IQuickPickSeparator, QuickPickInput, QuickPicks};
use je_platform::url::{UrlInput, test_url_matches_glob_js};

use super::configuration::{DEFAULT_EXTERNAL_URI_OPENER_ID, EXTERNAL_URI_OPENERS_SETTING_ID};
use crate::services::preferences::preferences::{IOpenSettingsOptions, IPreferencesService, RevealSetting};

service_id!(pub I_EXTERNAL_URI_OPENER_SERVICE: dyn IExternalUriOpenerService = "externalUriOpenerService");

/// `IExternalOpenerProvider`.
pub trait IExternalOpenerProvider {
    /// `getOpeners(targetUri)`: os openers do provedor para o endereço (ver a nota do módulo sobre o `AsyncIterable`).
    fn get_openers(&self, target_uri: &URI) -> Promise<Vec<Rc<dyn IExternalUriOpener>>>;
}

/// O `ctx` de `IExternalUriOpener.openExternalUri`: `{ sourceUri: URI; viewColumn?: ViewColumn }`.
#[derive(Clone)]
pub struct ExternalUriOpenerContext {
    pub source_uri: URI,
    pub view_column: Option<ViewColumn>,
}

/// O `ctx` de `IExternalUriOpenerService.getOpener`: `{ sourceUri: URI; preferredOpenerId?: string }`.
#[derive(Clone)]
pub struct GetOpenerContext {
    pub source_uri: URI,
    pub preferred_opener_id: Option<JsString>,
}

/// `IExternalUriOpener`.
pub trait IExternalUriOpener {
    fn id(&self) -> String;
    fn label(&self) -> String;

    fn can_open(&self, uri: &URI, token: &CancellationToken) -> Promise<ExternalUriOpenerPriority>;
    fn open_external_uri(&self, uri: &URI, ctx: &ExternalUriOpenerContext, token: &CancellationToken) -> Promise<bool>;
}

/// `IExternalUriOpenerService`.
pub trait IExternalUriOpenerService {
    /// Registers a provider for external resources openers.
    fn register_external_opener_provider(&self, provider: Rc<dyn IExternalOpenerProvider>) -> DisposableRef;

    /// Get the configured IExternalUriOpener for the uri.
    /// If there is no opener configured, then returns the first opener that can handle the uri.
    fn get_opener(&self, uri: &URI, ctx: &GetOpenerContext, token: &CancellationToken) -> Promise<Option<Rc<dyn IExternalUriOpener>>>;
}

/// O `opener` de um `PickItem`: `IExternalUriOpener | 'configureDefault' | undefined`.
#[derive(Clone)]
enum OpenerPick {
    /// `opener: undefined`: abrir com o opener padrão.
    Default,
    /// `opener: 'configureDefault'`.
    ConfigureDefault,
    Opener(Rc<dyn IExternalUriOpener>),
}

/// O resultado de `getConfiguredOpenerForUri`: `IExternalUriOpener | 'default'`.
enum ConfiguredOpener {
    Default,
    Opener(Rc<dyn IExternalUriOpener>),
}

type OpenerMap = IndexMap<String, Rc<dyn IExternalUriOpener>>;

/// O estado que os métodos `async` do serviço compartilham.
struct Core {
    providers: LinkedList<Rc<dyn IExternalOpenerProvider>>,
    configuration_service: Rc<dyn IConfigurationService>,
    log_service: LogServiceRef,
    preferences_service: Rc<dyn IPreferencesService>,
    quick_input_service: Rc<dyn IQuickInputService>,
}

impl Core {
    /// `getOpeners`.
    async fn get_openers(
        &self,
        target_uri: &URI,
        allow_optional: bool,
        ctx: &GetOpenerContext,
        token: &CancellationToken,
    ) -> Result<Vec<Rc<dyn IExternalUriOpener>>, Error> {
        let all_openers = self.get_all_openers_for_uri(target_uri).await?;

        if all_openers.is_empty() {
            return Ok(Vec::new());
        }

        // First see if we have a preferredOpener
        if let Some(preferred_opener_id) = ctx.preferred_opener_id.as_ref().filter(|id| !id.is_empty()) {
            if *preferred_opener_id == DEFAULT_EXTERNAL_URI_OPENER_ID {
                return Ok(Vec::new());
            }

            if let Some(preferred_opener) = all_openers.get(&preferred_opener_id.to_string_lossy()) {
                // Skip the `canOpen` check here since the opener was specifically requested.
                return Ok(vec![Rc::clone(preferred_opener)]);
            }
        }

        // Check to see if we have a configured opener
        if let Some(configured_opener) = self.get_configured_opener_for_uri(&all_openers, target_uri) {
            // Skip the `canOpen` check here since the opener was specifically requested.
            return Ok(match configured_opener {
                ConfiguredOpener::Default => Vec::new(),
                ConfiguredOpener::Opener(opener) => vec![opener],
            });
        }

        // Then check to see if there is a valid opener
        let valid_openers: Rc<RefCell<Vec<(Rc<dyn IExternalUriOpener>, ExternalUriOpenerPriority)>>> = Rc::default();
        let checks: Vec<Promise<()>> = all_openers
            .values()
            .map(|opener| {
                let (opener, valid_openers, log_service) = (Rc::clone(opener), Rc::clone(&valid_openers), Rc::clone(&self.log_service));
                let (source_uri, token) = (ctx.source_uri.clone(), token.clone());
                async_fn(async move {
                    let priority = match opener.can_open(&source_uri, &token).await {
                        Ok(priority) => priority,
                        Err(error) => {
                            log_service.error(&error.to_string());
                            return Ok(());
                        }
                    };

                    match priority {
                        ExternalUriOpenerPriority::Option | ExternalUriOpenerPriority::Default | ExternalUriOpenerPriority::Preferred => {
                            valid_openers.borrow_mut().push((opener, priority));
                        }
                        ExternalUriOpenerPriority::None => {}
                    }
                    Ok(())
                })
            })
            .collect();
        Promise::all(checks).await?;
        let valid_openers = valid_openers.take();

        if valid_openers.is_empty() {
            return Ok(Vec::new());
        }

        // See if we have a preferred opener first
        if let Some((preferred, _)) = valid_openers.iter().find(|(_, priority)| *priority == ExternalUriOpenerPriority::Preferred) {
            return Ok(vec![Rc::clone(preferred)]);
        }

        // See if we only have optional openers, use the default opener
        if !allow_optional && valid_openers.iter().all(|(_, priority)| *priority == ExternalUriOpenerPriority::Option) {
            return Ok(Vec::new());
        }

        Ok(valid_openers.into_iter().map(|(opener, _)| opener).collect())
    }

    /// `openExternal`.
    async fn open_external(&self, href: &JsString, ctx: &ExternalOpenerContext, token: &CancellationToken) -> Result<bool, Error> {
        let target_uri = URI::parse(href.clone(), false);

        let get_opener_context = GetOpenerContext { source_uri: ctx.source_uri.clone(), preferred_opener_id: ctx.preferred_opener_id.clone() };
        let all_openers = self.get_openers(&target_uri, false, &get_opener_context, token).await?;
        let open_context = ExternalUriOpenerContext { source_uri: ctx.source_uri.clone(), view_column: ctx.view_column };
        if all_openers.is_empty() {
            return Ok(false);
        } else if all_openers.len() == 1 {
            return all_openers[0].open_external_uri(&target_uri, &open_context, token).await;
        }

        // Otherwise prompt
        self.show_opener_prompt(&all_openers, &target_uri, &open_context, token).await
    }

    /// `getOpener`.
    async fn get_opener(&self, target_uri: &URI, ctx: &GetOpenerContext, token: &CancellationToken) -> Result<Option<Rc<dyn IExternalUriOpener>>, Error> {
        let all_openers = self.get_openers(target_uri, true, ctx, token).await?;
        Ok(all_openers.into_iter().next())
    }

    /// `getAllOpenersForUri`.
    async fn get_all_openers_for_uri(&self, target_uri: &URI) -> Result<OpenerMap, Error> {
        let all_openers: Rc<RefCell<OpenerMap>> = Rc::default();
        let collected: Vec<Promise<()>> = self
            .providers
            .iter()
            .map(|provider| {
                let (all_openers, target_uri) = (Rc::clone(&all_openers), target_uri.clone());
                async_fn(async move {
                    for opener in provider.get_openers(&target_uri).await? {
                        all_openers.borrow_mut().insert(opener.id(), opener);
                    }
                    Ok(())
                })
            })
            .collect();
        Promise::all(collected).await?;
        Ok(all_openers.take())
    }

    /// `getConfiguredOpenerForUri`.
    fn get_configured_opener_for_uri(&self, openers: &OpenerMap, target_uri: &URI) -> Option<ConfiguredOpener> {
        let config = match self.configuration_service.get_value(Some(EXTERNAL_URI_OPENERS_SETTING_ID), None) {
            Dynamic::Object(config) => config.entries(),
            _ => Vec::new(),
        };
        for (uri_glob, id) in config {
            if test_url_matches_glob_js(UrlInput::Uri(target_uri.clone()), &uri_glob) {
                let Dynamic::String(id) = id else { continue };
                if id == DEFAULT_EXTERNAL_URI_OPENER_ID {
                    return Some(ConfiguredOpener::Default);
                }

                if let Some(entry) = openers.get(&id.to_string_lossy()) {
                    return Some(ConfiguredOpener::Opener(Rc::clone(entry)));
                }
            }
        }
        None
    }

    /// `showOpenerPrompt`.
    async fn show_opener_prompt(
        &self,
        openers: &[Rc<dyn IExternalUriOpener>],
        target_uri: &URI,
        ctx: &ExternalUriOpenerContext,
        token: &CancellationToken,
    ) -> Result<bool, Error> {
        let pick_item = |label: String, opener: OpenerPick| {
            QuickPickInput::Item(IQuickPickItem {
                base: IQuickItem { label: JsString::from(label), ..IQuickItem::default() },
                extra: Dynamic::Object(ObjectRef::with_host(Rc::new(opener))),
                ..IQuickPickItem::default()
            })
        };

        let mut items: Vec<QuickPickInput> = openers.iter().map(|opener| pick_item(opener.label(), OpenerPick::Opener(Rc::clone(opener)))).collect();
        items.push(pick_item(
            if Platform::detect().is_web {
                localize("selectOpenerDefaultLabel.web", "Open in new browser window", &[])
            } else {
                localize("selectOpenerDefaultLabel", "Open in default browser", &[])
            },
            OpenerPick::Default,
        ));
        items.push(QuickPickInput::Separator(IQuickPickSeparator::default()));
        items.push(pick_item(localize("selectOpenerConfigureTitle", "Configure default opener...", &[]), OpenerPick::ConfigureDefault));

        let target_text = target_uri.to_string(false).to_string_lossy();
        let options = IPickOptions {
            place_holder: Some(localize("selectOpenerPlaceHolder", "How would you like to open: {0}", &[target_text.as_str()])),
            ..IPickOptions::default()
        };
        let picked = self.quick_input_service.pick(QuickPicks::Items(items), Some(&options), None).await?;

        let Some(picked) = picked else {
            // Still cancel the default opener here since we prompted the user
            return Ok(true);
        };

        let pick = match &picked.extra {
            Dynamic::Object(extra) => extra.host::<OpenerPick>().map(|pick| (*pick).clone()),
            _ => None,
        };
        match pick.unwrap_or(OpenerPick::Default) {
            // Fallback to default opener
            OpenerPick::Default => Ok(false),
            OpenerPick::ConfigureDefault => {
                self.preferences_service
                    .open_user_settings(Some(&IOpenSettingsOptions {
                        json_editor: Some(true),
                        reveal_setting: Some(RevealSetting { key: EXTERNAL_URI_OPENERS_SETTING_ID.to_owned(), edit: Some(true) }),
                        ..IOpenSettingsOptions::default()
                    }))
                    .await?;
                Ok(true)
            }
            OpenerPick::Opener(opener) => opener.open_external_uri(target_uri, ctx, token).await,
        }
    }
}

/// `ExternalUriOpenerService`.
pub struct ExternalUriOpenerService {
    base: Rc<Disposable>,
    core: Rc<Core>,
}

impl ExternalUriOpenerService {
    pub fn new(
        opener_service: Rc<dyn IOpenerService>,
        configuration_service: Rc<dyn IConfigurationService>,
        log_service: LogServiceRef,
        preferences_service: Rc<dyn IPreferencesService>,
        quick_input_service: Rc<dyn IQuickInputService>,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let core = Rc::new(Core { providers: LinkedList::new(), configuration_service, log_service, preferences_service, quick_input_service });
        let this = Rc::new(Self { base: Rc::clone(&base), core });
        base.register(opener_service.register_external_opener(this.clone()));
        this
    }
}

impl IExternalUriOpenerService for ExternalUriOpenerService {
    fn register_external_opener_provider(&self, provider: Rc<dyn IExternalOpenerProvider>) -> DisposableRef {
        let remove = self.core.providers.push(provider);
        plain_disposable(move || remove.remove())
    }

    fn get_opener(&self, uri: &URI, ctx: &GetOpenerContext, token: &CancellationToken) -> Promise<Option<Rc<dyn IExternalUriOpener>>> {
        let core = Rc::clone(&self.core);
        let (uri, ctx, token) = (uri.clone(), ctx.clone(), token.clone());
        async_fn(async move { core.get_opener(&uri, &ctx, &token).await })
    }
}

impl IExternalOpener for ExternalUriOpenerService {
    fn open_external(&self, href: &JsString, ctx: &ExternalOpenerContext, token: &CancellationToken) -> Promise<bool> {
        let core = Rc::clone(&self.core);
        let (href, ctx, token) = (href.clone(), ctx.clone(), token.clone());
        async_fn(async move { core.open_external(&href, &ctx, &token).await })
    }

    /// O `dispose` herdado de `Disposable`.
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl IDisposable for ExternalUriOpenerService {
    fn dispose(&self) {
        self.base.dispose();
    }
}
