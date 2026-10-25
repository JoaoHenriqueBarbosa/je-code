//! `TestLifecycleMainService` de `src/vs/platform/test/electron-main/workbenchTestServices.ts`: o
//! `ILifecycleMainService` de teste, que já está na fase `Ready` (o `when` assenta de uma vez) e deixa o teste disparar o
//! `onWillShutdown` e esperar o que os ouvintes juntaram (`fireOnWillShutdown`).
//!
//! Só os membros do contrato [`LifecycleMainService`] existem; o resto do original (`registerWindow`, `reload`...)
//! não tem como ser chamado por quem usa o contrato portado. O `quit` não é vetado.

use std::rc::Rc;

use je_base::event::{Emitter, Event};
use je_base::promise::Promise;
use je_platform::user_data_profile::user_data_profiles_handler::{
    LifecycleMainPhase, LifecycleMainService, ProfilesWindow, ShutdownEvent, ShutdownReason, WindowLoadEvent,
};

pub struct TestLifecycleMainService {
    on_will_shutdown: Rc<Emitter<ShutdownEvent>>,
}

impl TestLifecycleMainService {
    pub fn new() -> Rc<Self> {
        Rc::new(Self { on_will_shutdown: Emitter::new() })
    }

    /// `fireOnWillShutdown()`: dispara o evento e espera todas as promessas que os ouvintes juntaram (`Promises.settled`).
    pub fn fire_on_will_shutdown(&self) -> Promise<()> {
        let event = ShutdownEvent::new(ShutdownReason::Quit);
        self.on_will_shutdown.fire(event.clone());

        Promise::all_settled(event.joiners().into_iter().map(|(_, promise)| promise).collect()).then(|_| ())
    }
}

impl LifecycleMainService for TestLifecycleMainService {
    fn on_will_load_window(&self) -> Event<WindowLoadEvent> {
        Event::none()
    }

    fn on_before_close_window(&self) -> Event<Rc<dyn ProfilesWindow>> {
        Event::none()
    }

    fn on_will_shutdown(&self) -> Event<ShutdownEvent> {
        self.on_will_shutdown.event()
    }

    fn on_before_shutdown(&self) -> Event<()> {
        Event::none()
    }

    fn quit_requested(&self) -> bool {
        false
    }

    fn was_restarted(&self) -> bool {
        false
    }

    fn when(&self, _phase: LifecycleMainPhase) -> Promise<()> {
        Promise::resolved(())
    }

    fn quit(&self, _will_restart: bool) -> Promise<bool> {
        Promise::resolved(false)
    }
}
