//! Porte de `src/vs/workbench/services/editor/test/browser/editorService.test.ts` (suíte `EditorService`), caso a caso e na mesma ordem.
//!
//! Os casos rodam sobre o `EditorService` real e a [`EditorPart`] lógica, montados pelo [`EditorServiceAccessor`]
//! (`workbenchInstantiationService` mais `createEditorPart`; as divergências do acessor estão no cabeçalho de
//! `support/editor_service_support.rs`). Ainda NÃO portado (o placar conta só os `#[test]`):
//! - `editor type alternatives reuse the group right of the source editor` e `... down of the source editor` (as duas
//!   iterações do `for (const direction of ['right', 'down'])`): exercitam `getAvailableEditorTypes` e `createEditorTypeActions`
//!   de `editorTypePicker.ts`, que não foram portados (e o `NullCommandService`, que eles recebem). Portar o seletor de tipos
//!   de editor é pré-requisito.
//! - `activeTextEditorControl / activeTextEditorMode`: o painel `TestResourceEditor` do original cria um `TestCodeEditor`, e
//!   o caso compara `activeTextEditorControl` com `editor.getControl()` e `activeTextEditorLanguageId` com `plaintext`. O painel
//!   lógico (`EditorPane`) não tem controle de editor (`getControl()` é `None`), então a asserção não tem como ser fiel.
//! - `activeEditorPane scopedContextKeyService`: o `TestEditor` do original expõe um `scopedContextKeyService` próprio (um
//!   `MockContextKeyService`) e o caso exige que ele exista e seja o mesmo do painel do grupo. O painel lógico não tem
//!   serviço de chave de contexto próprio (`IEditorPane` não tem `scopedContextKeyService`).
//!
//! Notas de tradução:
//! - **`setup` e `teardown`.** O `setup` registra o painel `MyTestEditorForEditorService` para a `TestFileEditorInput` (a
//!   `TestForceRevealFileEditorInput` é a mesma struct, com a capacidade `ForceReveal`, e tem o mesmo `TypeId`), o painel do
//!   recurso de texto e o painel lado a lado ([`setup`]). O `teardown` é o `workbenchTeardown` do acessor seguido do
//!   `disposables.clear()` ([`finish`]). Os casos `async` rodam em `run_with_faked_timers` ([`run_async`]); `await` é
//!   `.await.unwrap()`.
//! - **`createEditorService`.** [`create_editor_service`] cria o acessor (o `IEditorGroupsService` é o adaptador sobre a parte
//!   principal, como `instantiationService.stub(IEditorGroupsService, part)`), espera a parte (`await part.whenReady`) e
//!   devolve a parte, o `EditorService` e o acessor no [`Fixture`].
//! - **Sobrecargas de `openEditor`.** `openEditor(untyped, group?)` é `open_editor_in`; `openEditor(input, options?, group?)` é
//!   `open_editor_typed`. Nos casos que passam `{ pinned: true }` depois de uma entrada sem tipo (a posição de `group` da
//!   sobrecarga), o objeto não é um grupo e o `findGroup` o ignora: é o grupo `None`. `openEditors(editors, group?, options?)` é
//!   `open_editors_in` (ou `open_editors` sem grupo). `findEditors` tem uma função por forma (`find_editors`,
//!   `find_editors_by_identifier`, `find_editors_in_group`, `find_editor_in_group`).
//! - **Painéis.** `editor.group` é a busca do grupo pelo `group_id` do painel ([`pane_group`]); `pane.options` é
//!   `IEditorPane::options`; `pane instanceof ErrorPlaceholderEditor` e `instanceof TestTextFileEditor` são comparações do
//!   `getId()` com o identificador do painel registrado; `typedEditor instanceof T` é o `downcast_ref` de `as_any`;
//!   `assert.strictEqual(a, b)` entre painéis, grupos e entradas é a identidade do ponteiro.
//! - **Identidade das entradas sem tipo.** O resolvedor recebe as entradas sem tipo por referência e a fábrica as devolve ao
//!   caso por cópia; `assert.strictEqual(lastEditorFactoryEditor, untypedEditor)` é a comparação por valor do recurso (e do
//!   `original`/`modified`) mais as opções `sticky` e `preserveFocus` ([`untyped_matches`]).
//! - **`IEditorPaneService`.** É o serviço real (`EditorPaneService`), que repassa ao estado estático do descritor de painel
//!   (um `thread_local`, e cada caso roda numa thread).
//! - **`accessor.editorGroupService.groups`/`activateGroup`.** O `IEditorGroupsService` do original é a própria parte, então
//!   é a parte.
//! - **`workspaceTrustRequestService.requestOpenUrisHandler`.** É o campo `request_open_uris_handler` do dublê, trocado por uma
//!   função e restaurado no fim do caso (o `try/finally` do original).
//! - **`workbench.editor.revealIfOpen`.** O `TestConfigurationService` com a opção, que o original cria antes do
//!   `workbenchInstantiationService`, recebe aqui `set_user_configuration` logo depois da criação do acessor: o `EditorService` lê
//!   a configuração na hora de abrir.
//! - **`Object.defineProperty(input, 'preferredResource', ...)`.** É `TestFileEditorInput::override_preferred_resource`; a
//!   subclasse que conta as chamadas de `rename` é o `rename_calls` da própria `TestFileEditorInput`.
//! - **`Event.toPromise(Event.once(...))`.** É `Event::to_promise(None)` (que já dispara uma vez só).
//! - **`FileOperationEvent` de `MOVE`.** O `IFileStatWithMetadata` do alvo é o `FileStatWithMetadata::new` com os mesmos campos.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::any::TypeId;
use std::cell::{Cell, RefCell};
use std::future::Future;
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers, timeout};
use je_base::JsString;
use je_base::dynamic::Dynamic;
use je_base::event::Event;
use je_base::lifecycle::{DisposableRef, DisposableStore, id_of};
use je_base::promise::{MaybePromise, Promise};
use je_base::uri::{Field, URI, UriChange};
use je_platform::files::files::{FileOperation, FileOperationEvent, FileStat, FileStatWithMetadata};
use je_platform::opener::editor_types::{EditorActivation, EditorOptions, EditorOverride};
use je_platform::resources::uri_file;
use je_platform::workspace_trust::WorkspaceTrustUriResponse;
use je_workbench::browser::parts::editor::editor_panes::ERROR_EDITOR_ID;
use je_workbench::browser::parts::editor::editor_part::EditorPart;
use je_workbench::common::editor::editor_group_model::{EditorOrIndex, GetEditorsOptions};
use je_workbench::common::editor::editor_input::EditorInput;
use je_workbench::common::editor::side_by_side_editor_input::SideBySideEditorInput;
use je_workbench::common::editor::{
    DEFAULT_EDITOR_ASSOCIATION_ID, EditorCloseContext, EditorCloseEvent, EditorIdentifier, EditorInputCapabilities, EditorInputOrUntyped, EditorInputRef,
    EditorInputWithOptions, EditorsOrder, FindEditorOptions, IEditorPane, MoveResult, SideBySideEditor, TextEditorOptions, UntypedEditorInput,
};
use je_workbench::contrib::files::browser::editors::file_editor_input::FileEditorInput;
use je_workbench::services::editor::editor_group_finder::FindGroupEditor;
use je_workbench::services::editor::editor_groups_container::EditorGroupsContainer;
use je_workbench::services::editor::editor_groups_service::{
    EditorReplacement, GroupDirection, GroupRef, GroupsArrangement, IEditorGroup, IEditorPart, PreferredGroup,
};
use je_workbench::services::editor::editor_pane_service::IEditorPaneService;
use je_workbench::services::editor::editor_resolver_service::{
    EditorInputFactoryFunction, EditorInputFactoryObject, GlobPattern, IEditorResolverService, RegisteredEditorOptions, RegisteredEditorPriority,
    RegisteredEditorRegistrationInfo,
};
use je_workbench::services::editor::editor_service::{
    BaseSaveRevertAllEditorOptions, EditorReplacementInput, EditorsChangeEvent, IEditorService, IncludeUntitled, OpenEditorsOptions, ResourceEditorInputIdentifier,
    RevertAllEditorsOptions, SaveAllEditorsOptions, SaveEditorsOptions, UntypedEditorReplacement,
};
use je_workbench::services::textfile::text_editor_service::ITextEditorService;
use je_workbench::test::browser::workbench_test_services::{TestFileEditorInput, register_test_editor};
use support::editor_service_support::{
    EditorServiceAccessor, TEST_TEXT_FILE_EDITOR_ID, register_test_file_editor, register_test_resource_editor, register_test_side_by_side_editor,
};

const TEST_EDITOR_ID: &str = "MyTestEditorForEditorService";
const TEST_EDITOR_INPUT_ID: &str = "testEditorInputForEditorService";

type Group = Rc<dyn IEditorGroup>;
type Pane = Rc<dyn IEditorPane>;

//#region Infraestrutura da suíte

/// `createEditorService()`: a parte, o `EditorService` e o acessor.
struct Fixture {
    accessor: EditorServiceAccessor,
    part: Rc<EditorPart>,
    service: Rc<dyn IEditorService>,
}

/// O `setup` da suíte.
fn setup(disposables: &Rc<DisposableStore>) {
    disposables.add(register_test_editor(TEST_EDITOR_ID, vec![TypeId::of::<TestFileEditorInput>()], Some(TEST_EDITOR_INPUT_ID)));
    disposables.add(register_test_resource_editor());
    disposables.add(register_test_side_by_side_editor());
}

/// `createEditorService(instantiationService = workbenchInstantiationService(undefined, disposables))`.
async fn create_editor_service(disposables: &Rc<DisposableStore>) -> Fixture {
    let accessor = EditorServiceAccessor::with_default_backup_service(disposables);
    let part = accessor.part.clone();
    accessor.part.when_ready().await.unwrap();
    let service: Rc<dyn IEditorService> = accessor.editor_service.clone();

    Fixture { accessor, part, service }
}

/// O `teardown`: `workbenchTeardown(testLocalInstantiationService)` e `disposables.clear()`.
async fn finish(fixture: &Fixture, disposables: &Rc<DisposableStore>) {
    fixture.accessor.teardown().await.unwrap();
    disposables.clear();
}

/// O caso assíncrono: o guarda de vazamentos, o `setup` e o corpo no relógio virtual.
fn run_async<Fut: Future<Output = ()> + 'static>(body: impl FnOnce(Rc<DisposableStore>) -> Fut + 'static) {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let disposables = guard.store();

    run_with_faked_timers(async move {
        setup(&disposables);
        body(disposables).await;
    });
}

/// `URI.parse(value)`.
fn uri(value: &str) -> URI {
    URI::parse(value, false)
}

/// `resource.toString()`.
fn res(resource: &URI) -> String {
    resource.to_string(false).to_string_lossy()
}

/// `createTestFileEditorInput(URI.parse(resource), TEST_EDITOR_INPUT_ID)`.
fn new_input(disposables: &Rc<DisposableStore>, resource: &str) -> Rc<TestFileEditorInput> {
    new_input_of(disposables, uri(resource), TEST_EDITOR_INPUT_ID)
}

/// `createTestFileEditorInput(resource, typeId)`.
fn new_input_of(disposables: &Rc<DisposableStore>, resource: URI, type_id: &str) -> Rc<TestFileEditorInput> {
    disposables.add_rc(TestFileEditorInput::new(resource, type_id))
}

/// A entrada de teste como `EditorInput`.
fn editor_ref<T: EditorInput + 'static>(input: &Rc<T>) -> EditorInputRef {
    input.clone()
}

/// `{ ...options }` sobre as opções de `IEditorOptions`.
fn opts(configure: impl FnOnce(&mut EditorOptions)) -> TextEditorOptions {
    let mut options = TextEditorOptions::default();
    configure(&mut options.base);

    options
}

/// `{ pinned: true }`.
fn pinned() -> Option<TextEditorOptions> {
    Some(opts(|options| options.pinned = Some(true)))
}

/// `{ resource, options }` como `IResourceEditorInput`.
fn untyped(resource: URI, options: Option<TextEditorOptions>) -> UntypedEditorInput {
    UntypedEditorInput { resource: Some(resource), options, ..UntypedEditorInput::default() }
}

/// `{ resource: URI.parse(resource), options: { pinned: true } }`.
fn untyped_pinned(resource: &str) -> UntypedEditorInput {
    untyped(uri(resource), pinned())
}

/// `{ ...input, options: { ...input.options, <configure> } }`.
fn with_options(input: &UntypedEditorInput, configure: impl FnOnce(&mut EditorOptions)) -> UntypedEditorInput {
    let mut options = input.options.clone().unwrap_or_default();
    configure(&mut options.base);

    UntypedEditorInput { options: Some(options), ..input.clone() }
}

/// `{ override: id }`.
fn override_of(id: &str) -> EditorOverride {
    EditorOverride::Id(JsString::from(id))
}

fn gref(group: &Group) -> GroupRef {
    GroupRef::Group(group.clone())
}

fn pref(group: &Group) -> PreferredGroup {
    PreferredGroup::Group(group.clone())
}

/// `assert.strictEqual(a, b)` entre grupos.
fn same_group(a: &Group, b: &Group) -> bool {
    Rc::ptr_eq(a, b)
}

/// `assert.strictEqual(a, b)` entre painéis.
fn same_pane(a: &Pane, b: &Pane) -> bool {
    id_of(a) == id_of(b)
}

/// `assert.strictEqual(a, b)` entre entradas.
fn is<T: ?Sized, U: ?Sized>(actual: &Rc<T>, expected: &Rc<U>) -> bool {
    id_of(actual) == id_of(expected)
}

/// `assert.strictEqual(maybe, expected)` entre uma entrada que pode faltar e a esperada.
fn is_some_of<T: ?Sized, U: ?Sized>(actual: Option<Rc<T>>, expected: &Rc<U>) -> bool {
    actual.is_some_and(|actual| is(&actual, expected))
}

/// O `editor` de um resultado de `save`.
fn is_typed<U: ?Sized>(actual: &EditorInputOrUntyped, expected: &Rc<U>) -> bool {
    matches!(actual, EditorInputOrUntyped::Typed(actual) if is(actual, expected))
}

/// `assert.strictEqual(service.activeEditor, input)`.
fn active_editor_is<U: ?Sized>(service: &dyn IEditorService, expected: &Rc<U>) -> bool {
    service.active_editor().is_some_and(|active| is_typed(&active, expected))
}

/// `pane.group`.
fn pane_group(part: &Rc<EditorPart>, pane: &Pane) -> Group {
    part.get_group(pane.group_id()).expect("o grupo do painel existe")
}

/// `assert.ok(pane)`.
fn expect_pane(pane: Option<Pane>) -> Pane {
    pane.expect("o editor abriu")
}

/// `group.activeEditor?.resource?.toString()`.
fn active_resource(group: &Group) -> Option<String> {
    group.active_editor().and_then(|editor| editor.resource()).map(|resource| res(&resource))
}

/// `part.activeGroup.activeEditor?.resource?.toString()`.
fn part_active_resource(part: &Rc<EditorPart>) -> Option<String> {
    active_resource(&part.active_group())
}

/// `editor.resource.toString()` de uma entrada que o painel mostra.
fn resource_string(editor: &EditorInputRef) -> String {
    res(&editor.resource().expect("a entrada tem recurso"))
}

/// O identificador `{ resource, typeId, editorId }` de uma entrada.
fn identifier(input: &Rc<TestFileEditorInput>) -> ResourceEditorInputIdentifier {
    ResourceEditorInputIdentifier { resource: input.resource.clone(), type_id: input.editor_type_id(), editor_id: input.editor_id() }
}

/// O contador de eventos de `event`, com o ouvinte no `disposables`.
fn counter<T: 'static>(disposables: &Rc<DisposableStore>, event: &Event<T>) -> Rc<Cell<usize>> {
    let counter = Rc::new(Cell::new(0));
    let listener = {
        let counter = counter.clone();
        event.listen(move |_| counter.set(counter.get() + 1))
    };
    disposables.add(listener);

    counter
}

/// A bandeira que o ouvinte de `event` liga (`let fired = false; event(() => { fired = true; })`).
fn flag<T: 'static>(event: &Event<T>) -> (Rc<Cell<bool>>, DisposableRef) {
    let fired = Rc::new(Cell::new(false));
    let listener = {
        let fired = fired.clone();
        event.listen(move |_| fired.set(true))
    };

    (fired, listener)
}

/// `service.openEditor(input, options, group)`.
async fn open<T: EditorInput + 'static>(service: &dyn IEditorService, input: &Rc<T>, options: Option<TextEditorOptions>, group: Option<PreferredGroup>) -> Option<Pane> {
    service.open_editor_typed(&editor_ref(input), options, group).await.unwrap()
}

/// `service.openEditor(untyped, group)`.
async fn open_untyped(service: &dyn IEditorService, input: UntypedEditorInput, group: Option<PreferredGroup>) -> Option<Pane> {
    service.open_editor_in(input, group).await.unwrap()
}

/// `service.openEditors([{ editor: input }, ...])`.
async fn open_all(service: &dyn IEditorService, inputs: &[&Rc<TestFileEditorInput>]) {
    let editors = inputs.iter().map(|input| EditorInputWithOptions { editor: editor_ref(input), options: None }).collect();
    service.open_editors(editors).await.unwrap();
}

/// `group.closeEditor(input)`.
async fn close<T: EditorInput + 'static>(group: &Group, input: &Rc<T>) {
    group.close_editor(Some(&editor_ref(input))).await.unwrap();
}

/// `registerEditor(glob, { id: TEST_EDITOR_INPUT_ID, label: 'Label', priority: exclusive }, {}, factories)`.
fn register_exclusive_editor(fixture: &Fixture, disposables: &Rc<DisposableStore>, glob: &str, factories: EditorInputFactoryObject) {
    disposables.add(fixture.accessor.editor_resolver_service.register_editor(
        GlobPattern::Pattern(glob.to_string()),
        RegisteredEditorRegistrationInfo { id: TEST_EDITOR_INPUT_ID.to_string(), label: "Label".to_string(), detail: None, priority: RegisteredEditorPriority::Exclusive.into() },
        RegisteredEditorOptions::default(),
        factories,
    ));
}

/// `createEditorInput: editor => ({ editor: createTestFileEditorInput(editor.resource, TEST_EDITOR_INPUT_ID) })`.
fn test_file_factories(disposables: &Rc<DisposableStore>) -> EditorInputFactoryObject {
    let disposables = disposables.clone();

    EditorInputFactoryObject {
        create_editor_input: Some(Rc::new(move |editor: &UntypedEditorInput, _group: &Group| {
            let resource = editor.resource.clone().expect("a entrada tem recurso");

            MaybePromise::Value(EditorInputWithOptions { editor: new_input_of(&disposables, resource, TEST_EDITOR_INPUT_ID), options: None })
        })),
        ..EditorInputFactoryObject::default()
    }
}

