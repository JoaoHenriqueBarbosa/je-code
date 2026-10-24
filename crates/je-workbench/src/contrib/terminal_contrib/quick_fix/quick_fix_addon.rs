//! Porte de `src/vs/workbench/contrib/terminalContrib/quickFix/browser/quickFixAddon.ts`: o addon que resolve os quick fixes de
//! cada comando terminado (`TerminalQuickFixAddon`) e a função que casa as opções com um comando
//! ([`get_quick_fixes_for_command`]).
//!
//! Decisões de tradução:
//! - **Fora do porte: a decoração e o menu.** `_registerQuickFixDecoration` (o `registerDecoration` do xterm com o elemento DOM,
//!   a lâmpada, o clique e o sinal de acessibilidade), `showMenu` (o `IActionWidgetService`), `TerminalQuickFixItem`,
//!   `toActionWidgetItems`, `getQuickFixIcon` e o ouvinte de `ShellIntegrationQuickFixEnabled` que limpa a decoração dependem de
//!   decorações do xterm e do widget de ações, que a UI nativa ainda não tem. O addon dispara `onDidUpdateQuickFixes` com as
//!   ações, que é o que a decoração consumiria, e por isso não recebe o `IConfigurationService`, o
//!   `IAccessibilitySignalService` nem o `IActionWidgetService` do construtor original.
//! - `@xterm/xterm`'s `Terminal` é [`IHeadlessTerminal`] (`buffer.active`, `cols`).
//! - [`ITerminalAction`] é uma struct (o `IAction` do original com `type`, `kind`, `source`, `uri`, `command` e `shouldExecute`):
//!   `run` devolve `()`, porque o original descarta o resultado das promessas dos serviços de abertura e de comando.
//! - `getQuickFixesForCommand` é `async` e devolve `Result`: o `throw new Error('No resolved fix provider')` é um `Err`, e o
//!   `return` de dentro do laço do tipo `Opener` com `uri` ausente não existe (o `uri` é obrigatório no tipo).
//! - O `Map<string, ...[]>` com ordem de inserção é um [`IndexMap`].
//! - O `new CancellationTokenSource().token` de cada chamada é um [`CancellationTokenSource`] novo.
//! - O `_register(dom.addDisposableListener(...))` do clique não existe sem DOM.

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::{Rc, Weak};

use indexmap::IndexMap;
use je_base::async_::async_fn;
use je_base::cancellation::CancellationTokenSource;
use je_base::dynamic::Dynamic;
use je_base::errors::Error;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::network::schemas;
use je_base::nls;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_platform::commands::commands::ICommandService;
use je_platform::label::label::{ILabelService, UriLabelOptions};
use je_platform::opener::opener::{IOpenerService, OpenResource};
use je_platform::telemetry::telemetry::ITelemetryService;
use je_platform::terminal::capabilities::capabilities::{CommandDetection, ITerminalCapabilityStore, ITerminalCommand, TerminalCapability};
use je_platform::terminal::capabilities::command_detection_capability::get_lines_for_command;
use je_platform::terminal::capabilities::terminal_capability_store::TerminalCapabilityStoreExt;
use je_platform::terminal::xterm_headless::IHeadlessTerminal;

use super::quick_fix::{
    CommandExitResult, ITerminalCommandMatchResult, ITerminalCommandSelector, ITerminalQuickFixOptions, ITerminalQuickFixProviderSelector, ITerminalQuickFixService,
    QuickFixHandler, QuickFixKind, QuickFixOption, QuickFixOptionType, TerminalQuickFixActionInternal, TerminalQuickFixType, match_text, matcher_to_string,
};
use crate::services::extensions::extensions::IExtensionService;

/// `{ command: string; shouldExecute?: boolean }` do `onDidRequestRerunCommand`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RerunCommandRequest {
    pub command: String,
    pub should_execute: Option<bool>,
}

