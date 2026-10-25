//! Porte de `src/vs/editor/standalone/test/browser/monarch.test.ts` (suíte `Monarch`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - As regexes literais do original (`/.../`) são o `source` delas, em texto cru; o `compile` só lê o `source`.
//! - O objeto `IMonarchLanguage` é o [`MonarchLanguage`]; as regras em lista, em objeto e as ações em texto, objeto,
//!   lista e `cases` são as variantes de [`MonarchRule`] e [`MonarchAction`].
//! - `new StandaloneConfigurationService(new NullLogService())` é o [`StandaloneConfigurationService`] com o
//!   `NullLogService`; o `await updateValue` é a chamada direta (a promessa já nasce resolvida).
//! - `new MonarchTokenizer(languageService, null!, ...)` passa `None` no serviço de tema (o caminho clássico não o lê).
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o `LeakGuard` criado no começo de cada caso.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::JsString;
use je_base::dynamic::Dynamic;
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_editor::language_service::{ILanguageService, LanguageService};
use je_editor::languages::{ITokenizationSupport, Token};
use je_editor::modes_registry::LanguageExtensionPoint;
use je_editor::monarch::compile::compile;
use je_editor::monarch::lexer::MonarchTokenizer;
use je_editor::monarch::types::{MonarchAction, MonarchAttr, MonarchLanguage, MonarchRule, MonarchTokenAction};
use je_editor::standalone_configuration_service::StandaloneConfigurationService;
use je_editor::tokenization_registry::tokenization_registry;
use je_platform::configuration::service::IConfigurationService;
use je_platform::log::NullLogService;
use zyarr::RegExp;

fn create_monarch_tokenizer(
    language_service: &Rc<LanguageService>,
    language_id: &str,
    language: MonarchLanguage,
    configuration_service: &Rc<StandaloneConfigurationService>,
) -> Rc<MonarchTokenizer> {
    let lexer = compile(language_id, &language).expect("a linguagem compila");
    MonarchTokenizer::new(language_service.clone(), None, language_id, lexer, configuration_service.clone())
}

fn get_tokens(tokenizer: &MonarchTokenizer, lines: &[&str]) -> Vec<Vec<Token>> {
    let mut actual_tokens: Vec<Vec<Token>> = Vec::new();
    let mut state = tokenizer.get_initial_state();
    for line in lines {
        let result = tokenizer.tokenize(&JsString::from(*line), true, &state);
        actual_tokens.push(result.tokens);
        state = result.end_state;
    }
    actual_tokens
}

/// `[regex, action]`.
fn rule(regex: &str, action: MonarchAction) -> MonarchRule {
    MonarchRule::Tuple { regex: regex.to_owned(), action, next: None }
}

/// `[regex, action, next]`.
fn rule_next(regex: &str, action: MonarchAction, next: &str) -> MonarchRule {
    MonarchRule::Tuple { regex: regex.to_owned(), action, next: Some(next.to_owned()) }
}

/// `{ regex, action }`.
fn rule_object(regex: &str, action: MonarchAction) -> MonarchRule {
    MonarchRule::Object { regex: Some(regex.to_owned()), action, name: None }
}

fn str_action(token: &str) -> MonarchAction {
    MonarchAction::Str(token.to_owned())
}

/// `{ token, next?, nextEmbedded?, switchTo? }`.
fn token_action(token: &str, next: Option<&str>, next_embedded: Option<&str>, switch_to: Option<&str>) -> MonarchAction {
    MonarchAction::Token(MonarchTokenAction {
        token: Some(token.to_owned()),
        next: next.map(str::to_owned),
        next_embedded: next_embedded.map(str::to_owned),
        switch_to: switch_to.map(str::to_owned),
        ..MonarchTokenAction::default()
    })
}

const SQL_QUERY_START: &str = "(SELECT|INSERT|UPDATE|DELETE|CREATE|REPLACE|ALTER|WITH)";

