//! Porte de `src/vs/workbench/services/themes/test/node/tokenStyleResolving.test.ts` (suíte `Themes -
//! TokenStyleResolving`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `TokenStyle`, `ts(foreground, styleFlags)`: `ts` devolve o [`TokenStyle`] com `Color::from_hex` na cor; `styleFlags`
//!   é o [`StyleFlags`] (os quatro campos opcionais), e `undefinedStyle`/`unsetStyle` são as constantes de mesmo nome.
//!   `ts('#xxxxxx', undefined)` é `ts(Some("#xxxxxx"), None)`.
//! - `ColorThemeData.createUnloadedTheme('foo')` é `create_unloaded_theme("foo", &[])`; `themeData.location = ...` é o
//!   campo `location`; `await themeData.ensureLoaded(service)` é `ensure_loaded(&loader).await`, dentro de
//!   `run_with_real_timers` (o corpo `async` do caso).
//! - O `ExtensionResourceLoaderService` do original (com `FileService`, `DiskFileSystemProvider` e os mocks de
//!   `IRequestService`, `IStorageService`, `IEnvironmentService` e `IConfigurationService`, que o carregamento de um
//!   arquivo local nunca consulta) é o [`DiskExtensionResourceLoader`], que lê o arquivo do disco e devolve a `Promise`
//!   já resolvida (ou rejeitada com o erro de E/S). `FileAccess.asFileUri('vs/workbench/services/themes/test/node/
//!   color-theme.json')` é o arquivo no oráculo `ref/vscode` (fora do git), achado subindo a partir de
//!   `CARGO_MANIFEST_DIR`, como em `tests/encoding.rs`.
//! - `ITokenColorCustomizations` e `ISemanticTokenColorCustomizations` são objetos JSON (`serde_json::Map`), como as
//!   customizações de `setCustomColors`; `setCustomSemanticTokenColors` recebe `Some(&map)` (o parâmetro do original
//!   aceita `undefined`).
//! - `themeData.resolveScopes([['variable']])` é `resolve_scopes(&scopes(&[&["variable"]]), None)`; o `undefined` de
//!   `defaultTokenStyle` é `None`, e `assertTokenStyle` compara as duas pontas por `tokenStyleAsString`.
//! - `themeData.tokenColorMap` é `token_color_map()` (um `Vec<Option<String>>`, com o índice 0 como o buraco do array
//!   do original); `getTokenStyleMetadata(type, modifiers, language)` usa `use_default = true` e `definitions = None`,
//!   que são os padrões do original.
//! - `registry.registerTokenType(...)` devolve `Result` (o original lança `Error`), e o `try { ... } finally { ... }` é
//!   [`try_finally`], que roda a limpeza também quando o corpo falha a asserção e depois relança o pânico.
//! - O registro de classificação é o singleton do processo, como no original; os casos que o alteram (`super type` e
//!   `language - scope resolving`) só registram tipos e regras com nomes próprios e desfazem tudo ao fim.
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o [`LeakGuard`] de `common`; o `teardown` que descarta o
//!   `DiskFileSystemProvider` não tem equivalente (o carregador do teste não guarda recurso).

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::path::{Path, PathBuf};

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_real_timers};
use je_base::errors::Error;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_platform::resources::{fs_path, uri_file};
use je_theme::color::Color;
use je_theme::color::format::css::format_hex_a;
use je_theme::token_classification_registry::{ProbeScope, TokenStyle, TokenStyleDefaults, get_token_classification_registry};
use je_workbench::contrib::snippets::snippets_file::IExtensionResourceLoaderService;
use je_workbench::services::themes::color_theme_data::{ColorThemeData, ITokenStyle};
use serde_json::{Map, Value, json};

/// Os quatro estilos de `{ bold?, underline?, strikethrough?, italic? }`.
#[derive(Clone, Copy, Default)]
struct StyleFlags {
    bold: Option<bool>,
    underline: Option<bool>,
    strikethrough: Option<bool>,
    italic: Option<bool>,
}

const UNDEFINED_STYLE: StyleFlags = StyleFlags { bold: None, underline: None, strikethrough: None, italic: None };
const UNSET_STYLE: StyleFlags = StyleFlags { bold: Some(false), underline: Some(false), strikethrough: None, italic: Some(false) };

