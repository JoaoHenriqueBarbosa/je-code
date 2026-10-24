//! Porte de `src/vs/workbench/services/editor/test/browser/editorGroupsService.test.ts` (suíte `EditorGroupsService`), caso a caso e na mesma ordem.
//!
//! Fatias 2 a 4 do `PORT_PLAN.md` (o `EditorPart` lógico, o `EditorGroupView` completo e o `EditorParts`): entram os casos de
//! grupos, todos os de editor (abrir, fechar, mover, copiar, substituir, aderentes, transientes, seleção e bloqueio), o estado
//! salvo, os conjuntos de trabalho (`applyState`) e o provedor de chave de contexto. Ainda NÃO portado (o placar conta só os
//! `#[test]`):
//! - `closeAllGroups action - cannot close editor handling`: usa a `CloseAllEditorGroupsAction` de `editorActions.ts`, que
//!   não foi portada. A ação lê o `IEditorService.getEditors(order, { excludeSticky })`, o `IEditorService.revert` e o
//!   `IEditorService.save` com razão, o `IProgressService` e o `ILogService`, dos quais o porte só tem o `getEditors(order)`
//!   (sem `excludeSticky`) e o `save`; o `TestEditorService` do porte é uma casca que não lista editores. Portar a ação
//!   exige primeiro o `EditorService` real e o `IProgressService`.
//!
//! Notas de tradução:
//! - **`createParts` e `createPart`.** `workbenchInstantiationService` com o `MockScopableContextKeyService` é um
//!   `InstantiationService` com o `TestConfigurationService`, um `InMemoryStorageService` e o
//!   `MockContextKeyService::scopable()` (o serviço raiz, [`TestParts::root_context_key_service`]); [`create_parts`] inicia
//!   o registro de serializadores (`EditorFactoryRegistry.start`), cria o `TestEditorParts` (`createEditorParts`: a parte
//!   principal com `create` e `layout(1080, 800, 0, 0)`, esperando a restauração das partes) e registra o `EditorParts` como
//!   `IEditorGroupsService`. [`create_test_part`] acrescenta o que o `workbenchInstantiationService` entrega e o grupo
//!   lê: o `TestFileDialogService` (`setConfirmResult`), o `TestDialogService`, o `IFilesConfigurationService` e o
//!   `IUriIdentityService` (de `editor_input_services`). `createPart` devolve a `TestEditorPart` (`parts.testMainPart`).
//! - **`registerTestEditor`.** O `setup` do original registra o painel `TEST_EDITOR_ID` para `TestFileEditorInput` e
//!   `SideBySideEditorInput` (a classe da entrada é o `TypeId`) e o serializador de `TEST_EDITOR_INPUT_ID`: é o
//!   `register_test_editor` de `workbench_test_services`, chamado uma vez por caso em [`register_test_editor_for_suite`].
//! - **Funções `async`.** Os casos `async` rodam em `run_with_faked_timers` (o `await` custa uma rodada de microtarefas como
//!   no motor JS), com o guarda criado dentro do corpo. Promessas que o original solta sem `await` (`onWillOpenEditor`) são
//!   esperadas no fim, para o corpo não terminar com tarefas pendentes.
//! - **Eventos e grupos.** `part.groups[0]` é `part.groups()[0]`; `assert.strictEqual(a, b)` entre grupos é `Rc::ptr_eq`
//!   ([`same`]); `{ group }` onde o original passa um `IEditorGroup | GroupIdentifier` é [`group_ref`]; entre editores é a
//!   identidade do ponteiro ([`is_editor`]).
//! - **`ensureNoDisposablesAreLeakedInTestSuite`** é o `LeakGuard` (o original o importa mas esta suíte não o chama: o guarda
//!   aqui é mais estrito); o `disposables` do original é o próprio guarda.
//! - **`assert.strictEqual(oldOptions, currentOptions)`** compara por valor (`EditorPartOptions` é `PartialEq`).
//! - **`URI.file('foo/bar')`** é `uri_file("foo/bar")`; `{ pinned: true }` e companhia são o `EditorOptions` com os campos
//!   dados; `openEditor(editor)` sem opções é `None`.
//! - **`save & restore state`.** `part.dispose()` descarta só a parte principal (as partes ficam, como no original), e a
//!   segunda `createPart(instantiationService)` é [`create_parts`] sobre o mesmo serviço de instanciação. As entradas que o
//!   serializador de teste recria ficam num store do `registerTestEditor` (o guarda de vazamentos daqui as exige descartadas).
//! - **`working sets`.** `part.applyState(state)` recebe [`ApplyStateTarget`] (`'empty'` é `Empty`); `pane?.group` é a busca do
//!   grupo pelo `group_id` do painel; `input3.dirty = true` é `dirty.set(true)`; a atribuição por `cast` de
//!   `_contentDimension = undefined` é `TestEditorPart::reset_content_dimension`.
//! - **`context key provider`.** O provedor (`IEditorGroupContextKeyProvider<number>`) devolve [`Dynamic`]; o contexto raiz
//!   é o `MockContextKeyService` que as partes recebem (`rootContextKeyService`), e `getContextKeyValue` é comparado com
//!   `strict_equals` (`assert.strictEqual`).

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod editor_input_services;

use std::any::TypeId;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use editor_input_services::{WorkbenchServices, uri_identity_service};
use je_base::dynamic::Dynamic;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{DisposableRef, IDisposable, id_of};
use je_base::promise::Promise;
use je_instantiation::{InstantiationService, ServiceCollection};
use je_platform::configuration::service::{I_CONFIGURATION_SERVICE, IConfigurationService};
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::contextkey::contextkey::{ContextKeyMeta, I_CONTEXT_KEY_SERVICE, IContextKeyService, RawContextKey};
use je_platform::dialogs::{
    ConfirmResult, Confirmation, ConfirmationResult, FileNameOrResource, I_DIALOG_SERVICE, I_FILE_DIALOG_SERVICE, IDialogService, IFileDialogService, Input,
    InputResult, OpenDialogOptions, PickAndOpenOptions, Prompt, PromptResult, SaveDialogOptions,
};
use je_platform::keybinding::mock_keybinding_service::MockContextKeyService;
use je_platform::opener::editor_types::EditorOptions;
use je_platform::resources::{ext_uri, uri_file};
use je_platform::storage::{I_STORAGE_SERVICE, InMemoryStorageService, StorageService};
use je_platform::uri_identity::uri_identity::I_URI_IDENTITY_SERVICE;
use je_ui::Ui;
use je_workbench::browser::parts::editor::editor_part::ApplyStateTarget;
use je_workbench::common::editor::editor_factory_registry::editor_factory_registry;
use je_workbench::common::editor::editor_group_model::{EditorOrIndex, GetEditorsOptions, GroupModelChangeEvent};
use je_workbench::common::editor::side_by_side_editor_input::SideBySideEditorInput;
use je_workbench::common::editor::{
    CloseDirection, EditorInputCapabilities, EditorInputOrUntyped, EditorInputRef, EditorInputWithOptions, EditorsOrder, FindEditorOptions, GroupModelChangeKind,
    SideBySideEditor, TextEditorOptions,
};
use je_workbench::services::editor::editor_groups_service::{
    CloseAllEditorsOptions, CloseEditorOptions, CloseEditorsFilter, CloseEditorsTarget, EditorGroupActivationEvent, EditorGroupContextKeyProvider,
    EditorGroupLayout, EditorReplacement, FindGroupScope, GroupActivationReason, GroupDirection, GroupLayoutArgument, GroupLocation,
    GroupOrientation, GroupRef, GroupsArrangement, GroupsOrder, I_EDITOR_GROUPS_SERVICE, IEditorGroup, IEditorGroupsService, IEditorPart, MergeGroupMode,
    MergeGroupOptions, ShowTabs, is_editor_group,
};
use je_workbench::services::editor::editor_service::IEditorService;
use je_workbench::services::files_configuration::files_configuration_service::{I_FILES_CONFIGURATION_SERVICE, IFilesConfigurationService};
use je_workbench::test::browser::workbench_test_services::{
    TestEditorParts, TestEditorPart, TestEditorService, TestFileEditorInput, create_editor_parts, register_test_editor,
};

const TEST_EDITOR_ID: &str = "MyFileEditorForEditorGroupService";
const TEST_EDITOR_INPUT_ID: &str = "testEditorInputForEditorGroupService";

/// `TestFileDialogService`: só o `showSaveConfirm` com o resultado que o caso escolhe (`setConfirmResult`).
struct TestFileDialogService {
    confirm_result: Cell<ConfirmResult>,
}

impl TestFileDialogService {
    fn new() -> Rc<Self> {
        Rc::new(Self { confirm_result: Cell::new(ConfirmResult::DontSave) })
    }

    /// `setConfirmResult(result)`.
    fn set_confirm_result(&self, result: ConfirmResult) {
        self.confirm_result.set(result);
    }
}

impl IFileDialogService for TestFileDialogService {
    fn default_file_path(&self, _scheme_filter: Option<&str>) -> Promise<je_base::uri::URI> {
        panic!("Method not implemented.")
    }

    fn default_folder_path(&self, _scheme_filter: Option<&str>) -> Promise<je_base::uri::URI> {
        panic!("Method not implemented.")
    }

    fn default_workspace_path(&self, _scheme_filter: Option<&str>) -> Promise<je_base::uri::URI> {
        panic!("Method not implemented.")
    }

    fn pick_file_folder_and_open(&self, _options: PickAndOpenOptions) -> Promise<()> {
        panic!("Method not implemented.")
    }

    fn pick_file_and_open(&self, _options: PickAndOpenOptions) -> Promise<()> {
        panic!("Method not implemented.")
    }

    fn pick_folder_and_open(&self, _options: PickAndOpenOptions) -> Promise<()> {
        panic!("Method not implemented.")
    }

    fn pick_workspace_and_open(&self, _options: PickAndOpenOptions) -> Promise<()> {
        panic!("Method not implemented.")
    }

    fn pick_file_to_save(&self, _default_uri: &je_base::uri::URI, _available_file_systems: Option<Vec<String>>) -> Promise<Option<je_base::uri::URI>> {
        panic!("Method not implemented.")
    }

    fn preferred_home(&self, _scheme_filter: Option<&str>) -> Promise<je_base::uri::URI> {
        panic!("Method not implemented.")
    }

    fn show_save_dialog(&self, _options: SaveDialogOptions) -> Promise<Option<je_base::uri::URI>> {
        panic!("Method not implemented.")
    }

    fn show_save_confirm(&self, _file_names_or_resources: &[FileNameOrResource]) -> Promise<ConfirmResult> {
        Promise::resolved(self.confirm_result.get())
    }

    fn show_open_dialog(&self, _options: OpenDialogOptions) -> Promise<Option<Vec<je_base::uri::URI>>> {
        panic!("Method not implemented.")
    }
}

/// `TestDialogService` do `workbenchInstantiationService`: `info`, `warn` e `error` resolvem sem mostrar nada; os demais
/// não são exercitados por esta suíte.
struct TestDialogService;

impl IDialogService for TestDialogService {
    fn on_will_show_dialog(&self) -> Event<()> {
        Event::none()
    }

    fn on_did_show_dialog(&self) -> Event<()> {
        Event::none()
    }

    fn confirm(&self, _confirmation: Confirmation) -> Promise<ConfirmationResult> {
        unreachable!("TestDialogService.confirm não é exercitado por esta suíte")
    }

    fn prompt(&self, _prompt: Prompt) -> Promise<PromptResult> {
        unreachable!("TestDialogService.prompt não é exercitado por esta suíte")
    }

    fn input(&self, _input: Input) -> Promise<InputResult> {
        unreachable!("TestDialogService.input não é exercitado por esta suíte")
    }

    fn info(&self, _message: &str, _detail: Option<&str>) -> Promise<()> {
        Promise::resolved(())
    }

    fn warn(&self, _message: &str, _detail: Option<&str>) -> Promise<()> {
        Promise::resolved(())
    }

    fn error(&self, _message: &str, _detail: Option<&str>) -> Promise<()> {
        Promise::resolved(())
    }

    fn about(&self) -> Promise<()> {
        unreachable!("TestDialogService.about não é exercitado por esta suíte")
    }
}