/// O registro de `*.editor-service-locked-group-tests` que os casos de grupos bloqueados fazem.
fn register_locked_group_editor(fixture: &Fixture, disposables: &Rc<DisposableStore>) {
    register_exclusive_editor(fixture, disposables, "*.editor-service-locked-group-tests", test_file_factories(disposables));
}

//#endregion

//#region openEditor() - basics

#[test]
fn open_editor_basics() {
    run_async(|disposables| async move {
        let fixture = create_editor_service(&disposables).await;

        test_open_basics(&disposables, &*fixture.service, &fixture.part, fixture.accessor.editor_pane_service.as_ref()).await;

        finish(&fixture, &disposables).await;
    });
}

#[test]
fn open_editor_basics_scoped() {
    run_async(|disposables| async move {
        let fixture = create_editor_service(&disposables).await;
        let part_container: Rc<dyn IEditorPart> = fixture.part.clone();
        let scoped = fixture.service.create_scoped(EditorGroupsContainer::Part(part_container), &disposables);
        fixture.part.when_ready().await.unwrap();

        test_open_basics(&disposables, &*scoped, &fixture.part, fixture.accessor.editor_pane_service.as_ref()).await;

        finish(&fixture, &disposables).await;
    });
}

async fn test_open_basics(disposables: &Rc<DisposableStore>, editor_service: &dyn IEditorService, part: &Rc<EditorPart>, editor_pane_service: &dyn IEditorPaneService) {
    let mut input = new_input(disposables, "my://resource-basics");
    let mut other_input = new_input(disposables, "my://resource2-basics");

    let active_editor_change_event_counter = counter(disposables, &editor_service.on_did_active_editor_change());
    let visible_editor_change_event_counter = counter(disposables, &editor_service.on_did_visible_editors_change());
    let will_open_editor_listener_counter = counter(disposables, &editor_service.on_will_open_editor());
    let did_close_editor_listener_counter = counter(disposables, &editor_service.on_did_close_editor());

    let will_instantiate_editor_pane_listener_counter = Rc::new(Cell::new(0));
    disposables.add({
        let counter = will_instantiate_editor_pane_listener_counter.clone();
        editor_pane_service.on_will_instantiate_editor_pane().listen(move |event| {
            if event.editor_type_id == TEST_EDITOR_ID {
                counter.set(counter.get() + 1);
            }
        })
    });

    // Open input
    let editor = expect_pane(open(editor_service, &input, pinned(), None).await);

    assert_eq!(editor.get_id(), TEST_EDITOR_ID);
    assert!(same_pane(&editor, &editor_service.active_editor_pane().expect("há um painel ativo")));
    assert_eq!(1, editor_service.count());
    assert!(is(&editor_service.get_editors(EditorsOrder::MostRecentlyActive)[0].editor, &input));
    assert!(is(&editor_service.get_editors(EditorsOrder::Sequential)[0].editor, &input));
    assert!(active_editor_is(editor_service, &input));
    assert_eq!(editor_service.visible_editor_panes().len(), 1);
    assert!(same_pane(&editor_service.visible_editor_panes()[0], &editor));
    assert!(editor_service.active_text_editor_control().is_none());
    assert!(editor_service.active_text_editor_language_id().is_none());
    assert_eq!(editor_service.visible_text_editor_controls().len(), 0);
    assert_eq!(editor_service.get_visible_text_editor_controls(EditorsOrder::MostRecentlyActive).len(), 0);
    assert!(editor_service.is_opened(&identifier(&input)));
    assert!(editor_service.is_opened(&identifier(&input)));
    assert!(!editor_service.is_opened(&ResourceEditorInputIdentifier { editor_id: Some("unknownTypeId".to_string()), ..identifier(&input) }));
    assert!(!editor_service.is_opened(&ResourceEditorInputIdentifier { type_id: "unknownTypeId".to_string(), ..identifier(&input) }));
    assert!(!editor_service.is_opened(&ResourceEditorInputIdentifier {
        type_id: "unknownTypeId".to_string(),
        editor_id: Some("unknownTypeId".to_string()),
        ..identifier(&input)
    }));
    assert!(editor_service.is_visible(&editor_ref(&input)));
    assert!(!editor_service.is_visible(&editor_ref(&other_input)));
    assert_eq!(will_open_editor_listener_counter.get(), 1);
    assert_eq!(active_editor_change_event_counter.get(), 1);
    assert_eq!(visible_editor_change_event_counter.get(), 1);
    assert!(editor_pane_service.did_instantiate_editor_pane(TEST_EDITOR_ID));
    assert_eq!(will_instantiate_editor_pane_listener_counter.get(), 1);

    // Close input
    close(&pane_group(part, &editor), &input).await;

    assert_eq!(0, editor_service.count());
    assert_eq!(0, editor_service.get_editors(EditorsOrder::MostRecentlyActive).len());
    assert_eq!(0, editor_service.get_editors(EditorsOrder::Sequential).len());
    assert_eq!(did_close_editor_listener_counter.get(), 1);
    assert_eq!(active_editor_change_event_counter.get(), 2);
    assert_eq!(visible_editor_change_event_counter.get(), 2);
    assert!(input.got_disposed.get());

    // Open again 2 inputs (disposed editors are ignored!)
    open(editor_service, &input, pinned(), None).await;
    assert_eq!(0, editor_service.count());

    // Open again 2 inputs (recreate because disposed)
    input = new_input(disposables, "my://resource-basics");
    other_input = new_input(disposables, "my://resource2-basics");

    open(editor_service, &input, pinned(), None).await;
    open(editor_service, &other_input, pinned(), None).await;

    assert_eq!(2, editor_service.count());
    assert!(is(&editor_service.get_editors(EditorsOrder::MostRecentlyActive)[0].editor, &other_input));
    assert!(is(&editor_service.get_editors(EditorsOrder::MostRecentlyActive)[1].editor, &input));
    assert!(is(&editor_service.get_editors(EditorsOrder::Sequential)[0].editor, &input));
    assert!(is(&editor_service.get_editors(EditorsOrder::Sequential)[1].editor, &other_input));
    assert_eq!(editor_service.visible_editor_panes().len(), 1);
    assert!(editor_service.is_opened(&identifier(&input)));
    assert!(editor_service.is_opened(&identifier(&input)));
    assert!(editor_service.is_opened(&identifier(&other_input)));
    assert!(editor_service.is_opened(&identifier(&other_input)));

    assert_eq!(active_editor_change_event_counter.get(), 4);
    assert_eq!(will_open_editor_listener_counter.get(), 3);
    assert_eq!(visible_editor_change_event_counter.get(), 4);

    let sticky_input = new_input(disposables, "my://resource3-basics");
    open(editor_service, &sticky_input, Some(opts(|options| options.sticky = Some(true))), None).await;

    assert_eq!(3, editor_service.count());

    let all_sequential_editors = editor_service.get_editors(EditorsOrder::Sequential);
    assert_eq!(all_sequential_editors.len(), 3);
    assert!(is(&all_sequential_editors[0].editor, &sticky_input));
    assert!(is(&all_sequential_editors[1].editor, &input));
    assert!(is(&all_sequential_editors[2].editor, &other_input));

    let exclude_sticky = Some(GetEditorsOptions { exclude_sticky: Some(true) });
    let sequential_editors_excluding_sticky = editor_service.get_editors_with_options(EditorsOrder::Sequential, exclude_sticky);
    assert_eq!(sequential_editors_excluding_sticky.len(), 2);
    assert!(is(&sequential_editors_excluding_sticky[0].editor, &input));
    assert!(is(&sequential_editors_excluding_sticky[1].editor, &other_input));

    let mru_editors_excluding_sticky = editor_service.get_editors_with_options(EditorsOrder::MostRecentlyActive, exclude_sticky);
    assert_eq!(mru_editors_excluding_sticky.len(), 2);
    assert!(is(&sequential_editors_excluding_sticky[0].editor, &input));
    assert!(is(&sequential_editors_excluding_sticky[1].editor, &other_input));
}

#[test]
fn open_editor_multiple_calls_are_cancelled_and_indicated_as_such() {
    run_async(|disposables| async move {
        let fixture = create_editor_service(&disposables).await;
        let service = &fixture.service;

        let input = new_input(&disposables, "my://resource-basics");
        let other_input = new_input(&disposables, "my://resource2-basics");

        let active_editor_change_listener_counter = Rc::new(Cell::new(0));
        let active_editor_change_listener = {
            let counter = active_editor_change_listener_counter.clone();
            service.on_did_active_editor_change().listen(move |_| counter.set(counter.get() + 1))
        };

        let visible_editor_change_listener_counter = Rc::new(Cell::new(0));
        let visible_editor_change_listener = {
            let counter = visible_editor_change_listener_counter.clone();
            service.on_did_visible_editors_change().listen(move |_| counter.set(counter.get() + 1))
        };

        let editor_p1 = service.open_editor_typed(&editor_ref(&input), pinned(), None);
        let editor_p2 = service.open_editor_typed(&editor_ref(&other_input), pinned(), None);

        let editor1 = editor_p1.await.unwrap();
        assert!(editor1.is_none());

        let editor2 = editor_p2.await.unwrap();
        assert!(is_some_of(editor2.and_then(|editor| editor.input()), &other_input));

        assert_eq!(active_editor_change_listener_counter.get(), 1);
        assert_eq!(visible_editor_change_listener_counter.get(), 1);

        active_editor_change_listener.dispose();
        visible_editor_change_listener.dispose();

        finish(&fixture, &disposables).await;
    });
}

#[test]
fn open_editor_same_input_does_not_cancel_previous_one_136684() {
    run_async(|disposables| async move {
        let fixture = create_editor_service(&disposables).await;
        let service = &fixture.service;

        let mut input = new_input(&disposables, "my://resource-basics");

        let mut editor_p1 = service.open_editor_typed(&editor_ref(&input), pinned(), None);
        let mut editor_p2 = service.open_editor_typed(&editor_ref(&input), pinned(), None);

        let mut editor1 = editor_p1.await.unwrap();
        assert!(is_some_of(editor1.and_then(|editor| editor.input()), &input));

        let mut editor2 = editor_p2.await.unwrap();
        assert!(is_some_of(editor2.clone().and_then(|editor| editor.input()), &input));

        let editor2_pane = editor2.take().expect("o editor abriu");
        pane_group(&fixture.part, &editor2_pane).close_all_editors(None).await.unwrap();

        input = new_input(&disposables, "my://resource-basics");
        let input_same = new_input(&disposables, "my://resource-basics");

        editor_p1 = service.open_editor_typed(&editor_ref(&input), pinned(), None);
        editor_p2 = service.open_editor_typed(&editor_ref(&input_same), pinned(), None);

        editor1 = editor_p1.await.unwrap();
        assert!(is_some_of(editor1.and_then(|editor| editor.input()), &input));

        editor2 = editor_p2.await.unwrap();
        assert!(is_some_of(editor2.and_then(|editor| editor.input()), &input));

        finish(&fixture, &disposables).await;
    });
}

#[test]
fn open_editor_force_reveal_typed_editors_reveal_instead_of_split() {
    run_async(|disposables| async move {
        let fixture = create_editor_service(&disposables).await;
        let (part, service) = (&fixture.part, &fixture.service);

        let input1 = disposables.add_rc(TestFileEditorInput::new_force_reveal(uri("my://resource-basics1"), TEST_EDITOR_INPUT_ID));
        let input2 = disposables.add_rc(TestFileEditorInput::new_force_reveal(uri("my://resource-basics2"), TEST_EDITOR_INPUT_ID));

        let input1_group = pane_group(part, &expect_pane(open(&**service, &input1, pinned(), None).await));
        let input2_group = pane_group(part, &expect_pane(open(&**service, &input2, pinned(), Some(PreferredGroup::Side)).await));

        assert!(same_group(&part.active_group(), &input2_group));

        open(&**service, &input1, pinned(), None).await;

        assert!(same_group(&part.active_group(), &input1_group));

        finish(&fixture, &disposables).await;
    });
}

#[test]
fn open_editor_locked_groups() {
    run_async(|disposables| async move {
        disposables.add(register_test_file_editor());

        let fixture = create_editor_service(&disposables).await;
        let (part, service) = (&fixture.part, &fixture.service);

        register_locked_group_editor(&fixture, &disposables);

        let input1 = untyped_pinned("file://resource-basics.editor-service-locked-group-tests");
        let input2 = untyped_pinned("file://resource2-basics.editor-service-locked-group-tests");
        let input3 = untyped_pinned("file://resource3-basics.editor-service-locked-group-tests");
        let input4 = untyped_pinned("file://resource4-basics.editor-service-locked-group-tests");
        let input5 = untyped_pinned("file://resource5-basics.editor-service-locked-group-tests");
        let input6 = untyped_pinned("file://resource6-basics.editor-service-locked-group-tests");
        let input7 = untyped_pinned("file://resource7-basics.editor-service-locked-group-tests");
        let resource_of = |input: &UntypedEditorInput| res(input.resource.as_ref().expect("a entrada tem recurso"));

        let editor1 = open_untyped(&**service, input1.clone(), None).await;
        let editor2 = open_untyped(&**service, input2.clone(), Some(PreferredGroup::Side)).await;

        let group1 = pane_group(part, &expect_pane(editor1));
        assert_eq!(group1.count(), 1);

        let group2 = pane_group(part, &expect_pane(editor2));
        assert_eq!(group2.count(), 1);

        group2.lock(true);
        part.activate_group(GroupRef::Id(group2.id()));

        // Will open in group 1 because group 2 is locked
        open_untyped(&**service, input3.clone(), None).await;

        assert_eq!(group1.count(), 2);
        assert_eq!(active_resource(&group1), Some(resource_of(&input3)));
        assert_eq!(group2.count(), 1);

        // Will open in group 2 because group was provided
        open_untyped(&**service, input3.clone(), Some(PreferredGroup::Id(group2.id()))).await;

        assert_eq!(group1.count(), 2);
        assert_eq!(group2.count(), 2);
        assert_eq!(active_resource(&group2), Some(resource_of(&input3)));

        // Will reveal editor in group 2 because it is contained
        open_untyped(&**service, input2.clone(), Some(pref(&group2))).await;
        open_untyped(&**service, input2.clone(), Some(PreferredGroup::Active)).await;

        assert_eq!(group1.count(), 2);
        assert_eq!(group2.count(), 2);
        assert_eq!(active_resource(&group2), Some(resource_of(&input2)));

        // Will open a new group because side group is locked
        part.activate_group(GroupRef::Id(group1.id()));
        let editor3 = open_untyped(&**service, input4.clone(), Some(PreferredGroup::Side)).await;
        assert_eq!(part.count(), 3);

        let group3 = pane_group(part, &expect_pane(editor3));
        assert_eq!(group3.count(), 1);

        // Will reveal editor in group 2 because it is contained
        open_untyped(&**service, input3.clone(), Some(pref(&group2))).await;
        part.activate_group(GroupRef::Id(group1.id()));
        open_untyped(&**service, input3.clone(), Some(PreferredGroup::Side)).await;
        assert_eq!(part.count(), 3);

        // Will open a new group if all groups are locked
        group1.lock(true);
        group2.lock(true);
        group3.lock(true);

        part.activate_group(GroupRef::Id(group1.id()));
        let editor5 = open_untyped(&**service, input5.clone(), None).await;
        let group4 = pane_group(part, &expect_pane(editor5));
        assert_eq!(group4.count(), 1);
        assert_eq!(active_resource(&group4), Some(resource_of(&input5)));
        assert_eq!(part.count(), 4);

        // Will open editor in most recently non-locked group
        group1.lock(false);
        group2.lock(false);
        group3.lock(false);
        group4.lock(false);

        part.activate_group(GroupRef::Id(group3.id()));
        part.activate_group(GroupRef::Id(group2.id()));
        part.activate_group(GroupRef::Id(group4.id()));
        group4.lock(true);
        group2.lock(true);

        open_untyped(&**service, input6.clone(), None).await;
        assert_eq!(part.count(), 4);
        assert!(same_group(&part.active_group(), &group3));
        assert_eq!(active_resource(&group3), Some(resource_of(&input6)));

        // Will find the right group where editor is already opened in when all groups are locked
        group1.lock(true);
        group2.lock(true);
        group3.lock(true);
        group4.lock(true);

        part.activate_group(GroupRef::Id(group1.id()));

        open_untyped(&**service, input6.clone(), None).await;

        assert_eq!(part.count(), 4);
        assert!(same_group(&part.active_group(), &group3));
        assert_eq!(active_resource(&group3), Some(resource_of(&input6)));

        assert!(same_group(&part.active_group(), &group3));
        assert_eq!(active_resource(&group3), Some(resource_of(&input6)));

        part.activate_group(GroupRef::Id(group1.id()));

        open_untyped(&**service, input6.clone(), None).await;

        assert_eq!(part.count(), 4);
        assert!(same_group(&part.active_group(), &group3));
        assert_eq!(active_resource(&group3), Some(resource_of(&input6)));

        // Will reveal an opened editor in the active locked group
        open_untyped(&**service, input7.clone(), Some(pref(&group3))).await;
        open_untyped(&**service, input6.clone(), None).await;

        assert_eq!(part.count(), 4);
        assert!(same_group(&part.active_group(), &group3));
        assert_eq!(active_resource(&group3), Some(resource_of(&input6)));

        finish(&fixture, &disposables).await;
    });
}

#[test]
fn locked_groups_workbench_editor_reveal_if_open() {
    run_async(|disposables| async move {
        let fixture = create_editor_service(&disposables).await;
        let (part, service) = (&fixture.part, &fixture.service);

        fixture
            .accessor
            .test_configuration_service
            .set_user_configuration("workbench", Dynamic::object([("editor", Dynamic::object([("revealIfOpen", Dynamic::Bool(true))]))]), None)
            .await
            .unwrap();

        disposables.add(register_test_file_editor());

        register_locked_group_editor(&fixture, &disposables);

        let root_group = part.active_group();
        let right_group = part.add_group(Some(gref(&root_group)), GroupDirection::Right);

        part.activate_group(gref(&root_group));

        let input1 = untyped_pinned("file://resource-basics.editor-service-locked-group-tests");
        let input2 = untyped_pinned("file://resource2-basics.editor-service-locked-group-tests");
        let input3 = untyped_pinned("file://resource3-basics.editor-service-locked-group-tests");
        let input4 = untyped_pinned("file://resource4-basics.editor-service-locked-group-tests");

        open_untyped(&**service, input1.clone(), Some(PreferredGroup::Id(root_group.id()))).await;
        open_untyped(&**service, input2.clone(), Some(PreferredGroup::Id(root_group.id()))).await;

        assert_eq!(part.active_group().id(), root_group.id());

        open_untyped(&**service, input3.clone(), Some(PreferredGroup::Id(right_group.id()))).await;
        open_untyped(&**service, input4.clone(), Some(PreferredGroup::Id(right_group.id()))).await;

        assert_eq!(part.active_group().id(), right_group.id());

        root_group.lock(true);
        right_group.lock(true);

        open_untyped(&**service, input1.clone(), None).await;

        assert_eq!(part.active_group().id(), root_group.id());
        assert_eq!(part_active_resource(part), Some(res(input1.resource.as_ref().unwrap())));

        open_untyped(&**service, input3.clone(), None).await;

        assert_eq!(part.active_group().id(), right_group.id());
        assert_eq!(part_active_resource(part), Some(res(input3.resource.as_ref().unwrap())));

        assert_eq!(part.groups().len(), 2);

        finish(&fixture, &disposables).await;
    });
}