fn ts(foreground: Option<&str>, style_flags: Option<StyleFlags>) -> TokenStyle {
    let foreground_color = foreground.map(Color::from_hex);
    let flags = style_flags.unwrap_or_default();
    TokenStyle {
        foreground: foreground_color,
        bold: flags.bold,
        underline: flags.underline,
        strikethrough: flags.strikethrough,
        italic: flags.italic,
    }
}

fn token_style_as_string(ts: Option<&TokenStyle>) -> String {
    let Some(ts) = ts else { return "tokenstyle-undefined".to_owned() };
    let mut text = ts.foreground.as_ref().map_or_else(|| "no-foreground".to_owned(), |foreground| foreground.to_string());
    if let Some(bold) = ts.bold {
        text.push_str(if bold { "+B" } else { "-B" });
    }
    if let Some(underline) = ts.underline {
        text.push_str(if underline { "+U" } else { "-U" });
    }
    if let Some(italic) = ts.italic {
        text.push_str(if italic { "+I" } else { "-I" });
    }
    text
}

fn assert_token_style(actual: Option<TokenStyle>, expected: Option<TokenStyle>, message: &str) {
    assert_eq!(token_style_as_string(actual.as_ref()), token_style_as_string(expected.as_ref()), "{message}");
}

fn assert_token_style_meta_data(color_index: &[Option<String>], actual: Option<ITokenStyle>, expected: Option<&TokenStyle>, message: &str) {
    let (Some(actual), Some(expected)) = (actual, expected) else {
        assert!(actual.is_none() && expected.is_none(), "{message}");
        return;
    };
    assert_eq!(actual.bold, expected.bold, "bold {message}");
    assert_eq!(actual.italic, expected.italic, "italic {message}");
    assert_eq!(actual.underline, expected.underline, "underline {message}");

    let actual_foreground_index = actual.foreground;
    match (actual_foreground_index, &expected.foreground) {
        (Some(index), Some(expected_foreground)) if index != 0 => {
            assert_eq!(
                color_index.get(index as usize).cloned().flatten(),
                Some(format_hex_a(expected_foreground, true).to_uppercase()),
                "foreground {message}"
            );
        }
        // `expected.foreground || 0`: um objeto `Color` nunca é igual a um número.
        (index, foreground) => assert!(foreground.is_none() && index == Some(0), "foreground {message}"),
    }
}

fn assert_token_styles(theme_data: &ColorThemeData, expected: &[(&str, TokenStyle)], language: &str) {
    let color_index = theme_data.token_color_map();

    for (qualified_classifier, expected_token_style) in expected {
        let mut parts = qualified_classifier.split('.');
        let token_type = parts.next().unwrap_or("");
        let modifiers: Vec<String> = parts.map(str::to_owned).collect();

        let token_style_meta_data = theme_data.get_token_style_metadata(token_type, &modifiers, language, true, None);
        assert_token_style_meta_data(&color_index, token_style_meta_data, Some(expected_token_style), qualified_classifier);
    }
}

fn scopes(probes: &[&[&str]]) -> Vec<ProbeScope> {
    probes.iter().map(|probe| probe.iter().map(|scope| (*scope).to_owned()).collect()).collect()
}

fn object(value: Value) -> Map<String, Value> {
    value.as_object().expect("as customizações são um objeto").clone()
}

/// `try { body } finally { cleanup }`: a limpeza roda também se o corpo falhar.
fn try_finally(body: impl FnOnce(), cleanup: impl FnOnce()) {
    let outcome = catch_unwind(AssertUnwindSafe(body));
    cleanup();
    if let Err(panic) = outcome {
        resume_unwind(panic);
    }
}

/// O arquivo do oráculo `color-theme.json`, em `ref/vscode` (fora do git).
fn color_theme_file() -> PathBuf {
    let relative = "ref/vscode/src/vs/workbench/services/themes/test/node/color-theme.json";
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .map(|dir| dir.join(relative))
        .find(|file| file.is_file())
        .unwrap_or_else(|| panic!("o oráculo não foi achado ({relative}) acima de CARGO_MANIFEST_DIR"))
}

/// O `ExtensionResourceLoaderService` sobre o disco (ver as notas de tradução).
struct DiskExtensionResourceLoader;

impl IExtensionResourceLoaderService for DiskExtensionResourceLoader {
    fn read_extension_resource(&self, uri: &URI) -> Promise<String> {
        match std::fs::read_to_string(fs_path(uri)) {
            Ok(content) => Promise::resolved(content),
            Err(error) => Promise::rejected(Error::new(error.to_string())),
        }
    }
}

