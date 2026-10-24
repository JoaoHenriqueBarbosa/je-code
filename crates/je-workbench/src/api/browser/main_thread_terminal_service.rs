//! Porte de `src/vs/workbench/api/browser/mainThreadTerminalService.ts`: a ponte entre o `ITerminalService` do workbench e os
//! terminais que as extensões criam, controlam e observam (`MainThreadTerminalService`), mais o rastreador de dados
//! (`TerminalDataEventTracker`), o provedor de links da extensão (`ExtensionTerminalLinkProvider`), `getOutputMatchForLines` e
//! `parseQuickFix`.
//!
//! Divergências:
//! - `@extHostNamedCustomer(MainContext.MainThreadTerminalService)` e o `IExtHostContext` do construtor: o registro de clientes
//!   não existe ainda; o construtor recebe o [`IMainContext`] e os serviços de [`MainThreadTerminalServiceServices`] (os
//!   parâmetros decorados do original), e pede o proxy de `ExtHostTerminalService`. O `IInstantiationService` que cria o
//!   `TerminalDataEventTracker` não existe aqui: o rastreador é construído direto com o serviço de terminais.
//! - **Métodos `async` da forma.** `$createTerminal`, `$show`, `$hide`, `$dispose`, `$sendText` e `$registerQuickFixProvider` são
//!   `async` no original e os testes esperam o resultado; aqui são métodos próprios que devolvem [`Promise`], e a implementação de
//!   [`MainThreadTerminalServiceShape`] (cuja forma é `void`) os repassa e descarta a promessa, como o proxy de RPC.
//! - **`_store`.** O `Disposable` do original é um `DisposableStore` próprio (`store`), porque o `$createTerminal` remove dele o
//!   ouvinte de descarte (`this._store.delete`).
//! - **`onAnyInstanceSelectionChange`.** Fica de fora (e com ele `$acceptTerminalSelection`): o contrato do emulador do porte não tem
//!   seleção (ver `contrib/terminal/browser/terminal.rs`).
//! - **Provedores.** `ITerminalCompletionProvider`, `ITerminalProfileProvider` e `ITerminalQuickFixProvider` são structs que
//!   guardam o proxy e repassam a chamada. O `provider: 'ext:${id}'` do item de completion é o `provider` do
//!   [`ITerminalCompletion`]; o `isFile`/`isDirectory` do DTO (que o original espalha no objeto sem uso) não tem campo. O
//!   `resourceOptions` sem `cwd` (que o original passaria como `undefined` e quebraria depois) vira uma lista só de itens.
//!   O `globPattern` não existe no tipo da API portado.
//! - **`$setEnvironmentVariableCollection`.** O `persistent` da coleção traduzida não tem onde ficar: o `IEnvironmentVariableService`
//!   do porte guarda só `map` e `descriptionMap` (ver `common/environment_variable.rs`).
//! - **`options.outputMatcher.length`.** O `ITerminalQuickFixProvider` recebe as opções por referência mutável (ver
//!   `quick_fix.rs`).
//! - **`String#match`.** `terminalCommand.command.match(commandLineMatcher)` e `lines.join('\n').match(lineMatcher)` são o
//!   `match_line` do `je-platform` (sem a flag `g`, que o original trataria como `matchAll`).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use je_base::JsString;
use je_base::arrays::OneOrMany;
use je_base::async_::{async_fn, promises};
use je_base::cancellation::CancellationToken;
use je_base::event::Event;
use je_base::lifecycle::{DisposableMap, DisposableRef, DisposableStore, IDisposable, MutableDisposable, combined_disposable};
use je_base::platform::{OperatingSystem, Platform};
use je_base::promise::Promise;
use je_platform::log::LogServiceRef;
use je_platform::terminal::capabilities::capabilities::{CommandDetection, IBaseTerminalCommand, ITerminalCommand};
use je_platform::terminal::capabilities::command_detection::terminal_command::match_line;
use je_platform::terminal::environment_variable::{IEnvironmentVariableCollection, ISerializableEnvironmentDescriptionMap, ISerializableEnvironmentVariableCollection};
use je_platform::terminal::environment_variable_shared::{
    deserialize_environment_description_map, deserialize_environment_variable_collection, serialize_environment_variable_collection,
};
use je_platform::terminal::terminal::{
    IProcessProperty, IProcessReadyWindowsPty, IShellLaunchConfig, InitialText, ITerminalOutputMatch, ITerminalOutputMatcher, ShellCwd, TerminalExitReason,
    TerminalLocation, WaitOnExitValue,
};
use je_platform::terminal::terminal_data_buffering::TerminalDataBufferer;

use crate::api::common::ext_host_protocol::{
    ExtHostContext, ExtHostTerminalIdentifier, ExtHostTerminalServiceShape, IMainContext, ICreateContributedTerminalProfileOptions, ITerminalCommandDto,
    ITerminalCompletionContextDto, ITerminalCompletionItemDto, ITerminalDimensionsDto, MainThreadTerminalServiceShape, TerminalCommandMatchResultDto, TerminalLaunchConfig,
    TerminalLaunchLocation, TerminalQuickFix,
};
use crate::contrib::terminal::browser::terminal::{
    ICreateTerminalOptions, ITerminalEditorService, ITerminalExternalLinkProvider, ITerminalGroupService, ITerminalInstance, ITerminalLink, ITerminalService,
    ShellLaunchConfigOrProfile, TerminalCreateLocation,
};
use crate::contrib::terminal::browser::terminal_events::ITerminalServiceEvents;
use crate::contrib::terminal::browser::terminal_process_ext_host_proxy::TerminalProcessExtHostProxy;
use crate::contrib::terminal::common::environment_variable::IEnvironmentVariableService;
use crate::contrib::terminal::common::terminal::{
    IShellLaunchConfigResolveOptions, IStartExtensionTerminalRequest, ITerminalProcessExtHostProxy, ITerminalProfileProvider, ITerminalProfileResolverService,
    ITerminalProfileService,
};
use crate::contrib::terminal_contrib::links::links::ITerminalLinkProviderService;
use crate::contrib::terminal_contrib::quick_fix::quick_fix::{
    ITerminalQuickFix, ITerminalQuickFixOptions, ITerminalQuickFixProvider, ITerminalQuickFixService, TerminalQuickFixAction,
};
use crate::contrib::terminal_contrib::suggest::terminal_completion_item::{ITerminalCompletion, TerminalCompletionItemKind};
use crate::contrib::terminal_contrib::suggest::terminal_completion_service::{
    ITerminalCompletionProvider, ITerminalCompletionService, TerminalCompletionList, TerminalCompletionProviderResult, TerminalCompletionResourceOptions,
};
use crate::services::environment::environment_service::IWorkbenchEnvironmentService;
use crate::services::remote::remote_agent_service::IRemoteAgentService;
use crate::services::suggest::simple_completion_item::SimpleCompletionLabel;

