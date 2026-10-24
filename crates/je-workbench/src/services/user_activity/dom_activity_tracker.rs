//! Porte de `src/vs/workbench/services/userActivity/browser/domActivityTracker.ts`: o rastreador que marca o usuário
//! como ativo enquanto chegam eventos de entrada nas janelas.
//!
//! Como o DOM foi abstraído:
//! - Não há DOM. `window.document` (onde o original escuta `touchstart`, `mousedown` e `keydown` com
//!   `{ passive: true, capture: true }`) é o trait [`InputWindow`]: `add_listener(kind, listener)` registra o
//!   ouvinte (passivo e na fase de captura, como o original) e devolve o disposable que o remove. Os três tipos de
//!   evento são [`InputEventKind`].
//! - `dom.onDidRegisterWindow` e `mainWindow` são o trait [`InputWindowRegistry`]. O `{ window, disposables }` do
//!   evento é [`RegisteredWindow`]: os ouvintes de uma janela entram nos `disposables` dela e saem junto com ela.
//! - `Event.runAndSubscribe(onDidRegisterWindow, handler, { window: mainWindow, disposables: this._store })` roda o
//!   tratador já com a janela principal (os `disposables` são o store do próprio rastreador) e o assina no evento.
//! - `dom.WindowIntervalTimer` (um `setInterval` da janela de destino) é o [`IntervalTimer`] do runtime: o
//!   `targetWindow` do `cancelAndSet` só escolhe qual `setInterval` usar, e há um relógio só. Por isso o tratador não
//!   carrega a janela, só o tipo do evento importa.
//!
//! Divergência de tipos: `activeMutex.value = ...` é `MutableDisposable::set(Some(...))` e `Disposable` é o campo `base`.

use std::cell::Cell;
use std::rc::Rc;

use je_base::async_::IntervalTimer;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, DisposableRef, DisposableStore, IDisposable, MutableDisposable};

use super::user_activity_service::IUserActivityService;

/// Este tracker usa um intervalo de tempo e confere se houve atividade nele. Uma abordagem ingênua seria um debounce a
/// cada evento, mas isso tem custo de agendamento. Em vez disso, o rastreador conta quantos intervalos passaram desde
/// a última atividade.
///
/// Se passarem mais que `MIN_INTERVALS_WITHOUT_ACTIVITY`, o usuário é dado como inativo. Logo o tempo máximo até
/// detectar um usuário inativo é `CHECK_INTERVAL * (MIN_INTERVALS_WITHOUT_ACTIVITY + 1)`.
const CHECK_INTERVAL: u64 = 30_000;

/// Ver [`CHECK_INTERVAL`].
const MIN_INTERVALS_WITHOUT_ACTIVITY: u32 = 2;

/// Os eventos de entrada que contam como atividade (`touchstart`, `mousedown` e `keydown`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputEventKind {
    TouchStart,
    MouseDown,
    KeyDown,
}

impl InputEventKind {
    const ALL: [InputEventKind; 3] = [InputEventKind::TouchStart, InputEventKind::MouseDown, InputEventKind::KeyDown];
}

/// O `window.document` de uma janela, reduzido ao que o rastreador usa.
pub trait InputWindow {
    /// `addDisposableListener(window.document, kind, listener, { passive: true, capture: true })`.
    fn add_listener(&self, kind: InputEventKind, listener: Rc<dyn Fn()>) -> DisposableRef;
}

/// O `{ window, disposables }` de `dom.onDidRegisterWindow`.
#[derive(Clone)]
pub struct RegisteredWindow {
    pub window: Rc<dyn InputWindow>,
    pub disposables: Rc<DisposableStore>,
}

/// `mainWindow` e `dom.onDidRegisterWindow`.
pub trait InputWindowRegistry {
    fn main_window(&self) -> Rc<dyn InputWindow>;
    fn on_did_register_window(&self) -> Event<RegisteredWindow>;
}

