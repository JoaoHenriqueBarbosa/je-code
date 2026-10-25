//! Porte de `src/vs/workbench/contrib/notebook/browser/view/renderers/cellRenderer.ts`: o delegate de alturas da lista de células
//! (`NotebookCellListDelegate`), a base dos renderizadores (`AbstractCellRenderer`) e os dois renderizadores de linha, o de markup
//! (`MarkupCellRenderer`) e o de código (`CodeCellRenderer`), que montam o template de cada linha com as partes de célula
//! (`cell_parts/`) e ligam a célula (`MarkupCell` e `CodeCell`) ao template em `renderElement`.
//!
//! O editor da célula e o `CellEditorOverflowWidgets` são criados como no original (o `CodeEditorWidget` pela fábrica
//! [`ICellRendererEditorFactory`], com as mesmas opções e o nó de overflow no contêiner do notebook), o que deixa o renderizador pronto
//! para os 5 casos de `cellEditorOverflowWidgets.test.ts`.
//!
//! Divergências:
//! - **DOM é a árvore do `je-ui`.** O `HTMLElement` de `renderTemplate` é um [`NodeId`]; `DOM.append(parent, DOM.$('.a.b'))` é
//!   `create_el` com as classes separadas por espaço, `classList.add` é `add_class`, `style.display = 'none'` é `hide` e
//!   `FastDomNode` é o [`IndicatorNode`]. `setAttribute('tabindex', '0')` e `title` são atributos do nó. O `.output` da célula de
//!   código é o par de um [`ElementNode`] (as saídas lógicas que `cell_output.rs` manipula) e o nó da árvore que hospeda o
//!   `.output-collapse-container`; o `.output-show-more-container` é só o [`ElementNode`], porque nenhuma parte pinta nele. O
//!   `innerText = ''` do `.output` limpa o [`ElementNode`] e o `appendChild(cellOutputCollapsedContainer)` só reanexa o nó se ele
//!   saiu do pai.
//! - **`IInstantiationService` por modelo de serviços.** O `createChild(new ServiceCollection([IContextKeyService, ...]))` é
//!   `InstantiationService::create_child` registrado na loja de descarte do template. Quem o original resolvia por
//!   `createInstance(Parte, ...)` chega aos construtores como argumento, num [`CellRendererServices`] (um por widget) e nos serviços
//!   por template que `MarkupCell` e `CodeCell` já definem (`MarkupCellServices` e `CodeCellServices`, com o serviço de chaves de
//!   contexto e o serviço de instanciação do contêiner da linha).
//! - **`INotebookEditorDelegate` é o trait [`INotebookEditorDelegate`]**: a soma dos contratos estreitos de cada parte (o widget
//!   implementa todos) mais os `menuIds` de `creationOptions` ([`CellMenuIds`]). Cada parte recebe o `Rc` do delegate coagido para o
//!   contrato dela. As chamadas do próprio renderizador usam a sintaxe qualificada (`Trait::metodo(&*delegate)`), porque vários
//!   contratos repetem o mesmo nome de método.
//! - **`CodeEditorWidget`.** O contrato do editor da célula de código é o [`ICellRendererCodeEditor`] (o `ICodeCellEditor` mais o
//!   contrato do editor da barra de status e o da imagem de arraste), criado por [`ICellRendererEditorFactory::create_code_cell_editor`]
//!   com o mesmo pedido [`MarkupCellEditorOptions`] da célula de markup. O anfitrião que liga o editor ao view model
//!   (`attachTextEditor` e companhia, que o view model do porte ainda não tem) é o [`ICellRendererEditorHost`], a soma dos dois
//!   anfitriões das células, e `NotebookCellEditorPool` é o [`ICodeCellEditorPool`].
//! - **`renderedEditors: Map<ICellViewModel, ICodeEditor>`** é o [`RenderedEditors`], que guarda a ordem de inserção como o `Map` e
//!   implementa o [`IRenderedEditors`] que as células chamam.
//! - **Templates.** O `MarkdownCellRenderTemplate` e o `CodeCellRenderTemplate` completos do original são o
//!   [`MarkupCellRowTemplate`] e o [`CodeCellRowTemplate`]: cada um carrega o template estreito que a célula lê
//!   (`MarkdownCellRenderTemplate` de `markup_cell.rs`, `CodeCellTemplateData` de `code_cell.rs`) mais `templateDisposables`,
//!   `elementDisposables`, `currentRenderedCell` e os nós que só o renderizador usa. O `toJSON` do template (marcação de serialização)
//!   não existe.
//! - **`CellChatPart`** (o chat embutido da célula, `cellParts/chat/cellChatPart.ts`) está fora do escopo do porte (chat cortado), então
//!   a parte não entra nas coleções de partes; o contêiner `.cell-chat-part` continua criado na mesma posição do original, e a
//!   altura de chat da célula fica em zero.
//! - **`CellDragAndDropController`** é a classe [`CellDragAndDropController`] de `cell_dnd.rs`, ligada aos eventos de arrastar e
//!   soltar do `je-ui`. O renderizador de código chama o `registerDragHandle` dela; a imagem de arraste da célula de código é
//!   montada por `CodeCellDragImageRenderer` quando o controlador a pede.
//! - **`DOM.getWindow(rootContainer)`** é a janela do editor de notebook (`ICodeCellNotebookEditor::target_window`), a mesma que a
//!   linha usa.
//! - **`NotebookCellListDelegate`.** `PixelRatio.getInstance(targetWindow)` é o monitor de `je_base::browser::pixel_ratio`, que o
//!   shell da janela alimenta com o fator de escala. `IListVirtualDelegate` é o trait de `je-widgets`, e `getDynamicHeight` de uma
//!   célula de markup é `None` (o `null` do original).
//! - **`IListRenderer`.** `renderTemplate` devolve o [`TemplateData`] (um `Rc<dyn Any>`), que `renderElement` converte de volta. Os
//!   elementos da lista são [`CellViewModel`] (o `MarkupCellViewModel` e o `CodeCellViewModel` do original); a linha de um tipo
//!   errado é erro de programação e lança. O `styles` do tema (cores, fontes, medidas) chega em [`CellRendererStyles`], montado
//!   por quem tem o tema, e o `defaultProgressBarStyles` é uma função, porque cada template cria o seu.

use std::cell::RefCell;
use std::rc::Rc;

