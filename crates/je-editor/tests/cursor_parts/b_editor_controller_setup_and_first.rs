// imports do fragmento B:
use std::cell::Cell;
use std::rc::Rc;

use je_base::JsString;
use je_base::dynamic::Dynamic;
use je_base::lifecycle::IDisposable as _;
use je_editor::code_editor_widget::{ExecuteEditsSource, ICodeEditor};
use je_editor::core_commands::core_editing_commands;
use je_editor::core_commands::core_navigation_commands;
use je_editor::cursor_common::CursorSimpleModel;
use je_editor::edit_operation::{EditOperation, SingleEditOperation};
use je_editor::language_configuration::{AutoClosingPair, IndentAction, LanguageConfiguration};
use je_editor::languages::{EncodedTokenizationResult, ITokenizationSupport, State, TokenizationResult};
use je_editor::model::{EndOfLineSequence, RelaxedTextModelCreationOptions, ValidEditOperation};
use je_editor::null_tokenize::null_state;
use je_editor::position::Position;
use je_editor::range::Range;
use je_editor::selection::Selection;
use je_editor::tokenization_registry::tokenization_registry;
use support::test_view_model::no_options;
use support::u;

use editor_controller::*;

/// A suíte `Editor Controller`: o `setup`, o `teardown` e os auxiliares internos da suíte (`setupOnEnterLanguage`,
/// `setupIndentRulesLanguage`, `setupAutoClosingLanguage`, `createTextModel`, `withTestCodeEditor`, `usingCursor`,
/// `assertType`...), compartilhados com os outros fragmentos.
///
/// Notas de tradução:
/// - O `let disposables/instantiationService/languageConfigurationService/languageService` da suíte são os campos de
///   [`EditorControllerFixture`]; o `setup` é [`EditorControllerFixture::new`] e o `teardown` é
///   [`EditorControllerFixture::teardown`]. [`run_test`] é o que o mocha faz em volta de cada `test(...)`: o carregamento
///   do módulo `coreCommands`, o `ensureNoDisposablesAreLeakedInTestSuite()` (o `LeakGuard`), o `setup`, o corpo e o
///   `teardown`, nessa ordem.
/// - Os auxiliares que leem os `let` da suíte são métodos da fixture. Os que não leem (`extractAutoClosingSpecialColumns`,
///   `assertType`) são funções livres do módulo.
/// - Os parâmetros com valor padrão do original (`languageId = null`, `options = DEFAULT_CREATION_OPTIONS`, `uri = null`)
///   são `Option`: `None` é o padrão. `{}` de opções do editor é `&no_options()`.
/// - `editor.runCommand(CoreEditingCommands.X, null)` é `editor.run_command(&*commands.x, None)`: o porte dos comandos de
///   edição vira `ITestEditorCommand` aqui (as duas implementações abaixo, uma para os `CoreEditingCommand` e outra para
///   `Undo`/`Redo`), porque o `runCommand` do editor de teste chama `command.runEditorCommand(accessor, this, args)`.
/// - `viewModel.type(text, source)` é [`type_text`] (o texto vira UTF-16) e `viewModel.setSelections('test', ...)` é
///   [`set_test_selections`] (a fonte `test` e o motivo `NotSet`, o padrão do original).
pub mod editor_controller {
    use std::any::Any;
    use std::rc::Rc;

    use je_base::dynamic::Dynamic;
    use je_base::lifecycle::{DisposableStore, IDisposable, to_disposable};
    use je_base::promise::MaybePromise;
    use je_base::uri::URI;
    use je_base::JsString;
    use je_editor::code_editor_widget::{CodeEditorWidget, EditorViewModel, IEditorCommand};
    use je_editor::core_commands::core_editing_commands::{CoreEditingCommand, UndoRedoCommand};
    use je_editor::core_commands::{
        CoreEditorCommand, EditorOrNativeTextInputCommand, MoveCommandOptions, register_core_commands,
    };
    use je_editor::cursor_common::CursorSimpleModel;
    use je_editor::cursor_events::CursorChangeReason;
    use je_editor::encoded_token_attributes::{metadata_consts, standard_token_type};
    use je_editor::language_configuration::{
        AutoClosingPair, AutoClosingPairConditional, CommentRule, DocComment, ElectricCharacterSupportConfig, EnterAction,
        IndentAction, IndentationRule, LanguageConfiguration, OnEnterRule,
    };
    use je_editor::language_configuration_registry::ILanguageConfigurationService;
    use je_editor::language_service::{ILanguageService, I_LANGUAGE_SERVICE};
    use je_editor::languages::{EncodedTokenizationResult, IState, ITokenizationSupport, State, TokenizationResult};
    use je_editor::model::{EndOfLinePreference, RelaxedTextModelCreationOptions};
    use je_editor::modes_registry::LanguageExtensionPoint;
    use je_editor::position::Position;
    use je_editor::selection::Selection;
    use je_editor::text_model::{TextModel, TextModelServices, TextSource};
    use je_editor::tokenization_registry::tokenization_registry;
    use je_instantiation::ServicesAccessor;

    use super::move_to;
    use crate::common::ensure_no_disposables_are_leaked_in_test_suite;
    use crate::support::re;
    use crate::support::test_code_editor::{
        CodeEditorServices, ITestEditorCommand, TestCodeEditor, TestCodeEditorCreationOptions, TestEditorInput,
        create_code_editor_services, instantiate_test_code_editor,
    };
    use crate::support::test_language_configuration_service::TestLanguageConfigurationService;
    use crate::support::test_text_model::{PLAINTEXT_LANGUAGE_ID, resolve_options};
    use crate::support::u;

    pub const SURROUNDING_LANGUAGE_ID: &str = "surroundingLanguage";
    pub const INDENT_RULES_LANGUAGE_ID: &str = "indentRulesLanguage";
    pub const ELECTRIC_CHAR_LANGUAGE_ID: &str = "electricCharLanguage";
    pub const AUTO_CLOSING_LANGUAGE_ID: &str = "autoClosingLanguage";
    pub const EMPTY_CLOSING_SURROUND_LANGUAGE_ID: &str = "emptyClosingSurroundLanguage";

    /// O `[]` de linhas do `withTestCodeEditor` local (`string[]`): um vetor de tamanho fixo de linhas.
    impl<const N: usize> From<[&str; N]> for TestEditorInput {
        fn from(lines: [&str; N]) -> Self {
            TestEditorInput::Lines(lines.iter().map(|line| (*line).to_owned()).collect())
        }
    }

    /// `CoreEditingCommands.X` como argumento de `editor.runCommand(command, args)`: o `runEditorCommand` do
    /// `EditorCommand`, com o `args` ausente (`null`) valendo `{}` dentro do comando.
    impl ITestEditorCommand for CoreEditingCommand {
        fn run_editor_command(
            &self,
            accessor: &ServicesAccessor,
            editor: &Rc<CodeEditorWidget>,
            args: Option<&Dynamic>,
        ) -> MaybePromise<()> {
            let args = args.cloned().unwrap_or(Dynamic::Null);
            MaybePromise::Promise(IEditorCommand::run_editor_command(self, Some(accessor), editor, &args))
        }
    }

    /// `CoreEditingCommands.Undo` e `CoreEditingCommands.Redo` como argumento de `editor.runCommand(command, args)`: o
    /// `runEditorCommand` do `EditorOrNativeTextInputCommand`.
    impl ITestEditorCommand for UndoRedoCommand {
        fn run_editor_command(
            &self,
            accessor: &ServicesAccessor,
            editor: &Rc<CodeEditorWidget>,
            args: Option<&Dynamic>,
        ) -> MaybePromise<()> {
            let args = args.cloned().unwrap_or(Dynamic::Null);
            EditorOrNativeTextInputCommand::run_editor_command(self, Some(accessor), editor, &args)
        }
    }

