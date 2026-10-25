//! Porte de `src/vs/workbench/test/browser/componentFixtures/fixtureSyntaxHighlighting.ts`: os tokenizadores TextMate reais
//! (gramáticas das extensões embutidas) e o CSS de tokens que os fixtures de componentes instalam, com contagem de
//! referências para o tokenizador compartilhado.
//!
//! Notas de tradução:
//! - **Gramáticas pelo `TextMateTokenizationFeature`.** O original monta o `TMGrammarFactory` e um
//!   `TextMateTokenizationSupport` por linguagem à mão. Aqui as mesmas gramáticas (a tabela [`GRAMMAR_DESCRIPTORS`]) entram
//!   como `GrammarContribution` no `TextMateTokenizationFeature` de `je-languages` (o `ferriki-textmate` sobre o `ferroni`),
//!   que faz o papel do `TMGrammarFactory`; cada `TextMateTokenizationSupport` é criado à mão
//!   (`create_text_mate_tokenization_support`), guardado no `store` e registrado no `TokenizationRegistry`, como o original.
//!   O `onig.wasm` do `vscode-oniguruma` não existe: o Oniguruma é o `ferroni`, compilado na crate.
//! - **Tema.** `setTheme(rawTheme, darkTheme.tokenColorMap)` é `set_raw_theme` com o `IRawTheme` montado das
//!   `tokenColors` do `ColorThemeData` e o `tokenColorMap` como mapa de cores (os ids das classes `.mtkN` do CSS).
//! - **Mapa de cores do registro.** O `TokenizationRegistry` do porte guarda o mapa como texto (`#rrggbb`), e não como
//!   `Color`; `isValidColorMap` confere o tamanho e o texto não vazio de cada cor. A restauração no descarte compara o mapa
//!   por valor (o original compara a identidade do array).
//! - **O `<style>`.** `createStyleSheet(fixtureHost, undefined, disposables)` e o `textContent` são
//!   [`FixtureElement::append_style_element`], removido quando o `store` é descartado.
//! - **Assíncrono.** `registerFixtureSyntaxHighlighting` é síncrona: a criação do suporte, que no original espera a
//!   leitura do arquivo, é síncrona no `ferriki`.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

use je_base::lifecycle::{DisposableRef, DisposableStore, IDisposable, to_disposable};
use je_editor::encoded_token_attributes::color_id;
use je_editor::language_service::{ILanguageService, LanguageService};
use je_editor::modes_registry::LanguageExtensionPoint;
use je_editor::token_theme::{IFontTokenOptions, generate_tokens_css_for_color_map, generate_tokens_css_for_font_map};
use je_editor::tokenization_registry::tokenization_registry;
use je_languages::extensions::GrammarContribution;
use je_languages::text_mate_tokenization_feature::TextMateTokenizationFeature;
use je_theme::ThemeType;
use je_theme::color::Color;
use serde_json::{Map, Value};

use super::fixture_css::FixtureElement;
use super::fixture_resource_loader::fixture_root;
use crate::services::themes::color_theme_data::ColorThemeData;
use crate::services::themes::theme_compatibility::{TextMateScope, TextMateThemingRule};

/// `fixtureLanguageIds`.
pub const FIXTURE_LANGUAGE_IDS: [&str; 8] = ["typescript", "typescriptreact", "javascript", "javascriptreact", "json", "css", "html", "jsx-tags"];

/// `commonScriptTokenTypes` (`StandardTokenType.Other`).
const COMMON_SCRIPT_TOKEN_TYPES: &[(&str, &str)] = &[
    ("punctuation.definition.template-expression", "other"),
    ("entity.name.type.instance.jsdoc", "other"),
    ("entity.name.function.tagged-template", "other"),
    ("meta.import string.quoted", "other"),
    ("variable.other.jsdoc", "other"),
];

/// `FixtureGrammarDescriptor`.
struct FixtureGrammarDescriptor {
    language: Option<&'static str>,
    scope_name: &'static str,
    path: &'static str,
    embedded_languages: &'static [(&'static str, &'static str)],
    token_types: &'static [(&'static str, &'static str)],
    inject_to: Option<&'static [&'static str]>,
    unbalanced_bracket_scopes: Option<&'static [&'static str]>,
}

const HTML_EMBEDDED_LANGUAGES: &[(&str, &str)] = &[("text.html", "html"), ("source.css", "css"), ("source.js", "javascript")];
const HTML_TOKEN_TYPES: &[(&str, &str)] = &[("meta.tag string.quoted", "other")];

