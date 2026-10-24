//! Porte de `src/vs/editor/contrib/smartSelect/test/browser/smartSelect.test.ts` (suíte `SmartSelect`), caso a caso e na
//! mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()`, o `suiteSetup`/`suiteTeardown` (que sobem `_maxDuration` do provedor de
//!   colchetes para 5 segundos e o devolvem), o `setup` (a linguagem `mockJSMode` e a configuração com os colchetes, as
//!   regras de Enter do JavaScript e o padrão de palavra) e o `teardown` são [`run_test`], que roda o corpo do caso com
//!   o relógio virtual (`runWithFakedTimers`): os casos são `async` e esperam promessas.
//! - `createModelServices(disposables)` é `create_language_model_services`, e `modelService.createModel(text,
//!   languageSelection, uri)` é `TextModel::new_with_language_service` com os mesmos serviços; `destroyModel` é o
//!   `dispose()` do modelo.
//! - `providers` (o `LanguageFeatureRegistry<SelectionRangeProvider>` do módulo, compartilhado entre os casos) é um
//!   registro novo por caso, que é o que a suíte observa: o caso que registra um provedor o descarta no fim.
//! - O provedor literal `{ provideSelectionRanges() { return [[...]]; } }` é [`StaticSelectionRangeProvider`].
//! - `assert.deepStrictEqual` de listas de texto e `assert.strictEqual` são `assert_eq!`; `Range.equalsRange(a, b)` é
//!   `a == b`; `new Range(...)` é `Range::new` e `String(range)` é o `to_string()` do `Range`.
//! - O `ranges.reverse()` de `assertGetRangesToPosition` (que inverte a lista recebida) é o `.iter().rev()`, porque a
//!   lista de entrada não é lida de novo depois.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::future::Future;
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::cancellation::CancellationToken;
use je_base::lifecycle::IDisposable;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_editor::config::editor_options::smart_select::SmartSelectOptions;
use je_editor::language_configuration::LanguageConfiguration;
use je_editor::language_configuration_registry::{ILanguageConfigurationService as _, LanguageConfigurationService};
use je_editor::language_feature_registry::LanguageFeatureRegistry;
use je_editor::language_providers::{ProviderResult, SelectionRange, SelectionRangeProvider};
use je_editor::language_selector::LanguageSelector;
use je_editor::language_service::{ILanguageService as _, LanguageService};
use je_editor::model::RelaxedTextModelCreationOptions;
use je_editor::modes_registry::LanguageExtensionPoint;
use je_editor::position::Position;
use je_editor::range::Range;
use je_editor::smart_select::bracket_selections::BracketSelectionRangeProvider;
use je_editor::smart_select::provide_selection_ranges;
use je_editor::smart_select::word_selections::WordSelectionRangeProvider;
use je_editor::text_model::{TextModel, TextModelServices};
use je_editor::undo_redo_service::UndoRedoService;
use support::on_enter_rules::javascript_on_enter_rules;
use support::re;
use support::test_text_model::{create_language_model_services, resolve_options};

/// `languageId`.
const LANGUAGE_ID: &str = "mockJSMode";

/// Os `let` da suíte `SmartSelect`.
struct Suite {
    undo_redo_service: Rc<UndoRedoService>,
    language_service: Rc<LanguageService>,
    language_configuration_service: Rc<LanguageConfigurationService>,
    providers: Rc<LanguageFeatureRegistry<dyn SelectionRangeProvider>>,
}