/// O que o `workbenchInstantiationService` entrega aos casos de editor além do serviço de instanciação: o
/// `TestFileDialogService` (`setConfirmResult`) e o `TestDialogService`, o `IFilesConfigurationService` e o
/// `IUriIdentityService`, num `ServiceCollection` e o diálogo de arquivo que o caso controla.
fn editor_workbench_services(guard: &LeakGuard, configuration: &Rc<TestConfigurationService>) -> (ServiceCollection, Rc<TestFileDialogService>) {
    let workbench_services = WorkbenchServices::new(guard, configuration);
    let file_dialog_service = TestFileDialogService::new();
    let file_dialog: Rc<dyn IFileDialogService> = file_dialog_service.clone();
    let dialog_service: Rc<dyn IDialogService> = Rc::new(TestDialogService);
    let files_configuration_service: Rc<dyn IFilesConfigurationService> = workbench_services.files_configuration_service.clone();

    let services = ServiceCollection::new()
        .with(I_FILE_DIALOG_SERVICE, file_dialog)
        .with(I_DIALOG_SERVICE, dialog_service)
        .with(I_FILES_CONFIGURATION_SERVICE, files_configuration_service)
        .with(I_URI_IDENTITY_SERVICE, uri_identity_service());

    (services, file_dialog_service)
}

/// O que `createPart` devolve (o `[part, instantiationService]` do original) e o que os casos usam do
/// `TestServiceAccessor`.
struct TestParts {
    /// `parts`.
    parts: Rc<TestEditorParts>,
    /// `parts.testMainPart`.
    part: Rc<TestEditorPart>,
    /// `instantiationService`.
    instantiation_service: Rc<InstantiationService>,
    /// O `ServiceCollection` do `instantiationService` (o `stub` do original muda o serviço dentro dele).
    services: ServiceCollection,
    /// O `IContextKeyService` raiz (`instantiationService.get(IContextKeyService)`).
    root_context_key_service: Rc<MockContextKeyService>,
    /// `accessor.fileDialogService`.
    file_dialog_service: Rc<TestFileDialogService>,
    /// `accessor.editorService`.
    editor_service: Rc<dyn IEditorService>,
}

/// `createParts(instantiationService)`: inicia o registro de serializadores (`EditorFactoryRegistry.start`), cria as partes
/// (`createEditorParts`, com `create` e `layout(1080, 800, 0, 0)`) e registra o serviço de grupos.
fn create_parts(guard: &LeakGuard, instantiation_service: &Rc<InstantiationService>, services: &ServiceCollection) -> Rc<TestEditorParts> {
    instantiation_service.invoke_function(|accessor| editor_factory_registry().start(accessor));
    let parts = guard.add_rc(create_editor_parts(&Ui::new(), instantiation_service));

    // `instantiationService.stub(IEditorGroupsService, parts)`
    services.set(I_EDITOR_GROUPS_SERVICE, parts.groups_service());

    parts
}

/// `workbenchInstantiationService`: `services` é o que o ambiente já tinha posto; a configuração, o armazenamento e o
/// `IContextKeyService` (o `MockScopableContextKeyService`) entram aqui.
fn create_test_parts_with(
    guard: &LeakGuard,
    configuration: Rc<dyn IConfigurationService>,
    services: ServiceCollection,
    file_dialog_service: Rc<TestFileDialogService>,
) -> TestParts {
    let storage = InMemoryStorageService::new();
    let storage_service: Rc<dyn StorageService> = Rc::new(storage.clone());
    let root_context_key_service = MockContextKeyService::scopable();
    let context_key_service: Rc<dyn IContextKeyService> = root_context_key_service.clone();

    let services = services.with(I_CONFIGURATION_SERVICE, configuration).with(I_STORAGE_SERVICE, storage_service).with(I_CONTEXT_KEY_SERVICE, context_key_service);
    let instantiation_service = InstantiationService::new(services.clone());
    guard.add_rc(storage);
    guard.add_rc(instantiation_service.clone());

    let parts = create_parts(guard, &instantiation_service, &services);
    let part = parts.test_main_part.clone();
    let editor_service: Rc<dyn IEditorService> = Rc::new(TestEditorService);

    TestParts { parts, part, instantiation_service, services, root_context_key_service, file_dialog_service, editor_service }
}

/// O `setup` da suíte: `registerTestEditor(TEST_EDITOR_ID, [TestFileEditorInput, SideBySideEditorInput], TEST_EDITOR_INPUT_ID)`.
fn register_test_editor_for_suite(guard: &LeakGuard) {
    guard.add(register_test_editor(
        TEST_EDITOR_ID,
        vec![TypeId::of::<TestFileEditorInput>(), TypeId::of::<SideBySideEditorInput>()],
        Some(TEST_EDITOR_INPUT_ID),
    ));
}

/// `createPart()` para os casos de editor: o `setup` registra o painel e o serializador de teste, e o
/// `workbenchInstantiationService` entrega os serviços que o grupo lê. `configuration` é o `TestConfigurationService` que o
/// caso preparou (`setUserConfiguration`).
fn create_test_part(guard: &LeakGuard, configuration: Rc<TestConfigurationService>) -> TestParts {
    register_test_editor_for_suite(guard);

    let (services, file_dialog_service) = editor_workbench_services(guard, &configuration);

    create_test_parts_with(guard, configuration, services, file_dialog_service)
}

/// `createPart()` para os casos que só leem os grupos: o mesmo, com a configuração padrão.
fn create_part(guard: &LeakGuard) -> Rc<TestEditorPart> {
    create_test_part(guard, TestConfigurationService::new(None)).part
}

/// `createTestFileEditorInput(resource, typeId)`.
fn create_test_file_editor_input(guard: &LeakGuard, resource: &str, type_id: &str) -> Rc<TestFileEditorInput> {
    guard.add_rc(TestFileEditorInput::new(uri_file(resource), type_id))
}

/// `createTestFileEditorInput(URI.file(resource), TEST_EDITOR_INPUT_ID)`.
fn input(guard: &LeakGuard, resource: &str) -> Rc<TestFileEditorInput> {
    create_test_file_editor_input(guard, resource, TEST_EDITOR_INPUT_ID)
}

/// O mesmo que [`input`], para os casos em que uma variável `input` sombreia a função.
fn input_with_path(guard: &LeakGuard, resource: &str) -> Rc<TestFileEditorInput> {
    input(guard, resource)
}

/// `createCannotCloseTestFileEditorInput(resource, typeId)`.
fn create_cannot_close_test_file_editor_input(guard: &LeakGuard, resource: &str, type_id: &str) -> Rc<TestFileEditorInput> {
    let input = create_test_file_editor_input(guard, resource, type_id);
    input.set_capabilities(EditorInputCapabilities::CANNOT_CLOSE);

    input
}

/// A entrada de teste como o handle de entrada que o grupo recebe.
fn editor(input: &Rc<TestFileEditorInput>) -> EditorInputRef {
    input.clone()
}

/// `assert.strictEqual(actual, expected)` entre editores: a mesma entrada.
fn is_editor<T: ?Sized>(actual: Option<EditorInputRef>, expected: &Rc<T>) -> bool {
    actual.is_some_and(|actual| id_of(&actual) == id_of(expected))
}

/// `assert.deepStrictEqual(group.getEditors(SEQUENTIAL), [...])`: as mesmas entradas, na ordem.
fn editors_are(actual: &[EditorInputRef], expected: &[&Rc<TestFileEditorInput>]) -> bool {
    actual.len() == expected.len() && actual.iter().zip(expected).all(|(actual, expected)| id_of(actual) == id_of(*expected))
}

/// `EditorInput` como `EditorInput | IUntypedEditorInput`.
fn typed(input: &Rc<TestFileEditorInput>) -> EditorInputOrUntyped {
    EditorInputOrUntyped::Typed(input.clone())
}

/// `{ pinned: true }`.
fn pinned() -> EditorOptions {
    EditorOptions { pinned: Some(true), ..EditorOptions::default() }
}

/// `{ editor, options }`.
fn with_options(input: &Rc<TestFileEditorInput>, options: Option<EditorOptions>) -> EditorInputWithOptions {
    EditorInputWithOptions { editor: editor(input), options: options.map(TextEditorOptions::from) }
}

/// `{ index }`.
fn at_index(index: usize) -> EditorOptions {
    EditorOptions { index: Some(index as f64), ..EditorOptions::default() }
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
fn side_group() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let part = create_test_part(&guard, TestConfigurationService::new(None)).part;

        let root_group = part.active_group();

        let input1 = input(&guard, "foo/bar1");
        let input2 = input(&guard, "foo/bar2");
        let input3 = input(&guard, "foo/bar3");

        root_group.open_editor(&editor(&input1), Some(pinned())).await.unwrap();
        part.side_group().open_editor(&editor(&input2), Some(pinned())).await.unwrap();
        assert_eq!(part.count(), 2);

        part.activate_group(group_ref(&root_group));
        part.side_group().open_editor(&editor(&input3), Some(pinned())).await.unwrap();
        assert_eq!(part.count(), 2);
    });
}

#[test]
fn save_and_restore_state() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let test_parts = create_test_part(&guard, TestConfigurationService::new(None));
        let part = test_parts.part.clone();

        let root_group = part.groups()[0].clone();
        let right_group = part.add_group(Some(group_ref(&root_group)), GroupDirection::Right);
        let down_group = part.add_group(Some(group_ref(&right_group)), GroupDirection::Down);

        let root_group_input = input(&guard, "foo/bar1");
        root_group.open_editor(&editor(&root_group_input), Some(pinned())).await.unwrap();

        let right_group_input = input(&guard, "foo/bar2");
        right_group.open_editor(&editor(&right_group_input), Some(pinned())).await.unwrap();

        assert_eq!(part.groups().len(), 3);

        part.test_save_state();
        part.dispose();

        // `createPart(instantiationService)`: as partes novas sobre o mesmo serviço de instanciação
        let restored_part = create_parts(&guard, &test_parts.instantiation_service, &test_parts.services).test_main_part.clone();

        assert_eq!(restored_part.groups().len(), 3);
        assert!(restored_part.get_group(root_group.id()).is_some());
        assert!(restored_part.has_group(root_group.id()));
        assert!(restored_part.get_group(right_group.id()).is_some());
        assert!(restored_part.has_group(right_group.id()));
        assert!(restored_part.get_group(down_group.id()).is_some());
        assert!(restored_part.has_group(down_group.id()));

        restored_part.clear_state();
    });
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
fn copy_merge_groups() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let part = create_test_part(&guard, TestConfigurationService::new(None)).part;

        let (group_added_counter, group_added_listener) = count(&part.on_did_add_group());
        let (group_removed_counter, group_removed_listener) = count(&part.on_did_remove_group());

        let root_group = part.groups()[0].clone();
        let root_group_disposed = Rc::new(Cell::new(false));
        let dispose_listener = {
            let root_group_disposed = root_group_disposed.clone();
            root_group.on_will_dispose().listen(move |_| root_group_disposed.set(true))
        };

        let input = input(&guard, "foo/bar");

        root_group.open_editor(&editor(&input), Some(pinned())).await.unwrap();
        let right_group = part.add_group(Some(group_ref(&root_group)), GroupDirection::Right);
        part.activate_group(group_ref(&right_group));
        let down_group = part.copy_group(group_ref(&root_group), group_ref(&right_group), GroupDirection::Down);
        assert_eq!(group_added_counter.get(), 2);
        assert_eq!(down_group.count(), 1);
        assert!(down_group.active_editor().is_some_and(|active_editor| active_editor.as_any().is::<TestFileEditorInput>()));
        let mut res = part.merge_group(group_ref(&root_group), group_ref(&right_group), Some(MergeGroupOptions { mode: Some(MergeGroupMode::CopyEditors), ..MergeGroupOptions::default() }));
        assert!(res);
        assert_eq!(right_group.count(), 1);
        assert!(right_group.active_editor().is_some_and(|active_editor| active_editor.as_any().is::<TestFileEditorInput>()));
        res = part.merge_group(group_ref(&root_group), group_ref(&right_group), Some(MergeGroupOptions { mode: Some(MergeGroupMode::MoveEditors), ..MergeGroupOptions::default() }));
        assert!(res);
        assert_eq!(root_group.count(), 0);
        res = part.merge_group(group_ref(&root_group), group_ref(&down_group), None);
        assert!(res);
        assert_eq!(group_removed_counter.get(), 1);
        assert!(root_group_disposed.get());

        group_added_listener.dispose();
        group_removed_listener.dispose();
        dispose_listener.dispose();
        part.dispose();
    });
}

#[test]
fn merge_all_groups() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let part = create_test_part(&guard, TestConfigurationService::new(None)).part;

        let root_group = part.groups()[0].clone();

        let input1 = input(&guard, "foo/bar1");
        let input2 = input(&guard, "foo/bar2");
        let input3 = input(&guard, "foo/bar3");

        root_group.open_editor(&editor(&input1), Some(pinned())).await.unwrap();

        let right_group = part.add_group(Some(group_ref(&root_group)), GroupDirection::Right);
        right_group.open_editor(&editor(&input2), Some(pinned())).await.unwrap();

        let down_group = part.copy_group(group_ref(&root_group), group_ref(&right_group), GroupDirection::Down);
        down_group.open_editor(&editor(&input3), Some(pinned())).await.unwrap();

        part.activate_group(group_ref(&root_group));

        assert_eq!(root_group.count(), 1);

        let result = part.merge_all_groups(group_ref(&part.active_group()));
        assert!(result);
        assert_eq!(root_group.count(), 3);

        part.dispose();
    });
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

