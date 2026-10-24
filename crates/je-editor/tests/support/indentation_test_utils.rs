//! Os auxiliares que `indentation.test.ts` exporta e que `indentationLineProcessor.test.ts` importa
//! (`Language`, `registerLanguage`, `registerLanguageConfiguration`, `StandardTokenTypeData`,
//! `registerTokenizationSupport`), mais o que cada suíte repete no `setup`/`teardown` (o `LanguageService`, o
//! `TestLanguageConfigurationService` e a linguagem registrada) e o `createTextModel`/`withTestCodeEditor` que as suítes
//! usam com esses serviços.
//!
//! Notas de tradução:
//! - O `serviceCollection` com `[ILanguageService, languageService]` e `[ILanguageConfigurationService,
//!   languageConfigurationService]` é o `CodeEditorServices` de `create_code_editor_services`, cujo
//!   `ILanguageService` (`I_LANGUAGE_SERVICE`) e `TestLanguageConfigurationService` o editor de teste e os modelos usam
//!   (o `ILanguageConfigurationService` não tem identificador no porte, ver `test_code_editor.rs`). O que o
//!   `setup` registra vai para esses serviços, como em `tests/cursor.rs`.
//! - `IndentationFixture` é o conjunto dos `let` da suíte; [`run_test`] é o que o mocha faz em volta de cada
//!   `test(...)`: o `ensureNoDisposablesAreLeakedInTestSuite()`, o `setup`, o corpo e o `teardown`.
//! - `createTextModel(text, languageId, {})` é [`IndentationFixture::create_text_model`]: o modelo usa os serviços de
//!   linguagem da suíte e é descartado no `teardown`.
//! - `withTestCodeEditor(model, { autoIndent, serviceCollection }, callback)` é [`IndentationFixture::with_test_code_editor`]:
//!   o editor é descartado ao fim do callback, e o modelo (que é de quem chama) não.
//! - `viewModel.type(text, source)` e `viewModel.paste(text, true, undefined, 'keyboard')` são [`type_text`],
//!   [`type_keyboard`] e [`paste_from_keyboard`] (o texto vira UTF-16).
//! - O `registerTokenizationSupport` lê as `tokens` por um contador de linha (`lineIndex++`) a cada chamada de
//!   `tokenizeEncoded`, como o original; o `encodedLanguageId` é calculado uma vez no registro.

#![allow(dead_code)]

use std::cell::Cell;
use std::rc::Rc;

use je_base::JsString;
use je_base::dynamic::Dynamic;
use je_base::lifecycle::{DisposableRef, DisposableStore, IDisposable, to_disposable};
use je_editor::code_editor_widget::EditorViewModel;
use je_editor::encoded_token_attributes::metadata_consts;
use je_editor::language_configuration::{
    AutoClosingPairConditional, CommentRule, LanguageConfiguration, LineComment,
};
use je_editor::language_configuration_registry::ILanguageConfigurationService;
use je_editor::language_service::{ILanguageService, I_LANGUAGE_SERVICE};
use je_editor::languages::{EncodedTokenizationResult, ITokenizationSupport, State, TokenizationResult};
use je_editor::model::RelaxedTextModelCreationOptions;
use je_editor::modes_registry::LanguageExtensionPoint;
use je_editor::null_tokenize::null_state;
use je_editor::selection::Selection;
use je_editor::text_model::{TextModel, TextModelServices};
use je_editor::tokenization_registry::tokenization_registry;
use je_editor::view_model_types::IViewModel as _;

use crate::common::ensure_no_disposables_are_leaked_in_test_suite;
use crate::support::auto_closing_pairs_rules::{javascript_auto_closing_pairs_rules, latex_auto_closing_pairs_rules};
use crate::support::bracket_rules::{
    cpp_bracket_rules, go_bracket_rules, html_bracket_rules, latex_bracket_rules, lua_bracket_rules, php_bracket_rules,
    ruby_bracket_rules, typescript_bracket_rules, vb_bracket_rules,
};
use crate::support::indentation_rules::{
    go_indentation_rules, html_indentation_rules, javascript_indentation_rules, latex_indentation_rules,
    lua_indentation_rules, php_indentation_rules, ruby_indentation_rules, vb_indentation_rules,
};
use crate::support::on_enter_rules::{
    cpp_on_enter_rules, html_on_enter_rules, javascript_on_enter_rules, php_on_enter_rules, vb_on_enter_rules,
};
use crate::support::test_code_editor::{CodeEditorServices, TestCodeEditor, create_code_editor_services, instantiate_test_code_editor};
use crate::support::test_language_configuration_service::TestLanguageConfigurationService;
use crate::support::test_text_model::resolve_options;
use crate::support::u;

/// `Language`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Language {
    TypeScript,
    Ruby,
    Php,
    Go,
    Cpp,
    Html,
    Vb,
    Latex,
    Lua,
}