/// O que o mocha faz em volta de cada `test(...)` da suíte: `suiteSetup`, `setup`, o `ensureNoDisposablesAreLeaked...`,
/// o corpo (com o relógio virtual) e o `teardown` e o `suiteTeardown`, nessa ordem.
fn run_test<F: Future<Output = ()> + 'static>(body: impl FnOnce(Rc<Suite>) -> F) {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();

    // suiteSetup
    let original_max_duration = BracketSelectionRangeProvider::max_duration();
    BracketSelectionRangeProvider::set_max_duration(5000); // 5 seconds

    // setup
    let services = create_language_model_services(&ds);
    ds.add(services.language_service.register_language(LanguageExtensionPoint::new(LANGUAGE_ID)));
    ds.add(services.language_configuration_service.register(
        LANGUAGE_ID,
        LanguageConfiguration {
            brackets: Some(vec![
                ("(".to_owned(), ")".to_owned()),
                ("{".to_owned(), "}".to_owned()),
                ("[".to_owned(), "]".to_owned()),
            ]),
            on_enter_rules: Some(javascript_on_enter_rules()),
            word_pattern: Some(re(
                r##"(-?\d*\.\d\w*)|([^\`\~\!\@\#\$\%\^\&\*\(\)\=\+\[\{\]\}\\\;\:\'\"\,\.\<\>\/\?\s]+)"##,
                "g",
            )),
            ..LanguageConfiguration::default()
        },
        None,
    ));
    let suite = Rc::new(Suite {
        undo_redo_service: services.undo_redo_service.clone(),
        language_service: services.language_service.clone(),
        language_configuration_service: services.language_configuration_service.clone(),
        providers: Rc::new(LanguageFeatureRegistry::new(None)),
    });

    run_with_faked_timers(body(suite));

    // suiteTeardown
    BracketSelectionRangeProvider::set_max_duration(original_max_duration);
}

/// `modelService.createModel(text, new StaticLanguageSelector(languageId), uri)`.
fn create_model(suite: &Suite, text: &str, uri: URI) -> Rc<TextModel> {
    TextModel::new_with_language_service(
        text.into(),
        LANGUAGE_ID,
        resolve_options(&RelaxedTextModelCreationOptions::default()),
        Some(uri),
        suite.undo_redo_service.clone(),
        Some(TextModelServices {
            language_service: suite.language_service.clone(),
            language_configuration_service: suite.language_configuration_service.clone(),
        }),
    )
}

/// `assertGetRangesToPosition(text, lineNumber, column, ranges, selectLeadingAndTrailingWhitespace)`.
async fn assert_get_ranges_to_position(
    suite: &Suite,
    text: &[&str],
    line_number: i32,
    column: i32,
    ranges: &[Range],
    select_leading_and_trailing_whitespace: bool,
) {
    let model = create_model(suite, &text.join("\n"), URI::file("test.js", je_base::path::Platform::Posix));
    let options = SmartSelectOptions { select_leading_and_trailing_whitespace, select_subwords: true };
    let result = provide_selection_ranges(
        &suite.providers,
        &model,
        &[Position::new(line_number, column)],
        options,
        &CancellationToken::None,
    )
    .await
    .expect("os provedores não rejeitam");
    let actual = &result[0];
    let actual_str: Vec<String> = actual
        .iter()
        .map(|r| Range::new(r.start_line_number, r.start_column, r.end_line_number, r.end_column).to_string())
        .collect();
    let desired_str: Vec<String> = ranges.iter().rev().map(|r| r.to_string()).collect();

    assert_eq!(actual_str, desired_str, "\nA: {actual_str:?} VS \nE: {desired_str:?}");
    model.dispose();
}

#[test]
fn get_ranges_to_position_1() {
    run_test(|suite| async move {
        assert_get_ranges_to_position(
            &suite,
            &["function a(bar, foo){", "\tif (bar) {", "\t\treturn (bar + (2 * foo))", "\t}", "}"],
            3,
            20,
            &[
                Range::new(1, 1, 5, 2),  // all
                Range::new(1, 21, 5, 2), // {} outside
                Range::new(1, 22, 5, 1), // {} inside
                Range::new(2, 1, 4, 3),  // block
                Range::new(2, 1, 4, 3),
                Range::new(2, 2, 4, 3),
                Range::new(2, 11, 4, 3),
                Range::new(2, 12, 4, 2),
                Range::new(3, 1, 3, 27),  // line w/ triva
                Range::new(3, 3, 3, 27),  // line w/o triva
                Range::new(3, 10, 3, 27), // () outside
                Range::new(3, 11, 3, 26), // () inside
                Range::new(3, 17, 3, 26), // () outside
                Range::new(3, 18, 3, 25), // () inside
            ],
            true,
        )
        .await;
    });
}

