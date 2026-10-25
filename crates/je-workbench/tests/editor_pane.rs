//! Porte de `src/vs/workbench/test/browser/parts/editor/editorPane.test.ts` (suíte `EditorPane`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`ensureNoDisposablesAreLeakedInTestSuite()`.** É o [`LeakGuard`] criado no começo de cada caso; `disposables.add` vira
//!   `guard.add` / `guard.add_rc`. Os dublês sem `Disposable` no original (`TestConfigurationService`, os grupos) são criados
//!   antes do guarda, como o `TestStorageService` e companhia nascem fora do rastreador no mocha.
//! - **Subclasses de `EditorPane`.** `TestEditor`, `OtherTestEditor` e `TrustRequiredTestEditor` não têm herança em Rust: o
//!   painel lógico é criado pelo [`EditorPaneDescriptor`] com o id do `getId()` da subclasse (`testEditor`, `testOtherEditor`,
//!   `trustRequiredTestEditor`), que é o que o caso observa. `layout` e `createEditor` não existem no painel lógico.
//! - **Subclasses de `EditorInput`.** `TestInput`, `OtherTestInput`, `TrustRequiredTestInput` e o `TestEditorInput` do
//!   memento são tipos próprios (o registro compara o tipo concreto da entrada). `TestInput.prefersEditorPane` devolve o
//!   segundo candidato.
//! - **`TestResourceEditorInput extends TextResourceEditorInput`.** Sem herança, o registro só casa o tipo exato da entrada
//!   (ver `editor_panes.rs`): a entrada de teste é um tipo à parte, registrado no seu painel, e o caso "match on super class"
//!   confere a entrada de recurso de texto real contra o painel de recurso de texto (`registerTestResourceEditor` do original
//!   registra o `TestTextResourceEditor`, que herda o id `workbench.editors.textResourceEditor`).
//! - **`EditorMemento<TestViewState>`.** O estado `{ line }` é um objeto [`Dynamic`]; o memento cru (`Object.create(null)`) é um
//!   [`ObjectRef`] compartilhado entre os dois mementos do primeiro caso.
//! - **`WorkspaceTrustRequiredEditor`.** O caso abre a entrada pelo [`EditorPanes`] do grupo (o `group.openEditor` do original
//!   chega a ele) e espera a reabertura que a mudança de confiança dispara com `timeout(0)`, no lugar do
//!   `onDidActiveEditorChange` do `EditorService`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::any::{Any, TypeId};
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_real_timers, timeout};
use je_base::dynamic::{Dynamic, ObjectRef};
use je_base::errors;
use je_base::lifecycle::{DisposableRef, IDisposable, id_of};
use je_base::uri::URI;
use je_instantiation::{InstantiationOptions, InstantiationService, ServiceCollection};
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::resources::{ext_uri, uri_file};
use je_platform::workspace_trust::{IWorkspaceTrustManagementService, I_WORKSPACE_TRUST_MANAGEMENT_SERVICE};
use je_editor::text_resource_configuration_service::ITextResourceConfigurationService;
use je_workbench::browser::parts::editor::editor_pane::EditorMemento;
use je_workbench::browser::parts::editor::editor_panes::{
    EditorPaneDescriptor, EditorPanes, WORKSPACE_TRUST_REQUIRED_EDITOR_ID, editor_pane_registry,
};
use je_workbench::common::editor::editor_factory_registry::{IEditorSerializer, editor_factory_registry};
use je_workbench::common::editor::editor_input::{EditorInput, EditorInputBase};
use je_workbench::common::editor::text_resource_editor_input::TextResourceEditorInput;
use je_workbench::common::editor::{
    EditorInputCapabilities, EditorInputOrUntyped, EditorInputRef, EditorOpenContext, IEditorDescriptor, IEditorPane, TextEditorOptions,
};
use je_workbench::services::editor::editor_groups_service::{IEditorGroup, IEditorGroupsService};
use je_workbench::services::untitled::untitled_text_editor_input::UntitledTextEditorInput;
use je_workbench::test::browser::workbench_test_services::{TestEditorGroupsService, TestEditorInput};
use je_workbench::test::common::workbench_test_services::{TestTextResourceConfigurationService, TestWorkspaceTrustManagementService};
use support::test_editor_group_view::TestEditorGroupView;
use support::text_file_support::TextFileTestAccessor;

