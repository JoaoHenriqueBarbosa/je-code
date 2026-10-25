//! Porte do trecho "debug general schema" de `src/vs/workbench/contrib/debug/common/debugSchemas.ts`: o
//! `presentationSchema` e o `launchSchema`, o esquema JSON do `launch.json`.
//!
//! Traz também o `INTERNAL_CONSOLE_OPTIONS_SCHEMA` de `common/debug.ts`, que só o gerenciador de adaptadores usa.
//!
//! Ficam de fora (ainda não portados): os pontos de extensão `debuggers` e `breakpoints` e o `DebuggersDataRenderer`.
//!
//! Divergências:
//! - **`launchSchema` vivo.** É uma constante do módulo no original, mutada depois pelo gerenciador de configurações
//!   (`setCompoundSchemaValues`) e pelo gerenciador de adaptadores. Aqui é o objeto [`ObjectRef`] único da thread,
//!   [`launch_schema`] (o `Dynamic` não cruza threads), e todos mutam o mesmo objeto.
//! - **Registro.** O original registra o esquema em `debugConfigurationManager.ts` ao carregar o módulo
//!   (`jsonRegistry.registerSchema(launchSchemaId, launchSchema)`). Aqui o registro em
//!   `je_base::json_contribution_registry` acontece quando o objeto nasce, na primeira chamada de [`launch_schema`].
//! - **`presentationSchema`.** É a função [`presentation_schema`], que monta um objeto novo a cada chamada; o
//!   `launchSchema` embute o seu.

use je_base::dynamic::{Dynamic, ObjectRef};
use je_base::json_contribution_registry::get_json_contribution_registry;
use je_base::nls::localize;

use crate::services::configuration::common::configuration::LAUNCH_SCHEMA_ID;
use crate::services::configuration_resolver::common::configuration_resolver_schema::inputs_schema;

fn text(value: &str) -> Dynamic {
    Dynamic::string(value)
}

fn empty_enum() -> Dynamic {
    Dynamic::array([])
}

/// `{ type: 'boolean' | 'string' | 'number', default, description }`.
fn typed_with_default(type_name: &str, default: Dynamic, description: String) -> Dynamic {
    Dynamic::object([("type", text(type_name)), ("default", default), ("description", Dynamic::string(&description))])
}

/// `presentationSchema`.
pub fn presentation_schema() -> Dynamic {
    Dynamic::object([
        ("type", text("object")),
        (
            "description",
            Dynamic::string(&localize(
                "presentation",
                "Presentation options on how to show this configuration in the debug configuration dropdown and the command palette.",
                &[],
            )),
        ),
        (
            "properties",
            Dynamic::object([
                (
                    "hidden",
                    typed_with_default(
                        "boolean",
                        Dynamic::Bool(false),
                        localize("presentation.hidden", "Controls if this configuration should be shown in the configuration dropdown and the command palette.", &[]),
                    ),
                ),
                (
                    "group",
                    typed_with_default(
                        "string",
                        text(""),
                        localize(
                            "presentation.group",
                            "Group that this configuration belongs to. Used for grouping and sorting in the configuration dropdown and the command palette.",
                            &[],
                        ),
                    ),
                ),
                (
                    "order",
                    typed_with_default(
                        "number",
                        Dynamic::Number(1.0),
                        localize(
                            "presentation.order",
                            "Order of this configuration within a group. Used for grouping and sorting in the configuration dropdown and the command palette.",
                            &[],
                        ),
                    ),
                ),
            ]),
        ),
        ("default", Dynamic::object([("hidden", Dynamic::Bool(false)), ("group", text("")), ("order", Dynamic::Number(1.0))])),
    ])
}

