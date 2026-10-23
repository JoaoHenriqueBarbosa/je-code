//! Porte de `src/vs/editor/common/viewModelEventDispatcher.ts`: o despachante dos eventos do view model
//! ([`ViewModelEventDispatcher`]), o coletor ([`ViewModelEventsCollector`]) e os eventos de saída
//! ([`OutgoingViewModelEvent`] e as structs de cada um).
//!
//! Decisões de tradução:
//! - O union type `OutgoingViewModelEvent` vira o `enum` [`OutgoingViewModelEvent`], com uma variante por
//!   classe. O campo `kind` é [`OutgoingViewModelEvent::kind`]; `isNoOp()` e `attemptToMerge(other)` são
//!   [`OutgoingViewModelEvent::is_no_op`] e [`OutgoingViewModelEvent::attempt_to_merge`], com a lógica de
//!   cada classe nos braços do `match`. Os eventos do modelo (`ModelXxxChangedEvent`) nunca são fundidos
//!   nem são no-op, como no original.
//! - Fidelidade ao original, inclusive à sua esquisitice: a fusão de dois `WidgetFocusChangedEvent` devolve
//!   um `FocusChangedEvent` (o original instancia a classe errada), então o resultado muda de `kind`.
//! - O despachante é compartilhado (`Rc`) e muda de estado por trás de `&self`, com `Cell`/`RefCell`. Nenhum
//!   empréstimo atravessa uma chamada para fora (handler, listener), porque eles reentram no despachante.
//! - `ViewModelEventDispatcher extends Disposable` guarda um [`Disposable`] que registra o `Emitter`.
//! - Os handlers são `Rc<dyn ViewEventHandler>`; a identidade (o `===` do original) é o endereço do objeto.
//! - O contador do coletor é `i32`, como o `number` do original (que pode ficar negativo num `end` a mais).
//! - `try { ... } finally { _isConsumingViewEventQueue = false }` é um guarda que zera a flag ao sair,
//!   mesmo por pânico de um handler.
//! - `IContentSizeChangedEvent` (de `editorCommon.ts`) não tem porte: [`ContentSizeChangedEvent`] traz os
//!   mesmos quatro campos públicos.
//! - Os textos de `console.warn` vão para `stderr`.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, IDisposable, id_of};

use crate::selection::Selection;
use crate::text_model_events as model_events;
use crate::view_events::{ViewEvent, ViewEventHandler};
use crate::view_model_types::CursorChangeReason;

/// `ViewModelEventDispatcher`.
pub struct ViewModelEventDispatcher {
    base: Rc<Disposable>,
    on_event: Rc<Emitter<OutgoingViewModelEvent>>,

    event_handlers: RefCell<Vec<Rc<dyn ViewEventHandler>>>,
    view_event_queue: RefCell<Option<Vec<ViewEvent>>>,
    is_consuming_view_event_queue: Cell<bool>,
    collector: RefCell<Option<Rc<ViewModelEventsCollector>>>,
    collector_cnt: Cell<i32>,
    outgoing_events: RefCell<Vec<OutgoingViewModelEvent>>,
}

/// O `finally` de `_consumeViewEventQueue`: zera a flag ao sair, por qualquer caminho.
struct ResetOnDrop<'a>(&'a Cell<bool>);

impl Drop for ResetOnDrop<'_> {
    fn drop(&mut self) {
        self.0.set(false);
    }
}

impl ViewModelEventDispatcher {
    pub fn new() -> Rc<Self> {
        let base = Disposable::new();
        let on_event: Rc<Emitter<OutgoingViewModelEvent>> = Emitter::new();
        base.register(on_event.clone());
        Rc::new(Self {
            base,
            on_event,
            event_handlers: RefCell::new(Vec::new()),
            view_event_queue: RefCell::new(None),
            is_consuming_view_event_queue: Cell::new(false),
            collector: RefCell::new(None),
            collector_cnt: Cell::new(0),
            outgoing_events: RefCell::new(Vec::new()),
        })
    }

    /// `onEvent`.
    pub fn on_event(&self) -> Event<OutgoingViewModelEvent> {
        self.on_event.event()
    }

    pub fn emit_outgoing_event(&self, e: OutgoingViewModelEvent) {
        self.add_outgoing_event(e);
        self.emit_outgoing_events();
    }

    fn add_outgoing_event(&self, e: OutgoingViewModelEvent) {
        let mut outgoing_events = self.outgoing_events.borrow_mut();
        for i in 0..outgoing_events.len() {
            let merge_result = if outgoing_events[i].kind() == e.kind() { outgoing_events[i].attempt_to_merge(&e) } else { None };
            if let Some(merge_result) = merge_result {
                outgoing_events[i] = merge_result;
                return;
            }
        }
        // not merged
        outgoing_events.push(e);
    }

    fn emit_outgoing_events(&self) {
        loop {
            if self.outgoing_events.borrow().is_empty() {
                return;
            }
            if self.collector.borrow().is_some() || self.is_consuming_view_event_queue.get() {
                // right now collecting or emitting view events, so let's postpone emitting
                return;
            }
            let event = self.outgoing_events.borrow_mut().remove(0);
            if event.is_no_op() {
                continue;
            }
            self.on_event.fire(event);
        }
    }

    pub fn add_view_event_handler(&self, event_handler: Rc<dyn ViewEventHandler>) {
        let is_duplicate = self.event_handlers.borrow().iter().any(|handler| id_of(handler) == id_of(&event_handler));
        if is_duplicate {
            eprintln!("Detected duplicate listener in ViewEventDispatcher");
        }
        self.event_handlers.borrow_mut().push(event_handler);
    }

