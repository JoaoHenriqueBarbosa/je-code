//! Porte de `src/vs/platform/configuration/common/configurationService.ts`: o `ConfigurationService`, que carrega
//! o `settings.json` do usuário, observa mudanças, emite `onDidChangeConfiguration` e grava `updateValue` no
//! arquivo com `jsonEdit`, preservando comentários, formato e vírgulas.
//!
//! Decisões de tradução:
//! - **Sobrecargas** de `getValue` e `updateValue`: [`ConfigurationService::get_value`] recebe a seção e os
//!   `overrides` opcionais; [`ConfigurationService::update_value`] recebe os `overrides` (já na forma
//!   `IConfigurationUpdateOverrides`, [`ConfigurationUpdateOverrides`]) e o alvo opcionais. Um
//!   `IConfigurationOverrides` converte para os de atualização com [`update_overrides_of`]. `options`
//!   (`IConfigurationUpdateOptions`) não é lido pelo original neste serviço e não existe aqui.
//! - **`ConfigurationEditing`** (classe privada do original) são os campos `queue` e `formatting_options` do
//!   serviço, com o mesmo comportamento: as escritas entram numa fila (sem corrida) e as opções de formatação
//!   são lidas da configuração uma vez, na primeira escrita.
//! - **Valores dinâmicos** (`unknown`) são [`Dynamic`]; `undefined` em `updateValue` remove a chave.
//! - **Erros**: o `throw new Error(...)` de um `async` é uma promessa rejeitada.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::JsString;
use je_base::async_::{Queue, RunOnceScheduler, async_fn};
use je_base::buffer::VSBuffer;
use je_base::dynamic::{Dynamic, Truthy};
use je_base::errors::Error;
use je_base::event::{Emitter, Event};
use je_base::json::{self, ParseOptions};
use je_base::json_edit::{EditSegment, apply_edits, set_property};
use je_base::json_formatter::FormattingOptions;
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::map::ResourceMap;
use je_base::objects;
use je_base::promise::Promise;
use je_base::uri::URI;

use super::defaults::DefaultConfiguration;
use super::models::{Configuration, ConfigurationChangeEvent, ConfigurationInspectValue, ConfigurationKeys, ConfigurationModel, PreviousConfiguration};
use super::policy_configuration::{NullPolicyConfiguration, PolicyConfiguration, PolicyConfigurationApi};
use super::registry::key_from_override_identifiers;
use super::user_settings::UserSettings;
use super::{ConfigurationChange, ConfigurationData, ConfigurationOverrides, ConfigurationTarget, ConfigurationUpdateOverrides, ConfigurationValue};
use crate::files::file_service::FileService;
use crate::files::files::{FileOperationError, FileOperationResult};
use crate::js::to_json;
use crate::log::LogServiceRef;
use crate::policy::PolicyService;
use crate::resources::ext_uri_biased_ignore_path_case;

/// Os `IConfigurationOverrides` como `IConfigurationUpdateOverrides`: `{ resource, overrideIdentifiers:
/// [overrideIdentifier] }`.
pub fn update_overrides_of(overrides: &ConfigurationOverrides) -> ConfigurationUpdateOverrides {
    ConfigurationUpdateOverrides {
        resource: overrides.resource.clone(),
        override_identifiers: overrides.override_identifier.as_ref().filter(|id| !id.is_empty()).map(|id| vec![id.clone()]),
    }
}

/// `ConfigurationService`.
pub struct ConfigurationService {
    base: Rc<Disposable>,
    settings_resource: URI,
    file_service: Rc<FileService>,
    log_service: LogServiceRef,
    configuration: RefCell<Rc<Configuration>>,
    default_configuration: Rc<DefaultConfiguration>,
    policy_configuration: Rc<dyn PolicyConfigurationApi>,
    user_configuration: Rc<UserSettings>,
    reload_configuration_scheduler: Rc<RunOnceScheduler>,
    on_did_change_configuration: Rc<Emitter<Rc<ConfigurationChangeEvent>>>,
    // --- ConfigurationEditing
    queue: Queue<()>,
    formatting_options: Cell<Option<(bool, usize)>>,
    eol: RefCell<Option<String>>,
    /// O próprio serviço, para os métodos do trait [`IConfigurationService`] (que recebem `&self`) chamarem os que
    /// precisam do `Rc`.
    this: std::rc::Weak<Self>,
}

