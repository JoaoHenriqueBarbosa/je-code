//! Porte de `src/vs/workbench/contrib/terminal/browser/terminalExtensions.ts` e do que as contribuições leem de
//! `terminal.ts`: o `ITerminalContribution`, o contexto que cada contribuição recebe, o registro de contribuições e as
//! visões do emulador (`IXtermTerminal & { raw: RawXtermTerminal }`) que elas usam.
//!
//! Decisões de tradução:
//! - **Construtores com serviços injetados.** `registerTerminalContribution(id, ctor, canRunInDetachedTerminals)` guarda uma
//!   classe que o `IInstantiationService` instancia com o contexto mais os serviços. Aqui o `ctor` é uma função que recebe o
//!   [`TerminalContributionContext`] e devolve a contribuição (quem registra captura os serviços que ela lê).
//! - **`ITerminalContributionContext | IDetachedCompatibleTerminalContributionContext`.** São a mesma struct: o `instance`
//!   é o [`TerminalContributionInstance`] (`Full` ou `Detached`, o `isDetachedTerminalInstance`) e o `processManager` é o
//!   [`TerminalContributionProcess`] (`Manager` ou `Info`, o `isTerminalProcessManager`). `canRunInDetachedTerminals` é o
//!   campo da descrição, e o registro devolve todas as descrições (o código que ativa as contribuições filtra).
//! - **`Registry.add('terminal.contributions', ...)`** não existe (o registro global do porte é o `thread_local!` deste módulo).
//! - **`IXtermTerminal & { raw: RawXtermTerminal }`** é [`IXtermTerminal`], com o `raw` como [`IXtermRawTerminal`], e só os membros
//!   que as contribuições portadas leem (`onDidRequestCopyAsHtml`, `copySelection`). O `RawXtermTerminal` é o
//!   [`IXtermRawTerminal`]: o contrato do emulador (o xterm.js do original) com o que as contribuições leem dele (`cols`,
//!   `rows`, `buffer.active`, `onScroll`, `onRender`, `onSelectionChange`, `registerLinkProvider`, `options.linkHandler`,
//!   `modes.bracketedPasteMode`, `paste` e as dimensões CSS da célula do renderizador, o `_core._renderService`). O frontend
//!   nativo implementa os dois.
//! - **`ILink`, `ILinkProvider`, `ILinkDecorations` e `ILinkHandler`** do xterm são [`IXtermLink`], [`ILinkProvider`],
//!   [`ILinkDecorations`] e [`LinkHandler`] (um conjunto de funções, o `options.linkHandler`). O `callback` do
//!   `provideLinks` é um `FnOnce`, e o método devolve uma promessa do fim do trabalho assíncrono.
//! - **`MaybePromise<{ handled: boolean } | void>`** do `handleMouseEvent` é `MaybePromise<Option<bool>>` (o `handled`).
//! - **`IDimension`** é [`TerminalDimension`].

use std::cell::RefCell;
use std::rc::Rc;

use je_base::event::Event;
use je_base::lifecycle::{DisposableRef, IDisposable};
use je_base::promise::{MaybePromise, Promise};
use je_platform::terminal::capabilities::capabilities::{ITerminalCapabilityStore, ITerminalCommand};

use super::terminal::{ClientRect, ITerminalInstance};
use super::terminal_input_events::{ITerminalDocumentEvents, TerminalMouseEvent};
use super::widgets::widget_manager::TerminalWidgetManager;
use crate::contrib::terminal::common::terminal::{ITerminalProcessInfo, ITerminalProcessManager};
use crate::contrib::terminal::xterm_buffer::{IBufferRange, ITerminal};
use crate::contrib::terminal_contrib::links::links::TerminalLinkType;

/// `IDimension`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TerminalDimension {
    pub width: f64,
    pub height: f64,
}

/// `ILinkDecorations`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ILinkDecorations {
    pub pointer_cursor: bool,
    pub underline: bool,
}

