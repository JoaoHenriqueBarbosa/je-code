//! Porte de `src/vs/platform/configuration/common/configurationRegistry.ts`: o registro global das
//! propriedades de configuração (com esquema, escopo, política e valores padrão por extensão).
//!
//! Representação:
//! - O registro global (`Registry.as<IConfigurationRegistry>(Extensions.Configuration)`) é um
//!   singleton por thread, [`configuration_registry`]: o porte roda a UI num thread só (ver
//!   `docs/design.md`), e cada thread de teste do `cargo test` enxerga um registro próprio.
//! - Um nó de configuração é [`ConfigurationNode`] (identidade por `Rc`, como o objeto do original) e
//!   uma propriedade é [`PropertySchema`]: os campos de esquema JSON e as chaves do esquema que o
//!   original lê e escreve (`default`, `scope`, `restricted`, `tags`, `policy`, `experiment`...) ficam
//!   num objeto dinâmico (`schema`), e os campos que só o registro conhece (`section`, `source`,
//!   `defaultDefaultValue`, `defaultValueSource`) são tipados.
//! - `allSettings`, `windowSettings` etc. são os dicionários de esquema que alimentam a validação do
//!   `settings.json`; cada um guarda o objeto de esquema da propriedade, que continua vivo (uma
//!   mudança posterior na propriedade aparece neles, como no original).
//! - O esquema `resourceLanguage` ([`RESOURCE_LANGUAGE_SETTINGS_SCHEMA_ID`]) é registrado no
//!   `IJSONContributionRegistry` (`je_base::json_contribution_registry`) na criação do registro e de novo a cada
//!   `registerConfigurations`, `deregisterConfigurations` e `updateConfigurations`, como o original; o objeto
//!   registrado é o vivo, também acessível por [`ConfigurationRegistry::resource_language_settings_schema`].
//! - Políticas (`IPolicy`, `policyReference`), `agentHost`, `experiment` e `managedSettingsPresentation`
//!   ficam como valores dinâmicos dentro do esquema, porque `vs/base/common/policy.ts` não está portado.
//! - A ordem de iteração dos dicionários de propriedades é a de inserção (chaves que são inteiros
//!   canônicos não sobem para o início, como subiriam num objeto JavaScript).

use std::cell::RefCell;
use std::ops::Deref;
use std::rc::Rc;

use je_base::JsString;
use je_base::arrays;
use je_base::dynamic::{Dynamic, ObjectRef, Truthy};
use je_base::event::{Emitter, Event};
use je_base::json_contribution_registry::get_json_contribution_registry;
use je_base::lifecycle::mark_as_singleton;
use je_base::map::OrderedMap;
use je_base::nls::localize;
use je_base::types;

use super::{get_language_tag_setting_plain_key};
use crate::js::{Key, dynamic_to_text, for_in_entries, is_non_null_object, trim};


/// `EditPresentationTypes`.
pub mod edit_presentation_types {
    pub const MULTILINE: &str = "multilineText";
    pub const SINGLELINE: &str = "singlelineText";
}

/// `Extensions.Configuration`.
pub const EXTENSIONS_CONFIGURATION: &str = "base.contributions.configuration";

pub const RESOURCE_LANGUAGE_SETTINGS_SCHEMA_ID: &str = "vscode://schemas/settings/resourceLanguage";
pub const CONFIGURATION_DEFAULTS_SCHEMA_ID: &str = "vscode://schemas/settings/configurationDefaults";

/// `OVERRIDE_PROPERTY_PATTERN`: `^(\[([^\]]+)\])+$`.
pub const OVERRIDE_PROPERTY_PATTERN: &str = "^(\\[([^\\]]+)\\])+$";

/// Os ids de extensão que o `product.json` 1.143.1 lista em `defaultChatAgent` (`extensionId` e
/// `chatExtensionId`), em minúsculas. TODO(integração): ler do `product` quando ele for portado.
const EXTENSION_UNIFICATION_EXTENSION_IDS: [&str; 2] = ["github.copilot", "github.copilot-chat"];

/// `ConfigurationScope` (os valores numéricos são os do original).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ConfigurationScope {
    /// Configuração da aplicação, que só se define nas configurações do usuário do perfil padrão.
    Application = 1,
    /// Configuração da máquina, que só se define nas configurações do usuário local e remoto.
    Machine = 2,
    /// Da aplicação e da máquina: perfil padrão e usuário remoto.
    ApplicationMachine = 3,
    /// Da janela: usuário ou workspace.
    Window = 4,
    /// Do recurso: usuário, workspace ou pasta.
    Resource = 5,
    /// Do recurso, e também configurável por linguagem.
    LanguageOverridable = 6,
    /// Da máquina, e também configurável no workspace ou na pasta.
    MachineOverridable = 7,
}

impl ConfigurationScope {
    pub fn from_number(value: f64) -> Option<Self> {
        Some(match value as i64 {
            1 => Self::Application,
            2 => Self::Machine,
            3 => Self::ApplicationMachine,
            4 => Self::Window,
            5 => Self::Resource,
            6 => Self::LanguageOverridable,
            7 => Self::MachineOverridable,
            _ => return None,
        })
    }

    /// O valor numérico que o esquema guarda em `scope`.
    pub fn to_dynamic(self) -> Dynamic {
        Dynamic::Number(f64::from(self as u8))
    }

    /// O `scope` de um esquema; `undefined` e valores desconhecidos dão `None`.
    pub fn from_dynamic(value: &Dynamic) -> Option<Self> {
        match value {
            Dynamic::Number(n) if n.fract() == 0.0 => Self::from_number(*n),
            _ => None,
        }
    }
}

/// `IExtensionInfo`.
#[derive(Clone, Debug, PartialEq)]
pub struct ExtensionInfo {
    pub id: String,
    pub display_name: Option<String>,
}

impl ExtensionInfo {
    pub fn new(id: impl Into<String>) -> Self {
        Self { id: id.into(), display_name: None }
    }
}

/// `ConfigurationDefaultSource`: uma extensão ou um texto livre.
#[derive(Clone, Debug, PartialEq)]
pub enum ConfigurationDefaultSource {
    Extension(ExtensionInfo),
    Text(String),
}

/// `isConfigurationDefaultSourceEquals`.
pub fn is_configuration_default_source_equals(a: Option<&ConfigurationDefaultSource>, b: Option<&ConfigurationDefaultSource>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(ConfigurationDefaultSource::Text(a)), Some(ConfigurationDefaultSource::Text(b))) => a == b,
        (Some(ConfigurationDefaultSource::Extension(a)), Some(ConfigurationDefaultSource::Extension(b))) => a.id == b.id,
        _ => false,
    }
}

/// `getConfigurationExperimentName`: o nome do experimento, `config.<chave>` por padrão.
pub fn get_configuration_experiment_name(key: impl Key, experiment: &Dynamic) -> JsString {
    let name = if is_non_null_object(experiment) { experiment.get_str("name") } else { Dynamic::Undefined };
    match name {
        Dynamic::Undefined | Dynamic::Null => JsString::from(format!("config.{}", key.to_js())),
        Dynamic::String(name) => name,
        _ => JsString::from(format!("config.{}", key.to_js())),
    }
}

/// A origem de cada chave de um valor padrão de objeto (o `Map<string, ConfigurationDefaultSource>`).
pub type SourceMap = Rc<RefCell<OrderedMap<JsString, ConfigurationDefaultSource>>>;

/// `ConfigurationDefaultValueSource`: a origem de um valor padrão, ou o mapa de origens por chave.
#[derive(Clone)]
pub enum ConfigurationDefaultValueSource {
    Source(ConfigurationDefaultSource),
    Map(SourceMap),
}

