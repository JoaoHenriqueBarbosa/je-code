//! Porte de `src/vs/workbench/services/configuration/common/managedSettingsPresentation.ts`: o serviço que resolve o
//! valor que a interface mostra para uma configuração sob uma restrição gerenciada em tempo de execução (as
//! configurações gerenciadas do Copilot), sem trocar a política de configuração nem o valor salvo.
//!
//! Divergências:
//! - **Injeção.** `IManagedSettingsPresentationService` é o trait [`IManagedSettingsPresentationService`] com o
//!   identificador [`I_MANAGED_SETTINGS_PRESENTATION_SERVICE`]. O `@IManagedSettingsService` é o `Rc<dyn ...>` do
//!   construtor ([`ManagedSettingsPresentationService::new`]), e o `registerSingleton(..., Delayed)` do fim do arquivo
//!   não é portado (a montagem do workbench cria o serviço).
//! - **`ManagedSettingsPresentationValue`** (`ManagedSettingValue | readonly string[] | Readonly<Record<string,
//!   readonly string[]>>`) é um [`Dynamic`] (booleano, número, texto, lista de textos ou objeto de listas), e um valor
//!   ausente (`undefined`) é `None`. O `localValue?: unknown` é `Option<&Dynamic>`.
//! - **`managedSettingsPresentation`** vive no esquema da propriedade como uma função dinâmica (o registro guarda o
//!   esquema como objeto dinâmico), chamada com `(read, localValue)`; `read` é a função nativa que consulta o
//!   `IManagedSettingsService` (o valor gerenciado vira booleano, número ou texto, ou `undefined`).
//! - **`ReadonlySet<string>`** do evento é um `Rc<HashSet<String>>`.
//! - **`extends Disposable`** é um [`Disposable`] guardado em `base`, com o emissor e as duas assinaturas registrados.
//! - **[`read_managed_string_array`]** não existe no original: é o trecho que as duas configurações de lista
//!   (`chat.agent.sandbox.network.allowedDomains` e `chat.agent.sandbox.fileSystem.userConfiguredPaths`) repetem para ler
//!   o valor gerenciado como lista de textos, extraído para uma função. O `console.warn` é o [`je_base::console::warn`].

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use je_base::dynamic::{Dynamic, FunctionRef, Truthy};
use je_base::event::{Emitter, Event};
use je_base::json::{self, ParseOptions};
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::objects;
use je_base::policy::PolicyValue;
use je_base::{console, types};
use je_instantiation::service_id;
use je_platform::configuration::registry::configuration_registry;
use je_platform::js::from_json;
use je_platform::policy::copilot_managed_settings::IManagedSettingsService;

/// `IManagedSettingsPresentationService`: presentation-only restrictions; does not replace runtime policy
/// enforcement.
pub trait IManagedSettingsPresentationService {
    fn on_did_change(&self) -> Event<Rc<HashSet<String>>>;
    fn get_value(&self, setting: &str, local_value: Option<&Dynamic>) -> Option<Dynamic>;
}

service_id!(pub I_MANAGED_SETTINGS_PRESENTATION_SERVICE: dyn IManagedSettingsPresentationService = "managedSettingsPresentationService");

/// O valor gerenciado como valor dinâmico (`ManagedSettingValue`).
fn managed_value_to_dynamic(value: PolicyValue) -> Dynamic {
    match value {
        PolicyValue::Boolean(value) => Dynamic::Bool(value),
        PolicyValue::Number(value) => Dynamic::Number(value),
        PolicyValue::String(value) => Dynamic::string(&value),
    }
}

/// `read(key)`, o primeiro argumento de uma função `managedSettingsPresentation`: o valor gerenciado da chave, ou
/// `undefined`.
pub fn read_managed_setting(read: &Dynamic, key: &str) -> Dynamic {
    match read {
        Dynamic::Function(read) => read.call(&[Dynamic::string(key)]),
        _ => Dynamic::Undefined,
    }
}

