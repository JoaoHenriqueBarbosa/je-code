//! Porte de `src/vs/workbench/test/browser/parts/editor/filteredEditorGroupModel.test.ts` (suíte
//! `FilteredEditorGroupModel`), caso a caso e na mesma ordem.
//!
//! Notas de tradução: ver o módulo `editor_group_model_suite`, que tem o `inst()`, o `createEditorGroupModel`, as entradas
//! de teste e o `closeAllEditors`, compartilhados com `editorGroupModel.test.ts`. Aqui:
//! - `new StickyEditorGroupModel(model)` e `new UnstickyEditorGroupModel(model)` são o `new(model)` dos apelidos de tipo
//!   (ver o porte de `filteredEditorGroupModel.ts`), e o `disposables.add(...)` é `add_rc`.
//! - `assert.strictEqual(a, b)` entre entradas compara a identidade do objeto (`assert_same`, `assert_is`), `null` e
//!   `undefined` são `None` (`assert_none`), e `group.getEditors(order, { excludeSticky })` usa `exclude_sticky`.
//! - Os contadores de `EDITOR_DIRTY` e `EDITOR_LABEL` são `Suite::count_events`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
#[macro_use]
#[path = "editor_group_model_suite/mod.rs"]
mod suite;

use je_workbench::common::editor::editor_group_model::IReadonlyEditorGroupModel;
use je_workbench::common::editor::filtered_editor_group_model::{StickyEditorGroupModel, UnstickyEditorGroupModel};
use je_workbench::common::editor::{EditorInputRef, EditorsOrder, GroupModelChangeKind};
use suite::{Suite, assert_is, assert_none, assert_same, close, close_all_editors, ed, exclude_sticky, test_input, typed};

/// `group.indexOf(editor)`.
fn index_of(model: &dyn IReadonlyEditorGroupModel, editor: &EditorInputRef) -> i64 {
    model.index_of(Some(&typed(editor)), None, None)
}

#[test]
fn sticky_unsticky_count() {
    let s = Suite::new();

    let model = s.group();

    let sticky_filtered_editor_group = s.disposables.add_rc(StickyEditorGroupModel::new(model.clone()));
    let unsticky_filtered_editor_group = s.disposables.add_rc(UnstickyEditorGroupModel::new(model.clone()));

    let input1 = s.input();
    let input2 = s.input();

    open!(model, input1, pinned: true, sticky: true);
    open!(model, input2, pinned: true, sticky: true);

    assert_eq!(sticky_filtered_editor_group.count(), 2);
    assert_eq!(unsticky_filtered_editor_group.count(), 0);

    model.unstick(&input1);

    assert_eq!(sticky_filtered_editor_group.count(), 1);
    assert_eq!(unsticky_filtered_editor_group.count(), 1);

    model.unstick(&input2);

    assert_eq!(sticky_filtered_editor_group.count(), 0);
    assert_eq!(unsticky_filtered_editor_group.count(), 2);
}

#[test]
fn sticky_unsticky_sticky_count() {
    let s = Suite::new();
    let model = s.group();

    let sticky_filtered_editor_group = s.disposables.add_rc(StickyEditorGroupModel::new(model.clone()));
    let unsticky_filtered_editor_group = s.disposables.add_rc(UnstickyEditorGroupModel::new(model.clone()));

    let input1 = s.input();
    let input2 = s.input();

    open!(model, input1, pinned: true, sticky: true);
    open!(model, input2, pinned: true, sticky: true);

    assert_eq!(sticky_filtered_editor_group.sticky_count(), 2);
    assert_eq!(unsticky_filtered_editor_group.sticky_count(), 0);

    model.unstick(&input1);

    assert_eq!(sticky_filtered_editor_group.sticky_count(), 1);
    assert_eq!(unsticky_filtered_editor_group.sticky_count(), 0);

    model.unstick(&input2);

    assert_eq!(sticky_filtered_editor_group.sticky_count(), 0);
    assert_eq!(unsticky_filtered_editor_group.sticky_count(), 0);
}