/// `IConfigurationDefaults`. Tem identidade (`Rc`): o registro remove o que recebe por identidade.
#[derive(Clone)]
pub struct ConfigurationDefaults(Rc<ConfigurationDefaultsData>);

pub struct ConfigurationDefaultsData {
    pub overrides: Dynamic,
    pub source: Option<ConfigurationDefaultSource>,
    pub donot_cache: bool,
    pub prevent_experiment_override: bool,
}

impl ConfigurationDefaults {
    pub fn new(overrides: Dynamic, source: Option<ConfigurationDefaultSource>) -> Self {
        Self(Rc::new(ConfigurationDefaultsData { overrides, source, donot_cache: false, prevent_experiment_override: false }))
    }

    pub fn ptr_eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl Deref for ConfigurationDefaults {
    type Target = ConfigurationDefaultsData;

    fn deref(&self) -> &ConfigurationDefaultsData {
        &self.0
    }
}

/// `IConfigurationDefaultOverrideValue`.
#[derive(Clone)]
pub struct ConfigurationDefaultOverrideValue {
    pub value: Dynamic,
    pub source: Option<ConfigurationDefaultValueSource>,
}

#[derive(Clone)]
struct ConfigurationDefaultOverride {
    value: Dynamic,
    source: Option<ConfigurationDefaultSource>,
}

struct DefaultOverridesEntry {
    configuration_default_overrides: Vec<ConfigurationDefaultOverride>,
    configuration_default_override_value: Option<ConfigurationDefaultOverrideValue>,
}

/// Seção (o nó) a que uma propriedade registrada pertence.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Section {
    pub id: Option<String>,
    pub title: Option<String>,
    pub order: Option<f64>,
    pub extension_info: Option<ExtensionInfo>,
}

#[derive(Clone)]
struct PropertyMeta {
    section: Option<Section>,
    default_default_value: Dynamic,
    source: Option<ConfigurationDefaultSource>,
    default_value_source: Option<ConfigurationDefaultValueSource>,
}

/// `IRegisteredConfigurationPropertySchema` (e `IConfigurationPropertySchema`). Identidade por `Rc`.
#[derive(Clone)]
pub struct PropertySchema(Rc<PropertyData>);

pub struct PropertyData {
    /// Os campos de esquema JSON e as chaves que o registro lê e escreve (`type`, `default`, `scope`...).
    pub schema: ObjectRef,
    meta: RefCell<PropertyMeta>,
}

impl Deref for PropertySchema {
    type Target = PropertyData;

    fn deref(&self) -> &PropertyData {
        &self.0
    }
}

impl PropertySchema {
    pub fn new(schema: ObjectRef) -> Self {
        Self(Rc::new(PropertyData {
            schema,
            meta: RefCell::new(PropertyMeta { section: None, default_default_value: Dynamic::Undefined, source: None, default_value_source: None }),
        }))
    }

    /// Uma propriedade a partir dos pares `(campo, valor)`.
    pub fn from_entries<K: Into<JsString>>(entries: impl IntoIterator<Item = (K, Dynamic)>) -> Self {
        Self::new(ObjectRef::from_entries(entries))
    }

    pub fn ptr_eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }

    /// O objeto de esquema como valor dinâmico (a forma que `allSettings` guarda).
    pub fn json(&self) -> Dynamic {
        Dynamic::Object(self.schema.clone())
    }

    /// `property[field]`.
    pub fn get(&self, field: &str) -> Dynamic {
        self.schema.get(&JsString::from(field)).unwrap_or(Dynamic::Undefined)
    }

    /// `property[field] = value`.
    pub fn set(&self, field: &str, value: Dynamic) {
        self.schema.set(&JsString::from(field), value);
    }

    /// `property.hasOwnProperty(field)`.
    pub fn has_own(&self, field: &str) -> bool {
        self.schema.has_own(&JsString::from(field))
    }

    /// `property.default`.
    pub fn default_value(&self) -> Dynamic {
        self.get("default")
    }

    /// `property.scope`.
    pub fn scope(&self) -> Option<ConfigurationScope> {
        ConfigurationScope::from_dynamic(&self.get("scope"))
    }

    /// `!!property.restricted`.
    pub fn restricted(&self) -> bool {
        self.get("restricted").is_truthy()
    }

    pub fn section(&self) -> Option<Section> {
        self.meta.borrow().section.clone()
    }

    pub fn set_section(&self, section: Option<Section>) {
        self.meta.borrow_mut().section = section;
    }

    /// `property.defaultDefaultValue`.
    pub fn default_default_value(&self) -> Dynamic {
        self.meta.borrow().default_default_value.clone()
    }

    pub fn set_default_default_value(&self, value: Dynamic) {
        self.meta.borrow_mut().default_default_value = value;
    }

    /// `property.source`.
    pub fn source(&self) -> Option<ConfigurationDefaultSource> {
        self.meta.borrow().source.clone()
    }

    pub fn set_source(&self, source: Option<ConfigurationDefaultSource>) {
        self.meta.borrow_mut().source = source;
    }

    /// `property.defaultValueSource`.
    pub fn default_value_source(&self) -> Option<ConfigurationDefaultValueSource> {
        self.meta.borrow().default_value_source.clone()
    }

    pub fn set_default_value_source(&self, source: Option<ConfigurationDefaultValueSource>) {
        self.meta.borrow_mut().default_value_source = source;
    }

    /// `property.policy?.name` (vazio vale como ausente).
    pub fn policy_name(&self) -> Option<String> {
        name_of(&self.get("policy"))
    }

    /// `property.policyReference?.name`.
    pub fn policy_reference_name(&self) -> Option<String> {
        name_of(&self.get("policyReference"))
    }
}

fn name_of(value: &Dynamic) -> Option<String> {
    if !is_non_null_object(value) {
        return None;
    }
    match value.get_str("name") {
        Dynamic::String(name) if !name.is_empty() => Some(name.to_string_lossy()),
        _ => None,
    }
}

/// Um dicionário de propriedades (`IStringDictionary<IRegisteredConfigurationPropertySchema>`) que o
/// registro expõe vivo: quem guarda o handle enxerga os registros seguintes.
#[derive(Clone, Default)]
pub struct PropertyDictionary(Rc<RefCell<OrderedMap<JsString, PropertySchema>>>);

impl PropertyDictionary {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, key: impl Key) -> Option<PropertySchema> {
        self.0.borrow().get(&key.to_js()).cloned()
    }

    pub fn contains(&self, key: impl Key) -> bool {
        self.0.borrow().contains_key(&key.to_js())
    }

    pub fn len(&self) -> usize {
        self.0.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.borrow().is_empty()
    }

    pub fn keys(&self) -> Vec<JsString> {
        self.0.borrow().keys().cloned().collect()
    }

    pub fn entries(&self) -> Vec<(JsString, PropertySchema)> {
        self.0.borrow().iter().map(|(k, v)| (k.clone(), v.clone())).collect()
    }

    pub fn set(&self, key: impl Key, property: PropertySchema) {
        self.0.borrow_mut().insert(key.to_js(), property);
    }

    pub fn remove(&self, key: impl Key) {
        self.0.borrow_mut().remove(&key.to_js());
    }

    /// `{ ...dictionary }`: uma cópia independente (as propriedades continuam as mesmas).
    pub fn snapshot(&self) -> PropertyDictionary {
        PropertyDictionary(Rc::new(RefCell::new(self.0.borrow().clone())))
    }
}

/// As propriedades de um nó (`IConfigurationNode.properties`).
pub type NodeProperties = OrderedMap<JsString, PropertySchema>;

