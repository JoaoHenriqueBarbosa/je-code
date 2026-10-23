//! Porte de `EditorGoToLocation` (`editorOptions.ts`, região `gotoLocation`).
//!
//! Divergências:
//! - `validate(unknown)` do original lê os campos do objeto. [`OptionInput`] ainda não carrega objetos,
//!   então [`EditorOptionImpl::validate`] devolve o padrão e a validação real é
//!   [`EditorGoToLocation::validate_input`]. `// TODO(config)`: ligar quando `OptionInput` virar `Dynamic`.
//! - `enumDescriptions` e `deprecationMessage` ainda não existem em `ConfigurationPropertySchema`:
//!   `editor.gotoLocation.multiple` entra no esquema sem campos (o original só escreve a mensagem de
//!   depreciação, "This setting is deprecated, please use separate settings like
//!   'editor.editor.gotoLocation.multipleDefinitions' or 'editor.editor.gotoLocation.multipleImplementations'
//!   instead."). `// TODO(config)`.

use crate::config::editor_options::option_support::{string_enum, string_input};
use crate::config::editor_options::{
    ConfigurationPropertySchema, EditorOption, EditorOptionImpl, Opt, OptionInput, OptionSchema, SchemaValue,
};

string_enum! {
    /// `GoToLocationValues`: `'peek' | 'gotoAndPeek' | 'goto'`.
    pub enum GoToLocationValues {
        Peek => "peek",
        GotoAndPeek => "gotoAndPeek",
        Goto => "goto",
    }
}

/// Ids de comando aceitos nas chaves `alternative*Command` (a lista `alternativeCommandOptions`).
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

/// `IGotoLocationOptions`: configuração do "ir para" quando há vários destinos.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IGotoLocationOptions {
    /// `'peek' | 'gotoAndPeek' | 'goto'` (depreciado, use as chaves separadas).
    pub multiple: Option<String>,

    pub multiple_definitions: Option<String>,
    pub multiple_type_definitions: Option<String>,
    pub multiple_declarations: Option<String>,
    pub multiple_implementations: Option<String>,
    pub multiple_references: Option<String>,
    pub multiple_tests: Option<String>,

    pub alternative_definition_command: Option<String>,
    pub alternative_type_definition_command: Option<String>,
    pub alternative_declaration_command: Option<String>,
    pub alternative_implementation_command: Option<String>,
    pub alternative_reference_command: Option<String>,
    pub alternative_tests_command: Option<String>,
}

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

/// O `jsonSubset` do original mais a descrição de cada chave `multiple*`.
fn multiple_schema(description: &str) -> ConfigurationPropertySchema {
    ConfigurationPropertySchema {
        type_: Some("string".to_string()),
        default: Some(SchemaValue::String(GoToLocationValues::Peek.as_str().to_string())),
        enum_values: Some(GoToLocationValues::ALLOWED.iter().map(|s| (*s).to_string()).collect()),
        description: Some(description.to_string()),
        ..Default::default()
    }
}

/// Esquema de uma chave `alternative*Command`.
fn alternative_command_schema(default: &str, description: &str) -> ConfigurationPropertySchema {
    ConfigurationPropertySchema {
        type_: Some("string".to_string()),
        default: Some(SchemaValue::String(default.to_string())),
        enum_values: Some(ALTERNATIVE_COMMAND_OPTIONS.iter().map(|s| (*s).to_string()).collect()),
        description: Some(description.to_string()),
        ..Default::default()
    }
}

