//! Porte de `src/vs/editor/browser/config/elementSizeObserver.ts`: mede o elemento de referência do editor
//! (largura e altura, com mínimo de 5) e avisa quando muda.
//!
//! Divergências:
//! - O `HTMLElement` e o `ResizeObserver` do DOM são o trait [`ReferenceDomElement`], que o hospedeiro implementa
//!   (`clientWidth`, `clientHeight`, o `ResizeObserver.observe` e o `scheduleAtNextAnimationFrame(getWindow(el), ...)`).
//!   Sem elemento (`None`, o `null` do original) só vale a dimensão passada, ou 0.
//! - `entries[0].contentRect` é o argumento `Option<Dimension>` do callback de `observe_resize` (`None` quando a
//!   entrada não tem retângulo).
//! - O `try { ... } finally { schedule }` de `update` é o guarda [`RunOnDrop`]: o agendamento do próximo quadro roda
//!   também se `observe` entrar em pânico.
//! - `ElementSizeObserver extends Disposable` é composição com o [`Disposable`] base; `dispose` para de observar antes.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, DisposableRef, IDisposable};

use crate::core_2d::dimension::Dimension;

/// O `HTMLElement | null` de referência do observador, com o que o original lê dele.
pub trait ReferenceDomElement {
    /// `clientWidth`.
    fn client_width(&self) -> f64;
    /// `clientHeight`.
    fn client_height(&self) -> f64;
    /// `new ResizeObserver(callback).observe(element)`: o callback recebe o `contentRect` da primeira entrada (ou
    /// `None`). Soltar o disposable devolvido é o `disconnect()`.
    fn observe_resize(&self, on_resize: Rc<dyn Fn(Option<Dimension>)>) -> DisposableRef;
    /// `scheduleAtNextAnimationFrame(getWindow(element), callback)`.
    fn schedule_at_next_animation_frame(&self, callback: Box<dyn FnOnce()>) -> DisposableRef;
}

/// Roda a função ao sair do escopo (o `finally`).
struct RunOnDrop(Option<Box<dyn FnOnce()>>);

impl Drop for RunOnDrop {
    fn drop(&mut self) {
        if let Some(f) = self.0.take() {
            f();
        }
    }
}

/// O estado do `startObserving`: as variáveis `observedDimension`, `shouldObserve` e `alreadyObservedThisAnimationFrame`.
struct ObserveState {
    observed_dimension: Cell<Option<Dimension>>,
    should_observe: Cell<bool>,
    already_observed_this_animation_frame: Cell<bool>,
}

/// `ElementSizeObserver`.
pub struct ElementSizeObserver {
    base: Rc<Disposable>,
    me: Weak<ElementSizeObserver>,
    on_did_change: Rc<Emitter<()>>,
    reference_dom_element: Option<Rc<dyn ReferenceDomElement>>,
    width: Cell<f64>,
    height: Cell<f64>,
    resize_observer: RefCell<Option<DisposableRef>>,
}