/// `ITerminalAction`.
#[derive(Clone)]
pub struct ITerminalAction {
    pub fix_type: TerminalQuickFixType,
    pub kind: Option<QuickFixKind>,
    pub source: String,
    pub uri: Option<URI>,
    pub command: Option<String>,
    pub should_execute: Option<bool>,
    // O que sobra do `IAction`.
    pub id: String,
    pub label: String,
    pub tooltip: String,
    pub class: Option<String>,
    pub enabled: bool,
    pub run: Rc<dyn Fn()>,
}

/// `{ command: ITerminalCommand; actions: ITerminalAction[] | undefined }` do `onDidUpdateQuickFixes`.
#[derive(Clone)]
pub struct QuickFixesUpdate {
    pub command: Rc<dyn ITerminalCommand>,
    pub actions: Option<Vec<ITerminalAction>>,
}

/// `ITerminalQuickFixAddon`, sem `showMenu` (ver o topo do arquivo).
pub trait ITerminalQuickFixAddon {
    fn on_did_request_rerun_command(&self) -> Event<RerunCommandRequest>;
    fn on_did_update_quick_fixes(&self) -> Event<QuickFixesUpdate>;
    /// Registers a listener on onCommandFinished scoped to a particular command or regular
    /// expression and provides a callback to be executed for commands that match.
    fn register_command_finished_listener(&self, options: QuickFixOption);
}

/// Os serviços que o construtor do original recebe por injeção (os que o porte usa).
pub struct TerminalQuickFixAddonServices {
    pub command_service: Rc<dyn ICommandService>,
    pub extension_service: Rc<dyn IExtensionService>,
    pub label_service: Rc<dyn ILabelService>,
    pub opener_service: Rc<dyn IOpenerService>,
    pub telemetry_service: Rc<dyn ITelemetryService>,
    pub quick_fix_service: Rc<dyn ITerminalQuickFixService>,
}

type CommandListeners = IndexMap<String, Vec<QuickFixOption>>;

struct Inner {
    session_id: String,
    aliases: Option<Vec<Vec<String>>>,
    capabilities: Rc<dyn ITerminalCapabilityStore>,
    services: TerminalQuickFixAddonServices,

    terminal: RefCell<Option<Rc<dyn IHeadlessTerminal>>>,
    command_listeners: RefCell<CommandListeners>,
    quick_fixes: RefCell<Option<Vec<ITerminalAction>>>,
    last_quick_fix_id: RefCell<Option<String>>,
    registered_selectors: RefCell<HashSet<String>>,
    did_run: Cell<bool>,

    on_did_request_rerun_command: Rc<Emitter<RerunCommandRequest>>,
    on_did_update_quick_fixes: Rc<Emitter<QuickFixesUpdate>>,
}

/// `TerminalQuickFixAddon`.
pub struct TerminalQuickFixAddon {
    base: Rc<Disposable>,
    inner: Rc<Inner>,
}

impl TerminalQuickFixAddon {
    pub fn new(session_id: &str, aliases: Option<Vec<Vec<String>>>, capabilities: Rc<dyn ITerminalCapabilityStore>, services: TerminalQuickFixAddonServices) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_request_rerun_command = Emitter::new();
        base.register(on_did_request_rerun_command.clone());
        let on_did_update_quick_fixes = Emitter::new();
        base.register(on_did_update_quick_fixes.clone());
        let inner = Rc::new(Inner {
            session_id: session_id.to_owned(),
            aliases,
            capabilities: capabilities.clone(),
            services,
            terminal: RefCell::new(None),
            command_listeners: RefCell::new(IndexMap::new()),
            quick_fixes: RefCell::new(None),
            last_quick_fix_id: RefCell::new(None),
            registered_selectors: RefCell::new(HashSet::new()),
            did_run: Cell::new(false),
            on_did_request_rerun_command,
            on_did_update_quick_fixes,
        });

