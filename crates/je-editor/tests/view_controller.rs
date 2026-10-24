//! Porte de `src/vs/editor/test/browser/view/viewController.test.ts` (suítes `ViewController - Bracket content
//! selection` e `ViewController - String content selection`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` e o `setup`/`teardown` (`DisposableStore`, `viewModel?.dispose()`) viram
//!   o `LeakGuard` criado no começo de cada caso, que descarta tudo ao sair (o `ViewModel` vai com o editor de teste).
//!   O carregamento do módulo `coreCommands` (que registra os comandos) acontece antes do guarda, como o `import`.
//! - `createCodeEditorServices(disposables)` e o `ILanguageService`/`ILanguageConfigurationService` que o modelo usa
//!   são `create_language_model_services` de `tests/support/test_text_model.rs`: o modelo é criado com as linguagens
//!   de verdade (`testMode` e `stringTestMode` registradas, com os colchetes `{}`, `[]`, `()`), que é o que
//!   `matchBracket` e a tokenização leem.
//! - `new ViewModel(1, configuration, model, ...)` é o `ViewModel` do editor de teste (`with_test_code_editor` com o
//!   modelo pronto): `CoreNavigationCommands.X.runCoreEditorCommand` do porte recebe o `EditorViewModel` do widget, e
//!   a `TestConfiguration({})` é a do editor de teste (opções padrão, as mesmas do original). O
//!   `ViewUserInputEvents(viewModel.coordinatesConverter)` e o delegado de comandos vazio são os do original.
//! - `TokenizationRegistry.register(languageId, support)` com `tokenizeEncoded` fixo é o [`FixedTokensSupport`], que
//!   devolve os pares `[startIndex, metadata]` de `lineTokens` para qualquer linha (`tokenize: undefined!` é o `unreachable!`).
//! - `testBracketSelection(..., expectedText: string | undefined)`: os oito casos passam um texto; o ramo
//!   `undefined` (`assert.notStrictEqual(selectedText, undefined)`) nunca é exercido e `getValueInRange` nunca
//!   devolve `undefined`, então o parâmetro é só o texto.
//! - O texto hebraico do caso de RTL usa escapes `\u{...}` (`שלום עולם`).

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite};
use je_base::JsString;
use je_base::lifecycle::to_disposable;
use je_editor::language_configuration_registry::ILanguageConfigurationService as _;
use je_editor::code_editor_widget::EditorViewModel;
use je_editor::core_commands::{NavigationCommandRevealType, core_navigation_commands};
use je_editor::encoded_token_attributes::{metadata_consts as mc, standard_token_type};
use je_editor::language_configuration::LanguageConfiguration;
use je_editor::language_service::ILanguageService;
use je_editor::languages::{EncodedTokenizationResult, ITokenizationSupport, State, TokenizationResult};
use je_editor::modes_registry::LanguageExtensionPoint;
use je_editor::null_tokenize::null_state;
use je_editor::position::Position;
use je_editor::text_model::TextModel;
use je_editor::tokenization_registry::tokenization_registry;
use je_editor::view_controller::{ICommandDelegate, MouseDispatchData, ViewController};
use je_editor::view_model_types::IViewModel;
use je_editor::view_user_input_events::ViewUserInputEvents;
use support::test_code_editor::{TestCodeEditorInstantiationOptions, TestEditorInput, with_test_code_editor};
use support::test_text_model::{LanguageModelServices, create_language_model_services, instantiate_text_model_with_languages};

/// O `{ paste: () => { }, type: () => { }, ... }` do original.
struct NoopCommandDelegate;

impl ICommandDelegate for NoopCommandDelegate {
    fn paste(&self, _text: &JsString, _paste_on_new_line: bool, _multicursor_text: Option<&[JsString]>, _mode: Option<&str>, _is_block: bool) {}
    fn type_text(&self, _text: &JsString) {}
    fn composition_type(&self, _text: &JsString, _replace_prev_char_cnt: i32, _replace_next_char_cnt: i32, _position_delta: i32) {}
    fn start_composition(&self) {}
    fn end_composition(&self) {}
    fn cut(&self) {}
}

