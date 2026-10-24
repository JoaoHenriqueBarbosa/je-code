//! Porte de `src/vs/workbench/contrib/terminal/browser/terminalService.ts`: o [`TerminalService`], o dono do conjunto de
//! instâncias de terminal (as do painel, as dos editores e as de fundo), do terminal ativo, da criação de terminais, do descarte
//! seguro (com a confirmação de `confirmOnKill`), do estado da conexão com o backend primário e da atualização de título e ícone
//! no pty host.
//!
//! Recortes (cada um depende de um contrato que o porte ainda não tem, ou é interface do frontend):
//! - **Persistência de layout e reconexão.** `_reconnectToRemoteTerminals`, `_reconnectToLocalTerminals`, `_recreateTerminalGroups`,
//!   `_recreateTerminalGroup`, `_reviveBackgroundTerminalInstances`, `_saveState`, `restoredGroupCount`, `getReconnectedTerminals`,
//!   o tratador de `onDidRequestDetach`/`_addInstanceToGroup` (o `ITerminalBackend` do porte não tem `getTerminalLayoutInfo`,
//!   `setTerminalLayoutInfo`, `requestDetachInstance`, `acceptDetachInstanceReply`, `persistTerminalState` nem
//!   `getPerformanceMarks`, e não há grupo de terminais). O que fica de `_attachProcessLayoutListeners` são os dois ouvintes de
//!   título e de ícone (`_updateTitle` e `_updateIcon`), ligados quando o backend existe e a persistência de sessões está ligada.
//! - **Encerramento.** `_onBeforeShutdown`, `_onWillShutdown`, `_shouldReviveProcesses`, `_onBeforeShutdownConfirmation`,
//!   `setNativeDelegate` e `_isShuttingDown` (dependem da persistência de layout e do `ILifecycleService` nativo). O ouvinte de
//!   `onDidChangeActiveInstance` esconde o painel sem a guarda de encerramento.
//! - **Perfis.** `ITerminalProfileService` (o porte tem só `onDidChangeAvailableProfiles` e `registerTerminalProfileProvider`):
//!   `showProfileQuickPick`, `createContributedTerminalProfile`, `_getContributedProfile`, a espera de `profilesReady`, o perfil
//!   padrão e o laço dos perfis contribuídos de `createTerminal`. Sem `config`, o `IShellLaunchConfig` é o vazio; sem suporte a
//!   processos e sem perfil contribuído, `createTerminal` lança o erro do original.
//! - **Interface.** `TerminalEditorStyle` (CSS dos ícones das abas), `createDetachedTerminal` e seus ouvintes, `setContainers`,
//!   `focusInstance`, `focusActiveInstance`, `createAndFocusTerminal`, `getActiveOrCreateInstance` (o `xterm.isStdinDisabled` é do
//!   emulador), `openResource` (a revelação do comando), `moveToEditor`, `moveIntoNewEditor`, `moveToTerminalView`,
//!   `moveToBackground`, `getInstanceHost`, `getInstanceFromResource`, `isAttachedToTerminal` e o ouvinte de
//!   `onRequestAddInstanceToGroup`.
//! - **`getCwdForSplit`.** Precisa do seletor de pastas do `ICommandService` e da leitura do diretório da instância pai; sem ele,
//!   o `_resolveCwd` de uma divisão sem `cwd` deixa o `cwd` indefinido (o valor que o original devolve quando o seletor é
//!   cancelado), depois de conferir que há instância para dividir.
//! - **`columnToEditorGroup`.** `_getEditorOptions` devolve o `viewColumn` do chamador; resolvê-lo para um grupo de editores é do
//!   serviço de editores de terminal (`ITerminalEditorService.open_editor`).
//!
//! Divergências:
//! - **Async.** Os métodos `async` do original devolvem [`Promise`] e rodam como tarefas locais ([`run_async`]). `safeDisposeTerminal`
//!   e `createTerminal` rodam sincronamente até o primeiro `await`, como no JavaScript, no que o porte consegue: o descarte sem
//!   confirmação acontece na hora.
//! - **Injeção.** Os parâmetros decorados do construtor que o porte usa são a tupla `Deps` do [`Injectable`]; o
//!   `IInstantiationService`, o `IEditorGroupsService`, o `ITerminalProfileService`, o `ILifecycleService`, o
//!   `INotificationService`, o `ICommandService`, o `IKeybindingService`, o `ITimerService`, o `IThemeService` e o
//!   `IConfigurationService` só entram nos membros recortados acima.
//! - **`ITerminalInstanceHost`.** `_terminalGroupService` e `_terminalEditorService` são lidos como o host de instâncias que cada um
//!   entrega ([`ITerminalGroupService::instance_host`] e [`ITerminalEditorService::instance_host`]); a `Map<host, instance>` de
//!   terminais ativos é uma lista na ordem de registro (grupos, depois editores).
//! - **`createGroup`, `getGroupForInstance(...).split`.** O serviço de grupos devolve a instância do grupo novo
//!   ([`ITerminalGroupService::create_group`]) e divide a instância pai ([`ITerminalGroupService::split_instance`]); o `ITerminalGroup`
//!   não faz parte do porte.
//! - **`onDidChangeActiveGroup`.** Dispara sem carga (o grupo ativo não faz parte do porte).
//! - **`@memoize` dos eventos `onAnyInstance*`.** São [`Memoize`]; o multiplexador de cada um é registrado no store do serviço.
//!   `onAnyInstanceData` leva a tupla `(instância, dado)` e `onAnyInstanceIconChange` o [`IconChangedEvent`].
//! - **`@debounce(500)`.** `_updateTitle` e `_updateIcon` usam o [`Debounce`] do `je-base`.
//! - **`_primaryBackend`.** É um campo privado no original (o teste o atribui por reflexão): [`TerminalService::set_primary_backend`]
//!   o atribui.
//! - **`mark(...)` de desempenho.** Não há `performance.mark` no porte; as marcas não são emitidas.
//! - **`resolveLocation`.** O `parentTerminal` do porte é a instância (e não `ITerminalInstance | Promise<ITerminalInstance>`), então
//!   a resolução é síncrona.
//! - **`showBackgroundTerminal`.** Não tem `await` no original; é síncrono e devolve `()`.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::decorators::{Debounce, Memoize};
use je_base::dynamic::{Dynamic, Truthy};
use je_base::errors::Error;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{DisposableMap, DisposableRef, DisposableStore, IDisposable};
use je_base::nls::localize;
use je_base::platform::Platform;
use je_base::promise::{Promise, Resolver};
use je_base::uri::{URI, UriComponents};
use je_instantiation::{Injectable, service_impl};
use je_platform::contextkey::contextkey::{IContextKey, IContextKeyService};
use je_platform::dialogs::{BaseDialogOptions, Confirmation, DialogType, DialogTypeOrSeverity, IDialogService};
use je_platform::log::Logger;
use je_platform::terminal::capabilities::capabilities::TerminalCapability;
use je_platform::terminal::terminal::{
    IShellLaunchConfig, ITerminalLaunchError, ITerminalLogService, InitialText, ShellCwd, TerminalExitReason, TerminalLocation, TerminalType, TitleEventSource,
};
use je_platform::terminal::terminal_strings::{ITerminalFormatMessageOptions, format_message_for_terminal};

