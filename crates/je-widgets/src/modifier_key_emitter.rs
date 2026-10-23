//! Porte do `ModifierKeyEmitter` de `src/vs/base/browser/dom.ts`: o estado das teclas modificadoras
//! (Alt, Ctrl, Meta, Shift) da janela, com um evento a cada mudança. O menu usa para trocar a ação pela
//! alternativa enquanto Alt (ou Shift) está pressionado, e a barra de menus para o Alt focar o menu.
//!
//! Como o DOM virou `je-ui`: a "janela" é a raiz da árvore; `keydown`/`keyup` em captura na janela são os
//! ouvintes de captura da raiz, `mousedown`/`mouseup` no `body` (que zeram `lastKeyPressed`) são os de
//! ponteiro, também na raiz. O original é um singleton do processo; aqui é um singleton por [`Ui`]
//! ([`ModifierKeyEmitter::get_instance`]), marcado como singleton no rastreador de vazamentos.
//!
//! Fica de fora: o `blur` da janela (que zera o estado; o `je-ui` não tem evento de janela) e o
//! `mousemove` com botão pressionado (o evento de ponteiro não traz `buttons`). O `repeat` do Alt é
//! desnecessário: a repetição cai no mesmo ramo que o original descarta.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::actions::AltModeModifiers;
use je_base::event::{Emitter, Event as BaseEvent};
use je_base::key_codes::KeyCode;
use je_base::lifecycle::{Disposable, IDisposable, mark_as_singleton};
use je_ui::{Event, EventKind, Ui};

use crate::common::registered_emitter;

/// `ModifierKey`: a última tecla modificadora apertada ou solta.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModifierKey {
    Alt,
    Ctrl,
    Shift,
    Meta,
}

/// O evento de teclado que causou a mudança, só disponível enquanto os ouvintes são avisados (o
/// `event?: KeyboardEvent` de `IModifierKeyStatus`). `prevent_default` volta para o evento de origem.
#[derive(Debug)]
pub struct ModifierKeyEvent {
    pub key_code: KeyCode,
    default_prevented: Cell<bool>,
}

impl ModifierKeyEvent {
    pub fn prevent_default(&self) {
        self.default_prevented.set(true);
    }
}

/// `IModifierKeyStatus`.
#[derive(Clone, Debug, Default)]
pub struct ModifierKeyStatus {
    pub alt_key: bool,
    pub shift_key: bool,
    pub ctrl_key: bool,
    pub meta_key: bool,
    pub last_key_pressed: Option<ModifierKey>,
    pub last_key_released: Option<ModifierKey>,
    pub event: Option<Rc<ModifierKeyEvent>>,
}

impl ModifierKeyStatus {
    /// `hasModifierKeys(status)`.
    pub fn has_modifier_keys(&self) -> bool {
        self.ctrl_key || self.shift_key || self.alt_key || self.meta_key
    }

    /// Alt e Shift como o `getActionForAltMode` os lê.
    pub fn alt_shift(&self) -> AltModeModifiers {
        AltModeModifiers { alt_key: self.alt_key, shift_key: self.shift_key }
    }

    fn copy_modifiers(&mut self, e: &Event) {
        let m = e.key.map(|k| k.modifiers).unwrap_or_default();
        (self.alt_key, self.ctrl_key, self.meta_key, self.shift_key) = (m.alt, m.ctrl, m.meta, m.shift);
    }
}

thread_local! {
    /// Uma instância por `Ui`, achada pelo endereço da árvore (a instância guarda o `Ui`, então o endereço
    /// não é reaproveitado enquanto ela existe).
    static INSTANCES: RefCell<Vec<(usize, Rc<ModifierKeyEmitter>)>> = const { RefCell::new(Vec::new()) };
}

fn ui_key(ui: &Ui) -> usize {
    std::ptr::from_ref::<je_ui::Tree>(&ui.tree()) as usize
}

pub struct ModifierKeyEmitter {
    base: Rc<Disposable>,
    ui: Ui,
    emitter: Rc<Emitter<ModifierKeyStatus>>,
    key_status: RefCell<ModifierKeyStatus>,
}

impl ModifierKeyEmitter {
    /// `ModifierKeyEmitter.getInstance()`.
    pub fn get_instance(ui: &Ui) -> Rc<Self> {
        let key = ui_key(ui);
        if let Some(found) = INSTANCES.with(|i| i.borrow().iter().find(|(k, _)| *k == key).map(|(_, v)| v.clone())) {
            return found;
        }
        let instance = Self::new(ui);
        INSTANCES.with(|i| i.borrow_mut().push((key, instance.clone())));
        instance
    }

