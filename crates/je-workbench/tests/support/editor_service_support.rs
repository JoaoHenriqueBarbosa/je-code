//! Infraestrutura de teste de `src/vs/workbench/services/workingCopy/test/browser/workingCopyBackupTracker.test.ts`: o
//! `createEditorPart` e o `EditorService` real sobre o `workbenchInstantiationService`, que os casos que abrem editores usam
//! (`Track backups (untitled)` e `Restore backups (...)`).
//!
//! Divergências:
//! - **`workbenchInstantiationService` e `createEditorPart`.** O [`EditorServiceAccessor`] compõe o [`TextFileTestAccessor`] (o
//!   `workbenchInstantiationService` com o serviço de arquivos de texto, o `UntitledTextEditorService`, o `ModelService` e a
//!   coleção de serviços dele) e acrescenta à mesma coleção o que a parte de editores, o resolvedor, o `TextEditorService` e o
//!   `EditorService` leem. A parte é a [`EditorPart`] lógica com `create` e `layout(1080, 800, 0, 0)`. O `TestEditorParts` (e
//!   com ele o `EditorParts`) não existe nesta fatia: o `IEditorGroupsService` é o [`PartGroupsService`], um adaptador sobre a
//!   parte principal (no original, `instantiationService.stub(IEditorGroupsService, part)` registra a própria parte).
//! - **`IEditorService`, `ITextEditorService` e `IEditorResolverService`.** São os reais: [`EditorService`],
//!   [`TextEditorService`] e [`EditorResolverService`], criados pelo serviço de instanciação.
//! - **`ITextModelService`.** O `TextModelResolverService` real ainda não foi portado: o [`UntitledTextModelService`] faz o que
//!   ele faz para o esquema `untitled` (resolve o modelo pelo serviço de sem título e o descarta quando a última referência é
//!   liberada, depois que o `canDispose` deixa).
//! - **`IWorkingCopyEditorService`.** O real não foi portado: vale o dublê do [`TestServiceAccessor`], que guarda os handlers
//!   (`findEditor` não é exercitado pelo rastreador de backups).
//! - **`registerTestResourceEditor`.** O painel de teste do recurso de texto é registrado para as entradas sem título e de
//!   recurso de texto, mais a [`TestUntitledTextEditorInput`]: o painel lógico escolhe pelo `TypeId` exato da entrada, e a
//!   subclasse do original (achada pela cadeia de protótipos) é um tipo à parte aqui.
//! - **`TestUntitledTextEditorInput extends UntitledTextEditorInput`.** O Rust não tem herança: a entrada de teste guarda uma
//!   `UntitledTextEditorInput` e delega a ela todo membro do contrato `EditorInput` que ela sobrescreve, mudando só o `resolve`
//!   (que marca `resolved`), o `as_any` e o `this` (a identidade é a da entrada de teste), e o `matches` (a identidade e o
//!   `instanceof UntitledTextEditorInput` valem para as duas classes).
//! - **`workbenchTeardown`.** É [`EditorServiceAccessor::teardown`].
//!
//! Acrescentado para `editorService.test.ts`:
//! - **`registerTestFileEditor` e `registerTestSideBySideEditor`.** [`register_test_file_editor`] registra o painel
//!   `TestTextFileEditor` (só a identidade `getId()`, o painel lógico não tem controle) para o `FileEditorInput`, e
//!   [`register_test_side_by_side_editor`] o painel lado a lado para o `SideBySideEditorInput`.
//! - **`IEditorPaneService`, `IEditorResolverService`, `ITextEditorService` e `IWorkspaceTrustRequestService`.** O acessor os
//!   expõe (`editor_pane_service`, `editor_resolver_service`, `text_editor_service`, `workspace_trust_request_service`), como
//!   os membros do `TestServiceAccessor` do original; [`EditorServiceAccessor::with_default_backup_service`] é o
//!   `workbenchInstantiationService(undefined, disposables)` sem trocar o serviço de backup.

use std::any::{Any, TypeId};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ops::Deref;
use std::rc::{Rc, Weak};

