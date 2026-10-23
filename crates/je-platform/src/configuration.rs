//! Porte de `src/vs/platform/configuration/common/configuration.ts` (a árvore de valores, os alvos de
//! configuração e os tipos de dados) e módulo-pai de `configurationRegistry.ts`, `configurationModels.ts`,
//! `configurationVariables.ts` e `configurations.ts`.
//!
//! Representação:
//! - `IStringDictionary<unknown>` é [`Dynamic`] (um objeto com a ordem de propriedades do JavaScript).
//!   Um valor ausente (`undefined`) é `Dynamic::Undefined`, não `Option`.
//! - Chaves e seções de configuração são [`JsString`]; os métodos aceitam qualquer [`Key`] (`&str`,
//!   `String`, `JsString`).
//! - `IConfigurationOverrides` e `IConfigurationUpdateOverrides` são structs; `null` e `undefined`
//!   viram `None`. Os guardas de tipo `isConfigurationOverrides` e `isConfigurationUpdateOverrides`
//!   (que validam `unknown` vindo de fora do processo) não existem: o tipo já garante a forma.
//! - `IConfigurationService` e `IConfigurationChangeEvent` (interfaces de serviço, com injeção de
//!   dependência) não são portados aqui; a classe concreta do evento está em [`models`].
//! - `UriComponents` serializado (em `IConfigurationData.folders`) é o próprio [`URI`], porque o dado
//!   não atravessa fronteira de processo.

use je_base::JsString;
use je_base::dynamic::{Dynamic, Truthy};
use je_base::js_string;
use je_base::objects;
use je_base::types;
use je_base::uri::URI;

use crate::js::{Key, delete_property, for_in_entries, is_non_null_object, object_keys, set_property, split};

pub mod defaults;
pub mod models;
pub mod policy_configuration;
pub mod service;
pub mod user_settings;
pub mod registry;
pub mod variables;

const DOT: u16 = b'.' as u16;

/// `IConfigurationOverrides`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ConfigurationOverrides {
    pub override_identifier: Option<JsString>,
    pub resource: Option<URI>,
}

impl ConfigurationOverrides {
    pub fn with_identifier(identifier: impl Key) -> Self {
        Self { override_identifier: Some(identifier.to_js()), resource: None }
    }

    pub fn with_resource(resource: URI) -> Self {
        Self { override_identifier: None, resource: Some(resource) }
    }
}

/// `IConfigurationUpdateOverrides` (`Omit<IConfigurationOverrides, 'overrideIdentifier'> & { overrideIdentifiers }`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ConfigurationUpdateOverrides {
    pub override_identifiers: Option<Vec<JsString>>,
    pub resource: Option<URI>,
}

/// `ConfigurationTarget` (os valores numéricos são os do original).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ConfigurationTarget {
    Application = 1,
    User = 2,
    UserLocal = 3,
    UserRemote = 4,
    Workspace = 5,
    WorkspaceFolder = 6,
    Default = 7,
    Memory = 8,
}

/// `ConfigurationTargetToString`.
pub fn configuration_target_to_string(target: ConfigurationTarget) -> &'static str {
    match target {
        ConfigurationTarget::Application => "APPLICATION",
        ConfigurationTarget::User => "USER",
        ConfigurationTarget::UserLocal => "USER_LOCAL",
        ConfigurationTarget::UserRemote => "USER_REMOTE",
        ConfigurationTarget::Workspace => "WORKSPACE",
        ConfigurationTarget::WorkspaceFolder => "WORKSPACE_FOLDER",
        ConfigurationTarget::Default => "DEFAULT",
        ConfigurationTarget::Memory => "MEMORY",
    }
}

/// `IConfigurationChange`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ConfigurationChange {
    pub keys: Vec<JsString>,
    pub overrides: Vec<(JsString, Vec<JsString>)>,
}

/// `IOverrides`: o conteúdo de um bloco `[linguagem]` do arquivo de configuração. `contents` é um
/// [`Dynamic`] porque o original não garante que seja um objeto (os testes passam uma string).
#[derive(Clone, Debug)]
pub struct Overrides {
    pub keys: Vec<JsString>,
    pub contents: Dynamic,
    pub identifiers: Vec<JsString>,
}

impl Overrides {
    pub fn new<K: Key, I: Key>(identifiers: &[I], contents: Dynamic, keys: &[K]) -> Self {
        Self { keys: keys.iter().map(Key::to_js).collect(), contents, identifiers: identifiers.iter().map(Key::to_js).collect() }
    }

    /// A cópia profunda (`objects.deepClone` do original sobre o objeto).
    pub fn deep_clone(&self) -> Self {
        Self { keys: self.keys.clone(), contents: objects::deep_clone(&self.contents), identifiers: self.identifiers.clone() }
    }