    pub fn remove_view_event_handler(&self, event_handler: &Rc<dyn ViewEventHandler>) {
        let mut event_handlers = self.event_handlers.borrow_mut();
        if let Some(index) = event_handlers.iter().position(|handler| id_of(handler) == id_of(event_handler)) {
            event_handlers.remove(index);
        }
    }

    pub fn begin_emit_view_events(&self) -> Rc<ViewModelEventsCollector> {
        self.collector_cnt.set(self.collector_cnt.get() + 1);
        if self.collector_cnt.get() == 1 {
            *self.collector.borrow_mut() = Some(Rc::new(ViewModelEventsCollector::new()));
        }
        self.collector.borrow().clone().expect("o coletor existe enquanto há um begin sem o end")
    }

    pub fn end_emit_view_events(&self) {
        self.collector_cnt.set(self.collector_cnt.get() - 1);
        if self.collector_cnt.get() == 0 {
            let collector = self.collector.borrow_mut().take().expect("o coletor existe enquanto há um begin sem o end");
            let outgoing_events = std::mem::take(&mut *collector.outgoing_events.borrow_mut());
            let view_events = std::mem::take(&mut *collector.view_events.borrow_mut());

            for outgoing_event in outgoing_events {
                self.add_outgoing_event(outgoing_event);
            }

            if !view_events.is_empty() {
                self.emit_many(view_events);
            }
        }
        self.emit_outgoing_events();
    }

    pub fn emit_single_view_event(&self, event: ViewEvent) {
        let events_collector = self.begin_emit_view_events();
        events_collector.emit_view_event(event);
        self.end_emit_view_events();
    }

    fn emit_many(&self, events: Vec<ViewEvent>) {
        {
            let mut queue = self.view_event_queue.borrow_mut();
            match queue.as_mut() {
                Some(queue) => queue.extend(events),
                None => *queue = Some(events),
            }
        }

        if !self.is_consuming_view_event_queue.get() {
            self.consume_view_event_queue();
        }
    }

    fn consume_view_event_queue(&self) {
        self.is_consuming_view_event_queue.set(true);
        let _reset = ResetOnDrop(&self.is_consuming_view_event_queue);
        self.do_consume_queue();
    }

    fn do_consume_queue(&self) {
        loop {
            // Empty event queue, as events might come in while sending these off
            let events = self.view_event_queue.borrow_mut().take();
            let Some(events) = events else { return };

            // Use a clone of the event handlers list, as they might remove themselves
            let event_handlers = self.event_handlers.borrow().clone();
            for event_handler in event_handlers {
                event_handler.handle_events(&events);
            }
        }
    }
}

impl IDisposable for ViewModelEventDispatcher {
    fn dispose(&self) {
        self.base.dispose();
    }
}

/// `ViewModelEventsCollector`. Os dois vetores são públicos, como no original, atrás de `RefCell`.
#[derive(Default)]
pub struct ViewModelEventsCollector {
    pub view_events: RefCell<Vec<ViewEvent>>,
    pub outgoing_events: RefCell<Vec<OutgoingViewModelEvent>>,
}

impl ViewModelEventsCollector {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn emit_view_event(&self, event: ViewEvent) {
        self.view_events.borrow_mut().push(event);
    }

    pub fn emit_outgoing_event(&self, e: OutgoingViewModelEvent) {
        self.outgoing_events.borrow_mut().push(e);
    }
}

/// `OutgoingViewModelEventKind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutgoingViewModelEventKind {
    ContentSizeChanged,
    FocusChanged,
    WidgetFocusChanged,
    ScrollChanged,
    ViewZonesChanged,
    HiddenAreasChanged,
    ReadOnlyEditAttempt,
    CursorStateChanged,
    ModelDecorationsChanged,
    ModelLanguageChanged,
    ModelLanguageConfigurationChanged,
    ModelContentChanged,
    ModelOptionsChanged,
    ModelTokensChanged,
    ModelLineHeightChanged,
    ModelFontChangedEvent,
}

/// `OutgoingViewModelEvent`: o union type das classes de evento de saída.
#[derive(Clone, Debug)]
pub enum OutgoingViewModelEvent {
    ContentSizeChanged(ContentSizeChangedEvent),
    FocusChanged(FocusChangedEvent),
    WidgetFocusChanged(WidgetFocusChangedEvent),
    ScrollChanged(ScrollChangedEvent),
    ViewZonesChanged(ViewZonesChangedEvent),
    HiddenAreasChanged(HiddenAreasChangedEvent),
    ReadOnlyEditAttempt(ReadOnlyEditAttemptEvent),
    CursorStateChanged(CursorStateChangedEvent),
    ModelDecorationsChanged(ModelDecorationsChangedEvent),
    ModelLanguageChanged(ModelLanguageChangedEvent),
    ModelLanguageConfigurationChanged(ModelLanguageConfigurationChangedEvent),
    ModelContentChanged(ModelContentChangedEvent),
    ModelOptionsChanged(ModelOptionsChangedEvent),
    ModelTokensChanged(ModelTokensChangedEvent),
    ModelLineHeightChanged(ModelLineHeightChangedEvent),
    ModelFontChanged(ModelFontChangedEvent),
}

