//! Porte de `src/vs/base/browser/globalPointerMoveMonitor.ts`: acompanha o ponteiro mesmo fora do
//! elemento onde o arraste começou, até o botão subir.
//!
//! O original captura o ponteiro no elemento inicial (`setPointerCapture`) e, se o navegador recusar,
//! escuta a janela. Aqui a captura é [`Ui::set_pointer_capture`], que nunca falha, então os ouvintes de
//! movimento e de soltar ficam sempre no elemento inicial. O `PointerEvent.buttons` não existe no
//! [`PointerData`] (só o botão que mudou), então o ramo "o estado dos botões mudou, pare" fica a cargo do
//! `PointerUp`, que o [`Ui`] sempre entrega ao alvo capturado.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use je_base::lifecycle::{DisposableStore, IDisposable, to_disposable};
use je_ui::{EventKind, NodeId, PointerData, Ui};

/// O `IPointerMoveCallback`.
pub type PointerMoveCallback = Rc<dyn Fn(&PointerData)>;
/// O `IOnStopCallback`.
pub type OnStopCallback = Rc<dyn Fn()>;

struct Monitoring {
    on_stop: OnStopCallback,
}

pub struct GlobalPointerMoveMonitor {
    ui: Ui,
    hooks: Rc<DisposableStore>,
    monitoring: RefCell<Option<Monitoring>>,
    weak_self: Weak<GlobalPointerMoveMonitor>,
}

impl GlobalPointerMoveMonitor {
    pub fn new(ui: &Ui) -> Rc<Self> {
        Rc::new_cyclic(|weak| Self { ui: ui.clone(), hooks: DisposableStore::new(), monitoring: RefCell::new(None), weak_self: weak.clone() })
    }

    pub fn stop_monitoring(&self, invoke_stop_callback: bool) {
        let Some(monitoring) = self.monitoring.borrow_mut().take() else {
            // não está monitorando
            return;
        };
        // desengancha
        self.hooks.clear();
        if invoke_stop_callback {
            (monitoring.on_stop)();
        }
    }

    pub fn is_monitoring(&self) -> bool {
        self.monitoring.borrow().is_some()
    }

    /// `startMonitoring(initialElement, pointerId, initialButtons, pointerMoveCallback, onStopCallback)`;
    /// `pointerId` e `initialButtons` não existem aqui (um só ponteiro).
    pub fn start_monitoring(&self, initial_element: NodeId, pointer_move_callback: impl Fn(&PointerData) + 'static, on_stop_callback: impl Fn() + 'static) {
        if self.is_monitoring() {
            self.stop_monitoring(false);
        }
        *self.monitoring.borrow_mut() = Some(Monitoring { on_stop: Rc::new(on_stop_callback) });

        self.ui.set_pointer_capture(initial_element);
        let ui = self.ui.clone();
        self.hooks.add(to_disposable(move || {
            if ui.pointer_captured_by() == Some(initial_element) {
                ui.release_pointer_capture();
            }
        }));

        self.hooks.add(self.ui.listen(initial_element, EventKind::PointerMove, false, move |e| {
            e.prevent_default();
            if let Some(pointer) = e.pointer {
                pointer_move_callback(&pointer);
            }
        }));
        let weak = self.weak_self.clone();
        self.hooks.add(self.ui.listen(initial_element, EventKind::PointerUp, false, move |_| {
            if let Some(this) = weak.upgrade() {
                this.stop_monitoring(true);
            }
        }));
    }
}

impl IDisposable for GlobalPointerMoveMonitor {
    fn dispose(&self) {
        self.stop_monitoring(false);
        self.hooks.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "GlobalPointerMoveMonitor"
    }
}
