//! Porte de `src/vs/platform/userInteraction/browser/userInteractionService.ts` (o contrato `IUserInteractionService` e o
//! `MockUserInteractionService`) e de `userInteractionServiceImpl.ts` (o `UserInteractionService`): o acompanhamento do
//! foco, do hover e dos modificadores do teclado, que os testes podem trocar por estados simulados.
//!
//! Notas de tradução:
//! - **O elemento é a [`UiElement`].** `HTMLElement | Window | Element` do original é um nó da árvore nativa (`je-ui`) com
//!   a [`Ui`] que o contém; a `Window` é a raiz da árvore ([`UiElement::window`]), que no `je-ui` faz o papel de `window` e
//!   de `document` ao mesmo tempo. Por isso `hasFocusWithin(window)` (`document.hasFocus()`) vira "a raiz contém o foco",
//!   que só é verdade com algum nó focado.
//! - **Foco e hover vêm dos eventos da `Ui`.** `trackFocus` é o `FocusTracker` de `je-widgets` (que adia a perda do foco
//!   para o próximo ciclo, como o `dom.ts`), e `mouseenter`/`mouseleave` são `PointerEnter`/`PointerLeave`. O `ShadowRoot`
//!   do original não existe (a árvore nativa tem um só escopo de foco).
//! - **`ModifierKeyEmitter`.** O emissor global do original escuta teclado, `blur` e `mousemove` da janela; aqui os
//!   modificadores vêm do último `KeyDown`/`KeyUp` entregue à raiz da `Ui` (captura). O observável de cada janela é
//!   guardado pela identidade da árvore (o `WeakMap<Window, ...>` do original). A cada mudança o observável avisa
//!   (`observableFromEvent` cria um objeto novo a cada evento, por isso [`ModifierKeyStatus`] nunca é "igual").
//! - **A subclasse anônima do fixture.** `createFixtureUserInteractionService` estende o `UserInteractionService` só para
//!   trocar o `createHoverTracker`; aqui o serviço tem o campo `simulate_hover` ([`UserInteractionService::with_simulated_hover`]).
//! - **`queueMicrotask`.** O `createDomFocusTracker` do mock dispara o foco numa microtarefa do laço de `je_base::runtime`.
//! - **Registro como singleton.** O `registerSingleton(..., Delayed)` é a ligação do aplicativo, que ainda não existe
//!   para este serviço; fica o identificador [`I_USER_INTERACTION_SERVICE`].

use std::cell::RefCell;
use std::rc::Rc;

use je_base::event::{Emitter, Event as BaseEvent};
use je_base::lifecycle::{Disposable, DisposableRef, DisposableStore, IDisposable, mark_as_singleton};
use je_base::observable::base::{IObservable, IReader, Observable, SettableExt};
use je_base::observable::deps::StrictEquals;
use je_base::observable::{const_observable, observable_value};
use je_base::runtime::queue_microtask;
use je_instantiation::service_id;
use je_ui::{Event, EventKind, NodeId, Tree, Ui};
use je_widgets::dom::{FocusTracker, is_ancestor_of_active_element};

/// O elemento (ou a janela) que o serviço acompanha: um nó da árvore nativa e a [`Ui`] dele.
#[derive(Clone)]
pub struct UiElement {
    ui: Ui,
    node: NodeId,
}

impl UiElement {
    pub fn new(ui: &Ui, node: NodeId) -> Self {
        Self { ui: ui.clone(), node }
    }

    /// A `Window`: a raiz da árvore.
    pub fn window(ui: &Ui) -> Self {
        Self { ui: ui.clone(), node: ui.root() }
    }

    pub fn ui(&self) -> &Ui {
        &self.ui
    }

    pub fn node(&self) -> NodeId {
        self.node
    }

    /// `el.contains(document.activeElement)`: o nó, ou um descendente dele, tem o foco.
    fn has_focus_within(&self) -> bool {
        is_ancestor_of_active_element(&self.ui, self.node)
    }
}

/// `IModifierKeyStatus`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ModifierKeyStatus {
    pub ctrl_key: bool,
    pub shift_key: bool,
    pub alt_key: bool,
    pub meta_key: bool,
}

impl StrictEquals for ModifierKeyStatus {
    /// O observável do original entrega um objeto novo a cada evento, e o `===` entre objetos distintos é falso.
    fn strict_equals(&self, _other: &Self) -> bool {
        false
    }
}