/// A gramática do `sql` registrada antes do tokenizador de teste: `root: [[/./, 'token']]`.
fn register_sql(disposables: &DisposableStore, language_service: &Rc<LanguageService>, configuration_service: &Rc<StandaloneConfigurationService>) {
    disposables.add(language_service.register_language(LanguageExtensionPoint::new("sql")));
    let sql = create_monarch_tokenizer(language_service, "sql", MonarchLanguage::new(vec![("root", vec![rule(".", str_action("token"))])]), configuration_service);
    let sql = disposables.add_rc(sql);
    disposables.add(tokenization_registry().register("sql", sql));
}

fn embedded_sql_lines() -> [&'static str; 6] {
    [
        "mysql_query(\"\"\"SELECT * FROM table_name WHERE ds = '<DATEID>'\"\"\")",
        "mysql_query(\"\"\"",
        "SELECT *",
        "FROM table_name",
        "WHERE ds = '<DATEID>'",
        "\"\"\")",
    ]
}

fn embedded_sql_expected_tokens() -> Vec<Vec<Token>> {
    vec![
        vec![
            Token::new(0, "source.test1", "test1"),
            Token::new(12, "string.quote.test1", "test1"),
            Token::new(15, "token.sql", "sql"),
            Token::new(61, "string.quote.test1", "test1"),
            Token::new(64, "source.test1", "test1"),
        ],
        vec![Token::new(0, "source.test1", "test1"), Token::new(12, "string.quote.test1", "test1")],
        vec![Token::new(0, "token.sql", "sql")],
        vec![Token::new(0, "token.sql", "sql")],
        vec![Token::new(0, "token.sql", "sql")],
        vec![Token::new(0, "string.quote.test1", "test1"), Token::new(3, "source.test1", "test1")],
    ]
}

/// As regras de `root`, `maybeStringIsSQL` e `endDblDocString` que os dois primeiros casos dividem.
fn embedded_sql_states() -> Vec<(&'static str, Vec<MonarchRule>)> {
    vec![
        (
            "root",
            vec![
                rule(
                    &format!("(\"\"\"){SQL_QUERY_START}"),
                    MonarchAction::Group(vec![
                        token_action("string.quote", None, None, None),
                        token_action("@rematch", Some("@endStringWithSQL"), Some("sql"), None),
                    ]),
                ),
                rule("(\"\"\")$", MonarchAction::Group(vec![token_action("string.quote", Some("@maybeStringIsSQL"), None, None)])),
            ],
        ),
        (
            "maybeStringIsSQL",
            vec![rule(
                "(.*)",
                MonarchAction::Cases(vec![
                    (format!("{SQL_QUERY_START}\\b.*"), token_action("@rematch", Some("@endStringWithSQL"), Some("sql"), None)),
                    ("@default".to_owned(), token_action("@rematch", None, None, Some("@endDblDocString"))),
                ]),
            )],
        ),
        (
            "endDblDocString",
            vec![
                rule("[^']+", str_action("string")),
                rule("\\\\'", str_action("string")),
                rule_next("'''", str_action("string"), "@popall"),
                rule("'", str_action("string")),
            ],
        ),
    ]
}

#[test]
fn ensure_rematch_and_next_embedded_can_be_used_together_in_monarch_grammar() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let disposables = DisposableStore::new();
    let language_service = disposables.add_rc(LanguageService::new(false));
    let configuration_service = StandaloneConfigurationService::new(NullLogService::shared());
    register_sql(&disposables, &language_service, &configuration_service);

    let mut states = embedded_sql_states();
    states.push(("endStringWithSQL", vec![rule("\"\"\"", token_action("string.quote", Some("@popall"), Some("@pop"), None))]));
    let tokenizer = disposables.add_rc(create_monarch_tokenizer(&language_service, "test1", MonarchLanguage::new(states), &configuration_service));

    let actual_tokens = get_tokens(&tokenizer, &embedded_sql_lines());

    assert_eq!(actual_tokens, embedded_sql_expected_tokens());
    disposables.dispose();
}