/// `grammarDescriptors`.
const GRAMMAR_DESCRIPTORS: &[FixtureGrammarDescriptor] = &[
    FixtureGrammarDescriptor {
        language: Some("typescript"),
        scope_name: "source.ts",
        path: "extensions/typescript-basics/syntaxes/TypeScript.tmLanguage.json",
        embedded_languages: &[],
        token_types: COMMON_SCRIPT_TOKEN_TYPES,
        inject_to: None,
        unbalanced_bracket_scopes: Some(&["keyword.operator.relational", "storage.type.function.arrow", "keyword.operator.bitwise.shift", "meta.brace.angle", "punctuation.definition.tag", "keyword.operator.assignment.compound.bitwise.ts"]),
    },
    FixtureGrammarDescriptor {
        language: Some("typescriptreact"),
        scope_name: "source.tsx",
        path: "extensions/typescript-basics/syntaxes/TypeScriptReact.tmLanguage.json",
        embedded_languages: &[("meta.tag.tsx", "jsx-tags"), ("meta.tag.without-attributes.tsx", "jsx-tags"), ("meta.tag.attributes.tsx", "typescriptreact"), ("meta.embedded.expression.tsx", "typescriptreact")],
        token_types: COMMON_SCRIPT_TOKEN_TYPES,
        inject_to: None,
        unbalanced_bracket_scopes: Some(&["keyword.operator.relational", "storage.type.function.arrow", "keyword.operator.bitwise.shift", "punctuation.definition.tag", "keyword.operator.assignment.compound.bitwise.ts"]),
    },
    FixtureGrammarDescriptor {
        language: None,
        scope_name: "documentation.injection.ts",
        path: "extensions/typescript-basics/syntaxes/jsdoc.ts.injection.tmLanguage.json",
        embedded_languages: &[],
        token_types: &[],
        inject_to: Some(&["source.ts", "source.tsx"]),
        unbalanced_bracket_scopes: None,
    },
    FixtureGrammarDescriptor {
        language: None,
        scope_name: "documentation.injection.js.jsx",
        path: "extensions/typescript-basics/syntaxes/jsdoc.js.injection.tmLanguage.json",
        embedded_languages: &[],
        token_types: &[],
        inject_to: Some(&["source.js", "source.js.jsx"]),
        unbalanced_bracket_scopes: None,
    },
    FixtureGrammarDescriptor {
        language: Some("javascript"),
        scope_name: "source.js",
        path: "extensions/javascript/syntaxes/JavaScript.tmLanguage.json",
        embedded_languages: &[("meta.tag.js", "jsx-tags"), ("meta.tag.without-attributes.js", "jsx-tags"), ("meta.tag.attributes.js", "javascript"), ("meta.embedded.expression.js", "javascript")],
        token_types: COMMON_SCRIPT_TOKEN_TYPES,
        inject_to: None,
        unbalanced_bracket_scopes: None,
    },
    FixtureGrammarDescriptor {
        language: Some("javascriptreact"),
        scope_name: "source.js.jsx",
        path: "extensions/javascript/syntaxes/JavaScriptReact.tmLanguage.json",
        embedded_languages: &[("meta.tag.js", "jsx-tags"), ("meta.tag.without-attributes.js", "jsx-tags"), ("meta.tag.attributes.js.jsx", "javascriptreact"), ("meta.embedded.expression.js", "javascriptreact")],
        token_types: COMMON_SCRIPT_TOKEN_TYPES,
        inject_to: None,
        unbalanced_bracket_scopes: None,
    },
    FixtureGrammarDescriptor {
        language: None,
        scope_name: "source.js.regexp",
        path: "extensions/javascript/syntaxes/Regular Expressions (JavaScript).tmLanguage",
        embedded_languages: &[],
        token_types: &[],
        inject_to: None,
        unbalanced_bracket_scopes: None,
    },
    FixtureGrammarDescriptor {
        language: Some("json"),
        scope_name: "source.json",
        path: "extensions/json/syntaxes/JSON.tmLanguage.json",
        embedded_languages: &[],
        token_types: &[],
        inject_to: None,
        unbalanced_bracket_scopes: None,
    },
    FixtureGrammarDescriptor {
        language: Some("css"),
        scope_name: "source.css",
        path: "extensions/css/syntaxes/css.tmLanguage.json",
        embedded_languages: &[],
        token_types: &[("meta.function.url string.quoted", "other")],
        inject_to: None,
        unbalanced_bracket_scopes: None,
    },
    FixtureGrammarDescriptor {
        language: None,
        scope_name: "text.html.basic",
        path: "extensions/html/syntaxes/html.tmLanguage.json",
        embedded_languages: HTML_EMBEDDED_LANGUAGES,
        token_types: HTML_TOKEN_TYPES,
        inject_to: None,
        unbalanced_bracket_scopes: None,
    },
    FixtureGrammarDescriptor {
        language: Some("html"),
        scope_name: "text.html.derivative",
        path: "extensions/html/syntaxes/html-derivative.tmLanguage.json",
        embedded_languages: HTML_EMBEDDED_LANGUAGES,
        token_types: HTML_TOKEN_TYPES,
        inject_to: None,
        unbalanced_bracket_scopes: None,
    },
];