    /// `ICursorOpts`.
    #[derive(Default)]
    pub struct CursorOpts<'a> {
        pub text: Vec<&'a str>,
        pub language_id: Option<&'a str>,
        pub model_opts: Option<RelaxedTextModelCreationOptions>,
        pub editor_opts: Option<Dynamic>,
    }

    /// `AutoClosingColumnType`.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum AutoClosingColumnType {
        Normal = 0,
        Special1 = 1,
        Special2 = 2,
    }

    /// Os `let` da suíte `Editor Controller`.
    pub struct EditorControllerFixture {
        pub disposables: Rc<DisposableStore>,
        pub instantiation_service: Rc<CodeEditorServices>,
        pub language_configuration_service: Rc<TestLanguageConfigurationService>,
        pub language_service: Rc<dyn ILanguageService>,
    }

    /// As regras de indentação da `indentRulesLanguage`.
    fn indent_rules_language_indentation_rules() -> IndentationRule {
        IndentationRule {
            decrease_indent_pattern: re(
                r##"^\s*((?!\S.*\/[*]).*[*]\/\s*)?[})\]]|^\s*(case\b.*|default):\s*(\/\/.*|\/[*].*[*]\/\s*)?$"##,
                "",
            ),
            increase_indent_pattern: re(
                r##"^((?!\/\/).)*(\{[^}"'`]*|\([^)"']*|\[[^\]"']*|^\s*(\{\}|\(\)|\[\]|(case\b.*|default):))\s*(\/\/.*|\/[*].*[*]\/\s*)?$"##,
                "",
            ),
            indent_next_line_pattern: Some(re(
                r##"^\s*(for|while|if|else)\b(?!.*[;{}]\s*(\/\/.*|\/[*].*[*]\/\s*)?$)"##,
                "",
            )),
            un_indented_line_pattern: Some(re(
                r##"^(?!.*([;{}]|\S:)\s*(\/\/.*|\/[*].*[*]\/\s*)?$)(?!.*(\{[^}"']*|\([^)"']*|\[[^\]"']*|^\s*(\{\}|\(\)|\[\]|(case\b.*|default):))\s*(\/\/.*|\/[*].*[*]\/\s*)?$)(?!^\s*((?!\S.*\/[*]).*[*]\/\s*)?[})\]]|^\s*(case\b.*|default):\s*(\/\/.*|\/[*].*[*]\/\s*)?$)(?!^\s*(for|while|if|else)\b(?!.*[;{}]\s*(\/\/.*|\/[*].*[*]\/\s*)?$))"##,
                "",
            )),
        }
    }

    /// O `__electricCharacterSupport: { docComment: { open: '/**', close: ' */' } }` das linguagens `electricCharLanguage` e
    /// `autoClosingLanguage`.
    fn doc_comment_electric_character_support() -> Option<ElectricCharacterSupportConfig> {
        Some(ElectricCharacterSupportConfig {
            doc_comment: Some(DocComment { open: "/**".to_owned(), close: Some(" */".to_owned()) }),
        })
    }

    /// Os pares de fechamento automático que `setupAutoClosingLanguage` e `setAutoClosingLanguageEnabledSet` têm em comum
    /// (tudo menos o par `begin`/`end`).
    fn common_auto_closing_pairs() -> Vec<AutoClosingPairConditional> {
        vec![
            AutoClosingPairConditional::new("{", "}"),
            AutoClosingPairConditional::new("[", "]"),
            AutoClosingPairConditional::new("(", ")"),
            AutoClosingPairConditional::new("'", "'").not_in(&["string", "comment"]),
            AutoClosingPairConditional::new("\"", "\"").not_in(&["string"]),
            AutoClosingPairConditional::new("`", "`").not_in(&["string", "comment"]),
            AutoClosingPairConditional::new("/**", " */").not_in(&["string"]),
        ]
    }

    impl EditorControllerFixture {
        /// O `setup` da suíte.
        pub fn new() -> Self {
            let disposables = DisposableStore::new();
            let instantiation_service = create_code_editor_services(&disposables, None);
            let language_configuration_service = instantiation_service.language_configuration_service.clone();
            let language_service = instantiation_service.get(I_LANGUAGE_SERVICE);
            let fixture = Self { disposables, instantiation_service, language_configuration_service, language_service };

            fixture.register_language(
                SURROUNDING_LANGUAGE_ID,
                LanguageConfiguration {
                    auto_closing_pairs: Some(vec![AutoClosingPairConditional::new("(", ")")]),
                    ..LanguageConfiguration::default()
                },
            );

            fixture.register_language(
                EMPTY_CLOSING_SURROUND_LANGUAGE_ID,
                LanguageConfiguration {
                    surrounding_pairs: Some(vec![AutoClosingPair::new("<", "")]),
                    ..LanguageConfiguration::default()
                },
            );

            fixture.setup_indent_rules_language(INDENT_RULES_LANGUAGE_ID, indent_rules_language_indentation_rules());

            fixture.register_language(
                ELECTRIC_CHAR_LANGUAGE_ID,
                LanguageConfiguration {
                    electric_character_support: doc_comment_electric_character_support(),
                    brackets: Some(vec![
                        ("{".to_owned(), "}".to_owned()),
                        ("[".to_owned(), "]".to_owned()),
                        ("(".to_owned(), ")".to_owned()),
                    ]),
                    ..LanguageConfiguration::default()
                },
            );

            fixture.setup_auto_closing_language();
            fixture
        }

        /// O `teardown` da suíte.
        pub fn teardown(&self) {
            self.disposables.dispose();
        }

        /// O par `disposables.add(languageService.registerLanguage({ id }))` e
        /// `disposables.add(languageConfigurationService.register(id, configuration))` que o `setup` e os auxiliares repetem.
        pub fn register_language(&self, language_id: &str, configuration: LanguageConfiguration) {
            self.disposables.add(self.language_service.register_language(LanguageExtensionPoint::new(language_id)));
            self.disposables.add(self.language_configuration_service.register(language_id, configuration, None));
        }

        /// `setupOnEnterLanguage(indentAction)`: devolve o id da linguagem.
        pub fn setup_on_enter_language(&self, indent_action: IndentAction) -> String {
            let on_enter_language_id = "onEnterMode";

            self.register_language(
                on_enter_language_id,
                LanguageConfiguration {
                    on_enter_rules: Some(vec![OnEnterRule {
                        before_text: re(".*", ""),
                        after_text: None,
                        previous_line_text: None,
                        action: EnterAction::new(indent_action),
                    }]),
                    ..LanguageConfiguration::default()
                },
            );
            on_enter_language_id.to_owned()
        }

        /// `setupIndentRulesLanguage(languageId, indentationRules)`: devolve o id da linguagem.
        pub fn setup_indent_rules_language(&self, language_id: &str, indentation_rules: IndentationRule) -> String {
            self.register_language(
                language_id,
                LanguageConfiguration { indentation_rules: Some(indentation_rules), ..LanguageConfiguration::default() },
            );
            language_id.to_owned()
        }

        /// `setupAutoClosingLanguage()`.
        pub fn setup_auto_closing_language(&self) {
            let mut auto_closing_pairs = common_auto_closing_pairs();
            auto_closing_pairs.push(AutoClosingPairConditional::new("begin", "end").not_in(&["string"]));

            self.register_language(
                AUTO_CLOSING_LANGUAGE_ID,
                LanguageConfiguration {
                    comments: Some(CommentRule { line_comment: None, block_comment: Some(("/*".to_owned(), "*/".to_owned())) }),
                    auto_closing_pairs: Some(auto_closing_pairs),
                    electric_character_support: doc_comment_electric_character_support(),
                    ..LanguageConfiguration::default()
                },
            );
        }

        /// `setupAutoClosingLanguageTokenization()`: registra o tokenizador de teste da `autoClosingLanguage`.
        pub fn setup_auto_closing_language_tokenization(&self) {
            let encoded_language_id = self.language_service.language_id_codec().encode_language_id(AUTO_CLOSING_LANGUAGE_ID);
            self.disposables.add(
                tokenization_registry().register(AUTO_CLOSING_LANGUAGE_ID, Rc::new(AutoClosingTokenizationSupport { encoded_language_id })),
            );
        }

        /// `setAutoClosingLanguageEnabledSet(chars)`.
        pub fn set_auto_closing_language_enabled_set(&self, chars: &str) {
            self.disposables.add(self.language_configuration_service.register(
                AUTO_CLOSING_LANGUAGE_ID,
                LanguageConfiguration {
                    auto_close_before: Some(chars.to_owned()),
                    auto_closing_pairs: Some(common_auto_closing_pairs()),
                    ..LanguageConfiguration::default()
                },
                None,
            ));
        }

        /// `createTextModel(text, languageId = null, options = TextModel.DEFAULT_CREATION_OPTIONS, uri = null)`: o modelo usa
        /// os serviços de linguagem da suíte (o `instantiateTextModel(instantiationService, ...)` do original) e é
        /// descartado junto com a fixture.
        pub fn create_text_model(
            &self,
            text: impl Into<TextSource>,
            language_id: Option<&str>,
            options: RelaxedTextModelCreationOptions,
            uri: Option<URI>,
        ) -> Rc<TextModel> {
            let model = TextModel::new_with_language_service(
                text.into(),
                language_id.unwrap_or(PLAINTEXT_LANGUAGE_ID),
                resolve_options(&options),
                uri,
                self.instantiation_service.undo_redo_service.clone(),
                Some(TextModelServices {
                    language_service: self.language_service.clone(),
                    language_configuration_service: self.language_configuration_service.clone(),
                }),
            );
            let owned = model.clone();
            self.disposables.add(to_disposable(move || owned.dispose()));
            model
        }

        /// `withTestCodeEditor(text, options, callback)` local da suíte: o editor (e o modelo, quando `text` não é um)
        /// é descartado junto com a fixture, não ao fim do callback.
        pub fn with_test_code_editor(
            &self,
            text: impl Into<TestEditorInput>,
            options: &TestCodeEditorCreationOptions,
            callback: impl FnOnce(&Rc<TestCodeEditor>, &Rc<EditorViewModel>),
        ) {
            let model = match text.into() {
                TestEditorInput::Model(model) => model,
                TestEditorInput::Text(text) => self.create_text_model(text, None, RelaxedTextModelCreationOptions::default(), None),
                TestEditorInput::Lines(lines) => {
                    self.create_text_model(lines.join("\n"), None, RelaxedTextModelCreationOptions::default(), None)
                }
                TestEditorInput::Factory(factory) => {
                    self.create_text_model(factory, None, RelaxedTextModelCreationOptions::default(), None)
                }
            };
            let editor = self.disposables.add_rc(instantiate_test_code_editor(&self.instantiation_service, Some(model), options));
            let view_model = editor.get_view_model().expect("o editor de teste tem o modelo ligado");
            view_model.set_has_focus(true);
            callback(&editor, &view_model);
        }

        /// `usingCursor(opts, callback)`.
        pub fn using_cursor(
            &self,
            opts: CursorOpts<'_>,
            callback: impl FnOnce(&Rc<TestCodeEditor>, &Rc<TextModel>, &Rc<EditorViewModel>),
        ) {
            let model = self.create_text_model(
                opts.text.join("\n"),
                opts.language_id,
                opts.model_opts.unwrap_or_default(),
                None,
            );
            let editor_options = opts.editor_opts.unwrap_or_else(super::no_options);
            self.with_test_code_editor(model.clone(), &editor_options, |editor, view_model| {
                callback(editor, &model, view_model);
            });
        }
    }

    impl Default for EditorControllerFixture {
        fn default() -> Self {
            Self::new()
        }
    }

    /// O que o mocha faz em volta de cada `test(...)` da suíte `Editor Controller`: carrega o módulo `coreCommands`
    /// (que registra os comandos), instala o guarda de vazamentos, roda o `setup`, o corpo e o `teardown`.
    pub fn run_test(body: impl FnOnce(&EditorControllerFixture)) {
        register_core_commands();
        let _ds = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = EditorControllerFixture::new();
        body(&fixture);
        fixture.teardown();
    }

    /// `viewModel.type(text, source)`.
    pub fn type_text(view_model: &EditorViewModel, text: &str, source: Option<&str>) {
        view_model.r#type(&u(text), source);
    }

    /// `viewModel.setSelections('test', selections)`: o motivo omitido do original vale `NotSet`.
    pub fn set_test_selections(view_model: &EditorViewModel, selections: &[Selection]) {
        view_model.set_selections(Some("test"), selections, CursorChangeReason::NotSet);
    }

    /// `model.getValue()`.
    pub fn get_value(model: &TextModel) -> String {
        model.get_value().to_string_lossy()
    }

    /// `model.getValue(EndOfLinePreference.LF)`.
    pub fn get_value_lf(model: &TextModel) -> String {
        model.get_value_with(EndOfLinePreference::Lf, false).to_string_lossy()
    }

    /// `model.getLineContent(lineNumber)`.
    pub fn get_line_content(model: &TextModel, line_number: i32) -> String {
        CursorSimpleModel::get_line_content(model, line_number).to_string_lossy()
    }

    /// `CoreNavigationCommands.X.runCoreEditorCommand(viewModel, { position: new Position(lineNumber, column) })`, o
    /// argumento dos comandos `WordSelect` e `WordSelectDrag`.
    pub fn run_move_command(
        command: &CoreEditorCommand<MoveCommandOptions>,
        view_model: &EditorViewModel,
        line_number: i32,
        column: i32,
    ) {
        command.run_core_editor_command(
            view_model,
            &MoveCommandOptions { position: Some(Position::new(line_number, column)), ..MoveCommandOptions::default() },
        );
    }

    /// As opções `{ wordWrap: 'wordWrapColumn', wordWrapColumn: column }` do editor.
    pub fn word_wrap_options(column: i32) -> Dynamic {
        Dynamic::object([("wordWrap", Dynamic::from("wordWrapColumn")), ("wordWrapColumn", Dynamic::from(column))])
    }

    /// `extractAutoClosingSpecialColumns(maxColumn, annotatedLine)`: o índice do vetor é a coluna (a posição 0 não é
    /// usada); uma coluna além de `maxColumn` estende o vetor, com `Normal` nas posições intermediárias.
    pub fn extract_auto_closing_special_columns(max_column: i32, annotated_line: &str) -> Vec<AutoClosingColumnType> {
        let mut result = vec![AutoClosingColumnType::Normal; max_column.max(0) as usize + 1];
        let mut column = 1usize;
        for unit in annotated_line.encode_utf16() {
            let special = match unit {
                unit if unit == u16::from(b'|') => Some(AutoClosingColumnType::Special1),
                unit if unit == u16::from(b'!') => Some(AutoClosingColumnType::Special2),
                _ => None,
            };
            match special {
                Some(special) => {
                    if column >= result.len() {
                        result.resize(column + 1, AutoClosingColumnType::Normal);
                    }
                    result[column] = special;
                }
                None => column += 1,
            }
        }
        result
    }

    /// `assertType(editor, model, viewModel, lineNumber, column, chr, expectedInsert, message)`.
    #[allow(clippy::too_many_arguments)]
    pub fn assert_type(
        editor: &TestCodeEditor,
        model: &TextModel,
        view_model: &EditorViewModel,
        line_number: i32,
        column: i32,
        chr: &str,
        expected_insert: &str,
        message: &str,
    ) {
        let line_content = CursorSimpleModel::get_line_content(model, line_number);
        let units = line_content.units();
        let split = ((column - 1).max(0) as usize).min(units.len());
        let mut expected: Vec<u16> = units[..split].to_vec();
        expected.extend(expected_insert.encode_utf16());
        expected.extend_from_slice(&units[split..]);
        move_to(editor, view_model, line_number, column, false);
        type_text(view_model, chr, Some("keyboard"));
        assert_eq!(get_line_content(model, line_number), String::from_utf16_lossy(&expected), "{message}");
        model.undo();
    }

    // #region tokenização da autoClosingLanguage

    /// `BaseState`, `StringState` e `BlockCommentState` de `setupAutoClosingLanguageTokenization`. O `parent` de um
    /// `BaseState` e o `parentState` dos outros dois guardam o estado de fora.
    #[derive(Clone)]
    enum AutoClosingState {
        Base { parent: Option<Rc<AutoClosingState>> },
        Str { char: u16, parent_state: Rc<AutoClosingState> },
        BlockComment { parent_state: Rc<AutoClosingState> },
    }

    impl AutoClosingState {
        /// O `equals` de cada classe do original, com o `instanceof` do `BlockCommentState.equals` (que confere
        /// `StringState`, não `BlockCommentState`) preservado como está.
        fn state_equals(&self, other: &AutoClosingState) -> bool {
            match self {
                AutoClosingState::Base { parent } => match other {
                    AutoClosingState::Base { parent: other_parent } => match (parent, other_parent) {
                        (None, None) => true,
                        (Some(parent), Some(other_parent)) => parent.state_equals(other_parent),
                        _ => false,
                    },
                    _ => false,
                },
                AutoClosingState::Str { char, parent_state } => match other {
                    AutoClosingState::Str { char: other_char, parent_state: other_parent_state } => {
                        char == other_char && parent_state.state_equals(other_parent_state)
                    }
                    _ => false,
                },
                AutoClosingState::BlockComment { parent_state } => match other {
                    AutoClosingState::Str { parent_state: other_parent_state, .. } => parent_state.state_equals(other_parent_state),
                    _ => false,
                },
            }
        }
    }

    impl IState for AutoClosingState {
        fn clone_state(&self) -> State {
            Rc::new(self.clone())
        }

        fn equals(&self, other: &dyn IState) -> bool {
            other.as_any().downcast_ref::<AutoClosingState>().is_some_and(|other| self.state_equals(other))
        }

        fn as_any(&self) -> &dyn Any {
            self
        }
    }

    /// O `{ length, type }` de `tokens` no `tokenizeEncoded`.
    struct TokenSpan {
        length: usize,
        token_type: u32,
    }

    /// O estado local de um `tokenizeEncoded`: a linha que falta consumir, o estado e os tokens até aqui.
    struct LineTokenizer {
        line: Vec<u16>,
        state: AutoClosingState,
        tokens: Vec<TokenSpan>,
    }

    impl LineTokenizer {
        /// `generateToken(length, type, newState?)`.
        fn generate_token(&mut self, length: usize, token_type: u32, new_state: Option<AutoClosingState>) {
            match self.tokens.last_mut() {
                Some(last) if last.token_type == token_type => {
                    // grow last tokens
                    last.length += length;
                }
                _ => self.tokens.push(TokenSpan { length, token_type }),
            }
            self.line.drain(..length.min(self.line.len()));
            if let Some(new_state) = new_state {
                self.state = new_state;
            }
        }

        /// O tamanho do trecho inicial da linha cujos caracteres satisfazem `accepts` (o `^[...]+` do original).
        fn leading_run(&self, accepts: impl Fn(u16) -> bool) -> usize {
            self.line.iter().take_while(|unit| accepts(**unit)).count()
        }

        /// `advance()`.
        fn advance(&mut self) {
            const SINGLE_QUOTE: u16 = b'\'' as u16;
            const DOUBLE_QUOTE: u16 = b'"' as u16;
            const BACKTICK: u16 = b'`' as u16;
            const OPEN_BRACE: u16 = b'{' as u16;
            const CLOSE_BRACE: u16 = b'}' as u16;
            const SLASH: u16 = b'/' as u16;
            const STAR: u16 = b'*' as u16;
            const BACKSLASH: u16 = b'\\' as u16;
            const DOLLAR: u16 = b'$' as u16;

            let state = self.state.clone();
            let first = self.line[0];
            match &state {
                AutoClosingState::Base { parent } => {
                    let run = self.leading_run(|unit| !matches!(unit, SINGLE_QUOTE | DOUBLE_QUOTE | BACKTICK | OPEN_BRACE | CLOSE_BRACE | SLASH));
                    if run > 0 {
                        return self.generate_token(run, standard_token_type::OTHER, None);
                    }
                    if matches!(first, SINGLE_QUOTE | DOUBLE_QUOTE | BACKTICK) {
                        let new_state = AutoClosingState::Str { char: first, parent_state: Rc::new(state.clone()) };
                        return self.generate_token(1, standard_token_type::STRING, Some(new_state));
                    }
                    if first == OPEN_BRACE {
                        let new_state = AutoClosingState::Base { parent: Some(Rc::new(state.clone())) };
                        return self.generate_token(1, standard_token_type::OTHER, Some(new_state));
                    }
                    if first == CLOSE_BRACE {
                        let new_state = match parent {
                            Some(parent) => (**parent).clone(),
                            None => AutoClosingState::Base { parent: None },
                        };
                        return self.generate_token(1, standard_token_type::OTHER, Some(new_state));
                    }
                    if self.line.starts_with(&[SLASH, SLASH]) {
                        return self.generate_token(self.line.len(), standard_token_type::COMMENT, None);
                    }
                    if self.line.starts_with(&[SLASH, STAR]) {
                        let new_state = AutoClosingState::BlockComment { parent_state: Rc::new(state.clone()) };
                        return self.generate_token(2, standard_token_type::COMMENT, Some(new_state));
                    }
                    self.generate_token(1, standard_token_type::OTHER, None)
                }
                AutoClosingState::Str { char, parent_state } => {
                    let run = self.leading_run(|unit| !matches!(unit, BACKSLASH | SINGLE_QUOTE | DOUBLE_QUOTE | BACKTICK | DOLLAR));
                    if run > 0 {
                        return self.generate_token(run, standard_token_type::STRING, None);
                    }
                    if first == BACKSLASH {
                        return self.generate_token(2, standard_token_type::STRING, None);
                    }
                    if first == *char {
                        return self.generate_token(1, standard_token_type::STRING, Some((**parent_state).clone()));
                    }
                    if self.line.starts_with(&[DOLLAR, OPEN_BRACE]) {
                        let new_state = AutoClosingState::Base { parent: Some(Rc::new(state.clone())) };
                        return self.generate_token(2, standard_token_type::OTHER, Some(new_state));
                    }
                    self.generate_token(1, standard_token_type::OTHER, None)
                }
                AutoClosingState::BlockComment { parent_state } => {
                    let run = self.leading_run(|unit| unit != STAR);
                    if run > 0 {
                        return self.generate_token(run, standard_token_type::STRING, None);
                    }
                    if self.line.starts_with(&[STAR, SLASH]) {
                        return self.generate_token(2, standard_token_type::COMMENT, Some((**parent_state).clone()));
                    }
                    self.generate_token(1, standard_token_type::OTHER, None)
                }
            }
        }
    }

    /// O objeto `ITokenizationSupport` de `setupAutoClosingLanguageTokenization`.
    struct AutoClosingTokenizationSupport {
        encoded_language_id: u32,
    }

    impl ITokenizationSupport for AutoClosingTokenizationSupport {
        fn get_initial_state(&self) -> State {
            Rc::new(AutoClosingState::Base { parent: None })
        }

        fn tokenize(&self, _line: &JsString, _has_eol: bool, _state: &State) -> TokenizationResult {
            unreachable!("`tokenize: undefined!` no original")
        }

        fn tokenize_encoded(&self, line: &JsString, _has_eol: bool, state: &State) -> EncodedTokenizationResult {
            let state = state.as_any().downcast_ref::<AutoClosingState>().expect("o estado da autoClosingLanguage").clone();
            let mut tokenizer = LineTokenizer { line: line.units().to_vec(), state, tokens: Vec::new() };
            while !tokenizer.line.is_empty() {
                tokenizer.advance();
            }
            let mut result: Vec<u32> = Vec::with_capacity(tokenizer.tokens.len() * 2);
            let mut start_index = 0u32;
            for token in &tokenizer.tokens {
                result.push(start_index);
                result.push(
                    (self.encoded_language_id << metadata_consts::LANGUAGEID_OFFSET)
                        | (token.token_type << metadata_consts::TOKEN_TYPE_OFFSET),
                );
                start_index += token.length as u32;
            }
            let end_state: State = Rc::new(tokenizer.state);
            EncodedTokenizationResult { tokens: result, font_info: Vec::new(), end_state }
        }
    }

    // #endregion
}