#[test]
fn test_next_embedded_pop_in_cases_statement() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let disposables = DisposableStore::new();
    let language_service = disposables.add_rc(LanguageService::new(false));
    let configuration_service = StandaloneConfigurationService::new(NullLogService::shared());
    register_sql(&disposables, &language_service, &configuration_service);

    let mut states = embedded_sql_states();
    states.push((
        "endStringWithSQL",
        vec![rule(
            "\"\"\"",
            MonarchAction::Cases(vec![
                (
                    "\"\"\"".to_owned(),
                    MonarchAction::Cases(vec![("".to_owned(), token_action("string.quote", Some("@popall"), Some("@pop"), None))]),
                ),
                ("@default".to_owned(), str_action("")),
            ]),
        )],
    ));
    let tokenizer = disposables.add_rc(create_monarch_tokenizer(&language_service, "test1", MonarchLanguage::new(states), &configuration_service));

    let actual_tokens = get_tokens(&tokenizer, &embedded_sql_lines());

    assert_eq!(actual_tokens, embedded_sql_expected_tokens());
    disposables.dispose();
}

#[test]
fn monaco_editor_1235_empty_line_handling() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let disposables = DisposableStore::new();
    let configuration_service = StandaloneConfigurationService::new(NullLogService::shared());
    let language_service = disposables.add_rc(LanguageService::new(false));
    let tokenizer = disposables.add_rc(create_monarch_tokenizer(
        &language_service,
        "test",
        MonarchLanguage::new(vec![
            ("root", vec![MonarchRule::Include("@comments".to_owned())]),
            (
                "comments",
                vec![
                    rule(r"\/\/$", str_action("comment")),      // empty single-line comment
                    rule_next(r"\/\/", str_action("comment"), "@comment_cpp"),
                ],
            ),
            (
                "comment_cpp",
                vec![
                    rule_next(r"(?:[^\\]|(?:\\.))+$", str_action("comment"), "@pop"),
                    rule(r".+$", str_action("comment")),
                    rule_next(r"$", str_action("comment"), "@pop"),
                    // No possible rule to detect an empty line and @pop?
                ],
            ),
        ]),
        &configuration_service,
    ));

    let lines = [
        "// This comment \\",
        "   continues on the following line",
        "",
        "// This comment does NOT continue \\\\",
        "   because the escape char was itself escaped",
        "",
        "// This comment DOES continue because \\\\\\",
        "   the 1st '\\' escapes the 2nd; the 3rd escapes EOL",
        "",
        "// This comment continues to the following line \\",
        "",
        "But the line was empty. This line should not be commented.",
    ];

    let actual_tokens = get_tokens(&tokenizer, &lines);

    assert_eq!(
        actual_tokens,
        vec![
            vec![Token::new(0, "comment.test", "test")],
            vec![Token::new(0, "comment.test", "test")],
            vec![],
            vec![Token::new(0, "comment.test", "test")],
            vec![Token::new(0, "source.test", "test")],
            vec![],
            vec![Token::new(0, "comment.test", "test")],
            vec![Token::new(0, "comment.test", "test")],
            vec![],
            vec![Token::new(0, "comment.test", "test")],
            vec![],
            vec![Token::new(0, "source.test", "test")],
        ]
    );

    disposables.dispose();
}

