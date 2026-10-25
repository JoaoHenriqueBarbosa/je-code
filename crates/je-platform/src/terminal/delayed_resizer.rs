//! Porte da classe `DelayedResizer` de `src/vs/platform/terminal/node/terminalProcess.ts`: guarda o último pedido de
//! redimensionamento e o dispara depois de 1 s, porque o ConPTY ignora redimensionamentos muito cedo (só o
//! `Git\bin\bash.exe` aberto com `cols` e `rows` zero).
//!
//! `extends Disposable` é um [`Disposable`] por composição; `setTimeout` e `clearTimeout` são os do `je_base::runtime`, e o
//! timer é cancelado no `dispose`.

use std::cell::Cell;
use std::rc::{Rc, Weak};

use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, IDisposable, to_disposable};
use je_base::runtime::{self, TimerId};

/// O atraso do disparo (`setTimeout(..., 1000)`).
const TRIGGER_DELAY_MS: u64 = 1000;

/// `{ rows?: number; cols?: number }`: o que o `onTrigger` entrega.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DelayedResizeDimensions {
    pub rows: Option<i32>,
    pub cols: Option<i32>,
}

/// Tracks the latest resize event to be trigger at a later point.
pub struct DelayedResizer {
    base: Rc<Disposable>,
    rows: Cell<Option<i32>>,
    cols: Cell<Option<i32>>,
    timeout: Cell<Option<TimerId>>,
    on_trigger: Rc<Emitter<DelayedResizeDimensions>>,
}

impl DelayedResizer {
    pub fn new() -> Rc<Self> {
        let base = Disposable::new();
        let on_trigger: Rc<Emitter<DelayedResizeDimensions>> = Emitter::new();
        base.register(on_trigger.clone());
        let this = Rc::new_cyclic(|me: &Weak<Self>| {
            let weak = me.clone();
            let timer = runtime::set_timeout(
                move || {
                    if let Some(this) = weak.upgrade() {
                        this.timeout.set(None);
                        this.on_trigger.fire(DelayedResizeDimensions { rows: this.rows.get(), cols: this.cols.get() });
                    }
                },
                TRIGGER_DELAY_MS,
            );
            Self { base, rows: Cell::new(None), cols: Cell::new(None), timeout: Cell::new(Some(timer)), on_trigger }
        });
        let weak = Rc::downgrade(&this);
        this.base.register(to_disposable(move || {
            if let Some(this) = weak.upgrade() {
                if let Some(timer) = this.timeout.take() {
                    runtime::clear_timeout(timer);
                }
            }
        }));
        this
    }

    /// `rows = ...; cols = ...`: guarda o pedido mais recente.
    pub fn set_dimensions(&self, cols: i32, rows: i32) {
        self.cols.set(Some(cols));
        self.rows.set(Some(rows));
    }

    /// `onTrigger`.
    pub fn on_trigger(&self) -> Event<DelayedResizeDimensions> {
        self.on_trigger.event()
    }
}

impl IDisposable for DelayedResizer {
    fn dispose(&self) {
        self.base.dispose();
    }
}