/// O `tokenizationSupport` do caso `issue #46314`: tokens vazios e o estado nulo.
struct EmptyTokenizationSupport;

impl ITokenizationSupport for EmptyTokenizationSupport {
    fn get_initial_state(&self) -> State {
        null_state()
    }

    fn tokenize(&self, _line: &JsString, _has_eol: bool, _state: &State) -> TokenizationResult {
        unreachable!("`tokenize: undefined!` no original")
    }

    fn tokenize_encoded(&self, _line: &JsString, _has_eol: bool, state: &State) -> EncodedTokenizationResult {
        EncodedTokenizationResult { tokens: Vec::new(), font_info: Vec::new(), end_state: state.clone() }
    }
}

// Casos da suíte `Editor Controller`

#[test]
fn editor_controller_issue_microsoft_monaco_editor_443_indentation_of_a_single_row_deletes_selected_text_in_some_cases() {
    run_test(|f| {
        let editing = core_editing_commands::instance();
        let model = f.create_text_model(
            ["Hello world!", "another line"].join("\n"),
            None,
            RelaxedTextModelCreationOptions { insert_spaces: Some(false), ..Default::default() },
            None,
        );
        f.with_test_code_editor(model, &no_options(), |editor, view_model| {
            set_test_selections(view_model, &[Selection::new(1, 1, 1, 13)]);

            // Check that indenting maintains the selection start at column 1
            editor.run_command(&*editing.tab, None);
            assert_eq!(view_model.get_selection(), Selection::new(1, 1, 1, 14));
        });
    });
}