use je_base::browser::dom::CodeWindow;
use je_base::browser::pixel_ratio::PixelRatio;
use je_base::dynamic::Dynamic;
use je_base::errors::{self, Error};
use je_base::event::Event;
use je_base::lifecycle::{Disposable, DisposableRef, DisposableStore, IDisposable};
use je_base::nls::localize;
use je_editor::config::font_info_from_settings::create_bare_font_info_from_raw_settings;
use je_editor::language_service::ILanguageService;
use je_editor::modes_registry::PLAINTEXT_LANGUAGE_ID;
use je_instantiation::{InstantiationService, ServiceCollection};
use je_platform::accessibility::IAccessibilityService;
use je_platform::actions::{IMenuService, MenuId};
use je_platform::commands::ICommandService;
use je_platform::configuration::service::IConfigurationService;
use je_platform::contextkey::I_CONTEXT_KEY_SERVICE;
use je_platform::contextkey::contextkey::{IContextKeyService, IScopedContextKeyService};
use je_platform::contextview::context_view::IContextMenuService;
use je_platform::keybinding::keybinding::IKeybindingService;
use je_platform::notification::notification::INotificationService;
use je_platform::telemetry::ITelemetryService;
use je_ui::{NodeId, Role, Ui};
use je_widgets::dom::{clear_node, create_el, hide};
use je_widgets::highlighted_label::HighlightedLabelStyles;
use je_widgets::list::list_types::{IListRenderer, IListVirtualDelegate, ListElementRenderDetails, TemplateData};
use je_widgets::progress_bar::IProgressBarStyles;

use crate::contrib::notebook::browser::view::cell_part::{CellContentPart, CellOverlayPart, CellPartsCollection};
use crate::contrib::notebook::browser::view::cell_parts::cell_comments::{CellComments, ICellCommentService, ICellCommentThreadWidgetFactory, ICellCommentsEditor};
use crate::contrib::notebook::browser::view::cell_parts::cell_context_keys::{CellContextKeyPart, ICellContextKeyEditor};
use crate::contrib::notebook::browser::view::cell_parts::cell_decorations::{CellDecorations, ICellDecorationsEditor};
use crate::contrib::notebook::browser::view::cell_parts::cell_dnd::{CellDragAndDropController, CellDragAndDropPart, ICellDragHandleTemplate};
use crate::contrib::notebook::browser::view::cell_parts::cell_drag_renderer::{CellDragImageType, CodeCellDragImageRenderer, ICellDragImageEditor, ICellDragImageTemplate};
use crate::contrib::notebook::browser::view::cell_parts::cell_editor_options::{CellEditorOptions, merge_options};
use crate::contrib::notebook::browser::view::cell_parts::cell_editor_overflow_widgets::CellEditorOverflowWidgets;
use crate::contrib::notebook::browser::view::cell_parts::cell_execution::{CellExecutionPart, ICellExecutionEditor};
use crate::contrib::notebook::browser::view::cell_parts::cell_focus::{CellFocusPart, ICellFocusEditor};
use crate::contrib::notebook::browser::view::cell_parts::cell_focus_indicator::{CellFocusIndicator, ICellFocusIndicatorEditor, IndicatorNode};
use crate::contrib::notebook::browser::view::cell_parts::cell_output::{CodeCellRenderTemplate as CodeCellOutputTemplate, ElementNode, ICellOutputEditor};
use crate::contrib::notebook::browser::view::cell_parts::cell_progress_bar::CellProgressBar;
use crate::contrib::notebook::browser::view::cell_parts::cell_status_part::{CellEditorStatusBar, CellStatusBarServices, ICellEditorStatusBarEditor, ICellStatusBarCodeEditor};
use crate::contrib::notebook::browser::view::cell_parts::cell_toolbars::{
    BetweenCellToolbar, CellTitleToolbarPart, CellToolbarServices, IBetweenCellToolbarEditor, ICellTitleToolbarEditor, ICssClassDelegate,
};
use crate::contrib::notebook::browser::view::cell_parts::code_cell::{
    CodeCell, CodeCellServices, CodeCellStyles, CodeCellTemplateData, ICodeCellEditor, ICodeCellEditorHost, ICodeCellEditorPool, ICodeCellNotebookEditor,
};
use crate::contrib::notebook::browser::view::cell_parts::code_cell_run_toolbar::{IRunToolbarEditor, RunToolbar};
use crate::contrib::notebook::browser::view::cell_parts::collapsed_cell_input::{CollapsedCellInput, ICollapsedCellInputEditor};
use crate::contrib::notebook::browser::view::cell_parts::collapsed_cell_output::{CollapsedCellOutput, ICollapsedCellOutputEditor};
use crate::contrib::notebook::browser::view::cell_parts::folded_cell_hint::{FoldedCellHint, IFoldedCellHintEditor};
use crate::contrib::notebook::browser::view::cell_parts::markup_cell::{
    IMarkupCellEditor, IMarkupCellEditorFactory, IMarkupCellEditorHost, IMarkupCellNotebookEditor, IRenderedEditors, MarkdownCellRenderTemplate, MarkupCell, MarkupCellEditorOptions,
    MarkupCellServices, MarkupCellStyles, in_composite_editor,
};
use crate::contrib::notebook::browser::view_model::notebook_view_model_impl::CellViewModel;
use crate::contrib::notebook::common::notebook_common::CellKind;
use crate::contrib::notebook::common::notebook_execution_state_service::INotebookExecutionStateService;
use crate::contrib::notebook::common::notebook_logging_service::INotebookLoggingService;

/// `contextKeyServiceProvider`: o serviço de chaves de contexto escopado ao contêiner.
pub type ContextKeyServiceProvider = Rc<dyn Fn(NodeId) -> Rc<dyn IScopedContextKeyService>>;

/// A fábrica das threads de comentário de uma célula, a partir do serviço de chaves de contexto e do serviço de instanciação do
/// contêiner da linha (o `createInstance(CommentThreadWidget, ...)` de `cellComments.ts` os recebia injetados).
pub type CommentWidgetFactoryProvider = Rc<dyn Fn(&Rc<dyn IContextKeyService>, &Rc<InstantiationService>) -> Rc<dyn ICellCommentThreadWidgetFactory>>;

/// `creationOptions.menuIds`, no que o renderizador lê.
#[derive(Clone)]
pub struct CellMenuIds {
    /// `cellTitleToolbar`.
    pub cell_title_toolbar: MenuId,
    /// `cellDeleteToolbar`.
    pub cell_delete_toolbar: MenuId,
    /// `cellExecutePrimary`.
    pub cell_execute_primary: Option<MenuId>,
    /// `cellExecuteToolbar`.
    pub cell_execute_toolbar: Option<MenuId>,
}

