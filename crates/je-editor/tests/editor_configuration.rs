//! Porte de `src/vs/editor/test/browser/config/editorConfiguration.test.ts` (suítes `Common Editor Config` e
//! `migrateOptions`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o `LeakGuard` criado no começo de cada caso.
//! - O `setup`/`teardown` do nível de zoom (estado global do `EditorZoom`) da suíte `Common Editor Config` vira o
//!   `ZoomGuard`: guarda o nível ao nascer e o restaura no `Drop`, mesmo se o caso entrar em pânico.
//! - `TestWrappingConfiguration extends TestConfiguration` (sobrescreve `_readEnvConfiguration`) é o
//!   `TestWrappingEnvironment`, o `EditorEnvironment` que devolve o ambiente fixo do teste e delega o resto ao
//!   `TestEnvironment`; `TestWrappingConfiguration::new` é `TestConfiguration::with_environment`.
//! - `Object.defineProperty(hoverOptions, 'enabled', { writable: false, value: 'on' })` vira um objeto congelado
//!   com `enabled: 'on'` (o `Dynamic` não tem descritor por propriedade): escrever nele lança `TypeError`, que é o
//!   que o caso garante que a configuração não faz (ela trabalha numa cópia profunda).
//! - `assert.deepStrictEqual` de objetos de opções cruas é a igualdade estrutural `objects::equals` (mesmas chaves,
//!   inclusive as de valor `undefined`, e valores iguais).
//! - Os valores `<any>false` e `null!` são `Dynamic::Bool(false)` e `Dynamic::Null`.
//! - `let event = null; config.onDidChange(e => event = e)` é um `Rc<RefCell<Option<..>>>`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::cell::RefCell;
use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::dynamic::Dynamic;
use je_base::lifecycle::IDisposable;
use je_base::objects;
use je_editor::config::editor_configuration::IEditorConfiguration;
use je_editor::config::editor_configuration_impl::{EditorConfiguration, EditorEnvironment, EnvConfiguration};
use je_editor::config::editor_options::hover::{HOVER, HoverEnabled};
use je_editor::config::editor_options::quick_suggestions::{
    InternalQuickSuggestionsOptions, QUICK_SUGGESTIONS, QuickSuggestionsValue,
};
use je_editor::config::editor_options::simple_keys::GLYPH_MARGIN;
use je_editor::config::editor_options::unicode_highlight::{
    BoolOrInUntrustedWorkspace, InternalUnicodeHighlightOptions, TrueRecord, UNICODE_HIGHLIGHTING,
};
use je_editor::config::editor_options::wrapping_info::WRAPPING_INFO;
use je_editor::config::editor_options::{AccessibilitySupport, ConfigurationChangedEvent};
use je_editor::config::editor_zoom;
use je_editor::config::element_size_observer::ElementSizeObserver;
use je_editor::config::font_info::{BareFontInfo, FontInfo};
use je_editor::config::migrate_options::migrate_options;
use je_editor::cursor_common::EditorConfiguration as ConfigurationOptions;
use support::test_configuration::{TestConfiguration, TestEnvironment};

fn obj<const N: usize>(entries: [(&str, Dynamic); N]) -> Dynamic {
    Dynamic::object(entries)
}

fn text(value: &str) -> Dynamic {
    Dynamic::string(value)
}

fn flag(value: bool) -> Dynamic {
    Dynamic::Bool(value)
}

fn number(value: f64) -> Dynamic {
    Dynamic::Number(value)
}

/// `assert.deepStrictEqual(actual, expected)` para valores `Dynamic`.
fn assert_deep_strict_equal(actual: &Dynamic, expected: &Dynamic) {
    assert!(
        objects::equals(actual, expected),
        "esperava {}, veio {}",
        objects::safe_stringify(expected).unwrap_or_default(),
        objects::safe_stringify(actual).unwrap_or_default(),
    );
}

/// `setup(() => zoomLevel = EditorZoom.getZoomLevel())` e `teardown(() => EditorZoom.setZoomLevel(zoomLevel))`.
struct ZoomGuard {
    zoom_level: f64,
}

impl ZoomGuard {
    fn new() -> Self {
        Self { zoom_level: editor_zoom::get_zoom_level() }
    }
}

