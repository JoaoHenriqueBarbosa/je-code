//! Porte de `src/vs/workbench/services/editor/test/browser/editorsObserver.test.ts` (suíte `EditorsObserver`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`setup` e `teardown`.** O `setup` registra o painel `MyTestEditorForEditorsObserver` com o serializador de
//!   `TEST_SERIALIZABLE_EDITOR_INPUT_ID` e o painel lado a lado ([`setup`]); o `teardown` (`disposables.clear()`) e o
//!   `ensureNoDisposablesAreLeakedInTestSuite()` são o `LeakGuard`, criado no início de cada corpo e descartado no fim.
//!   Os casos não usam timers falsos: rodam em `run_with_real_timers`.
//! - **`createPart`.** O `workbenchInstantiationService(undefined, disposables)` com o `createEditorPart` e o
//!   `instantiationService.stub(IEditorGroupsService, part)` é o [`EditorServiceAccessor`] (as divergências dele estão em
//!   `support/editor_service_support.rs`: o `IEditorGroupsService` é o adaptador sobre a parte principal, e o `EditorService`
//!   real, que o original não cria aqui, existe no acessor e tem o seu próprio observador). O `EditorFactoryRegistry.start`
//!   roda logo depois da criação (antes de qualquer observador existir) e o `part.clearState()` é o
//!   `TestEditorPart::clear_state`, registrado no guarda.
//! - **`new EditorsObserver(container, part, storage)`.** O construtor tem os dois últimos argumentos injetados; o porte chama
//!   `Injectable::create` com o `IEditorGroupsService` da parte e o `TestStorageService` (o `InMemoryStorageService`).
//!   `scoped ? part : undefined` é `Some(EditorGroupsContainer::Part(part))` ou `None`. `storage.testEmitWillSaveState(r)` é
//!   `emit_will_save_state(r)`.
//! - **`scoped` de `testSingleGroupBasics`.** O original chama `createEditorObserver()` sem repassar `scoped`, então os dois
//!   casos usam o observador sem escopo; o porte preserva isso (o parâmetro não é lido).
//! - **Entradas de teste.** O original só descarta algumas das entradas (`disposables.add`); o guarda daqui é mais estrito e
//!   exige todas descartadas, então [`input`] as registra (descartar de novo uma entrada já fechada não tem efeito).
//! - **Asserções de lista.** As sequências de `assert.strictEqual(currentEditorsMRU[i].groupId, ...)` e
//!   `assert.strictEqual(currentEditorsMRU[i].editor, ...)` são [`assert_mru`]; as de `hasEditor` com o mesmo
//!   `{ resource, typeId, editorId }` são [`assert_observed`]. A identidade das entradas é a do ponteiro.
//! - **`copiedGroup.setActive(true)`.** O grupo devolvido por `copy_group` é a `EditorGroupView`, recuperada por `as_any_rc`.
//! - **`part.enforcePartOptions({ limit })`.** Recebe o objeto dinâmico da opção ([`limit_options`]).

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::any::TypeId;
use std::cell::Cell;
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_real_timers, timeout};
use je_base::dynamic::Dynamic;
use je_base::lifecycle::{IDisposable, id_of, to_disposable};
use je_base::uri::URI;
use je_instantiation::Injectable;
use je_platform::opener::editor_types::{EditorActivation, EditorOptions};
use je_platform::storage::{StorageService, WillSaveStateReason};
use je_workbench::browser::parts::editor::editor_group_view::EditorGroupView;
use je_workbench::browser::parts::editor::editors_observer::EditorsObserver;
use je_workbench::common::editor::editor_factory_registry::editor_factory_registry;
use je_workbench::common::editor::editor_input::EditorInput;
use je_workbench::common::editor::side_by_side_editor_input::SideBySideEditorInput;
use je_workbench::common::editor::{EditorIdentifier, EditorInputCapabilities, EditorInputOrUntyped, EditorInputRef, GroupIdentifier};
use je_workbench::services::editor::editor_groups_container::EditorGroupsContainer;
use je_workbench::services::editor::editor_groups_service::{GroupDirection, GroupRef, IEditorGroup, IEditorPart};
use je_workbench::services::editor::editor_service::{IEditorService, ResourceEditorInputIdentifier};
use je_workbench::test::browser::workbench_test_services::{TestEditorPart, TestFileEditorInput, register_test_editor};
use je_workbench::test::common::workbench_test_services::TestStorageService;
use support::editor_service_support::{EditorServiceAccessor, register_test_side_by_side_editor};

