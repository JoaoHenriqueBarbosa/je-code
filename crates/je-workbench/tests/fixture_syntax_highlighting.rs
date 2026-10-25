//! Porte de `src/vs/workbench/test/browser/componentFixtures/fixtureSyntaxHighlighting.test.ts`.
//!
//! Suíte `Component fixture TextMate syntax highlighting`, caso a caso e na mesma ordem. Depende de `ref/vscode` (fora do
//! git): as gramáticas e os temas das extensões embutidas são lidos de lá, como o original os serve do repositório.
//!
//! Notas de tradução:
//! - **`ensureNoDisposablesAreLeakedInTestSuite()`.** É o [`LeakGuard`] criado no começo de cada caso; `disposables.add`
//!   vira `guard.add_rc`. Os casos são `async` no original (leitura do tema e das gramáticas); aqui rodam em
//!   `run_with_real_timers`, porque o `ensure_loaded` do tema é assíncrono.
//! - **Tema.** O `themeLoaded ??= darkTheme.ensureLoaded(themeLoader)` (uma só leitura compartilhada) é o
//!   `ensure_loaded` do tema escuro, feito no começo de cada caso (cada caso tem o seu `ColorThemeData`, porque o objeto
//!   não é compartilhável entre threads de teste). O `themeLoader` anônimo é o `FixtureExtensionResourceLoader`.
//! - **DOM e estilo.** `mainWindow.document.body.appendChild($('div'))` é o `host` do documento do fixture,
//!   `host.querySelector('style')?.textContent` é `style_element_text()` e `getComputedStyle(token)` é o
//!   `computed_style()` do resolvedor (que interpreta o `@scope` do CSS dos tokens). `host.style.font`,
//!   `setProperty('--vscode-editor-foreground', ...)` e `host.appendChild($('span.mtkN.mtki.mtkb.mtku.mtks'))` são
//!   `set_style_property` e `append_child`.
//! - **`Color`.** `color.toString()` é o `Display` do `Color`; o mapa de cores do registro guarda texto, então `colorAt`
//!   passa pelo `Color::from_hex` para devolver o hexadecimal minúsculo do original.
//! - **`TokenizationRegistry.get(...) === null`.** É `None`. `Uint32Array` é `Vec<u32>`.
//! - **Estado dos casos anteriores.** O último caso parte de um mapa de cores válido no registro (`assert.ok(previousColorMap)`):
//!   no mocha ele existe porque os casos anteriores da suíte o deixaram instalado (o descarte só restaura um mapa anterior
//!   válido). Cada caso daqui roda na sua thread, com o registro vazio, então o caso reproduz esse estado criando e
//!   descartando um tokenizador antes de medir.
//! - **`deepStrictEqual` de objetos.** São tuplas comparadas campo a campo, na ordem do literal do original.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_real_timers};
use je_base::JsString;
use je_base::lifecycle::{DisposableStore, IDisposable, to_disposable};
use je_editor::encoded_token_attributes::{TokenMetadata, color_id};
use je_editor::language_service::{ILanguageService, LanguageService};
use je_editor::tokenization_registry::tokenization_registry;
use je_theme::color::Color;
use je_theme::color::format::css::format_rgb;
use je_workbench::services::themes::color_theme_data::ColorThemeData;
use je_workbench::services::themes::workbench_theme_service::{ExtensionData, IThemeExtensionPoint};
use je_workbench::test::browser::component_fixtures::fixture_css::{FixtureDocument, FixtureElement};
use je_workbench::test::browser::component_fixtures::fixture_resource_loader::{FixtureExtensionResourceLoader, fixture_resource_uri};
use je_workbench::test::browser::component_fixtures::fixture_syntax_highlighting::{register_fixture_languages, register_fixture_syntax_highlighting};

/// O tema de `extensions/theme-defaults/themes/<path>` como `ColorThemeData.fromExtensionTheme(...)`, já carregado.
async fn load_theme(id: &str, path: &str, ui_theme: &str) -> ColorThemeData {
    let point = IThemeExtensionPoint { id: id.to_owned(), label: None, description: None, path: path.to_owned(), ui_theme: Some(ui_theme.to_owned()), watch: false };
    let mut theme = ColorThemeData::from_extension_theme(
        &point,
        fixture_resource_uri(&format!("extensions/theme-defaults/themes/{path}")),
        ExtensionData::from_name("vscode", "theme-defaults", true),
    );
    theme.ensure_loaded(&FixtureExtensionResourceLoader).await.expect("o tema carrega");
    theme
}