#[test]
fn editor_controller_bug_9121_auto_indent_undo_redo_is_funky() {
    run_test(|f| {
        let editing = core_editing_commands::instance();
        let model = f.create_text_model(
            [""].join("\n"),
            None,
            RelaxedTextModelCreationOptions { insert_spaces: Some(false), trim_auto_whitespace: Some(false), ..Default::default() },
            None,
        );

        f.with_test_code_editor(model.clone(), &no_options(), |editor, view_model| {
            type_text(view_model, "\n", Some("keyboard"));
            assert_eq!(get_value_lf(&model), "\n", "assert1");

            editor.run_command(&*editing.tab, None);
            assert_eq!(get_value_lf(&model), "\n\t", "assert2");

            type_text(view_model, "\n", Some("keyboard"));
            assert_eq!(get_value_lf(&model), "\n\t\n\t", "assert3");

            type_text(view_model, "x", None);
            assert_eq!(get_value_lf(&model), "\n\t\n\tx", "assert4");

            move_left(editor, view_model, false);
            assert_eq!(get_value_lf(&model), "\n\t\n\tx", "assert5");

            editor.run_command(&*editing.delete_left, None);
            assert_eq!(get_value_lf(&model), "\n\t\nx", "assert6");

            editor.run_command(&*editing.delete_left, None);
            assert_eq!(get_value_lf(&model), "\n\tx", "assert7");

            editor.run_command(&*editing.delete_left, None);
            assert_eq!(get_value_lf(&model), "\nx", "assert8");

            editor.run_command(&*editing.delete_left, None);
            assert_eq!(get_value_lf(&model), "x", "assert9");

            editor.run_command(&*editing.undo, None);
            assert_eq!(get_value_lf(&model), "\nx", "assert10");

            editor.run_command(&*editing.undo, None);
            assert_eq!(get_value_lf(&model), "\n\t\nx", "assert11");

            editor.run_command(&*editing.undo, None);
            assert_eq!(get_value_lf(&model), "\n\t\n\tx", "assert12");

            editor.run_command(&*editing.redo, None);
            assert_eq!(get_value_lf(&model), "\n\t\nx", "assert13");

            editor.run_command(&*editing.redo, None);
            assert_eq!(get_value_lf(&model), "\nx", "assert14");

            editor.run_command(&*editing.redo, None);
            assert_eq!(get_value_lf(&model), "x", "assert15");
        });
    });
}

#[test]
fn editor_controller_issue_23539_setting_model_eol_isnt_undoable() {
    run_test(|f| {
        let editing = core_editing_commands::instance();
        f.with_test_code_editor(["Hello", "world"], &no_options(), |editor, view_model| {
            let model = editor.get_model().expect("o editor tem o modelo");

            assert_cursor(view_model, Position::new(1, 1));
            model.set_eol(EndOfLineSequence::Lf);
            assert_eq!(get_value(&model), "Hello\nworld");

            model.push_eol(EndOfLineSequence::CrLf);
            assert_eq!(get_value(&model), "Hello\r\nworld");

            editor.run_command(&*editing.undo, None);
            assert_eq!(get_value(&model), "Hello\nworld");
        });
    });
}

#[test]
fn editor_controller_issue_47733_undo_mangles_unicode_characters() {
    run_test(|f| {
        let editing = core_editing_commands::instance();
        let language_id = "myMode";

        f.register_language(
            language_id,
            LanguageConfiguration {
                surrounding_pairs: Some(vec![AutoClosingPair::new("%", "%")]),
                ..LanguageConfiguration::default()
            },
        );

        let model = f.create_text_model("'👁'", Some(language_id), Default::default(), None);

        f.with_test_code_editor(model.clone(), &no_options(), |editor, view_model| {
            editor.set_selection(Selection::new(1, 1, 1, 2).into(), None);

            type_text(view_model, "%", Some("keyboard"));
            assert_eq!(get_value_lf(&model), "%'%👁'", "assert1");

            editor.run_command(&*editing.undo, None);
            assert_eq!(get_value_lf(&model), "'👁'", "assert2");
        });
    });
}

#[test]
fn editor_controller_issue_46208_allow_empty_selections_in_the_undo_redo_stack() {
    run_test(|f| {
        let editing = core_editing_commands::instance();
        let model = f.create_text_model("", None, Default::default(), None);

        f.with_test_code_editor(model.clone(), &no_options(), |editor, view_model| {
            type_text(view_model, "Hello", Some("keyboard"));
            type_text(view_model, " ", Some("keyboard"));
            type_text(view_model, "world", Some("keyboard"));
            type_text(view_model, " ", Some("keyboard"));
            assert_eq!(get_line_content(&model, 1), "Hello world ");
            assert_cursor(view_model, Position::new(1, 13));

            move_left(editor, view_model, false);
            move_right(editor, view_model, false);

            let mut cursor_state_computer = |_: &[ValidEditOperation]| -> Option<Vec<Selection>> { Some(Vec::new()) };
            model.push_edit_operations(
                Some(Vec::new()),
                &[EditOperation::replace_move(Range::new(1, 12, 1, 13), Some(JsString::from("")))],
                Some(&mut cursor_state_computer),
                None,
                None,
            );
            assert_eq!(get_line_content(&model, 1), "Hello world");
            assert_cursor(view_model, Position::new(1, 12));

            editor.run_command(&*editing.undo, None);
            assert_eq!(get_line_content(&model, 1), "Hello world ");
            assert_cursor(view_model, Selection::new(1, 13, 1, 13));

            editor.run_command(&*editing.undo, None);
            assert_eq!(get_line_content(&model, 1), "Hello world");
            assert_cursor(view_model, Position::new(1, 12));

            editor.run_command(&*editing.undo, None);
            assert_eq!(get_line_content(&model, 1), "Hello");
            assert_cursor(view_model, Position::new(1, 6));

            editor.run_command(&*editing.undo, None);
            assert_eq!(get_line_content(&model, 1), "");
            assert_cursor(view_model, Position::new(1, 1));

            editor.run_command(&*editing.redo, None);
            assert_eq!(get_line_content(&model, 1), "Hello");
            assert_cursor(view_model, Position::new(1, 6));

            editor.run_command(&*editing.redo, None);
            assert_eq!(get_line_content(&model, 1), "Hello world");
            assert_cursor(view_model, Position::new(1, 12));

            editor.run_command(&*editing.redo, None);
            assert_eq!(get_line_content(&model, 1), "Hello world ");
            assert_cursor(view_model, Position::new(1, 13));

            editor.run_command(&*editing.redo, None);
            assert_eq!(get_line_content(&model, 1), "Hello world");
            assert_cursor(view_model, Position::new(1, 12));

            editor.run_command(&*editing.redo, None);
            assert_eq!(get_line_content(&model, 1), "Hello world");
            assert_cursor(view_model, Position::new(1, 12));
        });
    });
}

#[test]
fn editor_controller_bug_16815_shift_tab_doesnt_go_back_to_tabstop() {
    run_test(|f| {
        let editing = core_editing_commands::instance();
        let language_id = f.setup_on_enter_language(IndentAction::IndentOutdent);
        let model = f.create_text_model(["     function baz() {"].join("\n"), Some(language_id.as_str()), Default::default(), None);

        f.with_test_code_editor(model.clone(), &no_options(), |editor, view_model| {
            move_to(editor, view_model, 1, 6, false);
            assert_cursor(view_model, Selection::new(1, 6, 1, 6));

            editor.run_command(&*editing.outdent, None);
            assert_eq!(get_line_content(&model, 1), "    function baz() {");
            assert_cursor(view_model, Selection::new(1, 5, 1, 5));
        });
    });
}

#[test]
fn editor_controller_bug_18293_regression_editor_cant_outdent_whitespace_line() {
    run_test(|f| {
        let editing = core_editing_commands::instance();
        let model = f.create_text_model(["      "].join("\n"), None, Default::default(), None);

        f.with_test_code_editor(model.clone(), &no_options(), |editor, view_model| {
            move_to(editor, view_model, 1, 7, false);
            assert_cursor(view_model, Selection::new(1, 7, 1, 7));

            editor.run_command(&*editing.outdent, None);
            assert_eq!(get_line_content(&model, 1), "    ");
            assert_cursor(view_model, Selection::new(1, 5, 1, 5));
        });
    });
}

