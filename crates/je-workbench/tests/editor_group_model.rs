//! Porte de `src/vs/workbench/test/browser/parts/editor/editorGroupModel.test.ts` (suíte `EditorGroupModel`), caso a caso e
//! na mesma ordem.
//!
//! Notas de tradução: ver o módulo `editor_group_model_suite`, que tem o `inst()`, o `createEditorGroupModel`, as entradas
//! de teste, o serializador e o `groupListener`, compartilhados com `filteredEditorGroupModel.test.ts`. Aqui:
//! - `assert.strictEqual(a, b)` entre entradas compara a identidade do objeto (`assert_same`, `assert_is`), e `null` e
//!   `undefined` são `None` (`assert_none`).
//! - `group.isPinned(input)` e `group.isPinned(0)` são `is_pinned(&ed(&input))` e `is_pinned(&at(0))` (`EditorInput |
//!   number`), e `group.indexOf(input, undefined, options)` é `index_of(Some(&typed(&input)), None, options)`.
//! - Os casos com outro `TestInstantiationService` (`openPositioning: 'left'`, `focusRecentEditorAfterClose: false` e os
//!   de persistência) criam o `InstantiationService` próprio com `instantiation_service(...)`.
//! - `group.clone()` é `clone_model()`, e `IUntypedEditorInput` com `override` é um `UntypedEditorInput` com
//!   `TextEditorOptions::with_override_id`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
#[macro_use]
#[path = "editor_group_model_suite/mod.rs"]
mod suite;

use std::cell::Cell;
use std::rc::Rc;

use je_base::dynamic::Dynamic;
use je_base::lifecycle::IDisposable;
use je_base::uri::URI;
use je_platform::resources::uri_file;
use je_workbench::common::editor::editor_factory_registry::editor_factory_registry;
use je_workbench::common::editor::editor_group_model::{EditorGroupModel, IReadonlyEditorGroupModel};
use je_workbench::common::editor::editor_input::EditorInput;
use je_workbench::common::editor::{
    CloseDirection, EditorCloseContext, EditorsOrder, GroupModelChangeKind, MatchEditorOptions, SideBySideEditor, TextEditorOptions, UntypedEditorInput,
};
use suite::{
    Suite, any, any_strict, assert_is, assert_none, assert_same, at, both, both_strict, close, close_all_editors, close_editors, ed, editor_of, exclude_sticky,
    index_of, instantiation_service, mru, same, seq, set_disable_deserialize, set_disable_serialize, strict, test_input, typed, untyped,
};

/// `{ resource: URI.file(path), options: { override: overrideId } }`.
fn untyped_resource(path: &str, override_id: &str) -> UntypedEditorInput {
    UntypedEditorInput {
        resource: Some(uri_file(path)),
        options: Some(TextEditorOptions::with_override_id(override_id)),
        ..UntypedEditorInput::default()
    }
}

/// `{ original, modified }` (`IResourceDiffEditorInput`).
fn untyped_diff(original: &UntypedEditorInput, modified: &UntypedEditorInput) -> UntypedEditorInput {
    UntypedEditorInput { original: Some(Box::new(original.clone())), modified: Some(Box::new(modified.clone())), ..UntypedEditorInput::default() }
}

/// `{ primary, secondary }` (`IResourceSideBySideEditorInput`).
fn untyped_side_by_side(primary: &UntypedEditorInput, secondary: &UntypedEditorInput) -> UntypedEditorInput {
    UntypedEditorInput { primary: Some(Box::new(primary.clone())), secondary: Some(Box::new(secondary.clone())), ..UntypedEditorInput::default() }
}

#[test]
fn clone_group() {
    let s = Suite::new();
    let group = s.group();

    let input1 = s.input();
    let input2 = s.input();
    let input3 = s.input();

    // Pinned and Active
    open!(group, input1, pinned: true, active: true);
    open!(group, input2, pinned: true, active: true);
    open!(group, input3, pinned: false, active: true);

    // Sticky
    group.stick(&input2);
    assert!(group.is_sticky(&ed(&input2)));

    // Locked
    assert!(!group.is_locked());
    group.lock(true);
    assert!(group.is_locked());

    let clone = s.disposables.add_rc(group.clone_model());
    assert_ne!(group.id(), clone.id());
    assert_eq!(clone.count(), 3);
    assert!(!clone.is_locked()); // locking does not clone over

    let did_editor_label_change = Rc::new(Cell::new(false));
    let label_flag = did_editor_label_change.clone();
    let to_dispose = clone.on_did_model_change().listen(move |e| {
        if e.kind == GroupModelChangeKind::EditorLabel {
            label_flag.set(true);
        }
    });
    test_input(&input1).set_label();
    assert!(did_editor_label_change.get());

    assert!(clone.is_pinned(&ed(&input1)));
    assert!(!clone.is_active(&typed(&input1)));
    assert!(!clone.is_sticky(&ed(&input1)));

    assert!(clone.is_pinned(&ed(&input2)));
    assert!(!clone.is_active(&typed(&input2)));
    assert!(clone.is_sticky(&ed(&input2)));

    assert!(!clone.is_pinned(&ed(&input3)));
    assert!(clone.is_active(&typed(&input3)));
    assert!(!clone.is_sticky(&ed(&input3)));

    to_dispose.dispose();
}

#[test]
fn is_active_untyped() {
    let s = Suite::new();
    let group = s.group();
    let input = s.file("testInput", uri_file("fake"));
    let input2 = s.file("testInput2", uri_file("fake2"));
    let untyped_input = untyped_resource("/fake", "testInput");
    let untyped_non_active_input = untyped_resource("/fake2", "testInput2");

    open!(group, input, pinned: true, active: true);
    open!(group, input2, active: false);

    assert!(group.is_active(&typed(&input)));
    assert!(group.is_active(&untyped(&untyped_input)));
    assert!(!group.is_active(&untyped(&untyped_non_active_input)));
}

#[test]
fn open_editor_prefers_existing_side_by_side_editor_if_same() {
    let s = Suite::new();

    let group = s.group();
    let input1 = s.file("testInput", uri_file("fake1"));
    let input2 = s.file("testInput", uri_file("fake2"));

    let side_by_side_input_same = s.side_by_side(None, &input1, &input1);
    let side_by_side_input_different = s.side_by_side(None, &input1, &input2);

    let mut res = open!(group, side_by_side_input_same, pinned: true, active: true);
    assert_same(&res.editor, &side_by_side_input_same);
    assert!(res.is_new);

    res = open!(group, input1, pinned: true, active: true, support_side_by_side: SideBySideEditor::Both);
    assert_same(&res.editor, &side_by_side_input_same);
    assert!(!res.is_new);

    close(&group, &side_by_side_input_same);
    res = open!(group, side_by_side_input_different, pinned: true, active: true);
    assert_same(&res.editor, &side_by_side_input_different);
    assert!(res.is_new);

    res = open!(group, input1, pinned: true, active: true);
    assert_same(&res.editor, &input1);
    assert!(res.is_new);
}

#[test]
fn index_of_prefers_direct_matching_editor_over_side_by_side_matching_one() {
    let s = Suite::new();

    let group = s.group();
    let input1 = s.file("testInput", uri_file("fake1"));

    let side_by_side_input = s.side_by_side(None, &input1, &input1);

    open!(group, side_by_side_input, pinned: true, active: true);
    assert_eq!(index_of(&group, &side_by_side_input), 0);
    assert_eq!(index_of(&group, &input1), -1);
    assert_eq!(group.index_of(Some(&typed(&input1)), None, both()), 0);
    assert_eq!(group.index_of(Some(&typed(&input1)), None, any()), 0);

    open!(group, input1, pinned: true, active: true);
    assert_eq!(index_of(&group, &input1), 1);
    assert_eq!(group.index_of(Some(&typed(&input1)), None, both()), 1);
    assert_eq!(group.index_of(Some(&typed(&input1)), None, any()), 1);
}

#[test]
fn contains_untyped() {
    let s = Suite::new();
    let group = s.group();

    let input1 = s.file("input1", uri_file("/input1"));
    let input2 = s.file("input2", uri_file("/input2"));

    let untyped_input1 = untyped_resource("/input1", "input1");
    let untyped_input2 = untyped_resource("/input2", "input2");

    let diff_input1 = s.diff(&input1, &input2);
    let diff_input2 = s.diff(&input2, &input1);

    let untyped_diff_input1 = untyped_diff(&untyped_input1, &untyped_input2);
    let untyped_diff_input2 = untyped_diff(&untyped_input2, &untyped_input1);

    let side_by_side_input_same = s.side_by_side(Some("name"), &input1, &input1);
    let side_by_side_input_different = s.side_by_side(Some("name"), &input1, &input2);

    let untyped_side_by_side_input_same = untyped_side_by_side(&untyped_input1, &untyped_input1);
    let untyped_side_by_side_input_different = untyped_side_by_side(&untyped_input2, &untyped_input1);

    let contains = |candidate: &UntypedEditorInput, options: Option<MatchEditorOptions>| group.contains(&untyped(candidate), options);

    open!(group, input1, pinned: true, active: true);

    assert!(contains(&untyped_input1, None));
    assert!(!contains(&untyped_input1, strict()));
    assert!(contains(&untyped_input1, any()));
    assert!(contains(&untyped_input1, both()));
    assert!(!contains(&untyped_input2, None));
    assert!(!contains(&untyped_input2, strict()));
    assert!(!contains(&untyped_input2, any()));
    assert!(!contains(&untyped_input2, both()));
    assert!(!contains(&untyped_diff_input1, None));
    assert!(!contains(&untyped_diff_input2, None));

    open!(group, input2, pinned: true, active: true);

    assert!(contains(&untyped_input1, None));
    assert!(contains(&untyped_input2, None));
    assert!(!contains(&untyped_diff_input1, None));
    assert!(!contains(&untyped_diff_input2, None));

    open!(group, diff_input1, pinned: true, active: true);

    assert!(contains(&untyped_input1, None));
    assert!(contains(&untyped_input2, None));
    assert!(contains(&untyped_diff_input1, None));
    assert!(!contains(&untyped_diff_input2, None));

    open!(group, diff_input2, pinned: true, active: true);

    assert!(contains(&untyped_input1, None));
    assert!(contains(&untyped_input2, None));
    assert!(contains(&untyped_diff_input1, None));
    assert!(contains(&untyped_diff_input2, None));

    close(&group, &input1);

    assert!(!contains(&untyped_input1, None));
    assert!(contains(&untyped_input1, any()));
    assert!(!contains(&untyped_input1, both()));
    assert!(contains(&untyped_input2, None));
    assert!(contains(&untyped_diff_input1, None));
    assert!(contains(&untyped_diff_input2, None));

    close(&group, &input2);

    assert!(!contains(&untyped_input1, None));
    assert!(contains(&untyped_input1, any()));
    assert!(!contains(&untyped_input2, None));
    assert!(contains(&untyped_input2, any()));
    assert!(contains(&untyped_diff_input1, None));
    assert!(contains(&untyped_diff_input2, None));

    close(&group, &diff_input1);

    assert!(!contains(&untyped_input1, None));
    assert!(contains(&untyped_input1, any()));
    assert!(!contains(&untyped_input2, None));
    assert!(contains(&untyped_input2, any()));
    assert!(!contains(&untyped_diff_input1, None));
    assert!(contains(&untyped_diff_input2, None));

    close(&group, &diff_input2);

    assert!(!contains(&untyped_input1, None));
    assert!(!contains(&untyped_input1, any()));
    assert!(!contains(&untyped_input2, None));
    assert!(!contains(&untyped_input2, any()));
    assert!(!contains(&untyped_diff_input1, None));
    assert!(!contains(&untyped_diff_input2, None));

    assert_eq!(group.count(), 0);
    open!(group, side_by_side_input_same, pinned: true, active: true);
    assert!(contains(&untyped_side_by_side_input_same, None));
    assert!(contains(&untyped_input1, any()));
    assert!(contains(&untyped_input1, both()));
    assert!(!contains(&untyped_input1, any_strict()));
    assert!(!contains(&untyped_input1, both_strict()));

    close(&group, &side_by_side_input_same);

    assert_eq!(group.count(), 0);
    open!(group, side_by_side_input_different, pinned: true, active: true);
    assert!(contains(&untyped_side_by_side_input_different, None));
    assert!(contains(&untyped_input1, any()));
    assert!(!contains(&untyped_input1, both()));
}