/// `TerminalProcessProxyEntry`: o proxy do processo e o que o descarta.
struct TerminalProcessProxyEntry {
    proxy: Rc<dyn ITerminalProcessExtHostProxy>,
    store: Rc<DisposableStore>,
}

impl IDisposable for TerminalProcessProxyEntry {
    fn dispose(&self) {
        self.store.dispose();
    }
}

/// Os serviços que o construtor do original recebe decorados (`@ITerminalService` e os demais).
pub struct MainThreadTerminalServiceServices {
    pub terminal_service: Rc<dyn ITerminalService>,
    pub terminal_link_provider_service: Rc<dyn ITerminalLinkProviderService>,
    pub terminal_quick_fix_service: Rc<dyn ITerminalQuickFixService>,
    pub environment_variable_service: Rc<dyn IEnvironmentVariableService>,
    pub log_service: LogServiceRef,
    pub terminal_profile_resolver_service: Rc<dyn ITerminalProfileResolverService>,
    pub remote_agent_service: Rc<dyn IRemoteAgentService>,
    pub terminal_group_service: Rc<dyn ITerminalGroupService>,
    pub terminal_editor_service: Rc<dyn ITerminalEditorService>,
    pub terminal_profile_service: Rc<dyn ITerminalProfileService>,
    pub terminal_completion_service: Rc<dyn ITerminalCompletionService>,
    pub environment_service: Rc<dyn IWorkbenchEnvironmentService>,
}

/// `MainThreadTerminalService`.
pub struct MainThreadTerminalService {
    this: Weak<Self>,
    store: Rc<DisposableStore>,
    proxy: Rc<dyn ExtHostTerminalServiceShape>,

    /// Stores a map from a temporary terminal id (a UUID generated on the extension host side) to a numeric terminal id (an id
    /// generated on the renderer side). This comes in play only when dealing with terminals created on the extension host side.
    ext_host_terminals: Rc<RefCell<HashMap<String, Promise<Rc<dyn ITerminalInstance>>>>>,
    terminal_process_proxies: Rc<DisposableMap<u32, TerminalProcessProxyEntry>>,
    profile_providers: Rc<DisposableMap<String, dyn IDisposable>>,
    completion_providers: Rc<DisposableMap<String, dyn IDisposable>>,
    quick_fix_providers: Rc<DisposableMap<String, dyn IDisposable>>,
    data_event_tracker: Rc<MutableDisposable<TerminalDataEventTracker>>,
    send_command_event_listener: Rc<MutableDisposable<dyn IDisposable>>,

    /// A single shared terminal link provider for the exthost. When an ext registers a link provider, this is registered with the
    /// terminal on the renderer side and all links are provided through this, even from multiple ext link providers. Xterm should
    /// remove lower priority intersecting links itself.
    link_provider: Rc<MutableDisposable<dyn IDisposable>>,

    os: Cell<OperatingSystem>,
    services: MainThreadTerminalServiceServices,
}

