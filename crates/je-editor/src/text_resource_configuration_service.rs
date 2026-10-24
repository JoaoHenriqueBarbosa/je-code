//! Porte de `src/vs/editor/common/services/textResourceConfigurationService.ts` (o
//! `TextResourceConfigurationService`) e de `textResourceConfiguration.ts` (`ITextResourceConfigurationService`,
//! `ITextResourceConfigurationChangeEvent`, `ITextResourcePropertiesService`).
//!
//! O serviço lê a configuração de um recurso aplicando o override da linguagem dele: a do modelo aberto (na
//! posição, se houver) ou a que o `ILanguageService` adivinha pelo caminho. `updateValue` escolhe o alvo de
//! escrita pelo lugar onde a chave já está definida.
//!
//! Divergências:
//! - **Sobrecargas** de `getValue`: `getValue(resource, section?)` é [`ITextResourceConfigurationService::get_value`]
//!   e `getValue(resource, position?, section?)` é [`ITextResourceConfigurationService::get_value_at`]. Como no
//!   original, a posição só vale quando a seção vem junto (sem seção, a posição é ignorada).
//! - **Valores** (`T`, `unknown`) são [`Dynamic`], como no `IConfigurationService` do `je-platform`, e
//!   `IConfigurationValue<Readonly<T>>` é o [`ConfigurationValue`] que o serviço de configuração devolve.
//! - **`null` e `undefined`** do identificador de linguagem e do alvo são `Option`.
//! - **`IPosition`** é [`Position`] (no porte são o mesmo tipo).
//! - **`affectedKeys`** (`ReadonlySet<string>`) é uma fatia sem repetição (`ConfigurationChangeEvent` já a monta
//!   sem duplicatas, na ordem de inserção) e o `has` é `contains`.
//! - **`ITextResourceConfigurationChangeEvent`** é um trait; a implementação é a que o serviço cria a cada mudança.
//! - **`IModelService`** é o recorte mínimo de [`crate::services_model`].
//! - **`_serviceBrand`** não existe: a identidade do serviço é o [`ServiceId`](je_instantiation::ServiceId).

use std::rc::Rc;

use je_base::JsString;
use je_base::dynamic::Dynamic;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::promise::Promise;
use je_base::uri::URI;
use je_instantiation::{Injectable, service_id, service_impl};
use je_platform::configuration::models::{ConfigurationChangeEvent, InspectValue};
use je_platform::configuration::service::{IConfigurationService, update_overrides_of};
use je_platform::configuration::{ConfigurationOverrides, ConfigurationTarget, ConfigurationValue};

use crate::language_service::ILanguageService;
use crate::position::Position;
use crate::services_model::IModelService;

/// `ITextResourceConfigurationChangeEvent`.
pub trait ITextResourceConfigurationChangeEvent {
    /// Todas as chaves afetadas. Inclui também os overrides de linguagem e as chaves alteradas dentro deles.
    fn affected_keys(&self) -> &[JsString];

    /// `true` se a seção mudou para o recurso dado.
    ///
    /// Exemplo: para saber se a seção mudou para um recurso, `e.affects_configuration(resource, section)`.
    fn affects_configuration(&self, resource: Option<&URI>, section: &str) -> bool;
}

/// `ITextResourceConfigurationService`.
pub trait ITextResourceConfigurationService {
    /// Dispara quando a configuração muda.
    fn on_did_change_configuration(&self) -> Event<Rc<dyn ITextResourceConfigurationChangeEvent>>;

    /// `getValue(resource, section?)`: o valor da seção para o recurso, com os overrides da linguagem aplicados.
    /// O valor é nativo ou um objeto indexado pelo nome da seção.
    fn get_value(&self, resource: Option<&URI>, section: Option<&str>) -> Dynamic;

    /// `getValue(resource, position?, section?)`: como [`Self::get_value`], com a linguagem tomada na posição.
    fn get_value_at(&self, resource: Option<&URI>, position: Option<&Position>, section: Option<&str>) -> Dynamic;

    /// Inspeciona os valores da seção para o recurso, com os overrides da linguagem aplicados.
    fn inspect(&self, resource: Option<&URI>, position: Option<&Position>, section: &str) -> Rc<dyn ConfigurationValue>;

