//! Porte de `src/vs/workbench/contrib/terminalContrib/suggest/test/browser/terminalCompletionModel.test.ts`
//! (suíte `TerminalCompletionModel`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o `LeakGuard` criado no começo de cada caso.
//! - `Partial<ITerminalCompletion>` vira um `ITerminalCompletion` com `..Default::default()`, e o
//!   `createItem` aplica os mesmos valores padrão do original (`defaultLabel`, `defaultProvider`,
//!   `[0, 1]` e o tipo `Method`).
//! - `deepStrictEqual` vira `assert_eq!` sobre [`SimpleCompletionLabel`], que deriva `PartialEq`.
//! - `model.items` (getter) é `model.items()`, que recalcula o cache e por isso pede `&mut`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_workbench::contrib::terminal_contrib::suggest::terminal_completion_item::{
    ITerminalCompletion, TerminalCompletionItem, TerminalCompletionItemKind as Kind,
};
use je_workbench::contrib::terminal_contrib::suggest::terminal_completion_model::TerminalCompletionModel;
use je_workbench::services::suggest::simple_completion_item::{CompletionItemLabel, SimpleCompletionLabel};
use je_workbench::services::suggest::simple_completion_model::LineContext;

fn create_item(options: ITerminalCompletion) -> TerminalCompletionItem {
    let label = match &options.label {
        SimpleCompletionLabel::Text(text) if text.is_empty() => SimpleCompletionLabel::from("defaultLabel"),
        other => other.clone(),
    };
    TerminalCompletionItem::new(
        ITerminalCompletion {
            kind: options.kind.or(Some(Kind::Method)),
            label,
            provider: if options.provider.is_empty() { "defaultProvider".to_owned() } else { options.provider },
            replacement_range: options.replacement_range.or(Some([0, 1])),
            ..options
        },
        None,
    )
}

/// `createItem({ kind, label })`.
fn create_item_of(kind: Option<Kind>, label: impl Into<SimpleCompletionLabel>) -> TerminalCompletionItem {
    create_item(ITerminalCompletion { kind, label: label.into(), ..Default::default() })
}

fn create_file_items(labels: &[&str]) -> Vec<TerminalCompletionItem> {
    labels.iter().map(|label| create_item_of(Some(Kind::File), *label)).collect()
}

fn create_file_items_model(labels: &[&str]) -> TerminalCompletionModel {
    TerminalCompletionModel::new(create_file_items(labels), LineContext::new("", 0))
}

fn create_folder_items(labels: &[&str]) -> Vec<TerminalCompletionItem> {
    labels.iter().map(|label| create_item_of(Some(Kind::Folder), *label)).collect()
}

fn create_folder_items_model(labels: &[&str]) -> TerminalCompletionModel {
    TerminalCompletionModel::new(create_folder_items(labels), LineContext::new("", 0))
}

fn labels(texts: &[&str]) -> Vec<SimpleCompletionLabel> {
    texts.iter().map(|text| SimpleCompletionLabel::from(*text)).collect()
}

fn assert_items(model: &mut TerminalCompletionModel, expected: Vec<SimpleCompletionLabel>) {
    let items = model.items();
    let actual: Vec<SimpleCompletionLabel> = items.iter().map(|item| item.completion.label.clone()).collect();
    assert_eq!(actual, expected);
    assert_eq!(items.len(), expected.len()); // verificação de sanidade
}

fn assert_texts(model: &mut TerminalCompletionModel, expected: &[&str]) {
    assert_items(model, labels(expected));
}

fn first_label(model: &mut TerminalCompletionModel, index: usize) -> SimpleCompletionLabel {
    model.items()[index].completion.label.clone()
}

#[test]
fn should_handle_an_empty_list() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let mut model = TerminalCompletionModel::new(Vec::new(), LineContext::new("", 0));

    assert_eq!(model.items().len(), 0);
}

