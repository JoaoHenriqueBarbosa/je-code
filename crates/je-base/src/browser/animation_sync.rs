//! Porte de `src/vs/base/browser/animationSync.ts`: alinha a fase das animações CSS em laço e as pausa enquanto o
//! elemento está fora da área visível ou o documento está escondido.
//!
//! Divergências:
//! - **Web Animations e `IntersectionObserver`.** O navegador os oferece na janela; aqui o hospedeiro os instala em
//!   [`CodeWindow`] (`set_get_animations`, `set_intersection_observer_ctor`), no mesmo molde do `ResizeObserver`. Sem
//!   `getAnimations`, [`synchronize_css_animations`] não faz nada; sem `IntersectionObserver`,
//!   [`pause_css_animations_when_hidden`] cai no ramo do original que só acompanha o `document.hidden`.
//! - **Estado global.** O mapa de observadores por janela e o ouvinte de `onDidUnregisterWindow` do original são
//!   variáveis de módulo; aqui são `thread_local!`.
//! - **Opções.** `IPauseCSSAnimationsWhenHiddenOptions extends ISynchronizeAnimationsOptions` vira a composição:
//!   [`PauseCssAnimationsWhenHiddenOptions::synchronize`].
//! - **Chaves por elemento.** O `Map<HTMLElement, ...>` é uma lista com a ordem de inserção, comparada por identidade do
//!   [`Node`].

use std::cell::{Cell, RefCell};
use std::collections::BTreeSet;
use std::rc::Rc;

use crate::lifecycle::{DisposableRef, IDisposable, none, to_disposable};

use super::dom::{AnimationPlayState, CodeWindow, DomTarget, IntersectionObserverEntry, Node, ResizeObserverBackend, get_window, on_did_unregister_window};

/// `ISynchronizeAnimationsOptions`.
#[derive(Clone, Debug, Default)]
pub struct SynchronizeAnimationsOptions {
    /// Também sincroniza as animações dos descendentes (os pontos de um spinner, cujas animações vivem nos filhos).
    pub subtree: bool,
    /// Restringe a sincronização às animações CSS cujo `animation-name` está no conjunto. Animações que não são de
    /// keyframes (transições) são sempre ignoradas.
    pub animation_names: Option<BTreeSet<String>>,
}

/// Alinha a fase das animações CSS em execução e em laço, para que todas as de mesma duração mostrem o mesmo quadro ao
/// mesmo tempo, não importa quando cada uma começou.
///
/// Todas as animações CSS compartilham a linha do tempo do documento; ancorar o `startTime` de cada uma na mesma origem
/// (`0`) força o `currentTime` a igualar o tempo da linha do tempo. Os `animation-delay` de cada elemento se
/// preservam (fazem parte do tempo da própria animação), então as cascatas intencionais seguem valendo.
///
/// Diferente de mexer no `animation-delay`, isso reposiciona animações que já estão rodando. Chame quando uma animação
/// (re)começa ou volta de uma pausa fora da tela.
pub fn synchronize_css_animations(element: &Node, options: Option<&SynchronizeAnimationsOptions>) {
    let subtree = options.is_some_and(|options| options.subtree);
    let window = get_window(Some(DomTarget::Node(element)));
    let Some(animations) = window.get_animations(element, subtree) else {
        return; // Web Animations API not available; leave animations as-is.
    };
    for animation in animations {
        // Only CSS keyframe animations carry an `animationName`; skip transitions
        // and other Web Animations so this helper strictly aligns CSS animations.
        let Some(animation_name) = animation.animation_name() else { continue };
        if let Some(animation_names) = options.and_then(|options| options.animation_names.as_ref())
            && !animation_names.contains(&animation_name)
        {
            continue;
        }
        if animation.play_state() == AnimationPlayState::Paused {
            continue;
        }
        // Anchor to a shared origin so all animations of the same duration display
        // the same frame. Guard against the rare state where startTime is not yet
        // settable (e.g. an animation still in its pending/ready phase).
        let _ = animation.set_start_time(0.0);
    }
}

/// `IPauseCSSAnimationsWhenHiddenOptions`.
#[derive(Clone)]
pub struct PauseCssAnimationsWhenHiddenOptions {
    pub synchronize: SynchronizeAnimationsOptions,
    pub paused_class: String,
    /// Informa se o elemento está visível na área visível e no documento.
    pub on_did_change_visibility: Option<Rc<dyn Fn(bool)>>,
}

impl PauseCssAnimationsWhenHiddenOptions {
    pub fn new(paused_class: impl Into<String>) -> Self {
        Self { synchronize: SynchronizeAnimationsOptions::default(), paused_class: paused_class.into(), on_did_change_visibility: None }
    }
}

struct TrackedAnimation {
    options: PauseCssAnimationsWhenHiddenOptions,
    visible: Cell<bool>,
}

