//! Porte de `src/vs/workbench/contrib/notebook/browser/view/cellParts/cellOutput.ts`: as saídas de uma célula de código
//! (`CellOutputContainer`, que cuida da lista e do corte por `limit`, e `CellOutputElement`, que resolve o tipo de mídia, escolhe o
//! renderizador e liga a barra de ferramentas da saída).
//!
//! Divergências:
//! - Renderização nativa, sem DOM: os elementos do original (`#output`, `.output-inner-container`, `.cell-output-toolbar`,
//!   `.rendered-output`, `.output-show-more`) são nós lógicos [`ElementNode`] (classe, atributos, pai, filhos, `top`, `display`), que
//!   guardam o que o original lê e escreve neles (`insertBefore`, `appendChild`, `remove`, `nextElementSibling`, `className`,
//!   `style.top`, `style.display`, `DOM.show` e `DOM.hide`). O `FastDomNode<HTMLElement>` de `outputContainer` e de
//!   `outputShowMoreContainer` é um [`ElementNode`] dentro do [`CodeCellRenderTemplate`], que traz só os campos que esta parte lê
//!   (o resto do `CodeCellRenderTemplate` de `notebookRenderingCommon.ts` é do widget).
//! - `CellContentPart` (`cellPart.ts`) não tem porte: `CellOutputContainer` estende só o `Disposable`, e `updateInternalLayoutNow` é um
//!   método próprio (os ganchos `renderCell`, `unrenderCell` e `updateState` não são chamados por nada nesta parte).
//! - `INotebookEditorDelegate` é o trait [`ICellOutputEditor`], só com o que esta parte chama (`hasModel`, `textModel`,
//!   `activeKernel?.preloadProvides`, `createOutput`, `updateOutput`, `removeInset`, `hideInset`, `layoutNotebookCell`).
//!   `IInsetRenderOutput` e `RenderOutputType` (de `notebookBrowser.ts`) são [`IInsetRenderOutput`] e [`RenderOutputType`] aqui.
//! - O `@IInstantiationService` do container é o `InstantiationService` do porte: os serviços que o `createInstance` do
//!   `CellOutputElement` injeta (`INotebookService`, `IContextKeyService`, `IMenuService`) são lidos dele a cada elemento, e o
//!   `IOpenerService` do link de "mostrar mais" é lido só quando o link é acionado.
//! - O `WorkbenchToolBar` é o [`OutputToolbar`]: guarda o contexto (`toolbar.context`) e as ações secundárias que `setActions` recebe, sem
//!   pintura; o `$mid` do contexto (marcação de serialização) não existe. `getActionBarActions(groups, () => false)` (sem grupo primário e
//!   sem submenu embutido) é [`secondary_actions_of`]: os grupos em fila, com um separador entre eles. O ícone `mimetypeIcon` é a
//!   classe `codicon codicon-notebook-mimetype`.
//! - As chaves de contexto `NOTEBOOK_CELL_HAS_HIDDEN_OUTPUTS`, `NOTEBOOK_CELL_IS_FIRST_OUTPUT` e `NOTEBOOK_CELL_OUTPUT_MIMETYPE`
//!   (`notebookContextKeys.ts`, sem porte) são declaradas aqui como funções, no molde das chaves do diff de notebooks.
//! - O seletor de apresentação (`_pickActiveMimeTypeRenderer`, `_showJupyterExtension`, `_generateRendererInfo` e
//!   `JUPYTER_RENDERER_MIMETYPES`) depende do `IQuickInputService` e do `IExtensionsWorkbenchService`, que não têm ponto de entrada
//!   no porte do widget: a ação `notebook.output.pickMimetype` existe na barra, mas sem `actionCallback`, e o seletor fica para a
//!   fatia do widget. Pelo mesmo motivo `_validateFinalOutputHeight` do elemento (só chamada pelo seletor) está presente e o campo
//!   `_outputHeightTimer` do elemento só é armado por ela.
//! - `if (!this.renderResult)` depois de `_renderMissingRenderer` é um ramo morto no original (a função sempre devolve um resultado) e
//!   não é portado. `viewCell.outputsViewModels.indexOf(output)` que não acha (`-1`) é `usize::MAX` aqui, que o
//!   `CodeCellViewModel` rejeita com o mesmo "Output index out of range!".
//! - O `try`/`finally` de `render` é um `catch_unwind` que refaz o layout da célula e relança o pânico. O
//!   `updateInternalLayoutNow(viewCell)` não recebe a célula: o container já a guarda.
//! - O `renderMarkdown` do "mostrar mais" é um [`ElementNode`] com a classe `output-show-more`, o texto do markdown e o tratador do link
//!   de comando ([`ElementNode::activate_link`]); o `IMarkdownString` (`isTrusted`, `supportThemeIcons`) não tem outro leitor.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::rc::{Rc, Weak};

use je_base::actions::{Action, ActionRef, Separator};
use je_base::dynamic::Dynamic;
use je_base::errors::{self, Error};
use je_base::lifecycle::{Disposable, DisposableStore, IDisposable, to_disposable};
use je_base::mime::is_text_stream_mime;
use je_base::nls::localize;
use je_base::observable::{ISettableObservable, Observable, SettableExt, autorun, observable_value};
use je_base::runtime::{self, TimerId};
use je_instantiation::InstantiationService;
use je_platform::actions::actions::{I_MENU_SERVICE, IMenuService, MenuAction, MenuActionGroup, MenuActionOptions};
use je_platform::actions::menu_ids::notebook_output_toolbar;
use je_platform::contextkey::{ContextKeyMeta, I_CONTEXT_KEY_SERVICE, IContextKeyService, IContextKeyServiceTarget, RawContextKey};
use je_platform::opener::opener::{I_OPENER_SERVICE, OpenResource};

use crate::contrib::notebook::browser::view_model::cell_output_view_model::CellOutputViewModel;
use crate::contrib::notebook::browser::view_model::code_cell_view_model::CodeCellViewModel;
use crate::contrib::notebook::browser::view_model::cell_output_text_helper::TEXT_BASED_MIMETYPES;
use crate::contrib::notebook::common::model::notebook_text_model::NotebookTextModel;
use crate::contrib::notebook::common::notebook_common::{CellUri, ICell, INotebookRendererInfo, IOrderedMimeType, NotebookCellExecutionState, NotebookCellOutputsSplice};
use crate::contrib::notebook::common::notebook_execution_state_service::{INotebookExecutionStateService, I_NOTEBOOK_EXECUTION_STATE_SERVICE};
use crate::contrib::notebook::common::notebook_service::{INotebookService, I_NOTEBOOK_SERVICE};

/// `COPY_OUTPUT_COMMAND_ID` (`controller/cellOutputActions.ts`).
pub const COPY_OUTPUT_COMMAND_ID: &str = "notebook.cellOutput.copy";

