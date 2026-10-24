//! Porte de `src/vs/workbench/api/common/extHostTerminalService.ts`: o lado da extensão dos terminais
//! (`ExtHostTerminal`, o pseudoterminal de extensão, as coleções de variáveis de ambiente, o `BaseExtHostTerminalService` e o
//! `WorkerExtHostTerminalService`).
//!
//! De fora: o `ExtHostTerminalService` de Node (`node/extHostTerminalService.ts`), que cria os terminais com um processo de
//! verdade e é a outra subclasse de `BaseExtHostTerminalService`.
//!
//! Divergências:
//! - **Tipos `vscode.*`.** Os tipos da API que o serviço lê ou produz são os structs deste módulo: [`VscodeTerminal`]
//!   (`vscode.Terminal`, com os getters que fecham sobre `that`), [`TerminalOptions`] e [`ExtensionTerminalOptions`] (com a união
//!   [`TerminalCreationOptions`]), [`TerminalExitStatus`], [`TerminalState`], [`TerminalLinkContext`], [`TerminalProfile`] e os
//!   traits dos provedores ([`TerminalLinkProvider`], [`TerminalProfileProvider`], [`TerminalQuickFixProvider`],
//!   [`TerminalCompletionProvider`]) e do [`Pseudoterminal`]. `ProviderResult<T>` é `MaybePromise<Option<T>>`; um método opcional do
//!   `Pseudoterminal` (`handleInput?`, `setDimensions?`, `onDidClose?`...) tem implementação padrão que não faz nada (ou devolve
//!   `None`, nos eventos), o que é o `?.` do original.
//! - **`vscode.Terminal` e o estado compartilhado.** `ExtHostTerminal.value` fecha sobre `that`; aqui o estado fica no
//!   [`TerminalCore`], compartilhado por `Rc` entre o [`ExtHostTerminal`] e o [`VscodeTerminal`] (sem ciclo). O `ExtHostTerminal`
//!   faz `Deref` para o núcleo. `Object.freeze(creationOptions)` e `Object.freeze({ code, reason })` não têm equivalente (os campos
//!   não são expostos para escrita). `TerminalExitStatus.reason` é o `TerminalExitReason` da API, e `TerminalState.shell` é o
//!   `TerminalShellType` da plataforma (a união de strings que o original guarda).
//! - **`TerminalLocation`.** Os locais da API (`TerminalLocation | TerminalEditorLocationOptions | TerminalSplitLocationOptions`) são
//!   o [`TerminalLocationOption`], com o `TerminalLocation` da plataforma (os valores são os mesmos da API).
//! - **Ícone e cor.** `iconPath` é o [`TerminalIconPath`]; `color` é um [`ThemeColor`] (o `ThemeColor.isThemeColor` do original só
//!   distingue de `undefined`). `asTerminalIcon` devolve o `TerminalIcon` da plataforma.
//! - **Funções `async`.** Cada função `async` do original roda de forma síncrona até o primeiro `await` e devolve uma [`Promise`]; as
//!   que não esperam nada devolvem a promessa já resolvida depois de fazer o efeito. Um `throw` síncrono da API (por exemplo
//!   `_checkDisposed`) é o `Err`; o de uma função `async` é a rejeição.
//! - **`IExtHostCommands` e `IExtHostInitDataService`.** São os traits mínimos [`IExtHostCommands`] (o `registerArgumentProcessor` e
//!   o `converter`) e [`IExtHostInitDataService`] (o `remote.authority`). O `IExtHostRpcService` é o [`IMainContext`], como nos outros
//!   arquivos `extHost*`.
//! - **Argumento `TerminalContext`.** O `vscode.Terminal` que o processador de argumentos devolve cruza como um [`Dynamic`] objeto cuja
//!   carga nativa (`ObjectRef::host`) é o [`VscodeTerminal`]; `$mid` e `instanceId` são lidos como propriedades.
//! - **Classe abstrata.** `BaseExtHostTerminalService` e as suas subclasses são o [`BaseExtHostTerminalService`] genérico sobre uma
//!   estratégia [`ExtHostTerminalCreation`] (`createTerminal` e `createTerminalFromOptions`, os dois métodos abstratos); o
//!   [`WorkerExtHostTerminalService`] é o serviço com a estratégia [`WorkerTerminalCreation`].
//! - **`ITerminalChildProcess`.** `_terminalProcesses` guarda um [`TerminalChildProcess`], que distingue o `ExtHostPseudoterminal` (o
//!   `instanceof` do original) de qualquer outro processo. O `pid: -1` de `onProcessReady` é o `i32` do `IProcessReadyEvent`.
//! - **`console.error`.** É `eprintln!`.
//! - **`getTerminalIdByApiObject`.** Devolve o índice do terminal na lista, como o original (que o chama de id).
//! - **`$provideTerminalQuickFixes`.** O `null` e o `undefined` de um resultado ausente são o mesmo `None`, que devolve `None` sem criar
//!   o `DisposableStore` dos comandos.
//! - **`TerminalCompletionResourceOptions`.** O `globPattern` que o original converte não existe no tipo da API portado.
//! - **Coleção de variáveis de ambiente.** O `ScopedEnvironmentVariableCollection` guarda um [`Weak`] da coleção unificada (que o
//!   serviço guarda enquanto vive). `description` aceita [`Documentation`] (`string | MarkdownString`), e a leitura devolve o texto.
//!   `EnvironmentVariableMutatorType` e o escopo são os da plataforma, que têm os mesmos valores e campos.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use je_base::arrays::OneOrMany;
use je_base::async_::{async_fn, await_maybe, promises};
use je_base::cancellation::{CancellationToken, CancellationTokenSource};
use je_base::dynamic::{Dynamic, ObjectRef};
use je_base::errors::{self, Error};
use je_base::event::{Emitter, EmitterOptions, Event};
use je_base::lifecycle::{Disposable, DisposableRef, DisposableStore, IDisposable, MutableDisposable, to_disposable};
use je_base::map::OrderedMap;
use je_base::marshalling_ids::MarshalledId;
use je_base::nls::localize;
use je_base::platform::Platform;
use je_base::promise::{MaybePromise, Promise, Resolver};
use je_base::themables::{ThemeColor, ThemeIcon};
use je_base::uri::URI;
use je_base::uuid::generate_uuid;
use je_extensions::scanner::ExtensionDescription;
use je_platform::terminal::environment_variable::{
    EnvironmentVariableMutatorType, EnvironmentVariableScope, IEnvironmentVariableCollectionDescription, IEnvironmentVariableMutator,
    IEnvironmentVariableMutatorOptions, ISerializableEnvironmentVariableCollection,
};
use je_platform::terminal::environment_variable_shared::{serialize_environment_description_map, serialize_environment_variable_collection};
use je_platform::terminal::terminal::{
    IProcessProperty, IProcessReadyEvent, ITerminalChildProcess, ITerminalDimensionsOverride, ITerminalEnvironment, ITerminalLaunchError,
    ITerminalLaunchOutcome, ITerminalProfile, LightDarkIcon, ProcessData, ProcessPropertyType, ShellCwd, ShellType, TerminalExitReason as PlatformExitReason,
    TerminalIcon, TerminalLocation, TerminalShellType, UnicodeVersion, UriLike, WindowsShellType,
};
use je_platform::terminal::terminal_data_buffering::TerminalDataBufferer;

use super::ext_host_commands::{ArgumentProcessor, ExtHostCommands};
use super::ext_host_protocol::{
    ExtHostTerminalIdentifier, ExtHostTerminalServiceShape, IMainContext, ICreateContributedTerminalProfileOptions, ITerminalCommandDto,
    ITerminalCompletionContextDto, ITerminalCompletionItemDto, ITerminalDimensionsDto, ITerminalLinkDto, ITerminalQuickFixOpenerDto,
    ITerminalQuickFixTerminalCommandDto, IShellLaunchConfigDto, MainContext, MainThreadTerminalServiceShape, TerminalCommandMatchResultDto,
    TerminalCompletionListDto, TerminalCompletionResourceOptionsDto, TerminalLaunchConfig, TerminalLaunchLocation, TerminalProfileLocation, TerminalQuickFix,
};
use super::ext_host_terminal_shell_integration::TerminalShellIntegration;
use super::ext_host_type_converters::{ICommandsConverter, MarkdownString as MarkdownStringConverter, ViewColumn};
use super::ext_host_types::{Command, Disposable as VSCodeDisposable, Documentation, TerminalCompletionItem, TerminalCompletionList, TerminalExitReason, TerminalLink};
use crate::services::extensions::extensions::is_proposed_api_enabled;

// ---- os tipos da API `vscode.*`

/// `vscode.TerminalDimensions`.
pub type TerminalDimensions = ITerminalDimensionsDto;

/// `vscode.TerminalExitStatus`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TerminalExitStatus {
    pub code: Option<i32>,
    pub reason: TerminalExitReason,
}

/// `vscode.TerminalState`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TerminalState {
    pub is_interacted_with: bool,
    pub shell: TerminalShellType,
}

/// `Uri | { light: Uri; dark: Uri } | ThemeIcon`: o `iconPath` das opções de terminal.
#[derive(Clone, Debug, PartialEq)]
pub enum TerminalIconPath {
    Uri(URI),
    LightDark { light: URI, dark: URI },
    Theme(ThemeIcon),
}

/// `vscode.TerminalEditorLocationOptions`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TerminalEditorLocationOptions {
    pub view_column: f64,
    pub preserve_focus: Option<bool>,
}

/// `TerminalLocation | TerminalEditorLocationOptions | TerminalSplitLocationOptions`.
#[derive(Clone)]
pub enum TerminalLocationOption {
    Location(TerminalLocation),
    Editor(TerminalEditorLocationOptions),
    /// `TerminalSplitLocationOptions`: `{ parentTerminal: Terminal }`.
    Split { parent_terminal: Rc<VscodeTerminal> },
}

/// `vscode.TerminalOptions`.
#[derive(Clone, Default)]
pub struct TerminalOptions {
    pub name: Option<String>,
    pub shell_path: Option<String>,
    pub shell_args: Option<OneOrMany<String>>,
    pub cwd: Option<ShellCwd>,
    pub env: Option<ITerminalEnvironment>,
    pub icon_path: Option<TerminalIconPath>,
    pub color: Option<ThemeColor>,
    pub location: Option<TerminalLocationOption>,
    pub message: Option<String>,
    pub strict_env: Option<bool>,
    pub hide_from_user: Option<bool>,
    pub is_transient: Option<bool>,
    pub shell_integration_nonce: Option<String>,
    pub title_template: Option<String>,
}

/// `vscode.ExtensionTerminalOptions`.
#[derive(Clone)]
pub struct ExtensionTerminalOptions {
    pub name: String,
    pub pty: Rc<dyn Pseudoterminal>,
    pub icon_path: Option<TerminalIconPath>,
    pub color: Option<ThemeColor>,
    pub location: Option<TerminalLocationOption>,
    pub is_transient: Option<bool>,
    pub shell_integration_nonce: Option<String>,
    pub title_template: Option<String>,
}

/// `vscode.TerminalOptions | vscode.ExtensionTerminalOptions`.
#[derive(Clone)]
pub enum TerminalCreationOptions {
    Terminal(TerminalOptions),
    Extension(ExtensionTerminalOptions),
}

impl TerminalCreationOptions {
    /// `options.titleTemplate`.
    pub fn title_template(&self) -> Option<&str> {
        match self {
            Self::Terminal(options) => options.title_template.as_deref(),
            Self::Extension(options) => options.title_template.as_deref(),
        }
    }

    /// `{ ...options, titleTemplate }`.
    pub fn with_title_template(&self, title_template: Option<String>) -> Self {
        match self {
            Self::Terminal(options) => Self::Terminal(TerminalOptions { title_template, ..options.clone() }),
            Self::Extension(options) => Self::Extension(ExtensionTerminalOptions { title_template, ..options.clone() }),
        }
    }
}

/// `vscode.TerminalProfile`.
#[derive(Clone)]
pub struct TerminalProfile {
    pub options: TerminalCreationOptions,
}

/// `vscode.TerminalProfile | vscode.TerminalOptions | vscode.ExtensionTerminalOptions`: o que `provideTerminalProfile` devolve (o
/// provedor pode devolver as opções sem o envelope `{ options }`).
#[derive(Clone)]
pub enum TerminalProfileResult {
    Profile(TerminalProfile),
    Options(TerminalCreationOptions),
}

/// `vscode.TerminalLinkContext`.
#[derive(Clone)]
pub struct TerminalLinkContext {
    pub terminal: Rc<VscodeTerminal>,
    pub line: String,
}