#[test]
fn contains() {
    let s = Suite::new();
    let group = s.group();

    let input1 = s.input();
    let input2 = s.input();

    let diff_input1 = s.diff(&input1, &input2);
    let diff_input2 = s.diff(&input2, &input1);

    let side_by_side_input_same = s.side_by_side(Some("name"), &input1, &input1);
    let side_by_side_input_different = s.side_by_side(Some("name"), &input1, &input2);

    let contains = |candidate: &_, options: Option<MatchEditorOptions>| group.contains(&typed(candidate), options);

    open!(group, input1, pinned: true, active: true);

    assert!(contains(&input1, None));
    assert!(contains(&input1, strict()));
    assert!(contains(&input1, any()));
    assert!(!contains(&input2, None));
    assert!(!contains(&input2, strict()));
    assert!(!contains(&input2, any()));
    assert!(!contains(&diff_input1, None));
    assert!(!contains(&diff_input2, None));

    open!(group, input2, pinned: true, active: true);

    assert!(contains(&input1, None));
    assert!(contains(&input2, None));
    assert!(!contains(&diff_input1, None));
    assert!(!contains(&diff_input2, None));

    open!(group, diff_input1, pinned: true, active: true);

    assert!(contains(&input1, None));
    assert!(contains(&input2, None));
    assert!(contains(&diff_input1, None));
    assert!(!contains(&diff_input2, None));

    open!(group, diff_input2, pinned: true, active: true);

    assert!(contains(&input1, None));
    assert!(contains(&input2, None));
    assert!(contains(&diff_input1, None));
    assert!(contains(&diff_input2, None));

    close(&group, &input1);

    assert!(!contains(&input1, None));
    assert!(contains(&input1, any()));
    assert!(contains(&input2, None));
    assert!(contains(&diff_input1, None));
    assert!(contains(&diff_input2, None));

    close(&group, &input2);

    assert!(!contains(&input1, None));
    assert!(contains(&input1, any()));
    assert!(!contains(&input2, None));
    assert!(contains(&input2, any()));
    assert!(contains(&diff_input1, None));
    assert!(contains(&diff_input2, None));

    close(&group, &diff_input1);

    assert!(!contains(&input1, None));
    assert!(contains(&input1, any()));
    assert!(!contains(&input2, None));
    assert!(contains(&input2, any()));
    assert!(!contains(&diff_input1, None));
    assert!(contains(&diff_input2, None));

    close(&group, &diff_input2);

    assert!(!contains(&input1, None));
    assert!(!contains(&input1, any()));
    assert!(!contains(&input2, None));
    assert!(!contains(&input2, any()));
    assert!(!contains(&diff_input1, None));
    assert!(!contains(&diff_input2, None));

    let input3 = s.file(&s.next_id(), URI::parse("foo://bar", false));

    let input4 = s.file(&s.next_id(), URI::parse("foo://barsomething", false));

    open!(group, input3, pinned: true, active: true);
    assert!(!contains(&input4, None));
    assert!(contains(&input3, None));

    close(&group, &input3);

    assert!(!contains(&input3, None));

    assert_eq!(group.count(), 0);
    open!(group, side_by_side_input_same, pinned: true, active: true);

    assert!(contains(&side_by_side_input_same, None));
    assert!(contains(&input1, any()));
    assert!(contains(&input1, both()));
    assert!(contains(&input1, any_strict()));
    assert!(contains(&input1, both_strict()));

    close(&group, &side_by_side_input_same);

    assert_eq!(group.count(), 0);
    open!(group, side_by_side_input_different, pinned: true, active: true);
    assert!(contains(&side_by_side_input_different, None));
    assert!(contains(&input1, any()));
    assert!(contains(&input1, any_strict()));
    assert!(!contains(&input1, both()));
    assert!(!contains(&input1, both_strict()));
}

#[test]
fn group_serialization() {
    let s = Suite::new();
    s.instantiation_service.invoke_function(|accessor| editor_factory_registry().start(accessor));
    let group = s.group();

    let input1 = s.input();
    let input2 = s.input();
    let input3 = s.input();

    // Case 1: inputs can be serialized and deserialized

    open!(group, input1, pinned: true, active: true);
    open!(group, input2, pinned: true, active: true);
    open!(group, input3, pinned: false, active: true);

    let mut deserialized = s.create_editor_group_model(Some(group.serialize()));
    assert_eq!(group.id(), deserialized.id());
    assert_eq!(deserialized.count(), 3);
    assert_eq!(seq(&deserialized).len(), 3);
    assert_eq!(mru(&deserialized).len(), 3);
    assert!(deserialized.is_pinned(&ed(&input1)));
    assert!(deserialized.is_pinned(&ed(&input2)));
    assert!(!deserialized.is_pinned(&ed(&input3)));
    assert!(deserialized.is_active(&typed(&input3)));

    // Case 2: inputs cannot be serialized
    set_disable_serialize(true);

    deserialized = s.create_editor_group_model(Some(group.serialize()));
    assert_eq!(group.id(), deserialized.id());
    assert_eq!(deserialized.count(), 0);
    assert_eq!(seq(&deserialized).len(), 0);
    assert_eq!(mru(&deserialized).len(), 0);

    // Case 3: inputs cannot be deserialized
    set_disable_serialize(false);
    set_disable_deserialize(true);

    deserialized = s.create_editor_group_model(Some(group.serialize()));
    assert_eq!(group.id(), deserialized.id());
    assert_eq!(deserialized.count(), 0);
    assert_eq!(seq(&deserialized).len(), 0);
    assert_eq!(mru(&deserialized).len(), 0);
}

#[test]
fn group_serialization_sticky_editor() {
    let s = Suite::new();
    s.instantiation_service.invoke_function(|accessor| editor_factory_registry().start(accessor));
    let group = s.group();

    let input1 = s.input();
    let input2 = s.input();
    let input3 = s.input();

    // Case 1: inputs can be serialized and deserialized

    open!(group, input1, pinned: true, active: true);
    open!(group, input2, pinned: true, active: true);
    open!(group, input3, pinned: false, active: true);

    group.stick(&input2);
    assert!(group.is_sticky(&ed(&input2)));

    let mut deserialized = s.create_editor_group_model(Some(group.serialize()));
    assert_eq!(group.id(), deserialized.id());
    assert_eq!(deserialized.count(), 3);

    assert!(deserialized.is_pinned(&ed(&input1)));
    assert!(!deserialized.is_active(&typed(&input1)));
    assert!(!deserialized.is_sticky(&ed(&input1)));

    assert!(deserialized.is_pinned(&ed(&input2)));
    assert!(!deserialized.is_active(&typed(&input2)));
    assert!(deserialized.is_sticky(&ed(&input2)));

    assert!(!deserialized.is_pinned(&ed(&input3)));
    assert!(deserialized.is_active(&typed(&input3)));
    assert!(!deserialized.is_sticky(&ed(&input3)));

    // Case 2: inputs cannot be serialized
    set_disable_serialize(true);

    deserialized = s.create_editor_group_model(Some(group.serialize()));
    assert_eq!(group.id(), deserialized.id());
    assert_eq!(deserialized.count(), 0);
    assert_eq!(deserialized.sticky_count(), 0);
    assert_eq!(seq(&deserialized).len(), 0);
    assert_eq!(mru(&deserialized).len(), 0);

    // Case 3: inputs cannot be deserialized
    set_disable_serialize(false);
    set_disable_deserialize(true);

    deserialized = s.create_editor_group_model(Some(group.serialize()));
    assert_eq!(group.id(), deserialized.id());
    assert_eq!(deserialized.count(), 0);
    assert_eq!(deserialized.sticky_count(), 0);
    assert_eq!(seq(&deserialized).len(), 0);
    assert_eq!(mru(&deserialized).len(), 0);
}

#[test]
fn group_serialization_locked_group() {
    let s = Suite::new();
    let group = s.group();
    let events = s.group_listener(&group);

    assert_eq!(events.locked().len(), 0);

    group.lock(true);
    group.lock(true);

    assert_eq!(events.locked().len(), 1);

    group.lock(false);
    group.lock(false);

    assert_eq!(events.locked().len(), 2);
}

#[test]
fn locked_group() {
    let s = Suite::new();
    let group = s.group();
    group.lock(true);

    let mut deserialized = s.create_editor_group_model(Some(group.serialize()));
    assert_eq!(group.id(), deserialized.id());
    assert_eq!(deserialized.count(), 0);
    assert!(deserialized.is_locked());

    group.lock(false);
    deserialized = s.create_editor_group_model(Some(group.serialize()));
    assert_eq!(group.id(), deserialized.id());
    assert_eq!(deserialized.count(), 0);
    assert!(!deserialized.is_locked());
}

#[test]
fn index() {
    let s = Suite::new();
    let group = s.group();
    let events = s.group_listener(&group);

    assert_eq!(events.index().len(), 0);

    group.set_index(4);

    assert_eq!(events.index().len(), 1);
}

#[test]
fn label() {
    let s = Suite::new();
    let group = s.group();
    let events = s.group_listener(&group);

    assert_eq!(events.label().len(), 0);

    group.set_label("Window 1");

    assert_eq!(events.label().len(), 1);
}

#[test]
fn active() {
    let s = Suite::new();
    let group = s.group();
    let events = s.group_listener(&group);

    assert_eq!(events.active().len(), 0);

    group.set_active(None);

    assert_eq!(events.active().len(), 1);
}