impl Language {
    /// O valor do `enum Language` do original (o id da linguagem).
    pub fn id(self) -> &'static str {
        match self {
            Language::TypeScript => "ts-test",
            Language::Ruby => "ruby-test",
            Language::Php => "php-test",
            Language::Go => "go-test",
            Language::Cpp => "cpp-test",
            Language::Html => "html-test",
            Language::Vb => "vb-test",
            Language::Latex => "latex-test",
            Language::Lua => "lua-test",
        }
    }
}

/// `registerLanguage(languageService, language)`.
pub fn register_language(language_service: &dyn ILanguageService, language: Language) -> DisposableRef {
    language_service.register_language(LanguageExtensionPoint::new(language.id()))
}

/// `registerLanguageConfiguration(languageConfigurationService, language)`.
pub fn register_language_configuration(
    language_configuration_service: &dyn ILanguageConfigurationService,
    language: Language,
) -> DisposableRef {
    let configuration = match language {
        Language::TypeScript => LanguageConfiguration {
            brackets: Some(typescript_bracket_rules()),
            comments: Some(CommentRule {
                line_comment: Some(LineComment::Token("//".to_owned())),
                block_comment: Some(("/*".to_owned(), "*/".to_owned())),
            }),
            auto_closing_pairs: Some(javascript_auto_closing_pairs_rules()),
            indentation_rules: Some(javascript_indentation_rules()),
            on_enter_rules: Some(javascript_on_enter_rules()),
            ..LanguageConfiguration::default()
        },
        Language::Ruby => LanguageConfiguration {
            brackets: Some(ruby_bracket_rules()),
            indentation_rules: Some(ruby_indentation_rules()),
            ..LanguageConfiguration::default()
        },
        Language::Php => LanguageConfiguration {
            brackets: Some(php_bracket_rules()),
            indentation_rules: Some(php_indentation_rules()),
            on_enter_rules: Some(php_on_enter_rules()),
            ..LanguageConfiguration::default()
        },
        Language::Go => LanguageConfiguration {
            brackets: Some(go_bracket_rules()),
            indentation_rules: Some(go_indentation_rules()),
            ..LanguageConfiguration::default()
        },
        Language::Cpp => LanguageConfiguration {
            brackets: Some(cpp_bracket_rules()),
            on_enter_rules: Some(cpp_on_enter_rules()),
            ..LanguageConfiguration::default()
        },
        Language::Html => LanguageConfiguration {
            brackets: Some(html_bracket_rules()),
            indentation_rules: Some(html_indentation_rules()),
            on_enter_rules: Some(html_on_enter_rules()),
            ..LanguageConfiguration::default()
        },
        Language::Vb => LanguageConfiguration {
            brackets: Some(vb_bracket_rules()),
            indentation_rules: Some(vb_indentation_rules()),
            on_enter_rules: Some(vb_on_enter_rules()),
            ..LanguageConfiguration::default()
        },
        Language::Latex => LanguageConfiguration {
            brackets: Some(latex_bracket_rules()),
            auto_closing_pairs: Some(
                latex_auto_closing_pairs_rules()
                    .iter()
                    .map(|pair| AutoClosingPairConditional::new(&pair.open, &pair.close))
                    .collect(),
            ),
            indentation_rules: Some(latex_indentation_rules()),
            ..LanguageConfiguration::default()
        },
        Language::Lua => LanguageConfiguration {
            brackets: Some(lua_bracket_rules()),
            indentation_rules: Some(lua_indentation_rules()),
            ..LanguageConfiguration::default()
        },
    };
    language_configuration_service.register(language.id(), configuration, None)
}

/// `StandardTokenTypeData`.
#[derive(Clone, Copy, Debug)]
pub struct StandardTokenTypeData {
    pub start_index: u32,
    /// Um valor de `standard_token_type`.
    pub standard_token_type: u32,
}

/// `{ startIndex, standardTokenType }`.
pub fn token(start_index: u32, standard_token_type: u32) -> StandardTokenTypeData {
    StandardTokenTypeData { start_index, standard_token_type }
}

/// O `tokenizationSupport` de `registerTokenizationSupport`.
struct LineIndexedTokenizationSupport {
    tokens: Vec<Vec<StandardTokenTypeData>>,
    /// `lineIndex`.
    line_index: Cell<usize>,
    encoded_language_id: u32,
}

impl ITokenizationSupport for LineIndexedTokenizationSupport {
    fn get_initial_state(&self) -> State {
        null_state()
    }

    fn tokenize(&self, _line: &JsString, _has_eol: bool, _state: &State) -> TokenizationResult {
        unreachable!("`tokenize: undefined!` no original")
    }

    fn tokenize_encoded(&self, _line: &JsString, _has_eol: bool, state: &State) -> EncodedTokenizationResult {
        let line_index = self.line_index.get();
        self.line_index.set(line_index + 1);
        let tokens_on_line = &self.tokens[line_index];
        let mut result: Vec<u32> = Vec::with_capacity(2 * tokens_on_line.len());
        for token in tokens_on_line {
            result.push(token.start_index);
            result.push(
                (self.encoded_language_id << metadata_consts::LANGUAGEID_OFFSET)
                    | (token.standard_token_type << metadata_consts::TOKEN_TYPE_OFFSET),
            );
        }
        EncodedTokenizationResult { tokens: result, font_info: Vec::new(), end_state: state.clone() }
    }
}