const TEST_EDITOR_ID: &str = "MyTestEditorForEditorsObserver";
const TEST_EDITOR_INPUT_ID: &str = "testEditorInputForEditorsObserver";
const TEST_SERIALIZABLE_EDITOR_INPUT_ID: &str = "testSerializableEditorInputForEditorsObserver";

type Group = Rc<dyn IEditorGroup>;

//#region Infraestrutura da suíte

/// O `setup` da suíte.
fn setup(guard: &LeakGuard) {
    guard.add(register_test_editor(TEST_EDITOR_ID, vec![TypeId::of::<TestFileEditorInput>()], Some(TEST_SERIALIZABLE_EDITOR_INPUT_ID)));
    guard.add(register_test_side_by_side_editor());
}

/// O `[part, instantiationService]` de `createPart()`.
struct Fixture {
    part: Rc<TestEditorPart>,
    accessor: EditorServiceAccessor,
}

impl Fixture {
    /// `part.activeGroup`.
    fn active_group(&self) -> Group {
        self.part.active_group()
    }
}

/// `createPart()`.
async fn create_part(guard: &LeakGuard) -> Fixture {
    let accessor = EditorServiceAccessor::with_default_backup_service(&guard.store());
    accessor.instantiation_service.invoke_function(|services| editor_factory_registry().start(services));
    accessor.part.when_ready().await.unwrap();

    let part = TestEditorPart::new(accessor.part.clone());
    let cleared = part.clone();
    guard.add(to_disposable(move || cleared.clear_state()));

    Fixture { part, accessor }
}

/// `disposables.add(new EditorsObserver(scoped ? part : undefined, part, storage))`.
fn new_observer(guard: &LeakGuard, fixture: &Fixture, scoped: bool, storage: Rc<dyn StorageService>) -> Rc<EditorsObserver> {
    let part: Rc<dyn IEditorPart> = fixture.accessor.part.clone();
    let container = scoped.then(|| EditorGroupsContainer::Part(part));

    guard.add_rc(EditorsObserver::create(container, (fixture.accessor.editor_groups_service.clone(), storage)))
}

/// `createEditorObserver(scoped)`: o `TestStorageService` do observador é descartado com o guarda.
async fn create_editor_observer(guard: &LeakGuard, scoped: bool) -> (Fixture, Rc<EditorsObserver>) {
    let fixture = create_part(guard).await;
    let storage = guard.add_rc(TestStorageService::new());
    let observer = new_observer(guard, &fixture, scoped, Rc::new(storage));

    (fixture, observer)
}

/// `disposables.add(new TestFileEditorInput(URI.parse(resource), typeId))`.
fn input_of(guard: &LeakGuard, resource: &str, type_id: &str) -> Rc<TestFileEditorInput> {
    guard.add_rc(TestFileEditorInput::new(URI::parse(resource, false), type_id))
}

/// A entrada serializável (`TEST_SERIALIZABLE_EDITOR_INPUT_ID`).
fn input(guard: &LeakGuard, resource: &str) -> Rc<TestFileEditorInput> {
    input_of(guard, resource, TEST_SERIALIZABLE_EDITOR_INPUT_ID)
}

/// A entrada que não pode ser serializada (`TEST_EDITOR_INPUT_ID`).
fn plain_input(guard: &LeakGuard, resource: &str) -> Rc<TestFileEditorInput> {
    input_of(guard, resource, TEST_EDITOR_INPUT_ID)
}

/// A entrada de teste como `EditorInput`.
fn editor(input: &Rc<TestFileEditorInput>) -> EditorInputRef {
    input.clone()
}

/// `{ pinned: true }`.
fn pinned() -> Option<EditorOptions> {
    Some(EditorOptions { pinned: Some(true), ..EditorOptions::default() })
}

/// `{ resource: input.resource, typeId: input.typeId, editorId: input.editorId }`.
fn identifier(input: &Rc<TestFileEditorInput>) -> ResourceEditorInputIdentifier {
    ResourceEditorInputIdentifier { resource: input.resource.clone(), type_id: input.editor_type_id(), editor_id: input.editor_id() }
}

/// `observer.hasEditor(identifier(input))`.
fn has_editor(observer: &EditorsObserver, input: &Rc<TestFileEditorInput>) -> bool {
    observer.has_editor(&identifier(input))
}

/// As asserções `assert.strictEqual(observer.hasEditor(identifier(input)), expected)` em sequência.
fn assert_observed(observer: &EditorsObserver, expected: &[(&Rc<TestFileEditorInput>, bool)]) {
    for (input, observed) in expected {
        assert_eq!(has_editor(observer, input), *observed);
    }
}

