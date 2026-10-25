//! Porte parcial de `src/vs/workbench/contrib/terminal/browser/terminal.ts`: `ITerminalConfigurationService`, `LinuxDistro`
//! (mais os tipos que o serviço recebe: o contêiner do painel e o núcleo do xterm), o `ITerminalInstanceService`, e as
//! versões reduzidas de `ITerminalService` e `ITerminalInstance` que o resto do porte já lê.
//!
//! O resto do arquivo (`ITerminalGroupService`, `ITerminalEditorService`, `IXtermTerminal`...) entra quando um consumidor
//! pedir.
//!
//! Divergências:
//! - **`config`.** `Readonly<ITerminalConfiguration>` é um [`Dynamic`] (objeto): a configuração tem cerca de 120
//!   propriedades e o serviço só precisa delas como o `getValue` as devolve. Cada leitura devolve o mesmo objeto
//!   (a identidade muda quando o serviço recria a configuração, o que `notStrictEqual` observa, ver [`Dynamic::identity`]).
//! - **`setPanelContainer(HTMLElement)`.** Não há DOM: o contêiner é um [`ITerminalPanelContainer`], que sabe medir a caixa
//!   de um texto numa fonte (o que o elemento `div` de medição do original faz com `getBoundingClientRect`).
//! - **`getFont(w: Window, ...)`.** A janela só serve para o `devicePixelRatio`, que é um número aqui.
//! - **`xtermCore?: IXtermCore`.** O núcleo privado do xterm vira [`IXtermCore`], que devolve as dimensões em CSS de uma
//!   célula quando o renderizador existe (`_renderService._renderer.value` definido).
//! - **`ITerminalService`** tem o que o gerenciador de processo, a informação de variáveis de ambiente e as pontes
//!   `MainThreadTerminalService` e `MainThreadTerminalShellIntegration` leem: `instances`, `activeInstance`, os eventos
//!   `onDidCreateInstance`, `onDidDisposeInstance`, `onAnyInstanceProcessIdReady`, `onDidChangeInstanceDimensions`,
//!   `onAnyInstanceMaximumDimensionsChange`, `onDidRequestStartExtensionTerminal`, `onDidChangeActiveInstance`,
//!   `onAnyInstanceTitleChange`, `onAnyInstanceDataInput` e `onAnyInstanceShellTypeChanged`, `registerProcessSupport`,
//!   `setActiveInstance`, `createTerminal`, `requestStartExtensionTerminal`, `getInstanceFromId` e `setNextCommandId`.
//!   `createOnInstanceEvent<T>` e `createOnInstanceCapabilityEvent<T, K>` (genéricos, fora de um trait de objeto) são o
//!   [`ITerminalServiceEvents`](super::terminal_events::ITerminalServiceEvents), um trait de extensão implementado para todo
//!   serviço, que monta o multiplexador com `instances`, `onDidCreateInstance` e `onDidDisposeInstance`, como o
//!   `TerminalService` do original. `onAnyInstanceSelectionChange` e `ITerminalInstance#selection` ficam de fora: o contrato do
//!   emulador do porte não tem seleção (ver `terminal_instance.rs`).
//!   **`ITerminalInstance`** tem o que esses consumidores leem (o id, o título, o relançamento, o estado de descarte, os
//!   eventos de descarte, as capacidades, o tipo de shell, o local, o id e a prontidão do processo, as dimensões, o código e o
//!   motivo de saída, os dados iniciais, `onData`, `runCommand`, `sendText`, `rename` e `dispose(reason)`); a classe
//!   [`TerminalInstance`](super::terminal_instance::TerminalInstance) tem o resto como métodos próprios.
//! - **`ICreateTerminalOptions`** tem `config` (`IShellLaunchConfig | ITerminalProfile`, sem o `IExtensionTerminalProfile`),
//!   `cwd`, `resource`, `location` e `skipContributedProfileCheck`. O `location` é o [`TerminalCreateLocation`].
//! - **`ITerminalGroupService`** e **`ITerminalEditorService`** têm o que o `TerminalService`, o `MainThreadTerminalService` e as
//!   pontes leem (o painel, o host de instâncias, a ativação, a criação de grupos e `getGroupForInstance` no primeiro; o
//!   `revealActiveEditor` e o host no segundo). **`ITerminalGroup`** é o conjunto de instâncias de um grupo, sem o contêiner de
//!   painéis (ver `terminal_group.rs`).
//! - **`ITerminalLink`** e **`ITerminalExternalLinkProvider`** são os do `MainThreadTerminalService` (o `activate` é um `Rc<dyn Fn()>`).
//! - **`ITerminalInstanceService.createInstance(launchConfig, target, editorOptions?)`** não leva o `editorOptions`
//!   (`TerminalEditorLocation`, do serviço de editores de terminal, que não existe no porte).
//! - **`convertProfileToShellLaunchConfig(IShellLaunchConfig | ITerminalProfile | undefined, cwd?)`** recebe o
//!   [`ShellLaunchConfigOrProfile`].
//! - **`getRegisteredBackends(): IterableIterator`** é um `Vec`.

