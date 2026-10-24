//! Porte de `src/vs/editor/contrib/snippet/test/browser/snippetVariables.test.ts` (suíte `Snippet Variables
//! Resolver`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `setup`/`teardown` e `ensureNoDisposablesAreLeakedInTestSuite()` são o `with_fixture`: cria o `LeakGuard`, o
//!   modelo de `file:///foo/files/text.txt` e o resolvedor composto, roda o caso e descarta o modelo.
//! - O `labelService` com `override getUriLabel` é o `ClosureLabelService` (em `tests/snippet_doubles`), e o
//!   `IWorkspaceContextService` cujos métodos lançam, menos `getWorkspace()`, é o `WorkspaceOnlyService`.
//! - `sinon.useFakeTimers({ now })` é o `with_faked_timers` com `set_system_time(now)` (o `Date.now()` do relógio
//!   virtual), e `clock.tick(ms)` é o `advance_fake_time(ms)`. Os campos locais da data esperada vêm do `jiff` no
//!   fuso do sistema, o mesmo do `new Date()` do original.
//! - `{ resolve(variable) { return varValue || variable.name; } }` e `{ resolve() { return '10'; } }` são os resolvedores
//!   `NameResolver` e `FixedResolver` deste arquivo.
//! - `() => null!` do clipboard é o mesmo `None` que `() => undefined`: o `Option` não distingue os dois.
//! - `isWindows` é `cfg!(windows)`; `sep` de `path` é `MAIN_SEPARATOR_STR`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod snippet_doubles;
mod support;

use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::JsString;
use je_base::runtime;
use je_base::uri::URI;
use je_editor::model::RelaxedTextModelCreationOptions;
use je_editor::selection::Selection;
use je_editor::snippet::snippet_parser::{MarkerRef, SnippetParser, VariableResolver};
use je_editor::snippet::snippet_variables::{
    ClipboardBasedVariableResolver, CompositeSnippetVariableResolver, ModelBasedVariableResolver, ReadClipboardText,
    SelectionBasedVariableResolver, TimeBasedVariableResolver, WorkspaceBasedVariableResolver,
};
use je_editor::text_model::TextModel;
use je_platform::resources::{ext_uri_biased_ignore_path_case, fs_path, uri_file};
use je_platform::workspace::{RawWorkspaceFolder, Workspace, to_workspace_folder};
use je_platform::workspaces::to_workspace_folders;
use jiff::Timestamp;
use jiff::tz::TimeZone;
use snippet_doubles::{ClosureLabelService, WorkspaceOnlyService};
use support::test_text_model::create_text_model_with;

fn sel(start_line_number: i32, start_column: i32, end_line_number: i32, end_column: i32) -> Selection {
    Selection::new(start_line_number, start_column, end_line_number, end_column)
}

fn utf16(value: &str) -> Vec<u16> {
    value.encode_utf16().collect()
}

/// `createTextModel(text, undefined, undefined, URI.parse(uri))`.
fn model_with_uri(text: &str, uri: &str) -> Rc<TextModel> {
    create_text_model_with(text, None, RelaxedTextModelCreationOptions::default(), Some(URI::parse(uri, false)))
}

/// O `labelService` do `setup`: `getUriLabel(uri) { return uri.fsPath; }`.
fn fs_path_label_service() -> Rc<ClosureLabelService> {
    ClosureLabelService::new(|uri, _| fs_path(uri))
}

/// `{ resolve(variable) { return varValue || variable.name; } }`.
struct NameResolver {
    value: Option<&'static str>,
}

impl VariableResolver for NameResolver {
    fn resolve(&self, variable: &MarkerRef) -> Option<JsString> {
        let name = &variable.as_variable().expect("Variable").name;
        Some(JsString::from(self.value.filter(|value| !value.is_empty()).unwrap_or(name.as_str())))
    }
}

