//! Porte de `src/vs/workbench/services/editor/test/browser/editorGroupsService.test.ts` (suíte `EditorGroupsService`), caso a caso e na mesma ordem.
//!
//! Esta é a fatia 2 do `PORT_PLAN.md` (o `EditorPart` lógico com o `EditorGroupView` mínimo): entram os casos de GRUPOS que não
//! abrem editores. Ainda NÃO portados (o placar conta só os `#[test]`):
//! - `sideGroup`, `save & restore state`, `copy/merge groups` e `merge all groups`: abrem editores em grupos (`openEditor`), que
//!   vem com o `EditorGroupView` completo (fatia 3); `save & restore state` ainda precisa do `TestEditorPart` (fatia 4).
//! - `editor basics`, `openEditors / closeEditors` e todos os casos de `closeEditor(s)`, `closeAllEditors`, `moveEditor(s)`,
//!   `copyEditor(s)`, `replaceEditors`, `find editors`, `sticky editors`, `selection`, `moveEditor with context`,
//!   `moveEditor disabled`, `onWillOpenEditor`, `copyEditor with context`, `locked groups`, `closeAllGroups action` e
//!   `transient editors`: dependem das operações de editor do grupo (fatia 3).
//! - `working sets` (3 casos) e `context key provider` (4 casos): dependem de `applyState`, do `EditorParts` e do
//!   `common/contextkeys.ts` (fatias 3 e 4).
//!
//! Notas de tradução:
//! - **`createPart`.** `workbenchInstantiationService` com o `MockScopableContextKeyService` é um `InstantiationService` com o
//!   `TestConfigurationService`, um `InMemoryStorageService` e o `MockContextKeyService::scopable()`; `createEditorParts` (que cria
//!   a parte, dá `create` e `layout(1080, 800, 0, 0)`) é o [`create_part`]. O `EditorParts` e o `TestEditorPart` são da fatia 4.
//! - **Eventos e grupos.** `part.groups[0]` é `part.groups()[0]`; `assert.strictEqual(a, b)` entre grupos é `Rc::ptr_eq`
//!   ([`same`]); `{ group }` onde o original passa um `IEditorGroup | GroupIdentifier` é [`group_ref`].
//! - **`ensureNoDisposablesAreLeakedInTestSuite`** é o `LeakGuard`; o `disposables` do original é o próprio guarda.
//! - **`assert.strictEqual(oldOptions, currentOptions)`** compara por valor (`EditorPartOptions` é `PartialEq`).

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite};
use je_base::dynamic::Dynamic;
use je_base::event::Event;
use je_base::lifecycle::DisposableRef;
use je_instantiation::{InstantiationService, ServiceCollection};
use je_platform::configuration::service::{I_CONFIGURATION_SERVICE, IConfigurationService};
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::keybinding::mock_keybinding_service::MockContextKeyService;
use je_platform::storage::{InMemoryStorageService, StorageService};
use je_ui::Ui;
use je_workbench::browser::parts::editor::editor_part::EditorPart;
use je_workbench::common::editor::GroupModelChangeKind;
use je_workbench::services::editor::editor_groups_service::{
    EditorGroupActivationEvent, EditorGroupLayout, FindGroupScope, GroupActivationReason, GroupDirection, GroupLayoutArgument, GroupLocation, GroupOrientation,
    GroupRef, GroupsArrangement, GroupsOrder, IEditorGroup, IEditorPart, ShowTabs, is_editor_group,
};
use je_workbench::services::layout::layout_service::MAIN_WINDOW;

/// `createPart()`: a parte principal criada (`create`) e com layout de 1080x800.
fn create_part(guard: &LeakGuard) -> Rc<EditorPart> {
    let configuration: Rc<dyn IConfigurationService> = TestConfigurationService::new(None);
    let instantiation_service = InstantiationService::new(ServiceCollection::new().with(I_CONFIGURATION_SERVICE, configuration.clone()));
    let storage = InMemoryStorageService::new();
    let storage_service: Rc<dyn StorageService> = storage.clone();
    let context_key_service = MockContextKeyService::scopable();

    let part = guard.add_rc(EditorPart::new(
        &Ui::new(),
        instantiation_service.clone(),
        configuration,
        storage_service,
        &*context_key_service,
        "workbench.parts.editor",
        "",
        MAIN_WINDOW,
    ));
    guard.add_rc(storage);
    guard.add_rc(instantiation_service);

    part.create(None);
    part.layout(1080.0, 800.0, 0.0, 0.0);

    part
}