use std::rc::Rc;

use je_base::dynamic::Dynamic;
use je_base::event::Event;
use je_base::lifecycle::DisposableRef;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_instantiation::service_id;
use je_platform::terminal::capabilities::capabilities::ITerminalCapabilityStore;
use je_platform::terminal::terminal::{
    IShellLaunchConfig, ITerminalLaunchError, ITerminalProfile, ShellCwd, ShellIntegrationInjectionFailureReason, TerminalExitReason, TerminalIcon, TerminalLocation,
    TerminalShellType, TitleEventSource,
};

use super::terminal_instance::IconChangedEvent;
use crate::contrib::terminal::common::terminal::{IStartExtensionTerminalRequest, ITerminalProcessExtHostProxy, SharedShellLaunchConfig, TerminalFont};
use crate::contrib::terminal::common::terminal_backend::ITerminalBackend;

/// `LinuxDistro`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinuxDistro {
    Unknown = 1,
    Fedora = 2,
    Ubuntu = 3,
}

/// A caixa de um texto medido (`DOMRect`), em pixels CSS.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClientRect {
    pub width: f64,
    pub height: f64,
}

/// O elemento do painel (`HTMLElement`) onde o serviço cria o elemento de medição de caractere.
pub trait ITerminalPanelContainer {
    /// `_getBoundingRectFor`: mede `text` na família e no tamanho dados (o elemento `inline-block` com `lineHeight: normal`).
    /// O elemento de medição é criado se ainda não existe ou se o pai o descartou.
    fn measure_text(&self, text: &str, font_family: &str, font_size: f64) -> ClientRect;

    /// `this._charMeasureElement?.remove()`: o serviço descarta o elemento de medição ao ser descartado.
    fn remove_char_measure_element(&self);
}

/// O que o serviço lê do `IXtermCore` privado.
pub trait IXtermCore {
    /// `_renderService.dimensions.css.cell` quando `_renderService._renderer.value` existe, e `None` sem renderizador.
    fn css_cell_dimensions(&self) -> Option<ClientRect>;
}

/// `ITerminalConfigurationService`: acesso à configuração do terminal e aos valores derivados dela.
pub trait ITerminalConfigurationService {
    /// A representação tipada e parcialmente validada da configuração do terminal.
    fn config(&self) -> Dynamic;

    /// O local padrão dos terminais.
    fn default_location(&self) -> TerminalLocation;

    /// Dispara quando algo na configuração do terminal muda.
    fn on_config_changed(&self) -> Event<()>;

    fn set_panel_container(&self, panel_container: std::rc::Rc<dyn ITerminalPanelContainer>);
    fn config_font_is_monospace(&self) -> bool;
    fn get_font(&self, device_pixel_ratio: f64, xterm_core: Option<&dyn IXtermCore>, exclude_dimensions: bool) -> TerminalFont;

    /// Se um comando deve pular o shell e ser tratado como um atalho comum.
    fn should_command_skip_shell(&self, command_id: &str) -> bool;
}

