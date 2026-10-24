//! Porte de `src/vs/workbench/contrib/scm/test/browser/scmHistory.test.ts` (suíte `toISCMHistoryItemViewModelArray`), caso a caso e na mesma ordem.
//!
//! Divergências:
//! - **`ensureNoDisposablesAreLeakedInTestSuite`.** É o [`LeakGuard`] criado no começo de cada caso.
//! - **Conferência das raias.** A sequência de `assert.strictEqual` do original sobre `viewModels[i].inputSwimlanes` e
//!   `outputSwimlanes` (o tamanho e, para cada raia, `id` e `color`) é [`assert_swimlanes`], que confere o tamanho e cada par
//!   `(id, color)` na mesma ordem. Os dois pontos em que o original confere só a cor (a raia de entrada do nó `b` nos casos de
//!   mudanças de entrada e de saída com a referência remota primeiro e com mudanças já mescladas) conferem só a cor.
//! - **`ISCMHistoryItem`.** `toSCMHistoryItem(id, parentIds, references?)` preenche `subject` e `message` vazios e os demais
//!   campos ausentes (o `Default` do item).
//! - **Parâmetros opcionais.** `undefined` nos parâmetros de referência é `None`; `addIncomingChanges` e `addOutgoingChanges`
//!   ausentes são `false`, e o `mergeBase` ausente é `None`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_workbench::contrib::scm::browser::scm_history::{
    COLOR_REGISTRY, ColorMap, HISTORY_ITEM_BASE_REF_COLOR, HISTORY_ITEM_REF_COLOR, HISTORY_ITEM_REMOTE_REF_COLOR,
    to_iscm_history_item_view_model_array,
};
use je_workbench::contrib::scm::common::history::{
    HistoryItemViewModelKind, ISCMHistoryItem, ISCMHistoryItemGraphNode, ISCMHistoryItemRef, ISCMHistoryItemViewModel, SCM_INCOMING_HISTORY_ITEM_ID,
};

fn to_scm_history_item(id: &str, parent_ids: &[&str], references: Option<Vec<ISCMHistoryItemRef>>) -> ISCMHistoryItem {
    ISCMHistoryItem {
        id: id.to_string(),
        parent_ids: parent_ids.iter().map(|parent_id| parent_id.to_string()).collect(),
        subject: String::new(),
        message: String::new(),
        references,
        ..ISCMHistoryItem::default()
    }
}

/// `{ id, name: id }`.
fn a_ref(id: &str) -> ISCMHistoryItemRef {
    ISCMHistoryItemRef { id: id.to_string(), name: id.to_string(), ..ISCMHistoryItemRef::default() }
}

/// `{ id, name, revision }`.
fn a_ref_with_revision(id: &str, revision: &str) -> ISCMHistoryItemRef {
    ISCMHistoryItemRef { revision: Some(revision.to_string()), ..a_ref(id) }
}

/// O `Map<string, ColorIdentifier>` do original.
fn color_map(entries: &[(&str, &str)]) -> ColorMap {
    entries.iter().map(|(id, color)| (id.to_string(), Some(color.to_string()))).collect()
}

/// `assert.strictEqual(swimlanes.length, n)` seguido de `id` e `color` de cada raia.
fn assert_swimlanes(swimlanes: &[ISCMHistoryItemGraphNode], expected: &[(&str, &str)]) {
    assert_eq!(swimlanes.len(), expected.len());
    for (swimlane, (id, color)) in swimlanes.iter().zip(expected) {
        assert_eq!(swimlane.id, *id);
        assert_eq!(swimlane.color, *color);
    }
}

fn assert_node(view_model: &ISCMHistoryItemViewModel, input_swimlanes: &[(&str, &str)], output_swimlanes: &[(&str, &str)]) {
    assert_swimlanes(&view_model.input_swimlanes, input_swimlanes);
    assert_swimlanes(&view_model.output_swimlanes, output_swimlanes);
}

fn assert_kind(view_model: &ISCMHistoryItemViewModel, kind: HistoryItemViewModelKind) {
    assert_eq!(view_model.kind.as_str(), kind.as_str());
}