/// `counter++`.
fn bump(counter: &Cell<usize>) {
    counter.set(counter.get() + 1);
}

/// `createPart()` com a configuração padrão do `workbenchInstantiationService`.
fn part_of(guard: &LeakGuard) -> Rc<TestEditorPart> {
    create_test_part(guard, TestConfigurationService::new(None)).part
}

/// `{ editor, replacement, forceReplaceDirty }`.
fn replacement(editor_to_replace: &Rc<TestFileEditorInput>, replacement: &Rc<TestFileEditorInput>, force_replace_dirty: Option<bool>) -> EditorReplacement {
    EditorReplacement { editor: editor(editor_to_replace), replacement: editor(replacement), options: None, force_replace_dirty }
}

/// `{ except }` e as demais opções de `ICloseEditorsFilter`.
fn close_filter(except: Option<&Rc<TestFileEditorInput>>, direction: Option<CloseDirection>, exclude_sticky: bool, saved_only: bool) -> CloseEditorsTarget {
    CloseEditorsTarget::Filter(CloseEditorsFilter {
        except: except.map(editor),
        direction,
        saved_only: saved_only.then_some(true),
        exclude_sticky: exclude_sticky.then_some(true),
    })
}

/// `[input...]`: o primeiro argumento de `closeEditors(editors)`.
fn close_list(inputs: &[&Rc<TestFileEditorInput>]) -> CloseEditorsTarget {
    CloseEditorsTarget::Editors(inputs.iter().map(|input| editor(input)).collect())
}

#[test]
fn editor_basics() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let part = part_of(&guard);
        let group = part.active_group();
        assert!(group.is_empty());

        let active_editor_change_counter = Rc::new(Cell::new(0));
        let editor_did_open_counter = Rc::new(Cell::new(0));
        let editor_open_events: Rc<RefCell<Vec<GroupModelChangeEvent>>> = Rc::default();
        let editor_close_counter = Rc::new(Cell::new(0));
        let editor_close_events: Rc<RefCell<Vec<GroupModelChangeEvent>>> = Rc::default();
        let editor_pin_counter = Rc::new(Cell::new(0));
        let editor_sticky_counter = Rc::new(Cell::new(0));
        let editor_capabilities_counter = Rc::new(Cell::new(0));
        guard.add({
            let (editor_did_open_counter, editor_open_events, editor_close_counter, editor_close_events) =
                (editor_did_open_counter.clone(), editor_open_events.clone(), editor_close_counter.clone(), editor_close_events.clone());
            let (editor_pin_counter, editor_sticky_counter, editor_capabilities_counter) =
                (editor_pin_counter.clone(), editor_sticky_counter.clone(), editor_capabilities_counter.clone());
            group.on_did_model_change().listen(move |e| match e.kind {
                GroupModelChangeKind::EditorOpen => {
                    assert!(e.editor.is_some());
                    bump(&editor_did_open_counter);
                    editor_open_events.borrow_mut().push(e.clone());
                }
                GroupModelChangeKind::EditorPin => {
                    assert!(e.editor.is_some());
                    bump(&editor_pin_counter);
                }
                GroupModelChangeKind::EditorSticky => {
                    assert!(e.editor.is_some());
                    bump(&editor_sticky_counter);
                }
                GroupModelChangeKind::EditorCapabilities => {
                    assert!(e.editor.is_some());
                    bump(&editor_capabilities_counter);
                }
                GroupModelChangeKind::EditorClose => {
                    assert!(e.editor.is_some());
                    bump(&editor_close_counter);
                    editor_close_events.borrow_mut().push(e.clone());
                }
                _ => {}
            })
        });
        guard.add({
            let active_editor_change_counter = active_editor_change_counter.clone();
            group.on_did_active_editor_change().listen(move |e| {
                assert!(e.editor.is_some());
                bump(&active_editor_change_counter);
            })
        });

        let (editor_close_counter1, editor_close_listener) = count(&group.on_did_close_editor());
        guard.add(editor_close_listener);

        let (editor_will_close_counter, editor_will_close_listener) = count(&group.on_will_close_editor());
        guard.add(editor_will_close_listener);

        let (editor_did_close_counter, editor_did_close_listener) = count(&group.on_did_close_editor());
        guard.add(editor_did_close_listener);

        let input = input(&guard, "foo/bar");
        let input_inactive = input_with_path(&guard, "foo/bar/inactive");

        group.open_editor(&editor(&input), Some(pinned())).await.unwrap();
        group.open_editor(&editor(&input_inactive), Some(EditorOptions { inactive: Some(true), ..EditorOptions::default() })).await.unwrap();

        assert!(group.is_active(&typed(&input)));
        assert!(!group.is_active(&typed(&input_inactive)));
        assert!(group.contains(&typed(&input), None));
        assert!(group.contains(&typed(&input_inactive), None));
        assert!(!group.is_empty());
        assert_eq!(group.count(), 2);
        assert_eq!(editor_capabilities_counter.get(), 0);
        assert_eq!(editor_did_open_counter.get(), 2);
        assert_eq!(editor_open_events.borrow()[0].editor_index, Some(0));
        assert_eq!(editor_open_events.borrow()[1].editor_index, Some(1));
        assert!(is_editor(editor_open_events.borrow()[0].editor.clone(), &input));
        assert!(is_editor(editor_open_events.borrow()[1].editor.clone(), &input_inactive));
        assert_eq!(active_editor_change_counter.get(), 1);
        assert!(is_editor(group.get_editor_by_index(0), &input));
        assert!(is_editor(group.get_editor_by_index(1), &input_inactive));
        assert_eq!(group.get_index_of_editor(&editor(&input)), 0);
        assert_eq!(group.get_index_of_editor(&editor(&input_inactive)), 1);
        assert!(group.is_first(&editor(&input)));
        assert!(!group.is_first(&editor(&input_inactive)));
        assert!(!group.is_last(&editor(&input)));
        assert!(group.is_last(&editor(&input_inactive)));

        input.set_capabilities(EditorInputCapabilities::REQUIRES_TRUST);
        assert_eq!(editor_capabilities_counter.get(), 1);

        input_inactive.set_capabilities(EditorInputCapabilities::SINGLETON);
        assert_eq!(editor_capabilities_counter.get(), 2);

        assert!(is_editor(group.preview_editor(), &input_inactive));
        assert!(!group.is_pinned(&EditorOrIndex::Editor(editor(&input_inactive))));
        group.pin_editor(Some(&editor(&input_inactive)));
        assert_eq!(editor_pin_counter.get(), 1);
        assert!(group.is_pinned(&EditorOrIndex::Editor(editor(&input_inactive))));
        assert!(group.preview_editor().is_none());

        assert!(is_editor(group.active_editor(), &input));
        assert_eq!(group.active_editor_pane().map(|pane| pane.get_id()).as_deref(), Some(TEST_EDITOR_ID));
        assert_eq!(group.count(), 2);

        let mru = group.get_editors(EditorsOrder::MostRecentlyActive, None);
        assert!(is_editor(mru.first().cloned(), &input));
        assert!(is_editor(mru.get(1).cloned(), &input_inactive));

        group.open_editor(&editor(&input_inactive), None).await.unwrap();
        assert_eq!(active_editor_change_counter.get(), 2);
        assert!(is_editor(group.active_editor(), &input_inactive));

        group.open_editor(&editor(&input), None).await.unwrap();
        let closed = group.close_editor(Some(&editor(&input_inactive))).await.unwrap();
        assert!(closed);

        assert_eq!(active_editor_change_counter.get(), 3);
        assert_eq!(editor_close_counter.get(), 1);
        assert_eq!(editor_close_events.borrow()[0].editor_index, Some(1));
        assert!(is_editor(editor_close_events.borrow()[0].editor.clone(), &input_inactive));
        assert_eq!(editor_close_counter1.get(), 1);
        assert_eq!(editor_will_close_counter.get(), 1);
        assert_eq!(editor_did_close_counter.get(), 1);

        assert!(input_inactive.got_disposed.get());

        assert!(is_editor(group.active_editor(), &input));

        assert_eq!(editor_sticky_counter.get(), 0);
        group.stick_editor(Some(&editor(&input)));
        assert_eq!(editor_sticky_counter.get(), 1);
        group.unstick_editor(Some(&editor(&input)));
        assert_eq!(editor_sticky_counter.get(), 2);
    });
}

#[test]
fn open_editors_close_editors() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let part = part_of(&guard);
        let group = part.active_group();
        assert!(group.is_empty());

        let input = input(&guard, "foo/bar");
        let input_inactive = input_with_path(&guard, "foo/bar/inactive");

        group.open_editors(vec![with_options(&input, Some(pinned())), with_options(&input_inactive, None)]).await.unwrap();

        assert_eq!(group.count(), 2);
        assert!(is_editor(group.get_editor_by_index(0), &input));
        assert!(is_editor(group.get_editor_by_index(1), &input_inactive));

        group.close_editors(close_list(&[&input, &input_inactive]), None).await.unwrap();

        assert!(input.got_disposed.get());
        assert!(input_inactive.got_disposed.get());

        assert!(group.is_empty());
    });
}

#[test]
fn close_editor_dirty_editor_handling() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let test_part = create_test_part(&guard, TestConfigurationService::new(None));
        let part = test_part.part.clone();

        test_part.file_dialog_service.set_confirm_result(ConfirmResult::DontSave);

        let group = part.active_group();

        let input = input(&guard, "foo/bar");
        input.dirty.set(true);

        group.open_editor(&editor(&input), None).await.unwrap();

        test_part.file_dialog_service.set_confirm_result(ConfirmResult::Cancel);
        let mut closed = group.close_editor(Some(&editor(&input))).await.unwrap();
        assert!(!closed);

        assert!(!input.got_disposed.get());

        test_part.file_dialog_service.set_confirm_result(ConfirmResult::DontSave);
        closed = group.close_editor(Some(&editor(&input))).await.unwrap();
        assert!(closed);

        assert!(input.got_disposed.get());
    });
}

#[test]
fn close_editor_one_opened_in_multiple_groups() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let part = part_of(&guard);
        let group = part.active_group();
        assert!(group.is_empty());

        let right_group = part.add_group(Some(group_ref(&group)), GroupDirection::Right);

        let input = input(&guard, "foo/bar");
        let input_inactive = input_with_path(&guard, "foo/bar/inactive");

        group.open_editors(vec![with_options(&input, Some(pinned())), with_options(&input_inactive, None)]).await.unwrap();
        right_group.open_editors(vec![with_options(&input, Some(pinned())), with_options(&input_inactive, None)]).await.unwrap();

        let mut closed = right_group.close_editor(Some(&editor(&input))).await.unwrap();
        assert!(closed);

        assert!(!input.got_disposed.get());

        closed = group.close_editor(Some(&editor(&input))).await.unwrap();
        assert!(closed);

        assert!(input.got_disposed.get());
    });
}

#[test]
fn close_editor_cannot_close_editor_handling() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let part = part_of(&guard);
        let group = part.active_group();

        let input = create_cannot_close_test_file_editor_input(&guard, "foo/bar", TEST_EDITOR_INPUT_ID);

        group.open_editor(&editor(&input), None).await.unwrap();

        let closed = group.close_editor(Some(&editor(&input))).await.unwrap();
        assert!(!closed);
        assert_eq!(group.count(), 1);
        assert!(is_editor(group.active_editor(), &input));
        assert!(!input.got_disposed.get());

        let force_closed = group.close_editor_with_options(Some(&editor(&input)), Some(CloseEditorOptions { force: Some(true), ..CloseEditorOptions::default() })).await.unwrap();
        assert!(force_closed);
        assert!(group.is_empty());
        assert!(input.got_disposed.get());
    });
}

#[test]
fn close_editors_dirty_editor_handling() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let test_part = create_test_part(&guard, TestConfigurationService::new(None));
        let part = test_part.part.clone();

        test_part.file_dialog_service.set_confirm_result(ConfirmResult::DontSave);

        let group = part.active_group();

        let input1 = input(&guard, "foo/bar1");
        input1.dirty.set(true);

        let input2 = input(&guard, "foo/bar2");

        group.open_editor(&editor(&input1), None).await.unwrap();
        group.open_editor(&editor(&input2), None).await.unwrap();

        test_part.file_dialog_service.set_confirm_result(ConfirmResult::Cancel);
        let mut close_result = group.close_editors(close_list(&[&input1, &input2]), None).await.unwrap();
        assert!(!close_result);

        assert!(!input1.got_disposed.get());
        assert!(!input2.got_disposed.get());

        test_part.file_dialog_service.set_confirm_result(ConfirmResult::DontSave);
        close_result = group.close_editors(close_list(&[&input1, &input2]), None).await.unwrap();
        assert!(close_result);

        assert!(input1.got_disposed.get());
        assert!(input2.got_disposed.get());
    });
}