impl MainThreadTerminalService {
    pub fn new(ext_host_context: &dyn IMainContext, services: MainThreadTerminalServiceServices) -> Rc<Self> {
        let store = DisposableStore::new();
        let service = Rc::new_cyclic(|this| Self {
            this: this.clone(),
            proxy: ext_host_context.get_proxy(&ExtHostContext::EXT_HOST_TERMINAL_SERVICE),
            ext_host_terminals: Rc::default(),
            terminal_process_proxies: store.add_rc(DisposableMap::new()),
            profile_providers: store.add_rc(DisposableMap::new()),
            completion_providers: store.add_rc(DisposableMap::new()),
            quick_fix_providers: store.add_rc(DisposableMap::new()),
            data_event_tracker: store.add_rc(MutableDisposable::new()),
            send_command_event_listener: store.add_rc(MutableDisposable::new()),
            link_provider: store.add_rc(MutableDisposable::new()),
            os: Cell::new(Platform::detect().os),
            store,
            services,
        });

        let terminal_service = service.services.terminal_service.clone();

        // ITerminalService listeners
        service.listen(terminal_service.on_did_create_instance(), |this, instance| {
            this.on_terminal_opened(instance);
            this.on_instance_dimensions_changed(instance);
        });
        service.listen(terminal_service.on_did_dispose_instance(), |this, instance| this.on_terminal_disposed(instance));
        service.listen(terminal_service.on_any_instance_process_id_ready(), |this, instance| this.on_terminal_process_id_ready(instance));
        service.listen(terminal_service.on_did_change_instance_dimensions(), |this, instance| this.on_instance_dimensions_changed(instance));
        service.listen(terminal_service.on_any_instance_maximum_dimensions_change(), |this, instance| this.on_instance_maximum_dimensions_changed(instance));
        service.listen(terminal_service.on_did_request_start_extension_terminal(), |this, request| this.on_request_start_extension_terminal(request));
        service.listen(terminal_service.on_did_change_active_instance(), |this, instance| {
            this.on_active_terminal_changed(instance.as_ref().map(|instance| instance.instance_id()));
        });
        service.listen(terminal_service.on_any_instance_title_change(), |this, instance| this.on_title_changed(instance.instance_id(), &instance.title()));
        service.listen(terminal_service.on_any_instance_data_input(), |this, instance| this.proxy.rpc_accept_terminal_interaction(instance.instance_id()));
        service.listen(terminal_service.on_any_instance_shell_type_changed(), |this, instance| this.on_shell_type_changed(instance.instance_id()));

        // Set initial ext host state
        for instance in terminal_service.instances() {
            service.on_terminal_opened(&instance);
            let (this, ready_instance) = (service.this.clone(), instance.clone());
            drop(instance.process_ready().then(move |()| {
                if let Some(this) = this.upgrade() {
                    this.on_terminal_process_id_ready(&ready_instance);
                }
            }));
            if let Some(shell_type) = instance.shell_type() {
                service.proxy.rpc_accept_terminal_shell_type(instance.instance_id(), Some(shell_type));
            }
        }
        if let Some(active_instance) = terminal_service.active_instance() {
            drop(service.proxy.rpc_accept_active_terminal_changed(Some(active_instance.instance_id())));
        }
        let collections = service.services.environment_variable_service.collections();
        if collections.len() > 0 {
            let serialized_collections: Vec<(String, ISerializableEnvironmentVariableCollection)> =
                collections.iter().map(|(extension, collection)| (extension.clone(), serialize_environment_variable_collection(&collection.map))).collect();
            service.proxy.rpc_init_environment_variable_collections(serialized_collections);
        }
        {
            let this = service.this.clone();
            drop(service.services.remote_agent_service.get_environment().then(move |env| {
                if let Some(this) = this.upgrade() {
                    this.os.set(env.map_or_else(|| Platform::detect().os, |env| env.os));
                    drop(this.update_default_profile());
                }
            }));
        }
        service.listen(service.services.terminal_profile_service.on_did_change_available_profiles(), |this, _| {
            drop(this.update_default_profile());
        });

        service
    }

