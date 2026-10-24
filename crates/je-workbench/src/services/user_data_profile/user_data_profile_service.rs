//! Porte de `src/vs/workbench/services/userDataProfile/common/userDataProfileService.ts`: o `UserDataProfileService`, que
//! guarda o perfil corrente da janela e avisa quando ele troca.
//!
//! Notas de tradução:
//! - **Injeção.** O construtor `constructor(currentProfile)` não tem parâmetros decorados: `Args` do [`Injectable`] é o
//!   perfil corrente e `Deps` é vazio, e `create` é o construtor (`new UserDataProfileService(profile)`).
//! - **`equals`.** `equals(this._currentProfile, userDataProfile)` (`objects.ts`, comparação profunda) é a igualdade do
//!   [`UserDataProfile`], que compara todos os campos.
//! - **`updateCurrentProfile`.** O corpo `async` roda até o primeiro `await` de forma síncrona (a troca do perfil e o
//!   disparo do evento), e o que sobra é a espera das promessas que os ouvintes entregaram a `join`
//!   (`Promises.settled(joiners)`), como o [`async_fn`] faz.

use std::cell::RefCell;
use std::rc::Rc;

use je_base::async_::{async_fn, promises};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::promise::Promise;
use je_instantiation::{Injectable, service_impl};
use je_platform::user_data_profile::user_data_profile::UserDataProfile;

use super::user_data_profile::{DidChangeUserDataProfileEvent, IUserDataProfileService};

/// `UserDataProfileService`.
pub struct UserDataProfileService {
    base: Rc<Disposable>,
    on_did_change_current_profile: Rc<Emitter<DidChangeUserDataProfileEvent>>,
    current_profile: RefCell<UserDataProfile>,
}

impl Injectable for UserDataProfileService {
    type Args = UserDataProfile;
    type Deps = ();

    fn create(current_profile: UserDataProfile, _: ()) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change_current_profile = Emitter::new();
        base.register(on_did_change_current_profile.clone());
        Rc::new(Self { base, on_did_change_current_profile, current_profile: RefCell::new(current_profile) })
    }
}

service_impl!(UserDataProfileService => dyn IUserDataProfileService, disposable);

impl IDisposable for UserDataProfileService {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl IUserDataProfileService for UserDataProfileService {
    fn current_profile(&self) -> UserDataProfile {
        self.current_profile.borrow().clone()
    }

    fn on_did_change_current_profile(&self) -> Event<DidChangeUserDataProfileEvent> {
        self.on_did_change_current_profile.event()
    }

    fn update_current_profile(&self, user_data_profile: &UserDataProfile) -> Promise<()> {
        if *self.current_profile.borrow() == *user_data_profile {
            return Promise::resolved(());
        }
        let previous = self.current_profile.replace(user_data_profile.clone());
        let event = DidChangeUserDataProfileEvent::new(previous, user_data_profile.clone());
        self.on_did_change_current_profile.fire(event.clone());
        let joiners = event.joined();
        async_fn(async move {
            promises::settled(joiners).await?;
            Ok(())
        })
    }
}