use super::terminal::{
    ICreateTerminalOptions, ITerminalConfigurationService, ITerminalEditorService, ITerminalGroupService, ITerminalInstance, ITerminalInstanceHost,
    ITerminalInstanceService, ITerminalService, TerminalCreateLocation,
};
use super::terminal_events::ITerminalServiceEvents;
use super::terminal_instance::IconChangedEvent;
use super::terminal_process_manager::run_async;
use crate::contrib::terminal::common::terminal::{IStartExtensionTerminalRequest, ITerminalProcessExtHostProxy};
use crate::contrib::terminal::common::terminal_backend::ITerminalBackend;
use crate::contrib::terminal::common::terminal_context_key;
use crate::services::environment::environment_service::IWorkbenchEnvironmentService;
use crate::services::extensions::extensions::IExtensionService;
use crate::services::remote::remote_agent_service::IRemoteAgentService;

/// `TerminalConnectionState` (de `terminal.ts`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerminalConnectionState {
    Connecting,
    Connected,
}

/// `IBackgroundTerminal`.
struct BackgroundTerminal {
    instance: Rc<dyn ITerminalInstance>,
    terminal_location_options: Option<TerminalCreateLocation>,
}

/// `ITerminalLocationOptions` (o `location` de `ICreateTerminalOptions`) como referência.
type LocationOptions<'a> = Option<&'a TerminalCreateLocation>;

/// `!cwd`: o `cwd` ausente ou o texto vazio (um URI é sempre verdadeiro).
fn cwd_is_empty(cwd: &Option<ShellCwd>) -> bool {
    match cwd {
        None => true,
        Some(ShellCwd::Path(path)) => path.is_empty(),
        Some(ShellCwd::Uri(_)) => false,
    }
}

/// `TerminalService`.
pub struct TerminalService {
    this: Weak<Self>,
    /// O `_store` do `Disposable` do original.
    store: Rc<DisposableStore>,

    terminal_shell_type_context_key: Rc<dyn IContextKey>,
    process_support_context_key: Rc<dyn IContextKey>,
    terminal_has_been_created: Rc<dyn IContextKey>,
    terminal_count_context_key: Rc<dyn IContextKey>,

    /// `_hostActiveTerminals`: o terminal ativo de cada host, na ordem de registro (grupos, depois editores).
    host_active_terminals: RefCell<Vec<Option<Rc<dyn ITerminalInstance>>>>,
    backgrounded_terminal_instances: RefCell<Vec<BackgroundTerminal>>,
    backgrounded_terminal_disposables: Rc<DisposableMap<u32, dyn IDisposable>>,
    primary_backend: RefCell<Option<Rc<dyn ITerminalBackend>>>,
    connection_state: Cell<TerminalConnectionState>,
    when_connected: Promise<()>,
    when_connected_resolver: Resolver<()>,
    active_instance: RefCell<Option<Rc<dyn ITerminalInstance>>>,

    update_title_debounce: Debounce<Option<Rc<dyn ITerminalInstance>>, Option<Rc<dyn ITerminalInstance>>>,
    update_icon_debounce: Debounce<(Rc<dyn ITerminalInstance>, bool), (Rc<dyn ITerminalInstance>, bool)>,

    on_did_create_instance: Rc<Emitter<Rc<dyn ITerminalInstance>>>,
    on_did_change_instance_dimensions: Rc<Emitter<Rc<dyn ITerminalInstance>>>,
    on_did_register_process_support: Rc<Emitter<()>>,
    on_did_change_connection_state: Rc<Emitter<()>>,
    on_did_request_start_extension_terminal: Rc<Emitter<IStartExtensionTerminalRequest>>,
    on_did_dispose_instance: Rc<Emitter<Rc<dyn ITerminalInstance>>>,
    on_did_focus_instance: Rc<Emitter<Rc<dyn ITerminalInstance>>>,
    on_did_change_active_instance: Rc<Emitter<Option<Rc<dyn ITerminalInstance>>>>,
    on_did_change_instances: Rc<Emitter<()>>,
    on_did_change_instance_capability: Rc<Emitter<Rc<dyn ITerminalInstance>>>,
    on_did_change_active_group: Rc<Emitter<()>>,

    on_any_instance_data: Memoize<Event<(Rc<dyn ITerminalInstance>, String)>>,
    on_any_instance_data_input: Memoize<Event<Rc<dyn ITerminalInstance>>>,
    on_any_instance_icon_change: Memoize<Event<IconChangedEvent>>,
    on_any_instance_maximum_dimensions_change: Memoize<Event<Rc<dyn ITerminalInstance>>>,
    on_any_instance_process_id_ready: Memoize<Event<Rc<dyn ITerminalInstance>>>,
    on_any_instance_title_change: Memoize<Event<Rc<dyn ITerminalInstance>>>,
    on_any_instance_shell_type_changed: Memoize<Event<Rc<dyn ITerminalInstance>>>,

    context_key_service: Rc<dyn IContextKeyService>,
    log_service: Rc<dyn ITerminalLogService>,
    dialog_service: Rc<dyn IDialogService>,
    remote_agent_service: Rc<dyn IRemoteAgentService>,
    environment_service: Rc<dyn IWorkbenchEnvironmentService>,
    terminal_configuration_service: Rc<dyn ITerminalConfigurationService>,
    terminal_editor_service: Rc<dyn ITerminalEditorService>,
    terminal_group_service: Rc<dyn ITerminalGroupService>,
    terminal_instance_service: Rc<dyn ITerminalInstanceService>,
    extension_service: Rc<dyn IExtensionService>,
}

impl TerminalService {
    fn rc(&self) -> Rc<Self> {
        self.this.upgrade().expect("o serviço de terminais foi descartado da memória")
    }

    fn group_host(&self) -> Rc<dyn ITerminalInstanceHost> {
        self.terminal_group_service.instance_host()
    }

    fn editor_host(&self) -> Rc<dyn ITerminalInstanceHost> {
        self.terminal_editor_service.instance_host()
    }