//#region Entradas de teste

/// Define uma entrada sem recurso (`readonly resource = undefined`) com o `typeId` e as capacidades dadas; `prefers_second`
/// é o `prefersEditorPane` do `TestInput` (`editors[1]`), e sem ele vale o padrão (o primeiro candidato).
macro_rules! define_test_input {
    ($name:ident, $type_id:literal, $capabilities:expr, $prefers_second:literal) => {
        struct $name {
            base: EditorInputBase,
        }

        impl $name {
            fn new() -> Rc<Self> {
                Rc::new_cyclic(|this| Self { base: EditorInputBase::new(this.clone()) })
            }
        }

        impl IDisposable for $name {
            fn dispose(&self) {
                self.base.dispose();
            }
        }

        impl EditorInput for $name {
            fn base(&self) -> &EditorInputBase {
                &self.base
            }

            fn as_any(&self) -> &dyn Any {
                self
            }

            fn editor_type_id(&self) -> String {
                $type_id.to_string()
            }

            fn resource(&self) -> Option<URI> {
                None
            }

            fn capabilities(&self) -> EditorInputCapabilities {
                $capabilities
            }

            fn prefers_editor_pane(&self, editor_panes: &[Rc<dyn IEditorDescriptor>]) -> Option<Rc<dyn IEditorDescriptor>> {
                if $prefers_second { editor_panes.get(1).cloned() } else { editor_panes.first().cloned() }
            }
        }
    };
}

define_test_input!(TestInput, "testInput", EditorInputCapabilities::READONLY, true);
define_test_input!(OtherTestInput, "otherTestInput", EditorInputCapabilities::READONLY, false);
define_test_input!(TestResourceEditorInput, "testResourceEditorInput", EditorInputCapabilities::READONLY, false);
define_test_input!(TrustRequiredTestInput, "trustRequiredTestInput", EditorInputCapabilities::REQUIRES_TRUST, false);

/// O `TestEditorInput` local do memento: `resource` e `id`, com o `matches` pelo `id`.
struct MementoTestEditorInput {
    base: EditorInputBase,
    resource: URI,
    id: String,
}

impl MementoTestEditorInput {
    /// `constructor(public resource: URI, private id = 'testEditorInputForMementoTest')`.
    fn new(resource: URI) -> Rc<Self> {
        Rc::new_cyclic(|this| Self { base: EditorInputBase::new(this.clone()), resource, id: "testEditorInputForMementoTest".to_string() })
    }
}

impl IDisposable for MementoTestEditorInput {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl EditorInput for MementoTestEditorInput {
    fn base(&self) -> &EditorInputBase {
        &self.base
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn editor_type_id(&self) -> String {
        "testEditorInputForMementoTest".to_string()
    }

    fn resource(&self) -> Option<URI> {
        Some(self.resource.clone())
    }

    fn matches(&self, other: &EditorInputOrUntyped) -> bool {
        match other {
            EditorInputOrUntyped::Typed(other) => other.as_any().downcast_ref::<Self>().is_some_and(|other| other.id == self.id),
            EditorInputOrUntyped::Untyped(_) => false,
        }
    }
}

/// `TestInputSerializer`.
struct TestInputSerializer;

impl je_instantiation::Injectable for TestInputSerializer {
    type Args = ();
    type Deps = ();

    fn create(_: (), _: ()) -> Rc<Self> {
        Rc::new(Self)
    }
}

impl IEditorSerializer for TestInputSerializer {
    fn can_serialize(&self, _editor_input: &EditorInputRef) -> bool {
        true
    }