    /// Atualiza o valor da configuração do recurso no local efetivo.
    ///
    /// - Sem `configuration_target`, o alvo é derivado de onde a configuração está definida.
    /// - Se os overrides de linguagem do recurso contêm a configuração, é ela que se atualiza.
    fn update_value(&self, resource: Option<&URI>, key: &str, value: Dynamic, configuration_target: Option<ConfigurationTarget>) -> Promise<()>;
}

service_id!(pub I_TEXT_RESOURCE_CONFIGURATION_SERVICE: dyn ITextResourceConfigurationService = "textResourceConfigurationService");

/// `ITextResourcePropertiesService`.
pub trait ITextResourcePropertiesService {
    /// Os caracteres de fim de linha do recurso dado.
    fn get_eol(&self, resource: &URI, language: Option<&str>) -> String;
}

service_id!(pub I_TEXT_RESOURCE_PROPERTIES_SERVICE: dyn ITextResourcePropertiesService = "textResourcePropertiesService");

/// `{ resource, overrideIdentifier }`.
fn overrides_of(resource: Option<&URI>, override_identifier: Option<&str>) -> ConfigurationOverrides {
    ConfigurationOverrides { resource: resource.cloned(), override_identifier: override_identifier.map(JsString::from) }
}

/// A resolução da linguagem de um recurso (`getLanguage`), compartilhada entre o serviço e os eventos de
/// mudança que ele emite.
#[derive(Clone)]
struct LanguageResolver {
    model_service: Rc<dyn IModelService>,
    language_service: Rc<dyn ILanguageService>,
}

impl LanguageResolver {
    /// `getLanguage(resource, position)`.
    fn get_language(&self, resource: &URI, position: Option<&Position>) -> Option<String> {
        if let Some(model) = self.model_service.get_model(resource) {
            return Some(match position {
                Some(position) => model.get_language_id_at_position(position.line_number, position.column),
                None => model.get_language_id(),
            });
        }
        self.language_service.guess_language_id_by_filepath_or_first_line(Some(resource), None)
    }
}

/// O objeto que `toResourceConfigurationChangeEvent` devolve.
struct ResourceConfigurationChangeEvent {
    configuration_change_event: Rc<ConfigurationChangeEvent>,
    languages: LanguageResolver,
}

impl ITextResourceConfigurationChangeEvent for ResourceConfigurationChangeEvent {
    fn affected_keys(&self) -> &[JsString] {
        &self.configuration_change_event.affected_keys
    }

    fn affects_configuration(&self, resource: Option<&URI>, section: &str) -> bool {
        let override_identifier = resource.and_then(|resource| self.languages.get_language(resource, None));
        if self.configuration_change_event.affects_configuration(section, Some(&overrides_of(resource, override_identifier.as_deref()))) {
            return true;
        }
        match override_identifier.filter(|identifier| !identifier.is_empty()) {
            //TODO@sandy081 workaround for https://github.com/microsoft/vscode/issues/240410
            Some(identifier) => self.configuration_change_event.affected_keys.contains(&JsString::from(format!("[{identifier}]"))),
            None => false,
        }
    }
}

/// `deriveConfigurationTarget`: o alvo onde a chave já está definida. Com linguagem, procura primeiro o
/// override nas camadas (da mais específica para a menos); depois o valor. Sem nada definido, é o usuário local.
fn derive_configuration_target(configuration_value: &dyn ConfigurationValue, language: Option<&str>) -> ConfigurationTarget {
    let layers: [(ConfigurationTarget, Option<InspectValue>); 5] = [
        (ConfigurationTarget::Memory, configuration_value.memory()),
        (ConfigurationTarget::WorkspaceFolder, configuration_value.workspace_folder()),
        (ConfigurationTarget::Workspace, configuration_value.workspace()),
        (ConfigurationTarget::UserRemote, configuration_value.user_remote()),
        (ConfigurationTarget::UserLocal, configuration_value.user_local()),
    ];
    let defines = |read: fn(&InspectValue) -> Dynamic| {
        layers.iter().find(|(_, layer)| layer.as_ref().is_some_and(|layer| !read(layer).is_undefined())).map(|(target, _)| *target)
    };
    if language.is_some_and(|language| !language.is_empty())
        && let Some(target) = defines(InspectValue::override_value)
    {
        return target;
    }
    // A camada `userLocal` fecha a lista: sem valor em nenhuma, ela também é o alvo padrão.
    defines(InspectValue::value).unwrap_or(ConfigurationTarget::UserLocal)
}