#[test]
fn color_defaults() {
    run_with_real_timers(async move {
        let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let loader = DiskExtensionResourceLoader;

        let mut theme_data = ColorThemeData::create_unloaded_theme("foo", &[]);
        theme_data.location = Some(uri_file(&color_theme_file().to_string_lossy()));
        theme_data.ensure_loaded(&loader).await.expect("o tema carrega");

        assert!(theme_data.is_loaded);

        assert_token_styles(
            &theme_data,
            &[
                ("comment", ts(Some("#000000"), Some(UNDEFINED_STYLE))),
                ("variable", ts(Some("#111111"), Some(UNSET_STYLE))),
                ("type", ts(Some("#333333"), Some(StyleFlags { bold: Some(false), underline: Some(true), italic: Some(false), ..UNDEFINED_STYLE }))),
                ("function", ts(Some("#333333"), Some(UNSET_STYLE))),
                ("string", ts(Some("#444444"), Some(UNDEFINED_STYLE))),
                ("number", ts(Some("#555555"), Some(UNDEFINED_STYLE))),
                ("keyword", ts(Some("#666666"), Some(UNDEFINED_STYLE))),
            ],
            "typescript",
        );
    });
}

#[test]
fn resolve_scopes() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let mut theme_data = ColorThemeData::create_loaded_empty_theme("test", "test");

    let custom_token_colors = object(json!({
        "textMateRules": [
            {
                "scope": "variable",
                "settings": {
                    "fontStyle": "",
                    "foreground": "#F8F8F2"
                }
            },
            {
                "scope": "keyword.operator",
                "settings": {
                    "fontStyle": "italic bold underline",
                    "foreground": "#F92672"
                }
            },
            {
                "scope": "storage",
                "settings": {
                    "fontStyle": "italic",
                    "foreground": "#F92672"
                }
            },
            {
                "scope": ["storage.type", "meta.structure.dictionary.json string.quoted.double.json"],
                "settings": {
                    "foreground": "#66D9EF"
                }
            },
            {
                "scope": "entity.name.type, entity.name.class, entity.name.namespace, entity.name.scope-resolution",
                "settings": {
                    "fontStyle": "underline",
                    "foreground": "#A6E22E"
                }
            },
        ]
    }));

    theme_data.set_custom_token_colors(&custom_token_colors);

    let default_token_style: Option<TokenStyle> = None;

    let mut token_style = theme_data.resolve_scopes(&scopes(&[&["variable"]]), None);
    assert_token_style(token_style, Some(ts(Some("#F8F8F2"), Some(UNSET_STYLE))), "variable");

    token_style = theme_data.resolve_scopes(&scopes(&[&["keyword.operator"]]), None);
    assert_token_style(
        token_style,
        Some(ts(Some("#F92672"), Some(StyleFlags { italic: Some(true), bold: Some(true), underline: Some(true), ..UNDEFINED_STYLE }))),
        "keyword",
    );

    token_style = theme_data.resolve_scopes(&scopes(&[&["keyword"]]), None);
    assert_token_style(token_style, default_token_style.clone(), "keyword");

    token_style = theme_data.resolve_scopes(&scopes(&[&["keyword.operator"]]), None);
    assert_token_style(
        token_style,
        Some(ts(Some("#F92672"), Some(StyleFlags { italic: Some(true), bold: Some(true), underline: Some(true), ..UNDEFINED_STYLE }))),
        "keyword.operator",
    );

    token_style = theme_data.resolve_scopes(&scopes(&[&["keyword.operators"]]), None);
    assert_token_style(token_style, default_token_style.clone(), "keyword.operators");

    token_style = theme_data.resolve_scopes(&scopes(&[&["storage"]]), None);
    assert_token_style(
        token_style,
        Some(ts(Some("#F92672"), Some(StyleFlags { italic: Some(true), bold: Some(false), underline: Some(false), ..UNDEFINED_STYLE }))),
        "storage",
    );

    token_style = theme_data.resolve_scopes(&scopes(&[&["storage.type"]]), None);
    assert_token_style(
        token_style,
        Some(ts(Some("#66D9EF"), Some(StyleFlags { italic: Some(true), bold: Some(false), underline: Some(false), ..UNDEFINED_STYLE }))),
        "storage.type",
    );

    token_style = theme_data.resolve_scopes(&scopes(&[&["entity.name.class"]]), None);
    assert_token_style(
        token_style,
        Some(ts(Some("#A6E22E"), Some(StyleFlags { italic: Some(false), bold: Some(false), underline: Some(true), ..UNDEFINED_STYLE }))),
        "entity.name.class",
    );

    token_style = theme_data.resolve_scopes(&scopes(&[&["meta.structure.dictionary.json", "string.quoted.double.json"]]), None);
    assert_token_style(token_style, Some(ts(Some("#66D9EF"), None)), "json property");

    token_style = theme_data.resolve_scopes(
        &scopes(&[&["source.json", "meta.structure.dictionary.json", "string.quoted.double.json"]]),
        None,
    );
    assert_token_style(token_style, Some(ts(Some("#66D9EF"), None)), "json property");

    token_style = theme_data.resolve_scopes(&scopes(&[&["keyword"], &["storage.type"], &["entity.name.class"]]), None);
    assert_token_style(
        token_style,
        Some(ts(Some("#66D9EF"), Some(StyleFlags { italic: Some(true), bold: Some(false), underline: Some(false), ..UNDEFINED_STYLE }))),
        "storage.type",
    );
}