#[test]
fn sticky_unsticky_is_empty() {
    let s = Suite::new();
    let model = s.group();

    let sticky_filtered_editor_group = s.disposables.add_rc(StickyEditorGroupModel::new(model.clone()));
    let unsticky_filtered_editor_group = s.disposables.add_rc(UnstickyEditorGroupModel::new(model.clone()));

    let input1 = s.input();
    let input2 = s.input();

    open!(model, input1, pinned: true, sticky: false);
    open!(model, input2, pinned: true, sticky: false);

    assert_eq!(sticky_filtered_editor_group.count() == 0, true);
    assert_eq!(unsticky_filtered_editor_group.count() == 0, false);

    model.stick(&input1);

    assert_eq!(sticky_filtered_editor_group.count() == 0, false);
    assert_eq!(unsticky_filtered_editor_group.count() == 0, false);

    model.stick(&input2);

    assert_eq!(sticky_filtered_editor_group.count() == 0, false);
    assert_eq!(unsticky_filtered_editor_group.count() == 0, true);
}

#[test]
fn sticky_unsticky_editors() {
    let s = Suite::new();
    let model = s.group();

    let sticky_filtered_editor_group = s.disposables.add_rc(StickyEditorGroupModel::new(model.clone()));
    let unsticky_filtered_editor_group = s.disposables.add_rc(UnstickyEditorGroupModel::new(model.clone()));

    let input1 = s.input();
    let input2 = s.input();

    open!(model, input1, pinned: true, sticky: true);
    open!(model, input2, pinned: true, sticky: true);

    assert_eq!(sticky_filtered_editor_group.get_editors(EditorsOrder::Sequential, None).len(), 2);
    assert_eq!(unsticky_filtered_editor_group.get_editors(EditorsOrder::Sequential, None).len(), 0);

    model.unstick(&input1);

    assert_eq!(sticky_filtered_editor_group.get_editors(EditorsOrder::Sequential, None).len(), 1);
    assert_eq!(unsticky_filtered_editor_group.get_editors(EditorsOrder::Sequential, None).len(), 1);

    assert_same(&sticky_filtered_editor_group.get_editors(EditorsOrder::Sequential, None)[0], &input2);
    assert_same(&unsticky_filtered_editor_group.get_editors(EditorsOrder::Sequential, None)[0], &input1);

    model.unstick(&input2);

    assert_eq!(sticky_filtered_editor_group.get_editors(EditorsOrder::Sequential, None).len(), 0);
    assert_eq!(unsticky_filtered_editor_group.get_editors(EditorsOrder::Sequential, None).len(), 2);
}

#[test]
fn sticky_unsticky_active_editor() {
    let s = Suite::new();
    let model = s.group();

    let sticky_filtered_editor_group = s.disposables.add_rc(StickyEditorGroupModel::new(model.clone()));
    let unsticky_filtered_editor_group = s.disposables.add_rc(UnstickyEditorGroupModel::new(model.clone()));

    let input1 = s.input();
    let input2 = s.input();

    open!(model, input1, pinned: true, sticky: true, active: true);

    assert_is(sticky_filtered_editor_group.active_editor(), &input1);
    assert_none(unsticky_filtered_editor_group.active_editor());

    open!(model, input2, pinned: true, sticky: false, active: true);

    assert_none(sticky_filtered_editor_group.active_editor());
    assert_is(unsticky_filtered_editor_group.active_editor(), &input2);

    close(&model, &input1);

    assert_none(sticky_filtered_editor_group.active_editor());
    assert_is(unsticky_filtered_editor_group.active_editor(), &input2);

    close(&model, &input2);

    assert_none(sticky_filtered_editor_group.active_editor());
    assert_none(unsticky_filtered_editor_group.active_editor());
}

#[test]
fn sticky_unsticky_preview_editor() {
    let s = Suite::new();
    let model = s.group();

    let sticky_filtered_editor_group = s.disposables.add_rc(StickyEditorGroupModel::new(model.clone()));
    let unsticky_filtered_editor_group = s.disposables.add_rc(UnstickyEditorGroupModel::new(model.clone()));

    let input1 = s.input();
    let input2 = s.input();

    open!(model, input1);

    assert_none(sticky_filtered_editor_group.preview_editor());
    assert_is(unsticky_filtered_editor_group.preview_editor(), &input1);

    open!(model, input2, sticky: true);
    assert_none(sticky_filtered_editor_group.preview_editor());
    assert_is(unsticky_filtered_editor_group.preview_editor(), &input1);
}

