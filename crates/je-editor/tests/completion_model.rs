//! Porte de `src/vs/editor/contrib/suggest/test/browser/completionModel.test.ts` (suíte `CompletionModel`), caso a caso
//! e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o `LeakGuard` criado no começo de cada caso; o `setup` é [`setup`].
//! - `createSuggestItem` (exportada pelo próprio arquivo de teste do original) é `create_suggest_item` e
//!   `create_suggest_item_with` de `tests/support`. Cada chamada cria o seu provedor, como o original.
//! - `EditorOptions.suggest.defaultValue` é o valor padrão do `EditorSuggest`; `EditorOptions.snippetSuggestions.defaultValue`
//!   é `inline`. O `defaultOptions` do original (o literal com `filterGraceful: true` e os `show*`) difere do padrão só
//!   em `snippetsPreventQuickSuggestions`, o único campo que o literal liga e o padrão desliga; os que o literal omite
//!   (`undefined`) o modelo não lê.
//! - `WordDistance.None` é `<dyn WordDistance>::none()`; o último argumento `undefined` do construtor (as opções do
//!   filtro fuzzy) é `None`, e o `clipboardText` ausente também.
//! - `model.lineContext = { ... }` é `set_line_context`; `itemsNow === itemsThen` é `Rc::ptr_eq` sobre as listas.
//! - `a.score < b.score` compara arrays do JavaScript: eles viram texto (`join(',')`) e os textos se comparam; o
//!   auxiliar [`score_text`] faz a conversão.
//! - `assert.deepStrictEqual(actual, [itemAAA, itemIII, itemZZZ])` compara a identidade dos itens (`is_same`), que é o
//!   que a igualdade profunda de objetos distintos com campos diferentes verifica aqui.
//! - `getSuggestionComparator(Inline)` devolve o comparador de três vias; o `Array.prototype.sort` com ele é o
//!   `sort_by` com a ordenação do resultado.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_editor::config::editor_options::EditorOptionImpl;
use je_editor::config::editor_options::suggest::{EditorSuggest, InternalSuggestOptions};
use je_editor::languages::{CompletionItemKind, CompletionItemLabel, CompletionLabel};
use je_editor::position::Position;
use je_editor::suggest::completion_model::{CompletionModel, LineContext};
use je_editor::suggest::suggest::{CompletionItem, SnippetSortOrder, get_suggestion_comparator, label_text};
use je_editor::suggest::word_distance::WordDistance;
use support::suggest_test_utils::{SuggestItemOptions, create_suggest_item, create_suggest_item_with};

/// `EditorOptions.snippetSuggestions.defaultValue`.
const DEFAULT_SNIPPET_SUGGESTIONS: &str = "inline";

/// `EditorOptions.suggest.defaultValue`.
fn default_suggest_options() -> InternalSuggestOptions {
    EditorSuggest::new().default_value()
}

/// `defaultOptions` (ver as notas no topo do arquivo).
fn default_options() -> InternalSuggestOptions {
    InternalSuggestOptions { snippets_prevent_quick_suggestions: true, ..default_suggest_options() }
}

fn line_context(leading_line_content: &str, character_count_delta: i32) -> LineContext {
    LineContext { leading_line_content: leading_line_content.to_string(), character_count_delta }
}

/// `new CompletionModel(items, column, lineContext, WordDistance.None, options, snippetSuggestions, undefined)`.
fn new_model(
    items: Vec<CompletionItem>,
    column: i32,
    context: LineContext,
    options: InternalSuggestOptions,
    snippet_suggestions: &str,
) -> CompletionModel {
    CompletionModel::new(items, column, context, <dyn WordDistance>::none(), options, snippet_suggestions, None, None)
}

/// O modelo com as opções e o `snippetSuggestions` padrão.
fn new_default_model(items: Vec<CompletionItem>, column: i32, context: LineContext) -> CompletionModel {
    new_model(items, column, context, default_suggest_options(), DEFAULT_SNIPPET_SUGGESTIONS)
}

