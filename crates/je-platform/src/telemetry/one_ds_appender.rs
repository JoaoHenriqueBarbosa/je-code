//! Porte de `src/vs/platform/telemetry/common/1dsAppender.ts` e `browser/1dsAppender.ts`: o appender do One Data
//! System (1DS, o Application Insights da Microsoft), que monta o envelope de cada evento, valida os dados e os
//! entrega a um cliente do 1DS.
//!
//! O original delega o envio ao SDK `@microsoft/1ds-core-js` (`AppInsightsCore`) e ao canal
//! `@microsoft/1ds-post-js` (`PostChannel`), que serializam, agrupam, repetem e enviam por HTTP ao endpoint
//! `OneCollector`. Esse SDK é JavaScript e não é portado: o je-code **não envia telemetria a lugar nenhum por
//! conta própria**. Cada ponto de rede do original é uma dependência injetada, e sem ela nada sai:
//! - [`IAppInsightsCore`] é o subconjunto do `AppInsightsCore` que o appender usa (`pluginVersionString`, `track` e
//!   `unload`), o mesmo recorte que o original declara para facilitar mocks de teste;
//! - [`IOneDsClientBuilder`] é o `getClient` do original: constrói o cliente a partir da chave de instrumentação,
//!   na primeira vez que um evento precisa dele. É quem carrega o `xhrOverride` (o transporte HTTP do SDK) e a
//!   configuração do núcleo. Sem builder o appender por chave nem existe: [`OneDsClientSource::Key`] o exige;
//! - [`IOneDsHttpClient`] é o `fetch` da sonda de saúde do appender web (`OneDataSystemWebAppender`), que desliga
//!   o appender quando o endpoint não responde. O original dispara essa requisição na construção; aqui o
//!   construtor só a faz se o hospedeiro entrega o cliente HTTP, ou seja, se o usuário configurou o envio.
//!
//! Divergências:
//! - `iKeyOrClientFactory: string | (() => IAppInsightsCore)` é [`OneDsClientSource`]; o `xhrOverride` do
//!   construtor mora no builder (só o `getClient` o usava).
//! - [`TelemetryItem`] tem só os campos que o appender e o `applyEnvelopeDefaults` leem e escrevem (`name`,
//!   `baseData` e `ext`); o `ITelemetryItem` do SDK tem dezenas de outros, que o SDK preenche.
//! - `console.error(err)` é `eprintln!`.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use je_base::JsString;
use je_base::dynamic::{Dynamic, ObjectRef};
use je_base::errors::{self, Error};
use je_base::objects;
use je_base::promise::Promise;

use super::telemetry_utils::{ITelemetryAppender, validate_telemetry_data};

/// O endpoint de coleta do 1DS (`endpointUrl`).
pub const ENDPOINT_URL: &str = "https://mobile.events.data.microsoft.com/OneCollector/1.0";
/// O endpoint da sonda de saúde (`endpointHealthUrl`).
pub const ENDPOINT_HEALTH_URL: &str = "https://mobile.events.data.microsoft.com/ping";

/// `ITelemetryItem | IExtendedTelemetryItem`, no recorte que o appender usa. `base_data` e `ext` são objetos
/// dinâmicos (`undefined` quando ausentes), porque o SDK e os inicializadores os leem e completam por nome.
#[derive(Clone, Debug)]
pub struct TelemetryItem {
    pub name: String,
    pub base_data: Dynamic,
    pub ext: Dynamic,
}

/// `ITelemetryUnloadState`, que o `unload` do núcleo entrega ao terminar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TelemetryUnloadState {
    /// `TelemetryUnloadReason`.
    pub reason: u32,
    pub is_async: bool,
    pub flush_complete: Option<bool>,
}

/// `IAppInsightsCore`: o subconjunto do `AppInsightsCore` do `@microsoft/1ds-core-js` que o appender usa. Os
/// métodos recebem `&self` porque o cliente é compartilhado e `pluginVersionString` é uma propriedade mutável.
pub trait IAppInsightsCore {
    /// `pluginVersionString`.
    fn plugin_version_string(&self) -> String;
    fn set_plugin_version_string(&self, value: &str);
    fn track(&self, item: TelemetryItem);
    fn unload(&self, is_async: bool, unload_complete: Box<dyn FnOnce(&TelemetryUnloadState)>);
}

/// O que o `getClient` do original recebe e configura: a chave de instrumentação, o endpoint e a marca de
/// máquina interna que o inicializador de envelope usa ([`apply_envelope_defaults`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OneDsClientRequest {
    pub instrumentation_key: String,
    pub endpoint_url: String,
    pub add_internal_flag: bool,
}