/// `IAnimationVisibilityObserver`.
struct VisibilityObserver {
    observer: RefCell<Option<Rc<dyn ResizeObserverBackend>>>,
    tracked_animations: RefCell<Vec<(Node, Rc<TrackedAnimation>)>>,
    intersecting_elements: RefCell<Vec<Node>>,
    visibility_listener: RefCell<Option<DisposableRef>>,
}

impl VisibilityObserver {
    fn tracked(&self, target: &Node) -> Option<Rc<TrackedAnimation>> {
        self.tracked_animations.borrow().iter().find(|(candidate, _)| candidate == target).map(|(_, tracked)| tracked.clone())
    }

    /// `trackedAnimations.set(element, tracked)`: um elemento já presente mantém a posição.
    fn set_tracked(&self, target: &Node, tracked: Rc<TrackedAnimation>) {
        let mut tracked_animations = self.tracked_animations.borrow_mut();
        match tracked_animations.iter_mut().find(|(candidate, _)| candidate == target) {
            Some(slot) => slot.1 = tracked,
            None => tracked_animations.push((target.clone(), tracked)),
        }
    }

    /// `observer.disconnect(); visibilityListener.dispose()`.
    fn disconnect(&self) {
        let observer = self.observer.borrow_mut().take();
        if let Some(observer) = observer {
            observer.disconnect();
        }
        let listener = self.visibility_listener.borrow_mut().take();
        if let Some(listener) = listener {
            listener.dispose();
        }
    }
}

thread_local! {
    static ANIMATION_VISIBILITY_OBSERVERS: RefCell<Vec<(Rc<CodeWindow>, Rc<VisibilityObserver>)>> = const { RefCell::new(Vec::new()) };
    static UNREGISTER_WINDOW_LISTENER: RefCell<Option<DisposableRef>> = const { RefCell::new(None) };
}

fn registered_observer(window: &Rc<CodeWindow>) -> Option<Rc<VisibilityObserver>> {
    ANIMATION_VISIBILITY_OBSERVERS.with(|observers| observers.borrow().iter().find(|(candidate, _)| Rc::ptr_eq(candidate, window)).map(|(_, state)| state.clone()))
}

fn unregister_observer(window: &Rc<CodeWindow>) {
    ANIMATION_VISIBILITY_OBSERVERS.with(|observers| observers.borrow_mut().retain(|(candidate, _)| !Rc::ptr_eq(candidate, window)));
}

/// O trabalho que o callback do observador e o `visibilitychange` adiam para depois de varrer os elementos.
#[derive(Default)]
struct PendingWork {
    to_resync: Vec<(Node, SynchronizeAnimationsOptions)>,
    visibility_changes: Vec<Box<dyn FnOnce()>>,
}

/// O corpo comum ao callback do `IntersectionObserver` e ao `visibilitychange`: alterna a classe de pausa e
/// agenda a ressincronização e o aviso de mudança de visibilidade de um elemento.
fn apply_visibility(state: &Rc<VisibilityObserver>, target: &Node, tracked: &Rc<TrackedAnimation>, paused: bool, pending: &mut PendingWork) {
    target.class_list_toggle(&tracked.options.paused_class, paused);
    if let Some(notify) = &tracked.options.on_did_change_visibility
        && tracked.visible.get() == paused
    {
        tracked.visible.set(!paused);
        let (state, tracked, target, notify) = (state.clone(), tracked.clone(), target.clone(), notify.clone());
        pending.visibility_changes.push(Box::new(move || {
            if state.tracked(&target).is_some_and(|current| Rc::ptr_eq(&current, &tracked)) {
                notify(!paused);
            }
        }));
    }
    if !paused {
        pending.to_resync.push((target.clone(), tracked.options.synchronize.clone()));
    }
}

fn settle(window: &Rc<CodeWindow>, pending: PendingWork) {
    for (target, options) in &pending.to_resync {
        synchronize_css_animations(target, Some(options));
    }
    for notify in pending.visibility_changes {
        notify();
    }
    dispose_visibility_observer_if_empty(window, registered_observer(window));
}