use je_base::async_::async_fn;
use je_base::event::Event;
use je_base::lifecycle::{DisposableRef, DisposableStore, IDisposable, id_of, id_of_ref, to_disposable};
use je_base::promise::Promise;
use je_base::runtime;
use je_base::uri::URI;
use je_instantiation::InstantiationService;
use je_platform::commands::commands::{ICommandService, I_COMMAND_SERVICE};
use je_platform::configuration::service::IConfigurationService;
use je_platform::dialogs::{I_DIALOG_SERVICE, I_FILE_DIALOG_SERVICE, IDialogService, IFileDialogService};
use je_platform::keybinding::mock_keybinding_service::MockContextKeyService;
use je_platform::log::I_LOG_SERVICE;
use je_platform::notification::notification::{INotificationService, I_NOTIFICATION_SERVICE};
use je_platform::opener::editor_types::{AuxiliaryEditorOptions, ModalEditorPartOptions};
use je_platform::resources::ext_uri;
use je_platform::storage::{I_STORAGE_SERVICE, InMemoryStorageService, StorageService};
use je_platform::uri_identity::uri_identity::{IUriIdentityService, I_URI_IDENTITY_SERVICE};
use je_platform::workspace::{IWorkspaceContextService, I_WORKSPACE_CONTEXT_SERVICE};
use je_platform::workspace_trust::{IWorkspaceTrustRequestService, I_WORKSPACE_TRUST_REQUEST_SERVICE};
use je_ui::Ui;
use je_workbench::browser::parts::editor::editor_panes::{EditorPaneDescriptor, editor_pane_registry};
use je_workbench::browser::parts::editor::editor_part::EditorPart;
use je_workbench::common::editor::editor_input::{EditorInput, EditorInputBase, UntypedEditorOptions, matches_by_default};
use je_workbench::common::editor::text_resource_editor_input::TextResourceEditorInput;
use je_workbench::common::editor::editor_model::IEditorModel;
use je_workbench::common::editor::{
    EditorInputCapabilities, EditorInputOrUntyped, EditorInputRef, GroupIdentifier, RevertOptions, SIDE_BY_SIDE_EDITOR_ID, SaveOptions, UntypedEditorInput, Verbosity,
    is_untitled_resource_editor_input,
};
use je_workbench::common::editor::side_by_side_editor_input::SideBySideEditorInput;
use je_workbench::contrib::files::browser::editors::file_editor_input::FileEditorInput;
use je_workbench::services::editor::editor_pane_service::{EditorPaneService, IEditorPaneService, I_EDITOR_PANE_SERVICE};
use je_workbench::services::files_configuration::files_configuration_service::Readonly;
use je_workbench::services::editor::browser_editor_resolver_service::EditorResolverService;
use je_workbench::services::editor::editor_groups_service::{
    EditorGroupActivationEvent, EditorPartOptions, EditorPartOptionsChangeEvent, FindGroupScope, GroupDirection, GroupRef, GroupsOrder, I_EDITOR_GROUPS_SERVICE, IEditorGroup,
    IEditorGroupsService, IEditorPart, IModalEditorPart,
};
use je_workbench::services::editor::editor_resolver_service::{IEditorResolverService, I_EDITOR_RESOLVER_SERVICE};
use je_workbench::services::editor::editor_service::{IEditorService, I_EDITOR_SERVICE};
use je_workbench::services::editor::editor_service_impl::EditorService;
use je_workbench::services::environment::environment_service::{IWorkbenchEnvironmentService, I_WORKBENCH_ENVIRONMENT_SERVICE};
use je_workbench::services::extensions::extensions::{IExtensionService, I_EXTENSION_SERVICE};
use je_workbench::services::host::host::{IHostService, I_HOST_SERVICE};
use je_workbench::services::layout::layout_service::MAIN_WINDOW;
use je_workbench::services::textfile::text_editor_service::{ITextEditorService, I_TEXT_EDITOR_SERVICE, TextEditorService};
use je_workbench::services::textmodel_resolver::resolver_service::{ITextModelService, TextModelReference, I_TEXT_MODEL_SERVICE};
use je_workbench::services::untitled::untitled_text_editor_input::UntitledTextEditorInput;
use je_workbench::services::untitled::untitled_text_editor_model::IUntitledTextEditorModel;
use je_workbench::services::untitled::untitled_text_editor_service::{
    IUntitledTextEditorService, I_UNTITLED_TEXT_EDITOR_SERVICE, UntitledTextEditorOptions, UntitledTextEditorService,
};
use je_workbench::services::working_copy::working_copy_backup::{IWorkingCopyBackupService, I_WORKING_COPY_BACKUP_SERVICE};
use je_workbench::services::working_copy::working_copy_service::IWorkingCopyService;
use je_workbench::test::browser::workbench_test_services::{TestEnvironmentService, TestExtensionService, TestHostService};
use je_workbench::test::common::workbench_test_services::TestWorkspaceTrustRequestService;

