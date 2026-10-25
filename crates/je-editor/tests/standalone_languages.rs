//! Porte de `src/vs/editor/standalone/test/browser/standaloneLanguages.test.ts`.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o `LeakGuard` de cada caso, criado no começo e conferido quando sai de
//!   escopo; o `DisposableStore` local do `testBadTokensProvider` é o `DisposableStore` do `je-base`.
//! - `class MockTokenTheme extends TokenTheme` (sobrescreve `match` com um contador) é o [`MockTokenTheme`], que implementa
//!   o [`ITokenTheme`] do módulo `standalone_theme` (o `TokenTheme` concreto não é herdável). O `(counter++ << FOREGROUND_OFFSET)
//!   | (languageId << LANGUAGEID_OFFSET)) >>> 0` é a mesma conta em `u32`.
//! - `MockThemeService implements IStandaloneThemeService` implementa o supertrait [`IThemeService`] e o
//!   [`IStandaloneThemeService`]; os membros que o original lança `Not implemented` lançam o mesmo erro, e o
//!   `getColorTheme()` do standalone é `get_standalone_color_theme` (um tema novo, com um `MockTokenTheme` de contador zerado,
//!   a cada chamada, como o literal do original).
//! - `MockState.INSTANCE` (singleton com construtor privado, `equals` por identidade) é um `Rc` por thread; `equals` compara
//!   o endereço do objeto.
//! - `Token` e `number[]` do `deepStrictEqual` são `Vec<Token>` e `Vec<u32>` comparados com `assert_eq!`.
//! - `expectedModernTokens[i] |= tokenLanguageMetadata` altera o vetor esperado no lugar, como o original.
#![allow(clippy::identity_op)] // o `0 << FOREGROUND_OFFSET` do esperado é literal do original

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::any::Any;
use std::cell::Cell;
use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::JsString;
use je_base::errors::{self, Error};
use je_base::event::Event;
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::uri::URI;
use je_editor::encoded_token_attributes::metadata_consts;
use je_editor::language_selector::{FilterPattern, LanguageFilter, LanguageSelector};
use je_editor::language_service::{ILanguageService, LanguageService};
use je_editor::languages::{IState, ITokenizationSupport, State, Token};
use je_editor::modes_registry::LanguageExtensionPoint;
use je_editor::services::theme_service::{IColorTheme, IThemeService};
use je_editor::standalone::standalone_languages::{ILineTokens, IToken, TokenizationSupportAdapter, TokensProvider, score};
use je_editor::standalone::standalone_theme::{IStandaloneTheme, IStandaloneThemeService, ITokenTheme, StandaloneThemeData};
use je_theme::ThemeType;
use je_theme::color::Color;

#[test]
fn language_selector_score() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();

    let uri = URI::parse("file:///test/file.ts", false);
    let typescript = LanguageSelector::from("typescript");
    let filter = LanguageSelector::from(LanguageFilter {
        language: Some("typescript".to_string()),
        scheme: Some("file".to_string()),
        pattern: Some(FilterPattern::from("**/*.ts")),
        ..LanguageFilter::default()
    });
    let javascript = LanguageSelector::from("javascript");

    assert_eq!(
        [score(Some(&typescript), &uri, "typescript"), score(Some(&filter), &uri, "typescript"), score(Some(&javascript), &uri, "typescript")],
        [10, 10, 0]
    );
}

const LANGUAGE_ID: &str = "tttt";

/// `MockTokenTheme`.
struct MockTokenTheme {
    counter: Cell<u32>,
}

impl MockTokenTheme {
    fn new() -> Self {
        Self { counter: Cell::new(0) }
    }
}

impl ITokenTheme for MockTokenTheme {
    fn match_token(&self, language_id: u32, _token: &str) -> u32 {
        let counter = self.counter.get();
        self.counter.set(counter + 1);
        (counter << metadata_consts::FOREGROUND_OFFSET) | (language_id << metadata_consts::LANGUAGEID_OFFSET)
    }
}

/// O literal de `IStandaloneTheme` que o `getColorTheme()` do `MockThemeService` devolve.
struct MockStandaloneTheme {
    token_theme: Rc<MockTokenTheme>,
}

fn not_implemented() -> ! {
    errors::throw(Error::new("Not implemented"))
}

impl IColorTheme for MockStandaloneTheme {
    fn get_color(&self, _color: &str, _use_default: Option<bool>) -> Option<Color> {
        not_implemented()
    }

    fn theme_type(&self) -> ThemeType {
        ThemeType::Light
    }
}

impl IStandaloneTheme for MockStandaloneTheme {
    fn token_theme(&self) -> Rc<dyn ITokenTheme> {
        self.token_theme.clone()
    }

    fn theme_name(&self) -> String {
        "light".to_string()
    }
}

/// `MockThemeService`.
struct MockThemeService;

impl IThemeService for MockThemeService {
    fn get_color_theme(&self) -> Rc<dyn IColorTheme> {
        self.get_standalone_color_theme()
    }