#[test]
fn close_editors_except_one() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let part = part_of(&guard);
        let group = part.active_group();
        assert!(group.is_empty());

        let input1 = input(&guard, "foo/bar1");
        let input2 = input(&guard, "foo/bar2");
        let input3 = input(&guard, "foo/bar3");

        group.open_editors(vec![with_options(&input1, Some(pinned())), with_options(&input2, Some(pinned())), with_options(&input3, None)]).await.unwrap();

        assert_eq!(group.count(), 3);
        assert!(is_editor(group.get_editor_by_index(0), &input1));
        assert!(is_editor(group.get_editor_by_index(1), &input2));
        assert!(is_editor(group.get_editor_by_index(2), &input3));

        group.close_editors(close_filter(Some(&input2), None, false, false), None).await.unwrap();
        assert_eq!(group.count(), 1);
        assert!(is_editor(group.get_editor_by_index(0), &input2));
    });
}

#[test]
fn close_editors_cannot_close_editor_handling() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let part = part_of(&guard);
        let group = part.active_group();

        let input1 = input(&guard, "foo/bar1");
        let input2 = create_cannot_close_test_file_editor_input(&guard, "foo/bar2", TEST_EDITOR_INPUT_ID);

        group.open_editors(vec![with_options(&input1, Some(pinned())), with_options(&input2, Some(pinned()))]).await.unwrap();

        let close_result = group.close_editors(close_list(&[&input1, &input2]), None).await.unwrap();
        assert!(close_result);
        assert!(editors_are(&group.get_editors(EditorsOrder::Sequential, None), &[&input2]));
        assert!(input1.got_disposed.get());
        assert!(!input2.got_disposed.get());

        let force_close_result = group.close_editors(close_list(&[&input2]), Some(CloseEditorOptions { force: Some(true), ..CloseEditorOptions::default() })).await.unwrap();
        assert!(force_close_result);
        assert!(group.is_empty());
        assert!(input2.got_disposed.get());
    });
}

#[test]
fn close_editors_except_one_sticky_editor() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let part = part_of(&guard);
        let group = part.active_group();
        assert!(group.is_empty());

        let input1 = input(&guard, "foo/bar1");
        let input2 = input(&guard, "foo/bar2");
        let input3 = input(&guard, "foo/bar3");

        group
            .open_editors(vec![
                with_options(&input1, Some(EditorOptions { pinned: Some(true), sticky: Some(true), ..EditorOptions::default() })),
                with_options(&input2, Some(pinned())),
                with_options(&input3, None),
            ])
            .await
            .unwrap();

        assert_eq!(group.count(), 3);
        assert_eq!(group.sticky_count(), 1);
        assert!(is_editor(group.get_editor_by_index(0), &input1));
        assert!(is_editor(group.get_editor_by_index(1), &input2));
        assert!(is_editor(group.get_editor_by_index(2), &input3));

        group.close_editors(close_filter(Some(&input2), None, true, false), None).await.unwrap();

        assert_eq!(group.count(), 2);
        assert_eq!(group.sticky_count(), 1);
        assert!(is_editor(group.get_editor_by_index(0), &input1));
        assert!(is_editor(group.get_editor_by_index(1), &input2));

        group.close_editors(close_filter(Some(&input2), None, false, false), None).await.unwrap();

        assert_eq!(group.count(), 1);
        assert_eq!(group.sticky_count(), 0);
        assert!(is_editor(group.get_editor_by_index(0), &input2));
    });
}

#[test]
fn close_editors_saved_only() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let part = part_of(&guard);
        let group = part.active_group();
        assert!(group.is_empty());

        let input1 = input(&guard, "foo/bar1");
        let input2 = input(&guard, "foo/bar2");
        let input3 = input(&guard, "foo/bar3");

        group.open_editors(vec![with_options(&input1, Some(pinned())), with_options(&input2, Some(pinned())), with_options(&input3, None)]).await.unwrap();

        assert_eq!(group.count(), 3);
        assert!(is_editor(group.get_editor_by_index(0), &input1));
        assert!(is_editor(group.get_editor_by_index(1), &input2));
        assert!(is_editor(group.get_editor_by_index(2), &input3));

        group.close_editors(close_filter(None, None, false, true), None).await.unwrap();
        assert_eq!(group.count(), 0);
    });
}

#[test]
fn close_editors_saved_only_sticky_editor() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let part = part_of(&guard);
        let group = part.active_group();
        assert!(group.is_empty());

        let input1 = input(&guard, "foo/bar1");
        let input2 = input(&guard, "foo/bar2");
        let input3 = input(&guard, "foo/bar3");

        group
            .open_editors(vec![
                with_options(&input1, Some(EditorOptions { pinned: Some(true), sticky: Some(true), ..EditorOptions::default() })),
                with_options(&input2, Some(pinned())),
                with_options(&input3, None),
            ])
            .await
            .unwrap();

        assert_eq!(group.count(), 3);
        assert_eq!(group.sticky_count(), 1);
        assert!(is_editor(group.get_editor_by_index(0), &input1));
        assert!(is_editor(group.get_editor_by_index(1), &input2));
        assert!(is_editor(group.get_editor_by_index(2), &input3));

        group.close_editors(close_filter(None, None, true, true), None).await.unwrap();

        assert_eq!(group.count(), 1);
        assert_eq!(group.sticky_count(), 1);
        assert!(is_editor(group.get_editor_by_index(0), &input1));

        group.close_editors(close_filter(None, None, false, true), None).await.unwrap();
        assert_eq!(group.count(), 0);
    });
}

#[test]
fn close_editors_direction_right() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let part = part_of(&guard);
        let group = part.active_group();
        assert!(group.is_empty());

        let input1 = input(&guard, "foo/bar1");
        let input2 = input(&guard, "foo/bar2");
        let input3 = input(&guard, "foo/bar3");

        group.open_editors(vec![with_options(&input1, Some(pinned())), with_options(&input2, Some(pinned())), with_options(&input3, None)]).await.unwrap();

        assert_eq!(group.count(), 3);
        assert!(is_editor(group.get_editor_by_index(0), &input1));
        assert!(is_editor(group.get_editor_by_index(1), &input2));
        assert!(is_editor(group.get_editor_by_index(2), &input3));

        group.close_editors(close_filter(Some(&input2), Some(CloseDirection::Right), false, false), None).await.unwrap();
        assert_eq!(group.count(), 2);
        assert!(is_editor(group.get_editor_by_index(0), &input1));
        assert!(is_editor(group.get_editor_by_index(1), &input2));
    });
}

#[test]
fn close_editors_direction_right_sticky_editor() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let part = part_of(&guard);
        let group = part.active_group();
        assert!(group.is_empty());

        let input1 = input(&guard, "foo/bar1");
        let input2 = input(&guard, "foo/bar2");
        let input3 = input(&guard, "foo/bar3");

        group
            .open_editors(vec![
                with_options(&input1, Some(EditorOptions { pinned: Some(true), sticky: Some(true), ..EditorOptions::default() })),
                with_options(&input2, Some(pinned())),
                with_options(&input3, None),
            ])
            .await
            .unwrap();

        assert_eq!(group.count(), 3);
        assert_eq!(group.sticky_count(), 1);
        assert!(is_editor(group.get_editor_by_index(0), &input1));
        assert!(is_editor(group.get_editor_by_index(1), &input2));
        assert!(is_editor(group.get_editor_by_index(2), &input3));

        group.close_editors(close_filter(Some(&input2), Some(CloseDirection::Right), true, false), None).await.unwrap();
        assert_eq!(group.count(), 2);
        assert_eq!(group.sticky_count(), 1);
        assert!(is_editor(group.get_editor_by_index(0), &input1));
        assert!(is_editor(group.get_editor_by_index(1), &input2));

        group.close_editors(close_filter(Some(&input2), Some(CloseDirection::Right), false, false), None).await.unwrap();
        assert_eq!(group.count(), 2);
        assert!(is_editor(group.get_editor_by_index(0), &input1));
        assert!(is_editor(group.get_editor_by_index(1), &input2));
    });
}

#[test]
fn close_editors_direction_left() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let part = part_of(&guard);
        let group = part.active_group();
        assert!(group.is_empty());

        let input1 = input(&guard, "foo/bar1");
        let input2 = input(&guard, "foo/bar2");
        let input3 = input(&guard, "foo/bar3");

        group.open_editors(vec![with_options(&input1, Some(pinned())), with_options(&input2, Some(pinned())), with_options(&input3, None)]).await.unwrap();

        assert_eq!(group.count(), 3);
        assert!(is_editor(group.get_editor_by_index(0), &input1));
        assert!(is_editor(group.get_editor_by_index(1), &input2));
        assert!(is_editor(group.get_editor_by_index(2), &input3));

        group.close_editors(close_filter(Some(&input2), Some(CloseDirection::Left), false, false), None).await.unwrap();
        assert_eq!(group.count(), 2);
        assert!(is_editor(group.get_editor_by_index(0), &input2));
        assert!(is_editor(group.get_editor_by_index(1), &input3));
    });
}

#[test]
fn close_editors_direction_left_sticky_editor() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let part = part_of(&guard);
        let group = part.active_group();
        assert!(group.is_empty());

        let input1 = input(&guard, "foo/bar1");
        let input2 = input(&guard, "foo/bar2");
        let input3 = input(&guard, "foo/bar3");

        group
            .open_editors(vec![
                with_options(&input1, Some(EditorOptions { pinned: Some(true), sticky: Some(true), ..EditorOptions::default() })),
                with_options(&input2, Some(pinned())),
                with_options(&input3, None),
            ])
            .await
            .unwrap();

        assert_eq!(group.count(), 3);
        assert_eq!(group.sticky_count(), 1);
        assert!(is_editor(group.get_editor_by_index(0), &input1));
        assert!(is_editor(group.get_editor_by_index(1), &input2));
        assert!(is_editor(group.get_editor_by_index(2), &input3));

        group.close_editors(close_filter(Some(&input2), Some(CloseDirection::Left), true, false), None).await.unwrap();
        assert_eq!(group.count(), 3);
        assert_eq!(group.sticky_count(), 1);
        assert!(is_editor(group.get_editor_by_index(0), &input1));
        assert!(is_editor(group.get_editor_by_index(1), &input2));
        assert!(is_editor(group.get_editor_by_index(2), &input3));

        group.close_editors(close_filter(Some(&input2), Some(CloseDirection::Left), false, false), None).await.unwrap();
        assert_eq!(group.count(), 2);
        assert!(is_editor(group.get_editor_by_index(0), &input2));
        assert!(is_editor(group.get_editor_by_index(1), &input3));
    });
}

#[test]
fn close_all_editors() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let part = part_of(&guard);
        let group = part.active_group();
        assert!(group.is_empty());

        let input = input(&guard, "foo/bar");
        let input_inactive = input_with_path(&guard, "foo/bar/inactive");

        group.open_editors(vec![with_options(&input, Some(pinned())), with_options(&input_inactive, None)]).await.unwrap();

        assert_eq!(group.count(), 2);
        assert!(is_editor(group.get_editor_by_index(0), &input));
        assert!(is_editor(group.get_editor_by_index(1), &input_inactive));

        group.close_all_editors(None).await.unwrap();
        assert!(group.is_empty());
    });
}

#[test]
fn close_all_editors_dirty_editor_handling() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let test_part = create_test_part(&guard, TestConfigurationService::new(None));
        let part = test_part.part.clone();

        test_part.file_dialog_service.set_confirm_result(ConfirmResult::DontSave);

        let group = part.active_group();

        let input1 = input(&guard, "foo/bar1");
        input1.dirty.set(true);

        let input2 = input(&guard, "foo/bar2");

        group.open_editor(&editor(&input1), None).await.unwrap();
        group.open_editor(&editor(&input2), None).await.unwrap();

        test_part.file_dialog_service.set_confirm_result(ConfirmResult::Cancel);
        let mut close_result = group.close_all_editors(None).await.unwrap();

        assert!(!close_result);
        assert!(!input1.got_disposed.get());
        assert!(!input2.got_disposed.get());

        test_part.file_dialog_service.set_confirm_result(ConfirmResult::DontSave);
        close_result = group.close_all_editors(None).await.unwrap();

        assert!(close_result);
        assert!(input1.got_disposed.get());
        assert!(input2.got_disposed.get());
    });
}