    fn serialize(&self, input: &EditorInputRef) -> Option<String> {
        Some(input.editor_type_id())
    }

    /// `deserialize(...) { return {} as EditorInput; }`: o porte não fabrica uma entrada vazia, a desserialização devolve nada.
    fn deserialize(&self, _instantiation_service: &InstantiationService, _raw: &str) -> Option<EditorInputRef> {
        None
    }
}

//#endregion

//#region Auxiliares

/// `registerTestResourceEditor()` com o id real do `TextResourceEditor` (o `TestTextResourceEditor` herda o `ID` dele).
fn register_text_resource_editor() -> DisposableRef {
    editor_pane_registry().register_editor_pane(
        EditorPaneDescriptor::new("workbench.editors.textResourceEditor", "Text Editor"),
        vec![TypeId::of::<UntitledTextEditorInput>(), TypeId::of::<TextResourceEditorInput>()],
    )
}

/// `{ line }`.
fn view_state(line: i32) -> Dynamic {
    Dynamic::object([("line", Dynamic::from(line))])
}

/// `state.line`.
fn line_of(state: &Dynamic) -> Dynamic {
    state.get_str("line")
}

/// `new TestEditorGroupsService([...groups])`.
fn groups_service(groups: &[&Rc<TestEditorGroupView>]) -> Rc<dyn IEditorGroupsService> {
    TestEditorGroupsService::new(Some(groups.iter().map(|group| (*group).clone() as Rc<dyn IEditorGroup>).collect()))
}

/// `new TestTextResourceConfigurationService(configuration?)`.
fn text_resource_configuration(configuration: Option<Dynamic>) -> Rc<dyn ITextResourceConfigurationService> {
    let configuration_service: Rc<dyn IConfigurationService> = TestConfigurationService::new(configuration);
    TestTextResourceConfigurationService::new(configuration_service)
}

/// `new EditorMemento<TestViewState>('id', 'key', rawMemento, 3, editorGroupService, configurationService)`.
fn new_memento(
    guard: &LeakGuard,
    raw_memento: &ObjectRef,
    editor_group_service: Rc<dyn IEditorGroupsService>,
    configuration_service: Rc<dyn ITextResourceConfigurationService>,
) -> Rc<EditorMemento<Dynamic>> {
    guard.add_rc(EditorMemento::<Dynamic>::new("id", "key", raw_memento.clone(), 3, editor_group_service, configuration_service))
}

//#endregion

#[test]
fn editor_pane_api() {
    run_with_real_timers(async {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let group = TestEditorGroupView::new(1);
        let editor = guard.add_rc(EditorPaneDescriptor::new("testEditor", "name").instantiate(group.id()));
        assert_eq!(editor.group_id(), 1);
        let input: EditorInputRef = guard.add_rc(OtherTestInput::new());

        assert!(!editor.is_visible());
        assert!(editor.input().is_none());

        editor.set_input(&input, Some(TextEditorOptions::default())).await.unwrap();
        assert_eq!(id_of(&editor.input().unwrap()), id_of(&input));
        editor.set_visible(true);
        assert!(editor.is_visible());
        editor.dispose();
        editor.clear_input();
        editor.set_visible(false);
        assert!(!editor.is_visible());
        assert!(editor.input().is_none());
        assert!(editor.get_control().is_none());
    });
}

#[test]
fn editor_pane_descriptor() {
    let editor_descriptor = EditorPaneDescriptor::new("id", "name");
    assert_eq!(editor_descriptor.editor_type_id(), "id");
    assert_eq!(editor_descriptor.name(), "name");
}

#[test]
fn editor_pane_registration() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let editor_descriptor1 = EditorPaneDescriptor::new("id1", "name");
    let editor_descriptor2 = EditorPaneDescriptor::new("id2", "name");

    let editor_registry = editor_pane_registry();
    let old_editors_cnt = editor_registry.get_editor_panes().len();
    let old_input_cnt = editor_registry.get_editors().len();