#[test]
fn editor_controller_issue_95591_unindenting_moves_cursor_to_beginning_of_line() {
    run_test(|f| {
        let editing = core_editing_commands::instance();
        let model = f.create_text_model(["        "].join("\n"), None, Default::default(), None);

        let options = Dynamic::object([("useTabStops", Dynamic::from(false))]);
        f.with_test_code_editor(model.clone(), &options, |editor, view_model| {
            move_to(editor, view_model, 1, 9, false);
            assert_cursor(view_model, Selection::new(1, 9, 1, 9));

            editor.run_command(&*editing.outdent, None);
            assert_eq!(get_line_content(&model, 1), "    ");
            assert_cursor(view_model, Selection::new(1, 5, 1, 5));
        });
    });
}

#[test]
fn editor_controller_bug_16657_editor_tab_on_empty_line_of_zero_indentation_moves_cursor_to_position_1_1() {
    run_test(|f| {
        let editing = core_editing_commands::instance();
        let model = f.create_text_model(
            [
                "function baz() {",
                "\tfunction hello() { // something here",
                "\t",
                "",
                "\t}",
                "}",
                "",
            ]
            .join("\n"),
            None,
            RelaxedTextModelCreationOptions { insert_spaces: Some(false), ..Default::default() },
            None,
        );

        f.with_test_code_editor(model.clone(), &no_options(), |editor, view_model| {
            move_to(editor, view_model, 7, 1, false);
            assert_cursor(view_model, Selection::new(7, 1, 7, 1));

            editor.run_command(&*editing.tab, None);
            assert_eq!(get_line_content(&model, 7), "\t");
            assert_cursor(view_model, Selection::new(7, 2, 7, 2));
        });
    });
}

#[test]
fn editor_controller_bug_16740_editor_cut_line_doesnt_quite_cut_the_last_line() {
    run_test(|f| {
        // Part 1 => there is text on the last line
        f.with_test_code_editor(["asdasd", "qwerty"], &no_options(), |editor, view_model| {
            let model = editor.get_model().expect("o editor tem o modelo");

            move_to(editor, view_model, 2, 1, false);
            assert_cursor(view_model, Selection::new(2, 1, 2, 1));

            view_model.cut(Some("keyboard"));
            assert_eq!(model.get_line_count(), 1);
            assert_eq!(get_line_content(&model, 1), "asdasd");
        });

        // Part 2 => there is no text on the last line
        f.with_test_code_editor(["asdasd", ""], &no_options(), |editor, view_model| {
            let model = editor.get_model().expect("o editor tem o modelo");

            move_to(editor, view_model, 2, 1, false);
            assert_cursor(view_model, Selection::new(2, 1, 2, 1));

            view_model.cut(Some("keyboard"));
            assert_eq!(model.get_line_count(), 1);
            assert_eq!(get_line_content(&model, 1), "asdasd");

            view_model.cut(Some("keyboard"));
            assert_eq!(model.get_line_count(), 1);
            assert_eq!(get_line_content(&model, 1), "");
        });
    });
}

#[test]
fn editor_controller_issue_128602_when_cutting_multiple_lines_ctrl_x_the_last_line_will_not_be_erased() {
    run_test(|f| {
        f.with_test_code_editor(["a1", "a2", "a3"], &no_options(), |editor, view_model| {
            let model = editor.get_model().expect("o editor tem o modelo");

            set_test_selections(
                view_model,
                &[Selection::new(1, 1, 1, 1), Selection::new(2, 1, 2, 1), Selection::new(3, 1, 3, 1)],
            );

            view_model.cut(Some("keyboard"));
            assert_eq!(model.get_line_count(), 1);
            assert_eq!(get_line_content(&model, 1), "");
        });
    });
}

#[test]
fn editor_controller_bug_11476_double_bracket_surrounding_undo_is_broken() {
    run_test(|f| {
        f.using_cursor(
            CursorOpts { text: vec!["hello"], language_id: Some(SURROUNDING_LANGUAGE_ID), ..Default::default() },
            |editor, _model, view_model| {
                move_to(editor, view_model, 1, 3, false);
                move_to(editor, view_model, 1, 5, true);
                assert_cursor(view_model, Selection::new(1, 3, 1, 5));

                type_text(view_model, "(", Some("keyboard"));
                assert_cursor(view_model, Selection::new(1, 4, 1, 6));

                type_text(view_model, "(", Some("keyboard"));
                assert_cursor(view_model, Selection::new(1, 5, 1, 7));
            },
        );
    });
}

#[test]
fn editor_controller_issue_206774_surround_selection_command_with_empty_char_after_selection_should_not_throw() {
    // This test reproduces the issue where SurroundSelectionCommand throws when charAfterSelection is empty
    // The problem is that addTrackedEditOperation ignores empty strings, causing computeCursorState to fail
    // when trying to access inverseEditOperations[1].range (which is undefined)
    run_test(|f| {
        f.using_cursor(
            CursorOpts {
                text: vec!["hello world"],
                language_id: Some(EMPTY_CLOSING_SURROUND_LANGUAGE_ID),
                ..Default::default()
            },
            |editor, model, view_model| {
                // Select "hello"
                move_to(editor, view_model, 1, 1, false);
                move_to(editor, view_model, 1, 6, true);
                assert_cursor(view_model, Selection::new(1, 1, 1, 6));

                // Type < which should surround with '<' and empty string
                // This reproduces the crash where charAfterSelection is empty
                type_text(view_model, "<", Some("keyboard"));

                // Test passes if we don't crash - the exact cursor position depends on the fix
                // The main issue is that computeCursorState fails when charAfterSelection is empty
                assert_eq!(get_value(model), "<hello world");
            },
        );
    });
}

#[test]
fn editor_controller_issue_1140_backspace_stops_prematurely() {
    run_test(|f| {
        let editing = core_editing_commands::instance();
        let model = f.create_text_model(["function baz() {", "  return 1;", "};"].join("\n"), None, Default::default(), None);

        f.with_test_code_editor(model.clone(), &no_options(), |editor, view_model| {
            move_to(editor, view_model, 3, 2, false);
            move_to(editor, view_model, 1, 14, true);
            assert_cursor(view_model, Selection::new(3, 2, 1, 14));

            editor.run_command(&*editing.delete_left, None);
            assert_cursor(view_model, Selection::new(1, 14, 1, 14));
            assert_eq!(model.get_line_count(), 1);
            assert_eq!(get_line_content(&model, 1), "function baz(;");
        });
    });
}

#[test]
fn editor_controller_issue_10212_pasting_entire_line_does_not_replace_selection() {
    run_test(|f| {
        f.using_cursor(CursorOpts { text: vec!["line1", "line2"], ..Default::default() }, |editor, model, view_model| {
            move_to(editor, view_model, 2, 1, false);
            move_to(editor, view_model, 2, 6, true);

            view_model.paste(&u("line1\n"), true, None, None, false);

            assert_eq!(get_line_content(model, 1), "line1");
            assert_eq!(get_line_content(model, 2), "line1");
            assert_eq!(get_line_content(model, 3), "");
        });
    });
}

#[test]
fn editor_controller_issue_74722_pasting_whole_line_does_not_replace_selection() {
    run_test(|f| {
        f.using_cursor(CursorOpts { text: vec!["line1", "line sel 2", "line3"], ..Default::default() }, |_editor, model, view_model| {
            set_test_selections(view_model, &[Selection::new(2, 6, 2, 9)]);

            view_model.paste(&u("line1\n"), true, None, None, false);

            assert_eq!(get_line_content(model, 1), "line1");
            assert_eq!(get_line_content(model, 2), "line line1");
            assert_eq!(get_line_content(model, 3), " 2");
            assert_eq!(get_line_content(model, 4), "line3");
        });
    });
}

#[test]
fn editor_controller_issue_4996_multiple_cursor_paste_pastes_contents_of_all_cursors() {
    run_test(|f| {
        f.using_cursor(CursorOpts { text: vec!["line1", "line2", "line3"], ..Default::default() }, |_editor, model, view_model| {
            set_test_selections(view_model, &[Selection::new(1, 1, 1, 1), Selection::new(2, 1, 2, 1)]);

            view_model.paste(
                &u("a\nb\nc\nd"),
                false,
                Some(&[JsString::from("a\nb"), JsString::from("c\nd")][..]),
                None,
                false,
            );

            assert_eq!(get_value(model), ["a", "bline1", "c", "dline2", "line3"].join("\n"));
        });
    });
}

#[test]
fn editor_controller_issue_16155_paste_into_multiple_cursors_has_edge_case_when_number_of_lines_equals_number_of_cursors_minus_1() {
    run_test(|f| {
        f.using_cursor(CursorOpts { text: vec!["test", "test", "test", "test"], ..Default::default() }, |_editor, model, view_model| {
            set_test_selections(
                view_model,
                &[Selection::new(1, 1, 1, 5), Selection::new(2, 1, 2, 5), Selection::new(3, 1, 3, 5), Selection::new(4, 1, 4, 5)],
            );

            view_model.paste(&u("aaa\nbbb\nccc\n"), false, None, None, false);

            assert_eq!(
                get_value(model),
                [
                    "aaa", "bbb", "ccc", "", "aaa", "bbb", "ccc", "", "aaa", "bbb", "ccc", "", "aaa", "bbb", "ccc", "",
                ]
                .join("\n")
            );
        });
    });
}

#[test]
fn editor_controller_issue_43722_multiline_paste_doesnt_work_anymore() {
    run_test(|f| {
        f.using_cursor(CursorOpts { text: vec!["test", "test", "test", "test"], ..Default::default() }, |_editor, model, view_model| {
            set_test_selections(
                view_model,
                &[Selection::new(1, 1, 1, 5), Selection::new(2, 1, 2, 5), Selection::new(3, 1, 3, 5), Selection::new(4, 1, 4, 5)],
            );

            view_model.paste(&u("aaa\r\nbbb\r\nccc\r\nddd\r\n"), false, None, None, false);

            assert_eq!(get_value(model), ["aaa", "bbb", "ccc", "ddd"].join("\n"));
        });
    });
}