use super::mock_command_service::MockCommandService;
use super::text_file_support::TextFileTestAccessor;

//#region PartGroupsService

/// O `IEditorGroupsService` sobre a parte principal: no original a parte é o próprio serviço (`instantiationService.stub(
/// IEditorGroupsService, part)`), então todo membro do contrato do container vai para ela.
pub struct PartGroupsService {
    pub part: Weak<EditorPart>,
}

impl PartGroupsService {
    fn part(&self) -> Rc<EditorPart> {
        self.part.upgrade().expect("a parte está viva enquanto o serviço é usado")
    }
}

impl IEditorGroupsService for PartGroupsService {
    fn active_group(&self) -> Rc<dyn IEditorGroup> {
        self.part().active_group()
    }

    fn groups(&self) -> Vec<Rc<dyn IEditorGroup>> {
        self.part().groups()
    }

    fn get_groups(&self, order: GroupsOrder) -> Vec<Rc<dyn IEditorGroup>> {
        self.part().get_groups(order)
    }

    fn get_group(&self, identifier: GroupIdentifier) -> Option<Rc<dyn IEditorGroup>> {
        self.part().get_group(identifier)
    }

    fn find_group(&self, scope: &FindGroupScope) -> Option<Rc<dyn IEditorGroup>> {
        self.part().find_group(scope)
    }

    fn add_group(&self, location: Option<GroupRef>, direction: GroupDirection) -> Rc<dyn IEditorGroup> {
        self.part().add_group(location, direction)
    }

    fn main_part(&self) -> Rc<dyn IEditorPart> {
        self.part()
    }

    fn create_auxiliary_editor_part(&self, _options: Option<AuxiliaryEditorOptions>) -> Promise<Rc<dyn IEditorPart>> {
        panic!("Method not implemented.")
    }

    fn create_modal_editor_part(&self, _options: Option<ModalEditorPartOptions>) -> Promise<Rc<dyn IModalEditorPart>> {
        panic!("Method not implemented.")
    }

    fn active_modal_editor_part(&self) -> Option<Rc<dyn IModalEditorPart>> {
        None
    }

    fn on_did_change_active_group(&self) -> Event<Rc<dyn IEditorGroup>> {
        self.part().on_did_change_active_group()
    }

    fn on_did_add_group(&self) -> Event<Rc<dyn IEditorGroup>> {
        self.part().on_did_add_group()
    }

    fn on_did_remove_group(&self) -> Event<Rc<dyn IEditorGroup>> {
        self.part().on_did_remove_group()
    }

    fn on_did_activate_group(&self) -> Event<EditorGroupActivationEvent> {
        self.part().on_did_activate_group()
    }

    fn on_did_change_editor_part_options(&self) -> Event<EditorPartOptionsChangeEvent> {
        self.part().on_did_change_editor_part_options()
    }

    fn is_ready(&self) -> bool {
        self.part().is_ready()
    }

    fn when_ready(&self) -> Promise<()> {
        self.part().when_ready()
    }

    fn when_restored(&self) -> Promise<()> {
        self.part().when_restored()
    }

    fn count(&self) -> usize {
        self.part().count()
    }

    fn part_options(&self) -> EditorPartOptions {
        self.part().part_options()
    }

    fn remove_group(&self, group: GroupRef) {
        self.part().remove_group(group);
    }
}

//#endregion

//#region UntitledTextModelService

