//! Porte de `InlineEditorSuggest`, `IInlineSuggestOptions` e `InternalInlineSuggestOptions`
//! (`editorOptions.ts`, linhas 4503 a 4795).
//!
//! A entrada (`IInlineSuggestOptions`, todos os campos opcionais) é um [`Dynamic`]; o valor
//! calculado é [`InternalInlineSuggestOptions`] (`Readonly<RequiredRecursive<IInlineSuggestOptions>>`),
//! com `edits` e `experimental` como structs próprias.
//!
//! O original valida `renderSideBySide` com `['never', 'auto']` e publica `['auto', 'never']` no
//! esquema; a ordem da lista de validação não muda o resultado, então o enum usa a ordem do esquema.

use je_base::dynamic::{Dynamic, Truthy};

use crate::config::editor_options::{boolean, clamped_int, string, strings_array, EditorOption, EditorOptionImpl, Opt};

string_enum! {
    /// `'prefix' | 'subword' | 'subwordSmart'`.
    pub enum InlineSuggestMode {
        Prefix => "prefix",
        Subword => "subword",
        SubwordSmart => "subwordSmart",
    }
}

string_enum! {
    /// `'always' | 'onHover' | 'never'`.
    pub enum InlineSuggestShowToolbar {
        Always => "always",
        OnHover => "onHover",
        Never => "never",
    }
}

string_enum! {
    /// `'always' | 'horizontal' | 'never'`.
    pub enum InlineEditsAllowCodeShifting {
        Always => "always",
        Horizontal => "horizontal",
        Never => "never",
    }
}

string_enum! {
    /// `'never' | 'auto'` (na ordem do esquema: `auto`, `never`).
    pub enum InlineEditsRenderSideBySide {
        Auto => "auto",
        Never => "never",
    }
}

string_enum! {
    /// `'always' | 'never' | 'whenSuggestListIsIncomplete'`.
    pub enum ShowOnSuggestConflict {
        Always => "always",
        Never => "never",
        WhenSuggestListIsIncomplete => "whenSuggestListIsIncomplete",
    }
}

/// `InternalInlineSuggestOptions['edits']`.
#[derive(Clone, Debug, PartialEq)]
pub struct InlineSuggestEditsOptions {
    /// Interno.
    pub enabled: bool,
    pub show_collapsed: bool,
    pub render_side_by_side: InlineEditsRenderSideBySide,
    pub allow_code_shifting: InlineEditsAllowCodeShifting,
    pub show_long_distance_hint: bool,
    /// Linhas de contexto acima e abaixo da linha alvo na prévia da sugestão de longa distância.
    pub long_distance_hint_context_line_count: f64,
}

/// `InternalInlineSuggestOptions['experimental']`.
#[derive(Clone, Debug, PartialEq)]
pub struct InlineSuggestExperimentalOptions {
    /// Interno: ids de extensão (separados por vírgula) cujas sugestões inline são suprimidas.
    pub suppress_inline_suggestions: String,
    pub show_on_suggest_conflict: ShowOnSuggestConflict,
    /// Interno.
    pub empty_response_information: bool,
}

/// `InternalInlineSuggestOptions`.
#[derive(Clone, Debug, PartialEq)]
pub struct InternalInlineSuggestOptions {
    pub enabled: bool,
    pub mode: InlineSuggestMode,
    pub show_toolbar: InlineSuggestShowToolbar,
    pub suppress_suggestions: bool,
    /// Não limpa as sugestões inline ativas quando o editor perde o foco.
    pub keep_on_blur: bool,
    /// `string | 'default'`.
    pub font_family: String,
    pub syntax_highlighting_enabled: bool,
    pub min_show_delay: f64,
    pub suppress_in_snippet_mode: bool,
    pub edits: InlineSuggestEditsOptions,
    /// Interno.
    pub trigger_command_on_provider_change: bool,
    /// Interno.
    pub experimental: InlineSuggestExperimentalOptions,
}

