//! Porte de `src/vs/workbench/contrib/onboarding/browser/spotlight/onboardingTarget.ts`: a marcação de elementos
//! como alvos de onboarding, os provedores de alvo que um dono registra e a resolução do alvo de um passo.
//!
//! Divergências (sem DOM, recorte declarado):
//! - **`HTMLElement`, `Window` e `document`.** Não há DOM no porte. [`OnboardingElement`] é o modelo mínimo do que o
//!   original lê e escreve num elemento: atributos, classes, `isConnected`, `ownerDocument`, `display`, `visibility`,
//!   a caixa (`getBoundingClientRect().width/height`) e o foco (`focus()` e `document.activeElement`).
//!   [`OnboardingWindow`] é a janela com o documento dela ([`OnboardingDocument`]), que guarda a ordem de inserção
//!   dos elementos conectados (a ordem de documento do `querySelectorAll`). A identidade (`===`) é `Rc::ptr_eq`.
//! - **`WeakMap<HTMLElement, registration>`.** O registro mora no estado do próprio elemento, com a mesma vida.
//! - **`querySelectorAll('[data-onboarding-id="..."]')` e `CSS.escape`.** A comparação é de igualdade do valor do
//!   atributo, então o escape não tem papel.
//! - **`Promise<void> | void`** do `open` é `Option<Promise<()>>`.
//! - **`scope?: string | (() => string | undefined)`** é [`OnboardingTargetScope`].
//! - O `Map` de provedores é por thread (`thread_local!`), como o estado de módulo do original é por realm.
//! - O `import '../media/onboardingTarget.css'` não tem equivalente: o CSS é especificação, não código executado.
//! - **Eventos, quadros e posição (acréscimo do spotlight).** O overlay do spotlight lê a posição do elemento
//!   (`getBoundingClientRect().left/top`), o `tabIndex`, escuta `click`, `keydown` e `keyup` no elemento e `keydown`
//!   na janela (fase de captura), e agenda trabalho no próximo quadro de animação. O modelo tem a posição
//!   ([`OnboardingElement::set_position`]), o `tabIndex`, o despacho de eventos sem árvore (o evento passa pelos
//!   ouvintes da janela em captura, depois pelos do próprio elemento, captura antes de bolha, com `preventDefault`,
//!   `stopPropagation` e `stopImmediatePropagation`) e um agendador de quadros por janela de 16 ms
//!   ([`OnboardingWindow::schedule_at_next_animation_frame`]). A viewport (`documentElement.clientWidth/Height`) é
//!   [`OnboardingWindow::set_viewport`], com 1024 por 768 de padrão.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::rc::{Rc, Weak};

use je_base::errors::{self, Error};
use je_base::event::Event;
use je_base::lifecycle::{DisposableRef, IDisposable, plain_disposable, to_disposable};
use je_base::promise::Promise;
use je_base::runtime;

/// Atributo que marca um elemento como alvo de spotlight de onboarding.
///
/// Os componentes aderem marcando o PRÓPRIO elemento com o atributo. Os tours referenciam o id; nunca entram no
/// markup ou nas classes de outro componente para achar um alvo.
pub const ONBOARDING_TARGET_ATTR: &str = "data-onboarding-id";

pub const ONBOARDING_TARGET_PULSE_CLASS: &str = "onboarding-target-pulse";

/// O tipo de evento despachado num [`OnboardingElement`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnboardingEventKind {
    Click,
    KeyDown,
    KeyUp,
}

/// A tecla de um evento de teclado (as que o onboarding distingue).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnboardingKey {
    Enter,
    Escape,
    Tab,
    Space,
    Other,
}

/// O evento entregue aos ouvintes.
pub struct OnboardingEvent {
    pub kind: OnboardingEventKind,
    pub key: OnboardingKey,
    pub shift_key: bool,
    /// `event.target`.
    pub target: OnboardingElement,
    default_prevented: Cell<bool>,
    propagation_stopped: Cell<bool>,
    immediate_stopped: Cell<bool>,
}

impl OnboardingEvent {
    /// Um evento novo, ainda sem nenhum `preventDefault` ou `stopPropagation`.
    pub fn new(kind: OnboardingEventKind, key: OnboardingKey, shift_key: bool, target: OnboardingElement) -> Self {
        Self {
            kind,
            key,
            shift_key,
            target,
            default_prevented: Cell::new(false),
            propagation_stopped: Cell::new(false),
            immediate_stopped: Cell::new(false),
        }
    }