/// O comando do link de "mostrar mais" do markdown de [`CellOutputContainer::generate_show_more_element`].
const OPEN_LARGE_OUTPUT_COMMAND: &str = "command:workbench.action.openLargeOutput";

thread_local! {
    static NOTEBOOK_CELL_IS_FIRST_OUTPUT: RawContextKey = RawContextKey::new("notebookCellIsFirstOutput", Dynamic::Bool(false), ContextKeyMeta::None);
    static NOTEBOOK_CELL_HAS_HIDDEN_OUTPUTS: RawContextKey = RawContextKey::new("hasHiddenOutputs", Dynamic::Bool(false), ContextKeyMeta::None);
    static NOTEBOOK_CELL_OUTPUT_MIMETYPE: RawContextKey = RawContextKey::new("notebookCellOutputMimeType", Dynamic::Undefined, ContextKeyMeta::None);
}

/// `RenderOutputType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderOutputType {
    Html = 0,
    Extension = 1,
}

/// `IInsetRenderOutput` (`IRenderPlainHtmlOutput | IRenderOutputViaExtension`).
#[derive(Clone)]
pub enum IInsetRenderOutput {
    Html { source: Rc<CellOutputViewModel>, html_content: String },
    Extension { source: Rc<CellOutputViewModel>, mime_type: String, renderer: Rc<dyn INotebookRendererInfo> },
}

impl IInsetRenderOutput {
    /// `type`.
    pub fn render_type(&self) -> RenderOutputType {
        match self {
            IInsetRenderOutput::Html { .. } => RenderOutputType::Html,
            IInsetRenderOutput::Extension { .. } => RenderOutputType::Extension,
        }
    }
}

/// O que `CellOutputElement` e `CellOutputContainer` chamam do editor de notebook (`INotebookEditorDelegate`).
pub trait ICellOutputEditor {
    /// `hasModel()`.
    fn has_model(&self) -> bool;
    /// `textModel`.
    fn text_model(&self) -> Rc<NotebookTextModel>;
    /// `activeKernel?.preloadProvides`.
    fn active_kernel_preload_provides(&self) -> Option<Vec<String>>;
    /// `createOutput(cell, output, offset, createWhenIdle)`.
    fn create_output(&self, cell: &Rc<CodeCellViewModel>, output: &IInsetRenderOutput, offset: f64, create_when_idle: bool);
    /// `updateOutput(cell, output, offset)`.
    fn update_output(&self, cell: &Rc<CodeCellViewModel>, output: &IInsetRenderOutput, offset: f64);
    /// `removeInset(output)`.
    fn remove_inset(&self, output: &Rc<CellOutputViewModel>);
    /// `hideInset(output)`.
    fn hide_inset(&self, output: &Rc<CellOutputViewModel>);
    /// `layoutNotebookCell(cell, height)`.
    fn layout_notebook_cell(&self, cell: &Rc<CodeCellViewModel>, height: f64);
}

/// Um nó lógico no lugar do `HTMLElement`.
pub struct ElementNode {
    class_name: RefCell<String>,
    text: RefCell<String>,
    attributes: RefCell<HashMap<String, String>>,
    parent: RefCell<Weak<ElementNode>>,
    children: RefCell<Vec<Rc<ElementNode>>>,
    top: Cell<f64>,
    position_relative: Cell<bool>,
    display_none: Cell<bool>,
    toolbar: RefCell<Option<Rc<OutputToolbar>>>,
    link_handler: RefCell<Option<Rc<dyn Fn(&str)>>>,
}

impl ElementNode {
    /// `document.createElement('div')` com a classe.
    pub fn new(class_name: &str) -> Rc<Self> {
        Rc::new(Self {
            class_name: RefCell::new(class_name.to_string()),
            text: RefCell::new(String::new()),
            attributes: RefCell::new(HashMap::new()),
            parent: RefCell::new(Weak::new()),
            children: RefCell::new(Vec::new()),
            top: Cell::new(0.0),
            position_relative: Cell::new(false),
            display_none: Cell::new(false),
            toolbar: RefCell::new(None),
            link_handler: RefCell::new(None),
        })
    }

    /// `className`.
    pub fn class_name(&self) -> String {
        self.class_name.borrow().clone()
    }

    /// `classList.add(name)`.
    pub fn add_class(&self, name: &str) {
        let mut class_name = self.class_name.borrow_mut();
        if !class_name.split(' ').any(|existing| existing == name) {
            if !class_name.is_empty() {
                class_name.push(' ');
            }
            class_name.push_str(name);
        }
    }

    /// `textContent`.
    pub fn text(&self) -> String {
        self.text.borrow().clone()
    }

    pub fn set_text(&self, text: &str) {
        *self.text.borrow_mut() = text.to_string();
    }

    /// `style.top` (em pixels).
    pub fn top(&self) -> f64 {
        self.top.get()
    }

    pub fn set_top(&self, top: f64) {
        self.top.set(top);
    }

    /// `style.position === 'relative'`.
    pub fn is_position_relative(&self) -> bool {
        self.position_relative.get()
    }

    pub fn set_position_relative(&self, relative: bool) {
        self.position_relative.set(relative);
    }

    /// `style.display !== 'none'`.
    pub fn is_displayed(&self) -> bool {
        !self.display_none.get()
    }

    /// `DOM.show(node)`.
    pub fn show(&self) {
        self.display_none.set(false);
        self.remove_attribute_named("aria-hidden");
    }

    /// `DOM.hide(node)`.
    pub fn hide(&self) {
        self.display_none.set(true);
        self.attributes.borrow_mut().insert("aria-hidden".to_string(), "true".to_string());
    }

    fn remove_attribute_named(&self, name: &str) {
        self.attributes.borrow_mut().remove(name);
    }

    /// `parentElement`.
    pub fn parent_node(&self) -> Option<Rc<ElementNode>> {
        self.parent.borrow().upgrade()
    }

    /// `children`.
    pub fn children(&self) -> Vec<Rc<ElementNode>> {
        self.children.borrow().clone()
    }

    /// `hasChildNodes()`.
    pub fn has_child_nodes(&self) -> bool {
        !self.children.borrow().is_empty()
    }

    /// `innerText = ''`.
    pub fn clear_children(&self) {
        let children = std::mem::take(&mut *self.children.borrow_mut());
        for child in children {
            *child.parent.borrow_mut() = Weak::new();
        }
        self.text.borrow_mut().clear();
    }

    /// `appendChild(child)`: move o filho de onde ele estiver.
    pub fn append_child(self: &Rc<Self>, child: &Rc<ElementNode>) {
        child.remove();
        *child.parent.borrow_mut() = Rc::downgrade(self);
        self.children.borrow_mut().push(child.clone());
    }