/// `INotebookEditorDelegate`: a soma dos contratos que cada parte de célula chama do editor, mais os `menuIds` (ver as divergências do
/// módulo).
pub trait INotebookEditorDelegate:
    IMarkupCellNotebookEditor
    + ICodeCellNotebookEditor
    + IBetweenCellToolbarEditor
    + ICellTitleToolbarEditor
    + IRunToolbarEditor
    + ICellFocusIndicatorEditor
    + ICellEditorStatusBarEditor
    + ICellDecorationsEditor
    + ICellCommentsEditor
    + ICollapsedCellInputEditor
    + ICollapsedCellOutputEditor
    + ICellFocusEditor
    + ICellContextKeyEditor
    + ICellExecutionEditor
    + IFoldedCellHintEditor
{
    /// `creationOptions.menuIds`.
    fn cell_menu_ids(&self) -> CellMenuIds;
}

/// O `CodeEditorWidget` da célula de código, com o que o renderizador e as partes dele chamam (ver as divergências do módulo).
pub trait ICellRendererCodeEditor: ICodeCellEditor + ICellStatusBarCodeEditor + ICellDragImageEditor {}

impl<T: ICodeCellEditor + ICellStatusBarCodeEditor + ICellDragImageEditor> ICellRendererCodeEditor for T {}

/// O anfitrião do editor da célula nas duas formas de célula.
pub trait ICellRendererEditorHost: IMarkupCellEditorHost + ICodeCellEditorHost {}

impl<T: IMarkupCellEditorHost + ICodeCellEditorHost> ICellRendererEditorHost for T {}

/// O `IInstantiationService` filho que cria o `CodeEditorWidget` das duas formas de célula.
pub trait ICellRendererEditorFactory: IMarkupCellEditorFactory {
    /// `editorInstaService.createInstance(CodeEditorWidget, ...)` da célula de código.
    fn create_code_cell_editor(&self, request: MarkupCellEditorOptions) -> Rc<dyn ICellRendererCodeEditor>;
}

/// As cores, as fontes e as medidas que o CSS dá aos nós que as linhas desenham, montadas por quem tem o tema.
#[derive(Clone)]
pub struct CellRendererStyles {
    /// A entrada recolhida, o indicador de dobra e o markdown de leitor de tela da célula de markup.
    pub markup: MarkupCellStyles,
    /// O rótulo do ícone de execução recolhido e a prévia recolhida da célula de código.
    pub code: CodeCellStyles,
    /// O texto do rótulo de ordem de execução, da dica de dobra e da imagem de arraste.
    pub label: HighlightedLabelStyles,
    /// `defaultProgressBarStyles`.
    pub progress_bar: Rc<dyn Fn() -> IProgressBarStyles>,
}

/// Os serviços que o `createInstance` do original injetava nos renderizadores e nas partes (um conjunto por widget).
pub struct CellRendererServices {
    /// `@IConfigurationService`.
    pub configuration_service: Rc<dyn IConfigurationService>,
    /// `@IContextMenuService`.
    pub context_menu_service: Rc<dyn IContextMenuService>,
    /// `@IMenuService`.
    pub menu_service: Rc<dyn IMenuService>,
    /// `@IKeybindingService`.
    pub keybinding_service: Rc<dyn IKeybindingService>,
    /// `@INotificationService`.
    pub notification_service: Rc<dyn INotificationService>,
    /// `@ICommandService`, injetado nas barras de ferramentas.
    pub command_service: Rc<dyn ICommandService>,
    /// `@ITelemetryService`, injetado nas barras de ferramentas.
    pub telemetry_service: Rc<dyn ITelemetryService>,
    /// `@IAccessibilityService`.
    pub accessibility_service: Rc<dyn IAccessibilityService>,
    /// `@ILanguageService`.
    pub language_service: Rc<dyn ILanguageService>,
    /// `@INotebookExecutionStateService`.
    pub notebook_execution_state_service: Rc<dyn INotebookExecutionStateService>,
    /// `@INotebookLoggingService`.
    pub notebook_log_service: Rc<dyn INotebookLoggingService>,
    /// Os serviços da barra de status das células.
    pub status_bar: Rc<CellStatusBarServices>,
    /// `@ICommentService`.
    pub comment_service: Rc<dyn ICellCommentService>,
    /// A fábrica das threads de comentário.
    pub comment_widget_factory_provider: CommentWidgetFactoryProvider,
    /// `IThemeService.onDidColorThemeChange`.
    pub on_did_color_theme_change: Event<()>,
    /// A criação do `CodeEditorWidget`.
    pub editor_factory: Rc<dyn ICellRendererEditorFactory>,
    /// A ligação do editor com o view model da célula.
    pub editor_host: Rc<dyn ICellRendererEditorHost>,
    /// As cores e medidas dos nós que as linhas desenham.
    pub styles: CellRendererStyles,
}

/// `renderedEditors: Map<ICellViewModel, ICodeEditor>`: o mapa das células com o editor delas, na ordem de inserção.
#[derive(Default)]
pub struct RenderedEditors {
    entries: RefCell<Vec<(CellViewModel, Option<Rc<dyn IMarkupCellEditor>>)>>,
}

impl RenderedEditors {
    pub fn new() -> Rc<Self> {
        Rc::new(Self::default())
    }

    /// `renderedEditors.get(cell)`: `None` quando a célula não está no mapa, e `Some(None)` quando está com o valor `undefined`.
    pub fn get(&self, cell: &CellViewModel) -> Option<Option<Rc<dyn IMarkupCellEditor>>> {
        self.entries.borrow().iter().find(|(existing, _)| existing.ptr_eq(cell)).map(|(_, editor)| editor.clone())
    }

    /// `renderedEditors.has(cell)`.
    pub fn has(&self, cell: &CellViewModel) -> bool {
        self.entries.borrow().iter().any(|(existing, _)| existing.ptr_eq(cell))
    }

    /// `renderedEditors.size`.
    pub fn size(&self) -> usize {
        self.entries.borrow().len()
    }

    /// `[...renderedEditors]`: uma cópia das entradas, na ordem de inserção.
    pub fn entries(&self) -> Vec<(CellViewModel, Option<Rc<dyn IMarkupCellEditor>>)> {
        self.entries.borrow().clone()
    }
}