impl OutgoingViewModelEvent {
    /// O campo `kind` do evento.
    pub fn kind(&self) -> OutgoingViewModelEventKind {
        match self {
            OutgoingViewModelEvent::ContentSizeChanged(_) => OutgoingViewModelEventKind::ContentSizeChanged,
            OutgoingViewModelEvent::FocusChanged(_) => OutgoingViewModelEventKind::FocusChanged,
            OutgoingViewModelEvent::WidgetFocusChanged(_) => OutgoingViewModelEventKind::WidgetFocusChanged,
            OutgoingViewModelEvent::ScrollChanged(_) => OutgoingViewModelEventKind::ScrollChanged,
            OutgoingViewModelEvent::ViewZonesChanged(_) => OutgoingViewModelEventKind::ViewZonesChanged,
            OutgoingViewModelEvent::HiddenAreasChanged(_) => OutgoingViewModelEventKind::HiddenAreasChanged,
            OutgoingViewModelEvent::ReadOnlyEditAttempt(_) => OutgoingViewModelEventKind::ReadOnlyEditAttempt,
            OutgoingViewModelEvent::CursorStateChanged(_) => OutgoingViewModelEventKind::CursorStateChanged,
            OutgoingViewModelEvent::ModelDecorationsChanged(_) => OutgoingViewModelEventKind::ModelDecorationsChanged,
            OutgoingViewModelEvent::ModelLanguageChanged(_) => OutgoingViewModelEventKind::ModelLanguageChanged,
            OutgoingViewModelEvent::ModelLanguageConfigurationChanged(_) => OutgoingViewModelEventKind::ModelLanguageConfigurationChanged,
            OutgoingViewModelEvent::ModelContentChanged(_) => OutgoingViewModelEventKind::ModelContentChanged,
            OutgoingViewModelEvent::ModelOptionsChanged(_) => OutgoingViewModelEventKind::ModelOptionsChanged,
            OutgoingViewModelEvent::ModelTokensChanged(_) => OutgoingViewModelEventKind::ModelTokensChanged,
            OutgoingViewModelEvent::ModelLineHeightChanged(_) => OutgoingViewModelEventKind::ModelLineHeightChanged,
            OutgoingViewModelEvent::ModelFontChanged(_) => OutgoingViewModelEventKind::ModelFontChangedEvent,
        }
    }

    /// `isNoOp()`: só os eventos que carregam estado antigo e novo podem ser no-op.
    pub fn is_no_op(&self) -> bool {
        match self {
            OutgoingViewModelEvent::ContentSizeChanged(e) => e.is_no_op(),
            OutgoingViewModelEvent::FocusChanged(e) => e.is_no_op(),
            OutgoingViewModelEvent::WidgetFocusChanged(e) => e.is_no_op(),
            OutgoingViewModelEvent::ScrollChanged(e) => e.is_no_op(),
            OutgoingViewModelEvent::CursorStateChanged(e) => e.is_no_op(),
            _ => false,
        }
    }

    /// `attemptToMerge(other)`: `None` quando os tipos diferem ou o evento não funde (os do modelo).
    pub fn attempt_to_merge(&self, other: &OutgoingViewModelEvent) -> Option<OutgoingViewModelEvent> {
        if other.kind() != self.kind() {
            return None;
        }
        match (self, other) {
            (OutgoingViewModelEvent::ContentSizeChanged(a), OutgoingViewModelEvent::ContentSizeChanged(b)) => {
                Some(OutgoingViewModelEvent::ContentSizeChanged(ContentSizeChangedEvent::new(
                    a.old_content_width,
                    a.old_content_height,
                    b.content_width,
                    b.content_height,
                )))
            }
            (OutgoingViewModelEvent::FocusChanged(a), OutgoingViewModelEvent::FocusChanged(b)) => {
                Some(OutgoingViewModelEvent::FocusChanged(FocusChangedEvent::new(a.old_has_focus, b.has_focus)))
            }
            // O original devolve um `FocusChangedEvent` aqui (não um `WidgetFocusChangedEvent`).
            (OutgoingViewModelEvent::WidgetFocusChanged(a), OutgoingViewModelEvent::WidgetFocusChanged(b)) => {
                Some(OutgoingViewModelEvent::FocusChanged(FocusChangedEvent::new(a.old_has_focus, b.has_focus)))
            }
            (OutgoingViewModelEvent::ScrollChanged(a), OutgoingViewModelEvent::ScrollChanged(b)) => {
                Some(OutgoingViewModelEvent::ScrollChanged(ScrollChangedEvent::new(
                    a.old_scroll_width,
                    a.old_scroll_left,
                    a.old_scroll_height,
                    a.old_scroll_top,
                    b.scroll_width,
                    b.scroll_left,
                    b.scroll_height,
                    b.scroll_top,
                )))
            }
            (OutgoingViewModelEvent::ViewZonesChanged(_), OutgoingViewModelEvent::ViewZonesChanged(_))
            | (OutgoingViewModelEvent::HiddenAreasChanged(_), OutgoingViewModelEvent::HiddenAreasChanged(_))
            | (OutgoingViewModelEvent::ReadOnlyEditAttempt(_), OutgoingViewModelEvent::ReadOnlyEditAttempt(_)) => Some(self.clone()),
            (OutgoingViewModelEvent::CursorStateChanged(a), OutgoingViewModelEvent::CursorStateChanged(b)) => {
                Some(OutgoingViewModelEvent::CursorStateChanged(CursorStateChangedEvent::new(
                    a.old_selections.clone(),
                    b.selections.clone(),
                    a.old_model_version_id,
                    b.model_version_id,
                    b.source.clone(),
                    b.reason,
                    a.reached_max_cursor_count || b.reached_max_cursor_count,
                )))
            }
            _ => None,
        }
    }
}

/// `ContentSizeChangedEvent` (implementa `IContentSizeChangedEvent`).
#[derive(Clone, Debug, PartialEq)]
pub struct ContentSizeChangedEvent {
    old_content_width: f64,
    old_content_height: f64,

    pub content_width: f64,
    pub content_height: f64,
    pub content_width_changed: bool,
    pub content_height_changed: bool,
}