    /// `this._terminalConfigurationService.config[key]` como texto (a leitura de `getValue` de um texto de configuração).
    fn config_text(&self, key: &str) -> String {
        je_platform::js::to_js_string(&self.terminal_configuration_service.config().get_str(key)).to_string_lossy()
    }

    /// `this._terminalConfigurationService.config[key]` com a verdade de JavaScript.
    fn config_flag(&self, key: &str) -> bool {
        self.terminal_configuration_service.config().get_str(key).is_truthy()
    }

    /// `isProcessSupportRegistered`.
    pub fn is_process_support_registered(&self) -> bool {
        self.process_support_context_key.get().is_truthy()
    }

    /// `connectionState`.
    pub fn connection_state(&self) -> TerminalConnectionState {
        self.connection_state.get()
    }

    /// `whenConnected`.
    pub fn when_connected(&self) -> Promise<()> {
        self.when_connected.clone()
    }

    /// `foregroundInstances`: all non-background terminals.
    pub fn foreground_instances(&self) -> Vec<Rc<dyn ITerminalInstance>> {
        let mut instances = self.group_host().instances();
        instances.extend(self.editor_host().instances());
        instances
    }

    /// `getPrimaryBackend()`.
    pub fn get_primary_backend(&self) -> Option<Rc<dyn ITerminalBackend>> {
        self.primary_backend.borrow().clone()
    }

    /// `_primaryBackend = backend` (ver "Divergências").
    pub fn set_primary_backend(&self, backend: Option<Rc<dyn ITerminalBackend>>) {
        *self.primary_backend.borrow_mut() = backend;
    }

    /// `onDidRegisterProcessSupport`.
    pub fn on_did_register_process_support(&self) -> Event<()> {
        self.on_did_register_process_support.event()
    }

    /// `onDidChangeConnectionState`.
    pub fn on_did_change_connection_state(&self) -> Event<()> {
        self.on_did_change_connection_state.event()
    }

    /// `onDidFocusInstance`.
    pub fn on_did_focus_instance(&self) -> Event<Rc<dyn ITerminalInstance>> {
        self.on_did_focus_instance.event()
    }

    /// `onDidChangeInstances`.
    pub fn on_did_change_instances(&self) -> Event<()> {
        self.on_did_change_instances.event()
    }

    /// `onDidChangeInstanceCapability`.
    pub fn on_did_change_instance_capability(&self) -> Event<Rc<dyn ITerminalInstance>> {
        self.on_did_change_instance_capability.event()
    }

    /// `onDidChangeActiveGroup`.
    pub fn on_did_change_active_group(&self) -> Event<()> {
        self.on_did_change_active_group.event()
    }

    /// `_backgroundedTerminalDisposables.size`.
    pub fn backgrounded_terminal_disposables_size_for_testing(&self) -> usize {
        self.backgrounded_terminal_disposables.len()
    }