#[test]
fn sticky_unsticky_is_sticky() {
    let s = Suite::new();
    let model = s.group();

    let sticky_filtered_editor_group = s.disposables.add_rc(StickyEditorGroupModel::new(model.clone()));
    let unsticky_filtered_editor_group = s.disposables.add_rc(UnstickyEditorGroupModel::new(model.clone()));

    let input1 = s.input();
    let input2 = s.input();

    open!(model, input1, pinned: true, sticky: true);
    open!(model, input2, pinned: true, sticky: true);

    assert!(sticky_filtered_editor_group.is_sticky(&ed(&input1)));
    assert!(sticky_filtered_editor_group.is_sticky(&ed(&input2)));

    model.unstick(&input1);
    close(&model, &input1);
    open!(model, input2, pinned: true, sticky: true);

    assert!(!unsticky_filtered_editor_group.is_sticky(&ed(&input1)));
    assert!(!unsticky_filtered_editor_group.is_sticky(&ed(&input2)));
}

#[test]
fn sticky_unsticky_is_pinned() {
    let s = Suite::new();
    let model = s.group();

    let sticky_filtered_editor_group = s.disposables.add_rc(StickyEditorGroupModel::new(model.clone()));
    let unsticky_filtered_editor_group = s.disposables.add_rc(UnstickyEditorGroupModel::new(model.clone()));

    let input1 = s.input();
    let input2 = s.input();
    let input3 = s.input();
    let input4 = s.input();

    open!(model, input1, pinned: true, sticky: true);
    open!(model, input2, pinned: true, sticky: false);
    open!(model, input3, pinned: false, sticky: true);
    open!(model, input4, pinned: false, sticky: false);

    assert!(sticky_filtered_editor_group.is_pinned(&ed(&input1)));
    assert!(unsticky_filtered_editor_group.is_pinned(&ed(&input2)));
    assert!(sticky_filtered_editor_group.is_pinned(&ed(&input3)));
    assert!(!unsticky_filtered_editor_group.is_pinned(&ed(&input4)));
}

#[test]
fn sticky_unsticky_is_active() {
    let s = Suite::new();
    let model = s.group();

    let sticky_filtered_editor_group = s.disposables.add_rc(StickyEditorGroupModel::new(model.clone()));
    let unsticky_filtered_editor_group = s.disposables.add_rc(UnstickyEditorGroupModel::new(model.clone()));

    let input1 = s.input();
    let input2 = s.input();

    open!(model, input1, pinned: true, sticky: true, active: true);

    assert!(sticky_filtered_editor_group.is_active(&typed(&input1)));

    open!(model, input2, pinned: true, sticky: false, active: true);

    assert!(!sticky_filtered_editor_group.is_active(&typed(&input1)));
    assert!(unsticky_filtered_editor_group.is_active(&typed(&input2)));

    model.unstick(&input1);

    assert!(!unsticky_filtered_editor_group.is_active(&typed(&input1)));
    assert!(unsticky_filtered_editor_group.is_active(&typed(&input2)));
}