fn view_models_of(models: &[ISCMHistoryItem]) -> Vec<ISCMHistoryItemViewModel> {
    to_iscm_history_item_view_model_array(models, None, None, None, None, false, false, None)
}

#[test]
fn empty_graph() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let view_models = view_models_of(&[]);

    assert_eq!(view_models.len(), 0);
}

/// ```text
/// * a
/// ```
#[test]
fn single_commit() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let models = [to_scm_history_item("a", &[], None)];

    let view_models = view_models_of(&models);

    assert_eq!(view_models.len(), 1);

    assert_eq!(view_models[0].input_swimlanes.len(), 0);
    assert_eq!(view_models[0].output_swimlanes.len(), 0);
}

/// ```text
/// * a(b)
/// * b(c)
/// * c(d)
/// * d(e)
/// * e
/// ```
#[test]
fn linear_graph() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let models = [
        to_scm_history_item("a", &["b"], None),
        to_scm_history_item("b", &["c"], None),
        to_scm_history_item("c", &["d"], None),
        to_scm_history_item("d", &["e"], None),
        to_scm_history_item("e", &[], None),
    ];

    let view_models = view_models_of(&models);

    assert_eq!(view_models.len(), 5);

    // node a
    assert_node(&view_models[0], &[], &[("b", COLOR_REGISTRY[0])]);

    // node b
    assert_node(&view_models[1], &[("b", COLOR_REGISTRY[0])], &[("c", COLOR_REGISTRY[0])]);

    // node c
    assert_node(&view_models[2], &[("c", COLOR_REGISTRY[0])], &[("d", COLOR_REGISTRY[0])]);

    // node d
    assert_node(&view_models[3], &[("d", COLOR_REGISTRY[0])], &[("e", COLOR_REGISTRY[0])]);

    // node e
    assert_node(&view_models[4], &[("e", COLOR_REGISTRY[0])], &[]);
}

/// ```text
/// * a(b)
/// *   b(c,d)
/// |\
/// | * d(c)
/// |/
/// * c(e)
/// * e(f)
/// ```
#[test]
fn merge_commit_single_commit_in_topic_branch() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let models = [
        to_scm_history_item("a", &["b"], None),
        to_scm_history_item("b", &["c", "d"], None),
        to_scm_history_item("d", &["c"], None),
        to_scm_history_item("c", &["e"], None),
        to_scm_history_item("e", &["f"], None),
    ];

    let view_models = view_models_of(&models);

    assert_eq!(view_models.len(), 5);

    // node a
    assert_node(&view_models[0], &[], &[("b", COLOR_REGISTRY[0])]);

    // node b
    assert_node(&view_models[1], &[("b", COLOR_REGISTRY[0])], &[("c", COLOR_REGISTRY[0]), ("d", COLOR_REGISTRY[1])]);

    // node d
    assert_node(
        &view_models[2],
        &[("c", COLOR_REGISTRY[0]), ("d", COLOR_REGISTRY[1])],
        &[("c", COLOR_REGISTRY[0]), ("c", COLOR_REGISTRY[1])],
    );

    // node c
    assert_node(&view_models[3], &[("c", COLOR_REGISTRY[0]), ("c", COLOR_REGISTRY[1])], &[("e", COLOR_REGISTRY[0])]);

    // node e
    assert_node(&view_models[4], &[("e", COLOR_REGISTRY[0])], &[("f", COLOR_REGISTRY[0])]);
}