    /// O evento que dispara quando `get_event` dispara em qualquer instância, com o multiplexador registrado no store do serviço
    /// (`this._register(this.createOnInstanceEvent(...)).event`).
    fn any_instance_event<T: Clone + 'static>(&self, get_event: impl Fn(&Rc<dyn ITerminalInstance>) -> Event<T> + 'static) -> Event<T> {
        let multiplexer = self.create_on_instance_event(get_event);
        let event = multiplexer.event.clone();
        self.store.add(Rc::new(multiplexer));
        event
    }

    /// `onAnyInstanceData`.
    pub fn on_any_instance_data(&self) -> Event<(Rc<dyn ITerminalInstance>, String)> {
        self.on_any_instance_data
            .get(|| {
                self.any_instance_event(|instance| {
                    let instance = instance.clone();
                    instance.on_data().map(move |data| (instance.clone(), data.clone()), None)
                })
            })
            .clone()
    }

    /// `onAnyInstanceIconChange`.
    pub fn on_any_instance_icon_change(&self) -> Event<IconChangedEvent> {
        self.on_any_instance_icon_change.get(|| self.any_instance_event(|instance| instance.on_icon_changed())).clone()
    }

    fn init(this: &Rc<Self>) {
        // the below avoids having to poll routinely: (a atualização dos perfis disponíveis é do `ITerminalProfileService`, recortado)
        let group_host = this.group_host();
        let editor_host = this.editor_host();
        Self::forward_instance_host_events(this, &group_host);
        Self::forward_instance_host_events(this, &editor_host);

        let weak = Rc::downgrade(this);
        this.store.add(this.terminal_group_service.on_did_change_active_group().listen({
            let weak = weak.clone();
            move |_| {
                if let Some(this) = weak.upgrade() {
                    this.on_did_change_active_group.fire(());
                }
            }
        }));
        this.store.add(this.terminal_instance_service.on_did_create_instance().listen({
            let weak = weak.clone();
            move |instance| {
                if let Some(this) = weak.upgrade() {
                    this.init_instance_listeners(instance);
                    this.on_did_create_instance.fire(instance.clone());
                }
            }
        }));

        // Hide the panel if there are no more instances. (A guarda de encerramento é do recorte do encerramento.)
        this.store.add(group_host.on_did_change_active_instance().listen({
            let weak = weak.clone();
            move |instance| {
                let Some(this) = weak.upgrade() else { return };
                if instance.is_none() && this.config_flag("hideOnLastClosed") {
                    this.terminal_group_service.hide_panel();
                }
                match instance.as_ref().and_then(|instance| instance.shell_type()) {
                    Some(shell_type) => this.terminal_shell_type_context_key.set(Dynamic::string(shell_type.as_str())),
                    None => this.terminal_shell_type_context_key.reset(),
                }
            }
        }));

        this.handle_instance_context_keys();
        this.process_support_context_key.set(Dynamic::Bool(!Platform::detect().is_web || this.remote_agent_service.get_connection().is_some()));

        this.initialize_primary_backend();
    }

    /// `_handleInstanceContextKeys`.
    fn handle_instance_context_keys(self: &Rc<Self>) {
        let terminal_is_open_context = terminal_context_key::is_open_bind_to(&*self.context_key_service);
        let weak = Rc::downgrade(self);
        self.store.add(self.on_did_change_instances.event().listen(move |_| {
            let Some(this) = weak.upgrade() else { return };
            let count = this.instances().len();
            terminal_is_open_context.set(Dynamic::Bool(count > 0));
            this.terminal_count_context_key.set(Dynamic::Number(count as f64));
        }));
    }

    /// `_forwardInstanceHostEvents`.
    fn forward_instance_host_events(this: &Rc<Self>, host: &Rc<dyn ITerminalInstanceHost>) {
        let index = {
            let mut host_active_terminals = this.host_active_terminals.borrow_mut();
            host_active_terminals.push(None);
            host_active_terminals.len() - 1
        };
        let weak = Rc::downgrade(this);
        this.store.add(host.on_did_change_instances().forward(&this.on_did_change_instances));
        this.store.add(host.on_did_dispose_instance().forward(&this.on_did_dispose_instance));
        this.store.add(host.on_did_change_active_instance().listen({
            let weak = weak.clone();
            move |instance| {
                if let Some(this) = weak.upgrade() {
                    this.evaluate_active_instance(index, instance.clone());
                }
            }
        }));
        this.store.add(host.on_did_focus_instance().listen(move |instance| {
            if let Some(this) = weak.upgrade() {
                this.on_did_focus_instance.fire(instance.clone());
                this.evaluate_active_instance(index, Some(instance.clone()));
            }
        }));
        this.store.add(host.on_did_change_instance_capability().forward(&this.on_did_change_instance_capability));
    }

    /// `_evaluateActiveInstance`.
    fn evaluate_active_instance(&self, host_index: usize, instance: Option<Rc<dyn ITerminalInstance>>) {
        // Track the latest active terminal for each host so that when one becomes undefined, the TerminalService's active terminal is
        // set to the last active terminal from the other host. This means if the last terminal editor is closed such that it becomes
        // undefined, the last active group's terminal will be used as the active terminal if available.
        let mut instance = instance;
        {
            let mut host_active_terminals = self.host_active_terminals.borrow_mut();
            host_active_terminals[host_index] = instance.clone();
            if instance.is_none() {
                for active in host_active_terminals.iter().flatten() {
                    instance = Some(active.clone());
                }
            }
        }
        *self.active_instance.borrow_mut() = instance.clone();
        self.on_did_change_active_instance.fire(instance);
    }

    /// `_initializePrimaryBackend`.
    fn initialize_primary_backend(&self) -> Promise<()> {
        let this = self.rc();
        run_async(async move {
            let remote_authority = this.environment_service.remote_authority();
            let backend = this.terminal_instance_service.get_backend(remote_authority.as_deref()).await?;
            *this.primary_backend.borrow_mut() = backend.clone();
            let enable_terminal_reconnection = this.config_flag("enablePersistentSessions");

            // Connect to the extension host if it's there, set the connection state to connected when it's done. This should happen
            // even when there is no extension host.
            this.connection_state.set(TerminalConnectionState::Connecting);

            // A reconexão (`_reconnectToRemoteTerminals` e `_reconnectToLocalTerminals`) é recortada; o que sobra dela é ligar os
            // ouvintes de título e de ícone quando o backend existe.
            if enable_terminal_reconnection {
                let reconnect_backend = if remote_authority.as_deref().is_some_and(|authority| !authority.is_empty()) {
                    this.terminal_instance_service.get_backend(remote_authority.as_deref()).await?
                } else {
                    this.terminal_instance_service.get_backend(None).await?
                };
                if reconnect_backend.is_some() {
                    this.attach_process_layout_listeners();
                }
            }

            this.set_connected();
            for registered_backend in this.terminal_instance_service.get_registered_backends() {
                registered_backend.set_ready();
            }
            this.when_connected_resolver.resolve(());
            Ok(())
        })
    }

    /// `_setConnected`.
    fn set_connected(&self) {
        self.connection_state.set(TerminalConnectionState::Connected);
        self.on_did_change_connection_state.fire(());
        self.log_service.trace("Pty host ready");
    }

    /// `_attachProcessLayoutListeners`, sem o `_saveState` (ver os recortes).
    fn attach_process_layout_listeners(&self) {
        let weak = self.this.clone();
        self.store.add(self.on_any_instance_title_change().listen({
            let weak = weak.clone();
            move |instance| {
                if let Some(this) = weak.upgrade() {
                    this.update_title(Some(instance.clone()));
                }
            }
        }));
        self.store.add(self.on_any_instance_icon_change().listen(move |event| {
            if let Some(this) = weak.upgrade() {
                this.update_icon(event.instance.clone(), event.user_initiated);
            }
        }));
    }

    /// `_updateTitle` (with `@debounce(500)`).
    pub fn update_title(&self, instance: Option<Rc<dyn ITerminalInstance>>) {
        self.update_title_debounce.call(instance);
    }

    fn do_update_title(&self, instance: Option<Rc<dyn ITerminalInstance>>) {
        if !self.config_flag("enablePersistentSessions") {
            return;
        }
        let Some(instance) = instance else { return };
        if instance.shell_launch_config().borrow().custom_pty_implementation.is_some() {
            return;
        }
        let Some(persistent_process_id) = instance.persistent_process_id().filter(|id| *id != 0) else { return };
        let title = instance.title();
        if title.is_empty() || instance.is_disposed() {
            return;
        }
        let backend = self.primary_backend.borrow().clone();
        if let Some(backend) = backend {
            match instance.static_title().filter(|static_title| !static_title.is_empty()) {
                Some(static_title) => {
                    let _ = backend.update_title(persistent_process_id, &static_title, TitleEventSource::Api);
                }
                None => {
                    let _ = backend.update_title(persistent_process_id, &title, instance.title_source());
                }
            }
        }
    }

    /// `_updateIcon` (with `@debounce(500)`).
    pub fn update_icon(&self, instance: Rc<dyn ITerminalInstance>, user_initiated: bool) {
        self.update_icon_debounce.call((instance, user_initiated));
    }

    fn do_update_icon(&self, instance: Rc<dyn ITerminalInstance>, user_initiated: bool) {
        if !self.config_flag("enablePersistentSessions") {
            return;
        }
        if instance.shell_launch_config().borrow().custom_pty_implementation.is_some() {
            return;
        }
        let Some(persistent_process_id) = instance.persistent_process_id().filter(|id| *id != 0) else { return };
        let Some(icon) = instance.icon() else { return };
        if instance.is_disposed() {
            return;
        }
        let backend = self.primary_backend.borrow().clone();
        if let Some(backend) = backend {
            let color = instance.color();
            let _ = backend.update_icon(persistent_process_id, user_initiated, &icon, color.as_deref());
        }
    }

    /// `_initInstanceListeners`.
    fn init_instance_listeners(&self, instance: &Rc<dyn ITerminalInstance>) {
        let instance_disposables = DisposableStore::new();
        let weak = self.this.clone();
        instance_disposables.add(instance.on_dimensions_changed().listen({
            let (weak, instance) = (weak.clone(), instance.clone());
            move |_| {
                if let Some(this) = weak.upgrade() {
                    this.on_did_change_instance_dimensions.fire(instance.clone());
                }
            }
        }));
        instance_disposables.add(instance.on_did_focus().listen({
            let weak = weak.clone();
            move |focused| {
                if let Some(this) = weak.upgrade() {
                    this.on_did_change_active_instance.fire(Some(focused.clone()));
                }
            }
        }));
        instance_disposables.add(instance.on_did_change_shell_type().listen({
            let (weak, instance) = (weak.clone(), instance.clone());
            move |_| {
                if let Some(this) = weak.upgrade() {
                    this.activate_by_shell_type_event(&instance, "onTerminal");
                }
            }
        }));
        let activate = {
            let (weak, instance) = (weak, instance.clone());
            move || {
                if let Some(this) = weak.upgrade()
                    && instance.capabilities().has(TerminalCapability::CommandDetection)
                {
                    this.activate_by_shell_type_event(&instance, "onTerminalShellIntegration");
                }
            }
        };
        instance_disposables.add(instance.capabilities().on_did_add_capability().run_and_subscribe(move |_| activate(), None));

        // `const disposeListener = this._register(instance.onDisposed(() => { instanceDisposables.dispose(); this._store.delete(disposeListener); }))`
        let slot: Rc<RefCell<Option<DisposableRef>>> = Rc::new(RefCell::new(None));
        let dispose_listener = instance.on_disposed().listen({
            let (weak, slot) = (self.this.clone(), slot.clone());
            move |_| {
                instance_disposables.dispose();
                let listener = slot.borrow_mut().take();
                if let (Some(this), Some(listener)) = (weak.upgrade(), listener) {
                    this.store.delete(&listener);
                }
            }
        });
        *slot.borrow_mut() = Some(dispose_listener.clone());
        self.store.add(dispose_listener);
    }

    /// `this._extensionService.activateByEvent(`${prefix}:${instance.shellType}`)`.
    fn activate_by_shell_type_event(&self, instance: &Rc<dyn ITerminalInstance>, prefix: &str) {
        let shell_type = instance.shell_type().map_or("undefined", |shell_type| shell_type.as_str());
        let _ = self.extension_service.activate_by_event(&format!("{prefix}:{shell_type}"), None);
    }

    /// `setNextCommandId`.
    fn set_next_command_id_task(&self, id: u32, command_line: &str, command_id: &str) -> Promise<()> {
        let backend = self.primary_backend.borrow().clone();
        match backend {
            Some(backend) if id > 0 => backend.set_next_command_id(id, command_line, command_id),
            _ => Promise::resolved(()),
        }
    }

    /// `safeDisposeTerminal`.
    pub fn safe_dispose_terminal(&self, instance: Rc<dyn ITerminalInstance>) -> Promise<()> {
        // Confirm on kill in the editor is handled by the editor input
        let needs_confirmation = instance.target() != Some(TerminalLocation::Editor)
            && instance.has_child_processes()
            && matches!(self.config_text("confirmOnKill").as_str(), "panel" | "always");
        if !needs_confirmation {
            return Self::dispose_and_wait_for_exit(instance);
        }
        // O `_showTerminalCloseConfirmation` roda até o primeiro `await` na hora (o diálogo já é pedido aqui).
        let confirmation = self.show_terminal_close_confirmation(true);
        run_async(async move {
            let veto = confirmation.await?;
            if veto {
                return Ok(());
            }
            Self::dispose_and_wait_for_exit(instance).await
        })
    }

    /// `new Promise<void>(r => { Event.once(instance.onExit)(() => r()); instance.dispose(TerminalExitReason.User); })`.
    fn dispose_and_wait_for_exit(instance: Rc<dyn ITerminalInstance>) -> Promise<()> {
        let (promise, resolver) = Promise::new();
        instance.on_exit_signal().once().listen(move |_| resolver.resolve(()));
        instance.dispose_with_reason(Some(TerminalExitReason::User));
        promise
    }

    /// `_showTerminalCloseConfirmation`: devolve o veto (`!confirmed`).
    fn show_terminal_close_confirmation(&self, single_terminal: bool) -> Promise<bool> {
        let foreground_instances = self.foreground_instances();
        let message = if foreground_instances.len() == 1 || single_terminal {
            localize("terminalService.terminalCloseConfirmationSingular", "Do you want to terminate the active terminal session?", &[])
        } else {
            localize(
                "terminalService.terminalCloseConfirmationPlural",
                "Do you want to terminate the {0} active terminal sessions?",
                &[foreground_instances.len().to_string().as_str()],
            )
        };
        let confirmation = self.dialog_service.confirm(Confirmation {
            base: BaseDialogOptions { dialog_type: Some(DialogTypeOrSeverity::Type(DialogType::Warning)), ..BaseDialogOptions::new(message) },
            primary_button: Some(localize("terminate", "&&Terminate", &[])),
            cancel_button: None,
        });
        run_async(async move { Ok(!confirmation.await?.confirmed) })
    }

    /// `refreshActiveGroup`.
    pub fn refresh_active_group(&self) {
        self.on_did_change_active_group.fire(());
    }

    /// `revealTerminal`.
    pub fn reveal_terminal(&self, source: &Rc<dyn ITerminalInstance>, preserve_focus: bool) -> Promise<()> {
        if source.target() == Some(TerminalLocation::Editor) {
            self.terminal_editor_service.reveal_active_editor(preserve_focus)
        } else {
            self.terminal_group_service.show_panel(false)
        }
    }

    /// `revealActiveTerminal`.
    pub fn reveal_active_terminal(&self, preserve_focus: bool) -> Promise<()> {
        match self.active_instance() {
            Some(instance) => self.reveal_terminal(&instance, preserve_focus),
            None => Promise::resolved(()),
        }
    }

    /// `getDefaultInstanceHost`.
    pub fn get_default_instance_host(&self) -> Rc<dyn ITerminalInstanceHost> {
        if self.terminal_configuration_service.default_location() == TerminalLocation::Editor { self.editor_host() } else { self.group_host() }
    }

    /// `resolveLocation`.
    pub fn resolve_location(&self, location: LocationOptions<'_>) -> Option<TerminalLocation> {
        match location? {
            // since we don't set the target unless it's an editor terminal, this is necessary
            TerminalCreateLocation::ParentTerminal(parent_terminal) => Some(parent_terminal.target().unwrap_or(TerminalLocation::Panel)),
            TerminalCreateLocation::Editor { .. } => Some(TerminalLocation::Editor),
            TerminalCreateLocation::SplitActiveTerminal(_) => {
                let target = self.active_instance.borrow().as_ref().and_then(|instance| instance.target());
                Some(target.unwrap_or(TerminalLocation::Panel))
            }
            TerminalCreateLocation::Location(location) => Some(*location),
        }
    }

    /// `_getSplitParent`.
    fn get_split_parent(&self, location: LocationOptions<'_>) -> Option<Rc<dyn ITerminalInstance>> {
        match location {
            Some(TerminalCreateLocation::ParentTerminal(parent_terminal)) => Some(parent_terminal.clone()),
            Some(TerminalCreateLocation::SplitActiveTerminal(_)) => self.active_instance(),
            _ => None,
        }
    }

    /// `_getEditorOptions`: o `viewColumn` do chamador (ver os recortes).
    fn get_editor_options(location: LocationOptions<'_>) -> Option<TerminalCreateLocation> {
        match location {
            Some(location @ TerminalCreateLocation::Editor { .. }) => Some(location.clone()),
            _ => None,
        }
    }

    /// `_resolveCwd`.
    fn resolve_cwd(&self, shell_launch_config: &mut IShellLaunchConfig, split_active_terminal: bool, options: &ICreateTerminalOptions) -> Result<(), Error> {
        if cwd_is_empty(&shell_launch_config.cwd) {
            if let Some(cwd) = &options.cwd {
                shell_launch_config.cwd = Some(cwd.clone());
            } else if split_active_terminal && options.location.is_some() {
                let mut parent = self.active_instance();
                if let Some(TerminalCreateLocation::ParentTerminal(parent_terminal)) = &options.location {
                    parent = Some(parent_terminal.clone());
                }
                if parent.is_none() {
                    return Err(Error::new("Cannot split without an active instance"));
                }
                // `getCwdForSplit` é recortado (ver o cabeçalho): o `cwd` fica indefinido.
            }
        }
        Ok(())
    }

    /// `_evaluateLocalCwd`.
    fn evaluate_local_cwd(&self, shell_launch_config: &mut IShellLaunchConfig) {
        if self.environment_service.is_sessions_window() {
            return;
        }

        // Add welcome message and title annotation for local terminals launched within remote or virtual workspaces
        if let Some(ShellCwd::Uri(uri)) = &shell_launch_config.cwd
            && uri.scheme().to_string_lossy() == "file"
        {
            let options = ITerminalFormatMessageOptions { exclude_leading_new_line: true, loud_formatting: true };
            if self.context_key_service.get_context_key_value("virtualWorkspace").is_truthy() {
                shell_launch_config.initial_text = Some(InitialText::Text(format_message_for_terminal(
                    &localize("localTerminalVirtualWorkspace", "This shell is open to a {0}local{1} folder, NOT to the virtual folder", &["\x1b[3m", "\x1b[23m"]),
                    options,
                )));
                shell_launch_config.type_ = Some(TerminalType::Local);
            } else if self.remote_agent_service.get_connection().is_some() {
                shell_launch_config.initial_text = Some(InitialText::Text(format_message_for_terminal(
                    &localize("localTerminalRemote", "This shell is running on your {0}local{1} machine, NOT on the connected remote machine", &["\x1b[3m", "\x1b[23m"]),
                    options,
                )));
                shell_launch_config.type_ = Some(TerminalType::Local);
            }
        }
    }

    /// `_splitTerminal`.
    fn split_terminal(&self, mut shell_launch_config: IShellLaunchConfig, location: TerminalLocation, parent: Rc<dyn ITerminalInstance>) -> Promise<Rc<dyn ITerminalInstance>> {
        // Use the URI from the base instance if it exists, this will correctly split local terminals
        let parent_cwd = parent.shell_launch_config().borrow().cwd.clone();
        if !matches!(shell_launch_config.cwd, Some(ShellCwd::Uri(_)))
            && let Some(ShellCwd::Uri(parent_uri)) = parent_cwd
        {
            let mut path = match &shell_launch_config.cwd {
                Some(ShellCwd::Path(path)) if !path.is_empty() => path.clone(),
                _ => parent_uri.path().to_string_lossy(),
            };
            if !parent_uri.authority().is_empty() && !path.is_empty() && !path.starts_with('/') {
                path = format!("/{path}");
            }
            shell_launch_config.cwd = Some(ShellCwd::Uri(URI::from(
                &UriComponents { scheme: Some(parent_uri.scheme().clone()), authority: Some(parent_uri.authority().clone()), path: Some(path.as_str().into()), ..UriComponents::default() },
                false,
            )));
        }
        if location == TerminalLocation::Editor || parent.target() == Some(TerminalLocation::Editor) {
            return self.terminal_editor_service.split_instance(parent, shell_launch_config);
        }
        shell_launch_config.parent_terminal_id = Some(parent.instance_id());
        match self.terminal_group_service.split_instance(parent.clone(), shell_launch_config) {
            Some(instance) => Promise::resolved(instance),
            None => Promise::rejected(Error::new(format!(
                "Cannot split a terminal without a group (instanceId: {}, title: {})",
                parent.instance_id(),
                parent.title()
            ))),
        }
    }

    /// `_createTerminal`.
    fn create_terminal_in_location(&self, shell_launch_config: IShellLaunchConfig, location: TerminalLocation, options: &ICreateTerminalOptions) -> Rc<dyn ITerminalInstance> {
        if location == TerminalLocation::Editor {
            let hide_from_user = shell_launch_config.hide_from_user.unwrap_or(false);
            let instance = self.terminal_instance_service.create_instance(shell_launch_config, TerminalLocation::Editor);
            if !hide_from_user {
                self.terminal_editor_service.open_editor(instance.clone(), Self::get_editor_options(options.location.as_ref()));
            }
            instance
        } else {
            // O original não repassa o `resource` ao grupo.
            self.terminal_group_service.create_group(shell_launch_config)
        }
    }

    /// `createTerminal`.
    async fn create_terminal_task(self: Rc<Self>, options: ICreateTerminalOptions) -> Result<Rc<dyn ITerminalInstance>, Error> {
        // Await the initialization of available profiles (recortado, ver o cabeçalho) as long as this is not a pty terminal or a local
        // terminal in a remote workspace.
        let is_local_in_remote_terminal = self.remote_agent_service.get_connection().is_some()
            && matches!(&options.cwd, Some(ShellCwd::Uri(uri)) if uri.scheme().to_string_lossy() == "file");

        let mut config = options.config.clone();
        if config.is_none() && is_local_in_remote_terminal {
            let backend = self.terminal_instance_service.get_backend(None).await?;
            let executable = match backend {
                Some(backend) => Some(backend.get_default_system_shell(None).await?),
                None => None,
            };
            if let Some(executable) = executable.filter(|executable| !executable.is_empty()) {
                config = Some(super::terminal::ShellLaunchConfigOrProfile::LaunchConfig(IShellLaunchConfig { executable: Some(executable), ..IShellLaunchConfig::default() }));
            }
        }

        let mut shell_launch_config = self.terminal_instance_service.convert_profile_to_shell_launch_config(config, None);

        let split_active_terminal = match &options.location {
            Some(TerminalCreateLocation::SplitActiveTerminal(split)) => *split,
            Some(TerminalCreateLocation::ParentTerminal(_)) => true,
            _ => false,
        };

        self.resolve_cwd(&mut shell_launch_config, split_active_terminal, &options)?;

        if shell_launch_config.custom_pty_implementation.is_none() && !self.is_process_support_registered() {
            // Sem `ITerminalProfileService` (recortado) não há perfil contribuído para o laço de reserva.
            return Err(Error::new("Could not create terminal when process support is not registered"));
        }

        self.evaluate_local_cwd(&mut shell_launch_config);
        let location = self.resolve_location(options.location.as_ref()).unwrap_or_else(|| self.terminal_configuration_service.default_location());

        if shell_launch_config.hide_from_user.unwrap_or(false) {
            let instance = self.terminal_instance_service.create_instance(shell_launch_config, location);
            self.backgrounded_terminal_instances.borrow_mut().push(BackgroundTerminal { instance: instance.clone(), terminal_location_options: options.location.clone() });
            let weak = self.this.clone();
            let listener = instance.on_disposed().listen(move |disposed| {
                if let Some(this) = weak.upgrade() {
                    this.on_background_terminal_disposed(disposed);
                }
            });
            self.backgrounded_terminal_disposables.set(instance.instance_id(), listener, false);
            self.on_did_change_instances.fire(());
            return Ok(instance);
        }

        let parent = self.get_split_parent(options.location.as_ref());
        self.terminal_has_been_created.set(Dynamic::Bool(true));
        let _ = self.extension_service.activate_by_event("onTerminal:*", None);
        let instance = match parent {
            Some(parent) => self.split_terminal(shell_launch_config, location, parent).await?,
            None => self.create_terminal_in_location(shell_launch_config, location, &options),
        };
        if instance.shell_type().is_some() {
            self.activate_by_shell_type_event(&instance, "onTerminal");
        }

        Ok(instance)
    }

    /// `_onBackgroundTerminalDisposed`.
    fn on_background_terminal_disposed(&self, instance: &Rc<dyn ITerminalInstance>) {
        {
            let mut backgrounded_terminal_instances = self.backgrounded_terminal_instances.borrow_mut();
            if let Some(index) = backgrounded_terminal_instances.iter().position(|backgrounded| Rc::ptr_eq(&backgrounded.instance, instance)) {
                backgrounded_terminal_instances.remove(index);
            }
        }
        self.backgrounded_terminal_disposables.delete_and_dispose(&instance.instance_id());
        self.on_did_dispose_instance.fire(instance.clone());
    }

    /// `showBackgroundTerminal`.
    pub fn show_background_terminal(&self, instance: &Rc<dyn ITerminalInstance>, suppress_set_active: bool) {
        let background_terminal = {
            let mut backgrounded_terminal_instances = self.backgrounded_terminal_instances.borrow_mut();
            let Some(index) = backgrounded_terminal_instances.iter().position(|backgrounded| Rc::ptr_eq(&backgrounded.instance, instance)) else { return };
            backgrounded_terminal_instances.remove(index)
        };
        self.backgrounded_terminal_disposables.delete_and_dispose(&instance.instance_id());
        if instance.target() == Some(TerminalLocation::Panel) {
            self.terminal_group_service.create_group_for_instance(instance.clone());

            // Make active automatically if it's the first instance
            if self.instances().len() == 1 && !suppress_set_active {
                self.terminal_group_service.set_active_instance_by_index(0);
            }
        } else {
            let editor_options = Self::get_editor_options(background_terminal.terminal_location_options.as_ref());
            self.terminal_editor_service.open_editor(instance.clone(), editor_options);
        }

        self.on_did_change_instances.fire(());
    }
}

