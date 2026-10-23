//! Porte de `EditorMinimap` e `IEditorMinimapOptions` (`editorOptions.ts`, linhas 3406 a 3623).

use je_base::dynamic::{Dynamic, Truthy};
use je_base::JsString;

use crate::config::editor_options::{
    boolean, clamp_float, clamped_int, float, strings_array, EditorOption, EditorOptionImpl, Opt,
};

string_enum! {
    /// `'none' | 'mouseover' | 'scroll'`.
    pub enum MinimapAutohide {
        None => "none",
        Mouseover => "mouseover",
        Scroll => "scroll",
    }
}

string_enum! {
    /// `'proportional' | 'fill' | 'fit'`.
    pub enum MinimapSize {
        Proportional => "proportional",
        Fill => "fill",
        Fit => "fit",
    }
}

string_enum! {
    /// `'right' | 'left'`, na ordem da lista de `validate` (o esquema lista `left` antes).
    pub enum MinimapSide {
        Right => "right",
        Left => "left",
    }
}

string_enum! {
    /// `'always' | 'mouseover'`.
    pub enum MinimapShowSlider {
        Always => "always",
        Mouseover => "mouseover",
    }
}

/// `EditorMinimapOptions` (`Readonly<Required<IEditorMinimapOptions>>`).
#[derive(Clone, Debug, PartialEq)]
pub struct EditorMinimapOptions {
    /// Liga a renderização do minimapa.
    pub enabled: bool,
    /// O tamanho do minimapa.
    pub size: MinimapSize,
    /// O lado do editor em que o minimapa aparece.
    pub side: MinimapSide,
    /// Quando o slider aparece.
    pub show_slider: MinimapShowSlider,
    /// Se o minimapa se esconde sozinho.
    pub autohide: MinimapAutohide,
    /// Desenha os caracteres da linha, em vez de blocos de cor.
    pub render_characters: bool,
    /// Largura máxima, em colunas.
    pub max_column: f64,
    /// Tamanho relativo da fonte: 1, 2 ou 3.
    pub scale: f64,
    /// Regiões nomeadas viram cabeçalhos de seção.
    pub show_region_section_headers: bool,
    /// Comentários `MARK:` viram cabeçalhos de seção.
    pub show_mark_section_headers: bool,
    /// A expressão regular dos cabeçalhos de seção (precisa do grupo nomeado `label`).
    pub mark_section_header_regex: String,
    /// Tamanho da fonte dos cabeçalhos de seção.
    pub section_header_font_size: f64,
    /// Espaço (em px de CSS) entre os caracteres dos cabeçalhos de seção.
    pub section_header_letter_spacing: f64,
}

/// Chave tipada da opção `minimap`.
pub const MINIMAP: Opt<EditorMinimapOptions> = Opt::new(EditorOption::Minimap);

/// O `markSectionHeaderRegex` padrão (`'\\bMARK:\\s*(?<separator>\-?)\\s*(?<label>.*)$'` no original; o
/// `\-` de um literal de string do JavaScript é só `-`).
const DEFAULT_MARK_SECTION_HEADER_REGEX: &str = r"\bMARK:\s*(?<separator>-?)\s*(?<label>.*)$";

/// `new RegExp(source, 'd')` sem lançar: a expressão compila?
fn is_valid_regex(source: &JsString) -> bool {
    zyarr::RegExp::new(source.units(), &[u16::from(b'd')]).is_ok()
}

/// `{ type, default, description }`.
fn entry(type_name: &str, default: Dynamic, description: &str) -> Dynamic {
    Dynamic::object([
        ("type", Dynamic::string(type_name)),
        ("default", default),
        ("description", Dynamic::string(description)),
    ])
}