#[test]
fn sticky_unsticky_get_editors() {
    let s = Suite::new();
    let model = s.group();

    let sticky_filtered_editor_group = s.disposables.add_rc(StickyEditorGroupModel::new(model.clone()));
    let unsticky_filtered_editor_group = s.disposables.add_rc(UnstickyEditorGroupModel::new(model.clone()));

    let input1 = s.input();
    let input2 = s.input();

    open!(model, input1, pinned: true, sticky: true, active: true);
    open!(model, input2, pinned: true, sticky: true, active: true);

    // all sticky editors
    assert_eq!(sticky_filtered_editor_group.get_editors(EditorsOrder::Sequential, None).len(), 2);
    assert_eq!(sticky_filtered_editor_group.get_editors(EditorsOrder::MostRecentlyActive, None).len(), 2);

    // no unsticky editors
    assert_eq!(unsticky_filtered_editor_group.get_editors(EditorsOrder::Sequential, None).len(), 0);
    assert_eq!(unsticky_filtered_editor_group.get_editors(EditorsOrder::MostRecentlyActive, None).len(), 0);

    // options: excludeSticky
    assert_eq!(sticky_filtered_editor_group.get_editors(EditorsOrder::Sequential, exclude_sticky(true)).len(), 0);
    assert_eq!(sticky_filtered_editor_group.get_editors(EditorsOrder::Sequential, exclude_sticky(false)).len(), 2);
    assert_eq!(unsticky_filtered_editor_group.get_editors(EditorsOrder::Sequential, exclude_sticky(true)).len(), 0);
    assert_eq!(unsticky_filtered_editor_group.get_editors(EditorsOrder::Sequential, exclude_sticky(false)).len(), 0);

    assert_same(&sticky_filtered_editor_group.get_editors(EditorsOrder::MostRecentlyActive, None)[0], &input2);
    assert_same(&sticky_filtered_editor_group.get_editors(EditorsOrder::MostRecentlyActive, None)[1], &input1);

    model.unstick(&input1);

    assert_eq!(sticky_filtered_editor_group.get_editors(EditorsOrder::Sequential, None).len(), 1);
    assert_eq!(unsticky_filtered_editor_group.get_editors(EditorsOrder::MostRecentlyActive, None).len(), 1);

    assert_same(&sticky_filtered_editor_group.get_editors(EditorsOrder::MostRecentlyActive, None)[0], &input2);
    assert_same(&unsticky_filtered_editor_group.get_editors(EditorsOrder::Sequential, None)[0], &input1);

    model.unstick(&input2);

    // all unsticky editors
    assert_eq!(sticky_filtered_editor_group.get_editors(EditorsOrder::Sequential, None).len(), 0);
    assert_eq!(unsticky_filtered_editor_group.get_editors(EditorsOrder::MostRecentlyActive, None).len(), 2);

    // order: MOST_RECENTLY_ACTIVE
    assert_same(&unsticky_filtered_editor_group.get_editors(EditorsOrder::MostRecentlyActive, None)[0], &input2);
    assert_same(&unsticky_filtered_editor_group.get_editors(EditorsOrder::MostRecentlyActive, None)[1], &input1);

    // order: SEQUENTIAL
    assert_same(&unsticky_filtered_editor_group.get_editors(EditorsOrder::Sequential, None)[0], &input2);
    assert_same(&unsticky_filtered_editor_group.get_editors(EditorsOrder::Sequential, None)[1], &input1);
}

#[test]
fn sticky_unsticky_get_editor_by_index() {
    let s = Suite::new();
    let model = s.group();

    let sticky_filtered_editor_group = s.disposables.add_rc(StickyEditorGroupModel::new(model.clone()));
    let unsticky_filtered_editor_group = s.disposables.add_rc(UnstickyEditorGroupModel::new(model.clone()));

    let input1 = s.input();
    let input2 = s.input();
    let input3 = s.input();

    open!(model, input1, pinned: true, sticky: true);
    open!(model, input2, pinned: true, sticky: true);

    assert_is(sticky_filtered_editor_group.get_editor_by_index(0), &input1);
    assert_is(sticky_filtered_editor_group.get_editor_by_index(1), &input2);
    assert_none(sticky_filtered_editor_group.get_editor_by_index(2));
    assert_none(unsticky_filtered_editor_group.get_editor_by_index(0));
    assert_none(unsticky_filtered_editor_group.get_editor_by_index(1));

    open!(model, input3, pinned: true, sticky: false);

    assert_is(sticky_filtered_editor_group.get_editor_by_index(0), &input1);
    assert_is(sticky_filtered_editor_group.get_editor_by_index(1), &input2);
    assert_none(sticky_filtered_editor_group.get_editor_by_index(2));
    assert_is(unsticky_filtered_editor_group.get_editor_by_index(0), &input3);
    assert_none(unsticky_filtered_editor_group.get_editor_by_index(1));

    model.unstick(&input1);

    assert_is(sticky_filtered_editor_group.get_editor_by_index(0), &input2);
    assert_none(sticky_filtered_editor_group.get_editor_by_index(1));
    assert_is(unsticky_filtered_editor_group.get_editor_by_index(0), &input1);
    assert_is(unsticky_filtered_editor_group.get_editor_by_index(1), &input3);
    assert_none(unsticky_filtered_editor_group.get_editor_by_index(2));
}