/// `ILink` do xterm.
pub trait IXtermLink {
    /// `range`.
    fn range(&self) -> IBufferRange;
    /// `text`.
    fn text(&self) -> String;
    /// `activate(event, text)`.
    fn activate(&self, event: Option<&TerminalMouseEvent>, text: &str);
    /// `hover?(event, text)`.
    fn hover(&self, _event: &TerminalMouseEvent, _text: &str) {}
    /// `leave?(event, text)`.
    fn leave(&self) {}
    /// `decorations`.
    fn decorations(&self) -> ILinkDecorations {
        ILinkDecorations::default()
    }
    /// `(link as TerminalLink).type`: só o `TerminalLink` do terminal tem tipo.
    fn link_type(&self) -> Option<TerminalLinkType> {
        None
    }
}

/// `ILinkProvider` do xterm.
pub trait ILinkProvider {
    /// `provideLinks(bufferLineNumber, callback)`: a promessa termina quando o `callback` foi chamado.
    fn provide_links(&self, buffer_line_number: i32, callback: Box<dyn FnOnce(Option<Vec<Rc<dyn IXtermLink>>>)>) -> Promise<()>;
}

/// `options.linkHandler` do xterm (`ILinkHandler`): `allowNonHttpProtocols`, `activate`, `hover` e `leave`.
#[derive(Clone)]
pub struct LinkHandler {
    pub allow_non_http_protocols: bool,
    pub activate: Rc<dyn Fn(&TerminalMouseEvent, &str, &IBufferRange) -> Promise<()>>,
    pub hover: Option<Rc<dyn Fn(&TerminalMouseEvent, &str, &IBufferRange)>>,
    pub leave: Option<Rc<dyn Fn(&TerminalMouseEvent, &str, &IBufferRange)>>,
}

/// O argumento de `onRender`: `{ start, end }`, as linhas da viewport redesenhadas.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RenderedRange {
    pub start: i32,
    pub end: i32,
}

/// O `Terminal` do xterm.js, no que as contribuições do terminal leem (ver o cabeçalho).
pub trait IXtermRawTerminal: ITerminal {
    /// `onScroll`: a linha de topo da viewport depois de rolar.
    fn on_scroll(&self) -> Event<i32>;
    /// `onRender`.
    fn on_render(&self) -> Event<RenderedRange>;
    /// `onSelectionChange`.
    fn on_selection_change(&self) -> Event<()>;
    /// `registerLinkProvider(provider)`.
    fn register_link_provider(&self, provider: Rc<dyn ILinkProvider>) -> DisposableRef;
    /// `options.linkHandler`.
    fn link_handler(&self) -> Option<LinkHandler>;
    /// `options.linkHandler = handler`.
    fn set_link_handler(&self, handler: Option<LinkHandler>);
    /// `_core._renderService.dimensions.css.cell`.
    fn css_cell_dimensions(&self) -> ClientRect;
    /// `modes.bracketedPasteMode`.
    fn bracketed_paste_mode(&self) -> bool;
    /// `paste(data)`.
    fn paste(&self, data: &str);
    /// O `document` da janela do terminal (`dom.getWindow(event).document`).
    fn document_events(&self) -> Rc<dyn ITerminalDocumentEvents>;
}

/// `IXtermTerminal & { raw: RawXtermTerminal }`.
pub trait IXtermTerminal {
    /// `raw`.
    fn raw(&self) -> Rc<dyn IXtermRawTerminal>;
    /// `onDidRequestCopyAsHtml`.
    fn on_did_request_copy_as_html(&self) -> Event<Rc<dyn ITerminalCommand>>;
    /// `copySelection(asHtml?, command?)`.
    fn copy_selection(&self, as_html: bool, command: Option<Rc<dyn ITerminalCommand>>);
}

/// `IDetachedTerminalInstance`, no que as contribuições leem.
pub trait IDetachedTerminalInstance {
    /// `capabilities`.
    fn capabilities(&self) -> Rc<dyn ITerminalCapabilityStore>;
    /// `focus()`: quem tem o foco é o frontend nativo.
    fn focus(&self) {}
    /// `hasSelection()`.
    fn has_selection(&self) -> bool {
        false
    }
    /// `clearSelection()`.
    fn clear_selection(&self) {}
}

