//! Porte de `src/vs/workbench/test/browser/componentFixtures/fixtureUtilsCss.test.ts`.
//!
//! Suíte `Component fixture theme CSS`, caso a caso e na mesma ordem (o `for (const reverse of [false, true])` do
//! original gera dois casos por iteração: um teste por combinação, na ordem em que o mocha os registra).
//!
//! Notas de tradução:
//! - **`ensureNoDisposablesAreLeakedInTestSuite()`.** É o [`LeakGuard`] criado no começo de cada caso; `disposables.add`
//!   vira `guard.add`. Salvar e restaurar `document.adoptedStyleSheets` não existe: cada caso tem o seu documento
//!   ([`FixtureDocument`]), que nasce sem folhas adotadas.
//! - **Os `import` de efeito.** `currentLineHighlight.js` e `editor.css` carregam no documento a folha do editor e o
//!   participante de tema do realce da linha atual (`fixture_editor_styles`); o participante é registrado no começo de
//!   cada caso e descartado ao fim, porque o registro é por thread.
//! - **DOM e `getComputedStyle`.** `$('div')`, `$('.a.b')`, `appendChild`, `classList.add` e `style` são os
//!   [`FixtureElement`] do documento; `getComputedStyle(el)` é o [`ComputedStyle`] do resolvedor de `fixture_css`, que
//!   interpreta o CSS gerado (`getThemeStyleSheet`) com a cascata, o `@scope`, o `:where` e os `var()` que os casos
//!   exercitam. `style.backgroundColor` e `style.border` são `background_color()` e `border()`.
//! - **`ColorThemeData.createLoadedEmptyTheme(id, id)` e `setCustomColors`.** `create_loaded_empty_theme` e
//!   `set_custom_colors` recebem o objeto de cores como `serde_json::Map`.
//! - **`deepStrictEqual`.** Objetos viram tuplas e listas comparadas campo a campo, na ordem do literal do original.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite};
use je_workbench::platform::theme::theme_service::{ICssStyleCollector, register_theming_participant};
use je_workbench::services::themes::color_theme_data::ColorThemeData;
use je_workbench::test::browser::component_fixtures::fixture_css::{FixtureDocument, FixtureElement};
use je_workbench::test::browser::component_fixtures::fixture_editor_styles::{editor_style_sheet, register_current_line_highlight_theming_participant};
use je_workbench::test::browser::component_fixtures::fixture_utils_css::get_theme_style_sheet;
use serde_json::{Map, Value, json};
use std::rc::Rc;

// `ThemeTypeSelector`.
const VS: &str = "vs";
const VS_DARK: &str = "vs-dark";
const HC_BLACK: &str = "hc-black";
const HC_LIGHT: &str = "hc-light";

/// `{ border, background }` do `getComputedStyle`.
type LineStyle = (String, String);

/// `ThemeVariant`.
struct ThemeVariant {
    id: String,
    colors: Map<String, Value>,
    expected_border: &'static str,
    expected_background: &'static str,
}

fn object(value: Value) -> Map<String, Value> {
    value.as_object().expect("as cores são um objeto").clone()
}

fn variant(id: String, colors: Value, expected_border: &'static str, expected_background: &'static str) -> ThemeVariant {
    ThemeVariant { id, colors: object(colors), expected_border, expected_background }
}

/// `variants`.
fn variants() -> Vec<ThemeVariant> {
    vec![
        variant(format!("{VS_DARK} fixture-dark-modern"), json!({ "editor.lineHighlightBackground": "#242526" }), "0px none rgb(0, 0, 0)", "rgb(36, 37, 38)"),
        variant(
            format!("{VS} fixture-light-modern"),
            json!({ "editor.lineHighlightBackground": "#00000000", "editor.lineHighlightBorder": "#eeeeee" }),
            "2px solid rgb(238, 238, 238)",
            "rgba(0, 0, 0, 0)",
        ),
        variant(format!("{VS} fixture-light-2026"), json!({ "editor.lineHighlightBackground": "#fafafa" }), "0px none rgb(0, 0, 0)", "rgb(250, 250, 250)"),
        variant(
            HC_BLACK.to_owned(),
            json!({ "editor.lineHighlightBackground": "#00000000", "editor.lineHighlightBorder": "#ffffff" }),
            "1px solid rgb(255, 255, 255)",
            "rgba(0, 0, 0, 0)",
        ),
        variant(
            HC_LIGHT.to_owned(),
            json!({ "editor.lineHighlightBackground": "#00000000", "editor.lineHighlightBorder": "#0f4a85" }),
            "1px solid rgb(15, 74, 133)",
            "rgba(0, 0, 0, 0)",
        ),
        variant(format!("{VS_DARK} fixture-solarized-dark"), json!({ "editor.lineHighlightBackground": "#002b36" }), "0px none rgb(0, 0, 0)", "rgb(0, 43, 54)"),
        variant(format!("{VS} fixture-solarized-light"), json!({ "editor.lineHighlightBackground": "#fdf6e3" }), "0px none rgb(0, 0, 0)", "rgb(253, 246, 227)"),
    ]
}