/// `createSuggestItem(label, overwriteBefore, kind, incomplete, position, sortText, filterText)`.
fn create_item(
    label: &str,
    overwrite_before: i32,
    kind: CompletionItemKind,
    incomplete: bool,
    position: Position,
) -> CompletionItem {
    create_suggest_item_with(
        CompletionLabel::Text(label.to_string()),
        overwrite_before,
        SuggestItemOptions { kind, incomplete, position, ..SuggestItemOptions::default() },
    )
}

/// `item.completion.label`, o texto do rótulo.
fn label(item: &CompletionItem) -> String {
    label_text(&item.completion.borrow().label).to_string()
}

/// `item.score` como o JavaScript o converte em texto (`[a, b, c].toString()`).
fn score_text(item: &CompletionItem) -> String {
    item.score.borrow().iter().map(i32::to_string).collect::<Vec<_>>().join(",")
}

/// O `setup` da suíte.
fn setup() -> CompletionModel {
    new_default_model(
        vec![create_suggest_item("foo", 3), create_suggest_item("Foo", 3), create_suggest_item("foo", 2)],
        1,
        line_context("foo", 0),
    )
}

// suite('CompletionModel')

#[test]
fn filtering_cached() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let model = setup();

    let items_now = model.items();
    let mut items_then = model.items();
    assert!(Rc::ptr_eq(&items_now, &items_then));

    // still the same context
    model.set_line_context(line_context("foo", 0));
    items_then = model.items();
    assert!(Rc::ptr_eq(&items_now, &items_then));

    // different context, refilter
    model.set_line_context(line_context("foo1", 1));
    items_then = model.items();
    assert!(!Rc::ptr_eq(&items_now, &items_then));
}

#[test]
fn complete_incomplete() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let model = setup();

    assert_eq!(model.get_incomplete_provider().size(), 0);

    let incomplete_model = new_default_model(
        vec![
            create_item("foo", 3, CompletionItemKind::Property, true, Position::new(1, 1)),
            create_suggest_item("foo", 2),
        ],
        1,
        line_context("foo", 0),
    );
    assert_eq!(incomplete_model.get_incomplete_provider().size(), 1);
}

#[test]
fn fuzzy_matching_of_snippets_stopped_working_with_inline_snippet_suggestions_49895() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let _model = setup();

    let at = Position::new(1, 2);
    let complete_item1 = create_item("foobar1", 1, CompletionItemKind::Property, false, at);
    let complete_item2 = create_item("foobar2", 1, CompletionItemKind::Property, false, at);
    let complete_item3 = create_item("foobar3", 1, CompletionItemKind::Property, false, at);
    let complete_item4 = create_item("foobar4", 1, CompletionItemKind::Property, false, at);
    let complete_item5 = create_item("foobar5", 1, CompletionItemKind::Property, false, at);
    let incomplete_item1 = create_item("foofoo1", 1, CompletionItemKind::Property, true, at);

    let model = new_default_model(
        vec![complete_item1, complete_item2, complete_item3, complete_item4, complete_item5, incomplete_item1],
        2,
        line_context("f", 0),
    );
    assert_eq!(model.get_incomplete_provider().size(), 1);
    assert_eq!(model.items().len(), 6);
}

#[test]
fn proper_current_word_when_length_0_16380() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let _model = setup();

    let model = new_default_model(
        vec![
            create_suggest_item("    </div", 4),
            create_suggest_item("a", 0),
            create_suggest_item("p", 0),
            create_suggest_item("    </tag", 4),
            create_suggest_item("    XYZ", 4),
        ],
        1,
        line_context("   <", 0),
    );

    let items = model.items();
    assert_eq!(items.len(), 4);

    let [a, b, c, d] = &items[..] else { panic!("esperados 4 itens") };
    assert_eq!(label(a), "    </div");
    assert_eq!(label(b), "    </tag");
    assert_eq!(label(c), "a");
    assert_eq!(label(d), "p");
}