/// ```text
/// * a(b,c)
/// |\
/// | * c(d)
/// * | b(e)
/// * | e(f)
/// * | f(d)
/// |/
/// * d(g)
/// ```
#[test]
fn merge_commit_multiple_commits_in_topic_branch() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let models = [
        to_scm_history_item("a", &["b", "c"], None),
        to_scm_history_item("c", &["d"], None),
        to_scm_history_item("b", &["e"], None),
        to_scm_history_item("e", &["f"], None),
        to_scm_history_item("f", &["d"], None),
        to_scm_history_item("d", &["g"], None),
    ];

    let view_models = view_models_of(&models);

    assert_eq!(view_models.len(), 6);

    // node a
    assert_node(&view_models[0], &[], &[("b", COLOR_REGISTRY[0]), ("c", COLOR_REGISTRY[1])]);

    // node c
    assert_node(
        &view_models[1],
        &[("b", COLOR_REGISTRY[0]), ("c", COLOR_REGISTRY[1])],
        &[("b", COLOR_REGISTRY[0]), ("d", COLOR_REGISTRY[1])],
    );

    // node b
    assert_node(
        &view_models[2],
        &[("b", COLOR_REGISTRY[0]), ("d", COLOR_REGISTRY[1])],
        &[("e", COLOR_REGISTRY[0]), ("d", COLOR_REGISTRY[1])],
    );

    // node e
    assert_node(
        &view_models[3],
        &[("e", COLOR_REGISTRY[0]), ("d", COLOR_REGISTRY[1])],
        &[("f", COLOR_REGISTRY[0]), ("d", COLOR_REGISTRY[1])],
    );

    // node f
    assert_node(
        &view_models[4],
        &[("f", COLOR_REGISTRY[0]), ("d", COLOR_REGISTRY[1])],
        &[("d", COLOR_REGISTRY[0]), ("d", COLOR_REGISTRY[1])],
    );

    // node d
    assert_node(&view_models[5], &[("d", COLOR_REGISTRY[0]), ("d", COLOR_REGISTRY[1])], &[("g", COLOR_REGISTRY[0])]);
}

/// ```text
/// * a(b,c)
/// |\
/// | * c(b)
/// |/
/// * b(d,e)
/// |\
/// | * e(f)
/// | * f(g)
/// * | d(h)
/// ```
#[test]
fn create_brach_from_merge_commit() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let models = [
        to_scm_history_item("a", &["b", "c"], None),
        to_scm_history_item("c", &["b"], None),
        to_scm_history_item("b", &["d", "e"], None),
        to_scm_history_item("e", &["f"], None),
        to_scm_history_item("f", &["g"], None),
        to_scm_history_item("d", &["h"], None),
    ];

    let view_models = view_models_of(&models);

    assert_eq!(view_models.len(), 6);

    // node a
    assert_node(&view_models[0], &[], &[("b", COLOR_REGISTRY[0]), ("c", COLOR_REGISTRY[1])]);

    // node c
    assert_node(
        &view_models[1],
        &[("b", COLOR_REGISTRY[0]), ("c", COLOR_REGISTRY[1])],
        &[("b", COLOR_REGISTRY[0]), ("b", COLOR_REGISTRY[1])],
    );

    // node b
    assert_node(
        &view_models[2],
        &[("b", COLOR_REGISTRY[0]), ("b", COLOR_REGISTRY[1])],
        &[("d", COLOR_REGISTRY[0]), ("e", COLOR_REGISTRY[2])],
    );

    // node e
    assert_node(
        &view_models[3],
        &[("d", COLOR_REGISTRY[0]), ("e", COLOR_REGISTRY[2])],
        &[("d", COLOR_REGISTRY[0]), ("f", COLOR_REGISTRY[2])],
    );

    // node f
    assert_node(
        &view_models[4],
        &[("d", COLOR_REGISTRY[0]), ("f", COLOR_REGISTRY[2])],
        &[("d", COLOR_REGISTRY[0]), ("g", COLOR_REGISTRY[2])],
    );

    // node d
    assert_node(
        &view_models[5],
        &[("d", COLOR_REGISTRY[0]), ("g", COLOR_REGISTRY[2])],
        &[("h", COLOR_REGISTRY[0]), ("g", COLOR_REGISTRY[2])],
    );
}

