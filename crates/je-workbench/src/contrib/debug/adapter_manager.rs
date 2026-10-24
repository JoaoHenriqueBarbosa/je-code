//! Porte de `src/vs/workbench/contrib/debug/browser/debugAdapterManager.ts`: o `AdapterManager` (os depuradores contribuídos
//! pelas extensões, as fábricas de adaptador e de descritor, a escolha do depurador de um arquivo) e o `Breakpoints` de
//! `common/breakpoints.ts`.
//!
//! Notas de tradução:
//! - **Pontos de extensão.** `debuggersExtPoint.setHandler(...)` e `breakpointsExtPoint.setHandler(...)` são os métodos
//!   [`AdapterManager::handle_debuggers_ext_point`] e [`AdapterManager::handle_breakpoints_ext_point`], que quem lê o ponto de
//!   extensão chama com o mesmo `extensions` e `delta` (as contribuições já como objetos JSON).
//! - **Serviços.** `IEditorService`, `ILanguageService`, `IDialogService`, `ICommandService`, `IQuickInputService`,
//!   `IMenuService` e a leitura do estado das extensões são o recorte [`IAdapterManagerUi`]; a configuração, o serviço de
//!   extensões e o de chaves de contexto são os serviços reais do porte; `IInstantiationService.createInstance(Debugger, ...)`
//!   é a fábrica [`AdapterManagerServices::debugger_services`] (os serviços do `Debugger`).
//! - **`ITextModel`.** `canSetBreakpointsIn(model)` só lê `model.getLanguageId()`: [`AdapterManager::can_set_breakpoints_in`]
//!   recebe esse id.
//! - NÃO foram portados: o esquema JSON de `launch.json` (`updateDebugAdapterSchema`, `launchSchema`, `jsonRegistry`, o
//!   `TaskDefinitionRegistry` e o agendador de rótulos de tarefa de 5 s), porque o registro de esquemas JSON e o serviço de
//!   tarefas não existem no porte e nada lógico do gerenciador lê o esquema; e o `lifecycleService.when(Eventually)`, que
//!   é [`AdapterManager::on_lifecycle_eventually`] (quem possui o ciclo de vida o chama).

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::{Rc, Weak};

use indexmap::IndexMap;
use je_base::async_::async_fn;
use je_base::dynamic::Dynamic;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{DisposableRef, DisposableStore, IDisposable, to_disposable};
use je_base::nls::localize;
use je_base::promise::Promise;
use je_extensions::scanner::ExtensionDescription;
use je_platform::configuration::ConfigurationOverrides;
use je_platform::configuration::service::IConfigurationService;
use je_platform::contextkey::contextkey::{ContextKeyExpr, ContextKeyExpression, ContextKeyMeta, IContextKey, IContextKeyService, RawContextKey};
use je_platform::workspace::WorkspaceFolder;
use serde_json::Value;

use super::configuration_manager::{ConfigurationManager, DynamicPick, DynamicProvider, GuessedDebugger, IManagerAdapterManager};
use super::debug::{IAdapterDescriptor, IAdapterManager, IConfig, IDebugAdapter, IDebugAdapterDescriptorFactory, IDebugAdapterFactory, IDebugSession};
use super::debugger::{Debugger, DebuggerServices, IDebuggerAdapterManager};
use super::debug_types::IDebugSessionFull;
use crate::services::extensions::extensions::IExtensionService;

/// `CONTEXT_DEBUGGERS_AVAILABLE`.
pub fn context_debuggers_available() -> RawContextKey {
    RawContextKey::new("debuggersAvailable", false, ContextKeyMeta::Hide)
}

/// `CONTEXT_DEBUG_EXTENSION_AVAILABLE`.
pub fn context_debug_extension_available() -> RawContextKey {
    RawContextKey::new("debugExtensionAvailable", true, ContextKeyMeta::Hide)
}

/// `Breakpoints` (`common/breakpoints.ts`): uma contribuição `breakpoints` de uma extensão.
pub struct Breakpoints {
    breakpoint_contribution: Value,
    breakpoints_when: Option<ContextKeyExpression>,
    context_key_service: Rc<dyn IContextKeyService>,
}

