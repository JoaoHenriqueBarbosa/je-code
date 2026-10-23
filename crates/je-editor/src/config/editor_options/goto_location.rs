//! Porte de `EditorGoToLocation` (`editorOptions.ts`, região `gotoLocation`).

use je_base::dynamic::{Dynamic, Truthy};

use crate::config::editor_options::{string, strings_array, EditorOption, EditorOptionImpl, Opt};

string_enum! {
    /// `GoToLocationValues`: `'peek' | 'gotoAndPeek' | 'goto'`.
    pub enum GoToLocationValues {
        Peek => "peek",
        GotoAndPeek => "gotoAndPeek",
        Goto => "goto",
    }
}

/// `alternativeCommandOptions`: ids de comando aceitos nas chaves `alternative*Command` do esquema.
const ALTERNATIVE_COMMAND_OPTIONS: [&str; 12] = [
    "",
    "editor.action.referenceSearch.trigger",
    "editor.action.goToReferences",
    "editor.action.peekImplementation",
    "editor.action.goToImplementation",
    "editor.action.peekTypeDefinition",
    "editor.action.goToTypeDefinition",
    "editor.action.peekDeclaration",
    "editor.action.revealDeclaration",
    "editor.action.peekDefinition",
    "editor.action.revealDefinitionAside",
    "editor.action.revealDefinition",
];

/// `GoToLocationOptions` (`Readonly<Required<IGotoLocationOptions>>`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GoToLocationOptions {
    pub multiple: GoToLocationValues,

    pub multiple_definitions: GoToLocationValues,
    pub multiple_type_definitions: GoToLocationValues,
    pub multiple_declarations: GoToLocationValues,
    pub multiple_implementations: GoToLocationValues,
    pub multiple_references: GoToLocationValues,
    pub multiple_tests: GoToLocationValues,

    pub alternative_definition_command: String,
    pub alternative_type_definition_command: String,
    pub alternative_declaration_command: String,
    pub alternative_implementation_command: String,
    pub alternative_reference_command: String,
    pub alternative_tests_command: String,
}

/// Chave tipada da opção `gotoLocation`.
pub const GOTO_LOCATION: Opt<GoToLocationOptions> = Opt::new(EditorOption::GotoLocation);

/// `{ description, ...jsonSubset }`: `jsonSubset` é `type`, `enum`, `default` e `enumDescriptions`.
fn multiple_schema(description: &str) -> Dynamic {
    Dynamic::object([
        ("description", Dynamic::string(description)),
        ("type", Dynamic::string("string")),
        ("enum", strings_array(GoToLocationValues::ALLOWED)),
        ("default", Dynamic::string(GoToLocationValues::Peek.as_str())),
        (
            "enumDescriptions",
            strings_array(&[
                "Show Peek view of the results (default)",
                "Go to the primary result and show a Peek view",
                "Go to the primary result and enable Peek-less navigation to others",
            ]),
        ),
    ])
}

/// `{ type: 'string', default, enum: alternativeCommandOptions, description }`.
fn alternative_command_schema(default: &str, description: &str) -> Dynamic {
    Dynamic::object([
        ("type", Dynamic::string("string")),
        ("default", Dynamic::string(default)),
        ("enum", strings_array(&ALTERNATIVE_COMMAND_OPTIONS)),
        ("description", Dynamic::string(description)),
    ])
}

/// `EditorGoToLocation`.
pub struct EditorGoToLocation {
    default_value: GoToLocationOptions,
    schema: Dynamic,
}