impl ITerminalService for TerminalService {
    fn instances(&self) -> Vec<Rc<dyn ITerminalInstance>> {
        let mut instances = self.foreground_instances();
        instances.extend(self.backgrounded_terminal_instances.borrow().iter().map(|background_terminal| background_terminal.instance.clone()));
        instances
    }

    fn active_instance(&self) -> Option<Rc<dyn ITerminalInstance>> {
        // Check if either an editor or panel terminal has focus and return that, regardless of the value of _activeInstance. This
        // avoids terminals created in the panel for example stealing the active status even when it's not focused.
        let host_active_terminals = self.host_active_terminals.borrow().clone();
        if let Some(focused) = host_active_terminals.into_iter().flatten().find(|active_host_terminal| active_host_terminal.has_focus()) {
            return Some(focused);
        }
        // Fallback to the last recorded active terminal if neither have focus
        self.active_instance.borrow().clone()
    }

    fn on_did_create_instance(&self) -> Event<Rc<dyn ITerminalInstance>> {
        self.on_did_create_instance.event()
    }

    fn on_did_dispose_instance(&self) -> Event<Rc<dyn ITerminalInstance>> {
        self.on_did_dispose_instance.event()
    }

    fn on_any_instance_process_id_ready(&self) -> Event<Rc<dyn ITerminalInstance>> {
        self.on_any_instance_process_id_ready.get(|| self.any_instance_event(|instance| instance.on_process_id_ready())).clone()
    }