    /// `this._register(event(handler))`, com o serviço seguro por uma referência fraca.
    fn listen<T: 'static>(&self, event: Event<T>, handler: impl Fn(&Rc<Self>, &T) + 'static) {
        let this = self.this.clone();
        self.store.add(event.listen(move |value| {
            if let Some(this) = this.upgrade() {
                handler(&this, value);
            }
        }));
    }

    /// `_updateDefaultProfile()`.
    fn update_default_profile(&self) -> Promise<()> {
        let remote_authority = self.services.environment_service.remote_authority();
        let os = self.os.get();
        let resolver = &self.services.terminal_profile_resolver_service;
        let default_profile = resolver.get_default_profile(IShellLaunchConfigResolveOptions {
            remote_authority: remote_authority.as_deref(),
            os,
            allow_automation_shell: None,
            allow_agent_host_shell: None,
        });
        let default_automation_profile = resolver.get_default_profile(IShellLaunchConfigResolveOptions {
            remote_authority: remote_authority.as_deref(),
            os,
            allow_automation_shell: Some(true),
            allow_agent_host_shell: None,
        });
        let proxy = self.proxy.clone();
        Promise::all(vec![default_profile, default_automation_profile]).then(move |profiles| {
            proxy.rpc_accept_default_profile(profiles[0].clone(), profiles[1].clone());
        })
    }

    /// `_getTerminalInstance(id)`.
    fn get_terminal_instance(&self, id: &ExtHostTerminalIdentifier) -> Promise<Option<Rc<dyn ITerminalInstance>>> {
        match id {
            ExtHostTerminalIdentifier::Uuid(uuid) => {
                let terminal = self.ext_host_terminals.borrow().get(uuid).cloned();
                match terminal {
                    Some(terminal) => terminal.then(Some),
                    None => Promise::resolved(None),
                }
            }
            ExtHostTerminalIdentifier::Id(id) => Promise::resolved(self.services.terminal_service.get_instance_from_id(*id)),
        }
    }

    /// `$createTerminal(extHostTerminalId, launchConfig)`.
    pub fn create_terminal(&self, ext_host_terminal_id: &str, launch_config: TerminalLaunchConfig) -> Promise<()> {
        let this = self.this.upgrade().expect("o serviço está vivo enquanto um método dele roda");
        let ext_host_terminal_id = ext_host_terminal_id.to_owned();

        let custom_pty_implementation = launch_config.is_extension_custom_pty_terminal.unwrap_or(false).then(|| {
            let terminal_service = this.services.terminal_service.clone();
            let create: je_platform::terminal::terminal::CustomPtyImplementation = Rc::new(move |id, cols, rows| {
                TerminalProcessExtHostProxy::new(id, cols, rows, terminal_service.clone()) as Rc<dyn je_platform::terminal::terminal::ITerminalChildProcess>
            });
            create
        });
        let shell_launch_config = IShellLaunchConfig {
            name: launch_config.name.clone(),
            executable: launch_config.shell_path.clone(),
            args: launch_config.shell_args.clone(),
            cwd: launch_config.cwd.clone(),
            icon: launch_config.icon.clone(),
            color: launch_config.color.clone(),
            initial_text: launch_config.initial_text.clone().map(InitialText::Text),
            wait_on_exit: launch_config.wait_on_exit.map(WaitOnExitValue::Bool),
            ignore_configuration_cwd: Some(true),
            env: launch_config.env.clone(),
            strict_env: launch_config.strict_env,
            hide_from_user: launch_config.hide_from_user,
            custom_pty_implementation,
            ext_host_terminal_id: Some(ext_host_terminal_id.clone()),
            force_shell_integration: launch_config.force_shell_integration,
            is_feature_terminal: launch_config.is_feature_terminal,
            remote_resolver_terminal: launch_config.is_remote_resolver_terminal.unwrap_or(false),
            is_extension_owned_terminal: launch_config.is_extension_owned_terminal,
            use_shell_environment: launch_config.use_shell_environment,
            is_transient: launch_config.is_transient,
            shell_integration_nonce: launch_config.shell_integration_nonce.clone(),
            title_template: launch_config.title_template.clone(),
            ..IShellLaunchConfig::default()
        };

        let terminal: Promise<Rc<dyn ITerminalInstance>> = {
            let this = this.clone();
            let location = launch_config.location.clone();
            let is_remote_resolver_terminal = launch_config.is_remote_resolver_terminal.unwrap_or(false);
            promises::with_async_body(move |resolver| {
                async_fn(async move {
                    let cwd = if is_remote_resolver_terminal { shell_launch_config.cwd.clone() } else { None };
                    let location = this.deserialize_parent_terminal(location).await?;
                    let terminal = this
                        .services
                        .terminal_service
                        .create_terminal(Some(ICreateTerminalOptions {
                            config: Some(ShellLaunchConfigOrProfile::LaunchConfig(shell_launch_config)),
                            cwd,
                            location,
                            ..ICreateTerminalOptions::default()
                        }))
                        .await?;
                    resolver.resolve(terminal);
                    Ok(())
                })
            })
        };
        self.ext_host_terminals.borrow_mut().insert(ext_host_terminal_id.clone(), terminal.clone());

        async_fn(async move {
            let terminal_instance = terminal.await?;
            if terminal_instance.is_disposed() {
                this.ext_host_terminals.borrow_mut().remove(&ext_host_terminal_id);
                return Ok(());
            }
            let dispose_listener: Rc<RefCell<Option<DisposableRef>>> = Rc::default();
            let listener = terminal_instance.on_disposed().listen({
                let (weak, dispose_listener, ext_host_terminal_id) = (this.this.clone(), dispose_listener.clone(), ext_host_terminal_id);
                move |_| {
                    let Some(this) = weak.upgrade() else { return };
                    this.ext_host_terminals.borrow_mut().remove(&ext_host_terminal_id);
                    let listener = dispose_listener.borrow_mut().take();
                    if let Some(listener) = listener {
                        this.store.delete(&listener);
                    }
                }
            });
            *dispose_listener.borrow_mut() = Some(this.store.add(listener));
            Ok(())
        })
    }

    /// `_deserializeParentTerminal(location)`.
    fn deserialize_parent_terminal(&self, location: Option<TerminalLaunchLocation>) -> Promise<Option<TerminalCreateLocation>> {
        let this = self.this.upgrade().expect("o serviço está vivo enquanto um método dele roda");
        async_fn(async move {
            Ok(match location {
                Some(TerminalLaunchLocation::ParentTerminal(parent)) => {
                    let parent_terminal = this.ext_host_terminals.borrow().get(&parent.as_text()).cloned();
                    match parent_terminal {
                        Some(parent_terminal) => Some(TerminalCreateLocation::ParentTerminal(parent_terminal.await?)),
                        None => None,
                    }
                }
                Some(TerminalLaunchLocation::Location(location)) => Some(TerminalCreateLocation::Location(location)),
                Some(TerminalLaunchLocation::ViewColumn { view_column, preserve_focus, .. }) => Some(TerminalCreateLocation::Editor { view_column, preserve_focus }),
                Some(TerminalLaunchLocation::SplitActiveTerminal(split)) => Some(TerminalCreateLocation::SplitActiveTerminal(split)),
                None => None,
            })
        })
    }

    /// `$show(id, preserveFocus)`.
    pub fn show(&self, id: &ExtHostTerminalIdentifier, preserve_focus: bool) -> Promise<()> {
        let this = self.this.upgrade().expect("o serviço está vivo enquanto um método dele roda");
        let terminal_instance = self.get_terminal_instance(id);
        async_fn(async move {
            if let Some(terminal_instance) = terminal_instance.await? {
                this.services.terminal_service.set_active_instance(terminal_instance.clone());
                if terminal_instance.target() == Some(TerminalLocation::Editor) {
                    this.services.terminal_editor_service.reveal_active_editor(preserve_focus).await?;
                } else {
                    this.services.terminal_group_service.show_panel(!preserve_focus).await?;
                }
            }
            Ok(())
        })
    }

    /// `$hide(id)`.
    pub fn hide(&self, id: &ExtHostTerminalIdentifier) -> Promise<()> {
        let this = self.this.upgrade().expect("o serviço está vivo enquanto um método dele roda");
        let instance_to_hide = self.get_terminal_instance(id);
        async_fn(async move {
            let instance_to_hide = instance_to_hide.await?;
            let active_instance = this.services.terminal_service.active_instance();
            if let Some(active_instance) = active_instance {
                if Some(active_instance.instance_id()) == instance_to_hide.map(|instance| instance.instance_id()) && active_instance.target() != Some(TerminalLocation::Editor) {
                    this.services.terminal_group_service.hide_panel();
                }
            }
            Ok(())
        })
    }

    /// `$dispose(id)`.
    pub fn dispose_terminal(&self, id: &ExtHostTerminalIdentifier) -> Promise<()> {
        let instance = self.get_terminal_instance(id);
        async_fn(async move {
            if let Some(instance) = instance.await? {
                instance.dispose_with_reason(Some(TerminalExitReason::Extension));
            }
            Ok(())
        })
    }

    /// `$sendText(id, text, shouldExecute)`.
    pub fn send_text(&self, id: &ExtHostTerminalIdentifier, text: &str, should_execute: bool) -> Promise<()> {
        let instance = self.get_terminal_instance(id);
        let text = text.to_owned();
        async_fn(async move {
            if let Some(instance) = instance.await? {
                instance.send_text(&text, should_execute).await?;
            }
            Ok(())
        })
    }

    /// `$registerQuickFixProvider(id, extensionId)`.
    pub fn register_quick_fix_provider(&self, id: &str, extension_id: &str) -> Promise<()> {
        let provider = Rc::new(ExtensionQuickFixProvider {
            proxy: self.proxy.clone(),
            log_service: self.services.log_service.clone(),
            id: id.to_owned(),
            extension_id: extension_id.to_owned(),
        });
        self.quick_fix_providers.set(id.to_owned(), self.services.terminal_quick_fix_service.register_quick_fix_provider(id, provider), false);
        Promise::resolved(())
    }

    fn on_active_terminal_changed(&self, terminal_id: Option<u32>) {
        drop(self.proxy.rpc_accept_active_terminal_changed(terminal_id));
    }

    fn on_terminal_data(&self, terminal_id: u32, data: &str) {
        drop(self.proxy.rpc_accept_terminal_process_data(terminal_id, data));
    }

    fn on_did_execute_command(&self, terminal_id: u32, command: ITerminalCommandDto) {
        drop(self.proxy.rpc_accept_did_execute_command(terminal_id, command));
    }

    fn on_title_changed(&self, terminal_id: u32, name: &str) {
        drop(self.proxy.rpc_accept_terminal_title_change(terminal_id, name));
    }

    fn on_shell_type_changed(&self, terminal_id: u32) {
        if let Some(terminal_instance) = self.services.terminal_service.get_instance_from_id(terminal_id) {
            self.proxy.rpc_accept_terminal_shell_type(terminal_id, terminal_instance.shell_type());
        }
    }

    fn on_terminal_disposed(&self, terminal_instance: &Rc<dyn ITerminalInstance>) {
        drop(self.proxy.rpc_accept_terminal_closed(
            terminal_instance.instance_id(),
            terminal_instance.exit_code(),
            terminal_instance.exit_reason().unwrap_or(TerminalExitReason::Unknown),
        ));
        self.terminal_process_proxies.delete_and_dispose(&terminal_instance.instance_id());
    }

    fn on_terminal_opened(&self, terminal_instance: &Rc<dyn ITerminalInstance>) {
        let shell_launch_config = terminal_instance.shell_launch_config().borrow().clone();
        let ext_host_terminal_id = shell_launch_config.ext_host_terminal_id.clone();
        let shell_launch_config_dto = crate::api::common::ext_host_protocol::IShellLaunchConfigDto {
            name: shell_launch_config.name.clone(),
            executable: shell_launch_config.executable.clone(),
            args: shell_launch_config.args.clone(),
            cwd: shell_launch_config.cwd.clone(),
            env: shell_launch_config.env.clone(),
            hide_from_user: shell_launch_config.hide_from_user,
            tab_actions: shell_launch_config.tab_actions.clone(),
            title_template: shell_launch_config.title_template.clone(),
            ..Default::default()
        };
        self.proxy.rpc_accept_terminal_opened(terminal_instance.instance_id(), ext_host_terminal_id.as_deref(), &terminal_instance.title(), shell_launch_config_dto);
    }

    fn on_terminal_process_id_ready(&self, terminal_instance: &Rc<dyn ITerminalInstance>) {
        let Some(process_id) = terminal_instance.process_id() else {
            return;
        };
        drop(self.proxy.rpc_accept_terminal_process_id(terminal_instance.instance_id(), process_id));
    }

    fn on_instance_dimensions_changed(&self, instance: &Rc<dyn ITerminalInstance>) {
        drop(self.proxy.rpc_accept_terminal_dimensions(instance.instance_id(), instance.cols(), instance.rows()));
    }

    fn on_instance_maximum_dimensions_changed(&self, instance: &Rc<dyn ITerminalInstance>) {
        drop(self.proxy.rpc_accept_terminal_maximum_dimensions(instance.instance_id(), instance.max_cols(), instance.max_rows()));
    }

    fn on_request_start_extension_terminal(&self, request: &IStartExtensionTerminalRequest) {
        let proxy = request.proxy.clone();
        let store = DisposableStore::new();
        store.add(proxy.clone());
        let instance_id = proxy.instance_id();
        self.terminal_process_proxies.set(instance_id, Rc::new(TerminalProcessProxyEntry { proxy: proxy.clone(), store: store.clone() }), false);

        // Note that onResize is not being listened to here as it needs to fire when max dimensions
        // change, excluding the dimension override
        let initial_dimensions = (request.cols != 0 && request.rows != 0).then_some(ITerminalDimensionsDto { columns: request.cols, rows: request.rows });

        let callback = request.callback.clone();
        drop(self.proxy.rpc_start_extension_terminal(instance_id, initial_dimensions).then(move |error| callback(error)));

        let ext_host = self.proxy.clone();
        store.add(proxy.on_input().listen({
            let ext_host = ext_host.clone();
            move |data| ext_host.rpc_accept_process_input(instance_id, data)
        }));
        store.add(proxy.on_shutdown().listen({
            let ext_host = ext_host.clone();
            move |immediate| ext_host.rpc_accept_process_shutdown(instance_id, *immediate)
        }));
        store.add(proxy.on_request_cwd().listen({
            let ext_host = ext_host.clone();
            move |()| ext_host.rpc_accept_process_request_cwd(instance_id)
        }));
        store.add(proxy.on_request_initial_cwd().listen(move |()| ext_host.rpc_accept_process_request_initial_cwd(instance_id)));
    }
}

