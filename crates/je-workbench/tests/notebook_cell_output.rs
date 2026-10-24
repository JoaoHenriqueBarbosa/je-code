//! Porte de `src/vs/workbench/contrib/notebook/test/browser/cellOutput.test.ts` (suíte `CellOutput`), caso a caso e na mesma ordem. Os 4 casos estão portados.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o `LeakGuard` do caso, e `await withTestNotebook(cells, (editor, viewModel,
//!   disposables, accessor) => ..., instantiationService)` é o `with_test_notebook_services` com o corpo síncrono sob o relógio
//!   simulado; o `setup` do original (o `setupInstantiationService(store)` e os três `instantiationService.stub(...)`) é o [`setup`]
//!   daqui, que monta um `InstantiationService` com os serviços do `setupInstantiationService` e o `INotebookService`, o
//!   `IMenuService` e o `IContextKeyService` do caso (o `ContextKeyService` real, como no original).
//! - `new class extends mock<INotebookService>()` é o [`StubNotebookService`]: só `getOutputMimeTypeInfo` (a lista filtrada pelos
//!   tipos de mídia que a saída traz) e `getRendererInfo` (o `Stubbed Renderer`, id `rendererId`) respondem; os demais membros lançam
//!   `TypeError`, como o `mock`. O `{ _lower: 'id', value: 'id' }` do `extensionId` é o `ExtensionIdentifier::new("id")`.
//! - `new class extends mock<IMenuService>()` com `createMenu` e o `outputMenus` são o [`StubMenuService`] e a lista `menus` do
//!   [`Fixture`]: cada `createMenu` empurra um [`StubMenu`] (pela identidade, um número), e o `dispose` do menu o tira da lista, o
//!   `filter(item => item !== menu)` do original.
//! - `createCellTemplate(disposables)` (um objeto com `FastDomNode` de `div`, `focusSinkElement` e os dois `DisposableStore`) é o
//!   [`create_cell_template`]: o `FastDomNode<HTMLElement>` é o `ElementNode` lógico do porte (renderização nativa, sem DOM), e só os
//!   campos que o `CellOutputContainer` lê entram no `CodeCellRenderTemplate` (`focusSinkElement` não é lido).
//! - O `editor` do `withTestNotebook` (um `IActiveNotebookEditorDelegate`) é o [`CellOutputEditor`], que implementa o contrato
//!   `ICellOutputEditor` sobre o editor de teste; `layoutNotebookCell`, `createOutput`, `removeInset` e os demais métodos de pintura
//!   são vazios, como os `override async ...() { }` do `testNotebookEditor.ts`.
//! - `accessor.createInstance(CellOutputContainer, editor, cell, cellTemplate, { limit: 100 })` é o `CellOutputContainer::new` com o
//!   limite 100 e o `InstantiationService` dos serviços; `disposables.add(...)` é o `disposables.add_rc`.
//! - `cell.outputsViewModels[i].setVisible(true)` é o `set_visible(true, false)` (o `force` padrão é falso), e
//!   `cellTemplate.outputContainer.domNode.style.display !== 'none'` é o `is_displayed` do nó do container.
//! - `getAllOutputsText(notebook, cell)` é o `get_all_outputs_text` com o `INotebookService` do caso (no original o
//!   `CellOutputViewModel` o guardava) e `shortErrors` falso.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