/// O que `createTokenizers()` devolve: `{ store, languageService, host }`.
struct Tokenizers {
    store: Rc<DisposableStore>,
    language_service: Rc<LanguageService>,
    host: FixtureElement,
}

/// `createTokenizers(theme)`.
fn create_tokenizers(disposables: &LeakGuard, document: &Rc<FixtureDocument>, dark_theme: &ColorThemeData, theme: &ColorThemeData) -> Tokenizers {
    let store = disposables.add_rc(DisposableStore::new());
    let language_service = LanguageService::new(false);
    register_fixture_languages(&store, &*language_service);
    let host = document.body().append_child("");
    {
        let host = host.clone();
        store.add(to_disposable(move || host.remove()));
    }
    register_fixture_syntax_highlighting(&store, &host, dark_theme, theme).expect("os tokenizadores são registrados");
    Tokenizers { store, language_service, host }
}

/// `distinctLanguageIds(tokens)`: os ids de linguagem dos tokens, sem repetição, na ordem em que aparecem.
fn distinct_language_ids(tokens: &[u32]) -> Vec<u32> {
    let mut result = Vec::new();
    let mut index = 1;
    while index < tokens.len() {
        let language_id = TokenMetadata::get_language_id(tokens[index]);
        if !result.contains(&language_id) {
            result.push(language_id);
        }
        index += 2;
    }
    result
}

/// `distinctLanguageIds(tokens).map(id => languageService.languageIdCodec.decodeLanguageId(id))`.
fn decoded_language_ids(language_service: &LanguageService, tokens: &[u32]) -> Vec<String> {
    distinct_language_ids(tokens).into_iter().map(|id| language_service.language_id_codec().decode_language_id(id).to_string()).collect()
}

/// `colorAt(tokens, offset)`: a cor de primeiro plano do token que cobre o deslocamento.
fn color_at(tokens: &[u32], offset: usize) -> String {
    let mut metadata = tokens[1];
    let mut index = 2;
    while index < tokens.len() && tokens[index] as usize <= offset {
        metadata = tokens[index + 1];
        index += 2;
    }
    let color_map = tokenization_registry().get_color_map().expect("o registro tem o mapa de cores");
    let color = color_map.get(TokenMetadata::get_foreground(metadata) as usize).expect("a cor do token existe no mapa");
    Color::from_hex(color).to_string()
}

fn tokenize(language_id: &str, line: &str) -> Vec<u32> {
    let support = tokenization_registry().get(language_id).expect("o tokenizador da linguagem está registrado");
    support.tokenize_encoded(&JsString::from(line), true, &support.get_initial_state()).tokens
}

/// `try { body } finally { cleanup }`: a limpeza roda também se o corpo falhar.
fn try_finally(body: impl FnOnce(), cleanup: impl FnOnce()) {
    let outcome = catch_unwind(AssertUnwindSafe(body));
    cleanup();
    if let Err(panic) = outcome {
        resume_unwind(panic);
    }
}

#[test]
fn registers_actual_text_mate_tokenizers_for_every_fixture_language() {
    run_with_real_timers(async move {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let dark_theme = load_theme("2026-dark", "2026-dark.json", "vs-dark").await;
        let document = FixtureDocument::new();
        let tokenizers = create_tokenizers(&disposables, &document, &dark_theme, &dark_theme);
        let samples = [
            ("typescript", "const value: number = 1;"),
            ("typescriptreact", "const view = <div>{value}</div>;"),
            ("javascript", "const value = /actual+/gi;"),
            ("javascriptreact", "const view = <div>{value}</div>;"),
            ("json", "{ \"enabled\": true }"),
            ("css", ".fixture { color: #ff7b72; }"),
            ("html", "<main class=\"fixture\">text</main>"),
        ];

        // languageId, registered, tokenCount, encodedLanguageIds
        let actual: Vec<(String, bool, usize, Vec<String>)> = samples
            .iter()
            .map(|&(language_id, line)| {
                let tokens = tokenize(language_id, line);
                (
                    language_id.to_owned(),
                    tokenizers.language_service.is_registered_language_id(Some(language_id)),
                    tokens.len() / 2,
                    decoded_language_ids(&tokenizers.language_service, &tokens),
                )
            })
            .collect();

        let expected: Vec<(String, bool, usize, Vec<String>)> = [
            ("typescript", 11, vec!["typescript"]),
            ("typescriptreact", 16, vec!["typescriptreact", "jsx-tags"]),
            ("javascript", 10, vec!["javascript"]),
            ("javascriptreact", 16, vec!["javascriptreact", "jsx-tags"]),
            ("json", 5, vec!["json"]),
            ("css", 6, vec!["css"]),
            ("html", 11, vec!["html"]),
        ]
        .into_iter()
        .map(|(language_id, token_count, languages)| (language_id.to_owned(), true, token_count, languages.into_iter().map(str::to_owned).collect()))
        .collect();
        assert_eq!(actual, expected);
    });
}