    /// `insertBefore(child, reference)`: sem o `reference` entre os filhos o `child` vai para o fim.
    pub fn insert_before(self: &Rc<Self>, child: &Rc<ElementNode>, reference: &Rc<ElementNode>) {
        child.remove();
        *child.parent.borrow_mut() = Rc::downgrade(self);
        let mut children = self.children.borrow_mut();
        let position = children.iter().position(|existing| Rc::ptr_eq(existing, reference));
        match position {
            Some(position) => children.insert(position, child.clone()),
            None => children.push(child.clone()),
        }
    }

    /// `remove()`.
    pub fn remove(&self) {
        let parent = self.parent.borrow().upgrade();
        if let Some(parent) = parent {
            parent.children.borrow_mut().retain(|existing| !std::ptr::eq(Rc::as_ptr(existing), self));
        }
        *self.parent.borrow_mut() = Weak::new();
    }

    /// `nextElementSibling`.
    pub fn next_element_sibling(&self) -> Option<Rc<ElementNode>> {
        let parent = self.parent.borrow().upgrade()?;
        let children = parent.children.borrow();
        let position = children.iter().position(|existing| std::ptr::eq(Rc::as_ptr(existing), self))?;
        children.get(position + 1).cloned()
    }

    /// O [`OutputToolbar`] que o `WorkbenchToolBar` criado dentro deste nó pintaria.
    pub fn toolbar(&self) -> Option<Rc<OutputToolbar>> {
        self.toolbar.borrow().clone()
    }

    pub fn set_toolbar(&self, toolbar: Option<Rc<OutputToolbar>>) {
        *self.toolbar.borrow_mut() = toolbar;
    }

    /// O `actionHandler` do `renderMarkdown`, que recebe o destino do link acionado.
    pub fn set_link_handler(&self, handler: Option<Rc<dyn Fn(&str)>>) {
        *self.link_handler.borrow_mut() = handler;
    }

    /// O clique num link do markdown renderizado.
    pub fn activate_link(&self, content: &str) {
        let handler = self.link_handler.borrow().clone();
        if let Some(handler) = handler {
            handler(content);
        }
    }
}

impl IContextKeyServiceTarget for ElementNode {
    fn parent_element(&self) -> Option<Rc<dyn IContextKeyServiceTarget>> {
        self.parent_node().map(|parent| parent as Rc<dyn IContextKeyServiceTarget>)
    }

    fn set_attribute(&self, attr: &str, value: &str) {
        self.attributes.borrow_mut().insert(attr.to_string(), value.to_string());
    }

    fn remove_attribute(&self, attr: &str) {
        self.remove_attribute_named(attr);
    }

    fn has_attribute(&self, attr: &str) -> bool {
        self.attributes.borrow().contains_key(attr)
    }

    fn get_attribute(&self, attr: &str) -> Option<String> {
        self.attributes.borrow().get(attr).cloned()
    }
}

/// O que `CellOutputContainer` lê do `CodeCellRenderTemplate`.
pub struct CodeCellRenderTemplate {
    /// `outputContainer`.
    pub output_container: Rc<ElementNode>,
    /// `outputShowMoreContainer`.
    pub output_show_more_container: Rc<ElementNode>,
    /// `templateDisposables`.
    pub template_disposables: Rc<DisposableStore>,
}

/// O contexto da barra de ferramentas da saída (`toolbar.context`).
pub struct OutputToolbarContext {
    pub ui: bool,
    pub cell: Rc<CodeCellViewModel>,
    pub output_view_model: Rc<CellOutputViewModel>,
}

/// O `WorkbenchToolBar` da saída (ver as divergências do módulo).
pub struct OutputToolbar {
    pub context: OutputToolbarContext,
    primary_actions: RefCell<Vec<ActionRef>>,
    secondary_actions: RefCell<Vec<ActionRef>>,
}

impl OutputToolbar {
    pub fn new(context: OutputToolbarContext) -> Self {
        Self { context, primary_actions: RefCell::new(Vec::new()), secondary_actions: RefCell::new(Vec::new()) }
    }

    /// `setActions(primary, secondary)`.
    pub fn set_actions(&self, primary: Vec<ActionRef>, secondary: Vec<ActionRef>) {
        *self.primary_actions.borrow_mut() = primary;
        *self.secondary_actions.borrow_mut() = secondary;
    }

    pub fn primary_actions(&self) -> Vec<ActionRef> {
        self.primary_actions.borrow().clone()
    }

    pub fn secondary_actions(&self) -> Vec<ActionRef> {
        self.secondary_actions.borrow().clone()
    }
}

impl IDisposable for OutputToolbar {
    fn dispose(&self) {
        self.primary_actions.borrow_mut().clear();
        self.secondary_actions.borrow_mut().clear();
    }

    fn constructor_name(&self) -> &'static str {
        "OutputToolbar"
    }
}

/// `getActionBarActions(groups, () => false).secondary`: nenhum grupo é primário e nenhum submenu é embutido, então as ações vão
/// todas para a lista secundária, com um separador entre um grupo e o seguinte.
pub fn secondary_actions_of(groups: Vec<MenuActionGroup>) -> Vec<ActionRef> {
    let mut secondary: Vec<ActionRef> = Vec::new();
    for (_group, actions) in groups {
        if !secondary.is_empty() {
            secondary.push(Separator::new());
        }
        for action in actions {
            secondary.push(match action {
                MenuAction::Item(item) => item as ActionRef,
                MenuAction::Submenu(submenu) => submenu.as_action(),
            });
        }
    }
    secondary
}

/// O texto de um nó do `outerHTML` (`&`, `<`, `>` e o espaço sem quebra).
fn escape_html_text(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('\u{a0}', "&nbsp;")
}

/// O valor de um atributo do `outerHTML` (`&`, `"` e o espaço sem quebra).
fn escape_html_attribute(value: &str) -> String {
    value.replace('&', "&amp;").replace('"', "&quot;").replace('\u{a0}', "&nbsp;")
}

/// `{ initRenderIsSynchronous: false }`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IRenderResult {
    pub init_render_is_synchronous: bool,
}

/// `[Output, previousSibling]` de uma entrada da lista (`OutputEntryViewHandler`).
#[derive(Clone)]
pub struct OutputEntryViewHandler {
    pub model: Rc<CellOutputViewModel>,
    pub element: Rc<CellOutputElement>,
}

/// `CellOutputUpdateContext`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CellOutputUpdateContext {
    Execution = 1,
    Other = 2,
}

/// `Array#slice(start, end)`: os dois limites são cortados no tamanho da lista.
fn js_slice<T: Clone>(items: &[T], start: usize, end: usize) -> Vec<T> {
    let start = start.min(items.len());
    let end = end.min(items.len());
    if start >= end { Vec::new() } else { items[start..end].to_vec() }
}

