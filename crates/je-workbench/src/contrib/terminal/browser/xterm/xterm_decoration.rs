//! O contrato das decorações do xterm que os addons do workbench leem (`registerDecoration`, `IDecoration`, `IDecorationOptions`
//! e o `HTMLElement` que cada decoração desenha): o `MarkNavigationAddon` e o `DecorationAddon` escrevem nele, e o emulador
//! do frontend nativo o implementa.
//!
//! Decisões de tradução:
//! - **`IDecoration`.** É o [`IXtermDecoration`]: `marker`, `element`, `isDisposed`, `onRender`, `onDispose` e `dispose`. O
//!   `onRender` entrega o elemento ([`IDecorationElement`]) a cada desenho, e quem assina depois do primeiro desenho recebe o
//!   elemento já existente, como no xterm. `options.overviewRulerOptions.color = color` (a atualização de cor que o
//!   `DecorationAddon` faz ao trocar o tema) é [`IXtermDecoration::set_overview_ruler_color`], que cria as opções quando elas
//!   não existem, como os dois ramos do original.
//! - **`HTMLElement`.** Sem DOM, o elemento é o [`IDecorationElement`] com só o que os addons usam: a lista de classes
//!   (`classList`), o `removeAttribute`, os estilos em linha de `updateLayout` ([`DecorationStyleProperty`]) e os ouvintes de
//!   mouse (`dom.addDisposableListener`, [`IDecorationElement::add_disposable_listener`]); o descarte do ouvinte é o
//!   `removeEventListener` do original. O evento ([`DecorationEvent`]) só carrega o `stopImmediatePropagation`.
//! - **`Terminal.registerDecoration`.** É [`IDecorationHost::register_decoration`], uma extensão do emulador sem tela
//!   ([`IHeadlessTerminal`]), que já traz o buffer, os marcadores e os eventos.
//! - **`IOverviewRulerDecorationOptions`.** O `position` aceita os quatro valores do xterm ([`OverviewRulerPosition`]).

use std::cell::Cell;
use std::rc::Rc;

use je_base::event::Event;
use je_base::lifecycle::{DisposableRef, IDisposable};
use je_platform::terminal::capabilities::capabilities::IMarker;
use je_platform::terminal::xterm_headless::IHeadlessTerminal;

/// `IDecorationOverviewRulerOptions['position']`: `'left' | 'center' | 'right' | 'full'`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OverviewRulerPosition {
    Left,
    Center,
    Right,
    Full,
}

/// `IDecorationOverviewRulerOptions`: `{ color, position? }`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IDecorationOverviewRulerOptions {
    pub color: String,
    pub position: Option<OverviewRulerPosition>,
}

/// `IDecorationOptions`: o `marker` é obrigatório; `x` e `width` são colunas do buffer.
#[derive(Clone)]
pub struct IDecorationOptions {
    pub marker: Rc<dyn IMarker>,
    pub x: Option<i32>,
    pub width: Option<i32>,
    pub overview_ruler_options: Option<IDecorationOverviewRulerOptions>,
}

/// Os tipos de evento de mouse que o `DecorationAddon` ouve (`dom.EventType.MOUSE_DOWN`, `CLICK` e `CONTEXT_MENU`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecorationEventType {
    MouseDown,
    Click,
    ContextMenu,
}

impl DecorationEventType {
    /// O nome do evento no DOM.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::MouseDown => "mousedown",
            Self::Click => "click",
            Self::ContextMenu => "contextmenu",
        }
    }
}

/// O evento de mouse de uma decoração: só o `stopImmediatePropagation()`.
#[derive(Default)]
pub struct DecorationEvent {
    propagation_stopped: Cell<bool>,
}

impl DecorationEvent {
    pub fn new() -> Self {
        Self::default()
    }

    /// `e.stopImmediatePropagation()`.
    pub fn stop_immediate_propagation(&self) {
        self.propagation_stopped.set(true);
    }

    /// Diz se algum ouvinte chamou `stopImmediatePropagation()`.
    pub fn is_propagation_stopped(&self) -> bool {
        self.propagation_stopped.get()
    }
}

/// O ouvinte de um [`DecorationEventType`].
pub type DecorationListener = Rc<dyn Fn(&DecorationEvent)>;

/// As propriedades de `element.style` que `updateLayout` escreve.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecorationStyleProperty {
    Width,
    Height,
    FontSize,
    MarginLeft,
}

/// O `HTMLElement` de uma decoração, no que os addons usam.
pub trait IDecorationElement {
    /// As classes do elemento, na ordem (`for (const c of element.classList)`).
    fn class_list(&self) -> Vec<String>;

    /// `element.classList.contains(name)`.
    fn class_list_contains(&self, name: &str) -> bool;

    /// `element.classList.add(...names)`.
    fn class_list_add(&self, names: &[&str]);

    /// `element.classList.remove(name)`.
    fn class_list_remove(&self, name: &str);

    /// `element.removeAttribute(name)`.
    fn remove_attribute(&self, name: &str);

    /// `element.style.<property> = value`.
    fn set_style(&self, property: DecorationStyleProperty, value: &str);

    /// `dom.addDisposableListener(element, type, listener)`: o descarte do que devolve é o `removeEventListener(type, ...)`.
    fn add_disposable_listener(&self, event_type: DecorationEventType, listener: DecorationListener) -> DisposableRef;
}

/// `IDecoration` do xterm.
pub trait IXtermDecoration: IDisposable {
    /// `marker`.
    fn marker(&self) -> Rc<dyn IMarker>;

    /// `element`: o elemento, depois do primeiro desenho.
    fn element(&self) -> Option<Rc<dyn IDecorationElement>>;

    /// `isDisposed`.
    fn is_disposed(&self) -> bool;

    /// `onRender`.
    fn on_render(&self) -> Event<Rc<dyn IDecorationElement>>;

    /// `onDispose`.
    fn on_dispose(&self) -> Event<()>;

    /// `options.overviewRulerOptions.color = color`, criando as opções da régua quando elas não existem.
    fn set_overview_ruler_color(&self, color: &str);
}

/// O emulador com `registerDecoration`.
pub trait IDecorationHost: IHeadlessTerminal {
    /// `registerDecoration(options)`: `None` quando o marcador já foi descartado.
    fn register_decoration(&self, options: IDecorationOptions) -> Option<Rc<dyn IXtermDecoration>>;
}