/// `IConfigurationNode`.
#[derive(Default)]
pub struct ConfigurationNodeData {
    pub id: Option<String>,
    pub order: Option<f64>,
    /// `string | string[]`.
    pub type_: Option<Dynamic>,
    pub title: Option<String>,
    pub description: Option<String>,
    /// O registro remove daqui as propriedades que rejeita ou esconde (`included: false`).
    pub properties: RefCell<Option<NodeProperties>>,
    pub all_of: Vec<ConfigurationNode>,
    pub scope: Option<ConfigurationScope>,
    pub extension_info: Option<ExtensionInfo>,
    pub restricted_properties: Option<Vec<JsString>>,
}

impl ConfigurationNodeData {
    /// Define as propriedades a partir dos pares `(chave, propriedade)`.
    pub fn with_properties<K: Key>(self, properties: impl IntoIterator<Item = (K, PropertySchema)>) -> Self {
        let mut map = NodeProperties::new();
        for (key, property) in properties {
            map.insert(key.to_js(), property);
        }
        *self.properties.borrow_mut() = Some(map);
        self
    }
}

/// Um nó de configuração com identidade (o objeto do original).
#[derive(Clone)]
pub struct ConfigurationNode(Rc<ConfigurationNodeData>);

impl ConfigurationNode {
    pub fn new(data: ConfigurationNodeData) -> Self {
        Self(Rc::new(data))
    }

    pub fn ptr_eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }

    /// `Object.keys(node.properties)`, `[]` se o nó não tem propriedades.
    pub fn property_keys(&self) -> Vec<JsString> {
        self.properties.borrow().as_ref().map(|p| p.keys().cloned().collect()).unwrap_or_default()
    }

    /// `node.properties?.[key]`.
    pub fn property(&self, key: impl Key) -> Option<PropertySchema> {
        self.properties.borrow().as_ref().and_then(|p| p.get(&key.to_js()).cloned())
    }

    /// `delete node.properties[key]`.
    pub fn delete_property(&self, key: &JsString) {
        if let Some(properties) = self.properties.borrow_mut().as_mut() {
            properties.remove(key);
        }
    }

    /// `node.properties ??= {}; node.properties[key] = property`.
    pub fn set_property(&self, key: impl Key, property: PropertySchema) {
        self.properties.borrow_mut().get_or_insert_with(NodeProperties::new).insert(key.to_js(), property);
    }
}

impl Deref for ConfigurationNode {
    type Target = ConfigurationNodeData;

    fn deref(&self) -> &ConfigurationNodeData {
        &self.0
    }
}

/// `IConfigurationDelta`.
#[derive(Clone, Default)]
pub struct ConfigurationDelta {
    pub removed_defaults: Option<Vec<ConfigurationDefaults>>,
    pub removed_configurations: Option<Vec<ConfigurationNode>>,
    pub added_defaults: Option<Vec<ConfigurationDefaults>>,
    pub added_configurations: Option<Vec<ConfigurationNode>>,
}

/// O evento `onDidUpdateConfiguration`: `{ properties, defaultsOverrides? }`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ConfigurationUpdate {
    pub properties: Vec<JsString>,
    pub defaults_overrides: Option<bool>,
}

/// Um `Set<string>` na ordem de inserção.
type StringSet = OrderedMap<JsString, ()>;

/// Dicionários de esquema de `allSettings`, `windowSettings` etc.
#[derive(Clone)]
pub struct SettingsSchema {
    pub properties: ObjectRef,
    pub pattern_properties: ObjectRef,
}

impl SettingsSchema {
    fn new() -> Self {
        Self { properties: ObjectRef::new(), pattern_properties: ObjectRef::new() }
    }
}

/// Os sete dicionários de esquema do original (`allSettings`, `applicationSettings`...).
#[derive(Clone)]
pub struct SettingsSchemas {
    pub all_settings: SettingsSchema,
    pub application_settings: SettingsSchema,
    pub application_machine_settings: SettingsSchema,
    pub machine_settings: SettingsSchema,
    pub machine_overridable_settings: SettingsSchema,
    pub window_settings: SettingsSchema,
    pub resource_settings: SettingsSchema,
}

impl SettingsSchemas {
    fn new() -> Self {
        Self {
            all_settings: SettingsSchema::new(),
            application_settings: SettingsSchema::new(),
            application_machine_settings: SettingsSchema::new(),
            machine_settings: SettingsSchema::new(),
            machine_overridable_settings: SettingsSchema::new(),
            window_settings: SettingsSchema::new(),
            resource_settings: SettingsSchema::new(),
        }
    }

    fn all(&self) -> [&SettingsSchema; 7] {
        [
            &self.all_settings,
            &self.application_settings,
            &self.application_machine_settings,
            &self.machine_settings,
            &self.machine_overridable_settings,
            &self.window_settings,
            &self.resource_settings,
        ]
    }
}

thread_local! {
    static SETTINGS_SCHEMAS: SettingsSchemas = SettingsSchemas::new();
    static CONFIGURATION_REGISTRY: Rc<ConfigurationRegistry> = Rc::new(ConfigurationRegistry::new());
}

/// Os dicionários de esquema do thread (`allSettings` e companhia).
pub fn settings_schemas() -> SettingsSchemas {
    SETTINGS_SCHEMAS.with(Clone::clone)
}

/// `Registry.as<IConfigurationRegistry>(Extensions.Configuration)`.
pub fn configuration_registry() -> Rc<ConfigurationRegistry> {
    CONFIGURATION_REGISTRY.with(Rc::clone)
}

fn empty_object() -> ObjectRef {
    ObjectRef::new()
}

/// `ConfigurationRegistry`.
pub struct ConfigurationRegistry {
    registered_configuration_defaults: RefCell<Vec<ConfigurationDefaults>>,
    configuration_defaults_overrides: RefCell<OrderedMap<JsString, Rc<RefCell<DefaultOverridesEntry>>>>,
    default_language_configuration_overrides_node: ConfigurationNode,
    configuration_contributors: RefCell<Vec<ConfigurationNode>>,
    configuration_properties: PropertyDictionary,
    policy_configurations: RefCell<OrderedMap<String, JsString>>,
    policy_reference_configurations: RefCell<OrderedMap<String, StringSet>>,
    agent_host_sync_configurations: RefCell<OrderedMap<JsString, Dynamic>>,
    /// Chaves escondidas (`included: false`) por nó: o registro as tira de `properties`, e a remoção do
    /// nó não teria outro jeito de achá-las.
    excluded_configuration_keys: RefCell<Vec<(ConfigurationNode, StringSet)>>,
    excluded_configuration_properties: PropertyDictionary,
    resource_language_settings_schema: ObjectRef,
    override_identifiers: RefCell<StringSet>,
    on_did_schema_change: Rc<Emitter<()>>,
    on_did_update_configuration: Rc<Emitter<ConfigurationUpdate>>,
}