/// `IConfigurationService` (de `configuration.ts`): o contrato do serviço de configuração, que os demais serviços
/// recebem por injeção.
///
/// Decisões de tradução:
/// - As sobrecargas de `getValue` e `updateValue` são a forma completa: seção e `overrides` opcionais.
/// - `reloadConfiguration(target?)` não recebe o alvo (`ConfigurationTarget | IWorkspaceFolder`): o serviço
///   desta camada lê só o `settings.json` do usuário e ignora o argumento, como o original.
/// - `getConfigurationData()` devolve `Option` (o `IConfigurationData | null` do original).
/// - `inspect` devolve o `IConfigurationValue<T>` do original, que é o trait [`ConfigurationValue`]: o serviço real
///   devolve o [`ConfigurationInspectValue`] e os serviços de teste, um literal com as camadas que preenchem.
pub trait IConfigurationService {
    fn on_did_change_configuration(&self) -> Event<Rc<ConfigurationChangeEvent>>;
    fn get_configuration_data(&self) -> Option<ConfigurationData>;
    fn get_value(&self, section: Option<&str>, overrides: Option<&ConfigurationOverrides>) -> Dynamic;
    fn update_value(&self, key: &str, value: Dynamic, overrides: Option<ConfigurationUpdateOverrides>, target: Option<ConfigurationTarget>) -> Promise<()>;
    fn inspect(&self, key: &str, overrides: &ConfigurationOverrides) -> Rc<dyn ConfigurationValue>;
    fn reload_configuration(&self) -> Promise<()>;
    fn keys(&self) -> ConfigurationKeys;
}

je_instantiation::service_id!(pub I_CONFIGURATION_SERVICE: dyn IConfigurationService = "configurationService");

impl IConfigurationService for ConfigurationService {
    fn on_did_change_configuration(&self) -> Event<Rc<ConfigurationChangeEvent>> {
        ConfigurationService::on_did_change_configuration(self)
    }
    fn get_configuration_data(&self) -> Option<ConfigurationData> {
        Some(ConfigurationService::get_configuration_data(self))
    }
    fn get_value(&self, section: Option<&str>, overrides: Option<&ConfigurationOverrides>) -> Dynamic {
        ConfigurationService::get_value(self, section, overrides)
    }
    fn update_value(&self, key: &str, value: Dynamic, overrides: Option<ConfigurationUpdateOverrides>, target: Option<ConfigurationTarget>) -> Promise<()> {
        match self.this.upgrade() {
            Some(this) => ConfigurationService::update_value(&this, key, value, overrides, target),
            None => Promise::rejected(Error::new("ConfigurationService is disposed")),
        }
    }
    fn inspect(&self, key: &str, overrides: &ConfigurationOverrides) -> Rc<dyn ConfigurationValue> {
        Rc::new(ConfigurationService::inspect(self, key, overrides))
    }
    fn reload_configuration(&self) -> Promise<()> {
        match self.this.upgrade() {
            Some(this) => ConfigurationService::reload_configuration(&this),
            None => Promise::rejected(Error::new("ConfigurationService is disposed")),
        }
    }
    fn keys(&self) -> ConfigurationKeys {
        ConfigurationService::keys(self)
    }
}