        if capabilities.has(TerminalCapability::CommandDetection) {
            Self::register_command_handlers(&inner, &Rc::downgrade(&base));
        } else {
            let inner = inner.clone();
            let weak_base = Rc::downgrade(&base);
            base.register(capabilities.on_did_add_command_detection_capability().listen(move |_| {
                Self::register_command_handlers(&inner, &weak_base);
            }));
        }
        {
            let weak = Rc::downgrade(&inner);
            base.register(inner.services.quick_fix_service.on_did_register_provider().listen(move |result: &ITerminalQuickFixProviderSelector| {
                if let Some(inner) = weak.upgrade() {
                    inner.register_command_finished_listener(convert_to_quick_fix_options(result));
                }
            }));
        }
        {
            let weak = Rc::downgrade(&inner);
            inner.services.quick_fix_service.extension_quick_fixes().then(move |quick_fix_selectors| {
                if let Some(inner) = weak.upgrade() {
                    for selector in quick_fix_selectors {
                        inner.register_command_selector(&selector);
                    }
                }
            });
        }
        {
            let weak = Rc::downgrade(&inner);
            base.register(inner.services.quick_fix_service.on_did_register_command_selector().listen(move |selector: &ITerminalCommandSelector| {
                if let Some(inner) = weak.upgrade() {
                    inner.register_command_selector(selector);
                }
            }));
        }
        {
            let weak = Rc::downgrade(&inner);
            base.register(inner.services.quick_fix_service.on_did_unregister_provider().listen(move |id: &String| {
                if let Some(inner) = weak.upgrade() {
                    inner.command_listeners.borrow_mut().shift_remove(id);
                }
            }));
        }
        Rc::new(Self { base, inner })
    }

    /// `activate(terminal)` do `ITerminalAddon`.
    pub fn activate(&self, terminal: Rc<dyn IHeadlessTerminal>) {
        *self.inner.terminal.borrow_mut() = Some(terminal);
    }

    pub fn register_command_selector(&self, selector: &ITerminalCommandSelector) {
        self.inner.register_command_selector(selector);
    }

    fn register_command_handlers(inner: &Rc<Inner>, base: &Weak<Disposable>) {
        let command_detection = inner.capabilities.get::<CommandDetection>();
        if inner.terminal.borrow().is_none() {
            return;
        }
        let (Some(command_detection), Some(base)) = (command_detection, base.upgrade()) else {
            return;
        };
        let inner = inner.clone();
        base.register(command_detection.on_command_finished().listen(move |command| {
            let inner = inner.clone();
            let command = command.clone();
            // `async command => await this._resolveQuickFixes(command, this._aliases)`
            async_fn(async move { Inner::resolve_quick_fixes(&inner, command).await });
        }));
    }
}

impl ITerminalQuickFixAddon for TerminalQuickFixAddon {
    fn on_did_request_rerun_command(&self) -> Event<RerunCommandRequest> {
        self.inner.on_did_request_rerun_command.event()
    }

    fn on_did_update_quick_fixes(&self) -> Event<QuickFixesUpdate> {
        self.inner.on_did_update_quick_fixes.event()
    }

    fn register_command_finished_listener(&self, options: QuickFixOption) {
        self.inner.register_command_finished_listener(options);
    }
}

impl IDisposable for TerminalQuickFixAddon {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl Inner {
    fn register_command_selector(&self, selector: &ITerminalCommandSelector) {
        if self.registered_selectors.borrow().contains(&selector.id) {
            return;
        }
        let matcher_key = matcher_to_string(&selector.command_line_matcher);
        self.command_listeners.borrow_mut().entry(matcher_key).or_default().push(QuickFixOption::new(
            &selector.id,
            selector.command_line_matcher.clone(),
            selector.output_matcher.clone(),
            selector.command_exit_result,
            selector.kind,
            QuickFixHandler::Unresolved,
        ));
        self.registered_selectors.borrow_mut().insert(selector.id.clone());
    }