impl IRenderedEditors for RenderedEditors {
    fn set(&self, cell: &CellViewModel, editor: Option<Rc<dyn IMarkupCellEditor>>) {
        let mut entries = self.entries.borrow_mut();
        match entries.iter_mut().find(|(existing, _)| existing.ptr_eq(cell)) {
            Some(entry) => entry.1 = editor,
            None => entries.push((cell.clone(), editor)),
        }
    }

    fn delete(&self, cell: &CellViewModel) {
        self.entries.borrow_mut().retain(|(existing, _)| !existing.ptr_eq(cell));
    }
}

/// `NotebookCellListDelegate`.
pub struct NotebookCellListDelegate {
    base: Rc<Disposable>,
    line_height: f64,
}

impl NotebookCellListDelegate {
    pub fn new(target_window: &CodeWindow, configuration_service: &dyn IConfigurationService) -> Rc<Self> {
        let editor_options = configuration_service.get_value(Some("editor"), None);
        let line_height = create_bare_font_info_from_raw_settings(&editor_options, PixelRatio::get_instance(target_window).value(), false).line_height;
        Rc::new(Self { base: Disposable::new(), line_height })
    }

    /// `lineHeight`.
    pub fn line_height(&self) -> f64 {
        self.line_height
    }
}

impl IListVirtualDelegate<CellViewModel> for NotebookCellListDelegate {
    fn get_height(&self, element: &CellViewModel) -> f64 {
        element.get_height(self.line_height)
    }

    fn get_dynamic_height(&self, element: &CellViewModel) -> Option<f64> {
        element.get_dynamic_height()
    }

    fn get_template_id(&self, element: &CellViewModel) -> String {
        if matches!(element.cell_kind(), CellKind::Markup) { MarkupCellRenderer::TEMPLATE_ID.to_owned() } else { CodeCellRenderer::TEMPLATE_ID.to_owned() }
    }
}

impl IDisposable for NotebookCellListDelegate {
    fn dispose(&self) {
        self.base.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "NotebookCellListDelegate"
    }
}

/// `rootClassDelegate`: `{ toggle: (className, force) => container.classList.toggle(className, force) }`.
struct ContainerClassDelegate {
    ui: Ui,
    container: NodeId,
}

impl ICssClassDelegate for ContainerClassDelegate {
    fn toggle(&self, class_name: &str, force: bool) {
        self.ui.tree_mut().toggle_class(self.container, class_name, force);
    }
}

/// O `container` como o `BaseCellRenderTemplate` que a imagem de arraste lê.
struct ContainerDragImageTemplate(NodeId);

impl ICellDragImageTemplate for ContainerDragImageTemplate {
    fn container(&self) -> NodeId {
        self.0
    }
}

/// `templateDisposables.add(part)`: registra a parte na loja do template e devolve a mesma parte.
fn track<T: IDisposable + 'static>(store: &DisposableStore, part: Rc<T>) -> Rc<T> {
    let handle: DisposableRef = part.clone();
    store.add(handle);
    part
}

/// O que `renderElement` e `disposeElement` leem do `templateData` (o `Rc<dyn Any>` de volta ao tipo do renderizador).
fn downcast_template<T: 'static>(template_data: &TemplateData) -> &T {
    template_data.downcast_ref::<T>().expect("o template da linha é do tipo do renderizador")
}

/// `AbstractCellRenderer`.
struct AbstractCellRenderer {
    base: Rc<Disposable>,
    ui: Ui,
    instantiation_service: Rc<InstantiationService>,
    notebook_editor: Rc<dyn INotebookEditorDelegate>,
    services: Rc<CellRendererServices>,
    context_key_service_provider: ContextKeyServiceProvider,
    editor_options: Rc<CellEditorOptions>,
    dnd_controller: RefCell<Option<Rc<CellDragAndDropController>>>,
}

impl AbstractCellRenderer {
    #[allow(clippy::too_many_arguments)]
    fn new(
        ui: &Ui,
        instantiation_service: Rc<InstantiationService>,
        notebook_editor: Rc<dyn INotebookEditorDelegate>,
        services: Rc<CellRendererServices>,
        context_key_service_provider: ContextKeyServiceProvider,
        language: &str,
        dnd_controller: Option<Rc<CellDragAndDropController>>,
    ) -> Self {
        let base = Disposable::new();
        let editor_options = CellEditorOptions::new(
            IMarkupCellNotebookEditor::get_base_cell_editor_options(&*notebook_editor, language),
            IMarkupCellNotebookEditor::notebook_options(&*notebook_editor),
            services.configuration_service.clone(),
        );
        base.register(editor_options.clone());
        Self { base, ui: ui.clone(), instantiation_service, notebook_editor, services, context_key_service_provider, editor_options, dnd_controller: RefCell::new(dnd_controller) }
    }

    /// `override dispose() { super.dispose(); this.dndController = undefined; }`.
    fn dispose(&self) {
        self.base.dispose();
        *self.dnd_controller.borrow_mut() = None;
    }

    /// `DOM.append(parent, DOM.$('.a.b'))`: as classes vêm separadas por espaço.
    fn append(&self, parent: NodeId, class_name: &str) -> NodeId {
        create_el(&mut self.ui.tree_mut(), parent, Role::Generic, class_name)
    }

    /// `new FastDomNode(DOM.append(parent, DOM.$('.a.b')))`.
    fn append_indicator(&self, parent: NodeId, class_name: &str) -> Rc<IndicatorNode> {
        let node = self.append(parent, class_name);
        IndicatorNode::new(&self.ui, node)
    }

    /// `node.style.display = 'none'`.
    fn hide(&self, node: NodeId) {
        hide(&mut self.ui.tree_mut(), node);
    }

    fn add_class(&self, node: NodeId, class_name: &str) {
        self.ui.tree_mut().add_class(node, class_name);
    }

    /// A janela do notebook, no lugar do `DOM.getWindow(rootContainer)`.
    fn target_window(&self) -> Rc<CodeWindow> {
        ICodeCellNotebookEditor::target_window(&*self.notebook_editor)
    }

    /// `templateDisposables.add(this.contextKeyServiceProvider(container))`.
    fn create_scoped_context_key_service(&self, container: NodeId, store: &DisposableStore) -> Rc<dyn IScopedContextKeyService> {
        let context_key_service = (self.context_key_service_provider)(container);
        let handle: DisposableRef = context_key_service.clone();
        store.add(handle);
        context_key_service
    }