#[test]
fn monaco_editor_2265_exit_a_state_at_end_of_line() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let disposables = DisposableStore::new();
    let configuration_service = StandaloneConfigurationService::new(NullLogService::shared());
    let language_service = disposables.add_rc(LanguageService::new(false));
    let mut language = MonarchLanguage::new(vec![
        (
            "root",
            vec![
                rule_next(r"^\*", str_action(""), "@inner"),
                rule_next(r"\:\*", str_action(""), "@inner"),
                rule(r"[^*:]+", str_action("string")),
                rule(r"[*:]", str_action("string")),
            ],
        ),
        (
            "inner",
            vec![rule_next(r"\n", str_action(""), "@pop"), rule(r"\d+", str_action("number")), rule(r"[^\d]+", str_action(""))],
        ),
    ]);
    language.include_lf = Some(true);
    let tokenizer = disposables.add_rc(create_monarch_tokenizer(&language_service, "test", language, &configuration_service));

    let lines = ["PRINT 10 * 20", "*FX200, 3", "PRINT 2*3:*FX200, 3"];

    let actual_tokens = get_tokens(&tokenizer, &lines);

    assert_eq!(
        actual_tokens,
        vec![
            vec![Token::new(0, "string.test", "test")],
            vec![
                Token::new(0, "", "test"),
                Token::new(3, "number.test", "test"),
                Token::new(6, "", "test"),
                Token::new(8, "number.test", "test"),
            ],
            vec![
                Token::new(0, "string.test", "test"),
                Token::new(9, "", "test"),
                Token::new(13, "number.test", "test"),
                Token::new(16, "", "test"),
                Token::new(18, "number.test", "test"),
            ],
        ]
    );

    disposables.dispose();
}

#[test]
fn issue_115662_monarch_compile_function_need_an_extra_option_which_can_control_replacement() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let disposables = DisposableStore::new();
    let configuration_service = StandaloneConfigurationService::new(NullLogService::shared());
    let language_service = disposables.add_rc(LanguageService::new(false));

    // `/@\w+/.test('@ham') ? new RegExp(`^${'@uselessReplaceKey1'}$`) : new RegExp(`^${'@ham'}$`)`
    let ham: Vec<u16> = "@ham".encode_utf16().collect();
    let regex_source = if RegExp::from_str(r"@\w+", "").expect("regex válida").exec(&ham, 0).is_some() { "^@uselessReplaceKey1$" } else { "^@ham$" };
    let mut language1 = MonarchLanguage::new(vec![("root", vec![rule_object(regex_source, token_action("ham", None, None, None))])])
        .with_attribute("uselessReplaceKey1", MonarchAttr::Str("@uselessReplaceKey2".to_owned()))
        .with_attribute("uselessReplaceKey2", MonarchAttr::Str("@uselessReplaceKey3".to_owned()))
        .with_attribute("uselessReplaceKey3", MonarchAttr::Str("@uselessReplaceKey4".to_owned()))
        .with_attribute("uselessReplaceKey4", MonarchAttr::Str("@uselessReplaceKey5".to_owned()))
        .with_attribute("uselessReplaceKey5", MonarchAttr::Str("@ham".to_owned()));
    language1.ignore_case = Some(false);
    let tokenizer1 = disposables.add_rc(create_monarch_tokenizer(&language_service, "test", language1, &configuration_service));

    let mut language2 = MonarchLanguage::new(vec![("root", vec![rule_object("@@ham", token_action("ham", None, None, None))])]);
    language2.ignore_case = Some(false);
    let tokenizer2 = disposables.add_rc(create_monarch_tokenizer(&language_service, "test", language2, &configuration_service));

    let lines = ["@ham"];

    let actual_tokens1 = get_tokens(&tokenizer1, &lines);
    assert_eq!(actual_tokens1, vec![vec![Token::new(0, "ham.test", "test")]]);

    let actual_tokens2 = get_tokens(&tokenizer2, &lines);
    assert_eq!(actual_tokens2, vec![vec![Token::new(0, "ham.test", "test")]]);

    disposables.dispose();
}

#[test]
fn monaco_editor_2424_allow_to_target_double_at() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let disposables = DisposableStore::new();
    let configuration_service = StandaloneConfigurationService::new(NullLogService::shared());
    let language_service = disposables.add_rc(LanguageService::new(false));

    let mut language = MonarchLanguage::new(vec![("root", vec![rule_object("@@@@", token_action("ham", None, None, None))])]);
    language.ignore_case = Some(false);
    let tokenizer = disposables.add_rc(create_monarch_tokenizer(&language_service, "test", language, &configuration_service));

    let lines = ["@@"];

    let actual_tokens = get_tokens(&tokenizer, &lines);
    assert_eq!(actual_tokens, vec![vec![Token::new(0, "ham.test", "test")]]);

    disposables.dispose();
}