impl ConfigurationRegistry {
    fn new() -> Self {
        let default_language_configuration_overrides_node = ConfigurationNode::new(ConfigurationNodeData {
            id: Some("defaultOverrides".to_string()),
            title: Some(localize("defaultLanguageConfigurationOverrides.title", "Default Language Configuration Overrides", &[])),
            properties: RefCell::new(Some(NodeProperties::new())),
            ..Default::default()
        });
        let resource_language_settings_schema = ObjectRef::from_entries([
            ("properties", Dynamic::Object(empty_object())),
            ("patternProperties", Dynamic::Object(empty_object())),
            ("additionalProperties", Dynamic::Bool(true)),
            ("allowTrailingCommas", Dynamic::Bool(true)),
            ("allowComments", Dynamic::Bool(true)),
        ]);
        let registry = Self {
            registered_configuration_defaults: RefCell::new(Vec::new()),
            configuration_defaults_overrides: RefCell::new(OrderedMap::new()),
            configuration_contributors: RefCell::new(vec![default_language_configuration_overrides_node.clone()]),
            default_language_configuration_overrides_node,
            configuration_properties: PropertyDictionary::new(),
            policy_configurations: RefCell::new(OrderedMap::new()),
            policy_reference_configurations: RefCell::new(OrderedMap::new()),
            agent_host_sync_configurations: RefCell::new(OrderedMap::new()),
            excluded_configuration_keys: RefCell::new(Vec::new()),
            excluded_configuration_properties: PropertyDictionary::new(),
            resource_language_settings_schema,
            override_identifiers: RefCell::new(StringSet::new()),
            on_did_schema_change: mark_as_singleton(Emitter::new()),
            on_did_update_configuration: mark_as_singleton(Emitter::new()),
        };
        registry.register_resource_language_settings_schema();
        registry.register_override_property_pattern_key();
        registry
    }

    /// `contributionRegistry.registerSchema(resourceLanguageSettingsSchemaId, this.resourceLanguageSettingsSchema)`.
    fn register_resource_language_settings_schema(&self) {
        get_json_contribution_registry().register_schema(RESOURCE_LANGUAGE_SETTINGS_SCHEMA_ID, self.resource_language_settings_schema(), None);
    }

    /// `onDidSchemaChange`.
    pub fn on_did_schema_change(&self) -> Event<()> {
        self.on_did_schema_change.event()
    }

    /// `onDidUpdateConfiguration`.
    pub fn on_did_update_configuration(&self) -> Event<ConfigurationUpdate> {
        self.on_did_update_configuration.event()
    }

    /// O esquema `vscode://schemas/settings/resourceLanguage`.
    pub fn resource_language_settings_schema(&self) -> Dynamic {
        Dynamic::Object(self.resource_language_settings_schema.clone())
    }

    fn schema_properties(&self) -> ObjectRef {
        self.resource_language_settings_schema.get(&JsString::from("properties")).map_or_else(ObjectRef::new, |p| match p {
            Dynamic::Object(o) => o,
            _ => ObjectRef::new(),
        })
    }

    fn fire_update(&self, properties: StringSet, defaults_overrides: Option<bool>) {
        self.on_did_schema_change.fire(());
        self.on_did_update_configuration.fire(ConfigurationUpdate { properties: properties.keys().cloned().collect(), defaults_overrides });
    }

    /// `registerConfiguration(configuration)`: valida a propriedade e devolve o nó.
    pub fn register_configuration(&self, configuration: ConfigurationNode) -> ConfigurationNode {
        self.register_configuration_with(configuration, true)
    }

    /// `registerConfiguration(configuration, validate)`.
    pub fn register_configuration_with(&self, configuration: ConfigurationNode, validate: bool) -> ConfigurationNode {
        self.register_configurations(std::slice::from_ref(&configuration), validate);
        configuration
    }

    /// `registerConfigurations(configurations, validate)`.
    pub fn register_configurations(&self, configurations: &[ConfigurationNode], validate: bool) {
        let mut properties = StringSet::new();
        self.do_register_configurations(configurations, validate, &mut properties);
        self.register_resource_language_settings_schema();
        self.fire_update(properties, None);
    }

    /// `deregisterConfigurations`.
    pub fn deregister_configurations(&self, configurations: &[ConfigurationNode]) {
        let mut properties = StringSet::new();
        self.do_deregister_configurations(configurations, &mut properties);
        self.register_resource_language_settings_schema();
        self.fire_update(properties, None);
    }

    /// `updateConfigurations({ add, remove })`.
    pub fn update_configurations(&self, add: &[ConfigurationNode], remove: &[ConfigurationNode]) {
        let mut properties = StringSet::new();
        self.do_deregister_configurations(remove, &mut properties);
        self.do_register_configurations(add, false, &mut properties);
        self.register_resource_language_settings_schema();
        self.fire_update(properties, None);
    }

    /// `registerDefaultConfigurations`.
    pub fn register_default_configurations(&self, configuration_defaults: &[ConfigurationDefaults]) {
        let mut properties = StringSet::new();
        self.do_register_default_configurations(configuration_defaults, &mut properties);
        self.fire_update(properties, Some(true));
    }

    fn entry_for(&self, key: &JsString) -> Rc<RefCell<DefaultOverridesEntry>> {
        let mut map = self.configuration_defaults_overrides.borrow_mut();
        if let Some(entry) = map.get(key) {
            return entry.clone();
        }
        let entry = Rc::new(RefCell::new(DefaultOverridesEntry { configuration_default_overrides: Vec::new(), configuration_default_override_value: None }));
        map.insert(key.clone(), entry.clone());
        entry
    }

    /// `this.configurationProperties[key] ?? (excluded?.experiment ? excluded : undefined)`.
    fn property_or_experiment(&self, key: &JsString) -> Option<PropertySchema> {
        self.configuration_properties.get(key).or_else(|| self.excluded_configuration_properties.get(key).filter(|p| p.get("experiment").is_truthy()))
    }

    fn do_register_default_configurations(&self, configuration_defaults: &[ConfigurationDefaults], bucket: &mut StringSet) {
        self.registered_configuration_defaults.borrow_mut().extend(configuration_defaults.iter().cloned());

        let mut override_identifiers: Vec<JsString> = Vec::new();

        for defaults in configuration_defaults {
            let source = defaults.source.clone();
            for (key, value) in for_in_entries(&defaults.overrides) {
                bucket.insert(key.clone(), ());

                let entry = self.entry_for(&key);
                entry.borrow_mut().configuration_default_overrides.push(ConfigurationDefaultOverride { value: value.clone(), source: source.clone() });
                let existing = entry.borrow().configuration_default_override_value.clone();

                // Configuration defaults for Override Identifiers
                if is_override_property(&key) {
                    let Some(new_default_override) = self.merge_default_configurations_for_override_identifier(&value, source.as_ref(), existing.as_ref()) else {
                        continue;
                    };
                    entry.borrow_mut().configuration_default_override_value = Some(new_default_override.clone());
                    self.update_default_override_property(&key, &new_default_override, source.clone());
                    override_identifiers.extend(override_identifiers_from_key(&key));
                }
                // Configuration defaults for Configuration Properties
                else {
                    let Some(new_default_override) = self.merge_default_configurations_for_configuration_property(&key, value.clone(), source.as_ref(), existing.as_ref()) else {
                        continue;
                    };
                    entry.borrow_mut().configuration_default_override_value = Some(new_default_override);
                    if let Some(property) = self.property_or_experiment(&key) {
                        self.update_property_default_value(&key, &property);
                        if property.get("included") != Dynamic::Bool(false) {
                            self.update_schema(&key, &property);
                        }
                    }
                }
            }
        }

        self.do_register_override_identifiers(&override_identifiers);
    }

    /// `deregisterDefaultConfigurations`.
    pub fn deregister_default_configurations(&self, default_configurations: &[ConfigurationDefaults]) {
        let mut properties = StringSet::new();
        self.do_deregister_default_configurations(default_configurations, &mut properties);
        self.fire_update(properties, Some(true));
    }

