//! A decoração e o elemento de mentira do `TestTerminal` do teste do `DecorationAddon` (o `registerDecoration` que devolve um
//! objeto em vez de desenhar no xterm): o elemento guarda a lista de classes e os ouvintes, e avisa o emulador de teste de cada
//! `removeEventListener`; a decoração guarda os ouvintes de `onDispose`, dispara o `onRender` na hora de cada assinatura e
//! dispara os de `onDispose` ao ser descartada.
//!
//! Como o objeto do teste do original não tem `options`, `setOverviewRulerColor` não faz nada, como a atribuição a
//! `decoration.options?.overviewRulerOptions` do `DecorationAddon`.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::event::{Event, Listener};
use je_base::lifecycle::{DisposableRef, IDisposable, plain_disposable};
use je_platform::terminal::capabilities::capabilities::IMarker;
use je_workbench::contrib::terminal::browser::xterm::xterm_decoration::{
    DecorationEventType, DecorationListener, DecorationStyleProperty, IDecorationElement, IDecorationOptions, IXtermDecoration,
};

/// O `div` do `registerDecoration` do teste.
pub struct FakeDecorationElement {
    classes: RefCell<Vec<String>>,
    styles: RefCell<Vec<(DecorationStyleProperty, String)>>,
    listeners: Rc<RefCell<Vec<(u64, DecorationEventType, DecorationListener)>>>,
    next_listener_id: Cell<u64>,
    removed_event_listeners: Rc<RefCell<Vec<String>>>,
}

impl FakeDecorationElement {
    pub fn new(removed_event_listeners: Rc<RefCell<Vec<String>>>) -> Rc<Self> {
        Rc::new(Self {
            classes: RefCell::new(Vec::new()),
            styles: RefCell::new(Vec::new()),
            listeners: Rc::default(),
            next_listener_id: Cell::new(0),
            removed_event_listeners,
        })
    }

    /// `element.style.<property>`, o último valor escrito.
    pub fn style(&self, property: DecorationStyleProperty) -> Option<String> {
        self.styles.borrow().iter().rev().find(|(written, _)| *written == property).map(|(_, value)| value.clone())
    }

    /// Quantos ouvintes do tipo estão registrados.
    pub fn listener_count(&self, event_type: DecorationEventType) -> usize {
        self.listeners.borrow().iter().filter(|(_, registered, _)| *registered == event_type).count()
    }
}

impl IDecorationElement for FakeDecorationElement {
    fn class_list(&self) -> Vec<String> {
        self.classes.borrow().clone()
    }

    fn class_list_contains(&self, name: &str) -> bool {
        self.classes.borrow().iter().any(|class| class == name)
    }

    fn class_list_add(&self, names: &[&str]) {
        let mut classes = self.classes.borrow_mut();
        for name in names {
            if !classes.iter().any(|class| class == name) {
                classes.push((*name).to_owned());
            }
        }
    }

    fn class_list_remove(&self, name: &str) {
        self.classes.borrow_mut().retain(|class| class != name);
    }

    fn remove_attribute(&self, _name: &str) {}

    fn set_style(&self, property: DecorationStyleProperty, value: &str) {
        self.styles.borrow_mut().push((property, value.to_owned()));
    }

    fn add_disposable_listener(&self, event_type: DecorationEventType, listener: DecorationListener) -> DisposableRef {
        let id = self.next_listener_id.get() + 1;
        self.next_listener_id.set(id);
        self.listeners.borrow_mut().push((id, event_type, listener));
        let (listeners, removed) = (self.listeners.clone(), self.removed_event_listeners.clone());
        plain_disposable(move || {
            removed.borrow_mut().push(event_type.as_str().to_owned());
            listeners.borrow_mut().retain(|(listener_id, ..)| *listener_id != id);
        })
    }
}

/// O objeto que o `registerDecoration` do teste devolve.
pub struct FakeDecoration {
    marker: Rc<dyn IMarker>,
    element: Rc<FakeDecorationElement>,
    dispose_listeners: Rc<RefCell<Vec<(u64, Listener<()>)>>>,
    next_listener_id: Rc<Cell<u64>>,
    is_disposed: Cell<bool>,
}

impl FakeDecoration {
    /// `registerDecoration(options)`: `None` quando o marcador já foi descartado.
    pub fn create(options: IDecorationOptions, removed_event_listeners: Rc<RefCell<Vec<String>>>) -> Option<Rc<dyn IXtermDecoration>> {
        if options.marker.is_disposed() {
            return None;
        }
        Some(Rc::new(Self {
            marker: options.marker,
            element: FakeDecorationElement::new(removed_event_listeners),
            dispose_listeners: Rc::default(),
            next_listener_id: Rc::default(),
            is_disposed: Cell::new(false),
        }))
    }
}

impl IDisposable for FakeDecoration {
    fn dispose(&self) {
        self.is_disposed.set(true);
        let listeners: Vec<Listener<()>> = self.dispose_listeners.borrow().iter().map(|(_, listener)| listener.clone()).collect();
        for listener in listeners {
            listener(&());
        }
        self.dispose_listeners.borrow_mut().clear();
    }
}

impl IXtermDecoration for FakeDecoration {
    fn marker(&self) -> Rc<dyn IMarker> {
        self.marker.clone()
    }

    fn element(&self) -> Option<Rc<dyn IDecorationElement>> {
        Some(self.element.clone())
    }

    fn is_disposed(&self) -> bool {
        self.is_disposed.get()
    }

    /// `onRender: (listener) => { listener(element); return { dispose: () => { } }; }`.
    fn on_render(&self) -> Event<Rc<dyn IDecorationElement>> {
        let element: Rc<dyn IDecorationElement> = self.element.clone();
        Event::new(move |listener| {
            listener(&element);
            plain_disposable(|| {})
        })
    }

    /// `onDispose: (listener) => { disposeListeners.add(listener); return { dispose: () => disposeListeners.delete(listener) }; }`.
    fn on_dispose(&self) -> Event<()> {
        let listeners = self.dispose_listeners.clone();
        let next_id = self.next_listener_id.clone();
        Event::new(move |listener| {
            let id = next_id.get() + 1;
            next_id.set(id);
            listeners.borrow_mut().push((id, listener));
            let listeners = listeners.clone();
            plain_disposable(move || listeners.borrow_mut().retain(|(listener_id, _)| *listener_id != id))
        })
    }

    fn set_overview_ruler_color(&self, _color: &str) {}
}