#[test]
fn config_select_leading_and_trailing_whitespace() {
    run_test(|suite| async move {
        assert_get_ranges_to_position(
            &suite,
            &["aaa", "\tbbb", ""],
            2,
            3,
            &[
                Range::new(1, 1, 3, 1), // all
                Range::new(2, 1, 2, 5), // line w/ triva
                Range::new(2, 2, 2, 5), // bbb
            ],
            true,
        )
        .await;

        assert_get_ranges_to_position(
            &suite,
            &["aaa", "\tbbb", ""],
            2,
            3,
            &[
                Range::new(1, 1, 3, 1), // all
                Range::new(2, 2, 2, 5), // () inside
            ],
            false,
        )
        .await;
    });
}

#[test]
fn get_ranges_to_position_56886_skip_empty_lines_correctly() {
    run_test(|suite| async move {
        assert_get_ranges_to_position(
            &suite,
            &["function a(bar, foo){", "\tif (bar) {", "", "\t}", "}"],
            3,
            1,
            &[
                Range::new(1, 1, 5, 2),
                Range::new(1, 21, 5, 2),
                Range::new(1, 22, 5, 1),
                Range::new(2, 1, 4, 3),
                Range::new(2, 1, 4, 3),
                Range::new(2, 2, 4, 3),
                Range::new(2, 11, 4, 3),
                Range::new(2, 12, 4, 2),
            ],
            true,
        )
        .await;
    });
}

#[test]
fn get_ranges_to_position_56886_do_not_skip_lines_with_only_whitespaces() {
    run_test(|suite| async move {
        assert_get_ranges_to_position(
            &suite,
            &["function a(bar, foo){", "\tif (bar) {", " ", "\t}", "}"],
            3,
            1,
            &[
                Range::new(1, 1, 5, 2),  // all
                Range::new(1, 21, 5, 2), // {} outside
                Range::new(1, 22, 5, 1), // {} inside
                Range::new(2, 1, 4, 3),
                Range::new(2, 1, 4, 3),
                Range::new(2, 2, 4, 3),
                Range::new(2, 11, 4, 3),
                Range::new(2, 12, 4, 2),
                Range::new(3, 1, 3, 2), // block
                Range::new(3, 1, 3, 2), // empty line
            ],
            true,
        )
        .await;
    });
}

#[test]
fn get_ranges_to_position_40658_cursor_at_first_position_inside_brackets_should_select_line_inside() {
    run_test(|suite| async move {
        assert_get_ranges_to_position(
            &suite,
            &[" [ ]", " { } ", "( ) "],
            2,
            3,
            &[
                Range::new(1, 1, 3, 5),
                Range::new(2, 1, 2, 6), // line w/ triava
                Range::new(2, 2, 2, 5), // {} inside, line w/o triva
                Range::new(2, 3, 2, 4), // {} inside
            ],
            true,
        )
        .await;
    });
}

#[test]
fn get_ranges_to_position_40658_cursor_in_empty_brackets_should_reveal_brackets_first() {
    run_test(|suite| async move {
        assert_get_ranges_to_position(
            &suite,
            &[" [] ", " { } ", "  ( ) "],
            1,
            3,
            &[
                Range::new(1, 1, 3, 7), // all
                Range::new(1, 1, 1, 5), // line w/ trival
                Range::new(1, 2, 1, 4), // [] outside, line w/o trival
                Range::new(1, 3, 1, 3), // [] inside
            ],
            true,
        )
        .await;
    });
}