/// O `launchSchema` na sua forma inicial.
fn build_launch_schema() -> ObjectRef {
    // `defaultCompound`: o mesmo objeto nos dois `default`.
    let default_compound = Dynamic::object([("name", text("Compound")), ("configurations", Dynamic::array([]))]);
    let compound_name_description = localize("app.launch.json.compound.name", "Name of compound. Appears in the launch configuration drop down menu.", &[]);

    let compound_configurations = Dynamic::object([
        ("type", text("array")),
        ("default", Dynamic::array([])),
        (
            "items",
            Dynamic::object([(
                "oneOf",
                Dynamic::array([
                    Dynamic::object([
                        ("enum", empty_enum()),
                        ("description", Dynamic::string(&localize("useUniqueNames", "Please use unique configuration names.", &[]))),
                    ]),
                    Dynamic::object([
                        ("type", text("object")),
                        ("required", Dynamic::array([text("name")])),
                        (
                            "properties",
                            Dynamic::object([
                                ("name", Dynamic::object([("enum", empty_enum()), ("description", Dynamic::string(&compound_name_description))])),
                                (
                                    "folder",
                                    Dynamic::object([
                                        ("enum", empty_enum()),
                                        (
                                            "description",
                                            Dynamic::string(&localize("app.launch.json.compound.folder", "Name of folder in which the compound is located.", &[])),
                                        ),
                                    ]),
                                ),
                            ]),
                        ),
                    ]),
                ]),
            )]),
        ),
        (
            "description",
            Dynamic::string(&localize("app.launch.json.compounds.configurations", "Names of configurations that will be started as part of this compound.", &[])),
        ),
    ]);

    let compound_item = Dynamic::object([
        ("type", text("object")),
        ("required", Dynamic::array([text("name"), text("configurations")])),
        (
            "properties",
            Dynamic::object([
                ("name", Dynamic::object([("type", text("string")), ("description", Dynamic::string(&compound_name_description))])),
                ("presentation", presentation_schema()),
                ("configurations", compound_configurations),
                (
                    "stopAll",
                    typed_with_default(
                        "boolean",
                        Dynamic::Bool(false),
                        localize(
                            "app.launch.json.compound.stopAll",
                            "Controls whether manually terminating one session will stop all of the compound sessions.",
                            &[],
                        ),
                    ),
                ),
                (
                    "preLaunchTask",
                    typed_with_default("string", text(""), localize("compoundPrelaunchTask", "Task to run before any of the compound configurations start.", &[])),
                ),
            ]),
        ),
        ("default", default_compound.clone()),
    ]);

    let inputs = inputs_schema().get_str("definitions").get_str("inputs");

    ObjectRef::from_entries([
        ("id", text(LAUNCH_SCHEMA_ID)),
        ("type", text("object")),
        ("title", Dynamic::string(&localize("app.launch.json.title", "Launch", &[]))),
        ("allowTrailingCommas", Dynamic::Bool(true)),
        ("allowComments", Dynamic::Bool(true)),
        ("required", Dynamic::array([])),
        (
            "default",
            Dynamic::object([("version", text("0.2.0")), ("configurations", Dynamic::array([])), ("compounds", Dynamic::array([]))]),
        ),
        (
            "properties",
            Dynamic::object([
                (
                    "version",
                    typed_with_default("string", text("0.2.0"), localize("app.launch.json.version", "Version of this file format.", &[])),
                ),
                (
                    "configurations",
                    Dynamic::object([
                        ("type", text("array")),
                        (
                            "description",
                            Dynamic::string(&localize(
                                "app.launch.json.configurations",
                                "List of configurations. Add new configurations or edit existing ones by using IntelliSense.",
                                &[],
                            )),
                        ),
                        ("items", Dynamic::object([("defaultSnippets", Dynamic::array([])), ("type", text("object")), ("oneOf", Dynamic::array([]))])),
                    ]),
                ),
                (
                    "compounds",
                    Dynamic::object([
                        ("type", text("array")),
                        (
                            "description",
                            Dynamic::string(&localize(
                                "app.launch.json.compounds",
                                "List of compounds. Each compound references multiple configurations which will get launched together.",
                                &[],
                            )),
                        ),
                        ("items", compound_item),
                        ("default", Dynamic::array([default_compound])),
                    ]),
                ),
                ("inputs", inputs),
            ]),
        ),
    ])
}

thread_local! {
    static LAUNCH_SCHEMA: ObjectRef = {
        let schema = build_launch_schema();
        get_json_contribution_registry().register_schema(LAUNCH_SCHEMA_ID, Dynamic::Object(schema.clone()), None);
        schema
    };
}

/// `INTERNAL_CONSOLE_OPTIONS_SCHEMA` (`common/debug.ts`): o esquema da propriedade `internalConsoleOptions`, que o
/// gerenciador de adaptadores põe nas propriedades comuns do `launch.json`.
pub fn internal_console_options_schema() -> Dynamic {
    Dynamic::object([
        ("enum", Dynamic::array([text("neverOpen"), text("openOnSessionStart"), text("openOnFirstSessionStart")])),
        ("default", text("openOnFirstSessionStart")),
        ("description", Dynamic::string(&localize("internalConsoleOptions", "Controls when the internal Debug Console should open.", &[]))),
    ])
}

/// `launchSchema`: o objeto único da thread, que o gerenciador de configurações e o de adaptadores mutam.
pub fn launch_schema() -> ObjectRef {
    LAUNCH_SCHEMA.with(ObjectRef::clone)
}