/// O `previousSibling` do `i`-ésimo elemento: o contêiner interno do anterior, se ele ainda estiver na árvore.
fn previous_sibling_of(entries: &[OutputEntryViewHandler], i: usize) -> Option<Rc<ElementNode>> {
    if i == 0 {
        return None;
    }
    entries.get(i - 1).and_then(|entry| entry.element.inner_container()).filter(|inner| inner.parent_node().is_some())
}

// DOM structure
//
//  #output
//  |
//  |  #output-inner-container
//  |                        |  #cell-output-toolbar
//  |                        |  #output-element
//  |                        |  #output-element
//  |                        |  #output-element
//  |  #output-inner-container
//  |                        |  #cell-output-toolbar
//  |                        |  #output-element
//  |  #output-inner-container
//  |                        |  #cell-output-toolbar
//  |                        |  #output-element
/// `CellOutputElement`.
pub struct CellOutputElement {
    base: Rc<Disposable>,
    toolbar_disposables: Rc<DisposableStore>,
    me: Weak<CellOutputElement>,
    notebook_editor: Rc<dyn ICellOutputEditor>,
    view_cell: Rc<CodeCellViewModel>,
    cell_output_container: Weak<CellOutputContainer>,
    output_container: Rc<ElementNode>,
    pub output: Rc<CellOutputViewModel>,
    notebook_service: Rc<dyn INotebookService>,
    menu_service: Rc<dyn IMenuService>,
    context_key_service: Rc<dyn IContextKeyService>,
    inner_container: RefCell<Option<Rc<ElementNode>>>,
    rendered_output_container: RefCell<Option<Rc<ElementNode>>>,
    render_result: RefCell<Option<IInsetRenderOutput>>,
    toolbar_attached: Cell<bool>,
    output_height_timer: Cell<Option<TimerId>>,
}

impl CellOutputElement {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        notebook_editor: Rc<dyn ICellOutputEditor>,
        view_cell: Rc<CodeCellViewModel>,
        cell_output_container: Weak<CellOutputContainer>,
        output_container: Rc<ElementNode>,
        output: Rc<CellOutputViewModel>,
        notebook_service: Rc<dyn INotebookService>,
        menu_service: Rc<dyn IMenuService>,
        context_key_service: Rc<dyn IContextKeyService>,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let toolbar_disposables = DisposableStore::new();
        base.register(toolbar_disposables.clone());

        let this = Rc::new_cyclic(|me| Self {
            base,
            toolbar_disposables,
            me: me.clone(),
            notebook_editor,
            view_cell,
            cell_output_container,
            output_container,
            output,
            notebook_service,
            menu_service,
            context_key_service,
            inner_container: RefCell::new(None),
            rendered_output_container: RefCell::new(None),
            render_result: RefCell::new(None),
            toolbar_attached: Cell::new(false),
            output_height_timer: Cell::new(None),
        });

        let weak = this.me.clone();
        this.base.register(this.output.model().on_did_change_data().listen(move |_| {
            if let Some(this) = weak.upgrade() {
                this.rerender();
            }
        }));

        let weak = this.me.clone();
        this.base.register(this.output.on_did_reset_renderer().listen(move |_| {
            if let Some(this) = weak.upgrade() {
                this.rerender();
            }
        }));