impl IDisposable for MainThreadTerminalService {
    fn dispose(&self) {
        self.store.dispose();
    }
}

impl MainThreadTerminalServiceShape for MainThreadTerminalService {
    fn rpc_create_terminal(&self, ext_host_terminal_id: &str, config: TerminalLaunchConfig) -> Promise<()> {
        self.create_terminal(ext_host_terminal_id, config)
    }

    fn rpc_dispose(&self, id: &ExtHostTerminalIdentifier) {
        drop(self.dispose_terminal(id));
    }

    fn rpc_hide(&self, id: &ExtHostTerminalIdentifier) {
        drop(self.hide(id));
    }

    fn rpc_send_text(&self, id: &ExtHostTerminalIdentifier, text: &str, should_execute: bool) {
        drop(self.send_text(id, text, should_execute));
    }

    fn rpc_show(&self, id: &ExtHostTerminalIdentifier, preserve_focus: bool) {
        drop(self.show(id, preserve_focus));
    }

    fn rpc_register_process_support(&self, is_supported: bool) {
        self.services.terminal_service.register_process_support(is_supported);
    }

    fn rpc_register_profile_provider(&self, id: &str, extension_identifier: &str) {
        // Proxy profile provider requests through the extension host
        let provider = Rc::new(ExtensionProfileProvider { proxy: self.proxy.clone(), id: id.to_owned() });
        self.profile_providers.set(id.to_owned(), self.services.terminal_profile_service.register_terminal_profile_provider(extension_identifier, id, provider), false);
    }