    /// `templateDisposables.add(this.instantiationService.createChild(new ServiceCollection([IContextKeyService, contextKeyService])))`.
    fn create_scoped_instantiation_service(&self, context_key_service: &Rc<dyn IScopedContextKeyService>, store: &DisposableStore) -> Rc<InstantiationService> {
        let context_key_service: Rc<dyn IContextKeyService> = context_key_service.clone();
        self.instantiation_service.create_child(ServiceCollection::new().with(I_CONTEXT_KEY_SERVICE, context_key_service), Some(store))
    }

    /// Os serviços que `scopedInstaService.createInstance` injetava nas barras de ferramentas.
    fn toolbar_services(&self, context_key_service: &Rc<dyn IScopedContextKeyService>) -> CellToolbarServices {
        let context_key_service: Rc<dyn IContextKeyService> = context_key_service.clone();
        CellToolbarServices {
            menu_service: self.services.menu_service.clone(),
            context_key_service,
            context_menu_service: self.services.context_menu_service.clone(),
            keybinding_service: self.services.keybinding_service.clone(),
            command_service: self.services.command_service.clone(),
            telemetry_service: self.services.telemetry_service.clone(),
        }
    }

    /// O `CellTitleToolbarPart` das duas linhas, com o `rootClassDelegate` do contêiner interno.
    fn create_title_toolbar(&self, store: &DisposableStore, container: NodeId, title_toolbar_container: NodeId, services: CellToolbarServices) -> Rc<CellTitleToolbarPart> {
        let root_class_delegate: Rc<dyn ICssClassDelegate> = Rc::new(ContainerClassDelegate { ui: self.ui.clone(), container });
        let menu_ids = self.notebook_editor.cell_menu_ids();
        track(
            store,
            CellTitleToolbarPart::new(
                &self.ui,
                title_toolbar_container,
                root_class_delegate,
                menu_ids.cell_title_toolbar,
                menu_ids.cell_delete_toolbar,
                self.notebook_editor.clone(),
                services,
            ),
        )
    }

    /// `if (!this.notebookEditor.hasModel()) throw new Error(...)`.
    fn ensure_model(&self) {
        if !ICellOutputEditor::has_model(&*self.notebook_editor) {
            errors::throw(Error::new("The notebook editor is not attached with view model yet."));
        }
    }
}

/// O `MarkdownCellRenderTemplate` completo de uma linha de markup (ver as divergências do módulo).
pub struct MarkupCellRowTemplate {
    /// `rootContainer`.
    pub root_container: NodeId,
    /// `container` (`.cell-inner-container`).
    pub container: NodeId,
    /// `instantiationService`: o filho com o serviço de chaves de contexto do contêiner.
    pub instantiation_service: Rc<InstantiationService>,
    /// Os campos que `MarkupCell` lê e escreve.
    pub template: Rc<MarkdownCellRenderTemplate>,
    /// Os serviços que `MarkupCell` recebe, com o serviço de chaves de contexto do contêiner.
    pub services: Rc<MarkupCellServices>,
    /// `templateDisposables`.
    pub template_disposables: Rc<DisposableStore>,
    /// `elementDisposables`.
    pub element_disposables: Rc<DisposableStore>,
    /// `currentRenderedCell`.
    pub current_rendered_cell: RefCell<Option<CellViewModel>>,
}

/// O `CodeCellRenderTemplate` completo de uma linha de código (ver as divergências do módulo).
pub struct CodeCellRowTemplate {
    /// `rootContainer`.
    pub root_container: NodeId,
    /// `container` (`.cell-inner-container`).
    pub container: NodeId,
    /// `cellContainer` (`.cell.code`).
    pub cell_container: NodeId,
    /// O nó da árvore do `.output`, pai do `.output-collapse-container`.
    pub output_node: NodeId,
    /// `instantiationService`: o filho com o serviço de chaves de contexto do contêiner.
    pub instantiation_service: Rc<InstantiationService>,
    /// Os campos que `CodeCell` lê.
    pub template: Rc<CodeCellTemplateData>,
    /// Os serviços que `CodeCell` recebe, com o serviço de instanciação do contêiner.
    pub services: Rc<CodeCellServices>,
    /// `templateDisposables`.
    pub template_disposables: Rc<DisposableStore>,
    /// `elementDisposables`.
    pub element_disposables: Rc<DisposableStore>,
    /// `currentRenderedCell`.
    pub current_rendered_cell: RefCell<Option<CellViewModel>>,
}

impl ICellDragHandleTemplate for CodeCellRowTemplate {
    fn container(&self) -> NodeId {
        self.container
    }

    fn template_disposables(&self) -> Rc<DisposableStore> {
        self.template_disposables.clone()
    }

    fn current_rendered_cell(&self) -> Option<CellViewModel> {
        self.current_rendered_cell.borrow().clone()
    }
}

/// `MarkupCellRenderer`.
pub struct MarkupCellRenderer {
    base: AbstractCellRenderer,
    rendered_editors: Rc<RenderedEditors>,
}

impl MarkupCellRenderer {
    pub const TEMPLATE_ID: &'static str = "markdown_cell";

    pub fn new(
        ui: &Ui,
        notebook_editor: Rc<dyn INotebookEditorDelegate>,
        dnd_controller: Rc<CellDragAndDropController>,
        rendered_editors: Rc<RenderedEditors>,
        context_key_service_provider: ContextKeyServiceProvider,
        instantiation_service: Rc<InstantiationService>,
        services: Rc<CellRendererServices>,
    ) -> Rc<Self> {
        let base = AbstractCellRenderer::new(ui, instantiation_service, notebook_editor, services, context_key_service_provider, "markdown", Some(dnd_controller));
        Rc::new(Self { base, rendered_editors })
    }
}

impl IListRenderer<CellViewModel> for MarkupCellRenderer {
    fn template_id(&self) -> String {
        MarkupCellRenderer::TEMPLATE_ID.to_owned()
    }

