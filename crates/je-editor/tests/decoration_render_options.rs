//! Porte de `src/vs/editor/test/browser/services/decorationRenderOptions.test.ts` (suíte `Decoration Render
//! Options`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o `LeakGuard` criado no começo de cada caso (`store` é o
//!   próprio guarda).
//! - `new TestCodeEditorService(themeService)` é o `DecorationRenderService` (a parte de decorações do serviço),
//!   com a identidade no lugar de `FileAccess.uriToBrowserUri` (o contexto do teste não é nativo).
//!   `TestGlobalStyleSheet.read()` é `read_style_sheet`: as regras como `selector {cssText}`, a mais nova na
//!   frente, juntas por `\n`.
//! - `CSS.escape` é `css_escape`; `assert.throws(() => ...)` é `errors::catch` e `is_err()`.
//! - `{ id: 'editorBackground' }` (um `ThemeColor`) é `DecorationColor::Theme`, e uma cor em texto, `DecorationColor::Color`.
//! - `platform.isWindows` é a plataforma do processo (`ProcessEnvironment`), e `URI.file(path)` leva essa plataforma.
//! - `indexOf(...) > 0` dos casos de `gutterIconPath` é `find(...)` com índice maior que zero.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::panic::AssertUnwindSafe;
use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::errors;
use je_base::path::{Environment, Platform, ProcessEnvironment};
use je_base::uri::URI;
use je_editor::model::DecorationColor;
use je_editor::services::css_value::css_escape;
use je_editor::services::decoration_render_options::{
    ContentDecorationRenderOptions, DecorationRenderOptions, DecorationRenderService, GlobalStyleSheet, ThemeDecorationRenderOptions,
};
use je_editor::services::theme_service::IThemeService;
use support::test_color_theme_service::{TestColorTheme, TestThemeService};

fn color(value: &str) -> Option<DecorationColor> {
    Some(DecorationColor::Color(value.to_string()))
}

fn theme_color(id: &str) -> Option<DecorationColor> {
    Some(DecorationColor::Theme(id.to_string()))
}

/// `new TestCodeEditorService(themeService)`.
fn create_code_editor_service(theme_service: Rc<dyn IThemeService>) -> Rc<DecorationRenderService> {
    DecorationRenderService::new(theme_service, Rc::new(|uri: &URI| uri.clone()))
}

/// `styleSheet.read()`.
fn read_style_sheet(style_sheet: &GlobalStyleSheet) -> String {
    style_sheet.css_rules().iter().map(|rule| rule.to_css_text()).collect::<Vec<_>>().join("\n")
}

fn options() -> DecorationRenderOptions {
    DecorationRenderOptions {
        base: ThemeDecorationRenderOptions {
            gutter_icon_path: Some(URI::parse("https://github.com/microsoft/vscode/blob/main/resources/linux/code.png", false)),
            gutter_icon_size: Some("contain".to_string()),
            background_color: color("red"),
            border_color: color("yellow"),
            ..ThemeDecorationRenderOptions::default()
        },
        ..DecorationRenderOptions::default()
    }
}

#[test]
fn register_and_resolve_decoration_type() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let theme_service_mock = TestThemeService::new_default();
    let s = store.add_rc(create_code_editor_service(theme_service_mock));
    store.add_rc(s.register_decoration_type("test", "example", options(), None));
    assert!(errors::catch(AssertUnwindSafe(|| s.resolve_decoration_options("example", false))).is_ok());
}

#[test]
fn remove_decoration_type() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let theme_service_mock = TestThemeService::new_default();
    let s = store.add_rc(create_code_editor_service(theme_service_mock));
    s.register_decoration_type("test", "example", options(), None);
    assert!(errors::catch(AssertUnwindSafe(|| s.resolve_decoration_options("example", false))).is_ok());
    s.remove_decoration_type("example");
    assert!(errors::catch(AssertUnwindSafe(|| s.resolve_decoration_options("example", false))).is_err());
}

#[test]
fn css_properties() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let theme_service_mock = TestThemeService::new_default();
    let s = store.add_rc(create_code_editor_service(theme_service_mock));
    let style_sheet = s.global_style_sheet().clone();
    store.add_rc(s.register_decoration_type("test", "example", options(), None));
    let sheet = read_style_sheet(&style_sheet);
    assert!(sheet.contains(&format!(
        "{{background:url('{}') center center no-repeat;background-size:contain;}}",
        css_escape("https://github.com/microsoft/vscode/blob/main/resources/linux/code.png")
    )));
    assert!(sheet.contains("{background-color:red;border-color:yellow;box-sizing: border-box;}"));
}

#[test]
fn theme_color() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let options = DecorationRenderOptions {
        base: ThemeDecorationRenderOptions {
            background_color: theme_color("editorBackground"),
            border_color: theme_color("editorBorder"),
            ..ThemeDecorationRenderOptions::default()
        },
        ..DecorationRenderOptions::default()
    };

    let theme_service = TestThemeService::new(TestColorTheme::new(&[("editorBackground", "#FF0000")]));
    let s = store.add_rc(create_code_editor_service(theme_service.clone()));
    let style_sheet = s.global_style_sheet().clone();
    s.register_decoration_type("test", "example", options, None);
    assert_eq!(
        read_style_sheet(&style_sheet),
        ".monaco-editor .ced-example-0 {background-color:#ff0000;border-color:transparent;box-sizing: border-box;}"
    );

    theme_service.set_theme(TestColorTheme::new(&[("editorBackground", "#EE0000"), ("editorBorder", "#00FFFF")]));
    assert_eq!(
        read_style_sheet(&style_sheet),
        ".monaco-editor .ced-example-0 {background-color:#ee0000;border-color:#00ffff;box-sizing: border-box;}"
    );

    s.remove_decoration_type("example");
    assert_eq!(read_style_sheet(&style_sheet), "");
}