#[test]
fn one_editor() {
    let s = Suite::new();
    let group = s.group();
    let events = s.group_listener(&group);

    assert_eq!(group.count(), 0);
    assert_eq!(mru(&group).len(), 0);

    // Active && Pinned
    let input1 = s.input();
    let opened = open!(group, input1, active: true, pinned: true);
    let (opened_editor, is_new) = (opened.editor, opened.is_new);
    assert_same(&opened_editor, &input1);
    assert!(is_new);

    assert_eq!(group.count(), 1);
    assert_eq!(mru(&group).len(), 1);
    assert_same(&group.find_editor(Some(&input1), None).unwrap().0, &input1);
    assert_is(group.active_editor(), &input1);
    assert!(group.is_active(&typed(&input1)));
    assert!(group.is_pinned(&ed(&input1)));
    assert!(group.is_pinned(&at(0)));
    assert!(group.is_first(&input1, None));
    assert!(group.is_last(&input1, None));

    assert_same(&editor_of(&events.opened()[0]), &input1);
    assert_eq!(events.opened()[0].editor_index, Some(0));
    assert_same(&editor_of(&events.activated()[0]), &input1);
    assert_eq!(events.activated()[0].editor_index, Some(0));

    let index = index_of(&group, &input1);
    assert_eq!(group.find_editor(Some(&input1), None).unwrap().1, index);
    let mut event = group.close_editor(&input1, Some(EditorCloseContext::Unpin), None);
    assert_same(&event.as_ref().unwrap().editor, &input1);
    assert_eq!(event.as_ref().unwrap().editor_index, index);
    assert_eq!(group.count(), 0);
    assert_eq!(mru(&group).len(), 0);
    assert_none(group.active_editor());
    assert!(!group.is_first(&input1, None));
    assert!(!group.is_last(&input1, None));
    assert_same(&editor_of(&events.closed()[0]), &input1);
    assert_eq!(events.closed()[0].editor_index, Some(0));
    assert_eq!(events.closed()[0].context, Some(EditorCloseContext::Unpin));

    // Active && Preview
    let input2 = s.input();
    open!(group, input2, active: true, pinned: false);

    assert_eq!(group.count(), 1);
    assert_eq!(mru(&group).len(), 1);
    assert_is(group.active_editor(), &input2);
    assert!(group.is_active(&typed(&input2)));
    assert!(!group.is_pinned(&ed(&input2)));
    assert!(!group.is_pinned(&at(0)));

    assert_same(&editor_of(&events.opened()[1]), &input2);
    assert_eq!(events.opened()[1].editor_index, Some(0));
    assert_same(&editor_of(&events.activated()[1]), &input2);
    assert_eq!(events.activated()[1].editor_index, Some(0));

    close(&group, &input2);
    assert_eq!(group.count(), 0);
    assert_eq!(mru(&group).len(), 0);
    assert_none(group.active_editor());
    assert_same(&editor_of(&events.closed()[1]), &input2);
    assert_eq!(events.closed()[1].editor_index, Some(0));
    assert_ne!(events.closed()[1].context, Some(EditorCloseContext::Replace));

    event = close(&group, &input2);
    assert!(event.is_none());
    assert_eq!(group.count(), 0);
    assert_eq!(mru(&group).len(), 0);
    assert_none(group.active_editor());
    assert_same(&editor_of(&events.closed()[1]), &input2);

    // Nonactive && Pinned => gets active because its first editor
    let input3 = s.input();
    open!(group, input3, active: false, pinned: true);

    assert_eq!(group.count(), 1);
    assert_eq!(mru(&group).len(), 1);
    assert_is(group.active_editor(), &input3);
    assert!(group.is_active(&typed(&input3)));
    assert!(group.is_pinned(&ed(&input3)));
    assert!(group.is_pinned(&at(0)));

    assert_same(&editor_of(&events.opened()[2]), &input3);
    assert_same(&editor_of(&events.activated()[2]), &input3);

    close(&group, &input3);
    assert_eq!(group.count(), 0);
    assert_eq!(mru(&group).len(), 0);
    assert_none(group.active_editor());
    assert_same(&editor_of(&events.closed()[2]), &input3);

    assert_same(&editor_of(&events.opened()[2]), &input3);
    assert_same(&editor_of(&events.activated()[2]), &input3);

    close(&group, &input3);
    assert_eq!(group.count(), 0);
    assert_eq!(mru(&group).len(), 0);
    assert_none(group.active_editor());
    assert_same(&editor_of(&events.closed()[2]), &input3);

    // Nonactive && Preview => gets active because its first editor
    let input4 = s.input();
    open!(group, input4);

    assert_eq!(group.count(), 1);
    assert_eq!(mru(&group).len(), 1);
    assert_is(group.active_editor(), &input4);
    assert!(group.is_active(&typed(&input4)));
    assert!(!group.is_pinned(&ed(&input4)));
    assert!(!group.is_pinned(&at(0)));

    assert_same(&editor_of(&events.opened()[3]), &input4);
    assert_same(&editor_of(&events.activated()[3]), &input4);

    close(&group, &input4);
    assert_eq!(group.count(), 0);
    assert_eq!(mru(&group).len(), 0);
    assert_none(group.active_editor());
    assert_same(&editor_of(&events.closed()[3]), &input4);
}

#[test]
fn multiple_editors_pinned_and_active() {
    let s = Suite::new();
    let group = s.group();
    let events = s.group_listener(&group);

    let input1 = s.named("1");
    let input1_copy = s.named("1");
    let input2 = s.named("2");
    let input3 = s.named("3");

    // Pinned and Active
    let mut opened_editor_result = open!(group, input1, pinned: true, active: true);
    assert_same(&opened_editor_result.editor, &input1);
    assert!(opened_editor_result.is_new);

    opened_editor_result = open!(group, input1_copy, pinned: true, active: true); // opening copy of editor should still return existing one
    assert_same(&opened_editor_result.editor, &input1);
    assert!(!opened_editor_result.is_new);

    open!(group, input2, pinned: true, active: true);
    open!(group, input3, pinned: true, active: true);

    assert_eq!(group.count(), 3);
    assert_eq!(mru(&group).len(), 3);
    assert_is(group.active_editor(), &input3);
    assert!(!group.is_active(&typed(&input1)));
    assert!(group.is_pinned(&ed(&input1)));
    assert!(!group.is_active(&typed(&input2)));
    assert!(group.is_pinned(&ed(&input2)));
    assert!(group.is_active(&typed(&input3)));
    assert!(group.is_pinned(&ed(&input3)));
    assert!(group.is_first(&input1, None));
    assert!(!group.is_first(&input2, None));
    assert!(!group.is_first(&input3, None));
    assert!(!group.is_last(&input1, None));
    assert!(!group.is_last(&input2, None));
    assert!(group.is_last(&input3, None));

    assert_same(&editor_of(&events.opened()[0]), &input1);
    assert_same(&editor_of(&events.opened()[1]), &input2);
    assert_same(&editor_of(&events.opened()[2]), &input3);

    assert_same(&editor_of(&events.activated()[0]), &input1);
    assert_eq!(events.activated()[0].editor_index, Some(0));
    assert_same(&editor_of(&events.activated()[1]), &input2);
    assert_eq!(events.activated()[1].editor_index, Some(1));
    assert_same(&editor_of(&events.activated()[2]), &input3);
    assert_eq!(events.activated()[2].editor_index, Some(2));

    let most_recent = mru(&group);
    assert_same(&most_recent[0], &input3);
    assert_same(&most_recent[1], &input2);
    assert_same(&most_recent[2], &input1);

    // Add some tests where a matching input is used
    // and verify that events carry the original input
    let same_input1 = s.named("1");
    open!(group, same_input1, pinned: true, active: true);
    assert_same(&editor_of(&events.activated()[3]), &input1);
    assert_eq!(events.activated()[3].editor_index, Some(0));

    group.unpin(&same_input1);
    assert_same(&editor_of(&events.unpinned()[0]), &input1);
    assert_eq!(events.unpinned()[0].editor_index, Some(0));

    group.pin(&same_input1);
    assert_same(&editor_of(&events.pinned()[0]), &input1);
    assert_eq!(events.pinned()[0].editor_index, Some(0));

    group.stick(&same_input1);
    assert_same(&editor_of(&events.sticky()[0]), &input1);
    assert_eq!(events.sticky()[0].editor_index, Some(0));

    group.unstick(&same_input1);
    assert_same(&editor_of(&events.unsticky()[0]), &input1);
    assert_eq!(events.unsticky()[0].editor_index, Some(0));

    group.move_editor(&same_input1, 1);
    assert_same(&editor_of(&events.moved()[0]), &input1);
    assert_eq!(events.moved()[0].old_editor_index, Some(0));
    assert_eq!(events.moved()[0].editor_index, Some(1));

    close(&group, &same_input1);
    assert_same(&editor_of(&events.closed()[0]), &input1);
    assert_eq!(events.closed()[0].editor_index, Some(1));

    close_all_editors(&group);

    assert_eq!(events.closed().len(), 3);
    assert_eq!(group.count(), 0);
}

#[test]
fn multiple_editors_preview_editor_moves_to_the_side_of_the_active_one() {
    let s = Suite::new();
    let group = s.group();

    let input1 = s.input();
    let input2 = s.input();
    let input3 = s.input();

    open!(group, input1, pinned: false, active: true);
    open!(group, input2, pinned: true, active: true);
    open!(group, input3, pinned: true, active: true);

    assert_same(&input3, &seq(&group)[2]);

    let input4 = s.input();
    open!(group, input4, pinned: false, active: true); // this should cause the preview editor to move after input3

    assert_same(&input4, &seq(&group)[2]);
}

#[test]
fn multiple_editors_pinned_and_active_default_open_editor_direction_left() {
    let s = Suite::new();

    let inst = instantiation_service(&s.disposables, vec![("openPositioning", Dynamic::string("left"))]);

    let group = s.disposables.add_rc(inst.create_instance::<EditorGroupModel>(None));

    let events = s.group_listener(&group);

    let input1 = s.input();
    let input2 = s.input();
    let input3 = s.input();

    // Pinned and Active
    open!(group, input1, pinned: true, active: true);
    open!(group, input2, pinned: true, active: true);
    open!(group, input3, pinned: true, active: true);

    assert_same(&seq(&group)[0], &input3);
    assert_same(&seq(&group)[1], &input2);
    assert_same(&seq(&group)[2], &input1);

    close_all_editors(&group);

    assert_eq!(events.closed().len(), 3);
    assert_eq!(group.count(), 0);
}

#[test]
fn multiple_editors_pinned_and_not_active() {
    let s = Suite::new();
    let group = s.group();

    let input1 = s.input();
    let input2 = s.input();
    let input3 = s.input();

    // Pinned and Active
    open!(group, input1, pinned: true);
    open!(group, input2, pinned: true);
    open!(group, input3, pinned: true);

    assert_eq!(group.count(), 3);
    assert_eq!(mru(&group).len(), 3);
    assert_is(group.active_editor(), &input1);
    assert!(group.is_active(&typed(&input1)));
    assert!(group.is_pinned(&ed(&input1)));
    assert!(group.is_pinned(&at(0)));
    assert!(!group.is_active(&typed(&input2)));
    assert!(group.is_pinned(&ed(&input2)));
    assert!(group.is_pinned(&at(1)));
    assert!(!group.is_active(&typed(&input3)));
    assert!(group.is_pinned(&ed(&input3)));
    assert!(group.is_pinned(&at(2)));
    assert!(group.is_pinned(&ed(&input3)));

    let most_recent = mru(&group);
    assert_same(&most_recent[0], &input1);
    assert_same(&most_recent[1], &input3);
    assert_same(&most_recent[2], &input2);
}

