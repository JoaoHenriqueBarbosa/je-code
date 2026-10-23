//! Porte de `src/vs/platform/policy/common/fileManagedSettingsIpc.ts`: o canal de IPC do serviço de
//! configurações gerenciadas de arquivo (o lado do servidor, [`FileManagedSettingsChannel`], e o cliente,
//! [`FileManagedSettingsChannelClient`]).
//!
//! Decisões de tradução:
//! - Os valores que atravessam o canal são [`Marshalled`]. As configurações normalizadas
//!   ([`ManagedSettingsData`]) viajam como um objeto de texto, número e booleano; as brutas
//!   ([`RawManagedSettingsData`]) como o JSON de onde vieram (`null`, booleano, número, texto, lista, objeto;
//!   um `Date` viaja como `Date`). O documento bruto sai de `JSON.parse`, então o resto do [`Dynamic`]
//!   (funções, `RegExp`, arrays tipados) não aparece e viraria `undefined`.
//! - `IServerChannel.call` com um comando desconhecido lança (`throw`), como o original; o servidor de IPC
//!   converte o lançamento em rejeição.
//! - `updateRawManagedSettings` e `updateManagedSettings` do cliente são a mesma lógica sobre dois valores:
//!   um [`Snapshot`] genérico, com a igualdade de cada tipo (`equals` do `objects.ts` no bruto, e a do mapa no
//!   normalizado).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::JsString;
use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::errors::{self, Error};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, DisposableStore, IDisposable};
use je_base::marshalling::Marshalled;
use je_base::objects::equals;
use je_base::parts::ipc::{IChannel, IServerChannel};
use je_base::policy::PolicyValue;
use je_base::promise::Promise;

use super::copilot_managed_settings::{IFileManagedSettingsService, ManagedSettingsData, RawManagedSettingsData, empty_raw_managed_settings};

/// As configurações brutas como valor de canal.
pub fn raw_managed_settings_to_marshalled(raw: &RawManagedSettingsData) -> Marshalled {
    match raw {
        Dynamic::Null => Marshalled::Null,
        Dynamic::Bool(flag) => Marshalled::Bool(*flag),
        Dynamic::Number(number) => Marshalled::Number(*number),
        Dynamic::String(text) => Marshalled::String(text.clone()),
        Dynamic::Array(items) => Marshalled::Array(items.to_vec().iter().map(raw_managed_settings_to_marshalled).collect()),
        Dynamic::Object(object) => Marshalled::Object(object.entries().iter().map(|(key, value)| (key.clone(), raw_managed_settings_to_marshalled(value))).collect()),
        Dynamic::Date(date) => Marshalled::Date(date.time_value()),
        _ => Marshalled::Undefined,
    }
}

/// O inverso de [`raw_managed_settings_to_marshalled`].
pub fn raw_managed_settings_from_marshalled(value: &Marshalled) -> RawManagedSettingsData {
    match value {
        Marshalled::Null => Dynamic::Null,
        Marshalled::Bool(flag) => Dynamic::Bool(*flag),
        Marshalled::Number(number) => Dynamic::Number(*number),
        Marshalled::String(text) => Dynamic::String(text.clone()),
        Marshalled::Array(items) => Dynamic::array(items.iter().map(raw_managed_settings_from_marshalled)),
        Marshalled::Object(entries) => Dynamic::object(entries.iter().map(|(key, value)| (key.clone(), raw_managed_settings_from_marshalled(value)))),
        Marshalled::Date(time_value) => Dynamic::date(*time_value),
        _ => Dynamic::Undefined,
    }
}

/// As configurações normalizadas como valor de canal.
pub fn managed_settings_to_marshalled(managed_settings: &ManagedSettingsData) -> Marshalled {
    Marshalled::Object(
        managed_settings
            .iter()
            .map(|(key, value)| {
                let value = match value {
                    PolicyValue::Boolean(flag) => Marshalled::Bool(*flag),
                    PolicyValue::Number(number) => Marshalled::Number(*number),
                    PolicyValue::String(text) => Marshalled::String(JsString::from(text.as_str())),
                };
                (JsString::from(key.as_str()), value)
            })
            .collect(),
    )
}

/// O inverso de [`managed_settings_to_marshalled`]; o que não é texto, número nem booleano é ignorado.
pub fn managed_settings_from_marshalled(value: &Marshalled) -> ManagedSettingsData {
    let Marshalled::Object(entries) = value else { return ManagedSettingsData::new() };
    entries
        .iter()
        .filter_map(|(key, value)| {
            let value = match value {
                Marshalled::Bool(flag) => PolicyValue::Boolean(*flag),
                Marshalled::Number(number) => PolicyValue::Number(*number),
                Marshalled::String(text) => PolicyValue::String(text.to_string_lossy()),
                _ => return None,
            };
            Some((key.to_string_lossy(), value))
        })
        .collect()
}

/// `FileManagedSettingsChannel`.
pub struct FileManagedSettingsChannel {
    service: Rc<dyn IFileManagedSettingsService>,
    disposables: Rc<DisposableStore>,
}

impl FileManagedSettingsChannel {
    pub fn new(service: Rc<dyn IFileManagedSettingsService>) -> Self {
        Self { service, disposables: DisposableStore::new() }
    }
}