/// `vscode.TerminalQuickFixTerminalCommand | vscode.TerminalQuickFixOpener | vscode.Command`.
#[derive(Clone, Debug)]
pub enum TerminalQuickFixItem {
    TerminalCommand { terminal_command: String, should_execute: Option<bool> },
    Opener { uri: URI },
    Command(Command),
}

/// `SingleOrMany<TerminalQuickFixItem>`.
#[derive(Clone, Debug)]
pub enum TerminalQuickFixResult {
    Single(TerminalQuickFixItem),
    Many(Vec<TerminalQuickFixItem>),
}

/// `vscode.TerminalCompletionItem[] | vscode.TerminalCompletionList`.
#[derive(Clone, Debug)]
pub enum TerminalCompletionResult {
    Items(Vec<TerminalCompletionItem>),
    List(TerminalCompletionList),
}

/// `vscode.TerminalLinkProvider`.
pub trait TerminalLinkProvider {
    fn provide_terminal_links(&self, context: &TerminalLinkContext, token: CancellationToken) -> MaybePromise<Option<Vec<TerminalLink>>>;
    fn handle_terminal_link(&self, link: &TerminalLink);
}

/// `vscode.TerminalProfileProvider`.
pub trait TerminalProfileProvider {
    fn provide_terminal_profile(&self, token: CancellationToken) -> MaybePromise<Option<TerminalProfileResult>>;
}

/// `vscode.TerminalQuickFixProvider`.
pub trait TerminalQuickFixProvider {
    fn provide_terminal_quick_fixes(&self, command_match_result: &TerminalCommandMatchResultDto, token: CancellationToken) -> MaybePromise<Option<TerminalQuickFixResult>>;
}

/// `vscode.TerminalCompletionProvider`.
pub trait TerminalCompletionProvider {
    fn provide_terminal_completions(
        &self,
        terminal: &Rc<VscodeTerminal>,
        context: &ITerminalCompletionContextDto,
        token: CancellationToken,
    ) -> MaybePromise<Option<TerminalCompletionResult>>;
}

/// `vscode.Pseudoterminal`.
pub trait Pseudoterminal {
    fn on_did_write(&self) -> Event<String>;
    fn on_did_override_dimensions(&self) -> Option<Event<Option<TerminalDimensions>>> {
        None
    }
    /// `onDidClose?: Event<number | void>`: o `void` é o `None`.
    fn on_did_close(&self) -> Option<Event<Option<i32>>> {
        None
    }
    fn on_did_change_name(&self) -> Option<Event<String>> {
        None
    }
    fn open(&self, initial_dimensions: Option<TerminalDimensions>);
    fn close(&self);
    fn handle_input(&self, _data: &str) {}
    fn set_dimensions(&self, _dimensions: TerminalDimensions) {}
}

/// `IExtHostCommands`, só o que o serviço usa.
pub trait IExtHostCommands {
    fn register_argument_processor(&self, processor: Rc<dyn ArgumentProcessor>);
    fn converter(&self) -> Rc<dyn ICommandsConverter>;
}

impl IExtHostCommands for ExtHostCommands {
    fn register_argument_processor(&self, processor: Rc<dyn ArgumentProcessor>) {
        ExtHostCommands::register_argument_processor(self, processor);
    }

    fn converter(&self) -> Rc<dyn ICommandsConverter> {
        ExtHostCommands::converter(self)
    }
}

/// `IExtHostInitDataService`, só o `remote.authority`.
pub trait IExtHostInitDataService {
    fn remote_authority(&self) -> Option<String>;
}

/// `ITerminalInternalOptions`.
#[derive(Clone, Debug, Default)]
pub struct ITerminalInternalOptions {
    pub cwd: Option<ShellCwd>,
    pub is_feature_terminal: Option<bool>,
    pub is_remote_resolver_terminal: Option<bool>,
    pub force_shell_integration: Option<bool>,
    pub use_shell_environment: Option<bool>,
    pub resolved_ext_host_identifier: Option<ExtHostTerminalIdentifier>,
    /// This location is different from the API location because it can include splitActiveTerminal, a property we resolve
    /// internally.
    pub location: Option<TerminalProfileLocation>,
}

/// O `ICreateContributedTerminalProfileOptions` usado como `ITerminalInternalOptions` (o original o passa direto).
impl From<&ICreateContributedTerminalProfileOptions> for ITerminalInternalOptions {
    fn from(options: &ICreateContributedTerminalProfileOptions) -> Self {
        Self { cwd: options.cwd.clone(), location: options.location, ..Self::default() }
    }
}

// ---- o terminal

/// O estado de um `ExtHostTerminal`, compartilhado com o [`VscodeTerminal`] (ver as divergências no topo do módulo).
pub struct TerminalCore {
    proxy: Rc<dyn MainThreadTerminalServiceShape>,
    id: RefCell<ExtHostTerminalIdentifier>,
    creation_options: TerminalCreationOptions,
    name: RefCell<Option<String>>,
    disposed: Cell<bool>,
    pid_promise: RefCell<Promise<Option<i32>>>,
    pid_promise_complete: RefCell<Option<Resolver<Option<i32>>>>,
    cols: Cell<Option<i32>>,
    rows: Cell<Option<i32>>,
    exit_status: RefCell<Option<TerminalExitStatus>>,
    state: Cell<TerminalState>,
    selection: RefCell<Option<String>>,
    shell_integration: RefCell<Option<Rc<TerminalShellIntegration>>>,
    is_open: Cell<bool>,
}

impl TerminalCore {
    /// `_id`.
    pub fn id(&self) -> ExtHostTerminalIdentifier {
        self.id.borrow().clone()
    }

    /// `_id = id`.
    pub fn set_id(&self, id: ExtHostTerminalIdentifier) {
        *self.id.borrow_mut() = id;
    }

    /// `isOpen`.
    pub fn is_open(&self) -> bool {
        self.is_open.get()
    }

    pub fn set_is_open(&self, is_open: bool) {
        self.is_open.set(is_open);
    }

    /// `shellIntegration`.
    pub fn shell_integration(&self) -> Option<Rc<TerminalShellIntegration>> {
        self.shell_integration.borrow().clone()
    }

    pub fn set_shell_integration(&self, shell_integration: Option<Rc<TerminalShellIntegration>>) {
        *self.shell_integration.borrow_mut() = shell_integration;
    }

    fn check_disposed(&self) -> Result<(), Error> {
        if self.disposed.get() {
            return Err(Error::new("Terminal has already been disposed"));
        }
        Ok(())
    }

    /// `set name(name)`.
    pub fn set_name(&self, name: &str) {
        *self.name.borrow_mut() = Some(name.to_owned());
    }

    /// `setExitStatus(code, reason)`.
    pub fn set_exit_status(&self, code: Option<i32>, reason: PlatformExitReason) {
        let reason = TerminalExitReason::from_value(reason as i32).unwrap_or(TerminalExitReason::Unknown);
        *self.exit_status.borrow_mut() = Some(TerminalExitStatus { code, reason });
    }

    /// `setDimensions(cols, rows)`: `true` quando mudaram.
    pub fn set_dimensions(&self, cols: i32, rows: i32) -> bool {
        if Some(cols) == self.cols.get() && Some(rows) == self.rows.get() {
            // Nothing changed
            return false;
        }
        if cols == 0 || rows == 0 {
            return false;
        }
        self.cols.set(Some(cols));
        self.rows.set(Some(rows));
        true
    }

    /// `setInteractedWith()`.
    pub fn set_interacted_with(&self) -> bool {
        let state = self.state.get();
        if !state.is_interacted_with {
            self.state.set(TerminalState { is_interacted_with: true, ..state });
            return true;
        }
        false
    }

    /// `setShellType(shellType)`.
    pub fn set_shell_type(&self, shell_type: TerminalShellType) -> bool {
        let state = self.state.get();
        if state.shell != shell_type {
            self.state.set(TerminalState { shell: shell_type, ..state });
            return true;
        }
        false
    }

    /// `setSelection(selection)`.
    pub fn set_selection(&self, selection: Option<&str>) {
        *self.selection.borrow_mut() = selection.map(str::to_owned);
    }

    /// `_setProcessId(processId)`.
    pub fn set_process_id(self: &Rc<Self>, process_id: Option<i32>) {
        // The event may fire 2 times when the panel is restored
        let complete = self.pid_promise_complete.borrow_mut().take();
        match complete {
            Some(complete) => complete.resolve(process_id),
            None => {
                // Recreate the promise if this is the nth processId set (e.g. reused task terminals)
                let this = self.clone();
                let pid_promise = self.pid_promise.borrow().clone();
                drop(pid_promise.then(move |pid| {
                    if pid != process_id {
                        *this.pid_promise.borrow_mut() = Promise::resolved(process_id);
                    }
                }));
            }
        }
    }
}

/// `vscode.Terminal`: o objeto da API, com os getters que fecham sobre o estado do `ExtHostTerminal`.
pub struct VscodeTerminal {
    core: Rc<TerminalCore>,
}

impl VscodeTerminal {
    /// `name`.
    pub fn name(&self) -> String {
        self.core.name.borrow().clone().unwrap_or_default()
    }

    /// `processId`.
    pub fn process_id(&self) -> Promise<Option<i32>> {
        self.core.pid_promise.borrow().clone()
    }

    /// `creationOptions`.
    pub fn creation_options(&self) -> &TerminalCreationOptions {
        &self.core.creation_options
    }

    /// `exitStatus`.
    pub fn exit_status(&self) -> Option<TerminalExitStatus> {
        *self.core.exit_status.borrow()
    }

    /// `state`.
    pub fn state(&self) -> TerminalState {
        self.core.state.get()
    }

    /// `selection`.
    pub fn selection(&self) -> Option<String> {
        self.core.selection.borrow().clone()
    }

    /// `shellIntegration`.
    pub fn shell_integration(&self) -> Option<Rc<TerminalShellIntegration>> {
        self.core.shell_integration()
    }

    /// `sendText(text, shouldExecute = true)`.
    pub fn send_text(&self, text: &str, should_execute: Option<bool>) -> Result<(), Error> {
        self.core.check_disposed()?;
        self.core.proxy.rpc_send_text(&self.core.id(), text, should_execute.unwrap_or(true));
        Ok(())
    }

    /// `show(preserveFocus)`.
    pub fn show(&self, preserve_focus: bool) -> Result<(), Error> {
        self.core.check_disposed()?;
        self.core.proxy.rpc_show(&self.core.id(), preserve_focus);
        Ok(())
    }

    /// `hide()`.
    pub fn hide(&self) -> Result<(), Error> {
        self.core.check_disposed()?;
        self.core.proxy.rpc_hide(&self.core.id());
        Ok(())
    }

    /// `dimensions`.
    pub fn dimensions(&self) -> Option<TerminalDimensions> {
        match (self.core.cols.get(), self.core.rows.get()) {
            (Some(columns), Some(rows)) => Some(TerminalDimensions { columns, rows }),
            _ => None,
        }
    }
}

impl IDisposable for VscodeTerminal {
    /// `dispose()`.
    fn dispose(&self) {
        if !self.core.disposed.get() {
            self.core.disposed.set(true);
            self.core.proxy.rpc_dispose(&self.core.id());
        }
    }
}

/// `ExtHostTerminal`.
pub struct ExtHostTerminal {
    base: Rc<Disposable>,
    core: Rc<TerminalCore>,
    value: Rc<VscodeTerminal>,
    on_will_dispose_emitter: Rc<Emitter<()>>,
}

impl std::ops::Deref for ExtHostTerminal {
    type Target = TerminalCore;

    fn deref(&self) -> &TerminalCore {
        &self.core
    }
}

/// `asTerminalIcon(iconPath)`.
fn as_terminal_icon(icon_path: Option<&TerminalIconPath>) -> Option<TerminalIcon> {
    Some(match icon_path? {
        TerminalIconPath::Uri(uri) => TerminalIcon::Uri(UriLike::Uri(uri.clone())),
        TerminalIconPath::LightDark { light, dark } => TerminalIcon::LightDark(LightDarkIcon { light: UriLike::Uri(light.clone()), dark: UriLike::Uri(dark.clone()) }),
        TerminalIconPath::Theme(icon) => TerminalIcon::Theme(ThemeIcon { id: icon.id.clone(), color: icon.color.clone() }),
    })
}

/// `ExtHostTerminal#_serializeParentTerminal(location, parentTerminal)`.
fn serialize_parent_terminal_location(location: Option<&TerminalLocationOption>, parent_terminal: Option<&ExtHostTerminalIdentifier>) -> Option<TerminalLaunchLocation> {
    match location? {
        TerminalLocationOption::Split { .. } => parent_terminal.map(|parent| TerminalLaunchLocation::ParentTerminal(parent.clone())),
        TerminalLocationOption::Editor(options) => Some(TerminalLaunchLocation::ViewColumn {
            view_column: ViewColumn::from(Some(options.view_column)),
            preserve_focus: options.preserve_focus,
            preserve_state: None,
        }),
        TerminalLocationOption::Location(location) => Some(TerminalLaunchLocation::Location(*location)),
    }
}