    guard.add(editor_registry.register_editor_pane(editor_descriptor1.clone(), vec![TypeId::of::<TestInput>()]));
    guard.add(editor_registry.register_editor_pane(editor_descriptor2.clone(), vec![TypeId::of::<TestInput>(), TypeId::of::<OtherTestInput>()]));

    assert_eq!(editor_registry.get_editor_panes().len(), old_editors_cnt + 2);
    assert_eq!(editor_registry.get_editors().len(), old_input_cnt + 3);

    let test_input: EditorInputRef = guard.add_rc(TestInput::new());
    let other_test_input: EditorInputRef = guard.add_rc(OtherTestInput::new());
    assert!(Rc::ptr_eq(&editor_registry.get_editor_pane(&test_input).unwrap(), &editor_descriptor2));
    assert!(Rc::ptr_eq(&editor_registry.get_editor_pane(&other_test_input).unwrap(), &editor_descriptor2));

    assert!(Rc::ptr_eq(&editor_registry.get_editor_pane_by_type("id1").unwrap(), &editor_descriptor1));
    assert!(Rc::ptr_eq(&editor_registry.get_editor_pane_by_type("id2").unwrap(), &editor_descriptor2));
    assert!(editor_registry.get_editor_pane_by_type("id3").is_none());
}

#[test]
fn editor_pane_lookup_favors_specific_class_over_superclass_match_on_specific_class() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let editor_registry = editor_pane_registry();
    let d1 = EditorPaneDescriptor::new("testEditor", "name");

    guard.add(register_text_resource_editor());
    guard.add(editor_registry.register_editor_pane(d1, vec![TypeId::of::<TestResourceEditorInput>()]));

    let accessor = TextFileTestAccessor::new(&guard.store());
    guard.add_rc(accessor.files());

    let group = TestEditorGroupView::new(1);

    let specific: EditorInputRef = guard.add_rc(TestResourceEditorInput::new());
    let editor = guard.add_rc(editor_registry.get_editor_pane(&specific).unwrap().instantiate(group.id()));
    assert_eq!(editor.get_id(), "testEditor");

    let text_resource: EditorInputRef = guard.add_rc(accessor.instantiation_service.create_instance::<TextResourceEditorInput>((
        uri_file("/fake"),
        Some("fake".to_string()),
        Some(String::new()),
        None,
        None,
    )));
    let other_editor = guard.add_rc(editor_registry.get_editor_pane(&text_resource).unwrap().instantiate(group.id()));
    assert_eq!(other_editor.get_id(), "workbench.editors.textResourceEditor");
}

#[test]
fn editor_pane_lookup_favors_specific_class_over_superclass_match_on_super_class() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let editor_registry = editor_pane_registry();

    let accessor = TextFileTestAccessor::new(&guard.store());
    guard.add_rc(accessor.files());

    let group = TestEditorGroupView::new(1);

    guard.add(register_text_resource_editor());
    let text_resource: EditorInputRef = guard.add_rc(accessor.instantiation_service.create_instance::<TextResourceEditorInput>((
        uri_file("/fake"),
        Some("fake".to_string()),
        Some(String::new()),
        None,
        None,
    )));
    let editor = guard.add_rc(editor_registry.get_editor_pane(&text_resource).unwrap().instantiate(group.id()));

    assert_eq!("workbench.editors.textResourceEditor", editor.get_id());
}

#[test]
fn editor_input_serializer() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let test_input: EditorInputRef = guard.add_rc(TestEditorInput::new(uri_file("/fake"), "testTypeId"));
    let editor_input_registry = editor_factory_registry();

    let instantiation_service = InstantiationService::with_options(ServiceCollection::new(), InstantiationOptions { strict: false, parent: None, enable_tracing: false });
    instantiation_service.invoke_function(|accessor| editor_input_registry.start(accessor));
    guard.add(editor_input_registry.register_editor_serializer::<TestInputSerializer>(&test_input.editor_type_id()));

    assert!(editor_input_registry.get_editor_serializer_by_type_id("testTypeId").is_some());

    assert!(editor_input_registry.get_editor_serializer(&test_input).is_some());

    // throws when registering serializer for same type
    assert!(errors::catch(|| editor_input_registry.register_editor_serializer::<TestInputSerializer>(&test_input.editor_type_id())).is_err());
}