#[test]
fn resolve_scopes_match_most_specific() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let mut theme_data = ColorThemeData::create_loaded_empty_theme("test", "test");

    let custom_token_colors = object(json!({
        "textMateRules": [
            {
                "scope": "entity.name.type",
                "settings": {
                    "fontStyle": "underline",
                    "foreground": "#A6E22E"
                }
            },
            {
                "scope": "entity.name.type.class",
                "settings": {
                    "foreground": "#FF00FF"
                }
            },
            {
                "scope": "entity.name",
                "settings": {
                    "foreground": "#FFFFFF"
                }
            },
        ]
    }));

    theme_data.set_custom_token_colors(&custom_token_colors);

    let token_style = theme_data.resolve_scopes(&scopes(&[&["entity.name.type.class"]]), None);
    assert_token_style(
        token_style,
        Some(ts(Some("#FF00FF"), Some(StyleFlags { italic: Some(false), bold: Some(false), underline: Some(true), ..UNDEFINED_STYLE }))),
        "entity.name.type.class",
    );
}

#[test]
fn rule_matching() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let mut theme_data = ColorThemeData::create_loaded_empty_theme("test", "test");
    theme_data.set_custom_colors(&object(json!({ "editor.foreground": "#000000" })));
    theme_data.set_custom_semantic_token_colors(Some(&object(json!({
        "enabled": true,
        "rules": {
            "type": "#ff0000",
            "class": { "foreground": "#0000ff", "italic": true },
            "*.static": { "bold": true },
            "*.declaration": { "italic": true },
            "*.async.static": { "italic": true, "underline": true },
            "*.async": { "foreground": "#000fff", "underline": true }
        }
    }))));

    assert_token_styles(
        &theme_data,
        &[
            ("type", ts(Some("#ff0000"), Some(UNDEFINED_STYLE))),
            ("type.static", ts(Some("#ff0000"), Some(StyleFlags { bold: Some(true), ..UNDEFINED_STYLE }))),
            ("type.static.declaration", ts(Some("#ff0000"), Some(StyleFlags { bold: Some(true), italic: Some(true), ..UNDEFINED_STYLE }))),
            ("class", ts(Some("#0000ff"), Some(StyleFlags { italic: Some(true), ..UNDEFINED_STYLE }))),
            ("class.static.declaration", ts(Some("#0000ff"), Some(StyleFlags { bold: Some(true), italic: Some(true), ..UNDEFINED_STYLE }))),
            ("class.declaration", ts(Some("#0000ff"), Some(StyleFlags { italic: Some(true), ..UNDEFINED_STYLE }))),
            ("class.declaration.async", ts(Some("#000fff"), Some(StyleFlags { underline: Some(true), italic: Some(true), ..UNDEFINED_STYLE }))),
            (
                "class.declaration.async.static",
                ts(Some("#000fff"), Some(StyleFlags { italic: Some(true), underline: Some(true), bold: Some(true), ..UNDEFINED_STYLE })),
            ),
        ],
        "typescript",
    );
}