#[test]
fn should_handle_a_list_with_one_item() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let mut model = TerminalCompletionModel::new(vec![create_item_of(None, "a")], LineContext::new("", 0));

    assert_eq!(model.items().len(), 1);
    assert_eq!(first_label(&mut model, 0), SimpleCompletionLabel::from("a"));
}

#[test]
fn should_sort_alphabetically() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let mut model = TerminalCompletionModel::new(
        vec![create_item_of(None, "b"), create_item_of(None, "z"), create_item_of(None, "a")],
        LineContext::new("", 0),
    );

    assert_eq!(model.items().len(), 3);
    assert_eq!(first_label(&mut model, 0), SimpleCompletionLabel::from("a"));
    assert_eq!(first_label(&mut model, 1), SimpleCompletionLabel::from("b"));
    assert_eq!(first_label(&mut model, 2), SimpleCompletionLabel::from("z"));
}

#[test]
fn fuzzy_matching() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let initial = [".\\.eslintrc", ".\\resources\\", ".\\scripts\\", ".\\src\\"];
    let expected = [".\\scripts\\", ".\\src\\", ".\\.eslintrc", ".\\resources\\"];
    let mut model =
        TerminalCompletionModel::new(initial.iter().map(|label| create_item_of(None, *label)).collect(), LineContext::new("s", 0));

    assert_texts(&mut model, &expected);
}

// suíte `files and folders`

#[test]
fn should_deprioritize_files_that_start_with_underscore() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let initial = ["_a", "a", "z"];
    let expected = ["a", "z", "_a"];
    assert_texts(&mut create_file_items_model(&initial), &expected);
    assert_texts(&mut create_folder_items_model(&initial), &expected);
}

#[test]
fn should_ignore_the_dot_in_dotfiles_when_sorting() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let initial = ["b", ".a", "a", ".b"];
    let expected = [".a", "a", "b", ".b"];
    assert_texts(&mut create_file_items_model(&initial), &expected);
    assert_texts(&mut create_folder_items_model(&initial), &expected);
}

#[test]
fn should_handle_many_files_and_folders_correctly() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    // Esta é a pasta raiz do VS Code com alguns itens de python acrescentados que têm ordenação especial
    let mut items = create_folder_items(&[
        "__pycache",
        ".build",
        ".configurations",
        ".devcontainer",
        ".eslint-plugin-local",
        ".github",
        ".profile-oss",
        ".vscode",
        ".vscode-test",
        "build",
        "cli",
        "extensions",
        "node_modules",
        "out",
        "remote",
        "resources",
        "scripts",
        "src",
        "test",
    ]);
    items.extend(create_file_items(&[
        "__init__.py",
        ".editorconfig",
        ".eslint-ignore",
        ".git-blame-ignore-revs",
        ".gitattributes",
        ".gitignore",
        ".lsifrc.json",
        ".mailmap",
        ".mention-bot",
        ".npmrc",
        ".nvmrc",
        ".vscode-test.js",
        "cglicenses.json",
        "cgmanifest.json",
        "CodeQL.yml",
        "CONTRIBUTING.md",
        "eslint.config.js",
        "gulpfile.js",
        "LICENSE.txt",
        "package-lock.json",
        "package.json",
        "product.json",
        "README.md",
        "SECURITY.md",
        "ThirdPartyNotices.txt",
        "tsfmt.json",
    ]));
    let mut model = TerminalCompletionModel::new(items, LineContext::new("", 0));
    assert_texts(
        &mut model,
        &[
            ".build",
            "build",
            "cglicenses.json",
            "cgmanifest.json",
            "cli",
            "CodeQL.yml",
            ".configurations",
            "CONTRIBUTING.md",
            ".devcontainer",
            ".editorconfig",
            "eslint.config.js",
            ".eslint-ignore",
            ".eslint-plugin-local",
            "extensions",
            ".gitattributes",
            ".git-blame-ignore-revs",
            ".github",
            ".gitignore",
            "gulpfile.js",
            "LICENSE.txt",
            ".lsifrc.json",
            ".mailmap",
            ".mention-bot",
            "node_modules",
            ".npmrc",
            ".nvmrc",
            "out",
            "package.json",
            "package-lock.json",
            "product.json",
            ".profile-oss",
            "README.md",
            "remote",
            "resources",
            "scripts",
            "SECURITY.md",
            "src",
            "test",
            "ThirdPartyNotices.txt",
            "tsfmt.json",
            ".vscode",
            ".vscode-test",
            ".vscode-test.js",
            "__init__.py",
            "__pycache",
        ],
    );
}

