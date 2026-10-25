//! O estado nativo de um elemento de interface: o que o `actionWidget.ts`, o `actionList.ts` e a `ContextView` leem do DOM
//! (`classList`, `style`, `parentElement`, `closest`, `getBoundingClientRect`, `clientHeight`) e o evento de teclado que
//! eles tratam (`KeyboardEvent` com `defaultPrevented` e `stopPropagation`).

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::rc::Rc;

use je_base::key_codes::KeyCode;
use je_base::layout::{IRect, ISize};

/// Mede o conteúdo do elemento sob demanda (o refluxo do DOM): devolve o tamanho que o elemento ocupa agora.
pub type MeasureFn = Rc<dyn Fn() -> ISize>;

/// Um elemento: pai, classes, estilo, posição e tamanho.
pub struct ViewElement {
    parent: RefCell<Option<Rc<ViewElement>>>,
    classes: RefCell<Vec<String>>,
    style: RefCell<BTreeMap<String, String>>,
    top: Cell<f64>,
    left: Cell<f64>,
    width: Cell<f64>,
    height: Cell<f64>,
    client_height: Cell<f64>,
    measure: RefCell<Option<MeasureFn>>,
    opening_animation_finished: Cell<bool>,
    native: Option<Rc<dyn Any>>,
}

impl ViewElement {
    pub fn new() -> Rc<Self> {
        Self::build(None)
    }

    /// Um elemento que representa um nó da árvore de widgets nativa. O identificador é opaco para a plataforma: quem
    /// preenche (o workbench, com o `NodeId` do `Ui`) o lê de volta com [`ViewElement::native_as`].
    pub fn from_native(node: Rc<dyn Any>) -> Rc<Self> {
        Self::build(Some(node))
    }

    /// O identificador nativo lido como `T`, ou `None` se o elemento não é nativo ou o tipo é outro.
    pub fn native_as<T: Any + Clone>(&self) -> Option<T> {
        self.native.as_ref().and_then(|node| node.downcast_ref::<T>()).cloned()
    }

    fn build(native: Option<Rc<dyn Any>>) -> Rc<Self> {
        Rc::new(Self {
            native,
            parent: RefCell::new(None),
            classes: RefCell::new(Vec::new()),
            style: RefCell::new(BTreeMap::new()),
            top: Cell::new(0.0),
            left: Cell::new(0.0),
            width: Cell::new(0.0),
            height: Cell::new(0.0),
            client_height: Cell::new(0.0),
            measure: RefCell::new(None),
            opening_animation_finished: Cell::new(false),
        })
    }

    /// Um elemento já com classes (`dom.$('.a.b')`).
    pub fn with_classes(classes: &[&str]) -> Rc<Self> {
        let element = Self::new();
        for class_name in classes {
            element.class_list_add(class_name);
        }
        element
    }

    /// `parentElement`.
    pub fn parent_element(&self) -> Option<Rc<ViewElement>> {
        self.parent.borrow().clone()
    }

    /// `parent.appendChild(this)` e `this.remove()`: troca o pai.
    pub fn set_parent(&self, parent: Option<Rc<ViewElement>>) {
        *self.parent.borrow_mut() = parent;
    }

    /// `classList.add(name)`.
    pub fn class_list_add(&self, name: &str) {
        let mut classes = self.classes.borrow_mut();
        if !classes.iter().any(|existing| existing == name) {
            classes.push(name.to_owned());
        }
    }

    /// `classList.remove(name)`.
    pub fn class_list_remove(&self, name: &str) {
        self.classes.borrow_mut().retain(|existing| existing != name);
    }

    /// `classList.contains(name)`.
    pub fn class_list_contains(&self, name: &str) -> bool {
        self.classes.borrow().iter().any(|existing| existing == name)
    }

    /// `classList` inteira, na ordem de inserção.
    pub fn class_list(&self) -> Vec<String> {
        self.classes.borrow().clone()
    }

    /// `className = value`: troca todas as classes.
    pub fn set_class_name(&self, value: &str) {
        *self.classes.borrow_mut() = value.split_whitespace().map(str::to_owned).collect();
    }

    /// `element.closest(selector)` para um seletor de classe: o próprio elemento ou o ancestral mais próximo que a tem.
    pub fn closest(self: &Rc<Self>, class_name: &str) -> Option<Rc<ViewElement>> {
        let mut candidate = Some(self.clone());
        while let Some(current) = candidate {
            if current.class_list_contains(class_name) {
                return Some(current);
            }
            candidate = current.parent_element();
        }
        None
    }