impl ExtHostTerminal {
    pub fn new(proxy: Rc<dyn MainThreadTerminalServiceShape>, id: ExtHostTerminalIdentifier, creation_options: TerminalCreationOptions, name: Option<String>) -> Rc<Self> {
        let (pid_promise, pid_promise_complete) = Promise::new();
        let core = Rc::new(TerminalCore {
            proxy,
            id: RefCell::new(id),
            creation_options,
            name: RefCell::new(name),
            disposed: Cell::new(false),
            pid_promise: RefCell::new(pid_promise),
            pid_promise_complete: RefCell::new(Some(pid_promise_complete)),
            cols: Cell::new(None),
            rows: Cell::new(None),
            exit_status: RefCell::new(None),
            state: Cell::new(TerminalState { is_interacted_with: false, shell: None }),
            selection: RefCell::new(None),
            shell_integration: RefCell::new(None),
            is_open: Cell::new(false),
        });
        let base = Disposable::new();
        let on_will_dispose_emitter = Emitter::new();
        base.register(on_will_dispose_emitter.clone());
        Rc::new(Self { base, value: Rc::new(VscodeTerminal { core: core.clone() }), core, on_will_dispose_emitter })
    }

    /// `value`.
    pub fn value(&self) -> Rc<VscodeTerminal> {
        self.value.clone()
    }

    /// `onWillDispose`.
    pub fn on_will_dispose(&self) -> Event<()> {
        self.on_will_dispose_emitter.event()
    }

    /// `create(options, internalOptions)`.
    pub fn create(&self, options: &TerminalOptions, internal_options: Option<&ITerminalInternalOptions>) -> Promise<()> {
        let ExtHostTerminalIdentifier::Uuid(uuid) = self.core.id() else {
            return Promise::rejected(Error::new("Terminal has already been created"));
        };
        let internal = internal_options.cloned().unwrap_or_default();
        let launch_config = TerminalLaunchConfig {
            name: options.name.clone(),
            shell_path: options.shell_path.clone(),
            shell_args: options.shell_args.clone(),
            cwd: options.cwd.clone().or_else(|| internal.cwd.clone()),
            env: options.env.clone(),
            icon: as_terminal_icon(options.icon_path.as_ref()),
            color: options.color.as_ref().map(|color| color.id.clone()),
            initial_text: options.message.clone(),
            strict_env: options.strict_env,
            hide_from_user: options.hide_from_user,
            force_shell_integration: internal.force_shell_integration,
            is_feature_terminal: internal.is_feature_terminal,
            is_remote_resolver_terminal: internal.is_remote_resolver_terminal,
            is_extension_owned_terminal: Some(true),
            use_shell_environment: internal.use_shell_environment,
            location: internal
                .location
                .map(TerminalLaunchLocation::from)
                .or_else(|| serialize_parent_terminal_location(options.location.as_ref(), internal.resolved_ext_host_identifier.as_ref())),
            is_transient: options.is_transient,
            shell_integration_nonce: options.shell_integration_nonce.clone(),
            title_template: options.title_template.clone(),
            ..TerminalLaunchConfig::default()
        };
        self.core.proxy.rpc_create_terminal(&uuid, launch_config)
    }

    /// `createExtensionTerminal(location, internalOptions, parentTerminal, iconPath, color, shellIntegrationNonce, titleTemplate)`.
    #[allow(clippy::too_many_arguments)]
    pub fn create_extension_terminal(
        &self,
        location: Option<&TerminalLocationOption>,
        internal_options: Option<&ITerminalInternalOptions>,
        parent_terminal: Option<&ExtHostTerminalIdentifier>,
        icon_path: Option<TerminalIcon>,
        color: Option<&ThemeColor>,
        shell_integration_nonce: Option<String>,
        title_template: Option<String>,
    ) -> Promise<u32> {
        let ExtHostTerminalIdentifier::Uuid(uuid) = self.core.id() else {
            return Promise::rejected(Error::new("Terminal has already been created"));
        };
        let launch_config = TerminalLaunchConfig {
            name: self.core.name.borrow().clone(),
            is_extension_custom_pty_terminal: Some(true),
            icon: icon_path,
            color: color.map(|color| color.id.clone()),
            location: internal_options
                .and_then(|internal| internal.location)
                .map(TerminalLaunchLocation::from)
                .or_else(|| serialize_parent_terminal_location(location, parent_terminal)),
            is_transient: Some(true),
            shell_integration_nonce,
            title_template,
            ..TerminalLaunchConfig::default()
        };
        let core = self.core.clone();
        self.core.proxy.rpc_create_terminal(&uuid, launch_config).then_result(move |result| {
            result?;
            // At this point, the id has been set via `$acceptTerminalOpened`
            match core.id() {
                ExtHostTerminalIdentifier::Uuid(_) => Err(Error::new("Terminal creation failed")),
                ExtHostTerminalIdentifier::Id(id) => Ok(id),
            }
        })
    }
}
impl IDisposable for ExtHostTerminal {
    fn dispose(&self) {
        self.on_will_dispose_emitter.fire(());
        self.base.dispose();
    }
}

// ---- o pseudoterminal de extensão

/// `ExtHostPseudoterminal`.
pub struct ExtHostPseudoterminal {
    base: Rc<Disposable>,
    pty: Rc<dyn Pseudoterminal>,
    on_process_data_emitter: Rc<Emitter<ProcessData>>,
    on_process_ready_emitter: Rc<Emitter<IProcessReadyEvent>>,
    on_did_change_property_emitter: Rc<Emitter<IProcessProperty>>,
    on_process_exit_emitter: Rc<Emitter<Option<i32>>>,
}

impl ExtHostPseudoterminal {
    pub fn new(pty: Rc<dyn Pseudoterminal>) -> Rc<Self> {
        Rc::new(Self {
            base: Disposable::new(),
            pty,
            on_process_data_emitter: Emitter::new(),
            on_process_ready_emitter: Emitter::new(),
            on_did_change_property_emitter: Emitter::new(),
            on_process_exit_emitter: Emitter::new(),
        })
    }

    /// `startSendingEvents(initialDimensions)`.
    pub fn start_sending_events(&self, initial_dimensions: Option<ITerminalDimensionsDto>) {
        // Attach the listeners
        let on_process_data = self.on_process_data_emitter.clone();
        self.base.register(self.pty.on_did_write().listen(move |data| on_process_data.fire(ProcessData::Text(data.clone()))));
        if let Some(on_did_close) = self.pty.on_did_close() {
            let on_process_exit = self.on_process_exit_emitter.clone();
            self.base.register(on_did_close.listen(move |exit_code| on_process_exit.fire(*exit_code)));
        }
        if let Some(on_did_override_dimensions) = self.pty.on_did_override_dimensions() {
            let on_did_change_property = self.on_did_change_property_emitter.clone();
            self.base.register(on_did_override_dimensions.listen(move |dimensions| {
                if let Some(dimensions) = dimensions {
                    on_did_change_property.fire(IProcessProperty::OverrideDimensions(Some(ITerminalDimensionsOverride {
                        cols: dimensions.columns,
                        rows: dimensions.rows,
                        force_exact_size: None,
                    })));
                }
            }));
        }
        if let Some(on_did_change_name) = self.pty.on_did_change_name() {
            let on_did_change_property = self.on_did_change_property_emitter.clone();
            self.base.register(on_did_change_name.listen(move |title| on_did_change_property.fire(IProcessProperty::Title(title.clone()))));
        }

        self.pty.open(initial_dimensions);

        if let Some(initial_dimensions) = initial_dimensions {
            self.pty.set_dimensions(initial_dimensions);
        }

        self.on_process_ready_emitter.fire(IProcessReadyEvent { pid: -1, cwd: String::new(), windows_pty: None });
    }
}

impl IDisposable for ExtHostPseudoterminal {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl ITerminalChildProcess for ExtHostPseudoterminal {
    fn id(&self) -> u32 {
        0
    }

    fn should_persist(&self) -> bool {
        false
    }

    fn on_process_data(&self) -> Event<ProcessData> {
        self.on_process_data_emitter.event()
    }

    fn on_process_ready(&self) -> Event<IProcessReadyEvent> {
        self.on_process_ready_emitter.event()
    }

    fn on_did_change_property(&self) -> Event<IProcessProperty> {
        self.on_did_change_property_emitter.event()
    }

    fn on_process_exit(&self) -> Event<Option<i32>> {
        self.on_process_exit_emitter.event()
    }

    fn start(&self) -> Promise<Option<ITerminalLaunchOutcome>> {
        Promise::resolved(None)
    }

    fn shutdown(&self, _immediate: bool) {
        self.pty.close();
    }

    fn input(&self, data: &str) {
        self.pty.handle_input(data);
    }

    fn send_signal(&self, _signal: &str) {
        // Extension owned terminals don't support sending signals directly to processes
        // This could be extended in the future if the pseudoterminal API is enhanced
    }

    fn process_binary(&self, _data: &str) -> Promise<()> {
        // No-op, processBinary is not supported in extension owned terminals.
        Promise::resolved(())
    }

    fn resize(&self, cols: i32, rows: i32, _pixel_width: Option<i32>, _pixel_height: Option<i32>) -> Result<(), Error> {
        self.pty.set_dimensions(TerminalDimensions { columns: cols, rows });
        Ok(())
    }

    fn clear_buffer(&self) -> Promise<()> {
        // no-op
        Promise::resolved(())
    }

    fn acknowledge_data_event(&self, _char_count: usize) {
        // No-op, flow control is not supported in extension owned terminals. If this is ever
        // implemented it will need new pause and resume VS Code APIs.
    }

    fn set_unicode_version(&self, _version: UnicodeVersion) -> Promise<()> {
        // No-op, xterm-headless isn't used for extension owned terminals.
        Promise::resolved(())
    }

    fn get_initial_cwd(&self) -> Promise<String> {
        Promise::resolved(String::new())
    }

    fn get_cwd(&self) -> Promise<String> {
        Promise::resolved(String::new())
    }

    fn refresh_property(&self, property: ProcessPropertyType) -> Promise<IProcessProperty> {
        errors::throw(Error::new(format!("refreshProperty is not suppported in extension owned terminals. property: {}", property.as_str())))
    }

    fn update_property(&self, property: IProcessProperty) -> Promise<()> {
        errors::throw(Error::new(format!(
            "updateProperty is not suppported in extension owned terminals. property: {}",
            property.property_type().as_str()
        )))
    }
}

/// O processo de um terminal: o `ExtHostPseudoterminal` (que o original reconhece com `instanceof`) ou qualquer outro.
#[derive(Clone)]
pub enum TerminalChildProcess {
    Pseudoterminal(Rc<ExtHostPseudoterminal>),
    Other(Rc<dyn ITerminalChildProcess>),
}

impl TerminalChildProcess {
    /// O processo como o `ITerminalChildProcess` do original.
    pub fn process(&self) -> Rc<dyn ITerminalChildProcess> {
        match self {
            Self::Pseudoterminal(pty) => pty.clone(),
            Self::Other(process) => process.clone(),
        }
    }
}

// ---- as coleções de variáveis de ambiente

/// `vscode.EnvironmentVariableMutator`: o `variable` fica no objeto, como no `convertMutator`.
#[derive(Clone, Debug, PartialEq)]
pub struct EnvironmentVariableMutator {
    pub variable: String,
    pub mutator_type: EnvironmentVariableMutatorType,
    pub value: String,
    pub options: Option<IEnvironmentVariableMutatorOptions>,
}

/// `convertMutator(mutator)`.
fn convert_mutator(mutator: &IEnvironmentVariableMutator) -> EnvironmentVariableMutator {
    EnvironmentVariableMutator { variable: mutator.variable.clone(), mutator_type: mutator.mutator_type, value: mutator.value.clone(), options: mutator.options }
}

/// `UnifiedEnvironmentVariableCollection`: a coleção com a informação de todos os escopos de uma extensão.
pub struct UnifiedEnvironmentVariableCollection {
    this: Weak<Self>,
    base: Rc<Disposable>,
    map: RefCell<OrderedMap<String, IEnvironmentVariableMutator>>,
    scoped_collections: RefCell<HashMap<String, Rc<ScopedEnvironmentVariableCollection>>>,
    description_map: RefCell<OrderedMap<String, IEnvironmentVariableCollectionDescription>>,
    persistent: Cell<bool>,
    on_did_change_collection_emitter: Rc<Emitter<()>>,
}

impl UnifiedEnvironmentVariableCollection {
    pub fn new(serialized: Option<ISerializableEnvironmentVariableCollection>) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change_collection_emitter = Emitter::new();
        base.register(on_did_change_collection_emitter.clone());
        let mut map = OrderedMap::new();
        for (key, mutator) in serialized.unwrap_or_default() {
            map.insert(key, mutator);
        }
        Rc::new_cyclic(|this| Self {
            this: this.clone(),
            base,
            map: RefCell::new(map),
            scoped_collections: RefCell::new(HashMap::new()),
            description_map: RefCell::new(OrderedMap::new()),
            persistent: Cell::new(true),
            on_did_change_collection_emitter,
        })
    }

