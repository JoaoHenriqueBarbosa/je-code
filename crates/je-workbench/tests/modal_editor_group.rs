//! Porte de `src/vs/workbench/services/editor/test/browser/modalEditorGroup.test.ts` (suíte `Modal Editor Group`), caso a caso e na mesma ordem.
//!
//! Fatia do `ModalEditorPart` sobre o `EditorPart` lógico e o `EditorParts` (ver `PORT_PLAN.md`). Ainda NÃO portados (o placar
//! conta só os `#[test]`):
//! - `floating layout applies DPR 2.5 border insets only to the main editor`: lê `element.style.height`, as classes CSS
//!   `floating-editor-outer-*` e `modalElement.querySelector('.modal-editor-part')`, e usa o `TestLayoutService` com
//!   `isFloatingPanelsEnabled`. A renderização é nativa e esses atributos do DOM não existem (o `layout` flutuante do
//!   `EditorPart` também não foi portado, ver `editor_part.rs`).
//! - `Escape closes modal before focused controls can stop propagation` e `bubble close keybindings reach focused controls and
//!   dispatch once`: despacham `KeyboardEvent` de DOM num botão dentro do `modalElement`, com os ouvintes de captura e de
//!   bolha que o `ModalEditorPart.create` registra no elemento e no `mainWindow`. A entrada de teclado é da árvore de widgets.
//! - `openEditor with MODAL_GROUP ignores preserveFocus`: afirma `pane.options?.preserveFocus === false`, e o `IEditorPane` do
//!   porte não expõe as `options` com que o painel foi aberto (o trait não tem esse membro).
//!
//! Notas de tradução:
//! - **`workbenchInstantiationService({ contextKeyService: MockScopableContextKeyService }, disposables)`** é o
//!   [`TextFileTestAccessor`] (o `workbenchInstantiationService` com o serviço de arquivos de texto) mais os serviços que a
//!   parte de editores, o resolvedor, o `TextEditorService` e o `EditorService` leem, registrados como o
//!   `EditorServiceAccessor` do `workingCopyBackupTracker` faz; o `IContextKeyService` é o `MockContextKeyService::scopable()`
//!   ([`ModalEditorGroupEnv::context_key_service`]) e o `IEditorService` padrão é o `TestEditorService` do acessor.
//! - **`createEditorParts(instantiationService, disposables)`** e `instantiationService.stub(IEditorGroupsService, parts)` são o
//!   `create_editor_parts` e o `services.set(I_EDITOR_GROUPS_SERVICE, ...)`, depois de iniciar o registro de serializadores
//!   (`EditorFactoryRegistry.start`). O `setup` registra o painel e o serializador de teste (`register_test_editor`).
//! - **`TestConfigurationService.setUserConfiguration('workbench.editor.useModal', ...)` e
//!   `instantiationService.stub(IConfigurationService, ...)`** valem como `set_user_configuration` no
//!   `TestConfigurationService` do acessor, antes de criar as partes (o `stub` do original troca o serviço antes de o
//!   `createEditorParts` ler o valor).
//! - **`instantiationService.createInstance(EditorService, undefined)` e `stub(IEditorService, editorService)`** são
//!   [`ModalEditorGroupEnv::use_editor_service`], que cria o `EditorService` real e o registra como `IEditorService`.
//! - **Funções `async`.** Os casos `async` rodam em `run_with_faked_timers`, com o guarda criado dentro do corpo, e `await` é
//!   `.await.unwrap()`. `await findGroup(...)` sobre `T | Promise<T>` é `await_maybe`. `modalPart.close()` é
//!   `close_with_options(None)` (a promessa; o `close()` do contrato é a forma síncrona).
//! - **Identidade de partes.** `assert.strictEqual(modalPart1, modalPart2)` compara o endereço do objeto ([`same_part`]).
//! - **`deepStrictEqual` do `activeModalEditorPart`.** O objeto `{ activeModalEditorPart, modalEditorVisible }` vira as duas
//!   afirmações, uma por campo.
//! - **`MODAL_GROUP_TYPE`.** Alias de tipo do TypeScript (`typeof MODAL_GROUP`): aqui é o `GroupIdentifier` que a constante tem.
//! - **`Memento.clear(StorageScope.PROFILE)`.** O `EditorParts` de cada caso lê o memento do perfil da thread do caso, que
//!   nasce vazio, então a limpeza do cache não tem o que limpar; o estado entra no armazenamento antes de `create_editor_parts`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::any::TypeId;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::async_::await_maybe;
use je_base::dynamic::Dynamic;
use je_base::event::Event;
use je_base::lifecycle::{DisposableRef, id_of};
use je_base::promise::{MaybePromise, Promise};
use je_instantiation::InstantiationService;
use je_platform::commands::commands::{I_COMMAND_SERVICE, ICommandService};
use je_platform::contextkey::contextkey::{I_CONTEXT_KEY_SERVICE, IContextKeyService};
use je_platform::dialogs::{I_DIALOG_SERVICE, I_FILE_DIALOG_SERVICE, IDialogService, IFileDialogService};
use je_platform::keybinding::mock_keybinding_service::MockContextKeyService;
use je_platform::log::I_LOG_SERVICE;
use je_platform::notification::notification::{I_NOTIFICATION_SERVICE, INotificationService};
use je_platform::opener::editor_types::{EditorActivation, EditorOptions, ModalEditorPartOptions};
use je_platform::resources::uri_file;
use je_platform::storage::{I_STORAGE_SERVICE, InMemoryStorageService, StorageScope, StorageService, StorageTarget};
use je_platform::uri_identity::uri_identity::{I_URI_IDENTITY_SERVICE, IUriIdentityService};
use je_platform::workspace::{I_WORKSPACE_CONTEXT_SERVICE, IWorkspaceContextService};
use je_platform::workspace_trust::{I_WORKSPACE_TRUST_REQUEST_SERVICE, IWorkspaceTrustRequestService};
use je_storage::StorageValue;
use je_ui::Ui;
use je_workbench::common::contextkeys::editor_part_modal_visible_context;
use je_workbench::common::editor::editor_factory_registry::editor_factory_registry;
use je_workbench::common::editor::side_by_side_editor_input::SideBySideEditorInput;
use je_workbench::common::editor::{EditorInputCapabilities, EditorInputRef, EditorInputWithOptions, GroupIdentifier, TextEditorOptions, UntypedEditorInput};
use je_workbench::services::editor::browser_editor_resolver_service::EditorResolverService;
use je_workbench::services::editor::editor_group_finder::{FindGroupEditor, FindGroupResult, find_group};
use je_workbench::services::editor::editor_groups_service::{
    GroupsOrder, I_EDITOR_GROUPS_SERVICE, IEditorGroup, IEditorGroupsService, IEditorPart, IModalEditorPart, MODAL_GROUP, PreferredGroup, ShowTabs,
};
use je_workbench::services::editor::editor_resolver_service::{I_EDITOR_RESOLVER_SERVICE, IEditorResolverService};
use je_workbench::services::editor::editor_service::{I_EDITOR_SERVICE, IEditorService};
use je_workbench::services::editor::editor_service_impl::EditorService;
use je_workbench::services::environment::environment_service::{I_WORKBENCH_ENVIRONMENT_SERVICE, IWorkbenchEnvironmentService};
use je_workbench::services::extensions::extensions::{I_EXTENSION_SERVICE, IExtensionService};
use je_workbench::services::host::host::{I_HOST_SERVICE, IHostService};
use je_workbench::services::textfile::text_editor_service::{I_TEXT_EDITOR_SERVICE, ITextEditorService, TextEditorService};
use je_workbench::services::untitled::untitled_text_editor_service::{I_UNTITLED_TEXT_EDITOR_SERVICE, IUntitledTextEditorService};
use je_workbench::test::browser::workbench_test_services::{
    TestEditorParts, TestEnvironmentService, TestExtensionService, TestFileEditorInput, TestHostService, create_editor_parts, register_test_editor,
};
use je_workbench::test::common::workbench_test_services::TestWorkspaceTrustRequestService;
use support::mock_command_service::MockCommandService;
use support::text_file_support::TextFileTestAccessor;