/// ```text
/// * a(b,c)
/// |\
/// | * c(d)
/// * | b(e,f)
/// |\|
/// | |\
/// | | * f(g)
/// * | | e(g)
/// | * | d(g)
/// |/ /
/// | /
/// |/
/// * g(h)
/// ```
#[test]
fn create_multiple_branches_from_a_commit() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let models = [
        to_scm_history_item("a", &["b", "c"], None),
        to_scm_history_item("c", &["d"], None),
        to_scm_history_item("b", &["e", "f"], None),
        to_scm_history_item("f", &["g"], None),
        to_scm_history_item("e", &["g"], None),
        to_scm_history_item("d", &["g"], None),
        to_scm_history_item("g", &["h"], None),
    ];

    let view_models = view_models_of(&models);

    assert_eq!(view_models.len(), 7);

    // node a
    assert_node(&view_models[0], &[], &[("b", COLOR_REGISTRY[0]), ("c", COLOR_REGISTRY[1])]);

    // node c
    assert_node(
        &view_models[1],
        &[("b", COLOR_REGISTRY[0]), ("c", COLOR_REGISTRY[1])],
        &[("b", COLOR_REGISTRY[0]), ("d", COLOR_REGISTRY[1])],
    );

    // node b
    assert_node(
        &view_models[2],
        &[("b", COLOR_REGISTRY[0]), ("d", COLOR_REGISTRY[1])],
        &[("e", COLOR_REGISTRY[0]), ("d", COLOR_REGISTRY[1]), ("f", COLOR_REGISTRY[2])],
    );

    // node f
    assert_node(
        &view_models[3],
        &[("e", COLOR_REGISTRY[0]), ("d", COLOR_REGISTRY[1]), ("f", COLOR_REGISTRY[2])],
        &[("e", COLOR_REGISTRY[0]), ("d", COLOR_REGISTRY[1]), ("g", COLOR_REGISTRY[2])],
    );

    // node e
    assert_node(
        &view_models[4],
        &[("e", COLOR_REGISTRY[0]), ("d", COLOR_REGISTRY[1]), ("g", COLOR_REGISTRY[2])],
        &[("g", COLOR_REGISTRY[0]), ("d", COLOR_REGISTRY[1]), ("g", COLOR_REGISTRY[2])],
    );

    // node d
    assert_node(
        &view_models[5],
        &[("g", COLOR_REGISTRY[0]), ("d", COLOR_REGISTRY[1]), ("g", COLOR_REGISTRY[2])],
        &[("g", COLOR_REGISTRY[0]), ("g", COLOR_REGISTRY[1]), ("g", COLOR_REGISTRY[2])],
    );

    // node g
    assert_node(
        &view_models[6],
        &[("g", COLOR_REGISTRY[0]), ("g", COLOR_REGISTRY[1]), ("g", COLOR_REGISTRY[2])],
        &[("h", COLOR_REGISTRY[0])],
    );
}

/// ```text
/// * a(b) [topic]
/// * b(c)
/// * c(d) [origin/topic]
/// * d(e)
/// * e(f,g)
/// |\
/// | * g(h) [origin/main]
/// ```
#[test]
fn graph_with_color_map() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let models = [
        to_scm_history_item("a", &["b"], Some(vec![a_ref("topic")])),
        to_scm_history_item("b", &["c"], None),
        to_scm_history_item("c", &["d"], Some(vec![a_ref("origin/topic")])),
        to_scm_history_item("d", &["e"], None),
        to_scm_history_item("e", &["f", "g"], None),
        to_scm_history_item("g", &["h"], Some(vec![a_ref("origin/main")])),
    ];

    let color_map = color_map(&[
        ("topic", HISTORY_ITEM_REF_COLOR),
        ("origin/topic", HISTORY_ITEM_REMOTE_REF_COLOR),
        ("origin/main", HISTORY_ITEM_BASE_REF_COLOR),
    ]);

    let view_models = to_iscm_history_item_view_model_array(&models, Some(&color_map), None, None, None, false, false, None);

    assert_eq!(view_models.len(), 6);

    // node a
    assert_node(&view_models[0], &[], &[("b", HISTORY_ITEM_REF_COLOR)]);

    // node b
    assert_node(&view_models[1], &[("b", HISTORY_ITEM_REF_COLOR)], &[("c", HISTORY_ITEM_REF_COLOR)]);

    // node c
    assert_node(&view_models[2], &[("c", HISTORY_ITEM_REF_COLOR)], &[("d", HISTORY_ITEM_REMOTE_REF_COLOR)]);

    // node d
    assert_node(&view_models[3], &[("d", HISTORY_ITEM_REMOTE_REF_COLOR)], &[("e", HISTORY_ITEM_REMOTE_REF_COLOR)]);

    // node e
    assert_node(
        &view_models[4],
        &[("e", HISTORY_ITEM_REMOTE_REF_COLOR)],
        &[("f", HISTORY_ITEM_REMOTE_REF_COLOR), ("g", HISTORY_ITEM_BASE_REF_COLOR)],
    );

    // node g
    assert_node(
        &view_models[5],
        &[("f", HISTORY_ITEM_REMOTE_REF_COLOR), ("g", HISTORY_ITEM_BASE_REF_COLOR)],
        &[("f", HISTORY_ITEM_REMOTE_REF_COLOR), ("h", HISTORY_ITEM_BASE_REF_COLOR)],
    );
}