#[test]
fn locked_groups_reveal_if_visible() {
    run_async(|disposables| async move {
        disposables.add(register_test_file_editor());

        let fixture = create_editor_service(&disposables).await;
        let (part, service) = (&fixture.part, &fixture.service);

        register_locked_group_editor(&fixture, &disposables);

        let root_group = part.active_group();
        let right_group = part.add_group(Some(gref(&root_group)), GroupDirection::Right);

        part.activate_group(gref(&root_group));

        let input1 = untyped_pinned("file://resource-basics.editor-service-locked-group-tests");
        let input2 = untyped_pinned("file://resource2-basics.editor-service-locked-group-tests");
        let input3 = untyped_pinned("file://resource3-basics.editor-service-locked-group-tests");
        let input4 = untyped_pinned("file://resource4-basics.editor-service-locked-group-tests");

        open_untyped(&**service, input1.clone(), Some(PreferredGroup::Id(root_group.id()))).await;
        open_untyped(&**service, input2.clone(), Some(PreferredGroup::Id(root_group.id()))).await;

        assert_eq!(part.active_group().id(), root_group.id());

        open_untyped(&**service, input3.clone(), Some(PreferredGroup::Id(right_group.id()))).await;
        open_untyped(&**service, input4.clone(), Some(PreferredGroup::Id(right_group.id()))).await;

        assert_eq!(part.active_group().id(), right_group.id());

        root_group.lock(true);
        right_group.lock(true);

        open_untyped(&**service, with_options(&input2, |options| options.reveal_if_visible = Some(true)), None).await;

        assert_eq!(part.active_group().id(), root_group.id());
        assert_eq!(part_active_resource(part), Some(res(input2.resource.as_ref().unwrap())));

        open_untyped(&**service, with_options(&input4, |options| options.reveal_if_visible = Some(true)), None).await;

        assert_eq!(part.active_group().id(), right_group.id());
        assert_eq!(part_active_resource(part), Some(res(input4.resource.as_ref().unwrap())));

        assert_eq!(part.groups().len(), 2);

        finish(&fixture, &disposables).await;
    });
}

#[test]
fn locked_groups_reveal_if_opened() {
    run_async(|disposables| async move {
        disposables.add(register_test_file_editor());

        let fixture = create_editor_service(&disposables).await;
        let (part, service) = (&fixture.part, &fixture.service);

        register_locked_group_editor(&fixture, &disposables);

        let root_group = part.active_group();
        let right_group = part.add_group(Some(gref(&root_group)), GroupDirection::Right);

        part.activate_group(gref(&root_group));

        let input1 = untyped_pinned("file://resource-basics.editor-service-locked-group-tests");
        let input2 = untyped_pinned("file://resource2-basics.editor-service-locked-group-tests");
        let input3 = untyped_pinned("file://resource3-basics.editor-service-locked-group-tests");
        let input4 = untyped_pinned("file://resource4-basics.editor-service-locked-group-tests");

        open_untyped(&**service, input1.clone(), Some(PreferredGroup::Id(root_group.id()))).await;
        open_untyped(&**service, input2.clone(), Some(PreferredGroup::Id(root_group.id()))).await;

        assert_eq!(part.active_group().id(), root_group.id());

        open_untyped(&**service, input3.clone(), Some(PreferredGroup::Id(right_group.id()))).await;
        open_untyped(&**service, input4.clone(), Some(PreferredGroup::Id(right_group.id()))).await;

        assert_eq!(part.active_group().id(), right_group.id());

        root_group.lock(true);
        right_group.lock(true);

        open_untyped(&**service, with_options(&input1, |options| options.reveal_if_opened = Some(true)), None).await;

        assert_eq!(part.active_group().id(), root_group.id());
        assert_eq!(part_active_resource(part), Some(res(input1.resource.as_ref().unwrap())));

        open_untyped(&**service, with_options(&input3, |options| options.reveal_if_opened = Some(true)), None).await;

        assert_eq!(part.active_group().id(), right_group.id());
        assert_eq!(part_active_resource(part), Some(res(input3.resource.as_ref().unwrap())));

        assert_eq!(part.groups().len(), 2);

        finish(&fixture, &disposables).await;
    });
}

//#endregion

//#region openEditor() / openEditors() - untyped, typed

/// O estado que as fábricas do `registerEditor` de `testOpenEditors` gravam.
#[derive(Default)]
struct FactoryState {
    editor_factory_called: usize,
    untitled_editor_factory_called: usize,
    diff_editor_factory_called: usize,
    last_editor_factory_editor: Option<UntypedEditorInput>,
    last_untitled_editor_factory_editor: Option<UntypedEditorInput>,
    last_diff_editor_factory_editor: Option<UntypedEditorInput>,
}

/// `editorFactoryCalled`, `untitledEditorFactoryCalled` e `diffEditorFactoryCalled`, mais quais `last*FactoryEditor` existem.
fn assert_factory_state(state: &RefCell<FactoryState>, calls: (usize, usize, usize), last_present: (bool, bool, bool)) {
    let state = state.borrow();

    assert_eq!((state.editor_factory_called, state.untitled_editor_factory_called, state.diff_editor_factory_called), calls);
    assert_eq!(
        (state.last_editor_factory_editor.is_some(), state.last_untitled_editor_factory_editor.is_some(), state.last_diff_editor_factory_editor.is_some()),
        last_present
    );
}

/// `assert.strictEqual(last*FactoryEditor, untypedEditor)`: o mesmo recurso (e `original`/`modified`), e as opções `sticky` e
/// `preserveFocus`.
fn untyped_matches(actual: Option<&UntypedEditorInput>, expected: &UntypedEditorInput) -> bool {
    let Some(actual) = actual else {
        return false;
    };

    actual.resource == expected.resource
        && side_resource(&actual.original) == side_resource(&expected.original)
        && side_resource(&actual.modified) == side_resource(&expected.modified)
        && actual.options.as_ref().and_then(|options| options.sticky) == expected.options.as_ref().and_then(|options| options.sticky)
        && actual.options.as_ref().and_then(|options| options.preserve_focus) == expected.options.as_ref().and_then(|options| options.preserve_focus)
}

/// O recurso de um lado (`original` ou `modified`) de uma entrada de diff sem tipo.
fn side_resource(side: &Option<Box<UntypedEditorInput>>) -> Option<URI> {
    side.as_ref().and_then(|side| side.resource.clone())
}

/// O que `openEditor(editor, group)` de `testOpenEditors` recebe: uma entrada sem tipo, um `EditorInputWithOptions` ou um
/// `EditorInput` solto.
enum OpenInput {
    Untyped(UntypedEditorInput),
    WithOptions(EditorInputWithOptions),
    Typed(EditorInputRef),
}

/// `openEditor(editor, group?)` de `testOpenEditors`.
async fn open_any(service: &dyn IEditorService, use_open_editors: bool, editor: OpenInput, group: Option<PreferredGroup>) -> Option<Pane> {
    if use_open_editors {
        // The type safety isn't super good here, so we assist with runtime checks. Open editors expects untyped or editor
        // input with options, you cannot pass a typed editor input without options.
        let editor = match editor {
            OpenInput::Untyped(editor) => FindGroupEditor::Untyped(editor),
            OpenInput::WithOptions(editor) => FindGroupEditor::WithOptions(editor),
            OpenInput::Typed(editor) => FindGroupEditor::WithOptions(EditorInputWithOptions { editor, options: Some(TextEditorOptions::default()) }),
        };

        return service.open_editors_in(vec![editor], group, None).await.unwrap().into_iter().next();
    }

    match editor {
        OpenInput::WithOptions(editor) => service.open_editor_typed(&editor.editor, editor.options, group).await.unwrap(),
        OpenInput::Untyped(editor) => service.open_editor_in(editor, group).await.unwrap(),
        OpenInput::Typed(editor) => service.open_editor_typed(&editor, None, group).await.unwrap(),
    }
}

/// `{ editor: typedEditor }` e `{ editor, options }`.
fn with_opts(editor: &Rc<TestFileEditorInput>, options: Option<TextEditorOptions>) -> OpenInput {
    OpenInput::WithOptions(EditorInputWithOptions { editor: editor_ref(editor), options })
}

/// `{ sticky: true, preserveFocus: true }`, mais o `override` quando dado.
fn sticky_preserve_focus(override_id: Option<&str>) -> TextEditorOptions {
    opts(|options| {
        options.sticky = Some(true);
        options.preserve_focus = Some(true);
        options.override_ = override_id.map(override_of);
    })
}

/// `{ override: id }`.
fn override_options(id: &str) -> TextEditorOptions {
    opts(|options| options.override_ = Some(override_of(id)))
}

/// `{ resource: URI.file(name), options }` como `IResourceEditorInput`.
fn untyped_file(name: &str, options: Option<TextEditorOptions>) -> UntypedEditorInput {
    untyped(uri_file(name), options)
}

/// `{ original: { resource: URI.file(...) }, modified: { resource: URI.file(...) }, options }` como `IResourceDiffEditorInput`.
fn untyped_diff(options: TextEditorOptions) -> UntypedEditorInput {
    UntypedEditorInput {
        original: Some(Box::new(UntypedEditorInput::from_resource(uri_file("file-original.editor-service-override-tests")))),
        modified: Some(Box::new(UntypedEditorInput::from_resource(uri_file("file-modified.editor-service-override-tests")))),
        options: Some(options),
        ..UntypedEditorInput::default()
    }
}

/// `resetTestState()`.
async fn reset_test_state(fixture: &Fixture, state: &RefCell<FactoryState>) -> Group {
    *state.borrow_mut() = FactoryState::default();

    fixture.accessor.teardown().await.unwrap();

    fixture.part.active_group()
}

#[test]
fn open_editor_untyped_typed() {
    run_async(|disposables| test_open_editors(disposables, false));
}

#[test]
fn open_editors_untyped_typed() {
    run_async(|disposables| test_open_editors(disposables, true));
}