    pub fn prevent_default(&self) {
        self.default_prevented.set(true);
    }

    pub fn stop_propagation(&self) {
        self.propagation_stopped.set(true);
    }

    pub fn stop_immediate_propagation(&self) {
        self.propagation_stopped.set(true);
        self.immediate_stopped.set(true);
    }

    pub fn default_prevented(&self) -> bool {
        self.default_prevented.get()
    }
}

/// Um ouvinte de evento.
pub type OnboardingEventListener = Rc<dyn Fn(&OnboardingEvent)>;

/// Intervalo entre quadros de animação (60 Hz).
const ANIMATION_FRAME_INTERVAL_MS: u64 = 16;
/// `documentElement.clientWidth` e `clientHeight` padrão.
const DEFAULT_VIEWPORT: (f64, f64) = (1024.0, 768.0);

/// O documento de uma [`OnboardingWindow`].
#[derive(Default)]
pub struct OnboardingDocument {
    /// Os elementos conectados, na ordem de inserção.
    connected: RefCell<Vec<OnboardingElement>>,
    active_element: RefCell<Option<OnboardingElement>>,
    viewport: Cell<Option<(f64, f64)>>,
    next_id: Cell<u64>,
    frames: RefCell<Vec<(u64, Box<dyn FnOnce()>)>>,
    frame_armed: Cell<bool>,
    key_capture_listeners: RefCell<Vec<(u64, OnboardingEventListener)>>,
}

impl OnboardingDocument {
    /// `document.activeElement` (sem foco é `None`; o `body` do navegador não é modelado).
    pub fn active_element(&self) -> Option<OnboardingElement> {
        self.active_element.borrow().clone()
    }

    fn next_id(&self) -> u64 {
        let id = self.next_id.get() + 1;
        self.next_id.set(id);
        id
    }

    /// Arma o timer do próximo quadro, se houver trabalho e ele ainda não estiver armado.
    fn arm_frame(document: &Rc<Self>) {
        if document.frame_armed.get() || document.frames.borrow().is_empty() {
            return;
        }
        document.frame_armed.set(true);
        let document = document.clone();
        runtime::set_timeout(
            {
                let document = document.clone();
                move || {
                    document.frame_armed.set(false);
                    let ids: Vec<u64> = document.frames.borrow().iter().map(|(id, _)| *id).collect();
                    for id in ids {
                        // Um quadro descartado por um quadro anterior do mesmo ciclo já não está na fila.
                        let position = document.frames.borrow().iter().position(|(frame, _)| *frame == id);
                        let entry = position.map(|position| document.frames.borrow_mut().remove(position));
                        if let Some((_, callback)) = entry {
                            callback();
                        }
                    }
                    Self::arm_frame(&document);
                }
            },
            ANIMATION_FRAME_INTERVAL_MS,
        );
    }
}

/// A `Window` do original: a identidade dela é a do documento.
#[derive(Clone, Default)]
pub struct OnboardingWindow {
    document: Rc<OnboardingDocument>,
}

impl OnboardingWindow {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn document(&self) -> &OnboardingDocument {
        &self.document
    }

    /// `documentElement.clientWidth` e `clientHeight`.
    pub fn viewport(&self) -> (f64, f64) {
        self.document.viewport.get().unwrap_or(DEFAULT_VIEWPORT)
    }

    pub fn set_viewport(&self, width: f64, height: f64) {
        self.document.viewport.set(Some((width, height)));
    }

    /// `scheduleAtNextAnimationFrame(targetWindow, callback)`: os quadros do mesmo ciclo rodam na ordem em que foram
    /// pedidos, e o que for pedido durante o ciclo roda no ciclo seguinte. O handle é o objeto literal `{ dispose }` do
    /// original, que ninguém rastreia (o `animate` troca um por quadro e não descarta os que já dispararam).
    pub fn schedule_at_next_animation_frame(&self, callback: impl FnOnce() + 'static) -> DisposableRef {
        let id = self.document.next_id();
        self.document.frames.borrow_mut().push((id, Box::new(callback)));
        OnboardingDocument::arm_frame(&self.document);
        let document = self.document.clone();
        plain_disposable(move || document.frames.borrow_mut().retain(|(frame, _)| *frame != id))
    }