        this
    }

    /// `innerContainer`.
    pub fn inner_container(&self) -> Option<Rc<ElementNode>> {
        self.inner_container.borrow().clone()
    }

    /// `renderedOutputContainer`.
    pub fn rendered_output_container(&self) -> Option<Rc<ElementNode>> {
        self.rendered_output_container.borrow().clone()
    }

    /// `renderResult`.
    pub fn render_result(&self) -> Option<IInsetRenderOutput> {
        self.render_result.borrow().clone()
    }

    fn container(&self) -> Rc<CellOutputContainer> {
        self.cell_output_container.upgrade().expect("o container vive enquanto o elemento é usado")
    }

    /// `viewCell.outputsViewModels.indexOf(this.output)`; `-1` é `usize::MAX` (ver as divergências do módulo).
    fn output_index(&self) -> usize {
        self.view_cell.outputs_view_models().iter().position(|output| Rc::ptr_eq(output, &self.output)).unwrap_or(usize::MAX)
    }

    pub fn detach(&self) {
        let rendered = self.rendered_output_container.borrow().clone();
        if let Some(rendered) = rendered {
            rendered.remove();
        }

        let inner = self.inner_container.borrow().clone();
        if let Some(inner) = inner {
            // O laço do original conta os filhos `rendered-output` e para no segundo; só o `count === 0` é lido.
            let count = inner.children().iter().filter(|child| child.class_name() == "rendered-output").count();
            if count == 0 {
                inner.remove();
            }
        }

        self.notebook_editor.remove_inset(&self.output);
    }

    pub fn update_dom_top(&self, top: f64) {
        if let Some(inner) = self.inner_container() {
            inner.set_top(top);
        }
    }

    pub fn rerender(&self) {
        let render_result = self.render_result();
        if self.notebook_editor.has_model()
            && self.inner_container().is_some()
            && let Some(IInsetRenderOutput::Extension { mime_type, renderer, .. }) = &render_result
        {
            // Output rendered by extension renderer got an update
            let text_model = self.notebook_editor.text_model();
            let preload_provides = self.notebook_editor.active_kernel_preload_provides();
            let (mime_types, pick) = self.output.resolve_mime_types(self.notebook_service.as_ref(), &text_model, preload_provides.as_deref());
            let Some(picked_mime_type) = mime_types.get(pick) else {
                errors::throw(Error::type_error("Cannot read properties of undefined (reading 'mimeType')"))
            };
            if picked_mime_type.mime_type == *mime_type && picked_mime_type.renderer_id == renderer.id() {
                // Same mimetype, same renderer, call the extension renderer to update
                let index = self.output_index();
                let offset = self.view_cell.get_output_offset(index);
                if let Some(result) = &render_result {
                    self.notebook_editor.update_output(&self.view_cell, result, offset);
                }
                return;
            }
        }

        let inner = self.inner_container();
        match inner {
            None => {
                // init rendering didn't happen
                let entries = self.container().rendered_output_entries();
                let curr_output_index = entries.iter().position(|entry| std::ptr::eq(Rc::as_ptr(&entry.element), self));
                let previous_sibling = match curr_output_index {
                    Some(curr) if curr > 0 => previous_sibling_of(&entries, curr),
                    _ => None,
                };
                self.render(previous_sibling);
            }
            Some(inner) => {
                // Another mimetype or renderer is picked, we need to clear the current output and re-render
                let next_element = inner.next_element_sibling();
                self.toolbar_disposables.clear();
                inner.remove();
                self.notebook_editor.remove_inset(&self.output);

                self.render(next_element);
            }
        }

        self.relayout_cell();
    }

    // insert after previousSibling
    fn generate_inner_output_container(&self, previous_sibling: Option<Rc<ElementNode>>, picked_mime_type_renderer: &IOrderedMimeType) -> Rc<ElementNode> {
        let inner_container = ElementNode::new("output-inner-container");
        *self.inner_container.borrow_mut() = Some(inner_container.clone());

        let next_sibling = previous_sibling.and_then(|previous| previous.next_element_sibling());
        match next_sibling {
            Some(next_sibling) => self.output_container.insert_before(&inner_container, &next_sibling),
            None => self.output_container.append_child(&inner_container),
        }

        inner_container.set_attribute("output-mime-type", &picked_mime_type_renderer.mime_type);
        inner_container
    }

    pub fn render(&self, previous_sibling: Option<Rc<ElementNode>>) -> Option<IRenderResult> {
        let index = self.output_index();

        if self.view_cell.is_output_collapsed() || !self.notebook_editor.has_model() {
            self.container().flag_as_stale();
            return None;
        }

        CellUri::parse(&self.view_cell.uri())?;

        let notebook_text_model = self.notebook_editor.text_model();
        let preload_provides = self.notebook_editor.active_kernel_preload_provides();

        let (mime_types, pick) = self.output.resolve_mime_types(self.notebook_service.as_ref(), &notebook_text_model, preload_provides.as_deref());
        let current_mime_type = mime_types.get(pick).cloned();
        let Some(current_mime_type) = current_mime_type.filter(|_| mime_types.iter().any(|mime_type| mime_type.is_trusted)) else {
            self.view_cell.update_output_height(index, 0.0, Some("CellOutputElement#noMimeType"));
            return None;
        };

        let selected_presentation = current_mime_type.clone();
        let mut renderer = self.notebook_service.get_renderer_info(&selected_presentation.renderer_id);
        if renderer.is_none() && selected_presentation.mime_type.contains("text/") {
            renderer = self.notebook_service.get_renderer_info("vscode.builtin-renderer");
        }

        let inner_container = self.generate_inner_output_container(previous_sibling, &selected_presentation);
        if index == 0 || self.output.visible() {
            self.attach_toolbar(&inner_container, index, &current_mime_type, &mime_types);
        } else {
            let weak = self.me.clone();
            let output_item_div = inner_container.clone();
            let current_mime_type = current_mime_type.clone();
            let mime_types = mime_types.clone();
            let visible = self.output.visible_observable();
            self.base.register(autorun(move |reader| {
                let visible = visible.read(reader);
                let Some(this) = weak.upgrade() else {
                    return;
                };
                if visible && !this.toolbar_attached.get() {
                    this.attach_toolbar(&output_item_div, index, &current_mime_type, &mime_types);
                } else if !visible {
                    this.toolbar_disposables.clear();
                }
                this.container().check_for_hidden_outputs();
            }));
            self.container().has_hidden_outputs.set(true, None);
        }

        let rendered_output_container = ElementNode::new("rendered-output");
        inner_container.append_child(&rendered_output_container);
        *self.rendered_output_container.borrow_mut() = Some(rendered_output_container);

        let render_result = match renderer {
            Some(renderer) => IInsetRenderOutput::Extension { source: self.output.clone(), mime_type: selected_presentation.mime_type.clone(), renderer },
            None => self.render_missing_renderer(&self.output, Some(selected_presentation.mime_type.as_str())),
        };

        self.output.set_picked_mime_type(Some(selected_presentation));

        *self.render_result.borrow_mut() = Some(render_result.clone());

        let offset = self.view_cell.get_output_offset(index);
        self.notebook_editor.create_output(&self.view_cell, &render_result, offset, false);
        inner_container.add_class("background");

        Some(IRenderResult { init_render_is_synchronous: false })
    }

    fn render_missing_renderer(&self, view_model: &Rc<CellOutputViewModel>, preferred_mime_type: Option<&str>) -> IInsetRenderOutput {
        let outputs = view_model.model().outputs();
        if outputs.is_empty() {
            return self.render_message(view_model, &localize("empty", "Cell has no output", &[]));
        }

        let Some(preferred_mime_type) = preferred_mime_type.filter(|mime_type| !mime_type.is_empty()) else {
            let mime_types: Vec<String> = outputs.iter().map(|output| output.mime.clone()).collect();
            let mime_types_message = mime_types.join(", ");
            return self.render_message(
                view_model,
                &localize("noRenderer.2", "No renderer could be found for output. It has the following mimetypes: {0}", &[mime_types_message.as_str()]),
            );
        };

        self.render_search_for_mimetype(view_model, preferred_mime_type)
    }

    fn render_search_for_mimetype(&self, view_model: &Rc<CellOutputViewModel>, mime_type: &str) -> IInsetRenderOutput {
        let query = format!("@tag:notebookRenderer {mime_type}");

        let p = format!(
            "<p>{}</p>",
            escape_html_text(&format!("No renderer could be found for mimetype \"{mime_type}\", but one might be available on the Marketplace."))
        );
        let href = format!("command:workbench.extensions.search?%22{query}%22");
        let a = format!(
            "<a href=\"{}\" class=\"monaco-button monaco-text-button\" tabindex=\"0\" role=\"button\" style=\"padding: 8px; text-decoration: none; color: rgb(255, 255, 255); background-color: rgb(14, 99, 156); max-width: 200px;\">Search Marketplace</a>",
            escape_html_attribute(&href)
        );

        IInsetRenderOutput::Html { source: view_model.clone(), html_content: format!("{p}{a}") }
    }

    fn render_message(&self, view_model: &Rc<CellOutputViewModel>, message: &str) -> IInsetRenderOutput {
        IInsetRenderOutput::Html { source: view_model.clone(), html_content: format!("<p>{}</p>", escape_html_text(message)) }
    }

    fn should_enable_copy(&self, mime_types: &[IOrderedMimeType]) -> bool {
        // `TEXT_BASED_MIMETYPES.indexOf(mimeType.mimeType)` é lido como booleano no original: só o índice 0 é falso (o `-1` de "não
        // achou" é verdadeiro), e o porte repete essa leitura.
        let is_copyable = |mime_type: &IOrderedMimeType| {
            TEXT_BASED_MIMETYPES.iter().position(|text_based| *text_based == mime_type.mime_type).is_none_or(|index| index != 0) || mime_type.mime_type.starts_with("image/")
        };
        if !mime_types.iter().any(is_copyable) {
            return false;
        }

        let Some(first) = mime_types.first() else {
            errors::throw(Error::type_error("Cannot read properties of undefined (reading 'mimeType')"))
        };
        if is_text_stream_mime(&first.mime_type) {
            let index = self.view_cell.outputs_view_models().iter().position(|output| Rc::ptr_eq(output, &self.output));
            if let Some(index) = index
                && index > 0
            {
                let model_outputs = self.view_cell.model.outputs();
                let previous_output = &model_outputs[index - 1];
                // if the previous output was also a stream, the copy command will be in that output instead
                let Some(previous_item) = previous_output.outputs().into_iter().next() else {
                    errors::throw(Error::type_error("Cannot read properties of undefined (reading 'mime')"))
                };
                return !is_text_stream_mime(&previous_item.mime);
            }
        }

        true
    }

    fn attach_toolbar(&self, output_item_div: &Rc<ElementNode>, index: usize, current_mime_type: &IOrderedMimeType, mime_types: &[IOrderedMimeType]) {
        let has_multiple_mime_types = mime_types.iter().filter(|mime_type| mime_type.is_trusted).count() > 1;
        let is_copy_enabled = self.should_enable_copy(mime_types);
        if index > 0 && !has_multiple_mime_types && !is_copy_enabled {
            // nothing to put in the toolbar
            return;
        }

        if !self.notebook_editor.has_model() {
            return;
        }

        output_item_div.set_position_relative(true);
        let mime_type_picker = ElementNode::new("cell-output-toolbar");

        output_item_div.append_child(&mime_type_picker);

        let toolbar = self.toolbar_disposables.add_rc(Rc::new(OutputToolbar::new(OutputToolbarContext {
            ui: true,
            cell: self.view_cell.clone(),
            output_view_model: self.output.clone(),
        })));
        mime_type_picker.set_toolbar(Some(toolbar.clone()));

        let pick_action = self.toolbar_disposables.add_rc(Action::new(
            "notebook.output.pickMimetype",
            &localize("pickMimeType", "Change Presentation", &[]),
            "codicon codicon-notebook-mimetype",
            true,
            None,
        ));

        let menu_context_key_service = self.context_key_service.create_scoped(output_item_div.clone() as Rc<dyn IContextKeyServiceTarget>);
        {
            let scoped = menu_context_key_service.clone();
            self.toolbar_disposables.add(to_disposable(move || scoped.dispose()));
        }
        let has_hidden_outputs = NOTEBOOK_CELL_HAS_HIDDEN_OUTPUTS.with(|key| key.bind_to(&*menu_context_key_service));
        let is_first_cell_output = NOTEBOOK_CELL_IS_FIRST_OUTPUT.with(|key| key.bind_to(&*menu_context_key_service));
        let cell_output_mimetype = NOTEBOOK_CELL_OUTPUT_MIMETYPE.with(|key| key.bind_to(&*menu_context_key_service));
        is_first_cell_output.set(Dynamic::Bool(index == 0));
        cell_output_mimetype.set(Dynamic::from(current_mime_type.mime_type.clone()));
        let container = self.cell_output_container.clone();
        self.toolbar_disposables.add(autorun(move |reader| {
            if let Some(container) = container.upgrade() {
                has_hidden_outputs.set(Dynamic::Bool(container.has_hidden_outputs.read(reader)));
            }
        }));
        let scoped_key_service: Rc<dyn IContextKeyService> = menu_context_key_service.clone();
        let menu = self.menu_service.create_menu(&notebook_output_toolbar(), scoped_key_service, None);
        {
            let menu = menu.clone();
            self.toolbar_disposables.add(to_disposable(move || menu.dispose()));
        }

        let weak_menu = Rc::downgrade(&menu);
        let update_menu_toolbar: Rc<dyn Fn()> = Rc::new(move || {
            let Some(menu) = weak_menu.upgrade() else {
                return;
            };
            let groups = menu.get_actions(Some(&MenuActionOptions { should_forward_args: true, ..Default::default() }));
            let mut secondary = secondary_actions_of(groups);
            if !is_copy_enabled {
                secondary.retain(|action| action.id() != COPY_OUTPUT_COMMAND_ID);
            }
            if has_multiple_mime_types {
                secondary.insert(0, pick_action.clone());
            }

            toolbar.set_actions(Vec::new(), secondary);
        });
        update_menu_toolbar();
        let on_change = update_menu_toolbar.clone();
        self.toolbar_disposables.add(menu.on_did_change().listen(move |_| on_change()));
    }

    fn relayout_cell(&self) {
        self.notebook_editor.layout_notebook_cell(&self.view_cell, self.view_cell.layout_info().total_height);
    }
}