const TEST_EDITOR_ID: &str = "MyFileEditorForModalEditorGroup";
const TEST_EDITOR_INPUT_ID: &str = "testEditorInputForModalEditorGroup";

/// `workbench.editor.useModal`.
const USE_MODAL_SETTING: &str = "workbench.editor.useModal";

/// O que cada caso escolhe do ambiente antes de as partes nascerem.
#[derive(Default)]
struct EnvOptions {
    /// `setUserConfiguration('workbench.editor.useModal', value)`.
    use_modal: Option<&'static str>,
    /// O JSON que o caso grava em `memento/workbench.editorParts` (perfil).
    profile_state: Option<&'static str>,
}

/// O que `workbenchInstantiationService`, `createEditorParts` e o `stub(IEditorGroupsService, parts)` entregam aos casos.
struct ModalEditorGroupEnv {
    accessor: TextFileTestAccessor,
    /// `parts`.
    parts: Rc<TestEditorParts>,
    /// `instantiationService.get(IContextKeyService)`.
    context_key_service: Rc<MockContextKeyService>,
}

impl ModalEditorGroupEnv {
    fn new(guard: &LeakGuard, options: EnvOptions) -> Self {
        // O `setup` da suíte: `registerTestEditor(TEST_EDITOR_ID, [TestFileEditorInput, SideBySideEditorInput], TEST_EDITOR_INPUT_ID)`
        guard.add(register_test_editor(
            TEST_EDITOR_ID,
            vec![TypeId::of::<TestFileEditorInput>(), TypeId::of::<SideBySideEditorInput>()],
            Some(TEST_EDITOR_INPUT_ID),
        ));

        let disposables = guard.store();
        let accessor = TextFileTestAccessor::new(&disposables);
        let collection = accessor.collection.clone();
        let instantiation_service = accessor.instantiation_service.clone();

        if let Some(use_modal) = options.use_modal {
            let _ = accessor.test_configuration_service.set_user_configuration(USE_MODAL_SETTING, Dynamic::string(use_modal), None);
        }

        let storage = InMemoryStorageService::new();
        if let Some(profile_state) = options.profile_state {
            storage.store("memento/workbench.editorParts", StorageValue::String(profile_state.to_string()), StorageScope::Profile, StorageTarget::Machine);
        }
        let storage_service: Rc<dyn StorageService> = Rc::new(storage.clone());
        disposables.add_rc(storage);

        // Os serviços do `workbenchInstantiationService` que a parte, o resolvedor e o `EditorService` leem.
        let context_key_service = MockContextKeyService::scopable();
        collection.set(I_CONTEXT_KEY_SERVICE, context_key_service.clone() as Rc<dyn IContextKeyService>);
        collection.set(I_STORAGE_SERVICE, storage_service);
        collection.set(I_WORKSPACE_CONTEXT_SERVICE, accessor.context_service.clone() as Rc<dyn IWorkspaceContextService>);
        collection.set(I_URI_IDENTITY_SERVICE, accessor.uri_identity_service.clone() as Rc<dyn IUriIdentityService>);
        collection.set(I_DIALOG_SERVICE, accessor.dialog_service.clone() as Rc<dyn IDialogService>);
        collection.set(I_FILE_DIALOG_SERVICE, accessor.file_dialog_service.clone() as Rc<dyn IFileDialogService>);
        collection.set(I_LOG_SERVICE, accessor.log_service.clone());
        collection.set(I_COMMAND_SERVICE, Rc::new(MockCommandService::default()) as Rc<dyn ICommandService>);
        collection.set(I_NOTIFICATION_SERVICE, accessor.notification_service.clone() as Rc<dyn INotificationService>);
        collection.set(I_EXTENSION_SERVICE, TestExtensionService::new() as Rc<dyn IExtensionService>);
        collection.set(I_HOST_SERVICE, Rc::new(TestHostService) as Rc<dyn IHostService>);
        let workspace_trust_request_service = disposables.add_rc(TestWorkspaceTrustRequestService::new(false));
        collection.set(I_WORKSPACE_TRUST_REQUEST_SERVICE, workspace_trust_request_service as Rc<dyn IWorkspaceTrustRequestService>);
        collection.set(I_WORKBENCH_ENVIRONMENT_SERVICE, TestEnvironmentService::new() as Rc<dyn IWorkbenchEnvironmentService>);
        collection.set(I_UNTITLED_TEXT_EDITOR_SERVICE, accessor.untitled_text_editor_service.clone() as Rc<dyn IUntitledTextEditorService>);

        // `createEditorParts(instantiationService, disposables)` e `instantiationService.stub(IEditorGroupsService, parts)`
        instantiation_service.invoke_function(|accessor| editor_factory_registry().start(accessor));
        let parts = guard.add_rc(create_editor_parts(&Ui::new(), &instantiation_service));
        collection.set(I_EDITOR_GROUPS_SERVICE, parts.groups_service());

        let editor_resolver_service = disposables.add_rc(instantiation_service.create_instance::<EditorResolverService>(()));
        collection.set(I_EDITOR_RESOLVER_SERVICE, editor_resolver_service as Rc<dyn IEditorResolverService>);
        let text_editor_service = disposables.add_rc(instantiation_service.create_instance::<TextEditorService>(()));
        collection.set(I_TEXT_EDITOR_SERVICE, text_editor_service as Rc<dyn ITextEditorService>);

        Self { accessor, parts, context_key_service }
    }