#[test]
fn sticky_unsticky_index_of() {
    let s = Suite::new();
    let model = s.group();

    let sticky_filtered_editor_group = s.disposables.add_rc(StickyEditorGroupModel::new(model.clone()));
    let unsticky_filtered_editor_group = s.disposables.add_rc(UnstickyEditorGroupModel::new(model.clone()));

    let input1 = s.input();
    let input2 = s.input();
    let input3 = s.input();

    open!(model, input1, pinned: true, sticky: true);
    open!(model, input2, pinned: true, sticky: true);

    assert_eq!(index_of(&*sticky_filtered_editor_group, &input1), 0);
    assert_eq!(index_of(&*sticky_filtered_editor_group, &input2), 1);
    assert_eq!(index_of(&*unsticky_filtered_editor_group, &input1), -1);
    assert_eq!(index_of(&*unsticky_filtered_editor_group, &input2), -1);

    open!(model, input3, pinned: true, sticky: false);

    assert_eq!(index_of(&*sticky_filtered_editor_group, &input1), 0);
    assert_eq!(index_of(&*sticky_filtered_editor_group, &input2), 1);
    assert_eq!(index_of(&*sticky_filtered_editor_group, &input3), -1);
    assert_eq!(index_of(&*unsticky_filtered_editor_group, &input1), -1);
    assert_eq!(index_of(&*unsticky_filtered_editor_group, &input2), -1);
    assert_eq!(index_of(&*unsticky_filtered_editor_group, &input3), 0);

    model.unstick(&input1);

    assert_eq!(index_of(&*sticky_filtered_editor_group, &input1), -1);
    assert_eq!(index_of(&*sticky_filtered_editor_group, &input2), 0);
    assert_eq!(index_of(&*sticky_filtered_editor_group, &input3), -1);
    assert_eq!(index_of(&*unsticky_filtered_editor_group, &input1), 0);
    assert_eq!(index_of(&*unsticky_filtered_editor_group, &input2), -1);
    assert_eq!(index_of(&*unsticky_filtered_editor_group, &input3), 1);
}

#[test]
fn sticky_unsticky_is_first() {
    let s = Suite::new();
    let model = s.group();

    let sticky_filtered_editor_group = s.disposables.add_rc(StickyEditorGroupModel::new(model.clone()));
    let unsticky_filtered_editor_group = s.disposables.add_rc(UnstickyEditorGroupModel::new(model.clone()));

    let input1 = s.input();
    let input2 = s.input();

    open!(model, input1, pinned: true, sticky: true);

    assert!(sticky_filtered_editor_group.is_first(&input1, None));

    open!(model, input2, pinned: true, sticky: true);

    assert!(sticky_filtered_editor_group.is_first(&input1, None));
    assert!(!sticky_filtered_editor_group.is_first(&input2, None));

    model.unstick(&input1);

    assert!(unsticky_filtered_editor_group.is_first(&input1, None));
    assert!(sticky_filtered_editor_group.is_first(&input2, None));

    model.unstick(&input2);

    assert!(!unsticky_filtered_editor_group.is_first(&input1, None));
    assert!(unsticky_filtered_editor_group.is_first(&input2, None));

    model.move_editor(&input2, 1);

    assert!(unsticky_filtered_editor_group.is_first(&input1, None));
    assert!(!unsticky_filtered_editor_group.is_first(&input2, None));
}