    /// `style.setProperty(name, value)` (e as atribuições a `style.top` e companhia).
    pub fn style_set_property(&self, name: &str, value: &str) {
        self.style.borrow_mut().insert(name.to_owned(), value.to_owned());
    }

    /// `style.removeProperty(name)`.
    pub fn style_remove_property(&self, name: &str) {
        self.style.borrow_mut().remove(name);
    }

    /// `style.getPropertyValue(name)`, `None` quando não definida (o `''` do DOM).
    pub fn style_get_property(&self, name: &str) -> Option<String> {
        self.style.borrow().get(name).cloned()
    }

    /// `DOM.hide(element)` (`display: none`).
    pub fn hide(&self) {
        self.style_set_property("display", "none");
    }

    /// `DOM.show(element)`.
    pub fn show(&self) {
        self.style_remove_property("display");
    }

    /// `style.display === 'none'`.
    pub fn is_hidden(&self) -> bool {
        self.style_get_property("display").as_deref() == Some("none")
    }

    /// Define como o elemento mede o próprio conteúdo (o refluxo).
    pub fn set_measure(&self, measure: MeasureFn) {
        *self.measure.borrow_mut() = Some(measure);
    }

    /// Desfaz [`ViewElement::set_measure`] (o elemento volta ao tamanho guardado).
    pub fn clear_measure(&self) {
        *self.measure.borrow_mut() = None;
    }

    /// `offsetWidth` e `offsetHeight`: o resultado da medição, ou o tamanho guardado.
    pub fn size(&self) -> ISize {
        let measure = self.measure.borrow().clone();
        match measure {
            Some(measure) => measure(),
            None => ISize { width: self.width.get(), height: self.height.get() },
        }
    }

    /// Guarda o tamanho (`style.width`/`style.height` de um elemento sem medição própria).
    pub fn set_size(&self, width: f64, height: f64) {
        self.width.set(width);
        self.height.set(height);
    }

    /// Guarda a posição (`style.top` e `style.left`).
    pub fn set_position(&self, top: f64, left: f64) {
        self.top.set(top);
        self.left.set(left);
    }

    /// `getBoundingClientRect()`.
    pub fn rect(&self) -> IRect {
        let size = self.size();
        IRect { top: self.top.get(), left: self.left.get(), width: size.width, height: size.height }
    }

    /// `clientHeight`.
    pub fn client_height(&self) -> f64 {
        self.client_height.get()
    }

    pub fn set_client_height(&self, value: f64) {
        self.client_height.set(value);
    }

    /// `animation.finish()` das animações de entrada do elemento (o pintor nativo lê o estado).
    pub fn finish_opening_animation(&self) {
        self.opening_animation_finished.set(true);
    }

    pub fn is_opening_animation_finished(&self) -> bool {
        self.opening_animation_finished.get()
    }
}

/// O `KeyboardEvent` que os tratadores do widget leem e alteram.
pub struct UiKeyEvent {
    /// `key`.
    pub key: String,
    /// `keyCode` já na codificação do VS Code (`StandardKeyboardEvent.keyCode`).
    pub key_code: KeyCode,
    pub ctrl_key: bool,
    pub meta_key: bool,
    pub alt_key: bool,
    pub shift_key: bool,
    pub is_composing: bool,
    default_prevented: Cell<bool>,
    propagation_stopped: Cell<bool>,
}

impl UiKeyEvent {
    pub fn new(key: &str, key_code: KeyCode) -> Self {
        Self {
            key: key.to_owned(),
            key_code,
            ctrl_key: false,
            meta_key: false,
            alt_key: false,
            shift_key: false,
            is_composing: false,
            default_prevented: Cell::new(false),
            propagation_stopped: Cell::new(false),
        }
    }

    /// `preventDefault()`.
    pub fn prevent_default(&self) {
        self.default_prevented.set(true);
    }

    /// `defaultPrevented`.
    pub fn default_prevented(&self) -> bool {
        self.default_prevented.get()
    }

    /// `stopPropagation()`.
    pub fn stop_propagation(&self) {
        self.propagation_stopped.set(true);
    }

    pub fn propagation_stopped(&self) -> bool {
        self.propagation_stopped.get()
    }

    /// `EventHelper.stop(e, true)`: `preventDefault()` e `stopPropagation()`.
    pub fn stop(&self) {
        self.prevent_default();
        self.stop_propagation();
    }
}
