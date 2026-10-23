//! Porte de `src/vs/base/test/browser/ui/contextview/contextview.test.ts` (suíte `ContextView`), caso a caso e na mesma ordem.
//!
//! Divergências: o `je-ui` não tem `position`, `getComputedStyle` nem raiz de sombra. `getDomNodePagePosition` é
//! `tree.absolute_bounds`; o `view.style.display === 'none'` é `!style.visible`; `view.inert` é
//! `ContextView::is_inert`; a variável de CSS da duração é `ContextView::close_animation_duration_variable`;
//! `position: fixed` do `.shadow-root-host` é o nó com origem na janela e tamanho 0x0 mais `ContextView::is_fixed`.
//! O contêiner com borda e rolagem do caso "bordered scrolling containing block" é o contêiner deslocado pela
//! borda e pelo `scrollLeft`/`scrollTop` (o `je-ui` não tem rolagem de nó). O caso do `animation-fill-mode` e do
//! `will-change` (CSS computado de uma animação) fica `#[ignore]`.

mod support;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::layout::IAnchor;
use je_base::lifecycle::{DisposableRef, IDisposable, to_disposable};
use je_base::runtime;
use je_render::kurbo::Rect;
use je_ui::{NodeId, Role, Ui};
use je_widgets::context_view::ContextView;
use je_widgets::context_view_api::{Anchor, CONTEXT_VIEW_MENU_MOTION_CLASS, ContextViewDOMPosition, IContextViewCloseAnimation, IContextViewDelegate, context_view_menu_close_animation, CONTEXT_VIEW_MENU_MOTION_CLOSING_CLASS};
use support::{create_container, ensure_no_disposables_are_leaked_in_test_suite};

/// O `IDelegate` do teste: âncora, `render`, camada, animação de fechamento e `onHide`, todos opcionais.
struct Delegate {
    anchor: Box<dyn Fn() -> Anchor>,
    render: Box<dyn Fn(NodeId) -> Option<DisposableRef>>,
    layer: i32,
    close_animation: Option<IContextViewCloseAnimation>,
    on_hide: Option<Box<dyn Fn()>>,
}

impl Delegate {
    fn new(anchor: impl Fn() -> Anchor + 'static, render: impl Fn(NodeId) -> Option<DisposableRef> + 'static) -> Rc<Self> {
        Rc::new(Self { anchor: Box::new(anchor), render: Box::new(render), layer: 0, close_animation: None, on_hide: None })
    }
}

impl IContextViewDelegate for Delegate {
    fn get_anchor(&self) -> Anchor {
        (self.anchor)()
    }
    fn render(&self, container: NodeId) -> Option<DisposableRef> {
        (self.render)(container)
    }
    fn layer(&self) -> i32 {
        self.layer
    }
    fn close_animation(&self) -> Option<IContextViewCloseAnimation> {
        self.close_animation.clone()
    }
    fn on_hide(&self, _did_cancel: Option<bool>) {
        if let Some(on_hide) = &self.on_hide {
            on_hide();
        }
    }
}

fn anchor_at(x: f64, y: f64, width: Option<f64>, height: Option<f64>) -> Anchor {
    Anchor::Rect(IAnchor { x, y, width, height })
}

/// A janela do navegador: a raiz da árvore com o tamanho de uma janela.
fn new_ui() -> Ui {
    let ui = Ui::new();
    let root = ui.root();
    ui.tree_mut().set_bounds(root, Rect::new(0.0, 0.0, 1024.0, 768.0));
    ui
}

/// O `render` que dá ao nó da visão o tamanho de 10x10 (o `view.style.width/height = '10px'`).
fn size_10(ui: &Ui) -> impl Fn(NodeId) -> Option<DisposableRef> + 'static {
    let ui = ui.clone();
    move |view| {
        ui.tree_mut().set_bounds(view, Rect::new(0.0, 0.0, 10.0, 10.0));
        None
    }
}

/// `getDomNodePagePosition` arredondado: `(left, top)`.
fn page_position(ui: &Ui, node: NodeId) -> (f64, f64) {
    let r = ui.tree().absolute_bounds(node);
    (r.x0.round(), r.y0.round())
}