impl EditorGoToLocation {
    pub fn new() -> Self {
        let defaults = GoToLocationOptions {
            multiple: GoToLocationValues::Peek,
            multiple_definitions: GoToLocationValues::Peek,
            multiple_type_definitions: GoToLocationValues::Peek,
            multiple_declarations: GoToLocationValues::Peek,
            multiple_implementations: GoToLocationValues::Peek,
            multiple_references: GoToLocationValues::Peek,
            multiple_tests: GoToLocationValues::Peek,
            alternative_definition_command: "editor.action.goToReferences".to_string(),
            alternative_type_definition_command: "editor.action.goToReferences".to_string(),
            alternative_declaration_command: "editor.action.goToReferences".to_string(),
            alternative_implementation_command: String::new(),
            alternative_reference_command: String::new(),
            alternative_tests_command: String::new(),
        };
        let schema = Dynamic::object([
            (
                "editor.gotoLocation.multiple",
                Dynamic::object([(
                    "deprecationMessage",
                    Dynamic::string("This setting is deprecated, please use separate settings like 'editor.editor.gotoLocation.multipleDefinitions' or 'editor.editor.gotoLocation.multipleImplementations' instead."),
                )]),
            ),
            (
                "editor.gotoLocation.multipleDefinitions",
                multiple_schema("Controls the behavior the 'Go to Definition'-command when multiple target locations exist."),
            ),
            (
                "editor.gotoLocation.multipleTypeDefinitions",
                multiple_schema("Controls the behavior the 'Go to Type Definition'-command when multiple target locations exist."),
            ),
            (
                "editor.gotoLocation.multipleDeclarations",
                multiple_schema("Controls the behavior the 'Go to Declaration'-command when multiple target locations exist."),
            ),
            (
                "editor.gotoLocation.multipleImplementations",
                multiple_schema("Controls the behavior the 'Go to Implementations'-command when multiple target locations exist."),
            ),
            (
                "editor.gotoLocation.multipleReferences",
                multiple_schema("Controls the behavior the 'Go to References'-command when multiple target locations exist."),
            ),
            (
                "editor.gotoLocation.alternativeDefinitionCommand",
                alternative_command_schema(
                    &defaults.alternative_definition_command,
                    "Alternative command id that is being executed when the result of 'Go to Definition' is the current location.",
                ),
            ),
            (
                "editor.gotoLocation.alternativeTypeDefinitionCommand",
                alternative_command_schema(
                    &defaults.alternative_type_definition_command,
                    "Alternative command id that is being executed when the result of 'Go to Type Definition' is the current location.",
                ),
            ),
            (
                "editor.gotoLocation.alternativeDeclarationCommand",
                alternative_command_schema(
                    &defaults.alternative_declaration_command,
                    "Alternative command id that is being executed when the result of 'Go to Declaration' is the current location.",
                ),
            ),
            (
                "editor.gotoLocation.alternativeImplementationCommand",
                alternative_command_schema(
                    &defaults.alternative_implementation_command,
                    "Alternative command id that is being executed when the result of 'Go to Implementation' is the current location.",
                ),
            ),
            (
                "editor.gotoLocation.alternativeReferenceCommand",
                alternative_command_schema(
                    &defaults.alternative_reference_command,
                    "Alternative command id that is being executed when the result of 'Go to Reference' is the current location.",
                ),
            ),
        ]);
        Self { default_value: defaults, schema }
    }
}

impl Default for EditorGoToLocation {
    fn default() -> Self {
        Self::new()
    }
}

impl EditorOptionImpl for EditorGoToLocation {
    type Value = GoToLocationOptions;

    fn id(&self) -> EditorOption {
        EditorOption::GotoLocation
    }

    fn name(&self) -> &str {
        "gotoLocation"
    }

    fn default_value(&self) -> GoToLocationOptions {
        self.default_value.clone()
    }

    fn schema(&self) -> Option<&Dynamic> {
        Some(&self.schema)
    }