impl Breakpoints {
    pub fn new(breakpoint_contribution: Value, context_key_service: Rc<dyn IContextKeyService>) -> Self {
        let breakpoints_when = breakpoint_contribution.get("when").and_then(Value::as_str).and_then(|text| ContextKeyExpr::deserialize(Some(text)));
        Self { breakpoint_contribution, breakpoints_when, context_key_service }
    }

    pub fn language(&self) -> String {
        self.breakpoint_contribution.get("language").and_then(Value::as_str).unwrap_or_default().to_owned()
    }

    pub fn enabled(&self) -> bool {
        self.breakpoints_when.as_ref().is_none_or(|when| self.context_key_service.context_matches_rules(Some(when)))
    }
}

/// `IAdapterManagerDelegate`.
pub trait IAdapterManagerDelegate {
    fn on_did_new_session(&self) -> Event<Rc<dyn IDebugSession>>;
    fn configuration_manager(&self) -> Rc<ConfigurationManager>;
}

/// Uma ação contribuída ao menu `DebugCreateConfiguration` (`MenuItemAction`).
#[derive(Clone)]
pub struct ContributedAction {
    pub label: String,
    pub run: Rc<dyn Fn()>,
}

/// Um item do quick pick do `guessDebugger`.
#[derive(Clone)]
pub enum AdapterPick {
    Separator { label: String },
    Debugger { label: String, debugger: Rc<Debugger> },
    DynamicOptions { label: String, provider: DynamicProvider },
    InstallExtension { label: String },
    Contributed(ContributedAction),
}

/// O que o `AdapterManager` usa do editor, da linguagem, dos diálogos, dos comandos e do quick input.
pub trait IAdapterManagerUi {
    /// `editorService.activeTextEditorControl` quando é um editor de código: o `getLanguageId()` do modelo (`Some(None)` é um
    /// editor sem modelo).
    fn active_code_editor_language_id(&self) -> Option<Option<String>>;
    /// `languageService.getLanguageName(languageId)`.
    fn get_language_name(&self, language_id: &str) -> Option<String>;
    /// `dialogService.confirm({ type: Warning, message, primaryButton })`: `confirmed`.
    fn confirm(&self, message: &str, primary_button: &str) -> Promise<bool>;
    /// `commandService.executeCommand(id, ...args)`.
    fn execute_command(&self, id: &str, argument: Option<String>) -> Promise<()>;
    /// `quickInputService.pick(picks, { activeItem: picks[0], placeHolder })`: o índice escolhido.
    fn pick(&self, picks: &[AdapterPick], place_holder: &str) -> Promise<Option<usize>>;
    /// `menuService.getMenuActions(MenuId.DebugCreateConfiguration, contextKeyService)` achatado.
    fn contributed_create_configuration_actions(&self) -> Vec<ContributedAction>;
    /// `extensionService.getExtensionsStatus()`: os ids das extensões que já têm `activationTimes`.
    fn extensions_with_activation_times(&self) -> Vec<String>;
}

/// Um `added`/`extensions` do ponto de extensão `debuggers`.
#[derive(Clone)]
pub struct DebuggersContribution {
    pub description: Rc<ExtensionDescription>,
    pub value: Vec<Value>,
    /// `collector.error(message)`.
    pub report_error: Rc<dyn Fn(&str)>,
}

/// Os serviços do construtor do `AdapterManager`.
#[derive(Clone)]
pub struct AdapterManagerServices {
    pub configuration_service: Rc<dyn IConfigurationService>,
    pub extension_service: Rc<dyn IExtensionService>,
    pub context_key_service: Rc<dyn IContextKeyService>,
    pub ui: Rc<dyn IAdapterManagerUi>,
    /// Os serviços do `Debugger` (o `createInstance(Debugger, ...)`), sem o gerenciador de adaptadores.
    pub debugger_services: Rc<dyn Fn() -> DebuggerServices>,
}