#[test]
fn keep_snippet_sorting_with_prefix_top_25495() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let _model = setup();

    let model = new_model(
        vec![
            create_item("Snippet1", 1, CompletionItemKind::Snippet, false, Position::new(1, 1)),
            create_item("tnippet2", 1, CompletionItemKind::Snippet, false, Position::new(1, 1)),
            create_item("semver", 1, CompletionItemKind::Property, false, Position::new(1, 1)),
        ],
        1,
        line_context("s", 0),
        default_options(),
        "top",
    );

    let items = model.items();
    assert_eq!(items.len(), 2);
    let [a, b] = &items[..] else { panic!("esperados 2 itens") };
    assert_eq!(label(a), "Snippet1");
    assert_eq!(label(b), "semver");
    assert!(score_text(a) < score_text(b)); // snippet really promoted
}

#[test]
fn keep_snippet_sorting_with_prefix_bottom_25495() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let _model = setup();

    let model = new_model(
        vec![
            create_item("snippet1", 1, CompletionItemKind::Snippet, false, Position::new(1, 1)),
            create_item("tnippet2", 1, CompletionItemKind::Snippet, false, Position::new(1, 1)),
            create_item("Semver", 1, CompletionItemKind::Property, false, Position::new(1, 1)),
        ],
        1,
        line_context("s", 0),
        default_options(),
        "bottom",
    );

    let items = model.items();
    assert_eq!(items.len(), 2);
    let [a, b] = &items[..] else { panic!("esperados 2 itens") };
    assert_eq!(label(a), "Semver");
    assert_eq!(label(b), "snippet1");
    assert!(score_text(a) < score_text(b)); // snippet really demoted
}

#[test]
fn keep_snippet_sorting_with_prefix_inline_25495() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let _model = setup();

    let model = new_model(
        vec![
            create_item("snippet1", 1, CompletionItemKind::Snippet, false, Position::new(1, 1)),
            create_item("tnippet2", 1, CompletionItemKind::Snippet, false, Position::new(1, 1)),
            create_suggest_item("Semver", 1),
        ],
        1,
        line_context("s", 0),
        default_options(),
        "inline",
    );

    let items = model.items();
    assert_eq!(items.len(), 2);
    let [a, b] = &items[..] else { panic!("esperados 2 itens") };
    assert_eq!(label(a), "snippet1");
    assert_eq!(label(b), "Semver");
    assert!(score_text(a) > score_text(b)); // snippet really demoted
}

#[test]
fn filter_text_seems_ignored_in_autocompletion_26874() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let _model = setup();

    let item1 = create_suggest_item_with(
        CompletionLabel::Text("Map - java.util".to_string()),
        1,
        SuggestItemOptions { filter_text: Some("Map".to_string()), ..SuggestItemOptions::default() },
    );
    let item2 = create_suggest_item("Map - java.util", 1);

    let model = new_default_model(vec![item1, item2], 1, line_context("M", 0));

    assert_eq!(model.items().len(), 2);

    model.set_line_context(line_context("Map ", 3));
    assert_eq!(model.items().len(), 1);
}

#[test]
fn vscode_1_12_no_longer_obeys_sort_text_in_completion_items_from_language_server_26096() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let _model = setup();

    let item1 = create_suggest_item_with(
        CompletionLabel::Text("<- groups".to_string()),
        2,
        SuggestItemOptions {
            kind: CompletionItemKind::Property,
            incomplete: false,
            position: Position::new(1, 3),
            sort_text: Some("00002".to_string()),
            filter_text: Some("  groups".to_string()),
        },
    );
    let item2 = create_suggest_item_with(
        CompletionLabel::Text("source".to_string()),
        0,
        SuggestItemOptions {
            kind: CompletionItemKind::Property,
            incomplete: false,
            position: Position::new(1, 3),
            sort_text: Some("00001".to_string()),
            filter_text: Some("source".to_string()),
        },
    );
    let comparator = get_suggestion_comparator(SnippetSortOrder::Inline);
    let mut items = vec![item1, item2];
    items.sort_by(|a, b| comparator(a, b).cmp(&0));

    let model = new_default_model(items, 3, line_context("  ", 0));

    let items = model.items();
    assert_eq!(items.len(), 2);

    let [first, second] = &items[..] else { panic!("esperados 2 itens") };
    assert_eq!(label(first), "source");
    assert_eq!(label(second), "<- groups");
}

