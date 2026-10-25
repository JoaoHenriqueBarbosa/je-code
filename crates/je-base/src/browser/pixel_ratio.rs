//! Porte de `src/vs/base/browser/pixelRatio.ts`: a razão de pixels de cada janela (`PixelRatio.getInstance(window).value`)
//! e o aviso de quando ela muda.
//!
//! Divergências:
//! - **Quem informa o valor é a janela nativa.** O original lê `window.devicePixelRatio` e escuta a media query
//!   `(resolution: Xdppx)` para saber da mudança de monitor ou de zoom (`DevicePixelRatioMonitor`). Aqui o fator de escala
//!   é o que o shell da janela informa a cada mudança, por [`PixelRatioMonitorImpl::set_device_pixel_ratio`]; até a primeira
//!   informação vale `1`, como `devicePixelRatio || 1`. O `DevicePixelRatioMonitor` não existe: a mudança que ele detectava
//!   chega já pronta.
//! - **`backingStorePixelRatio`.** O `<canvas>` do original só serve para ler o `backingStorePixelRatio` (sempre 1 nos
//!   navegadores atuais, e inexistente na pintura nativa), então o valor é o próprio fator de escala.
//! - `IPixelRatioMonitor` é o próprio [`PixelRatioMonitorImpl`] (`value` e `on_did_change`), a única implementação.
//! - O `mapWindowIdToPixelRatioMonitor` é um estado da thread, indexado pelo id da janela (`None` para a janela sem id).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use crate::browser::dom::{CodeWindow, get_window_id, on_did_unregister_window};
use crate::event::{Emitter, Event};
use crate::lifecycle::{Disposable, DisposableRef, IDisposable, mark_as_singleton};

/// `window.devicePixelRatio || 1`: um fator zero ou inválido vale um.
fn normalize(device_pixel_ratio: f64) -> f64 {
    if device_pixel_ratio == 0.0 || device_pixel_ratio.is_nan() { 1.0 } else { device_pixel_ratio }
}

/// `PixelRatioMonitorImpl` (e `IPixelRatioMonitor`).
pub struct PixelRatioMonitorImpl {
    base: Rc<Disposable>,
    on_did_change: Rc<Emitter<f64>>,
    value: Cell<f64>,
}

impl PixelRatioMonitorImpl {
    fn new(device_pixel_ratio: f64) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change = Emitter::<f64>::new();
        base.register(on_did_change.clone());
        Rc::new(Self { base, on_did_change, value: Cell::new(normalize(device_pixel_ratio)) })
    }

    /// `value`.
    pub fn value(&self) -> f64 {
        self.value.get()
    }

    /// `onDidChange`.
    pub fn on_did_change(&self) -> Event<f64> {
        self.on_did_change.event()
    }

    /// O shell da janela informa o fator de escala atual. Só dispara `onDidChange` quando o valor mudou (a media query do
    /// original também só avisa quando a resolução deixa de casar).
    pub fn set_device_pixel_ratio(&self, device_pixel_ratio: f64) {
        let value = normalize(device_pixel_ratio);
        if value != self.value.get() {
            self.value.set(value);
            self.on_did_change.fire(value);
        }
    }
}

impl IDisposable for PixelRatioMonitorImpl {
    fn dispose(&self) {
        self.base.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "PixelRatioMonitorImpl"
    }
}

thread_local! {
    static MONITORS: RefCell<HashMap<Option<u32>, Rc<PixelRatioMonitorImpl>>> = RefCell::new(HashMap::new());
}

/// `PixelRatio` (`PixelRatioMonitorFacade`).
pub struct PixelRatio;

impl PixelRatio {
    /// `getInstance(targetWindow)`: o monitor da janela, criado no primeiro pedido e descartado quando a janela é
    /// desregistrada.
    pub fn get_instance(target_window: &CodeWindow) -> Rc<PixelRatioMonitorImpl> {
        let target_window_id = get_window_id(target_window);
        if let Some(monitor) = MONITORS.with(|monitors| monitors.borrow().get(&target_window_id).cloned()) {
            return monitor;
        }

        let monitor = mark_as_singleton(PixelRatioMonitorImpl::new(1.0));
        MONITORS.with(|monitors| monitors.borrow_mut().insert(target_window_id, monitor.clone()));

        let unregister_listener: Rc<RefCell<Option<DisposableRef>>> = Rc::default();
        let listener = {
            let (monitor, unregister_listener) = (monitor.clone(), unregister_listener.clone());
            on_did_unregister_window(move |unregistered_window| {
                if get_window_id(unregistered_window) == target_window_id {
                    if let Some(listener) = unregister_listener.borrow_mut().take() {
                        listener.dispose();
                    }
                    monitor.dispose();
                    MONITORS.with(|monitors| monitors.borrow_mut().remove(&target_window_id));
                }
            })
        };
        *unregister_listener.borrow_mut() = Some(mark_as_singleton(listener));
        monitor
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use super::*;

    #[test]
    fn the_value_starts_at_one_and_follows_the_window() {
        let window = CodeWindow::new();
        let monitor = PixelRatio::get_instance(&window);
        assert_eq!(monitor.value(), 1.0);

        let seen: Rc<RefCell<Vec<f64>>> = Rc::default();
        let sink = seen.clone();
        let subscription = monitor.on_did_change().listen(move |value| sink.borrow_mut().push(*value));
        monitor.set_device_pixel_ratio(2.0);
        monitor.set_device_pixel_ratio(2.0);
        monitor.set_device_pixel_ratio(0.0);
        assert_eq!(*seen.borrow(), vec![2.0, 1.0]);
        subscription.dispose();
    }

    #[test]
    fn the_instance_is_shared_per_window() {
        let window = CodeWindow::new();
        assert!(Rc::ptr_eq(&PixelRatio::get_instance(&window), &PixelRatio::get_instance(&window)));
    }
}