/// O `TextModelResolverService` para o esquema `untitled`: resolve o modelo pelo serviço de sem título e o descarta quando a
/// última referência é liberada, depois que o `canDispose` do serviço deixa (`destroyReferencedObject`).
pub struct UntitledTextModelService {
    untitled: Rc<UntitledTextEditorService>,
    references: Rc<RefCell<HashMap<String, usize>>>,
}

impl UntitledTextModelService {
    pub fn new(untitled: Rc<UntitledTextEditorService>) -> Rc<Self> {
        Rc::new(Self { untitled, references: Rc::default() })
    }
}

impl ITextModelService for UntitledTextModelService {
    fn create_model_reference(&self, resource: &URI) -> Promise<TextModelReference> {
        let untitled = self.untitled.clone();
        let references = self.references.clone();
        let key = resource.to_string(false).to_string_lossy();
        *references.borrow_mut().entry(key.clone()).or_insert(0) += 1;

        let resource = resource.clone();
        async_fn(async move {
            let model = IUntitledTextEditorService::resolve(
                untitled.as_ref(),
                Some(UntitledTextEditorOptions { untitled_resource: Some(resource), ..UntitledTextEditorOptions::default() }),
            )
            .await?;

            let model_to_release = model.clone();
            let release = to_disposable(move || {
                let remaining = {
                    let mut references = references.borrow_mut();
                    let count = references.get_mut(&key).expect("a referência foi adquirida");
                    *count -= 1;
                    *count
                };
                if remaining > 0 {
                    return;
                }
                references.borrow_mut().remove(&key);

                // `destroyReferencedObject` é `async`: espera o `modelPromise` antes de consultar o `canDispose`, então a
                // decisão acontece numa microtarefa (o `await modelPromise` do original).
                let untitled = untitled.clone();
                runtime::queue_microtask(move || {
                    runtime::spawn_local(async move {
                        // untitled file models have conditions that prevent them from dispose, so we have to wait until we can dispose
                        let can_dispose = match IUntitledTextEditorService::can_dispose(untitled.as_ref(), &model_to_release) {
                            None => true,
                            Some(can_dispose) => can_dispose.await.is_ok(),
                        };
                        if can_dispose {
                            model_to_release.dispose();
                        }
                    });
                });
            });

            Ok(TextModelReference::new(model, release))
        })
    }
}

//#endregion

//#region TestUntitledTextEditorInput

/// `TestUntitledTextEditorInput extends UntitledTextEditorInput`: o `resolve` marca `resolved`.
pub struct TestUntitledTextEditorInput {
    this: Weak<TestUntitledTextEditorInput>,
    input: Rc<UntitledTextEditorInput>,
    resolved: Cell<bool>,
}

impl TestUntitledTextEditorInput {
    /// `instantiationService.createInstance(TestUntitledTextEditorInput, model)`.
    pub fn new(instantiation_service: &InstantiationService, model: Rc<dyn IUntitledTextEditorModel>) -> Rc<Self> {
        let input = instantiation_service.create_instance::<UntitledTextEditorInput>((model,));

        Rc::new_cyclic(|this| Self { this: this.clone(), input, resolved: Cell::new(false) })
    }

    /// `resolved`.
    pub fn resolved(&self) -> bool {
        self.resolved.get()
    }

    /// `editor.resolved = value`.
    pub fn set_resolved(&self, resolved: bool) {
        self.resolved.set(resolved);
    }
}

impl IDisposable for TestUntitledTextEditorInput {
    fn dispose(&self) {
        self.input.dispose();
    }
}