/// As asserções `assert.strictEqual(observer.hasEditors(resource), expected)` em sequência.
fn assert_has_editors(observer: &EditorsObserver, expected: &[(&Rc<TestFileEditorInput>, bool)]) {
    for (input, has) in expected {
        assert_eq!(observer.has_editors(&input.resource), *has);
    }
}

/// `currentEditorsMRU.length` e, para cada posição, `currentEditorsMRU[i].groupId` e `currentEditorsMRU[i].editor`.
fn assert_mru(current_editors_mru: &[EditorIdentifier], expected: &[(GroupIdentifier, &Rc<TestFileEditorInput>)]) {
    assert_eq!(current_editors_mru.len(), expected.len());
    for (current, (group_id, input)) in current_editors_mru.iter().zip(expected) {
        assert_eq!(current.group_id, *group_id);
        assert!(id_of(&current.editor) == id_of(*input));
    }
}

/// `{ limit: { enabled: true, value, perEditorGroup? } }`.
fn limit_options(value: f64, per_editor_group: bool) -> Dynamic {
    let mut limit = vec![("enabled", Dynamic::Bool(true)), ("value", Dynamic::Number(value))];
    if per_editor_group {
        limit.push(("perEditorGroup", Dynamic::Bool(true)));
    }

    Dynamic::object([("limit", Dynamic::object(limit))])
}

//#endregion

#[test]
fn basics_single_group() {
    run_with_real_timers(async {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        setup(&guard);
        test_single_group_basics(&guard, false).await;
        drop(guard);
    });
}

#[test]
fn basics_single_group_scoped() {
    run_with_real_timers(async {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        setup(&guard);
        test_single_group_basics(&guard, true).await;
        drop(guard);
    });
}

/// `testSingleGroupBasics(scoped)`: o `scoped` não chega ao `createEditorObserver()` (ver o cabeçalho).
async fn test_single_group_basics(guard: &LeakGuard, _scoped: bool) {
    let (fixture, observer) = create_editor_observer(guard, false).await;
    let part = &fixture.part;

    let on_did_most_recently_active_editors_change_called = Rc::new(Cell::new(false));
    let called = on_did_most_recently_active_editors_change_called.clone();
    guard.add(observer.on_did_most_recently_active_editors_change().listen(move |_| called.set(true)));

    let mut current_editors_mru = observer.editors();
    assert_eq!(current_editors_mru.len(), 0);
    assert!(!on_did_most_recently_active_editors_change_called.get());

    let input1 = input(guard, "foo://bar1");

    part.active_group().open_editor(&editor(&input1), pinned()).await.unwrap();

    current_editors_mru = observer.editors();
    let group_id = part.active_group().id();
    assert_mru(&current_editors_mru, &[(group_id, &input1)]);
    assert!(on_did_most_recently_active_editors_change_called.get());
    assert!(has_editor(&observer, &input1));
    assert!(observer.has_editors(&input1.resource));
    assert!(!observer.has_editor(&ResourceEditorInputIdentifier { resource: input1.resource.clone(), type_id: "unknownTypeId".to_string(), editor_id: Some("unknownTypeId".to_string()) }));

    let input2 = input(guard, "foo://bar2");
    let input3 = input(guard, "foo://bar3");

    assert!(!observer.has_editors(&input2.resource));
    assert!(!has_editor(&observer, &input2));

    part.active_group().open_editor(&editor(&input2), pinned()).await.unwrap();
    part.active_group().open_editor(&editor(&input3), pinned()).await.unwrap();

    current_editors_mru = observer.editors();
    assert_mru(&current_editors_mru, &[(group_id, &input3), (group_id, &input2), (group_id, &input1)]);
    assert_observed(&observer, &[(&input2, true), (&input3, true)]);

    part.active_group().open_editor(&editor(&input2), pinned()).await.unwrap();

    current_editors_mru = observer.editors();
    assert_mru(&current_editors_mru, &[(group_id, &input2), (group_id, &input3), (group_id, &input1)]);
    assert_observed(&observer, &[(&input1, true), (&input2, true), (&input3, true)]);

    on_did_most_recently_active_editors_change_called.set(false);
    part.active_group().close_editor(Some(&editor(&input1))).await.unwrap();

    current_editors_mru = observer.editors();
    assert_mru(&current_editors_mru, &[(group_id, &input2), (group_id, &input3)]);
    assert!(on_did_most_recently_active_editors_change_called.get());
    assert_observed(&observer, &[(&input1, false), (&input2, true), (&input3, true)]);

    part.active_group().close_all_editors(None).await.unwrap();
    current_editors_mru = observer.editors();
    assert_eq!(current_editors_mru.len(), 0);
    assert_observed(&observer, &[(&input1, false), (&input2, false), (&input3, false)]);
}