    fn on_did_change_instance_dimensions(&self) -> Event<Rc<dyn ITerminalInstance>> {
        self.on_did_change_instance_dimensions.event()
    }

    fn on_any_instance_maximum_dimensions_change(&self) -> Event<Rc<dyn ITerminalInstance>> {
        self.on_any_instance_maximum_dimensions_change
            .get(|| {
                self.any_instance_event(|instance| {
                    let instance = instance.clone();
                    instance.on_maximum_dimensions_changed().map(move |_| instance.clone(), None)
                })
            })
            .clone()
    }

    fn on_did_request_start_extension_terminal(&self) -> Event<IStartExtensionTerminalRequest> {
        self.on_did_request_start_extension_terminal.event()
    }

    fn on_did_change_active_instance(&self) -> Event<Option<Rc<dyn ITerminalInstance>>> {
        self.on_did_change_active_instance.event()
    }

    fn on_any_instance_title_change(&self) -> Event<Rc<dyn ITerminalInstance>> {
        self.on_any_instance_title_change.get(|| self.any_instance_event(|instance| instance.on_title_changed())).clone()
    }

    fn on_any_instance_data_input(&self) -> Event<Rc<dyn ITerminalInstance>> {
        self.on_any_instance_data_input
            .get(|| {
                self.any_instance_event(|instance| {
                    let instance = instance.clone();
                    instance.on_did_input_data().map(move |_| instance.clone(), None)
                })
            })
            .clone()
    }