fn create_visibility_observer(window: &Rc<CodeWindow>, ctor: super::dom::IntersectionObserverCtor) -> Rc<VisibilityObserver> {
    let state = Rc::new(VisibilityObserver {
        observer: RefCell::new(None),
        tracked_animations: RefCell::new(Vec::new()),
        intersecting_elements: RefCell::new(Vec::new()),
        visibility_listener: RefCell::new(None),
    });

    let on_intersection: Rc<dyn Fn(&[IntersectionObserverEntry])> = {
        let (window, state) = (window.clone(), Rc::downgrade(&state));
        Rc::new(move |entries: &[IntersectionObserverEntry]| {
            let Some(state) = state.upgrade() else { return };
            let mut pending = PendingWork::default();
            for entry in entries {
                let target = &entry.target;
                let Some(tracked) = state.tracked(target) else { continue };
                {
                    let mut intersecting = state.intersecting_elements.borrow_mut();
                    intersecting.retain(|candidate| candidate != target);
                    if entry.is_intersecting {
                        intersecting.push(target.clone());
                    }
                }
                let paused = window.document().hidden() || !entry.is_intersecting;
                apply_visibility(&state, target, &tracked, paused, &mut pending);
            }
            settle(&window, pending);
        })
    };
    *state.observer.borrow_mut() = Some(ctor(on_intersection));

    let visibility_listener = {
        let (window, state) = (window.clone(), Rc::downgrade(&state));
        window.document().add_visibility_change_listener({
            let window = window.clone();
            move || {
                let Some(state) = state.upgrade() else { return };
                let document_hidden = window.document().hidden();
                let mut pending = PendingWork::default();
                let tracked_animations = state.tracked_animations.borrow().clone();
                for (target, tracked) in &tracked_animations {
                    let paused = document_hidden || !state.intersecting_elements.borrow().contains(target);
                    apply_visibility(&state, target, tracked, paused, &mut pending);
                }
                settle(&window, pending);
            }
        })
    };
    *state.visibility_listener.borrow_mut() = Some(visibility_listener);
    state
}

/// Pausa as animações CSS enquanto o elemento está fora da área visível ou o documento está escondido. O acompanhamento
/// sobrevive a uma desconexão temporária do DOM; descarte quando o elemento não for mais reutilizável.
pub fn pause_css_animations_when_hidden(element: &Node, options: PauseCssAnimationsWhenHiddenOptions) -> DisposableRef {
    let target_window = get_window(Some(DomTarget::Node(element)));
    let Some(ctor) = target_window.intersection_observer_ctor() else {
        let Some(on_did_change_visibility) = options.on_did_change_visibility.clone() else {
            return none();
        };
        let document = target_window.document();
        let update = {
            let document = document.clone();
            move || on_did_change_visibility(!document.hidden())
        };
        update();
        return document.add_visibility_change_listener(update);
    };

    let state = match registered_observer(&target_window) {
        Some(state) => state,
        None => {
            let state = create_visibility_observer(&target_window, ctor);
            ANIMATION_VISIBILITY_OBSERVERS.with(|observers| observers.borrow_mut().push((target_window.clone(), state.clone())));

            if UNREGISTER_WINDOW_LISTENER.with(|listener| listener.borrow().is_none()) {
                let listener = on_did_unregister_window(|window| {
                    let Some(state) = registered_observer(window) else { return };
                    state.disconnect();
                    let tracked_animations = state.tracked_animations.borrow().clone();
                    for (_, tracked) in tracked_animations {
                        if tracked.visible.get()
                            && let Some(notify) = &tracked.options.on_did_change_visibility
                        {
                            notify(false);
                        }
                    }
                    unregister_observer(window);
                    dispose_unregister_window_listener_if_unused();
                });
                UNREGISTER_WINDOW_LISTENER.with(|slot| *slot.borrow_mut() = Some(listener));
            }
            state
        }
    };

    element.class_list_add(&options.paused_class);
    let on_did_change_visibility = options.on_did_change_visibility.clone();
    let paused_class = options.paused_class.clone();
    state.set_tracked(element, Rc::new(TrackedAnimation { options, visible: Cell::new(false) }));
    if let Some(notify) = on_did_change_visibility {
        notify(false);
    }
    let observer = state.observer.borrow().clone();
    if let Some(observer) = observer {
        observer.observe(element);
    }

    let element = element.clone();
    to_disposable(move || {
        let observer = state.observer.borrow().clone();
        if let Some(observer) = observer {
            observer.unobserve(&element);
        }
        state.tracked_animations.borrow_mut().retain(|(candidate, _)| *candidate != element);
        state.intersecting_elements.borrow_mut().retain(|candidate| *candidate != element);
        element.class_list_remove(&paused_class);
        dispose_visibility_observer_if_empty(&target_window, Some(state));
    })
}

fn dispose_visibility_observer_if_empty(target_window: &Rc<CodeWindow>, state: Option<Rc<VisibilityObserver>>) {
    let Some(state) = state else { return };
    if !state.tracked_animations.borrow().is_empty() || !registered_observer(target_window).is_some_and(|current| Rc::ptr_eq(&current, &state)) {
        return;
    }
    state.disconnect();
    unregister_observer(target_window);
    dispose_unregister_window_listener_if_unused();
}

fn dispose_unregister_window_listener_if_unused() {
    if ANIMATION_VISIBILITY_OBSERVERS.with(|observers| observers.borrow().is_empty()) {
        let listener = UNREGISTER_WINDOW_LISTENER.with(|slot| slot.borrow_mut().take());
        if let Some(listener) = listener {
            listener.dispose();
        }
    }
}