/// Chave tipada da opção `inlineSuggest`.
pub const INLINE_SUGGEST: Opt<InternalInlineSuggestOptions> = Opt::new(EditorOption::InlineSuggest);

/// Um valor de texto do esquema.
fn text(value: &str) -> Dynamic {
    Dynamic::string(value)
}

/// `experiment: { mode: 'auto' }`.
fn experiment() -> Dynamic {
    Dynamic::object([("mode", Dynamic::string("auto"))])
}

/// `!_input || typeof _input !== 'object'`.
fn is_not_object(input: &Dynamic) -> bool {
    !input.is_truthy() || input.type_of() != "object"
}

/// `InlineEditorSuggest`.
pub struct InlineEditorSuggest {
    default_value: InternalInlineSuggestOptions,
    schema: Dynamic,
}

impl InlineEditorSuggest {
    pub fn new() -> Self {
        let defaults = InternalInlineSuggestOptions {
            enabled: true,
            mode: InlineSuggestMode::SubwordSmart,
            show_toolbar: InlineSuggestShowToolbar::OnHover,
            suppress_suggestions: false,
            keep_on_blur: false,
            font_family: "default".to_string(),
            syntax_highlighting_enabled: true,
            min_show_delay: 0.0,
            suppress_in_snippet_mode: true,
            edits: InlineSuggestEditsOptions {
                enabled: true,
                show_collapsed: false,
                render_side_by_side: InlineEditsRenderSideBySide::Auto,
                allow_code_shifting: InlineEditsAllowCodeShifting::Always,
                show_long_distance_hint: true,
                long_distance_hint_context_line_count: 0.0,
            },
            trigger_command_on_provider_change: false,
            experimental: InlineSuggestExperimentalOptions {
                suppress_inline_suggestions: String::new(),
                show_on_suggest_conflict: ShowOnSuggestConflict::Never,
                empty_response_information: true,
            },
        };
        let boolean_type = || text("boolean");
        let string_type = || text("string");
        let schema = Dynamic::object([
            (
                "editor.inlineSuggest.enabled",
                Dynamic::object([
                    ("type", boolean_type()),
                    ("default", Dynamic::Bool(defaults.enabled)),
                    ("description", text("Controls whether to automatically show inline suggestions in the editor.")),
                ]),
            ),
            (
                "editor.inlineSuggest.showToolbar",
                Dynamic::object([
                    ("type", string_type()),
                    ("default", text(defaults.show_toolbar.as_str())),
                    ("enum", strings_array(InlineSuggestShowToolbar::ALLOWED)),
                    (
                        "enumDescriptions",
                        strings_array(&[
                            "Show the inline suggestion toolbar whenever an inline suggestion is shown.",
                            "Show the inline suggestion toolbar when hovering over an inline suggestion.",
                            "Never show the inline suggestion toolbar.",
                        ]),
                    ),
                    ("description", text("Controls when to show the inline suggestion toolbar.")),
                ]),
            ),
            (
                "editor.inlineSuggest.syntaxHighlightingEnabled",
                Dynamic::object([
                    ("type", boolean_type()),
                    ("default", Dynamic::Bool(defaults.syntax_highlighting_enabled)),
                    ("description", text("Controls whether to show syntax highlighting for inline suggestions in the editor.")),
                ]),
            ),
            (
                "editor.inlineSuggest.suppressSuggestions",
                Dynamic::object([
                    ("type", boolean_type()),
                    ("default", Dynamic::Bool(defaults.suppress_suggestions)),
                    (
                        "description",
                        text("Controls how inline suggestions interact with the suggest widget. If enabled, the suggest widget is not shown automatically when inline suggestions are available."),
                    ),
                ]),
            ),
            (
                "editor.inlineSuggest.suppressInSnippetMode",
                Dynamic::object([
                    ("type", boolean_type()),
                    ("default", Dynamic::Bool(defaults.suppress_in_snippet_mode)),
                    ("description", text("Controls whether inline suggestions are suppressed when in snippet mode.")),
                ]),
            ),
            (
                "editor.inlineSuggest.minShowDelay",
                Dynamic::object([
                    ("type", text("number")),
                    ("default", Dynamic::Number(0.0)),
                    ("minimum", Dynamic::Number(0.0)),
                    ("maximum", Dynamic::Number(10000.0)),
                    ("description", text("Controls the minimal delay in milliseconds after which inline suggestions are shown after typing.")),
                ]),
            ),
            (
                "editor.inlineSuggest.experimental.suppressInlineSuggestions",
                Dynamic::object([
                    ("type", string_type()),
                    ("default", text(&defaults.experimental.suppress_inline_suggestions)),
                    ("tags", strings_array(&["experimental"])),
                    ("description", text("Suppresses inline completions for specified extension IDs -- comma separated.")),
                    ("experiment", experiment()),
                ]),
            ),
            (
                "editor.inlineSuggest.experimental.emptyResponseInformation",
                Dynamic::object([
                    ("type", boolean_type()),
                    ("default", Dynamic::Bool(defaults.experimental.empty_response_information)),
                    ("tags", strings_array(&["experimental"])),
                    ("description", text("Controls whether to send request information from the inline suggestion provider.")),
                    ("experiment", experiment()),
                ]),
            ),
            (
                "editor.inlineSuggest.triggerCommandOnProviderChange",
                Dynamic::object([
                    ("type", boolean_type()),
                    ("default", Dynamic::Bool(defaults.trigger_command_on_provider_change)),
                    ("tags", strings_array(&["experimental"])),
                    ("description", text("Controls whether to trigger a command when the inline suggestion provider changes.")),
                    ("experiment", experiment()),
                ]),
            ),
            (
                "editor.inlineSuggest.experimental.showOnSuggestConflict",
                Dynamic::object([
                    ("type", string_type()),
                    ("default", text(defaults.experimental.show_on_suggest_conflict.as_str())),
                    ("tags", strings_array(&["experimental"])),
                    ("enum", strings_array(ShowOnSuggestConflict::ALLOWED)),
                    ("description", text("Controls whether to show inline suggestions when there is a suggest conflict.")),
                    ("experiment", experiment()),
                ]),
            ),
            (
                "editor.inlineSuggest.fontFamily",
                Dynamic::object([
                    ("type", string_type()),
                    ("default", text(&defaults.font_family)),
                    ("description", text("Controls the font family of the inline suggestions.")),
                ]),
            ),
            (
                "editor.inlineSuggest.edits.allowCodeShifting",
                Dynamic::object([
                    ("type", string_type()),
                    ("default", text(defaults.edits.allow_code_shifting.as_str())),
                    (
                        "description",
                        text("Controls whether showing a suggestion will shift the code to make space for the suggestion inline."),
                    ),
                    ("enum", strings_array(InlineEditsAllowCodeShifting::ALLOWED)),
                    ("tags", strings_array(&["nextEditSuggestions"])),
                ]),
            ),
            (
                "editor.inlineSuggest.edits.showLongDistanceHint",
                Dynamic::object([
                    ("type", boolean_type()),
                    ("default", Dynamic::Bool(defaults.edits.show_long_distance_hint)),
                    ("description", text("Controls whether long distance inline suggestions are shown.")),
                    ("tags", strings_array(&["nextEditSuggestions", "experimental"])),
                ]),
            ),
            (
                "editor.inlineSuggest.edits.longDistanceHintContextLineCount",
                Dynamic::object([
                    ("type", text("number")),
                    ("default", Dynamic::Number(defaults.edits.long_distance_hint_context_line_count)),
                    ("minimum", Dynamic::Number(0.0)),
                    ("maximum", Dynamic::Number(10.0)),
                    (
                        "description",
                        text("Controls how many lines of surrounding context are shown above and below the target line in the long distance inline suggestion preview. Set to 0 to only show the target line."),
                    ),
                    ("tags", strings_array(&["nextEditSuggestions", "experimental"])),
                    ("experiment", experiment()),
                ]),
            ),
            (
                "editor.inlineSuggest.edits.renderSideBySide",
                Dynamic::object([
                    ("type", string_type()),
                    ("default", text(defaults.edits.render_side_by_side.as_str())),
                    ("description", text("Controls whether larger suggestions can be shown side by side.")),
                    ("enum", strings_array(InlineEditsRenderSideBySide::ALLOWED)),
                    (
                        "enumDescriptions",
                        strings_array(&[
                            "Larger suggestions will show side by side if there is enough space, otherwise they will be shown below.",
                            "Larger suggestions are never shown side by side and will always be shown below.",
                        ]),
                    ),
                    ("tags", strings_array(&["nextEditSuggestions"])),
                ]),
            ),
            (
                "editor.inlineSuggest.edits.showCollapsed",
                Dynamic::object([
                    ("type", boolean_type()),
                    ("default", Dynamic::Bool(defaults.edits.show_collapsed)),
                    ("description", text("Controls whether the suggestion will show as collapsed until jumping to it.")),
                    ("tags", strings_array(&["nextEditSuggestions"])),
                ]),
            ),
        ]);
        Self { default_value: defaults, schema }
    }