#[test]
fn editor_controller_issue_46440_1_pasting_a_multi_line_selection_pastes_entire_selection_into_every_insertion_point() {
    run_test(|f| {
        f.using_cursor(CursorOpts { text: vec!["line1", "line2", "line3"], ..Default::default() }, |_editor, model, view_model| {
            set_test_selections(
                view_model,
                &[Selection::new(1, 1, 1, 1), Selection::new(2, 1, 2, 1), Selection::new(3, 1, 3, 1)],
            );

            view_model.paste(&u("a\nb\nc"), false, None, None, false);

            assert_eq!(get_value(model), ["aline1", "bline2", "cline3"].join("\n"));
        });
    });
}

#[test]
fn editor_controller_issue_46440_2_pasting_a_multi_line_selection_pastes_entire_selection_into_every_insertion_point() {
    run_test(|f| {
        f.using_cursor(CursorOpts { text: vec!["line1", "line2", "line3"], ..Default::default() }, |_editor, model, view_model| {
            set_test_selections(
                view_model,
                &[Selection::new(1, 1, 1, 1), Selection::new(2, 1, 2, 1), Selection::new(3, 1, 3, 1)],
            );

            view_model.paste(&u("a\nb\nc\n"), false, None, None, false);

            assert_eq!(get_value(model), ["aline1", "bline2", "cline3"].join("\n"));
        });
    });
}

#[test]
fn editor_controller_issue_256039_paste_from_multiple_cursors_with_empty_selections_and_multi_cursor_paste_full() {
    run_test(|f| {
        f.using_cursor(
            CursorOpts {
                text: vec!["line1", "line2", "line3"],
                editor_opts: Some(Dynamic::object([("multiCursorPaste", Dynamic::from("full"))])),
                ..Default::default()
            },
            |_editor, model, view_model| {
                // 2 cursors on lines 1 and 2
                set_test_selections(view_model, &[Selection::new(1, 1, 1, 1), Selection::new(2, 1, 2, 1)]);

                view_model.paste(
                    &u("line1\nline2\n"),
                    true,
                    Some(&[JsString::from("line1\n"), JsString::from("line2\n")][..]),
                    None,
                    false,
                );

                // Each cursor gets its respective line
                assert_eq!(get_value(model), ["line1", "line1", "line2", "line2", "line3"].join("\n"));
            },
        );
    });
}

#[test]
fn editor_controller_issue_3071_investigate_why_undo_stack_gets_corrupted() {
    run_test(|f| {
        let editing = core_editing_commands::instance();
        let model = f.create_text_model(["some lines", "and more lines", "just some text"].join("\n"), None, Default::default(), None);

        f.with_test_code_editor(model.clone(), &no_options(), |editor, view_model| {
            move_to(editor, view_model, 1, 1, false);
            move_to(editor, view_model, 3, 4, true);

            let is_first = Rc::new(Cell::new(true));
            let listener_view_model = view_model.clone();
            let listener_is_first = is_first.clone();
            let disposable = model.on_did_change_content().listen(move |_| {
                if listener_is_first.get() {
                    listener_is_first.set(false);
                    type_text(&listener_view_model, "\t", Some("keyboard"));
                }
            });

            editor.run_command(&*editing.tab, None);
            assert_eq!(get_value(&model), ["\t just some text"].join("\n"), "001");

            editor.run_command(&*editing.undo, None);
            assert_eq!(get_value(&model), ["    some lines", "    and more lines", "    just some text"].join("\n"), "002");

            editor.run_command(&*editing.undo, None);
            assert_eq!(get_value(&model), ["some lines", "and more lines", "just some text"].join("\n"), "003");

            editor.run_command(&*editing.undo, None);
            assert_eq!(get_value(&model), ["some lines", "and more lines", "just some text"].join("\n"), "004");

            disposable.dispose();
        });
    });
}

#[test]
fn editor_controller_issue_12950_cannot_double_click_to_insert_emoji_using_osx_emoji_panel() {
    run_test(|f| {
        f.using_cursor(
            CursorOpts { text: vec!["some lines", "and more lines", "just some text"], language_id: None, ..Default::default() },
            |editor, model, view_model| {
                move_to(editor, view_model, 3, 1, false);

                type_text(view_model, "😍", Some("keyboard"));

                assert_eq!(get_value(model), ["some lines", "and more lines", "😍just some text"].join("\n"));
            },
        );
    });
}

#[test]
fn editor_controller_issue_3463_pressing_tab_adds_spaces_but_not_as_many_as_for_a_tab() {
    run_test(|f| {
        let editing = core_editing_commands::instance();
        let model = f.create_text_model(
            ["function a() {", "\tvar a = {", "\t\tx: 3", "\t};", "}"].join("\n"),
            None,
            Default::default(),
            None,
        );

        f.with_test_code_editor(model.clone(), &no_options(), |editor, view_model| {
            move_to(editor, view_model, 3, 2, false);
            editor.run_command(&*editing.tab, None);
            assert_eq!(get_line_content(&model, 3), "\t    \tx: 3");
        });
    });
}

#[test]
fn editor_controller_issue_4312_trying_to_type_a_tab_character_over_a_sequence_of_spaces_results_in_unexpected_behaviour() {
    run_test(|f| {
        let editing = core_editing_commands::instance();
        let model = f.create_text_model(
            ["var foo = 123;       // this is a comment", "var bar = 4;       // another comment"].join("\n"),
            None,
            RelaxedTextModelCreationOptions { insert_spaces: Some(false), ..Default::default() },
            None,
        );

        f.with_test_code_editor(model.clone(), &no_options(), |editor, view_model| {
            move_to(editor, view_model, 1, 15, false);
            move_to(editor, view_model, 1, 22, true);
            editor.run_command(&*editing.tab, None);
            assert_eq!(get_line_content(&model, 1), "var foo = 123;\t// this is a comment");
        });
    });
}

#[test]
fn editor_controller_issue_832_word_right() {
    run_test(|f| {
        let navigation = core_navigation_commands::instance();
        f.using_cursor(
            CursorOpts { text: vec!["   /* Just some   more   text a+= 3 +5-3 + 7 */  "], ..Default::default() },
            |editor, _model, view_model| {
                move_to(editor, view_model, 1, 1, false);

                // `expected` é o texto que o original mede com `.length`: a coluna esperada é o comprimento mais um.
                let assert_word_right = |col: i32, expected: &str| {
                    if col == 1 {
                        run_move_command(&navigation.word_select, view_model, 1, col);
                    } else {
                        run_move_command(&navigation.word_select_drag, view_model, 1, col);
                    }

                    let expected_col = expected.encode_utf16().count() as i32 + 1;
                    assert_eq!(view_model.get_selection().start_column, 1, "TEST FOR {col}");
                    assert_eq!(view_model.get_selection().end_column, expected_col, "TEST FOR {col}");
                };

                assert_word_right(1, "   ");
                assert_word_right(2, "   ");
                assert_word_right(3, "   ");
                assert_word_right(4, "   ");
                assert_word_right(5, "   /");
                assert_word_right(6, "   /*");
                assert_word_right(7, "   /* ");
                assert_word_right(8, "   /* Just");
                assert_word_right(9, "   /* Just");
                assert_word_right(10, "   /* Just");
                assert_word_right(11, "   /* Just");
                assert_word_right(12, "   /* Just ");
                assert_word_right(13, "   /* Just some");
                assert_word_right(14, "   /* Just some");
                assert_word_right(15, "   /* Just some");
                assert_word_right(16, "   /* Just some");
                assert_word_right(17, "   /* Just some ");
                assert_word_right(18, "   /* Just some  ");
                assert_word_right(19, "   /* Just some   ");
                assert_word_right(20, "   /* Just some   more");
                assert_word_right(21, "   /* Just some   more");
                assert_word_right(22, "   /* Just some   more");
                assert_word_right(23, "   /* Just some   more");
                assert_word_right(24, "   /* Just some   more ");
                assert_word_right(25, "   /* Just some   more  ");
                assert_word_right(26, "   /* Just some   more   ");
                assert_word_right(27, "   /* Just some   more   text");
                assert_word_right(28, "   /* Just some   more   text");
                assert_word_right(29, "   /* Just some   more   text");
                assert_word_right(30, "   /* Just some   more   text");
                assert_word_right(31, "   /* Just some   more   text ");
                assert_word_right(32, "   /* Just some   more   text a");
                assert_word_right(33, "   /* Just some   more   text a+");
                assert_word_right(34, "   /* Just some   more   text a+=");
                assert_word_right(35, "   /* Just some   more   text a+= ");
                assert_word_right(36, "   /* Just some   more   text a+= 3");
                assert_word_right(37, "   /* Just some   more   text a+= 3 ");
                assert_word_right(38, "   /* Just some   more   text a+= 3 +");
                assert_word_right(39, "   /* Just some   more   text a+= 3 +5");
                assert_word_right(40, "   /* Just some   more   text a+= 3 +5-");
                assert_word_right(41, "   /* Just some   more   text a+= 3 +5-3");
                assert_word_right(42, "   /* Just some   more   text a+= 3 +5-3 ");
                assert_word_right(43, "   /* Just some   more   text a+= 3 +5-3 +");
                assert_word_right(44, "   /* Just some   more   text a+= 3 +5-3 + ");
                assert_word_right(45, "   /* Just some   more   text a+= 3 +5-3 + 7");
                assert_word_right(46, "   /* Just some   more   text a+= 3 +5-3 + 7 ");
                assert_word_right(47, "   /* Just some   more   text a+= 3 +5-3 + 7 *");
                assert_word_right(48, "   /* Just some   more   text a+= 3 +5-3 + 7 */");
                assert_word_right(49, "   /* Just some   more   text a+= 3 +5-3 + 7 */ ");
                assert_word_right(50, "   /* Just some   more   text a+= 3 +5-3 + 7 */  ");
            },
        );
    });
}

#[test]
fn editor_controller_issue_33788_wrong_cursor_position_when_double_click_to_select_a_word() {
    run_test(|f| {
        let navigation = core_navigation_commands::instance();
        let model = f.create_text_model(["Just some text"].join("\n"), None, Default::default(), None);

        f.with_test_code_editor(model, &no_options(), |_editor, view_model| {
            run_move_command(&navigation.word_select, view_model, 1, 8);
            assert_eq!(view_model.get_selection(), Selection::new(1, 6, 1, 10));

            run_move_command(&navigation.word_select_drag, view_model, 1, 8);
            assert_eq!(view_model.get_selection(), Selection::new(1, 6, 1, 10));
        });
    });
}