service_id!(pub I_TERMINAL_CONFIGURATION_SERVICE: dyn ITerminalConfigurationService = "terminalConfigurationService");

/// `ITerminalInstance`, reduzido ao que o gerenciador de processo, a informação de variáveis de ambiente e as pontes
/// `MainThreadTerminalService` e `MainThreadTerminalShellIntegration` leem (ver "Divergências").
pub trait ITerminalInstance {
    fn instance_id(&self) -> u32;
    fn title(&self) -> String;
    fn is_disposed(&self) -> bool;
    fn shell_launch_config(&self) -> SharedShellLaunchConfig;

    /// Fired when the terminal instance is about to be disposed (the terminal is still alive for the listeners).
    fn on_will_dispose(&self) -> Event<Rc<dyn ITerminalInstance>>;
    /// Fired after the terminal instance (and its xterm) has been disposed.
    fn on_disposed(&self) -> Event<Rc<dyn ITerminalInstance>>;

    /// Relaunches the process with the same launch config (debounced by one second, as in the original).
    fn relaunch(&self);

    /// `capabilities`.
    fn capabilities(&self) -> Rc<dyn ITerminalCapabilityStore>;
    /// `shellType`.
    fn shell_type(&self) -> TerminalShellType;
    /// `target`.
    fn target(&self) -> Option<TerminalLocation>;
    /// `processId`: o `-1` de um terminal de extensão cabe aqui.
    fn process_id(&self) -> Option<i32>;
    /// `processReady`.
    fn process_ready(&self) -> Promise<()>;
    /// `cols`, `rows`, `maxCols` e `maxRows`.
    fn cols(&self) -> i32;
    fn rows(&self) -> i32;
    fn max_cols(&self) -> i32;
    fn max_rows(&self) -> i32;
    /// `exitCode` e `exitReason`.
    fn exit_code(&self) -> Option<i32>;
    fn exit_reason(&self) -> Option<TerminalExitReason>;
    /// `initialDataEvents`.
    fn initial_data_events(&self) -> Option<Vec<String>>;
    /// `onData`.
    fn on_data(&self) -> Event<String>;
    /// `runCommand(commandLine, shouldExecute)` (sem os argumentos opcionais).
    fn run_command(&self, command_line: &str, should_execute: bool) -> Promise<()>;
    /// `sendText(text, shouldExecute)` (sem o `forceBracketedPasteMode`).
    fn send_text(&self, text: &str, should_execute: bool) -> Promise<()>;
    /// `rename(title?)`.
    fn rename(&self, title: Option<&str>);
    /// `dispose(reason?)`.
    fn dispose_with_reason(&self, reason: Option<TerminalExitReason>);

    // Os membros que as contribuições do terminal e a telemetria leem. O padrão é o de um `mock<ITerminalInstance>()` (o membro
    // não definido lança), menos os de seleção e foco, que o frontend nativo (dono do foco e da seleção) sobrescreve.

    /// `resource`.
    fn resource(&self) -> URI {
        je_base::errors::throw(je_base::errors::Error::new("resource is not a function"))
    }
    /// `hasRemoteAuthority`.
    fn has_remote_authority(&self) -> bool {
        je_base::errors::throw(je_base::errors::Error::new("hasRemoteAuthority is not a function"))
    }
    /// `usedShellIntegrationInjection`.
    fn used_shell_integration_injection(&self) -> bool {
        je_base::errors::throw(je_base::errors::Error::new("usedShellIntegrationInjection is not a function"))
    }
    /// `shellIntegrationInjectionFailureReason`.
    fn shell_integration_injection_failure_reason(&self) -> Option<ShellIntegrationInjectionFailureReason> {
        je_base::errors::throw(je_base::errors::Error::new("shellIntegrationInjectionFailureReason is not a function"))
    }
    /// `sessionId`.
    fn session_id(&self) -> String {
        je_base::errors::throw(je_base::errors::Error::new("sessionId is not a function"))
    }
    /// `xterm?.isImageAddonLoaded ?? false`.
    fn is_image_addon_loaded(&self) -> bool {
        false
    }
    /// `focus()`: quem tem o foco é o frontend nativo.
    fn focus(&self) {}
    /// `hasSelection()`: o contrato do emulador do porte não tem seleção.
    fn has_selection(&self) -> bool {
        false
    }
    /// `clearSelection()`.
    fn clear_selection(&self) {}