#[test]
fn multiple_editors_preview_gets_overwritten() {
    let s = Suite::new();
    let group = s.group();
    let events = s.group_listener(&group);

    let input1 = s.input();
    let input2 = s.input();
    let input3 = s.input();

    // Non active, preview
    open!(group, input1); // becomes active, preview
    open!(group, input2); // overwrites preview
    open!(group, input3); // overwrites preview

    assert_eq!(group.count(), 1);
    assert_eq!(mru(&group).len(), 1);
    assert_is(group.active_editor(), &input3);
    assert!(group.is_active(&typed(&input3)));
    assert!(!group.is_pinned(&ed(&input3)));

    assert_same(&editor_of(&events.opened()[0]), &input1);
    assert_same(&editor_of(&events.opened()[1]), &input2);
    assert_same(&editor_of(&events.opened()[2]), &input3);
    assert_same(&editor_of(&events.closed()[0]), &input1);
    assert_same(&editor_of(&events.closed()[1]), &input2);
    assert_eq!(events.closed()[0].context, Some(EditorCloseContext::Replace));
    assert_eq!(events.closed()[1].context, Some(EditorCloseContext::Replace));

    let most_recent = mru(&group);
    assert_same(&most_recent[0], &input3);
    assert_eq!(most_recent.len(), 1);
}

#[test]
fn multiple_editors_set_active() {
    let s = Suite::new();
    let group = s.group();
    let events = s.group_listener(&group);

    let input1 = s.input();
    let input2 = s.input();
    let input3 = s.input();

    open!(group, input1, pinned: true, active: true);
    open!(group, input2, pinned: true, active: true);
    open!(group, input3, pinned: false, active: true);

    assert_is(group.active_editor(), &input3);

    let mut most_recent = mru(&group);
    assert_same(&most_recent[0], &input3);
    assert_same(&most_recent[1], &input2);
    assert_same(&most_recent[2], &input1);

    group.set_active(Some(&input3));
    assert_eq!(events.activated().len(), 3);

    group.set_active(Some(&input1));
    assert_same(&editor_of(&events.activated()[3]), &input1);
    assert_is(group.active_editor(), &input1);
    assert!(group.is_active(&typed(&input1)));
    assert!(!group.is_active(&typed(&input2)));
    assert!(!group.is_active(&typed(&input3)));

    most_recent = mru(&group);
    assert_same(&most_recent[0], &input1);
    assert_same(&most_recent[1], &input3);
    assert_same(&most_recent[2], &input2);
}

#[test]
fn multiple_editors_pin_and_unpin() {
    let s = Suite::new();
    let group = s.group();
    let events = s.group_listener(&group);

    let input1 = s.input();
    let input2 = s.input();
    let input3 = s.input();

    open!(group, input1, pinned: true, active: true);
    open!(group, input2, pinned: true, active: true);
    open!(group, input3, pinned: false, active: true);

    assert_is(group.active_editor(), &input3);
    assert_eq!(group.count(), 3);

    group.pin(&input3);

    assert_is(group.active_editor(), &input3);
    assert!(group.is_pinned(&ed(&input3)));
    assert!(group.is_active(&typed(&input3)));
    assert_same(&editor_of(&events.pinned()[0]), &input3);
    assert_eq!(group.count(), 3);

    group.unpin(&input1);

    assert_is(group.active_editor(), &input3);
    assert!(!group.is_pinned(&ed(&input1)));
    assert!(!group.is_active(&typed(&input1)));
    assert_same(&editor_of(&events.unpinned()[0]), &input1);
    assert_eq!(group.count(), 3);

    group.unpin(&input2);

    assert_is(group.active_editor(), &input3);
    assert_eq!(group.count(), 2); // 2 previews got merged into one
    assert_same(&seq(&group)[0], &input2);
    assert_same(&seq(&group)[1], &input3);
    assert_same(&editor_of(&events.closed()[0]), &input1);
    assert_eq!(group.count(), 2);

    group.unpin(&input3);

    assert_is(group.active_editor(), &input3);
    assert_eq!(group.count(), 1); // pinning replaced the preview
    assert_same(&seq(&group)[0], &input3);
    assert_same(&editor_of(&events.closed()[1]), &input2);
    assert_eq!(group.count(), 1);
}

#[test]
fn multiple_editors_closing_picks_next_from_mru_list() {
    let s = Suite::new();
    let group = s.group();
    let events = s.group_listener(&group);

    let input1 = s.input();
    let input2 = s.input();
    let input3 = s.input();
    let input4 = s.input();
    let input5 = s.input();

    open!(group, input1, pinned: true, active: true);
    open!(group, input2, pinned: true, active: true);
    open!(group, input3, pinned: true, active: true);
    open!(group, input4, pinned: true, active: true);
    open!(group, input5, pinned: true, active: true);

    assert_is(group.active_editor(), &input5);
    assert_same(&mru(&group)[0], &input5);
    assert_eq!(group.count(), 5);

    close(&group, &input5);
    assert_is(group.active_editor(), &input4);
    assert_same(&editor_of(&events.activated()[5]), &input4);
    assert_eq!(group.count(), 4);

    group.set_active(Some(&input1));
    group.set_active(Some(&input4));
    close(&group, &input4);

    assert_is(group.active_editor(), &input1);
    assert_eq!(group.count(), 3);

    close(&group, &input1);

    assert_is(group.active_editor(), &input3);
    assert_eq!(group.count(), 2);

    group.set_active(Some(&input2));
    close(&group, &input2);

    assert_is(group.active_editor(), &input3);
    assert_eq!(group.count(), 1);

    close(&group, &input3);

    assert_none(group.active_editor());
    assert_eq!(group.count(), 0);
}

#[test]
fn multiple_editors_closing_picks_next_to_the_right() {
    let s = Suite::new();

    let inst = instantiation_service(&s.disposables, vec![("focusRecentEditorAfterClose", Dynamic::Bool(false))]);

    let group = s.disposables.add_rc(inst.create_instance::<EditorGroupModel>(None));
    let events = s.group_listener(&group);

    let input1 = s.input();
    let input2 = s.input();
    let input3 = s.input();
    let input4 = s.input();
    let input5 = s.input();

    open!(group, input1, pinned: true, active: true);
    open!(group, input2, pinned: true, active: true);
    open!(group, input3, pinned: true, active: true);
    open!(group, input4, pinned: true, active: true);
    open!(group, input5, pinned: true, active: true);

    assert_is(group.active_editor(), &input5);
    assert_same(&mru(&group)[0], &input5);
    assert_eq!(group.count(), 5);

    close(&group, &input5);
    assert_is(group.active_editor(), &input4);
    assert_same(&editor_of(&events.activated()[5]), &input4);
    assert_eq!(group.count(), 4);

    group.set_active(Some(&input1));
    close(&group, &input1);

    assert_is(group.active_editor(), &input2);
    assert_eq!(group.count(), 3);

    group.set_active(Some(&input3));
    close(&group, &input3);

    assert_is(group.active_editor(), &input4);
    assert_eq!(group.count(), 2);

    close(&group, &input4);

    assert_is(group.active_editor(), &input2);
    assert_eq!(group.count(), 1);

    close(&group, &input2);

    assert_none(group.active_editor());
    assert_eq!(group.count(), 0);
}

#[test]
fn multiple_editors_move_editor() {
    let s = Suite::new();
    let group = s.group();
    let events = s.group_listener(&group);

    let input1 = s.input();
    let input2 = s.input();
    let input3 = s.input();
    let input4 = s.input();
    let input5 = s.input();

    open!(group, input1, pinned: true, active: true);
    open!(group, input2, pinned: true, active: true);

    group.move_editor(&input1, 1);

    assert_same(&editor_of(&events.moved()[0]), &input1);
    assert_eq!(events.moved()[0].old_editor_index, Some(0));
    assert_eq!(events.moved()[0].editor_index, Some(1));
    assert_same(&seq(&group)[0], &input2);
    assert_same(&seq(&group)[1], &input1);

    group.set_active(Some(&input1));
    open!(group, input3, pinned: true, active: true);
    open!(group, input4, pinned: true, active: true);
    open!(group, input5, pinned: true, active: true);

    group.move_editor(&input4, 0);

    assert_same(&editor_of(&events.moved()[1]), &input4);
    assert_eq!(events.moved()[1].old_editor_index, Some(3));
    assert_eq!(events.moved()[1].editor_index, Some(0));
    assert_same(&editor_of(&events.moved()[1]), &input4);
    assert_same(&seq(&group)[0], &input4);
    assert_same(&seq(&group)[1], &input2);
    assert_same(&seq(&group)[2], &input1);
    assert_same(&seq(&group)[3], &input3);
    assert_same(&seq(&group)[4], &input5);

    group.move_editor(&input4, 3);
    group.move_editor(&input2, 1);

    assert_same(&seq(&group)[0], &input1);
    assert_same(&seq(&group)[1], &input2);
    assert_same(&seq(&group)[2], &input3);
    assert_same(&seq(&group)[3], &input4);
    assert_same(&seq(&group)[4], &input5);

    assert_eq!(events.moved().len(), 4);
    group.move_editor(&input1, 0);
    assert_eq!(events.moved().len(), 4);
    group.move_editor(&input1, -1);
    assert_eq!(events.moved().len(), 4);

    group.move_editor(&input5, 4);
    assert_eq!(events.moved().len(), 4);
    group.move_editor(&input5, 100);
    assert_eq!(events.moved().len(), 4);

    group.move_editor(&input5, -1);
    assert_eq!(events.moved().len(), 5);
    assert_same(&seq(&group)[0], &input5);

    group.move_editor(&input1, 100);
    assert_eq!(events.moved().len(), 6);
    assert_same(&seq(&group)[4], &input1);
}

#[test]
fn multiple_editors_move_editor_across_groups() {
    let s = Suite::new();
    let group1 = s.group();
    let group2 = s.group();

    let g1_input1 = s.input();
    let g1_input2 = s.input();
    let g2_input1 = s.input();

    open!(group1, g1_input1, active: true, pinned: true);
    open!(group1, g1_input2, active: true, pinned: true);
    open!(group2, g2_input1, active: true, pinned: true);

    // A move across groups is a close in the one group and an open in the other group at a specific index
    close(&group2, &g2_input1);
    open!(group1, g2_input1, active: true, pinned: true, index: 1);

    assert_eq!(group1.count(), 3);
    assert_same(&seq(&group1)[0], &g1_input1);
    assert_same(&seq(&group1)[1], &g2_input1);
    assert_same(&seq(&group1)[2], &g1_input2);
}

#[test]
fn multiple_editors_move_editor_across_groups_input_already_exists_in_group_1() {
    let s = Suite::new();
    let group1 = s.group();
    let group2 = s.group();

    let g1_input1 = s.input();
    let g1_input2 = s.input();
    let g1_input3 = s.input();
    let g2_input1 = g1_input2.clone();

    open!(group1, g1_input1, active: true, pinned: true);
    open!(group1, g1_input2, active: true, pinned: true);
    open!(group1, g1_input3, active: true, pinned: true);
    open!(group2, g2_input1, active: true, pinned: true);

    // A move across groups is a close in the one group and an open in the other group at a specific index
    close(&group2, &g2_input1);
    open!(group1, g2_input1, active: true, pinned: true, index: 0);

    assert_eq!(group1.count(), 3);
    assert_same(&seq(&group1)[0], &g1_input2);
    assert_same(&seq(&group1)[1], &g1_input1);
    assert_same(&seq(&group1)[2], &g1_input3);
}