fn brackets() -> Vec<(String, String)> {
    [("{", "}"), ("[", "]"), ("(", ")")].iter().map(|(open, close)| ((*open).to_owned(), (*close).to_owned())).collect()
}

/// `disposables.add(languageService.registerLanguage({ id }))` e `disposables.add(languageConfigurationService.register(id,
/// { brackets }))`.
fn register_test_language(ds: &LeakGuard, services: &LanguageModelServices, language_id: &str) {
    ds.add(services.language_service.register_language(LanguageExtensionPoint::new(language_id)));
    ds.add(services.language_configuration_service.register(
        language_id,
        LanguageConfiguration { brackets: Some(brackets()), ..Default::default() },
        None,
    ));
}

/// O `ViewController` que o original cria sobre o `viewModel`, com os eventos de usuário e o delegado vazio.
fn create_view_controller(view_model: &Rc<EditorViewModel>) -> ViewController<(), ()> {
    ViewController::new(
        view_model.clone(),
        Rc::new(ViewUserInputEvents::new(view_model.coordinates_converter())),
        Rc::new(NoopCommandDelegate),
    )
}

/// O `controller.dispatchMouse({ ..., mouseDownCount: 2, ... })` dos dois testes de seleção.
fn double_click_at(controller: &ViewController<(), ()>, position: Position) {
    controller.dispatch_mouse(&MouseDispatchData {
        position,
        mouse_column: position.column,
        started_on_line_numbers: false,
        reveal_type: NavigationCommandRevealType::Minimal,
        mouse_down_count: 2,
        in_selection_mode: false,
        alt_key: false,
        ctrl_key: false,
        meta_key: false,
        shift_key: false,
        left_button: true,
        middle_button: false,
        on_injected_text: false,
    });
}

/// `viewModel!.model.getValueInRange(viewModel!.getSelections()[0])`.
fn selected_text(view_model: &EditorViewModel, model: &TextModel) -> String {
    let selections = view_model.get_selections();
    model.get_value_in_range(&selections[0]).to_string_lossy()
}

mod view_controller_bracket_content_selection {
    use super::*;

    fn test_bracket_selection(text: &str, position: Position, expected_text: &str) {
        core_navigation_commands::instance();
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let language_id = "testMode";
        let services = create_language_model_services(&ds);
        register_test_language(&ds, &services, language_id);
        let model = instantiate_text_model_with_languages(&services, text, language_id);
        ds.add(to_disposable({ let model = model.clone(); move || model.dispose() }));

        with_test_code_editor(TestEditorInput::Model(model.clone()), TestCodeEditorInstantiationOptions::default(), |_, view_model, _| {
            let controller = create_view_controller(view_model);
            double_click_at(&controller, position);

            assert_eq!(selected_text(view_model, &model), expected_text);
        });
    }

    #[test]
    fn select_content_after_opening_curly_brace() {
        test_bracket_selection("var x = { hello };", Position::new(1, 10), " hello ");
    }

    #[test]
    fn select_content_before_closing_curly_brace() {
        test_bracket_selection("var x = { hello };", Position::new(1, 17), " hello ");
    }

    #[test]
    fn select_content_after_opening_parenthesis() {
        test_bracket_selection("function foo(arg1, arg2) {}", Position::new(1, 14), "arg1, arg2");
    }

    #[test]
    fn select_content_before_closing_parenthesis() {
        test_bracket_selection("function foo(arg1, arg2) {}", Position::new(1, 24), "arg1, arg2");
    }

    #[test]
    fn select_content_after_opening_square_bracket() {
        test_bracket_selection("const arr = [ 1, 2, 3 ];", Position::new(1, 14), " 1, 2, 3 ");
    }

    #[test]
    fn select_content_before_closing_square_bracket() {
        test_bracket_selection("const arr = [ 1, 2, 3 ];", Position::new(1, 23), " 1, 2, 3 ");
    }