#[test]
fn editor_memento_basics() {
    let test_group0 = TestEditorGroupView::new(0);
    let test_group1 = TestEditorGroupView::new(1);
    let test_group4 = TestEditorGroupView::new(4);

    let configuration_service = text_resource_configuration(None);

    let editor_group_service = groups_service(&[&test_group0, &test_group1, &TestEditorGroupView::new(2)]);

    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let raw_memento = ObjectRef::new();
    let mut memento = new_memento(&guard, &raw_memento, editor_group_service.clone(), configuration_service.clone());

    let a = uri_file("/A");
    let mut res = memento.load_editor_state(&*test_group0, &a);
    assert!(res.is_none());

    memento.save_editor_state(&*test_group0, &a, view_state(3));
    res = memento.load_editor_state(&*test_group0, &a);
    assert_eq!(line_of(&res.unwrap()), Dynamic::from(3));

    memento.save_editor_state(&*test_group1, &a, view_state(5));
    res = memento.load_editor_state(&*test_group1, &a);
    assert_eq!(line_of(&res.unwrap()), Dynamic::from(5));

    // Ensure capped at 3 elements
    for path in ["/B", "/C", "/D", "/E"] {
        memento.save_editor_state(&*test_group0, &uri_file(path), view_state(1));
    }

    assert!(memento.load_editor_state(&*test_group0, &a).is_none());
    assert!(memento.load_editor_state(&*test_group0, &uri_file("/B")).is_none());
    assert!(memento.load_editor_state(&*test_group0, &uri_file("/C")).is_some());
    assert!(memento.load_editor_state(&*test_group0, &uri_file("/D")).is_some());
    assert!(memento.load_editor_state(&*test_group0, &uri_file("/E")).is_some());

    // Save at an unknown group
    memento.save_editor_state(&*test_group4, &uri_file("/E"), view_state(1));
    assert!(memento.load_editor_state(&*test_group4, &uri_file("/E")).is_some()); // only gets removed when memento is saved
    memento.save_editor_state(&*test_group4, &uri_file("/C"), view_state(1));
    assert!(memento.load_editor_state(&*test_group4, &uri_file("/C")).is_some()); // only gets removed when memento is saved

    memento.save_state();

    memento = new_memento(&guard, &raw_memento, editor_group_service, configuration_service);
    assert!(memento.load_editor_state(&*test_group0, &uri_file("/C")).is_some());
    assert!(memento.load_editor_state(&*test_group0, &uri_file("/D")).is_some());
    assert!(memento.load_editor_state(&*test_group0, &uri_file("/E")).is_some());

    // Check on entries no longer there from invalid groups
    assert!(memento.load_editor_state(&*test_group4, &uri_file("/E")).is_none());
    assert!(memento.load_editor_state(&*test_group4, &uri_file("/C")).is_none());

    memento.clear_editor_state(&uri_file("/C"), Some(&*test_group4));
    memento.clear_editor_state(&uri_file("/E"), None);

    assert!(memento.load_editor_state(&*test_group4, &uri_file("/C")).is_none());
    assert!(memento.load_editor_state(&*test_group0, &uri_file("/D")).is_some());
    assert!(memento.load_editor_state(&*test_group0, &uri_file("/E")).is_none());
}