/// `AdapterManager`.
pub struct AdapterManager {
    store: Rc<DisposableStore>,
    delegate: Rc<dyn IAdapterManagerDelegate>,
    services: AdapterManagerServices,
    debuggers: RefCell<Vec<Rc<Debugger>>>,
    adapter_descriptor_factories: RefCell<Vec<Rc<IDebugAdapterDescriptorFactory>>>,
    debug_adapter_factories: RefCell<IndexMap<String, Rc<dyn IDebugAdapterFactory>>>,
    debuggers_available: Rc<dyn IContextKey>,
    debug_extensions_available: Rc<dyn IContextKey>,
    on_did_register_debugger: Rc<Emitter<()>>,
    on_did_debuggers_ext_point_read: Rc<Emitter<()>>,
    breakpoint_contributions: RefCell<Vec<Breakpoints>>,
    debugger_when_keys: RefCell<HashSet<String>>,
    /// Extensions that were already active before any debugger activation events
    early_activated_extensions: RefCell<Option<HashSet<String>>>,
    used_debug_types: RefCell<HashSet<String>>,
    me: Weak<AdapterManager>,
}

impl AdapterManager {
    pub fn new(delegate: Rc<dyn IAdapterManagerDelegate>, services: AdapterManagerServices) -> Rc<Self> {
        let store = DisposableStore::new();
        let on_did_register_debugger = Emitter::new();
        let on_did_debuggers_ext_point_read = Emitter::new();
        store.add(on_did_register_debugger.clone());
        store.add(on_did_debuggers_ext_point_read.clone());

        let keys: RefCell<Vec<Rc<dyn IContextKey>>> = RefCell::new(Vec::new());
        services.context_key_service.buffer_change_events(&|| {
            keys.borrow_mut().push(context_debuggers_available().bind_to(&*services.context_key_service));
            keys.borrow_mut().push(context_debug_extension_available().bind_to(&*services.context_key_service));
        });
        let mut keys = keys.into_inner();
        let debug_extensions_available = keys.pop().expect("a chave de extensões");
        let debuggers_available = keys.pop().expect("a chave de depuradores");

        let this = Rc::new_cyclic(|me| Self {
            store: store.clone(),
            delegate: delegate.clone(),
            services: services.clone(),
            debuggers: RefCell::new(Vec::new()),
            adapter_descriptor_factories: RefCell::new(Vec::new()),
            debug_adapter_factories: RefCell::new(IndexMap::new()),
            debuggers_available,
            debug_extensions_available,
            on_did_register_debugger,
            on_did_debuggers_ext_point_read,
            breakpoint_contributions: RefCell::new(Vec::new()),
            debugger_when_keys: RefCell::new(HashSet::new()),
            early_activated_extensions: RefCell::new(None),
            used_debug_types: RefCell::new(HashSet::new()),
            me: me.clone(),
        });

        let weak = Rc::downgrade(&this);
        let context_weak = weak.clone();
        store.add(services.context_key_service.on_did_change_context().listen(move |e| {
            let Some(this) = context_weak.upgrade() else {
                return;
            };
            let keys = this.debugger_when_keys.borrow().clone();
            if e.affects_some(&keys) {
                this.debuggers_available.set(Dynamic::Bool(this.has_enabled_debuggers()));
            }
        }));
        let read_weak = weak.clone();
        store.add(this.on_did_debuggers_ext_point_read.event().listen(move |_| {
            if let Some(this) = read_weak.upgrade() {
                this.debug_extensions_available.set(Dynamic::Bool(!this.debuggers.borrow().is_empty()));
            }
        }));
        let session_weak = weak;
        store.add(delegate.on_did_new_session().listen(move |s| {
            if let Some(this) = session_weak.upgrade() {
                let debug_type = s.configuration().get("type").and_then(Value::as_str).unwrap_or_default().to_owned();
                this.used_debug_types.borrow_mut().insert(debug_type);
            }
        }));
        this
    }

    fn this(&self) -> Rc<AdapterManager> {
        self.me.upgrade().expect("o gerenciador está vivo")
    }

    /// O `lifecycleService.when(LifecyclePhase.Eventually).then(...)`: se nenhuma extensão com depurador foi carregada.
    pub fn on_lifecycle_eventually(&self) {
        self.debug_extensions_available.set(Dynamic::Bool(!self.debuggers.borrow().is_empty()));
    }