/// `IFocusTracker` de `dom.ts`, como o serviço o devolve.
pub trait IDomFocusTracker: IDisposable {
    fn on_did_focus(&self) -> BaseEvent<()>;

    fn on_did_blur(&self) -> BaseEvent<()>;

    /// `refreshState`: recalcula se o elemento tem o foco.
    fn refresh_state(&self);
}

/// `IUserInteractionService`.
pub trait IUserInteractionService {
    /// `readModifierKeyStatus(element, reader)`: os modificadores da janela que contém o elemento.
    fn read_modifier_key_status(&self, element: &UiElement, reader: Option<&dyn IReader>) -> ModifierKeyStatus;

    /// `createFocusTracker(element, store)`: o observável de "o elemento (ou um descendente) tem o foco", que acaba
    /// quando o `store` é descartado.
    fn create_focus_tracker(&self, element: &UiElement, store: &DisposableStore) -> IObservable<bool>;

    /// `createHoverTracker(element, store)`: o observável de "o elemento está sob o ponteiro".
    fn create_hover_tracker(&self, element: &UiElement, store: &DisposableStore) -> IObservable<bool>;

    /// `createDomFocusTracker(element)`.
    fn create_dom_focus_tracker(&self, element: &UiElement) -> Rc<dyn IDomFocusTracker>;
}

service_id!(pub I_USER_INTERACTION_SERVICE: dyn IUserInteractionService = "userInteractionService");

/// O `FocusTracker` de `je-widgets` ligado ao elemento que ele acompanha.
struct DomFocusTracker {
    tracker: Rc<FocusTracker>,
    element: UiElement,
}

impl IDisposable for DomFocusTracker {
    fn dispose(&self) {
        self.tracker.dispose();
    }
}

impl IDomFocusTracker for DomFocusTracker {
    fn on_did_focus(&self) -> BaseEvent<()> {
        self.tracker.on_did_focus()
    }

    fn on_did_blur(&self) -> BaseEvent<()> {
        self.tracker.on_did_blur()
    }

    fn refresh_state(&self) {
        self.tracker.refresh_state(self.element.ui(), self.element.node());
    }
}

/// `UserInteractionService`.
pub struct UserInteractionService {
    simulate_hover: bool,
    /// O observável dos modificadores de cada janela, pela identidade da árvore.
    modifier_observables: RefCell<Vec<(usize, IObservable<ModifierKeyStatus>)>>,
    /// Os ouvintes de teclado da raiz de cada janela: vivem tanto quanto o `ModifierKeyEmitter` singleton do original.
    modifier_listeners: RefCell<Vec<DisposableRef>>,
}

impl UserInteractionService {
    pub fn new() -> Rc<Self> {
        Self::with_simulated_hover(false)
    }

    /// O serviço com o hover fixo em verdadeiro (`simulate_hover`) ou o real (falso).
    pub fn with_simulated_hover(simulate_hover: bool) -> Rc<Self> {
        Rc::new(Self { simulate_hover, modifier_observables: RefCell::new(Vec::new()), modifier_listeners: RefCell::new(Vec::new()) })
    }

    /// O observável dos modificadores da janela da `Ui`, criado (com os ouvintes) na primeira leitura.
    fn modifier_observable(&self, ui: &Ui) -> IObservable<ModifierKeyStatus> {
        let identity = ui_identity(ui);
        let existing = self.modifier_observables.borrow().iter().find(|(id, _)| *id == identity).map(|(_, observable)| observable.clone());
        if let Some(observable) = existing {
            return observable;
        }
        let value = observable_value::<ModifierKeyStatus, ()>("modifierKeyStatus", ModifierKeyStatus::default());
        for kind in [EventKind::KeyDown, EventKind::KeyUp] {
            let target = value.clone();
            let listener = ui.listen(ui.root(), kind, true, move |event: &mut Event| {
                if let Some(key) = event.key {
                    let status = ModifierKeyStatus {
                        ctrl_key: key.modifiers.ctrl,
                        shift_key: key.modifiers.shift,
                        alt_key: key.modifiers.alt,
                        meta_key: key.modifiers.meta,
                    };
                    target.set(status, None);
                }
            });
            self.modifier_listeners.borrow_mut().push(mark_as_singleton(listener));
        }
        let observable: IObservable<ModifierKeyStatus> = value;
        self.modifier_observables.borrow_mut().push((identity, observable.clone()));
        observable
    }
}