/// ```text
/// * a(b) [origin/main]
/// * b(e)
/// | * c(d) [main]
/// | * d(e)
/// |/
/// * e(f)
/// * f(g)
/// ```
#[test]
fn graph_with_incoming_outgoing_changes_remote_ref_first() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let models = [
        to_scm_history_item("a", &["b"], Some(vec![a_ref("origin/main")])),
        to_scm_history_item("b", &["e"], None),
        to_scm_history_item("c", &["d"], Some(vec![a_ref("main")])),
        to_scm_history_item("d", &["e"], None),
        to_scm_history_item("e", &["f"], None),
        to_scm_history_item("f", &["g"], None),
    ];

    let color_map = color_map(&[("origin/main", HISTORY_ITEM_REMOTE_REF_COLOR), ("main", HISTORY_ITEM_REF_COLOR)]);

    let view_models = to_iscm_history_item_view_model_array(
        &models,
        Some(&color_map),
        Some(&a_ref_with_revision("main", "c")),
        Some(&a_ref_with_revision("origin/main", "a")),
        None,
        true,
        true,
        Some("e"),
    );

    assert_eq!(view_models.len(), 8);

    // node a
    assert_kind(&view_models[0], HistoryItemViewModelKind::Node);
    assert_node(&view_models[0], &[], &[("b", HISTORY_ITEM_REMOTE_REF_COLOR)]);

    // node b
    assert_kind(&view_models[1], HistoryItemViewModelKind::Node);
    assert_eq!(view_models[1].input_swimlanes.len(), 1);
    assert_eq!(view_models[1].input_swimlanes[0].color, HISTORY_ITEM_REMOTE_REF_COLOR);
    assert_swimlanes(&view_models[1].output_swimlanes, &[("e", HISTORY_ITEM_REMOTE_REF_COLOR)]);

    // outgoing changes node
    assert_kind(&view_models[2], HistoryItemViewModelKind::OutgoingChanges);
    assert_node(
        &view_models[2],
        &[("e", HISTORY_ITEM_REMOTE_REF_COLOR)],
        &[("e", HISTORY_ITEM_REMOTE_REF_COLOR), ("c", HISTORY_ITEM_REF_COLOR)],
    );

    // node c
    assert_kind(&view_models[3], HistoryItemViewModelKind::Head);
    assert_node(
        &view_models[3],
        &[("e", HISTORY_ITEM_REMOTE_REF_COLOR), ("c", HISTORY_ITEM_REF_COLOR)],
        &[("e", HISTORY_ITEM_REMOTE_REF_COLOR), ("d", HISTORY_ITEM_REF_COLOR)],
    );

    // node d
    assert_kind(&view_models[4], HistoryItemViewModelKind::Node);
    assert_node(
        &view_models[4],
        &[(SCM_INCOMING_HISTORY_ITEM_ID, HISTORY_ITEM_REMOTE_REF_COLOR), ("d", HISTORY_ITEM_REF_COLOR)],
        &[(SCM_INCOMING_HISTORY_ITEM_ID, HISTORY_ITEM_REMOTE_REF_COLOR), ("e", HISTORY_ITEM_REF_COLOR)],
    );

    // incoming changes node
    assert_kind(&view_models[5], HistoryItemViewModelKind::IncomingChanges);
    assert_node(
        &view_models[5],
        &[(SCM_INCOMING_HISTORY_ITEM_ID, HISTORY_ITEM_REMOTE_REF_COLOR), ("e", HISTORY_ITEM_REF_COLOR)],
        &[("e", HISTORY_ITEM_REMOTE_REF_COLOR), ("e", HISTORY_ITEM_REF_COLOR)],
    );

    // node e
    assert_kind(&view_models[6], HistoryItemViewModelKind::Node);
    assert_node(
        &view_models[6],
        &[("e", HISTORY_ITEM_REMOTE_REF_COLOR), ("e", HISTORY_ITEM_REF_COLOR)],
        &[("f", HISTORY_ITEM_REMOTE_REF_COLOR)],
    );

    // node f
    assert_kind(&view_models[7], HistoryItemViewModelKind::Node);
    assert_node(&view_models[7], &[("f", HISTORY_ITEM_REMOTE_REF_COLOR)], &[("g", HISTORY_ITEM_REMOTE_REF_COLOR)]);
}