#[test]
fn multiple_editors_pinned_and_non_active() {
    let s = Suite::new();
    let group = s.group();

    let input1 = s.input();
    open!(group, input1);
    assert_is(group.active_editor(), &input1);
    assert_is(group.preview_editor(), &input1);
    assert_same(&seq(&group)[0], &input1);
    assert_eq!(group.count(), 1);

    let input2 = s.input();
    open!(group, input2, pinned: true, active: false);
    assert_is(group.active_editor(), &input1);
    assert_is(group.preview_editor(), &input1);
    assert_same(&seq(&group)[0], &input1);
    assert_same(&seq(&group)[1], &input2);
    assert_eq!(group.count(), 2);

    let input3 = s.input();
    open!(group, input3, pinned: true, active: false);
    assert_is(group.active_editor(), &input1);
    assert_is(group.preview_editor(), &input1);
    assert_same(&seq(&group)[0], &input1);
    assert_same(&seq(&group)[1], &input3);
    assert_same(&seq(&group)[2], &input2);
    assert!(!group.is_pinned(&ed(&input1)));
    assert!(group.is_pinned(&ed(&input2)));
    assert!(group.is_pinned(&ed(&input3)));
    assert_eq!(group.count(), 3);
}

#[test]
fn multiple_editors_close_others_close_left_close_right() {
    let s = Suite::new();
    let group = s.group();

    let input1 = s.input();
    let input2 = s.input();
    let input3 = s.input();
    let input4 = s.input();
    let input5 = s.input();

    open!(group, input1, active: true, pinned: true);
    open!(group, input2, active: true, pinned: true);
    open!(group, input3, active: true, pinned: true);
    open!(group, input4, active: true, pinned: true);
    open!(group, input5, active: true, pinned: true);

    // Close Others
    close_editors(&group, &group.active_editor().unwrap(), None);
    assert_is(group.active_editor(), &input5);
    assert_eq!(group.count(), 1);

    close_all_editors(&group);
    open!(group, input1, active: true, pinned: true);
    open!(group, input2, active: true, pinned: true);
    open!(group, input3, active: true, pinned: true);
    open!(group, input4, active: true, pinned: true);
    open!(group, input5, active: true, pinned: true);
    group.set_active(Some(&input3));

    // Close Left
    assert_is(group.active_editor(), &input3);
    close_editors(&group, &group.active_editor().unwrap(), Some(CloseDirection::Left));
    assert_is(group.active_editor(), &input3);
    assert_eq!(group.count(), 3);
    assert_same(&seq(&group)[0], &input3);
    assert_same(&seq(&group)[1], &input4);
    assert_same(&seq(&group)[2], &input5);

    close_all_editors(&group);
    open!(group, input1, active: true, pinned: true);
    open!(group, input2, active: true, pinned: true);
    open!(group, input3, active: true, pinned: true);
    open!(group, input4, active: true, pinned: true);
    open!(group, input5, active: true, pinned: true);
    group.set_active(Some(&input3));

    // Close Right
    assert_is(group.active_editor(), &input3);
    close_editors(&group, &group.active_editor().unwrap(), Some(CloseDirection::Right));
    assert_is(group.active_editor(), &input3);
    assert_eq!(group.count(), 3);
    assert_same(&seq(&group)[0], &input1);
    assert_same(&seq(&group)[1], &input2);
    assert_same(&seq(&group)[2], &input3);
}

#[test]
fn multiple_editors_real_user_example() {
    let s = Suite::new();
    let group = s.group();

    // [] -> /index.html/
    let index_html = s.named("index.html");
    let mut opened_editor = open!(group, index_html).editor;
    assert_same(&opened_editor, &index_html);
    assert_is(group.active_editor(), &index_html);
    assert_is(group.preview_editor(), &index_html);
    assert_same(&seq(&group)[0], &index_html);
    assert_eq!(group.count(), 1);

    // /index.html/ -> /index.html/
    let same_index_html = s.named("index.html");
    opened_editor = open!(group, same_index_html).editor;
    assert_same(&opened_editor, &index_html);
    assert_is(group.active_editor(), &index_html);
    assert_is(group.preview_editor(), &index_html);
    assert_same(&seq(&group)[0], &index_html);
    assert_eq!(group.count(), 1);

    // /index.html/ -> /style.css/
    let style_css = s.named("style.css");
    opened_editor = open!(group, style_css).editor;
    assert_same(&opened_editor, &style_css);
    assert_is(group.active_editor(), &style_css);
    assert_is(group.preview_editor(), &style_css);
    assert_same(&seq(&group)[0], &style_css);
    assert_eq!(group.count(), 1);

    // /style.css/ -> [/style.css/, test.js]
    let test_js = s.named("test.js");
    opened_editor = open!(group, test_js, active: true, pinned: true).editor;
    assert_same(&opened_editor, &test_js);
    assert_is(group.preview_editor(), &style_css);
    assert_is(group.active_editor(), &test_js);
    assert!(!group.is_pinned(&ed(&style_css)));
    assert!(group.is_pinned(&ed(&test_js)));
    assert_same(&seq(&group)[0], &style_css);
    assert_same(&seq(&group)[1], &test_js);
    assert_eq!(group.count(), 2);

    // [/style.css/, test.js] -> [test.js, /index.html/]
    let index_html2 = s.named("index.html");
    open!(group, index_html2, active: true);
    assert_is(group.active_editor(), &index_html2);
    assert_is(group.preview_editor(), &index_html2);
    assert!(!group.is_pinned(&ed(&index_html2)));
    assert!(group.is_pinned(&ed(&test_js)));
    assert_same(&seq(&group)[0], &test_js);
    assert_same(&seq(&group)[1], &index_html2);
    assert_eq!(group.count(), 2);

    // make test.js active
    let test_js2 = s.named("test.js");
    group.set_active(Some(&test_js2));
    assert_is(group.active_editor(), &test_js);
    assert!(group.is_active(&typed(&test_js2)));
    assert_eq!(group.count(), 2);

    // [test.js, /indexHtml/] -> [test.js, index.html]
    let index_html3 = s.named("index.html");
    group.pin(&index_html3);
    assert!(group.is_pinned(&ed(&index_html3)));
    assert_is(group.active_editor(), &test_js);

    // [test.js, index.html] -> [test.js, file.ts, index.html]
    let file_ts = s.named("file.ts");
    open!(group, file_ts, active: true, pinned: true);
    assert!(group.is_pinned(&ed(&file_ts)));
    assert_eq!(group.count(), 3);
    assert_is(group.active_editor(), &file_ts);

    // [test.js, index.html, file.ts] -> [test.js, /file.ts/, index.html]
    group.unpin(&file_ts);
    assert_eq!(group.count(), 3);
    assert!(!group.is_pinned(&ed(&file_ts)));
    assert_is(group.active_editor(), &file_ts);

    // [test.js, /file.ts/, index.html] -> [test.js, /other.ts/, index.html]
    let other_ts = s.named("other.ts");
    open!(group, other_ts, active: true);
    assert_eq!(group.count(), 3);
    assert_is(group.active_editor(), &other_ts);
    assert!(seq(&group)[0].matches(&typed(&test_js)));
    assert_same(&seq(&group)[1], &other_ts);
    assert!(seq(&group)[2].matches(&typed(&index_html)));

    // make index.html active
    let index_html4 = s.named("index.html");
    group.set_active(Some(&index_html4));
    assert_is(group.active_editor(), &index_html2);

    // [test.js, /other.ts/, index.html] -> [test.js, /other.ts/]
    close(&group, &index_html);
    assert_eq!(group.count(), 2);
    assert_is(group.active_editor(), &other_ts);
    assert!(seq(&group)[0].matches(&typed(&test_js)));
    assert_same(&seq(&group)[1], &other_ts);

    // [test.js, /other.ts/] -> [test.js]
    close(&group, &other_ts);
    assert_eq!(group.count(), 1);
    assert_is(group.active_editor(), &test_js);
    assert!(seq(&group)[0].matches(&typed(&test_js)));

    // [test.js] -> /test.js/
    group.unpin(&test_js);
    assert_eq!(group.count(), 1);
    assert_is(group.active_editor(), &test_js);
    assert!(seq(&group)[0].matches(&typed(&test_js)));
    assert!(!group.is_pinned(&ed(&test_js)));

    // /test.js/ -> []
    close(&group, &test_js);
    assert_eq!(group.count(), 0);
    assert_none(group.active_editor());
    assert_none(group.preview_editor());
}

#[test]
fn single_group_single_editor_persist() {
    let s = Suite::new();

    let inst = instantiation_service(&s.disposables, vec![("openPositioning", Dynamic::string("right"))]);

    inst.invoke_function(|accessor| editor_factory_registry().start(accessor));

    let mut group = s.group();

    let input1 = s.input();
    open!(group, input1);

    assert_eq!(group.count(), 1);
    assert!(group.active_editor().unwrap().matches(&typed(&input1)));
    assert!(group.preview_editor().unwrap().matches(&typed(&input1)));
    assert!(group.is_active(&typed(&input1)));

    // Create model again - should load from storage
    group = s.disposables.add_rc(inst.create_instance::<EditorGroupModel>(Some(group.serialize())));

    assert_eq!(group.count(), 1);
    assert!(group.active_editor().unwrap().matches(&typed(&input1)));
    assert!(group.preview_editor().unwrap().matches(&typed(&input1)));
    assert!(group.is_active(&typed(&input1)));
}

#[test]
fn multiple_groups_multiple_editors_persist() {
    let s = Suite::new();

    let inst = instantiation_service(&s.disposables, vec![("openPositioning", Dynamic::string("right"))]);

    inst.invoke_function(|accessor| editor_factory_registry().start(accessor));

    let mut group1 = s.group();

    let g1_input1 = s.input();
    let g1_input2 = s.input();
    let g1_input3 = s.input();

    open!(group1, g1_input1, active: true, pinned: true);
    open!(group1, g1_input2, active: true, pinned: false);
    open!(group1, g1_input3, active: false, pinned: true);

    let mut group2 = s.group();

    let g2_input1 = s.input();
    let g2_input2 = s.input();
    let g2_input3 = s.input();

    open!(group2, g2_input1, active: true, pinned: true);
    open!(group2, g2_input2, active: false, pinned: false);
    open!(group2, g2_input3, active: false, pinned: true);

    assert_eq!(group1.count(), 3);
    assert_eq!(group2.count(), 3);
    assert!(group1.active_editor().unwrap().matches(&typed(&g1_input2)));
    assert!(group2.active_editor().unwrap().matches(&typed(&g2_input1)));
    assert!(group1.preview_editor().unwrap().matches(&typed(&g1_input2)));
    assert!(group2.preview_editor().unwrap().matches(&typed(&g2_input2)));

    assert!(mru(&group1)[0].matches(&typed(&g1_input2)));
    assert!(mru(&group1)[1].matches(&typed(&g1_input3)));
    assert!(mru(&group1)[2].matches(&typed(&g1_input1)));

    assert!(mru(&group2)[0].matches(&typed(&g2_input1)));
    assert!(mru(&group2)[1].matches(&typed(&g2_input3)));
    assert!(mru(&group2)[2].matches(&typed(&g2_input2)));

    // Create model again - should load from storage
    group1 = s.disposables.add_rc(inst.create_instance::<EditorGroupModel>(Some(group1.serialize())));
    group2 = s.disposables.add_rc(inst.create_instance::<EditorGroupModel>(Some(group2.serialize())));

    assert_eq!(group1.count(), 3);
    assert_eq!(group2.count(), 3);
    assert!(group1.active_editor().unwrap().matches(&typed(&g1_input2)));
    assert!(group2.active_editor().unwrap().matches(&typed(&g2_input1)));
    assert!(group1.preview_editor().unwrap().matches(&typed(&g1_input2)));
    assert!(group2.preview_editor().unwrap().matches(&typed(&g2_input2)));

    assert!(mru(&group1)[0].matches(&typed(&g1_input2)));
    assert!(mru(&group1)[1].matches(&typed(&g1_input3)));
    assert!(mru(&group1)[2].matches(&typed(&g1_input1)));

    assert!(mru(&group2)[0].matches(&typed(&g2_input1)));
    assert!(mru(&group2)[1].matches(&typed(&g2_input3)));
    assert!(mru(&group2)[2].matches(&typed(&g2_input2)));
}