impl EditorInput for TestUntitledTextEditorInput {
    fn base(&self) -> &EditorInputBase {
        EditorInput::base(self.input.as_ref())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn this(&self) -> EditorInputRef {
        self.this.upgrade().expect("a entrada está viva enquanto é usada")
    }

    fn editor_type_id(&self) -> String {
        EditorInput::editor_type_id(self.input.as_ref())
    }

    fn editor_id(&self) -> Option<String> {
        EditorInput::editor_id(self.input.as_ref())
    }

    fn resource(&self) -> Option<URI> {
        EditorInput::resource(self.input.as_ref())
    }

    fn preferred_resource(&self) -> Option<URI> {
        EditorInput::preferred_resource(self.input.as_ref())
    }

    fn capabilities(&self) -> EditorInputCapabilities {
        EditorInput::capabilities(self.input.as_ref())
    }

    fn is_readonly(&self) -> Readonly {
        EditorInput::is_readonly(self.input.as_ref())
    }

    fn get_name(&self) -> String {
        EditorInput::get_name(self.input.as_ref())
    }

    fn get_description(&self, verbosity: Option<Verbosity>) -> Option<String> {
        EditorInput::get_description(self.input.as_ref(), verbosity)
    }

    fn get_title(&self, verbosity: Option<Verbosity>) -> String {
        EditorInput::get_title(self.input.as_ref(), verbosity)
    }

    fn is_dirty(&self) -> bool {
        EditorInput::is_dirty(self.input.as_ref())
    }

    /// `override resolve() { this.resolved = true; return super.resolve(); }`.
    fn resolve(&self) -> Promise<Option<Rc<dyn IEditorModel>>> {
        self.resolved.set(true);

        EditorInput::resolve(self.input.as_ref())
    }

    fn save(&self, group: GroupIdentifier, options: Option<SaveOptions>) -> Promise<Option<EditorInputOrUntyped>> {
        EditorInput::save(self.input.as_ref(), group, options)
    }

    fn save_as(&self, group: GroupIdentifier, options: Option<SaveOptions>) -> Promise<Option<EditorInputOrUntyped>> {
        EditorInput::save_as(self.input.as_ref(), group, options)
    }

    fn revert(&self, group: GroupIdentifier, options: Option<RevertOptions>) -> Promise<()> {
        EditorInput::revert(self.input.as_ref(), group, options)
    }

    fn to_untyped(&self, options: Option<&UntypedEditorOptions>) -> Option<UntypedEditorInput> {
        EditorInput::to_untyped(self.input.as_ref(), options)
    }

    /// O `matches` da `UntitledTextEditorInput`, onde o `instanceof UntitledTextEditorInput` vale também para esta subclasse.
    fn matches(&self, other_input: &EditorInputOrUntyped) -> bool {
        if let EditorInputOrUntyped::Typed(other) = other_input {
            if id_of_ref(self) == id_of(other) {
                return true;
            }

            let other_resource = match other.as_any().downcast_ref::<Self>() {
                Some(other) => Some(&*other.input),
                None => other.as_any().downcast_ref::<UntitledTextEditorInput>(),
            }
            .and_then(|other| EditorInput::resource(other));
            if other_resource.is_some() {
                return ext_uri().is_equal(other_resource.as_ref(), EditorInput::resource(self.input.as_ref()).as_ref(), false);
            }
        }

        if is_untitled_resource_editor_input(Some(other_input)) {
            return matches_by_default(self, other_input);
        }

        false
    }
}

//#endregion

/// `TestTextResourceEditor.ID`.
const TEST_TEXT_RESOURCE_EDITOR_ID: &str = "testTextResourceEditor";

/// `TestTextFileEditor.ID`.
pub const TEST_TEXT_FILE_EDITOR_ID: &str = "testTextFileEditor";

/// `registerTestResourceEditor()`: o painel de teste do recurso de texto para as entradas sem título e de recurso de texto
/// (mais a [`TestUntitledTextEditorInput`], ver o cabeçalho).
pub fn register_test_resource_editor() -> DisposableRef {
    editor_pane_registry().register_editor_pane(
        EditorPaneDescriptor::new(TEST_TEXT_RESOURCE_EDITOR_ID, "Text Editor"),
        vec![TypeId::of::<UntitledTextEditorInput>(), TypeId::of::<TextResourceEditorInput>(), TypeId::of::<TestUntitledTextEditorInput>()],
    )
}

/// `registerTestFileEditor()`: o painel `TestTextFileEditor` para o `FileEditorInput`. O `TestTextFileEditor` do original cria
/// um `TestCodeEditor`; o painel lógico não tem controle, só a identidade (`getId()`).
pub fn register_test_file_editor() -> DisposableRef {
    editor_pane_registry().register_editor_pane(EditorPaneDescriptor::new(TEST_TEXT_FILE_EDITOR_ID, "Text File Editor"), vec![TypeId::of::<FileEditorInput>()])
}

/// `registerTestSideBySideEditor()`: o painel lado a lado para o `SideBySideEditorInput`.
pub fn register_test_side_by_side_editor() -> DisposableRef {
    editor_pane_registry().register_editor_pane(EditorPaneDescriptor::new(SIDE_BY_SIDE_EDITOR_ID, "Text Editor"), vec![TypeId::of::<SideBySideEditorInput>()])
}

//#region EditorServiceAccessor

/// `workbenchInstantiationService(undefined, disposables)` com o `IWorkingCopyBackupService` dado, o `createEditorPart`, o
/// `EditorService` real e o `TestServiceAccessor` sobre eles. O que o acessor do arquivo de texto entrega vem dele por `Deref`
/// (`untitled_text_editor_service`, `working_copy_service`, `instantiation_service`, `lifecycle_service`, ...).
pub struct EditorServiceAccessor {
    base: TextFileTestAccessor,
    /// A parte principal de `createEditorPart`.
    pub part: Rc<EditorPart>,
    /// `IEditorGroupsService`.
    pub editor_groups_service: Rc<dyn IEditorGroupsService>,
    /// `editorService`.
    pub editor_service: Rc<EditorService>,
    /// `editorResolverService`.
    pub editor_resolver_service: Rc<EditorResolverService>,
    /// `textEditorService`.
    pub text_editor_service: Rc<TextEditorService>,
    /// `editorPaneService`.
    pub editor_pane_service: Rc<dyn IEditorPaneService>,
    /// `workspaceTrustRequestService`.
    pub workspace_trust_request_service: Rc<TestWorkspaceTrustRequestService>,
}

impl Deref for EditorServiceAccessor {
    type Target = TextFileTestAccessor;