// suíte `Punctuation`

#[test]
fn punctuation_chars_should_be_below_other_methods() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let items = ["a", "b", ",", ";", ":", "c", "[", "..."].iter().map(|label| create_item_of(None, *label)).collect();
    let mut model = TerminalCompletionModel::new(items, LineContext::new("", 0));
    assert_texts(&mut model, &["a", "b", "c", ",", ";", ":", "[", "..."]);
}

#[test]
fn punctuation_chars_should_be_below_other_files() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let items = ["..", "...", "../", "./a/", "./b/"].iter().map(|label| create_item_of(None, *label)).collect();
    let mut model = TerminalCompletionModel::new(items, LineContext::new("", 0));
    assert_texts(&mut model, &["./a/", "./b/", "..", "...", "../"]);
}

// suíte `inline completions`

fn create_inline_items(kind: Kind) -> Vec<TerminalCompletionItem> {
    let mut items = create_folder_items(&["a", "c"]);
    items.extend(create_file_items(&["b", "d"]));
    items.push(TerminalCompletionItem::new(
        ITerminalCompletion {
            label: "ab".into(),
            provider: "core".to_owned(),
            replacement_range: Some([0, 0]),
            kind: Some(kind),
            ..Default::default()
        },
        None,
    ));
    items
}

// suíte `InlineSuggestion`

#[test]
fn inline_suggestion_should_put_on_top_generally() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let mut model = TerminalCompletionModel::new(create_inline_items(Kind::InlineSuggestion), LineContext::new("", 0));
    assert_eq!(first_label(&mut model, 0), SimpleCompletionLabel::from("ab"));
}

#[test]
fn inline_suggestion_should_not_put_on_top_when_there_is_an_exact_match_of_another_item() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let mut model = TerminalCompletionModel::new(create_inline_items(Kind::InlineSuggestion), LineContext::new("a", 0));
    assert_ne!(first_label(&mut model, 0), SimpleCompletionLabel::from("ab"));
    assert_eq!(first_label(&mut model, 1), SimpleCompletionLabel::from("ab"));
}

// suíte `InlineSuggestionAlwaysOnTop`

#[test]
fn inline_suggestion_always_on_top_should_put_on_top_generally() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let mut model = TerminalCompletionModel::new(create_inline_items(Kind::InlineSuggestionAlwaysOnTop), LineContext::new("", 0));
    assert_eq!(first_label(&mut model, 0), SimpleCompletionLabel::from("ab"));
}

#[test]
fn inline_suggestion_always_on_top_should_put_on_top_even_if_there_is_an_exact_match_of_another_item() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let mut model = TerminalCompletionModel::new(create_inline_items(Kind::InlineSuggestionAlwaysOnTop), LineContext::new("a", 0));
    assert_eq!(first_label(&mut model, 0), SimpleCompletionLabel::from("ab"));
}

// suíte `git branch priority sorting`

fn create_argument_items(items: &[SimpleCompletionLabel]) -> Vec<TerminalCompletionItem> {
    items.iter().map(|label| create_item_of(Some(Kind::Argument), label.clone())).collect()
}