/// `{ type: 'string', enum, enumDescriptions?, default, description }`.
fn string_entry(allowed: &[&str], enum_descriptions: Option<&[&str]>, default: &str, description: &str) -> Dynamic {
    let mut entries = vec![("type", Dynamic::string("string")), ("enum", strings_array(allowed))];
    if let Some(descriptions) = enum_descriptions {
        entries.push(("enumDescriptions", strings_array(descriptions)));
    }
    entries.push(("default", Dynamic::string(default)));
    entries.push(("description", Dynamic::string(description)));
    Dynamic::object(entries)
}

/// `EditorMinimap`.
pub struct EditorMinimap {
    default_value: EditorMinimapOptions,
    schema: Dynamic,
}

impl EditorMinimap {
    pub fn new() -> Self {
        let defaults = EditorMinimapOptions {
            enabled: true,
            size: MinimapSize::Proportional,
            side: MinimapSide::Right,
            show_slider: MinimapShowSlider::Mouseover,
            autohide: MinimapAutohide::None,
            render_characters: true,
            max_column: 120.0,
            scale: 1.0,
            show_region_section_headers: true,
            show_mark_section_headers: true,
            mark_section_header_regex: DEFAULT_MARK_SECTION_HEADER_REGEX.to_string(),
            section_header_font_size: 9.0,
            section_header_letter_spacing: 1.0,
        };
        let schema = Dynamic::object([
            (
                "editor.minimap.enabled",
                entry("boolean", Dynamic::Bool(defaults.enabled), "Controls whether the minimap is shown."),
            ),
            (
                "editor.minimap.autohide",
                string_entry(
                    MinimapAutohide::ALLOWED,
                    Some(&[
                        "The minimap is always shown.",
                        "The minimap is hidden when mouse is not over the minimap and shown when mouse is over the minimap.",
                        "The minimap is only shown when the editor is scrolled",
                    ]),
                    defaults.autohide.as_str(),
                    "Controls whether the minimap is hidden automatically.",
                ),
            ),
            (
                "editor.minimap.size",
                string_entry(
                    MinimapSize::ALLOWED,
                    Some(&[
                        "The minimap has the same size as the editor contents (and might scroll).",
                        "The minimap will stretch or shrink as necessary to fill the height of the editor (no scrolling).",
                        "The minimap will shrink as necessary to never be larger than the editor (no scrolling).",
                    ]),
                    defaults.size.as_str(),
                    "Controls the size of the minimap.",
                ),
            ),
            (
                "editor.minimap.side",
                // O esquema lista `left` antes de `right`, ao contrário da lista de `validate`.
                string_entry(&["left", "right"], None, defaults.side.as_str(), "Controls the side where to render the minimap."),
            ),
            (
                "editor.minimap.showSlider",
                string_entry(MinimapShowSlider::ALLOWED, None, defaults.show_slider.as_str(), "Controls when the minimap slider is shown."),
            ),
            (
                "editor.minimap.scale",
                Dynamic::object([
                    ("type", Dynamic::string("number")),
                    ("default", Dynamic::Number(defaults.scale)),
                    ("minimum", Dynamic::Number(1.0)),
                    ("maximum", Dynamic::Number(3.0)),
                    ("enum", Dynamic::array([Dynamic::Number(1.0), Dynamic::Number(2.0), Dynamic::Number(3.0)])),
                    ("description", Dynamic::string("Scale of content drawn in the minimap: 1, 2 or 3.")),
                ]),
            ),
            (
                "editor.minimap.renderCharacters",
                entry(
                    "boolean",
                    Dynamic::Bool(defaults.render_characters),
                    "Render the actual characters on a line as opposed to color blocks.",
                ),
            ),
            (
                "editor.minimap.maxColumn",
                entry(
                    "number",
                    Dynamic::Number(defaults.max_column),
                    "Limit the width of the minimap to render at most a certain number of columns.",
                ),
            ),
            (
                "editor.minimap.showRegionSectionHeaders",
                entry(
                    "boolean",
                    Dynamic::Bool(defaults.show_region_section_headers),
                    "Controls whether named regions are shown as section headers in the minimap.",
                ),
            ),
            (
                "editor.minimap.showMarkSectionHeaders",
                entry(
                    "boolean",
                    Dynamic::Bool(defaults.show_mark_section_headers),
                    "Controls whether MARK: comments are shown as section headers in the minimap.",
                ),
            ),
            (
                "editor.minimap.markSectionHeaderRegex",
                entry(
                    "string",
                    Dynamic::string(&defaults.mark_section_header_regex),
                    "Defines the regular expression used to find section headers in comments. The regex must contain a named match group `label` (written as `(?<label>.+)`) that encapsulates the section header, otherwise it will not work. Optionally you can include another match group named `separator`. Use \\n in the pattern to match multi-line headers.",
                ),
            ),
            (
                "editor.minimap.sectionHeaderFontSize",
                entry(
                    "number",
                    Dynamic::Number(defaults.section_header_font_size),
                    "Controls the font size of section headers in the minimap.",
                ),
            ),
            (
                "editor.minimap.sectionHeaderLetterSpacing",
                entry(
                    "number",
                    Dynamic::Number(defaults.section_header_letter_spacing),
                    "Controls the amount of space (in pixels) between characters of section header. This helps the readability of the header in small font sizes.",
                ),
            ),
        ]);
        Self { default_value: defaults, schema }
    }
}