#[test]
fn get_ranges_to_position_40658_tokens_before_bracket_will_be_revealed_first() {
    run_test(|suite| async move {
        assert_get_ranges_to_position(
            &suite,
            &["  [] ", " { } ", "selectthis( ) "],
            3,
            11,
            &[
                Range::new(1, 1, 3, 15), // all
                Range::new(3, 1, 3, 15), // line w/ trivia
                Range::new(3, 1, 3, 14), // line w/o trivia
                Range::new(3, 1, 3, 11), // word
            ],
            true,
        )
        .await;
    });
}

// -- bracket selections

/// `assertRanges(provider, value, ...expected)`.
async fn assert_ranges(suite: &Suite, provider: &dyn SelectionRangeProvider, value: &str, expected: &[Range]) {
    let index = value.find('|').expect("o texto marca a posição com |");
    let value = value.replacen('|', "", 1); // CodeQL [SM02383] js/incomplete-sanitization this is purpose only the first | character

    let model = create_model(suite, &value, URI::parse("fake:lang", false));
    let pos = model.get_position_at(index as i64);
    let all = provider
        .provide_selection_ranges(&model, &[pos], &CancellationToken::None)
        .await
        .expect("o provedor não rejeita")
        .expect("o provedor devolve intervalos");
    let ranges = &all[0];

    model.dispose();

    assert_eq!(expected.len(), ranges.len());
    for (range, exp) in ranges.iter().zip(expected) {
        assert!(range.range == *exp, "A={} <> E={}", range.range, exp);
    }
}

#[test]
fn bracket_selection() {
    run_test(|suite| async move {
        assert_ranges(&suite, &BracketSelectionRangeProvider, "(|)", &[Range::new(1, 2, 1, 2), Range::new(1, 1, 1, 3)])
            .await;

        assert_ranges(
            &suite,
            &BracketSelectionRangeProvider,
            "[[[](|)]]",
            &[
                Range::new(1, 6, 1, 6),
                Range::new(1, 5, 1, 7), // ()
                Range::new(1, 3, 1, 7),
                Range::new(1, 2, 1, 8), // [[]()]
                Range::new(1, 2, 1, 8),
                Range::new(1, 1, 1, 9), // [[[]()]]
            ],
        )
        .await;

        assert_ranges(
            &suite,
            &BracketSelectionRangeProvider,
            "[a[](|)a]",
            &[Range::new(1, 6, 1, 6), Range::new(1, 5, 1, 7), Range::new(1, 2, 1, 8), Range::new(1, 1, 1, 9)],
        )
        .await;

        // no bracket
        assert_ranges(&suite, &BracketSelectionRangeProvider, "fofof|fofo", &[]).await;

        // empty
        assert_ranges(&suite, &BracketSelectionRangeProvider, "[[[]()]]|", &[]).await;
        assert_ranges(&suite, &BracketSelectionRangeProvider, "|[[[]()]]", &[]).await;

        // edge
        assert_ranges(
            &suite,
            &BracketSelectionRangeProvider,
            "[|[[]()]]",
            &[Range::new(1, 2, 1, 8), Range::new(1, 1, 1, 9)],
        )
        .await;
        assert_ranges(
            &suite,
            &BracketSelectionRangeProvider,
            "[[[]()]|]",
            &[Range::new(1, 2, 1, 8), Range::new(1, 1, 1, 9)],
        )
        .await;

        assert_ranges(
            &suite,
            &BracketSelectionRangeProvider,
            "aaa(aaa)bbb(b|b)ccc(ccc)",
            &[Range::new(1, 13, 1, 15), Range::new(1, 12, 1, 16)],
        )
        .await;
        assert_ranges(
            &suite,
            &BracketSelectionRangeProvider,
            "(aaa(aaa)bbb(b|b)ccc(ccc))",
            &[Range::new(1, 14, 1, 16), Range::new(1, 13, 1, 17), Range::new(1, 2, 1, 25), Range::new(1, 1, 1, 26)],
        )
        .await;
    });
}