    /// `_validateEdits`.
    fn validate_edits(&self, input: &Dynamic) -> InlineSuggestEditsOptions {
        let defaults = &self.default_value.edits;
        if is_not_object(input) {
            return defaults.clone();
        }
        InlineSuggestEditsOptions {
            enabled: boolean(&input.get_str("enabled"), defaults.enabled),
            show_collapsed: boolean(&input.get_str("showCollapsed"), defaults.show_collapsed),
            allow_code_shifting: InlineEditsAllowCodeShifting::validate(&input.get_str("allowCodeShifting"), defaults.allow_code_shifting),
            show_long_distance_hint: boolean(&input.get_str("showLongDistanceHint"), defaults.show_long_distance_hint),
            long_distance_hint_context_line_count: clamped_int(
                &input.get_str("longDistanceHintContextLineCount"),
                defaults.long_distance_hint_context_line_count,
                0.0,
                10.0,
            ),
            render_side_by_side: InlineEditsRenderSideBySide::validate(&input.get_str("renderSideBySide"), defaults.render_side_by_side),
        }
    }

    /// `_validateExperimental`.
    fn validate_experimental(&self, input: &Dynamic) -> InlineSuggestExperimentalOptions {
        let defaults = &self.default_value.experimental;
        if is_not_object(input) {
            return defaults.clone();
        }
        InlineSuggestExperimentalOptions {
            suppress_inline_suggestions: string(&input.get_str("suppressInlineSuggestions"), &defaults.suppress_inline_suggestions),
            show_on_suggest_conflict: ShowOnSuggestConflict::validate(&input.get_str("showOnSuggestConflict"), defaults.show_on_suggest_conflict),
            empty_response_information: boolean(&input.get_str("emptyResponseInformation"), defaults.empty_response_information),
        }
    }
}