    // Os membros que o `TerminalService` lê (o `TerminalService` do original os lê do `ITerminalInstance`). O padrão é o de um
    // `mock<ITerminalInstance>()` (o membro não definido lança); a classe [`TerminalInstance`](super::terminal_instance::TerminalInstance)
    // sobrescreve os que ela tem. `onMaximumDimensionsChanged`, `onDidFocus` e `hasFocus` valem "nunca dispara" e "sem foco" por
    // padrão: a classe do porte não os tem (o dimensionamento e o foco são do frontend nativo).

    /// `hasChildProcesses`.
    fn has_child_processes(&self) -> bool {
        je_base::errors::throw(je_base::errors::Error::new("hasChildProcesses is not a function"))
    }
    /// `persistentProcessId`.
    fn persistent_process_id(&self) -> Option<u32> {
        je_base::errors::throw(je_base::errors::Error::new("persistentProcessId is not a function"))
    }
    /// `titleSource`.
    fn title_source(&self) -> TitleEventSource {
        je_base::errors::throw(je_base::errors::Error::new("titleSource is not a function"))
    }
    /// `staticTitle`.
    fn static_title(&self) -> Option<String> {
        je_base::errors::throw(je_base::errors::Error::new("staticTitle is not a function"))
    }
    /// `icon`.
    fn icon(&self) -> Option<TerminalIcon> {
        je_base::errors::throw(je_base::errors::Error::new("icon is not a function"))
    }
    /// `color`.
    fn color(&self) -> Option<String> {
        je_base::errors::throw(je_base::errors::Error::new("color is not a function"))
    }
    /// `onExit`, sem a carga (o código ou o erro de saída): o `TerminalService` só espera o evento.
    fn on_exit_signal(&self) -> Event<()> {
        je_base::errors::throw(je_base::errors::Error::new("onExit is not a function"))
    }
    /// `onTitleChanged`.
    fn on_title_changed(&self) -> Event<Rc<dyn ITerminalInstance>> {
        je_base::errors::throw(je_base::errors::Error::new("onTitleChanged is not a function"))
    }
    /// `onIconChanged`.
    fn on_icon_changed(&self) -> Event<IconChangedEvent> {
        je_base::errors::throw(je_base::errors::Error::new("onIconChanged is not a function"))
    }
    /// `onProcessIdReady`.
    fn on_process_id_ready(&self) -> Event<Rc<dyn ITerminalInstance>> {
        je_base::errors::throw(je_base::errors::Error::new("onProcessIdReady is not a function"))
    }
    /// `onDimensionsChanged`.
    fn on_dimensions_changed(&self) -> Event<()> {
        je_base::errors::throw(je_base::errors::Error::new("onDimensionsChanged is not a function"))
    }
    /// `onDidInputData`.
    fn on_did_input_data(&self) -> Event<String> {
        je_base::errors::throw(je_base::errors::Error::new("onDidInputData is not a function"))
    }
    /// `onDidChangeShellType`.
    fn on_did_change_shell_type(&self) -> Event<TerminalShellType> {
        je_base::errors::throw(je_base::errors::Error::new("onDidChangeShellType is not a function"))
    }
    /// `onMaximumDimensionsChanged`.
    fn on_maximum_dimensions_changed(&self) -> Event<()> {
        Event::none()
    }
    /// `onDidFocus`.
    fn on_did_focus(&self) -> Event<Rc<dyn ITerminalInstance>> {
        Event::none()
    }
    /// `hasFocus`.
    fn has_focus(&self) -> bool {
        false
    }
    /// `hadFocusOnExit`: o foco é do frontend nativo, então o padrão vale "sem foco ao sair".
    fn had_focus_on_exit(&self) -> bool {
        false
    }
}