/// ```text
/// * a(b) [main]
/// * b(e)
/// | * c(d) [origin/main]
/// | * d(e)
/// |/
/// * e(f)
/// * f(g)
/// ```
#[test]
fn graph_with_incoming_outgoing_changes_local_ref_first() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let models = [
        to_scm_history_item("a", &["b"], Some(vec![a_ref("main")])),
        to_scm_history_item("b", &["e"], None),
        to_scm_history_item("c", &["d"], Some(vec![a_ref("origin/main")])),
        to_scm_history_item("d", &["e"], None),
        to_scm_history_item("e", &["f"], None),
        to_scm_history_item("f", &["g"], None),
    ];

    let color_map = color_map(&[("origin/main", HISTORY_ITEM_REMOTE_REF_COLOR), ("main", HISTORY_ITEM_REF_COLOR)]);

    let view_models = to_iscm_history_item_view_model_array(
        &models,
        Some(&color_map),
        Some(&a_ref_with_revision("main", "a")),
        Some(&a_ref_with_revision("origin/main", "c")),
        None,
        true,
        true,
        Some("e"),
    );

    assert_eq!(view_models.len(), 8);

    // outgoing changes node
    assert_kind(&view_models[0], HistoryItemViewModelKind::OutgoingChanges);
    assert_node(&view_models[0], &[], &[("a", HISTORY_ITEM_REF_COLOR)]);

    // node a
    assert_kind(&view_models[1], HistoryItemViewModelKind::Head);
    assert_node(&view_models[1], &[("a", HISTORY_ITEM_REF_COLOR)], &[("b", HISTORY_ITEM_REF_COLOR)]);

    // node b
    assert_kind(&view_models[2], HistoryItemViewModelKind::Node);
    assert_node(&view_models[2], &[("b", HISTORY_ITEM_REF_COLOR)], &[("e", HISTORY_ITEM_REF_COLOR)]);

    // node c
    assert_kind(&view_models[3], HistoryItemViewModelKind::Node);
    assert_node(
        &view_models[3],
        &[("e", HISTORY_ITEM_REF_COLOR)],
        &[("e", HISTORY_ITEM_REF_COLOR), ("d", HISTORY_ITEM_REMOTE_REF_COLOR)],
    );

    // node d
    assert_kind(&view_models[4], HistoryItemViewModelKind::Node);
    assert_node(
        &view_models[4],
        &[("e", HISTORY_ITEM_REF_COLOR), ("d", HISTORY_ITEM_REMOTE_REF_COLOR)],
        &[("e", HISTORY_ITEM_REF_COLOR), (SCM_INCOMING_HISTORY_ITEM_ID, HISTORY_ITEM_REMOTE_REF_COLOR)],
    );

    // incoming changes node
    assert_kind(&view_models[5], HistoryItemViewModelKind::IncomingChanges);
    assert_node(
        &view_models[5],
        &[("e", HISTORY_ITEM_REF_COLOR), (SCM_INCOMING_HISTORY_ITEM_ID, HISTORY_ITEM_REMOTE_REF_COLOR)],
        &[("e", HISTORY_ITEM_REF_COLOR), ("e", HISTORY_ITEM_REMOTE_REF_COLOR)],
    );

    // node e
    assert_kind(&view_models[6], HistoryItemViewModelKind::Node);
    assert_node(
        &view_models[6],
        &[("e", HISTORY_ITEM_REF_COLOR), ("e", HISTORY_ITEM_REMOTE_REF_COLOR)],
        &[("f", HISTORY_ITEM_REF_COLOR)],
    );

    // node f
    assert_kind(&view_models[7], HistoryItemViewModelKind::Node);
    assert_node(&view_models[7], &[("f", HISTORY_ITEM_REF_COLOR)], &[("g", HISTORY_ITEM_REF_COLOR)]);
}

