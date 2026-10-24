//! Porte de `src/vs/platform/userDataSync/common/userDataSyncAccount.ts`: a conta com que o Settings Sync fala
//! com o servidor, e o que fazer quando o servidor recusa o token.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::dynamic::Dynamic;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::promise::Promise;
use je_instantiation::service_id;

use super::user_data_sync::{IUserDataSyncLogService, IUserDataSyncStoreService, UserDataSyncErrorCode};
use crate::log::{LogArg};

/// `IUserDataSyncAccount`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IUserDataSyncAccount {
    pub authentication_provider_id: String,
    pub token: String,
}

/// `IUserDataSyncAccountService`.
pub trait IUserDataSyncAccountService {
    /// `onTokenFailed`: o valor é se deve desistir (`bail out`).
    fn on_token_failed(&self) -> Event<bool>;
    fn account(&self) -> Option<IUserDataSyncAccount>;
    fn on_did_change_account(&self) -> Event<Option<IUserDataSyncAccount>>;
    fn update_account(&self, account: Option<IUserDataSyncAccount>) -> Promise<()>;
}

service_id!(pub I_USER_DATA_SYNC_ACCOUNT_SERVICE: dyn IUserDataSyncAccountService = "IUserDataSyncAccountService");

/// `UserDataSyncAccountService`.
pub struct UserDataSyncAccountService {
    base: Rc<Disposable>,
    user_data_sync_store_service: Rc<dyn IUserDataSyncStoreService>,
    log_service: IUserDataSyncLogService,
    account: RefCell<Option<IUserDataSyncAccount>>,
    on_did_change_account: Rc<Emitter<Option<IUserDataSyncAccount>>>,
    on_token_failed: Rc<Emitter<bool>>,
    was_token_failed: Cell<bool>,
}

impl UserDataSyncAccountService {
    pub fn new(user_data_sync_store_service: Rc<dyn IUserDataSyncStoreService>, log_service: IUserDataSyncLogService) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change_account = Emitter::new();
        base.register(on_did_change_account.clone());
        let on_token_failed = Emitter::new();
        base.register(on_token_failed.clone());

        let this = Rc::new(Self { base, user_data_sync_store_service, log_service, account: RefCell::new(None), on_did_change_account, on_token_failed, was_token_failed: Cell::new(false) });

        let weak = Rc::downgrade(&this);
        this.base.register(this.user_data_sync_store_service.on_token_failed().listen(move |code| {
            let Some(this) = weak.upgrade() else { return };
            let provider = this.account().map_or(Dynamic::Undefined, |account| Dynamic::string(&account.authentication_provider_id));
            this.log_service.info_args("Settings Sync auth token failed", &[LogArg::from(provider), LogArg::from(Dynamic::Bool(this.was_token_failed.get())), LogArg::from(code.as_str())]);
            let _ = this.update_account(None);
            if *code == UserDataSyncErrorCode::Forbidden {
                this.on_token_failed.fire(true /* bail out immediately */);
            } else {
                this.on_token_failed.fire(this.was_token_failed.get() /* bail out if token failed before */);
            }
            this.was_token_failed.set(true);
        }));
        let weak = Rc::downgrade(&this);
        this.base.register(this.user_data_sync_store_service.on_token_succeed().listen(move |_| {
            if let Some(this) = weak.upgrade() {
                this.was_token_failed.set(false);
            }
        }));
        this
    }
}

impl IDisposable for UserDataSyncAccountService {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl IUserDataSyncAccountService for UserDataSyncAccountService {
    fn on_token_failed(&self) -> Event<bool> {
        self.on_token_failed.event()
    }

    fn account(&self) -> Option<IUserDataSyncAccount> {
        self.account.borrow().clone()
    }

    fn on_did_change_account(&self) -> Event<Option<IUserDataSyncAccount>> {
        self.on_did_change_account.event()
    }

    /// A conta muda se o token ou o provedor de autenticação mudam (ou se ela passa a existir ou deixa de existir).
    fn update_account(&self, account: Option<IUserDataSyncAccount>) -> Promise<()> {
        if account != *self.account.borrow() {
            *self.account.borrow_mut() = account.clone();
            if let Some(current) = &account {
                self.user_data_sync_store_service.set_auth_token(&current.token, &current.authentication_provider_id);
            }
            self.on_did_change_account.fire(account);
        }
        Promise::resolved(())
    }
}