#[test]
fn basics_multi_group() {
    run_with_real_timers(async {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        setup(&guard);
        let (fixture, observer) = create_editor_observer(&guard, false).await;
        let part = &fixture.part;

        let root_group = fixture.active_group();

        let mut current_editors_mru = observer.editors();
        assert_eq!(current_editors_mru.len(), 0);

        let side_group = part.add_group(Some(GroupRef::Group(root_group.clone())), GroupDirection::Right);

        let input1 = input(&guard, "foo://bar1");

        let activate = || Some(EditorOptions { pinned: Some(true), activation: Some(EditorActivation::Activate), ..EditorOptions::default() });
        root_group.open_editor(&editor(&input1), activate()).await.unwrap();
        side_group.open_editor(&editor(&input1), activate()).await.unwrap();

        current_editors_mru = observer.editors();
        assert_mru(&current_editors_mru, &[(side_group.id(), &input1), (root_group.id(), &input1)]);
        assert_has_editors(&observer, &[(&input1, true)]);
        assert_observed(&observer, &[(&input1, true)]);

        root_group.open_editor(&editor(&input1), activate()).await.unwrap();

        current_editors_mru = observer.editors();
        assert_mru(&current_editors_mru, &[(root_group.id(), &input1), (side_group.id(), &input1)]);
        assert_has_editors(&observer, &[(&input1, true)]);
        assert_observed(&observer, &[(&input1, true)]);

        // Opening an editor inactive should not change
        // the most recent editor, but rather put it behind
        let input2 = input(&guard, "foo://bar2");

        root_group.open_editor(&editor(&input2), Some(EditorOptions { inactive: Some(true), ..EditorOptions::default() })).await.unwrap();

        current_editors_mru = observer.editors();
        assert_mru(&current_editors_mru, &[(root_group.id(), &input1), (root_group.id(), &input2), (side_group.id(), &input1)]);
        assert_has_editors(&observer, &[(&input1, true), (&input2, true)]);
        assert_observed(&observer, &[(&input1, true), (&input2, true)]);

        root_group.close_all_editors(None).await.unwrap();

        current_editors_mru = observer.editors();
        assert_mru(&current_editors_mru, &[(side_group.id(), &input1)]);
        assert_has_editors(&observer, &[(&input1, true), (&input2, false)]);
        assert_observed(&observer, &[(&input1, true), (&input2, false)]);

        side_group.close_all_editors(None).await.unwrap();

        current_editors_mru = observer.editors();
        assert_eq!(current_editors_mru.len(), 0);
        assert_has_editors(&observer, &[(&input1, false), (&input2, false)]);
        assert_observed(&observer, &[(&input1, false), (&input2, false)]);

        part.remove_group(GroupRef::Group(side_group));
        drop(guard);
    });
}

#[test]
fn has_editor_has_editors_same_resource_different_type_id() {
    run_with_real_timers(async {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        setup(&guard);
        let (fixture, observer) = create_editor_observer(&guard, false).await;
        let part = &fixture.part;

        let input1 = input(&guard, "foo://bar1");
        let input2 = guard.add_rc(TestFileEditorInput::new(input1.resource.clone(), "otherTypeId"));

        assert_has_editors(&observer, &[(&input1, false)]);
        assert_observed(&observer, &[(&input1, false), (&input2, false)]);

        part.active_group().open_editor(&editor(&input1), pinned()).await.unwrap();

        assert_has_editors(&observer, &[(&input1, true)]);
        assert_observed(&observer, &[(&input1, true), (&input2, false)]);

        part.active_group().open_editor(&editor(&input2), pinned()).await.unwrap();

        assert_has_editors(&observer, &[(&input1, true)]);
        assert_observed(&observer, &[(&input1, true), (&input2, true)]);

        part.active_group().close_editor(Some(&editor(&input2))).await.unwrap();

        assert_has_editors(&observer, &[(&input1, true)]);
        assert_observed(&observer, &[(&input1, true), (&input2, false)]);

        part.active_group().close_editor(Some(&editor(&input1))).await.unwrap();

        assert_has_editors(&observer, &[(&input1, false)]);
        assert_observed(&observer, &[(&input1, false), (&input2, false)]);
        drop(guard);
    });
}