#[test]
fn theme_overrides() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let options = DecorationRenderOptions {
        base: ThemeDecorationRenderOptions { color: theme_color("editorBackground"), ..ThemeDecorationRenderOptions::default() },
        light: Some(ThemeDecorationRenderOptions { color: color("#FF00FF"), ..ThemeDecorationRenderOptions::default() }),
        dark: Some(ThemeDecorationRenderOptions {
            color: color("#000000"),
            after: Some(ContentDecorationRenderOptions { color: theme_color("infoForeground"), ..ContentDecorationRenderOptions::default() }),
            ..ThemeDecorationRenderOptions::default()
        }),
        ..DecorationRenderOptions::default()
    };

    let theme_service = TestThemeService::new(TestColorTheme::new(&[("editorBackground", "#FF0000"), ("infoForeground", "#444444")]));
    let s = store.add_rc(create_code_editor_service(theme_service));
    let style_sheet = s.global_style_sheet().clone();
    s.register_decoration_type("test", "example", options, None);
    let expected = [
        ".vs-dark.monaco-editor .ced-example-4::after, .hc-black.monaco-editor .ced-example-4::after {color:#444444 !important;}",
        ".vs-dark.monaco-editor .ced-example-1, .hc-black.monaco-editor .ced-example-1 {color:#000000 !important;}",
        ".vs.monaco-editor .ced-example-1, .hc-light.monaco-editor .ced-example-1 {color:#FF00FF !important;}",
        ".monaco-editor .ced-example-1 {color:#ff0000 !important;}",
    ]
    .join("\n");
    assert_eq!(read_style_sheet(&style_sheet), expected);

    s.remove_decoration_type("example");
    assert_eq!(read_style_sheet(&style_sheet), "");
}

#[test]
fn css_properties_gutter_icon_paths() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let theme_service_mock = TestThemeService::new_default();
    let s = store.add_rc(create_code_editor_service(theme_service_mock));
    let style_sheet = s.global_style_sheet().clone();

    let gutter_icon = |uri: URI| DecorationRenderOptions {
        base: ThemeDecorationRenderOptions { gutter_icon_path: Some(uri), ..ThemeDecorationRenderOptions::default() },
        ..DecorationRenderOptions::default()
    };
    let background_rule = |url: &str| format!("{{background:url('{url}') center center no-repeat;}}");
    let has_rule_after_start = |rule: &str| read_style_sheet(&style_sheet).find(rule).is_some_and(|index| index > 0);

    // URI, only minimal encoding
    s.register_decoration_type("test", "example", gutter_icon(URI::parse("data:image/svg+xml;base64,PHN2ZyB4b+", false)), None);
    assert!(has_rule_after_start(&background_rule(&css_escape("data:image/svg+xml;base64,PHN2ZyB4b+"))));
    s.remove_decoration_type("example");

    let assert_background = |url1: &str, url2: &str| {
        assert!(has_rule_after_start(&background_rule(url1)) || has_rule_after_start(&background_rule(url2)));
    };

    let platform = ProcessEnvironment.platform();
    if platform == Platform::Win32 {
        // windows file path (used as string)
        s.register_decoration_type("test", "example", gutter_icon(URI::file("c:\\files\\miles\\more.png", platform)), None);
        assert_background(&css_escape("file:///c:/files/miles/more.png"), &css_escape("vscode-file://vscode-app/c:/files/miles/more.png"));
        s.remove_decoration_type("example");

        // single quote must always be escaped/encoded
        s.register_decoration_type("test", "example", gutter_icon(URI::file("c:\\files\\foo\\b'ar.png", platform)), None);
        assert_background(&css_escape("file:///c:/files/foo/b'ar.png"), &css_escape("vscode-file://vscode-app/c:/files/foo/b'ar.png"));
        s.remove_decoration_type("example");
    } else {
        // unix file path (used as string)
        s.register_decoration_type("test", "example", gutter_icon(URI::file("/Users/foo/bar.png", platform)), None);
        assert_background(&css_escape("file:///Users/foo/bar.png"), &css_escape("vscode-file://vscode-app/Users/foo/bar.png"));
        s.remove_decoration_type("example");

        // single quote must always be escaped/encoded
        s.register_decoration_type("test", "example", gutter_icon(URI::file("/Users/foo/b'ar.png", platform)), None);
        assert_background(&css_escape("file:///Users/foo/b'ar.png"), &css_escape("vscode-file://vscode-app/Users/foo/b'ar.png"));
        s.remove_decoration_type("example");
    }

    s.register_decoration_type("test", "example", gutter_icon(URI::parse("http://test/pa'th", false)), None);
    assert!(has_rule_after_start(&background_rule(&css_escape("http://test/pa'th"))));
    s.remove_decoration_type("example");
}
