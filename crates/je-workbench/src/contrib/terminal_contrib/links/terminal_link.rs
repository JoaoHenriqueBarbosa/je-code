//! Porte de `src/vs/workbench/contrib/terminalContrib/links/browser/terminalLink.ts`: o link que o xterm mostra, com o
//! hover (tooltip atrasado, decorações de sublinhado e cursor, invalidação quando o terminal redesenha a linha).
//!
//! Decisões de tradução:
//! - **`implements ILink`** é o [`IXtermLink`]; `type` é `link_type`.
//! - **`dom.getWindow(event).document`** (`keydown`, `keyup` e `mousemove`) e **`w.devicePixelRatio`** são o
//!   [`ITerminalDocumentEvents`] do `IXtermRawTerminal::document_events`. `event.altKey/ctrlKey/metaKey` e
//!   `pageX/pageY` são os campos do [`TerminalMouseEvent`] e do [`TerminalKeyEvent`].
//! - **`MutableDisposable<RunOnceScheduler>` e `MutableDisposable<DisposableStore>`** são os `MutableDisposable` do porte.
//! - O `TerminalLink` precisa de si mesmo (`this`) nos callbacks do tooltip e das decorações: o construtor devolve um `Rc`
//!   criado com `Rc::new_cyclic`, e os callbacks seguram um `Weak`.
//! - **`getValue('workbench.hover.delay')`** é lido como número (`Dynamic::Number`); ausente vale 0, como o `setTimeout(undefined)`.
//! - **`IInvalidatableLink`** é o recorte de `onInvalidated` que o gerenciador de links lê (o original usa `TerminalLink`,
//!   e o teste do gerenciador passa um objeto parcial só com `onInvalidated`).

use std::cell::Cell;
use std::rc::{Rc, Weak};

use je_base::async_::RunOnceScheduler;
use je_base::dynamic::Dynamic;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, DisposableStore, IDisposable, MutableDisposable};
use je_base::platform::Platform;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_platform::configuration::service::IConfigurationService;
use je_widgets::hover_api::IHoverAction;

use super::links::TerminalLinkType;
use super::terminal_link_helpers::convert_buffer_range_to_viewport;
use super::terminal_link_parsing::ParsedLink;
use crate::contrib::terminal::browser::terminal_extensions::{ILinkDecorations, IXtermLink, IXtermRawTerminal};
use crate::contrib::terminal::browser::terminal_input_events::{TerminalKeyEvent, TerminalMouseEvent};
use crate::contrib::terminal::xterm_buffer::{IBufferRange, IViewportRange};

/// `(event, uri) => Promise<void>`: o `_activateCallback`.
pub type LinkActivateCallback = Rc<dyn Fn(Option<&TerminalMouseEvent>, &str) -> Promise<()>>;

/// `(link, viewportRange, modifierDownCallback?, modifierUpCallback?) => void`: o `_tooltipCallback`.
pub type LinkTooltipCallback = Rc<dyn Fn(Rc<TerminalLink>, IViewportRange, Option<Rc<dyn Fn()>>, Option<Rc<dyn Fn()>>)>;

/// O recorte de `TerminalLink` que o gerenciador de links lê para invalidar um hover (`onInvalidated`).
pub trait IInvalidatableLink {
    /// `onInvalidated`.
    fn on_invalidated(&self) -> Event<()>;
}

/// `TerminalLink`.
pub struct TerminalLink {
    base: Rc<Disposable>,
    this: Weak<TerminalLink>,
    xterm: Rc<dyn IXtermRawTerminal>,
    pub range: IBufferRange,
    pub text: String,
    pub uri: Option<URI>,
    pub parsed_link: Option<ParsedLink>,
    pub actions: Option<Vec<IHoverAction>>,
    viewport_y: i32,
    activate_callback: LinkActivateCallback,
    tooltip_callback: LinkTooltipCallback,
    is_high_confidence_link: bool,
    pub label: Option<String>,
    type_: TerminalLinkType,
    configuration_service: Rc<dyn IConfigurationService>,

    decorations: Cell<ILinkDecorations>,
    tooltip_scheduler: Rc<MutableDisposable<RunOnceScheduler>>,
    hover_listeners: Rc<MutableDisposable<DisposableStore>>,
    on_invalidated: Rc<Emitter<()>>,
}