#[test]
fn has_editor_has_editors_side_by_side_editor_support() {
    run_with_real_timers(async {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        setup(&guard);
        let (fixture, observer) = create_editor_observer(&guard, false).await;
        let part = &fixture.part;

        let primary = input(&guard, "foo://bar1");
        let secondary = input_of(&guard, "foo://bar2", "otherTypeId");

        let editor_service: Rc<dyn IEditorService> = fixture.accessor.editor_service.clone();
        let side_by_side: EditorInputRef = SideBySideEditorInput::new(Some("name"), None, editor(&secondary), editor(&primary), editor_service);
        let side_by_side_tracked = side_by_side.clone();
        guard.add(to_disposable(move || side_by_side_tracked.dispose()));

        assert_has_editors(&observer, &[(&primary, false)]);
        assert_observed(&observer, &[(&primary, false), (&secondary, false)]);

        part.active_group().open_editor(&side_by_side, pinned()).await.unwrap();

        assert_has_editors(&observer, &[(&primary, true)]);
        assert_observed(&observer, &[(&primary, true), (&secondary, false)]);

        part.active_group().open_editor(&editor(&primary), pinned()).await.unwrap();

        assert_has_editors(&observer, &[(&primary, true)]);
        assert_observed(&observer, &[(&primary, true), (&secondary, false)]);

        part.active_group().close_editor(Some(&side_by_side)).await.unwrap();

        assert_has_editors(&observer, &[(&primary, true)]);
        assert_observed(&observer, &[(&primary, true), (&secondary, false)]);

        part.active_group().close_editor(Some(&editor(&primary))).await.unwrap();

        assert_has_editors(&observer, &[(&primary, false)]);
        assert_observed(&observer, &[(&primary, false), (&secondary, false)]);
        drop(guard);
    });
}

#[test]
fn copy_group() {
    run_with_real_timers(async {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        setup(&guard);
        let (fixture, observer) = create_editor_observer(&guard, false).await;
        let part = &fixture.part;

        let input1 = input(&guard, "foo://bar1");
        let input2 = input(&guard, "foo://bar2");
        let input3 = input(&guard, "foo://bar3");

        let root_group = fixture.active_group();

        root_group.open_editor(&editor(&input1), pinned()).await.unwrap();
        root_group.open_editor(&editor(&input2), pinned()).await.unwrap();
        root_group.open_editor(&editor(&input3), pinned()).await.unwrap();

        let mut current_editors_mru = observer.editors();
        assert_mru(&current_editors_mru, &[(root_group.id(), &input3), (root_group.id(), &input2), (root_group.id(), &input1)]);
        assert_observed(&observer, &[(&input1, true), (&input2, true), (&input3, true)]);

        let copied_group = part.copy_group(GroupRef::Group(root_group.clone()), GroupRef::Group(root_group.clone()), GroupDirection::Right);
        let copied_group_view = copied_group.clone().as_any_rc().and_then(|any| any.downcast::<EditorGroupView>().ok()).expect("o grupo copiado é uma EditorGroupView");
        copied_group_view.set_active(true);
        copied_group.focus();

        current_editors_mru = observer.editors();
        assert_mru(
            &current_editors_mru,
            &[
                (copied_group.id(), &input3),
                (root_group.id(), &input3),
                (copied_group.id(), &input2),
                (copied_group.id(), &input1),
                (root_group.id(), &input2),
                (root_group.id(), &input1),
            ],
        );
        assert_observed(&observer, &[(&input1, true), (&input2, true), (&input3, true)]);

        root_group.close_all_editors(None).await.unwrap();

        assert_observed(&observer, &[(&input1, true), (&input2, true), (&input3, true)]);

        copied_group.close_all_editors(None).await.unwrap();

        assert_observed(&observer, &[(&input1, false), (&input2, false), (&input3, false)]);
        drop(guard);
    });
}

/// O trecho comum de `initial editors are part of observer ...`: o observador sobre o armazenamento dado e `await part.whenReady`.
async fn create_observer_on(guard: &LeakGuard, fixture: &Fixture, storage: &Rc<dyn StorageService>) -> Rc<EditorsObserver> {
    let observer = new_observer(guard, fixture, false, storage.clone());
    fixture.accessor.part.when_ready().await.unwrap();

    observer
}

#[test]
fn initial_editors_are_part_of_observer_and_state_is_persisted_and_restored_single_group() {
    run_with_real_timers(async {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        setup(&guard);
        let fixture = create_part(&guard).await;

        let root_group = fixture.active_group();

        let input1 = input(&guard, "foo://bar1");
        let input2 = input(&guard, "foo://bar2");
        let input3 = input(&guard, "foo://bar3");

        root_group.open_editor(&editor(&input1), pinned()).await.unwrap();
        root_group.open_editor(&editor(&input2), pinned()).await.unwrap();
        root_group.open_editor(&editor(&input3), pinned()).await.unwrap();

        let storage = guard.add_rc(TestStorageService::new());
        let storage_service: Rc<dyn StorageService> = Rc::new(storage.clone());
        let observer = create_observer_on(&guard, &fixture, &storage_service).await;

        let mut current_editors_mru = observer.editors();
        assert_mru(&current_editors_mru, &[(root_group.id(), &input3), (root_group.id(), &input2), (root_group.id(), &input1)]);
        assert_observed(&observer, &[(&input1, true), (&input2, true), (&input3, true)]);

        storage.emit_will_save_state(WillSaveStateReason::Shutdown);

        let restored_observer = create_observer_on(&guard, &fixture, &storage_service).await;

        current_editors_mru = restored_observer.editors();
        assert_mru(&current_editors_mru, &[(root_group.id(), &input3), (root_group.id(), &input2), (root_group.id(), &input1)]);
        assert_observed(&observer, &[(&input1, true), (&input2, true), (&input3, true)]);
        drop(guard);
    });
}