    /// `requestAnimationFrame`: o mesmo agendador, sem handle para cancelar.
    pub fn request_animation_frame(&self, callback: impl FnOnce() + 'static) {
        let id = self.document.next_id();
        self.document.frames.borrow_mut().push((id, Box::new(callback)));
        OnboardingDocument::arm_frame(&self.document);
    }

    /// `animate(targetWindow, fn)`: roda `callback` a cada quadro até o disposable devolvido ser descartado. Divergência:
    /// o original, se descartado de dentro do próprio `fn`, agenda o quadro seguinte mesmo assim (um laço zumbi que
    /// nunca termina); aqui o descarte encerra o laço.
    pub fn animate(&self, callback: impl Fn() + 'static) -> DisposableRef {
        let state = Rc::new(AnimateState { disposed: Cell::new(false), step: RefCell::new(None), callback: Rc::new(callback) });
        Self::animate_step(self.clone(), state.clone());
        to_disposable(move || {
            state.disposed.set(true);
            let step = state.step.borrow_mut().take();
            if let Some(step) = step {
                step.dispose();
            }
        })
    }

    fn animate_step(window: OnboardingWindow, state: Rc<AnimateState>) {
        let step = window.schedule_at_next_animation_frame({
            let (window, state) = (window.clone(), state.clone());
            move || {
                (state.callback)();
                if !state.disposed.get() {
                    Self::animate_step(window, state);
                }
            }
        });
        *state.step.borrow_mut() = Some(step);
    }

    /// `addDisposableListener(window, KEY_DOWN, listener, true)`: um ouvinte de teclado da janela, na fase de captura.
    pub fn add_key_capture_listener(&self, listener: impl Fn(&OnboardingEvent) + 'static) -> DisposableRef {
        let id = self.document.next_id();
        self.document.key_capture_listeners.borrow_mut().push((id, Rc::new(listener)));
        let document = self.document.clone();
        to_disposable(move || document.key_capture_listeners.borrow_mut().retain(|(listener, _)| *listener != id))
    }
}

struct AnimateState {
    disposed: Cell<bool>,
    step: RefCell<Option<DisposableRef>>,
    callback: Rc<dyn Fn()>,
}

impl PartialEq for OnboardingWindow {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.document, &other.document)
    }
}

impl Eq for OnboardingWindow {}

impl Hash for OnboardingWindow {
    fn hash<H: Hasher>(&self, state: &mut H) {
        Rc::as_ptr(&self.document).cast::<()>().hash(state);
    }
}

struct ElementState {
    owner: Weak<OnboardingDocument>,
    attributes: RefCell<Vec<(String, String)>>,
    classes: RefCell<Vec<String>>,
    connected: Cell<bool>,
    display_none: Cell<bool>,
    visibility_hidden: Cell<bool>,
    width: Cell<f64>,
    height: Cell<f64>,
    left: Cell<f64>,
    top: Cell<f64>,
    tab_index: Cell<i32>,
    listeners: RefCell<Vec<ListenerEntry>>,
    registration: RefCell<Option<Rc<OnboardingTargetRegistration>>>,
}

struct ListenerEntry {
    id: u64,
    kind: OnboardingEventKind,
    capture: bool,
    listener: OnboardingEventListener,
}

/// Um `HTMLElement` reduzido ao que o onboarding usa. Clonar compartilha o mesmo elemento.
#[derive(Clone)]
pub struct OnboardingElement(Rc<ElementState>);

impl PartialEq for OnboardingElement {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for OnboardingElement {}

impl fmt::Debug for OnboardingElement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "OnboardingElement({:?})", self.0.attributes.borrow())
    }
}

impl OnboardingElement {
    /// `$('button')`: um elemento solto, ainda fora do documento, dono é o documento da janela.
    pub fn new(window: &OnboardingWindow) -> Self {
        Self(Rc::new(ElementState {
            owner: Rc::downgrade(&window.document),
            attributes: RefCell::new(Vec::new()),
            classes: RefCell::new(Vec::new()),
            connected: Cell::new(false),
            display_none: Cell::new(false),
            visibility_hidden: Cell::new(false),
            width: Cell::new(0.0),
            height: Cell::new(0.0),
            left: Cell::new(0.0),
            top: Cell::new(0.0),
            tab_index: Cell::new(0),
            listeners: RefCell::new(Vec::new()),
            registration: RefCell::new(None),
        }))
    }