    fn do_deregister_default_configurations(&self, default_configurations: &[ConfigurationDefaults], bucket: &mut StringSet) {
        for default_configuration in default_configurations {
            let mut registered = self.registered_configuration_defaults.borrow_mut();
            if let Some(index) = registered.iter().position(|d| d.ptr_eq(default_configuration)) {
                registered.remove(index);
            }
        }

        for defaults in default_configurations {
            let source = defaults.source.clone();
            for (key, override_value) in for_in_entries(&defaults.overrides) {
                let Some(entry) = self.configuration_defaults_overrides.borrow().get(&key).cloned() else {
                    continue;
                };

                let index = entry.borrow().configuration_default_overrides.iter().position(|o| match &source {
                    Some(source) => is_configuration_default_source_equals(o.source.as_ref(), Some(source)),
                    None => o.value.strict_equals(&override_value),
                });
                let Some(index) = index else {
                    continue;
                };

                entry.borrow_mut().configuration_default_overrides.remove(index);
                if entry.borrow().configuration_default_overrides.is_empty() {
                    self.configuration_defaults_overrides.borrow_mut().remove(&key);
                }

                let remaining = entry.borrow().configuration_default_overrides.clone();
                if is_override_property(&key) {
                    let mut value: Option<ConfigurationDefaultOverrideValue> = None;
                    for o in &remaining {
                        value = self.merge_default_configurations_for_override_identifier(&o.value, o.source.as_ref(), value.as_ref());
                    }
                    match value.filter(|v| !types::is_empty_object(&v.value)) {
                        Some(value) => {
                            entry.borrow_mut().configuration_default_override_value = Some(value.clone());
                            self.update_default_override_property(&key, &value, source.clone());
                        }
                        None => {
                            self.configuration_defaults_overrides.borrow_mut().remove(&key);
                            self.configuration_properties.remove(&key);
                            self.default_language_configuration_overrides_node.delete_property(&key);
                        }
                    }
                } else {
                    let mut value: Option<ConfigurationDefaultOverrideValue> = None;
                    for o in &remaining {
                        value = self.merge_default_configurations_for_configuration_property(&key, o.value.clone(), o.source.as_ref(), value.as_ref());
                    }
                    entry.borrow_mut().configuration_default_override_value = value;
                    if let Some(property) = self.property_or_experiment(&key) {
                        self.update_property_default_value(&key, &property);
                        if property.get("included") != Dynamic::Bool(false) {
                            self.update_schema(&key, &property);
                        }
                    }
                }
                bucket.insert(key.clone(), ());
            }
        }
        self.update_override_property_pattern_key();
    }

    fn update_default_override_property(&self, key: &JsString, new_default_override: &ConfigurationDefaultOverrideValue, source: Option<ConfigurationDefaultSource>) {
        let node = &self.default_language_configuration_overrides_node;
        let property = PropertySchema::from_entries([
            ("type", Dynamic::string("object")),
            ("default", new_default_override.value.clone()),
            (
                "description",
                Dynamic::from(localize(
                    "defaultLanguageConfiguration.description",
                    "Configure settings to be overridden for {0}.",
                    &[&get_language_tag_setting_plain_key(key).to_string_lossy()],
                )),
            ),
            ("$ref", Dynamic::string(RESOURCE_LANGUAGE_SETTINGS_SCHEMA_ID)),
        ]);
        property.set_section(Some(Section { id: node.id.clone(), title: node.title.clone(), order: node.order, extension_info: node.extension_info.clone() }));
        property.set_default_default_value(new_default_override.value.clone());
        property.set_source(source.clone());
        property.set_default_value_source(source.map(ConfigurationDefaultValueSource::Source));
        self.configuration_properties.set(key, property.clone());
        node.set_property(key, property);
    }

    fn merge_default_configurations_for_override_identifier(
        &self,
        configuration_value_object: &Dynamic,
        value_source: Option<&ConfigurationDefaultSource>,
        existing_default_override: Option<&ConfigurationDefaultOverrideValue>,
    ) -> Option<ConfigurationDefaultOverrideValue> {
        let default_value = match existing_default_override.map(|e| &e.value) {
            Some(value) if value.is_truthy() => value.clone(),
            _ => Dynamic::Object(ObjectRef::new()),
        };
        let source: SourceMap = match existing_default_override.and_then(|e| e.source.as_ref()) {
            None => Rc::new(RefCell::new(OrderedMap::new())),
            Some(ConfigurationDefaultValueSource::Map(map)) => map.clone(),
            // This should not happen
            Some(ConfigurationDefaultValueSource::Source(_)) => {
                eprintln!("objectConfigurationSources is not a Map");
                return None;
            }
        };

        for property_key in configuration_value_object.own_keys() {
            let property_default_value = configuration_value_object.get(&property_key);
            let current = default_value.get(&property_key);

            let is_object_setting = types::is_object(&property_default_value) && (current.is_undefined() || types::is_object(&current));

            // If the default value is an object, merge the objects and store the source of each keys
            if is_object_setting {
                let merged = ObjectRef::new();
                if !current.is_nullish() {
                    for (k, v) in current.own_entries() {
                        merged.set(&k, v);
                    }
                }
                for (k, v) in property_default_value.own_entries() {
                    merged.set(&k, v);
                }
                crate::js::set_property(&default_value, &property_key, Dynamic::Object(merged));
                // Track the source of each value in the object
                if let Some(value_source) = value_source {
                    for object_key in property_default_value.own_keys() {
                        source.borrow_mut().insert(JsString::from(format!("{property_key}.{object_key}")), value_source.clone());
                    }
                }
            }
            // Primitive values are overridden
            else {
                crate::js::set_property(&default_value, &property_key, property_default_value);
                match value_source {
                    Some(value_source) => {
                        source.borrow_mut().insert(property_key.clone(), value_source.clone());
                    }
                    None => {
                        source.borrow_mut().remove(&property_key);
                    }
                }
            }
        }

        Some(ConfigurationDefaultOverrideValue { value: default_value, source: Some(ConfigurationDefaultValueSource::Map(source)) })
    }

    fn merge_default_configurations_for_configuration_property(
        &self,
        property_key: &JsString,
        value: Dynamic,
        values_source: Option<&ConfigurationDefaultSource>,
        existing_default_override: Option<&ConfigurationDefaultOverrideValue>,
    ) -> Option<ConfigurationDefaultOverrideValue> {
        let property = self.property_or_experiment(property_key);
        let existing_default_value = match existing_default_override.map(|e| e.value.clone()) {
            Some(value) if !value.is_nullish() => value,
            _ => property.as_ref().map_or(Dynamic::Undefined, PropertySchema::default_default_value),
        };
        let mut source: Option<ConfigurationDefaultValueSource> = values_source.cloned().map(ConfigurationDefaultValueSource::Source);
        let mut value = value;

        let is_object_setting = types::is_object(&value)
            && match &property {
                Some(property) => property.get("type") == Dynamic::string("object"),
                None => existing_default_value.is_undefined() || types::is_object(&existing_default_value),
            };

        // If the default value is an object, merge the objects and store the source of each keys
        if is_object_setting {
            let map: SourceMap = match existing_default_override.and_then(|e| e.source.as_ref()) {
                None => Rc::new(RefCell::new(OrderedMap::new())),
                Some(ConfigurationDefaultValueSource::Map(map)) => map.clone(),
                // This should not happen
                Some(ConfigurationDefaultValueSource::Source(_)) => {
                    eprintln!("defaultValueSource is not a Map");
                    return None;
                }
            };
            if let Some(values_source) = values_source {
                for object_key in value.own_keys() {
                    map.borrow_mut().insert(JsString::from(format!("{property_key}.{object_key}")), values_source.clone());
                }
            }
            source = Some(ConfigurationDefaultValueSource::Map(map));

            let merged = ObjectRef::new();
            if types::is_object(&existing_default_value) {
                for (k, v) in existing_default_value.own_entries() {
                    merged.set(&k, v);
                }
            }
            for (k, v) in value.own_entries() {
                merged.set(&k, v);
            }
            value = Dynamic::Object(merged);
        }

        Some(ConfigurationDefaultOverrideValue { value, source })
    }