impl IServerChannel for FileManagedSettingsChannel {
    fn listen(&self, _ctx: &Marshalled, event: &str, _arg: Marshalled) -> Event<Marshalled> {
        match event {
            "onDidChangeRawManagedSettings" => self.service.on_did_change_raw_managed_settings().map(raw_managed_settings_to_marshalled, None),
            "onDidChangeManagedSettings" => self.service.on_did_change_managed_settings().map(managed_settings_to_marshalled, None),
            _ => errors::throw(Error::new(format!("Event not found: {event}"))),
        }
    }

    fn call(&self, _ctx: &Marshalled, command: &str, _arg: Marshalled, _cancellation_token: CancellationToken) -> Promise<Marshalled> {
        let service = self.service.clone();
        match command {
            "getRawManagedSettings" => self.service.initialize().then(move |_| raw_managed_settings_to_marshalled(&service.raw_managed_settings())),
            "getManagedSettings" => self.service.initialize().then(move |_| managed_settings_to_marshalled(&service.managed_settings())),
            _ => errors::throw(Error::new(format!("Call not found: {command}"))),
        }
    }
}

impl IDisposable for FileManagedSettingsChannel {
    fn dispose(&self) {
        self.disposables.dispose();
    }
}

/// Um valor recebido do servidor: o atual, se já chegou algum, e o evento de mudança.
struct Snapshot<T: Clone + 'static> {
    value: RefCell<T>,
    has_received: Cell<bool>,
    on_did_change: Rc<Emitter<T>>,
    equal: fn(&T, &T) -> bool,
}

impl<T: Clone + 'static> Snapshot<T> {
    fn new(initial: T, equal: fn(&T, &T) -> bool) -> Rc<Self> {
        Rc::new(Self { value: RefCell::new(initial), has_received: Cell::new(false), on_did_change: Emitter::new(), equal })
    }

    fn get(&self) -> T {
        self.value.borrow().clone()
    }

    /// `updateRawManagedSettings` / `updateManagedSettings`.
    fn update(&self, value: T, fire_event: bool) {
        self.has_received.set(true);
        if (self.equal)(&self.value.borrow(), &value) {
            return;
        }

        *self.value.borrow_mut() = value.clone();
        if fire_event {
            self.on_did_change.fire(value);
        }
    }

    /// O instantâneo inicial só vale se nenhum evento chegou antes dele.
    fn update_unless_received(&self, value: T, fire_event: bool) {
        if !self.has_received.get() {
            self.update(value, fire_event);
        }
    }
}

/// `FileManagedSettingsChannelClient`.
pub struct FileManagedSettingsChannelClient {
    base: Rc<Disposable>,
    raw: Rc<Snapshot<RawManagedSettingsData>>,
    managed: Rc<Snapshot<ManagedSettingsData>>,
    initial_snapshot: Promise<()>,
}

impl FileManagedSettingsChannelClient {
    pub fn new(channel: Rc<dyn IChannel>) -> Self {
        let base = Disposable::new();
        let raw = Snapshot::new(empty_raw_managed_settings(), equals);
        let managed = Snapshot::new(ManagedSettingsData::new(), |a, b| a == b);
        base.register(raw.on_did_change.clone());
        base.register(managed.on_did_change.clone());

        let listener_raw = raw.clone();
        base.register(channel.listen("onDidChangeRawManagedSettings", Marshalled::Undefined).listen(move |value| listener_raw.update(raw_managed_settings_from_marshalled(value), true)));
        let listener_managed = managed.clone();
        base.register(channel.listen("onDidChangeManagedSettings", Marshalled::Undefined).listen(move |value| listener_managed.update(managed_settings_from_marshalled(value), true)));

        let snapshot_raw = raw.clone();
        let raw_snapshot = channel
            .call("getRawManagedSettings", Marshalled::Undefined, CancellationToken::None)
            .then(move |value| snapshot_raw.update_unless_received(raw_managed_settings_from_marshalled(&value), true));
        let snapshot_managed = managed.clone();
        let managed_snapshot = channel
            .call("getManagedSettings", Marshalled::Undefined, CancellationToken::None)
            .then(move |value| snapshot_managed.update_unless_received(managed_settings_from_marshalled(&value), true));
        let initial_snapshot = Promise::all(vec![raw_snapshot, managed_snapshot]).then(|_| ());

        Self { base, raw, managed, initial_snapshot }
    }
}

impl IFileManagedSettingsService for FileManagedSettingsChannelClient {
    fn raw_managed_settings(&self) -> RawManagedSettingsData {
        self.raw.get()
    }

    fn managed_settings(&self) -> ManagedSettingsData {
        self.managed.get()
    }

    fn on_did_change_raw_managed_settings(&self) -> Event<RawManagedSettingsData> {
        self.raw.on_did_change.event()
    }

    fn on_did_change_managed_settings(&self) -> Event<ManagedSettingsData> {
        self.managed.on_did_change.event()
    }

    fn initialize(&self) -> Promise<ManagedSettingsData> {
        let managed = self.managed.clone();
        self.initial_snapshot.then(move |()| managed.get())
    }
}

impl IDisposable for FileManagedSettingsChannelClient {
    fn dispose(&self) {
        self.base.dispose();
    }
}