    /// O handler de `debuggersExtPoint`: `extensions` são todas as contribuições, `added` e `removed` o delta.
    pub fn handle_debuggers_ext_point(&self, extensions: &[DebuggersContribution], added: &[DebuggersContribution], removed: &[Vec<Value>]) {
        for added in added {
            for raw_adapter in &added.value {
                let raw_type = raw_adapter.get("type").and_then(Value::as_str).filter(|t| !t.is_empty());
                if raw_type.is_none() {
                    (added.report_error)(&localize("debugNoType", "Debugger 'type' can not be omitted and must be of type 'string'.", &[]));
                }
                if raw_type != Some("*") {
                    let existing = raw_type.and_then(|t| self.get_debugger(t));
                    match existing {
                        Some(existing) => existing.merge(raw_adapter, added.description.clone()),
                        None => {
                            let mut services = (self.services.debugger_services)();
                            services.adapter_manager = Some(self.this() as Rc<dyn IDebuggerAdapterManager>);
                            let dbg = Rc::new(Debugger::new(raw_adapter, added.description.clone(), services));
                            if let Some(when) = dbg.when() {
                                self.debugger_when_keys.borrow_mut().extend(when.keys());
                            }
                            self.debuggers.borrow_mut().push(dbg);
                        }
                    }
                }
            }
        }

        // take care of all wildcard contributions
        for extension in extensions {
            for raw_adapter in &extension.value {
                if raw_adapter.get("type").and_then(Value::as_str) == Some("*") {
                    for dbg in self.debuggers.borrow().iter() {
                        dbg.merge(raw_adapter, extension.description.clone());
                    }
                }
            }
        }

        for removed in removed {
            let removed_types: Vec<Option<String>> = removed.iter().map(|raw| raw.get("type").and_then(Value::as_str).map(str::to_owned)).collect();
            self.debuggers.borrow_mut().retain(|d| !removed_types.contains(&Some(d.debug_type())));
        }

        self.on_did_debuggers_ext_point_read.fire(());
    }

    /// O handler de `breakpointsExtPoint`.
    pub fn handle_breakpoints_ext_point(&self, extensions: &[Vec<Value>]) {
        *self.breakpoint_contributions.borrow_mut() =
            extensions.iter().flatten().map(|breakpoint| Breakpoints::new(breakpoint.clone(), self.services.context_key_service.clone())).collect();
    }

    pub fn register_debug_adapter_factory(&self, debug_types: Vec<String>, debug_adapter_launcher: Rc<dyn IDebugAdapterFactory>) -> DisposableRef {
        for debug_type in &debug_types {
            self.debug_adapter_factories.borrow_mut().insert(debug_type.clone(), debug_adapter_launcher.clone());
        }
        self.debuggers_available.set(Dynamic::Bool(self.has_enabled_debuggers()));
        self.on_did_register_debugger.fire(());
        let this = self.this();
        to_disposable(move || {
            for debug_type in &debug_types {
                this.debug_adapter_factories.borrow_mut().shift_remove(debug_type);
            }
        })
    }

    pub fn has_enabled_debuggers(&self) -> bool {
        let types: Vec<String> = self.debug_adapter_factories.borrow().keys().cloned().collect();
        for debug_type in types {
            if let Some(dbg) = self.get_debugger(&debug_type)
                && dbg.enabled()
            {
                return true;
            }
        }
        false
    }

    pub fn create_debug_adapter(&self, session: &Rc<dyn IDebugSession>) -> Option<Rc<dyn IDebugAdapter>> {
        let debug_type = session.configuration().get("type").and_then(Value::as_str).unwrap_or_default().to_owned();
        let factory = self.debug_adapter_factories.borrow().get(&debug_type).cloned();
        factory.map(|factory| factory.create_debug_adapter(session.clone()))
    }

    pub fn substitute_variables(&self, debug_type: &str, folder: Option<WorkspaceFolder>, config: IConfig) -> Promise<IConfig> {
        let factory = self.debug_adapter_factories.borrow().get(debug_type).cloned();
        match factory {
            Some(factory) => factory.substitute_variables(folder, config),
            None => Promise::resolved(config),
        }
    }

    pub fn run_in_terminal(&self, debug_type: &str, args: Value, session_id: &str) -> Promise<Option<i64>> {
        let factory = self.debug_adapter_factories.borrow().get(debug_type).cloned();
        match factory {
            Some(factory) => factory.run_in_terminal(args, session_id),
            None => Promise::resolved(None),
        }
    }

    pub fn register_debug_adapter_descriptor_factory(&self, debug_adapter_provider: Rc<IDebugAdapterDescriptorFactory>) -> DisposableRef {
        self.adapter_descriptor_factories.borrow_mut().push(debug_adapter_provider.clone());
        let this = self.this();
        to_disposable(move || this.unregister_debug_adapter_descriptor_factory(&debug_adapter_provider))
    }

