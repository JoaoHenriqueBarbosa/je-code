//! Porte de `src/vs/workbench/contrib/externalUriOpener/common/configuration.ts`: o identificador da configuração
//! `workbench.externalUriOpeners`, o nó de configuração dela e `updateContributedOpeners`, que atualiza o esquema com os
//! ids dos openers que as extensões registraram.
//!
//! Divergências:
//! - `export { defaultExternalUriOpenerId }` é a constante [`DEFAULT_EXTERNAL_URI_OPENER_ID`] de
//!   `je_platform::opener`, reexportada aqui.
//! - `ExternalUriOpenersConfiguration` (`{ readonly [uriGlob: string]: string }`) é o objeto dinâmico que o serviço de
//!   configuração devolve; quem lê (o `ExternalUriOpenerService`) percorre os pares do objeto.
//! - `externalUriOpenerIdSchemaAddition` e `externalUriOpenersConfigurationNode` são constantes de módulo com identidade
//!   (o `updateContributedOpeners` muda o `enum` do esquema e o nó é o mesmo objeto que o registro guarda). Aqui os dois
//!   vivem num `thread_local` ([`Schema`]), e o esquema compartilhado entra no `anyOf` pelo mesmo [`ObjectRef`].
//! - `Registry.as<IConfigurationRegistry>(Extensions.Configuration)` é o [`configuration_registry`] da thread.
//! - O registro do nó (`registerConfiguration`) é do `externalUriOpener.contribution.ts`, que não foi portado.

use je_base::JsString;
use je_base::dynamic::{Dynamic, ObjectRef};
use je_base::nls::localize;
use je_platform::configuration::registry::{ConfigurationNode, PropertySchema, configuration_registry};

pub use je_platform::opener::DEFAULT_EXTERNAL_URI_OPENER_ID;

use crate::services::editor::editor_resolver_service::workbench_configuration_node_base;

/// `externalUriOpenersSettingId`.
pub const EXTERNAL_URI_OPENERS_SETTING_ID: &str = "workbench.externalUriOpeners";

/// `exampleUriPatterns`.
const EXAMPLE_URI_PATTERNS: &str = "\n\
- `https://microsoft.com`: Matches this specific domain using https\n\
- `https://microsoft.com:8080`: Matches this specific domain on this port using https\n\
- `https://microsoft.com:*`: Matches this specific domain on any port using https\n\
- `https://microsoft.com/foo`: Matches `https://microsoft.com/foo` and `https://microsoft.com/foo/bar`, but not `https://microsoft.com/foobar` or `https://microsoft.com/bar`\n\
- `https://*.microsoft.com`: Match all domains ending in `microsoft.com` using https\n\
- `microsoft.com`: Match this specific domain using either http or https\n\
- `*.microsoft.com`: Match all domains ending in `microsoft.com` using either http or https\n\
- `http://192.168.0.1`: Matches this specific IP using http\n\
- `http://192.168.0.*`: Matches all IP's with this prefix using http\n\
- `*`: Match all domains using either http or https";

/// O esquema compartilhado e o nó que o contém.
struct Schema {
    /// `externalUriOpenerIdSchemaAddition`: `{ type: 'string', enum: [] }`.
    id_schema_addition: ObjectRef,
    /// `externalUriOpenersConfigurationNode`.
    node: ConfigurationNode,
}

impl Schema {
    fn new() -> Self {
        let id_schema_addition = ObjectRef::from_entries([("type", Dynamic::string("string")), ("enum", Dynamic::array([]))]);

        let uri_description = || {
            Dynamic::string(&localize("externalUriOpeners.uri", "Map URI pattern to an opener id.\nExample patterns: \n{0}", &[EXAMPLE_URI_PATTERNS]))
        };

        let any_of = Dynamic::array([
            Dynamic::object([("type", Dynamic::string("string")), ("markdownDescription", uri_description())]),
            Dynamic::object([
                ("type", Dynamic::string("string")),
                ("markdownDescription", uri_description()),
                ("enum", Dynamic::array([Dynamic::string(DEFAULT_EXTERNAL_URI_OPENER_ID)])),
                (
                    "enumDescriptions",
                    Dynamic::array([Dynamic::string(&localize("externalUriOpeners.defaultId", "Open using VS Code's standard opener.", &[]))]),
                ),
            ]),
            Dynamic::Object(id_schema_addition.clone()),
        ]);

        let property = PropertySchema::from_entries([
            ("type", Dynamic::string("object")),
            (
                "markdownDescription",
                Dynamic::string(&localize("externalUriOpeners", "Configure the opener to use for external URIs (http, https).", &[])),
            ),
            ("defaultSnippets", Dynamic::array([Dynamic::object([("body", Dynamic::object([("example.com", Dynamic::string("$1"))]))])])),
            ("additionalProperties", Dynamic::object([("anyOf", any_of)])),
        ]);

        let node = ConfigurationNode::new(workbench_configuration_node_base().with_properties([(EXTERNAL_URI_OPENERS_SETTING_ID, property)]));
        Self { id_schema_addition, node }
    }
}

thread_local! {
    static SCHEMA: Schema = Schema::new();
}

/// `externalUriOpenersConfigurationNode`.
pub fn external_uri_openers_configuration_node() -> ConfigurationNode {
    SCHEMA.with(|schema| schema.node.clone())
}

/// `updateContributedOpeners(enumValues, enumDescriptions)`.
pub fn update_contributed_openers(enum_values: &[String], enum_descriptions: &[String]) {
    SCHEMA.with(|schema| {
        let to_array = |values: &[String]| Dynamic::array(values.iter().map(|value| Dynamic::string(value)));
        schema.id_schema_addition.set(&JsString::from("enum"), to_array(enum_values));
        schema.id_schema_addition.set(&JsString::from("enumDescriptions"), to_array(enum_descriptions));

        configuration_registry().notify_configuration_schema_updated(std::slice::from_ref(&schema.node));
    });
}