impl Drop for ZoomGuard {
    fn drop(&mut self) {
        editor_zoom::set_zoom_level(self.zoom_level);
    }
}

/// `TestWrappingConfiguration._readEnvConfiguration`: o ambiente com 1000 por 100 de área externa.
struct TestWrappingEnvironment {
    base: TestEnvironment,
}

impl EditorEnvironment for TestWrappingEnvironment {
    fn read_env_configuration(&self, _raw_options: &Dynamic, _container_observer: &ElementSizeObserver) -> EnvConfiguration {
        EnvConfiguration {
            extra_editor_class_name: String::new(),
            outer_width: 1000.0,
            outer_height: 100.0,
            empty_selection_clipboard: true,
            pixel_ratio: 1.0,
            accessibility_support: AccessibilitySupport::Unknown,
            edit_context_supported: true,
        }
    }

    fn read_font_info(&self, bare_font_info: &BareFontInfo) -> FontInfo {
        self.base.read_font_info(bare_font_info)
    }

    fn on_did_change_font_measurements(&self) -> je_base::event::Event<()> {
        self.base.on_did_change_font_measurements()
    }

    fn on_did_change_pixel_ratio(&self) -> je_base::event::Event<()> {
        self.base.on_did_change_pixel_ratio()
    }

    fn on_did_change_screen_reader_optimized(&self) -> je_base::event::Event<()> {
        self.base.on_did_change_screen_reader_optimized()
    }
}

/// `new TestWrappingConfiguration(opts)`.
fn new_wrapping_configuration(opts: &Dynamic) -> Rc<EditorConfiguration> {
    TestConfiguration::with_environment(opts, Rc::new(TestWrappingEnvironment { base: TestEnvironment }))
}

fn assert_wrapping(config: &EditorConfiguration, is_viewport_wrapping: bool, wrapping_column: f64) {
    let options = config.options();
    let wrapping_info = options.get(WRAPPING_INFO);
    assert_eq!(wrapping_info.is_viewport_wrapping, is_viewport_wrapping);
    assert_eq!(wrapping_info.wrapping_column, wrapping_column);
}

// ---------------------------------------------------------------------------------------------------------------
// Common Editor Config
// ---------------------------------------------------------------------------------------------------------------

#[test]
fn zoom_level() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let _zoom = ZoomGuard::new();

    // Zoom levels are defined to go between -5, 20 inclusive
    editor_zoom::set_zoom_level(0.0);
    assert_eq!(editor_zoom::get_zoom_level(), 0.0);

    editor_zoom::set_zoom_level(-0.0);
    assert_eq!(editor_zoom::get_zoom_level(), 0.0);

    editor_zoom::set_zoom_level(5.0);
    assert_eq!(editor_zoom::get_zoom_level(), 5.0);

    editor_zoom::set_zoom_level(-1.0);
    assert_eq!(editor_zoom::get_zoom_level(), -1.0);

    editor_zoom::set_zoom_level(9.0);
    assert_eq!(editor_zoom::get_zoom_level(), 9.0);

    editor_zoom::set_zoom_level(-9.0);
    assert_eq!(editor_zoom::get_zoom_level(), -5.0);

    editor_zoom::set_zoom_level(20.0);
    assert_eq!(editor_zoom::get_zoom_level(), 20.0);

    editor_zoom::set_zoom_level(-10.0);
    assert_eq!(editor_zoom::get_zoom_level(), -5.0);

    editor_zoom::set_zoom_level(9.1);
    assert_eq!(editor_zoom::get_zoom_level(), 9.1);

    editor_zoom::set_zoom_level(-9.1);
    assert_eq!(editor_zoom::get_zoom_level(), -5.0);

    editor_zoom::set_zoom_level(f64::INFINITY);
    assert_eq!(editor_zoom::get_zoom_level(), 20.0);

    editor_zoom::set_zoom_level(f64::NEG_INFINITY);
    assert_eq!(editor_zoom::get_zoom_level(), -5.0);
}

#[test]
fn word_wrap_default() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let _zoom = ZoomGuard::new();
    let config = new_wrapping_configuration(&obj([]));
    assert_wrapping(&config, false, -1.0);
    config.dispose();
}