mod support;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::buffer::VSBuffer;
use je_base::errors::{self, Error};
use je_base::event::Event;
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_extensions::manifest::ExtensionIdentifier;
use je_instantiation::{InstantiationService, ServiceCollection};
use je_platform::actions::actions::{I_MENU_SERVICE, IMenu, IMenuService, MenuActionGroup, MenuActionOptions, MenuChangeEvent, MenuCreateOptions, MenuId};
use je_platform::contextkey::service::ContextKeyService;
use je_platform::contextkey::{I_CONTEXT_KEY_SERVICE, IContextKeyService};
use je_workbench::contrib::notebook::browser::view::cell_parts::cell_output::{
    CellOutputContainer, CodeCellRenderTemplate, ElementNode, ICellOutputEditor, IInsetRenderOutput,
};
use je_workbench::contrib::notebook::browser::view_model::cell_output_text_helper::get_all_outputs_text;
use je_workbench::contrib::notebook::browser::view_model::cell_output_view_model::CellOutputViewModel;
use je_workbench::contrib::notebook::browser::view_model::code_cell_view_model::CodeCellViewModel;
use je_workbench::contrib::notebook::common::model::notebook_text_model::NotebookTextModel;
use je_workbench::contrib::notebook::common::notebook_common::{
    CellKind, ICellOutput, INotebookRendererInfo, IOrderedMimeType, IOutputDto, IOutputItemDto, NotebookRendererMatch, RendererMessagingSpec,
};
use je_workbench::contrib::notebook::common::notebook_common::NotebookRendererEntrypoint;
use je_workbench::contrib::notebook::common::notebook_execution_state_service::I_NOTEBOOK_EXECUTION_STATE_SERVICE;
use je_workbench::contrib::notebook::common::notebook_logging_service::I_NOTEBOOK_LOGGING_SERVICE;
use je_workbench::contrib::notebook::common::notebook_service::{INotebookService, I_NOTEBOOK_SERVICE};
use je_workbench::services::language_detection::language_detection_worker_service::I_LANGUAGE_DETECTION_SERVICE;
use je_editor::language_service::I_LANGUAGE_SERVICE;
use je_base::uri::URI;
use support::test_notebook_editor::{
    ActiveTestNotebookEditorDelegate, NotebookTestServices, empty_metadata, setup_notebook_services, with_test_notebook_services,
};

/// `new class extends mock<INotebookRendererInfo>()`-like: o `{ id, displayName, extensionId } as INotebookRendererInfo` do original.
struct StubRendererInfo;

impl INotebookRendererInfo for StubRendererInfo {
    fn id(&self) -> String {
        "rendererId".to_string()
    }

    fn display_name(&self) -> String {
        "Stubbed Renderer".to_string()
    }

    fn entrypoint(&self) -> NotebookRendererEntrypoint {
        errors::throw(Error::type_error("entrypoint is not a function"))
    }

    fn extension_location(&self) -> URI {
        errors::throw(Error::type_error("extensionLocation is not a function"))
    }

    fn extension_id(&self) -> ExtensionIdentifier {
        ExtensionIdentifier::new("id")
    }

    fn messaging(&self) -> RendererMessagingSpec {
        errors::throw(Error::type_error("messaging is not a function"))
    }

    fn mime_types(&self) -> Vec<String> {
        errors::throw(Error::type_error("mimeTypes is not a function"))
    }

    fn is_builtin(&self) -> bool {
        errors::throw(Error::type_error("isBuiltin is not a function"))
    }

    fn matches_without_kernel(&self, _mime_type: &str) -> NotebookRendererMatch {
        errors::throw(Error::type_error("matchesWithoutKernel is not a function"))
    }

    fn matches(&self, _mime_type: &str, _kernel_provides: &[String]) -> NotebookRendererMatch {
        errors::throw(Error::type_error("matches is not a function"))
    }
}

/// `new class extends mock<INotebookService>()` de `setup`: só `getOutputMimeTypeInfo` e `getRendererInfo` respondem.
struct StubNotebookService;

fn absent(member: &str) -> ! {
    errors::throw(Error::type_error(format!("{member} is not a function")))
}

