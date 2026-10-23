//! Porte de `src/vs/platform/environment/test/node/argv.test.ts` (suítes `formatOptions` e `parseArgs`), caso a caso e na mesma ordem.
//!
//! Divergências: `OptionDescriptions<T>` é a lista ordenada [`OptionDescriptions`] e o resultado de
//! `parseArgs` é o objeto dinâmico, comparado com `deepStrictEqual` sobre literais no estilo do TypeScript
//! (`j("{...}")`). `ensureNoDisposablesAreLeakedInTestSuite` não tem o que vigiar (nada aqui é descartável).

mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::{assert_deep_strict_equal, j};
use je_platform::environment::argv::{
    ErrorReporter, OptionDescription, OptionDescriptions, OptionType, Subcommand, format_options, option, parse_args,
};
use je_platform::environment::argv_helper::add_arg;

fn o(description: &str, type_: OptionType) -> (OptionType, String) {
    (type_, description.to_owned())
}

fn opt(id: &str, (type_, description): (OptionType, String)) -> (String, OptionDescription) {
    (id.to_owned(), option(type_).description(&description).into())
}

fn c(id: &str, description: &str, options: OptionDescriptions) -> (String, OptionDescription) {
    (id.to_owned(), OptionDescription::Subcommand(Subcommand { description: Some(description.to_owned()), deprecation_message: None, options }))
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|item| (*item).to_owned()).collect()
}

#[test]
fn text_should_display_small_columns_correctly() {
    assert_eq!(format_options(&vec![opt("add", o("bar", OptionType::String))], 80), strings(&["  --add        bar"]));
    assert_eq!(
        format_options(&vec![opt("add", o("bar", OptionType::String)), opt("wait", o("ba", OptionType::String)), opt("trace", o("b", OptionType::String))], 80),
        strings(&["  --add        bar", "  --wait       ba", "  --trace      b"])
    );
}

#[test]
fn text_should_wrap() {
    assert_eq!(
        format_options(&vec![opt("add", o(&"bar ".repeat(9), OptionType::String))], 40),
        strings(&["  --add        bar bar bar bar bar bar", "               bar bar bar"])
    );
}

#[test]
fn text_should_revert_to_the_condensed_view_when_the_terminal_is_too_narrow() {
    assert_eq!(
        format_options(&vec![opt("add", o(&"bar ".repeat(9), OptionType::String))], 30),
        strings(&["  --add", "      bar bar bar bar bar bar bar bar bar "])
    );
}

#[test]
fn add_arg_test() {
    assert_eq!(add_arg(vec![], &["foo"]), strings(&["foo"]));
    assert_eq!(add_arg(vec![], &["foo", "bar"]), strings(&["foo", "bar"]));
    assert_eq!(add_arg(strings(&["foo"]), &["bar"]), strings(&["foo", "bar"]));
    assert_eq!(add_arg(strings(&["--wait"]), &["bar"]), strings(&["--wait", "bar"]));
    assert_eq!(add_arg(strings(&["--wait", "--", "--foo"]), &["bar"]), strings(&["--wait", "bar", "--", "--foo"]));
    assert_eq!(add_arg(strings(&["--", "--foo"]), &["bar"]), strings(&["bar", "--", "--foo"]));
}

#[test]
fn format_options_subcommands() {
    assert_eq!(
        format_options(&vec![c("testcmd", "A test command", vec![opt("add", o("A test command option", OptionType::String))])], 30),
        strings(&["  --testcmd", "      A test command"])
    );
}

/// `newErrorReporter(result, command)`.
struct TestReporter {
    result: Rc<RefCell<Vec<String>>>,
    command_prefix: String,
}

impl TestReporter {
    fn new(result: &Rc<RefCell<Vec<String>>>, command: &str) -> Self {
        Self { result: result.clone(), command_prefix: if command.is_empty() { String::new() } else { format!("{command}-") } }
    }

    fn push(&self, text: String) {
        self.result.borrow_mut().push(format!("{}{text}", self.command_prefix));
    }
}

impl ErrorReporter for TestReporter {
    fn on_deprecated_option(&self, deprecated_id: &str, _message: &str) {
        self.push(format!("onDeprecatedOption {deprecated_id}"));
    }

    fn on_unknown_option(&self, id: &str) {
        self.push(format!("onUnknownOption {id}"));
    }