/// `ITerminalInstanceHost`, reduzido ao que o `TerminalService` lê: o dono de um conjunto de instâncias (o serviço de grupos ou o
/// de editores) com os eventos que o `TerminalService` repassa.
pub trait ITerminalInstanceHost {
    /// `activeInstance`.
    fn active_instance(&self) -> Option<Rc<dyn ITerminalInstance>>;
    /// `instances`.
    fn instances(&self) -> Vec<Rc<dyn ITerminalInstance>>;
    /// `onDidChangeActiveInstance`.
    fn on_did_change_active_instance(&self) -> Event<Option<Rc<dyn ITerminalInstance>>>;
    /// `onDidDisposeInstance`.
    fn on_did_dispose_instance(&self) -> Event<Rc<dyn ITerminalInstance>>;
    /// `onDidFocusInstance`.
    fn on_did_focus_instance(&self) -> Event<Rc<dyn ITerminalInstance>>;
    /// `onDidChangeInstanceCapability`.
    fn on_did_change_instance_capability(&self) -> Event<Rc<dyn ITerminalInstance>>;
    /// `onDidChangeInstances`.
    fn on_did_change_instances(&self) -> Event<()>;
}

/// `ITerminalLink`.
#[derive(Clone)]
pub struct ITerminalLink {
    pub id: u32,
    pub start_index: f64,
    pub length: f64,
    pub label: Option<String>,
    pub activate: Rc<dyn Fn()>,
}

/// `ITerminalExternalLinkProvider`.
pub trait ITerminalExternalLinkProvider {
    fn provide_links(&self, instance: Rc<dyn ITerminalInstance>, line: &str) -> Promise<Option<Vec<ITerminalLink>>>;
}

/// `'before' | 'after'` de `moveInstance`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerminalMovePosition {
    Before,
    After,
}

/// `IShellLaunchConfig | ITerminalInstance`: o argumento de `TerminalGroup#addInstance` e de `createGroup`.
#[derive(Clone)]
pub enum TerminalGroupSource {
    LaunchConfig(IShellLaunchConfig),
    Instance(Rc<dyn ITerminalInstance>),
}

/// `ITerminalGroup`, reduzido ao que o `TerminalGroupService` e o `MainThreadTerminalService` leem: as instâncias, a instância
/// ativa, os eventos e a edição do conjunto. O contêiner de painéis (`attachToElement`, `layout`, `resizePane*`, `setVisible`,
/// `getLayoutInfo`, `onPanelOrientationChanged`) e o `title` são do frontend nativo e não fazem parte do porte.
pub trait ITerminalGroup {
    /// `terminalInstances`.
    fn terminal_instances(&self) -> Vec<Rc<dyn ITerminalInstance>>;
    /// `activeInstance`.
    fn active_instance(&self) -> Option<Rc<dyn ITerminalInstance>>;
    /// `hadFocusOnExit`.
    fn had_focus_on_exit(&self) -> bool;

    /// `onDidDisposeInstance`.
    fn on_did_dispose_instance(&self) -> Event<Rc<dyn ITerminalInstance>>;
    /// `onDidFocusInstance`.
    fn on_did_focus_instance(&self) -> Event<Rc<dyn ITerminalInstance>>;
    /// `onDidChangeInstanceCapability`.
    fn on_did_change_instance_capability(&self) -> Event<Rc<dyn ITerminalInstance>>;
    /// `onDisposed`.
    fn on_disposed(&self) -> Event<Rc<dyn ITerminalGroup>>;
    /// `onInstancesChanged`.
    fn on_instances_changed(&self) -> Event<()>;
    /// `onDidChangeActiveInstance`.
    fn on_did_change_active_instance(&self) -> Event<Option<Rc<dyn ITerminalInstance>>>;