#[test]
fn single_group_multiple_editors_persist_some_not_persistable() {
    let s = Suite::new();

    let inst = instantiation_service(&s.disposables, vec![("openPositioning", Dynamic::string("right"))]);

    inst.invoke_function(|accessor| editor_factory_registry().start(accessor));

    let mut group = s.group();

    let serializable_input1 = s.input();
    let non_serializable_input2 = s.non_serializable("3");
    let serializable_input2 = s.input();

    open!(group, serializable_input1, active: true, pinned: true);
    open!(group, non_serializable_input2, active: true, pinned: false);
    open!(group, serializable_input2, active: false, pinned: true);

    assert_eq!(group.count(), 3);
    assert!(group.active_editor().unwrap().matches(&typed(&non_serializable_input2)));
    assert!(group.preview_editor().unwrap().matches(&typed(&non_serializable_input2)));

    assert!(mru(&group)[0].matches(&typed(&non_serializable_input2)));
    assert!(mru(&group)[1].matches(&typed(&serializable_input2)));
    assert!(mru(&group)[2].matches(&typed(&serializable_input1)));

    // Create model again - should load from storage
    group = s.disposables.add_rc(inst.create_instance::<EditorGroupModel>(Some(group.serialize())));

    assert_eq!(group.count(), 2);
    assert!(group.active_editor().unwrap().matches(&typed(&serializable_input2)));
    assert_none(group.preview_editor());

    assert!(mru(&group)[0].matches(&typed(&serializable_input2)));
    assert!(mru(&group)[1].matches(&typed(&serializable_input1)));
}

#[test]
fn single_group_multiple_editors_persist_some_not_persistable_sticky_editors() {
    let s = Suite::new();

    let inst = instantiation_service(&s.disposables, vec![("openPositioning", Dynamic::string("right"))]);

    inst.invoke_function(|accessor| editor_factory_registry().start(accessor));

    let mut group = s.group();

    let serializable_input1 = s.input();
    let non_serializable_input2 = s.non_serializable("3");
    let serializable_input2 = s.input();

    open!(group, serializable_input1, active: true, pinned: true);
    open!(group, non_serializable_input2, active: true, pinned: true, sticky: true);
    open!(group, serializable_input2, active: false, pinned: true);

    assert_eq!(group.count(), 3);
    assert_eq!(group.sticky_count(), 1);

    // Create model again - should load from storage
    group = s.disposables.add_rc(inst.create_instance::<EditorGroupModel>(Some(group.serialize())));

    assert_eq!(group.count(), 2);
    assert_eq!(group.sticky_count(), 0);
}

#[test]
fn multiple_groups_multiple_editors_persist_some_not_persistable_causes_empty_group() {
    let s = Suite::new();

    let inst = instantiation_service(&s.disposables, vec![("openPositioning", Dynamic::string("right"))]);

    inst.invoke_function(|accessor| editor_factory_registry().start(accessor));

    let mut group1 = s.group();
    let mut group2 = s.group();

    let serializable_input1 = s.input();
    let serializable_input2 = s.input();
    let non_serializable_input = s.non_serializable("2");

    open!(group1, serializable_input1, pinned: true);
    open!(group1, serializable_input2);

    open!(group2, non_serializable_input);

    // Create model again - should load from storage
    group1 = s.disposables.add_rc(inst.create_instance::<EditorGroupModel>(Some(group1.serialize())));
    group2 = s.disposables.add_rc(inst.create_instance::<EditorGroupModel>(Some(group2.serialize())));

    assert_eq!(group1.count(), 2);
    assert!(seq(&group1)[0].matches(&typed(&serializable_input1)));
    assert!(seq(&group1)[1].matches(&typed(&serializable_input2)));

    // O `group2` do original só é recriado: o grupo vazio não é conferido.
    let _ = &group2;
}

#[test]
fn multiple_editors_editor_dispose() {
    let s = Suite::new();
    let group1 = s.group();
    let group2 = s.group();

    let group1_listener = s.group_listener(&group1);
    let group2_listener = s.group_listener(&group2);

    let input1 = s.input();
    let input2 = s.input();
    let input3 = s.input();

    open!(group1, input1, pinned: true, active: true);
    open!(group1, input2, pinned: true, active: true);
    open!(group1, input3, pinned: true, active: true);

    open!(group2, input1, pinned: true, active: true);
    open!(group2, input2, pinned: true, active: true);

    input1.dispose();

    assert_eq!(group1_listener.disposed().len(), 1);
    assert_eq!(group1_listener.disposed()[0].editor_index, Some(0));
    assert_eq!(group2_listener.disposed().len(), 1);
    assert_eq!(group2_listener.disposed()[0].editor_index, Some(0));
    assert!(editor_of(&group1_listener.disposed()[0]).matches(&typed(&input1)));
    assert!(editor_of(&group2_listener.disposed()[0]).matches(&typed(&input1)));

    input3.dispose();
    assert_eq!(group1_listener.disposed().len(), 2);
    assert_eq!(group1_listener.disposed()[1].editor_index, Some(2));
    assert_eq!(group2_listener.disposed().len(), 1);
    assert!(editor_of(&group1_listener.disposed()[1]).matches(&typed(&input3)));
}

#[test]
fn preview_tab_does_not_have_a_stable_position() {
    let s = Suite::new();
    let group1 = s.group();

    let input1 = s.input();
    let input2 = s.input();
    let input3 = s.input();

    open!(group1, input1, pinned: true, active: true);
    open!(group1, input2, active: true);
    group1.set_active(Some(&input1));

    open!(group1, input3, active: true);
    assert_eq!(index_of(&group1, &input3), 1);
}

#[test]
fn multiple_editors_editor_emits_dirty_and_label_changed() {
    let s = Suite::new();
    let group1 = s.group();
    let group2 = s.group();

    let input1 = s.input();
    let input2 = s.input();

    open!(group1, input1, pinned: true, active: true);
    open!(group2, input2, pinned: true, active: true);

    let dirty1_counter = s.count_events(&*group1, GroupModelChangeKind::EditorDirty);
    let dirty2_counter = s.count_events(&*group2, GroupModelChangeKind::EditorDirty);
    let label1_change_counter = s.count_events(&*group1, GroupModelChangeKind::EditorLabel);
    let label2_change_counter = s.count_events(&*group2, GroupModelChangeKind::EditorLabel);

    test_input(&input1).set_dirty();
    test_input(&input1).set_label();

    assert_eq!(dirty1_counter.get(), 1);
    assert_eq!(label1_change_counter.get(), 1);

    test_input(&input2).set_dirty();
    test_input(&input2).set_label();

    assert_eq!(dirty2_counter.get(), 1);
    assert_eq!(label2_change_counter.get(), 1);

    close_all_editors(&group2);

    test_input(&input2).set_dirty();
    test_input(&input2).set_label();

    assert_eq!(dirty2_counter.get(), 1);
    assert_eq!(label2_change_counter.get(), 1);
    assert_eq!(dirty1_counter.get(), 1);
    assert_eq!(label1_change_counter.get(), 1);
}