/// `getClient(instrumentationKey, addInternalFlag, xhrOverride)`: constrói o cliente do 1DS. Quem implementa
/// reproduz a configuração do original:
/// - núcleo com `instrumentationKey`, `endpointUrl`, `loggingLevelTelemetry: 0`, `loggingLevelConsole: 0`,
///   `disableCookiesUsage: true`, `disableDbgExt: true` e `disableInstrumentationKeyValidation: true`;
/// - um canal de envio (`PostChannel`), com o transporte HTTP próprio (o `xhrOverride`) quando o ambiente não tem
///   `XMLHttpRequest`;
/// - um inicializador de telemetria que chama [`apply_envelope_defaults`] com `add_internal_flag` em cada envelope.
pub trait IOneDsClientBuilder {
    fn get_client(&self, request: &OneDsClientRequest) -> Promise<Rc<dyn IAppInsightsCore>>;
}

/// `fetch(url, { method: 'GET' })` da sonda de saúde: a promessa resolve se o endpoint respondeu e rejeita se a
/// requisição falhou (endpoint fora do ar, quase sempre um bloqueador de anúncios).
pub trait IOneDsHttpClient {
    fn get(&self, url: &str) -> Promise<()>;
}

/// `iKeyOrClientFactory: string | (() => IAppInsightsCore)`.
pub enum OneDsClientSource {
    /// A chave de instrumentação: o cliente é construído pelo `builder` quando o primeiro evento precisa dele.
    Key { instrumentation_key: String, builder: Rc<dyn IOneDsClientBuilder> },
    /// A fábrica do cliente (usada nos testes), chamada na construção do appender.
    Factory(Box<dyn FnOnce() -> Rc<dyn IAppInsightsCore>>),
}

/// `_aiCoreOrKey` sem o `undefined`: o cliente pronto, ou a chave com quem o constrói.
#[derive(Clone)]
enum AiCoreOrKey {
    Core(Rc<dyn IAppInsightsCore>),
    Key { instrumentation_key: String, builder: Rc<dyn IOneDsClientBuilder> },
}

/// `envelope[name] ??= {}`: o objeto filho, criado se faltar.
fn child_object(parent: &ObjectRef, name: &str) -> ObjectRef {
    let key = JsString::from(name);
    match parent.get(&key) {
        Some(Dynamic::Object(existing)) => existing,
        _ => {
            let created = ObjectRef::new();
            parent.set(&key, Dynamic::Object(created.clone()));
            created
        }
    }
}

/// `baseData?.measurements?.['common.msftInternal'] === 1`.
fn is_internal_account(base_data: &Dynamic) -> bool {
    let Dynamic::Object(base_data) = base_data else { return false };
    let Some(Dynamic::Object(measurements)) = base_data.get(&JsString::from("measurements")) else { return false };
    matches!(measurements.get(&JsString::from("common.msftInternal")), Some(Dynamic::Number(value)) if value == 1.0)
}

