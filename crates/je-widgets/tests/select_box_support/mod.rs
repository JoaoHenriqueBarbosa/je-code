//! Ajudantes dos testes da caixa de seleção: o `TestContextViewProvider` do `selectBox.test.ts` e os montadores de
//! janela, contêiner e eventos que o porte (`select_box.rs`) e os extras (`select_box_extra.rs`) compartilham.
#![allow(dead_code)]

use std::cell::RefCell;
use std::rc::Rc;

use je_base::key_codes::KeyCode;
use je_base::layout::AnchorPosition;
use je_base::lifecycle::{Disposable, DisposableRef, IDisposable, MutableDisposable, to_disposable};
use je_render::kurbo::Rect;
use je_ui::{Content, Event, EventKind, KeyData, Modifiers, NodeId, Role, Ui};
use je_widgets::context_view_api::{IContextViewDelegate, IContextViewProvider};
use je_widgets::dom::{clear_node, create_el};
use je_widgets::select_box::{ISelectBoxOptions, ISelectOptionItem, unthemed_select_box_styles};
use je_widgets::select_box_custom::SelectBoxList;

use crate::support::LeakGuard;

/// O `TestContextViewProvider` do teste.
pub struct TestContextViewProvider {
    base: Rc<Disposable>,
    ui: Ui,
    pub container: NodeId,
    view: Rc<MutableDisposable<dyn IDisposable>>,
    delegate: RefCell<Option<Rc<dyn IContextViewDelegate>>>,
}

impl TestContextViewProvider {
    pub fn new(ui: &Ui) -> Rc<Self> {
        let base = Disposable::new();
        let root = ui.root();
        let container = create_el(&mut ui.tree_mut(), root, Role::Generic, "test-context-view-provider");
        let view = MutableDisposable::<dyn IDisposable>::new();
        base.register(view.clone());
        let ui_for_remove = ui.clone();
        base.register(to_disposable(move || ui_for_remove.remove(container)));
        Rc::new(Self { base, ui: ui.clone(), container, view, delegate: RefCell::new(None) })
    }

    pub fn active_layer(&self) -> Option<i32> {
        self.delegate.borrow().as_ref().map(|d| d.layer())
    }

    pub fn anchor_position(&self) -> Option<AnchorPosition> {
        self.delegate.borrow().as_ref().and_then(|d| d.anchor_position())
    }
}

impl IContextViewProvider for TestContextViewProvider {
    fn show_context_view(&self, delegate: Rc<dyn IContextViewDelegate>, _container: Option<NodeId>) {
        self.view.clear();
        clear_node(&mut self.ui.tree_mut(), self.container);
        *self.delegate.borrow_mut() = Some(delegate.clone());
        let rendered: Option<DisposableRef> = delegate.render(self.container);
        self.view.set(rendered);
        delegate.layout();
    }

    fn hide_context_view(&self) {
        self.view.clear();
        let delegate = self.delegate.borrow_mut().take();
        if let Some(delegate) = delegate {
            delegate.on_hide(None);
        }
    }

    fn layout(&self) {
        let delegate = self.delegate.borrow().clone();
        if let Some(delegate) = delegate {
            delegate.layout();
        }
    }
}

impl IDisposable for TestContextViewProvider {
    fn dispose(&self) {
        self.base.dispose();
    }
}

pub fn new_ui() -> Ui {
    let ui = Ui::new();
    let root = ui.root();
    ui.tree_mut().set_bounds(root, Rect::new(0.0, 0.0, 1024.0, 768.0));
    ui
}

pub const WINDOW_HEIGHT: f64 = 768.0;

pub fn new_select_box(guard: &LeakGuard, ui: &Ui, provider: &Rc<TestContextViewProvider>, options: Vec<ISelectOptionItem>, select_box_options: ISelectBoxOptions) -> Rc<SelectBoxList> {
    let provider: Rc<dyn IContextViewProvider> = provider.clone();
    guard.add_rc(SelectBoxList::new(ui, options, 0, provider, unthemed_select_box_styles(), Some(select_box_options)))
}

pub fn numbered_options(count: usize) -> Vec<ISelectOptionItem> {
    (0..count).map(|index| ISelectOptionItem::new(&format!("Option {index}"))).collect()
}

/// O contêiner de `position: fixed` com `top` e a largura dadas (a do `<select>` se não houver).
pub fn fixed_container(guard: &LeakGuard, ui: &Ui, select_box: &SelectBoxList, top: f64, width: Option<f64>) -> NodeId {
    let (natural_width, height) = select_box.natural_size();
    let root = ui.root();
    let container = create_el(&mut ui.tree_mut(), root, Role::Generic, "fixed-container");
    set_top(ui, container, top, width.unwrap_or(natural_width), height);
    let ui = ui.clone();
    guard.add(to_disposable(move || ui.remove(container)));
    container
}

pub fn set_top(ui: &Ui, container: NodeId, top: f64, width: f64, height: f64) {
    ui.tree_mut().set_bounds(container, Rect::new(0.0, top, width, top + height));
}

pub fn click(ui: &Ui, node: NodeId) {
    ui.dispatch(&mut Event::new(EventKind::Click, node));
}

pub fn key(code: KeyCode) -> KeyData {
    KeyData { key_code: code, modifiers: Modifiers::default() }
}

pub fn text_of(ui: &Ui, node: NodeId) -> String {
    match &ui.tree().style(node).content {
        Content::Text { text, .. } => text.clone(),
        _ => String::new(),
    }
}

pub fn list_box(ui: &Ui, provider: &TestContextViewProvider) -> NodeId {
    let tree = ui.tree();
    tree.subtree(provider.container).into_iter().find(|n| tree.role(*n) == Role::ListBox).expect("lista")
}