/// Lê um valor gerenciado que deve ser uma lista de textos, entregue como texto JSON ou já como lista: o trecho que as
/// configurações `managedSettingsPresentation` de listas (hosts e caminhos do sandbox) repetem. Um JSON inválido
/// (`parse_failure_warning`) ou um valor que não é lista de textos (`shape_warning`) avisa no `console.warn` e dá `None`,
/// que é o `return undefined` do original (a apresentação gerenciada é ignorada).
pub fn read_managed_string_array(value: &Dynamic, parse_failure_warning: &str, shape_warning: &str) -> Option<Dynamic> {
    let mut parsed = value.clone();
    if let Dynamic::String(text) = value {
        let mut errors = Vec::new();
        let options = ParseOptions { disallow_comments: true, allow_trailing_comma: false, allow_empty_content: false };
        match json::parse(text.units(), &mut errors, options) {
            Some(json_value) if errors.is_empty() => parsed = from_json(&json_value),
            _ => {
                console::warn(parse_failure_warning);
                return None;
            }
        }
    }
    if !types::is_string_array(&parsed) {
        console::warn(shape_warning);
        return None;
    }
    Some(parsed)
}

/// `ManagedSettingsPresentationService`.
pub struct ManagedSettingsPresentationService {
    base: Rc<Disposable>,
    managed_settings_service: Rc<dyn IManagedSettingsService>,
    values: RefCell<HashMap<String, Dynamic>>,
    on_did_change: Rc<Emitter<Rc<HashSet<String>>>>,
}

impl ManagedSettingsPresentationService {
    pub fn new(managed_settings_service: Rc<dyn IManagedSettingsService>) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change = Emitter::new();
        base.register(on_did_change.clone());
        let service = Rc::new(Self { base, managed_settings_service, values: RefCell::new(HashMap::new()), on_did_change });
        service.update();

        let weak = Rc::downgrade(&service);
        service.base.register(service.managed_settings_service.on_did_change_managed_settings().listen({
            let weak = weak.clone();
            move |()| {
                if let Some(this) = weak.upgrade() {
                    this.update();
                }
            }
        }));
        service.base.register(configuration_registry().on_did_update_configuration().listen(move |_| {
            if let Some(this) = weak.upgrade() {
                this.update();
            }
        }));
        service
    }

    fn update(&self) {
        let mut values: HashMap<String, Dynamic> = HashMap::new();
        let mut changed: HashSet<String> = HashSet::new();
        for (key, property) in configuration_registry().get_configuration_properties().entries() {
            if !property.get("managedSettingsPresentation").is_truthy() {
                continue;
            }
            let key = key.to_string_lossy();
            let value = self.get_value(&key, None);
            let previous = self.values.borrow().get(&key).cloned();
            if !objects::equals(&value.clone().unwrap_or(Dynamic::Undefined), &previous.unwrap_or(Dynamic::Undefined)) {
                changed.insert(key.clone());
            }
            if let Some(value) = value {
                values.insert(key, value);
            }
        }
        for key in self.values.borrow().keys() {
            if !values.contains_key(key) {
                changed.insert(key.clone());
            }
        }
        *self.values.borrow_mut() = values;
        if !changed.is_empty() {
            self.on_did_change.fire(Rc::new(changed));
        }
    }
}

impl IManagedSettingsPresentationService for ManagedSettingsPresentationService {
    fn on_did_change(&self) -> Event<Rc<HashSet<String>>> {
        self.on_did_change.event()
    }

    fn get_value(&self, setting: &str, local_value: Option<&Dynamic>) -> Option<Dynamic> {
        let property = configuration_registry().get_configuration_properties().get(setting)?;
        let Dynamic::Function(presentation) = property.get("managedSettingsPresentation") else {
            return None;
        };
        let managed_settings_service = self.managed_settings_service.clone();
        let read = FunctionRef::native("read", 1, move |args| {
            let key = match args.first() {
                Some(Dynamic::String(key)) => key.to_string_lossy(),
                _ => return Dynamic::Undefined,
            };
            managed_settings_service.get_managed_setting_value(&key).map_or(Dynamic::Undefined, managed_value_to_dynamic)
        });
        let value = presentation.call(&[Dynamic::Function(read), local_value.cloned().unwrap_or(Dynamic::Undefined)]);
        (!value.is_undefined()).then_some(value)
    }
}

impl IDisposable for ManagedSettingsPresentationService {
    fn dispose(&self) {
        self.base.dispose();
    }
}