/// `ITerminalContributionContext['instance'] | IDetachedCompatibleTerminalContributionContext['instance']`.
#[derive(Clone)]
pub enum TerminalContributionInstance {
    Full(Rc<dyn ITerminalInstance>),
    Detached(Rc<dyn IDetachedTerminalInstance>),
}

impl TerminalContributionInstance {
    /// `instance.capabilities`.
    pub fn capabilities(&self) -> Rc<dyn ITerminalCapabilityStore> {
        match self {
            Self::Full(instance) => instance.capabilities(),
            Self::Detached(instance) => instance.capabilities(),
        }
    }

    /// `instance.focus()`.
    pub fn focus(&self) {
        match self {
            Self::Full(instance) => instance.focus(),
            Self::Detached(instance) => instance.focus(),
        }
    }

    /// `instance.hasSelection()`.
    pub fn has_selection(&self) -> bool {
        match self {
            Self::Full(instance) => instance.has_selection(),
            Self::Detached(instance) => instance.has_selection(),
        }
    }

    /// `instance.clearSelection()`.
    pub fn clear_selection(&self) {
        match self {
            Self::Full(instance) => instance.clear_selection(),
            Self::Detached(instance) => instance.clear_selection(),
        }
    }
}

/// `ITerminalProcessManager | ITerminalProcessInfo`.
#[derive(Clone)]
pub enum TerminalContributionProcess {
    Manager(Rc<dyn ITerminalProcessManager>),
    Info(Rc<dyn ITerminalProcessInfo>),
}

/// `ITerminalContributionContext | IDetachedCompatibleTerminalContributionContext`.
#[derive(Clone)]
pub struct TerminalContributionContext {
    pub instance: TerminalContributionInstance,
    pub process_manager: TerminalContributionProcess,
    pub widget_manager: Rc<TerminalWidgetManager>,
}

/// `ITerminalContribution`.
pub trait ITerminalContribution: IDisposable {
    /// `layout?(xterm, dimension)`.
    fn layout(&self, _xterm: &dyn IXtermTerminal, _dimension: TerminalDimension) {}
    /// `xtermOpen?(xterm)`.
    fn xterm_open(&self, _xterm: Rc<dyn IXtermTerminal>) {}
    /// `xtermReady?(xterm)`.
    fn xterm_ready(&self, _xterm: Rc<dyn IXtermTerminal>) {}
    /// `handleMouseEvent?(event)`: `Some(handled)` quando a contribuição tratou o evento.
    fn handle_mouse_event(&self, _event: &TerminalMouseEvent) -> MaybePromise<Option<bool>> {
        MaybePromise::Value(None)
    }
}

/// O construtor de uma contribuição.
pub type TerminalContributionCtor = Rc<dyn Fn(TerminalContributionContext) -> Rc<dyn ITerminalContribution>>;

/// `ITerminalContributionDescription`.
#[derive(Clone)]
pub struct ITerminalContributionDescription {
    pub id: String,
    pub can_run_in_detached_terminals: bool,
    pub ctor: TerminalContributionCtor,
}

thread_local! {
    /// `TerminalContributionRegistry.INSTANCE`.
    static TERMINAL_CONTRIBUTIONS: RefCell<Vec<ITerminalContributionDescription>> = const { RefCell::new(Vec::new()) };
}

/// `registerTerminalContribution(id, ctor, canRunInDetachedTerminals = false)`.
pub fn register_terminal_contribution(id: &str, ctor: TerminalContributionCtor, can_run_in_detached_terminals: bool) {
    TERMINAL_CONTRIBUTIONS.with(|contributions| {
        contributions.borrow_mut().push(ITerminalContributionDescription { id: id.to_owned(), can_run_in_detached_terminals, ctor });
    });
}

/// `TerminalExtensionsRegistry.getTerminalContributions()`: uma cópia da lista, na ordem de registro.
pub fn get_terminal_contributions() -> Vec<ITerminalContributionDescription> {
    TERMINAL_CONTRIBUTIONS.with(|contributions| contributions.borrow().clone())
}