#[test]
fn close_all_editors_sticky_editor() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let part = part_of(&guard);
        let group = part.active_group();
        assert!(group.is_empty());

        let input = input(&guard, "foo/bar");
        let input_inactive = input_with_path(&guard, "foo/bar/inactive");

        group
            .open_editors(vec![with_options(&input, Some(EditorOptions { pinned: Some(true), sticky: Some(true), ..EditorOptions::default() })), with_options(&input_inactive, None)])
            .await
            .unwrap();

        assert_eq!(group.count(), 2);
        assert_eq!(group.sticky_count(), 1);

        group.close_all_editors(Some(CloseAllEditorsOptions { exclude_sticky: Some(true), ..CloseAllEditorsOptions::default() })).await.unwrap();

        assert_eq!(group.count(), 1);
        assert_eq!(group.sticky_count(), 1);
        assert!(is_editor(group.get_editor_by_index(0), &input));

        group.close_all_editors(None).await.unwrap();

        assert!(group.is_empty());
    });
}

#[test]
fn close_all_editors_cannot_close_editor_handling() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let part = part_of(&guard);
        let group = part.active_group();

        let input1 = input(&guard, "foo/bar1");
        let input2 = create_cannot_close_test_file_editor_input(&guard, "foo/bar2", TEST_EDITOR_INPUT_ID);

        group.open_editors(vec![with_options(&input1, Some(pinned())), with_options(&input2, Some(pinned()))]).await.unwrap();

        let close_result = group.close_all_editors(None).await.unwrap();
        assert!(close_result);
        assert!(editors_are(&group.get_editors(EditorsOrder::Sequential, None), &[&input2]));
        assert!(input1.got_disposed.get());
        assert!(!input2.got_disposed.get());
    });
}

#[test]
fn close_all_editors_force_closes_cannot_close_editors() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let part = part_of(&guard);
        let group = part.active_group();

        let input = create_cannot_close_test_file_editor_input(&guard, "foo/bar", TEST_EDITOR_INPUT_ID);

        group.open_editor(&editor(&input), None).await.unwrap();

        let close_result = group.close_all_editors(Some(CloseAllEditorsOptions { force: Some(true), ..CloseAllEditorsOptions::default() })).await.unwrap();
        assert!(close_result);
        assert!(group.is_empty());
        assert!(input.got_disposed.get());
    });
}

#[test]
fn move_editor_same_group() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let part = part_of(&guard);
        let group = part.active_group();
        assert!(group.is_empty());

        let input = input(&guard, "foo/bar");
        let input_inactive = input_with_path(&guard, "foo/bar/inactive");

        let move_events: Rc<RefCell<Vec<GroupModelChangeEvent>>> = Rc::default();
        guard.add({
            let move_events = move_events.clone();
            group.on_did_model_change().listen(move |e| {
                if e.kind == GroupModelChangeKind::EditorMove {
                    assert!(e.editor.is_some());
                    move_events.borrow_mut().push(e.clone());
                }
            })
        });

        group.open_editors(vec![with_options(&input, Some(pinned())), with_options(&input_inactive, None)]).await.unwrap();
        assert_eq!(group.count(), 2);
        assert!(is_editor(group.get_editor_by_index(0), &input));
        assert!(is_editor(group.get_editor_by_index(1), &input_inactive));
        group.move_editor_with_options(&editor(&input_inactive), &group, Some(at_index(0)));
        assert_eq!(move_events.borrow().len(), 1);
        assert_eq!(move_events.borrow()[0].editor_index, Some(0));
        assert_eq!(move_events.borrow()[0].old_editor_index, Some(1));
        assert!(is_editor(move_events.borrow()[0].editor.clone(), &input_inactive));
        assert!(is_editor(group.get_editor_by_index(0), &input_inactive));
        assert!(is_editor(group.get_editor_by_index(1), &input));

        let res = group.move_editors(vec![with_options(&input_inactive, Some(at_index(1)))], &group);
        assert!(res);
        assert_eq!(move_events.borrow().len(), 2);
        assert_eq!(move_events.borrow()[1].editor_index, Some(1));
        assert_eq!(move_events.borrow()[1].old_editor_index, Some(0));
        assert!(is_editor(move_events.borrow()[1].editor.clone(), &input_inactive));
        assert!(is_editor(group.get_editor_by_index(0), &input));
        assert!(is_editor(group.get_editor_by_index(1), &input_inactive));
    });
}

#[test]
fn move_editor_across_groups() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let part = part_of(&guard);
        let group = part.active_group();
        assert!(group.is_empty());

        let right_group = part.add_group(Some(group_ref(&group)), GroupDirection::Right);

        let input = input(&guard, "foo/bar");
        let input_inactive = input_with_path(&guard, "foo/bar/inactive");

        group.open_editors(vec![with_options(&input, Some(pinned())), with_options(&input_inactive, None)]).await.unwrap();
        assert_eq!(group.count(), 2);
        assert!(is_editor(group.get_editor_by_index(0), &input));
        assert!(is_editor(group.get_editor_by_index(1), &input_inactive));
        group.move_editor_with_options(&editor(&input_inactive), &right_group, Some(at_index(0)));
        assert_eq!(group.count(), 1);
        assert!(is_editor(group.get_editor_by_index(0), &input));
        assert_eq!(right_group.count(), 1);
        assert!(is_editor(right_group.get_editor_by_index(0), &input_inactive));
    });
}

#[test]
fn move_editors_across_groups() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let part = part_of(&guard);
        let group = part.active_group();
        assert!(group.is_empty());

        let right_group = part.add_group(Some(group_ref(&group)), GroupDirection::Right);

        let input1 = input(&guard, "foo/bar1");
        let input2 = input(&guard, "foo/bar2");
        let input3 = input(&guard, "foo/bar3");

        group.open_editors(vec![with_options(&input1, Some(pinned())), with_options(&input2, Some(pinned())), with_options(&input3, Some(pinned()))]).await.unwrap();
        assert!(is_editor(group.get_editor_by_index(0), &input1));
        assert!(is_editor(group.get_editor_by_index(1), &input2));
        assert!(is_editor(group.get_editor_by_index(2), &input3));
        group.move_editors(vec![with_options(&input2, None), with_options(&input3, None)], &right_group);
        assert_eq!(group.count(), 1);
        assert_eq!(right_group.count(), 2);
        assert!(is_editor(group.get_editor_by_index(0), &input1));
        assert!(is_editor(right_group.get_editor_by_index(0), &input2));
        assert!(is_editor(right_group.get_editor_by_index(1), &input3));
    });
}

#[test]
fn copy_editor_across_groups() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let part = part_of(&guard);
        let group = part.active_group();
        assert!(group.is_empty());

        let right_group = part.add_group(Some(group_ref(&group)), GroupDirection::Right);

        let input = input(&guard, "foo/bar");
        let input_inactive = input_with_path(&guard, "foo/bar/inactive");

        group.open_editors(vec![with_options(&input, Some(pinned())), with_options(&input_inactive, None)]).await.unwrap();
        assert_eq!(group.count(), 2);
        assert!(is_editor(group.get_editor_by_index(0), &input));
        assert!(is_editor(group.get_editor_by_index(1), &input_inactive));
        group.copy_editor(&editor(&input_inactive), &right_group, Some(at_index(0)));
        assert_eq!(group.count(), 2);
        assert!(is_editor(group.get_editor_by_index(0), &input));
        assert!(is_editor(group.get_editor_by_index(1), &input_inactive));
        assert_eq!(right_group.count(), 1);
        assert!(is_editor(right_group.get_editor_by_index(0), &input_inactive));
    });
}

#[test]
fn copy_editors_across_groups() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let part = part_of(&guard);
        let group = part.active_group();
        assert!(group.is_empty());

        let right_group = part.add_group(Some(group_ref(&group)), GroupDirection::Right);

        let input1 = input(&guard, "foo/bar1");
        let input2 = input(&guard, "foo/bar2");
        let input3 = input(&guard, "foo/bar3");

        group.open_editors(vec![with_options(&input1, Some(pinned())), with_options(&input2, Some(pinned())), with_options(&input3, Some(pinned()))]).await.unwrap();
        assert!(is_editor(group.get_editor_by_index(0), &input1));
        assert!(is_editor(group.get_editor_by_index(1), &input2));
        assert!(is_editor(group.get_editor_by_index(2), &input3));
        group.copy_editors(vec![with_options(&input1, None), with_options(&input2, None), with_options(&input3, None)], &right_group);
        for group in [&group, &right_group] {
            assert!(is_editor(group.get_editor_by_index(0), &input1));
            assert!(is_editor(group.get_editor_by_index(1), &input2));
            assert!(is_editor(group.get_editor_by_index(2), &input3));
        }
    });
}

#[test]
fn replace_editors() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let part = part_of(&guard);
        let group = part.active_group();
        assert!(group.is_empty());

        let input = input(&guard, "foo/bar");
        let input_inactive = input_with_path(&guard, "foo/bar/inactive");

        group.open_editor(&editor(&input), None).await.unwrap();
        assert_eq!(group.count(), 1);
        assert!(is_editor(group.get_editor_by_index(0), &input));

        group.replace_editors(vec![replacement(&input, &input_inactive, None)]).await.unwrap();
        assert_eq!(group.count(), 1);
        assert!(is_editor(group.get_editor_by_index(0), &input_inactive));
    });
}

#[test]
fn replace_editors_dirty_editor_handling() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let test_part = create_test_part(&guard, TestConfigurationService::new(None));
        let part = test_part.part.clone();

        test_part.file_dialog_service.set_confirm_result(ConfirmResult::DontSave);

        let group = part.active_group();

        let input1 = input(&guard, "foo/bar1");
        input1.dirty.set(true);

        let input2 = input(&guard, "foo/bar2");

        group.open_editor(&editor(&input1), None).await.unwrap();
        assert!(is_editor(group.active_editor(), &input1));

        test_part.file_dialog_service.set_confirm_result(ConfirmResult::Cancel);
        group.replace_editors(vec![replacement(&input1, &input2, None)]).await.unwrap();

        assert!(is_editor(group.active_editor(), &input1));
        assert!(!input1.got_disposed.get());

        test_part.file_dialog_service.set_confirm_result(ConfirmResult::DontSave);
        group.replace_editors(vec![replacement(&input1, &input2, None)]).await.unwrap();

        assert!(is_editor(group.active_editor(), &input2));
        assert!(input1.got_disposed.get());
    });
}

#[test]
fn replace_editors_force_replace_dirty_flag() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let test_part = create_test_part(&guard, TestConfigurationService::new(None));
        let part = test_part.part.clone();

        test_part.file_dialog_service.set_confirm_result(ConfirmResult::DontSave);

        let group = part.active_group();

        let input1 = input(&guard, "foo/bar1");
        input1.dirty.set(true);

        let input2 = input(&guard, "foo/bar2");

        group.open_editor(&editor(&input1), None).await.unwrap();
        assert!(is_editor(group.active_editor(), &input1));
        test_part.file_dialog_service.set_confirm_result(ConfirmResult::Cancel);
        group.replace_editors(vec![replacement(&input1, &input2, Some(false))]).await.unwrap();

        assert!(is_editor(group.active_editor(), &input1));
        assert!(!input1.got_disposed.get());

        group.replace_editors(vec![replacement(&input1, &input2, Some(true))]).await.unwrap();

        assert!(is_editor(group.active_editor(), &input2));
        assert!(input1.got_disposed.get());
    });
}

#[test]
fn replace_editors_proper_index_handling() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let part = part_of(&guard);
        let group = part.active_group();
        assert!(group.is_empty());

        let input1 = input(&guard, "foo/bar1");
        let input2 = input(&guard, "foo/bar2");
        let input3 = input(&guard, "foo/bar3");
        let input4 = input(&guard, "foo/bar4");
        let input5 = input(&guard, "foo/bar5");

        let input6 = input(&guard, "foo/bar6");
        let input7 = input(&guard, "foo/bar7");
        let input8 = input(&guard, "foo/bar8");

        group.open_editor(&editor(&input1), Some(pinned())).await.unwrap();
        group.open_editor(&editor(&input2), Some(pinned())).await.unwrap();
        group.open_editor(&editor(&input3), Some(pinned())).await.unwrap();
        group.open_editor(&editor(&input4), Some(pinned())).await.unwrap();
        group.open_editor(&editor(&input5), Some(pinned())).await.unwrap();

        group.replace_editors(vec![replacement(&input1, &input6, None), replacement(&input3, &input7, None), replacement(&input5, &input8, None)]).await.unwrap();

        assert!(is_editor(group.get_editor_by_index(0), &input6));
        assert!(is_editor(group.get_editor_by_index(1), &input2));
        assert!(is_editor(group.get_editor_by_index(2), &input7));
        assert!(is_editor(group.get_editor_by_index(3), &input4));
        assert!(is_editor(group.get_editor_by_index(4), &input8));
    });
}