#[test]
fn sticky_editors() {
    let s = Suite::new();
    let group = s.group();

    let input1 = s.input();
    let input2 = s.input();
    let input3 = s.input();
    let input4 = s.input();

    open!(group, input1, pinned: true, active: true);
    open!(group, input2, pinned: true, active: true);
    open!(group, input3, pinned: false, active: true);

    assert_eq!(group.sticky_count(), 0);

    assert_eq!(group.get_editors(EditorsOrder::Sequential, None).len(), 3);
    assert_eq!(group.get_editors(EditorsOrder::Sequential, exclude_sticky(true)).len(), 3);
    assert_eq!(group.get_editors(EditorsOrder::MostRecentlyActive, None).len(), 3);
    assert_eq!(group.get_editors(EditorsOrder::MostRecentlyActive, exclude_sticky(true)).len(), 3);

    // Stick last editor should move it first and pin
    group.stick(&input3);
    assert_eq!(group.sticky_count(), 1);
    assert_eq!(group.get_editors(EditorsOrder::MostRecentlyActive, exclude_sticky(true)).len(), 2);
    assert_eq!(group.get_editors(EditorsOrder::MostRecentlyActive, exclude_sticky(false)).len(), 3);
    assert!(!group.is_sticky(&ed(&input1)));
    assert!(!group.is_sticky(&ed(&input2)));
    assert!(group.is_sticky(&ed(&input3)));
    assert!(group.is_pinned(&ed(&input3)));
    assert_eq!(index_of(&group, &input1), 1);
    assert_eq!(index_of(&group, &input2), 2);
    assert_eq!(index_of(&group, &input3), 0);

    let mut sequential_all_editors = group.get_editors(EditorsOrder::Sequential, None);
    assert_eq!(sequential_all_editors.len(), 3);
    let mut sequential_editors_excluding_sticky = group.get_editors(EditorsOrder::Sequential, exclude_sticky(true));
    assert_eq!(sequential_editors_excluding_sticky.len(), 2);
    assert!(sequential_editors_excluding_sticky.iter().any(|editor| same(editor, &input1)));
    assert!(sequential_editors_excluding_sticky.iter().any(|editor| same(editor, &input2)));
    let mut mru_all_editors = group.get_editors(EditorsOrder::MostRecentlyActive, None);
    assert_eq!(mru_all_editors.len(), 3);
    let mut mru_editors_excluding_sticky = group.get_editors(EditorsOrder::MostRecentlyActive, exclude_sticky(true));
    assert_eq!(mru_editors_excluding_sticky.len(), 2);
    assert!(mru_editors_excluding_sticky.iter().any(|editor| same(editor, &input1)));
    assert!(mru_editors_excluding_sticky.iter().any(|editor| same(editor, &input2)));

    // Sticking same editor again is a no-op
    group.stick(&input3);
    assert!(group.is_sticky(&ed(&input3)));

    // Sticking last editor now should move it after sticky one
    group.stick(&input2);
    assert_eq!(group.sticky_count(), 2);
    assert!(!group.is_sticky(&ed(&input1)));
    assert!(group.is_sticky(&ed(&input2)));
    assert!(group.is_sticky(&ed(&input3)));
    assert_eq!(index_of(&group, &input1), 2);
    assert_eq!(index_of(&group, &input2), 1);
    assert_eq!(index_of(&group, &input3), 0);

    sequential_all_editors = group.get_editors(EditorsOrder::Sequential, None);
    assert_eq!(sequential_all_editors.len(), 3);
    sequential_editors_excluding_sticky = group.get_editors(EditorsOrder::Sequential, exclude_sticky(true));
    assert_eq!(sequential_editors_excluding_sticky.len(), 1);
    assert!(sequential_editors_excluding_sticky.iter().any(|editor| same(editor, &input1)));
    mru_all_editors = group.get_editors(EditorsOrder::MostRecentlyActive, None);
    assert_eq!(mru_all_editors.len(), 3);
    mru_editors_excluding_sticky = group.get_editors(EditorsOrder::MostRecentlyActive, exclude_sticky(true));
    assert_eq!(mru_editors_excluding_sticky.len(), 1);
    assert!(mru_editors_excluding_sticky.iter().any(|editor| same(editor, &input1)));

    // Sticking remaining editor also works
    group.stick(&input1);
    assert_eq!(group.sticky_count(), 3);
    assert!(group.is_sticky(&ed(&input1)));
    assert!(group.is_sticky(&ed(&input2)));
    assert!(group.is_sticky(&ed(&input3)));
    assert_eq!(index_of(&group, &input1), 2);
    assert_eq!(index_of(&group, &input2), 1);
    assert_eq!(index_of(&group, &input3), 0);

    sequential_all_editors = group.get_editors(EditorsOrder::Sequential, None);
    assert_eq!(sequential_all_editors.len(), 3);
    sequential_editors_excluding_sticky = group.get_editors(EditorsOrder::Sequential, exclude_sticky(true));
    assert_eq!(sequential_editors_excluding_sticky.len(), 0);
    mru_all_editors = group.get_editors(EditorsOrder::MostRecentlyActive, None);
    assert_eq!(mru_all_editors.len(), 3);
    mru_editors_excluding_sticky = group.get_editors(EditorsOrder::MostRecentlyActive, exclude_sticky(true));
    assert_eq!(mru_editors_excluding_sticky.len(), 0);

    // Unsticking moves editor after sticky ones
    group.unstick(&input3);
    assert_eq!(group.sticky_count(), 2);
    assert!(group.is_sticky(&ed(&input1)));
    assert!(group.is_sticky(&ed(&input2)));
    assert!(!group.is_sticky(&ed(&input3)));
    assert_eq!(index_of(&group, &input1), 1);
    assert_eq!(index_of(&group, &input2), 0);
    assert_eq!(index_of(&group, &input3), 2);

    // Unsticking all works
    group.unstick(&input1);
    group.unstick(&input2);
    assert_eq!(group.sticky_count(), 0);
    assert!(!group.is_sticky(&ed(&input1)));
    assert!(!group.is_sticky(&ed(&input2)));
    assert!(!group.is_sticky(&ed(&input3)));

    group.move_editor(&input1, 0);
    group.move_editor(&input2, 1);
    group.move_editor(&input3, 2);

    // Opening a new editor always opens after sticky editors
    group.stick(&input1);
    group.stick(&input2);
    group.set_active(Some(&input1));

    let events = s.group_listener(&group);

    open!(group, input4, pinned: true, active: true);
    assert_eq!(index_of(&group, &input4), 2);
    close(&group, &input4);

    assert_eq!(events.closed()[0].sticky, Some(false));

    group.set_active(Some(&input2));

    open!(group, input4, pinned: true, active: true);
    assert_eq!(index_of(&group, &input4), 2);
    close(&group, &input4);

    assert_eq!(events.closed()[1].sticky, Some(false));

    // Reset
    assert_eq!(group.sticky_count(), 2);
    assert!(group.is_sticky(&ed(&input1)));
    assert!(group.is_sticky(&ed(&input2)));
    assert!(!group.is_sticky(&ed(&input3)));
    assert_eq!(index_of(&group, &input1), 0);
    assert_eq!(index_of(&group, &input2), 1);
    assert_eq!(index_of(&group, &input3), 2);

    // Moving a sticky editor works
    group.move_editor(&input1, 1); // still moved within sticky range
    assert!(group.is_sticky(&ed(&input1)));
    assert!(group.is_sticky(&ed(&input2)));
    assert!(!group.is_sticky(&ed(&input3)));
    assert_eq!(index_of(&group, &input1), 1);
    assert_eq!(index_of(&group, &input2), 0);
    assert_eq!(index_of(&group, &input3), 2);

    group.move_editor(&input1, 0); // still moved within sticky range
    assert!(group.is_sticky(&ed(&input1)));
    assert!(group.is_sticky(&ed(&input2)));
    assert!(!group.is_sticky(&ed(&input3)));
    assert_eq!(index_of(&group, &input1), 0);
    assert_eq!(index_of(&group, &input2), 1);
    assert_eq!(index_of(&group, &input3), 2);

    group.move_editor(&input1, 2); // moved out of sticky range//
    assert!(!group.is_sticky(&ed(&input1)));
    assert!(group.is_sticky(&ed(&input2)));
    assert!(!group.is_sticky(&ed(&input3)));
    assert_eq!(index_of(&group, &input1), 2);
    assert_eq!(index_of(&group, &input2), 0);
    assert_eq!(index_of(&group, &input3), 1);

    group.move_editor(&input2, 2); // moved out of sticky range
    assert!(!group.is_sticky(&ed(&input1)));
    assert!(!group.is_sticky(&ed(&input2)));
    assert!(!group.is_sticky(&ed(&input3)));
    assert_eq!(index_of(&group, &input1), 1);
    assert_eq!(index_of(&group, &input2), 2);
    assert_eq!(index_of(&group, &input3), 0);

    // Reset
    group.move_editor(&input1, 0);
    group.move_editor(&input2, 1);
    group.move_editor(&input3, 2);
    group.stick(&input1);
    group.unstick(&input2);
    assert_eq!(group.sticky_count(), 1);
    assert!(group.is_sticky(&ed(&input1)));
    assert!(!group.is_sticky(&ed(&input2)));
    assert!(!group.is_sticky(&ed(&input3)));
    assert_eq!(index_of(&group, &input1), 0);
    assert_eq!(index_of(&group, &input2), 1);
    assert_eq!(index_of(&group, &input3), 2);

    // Moving a unsticky editor in works
    group.move_editor(&input3, 1); // still moved within unsticked range
    assert!(group.is_sticky(&ed(&input1)));
    assert!(!group.is_sticky(&ed(&input2)));
    assert!(!group.is_sticky(&ed(&input3)));
    assert_eq!(index_of(&group, &input1), 0);
    assert_eq!(index_of(&group, &input2), 2);
    assert_eq!(index_of(&group, &input3), 1);

    group.move_editor(&input3, 2); // still moved within unsticked range
    assert!(group.is_sticky(&ed(&input1)));
    assert!(!group.is_sticky(&ed(&input2)));
    assert!(!group.is_sticky(&ed(&input3)));
    assert_eq!(index_of(&group, &input1), 0);
    assert_eq!(index_of(&group, &input2), 1);
    assert_eq!(index_of(&group, &input3), 2);

    group.move_editor(&input3, 0); // moved into sticky range//
    assert!(group.is_sticky(&ed(&input1)));
    assert!(!group.is_sticky(&ed(&input2)));
    assert!(group.is_sticky(&ed(&input3)));
    assert_eq!(index_of(&group, &input1), 1);
    assert_eq!(index_of(&group, &input2), 2);
    assert_eq!(index_of(&group, &input3), 0);

    group.move_editor(&input2, 0); // moved into sticky range
    assert!(group.is_sticky(&ed(&input1)));
    assert!(group.is_sticky(&ed(&input2)));
    assert!(group.is_sticky(&ed(&input3)));
    assert_eq!(index_of(&group, &input1), 2);
    assert_eq!(index_of(&group, &input2), 0);
    assert_eq!(index_of(&group, &input3), 1);

    // Closing a sticky editor updates state properly
    group.stick(&input1);
    group.stick(&input2);
    group.unstick(&input3);
    assert_eq!(group.sticky_count(), 2);
    close(&group, &input1);
    assert_eq!(events.closed()[2].sticky, Some(true));
    assert_eq!(group.sticky_count(), 1);
    close(&group, &input2);
    assert_eq!(events.closed()[3].sticky, Some(true));
    assert_eq!(group.sticky_count(), 0);

    close_all_editors(&group);
    assert_eq!(group.sticky_count(), 0);

    // Open sticky
    open!(group, input1, sticky: true);
    assert_eq!(group.sticky_count(), 1);
    assert!(group.is_sticky(&ed(&input1)));

    open!(group, input2, pinned: true, active: true);
    assert_eq!(group.sticky_count(), 1);
    assert!(group.is_sticky(&ed(&input1)));
    assert!(!group.is_sticky(&ed(&input2)));

    open!(group, input2, sticky: true);
    assert_eq!(group.sticky_count(), 2);
    assert!(group.is_sticky(&ed(&input1)));
    assert!(group.is_sticky(&ed(&input2)));

    open!(group, input3, pinned: true, active: true);
    open!(group, input4, pinned: false, active: true, sticky: true);
    assert_eq!(group.sticky_count(), 3);
    assert!(group.is_sticky(&ed(&input1)));
    assert!(group.is_sticky(&ed(&input2)));
    assert!(!group.is_sticky(&ed(&input3)));
    assert!(group.is_sticky(&ed(&input4)));
    assert!(group.is_pinned(&ed(&input4)));

    assert_eq!(index_of(&group, &input1), 0);
    assert_eq!(index_of(&group, &input2), 1);
    assert_eq!(index_of(&group, &input3), 3);
    assert_eq!(index_of(&group, &input4), 2);
}

#[test]
fn sticky_unsticky_editors_sends_correct_editor_index() {
    let s = Suite::new();
    let group = s.group();

    let input1 = s.input();
    let input2 = s.input();
    let input3 = s.input();

    open!(group, input1, pinned: true, active: true);
    open!(group, input2, pinned: true, active: true);
    open!(group, input3, pinned: false, active: true);

    assert_eq!(group.sticky_count(), 0);

    let events = s.group_listener(&group);

    group.stick(&input3);

    assert_eq!(events.sticky()[0].editor_index, Some(0));
    assert!(group.is_sticky(&ed(&input3)));
    assert_eq!(group.sticky_count(), 1);

    group.stick(&input2);

    assert_eq!(events.sticky()[1].editor_index, Some(1));
    assert!(group.is_sticky(&ed(&input2)));
    assert_eq!(group.sticky_count(), 2);

    group.unstick(&input3);
    assert_eq!(events.unsticky()[0].editor_index, Some(1));
    assert!(!group.is_sticky(&ed(&input3)));
    assert!(group.is_sticky(&ed(&input2)));
    assert_eq!(group.sticky_count(), 1);
}

