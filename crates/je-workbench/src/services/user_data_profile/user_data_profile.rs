//! Porte parcial de `src/vs/workbench/services/userDataProfile/common/userDataProfile.ts`: só o
//! `IUserDataProfileService` (o perfil corrente da janela) e o evento de troca. O resto do arquivo
//! (`IUserDataProfileManagementService`, os templates e as constantes de contexto) entra com os serviços que o usam.
//!
//! Divergências:
//! - `currentProfile` e `onDidChangeCurrentProfile` são métodos; o perfil devolvido é uma cópia (o
//!   [`UserDataProfile`] é um valor).
//! - `DidChangeUserDataProfileEvent.join(promise)` guarda as promessas para quem dispara o evento esperar por elas
//!   (como o [`WillChangeProfileEvent`](je_platform::user_data_profile::user_data_profile::WillChangeProfileEvent)
//!   da plataforma).

use std::cell::RefCell;
use std::rc::Rc;

use je_base::event::Event;
use je_base::promise::Promise;
use je_instantiation::service_id;
use je_platform::user_data_profile::user_data_profile::UserDataProfile;

/// `DidChangeUserDataProfileEvent`.
#[derive(Clone)]
pub struct DidChangeUserDataProfileEvent {
    pub previous: UserDataProfile,
    pub profile: UserDataProfile,
    joiners: Rc<RefCell<Vec<Promise<()>>>>,
}

impl DidChangeUserDataProfileEvent {
    pub fn new(previous: UserDataProfile, profile: UserDataProfile) -> Self {
        Self { previous, profile, joiners: Rc::new(RefCell::new(Vec::new())) }
    }

    pub fn join(&self, promise: Promise<()>) {
        self.joiners.borrow_mut().push(promise);
    }

    /// As promessas entregues a `join` até agora.
    pub fn joined(&self) -> Vec<Promise<()>> {
        self.joiners.borrow().clone()
    }
}

/// `IUserDataProfileService`.
pub trait IUserDataProfileService {
    fn current_profile(&self) -> UserDataProfile;
    fn on_did_change_current_profile(&self) -> Event<DidChangeUserDataProfileEvent>;
    fn update_current_profile(&self, current_profile: &UserDataProfile) -> Promise<()>;
}

service_id!(pub I_USER_DATA_PROFILE_SERVICE: dyn IUserDataProfileService = "IUserDataProfileService");