    /// `deltaConfiguration`.
    pub fn delta_configuration(&self, delta: &ConfigurationDelta) {
        // defaults: remove
        let mut defaults_overrides = false;
        let mut properties = StringSet::new();
        if let Some(removed) = &delta.removed_defaults {
            self.do_deregister_default_configurations(removed, &mut properties);
            defaults_overrides = true;
        }
        // defaults: add
        if let Some(added) = &delta.added_defaults {
            self.do_register_default_configurations(added, &mut properties);
            defaults_overrides = true;
        }
        // configurations: remove
        if let Some(removed) = &delta.removed_configurations {
            self.do_deregister_configurations(removed, &mut properties);
        }
        // configurations: add
        if let Some(added) = &delta.added_configurations {
            self.do_register_configurations(added, false, &mut properties);
        }
        self.fire_update(properties, Some(defaults_overrides));
    }

    /// `notifyConfigurationSchemaUpdated`.
    pub fn notify_configuration_schema_updated(&self, _configurations: &[ConfigurationNode]) {
        self.on_did_schema_change.fire(());
    }

    /// `registerOverrideIdentifiers`.
    pub fn register_override_identifiers(&self, override_identifiers: &[JsString]) {
        self.do_register_override_identifiers(override_identifiers);
        self.on_did_schema_change.fire(());
    }

    fn do_register_override_identifiers(&self, override_identifiers: &[JsString]) {
        for identifier in override_identifiers {
            self.override_identifiers.borrow_mut().insert(identifier.clone(), ());
        }
        self.update_override_property_pattern_key();
    }

    fn do_register_configurations(&self, configurations: &[ConfigurationNode], validate: bool, bucket: &mut StringSet) {
        for configuration in configurations {
            self.validate_and_register_properties(
                configuration,
                validate,
                configuration.extension_info.as_ref(),
                configuration.restricted_properties.as_deref(),
                ConfigurationScope::Window,
                bucket,
            );

            self.configuration_contributors.borrow_mut().push(configuration.clone());
            self.register_json_configuration(configuration);
        }
    }

    fn do_deregister_configurations(&self, configurations: &[ConfigurationNode], bucket: &mut StringSet) {
        for configuration in configurations {
            self.deregister_configuration(configuration, bucket);
            let mut contributors = self.configuration_contributors.borrow_mut();
            if let Some(index) = contributors.iter().position(|c| c.ptr_eq(configuration)) {
                contributors.remove(index);
            }
        }
    }

    fn deregister_configuration(&self, configuration: &ConfigurationNode, bucket: &mut StringSet) {
        // Properties hidden with `included: false` are stripped from `configuration.properties` at
        // registration time, so the loop below cannot see them. Clean their mirroring entries and
        // experimental metadata using the side table recorded when they were excluded.
        let excluded_keys = {
            let mut table = self.excluded_configuration_keys.borrow_mut();
            table.iter().position(|(node, _)| node.ptr_eq(configuration)).map(|i| table.remove(i).1)
        };
        if let Some(excluded_keys) = excluded_keys {
            for key in excluded_keys.keys() {
                bucket.insert(key.clone(), ());
                self.agent_host_sync_configurations.borrow_mut().remove(key);
                if self.excluded_configuration_properties.get(key).is_some_and(|p| p.get("experiment").is_truthy()) {
                    self.excluded_configuration_properties.remove(key);
                }
            }
        }
        for key in configuration.property_keys() {
            bucket.insert(key.clone(), ());
            let property = self.configuration_properties.get(&key);
            if let Some(name) = property.as_ref().and_then(PropertySchema::policy_name) {
                self.policy_configurations.borrow_mut().remove(&name);
            }
            self.agent_host_sync_configurations.borrow_mut().remove(&key);
            if let Some(name) = property.as_ref().and_then(PropertySchema::policy_reference_name) {
                let mut references = self.policy_reference_configurations.borrow_mut();
                if let Some(refs) = references.get_mut(&name) {
                    refs.remove(&key);
                    if refs.is_empty() {
                        references.remove(&name);
                    }
                }
            }
            self.configuration_properties.remove(&key);
            if let Some(schema) = configuration.property(&key) {
                self.remove_from_schema(&key, &schema);
            }
        }
        for node in &configuration.all_of {
            self.deregister_configuration(node, bucket);
        }
    }

    fn validate_and_register_properties(
        &self,
        configuration: &ConfigurationNode,
        validate: bool,
        extension_info: Option<&ExtensionInfo>,
        restricted_properties: Option<&[JsString]>,
        scope: ConfigurationScope,
        bucket: &mut StringSet,
    ) {
        let scope = configuration.scope.unwrap_or(scope);
        for key in configuration.property_keys() {
            let Some(property) = configuration.property(&key) else {
                continue;
            };
            property.set_section(Some(Section {
                id: configuration.id.clone(),
                title: configuration.title.clone(),
                order: configuration.order,
                extension_info: configuration.extension_info.clone(),
            }));
            if validate && self.validate_property(&key, &property, extension_info.map(|e| e.id.as_str())).is_some() {
                configuration.delete_property(&key);
                continue;
            }

            property.set_source(extension_info.cloned().map(ConfigurationDefaultSource::Extension));

            // update default value
            property.set_default_default_value(property.default_value());
            self.update_property_default_value(&key, &property);

            // update scope
            if is_override_property(&key) {
                property.set("scope", Dynamic::Undefined); // No scope for overridable properties `[${identifier}]`
            } else {
                if property.get("scope").is_nullish() {
                    property.set("scope", scope.to_dynamic());
                }
                if property.get("restricted").is_nullish() {
                    property.set("restricted", Dynamic::Bool(restricted_properties.is_some_and(|r| r.contains(&key))));
                }
            }

            let has_on_exp_tag = |property: &PropertySchema| match property.get("tags") {
                Dynamic::Array(tags) => tags.to_vec().iter().any(|tag| matches!(tag, Dynamic::String(t) if t.to_lower_case() == "onexp")),
                _ => false,
            };
            if property.get("experiment").is_truthy() {
                if !has_on_exp_tag(&property) {
                    let tags = match property.get("tags") {
                        Dynamic::Array(tags) => tags,
                        _ => {
                            let tags = je_base::dynamic::ArrayRef::new(Vec::new());
                            property.set("tags", Dynamic::Array(tags.clone()));
                            tags
                        }
                    };
                    tags.push(Dynamic::string("onExP"));
                }
            } else if has_on_exp_tag(&property) {
                eprintln!("Invalid tag 'onExP' found for property '{key}'. Please use 'experiment' property instead.");
                property.set("experiment", Dynamic::object([("mode", Dynamic::string("startup"))]));
            }

            let excluded = property.has_own("included") && !property.get("included").is_truthy();
            let policy_name = property.policy_name();
            let policy_reference_name = property.policy_reference_name();
            let agent_host_sync = property.get("agentHost");

            if agent_host_sync.is_truthy() {
                self.agent_host_sync_configurations.borrow_mut().insert(key.clone(), agent_host_sync.clone());
            }

            if excluded {
                self.excluded_configuration_properties.set(&key, property.clone());
                if let Some(policy_name) = &policy_name {
                    self.policy_configurations.borrow_mut().insert(policy_name.clone(), key.clone());
                    bucket.insert(key.clone(), ());
                }
                if let Some(policy_reference_name) = &policy_reference_name {
                    self.add_policy_reference_configuration(policy_reference_name, &key);
                    bucket.insert(key.clone(), ());
                }
                if agent_host_sync.is_truthy() || property.get("experiment").is_truthy() {
                    // Hidden settings can participate in experiments and host
                    // mirroring without entering the Settings UI schemas.
                    bucket.insert(key.clone(), ());
                    let mut table = self.excluded_configuration_keys.borrow_mut();
                    match table.iter_mut().find(|(node, _)| node.ptr_eq(configuration)) {
                        Some((_, keys)) => {
                            keys.insert(key.clone(), ());
                        }
                        None => {
                            let mut keys = StringSet::new();
                            keys.insert(key.clone(), ());
                            table.push((configuration.clone(), keys));
                        }
                    }
                }
                configuration.delete_property(&key);
            } else {
                bucket.insert(key.clone(), ());
                if let Some(policy_name) = &policy_name {
                    self.policy_configurations.borrow_mut().insert(policy_name.clone(), key.clone());
                }
                if let Some(policy_reference_name) = &policy_reference_name {
                    self.add_policy_reference_configuration(policy_reference_name, &key);
                }
                self.configuration_properties.set(&key, property.clone());
                if !property.get("deprecationMessage").is_truthy() && property.get("markdownDeprecationMessage").is_truthy() {
                    // If not set, default deprecationMessage to the markdown source
                    property.set("deprecationMessage", property.get("markdownDeprecationMessage"));
                }
            }
        }
        for node in &configuration.all_of {
            self.validate_and_register_properties(node, validate, extension_info, restricted_properties, scope, bucket);
        }
    }