    fn render_template(&self, root_container: NodeId) -> TemplateData {
        let base = &self.base;
        let ui = &base.ui;
        let editor = &base.notebook_editor;
        let services = &base.services;

        base.add_class(root_container, "markdown-cell-row");
        let container = base.append(root_container, "cell-inner-container");
        let template_disposables = DisposableStore::new();
        let context_key_service = base.create_scoped_context_key_service(container, &template_disposables);
        let decoration_container = base.append(root_container, "cell-decoration");
        let title_toolbar_container = base.append(container, "cell-title-toolbar");

        let focus_indicator_top = base.append_indicator(container, "cell-focus-indicator cell-focus-indicator-top");
        let focus_indicator_left = base.append_indicator(container, "cell-focus-indicator cell-focus-indicator-side cell-focus-indicator-left");
        let folding_indicator = base.append(focus_indicator_left.node(), "notebook-folding-indicator");
        let focus_indicator_right = base.append_indicator(container, "cell-focus-indicator cell-focus-indicator-side cell-focus-indicator-right");

        let code_inner_content = base.append(container, "cell code");
        let editor_part = base.append(code_inner_content, "cell-editor-part");
        let _cell_chat_part = base.append(editor_part, "cell-chat-part");
        let cell_input_collapsed_container = base.append(code_inner_content, "input-collapse-container");
        base.hide(cell_input_collapsed_container);
        let editor_container = base.append(editor_part, "cell-editor-container");
        base.hide(editor_part);
        let cell_comment_part_container = base.append(container, "cell-comment-container");
        let inner_content = base.append(container, "cell markdown");
        let bottom_cell_container = base.append_indicator(container, "cell-bottom-toolbar-container");

        let scoped_insta_service = base.create_scoped_instantiation_service(&context_key_service, &template_disposables);
        let toolbar_services = base.toolbar_services(&context_key_service);
        let title_toolbar = base.create_title_toolbar(&template_disposables, container, title_toolbar_container, toolbar_services.clone());
        let focus_indicator_bottom = base.append_indicator(container, "cell-focus-indicator cell-focus-indicator-bottom");

        let scoped_context_key_service: Rc<dyn IContextKeyService> = context_key_service.clone();
        let comment_widget_factory = (services.comment_widget_factory_provider)(&scoped_context_key_service, &scoped_insta_service);

        let mut content_parts: Vec<Rc<dyn CellContentPart>> = Vec::new();
        // `CellChatPart` fica fora do porte (ver as divergências do módulo).
        content_parts.push(track(&template_disposables, CellEditorStatusBar::new(ui, editor.clone(), container, editor_part, None, services.status_bar.clone())));
        content_parts.push(track(
            &template_disposables,
            CellFocusIndicator::new(ui, editor.clone(), title_toolbar.clone(), focus_indicator_top, focus_indicator_left, focus_indicator_right, focus_indicator_bottom),
        ));
        let folded_hint_container = base.append(container, "notebook-folded-hint");
        content_parts.push(track(
            &template_disposables,
            FoldedCellHint::new(ui, editor.clone(), folded_hint_container, services.styles.label, services.notebook_execution_state_service.clone()),
        ));
        content_parts.push(track(&template_disposables, CellDecorations::new(ui, editor.clone(), root_container, decoration_container)));
        content_parts.push(track(
            &template_disposables,
            CellComments::new(
                ui,
                editor.clone(),
                cell_comment_part_container,
                services.on_did_color_theme_change.clone(),
                services.comment_service.clone(),
                comment_widget_factory,
            ),
        ));
        content_parts.push(track(&template_disposables, CollapsedCellInput::new(ui, editor.clone(), cell_input_collapsed_container)));
        content_parts.push(track(&template_disposables, CellFocusPart::new(ui, container, None, editor.clone())));
        content_parts.push(track(&template_disposables, CellDragAndDropPart::new(ui, container)));
        content_parts.push(track(
            &template_disposables,
            CellContextKeyPart::new(editor.clone(), scoped_context_key_service.clone(), services.notebook_execution_state_service.clone()),
        ));

        let mut overlay_parts: Vec<Rc<dyn CellOverlayPart>> = Vec::new();
        overlay_parts.push(title_toolbar);
        overlay_parts.push(track(&template_disposables, BetweenCellToolbar::new(ui, editor.clone(), bottom_cell_container, toolbar_services)));

        let cell_parts = CellPartsCollection::new(base.target_window(), content_parts, overlay_parts);
        track(&template_disposables, cell_parts.clone());

        let template = Rc::new(MarkdownCellRenderTemplate {
            root_container,
            container,
            cell_container: inner_content,
            editor_part,
            editor_container,
            cell_input_collapsed_container,
            folding_indicator,
            cell_parts,
            current_editor: RefCell::new(None),
        });
        let markup_services = Rc::new(MarkupCellServices {
            accessibility_service: services.accessibility_service.clone(),
            context_key_service: scoped_context_key_service,
            editor_factory: services.editor_factory.clone(),
            language_service: services.language_service.clone(),
            configuration_service: services.configuration_service.clone(),
            keybinding_service: services.keybinding_service.clone(),
            editor_host: services.editor_host.clone(),
            styles: services.styles.markup.clone(),
        });

        let element_disposables = DisposableStore::new();
        let handle: DisposableRef = element_disposables.clone();
        template_disposables.add(handle);

        let row: TemplateData = Rc::new(MarkupCellRowTemplate {
            root_container,
            container,
            instantiation_service: scoped_insta_service,
            template,
            services: markup_services,
            template_disposables,
            element_disposables,
            current_rendered_cell: RefCell::new(None),
        });
        row
    }

    fn render_element(&self, element: &CellViewModel, _index: usize, template_data: &TemplateData, details: Option<&ListElementRenderDetails>) {
        let base = &self.base;
        base.ensure_model();
        let row = downcast_template::<MarkupCellRowTemplate>(template_data);

        *row.current_rendered_cell.borrow_mut() = Some(element.clone());
        *row.template.current_editor.borrow_mut() = None;
        base.hide(row.template.editor_part);
        clear_node(&mut base.ui.tree_mut(), row.template.cell_container);

        if details.and_then(|details| details.height).is_none() {
            return;
        }

        let cell = MarkupCell::new(&base.ui, base.notebook_editor.clone(), element.clone(), row.template.clone(), self.rendered_editors.clone(), row.services.clone());
        track(&row.element_disposables, cell);
    }

    fn dispose_template(&self, template_data: &TemplateData) {
        downcast_template::<MarkupCellRowTemplate>(template_data).template_disposables.dispose();
    }

    fn dispose_element(&self, _element: &CellViewModel, _index: i64, template_data: &TemplateData, _details: Option<&ListElementRenderDetails>) {
        downcast_template::<MarkupCellRowTemplate>(template_data).element_disposables.clear();
    }
}