fn group_ref(group: &Rc<dyn IEditorGroup>) -> GroupRef {
    GroupRef::Group(group.clone())
}

/// `assert.strictEqual(a, b)` entre grupos: o mesmo objeto.
fn same(a: &Rc<dyn IEditorGroup>, b: &Rc<dyn IEditorGroup>) -> bool {
    Rc::ptr_eq(a, b)
}

/// O contador que um `listener` incrementa, e o `IDisposable` do ouvinte.
fn count<T: 'static>(event: &Event<T>) -> (Rc<Cell<usize>>, DisposableRef) {
    let counter = Rc::new(Cell::new(0));
    let listener = {
        let counter = counter.clone();
        event.listen(move |_| counter.set(counter.get() + 1))
    };

    (counter, listener)
}

/// O contador dos eventos `kind` do modelo de `group`.
fn count_model_changes(group: &Rc<dyn IEditorGroup>, kind: GroupModelChangeKind) -> (Rc<Cell<usize>>, DisposableRef) {
    let counter = Rc::new(Cell::new(0));
    let listener = {
        let counter = counter.clone();
        group.on_did_model_change().listen(move |event| {
            if event.kind == kind {
                counter.set(counter.get() + 1);
            }
        })
    };

    (counter, listener)
}

fn scope_direction(direction: GroupDirection) -> FindGroupScope {
    FindGroupScope { direction: Some(direction), ..FindGroupScope::default() }
}

fn scope_location(location: GroupLocation) -> FindGroupScope {
    FindGroupScope { location: Some(location), ..FindGroupScope::default() }
}

/// `{}`: uma folha do layout.
fn leaf() -> GroupLayoutArgument {
    GroupLayoutArgument::default()
}

/// `{ groups: [...] }`: um ramo do layout.
fn branch(groups: Vec<GroupLayoutArgument>) -> GroupLayoutArgument {
    GroupLayoutArgument { size: None, groups: Some(groups) }
}