#[test]
fn completion_item_sorting_broken_when_using_label_details_153026() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let _model = setup();

    let label_item = |text: &str| CompletionLabel::Item(CompletionItemLabel { label: text.to_string(), detail: None, description: None });
    let operator_options = || SuggestItemOptions { kind: CompletionItemKind::Operator, ..SuggestItemOptions::default() };
    let item_zzz = create_suggest_item_with(label_item("ZZZ"), 0, operator_options());
    let item_aaa = create_suggest_item_with(label_item("AAA"), 0, operator_options());
    let item_iii = create_suggest_item_with(CompletionLabel::Text("III".to_string()), 0, operator_options());

    let cmp = get_suggestion_comparator(SnippetSortOrder::Inline);
    let mut actual = vec![item_zzz.clone(), item_aaa.clone(), item_iii.clone()];
    actual.sort_by(|a, b| cmp(a, b).cmp(&0));

    let expected = [item_aaa, item_iii, item_zzz];
    assert_eq!(actual.len(), expected.len());
    assert!(actual.iter().zip(&expected).all(|(actual, expected)| actual.is_same(expected)));
}

#[test]
fn score_only_filtered_items_when_typing_more_score_all_when_typing_less() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let _model = setup();

    let model = new_default_model(
        vec![
            create_suggest_item("console", 0),
            create_suggest_item("co_new", 0),
            create_suggest_item("bar", 0),
            create_suggest_item("car", 0),
            create_suggest_item("foo", 0),
        ],
        1,
        line_context("", 0),
    );

    assert_eq!(model.items().len(), 5);

    // narrow down once
    model.set_line_context(line_context("c", 1));
    assert_eq!(model.items().len(), 3);

    // query gets longer, narrow down the narrow-down'ed-set from before
    model.set_line_context(line_context("cn", 2));
    assert_eq!(model.items().len(), 2);

    // query gets shorter, refilter everything
    model.set_line_context(line_context("", 0));
    assert_eq!(model.items().len(), 5);
}

#[test]
fn have_more_relaxed_suggest_matching_algorithm_15419() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let _model = setup();

    let model = new_default_model(
        vec![
            create_suggest_item("result", 0),
            create_suggest_item("replyToUser", 0),
            create_suggest_item("randomLolut", 0),
            create_suggest_item("car", 0),
            create_suggest_item("foo", 0),
        ],
        1,
        line_context("", 0),
    );

    // query gets longer, narrow down the narrow-down'ed-set from before
    model.set_line_context(line_context("rlut", 4));
    let items = model.items();
    assert_eq!(items.len(), 3);

    let [first, second, third] = &items[..] else { panic!("esperados 3 itens") };
    assert_eq!(label(first), "result"); // best with `rult`
    assert_eq!(label(second), "replyToUser"); // best with `rltu`
    assert_eq!(label(third), "randomLolut"); // best with `rlut`
}

#[test]
fn emmet_suggestion_not_appearing_at_the_top_of_the_list_in_jsx_files_39518() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let _model = setup();

    let model = new_default_model(
        vec![
            create_suggest_item("from", 0),
            create_suggest_item("form", 0),
            create_suggest_item("form:get", 0),
            create_suggest_item("testForeignMeasure", 0),
            create_suggest_item("fooRoom", 0),
        ],
        1,
        line_context("", 0),
    );

    model.set_line_context(line_context("form", 4));
    let items = model.items();
    assert_eq!(items.len(), 5);
    let [first, second, third, ..] = &items[..] else { panic!("esperados 5 itens") };
    assert_eq!(label(first), "form"); // best with `form`
    assert_eq!(label(second), "form:get"); // best with `form`
    assert_eq!(label(third), "from"); // best with `from`
}