async fn test_open_editors(disposables: Rc<DisposableStore>, use_open_editors: bool) {
    disposables.add(register_test_file_editor());

    let fixture = create_editor_service(&disposables).await;
    let (part, service) = (&fixture.part, &fixture.service);
    let service: &dyn IEditorService = &**service;

    let mut root_group = part.active_group();

    let state = Rc::new(RefCell::new(FactoryState::default()));

    register_exclusive_editor(
        &fixture,
        &disposables,
        "*.editor-service-override-tests",
        EditorInputFactoryObject {
            create_editor_input: Some({
                let (state, disposables) = (state.clone(), disposables.clone());
                Rc::new(move |editor: &UntypedEditorInput, _group: &Group| {
                    let mut state = state.borrow_mut();
                    state.editor_factory_called += 1;
                    state.last_editor_factory_editor = Some(editor.clone());

                    let resource = editor.resource.clone().expect("a entrada tem recurso");
                    MaybePromise::Value(EditorInputWithOptions { editor: new_input_of(&disposables, resource, TEST_EDITOR_INPUT_ID), options: None })
                })
            }),
            create_untitled_editor_input: Some({
                let (state, disposables) = (state.clone(), disposables.clone());
                Rc::new(move |untitled_editor: &UntypedEditorInput, _group: &Group| {
                    let mut state = state.borrow_mut();
                    state.untitled_editor_factory_called += 1;
                    state.last_untitled_editor_factory_editor = Some(untitled_editor.clone());

                    let resource = untitled_editor
                        .resource
                        .clone()
                        .unwrap_or_else(|| uri(&format!("untitled://my-untitled-editor-{}", state.untitled_editor_factory_called)));
                    MaybePromise::Value(EditorInputWithOptions { editor: new_input_of(&disposables, resource, TEST_EDITOR_INPUT_ID), options: None })
                })
            }),
            create_diff_editor_input: Some({
                let (state, disposables) = (state.clone(), disposables.clone());
                Rc::new(move |diff_editor: &UntypedEditorInput, _group: &Group| {
                    let mut state = state.borrow_mut();
                    state.diff_editor_factory_called += 1;
                    state.last_diff_editor_factory_editor = Some(diff_editor.clone());

                    let resource = uri_file(&format!("diff-editor-{}", state.diff_editor_factory_called));
                    MaybePromise::Value(EditorInputWithOptions { editor: new_input_of(&disposables, resource, TEST_EDITOR_INPUT_ID), options: None })
                })
            }),
            ..EditorInputFactoryObject::default()
        },
    );

    // untyped
    {
        // untyped resource editor, no options, no group
        {
            let untyped_editor = untyped_file("file.editor-service-override-tests", None);
            let pane = open_any(service, use_open_editors, OpenInput::Untyped(untyped_editor.clone()), None).await;
            let mut typed_editor = pane.as_ref().and_then(|pane| pane.input());

            assert!(same_group(&pane_group(part, pane.as_ref().unwrap()), &root_group));
            assert!(typed_editor.as_ref().unwrap().as_any().is::<TestFileEditorInput>());
            assert_eq!(resource_string(typed_editor.as_ref().unwrap()), res(untyped_editor.resource.as_ref().unwrap()));

            assert_factory_state(&state, (1, 0, 0), (true, false, false));
            assert!(untyped_matches(state.borrow().last_editor_factory_editor.as_ref(), &untyped_editor));

            // opening the same editor should not create a new editor input
            open_any(service, use_open_editors, OpenInput::Untyped(untyped_editor.clone()), None).await;
            assert!(is_some_of(pane_group(part, pane.as_ref().unwrap()).active_editor(), typed_editor.as_ref().unwrap()));

            // replaceEditors should work too
            let untyped_editor_replacement = untyped_file("file-replaced.editor-service-override-tests", None);
            service
                .replace_editors(
                    vec![EditorReplacementInput::Untyped(UntypedEditorReplacement {
                        editor: typed_editor.clone().unwrap(),
                        replacement: untyped_editor_replacement.clone(),
                        force_replace_dirty: None,
                    })],
                    gref(&root_group),
                )
                .await
                .unwrap();

            typed_editor = root_group.active_editor();

            assert!(typed_editor.as_ref().unwrap().as_any().is::<TestFileEditorInput>());
            assert_eq!(resource_string(typed_editor.as_ref().unwrap()), res(untyped_editor_replacement.resource.as_ref().unwrap()));

            assert_factory_state(&state, (3, 0, 0), (true, false, false));
            assert!(untyped_matches(state.borrow().last_editor_factory_editor.as_ref(), &untyped_editor_replacement));

            root_group = reset_test_state(&fixture, &state).await;
        }

        // untyped resource editor, options (override text), no group
        {
            let untyped_editor = untyped_file("file.editor-service-override-tests", Some(override_options(DEFAULT_EDITOR_ASSOCIATION_ID)));
            let pane = open_any(service, use_open_editors, OpenInput::Untyped(untyped_editor.clone()), None).await;
            let typed_editor = pane.as_ref().and_then(|pane| pane.input());

            assert!(same_group(&pane_group(part, pane.as_ref().unwrap()), &root_group));
            assert!(typed_editor.as_ref().unwrap().as_any().is::<FileEditorInput>());
            assert_eq!(resource_string(typed_editor.as_ref().unwrap()), res(untyped_editor.resource.as_ref().unwrap()));

            assert_factory_state(&state, (0, 0, 0), (false, false, false));

            // opening the same editor should not create a new editor input
            open_any(service, use_open_editors, OpenInput::Untyped(untyped_editor.clone()), None).await;
            assert!(is_some_of(pane_group(part, pane.as_ref().unwrap()).active_editor(), typed_editor.as_ref().unwrap()));

            root_group = reset_test_state(&fixture, &state).await;
        }

        // untyped resource editor, options (override text, sticky: true, preserveFocus: true), no group
        {
            let untyped_editor = untyped_file("file.editor-service-override-tests", Some(sticky_preserve_focus(Some(DEFAULT_EDITOR_ASSOCIATION_ID))));
            let pane = expect_pane(open_any(service, use_open_editors, OpenInput::Untyped(untyped_editor.clone()), None).await);
            let pane_input = pane.input().unwrap();

            assert!(same_group(&pane_group(part, &pane), &root_group));
            assert!(pane_input.as_any().is::<FileEditorInput>());
            assert_eq!(resource_string(&pane_input), res(untyped_editor.resource.as_ref().unwrap()));
            assert!(pane_group(part, &pane).is_sticky(&EditorOrIndex::Editor(pane_input.clone())));

            assert_factory_state(&state, (0, 0, 0), (false, false, false));

            root_group = reset_test_state(&fixture, &state).await;
            part.active_group().close_editor(pane.input().as_ref()).await.unwrap();
        }

        // untyped resource editor, options (override default), no group
        {
            let untyped_editor = untyped_file("file.editor-service-override-tests", Some(override_options(DEFAULT_EDITOR_ASSOCIATION_ID)));
            let pane = expect_pane(open_any(service, use_open_editors, OpenInput::Untyped(untyped_editor.clone()), None).await);
            let pane_input = pane.input().unwrap();

            assert!(same_group(&pane_group(part, &pane), &root_group));
            assert!(pane_input.as_any().is::<FileEditorInput>());
            assert_eq!(resource_string(&pane_input), res(untyped_editor.resource.as_ref().unwrap()));

            assert_factory_state(&state, (0, 0, 0), (false, false, false));

            root_group = reset_test_state(&fixture, &state).await;
        }

        // untyped resource editor, options (override: TEST_EDITOR_INPUT_ID), no group
        {
            let untyped_editor = untyped_file("file.editor-service-override-tests", Some(override_options(TEST_EDITOR_INPUT_ID)));
            let pane = expect_pane(open_any(service, use_open_editors, OpenInput::Untyped(untyped_editor.clone()), None).await);
            let pane_input = pane.input().unwrap();

            assert!(same_group(&pane_group(part, &pane), &root_group));
            assert!(pane_input.as_any().is::<TestFileEditorInput>());
            assert_eq!(resource_string(&pane_input), res(untyped_editor.resource.as_ref().unwrap()));

            assert_factory_state(&state, (1, 0, 0), (true, false, false));
            assert!(untyped_matches(state.borrow().last_editor_factory_editor.as_ref(), &untyped_editor));

            root_group = reset_test_state(&fixture, &state).await;
        }

        // untyped resource editor, options (sticky: true, preserveFocus: true), no group
        {
            let untyped_editor = untyped_file("file.editor-service-override-tests", Some(sticky_preserve_focus(None)));
            let pane = expect_pane(open_any(service, use_open_editors, OpenInput::Untyped(untyped_editor.clone()), None).await);
            let pane_input = pane.input().unwrap();

            assert!(same_group(&pane_group(part, &pane), &root_group));
            assert!(pane_input.as_any().is::<TestFileEditorInput>());
            assert_eq!(resource_string(&pane_input), res(untyped_editor.resource.as_ref().unwrap()));
            assert!(pane_group(part, &pane).is_sticky(&EditorOrIndex::Editor(pane_input.clone())));

            assert_factory_state(&state, (1, 0, 0), (true, false, false));
            {
                let state = state.borrow();
                let last_editor = state.last_editor_factory_editor.as_ref().unwrap();
                assert_eq!(res(last_editor.resource.as_ref().unwrap()), res(untyped_editor.resource.as_ref().unwrap()));
                assert_eq!(last_editor.options.as_ref().and_then(|options| options.preserve_focus), Some(true));
            }

            root_group = reset_test_state(&fixture, &state).await;
            part.active_group().close_editor(pane.input().as_ref()).await.unwrap();
        }

        // untyped resource editor, options (override: TEST_EDITOR_INPUT_ID, sticky: true, preserveFocus: true), no group
        {
            let untyped_editor = untyped_file("file.editor-service-override-tests", Some(sticky_preserve_focus(Some(TEST_EDITOR_INPUT_ID))));
            let pane = expect_pane(open_any(service, use_open_editors, OpenInput::Untyped(untyped_editor.clone()), None).await);
            let pane_input = pane.input().unwrap();

            assert!(same_group(&pane_group(part, &pane), &root_group));
            assert!(pane_input.as_any().is::<TestFileEditorInput>());
            assert_eq!(resource_string(&pane_input), res(untyped_editor.resource.as_ref().unwrap()));
            assert!(pane_group(part, &pane).is_sticky(&EditorOrIndex::Editor(pane_input.clone())));

            assert_factory_state(&state, (1, 0, 0), (true, false, false));
            {
                let state = state.borrow();
                let last_editor = state.last_editor_factory_editor.as_ref().unwrap();
                assert_eq!(res(last_editor.resource.as_ref().unwrap()), res(untyped_editor.resource.as_ref().unwrap()));
                assert_eq!(last_editor.options.as_ref().and_then(|options| options.preserve_focus), Some(true));
            }

            root_group = reset_test_state(&fixture, &state).await;
            part.active_group().close_editor(pane.input().as_ref()).await.unwrap();
        }

        // untyped resource editor, no options, SIDE_GROUP
        {
            let untyped_editor = untyped_file("file.editor-service-override-tests", None);
            let pane = open_any(service, use_open_editors, OpenInput::Untyped(untyped_editor.clone()), Some(PreferredGroup::Side)).await;

            assert_eq!(part.groups().len(), 2);
            assert!(!same_group(&pane_group(part, pane.as_ref().unwrap()), &root_group));
            let pane_input = pane.as_ref().and_then(|pane| pane.input()).unwrap();
            assert!(pane_input.as_any().is::<TestFileEditorInput>());
            assert_eq!(resource_string(&pane_input), res(untyped_editor.resource.as_ref().unwrap()));

            assert_factory_state(&state, (1, 0, 0), (true, false, false));
            assert!(untyped_matches(state.borrow().last_editor_factory_editor.as_ref(), &untyped_editor));

            root_group = reset_test_state(&fixture, &state).await;
        }

        // untyped resource editor, options (override text), SIDE_GROUP
        {
            let untyped_editor = untyped_file("file.editor-service-override-tests", Some(override_options(DEFAULT_EDITOR_ASSOCIATION_ID)));
            let pane = open_any(service, use_open_editors, OpenInput::Untyped(untyped_editor.clone()), Some(PreferredGroup::Side)).await;

            assert_eq!(part.groups().len(), 2);
            assert!(!same_group(&pane_group(part, pane.as_ref().unwrap()), &root_group));
            let pane_input = pane.as_ref().and_then(|pane| pane.input()).unwrap();
            assert!(pane_input.as_any().is::<FileEditorInput>());
            assert_eq!(resource_string(&pane_input), res(untyped_editor.resource.as_ref().unwrap()));

            assert_factory_state(&state, (0, 0, 0), (false, false, false));

            root_group = reset_test_state(&fixture, &state).await;
        }
    }

    // Typed
    {
        // typed editor, no options, no group
        {
            let typed_editor = new_input_of(&disposables, uri_file("file.editor-service-override-tests"), TEST_EDITOR_INPUT_ID);
            let pane = open_any(service, use_open_editors, with_opts(&typed_editor, None), None).await;
            let mut typed_input = pane.as_ref().and_then(|pane| pane.input());

            assert!(same_group(&pane_group(part, pane.as_ref().unwrap()), &root_group));
            assert!(typed_input.as_ref().unwrap().as_any().is::<TestFileEditorInput>());
            assert_eq!(resource_string(typed_input.as_ref().unwrap()), res(&typed_editor.resource));

            // It's a typed editor input so the resolver should not have been called
            assert_factory_state(&state, (0, 0, 0), (false, false, false));

            // opening the same editor should not create a new editor input
            open_any(service, use_open_editors, OpenInput::Typed(editor_ref(&typed_editor)), None).await;
            assert!(is_some_of(pane_group(part, pane.as_ref().unwrap()).active_editor(), typed_input.as_ref().unwrap()));

            // replaceEditors should work too
            let typed_editor_replacement = new_input_of(&disposables, uri_file("file-replaced.editor-service-override-tests"), TEST_EDITOR_INPUT_ID);
            service
                .replace_editors(
                    vec![EditorReplacementInput::Typed(EditorReplacement {
                        editor: editor_ref(&typed_editor),
                        replacement: editor_ref(&typed_editor_replacement),
                        options: None,
                        force_replace_dirty: None,
                    })],
                    gref(&root_group),
                )
                .await
                .unwrap();

            typed_input = root_group.active_editor();

            assert!(typed_input.as_ref().unwrap().as_any().is::<TestFileEditorInput>());
            assert_eq!(resource_string(typed_input.as_ref().unwrap()), res(&typed_editor_replacement.resource));

            assert_factory_state(&state, (0, 0, 0), (false, false, false));

            root_group = reset_test_state(&fixture, &state).await;
        }

        // typed editor, no options, no group
        {
            let typed_editor = new_input_of(&disposables, uri_file("file.editor-service-override-tests"), TEST_EDITOR_INPUT_ID);
            let pane = open_any(service, use_open_editors, with_opts(&typed_editor, None), None).await;
            let typed_input = pane.as_ref().and_then(|pane| pane.input());

            assert!(same_group(&pane_group(part, pane.as_ref().unwrap()), &root_group));
            assert!(typed_input.as_ref().unwrap().as_any().is::<TestFileEditorInput>());
            assert_eq!(resource_string(typed_input.as_ref().unwrap()), res(&typed_editor.resource));

            assert_factory_state(&state, (0, 0, 0), (false, false, false));

            // opening the same editor should not create a new editor input
            open_any(service, use_open_editors, OpenInput::Typed(editor_ref(&typed_editor)), None).await;
            assert!(is_some_of(pane_group(part, pane.as_ref().unwrap()).active_editor(), &typed_editor));

            root_group = reset_test_state(&fixture, &state).await;
        }

        // typed editor, options (no override, sticky: true, preserveFocus: true), no group
        {
            let typed_editor = new_input_of(&disposables, uri_file("file.editor-service-override-tests"), TEST_EDITOR_INPUT_ID);
            let pane = expect_pane(open_any(service, use_open_editors, with_opts(&typed_editor, Some(sticky_preserve_focus(None))), None).await);
            let pane_input = pane.input().unwrap();

            assert!(same_group(&pane_group(part, &pane), &root_group));
            assert!(pane_input.as_any().is::<TestFileEditorInput>());
            assert_eq!(resource_string(&pane_input), res(&typed_editor.resource));
            assert!(pane_group(part, &pane).is_sticky(&EditorOrIndex::Editor(pane_input.clone())));

            assert_factory_state(&state, (0, 0, 0), (false, false, false));

            root_group = reset_test_state(&fixture, &state).await;
            part.active_group().close_editor(pane.input().as_ref()).await.unwrap();
        }

        // typed editor, options (override default), no group
        {
            let typed_editor = new_input_of(&disposables, uri_file("file.editor-service-override-tests"), TEST_EDITOR_INPUT_ID);
            let pane = expect_pane(open_any(service, use_open_editors, with_opts(&typed_editor, Some(override_options(DEFAULT_EDITOR_ASSOCIATION_ID))), None).await);
            let pane_input = pane.input().unwrap();

            assert!(same_group(&pane_group(part, &pane), &root_group));
            // We shouldn't have resolved because it is a typed editor, even though we have an override specified
            assert!(pane_input.as_any().is::<TestFileEditorInput>());
            assert_eq!(resource_string(&pane_input), res(&typed_editor.resource));

            assert_factory_state(&state, (0, 0, 0), (false, false, false));

            root_group = reset_test_state(&fixture, &state).await;
        }

        // typed editor, options (override: TEST_EDITOR_INPUT_ID), no group
        {
            let typed_editor = new_input_of(&disposables, uri_file("file.editor-service-override-tests"), TEST_EDITOR_INPUT_ID);
            let pane = expect_pane(open_any(service, use_open_editors, with_opts(&typed_editor, Some(override_options(TEST_EDITOR_INPUT_ID))), None).await);
            let pane_input = pane.input().unwrap();

            assert!(same_group(&pane_group(part, &pane), &root_group));
            assert!(pane_input.as_any().is::<TestFileEditorInput>());
            assert_eq!(resource_string(&pane_input), res(&typed_editor.resource));

            assert_factory_state(&state, (0, 0, 0), (false, false, false));

            root_group = reset_test_state(&fixture, &state).await;
        }

        // typed editor, options (sticky: true, preserveFocus: true), no group
        {
            let typed_editor = new_input_of(&disposables, uri_file("file.editor-service-override-tests"), TEST_EDITOR_INPUT_ID);
            let pane = expect_pane(open_any(service, use_open_editors, with_opts(&typed_editor, Some(sticky_preserve_focus(None))), None).await);
            let pane_input = pane.input().unwrap();

            assert!(same_group(&pane_group(part, &pane), &root_group));
            assert!(pane_input.as_any().is::<TestFileEditorInput>());
            assert_eq!(resource_string(&pane_input), res(&typed_editor.resource));
            assert!(pane_group(part, &pane).is_sticky(&EditorOrIndex::Editor(pane_input.clone())));

            assert_factory_state(&state, (0, 0, 0), (false, false, false));

            root_group = reset_test_state(&fixture, &state).await;
            part.active_group().close_editor(pane.input().as_ref()).await.unwrap();
        }

        // typed editor, options (override: TEST_EDITOR_INPUT_ID, sticky: true, preserveFocus: true), no group
        {
            let typed_editor = new_input_of(&disposables, uri_file("file.editor-service-override-tests"), TEST_EDITOR_INPUT_ID);
            let pane = expect_pane(open_any(service, use_open_editors, with_opts(&typed_editor, Some(sticky_preserve_focus(Some(TEST_EDITOR_INPUT_ID)))), None).await);
            let pane_input = pane.input().unwrap();

            assert!(same_group(&pane_group(part, &pane), &root_group));
            assert!(pane_input.as_any().is::<TestFileEditorInput>());
            assert_eq!(resource_string(&pane_input), res(&typed_editor.resource));
            assert!(pane_group(part, &pane).is_sticky(&EditorOrIndex::Editor(pane_input.clone())));

            assert_factory_state(&state, (0, 0, 0), (false, false, false));

            root_group = reset_test_state(&fixture, &state).await;
            part.active_group().close_editor(pane.input().as_ref()).await.unwrap();
        }

        // typed editor, no options, SIDE_GROUP
        {
            let typed_editor = new_input_of(&disposables, uri_file("file.editor-service-override-tests"), TEST_EDITOR_INPUT_ID);
            let pane = open_any(service, use_open_editors, with_opts(&typed_editor, None), Some(PreferredGroup::Side)).await;

            assert_eq!(part.groups().len(), 2);
            assert!(!same_group(&pane_group(part, pane.as_ref().unwrap()), &root_group));
            let pane_input = pane.as_ref().and_then(|pane| pane.input()).unwrap();
            assert!(pane_input.as_any().is::<TestFileEditorInput>());
            assert_eq!(resource_string(&pane_input), res(&typed_editor.resource));

            assert_factory_state(&state, (0, 0, 0), (false, false, false));

            root_group = reset_test_state(&fixture, &state).await;
        }

        // typed editor, options (no override), SIDE_GROUP
        {
            let typed_editor = new_input_of(&disposables, uri_file("file.editor-service-override-tests"), TEST_EDITOR_INPUT_ID);
            let pane = open_any(service, use_open_editors, with_opts(&typed_editor, None), Some(PreferredGroup::Side)).await;

            assert_eq!(part.groups().len(), 2);
            assert!(!same_group(&pane_group(part, pane.as_ref().unwrap()), &root_group));
            let pane_input = pane.as_ref().and_then(|pane| pane.input()).unwrap();
            assert!(pane_input.as_any().is::<TestFileEditorInput>());
            assert_eq!(resource_string(&pane_input), res(&typed_editor.resource));

            assert_factory_state(&state, (0, 0, 0), (false, false, false));

            root_group = reset_test_state(&fixture, &state).await;
        }
    }

    // Untyped untitled
    {
        let untitled_untyped = |options: TextEditorOptions| UntypedEditorInput { resource: None, options: Some(options), ..UntypedEditorInput::default() };
        let scheme_is_untitled = |editor: &EditorInputRef| editor.resource().is_some_and(|resource| resource.scheme().to_string_lossy() == "untitled");

        // untyped untitled editor, no options, no group
        {
            let untyped_editor = untitled_untyped(override_options(TEST_EDITOR_INPUT_ID));
            let pane = expect_pane(open_any(service, use_open_editors, OpenInput::Untyped(untyped_editor.clone()), None).await);
            let pane_input = pane.input().unwrap();

            assert!(same_group(&pane_group(part, &pane), &root_group));
            assert!(pane_input.as_any().is::<TestFileEditorInput>());
            assert!(scheme_is_untitled(&pane_input));

            assert_factory_state(&state, (0, 1, 0), (false, true, false));
            assert!(untyped_matches(state.borrow().last_untitled_editor_factory_editor.as_ref(), &untyped_editor));

            root_group = reset_test_state(&fixture, &state).await;
        }

        // untyped untitled editor, no options, SIDE_GROUP
        {
            let untyped_editor = untitled_untyped(override_options(TEST_EDITOR_INPUT_ID));
            let pane = open_any(service, use_open_editors, OpenInput::Untyped(untyped_editor.clone()), Some(PreferredGroup::Side)).await;

            assert_eq!(part.groups().len(), 2);
            assert!(!same_group(&pane_group(part, pane.as_ref().unwrap()), &root_group));
            let pane_input = pane.as_ref().and_then(|pane| pane.input()).unwrap();
            assert!(pane_input.as_any().is::<TestFileEditorInput>());
            assert!(scheme_is_untitled(&pane_input));

            assert_factory_state(&state, (0, 1, 0), (false, true, false));
            assert!(untyped_matches(state.borrow().last_untitled_editor_factory_editor.as_ref(), &untyped_editor));

            root_group = reset_test_state(&fixture, &state).await;
        }

        // untyped untitled editor with associated resource, no options, no group
        {
            let untitled_resource = uri_file("file-original.editor-service-override-tests").with(&UriChange { scheme: Field::from("untitled"), ..UriChange::default() });
            let untyped_editor = untyped(untitled_resource, None);
            let pane = open_any(service, use_open_editors, OpenInput::Untyped(untyped_editor.clone()), None).await;
            let typed_editor = pane.as_ref().and_then(|pane| pane.input());

            assert!(same_group(&pane_group(part, pane.as_ref().unwrap()), &root_group));
            assert!(typed_editor.as_ref().unwrap().as_any().is::<TestFileEditorInput>());
            assert!(scheme_is_untitled(typed_editor.as_ref().unwrap()));

            assert_factory_state(&state, (0, 1, 0), (false, true, false));
            assert!(untyped_matches(state.borrow().last_untitled_editor_factory_editor.as_ref(), &untyped_editor));

            // opening the same editor should not create a new editor input
            open_any(service, use_open_editors, OpenInput::Untyped(untyped_editor.clone()), None).await;
            assert!(is_some_of(pane_group(part, pane.as_ref().unwrap()).active_editor(), typed_editor.as_ref().unwrap()));

            root_group = reset_test_state(&fixture, &state).await;
        }

        // untyped untitled editor, options (sticky: true, preserveFocus: true), no group
        {
            let untyped_editor = untitled_untyped(sticky_preserve_focus(Some(TEST_EDITOR_INPUT_ID)));
            let pane = expect_pane(open_any(service, use_open_editors, OpenInput::Untyped(untyped_editor.clone()), None).await);
            let pane_input = pane.input().unwrap();

            assert!(same_group(&pane_group(part, &pane), &root_group));
            assert!(pane_input.as_any().is::<TestFileEditorInput>());
            assert!(scheme_is_untitled(&pane_input));
            assert!(pane_group(part, &pane).is_sticky(&EditorOrIndex::Editor(pane_input.clone())));

            assert_factory_state(&state, (0, 1, 0), (false, true, false));
            assert!(untyped_matches(state.borrow().last_untitled_editor_factory_editor.as_ref(), &untyped_editor));
            {
                let state = state.borrow();
                let options = state.last_untitled_editor_factory_editor.as_ref().unwrap().options.as_ref().unwrap();
                assert_eq!(options.preserve_focus, Some(true));
                assert_eq!(options.sticky, Some(true));
            }

            root_group = reset_test_state(&fixture, &state).await;
        }
    }

    // Untyped diff
    {
        // untyped diff editor, no options, no group
        {
            let untyped_editor = untyped_diff(override_options(TEST_EDITOR_INPUT_ID));
            let pane = open_any(service, use_open_editors, OpenInput::Untyped(untyped_editor.clone()), None).await;
            let typed_editor = pane.as_ref().and_then(|pane| pane.input());

            assert!(same_group(&pane_group(part, pane.as_ref().unwrap()), &root_group));
            assert!(typed_editor.as_ref().unwrap().as_any().is::<TestFileEditorInput>());

            assert_factory_state(&state, (0, 0, 1), (false, false, true));
            assert!(untyped_matches(state.borrow().last_diff_editor_factory_editor.as_ref(), &untyped_editor));

            root_group = reset_test_state(&fixture, &state).await;
        }

        // untyped diff editor, no options, SIDE_GROUP
        {
            let untyped_editor = untyped_diff(override_options(TEST_EDITOR_INPUT_ID));
            let pane = open_any(service, use_open_editors, OpenInput::Untyped(untyped_editor.clone()), Some(PreferredGroup::Side)).await;

            assert_eq!(part.groups().len(), 2);
            assert!(!same_group(&pane_group(part, pane.as_ref().unwrap()), &root_group));
            assert!(pane.as_ref().and_then(|pane| pane.input()).unwrap().as_any().is::<TestFileEditorInput>());

            assert_factory_state(&state, (0, 0, 1), (false, false, true));
            assert!(untyped_matches(state.borrow().last_diff_editor_factory_editor.as_ref(), &untyped_editor));

            root_group = reset_test_state(&fixture, &state).await;
        }

        // untyped diff editor, options (sticky: true, preserveFocus: true), no group
        {
            let untyped_editor = untyped_diff(sticky_preserve_focus(Some(TEST_EDITOR_INPUT_ID)));
            let pane = expect_pane(open_any(service, use_open_editors, OpenInput::Untyped(untyped_editor.clone()), None).await);
            let pane_input = pane.input().unwrap();

            assert!(same_group(&pane_group(part, &pane), &root_group));
            assert!(pane_input.as_any().is::<TestFileEditorInput>());
            assert!(pane_group(part, &pane).is_sticky(&EditorOrIndex::Editor(pane_input.clone())));
            assert_factory_state(&state, (0, 0, 1), (false, false, true));
            assert!(untyped_matches(state.borrow().last_diff_editor_factory_editor.as_ref(), &untyped_editor));
            {
                let state = state.borrow();
                let options = state.last_diff_editor_factory_editor.as_ref().unwrap().options.as_ref().unwrap();
                assert_eq!(options.preserve_focus, Some(true));
                assert_eq!(options.sticky, Some(true));
            }

            root_group = reset_test_state(&fixture, &state).await;
        }
    }

    // typed editor, not registered
    {
        // no options, no group
        {
            let typed_editor = new_input_of(&disposables, uri_file("file.something"), TEST_EDITOR_INPUT_ID);
            let pane = expect_pane(open_any(service, use_open_editors, with_opts(&typed_editor, None), None).await);
            let pane_input = pane.input().unwrap();

            assert!(same_group(&pane_group(part, &pane), &root_group));
            assert!(pane_input.as_any().is::<TestFileEditorInput>());
            assert!(is(&pane_input, &typed_editor));

            assert_factory_state(&state, (0, 0, 0), (false, false, false));

            root_group = reset_test_state(&fixture, &state).await;
        }

        // no options, SIDE_GROUP
        {
            let typed_editor = new_input_of(&disposables, uri_file("file.something"), TEST_EDITOR_INPUT_ID);
            let pane = open_any(service, use_open_editors, with_opts(&typed_editor, None), Some(PreferredGroup::Side)).await;

            assert_eq!(part.groups().len(), 2);
            assert!(!same_group(&pane_group(part, pane.as_ref().unwrap()), &root_group));
            let pane_input = pane.as_ref().and_then(|pane| pane.input()).unwrap();
            assert!(pane_input.as_any().is::<TestFileEditorInput>());
            assert!(is(&pane_input, &typed_editor));

            assert_factory_state(&state, (0, 0, 0), (false, false, false));

            root_group = reset_test_state(&fixture, &state).await;
        }
    }

    // typed editor, not supporting `toUntyped`
    {
        // no options, no group
        {
            let typed_editor = new_input_of(&disposables, uri_file("file.something"), TEST_EDITOR_INPUT_ID);
            typed_editor.disable_to_untyped.set(true);
            let pane = expect_pane(open_any(service, use_open_editors, with_opts(&typed_editor, None), None).await);
            let pane_input = pane.input().unwrap();

            assert!(same_group(&pane_group(part, &pane), &root_group));
            assert!(pane_input.as_any().is::<TestFileEditorInput>());
            assert!(is(&pane_input, &typed_editor));

            assert_factory_state(&state, (0, 0, 0), (false, false, false));

            root_group = reset_test_state(&fixture, &state).await;
        }

        // no options, SIDE_GROUP
        {
            let typed_editor = new_input_of(&disposables, uri_file("file.something"), TEST_EDITOR_INPUT_ID);
            typed_editor.disable_to_untyped.set(true);
            let pane = open_any(service, use_open_editors, with_opts(&typed_editor, None), Some(PreferredGroup::Side)).await;

            assert_eq!(part.groups().len(), 2);
            assert!(!same_group(&pane_group(part, pane.as_ref().unwrap()), &root_group));
            let pane_input = pane.as_ref().and_then(|pane| pane.input()).unwrap();
            assert!(pane_input.as_any().is::<TestFileEditorInput>());
            assert!(is(&pane_input, &typed_editor));

            assert_factory_state(&state, (0, 0, 0), (false, false, false));

            root_group = reset_test_state(&fixture, &state).await;
        }
    }

    // openEditors with >1 editor
    if use_open_editors {
        // mix of untyped and typed editors
        {
            let untyped_editor1 = untyped_file("file1.editor-service-override-tests", None);
            let untyped_editor2 = untyped_file("file2.editor-service-override-tests", None);
            let untyped_editor3 = EditorInputWithOptions {
                editor: editor_ref(&new_input_of(&disposables, uri_file("file3.editor-service-override-tests"), TEST_EDITOR_INPUT_ID)),
                options: None,
            };
            let untyped_editor4 = EditorInputWithOptions {
                editor: editor_ref(&new_input_of(&disposables, uri_file("file4.editor-service-override-tests"), TEST_EDITOR_INPUT_ID)),
                options: None,
            };
            let untyped_editor5 = untyped_file("file5.editor-service-override-tests", None);
            let pane = service
                .open_editors_in(
                    vec![
                        FindGroupEditor::Untyped(untyped_editor1),
                        FindGroupEditor::Untyped(untyped_editor2),
                        FindGroupEditor::WithOptions(untyped_editor3),
                        FindGroupEditor::WithOptions(untyped_editor4),
                        FindGroupEditor::Untyped(untyped_editor5),
                    ],
                    None,
                    None,
                )
                .await
                .unwrap()
                .into_iter()
                .next();

            assert!(same_group(&pane_group(part, pane.as_ref().unwrap()), &root_group));
            assert_eq!(pane_group(part, pane.as_ref().unwrap()).count(), 5);

            // Only the untyped editors should have had factories called (3 untyped editors)
            assert_factory_state(&state, (3, 0, 0), (true, false, false));

            root_group = reset_test_state(&fixture, &state).await;
        }
    }

    // untyped default editor
    {
        // untyped default editor, options: revealIfVisible
        {
            let untyped_editor1 = untyped_file("file-1", Some(opts(|options| {
                options.reveal_if_visible = Some(true);
                options.pinned = Some(true);
            })));
            let untyped_editor2 = untyped_file("file-2", pinned());

            let root_pane = expect_pane(open_any(service, use_open_editors, OpenInput::Untyped(untyped_editor1.clone()), None).await);
            let side_pane = expect_pane(open_any(service, use_open_editors, OpenInput::Untyped(untyped_editor2.clone()), Some(PreferredGroup::Side)).await);

            assert_eq!(pane_group(part, &root_pane).count(), 1);
            assert_eq!(pane_group(part, &side_pane).count(), 1);

            part.activate_group(GroupRef::Id(side_pane.group_id()));

            open_any(service, use_open_editors, OpenInput::Untyped(untyped_editor1.clone()), None).await;

            assert_eq!(pane_group(part, &root_pane).count(), 1);
            assert_eq!(pane_group(part, &side_pane).count(), 1);

            root_group = reset_test_state(&fixture, &state).await;
        }

        // untyped default editor, options: revealIfOpened
        {
            let untyped_editor1 = untyped_file("file-1", Some(opts(|options| {
                options.reveal_if_opened = Some(true);
                options.pinned = Some(true);
            })));
            let untyped_editor2 = untyped_file("file-2", pinned());

            let root_pane = expect_pane(open_any(service, use_open_editors, OpenInput::Untyped(untyped_editor1.clone()), None).await);
            open_any(service, use_open_editors, OpenInput::Untyped(untyped_editor2.clone()), None).await;
            assert_eq!(active_resource(&pane_group(part, &root_pane)), Some(res(untyped_editor2.resource.as_ref().unwrap())));
            let side_pane = expect_pane(open_any(service, use_open_editors, OpenInput::Untyped(untyped_editor2.clone()), Some(PreferredGroup::Side)).await);

            assert_eq!(pane_group(part, &root_pane).count(), 2);
            assert_eq!(pane_group(part, &side_pane).count(), 1);

            part.activate_group(GroupRef::Id(side_pane.group_id()));

            open_any(service, use_open_editors, OpenInput::Untyped(untyped_editor1.clone()), None).await;

            assert_eq!(pane_group(part, &root_pane).count(), 2);
            assert_eq!(pane_group(part, &side_pane).count(), 1);

            root_group = reset_test_state(&fixture, &state).await;
        }
    }
    let _ = root_group;

    finish(&fixture, &disposables).await;
}