    /// `persistent`.
    pub fn persistent(&self) -> bool {
        self.persistent.get()
    }

    /// `set persistent(value)`.
    pub fn set_persistent(&self, value: bool) {
        self.persistent.set(value);
        self.on_did_change_collection_emitter.fire(());
    }

    /// `onDidChangeCollection`.
    pub fn on_did_change_collection(&self) -> Event<()> {
        self.on_did_change_collection_emitter.event()
    }

    /// `map`, como a lista serializável de pares.
    pub fn serialized_map(&self) -> ISerializableEnvironmentVariableCollection {
        serialize_environment_variable_collection(&self.map.borrow())
    }

    /// `descriptionMap`, serializado.
    pub fn serialized_description_map(&self) -> Vec<(String, IEnvironmentVariableCollectionDescription)> {
        serialize_environment_description_map(Some(&self.description_map.borrow()))
    }

    /// `getScopedEnvironmentVariableCollection(scope)`.
    pub fn get_scoped_environment_variable_collection(&self, scope: Option<&EnvironmentVariableScope>) -> Rc<ScopedEnvironmentVariableCollection> {
        let scoped_collection_key = Self::get_scope_key(scope);
        let existing = self.scoped_collections.borrow().get(&scoped_collection_key).cloned();
        if let Some(existing) = existing {
            return existing;
        }
        let scoped_collection = ScopedEnvironmentVariableCollection::new(self.this.clone(), scope.cloned());
        self.scoped_collections.borrow_mut().insert(scoped_collection_key, scoped_collection.clone());
        let this = self.this.clone();
        self.base.register(scoped_collection.on_did_change_collection().listen(move |()| {
            if let Some(this) = this.upgrade() {
                this.on_did_change_collection_emitter.fire(());
            }
        }));
        scoped_collection
    }

    /// `replace(variable, value, options, scope)`.
    pub fn replace(&self, variable: &str, value: &str, options: Option<IEnvironmentVariableMutatorOptions>, scope: Option<&EnvironmentVariableScope>) -> Result<(), Error> {
        self.set_if_differs(variable, value, EnvironmentVariableMutatorType::Replace, options, scope)
    }

    /// `append(variable, value, options, scope)`.
    pub fn append(&self, variable: &str, value: &str, options: Option<IEnvironmentVariableMutatorOptions>, scope: Option<&EnvironmentVariableScope>) -> Result<(), Error> {
        self.set_if_differs(variable, value, EnvironmentVariableMutatorType::Append, options, scope)
    }

    /// `prepend(variable, value, options, scope)`.
    pub fn prepend(&self, variable: &str, value: &str, options: Option<IEnvironmentVariableMutatorOptions>, scope: Option<&EnvironmentVariableScope>) -> Result<(), Error> {
        self.set_if_differs(variable, value, EnvironmentVariableMutatorType::Prepend, options, scope)
    }

    /// `_setIfDiffers(variable, mutator)`, com as `options ?? { applyAtProcessCreation: true }` que os três chamadores aplicam.
    fn set_if_differs(
        &self,
        variable: &str,
        value: &str,
        mutator_type: EnvironmentVariableMutatorType,
        options: Option<IEnvironmentVariableMutatorOptions>,
        scope: Option<&EnvironmentVariableScope>,
    ) -> Result<(), Error> {
        let options = options.unwrap_or(IEnvironmentVariableMutatorOptions { apply_at_process_creation: Some(true), apply_at_shell_integration: None });
        if options.apply_at_process_creation == Some(false) && !options.apply_at_shell_integration.unwrap_or(false) {
            return Err(Error::new("EnvironmentVariableMutatorOptions must apply at either process creation or shell integration"));
        }
        let key = Self::get_key(variable, scope);
        let current = self.map.borrow().get(&key).cloned();
        let new_options = IEnvironmentVariableMutatorOptions {
            apply_at_process_creation: Some(options.apply_at_process_creation.unwrap_or(false)),
            apply_at_shell_integration: Some(options.apply_at_shell_integration.unwrap_or(false)),
        };
        let differs = match &current {
            None => true,
            Some(current) => {
                current.value != value
                    || current.mutator_type != mutator_type
                    || current.options.and_then(|options| options.apply_at_process_creation) != new_options.apply_at_process_creation
                    || current.options.and_then(|options| options.apply_at_shell_integration) != new_options.apply_at_shell_integration
                    || current.scope.as_ref().and_then(|scope| scope.workspace_folder.as_ref()).map(|folder| folder.index)
                        != scope.and_then(|scope| scope.workspace_folder.as_ref()).map(|folder| folder.index)
            }
        };
        if differs {
            let mutator = IEnvironmentVariableMutator {
                variable: variable.to_owned(),
                value: value.to_owned(),
                mutator_type,
                scope: scope.cloned(),
                options: Some(new_options),
            };
            self.map.borrow_mut().insert(key, mutator);
            self.on_did_change_collection_emitter.fire(());
        }
        Ok(())
    }

    /// `get(variable, scope)`.
    pub fn get(&self, variable: &str, scope: Option<&EnvironmentVariableScope>) -> Option<EnvironmentVariableMutator> {
        let key = Self::get_key(variable, scope);
        // Set options to defaults if needed (o original deixa isto pendente: as opções saem como estão)
        self.map.borrow().get(&key).map(convert_mutator)
    }

    fn get_key(variable: &str, scope: Option<&EnvironmentVariableScope>) -> String {
        let scope_key = Self::get_scope_key(scope);
        if scope_key.is_empty() { variable.to_owned() } else { format!("{variable}:::{scope_key}") }
    }

    fn get_scope_key(scope: Option<&EnvironmentVariableScope>) -> String {
        Self::get_workspace_key(scope.and_then(|scope| scope.workspace_folder.as_ref())).unwrap_or_default()
    }

    fn get_workspace_key(workspace_folder: Option<&je_platform::workspace::WorkspaceFolder>) -> Option<String> {
        workspace_folder.map(|folder| folder.uri.to_string(false).to_string_lossy())
    }

    /// `getVariableMap(scope)`.
    pub fn get_variable_map(&self, scope: Option<&EnvironmentVariableScope>) -> OrderedMap<String, EnvironmentVariableMutator> {
        let mut map = OrderedMap::new();
        for (_, value) in self.map.borrow().iter() {
            if Self::get_scope_key(value.scope.as_ref()) == Self::get_scope_key(scope) {
                map.insert(value.variable.clone(), convert_mutator(value));
            }
        }
        map
    }

    /// `delete(variable, scope)`.
    pub fn delete(&self, variable: &str, scope: Option<&EnvironmentVariableScope>) {
        let key = Self::get_key(variable, scope);
        self.map.borrow_mut().remove(&key);
        self.on_did_change_collection_emitter.fire(());
    }

    /// `clear(scope)`.
    pub fn clear(&self, scope: Option<&EnvironmentVariableScope>) {
        match scope.and_then(|scope| scope.workspace_folder.as_ref()) {
            Some(folder) => {
                let keys: Vec<String> = self
                    .map
                    .borrow()
                    .iter()
                    .filter(|(_, mutator)| mutator.scope.as_ref().and_then(|scope| scope.workspace_folder.as_ref()).map(|other| other.index) == Some(folder.index))
                    .map(|(key, _)| key.clone())
                    .collect();
                for key in keys {
                    self.map.borrow_mut().remove(&key);
                }
                self.clear_description(scope);
            }
            None => {
                self.map.borrow_mut().clear();
                self.description_map.borrow_mut().clear();
            }
        }
        self.on_did_change_collection_emitter.fire(());
    }

    /// `setDescription(description, scope)`.
    pub fn set_description(&self, description: Option<&Documentation>, scope: Option<&EnvironmentVariableScope>) {
        let key = Self::get_scope_key(scope);
        let current = self.description_map.borrow().get(&key).cloned();
        let unchanged = match (&current, description) {
            (Some(current), Some(Documentation::Text(text))) => current.description.as_deref() == Some(text.as_str()),
            (Some(current), None) => current.description.is_none(),
            // Um `MarkdownString` nunca é igual a um texto.
            _ => false,
        };
        if !unchanged {
            let description_str = match description {
                Some(Documentation::Text(text)) => Some(text.clone()),
                // Only take the description before the first `\n\n`, so that the description doesn't mess up the UI
                Some(Documentation::Markdown(markdown)) => markdown.value.to_string_lossy().split("\n\n").next().map(str::to_owned),
                None => None,
            };
            let value = IEnvironmentVariableCollectionDescription { description: description_str, scope: scope.cloned() };
            self.description_map.borrow_mut().insert(key, value);
            self.on_did_change_collection_emitter.fire(());
        }
    }

    /// `getDescription(scope)`.
    pub fn get_description(&self, scope: Option<&EnvironmentVariableScope>) -> Option<String> {
        let key = Self::get_scope_key(scope);
        self.description_map.borrow().get(&key).and_then(|description| description.description.clone())
    }

    fn clear_description(&self, scope: Option<&EnvironmentVariableScope>) {
        let key = Self::get_scope_key(scope);
        self.description_map.borrow_mut().remove(&key);
    }
}

impl IDisposable for UnifiedEnvironmentVariableCollection {
    fn dispose(&self) {
        self.base.dispose();
    }
}

/// `ScopedEnvironmentVariableCollection`: a `vscode.EnvironmentVariableCollection` de um escopo (com o `getScoped`).
pub struct ScopedEnvironmentVariableCollection {
    collection: Weak<UnifiedEnvironmentVariableCollection>,
    scope: Option<EnvironmentVariableScope>,
    on_did_change_collection_emitter: Rc<Emitter<()>>,
}

impl ScopedEnvironmentVariableCollection {
    fn new(collection: Weak<UnifiedEnvironmentVariableCollection>, scope: Option<EnvironmentVariableScope>) -> Rc<Self> {
        Rc::new(Self { collection, scope, on_did_change_collection_emitter: Emitter::new() })
    }

    fn collection(&self) -> Rc<UnifiedEnvironmentVariableCollection> {
        self.collection.upgrade().expect("a coleção unificada vive enquanto o serviço de terminais vive")
    }

    /// `onDidChangeCollection`.
    pub fn on_did_change_collection(&self) -> Event<()> {
        self.on_did_change_collection_emitter.event()
    }

    /// `persistent`.
    pub fn persistent(&self) -> bool {
        self.collection().persistent()
    }

    /// `set persistent(value)`.
    pub fn set_persistent(&self, value: bool) {
        self.collection().set_persistent(value);
    }

    /// `getScoped(scope)`.
    pub fn get_scoped(&self, scope: Option<&EnvironmentVariableScope>) -> Rc<ScopedEnvironmentVariableCollection> {
        self.collection().get_scoped_environment_variable_collection(scope)
    }

    pub fn replace(&self, variable: &str, value: &str, options: Option<IEnvironmentVariableMutatorOptions>) -> Result<(), Error> {
        self.collection().replace(variable, value, options, self.scope.as_ref())
    }

    pub fn append(&self, variable: &str, value: &str, options: Option<IEnvironmentVariableMutatorOptions>) -> Result<(), Error> {
        self.collection().append(variable, value, options, self.scope.as_ref())
    }

    pub fn prepend(&self, variable: &str, value: &str, options: Option<IEnvironmentVariableMutatorOptions>) -> Result<(), Error> {
        self.collection().prepend(variable, value, options, self.scope.as_ref())
    }

    pub fn get(&self, variable: &str) -> Option<EnvironmentVariableMutator> {
        self.collection().get(variable, self.scope.as_ref())
    }

    /// `forEach(callback)`.
    pub fn for_each(&self, callback: impl Fn(&str, &EnvironmentVariableMutator, &ScopedEnvironmentVariableCollection)) {
        let map = self.collection().get_variable_map(self.scope.as_ref());
        for (variable, value) in map.iter() {
            callback(variable, value, self);
        }
    }