    /// O objeto `{ identifiers, keys, contents }` do original (nessa ordem de propriedades).
    pub fn to_dynamic(&self) -> Dynamic {
        Dynamic::object([
            ("identifiers", strings_to_dynamic(&self.identifiers)),
            ("keys", strings_to_dynamic(&self.keys)),
            ("contents", self.contents.clone()),
        ])
    }
}

/// Um array de strings como valor dinâmico.
pub fn strings_to_dynamic(strings: &[JsString]) -> Dynamic {
    Dynamic::array(strings.iter().cloned().map(Dynamic::String))
}

/// `IConfigurationModel`: o dado de um modelo (sem a classe).
#[derive(Clone)]
pub struct ConfigurationModelData {
    pub contents: Dynamic,
    pub keys: Vec<JsString>,
    pub overrides: Vec<Overrides>,
    pub raw: Option<models::Raw>,
}

/// `IConfigurationData`.
#[derive(Clone)]
pub struct ConfigurationData {
    pub defaults: ConfigurationModelData,
    pub policy: ConfigurationModelData,
    pub application: ConfigurationModelData,
    pub user_local: ConfigurationModelData,
    pub user_remote: ConfigurationModelData,
    pub workspace: ConfigurationModelData,
    pub folders: Vec<(URI, ConfigurationModelData)>,
}

/// `IConfigurationCompareResult`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ConfigurationCompareResult {
    pub added: Vec<JsString>,
    pub removed: Vec<JsString>,
    pub updated: Vec<JsString>,
    pub overrides: Vec<(JsString, Vec<JsString>)>,
}

/// `IConfigurationValue<T>`: o valor de uma chave em cada camada. Um valor ausente é `undefined`.
pub trait ConfigurationValue {
    fn default_value(&self) -> Dynamic;
    fn application_value(&self) -> Dynamic;
    fn user_value(&self) -> Dynamic;
    fn user_local_value(&self) -> Dynamic;
    fn user_remote_value(&self) -> Dynamic;
    fn workspace_value(&self) -> Dynamic;
    fn workspace_folder_value(&self) -> Dynamic;
    fn memory_value(&self) -> Dynamic;
    fn policy_value(&self) -> Dynamic;
    fn value(&self) -> Dynamic;
    fn override_identifiers(&self) -> Option<Vec<JsString>>;
}

/// `getConfigValueInTarget`.
pub fn get_config_value_in_target(config_value: &dyn ConfigurationValue, scope: ConfigurationTarget) -> Dynamic {
    match scope {
        ConfigurationTarget::Application => config_value.application_value(),
        ConfigurationTarget::User => config_value.user_value(),
        ConfigurationTarget::UserLocal => config_value.user_local_value(),
        ConfigurationTarget::UserRemote => config_value.user_remote_value(),
        ConfigurationTarget::Workspace => config_value.workspace_value(),
        ConfigurationTarget::WorkspaceFolder => config_value.workspace_folder_value(),
        ConfigurationTarget::Default => config_value.default_value(),
        ConfigurationTarget::Memory => config_value.memory_value(),
    }
}

/// `isConfigured`.
pub fn is_configured(config_value: &dyn ConfigurationValue) -> bool {
    !config_value.application_value().is_undefined()
        || !config_value.user_value().is_undefined()
        || !config_value.user_local_value().is_undefined()
        || !config_value.user_remote_value().is_undefined()
        || !config_value.workspace_value().is_undefined()
        || !config_value.workspace_folder_value().is_undefined()
}

/// `IConfigurationUpdateOptions`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ConfigurationUpdateOptions {
    /// Se `true`, não avisa o erro ao usuário com uma caixa de mensagem.
    pub donot_notify_error: bool,
    pub handle_dirty_file: Option<HandleDirtyFile>,
}

/// `handleDirtyFile: 'save' | 'revert'`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HandleDirtyFile {
    Save,
    Revert,
}

/// `toValuesTree`: transforma chaves com ponto (`a.b.c`) numa árvore de objetos. Um conflito (`a` é um
/// número e chega `a.b`) não lança: vai para `conflict_reporter` e a chave é ignorada.
pub fn to_values_tree(properties: &Dynamic, conflict_reporter: &dyn Fn(&str)) -> Dynamic {
    let root = Dynamic::object(Vec::<(&str, Dynamic)>::new());
    for (key, value) in for_in_entries(properties) {
        add_to_value_tree(&root, &key, value, conflict_reporter);
    }
    root
}

/// `JSON.stringify(value)` como texto de mensagem (`undefined` quando o JSON não o representa).
fn json_text(value: &Dynamic) -> String {
    objects::safe_stringify(value).unwrap_or_else(|| "undefined".to_string())
}