/// `TextResourceConfigurationService`.
pub struct TextResourceConfigurationService {
    base: Rc<Disposable>,
    on_did_change_configuration: Rc<Emitter<Rc<dyn ITextResourceConfigurationChangeEvent>>>,
    configuration_service: Rc<dyn IConfigurationService>,
    languages: LanguageResolver,
}

impl Injectable for TextResourceConfigurationService {
    type Args = ();
    type Deps = (Rc<dyn IConfigurationService>, Rc<dyn IModelService>, Rc<dyn ILanguageService>);

    fn create(_: (), (configuration_service, model_service, language_service): Self::Deps) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change_configuration: Rc<Emitter<Rc<dyn ITextResourceConfigurationChangeEvent>>> = Emitter::new();
        base.register(on_did_change_configuration.clone());
        let languages = LanguageResolver { model_service, language_service };
        base.register(configuration_service.on_did_change_configuration().listen({
            let on_did_change_configuration = on_did_change_configuration.clone();
            let languages = languages.clone();
            move |configuration_change_event| {
                on_did_change_configuration.fire(Rc::new(ResourceConfigurationChangeEvent {
                    configuration_change_event: configuration_change_event.clone(),
                    languages: languages.clone(),
                }));
            }
        }));
        Rc::new(Self { base, on_did_change_configuration, configuration_service, languages })
    }
}

service_impl!(TextResourceConfigurationService => dyn ITextResourceConfigurationService, disposable);

impl IDisposable for TextResourceConfigurationService {
    fn dispose(&self) {
        IDisposable::dispose(&*self.base);
    }
}

impl TextResourceConfigurationService {
    /// `_getValue`.
    fn get_value_in(&self, resource: Option<&URI>, position: Option<&Position>, section: Option<&str>) -> Dynamic {
        let language = resource.and_then(|resource| self.languages.get_language(resource, position));
        self.configuration_service.get_value(section, Some(&overrides_of(resource, language.as_deref())))
    }
}

impl ITextResourceConfigurationService for TextResourceConfigurationService {
    fn on_did_change_configuration(&self) -> Event<Rc<dyn ITextResourceConfigurationChangeEvent>> {
        self.on_did_change_configuration.event()
    }

    fn get_value(&self, resource: Option<&URI>, section: Option<&str>) -> Dynamic {
        self.get_value_in(resource, None, section)
    }

    fn get_value_at(&self, resource: Option<&URI>, position: Option<&Position>, section: Option<&str>) -> Dynamic {
        match section {
            Some(_) => self.get_value_in(resource, position, section),
            None => self.get_value_in(resource, None, None),
        }
    }

    fn inspect(&self, resource: Option<&URI>, position: Option<&Position>, section: &str) -> Rc<dyn ConfigurationValue> {
        let language = resource.and_then(|resource| self.languages.get_language(resource, position));
        self.configuration_service.inspect(section, &overrides_of(resource, language.as_deref()))
    }

    fn update_value(&self, resource: Option<&URI>, key: &str, value: Dynamic, configuration_target: Option<ConfigurationTarget>) -> Promise<()> {
        let language = resource.and_then(|resource| self.languages.get_language(resource, None));
        let configuration_value = self.configuration_service.inspect(key, &overrides_of(resource, language.as_deref()));
        let configuration_target = configuration_target.unwrap_or_else(|| derive_configuration_target(&*configuration_value, language.as_deref()));
        let override_identifier = language.as_deref().filter(|language| {
            !language.is_empty() && configuration_value.override_identifiers().is_some_and(|identifiers| identifiers.contains(&JsString::from(*language)))
        });
        self.configuration_service.update_value(
            key,
            value,
            Some(update_overrides_of(&overrides_of(resource, override_identifier))),
            Some(configuration_target),
        )
    }
}