    /// `[Symbol.iterator]()`: os pares `[variable, mutator]`.
    pub fn entries(&self) -> Vec<(String, EnvironmentVariableMutator)> {
        self.collection().get_variable_map(self.scope.as_ref()).iter().map(|(variable, mutator)| (variable.clone(), mutator.clone())).collect()
    }

    pub fn delete(&self, variable: &str) {
        self.collection().delete(variable, self.scope.as_ref());
        self.on_did_change_collection_emitter.fire(());
    }

    pub fn clear(&self) {
        self.collection().clear(self.scope.as_ref());
    }

    /// `set description(description)`.
    pub fn set_description(&self, description: Option<&Documentation>) {
        self.collection().set_description(description, self.scope.as_ref());
    }

    /// `get description()`.
    pub fn description(&self) -> Option<String> {
        self.collection().get_description(self.scope.as_ref())
    }
}

// ---- o serviço

/// O que o cache de links guarda de cada link: o provedor e o link dele.
#[derive(Clone)]
struct CachedLinkEntry {
    provider: Rc<dyn TerminalLinkProvider>,
    link: TerminalLink,
}

/// O resultado de um provedor de links em `$provideLinks`.
#[derive(Clone)]
struct ProviderLinks {
    provider: Rc<dyn TerminalLinkProvider>,
    links: Vec<TerminalLink>,
}

/// Os dois métodos abstratos de `BaseExtHostTerminalService`: como cada serviço cria um terminal.
pub trait ExtHostTerminalCreation: Sized + 'static {
    /// `createTerminal(name?, shellPath?, shellArgs?)`.
    fn create_terminal(
        &self,
        service: &BaseExtHostTerminalService<Self>,
        name: Option<String>,
        shell_path: Option<String>,
        shell_args: Option<OneOrMany<String>>,
    ) -> Result<Rc<VscodeTerminal>, Error>;

    /// `createTerminalFromOptions(options, internalOptions?)`.
    fn create_terminal_from_options(
        &self,
        service: &BaseExtHostTerminalService<Self>,
        options: TerminalOptions,
        internal_options: Option<ITerminalInternalOptions>,
    ) -> Result<Rc<VscodeTerminal>, Error>;
}

/// O processador de argumentos do construtor: troca o contexto `TerminalContext` pelo `vscode.Terminal`.
struct TerminalArgumentProcessor<S: ExtHostTerminalCreation> {
    service: Weak<BaseExtHostTerminalService<S>>,
}

impl<S: ExtHostTerminalCreation> TerminalArgumentProcessor<S> {
    /// `deserialize(arg)`: `this.getTerminalById(arg.instanceId)?.value`.
    fn deserialize(&self, arg: &Dynamic) -> Dynamic {
        let terminal = match (self.service.upgrade(), arg.get_str("instanceId")) {
            (Some(service), Dynamic::Number(instance_id)) => service.get_terminal_by_id(instance_id as u32),
            _ => None,
        };
        terminal.map_or(Dynamic::Undefined, |terminal| Dynamic::Object(ObjectRef::with_host(terminal.value())))
    }

    /// `arg?.$mid === MarshalledId.TerminalContext`.
    fn is_terminal_context(arg: &Dynamic) -> bool {
        matches!(arg, Dynamic::Object(_)) && matches!(arg.get_str("$mid"), Dynamic::Number(mid) if mid == f64::from(MarshalledId::TerminalContext as i32))
    }
}

impl<S: ExtHostTerminalCreation> ArgumentProcessor for TerminalArgumentProcessor<S> {
    fn process_argument(&self, arg: &Dynamic, _extension: Option<&ExtensionDescription>) -> Dynamic {
        if Self::is_terminal_context(arg) {
            return self.deserialize(arg);
        }
        // Do array transformation in place as this is a hot path
        if let Dynamic::Array(items) = arg {
            for i in 0..items.len() {
                let item = items.get(i);
                if Self::is_terminal_context(&item) {
                    items.set(i, self.deserialize(&item));
                } else {
                    // Probably something else, so exit early
                    break;
                }
            }
        }
        arg.clone()
    }
}

/// `BaseExtHostTerminalService` (e a sua `IExtHostTerminalService`).
pub struct BaseExtHostTerminalService<S: ExtHostTerminalCreation> {
    this: Weak<Self>,
    base: Rc<Disposable>,
    creation: S,
    proxy: Rc<dyn MainThreadTerminalServiceShape>,
    ext_host_commands: Rc<dyn IExtHostCommands>,

    active_terminal: RefCell<Option<Rc<ExtHostTerminal>>>,
    terminals: RefCell<Vec<Rc<ExtHostTerminal>>>,
    terminal_processes: RefCell<HashMap<u32, TerminalChildProcess>>,
    terminal_process_disposables: RefCell<HashMap<u32, DisposableRef>>,
    /// `_extensionTerminalAwaitingStart`: a presença da chave é o `{ initialDimensions }` do original.
    extension_terminal_awaiting_start: RefCell<HashMap<u32, Option<ITerminalDimensionsDto>>>,
    environment_variable_collections: RefCell<HashMap<String, Rc<UnifiedEnvironmentVariableCollection>>>,
    default_profile: RefCell<Option<ITerminalProfile>>,
    default_automation_profile: RefCell<Option<ITerminalProfile>>,
    last_quick_fix_commands: Rc<MutableDisposable<dyn IDisposable>>,

    bufferer: TerminalDataBufferer,
    link_providers: RefCell<Vec<Rc<dyn TerminalLinkProvider>>>,
    completion_providers: RefCell<HashMap<String, Rc<dyn TerminalCompletionProvider>>>,
    profile_providers: RefCell<HashMap<String, (Rc<dyn TerminalProfileProvider>, ExtensionDescription)>>,
    quick_fix_providers: RefCell<HashMap<String, Rc<dyn TerminalQuickFixProvider>>>,
    terminal_link_cache: RefCell<HashMap<u32, HashMap<u32, CachedLinkEntry>>>,
    terminal_link_cancellation_source: RefCell<HashMap<u32, CancellationTokenSource>>,
    next_link_id: Cell<u32>,

    on_did_close_terminal_emitter: Rc<Emitter<Rc<VscodeTerminal>>>,
    on_did_open_terminal_emitter: Rc<Emitter<Rc<VscodeTerminal>>>,
    on_did_change_active_terminal_emitter: Rc<Emitter<Option<Rc<VscodeTerminal>>>>,
    on_did_change_terminal_dimensions_emitter: Rc<Emitter<(Rc<VscodeTerminal>, TerminalDimensions)>>,
    on_did_change_terminal_state_emitter: Rc<Emitter<Rc<VscodeTerminal>>>,
    on_did_change_shell_emitter: Rc<Emitter<String>>,
    on_did_write_terminal_data_emitter: Rc<Emitter<(Rc<VscodeTerminal>, String)>>,
    on_did_execute_command_emitter: Rc<Emitter<(Rc<VscodeTerminal>, ITerminalCommandDto)>>,
}

impl<S: ExtHostTerminalCreation> BaseExtHostTerminalService<S> {
    pub fn with_creation(supports_processes: bool, ext_host_commands: Rc<dyn IExtHostCommands>, ext_host_rpc: &dyn IMainContext, creation: S) -> Rc<Self> {
        let proxy = ext_host_rpc.get_proxy(&MainContext::MAIN_THREAD_TERMINAL_SERVICE);
        let base = Disposable::new();

        let data_emitter = {
            let (start_proxy, stop_proxy) = (proxy.clone(), proxy.clone());
            Emitter::with_options(EmitterOptions {
                on_will_add_first_listener: Some(Rc::new(move |_| start_proxy.rpc_start_sending_data_events())),
                on_did_remove_last_listener: Some(Rc::new(move |_| stop_proxy.rpc_stop_sending_data_events())),
                ..EmitterOptions::default()
            })
        };
        let command_emitter = {
            let (start_proxy, stop_proxy) = (proxy.clone(), proxy.clone());
            Emitter::with_options(EmitterOptions {
                on_will_add_first_listener: Some(Rc::new(move |_| start_proxy.rpc_start_sending_command_events())),
                on_did_remove_last_listener: Some(Rc::new(move |_| stop_proxy.rpc_stop_sending_command_events())),
                ..EmitterOptions::default()
            })
        };
        let bufferer = {
            let proxy = proxy.clone();
            TerminalDataBufferer::new(move |id, data| proxy.rpc_send_process_data(id, data))
        };

        let service = Rc::new_cyclic(|this| Self {
            this: this.clone(),
            base,
            creation,
            proxy,
            ext_host_commands,
            active_terminal: RefCell::new(None),
            terminals: RefCell::new(Vec::new()),
            terminal_processes: RefCell::new(HashMap::new()),
            terminal_process_disposables: RefCell::new(HashMap::new()),
            extension_terminal_awaiting_start: RefCell::new(HashMap::new()),
            environment_variable_collections: RefCell::new(HashMap::new()),
            default_profile: RefCell::new(None),
            default_automation_profile: RefCell::new(None),
            last_quick_fix_commands: MutableDisposable::new(),
            bufferer,
            link_providers: RefCell::new(Vec::new()),
            completion_providers: RefCell::new(HashMap::new()),
            profile_providers: RefCell::new(HashMap::new()),
            quick_fix_providers: RefCell::new(HashMap::new()),
            terminal_link_cache: RefCell::new(HashMap::new()),
            terminal_link_cancellation_source: RefCell::new(HashMap::new()),
            next_link_id: Cell::new(1),
            on_did_close_terminal_emitter: Emitter::new(),
            on_did_open_terminal_emitter: Emitter::new(),
            on_did_change_active_terminal_emitter: Emitter::new(),
            on_did_change_terminal_dimensions_emitter: Emitter::new(),
            on_did_change_terminal_state_emitter: Emitter::new(),
            on_did_change_shell_emitter: Emitter::new(),
            on_did_write_terminal_data_emitter: data_emitter,
            on_did_execute_command_emitter: command_emitter,
        });

        service.base.register(service.last_quick_fix_commands.clone());
        // Os emissores do original não são `_register`ados; aqui o guarda de vazamento os rastreia, então morrem com o serviço.
        service.base.register(service.on_did_close_terminal_emitter.clone());
        service.base.register(service.on_did_open_terminal_emitter.clone());
        service.base.register(service.on_did_change_active_terminal_emitter.clone());
        service.base.register(service.on_did_change_terminal_dimensions_emitter.clone());
        service.base.register(service.on_did_change_terminal_state_emitter.clone());
        service.base.register(service.on_did_change_shell_emitter.clone());
        service.base.register(service.on_did_write_terminal_data_emitter.clone());
        service.base.register(service.on_did_execute_command_emitter.clone());
        service.proxy.rpc_register_process_support(supports_processes);
        service.ext_host_commands.register_argument_processor(Rc::new(TerminalArgumentProcessor { service: service.this.clone() }));
        let this = service.this.clone();
        service.base.register(to_disposable(move || {
            let Some(this) = this.upgrade() else { return };
            let processes: Vec<TerminalChildProcess> = this.terminal_processes.borrow().values().cloned().collect();
            for terminal_process in processes {
                terminal_process.process().shutdown(true);
            }
        }));
        service
    }

    fn rc(&self) -> Rc<Self> {
        self.this.upgrade().expect("o serviço de terminais está vivo enquanto um método dele roda")
    }

    /// `activeTerminal`.
    pub fn active_terminal(&self) -> Option<Rc<VscodeTerminal>> {
        self.active_terminal.borrow().as_ref().map(|terminal| terminal.value())
    }

    /// `terminals`.
    pub fn terminals(&self) -> Vec<Rc<VscodeTerminal>> {
        self.terminals.borrow().iter().map(|terminal| terminal.value()).collect()
    }

    /// `onDidCloseTerminal`.
    pub fn on_did_close_terminal(&self) -> Event<Rc<VscodeTerminal>> {
        self.on_did_close_terminal_emitter.event()
    }

    /// `onDidOpenTerminal`.
    pub fn on_did_open_terminal(&self) -> Event<Rc<VscodeTerminal>> {
        self.on_did_open_terminal_emitter.event()
    }

    /// `onDidChangeActiveTerminal`.
    pub fn on_did_change_active_terminal(&self) -> Event<Option<Rc<VscodeTerminal>>> {
        self.on_did_change_active_terminal_emitter.event()
    }

    /// `onDidChangeTerminalDimensions`: o terminal e as dimensões novas.
    pub fn on_did_change_terminal_dimensions(&self) -> Event<(Rc<VscodeTerminal>, TerminalDimensions)> {
        self.on_did_change_terminal_dimensions_emitter.event()
    }