#[test]
fn on_did_move_editor_event() {
    let s = Suite::new();
    let group1 = s.group();
    let group2 = s.group();

    let input1_group1 = s.input();
    let input2_group1 = s.input();
    let input1_group2 = s.input();
    let input2_group2 = s.input();

    // Open all the editors
    open!(group1, input1_group1, pinned: true, active: true, index: 0);
    open!(group1, input2_group1, pinned: true, active: false, index: 1);
    open!(group2, input1_group2, pinned: true, active: true, index: 0);
    open!(group2, input2_group2, pinned: true, active: false, index: 1);

    let group1_events = s.group_listener(&group1);
    let group2_events = s.group_listener(&group2);

    group1.move_editor(&input1_group1, 1);
    assert_same(&editor_of(&group1_events.moved()[0]), &input1_group1);
    assert_eq!(group1_events.moved()[0].old_editor_index, Some(0));
    assert_eq!(group1_events.moved()[0].editor_index, Some(1));

    group2.move_editor(&input1_group2, 1);
    assert_same(&editor_of(&group2_events.moved()[0]), &input1_group2);
    assert_eq!(group2_events.moved()[0].old_editor_index, Some(0));
    assert_eq!(group2_events.moved()[0].editor_index, Some(1));
}

#[test]
fn on_did_openeditor_event() {
    let s = Suite::new();
    let group1 = s.group();
    let group2 = s.group();

    let group1_events = s.group_listener(&group1);
    let group2_events = s.group_listener(&group2);

    let input1_group1 = s.input();
    let input2_group1 = s.input();
    let input1_group2 = s.input();
    let input2_group2 = s.input();

    // Open all the editors
    open!(group1, input1_group1, pinned: true, active: true, index: 0);
    open!(group1, input2_group1, pinned: true, active: false, index: 1);
    open!(group2, input1_group2, pinned: true, active: true, index: 0);
    open!(group2, input2_group2, pinned: true, active: false, index: 1);

    assert_eq!(group1_events.opened().len(), 2);
    assert_same(&editor_of(&group1_events.opened()[0]), &input1_group1);
    assert_eq!(group1_events.opened()[0].editor_index, Some(0));
    assert_same(&editor_of(&group1_events.opened()[1]), &input2_group1);
    assert_eq!(group1_events.opened()[1].editor_index, Some(1));

    assert_eq!(group2_events.opened().len(), 2);
    assert_same(&editor_of(&group2_events.opened()[0]), &input1_group2);
    assert_eq!(group2_events.opened()[0].editor_index, Some(0));
    assert_same(&editor_of(&group2_events.opened()[1]), &input2_group2);
    assert_eq!(group2_events.opened()[1].editor_index, Some(1));
}

#[test]
fn moving_editor_sends_sticky_event_when_sticky_changes() {
    let s = Suite::new();
    let group1 = s.group();

    let input1_group1 = s.input();
    let input2_group1 = s.input();
    let input3_group1 = s.input();

    // Open all the editors
    open!(group1, input1_group1, pinned: true, active: true, index: 0, sticky: true);
    open!(group1, input2_group1, pinned: true, active: false, index: 1);
    open!(group1, input3_group1, pinned: true, active: false, index: 2);

    let group1_events = s.group_listener(&group1);

    group1.move_editor(&input2_group1, 0);
    assert_same(&editor_of(&group1_events.sticky()[0]), &input2_group1);
    assert_eq!(group1_events.sticky()[0].editor_index, Some(0));

    let group2 = s.group();

    let input1_group2 = s.input();
    let input2_group2 = s.input();
    let input3_group2 = s.input();

    // Open all the editors
    open!(group2, input1_group2, pinned: true, active: true, index: 0, sticky: true);
    open!(group2, input2_group2, pinned: true, active: false, index: 1);
    open!(group2, input3_group2, pinned: true, active: false, index: 2);

    let group2_events = s.group_listener(&group2);

    group2.move_editor(&input1_group2, 1);
    assert_same(&editor_of(&group2_events.unsticky()[0]), &input1_group2);
    assert_eq!(group2_events.unsticky()[0].editor_index, Some(1));
}

fn assert_selection(group: &EditorGroupModel, active_editor: &je_workbench::common::editor::EditorInputRef, selected_editors: &[je_workbench::common::editor::EditorInputRef]) {
    assert_is(group.active_editor(), active_editor);
    assert_eq!(group.selected_editors().len(), selected_editors.len());
    for (i, selected_editor) in selected_editors.iter().enumerate() {
        assert_same(&group.selected_editors()[i], selected_editor);
    }
}

#[test]
fn editor_selection_selected_editors() {
    let s = Suite::new();
    let group = s.group();

    let active_editor = group.active_editor();
    let selected_editors = group.selected_editors();
    assert_none(active_editor);
    assert_eq!(selected_editors.len(), 0);

    // active editor: input1, selection: [input1]
    let input1 = s.input();
    open!(group, input1, pinned: true, active: true, index: 0);
    assert_selection(&group, &input1, &[input1.clone()]);

    // active editor: input3, selection: [input3]
    let input2 = s.input();
    let input3 = s.input();
    open!(group, input2, pinned: true, active: true, index: 1);
    open!(group, input3, pinned: true, active: true, index: 2);
    assert_selection(&group, &input3, &[input3.clone()]);

    // active editor: input2, selection: [input1, input2] (in sequential order)
    group.set_selection(&input2, &[input1.clone()]);
    assert_selection(&group, &input2, &[input1.clone(), input2.clone()]);
}

#[test]
fn editor_selection_open_editor_with_inactive_selection() {
    let s = Suite::new();
    let group = s.group();

    // active editor: input3, selection: [input3]
    let input1 = s.input();
    let input2 = s.input();
    let input3 = s.input();
    open!(group, input1, pinned: true, active: true, index: 0);
    open!(group, input2, pinned: true, active: true, index: 1);
    open!(group, input3, pinned: true, active: true, index: 2);

    // active editor: input2, selection: [input1, input2, input3] (in sequential order)
    open!(group, input2, active: true, inactive_selection: vec![input3.clone(), input1.clone()]);
    assert_selection(&group, &input2, &[input1.clone(), input2.clone(), input3.clone()]);

    // active editor: input1, selection: [input1, input3] (in sequential order)
    // test duplicate entries
    open!(group, input1, active: true, inactive_selection: vec![input3.clone(), input1.clone(), input3.clone()]);
    assert_selection(&group, &input1, &[input1.clone(), input3.clone()]);

    // active editor: input1, selection: [input1, input2] (in sequential order)
    // open new Editor as inactive with selection
    let input4 = s.input();
    open!(group, input4, pinned: true, active: false, inactive_selection: vec![input2.clone()], index: 3);
    assert_selection(&group, &input1, &[input1.clone(), input2.clone()]);

    // active editor: input5, selection: [input4, input5] (in sequential order)
    // open new Editor as active with selection
    let input5 = s.input();
    open!(group, input5, pinned: true, active: true, inactive_selection: vec![input4.clone()], index: 4);
    assert_selection(&group, &input5, &[input4.clone(), input5.clone()]);
}

#[test]
fn editor_selection_close_editor_keeps_selection() {
    let s = Suite::new();
    let group = s.group();

    // active editor: input3, selection: [input3]
    let input1 = s.input();
    let input2 = s.input();
    let input3 = s.input();
    open!(group, input1, pinned: true, active: true, index: 0);
    open!(group, input2, pinned: true, active: true, index: 1);
    open!(group, input3, pinned: true, active: true, index: 2);

    group.set_selection(&input2, &[input3.clone(), input1.clone()]);
    close(&group, &input3);
    assert_selection(&group, &input2, &[input1.clone(), input2.clone()]);
}

#[test]
fn editor_selection_set_selection() {
    let s = Suite::new();
    let group = s.group();

    // active editor: input3, selection: [input3]
    let input1 = s.input();
    let input2 = s.input();
    let input3 = s.input();
    open!(group, input1, pinned: true, active: true, index: 0);
    open!(group, input2, pinned: true, active: true, index: 1);
    open!(group, input3, pinned: true, active: true, index: 2);

    // active editor: input2, selection: [input1, input2, input3] (in sequential order)
    group.set_selection(&input2, &[input3.clone(), input1.clone()]);
    assert_selection(&group, &input2, &[input1.clone(), input2.clone(), input3.clone()]);

    // active editor: input3, selection: [input3]
    group.set_selection(&input3, &[]);
    assert_selection(&group, &input3, &[input3.clone()]);

    // active editor: input2, selection: [input1, input2]
    // test duplicate entries
    group.set_selection(&input2, &[input1.clone(), input2.clone(), input1.clone()]);
    assert_selection(&group, &input2, &[input1.clone(), input2.clone()]);
}

#[test]
fn editor_selection_is_selected() {
    let s = Suite::new();
    let group = s.group();

    // active editor: input3, selection: [input3]
    let input1 = s.input();
    let input2 = s.input();
    let input3 = s.input();
    open!(group, input1, pinned: true, active: true, index: 0);
    open!(group, input2, pinned: true, active: true, index: 1);
    open!(group, input3, pinned: true, active: true, index: 2);

    // active editor: input2, selection: [input1, input2, input3] (in sequential order)
    group.set_selection(&input2, &[input3.clone(), input1.clone()]);

    assert!(group.is_selected(&ed(&input1)));
    assert!(group.is_selected(&ed(&input2)));
    assert!(group.is_selected(&ed(&input3)));

    // active editor: input3, selection: [input3]
    group.set_selection(&input3, &[]);

    assert!(!group.is_selected(&ed(&input1)));
    assert!(!group.is_selected(&ed(&input2)));
    assert!(group.is_selected(&ed(&input3)));

    // use index
    assert!(!group.is_selected(&at(0)));
    assert!(!group.is_selected(&at(1)));
    assert!(group.is_selected(&at(2)));
}

#[test]
fn editor_selection_select_invalid_editor() {
    let s = Suite::new();
    let group = s.group();

    let input1 = s.input();
    let input2 = s.input();
    open!(group, input1, pinned: true, active: true, index: 0);

    group.set_selection(&input2, &[input1.clone()]);

    assert_is(group.active_editor(), &input1);
    assert_eq!(group.selected_editors().len(), 1);
    assert!(group.is_selected(&ed(&input1)));
    assert!(group.is_selected(&ed(&input1)));
    assert!(!group.is_selected(&ed(&input2)));

    group.set_selection(&input1, &[input2.clone()]);

    assert_is(group.active_editor(), &input1);
    assert_eq!(group.selected_editors().len(), 1);
    assert!(group.is_selected(&ed(&input1)));
    assert!(group.is_selected(&ed(&input1)));
    assert!(!group.is_selected(&ed(&input2)));
}

#[test]
fn editor_transient_basics() {
    let s = Suite::new();
    let group = s.group();
    let events = s.group_listener(&group);

    let input1 = s.input();
    let input2 = s.input();
    open!(group, input1, pinned: true, active: true);

    assert!(!group.is_transient(&ed(&input1)));
    assert_eq!(events.transient().len(), 0);

    open!(group, input2, pinned: true, active: true, transient: true);
    assert_same(&editor_of(&events.transient()[0]), &input2);

    assert!(group.is_transient(&ed(&input2)));

    group.set_transient(&input1, true);
    assert!(group.is_transient(&ed(&input1)));
    assert_same(&editor_of(&events.transient()[1]), &input1);

    group.set_transient(&input2, false);
    assert!(!group.is_transient(&ed(&input2)));
    assert_same(&editor_of(&events.transient()[2]), &input2);
}