    fn instantiation_service(&self) -> &Rc<InstantiationService> {
        &self.accessor.instantiation_service
    }

    /// `disposables.add(instantiationService.createInstance(EditorService, undefined))` e
    /// `instantiationService.stub(IEditorService, editorService)`.
    fn use_editor_service(&self, guard: &LeakGuard) -> Rc<EditorService> {
        let editor_service = guard.add_rc(self.instantiation_service().create_instance::<EditorService>(None));
        self.accessor.collection.set(I_EDITOR_SERVICE, editor_service.clone() as Rc<dyn IEditorService>);

        editor_service
    }

    /// `findGroup(accessor, editor, preferredGroup)`.
    fn find_group(&self, editor: FindGroupEditor, preferred_group: Option<PreferredGroup>) -> FindGroupResult {
        self.instantiation_service().invoke_function(|accessor| find_group(accessor, &editor, preferred_group.as_ref()))
    }
}

/// `createTestFileEditorInput(URI.file(resource), TEST_EDITOR_INPUT_ID)`.
fn input(guard: &LeakGuard, resource: &str) -> Rc<TestFileEditorInput> {
    guard.add_rc(TestFileEditorInput::new(uri_file(resource), TEST_EDITOR_INPUT_ID))
}

/// A entrada de teste como o handle de entrada que o grupo recebe.
fn editor(input: &Rc<TestFileEditorInput>) -> EditorInputRef {
    input.clone()
}

/// `assert.strictEqual(actual, expected)` entre editores: a mesma entrada.
fn is_editor(actual: Option<EditorInputRef>, expected: &Rc<TestFileEditorInput>) -> bool {
    actual.is_some_and(|actual| id_of(&actual) == id_of(expected))
}

/// `{ pinned: true }`.
fn pinned() -> EditorOptions {
    EditorOptions { pinned: Some(true), ..EditorOptions::default() }
}

/// `{ resource }`: o editor sem tipo de `findGroup(accessor, { resource: input.resource }, ...)`.
fn untyped(input: &Rc<TestFileEditorInput>) -> FindGroupEditor {
    FindGroupEditor::Untyped(UntypedEditorInput::from_resource(input.resource.clone()))
}

/// `{ editor: input, options: {} }`.
fn with_empty_options(input: &Rc<TestFileEditorInput>) -> FindGroupEditor {
    FindGroupEditor::WithOptions(EditorInputWithOptions { editor: editor(input), options: Some(TextEditorOptions::default()) })
}

/// `assert.strictEqual(a, b)` entre partes modais: o mesmo objeto.
fn same_part(a: &Rc<dyn IModalEditorPart>, b: &Rc<dyn IModalEditorPart>) -> bool {
    std::ptr::addr_eq(Rc::as_ptr(a), Rc::as_ptr(b))
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

/// `await findGroup(...)`, sobre `T | Promise<T>`: o grupo resolvido.
async fn resolved_group(result: FindGroupResult) -> (Rc<dyn IEditorGroup>, Option<EditorActivation>) {
    await_maybe(result).await.unwrap()
}

#[test]
fn modal_group_constant_is_defined_correctly() {
    assert_eq!(MODAL_GROUP, -4);
    let _: GroupIdentifier = MODAL_GROUP; // typeof MODAL_GROUP === 'number'
}

#[test]
fn modal_group_type_type_exists() {
    let modal_group_value: GroupIdentifier = MODAL_GROUP;
    assert_eq!(modal_group_value, -4);
}

#[test]
fn create_modal_editor_part_creates_a_modal_editor_part() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let env = ModalEditorGroupEnv::new(&guard, EnvOptions::default());
        let parts = env.parts.clone();

        let modal_part = parts.create_modal_editor_part(None).await.unwrap();

        let _ = modal_part.active_group(); // assert.ok(modalPart.activeGroup)
        // `typeof modalPart.close === 'function'` é o método `close_with_options` do trait.

        modal_part.close_with_options(None).await.unwrap();
    });
}