    /// `onDidChangeTerminalState`.
    pub fn on_did_change_terminal_state(&self) -> Event<Rc<VscodeTerminal>> {
        self.on_did_change_terminal_state_emitter.event()
    }

    /// `onDidChangeShell`.
    pub fn on_did_change_shell(&self) -> Event<String> {
        self.on_did_change_shell_emitter.event()
    }

    /// `onDidWriteTerminalData`: o terminal e os dados.
    pub fn on_did_write_terminal_data(&self) -> Event<(Rc<VscodeTerminal>, String)> {
        self.on_did_write_terminal_data_emitter.event()
    }

    /// `onDidExecuteTerminalCommand`.
    pub fn on_did_execute_terminal_command(&self) -> Event<(Rc<VscodeTerminal>, ITerminalCommandDto)> {
        self.on_did_execute_command_emitter.event()
    }

    /// `createTerminal(name?, shellPath?, shellArgs?)`.
    pub fn create_terminal(&self, name: Option<String>, shell_path: Option<String>, shell_args: Option<OneOrMany<String>>) -> Result<Rc<VscodeTerminal>, Error> {
        self.creation.create_terminal(self, name, shell_path, shell_args)
    }

    /// `createTerminalFromOptions(options, internalOptions?)`.
    pub fn create_terminal_from_options(&self, options: TerminalOptions, internal_options: Option<ITerminalInternalOptions>) -> Result<Rc<VscodeTerminal>, Error> {
        self.creation.create_terminal_from_options(self, options, internal_options)
    }

    /// `getDefaultShell(useAutomationShell)`.
    pub fn get_default_shell(&self, use_automation_shell: bool) -> String {
        let profile = if use_automation_shell { self.default_automation_profile.borrow() } else { self.default_profile.borrow() };
        profile.as_ref().map(|profile| profile.path.clone()).unwrap_or_default()
    }

    /// `getDefaultShellArgs(useAutomationShell)`.
    pub fn get_default_shell_args(&self, use_automation_shell: bool) -> OneOrMany<String> {
        let profile = if use_automation_shell { self.default_automation_profile.borrow() } else { self.default_profile.borrow() };
        profile.as_ref().and_then(|profile| profile.args.clone()).unwrap_or(OneOrMany::Many(Vec::new()))
    }

    /// `createExtensionTerminal(options, internalOptions?)`.
    pub fn create_extension_terminal(&self, options: &ExtensionTerminalOptions, internal_options: Option<ITerminalInternalOptions>) -> Rc<VscodeTerminal> {
        let terminal = ExtHostTerminal::new(
            self.proxy.clone(),
            ExtHostTerminalIdentifier::Uuid(generate_uuid()),
            TerminalCreationOptions::Extension(options.clone()),
            Some(options.name.clone()),
        );
        let pty = ExtHostPseudoterminal::new(options.pty.clone());
        let mut internal_options = internal_options;
        let serialized = self.serialize_parent_terminal(options.location.as_ref(), internal_options.as_mut());
        let this = self.rc();
        let created = terminal.create_extension_terminal(
            options.location.as_ref(),
            internal_options.as_ref(),
            serialized.resolved_ext_host_identifier.as_ref(),
            as_terminal_icon(options.icon_path.as_ref()),
            options.color.as_ref(),
            options.shell_integration_nonce.clone(),
            options.title_template.clone(),
        );
        drop(created.then(move |id| {
            if this.get_terminal_by_id(id).is_none() {
                return;
            }
            this.setup_ext_host_process_listeners(id, TerminalChildProcess::Pseudoterminal(pty));
        }));
        self.terminals.borrow_mut().push(terminal.clone());
        terminal.value()
    }

    /// `_serializeParentTerminal(options, internalOptions)`: com `internal_options` o objeto é alterado no lugar, como no original.
    pub fn serialize_parent_terminal(&self, location: Option<&TerminalLocationOption>, internal_options: Option<&mut ITerminalInternalOptions>) -> ITerminalInternalOptions {
        let mut fresh = ITerminalInternalOptions::default();
        let internal_options = internal_options.unwrap_or(&mut fresh);
        match location {
            Some(TerminalLocationOption::Split { parent_terminal }) => {
                let parent_ext_host_terminal = self.terminals.borrow().iter().find(|terminal| Rc::ptr_eq(&terminal.value, parent_terminal)).cloned();
                if let Some(parent_ext_host_terminal) = parent_ext_host_terminal {
                    internal_options.resolved_ext_host_identifier = Some(parent_ext_host_terminal.id());
                }
            }
            Some(TerminalLocationOption::Location(location)) => {
                internal_options.location = Some(TerminalProfileLocation::Location(*location));
            }
            _ => {
                if matches!(internal_options.location, Some(TerminalProfileLocation::SplitActiveTerminal(_))) {
                    internal_options.location = Some(TerminalProfileLocation::SplitActiveTerminal(true));
                }
            }
        }
        internal_options.clone()
    }

    /// `attachPtyToTerminal(id, pty)`.
    pub fn attach_pty_to_terminal(&self, id: u32, pty: Rc<dyn Pseudoterminal>) -> Result<(), Error> {
        if self.get_terminal_by_id(id).is_none() {
            return Err(Error::new(format!("Cannot resolve terminal with id {id} for virtual process")));
        }
        let p = ExtHostPseudoterminal::new(pty);
        self.setup_ext_host_process_listeners(id, TerminalChildProcess::Pseudoterminal(p));
        Ok(())
    }

    /// `_setupExtHostProcessListeners(id, p)`.
    pub fn setup_ext_host_process_listeners(&self, id: u32, p: TerminalChildProcess) {
        let disposables = DisposableStore::new();
        if let TerminalChildProcess::Pseudoterminal(pty) = &p {
            disposables.add_rc(pty.clone());
        }
        let process = p.process();
        let proxy = self.proxy.clone();
        disposables.add(process.on_process_ready().listen(move |e| proxy.rpc_send_process_ready(id, e.pid, &e.cwd, e.windows_pty)));
        let proxy = self.proxy.clone();
        disposables.add(process.on_did_change_property().listen(move |property| proxy.rpc_send_process_property(id, property.clone())));

        // Buffer data events to reduce the amount of messages going to the renderer
        disposables.add(self.bufferer.start_buffering(id, &process.on_process_data(), ProcessData::data, None));
        let this = self.this.clone();
        disposables.add(process.on_process_exit().listen(move |exit_code| {
            if let Some(this) = this.upgrade() {
                this.on_process_exit(id, *exit_code);
            }
        }));
        self.terminal_processes.borrow_mut().insert(id, p.clone());
        self.terminal_process_disposables.borrow_mut().insert(id, disposables);

        let awaiting_start = self.extension_terminal_awaiting_start.borrow().get(&id).copied();
        if let (Some(initial_dimensions), TerminalChildProcess::Pseudoterminal(pty)) = (awaiting_start, &p) {
            pty.start_sending_events(initial_dimensions);
            self.extension_terminal_awaiting_start.borrow_mut().remove(&id);
        }
    }

    /// `registerProfileProvider(extension, id, provider)`.
    pub fn register_profile_provider(&self, extension: &ExtensionDescription, id: &str, provider: Rc<dyn TerminalProfileProvider>) -> Result<Rc<VSCodeDisposable>, Error> {
        if self.profile_providers.borrow().contains_key(id) {
            return Err(Error::new(format!("Terminal profile provider \"{id}\" already registered")));
        }
        self.profile_providers.borrow_mut().insert(id.to_owned(), (provider, extension.clone()));
        self.proxy.rpc_register_profile_provider(id, &extension.identifier.value);
        let (this, id) = (self.this.clone(), id.to_owned());
        Ok(Rc::new(VSCodeDisposable::new(move || {
            if let Some(this) = this.upgrade() {
                this.profile_providers.borrow_mut().remove(&id);
                this.proxy.rpc_unregister_profile_provider(&id);
            }
        })))
    }

    /// `registerTerminalCompletionProvider(extension, provider, ...triggerCharacters)`.
    pub fn register_terminal_completion_provider(
        &self,
        extension: &ExtensionDescription,
        provider: Rc<dyn TerminalCompletionProvider>,
        trigger_characters: &[String],
    ) -> Result<Rc<VSCodeDisposable>, Error> {
        let id = extension.identifier.value.clone();
        if self.completion_providers.borrow().contains_key(&id) {
            return Err(Error::new(format!("Terminal completion provider \"{id}\" already registered")));
        }
        self.completion_providers.borrow_mut().insert(id.clone(), provider);
        self.proxy.rpc_register_completion_provider(&id, &id, trigger_characters);
        let this = self.this.clone();
        Ok(Rc::new(VSCodeDisposable::new(move || {
            if let Some(this) = this.upgrade() {
                this.completion_providers.borrow_mut().remove(&id);
                this.proxy.rpc_unregister_completion_provider(&id);
            }
        })))
    }

    /// `registerTerminalQuickFixProvider(id, extensionId, provider)`.
    pub fn register_terminal_quick_fix_provider(&self, id: &str, extension_id: &str, provider: Rc<dyn TerminalQuickFixProvider>) -> Result<Rc<VSCodeDisposable>, Error> {
        if self.quick_fix_providers.borrow().contains_key(id) {
            return Err(Error::new(format!("Terminal quick fix provider \"{id}\" is already registered")));
        }
        self.quick_fix_providers.borrow_mut().insert(id.to_owned(), provider);
        self.proxy.rpc_register_quick_fix_provider(id, extension_id);
        let (this, id) = (self.this.clone(), id.to_owned());
        Ok(Rc::new(VSCodeDisposable::new(move || {
            if let Some(this) = this.upgrade() {
                this.quick_fix_providers.borrow_mut().remove(&id);
                this.proxy.rpc_unregister_quick_fix_provider(&id);
            }
        })))
    }

    /// `registerLinkProvider(provider)`.
    pub fn register_link_provider(&self, provider: Rc<dyn TerminalLinkProvider>) -> Rc<VSCodeDisposable> {
        self.link_providers.borrow_mut().push(provider.clone());
        if self.link_providers.borrow().len() == 1 {
            self.proxy.rpc_start_link_provider();
        }
        let this = self.this.clone();
        Rc::new(VSCodeDisposable::new(move || {
            let Some(this) = this.upgrade() else { return };
            // `Set#delete`: tira o provedor se estiver registrado.
            let position = this.link_providers.borrow().iter().position(|registered| Rc::ptr_eq(registered, &provider));
            if let Some(position) = position {
                this.link_providers.borrow_mut().remove(position);
            }
            if this.link_providers.borrow().is_empty() {
                this.proxy.rpc_stop_link_provider();
            }
        }))
    }

    /// `getTerminalById(id)`.
    pub fn get_terminal_by_id(&self, id: u32) -> Option<Rc<ExtHostTerminal>> {
        self.get_terminal_object_by_id(&ExtHostTerminalIdentifier::Id(id))
    }

    /// `getTerminalIdByApiObject(terminal)`: o índice do terminal na lista (ver as divergências no topo do módulo).
    pub fn get_terminal_id_by_api_object(&self, terminal: &Rc<VscodeTerminal>) -> Option<usize> {
        self.terminals.borrow().iter().position(|item| Rc::ptr_eq(&item.value, terminal))
    }

    fn get_terminal_object_by_id(&self, id: &ExtHostTerminalIdentifier) -> Option<Rc<ExtHostTerminal>> {
        let index = self.get_terminal_object_index_by_id(id)?;
        self.terminals.borrow().get(index).cloned()
    }

    fn get_terminal_object_index_by_id(&self, id: &ExtHostTerminalIdentifier) -> Option<usize> {
        self.terminals.borrow().iter().position(|item| item.id() == *id)
    }

    fn on_process_exit(&self, id: u32, exit_code: Option<i32>) {
        self.clean_up_terminal_process(id);

        // Send exit event to main side
        self.proxy.rpc_send_process_exit(id, exit_code);
    }

    fn clean_up_terminal_process(&self, id: u32) {
        self.bufferer.stop_buffering(id);

        // Remove process reference
        self.terminal_processes.borrow_mut().remove(&id);
        self.extension_terminal_awaiting_start.borrow_mut().remove(&id);

        // Clean up process disposables
        let process_disposable = self.terminal_process_disposables.borrow_mut().remove(&id);
        if let Some(process_disposable) = process_disposable {
            process_disposable.dispose();
        }
    }

    /// `getEnvironmentVariableCollection(extension)`.
    pub fn get_environment_variable_collection(&self, extension: &ExtensionDescription) -> Rc<ScopedEnvironmentVariableCollection> {
        let existing = self.environment_variable_collections.borrow().get(&extension.identifier.value).cloned();
        let collection = match existing {
            Some(collection) => collection,
            None => {
                let collection = UnifiedEnvironmentVariableCollection::new(None);
                self.base.register(collection.clone());
                self.set_environment_variable_collection(&extension.identifier.value, collection.clone());
                collection
            }
        };
        collection.get_scoped_environment_variable_collection(None)
    }