/// `registerFixtureLanguages(disposables, languageService)`.
pub fn register_fixture_languages(disposables: &DisposableStore, language_service: &dyn ILanguageService) {
    for language_id in FIXTURE_LANGUAGE_IDS {
        disposables.add(language_service.register_language(LanguageExtensionPoint::new(language_id)));
    }
}

/// `registerFixtureSyntaxHighlighting(disposables, fixtureHost, darkTheme, theme)`: adquire o tokenizador compartilhado e
/// instala no `fixture_host` o CSS dos tokens. No tema escuro o CSS é o do `tokenColorMap` e do `tokenFontMap` do Dark
/// 2026; nos demais, os tokens ficam monocromáticos (herdam o primeiro plano do editor).
pub fn register_fixture_syntax_highlighting(disposables: &DisposableStore, fixture_host: &FixtureElement, dark_theme: &ColorThemeData, theme: &ColorThemeData) -> Result<(), String> {
    disposables.add(acquire_fixture_text_mate_tokenizers(dark_theme)?);
    let color_map = to_color_map(&dark_theme.token_color_map());
    let styles = if theme.theme_type() == ThemeType::Dark {
        let font_map: Vec<IFontTokenOptions> = dark_theme.token_font_map().into_iter().map(Option::unwrap_or_default).collect();
        format!("{}\n{}", generate_tokens_css_for_color_map(&color_map), generate_tokens_css_for_font_map(&font_map))
    } else {
        let selectors: Vec<String> = (1..color_map.len()).map(|index| format!(".mtk{index}")).collect();
        format!("{} {{ color: var(--vscode-editor-foreground); font: inherit; text-decoration: none; }}", selectors.join(", "))
    };
    fixture_host.append_style_element(&format!("@scope {{\n{styles}\n}}"), disposables)
}

/// `FixtureTextMateTokenizers`: o tokenizador é criado na primeira aquisição e descartado na última liberação.
struct FixtureTextMateTokenizers {
    references: usize,
    instance: Option<Rc<FixtureTextMateTokenizerInstance>>,
}

thread_local! {
    static FIXTURE_TEXT_MATE_TOKENIZERS: RefCell<FixtureTextMateTokenizers> = const { RefCell::new(FixtureTextMateTokenizers { references: 0, instance: None }) };
}

/// `fixtureTextMateTokenizers.acquire(darkTheme)`. Descartar o resultado libera a referência (só a primeira vez).
fn acquire_fixture_text_mate_tokenizers(dark_theme: &ColorThemeData) -> Result<DisposableRef, String> {
    let needs_instance = FIXTURE_TEXT_MATE_TOKENIZERS.with(|shared| shared.borrow().instance.is_none());
    if needs_instance {
        let instance = FixtureTextMateTokenizerInstance::create(dark_theme)?;
        FIXTURE_TEXT_MATE_TOKENIZERS.with(|shared| shared.borrow_mut().instance = Some(Rc::new(instance)));
    }
    FIXTURE_TEXT_MATE_TOKENIZERS.with(|shared| shared.borrow_mut().references += 1);

    let disposed = Rc::new(Cell::new(false));
    Ok(to_disposable(move || {
        if disposed.replace(true) {
            return;
        }
        let last = FIXTURE_TEXT_MATE_TOKENIZERS.with(|shared| {
            let mut shared = shared.borrow_mut();
            shared.references -= 1;
            if shared.references == 0 { shared.instance.take() } else { None }
        });
        if let Some(instance) = last {
            instance.dispose();
        }
    }))
}

/// `FixtureTextMateTokenizerInstance`.
struct FixtureTextMateTokenizerInstance {
    store: Rc<DisposableStore>,
    installed_color_map: Vec<String>,
    previous_color_map: Option<Vec<String>>,
}