#[test]
fn hide_is_re_entrant_safe_and_does_not_double_dispose_render_result_319393() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = new_ui();
    let container = create_container(&ui, 0.0, 0.0);
    let context_view = ContextView::new(&ui, container, ContextViewDOMPosition::Absolute);

    let dispose_count = Rc::new(Cell::new(0));
    let (count, weak) = (dispose_count.clone(), Rc::downgrade(&context_view));
    let delegate = Delegate::new(
        || anchor_at(0.0, 0.0, None, None),
        move |_| {
            let (count, weak) = (count.clone(), weak.clone());
            Some(to_disposable(move || {
                count.set(count.get() + 1);
                if count.get() == 1 {
                    // Simula uma chamada reentrante de hide() (por exemplo, por um blur disparado ao tirar o nó do documento).
                    if let Some(context_view) = weak.upgrade() {
                        context_view.hide();
                    }
                }
            }))
        },
    );

    context_view.show(delegate);

    context_view.hide();
    assert_eq!(dispose_count.get(), 1, "render disposable must be disposed exactly once");

    context_view.dispose();
}

#[test]
fn shadow_dom_host_is_layered_with_the_context_view() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = new_ui();
    let container = create_container(&ui, 0.0, 0.0);
    let context_view = ContextView::new(&ui, container, ContextViewDOMPosition::FixedShadow);
    let delegate = Rc::new(Delegate { anchor: Box::new(|| anchor_at(0.0, 0.0, None, None)), render: Box::new(|_| None), layer: 1, close_animation: None, on_hide: None });

    context_view.show(delegate);

    let host = context_view.get_shadow_root_host().expect("o .shadow-root-host");
    let tree = ui.tree();
    assert!(tree.has_class(host, "shadow-root-host"));
    let host_abs = tree.absolute_bounds(host);
    assert_eq!(
        (context_view.is_fixed(), host_abs.x0, host_abs.y0, host_abs.width(), host_abs.height(), tree.style(host).z_index, tree.style(context_view.get_view_element()).z_index),
        (true, 0.0, 0.0, 0.0, 0.0, 2576, 2576)
    );
    drop(tree);

    context_view.dispose();
}

/// Lê o estado que os casos de animação de fechamento comparam: `(descartes, tem a classe, valor da variável, inert)`.
fn read_closing(ui: &Ui, context_view: &ContextView, disposals: &Cell<u32>, class_name: &str) -> (u32, bool, Option<String>, bool) {
    (disposals.get(), ui.tree().has_class(context_view.get_view_element(), class_name), context_view.close_animation_duration_variable(), context_view.is_inert())
}

#[test]
fn hide_delays_render_disposal_for_close_animations() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = new_ui();
    let container = create_container(&ui, 0.0, 0.0);
    ui.tree_mut().add_class(container, "modern-ui");
    ui.tree_mut().add_class(container, "monaco-enable-motion");
    let context_view = ContextView::new(&ui, container, ContextViewDOMPosition::Absolute);

    let dispose_count = Rc::new(Cell::new(0u32));
    let count = dispose_count.clone();
    let delegate = Rc::new(Delegate {
        anchor: Box::new(|| anchor_at(0.0, 0.0, None, None)),
        render: Box::new(move |_| {
            let count = count.clone();
            Some(to_disposable(move || count.set(count.get() + 1)))
        }),
        layer: 0,
        close_animation: Some(IContextViewCloseAnimation { class_name: "closing".to_owned(), duration: 100.0, required_ancestor_classes: vec![vec!["modern-ui".to_owned()], vec!["monaco-enable-motion".to_owned()]] }),
        on_hide: None,
    });

    let observed = runtime::with_faked_timers(|| {
        context_view.show(delegate);
        context_view.hide();
        context_view.hide();
        let during = read_closing(&ui, &context_view, &dispose_count, "closing");
        runtime::advance_fake_time(100);
        let after = read_closing(&ui, &context_view, &dispose_count, "closing");
        (during, after)
    });
    assert_eq!(observed.0, (0, true, Some("100ms".to_owned()), true));
    assert_eq!(observed.1, (1, false, None, false));

    context_view.dispose();
    assert_eq!(dispose_count.get(), 1);
}