#[test]
fn replace_editors_should_be_able_to_replace_when_side_by_side_editor_is_involved_with_same_input_side_by_side() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let test_part = create_test_part(&guard, TestConfigurationService::new(None));
        let part = test_part.part.clone();
        let group = part.active_group();
        assert!(group.is_empty());

        let input = input(&guard, "foo/bar");
        let side_by_side_input = guard.add_rc(SideBySideEditorInput::new(None, None, editor(&input), editor(&input), test_part.editor_service.clone()));
        let side_by_side_ref: EditorInputRef = side_by_side_input.clone();

        group.open_editor(&editor(&input), None).await.unwrap();
        assert_eq!(group.count(), 1);
        assert!(is_editor(group.get_editor_by_index(0), &input));

        group.replace_editors(vec![EditorReplacement { editor: editor(&input), replacement: side_by_side_ref.clone(), options: None, force_replace_dirty: None }]).await.unwrap();
        assert_eq!(group.count(), 1);
        assert!(is_editor(group.get_editor_by_index(0), &side_by_side_input));

        group.replace_editors(vec![EditorReplacement { editor: side_by_side_ref, replacement: editor(&input), options: None, force_replace_dirty: None }]).await.unwrap();
        assert_eq!(group.count(), 1);
        assert!(is_editor(group.get_editor_by_index(0), &input));
    });
}

#[test]
fn replace_editors_cannot_close_editor_handling() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let part = part_of(&guard);
        let group = part.active_group();

        let input = create_cannot_close_test_file_editor_input(&guard, "foo/bar", TEST_EDITOR_INPUT_ID);
        let replacement_input = input_with_path(&guard, "foo/baz");

        group.open_editor(&editor(&input), None).await.unwrap();
        group.replace_editors(vec![replacement(&input, &replacement_input, None)]).await.unwrap();

        assert!(editors_are(&group.get_editors(EditorsOrder::Sequential, None), &[&replacement_input]));
        assert!(input.got_disposed.get());
    });
}

#[test]
fn find_editors() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let part = part_of(&guard);
        let group = part.active_group();
        let group2 = part.add_group(Some(group_ref(&group)), GroupDirection::Right);
        assert!(group.is_empty());

        let input1 = input(&guard, "foo/bar1");
        let input2 = create_test_file_editor_input(&guard, "foo/bar1", &format!("{TEST_EDITOR_INPUT_ID}-1"));
        let input3 = input(&guard, "foo/bar3");
        let input4 = input(&guard, "foo/bar4");
        let input5 = create_test_file_editor_input(&guard, "foo/bar4", &format!("{TEST_EDITOR_INPUT_ID}-1"));

        group.open_editor(&editor(&input1), Some(pinned())).await.unwrap();
        group.open_editor(&editor(&input2), Some(pinned())).await.unwrap();
        group.open_editor(&editor(&input3), Some(pinned())).await.unwrap();
        group.open_editor(&editor(&input4), Some(pinned())).await.unwrap();
        group2.open_editor(&editor(&input5), Some(pinned())).await.unwrap();

        let mut found_editors = group.find_editors(&uri_file("foo/bar1"), None);
        assert_eq!(found_editors.len(), 2);
        found_editors = group2.find_editors(&uri_file("foo/bar4"), None);
        assert_eq!(found_editors.len(), 1);
    });
}

#[test]
fn find_editors_side_by_side_support() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let test_part = create_test_part(&guard, TestConfigurationService::new(None));
        let part = test_part.part.clone();

        let group = part.active_group();
        assert!(group.is_empty());

        let secondary_input = input(&guard, "foo/bar-secondary");
        let primary_input = create_test_file_editor_input(&guard, "foo/bar-primary", &format!("{TEST_EDITOR_INPUT_ID}-1"));

        let side_by_side_editor = guard.add_rc(SideBySideEditorInput::new(None, None, editor(&secondary_input), editor(&primary_input), test_part.editor_service.clone()));
        let side_by_side_ref: EditorInputRef = side_by_side_editor.clone();
        group.open_editor(&side_by_side_ref, Some(pinned())).await.unwrap();

        let support = |support_side_by_side: SideBySideEditor| Some(FindEditorOptions { support_side_by_side: Some(support_side_by_side), ..FindEditorOptions::default() });

        let mut found_editors = group.find_editors(&uri_file("foo/bar-secondary"), None);
        assert_eq!(found_editors.len(), 0);

        found_editors = group.find_editors(&uri_file("foo/bar-secondary"), support(SideBySideEditor::Primary));
        assert_eq!(found_editors.len(), 0);

        found_editors = group.find_editors(&uri_file("foo/bar-primary"), support(SideBySideEditor::Primary));
        assert_eq!(found_editors.len(), 1);

        found_editors = group.find_editors(&uri_file("foo/bar-secondary"), support(SideBySideEditor::Secondary));
        assert_eq!(found_editors.len(), 1);

        found_editors = group.find_editors(&uri_file("foo/bar-primary"), support(SideBySideEditor::Secondary));
        assert_eq!(found_editors.len(), 0);

        found_editors = group.find_editors(&uri_file("foo/bar-secondary"), support(SideBySideEditor::Any));
        assert_eq!(found_editors.len(), 1);

        found_editors = group.find_editors(&uri_file("foo/bar-primary"), support(SideBySideEditor::Any));
        assert_eq!(found_editors.len(), 1);
    });
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
fn sticky_editors() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let part = part_of(&guard);
        let group = part.active_group();

        let exclude_sticky = Some(GetEditorsOptions { exclude_sticky: Some(true) });
        let is_sticky = |input: &Rc<TestFileEditorInput>| group.is_sticky(&EditorOrIndex::Editor(editor(input)));

        assert_eq!(group.sticky_count(), 0);
        assert_eq!(group.get_editors(EditorsOrder::Sequential, None).len(), 0);
        assert_eq!(group.get_editors(EditorsOrder::MostRecentlyActive, None).len(), 0);
        assert_eq!(group.get_editors(EditorsOrder::Sequential, exclude_sticky).len(), 0);
        assert_eq!(group.get_editors(EditorsOrder::MostRecentlyActive, exclude_sticky).len(), 0);

        let input = input(&guard, "foo/bar");
        let input_inactive = input_with_path(&guard, "foo/bar/inactive");

        group.open_editor(&editor(&input), Some(pinned())).await.unwrap();
        group.open_editor(&editor(&input_inactive), Some(EditorOptions { inactive: Some(true), ..EditorOptions::default() })).await.unwrap();

        assert_eq!(group.sticky_count(), 0);
        assert!(!is_sticky(&input));
        assert!(!is_sticky(&input_inactive));

        assert_eq!(group.get_editors(EditorsOrder::Sequential, None).len(), 2);
        assert_eq!(group.get_editors(EditorsOrder::MostRecentlyActive, None).len(), 2);
        assert_eq!(group.get_editors(EditorsOrder::Sequential, exclude_sticky).len(), 2);
        assert_eq!(group.get_editors(EditorsOrder::MostRecentlyActive, exclude_sticky).len(), 2);

        group.stick_editor(Some(&editor(&input)));

        assert_eq!(group.sticky_count(), 1);
        assert!(is_sticky(&input));
        assert!(!is_sticky(&input_inactive));

        assert_eq!(group.get_editors(EditorsOrder::Sequential, None).len(), 2);
        assert_eq!(group.get_editors(EditorsOrder::MostRecentlyActive, None).len(), 2);
        assert_eq!(group.get_editors(EditorsOrder::Sequential, exclude_sticky).len(), 1);
        assert_eq!(group.get_editors(EditorsOrder::MostRecentlyActive, exclude_sticky).len(), 1);

        group.unstick_editor(Some(&editor(&input)));

        assert_eq!(group.sticky_count(), 0);
        assert!(!is_sticky(&input));
        assert!(!is_sticky(&input_inactive));

        assert_eq!(group.get_index_of_editor(&editor(&input)), 0);
        assert_eq!(group.get_index_of_editor(&editor(&input_inactive)), 1);

        assert_eq!(group.get_editors(EditorsOrder::Sequential, None).len(), 2);
        assert_eq!(group.get_editors(EditorsOrder::MostRecentlyActive, None).len(), 2);
        assert_eq!(group.get_editors(EditorsOrder::Sequential, exclude_sticky).len(), 2);
        assert_eq!(group.get_editors(EditorsOrder::MostRecentlyActive, exclude_sticky).len(), 2);

        let (editor_move_counter, editor_move_listener) = count_model_changes(&group, GroupModelChangeKind::EditorMove);
        guard.add(editor_move_listener);

        group.stick_editor(Some(&editor(&input_inactive)));

        assert_eq!(group.sticky_count(), 1);
        assert!(!is_sticky(&input));
        assert!(is_sticky(&input_inactive));

        assert_eq!(group.get_index_of_editor(&editor(&input)), 1);
        assert_eq!(group.get_index_of_editor(&editor(&input_inactive)), 0);
        assert_eq!(editor_move_counter.get(), 1);

        assert_eq!(group.get_editors(EditorsOrder::Sequential, None).len(), 2);
        assert_eq!(group.get_editors(EditorsOrder::MostRecentlyActive, None).len(), 2);
        assert_eq!(group.get_editors(EditorsOrder::Sequential, exclude_sticky).len(), 1);
        assert_eq!(group.get_editors(EditorsOrder::MostRecentlyActive, exclude_sticky).len(), 1);

        let input_sticky = input_with_path(&guard, "foo/bar/sticky");

        group.open_editor(&editor(&input_sticky), Some(EditorOptions { sticky: Some(true), ..EditorOptions::default() })).await.unwrap();

        assert_eq!(group.sticky_count(), 2);
        assert!(!is_sticky(&input));
        assert!(is_sticky(&input_inactive));
        assert!(is_sticky(&input_sticky));

        assert_eq!(group.get_index_of_editor(&editor(&input_inactive)), 0);
        assert_eq!(group.get_index_of_editor(&editor(&input_sticky)), 1);
        assert_eq!(group.get_index_of_editor(&editor(&input)), 2);

        group.open_editor(&editor(&input), Some(EditorOptions { sticky: Some(true), ..EditorOptions::default() })).await.unwrap();

        assert_eq!(group.sticky_count(), 3);
        assert!(is_sticky(&input));
        assert!(is_sticky(&input_inactive));
        assert!(is_sticky(&input_sticky));

        assert_eq!(group.get_index_of_editor(&editor(&input_inactive)), 0);
        assert_eq!(group.get_index_of_editor(&editor(&input_sticky)), 1);
        assert_eq!(group.get_index_of_editor(&editor(&input)), 2);
    });
}

#[test]
fn sticky_true_wins_over_index() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let part = part_of(&guard);
        let group = part.active_group();

        assert_eq!(group.sticky_count(), 0);

        let input = input(&guard, "foo/bar");
        let input_inactive = input_with_path(&guard, "foo/bar/inactive");
        let input_sticky = input_with_path(&guard, "foo/bar/sticky");

        group.open_editor(&editor(&input), Some(pinned())).await.unwrap();
        group.open_editor(&editor(&input_inactive), Some(EditorOptions { inactive: Some(true), ..EditorOptions::default() })).await.unwrap();
        group.open_editor(&editor(&input_sticky), Some(EditorOptions { sticky: Some(true), index: Some(2.0), ..EditorOptions::default() })).await.unwrap();

        assert_eq!(group.sticky_count(), 1);
        assert!(group.is_sticky(&EditorOrIndex::Editor(editor(&input_sticky))));

        assert_eq!(group.get_index_of_editor(&editor(&input)), 1);
        assert_eq!(group.get_index_of_editor(&editor(&input_inactive)), 2);
        assert_eq!(group.get_index_of_editor(&editor(&input_sticky)), 0);
    });
}