    /// `addInstance(shellLaunchConfigOrInstance, parentTerminalId?)`.
    fn add_instance(&self, source: TerminalGroupSource, parent_terminal_id: Option<u32>);
    /// `removeInstance(instance)`.
    fn remove_instance(&self, instance: &Rc<dyn ITerminalInstance>);
    /// `moveInstance(instances, index, position)`.
    fn move_instance(&self, instances: &[Rc<dyn ITerminalInstance>], index: usize, position: TerminalMovePosition);
    /// `setActiveInstanceByIndex(index, force?)`.
    fn set_active_instance_by_index(&self, index: usize, force: bool);
    /// `focusPreviousPane()`.
    fn focus_previous_pane(&self);
    /// `focusNextPane()`.
    fn focus_next_pane(&self);
    /// `split(shellLaunchConfig)`.
    fn split(&self, shell_launch_config: IShellLaunchConfig) -> Rc<dyn ITerminalInstance>;
    /// `addDisposable(disposable)`.
    fn add_disposable(&self, disposable: DisposableRef);
}

/// `ITerminalGroupService`, reduzido ao que o `MainThreadTerminalService` e o `TerminalService` leem. Os membros do
/// `TerminalService` têm o padrão de um `mock<ITerminalGroupService>()` (o membro não definido lança).
pub trait ITerminalGroupService {
    /// `showPanel(focus?)`.
    fn show_panel(&self, focus: bool) -> Promise<()>;
    /// `hidePanel()`.
    fn hide_panel(&self);

    /// O `ITerminalInstanceHost` do serviço (`instances`, `activeInstance` e os eventos de instância).
    fn instance_host(&self) -> Rc<dyn ITerminalInstanceHost> {
        je_base::errors::throw(je_base::errors::Error::new("instanceHost is not a function"))
    }
    /// `onDidChangeActiveGroup`, sem a carga (o grupo ativo não faz parte do porte).
    fn on_did_change_active_group(&self) -> Event<()> {
        je_base::errors::throw(je_base::errors::Error::new("onDidChangeActiveGroup is not a function"))
    }
    /// `setActiveInstance(instance)`.
    fn set_active_instance(&self, _instance: Rc<dyn ITerminalInstance>) {
        je_base::errors::throw(je_base::errors::Error::new("setActiveInstance is not a function"))
    }
    /// `setActiveInstanceByIndex(index)`.
    fn set_active_instance_by_index(&self, _index: usize) {
        je_base::errors::throw(je_base::errors::Error::new("setActiveInstanceByIndex is not a function"))
    }
    /// `createGroup(shellLaunchConfig)` seguido de `group.terminalInstances[0]`: a instância do grupo novo.
    fn create_group(&self, _shell_launch_config: IShellLaunchConfig) -> Rc<dyn ITerminalInstance> {
        je_base::errors::throw(je_base::errors::Error::new("createGroup is not a function"))
    }
    /// `createGroup(instance)`.
    fn create_group_for_instance(&self, _instance: Rc<dyn ITerminalInstance>) {
        je_base::errors::throw(je_base::errors::Error::new("createGroup is not a function"))
    }
    /// `getGroupForInstance(parent)?.split(shellLaunchConfig)`: `None` quando a instância não tem grupo.
    fn split_instance(&self, _parent: Rc<dyn ITerminalInstance>, _shell_launch_config: IShellLaunchConfig) -> Option<Rc<dyn ITerminalInstance>> {
        je_base::errors::throw(je_base::errors::Error::new("getGroupForInstance is not a function"))
    }
    /// `getGroupForInstance(instance)`.
    fn get_group_for_instance(&self, _instance: &Rc<dyn ITerminalInstance>) -> Option<Rc<dyn ITerminalGroup>> {
        je_base::errors::throw(je_base::errors::Error::new("getGroupForInstance is not a function"))
    }
}

service_id!(pub I_TERMINAL_GROUP_SERVICE: dyn ITerminalGroupService = "terminalGroupService");

/// `ITerminalEditorService`, reduzido ao que o `MainThreadTerminalService` e o `TerminalService` leem. Os membros do
/// `TerminalService` têm o padrão de um `mock<ITerminalEditorService>()` (o membro não definido lança).
pub trait ITerminalEditorService {
    /// `revealActiveEditor(preserveFocus?)`.
    fn reveal_active_editor(&self, preserve_focus: bool) -> Promise<()>;