/// `{ resolve() { return '10'; } }`.
struct FixedResolver(&'static str);

impl VariableResolver for FixedResolver {
    fn resolve(&self, _variable: &MarkerRef) -> Option<JsString> {
        Some(JsString::from(self.0))
    }
}

/// O que o `setup` deixa em variáveis da suíte.
struct Fixture {
    model: Rc<TextModel>,
    resolver: Rc<dyn VariableResolver>,
}

fn with_fixture(test: impl FnOnce(&Fixture)) {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    let model = model_with_uri(
        &["this is line one", "this is line two", "    this is line three"].join("\n"),
        "file:///foo/files/text.txt",
    );

    let resolver: Rc<dyn VariableResolver> = Rc::new(CompositeSnippetVariableResolver::new(vec![
        Rc::new(ModelBasedVariableResolver::new(fs_path_label_service(), model.clone())),
        Rc::new(SelectionBasedVariableResolver::new(model.clone(), sel(1, 1, 1, 1), 0, None)),
    ]));

    test(&Fixture { model: model.clone(), resolver });

    // teardown
    model.dispose();
}

/// `assertVariableResolve(resolver, varName, expected?)`.
fn assert_variable_resolve(resolver: &dyn VariableResolver, var_name: &str, expected: Option<&str>) {
    let snippet = SnippetParser::new().parse(&utf16(&format!("${var_name}")), false, false);
    let variable = snippet.children()[0].clone();
    variable.resolve_variable(resolver);
    if variable.children().is_empty() {
        assert_eq!(None, expected);
    } else {
        assert_eq!(Some(variable.to_js_string()), expected.map(JsString::from));
    }
}

/// `assertVariableResolve2(input, expected, varValue?)`.
fn assert_variable_resolve2(input: &str, expected: &str, var_value: Option<&'static str>) {
    let snippet = SnippetParser::new().parse(&utf16(input), false, false);
    snippet.resolve_variables(&NameResolver { value: var_value });

    assert_eq!(snippet.to_js_string(), expected);
}

/// `assertVariableResolve3(resolver, varName)`.
fn assert_variable_resolve3(resolver: &dyn VariableResolver, var_name: &str) {
    let snippet = SnippetParser::new().parse(&utf16(&format!("${var_name}")), false, false);
    let variable = snippet.children()[0].clone();

    assert!(variable.resolve_variable(resolver), "{var_name} failed to resolve");
}

/// `() => text`.
fn clipboard(text: Option<&'static str>) -> ReadClipboardText {
    Rc::new(move || text.map(JsString::from))
}

#[test]
fn editor_variables_basics() {
    with_fixture(|f| {
        assert_variable_resolve(&*f.resolver, "TM_FILENAME", Some("text.txt"));
        assert_variable_resolve(&*f.resolver, "something", None);
    });
}

#[test]
fn editor_variables_file_dir() {
    with_fixture(|f| {
        assert_variable_resolve(&*f.resolver, "TM_FILENAME", Some("text.txt"));
        if !cfg!(windows) {
            assert_variable_resolve(&*f.resolver, "TM_DIRECTORY", Some("/foo/files"));
            assert_variable_resolve(&*f.resolver, "TM_DIRECTORY_BASE", Some("files"));
            assert_variable_resolve(&*f.resolver, "TM_FILEPATH", Some("/foo/files/text.txt"));
        }

        let model = model_with_uri("", "http://www.pb.o/abc/def/ghi");
        let resolver = ModelBasedVariableResolver::new(fs_path_label_service(), model.clone());
        assert_variable_resolve(&resolver, "TM_FILENAME", Some("ghi"));
        if !cfg!(windows) {
            assert_variable_resolve(&resolver, "TM_DIRECTORY", Some("/abc/def"));
            assert_variable_resolve(&resolver, "TM_DIRECTORY_BASE", Some("def"));
            assert_variable_resolve(&resolver, "TM_FILEPATH", Some("/abc/def/ghi"));
        }
        model.dispose();

        let model = model_with_uri("", "mem:fff.ts");
        let resolver = ModelBasedVariableResolver::new(fs_path_label_service(), model.clone());
        assert_variable_resolve(&resolver, "TM_DIRECTORY", Some(""));
        assert_variable_resolve(&resolver, "TM_DIRECTORY_BASE", Some(""));
        assert_variable_resolve(&resolver, "TM_FILEPATH", Some("fff.ts"));
        model.dispose();
    });
}

#[test]
fn path_delimiters_in_code_snippet_variables_arent_specific_to_remote_os_76840() {
    with_fixture(|_| {
        let label_service = ClosureLabelService::new(|uri, _| fs_path(uri).replace(['/', '\\'], "|"));

        let model = model_with_uri("", "foo:///foo/files/text.txt");

        let resolver = CompositeSnippetVariableResolver::new(vec![Rc::new(ModelBasedVariableResolver::new(label_service, model.clone()))]);

        assert_variable_resolve(&resolver, "TM_FILEPATH", Some("|foo|files|text.txt"));

        model.dispose();
    });
}

#[test]
fn editor_variables_selection() {
    with_fixture(|f| {
        let mut resolver = SelectionBasedVariableResolver::new(f.model.clone(), sel(1, 2, 2, 3), 0, None);
        assert_variable_resolve(&resolver, "TM_SELECTED_TEXT", Some("his is line one\nth"));
        assert_variable_resolve(&resolver, "TM_CURRENT_LINE", Some("this is line two"));
        assert_variable_resolve(&resolver, "TM_LINE_INDEX", Some("1"));
        assert_variable_resolve(&resolver, "TM_LINE_NUMBER", Some("2"));
        assert_variable_resolve(&resolver, "CURSOR_INDEX", Some("0"));
        assert_variable_resolve(&resolver, "CURSOR_NUMBER", Some("1"));

        resolver = SelectionBasedVariableResolver::new(f.model.clone(), sel(1, 2, 2, 3), 4, None);
        assert_variable_resolve(&resolver, "CURSOR_INDEX", Some("4"));
        assert_variable_resolve(&resolver, "CURSOR_NUMBER", Some("5"));

        resolver = SelectionBasedVariableResolver::new(f.model.clone(), sel(2, 3, 1, 2), 0, None);
        assert_variable_resolve(&resolver, "TM_SELECTED_TEXT", Some("his is line one\nth"));
        assert_variable_resolve(&resolver, "TM_CURRENT_LINE", Some("this is line one"));
        assert_variable_resolve(&resolver, "TM_LINE_INDEX", Some("0"));
        assert_variable_resolve(&resolver, "TM_LINE_NUMBER", Some("1"));

        resolver = SelectionBasedVariableResolver::new(f.model.clone(), sel(1, 2, 1, 2), 0, None);
        assert_variable_resolve(&resolver, "TM_SELECTED_TEXT", None);

        assert_variable_resolve(&resolver, "TM_CURRENT_WORD", Some("this"));

        resolver = SelectionBasedVariableResolver::new(f.model.clone(), sel(3, 1, 3, 1), 0, None);
        assert_variable_resolve(&resolver, "TM_CURRENT_WORD", None);
    });
}

#[test]
fn textmate_snippet_resolve_variable() {
    with_fixture(|f| {
        let snippet = SnippetParser::new().parse(&utf16("\"$TM_CURRENT_WORD\""), true, false);
        assert_eq!(snippet.to_js_string(), "\"\"");
        snippet.resolve_variables(&*f.resolver);
        assert_eq!(snippet.to_js_string(), "\"this\"");
    });
}

#[test]
fn textmate_snippet_resolve_variable_with_default() {
    with_fixture(|f| {
        let snippet = SnippetParser::new().parse(&utf16("\"${TM_CURRENT_WORD:foo}\""), true, false);
        assert_eq!(snippet.to_js_string(), "\"foo\"");
        snippet.resolve_variables(&*f.resolver);
        assert_eq!(snippet.to_js_string(), "\"this\"");
    });
}

#[test]
fn more_useful_environment_variables_for_snippets_32737() {
    with_fixture(|f| {
        assert_variable_resolve(&*f.resolver, "TM_FILENAME_BASE", Some("text"));

        let model = model_with_uri("", "http://www.pb.o/abc/def/ghi");
        assert_variable_resolve(&ModelBasedVariableResolver::new(fs_path_label_service(), model.clone()), "TM_FILENAME_BASE", Some("ghi"));
        model.dispose();

        let model = model_with_uri("", "mem:.git");
        assert_variable_resolve(&ModelBasedVariableResolver::new(fs_path_label_service(), model.clone()), "TM_FILENAME_BASE", Some(".git"));
        model.dispose();

        let model = model_with_uri("", "mem:foo.");
        assert_variable_resolve(&ModelBasedVariableResolver::new(fs_path_label_service(), model.clone()), "TM_FILENAME_BASE", Some("foo"));
        model.dispose();
    });
}

#[test]
fn variable_snippet_transform() {
    with_fixture(|f| {
        let snippet = SnippetParser::new().parse(&utf16("name=${TM_FILENAME/(.*)\\..+$/$1/}"), true, false);
        snippet.resolve_variables(&*f.resolver);
        assert_eq!(snippet.to_js_string(), "name=text");

        assert_variable_resolve2("${ThisIsAVar/([A-Z]).*(Var)/$2/}", "Var", None);
        assert_variable_resolve2("${ThisIsAVar/([A-Z]).*(Var)/$2-${1:/downcase}/}", "Var-t", None);
        assert_variable_resolve2("${Foo/(.*)/${1:+Bar}/img}", "Bar", None);

        //https://github.com/microsoft/vscode/issues/33162
        assert_variable_resolve2(
            "export default class ${TM_FILENAME/(\\w+)\\.js/$1/g}",
            "export default class FooFile",
            Some("FooFile.js"),
        );

        assert_variable_resolve2("${foobarfoobar/(foo)/${1:+FAR}/g}", "FARbarFARbar", None); // global
        assert_variable_resolve2("${foobarfoobar/(foo)/${1:+FAR}/}", "FARbarfoobar", None); // first match
        assert_variable_resolve2("${foobarfoobar/(bazz)/${1:+FAR}/g}", "foobarfoobar", None); // no match, no else
        // assertVariableResolve2('${foobarfoobar/(bazz)/${1:+FAR}/g}', ''); // no match

        assert_variable_resolve2("${foobarfoobar/(foo)/${2:+FAR}/g}", "barbar", None); // bad group reference
    });
}

#[test]
fn snippet_transforms_do_not_handle_regex_with_alternatives_or_optional_matches_36089() {
    with_fixture(|_| {
        assert_variable_resolve2(
            "${TM_FILENAME/^(.)|(?:-(.))|(\\.js)/${1:/upcase}${2:/upcase}/g}",
            "MyClass",
            Some("my-class.js"),
        );

        // no hyphens
        assert_variable_resolve2(
            "${TM_FILENAME/^(.)|(?:-(.))|(\\.js)/${1:/upcase}${2:/upcase}/g}",
            "Myclass",
            Some("myclass.js"),
        );

        // none matching suffix
        assert_variable_resolve2(
            "${TM_FILENAME/^(.)|(?:-(.))|(\\.js)/${1:/upcase}${2:/upcase}/g}",
            "Myclass.foo",
            Some("myclass.foo"),
        );

        // more than one hyphen
        assert_variable_resolve2(
            "${TM_FILENAME/^(.)|(?:-(.))|(\\.js)/${1:/upcase}${2:/upcase}/g}",
            "ThisIsAFile",
            Some("this-is-a-file.js"),
        );

        // KEBAB CASE
        assert_variable_resolve2(
            "${TM_FILENAME_BASE/([A-Z][a-z]+)([A-Z][a-z]+$)?/${1:/downcase}-${2:/downcase}/g}",
            "capital-case",
            Some("CapitalCase"),
        );

        assert_variable_resolve2(
            "${TM_FILENAME_BASE/([A-Z][a-z]+)([A-Z][a-z]+$)?/${1:/downcase}-${2:/downcase}/g}",
            "capital-case-more",
            Some("CapitalCaseMore"),
        );
    });
}

#[test]
fn add_variable_to_insert_value_from_clipboard_to_a_snippet_40153() {
    with_fixture(|_| {
        assert_variable_resolve(&ClipboardBasedVariableResolver::new(clipboard(None), 1, 0, true), "CLIPBOARD", None);

        // `() => null!`
        assert_variable_resolve(&ClipboardBasedVariableResolver::new(clipboard(None), 1, 0, true), "CLIPBOARD", None);

        assert_variable_resolve(&ClipboardBasedVariableResolver::new(clipboard(Some("")), 1, 0, true), "CLIPBOARD", None);

        assert_variable_resolve(&ClipboardBasedVariableResolver::new(clipboard(Some("foo")), 1, 0, true), "CLIPBOARD", Some("foo"));

        assert_variable_resolve(&ClipboardBasedVariableResolver::new(clipboard(Some("foo")), 1, 0, true), "foo", None);
        assert_variable_resolve(&ClipboardBasedVariableResolver::new(clipboard(Some("foo")), 1, 0, true), "cLIPBOARD", None);
    });
}

#[test]
fn add_variable_to_insert_value_from_clipboard_to_a_snippet_40153_2() {
    with_fixture(|_| {
        assert_variable_resolve(&ClipboardBasedVariableResolver::new(clipboard(Some("line1")), 1, 2, true), "CLIPBOARD", Some("line1"));
        assert_variable_resolve(
            &ClipboardBasedVariableResolver::new(clipboard(Some("line1\nline2\nline3")), 1, 2, true),
            "CLIPBOARD",
            Some("line1\nline2\nline3"),
        );

        assert_variable_resolve(&ClipboardBasedVariableResolver::new(clipboard(Some("line1\nline2")), 1, 2, true), "CLIPBOARD", Some("line2"));
        assert_variable_resolve(&ClipboardBasedVariableResolver::new(clipboard(Some("line1\nline2")), 0, 2, true), "CLIPBOARD", Some("line1"));

        assert_variable_resolve(
            &ClipboardBasedVariableResolver::new(clipboard(Some("line1\nline2")), 0, 2, false),
            "CLIPBOARD",
            Some("line1\nline2"),
        );
    });
}

#[test]
fn add_time_variables_for_snippets_41631_43140() {
    with_fixture(|_| {
        let resolver = TimeBasedVariableResolver::default();

        assert_variable_resolve3(&resolver, "CURRENT_YEAR");
        assert_variable_resolve3(&resolver, "CURRENT_YEAR_SHORT");
        assert_variable_resolve3(&resolver, "CURRENT_MONTH");
        assert_variable_resolve3(&resolver, "CURRENT_DATE");
        assert_variable_resolve3(&resolver, "CURRENT_HOUR");
        assert_variable_resolve3(&resolver, "CURRENT_MINUTE");
        assert_variable_resolve3(&resolver, "CURRENT_SECOND");
        assert_variable_resolve3(&resolver, "CURRENT_MILLISECOND");
        assert_variable_resolve3(&resolver, "CURRENT_DAY_NAME");
        assert_variable_resolve3(&resolver, "CURRENT_DAY_NAME_SHORT");
        assert_variable_resolve3(&resolver, "CURRENT_MONTH_NAME");
        assert_variable_resolve3(&resolver, "CURRENT_MONTH_NAME_SHORT");
        assert_variable_resolve3(&resolver, "CURRENT_SECONDS_UNIX");
        assert_variable_resolve3(&resolver, "CURRENT_MILLISECONDS_UNIX");
        assert_variable_resolve3(&resolver, "CURRENT_TIMEZONE_OFFSET");
        assert_variable_resolve3(&resolver, "CURRENT_TIMEZONE_NAME");
    });
}

#[test]
fn time_based_snippet_variables_have_deterministic_millisecond_and_unix_values() {
    with_fixture(|_| {
        // `Date.UTC(2024, 3, 15, 12, 34, 56, 7)`
        let now = "2024-04-15T12:34:56.007Z".parse::<Timestamp>().expect("instante válido").as_millisecond();
        runtime::with_faked_timers(|| {
            runtime::set_system_time(u64::try_from(now).expect("depois da época"));
            let resolver = TimeBasedVariableResolver::default();
            let expected_date = Timestamp::from_millisecond(now).expect("instante válido").to_zoned(TimeZone::system());
            let pad = |value: i64, length: usize| format!("{value:0length$}");

            assert_variable_resolve(&resolver, "CURRENT_YEAR", Some(&expected_date.year().to_string()));
            let year = expected_date.year().to_string();
            assert_variable_resolve(&resolver, "CURRENT_YEAR_SHORT", Some(&year[year.len() - 2..]));
            assert_variable_resolve(&resolver, "CURRENT_MONTH", Some(&pad(i64::from(expected_date.month()), 2)));
            assert_variable_resolve(&resolver, "CURRENT_DATE", Some(&pad(i64::from(expected_date.day()), 2)));
            assert_variable_resolve(&resolver, "CURRENT_HOUR", Some(&pad(i64::from(expected_date.hour()), 2)));
            assert_variable_resolve(&resolver, "CURRENT_MINUTE", Some(&pad(i64::from(expected_date.minute()), 2)));
            assert_variable_resolve(&resolver, "CURRENT_SECOND", Some(&pad(i64::from(expected_date.second()), 2)));
            assert_variable_resolve(&resolver, "CURRENT_MILLISECOND", Some(&pad(i64::from(expected_date.millisecond()), 3)));
            assert_variable_resolve(&resolver, "CURRENT_SECONDS_UNIX", Some(&now.div_euclid(1000).to_string()));
            assert_variable_resolve(&resolver, "CURRENT_MILLISECONDS_UNIX", Some(&now.to_string()));
        });
    });
}

#[test]
fn time_based_snippet_variables_resolve_to_the_same_values_even_as_time_progresses() {
    with_fixture(|_| {
        let snippet_text = "
\t\t\t\t$CURRENT_YEAR
\t\t\t\t$CURRENT_YEAR_SHORT
\t\t\t\t$CURRENT_MONTH
\t\t\t\t$CURRENT_DATE
\t\t\t\t$CURRENT_HOUR
\t\t\t\t$CURRENT_MINUTE
\t\t\t\t$CURRENT_SECOND
\t\t\t\t$CURRENT_MILLISECOND
\t\t\t\t$CURRENT_DAY_NAME
\t\t\t\t$CURRENT_DAY_NAME_SHORT
\t\t\t\t$CURRENT_MONTH_NAME
\t\t\t\t$CURRENT_MONTH_NAME_SHORT
\t\t\t\t$CURRENT_SECONDS_UNIX
\t\t\t\t$CURRENT_MILLISECONDS_UNIX
\t\t\t\t$CURRENT_TIMEZONE_OFFSET
\t\t\t\t$CURRENT_TIMEZONE_NAME
\t\t\t";

        runtime::with_faked_timers(|| {
            let resolver = TimeBasedVariableResolver::default();

            let first_resolve = SnippetParser::new().parse(&utf16(snippet_text), false, false);
            first_resolve.resolve_variables(&resolver);
            // 1 year + 1 day + 1 hour + 1 minute + 1 second
            runtime::advance_fake_time((365 * 24 * 3600 * 1000) + (24 * 3600 * 1000) + (3661 * 1000));
            let second_resolve = SnippetParser::new().parse(&utf16(snippet_text), false, false);
            second_resolve.resolve_variables(&resolver);

            assert_eq!(
                first_resolve.to_js_string(),
                second_resolve.to_js_string(),
                "Time-based snippet variables resolved differently"
            );
        });
    });
}

#[test]
fn creating_snippet_format_condition_doesnt_work_53617() {
    with_fixture(|_| {
        let snippet = SnippetParser::new().parse(&utf16("${TM_LINE_NUMBER/(10)/${1:?It is:It is not}/} line 10"), true, false);
        snippet.resolve_variables(&FixedResolver("10"));
        assert_eq!(snippet.to_js_string(), "It is line 10");

        snippet.resolve_variables(&FixedResolver("11"));
        assert_eq!(snippet.to_js_string(), "It is not line 10");
    });
}

#[test]
fn add_workspace_name_and_folder_variables_for_snippets_68261() {
    with_fixture(|_| {
        let new_workspace = |folders, configuration| Workspace::new("", folders, false, configuration, Rc::new(|_: &URI| false), None);

        let workspace_service = WorkspaceOnlyService::new(new_workspace(Vec::new(), None));

        let resolver = WorkspaceBasedVariableResolver::new(Some(workspace_service.clone()));

        // empty workspace
        workspace_service.set_workspace(new_workspace(Vec::new(), None));
        assert_variable_resolve(&resolver, "WORKSPACE_NAME", None);
        assert_variable_resolve(&resolver, "WORKSPACE_FOLDER", None);

        // single folder workspace without config
        workspace_service.set_workspace(new_workspace(vec![to_workspace_folder(&uri_file("/folderName"), "folderName")], None));
        assert_variable_resolve(&resolver, "WORKSPACE_NAME", Some("folderName"));
        if !cfg!(windows) {
            assert_variable_resolve(&resolver, "WORKSPACE_FOLDER", Some("/folderName"));
        }

        // workspace with config
        let workspace_config_path = uri_file("testWorkspace.code-workspace");
        workspace_service.set_workspace(new_workspace(
            to_workspace_folders(
                &[RawWorkspaceFolder::Path { path: "folderName".to_owned(), name: None }],
                &workspace_config_path,
                &ext_uri_biased_ignore_path_case(),
            ),
            Some(workspace_config_path.clone()),
        ));
        assert_variable_resolve(&resolver, "WORKSPACE_NAME", Some("testWorkspace"));
        if !cfg!(windows) {
            assert_variable_resolve(&resolver, "WORKSPACE_FOLDER", Some("/"));
        }
    });
}

#[test]
fn add_relative_filepath_snippet_variable_114208() {
    with_fixture(|_| {
        // Mock a label service (only coded for file uris)
        let workspace_label_service = |root_path: &str| {
            let root_path = root_path.to_owned();
            ClosureLabelService::new(move |uri, options| {
                let fs = fs_path(uri);
                if options.relative && !root_path.is_empty() {
                    let root_fs_path = format!("{}{}", fs_path(&uri_file(&root_path)), std::path::MAIN_SEPARATOR_STR);
                    if let Some(relative) = fs.strip_prefix(&root_fs_path) {
                        return relative.to_owned();
                    }
                }
                fs
            })
        };

        let model = model_with_uri("", "file:///foo/files/text.txt");

        // empty workspace
        let mut resolver = ModelBasedVariableResolver::new(workspace_label_service(""), model.clone());

        if !cfg!(windows) {
            assert_variable_resolve(&resolver, "RELATIVE_FILEPATH", Some("/foo/files/text.txt"));
        } else {
            assert_variable_resolve(&resolver, "RELATIVE_FILEPATH", Some("\\foo\\files\\text.txt"));
        }

        // single folder workspace
        resolver = ModelBasedVariableResolver::new(workspace_label_service("/foo"), model.clone());
        if !cfg!(windows) {
            assert_variable_resolve(&resolver, "RELATIVE_FILEPATH", Some("files/text.txt"));
        } else {
            assert_variable_resolve(&resolver, "RELATIVE_FILEPATH", Some("files\\text.txt"));
        }

        model.dispose();
    });
}