    fn register_command_finished_listener(&self, options: QuickFixOption) {
        let matcher_key = matcher_to_string(&options.command_line_matcher);
        let mut listeners = self.command_listeners.borrow_mut();
        let current_options = listeners.entry(matcher_key).or_default();
        // removes the unresolved options
        current_options.retain(|option| option.id != options.id);
        current_options.push(options);
    }

    /// Resolves quick fixes, if any, based on the command & its output.
    async fn resolve_quick_fixes(this: &Rc<Self>, command: Rc<dyn ITerminalCommand>) -> Result<(), Error> {
        let terminal = this.terminal.borrow().clone();
        let Some(terminal) = terminal else {
            return Ok(());
        };
        if command.was_replayed() == Some(true) {
            return Ok(());
        }
        let last_quick_fix_id = this.last_quick_fix_id.borrow().clone();
        if !command.command().is_empty()
            && let Some(last_quick_fix_id) = last_quick_fix_id
        {
            this.dispose_quick_fix(&command, &last_quick_fix_id);
        }

        let resolver = {
            let this = this.clone();
            let command = command.clone();
            move |selector: &ITerminalQuickFixOptions, lines: Option<Vec<String>>| -> Promise<Vec<TerminalQuickFixActionInternal>> {
                let Some(lines) = lines else {
                    return Promise::resolved(Vec::new());
                };
                let this = this.clone();
                let command = command.clone();
                let selector = selector.clone();
                async_fn(async move {
                    let id = selector.id.clone();
                    this.services.extension_service.activate_by_event(&format!("onTerminalQuickFixRequest:{id}"), None).await?;
                    let provider = this.services.quick_fix_service.providers().borrow().get(&id).cloned();
                    Ok(match provider {
                        Some(provider) => {
                            let resolved = ITerminalQuickFixOptions {
                                option_type: QuickFixOptionType::Resolved,
                                id: selector.id.clone(),
                                command_line_matcher: selector.command_line_matcher.clone(),
                                output_matcher: selector.output_matcher.clone(),
                                command_exit_result: selector.command_exit_result,
                                kind: selector.kind,
                            };
                            provider.provide_terminal_quick_fixes(command, Some(lines), &resolved, CancellationTokenSource::new(None).token()).await?
                        }
                        None => Vec::new(),
                    })
                })
            }
        };
        let listeners = this.command_listeners.borrow().clone();
        let result = get_quick_fixes_for_command(
            this.aliases.as_deref(),
            terminal.as_ref(),
            &command,
            &listeners,
            &this.services.command_service,
            &this.services.opener_service,
            &this.services.label_service,
            Some(&this.on_did_request_rerun_command),
            Some(&resolver as &dyn Fn(&ITerminalQuickFixOptions, Option<Vec<String>>) -> Promise<Vec<TerminalQuickFixActionInternal>>),
        )
        .await?;
        let Some(result) = result else {
            return Ok(());
        };

        *this.last_quick_fix_id.borrow_mut() = Some(result[0].id.clone());
        // `_registerQuickFixDecoration()` não faz parte do porte (ver o topo do arquivo).
        this.on_did_update_quick_fixes.fire(QuickFixesUpdate { command, actions: Some(result) });
        *this.quick_fixes.borrow_mut() = None;
        Ok(())
    }