#[test]
fn groups_basics() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let part = create_part(&guard);

    let (active_group_model_change_counter, active_group_model_change_listener) = count(&part.on_did_change_active_group());
    let (group_added_counter, group_added_listener) = count(&part.on_did_add_group());
    let (group_removed_counter, group_removed_listener) = count(&part.on_did_remove_group());
    let (group_moved_counter, group_moved_listener) = count(&part.on_did_move_group());

    // always a root group
    let root_group = part.groups()[0].clone();
    assert!(is_editor_group(&*root_group));
    assert_eq!(part.groups().len(), 1);
    assert_eq!(part.count(), 1);
    assert!(same(&root_group, &part.get_group(root_group.id()).unwrap()));
    assert!(same(&part.active_group(), &root_group));
    assert_eq!(root_group.label(), "Group 1");

    let mut mru = part.get_groups(GroupsOrder::MostRecentlyActive);
    assert_eq!(mru.len(), 1);
    assert!(same(&mru[0], &root_group));

    let right_group = part.add_group(Some(group_ref(&root_group)), GroupDirection::Right);
    assert!(same(&right_group, &part.get_group(right_group.id()).unwrap()));
    assert_eq!(group_added_counter.get(), 1);
    assert_eq!(part.groups().len(), 2);
    assert_eq!(part.count(), 2);
    assert!(same(&part.active_group(), &root_group));
    assert_eq!(root_group.label(), "Group 1");
    assert_eq!(right_group.label(), "Group 2");

    mru = part.get_groups(GroupsOrder::MostRecentlyActive);
    assert_eq!(mru.len(), 2);
    assert!(same(&mru[0], &root_group));
    assert!(same(&mru[1], &right_group));

    assert_eq!(active_group_model_change_counter.get(), 0);

    let (root_group_active_change_counter, root_group_model_change_listener) = count_model_changes(&root_group, GroupModelChangeKind::GroupActive);
    let (right_group_active_change_counter, right_group_model_change_listener) = count_model_changes(&right_group, GroupModelChangeKind::GroupActive);

    part.activate_group(group_ref(&right_group));
    assert!(same(&part.active_group(), &right_group));
    assert_eq!(active_group_model_change_counter.get(), 1);
    assert_eq!(root_group_active_change_counter.get(), 1);
    assert_eq!(right_group_active_change_counter.get(), 1);

    root_group_model_change_listener.dispose();
    right_group_model_change_listener.dispose();

    mru = part.get_groups(GroupsOrder::MostRecentlyActive);
    assert_eq!(mru.len(), 2);
    assert!(same(&mru[0], &right_group));
    assert!(same(&mru[1], &root_group));

    let down_group = part.add_group(Some(group_ref(&right_group)), GroupDirection::Down);
    let did_dispose = Rc::new(Cell::new(false));
    guard.add({
        let did_dispose = did_dispose.clone();
        down_group.on_will_dispose().listen(move |_| did_dispose.set(true))
    });
    assert_eq!(group_added_counter.get(), 2);
    assert_eq!(part.groups().len(), 3);
    assert!(same(&part.active_group(), &right_group));
    assert!(down_group.active_editor_pane().is_none());
    assert_eq!(root_group.label(), "Group 1");
    assert_eq!(right_group.label(), "Group 2");
    assert_eq!(down_group.label(), "Group 3");

    mru = part.get_groups(GroupsOrder::MostRecentlyActive);
    assert_eq!(mru.len(), 3);
    assert!(same(&mru[0], &right_group));
    assert!(same(&mru[1], &root_group));
    assert!(same(&mru[2], &down_group));

    let grid_order = part.get_groups(GroupsOrder::GridAppearance);
    assert_eq!(grid_order.len(), 3);
    assert!(same(&grid_order[0], &root_group));
    assert_eq!(grid_order[0].index(), 0);
    assert!(same(&grid_order[1], &right_group));
    assert_eq!(grid_order[1].index(), 1);
    assert!(same(&grid_order[2], &down_group));
    assert_eq!(grid_order[2].index(), 2);

    part.move_group(group_ref(&down_group), group_ref(&right_group), GroupDirection::Down);
    assert_eq!(group_moved_counter.get(), 1);

    part.remove_group(group_ref(&down_group));
    assert!(part.get_group(down_group.id()).is_none());
    assert!(!part.has_group(down_group.id()));
    assert!(did_dispose.get());
    assert_eq!(group_removed_counter.get(), 1);
    assert_eq!(part.groups().len(), 2);
    assert!(same(&part.active_group(), &right_group));
    assert_eq!(root_group.label(), "Group 1");
    assert_eq!(right_group.label(), "Group 2");

    mru = part.get_groups(GroupsOrder::MostRecentlyActive);
    assert_eq!(mru.len(), 2);
    assert!(same(&mru[0], &right_group));
    assert!(same(&mru[1], &root_group));

    let right_group_context_key_service = part.active_group().scoped_context_key_service();
    let root_group_context_key_service = root_group.scoped_context_key_service();

    assert!(!std::ptr::addr_eq(Rc::as_ptr(&right_group_context_key_service), Rc::as_ptr(&root_group_context_key_service)));

    part.remove_group(group_ref(&right_group));
    assert_eq!(group_removed_counter.get(), 2);
    assert_eq!(part.groups().len(), 1);
    assert!(same(&part.active_group(), &root_group));

    mru = part.get_groups(GroupsOrder::MostRecentlyActive);
    assert_eq!(mru.len(), 1);
    assert!(same(&mru[0], &root_group));

    part.remove_group(group_ref(&root_group)); // cannot remove root group
    assert_eq!(part.groups().len(), 1);
    assert_eq!(group_removed_counter.get(), 2);
    assert!(same(&part.active_group(), &root_group));

    part.set_group_orientation(if part.orientation() == GroupOrientation::Horizontal { GroupOrientation::Vertical } else { GroupOrientation::Horizontal });

    active_group_model_change_listener.dispose();
    group_added_listener.dispose();
    group_removed_listener.dispose();
    group_moved_listener.dispose();
}