/// `registerTokenizationSupport(instantiationService, tokens, languageId)`.
pub fn register_tokenization_support(
    language_service: &dyn ILanguageService,
    tokens: Vec<Vec<StandardTokenTypeData>>,
    language: Language,
) -> DisposableRef {
    let encoded_language_id = language_service.language_id_codec().encode_language_id(language.id());
    tokenization_registry().register(
        language.id(),
        Rc::new(LineIndexedTokenizationSupport { tokens, line_index: Cell::new(0), encoded_language_id }),
    )
}

/// Os `let` de uma suíte (`disposables`, `serviceCollection`, `languageService`, `languageConfigurationService`) e a
/// linguagem do `languageId`.
pub struct IndentationFixture {
    pub disposables: Rc<DisposableStore>,
    pub services: Rc<CodeEditorServices>,
    pub language_service: Rc<dyn ILanguageService>,
    pub language_configuration_service: Rc<TestLanguageConfigurationService>,
    pub language: Language,
}

impl IndentationFixture {
    /// O `setup` da suíte.
    pub fn new(language: Language) -> Self {
        let disposables = DisposableStore::new();
        let services = create_code_editor_services(&disposables, None);
        let language_service = services.get(I_LANGUAGE_SERVICE);
        let language_configuration_service = services.language_configuration_service.clone();
        disposables.add(register_language(&*language_service, language));
        disposables.add(register_language_configuration(&*language_configuration_service, language));
        Self { disposables, services, language_service, language_configuration_service, language }
    }

    /// O `teardown` da suíte.
    pub fn teardown(&self) {
        self.disposables.dispose();
    }

    /// O `languageId` da suíte.
    pub fn language_id(&self) -> &'static str {
        self.language.id()
    }

    /// `createTextModel(text, languageId, {})` seguido de `disposables.add(model)`.
    pub fn create_text_model(&self, text: &str) -> Rc<TextModel> {
        let model = TextModel::new_with_language_service(
            text.into(),
            self.language_id(),
            resolve_options(&RelaxedTextModelCreationOptions::default()),
            None,
            self.services.undo_redo_service.clone(),
            Some(TextModelServices {
                language_service: self.language_service.clone(),
                language_configuration_service: self.language_configuration_service.clone(),
            }),
        );
        let owned = model.clone();
        self.disposables.add(to_disposable(move || owned.dispose()));
        model
    }

    /// `withTestCodeEditor(model, { autoIndent, serviceCollection }, callback)`.
    pub fn with_test_code_editor(
        &self,
        model: &Rc<TextModel>,
        auto_indent: &str,
        callback: impl FnOnce(&Rc<TestCodeEditor>, &Rc<EditorViewModel>, &Rc<CodeEditorServices>),
    ) {
        let options = Dynamic::object([("autoIndent", Dynamic::from(auto_indent))]);
        let editor = instantiate_test_code_editor(&self.services, Some(model.clone()), &options);
        let view_model = editor.get_view_model().expect("o editor de teste tem o modelo ligado");
        view_model.set_has_focus(true);
        callback(&editor, &view_model, &self.services);
        editor.dispose();
    }

    /// `disposables.add(registerTokenizationSupport(instantiationService, tokens, languageId))`.
    pub fn register_tokenization_support(&self, tokens: Vec<Vec<StandardTokenTypeData>>) {
        self.disposables.add(register_tokenization_support(&*self.language_service, tokens, self.language));
    }
}

/// O que o mocha faz em volta de cada `test(...)` das suítes de uma linguagem.
pub fn run_test(language: Language, body: impl FnOnce(&IndentationFixture)) {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let fixture = IndentationFixture::new(language);
    body(&fixture);
    fixture.teardown();
}

/// `viewModel.type(text)`.
pub fn type_text(view_model: &EditorViewModel, text: &str) {
    view_model.r#type(&u(text), None);
}

/// `viewModel.type(text, 'keyboard')`.
pub fn type_keyboard(view_model: &EditorViewModel, text: &str) {
    view_model.r#type(&u(text), Some("keyboard"));
}

/// `viewModel.paste(text, true, undefined, 'keyboard')`.
pub fn paste_from_keyboard(view_model: &EditorViewModel, text: &str) {
    view_model.paste(&u(text), true, None, Some("keyboard"), false);
}

/// `model.getValue()`.
pub fn value_of(model: &TextModel) -> String {
    model.get_value().to_string_lossy()
}

/// `editor.setSelection(new Selection(a, b, c, d))`.
pub fn set_selection(editor: &TestCodeEditor, start_line_number: i32, start_column: i32, end_line_number: i32, end_column: i32) {
    use je_editor::code_editor_widget::ICodeEditor;
    editor.set_selection(Selection::new(start_line_number, start_column, end_line_number, end_column).into(), None);
}