impl ContentSizeChangedEvent {
    pub fn new(old_content_width: f64, old_content_height: f64, content_width: f64, content_height: f64) -> Self {
        Self {
            old_content_width,
            old_content_height,
            content_width,
            content_height,
            content_width_changed: old_content_width != content_width,
            content_height_changed: old_content_height != content_height,
        }
    }

    pub fn is_no_op(&self) -> bool {
        !self.content_width_changed && !self.content_height_changed
    }
}

/// Gera as structs `FocusChangedEvent` e `WidgetFocusChangedEvent`: iguais no original, tipos distintos.
macro_rules! focus_event {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub struct $name {
            pub old_has_focus: bool,
            pub has_focus: bool,
        }

        impl $name {
            pub fn new(old_has_focus: bool, has_focus: bool) -> Self {
                Self { old_has_focus, has_focus }
            }

            pub fn is_no_op(&self) -> bool {
                self.old_has_focus == self.has_focus
            }
        }
    };
}

focus_event!(
    /// `FocusChangedEvent`.
    FocusChangedEvent
);

focus_event!(
    /// `WidgetFocusChangedEvent`.
    WidgetFocusChangedEvent
);

/// `ScrollChangedEvent`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrollChangedEvent {
    old_scroll_width: f64,
    old_scroll_left: f64,
    old_scroll_height: f64,
    old_scroll_top: f64,

    pub scroll_width: f64,
    pub scroll_left: f64,
    pub scroll_height: f64,
    pub scroll_top: f64,

    pub scroll_width_changed: bool,
    pub scroll_left_changed: bool,
    pub scroll_height_changed: bool,
    pub scroll_top_changed: bool,
}

impl ScrollChangedEvent {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        old_scroll_width: f64,
        old_scroll_left: f64,
        old_scroll_height: f64,
        old_scroll_top: f64,
        scroll_width: f64,
        scroll_left: f64,
        scroll_height: f64,
        scroll_top: f64,
    ) -> Self {
        Self {
            old_scroll_width,
            old_scroll_left,
            old_scroll_height,
            old_scroll_top,

            scroll_width,
            scroll_left,
            scroll_height,
            scroll_top,

            scroll_width_changed: old_scroll_width != scroll_width,
            scroll_left_changed: old_scroll_left != scroll_left,
            scroll_height_changed: old_scroll_height != scroll_height,
            scroll_top_changed: old_scroll_top != scroll_top,
        }
    }

    pub fn is_no_op(&self) -> bool {
        !self.scroll_width_changed && !self.scroll_left_changed && !self.scroll_height_changed && !self.scroll_top_changed
    }
}

/// Gera os eventos de saída sem dado: nunca são no-op, e a fusão devolve o próprio evento.
macro_rules! unit_outgoing_event {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
        pub struct $name;

        impl $name {
            pub fn new() -> Self {
                Self
            }
        }
    };
}

unit_outgoing_event!(
    /// `ViewZonesChangedEvent` (de saída, distinto do evento de mesmo nome de `viewEvents`).
    ViewZonesChangedEvent
);

unit_outgoing_event!(
    /// `HiddenAreasChangedEvent`.
    HiddenAreasChangedEvent
);

unit_outgoing_event!(
    /// `ReadOnlyEditAttemptEvent`.
    ReadOnlyEditAttemptEvent
);

/// `CursorStateChangedEvent`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CursorStateChangedEvent {
    pub old_selections: Option<Vec<Selection>>,
    pub selections: Vec<Selection>,
    pub old_model_version_id: i64,
    pub model_version_id: i64,
    pub source: String,
    pub reason: CursorChangeReason,
    pub reached_max_cursor_count: bool,
}

impl CursorStateChangedEvent {
    pub fn new(
        old_selections: Option<Vec<Selection>>,
        selections: Vec<Selection>,
        old_model_version_id: i64,
        model_version_id: i64,
        source: String,
        reason: CursorChangeReason,
        reached_max_cursor_count: bool,
    ) -> Self {
        Self { old_selections, selections, old_model_version_id, model_version_id, source, reason, reached_max_cursor_count }
    }

    /// `_selectionsAreEqual(a, b)`: `null` só é igual a `null`, e o resto compara por `equalsSelection`.
    fn selections_are_equal(a: Option<&[Selection]>, b: Option<&[Selection]>) -> bool {
        a == b
    }

    pub fn is_no_op(&self) -> bool {
        Self::selections_are_equal(self.old_selections.as_deref(), Some(self.selections.as_slice()))
            && self.old_model_version_id == self.model_version_id
    }
}