impl IDisposable for CellOutputElement {
    fn dispose(&self) {
        if let Some(timer) = self.output_height_timer.get() {
            self.view_cell.unlock_output_height();
            runtime::clear_timeout(timer);
        }

        self.base.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "CellOutputElement"
    }
}

/// `CellOutputContainer`.
pub struct CellOutputContainer {
    base: Rc<Disposable>,
    me: Weak<CellOutputContainer>,
    notebook_editor: Rc<dyn ICellOutputEditor>,
    view_cell: Rc<CodeCellViewModel>,
    template_data: Rc<CodeCellRenderTemplate>,
    limit: usize,
    instantiation_service: Rc<InstantiationService>,
    notebook_execution_state_service: Rc<dyn INotebookExecutionStateService>,
    output_entries: RefCell<Vec<OutputEntryViewHandler>>,
    has_stale_outputs: Cell<bool>,
    pub has_hidden_outputs: ISettableObservable<bool>,
    output_height_timer: Cell<Option<TimerId>>,
}

impl CellOutputContainer {
    /// `new CellOutputContainer(notebookEditor, viewCell, templateData, { limit }, openerService, executionStateService, instantiationService)`.
    pub fn new(
        notebook_editor: Rc<dyn ICellOutputEditor>,
        view_cell: Rc<CodeCellViewModel>,
        template_data: Rc<CodeCellRenderTemplate>,
        limit: usize,
        instantiation_service: Rc<InstantiationService>,
    ) -> Rc<Self> {
        let notebook_execution_state_service = instantiation_service.invoke_function(|accessor| accessor.get(I_NOTEBOOK_EXECUTION_STATE_SERVICE));
        let this = Rc::new_cyclic(|me| Self {
            base: Disposable::new(),
            me: me.clone(),
            notebook_editor,
            view_cell,
            template_data,
            limit,
            instantiation_service,
            notebook_execution_state_service,
            output_entries: RefCell::new(Vec::new()),
            has_stale_outputs: Cell::new(false),
            has_hidden_outputs: observable_value::<bool, ()>("hasHiddenOutputs", false),
            output_height_timer: Cell::new(None),
        });

        let weak = this.me.clone();
        this.base.register(this.view_cell.on_did_start_execution().listen(move |_| {
            if let Some(this) = weak.upgrade() {
                this.view_cell.update_output_min_height(this.view_cell.layout_info().output_total_height);
            }
        }));

        let weak = this.me.clone();
        this.base.register(this.view_cell.on_did_stop_execution().listen(move |_| {
            if let Some(this) = weak.upgrade() {
                this.validate_final_output_height(false);
            }
        }));

        let weak = this.me.clone();
        this.base.register(this.view_cell.on_did_change_outputs().listen(move |splice| {
            if let Some(this) = weak.upgrade() {
                let execution_state = this.notebook_execution_state_service.get_cell_execution(&this.view_cell.uri());
                let context = if execution_state.is_some() { CellOutputUpdateContext::Execution } else { CellOutputUpdateContext::Other };
                this.update_outputs(splice, context);
            }
        }));

        let weak = this.me.clone();
        this.base.register(this.view_cell.on_did_change_layout().listen(move |_| {
            if let Some(this) = weak.upgrade() {
                this.update_internal_layout_now();
            }
        }));

        this
    }