impl ConfigurationService {
    pub fn new(settings_resource: URI, file_service: Rc<FileService>, policy_service: Rc<dyn PolicyService>, log_service: LogServiceRef) -> Rc<Self> {
        let base = Disposable::new();
        let default_configuration = DefaultConfiguration::new(log_service.clone());
        base.register(default_configuration.clone());
        let policy_configuration: Rc<dyn PolicyConfigurationApi> = if policy_service.is_null() {
            Rc::new(NullPolicyConfiguration::new())
        } else {
            let policy_configuration = PolicyConfiguration::new(default_configuration.clone(), policy_service, log_service.clone());
            base.register(policy_configuration.clone());
            Rc::new(policy_configuration)
        };
        let user_configuration = UserSettings::new(settings_resource.clone(), Default::default(), ext_uri_biased_ignore_path_case(), file_service.clone(), log_service.clone());
        base.register(user_configuration.clone());
        let empty = || ConfigurationModel::create_empty_model(log_service.clone());
        let configuration = Configuration::new(
            default_configuration.configuration_model(),
            policy_configuration.configuration_model(),
            empty(),
            empty(),
            empty(),
            empty(),
            ResourceMap::new(),
            empty(),
            ResourceMap::new(),
            log_service.clone(),
        );

        let on_did_change_configuration = Emitter::new();
        base.register(on_did_change_configuration.clone());

        let service = Rc::new_cyclic(|weak: &std::rc::Weak<Self>| {
            let weak_for_self = weak.clone();
            let weak = weak.clone();
            let reload_configuration_scheduler = RunOnceScheduler::new(
                move || {
                    if let Some(this) = weak.upgrade() {
                        let _ = this.reload_configuration();
                    }
                },
                50,
            );
            base.register(reload_configuration_scheduler.clone());
            Self {
                base,
                settings_resource,
                file_service,
                log_service,
                configuration: RefCell::new(Rc::new(configuration)),
                default_configuration,
                policy_configuration,
                user_configuration,
                reload_configuration_scheduler,
                on_did_change_configuration,
                queue: Queue::new(),
                formatting_options: Cell::new(None),
                eol: RefCell::new(None),
                this: weak_for_self,
            }
        });

        let weak = Rc::downgrade(&service);
        service.base.register(service.default_configuration.on_did_change_configuration().listen({
            let weak = weak.clone();
            move |change| {
                if let Some(this) = weak.upgrade() {
                    this.on_did_default_configuration_change(change.defaults.clone(), &change.properties);
                }
            }
        }));
        service.base.register(service.policy_configuration.on_did_change_configuration().listen({
            let weak = weak.clone();
            move |model| {
                if let Some(this) = weak.upgrade() {
                    this.on_did_policy_configuration_change(model.clone());
                }
            }
        }));
        service.base.register(service.user_configuration.on_did_change().listen(move |()| {
            if let Some(this) = weak.upgrade() {
                this.reload_configuration_scheduler.schedule(None);
            }
        }));

        service
    }

    /// `onDidChangeConfiguration`.
    pub fn on_did_change_configuration(&self) -> Event<Rc<ConfigurationChangeEvent>> {
        self.on_did_change_configuration.event()
    }

    fn configuration(&self) -> Rc<Configuration> {
        self.configuration.borrow().clone()
    }

    fn empty(&self) -> ConfigurationModel {
        ConfigurationModel::create_empty_model(self.log_service.clone())
    }

    /// `initialize()`.
    pub fn initialize(self: &Rc<Self>) -> Promise<()> {
        let this = self.clone();
        let default_model = self.default_configuration.initialize();
        let policy = self.policy_configuration.initialize();
        let user = self.user_configuration.load_configuration();
        async_fn(async move {
            let policy_model = policy.await?;
            let user_model = user.await?;
            *this.configuration.borrow_mut() = Rc::new(Configuration::new(
                default_model,
                policy_model,
                this.empty(),
                user_model,
                this.empty(),
                this.empty(),
                ResourceMap::new(),
                this.empty(),
                ResourceMap::new(),
                this.log_service.clone(),
            ));
            Ok(())
        })
    }

    /// `getConfigurationData()`.
    pub fn get_configuration_data(&self) -> ConfigurationData {
        self.configuration().to_data()
    }

    /// `getValue(section?, overrides?)`.
    pub fn get_value(&self, section: Option<&str>, overrides: Option<&ConfigurationOverrides>) -> Dynamic {
        let section = section.map(JsString::from);
        self.configuration().get_value(section.as_ref(), &overrides.cloned().unwrap_or_default(), None)
    }