/// `addToValueTree`.
pub fn add_to_value_tree(settings_tree_root: &Dynamic, key: &JsString, value: Dynamic, conflict_reporter: &dyn Fn(&str)) {
    let mut segments = split(key, DOT);
    let last = segments.pop().expect("split devolve ao menos um pedaço");
    let joined = |segments: &[JsString]| segments.iter().map(JsString::to_string_lossy).collect::<Vec<_>>().join(".");

    let mut curr = settings_tree_root.clone();
    for i in 0..segments.len() {
        let s = &segments[i];
        let mut obj = curr.get(s);
        match obj.type_of() {
            "undefined" => {
                obj = Dynamic::object(Vec::<(&str, Dynamic)>::new());
                set_property(&curr, s, obj.clone());
            }
            "object" => {
                if obj.is_null() {
                    conflict_reporter(&format!("Ignoring {key} as {} is null", joined(&segments[..=i])));
                    return;
                }
            }
            _ => {
                conflict_reporter(&format!("Ignoring {key} as {} is {}", joined(&segments[..=i]), json_text(&obj)));
                return;
            }
        }
        curr = obj;
    }

    if is_non_null_object(&curr) {
        // workaround https://github.com/microsoft/vscode/issues/13606: no original a escrita pode lançar
        // (objeto congelado), e isso vira um conflito.
        let target = curr.clone();
        if je_base::errors::catch(|| set_property(&target, &last, value)).is_err() {
            conflict_reporter(&format!("Ignoring {key} as {} is {}", joined(&segments), json_text(&curr)));
        }
    } else {
        conflict_reporter(&format!("Ignoring {key} as {} is {}", joined(&segments), json_text(&curr)));
    }
}

/// `removeFromValueTree`.
pub fn remove_from_value_tree(value_tree: &Dynamic, key: impl Key) {
    let mut segments = split(&key.to_js(), DOT);
    do_remove_from_value_tree(value_tree, &mut segments);
}

fn do_remove_from_value_tree(value_tree: &Dynamic, segments: &mut Vec<JsString>) {
    if !value_tree.is_truthy() {
        return;
    }
    let first = segments.remove(0);
    if segments.is_empty() {
        // Reached last segment
        delete_property(value_tree, &first);
        return;
    }
    if object_keys(value_tree).contains(&first) {
        let value = value_tree.get(&first);
        if is_non_null_object(&value) && !value.is_array() {
            do_remove_from_value_tree(&value, segments);
            if object_keys(&value).is_empty() {
                delete_property(value_tree, &first);
            }
        }
    }
}

/// `getConfigurationValue`: o valor num caminho como `config.some.setting`, ou `default_value` (por
/// padrão `undefined`) se o caminho não existe.
pub fn get_configuration_value(config: &Dynamic, setting_path: impl Key, default_value: Option<Dynamic>) -> Dynamic {
    let mut current = config.clone();
    for component in split(&setting_path.to_js(), DOT) {
        if !is_non_null_object(&current) {
            current = Dynamic::Undefined;
            break;
        }
        current = current.get(&component);
    }
    if current.is_undefined() { default_value.unwrap_or(Dynamic::Undefined) } else { current }
}

/// `merge`: junta `add` em `base`. Com `overwrite`, o valor de `add` vence nas chaves em conflito que
/// não são objetos nos dois lados. A chave `__proto__` é ignorada.
pub fn merge(base: &Dynamic, add: &Dynamic, overwrite: bool) {
    for key in add.own_keys() {
        if key == "__proto__" {
            continue;
        }
        if base.has_property(&key) {
            let (base_value, add_value) = (base.get(&key), add.get(&key));
            if types::is_object(&base_value) && types::is_object(&add_value) {
                merge(&base_value, &add_value, overwrite);
            } else if overwrite {
                set_property(base, &key, add_value);
            }
        } else {
            set_property(base, &key, add.get(&key));
        }
    }
}

/// `getLanguageTagSettingPlainKey`: `[a][b]` vira `a, b`.
pub fn get_language_tag_setting_plain_key(setting_key: impl Key) -> JsString {
    let key = setting_key.to_js();
    let mut units = key.units();
    if units.first() == Some(&u16::from(b'[')) {
        units = &units[1..];
    }
    if units.last() == Some(&u16::from(b']')) {
        units = &units[..units.len() - 1];
    }
    let separator: Vec<u16> = "][".encode_utf16().collect();
    let replacement: Vec<u16> = ", ".encode_utf16().collect();
    let mut result = Vec::with_capacity(units.len());
    let mut position = 0;
    while let Some(found) = js_string::index_of(units, &separator, position) {
        result.extend_from_slice(&units[position..found]);
        result.extend_from_slice(&replacement);
        position = found + separator.len();
    }
    result.extend_from_slice(&units[position..]);
    JsString::from_units(result)
}