    fn rpc_unregister_profile_provider(&self, id: &str) {
        self.profile_providers.delete_and_dispose(&id.to_owned());
    }

    fn rpc_register_completion_provider(&self, id: &str, extension_identifier: &str, trigger_characters: &[String]) {
        let provider = Rc::new(ExtensionCompletionProvider {
            proxy: self.proxy.clone(),
            id: RefCell::new(id.to_owned()),
            remote_id: id.to_owned(),
            trigger_characters: RefCell::new(Vec::new()),
        });
        self.completion_providers.set(
            id.to_owned(),
            self.services.terminal_completion_service.register_terminal_completion_provider(extension_identifier, id, provider, trigger_characters),
            false,
        );
    }

    fn rpc_unregister_completion_provider(&self, id: &str) {
        self.completion_providers.delete_and_dispose(&id.to_owned());
    }

    fn rpc_register_quick_fix_provider(&self, id: &str, extension_identifier: &str) {
        drop(self.register_quick_fix_provider(id, extension_identifier));
    }

    fn rpc_unregister_quick_fix_provider(&self, id: &str) {
        self.quick_fix_providers.delete_and_dispose(&id.to_owned());
    }

    fn rpc_set_environment_variable_collection(
        &self,
        extension_identifier: &str,
        _persistent: bool,
        collection: Option<ISerializableEnvironmentVariableCollection>,
        description_map: ISerializableEnvironmentDescriptionMap,
    ) {
        match collection {
            Some(collection) => {
                let translated_collection = IEnvironmentVariableCollection {
                    map: deserialize_environment_variable_collection(collection),
                    description_map: Some(deserialize_environment_description_map(Some(description_map))),
                };
                self.services.environment_variable_service.set(extension_identifier, translated_collection);
            }
            None => self.services.environment_variable_service.delete(extension_identifier),
        }
    }

    fn rpc_start_sending_data_events(&self) {
        if self.data_event_tracker.value().is_none() {
            let weak = self.this.clone();
            let tracker = TerminalDataEventTracker::new(
                Rc::new(move |id, data: &str| {
                    if let Some(this) = weak.upgrade() {
                        this.on_terminal_data(id, data);
                    }
                }),
                self.services.terminal_service.clone(),
            );
            self.data_event_tracker.set(Some(tracker));
            // Send initial events if they exist
            for instance in self.services.terminal_service.instances() {
                for data in instance.initial_data_events().unwrap_or_default() {
                    self.on_terminal_data(instance.instance_id(), &data);
                }
            }
        }
    }

    fn rpc_stop_sending_data_events(&self) {
        self.data_event_tracker.clear();
    }

    fn rpc_start_sending_command_events(&self) {
        if self.send_command_event_listener.value().is_some() {
            return;
        }

        let multiplexer = self.services.terminal_service.create_on_instance_capability_event::<CommandDetection, _>(|capability| capability.on_command_finished());
        let weak = self.this.clone();
        let sub = multiplexer.event.listen(move |e| {
            let Some(this) = weak.upgrade() else { return };
            let command: &Rc<dyn ITerminalCommand> = &e.data;
            this.on_did_execute_command(
                e.instance.instance_id(),
                ITerminalCommandDto {
                    command_line: Some(command.command()),
                    // Convert to URI if possible
                    cwd: command.cwd().map(ShellCwd::Path),
                    exit_code: command.exit_code().map(|exit_code| exit_code as i32),
                    output: command.get_output(),
                },
            );
        });
        self.send_command_event_listener.set(Some(combined_disposable(vec![Rc::new(multiplexer) as DisposableRef, sub])));
    }

    fn rpc_stop_sending_command_events(&self) {
        self.send_command_event_listener.clear();
    }

    fn rpc_start_link_provider(&self) {
        self.link_provider
            .set(Some(self.services.terminal_link_provider_service.register_link_provider(Rc::new(ExtensionTerminalLinkProvider { proxy: self.proxy.clone() }))));
    }

    fn rpc_stop_link_provider(&self) {
        self.link_provider.clear();
    }

    fn rpc_send_process_data(&self, terminal_id: u32, data: &str) {
        if let Some(entry) = self.terminal_process_proxies.get(&terminal_id) {
            entry.proxy.emit_data(data);
        }
    }

    fn rpc_send_process_ready(&self, terminal_id: u32, pid: i32, cwd: &str, windows_pty: Option<IProcessReadyWindowsPty>) {
        if let Some(entry) = self.terminal_process_proxies.get(&terminal_id) {
            entry.proxy.emit_ready(pid, cwd, windows_pty);
        }
    }

    fn rpc_send_process_property(&self, terminal_id: u32, property: IProcessProperty) {
        if let IProcessProperty::Title(title) = &property {
            if let Some(instance) = self.services.terminal_service.get_instance_from_id(terminal_id) {
                instance.rename(Some(title));
            }
        }
        if let Some(entry) = self.terminal_process_proxies.get(&terminal_id) {
            entry.proxy.emit_process_property(property);
        }
    }

    fn rpc_send_process_exit(&self, terminal_id: u32, exit_code: Option<i32>) {
        if let Some(entry) = self.terminal_process_proxies.get(&terminal_id) {
            entry.proxy.emit_exit(exit_code);
        }
    }
}