//#endregion

#[test]
fn open_editor_applies_options_if_editor_already_opened() {
    run_async(|disposables| async move {
        disposables.add(register_test_file_editor());

        let fixture = create_editor_service(&disposables).await;
        let service = &fixture.service;

        register_exclusive_editor(&fixture, &disposables, "*.editor-service-override-tests", test_file_factories(&disposables));

        // Typed editor
        open(&**service, &new_input(&disposables, "my://resource-openEditors"), None, None).await;
        let pane = open(&**service, &new_input(&disposables, "my://resource-openEditors"), Some(sticky_preserve_focus(None)), None).await;

        let typed_pane = expect_pane(pane);
        assert_eq!(typed_pane.options().and_then(|options| options.sticky), Some(true));
        assert_eq!(typed_pane.options().and_then(|options| options.preserve_focus), Some(true));

        pane_group(&fixture.part, &typed_pane).close_all_editors(None).await.unwrap();

        // Untyped editor (without registered editor)
        open_untyped(&**service, untyped_file("resource-openEditors", None), None).await;
        let pane = expect_pane(open_untyped(&**service, untyped_file("resource-openEditors", Some(sticky_preserve_focus(None))), None).await);

        assert_eq!(pane.get_id(), TEST_TEXT_FILE_EDITOR_ID);
        assert_eq!(pane.options().and_then(|options| options.sticky), Some(true));
        assert_eq!(pane.options().and_then(|options| options.preserve_focus), Some(true));

        // Untyped editor (with registered editor)
        open_untyped(&**service, untyped_file("file.editor-service-override-tests", None), None).await;
        let pane = expect_pane(open_untyped(&**service, untyped_file("file.editor-service-override-tests", Some(sticky_preserve_focus(None))), None).await);

        assert_eq!(pane.options().and_then(|options| options.sticky), Some(true));
        assert_eq!(pane.options().and_then(|options| options.preserve_focus), Some(true));

        finish(&fixture, &disposables).await;
    });
}

#[test]
fn is_open_with_side_by_side_editor() {
    run_async(|disposables| async move {
        let fixture = create_editor_service(&disposables).await;
        let (part, service) = (&fixture.part, &fixture.service);

        let input = new_input(&disposables, "my://resource-openEditors");
        let other_input = new_input(&disposables, "my://resource2-openEditors");
        let side_by_side_input = disposables.add_rc(SideBySideEditorInput::new(Some("sideBySide"), Some(""), editor_ref(&input), editor_ref(&other_input), service.clone()));

        let editor1 = expect_pane(open(&**service, &side_by_side_input, pinned(), None).await);
        assert_eq!(part.active_group().count(), 1);

        assert!(!service.is_opened(&identifier(&input)));
        assert!(service.is_opened(&identifier(&other_input)));
        assert!(!service.is_opened(&identifier(&input)));
        assert!(service.is_opened(&identifier(&other_input)));

        let editor2 = expect_pane(open(&**service, &input, pinned(), None).await);
        assert_eq!(part.active_group().count(), 2);

        assert!(service.is_opened(&identifier(&input)));
        assert!(service.is_opened(&identifier(&other_input)));
        assert!(service.is_opened(&identifier(&input)));
        assert!(service.is_opened(&identifier(&other_input)));

        close(&pane_group(part, &editor2), &input).await;
        assert_eq!(part.active_group().count(), 1);

        assert!(!service.is_opened(&identifier(&input)));
        assert!(service.is_opened(&identifier(&other_input)));
        assert!(!service.is_opened(&identifier(&input)));
        assert!(service.is_opened(&identifier(&other_input)));

        close(&pane_group(part, &editor1), &side_by_side_input).await;

        assert!(!service.is_opened(&identifier(&input)));
        assert!(!service.is_opened(&identifier(&other_input)));
        assert!(!service.is_opened(&identifier(&input)));
        assert!(!service.is_opened(&identifier(&other_input)));

        finish(&fixture, &disposables).await;
    });
}

#[test]
fn open_editors_replace_editors() {
    run_async(|disposables| async move {
        let fixture = create_editor_service(&disposables).await;
        let (part, service) = (&fixture.part, &fixture.service);

        let input = new_input(&disposables, "my://resource-openEditors");
        let other_input = new_input(&disposables, "my://resource2-openEditors");
        let replace_input = new_input(&disposables, "my://resource3-openEditors");

        // Open editors
        open_all(&**service, &[&input, &other_input]).await;
        assert_eq!(part.active_group().count(), 2);

        // Replace editors
        service
            .replace_editors(
                vec![EditorReplacementInput::Typed(EditorReplacement {
                    editor: editor_ref(&input),
                    replacement: editor_ref(&replace_input),
                    options: None,
                    force_replace_dirty: None,
                })],
                gref(&part.active_group()),
            )
            .await
            .unwrap();
        assert_eq!(part.active_group().count(), 2);
        assert_eq!(part.active_group().get_index_of_editor(&editor_ref(&replace_input)), 0);

        finish(&fixture, &disposables).await;
    });
}

/// `new SideBySideEditorInput('side by side', undefined, input3, input4, service)` com as entradas `input1`, `input2` e o lado a lado
/// que os casos de confiança abrem.
fn trust_inputs(disposables: &Rc<DisposableStore>, service: &Rc<dyn IEditorService>) -> (Rc<TestFileEditorInput>, Rc<TestFileEditorInput>, Rc<TestFileEditorInput>, Rc<TestFileEditorInput>, Rc<SideBySideEditorInput>) {
    let input1 = new_input(disposables, "my://resource1-openEditors");
    let input2 = new_input(disposables, "my://resource2-openEditors");

    let input3 = new_input(disposables, "my://resource3-openEditors");
    let input4 = new_input(disposables, "my://resource4-openEditors");
    let side_by_side_input = disposables.add_rc(SideBySideEditorInput::new(Some("side by side"), None, editor_ref(&input3), editor_ref(&input4), service.clone()));

    (input1, input2, input3, input4, side_by_side_input)
}

/// `service.openEditors([{ editor: input1 }, { editor: input2 }, { editor: sideBySideInput }], undefined, { validateTrust })`.
async fn open_with_trust(service: &dyn IEditorService, editors: &[EditorInputRef], validate_trust: Option<bool>) {
    let editors = editors.iter().map(|editor| FindGroupEditor::WithOptions(EditorInputWithOptions { editor: editor.clone(), options: None })).collect();

    service.open_editors_in(editors, None, validate_trust.map(|validate_trust| OpenEditorsOptions { validate_trust: Some(validate_trust) })).await.unwrap();
}

#[test]
fn open_editors_handles_workspace_trust_typed_editors() {
    run_async(|disposables| async move {
        let fixture = create_editor_service(&disposables).await;
        let (part, service) = (&fixture.part, &fixture.service);

        let (input1, input2, input3, input4, side_by_side_input) = trust_inputs(&disposables, service);
        let editors = [editor_ref(&input1), editor_ref(&input2), editor_ref(&side_by_side_input)];

        let handler = &fixture.accessor.workspace_trust_request_service.request_open_uris_handler;
        let old_handler = handler.borrow().clone();

        // Trust: cancel
        let trust_editor_uris: Rc<RefCell<Vec<URI>>> = Rc::default();
        *handler.borrow_mut() = {
            let trust_editor_uris = trust_editor_uris.clone();
            Rc::new(move |uris: &[URI]| {
                *trust_editor_uris.borrow_mut() = uris.to_vec();
                Promise::resolved(WorkspaceTrustUriResponse::Cancel)
            })
        };

        open_with_trust(&**service, &editors, Some(true)).await;
        assert_eq!(part.active_group().count(), 0);
        let trust_editor_uris = trust_editor_uris.borrow().clone();
        assert_eq!(trust_editor_uris.len(), 4);
        for input in [&input1, &input2, &input3, &input4] {
            assert!(trust_editor_uris.iter().any(|uri| res(uri) == res(&input.resource)));
        }

        // Trust: open in new window
        *handler.borrow_mut() = Rc::new(|_uris: &[URI]| Promise::resolved(WorkspaceTrustUriResponse::OpenInNewWindow));

        open_with_trust(&**service, &editors, Some(true)).await;
        assert_eq!(part.active_group().count(), 0);

        // Trust: allow
        *handler.borrow_mut() = Rc::new(|_uris: &[URI]| Promise::resolved(WorkspaceTrustUriResponse::Open));

        open_with_trust(&**service, &editors, Some(true)).await;
        assert_eq!(part.active_group().count(), 3);

        *handler.borrow_mut() = old_handler;

        finish(&fixture, &disposables).await;
    });
}

#[test]
fn open_editors_ignores_trust_when_validate_trust_false() {
    run_async(|disposables| async move {
        let fixture = create_editor_service(&disposables).await;
        let (part, service) = (&fixture.part, &fixture.service);

        let (input1, input2, _input3, _input4, side_by_side_input) = trust_inputs(&disposables, service);
        let editors = [editor_ref(&input1), editor_ref(&input2), editor_ref(&side_by_side_input)];

        let handler = &fixture.accessor.workspace_trust_request_service.request_open_uris_handler;
        let old_handler = handler.borrow().clone();

        // Trust: cancel
        *handler.borrow_mut() = Rc::new(|_uris: &[URI]| Promise::resolved(WorkspaceTrustUriResponse::Cancel));

        open_with_trust(&**service, &editors, None).await;
        assert_eq!(part.active_group().count(), 3);

        *handler.borrow_mut() = old_handler;

        finish(&fixture, &disposables).await;
    });
}