#[test]
fn editor_controller_issue_12887_double_click_highlighting_separating_white_space() {
    run_test(|f| {
        let navigation = core_navigation_commands::instance();
        let model = f.create_text_model(["abc def"].join("\n"), None, Default::default(), None);

        f.with_test_code_editor(model, &no_options(), |_editor, view_model| {
            run_move_command(&navigation.word_select, view_model, 1, 5);
            assert_eq!(view_model.get_selection(), Selection::new(1, 5, 1, 8));
        });
    });
}

#[test]
fn editor_controller_double_click_on_punctuation_should_select_the_character_not_adjacent_space() {
    run_test(|f| {
        let navigation = core_navigation_commands::instance();
        let model = f.create_text_model(
            ["// a b c 1 2 3 ~ ! @ # $ % ^ & * ( ) _ + \\ /"].join("\n"),
            None,
            Default::default(),
            None,
        );

        f.with_test_code_editor(model, &no_options(), |_editor, view_model| {
            // Test double-click on '@' at position 20
            run_move_command(&navigation.word_select, view_model, 1, 20);
            assert_eq!(view_model.get_selection(), Selection::new(1, 20, 1, 21), "Should select @ character");

            // Test double-click on '#' at position 22
            run_move_command(&navigation.word_select, view_model, 1, 22);
            assert_eq!(view_model.get_selection(), Selection::new(1, 22, 1, 23), "Should select # character");

            // Test double-click on '!' at position 18
            run_move_command(&navigation.word_select, view_model, 1, 18);
            assert_eq!(view_model.get_selection(), Selection::new(1, 18, 1, 19), "Should select ! character");

            // Test double-click on first '/' in '//' at position 1
            run_move_command(&navigation.word_select, view_model, 1, 1);
            assert_eq!(view_model.get_selection(), Selection::new(1, 1, 1, 3), "Should select // token");

            // Test double-click on second '/' in '//' at position 2
            run_move_command(&navigation.word_select, view_model, 1, 2);
            assert_eq!(view_model.get_selection(), Selection::new(1, 1, 1, 3), "Should select // token");

            // Test double-click on '\' at position 42
            run_move_command(&navigation.word_select, view_model, 1, 42);
            assert_eq!(view_model.get_selection(), Selection::new(1, 42, 1, 43), "Should select \\ character");
        });
    });
}

#[test]
fn editor_controller_issue_9675_undo_redo_adds_a_stop_in_between_chn_characters() {
    run_test(|f| {
        let editing = core_editing_commands::instance();
        f.with_test_code_editor([""; 0], &no_options(), |editor, view_model| {
            let model = editor.get_model().expect("o editor tem o modelo");
            assert_cursor(view_model, Position::new(1, 1));

            // Typing sennsei in Japanese - Hiragana
            type_text(view_model, "ｓ", Some("keyboard"));
            view_model.composition_type(&u("せ"), 1, 0, 0, None);
            view_model.composition_type(&u("せｎ"), 1, 0, 0, None);
            view_model.composition_type(&u("せん"), 2, 0, 0, None);
            view_model.composition_type(&u("せんｓ"), 2, 0, 0, None);
            view_model.composition_type(&u("せんせ"), 3, 0, 0, None);
            view_model.composition_type(&u("せんせ"), 3, 0, 0, None);
            view_model.composition_type(&u("せんせい"), 3, 0, 0, None);
            view_model.composition_type(&u("せんせい"), 4, 0, 0, None);
            view_model.composition_type(&u("せんせい"), 4, 0, 0, None);
            view_model.composition_type(&u("せんせい"), 4, 0, 0, None);

            assert_eq!(get_line_content(&model, 1), "せんせい");
            assert_cursor(view_model, Position::new(1, 5));

            editor.run_command(&*editing.undo, None);
            assert_eq!(get_line_content(&model, 1), "");
            assert_cursor(view_model, Position::new(1, 1));
        });
    });
}

#[test]
fn editor_controller_issue_23983_calling_model_set_eol_does_not_reset_cursor_position() {
    run_test(|f| {
        f.using_cursor(CursorOpts { text: vec!["first line", "second line"], ..Default::default() }, |_editor, model, view_model| {
            model.set_eol(EndOfLineSequence::CrLf);

            set_test_selections(view_model, &[Selection::new(2, 2, 2, 2)]);
            model.set_eol(EndOfLineSequence::Lf);

            assert_cursor(view_model, Selection::new(2, 2, 2, 2));
        });
    });
}

#[test]
fn editor_controller_issue_23983_calling_model_set_value_resets_cursor_position() {
    run_test(|f| {
        f.using_cursor(CursorOpts { text: vec!["first line", "second line"], ..Default::default() }, |_editor, model, view_model| {
            model.set_eol(EndOfLineSequence::CrLf);

            set_test_selections(view_model, &[Selection::new(2, 2, 2, 2)]);
            model.set_value(["different first line", "different second line", "new third line"].join("\n").into());

            assert_cursor(view_model, Selection::new(1, 1, 1, 1));
        });
    });
}

#[test]
fn editor_controller_issue_36740_wordwrap_creates_an_extra_step_character_at_the_wrapping_point() {
    run_test(|f| {
        // a single model line => 4 view lines
        f.with_test_code_editor(
            [["Lorem ipsum ", "dolor sit amet ", "consectetur ", "adipiscing elit"].join("")],
            &word_wrap_options(16),
            |editor, view_model| {
                set_test_selections(view_model, &[Selection::new(1, 7, 1, 7)]);

                move_right(editor, view_model, false);
                assert_cursor(view_model, Selection::new(1, 8, 1, 8));

                move_right(editor, view_model, false);
                assert_cursor(view_model, Selection::new(1, 9, 1, 9));

                move_right(editor, view_model, false);
                assert_cursor(view_model, Selection::new(1, 10, 1, 10));

                move_right(editor, view_model, false);
                assert_cursor(view_model, Selection::new(1, 11, 1, 11));

                move_right(editor, view_model, false);
                assert_cursor(view_model, Selection::new(1, 12, 1, 12));

                move_right(editor, view_model, false);
                assert_cursor(view_model, Selection::new(1, 13, 1, 13));

                // moving to view line 2
                move_right(editor, view_model, false);
                assert_cursor(view_model, Selection::new(1, 14, 1, 14));

                move_left(editor, view_model, false);
                assert_cursor(view_model, Selection::new(1, 13, 1, 13));

                // moving back to view line 1
                move_left(editor, view_model, false);
                assert_cursor(view_model, Selection::new(1, 12, 1, 12));
            },
        );
    });
}

#[test]
fn editor_controller_issue_110376_multiple_selections_with_wordwrap_behave_differently() {
    run_test(|f| {
        // a single model line => 4 view lines
        f.with_test_code_editor(
            [["just a sentence. just a ", "sentence. just a sentence."].join("")],
            &word_wrap_options(25),
            |editor, view_model| {
                set_test_selections(
                    view_model,
                    &[Selection::new(1, 1, 1, 16), Selection::new(1, 18, 1, 33), Selection::new(1, 35, 1, 50)],
                );

                move_left(editor, view_model, false);
                assert_cursor(
                    view_model,
                    vec![Selection::new(1, 1, 1, 1), Selection::new(1, 18, 1, 18), Selection::new(1, 35, 1, 35)],
                );

                set_test_selections(
                    view_model,
                    &[Selection::new(1, 1, 1, 16), Selection::new(1, 18, 1, 33), Selection::new(1, 35, 1, 50)],
                );

                move_right(editor, view_model, false);
                assert_cursor(
                    view_model,
                    vec![Selection::new(1, 16, 1, 16), Selection::new(1, 33, 1, 33), Selection::new(1, 50, 1, 50)],
                );
            },
        );
    });
}

#[test]
fn editor_controller_issue_98320_multi_cursor_wrap_lines_and_cursor_select_right_cursors_out_of_sync() {
    run_test(|f| {
        // a single model line => 4 view lines
        f.with_test_code_editor(
            [[
                "lorem_ipsum-1993x11x13",
                "dolor_sit_amet-1998x04x27",
                "consectetur-2007x10x08",
                "adipiscing-2012x07x27",
                "elit-2015x02x27",
            ]
            .join("\n")],
            &word_wrap_options(16),
            |editor, view_model| {
                set_test_selections(
                    view_model,
                    &[
                        Selection::new(1, 13, 1, 13),
                        Selection::new(2, 16, 2, 16),
                        Selection::new(3, 13, 3, 13),
                        Selection::new(4, 12, 4, 12),
                        Selection::new(5, 6, 5, 6),
                    ],
                );
                assert_cursor(
                    view_model,
                    vec![
                        Selection::new(1, 13, 1, 13),
                        Selection::new(2, 16, 2, 16),
                        Selection::new(3, 13, 3, 13),
                        Selection::new(4, 12, 4, 12),
                        Selection::new(5, 6, 5, 6),
                    ],
                );

                move_right(editor, view_model, true);
                assert_cursor(
                    view_model,
                    vec![
                        Selection::new(1, 13, 1, 14),
                        Selection::new(2, 16, 2, 17),
                        Selection::new(3, 13, 3, 14),
                        Selection::new(4, 12, 4, 13),
                        Selection::new(5, 6, 5, 7),
                    ],
                );

                move_right(editor, view_model, true);
                assert_cursor(
                    view_model,
                    vec![
                        Selection::new(1, 13, 1, 15),
                        Selection::new(2, 16, 2, 18),
                        Selection::new(3, 13, 3, 15),
                        Selection::new(4, 12, 4, 14),
                        Selection::new(5, 6, 5, 8),
                    ],
                );

                move_right(editor, view_model, true);
                assert_cursor(
                    view_model,
                    vec![
                        Selection::new(1, 13, 1, 16),
                        Selection::new(2, 16, 2, 19),
                        Selection::new(3, 13, 3, 16),
                        Selection::new(4, 12, 4, 15),
                        Selection::new(5, 6, 5, 9),
                    ],
                );

                move_right(editor, view_model, true);
                assert_cursor(
                    view_model,
                    vec![
                        Selection::new(1, 13, 1, 17),
                        Selection::new(2, 16, 2, 20),
                        Selection::new(3, 13, 3, 17),
                        Selection::new(4, 12, 4, 16),
                        Selection::new(5, 6, 5, 10),
                    ],
                );
            },
        );
    });
}

