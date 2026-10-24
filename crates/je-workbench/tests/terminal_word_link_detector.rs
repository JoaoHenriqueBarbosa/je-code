//! Porte de `src/vs/workbench/contrib/terminalContrib/links/test/browser/terminalWordLinkDetector.test.ts` (suíte `Workbench - TerminalWordLinkDetector`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`setup` e `ensureNoDisposablesAreLeakedInTestSuite()`.** O [`Fixture`] cria o guarda de vazamentos antes de tudo, o
//!   `TestConfigurationService` com `terminal.integrated.wordSeparators` vazio, o `TestProductService`, o `FakeXterm` (80 colunas
//!   por 30 linhas, no lugar do `Terminal` do xterm) e o detector, e registra tudo no guarda.
//! - **`configurationService.setUserConfiguration(...)` e `onDidChangeConfigurationEmitter.fire({ affectsConfiguration: () => true })`.**
//!   É [`Fixture::set_word_separators`]: grava a configuração e dispara o `ConfigurationChangeEvent` real que afeta
//!   `terminal.integrated.wordSeparators` (o literal do original afirma tudo, e o detector só pergunta por essa chave).
//! - **`assertLink`.** Delega ao `assert_link_helper` de `tests/support/terminal_link_support.rs` (o `assertLinkHelper` de
//!   `linkTestUtils.ts`).
//! - **`for (let i = 0xe0b0; i <= 0xe0bf; i++) test(...)`.** O macro `powerline_test!` gera um `#[test]` por iteração.
//! - **Casos fora do porte.** `should support wide characters` é `test.skip` no original (o `FakeXterm` também não tem caracteres
//!   largos); sem `#[test]` (o `test.skip` do original nem roda).

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod configuration_event_support;
mod support;

use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::dynamic::Dynamic;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_workbench::contrib::terminal_contrib::links::links::TerminalBuiltinLinkType;
use je_workbench::contrib::terminal_contrib::links::terminal_word_link_detector::TerminalWordLinkDetector;
use je_workbench::test::common::workbench_test_services::TestProductService;
use support::fake_xterm::FakeXterm;
use support::terminal_link_support::assert_link_helper;

/// O `setup` da suíte.
struct Fixture {
    configuration_service: Rc<TestConfigurationService>,
    xterm: Rc<FakeXterm>,
    detector: Rc<TerminalWordLinkDetector>,
    // Declarado por último: o `Drop` dele descarta o que o `Fixture` registrou e confere os vazamentos.
    _guard: LeakGuard,
}

/// `{ integrated: { wordSeparators } }`.
fn terminal_configuration(word_separators: &str) -> Dynamic {
    Dynamic::object([("integrated", Dynamic::object([("wordSeparators", Dynamic::string(word_separators))]))])
}

impl Fixture {
    fn new() -> Self {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let configuration_service = TestConfigurationService::new(None);
        let _ = configuration_service.set_user_configuration("terminal", terminal_configuration(""), None);
        let product_service = Rc::new(TestProductService::default());
        let xterm = guard.add_rc(FakeXterm::new(80, 30));
        let detector = guard.add_rc(TerminalWordLinkDetector::new(xterm.clone(), configuration_service.clone(), product_service));
        Self { configuration_service, xterm, detector, _guard: guard }
    }

    /// `setUserConfiguration('terminal', { integrated: { wordSeparators } })` e o disparo do evento de mudança.
    fn set_word_separators(&self, word_separators: &str) {
        let _ = self.configuration_service.set_user_configuration("terminal", terminal_configuration(word_separators), None);
        configuration_event_support::fire_configuration_change(&self.configuration_service, &["terminal.integrated.wordSeparators"]);
    }