#[test]
fn uses_dark_2026_text_mate_scope_distinctions_and_generated_token_css() {
    run_with_real_timers(async move {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let dark_theme = load_theme("2026-dark", "2026-dark.json", "vs-dark").await;
        let document = FixtureDocument::new();
        let tokenizers = create_tokenizers(&disposables, &document, &dark_theme, &dark_theme);
        let line = "import value from \"./value\"; class Example { private readonly field = 1; } const result = value;";
        let tokens = tokenize("typescript", line);
        let offset = |text: &str| line.find(text).expect("o texto está na linha");
        let style_text = tokenizers.host.style_element_text();
        let has_rule = |color: &str| style_text.as_deref().is_some_and(|text| text.contains(color));

        // import, private, readonly, const, hasPurpleRule, hasBlueRule, hasCoralRule
        assert_eq!(
            (
                color_at(&tokens, offset("import")),
                color_at(&tokens, offset("private")),
                color_at(&tokens, offset("readonly")),
                color_at(&tokens, offset("const")),
                has_rule("#c586c0"),
                has_rule("#569cd6"),
                has_rule("#ff7b72"),
            ),
            ("#c586c0".to_owned(), "#569cd6".to_owned(), "#569cd6".to_owned(), "#ff7b72".to_owned(), true, true, true),
        );
    });
}

#[test]
fn preserves_multiline_text_mate_state_and_embedded_language_ids() {
    run_with_real_timers(async move {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let dark_theme = load_theme("2026-dark", "2026-dark.json", "vs-dark").await;
        let document = FixtureDocument::new();
        let tokenizers = create_tokenizers(&disposables, &document, &dark_theme, &dark_theme);
        let registry = tokenization_registry();
        let typescript = registry.get("typescript").expect("o tokenizador de typescript está registrado");
        let initial_state = typescript.get_initial_state();
        let comment_start = typescript.tokenize_encoded(&JsString::from("/* first line"), true, &initial_state);
        let comment_end_line = "continued */ const value = 1;";
        let comment_end = typescript.tokenize_encoded(&JsString::from(comment_end_line), true, &comment_start.end_state);

        let html = registry.get("html").expect("o tokenizador de html está registrado");
        let script_start = html.tokenize_encoded(&JsString::from("<script>"), true, &html.get_initial_state());
        let embedded_script = html.tokenize_encoded(&JsString::from("const value = 1;"), true, &script_start.end_state);

        // stateChanged, continuedComment, constAfterComment, embeddedLanguages
        assert_eq!(
            (
                !comment_start.end_state.equals(&*initial_state),
                color_at(&comment_end.tokens, 0),
                color_at(&comment_end.tokens, comment_end_line.find("const").expect("o texto está na linha")),
                decoded_language_ids(&tokenizers.language_service, &embedded_script.tokens),
            ),
            (true, "#8b949e".to_owned(), "#ff7b72".to_owned(), vec!["javascript".to_owned()]),
        );
    });
}