    fn dispose_quick_fix(&self, command: &Rc<dyn ITerminalCommand>, id: &str) {
        self.services.telemetry_service.public_log2(
            "terminal/quick-fix",
            Some(Dynamic::object([
                ("quickFixId", Dynamic::from(id)),
                ("ranQuickFix", Dynamic::Bool(self.did_run.get())),
                ("terminalSessionId", Dynamic::from(self.session_id.as_str())),
            ])),
        );
        let quick_fixes = self.quick_fixes.borrow_mut().take();
        self.on_did_update_quick_fixes.fire(QuickFixesUpdate { command: command.clone(), actions: quick_fixes });
        *self.last_quick_fix_id.borrow_mut() = None;
        self.did_run.set(false);
    }
}

/// `encodeURI`.
fn encode_uri(text: &str) -> String {
    let mut out = String::new();
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"-_.!~*'();/?:@&=+$,#".contains(&byte) {
            out.push(char::from(byte));
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// `getQuickFixesForCommand(aliases, terminal, terminalCommand, quickFixOptions, commandService, openerService, labelService,
/// onDidRequestRerunCommand?, getResolvedFixes?)`.
#[allow(clippy::too_many_arguments)]
pub async fn get_quick_fixes_for_command(
    _aliases: Option<&[Vec<String>]>,
    terminal: &dyn IHeadlessTerminal,
    terminal_command: &Rc<dyn ITerminalCommand>,
    quick_fix_options: &IndexMap<String, Vec<QuickFixOption>>,
    command_service: &Rc<dyn ICommandService>,
    opener_service: &Rc<dyn IOpenerService>,
    label_service: &Rc<dyn ILabelService>,
    on_did_request_rerun_command: Option<&Rc<Emitter<RerunCommandRequest>>>,
    get_resolved_fixes: Option<&dyn Fn(&ITerminalQuickFixOptions, Option<Vec<String>>) -> Promise<Vec<TerminalQuickFixActionInternal>>>,
) -> Result<Option<Vec<ITerminalAction>>, Error> {
    // Prevent duplicates by tracking added entries
    let mut command_quick_fix_set: HashSet<String> = HashSet::new();
    let mut open_quick_fix_set: HashSet<String> = HashSet::new();

    let mut fixes: Vec<ITerminalAction> = Vec::new();
    let new_command = terminal_command.command();
    let exit_code_is_zero = terminal_command.exit_code() == Some(0.0);
    for options in quick_fix_options.values() {
        for option in options {
            if (option.command_exit_result == CommandExitResult::Success && !exit_code_is_zero) || (option.command_exit_result == CommandExitResult::Error && exit_code_is_zero) {
                continue;
            }
            let quick_fixes: Vec<TerminalQuickFixActionInternal> = match &option.handler {
                QuickFixHandler::Resolved(get_quick_fixes) => {
                    let lines = get_lines_for_command(terminal.buffer_active().as_ref(), terminal_command.as_ref(), terminal.cols(), option.output_matcher.as_ref());
                    get_quick_fixes(terminal_command.clone(), lines, option.options(), CancellationTokenSource::new(None).token()).await?
                }
                QuickFixHandler::Unresolved => {
                    let Some(get_resolved_fixes) = get_resolved_fixes else {
                        return Err(Error::new("No resolved fix provider"));
                    };
                    let lines = option
                        .output_matcher
                        .as_ref()
                        .and_then(|output_matcher| get_lines_for_command(terminal.buffer_active().as_ref(), terminal_command.as_ref(), terminal.cols(), Some(output_matcher)));
                    get_resolved_fixes(option.options(), lines).await?
                }
                QuickFixHandler::Internal(get_quick_fixes) => {
                    let Some(command_line_match) = match_text(&option.command_line_matcher, &new_command) else {
                        continue;
                    };
                    let output_match = option.output_matcher.as_ref().and_then(|output_matcher| terminal_command.get_output_match(output_matcher));
                    if option.output_matcher.is_none() || output_match.is_none() {
                        continue;
                    }
                    let match_result = ITerminalCommandMatchResult { command_line_match, output_match, command_line: terminal_command.command() };
                    get_quick_fixes(&match_result)
                }
            };

            for quick_fix in quick_fixes {
                let action = match quick_fix {
                    TerminalQuickFixActionInternal::TerminalCommand(fix) => {
                        if !command_quick_fix_set.insert(fix.terminal_command.clone()) {
                            continue;
                        }
                        let label = nls::localize("quickFix.command", "Run: {0}", &[fix.terminal_command.as_str()]);
                        let rerun = on_did_request_rerun_command.cloned();
                        let rerun_command = fix.terminal_command.clone();
                        let should_execute = fix.should_execute;
                        ITerminalAction {
                            fix_type: TerminalQuickFixType::TerminalCommand,
                            kind: option.kind,
                            class: None,
                            source: fix.source,
                            id: fix.id,
                            label: label.clone(),
                            enabled: true,
                            run: Rc::new(move || {
                                if let Some(rerun) = &rerun {
                                    rerun.fire(RerunCommandRequest { command: rerun_command.clone(), should_execute: Some(should_execute.unwrap_or(true)) });
                                }
                            }),
                            tooltip: label,
                            command: Some(fix.terminal_command),
                            should_execute: fix.should_execute,
                            uri: None,
                        }
                    }
                    TerminalQuickFixActionInternal::Opener(fix) => {
                        if !open_quick_fix_set.insert(fix.uri.to_string(false).to_string_lossy()) {
                            continue;
                        }
                        let scheme = fix.uri.scheme().to_string_lossy();
                        let is_url = scheme == schemas::HTTP || scheme == schemas::HTTPS;
                        let uri_label = if is_url { encode_uri(&fix.uri.to_string(true).to_string_lossy()) } else { label_service.get_uri_label(&fix.uri, &UriLabelOptions::default()) };
                        let label = nls::localize("quickFix.opener", "Open: {0}", &[uri_label.as_str()]);
                        let opener_service = opener_service.clone();
                        let uri = fix.uri.clone();
                        ITerminalAction {
                            source: fix.source,
                            id: fix.id,
                            label: label.clone(),
                            fix_type: TerminalQuickFixType::Opener,
                            kind: option.kind,
                            class: None,
                            enabled: true,
                            run: Rc::new(move || {
                                opener_service.open(&OpenResource::Uri(uri.clone()), None);
                            }),
                            tooltip: label,
                            uri: Some(fix.uri),
                            command: None,
                            should_execute: None,
                        }
                    }
                    TerminalQuickFixActionInternal::Port(fix) => {
                        let run = fix.run.clone();
                        ITerminalAction {
                            source: "builtin".to_owned(),
                            fix_type: TerminalQuickFixType::Port,
                            kind: option.kind,
                            id: fix.id,
                            label: fix.label,
                            class: fix.class,
                            enabled: fix.enabled,
                            run: Rc::new(move || run()),
                            tooltip: fix.tooltip,
                            uri: None,
                            command: None,
                            should_execute: None,
                        }
                    }
                    TerminalQuickFixActionInternal::VscodeCommand(fix) => {
                        let command_service = command_service.clone();
                        let command_id = fix.id.clone();
                        ITerminalAction {
                            source: fix.source,
                            fix_type: TerminalQuickFixType::VscodeCommand,
                            kind: option.kind,
                            id: fix.id,
                            label: fix.title.clone(),
                            class: None,
                            enabled: true,
                            run: Rc::new(move || {
                                command_service.execute_command(&command_id, Vec::new());
                            }),
                            tooltip: fix.title,
                            uri: None,
                            command: None,
                            should_execute: None,
                        }
                    }
                };
                fixes.push(action);
            }
        }
    }
    Ok(if fixes.is_empty() { None } else { Some(fixes) })
}

/// `convertToQuickFixOptions(selectorProvider)`.
fn convert_to_quick_fix_options(selector_provider: &ITerminalQuickFixProviderSelector) -> QuickFixOption {
    let provider = selector_provider.provider.clone();
    QuickFixOption::new(
        &selector_provider.selector.id,
        selector_provider.selector.command_line_matcher.clone(),
        selector_provider.selector.output_matcher.clone(),
        selector_provider.selector.command_exit_result,
        selector_provider.selector.kind,
        QuickFixHandler::Resolved(Rc::new(move |command, lines, option, token| provider.provide_terminal_quick_fixes(command, lines, option, token))),
    )
}