    /// A caixa do elemento (o `width` e o `height` de `getBoundingClientRect()`).
    pub fn set_size(&self, width: f64, height: f64) {
        self.0.width.set(width);
        self.0.height.set(height);
    }

    /// `style.left` e `style.top` (posição `fixed`): a origem de `getBoundingClientRect()`.
    pub fn set_position(&self, left: f64, top: f64) {
        self.0.left.set(left);
        self.0.top.set(top);
    }

    /// `getBoundingClientRect()`: `(left, top, width, height)`. Um elemento desconectado ou com `display: none` não
    /// tem caixa (zeros), como no navegador.
    pub fn bounding_rect(&self) -> (f64, f64, f64, f64) {
        if !self.0.connected.get() || self.0.display_none.get() {
            return (0.0, 0.0, 0.0, 0.0);
        }
        (self.0.left.get(), self.0.top.get(), self.0.width.get(), self.0.height.get())
    }

    /// `element.tabIndex`.
    pub fn tab_index(&self) -> i32 {
        self.0.tab_index.get()
    }

    pub fn set_tab_index(&self, tab_index: i32) {
        self.0.tab_index.set(tab_index);
    }

    /// Se `style.display` é `'none'`.
    pub fn is_display_none(&self) -> bool {
        self.0.display_none.get()
    }

    /// Se `style.visibility` é `'hidden'`.
    pub fn is_visibility_hidden(&self) -> bool {
        self.0.visibility_hidden.get()
    }

    /// `addDisposableListener(element, type, listener, capture)`.
    pub fn add_event_listener(&self, kind: OnboardingEventKind, capture: bool, listener: impl Fn(&OnboardingEvent) + 'static) -> DisposableRef {
        let id = self.0.owner.upgrade().map_or(0, |document| document.next_id());
        self.0.listeners.borrow_mut().push(ListenerEntry { id, kind, capture, listener: Rc::new(listener) });
        let element = self.clone();
        to_disposable(move || element.0.listeners.borrow_mut().retain(|entry| entry.id != id))
    }

    /// `element.click()`. Devolve `false` se algum ouvinte chamou `preventDefault`.
    pub fn click(&self) -> bool {
        self.dispatch(OnboardingEventKind::Click, OnboardingKey::Other, false)
    }

    /// `dispatchEvent(new KeyboardEvent('keydown', ...))`. Devolve `false` se algum ouvinte chamou `preventDefault`.
    pub fn key_down(&self, key: OnboardingKey, shift_key: bool) -> bool {
        self.dispatch(OnboardingEventKind::KeyDown, key, shift_key)
    }

    /// `dispatchEvent(new KeyboardEvent('keyup', ...))`.
    pub fn key_up(&self, key: OnboardingKey, shift_key: bool) -> bool {
        self.dispatch(OnboardingEventKind::KeyUp, key, shift_key)
    }

    fn dispatch(&self, kind: OnboardingEventKind, key: OnboardingKey, shift_key: bool) -> bool {
        let event = OnboardingEvent::new(kind, key, shift_key, self.clone());
        if kind != OnboardingEventKind::Click
            && let Some(document) = self.0.owner.upgrade()
        {
            let snapshot: Vec<(u64, OnboardingEventListener)> = document.key_capture_listeners.borrow().clone();
            for (id, listener) in snapshot {
                let registered = document.key_capture_listeners.borrow().iter().any(|(current, _)| *current == id);
                if registered && !event.immediate_stopped.get() {
                    listener(&event);
                }
            }
            if event.propagation_stopped.get() {
                return !event.default_prevented();
            }
        }
        // No alvo do evento, os ouvintes de captura rodam antes dos de bolha.
        for capture in [true, false] {
            let snapshot: Vec<(u64, OnboardingEventListener)> = self
                .0
                .listeners
                .borrow()
                .iter()
                .filter(|entry| entry.kind == kind && entry.capture == capture)
                .map(|entry| (entry.id, entry.listener.clone()))
                .collect();
            for (id, listener) in snapshot {
                let registered = self.0.listeners.borrow().iter().any(|entry| entry.id == id);
                if registered && !event.immediate_stopped.get() {
                    listener(&event);
                }
            }
        }
        !event.default_prevented()
    }