#[test]
fn open_editors_extracts_proper_resources_from_untyped_editors_for_workspace_trust() {
    run_async(|disposables| async move {
        let fixture = create_editor_service(&disposables).await;
        let service = &fixture.service;

        let input = UntypedEditorInput::from_resource(uri_file("resource-openEditors"));
        let other_input = UntypedEditorInput {
            original: Some(Box::new(UntypedEditorInput::from_resource(uri("my://resource2-openEditors")))),
            modified: Some(Box::new(UntypedEditorInput::from_resource(uri("my://resource3-openEditors")))),
            ..UntypedEditorInput::default()
        };

        let handler = &fixture.accessor.workspace_trust_request_service.request_open_uris_handler;
        let old_handler = handler.borrow().clone();

        let trust_editor_uris: Rc<RefCell<Vec<URI>>> = Rc::default();
        *handler.borrow_mut() = {
            let (trust_editor_uris, old_handler) = (trust_editor_uris.clone(), old_handler.clone());
            Rc::new(move |uris: &[URI]| {
                *trust_editor_uris.borrow_mut() = uris.to_vec();
                old_handler(uris)
            })
        };

        service
            .open_editors_in(
                vec![FindGroupEditor::Untyped(input.clone()), FindGroupEditor::Untyped(other_input.clone())],
                None,
                Some(OpenEditorsOptions { validate_trust: Some(true) }),
            )
            .await
            .unwrap();
        let trust_editor_uris = trust_editor_uris.borrow().clone();
        assert_eq!(trust_editor_uris.len(), 3);
        assert!(trust_editor_uris.iter().any(|uri| res(uri) == res(input.resource.as_ref().unwrap())));
        assert!(trust_editor_uris.iter().any(|uri| res(uri) == res(other_input.original.as_ref().unwrap().resource.as_ref().unwrap())));
        assert!(trust_editor_uris.iter().any(|uri| res(uri) == res(other_input.modified.as_ref().unwrap().resource.as_ref().unwrap())));

        *handler.borrow_mut() = old_handler;

        finish(&fixture, &disposables).await;
    });
}

#[test]
fn close_editor_does_not_dispose_when_editor_opened_in_other_group() {
    run_async(|disposables| async move {
        let fixture = create_editor_service(&disposables).await;
        let (part, service) = (&fixture.part, &fixture.service);

        let input = new_input(&disposables, "my://resource-close1");

        let root_group = part.active_group();
        let right_group = part.add_group(Some(gref(&root_group)), GroupDirection::Right);

        // Open input
        open(&**service, &input, pinned(), None).await;
        open(&**service, &input, pinned(), Some(pref(&right_group))).await;

        let editors = service.editors();
        assert_eq!(editors.len(), 2);
        assert!(is(&editors[0], &input));
        assert!(is(&editors[1], &input));

        // Close input
        close(&root_group, &input).await;
        assert!(!input.is_disposed());

        close(&right_group, &input).await;
        assert!(input.is_disposed());

        finish(&fixture, &disposables).await;
    });
}

#[test]
fn open_to_the_side() {
    run_async(|disposables| async move {
        let fixture = create_editor_service(&disposables).await;
        let (part, service) = (&fixture.part, &fixture.service);

        let input1 = new_input(&disposables, "my://resource1-openside");
        let input2 = new_input(&disposables, "my://resource2-openside");

        let root_group = part.active_group();

        open(&**service, &input1, pinned(), Some(pref(&root_group))).await;
        let preserve_focus = Some(opts(|options| {
            options.pinned = Some(true);
            options.preserve_focus = Some(true);
        }));
        let editor = open(&**service, &input1, preserve_focus.clone(), Some(PreferredGroup::Side)).await;

        assert!(same_group(&part.active_group(), &root_group));
        assert_eq!(part.count(), 2);
        assert!(same_group(&pane_group(part, editor.as_ref().unwrap()), &part.groups()[1]));

        assert!(service.is_visible(&editor_ref(&input1)));
        assert!(service.is_opened(&identifier(&input1)));

        // Open to the side uses existing neighbour group if any
        let editor = open(&**service, &input2, preserve_focus, Some(PreferredGroup::Side)).await;
        assert!(same_group(&part.active_group(), &root_group));
        assert_eq!(part.count(), 2);
        assert!(same_group(&pane_group(part, editor.as_ref().unwrap()), &part.groups()[1]));

        assert!(service.is_visible(&editor_ref(&input2)));
        assert!(service.is_opened(&identifier(&input2)));

        finish(&fixture, &disposables).await;
    });
}

#[test]
fn editor_group_activation() {
    run_async(|disposables| async move {
        let fixture = create_editor_service(&disposables).await;
        let (part, service) = (&fixture.part, &fixture.service);

        let input1 = new_input(&disposables, "my://resource1-openside");
        let input2 = new_input(&disposables, "my://resource2-openside");

        let root_group = part.active_group();

        let activation = |activation: EditorActivation, preserve_focus: bool| {
            Some(opts(|options| {
                options.pinned = Some(true);
                options.preserve_focus = preserve_focus.then_some(true);
                options.activation = Some(activation);
            }))
        };

        open(&**service, &input1, pinned(), Some(pref(&root_group))).await;
        let editor = open(&**service, &input2, activation(EditorActivation::Activate, true), Some(PreferredGroup::Side)).await;
        let side_group = pane_group(part, editor.as_ref().unwrap());

        assert!(same_group(&part.active_group(), &side_group));

        open(&**service, &input1, activation(EditorActivation::Preserve, true), Some(pref(&root_group))).await;
        assert!(same_group(&part.active_group(), &side_group));

        open(&**service, &input1, activation(EditorActivation::Activate, true), Some(pref(&root_group))).await;
        assert!(same_group(&part.active_group(), &root_group));

        open(&**service, &input2, activation(EditorActivation::Preserve, false), Some(pref(&side_group))).await;
        assert!(same_group(&part.active_group(), &root_group));

        open(&**service, &input2, activation(EditorActivation::Activate, false), Some(pref(&side_group))).await;
        assert!(same_group(&part.active_group(), &side_group));

        part.arrange_groups(GroupsArrangement::Expand, None);
        open(&**service, &input1, activation(EditorActivation::Restore, true), Some(pref(&root_group))).await;
        assert!(same_group(&part.active_group(), &side_group));

        finish(&fixture, &disposables).await;
    });
}

#[test]
fn inactive_editor_group_does_not_activate_when_closing_editor_117686() {
    run_async(|disposables| async move {
        let fixture = create_editor_service(&disposables).await;
        let (part, service) = (&fixture.part, &fixture.service);

        let input1 = new_input(&disposables, "my://resource1-openside");
        let input2 = new_input(&disposables, "my://resource2-openside");

        let root_group = part.active_group();

        open(&**service, &input1, pinned(), Some(pref(&root_group))).await;
        open(&**service, &input2, pinned(), Some(pref(&root_group))).await;

        let side_group = pane_group(part, &expect_pane(open(&**service, &input2, pinned(), Some(PreferredGroup::Side)).await));
        assert!(same_group(&part.active_group(), &side_group));
        assert!(!same_group(&root_group, &side_group));

        part.arrange_groups(GroupsArrangement::Expand, Some(gref(&part.active_group())));

        close(&root_group, &input2).await;
        assert!(same_group(&part.active_group(), &side_group));

        assert!(!part.is_group_expanded(&gref(&root_group)));
        assert!(part.is_group_expanded(&gref(&part.active_group())));

        finish(&fixture, &disposables).await;
    });
}

#[test]
fn active_editor_change_visible_editor_change_events() {
    run_async(|disposables| async move {
        let fixture = create_editor_service(&disposables).await;
        let (part, service) = (&fixture.part, &fixture.service);

        let mut input = new_input(&disposables, "my://resource-active");
        let mut other_input = new_input(&disposables, "my://resource2-active");

        let (active_editor_change_event_fired, active_editor_change_listener) = flag(&service.on_did_active_editor_change());
        let (visible_editor_change_event_fired, visible_editor_change_listener) = flag(&service.on_did_visible_editors_change());

        let assert_active_editor_changed_event = |expected: bool| {
            assert_eq!(
                active_editor_change_event_fired.get(),
                expected,
                "Unexpected active editor change state (got {}, expected {expected})",
                active_editor_change_event_fired.get()
            );
            active_editor_change_event_fired.set(false);
        };

        let assert_visible_editors_changed_event = |expected: bool| {
            assert_eq!(
                visible_editor_change_event_fired.get(),
                expected,
                "Unexpected visible editors change state (got {}, expected {expected})",
                visible_editor_change_event_fired.get()
            );
            visible_editor_change_event_fired.set(false);
        };

        // closing an editor will not immediately open the next one, so we need to wait
        async fn close_editor_and_wait_for_next_to_open(group: &Group, input: &Rc<TestFileEditorInput>) {
            close(group, input).await;
            timeout(0).await.unwrap();
        }

        // 1.) open, open same, open other, close
        let editor = open(&**service, &input, pinned(), None).await;
        let group = pane_group(part, editor.as_ref().unwrap());
        assert_active_editor_changed_event(true);
        assert_visible_editors_changed_event(true);

        open(&**service, &input, None, None).await;
        assert_active_editor_changed_event(false);
        assert_visible_editors_changed_event(false);

        open(&**service, &other_input, None, None).await;
        assert_active_editor_changed_event(true);
        assert_visible_editors_changed_event(true);

        close_editor_and_wait_for_next_to_open(&group, &other_input).await;
        assert_active_editor_changed_event(true);
        assert_visible_editors_changed_event(true);

        close_editor_and_wait_for_next_to_open(&group, &input).await;
        assert_active_editor_changed_event(true);
        assert_visible_editors_changed_event(true);

        // 2.) open, open same (forced open) (recreate inputs that got disposed)
        input = new_input(&disposables, "my://resource-active");
        other_input = new_input(&disposables, "my://resource2-active");
        open(&**service, &input, None, None).await;
        assert_active_editor_changed_event(true);
        assert_visible_editors_changed_event(true);

        open(&**service, &input, Some(opts(|options| options.force_reload = Some(true))), None).await;
        assert_active_editor_changed_event(false);
        assert_visible_editors_changed_event(false);

        close_editor_and_wait_for_next_to_open(&group, &input).await;

        // 3.) open, open inactive, close (recreate inputs that got disposed)
        input = new_input(&disposables, "my://resource-active");
        other_input = new_input(&disposables, "my://resource2-active");
        open(&**service, &input, pinned(), None).await;
        assert_active_editor_changed_event(true);
        assert_visible_editors_changed_event(true);

        open(&**service, &other_input, Some(opts(|options| options.inactive = Some(true))), None).await;
        assert_active_editor_changed_event(false);
        assert_visible_editors_changed_event(false);

        group.close_all_editors(None).await.unwrap();
        assert_active_editor_changed_event(true);
        assert_visible_editors_changed_event(true);

        // 4.) open, open inactive, close inactive (recreate inputs that got disposed)
        input = new_input(&disposables, "my://resource-active");
        other_input = new_input(&disposables, "my://resource2-active");
        open(&**service, &input, pinned(), None).await;
        assert_active_editor_changed_event(true);
        assert_visible_editors_changed_event(true);

        open(&**service, &other_input, Some(opts(|options| options.inactive = Some(true))), None).await;
        assert_active_editor_changed_event(false);
        assert_visible_editors_changed_event(false);

        close_editor_and_wait_for_next_to_open(&group, &other_input).await;
        assert_active_editor_changed_event(false);
        assert_visible_editors_changed_event(false);

        group.close_all_editors(None).await.unwrap();
        assert_active_editor_changed_event(true);
        assert_visible_editors_changed_event(true);

        // 5.) add group, remove group (recreate inputs that got disposed)
        input = new_input(&disposables, "my://resource-active");
        other_input = new_input(&disposables, "my://resource2-active");
        open(&**service, &input, pinned(), None).await;
        assert_active_editor_changed_event(true);
        assert_visible_editors_changed_event(true);

        let mut right_group = part.add_group(Some(gref(&part.active_group())), GroupDirection::Right);
        assert_active_editor_changed_event(false);
        assert_visible_editors_changed_event(false);

        right_group.focus();
        assert_active_editor_changed_event(true);
        assert_visible_editors_changed_event(false);

        part.remove_group(gref(&right_group));
        assert_active_editor_changed_event(true);
        assert_visible_editors_changed_event(false);

        group.close_all_editors(None).await.unwrap();
        assert_active_editor_changed_event(true);
        assert_visible_editors_changed_event(true);

        // 6.) open editor in inactive group (recreate inputs that got disposed)
        input = new_input(&disposables, "my://resource-active");
        other_input = new_input(&disposables, "my://resource2-active");
        open(&**service, &input, pinned(), None).await;
        assert_active_editor_changed_event(true);
        assert_visible_editors_changed_event(true);

        right_group = part.add_group(Some(gref(&part.active_group())), GroupDirection::Right);
        assert_active_editor_changed_event(false);
        assert_visible_editors_changed_event(false);

        right_group.open_editor(&editor_ref(&other_input), None).await.unwrap();
        assert_active_editor_changed_event(true);
        assert_visible_editors_changed_event(true);

        close_editor_and_wait_for_next_to_open(&right_group, &other_input).await;
        assert_active_editor_changed_event(true);
        assert_visible_editors_changed_event(true);

        group.close_all_editors(None).await.unwrap();
        assert_active_editor_changed_event(true);
        assert_visible_editors_changed_event(true);

        // 7.) activate group (recreate inputs that got disposed)
        input = new_input(&disposables, "my://resource-active");
        other_input = new_input(&disposables, "my://resource2-active");
        open(&**service, &input, pinned(), None).await;
        assert_active_editor_changed_event(true);
        assert_visible_editors_changed_event(true);

        right_group = part.add_group(Some(gref(&part.active_group())), GroupDirection::Right);
        assert_active_editor_changed_event(false);
        assert_visible_editors_changed_event(false);

        right_group.open_editor(&editor_ref(&other_input), None).await.unwrap();
        assert_active_editor_changed_event(true);
        assert_visible_editors_changed_event(true);

        group.focus();
        assert_active_editor_changed_event(true);
        assert_visible_editors_changed_event(false);

        close_editor_and_wait_for_next_to_open(&right_group, &other_input).await;
        assert_active_editor_changed_event(false);
        assert_visible_editors_changed_event(true);

        group.close_all_editors(None).await.unwrap();
        assert_active_editor_changed_event(true);
        assert_visible_editors_changed_event(true);

        // 8.) move editor (recreate inputs that got disposed)
        input = new_input(&disposables, "my://resource-active");
        other_input = new_input(&disposables, "my://resource2-active");
        open(&**service, &input, pinned(), None).await;
        assert_active_editor_changed_event(true);
        assert_visible_editors_changed_event(true);

        open(&**service, &other_input, pinned(), None).await;
        assert_active_editor_changed_event(true);
        assert_visible_editors_changed_event(true);

        group.move_editor_with_options(&editor_ref(&other_input), &group, Some(EditorOptions { index: Some(0.0), ..EditorOptions::default() }));
        assert_active_editor_changed_event(false);
        assert_visible_editors_changed_event(false);

        group.close_all_editors(None).await.unwrap();
        assert_active_editor_changed_event(true);
        assert_visible_editors_changed_event(true);

        // 9.) close editor in inactive group (recreate inputs that got disposed)
        input = new_input(&disposables, "my://resource-active");
        other_input = new_input(&disposables, "my://resource2-active");
        open(&**service, &input, pinned(), None).await;
        assert_active_editor_changed_event(true);
        assert_visible_editors_changed_event(true);

        right_group = part.add_group(Some(gref(&part.active_group())), GroupDirection::Right);
        assert_active_editor_changed_event(false);
        assert_visible_editors_changed_event(false);

        right_group.open_editor(&editor_ref(&other_input), None).await.unwrap();
        assert_active_editor_changed_event(true);
        assert_visible_editors_changed_event(true);

        close_editor_and_wait_for_next_to_open(&group, &input).await;
        assert_active_editor_changed_event(false);
        assert_visible_editors_changed_event(true);

        // cleanup
        active_editor_change_listener.dispose();
        visible_editor_change_listener.dispose();

        finish(&fixture, &disposables).await;
    });
}

/// `assertEditorsChangeEvent(fn, expected)`.
async fn assert_editors_change_event<T: Clone + 'static>(service: &dyn IEditorService, events: &Cell<usize>, action: impl FnOnce() -> Promise<T>, expected: usize) {
    let promise: Promise<EditorsChangeEvent> = service.on_did_editors_change().to_promise(None).promise;
    action().await.unwrap();
    promise.await.unwrap();
    events.set(events.get() + 1);

    assert_eq!(events.get(), expected);
}

#[test]
fn editors_change_event() {
    run_async(|disposables| async move {
        let fixture = create_editor_service(&disposables).await;
        let (part, service) = (&fixture.part, &fixture.service);
        let root_group = part.active_group();

        let mut input = new_input(&disposables, "my://resource-active");
        let mut other_input = new_input(&disposables, "my://resource2-active");

        let editors_change_event_counter = Cell::new(0);

        // open
        assert_editors_change_event(&**service, &editors_change_event_counter, || service.open_editor_typed(&editor_ref(&input), pinned(), None), 1).await;

        // open (other)
        assert_editors_change_event(&**service, &editors_change_event_counter, || service.open_editor_typed(&editor_ref(&other_input), pinned(), None), 2).await;

        // close (inactive)
        assert_editors_change_event(&**service, &editors_change_event_counter, || root_group.close_editor(Some(&editor_ref(&input))), 3).await;

        // close (active)
        assert_editors_change_event(&**service, &editors_change_event_counter, || root_group.close_editor(Some(&editor_ref(&other_input))), 4).await;

        input = new_input(&disposables, "my://resource-active");
        other_input = new_input(&disposables, "my://resource2-active");

        // open editors
        assert_editors_change_event(
            &**service,
            &editors_change_event_counter,
            || {
                service.open_editors(vec![
                    EditorInputWithOptions { editor: editor_ref(&input), options: pinned() },
                    EditorInputWithOptions { editor: editor_ref(&other_input), options: pinned() },
                ])
            },
            5,
        )
        .await;

        // active editor change
        assert_editors_change_event(&**service, &editors_change_event_counter, || service.open_editor_typed(&editor_ref(&other_input), None, None), 6).await;

        // move editor (in group)
        assert_editors_change_event(
            &**service,
            &editors_change_event_counter,
            || service.open_editor_typed(&editor_ref(&input), Some(opts(|options| {
                options.pinned = Some(true);
                options.index = Some(1.0);
            })), None),
            7,
        )
        .await;

        let right_group = part.add_group(Some(gref(&part.active_group())), GroupDirection::Right);
        assert_editors_change_event(
            &**service,
            &editors_change_event_counter,
            || {
                root_group.move_editor(&editor_ref(&input), &right_group);
                Promise::resolved(())
            },
            8,
        )
        .await;

        // move group
        assert_editors_change_event(
            &**service,
            &editors_change_event_counter,
            || {
                part.move_group(gref(&right_group), gref(&root_group), GroupDirection::Left);
                Promise::resolved(())
            },
            9,
        )
        .await;

        finish(&fixture, &disposables).await;
    });
}