#[test]
fn initial_editors_are_part_of_observer_multi_group() {
    run_with_real_timers(async {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        setup(&guard);
        let fixture = create_part(&guard).await;

        let root_group = fixture.active_group();

        let input1 = input(&guard, "foo://bar1");
        let input2 = input(&guard, "foo://bar2");
        let input3 = input(&guard, "foo://bar3");

        root_group.open_editor(&editor(&input1), pinned()).await.unwrap();
        root_group.open_editor(&editor(&input2), pinned()).await.unwrap();

        let side_group = fixture.part.add_group(Some(GroupRef::Group(root_group.clone())), GroupDirection::Right);
        side_group.open_editor(&editor(&input3), pinned()).await.unwrap();

        let storage = guard.add_rc(TestStorageService::new());
        let storage_service: Rc<dyn StorageService> = Rc::new(storage.clone());
        let observer = create_observer_on(&guard, &fixture, &storage_service).await;

        let mut current_editors_mru = observer.editors();
        assert_mru(&current_editors_mru, &[(side_group.id(), &input3), (root_group.id(), &input2), (root_group.id(), &input1)]);
        assert_observed(&observer, &[(&input1, true), (&input2, true), (&input3, true)]);

        storage.emit_will_save_state(WillSaveStateReason::Shutdown);

        let restored_observer = create_observer_on(&guard, &fixture, &storage_service).await;

        current_editors_mru = restored_observer.editors();
        assert_mru(&current_editors_mru, &[(side_group.id(), &input3), (root_group.id(), &input2), (root_group.id(), &input1)]);
        assert_observed(&restored_observer, &[(&input1, true), (&input2, true), (&input3, true)]);
        drop(guard);
    });
}

#[test]
fn observer_does_not_restore_editors_that_cannot_be_serialized() {
    run_with_real_timers(async {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        setup(&guard);
        let fixture = create_part(&guard).await;

        let root_group = fixture.active_group();

        let input1 = plain_input(&guard, "foo://bar1");

        root_group.open_editor(&editor(&input1), pinned()).await.unwrap();

        let storage = guard.add_rc(TestStorageService::new());
        let storage_service: Rc<dyn StorageService> = Rc::new(storage.clone());
        let observer = create_observer_on(&guard, &fixture, &storage_service).await;

        let mut current_editors_mru = observer.editors();
        assert_mru(&current_editors_mru, &[(root_group.id(), &input1)]);
        assert_observed(&observer, &[(&input1, true)]);

        storage.emit_will_save_state(WillSaveStateReason::Shutdown);

        let restored_observer = create_observer_on(&guard, &fixture, &storage_service).await;

        current_editors_mru = restored_observer.editors();
        assert_eq!(current_editors_mru.len(), 0);
        assert_observed(&restored_observer, &[(&input1, false)]);
        drop(guard);
    });
}

/// O início comum dos casos de limite: o limite imposto à parte e o observador sobre um armazenamento novo.
async fn create_part_with_limit(guard: &LeakGuard, limit: Dynamic) -> (Fixture, Rc<EditorsObserver>) {
    let fixture = create_part(guard).await;
    guard.add(fixture.part.enforce_part_options(limit));

    let storage = guard.add_rc(TestStorageService::new());
    let observer = new_observer(guard, &fixture, false, Rc::new(storage));

    (fixture, observer)
}