impl Default for InlineEditorSuggest {
    fn default() -> Self {
        Self::new()
    }
}

impl EditorOptionImpl for InlineEditorSuggest {
    type Value = InternalInlineSuggestOptions;

    fn id(&self) -> EditorOption {
        EditorOption::InlineSuggest
    }

    fn name(&self) -> &str {
        "inlineSuggest"
    }

    fn default_value(&self) -> InternalInlineSuggestOptions {
        self.default_value.clone()
    }

    fn schema(&self) -> Option<&Dynamic> {
        Some(&self.schema)
    }

    fn validate(&self, input: &Dynamic) -> InternalInlineSuggestOptions {
        if is_not_object(input) {
            return self.default_value.clone();
        }
        let defaults = &self.default_value;
        InternalInlineSuggestOptions {
            enabled: boolean(&input.get_str("enabled"), defaults.enabled),
            mode: InlineSuggestMode::validate(&input.get_str("mode"), defaults.mode),
            show_toolbar: InlineSuggestShowToolbar::validate(&input.get_str("showToolbar"), defaults.show_toolbar),
            suppress_suggestions: boolean(&input.get_str("suppressSuggestions"), defaults.suppress_suggestions),
            keep_on_blur: boolean(&input.get_str("keepOnBlur"), defaults.keep_on_blur),
            font_family: string(&input.get_str("fontFamily"), &defaults.font_family),
            syntax_highlighting_enabled: boolean(&input.get_str("syntaxHighlightingEnabled"), defaults.syntax_highlighting_enabled),
            // O padrão do original é o literal `0`, não `defaultValue.minShowDelay` (mesmo valor).
            min_show_delay: clamped_int(&input.get_str("minShowDelay"), 0.0, 0.0, 10000.0),
            suppress_in_snippet_mode: boolean(&input.get_str("suppressInSnippetMode"), defaults.suppress_in_snippet_mode),
            edits: self.validate_edits(&input.get_str("edits")),
            trigger_command_on_provider_change: boolean(
                &input.get_str("triggerCommandOnProviderChange"),
                defaults.trigger_command_on_provider_change,
            ),
            experimental: self.validate_experimental(&input.get_str("experimental")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(value: &str) -> Dynamic {
        Dynamic::string(value)
    }

    fn num(value: f64) -> Dynamic {
        Dynamic::Number(value)
    }

    fn validate(entries: Vec<(&str, Dynamic)>) -> InternalInlineSuggestOptions {
        EditorOptionImpl::validate(&InlineEditorSuggest::new(), &Dynamic::object(entries))
    }

    fn defaults() -> InternalInlineSuggestOptions {
        EditorOptionImpl::default_value(&InlineEditorSuggest::new())
    }

    #[test]
    fn defaults_match_the_original() {
        let defaults = defaults();
        assert!(defaults.enabled && defaults.syntax_highlighting_enabled && defaults.suppress_in_snippet_mode);
        assert!(!defaults.suppress_suggestions && !defaults.keep_on_blur && !defaults.trigger_command_on_provider_change);
        assert_eq!(defaults.mode, InlineSuggestMode::SubwordSmart);
        assert_eq!(defaults.show_toolbar, InlineSuggestShowToolbar::OnHover);
        assert_eq!(defaults.font_family, "default");
        assert_eq!(defaults.min_show_delay, 0.0);
        assert!(defaults.edits.enabled && !defaults.edits.show_collapsed && defaults.edits.show_long_distance_hint);
        assert_eq!(defaults.edits.render_side_by_side, InlineEditsRenderSideBySide::Auto);
        assert_eq!(defaults.edits.allow_code_shifting, InlineEditsAllowCodeShifting::Always);
        assert_eq!(defaults.edits.long_distance_hint_context_line_count, 0.0);
        assert_eq!(defaults.experimental.suppress_inline_suggestions, "");
        assert_eq!(defaults.experimental.show_on_suggest_conflict, ShowOnSuggestConflict::Never);
        assert!(defaults.experimental.empty_response_information);
        // Objeto vazio valida para o padrão; o que não é objeto devolve o padrão.
        assert_eq!(validate(vec![]), defaults);
        let option = InlineEditorSuggest::new();
        for input in [Dynamic::Undefined, Dynamic::Null, Dynamic::Bool(true), num(1.0), text("x"), num(0.0)] {
            assert_eq!(EditorOptionImpl::validate(&option, &input), defaults);
        }
    }

    #[test]
    fn top_level_fields() {
        let value = validate(vec![
            ("enabled", Dynamic::Bool(false)),
            ("mode", text("prefix")),
            ("showToolbar", text("always")),
            ("suppressSuggestions", Dynamic::Bool(true)),
            ("keepOnBlur", text("true")),
            ("fontFamily", text("Fira Code")),
            ("syntaxHighlightingEnabled", text("false")),
            ("suppressInSnippetMode", num(0.0)),
            ("triggerCommandOnProviderChange", Dynamic::Bool(true)),
        ]);
        assert!(!value.enabled && !value.syntax_highlighting_enabled && !value.suppress_in_snippet_mode);
        assert!(value.suppress_suggestions && value.keep_on_blur && value.trigger_command_on_provider_change);
        assert_eq!(value.mode, InlineSuggestMode::Prefix);
        assert_eq!(value.show_toolbar, InlineSuggestShowToolbar::Always);
        assert_eq!(value.font_family, "Fira Code");
        assert_eq!(validate(vec![("mode", text("subword"))]).mode, InlineSuggestMode::Subword);
    }

    #[test]
    fn invalid_values_fall_back_to_defaults() {
        let value = validate(vec![
            ("mode", text("suffix")),
            ("showToolbar", num(1.0)),
            ("fontFamily", num(3.0)),
            ("minShowDelay", text("abc")),
            ("edits", text("x")),
            ("experimental", Dynamic::Null),
        ]);
        assert_eq!(value, defaults());
    }

    #[test]
    fn min_show_delay_is_clamped_to_zero_and_ten_thousand() {
        assert_eq!(validate(vec![("minShowDelay", num(250.0))]).min_show_delay, 250.0);
        assert_eq!(validate(vec![("minShowDelay", num(-5.0))]).min_show_delay, 0.0);
        assert_eq!(validate(vec![("minShowDelay", num(99999.0))]).min_show_delay, 10000.0);
        assert_eq!(validate(vec![("minShowDelay", text("42"))]).min_show_delay, 42.0);
        assert_eq!(validate(vec![("minShowDelay", num(7.9))]).min_show_delay, 7.0);
    }

    #[test]
    fn edits_are_validated_field_by_field() {
        let edits = Dynamic::object([
            ("enabled", Dynamic::Bool(false)),
            ("showCollapsed", Dynamic::Bool(true)),
            ("allowCodeShifting", text("horizontal")),
            ("showLongDistanceHint", Dynamic::Bool(false)),
            ("longDistanceHintContextLineCount", num(50.0)),
            ("renderSideBySide", text("never")),
        ]);
        let value = validate(vec![("edits", edits)]).edits;
        assert!(!value.enabled && value.show_collapsed && !value.show_long_distance_hint);
        assert_eq!(value.allow_code_shifting, InlineEditsAllowCodeShifting::Horizontal);
        assert_eq!(value.long_distance_hint_context_line_count, 10.0);
        assert_eq!(value.render_side_by_side, InlineEditsRenderSideBySide::Never);

        // Valores inválidos em `edits` caem no padrão de cada campo.
        let edits = Dynamic::object([("allowCodeShifting", text("vertical")), ("renderSideBySide", text("always")), ("longDistanceHintContextLineCount", num(-3.0))]);
        let value = validate(vec![("edits", edits)]).edits;
        assert_eq!(value.allow_code_shifting, InlineEditsAllowCodeShifting::Always);
        assert_eq!(value.render_side_by_side, InlineEditsRenderSideBySide::Auto);
        assert_eq!(value.long_distance_hint_context_line_count, 0.0);

        // Um array é objeto: todos os campos caem no padrão.
        assert_eq!(validate(vec![("edits", Dynamic::array([]))]).edits, defaults().edits);
    }

    #[test]
    fn experimental_is_validated_field_by_field() {
        let experimental = Dynamic::object([
            ("suppressInlineSuggestions", text("a.b,c.d")),
            ("showOnSuggestConflict", text("whenSuggestListIsIncomplete")),
            ("emptyResponseInformation", Dynamic::Bool(false)),
        ]);
        let value = validate(vec![("experimental", experimental)]).experimental;
        assert_eq!(value.suppress_inline_suggestions, "a.b,c.d");
        assert_eq!(value.show_on_suggest_conflict, ShowOnSuggestConflict::WhenSuggestListIsIncomplete);
        assert!(!value.empty_response_information);

        let experimental = Dynamic::object([("suppressInlineSuggestions", num(1.0)), ("showOnSuggestConflict", text("sometimes"))]);
        assert_eq!(validate(vec![("experimental", experimental)]).experimental, defaults().experimental);
    }

    #[test]
    fn schema_keeps_the_original_keys_and_order() {
        let option = InlineEditorSuggest::new();
        let schema = EditorOptionImpl::schema(&option).expect("sem esquema");
        assert_eq!(
            schema.own_keys(),
            vec![
                "editor.inlineSuggest.enabled",
                "editor.inlineSuggest.showToolbar",
                "editor.inlineSuggest.syntaxHighlightingEnabled",
                "editor.inlineSuggest.suppressSuggestions",
                "editor.inlineSuggest.suppressInSnippetMode",
                "editor.inlineSuggest.minShowDelay",
                "editor.inlineSuggest.experimental.suppressInlineSuggestions",
                "editor.inlineSuggest.experimental.emptyResponseInformation",
                "editor.inlineSuggest.triggerCommandOnProviderChange",
                "editor.inlineSuggest.experimental.showOnSuggestConflict",
                "editor.inlineSuggest.fontFamily",
                "editor.inlineSuggest.edits.allowCodeShifting",
                "editor.inlineSuggest.edits.showLongDistanceHint",
                "editor.inlineSuggest.edits.longDistanceHintContextLineCount",
                "editor.inlineSuggest.edits.renderSideBySide",
                "editor.inlineSuggest.edits.showCollapsed",
            ]
        );
        let toolbar = schema.get_str("editor.inlineSuggest.showToolbar");
        assert_eq!(toolbar.own_keys(), vec!["type", "default", "enum", "enumDescriptions", "description"]);
        assert_eq!(toolbar.get_str("default"), text("onHover"));
        assert_eq!(toolbar.get_str("enum").get_str("1"), text("onHover"));
        let delay = schema.get_str("editor.inlineSuggest.minShowDelay");
        assert_eq!(delay.own_keys(), vec!["type", "default", "minimum", "maximum", "description"]);
        assert_eq!(delay.get_str("maximum"), num(10000.0));
        let conflict = schema.get_str("editor.inlineSuggest.experimental.showOnSuggestConflict");
        assert_eq!(conflict.own_keys(), vec!["type", "default", "tags", "enum", "description", "experiment"]);
        assert_eq!(conflict.get_str("experiment").get_str("mode"), text("auto"));
        assert_eq!(conflict.get_str("enum").get_str("length"), num(3.0));
        let count = schema.get_str("editor.inlineSuggest.edits.longDistanceHintContextLineCount");
        assert_eq!(count.own_keys(), vec!["type", "default", "minimum", "maximum", "description", "tags", "experiment"]);
        assert_eq!(count.get_str("maximum"), num(10.0));
        assert_eq!(count.get_str("tags").get_str("length"), num(2.0));
        let side = schema.get_str("editor.inlineSuggest.edits.renderSideBySide");
        assert_eq!(side.own_keys(), vec!["type", "default", "description", "enum", "enumDescriptions", "tags"]);
        assert_eq!(side.get_str("enum").get_str("0"), text("auto"));
        assert_eq!(side.get_str("default"), text("auto"));
        let shifting = schema.get_str("editor.inlineSuggest.edits.allowCodeShifting");
        assert_eq!(shifting.own_keys(), vec!["type", "default", "description", "enum", "tags"]);
        let suppress = schema.get_str("editor.inlineSuggest.experimental.suppressInlineSuggestions");
        assert_eq!(suppress.own_keys(), vec!["type", "default", "tags", "description", "experiment"]);
        assert_eq!(suppress.get_str("default"), text(""));
        assert_eq!(schema.get_str("editor.inlineSuggest.fontFamily").get_str("default"), text("default"));
    }
}