impl IDisposable for MarkupCellRenderer {
    fn dispose(&self) {
        self.base.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "MarkupCellRenderer"
    }
}

/// `CodeCellRenderer`.
pub struct CodeCellRenderer {
    base: AbstractCellRenderer,
    rendered_editors: Rc<RenderedEditors>,
    editor_pool: Rc<dyn ICodeCellEditorPool>,
}

impl CodeCellRenderer {
    pub const TEMPLATE_ID: &'static str = "code_cell";

    #[allow(clippy::too_many_arguments)]
    pub fn new(
        ui: &Ui,
        notebook_editor: Rc<dyn INotebookEditorDelegate>,
        rendered_editors: Rc<RenderedEditors>,
        editor_pool: Rc<dyn ICodeCellEditorPool>,
        dnd_controller: Rc<CellDragAndDropController>,
        context_key_service_provider: ContextKeyServiceProvider,
        instantiation_service: Rc<InstantiationService>,
        services: Rc<CellRendererServices>,
    ) -> Rc<Self> {
        let base = AbstractCellRenderer::new(ui, instantiation_service, notebook_editor, services, context_key_service_provider, PLAINTEXT_LANGUAGE_ID, Some(dnd_controller));
        Rc::new(Self { base, rendered_editors, editor_pool })
    }
}

impl IListRenderer<CellViewModel> for CodeCellRenderer {
    fn template_id(&self) -> String {
        CodeCellRenderer::TEMPLATE_ID.to_owned()
    }

    fn render_template(&self, root_container: NodeId) -> TemplateData {
        let base = &self.base;
        let ui = &base.ui;
        let editor = &base.notebook_editor;
        let services = &base.services;

        base.add_class(root_container, "code-cell-row");
        let container = base.append(root_container, "cell-inner-container");
        let template_disposables = DisposableStore::new();
        let context_key_service = base.create_scoped_context_key_service(container, &template_disposables);
        let decoration_container = base.append(root_container, "cell-decoration");
        let focus_indicator_top = base.append_indicator(container, "cell-focus-indicator cell-focus-indicator-top");
        let title_toolbar_container = base.append(container, "cell-title-toolbar");

        // This is also the drag handle
        let focus_indicator_left = base.append_indicator(container, "cell-focus-indicator cell-focus-indicator-side cell-focus-indicator-left");
        let focus_indicator_left_node = focus_indicator_left.node();
        let _cell_chat_part = base.append(container, "cell-chat-part");
        let cell_container = base.append(container, "cell code");
        let run_button_container = base.append(cell_container, "run-button-container");
        let cell_input_collapsed_container = base.append(cell_container, "input-collapse-container");
        base.hide(cell_input_collapsed_container);
        let execution_order_label = base.append(focus_indicator_left_node, "execution-count-label");
        ui.tree_mut().set_attribute(execution_order_label, "title", localize("cellExecutionOrderCountLabel", "Execution Order", &[]));
        let editor_part = base.append(cell_container, "cell-editor-part");
        let editor_container = base.append(editor_part, "cell-editor-container");
        let cell_comment_part_container = base.append(container, "cell-comment-container");

        // create a special context key service that set the inCompositeEditor-contextkey
        let editor_context_key_service = base.create_scoped_context_key_service(editor_part, &template_disposables);
        in_composite_editor().bind_to(&*editor_context_key_service).set(Dynamic::Bool(true));

        let editor_overflow_widgets = track(
            &template_disposables,
            CellEditorOverflowWidgets::new(ui, IMarkupCellNotebookEditor::overflow_container_dom_node(&**editor)),
        );
        let editor_options = merge_options(
            &base.editor_options.get_default_value(),
            [
                ("allowVariableLineHeights", Dynamic::Bool(false)),
                ("dimension", Dynamic::object([("width", Dynamic::Number(0.0)), ("height", Dynamic::Number(0.0))])),
                (
                    "scrollbar",
                    Dynamic::object([
                        ("vertical", Dynamic::string("hidden")),
                        ("horizontal", Dynamic::string("auto")),
                        ("handleMouseWheel", Dynamic::Bool(false)),
                        ("useShadows", Dynamic::Bool(false)),
                    ]),
                ),
            ],
        );
        let code_editor = services.editor_factory.create_code_cell_editor(MarkupCellEditorOptions {
            parent: editor_container,
            context_key_service: editor_context_key_service,
            options: editor_options,
            overflow_widgets_dom_node: editor_overflow_widgets.dom_node(),
            contributions: IMarkupCellNotebookEditor::cell_editor_contributions(&**editor),
        });

        let editor_handle: DisposableRef = code_editor.clone();
        template_disposables.add(editor_handle);
        editor_overflow_widgets.bind_editor(code_editor.clone());

        let output_node = base.append(container, "output");
        let cell_output_collapsed_container = base.append(output_node, "output-collapse-container");
        let output_container = ElementNode::new("output");
        let output_show_more_container = ElementNode::new("output-show-more-container");
        let focus_indicator_right = base.append_indicator(container, "cell-focus-indicator cell-focus-indicator-side cell-focus-indicator-right");
        let focus_sink_element = base.append(container, "cell-editor-focus-sink");
        ui.tree_mut().set_attribute(focus_sink_element, "tabindex", "0");
        let bottom_cell_toolbar_container = base.append_indicator(container, "cell-bottom-toolbar-container");
        let focus_indicator_bottom = base.append_indicator(container, "cell-focus-indicator cell-focus-indicator-bottom");

        let scoped_insta_service = base.create_scoped_instantiation_service(&context_key_service, &template_disposables);
        let toolbar_services = base.toolbar_services(&context_key_service);
        let title_toolbar = base.create_title_toolbar(&template_disposables, container, title_toolbar_container, toolbar_services.clone());
        let scoped_context_key_service: Rc<dyn IContextKeyService> = context_key_service.clone();
        let comment_widget_factory = (services.comment_widget_factory_provider)(&scoped_context_key_service, &scoped_insta_service);

        let focus_indicator_part = track(
            &template_disposables,
            CellFocusIndicator::new(ui, editor.clone(), title_toolbar.clone(), focus_indicator_top, focus_indicator_left, focus_indicator_right, focus_indicator_bottom),
        );
        let mut content_parts: Vec<Rc<dyn CellContentPart>> = Vec::new();
        content_parts.push(focus_indicator_part.clone());
        // `CellChatPart` fica fora do porte (ver as divergências do módulo).
        content_parts.push(track(
            &template_disposables,
            CellEditorStatusBar::new(ui, editor.clone(), container, editor_part, Some(code_editor.clone()), services.status_bar.clone()),
        ));
        content_parts.push(track(
            &template_disposables,
            CellProgressBar::new(ui, editor_part, cell_input_collapsed_container, (services.styles.progress_bar)(), services.notebook_execution_state_service.clone()),
        ));
        content_parts.push(track(&template_disposables, CellDecorations::new(ui, editor.clone(), root_container, decoration_container)));
        content_parts.push(track(
            &template_disposables,
            CellComments::new(
                ui,
                editor.clone(),
                cell_comment_part_container,
                services.on_did_color_theme_change.clone(),
                services.comment_service.clone(),
                comment_widget_factory,
            ),
        ));
        content_parts.push(track(
            &template_disposables,
            CellExecutionPart::new(ui, editor.clone(), execution_order_label, services.styles.label, services.notebook_execution_state_service.clone()),
        ));
        content_parts.push(track(
            &template_disposables,
            CollapsedCellOutput::new(ui, editor.clone(), cell_output_collapsed_container, &*services.keybinding_service),
        ));
        content_parts.push(track(&template_disposables, CollapsedCellInput::new(ui, editor.clone(), cell_input_collapsed_container)));
        content_parts.push(track(&template_disposables, CellFocusPart::new(ui, container, Some(focus_sink_element), editor.clone())));
        content_parts.push(track(&template_disposables, CellDragAndDropPart::new(ui, container)));
        content_parts.push(track(
            &template_disposables,
            CellContextKeyPart::new(editor.clone(), scoped_context_key_service.clone(), services.notebook_execution_state_service.clone()),
        ));

        let menu_ids = editor.cell_menu_ids();
        if let (Some(cell_execute_primary), Some(cell_execute_toolbar)) = (menu_ids.cell_execute_primary, menu_ids.cell_execute_toolbar) {
            content_parts.push(track(
                &template_disposables,
                RunToolbar::new(ui, editor.clone(), container, run_button_container, cell_execute_primary, cell_execute_toolbar, toolbar_services.clone()),
            ));
        }

        let mut overlay_parts: Vec<Rc<dyn CellOverlayPart>> = Vec::new();
        overlay_parts.push(title_toolbar);
        overlay_parts.push(track(&template_disposables, BetweenCellToolbar::new(ui, editor.clone(), bottom_cell_toolbar_container, toolbar_services)));

        let cell_parts = CellPartsCollection::new(base.target_window(), content_parts, overlay_parts);
        track(&template_disposables, cell_parts.clone());

        let element_disposables = DisposableStore::new();
        let handle: DisposableRef = element_disposables.clone();
        template_disposables.add(handle);

        let output = Rc::new(CodeCellOutputTemplate { output_container, output_show_more_container, template_disposables: template_disposables.clone() });
        let template = Rc::new(CodeCellTemplateData {
            output,
            container,
            editor_part,
            editor: code_editor.clone(),
            editor_overflow_widgets,
            cell_input_collapsed_container,
            cell_output_collapsed_container,
            focus_sink_element,
            cell_parts,
        });
        let code_services = Rc::new(CodeCellServices {
            instantiation_service: scoped_insta_service.clone(),
            keybinding_service: services.keybinding_service.clone(),
            language_service: services.language_service.clone(),
            configuration_service: services.configuration_service.clone(),
            execution_state_service: services.notebook_execution_state_service.clone(),
            log_service: services.notebook_log_service.clone(),
            editor_host: services.editor_host.clone(),
            editor_pool: self.editor_pool.clone(),
            styles: services.styles.code.clone(),
        });

        let row = Rc::new(CodeCellRowTemplate {
            root_container,
            container,
            cell_container,
            output_node,
            instantiation_service: scoped_insta_service,
            template,
            services: code_services,
            template_disposables,
            element_disposables,
            current_rendered_cell: RefCell::new(None),
        });

        // focusIndicatorLeft covers the left margin area
        // code/outputFocusIndicator need to be registered as drag handlers so their click handlers don't take over
        let drag_handles = [focus_indicator_left_node, focus_indicator_part.code_focus_indicator.node(), focus_indicator_part.output_focus_indicator.node()];
        let dnd_controller = base.dnd_controller.borrow().clone();
        if let Some(dnd_controller) = dnd_controller {
            let drag_image_renderer = CodeCellDragImageRenderer::new(ui, services.styles.label);
            let drag_image_template = ContainerDragImageTemplate(container);
            let drag_image_editor = code_editor;
            let drag_template: Rc<dyn ICellDragHandleTemplate> = row.clone();
            dnd_controller.register_drag_handle(
                drag_template,
                root_container,
                &drag_handles,
                Box::new(move || drag_image_renderer.get_drag_image(&drag_image_template, &*drag_image_editor, CellDragImageType::Code)),
            );
        }
        row
    }

