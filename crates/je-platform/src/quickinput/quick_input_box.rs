//! Porte de `src/vs/platform/quickinput/browser/quickInputBox.ts`: a caixa de texto do quick input (`QuickInputBox`), sobre o
//! `InputBox` de base, com a decoração de severidade, os botões de entrada, a altura e o modo de foco da lista.
//!
//! Notas de tradução:
//! - **DOM.** O `InputBox` de base escreve num `<input>`. Aqui o estado do `<input>` (valor, seleção, atributos `aria-*`,
//!   `placeholder`, `type=password`, desabilitado) é guardado direto, e o desenho nativo o lê. Seleção e cursor são em unidades
//!   UTF-16, como `selectionStart` e `selectionEnd`.
//! - **Foco.** `document.activeElement` é o [`QuickInputFocus`], compartilhado entre a caixa, a lista e o contêiner.
//! - **Estilos.** `styles.inputBox` entra como [`QuickInputBoxStyles`] (as cores de validação por severidade).
//! - **`toggles`.** O original guarda os `Toggle` de `quickInputBox.toggles`; o `QuickInputController` só os zera
//!   (`ui.inputBox.toggles = undefined`), então aqui `toggles` é o contador de toggles ativos.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::rc::Rc;

use je_base::actions::ActionRef;
use je_base::event::{Emitter, Event};
use je_base::key_codes::KeyCode;
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::severity::Severity;

use super::quick_input_browser::{IQuickInputBoxUi, QuickInputMessageStyles};

/// Para onde o foco do teclado aponta (o `document.activeElement` dentro do widget).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuickInputFocusTarget {
    /// Fora do widget.
    None,
    /// O contêiner (`tabIndex = -1`).
    Container,
    /// A caixa de texto.
    Input,
    /// A lista.
    List,
    /// A árvore.
    Tree,
}

/// O estado de foco compartilhado pelas peças do widget.
#[derive(Debug)]
pub struct QuickInputFocus {
    target: Cell<QuickInputFocusTarget>,
}

impl QuickInputFocus {
    pub fn new() -> Rc<Self> {
        Rc::new(Self { target: Cell::new(QuickInputFocusTarget::None) })
    }

    pub fn target(&self) -> QuickInputFocusTarget {
        self.target.get()
    }

    pub fn set(&self, target: QuickInputFocusTarget) {
        self.target.set(target);
    }

    /// `dom.isAncestorOfActiveElement(container)`: o foco está em alguma peça do widget.
    pub fn is_inside(&self) -> bool {
        self.target.get() != QuickInputFocusTarget::None
    }
}

/// `IInputBoxStyles` do que a caixa do quick input usa: as cores de cada severidade da mensagem.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct QuickInputBoxStyles {
    pub info: QuickInputMessageStyles,
    pub warning: QuickInputMessageStyles,
    pub error: QuickInputMessageStyles,
}

/// O `KeyboardEvent` do `keydown` da caixa (só o que o controlador lê).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QuickInputKeyDown {
    pub key_code: KeyCode,
}

/// `QuickInputBox`.
pub struct QuickInputBox {
    base: Rc<Disposable>,
    focus: Rc<QuickInputFocus>,
    value: RefCell<String>,
    placeholder: RefCell<String>,
    aria_label: RefCell<Option<String>>,
    password: Cell<bool>,
    selection: Cell<(u32, u32)>,
    enabled: Cell<bool>,
    height: Cell<Option<f64>>,
    list_focus_mode: Cell<bool>,
    attributes: RefCell<BTreeMap<String, String>>,
    severity: Cell<Severity>,
    styles: RefCell<QuickInputBoxStyles>,
    actions: RefCell<Vec<ActionRef>>,
    toggles: Cell<usize>,
    laid_out: Cell<u32>,
    on_did_change: Rc<Emitter<String>>,
    on_key_down: Rc<Emitter<QuickInputKeyDown>>,
}

fn utf16_len(text: &str) -> u32 {
    text.encode_utf16().count() as u32
}