/// `TerminalDataEventTracker`: encapsulates temporary tracking of data events from terminal instances, once disposed all listeners
/// are removed.
pub struct TerminalDataEventTracker {
    store: Rc<DisposableStore>,
    bufferer: Rc<TerminalDataBufferer>,
    instance_listeners: Rc<DisposableMap<u32, dyn IDisposable>>,
}

impl TerminalDataEventTracker {
    pub fn new(callback: Rc<dyn Fn(u32, &str)>, terminal_service: Rc<dyn ITerminalService>) -> Rc<Self> {
        let store = DisposableStore::new();
        let instance_listeners = store.add_rc(DisposableMap::<u32, dyn IDisposable>::new());
        let bufferer = {
            let callback = callback.clone();
            Rc::new(TerminalDataBufferer::new(move |id, data| callback(id, data)))
        };
        store.add_rc(bufferer.clone());
        let tracker = Rc::new(Self { store: store.clone(), bufferer, instance_listeners });

        for instance in terminal_service.instances() {
            tracker.register_instance(&instance);
        }
        {
            let weak = Rc::downgrade(&tracker);
            store.add(terminal_service.on_did_create_instance().listen(move |instance| {
                if let Some(tracker) = weak.upgrade() {
                    tracker.register_instance(instance);
                }
            }));
        }
        {
            let weak = Rc::downgrade(&tracker);
            store.add(terminal_service.on_did_dispose_instance().listen(move |instance| {
                if let Some(tracker) = weak.upgrade() {
                    tracker.bufferer.stop_buffering(instance.instance_id());
                    tracker.instance_listeners.delete_and_dispose(&instance.instance_id());
                }
            }));
        }
        tracker
    }

    fn register_instance(&self, instance: &Rc<dyn ITerminalInstance>) {
        // Buffer data events to reduce the amount of messages going to the extension host
        self.instance_listeners.set(instance.instance_id(), self.bufferer.start_buffering(instance.instance_id(), &instance.on_data(), String::clone, None), false);
    }
}

impl IDisposable for TerminalDataEventTracker {
    fn dispose(&self) {
        self.store.dispose();
    }
}

/// `ExtensionTerminalLinkProvider`.
struct ExtensionTerminalLinkProvider {
    proxy: Rc<dyn ExtHostTerminalServiceShape>,
}

impl ITerminalExternalLinkProvider for ExtensionTerminalLinkProvider {
    fn provide_links(&self, instance: Rc<dyn ITerminalInstance>, line: &str) -> Promise<Option<Vec<ITerminalLink>>> {
        let proxy = self.proxy.clone();
        let instance_id = instance.instance_id();
        self.proxy.rpc_provide_links(instance_id, line).then(move |ext_host_links| {
            Some(
                ext_host_links
                    .into_iter()
                    .map(|dto| {
                        let proxy = proxy.clone();
                        let link_id = dto.id;
                        ITerminalLink {
                            id: dto.id,
                            start_index: dto.start_index,
                            length: dto.length,
                            label: dto.label,
                            activate: Rc::new(move || proxy.rpc_activate_link(instance_id, link_id)),
                        }
                    })
                    .collect(),
            )
        })
    }
}

/// O `ITerminalProfileProvider` que `$registerProfileProvider` registra.
struct ExtensionProfileProvider {
    proxy: Rc<dyn ExtHostTerminalServiceShape>,
    id: String,
}

impl ITerminalProfileProvider for ExtensionProfileProvider {
    fn create_contributed_terminal_profile(&self, options: ICreateContributedTerminalProfileOptions) -> Promise<()> {
        self.proxy.rpc_create_contributed_profile_terminal(&self.id, options)
    }
}

/// O `ITerminalCompletionProvider` que `$registerCompletionProvider` registra. O `id` pode ser trocado pelo registro
/// (`provider.id = id`); o `remote_id` é o do `provideCompletions` do original, que fecha sobre o `id` do parâmetro.
struct ExtensionCompletionProvider {
    proxy: Rc<dyn ExtHostTerminalServiceShape>,
    id: RefCell<String>,
    remote_id: String,
    trigger_characters: RefCell<Vec<String>>,
}

/// `kind?: number` do DTO como o tipo de completion do terminal.
fn completion_kind_from_value(value: i32) -> Option<TerminalCompletionItemKind> {
    Some(match value {
        0 => TerminalCompletionItemKind::File,
        1 => TerminalCompletionItemKind::Folder,
        2 => TerminalCompletionItemKind::Method,
        3 => TerminalCompletionItemKind::Alias,
        4 => TerminalCompletionItemKind::Argument,
        5 => TerminalCompletionItemKind::Option,
        6 => TerminalCompletionItemKind::OptionValue,
        7 => TerminalCompletionItemKind::Flag,
        8 => TerminalCompletionItemKind::SymbolicLinkFile,
        9 => TerminalCompletionItemKind::SymbolicLinkFolder,
        10 => TerminalCompletionItemKind::Commit,
        11 => TerminalCompletionItemKind::Branch,
        12 => TerminalCompletionItemKind::Tag,
        13 => TerminalCompletionItemKind::Stash,
        14 => TerminalCompletionItemKind::Remote,
        15 => TerminalCompletionItemKind::PullRequest,
        16 => TerminalCompletionItemKind::PullRequestDone,
        100 => TerminalCompletionItemKind::InlineSuggestion,
        101 => TerminalCompletionItemKind::InlineSuggestionAlwaysOnTop,
        _ => return None,
    })
}