    fn sync_environment_variable_collection(&self, extension_identifier: &str, collection: &UnifiedEnvironmentVariableCollection) {
        let serialized = collection.serialized_map();
        let serialized_description = collection.serialized_description_map();
        let is_empty = serialized.is_empty();
        self.proxy.rpc_set_environment_variable_collection(extension_identifier, collection.persistent(), if is_empty { None } else { Some(serialized) }, serialized_description);
    }

    fn set_environment_variable_collection(&self, extension_identifier: &str, collection: Rc<UnifiedEnvironmentVariableCollection>) {
        self.environment_variable_collections.borrow_mut().insert(extension_identifier.to_owned(), collection.clone());
        let (this, extension_identifier, changed) = (self.this.clone(), extension_identifier.to_owned(), collection.clone());
        self.base.register(collection.on_did_change_collection().listen(move |()| {
            // When any collection value changes send this immediately, this is done to ensure
            // following calls to createTerminal will be created with the new environment. It will
            // result in more noise by sending multiple updates when called but collections are
            // expected to be small.
            if let Some(this) = this.upgrade() {
                this.sync_environment_variable_collection(&extension_identifier, &changed);
            }
        }));
    }

    /// `isProposedApiEnabled(extension, 'terminalTitle')`.
    fn is_proposed(extension: &ExtensionDescription) -> bool {
        is_proposed_api_enabled(extension, "terminalTitle")
    }
}

impl<S: ExtHostTerminalCreation> IDisposable for BaseExtHostTerminalService<S> {
    fn dispose(&self) {
        self.base.dispose();
    }
}

/// `TerminalQuickFix.from(quickFix, converter, disposables)`.
fn terminal_quick_fix_from(quick_fix: &TerminalQuickFixItem, converter: &dyn ICommandsConverter, disposables: &DisposableStore) -> Result<Option<TerminalQuickFix>, Error> {
    Ok(match quick_fix {
        TerminalQuickFixItem::TerminalCommand { terminal_command, should_execute } => Some(TerminalQuickFix::TerminalCommand(ITerminalQuickFixTerminalCommandDto {
            terminal_command: terminal_command.clone(),
            should_execute: *should_execute,
        })),
        TerminalQuickFixItem::Opener { uri } => Some(TerminalQuickFix::Opener(ITerminalQuickFixOpenerDto { uri: uri.clone() })),
        TerminalQuickFixItem::Command(command) => converter.to_internal(Some(command), disposables)?.map(TerminalQuickFix::Command),
    })
}

/// `TerminalCompletionItemDto.from(item)`.
fn terminal_completion_item_dto_from(item: &TerminalCompletionItem) -> ITerminalCompletionItemDto {
    ITerminalCompletionItemDto {
        label: item.label.clone(),
        detail: item.detail.clone(),
        documentation: MarkdownStringConverter::from_strict(item.documentation.as_ref()),
        icon: None,
        kind: item.kind.map(|kind| kind.value()),
        is_file: item.is_file,
        is_directory: item.is_directory,
        is_keyword: item.is_keyword,
        replacement_range: item.replacement_range,
    }
}

/// `TerminalCompletionList.from(completions, pathSeparator)`.
fn terminal_completion_list_from(completions: &TerminalCompletionResult, path_separator: &str) -> TerminalCompletionListDto {
    match completions {
        TerminalCompletionResult::Items(items) => TerminalCompletionListDto { items: items.iter().map(terminal_completion_item_dto_from).collect(), resource_options: None },
        TerminalCompletionResult::List(list) => TerminalCompletionListDto {
            items: list.items.iter().map(terminal_completion_item_dto_from).collect(),
            resource_options: list.resource_options.as_ref().map(|options| TerminalCompletionResourceOptionsDto {
                show_files: options.show_files,
                show_directories: options.show_directories,
                cwd: options.cwd.clone(),
                path_separator: path_separator.to_owned(),
            }),
        },
    }
}

impl<S: ExtHostTerminalCreation> ExtHostTerminalServiceShape for BaseExtHostTerminalService<S> {
    fn rpc_accept_active_terminal_changed(&self, id: Option<u32>) -> Promise<()> {
        let original = self.active_terminal.borrow().clone();
        let Some(id) = id else {
            *self.active_terminal.borrow_mut() = None;
            if original.is_some() {
                self.on_did_change_active_terminal_emitter.fire(None);
            }
            return Promise::resolved(());
        };
        if let Some(terminal) = self.get_terminal_by_id(id) {
            *self.active_terminal.borrow_mut() = Some(terminal.clone());
            if !original.is_some_and(|original| Rc::ptr_eq(&original, &terminal)) {
                self.on_did_change_active_terminal_emitter.fire(Some(terminal.value()));
            }
        }
        Promise::resolved(())
    }

    fn rpc_accept_terminal_process_data(&self, id: u32, data: &str) -> Promise<()> {
        if let Some(terminal) = self.get_terminal_by_id(id) {
            self.on_did_write_terminal_data_emitter.fire((terminal.value(), data.to_owned()));
        }
        Promise::resolved(())
    }

    fn rpc_accept_terminal_dimensions(&self, id: u32, cols: i32, rows: i32) -> Promise<()> {
        if let Some(terminal) = self.get_terminal_by_id(id) {
            if terminal.set_dimensions(cols, rows) {
                let value = terminal.value();
                let dimensions = value.dimensions().expect("as dimensões acabaram de ser definidas");
                self.on_did_change_terminal_dimensions_emitter.fire((value, dimensions));
            }
        }
        Promise::resolved(())
    }

    fn rpc_accept_did_execute_command(&self, id: u32, command: ITerminalCommandDto) -> Promise<()> {
        if let Some(terminal) = self.get_terminal_by_id(id) {
            self.on_did_execute_command_emitter.fire((terminal.value(), command));
        }
        Promise::resolved(())
    }

    fn rpc_accept_terminal_maximum_dimensions(&self, id: u32, cols: i32, rows: i32) -> Promise<()> {
        // Extension pty terminal only - when virtual process resize fires it means that the
        // terminal's maximum dimensions changed
        let process = self.terminal_processes.borrow().get(&id).cloned();
        match process.map(|process| process.process().resize(cols, rows, None, None)) {
            Some(Err(error)) => Promise::rejected(error),
            _ => Promise::resolved(()),
        }
    }

    fn rpc_accept_terminal_title_change(&self, id: u32, name: &str) -> Promise<()> {
        if let Some(terminal) = self.get_terminal_by_id(id) {
            terminal.set_name(name);
        }
        Promise::resolved(())
    }

    fn rpc_accept_terminal_closed(&self, id: u32, exit_code: Option<i32>, exit_reason: PlatformExitReason) -> Promise<()> {
        self.clean_up_terminal_process(id);

        // Release any cached terminal links and cancel in-flight link providers for this terminal
        self.terminal_link_cache.borrow_mut().remove(&id);
        let cancellation_source = self.terminal_link_cancellation_source.borrow_mut().remove(&id);
        if let Some(cancellation_source) = cancellation_source {
            cancellation_source.dispose_with(true);
        }

        let index = self.get_terminal_object_index_by_id(&ExtHostTerminalIdentifier::Id(id));
        if let Some(index) = index {
            let terminal = self.terminals.borrow_mut().remove(index);
            terminal.set_exit_status(exit_code, exit_reason);
            self.on_did_close_terminal_emitter.fire(terminal.value());
        }
        Promise::resolved(())
    }

    fn rpc_accept_terminal_opened(&self, id: u32, ext_host_terminal_id: Option<&str>, name: &str, shell_launch_config: IShellLaunchConfigDto) {
        if let Some(ext_host_terminal_id) = ext_host_terminal_id {
            // Resolve with the renderer generated id
            let index = self.get_terminal_object_index_by_id(&ExtHostTerminalIdentifier::Uuid(ext_host_terminal_id.to_owned()));
            if let Some(index) = index {
                // The terminal has already been created (via createTerminal*), only fire the event
                let terminal = self.terminals.borrow()[index].clone();
                terminal.set_id(ExtHostTerminalIdentifier::Id(id));
                self.on_did_open_terminal_emitter.fire(terminal.value());
                terminal.set_is_open(true);
                return;
            }
        }

        let creation_options = TerminalOptions {
            name: shell_launch_config.name,
            shell_path: shell_launch_config.executable,
            shell_args: shell_launch_config.args,
            cwd: shell_launch_config.cwd,
            env: shell_launch_config.env,
            hide_from_user: shell_launch_config.hide_from_user,
            title_template: shell_launch_config.title_template,
            ..TerminalOptions::default()
        };
        let terminal = ExtHostTerminal::new(self.proxy.clone(), ExtHostTerminalIdentifier::Id(id), TerminalCreationOptions::Terminal(creation_options), Some(name.to_owned()));
        self.terminals.borrow_mut().push(terminal.clone());
        self.on_did_open_terminal_emitter.fire(terminal.value());
        terminal.set_is_open(true);
    }

    fn rpc_accept_terminal_process_id(&self, id: u32, process_id: i32) -> Promise<()> {
        if let Some(terminal) = self.get_terminal_by_id(id) {
            terminal.core.set_process_id(Some(process_id));
        }
        Promise::resolved(())
    }

    fn rpc_start_extension_terminal(&self, id: u32, initial_dimensions: Option<ITerminalDimensionsDto>) -> Promise<Option<ITerminalLaunchError>> {
        let this = self.rc();
        async_fn(async move {
            // Make sure the ExtHostTerminal exists so onDidOpenTerminal has fired before we call
            // Pseudoterminal.start
            let Some(terminal) = this.get_terminal_by_id(id) else {
                return Ok(Some(ITerminalLaunchError {
                    message: localize("launchFail.idMissingOnExtHost", "Could not find the terminal with id {0} on the extension host", &[&id.to_string()]),
                    code: None,
                }));
            };

            // Wait for onDidOpenTerminal to fire
            if !terminal.is_open() {
                let (opened, resolver) = Promise::<()>::new();
                let listener_slot: Rc<RefCell<Option<DisposableRef>>> = Rc::default();
                let (slot, target) = (listener_slot.clone(), terminal.value());
                // Ensure open is called after onDidOpenTerminal
                let listener = this.on_did_open_terminal().listen(move |e| {
                    if Rc::ptr_eq(e, &target) {
                        let listener = slot.borrow_mut().take();
                        if let Some(listener) = listener {
                            listener.dispose();
                        }
                        resolver.resolve(());
                    }
                });
                *listener_slot.borrow_mut() = Some(listener);
                opened.await?;
            }

            let terminal_process = this.terminal_processes.borrow().get(&id).cloned();
            match terminal_process {
                Some(TerminalChildProcess::Pseudoterminal(pty)) => pty.start_sending_events(initial_dimensions),
                _ => {
                    // Defer startSendingEvents call to when _setupExtHostProcessListeners is called
                    this.extension_terminal_awaiting_start.borrow_mut().insert(id, initial_dimensions);
                }
            }

            Ok(None)
        })
    }

    fn rpc_accept_process_ack_data_event(&self, id: u32, char_count: usize) {
        let process = self.terminal_processes.borrow().get(&id).cloned();
        if let Some(process) = process {
            process.process().acknowledge_data_event(char_count);
        }
    }

    fn rpc_accept_process_input(&self, id: u32, data: &str) {
        let process = self.terminal_processes.borrow().get(&id).cloned();
        if let Some(process) = process {
            process.process().input(data);
        }
    }

    fn rpc_accept_terminal_interaction(&self, id: u32) {
        if let Some(terminal) = self.get_terminal_by_id(id) {
            if terminal.set_interacted_with() {
                self.on_did_change_terminal_state_emitter.fire(terminal.value());
            }
        }
    }

    fn rpc_accept_terminal_selection(&self, id: u32, selection: Option<&str>) {
        if let Some(terminal) = self.get_terminal_by_id(id) {
            terminal.set_selection(selection);
        }
    }

    fn rpc_accept_process_resize(&self, id: u32, cols: i32, rows: i32) {
        let process = self.terminal_processes.borrow().get(&id).cloned();
        if let Some(process) = process {
            if let Err(error) = process.process().resize(cols, rows, None, None) {
                // We tried to write to a closed pipe / channel.
                if !matches!(error.prop("code"), Some("EPIPE" | "ERR_IPC_CHANNEL_CLOSED")) {
                    errors::throw(error);
                }
            }
        }
    }