#[test]
fn observer_closes_editors_when_limit_reached_across_all_groups() {
    run_with_real_timers(async {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        setup(&guard);
        let (fixture, observer) = create_part_with_limit(&guard, limit_options(3.0, false)).await;
        let part = &fixture.part;

        let root_group = fixture.active_group();
        let side_group = part.add_group(Some(GroupRef::Group(root_group.clone())), GroupDirection::Right);

        let input1 = plain_input(&guard, "foo://bar1");
        let input2 = plain_input(&guard, "foo://bar2");
        let input3 = plain_input(&guard, "foo://bar3");
        let input4 = plain_input(&guard, "foo://bar4");

        root_group.open_editor(&editor(&input1), pinned()).await.unwrap();
        root_group.open_editor(&editor(&input2), pinned()).await.unwrap();
        root_group.open_editor(&editor(&input3), pinned()).await.unwrap();
        root_group.open_editor(&editor(&input4), pinned()).await.unwrap();

        let contains = |group: &Group, input: &Rc<TestFileEditorInput>| group.contains(&EditorInputOrUntyped::Typed(editor(input)), None);

        assert_eq!(root_group.count(), 3);
        assert!(!contains(&root_group, &input1));
        assert!(contains(&root_group, &input2));
        assert!(contains(&root_group, &input3));
        assert!(contains(&root_group, &input4));
        assert_observed(&observer, &[(&input1, false), (&input2, true), (&input3, true), (&input4, true)]);

        input2.set_dirty();
        guard.add(part.enforce_part_options(limit_options(1.0, false)));

        timeout(0).await.unwrap();

        assert_eq!(root_group.count(), 2);
        assert!(!contains(&root_group, &input1));
        assert!(contains(&root_group, &input2)); // dirty
        assert!(!contains(&root_group, &input3));
        assert!(contains(&root_group, &input4));
        assert_observed(&observer, &[(&input1, false), (&input2, true), (&input3, false), (&input4, true)]);

        let input5 = plain_input(&guard, "foo://bar5");
        side_group.open_editor(&editor(&input5), pinned()).await.unwrap();

        assert_eq!(root_group.count(), 1);
        assert!(!contains(&root_group, &input1));
        assert!(contains(&root_group, &input2)); // dirty
        assert!(!contains(&root_group, &input3));
        assert!(!contains(&root_group, &input4));
        assert!(contains(&side_group, &input5));
        assert_observed(&observer, &[(&input1, false), (&input2, true), (&input3, false), (&input4, false), (&input5, true)]);
        drop(guard);
    });
}

#[test]
fn observer_closes_editors_when_limit_reached_in_group() {
    run_with_real_timers(async {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        setup(&guard);
        let (fixture, observer) = create_part_with_limit(&guard, limit_options(3.0, true)).await;
        let part = &fixture.part;

        let root_group = fixture.active_group();
        let side_group = part.add_group(Some(GroupRef::Group(root_group.clone())), GroupDirection::Right);

        let input1 = plain_input(&guard, "foo://bar1");
        let input2 = plain_input(&guard, "foo://bar2");
        let input3 = plain_input(&guard, "foo://bar3");
        let input4 = plain_input(&guard, "foo://bar4");

        root_group.open_editor(&editor(&input1), pinned()).await.unwrap();
        root_group.open_editor(&editor(&input2), pinned()).await.unwrap();
        root_group.open_editor(&editor(&input3), pinned()).await.unwrap();
        root_group.open_editor(&editor(&input4), pinned()).await.unwrap();

        let contains = |group: &Group, input: &Rc<TestFileEditorInput>| group.contains(&EditorInputOrUntyped::Typed(editor(input)), None);

        assert_eq!(root_group.count(), 3); // 1 editor got closed due to our limit!
        assert!(!contains(&root_group, &input1));
        assert!(contains(&root_group, &input2));
        assert!(contains(&root_group, &input3));
        assert!(contains(&root_group, &input4));
        assert_observed(&observer, &[(&input1, false), (&input2, true), (&input3, true), (&input4, true)]);

        side_group.open_editor(&editor(&input1), pinned()).await.unwrap();
        side_group.open_editor(&editor(&input2), pinned()).await.unwrap();
        side_group.open_editor(&editor(&input3), pinned()).await.unwrap();
        side_group.open_editor(&editor(&input4), pinned()).await.unwrap();

        assert_eq!(side_group.count(), 3);
        assert!(!contains(&side_group, &input1));
        assert!(contains(&side_group, &input2));
        assert!(contains(&side_group, &input3));
        assert!(contains(&side_group, &input4));
        assert_observed(&observer, &[(&input1, false), (&input2, true), (&input3, true), (&input4, true)]);

        guard.add(part.enforce_part_options(limit_options(1.0, true)));

        timeout(10).await.unwrap();

        assert_eq!(root_group.count(), 1);
        assert!(!contains(&root_group, &input1));
        assert!(!contains(&root_group, &input2));
        assert!(!contains(&root_group, &input3));
        assert!(contains(&root_group, &input4));

        assert_eq!(side_group.count(), 1);
        assert!(!contains(&side_group, &input1));
        assert!(!contains(&side_group, &input2));
        assert!(!contains(&side_group, &input3));
        assert!(contains(&side_group, &input4));

        assert_observed(&observer, &[(&input1, false), (&input2, false), (&input3, false), (&input4, true)]);
        drop(guard);
    });
}