/// O corpo dos 12 casos `menu closing motion with <nome> in DOM position <n>`.
fn menu_closing_motion(classes: &str, animate: bool, dom_position: ContextViewDOMPosition) {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = new_ui();
    let container = create_container(&ui, 0.0, 0.0);
    for class in classes.split(' ') {
        ui.tree_mut().add_class(container, class);
    }
    let context_view = store.add_rc(ContextView::new(&ui, container, dom_position));
    let (hides, disposals) = (Rc::new(Cell::new(0u32)), Rc::new(Cell::new(0u32)));
    let (h, d) = (hides.clone(), disposals.clone());
    let delegate = Rc::new(Delegate {
        anchor: Box::new(|| anchor_at(0.0, 0.0, None, None)),
        render: Box::new(move |_| {
            let d = d.clone();
            Some(to_disposable(move || d.set(d.get() + 1)))
        }),
        layer: 0,
        close_animation: Some(context_view_menu_close_animation()),
        on_hide: Some(Box::new(move || h.set(h.get() + 1))),
    });

    let view = context_view.get_view_element();
    // `(hides, disposals, closing, hidden, inert)`
    let read = || (hides.get(), disposals.get(), ui.tree().has_class(view, CONTEXT_VIEW_MENU_MOTION_CLOSING_CLASS), !ui.tree().style(view).visible, context_view.is_inert());
    let (initial, before_end, finished) = runtime::with_faked_timers(|| {
        context_view.show(delegate);
        context_view.hide();
        context_view.hide();
        let initial = read();
        runtime::advance_fake_time(context_view_menu_close_animation().duration as u64 - 1);
        let before_end = read();
        runtime::advance_fake_time(1);
        (initial, before_end, read())
    });

    let during_close = (1, u32::from(!animate), animate, !animate, animate);
    assert_eq!((initial, before_end, finished), (during_close, during_close, (1, 1, false, true, false)));
}

const EDITOR_MOTION: &str = "modern-ui monaco-enable-motion";
const AGENTS_GLASS_MOTION: &str = "agent-sessions-workbench modern-ui-frosted-glass monaco-enable-motion";
const AGENTS_WITHOUT_GLASS: &str = "agent-sessions-workbench monaco-enable-motion";
const AGENTS_REDUCED_MOTION: &str = "agent-sessions-workbench modern-ui-frosted-glass monaco-reduce-motion";
const EDITOR_REDUCED_MOTION: &str = "modern-ui monaco-reduce-motion";
const LEGACY_MOTION: &str = "monaco-enable-motion";

#[test]
fn menu_closing_motion_with_editor_motion_in_dom_position_1() {
    menu_closing_motion(EDITOR_MOTION, true, ContextViewDOMPosition::Absolute);
}

#[test]
fn menu_closing_motion_with_agents_glass_motion_in_dom_position_1() {
    menu_closing_motion(AGENTS_GLASS_MOTION, true, ContextViewDOMPosition::Absolute);
}

#[test]
fn menu_closing_motion_with_agents_without_glass_in_dom_position_1() {
    menu_closing_motion(AGENTS_WITHOUT_GLASS, false, ContextViewDOMPosition::Absolute);
}

#[test]
fn menu_closing_motion_with_agents_reduced_motion_in_dom_position_1() {
    menu_closing_motion(AGENTS_REDUCED_MOTION, false, ContextViewDOMPosition::Absolute);
}

#[test]
fn menu_closing_motion_with_editor_reduced_motion_in_dom_position_1() {
    menu_closing_motion(EDITOR_REDUCED_MOTION, false, ContextViewDOMPosition::Absolute);
}

#[test]
fn menu_closing_motion_with_legacy_motion_in_dom_position_1() {
    menu_closing_motion(LEGACY_MOTION, false, ContextViewDOMPosition::Absolute);
}

#[test]
fn menu_closing_motion_with_editor_motion_in_dom_position_3() {
    menu_closing_motion(EDITOR_MOTION, true, ContextViewDOMPosition::FixedShadow);
}

#[test]
fn menu_closing_motion_with_agents_glass_motion_in_dom_position_3() {
    menu_closing_motion(AGENTS_GLASS_MOTION, true, ContextViewDOMPosition::FixedShadow);
}

#[test]
fn menu_closing_motion_with_agents_without_glass_in_dom_position_3() {
    menu_closing_motion(AGENTS_WITHOUT_GLASS, false, ContextViewDOMPosition::FixedShadow);
}

#[test]
fn menu_closing_motion_with_agents_reduced_motion_in_dom_position_3() {
    menu_closing_motion(AGENTS_REDUCED_MOTION, false, ContextViewDOMPosition::FixedShadow);
}

#[test]
fn menu_closing_motion_with_editor_reduced_motion_in_dom_position_3() {
    menu_closing_motion(EDITOR_REDUCED_MOTION, false, ContextViewDOMPosition::FixedShadow);
}