    /// `ModifierKeyEmitter.disposeInstance()`.
    pub fn dispose_instance(ui: &Ui) {
        let key = ui_key(ui);
        let removed = INSTANCES.with(|i| {
            let mut instances = i.borrow_mut();
            instances.iter().position(|(k, _)| *k == key).map(|index| instances.remove(index).1)
        });
        if let Some(instance) = removed {
            instance.dispose();
        }
    }

    fn new(ui: &Ui) -> Rc<Self> {
        let base = Disposable::new();
        mark_as_singleton(base.clone());
        let emitter = registered_emitter::<ModifierKeyStatus>(&base);
        let this = Rc::new(Self { base, ui: ui.clone(), emitter, key_status: RefCell::new(ModifierKeyStatus::default()) });
        let root = ui.root();
        let weak = Rc::downgrade(&this);
        this.base.register(ui.listen(root, EventKind::KeyDown, true, {
            let weak = weak.clone();
            move |e| {
                if let Some(this) = weak.upgrade() {
                    this.on_key_down(e);
                }
            }
        }));
        this.base.register(ui.listen(root, EventKind::KeyUp, true, {
            let weak = weak.clone();
            move |e| {
                if let Some(this) = weak.upgrade() {
                    this.on_key_up(e);
                }
            }
        }));
        for kind in [EventKind::PointerDown, EventKind::PointerUp] {
            let weak = weak.clone();
            this.base.register(ui.listen(root, kind, true, move |_| {
                if let Some(this) = weak.upgrade() {
                    this.key_status.borrow_mut().last_key_pressed = None;
                }
            }));
        }
        this
    }

    fn on_key_down(&self, e: &mut Event) {
        if e.default_prevented() {
            return;
        }
        let Some(key) = e.key else { return };
        let m = key.modifiers;
        let fire = {
            let mut status = self.key_status.borrow_mut();
            if m.alt && !status.alt_key {
                status.last_key_pressed = Some(ModifierKey::Alt);
            } else if m.ctrl && !status.ctrl_key {
                status.last_key_pressed = Some(ModifierKey::Ctrl);
            } else if m.meta && !status.meta_key {
                status.last_key_pressed = Some(ModifierKey::Meta);
            } else if m.shift && !status.shift_key {
                status.last_key_pressed = Some(ModifierKey::Shift);
            } else if key.key_code != KeyCode::Alt {
                status.last_key_pressed = None;
            } else {
                return;
            }
            status.copy_modifiers(e);
            status.last_key_pressed.is_some()
        };
        if fire {
            self.fire_with_event(e);
        }
    }

    fn on_key_up(&self, e: &mut Event) {
        if e.default_prevented() {
            return;
        }
        let m = e.key.map(|k| k.modifiers).unwrap_or_default();
        let fire = {
            let mut status = self.key_status.borrow_mut();
            status.last_key_released = if !m.alt && status.alt_key {
                Some(ModifierKey::Alt)
            } else if !m.ctrl && status.ctrl_key {
                Some(ModifierKey::Ctrl)
            } else if !m.meta && status.meta_key {
                Some(ModifierKey::Meta)
            } else if !m.shift && status.shift_key {
                Some(ModifierKey::Shift)
            } else {
                None
            };
            if status.last_key_pressed != status.last_key_released {
                status.last_key_pressed = None;
            }
            status.copy_modifiers(e);
            status.last_key_released.is_some()
        };
        if fire {
            self.fire_with_event(e);
        }
    }

    /// `fireWithEvent`: o evento só existe enquanto os ouvintes são avisados.
    fn fire_with_event(&self, e: &mut Event) {
        let handle = Rc::new(ModifierKeyEvent { key_code: e.key.map_or(KeyCode::Unknown, |k| k.key_code), default_prevented: Cell::new(false) });
        let status = ModifierKeyStatus { event: Some(handle.clone()), ..self.key_status.borrow().clone() };
        self.emitter.fire(status);
        if handle.default_prevented.get() {
            e.prevent_default();
        }
    }

    /// `keyStatus` (uma cópia: o original devolve o objeto vivo).
    pub fn key_status(&self) -> ModifierKeyStatus {
        self.key_status.borrow().clone()
    }

    /// O evento `onDidChange` do emissor.
    pub fn event(&self) -> BaseEvent<ModifierKeyStatus> {
        self.emitter.event()
    }

    /// `isModifierPressed`.
    pub fn is_modifier_pressed(&self) -> bool {
        self.key_status.borrow().has_modifier_keys()
    }

    /// `resetKeyStatus()`.
    pub fn reset_key_status(&self) {
        *self.key_status.borrow_mut() = ModifierKeyStatus::default();
        let status = self.key_status();
        self.emitter.fire(status);
    }

    pub fn ui(&self) -> &Ui {
        &self.ui
    }
}

impl IDisposable for ModifierKeyEmitter {
    fn dispose(&self) {
        self.base.dispose();
    }
}