    fn render_element(&self, element: &CellViewModel, _index: usize, template_data: &TemplateData, details: Option<&ListElementRenderDetails>) {
        let base = &self.base;
        base.ensure_model();
        let row = downcast_template::<CodeCellRowTemplate>(template_data);

        *row.current_rendered_cell.borrow_mut() = Some(element.clone());

        if details.and_then(|details| details.height).is_none() {
            return;
        }

        row.template.output.output_container.clear_children();
        {
            let mut tree = base.ui.tree_mut();
            let collapsed_container = row.template.cell_output_collapsed_container;
            if tree.parent(collapsed_container) != Some(row.output_node) {
                tree.append_child(row.output_node, collapsed_container);
            }
        }

        let cell = CodeCell::new(&base.ui, base.notebook_editor.clone(), element.clone(), row.template.clone(), row.services.clone());
        track(&row.element_disposables, cell);
        self.rendered_editors.set(element, Some(row.template.editor.clone()));
    }

    fn dispose_template(&self, template_data: &TemplateData) {
        downcast_template::<CodeCellRowTemplate>(template_data).template_disposables.dispose();
    }

    fn dispose_element(&self, element: &CellViewModel, _index: i64, template_data: &TemplateData, _details: Option<&ListElementRenderDetails>) {
        downcast_template::<CodeCellRowTemplate>(template_data).element_disposables.clear();
        self.rendered_editors.delete(element);
    }
}

impl IDisposable for CodeCellRenderer {
    fn dispose(&self) {
        self.base.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "CodeCellRenderer"
    }
}