    fn add_policy_reference_configuration(&self, policy_name: &str, key: &JsString) {
        let mut references = self.policy_reference_configurations.borrow_mut();
        if !references.contains_key(&policy_name.to_string()) {
            references.insert(policy_name.to_string(), StringSet::new());
        }
        references.get_mut(&policy_name.to_string()).expect("acabou de inserir").insert(key.clone(), ());
    }

    /// `getConfigurations()` (um instantâneo; só para testes no original).
    pub fn get_configurations(&self) -> Vec<ConfigurationNode> {
        self.configuration_contributors.borrow().clone()
    }

    /// `getConfigurationProperties()`: o dicionário vivo.
    pub fn get_configuration_properties(&self) -> PropertyDictionary {
        self.configuration_properties.clone()
    }

    /// `getPolicyConfigurations()`: o nome da política e a chave da configuração dona dela.
    pub fn get_policy_configurations(&self) -> OrderedMap<String, JsString> {
        self.policy_configurations.borrow().clone()
    }

    /// `getPolicyReferenceConfigurations()`.
    pub fn get_policy_reference_configurations(&self) -> OrderedMap<String, Vec<JsString>> {
        let mut result = OrderedMap::new();
        for (name, keys) in self.policy_reference_configurations.borrow().iter() {
            result.insert(name.clone(), keys.keys().cloned().collect());
        }
        result
    }

    /// `getAgentHostSyncConfigurations()`.
    pub fn get_agent_host_sync_configurations(&self) -> OrderedMap<JsString, Dynamic> {
        self.agent_host_sync_configurations.borrow().clone()
    }

    /// `getExcludedConfigurationProperties()`: o dicionário vivo.
    pub fn get_excluded_configuration_properties(&self) -> PropertyDictionary {
        self.excluded_configuration_properties.clone()
    }

    /// `getRegisteredDefaultConfigurations()`.
    pub fn get_registered_default_configurations(&self) -> Vec<ConfigurationDefaults> {
        self.registered_configuration_defaults.borrow().clone()
    }

    /// `getConfigurationDefaultsOverrides()`.
    pub fn get_configuration_defaults_overrides(&self) -> OrderedMap<JsString, ConfigurationDefaultOverrideValue> {
        let mut result = OrderedMap::new();
        for (key, entry) in self.configuration_defaults_overrides.borrow().iter() {
            if let Some(value) = entry.borrow().configuration_default_override_value.clone() {
                result.insert(key.clone(), value);
            }
        }
        result
    }

    fn register_json_configuration(&self, configuration: &ConfigurationNode) {
        for key in configuration.property_keys() {
            if let Some(property) = configuration.property(&key) {
                self.update_schema(&key, &property);
            }
        }
        for node in &configuration.all_of {
            self.register_json_configuration(node);
        }
    }

    fn update_schema(&self, key: &JsString, property: &PropertySchema) {
        let schemas = settings_schemas();
        let json = property.json();
        schemas.all_settings.properties.set(key, json.clone());
        let target = match property.scope() {
            Some(ConfigurationScope::Application) => Some(&schemas.application_settings),
            Some(ConfigurationScope::Machine) => Some(&schemas.machine_settings),
            Some(ConfigurationScope::ApplicationMachine) => Some(&schemas.application_machine_settings),
            Some(ConfigurationScope::MachineOverridable) => Some(&schemas.machine_overridable_settings),
            Some(ConfigurationScope::Window) => Some(&schemas.window_settings),
            Some(ConfigurationScope::Resource) => Some(&schemas.resource_settings),
            Some(ConfigurationScope::LanguageOverridable) => {
                schemas.resource_settings.properties.set(key, json.clone());
                self.schema_properties().set(key, json);
                None
            }
            None => None,
        };
        if let Some(target) = target {
            target.properties.set(key, property.json());
        }
    }

    fn remove_from_schema(&self, key: &JsString, property: &PropertySchema) {
        let schemas = settings_schemas();
        schemas.all_settings.properties.delete(key);
        let target = match property.scope() {
            Some(ConfigurationScope::Application) => Some(&schemas.application_settings),
            Some(ConfigurationScope::Machine) => Some(&schemas.machine_settings),
            Some(ConfigurationScope::ApplicationMachine) => Some(&schemas.application_machine_settings),
            Some(ConfigurationScope::MachineOverridable) => Some(&schemas.machine_overridable_settings),
            Some(ConfigurationScope::Window) => Some(&schemas.window_settings),
            Some(ConfigurationScope::Resource | ConfigurationScope::LanguageOverridable) => {
                self.schema_properties().delete(key);
                Some(&schemas.resource_settings)
            }
            None => None,
        };
        if let Some(target) = target {
            target.properties.delete(key);
        }
    }

    fn resource_language_properties_schema() -> PropertySchema {
        PropertySchema::from_entries([
            ("type", Dynamic::string("object")),
            ("description", Dynamic::from(localize("overrideSettings.defaultDescription", "Configure editor settings to be overridden for a language.", &[]))),
            ("errorMessage", Dynamic::from(localize("overrideSettings.errorMessage", "This setting does not support per-language configuration.", &[]))),
            ("$ref", Dynamic::string(RESOURCE_LANGUAGE_SETTINGS_SCHEMA_ID)),
        ])
    }

    fn update_override_property_pattern_key(&self) {
        let identifiers: Vec<JsString> = self.override_identifiers.borrow().keys().cloned().collect();
        let schemas = settings_schemas();
        for override_identifier in identifiers {
            let override_identifier_property = JsString::from(format!("[{override_identifier}]"));
            let resource_language_properties_schema = Self::resource_language_properties_schema();
            self.update_property_default_value(&override_identifier_property, &resource_language_properties_schema);
            for schema in schemas.all() {
                schema.properties.set(&override_identifier_property, resource_language_properties_schema.json());
            }
        }
    }

    fn register_override_property_pattern_key(&self) {
        let schemas = settings_schemas();
        let pattern = JsString::from(OVERRIDE_PROPERTY_PATTERN);
        let resource_language_properties_schema = Self::resource_language_properties_schema();
        for schema in schemas.all() {
            schema.pattern_properties.set(&pattern, resource_language_properties_schema.json());
        }
        self.on_did_schema_change.fire(());
    }