    fn on_any_instance_shell_type_changed(&self) -> Event<Rc<dyn ITerminalInstance>> {
        self.on_any_instance_shell_type_changed
            .get(|| {
                self.any_instance_event(|instance| {
                    let instance = instance.clone();
                    instance.on_did_change_shell_type().map(move |_| instance.clone(), None)
                })
            })
            .clone()
    }

    fn register_process_support(&self, is_supported: bool) {
        if !is_supported {
            return;
        }
        self.process_support_context_key.set(Dynamic::Bool(is_supported));
        self.on_did_register_process_support.fire(());
    }

    fn set_active_instance(&self, instance: Rc<dyn ITerminalInstance>) {
        // If this was a hideFromUser terminal created by the API this was triggered by show, in which case we need to create the
        // terminal group
        if instance.shell_launch_config().borrow().hide_from_user.unwrap_or(false) {
            self.show_background_terminal(&instance, false);
        }
        if instance.target() == Some(TerminalLocation::Editor) {
            self.terminal_editor_service.set_active_instance(instance);
        } else {
            self.terminal_group_service.set_active_instance(instance);
        }
    }

    fn create_terminal(&self, options: Option<ICreateTerminalOptions>) -> Promise<Rc<dyn ITerminalInstance>> {
        let this = self.rc();
        run_async(async move { this.create_terminal_task(options.unwrap_or_default()).await })
    }