#[test]
fn word_wrap_compat_false() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let _zoom = ZoomGuard::new();
    let config = new_wrapping_configuration(&obj([("wordWrap", flag(false))]));
    assert_wrapping(&config, false, -1.0);
    config.dispose();
}

#[test]
fn word_wrap_compat_true() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let _zoom = ZoomGuard::new();
    let config = new_wrapping_configuration(&obj([("wordWrap", flag(true))]));
    assert_wrapping(&config, true, 80.0);
    config.dispose();
}

#[test]
fn word_wrap_on() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let _zoom = ZoomGuard::new();
    let config = new_wrapping_configuration(&obj([("wordWrap", text("on"))]));
    assert_wrapping(&config, true, 80.0);
    config.dispose();
}

#[test]
fn word_wrap_on_without_minimap() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let _zoom = ZoomGuard::new();
    let config = new_wrapping_configuration(&obj([
        ("wordWrap", text("on")),
        ("minimap", obj([("enabled", flag(false))])),
    ]));
    assert_wrapping(&config, true, 88.0);
    config.dispose();
}

#[test]
fn word_wrap_on_does_not_use_word_wrap_column() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let _zoom = ZoomGuard::new();
    let config = new_wrapping_configuration(&obj([("wordWrap", text("on")), ("wordWrapColumn", number(10.0))]));
    assert_wrapping(&config, true, 80.0);
    config.dispose();
}

#[test]
fn word_wrap_off() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let _zoom = ZoomGuard::new();
    let config = new_wrapping_configuration(&obj([("wordWrap", text("off"))]));
    assert_wrapping(&config, false, -1.0);
    config.dispose();
}

#[test]
fn word_wrap_off_does_not_use_word_wrap_column() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let _zoom = ZoomGuard::new();
    let config = new_wrapping_configuration(&obj([("wordWrap", text("off")), ("wordWrapColumn", number(10.0))]));
    assert_wrapping(&config, false, -1.0);
    config.dispose();
}

#[test]
fn word_wrap_word_wrap_column_uses_default_word_wrap_column() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let _zoom = ZoomGuard::new();
    let config = new_wrapping_configuration(&obj([("wordWrap", text("wordWrapColumn"))]));
    assert_wrapping(&config, false, 80.0);
    config.dispose();
}

#[test]
fn word_wrap_word_wrap_column_uses_word_wrap_column() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let _zoom = ZoomGuard::new();
    let config = new_wrapping_configuration(&obj([
        ("wordWrap", text("wordWrapColumn")),
        ("wordWrapColumn", number(100.0)),
    ]));
    assert_wrapping(&config, false, 100.0);
    config.dispose();
}

#[test]
fn word_wrap_word_wrap_column_validates_word_wrap_column() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let _zoom = ZoomGuard::new();
    let config = new_wrapping_configuration(&obj([
        ("wordWrap", text("wordWrapColumn")),
        ("wordWrapColumn", number(-1.0)),
    ]));
    assert_wrapping(&config, false, 1.0);
    config.dispose();
}

#[test]
fn word_wrap_bounded_uses_default_word_wrap_column() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let _zoom = ZoomGuard::new();
    let config = new_wrapping_configuration(&obj([("wordWrap", text("bounded"))]));
    assert_wrapping(&config, true, 80.0);
    config.dispose();
}

#[test]
fn word_wrap_bounded_uses_word_wrap_column() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let _zoom = ZoomGuard::new();
    let config = new_wrapping_configuration(&obj([("wordWrap", text("bounded")), ("wordWrapColumn", number(40.0))]));
    assert_wrapping(&config, true, 40.0);
    config.dispose();
}

#[test]
fn word_wrap_bounded_validates_word_wrap_column() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let _zoom = ZoomGuard::new();
    let config = new_wrapping_configuration(&obj([("wordWrap", text("bounded")), ("wordWrapColumn", number(-1.0))]));
    assert_wrapping(&config, true, 1.0);
    config.dispose();
}

#[test]
fn issue_53152_cannot_assign_to_read_only_property_enabled_of_object() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let _zoom = ZoomGuard::new();
    let hover_options = obj([("enabled", text("on"))]);
    if let Dynamic::Object(object) = &hover_options {
        object.freeze();
    }
    let config = TestConfiguration::new(&obj([("hover", hover_options)]));

    assert_eq!(config.options().get(HOVER).enabled, HoverEnabled::On);
    config.update_options(&obj([("hover", obj([("enabled", text("off"))]))]));
    assert_eq!(config.options().get(HOVER).enabled, HoverEnabled::Off);

    config.dispose();
}

