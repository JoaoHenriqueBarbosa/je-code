//! Porte de `src/vs/platform/lifecycle/node/sharedProcessLifecycleService.ts`: o contrato
//! `ISharedProcessLifecycleService` e o `SharedProcessLifecycleService`, que dispara o `onWillShutdown` do shared process.
//!
//! Divergências:
//! - `ISharedProcessLifecycleService` é um trait usado como `Rc<dyn ISharedProcessLifecycleService>` (sem
//!   `createDecorator` nem `_serviceBrand`); `Event<void>` é `Event<()>`.
//! - `extends Disposable` é um [`Disposable`] guardado em `base`, que registra o emissor.

use std::rc::Rc;

use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, IDisposable};

use crate::log::LogServiceRef;

/// `ISharedProcessLifecycleService`.
pub trait ISharedProcessLifecycleService {
    /// An event for when the application will shutdown.
    fn on_will_shutdown(&self) -> Event<()>;
}

/// `SharedProcessLifecycleService`.
pub struct SharedProcessLifecycleService {
    base: Rc<Disposable>,
    on_will_shutdown: Rc<Emitter<()>>,
    log_service: LogServiceRef,
}

impl SharedProcessLifecycleService {
    pub fn new(log_service: LogServiceRef) -> Rc<Self> {
        let base = Disposable::new();
        let on_will_shutdown: Rc<Emitter<()>> = Emitter::new();
        base.register(on_will_shutdown.clone());
        Rc::new(Self { base, on_will_shutdown, log_service })
    }

    /// `fireOnWillShutdown()`.
    pub fn fire_on_will_shutdown(&self) {
        self.log_service.trace("Lifecycle#onWillShutdown.fire()");

        self.on_will_shutdown.fire(());
    }
}

impl ISharedProcessLifecycleService for SharedProcessLifecycleService {
    fn on_will_shutdown(&self) -> Event<()> {
        self.on_will_shutdown.event()
    }
}

impl IDisposable for SharedProcessLifecycleService {
    fn dispose(&self) {
        self.base.dispose();
    }
}