impl INotebookService for StubNotebookService {
    fn can_resolve(&self, _view_type: &str) -> je_base::promise::Promise<bool> {
        absent("canResolve")
    }
    fn on_add_view_type(&self) -> Event<String> {
        absent("onAddViewType")
    }
    fn on_will_remove_view_type(&self) -> Event<String> {
        absent("onWillRemoveViewType")
    }
    fn on_did_change_output_renderers(&self) -> Event<()> {
        absent("onDidChangeOutputRenderers")
    }
    fn on_will_add_notebook_document(&self) -> Event<Rc<NotebookTextModel>> {
        absent("onWillAddNotebookDocument")
    }
    fn on_did_add_notebook_document(&self) -> Event<Rc<NotebookTextModel>> {
        absent("onDidAddNotebookDocument")
    }
    fn on_will_remove_notebook_document(&self) -> Event<Rc<NotebookTextModel>> {
        absent("onWillRemoveNotebookDocument")
    }
    fn on_did_remove_notebook_document(&self) -> Event<Rc<NotebookTextModel>> {
        absent("onDidRemoveNotebookDocument")
    }
    fn register_notebook_serializer(
        &self,
        _view_type: &str,
        _extension_data: je_workbench::contrib::notebook::common::notebook_common::NotebookExtensionDescription,
        _serializer: Rc<dyn je_workbench::contrib::notebook::common::notebook_service::INotebookSerializer>,
    ) -> je_base::lifecycle::DisposableRef {
        absent("registerNotebookSerializer")
    }
    fn with_notebook_data_provider(&self, _view_type: &str) -> je_base::promise::Promise<Rc<je_workbench::contrib::notebook::common::notebook_service::SimpleNotebookProviderInfo>> {
        absent("withNotebookDataProvider")
    }
    fn try_get_data_provider_sync(&self, _view_type: &str) -> Option<Rc<je_workbench::contrib::notebook::common::notebook_service::SimpleNotebookProviderInfo>> {
        absent("tryGetDataProviderSync")
    }
    fn get_view_type_provider(&self, _view_type: &str) -> Option<String> {
        absent("getViewTypeProvider")
    }
    fn get_renderer_info(&self, _id: &str) -> Option<Rc<dyn INotebookRendererInfo>> {
        Some(Rc::new(StubRendererInfo))
    }
    fn get_renderers(&self) -> Vec<Rc<dyn INotebookRendererInfo>> {
        absent("getRenderers")
    }
    fn get_static_preloads(&self, _view_type: &str) -> Vec<je_workbench::contrib::notebook::common::notebook_common::INotebookStaticPreloadInfo> {
        absent("getStaticPreloads")
    }
    fn get_output_mime_type_info(&self, _text_model: &NotebookTextModel, _kernel_provides: Option<&[String]>, output: &dyn ICellOutput) -> Vec<IOrderedMimeType> {
        let outputs = output.outputs();
        [
            ("plainTextRendererId", "text/plain"),
            ("htmlRendererId", "text/html"),
            ("errorRendererId", "application/vnd.code.notebook.error"),
            ("stderrRendererId", "application/vnd.code.notebook.stderr"),
            ("stdoutRendererId", "application/vnd.code.notebook.stdout"),
        ]
        .into_iter()
        .map(|(renderer_id, mime_type)| IOrderedMimeType { mime_type: mime_type.to_string(), renderer_id: renderer_id.to_string(), is_trusted: true })
        .filter(|info| outputs.iter().any(|output| output.mime == info.mime_type))
        .collect()
    }
    fn create_notebook_text_model(
        &self,
        _view_type: &str,
        _uri: &URI,
        _stream: Option<je_base::buffer::VSBufferReadableStream>,
    ) -> je_base::promise::Promise<Rc<NotebookTextModel>> {
        absent("createNotebookTextModel")
    }
    fn get_notebook_text_model(&self, _uri: &URI) -> Option<Rc<NotebookTextModel>> {
        absent("getNotebookTextModel")
    }
    fn get_notebook_text_models(&self) -> Vec<Rc<NotebookTextModel>> {
        absent("getNotebookTextModels")
    }
    fn list_notebook_documents(&self) -> Vec<Rc<NotebookTextModel>> {
        absent("listNotebookDocuments")
    }
    fn register_contributed_notebook_type(
        &self,
        _view_type: &str,
        _data: je_workbench::contrib::notebook::common::notebook_common::INotebookContributionData,
    ) -> je_base::lifecycle::DisposableRef {
        absent("registerContributedNotebookType")
    }
    fn has_supported_notebooks(&self, _resource: &URI) -> bool {
        absent("hasSupportedNotebooks")
    }
    fn get_notebook_provider_resource_roots(&self) -> Vec<URI> {
        absent("getNotebookProviderResourceRoots")
    }
    fn clear_editor_cache(&self) {
        absent("clearEditorCache")
    }
}