fn ordered_variants(reverse: bool) -> Vec<ThemeVariant> {
    let mut ordered = variants();
    if reverse {
        ordered.reverse();
    }
    ordered
}

/// O documento do fixture: a folha do editor (`editor.css`) e o participante do realce da linha atual (`import`).
fn create_document(disposables: &LeakGuard) -> Rc<FixtureDocument> {
    disposables.add(register_current_line_highlight_theming_participant());
    let document = FixtureDocument::new();
    document.add_document_style_sheet(editor_style_sheet());
    document
}

/// `ColorThemeData.createLoadedEmptyTheme(id, id)` com `setCustomColors(colors)`.
fn create_theme(id: &str, colors: &Map<String, Value>) -> ColorThemeData {
    let mut theme = ColorThemeData::create_loaded_empty_theme(id, id);
    theme.set_custom_colors(colors);
    theme
}

/// `root = host.appendChild($('.monaco-workbench')); root.classList.add(...theme.classNames)` depois de instalar a folha
/// do tema.
fn append_workbench_root(document: &FixtureDocument, host: &FixtureElement, theme: &ColorThemeData) -> FixtureElement {
    document.adopt_style_sheet(get_theme_style_sheet(theme));
    let root = host.append_child("monaco-workbench");
    root.add_classes(&theme.class_names());
    root
}

/// `parent.appendChild($('.monaco-editor'))` com a primeira classe do tema (`editor.classList.add(theme.classNames[0])`).
fn append_nested_editor(root: &FixtureElement, theme: &ColorThemeData) -> FixtureElement {
    let editor = root.append_child("monaco-editor");
    editor.add_classes(&[theme.class_names()[0]]);
    editor
}

#[test]
fn does_not_apply_workbench_backgrounds_to_nested_editor_theme_classes() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let document = create_document(&disposables);
    disposables.add(register_theming_participant(|_theme: &ColorThemeData, collector: &mut dyn ICssStyleCollector| {
        collector.add_rule(".monaco-workbench { background-color: #654321; }");
    }));
    let host = document.body().append_child("");

    let actual: Vec<[String; 3]> = variants()
        .iter()
        .map(|variant| {
            let id = format!("{} workbench-background", variant.id);
            let theme = create_theme(&id, &object(json!({ "editor.background": "#123456" })));
            let root = append_workbench_root(&document, &host, &theme);
            let editor = append_nested_editor(&root, &theme);
            let scrollable = editor.append_child("monaco-scrollable-element");
            scrollable.add_classes(&[theme.class_names()[0]]);

            [root, editor, scrollable].map(|element| element.computed_style().background_color())
        })
        .collect();

    let expected: Vec<[String; 3]> = variants().iter().map(|_| ["rgb(101, 67, 33)".to_owned(), "rgb(18, 52, 86)".to_owned(), "rgba(0, 0, 0, 0)".to_owned()]).collect();
    assert_eq!(actual, expected);
}

/// `createCurrentLine(parent)`.
fn create_current_line(parent: &FixtureElement) -> FixtureElement {
    let editor = parent.append_child("monaco-editor focused");
    let overlays = editor.append_child("view-overlays");
    let line = overlays.append_child("current-line current-line-exact");
    line.set_style_property("color", "black");
    line
}

/// `getCurrentLineStyle(line)`.
fn get_current_line_style(line: &FixtureElement) -> LineStyle {
    let style = line.computed_style();
    (style.border(), style.background_color())
}

