//! Porte de `src/vs/base/test/browser/animationSync.test.ts`.
//!
//! O original roda num navegador de verdade (`IntersectionObserver`, `getAnimations`, layout e CSS). A renderização
//! nativa não tem nenhum dos três, então a produção (`je_base::browser::animation_sync`) fala com a janela por duas portas
//! (`CodeWindow::set_intersection_observer_ctor` e `CodeWindow::set_get_animations`) e este arquivo instala nelas um
//! navegador falso, [`FakeBrowser`], que reproduz só o contrato que os casos exercitam.
//!
//! Notas de tradução:
//! - `await waitForAnimationFrames(n)` é [`FakeBrowser::wait_for_animation_frames`]: cada quadro pede um
//!   `request_animation_frame`, entrega-o com `render_frame` e, como o navegador faz depois dos callbacks de quadro,
//!   entrega as observações de interseção.
//! - `IntersectionObserver` falso: a primeira observação sempre reporta (`isIntersecting` falso num elemento
//!   desconectado), e as seguintes só reportam mudança. Um elemento intersecta quando está conectado ao `body`
//!   (`Node::is_connected`) e o `margin-top` dele não o joga para fora da altura do pai com `overflow: hidden`
//!   (o layout do caso 4).
//! - `getAnimations` falso: as folhas de estilo dos casos viram regras em Rust. Uma animação existe quando o elemento
//!   tem `animation` inline ou a classe `animation-sync-running-test`; está pausada com o token `paused` no `animation`
//!   inline ou com a classe `paused` ao lado da regra. Como no navegador, a pausada tem `startTime` nulo, e a que volta a
//!   rodar ganha um `startTime` da linha do tempo (aqui `TIMELINE_NOW`) até alguém ancorá-lo em `0`.
//! - Os `<style>` e o `host.style.cssText` são as regras acima e o atributo `style` do nó.

mod common;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::browser::animation_sync::{PauseCssAnimationsWhenHiddenOptions, pause_css_animations_when_hidden, synchronize_css_animations};
use je_base::browser::dom::{AnimationPlayState, CodeWindow, CssAnimation, Document, IntersectionObserverEntry, Node, ResizeObserverBackend, main_window};
use je_base::errors::Error;
use je_base::lifecycle::to_disposable;

/// O `startTime` que o navegador dá a uma animação que acabou de rodar, antes de alguém ancorá-lo.
const TIMELINE_NOW: f64 = 1234.5;

const RUNNING_TEST_CLASS: &str = "animation-sync-running-test";

fn document() -> Rc<Document> {
    main_window().document()
}

/// O nome da animação CSS do elemento, se houver.
fn animation_name(node: &Node) -> Option<String> {
    let inline = node.style_get_property_value("animation");
    match inline.split_whitespace().next() {
        Some(name) => Some(name.to_owned()),
        None => node.class_list_contains(RUNNING_TEST_CLASS).then(|| RUNNING_TEST_CLASS.to_owned()),
    }
}

fn is_paused(node: &Node) -> bool {
    let inline = node.style_get_property_value("animation");
    inline.split_whitespace().any(|token| token == "paused") || (inline.is_empty() && node.class_list_contains(RUNNING_TEST_CLASS) && node.class_list_contains("paused"))
}

fn pixels(value: &str) -> Option<f64> {
    value.trim().strip_suffix("px")?.parse().ok()
}

/// O elemento cai fora do pai que recorta (`overflow: hidden`) por causa do `margin-top`.
fn is_clipped(node: &Node) -> bool {
    let Some(parent) = node.parent_node() else { return false };
    if parent.style_get_property_value("overflow") != "hidden" {
        return false;
    }
    match (pixels(&node.style_get_property_value("margin-top")), pixels(&parent.style_get_property_value("height"))) {
        (Some(top), Some(height)) => top >= height,
        _ => false,
    }
}

fn is_intersecting(node: &Node) -> bool {
    node.is_connected() && !is_clipped(node)
}

fn descendant_elements(node: &Node, out: &mut Vec<Node>) {
    for child in node.child_nodes().into_iter().filter(Node::is_html_element) {
        out.push(child.clone());
        descendant_elements(&child, out);
    }
}

struct FakeAnimation {
    node: Node,
    start_time: Cell<Option<f64>>,
}