#[test]
fn selection_set_selection_is_selected_selected_editors() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let part = part_of(&guard);
        let group = part.active_group();

        let input1 = input(&guard, "foo/bar1");
        let input2 = input(&guard, "foo/bar2");
        let input3 = input(&guard, "foo/bar3");

        let is_selection = |inputs: &[&Rc<TestFileEditorInput>]| -> bool {
            let selected_editors = group.selected_editors();
            for input in inputs {
                if !selected_editors.iter().any(|selected| id_of(selected) == id_of(*input)) {
                    return false;
                }
            }
            inputs.len() == selected_editors.len()
        };

        // Active: input1, Selected: input1
        group.open_editors([&input1, &input2, &input3].into_iter().map(|input| with_options(input, Some(pinned()))).collect()).await.unwrap();

        assert!(group.is_active(&typed(&input1)));
        assert!(group.is_selected(&editor(&input1)));
        assert!(!group.is_selected(&editor(&input2)));
        assert!(!group.is_selected(&editor(&input3)));

        assert!(is_selection(&[&input1]));

        // Active: input1, Selected: input1, input3
        group.set_selection(&editor(&input1), vec![editor(&input3)]).await.unwrap();

        assert!(group.is_active(&typed(&input1)));
        assert!(group.is_selected(&editor(&input1)));
        assert!(!group.is_selected(&editor(&input2)));
        assert!(group.is_selected(&editor(&input3)));

        assert!(is_selection(&[&input1, &input3]));

        // Active: input2, Selected: input1, input3
        group.set_selection(&editor(&input2), vec![editor(&input1), editor(&input3)]).await.unwrap();

        assert!(group.is_selected(&editor(&input1)));
        assert!(group.is_active(&typed(&input2)));
        assert!(group.is_selected(&editor(&input2)));
        assert!(group.is_selected(&editor(&input3)));

        assert!(is_selection(&[&input1, &input2, &input3]));

        group.set_selection(&editor(&input1), Vec::new()).await.unwrap();

        // Selected: input3
        assert!(group.is_active(&typed(&input1)));
        assert!(group.is_selected(&editor(&input1)));
        assert!(!group.is_selected(&editor(&input2)));
        assert!(!group.is_selected(&editor(&input3)));

        assert!(is_selection(&[&input1]));
    });
}

#[test]
fn move_editor_with_context_across_groups() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let part = part_of(&guard);
        let group = part.active_group();
        assert!(group.is_empty());

        let right_group = part.add_group(Some(group_ref(&group)), GroupDirection::Right);

        let input = input(&guard, "foo/bar");
        let input_inactive = input_with_path(&guard, "foo/bar/inactive");
        let third_input = input_with_path(&guard, "foo/bar/third");

        let (left_fired_count, left_group_listener) = count(&group.on_will_move_editor());
        let (right_fired_count, right_group_listener) = count(&right_group.on_will_move_editor());

        group.open_editors(vec![with_options(&input, Some(pinned())), with_options(&input_inactive, None), with_options(&third_input, None)]).await.unwrap();
        assert_eq!(left_fired_count.get(), 0);
        assert_eq!(right_fired_count.get(), 0);

        let mut result = group.move_editor(&editor(&input), &right_group);
        assert!(result);
        assert_eq!(left_fired_count.get(), 1);
        assert_eq!(right_fired_count.get(), 0);

        result = group.move_editor(&editor(&input_inactive), &right_group);
        assert!(result);
        assert_eq!(left_fired_count.get(), 2);
        assert_eq!(right_fired_count.get(), 0);

        result = right_group.move_editor(&editor(&input_inactive), &group);
        assert!(result);
        assert_eq!(left_fired_count.get(), 2);
        assert_eq!(right_fired_count.get(), 1);

        left_group_listener.dispose();
        right_group_listener.dispose();
    });
}

#[test]
fn move_editor_disabled() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let part = part_of(&guard);
        let group = part.active_group();
        assert!(group.is_empty());

        let right_group = part.add_group(Some(group_ref(&group)), GroupDirection::Right);

        let input = input(&guard, "foo/bar");
        let input_inactive = input_with_path(&guard, "foo/bar/inactive");
        let third_input = input_with_path(&guard, "foo/bar/third");

        group.open_editors(vec![with_options(&input, Some(pinned())), with_options(&input_inactive, None), with_options(&third_input, None)]).await.unwrap();

        input.set_move_disabled("disabled");
        let result = group.move_editor(&editor(&input), &right_group);

        assert!(!result);
        assert_eq!(group.count(), 3);
    });
}

#[test]
fn on_will_open_editor() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let part = part_of(&guard);
        let group = part.active_group();
        assert!(group.is_empty());

        let right_group = part.add_group(Some(group_ref(&group)), GroupDirection::Right);

        let input = input(&guard, "foo/bar");
        let second_input = input_with_path(&guard, "foo/bar/second");
        let third_input = input_with_path(&guard, "foo/bar/third");

        let (left_fired_count, left_group_listener) = count(&group.on_will_open_editor());
        let (right_fired_count, right_group_listener) = count(&right_group.on_will_open_editor());

        group.open_editor(&editor(&input), None).await.unwrap();
        assert_eq!(left_fired_count.get(), 1);
        assert_eq!(right_fired_count.get(), 0);

        let second_open = right_group.open_editor(&editor(&second_input), None);
        assert_eq!(left_fired_count.get(), 1);
        assert_eq!(right_fired_count.get(), 1);

        let third_open = group.open_editor(&editor(&third_input), None);
        assert_eq!(left_fired_count.get(), 2);
        assert_eq!(right_fired_count.get(), 1);

        // Ensure move fires the open event too
        right_group.move_editor(&editor(&second_input), &group);
        assert_eq!(left_fired_count.get(), 3);
        assert_eq!(right_fired_count.get(), 1);

        left_group_listener.dispose();
        right_group_listener.dispose();

        // as promessas soltas do original terminam aqui, para o corpo não acabar com tarefas pendentes
        second_open.await.unwrap();
        third_open.await.unwrap();
    });
}

#[test]
fn copy_editor_with_context_across_groups() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let part = part_of(&guard);
        let group = part.active_group();
        assert!(group.is_empty());
        let (fired_count, move_listener) = count(&group.on_will_move_editor());

        let right_group = part.add_group(Some(group_ref(&group)), GroupDirection::Right);
        let input = input(&guard, "foo/bar");
        let input_inactive = input_with_path(&guard, "foo/bar/inactive");
        group.open_editors(vec![with_options(&input, Some(pinned())), with_options(&input_inactive, None)]).await.unwrap();
        assert_eq!(fired_count.get(), 0);

        group.copy_editor(&editor(&input_inactive), &right_group, Some(at_index(0)));

        assert_eq!(fired_count.get(), 0);
        move_listener.dispose();
    });
}

#[test]
fn locked_groups_basics() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let part = part_of(&guard);
        let group = part.active_group();

        let right_group = part.add_group(Some(group_ref(&group)), GroupDirection::Right);

        let left_fired_count_from_part = Rc::new(Cell::new(0));
        let right_fired_count_from_part = Rc::new(Cell::new(0));
        let part_listener = {
            let (group, right_group) = (group.clone(), right_group.clone());
            let (left_fired_count_from_part, right_fired_count_from_part) = (left_fired_count_from_part.clone(), right_fired_count_from_part.clone());
            part.on_did_change_group_locked().listen(move |g| {
                if same(g, &group) {
                    bump(&left_fired_count_from_part);
                } else if same(g, &right_group) {
                    bump(&right_fired_count_from_part);
                }
            })
        };

        let (left_fired_count_from_group, left_group_listener) = count_model_changes(&group, GroupModelChangeKind::GroupLocked);
        let (right_fired_count_from_group, right_group_listener) = count_model_changes(&right_group, GroupModelChangeKind::GroupLocked);

        right_group.lock(true);
        right_group.lock(true);

        assert_eq!(left_fired_count_from_group.get(), 0);
        assert_eq!(left_fired_count_from_part.get(), 0);
        assert_eq!(right_fired_count_from_group.get(), 1);
        assert_eq!(right_fired_count_from_part.get(), 1);

        right_group.lock(false);
        right_group.lock(false);

        assert_eq!(left_fired_count_from_group.get(), 0);
        assert_eq!(left_fired_count_from_part.get(), 0);
        assert_eq!(right_fired_count_from_group.get(), 2);
        assert_eq!(right_fired_count_from_part.get(), 2);

        group.lock(true);
        group.lock(true);

        assert_eq!(left_fired_count_from_group.get(), 1);
        assert_eq!(left_fired_count_from_part.get(), 1);
        assert_eq!(right_fired_count_from_group.get(), 2);
        assert_eq!(right_fired_count_from_part.get(), 2);

        group.lock(false);
        group.lock(false);

        assert_eq!(left_fired_count_from_group.get(), 2);
        assert_eq!(left_fired_count_from_part.get(), 2);
        assert_eq!(right_fired_count_from_group.get(), 2);
        assert_eq!(right_fired_count_from_part.get(), 2);

        part_listener.dispose();
        left_group_listener.dispose();
        right_group_listener.dispose();
    });
}

#[test]
fn locked_groups_single_group_is_can_be_locked() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let part = part_of(&guard);
        let group = part.active_group();

        group.lock(true);
        assert!(group.is_locked());

        let right_group = part.add_group(Some(group_ref(&group)), GroupDirection::Right);
        right_group.lock(true);

        assert!(right_group.is_locked());

        part.remove_group(group_ref(&group));
        assert!(right_group.is_locked());

        let right_group2 = part.add_group(Some(group_ref(&right_group)), GroupDirection::Right);
        right_group.lock(true);
        right_group2.lock(true);

        assert!(right_group.is_locked());
        assert!(right_group2.is_locked());

        part.remove_group(group_ref(&right_group2));

        assert!(right_group.is_locked());
    });
}

#[test]
fn locked_groups_auto_locking_via_setting() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let configuration = TestConfigurationService::new(None);
        configuration
            .set_user_configuration(
                "workbench",
                Dynamic::object([("editor", Dynamic::object([("autoLockGroups", Dynamic::object([(TEST_EDITOR_INPUT_ID, Dynamic::Bool(true))]))]))]),
                None,
            )
            .await
            .unwrap();

        let part = create_test_part(&guard, configuration).part;

        let root_group = part.active_group();
        let right_group = part.add_group(Some(group_ref(&root_group)), GroupDirection::Right);

        let input1 = input(&guard, "foo/bar1");
        let input2 = input(&guard, "foo/bar2");

        // First editor opens in right group: Locked=true
        right_group.open_editor(&editor(&input1), Some(pinned())).await.unwrap();
        assert!(right_group.is_locked());

        // Second editors opens in now unlocked right group: Locked=false
        right_group.lock(false);
        right_group.open_editor(&editor(&input2), Some(pinned())).await.unwrap();
        assert!(!right_group.is_locked());

        //First editor opens in root group without other groups being opened: Locked=false
        right_group.close_all_editors(None).await.unwrap();
        part.remove_group(group_ref(&right_group));
        root_group.close_all_editors(None).await.unwrap();

        let input1 = input(&guard, "foo/bar1");
        let _input2 = input(&guard, "foo/bar2");

        root_group.open_editor(&editor(&input1), Some(pinned())).await.unwrap();
        assert!(!root_group.is_locked());
        let _right_group = part.add_group(Some(group_ref(&root_group)), GroupDirection::Right);
        assert!(!root_group.is_locked());
        let left_group = part.add_group(Some(group_ref(&root_group)), GroupDirection::Left);
        assert!(!root_group.is_locked());
        part.remove_group(group_ref(&left_group));
        assert!(!root_group.is_locked());
    });
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
fn transient_editors_basics() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let part = part_of(&guard);
        let group = part.active_group();

        let input = input(&guard, "foo/bar");
        let input_transient = input_with_path(&guard, "foo/bar/inactive");
        let transient = EditorOptions { transient: Some(true), ..EditorOptions::default() };
        let is_transient = |input: &Rc<TestFileEditorInput>| group.is_transient(&EditorOrIndex::Editor(editor(input)));

        group.open_editor(&editor(&input), Some(pinned())).await.unwrap();
        group.open_editor(&editor(&input_transient), Some(transient.clone())).await.unwrap();

        assert!(!is_transient(&input));
        assert!(is_transient(&input_transient));

        group.open_editor(&editor(&input), Some(pinned())).await.unwrap();
        group.open_editor(&editor(&input_transient), Some(transient.clone())).await.unwrap();

        assert!(is_transient(&input_transient));

        group.open_editor(&editor(&input_transient), Some(EditorOptions { transient: Some(false), ..EditorOptions::default() })).await.unwrap();
        assert!(!is_transient(&input_transient));

        group.open_editor(&editor(&input_transient), Some(transient)).await.unwrap();
        assert!(!is_transient(&input_transient)); // cannot make a non-transient editor transient when already opened
    });
}

#[test]
fn transient_editors_pinning_clears_transient() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let part = part_of(&guard);
        let group = part.active_group();

        let input = input(&guard, "foo/bar");
        let input_transient = input_with_path(&guard, "foo/bar/inactive");
        let is_transient = |input: &Rc<TestFileEditorInput>| group.is_transient(&EditorOrIndex::Editor(editor(input)));

        group.open_editor(&editor(&input), Some(pinned())).await.unwrap();
        group.open_editor(&editor(&input_transient), Some(EditorOptions { transient: Some(true), ..EditorOptions::default() })).await.unwrap();

        assert!(!is_transient(&input));
        assert!(is_transient(&input_transient));

        group.open_editor(&editor(&input), Some(pinned())).await.unwrap();
        group.open_editor(&editor(&input_transient), Some(EditorOptions { pinned: Some(true), transient: Some(true), ..EditorOptions::default() })).await.unwrap();

        assert!(!is_transient(&input_transient));
    });
}