#[test]
fn sticky_unsticky_is_last() {
    let s = Suite::new();
    let model = s.group();

    let sticky_filtered_editor_group = s.disposables.add_rc(StickyEditorGroupModel::new(model.clone()));
    let unsticky_filtered_editor_group = s.disposables.add_rc(UnstickyEditorGroupModel::new(model.clone()));

    let input1 = s.input();
    let input2 = s.input();

    open!(model, input1, pinned: true, sticky: true);

    assert!(sticky_filtered_editor_group.is_last(&input1, None));

    open!(model, input2, pinned: true, sticky: true);

    assert!(!sticky_filtered_editor_group.is_last(&input1, None));
    assert!(sticky_filtered_editor_group.is_last(&input2, None));

    model.unstick(&input1);

    assert!(unsticky_filtered_editor_group.is_last(&input1, None));
    assert!(sticky_filtered_editor_group.is_last(&input2, None));

    model.unstick(&input2);

    assert!(unsticky_filtered_editor_group.is_last(&input1, None));
    assert!(!unsticky_filtered_editor_group.is_last(&input2, None));

    model.move_editor(&input2, 1);

    assert!(!unsticky_filtered_editor_group.is_last(&input1, None));
    assert!(unsticky_filtered_editor_group.is_last(&input2, None));
}

#[test]
fn sticky_unsticky_contains() {
    let s = Suite::new();
    let model = s.group();

    let sticky_filtered_editor_group = s.disposables.add_rc(StickyEditorGroupModel::new(model.clone()));
    let unsticky_filtered_editor_group = s.disposables.add_rc(UnstickyEditorGroupModel::new(model.clone()));

    let input1 = s.input();
    let input2 = s.input();

    open!(model, input1, pinned: true, sticky: true);
    open!(model, input2, pinned: true, sticky: true);

    assert!(sticky_filtered_editor_group.contains(&typed(&input1), None));
    assert!(sticky_filtered_editor_group.contains(&typed(&input2), None));

    assert!(!unsticky_filtered_editor_group.contains(&typed(&input1), None));
    assert!(!unsticky_filtered_editor_group.contains(&typed(&input2), None));

    model.unstick(&input1);

    assert!(!sticky_filtered_editor_group.contains(&typed(&input1), None));
    assert!(sticky_filtered_editor_group.contains(&typed(&input2), None));

    assert!(unsticky_filtered_editor_group.contains(&typed(&input1), None));
    assert!(!unsticky_filtered_editor_group.contains(&typed(&input2), None));

    model.unstick(&input2);

    assert!(!sticky_filtered_editor_group.contains(&typed(&input1), None));
    assert!(!sticky_filtered_editor_group.contains(&typed(&input2), None));

    assert!(unsticky_filtered_editor_group.contains(&typed(&input1), None));
    assert!(unsticky_filtered_editor_group.contains(&typed(&input2), None));
}

#[test]
fn sticky_unsticky_group_information() {
    let s = Suite::new();
    let model = s.group();

    let sticky_filtered_editor_group = s.disposables.add_rc(StickyEditorGroupModel::new(model.clone()));
    let unsticky_filtered_editor_group = s.disposables.add_rc(UnstickyEditorGroupModel::new(model.clone()));

    // same id
    assert_eq!(sticky_filtered_editor_group.id(), model.id());
    assert_eq!(unsticky_filtered_editor_group.id(), model.id());

    // group locking same behaviour
    assert_eq!(sticky_filtered_editor_group.is_locked(), model.is_locked());
    assert_eq!(unsticky_filtered_editor_group.is_locked(), model.is_locked());

    model.lock(true);

    assert_eq!(sticky_filtered_editor_group.is_locked(), model.is_locked());
    assert_eq!(unsticky_filtered_editor_group.is_locked(), model.is_locked());

    model.lock(false);

    assert_eq!(sticky_filtered_editor_group.is_locked(), model.is_locked());
    assert_eq!(unsticky_filtered_editor_group.is_locked(), model.is_locked());
}