    /// `getInputFromResource(resource)`: a entrada de editor do terminal, ou o erro de um recurso que não é de um terminal de
    /// editor. O padrão é o de um `mock<ITerminalEditorService>()` (o membro não definido lança).
    fn get_input_from_resource(&self, _resource: &URI) -> Result<TerminalEditorInput, je_base::errors::Error> {
        Err(je_base::errors::Error::new("getInputFromResource is not a function"))
    }

    /// O `ITerminalInstanceHost` do serviço (`instances`, `activeInstance` e os eventos de instância).
    fn instance_host(&self) -> Rc<dyn ITerminalInstanceHost> {
        je_base::errors::throw(je_base::errors::Error::new("instanceHost is not a function"))
    }
    /// `setActiveInstance(instance)`.
    fn set_active_instance(&self, _instance: Rc<dyn ITerminalInstance>) {
        je_base::errors::throw(je_base::errors::Error::new("setActiveInstance is not a function"))
    }
    /// `openEditor(instance, editorOptions?)`: o `location` é o `TerminalEditorLocation` (`viewColumn` e `preserveFocus`), na
    /// forma [`TerminalCreateLocation::Editor`].
    fn open_editor(&self, _instance: Rc<dyn ITerminalInstance>, _location: Option<TerminalCreateLocation>) {
        je_base::errors::throw(je_base::errors::Error::new("openEditor is not a function"))
    }
    /// `splitInstance(parent, shellLaunchConfig)`.
    fn split_instance(&self, _parent: Rc<dyn ITerminalInstance>, _shell_launch_config: IShellLaunchConfig) -> Promise<Rc<dyn ITerminalInstance>> {
        je_base::errors::throw(je_base::errors::Error::new("splitInstance is not a function"))
    }
}

/// O que se lê da entrada de editor do terminal: `!!(input.group?.windowId && isAuxiliaryWindow(getWindowById(windowId, true).window))`,
/// isto é, se o editor está numa janela auxiliar (a janela é do frontend, que resolve o `windowId`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TerminalEditorInput {
    pub is_in_auxiliary_window: bool,
}

service_id!(pub I_TERMINAL_EDITOR_SERVICE: dyn ITerminalEditorService = "terminalEditorService");

/// O `location` de `ICreateTerminalOptions`: `TerminalLocation | TerminalEditorLocationOptions | { parentTerminal: ITerminalInstance } |
/// { splitActiveTerminal: boolean }`.
#[derive(Clone)]
pub enum TerminalCreateLocation {
    Location(TerminalLocation),
    /// `TerminalEditorLocationOptions`: `{ viewColumn, preserveFocus? }`.
    Editor { view_column: f64, preserve_focus: Option<bool> },
    ParentTerminal(Rc<dyn ITerminalInstance>),
    SplitActiveTerminal(bool),
}

/// `ICreateTerminalOptions`.
#[derive(Clone, Default)]
pub struct ICreateTerminalOptions {
    /// The shell launch config or profile to launch with, when not specified the default terminal profile will be used.
    pub config: Option<ShellLaunchConfigOrProfile>,
    /// The current working directory to launch the shell with, this will override the `cwd` property of `config`.
    pub cwd: Option<ShellCwd>,
    /// The terminal's resource, passed when the terminal has moved windows.
    pub resource: Option<URI>,
    /// The terminal's location (editor or panel).
    pub location: Option<TerminalCreateLocation>,
    /// This terminal will not wait for contributed profiles to resolve which means it will proceed to launch with the default
    /// profile.
    pub skip_contributed_profile_check: Option<bool>,
}

/// `ITerminalService`, reduzido ao que o gerenciador de processo, a informação de variáveis de ambiente e as pontes
/// `MainThreadTerminalService` e `MainThreadTerminalShellIntegration` leem (ver "Divergências").
pub trait ITerminalService {
    fn instances(&self) -> Vec<Rc<dyn ITerminalInstance>>;
    fn active_instance(&self) -> Option<Rc<dyn ITerminalInstance>>;