    /// `updateValue(key, value, overrides?, target?)`.
    pub fn update_value(
        self: &Rc<Self>,
        key: &str,
        value: Dynamic,
        overrides: Option<ConfigurationUpdateOverrides>,
        target: Option<ConfigurationTarget>,
    ) -> Promise<()> {
        let (this, key) = (self.clone(), key.to_owned());
        async_fn(async move {
            if let Some(target) = target
                && target != ConfigurationTarget::UserLocal
                && target != ConfigurationTarget::User
            {
                return Err(Error::new(format!("Unable to write {key} to target {}.", target as i32)));
            }

            let mut overrides = overrides;
            if let Some(overrides) = overrides.as_mut()
                && let Some(identifiers) = overrides.override_identifiers.take()
            {
                let mut distinct: Vec<JsString> = Vec::new();
                for identifier in identifiers {
                    if !distinct.contains(&identifier) {
                        distinct.push(identifier);
                    }
                }
                overrides.override_identifiers = (!distinct.is_empty()).then_some(distinct);
            }

            let inspect = this.inspect(
                &key,
                &ConfigurationOverrides {
                    resource: overrides.as_ref().and_then(|o| o.resource.clone()),
                    override_identifier: overrides.as_ref().and_then(|o| o.override_identifiers.as_ref()).and_then(|ids| ids.first().cloned()),
                },
            );
            if !super::ConfigurationValue::policy_value(&inspect).is_undefined() {
                return Err(Error::new(format!("Unable to write {key} because it is configured in system policy.")));
            }

            // Remove the setting, if the value is same as default value
            let mut value = value;
            if objects::equals(&value, &super::ConfigurationValue::default_value(&inspect)) {
                value = Dynamic::Undefined;
            }

            if let Some(overrides) = overrides.as_mut()
                && let Some(identifiers) = overrides.override_identifiers.as_mut()
                && identifiers.len() > 1
            {
                identifiers.sort_by(|a, b| a.units().cmp(b.units()));
                let sorted = identifiers.clone();
                let existing = this.configuration().local_user_configuration().overrides().into_iter().find(|existing| {
                    let mut existing_identifiers = existing.identifiers.clone();
                    existing_identifiers.sort_by(|a, b| a.units().cmp(b.units()));
                    existing_identifiers == sorted
                });
                if let Some(existing) = existing {
                    *identifiers = existing.identifiers;
                }
            }

            let path: Vec<JsString> = match overrides.as_ref().and_then(|o| o.override_identifiers.as_ref()).filter(|ids| !ids.is_empty()) {
                Some(ids) => vec![key_from_override_identifiers(ids), JsString::from(key.as_str())],
                None => vec![JsString::from(key.as_str())],
            };

            this.write(path, value).await?;
            this.reload_configuration().await
        })
    }

    /// `inspect(key, overrides)`.
    pub fn inspect(&self, key: &str, overrides: &ConfigurationOverrides) -> ConfigurationInspectValue {
        self.configuration().inspect(key, overrides, None)
    }

    /// `keys()`.
    pub fn keys(&self) -> ConfigurationKeys {
        self.configuration().keys(None)
    }

    /// `reloadConfiguration()`.
    pub fn reload_configuration(self: &Rc<Self>) -> Promise<()> {
        let this = self.clone();
        self.user_configuration.load_configuration().then(move |model| this.on_did_change_user_configuration(model))
    }

    fn on_did_change_user_configuration(&self, user_configuration_model: ConfigurationModel) {
        let previous = self.configuration().to_data();
        let change = self.configuration().compare_and_update_local_user_configuration(user_configuration_model);
        self.trigger(change, previous, ConfigurationTarget::User);
    }

    fn on_did_default_configuration_change(&self, default_configuration_model: ConfigurationModel, properties: &[JsString]) {
        let previous = self.configuration().to_data();
        let change = self.configuration().compare_and_update_default_configuration(default_configuration_model, Some(properties.to_vec()));
        self.trigger(change, previous, ConfigurationTarget::Default);
    }

    fn on_did_policy_configuration_change(&self, policy_configuration: ConfigurationModel) {
        let previous = self.configuration().to_data();
        let change = self.configuration().compare_and_update_policy_configuration(policy_configuration);
        self.trigger(change, previous, ConfigurationTarget::Default);
    }

