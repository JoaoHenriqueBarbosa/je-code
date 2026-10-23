//! O que o `dom.ts` e o `keyboardEvent.ts` do VS Code oferecem aos widgets, sobre a árvore do `je-ui`:
//! criar elemento com classes (`$('.a.b')`), `StandardKeyboardEvent`, `trackFocus`, `isAncestor`,
//! `clearNode`, `show`/`hide` e o tamanho da janela.

use std::cell::Cell;
use std::rc::Rc;

use je_base::async_::TimeoutTimer;
use je_base::event::{Emitter, Event as BaseEvent};
use je_base::key_codes::{KeyCode, is_modifier_key};
use je_base::lifecycle::{Disposable, IDisposable};
use je_render::kurbo::Rect;
use je_ui::{Event, EventKind, NodeId, Role, Tree, Ui};

use crate::common::registered_emitter;
use crate::sash::is_macintosh;

/// `$('.a.b')` + `append`: cria um nó como último filho de `parent`, com as classes (separadas por espaço).
pub fn create_el(tree: &mut Tree, parent: NodeId, role: Role, class_name: &str) -> NodeId {
    let node = tree.create(parent, role);
    tree.set_class_name(node, class_name);
    node
}

/// `document.createElement` + `className`: um nó solto, com as classes.
pub fn create_detached_el(tree: &mut Tree, role: Role, class_name: &str) -> NodeId {
    let node = tree.create_detached(role);
    tree.set_class_name(node, class_name);
    node
}

/// `clearNode`: remove (destrói) todos os filhos.
pub fn clear_node(tree: &mut Tree, node: NodeId) {
    for child in tree.children(node).to_vec() {
        tree.remove(child);
    }
}

/// `hide(...)`: `display: none`.
pub fn hide(tree: &mut Tree, node: NodeId) {
    tree.style_mut(node).visible = false;
}

/// `show(...)`: tira o `display: none`.
pub fn show(tree: &mut Tree, node: NodeId) {
    tree.style_mut(node).visible = true;
}

/// O retângulo da janela (a raiz da árvore): `window.innerWidth` e `innerHeight`.
pub fn window_size(ui: &Ui) -> (f64, f64) {
    let tree = ui.tree();
    let r: Rect = tree.bounds(tree.root());
    (r.width(), r.height())
}

/// `isAncestor(testChild, testAncestor)`: o ancestral é o próprio nó ou um dos antepassados dele.
pub fn is_ancestor(ui: &Ui, node: NodeId, ancestor: NodeId) -> bool {
    ui.tree().is_ancestor(ancestor, node)
}

/// `isActiveElement(el)`.
pub fn is_active_element(ui: &Ui, node: NodeId) -> bool {
    ui.focused() == Some(node)
}

/// `isAncestorOfActiveElement(el)`: o foco está no nó ou num descendente.
pub fn is_ancestor_of_active_element(ui: &Ui, node: NodeId) -> bool {
    ui.focused().is_some_and(|f| ui.tree().is_ancestor(node, f))
}

/// O `StandardKeyboardEvent`: a tecla e os modificadores de um `keydown`, com o `equals` do original.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StandardKeyboardEvent {
    pub key_code: KeyCode,
    pub ctrl_key: bool,
    pub shift_key: bool,
    pub alt_key: bool,
    pub meta_key: bool,
}

impl StandardKeyboardEvent {
    /// Lê a tecla de um evento de teclado; `None` se o evento não traz tecla.
    pub fn from_event(e: &Event) -> Option<Self> {
        let key = e.key?;
        Some(Self { key_code: key.key_code, ctrl_key: key.modifiers.ctrl, shift_key: key.modifiers.shift, alt_key: key.modifiers.alt, meta_key: key.modifiers.meta })
    }

    /// A tecla e os modificadores como número de `KeyMod | KeyCode` (o `_computeKeybinding`): `ctrl` é
    /// `CtrlCmd` fora do macOS e `WinCtrl` no macOS, e `meta` o contrário.
    pub fn as_keybinding(&self) -> u32 {
        use je_base::key_codes::KeyMod;
        let key = if is_modifier_key(self.key_code) { 0 } else { self.key_code.0 as u32 };
        let (ctrl_mod, meta_mod) = if is_macintosh() { (KeyMod::WIN_CTRL, KeyMod::CTRL_CMD) } else { (KeyMod::CTRL_CMD, KeyMod::WIN_CTRL) };
        let mut result = key;
        if self.ctrl_key {
            result |= ctrl_mod;
        }
        if self.alt_key {
            result |= KeyMod::ALT;
        }
        if self.shift_key {
            result |= KeyMod::SHIFT;
        }
        if self.meta_key {
            result |= meta_mod;
        }
        result
    }

    /// `equals(KeyMod.CtrlCmd | KeyCode.X)`: igualdade exata da tecla e dos modificadores.
    pub fn equals(&self, keybinding: u32) -> bool {
        self.as_keybinding() == keybinding
    }
}

/// `trackFocus`: avisa quando o nó ou um descendente ganha ou perde o foco (escuta `focus` e `blur` em
/// captura, como o original). Como no `FocusTracker` do `dom.ts`, a perda é adiada para o próximo ciclo do
/// event loop (`setTimeout(0)`): o `blur` de um descendente seguido de `focus` em outro descendente não conta
/// como perda.
pub struct FocusTracker {
    base: Rc<Disposable>,
    did_focus: Rc<Emitter<()>>,
    did_blur: Rc<Emitter<()>>,
    has_focus: Rc<Cell<bool>>,
}

impl FocusTracker {
    pub fn new(ui: &Ui, node: NodeId) -> Rc<Self> {
        let base = Disposable::new();
        let did_focus = registered_emitter::<()>(&base);
        let did_blur = registered_emitter::<()>(&base);
        let has_focus = Rc::new(Cell::new(is_ancestor_of_active_element(ui, node)));
        let loosing_focus = Rc::new(Cell::new(false));
        let timer = TimeoutTimer::new();
        base.register(timer.clone());
        {
            let (emitter, state, loosing) = (did_focus.clone(), has_focus.clone(), loosing_focus.clone());
            base.register(ui.listen(node, EventKind::Focus, true, move |_| {
                loosing.set(false);
                if !state.get() {
                    state.set(true);
                    emitter.fire(());
                }
            }));
        }
        {
            let (emitter, state, loosing) = (did_blur.clone(), has_focus.clone(), loosing_focus);
            base.register(ui.listen(node, EventKind::Blur, true, move |_| {
                if state.get() {
                    loosing.set(true);
                    let (emitter, state, loosing) = (emitter.clone(), state.clone(), loosing.clone());
                    timer.cancel_and_set(
                        move || {
                            if loosing.get() {
                                loosing.set(false);
                                state.set(false);
                                emitter.fire(());
                            }
                        },
                        0,
                    );
                }
            }));
        }
        Rc::new(Self { base, did_focus, did_blur, has_focus })
    }

    pub fn on_did_focus(&self) -> BaseEvent<()> {
        self.did_focus.event()
    }

    pub fn on_did_blur(&self) -> BaseEvent<()> {
        self.did_blur.event()
    }

    /// `refreshState`: recalcula se o nó tem o foco.
    pub fn refresh_state(&self, ui: &Ui, node: NodeId) {
        self.has_focus.set(is_ancestor_of_active_element(ui, node));
    }

    pub fn has_focus(&self) -> bool {
        self.has_focus.get()
    }
}

impl IDisposable for FocusTracker {
    fn dispose(&self) {
        self.base.dispose();
    }
}