#[test]
fn groups_index_labels() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let part = create_part(&guard);

    let root_group = part.groups()[0].clone();
    let right_group = part.add_group(Some(group_ref(&root_group)), GroupDirection::Right);
    let down_group = part.add_group(Some(group_ref(&right_group)), GroupDirection::Down);

    let (group_index_changed_counter, group_index_changed_listener) = count(&part.on_did_change_group_index());
    let (index_change_counter, label_change_listener) = count_model_changes(&down_group, GroupModelChangeKind::GroupIndex);

    assert_eq!(root_group.index(), 0);
    assert_eq!(right_group.index(), 1);
    assert_eq!(down_group.index(), 2);
    assert_eq!(root_group.label(), "Group 1");
    assert_eq!(right_group.label(), "Group 2");
    assert_eq!(down_group.label(), "Group 3");

    part.remove_group(group_ref(&right_group));
    assert_eq!(root_group.index(), 0);
    assert_eq!(down_group.index(), 1);
    assert_eq!(root_group.label(), "Group 1");
    assert_eq!(down_group.label(), "Group 2");
    assert_eq!(index_change_counter.get(), 1);
    assert_eq!(group_index_changed_counter.get(), 1);

    part.move_group(group_ref(&down_group), group_ref(&root_group), GroupDirection::Up);
    assert_eq!(down_group.index(), 0);
    assert_eq!(root_group.index(), 1);
    assert_eq!(down_group.label(), "Group 1");
    assert_eq!(root_group.label(), "Group 2");
    assert_eq!(index_change_counter.get(), 2);
    assert_eq!(group_index_changed_counter.get(), 3);

    let new_first_group = part.add_group(Some(group_ref(&down_group)), GroupDirection::Up);
    assert_eq!(new_first_group.index(), 0);
    assert_eq!(down_group.index(), 1);
    assert_eq!(root_group.index(), 2);
    assert_eq!(new_first_group.label(), "Group 1");
    assert_eq!(down_group.label(), "Group 2");
    assert_eq!(root_group.label(), "Group 3");
    assert_eq!(index_change_counter.get(), 3);
    assert_eq!(group_index_changed_counter.get(), 6);

    label_change_listener.dispose();
    group_index_changed_listener.dispose();
}

#[test]
fn groups_label() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let part = create_part(&guard);

    let root_group = part.groups()[0].clone();
    let right_group = part.add_group(Some(group_ref(&root_group)), GroupDirection::Right);

    let (part_label_changed_counter, group_index_changed_listener) = count(&part.on_did_change_group_label());
    let (root_group_label_change_counter, root_group_label_change_listener) = count_model_changes(&root_group, GroupModelChangeKind::GroupLabel);
    let (right_group_label_change_counter, right_group_label_change_listener) = count_model_changes(&right_group, GroupModelChangeKind::GroupLabel);

    assert_eq!(root_group.label(), "Group 1");
    assert_eq!(right_group.label(), "Group 2");

    part.notify_groups_label_change("Window 2");

    assert_eq!(root_group.label(), "Window 2: Group 1");
    assert_eq!(right_group.label(), "Window 2: Group 2");

    assert_eq!(root_group_label_change_counter.get(), 1);
    assert_eq!(right_group_label_change_counter.get(), 1);
    assert_eq!(part_label_changed_counter.get(), 2);

    part.notify_groups_label_change("Window 3");

    assert_eq!(root_group.label(), "Window 3: Group 1");
    assert_eq!(right_group.label(), "Window 3: Group 2");

    assert_eq!(root_group_label_change_counter.get(), 2);
    assert_eq!(right_group_label_change_counter.get(), 2);
    assert_eq!(part_label_changed_counter.get(), 4);

    root_group_label_change_listener.dispose();
    right_group_label_change_listener.dispose();
    group_index_changed_listener.dispose();
}

#[test]
fn when_ready_when_restored() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let part = create_part(&guard);

    assert!(part.when_ready().result().is_some_and(|result| result.is_ok()));
    assert!(part.is_ready());
    assert!(part.when_restored().result().is_some_and(|result| result.is_ok()));
}