    fn request_start_extension_terminal(&self, proxy: Rc<dyn ITerminalProcessExtHostProxy>, cols: i32, rows: i32) -> Promise<Option<ITerminalLaunchError>> {
        // The initial request came from the extension host, no need to wait for it
        let (promise, resolver) = Promise::new();
        let callback: Rc<dyn Fn(Option<ITerminalLaunchError>)> = Rc::new(move |launch_error| resolver.resolve(launch_error));
        self.on_did_request_start_extension_terminal.fire(IStartExtensionTerminalRequest { proxy, cols, rows, callback });
        promise
    }

    fn get_instance_from_id(&self, terminal_id: u32) -> Option<Rc<dyn ITerminalInstance>> {
        let backgrounded = self.backgrounded_terminal_instances.borrow().iter().rev().find(|bg| bg.instance.instance_id() == terminal_id).map(|bg| bg.instance.clone());
        backgrounded.or_else(|| self.instances().into_iter().rev().find(|instance| instance.instance_id() == terminal_id))
    }

    fn set_next_command_id(&self, id: u32, command_line: &str, command_id: &str) -> Promise<()> {
        self.set_next_command_id_task(id, command_line, command_id)
    }
}

impl IDisposable for TerminalService {
    fn dispose(&self) {
        self.store.dispose();
    }
}

service_impl!(TerminalService => dyn ITerminalService, disposable);

impl Injectable for TerminalService {
    type Args = ();
    type Deps = (
        Rc<dyn IContextKeyService>,
        Rc<dyn ITerminalLogService>,
        Rc<dyn IDialogService>,
        Rc<dyn IRemoteAgentService>,
        Rc<dyn IWorkbenchEnvironmentService>,
        Rc<dyn ITerminalConfigurationService>,
        Rc<dyn ITerminalEditorService>,
        Rc<dyn ITerminalGroupService>,
        Rc<dyn ITerminalInstanceService>,
        Rc<dyn IExtensionService>,
    );

    fn create(
        _: (),
        (
            context_key_service,
            log_service,
            dialog_service,
            remote_agent_service,
            environment_service,
            terminal_configuration_service,
            terminal_editor_service,
            terminal_group_service,
            terminal_instance_service,
            extension_service,
        ): Self::Deps,
    ) -> Rc<Self> {
        let store = DisposableStore::new();
        let backgrounded_terminal_disposables = store.add_rc(DisposableMap::<u32, dyn IDisposable>::new());
        let (when_connected, when_connected_resolver) = Promise::new();

        let terminal_shell_type_context_key = terminal_context_key::shell_type_bind_to(&*context_key_service);
        let process_support_context_key = terminal_context_key::process_supported_bind_to(&*context_key_service);
        let terminal_has_been_created = terminal_context_key::terminal_has_been_created_bind_to(&*context_key_service);
        let terminal_count_context_key = terminal_context_key::count_bind_to(&*context_key_service);

        let this = Rc::new_cyclic(|weak: &Weak<Self>| Self {
            this: weak.clone(),
            terminal_shell_type_context_key,
            process_support_context_key,
            terminal_has_been_created,
            terminal_count_context_key,
            host_active_terminals: RefCell::new(Vec::new()),
            backgrounded_terminal_instances: RefCell::new(Vec::new()),
            backgrounded_terminal_disposables,
            primary_backend: RefCell::new(None),
            connection_state: Cell::new(TerminalConnectionState::Connecting),
            when_connected,
            when_connected_resolver,
            active_instance: RefCell::new(None),
            update_title_debounce: Debounce::without_reducer(500, {
                let weak = weak.clone();
                move |instance: Option<Rc<dyn ITerminalInstance>>| {
                    if let Some(this) = weak.upgrade() {
                        this.do_update_title(instance);
                    }
                }
            }),
            update_icon_debounce: Debounce::without_reducer(500, {
                let weak = weak.clone();
                move |(instance, user_initiated): (Rc<dyn ITerminalInstance>, bool)| {
                    if let Some(this) = weak.upgrade() {
                        this.do_update_icon(instance, user_initiated);
                    }
                }
            }),
            on_did_create_instance: store.add_rc(Emitter::new()),
            on_did_change_instance_dimensions: store.add_rc(Emitter::new()),
            on_did_register_process_support: store.add_rc(Emitter::new()),
            on_did_change_connection_state: store.add_rc(Emitter::new()),
            on_did_request_start_extension_terminal: store.add_rc(Emitter::new()),
            on_did_dispose_instance: store.add_rc(Emitter::new()),
            on_did_focus_instance: store.add_rc(Emitter::new()),
            on_did_change_active_instance: store.add_rc(Emitter::new()),
            on_did_change_instances: store.add_rc(Emitter::new()),
            on_did_change_instance_capability: store.add_rc(Emitter::new()),
            on_did_change_active_group: store.add_rc(Emitter::new()),
            on_any_instance_data: Memoize::new(),
            on_any_instance_data_input: Memoize::new(),
            on_any_instance_icon_change: Memoize::new(),
            on_any_instance_maximum_dimensions_change: Memoize::new(),
            on_any_instance_process_id_ready: Memoize::new(),
            on_any_instance_title_change: Memoize::new(),
            on_any_instance_shell_type_changed: Memoize::new(),
            context_key_service,
            log_service,
            dialog_service,
            remote_agent_service,
            environment_service,
            terminal_configuration_service,
            terminal_editor_service,
            terminal_group_service,
            terminal_instance_service,
            extension_service,
            store,
        });

        Self::init(&this);
        this
    }
}