impl TerminalLink {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        xterm: Rc<dyn IXtermRawTerminal>,
        range: IBufferRange,
        text: String,
        uri: Option<URI>,
        parsed_link: Option<ParsedLink>,
        actions: Option<Vec<IHoverAction>>,
        viewport_y: i32,
        activate_callback: LinkActivateCallback,
        tooltip_callback: LinkTooltipCallback,
        is_high_confidence_link: bool,
        label: Option<String>,
        type_: TerminalLinkType,
        configuration_service: Rc<dyn IConfigurationService>,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let tooltip_scheduler = MutableDisposable::<RunOnceScheduler>::new();
        let hover_listeners = MutableDisposable::<DisposableStore>::new();
        let on_invalidated = Emitter::new();
        base.register(tooltip_scheduler.clone());
        base.register(hover_listeners.clone());
        base.register(on_invalidated.clone());
        Rc::new_cyclic(|this| Self {
            base,
            this: this.clone(),
            xterm,
            range,
            text,
            uri,
            parsed_link,
            actions,
            viewport_y,
            activate_callback,
            tooltip_callback,
            is_high_confidence_link,
            label,
            type_,
            configuration_service,
            decorations: Cell::new(ILinkDecorations { pointer_cursor: false, underline: is_high_confidence_link }),
            tooltip_scheduler,
            hover_listeners,
            on_invalidated,
        })
    }

    /// `type`.
    pub fn type_(&self) -> &TerminalLinkType {
        &self.type_
    }

    fn enable_decorations(&self) {
        let mut decorations = self.decorations.get();
        decorations.pointer_cursor = true;
        decorations.underline = true;
        self.decorations.set(decorations);
    }

    fn disable_decorations(&self) {
        let mut decorations = self.decorations.get();
        decorations.pointer_cursor = false;
        decorations.underline = self.is_high_confidence_link;
        self.decorations.set(decorations);
    }

    /// `_isModifierDown(event)`.
    fn is_modifier_down(&self, alt_key: bool, ctrl_key: bool, meta_key: bool) -> bool {
        let multi_cursor_modifier = self.configuration_service.get_value(Some("editor.multiCursorModifier"), None);
        if multi_cursor_modifier.strict_equals(&Dynamic::string("ctrlCmd")) {
            return alt_key;
        }
        if Platform::detect().is_macintosh { meta_key } else { ctrl_key }
    }

    fn weak_action(&self, action: fn(&TerminalLink)) -> Rc<dyn Fn()> {
        let this = self.this.clone();
        Rc::new(move || {
            if let Some(this) = this.upgrade() {
                action(&this);
            }
        })
    }

    fn key_modifier_down(&self, event: &TerminalKeyEvent) -> bool {
        self.is_modifier_down(event.alt_key, event.ctrl_key, event.meta_key)
    }
}

impl IDisposable for TerminalLink {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl IInvalidatableLink for TerminalLink {
    fn on_invalidated(&self) -> Event<()> {
        self.on_invalidated.event()
    }
}

impl IXtermLink for TerminalLink {
    fn range(&self) -> IBufferRange {
        self.range
    }

    fn text(&self) -> String {
        self.text.clone()
    }

    fn activate(&self, event: Option<&TerminalMouseEvent>, text: &str) {
        let _ = (self.activate_callback)(event, text);
    }

    fn hover(&self, event: &TerminalMouseEvent, _text: &str) {
        let document = self.xterm.document_events();
        // Listen for modifier before handing it off to the hover to handle so it gets disposed correctly
        let hover_listeners = DisposableStore::new();
        self.hover_listeners.set(Some(hover_listeners.clone()));
        let this = self.this.clone();
        hover_listeners.add(document.on_key_down().listen({
            let this = this.clone();
            move |e| {
                let Some(this) = this.upgrade() else { return };
                if !e.repeat && this.key_modifier_down(e) {
                    this.enable_decorations();
                }
            }
        }));
        hover_listeners.add(document.on_key_up().listen({
            let this = this.clone();
            move |e| {
                let Some(this) = this.upgrade() else { return };
                if !e.repeat && !this.key_modifier_down(e) {
                    this.disable_decorations();
                }
            }
        }));

        // Listen for when the terminal renders on the same line as the link
        hover_listeners.add(self.xterm.on_render().listen({
            let this = this.clone();
            move |e| {
                let Some(this) = this.upgrade() else { return };
                let viewport_range_y = this.range.start.y - this.viewport_y;
                if viewport_range_y >= e.start && viewport_range_y <= e.end {
                    this.on_invalidated.fire(());
                }
            }
        }));

        // Only show the tooltip and highlight for high confidence links (not word/search workspace
        // links). Feedback was that this makes using the terminal overly noisy.
        if self.is_high_confidence_link {
            let delay = match self.configuration_service.get_value(Some("workbench.hover.delay"), None) {
                Dynamic::Number(delay) if delay > 0.0 => delay as u64,
                _ => 0,
            };
            let scheduler_this = self.this.clone();
            let scheduler = RunOnceScheduler::new(
                move || {
                    let Some(this) = scheduler_this.upgrade() else { return };
                    (this.tooltip_callback)(
                        this.clone(),
                        convert_buffer_range_to_viewport(&this.range, this.viewport_y),
                        Some(this.weak_action(TerminalLink::enable_decorations)),
                        Some(this.weak_action(TerminalLink::disable_decorations)),
                    );
                    // Clear out scheduler until next hover event
                    this.tooltip_scheduler.clear();
                },
                delay,
            );
            self.tooltip_scheduler.set(Some(scheduler.clone()));
            scheduler.schedule(None);
        }

        let origin = Rc::new(Cell::new((event.page_x, event.page_y)));
        hover_listeners.add(document.on_mouse_move().listen({
            let this = this.clone();
            let device_pixel_ratio = document.device_pixel_ratio();
            move |e| {
                let Some(this) = this.upgrade() else { return };
                // Update decorations
                if this.is_modifier_down(e.alt_key, e.ctrl_key, e.meta_key) {
                    this.enable_decorations();
                } else {
                    this.disable_decorations();
                }

                // Reset the scheduler if the mouse moves too much
                let (origin_x, origin_y) = origin.get();
                if (e.page_x - origin_x).abs() > device_pixel_ratio * 2.0 || (e.page_y - origin_y).abs() > device_pixel_ratio * 2.0 {
                    origin.set((e.page_x, e.page_y));
                    if let Some(scheduler) = this.tooltip_scheduler.value() {
                        scheduler.schedule(None);
                    }
                }
            }
        }));
    }

    fn leave(&self) {
        self.hover_listeners.clear();
        self.tooltip_scheduler.clear();
    }

    fn decorations(&self) -> ILinkDecorations {
        self.decorations.get()
    }

    fn link_type(&self) -> Option<TerminalLinkType> {
        Some(self.type_.clone())
    }
}