#[test]
fn editor_memento_move() {
    let test_group0 = TestEditorGroupView::new(0);

    let configuration_service = text_resource_configuration(None);
    let editor_group_service = groups_service(&[&test_group0]);

    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let raw_memento = ObjectRef::new();
    let memento = new_memento(&guard, &raw_memento, editor_group_service, configuration_service);

    memento.save_editor_state(&*test_group0, &uri_file("/some/folder/file-1.txt"), view_state(1));
    memento.save_editor_state(&*test_group0, &uri_file("/some/folder/file-2.txt"), view_state(2));
    memento.save_editor_state(&*test_group0, &uri_file("/some/other/file.txt"), view_state(3));

    memento.move_editor_state(&uri_file("/some/folder/file-1.txt"), &uri_file("/some/folder/file-moved.txt"), &ext_uri());

    let mut res = memento.load_editor_state(&*test_group0, &uri_file("/some/folder/file-1.txt"));
    assert!(res.is_none());

    res = memento.load_editor_state(&*test_group0, &uri_file("/some/folder/file-moved.txt"));
    assert_eq!(line_of(&res.unwrap()), Dynamic::from(1));

    memento.move_editor_state(&uri_file("/some/folder"), &uri_file("/some/folder-moved"), &ext_uri());

    res = memento.load_editor_state(&*test_group0, &uri_file("/some/folder-moved/file-moved.txt"));
    assert_eq!(line_of(&res.unwrap()), Dynamic::from(1));

    res = memento.load_editor_state(&*test_group0, &uri_file("/some/folder-moved/file-2.txt"));
    assert_eq!(line_of(&res.unwrap()), Dynamic::from(2));
}

#[test]
fn editor_memento_use_with_editor_input() {
    let test_group0 = TestEditorGroupView::new(0);
    let editor_group_service = TestEditorGroupsService::new(None);
    let configuration_service = text_resource_configuration(None);

    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let raw_memento = ObjectRef::new();
    let memento = new_memento(&guard, &raw_memento, editor_group_service, configuration_service);

    let test_input_a = guard.add_rc(MementoTestEditorInput::new(uri_file("/A")));
    let input_a: EditorInputRef = test_input_a.clone();

    let mut res = memento.load_editor_state(&*test_group0, &input_a);
    assert!(res.is_none());

    memento.save_editor_state(&*test_group0, &input_a, view_state(3));
    res = memento.load_editor_state(&*test_group0, &input_a);
    assert_eq!(line_of(&res.unwrap()), Dynamic::from(3));

    // State removed when input gets disposed
    test_input_a.dispose();
    res = memento.load_editor_state(&*test_group0, &input_a);
    assert!(res.is_none());
}

#[test]
fn editor_memento_clear_on_editor_dispose() {
    let test_group0 = TestEditorGroupView::new(0);
    let editor_group_service = TestEditorGroupsService::new(None);
    let configuration_service = text_resource_configuration(None);

    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let raw_memento = ObjectRef::new();
    let memento = new_memento(&guard, &raw_memento, editor_group_service, configuration_service);

    let test_input_a = guard.add_rc(MementoTestEditorInput::new(uri_file("/A")));
    let input_a: EditorInputRef = test_input_a.clone();

    let mut res = memento.load_editor_state(&*test_group0, &input_a);
    assert!(res.is_none());

    memento.save_editor_state(&*test_group0, &test_input_a.resource, view_state(3));
    res = memento.load_editor_state(&*test_group0, &input_a);
    assert_eq!(line_of(&res.unwrap()), Dynamic::from(3));

    // State not yet removed when input gets disposed because we used resource
    test_input_a.dispose();
    res = memento.load_editor_state(&*test_group0, &input_a);
    assert!(res.is_some());

    let test_input_b = guard.add_rc(MementoTestEditorInput::new(uri_file("/B")));
    let input_b: EditorInputRef = test_input_b.clone();

    res = memento.load_editor_state(&*test_group0, &input_b);
    assert!(res.is_none());

    memento.save_editor_state(&*test_group0, &test_input_b.resource, view_state(3));
    res = memento.load_editor_state(&*test_group0, &input_b);
    assert_eq!(line_of(&res.unwrap()), Dynamic::from(3));

    memento.clear_editor_state_on_dispose(&test_input_b.resource, &input_b);

    // State removed when input gets disposed
    test_input_b.dispose();
    res = memento.load_editor_state(&*test_group0, &input_b);
    assert!(res.is_none());
}