#[test]
fn bracket_with_leading_trailing() {
    run_test(|suite| async move {
        assert_ranges(
            &suite,
            &BracketSelectionRangeProvider,
            "for(a of b){\n  foo(|);\n}",
            &[
                Range::new(2, 7, 2, 7),
                Range::new(2, 6, 2, 8),
                Range::new(1, 13, 3, 1),
                Range::new(1, 12, 3, 2),
                Range::new(1, 1, 3, 2),
                Range::new(1, 1, 3, 2),
            ],
        )
        .await;

        assert_ranges(
            &suite,
            &BracketSelectionRangeProvider,
            "for(a of b)\n{\n  foo(|);\n}",
            &[
                Range::new(3, 7, 3, 7),
                Range::new(3, 6, 3, 8),
                Range::new(2, 2, 4, 1),
                Range::new(2, 1, 4, 2),
                Range::new(1, 1, 4, 2),
                Range::new(1, 1, 4, 2),
            ],
        )
        .await;
    });
}

#[test]
fn in_word_ranges() {
    run_test(|suite| async move {
        assert_ranges(
            &suite,
            &WordSelectionRangeProvider::new(true),
            "f|ooBar",
            &[
                Range::new(1, 1, 1, 4), // foo
                Range::new(1, 1, 1, 7), // fooBar
                Range::new(1, 1, 1, 7), // doc
            ],
        )
        .await;

        assert_ranges(
            &suite,
            &WordSelectionRangeProvider::new(true),
            "f|oo_Ba",
            &[Range::new(1, 1, 1, 4), Range::new(1, 1, 1, 7), Range::new(1, 1, 1, 7)],
        )
        .await;

        assert_ranges(
            &suite,
            &WordSelectionRangeProvider::new(true),
            "f|oo-Ba",
            &[Range::new(1, 1, 1, 4), Range::new(1, 1, 1, 7), Range::new(1, 1, 1, 7)],
        )
        .await;
    });
}

#[test]
fn in_word_ranges_with_select_subwords_false() {
    run_test(|suite| async move {
        assert_ranges(
            &suite,
            &WordSelectionRangeProvider::new(false),
            "f|ooBar",
            &[Range::new(1, 1, 1, 7), Range::new(1, 1, 1, 7)],
        )
        .await;

        assert_ranges(
            &suite,
            &WordSelectionRangeProvider::new(false),
            "f|oo_Ba",
            &[Range::new(1, 1, 1, 7), Range::new(1, 1, 1, 7)],
        )
        .await;

        assert_ranges(
            &suite,
            &WordSelectionRangeProvider::new(false),
            "f|oo-Ba",
            &[Range::new(1, 1, 1, 7), Range::new(1, 1, 1, 7)],
        )
        .await;
    });
}

#[test]
fn default_selection_should_select_current_word_hump_first_in_camel_case_67493() {
    run_test(|suite| async move {
        let provider = WordSelectionRangeProvider::new(true);

        assert_ranges(
            &suite,
            &provider,
            "Abs|tractSmartSelect",
            &[Range::new(1, 1, 1, 9), Range::new(1, 1, 1, 20), Range::new(1, 1, 1, 20)],
        )
        .await;

        assert_ranges(
            &suite,
            &provider,
            "AbstractSma|rtSelect",
            &[Range::new(1, 9, 1, 14), Range::new(1, 1, 1, 20), Range::new(1, 1, 1, 20)],
        )
        .await;

        assert_ranges(
            &suite,
            &provider,
            "Abstrac-Sma|rt-elect",
            &[Range::new(1, 9, 1, 14), Range::new(1, 1, 1, 20), Range::new(1, 1, 1, 20)],
        )
        .await;

        assert_ranges(
            &suite,
            &provider,
            "Abstrac_Sma|rt_elect",
            &[Range::new(1, 9, 1, 14), Range::new(1, 1, 1, 20), Range::new(1, 1, 1, 20)],
        )
        .await;

        assert_ranges(
            &suite,
            &provider,
            "Abstrac_Sma|rt-elect",
            &[Range::new(1, 9, 1, 14), Range::new(1, 1, 1, 20), Range::new(1, 1, 1, 20)],
        )
        .await;

        assert_ranges(
            &suite,
            &provider,
            "Abstrac_Sma|rtSelect",
            &[Range::new(1, 9, 1, 14), Range::new(1, 1, 1, 20), Range::new(1, 1, 1, 20)],
        )
        .await;
    });
}