impl FixtureTextMateTokenizerInstance {
    fn create(dark_theme: &ColorThemeData) -> Result<Self, String> {
        let store = DisposableStore::new();
        let language_service = store.add_rc(LanguageService::new(false));
        register_fixture_languages(&store, &*language_service);
        let language_service: Rc<dyn ILanguageService> = language_service;
        let feature = TextMateTokenizationFeature::new(language_service, &grammar_contributions(), HashMap::new());
        store.add_rc(feature.clone());

        let registry = tokenization_registry();
        let current_color_map = registry.get_color_map();
        let previous_color_map = current_color_map.filter(|color_map| is_valid_color_map(color_map));
        let token_color_map: Vec<String> = dark_theme.token_color_map().into_iter().map(Option::unwrap_or_default).collect();
        feature.set_raw_theme(raw_theme(dark_theme), Some(token_color_map));

        // Como o original, cada suporte é criado à mão, guardado no `store` e registrado (o registro também sai no descarte).
        for language_id in FIXTURE_LANGUAGE_IDS {
            if registry.get(language_id).is_some() {
                continue;
            }
            let Some(support) = feature.create_text_mate_tokenization_support(language_id) else { continue };
            store.add_rc(support.clone());
            store.add(registry.register(language_id, support));
        }

        let installed_color_map = registry.get_color_map().ok_or_else(|| "o tokenizador não publicou o mapa de cores".to_owned())?;
        Ok(Self { store, installed_color_map, previous_color_map })
    }
}

impl IDisposable for FixtureTextMateTokenizerInstance {
    fn dispose(&self) {
        if let Some(previous) = &self.previous_color_map {
            let registry = tokenization_registry();
            if registry.get_color_map().as_ref() == Some(&self.installed_color_map) {
                registry.set_color_map(previous.clone());
            }
        }
        self.store.dispose();
    }
}

/// `grammarDescriptors` como as contribuições `grammars` das extensões, com o repositório do oráculo como pasta.
fn grammar_contributions() -> Vec<GrammarContribution> {
    let root: PathBuf = fixture_root();
    let pairs = |entries: &[(&str, &str)]| -> Vec<(String, String)> { entries.iter().map(|(scope, value)| ((*scope).to_owned(), (*value).to_owned())).collect() };
    GRAMMAR_DESCRIPTORS
        .iter()
        .map(|descriptor| GrammarContribution {
            extension_id: format!("vscode.{}", descriptor.path.split('/').nth(1).unwrap_or_default()),
            extension_dir: root.clone(),
            language: descriptor.language.map(str::to_owned),
            scope_name: descriptor.scope_name.to_owned(),
            path: descriptor.path.to_owned(),
            embedded_languages: pairs(descriptor.embedded_languages),
            token_types: pairs(descriptor.token_types),
            inject_to: descriptor.inject_to.map(|scopes| scopes.iter().map(|scope| (*scope).to_owned()).collect()),
            balanced_bracket_scopes: None,
            unbalanced_bracket_scopes: descriptor.unbalanced_bracket_scopes.map(|scopes| scopes.iter().map(|scope| (*scope).to_owned()).collect()),
            invalid_language_type: false,
        })
        .collect()
}

/// O `IRawTheme` (`{ name, settings }`) das `tokenColors` do tema.
fn raw_theme(theme: &ColorThemeData) -> Value {
    let settings: Vec<Value> = theme.token_colors().iter().map(rule_to_json).collect();
    let mut object = Map::new();
    object.insert("name".to_owned(), Value::String(theme.label.clone()));
    object.insert("settings".to_owned(), Value::Array(settings));
    Value::Object(object)
}

fn rule_to_json(rule: &TextMateThemingRule) -> Value {
    let mut object = Map::new();
    match &rule.scope {
        Some(TextMateScope::One(scope)) => {
            object.insert("scope".to_owned(), Value::String(scope.clone()));
        }
        Some(TextMateScope::Many(scopes)) => {
            object.insert("scope".to_owned(), Value::Array(scopes.iter().cloned().map(Value::String).collect()));
        }
        None => {}
    }
    if let Some(settings) = &rule.settings {
        object.insert("settings".to_owned(), Value::Object(settings.iter().map(|(key, value)| (key.clone(), value.clone())).collect()));
    }
    Value::Object(object)
}

/// `toColorMap(colorMap)`: o índice 0 fica vazio.
pub fn to_color_map(color_map: &[Option<String>]) -> Vec<Option<Color>> {
    color_map.iter().enumerate().map(|(index, hex)| if index == 0 { None } else { hex.as_deref().map(Color::from_hex) }).collect()
}

/// `isValidColorMap(colorMap)`.
pub fn is_valid_color_map(color_map: &[String]) -> bool {
    color_map.len() > color_id::DEFAULT_BACKGROUND as usize && color_map.iter().skip(1).all(|color| !color.is_empty())
}