/// O `menu` de `createMenu`: `onDidChange = Event.None`, `getActions() { return [] }` e o `dispose` que o tira de `outputMenus`.
struct StubMenu {
    id: usize,
    menus: Rc<RefCell<Vec<usize>>>,
}

impl IDisposable for StubMenu {
    fn dispose(&self) {
        self.menus.borrow_mut().retain(|item| *item != self.id);
    }
}

impl IMenu for StubMenu {
    fn on_did_change(&self) -> Event<MenuChangeEvent> {
        Event::none()
    }

    fn get_actions(&self, _options: Option<&MenuActionOptions>) -> Vec<MenuActionGroup> {
        Vec::new()
    }
}

/// `new class extends mock<IMenuService>() { createMenu() { ... } }`.
struct StubMenuService {
    menus: Rc<RefCell<Vec<usize>>>,
    next_id: Cell<usize>,
}

impl IMenuService for StubMenuService {
    fn create_menu(&self, _id: &MenuId, _context_key_service: Rc<dyn IContextKeyService>, _options: Option<MenuCreateOptions>) -> Rc<dyn IMenu> {
        let id = self.next_id.get();
        self.next_id.set(id + 1);
        let menu = Rc::new(StubMenu { id, menus: self.menus.clone() });
        self.menus.borrow_mut().push(id);
        menu
    }

    fn get_menu_actions(&self, _id: &MenuId, _context_key_service: &dyn IContextKeyService, _options: Option<&MenuActionOptions>) -> Vec<MenuActionGroup> {
        absent("getMenuActions")
    }

    fn get_menu_contexts(&self, _id: &MenuId) -> std::collections::HashSet<String> {
        absent("getMenuContexts")
    }

    fn reset_hidden_states(&self, _menu_ids: Option<&[MenuId]>) {
        absent("resetHiddenStates")
    }
}

/// O que o `setup` do original guarda em variáveis da suíte (`outputMenus`) e o serviço de notebooks do caso.
struct Fixture {
    menus: Rc<RefCell<Vec<usize>>>,
    notebook_service: Rc<dyn INotebookService>,
}

/// `setup()`: o `setupInstantiationService(store)` com os três `instantiationService.stub(...)`.
fn setup(disposables: &DisposableStore) -> (NotebookTestServices, Fixture) {
    let base = setup_notebook_services(disposables);

    let notebook_service: Rc<dyn INotebookService> = Rc::new(StubNotebookService);
    let menus = Rc::new(RefCell::new(Vec::new()));
    let menu_service: Rc<dyn IMenuService> = Rc::new(StubMenuService { menus: menus.clone(), next_id: Cell::new(0) });
    let context_key_service = disposables.add_rc(ContextKeyService::new(base.configuration_service.clone()));
    let context_key_service: Rc<dyn IContextKeyService> = context_key_service;

    let instantiation_service = disposables.add_rc(InstantiationService::new(
        ServiceCollection::new()
            .with(I_LANGUAGE_SERVICE, base.language_service())
            .with(I_NOTEBOOK_EXECUTION_STATE_SERVICE, base.notebook_execution_state_service())
            .with(I_NOTEBOOK_LOGGING_SERVICE, base.notebook_logging_service())
            .with(I_LANGUAGE_DETECTION_SERVICE, base.language_detection_service())
            .with(I_NOTEBOOK_SERVICE, notebook_service.clone())
            .with(I_MENU_SERVICE, menu_service)
            .with(I_CONTEXT_KEY_SERVICE, context_key_service),
    ));

    let services = NotebookTestServices {
        instantiation_service,
        undo_redo_service: base.undo_redo_service.clone(),
        model_service: base.model_service.clone(),
        configuration_service: base.configuration_service.clone(),
    };
    (services, Fixture { menus, notebook_service })
}