/// `resolveStyle(parent, expected)`: o estilo que o navegador normaliza para o `border` e o fundo esperados.
fn resolve_style(parent: &FixtureElement, border: &str, background: &str) -> LineStyle {
    let reference = parent.append_child("");
    reference.set_style_property("border", border);
    reference.set_style_property("background-color", background);
    get_current_line_style(&reference)
}

fn isolates_active_line_styles(reverse: bool) {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let document = create_document(&disposables);
    let host = document.body().append_child("");
    let outside_line = create_current_line(&host);
    let mut lines: Vec<(String, FixtureElement)> = Vec::new();
    let mut actual: Vec<(LineStyle, Vec<(String, LineStyle)>)> = Vec::new();
    let ordered_variants = ordered_variants(reverse);
    let expected_styles: Vec<(String, LineStyle)> =
        ordered_variants.iter().map(|variant| (variant.id.clone(), resolve_style(&host, variant.expected_border, variant.expected_background))).collect();

    for variant in &ordered_variants {
        let theme = create_theme(&variant.id, &variant.colors);
        let root = append_workbench_root(&document, &host, &theme);
        lines.push((variant.id.clone(), create_current_line(&root)));
        actual.push((get_current_line_style(&outside_line), lines.iter().map(|(id, line)| (id.clone(), get_current_line_style(line))).collect()));
    }

    let outside: LineStyle = ("0px none rgb(0, 0, 0)".to_owned(), "rgba(0, 0, 0, 0)".to_owned());
    let expected: Vec<(LineStyle, Vec<(String, LineStyle)>)> = (0..ordered_variants.len()).map(|index| (outside.clone(), expected_styles[..=index].to_vec())).collect();
    assert_eq!(actual, expected);
}

fn isolates_color_variables_on_nested_editor_theme_classes(reverse: bool) {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let document = create_document(&disposables);
    let host = document.body().append_child("");
    let ordered_variants = ordered_variants(reverse);
    let mut editors: Vec<FixtureElement> = Vec::new();
    let mut actual: Vec<Vec<String>> = Vec::new();

    for variant in &ordered_variants {
        let theme = create_theme(&variant.id, &variant.colors);
        let root = append_workbench_root(&document, &host, &theme);
        let editor = append_nested_editor(&root, &theme);
        editor.set_style_property("background-color", "var(--vscode-editor-lineHighlightBackground)");
        editors.push(editor);
        actual.push(editors.iter().map(|element| element.computed_style().background_color()).collect());
    }

    let expected: Vec<Vec<String>> =
        (0..ordered_variants.len()).map(|index| ordered_variants[..=index].iter().map(|variant| variant.expected_background.to_owned()).collect()).collect();
    assert_eq!(actual, expected);
}

#[test]
fn isolates_active_line_styles_in_forward_theme_installation_order() {
    isolates_active_line_styles(false);
}

#[test]
fn isolates_color_variables_on_nested_editor_theme_classes_in_forward_theme_installation_order() {
    isolates_color_variables_on_nested_editor_theme_classes(false);
}

#[test]
fn isolates_active_line_styles_in_reverse_theme_installation_order() {
    isolates_active_line_styles(true);
}

#[test]
fn isolates_color_variables_on_nested_editor_theme_classes_in_reverse_theme_installation_order() {
    isolates_color_variables_on_nested_editor_theme_classes(true);
}

#[test]
fn preserves_theme_variables_on_fixture_and_nested_editor_roots() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let document = create_document(&disposables);
    let theme = create_theme(&format!("{VS} fixture-light"), &object(json!({ "editor.background": "#123456" })));
    document.adopt_style_sheet(get_theme_style_sheet(&theme));
    let root = document.body().append_child("monaco-workbench");
    root.add_classes(&theme.class_names());
    root.set_style_property("background-color", "var(--vscode-editor-background)");
    let root_background = root.computed_style().background_color();
    root.set_style_property("--vscode-editor-background", "#abcdef");
    let editor = root.append_child("monaco-editor vs");
    editor.set_style_property("background-color", "var(--vscode-editor-background)");

    // rootBackground, editorBackground
    assert_eq!((root_background, editor.computed_style().background_color()), ("rgb(18, 52, 86)".to_owned(), "rgb(18, 52, 86)".to_owned()));
}