#[test]
fn options() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let part = create_part(&guard);

    let old_options = Rc::new(RefCell::new(None));
    let new_options = Rc::new(RefCell::new(None));
    guard.add({
        let (old_options, new_options) = (old_options.clone(), new_options.clone());
        part.on_did_change_editor_part_options().listen(move |event| {
            *old_options.borrow_mut() = Some(event.old_part_options.clone());
            *new_options.borrow_mut() = Some(event.new_part_options.clone());
        })
    });

    let current_options = part.part_options();

    guard.add(part.enforce_part_options(Dynamic::object([("showTabs", Dynamic::string("single"))])));
    assert_eq!(part.part_options().show_tabs, ShowTabs::Single);
    assert_eq!(new_options.borrow().as_ref().unwrap().show_tabs, ShowTabs::Single);
    assert_eq!(old_options.borrow().as_ref().unwrap(), &current_options);

    let enforced = part.enforce_part_options(Dynamic::object([("allowDropIntoGroup", Dynamic::Bool(false))]));
    assert!(!part.part_options().allow_drop_into_group);
    enforced.dispose();
    assert!(part.part_options().allow_drop_into_group);
}

#[test]
fn find_neighbour_group_left_right() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let part = create_part(&guard);
    let root_group = part.active_group();
    let right_group = part.add_group(Some(group_ref(&root_group)), GroupDirection::Right);

    assert!(same(&right_group, &part.find_group_from(&scope_direction(GroupDirection::Right), Some(group_ref(&root_group)), false).unwrap()));
    assert!(same(&root_group, &part.find_group_from(&scope_direction(GroupDirection::Left), Some(group_ref(&right_group)), false).unwrap()));
}

#[test]
fn find_neighbour_group_up_down() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let part = create_part(&guard);
    let root_group = part.active_group();
    let down_group = part.add_group(Some(group_ref(&root_group)), GroupDirection::Down);

    assert!(same(&down_group, &part.find_group_from(&scope_direction(GroupDirection::Down), Some(group_ref(&root_group)), false).unwrap()));
    assert!(same(&root_group, &part.find_group_from(&scope_direction(GroupDirection::Up), Some(group_ref(&down_group)), false).unwrap()));
}

#[test]
fn find_group_by_location_left_right() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let part = create_part(&guard);
    let root_group = part.active_group();
    let right_group = part.add_group(Some(group_ref(&root_group)), GroupDirection::Right);
    let down_group = part.add_group(Some(group_ref(&right_group)), GroupDirection::Down);

    assert!(same(&root_group, &part.find_group(&scope_location(GroupLocation::First)).unwrap()));
    assert!(same(&down_group, &part.find_group(&scope_location(GroupLocation::Last)).unwrap()));

    assert!(same(&right_group, &part.find_group_from(&scope_location(GroupLocation::Next), Some(group_ref(&root_group)), false).unwrap()));
    assert!(same(&root_group, &part.find_group_from(&scope_location(GroupLocation::Previous), Some(group_ref(&right_group)), false).unwrap()));

    assert!(same(&down_group, &part.find_group_from(&scope_location(GroupLocation::Next), Some(group_ref(&right_group)), false).unwrap()));
    assert!(same(&right_group, &part.find_group_from(&scope_location(GroupLocation::Previous), Some(group_ref(&down_group)), false).unwrap()));
}

#[test]
fn apply_layout_2x2() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let part = create_part(&guard);

    part.apply_layout(&EditorGroupLayout { groups: vec![branch(vec![leaf(), leaf()]), branch(vec![leaf(), leaf()])], orientation: GroupOrientation::Horizontal });

    assert_eq!(part.groups().len(), 4);
}

#[test]
fn get_layout() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let part = create_part(&guard);

    // 2x2
    part.apply_layout(&EditorGroupLayout { groups: vec![branch(vec![leaf(), leaf()]), branch(vec![leaf(), leaf()])], orientation: GroupOrientation::Horizontal });
    let mut layout = part.get_layout();

    assert_eq!(layout.orientation, GroupOrientation::Horizontal);
    assert_eq!(layout.groups.len(), 2);
    assert_eq!(layout.groups[0].groups.as_ref().unwrap().len(), 2);
    assert_eq!(layout.groups[1].groups.as_ref().unwrap().len(), 2);

    // 3 columns
    part.apply_layout(&EditorGroupLayout { groups: vec![leaf(), leaf(), leaf()], orientation: GroupOrientation::Vertical });
    layout = part.get_layout();

    assert_eq!(layout.orientation, GroupOrientation::Vertical);
    assert_eq!(layout.groups.len(), 3);
    assert!(layout.groups[0].size.is_some());
    assert!(layout.groups[1].size.is_some());
    assert!(layout.groups[2].size.is_some());
}