#[test]
fn two_active_editor_change_events_when_opening_editor_to_the_side() {
    run_async(|disposables| async move {
        let fixture = create_editor_service(&disposables).await;
        let service = &fixture.service;

        let input = new_input(&disposables, "my://resource-active");

        let active_editor_change_events = Rc::new(Cell::new(0));
        let active_editor_change_listener = {
            let active_editor_change_events = active_editor_change_events.clone();
            service.on_did_active_editor_change().listen(move |_| active_editor_change_events.set(active_editor_change_events.get() + 1))
        };

        let assert_active_editor_changed_event = |expected: usize| {
            assert_eq!(
                active_editor_change_events.get(),
                expected,
                "Unexpected active editor change state (got {}, expected {expected})",
                active_editor_change_events.get()
            );
            active_editor_change_events.set(0);
        };

        open(&**service, &input, pinned(), None).await;
        assert_active_editor_changed_event(1);

        open(&**service, &input, pinned(), Some(PreferredGroup::Side)).await;

        // we expect 2 active editor change events: one for the fact that the
        // active editor is now in the side group but also one for when the
        // editor has finished loading. we used to ignore that second change
        // event, however many listeners are interested on the active editor
        // when it has fully loaded (e.g. a model is set). as such, we cannot
        // simply ignore that second event from the editor service, even though
        // the actual editor input is the same
        assert_active_editor_changed_event(2);

        // cleanup
        active_editor_change_listener.dispose();

        finish(&fixture, &disposables).await;
    });
}

#[test]
fn open_editor_returns_undefined_when_inactive() {
    run_async(|disposables| async move {
        let fixture = create_editor_service(&disposables).await;
        let service = &fixture.service;

        let input = new_input(&disposables, "my://resource-active");
        let other_input = new_input(&disposables, "my://resource2-inactive");

        let editor = open(&**service, &input, pinned(), None).await;
        assert!(editor.is_some());

        let other_editor = open(&**service, &other_input, Some(opts(|options| options.inactive = Some(true))), None).await;
        assert!(other_editor.is_none());

        finish(&fixture, &disposables).await;
    });
}

#[test]
fn open_editor_shows_placeholder_when_opening_fails() {
    run_async(|disposables| async move {
        let fixture = create_editor_service(&disposables).await;
        let service = &fixture.service;

        let failing_input = new_input(&disposables, "my://resource-failing");
        failing_input.set_fail_to_open();

        let failing_editor = open(&**service, &failing_input, None, None).await;
        assert_eq!(expect_pane(failing_editor).get_id(), ERROR_EDITOR_ID);

        finish(&fixture, &disposables).await;
    });
}

#[test]
fn open_editor_shows_placeholder_when_restoring_fails() {
    run_async(|disposables| async move {
        let fixture = create_editor_service(&disposables).await;
        let service = &fixture.service;

        let input = new_input(&disposables, "my://resource-active");
        let failing_input = new_input(&disposables, "my://resource-failing");

        open(&**service, &input, pinned(), None).await;
        open(&**service, &failing_input, Some(opts(|options| options.inactive = Some(true))), None).await;

        failing_input.set_fail_to_open();
        let failing_editor = open(&**service, &failing_input, None, None).await;
        assert_eq!(expect_pane(failing_editor).get_id(), ERROR_EDITOR_ID);

        finish(&fixture, &disposables).await;
    });
}

/// `{ groupId, editor }`.
fn identifier_in(group: &Group, input: &Rc<TestFileEditorInput>) -> EditorIdentifier {
    EditorIdentifier { group_id: group.id(), editor: editor_ref(input) }
}

/// `input.gotSaved = false; input.gotSavedAs = false; input.gotReverted = false;`.
fn reset_got(input: &Rc<TestFileEditorInput>) {
    input.got_saved.set(false);
    input.got_saved_as.set(false);
    input.got_reverted.set(false);
}

#[test]
fn save_save_all_revert_all() {
    run_async(|disposables| async move {
        let fixture = create_editor_service(&disposables).await;
        let (part, service) = (&fixture.part, &fixture.service);

        let input1 = new_input(&disposables, "my://resource1");
        input1.dirty.set(true);
        let input2 = new_input(&disposables, "my://resource2");
        input2.dirty.set(true);
        let same_input1 = new_input(&disposables, "my://resource1");
        same_input1.dirty.set(true);

        let root_group = part.active_group();

        open(&**service, &input1, pinned(), None).await;
        open(&**service, &input2, pinned(), None).await;
        open(&**service, &same_input1, pinned(), Some(PreferredGroup::Side)).await;

        let res1 = service.save(vec![identifier_in(&root_group, &input1)], None).await.unwrap();
        assert!(res1.success);
        assert!(is_typed(&res1.editors[0], &input1));
        assert!(input1.got_saved.get());

        reset_got(&input1);

        input1.dirty.set(true);
        input2.dirty.set(true);
        same_input1.dirty.set(true);

        let res2 = service.save(vec![identifier_in(&root_group, &input1)], Some(SaveEditorsOptions { save_as: Some(true), ..SaveEditorsOptions::default() })).await.unwrap();
        assert!(res2.success);
        assert!(is_typed(&res2.editors[0], &input1));
        assert!(input1.got_saved_as.get());

        reset_got(&input1);

        input1.dirty.set(true);
        input2.dirty.set(true);
        same_input1.dirty.set(true);

        let revert_res = service.revert_all(None).await.unwrap();
        assert!(revert_res);
        assert!(input1.got_reverted.get());

        reset_got(&input1);

        input1.dirty.set(true);
        input2.dirty.set(true);
        same_input1.dirty.set(true);

        let res3 = service.save_all(None).await.unwrap();
        assert!(res3.success);
        assert_eq!(res3.editors.len(), 2);
        assert!(input1.got_saved.get());
        assert!(input2.got_saved.get());

        reset_got(&input1);
        reset_got(&input2);

        input1.dirty.set(true);
        input2.dirty.set(true);
        same_input1.dirty.set(true);

        service.save_all(Some(SaveAllEditorsOptions { save: SaveEditorsOptions { save_as: Some(true), ..SaveEditorsOptions::default() }, ..SaveAllEditorsOptions::default() })).await.unwrap();

        assert!(input1.got_saved_as.get());
        assert!(input2.got_saved_as.get());

        // services dedupes inputs automatically
        assert!(!same_input1.got_saved.get());
        assert!(!same_input1.got_saved_as.get());
        assert!(!same_input1.got_reverted.get());

        finish(&fixture, &disposables).await;
    });
}

#[test]
fn save_all_revert_all_sticky_editor() {
    run_async(|disposables| async move {
        let fixture = create_editor_service(&disposables).await;
        let service = &fixture.service;

        let input1 = new_input(&disposables, "my://resource1");
        input1.dirty.set(true);
        let input2 = new_input(&disposables, "my://resource2");
        input2.dirty.set(true);
        let same_input1 = new_input(&disposables, "my://resource1");
        same_input1.dirty.set(true);

        open(&**service, &input1, Some(opts(|options| {
            options.pinned = Some(true);
            options.sticky = Some(true);
        })), None)
        .await;
        open(&**service, &input2, pinned(), None).await;
        open(&**service, &same_input1, pinned(), Some(PreferredGroup::Side)).await;

        let revert_res = service
            .revert_all(Some(RevertAllEditorsOptions {
                base: BaseSaveRevertAllEditorOptions { exclude_sticky: Some(true), ..BaseSaveRevertAllEditorOptions::default() },
                ..RevertAllEditorsOptions::default()
            }))
            .await
            .unwrap();
        assert!(revert_res);
        assert!(!input1.got_reverted.get());
        assert!(same_input1.got_reverted.get());

        reset_got(&input1);
        reset_got(&same_input1);

        input1.dirty.set(true);
        input2.dirty.set(true);
        same_input1.dirty.set(true);

        let save_res = service.save_all(Some(SaveAllEditorsOptions { exclude_sticky: Some(true), ..SaveAllEditorsOptions::default() })).await.unwrap();
        assert!(save_res.success);
        assert_eq!(save_res.editors.len(), 2);
        assert!(!input1.got_saved.get());
        assert!(input2.got_saved.get());
        assert!(same_input1.got_saved.get());

        finish(&fixture, &disposables).await;
    });
}

#[test]
fn save_all_revert_all_untitled_exclude_untitled() {
    run_async(|disposables| async move {
        test_save_revert_untitled(&disposables, None, false, false).await;
        test_save_revert_untitled(&disposables, Some(IncludeUntitled::Bool(false)), false, false).await;
    });
}

#[test]
fn save_all_revert_all_untitled_include_untitled() {
    run_async(|disposables| async move {
        test_save_revert_untitled(&disposables, Some(IncludeUntitled::Bool(true)), true, false).await;
        test_save_revert_untitled(&disposables, Some(IncludeUntitled::Options { include_scratchpad: false }), true, false).await;
    });
}

#[test]
fn save_all_revert_all_untitled_include_scratchpad() {
    run_async(|disposables| async move {
        test_save_revert_untitled(&disposables, Some(IncludeUntitled::Options { include_scratchpad: true }), true, true).await;
    });
}

/// `testSaveRevertUntitled(options, expectUntitled, expectScratchpad)`: o `options` é o `includeUntitled` do
/// `IBaseSaveRevertAllEditorOptions`. O `createEditorService()` do corpo cria o serviço e o `teardown` é o do fim do caso (aqui
/// cada chamada derruba o seu, porque o guarda de vazamentos do porte espera os editores fechados).
async fn test_save_revert_untitled(disposables: &Rc<DisposableStore>, include_untitled: Option<IncludeUntitled>, expect_untitled: bool, expect_scratchpad: bool) {
    let fixture = create_editor_service(disposables).await;
    let service = &fixture.service;

    let input1 = new_input(disposables, "my://resource1");
    input1.dirty.set(true);
    let untitled_input = new_input(disposables, "my://resource2");
    untitled_input.dirty.set(true);
    untitled_input.set_capabilities(EditorInputCapabilities::UNTITLED);
    let scratchpad_input = new_input(disposables, "my://resource3");
    scratchpad_input.modified.set(Some(true));
    scratchpad_input.set_capabilities(EditorInputCapabilities::SCRATCHPAD | EditorInputCapabilities::UNTITLED);

    open(&**service, &input1, Some(opts(|options| {
        options.pinned = Some(true);
        options.sticky = Some(true);
    })), None)
    .await;
    open(&**service, &untitled_input, pinned(), None).await;
    open(&**service, &scratchpad_input, pinned(), None).await;

    let revert_options = RevertAllEditorsOptions {
        base: BaseSaveRevertAllEditorOptions { include_untitled, ..BaseSaveRevertAllEditorOptions::default() },
        ..RevertAllEditorsOptions::default()
    };
    let revert_res = service.revert_all(Some(revert_options)).await.unwrap();
    assert!(revert_res);
    assert!(input1.got_reverted.get());
    assert_eq!(untitled_input.got_reverted.get(), expect_untitled);
    assert_eq!(scratchpad_input.got_reverted.get(), expect_scratchpad);

    input1.got_saved.set(false);
    untitled_input.got_saved_as.set(false);
    scratchpad_input.got_reverted.set(false);

    input1.got_saved.set(false);
    untitled_input.got_saved_as.set(false);
    scratchpad_input.got_reverted.set(false);

    input1.dirty.set(true);
    untitled_input.dirty.set(true);
    scratchpad_input.modified.set(Some(true));

    let save_res = service.save_all(Some(SaveAllEditorsOptions { include_untitled, ..SaveAllEditorsOptions::default() })).await.unwrap();
    assert!(save_res.success);
    assert_eq!(save_res.editors.len(), if expect_scratchpad { 3 } else if expect_untitled { 2 } else { 1 });
    assert!(input1.got_saved.get());
    assert_eq!(untitled_input.got_saved.get(), expect_untitled);
    assert_eq!(scratchpad_input.got_saved.get(), expect_scratchpad);

    fixture.accessor.teardown().await.unwrap();
}

/// `awaitActiveEditorChange(service)`: `Event.toPromise(Event.once(editorService.onDidActiveEditorChange))`.
fn await_active_editor_change(service: &dyn IEditorService) -> je_base::promise::CancelablePromise<()> {
    service.on_did_active_editor_change().once().to_promise(None)
}

#[test]
fn file_delete_closes_editor() {
    run_async(|disposables| async move { test_file_delete_editor_close(disposables, false).await });
}

#[test]
fn file_delete_leaves_dirty_editors_open() {
    run_async(|disposables| async move { test_file_delete_editor_close(disposables, true).await });
}

#[test]
fn file_delete_closes_editor_by_preferred_resource() {
    run_async(|disposables| async move {
        let fixture = create_editor_service(&disposables).await;
        let (part, service) = (&fixture.part, &fixture.service);
        let resource = uri("my://resource");
        let input = new_input(&disposables, "test-browser://editor");
        input.override_preferred_resource(&resource);
        open(&**service, &input, pinned(), None).await;

        let active_editor_change_promise = await_active_editor_change(&**service);
        fixture.accessor.file_service.fire_after_operation(FileOperationEvent::new(resource, FileOperation::Delete, None));
        active_editor_change_promise.promise.await.unwrap();

        assert!(part.active_group().active_editor().is_none());

        finish(&fixture, &disposables).await;
    });
}

async fn test_file_delete_editor_close(disposables: Rc<DisposableStore>, dirty: bool) {
    let fixture = create_editor_service(&disposables).await;
    let (part, service) = (&fixture.part, &fixture.service);

    let input1 = new_input(&disposables, "my://resource1");
    input1.dirty.set(dirty);
    let input2 = new_input(&disposables, "my://resource2");
    input2.dirty.set(dirty);

    let root_group = part.active_group();

    open(&**service, &input1, pinned(), None).await;
    open(&**service, &input2, pinned(), None).await;

    assert!(is_some_of(root_group.active_editor(), &input2));

    let active_editor_change_promise = await_active_editor_change(&**service);
    fixture.accessor.file_service.fire_after_operation(FileOperationEvent::new(input2.resource.clone(), FileOperation::Delete, None));
    if !dirty {
        active_editor_change_promise.promise.clone().await.unwrap();
    }

    if dirty {
        assert!(is_some_of(root_group.active_editor(), &input2));
        active_editor_change_promise.cancel();
    } else {
        assert!(is_some_of(root_group.active_editor(), &input1));
    }

    finish(&fixture, &disposables).await;
}

/// O alvo `{ resource, ctime: 0, etag: '', isDirectory: false, mtime: 0, name: 'resource2', size: 0, isSymbolicLink: false,
/// readonly: false, locked: false, executable: false, children: undefined }` de um `FileOperationEvent` de `MOVE`.
fn move_target(resource: &URI) -> FileStatWithMetadata {
    FileStatWithMetadata::new(FileStat {
        resource: resource.clone(),
        name: "resource2".to_string(),
        size: Some(0),
        mtime: Some(0),
        ctime: Some(0),
        etag: Some(String::new()),
        readonly: Some(false),
        locked: Some(false),
        executable: Some(false),
        is_file: true,
        is_directory: false,
        is_symbolic_link: false,
        children: None,
    })
}

#[test]
fn file_move_asks_input_to_move() {
    run_async(|disposables| async move {
        let fixture = create_editor_service(&disposables).await;
        let (part, service) = (&fixture.part, &fixture.service);

        let input1 = new_input(&disposables, "my://resource1");
        let moved_input = new_input(&disposables, "my://resource2");
        *input1.moved_editor.borrow_mut() = Some(MoveResult { editor: EditorInputOrUntyped::Typed(editor_ref(&moved_input)), options: None });

        let root_group = part.active_group();

        open(&**service, &input1, pinned(), None).await;

        let active_editor_change_promise = await_active_editor_change(&**service);
        fixture
            .accessor
            .file_service
            .fire_after_operation(FileOperationEvent::new(input1.resource.clone(), FileOperation::Move, Some(move_target(&moved_input.resource))));
        active_editor_change_promise.promise.await.unwrap();

        assert!(is_some_of(root_group.active_editor(), &moved_input));

        finish(&fixture, &disposables).await;
    });
}

#[test]
fn file_move_asks_input_to_move_by_preferred_resource() {
    run_async(|disposables| async move {
        let fixture = create_editor_service(&disposables).await;
        let (part, service) = (&fixture.part, &fixture.service);
        let resource = uri("my://resource1");
        let target = uri("my://resource2");
        let input = new_input(&disposables, "test-browser://editor");
        input.override_preferred_resource(&resource);
        let moved_input = new_input_of(&disposables, target.clone(), TEST_EDITOR_INPUT_ID);
        *input.moved_editor.borrow_mut() = Some(MoveResult { editor: EditorInputOrUntyped::Typed(editor_ref(&moved_input)), options: None });
        open(&**service, &input, pinned(), None).await;

        let active_editor_change_promise = await_active_editor_change(&**service);
        fixture.accessor.file_service.fire_after_operation(FileOperationEvent::new(resource, FileOperation::Move, Some(move_target(&target))));
        active_editor_change_promise.promise.await.unwrap();

        assert!(is_some_of(part.active_group().active_editor(), &moved_input));

        finish(&fixture, &disposables).await;
    });
}

#[test]
fn file_move_is_only_handled_by_the_global_editor_service() {
    run_async(|disposables| async move {
        let fixture = create_editor_service(&disposables).await;
        let (part, service) = (&fixture.part, &fixture.service);
        let resource = uri("my://resource1");
        let target = uri("my://resource2");
        let moved_input = new_input_of(&disposables, target.clone(), TEST_EDITOR_INPUT_ID);
        // `new class extends TestFileEditorInput { override async rename() { renameCalls++; return { editor: movedInput }; } }`
        let input = new_input_of(&disposables, resource.clone(), TEST_EDITOR_INPUT_ID);
        *input.moved_editor.borrow_mut() = Some(MoveResult { editor: EditorInputOrUntyped::Typed(editor_ref(&moved_input)), options: None });
        let part_container: Rc<dyn IEditorPart> = part.clone();
        service.create_scoped(EditorGroupsContainer::Part(part_container), &disposables);
        open(&**service, &input, pinned(), None).await;

        let active_editor_change_promise = await_active_editor_change(&**service);
        fixture.accessor.file_service.fire_after_operation(FileOperationEvent::new(resource, FileOperation::Move, Some(move_target(&target))));
        active_editor_change_promise.promise.await.unwrap();

        assert_eq!(input.rename_calls(), 1);
        assert!(is_some_of(part.active_group().active_editor(), &moved_input));

        finish(&fixture, &disposables).await;
    });
}