impl FakeAnimation {
    /// Pausada, não tem `startTime`; ao voltar a rodar, ganha o da linha do tempo.
    fn settle(&self) {
        if is_paused(&self.node) {
            self.start_time.set(None);
        } else if self.start_time.get().is_none() {
            self.start_time.set(Some(TIMELINE_NOW));
        }
    }
}

impl CssAnimation for FakeAnimation {
    fn animation_name(&self) -> Option<String> {
        animation_name(&self.node)
    }

    fn play_state(&self) -> AnimationPlayState {
        self.settle();
        if is_paused(&self.node) { AnimationPlayState::Paused } else { AnimationPlayState::Running }
    }

    fn start_time(&self) -> Option<f64> {
        self.settle();
        self.start_time.get()
    }

    fn set_start_time(&self, time: f64) -> Result<(), Error> {
        self.start_time.set(Some(time));
        Ok(())
    }
}

struct FakeIntersectionObserver {
    callback: Rc<dyn Fn(&[IntersectionObserverEntry])>,
    /// Cada alvo com o último estado reportado.
    targets: RefCell<Vec<(Node, Option<bool>)>>,
}

impl ResizeObserverBackend for FakeIntersectionObserver {
    fn observe(&self, target: &Node) {
        let mut targets = self.targets.borrow_mut();
        if !targets.iter().any(|(candidate, _)| candidate == target) {
            targets.push((target.clone(), None));
        }
    }

    fn unobserve(&self, target: &Node) {
        self.targets.borrow_mut().retain(|(candidate, _)| candidate != target);
    }

    fn disconnect(&self) {
        self.targets.borrow_mut().clear();
    }
}

impl FakeIntersectionObserver {
    /// Entrega as mudanças desde a última entrega (e a primeira observação de cada alvo).
    fn deliver(&self) {
        let mut entries = Vec::new();
        for (target, reported) in self.targets.borrow_mut().iter_mut() {
            let intersecting = is_intersecting(target);
            if *reported != Some(intersecting) {
                *reported = Some(intersecting);
                entries.push(IntersectionObserverEntry { target: target.clone(), is_intersecting: intersecting });
            }
        }
        if !entries.is_empty() {
            (self.callback)(&entries);
        }
    }
}

struct FakeBrowser {
    window: Rc<CodeWindow>,
    observers: RefCell<Vec<Rc<FakeIntersectionObserver>>>,
    animations: RefCell<Vec<Rc<FakeAnimation>>>,
}

impl FakeBrowser {
    fn install() -> Rc<Self> {
        let browser = Rc::new(Self { window: main_window(), observers: RefCell::new(Vec::new()), animations: RefCell::new(Vec::new()) });

        let weak = Rc::downgrade(&browser);
        browser.window.set_intersection_observer_ctor(Rc::new(move |callback: Rc<dyn Fn(&[IntersectionObserverEntry])>| {
            let browser = weak.upgrade().expect("o navegador falso vive até o fim do teste");
            let observer = Rc::new(FakeIntersectionObserver { callback, targets: RefCell::new(Vec::new()) });
            browser.observers.borrow_mut().push(observer.clone());
            observer as Rc<dyn ResizeObserverBackend>
        }));

        let weak = Rc::downgrade(&browser);
        browser.window.set_get_animations(Rc::new(move |element: &Node, subtree: bool| -> Vec<Rc<dyn CssAnimation>> {
            let browser = weak.upgrade().expect("o navegador falso vive até o fim do teste");
            let mut nodes = vec![element.clone()];
            if subtree {
                descendant_elements(element, &mut nodes);
            }
            nodes.iter().filter(|node| animation_name(node).is_some()).map(|node| browser.animation_of(node) as Rc<dyn CssAnimation>).collect()
        }));
        browser
    }

    /// A animação (com identidade) do elemento.
    fn animation_of(&self, node: &Node) -> Rc<FakeAnimation> {
        let mut animations = self.animations.borrow_mut();
        if let Some(existing) = animations.iter().find(|animation| animation.node == *node) {
            return existing.clone();
        }
        let animation = Rc::new(FakeAnimation { node: node.clone(), start_time: Cell::new(None) });
        animations.push(animation.clone());
        animation
    }

    /// `element.getAnimations()`.
    fn get_animations(&self, element: &Node) -> Vec<Rc<dyn CssAnimation>> {
        self.window.get_animations(element, false).expect("a janela tem a Web Animations API")
    }

