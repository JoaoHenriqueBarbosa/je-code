//! Porte de `src/vs/platform/policy/node/nativeManagedSettingsService.ts`: as configurações gerenciadas que o
//! sistema operacional entrega (MDM nativo), observadas por um vigia e mantidas como um mapa de chave para valor.
//!
//! Decisões de tradução:
//! - **O módulo nativo `@vscode/policy-watcher`** é a [`NativePolicyWatcherFactory`], a mesma injeção que o teste
//!   do original faz (`watcherFactory`). O módulo recebe o nome do produto, o `{ type }` de cada chave observada
//!   ([`NativePolicyDefinitions`]), um callback de atualização ([`NativePolicyCallback`]) e as opções
//!   ([`NativePolicyWatcherOptions`]), e devolve o vigia (algo descartável). Criá-lo pode falhar (`throw`), o que
//!   é o `Err` da fábrica.
//! - **`update: Record<string, PolicyValue | undefined>`** é um [`NativePolicyUpdate`]: `None` é a chave removida.
//! - **`ManagedSettingsData`** é o mapa de `je_base::policy`; a ordem de inserção do `Map` do original vale
//!   (`shift_remove` quando uma chave sai).
//! - **`initialize()` e `updatePolicyDefinitions()`** são métodos do trait [`INativeManagedSettingsService`], que
//!   só tem `&self`; o serviço guarda um `Weak` de si mesmo para obter o `Rc` que as funções assíncronas seguram.
//! - O vigia fica num `MutableDisposable`: trocá-lo descarta o anterior, e esvaziar as definições o descarta.
//!
//! Divergências:
//! - O original, sem fábrica injetada, carrega o módulo nativo (`await import('@vscode/policy-watcher')`). Aqui a
//!   fábrica é obrigatória no construtor: quem monta a aplicação injeta a do sistema. No Windows ela lê o
//!   registro (`SOFTWARE\Policies\<produto>`, ou `registryPath`), no macOS os plists de preferências gerenciadas
//!   (`CFPreferences`, com o identificador do pacote como nome do produto) e no Linux o arquivo
//!   `/etc/vscode/policy.json` (`LINUX_SYSTEM_POLICY_FILE_PATH`). A leitura do registro precisa de chamadas de
//!   sistema inseguras e o projeto proíbe `unsafe`, então ela não vive neste crate.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use indexmap::IndexMap;
use je_base::async_::{Throttler, async_fn};
use je_base::dynamic::{Dynamic, json_stringify};
use je_base::errors::Error;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, DisposableRef, IDisposable, MutableDisposable};
use je_base::policy::{IManagedSettingsPolicyDefinitions, ManagedSettingType, PolicyValue};
use je_base::promise::{Promise, Resolver};

use super::copilot_managed_settings::{INativeManagedSettingsService, MANAGED_SETTINGS_CONTROL_DEFINITIONS, ManagedSettingsData, collect_managed_settings_definitions};
use super::policy::PolicyDefinitions;
use crate::log::{LogArg, LogMessage, LogServiceRef};

/// `INativePolicyWatcherOptions`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NativePolicyWatcherOptions {
    pub registry_path: Option<String>,
}

/// `Record<string, { type: 'string' | 'number' | 'boolean' }>`: o que o vigia nativo observa.
pub type NativePolicyDefinitions = IndexMap<String, ManagedSettingType>;

/// `Record<string, PolicyValue | undefined>`: o que o vigia entrega a cada mudança.
pub type NativePolicyUpdate = IndexMap<String, Option<PolicyValue>>;

/// O callback `onDidChange` que o vigia chama com cada atualização.
pub type NativePolicyCallback = Rc<dyn Fn(NativePolicyUpdate)>;

type CreateWatcher = dyn Fn(&str, &NativePolicyDefinitions, NativePolicyCallback, Option<&NativePolicyWatcherOptions>) -> Result<DisposableRef, Error>;

/// `NativePolicyWatcherFactory`: `(productName, policies, onDidChange, options?) => Watcher`.
#[derive(Clone)]
pub struct NativePolicyWatcherFactory(Rc<CreateWatcher>);

impl NativePolicyWatcherFactory {
    pub fn new(create: impl Fn(&str, &NativePolicyDefinitions, NativePolicyCallback, Option<&NativePolicyWatcherOptions>) -> Result<DisposableRef, Error> + 'static) -> Self {
        Self(Rc::new(create))
    }