#[test]
fn file_watcher_gets_installed_for_out_of_workspace_files() {
    run_async(|disposables| async move {
        let fixture = create_editor_service(&disposables).await;
        let service = &fixture.service;
        let watches = &fixture.accessor.file_service.watches;

        let input1 = new_input(&disposables, "file://resource1");
        let input2 = new_input(&disposables, "file://resource2");

        open(&**service, &input1, pinned(), None).await;
        assert_eq!(watches.borrow().len(), 1);
        assert_eq!(res(&watches.borrow()[0]), res(&input1.resource));

        let editor = expect_pane(open(&**service, &input2, pinned(), None).await);
        assert_eq!(watches.borrow().len(), 1);
        assert_eq!(res(&watches.borrow()[0]), res(&input2.resource));

        pane_group(&fixture.part, &editor).close_all_editors(None).await.unwrap();
        assert_eq!(watches.borrow().len(), 0);

        finish(&fixture, &disposables).await;
    });
}

/// O registro de `*.md` das três variantes de `editorResolverService`: o `createEditorInput` incrementa o contador e cria a
/// entrada de texto pelo `ITextEditorService`.
fn register_markdown_editor(fixture: &Fixture) -> (Rc<Cell<usize>>, DisposableRef) {
    let editor_count = Rc::new(Cell::new(0));
    let text_editor_service: Rc<dyn ITextEditorService> = fixture.accessor.text_editor_service.clone();

    let create_editor_input: EditorInputFactoryFunction = {
        let (editor_count, text_editor_service) = (editor_count.clone(), text_editor_service.clone());
        Rc::new(move |editor: &UntypedEditorInput, _group: &Group| {
            editor_count.set(editor_count.get() + 1);

            MaybePromise::Value(EditorInputWithOptions { editor: text_editor_service.create_text_editor(editor.clone()), options: None })
        })
    };
    let create_diff_editor_input: EditorInputFactoryFunction =
        Rc::new(move |diff_editor: &UntypedEditorInput, _group: &Group| MaybePromise::Value(EditorInputWithOptions { editor: text_editor_service.create_text_editor(diff_editor.clone()), options: None }));

    let registration_disposable = fixture.accessor.editor_resolver_service.register_editor(
        GlobPattern::Pattern("*.md".to_string()),
        RegisteredEditorRegistrationInfo {
            id: "TestEditor".to_string(),
            label: "Test Editor".to_string(),
            detail: Some("Test Editor Provider".to_string()),
            priority: RegisteredEditorPriority::Builtin.into(),
        },
        RegisteredEditorOptions::default(),
        EditorInputFactoryObject { create_editor_input: Some(create_editor_input), create_diff_editor_input: Some(create_diff_editor_input), ..EditorInputFactoryObject::default() },
    );

    (editor_count, registration_disposable)
}

#[test]
fn editor_resolver_service_open_editor() {
    run_async(|disposables| async move {
        let fixture = create_editor_service(&disposables).await;
        let service = &fixture.service;

        let (editor_count, registration_disposable) = register_markdown_editor(&fixture);
        assert_eq!(editor_count.get(), 0);

        let input1 = UntypedEditorInput::from_resource(uri("file://test/path/resource1.txt"));
        let input2 = UntypedEditorInput::from_resource(uri("file://test/path/resource1.md"));

        // Open editor input 1 and it shouln't trigger override as the glob doesn't match
        open_untyped(&**service, input1, None).await;
        assert_eq!(editor_count.get(), 0);

        // Open editor input 2 and it should trigger override as the glob doesn match
        open_untyped(&**service, input2.clone(), None).await;
        assert_eq!(editor_count.get(), 1);

        // Because we specify an override we shouldn't see it triggered even if it matches
        open_untyped(&**service, with_options(&input2, |options| options.override_ = Some(override_of("default"))), None).await;
        assert_eq!(editor_count.get(), 1);

        registration_disposable.dispose();

        finish(&fixture, &disposables).await;
    });
}

#[test]
fn editor_resolver_service_open_editors() {
    run_async(|disposables| async move {
        let fixture = create_editor_service(&disposables).await;
        let service = &fixture.service;

        let (editor_count, registration_disposable) = register_markdown_editor(&fixture);
        assert_eq!(editor_count.get(), 0);

        let untyped_of = |resource: &str| new_input(&disposables, resource).to_untyped(None).expect("a entrada tem forma sem tipo");
        let input1 = untyped_of("file://test/path/resource1.txt");
        let input2 = untyped_of("file://test/path/resource2.txt");
        let input3 = untyped_of("file://test/path/resource3.md");
        let input4 = untyped_of("file://test/path/resource4.md");

        // Open editor inputs
        service.open_editors_in(vec![input1, input2, input3, input4].into_iter().map(FindGroupEditor::Untyped).collect(), None, None).await.unwrap();
        // Only two matched the factory glob
        assert_eq!(editor_count.get(), 2);

        registration_disposable.dispose();

        finish(&fixture, &disposables).await;
    });
}

#[test]
fn editor_resolver_service_replace_editors() {
    run_async(|disposables| async move {
        let fixture = create_editor_service(&disposables).await;
        let (part, service) = (&fixture.part, &fixture.service);

        let (editor_count, registration_disposable) = register_markdown_editor(&fixture);

        assert_eq!(editor_count.get(), 0);

        let input1 = new_input(&disposables, "file://test/path/resource2.md");
        let untyped_input1 = input1.to_untyped(None).expect("a entrada tem forma sem tipo");

        // Open editor input 1 and it shouldn't trigger because typed inputs aren't overriden
        open(&**service, &input1, None, None).await;
        assert_eq!(editor_count.get(), 0);

        service
            .replace_editors(
                vec![EditorReplacementInput::Untyped(UntypedEditorReplacement { editor: editor_ref(&input1), replacement: untyped_input1, force_replace_dirty: None })],
                gref(&part.active_group()),
            )
            .await
            .unwrap();
        assert_eq!(editor_count.get(), 1);

        registration_disposable.dispose();

        finish(&fixture, &disposables).await;
    });
}

#[test]
fn close_editor() {
    run_async(|disposables| async move {
        let fixture = create_editor_service(&disposables).await;
        let (part, service) = (&fixture.part, &fixture.service);

        let input = new_input(&disposables, "my://resource-openEditors");
        let other_input = new_input(&disposables, "my://resource2-openEditors");

        // Open editors
        open_all(&**service, &[&input, &other_input]).await;
        assert_eq!(part.active_group().count(), 2);

        // Close editor
        service.close_editor(identifier_in(&part.active_group(), &input), None).await.unwrap();
        assert_eq!(part.active_group().count(), 1);

        service.close_editor(identifier_in(&part.active_group(), &input), None).await.unwrap();
        assert_eq!(part.active_group().count(), 1);

        service.close_editor(identifier_in(&part.active_group(), &other_input), None).await.unwrap();
        assert_eq!(part.active_group().count(), 0);

        service.close_editor(EditorIdentifier { editor: editor_ref(&other_input), group_id: 999 }, None).await.unwrap();
        assert_eq!(part.active_group().count(), 0);

        finish(&fixture, &disposables).await;
    });
}

#[test]
fn close_editors() {
    run_async(|disposables| async move {
        let fixture = create_editor_service(&disposables).await;
        let (part, service) = (&fixture.part, &fixture.service);

        let input = new_input(&disposables, "my://resource-openEditors");
        let other_input = new_input(&disposables, "my://resource2-openEditors");

        // Open editors
        open_all(&**service, &[&input, &other_input]).await;
        assert_eq!(part.active_group().count(), 2);

        // Close editors
        service.close_editors(vec![identifier_in(&part.active_group(), &input), identifier_in(&part.active_group(), &other_input)], None).await.unwrap();
        assert_eq!(part.active_group().count(), 0);

        finish(&fixture, &disposables).await;
    });
}

#[test]
fn find_editors_in_group() {
    run_async(|disposables| async move {
        let fixture = create_editor_service(&disposables).await;
        let (part, service) = (&fixture.part, &fixture.service);

        let input = new_input(&disposables, "my://resource-openEditors");
        let other_input = new_input(&disposables, "my://resource2-openEditors");

        // Open editors
        open_all(&**service, &[&input, &other_input]).await;
        assert_eq!(part.active_group().count(), 2);

        // Try using find editors for opened editors
        {
            let found1 = service.find_editors_in_group(&input.resource, None, gref(&part.active_group()));
            assert_eq!(found1.len(), 1);
            assert!(is(&found1[0], &input));

            let found2 = service.find_editor_in_group(&identifier(&input), None, gref(&part.active_group()));
            assert!(is_some_of(found2, &input));
        }
        {
            let found1 = service.find_editors_in_group(&other_input.resource, None, gref(&part.active_group()));
            assert_eq!(found1.len(), 1);
            assert!(is(&found1[0], &other_input));

            let found2 = service.find_editor_in_group(&identifier(&other_input), None, gref(&part.active_group()));
            assert!(is_some_of(found2, &other_input));
        }

        // Make sure we don't find non-opened editors
        {
            let found1 = service.find_editors_in_group(&uri("my://no-such-resource"), None, gref(&part.active_group()));
            assert_eq!(found1.len(), 0);

            let no_such_resource = ResourceEditorInputIdentifier { resource: uri("my://no-such-resource"), type_id: String::new(), editor_id: Some(TEST_EDITOR_INPUT_ID.to_string()) };
            let found2 = service.find_editor_in_group(&no_such_resource, None, gref(&part.active_group()));
            assert!(found2.is_none());
        }

        // Make sure we don't find editors across groups
        {
            let new_editor = open(
                &**service,
                &new_input(&disposables, "my://other-group-resource"),
                Some(opts(|options| {
                    options.pinned = Some(true);
                    options.preserve_focus = Some(true);
                })),
                Some(PreferredGroup::Side),
            )
            .await;
            let new_group_id = new_editor.as_ref().expect("o editor abriu").group_id();

            let found1 = service.find_editors_in_group(&input.resource, None, GroupRef::Id(new_group_id));
            assert_eq!(found1.len(), 0);

            let found2 = service.find_editor_in_group(&identifier(&input), None, GroupRef::Id(new_group_id));
            assert!(found2.is_none());
        }

        // Check we don't find editors after closing them
        part.active_group().close_all_editors(None).await.unwrap();
        {
            let found1 = service.find_editors_in_group(&input.resource, None, gref(&part.active_group()));
            assert_eq!(found1.len(), 0);

            let found2 = service.find_editor_in_group(&identifier(&input), None, gref(&part.active_group()));
            assert!(found2.is_none());
        }

        finish(&fixture, &disposables).await;
    });
}

#[test]
fn find_editors_across_groups() {
    run_async(|disposables| async move {
        let fixture = create_editor_service(&disposables).await;
        let (part, service) = (&fixture.part, &fixture.service);

        let root_group = part.active_group();

        let input = new_input(&disposables, "my://resource-openEditors");
        let other_input = new_input(&disposables, "my://resource2-openEditors");

        // Open editors
        open_all(&**service, &[&input, &other_input]).await;
        let side_editor = open(&**service, &input, pinned(), Some(PreferredGroup::Side)).await;
        let side_group = pane_group(part, side_editor.as_ref().expect("o editor abriu"));

        // Try using find editors for opened editors
        {
            let found1 = service.find_editors(&input.resource, None);
            assert_eq!(found1.len(), 2);
            assert!(is(&found1[0].editor, &input));
            assert_eq!(found1[0].group_id, side_group.id());
            assert!(is(&found1[1].editor, &input));
            assert_eq!(found1[1].group_id, root_group.id());

            let found2 = service.find_editors_by_identifier(&identifier(&input), None);
            assert_eq!(found2.len(), 2);
            assert!(is(&found2[0].editor, &input));
            assert_eq!(found2[0].group_id, side_group.id());
            assert!(is(&found2[1].editor, &input));
            assert_eq!(found2[1].group_id, root_group.id());
        }
        {
            let found1 = service.find_editors(&other_input.resource, None);
            assert_eq!(found1.len(), 1);
            assert!(is(&found1[0].editor, &other_input));
            assert_eq!(found1[0].group_id, root_group.id());

            let found2 = service.find_editors_by_identifier(&identifier(&other_input), None);
            assert_eq!(found2.len(), 1);
            assert!(is(&found2[0].editor, &other_input));
            assert_eq!(found2[0].group_id, root_group.id());
        }

        // Make sure we don't find non-opened editors
        {
            let found1 = service.find_editors(&uri("my://no-such-resource"), None);
            assert_eq!(found1.len(), 0);

            let found2 = service.find_editors_by_identifier(
                &ResourceEditorInputIdentifier { resource: uri("my://no-such-resource"), type_id: String::new(), editor_id: Some(TEST_EDITOR_INPUT_ID.to_string()) },
                None,
            );
            assert_eq!(found2.len(), 0);
        }

        // Check we don't find editors after closing them
        root_group.close_all_editors(None).await.unwrap();
        side_group.close_all_editors(None).await.unwrap();
        {
            let found1 = service.find_editors(&input.resource, None);
            assert_eq!(found1.len(), 0);

            let found2 = service.find_editors_by_identifier(&identifier(&input), None);
            assert_eq!(found2.len(), 0);
        }

        finish(&fixture, &disposables).await;
    });
}

#[test]
fn find_editors_support_side_by_side_via_options() {
    run_async(|disposables| async move {
        let fixture = create_editor_service(&disposables).await;
        let service = &fixture.service;

        let secondary_input = new_input(&disposables, "my://resource-findEditors-secondary");
        let primary_input = new_input(&disposables, "my://resource-findEditors-primary");

        let side_by_side_input = disposables.add_rc(SideBySideEditorInput::new(None, None, editor_ref(&secondary_input), editor_ref(&primary_input), service.clone()));

        open(&**service, &side_by_side_input, pinned(), None).await;

        let by_side = |side: SideBySideEditor| Some(FindEditorOptions { support_side_by_side: Some(side), order: None });

        let mut found_editors = service.find_editors(&uri("my://resource-findEditors-primary"), None);
        assert_eq!(found_editors.len(), 0);

        found_editors = service.find_editors(&uri("my://resource-findEditors-primary"), by_side(SideBySideEditor::Primary));
        assert_eq!(found_editors.len(), 1);

        found_editors = service.find_editors(&uri("my://resource-findEditors-secondary"), by_side(SideBySideEditor::Primary));
        assert_eq!(found_editors.len(), 0);

        found_editors = service.find_editors(&uri("my://resource-findEditors-primary"), by_side(SideBySideEditor::Secondary));
        assert_eq!(found_editors.len(), 0);

        found_editors = service.find_editors(&uri("my://resource-findEditors-secondary"), by_side(SideBySideEditor::Secondary));
        assert_eq!(found_editors.len(), 1);

        found_editors = service.find_editors(&uri("my://resource-findEditors-primary"), by_side(SideBySideEditor::Any));
        assert_eq!(found_editors.len(), 1);

        found_editors = service.find_editors(&uri("my://resource-findEditors-secondary"), by_side(SideBySideEditor::Any));
        assert_eq!(found_editors.len(), 1);

        finish(&fixture, &disposables).await;
    });
}

#[test]
fn side_by_side_editor_is_not_matching_all_other_editors_132859() {
    run_async(|disposables| async move {
        let fixture = create_editor_service(&disposables).await;
        let (part, service) = (&fixture.part, &fixture.service);

        let root_group = part.active_group();

        let input = new_input(&disposables, "my://resource-openEditors");
        let other_input = new_input(&disposables, "my://resource2-openEditors");
        let side_by_side_input = disposables.add_rc(SideBySideEditorInput::new(None, None, editor_ref(&input), editor_ref(&input), service.clone()));
        let other_side_by_side_input = disposables.add_rc(SideBySideEditorInput::new(None, None, editor_ref(&other_input), editor_ref(&other_input), service.clone()));

        open(&**service, &side_by_side_input, None, Some(PreferredGroup::Side)).await;

        part.activate_group(gref(&root_group));

        open(
            &**service,
            &other_side_by_side_input,
            Some(opts(|options| {
                options.reveal_if_opened = Some(true);
                options.reveal_if_visible = Some(true);
            })),
            None,
        )
        .await;

        assert_eq!(root_group.count(), 1);

        finish(&fixture, &disposables).await;
    });
}

#[test]
fn on_did_close_editor_indicates_proper_context_when_moving_editor_across_groups() {
    run_async(|disposables| async move {
        let fixture = create_editor_service(&disposables).await;
        let (part, service) = (&fixture.part, &fixture.service);

        let root_group = part.active_group();

        let input1 = new_input(&disposables, "my://resource-onDidCloseEditor1");
        let input2 = new_input(&disposables, "my://resource-onDidCloseEditor2");

        open(&**service, &input1, pinned(), None).await;
        open(&**service, &input2, pinned(), None).await;

        let sidegroup = part.add_group(Some(gref(&root_group)), GroupDirection::Right);

        let events: Rc<RefCell<Vec<EditorCloseEvent>>> = Rc::default();
        disposables.add({
            let events = events.clone();
            service.on_did_close_editor().listen(move |event| events.borrow_mut().push(event.clone()))
        });

        root_group.move_editor(&editor_ref(&input1), &sidegroup);

        assert_eq!(events.borrow()[0].context, EditorCloseContext::Move);

        close(&sidegroup, &input1).await;

        assert_eq!(events.borrow()[1].context, EditorCloseContext::Unknown);

        finish(&fixture, &disposables).await;
    });
}

#[test]
fn on_did_close_editor_indicates_proper_context_when_replacing_an_editor() {
    run_async(|disposables| async move {
        let fixture = create_editor_service(&disposables).await;
        let (part, service) = (&fixture.part, &fixture.service);

        let root_group = part.active_group();

        let input1 = new_input(&disposables, "my://resource-onDidCloseEditor1");
        let input2 = new_input(&disposables, "my://resource-onDidCloseEditor2");

        open(&**service, &input1, pinned(), None).await;

        let events: Rc<RefCell<Vec<EditorCloseEvent>>> = Rc::default();
        disposables.add({
            let events = events.clone();
            service.on_did_close_editor().listen(move |event| events.borrow_mut().push(event.clone()))
        });

        root_group
            .replace_editors(vec![EditorReplacement { editor: editor_ref(&input1), replacement: editor_ref(&input2), options: None, force_replace_dirty: None }])
            .await
            .unwrap();

        assert_eq!(events.borrow()[0].context, EditorCloseContext::Replace);

        finish(&fixture, &disposables).await;
    });
}