#[test]
fn modal_editor_part_has_correct_initial_state() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let env = ModalEditorGroupEnv::new(&guard, EnvOptions::default());
        let parts = env.parts.clone();

        let modal_part = parts.create_modal_editor_part(None).await.unwrap();

        // Modal part should have exactly one group initially with 0 editors
        assert_eq!(modal_part.active_group().count(), 0);

        modal_part.close_with_options(None).await.unwrap();
    });
}

#[test]
fn modal_editor_part_can_open_editors() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let env = ModalEditorGroupEnv::new(&guard, EnvOptions::default());
        let parts = env.parts.clone();

        let modal_part = parts.create_modal_editor_part(None).await.unwrap();

        let input = input(&guard, "foo/bar");
        modal_part.active_group().open_editor(&editor(&input), Some(pinned())).await.unwrap();

        assert_eq!(modal_part.active_group().count(), 1);
        assert!(is_editor(modal_part.active_group().active_editor(), &input));

        modal_part.close_with_options(None).await.unwrap();
    });
}

#[test]
fn modal_editor_part_is_added_to_parts_list() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let env = ModalEditorGroupEnv::new(&guard, EnvOptions::default());
        let parts = env.parts.clone();

        let initial_group_count = parts.groups().len();

        let modal_part = parts.create_modal_editor_part(None).await.unwrap();

        // Modal part's group should be added to the total groups
        assert_eq!(parts.groups().len(), initial_group_count + 1);

        modal_part.close_with_options(None).await.unwrap();
    });
}

#[test]
fn closing_modal_part_fires_on_will_close_event() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let env = ModalEditorGroupEnv::new(&guard, EnvOptions::default());
        let parts = env.parts.clone();

        let modal_part = parts.create_modal_editor_part(None).await.unwrap();

        // Verify onWillClose is an event that can be listened to
        let (_, listener) = count(&modal_part.on_will_close());
        guard.add(listener);

        let input = input(&guard, "foo/bar");
        modal_part.active_group().open_editor(&editor(&input), Some(pinned())).await.unwrap();

        // Verify close returns true
        let result = modal_part.close_with_options(None).await.unwrap();
        assert!(result);
    });
}

#[test]
fn modal_editor_part_close_returns_true_when_no_confirming_editors() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let env = ModalEditorGroupEnv::new(&guard, EnvOptions::default());
        let parts = env.parts.clone();

        let modal_part = parts.create_modal_editor_part(None).await.unwrap();

        let input = input(&guard, "foo/bar");
        modal_part.active_group().open_editor(&editor(&input), Some(pinned())).await.unwrap();

        let result = modal_part.close_with_options(None).await.unwrap();

        assert!(result);
    });
}

#[test]
fn modal_editor_part_get_groups_returns_groups_in_correct_order() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let env = ModalEditorGroupEnv::new(&guard, EnvOptions::default());
        let parts = env.parts.clone();

        let modal_part = parts.create_modal_editor_part(None).await.unwrap();

        let input = input(&guard, "foo/bar");
        modal_part.active_group().open_editor(&editor(&input), Some(pinned())).await.unwrap();

        // Modal part group should be in the groups list
        let all_groups = parts.get_groups(GroupsOrder::MostRecentlyActive);
        let modal_group = modal_part.active_group();

        assert!(all_groups.iter().any(|group| group.id() == modal_group.id()));

        modal_part.close_with_options(None).await.unwrap();
    });
}

#[test]
fn modal_editor_part_is_singleton_subsequent_calls_return_same_instance() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let env = ModalEditorGroupEnv::new(&guard, EnvOptions::default());
        let parts = env.parts.clone();

        let modal_part1 = parts.create_modal_editor_part(None).await.unwrap();
        let modal_part2 = parts.create_modal_editor_part(None).await.unwrap();

        // Same instance should be returned
        assert!(same_part(&modal_part1, &modal_part2));
        assert_eq!(modal_part1.active_group().id(), modal_part2.active_group().id());

        modal_part1.close_with_options(None).await.unwrap();
    });
}

#[test]
fn create_modal_editor_part_is_race_proof_concurrent_creation_returns_same_singleton_instance() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let env = ModalEditorGroupEnv::new(&guard, EnvOptions::default());
        let parts = env.parts.clone();

        let (add_group_count, listener) = count(&parts.on_did_add_group());
        guard.add(listener);

        // Fire multiple concurrent creation requests before any of them has a chance to resolve and assign the singleton
        // instance
        let created = Promise::all(vec![parts.create_modal_editor_part(None), parts.create_modal_editor_part(None), parts.create_modal_editor_part(None)])
            .await
            .unwrap();
        let (modal_part1, modal_part2, modal_part3) = (created[0].clone(), created[1].clone(), created[2].clone());

        // All concurrent calls must resolve to the exact same singleton instance/group
        assert!(same_part(&modal_part1, &modal_part2));
        assert!(same_part(&modal_part2, &modal_part3));
        assert_eq!(modal_part1.active_group().id(), modal_part2.active_group().id());
        assert_eq!(modal_part1.active_group().id(), modal_part3.active_group().id());

        // Only a single group/registration event should have fired for the singleton, proving only one modal part was ever
        // created despite the concurrent calls
        assert_eq!(add_group_count.get(), 1);

        modal_part1.close_with_options(None).await.unwrap();
    });
}

#[test]
fn modal_editor_part_singleton_is_reset_after_close() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let env = ModalEditorGroupEnv::new(&guard, EnvOptions::default());
        let parts = env.parts.clone();

        // Create first modal
        let modal_part1 = parts.create_modal_editor_part(None).await.unwrap();
        let first_group_id = modal_part1.active_group().id();

        // Close it
        modal_part1.close_with_options(None).await.unwrap();

        // Create another modal - should be a new instance
        let modal_part2 = parts.create_modal_editor_part(None).await.unwrap();

        // Should be a different group
        assert_ne!(modal_part2.active_group().id(), first_group_id);

        modal_part2.close_with_options(None).await.unwrap();
    });
}