    fn update_property_default_value(&self, key: &JsString, property: &PropertySchema) {
        let configuration_default_override = self.configuration_defaults_overrides.borrow().get(key).and_then(|e| e.borrow().configuration_default_override_value.clone());
        let mut default_value = Dynamic::Undefined;
        let mut default_source: Option<ConfigurationDefaultValueSource> = None;
        if let Some(configuration_default_override) = configuration_default_override {
            // Prevent overriding the default value if the property is disallowed to be overridden by
            // configuration defaults from extensions
            let source_is_truthy = match &configuration_default_override.source {
                None => false,
                Some(ConfigurationDefaultValueSource::Source(ConfigurationDefaultSource::Text(text))) => !text.is_empty(),
                Some(_) => true,
            };
            if !property.get("disallowConfigurationDefault").is_truthy() || !source_is_truthy {
                default_value = configuration_default_override.value;
                default_source = configuration_default_override.source;
            }
        }
        if default_value.is_undefined() {
            default_value = property.default_default_value();
            default_source = None;
        }
        if default_value.is_undefined() {
            default_value = get_default_value(&property.get("type"));
        }
        property.set("default", default_value);
        property.set_default_value_source(default_source);
    }

    /// `validateProperty`, contra este registro: devolve a mensagem de erro, ou `None` se a propriedade
    /// pode ser registrada.
    pub fn validate_property(&self, property: &JsString, schema: &PropertySchema, extension_id: Option<&str>) -> Option<String> {
        if trim(property).is_empty() {
            return Some(localize("config.property.empty", "Cannot register an empty property", &[]));
        }
        let property_text = property.to_string_lossy();
        if is_override_property(property) {
            return Some(localize(
                "config.property.languageDefault",
                "Cannot register '{0}'. This matches property pattern '\\\\[.*\\\\]$' for describing language specific editor settings. Use 'configurationDefaults' contribution.",
                &[&property_text],
            ));
        }
        if self.configuration_properties.contains(property) && extension_id.is_none_or(|id| !EXTENSION_UNIFICATION_EXTENSION_IDS.contains(&id.to_lowercase().as_str())) {
            return Some(localize("config.property.duplicate", "Cannot register '{0}'. This property is already registered.", &[&property_text]));
        }
        if schema.get("policy").is_truthy() && schema.get("policyReference").is_truthy() {
            return Some(localize(
                "config.policy.bothPolicyAndReference",
                "Cannot register '{0}'. A setting must not declare both 'policy' and 'policyReference'.",
                &[&property_text],
            ));
        }
        if let Some(name) = schema.policy_name()
            && let Some(owner) = self.policy_configurations.borrow().get(&name)
        {
            return Some(localize(
                "config.policy.duplicate",
                "Cannot register '{0}'. The associated policy {1} is already registered with {2}. To attach another setting to the same policy, use 'policyReference'.",
                &[&property_text, &name, &owner.to_string_lossy()],
            ));
        }
        let agent_host = schema.get("agentHost");
        if agent_host.is_truthy() {
            let key = agent_host.get_str("key");
            for (owner, sync) in self.agent_host_sync_configurations.borrow().iter() {
                if sync.get_str("key").strict_equals(&key) && owner != property {
                    return Some(localize(
                        "config.agentHost.duplicate",
                        "Cannot register '{0}'. The agent host configuration key '{1}' is already mirrored from '{2}'.",
                        &[&property_text, &dynamic_to_text(&key), &owner.to_string_lossy()],
                    ));
                }
            }
        }
        None
    }
}

/// `OVERRIDE_PROPERTY_REGEX.test(key)`: a chave é só uma sequência de `[identificador]`.
pub fn is_override_property(key: &JsString) -> bool {
    let units = key.units();
    let (open, close) = (u16::from(b'['), u16::from(b']'));
    let mut i = 0;
    let mut groups = 0;
    while i < units.len() {
        if units[i] != open {
            return false;
        }
        i += 1;
        let start = i;
        while i < units.len() && units[i] != close {
            i += 1;
        }
        if i == start || i >= units.len() {
            return false;
        }
        i += 1;
        groups += 1;
    }
    groups > 0
}

/// `overrideIdentifiersFromKey`: os identificadores de `[a][b]`, sem repetição e sem vazios.
pub fn override_identifiers_from_key(key: &JsString) -> Vec<JsString> {
    if !is_override_property(key) {
        return Vec::new();
    }
    let units = key.units();
    let (open, close) = (u16::from(b'['), u16::from(b']'));
    let mut identifiers = Vec::new();
    let mut i = 0;
    while i < units.len() {
        debug_assert_eq!(units[i], open);
        let start = i + 1;
        let end = units[start..].iter().position(|&u| u == close).map_or(units.len(), |p| start + p);
        let identifier = trim(&JsString::from(&units[start..end]));
        if !identifier.is_empty() {
            identifiers.push(identifier);
        }
        i = end + 1;
    }
    arrays::distinct(&identifiers, Clone::clone)
}

/// `keyFromOverrideIdentifiers`: `['a', 'b']` vira `[a][b]`.
pub fn key_from_override_identifiers(override_identifiers: &[JsString]) -> JsString {
    JsString::from(override_identifiers.iter().map(|i| format!("[{i}]")).collect::<String>())
}

/// `getDefaultValue(type)`: o valor padrão de um tipo de esquema (`string` ou lista de tipos).
pub fn get_default_value(type_: &Dynamic) -> Dynamic {
    let first = match type_ {
        Dynamic::Array(types) => types.get(0),
        other => other.clone(),
    };
    match first {
        Dynamic::String(t) if t == "boolean" => Dynamic::Bool(false),
        Dynamic::String(t) if t == "integer" || t == "number" => Dynamic::Number(0.0),
        Dynamic::String(t) if t == "string" => Dynamic::string(""),
        Dynamic::String(t) if t == "array" => Dynamic::array([]),
        Dynamic::String(t) if t == "object" => Dynamic::Object(ObjectRef::new()),
        _ => Dynamic::Null,
    }
}

/// `validateProperty` contra o registro global.
pub fn validate_property(property: &JsString, schema: &PropertySchema, extension_id: Option<&str>) -> Option<String> {
    configuration_registry().validate_property(property, schema, extension_id)
}

/// `getScopes`: o escopo de cada propriedade registrada, mais `launch` e `task`.
pub fn get_scopes() -> Vec<(JsString, Option<ConfigurationScope>)> {
    let mut scopes: Vec<(JsString, Option<ConfigurationScope>)> =
        configuration_registry().get_configuration_properties().entries().into_iter().map(|(key, property)| (key, property.scope())).collect();
    scopes.push((JsString::from("launch"), Some(ConfigurationScope::Resource)));
    scopes.push((JsString::from("task"), Some(ConfigurationScope::Resource)));
    scopes
}

/// `getAllConfigurationProperties`: as propriedades de todos os nós e de seus `allOf`.
pub fn get_all_configuration_properties(configuration_nodes: &[ConfigurationNode]) -> PropertyDictionary {
    let result = PropertyDictionary::new();
    for configuration in configuration_nodes {
        for key in configuration.property_keys() {
            if let Some(property) = configuration.property(&key) {
                result.set(&key, property);
            }
        }
        if !configuration.all_of.is_empty() {
            for (key, property) in get_all_configuration_properties(&configuration.all_of).entries() {
                result.set(&key, property);
            }
        }
    }
    result
}

/// `parseScope`.
pub fn parse_scope(scope: &str) -> ConfigurationScope {
    match scope {
        "application" => ConfigurationScope::Application,
        "machine" => ConfigurationScope::Machine,
        "resource" => ConfigurationScope::Resource,
        "machine-overridable" => ConfigurationScope::MachineOverridable,
        "language-overridable" => ConfigurationScope::LanguageOverridable,
        _ => ConfigurationScope::Window,
    }
}