    fn on_empty_value(&self, id: &str) {
        self.push(format!("onEmptyValue {id}"));
    }

    fn on_multiple_values(&self, id: &str, used_value: &str) {
        self.push(format!("onMultipleValues {id} {used_value}"));
    }

    fn get_subcommand_reporter(&self, command: &str) -> Option<Box<dyn ErrorReporter>> {
        Some(Box::new(Self::new(&self.result, &format!("{}{command}", self.command_prefix.trim_end_matches('-')))))
    }
}

fn assert_parse(options: &OptionDescriptions, input: &[&str], expected: &str, expected_errors: &[&str]) {
    let result = Rc::new(RefCell::new(Vec::new()));
    let error_reporter = TestReporter::new(&result, "");
    assert_deep_strict_equal(&parse_args(&strings(input), options, &error_reporter), &j(expected));
    assert_eq!(*result.borrow(), strings(expected_errors));
}

fn typed(id: &str, type_: OptionType) -> (String, OptionDescription) {
    (id.to_owned(), option(type_).into())
}

#[test]
fn no_key_with_boolean_no_key_and_string_key_does_not_trigger_empty_value_error() {
    // Regression test for https://github.com/microsoft/vscode/issues/199664
    // '--no-proxy-server' should set 'no-proxy-server=true' without triggering
    // an empty value error for 'proxy-server'
    let options = vec![typed("no-proxy-server", OptionType::Boolean), typed("proxy-server", OptionType::String), typed("_", OptionType::StringArray)];

    assert_parse(&options, &["--no-proxy-server"], "{'no-proxy-server': true, '_': []}", &[]);
    assert_parse(
        &options,
        &["--proxy-server=http://proxy.example.com:8080"],
        "{'no-proxy-server': false, 'proxy-server': 'http://proxy.example.com:8080', '_': []}",
        &[],
    );
    assert_parse(
        &options,
        &["--no-proxy-server", "--proxy-server=http://proxy.example.com:8080"],
        "{'no-proxy-server': true, 'proxy-server': 'http://proxy.example.com:8080', '_': []}",
        &[],
    );
    assert_parse(&options, &[], "{'no-proxy-server': false, '_': []}", &[]);
}

#[test]
fn no_key_with_boolean_no_key_correctly_sets_the_value_to_true() {
    let options = vec![typed("no-foo", OptionType::Boolean), typed("_", OptionType::StringArray)];

    assert_parse(&options, &["--no-foo"], "{'no-foo': true, '_': []}", &[]);
    assert_parse(&options, &[], "{'no-foo': false, '_': []}", &[]);
}

#[test]
fn parse_args_subcommands() {
    let options1 = vec![
        c("testcmd", "A test command", vec![opt("testArg", o("A test command option", OptionType::String)), typed("_", OptionType::StringArray)]),
        typed("_", OptionType::StringArray),
    ];
    assert_parse(&options1, &["testcmd", "--testArg=foo"], "{'testcmd': {'testArg': 'foo', '_': []}, '_': []}", &[]);
    assert_parse(
        &options1,
        &["testcmd", "--testArg=foo", "--testX"],
        "{'testcmd': {'testArg': 'foo', '_': []}, '_': []}",
        &["testcmd-onUnknownOption testX"],
    );

    assert_parse(
        &options1,
        &["--testArg=foo", "testcmd", "--testX"],
        "{'testcmd': {'testArg': 'foo', '_': []}, '_': []}",
        &["testcmd-onUnknownOption testX"],
    );

    assert_parse(&options1, &["--testArg=foo", "testcmd"], "{'testcmd': {'testArg': 'foo', '_': []}, '_': []}", &[]);

    assert_parse(&options1, &["--testArg", "foo", "testcmd"], "{'testcmd': {'testArg': 'foo', '_': []}, '_': []}", &[]);

    let options2 = vec![
        c("testcmd", "A test command", vec![opt("testArg", o("A test command option", OptionType::String))]),
        ("testX".to_owned(), option(OptionType::Boolean).global().description("").into()),
        typed("_", OptionType::StringArray),
    ];
    assert_parse(&options2, &["testcmd", "--testArg=foo", "--testX"], "{'testcmd': {'testArg': 'foo', 'testX': true, '_': []}, '_': []}", &[]);
}