    fn trigger(&self, configuration_change: ConfigurationChange, previous: ConfigurationData, source: ConfigurationTarget) {
        let event = ConfigurationChangeEvent::new(
            configuration_change,
            Some(PreviousConfiguration { workspace: None, data: previous }),
            self.configuration(),
            None,
            self.log_service.clone(),
        );
        event.source.set(Some(source));
        self.on_did_change_configuration.fire(Rc::new(event));
    }

    // region: ConfigurationEditing

    /// `ConfigurationEditing#write(path, value)`: as escritas entram numa fila, para não haver corrida.
    fn write(self: &Rc<Self>, path: Vec<JsString>, value: Dynamic) -> Promise<()> {
        let this = self.clone();
        self.queue.queue(move || this.do_write_configuration(path, value))
    }

    fn do_write_configuration(self: &Rc<Self>, path: Vec<JsString>, value: Dynamic) -> Promise<()> {
        let this = self.clone();
        async_fn(async move {
            let mut content = match this.file_service.read_file(&this.settings_resource, None, None).await {
                Ok(file_content) => file_content.value.to_string(),
                Err(error) => {
                    if FileOperationError::from_error(&error).map(|e| e.file_operation_result) == Some(FileOperationResult::FileNotFound) {
                        "{}".to_owned()
                    } else {
                        return Err(error);
                    }
                }
            };

            let mut parse_errors = Vec::new();
            let units: Vec<u16> = content.encode_utf16().collect();
            json::parse(&units, &mut parse_errors, ParseOptions { allow_trailing_comma: true, allow_empty_content: true, ..Default::default() });
            if !parse_errors.is_empty() {
                return Err(Error::new("Unable to write into the settings file. Please open the file to correct errors/warnings in the file and try again."));
            }

            let edits = this.get_edits(&units, &path, &value)?;
            let edited = apply_edits(&units, &edits).map_err(|error| Error::new(error.0))?;
            content = String::from_utf16_lossy(&edited);

            this.file_service.write_file(&this.settings_resource, VSBuffer::from_string(&content).into(), None).await?;
            Ok(())
        })
    }

    fn get_edits(&self, content: &[u16], path: &[JsString], value: &Dynamic) -> Result<Vec<je_base::json_formatter::Edit>, Error> {
        let formatting_options = self.formatting_options();
        let path: Vec<EditSegment> = path.iter().cloned().map(EditSegment::Property).collect();
        set_property(content, &path, to_json(value), &formatting_options, None).map_err(|error| Error::new(error.0))
    }

    /// `formattingOptions`: lidas da configuração (`jsonc`) na primeira escrita e guardadas.
    fn formatting_options(&self) -> FormattingOptions {
        if self.formatting_options.get().is_none() {
            let mut eol = if cfg!(windows) { "\r\n".to_owned() } else { "\n".to_owned() };
            let overrides = ConfigurationOverrides::with_identifier("jsonc");
            if let Dynamic::String(configured) = self.get_value(Some("files.eol"), Some(&overrides)) {
                let configured = configured.to_string_lossy();
                if !configured.is_empty() && configured != "auto" {
                    eol = configured;
                }
            }
            *self.eol.borrow_mut() = Some(eol);
            let insert_spaces = self.get_value(Some("editor.insertSpaces"), Some(&overrides)).is_truthy();
            let tab_size = match self.get_value(Some("editor.tabSize"), Some(&overrides)) {
                Dynamic::Number(n) if n >= 0.0 && n.is_finite() => n as usize,
                // `tabSize: undefined` no original cai no padrão do formatador (4).
                _ => 4,
            };
            self.formatting_options.set(Some((insert_spaces, tab_size)));
        }
        let (insert_spaces, tab_size) = self.formatting_options.get().expect("calculado acima");
        FormattingOptions { tab_size: Some(tab_size), insert_spaces: Some(insert_spaces), eol: self.eol.borrow().as_deref().map(JsString::from) }
    }

    // endregion
}

impl IDisposable for ConfigurationService {
    fn dispose(&self) {
        self.base.dispose();
        self.queue.dispose();
    }
}