    fn on_did_color_theme_change(&self) -> Event<Rc<dyn IColorTheme>> {
        Event::none()
    }
}

impl IStandaloneThemeService for MockThemeService {
    fn set_theme(&self, _theme_name: &str) {
        not_implemented()
    }

    fn set_auto_detect_high_contrast(&self, _auto_detect_high_contrast: bool) {
        not_implemented()
    }

    fn define_theme(&self, _theme_name: &str, _theme_data: StandaloneThemeData) {
        not_implemented()
    }

    fn get_standalone_color_theme(&self) -> Rc<dyn IStandaloneTheme> {
        Rc::new(MockStandaloneTheme { token_theme: Rc::new(MockTokenTheme::new()) })
    }

    fn set_color_map_override(&self, _color_map_override: Option<Vec<Option<Color>>>) {}
}

/// `MockState`: `INSTANCE` é o único objeto, e `equals` é a identidade.
struct MockState;

thread_local! {
    static MOCK_STATE_INSTANCE: Rc<MockState> = Rc::new(MockState);
}

/// `MockState.INSTANCE`.
fn mock_state() -> State {
    MOCK_STATE_INSTANCE.with(|instance| instance.clone() as State)
}

impl IState for MockState {
    fn clone_state(&self) -> State {
        mock_state()
    }

    fn equals(&self, other: &dyn IState) -> bool {
        std::ptr::addr_eq(self as *const MockState, other as *const dyn IState)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// `BadTokensProvider`.
struct BadTokensProvider {
    tokens: Vec<IToken>,
}

impl TokensProvider for BadTokensProvider {
    fn get_initial_state(&self) -> State {
        mock_state()
    }

    fn tokenize(&self, _line: &JsString, _state: &State) -> ILineTokens {
        ILineTokens { tokens: self.tokens.clone(), end_state: mock_state() }
    }
}

fn token(start_index: usize, scopes: &str) -> IToken {
    IToken { start_index, scopes: scopes.to_string() }
}

/// `testBadTokensProvider(providerTokens, expectedClassicTokens, expectedModernTokens)`.
fn test_bad_tokens_provider(provider_tokens: Vec<IToken>, expected_classic_tokens: Vec<Token>, mut expected_modern_tokens: Vec<u32>) {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let disposables = DisposableStore::new();
    let language_service = LanguageService::new(false);
    disposables.add(language_service.clone());
    disposables.add(language_service.register_language(LanguageExtensionPoint::new(LANGUAGE_ID)));
    let adapter = TokenizationSupportAdapter::new(
        LANGUAGE_ID,
        Rc::new(BadTokensProvider { tokens: provider_tokens }),
        language_service.clone(),
        Rc::new(MockThemeService),
    );

    let line = JsString::from("whatever");
    let actual_classic_tokens = adapter.tokenize(&line, true, &mock_state());
    assert_eq!(actual_classic_tokens.tokens, expected_classic_tokens);

    let actual_modern_tokens = adapter.tokenize_encoded(&line, true, &mock_state());
    let modern_tokens: Vec<u32> = actual_modern_tokens.tokens.clone();

    // Add the encoded language id to the expected tokens
    let encoded_language_id = language_service.language_id_codec().encode_language_id(LANGUAGE_ID);
    let token_language_metadata = encoded_language_id << metadata_consts::LANGUAGEID_OFFSET;
    for i in (1..expected_modern_tokens.len()).step_by(2) {
        expected_modern_tokens[i] |= token_language_metadata;
    }
    assert_eq!(modern_tokens, expected_modern_tokens);

    disposables.dispose();
}

#[test]
fn tokens_always_start_at_index_0() {
    test_bad_tokens_provider(
        vec![token(7, "foo"), token(0, "bar")],
        vec![Token::new(0, "foo", LANGUAGE_ID), Token::new(0, "bar", LANGUAGE_ID)],
        vec![
            0,
            (0 << metadata_consts::FOREGROUND_OFFSET) | metadata_consts::BALANCED_BRACKETS_MASK,
            0,
            (1 << metadata_consts::FOREGROUND_OFFSET) | metadata_consts::BALANCED_BRACKETS_MASK,
        ],
    );
}

#[test]
fn tokens_always_start_after_each_other() {
    test_bad_tokens_provider(
        vec![token(0, "foo"), token(5, "bar"), token(3, "foo")],
        vec![Token::new(0, "foo", LANGUAGE_ID), Token::new(5, "bar", LANGUAGE_ID), Token::new(5, "foo", LANGUAGE_ID)],
        vec![
            0,
            (0 << metadata_consts::FOREGROUND_OFFSET) | metadata_consts::BALANCED_BRACKETS_MASK,
            5,
            (1 << metadata_consts::FOREGROUND_OFFSET) | metadata_consts::BALANCED_BRACKETS_MASK,
            5,
            (2 << metadata_consts::FOREGROUND_OFFSET) | metadata_consts::BALANCED_BRACKETS_MASK,
        ],
    );
}