#[test]
fn menu_closing_motion_with_legacy_motion_in_dom_position_3() {
    menu_closing_motion(LEGACY_MOTION, false, ContextViewDOMPosition::FixedShadow);
}

#[test]
fn positions_absolute_view_when_the_container_is_position_static() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = new_ui();
    let (host, container) = {
        let mut tree = ui.tree_mut();
        let root = tree.root();
        let host = tree.create(root, Role::Generic);
        tree.set_bounds(host, Rect::new(0.0, 0.0, 1024.0, 60.0));
        let spacer = tree.create(host, Role::Generic);
        tree.set_bounds(spacer, Rect::new(0.0, 0.0, 1024.0, 60.0));
        let container = tree.create(host, Role::Generic);
        tree.set_bounds(container, Rect::new(0.0, 60.0, 1024.0, 60.0));
        (host, container)
    };

    let context_view = ContextView::new(&ui, container, ContextViewDOMPosition::Absolute);
    context_view.show(Delegate::new(|| anchor_at(100.0, 100.0, Some(1.0), Some(1.0)), size_10(&ui)));

    assert_eq!(page_position(&ui, context_view.get_view_element()), (100.0, 101.0));

    context_view.dispose();
    ui.remove(host);
}

#[test]
fn positions_absolute_view_in_a_bordered_scrolling_containing_block() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = new_ui();
    // `ancestor` (200x200, borda de 10px, rolado 30 e 40) e o contêiner de 500x500 dentro dele: a borda e a
    // rolagem deslocam o contêiner em (10 - 30, 10 - 40) em relação ao ancestral.
    let (ancestor, container) = {
        let mut tree = ui.tree_mut();
        let root = tree.root();
        let ancestor = tree.create(root, Role::Generic);
        tree.set_bounds(ancestor, Rect::new(37.0, 53.0, 237.0, 253.0));
        tree.style_mut(ancestor).clip = true;
        let container = tree.create(ancestor, Role::Generic);
        tree.set_bounds(container, Rect::new(10.0 - 30.0, 10.0 - 40.0, 10.0 - 30.0 + 500.0, 10.0 - 40.0 + 500.0));
        (ancestor, container)
    };

    let ancestor_position = ui.tree().absolute_bounds(ancestor);
    let anchor = IAnchor { x: ancestor_position.x0 + 100.0, y: ancestor_position.y0 + 100.0, width: Some(1.0), height: Some(1.0) };
    let context_view = ContextView::new(&ui, container, ContextViewDOMPosition::Absolute);
    context_view.show(Delegate::new(move || Anchor::Rect(anchor), size_10(&ui)));

    assert_eq!(page_position(&ui, context_view.get_view_element()), (anchor.x.round(), (anchor.y + anchor.height.expect("altura")).round()));

    context_view.dispose();
    ui.remove(ancestor);
}

#[test]
fn relayouts_fixed_view_from_the_positioning_origin() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = new_ui();
    let container = {
        let mut tree = ui.tree_mut();
        let root = tree.root();
        let container = tree.create(root, Role::Generic);
        tree.set_bounds(container, Rect::new(0.0, 0.0, 0.0, 0.0));
        container
    };

    let anchor_y = Rc::new(RefCell::new(100.0));
    let current = anchor_y.clone();
    let context_view = ContextView::new(&ui, container, ContextViewDOMPosition::Fixed);
    context_view.show(Delegate::new(move || anchor_at(100.0, *current.borrow(), Some(1.0), Some(1.0)), size_10(&ui)));

    *anchor_y.borrow_mut() = 200.0;
    context_view.layout();

    assert_eq!(page_position(&ui, context_view.get_view_element()), (100.0, 201.0));

    context_view.dispose();
    ui.remove(container);
}

#[test]
#[ignore = "lê o CSS computado de uma animação (animation-fill-mode e will-change da folha de estilo de movimento do menu); o je-ui não tem CSS nem animação"]
fn menu_motion_does_not_retain_a_containing_block_for_submenus_326248() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = new_ui();
    let container = create_container(&ui, 0.0, 0.0);
    let context_view = ContextView::new(&ui, container, ContextViewDOMPosition::Absolute);
    context_view.show(Delegate::new(|| anchor_at(0.0, 0.0, None, None), |_| None));
    ui.tree_mut().add_class(context_view.get_view_element(), CONTEXT_VIEW_MENU_MOTION_CLASS);
    context_view.dispose();
}