fn argument_model(texts: &[&str], leading_line_content: &str) -> TerminalCompletionModel {
    TerminalCompletionModel::new(create_argument_items(&labels(texts)), LineContext::new(leading_line_content, 0))
}

#[test]
fn should_prioritize_main_and_master_branches_for_git_commands() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let mut model = argument_model(&["feature-branch", "master", "development", "main"], "git checkout ");
    assert_texts(&mut model, &["main", "master", "development", "feature-branch"]);
}

#[test]
fn should_prioritize_main_and_master_branches_for_git_switch_command() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let mut model = argument_model(&["feature-branch", "main", "another-feature", "master"], "git switch ");
    assert_texts(&mut model, &["main", "master", "another-feature", "feature-branch"]);
}

#[test]
fn should_not_prioritize_main_and_master_for_non_git_commands() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let mut model = argument_model(&["feature-branch", "master", "main"], "ls ");
    assert_texts(&mut model, &["feature-branch", "main", "master"]);
}

#[test]
fn should_handle_git_commands_with_leading_whitespace() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let mut model = argument_model(&["feature-branch", "master", "main"], "  git checkout ");
    assert_texts(&mut model, &["main", "master", "feature-branch"]);
}

#[test]
fn should_work_with_complex_label_objects() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let complex = |label: &str, description: &str| {
        SimpleCompletionLabel::from(CompletionItemLabel {
            label: label.to_owned(),
            detail: None,
            description: Some(description.to_owned()),
        })
    };
    let items = [
        complex("feature-branch", "Feature branch"),
        complex("master", "Master branch"),
        complex("main", "Main branch"),
    ];
    let mut model = TerminalCompletionModel::new(create_argument_items(&items), LineContext::new("git checkout ", 0));
    assert_items(
        &mut model,
        vec![complex("main", "Main branch"), complex("master", "Master branch"), complex("feature-branch", "Feature branch")],
    );
}

#[test]
fn should_not_prioritize_branches_with_similar_names() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let mut model = argument_model(&["mainline", "masterpiece", "main", "master"], "git checkout ");
    assert_texts(&mut model, &["main", "master", "mainline", "masterpiece"]);
}

#[test]
fn should_prioritize_for_git_branch_d() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let mut model = argument_model(&["main", "master", "dev"], "git branch -d ");
    assert_texts(&mut model, &["main", "master", "dev"]);
}

// suíte `mixed kind sorting`

#[test]
fn should_sort_arguments_before_flags_and_options() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let items = vec![
        create_item_of(Some(Kind::Flag), "--verbose"),
        create_item_of(Some(Kind::Option), "--config"),
        create_item_of(Some(Kind::Argument), "value2"),
        create_item_of(Some(Kind::Argument), "value1"),
        create_item_of(Some(Kind::Flag), "--all"),
    ];
    let mut model = TerminalCompletionModel::new(items, LineContext::new("cmd ", 0));
    assert_texts(&mut model, &["value1", "value2", "--all", "--config", "--verbose"]);
}

#[test]
fn should_sort_by_kind_hierarchy_methods_aliases_arguments_others_files_folders() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let items = vec![
        create_item_of(Some(Kind::File), "file.txt"),
        create_item_of(Some(Kind::Flag), "--flag"),
        create_item_of(Some(Kind::Argument), "arg"),
        create_item_of(Some(Kind::Method), "method"),
        create_item_of(Some(Kind::Folder), "folder/"),
        create_item_of(Some(Kind::Option), "--option"),
        create_item_of(Some(Kind::Alias), "alias"),
        create_item_of(Some(Kind::SymbolicLinkFile), "file2.txt"),
        create_item_of(Some(Kind::SymbolicLinkFolder), "folder2/"),
    ];
    let mut model = TerminalCompletionModel::new(items, LineContext::new("", 0));
    assert_texts(&mut model, &["alias", "method", "arg", "--flag", "--option", "file2.txt", "file.txt", "folder/", "folder2/"]);
}