    pub fn unregister_debug_adapter_descriptor_factory(&self, debug_adapter_provider: &Rc<IDebugAdapterDescriptorFactory>) {
        let ix = self.adapter_descriptor_factories.borrow().iter().position(|f| Rc::ptr_eq(f, debug_adapter_provider));
        if let Some(ix) = ix {
            self.adapter_descriptor_factories.borrow_mut().remove(ix);
        }
    }

    pub fn get_debug_adapter_descriptor(&self, session: Rc<dyn IDebugSession>) -> Promise<Option<IAdapterDescriptor>> {
        let config = session.configuration();
        let debug_type = config.get("type").and_then(Value::as_str).unwrap_or_default().to_owned();
        let providers: Vec<Rc<IDebugAdapterDescriptorFactory>> = self.adapter_descriptor_factories.borrow().iter().filter(|p| p.r#type == debug_type).cloned().collect();
        if providers.len() == 1 {
            return (providers[0].create_debug_adapter_descriptor)(session).then(Some);
        }
        // TODO@AW handle n > 1 case
        Promise::resolved(None)
    }

    pub fn get_debugger_label(&self, debug_type: &str) -> Option<String> {
        self.get_debugger(debug_type).map(|dbgr| dbgr.label())
    }

    pub fn on_did_register_debugger(&self) -> Event<()> {
        self.on_did_register_debugger.event()
    }

    pub fn on_did_debuggers_ext_point_read(&self) -> Event<()> {
        self.on_did_debuggers_ext_point_read.event()
    }

    /// `canSetBreakpointsIn(model)`: `language_id` é o `model.getLanguageId()` (vazio quando não há).
    pub fn can_set_breakpoints_in(&self, language_id: &str) -> bool {
        if language_id.is_empty() || language_id == "jsonc" || language_id == "log" {
            // do not allow breakpoints in our settings files and output
            return false;
        }

        let debug = self.services.configuration_service.get_value(Some("debug"), None::<&ConfigurationOverrides>);
        if matches!(debug.get_str("allowBreakpointsEverywhere"), Dynamic::Bool(true)) {
            return true;
        }

        self.breakpoint_contributions.borrow().iter().any(|breakpoints| breakpoints.language() == language_id && breakpoints.enabled())
    }

    pub fn get_debugger(&self, debug_type: &str) -> Option<Rc<Debugger>> {
        self.debuggers.borrow().iter().find(|dbg| dbg.debug_type().to_lowercase() == debug_type.to_lowercase()).cloned()
    }

    pub fn get_enabled_debugger(&self, debug_type: &str) -> Option<Rc<Debugger>> {
        self.get_debugger(debug_type).filter(|adapter| adapter.enabled())
    }

    pub fn some_debugger_interested_in_language(&self, language_id: &str) -> bool {
        self.debuggers.borrow().iter().filter(|d| d.enabled()).any(|a| a.interested_in_language(language_id))
    }

    pub fn guess_debugger(self: &Rc<Self>, getting_configurations: bool) -> Promise<Option<GuessedDebugger>> {
        let this = self.clone();
        async_fn(async move {
            let active_code_editor = this.services.ui.active_code_editor_language_id();
            let mut candidates: Vec<Rc<Debugger>> = Vec::new();
            let mut language_label: Option<String> = None;
            let mut model_language_id: Option<String> = None;
            let mut has_model = false;

            if let Some(model_language) = active_code_editor {
                has_model = model_language.is_some();
                let language = model_language.clone().filter(|l| !l.is_empty());
                model_language_id = model_language;
                if let Some(language) = &language {
                    language_label = this.services.ui.get_language_name(language);
                }
                let adapters: Vec<Rc<Debugger>> = this
                    .debuggers
                    .borrow()
                    .iter()
                    .filter(|a| a.enabled())
                    .filter(|a| language.as_ref().is_some_and(|language| a.interested_in_language(language)))
                    .cloned()
                    .collect();
                if adapters.len() == 1 {
                    return Ok(Some(GuessedDebugger { debugger: Some(adapters[0].clone()), with_config: None }));
                }
                if adapters.len() > 1 {
                    candidates = adapters;
                }
            }

            // We want to get the debuggers that have configuration providers in the case we are fetching configurations
            // Or if a breakpoint can be set in the current file (good hint that an extension can handle it)
            let can_set_in_model = has_model && this.can_set_breakpoints_in(model_language_id.as_deref().unwrap_or_default());
            if (language_label.is_none() || getting_configurations || can_set_in_model) && candidates.is_empty() {
                this.activate_debuggers_impl("onDebugInitialConfigurations", None).await?;
                candidates = this
                    .debuggers
                    .borrow()
                    .iter()
                    .filter(|a| a.enabled())
                    .filter(|dbg| dbg.has_initial_configuration() || dbg.has_dynamic_configuration_providers() || dbg.has_configuration_provider())
                    .cloned()
                    .collect();
            }

            if candidates.is_empty()
                && let Some(label) = language_label.clone()
            {
                let label = if label.contains(' ') { format!("'{label}'") } else { label };
                let confirmed = this
                    .services
                    .ui
                    .confirm(
                        &localize("CouldNotFindLanguage", "You don't have an extension for debugging {0}. Should we find a {0} extension in the Marketplace?", &[&label]),
                        &localize("findExtension", "&&Find {0} extension", &[&label]),
                    )
                    .await?;
                if confirmed {
                    this.services.ui.execute_command("debug.installAdditionalDebuggers", Some(label)).await?;
                }
                return Ok(None);
            }

            this.init_extension_activations_if_needed();
            candidates.sort_by(|first, second| je_base::collator::locale_compare(&first.label(), &second.label()));
            candidates.retain(|a| !a.is_hidden_from_dropdown());

            let mut suggested_candidates: Vec<Rc<Debugger>> = Vec::new();
            let mut other_candidates: Vec<Rc<Debugger>> = Vec::new();
            for d in &candidates {
                let descriptor = d.get_main_extension_descriptor();
                let early = this.early_activated_extensions.borrow().as_ref().is_some_and(|set| set.contains(&descriptor.id));
                if !descriptor.id.is_empty() && early {
                    // Was activated early
                    suggested_candidates.push(d.clone());
                } else if this.used_debug_types.borrow().contains(&d.debug_type()) {
                    // Was used already
                    suggested_candidates.push(d.clone());
                } else {
                    other_candidates.push(d.clone());
                }
            }

            let mut picks: Vec<AdapterPick> = Vec::new();
            let dynamic = this.delegate.configuration_manager().get_dynamic_providers().await?;
            if !suggested_candidates.is_empty() {
                picks.push(AdapterPick::Separator { label: localize("suggestedDebuggers", "Suggested", &[]) });
                picks.extend(suggested_candidates.iter().map(|c| AdapterPick::Debugger { label: c.label(), debugger: c.clone() }));
            }

            if !other_candidates.is_empty() {
                if !picks.is_empty() {
                    picks.push(AdapterPick::Separator { label: String::new() });
                }
                picks.extend(other_candidates.iter().map(|c| AdapterPick::Debugger { label: c.label(), debugger: c.clone() }));
            }

            if !dynamic.is_empty() {
                if !picks.is_empty() {
                    picks.push(AdapterPick::Separator { label: String::new() });
                }

                for d in &dynamic {
                    picks.push(AdapterPick::DynamicOptions { label: localize("moreOptionsForDebugType", "More {0} options...", &[&d.label]), provider: d.clone() });
                }
            }

            picks.push(AdapterPick::Separator { label: String::new() });
            picks.push(AdapterPick::InstallExtension {
                label: match &language_label {
                    Some(label) => localize("installLanguage", "Install an extension for {0}...", &[label]),
                    None => localize("installExt", "Install extension...", &[]),
                },
            });

            for action in this.services.ui.contributed_create_configuration_actions() {
                picks.push(AdapterPick::Contributed(action));
            }

            let place_holder = localize("selectDebug", "Select debugger", &[]);
            let picked = this.services.ui.pick(&picks, &place_holder).await?;
            match picked.map(|index| picks[index].clone()) {
                Some(AdapterPick::Debugger { debugger, .. }) => Ok(Some(GuessedDebugger { debugger: Some(debugger), with_config: None })),
                Some(AdapterPick::DynamicOptions { provider, .. }) => {
                    let cfg: Option<DynamicPick> = provider.pick().await?;
                    match cfg {
                        None => Ok(None),
                        Some(cfg) => Ok(Some(GuessedDebugger { debugger: this.get_debugger(&provider.r#type), with_config: Some(cfg) })),
                    }
                }
                Some(AdapterPick::Contributed(action)) => {
                    (action.run)();
                    Ok(None)
                }
                Some(AdapterPick::InstallExtension { .. }) => {
                    this.services.ui.execute_command("debug.installAdditionalDebuggers", language_label.clone());
                    Ok(None)
                }
                Some(AdapterPick::Separator { .. }) | None => Ok(None),
            }
        })
    }

    fn init_extension_activations_if_needed(&self) {
        let mut early = self.early_activated_extensions.borrow_mut();
        if early.is_none() {
            let mut set = HashSet::new();
            for id in self.services.ui.extensions_with_activation_times() {
                set.insert(id);
            }
            *early = Some(set);
        }
    }

    fn activate_debuggers_impl(&self, activation_event: &str, debug_type: Option<&str>) -> Promise<()> {
        self.init_extension_activations_if_needed();
        let mut promises: Vec<Promise<()>> = vec![
            self.services.extension_service.activate_by_event(activation_event, None),
            self.services.extension_service.activate_by_event("onDebug", None),
        ];
        if let Some(debug_type) = debug_type.filter(|t| !t.is_empty()) {
            promises.push(self.services.extension_service.activate_by_event(&format!("{activation_event}:{debug_type}"), None));
        }
        Promise::all(promises).then(|_| ())
    }
}

impl IDisposable for AdapterManager {
    fn dispose(&self) {
        self.store.dispose();
    }
}

impl IManagerAdapterManager for AdapterManager {
    fn activate_debuggers(&self, activation_event: &str, debug_type: Option<&str>) -> Promise<()> {
        self.activate_debuggers_impl(activation_event, debug_type)
    }

    fn get_debugger_label(&self, debug_type: &str) -> Option<String> {
        AdapterManager::get_debugger_label(self, debug_type)
    }

    fn get_enabled_debugger(&self, debug_type: &str) -> Option<Rc<Debugger>> {
        AdapterManager::get_enabled_debugger(self, debug_type)
    }

    fn guess_debugger(&self, getting_configurations: bool) -> Promise<Option<GuessedDebugger>> {
        AdapterManager::guess_debugger(&self.this(), getting_configurations)
    }

    fn on_did_debuggers_ext_point_read(&self) -> Event<()> {
        AdapterManager::on_did_debuggers_ext_point_read(self)
    }
}

impl IDebuggerAdapterManager for AdapterManager {
    fn activate_debuggers(&self, activation_event: &str, debug_type: &str) -> Promise<()> {
        self.activate_debuggers_impl(activation_event, Some(debug_type))
    }

    fn create_debug_adapter(&self, session: Rc<dyn IDebugSessionFull>) -> Option<Rc<dyn IDebugAdapter>> {
        let session: Rc<dyn IDebugSession> = session;
        AdapterManager::create_debug_adapter(self, &session)
    }

    fn substitute_variables(&self, debug_type: &str, folder: Option<WorkspaceFolder>, config: IConfig) -> Promise<IConfig> {
        AdapterManager::substitute_variables(self, debug_type, folder, config)
    }

    fn run_in_terminal(&self, debug_type: &str, args: Value, session_id: &str) -> Promise<Option<i64>> {
        AdapterManager::run_in_terminal(self, debug_type, args, session_id)
    }
}

impl IAdapterManager for AdapterManager {
    fn register_debug_adapter_factory(&self, debug_types: Vec<String>, debug_adapter_launcher: Rc<dyn IDebugAdapterFactory>) -> DisposableRef {
        AdapterManager::register_debug_adapter_factory(self, debug_types, debug_adapter_launcher)
    }

    fn register_debug_adapter_descriptor_factory(&self, factory: Rc<IDebugAdapterDescriptorFactory>) -> DisposableRef {
        AdapterManager::register_debug_adapter_descriptor_factory(self, factory)
    }

    fn unregister_debug_adapter_descriptor_factory(&self, factory: &Rc<IDebugAdapterDescriptorFactory>) {
        AdapterManager::unregister_debug_adapter_descriptor_factory(self, factory);
    }
}