#[test]
fn centered_layout() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let part = create_part(&guard);

    part.center_layout(true);

    assert!(part.is_layout_centered());
}

#[test]
fn maximize_editor_group() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let part = create_part(&guard);

    let root_group = part.active_group();
    let editor_part_size = part.get_size(group_ref(&root_group));

    // If there is only one group, it should not be considered maximized
    assert!(!part.has_maximized_group());

    let right_group = part.add_group(Some(group_ref(&root_group)), GroupDirection::Right);
    let right_bottom_group = part.add_group(Some(group_ref(&right_group)), GroupDirection::Down);

    let size_root_group = part.get_size(group_ref(&root_group));
    let size_right_group = part.get_size(group_ref(&right_group));
    let size_right_bottom_group = part.get_size(group_ref(&right_bottom_group));

    let maximized_value = Rc::new(Cell::new(None));
    let maximize_group_event_disposable = {
        let maximized_value = maximized_value.clone();
        part.on_did_change_group_maximized().listen(move |maximized| maximized_value.set(Some(*maximized)))
    };

    assert!(!part.has_maximized_group());

    part.arrange_groups(GroupsArrangement::Maximize, Some(group_ref(&root_group)));

    assert!(part.has_maximized_group());

    // getSize()
    assert_eq!(part.get_size(group_ref(&root_group)), editor_part_size);
    let zero = je_editor::core_2d::dimension::Dimension { width: 0.0, height: 0.0 };
    assert_eq!(part.get_size(group_ref(&right_group)), zero);
    assert_eq!(part.get_size(group_ref(&right_bottom_group)), zero);

    assert_eq!(maximized_value.get(), Some(true));

    part.toggle_maximize_group(None);

    assert!(!part.has_maximized_group());

    // Size is restored
    assert_eq!(part.get_size(group_ref(&root_group)), size_root_group);
    assert_eq!(part.get_size(group_ref(&right_group)), size_right_group);
    assert_eq!(part.get_size(group_ref(&right_bottom_group)), size_right_bottom_group);

    assert_eq!(maximized_value.get(), Some(false));
    maximize_group_event_disposable.dispose();
}

#[test]
fn on_did_activate_group_carries_activation_reason() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let part = create_part(&guard);

    let activation_events: Rc<RefCell<Vec<EditorGroupActivationEvent>>> = Rc::new(RefCell::new(Vec::new()));
    guard.add({
        let activation_events = activation_events.clone();
        part.on_did_activate_group().listen(move |event| activation_events.borrow_mut().push(event.clone()))
    });

    let root_group = part.groups()[0].clone();
    let right_group = part.add_group(Some(group_ref(&root_group)), GroupDirection::Right);

    // Activate a group explicitly - should carry DEFAULT reason
    activation_events.borrow_mut().clear();
    part.activate_group(group_ref(&right_group));
    assert_eq!(activation_events.borrow().len(), 1);
    assert!(same(&activation_events.borrow()[0].group, &right_group));
    assert_eq!(activation_events.borrow()[0].reason, GroupActivationReason::Default);

    // Activate the same group again - should still fire with DEFAULT reason
    activation_events.borrow_mut().clear();
    part.activate_group(group_ref(&right_group));
    assert_eq!(activation_events.borrow().len(), 1);
    assert!(same(&activation_events.borrow()[0].group, &right_group));
    assert_eq!(activation_events.borrow()[0].reason, GroupActivationReason::Default);

    // Activate root group back
    activation_events.borrow_mut().clear();
    part.activate_group(group_ref(&root_group));
    assert_eq!(activation_events.borrow().len(), 1);
    assert!(same(&activation_events.borrow()[0].group, &root_group));
    assert_eq!(activation_events.borrow()[0].reason, GroupActivationReason::Default);
}