    #[test]
    fn select_innermost_bracket_content_with_nested_brackets() {
        test_bracket_selection("var x = { a: { b: 123 }};", Position::new(1, 15), " b: 123 ");
    }

    #[test]
    fn empty_brackets_create_empty_selection() {
        test_bracket_selection("var x = {};", Position::new(1, 10), "");
    }
}

/// `TokenSpan`.
struct TokenSpan {
    start_index: u32,
    token_type: u32,
}

const fn span(start_index: u32, token_type: u32) -> TokenSpan {
    TokenSpan { start_index, token_type }
}

/// O `ITokenizationSupport` do `createViewControllerWithTokens`: os mesmos tokens para qualquer linha.
struct FixedTokensSupport {
    /// Os pares `[startIndex, metadata]` (`makeMetadata`).
    tokens: Vec<u32>,
}

impl ITokenizationSupport for FixedTokensSupport {
    fn get_initial_state(&self) -> State {
        null_state()
    }

    fn tokenize(&self, _line: &JsString, _has_eol: bool, _state: &State) -> TokenizationResult {
        unreachable!("`tokenize: undefined!` no original")
    }

    fn tokenize_encoded(&self, _line: &JsString, _has_eol: bool, state: &State) -> EncodedTokenizationResult {
        EncodedTokenizationResult { tokens: self.tokens.clone(), font_info: Vec::new(), end_state: state.clone() }
    }
}

mod view_controller_string_content_selection {
    use super::*;

    use standard_token_type::{OTHER, STRING};

    /// `createViewControllerWithTokens(text, lineTokens)` seguido do `doubleClickAt(controller, position)`: devolve o
    /// texto da primeira seleção.
    fn double_click_selection(text: &str, line_tokens: &[TokenSpan], position: Position) -> String {
        core_navigation_commands::instance();
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let language_id = "stringTestMode";
        let services = create_language_model_services(&ds);
        register_test_language(&ds, &services, language_id);

        let encoded_language_id = services.language_service.language_id_codec().encode_language_id(language_id);
        let tokens: Vec<u32> = line_tokens
            .iter()
            .flat_map(|token| {
                let metadata = (encoded_language_id << mc::LANGUAGEID_OFFSET) | (token.token_type << mc::TOKEN_TYPE_OFFSET);
                [token.start_index, metadata]
            })
            .collect();
        ds.add(tokenization_registry().register(language_id, Rc::new(FixedTokensSupport { tokens })));

        let model = instantiate_text_model_with_languages(&services, text, language_id);
        ds.add(to_disposable({ let model = model.clone(); move || model.dispose() }));

        model.tokenization().force_tokenization(1);

        let mut result = String::new();
        with_test_code_editor(TestEditorInput::Model(model.clone()), TestCodeEditorInstantiationOptions::default(), |_, view_model, _| {
            let controller = create_view_controller(view_model);
            double_click_at(&controller, position);
            result = selected_text(view_model, &model);
        });
        result
    }

    // -- Happy-path: whole string as a single token including quotes --

    #[test]
    fn select_string_content_clicking_right_after_opening_double_quote() {
        // Token layout: [0..8) Other  [8..15) String("hello")  [15..16) Other
        let text = "var x = \"hello\";";
        let tokens = [span(0, OTHER), span(8, STRING), span(15, OTHER)];
        // Column right after opening quote: offset 9 → column 10
        assert_eq!(double_click_selection(text, &tokens, Position::new(1, 10)), "hello");
    }

    #[test]
    fn select_string_content_clicking_at_closing_double_quote() {
        let text = "var x = \"hello\";";
        let tokens = [span(0, OTHER), span(8, STRING), span(15, OTHER)];
        // Column at closing quote: offset 14 → column 15
        assert_eq!(double_click_selection(text, &tokens, Position::new(1, 15)), "hello");
    }