    fn validate(&self, input: &Dynamic) -> GoToLocationOptions {
        if !input.is_truthy() || input.type_of() != "object" {
            return self.default_value.clone();
        }
        let defaults = &self.default_value;
        // O original usa `defaultValue.multiple` na chave `multiple` e `'peek'` nas demais, que é o mesmo valor.
        let multiple = |key: &str, default_value: GoToLocationValues| GoToLocationValues::validate(&input.get_str(key), default_value);
        let command = |key: &str, default_value: &str| string(&input.get_str(key), default_value);
        GoToLocationOptions {
            multiple: multiple("multiple", defaults.multiple),
            multiple_definitions: multiple("multipleDefinitions", GoToLocationValues::Peek),
            multiple_type_definitions: multiple("multipleTypeDefinitions", GoToLocationValues::Peek),
            multiple_declarations: multiple("multipleDeclarations", GoToLocationValues::Peek),
            multiple_implementations: multiple("multipleImplementations", GoToLocationValues::Peek),
            multiple_references: multiple("multipleReferences", GoToLocationValues::Peek),
            multiple_tests: multiple("multipleTests", GoToLocationValues::Peek),
            alternative_definition_command: command("alternativeDefinitionCommand", &defaults.alternative_definition_command),
            alternative_type_definition_command: command("alternativeTypeDefinitionCommand", &defaults.alternative_type_definition_command),
            alternative_declaration_command: command("alternativeDeclarationCommand", &defaults.alternative_declaration_command),
            alternative_implementation_command: command("alternativeImplementationCommand", &defaults.alternative_implementation_command),
            alternative_reference_command: command("alternativeReferenceCommand", &defaults.alternative_reference_command),
            alternative_tests_command: command("alternativeTestsCommand", &defaults.alternative_tests_command),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(value: &str) -> Dynamic {
        Dynamic::string(value)
    }

    fn validate(entries: Vec<(&str, Dynamic)>) -> GoToLocationOptions {
        EditorOptionImpl::validate(&EditorGoToLocation::new(), &Dynamic::object(entries))
    }

    fn defaults() -> GoToLocationOptions {
        EditorOptionImpl::default_value(&EditorGoToLocation::new())
    }

    #[test]
    fn defaults_match_the_original() {
        let defaults = defaults();
        assert_eq!(defaults.multiple, GoToLocationValues::Peek);
        assert_eq!(defaults.multiple_tests, GoToLocationValues::Peek);
        assert_eq!(defaults.alternative_definition_command, "editor.action.goToReferences");
        assert_eq!(defaults.alternative_type_definition_command, "editor.action.goToReferences");
        assert_eq!(defaults.alternative_declaration_command, "editor.action.goToReferences");
        assert_eq!(defaults.alternative_implementation_command, "");
        assert_eq!(defaults.alternative_reference_command, "");
        assert_eq!(defaults.alternative_tests_command, "");
        assert_eq!(validate(vec![]), defaults);
        let option = EditorGoToLocation::new();
        for input in [Dynamic::Undefined, Dynamic::Null, Dynamic::Bool(true), Dynamic::Number(2.0), text("peek")] {
            assert_eq!(EditorOptionImpl::validate(&option, &input), defaults);
        }
    }

    #[test]
    fn multiple_values_validate_and_fall_back_to_peek() {
        let value = validate(vec![
            ("multiple", text("goto")),
            ("multipleDefinitions", text("gotoAndPeek")),
            ("multipleTypeDefinitions", text("goto")),
            ("multipleDeclarations", text("nonsense")),
            ("multipleImplementations", text("")),
            ("multipleReferences", text("Peek")),
            ("multipleTests", Dynamic::Number(1.0)),
        ]);
        assert_eq!(value.multiple, GoToLocationValues::Goto);
        assert_eq!(value.multiple_definitions, GoToLocationValues::GotoAndPeek);
        assert_eq!(value.multiple_type_definitions, GoToLocationValues::Goto);
        assert_eq!(value.multiple_declarations, GoToLocationValues::Peek);
        assert_eq!(value.multiple_implementations, GoToLocationValues::Peek);
        assert_eq!(value.multiple_references, GoToLocationValues::Peek);
        assert_eq!(value.multiple_tests, GoToLocationValues::Peek);
        assert_eq!(validate(vec![("multipleTests", text("goto"))]).multiple_tests, GoToLocationValues::Goto);
    }

    #[test]
    fn alternative_commands_accept_any_string() {
        let value = validate(vec![
            ("alternativeDefinitionCommand", text("my.command")),
            ("alternativeTypeDefinitionCommand", text("")),
            ("alternativeReferenceCommand", text("editor.action.peekDefinition")),
            ("alternativeTestsCommand", Dynamic::Bool(true)),
            ("alternativeImplementationCommand", Dynamic::Null),
        ]);
        // Não há lista de valores permitidos na validação, só no esquema; o que não é string cai no padrão.
        assert_eq!(value.alternative_definition_command, "my.command");
        assert_eq!(value.alternative_type_definition_command, "");
        assert_eq!(value.alternative_declaration_command, "editor.action.goToReferences");
        assert_eq!(value.alternative_reference_command, "editor.action.peekDefinition");
        assert_eq!(value.alternative_tests_command, "");
        assert_eq!(value.alternative_implementation_command, "");
    }

    #[test]
    fn string_enum_helpers() {
        assert_eq!(GoToLocationValues::from_text("goto"), Some(GoToLocationValues::Goto));
        assert_eq!(GoToLocationValues::from_text("Goto"), None);
        assert_eq!(GoToLocationValues::GotoAndPeek.as_str(), "gotoAndPeek");
        assert_eq!(GoToLocationValues::ALLOWED, ["peek", "gotoAndPeek", "goto"]);
        assert_eq!(GoToLocationValues::validate(&Dynamic::Number(1.0), GoToLocationValues::Goto), GoToLocationValues::Goto);
    }

    #[test]
    fn schema_has_eleven_keys_in_order() {
        let option = EditorGoToLocation::new();
        let schema = EditorOptionImpl::schema(&option).expect("sem esquema");
        let keys = schema.own_keys();
        assert_eq!(keys.len(), 11);
        assert_eq!(keys[0], "editor.gotoLocation.multiple");
        assert_eq!(keys[1], "editor.gotoLocation.multipleDefinitions");
        assert_eq!(keys[10], "editor.gotoLocation.alternativeReferenceCommand");
        assert!(!keys.iter().any(|key| key.to_string_lossy().contains("Tests")));

        let deprecated = schema.get_str("editor.gotoLocation.multiple");
        assert_eq!(deprecated.own_keys(), vec!["deprecationMessage"]);

        let definitions = schema.get_str("editor.gotoLocation.multipleDefinitions");
        assert_eq!(definitions.own_keys(), vec!["description", "type", "enum", "default", "enumDescriptions"]);
        assert_eq!(definitions.get_str("default"), text("peek"));
        assert_eq!(definitions.get_str("enum").get_str("length"), Dynamic::Number(3.0));

        let alternative = schema.get_str("editor.gotoLocation.alternativeDefinitionCommand");
        assert_eq!(alternative.own_keys(), vec!["type", "default", "enum", "description"]);
        assert_eq!(alternative.get_str("default"), text("editor.action.goToReferences"));
        assert_eq!(alternative.get_str("enum").get_str("length"), Dynamic::Number(12.0));
    }
}