    /// `style.display = 'none'` (verdadeiro) ou `style.display = ''` (falso).
    pub fn set_display_none(&self, hidden: bool) {
        self.0.display_none.set(hidden);
    }

    /// `style.visibility = 'hidden'` (verdadeiro) ou `''` (falso).
    pub fn set_visibility_hidden(&self, hidden: bool) {
        self.0.visibility_hidden.set(hidden);
    }

    /// `document.body.appendChild(element)`: conecta o elemento no fim do documento dele.
    pub fn append_to_body(&self) {
        let Some(document) = self.0.owner.upgrade() else {
            return;
        };
        document.connected.borrow_mut().retain(|element| element != self);
        document.connected.borrow_mut().push(self.clone());
        self.0.connected.set(true);
    }

    /// `element.remove()`: desconecta o elemento e tira o foco dele.
    pub fn remove(&self) {
        if let Some(document) = self.0.owner.upgrade() {
            document.connected.borrow_mut().retain(|element| element != self);
            let focused = document.active_element.borrow().as_ref() == Some(self);
            if focused {
                *document.active_element.borrow_mut() = None;
            }
        }
        self.0.connected.set(false);
    }

    pub fn is_connected(&self) -> bool {
        self.0.connected.get()
    }

    /// `element.focus()`: só um elemento conectado recebe o foco.
    pub fn focus(&self) {
        if !self.0.connected.get() {
            return;
        }
        if let Some(document) = self.0.owner.upgrade() {
            *document.active_element.borrow_mut() = Some(self.clone());
        }
    }

    pub fn has_attribute(&self, name: &str) -> bool {
        self.get_attribute(name).is_some()
    }

    pub fn get_attribute(&self, name: &str) -> Option<String> {
        self.0.attributes.borrow().iter().find(|(key, _)| key == name).map(|(_, value)| value.clone())
    }

    fn set_attribute(&self, name: &str, value: &str) {
        let mut attributes = self.0.attributes.borrow_mut();
        match attributes.iter_mut().find(|(key, _)| key == name) {
            Some((_, current)) => *current = value.to_string(),
            None => attributes.push((name.to_string(), value.to_string())),
        }
    }

    fn remove_attribute(&self, name: &str) {
        self.0.attributes.borrow_mut().retain(|(key, _)| key != name);
    }

    /// `element.classList.contains(name)`.
    pub fn has_class(&self, name: &str) -> bool {
        self.0.classes.borrow().iter().any(|class| class == name)
    }

    fn add_class(&self, name: &str) {
        if !self.has_class(name) {
            self.0.classes.borrow_mut().push(name.to_string());
        }
    }

    fn remove_class(&self, name: &str) {
        self.0.classes.borrow_mut().retain(|class| class != name);
    }
}

/// `IOnboardingTarget.open` e `IOnboardingTargetOptions.open`: abre ou expande o alvo.
pub type OnboardingTargetOpen = Rc<dyn Fn() -> Option<Promise<()>>>;

/// `IOnboardingFocusTarget`: o dono do foco de teclado quando o elemento destacado não é focável.
#[derive(Clone)]
pub struct OnboardingFocusTarget {
    pub element: OnboardingElement,
    pub focus: Rc<dyn Fn()>,
}

impl PartialEq for OnboardingFocusTarget {
    fn eq(&self, other: &Self) -> bool {
        self.element == other.element && Rc::ptr_eq(&self.focus, &other.focus)
    }
}

/// `scope?: string | (() => string | undefined)`.
#[derive(Clone)]
pub enum OnboardingTargetScope {
    Fixed(String),
    Dynamic(Rc<dyn Fn() -> Option<String>>),
}

/// `IOnboardingTargetOptions`.
#[derive(Clone, Default)]
pub struct OnboardingTargetOptions {
    /// Abre ou expande o alvo antes de o passo de spotlight aparecer.
    pub open: Option<OnboardingTargetOpen>,
    pub focus_target: Option<OnboardingFocusTarget>,

    /// Se o controle já tem um valor selecionado, independente da aceitação assíncrona.
    pub has_selection: Option<Rc<dyn Fn() -> bool>>,