/// Preenche os campos do envelope de que todo evento precisa, inclusive a marca de roteamento interno.
pub fn apply_envelope_defaults(envelope: &mut TelemetryItem, is_internal_machine: Option<bool>) {
    // Opt the user out of 1DS data sharing
    if !matches!(envelope.ext, Dynamic::Object(_)) {
        envelope.ext = Dynamic::Object(ObjectRef::new());
    }
    let Dynamic::Object(ext) = &envelope.ext else { unreachable!("`ext` acabou de virar objeto") };
    child_object(ext, "web").set(&JsString::from("consentDetails"), Dynamic::string(r#"{"GPC_DataSharingOptIn":false}"#));

    // An internal account is only known after sign-in, so check each event and not just the machine.
    // `validateTelemetryData` turns the boolean common property into the measurement `1`.
    if is_internal_machine == Some(true) || is_internal_account(&envelope.base_data) {
        // Sets it to be internal only based on Windows UTC flagging
        child_object(ext, "utc").set(&JsString::from("flags"), Dynamic::Number(f64::from(0x0000811ECD_u32)));
    }
}

/// `AbstractOneDataSystemAppender`.
pub struct AbstractOneDataSystemAppender {
    this: Weak<AbstractOneDataSystemAppender>,
    is_internal_telemetry: bool,
    event_prefix: String,
    default_data: Dynamic,
    ai_core_or_key: RefCell<Option<AiCoreOrKey>>,
    async_ai_core: RefCell<Option<Promise<Rc<dyn IAppInsightsCore>>>>,
}

impl AbstractOneDataSystemAppender {
    pub fn new(is_internal_telemetry: bool, event_prefix: &str, default_data: Option<Dynamic>, client: OneDsClientSource) -> Rc<Self> {
        let ai_core_or_key = match client {
            OneDsClientSource::Factory(factory) => AiCoreOrKey::Core(factory()),
            OneDsClientSource::Key { instrumentation_key, builder } => AiCoreOrKey::Key { instrumentation_key, builder },
        };
        Rc::new_cyclic(|this| Self {
            this: this.clone(),
            is_internal_telemetry,
            event_prefix: event_prefix.to_owned(),
            default_data: default_data.unwrap_or_else(|| Dynamic::object(Vec::<(&str, Dynamic)>::new())),
            ai_core_or_key: RefCell::new(Some(ai_core_or_key)),
            async_ai_core: RefCell::new(None),
        })
    }

    /// `this._aiCoreOrKey = undefined`: daqui em diante o appender não envia nada.
    pub fn disable(&self) {
        *self.ai_core_or_key.borrow_mut() = None;
    }

    /// `!!this._aiCoreOrKey`: o cliente pronto ou uma chave não vazia.
    fn is_enabled(&self) -> bool {
        match &*self.ai_core_or_key.borrow() {
            None => false,
            Some(AiCoreOrKey::Core(_)) => true,
            Some(AiCoreOrKey::Key { instrumentation_key, .. }) => !instrumentation_key.is_empty(),
        }
    }

    fn with_ai_client(&self, callback: impl FnOnce(Rc<dyn IAppInsightsCore>) + 'static) {
        if !self.is_enabled() {
            return;
        }
        let source = self.ai_core_or_key.borrow().clone();
        let (instrumentation_key, builder) = match source {
            Some(AiCoreOrKey::Core(core)) => {
                callback(core);
                return;
            }
            Some(AiCoreOrKey::Key { instrumentation_key, builder }) => (instrumentation_key, builder),
            None => return,
        };

        let pending = self
            .async_ai_core
            .borrow_mut()
            .get_or_insert_with(|| {
                builder.get_client(&OneDsClientRequest { instrumentation_key, endpoint_url: ENDPOINT_URL.to_owned(), add_internal_flag: self.is_internal_telemetry })
            })
            .clone();
        let _ = pending.then_result(move |result| {
            match result {
                Ok(client) => callback(client),
                Err(err) => {
                    errors::on_unexpected_error(&err);
                    eprintln!("{err}");
                }
            }
            Ok::<(), Error>(())
        });
    }
}

impl ITelemetryAppender for AbstractOneDataSystemAppender {
    fn log(&self, event_name: &str, data: &Dynamic) {
        if !self.is_enabled() {
            return;
        }
        let data = objects::mixin(data, &self.default_data, true);
        let validated = validate_telemetry_data(&data);
        let name = format!("{}/{}", self.event_prefix, event_name);
        let version = validated.properties.iter().find(|(key, _)| key == "version").map_or_else(|| "Unknown".to_owned(), |(_, value)| value.clone());
        let validated = validated.to_dynamic();
        let base_data = Dynamic::object([
            ("name", Dynamic::string(&name)),
            ("properties", validated.get_str("properties")),
            ("measurements", validated.get_str("measurements")),
        ]);

        let _ = errors::catch(|| {
            self.with_ai_client(move |ai_client| {
                ai_client.set_plugin_version_string(&version);
                ai_client.track(TelemetryItem { name, base_data, ext: Dynamic::Undefined });
            });
        });
    }

    fn flush(&self) -> Promise<()> {
        if !self.is_enabled() {
            return Promise::resolved(());
        }
        Promise::with_executor(|resolver| {
            let this = self.this.clone();
            self.with_ai_client(move |ai_client| {
                ai_client.unload(
                    true,
                    Box::new(move |_| {
                        if let Some(this) = this.upgrade() {
                            this.disable();
                        }
                        resolver.resolve(());
                    }),
                );
            });
        })
    }
}

/// `OneDataSystemWebAppender`.
pub struct OneDataSystemWebAppender {
    base: Rc<AbstractOneDataSystemAppender>,
}

impl OneDataSystemWebAppender {
    pub fn new(is_internal_telemetry: bool, event_prefix: &str, default_data: Option<Dynamic>, client: OneDsClientSource, http_client: &dyn IOneDsHttpClient) -> Self {
        let base = AbstractOneDataSystemAppender::new(is_internal_telemetry, event_prefix, default_data, client);

        // If we cannot fetch the endpoint it means it is down and we should not send any telemetry.
        // This is most likely due to ad blockers
        let weak = Rc::downgrade(&base);
        let _ = http_client.get(ENDPOINT_HEALTH_URL).then_result(move |result| {
            if result.is_err()
                && let Some(base) = weak.upgrade()
            {
                base.disable();
            }
            Ok::<(), Error>(())
        });
        Self { base }
    }
}

impl ITelemetryAppender for OneDataSystemWebAppender {
    fn log(&self, event_name: &str, data: &Dynamic) {
        self.base.log(event_name, data);
    }

    fn flush(&self) -> Promise<()> {
        self.base.flush()
    }
}