impl Default for EditorMinimap {
    fn default() -> Self {
        Self::new()
    }
}

impl EditorOptionImpl for EditorMinimap {
    type Value = EditorMinimapOptions;

    fn id(&self) -> EditorOption {
        EditorOption::Minimap
    }

    fn name(&self) -> &str {
        "minimap"
    }

    fn default_value(&self) -> EditorMinimapOptions {
        self.default_value.clone()
    }

    fn schema(&self) -> Option<&Dynamic> {
        Some(&self.schema)
    }

    fn validate(&self, input: &Dynamic) -> EditorMinimapOptions {
        if !input.is_truthy() || input.type_of() != "object" {
            return self.default_value.clone();
        }
        let defaults = &self.default_value;

        // Valida a regex dos cabeçalhos de seção: expressão que não compila fica com o padrão.
        let mut mark_section_header_regex = defaults.mark_section_header_regex.clone();
        if let Dynamic::String(input_regex) = input.get_str("markSectionHeaderRegex") {
            if is_valid_regex(&input_regex) {
                mark_section_header_regex = input_regex.to_string_lossy();
            }
        }

        EditorMinimapOptions {
            enabled: boolean(&input.get_str("enabled"), defaults.enabled),
            autohide: MinimapAutohide::validate(&input.get_str("autohide"), defaults.autohide),
            size: MinimapSize::validate(&input.get_str("size"), defaults.size),
            side: MinimapSide::validate(&input.get_str("side"), defaults.side),
            show_slider: MinimapShowSlider::validate(&input.get_str("showSlider"), defaults.show_slider),
            render_characters: boolean(&input.get_str("renderCharacters"), defaults.render_characters),
            // O original passa `1` literal como padrão, não `defaultValue.scale` (o mesmo valor).
            scale: clamped_int(&input.get_str("scale"), 1.0, 1.0, 3.0),
            max_column: clamped_int(&input.get_str("maxColumn"), defaults.max_column, 1.0, 10000.0),
            show_region_section_headers: boolean(&input.get_str("showRegionSectionHeaders"), defaults.show_region_section_headers),
            show_mark_section_headers: boolean(&input.get_str("showMarkSectionHeaders"), defaults.show_mark_section_headers),
            mark_section_header_regex,
            section_header_font_size: clamp_float(
                float(&input.get_str("sectionHeaderFontSize"), defaults.section_header_font_size),
                4.0,
                32.0,
            ),
            section_header_letter_spacing: clamp_float(
                float(&input.get_str("sectionHeaderLetterSpacing"), defaults.section_header_letter_spacing),
                0.0,
                5.0,
            ),
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

    fn validate(entries: Vec<(&str, Dynamic)>) -> EditorMinimapOptions {
        EditorOptionImpl::validate(&EditorMinimap::new(), &Dynamic::object(entries))
    }

    fn defaults() -> EditorMinimapOptions {
        EditorOptionImpl::default_value(&EditorMinimap::new())
    }

    #[test]
    fn defaults_match_the_original() {
        let defaults = defaults();
        assert!(defaults.enabled && defaults.render_characters);
        assert!(defaults.show_region_section_headers && defaults.show_mark_section_headers);
        assert_eq!(defaults.size, MinimapSize::Proportional);
        assert_eq!(defaults.side, MinimapSide::Right);
        assert_eq!(defaults.show_slider, MinimapShowSlider::Mouseover);
        assert_eq!(defaults.autohide, MinimapAutohide::None);
        assert_eq!((defaults.max_column, defaults.scale), (120.0, 1.0));
        assert_eq!((defaults.section_header_font_size, defaults.section_header_letter_spacing), (9.0, 1.0));
        assert_eq!(defaults.mark_section_header_regex, r"\bMARK:\s*(?<separator>-?)\s*(?<label>.*)$");
        assert_eq!(validate(vec![]), defaults);
        let option = EditorMinimap::new();
        for input in [Dynamic::Undefined, Dynamic::Null, Dynamic::Bool(true), num(1.0), text("minimap")] {
            assert_eq!(EditorOptionImpl::validate(&option, &input), defaults);
        }
    }

    #[test]
    fn enums_accept_only_the_listed_strings() {
        let value = validate(vec![
            ("autohide", text("scroll")),
            ("size", text("fit")),
            ("side", text("left")),
            ("showSlider", text("always")),
        ]);
        assert_eq!(value.autohide, MinimapAutohide::Scroll);
        assert_eq!(value.size, MinimapSize::Fit);
        assert_eq!(value.side, MinimapSide::Left);
        assert_eq!(value.show_slider, MinimapShowSlider::Always);
        let invalid = validate(vec![
            ("autohide", text("sometimes")),
            ("size", num(1.0)),
            ("side", text("top")),
            ("showSlider", Dynamic::Bool(true)),
        ]);
        assert_eq!(invalid, defaults());
    }

    #[test]
    fn scale_and_max_column_are_clamped_integers() {
        assert_eq!(validate(vec![("scale", num(2.0))]).scale, 2.0);
        assert_eq!(validate(vec![("scale", num(2.9))]).scale, 2.0);
        assert_eq!(validate(vec![("scale", num(7.0))]).scale, 3.0);
        assert_eq!(validate(vec![("scale", num(0.0))]).scale, 1.0);
        assert_eq!(validate(vec![("scale", text("3"))]).scale, 3.0);
        assert_eq!(validate(vec![("scale", text("abc"))]).scale, 1.0);
        assert_eq!(validate(vec![("maxColumn", num(80.0))]).max_column, 80.0);
        assert_eq!(validate(vec![("maxColumn", num(0.0))]).max_column, 1.0);
        assert_eq!(validate(vec![("maxColumn", num(99999.0))]).max_column, 10000.0);
        assert_eq!(validate(vec![("maxColumn", Dynamic::Null)]).max_column, 120.0);
    }

    #[test]
    fn section_header_floats_are_clamped_but_not_truncated() {
        assert_eq!(validate(vec![("sectionHeaderFontSize", num(10.5))]).section_header_font_size, 10.5);
        assert_eq!(validate(vec![("sectionHeaderFontSize", num(1.0))]).section_header_font_size, 4.0);
        assert_eq!(validate(vec![("sectionHeaderFontSize", num(100.0))]).section_header_font_size, 32.0);
        assert_eq!(validate(vec![("sectionHeaderFontSize", text("12px"))]).section_header_font_size, 12.0);
        assert_eq!(validate(vec![("sectionHeaderFontSize", text("x"))]).section_header_font_size, 9.0);
        assert_eq!(validate(vec![("sectionHeaderLetterSpacing", num(0.25))]).section_header_letter_spacing, 0.25);
        assert_eq!(validate(vec![("sectionHeaderLetterSpacing", num(-3.0))]).section_header_letter_spacing, 0.0);
        assert_eq!(validate(vec![("sectionHeaderLetterSpacing", num(9.0))]).section_header_letter_spacing, 5.0);
    }

    #[test]
    fn booleans_follow_the_boolean_helper() {
        let value = validate(vec![
            ("enabled", text("false")),
            ("renderCharacters", num(0.0)),
            ("showRegionSectionHeaders", Dynamic::Bool(false)),
            ("showMarkSectionHeaders", Dynamic::Null),
        ]);
        assert!(!value.enabled && !value.render_characters);
        assert!(!value.show_region_section_headers && !value.show_mark_section_headers);
    }

    #[test]
    fn mark_section_header_regex_must_compile() {
        let valid = r"^={3,}\n^\/\/ *(?<label>[^\n]*?)\n^={3,}$";
        assert_eq!(validate(vec![("markSectionHeaderRegex", text(valid))]).mark_section_header_regex, valid);
        // Parêntese sem par: não compila, fica o padrão.
        assert_eq!(validate(vec![("markSectionHeaderRegex", text("(unclosed"))]).mark_section_header_regex, defaults().mark_section_header_regex);
        // Não é string: fica o padrão.
        assert_eq!(validate(vec![("markSectionHeaderRegex", num(1.0))]).mark_section_header_regex, defaults().mark_section_header_regex);
        // A string vazia compila.
        assert_eq!(validate(vec![("markSectionHeaderRegex", text(""))]).mark_section_header_regex, "");
    }

    #[test]
    fn schema_keeps_the_original_shape_and_order() {
        let option = EditorMinimap::new();
        let schema = EditorOptionImpl::schema(&option).expect("sem esquema");
        assert_eq!(
            schema.own_keys(),
            vec![
                "editor.minimap.enabled",
                "editor.minimap.autohide",
                "editor.minimap.size",
                "editor.minimap.side",
                "editor.minimap.showSlider",
                "editor.minimap.scale",
                "editor.minimap.renderCharacters",
                "editor.minimap.maxColumn",
                "editor.minimap.showRegionSectionHeaders",
                "editor.minimap.showMarkSectionHeaders",
                "editor.minimap.markSectionHeaderRegex",
                "editor.minimap.sectionHeaderFontSize",
                "editor.minimap.sectionHeaderLetterSpacing",
            ]
        );
        let autohide = schema.get_str("editor.minimap.autohide");
        assert_eq!(autohide.own_keys(), vec!["type", "enum", "enumDescriptions", "default", "description"]);
        assert_eq!(autohide.get_str("enumDescriptions").get_str("length"), num(3.0));
        let side = schema.get_str("editor.minimap.side");
        assert_eq!(side.own_keys(), vec!["type", "enum", "default", "description"]);
        assert_eq!(side.get_str("enum").get_str("0"), text("left"));
        assert_eq!(side.get_str("default"), text("right"));
        let scale = schema.get_str("editor.minimap.scale");
        assert_eq!(scale.own_keys(), vec!["type", "default", "minimum", "maximum", "enum", "description"]);
        assert_eq!(scale.get_str("enum").get_str("length"), num(3.0));
        let regex = schema.get_str("editor.minimap.markSectionHeaderRegex");
        assert_eq!(regex.get_str("default"), text(r"\bMARK:\s*(?<separator>-?)\s*(?<label>.*)$"));
        assert_eq!(schema.get_str("editor.minimap.enabled").own_keys(), vec!["type", "default", "description"]);
    }
}