#[test]
fn monaco_editor_3025_check_max_tokenization_line_length_before_tokenizing() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let disposables = DisposableStore::new();

    let configuration_service = StandaloneConfigurationService::new(NullLogService::shared());
    let language_service = disposables.add_rc(LanguageService::new(false));

    // Set maxTokenizationLineLength to 4 so that "ham" works but "hamham" would fail
    let _ = configuration_service.update_value("editor.maxTokenizationLineLength", Dynamic::Number(4.0), None, None);

    let tokenizer = disposables.add_rc(create_monarch_tokenizer(
        &language_service,
        "test",
        MonarchLanguage::new(vec![("root", vec![rule_object("ham", token_action("ham", None, None, None))])]),
        &configuration_service,
    ));

    let lines = [
        "ham",    // length 3, should be tokenized
        "hamham", // length 6, should NOT be tokenized
    ];

    let actual_tokens = get_tokens(&tokenizer, &lines);
    assert_eq!(actual_tokens, vec![vec![Token::new(0, "ham.test", "test")], vec![Token::new(0, "", "test")]]);

    disposables.dispose();
}

/// A gramática dos dois últimos casos: a string crua do C++ 11, com `@encoding` e `$S2` do estado.
fn cpp_raw_string_language() -> MonarchLanguage {
    let mut language = MonarchLanguage::new(vec![
        (
            "root",
            vec![
                // C++ 11 Raw String
                rule(r#"@encoding?R\"(?:([^ ()\\\t]*))\("#, token_action("string.raw.begin", Some("@raw.$1"), None, None)),
            ],
        ),
        ("raw", vec![rule_next(r#".*\)$S2\""#, str_action("string.raw"), "@pop"), rule(".*", str_action("string.raw"))]),
    ])
    .with_attribute("encoding", MonarchAttr::Re("u|u8|U|L".to_owned()));
    language.ignore_case = Some(false);
    language
}

#[test]
fn monaco_editor_3128_allow_state_access_within_rules() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let disposables = DisposableStore::new();
    let configuration_service = StandaloneConfigurationService::new(NullLogService::shared());
    let language_service = disposables.add_rc(LanguageService::new(false));

    let tokenizer = disposables.add_rc(create_monarch_tokenizer(&language_service, "test", cpp_raw_string_language(), &configuration_service));

    let lines = ["int main(){", "", "\tauto s = R\"\"\"\"(", "\tHello World", "\t)\"\"\"\";", "", "\tstd::cout << \"hello\";", "", "}"];

    let actual_tokens = get_tokens(&tokenizer, &lines);
    assert_eq!(
        actual_tokens,
        vec![
            vec![Token::new(0, "source.test", "test")],
            vec![],
            vec![Token::new(0, "source.test", "test"), Token::new(10, "string.raw.begin.test", "test")],
            vec![Token::new(0, "string.raw.test", "test")],
            vec![Token::new(0, "string.raw.test", "test"), Token::new(6, "source.test", "test")],
            vec![],
            vec![Token::new(0, "source.test", "test")],
            vec![],
            vec![Token::new(0, "source.test", "test")],
        ]
    );

    disposables.dispose();
}

#[test]
fn monaco_editor_4775_raw_strings_in_cpp_can_break_monarch() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let disposables = DisposableStore::new();
    let configuration_service = StandaloneConfigurationService::new(NullLogService::shared());
    let language_service = disposables.add_rc(LanguageService::new(false));

    let tokenizer = disposables.add_rc(create_monarch_tokenizer(&language_service, "test", cpp_raw_string_language(), &configuration_service));

    let lines = ["R\"[())\""];

    let actual_tokens = get_tokens(&tokenizer, &lines);
    assert_eq!(actual_tokens, vec![vec![Token::new(0, "string.raw.begin.test", "test"), Token::new(4, "string.raw.test", "test")]]);

    disposables.dispose();
}