    /// `assertLink(text, expected)`.
    async fn assert_link(&self, text: &str, expected: &[(&str, [(i32, i32); 2])]) {
        let expected: Vec<(String, [(i32, i32); 2])> = expected.iter().map(|(link, range)| ((*link).to_owned(), *range)).collect();
        assert_link_helper(&self.xterm, self.detector.as_ref(), text, &expected, TerminalBuiltinLinkType::Search).await;
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// suite('should link words as defined by wordSeparators')
// ---------------------------------------------------------------------------------------------------------------------

#[test]
fn should_link_words_as_defined_by_word_separators_space_parens_brackets() {
    run_with_faked_timers(async {
        let f = Fixture::new();
        f.set_word_separators(" ()[]");
        f.assert_link("foo", &[("foo", [(1, 1), (3, 1)])]).await;
        f.assert_link(" foo ", &[("foo", [(2, 1), (4, 1)])]).await;
        f.assert_link("(foo)", &[("foo", [(2, 1), (4, 1)])]).await;
        f.assert_link("[foo]", &[("foo", [(2, 1), (4, 1)])]).await;
        f.assert_link("{foo}", &[("{foo}", [(1, 1), (5, 1)])]).await;
    });
}

#[test]
fn should_link_words_as_defined_by_word_separators_space() {
    run_with_faked_timers(async {
        let f = Fixture::new();
        f.set_word_separators(" ");
        f.assert_link("foo", &[("foo", [(1, 1), (3, 1)])]).await;
        f.assert_link(" foo ", &[("foo", [(2, 1), (4, 1)])]).await;
        f.assert_link("(foo)", &[("(foo)", [(1, 1), (5, 1)])]).await;
        f.assert_link("[foo]", &[("[foo]", [(1, 1), (5, 1)])]).await;
        f.assert_link("{foo}", &[("{foo}", [(1, 1), (5, 1)])]).await;
    });
}

#[test]
fn should_link_words_as_defined_by_word_separators_space_brackets() {
    run_with_faked_timers(async {
        let f = Fixture::new();
        f.set_word_separators(" []");
        f.assert_link("aabbccdd.txt ", &[("aabbccdd.txt", [(1, 1), (12, 1)])]).await;
        f.assert_link(" aabbccdd.txt ", &[("aabbccdd.txt", [(2, 1), (13, 1)])]).await;
        f.assert_link(" [aabbccdd.txt] ", &[("aabbccdd.txt", [(3, 1), (14, 1)])]).await;
    });
}

// ---------------------------------------------------------------------------------------------------------------------
// suite('should ignore powerline symbols')
// ---------------------------------------------------------------------------------------------------------------------

/// `test(`\\u${i.toString(16)}`)`: um `#[test]` por símbolo de `0xe0b0` a `0xe0bf`.
macro_rules! powerline_test {
    ($name:ident, $code:expr) => {
        #[test]
        fn $name() {
            run_with_faked_timers(async {
                let f = Fixture::new();
                let symbol = char::from_u32($code).expect("a faixa powerline é BMP");
                f.assert_link(&format!("{symbol}foo{symbol}"), &[("foo", [(2, 1), (4, 1)])]).await;
            });
        }
    };
}

powerline_test!(should_ignore_powerline_symbols_u_e0b0, 0xe0b0);
powerline_test!(should_ignore_powerline_symbols_u_e0b1, 0xe0b1);
powerline_test!(should_ignore_powerline_symbols_u_e0b2, 0xe0b2);
powerline_test!(should_ignore_powerline_symbols_u_e0b3, 0xe0b3);
powerline_test!(should_ignore_powerline_symbols_u_e0b4, 0xe0b4);
powerline_test!(should_ignore_powerline_symbols_u_e0b5, 0xe0b5);
powerline_test!(should_ignore_powerline_symbols_u_e0b6, 0xe0b6);
powerline_test!(should_ignore_powerline_symbols_u_e0b7, 0xe0b7);
powerline_test!(should_ignore_powerline_symbols_u_e0b8, 0xe0b8);
powerline_test!(should_ignore_powerline_symbols_u_e0b9, 0xe0b9);
powerline_test!(should_ignore_powerline_symbols_u_e0ba, 0xe0ba);
powerline_test!(should_ignore_powerline_symbols_u_e0bb, 0xe0bb);
powerline_test!(should_ignore_powerline_symbols_u_e0bc, 0xe0bc);
powerline_test!(should_ignore_powerline_symbols_u_e0bd, 0xe0bd);
powerline_test!(should_ignore_powerline_symbols_u_e0be, 0xe0be);
powerline_test!(should_ignore_powerline_symbols_u_e0bf, 0xe0bf);

// ---------------------------------------------------------------------------------------------------------------------
// Casos soltos da suíte principal
// ---------------------------------------------------------------------------------------------------------------------

#[test]
fn should_support_multiple_link_results() {
    run_with_faked_timers(async {
        let f = Fixture::new();
        f.set_word_separators(" ");
        f.assert_link("foo bar", &[("foo", [(1, 1), (3, 1)]), ("bar", [(5, 1), (7, 1)])]).await;
    });
}

#[test]
fn should_remove_trailing_colon_in_the_link_results() {
    run_with_faked_timers(async {
        let f = Fixture::new();
        f.set_word_separators(" ");
        f.assert_link("foo:5:6: bar:0:32:", &[("foo:5:6", [(1, 1), (7, 1)]), ("bar:0:32", [(10, 1), (17, 1)])]).await;
    });
}

#[test]
fn should_support_wrapping() {
    run_with_faked_timers(async {
        let f = Fixture::new();
        f.set_word_separators(" ");
        let word = "fsdjfsdkfjslkdfjskdfjsldkfjsdlkfjslkdjfskldjflskdfjskldjflskdfjsdklfjsdklfjsldkfjsdlkfjsdlkfjsdlkfjsldkfjslkdfjsdlkfjsldkfjsdlkfjskdfjsldkfjsdlkfjslkdfjsdlkfjsldkfjsldkfjsldkfjslkdfjsdlkfjslkdfjsdklfsd";
        f.assert_link(word, &[(word, [(1, 1), (41, 3)])]).await;
    });
}

#[test]
fn should_support_wrapping_with_multiple_links() {
    run_with_faked_timers(async {
        let f = Fixture::new();
        f.set_word_separators(" ");
        f.assert_link(
            "fsdjfsdkfjslkdfjskdfjsldkfj sdlkfjslkdjfskldjflskdfjskldjflskdfj sdklfjsdklfjsldkfjsdlkfjsdlkfjsdlkfjsldkfjslkdfjsdlkfjsldkfjsdlkfjskdfjsldkfjsdlkfjslkdfjsdlkfjsldkfjsldkfjsldkfjslkdfjsdlkfjslkdfjsdklfsd",
            &[
                ("fsdjfsdkfjslkdfjskdfjsldkfj", [(1, 1), (27, 1)]),
                ("sdlkfjslkdjfskldjflskdfjskldjflskdfj", [(29, 1), (64, 1)]),
                (
                    "sdklfjsdklfjsldkfjsdlkfjsdlkfjsdlkfjsldkfjslkdfjsdlkfjsldkfjsdlkfjskdfjsldkfjsdlkfjslkdfjsdlkfjsldkfjsldkfjsldkfjslkdfjsdlkfjslkdfjsdklfsd",
                    [(66, 1), (43, 3)],
                ),
            ],
        )
        .await;
    });
}

#[test]
fn does_not_return_any_links_for_empty_text() {
    run_with_faked_timers(async {
        let f = Fixture::new();
        f.set_word_separators(" ");
        f.assert_link("", &[]).await;
    });
}

#[test]
fn should_support_file_scheme_links() {
    run_with_faked_timers(async {
        let f = Fixture::new();
        f.set_word_separators(" ");
        f.assert_link("file:///C:/users/test/file.txt ", &[("file:///C:/users/test/file.txt", [(1, 1), (30, 1)])]).await;
        f.assert_link("file:///C:/users/test/file.txt:1:10 ", &[("file:///C:/users/test/file.txt:1:10", [(1, 1), (35, 1)])]).await;
    });
}