#[test]
fn observer_does_not_close_sticky() {
    run_with_real_timers(async {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        setup(&guard);
        let (fixture, observer) = create_part_with_limit(&guard, limit_options(3.0, false)).await;

        let root_group = fixture.active_group();

        let input1 = plain_input(&guard, "foo://bar1");
        let input2 = plain_input(&guard, "foo://bar2");
        let input3 = plain_input(&guard, "foo://bar3");
        let input4 = plain_input(&guard, "foo://bar4");

        root_group.open_editor(&editor(&input1), Some(EditorOptions { pinned: Some(true), sticky: Some(true), ..EditorOptions::default() })).await.unwrap();
        root_group.open_editor(&editor(&input2), pinned()).await.unwrap();
        root_group.open_editor(&editor(&input3), pinned()).await.unwrap();
        root_group.open_editor(&editor(&input4), pinned()).await.unwrap();

        let contains = |input: &Rc<TestFileEditorInput>| root_group.contains(&EditorInputOrUntyped::Typed(editor(input)), None);

        assert_eq!(root_group.count(), 3);
        assert!(contains(&input1));
        assert!(!contains(&input2));
        assert!(contains(&input3));
        assert!(contains(&input4));
        assert_observed(&observer, &[(&input1, true), (&input2, false), (&input3, true), (&input4, true)]);
        drop(guard);
    });
}

#[test]
fn observer_does_not_close_scratchpads() {
    run_with_real_timers(async {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        setup(&guard);
        let (fixture, observer) = create_part_with_limit(&guard, limit_options(3.0, false)).await;

        let root_group = fixture.active_group();

        let input1 = plain_input(&guard, "foo://bar1");
        input1.set_capabilities(EditorInputCapabilities::UNTITLED | EditorInputCapabilities::SCRATCHPAD);
        let input2 = plain_input(&guard, "foo://bar2");
        let input3 = plain_input(&guard, "foo://bar3");
        let input4 = plain_input(&guard, "foo://bar4");

        root_group.open_editor(&editor(&input1), pinned()).await.unwrap();
        root_group.open_editor(&editor(&input2), pinned()).await.unwrap();
        root_group.open_editor(&editor(&input3), pinned()).await.unwrap();
        root_group.open_editor(&editor(&input4), pinned()).await.unwrap();

        let contains = |input: &Rc<TestFileEditorInput>| root_group.contains(&EditorInputOrUntyped::Typed(editor(input)), None);

        assert_eq!(root_group.count(), 3);
        assert!(contains(&input1));
        assert!(!contains(&input2));
        assert!(contains(&input3));
        assert!(contains(&input4));
        assert_observed(&observer, &[(&input1, true), (&input2, false), (&input3, true), (&input4, true)]);
        drop(guard);
    });
}

#[test]
fn observer_does_not_close_editors_excluded_from_the_limit_but_still_honors_the_limit_for_others() {
    run_with_real_timers(async {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        setup(&guard);
        let (fixture, observer) = create_part_with_limit(&guard, limit_options(1.0, true)).await;

        let root_group = fixture.active_group();

        let managed1 = plain_input(&guard, "foo://managed1");
        managed1.set_capabilities(EditorInputCapabilities::EXCLUDE_FROM_EDITOR_LIMIT);
        let managed2 = plain_input(&guard, "foo://managed2");
        managed2.set_capabilities(EditorInputCapabilities::EXCLUDE_FROM_EDITOR_LIMIT);
        let file1 = plain_input(&guard, "foo://file1");
        let file2 = plain_input(&guard, "foo://file2");

        root_group.open_editor(&editor(&managed1), pinned()).await.unwrap();
        root_group.open_editor(&editor(&managed2), pinned()).await.unwrap();
        root_group.open_editor(&editor(&file1), pinned()).await.unwrap();
        root_group.open_editor(&editor(&file2), pinned()).await.unwrap();

        let contains = |input: &Rc<TestFileEditorInput>| root_group.contains(&EditorInputOrUntyped::Typed(editor(input)), None);

        // Excluded editors are never evicted (no open/close loop), while a normal
        // editor still honors the limit of 1 (only the most recent survives).
        assert_eq!(
            (root_group.count(), contains(&managed1), contains(&managed2), contains(&file1), contains(&file2), has_editor(&observer, &file1), has_editor(&observer, &file2)),
            (3, true, true, false, true, false, true)
        );
        drop(guard);
    });
}