    pub fn check_for_hidden_outputs(&self) {
        let has_hidden = self.output_entries.borrow().iter().any(|entry| !entry.model.visible());
        self.has_hidden_outputs.set(has_hidden, None);
    }

    /// `renderedOutputEntries`.
    pub fn rendered_output_entries(&self) -> Vec<OutputEntryViewHandler> {
        self.output_entries.borrow().clone()
    }

    /// `createInstance(CellOutputElement, ...)`: os serviços do elemento vêm do `InstantiationService`.
    fn create_element(&self, output: &Rc<CellOutputViewModel>) -> Rc<CellOutputElement> {
        let (notebook_service, menu_service, context_key_service) = self
            .instantiation_service
            .invoke_function(|accessor| (accessor.get(I_NOTEBOOK_SERVICE), accessor.get(I_MENU_SERVICE), accessor.get(I_CONTEXT_KEY_SERVICE)));
        CellOutputElement::new(
            self.notebook_editor.clone(),
            self.view_cell.clone(),
            self.me.clone(),
            self.template_data.output_container.clone(),
            output.clone(),
            notebook_service,
            menu_service,
            context_key_service,
        )
    }

    fn create_entry(&self, output: &Rc<CellOutputViewModel>) -> OutputEntryViewHandler {
        OutputEntryViewHandler { model: output.clone(), element: self.create_element(output) }
    }

    pub fn update_internal_layout_now(&self) {
        let layout_info = self.view_cell.layout_info();
        self.template_data.output_container.set_top(layout_info.output_container_offset);
        self.template_data.output_show_more_container.set_top(layout_info.output_show_more_container_offset);

        let outputs = self.view_cell.outputs_view_models();
        for entry in self.rendered_output_entries() {
            if let Some(index) = outputs.iter().position(|output| Rc::ptr_eq(output, &entry.model)) {
                let top = self.view_cell.get_output_offset_in_container(index);
                entry.element.update_dom_top(top);
            }
        }
    }

    pub fn render(&self) {
        let result = catch_unwind(AssertUnwindSafe(|| self.do_render()));
        // TODO@rebornix, this is probably not necessary at all as cell layout change would send the update request.
        self.relayout_cell();
        if let Err(payload) = result {
            resume_unwind(payload);
        }
    }

    /// Notify that an output may have been swapped out without the model getting rendered.
    pub fn flag_as_stale(&self) {
        self.has_stale_outputs.set(true);
    }

    fn do_render(&self) {
        let outputs = self.view_cell.outputs_view_models();
        if !outputs.is_empty() {
            let output_total_height = self.view_cell.layout_info().output_total_height;
            if output_total_height != 0.0 {
                self.view_cell.update_output_min_height(output_total_height);
            }

            self.template_data.output_container.show();
            for output in outputs.iter().take(self.limit) {
                let entry = self.create_entry(output);
                let element = entry.element.clone();
                self.output_entries.borrow_mut().push(entry);
                element.render(None);
            }

            if outputs.len() > self.limit {
                self.template_data.output_show_more_container.show();
                self.view_cell.update_output_show_more_container_height(46.0);
            }

            self.validate_final_output_height(false);
        } else {
            // noop
            self.template_data.output_container.hide();
        }

        self.template_data.output_show_more_container.clear_children();
        if outputs.len() > self.limit {
            let element = self.generate_show_more_element(&self.template_data.template_disposables);
            self.template_data.output_show_more_container.append_child(&element);
        } else {
            self.template_data.output_show_more_container.hide();
            self.view_cell.update_output_show_more_container_height(0.0);
        }
    }

    pub fn view_update_show_outputs(&self, _init_rendering: bool) {
        if self.has_stale_outputs.get() {
            self.has_stale_outputs.set(false);
            for entry in self.rendered_output_entries() {
                entry.element.rerender();
            }
        }

        let entries = self.rendered_output_entries();
        for (index, view_handler) in entries.iter().enumerate() {
            let output_entry = &view_handler.element;
            match output_entry.render_result() {
                Some(render_result) => {
                    let offset = self.view_cell.get_output_offset(index);
                    self.notebook_editor.create_output(&self.view_cell, &render_result, offset, false);
                }
                None => {
                    output_entry.render(None);
                }
            }
        }

        self.relayout_cell();
    }

    pub fn view_update_hide_ouputs(&self) {
        for entry in self.rendered_output_entries() {
            self.notebook_editor.hide_inset(&entry.model);
        }
    }

    fn validate_final_output_height(&self, synchronous: bool) {
        if let Some(timer) = self.output_height_timer.get() {
            runtime::clear_timeout(timer);
        }

        let execution_state = self.notebook_execution_state_service.get_cell_execution(&self.view_cell.uri());

        if synchronous {
            self.view_cell.unlock_output_height();
        } else if !execution_state.is_some_and(|execution| execution.state() == NotebookCellExecutionState::Executing) {
            let weak = self.me.clone();
            self.output_height_timer.set(Some(runtime::set_timeout(
                move || {
                    if let Some(this) = weak.upgrade() {
                        this.view_cell.unlock_output_height();
                    }
                },
                200,
            )));
        }
    }

    fn update_outputs(&self, splice: &NotebookCellOutputsSplice, context: CellOutputUpdateContext) {
        let previous_output_height = self.view_cell.layout_info().output_total_height;

        // for cell output update, we make sure the cell does not shrink before the new outputs are rendered.
        self.view_cell.update_output_min_height(previous_output_height);

        if !self.view_cell.outputs_view_models().is_empty() {
            self.template_data.output_container.show();
        } else {
            self.template_data.output_container.hide();
        }

        let heights: Vec<f64> = splice.new_outputs.iter().map(|_| 0.0).collect();
        self.view_cell.splice_output_heights(splice.start, splice.delete_count, &heights);
        self.render_now(splice, context);
    }

