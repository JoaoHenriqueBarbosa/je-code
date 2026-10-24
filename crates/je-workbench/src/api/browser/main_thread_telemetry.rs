//! Porte de `src/vs/workbench/api/browser/mainThreadTelemetry.ts`: a ponte entre o `ITelemetryService` do workbench e os
//! eventos que as extensões publicam no host, mais o comando interno que grava o contexto de atribuição do CAPI.
//!
//! Divergências:
//! - `@extHostNamedCustomer(MainContext.MainThreadTelemetry)` e o `IExtHostContext` do construtor: o registro de clientes
//!   não existe ainda; o construtor recebe o [`IMainContext`] e pede o proxy de `ExtHostTelemetry`. `extends Disposable` é
//!   o [`Disposable`] guardado no campo `base`.
//! - O `data: ITelemetryData = Object.create(null)` de `$publicLog` e o `data?` de `$publicLog2` são `Option<Dynamic>`: o objeto
//!   vira o [`ITelemetryData`] (as chaves `from` e `target` com texto vão para os campos próprios, e as demais, na ordem, para
//!   `extra`), e `data[MainThreadTelemetry._name] = true` substitui ou acrescenta a chave `pluginHostTelemetry`.
//! - `CommandsRegistry.registerCommand(SET_CAPI_ASSIGNMENT_CONTEXT_COMMAND, ...)` roda no carregamento do módulo no original; aqui é
//!   [`register_set_capi_assignment_context_command`], que o ponto de entrada do workbench chama uma vez. `typeof value !== 'string'` é
//!   o argumento não ser [`Dynamic::String`] (ou faltar).
//! - `isValidAssignmentContext` é `je_platform::telemetry::assignment_context::is_valid_assignment_context`, portado junto.

use std::rc::Rc;

use je_base::dynamic::Dynamic;
use je_base::lifecycle::{Disposable, DisposableRef, IDisposable};
use je_platform::commands::commands::{commands_registry, CommandHandler};
use je_platform::configuration::service::IConfigurationService;
use je_platform::environment::environment::EnvironmentService;
use je_platform::product::IProductService;
use je_platform::telemetry::assignment_context::is_valid_assignment_context;
use je_platform::telemetry::{supports_telemetry, ITelemetryData, ITelemetryService, TelemetryLevel, I_TELEMETRY_SERVICE, TELEMETRY_OLD_SETTING_ID, TELEMETRY_SETTING_ID};

use crate::api::common::ext_host_protocol::{ExtHostContext, ExtHostTelemetryShape, IMainContext, MainThreadTelemetryShape, TelemetryProductConfig};

/// `MainThreadTelemetry`.
pub struct MainThreadTelemetry {
    base: Rc<Disposable>,
    telemetry_service: Rc<dyn ITelemetryService>,
    environment_service: Rc<dyn EnvironmentService>,
    product_service: Rc<dyn IProductService>,
}

impl MainThreadTelemetry {
    const NAME: &'static str = "pluginHostTelemetry";

    pub fn new(
        ext_host_context: &dyn IMainContext,
        telemetry_service: Rc<dyn ITelemetryService>,
        configuration_service: Rc<dyn IConfigurationService>,
        environment_service: Rc<dyn EnvironmentService>,
        product_service: Rc<dyn IProductService>,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let proxy: Rc<dyn ExtHostTelemetryShape> = ext_host_context.get_proxy(&ExtHostContext::EXT_HOST_TELEMETRY);
        let this = Rc::new(Self { base, telemetry_service, environment_service, product_service });

        if supports_telemetry(&*this.product_service, &*this.environment_service) {
            let proxy = proxy.clone();
            let weak = Rc::downgrade(&this);
            this.base.register(configuration_service.on_did_change_configuration().listen(move |event| {
                if (event.affects_configuration(TELEMETRY_SETTING_ID, None) || event.affects_configuration(TELEMETRY_OLD_SETTING_ID, None))
                    && let Some(this) = weak.upgrade()
                {
                    proxy.rpc_on_did_change_telemetry_level(this.telemetry_level());
                }
            }));
        }
        proxy.rpc_initialize_telemetry_level(
            this.telemetry_level(),
            supports_telemetry(&*this.product_service, &*this.environment_service),
            this.product_service.enabled_telemetry_levels().map(|levels| TelemetryProductConfig { usage: levels.usage, error: levels.error }),
        );
        this
    }

    /// `get telemetryLevel()`.
    fn telemetry_level(&self) -> TelemetryLevel {
        if !supports_telemetry(&*self.product_service, &*self.environment_service) {
            return TelemetryLevel::None;
        }

        self.telemetry_service.telemetry_level()
    }
}

impl IDisposable for MainThreadTelemetry {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl MainThreadTelemetryShape for MainThreadTelemetry {
    fn rpc_public_log(&self, event_name: &str, data: Option<Dynamic>) {
        // __GDPR__COMMON__ "pluginHostTelemetry" : { "classification": "SystemMetaData", "purpose": "FeatureInsight", "isMeasurement": true }
        let mut telemetry_data = ITelemetryData::default();
        if let Some(Dynamic::Object(object)) = &data {
            for (key, value) in object.entries() {
                let key = key.to_string_lossy();
                match (key.as_str(), &value) {
                    ("from", Dynamic::String(text)) => telemetry_data.from = Some(text.to_string_lossy()),
                    ("target", Dynamic::String(text)) => telemetry_data.target = Some(text.to_string_lossy()),
                    _ => telemetry_data.extra.push((key, value)),
                }
            }
        }
        telemetry_data.extra.retain(|(key, _)| key != Self::NAME);
        telemetry_data.extra.push((Self::NAME.to_owned(), Dynamic::Bool(true)));
        self.telemetry_service.public_log(event_name, Some(telemetry_data));
    }

    fn rpc_public_log2(&self, event_name: &str, data: Option<Dynamic>) {
        self.rpc_public_log(event_name, data);
    }
}

pub const CAPI_ASSIGNMENT_CONTEXT_PROPERTY: &str = "capi.assignmentcontext";

pub const SET_CAPI_ASSIGNMENT_CONTEXT_COMMAND: &str = "_telemetry.setCapiAssignmentContext";

/// `isValidCapiAssignmentContext(value)`.
pub fn is_valid_capi_assignment_context(value: &str) -> bool {
    is_valid_assignment_context(value)
}

/// `CommandsRegistry.registerCommand(SET_CAPI_ASSIGNMENT_CONTEXT_COMMAND, ...)`: grava o contexto de atribuição como propriedade de
/// experimento do serviço de telemetria, ignorando o que não é um contexto válido.
pub fn register_set_capi_assignment_context_command() -> DisposableRef {
    let handler: CommandHandler = Rc::new(|accessor, args| {
        if let Some(Dynamic::String(value)) = args.first() {
            let value = value.to_string_lossy();
            if is_valid_capi_assignment_context(&value) {
                accessor.get(I_TELEMETRY_SERVICE).set_experiment_property(CAPI_ASSIGNMENT_CONTEXT_PROPERTY, &value);
            }
        }
        Dynamic::Undefined
    });
    commands_registry().register_command_handler(SET_CAPI_ASSIGNMENT_CONTEXT_COMMAND, Some(handler))
}