/// Gera os eventos de saída que embrulham um evento do modelo: nunca são no-op nem se fundem.
macro_rules! model_outgoing_event {
    ($(#[$meta:meta])* $name:ident, $inner:ty) => {
        $(#[$meta])*
        #[derive(Clone, Debug)]
        pub struct $name {
            pub event: $inner,
        }

        impl $name {
            pub fn new(event: $inner) -> Self {
                Self { event }
            }
        }
    };
}

model_outgoing_event!(
    /// `ModelDecorationsChangedEvent`.
    ModelDecorationsChangedEvent,
    model_events::ModelDecorationsChangedEvent
);

model_outgoing_event!(
    /// `ModelLanguageChangedEvent`.
    ModelLanguageChangedEvent,
    model_events::ModelLanguageChangedEvent
);

model_outgoing_event!(
    /// `ModelLanguageConfigurationChangedEvent`.
    ModelLanguageConfigurationChangedEvent,
    model_events::ModelLanguageConfigurationChangedEvent
);

model_outgoing_event!(
    /// `ModelContentChangedEvent`.
    ModelContentChangedEvent,
    model_events::ModelContentChangedEvent
);

model_outgoing_event!(
    /// `ModelOptionsChangedEvent`.
    ModelOptionsChangedEvent,
    model_events::ModelOptionsChangedEvent
);

model_outgoing_event!(
    /// `ModelTokensChangedEvent`.
    ModelTokensChangedEvent,
    model_events::ModelTokensChangedEvent
);

model_outgoing_event!(
    /// `ModelLineHeightChangedEvent` (o `OriginalModelLineHeightChangedEvent` do original).
    ModelLineHeightChangedEvent,
    model_events::ModelLineHeightChangedEvent
);

model_outgoing_event!(
    /// `ModelFontChangedEvent` (o `OriginalModelFontChangedEvent` do original).
    ModelFontChangedEvent,
    model_events::ModelFontChangedEvent
);

#[cfg(test)]
mod tests {
    use std::rc::Weak;

    use super::*;
    use crate::view_events::{
        ShouldRenderState, ViewEventType, ViewFlushedEvent, ViewLinesChangedEvent, ViewZonesChangedEvent as ViewZonesViewEvent,
    };

    fn content_size(old_w: f64, old_h: f64, w: f64, h: f64) -> OutgoingViewModelEvent {
        OutgoingViewModelEvent::ContentSizeChanged(ContentSizeChangedEvent::new(old_w, old_h, w, h))
    }

    fn focus(old: bool, new: bool) -> OutgoingViewModelEvent {
        OutgoingViewModelEvent::FocusChanged(FocusChangedEvent::new(old, new))
    }

    fn widget_focus(old: bool, new: bool) -> OutgoingViewModelEvent {
        OutgoingViewModelEvent::WidgetFocusChanged(WidgetFocusChangedEvent::new(old, new))
    }

    #[allow(clippy::too_many_arguments)]
    fn scroll(ow: f64, ol: f64, oh: f64, ot: f64, w: f64, l: f64, h: f64, t: f64) -> OutgoingViewModelEvent {
        OutgoingViewModelEvent::ScrollChanged(ScrollChangedEvent::new(ow, ol, oh, ot, w, l, h, t))
    }

    fn cursor(
        old: Option<Vec<Selection>>,
        selections: Vec<Selection>,
        old_version: i64,
        version: i64,
        source: &str,
        reason: CursorChangeReason,
        reached_max: bool,
    ) -> OutgoingViewModelEvent {
        OutgoingViewModelEvent::CursorStateChanged(CursorStateChangedEvent::new(old, selections, old_version, version, source.to_owned(), reason, reached_max))
    }

    fn decorations(minimap: bool) -> OutgoingViewModelEvent {
        OutgoingViewModelEvent::ModelDecorationsChanged(ModelDecorationsChangedEvent::new(model_events::ModelDecorationsChangedEvent {
            affects_minimap: minimap,
            affects_overview_ruler: false,
            affects_glyph_margin: false,
            affects_line_number: false,
        }))
    }

    /// Escuta o despachante e junta o que ele entrega.
    fn record_outgoing(dispatcher: &ViewModelEventDispatcher) -> Rc<RefCell<Vec<OutgoingViewModelEvent>>> {
        let log = Rc::new(RefCell::new(Vec::new()));
        let sink = log.clone();
        dispatcher.on_event().listen(move |e| sink.borrow_mut().push(e.clone()));
        log
    }

    #[test]
    fn content_size_merge_keeps_oldest_size_and_takes_newest() {
        let a = content_size(0.0, 0.0, 10.0, 20.0);
        let b = content_size(10.0, 20.0, 10.0, 30.0);
        assert!(!a.is_no_op());
        let merged = a.attempt_to_merge(&b).expect("mesmo tipo funde");
        let OutgoingViewModelEvent::ContentSizeChanged(m) = &merged else { panic!("tipo errado") };
        assert_eq!((m.content_width, m.content_height), (10.0, 30.0));
        assert!(m.content_width_changed && m.content_height_changed);
        assert!(!merged.is_no_op());

        // Voltar ao tamanho original deixa a fusão sem efeito.
        let back = content_size(5.0, 5.0, 10.0, 10.0).attempt_to_merge(&content_size(10.0, 10.0, 5.0, 5.0)).expect("funde");
        assert!(back.is_no_op());
    }

    #[test]
    fn content_size_only_one_axis_changed() {
        let e = ContentSizeChangedEvent::new(4.0, 7.0, 4.0, 9.0);
        assert!(!e.content_width_changed);
        assert!(e.content_height_changed);
        assert!(!e.is_no_op());
        assert!(ContentSizeChangedEvent::new(4.0, 7.0, 4.0, 7.0).is_no_op());
    }

    #[test]
    fn focus_merge_and_no_op() {
        assert!(focus(true, true).is_no_op());
        assert!(!focus(false, true).is_no_op());
        let merged = focus(false, true).attempt_to_merge(&focus(true, false)).expect("funde");
        let OutgoingViewModelEvent::FocusChanged(m) = &merged else { panic!("tipo errado") };
        assert_eq!((m.old_has_focus, m.has_focus), (false, false));
        assert!(merged.is_no_op());
    }

    #[test]
    fn widget_focus_merge_produces_a_focus_changed_event_like_the_original() {
        let merged = widget_focus(false, true).attempt_to_merge(&widget_focus(true, true)).expect("funde");
        assert_eq!(merged.kind(), OutgoingViewModelEventKind::FocusChanged);
        let OutgoingViewModelEvent::FocusChanged(m) = &merged else { panic!("tipo errado") };
        assert_eq!((m.old_has_focus, m.has_focus), (false, true));
    }

    #[test]
    fn merge_of_different_kinds_is_none() {
        assert!(focus(false, true).attempt_to_merge(&widget_focus(false, true)).is_none());
        assert!(widget_focus(false, true).attempt_to_merge(&focus(false, true)).is_none());
        assert!(content_size(0.0, 0.0, 1.0, 1.0).attempt_to_merge(&scroll(0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0)).is_none());
    }

    #[test]
    fn scroll_merge_keeps_oldest_and_recomputes_flags() {
        let a = scroll(0.0, 0.0, 100.0, 0.0, 0.0, 0.0, 100.0, 10.0);
        let OutgoingViewModelEvent::ScrollChanged(a_inner) = &a else { panic!("tipo errado") };
        assert!(a_inner.scroll_top_changed);
        assert!(!a_inner.scroll_height_changed);

        let b = scroll(0.0, 0.0, 100.0, 10.0, 0.0, 0.0, 120.0, 30.0);
        let merged = a.attempt_to_merge(&b).expect("funde");
        let OutgoingViewModelEvent::ScrollChanged(m) = &merged else { panic!("tipo errado") };
        assert_eq!((m.scroll_width, m.scroll_left, m.scroll_height, m.scroll_top), (0.0, 0.0, 120.0, 30.0));
        assert!(!m.scroll_width_changed);
        assert!(!m.scroll_left_changed);
        assert!(m.scroll_height_changed);
        assert!(m.scroll_top_changed);
        assert!(!merged.is_no_op());

        assert!(scroll(1.0, 2.0, 3.0, 4.0, 1.0, 2.0, 3.0, 4.0).is_no_op());
        let round_trip = scroll(0.0, 0.0, 100.0, 0.0, 0.0, 0.0, 100.0, 10.0)
            .attempt_to_merge(&scroll(0.0, 0.0, 100.0, 10.0, 0.0, 0.0, 100.0, 0.0))
            .expect("funde");
        assert!(round_trip.is_no_op());
    }

    #[test]
    fn cursor_state_is_no_op_needs_equal_selections_and_version() {
        let s1 = Selection::new(1, 1, 1, 1);
        let s2 = Selection::new(1, 3, 1, 3);
        assert!(cursor(Some(vec![s1]), vec![s1], 5, 5, "keyboard", CursorChangeReason::NotSet, false).is_no_op());
        // Mesma seleção, versão do modelo diferente.
        assert!(!cursor(Some(vec![s1]), vec![s1], 5, 6, "keyboard", CursorChangeReason::NotSet, false).is_no_op());
        // Seleção diferente.
        assert!(!cursor(Some(vec![s1]), vec![s2], 5, 5, "keyboard", CursorChangeReason::NotSet, false).is_no_op());
        // Sem seleção antiga (`null`) nunca é igual a um array.
        assert!(!cursor(None, vec![s1], 5, 5, "keyboard", CursorChangeReason::NotSet, false).is_no_op());
        // Comprimentos diferentes.
        assert!(!cursor(Some(vec![s1]), vec![s1, s2], 5, 5, "keyboard", CursorChangeReason::NotSet, false).is_no_op());
        // Âncora diferente com a mesma posição não é igual.
        assert!(!cursor(Some(vec![Selection::new(1, 1, 1, 3)]), vec![Selection::new(1, 3, 1, 3)], 5, 5, "keyboard", CursorChangeReason::NotSet, false).is_no_op());
    }

    #[test]
    fn cursor_state_merge_keeps_old_state_and_takes_latest_details() {
        let s1 = Selection::new(1, 1, 1, 1);
        let s2 = Selection::new(1, 3, 1, 3);
        let a = cursor(Some(vec![s1]), vec![s2], 1, 2, "keyboard", CursorChangeReason::NotSet, false);
        let b = cursor(Some(vec![s2]), vec![s1], 2, 3, "mouse", CursorChangeReason::Explicit, true);
        let merged = a.attempt_to_merge(&b).expect("funde");
        let OutgoingViewModelEvent::CursorStateChanged(m) = &merged else { panic!("tipo errado") };
        assert_eq!(m.old_selections, Some(vec![s1]));
        assert_eq!(m.selections, vec![s1]);
        assert_eq!((m.old_model_version_id, m.model_version_id), (1, 3));
        assert_eq!(m.source, "mouse");
        assert_eq!(m.reason, CursorChangeReason::Explicit);
        assert!(m.reached_max_cursor_count);
        // As seleções são iguais, mas as versões diferem (1 contra 3).
        assert!(!merged.is_no_op());

        // `reachedMaxCursorCount` vale se qualquer um dos dois chegou ao máximo.
        let first_reached = cursor(Some(vec![s1]), vec![s2], 1, 2, "a", CursorChangeReason::NotSet, true);
        let second_not = cursor(Some(vec![s2]), vec![s1], 2, 3, "b", CursorChangeReason::NotSet, false);
        let OutgoingViewModelEvent::CursorStateChanged(m) = first_reached.attempt_to_merge(&second_not).expect("funde") else { panic!("tipo errado") };
        assert!(m.reached_max_cursor_count);
    }

    #[test]
    fn unit_events_merge_into_themselves_and_are_never_no_op() {
        let zones = OutgoingViewModelEvent::ViewZonesChanged(ViewZonesChangedEvent::new());
        let hidden = OutgoingViewModelEvent::HiddenAreasChanged(HiddenAreasChangedEvent::new());
        let read_only = OutgoingViewModelEvent::ReadOnlyEditAttempt(ReadOnlyEditAttemptEvent::new());
        for event in [zones, hidden, read_only] {
            assert!(!event.is_no_op());
            let merged = event.attempt_to_merge(&event).expect("funde");
            assert_eq!(merged.kind(), event.kind());
        }
    }

    #[test]
    fn model_events_never_merge() {
        let a = decorations(true);
        let b = decorations(false);
        assert!(!a.is_no_op());
        assert!(a.attempt_to_merge(&b).is_none());
        assert!(a.attempt_to_merge(&a).is_none());
    }

    #[test]
    fn dispatcher_emits_immediately_and_drops_no_ops() {
        let dispatcher = ViewModelEventDispatcher::new();
        let log = record_outgoing(&dispatcher);

        dispatcher.emit_outgoing_event(focus(true, true));
        assert!(log.borrow().is_empty());

        dispatcher.emit_outgoing_event(focus(false, true));
        assert_eq!(log.borrow().len(), 1);
        assert_eq!(log.borrow()[0].kind(), OutgoingViewModelEventKind::FocusChanged);
    }

    #[test]
    fn dispatcher_postpones_and_merges_outgoing_events_while_collecting() {
        let dispatcher = ViewModelEventDispatcher::new();
        let log = record_outgoing(&dispatcher);

        let collector = dispatcher.begin_emit_view_events();
        collector.emit_outgoing_event(focus(false, true));
        collector.emit_outgoing_event(focus(true, false));
        collector.emit_outgoing_event(content_size(0.0, 0.0, 10.0, 10.0));
        // Um evento direto durante a coleta também espera.
        dispatcher.emit_outgoing_event(scroll(0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 5.0));
        assert!(log.borrow().is_empty());

        dispatcher.end_emit_view_events();

        // O foco voltou ao que era (no-op, descartado); sobram o evento direto e o tamanho, na ordem em
        // que entraram na fila: o direto já estava lá, o coletado entra no `end`.
        let kinds: Vec<_> = log.borrow().iter().map(OutgoingViewModelEvent::kind).collect();
        assert_eq!(kinds, vec![OutgoingViewModelEventKind::ScrollChanged, OutgoingViewModelEventKind::ContentSizeChanged]);
    }

    #[test]
    fn dispatcher_nested_collectors_only_flush_at_the_outermost_end() {
        let dispatcher = ViewModelEventDispatcher::new();
        let log = record_outgoing(&dispatcher);

        let outer = dispatcher.begin_emit_view_events();
        let inner = dispatcher.begin_emit_view_events();
        assert!(Rc::ptr_eq(&outer, &inner));
        inner.emit_outgoing_event(focus(false, true));
        dispatcher.end_emit_view_events();
        assert!(log.borrow().is_empty());
        dispatcher.end_emit_view_events();
        assert_eq!(log.borrow().len(), 1);
    }

    #[test]
    fn dispatcher_keeps_model_events_unmerged_and_merges_zones_changed() {
        let dispatcher = ViewModelEventDispatcher::new();
        let log = record_outgoing(&dispatcher);

        let collector = dispatcher.begin_emit_view_events();
        collector.emit_outgoing_event(decorations(true));
        collector.emit_outgoing_event(decorations(false));
        collector.emit_outgoing_event(OutgoingViewModelEvent::ViewZonesChanged(ViewZonesChangedEvent::new()));
        collector.emit_outgoing_event(OutgoingViewModelEvent::ViewZonesChanged(ViewZonesChangedEvent::new()));
        dispatcher.end_emit_view_events();

        let kinds: Vec<_> = log.borrow().iter().map(OutgoingViewModelEvent::kind).collect();
        assert_eq!(
            kinds,
            vec![
                OutgoingViewModelEventKind::ModelDecorationsChanged,
                OutgoingViewModelEventKind::ModelDecorationsChanged,
                OutgoingViewModelEventKind::ViewZonesChanged
            ]
        );
    }

    /// Um handler que anota os tipos de evento que recebeu e pede render só para `Flushed`.
    struct Recorder {
        state: ShouldRenderState,
        log: Rc<RefCell<Vec<(&'static str, ViewEventType)>>>,
        name: &'static str,
        on_flushed: RefCell<Option<Box<dyn Fn()>>>,
    }

    impl Recorder {
        fn new(name: &'static str, log: &Rc<RefCell<Vec<(&'static str, ViewEventType)>>>) -> Rc<Self> {
            Rc::new(Self { state: ShouldRenderState::default(), log: log.clone(), name, on_flushed: RefCell::new(None) })
        }
    }

    impl ViewEventHandler for Recorder {
        fn should_render_state(&self) -> &ShouldRenderState {
            &self.state
        }

        fn on_flushed(&self, _e: &ViewFlushedEvent) -> bool {
            self.log.borrow_mut().push((self.name, ViewEventType::ViewFlushed));
            let action = self.on_flushed.borrow_mut().take();
            if let Some(action) = action {
                action();
            }
            true
        }

        fn on_zones_changed(&self, _e: &ViewZonesViewEvent) -> bool {
            self.log.borrow_mut().push((self.name, ViewEventType::ViewZonesChanged));
            false
        }

        fn on_lines_changed(&self, _e: &ViewLinesChangedEvent) -> bool {
            self.log.borrow_mut().push((self.name, ViewEventType::ViewLinesChanged));
            false
        }
    }

    #[test]
    fn view_events_reach_every_handler_in_order_and_flag_should_render() {
        let dispatcher = ViewModelEventDispatcher::new();
        let log = Rc::new(RefCell::new(Vec::new()));
        let a = Recorder::new("a", &log);
        let b = Recorder::new("b", &log);
        a.on_did_render();
        b.on_did_render();
        assert!(!a.should_render());
        dispatcher.add_view_event_handler(a.clone());
        dispatcher.add_view_event_handler(b.clone());

        let collector = dispatcher.begin_emit_view_events();
        collector.emit_view_event(ViewEvent::LinesChanged(ViewLinesChangedEvent::new(3, 2)));
        collector.emit_view_event(ViewEvent::Flushed(ViewFlushedEvent));
        // Nada é entregue enquanto o coletor está aberto.
        assert!(log.borrow().is_empty());
        dispatcher.end_emit_view_events();

        assert_eq!(
            *log.borrow(),
            vec![
                ("a", ViewEventType::ViewLinesChanged),
                ("a", ViewEventType::ViewFlushed),
                ("b", ViewEventType::ViewLinesChanged),
                ("b", ViewEventType::ViewFlushed),
            ]
        );
        // `Flushed` devolveu true, então o handler pede render de novo.
        assert!(a.should_render());
        assert!(b.should_render());
    }

    #[test]
    fn handler_returning_false_does_not_request_render() {
        let dispatcher = ViewModelEventDispatcher::new();
        let log = Rc::new(RefCell::new(Vec::new()));
        let a = Recorder::new("a", &log);
        a.on_did_render();
        dispatcher.add_view_event_handler(a.clone());
        dispatcher.emit_single_view_event(ViewEvent::LinesChanged(ViewLinesChangedEvent::new(1, 1)));
        assert_eq!(*log.borrow(), vec![("a", ViewEventType::ViewLinesChanged)]);
        assert!(!a.should_render());
    }

    #[test]
    fn view_event_emitted_by_a_handler_is_queued_after_the_current_batch() {
        let dispatcher = ViewModelEventDispatcher::new();
        let log = Rc::new(RefCell::new(Vec::new()));
        let a = Recorder::new("a", &log);
        let b = Recorder::new("b", &log);

        let weak: Weak<ViewModelEventDispatcher> = Rc::downgrade(&dispatcher);
        *a.on_flushed.borrow_mut() = Some(Box::new(move || {
            if let Some(dispatcher) = weak.upgrade() {
                dispatcher.emit_single_view_event(ViewEvent::ZonesChanged(ViewZonesViewEvent));
            }
        }));
        dispatcher.add_view_event_handler(a.clone());
        dispatcher.add_view_event_handler(b.clone());

        dispatcher.emit_single_view_event(ViewEvent::Flushed(ViewFlushedEvent));

        // O lote do `Flushed` termina em todos os handlers antes de o `ZonesChanged` ser entregue.
        assert_eq!(
            *log.borrow(),
            vec![
                ("a", ViewEventType::ViewFlushed),
                ("b", ViewEventType::ViewFlushed),
                ("a", ViewEventType::ViewZonesChanged),
                ("b", ViewEventType::ViewZonesChanged),
            ]
        );
    }

    #[test]
    fn outgoing_events_wait_while_view_events_are_being_consumed() {
        let dispatcher = ViewModelEventDispatcher::new();
        let outgoing = record_outgoing(&dispatcher);
        let log = Rc::new(RefCell::new(Vec::new()));
        let a = Recorder::new("a", &log);

        let weak = Rc::downgrade(&dispatcher);
        let outgoing_seen_inside = Rc::new(Cell::new(usize::MAX));
        let (probe_log, probe) = (outgoing.clone(), outgoing_seen_inside.clone());
        *a.on_flushed.borrow_mut() = Some(Box::new(move || {
            if let Some(dispatcher) = weak.upgrade() {
                dispatcher.emit_outgoing_event(focus(false, true));
                // Durante o consumo da fila, o evento de saída fica retido.
                probe.set(probe_log.borrow().len());
            }
        }));
        dispatcher.add_view_event_handler(a.clone());

        dispatcher.emit_single_view_event(ViewEvent::Flushed(ViewFlushedEvent));

        assert_eq!(outgoing_seen_inside.get(), 0);
        // Ao fim do `end_emit_view_events`, o evento sai.
        assert_eq!(outgoing.borrow().len(), 1);
    }

    #[test]
    fn removed_handler_stops_receiving_and_a_removal_during_dispatch_is_safe() {
        let dispatcher = ViewModelEventDispatcher::new();
        let log = Rc::new(RefCell::new(Vec::new()));
        let a = Recorder::new("a", &log);
        let b = Recorder::new("b", &log);
        let a_dyn: Rc<dyn ViewEventHandler> = a.clone();
        let b_dyn: Rc<dyn ViewEventHandler> = b.clone();

        // `a` tira `b` da lista no meio do despacho; `b` ainda recebe esse lote (a lista foi clonada).
        let weak = Rc::downgrade(&dispatcher);
        let b_for_removal = b_dyn.clone();
        *a.on_flushed.borrow_mut() = Some(Box::new(move || {
            if let Some(dispatcher) = weak.upgrade() {
                dispatcher.remove_view_event_handler(&b_for_removal);
            }
        }));
        dispatcher.add_view_event_handler(a_dyn);
        dispatcher.add_view_event_handler(b_dyn);

        dispatcher.emit_single_view_event(ViewEvent::Flushed(ViewFlushedEvent));
        assert_eq!(*log.borrow(), vec![("a", ViewEventType::ViewFlushed), ("b", ViewEventType::ViewFlushed)]);

        log.borrow_mut().clear();
        dispatcher.emit_single_view_event(ViewEvent::ZonesChanged(ViewZonesViewEvent));
        assert_eq!(*log.borrow(), vec![("a", ViewEventType::ViewZonesChanged)]);
    }

    #[test]
    fn disposing_the_dispatcher_drops_event_listeners() {
        let dispatcher = ViewModelEventDispatcher::new();
        let log = record_outgoing(&dispatcher);
        dispatcher.emit_outgoing_event(focus(false, true));
        assert_eq!(log.borrow().len(), 1);
        dispatcher.dispose();
        dispatcher.emit_outgoing_event(focus(true, false));
        assert_eq!(log.borrow().len(), 1);
    }
}