/// A identidade da árvore de uma janela: o endereço da `Tree` dentro da `Ui` (o `WeakMap<Window, ...>` do original).
fn ui_identity(ui: &Ui) -> usize {
    let tree = ui.tree();
    let reference: &Tree = &tree;
    reference as *const Tree as usize
}

impl IUserInteractionService for UserInteractionService {
    fn read_modifier_key_status(&self, element: &UiElement, reader: Option<&dyn IReader>) -> ModifierKeyStatus {
        self.modifier_observable(element.ui()).read_opt(reader)
    }

    fn create_focus_tracker(&self, element: &UiElement, store: &DisposableStore) -> IObservable<bool> {
        let tracker = store.add_rc(FocusTracker::new(element.ui(), element.node()));
        let value = observable_value::<bool, ()>("isFocused", element.has_focus_within());
        let on_focus = value.clone();
        store.add(tracker.on_did_focus().listen(move |_| on_focus.set(true, None)));
        let on_blur = value.clone();
        store.add(tracker.on_did_blur().listen(move |_| on_blur.set(false, None)));
        value
    }

    fn create_hover_tracker(&self, element: &UiElement, store: &DisposableStore) -> IObservable<bool> {
        if self.simulate_hover {
            return const_observable(true);
        }
        let value = observable_value::<bool, ()>("isHovered", false);
        let on_enter = value.clone();
        store.add(element.ui().listen(element.node(), EventKind::PointerEnter, false, move |_| on_enter.set(true, None)));
        let on_leave = value.clone();
        store.add(element.ui().listen(element.node(), EventKind::PointerLeave, false, move |_| on_leave.set(false, None)));
        value
    }

    fn create_dom_focus_tracker(&self, element: &UiElement) -> Rc<dyn IDomFocusTracker> {
        Rc::new(DomFocusTracker { tracker: FocusTracker::new(element.ui(), element.node()), element: element.clone() })
    }
}

/// `MockUserInteractionService`: o foco e o hover simulados, para os testes e os fixtures.
pub struct MockUserInteractionService {
    simulate_focus: bool,
    simulate_hover: bool,
    modifiers: ModifierKeyStatus,
}

impl MockUserInteractionService {
    /// `new MockUserInteractionService(simulateFocus, simulateHover)`, sem modificadores pressionados.
    pub fn new(simulate_focus: bool, simulate_hover: bool) -> Rc<Self> {
        Self::with_modifiers(simulate_focus, simulate_hover, ModifierKeyStatus::default())
    }

    pub fn with_modifiers(simulate_focus: bool, simulate_hover: bool, modifiers: ModifierKeyStatus) -> Rc<Self> {
        Rc::new(Self { simulate_focus, simulate_hover, modifiers })
    }
}

/// O rastreador do mock: só dispara o foco simulado.
struct MockDomFocusTracker {
    base: Rc<Disposable>,
    did_focus: Rc<Emitter<()>>,
    did_blur: Rc<Emitter<()>>,
}

impl MockDomFocusTracker {
    fn new() -> Rc<Self> {
        let base = Disposable::new();
        let did_focus = Emitter::<()>::new();
        base.register(did_focus.clone());
        let did_blur = Emitter::<()>::new();
        base.register(did_blur.clone());
        Rc::new(Self { base, did_focus, did_blur })
    }
}

impl IDisposable for MockDomFocusTracker {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl IDomFocusTracker for MockDomFocusTracker {
    fn on_did_focus(&self) -> BaseEvent<()> {
        self.did_focus.event()
    }

    fn on_did_blur(&self) -> BaseEvent<()> {
        self.did_blur.event()
    }

    fn refresh_state(&self) {}
}

impl IUserInteractionService for MockUserInteractionService {
    fn read_modifier_key_status(&self, _element: &UiElement, _reader: Option<&dyn IReader>) -> ModifierKeyStatus {
        self.modifiers
    }

    fn create_focus_tracker(&self, _element: &UiElement, _store: &DisposableStore) -> IObservable<bool> {
        const_observable(self.simulate_focus)
    }

    fn create_hover_tracker(&self, _element: &UiElement, _store: &DisposableStore) -> IObservable<bool> {
        const_observable(self.simulate_hover)
    }

    fn create_dom_focus_tracker(&self, _element: &UiElement) -> Rc<dyn IDomFocusTracker> {
        let tracker = MockDomFocusTracker::new();
        if self.simulate_focus {
            let pending = tracker.clone();
            queue_microtask(move || pending.did_focus.fire(()));
        }
        tracker
    }
}