    fn render_now(&self, splice: &NotebookCellOutputsSplice, context: CellOutputUpdateContext) {
        if splice.start >= self.limit {
            // splice items out of limit
            return;
        }

        let entries = self.rendered_output_entries();
        let outputs = self.view_cell.outputs_view_models();
        let first_group_entries = js_slice(&entries, 0, splice.start);
        let deleted_entries = js_slice(&entries, splice.start, splice.start + splice.delete_count);
        let second_group_entries = js_slice(&entries, splice.start + splice.delete_count, entries.len());
        let mut newly_inserted = js_slice(&outputs, splice.start, splice.start + splice.new_outputs.len());

        // [...firstGroup, ...deletedEntries, ...secondGroupEntries]  [...restInModel]
        // [...firstGroup, ...newlyInserted, ...secondGroupEntries, restInModel]
        if first_group_entries.len() + newly_inserted.len() + second_group_entries.len() > self.limit {
            // exceeds limit again
            if first_group_entries.len() + newly_inserted.len() > self.limit {
                for entry in deleted_entries.iter().chain(second_group_entries.iter()) {
                    entry.element.detach();
                    entry.element.dispose();
                }

                newly_inserted.truncate(self.limit.saturating_sub(first_group_entries.len()));
                let newly_inserted_entries: Vec<OutputEntryViewHandler> = newly_inserted.iter().map(|insert| self.create_entry(insert)).collect();

                let new_entries: Vec<OutputEntryViewHandler> = first_group_entries.iter().cloned().chain(newly_inserted_entries).collect();
                *self.output_entries.borrow_mut() = new_entries.clone();

                // render newly inserted outputs
                for entry in new_entries.iter().skip(first_group_entries.len()) {
                    entry.element.render(None);
                }
            } else {
                // part of secondGroupEntries are pushed out of view
                // now we have to be creative as secondGroupEntries might not use dedicated containers
                let kept = self.limit.saturating_sub(first_group_entries.len()).saturating_sub(newly_inserted.len());
                let elements_pushed_out_of_view = js_slice(&second_group_entries, kept, second_group_entries.len());
                for entry in deleted_entries.iter().chain(elements_pushed_out_of_view.iter()) {
                    entry.element.detach();
                    entry.element.dispose();
                }

                // exclusive
                let re_render_right_boundary = first_group_entries.len() + newly_inserted.len();

                let newly_inserted_entries: Vec<OutputEntryViewHandler> = newly_inserted.iter().map(|insert| self.create_entry(insert)).collect();

                let new_entries: Vec<OutputEntryViewHandler> = first_group_entries
                    .iter()
                    .cloned()
                    .chain(newly_inserted_entries)
                    .chain(js_slice(&second_group_entries, 0, kept))
                    .collect();
                *self.output_entries.borrow_mut() = new_entries.clone();

                for i in first_group_entries.len()..re_render_right_boundary {
                    let previous_sibling = previous_sibling_of(&new_entries, i);
                    new_entries[i].element.render(previous_sibling);
                }
            }
        } else {
            // after splice, it doesn't exceed
            for entry in &deleted_entries {
                entry.element.detach();
                entry.element.dispose();
            }

            let re_render_right_boundary = first_group_entries.len() + newly_inserted.len();

            let newly_inserted_entries: Vec<OutputEntryViewHandler> = newly_inserted.iter().map(|insert| self.create_entry(insert)).collect();

            let mut outputs_newly_available: Vec<OutputEntryViewHandler> = Vec::new();

            if first_group_entries.len() + newly_inserted_entries.len() + second_group_entries.len() < outputs.len() {
                let last = self.limit.min(outputs.len());
                outputs_newly_available = js_slice(&outputs, first_group_entries.len() + newly_inserted_entries.len() + second_group_entries.len(), last)
                    .iter()
                    .map(|output| self.create_entry(output))
                    .collect();
            }

            let new_entries: Vec<OutputEntryViewHandler> = first_group_entries
                .iter()
                .cloned()
                .chain(newly_inserted_entries)
                .chain(second_group_entries.iter().cloned())
                .chain(outputs_newly_available.iter().cloned())
                .collect();
            *self.output_entries.borrow_mut() = new_entries.clone();

            for i in first_group_entries.len()..re_render_right_boundary {
                let previous_sibling = previous_sibling_of(&new_entries, i);
                new_entries[i].element.render(previous_sibling);
            }

            for i in 0..outputs_newly_available.len() {
                new_entries[first_group_entries.len() + newly_inserted.len() + second_group_entries.len() + i].element.render(None);
            }
        }

        if outputs.len() > self.limit {
            self.template_data.output_show_more_container.show();
            if !self.template_data.output_show_more_container.has_child_nodes() {
                let element = self.generate_show_more_element(&self.template_data.template_disposables);
                self.template_data.output_show_more_container.append_child(&element);
            }
            self.view_cell.update_output_show_more_container_height(46.0);
        } else {
            self.template_data.output_show_more_container.hide();
        }

        self.relayout_cell();
        // if it's clearing all outputs, or outputs are all rendered synchronously
        // shrink immediately as the final output height will be zero.
        // if it's rerun, then the output clearing might be temporary, so we don't shrink immediately
        self.validate_final_output_height(context == CellOutputUpdateContext::Other && self.view_cell.outputs_view_models().is_empty());
    }

    fn generate_show_more_element(&self, disposables: &Rc<DisposableStore>) -> Rc<ElementNode> {
        let element = ElementNode::new("output-show-more");
        element.set_text(&format!(
            "There are more than {} outputs, [show more (open the raw output data in a text editor) ...]({})",
            self.limit, OPEN_LARGE_OUTPUT_COMMAND
        ));

        let notebook_editor = self.notebook_editor.clone();
        let instantiation_service = self.instantiation_service.clone();
        element.set_link_handler(Some(Rc::new(move |content: &str| {
            if content == OPEN_LARGE_OUTPUT_COMMAND {
                let opener_service = instantiation_service.invoke_function(|accessor| accessor.get(I_OPENER_SERVICE));
                let uri = CellUri::generate_cell_output_uri_with_id(&notebook_editor.text_model().uri(), None);
                drop(opener_service.open(&OpenResource::Uri(uri), None));
            }
        })));
        let rendered = element.clone();
        disposables.add(to_disposable(move || rendered.set_link_handler(None)));

        element
    }

    fn relayout_cell(&self) {
        self.notebook_editor.layout_notebook_cell(&self.view_cell, self.view_cell.layout_info().total_height);
    }
}

impl IDisposable for CellOutputContainer {
    fn dispose(&self) {
        self.view_cell.update_output_min_height(0.0);

        if let Some(timer) = self.output_height_timer.get() {
            runtime::clear_timeout(timer);
        }

        for entry in self.rendered_output_entries() {
            entry.element.dispose();
        }

        self.base.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "CellOutputContainer"
    }
}