#[test]
fn super_type() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let registry = get_token_classification_registry();

    registry.register_token_type("myTestInterface", "A type just for testing", Some("interface"), None).expect("id válido");
    registry.register_token_type("myTestSubInterface", "A type just for testing", Some("myTestInterface"), None).expect("id válido");

    try_finally(
        || {
            let mut theme_data = ColorThemeData::create_loaded_empty_theme("test", "test");
            theme_data.set_custom_colors(&object(json!({ "editor.foreground": "#000000" })));
            theme_data.set_custom_semantic_token_colors(Some(&object(json!({
                "enabled": true,
                "rules": {
                    "interface": "#ff0000",
                    "myTestInterface": { "italic": true },
                    "interface.static": { "bold": true }
                }
            }))));

            assert_token_styles(
                &theme_data,
                &[("myTestSubInterface", ts(Some("#ff0000"), Some(StyleFlags { italic: Some(true), ..UNDEFINED_STYLE })))],
                "typescript",
            );
            assert_token_styles(
                &theme_data,
                &[(
                    "myTestSubInterface.static",
                    ts(Some("#ff0000"), Some(StyleFlags { italic: Some(true), bold: Some(true), ..UNDEFINED_STYLE })),
                )],
                "typescript",
            );

            theme_data.set_custom_semantic_token_colors(Some(&object(json!({
                "enabled": true,
                "rules": {
                    "interface": "#ff0000",
                    "myTestInterface": { "foreground": "#ff00ff", "italic": true }
                }
            }))));
            assert_token_styles(
                &theme_data,
                &[("myTestSubInterface", ts(Some("#ff00ff"), Some(StyleFlags { italic: Some(true), ..UNDEFINED_STYLE })))],
                "typescript",
            );
        },
        || {
            registry.deregister_token_type("myTestInterface");
            registry.deregister_token_type("myTestSubInterface");
        },
    );
}

#[test]
fn language() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    // O `try { ... } finally { }` do original tem o `finally` vazio, então o corpo vai direto.
    let mut theme_data = ColorThemeData::create_loaded_empty_theme("test", "test");
    theme_data.set_custom_colors(&object(json!({ "editor.foreground": "#000000" })));
    theme_data.set_custom_semantic_token_colors(Some(&object(json!({
        "enabled": true,
        "rules": {
            "interface": "#fff000",
            "interface:java": "#ff0000",
            "interface.static": { "bold": true },
            "interface.static:typescript": { "italic": true }
        }
    }))));

    assert_token_styles(&theme_data, &[("interface", ts(Some("#ff0000"), None))], "java");
    assert_token_styles(&theme_data, &[("interface", ts(Some("#fff000"), None))], "typescript");
    assert_token_styles(&theme_data, &[("interface.static", ts(Some("#ff0000"), Some(StyleFlags { bold: Some(true), ..UNDEFINED_STYLE })))], "java");
    assert_token_styles(
        &theme_data,
        &[("interface.static", ts(Some("#fff000"), Some(StyleFlags { bold: Some(true), italic: Some(true), ..UNDEFINED_STYLE })))],
        "typescript",
    );
}

#[test]
fn language_scope_resolving() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let registry = get_token_classification_registry();

    let number_of_default_rules = registry.get_token_styling_default_rules().len();

    registry.register_token_style_default(
        registry.parse_token_selector("type", Some("typescript1")),
        TokenStyleDefaults { scopes_to_probe: Some(scopes(&[&["entity.name.type.ts1"]])), ..TokenStyleDefaults::default() },
    );
    registry.register_token_style_default(
        registry.parse_token_selector("type:javascript1", None),
        TokenStyleDefaults { scopes_to_probe: Some(scopes(&[&["entity.name.type.js1"]])), ..TokenStyleDefaults::default() },
    );

    try_finally(
        || {
            let mut theme_data = ColorThemeData::create_loaded_empty_theme("test", "test");
            theme_data.set_custom_colors(&object(json!({ "editor.foreground": "#000000" })));
            theme_data.set_custom_token_colors(&object(json!({
                "textMateRules": [
                    {
                        "scope": "entity.name.type",
                        "settings": { "foreground": "#aa0000" }
                    },
                    {
                        "scope": "entity.name.type.ts1",
                        "settings": { "foreground": "#bb0000" }
                    }
                ]
            })));

            assert_token_styles(&theme_data, &[("type", ts(Some("#aa0000"), None))], "javascript1");
            assert_token_styles(&theme_data, &[("type", ts(Some("#bb0000"), None))], "typescript1");
        },
        || {
            registry.deregister_token_style_default(&registry.parse_token_selector("type", Some("typescript1")));
            registry.deregister_token_style_default(&registry.parse_token_selector("type:javascript1", None));

            assert_eq!(registry.get_token_styling_default_rules().len(), number_of_default_rules);
        },
    );
}