#[test]
fn modal_editor_part_on_did_add_group_fires_only_once_for_singleton() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let env = ModalEditorGroupEnv::new(&guard, EnvOptions::default());
        let parts = env.parts.clone();

        let (add_group_count, listener) = count(&parts.on_did_add_group());
        guard.add(listener);

        // Create modal twice
        parts.create_modal_editor_part(None).await.unwrap();
        parts.create_modal_editor_part(None).await.unwrap();

        // onDidAddGroup should fire only once since it's a singleton
        assert_eq!(add_group_count.get(), 1);

        parts.create_modal_editor_part(None).await.unwrap().close_with_options(None).await.unwrap();
    });
}

#[test]
fn modal_editor_part_enforces_no_tabs_mode() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let env = ModalEditorGroupEnv::new(&guard, EnvOptions::default());
        let parts = env.parts.clone();

        let modal_part = parts.create_modal_editor_part(None).await.unwrap();

        // Modal parts should enforce no tabs mode
        assert_eq!(modal_part.part_options().show_tabs, ShowTabs::None);

        modal_part.close_with_options(None).await.unwrap();
    });
}

#[test]
fn modal_editor_part_enforces_close_empty_groups() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let env = ModalEditorGroupEnv::new(&guard, EnvOptions::default());
        let parts = env.parts.clone();

        let modal_part = parts.create_modal_editor_part(None).await.unwrap();

        // Modal parts should enforce closeEmptyGroups
        assert!(modal_part.part_options().close_empty_groups);

        modal_part.close_with_options(None).await.unwrap();
    });
}

#[test]
fn closing_all_editors_in_modal_removes_the_modal_group() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let env = ModalEditorGroupEnv::new(&guard, EnvOptions::default());
        let parts = env.parts.clone();

        let modal_part = parts.create_modal_editor_part(None).await.unwrap();

        let input = input(&guard, "foo/bar");
        modal_part.active_group().open_editor(&editor(&input), Some(pinned())).await.unwrap();

        let modal_group_id = modal_part.active_group().id();

        // The modal group should exist in parts
        assert!(parts.get_group(modal_group_id).is_some());

        // Closing the last editor in the last group should close the modal which removes the group from parts
        modal_part.active_group().close_all_editors(None).await.unwrap();

        // The modal group should no longer exist in parts
        assert!(parts.get_group(modal_group_id).is_none());
    });
}

#[test]
fn modal_editor_part_does_not_persist_state() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let env = ModalEditorGroupEnv::new(&guard, EnvOptions::default());
        let parts = env.parts.clone();

        let modal_part = parts.create_modal_editor_part(None).await.unwrap();

        let input = input(&guard, "foo/bar");
        modal_part.active_group().open_editor(&editor(&input), Some(pinned())).await.unwrap();

        // Modal part should have saveState as a no-op (we can't directly test this, but we verify the modal was created
        // successfully which means state handling works)
        let _ = modal_part.active_group();

        modal_part.close_with_options(None).await.unwrap();
    });
}

#[test]
fn active_part_returns_modal_when_focused() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let env = ModalEditorGroupEnv::new(&guard, EnvOptions::default());
        let parts = env.parts.clone();

        let modal_part = parts.create_modal_editor_part(None).await.unwrap();

        let input = input(&guard, "foo/bar");
        modal_part.active_group().open_editor(&editor(&input), Some(pinned())).await.unwrap();

        // Focus the modal group
        modal_part.active_group().focus();

        // The modal group should be included in the groups
        let groups = parts.get_groups(GroupsOrder::MostRecentlyActive);
        assert!(groups.iter().any(|group| group.id() == modal_part.active_group().id()));

        modal_part.close_with_options(None).await.unwrap();
    });
}

#[test]
fn modal_part_group_can_be_found_by_id() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let env = ModalEditorGroupEnv::new(&guard, EnvOptions::default());
        let parts = env.parts.clone();

        let modal_part = parts.create_modal_editor_part(None).await.unwrap();

        let modal_group = modal_part.active_group();
        let found_group = parts.get_group(modal_group.id());

        assert!(found_group.is_some());
        assert_eq!(found_group.unwrap().id(), modal_group.id());

        modal_part.close_with_options(None).await.unwrap();
    });
}

#[test]
fn on_did_add_group_fires_when_modal_is_created() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let env = ModalEditorGroupEnv::new(&guard, EnvOptions::default());
        let parts = env.parts.clone();

        let added_group_id: Rc<Cell<Option<GroupIdentifier>>> = Rc::new(Cell::new(None));
        guard.add({
            let added_group_id = added_group_id.clone();
            parts.on_did_add_group().listen(move |group| added_group_id.set(Some(group.id())))
        });

        let modal_part = parts.create_modal_editor_part(None).await.unwrap();

        assert!(added_group_id.get().is_some());
        assert_eq!(added_group_id.get(), Some(modal_part.active_group().id()));

        modal_part.close_with_options(None).await.unwrap();
    });
}

#[test]
fn on_did_remove_group_fires_when_modal_is_closed() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let env = ModalEditorGroupEnv::new(&guard, EnvOptions::default());
        let parts = env.parts.clone();

        let modal_part = parts.create_modal_editor_part(None).await.unwrap();

        let modal_group_id = modal_part.active_group().id();

        let removed_group_id: Rc<Cell<Option<GroupIdentifier>>> = Rc::new(Cell::new(None));
        guard.add({
            let removed_group_id = removed_group_id.clone();
            parts.on_did_remove_group().listen(move |group| removed_group_id.set(Some(group.id())))
        });

        modal_part.close_with_options(None).await.unwrap();

        assert!(removed_group_id.get().is_some());
        assert_eq!(removed_group_id.get(), Some(modal_group_id));
    });
}