#[test]
fn does_not_emit_event_when_nothing_changes() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let _zoom = ZoomGuard::new();
    let config = TestConfiguration::new(&obj([("glyphMargin", flag(true)), ("roundedSelection", flag(false))]));
    let event: Rc<RefCell<Option<ConfigurationChangedEvent>>> = Rc::new(RefCell::new(None));
    let disposable = config.on_did_change().listen({
        let event = event.clone();
        move |e| *event.borrow_mut() = Some(e.clone())
    });
    assert!(*config.options().get(GLYPH_MARGIN));

    config.update_options(&obj([("glyphMargin", flag(true))]));
    config.update_options(&obj([("roundedSelection", flag(false))]));
    assert!(event.borrow().is_none());
    config.dispose();
    disposable.dispose();
}

#[test]
fn issue_94931_unable_to_open_source_file() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let _zoom = ZoomGuard::new();
    let config = TestConfiguration::new(&obj([("quickSuggestions", Dynamic::Null)]));
    let actual = *config.options().get(QUICK_SUGGESTIONS);
    assert_eq!(
        actual,
        InternalQuickSuggestionsOptions {
            other: QuickSuggestionsValue::OffWhenInlineCompletions,
            comments: QuickSuggestionsValue::Off,
            strings: QuickSuggestionsValue::Off,
        }
    );
    config.dispose();
}

#[test]
fn issue_102920_cant_snap_or_split_view_with_json_files() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let _zoom = ZoomGuard::new();
    let config = TestConfiguration::new(&obj([("quickSuggestions", Dynamic::Null)]));
    config.update_options(&obj([("quickSuggestions", obj([("strings", flag(true))]))]));
    let actual = *config.options().get(QUICK_SUGGESTIONS);
    assert_eq!(
        actual,
        InternalQuickSuggestionsOptions {
            other: QuickSuggestionsValue::OffWhenInlineCompletions,
            comments: QuickSuggestionsValue::Off,
            strings: QuickSuggestionsValue::On,
        }
    );
    config.dispose();
}

#[test]
fn issue_151926_untyped_editor_options_apply() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let _zoom = ZoomGuard::new();
    let config = TestConfiguration::new(&obj([]));
    config.update_options(&obj([(
        "unicodeHighlight",
        obj([("allowedCharacters", obj([("x", flag(true))]))]),
    )]));
    let actual = config.options().get(UNICODE_HIGHLIGHTING).clone();
    assert_eq!(
        actual,
        InternalUnicodeHighlightOptions {
            non_basic_ascii: BoolOrInUntrustedWorkspace::InUntrustedWorkspace,
            invisible_characters: true,
            ambiguous_characters: true,
            include_comments: BoolOrInUntrustedWorkspace::InUntrustedWorkspace,
            include_strings: BoolOrInUntrustedWorkspace::InUntrustedWorkspace,
            allowed_characters: TrueRecord::from_keys(["x"]),
            allowed_locales: TrueRecord::from_keys(["_os", "_vscode"]),
        }
    );
    config.dispose();
}

// ---------------------------------------------------------------------------------------------------------------
// migrateOptions
// ---------------------------------------------------------------------------------------------------------------

fn migrate(options: Dynamic) -> Dynamic {
    migrate_options(&options);
    options
}

#[test]
fn migrate_word_wrap() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    assert_deep_strict_equal(&migrate(obj([("wordWrap", flag(true))])), &obj([("wordWrap", text("on"))]));
    assert_deep_strict_equal(&migrate(obj([("wordWrap", flag(false))])), &obj([("wordWrap", text("off"))]));
}

#[test]
fn migrate_line_numbers() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    assert_deep_strict_equal(&migrate(obj([("lineNumbers", flag(true))])), &obj([("lineNumbers", text("on"))]));
    assert_deep_strict_equal(&migrate(obj([("lineNumbers", flag(false))])), &obj([("lineNumbers", text("off"))]));
}