    fn on_did_create_instance(&self) -> Event<Rc<dyn ITerminalInstance>>;
    fn on_did_dispose_instance(&self) -> Event<Rc<dyn ITerminalInstance>>;
    fn on_any_instance_process_id_ready(&self) -> Event<Rc<dyn ITerminalInstance>>;
    fn on_did_change_instance_dimensions(&self) -> Event<Rc<dyn ITerminalInstance>>;
    fn on_any_instance_maximum_dimensions_change(&self) -> Event<Rc<dyn ITerminalInstance>>;
    fn on_did_request_start_extension_terminal(&self) -> Event<IStartExtensionTerminalRequest>;
    fn on_did_change_active_instance(&self) -> Event<Option<Rc<dyn ITerminalInstance>>>;
    fn on_any_instance_title_change(&self) -> Event<Rc<dyn ITerminalInstance>>;
    fn on_any_instance_data_input(&self) -> Event<Rc<dyn ITerminalInstance>>;
    fn on_any_instance_shell_type_changed(&self) -> Event<Rc<dyn ITerminalInstance>>;

    /// `registerProcessSupport(isSupported)`.
    fn register_process_support(&self, is_supported: bool);
    /// `setActiveInstance(instance)`.
    fn set_active_instance(&self, instance: Rc<dyn ITerminalInstance>);
    /// `createTerminal(options?)`.
    fn create_terminal(&self, options: Option<ICreateTerminalOptions>) -> Promise<Rc<dyn ITerminalInstance>>;
    /// `requestStartExtensionTerminal(proxy, cols, rows)`.
    fn request_start_extension_terminal(&self, proxy: Rc<dyn ITerminalProcessExtHostProxy>, cols: i32, rows: i32) -> Promise<Option<ITerminalLaunchError>>;

    fn get_instance_from_id(&self, terminal_id: u32) -> Option<Rc<dyn ITerminalInstance>>;
    fn set_next_command_id(&self, id: u32, command_line: &str, command_id: &str) -> Promise<()>;
}

service_id!(pub I_TERMINAL_SERVICE: dyn ITerminalService = "terminalService");

/// `IShellLaunchConfig | ITerminalProfile`: o argumento de `convertProfileToShellLaunchConfig`.
#[derive(Clone)]
pub enum ShellLaunchConfigOrProfile {
    LaunchConfig(IShellLaunchConfig),
    Profile(ITerminalProfile),
}

/// `ITerminalInstanceService`.
pub trait ITerminalInstanceService {
    /// An event that's fired when a terminal instance is created.
    fn on_did_create_instance(&self) -> Event<Rc<dyn ITerminalInstance>>;

    /// An event that's fired when a new backend is registered.
    fn on_did_register_backend(&self) -> Event<Rc<dyn ITerminalBackend>>;

    /// Helper function to convert a shell launch config, a profile or undefined into its equivalent shell launch config.
    /// `cwd` is a cwd to override.
    fn convert_profile_to_shell_launch_config(&self, shell_launch_config_or_profile: Option<ShellLaunchConfigOrProfile>, cwd: Option<ShellCwd>) -> IShellLaunchConfig;

    /// Create a new terminal instance.
    fn create_instance(&self, launch_config: IShellLaunchConfig, target: TerminalLocation) -> Rc<dyn ITerminalInstance>;

    /// Gets the registered backend for a remote authority (`None` = local). This is a convenience method to avoid using the
    /// more verbose fetching from the registry.
    fn get_backend(&self, remote_authority: Option<&str>) -> Promise<Option<Rc<dyn ITerminalBackend>>>;

    fn get_registered_backends(&self) -> Vec<Rc<dyn ITerminalBackend>>;
    fn did_register_backend(&self, backend: Rc<dyn ITerminalBackend>);
}

service_id!(pub I_TERMINAL_INSTANCE_SERVICE: dyn ITerminalInstanceService = "terminalInstanceService");