#[test]
fn active_modal_editor_part_is_set_when_modal_is_created_and_cleared_on_close() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let env = ModalEditorGroupEnv::new(&guard, EnvOptions::default());
        let parts = env.parts.clone();
        let modal_editor_visible = || matches!(editor_part_modal_visible_context().get_value(&*env.context_key_service), Dynamic::Bool(true));

        // No modal initially
        assert!(parts.active_modal_editor_part().is_none());
        assert!(!modal_editor_visible());

        // Create modal
        let modal_part = parts.create_modal_editor_part(None).await.unwrap();
        assert!(parts.active_modal_editor_part().is_some_and(|active| same_part(&active, &modal_part)));
        assert!(modal_editor_visible());

        // Close modal
        modal_part.close_with_options(None).await.unwrap();
        assert!(parts.active_modal_editor_part().is_none());
        assert!(!modal_editor_visible());
    });
}

#[test]
fn find_group_returns_main_part_group_when_modal_is_active_and_preferred_group_is_not_modal_group() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let env = ModalEditorGroupEnv::new(&guard, EnvOptions::default());
        let parts = env.parts.clone();

        let main_group = parts.main_part().active_group();

        // Create modal and open an editor in it
        let modal_part = parts.create_modal_editor_part(None).await.unwrap();
        let input = input(&guard, "foo/bar");
        modal_part.active_group().open_editor(&editor(&input), Some(pinned())).await.unwrap();

        // findGroup without MODAL_GROUP should return main part group, not modal group
        let new_input = self::input(&guard, "foo/baz");
        let (group, _) = resolved_group(env.find_group(untyped(&new_input), None)).await;

        assert_eq!(group.id(), main_group.id());
    });
}

#[test]
fn find_group_closes_modal_when_preferred_group_is_not_modal_group_and_preserve_focus_is_not_set() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let env = ModalEditorGroupEnv::new(&guard, EnvOptions::default());
        let parts = env.parts.clone();

        // Create modal
        let modal_part = parts.create_modal_editor_part(None).await.unwrap();
        let input = input(&guard, "foo/bar");
        modal_part.active_group().open_editor(&editor(&input), Some(pinned())).await.unwrap();

        assert!(parts.active_modal_editor_part().is_some());

        // findGroup without MODAL_GROUP and without preserveFocus should close the modal
        let new_input = self::input(&guard, "foo/baz");
        resolved_group(env.find_group(untyped(&new_input), None)).await;

        assert!(parts.active_modal_editor_part().is_none());
    });
}

#[test]
fn find_group_keeps_modal_open_when_preserve_focus_is_true() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let env = ModalEditorGroupEnv::new(&guard, EnvOptions::default());
        let parts = env.parts.clone();

        // Create modal
        let modal_part = parts.create_modal_editor_part(None).await.unwrap();
        let input = input(&guard, "foo/bar");
        modal_part.active_group().open_editor(&editor(&input), Some(pinned())).await.unwrap();

        assert!(parts.active_modal_editor_part().is_some());

        // findGroup with preserveFocus should keep the modal open
        let new_input = self::input(&guard, "foo/baz");
        let options = TextEditorOptions::from(EditorOptions { preserve_focus: Some(true), ..EditorOptions::default() });
        let editor_with_preserve_focus = FindGroupEditor::Untyped(UntypedEditorInput { options: Some(options), ..UntypedEditorInput::from_resource(new_input.resource.clone()) });
        resolved_group(env.find_group(editor_with_preserve_focus, None)).await;

        assert!(parts.active_modal_editor_part().is_some_and(|active| same_part(&active, &modal_part)));

        modal_part.close_with_options(None).await.unwrap();
    });
}

#[test]
fn modal_editor_part_starts_not_maximized() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let env = ModalEditorGroupEnv::new(&guard, EnvOptions::default());
        let parts = env.parts.clone();

        let modal_part = parts.create_modal_editor_part(None).await.unwrap();

        assert!(!modal_part.maximized());

        modal_part.close_with_options(None).await.unwrap();
    });
}

#[test]
fn modal_editor_part_toggle_maximized_toggles_state() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let env = ModalEditorGroupEnv::new(&guard, EnvOptions::default());
        let parts = env.parts.clone();

        let modal_part = parts.create_modal_editor_part(None).await.unwrap();

        assert!(!modal_part.maximized());

        modal_part.toggle_maximized();
        assert!(modal_part.maximized());

        modal_part.toggle_maximized();
        assert!(!modal_part.maximized());

        modal_part.close_with_options(None).await.unwrap();
    });
}

#[test]
fn modal_editor_part_fires_on_did_change_maximized() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let env = ModalEditorGroupEnv::new(&guard, EnvOptions::default());
        let parts = env.parts.clone();

        let modal_part = parts.create_modal_editor_part(None).await.unwrap();

        let events: Rc<RefCell<Vec<bool>>> = Rc::default();
        guard.add({
            let events = events.clone();
            modal_part.on_did_change_maximized().listen(move |maximized| events.borrow_mut().push(*maximized))
        });

        modal_part.toggle_maximized();
        modal_part.toggle_maximized();

        assert_eq!(*events.borrow(), vec![true, false]);

        modal_part.close_with_options(None).await.unwrap();
    });
}

#[test]
fn modal_editor_part_remembers_maximized_state_across_instances() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let env = ModalEditorGroupEnv::new(&guard, EnvOptions::default());
        let parts = env.parts.clone();

        // Open modal and maximize it
        let modal_part1 = parts.create_modal_editor_part(None).await.unwrap();
        modal_part1.toggle_maximized();
        assert!(modal_part1.maximized());
        modal_part1.close_with_options(None).await.unwrap();

        // Open a new modal - should remember maximized state
        let modal_part2 = parts.create_modal_editor_part(None).await.unwrap();
        assert!(modal_part2.maximized());
        modal_part2.close_with_options(None).await.unwrap();

        // Open another modal after un-maximizing
        let modal_part3 = parts.create_modal_editor_part(None).await.unwrap();
        assert!(modal_part3.maximized());
        modal_part3.toggle_maximized();
        assert!(!modal_part3.maximized());
        modal_part3.close_with_options(None).await.unwrap();

        // Should now remember non-maximized state
        let modal_part4 = parts.create_modal_editor_part(None).await.unwrap();
        assert!(!modal_part4.maximized());
        modal_part4.close_with_options(None).await.unwrap();
    });
}