#[test]
fn uses_monochrome_token_styles_in_non_dark_fixtures_without_affecting_dark() {
    run_with_real_timers(async move {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let dark_theme = load_theme("2026-dark", "2026-dark.json", "vs-dark").await;
        let document = FixtureDocument::new();
        let variants = [("Light", "light_modern.json", "vs"), ("DarkHighContrast", "hc_black.json", "hc-black"), ("LightHighContrast", "hc_light.json", "hc-light"), ("Dark", "2026-dark.json", "vs-dark")];
        let mut tokenizers_alive: Vec<Tokenizers> = Vec::new();
        let mut rendered: Vec<(&str, FixtureElement, Color)> = Vec::new();
        for (name, path, ui_theme) in variants {
            let theme = load_theme(name, path, ui_theme).await;
            let tokenizers = create_tokenizers(&disposables, &document, &dark_theme, &theme);
            let foreground = theme.get_color("editor.foreground", true).expect("o tema define o primeiro plano do editor");
            tokenizers.host.set_style_property("--vscode-editor-foreground", &foreground.to_string());
            tokenizers.host.set_style_property("font", "14px monospace");

            let tokens = tokenize("typescript", "import value from \"./value\";");
            let token = tokenizers.host.append_child(&format!("mtk{} mtki mtkb mtku mtks", TokenMetadata::get_foreground(tokens[1])));
            rendered.push((name, token, foreground));
            tokenizers_alive.push(tokenizers);
        }

        // name, color, fontStyle, fontWeight, textDecoration
        let actual: Vec<(String, String, String, String, String)> = rendered
            .iter()
            .map(|(name, token, _)| {
                let style = token.computed_style();
                ((*name).to_owned(), style.color(), style.font_style(), style.font_weight(), style.text_decoration_line())
            })
            .collect();
        let expected: Vec<(String, String, String, String, String)> = rendered
            .iter()
            .map(|(name, _, foreground)| {
                let dark = *name == "Dark";
                (
                    (*name).to_owned(),
                    if dark { "rgb(197, 134, 192)".to_owned() } else { format_rgb(foreground) },
                    (if dark { "italic" } else { "normal" }).to_owned(),
                    (if dark { "700" } else { "400" }).to_owned(),
                    (if dark { "underline line-through" } else { "none" }).to_owned(),
                )
            })
            .collect();
        assert_eq!(actual, expected);
    });
}

#[test]
fn retains_shared_tokenizers_until_the_final_fixture_is_disposed() {
    run_with_real_timers(async move {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let dark_theme = load_theme("2026-dark", "2026-dark.json", "vs-dark").await;
        let document = FixtureDocument::new();
        let registry = tokenization_registry();

        // O mapa de cores que os casos anteriores da suíte deixam instalado (ver as notas de tradução).
        if registry.get_color_map().is_none() {
            create_tokenizers(&disposables, &document, &dark_theme, &dark_theme).store.dispose();
        }
        let previous_color_map = registry.get_color_map().expect("o registro tem o mapa de cores");
        let mut sentinel_color_map = previous_color_map.clone();
        sentinel_color_map[color_id::DEFAULT_FOREGROUND as usize] = Color::from_hex("#010203").to_string();
        registry.set_color_map(sentinel_color_map.clone());

        try_finally(
            || {
                let first = create_tokenizers(&disposables, &document, &dark_theme, &dark_theme);
                let second = create_tokenizers(&disposables, &document, &dark_theme, &dark_theme);
                let support = registry.get("typescript");
                first.store.dispose();

                let retained_after_first_dispose = match (&support, &registry.get("typescript")) {
                    (Some(before), Some(after)) => Rc::ptr_eq(before, after),
                    _ => false,
                };
                let second_uses_canonical_dark_css = second.host.style_element_text().is_some_and(|text| text.contains("#ff7b72"));
                // retainedAfterFirstDispose, secondUsesCanonicalDarkCSS
                assert_eq!((retained_after_first_dispose, second_uses_canonical_dark_css), (true, true));

                second.store.dispose();
                let final_color_map = registry.get_color_map();
                let default_background = registry.get_default_background();
                // tokenizerRemoved, sentinelColorMapRestored, defaultBackgroundLuminance
                assert_eq!(
                    (
                        registry.get("typescript").is_none(),
                        final_color_map.as_ref() == Some(&sentinel_color_map),
                        default_background.is_some_and(|color| Color::from_hex(&color).get_relative_luminance().is_finite()),
                    ),
                    (true, true, true),
                );
            },
            || registry.set_color_map(previous_color_map.clone()),
        );
    });
}