#[test]
fn editor_memento_workbench_editor_shared_view_state() {
    let test_group0 = TestEditorGroupView::new(0);
    let test_group1 = TestEditorGroupView::new(1);

    let configuration_service = text_resource_configuration(Some(Dynamic::object([(
        "workbench",
        Dynamic::object([("editor", Dynamic::object([("sharedViewState", Dynamic::Bool(true))]))]),
    )])));
    let editor_group_service = groups_service(&[&test_group0]);

    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let raw_memento = ObjectRef::new();
    let memento = new_memento(&guard, &raw_memento, editor_group_service, configuration_service);

    let resource = uri_file("/some/folder/file-1.txt");
    memento.save_editor_state(&*test_group0, &resource, view_state(1));

    let mut res = memento.load_editor_state(&*test_group0, &resource);
    assert_eq!(line_of(&res.unwrap()), Dynamic::from(1));

    res = memento.load_editor_state(&*test_group1, &resource);
    assert_eq!(line_of(&res.unwrap()), Dynamic::from(1));

    memento.save_editor_state(&*test_group0, &resource, view_state(3));

    res = memento.load_editor_state(&*test_group1, &resource);
    assert_eq!(line_of(&res.unwrap()), Dynamic::from(3));

    memento.save_editor_state(&*test_group1, &resource, view_state(1));

    res = memento.load_editor_state(&*test_group1, &resource);
    assert_eq!(line_of(&res.unwrap()), Dynamic::from(1));

    memento.clear_editor_state(&resource, Some(&*test_group0));
    memento.clear_editor_state(&resource, Some(&*test_group1));

    res = memento.load_editor_state(&*test_group1, &resource);
    assert_eq!(line_of(&res.unwrap()), Dynamic::from(1));

    memento.clear_editor_state(&resource, None);

    res = memento.load_editor_state(&*test_group1, &resource);
    assert!(res.is_none());
}

#[test]
fn workspace_trust_required_editor() {
    run_with_real_timers(async {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();

        let workspace_trust_service = guard.add_rc(TestWorkspaceTrustManagementService::new(Some(false), None));
        let services = ServiceCollection::new();
        services.set(I_WORKSPACE_TRUST_MANAGEMENT_SERVICE, workspace_trust_service.clone() as Rc<dyn IWorkspaceTrustManagementService>);
        let instantiation_service = InstantiationService::with_options(services, InstantiationOptions { strict: false, parent: None, enable_tracing: false });
        workspace_trust_service.set_workspace_trust(false).await.unwrap();

        let editor_panes = guard.add_rc(EditorPanes::new(1, instantiation_service));

        let editor_descriptor = EditorPaneDescriptor::new("trustRequiredTestEditor", "name");
        guard.add(editor_pane_registry().register_editor_pane(editor_descriptor, vec![TypeId::of::<TrustRequiredTestInput>()]));

        let test_input = TrustRequiredTestInput::new();
        let input: EditorInputRef = guard.add_rc(test_input);

        editor_panes.open_editor(&input, None, EditorOpenContext::default()).await.unwrap();
        assert_eq!(editor_panes.active_editor_pane().unwrap().get_id(), WORKSPACE_TRUST_REQUIRED_EDITOR_ID);

        workspace_trust_service.set_workspace_trust(true).await.unwrap();
        timeout(0).await.unwrap();
        assert_eq!(editor_panes.active_editor_pane().unwrap().get_id(), "trustRequiredTestEditor");

        workspace_trust_service.set_workspace_trust(false).await.unwrap();
        timeout(0).await.unwrap();
        assert_eq!(editor_panes.active_editor_pane().unwrap().get_id(), WORKSPACE_TRUST_REQUIRED_EDITOR_ID);

        editor_panes.close_editor(&input);
    });
}
