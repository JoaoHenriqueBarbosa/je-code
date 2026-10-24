//! Os eventos de entrada que o DOM entrega ao código do terminal (`MouseEvent` e `KeyboardEvent`) e os eventos do documento
//! da janela (`keydown`, `keyup` e `mousemove` em `document`), como contrato Rust.
//!
//! Não há DOM no je-code: o frontend nativo é quem dispara estes eventos tipados, e o código portado (o `TerminalLink`, o
//! `TerminalHover`, a contribuição de área de transferência e o gerenciador de links) só os lê.
//!
//! Notas de tradução:
//! - **`MouseEvent`** é [`TerminalMouseEvent`], só com o que o código do terminal lê: `button`, os modificadores, `pageX`, `pageY`
//!   e `preventDefault`. O `TerminalLinkQuickPickEvent extends MouseEvent` (o clique sintético de quando o link vem do quick
//!   pick) é o [`TerminalMouseEvent::link_quick_pick`], e o `instanceof TerminalLinkQuickPickEvent` é o campo
//!   [`TerminalMouseEvent::is_link_quick_pick`].
//! - **`KeyboardEvent`** é [`TerminalKeyEvent`] (os modificadores e `repeat`).
//! - **`dom.getWindow(event).document`** e **`window.devicePixelRatio`** são o [`ITerminalDocumentEvents`], que o frontend
//!   implementa para a janela que gerou o evento.

use std::cell::Cell;

use je_base::event::Event;

/// `MouseEvent`, reduzido ao que o terminal lê.
#[derive(Clone, Debug, Default)]
pub struct TerminalMouseEvent {
    /// `type`: `'click'`, `'mousemove'`...
    pub kind: String,
    /// `button`: 0 principal, 1 do meio, 2 secundário.
    pub button: i32,
    pub alt_key: bool,
    pub ctrl_key: bool,
    pub meta_key: bool,
    pub shift_key: bool,
    pub page_x: f64,
    pub page_y: f64,
    /// `event instanceof TerminalLinkQuickPickEvent`.
    pub is_link_quick_pick: bool,
    default_prevented: std::rc::Rc<Cell<bool>>,
}

impl TerminalMouseEvent {
    pub fn new(kind: impl Into<String>) -> Self {
        Self { kind: kind.into(), ..Self::default() }
    }

    /// `new TerminalLinkQuickPickEvent(type)`.
    pub fn link_quick_pick(kind: impl Into<String>) -> Self {
        Self { kind: kind.into(), is_link_quick_pick: true, ..Self::default() }
    }

    /// `event.preventDefault()`.
    pub fn prevent_default(&self) {
        self.default_prevented.set(true);
    }

    /// `event.defaultPrevented`.
    pub fn default_prevented(&self) -> bool {
        self.default_prevented.get()
    }
}

/// `KeyboardEvent`, reduzido ao que o terminal lê.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TerminalKeyEvent {
    pub alt_key: bool,
    pub ctrl_key: bool,
    pub meta_key: bool,
    pub shift_key: bool,
    pub repeat: bool,
}

/// O `document` (e a janela) onde o terminal está.
pub trait ITerminalDocumentEvents {
    /// `document.addEventListener('keydown', ...)`.
    fn on_key_down(&self) -> Event<TerminalKeyEvent>;
    /// `document.addEventListener('keyup', ...)`.
    fn on_key_up(&self) -> Event<TerminalKeyEvent>;
    /// `document.addEventListener('mousemove', ...)`.
    fn on_mouse_move(&self) -> Event<TerminalMouseEvent>;
    /// `window.devicePixelRatio`.
    fn device_pixel_ratio(&self) -> f64;
}