/// O `editor` do caso: o contrato do editor que a parte lê, sobre o editor de teste (a pintura é vazia).
struct CellOutputEditor {
    editor: Rc<ActiveTestNotebookEditorDelegate>,
}

impl ICellOutputEditor for CellOutputEditor {
    fn has_model(&self) -> bool {
        true
    }

    fn text_model(&self) -> Rc<NotebookTextModel> {
        self.editor.text_model.clone()
    }

    fn active_kernel_preload_provides(&self) -> Option<Vec<String>> {
        None
    }

    fn create_output(&self, _cell: &Rc<CodeCellViewModel>, _output: &IInsetRenderOutput, _offset: f64, _create_when_idle: bool) {}

    fn update_output(&self, _cell: &Rc<CodeCellViewModel>, _output: &IInsetRenderOutput, _offset: f64) {}

    fn remove_inset(&self, _output: &Rc<CellOutputViewModel>) {}

    fn hide_inset(&self, _output: &Rc<CellOutputViewModel>) {}

    fn layout_notebook_cell(&self, _cell: &Rc<CodeCellViewModel>, _height: f64) {}
}

/// `createCellTemplate(disposables)`.
fn create_cell_template(disposables: &Rc<DisposableStore>) -> Rc<CodeCellRenderTemplate> {
    Rc::new(CodeCellRenderTemplate {
        output_container: ElementNode::new(""),
        output_show_more_container: ElementNode::new(""),
        template_disposables: disposables.clone(),
    })
}

/// `{ data: VSBuffer.fromString(text), mime }`.
fn output_item(text: &str, mime: &str) -> IOutputItemDto {
    IOutputItemDto { data: VSBuffer::from_string(text), mime: mime.to_string() }
}

/// `{ outputId, outputs }`.
fn output_dto(output_id: &str, outputs: Vec<IOutputItemDto>) -> IOutputDto {
    IOutputDto { output_id: output_id.to_string(), outputs, metadata: None }
}

/// `withTestNotebook([['print(output content)', 'python', CellKind.Code, outputs, {}]], body, instantiationService)`.
fn with_output_notebook(
    outputs: Vec<IOutputDto>,
    body: impl FnOnce(&Rc<ActiveTestNotebookEditorDelegate>, &Rc<je_workbench::contrib::notebook::browser::view_model::notebook_view_model_impl::NotebookViewModel>, &Rc<DisposableStore>, &NotebookTestServices, &Fixture),
) {
    let fixture_slot: Rc<RefCell<Option<Fixture>>> = Rc::new(RefCell::new(None));
    let setup_slot = fixture_slot.clone();
    with_test_notebook_services(
        move |disposables| {
            let (services, fixture) = setup(disposables);
            *setup_slot.borrow_mut() = Some(fixture);
            services
        },
        vec![("print(output content)", "python", CellKind::Code, outputs, Some(empty_metadata()))],
        |editor, view_model, disposables, services| {
            let fixture = fixture_slot.borrow_mut().take().expect("o setup rodou");
            body(editor, view_model, disposables, services, &fixture);
        },
    );
}

/// `const output = disposables.add(accessor.createInstance(CellOutputContainer, editor, cell, cellTemplate, { limit: 100 }))`.
fn create_container(
    editor: &Rc<ActiveTestNotebookEditorDelegate>,
    cell: &Rc<CodeCellViewModel>,
    cell_template: &Rc<CodeCellRenderTemplate>,
    disposables: &Rc<DisposableStore>,
    services: &NotebookTestServices,
) -> Rc<CellOutputContainer> {
    let cell_output_editor: Rc<dyn ICellOutputEditor> = Rc::new(CellOutputEditor { editor: editor.clone() });
    disposables.add_rc(CellOutputContainer::new(cell_output_editor, cell.clone(), cell_template.clone(), 100, services.instantiation_service.clone()))
}