/// O provedor literal `{ provideSelectionRanges() { return [[...]]; } }`: devolve sempre os mesmos intervalos.
struct StaticSelectionRangeProvider(Vec<Vec<SelectionRange>>);

impl SelectionRangeProvider for StaticSelectionRangeProvider {
    fn provide_selection_ranges(
        &self,
        _model: &Rc<TextModel>,
        _positions: &[Position],
        _token: &CancellationToken,
    ) -> ProviderResult<Vec<Vec<SelectionRange>>> {
        Promise::resolved(Some(self.0.clone()))
    }
}

#[test]
fn smart_select_only_add_line_ranges_if_they_are_contained_by_the_next_range_73850() {
    run_test(|suite| async move {
        let reg = suite.providers.register(
            LanguageSelector::from("*"),
            Rc::new(StaticSelectionRangeProvider(vec![vec![
                SelectionRange { range: Range::new(1, 10, 1, 11) },
                SelectionRange { range: Range::new(1, 10, 3, 2) },
                SelectionRange { range: Range::new(1, 1, 3, 2) },
            ]])),
        );

        assert_get_ranges_to_position(
            &suite,
            &["type T = {", "\tx: number", "}"],
            1,
            10,
            &[
                Range::new(1, 1, 3, 2),   // all
                Range::new(1, 10, 3, 2),  // { ... }
                Range::new(1, 10, 1, 11), // {
            ],
            true,
        )
        .await;

        reg.dispose();
    });
}

#[test]
fn expand_selection_in_words_with_underscores_is_inconsistent_90589() {
    run_test(|suite| async move {
        let provider = WordSelectionRangeProvider::new(true);

        assert_ranges(
            &suite,
            &provider,
            "Hel|lo_World",
            &[Range::new(1, 1, 1, 6), Range::new(1, 1, 1, 12), Range::new(1, 1, 1, 12)],
        )
        .await;

        assert_ranges(
            &suite,
            &provider,
            "Hello_Wo|rld",
            &[Range::new(1, 7, 1, 12), Range::new(1, 1, 1, 12), Range::new(1, 1, 1, 12)],
        )
        .await;

        assert_ranges(
            &suite,
            &provider,
            "Hello|_World",
            &[Range::new(1, 1, 1, 6), Range::new(1, 1, 1, 12), Range::new(1, 1, 1, 12)],
        )
        .await;

        assert_ranges(
            &suite,
            &provider,
            "Hello_|World",
            &[Range::new(1, 7, 1, 12), Range::new(1, 1, 1, 12), Range::new(1, 1, 1, 12)],
        )
        .await;

        assert_ranges(
            &suite,
            &provider,
            "Hello|-World",
            &[Range::new(1, 1, 1, 6), Range::new(1, 1, 1, 12), Range::new(1, 1, 1, 12)],
        )
        .await;

        assert_ranges(
            &suite,
            &provider,
            "Hello-|World",
            &[Range::new(1, 7, 1, 12), Range::new(1, 1, 1, 12), Range::new(1, 1, 1, 12)],
        )
        .await;

        assert_ranges(
            &suite,
            &provider,
            "Hello|World",
            &[Range::new(1, 6, 1, 11), Range::new(1, 1, 1, 11), Range::new(1, 1, 1, 11)],
        )
        .await;
    });
}