    /// `waitForAnimationFrames(count)`.
    fn wait_for_animation_frames(&self, count: usize) {
        for _ in 0..count {
            let reached = Rc::new(Cell::new(false));
            let flag = reached.clone();
            self.window.request_animation_frame(move || flag.set(true));
            self.window.render_frame();
            assert!(reached.get());
            let observers = self.observers.borrow().clone();
            for observer in observers {
                observer.deliver();
            }
        }
    }
}

#[test]
fn continues_observing_an_element_after_temporary_dom_detachment() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let browser = FakeBrowser::install();
    assert!(browser.window.intersection_observer_ctor().is_some());

    let host = document().create_element("div");
    let element = document().create_element("div");
    host.append_child(&element);
    let removed_host = host.clone();
    store.add(to_disposable(move || removed_host.remove()));
    store.add(pause_css_animations_when_hidden(&element, PauseCssAnimationsWhenHiddenOptions::new("paused")));

    browser.wait_for_animation_frames(5);
    let detached = element.class_list_contains("paused");
    document().body().append_child(&host);
    browser.wait_for_animation_frames(5);
    let attached = element.class_list_contains("paused");

    assert_eq!((detached, attached), (true, false));
}

#[test]
fn synchronization_does_not_resume_intentionally_paused_css_animations() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let browser = FakeBrowser::install();
    let element = document().create_element("div");
    document().body().append_child(&element);
    let removed_element = element.clone();
    store.add(to_disposable(move || removed_element.remove()));
    element.style_set_property("animation", "animation-sync-paused-test 1s linear infinite paused");
    synchronize_css_animations(&element, None);
    let animations: Vec<(&str, Option<f64>)> = browser.get_animations(&element).iter().map(|animation| (animation.play_state().as_str(), animation.start_time())).collect();
    assert_eq!(animations, [("paused", None)]);
}

#[test]
fn reports_visibility_transitions_and_stops_reporting_after_disposal() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let browser = FakeBrowser::install();
    let host = document().create_element("div");
    let element = host.append_child(&document().create_element("div"));
    let removed_host = host.clone();
    store.add(to_disposable(move || removed_host.remove()));
    let visible: Rc<RefCell<Vec<bool>>> = Rc::default();
    let mut options = PauseCssAnimationsWhenHiddenOptions::new("paused");
    let recorded = visible.clone();
    options.on_did_change_visibility = Some(Rc::new(move |value: bool| recorded.borrow_mut().push(value)));
    let observer = store.add(pause_css_animations_when_hidden(&element, options));
    browser.wait_for_animation_frames(5);
    document().body().append_child(&host);
    browser.wait_for_animation_frames(5);
    host.remove();
    browser.wait_for_animation_frames(5);
    document().body().append_child(&host);
    browser.wait_for_animation_frames(5);
    observer.dispose();
    host.remove();
    browser.wait_for_animation_frames(5);
    assert_eq!(*visible.borrow(), [false, true, false, true]);
}

#[test]
fn resynchronizes_native_css_animations_after_they_are_revealed() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let browser = FakeBrowser::install();
    // `.animation-sync-running-test { animation: ...; width: 10px; height: 10px; }` e
    // `.animation-sync-running-test.paused { animation-play-state: paused; }` são as regras de `animation_name` e `is_paused`.
    let host = document().body().append_child(&document().create_element("div"));
    host.set_attribute("style", "position: fixed; top: 0; left: 0; width: 100px; height: 100px; overflow: hidden;");
    let removed_host = host.clone();
    store.add(to_disposable(move || removed_host.remove()));
    let element = host.append_child(&document().create_element("div"));
    element.set_class_name(RUNNING_TEST_CLASS);
    element.style_set_property("margin-top", "200px");
    store.add(pause_css_animations_when_hidden(&element, PauseCssAnimationsWhenHiddenOptions::new("paused")));
    browser.wait_for_animation_frames(5);
    let hidden: Vec<&str> = browser.get_animations(&element).iter().map(|animation| animation.play_state().as_str()).collect();
    element.style_set_property("margin-top", "0");
    browser.wait_for_animation_frames(5);
    let revealed: Vec<(&str, Option<f64>)> = browser.get_animations(&element).iter().map(|animation| (animation.play_state().as_str(), animation.start_time())).collect();
    assert_eq!((hidden, revealed), (vec!["paused"], vec![("running", Some(0.0))]));
}