mod use_modal_all {
    use super::*;

    #[test]
    fn find_group_creates_modal_and_returns_its_active_group() {
        run_with_faked_timers(async move {
            let guard = ensure_no_disposables_are_leaked_in_test_suite();
            let env = ModalEditorGroupEnv::new(&guard, EnvOptions { use_modal: Some("all"), ..EnvOptions::default() });
            let parts = env.parts.clone();

            // findGroup with undefined preferredGroup should create modal and return its group
            let input = input(&guard, "foo/bar");
            let result = env.find_group(untyped(&input), None);

            // Should return a promise (async modal creation)
            assert!(matches!(result, MaybePromise::Promise(_)));
            let (group, _) = resolved_group(result).await;

            // The group should be in the modal part
            let active_modal_editor_part = parts.active_modal_editor_part().unwrap();
            assert_eq!(group.id(), active_modal_editor_part.active_group().id());

            active_modal_editor_part.close_with_options(None).await.unwrap();
        });
    }

    #[test]
    fn find_group_does_not_auto_close_modal() {
        run_with_faked_timers(async move {
            let guard = ensure_no_disposables_are_leaked_in_test_suite();
            let env = ModalEditorGroupEnv::new(&guard, EnvOptions { use_modal: Some("all"), ..EnvOptions::default() });
            let parts = env.parts.clone();

            // Create modal first
            let modal_part = parts.create_modal_editor_part(None).await.unwrap();
            let input = input(&guard, "foo/bar");
            modal_part.active_group().open_editor(&editor(&input), Some(pinned())).await.unwrap();

            // findGroup without MODAL_GROUP should NOT close the modal
            let new_input = self::input(&guard, "foo/baz");
            let result = env.find_group(untyped(&new_input), None);

            // Since the setting is 'on', modal stays open
            let (group, _) = resolved_group(result).await;

            assert!(parts.active_modal_editor_part().is_some());
            assert_eq!(group.id(), modal_part.active_group().id());

            modal_part.close_with_options(None).await.unwrap();
        });
    }

    #[test]
    fn find_group_auto_closes_modal_when_setting_is_not_all() {
        run_with_faked_timers(async move {
            let guard = ensure_no_disposables_are_leaked_in_test_suite();
            let env = ModalEditorGroupEnv::new(&guard, EnvOptions { use_modal: Some("off"), ..EnvOptions::default() });
            let parts = env.parts.clone();

            // Create modal
            let modal_part = parts.create_modal_editor_part(None).await.unwrap();
            let input = input(&guard, "foo/bar");
            modal_part.active_group().open_editor(&editor(&input), Some(pinned())).await.unwrap();

            assert!(parts.active_modal_editor_part().is_some());

            // findGroup without MODAL_GROUP should close the modal
            let new_input = self::input(&guard, "foo/baz");
            resolved_group(env.find_group(untyped(&new_input), None)).await;

            assert!(parts.active_modal_editor_part().is_none());
        });
    }

    #[test]
    fn shows_tabs_when_multiple_editors_are_open() {
        run_with_faked_timers(async move {
            let guard = ensure_no_disposables_are_leaked_in_test_suite();
            let env = ModalEditorGroupEnv::new(&guard, EnvOptions { use_modal: Some("all"), ..EnvOptions::default() });
            let parts = env.parts.clone();

            let editor_service = env.use_editor_service(&guard);

            let input1 = input(&guard, "foo/bar");
            editor_service.open_editor_typed(&editor(&input1), Some(TextEditorOptions::from(pinned())), Some(PreferredGroup::Modal)).await.unwrap();

            let modal_part = parts.active_modal_editor_part().unwrap();

            // With 1 editor, tabs should be hidden
            assert_eq!(modal_part.part_options().show_tabs, ShowTabs::None);

            // Open a second editor
            let input2 = input(&guard, "foo/baz");
            editor_service.open_editor_typed(&editor(&input2), Some(TextEditorOptions::from(pinned())), Some(PreferredGroup::Modal)).await.unwrap();

            // With 2 editors, tabs should be visible
            assert_eq!(modal_part.part_options().show_tabs, ShowTabs::Multiple);

            modal_part.close_with_options(None).await.unwrap();
        });
    }

    #[test]
    fn hides_tabs_when_not_in_all_mode_even_with_multiple_editors() {
        run_with_faked_timers(async move {
            let guard = ensure_no_disposables_are_leaked_in_test_suite();
            let env = ModalEditorGroupEnv::new(&guard, EnvOptions { use_modal: Some("some"), ..EnvOptions::default() });
            let parts = env.parts.clone();

            let editor_service = env.use_editor_service(&guard);

            let input1 = input(&guard, "foo/bar");
            editor_service.open_editor_typed(&editor(&input1), Some(TextEditorOptions::from(pinned())), Some(PreferredGroup::Modal)).await.unwrap();

            let modal_part = parts.active_modal_editor_part().unwrap();

            let input2 = input(&guard, "foo/baz");
            editor_service.open_editor_typed(&editor(&input2), Some(TextEditorOptions::from(pinned())), Some(PreferredGroup::Modal)).await.unwrap();

            // With 'some' mode, tabs should remain hidden even with multiple editors
            assert_eq!(modal_part.part_options().show_tabs, ShowTabs::None);

            modal_part.close_with_options(None).await.unwrap();
        });
    }
}