/// ```text
/// * a(b) [origin/main]
/// * b(c,d)
/// |\
/// | * c(e) [main]
/// * | d(e)
///  |/
/// * e(f)
/// * f(g)
/// ```
#[test]
fn graph_with_merged_incoming_changes() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let models = [
        to_scm_history_item("a", &["b"], Some(vec![a_ref("origin/main")])),
        to_scm_history_item("b", &["c", "d"], None),
        to_scm_history_item("c", &["e"], Some(vec![a_ref("main")])),
        to_scm_history_item("d", &["e"], None),
        to_scm_history_item("e", &["f"], None),
        to_scm_history_item("f", &["g"], None),
    ];

    let color_map = color_map(&[("origin/main", HISTORY_ITEM_REMOTE_REF_COLOR), ("main", HISTORY_ITEM_REF_COLOR)]);

    let view_models = to_iscm_history_item_view_model_array(
        &models,
        Some(&color_map),
        Some(&a_ref_with_revision("main", "c")),
        Some(&a_ref_with_revision("origin/main", "a")),
        None,
        true,
        true,
        Some("c"),
    );

    assert_eq!(view_models.len(), 6);

    // node a
    assert_kind(&view_models[0], HistoryItemViewModelKind::Node);
    assert_node(&view_models[0], &[], &[("b", HISTORY_ITEM_REMOTE_REF_COLOR)]);

    // node b
    assert_kind(&view_models[1], HistoryItemViewModelKind::Node);
    assert_eq!(view_models[1].input_swimlanes.len(), 1);
    assert_eq!(view_models[1].input_swimlanes[0].color, HISTORY_ITEM_REMOTE_REF_COLOR);
    assert_swimlanes(&view_models[1].output_swimlanes, &[("c", HISTORY_ITEM_REMOTE_REF_COLOR), ("d", COLOR_REGISTRY[0])]);

    // node c
    assert_kind(&view_models[2], HistoryItemViewModelKind::Head);
    assert_node(
        &view_models[2],
        &[("c", HISTORY_ITEM_REMOTE_REF_COLOR), ("d", COLOR_REGISTRY[0])],
        &[("e", HISTORY_ITEM_REF_COLOR), ("d", COLOR_REGISTRY[0])],
    );

    // node d
    assert_kind(&view_models[3], HistoryItemViewModelKind::Node);
    assert_node(
        &view_models[3],
        &[("e", HISTORY_ITEM_REF_COLOR), ("d", COLOR_REGISTRY[0])],
        &[("e", HISTORY_ITEM_REF_COLOR), ("e", COLOR_REGISTRY[0])],
    );

    // node e
    assert_kind(&view_models[4], HistoryItemViewModelKind::Node);
    assert_node(&view_models[4], &[("e", HISTORY_ITEM_REF_COLOR), ("e", COLOR_REGISTRY[0])], &[("f", HISTORY_ITEM_REF_COLOR)]);

    // node f
    assert_kind(&view_models[5], HistoryItemViewModelKind::Node);
    assert_node(&view_models[5], &[("f", HISTORY_ITEM_REF_COLOR)], &[("g", HISTORY_ITEM_REF_COLOR)]);
}