#[test]
fn editor_controller_issue_41573_delete_across_multiple_lines_does_not_shrink_the_selection_when_word_wraps() {
    run_test(|f| {
        f.with_test_code_editor(
            ["Authorization: 'Bearer pHKRfCTFSnGxs6akKlb9ddIXcca0sIUSZJutPHYqz7vEeHdMTMh0SGN0IGU3a0n59DXjTLRsj5EJ2u33qLNIFi9fk5XF8pK39PndLYUZhPt4QvHGLScgSkK0L4gwzkzMloTQPpKhqiikiIOvyNNSpd2o8j29NnOmdTUOKi9DVt74PD2ohKxyOrWZ6oZprTkb3eKajcpnS0LABKfaw2rmv4',"].join("\n"),
            &word_wrap_options(100),
            |editor, view_model| {
                move_to(editor, view_model, 1, 43, false);
                move_to(editor, view_model, 1, 147, true);
                assert_cursor(view_model, Selection::new(1, 43, 1, 147));

                editor
                    .get_model()
                    .expect("o editor tem o modelo")
                    .apply_edits(&[EditOperation::replace(Range::new(1, 1, 1, 43), Some(JsString::from("")))]);

                assert_cursor(view_model, Selection::new(1, 1, 1, 105));
            },
        );
    });
}

#[test]
fn editor_controller_issue_22717_moving_text_cursor_cause_an_incorrect_position_in_chinese() {
    run_test(|f| {
        // a single model line => 4 view lines
        f.with_test_code_editor(["一二三四五六七八九十\n12345678901234567890"], &no_options(), |editor, view_model| {
            set_test_selections(view_model, &[Selection::new(1, 5, 1, 5)]);

            move_down(editor, view_model, false);
            assert_cursor(view_model, Selection::new(2, 9, 2, 9));

            move_right(editor, view_model, false);
            assert_cursor(view_model, Selection::new(2, 10, 2, 10));

            move_right(editor, view_model, false);
            assert_cursor(view_model, Selection::new(2, 11, 2, 11));

            move_up(editor, view_model, false);
            assert_cursor(view_model, Selection::new(1, 6, 1, 6));
        });
    });
}

#[test]
fn editor_controller_issue_112301_new_sticky_tab_stops_feature_interferes_with_word_wrap() {
    run_test(|f| {
        let options = Dynamic::object([
            ("wordWrap", Dynamic::from("wordWrapColumn")),
            ("wordWrapColumn", Dynamic::from(32)),
            ("stickyTabStops", Dynamic::from(true)),
        ]);
        f.with_test_code_editor(
            [["function hello() {", "        console.log(`this is a long console message`)", "}"].join("\n")],
            &options,
            |editor, view_model| {
                set_test_selections(view_model, &[Selection::new(2, 31, 2, 31)]);
                move_right(editor, view_model, false);
                assert_cursor(view_model, Position::new(2, 32));

                move_right(editor, view_model, false);
                assert_cursor(view_model, Position::new(2, 33));

                move_right(editor, view_model, false);
                assert_cursor(view_model, Position::new(2, 34));

                move_left(editor, view_model, false);
                assert_cursor(view_model, Position::new(2, 33));

                move_left(editor, view_model, false);
                assert_cursor(view_model, Position::new(2, 32));

                move_left(editor, view_model, false);
                assert_cursor(view_model, Position::new(2, 31));
            },
        );
    });
}

#[test]
fn editor_controller_issue_44805_should_not_be_able_to_undo_in_readonly_editor() {
    run_test(|f| {
        let editing = core_editing_commands::instance();
        let model = f.create_text_model([""].join("\n"), None, Default::default(), None);

        let options = Dynamic::object([("readOnly", Dynamic::from(true))]);
        f.with_test_code_editor(model.clone(), &options, |editor, _view_model| {
            let mut cursor_state_computer = |_: &[ValidEditOperation]| -> Option<Vec<Selection>> { Some(vec![Selection::new(1, 1, 1, 1)]) };
            model.push_edit_operations(
                Some(vec![Selection::new(1, 1, 1, 1)]),
                &[EditOperation::replace(Range::new(1, 1, 1, 1), Some(JsString::from("Hello world!")))],
                Some(&mut cursor_state_computer),
                None,
                None,
            );
            assert_eq!(get_value_lf(&model), "Hello world!");

            editor.run_command(&*editing.undo, None);
            assert_eq!(get_value_lf(&model), "Hello world!");
        });
    });
}

#[test]
fn editor_controller_issue_46314_view_model_is_out_of_sync_with_model() {
    run_test(|f| {
        let language_id = "modelModeTest1";
        let language_registration = tokenization_registry().register(language_id, Rc::new(EmptyTokenizationSupport));
        let model = f.create_text_model("Just text", Some(language_id), Default::default(), None);

        f.with_test_code_editor(model.clone(), &no_options(), |editor1, _cursor1| {
            f.with_test_code_editor(model.clone(), &no_options(), |_editor2, _cursor2| {
                let listener_model = model.clone();
                let disposable = editor1.on_did_change_cursor_position().listen(move |_| {
                    listener_model.tokenization().tokenize_if_cheap(1);
                });

                model.apply_edits(&[EditOperation::replace(Range::new(1, 1, 1, 1), Some(JsString::from("-")))]);

                disposable.dispose();
            });
        });

        language_registration.dispose();
        model.dispose();
    });
}

#[test]
fn editor_controller_issue_37967_problem_replacing_consecutive_characters() {
    run_test(|f| {
        let editing = core_editing_commands::instance();
        let model = f.create_text_model(["const a = \"foo\";", "const b = \"\""].join("\n"), None, Default::default(), None);

        let options = Dynamic::object([("multiCursorMergeOverlapping", Dynamic::from(false))]);
        f.with_test_code_editor(model.clone(), &options, |editor, view_model| {
            editor.set_selections(
                &[Selection::new(1, 12, 1, 12), Selection::new(1, 16, 1, 16), Selection::new(2, 12, 2, 12), Selection::new(2, 13, 2, 13)],
                None,
                None,
            );

            editor.run_command(&*editing.delete_left, None);

            assert_cursor(
                view_model,
                vec![
                    Selection::new(1, 11, 1, 11),
                    Selection::new(1, 14, 1, 14),
                    Selection::new(2, 11, 2, 11),
                    Selection::new(2, 11, 2, 11),
                ],
            );

            type_text(view_model, "'", Some("keyboard"));

            assert_eq!(get_line_content(&model, 1), "const a = 'foo';");
            assert_eq!(get_line_content(&model, 2), "const b = ''");
        });
    });
}

#[test]
fn editor_controller_issue_15761_cursor_doesnt_move_in_a_redo_operation() {
    run_test(|f| {
        let editing = core_editing_commands::instance();
        let model = f.create_text_model(["hello"].join("\n"), None, Default::default(), None);

        f.with_test_code_editor(model, &no_options(), |editor, view_model| {
            editor.set_selections(&[Selection::new(1, 4, 1, 4)], None, None);

            editor.execute_edits(
                ExecuteEditsSource::Name(Some("test")),
                vec![SingleEditOperation::new(Range::new(1, 1, 1, 1), Some(JsString::from("*")), true)],
                None,
            );
            assert_cursor(view_model, vec![Selection::new(1, 5, 1, 5)]);

            editor.run_command(&*editing.undo, None);
            assert_cursor(view_model, vec![Selection::new(1, 4, 1, 4)]);

            editor.run_command(&*editing.redo, None);
            assert_cursor(view_model, vec![Selection::new(1, 5, 1, 5)]);
        });
    });
}

#[test]
fn editor_controller_issue_42783_api_calls_with_undo_leave_cursor_in_wrong_position() {
    run_test(|f| {
        let editing = core_editing_commands::instance();
        let model = f.create_text_model(["ab"].join("\n"), None, Default::default(), None);

        f.with_test_code_editor(model, &no_options(), |editor, view_model| {
            editor.set_selections(&[Selection::new(1, 1, 1, 1)], None, None);

            editor.execute_edits(
                ExecuteEditsSource::Name(Some("test")),
                vec![SingleEditOperation::new(Range::new(1, 1, 1, 3), Some(JsString::from("")), false)],
                None,
            );
            assert_cursor(view_model, vec![Selection::new(1, 1, 1, 1)]);

            editor.run_command(&*editing.undo, None);
            assert_cursor(view_model, vec![Selection::new(1, 1, 1, 1)]);

            editor.execute_edits(
                ExecuteEditsSource::Name(Some("test")),
                vec![SingleEditOperation::new(Range::new(1, 1, 1, 2), Some(JsString::from("")), false)],
                None,
            );
            assert_cursor(view_model, vec![Selection::new(1, 1, 1, 1)]);
        });
    });
}

#[test]
fn editor_controller_issue_85712_paste_line_moves_cursor_to_start_of_current_line_rather_than_start_of_next_line() {
    run_test(|f| {
        let model = f.create_text_model(["abc123", ""].join("\n"), None, Default::default(), None);

        f.with_test_code_editor(model.clone(), &no_options(), |editor, view_model| {
            editor.set_selections(&[Selection::new(2, 1, 2, 1)], None, None);
            view_model.paste(&u("something\n"), true, None, None, false);
            assert_eq!(get_value(&model), ["abc123", "something", ""].join("\n"));
            assert_cursor(view_model, Position::new(3, 1));
        });
    });
}

#[test]
fn editor_controller_issue_84897_left_delete_behavior_in_some_languages_is_changed() {
    run_test(|f| {
        let editing = core_editing_commands::instance();
        let model = f.create_text_model(["สวัสดี"].join("\n"), None, Default::default(), None);

        f.with_test_code_editor(model.clone(), &no_options(), |editor, _view_model| {
            editor.set_selections(&[Selection::new(1, 7, 1, 7)], None, None);

            editor.run_command(&*editing.delete_left, None);
            assert_eq!(get_value_lf(&model), "สวัสด");

            editor.run_command(&*editing.delete_left, None);
            assert_eq!(get_value_lf(&model), "สวัส");

            editor.run_command(&*editing.delete_left, None);
            assert_eq!(get_value_lf(&model), "สวั");

            editor.run_command(&*editing.delete_left, None);
            assert_eq!(get_value_lf(&model), "สว");

            editor.run_command(&*editing.delete_left, None);
            assert_eq!(get_value_lf(&model), "ส");

            editor.run_command(&*editing.delete_left, None);
            assert_eq!(get_value_lf(&model), "");
        });
    });
}