#[test]
fn modal_editor_part_editors_can_be_moved_to_another_group() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let env = ModalEditorGroupEnv::new(&guard, EnvOptions::default());
        let parts = env.parts.clone();

        // Create modal and open editors
        let modal_part = parts.create_modal_editor_part(None).await.unwrap();
        let input1 = input(&guard, "foo/bar");
        let input2 = input(&guard, "foo/baz");
        modal_part.active_group().open_editor(&editor(&input1), Some(pinned())).await.unwrap();
        modal_part.active_group().open_editor(&editor(&input2), Some(pinned())).await.unwrap();

        assert_eq!(modal_part.active_group().count(), 2);

        // Move editors from modal to main part group
        let target_group = parts.main_part().active_group();
        for group in modal_part.get_groups(GroupsOrder::MostRecentlyActive) {
            let editors = group
                .editors()
                .into_iter()
                .map(|editor| EditorInputWithOptions {
                    editor,
                    options: Some(TextEditorOptions::from(EditorOptions { preserve_focus: Some(true), ..EditorOptions::default() })),
                })
                .collect();
            group.move_editors(editors, &target_group);
        }

        // Editors should be in the target group now
        assert_eq!(target_group.count(), 2);
        assert_eq!(modal_part.active_group().count(), 0);

        // Close modal
        modal_part.close_with_options(None).await.unwrap();
        assert!(parts.active_modal_editor_part().is_none());
    });
}

#[test]
fn modal_editor_part_state_is_remembered_on_close_and_reused_on_next_open() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let env = ModalEditorGroupEnv::new(&guard, EnvOptions::default());
        let parts = env.parts.clone();

        // Create maximized modal and close it
        let modal_part1 = parts.create_modal_editor_part(Some(ModalEditorPartOptions { maximized: Some(true), ..ModalEditorPartOptions::default() })).await.unwrap();
        modal_part1.close_with_options(None).await.unwrap();

        // Create a new modal - it should restore maximized state
        let modal_part2 = parts.create_modal_editor_part(None).await.unwrap();
        assert!(modal_part2.maximized());

        modal_part2.close_with_options(None).await.unwrap();
    });
}

#[test]
fn modal_editor_part_state_restores_from_profile_storage() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();

        // Pre-populate storage with modal state so the next EditorParts instance reads fresh from storage
        let env = ModalEditorGroupEnv::new(
            &guard,
            EnvOptions {
                profile_state: Some(
                    r#"{"editorparts.modalState":{"maximized":true,"size":{"width":500,"height":400},"position":{"left":100,"top":50}}}"#,
                ),
                ..EnvOptions::default()
            },
        );
        let parts = env.parts.clone();

        // Create modal - it should use state from storage
        let modal_part = parts.create_modal_editor_part(None).await.unwrap();
        assert!(modal_part.maximized());

        modal_part.close_with_options(None).await.unwrap();
    });
}

mod requires_modal_capability {
    use super::*;

    #[test]
    fn find_group_respects_use_modal_off_for_editor_with_requires_modal() {
        run_with_faked_timers(async move {
            let guard = ensure_no_disposables_are_leaked_in_test_suite();
            let env = ModalEditorGroupEnv::new(&guard, EnvOptions { use_modal: Some("off"), ..EnvOptions::default() });
            let parts = env.parts.clone();

            let input = input(&guard, "foo/bar");
            input.set_capabilities(EditorInputCapabilities::REQUIRES_MODAL);

            let result = env.find_group(with_empty_options(&input), None);

            let (group, _) = resolved_group(result).await;

            // With useModal: off, the user preference wins and no modal part is created
            assert!(parts.active_modal_editor_part().is_none());
            assert_eq!(group.id(), parts.main_part().active_group().id());
        });
    }

    #[test]
    fn find_group_opens_modal_for_editor_with_requires_modal_when_use_modal_is_some() {
        run_with_faked_timers(async move {
            let guard = ensure_no_disposables_are_leaked_in_test_suite();
            let env = ModalEditorGroupEnv::new(&guard, EnvOptions { use_modal: Some("some"), ..EnvOptions::default() });
            let parts = env.parts.clone();

            let input = input(&guard, "foo/bar");
            input.set_capabilities(EditorInputCapabilities::REQUIRES_MODAL);

            let result = env.find_group(with_empty_options(&input), None);

            assert!(matches!(result, MaybePromise::Promise(_)));
            let (group, _) = resolved_group(result).await;

            let active_modal_editor_part = parts.active_modal_editor_part().unwrap();
            assert_eq!(group.id(), active_modal_editor_part.active_group().id());

            active_modal_editor_part.close_with_options(None).await.unwrap();
        });
    }

    #[test]
    fn find_group_does_not_close_modal_for_requires_modal_editor_when_modal_is_already_open() {
        run_with_faked_timers(async move {
            let guard = ensure_no_disposables_are_leaked_in_test_suite();
            let env = ModalEditorGroupEnv::new(&guard, EnvOptions { use_modal: Some("some"), ..EnvOptions::default() });
            let parts = env.parts.clone();

            // Create a modal part first
            let modal_part = parts.create_modal_editor_part(None).await.unwrap();
            let existing_input = input(&guard, "foo/existing");
            modal_part.active_group().open_editor(&editor(&existing_input), Some(pinned())).await.unwrap();

            // Now open a RequiresModal editor - modal should stay open
            let input = input(&guard, "foo/bar");
            input.set_capabilities(EditorInputCapabilities::REQUIRES_MODAL);

            let result = env.find_group(with_empty_options(&input), None);

            assert!(matches!(result, MaybePromise::Promise(_)));
            let (group, _) = resolved_group(result).await;

            assert!(parts.active_modal_editor_part().is_some_and(|active| same_part(&active, &modal_part)));
            assert_eq!(group.id(), modal_part.active_group().id());

            modal_part.close_with_options(None).await.unwrap();
        });
    }
}