/// `{ provider: `ext:${id}`, ...c }`.
fn completion_from_dto(id: &str, dto: ITerminalCompletionItemDto) -> ITerminalCompletion {
    use crate::api::common::ext_host_types::CompletionLabel;
    ITerminalCompletion {
        label: match dto.label {
            CompletionLabel::Text(text) => SimpleCompletionLabel::Text(text),
            CompletionLabel::Detailed(label) => SimpleCompletionLabel::Item(je_editor::languages::CompletionItemLabel {
                label: label.label,
                detail: label.detail,
                description: label.description,
            }),
        },
        provider: format!("ext:{id}"),
        icon: dto.icon,
        detail: Rc::new(RefCell::new(dto.detail)),
        documentation: Rc::new(RefCell::new(dto.documentation)),
        replacement_range: Some([dto.replacement_range.0 as i64, dto.replacement_range.1 as i64]),
        kind: dto.kind.and_then(completion_kind_from_value),
        is_keyword: dto.is_keyword,
        ..ITerminalCompletion::default()
    }
}

impl ITerminalCompletionProvider for ExtensionCompletionProvider {
    fn id(&self) -> String {
        self.id.borrow().clone()
    }

    fn set_id(&self, id: &str) {
        *self.id.borrow_mut() = id.to_owned();
    }

    fn provide_completions(&self, command_line: &str, cursor_index: i64, token: &CancellationToken) -> Promise<Option<TerminalCompletionProviderResult>> {
        let id = self.remote_id.clone();
        self.proxy
            .rpc_provide_terminal_completions(&id, ITerminalCompletionContextDto { command_line: command_line.to_owned(), cursor_index }, token.clone())
            .then(move |completions| {
                let completions = completions?;
                if let Some(resource_options) = completions.resource_options.filter(|options| options.cwd.is_some()) {
                    let items = completions.items.into_iter().map(|item| completion_from_dto(&id, item)).collect();
                    return Some(TerminalCompletionProviderResult::List(TerminalCompletionList::new(
                        Some(items),
                        Some(TerminalCompletionResourceOptions {
                            show_files: resource_options.show_files,
                            show_directories: resource_options.show_directories,
                            glob_pattern: None,
                            cwd: resource_options.cwd.expect("o filtro garante o cwd"),
                            path_separator: resource_options.path_separator,
                        }),
                    )));
                }
                Some(TerminalCompletionProviderResult::Items(completions.items.into_iter().map(|item| completion_from_dto(&id, item)).collect()))
            })
    }

    fn set_trigger_characters(&self, trigger_characters: Vec<String>) {
        *self.trigger_characters.borrow_mut() = trigger_characters;
    }

    fn trigger_characters(&self) -> Option<Vec<String>> {
        Some(self.trigger_characters.borrow().clone())
    }
}

/// O `ITerminalQuickFixProvider` que `$registerQuickFixProvider` registra.
struct ExtensionQuickFixProvider {
    proxy: Rc<dyn ExtHostTerminalServiceShape>,
    log_service: LogServiceRef,
    id: String,
    extension_id: String,
}

impl ITerminalQuickFixProvider for ExtensionQuickFixProvider {
    fn provide_terminal_quick_fixes(
        &self,
        terminal_command: Rc<dyn ITerminalCommand>,
        lines: Option<Vec<String>>,
        options: &mut ITerminalQuickFixOptions,
        token: CancellationToken,
    ) -> Promise<Option<OneOrMany<ITerminalQuickFix>>> {
        if token.is_cancellation_requested() {
            return Promise::resolved(None);
        }
        if let Some(output_matcher) = options.output_matcher.as_mut() {
            if output_matcher.length != 0.0 && output_matcher.length > 40.0 {
                output_matcher.length = 40.0;
                self.log_service.warn("Cannot exceed output matcher length of 40");
            }
        }
        let command_line = terminal_command.command();
        let command_line_match = match_line(&options.command_line_matcher, &JsString::from(command_line.as_str()));
        let (Some(command_line_match), Some(lines)) = (command_line_match, lines) else {
            return Promise::resolved(None);
        };
        let output_match = options.output_matcher.as_ref().and_then(|output_matcher| get_output_match_for_lines(&lines, output_matcher));
        if output_match.is_none() {
            return Promise::resolved(None);
        }
        let match_result = TerminalCommandMatchResultDto { command_line_match, output_match, command_line };

        let (id, source) = (self.id.clone(), self.extension_id.clone());
        self.proxy.rpc_provide_terminal_quick_fixes(&self.id, match_result, token).then(move |result| match result? {
            OneOrMany::Many(fixes) => Some(OneOrMany::Many(fixes.into_iter().map(|fix| parse_quick_fix(&id, &source, fix)).collect())),
            OneOrMany::One(fix) => Some(OneOrMany::One(parse_quick_fix(&id, &source, fix))),
        })
    }
}

/// `getOutputMatchForLines(lines, outputMatcher)`.
pub fn get_output_match_for_lines(lines: &[String], output_matcher: &ITerminalOutputMatcher) -> Option<ITerminalOutputMatch> {
    let joined = JsString::from(lines.join("\n").as_str());
    match_line(&output_matcher.line_matcher, &joined).map(|regex_match| ITerminalOutputMatch { regex_match, output_lines: lines.to_vec() })
}

/// `parseQuickFix(id, source, fix)`. Num comando do VS Code o `id` da correção é o do comando (o espalhamento `...fix` o
/// sobrescreve).
pub fn parse_quick_fix(id: &str, source: &str, fix: TerminalQuickFix) -> ITerminalQuickFix {
    match fix {
        TerminalQuickFix::Opener(opener) => ITerminalQuickFix { id: id.to_owned(), source: source.to_owned(), action: TerminalQuickFixAction::Opener { uri: opener.uri } },
        TerminalQuickFix::Command(command) => ITerminalQuickFix { id: command.id.clone(), source: source.to_owned(), action: TerminalQuickFixAction::VscodeCommand(command) },
        TerminalQuickFix::TerminalCommand(command) => ITerminalQuickFix {
            id: id.to_owned(),
            source: source.to_owned(),
            action: TerminalQuickFixAction::TerminalCommand { terminal_command: command.terminal_command, should_execute: command.should_execute },
        },
    }
}