    /// Cria o vigia; `Err` é o `throw` do módulo nativo.
    pub fn create(
        &self,
        product_name: &str,
        policies: &NativePolicyDefinitions,
        on_did_change: NativePolicyCallback,
        options: Option<&NativePolicyWatcherOptions>,
    ) -> Result<DisposableRef, Error> {
        (self.0)(product_name, policies, on_did_change, options)
    }
}

/// O valor como [`Dynamic`], para o `JSON.stringify` do log.
fn policy_value_to_dynamic(value: &PolicyValue) -> Dynamic {
    match value {
        PolicyValue::String(text) => Dynamic::string(text),
        PolicyValue::Number(number) => Dynamic::Number(*number),
        PolicyValue::Boolean(flag) => Dynamic::Bool(*flag),
    }
}

/// `NativeManagedSettingsService`.
pub struct NativeManagedSettingsService {
    me: Weak<Self>,
    base: Rc<Disposable>,
    log_service: LogServiceRef,
    product_name: String,
    watcher_options: Option<NativePolicyWatcherOptions>,
    watcher_factory: NativePolicyWatcherFactory,
    throttler: Rc<Throttler<()>>,
    watcher: Rc<MutableDisposable<dyn IDisposable>>,
    managed_settings_values: RefCell<ManagedSettingsData>,
    watched_settings: RefCell<IManagedSettingsPolicyDefinitions>,
    initialization_promise: RefCell<Option<Promise<()>>>,
    initialization_version: Cell<u64>,
    on_did_change_managed_settings: Rc<Emitter<ManagedSettingsData>>,
}

impl NativeManagedSettingsService {
    pub fn new(log_service: LogServiceRef, product_name: &str, watcher_options: Option<NativePolicyWatcherOptions>, watcher_factory: NativePolicyWatcherFactory) -> Rc<Self> {
        let base = Disposable::new();
        let throttler = Rc::new(Throttler::new());
        base.register(throttler.clone());
        let watcher = MutableDisposable::<dyn IDisposable>::new();
        base.register(watcher.clone());
        let on_did_change_managed_settings: Rc<Emitter<ManagedSettingsData>> = Emitter::new();
        base.register(on_did_change_managed_settings.clone());

        Rc::new_cyclic(|me| Self {
            me: me.clone(),
            base,
            log_service,
            product_name: product_name.to_owned(),
            watcher_options,
            watcher_factory,
            throttler,
            watcher,
            managed_settings_values: RefCell::new(ManagedSettingsData::new()),
            watched_settings: RefCell::new(MANAGED_SETTINGS_CONTROL_DEFINITIONS.clone()),
            initialization_promise: RefCell::new(None),
            initialization_version: Cell::new(0),
            on_did_change_managed_settings,
        })
    }

    /// O `Rc` deste serviço, que as funções assíncronas seguram.
    fn rc(&self) -> Rc<Self> {
        self.me.upgrade().expect("o serviço está vivo enquanto é usado")
    }

    fn ensure_watcher(self: &Rc<Self>, force: bool) -> Promise<()> {
        if !force && let Some(promise) = self.initialization_promise.borrow().clone() {
            return promise;
        }
        let version = self.initialization_version.get() + 1;
        self.initialization_version.set(version);
        let update = self.update_watcher_and_track(version);
        *self.initialization_promise.borrow_mut() = Some(update.clone());
        update
    }

    fn update_watcher_and_track(self: &Rc<Self>, version: u64) -> Promise<()> {
        let this = self.clone();
        async_fn(async move {
            let result = this.update_watcher().await;
            if result.is_err() && this.initialization_version.get() == version {
                *this.initialization_promise.borrow_mut() = None;
            }
            result
        })
    }

    /// Remove os valores cujas chaves já não são observadas; devolve se algo saiu.
    fn prune_managed_settings_values(&self) -> bool {
        let watched = self.watched_settings.borrow();
        let mut values = self.managed_settings_values.borrow_mut();
        let before = values.len();
        values.retain(|key, _| watched.contains_key(key));
        values.len() != before
    }

