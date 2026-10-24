//! Porte de `src/vs/workbench/services/configurationResolver/common/configurationResolverSchema.ts`: o esquema JSON da seção
//! `inputs` (`promptString`, `pickString` e `command`).
//!
//! `inputsSchema` é uma constante do módulo no original; aqui é a função [`inputs_schema`], que monta o objeto dinâmico
//! (`IJSONSchema`, ver `je_base::json_schema`) a cada chamada, porque o `Dynamic` não é compartilhável entre threads.

use je_base::dynamic::Dynamic;
use je_base::nls::localize;

fn text(value: &str) -> Dynamic {
    Dynamic::string(value)
}

fn strings(values: &[&str]) -> Dynamic {
    Dynamic::array(values.iter().map(|value| text(value)))
}

/// `{ type, description }`.
fn typed(type_name: &str, description: &str) -> Dynamic {
    Dynamic::object([("type", text(type_name)), ("description", text(description))])
}

/// A propriedade `type` de uma entrada: `{ type: 'string', description, enum: [...], enumDescriptions: [...] }`.
fn input_type(description: &str, type_name: &str, enum_description: &str) -> Dynamic {
    Dynamic::object([
        ("type", text("string")),
        ("description", text(description)),
        ("enum", strings(&[type_name])),
        ("enumDescriptions", strings(&[enum_description])),
    ])
}

/// `inputsSchema`.
pub fn inputs_schema() -> Dynamic {
    let id_description = localize("JsonSchema.input.id", "The input's id is used to associate an input with a variable of the form ${input:id}.", &[]);
    let type_description = localize("JsonSchema.input.type", "The type of user input prompt to use.", &[]);
    let description_description = localize("JsonSchema.input.description", "The description is shown when the user is prompted for input.", &[]);
    let default_description = localize("JsonSchema.input.default", "The default value for the input.", &[]);
    let command_args_description = localize("JsonSchema.input.command.args", "Optional arguments passed to the command.", &[]);

    let prompt_string = Dynamic::object([
        ("type", text("object")),
        ("required", strings(&["id", "type", "description"])),
        ("additionalProperties", Dynamic::Bool(false)),
        (
            "properties",
            Dynamic::object([
                ("id", typed("string", &id_description)),
                (
                    "type",
                    input_type(
                        &type_description,
                        "promptString",
                        &localize("JsonSchema.input.type.promptString", "The 'promptString' type opens an input box to ask the user for input.", &[]),
                    ),
                ),
                ("description", typed("string", &description_description)),
                ("default", typed("string", &default_description)),
                (
                    "password",
                    typed("boolean", &localize("JsonSchema.input.password", "Controls if a password input is shown. Password input hides the typed text.", &[])),
                ),
            ]),
        ),
    ]);

    let pick_string = Dynamic::object([
        ("type", text("object")),
        ("required", strings(&["id", "type", "description", "options"])),
        ("additionalProperties", Dynamic::Bool(false)),
        (
            "properties",
            Dynamic::object([
                ("id", typed("string", &id_description)),
                (
                    "type",
                    input_type(
                        &type_description,
                        "pickString",
                        &localize("JsonSchema.input.type.pickString", "The 'pickString' type shows a selection list.", &[]),
                    ),
                ),
                ("description", typed("string", &description_description)),
                ("default", typed("string", &default_description)),
                (
                    "options",
                    Dynamic::object([
                        ("type", text("array")),
                        ("description", text(&localize("JsonSchema.input.options", "An array of strings that defines the options for a quick pick.", &[]))),
                        (
                            "items",
                            Dynamic::object([(
                                "oneOf",
                                Dynamic::array([
                                    Dynamic::object([("type", text("string"))]),
                                    Dynamic::object([
                                        ("type", text("object")),
                                        ("required", strings(&["value"])),
                                        ("additionalProperties", Dynamic::Bool(false)),
                                        (
                                            "properties",
                                            Dynamic::object([
                                                ("label", typed("string", &localize("JsonSchema.input.pickString.optionLabel", "Label for the option.", &[]))),
                                                ("value", typed("string", &localize("JsonSchema.input.pickString.optionValue", "Value for the option.", &[]))),
                                            ]),
                                        ),
                                    ]),
                                ]),
                            )]),
                        ),
                    ]),
                ),
            ]),
        ),
    ]);

    let command = Dynamic::object([
        ("type", text("object")),
        ("required", strings(&["id", "type", "command"])),
        ("additionalProperties", Dynamic::Bool(false)),
        (
            "properties",
            Dynamic::object([
                ("id", typed("string", &id_description)),
                (
                    "type",
                    input_type(&type_description, "command", &localize("JsonSchema.input.type.command", "The 'command' type executes a command.", &[])),
                ),
                ("command", typed("string", &localize("JsonSchema.input.command.command", "The command to execute for this input variable.", &[]))),
                (
                    "args",
                    Dynamic::object([(
                        "oneOf",
                        Dynamic::array([typed("object", &command_args_description), typed("array", &command_args_description), typed("string", &command_args_description)]),
                    )]),
                ),
            ]),
        ),
    ]);

    Dynamic::object([(
        "definitions",
        Dynamic::object([(
            "inputs",
            Dynamic::object([
                ("type", text("array")),
                (
                    "description",
                    text(&localize(
                        "JsonSchema.inputs",
                        "User inputs. Used for defining user input prompts, such as free string input or a choice from several options.",
                        &[],
                    )),
                ),
                ("items", Dynamic::object([("oneOf", Dynamic::array([prompt_string, pick_string, command]))])),
            ]),
        )]),
    )])
}