#[test]
fn render_cell_output_items_with_multiple_mime_types() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let output_item_plain = output_item("output content", "text/plain");
    let html_output_item = output_item("output content", "text/html");
    let output1 = output_dto("abc", vec![output_item_plain.clone(), html_output_item.clone()]);
    let output2 = output_dto("def", vec![output_item_plain, html_output_item]);

    with_output_notebook(vec![output1, output2], |editor, view_model, disposables, services, fixture| {
        let cell = view_model.view_cells()[0].as_code().expect("célula de código").clone();
        let cell_template = create_cell_template(disposables);
        let output = create_container(editor, &cell, &cell_template, disposables, services);
        output.render();
        cell.outputs_view_models()[0].set_visible(true, false);
        assert_eq!(fixture.menus.borrow().len(), 1, "should have 1 output menus");
        assert!(cell_template.output_container.is_displayed(), "output container should be visible");
        cell.outputs_view_models()[1].set_visible(true, false);
        assert_eq!(fixture.menus.borrow().len(), 2, "should have 2 output menus");
        cell.outputs_view_models()[1].set_visible(true, false);
        assert_eq!(fixture.menus.borrow().len(), 2, "should still have 2 output menus");
    });
}

#[test]
fn one_of_many_cell_outputs_becomes_hidden() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let output_item_plain = output_item("output content", "text/plain");
    let html_output_item = output_item("output content", "text/html");
    let output1 = output_dto("abc", vec![output_item_plain.clone(), html_output_item.clone()]);
    let output2 = output_dto("def", vec![output_item_plain.clone(), html_output_item.clone()]);
    let output3 = output_dto("ghi", vec![output_item_plain, html_output_item]);

    with_output_notebook(vec![output1, output2, output3], |editor, view_model, disposables, services, fixture| {
        let cell = view_model.view_cells()[0].as_code().expect("célula de código").clone();
        let cell_template = create_cell_template(disposables);
        let output = create_container(editor, &cell, &cell_template, disposables, services);
        output.render();
        cell.outputs_view_models()[0].set_visible(true, false);
        cell.outputs_view_models()[1].set_visible(true, false);
        cell.outputs_view_models()[2].set_visible(true, false);
        cell.outputs_view_models()[1].set_visible(false, false);
        assert!(cell_template.output_container.is_displayed(), "output container should be visible");
        assert_eq!(fixture.menus.borrow().len(), 2, "should have 2 output menus");
    });
}

#[test]
fn get_all_adjacent_stream_outputs() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let stdout = output_item("stdout", "application/vnd.code.notebook.stdout");
    let stderr = output_item("stderr", "application/vnd.code.notebook.stderr");
    let output1 = output_dto("abc", vec![stdout]);
    let output2 = output_dto("abc", vec![stderr]);

    with_output_notebook(vec![output1, output2], |_editor, view_model, _disposables, _services, fixture| {
        let cell = view_model.view_cells()[0].clone();
        let notebook = view_model.notebook_document();
        let result = get_all_outputs_text(&fixture.notebook_service, &notebook, &cell, false);

        assert_eq!(result, "stdoutstderr");
    });
}

#[test]
fn get_all_mixed_outputs_of_cell() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let stdout = output_item("stdout", "application/vnd.code.notebook.stdout");
    let stderr = output_item("stderr", "application/vnd.code.notebook.stderr");
    let plain_text = output_item("output content", "text/plain");
    let error = output_item(r#"{"name":"Error Name","message":"error message","stack":"error stack"}"#, "application/vnd.code.notebook.error");
    let output1 = output_dto("abc", vec![stdout]);
    let output2 = output_dto("abc", vec![stderr]);
    let output3 = output_dto("abc", vec![plain_text]);
    let output4 = output_dto("abc", vec![error]);

    with_output_notebook(vec![output1, output2, output3, output4], |_editor, view_model, _disposables, _services, fixture| {
        let cell = view_model.view_cells()[0].clone();
        let notebook = view_model.notebook_document();
        let result = get_all_outputs_text(&fixture.notebook_service, &notebook, &cell, false);

        assert_eq!(
            result,
            "Cell output 1 of 3\n\
             stdoutstderr\n\
             Cell output 2 of 3\n\
             output content\n\
             Cell output 3 of 3\n\
             error stack"
        );
    });
}