    fn update_watcher(self: &Rc<Self>) -> Promise<()> {
        let this = self.clone();
        async_fn(async move {
            let definitions = this.get_managed_setting_definitions();
            this.log_service.trace(&format!("NativeManagedSettingsService#updateWatcher - Found {} managed-settings definitions", definitions.len()));
            if definitions.is_empty() {
                this.watcher.clear();
                let had_managed_settings = !this.managed_settings_values.borrow().is_empty();
                this.managed_settings_values.borrow_mut().clear();
                if had_managed_settings {
                    this.on_did_change_managed_settings.fire(this.managed_settings());
                }
                return Ok(());
            }

            let service = this.clone();
            this.throttler.queue(move |_| Promise::with_executor(move |resolver| service.create_watcher(definitions, &resolver))).await
        })
    }

    /// O executor do `new Promise((c, e) => ...)`: resolve na primeira atualização do vigia e rejeita se a
    /// criação falhar.
    fn create_watcher(self: &Rc<Self>, definitions: NativePolicyDefinitions, resolver: &Resolver<()>) {
        self.log_service.trace(&format!("Creating native managed-settings watcher for productName {}", self.product_name));
        let (weak, resolve) = (self.me.clone(), resolver.clone());
        let on_did_change: NativePolicyCallback = Rc::new(move |update| {
            if let Some(service) = weak.upgrade() {
                service.on_did_managed_settings_change(update);
            }
            resolve.resolve(());
        });
        match self.watcher_factory.create(&self.product_name, &definitions, on_did_change, self.watcher_options.as_ref()) {
            Ok(watcher) => self.watcher.set(Some(watcher)),
            Err(error) => {
                self.log_service.error_args(&LogMessage::from("NativeManagedSettingsService#updateWatcher - Error creating watcher:"), &[LogArg::Error(error.clone())]);
                resolver.reject(error);
            }
        }
    }

    /// Project the internal definitions down to the minimal `{ type }` payload the native module expects.
    /// Deliberately a fresh, narrowly-typed copy rather than the internal state: it decouples the two shapes so a
    /// future field on `IManagedSettingPolicyDefinition` cannot silently leak across the native boundary.
    fn get_managed_setting_definitions(&self) -> NativePolicyDefinitions {
        self.watched_settings.borrow().iter().map(|(key, definition)| (key.clone(), definition.r#type)).collect()
    }

    fn on_did_managed_settings_change(&self, update: NativePolicyUpdate) {
        let updated = Dynamic::object(update.iter().filter_map(|(key, value)| value.as_ref().map(|value| (key.clone(), policy_value_to_dynamic(value)))));
        self.log_service.trace(&format!("NativeManagedSettingsService#_onDidManagedSettingsChange - Updated managed-settings values: {}", json_stringify(&updated).unwrap_or_default()));

        let mut changed = false;
        {
            let mut values = self.managed_settings_values.borrow_mut();
            for (key, value) in update {
                match value {
                    None => changed = values.shift_remove(&key).is_some() || changed,
                    Some(value) => {
                        if values.get(&key) != Some(&value) {
                            values.insert(key, value);
                            changed = true;
                        }
                    }
                }
            }
        }
        if changed {
            self.on_did_change_managed_settings.fire(self.managed_settings());
        }
    }
}

impl INativeManagedSettingsService for NativeManagedSettingsService {
    fn managed_settings(&self) -> ManagedSettingsData {
        self.managed_settings_values.borrow().clone()
    }

    fn on_did_change_managed_settings(&self) -> Event<ManagedSettingsData> {
        self.on_did_change_managed_settings.event()
    }

    fn initialize(&self) -> Promise<ManagedSettingsData> {
        let this = self.rc();
        async_fn(async move {
            this.ensure_watcher(false).await?;
            Ok(this.managed_settings())
        })
    }

    fn update_policy_definitions(&self, policy_definitions: PolicyDefinitions) -> Promise<ManagedSettingsData> {
        let this = self.rc();
        async_fn(async move {
            let mut managed_settings = collect_managed_settings_definitions(&policy_definitions);
            managed_settings.extend(MANAGED_SETTINGS_CONTROL_DEFINITIONS.iter().map(|(key, definition)| (key.clone(), definition.clone())));

            if *this.watched_settings.borrow() == managed_settings {
                return this.initialize().await;
            }

            *this.watched_settings.borrow_mut() = managed_settings;
            let changed = this.prune_managed_settings_values();
            this.ensure_watcher(true).await?;
            if changed {
                this.on_did_change_managed_settings.fire(this.managed_settings());
            }
            Ok(this.managed_settings())
        })
    }
}

impl IDisposable for NativeManagedSettingsService {
    fn dispose(&self) {
        self.base.dispose();
    }
}