impl QuickInputBox {
    pub fn new(focus: Rc<QuickInputFocus>, styles: QuickInputBoxStyles) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change = Emitter::new();
        let on_key_down = Emitter::new();
        base.register(on_did_change.clone());
        base.register(on_key_down.clone());
        Rc::new(Self {
            base,
            focus,
            value: RefCell::new(String::new()),
            placeholder: RefCell::new(String::new()),
            aria_label: RefCell::new(None),
            password: Cell::new(false),
            selection: Cell::new((0, 0)),
            enabled: Cell::new(true),
            height: Cell::new(None),
            list_focus_mode: Cell::new(false),
            attributes: RefCell::new(BTreeMap::new()),
            severity: Cell::new(Severity::Ignore),
            styles: RefCell::new(styles),
            actions: RefCell::new(Vec::new()),
            toggles: Cell::new(0),
            laid_out: Cell::new(0),
            on_did_change,
            on_key_down,
        })
    }

    /// `applyStyles` do `InputBox` de base.
    pub fn apply_styles(&self, styles: QuickInputBoxStyles) {
        *self.styles.borrow_mut() = styles;
    }

    pub fn on_key_down(&self) -> Event<QuickInputKeyDown> {
        self.on_key_down.event()
    }

    /// Entrega ao `onKeyDown` o `keydown` que a caixa recebeu (o que o desenho nativo faz ao receber a tecla).
    pub fn press_key(&self, key_code: KeyCode) {
        self.on_key_down.fire(QuickInputKeyDown { key_code });
    }

    /// O usuário digitou: troca o valor, leva o cursor ao fim e avisa (o `input` event do `<input>`).
    pub fn type_text(&self, value: &str) {
        self.set_value_internal(value);
    }

    fn set_value_internal(&self, value: &str) {
        if *self.value.borrow() == value {
            return;
        }
        *self.value.borrow_mut() = value.to_owned();
        let end = utf16_len(value);
        self.selection.set((end, end));
        self.on_did_change.fire(value.to_owned());
    }

    /// `set-attribute` do elemento `<input>` (`aria-describedby`, `aria-controls`, `aria-activedescendant`...).
    pub fn set_attribute(&self, name: &str, value: &str) {
        self.attributes.borrow_mut().insert(name.to_owned(), value.to_owned());
    }

    pub fn remove_attribute(&self, name: &str) {
        self.attributes.borrow_mut().remove(name);
    }

    pub fn attribute(&self, name: &str) -> Option<String> {
        self.attributes.borrow().get(name).cloned()
    }

    /// `setListFocusMode(enabled)`: `aria-haspopup` e o `role` de combobox com a lista focada.
    pub fn set_list_focus_mode(&self, enabled: bool) {
        self.list_focus_mode.set(enabled);
    }

    pub fn list_focus_mode(&self) -> bool {
        self.list_focus_mode.get()
    }

    /// `isSelectionAtEnd()`.
    pub fn is_selection_at_end(&self) -> bool {
        let (start, end) = self.selection.get();
        end == utf16_len(&self.value.borrow()) && start == end
    }

    pub fn enabled(&self) -> bool {
        self.enabled.get()
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.set(enabled);
        if !enabled && self.focus.target() == QuickInputFocusTarget::Input {
            self.focus.set(QuickInputFocusTarget::Container);
        }
    }

    /// `toggles = undefined`.
    pub fn clear_toggles(&self) {
        self.toggles.set(0);
    }

    pub fn toggles(&self) -> usize {
        self.toggles.get()
    }

    /// `actions = undefined`.
    pub fn clear_actions(&self) {
        self.actions.borrow_mut().clear();
    }

    pub fn actions(&self) -> Vec<ActionRef> {
        self.actions.borrow().clone()
    }

    /// `setHeight(height)`: `undefined` volta à altura natural.
    pub fn set_height(&self, height: Option<f64>) {
        self.height.set(height);
    }

    pub fn height(&self) -> Option<f64> {
        self.height.get()
    }

    /// `layout()`: quantas vezes o desenho foi refeito (o nativo mede a caixa de novo a cada chamada).
    pub fn layout(&self) {
        self.laid_out.set(self.laid_out.get() + 1);
    }

    pub fn layout_count(&self) -> u32 {
        self.laid_out.get()
    }

    pub fn severity(&self) -> Severity {
        self.severity.get()
    }
}

impl IQuickInputBoxUi for QuickInputBox {
    fn value(&self) -> String {
        self.value.borrow().clone()
    }

    fn set_value(&self, value: &str) {
        self.set_value_internal(value);
    }

    fn placeholder(&self) -> String {
        self.placeholder.borrow().clone()
    }

    fn set_placeholder(&self, placeholder: &str) {
        *self.placeholder.borrow_mut() = placeholder.to_owned();
    }

    fn aria_label(&self) -> Option<String> {
        self.aria_label.borrow().clone()
    }

    fn set_aria_label(&self, aria_label: &str) {
        *self.aria_label.borrow_mut() = Some(aria_label.to_owned());
    }

    fn password(&self) -> bool {
        self.password.get()
    }

    fn set_password(&self, password: bool) {
        self.password.set(password);
    }

    fn select(&self, range: Option<(u32, u32)>) {
        let len = utf16_len(&self.value.borrow());
        let (start, end) = range.unwrap_or((0, len));
        self.selection.set((start.min(len), end.min(len)));
    }

    fn get_selection(&self) -> Option<(u32, u32)> {
        Some(self.selection.get())
    }

    fn has_focus(&self) -> bool {
        self.focus.target() == QuickInputFocusTarget::Input
    }

    fn set_focus(&self) {
        if self.enabled.get() {
            self.focus.set(QuickInputFocusTarget::Input);
        }
    }

    fn on_did_change(&self) -> Event<String> {
        self.on_did_change.event()
    }

    fn show_decoration(&self, severity: Severity) {
        self.severity.set(severity);
    }

    fn styles_for_type(&self, severity: Severity) -> QuickInputMessageStyles {
        let styles = self.styles.borrow();
        match severity {
            Severity::Info => styles.info.clone(),
            Severity::Warning => styles.warning.clone(),
            Severity::Error => styles.error.clone(),
            Severity::Ignore => QuickInputMessageStyles::default(),
        }
    }

    fn set_actions(&self, actions: Vec<ActionRef>) {
        *self.actions.borrow_mut() = actions;
    }
}

impl IDisposable for QuickInputBox {
    fn dispose(&self) {
        self.base.dispose();
    }
}
