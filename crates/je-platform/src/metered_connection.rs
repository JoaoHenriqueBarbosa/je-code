//! Porte de `src/vs/platform/meteredConnection/common/meteredConnection.ts`: o serviço que informa se a conexão
//! é cobrada por volume (`IMeteredConnectionService`) e a base dele (`AbstractMeteredConnectionService`).
//!
//! Divergências: `NetworkInformation`, `NavigatorWithConnection` e `getIsBrowserConnectionMetered` leem
//! `navigator.connection`, que não existe fora do navegador: quem tem a informação da rede a passa em
//! [`AbstractMeteredConnectionService::new`] e em [`AbstractMeteredConnectionService::set_is_browser_connection_metered`].
//! A base não tem subclasse portada, então os ganchos `onChangeBrowserConnection`, `onUpdated` e
//! `onChangeIsConnectionMetered` são os métodos privados com o comportamento padrão. O valor do `network.meteredConnection`
//! é guardado como veio (texto ou ausente), porque o original compara o valor cru: qualquer valor que não seja
//! `on` nem `off` age como `auto`.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::dynamic::Dynamic;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::promise::Promise;
use je_instantiation::service_id;

use crate::configuration::service::IConfigurationService;

/// `IMeteredConnectionService`: informa o estado da conexão medida.
pub trait IMeteredConnectionService {
    /// Whether the current network connection is metered.
    /// Always returns `false` if the `network.meteredConnection` setting is `off`.
    /// Always returns `true` if the `network.meteredConnection` setting is `on`.
    fn is_connection_metered(&self) -> bool;

    /// Resolves once the initial connection state is available, when initialization is asynchronous.
    fn when_connection_state_initialized(&self) -> Option<Promise<()>> {
        None
    }

    /// Event that fires when the metered connection status changes.
    fn on_did_change_is_connection_metered(&self) -> Event<bool>;
}

service_id!(pub I_METERED_CONNECTION_SERVICE: dyn IMeteredConnectionService = "meteredConnectionService");

pub const METERED_CONNECTION_SETTING_KEY: &str = "network.meteredConnection";

/// O valor de `network.meteredConnection` (`MeteredConnectionSettingValue`: `on`, `off` ou `auto`); `None` é o
/// que não é texto (ausente).
type SettingValue = Option<String>;

fn read_setting(configuration_service: &dyn IConfigurationService) -> SettingValue {
    match configuration_service.get_value(Some(METERED_CONNECTION_SETTING_KEY), None) {
        Dynamic::String(value) => Some(value.to_string_lossy()),
        _ => None,
    }
}

/// `setting === 'on' || (setting !== 'off' && isBrowserConnectionMetered)`.
fn compute_is_metered(setting: &SettingValue, is_browser_connection_metered: bool) -> bool {
    setting.as_deref() == Some("on") || (setting.as_deref() != Some("off") && is_browser_connection_metered)
}

/// `AbstractMeteredConnectionService`.
pub struct AbstractMeteredConnectionService {
    base: Rc<Disposable>,
    on_did_change_is_connection_metered: Rc<Emitter<bool>>,
    is_connection_metered: Cell<bool>,
    is_browser_connection_metered: Cell<bool>,
    metered_connection_setting: RefCell<SettingValue>,
}

impl AbstractMeteredConnectionService {
    pub fn new(configuration_service: Rc<dyn IConfigurationService>, is_browser_connection_metered: bool) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change_is_connection_metered = Emitter::new();
        base.register(on_did_change_is_connection_metered.clone());

        let metered_connection_setting = read_setting(configuration_service.as_ref());
        let is_connection_metered = compute_is_metered(&metered_connection_setting, is_browser_connection_metered);
        let this = Rc::new(Self {
            base,
            on_did_change_is_connection_metered,
            is_connection_metered: Cell::new(is_connection_metered),
            is_browser_connection_metered: Cell::new(is_browser_connection_metered),
            metered_connection_setting: RefCell::new(metered_connection_setting),
        });

        let weak: Weak<Self> = Rc::downgrade(&this);
        let service = configuration_service.clone();
        this.base.register(configuration_service.on_did_change_configuration().listen(move |event| {
            if !event.affects_configuration(METERED_CONNECTION_SETTING_KEY, None) {
                return;
            }
            let Some(this) = weak.upgrade() else { return };
            let value = read_setting(service.as_ref());
            if value != *this.metered_connection_setting.borrow() {
                *this.metered_connection_setting.borrow_mut() = value;
                this.on_updated();
            }
        }));
        this
    }

    /// `protected get isBrowserConnectionMetered()`.
    pub fn is_browser_connection_metered(&self) -> bool {
        self.is_browser_connection_metered.get()
    }

    /// `setIsBrowserConnectionMetered`.
    pub fn set_is_browser_connection_metered(&self, value: bool) {
        if value != self.is_browser_connection_metered.get() {
            self.is_browser_connection_metered.set(value);
            self.on_change_browser_connection();
        }
    }

    fn on_change_browser_connection(&self) {
        self.on_updated();
    }

    fn on_updated(&self) {
        let value = compute_is_metered(&self.metered_connection_setting.borrow(), self.is_browser_connection_metered.get());
        if value != self.is_connection_metered.get() {
            self.is_connection_metered.set(value);
            self.on_change_is_connection_metered();
        }
    }

    fn on_change_is_connection_metered(&self) {
        self.on_did_change_is_connection_metered.fire(self.is_connection_metered.get());
    }
}

impl IDisposable for AbstractMeteredConnectionService {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl IMeteredConnectionService for AbstractMeteredConnectionService {
    fn is_connection_metered(&self) -> bool {
        self.is_connection_metered.get()
    }

    fn on_did_change_is_connection_metered(&self) -> Event<bool> {
        self.on_did_change_is_connection_metered.event()
    }
}