    fn deref(&self) -> &TextFileTestAccessor {
        &self.base
    }
}

impl EditorServiceAccessor {
    pub fn new(disposables: &Rc<DisposableStore>, working_copy_backup_service: Rc<dyn IWorkingCopyBackupService>) -> Self {
        Self::from_base(disposables, TextFileTestAccessor::new(disposables), working_copy_backup_service)
    }

    /// `workbenchInstantiationService(undefined, disposables)` sem trocar o `IWorkingCopyBackupService`: vale o dublê do
    /// `workbenchInstantiationService`.
    pub fn with_default_backup_service(disposables: &Rc<DisposableStore>) -> Self {
        let base = TextFileTestAccessor::new(disposables);
        let working_copy_backup_service: Rc<dyn IWorkingCopyBackupService> = base.working_copy_backup_service.clone();

        Self::from_base(disposables, base, working_copy_backup_service)
    }

    fn from_base(disposables: &Rc<DisposableStore>, base: TextFileTestAccessor, working_copy_backup_service: Rc<dyn IWorkingCopyBackupService>) -> Self {
        let collection = base.collection.clone();
        let instantiation_service = base.instantiation_service.clone();
        let configuration_service: Rc<dyn IConfigurationService> = base.test_configuration_service.clone();

        // `instantiationService.stub(IWorkingCopyBackupService, workingCopyBackupService)`
        collection.set(I_WORKING_COPY_BACKUP_SERVICE, working_copy_backup_service);

        // Os serviços do `workbenchInstantiationService` que a parte, o resolvedor e o `EditorService` leem.
        let storage = InMemoryStorageService::new();
        let storage_service: Rc<dyn StorageService> = Rc::new(storage.clone());
        disposables.add_rc(storage);
        collection.set(I_STORAGE_SERVICE, storage_service.clone());
        collection.set(I_WORKSPACE_CONTEXT_SERVICE, base.context_service.clone() as Rc<dyn IWorkspaceContextService>);
        collection.set(I_URI_IDENTITY_SERVICE, base.uri_identity_service.clone() as Rc<dyn IUriIdentityService>);
        collection.set(I_DIALOG_SERVICE, base.dialog_service.clone() as Rc<dyn IDialogService>);
        collection.set(I_FILE_DIALOG_SERVICE, base.file_dialog_service.clone() as Rc<dyn IFileDialogService>);
        collection.set(I_LOG_SERVICE, base.log_service.clone());
        collection.set(I_COMMAND_SERVICE, Rc::new(MockCommandService::default()) as Rc<dyn ICommandService>);
        collection.set(I_NOTIFICATION_SERVICE, base.notification_service.clone() as Rc<dyn INotificationService>);
        collection.set(I_EXTENSION_SERVICE, TestExtensionService::new() as Rc<dyn IExtensionService>);
        collection.set(I_HOST_SERVICE, Rc::new(TestHostService) as Rc<dyn IHostService>);
        // `instantiationService.stub(IWorkspaceTrustRequestService, disposables.add(new TestWorkspaceTrustRequestService(...)))`.
        let workspace_trust_request_service = disposables.add_rc(TestWorkspaceTrustRequestService::new(false));
        collection.set(I_WORKSPACE_TRUST_REQUEST_SERVICE, workspace_trust_request_service.clone() as Rc<dyn IWorkspaceTrustRequestService>);
        collection.set(I_WORKBENCH_ENVIRONMENT_SERVICE, TestEnvironmentService::new() as Rc<dyn IWorkbenchEnvironmentService>);
        collection.set(I_UNTITLED_TEXT_EDITOR_SERVICE, base.untitled_text_editor_service.clone() as Rc<dyn IUntitledTextEditorService>);
        collection.set(I_TEXT_MODEL_SERVICE, UntitledTextModelService::new(base.untitled_text_editor_service.clone()) as Rc<dyn ITextModelService>);
        let editor_pane_service: Rc<dyn IEditorPaneService> = instantiation_service.create_instance::<EditorPaneService>(());
        collection.set(I_EDITOR_PANE_SERVICE, editor_pane_service.clone());

        // `createEditorPart(instantiationService, disposables)` e `instantiationService.stub(IEditorGroupsService, part)`
        let context_key_service = MockContextKeyService::scopable();
        let part = disposables.add_rc(EditorPart::new(
            &Ui::new(),
            instantiation_service.clone(),
            configuration_service,
            storage_service,
            &*context_key_service,
            "workbench.parts.editor",
            "",
            MAIN_WINDOW,
        ));
        let editor_groups_service: Rc<dyn IEditorGroupsService> = Rc::new(PartGroupsService { part: Rc::downgrade(&part) });
        collection.set(I_EDITOR_GROUPS_SERVICE, editor_groups_service.clone());
        part.create(None);
        part.layout(1080.0, 800.0, 0.0, 0.0);

        // `instantiationService.createInstance(EditorService, undefined)` e `instantiationService.stub(IEditorService, editorService)`
        let editor_resolver_service = disposables.add_rc(instantiation_service.create_instance::<EditorResolverService>(()));
        collection.set(I_EDITOR_RESOLVER_SERVICE, editor_resolver_service.clone() as Rc<dyn IEditorResolverService>);
        let text_editor_service = disposables.add_rc(instantiation_service.create_instance::<TextEditorService>(()));
        collection.set(I_TEXT_EDITOR_SERVICE, text_editor_service.clone() as Rc<dyn ITextEditorService>);
        let editor_service = disposables.add_rc(instantiation_service.create_instance::<EditorService>(None));
        collection.set(I_EDITOR_SERVICE, editor_service.clone() as Rc<dyn IEditorService>);

        Self { base, part, editor_groups_service, editor_service, editor_resolver_service, text_editor_service, editor_pane_service, workspace_trust_request_service }
    }

    /// `workbenchTeardown(instantiationService)`.
    pub fn teardown(&self) -> Promise<()> {
        let working_copy_service = self.base.working_copy_service.clone();
        let editor_groups_service = self.editor_groups_service.clone();

        async_fn(async move {
            for working_copy in IWorkingCopyService::working_copies(working_copy_service.as_ref()) {
                working_copy.revert(None).await?;
            }

            for group in editor_groups_service.groups() {
                group.close_all_editors(None).await?;
            }

            for group in editor_groups_service.groups() {
                editor_groups_service.remove_group(GroupRef::Group(group));
            }

            Ok(())
        })
    }
}

//#endregion