    /// Reporta uma seleção do usuário e resolve quando ela é aceita ou rejeitada.
    pub on_did_select: Option<Event<Promise<bool>>>,
    /// Reporta ativação por mouse, teclado ou toque pelo controle dono.
    pub on_did_activate: Option<Event<()>>,
    /// Identifica a instância de UI preparada que é dona deste alvo.
    pub scope: Option<OnboardingTargetScope>,
}

/// `IOnboardingTarget`.
#[derive(Clone)]
pub struct OnboardingTarget {
    pub element: OnboardingElement,
    pub open: Option<OnboardingTargetOpen>,
    pub focus_target: Option<OnboardingFocusTarget>,
    /// Seleções aceitas fornecidas por um adaptador com escopo, sem trocar o marcador DOM do dono.
    pub on_did_select: Option<Event<Promise<bool>>>,
    pub on_did_activate: Option<Event<()>>,
}

impl OnboardingTarget {
    /// Um alvo só com o elemento, os demais membros ausentes.
    pub fn new(element: OnboardingElement) -> Self {
        Self { element, open: None, focus_target: None, on_did_select: None, on_did_activate: None }
    }
}

struct OnboardingTargetRegistration {
    #[allow(dead_code)]
    id: String,
    options: OnboardingTargetOptions,
}

/// O resolvedor de um provedor: recebe o escopo pedido.
pub type OnboardingTargetResolve = Rc<dyn Fn(Option<&str>) -> Option<OnboardingTarget>>;

struct ProviderRegistration {
    resolve: OnboardingTargetResolve,
}

thread_local! {
    static ONBOARDING_TARGET_PROVIDERS: RefCell<HashMap<String, Rc<ProviderRegistration>>> = RefCell::new(HashMap::new());
}

fn current_provider(id: &str) -> Option<Rc<ProviderRegistration>> {
    ONBOARDING_TARGET_PROVIDERS.with(|providers| providers.borrow().get(id).cloned())
}

/// Registra um adaptador que resolve um controle pela API da feature dona dele. Lança se já houver um provedor
/// para o `id`.
pub fn register_onboarding_target_provider(
    id: &str,
    provider: impl Fn(Option<&str>) -> Option<OnboardingTarget> + 'static,
) -> DisposableRef {
    let registration = Rc::new(ProviderRegistration { resolve: Rc::new(provider) });
    let already_registered = ONBOARDING_TARGET_PROVIDERS.with(|providers| {
        let mut providers = providers.borrow_mut();
        if providers.contains_key(id) {
            return true;
        }
        providers.insert(id.to_string(), registration.clone());
        false
    });
    if already_registered {
        errors::throw(Error::new(format!("An onboarding target provider for '{id}' is already registered.")));
    }
    let id = id.to_string();
    to_disposable(move || {
        ONBOARDING_TARGET_PROVIDERS.with(|providers| {
            let mut providers = providers.borrow_mut();
            if providers.get(&id).is_some_and(|current| Rc::ptr_eq(current, &registration)) {
                providers.remove(&id);
            }
        });
    })
}

pub fn resolve_onboarding_target(target_window: &OnboardingWindow, id: &str, scope: Option<&str>) -> Option<OnboardingTarget> {
    if let Some(provider) = current_provider(id) {
        let target = (provider.resolve)(scope)?;
        if !is_visible_onboarding_target(target_window, &target.element) {
            return None;
        }
        let (window, id, scope, element) = (target_window.clone(), id.to_string(), scope.map(str::to_string), target.element.clone());
        let open_provider = provider.clone();
        let open: OnboardingTargetOpen = Rc::new(move || {
            let still_current = current_provider(&id).is_some_and(|current| Rc::ptr_eq(&current, &open_provider));
            if !still_current {
                return None;
            }
            let current = (open_provider.resolve)(scope.as_deref())?;
            if current.element == element && is_visible_onboarding_target(&window, &current.element) {
                return current.open.as_ref().and_then(|open| open());
            }
            None
        });
        return Some(OnboardingTarget {
            element: target.element,
            open: Some(open),
            focus_target: target.focus_target,
            on_did_select: target.on_did_select,
            on_did_activate: target.on_did_activate,
        });
    }
    let element = find_onboarding_target(target_window, id, scope)?;
    let options = registration_options(&element);
    let open_element = element.clone();
    Some(OnboardingTarget {
        element,
        open: Some(Rc::new(move || open_onboarding_target(&open_element))),
        focus_target: options.as_ref().and_then(|options| options.focus_target.clone()),
        on_did_select: None,
        on_did_activate: options.and_then(|options| options.on_did_activate),
    })
}