    #[test]
    fn select_string_content_with_single_quotes() {
        let text = "var x = 'hello';";
        let tokens = [span(0, OTHER), span(8, STRING), span(15, OTHER)];
        assert_eq!(double_click_selection(text, &tokens, Position::new(1, 10)), "hello");
    }

    #[test]
    fn select_string_content_with_backtick_quotes() {
        let text = "var x = `hello`;";
        let tokens = [span(0, OTHER), span(8, STRING), span(15, OTHER)];
        assert_eq!(double_click_selection(text, &tokens, Position::new(1, 10)), "hello");
    }

    #[test]
    fn select_string_content_containing_escape_characters() {
        // Token layout: [0..8) Other  [8..22) String("hello\"world")  [22..23) Other
        let text = "var x = \"hello\\\"world\";";
        let tokens = [
            span(0, OTHER),
            span(8, STRING),
            span(9, STRING),
            span(14, STRING),
            span(16, STRING),
            span(21, STRING),
            span(22, OTHER),
        ];
        // Column right after opening quote: offset 9 → column 10
        assert_eq!(double_click_selection(text, &tokens, Position::new(1, 10)), "hello\\\"world");
    }

    // -- Click in middle of string should NOT select the whole string --

    #[test]
    fn click_in_middle_of_string_does_not_select_whole_string() {
        // Token layout: [0..8) Other  [8..21) String("hello world")  [21..22) Other
        let text = "var x = \"hello world\";";
        let tokens = [span(0, OTHER), span(8, STRING), span(21, OTHER)];
        // Click on 'w' in "world": word select should pick 'world', not 'hello world'
        assert_eq!(double_click_selection(text, &tokens, Position::new(1, 16)), "world");
    }

    // -- Bail-out: quotes as separate tokens (theme issue #292784) --

    #[test]
    fn separate_quote_tokens_fall_back_to_word_select() {
        let text = "var x = \"hello world\";";
        // Theme tokenizes quotes as separate Other tokens:
        // [0..8) Other  [8..9) Other(")  [9..20) String(hello world)  [20..21) Other(")  [21..22) Other
        let tokens = [
            span(0, OTHER),
            span(8, OTHER),  // opening "
            span(9, STRING), // hello world
            span(20, OTHER), // closing "
            span(21, OTHER),
        ];
        // The String token "hello world" doesn't start with a quote char: should bail out.
        // Click right after opening quote (column 10): word select picks just 'hello'.
        assert_eq!(double_click_selection(text, &tokens, Position::new(1, 10)), "hello");
    }

    // -- Bail-out: RTL content in string (#293384) --

    #[test]
    fn rtl_content_in_string_falls_back_to_word_select() {
        let text = "var x = \"\u{5E9}\u{5DC}\u{5D5}\u{5DD} \u{5E2}\u{5D5}\u{5DC}\u{5DD}\";";
        // Token layout: [0..8) Other  [8..19) String("שלום עולם")  [19..20) Other
        let tokens = [span(0, OTHER), span(8, STRING), span(19, OTHER)];
        // Should bail out due to RTL content: word select picks first word
        assert_eq!(double_click_selection(text, &tokens, Position::new(1, 10)), "\u{5E9}\u{5DC}\u{5D5}\u{5DD}");
    }

    // -- Bail-out: mismatched quotes (#293203, string split at braces) --

    #[test]
    fn string_token_without_matching_closing_quote_falls_back_to_word_select() {
        let text = "var x = \"a {} b\";";
        // Hypothetical tokenizer splits:
        // [0..8) Other  [8..11) String("a )  [11..13) Other({})  [13..17) String( b")  [17..18) Other
        let tokens = [
            span(0, OTHER),
            span(8, STRING),  // `"a ` starts with " but doesn't end with "
            span(11, OTHER),  // `{}`
            span(13, STRING), // ` b"` ends with " but doesn't start with "
            span(16, OTHER),
        ];
        // First String token starts with " but ends with space: bail out, word select picks 'a'
        assert_eq!(double_click_selection(text, &tokens, Position::new(1, 10)), "a");
    }
}
