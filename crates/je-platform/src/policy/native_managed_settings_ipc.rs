//! Porte de `src/vs/platform/policy/common/nativeManagedSettingsIpc.ts`: o canal de IPC do serviço de
//! configurações gerenciadas nativas (o lado do servidor, [`NativeManagedSettingsChannel`], e o cliente,
//! [`NativeManagedSettingsChannelClient`]).
//!
//! Decisões de tradução:
//! - Os valores que atravessam o canal são [`Marshalled`]: as configurações pelas conversões de
//!   `file_managed_settings_ipc` e as definições de política pelas de `policy_ipc`.
//! - `IServerChannel.call` com um comando desconhecido lança (`throw`), como o original; um argumento de
//!   `updatePolicyDefinitions` que não é um dicionário de definições válidas rejeita a promessa.
//! - `updateManagedSettings` do cliente é o `update` do [`Snapshot`] que o cliente de arquivo também usa (guarda,
//!   igualdade e evento são os mesmos), e `initializeFromChannel` é o `update_unless_received` dele.
//! - **`initialize()`** é um método do trait [`INativeManagedSettingsService`], que só tem `&self`; o cliente
//!   guarda um `Weak` de si mesmo para obter o `Rc` que as funções assíncronas seguram.
//! - `getErrorMessage(error)` do aviso de falha é a mensagem do erro.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use je_base::async_::async_fn;
use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::errors::{self, Error};
use je_base::event::Event;
use je_base::lifecycle::{Disposable, DisposableStore, IDisposable};
use je_base::marshalling::Marshalled;
use je_base::parts::ipc::{IChannel, IServerChannel};
use je_base::promise::Promise;

use super::copilot_managed_settings::{INativeManagedSettingsService, ManagedSettingsData};
use super::file_managed_settings_ipc::{Snapshot, managed_settings_from_marshalled, managed_settings_to_marshalled};
use super::policy::PolicyDefinitions;
use super::policy_ipc::{policy_definitions_from_marshalled, policy_definitions_to_marshalled};
use crate::log::{LogArg, LogMessage, LogServiceRef};

/// `NativeManagedSettingsChannel`.
pub struct NativeManagedSettingsChannel {
    service: Rc<dyn INativeManagedSettingsService>,
    disposables: Rc<DisposableStore>,
}

impl NativeManagedSettingsChannel {
    pub fn new(service: Rc<dyn INativeManagedSettingsService>) -> Self {
        Self { service, disposables: DisposableStore::new() }
    }
}

impl IServerChannel for NativeManagedSettingsChannel {
    fn listen(&self, _ctx: &Marshalled, event: &str, _arg: Marshalled) -> Event<Marshalled> {
        match event {
            "onDidChangeManagedSettings" => self.service.on_did_change_managed_settings().map(managed_settings_to_marshalled, None),
            _ => errors::throw(Error::new(format!("Event not found: {event}"))),
        }
    }

    fn call(&self, _ctx: &Marshalled, command: &str, arg: Marshalled, _cancellation_token: CancellationToken) -> Promise<Marshalled> {
        match command {
            "getManagedSettings" => self.service.initialize().then(|managed_settings| managed_settings_to_marshalled(&managed_settings)),
            "updatePolicyDefinitions" => match policy_definitions_from_marshalled(&arg) {
                Ok(definitions) => self.service.update_policy_definitions(definitions).then(|managed_settings| managed_settings_to_marshalled(&managed_settings)),
                Err(error) => Promise::rejected(error),
            },
            _ => errors::throw(Error::new(format!("Call not found: {command}"))),
        }
    }
}

impl IDisposable for NativeManagedSettingsChannel {
    fn dispose(&self) {
        self.disposables.dispose();
    }
}

/// `NativeManagedSettingsChannelClient`.
pub struct NativeManagedSettingsChannelClient {
    me: Weak<Self>,
    base: Rc<Disposable>,
    channel: Rc<dyn IChannel>,
    log_service: LogServiceRef,
    managed: Rc<Snapshot<ManagedSettingsData>>,
    initialization_promise: RefCell<Option<Promise<()>>>,
}

impl NativeManagedSettingsChannelClient {
    pub fn new(channel: Rc<dyn IChannel>, log_service: LogServiceRef) -> Rc<Self> {
        let base = Disposable::new();
        let managed = Snapshot::new(ManagedSettingsData::new(), |a, b| a == b);
        base.register(managed.on_did_change.clone());

        let listener = managed.clone();
        base.register(channel.listen("onDidChangeManagedSettings", Marshalled::Undefined).listen(move |value| listener.update(managed_settings_from_marshalled(value), true)));

        let this = Rc::new_cyclic(|me| Self { me: me.clone(), base, channel, log_service, managed, initialization_promise: RefCell::new(None) });
        this.initialize_in_background();
        this
    }

    /// `void this.initializeInBackground()`: a falha só vira aviso.
    fn initialize_in_background(self: &Rc<Self>) {
        let this = self.clone();
        let _ = self.initialize().then_result(move |result| {
            if let Err(error) = result {
                this.log_service.warn_args(
                    &LogMessage::from("NativeManagedSettingsChannelClient#initialize - Failed to initialize native managed settings"),
                    &[LogArg::Value(Dynamic::string(&error.message))],
                );
            }
            Ok(())
        });
    }
}

impl INativeManagedSettingsService for NativeManagedSettingsChannelClient {
    fn managed_settings(&self) -> ManagedSettingsData {
        self.managed.get()
    }

    fn on_did_change_managed_settings(&self) -> Event<ManagedSettingsData> {
        self.managed.on_did_change.event()
    }

    fn initialize(&self) -> Promise<ManagedSettingsData> {
        let this = self.me.upgrade().expect("o cliente está vivo enquanto é usado");
        async_fn(async move {
            let existing = this.initialization_promise.borrow().clone();
            let initialization = existing.unwrap_or_else(|| {
                // `initializeFromChannel()`: o instantâneo só vale se nenhum evento chegou antes dele.
                let managed = this.managed.clone();
                this.channel.call("getManagedSettings", Marshalled::Undefined, CancellationToken::None).then(move |value| managed.update_unless_received(managed_settings_from_marshalled(&value), true))
            });
            *this.initialization_promise.borrow_mut() = Some(initialization.clone());
            if let Err(error) = initialization.clone().await {
                let is_current = this.initialization_promise.borrow().as_ref().is_some_and(|current| current.ptr_eq(&initialization));
                if is_current {
                    *this.initialization_promise.borrow_mut() = None;
                }
                return Err(error);
            }
            Ok(this.managed.get())
        })
    }

    fn update_policy_definitions(&self, policy_definitions: PolicyDefinitions) -> Promise<ManagedSettingsData> {
        let managed = self.managed.clone();
        self.channel.call("updatePolicyDefinitions", policy_definitions_to_marshalled(&policy_definitions), CancellationToken::None).then(move |value| {
            managed.update(managed_settings_from_marshalled(&value), false);
            managed.get()
        })
    }
}

impl IDisposable for NativeManagedSettingsChannelClient {
    fn dispose(&self) {
        self.base.dispose();
    }
}