impl ElementSizeObserver {
    pub fn new(reference_dom_element: Option<Rc<dyn ReferenceDomElement>>, dimension: Option<Dimension>) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change = Emitter::<()>::new();
        base.register(on_did_change.clone());
        let this = Rc::new_cyclic(|me| Self {
            base,
            me: me.clone(),
            on_did_change,
            reference_dom_element,
            width: Cell::new(-1.0),
            height: Cell::new(-1.0),
            resize_observer: RefCell::new(None),
        });
        this.measure_reference_dom_element(false, dimension);
        this
    }

    /// `onDidChange`.
    pub fn on_did_change(&self) -> Event<()> {
        self.on_did_change.event()
    }

    pub fn get_width(&self) -> f64 {
        self.width.get()
    }

    pub fn get_height(&self) -> f64 {
        self.height.get()
    }

    pub fn start_observing(&self) {
        let Some(element) = self.reference_dom_element.clone() else { return };
        if self.resize_observer.borrow().is_some() {
            return;
        }
        // Reage ao observador de tamanho uma vez por quadro de animação: a primeira vez na hora, as seguintes no
        // próximo quadro.
        let state = Rc::new(ObserveState {
            observed_dimension: Cell::new(None),
            should_observe: Cell::new(false),
            already_observed_this_animation_frame: Cell::new(false),
        });
        let me = self.me.clone();
        let observed_element = element.clone();
        let subscription = element.observe_resize(Rc::new(move |entry_dimension: Option<Dimension>| {
            state.observed_dimension.set(entry_dimension);
            state.should_observe.set(true);
            Self::update(&me, &observed_element, &state);
        }));
        *self.resize_observer.borrow_mut() = Some(subscription);
    }

    /// `update` de `startObserving`.
    fn update(me: &Weak<ElementSizeObserver>, element: &Rc<dyn ReferenceDomElement>, state: &Rc<ObserveState>) {
        if !state.should_observe.get() || state.already_observed_this_animation_frame.get() {
            return;
        }
        let (schedule_element, schedule_state, schedule_me) = (element.clone(), state.clone(), me.clone());
        let _schedule_next_frame = RunOnDrop(Some(Box::new(move || {
            let (element, state, me) = (schedule_element.clone(), schedule_state.clone(), schedule_me.clone());
            // O disposable do agendamento não é guardado: o original também não cancela este quadro.
            schedule_element.schedule_at_next_animation_frame(Box::new(move || {
                state.already_observed_this_animation_frame.set(false);
                Self::update(&me, &element, &state);
            }));
        })));
        state.should_observe.set(false);
        state.already_observed_this_animation_frame.set(true);
        if let Some(this) = me.upgrade() {
            // `observeNow`.
            this.observe(state.observed_dimension.get());
        }
    }

    pub fn stop_observing(&self) {
        let resize_observer = self.resize_observer.borrow_mut().take();
        if let Some(resize_observer) = resize_observer {
            resize_observer.dispose();
        }
    }

    pub fn observe(&self, dimension: Option<Dimension>) {
        self.measure_reference_dom_element(true, dimension);
    }

    fn measure_reference_dom_element(&self, emit_event: bool, dimension: Option<Dimension>) {
        let (mut observed_width, mut observed_height) = (0.0, 0.0);
        if let Some(dimension) = dimension {
            observed_width = dimension.width;
            observed_height = dimension.height;
        } else if let Some(element) = &self.reference_dom_element {
            observed_width = element.client_width();
            observed_height = element.client_height();
        }
        // `Math.max(5, x)`: `NaN` se `x` é `NaN`.
        let at_least_five = |value: f64| if value.is_nan() { f64::NAN } else { value.max(5.0) };
        observed_width = at_least_five(observed_width);
        observed_height = at_least_five(observed_height);
        if self.width.get() != observed_width || self.height.get() != observed_height {
            self.width.set(observed_width);
            self.height.set(observed_height);
            if emit_event {
                self.on_did_change.fire(());
            }
        }
    }
}

impl IDisposable for ElementSizeObserver {
    fn dispose(&self) {
        self.stop_observing();
        self.base.dispose();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minimum_of_five_and_event_only_on_observe() {
        let observer = ElementSizeObserver::new(None, Some(Dimension { width: 100.0, height: 2.0 }));
        assert_eq!((observer.get_width(), observer.get_height()), (100.0, 5.0));

        let fired = Rc::new(Cell::new(0));
        let subscription = observer.on_did_change().listen({
            let fired = fired.clone();
            move |_| fired.set(fired.get() + 1)
        });
        observer.observe(Some(Dimension { width: 100.0, height: 3.0 }));
        assert_eq!(fired.get(), 0, "a altura continua 5");
        observer.observe(Some(Dimension { width: 120.0, height: 40.0 }));
        assert_eq!((fired.get(), observer.get_width(), observer.get_height()), (1, 120.0, 40.0));
        observer.observe(None);
        assert_eq!((observer.get_width(), observer.get_height()), (5.0, 5.0));
        subscription.dispose();
        observer.dispose();
    }

    #[test]
    fn without_dimension_or_element_it_is_five_by_five() {
        let observer = ElementSizeObserver::new(None, None);
        assert_eq!((observer.get_width(), observer.get_height()), (5.0, 5.0));
        observer.start_observing();
        observer.dispose();
    }
}