#[test]
fn migrate_auto_closing_brackets() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    assert_deep_strict_equal(
        &migrate(obj([("autoClosingBrackets", flag(false))])),
        &obj([
            ("autoClosingBrackets", text("never")),
            ("autoClosingQuotes", text("never")),
            ("autoSurround", text("never")),
        ]),
    );
}

#[test]
fn migrate_cursor_blinking() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    assert_deep_strict_equal(
        &migrate(obj([("cursorBlinking", text("visible"))])),
        &obj([("cursorBlinking", text("solid"))]),
    );
}

#[test]
fn migrate_render_whitespace() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    assert_deep_strict_equal(
        &migrate(obj([("renderWhitespace", flag(true))])),
        &obj([("renderWhitespace", text("boundary"))]),
    );
    assert_deep_strict_equal(
        &migrate(obj([("renderWhitespace", flag(false))])),
        &obj([("renderWhitespace", text("none"))]),
    );
}

#[test]
fn migrate_render_line_highlight() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    assert_deep_strict_equal(
        &migrate(obj([("renderLineHighlight", flag(true))])),
        &obj([("renderLineHighlight", text("line"))]),
    );
    assert_deep_strict_equal(
        &migrate(obj([("renderLineHighlight", flag(false))])),
        &obj([("renderLineHighlight", text("none"))]),
    );
}

#[test]
fn migrate_accept_suggestion_on_enter() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    assert_deep_strict_equal(
        &migrate(obj([("acceptSuggestionOnEnter", flag(true))])),
        &obj([("acceptSuggestionOnEnter", text("on"))]),
    );
    assert_deep_strict_equal(
        &migrate(obj([("acceptSuggestionOnEnter", flag(false))])),
        &obj([("acceptSuggestionOnEnter", text("off"))]),
    );
}

#[test]
fn migrate_tab_completion() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    assert_deep_strict_equal(
        &migrate(obj([("tabCompletion", flag(true))])),
        &obj([("tabCompletion", text("onlySnippets"))]),
    );
    assert_deep_strict_equal(
        &migrate(obj([("tabCompletion", flag(false))])),
        &obj([("tabCompletion", text("off"))]),
    );
}

#[test]
fn migrate_suggest_filtered_types() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let filtered_types = obj([
        ("method", flag(false)),
        ("function", flag(false)),
        ("constructor", flag(false)),
        ("deprecated", flag(false)),
        ("field", flag(false)),
        ("variable", flag(false)),
        ("class", flag(false)),
        ("struct", flag(false)),
        ("interface", flag(false)),
        ("module", flag(false)),
        ("property", flag(false)),
        ("event", flag(false)),
        ("operator", flag(false)),
        ("unit", flag(false)),
        ("value", flag(false)),
        ("constant", flag(false)),
        ("enum", flag(false)),
        ("enumMember", flag(false)),
        ("keyword", flag(false)),
        ("text", flag(false)),
        ("color", flag(false)),
        ("file", flag(false)),
        ("reference", flag(false)),
        ("folder", flag(false)),
        ("typeParameter", flag(false)),
        ("snippet", flag(false)),
    ]);
    assert_deep_strict_equal(
        &migrate(obj([("suggest", obj([("filteredTypes", filtered_types)]))])),
        &obj([(
            "suggest",
            obj([
                ("filteredTypes", Dynamic::Undefined),
                ("showMethods", flag(false)),
                ("showFunctions", flag(false)),
                ("showConstructors", flag(false)),
                ("showDeprecated", flag(false)),
                ("showFields", flag(false)),
                ("showVariables", flag(false)),
                ("showClasses", flag(false)),
                ("showStructs", flag(false)),
                ("showInterfaces", flag(false)),
                ("showModules", flag(false)),
                ("showProperties", flag(false)),
                ("showEvents", flag(false)),
                ("showOperators", flag(false)),
                ("showUnits", flag(false)),
                ("showValues", flag(false)),
                ("showConstants", flag(false)),
                ("showEnums", flag(false)),
                ("showEnumMembers", flag(false)),
                ("showKeywords", flag(false)),
                ("showWords", flag(false)),
                ("showColors", flag(false)),
                ("showFiles", flag(false)),
                ("showReferences", flag(false)),
                ("showFolders", flag(false)),
                ("showTypeParameters", flag(false)),
                ("showSnippets", flag(false)),
            ]),
        )]),
    );
}