    fn rpc_accept_process_shutdown(&self, id: u32, immediate: bool) {
        let process = self.terminal_processes.borrow().get(&id).cloned();
        if let Some(process) = process {
            process.process().shutdown(immediate);
        }
    }

    fn rpc_accept_process_request_initial_cwd(&self, id: u32) {
        let process = self.terminal_processes.borrow().get(&id).cloned();
        if let Some(process) = process {
            let proxy = self.proxy.clone();
            drop(process.process().get_initial_cwd().then(move |initial_cwd| proxy.rpc_send_process_property(id, IProcessProperty::InitialCwd(initial_cwd))));
        }
    }

    fn rpc_accept_process_request_cwd(&self, id: u32) {
        let process = self.terminal_processes.borrow().get(&id).cloned();
        if let Some(process) = process {
            let proxy = self.proxy.clone();
            drop(process.process().get_cwd().then(move |cwd| proxy.rpc_send_process_property(id, IProcessProperty::Cwd(cwd))));
        }
    }

    fn rpc_accept_process_request_latency(&self, id: u32) -> Promise<u32> {
        Promise::resolved(id)
    }

    fn rpc_accept_terminal_shell_type(&self, id: u32, shell_type: TerminalShellType) {
        if let Some(terminal) = self.get_terminal_by_id(id) {
            if terminal.set_shell_type(shell_type) {
                self.on_did_change_terminal_state_emitter.fire(terminal.value());
            }
        }
    }

    fn rpc_provide_terminal_completions(&self, id: &str, options: ITerminalCompletionContextDto, _token: CancellationToken) -> Promise<Option<TerminalCompletionListDto>> {
        let this = self.rc();
        let id = id.to_owned();
        async_fn(async move {
            let token = CancellationTokenSource::new(None).token();
            let active_terminal = this.active_terminal();
            let Some(active_terminal) = active_terminal.filter(|_| !token.is_cancellation_requested()) else {
                return Ok(None);
            };

            let provider = this.completion_providers.borrow().get(&id).cloned();
            let Some(provider) = provider else {
                return Ok(None);
            };

            let completions = await_maybe(provider.provide_terminal_completions(&active_terminal, &options, token)).await?;
            let Some(completions) = completions else {
                return Ok(None);
            };
            let git_bash = active_terminal.state().shell == Some(ShellType::Windows(WindowsShellType::GitBash));
            let path_separator = if !Platform::detect().is_windows || git_bash { "/" } else { "\\" };
            Ok(Some(terminal_completion_list_from(&completions, path_separator)))
        })
    }

    fn rpc_provide_terminal_quick_fixes(
        &self,
        id: &str,
        match_result: TerminalCommandMatchResultDto,
        _token: CancellationToken,
    ) -> Promise<Option<OneOrMany<TerminalQuickFix>>> {
        let this = self.rc();
        let id = id.to_owned();
        async_fn(async move {
            let token = CancellationTokenSource::new(None).token();
            if token.is_cancellation_requested() {
                return Ok(None);
            }
            let provider = this.quick_fix_providers.borrow().get(&id).cloned();
            let Some(provider) = provider else {
                return Ok(None);
            };
            let quick_fixes = await_maybe(provider.provide_terminal_quick_fixes(&match_result, token)).await?;
            let quick_fixes = match quick_fixes {
                None => return Ok(None),
                Some(TerminalQuickFixResult::Many(fixes)) if fixes.is_empty() => return Ok(None),
                Some(quick_fixes) => quick_fixes,
            };

            let store = DisposableStore::new();
            this.last_quick_fix_commands.set(Some(store.clone() as Rc<dyn IDisposable>));
            let converter = this.ext_host_commands.converter();

            match quick_fixes {
                // Single
                TerminalQuickFixResult::Single(fix) => Ok(terminal_quick_fix_from(&fix, &*converter, &store)?.map(OneOrMany::One)),
                // Many
                TerminalQuickFixResult::Many(fixes) => {
                    let mut result = Vec::new();
                    for fix in &fixes {
                        if let Some(converted) = terminal_quick_fix_from(fix, &*converter, &store)? {
                            result.push(converted);
                        }
                    }
                    Ok(Some(OneOrMany::Many(result)))
                }
            }
        })
    }

    fn rpc_create_contributed_profile_terminal(&self, id: &str, options: ICreateContributedTerminalProfileOptions) -> Promise<()> {
        let this = self.rc();
        let id = id.to_owned();
        async_fn(async move {
            let token = CancellationTokenSource::new(None).token();
            let profile_provider_data = this.profile_providers.borrow().get(&id).cloned();
            let Some((provider, extension)) = profile_provider_data else {
                return Err(Error::new(format!("No terminal profile provider registered for id \"{id}\"")));
            };
            let profile = await_maybe(provider.provide_terminal_profile(token.clone())).await?;
            if token.is_cancellation_requested() {
                return Ok(());
            }
            let Some(profile) = profile else {
                return Err(Error::new(format!("No terminal profile options provided for id \"{id}\"")));
            };
            let mut profile = match profile {
                TerminalProfileResult::Profile(profile) => profile,
                TerminalProfileResult::Options(options) => TerminalProfile { options },
            };

            let has_terminal_title_proposal = Self::is_proposed(&extension);
            if !has_terminal_title_proposal && profile.options.title_template().is_some() {
                eprintln!(
                    "[{}] `titleTemplate` returned from TerminalProfileProvider is ignored because the `terminalTitle` proposed API is not enabled.",
                    extension.identifier.value
                );
                profile = TerminalProfile { options: profile.options.with_title_template(None) };
            }
            // options.titleTemplate is not explicitly stripped here because the profileOptions
            // assignment below only applies it when hasTerminalTitleProposal is true.
            if !has_terminal_title_proposal && options.title_template.is_some() {
                eprintln!(
                    "[{}] `titleTemplate` passed to createContributedTerminalProfile is ignored because the `terminalTitle` proposed API is not enabled.",
                    extension.identifier.value
                );
            }

            let requested_title_template = options.title_template.as_deref().filter(|template| !template.is_empty());
            let profile_options = match requested_title_template {
                Some(template) if has_terminal_title_proposal && profile.options.title_template().is_none_or(str::is_empty) => {
                    profile.options.with_title_template(Some(template.to_owned()))
                }
                _ => profile.options.clone(),
            };

            let internal_options = ITerminalInternalOptions::from(&options);
            match profile_options {
                TerminalCreationOptions::Extension(extension_options) => {
                    this.create_extension_terminal(&extension_options, Some(internal_options));
                }
                TerminalCreationOptions::Terminal(terminal_options) => {
                    this.create_terminal_from_options(terminal_options, Some(internal_options))?;
                }
            }
            Ok(())
        })
    }

    fn rpc_provide_links(&self, terminal_id: u32, line: &str) -> Promise<Vec<ITerminalLinkDto>> {
        let this = self.rc();
        let line = line.to_owned();
        async_fn(async move {
            let Some(terminal) = this.get_terminal_by_id(terminal_id) else {
                return Ok(Vec::new());
            };

            // Discard any cached links the terminal has been holding, currently all links are released
            // when new links are provided.
            this.terminal_link_cache.borrow_mut().remove(&terminal_id);

            let old_token = this.terminal_link_cancellation_source.borrow().get(&terminal_id).cloned();
            if let Some(old_token) = old_token {
                old_token.dispose_with(true);
            }
            let cancellation_source = CancellationTokenSource::new(None);
            this.terminal_link_cancellation_source.borrow_mut().insert(terminal_id, cancellation_source.clone());

            let context = TerminalLinkContext { terminal: terminal.value(), line };
            let token = cancellation_source.token();
            let providers: Vec<Rc<dyn TerminalLinkProvider>> = this.link_providers.borrow().clone();
            let mut promises: Vec<Promise<ProviderLinks>> = Vec::new();

            for provider in providers {
                let (context, token) = (context.clone(), token.clone());
                promises.push(promises::with_async_body(move |r: Resolver<ProviderLinks>| {
                    async_fn(async move {
                        let cancel_subscription = {
                            let (r, provider) = (r.clone(), provider.clone());
                            token.on_cancellation_requested().listen(move |()| r.resolve(ProviderLinks { provider: provider.clone(), links: Vec::new() }))
                        };
                        let provided = await_maybe(provider.provide_terminal_links(&context, token.clone())).await;
                        cancel_subscription.dispose();
                        let links = provided?.unwrap_or_default();
                        if !token.is_cancellation_requested() {
                            r.resolve(ProviderLinks { provider, links });
                        }
                        Ok(())
                    })
                }));
            }

            let provide_results = Promise::all(promises).await?;

            if token.is_cancellation_requested() {
                return Ok(Vec::new());
            }

            let mut result = Vec::new();
            let mut cache_link_map: HashMap<u32, CachedLinkEntry> = HashMap::new();
            for provide_result in provide_results {
                for provider_link in provide_result.links {
                    let id = this.next_link_id.get();
                    this.next_link_id.set(id + 1);
                    result.push(ITerminalLinkDto { id, start_index: provider_link.start_index, length: provider_link.length, label: provider_link.tooltip.clone() });
                    cache_link_map.insert(id, CachedLinkEntry { provider: provide_result.provider.clone(), link: provider_link });
                }
            }

            this.terminal_link_cache.borrow_mut().insert(terminal_id, cache_link_map);

            Ok(result)
        })
    }

    fn rpc_activate_link(&self, terminal_id: u32, link_id: u32) {
        let cached_link = self.terminal_link_cache.borrow().get(&terminal_id).and_then(|links| links.get(&link_id)).cloned();
        let Some(cached_link) = cached_link else {
            return;
        };
        cached_link.provider.handle_terminal_link(&cached_link.link);
    }

    fn rpc_init_environment_variable_collections(&self, collections: Vec<(String, ISerializableEnvironmentVariableCollection)>) {
        for (extension_identifier, serialized) in collections {
            let collection = UnifiedEnvironmentVariableCollection::new(Some(serialized));
            self.base.register(collection.clone());
            self.set_environment_variable_collection(&extension_identifier, collection);
        }
    }

    fn rpc_accept_default_profile(&self, profile: ITerminalProfile, automation_profile: ITerminalProfile) {
        let old_profile = self.default_profile.borrow().clone();
        *self.default_profile.borrow_mut() = Some(profile.clone());
        *self.default_automation_profile.borrow_mut() = Some(automation_profile);
        if old_profile.map(|old| old.path) != Some(profile.path.clone()) {
            self.on_did_change_shell_emitter.fire(profile.path);
        }
    }
}

// ---- o serviço de worker

/// A estratégia de criação do `WorkerExtHostTerminalService`: só cria terminais quando há uma autoridade remota.
pub struct WorkerTerminalCreation {
    has_remote_authority: bool,
}

impl ExtHostTerminalCreation for WorkerTerminalCreation {
    fn create_terminal(
        &self,
        service: &BaseExtHostTerminalService<Self>,
        name: Option<String>,
        shell_path: Option<String>,
        shell_args: Option<OneOrMany<String>>,
    ) -> Result<Rc<VscodeTerminal>, Error> {
        if !self.has_remote_authority {
            return Err(Error::new("NotSupported"));
        }
        service.create_terminal_from_options(TerminalOptions { name, shell_path, shell_args, ..TerminalOptions::default() }, None)
    }

    fn create_terminal_from_options(
        &self,
        service: &BaseExtHostTerminalService<Self>,
        options: TerminalOptions,
        internal_options: Option<ITerminalInternalOptions>,
    ) -> Result<Rc<VscodeTerminal>, Error> {
        if !self.has_remote_authority {
            return Err(Error::new("NotSupported"));
        }
        let terminal = ExtHostTerminal::new(
            service.proxy.clone(),
            ExtHostTerminalIdentifier::Uuid(generate_uuid()),
            TerminalCreationOptions::Terminal(options.clone()),
            options.name.clone(),
        );
        service.terminals.borrow_mut().push(terminal.clone());
        let mut internal_options = internal_options;
        let serialized = service.serialize_parent_terminal(options.location.as_ref(), internal_options.as_mut());
        drop(terminal.create(&options, Some(&serialized)));
        Ok(terminal.value())
    }
}

/// `WorkerExtHostTerminalService`.
pub type WorkerExtHostTerminalService = BaseExtHostTerminalService<WorkerTerminalCreation>;

impl WorkerExtHostTerminalService {
    pub fn new(ext_host_commands: Rc<dyn IExtHostCommands>, ext_host_rpc: &dyn IMainContext, init_data: &dyn IExtHostInitDataService) -> Rc<Self> {
        let has_remote_authority = init_data.remote_authority().is_some_and(|authority| !authority.is_empty());
        Self::with_creation(false, ext_host_commands, ext_host_rpc, WorkerTerminalCreation { has_remote_authority })
    }
}