/// `EditorGoToLocation`.
pub struct EditorGoToLocation {
    default_value: GoToLocationOptions,
    schema: OptionSchema,
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
        let schema = OptionSchema::Map(vec![
            // `// TODO(config)`: só `deprecationMessage`, ver o topo do arquivo.
            ("editor.gotoLocation.multiple".to_string(), ConfigurationPropertySchema::default()),
            (
                "editor.gotoLocation.multipleDefinitions".to_string(),
                multiple_schema("Controls the behavior the 'Go to Definition'-command when multiple target locations exist."),
            ),
            (
                "editor.gotoLocation.multipleTypeDefinitions".to_string(),
                multiple_schema("Controls the behavior the 'Go to Type Definition'-command when multiple target locations exist."),
            ),
            (
                "editor.gotoLocation.multipleDeclarations".to_string(),
                multiple_schema("Controls the behavior the 'Go to Declaration'-command when multiple target locations exist."),
            ),
            (
                "editor.gotoLocation.multipleImplementations".to_string(),
                multiple_schema("Controls the behavior the 'Go to Implementations'-command when multiple target locations exist."),
            ),
            (
                "editor.gotoLocation.multipleReferences".to_string(),
                multiple_schema("Controls the behavior the 'Go to References'-command when multiple target locations exist."),
            ),
            (
                "editor.gotoLocation.alternativeDefinitionCommand".to_string(),
                alternative_command_schema(
                    &defaults.alternative_definition_command,
                    "Alternative command id that is being executed when the result of 'Go to Definition' is the current location.",
                ),
            ),
            (
                "editor.gotoLocation.alternativeTypeDefinitionCommand".to_string(),
                alternative_command_schema(
                    &defaults.alternative_type_definition_command,
                    "Alternative command id that is being executed when the result of 'Go to Type Definition' is the current location.",
                ),
            ),
            (
                "editor.gotoLocation.alternativeDeclarationCommand".to_string(),
                alternative_command_schema(
                    &defaults.alternative_declaration_command,
                    "Alternative command id that is being executed when the result of 'Go to Declaration' is the current location.",
                ),
            ),
            (
                "editor.gotoLocation.alternativeImplementationCommand".to_string(),
                alternative_command_schema(
                    &defaults.alternative_implementation_command,
                    "Alternative command id that is being executed when the result of 'Go to Implementation' is the current location.",
                ),
            ),
            (
                "editor.gotoLocation.alternativeReferenceCommand".to_string(),
                alternative_command_schema(
                    &defaults.alternative_reference_command,
                    "Alternative command id that is being executed when the result of 'Go to Reference' is the current location.",
                ),
            ),
        ]);
        Self { default_value: defaults, schema }
    }

    /// `validate` sobre a entrada tipada. `None` é a entrada que não é objeto (`undefined`, `null`...).
    pub fn validate_input(&self, input: Option<&IGotoLocationOptions>) -> GoToLocationOptions {
        let Some(input) = input else {
            return self.default_value.clone();
        };
        let defaults = &self.default_value;
        // O original usa `defaultValue.multiple` na chave `multiple` e `'peek'` nas demais, que é o mesmo valor.
        let multiple = |value: &Option<String>, default_value: GoToLocationValues| {
            GoToLocationValues::validate(&string_input(value.as_deref()), default_value)
        };
        // `EditorStringOption.string`: só vale se for string.
        let command = |value: &Option<String>, default_value: &str| value.clone().unwrap_or_else(|| default_value.to_string());
        GoToLocationOptions {
            multiple: multiple(&input.multiple, defaults.multiple),
            multiple_definitions: multiple(&input.multiple_definitions, GoToLocationValues::Peek),
            multiple_type_definitions: multiple(&input.multiple_type_definitions, GoToLocationValues::Peek),
            multiple_declarations: multiple(&input.multiple_declarations, GoToLocationValues::Peek),
            multiple_implementations: multiple(&input.multiple_implementations, GoToLocationValues::Peek),
            multiple_references: multiple(&input.multiple_references, GoToLocationValues::Peek),
            multiple_tests: multiple(&input.multiple_tests, GoToLocationValues::Peek),
            alternative_definition_command: command(&input.alternative_definition_command, &defaults.alternative_definition_command),
            alternative_type_definition_command: command(&input.alternative_type_definition_command, &defaults.alternative_type_definition_command),
            alternative_declaration_command: command(&input.alternative_declaration_command, &defaults.alternative_declaration_command),
            alternative_implementation_command: command(&input.alternative_implementation_command, &defaults.alternative_implementation_command),
            alternative_reference_command: command(&input.alternative_reference_command, &defaults.alternative_reference_command),
            alternative_tests_command: command(&input.alternative_tests_command, &defaults.alternative_tests_command),
        }
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

    fn schema(&self) -> Option<&OptionSchema> {
        Some(&self.schema)
    }

    fn validate(&self, _input: &OptionInput) -> GoToLocationOptions {
        // `// TODO(config)`: ver a divergência no topo do arquivo.
        self.default_value.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn some(text: &str) -> Option<String> {
        Some(text.to_string())
    }

    #[test]
    fn defaults_match_the_original() {
        let option = EditorGoToLocation::new();
        let defaults = EditorOptionImpl::default_value(&option);
        assert_eq!(defaults.multiple, GoToLocationValues::Peek);
        assert_eq!(defaults.multiple_tests, GoToLocationValues::Peek);
        assert_eq!(defaults.alternative_definition_command, "editor.action.goToReferences");
        assert_eq!(defaults.alternative_type_definition_command, "editor.action.goToReferences");
        assert_eq!(defaults.alternative_declaration_command, "editor.action.goToReferences");
        assert_eq!(defaults.alternative_implementation_command, "");
        assert_eq!(defaults.alternative_reference_command, "");
        assert_eq!(defaults.alternative_tests_command, "");
        assert_eq!(option.validate_input(None), defaults);
        assert_eq!(option.validate_input(Some(&IGotoLocationOptions::default())), defaults);
        assert_eq!(EditorOptionImpl::validate(&option, &OptionInput::Null), defaults);
    }

    #[test]
    fn multiple_values_validate_and_fall_back_to_peek() {
        let option = EditorGoToLocation::new();
        let input = IGotoLocationOptions {
            multiple: some("goto"),
            multiple_definitions: some("gotoAndPeek"),
            multiple_type_definitions: some("goto"),
            multiple_declarations: some("nonsense"),
            multiple_implementations: some(""),
            multiple_references: some("Peek"),
            multiple_tests: None,
            ..Default::default()
        };
        let value = option.validate_input(Some(&input));
        assert_eq!(value.multiple, GoToLocationValues::Goto);
        assert_eq!(value.multiple_definitions, GoToLocationValues::GotoAndPeek);
        assert_eq!(value.multiple_type_definitions, GoToLocationValues::Goto);
        assert_eq!(value.multiple_declarations, GoToLocationValues::Peek);
        assert_eq!(value.multiple_implementations, GoToLocationValues::Peek);
        assert_eq!(value.multiple_references, GoToLocationValues::Peek);
        assert_eq!(value.multiple_tests, GoToLocationValues::Peek);
    }

    #[test]
    fn alternative_commands_accept_any_string() {
        let option = EditorGoToLocation::new();
        let input = IGotoLocationOptions {
            alternative_definition_command: some("my.command"),
            alternative_type_definition_command: some(""),
            alternative_reference_command: some("editor.action.peekDefinition"),
            ..Default::default()
        };
        let value = option.validate_input(Some(&input));
        // Não há lista de valores permitidos na validação, só no esquema.
        assert_eq!(value.alternative_definition_command, "my.command");
        assert_eq!(value.alternative_type_definition_command, "");
        assert_eq!(value.alternative_declaration_command, "editor.action.goToReferences");
        assert_eq!(value.alternative_reference_command, "editor.action.peekDefinition");
        assert_eq!(value.alternative_tests_command, "");
    }

    #[test]
    fn string_enum_helpers() {
        assert_eq!(GoToLocationValues::from_text("goto"), Some(GoToLocationValues::Goto));
        assert_eq!(GoToLocationValues::from_text("Goto"), None);
        assert_eq!(GoToLocationValues::GotoAndPeek.as_str(), "gotoAndPeek");
        assert_eq!(GoToLocationValues::ALLOWED, ["peek", "gotoAndPeek", "goto"]);
        assert_eq!(GoToLocationValues::validate(&OptionInput::Number(1.0), GoToLocationValues::Goto), GoToLocationValues::Goto);
    }

    #[test]
    fn schema_has_eleven_keys_without_tests() {
        let option = EditorGoToLocation::new();
        let Some(OptionSchema::Map(entries)) = EditorOptionImpl::schema(&option) else { panic!("sem esquema") };
        assert_eq!(entries.len(), 11);
        assert!(!entries.iter().any(|(key, _)| key.contains("Tests")));
        let (_, definitions) = entries.iter().find(|(key, _)| key == "editor.gotoLocation.multipleDefinitions").unwrap();
        assert_eq!(definitions.default, Some(SchemaValue::String("peek".to_string())));
        assert_eq!(definitions.enum_values.as_ref().map(Vec::len), Some(3));
        let (_, alternative) = entries.iter().find(|(key, _)| key == "editor.gotoLocation.alternativeDefinitionCommand").unwrap();
        assert_eq!(alternative.default, Some(SchemaValue::String("editor.action.goToReferences".to_string())));
        assert_eq!(alternative.enum_values.as_ref().map(Vec::len), Some(12));
    }
}