#[test]
fn migrate_quick_suggestions() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    assert_deep_strict_equal(
        &migrate(obj([("quickSuggestions", flag(true))])),
        &obj([("quickSuggestions", obj([("comments", text("on")), ("strings", text("on")), ("other", text("on"))]))]),
    );
    assert_deep_strict_equal(
        &migrate(obj([("quickSuggestions", flag(false))])),
        &obj([("quickSuggestions", obj([("comments", text("off")), ("strings", text("off")), ("other", text("off"))]))]),
    );
    assert_deep_strict_equal(
        &migrate(obj([("quickSuggestions", obj([("comments", text("on")), ("strings", text("off"))]))])),
        &obj([("quickSuggestions", obj([("comments", text("on")), ("strings", text("off"))]))]),
    );
}

#[test]
fn migrate_hover() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    assert_deep_strict_equal(
        &migrate(obj([("hover", flag(true))])),
        &obj([("hover", obj([("enabled", text("on"))]))]),
    );
    assert_deep_strict_equal(
        &migrate(obj([("hover", flag(false))])),
        &obj([("hover", obj([("enabled", text("off"))]))]),
    );
}

#[test]
fn migrate_parameter_hints() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    assert_deep_strict_equal(
        &migrate(obj([("parameterHints", flag(true))])),
        &obj([("parameterHints", obj([("enabled", flag(true))]))]),
    );
    assert_deep_strict_equal(
        &migrate(obj([("parameterHints", flag(false))])),
        &obj([("parameterHints", obj([("enabled", flag(false))]))]),
    );
}

#[test]
fn migrate_auto_indent() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    assert_deep_strict_equal(&migrate(obj([("autoIndent", flag(true))])), &obj([("autoIndent", text("full"))]));
    assert_deep_strict_equal(&migrate(obj([("autoIndent", flag(false))])), &obj([("autoIndent", text("advanced"))]));
}

#[test]
fn migrate_match_brackets() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    assert_deep_strict_equal(
        &migrate(obj([("matchBrackets", flag(true))])),
        &obj([("matchBrackets", text("always"))]),
    );
    assert_deep_strict_equal(
        &migrate(obj([("matchBrackets", flag(false))])),
        &obj([("matchBrackets", text("never"))]),
    );
}

#[test]
fn migrate_render_indent_guides_highlight_active_indent_guide() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    assert_deep_strict_equal(
        &migrate(obj([("renderIndentGuides", flag(true))])),
        &obj([("renderIndentGuides", Dynamic::Undefined), ("guides", obj([("indentation", flag(true))]))]),
    );
    assert_deep_strict_equal(
        &migrate(obj([("renderIndentGuides", flag(false))])),
        &obj([("renderIndentGuides", Dynamic::Undefined), ("guides", obj([("indentation", flag(false))]))]),
    );
    assert_deep_strict_equal(
        &migrate(obj([("highlightActiveIndentGuide", flag(true))])),
        &obj([
            ("highlightActiveIndentGuide", Dynamic::Undefined),
            ("guides", obj([("highlightActiveIndentation", flag(true))])),
        ]),
    );
    assert_deep_strict_equal(
        &migrate(obj([("highlightActiveIndentGuide", flag(false))])),
        &obj([
            ("highlightActiveIndentGuide", Dynamic::Undefined),
            ("guides", obj([("highlightActiveIndentation", flag(false))])),
        ]),
    );
}

#[test]
fn migration_does_not_overwrite_new_setting() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    assert_deep_strict_equal(
        &migrate(obj([("renderIndentGuides", flag(true)), ("guides", obj([("indentation", flag(false))]))])),
        &obj([("renderIndentGuides", Dynamic::Undefined), ("guides", obj([("indentation", flag(false))]))]),
    );
    assert_deep_strict_equal(
        &migrate(obj([
            ("highlightActiveIndentGuide", flag(true)),
            ("guides", obj([("highlightActiveIndentation", flag(false))])),
        ])),
        &obj([
            ("highlightActiveIndentGuide", Dynamic::Undefined),
            ("guides", obj([("highlightActiveIndentation", flag(false))])),
        ]),
    );
}