#[test]
fn multiple_editors_editor_emits_dirty_and_label_changed() {
    let s = Suite::new();
    let model1 = s.group();
    let model2 = s.group();

    let sticky_filtered_editor_group1 = s.disposables.add_rc(StickyEditorGroupModel::new(model1.clone()));
    let unsticky_filtered_editor_group1 = s.disposables.add_rc(UnstickyEditorGroupModel::new(model1.clone()));
    let sticky_filtered_editor_group2 = s.disposables.add_rc(StickyEditorGroupModel::new(model2.clone()));
    let unsticky_filtered_editor_group2 = s.disposables.add_rc(UnstickyEditorGroupModel::new(model2.clone()));

    let input1 = s.input();
    let input2 = s.input();

    open!(model1, input1, pinned: true, active: true);
    open!(model2, input2, pinned: true, active: true, sticky: true);

    // DIRTY
    let dirty1_counter_sticky = s.count_events(&*sticky_filtered_editor_group1, GroupModelChangeKind::EditorDirty);
    let dirty1_counter_unsticky = s.count_events(&*unsticky_filtered_editor_group1, GroupModelChangeKind::EditorDirty);
    let dirty2_counter_sticky = s.count_events(&*sticky_filtered_editor_group2, GroupModelChangeKind::EditorDirty);
    let dirty2_counter_unsticky = s.count_events(&*unsticky_filtered_editor_group2, GroupModelChangeKind::EditorDirty);

    // LABEL
    let label1_change_counter_sticky = s.count_events(&*sticky_filtered_editor_group1, GroupModelChangeKind::EditorLabel);
    let label1_change_counter_unsticky = s.count_events(&*unsticky_filtered_editor_group1, GroupModelChangeKind::EditorLabel);
    let label2_change_counter_sticky = s.count_events(&*sticky_filtered_editor_group2, GroupModelChangeKind::EditorLabel);
    let label2_change_counter_unsticky = s.count_events(&*unsticky_filtered_editor_group2, GroupModelChangeKind::EditorLabel);

    test_input(&input1).set_dirty();
    test_input(&input1).set_label();

    assert_eq!(dirty1_counter_sticky.get(), 0);
    assert_eq!(dirty1_counter_unsticky.get(), 1);
    assert_eq!(label1_change_counter_sticky.get(), 0);
    assert_eq!(label1_change_counter_unsticky.get(), 1);

    test_input(&input2).set_dirty();
    test_input(&input2).set_label();

    assert_eq!(dirty2_counter_sticky.get(), 1);
    assert_eq!(dirty2_counter_unsticky.get(), 0);
    assert_eq!(label2_change_counter_sticky.get(), 1);
    assert_eq!(label2_change_counter_unsticky.get(), 0);

    close_all_editors(&model2);

    test_input(&input2).set_dirty();
    test_input(&input2).set_label();

    assert_eq!(dirty2_counter_sticky.get(), 1);
    assert_eq!(dirty2_counter_unsticky.get(), 0);
    assert_eq!(label2_change_counter_sticky.get(), 1);
    assert_eq!(label2_change_counter_unsticky.get(), 0);
    assert_eq!(dirty1_counter_sticky.get(), 0);
    assert_eq!(dirty1_counter_unsticky.get(), 1);
    assert_eq!(label1_change_counter_sticky.get(), 0);
    assert_eq!(label1_change_counter_unsticky.get(), 1);
}

#[test]
fn sticky_unsticky_is_transient() {
    let s = Suite::new();
    let model = s.group();

    let sticky_filtered_editor_group = s.disposables.add_rc(StickyEditorGroupModel::new(model.clone()));
    let unsticky_filtered_editor_group = s.disposables.add_rc(UnstickyEditorGroupModel::new(model.clone()));

    let input1 = s.input();
    let input2 = s.input();
    let input3 = s.input();
    let input4 = s.input();

    open!(model, input1, pinned: true, transient: false);
    open!(model, input2, pinned: true);
    open!(model, input3, pinned: true, transient: true);
    open!(model, input4, pinned: false, transient: true);

    assert!(!sticky_filtered_editor_group.is_transient(&ed(&input1)));
    assert!(!unsticky_filtered_editor_group.is_transient(&ed(&input2)));
    assert!(sticky_filtered_editor_group.is_transient(&ed(&input3)));
    assert!(unsticky_filtered_editor_group.is_transient(&ed(&input4)));
}