#[test]
fn transient_editors_overrides_enable_preview_setting() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let configuration = TestConfigurationService::new(None);
        configuration.set_user_configuration("workbench", Dynamic::object([("editor", Dynamic::object([("enablePreview", Dynamic::Bool(false))]))]), None).await.unwrap();

        let part = create_test_part(&guard, configuration).part;

        let group = part.active_group();
        assert!(group.is_empty());

        let input = input(&guard, "foo/bar");
        let input2 = input_with_path(&guard, "foo/bar2");

        group.open_editor(&editor(&input), Some(EditorOptions { pinned: Some(false), ..EditorOptions::default() })).await.unwrap();
        assert!(group.is_pinned(&EditorOrIndex::Editor(editor(&input))));

        group.open_editor(&editor(&input2), Some(EditorOptions { transient: Some(true), ..EditorOptions::default() })).await.unwrap();
        assert!(!group.is_pinned(&EditorOrIndex::Editor(editor(&input2))));

        group.focus();
        assert!(group.is_pinned(&EditorOrIndex::Editor(editor(&input2))));
    });
}

#[test]
fn working_sets_create_apply_state() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let part = create_test_part(&guard, TestConfigurationService::new(None)).part;

        let input = input_with_path(&guard, "foo/bar");
        let input2 = input_with_path(&guard, "foo/bar2");

        let pane1 = part.active_group().open_editor(&editor(&input), Some(pinned())).await.unwrap();
        let pane2 = part.side_group().open_editor(&editor(&input2), Some(pinned())).await.unwrap();
        // `pane1?.group` e `pane2?.group`
        let group1 = pane1.and_then(|pane| part.get_group(pane.group_id()));
        let group2 = pane2.and_then(|pane| part.get_group(pane.group_id()));

        let state = part.create_state();

        if let Some(group) = &group2 {
            group.close_all_editors(None).await.unwrap();
        }
        if let Some(group) = &group1 {
            group.close_all_editors(None).await.unwrap();
        }

        assert_eq!(part.count(), 1);
        assert!(part.active_group().is_empty());

        part.apply_state(ApplyStateTarget::State(state)).await.unwrap();

        assert_eq!(part.count(), 2);

        assert!(part.groups()[0].contains(&typed(&input), None));
        assert!(part.groups()[1].contains(&typed(&input2), None));

        for group in part.groups() {
            group.close_all_editors(None).await.unwrap();
        }

        let empty_state = part.create_state();

        part.apply_state(ApplyStateTarget::State(empty_state.clone())).await.unwrap();
        assert_eq!(part.count(), 1);

        let input3 = input_with_path(&guard, "foo/bar3");
        input3.dirty.set(true);
        part.active_group().open_editor(&editor(&input3), Some(pinned())).await.unwrap();

        part.apply_state(ApplyStateTarget::State(empty_state)).await.unwrap();

        assert_eq!(part.count(), 1);
        assert!(part.groups()[0].contains(&typed(&input3), None)); // dirty editors enforce to be there even when state is empty

        part.apply_state(ApplyStateTarget::Empty).await.unwrap();

        assert_eq!(part.count(), 1);
        assert!(part.groups()[0].contains(&typed(&input3), None)); // dirty editors enforce to be there even when state is empty

        input3.dirty.set(false);

        part.apply_state(ApplyStateTarget::Empty).await.unwrap();

        assert_eq!(part.count(), 1);
        assert!(part.active_group().is_empty());
    });
}

#[test]
fn working_sets_apply_state_when_the_part_has_never_been_laid_out_does_not_throw_and_registers_restored_groups() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let part = create_test_part(&guard, TestConfigurationService::new(None)).part;

        let input = input_with_path(&guard, "foo/bar");
        let input2 = input_with_path(&guard, "foo/bar2");

        part.active_group().open_editor(&editor(&input), Some(pinned())).await.unwrap();
        part.side_group().open_editor(&editor(&input2), Some(pinned())).await.unwrap();

        let state = part.create_state();

        for group in part.groups() {
            group.close_all_editors(None).await.unwrap();
        }

        // Simulate an editor part that has never been laid out (e.g. it stayed hidden since the window opened, like the Agents
        // window editor area after a reload with the side pane closed). In that state `_contentDimension` is still undefined
        // and laying out during the restore would throw, aborting before the `onDidAddGroup` events fire.
        part.reset_content_dimension();

        let (added_groups, listener) = count(&part.on_did_add_group());

        // Must not throw, must restore the groups, and must fire `onDidAddGroup` for them so listeners (e.g. the editor
        // service) register them.
        part.apply_state(ApplyStateTarget::State(state)).await.unwrap();
        listener.dispose();

        assert_eq!(part.count(), 2);
        assert!(part.groups()[0].contains(&typed(&input), None));
        assert!(part.groups()[1].contains(&typed(&input2), None));
        assert_eq!(added_groups.get(), 2, "expected exactly 2 onDidAddGroup events, got {}", added_groups.get());
    });
}

/// `assert.strictEqual(contextKeyService.getContextKeyValue(key), expected)`.
fn context_key_value_is(service: &dyn IContextKeyService, key: &str, expected: &Dynamic) -> bool {
    service.get_context_key_value(key).strict_equals(expected)
}

/// O número do `group.id + offset` que os provedores de teste devolvem.
fn id_value(group_id: i64) -> Dynamic {
    Dynamic::Number(group_id as f64)
}

#[test]
fn context_key_provider() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();

        // Instantiate workbench and setup initial state
        let test_parts = create_test_part(&guard, TestConfigurationService::new(None));
        let root_context_key_service = test_parts.root_context_key_service.clone();
        let parts = test_parts.parts.clone();

        let input1 = input_with_path(&guard, "foo/bar");
        let input2 = input_with_path(&guard, "foo/bar2");
        let input3 = input_with_path(&guard, "foo/bar3");

        let group1 = parts.active_group();
        let group2 = parts.add_group(Some(group_ref(&group1)), GroupDirection::Right);

        group2.open_editor(&editor(&input2), Some(pinned())).await.unwrap();
        group1.open_editor(&editor(&input1), Some(pinned())).await.unwrap();

        // Create context key provider
        let raw_context_key = RawContextKey::new("testContextKey", parts.active_group().id() as f64, ContextKeyMeta::None);
        let context_key_provider = EditorGroupContextKeyProvider {
            context_key: raw_context_key.clone(),
            get_group_context_key_value: Rc::new(|group: &Rc<dyn IEditorGroup>| id_value(group.id())),
            on_did_change: None,
        };
        guard.add(parts.register_context_key_provider(context_key_provider));

        // Initial state: group1 is active
        assert_eq!(parts.active_group().id(), group1.id());

        let key = raw_context_key.key();
        assert!(context_key_value_is(&*root_context_key_service, key, &id_value(group1.id())));
        assert!(context_key_value_is(&*group1.scoped_context_key_service(), key, &id_value(group1.id())));
        assert!(context_key_value_is(&*group2.scoped_context_key_service(), key, &id_value(group2.id())));

        // Make group2 active and ensure both gloabal and local context key values are updated
        parts.activate_group(group_ref(&group2));

        assert!(context_key_value_is(&*root_context_key_service, key, &id_value(group2.id())));
        assert!(context_key_value_is(&*group1.scoped_context_key_service(), key, &id_value(group1.id())));
        assert!(context_key_value_is(&*group2.scoped_context_key_service(), key, &id_value(group2.id())));

        // Add a new group and ensure both gloabal and local context key values are updated
        // Group 3 will be active
        let group3 = parts.add_group(Some(group_ref(&group2)), GroupDirection::Right);
        group3.open_editor(&editor(&input3), Some(pinned())).await.unwrap();

        assert!(context_key_value_is(&*root_context_key_service, key, &id_value(group3.id())));
        assert!(context_key_value_is(&*group1.scoped_context_key_service(), key, &id_value(group1.id())));
        assert!(context_key_value_is(&*group2.scoped_context_key_service(), key, &id_value(group2.id())));
        assert!(context_key_value_is(&*group3.scoped_context_key_service(), key, &id_value(group3.id())));
    });
}

#[test]
fn context_key_provider_on_did_change() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();

        // Instantiate workbench and setup initial state
        let test_parts = create_test_part(&guard, TestConfigurationService::new(None));
        let root_context_key_service = test_parts.root_context_key_service.clone();
        let parts = test_parts.parts.clone();

        let input1 = input_with_path(&guard, "foo/bar");
        let input2 = input_with_path(&guard, "foo/bar2");

        let group1 = parts.active_group();
        let group2 = parts.add_group(Some(group_ref(&group1)), GroupDirection::Right);

        group2.open_editor(&editor(&input2), Some(pinned())).await.unwrap();
        group1.open_editor(&editor(&input1), Some(pinned())).await.unwrap();

        // Create context key provider
        let offset = Rc::new(Cell::new(0_i64));
        let on_did_change: Rc<Emitter<()>> = guard.add_rc(Emitter::new());

        let raw_context_key = RawContextKey::new("testContextKey", parts.active_group().id() as f64, ContextKeyMeta::None);
        let context_key_provider = EditorGroupContextKeyProvider {
            context_key: raw_context_key.clone(),
            get_group_context_key_value: Rc::new({
                let offset = offset.clone();
                move |group: &Rc<dyn IEditorGroup>| id_value(group.id() + offset.get())
            }),
            on_did_change: Some(on_did_change.event()),
        };
        guard.add(parts.register_context_key_provider(context_key_provider));

        // Initial state: group1 is active
        assert_eq!(parts.active_group().id(), group1.id());

        let key = raw_context_key.key();
        assert!(context_key_value_is(&*root_context_key_service, key, &id_value(group1.id() + offset.get())));
        assert!(context_key_value_is(&*group1.scoped_context_key_service(), key, &id_value(group1.id() + offset.get())));
        assert!(context_key_value_is(&*group2.scoped_context_key_service(), key, &id_value(group2.id() + offset.get())));

        // Make a change to the context key provider and fire onDidChange such that all context key values are updated
        offset.set(10);
        on_did_change.fire(());

        assert!(context_key_value_is(&*root_context_key_service, key, &id_value(group1.id() + offset.get())));
        assert!(context_key_value_is(&*group1.scoped_context_key_service(), key, &id_value(group1.id() + offset.get())));
        assert!(context_key_value_is(&*group2.scoped_context_key_service(), key, &id_value(group2.id() + offset.get())));
    });
}

#[test]
fn context_key_provider_active_editor_change() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();

        // Instantiate workbench and setup initial state
        let test_parts = create_test_part(&guard, TestConfigurationService::new(None));
        let root_context_key_service = test_parts.root_context_key_service.clone();
        let parts = test_parts.parts.clone();

        let input1 = input_with_path(&guard, "foo/bar");
        let input2 = input_with_path(&guard, "foo/bar2");

        let group1 = parts.active_group();

        group1.open_editor(&editor(&input2), Some(pinned())).await.unwrap();
        group1.open_editor(&editor(&input1), Some(pinned())).await.unwrap();

        // Create context key provider
        let resource_text = |resource: &je_base::uri::URI| resource.to_string(false).to_string_lossy();
        let raw_context_key = RawContextKey::new("testContextKey", resource_text(&input1.resource), ContextKeyMeta::None);
        let context_key_provider = EditorGroupContextKeyProvider {
            context_key: raw_context_key.clone(),
            get_group_context_key_value: Rc::new(|group: &Rc<dyn IEditorGroup>| {
                Dynamic::string(&group.active_editor().and_then(|editor| editor.resource()).map_or_else(String::new, |resource| resource.to_string(false).to_string_lossy()))
            }),
            on_did_change: None,
        };
        guard.add(parts.register_context_key_provider(context_key_provider));

        // Initial state: input1 is active
        let active_resource = group1.active_editor().and_then(|editor| editor.resource());
        assert!(ext_uri().is_equal(active_resource.as_ref(), Some(&input1.resource), false));

        let key = raw_context_key.key();
        assert!(context_key_value_is(&*root_context_key_service, key, &Dynamic::string(&resource_text(&input1.resource))));
        assert!(context_key_value_is(&*group1.scoped_context_key_service(), key, &Dynamic::string(&resource_text(&input1.resource))));

        // Make input2 active and ensure both gloabal and local context key values are updated
        group1.open_editor(&editor(&input2), None).await.unwrap();

        assert!(context_key_value_is(&*root_context_key_service, key, &Dynamic::string(&resource_text(&input2.resource))));
        assert!(context_key_value_is(&*group1.scoped_context_key_service(), key, &Dynamic::string(&resource_text(&input2.resource))));
    });
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