fn registration_options(element: &OnboardingElement) -> Option<OnboardingTargetOptions> {
    element.0.registration.borrow().as_ref().map(|registration| registration.options.clone())
}

/// Marca `element` como o alvo de onboarding identificado por `id`.
///
/// Devolve um disposable que tira o atributo de novo.
pub fn mark_onboarding_target(element: &OnboardingElement, id: &str, options: OnboardingTargetOptions) -> DisposableRef {
    let registration = Rc::new(OnboardingTargetRegistration { id: id.to_string(), options });
    element.set_attribute(ONBOARDING_TARGET_ATTR, id);
    *element.0.registration.borrow_mut() = Some(registration.clone());
    let element = element.clone();
    to_disposable(move || {
        let is_current = element.0.registration.borrow().as_ref().is_some_and(|current| Rc::ptr_eq(current, &registration));
        if is_current {
            *element.0.registration.borrow_mut() = None;
            element.remove_attribute(ONBOARDING_TARGET_ATTR);
        }
    })
}

/// Abre ou expande um alvo pelo comportamento registrado pelo dono dele.
pub fn open_onboarding_target(element: &OnboardingElement) -> Option<Promise<()>> {
    registration_options(element).and_then(|options| options.open).and_then(|open| open())
}

/// Observa as seleções reportadas pelo dono do alvo.
pub fn on_did_select_onboarding_target(element: &OnboardingElement) -> Event<Promise<bool>> {
    registration_options(element).and_then(|options| options.on_did_select).unwrap_or_else(Event::none)
}

/// Lê o estado de seleção do dono do alvo, sem depender de estado de sessão a jusante.
pub fn has_onboarding_target_selection(element: &OnboardingElement) -> bool {
    registration_options(element).and_then(|options| options.has_selection).is_some_and(|has_selection| has_selection())
}

/// Aplica o tratamento padrão de pulso de onboarding em `element`.
///
/// Devolve um disposable que tira o pulso de novo.
pub fn pulse_onboarding_target(element: &OnboardingElement) -> DisposableRef {
    element.add_class(ONBOARDING_TARGET_PULSE_CLASS);
    let element = element.clone();
    to_disposable(move || element.remove_class(ONBOARDING_TARGET_PULSE_CLASS))
}

/// Resolve o elemento marcado com o id de alvo dado, dentro do documento da janela. Devolve `None` se não houver
/// (por exemplo, a feature não está renderizada agora).
///
/// É o ÚNICO lugar em que o onboarding consulta o DOM, e casa só pelo atributo de onboarding, nunca por classes
/// ou estrutura alheias.
pub fn find_onboarding_target(target_window: &OnboardingWindow, id: &str, scope: Option<&str>) -> Option<OnboardingElement> {
    let targets: Vec<OnboardingElement> = target_window.document.connected.borrow().clone();
    targets.into_iter().find(|target| {
        target.get_attribute(ONBOARDING_TARGET_ATTR).as_deref() == Some(id)
            && is_visible_onboarding_target(target_window, target)
            && matches_scope(target, scope)
    })
}

fn matches_scope(target: &OnboardingElement, scope: Option<&str>) -> bool {
    let registered = registration_options(target).and_then(|options| options.scope).and_then(|registered| match registered {
        OnboardingTargetScope::Fixed(scope) => Some(scope),
        OnboardingTargetScope::Dynamic(scope) => scope(),
    });
    registered.as_deref() == scope
}

fn is_visible_onboarding_target(target_window: &OnboardingWindow, target: &OnboardingElement) -> bool {
    let same_document = target.0.owner.upgrade().is_some_and(|owner| Rc::ptr_eq(&owner, &target_window.document));
    if !target.is_connected() || !same_document {
        return false;
    }
    if target.0.display_none.get() || target.0.visibility_hidden.get() {
        return false;
    }
    target.0.width.get() > 0.0 && target.0.height.get() > 0.0
}