/// Um registro com uma só janela, a principal, que nunca registra outra. Serve a quem não tem janelas auxiliares.
pub struct SingleWindowRegistry {
    window: Rc<dyn InputWindow>,
    emitter: Rc<Emitter<RegisteredWindow>>,
}

impl SingleWindowRegistry {
    pub fn new(window: Rc<dyn InputWindow>) -> Rc<Self> {
        Rc::new(Self { window, emitter: Emitter::new() })
    }
}

impl InputWindowRegistry for SingleWindowRegistry {
    fn main_window(&self) -> Rc<dyn InputWindow> {
        self.window.clone()
    }

    fn on_did_register_window(&self) -> Event<RegisteredWindow> {
        self.emitter.event()
    }
}

impl IDisposable for SingleWindowRegistry {
    fn dispose(&self) {
        self.emitter.dispose();
    }
}

pub struct DomActivityTracker {
    base: Rc<Disposable>,
}

impl DomActivityTracker {
    pub fn new(user_activity_service: Rc<dyn IUserActivityService>, windows: Rc<dyn InputWindowRegistry>) -> Rc<Self> {
        let base = Disposable::new();

        let intervals_without_activity = Rc::new(Cell::new(MIN_INTERVALS_WITHOUT_ACTIVITY));
        let interval_timer = IntervalTimer::new();
        base.register(interval_timer.clone());
        let active_mutex: Rc<MutableDisposable<dyn IDisposable>> = MutableDisposable::new();
        base.register(active_mutex.clone());
        active_mutex.set(Some(user_activity_service.mark_active(None)));

        let on_interval = {
            let (intervals_without_activity, active_mutex, interval_timer) =
                (intervals_without_activity.clone(), active_mutex.clone(), interval_timer.clone());
            move || {
                intervals_without_activity.set(intervals_without_activity.get() + 1);
                if intervals_without_activity.get() == MIN_INTERVALS_WITHOUT_ACTIVITY {
                    active_mutex.clear();
                    interval_timer.cancel();
                }
            }
        };

        let on_activity: Rc<dyn Fn()> = {
            let (intervals_without_activity, active_mutex, interval_timer, user_activity_service) =
                (intervals_without_activity.clone(), active_mutex.clone(), interval_timer.clone(), user_activity_service.clone());
            let on_interval = Rc::new(on_interval);
            Rc::new(move || {
                // if was inactive, they've now returned
                if intervals_without_activity.get() == MIN_INTERVALS_WITHOUT_ACTIVITY {
                    active_mutex.set(Some(user_activity_service.mark_active(None)));
                    let on_interval = on_interval.clone();
                    interval_timer.cancel_and_set(move || on_interval(), CHECK_INTERVAL);
                }

                intervals_without_activity.set(0);
            })
        };

        let register_window = {
            let on_activity = on_activity.clone();
            move |registered: &RegisteredWindow| {
                for kind in InputEventKind::ALL {
                    registered.disposables.add(registered.window.add_listener(kind, on_activity.clone()));
                }
            }
        };
        let register_window = Rc::new(register_window);

        // Event.runAndSubscribe(onDidRegisterWindow, handler, { window: mainWindow, disposables: this._store })
        register_window(&RegisteredWindow { window: windows.main_window(), disposables: base_store(&base) });
        let subscription = windows.on_did_register_window().listen({
            let register_window = register_window.clone();
            move |registered| register_window(&registered)
        });
        base.register(subscription);

        on_activity();

        Rc::new(Self { base })
    }
}

/// O `this._store` do `Disposable`: um store que o `base` descarta com ele.
fn base_store(base: &Rc<Disposable>) -> Rc<DisposableStore> {
    let store = DisposableStore::new();
    base.register(store.clone());
    store
}

impl IDisposable for DomActivityTracker {
    fn dispose(&self) {
        self.base.dispose();
    }
}
