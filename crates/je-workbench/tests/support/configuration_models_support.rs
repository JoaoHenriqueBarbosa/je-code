//! Auxiliares do porte de `configurationModels.test.ts` do workbench: literais no estilo do TypeScript, o registro de
//! nós de configuração e o `toConfigurationModel` do fim do arquivo original.

use je_base::JsString;
use je_base::dynamic::Dynamic;
use je_base::json;
use je_base::path::Platform;
use je_base::uri::URI;
use je_platform::configuration::models::{ConfigurationModel, ConfigurationModelParser};
use je_platform::configuration::registry::{ConfigurationNode, ConfigurationNodeData, ConfigurationScope, PropertySchema};
use je_platform::configuration::{ConfigurationChange, Overrides};
use je_platform::js::from_json;
use je_platform::log::{LogServiceRef, NullLogService};

/// `new NullLogService()`.
pub fn null_log() -> LogServiceRef {
    NullLogService::shared()
}

/// Um literal no estilo do TypeScript: JSON com aspas simples.
pub fn j(text: &str) -> Dynamic {
    let mut errors = Vec::new();
    let value = json::parse_str(&text.replace('\'', "\""), &mut errors).unwrap_or_else(|| panic!("literal inválido: {text}"));
    assert!(errors.is_empty(), "literal inválido: {text}");
    from_json(&value)
}

/// `JSON.stringify(obj)` de um literal.
pub fn stringify_literal(text: &str) -> String {
    text.replace('\'', "\"")
}

/// Uma propriedade `{ 'type': 'string', 'default': 'isSet' }`, com o `scope` quando houver.
pub fn string_property(scope: Option<ConfigurationScope>) -> PropertySchema {
    let property = match j("{ 'type': 'string', 'default': 'isSet' }") {
        Dynamic::Object(object) => PropertySchema::new(object),
        other => panic!("a propriedade deve ser um objeto: {other:?}"),
    };
    if let Some(scope) = scope {
        property.set("scope", scope.to_dynamic());
    }
    property
}

/// Um nó `{ id, type: 'object', properties }`.
pub fn node(id: &str, properties: Vec<(&str, PropertySchema)>) -> ConfigurationNode {
    ConfigurationNode::new(ConfigurationNodeData { id: Some(id.to_string()), type_: Some(Dynamic::string("object")), ..Default::default() }.with_properties(properties))
}

/// `{ identifiers, contents, keys }`.
pub fn overrides_of(identifiers: &[&str], contents: Dynamic, keys: &[&str]) -> Overrides {
    Overrides::new(identifiers, contents, keys)
}

/// A lista de overrides do modelo, como o valor que o `deepStrictEqual` compara.
pub fn overrides_dynamic(overrides: &[Overrides]) -> Dynamic {
    Dynamic::array(overrides.iter().map(Overrides::to_dynamic))
}

/// `toConfigurationModel(obj)`: `parser.parse(JSON.stringify(obj))` e o modelo resultante.
pub fn to_configuration_model(literal: &str) -> ConfigurationModel {
    let mut parser = ConfigurationModelParser::new("test", null_log());
    parser.parse(Some(&stringify_literal(literal)), None);
    parser.configuration_model()
}

/// `{ keys, overrides }` com os overrides no formato `[['a', ['1', '2']]]`.
pub fn change_of(keys: &[&str], overrides: &[(&str, &[&str])]) -> ConfigurationChange {
    let strings = |items: &[&str]| items.iter().map(|item| JsString::from(*item)).collect::<Vec<JsString>>();
    ConfigurationChange { keys: strings(keys), overrides: overrides.iter().map(|(identifier, keys)| (JsString::from(*identifier), strings(keys))).collect() }
}

/// `URI.file(path)`.
pub fn uri_file(path: &str) -> URI {
    URI::file(path, Platform::Posix)
}
