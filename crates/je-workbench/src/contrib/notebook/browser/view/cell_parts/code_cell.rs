//! Porte de `src/vs/workbench/contrib/notebook/browser/view/cellParts/codeCell.ts` (a classe `CodeCell`; o `CodeCellLayout` é de
//! `code_cell_layout.rs`): liga uma célula de código ao template dela, com o editor da célula, a conta de layout do editor, o foco, as
//! partes de conteúdo e os ouvintes do editor, do notebook e do view model. A classe inteira está aqui: o construtor e os ouvintes, o
//! estado recolhido da entrada e da saída (`updateForCollapseState` e a prévia da primeira linha), o layout do editor
//! (`layoutEditor`, `onCellWidthChange`, `onCellEditorHeightChange` e `relayoutCell`) e o `dispose()` sobrescrito.
//!
//! Próxima: `cellRenderer.ts` (que cria o template, o editor e as partes de `CodeCellTemplateData`) e os 5 casos de
//! `cellEditorOverflowWidgets.test.ts`.
//!
//! Divergências:
//! - **A prévia recolhida da entrada** (`_collapseInput`, `_attachInputExpandButton`, `_getRichText`, `_getRichTextFromLineTokens` e o
//!   ouvinte de `onDidChangeTokens`) usa a montagem de `markup_cell.rs` (a prévia é a mesma `.cell-collapse-preview` das duas
//!   células): o HTML tokenizado vira os trechos de um parágrafo ([`collapsed_preview_spans_from_html`]), e o `innerHTML = richText`
//!   mais o `_attachInputExpandButton(element)` são [`fill_collapsed_preview`] (limpar o elemento e montar de novo, o mesmo efeito de
//!   trocar o `innerHTML`). O `collapsedCellTTPolicy` (Trusted Types) não existe. O `display: flex` do contêiner recolhido põe a
//!   prévia depois do ícone de execução, se ele estiver visível. `_removeInputCollapsePreview` percorre os filhos com a classe
//!   `cell-collapse-preview` (`children_with_class`) e esquece o elemento (o original mantém a referência a um elemento já fora da
//!   árvore, que nenhum ouvinte lê). `_getRichTextFromLineTokens` lê o texto da linha dos próprios tokens da linha 1 (o mesmo
//!   `getLineContent(1)` do modelo, que o `LineTokens` guarda).
//! - **`outputContainer.domNode.children`** (`_updateOutputInnerContainer`) são os filhos do `ElementNode` do contêiner de saídas, e
//!   `DOM.setVisibility` sobre eles é `show` e `hide` do próprio nó. O `viewUpdateHideOuputs` mantém a grafia do original.
//! - O `${this.templateData.editor.getLayoutInfo()}` que o original interpola num objeto (`[object Object]`) sai com esse mesmo texto
//!   no `_debug` de `onCellEditorHeightChange`.
//! - **DOM é a árvore do `je-ui`.** Os elementos do template são [`NodeId`]s do [`CodeCellTemplateData`] (o `CodeCellRenderTemplate`
//!   completo; o `CodeCellRenderTemplate` de `cell_output.rs` é a fatia dele que as saídas leem e vai dentro como `output`).
//!   `classList.toggle` é `toggle_class`, `DOM.show` e `DOM.hide` são `show` e `hide`, `DOM.setVisibility` é [`set_visibility`],
//!   `executionItemElement.remove()` é `Tree::remove`, e `editorPart.style.top` é o `top` dos `bounds` do nó (o `''` é o `top` zero
//!   do fluxo normal).
//! - **`INotebookEditorDelegate`** é o contrato estreito [`ICodeCellNotebookEditor`], que estende o [`ICellOutputEditor`] das saídas.
//!   `DOM.getWindow(notebookEditor.getDomNode())` é `target_window()`, e `ownerDocument.activeElement === body` é nenhum nó focado na
//!   [`Ui`] (como em `markup_cell.rs`).
//! - **O editor da célula** (`templateData.editor`, o `CodeEditorWidget` do pool) é o contrato [`ICodeCellEditor`], que estende o
//!   [`IMarkupCellEditor`] de `markup_cell.rs` (a parte do `CodeEditorWidget` que as duas células chamam) com o que só a célula de
//!   código usa: `getLayoutInfo`, `setScrollTop`, `hasTextFocus`, `getOptions` (`readOnly` e `padding`), `onDidScrollChange`, os
//!   eventos de mouse e `CodeActionController.get(editor)?.hideLightBulbWidget()`. O `e.event.preventDefault()` do `onMouseDown`
//!   (o evento chega por referência) é `prevent_mouse_event_default`. O `options.get(EditorOption.padding) !== padding` compara por
//!   identidade um objeto sempre novo e portanto sempre difere; aqui a comparação é por valor (`padding_option() != padding`), então
//!   o `updateOptions` redundante é pulado.
//! - **A ligação com o view model do editor** (`attachTextEditor(editor, estimatedHasHorizontalScrolling)`, `detachTextEditor` e
//!   `resolveTextModel` de `BaseCellViewModel`, que o view model do porte não tem) é [`ICodeCellEditorHost`], o par de
//!   `IMarkupCellEditorHost` com o argumento `estimatedHasHorizontalScrolling` que a célula de markup não passa. O
//!   `NotebookCellEditorPool` é o contrato [`ICodeCellEditorPool`].
//! - **`CodeCellLayout` e seus colaboradores.** A conta de `code_cell_layout.rs` fala com cinco contratos estreitos que não têm a
//!   célula como argumento; [`CodeCellLayoutHost`] os implementa sobre o delegate, o template e o view model, e é também o
//!   `_debug` da classe (`notebookLogService.debug('CellLayout', `${debugPrefix} ${output}`)`).
//! - **Eventos da janela** (`addDisposableListener(win, ...)` de `registerMouseListener`): a janela é a raiz da árvore do `je-ui`.
//!   `mouseup` e `pointerup` são o mesmo `PointerUp`, ouvido uma vez só; `keydown` é o `KeyDown` na raiz; `blur` da janela é o `Blur`
//!   em captura na raiz (qualquer nó que perca o foco zera o estado do ponteiro, o que é o mesmo efeito de recuperação); o
//!   `pointercancel` não existe na árvore.
//! - **Serviços injetados chegam como argumentos** ([`CodeCellServices`]). O `IInstantiationService.createInstance(CellOutputContainer,
//!   ...)` é `CellOutputContainer::new` com o `InstantiationService` do porte (ver `cell_output.rs`), que agora é também um
//!   `CellContentPart`, para entrar no `concatContentPart`. O `CollapsedCodeCellExecutionIcon` recebe o serviço de execução e o estilo
//!   do rótulo.
//! - **Partes são `Rc`.** Os ouvintes guardam um `Weak` da célula (o `this` do original), então o construtor devolve `Rc<Self>`;
//!   `_register(x)` é o `register` do [`Disposable`] da célula. O `collapsedExecutionIcon`, criado no meio do construtor depois de a
//!   célula existir, é um [`OnceCell`].
//! - **`_useNewApproachForEditorLayout`** é a constante [`USE_NEW_APPROACH_FOR_EDITOR_LAYOUT`] (nunca muda no original); o ramo antigo
//!   de `adjustEditorPosition` fica portado, atrás dela.
//! - `raceCancellation(...).then(...)` mais `new CancellationTokenSource()` e `cts.dispose(true)` são `race_cancellation`,
//!   `CancellationTokenSource::dispose_with(true)` e `Promise::then`. `model.updateOptions({ indentSize, tabSize, insertSpaces })` é
//!   [`CellEditorOptions::text_model_update_options`]. `DOM.modify(window, cb)` é `schedule_at_next_animation_frame` com
//!   [`MODIFY_PRIORITY`] e `DOM.runAtThisOrScheduleAtNextAnimationFrame` é o método de mesmo nome da janela (prioridade 0).
//! - O `layoutInfo.fontInfo?.lineHeight || 17` trata `0` como ausente, como o `||`.
//! - `focusEditorIfNeeded` (a função local de `initializeEditor`) tem a mesma condição de `shouldPreserveEditor`, então as duas são
//!   [`CodeCell::should_preserve_editor`].

use std::cell::{Cell, OnceCell, RefCell};
use std::rc::{Rc, Weak};

use je_base::JsString;
use je_base::async_::race_cancellation;
use je_base::browser::dom::CodeWindow;
use je_base::cancellation::CancellationTokenSource;
use je_base::dynamic::Dynamic;
use je_base::event::{Event, any as any_event};
use je_base::key_codes::KeyCode;
use je_base::lifecycle::{Disposable, IDisposable, MutableDisposable, to_disposable};
use je_base::promise::Promise;
use je_base::scrollable::ScrollEvent;
use je_base::strings::escape;
use je_editor::editor_browser::EditorMouseEvent;
use je_editor::language_service::ILanguageService;
use je_editor::line_tokens::IViewLineTokens;
use je_editor::range::Range;
use je_editor::text_model::TextModel;
use je_editor::text_model_events::ModelTokensChangedEvent;
use je_instantiation::InstantiationService;
use je_platform::configuration::service::IConfigurationService;
use je_platform::keybinding::keybinding::IKeybindingService;
use je_render::kurbo::Rect;
use je_ui::{Event as UiEvent, EventKind, NodeId, Role, Tree, Ui};
use je_widgets::dom::{clear_node, create_el, hide, show};
use je_widgets::highlighted_label::HighlightedLabelStyles;

use super::cell_editor_options::{CellEditorOptions, ICellEditorBaseOptions};
use super::cell_editor_overflow_widgets::CellEditorOverflowWidgets;
use super::cell_output::{CellOutputContainer, CodeCellRenderTemplate, ICellOutputEditor};
use super::code_cell_execution_icon::CollapsedCodeCellExecutionIcon;
use super::code_cell_layout::{
    CellEditorDimension, CellLayoutChangeReason, CodeCellEditorVisibility, CodeCellLayout, ICodeCellLayoutCell, ICodeCellLayoutEditor, ICodeCellLayoutEditorPart,
    ICodeCellLayoutLogService, ICodeCellLayoutNotebook,
};
use super::markup_cell::{
    CollapsedPreviewNodes, CollapsedPreviewStyles, EXPAND_CELL_INPUT_COMMAND_ID, IMarkupCellEditor, collapsed_preview_spans_from_html, fill_collapsed_preview, layout_collapsed_preview,
    tokenize_line_to_html,
};
use crate::contrib::notebook::browser::notebook_options::{EditorPadding, NotebookOptions};
use crate::contrib::notebook::browser::notebook_view_events::NotebookLayoutInfo;
use crate::contrib::notebook::browser::view::cell_part::{CellContentPart, CellPartsCollection, MODIFY_PRIORITY};
use crate::contrib::notebook::browser::view_model::cell_view_model_types::{CellFocusMode, CellLayoutChangeEvent, CodeCellLayoutInfo};
use crate::contrib::notebook::browser::view_model::code_cell_view_model::{CodeCellViewModel, OUTPUT_DISPLAY_LIMIT};
use crate::contrib::notebook::browser::view_model::notebook_view_model_impl::CellViewModel;
use crate::contrib::notebook::common::notebook_execution_state_service::INotebookExecutionStateService;
use crate::contrib::notebook::common::notebook_logging_service::INotebookLoggingService;

/// `_useNewApproachForEditorLayout` (ver as divergências do módulo).
pub const USE_NEW_APPROACH_FOR_EDITOR_LAYOUT: bool = true;

/// A categoria do `notebookLogService.debug('CellLayout', ...)`.
const LAYOUT_LOG_CATEGORY: &str = "CellLayout";
/// A altura de linha que o `||` assume quando a célula ainda não tem `fontInfo`.
const FALLBACK_LINE_HEIGHT: f64 = 17.0;
/// A altura da barra de ferramentas do notebook que o ramo antigo de `adjustEditorPosition` desconta.
const NOTEBOOK_TOOLBAR_HEIGHT: f64 = 26.0;
/// A distância até o topo do editor da célula (6px sob o indicador de foco) mais a borda (1px), do ramo antigo de
/// `adjustEditorPosition`.
const EDITOR_TOP_EXTRA_OFFSET: f64 = -6.0 - 1.0;

/// `DOM.setVisibility(visible, node)`: `show` ou `hide`.
fn set_visibility(tree: &mut Tree, visible: bool, node: NodeId) {
    if visible {
        show(tree, node);
    } else {
        hide(tree, node);
    }
}

/// O editor da célula de código (`templateData.editor`), só com o que `CodeCell` chama (ver as divergências do módulo).
pub trait ICodeCellEditor: IMarkupCellEditor {
    /// `getLayoutInfo()`, no `width` e no `height`.
    fn get_layout_info(&self) -> CellEditorDimension;
    /// `setScrollTop(scrollTop)`.
    fn set_scroll_top(&self, scroll_top: f64);
    /// `hasTextFocus()`.
    fn has_text_focus(&self) -> bool;
    /// `getOptions().get(EditorOption.readOnly)`.
    fn read_only_option(&self) -> bool;
    /// `getOptions().get(EditorOption.padding)`.
    fn padding_option(&self) -> EditorPadding;
    /// `getModel()`.
    fn get_model(&self) -> Option<Rc<TextModel>>;
    /// `onDidScrollChange`.
    fn on_did_scroll_change(&self) -> Event<ScrollEvent>;
    /// `onMouseDown`.
    fn on_mouse_down(&self) -> Event<EditorMouseEvent>;
    /// `onMouseMove`.
    fn on_mouse_move(&self) -> Event<EditorMouseEvent>;
    /// `e.event.preventDefault()` do evento em despacho.
    fn prevent_mouse_event_default(&self, event: &EditorMouseEvent);
    /// `CodeActionController.get(editor)?.hideLightBulbWidget()`: sem o controlador não faz nada.
    fn hide_light_bulb_widget(&self);
}

/// O que `CodeCell` chama do `CodeCellViewModel` e que o view model do porte ainda não tem (ver as divergências do módulo).
pub trait ICodeCellEditorHost {
    /// `attachTextEditor(editor, estimatedHasHorizontalScrolling)`.
    fn attach_text_editor(&self, cell: &CellViewModel, editor: Rc<dyn ICodeCellEditor>, estimated_has_horizontal_scrolling: bool);
    /// `detachTextEditor()`.
    fn detach_text_editor(&self, cell: &CellViewModel);
    /// `resolveTextModel()`.
    fn resolve_text_model(&self, cell: &CellViewModel) -> Promise<Rc<TextModel>>;
}

/// `NotebookCellEditorPool`, só com o que `CodeCell` chama.
pub trait ICodeCellEditorPool {
    /// `preserveFocusedEditor(cell)`.
    fn preserve_focused_editor(&self, cell: &CellViewModel);
}

/// O que `CodeCell` chama do `IActiveNotebookEditorDelegate`, além do que as saídas chamam ([`ICellOutputEditor`]).
pub trait ICodeCellNotebookEditor: ICellOutputEditor {
    /// `getCellIndex(cell)`.
    fn get_cell_index(&self, cell: &CellViewModel) -> i64;
    /// `getBaseCellEditorOptions(language)`.
    fn get_base_cell_editor_options(&self, language: &str) -> Rc<dyn ICellEditorBaseOptions>;
    /// `notebookOptions`.
    fn notebook_options(&self) -> Rc<NotebookOptions>;
    /// `DOM.getWindow(getDomNode())`.
    fn target_window(&self) -> Rc<CodeWindow>;
    /// `scrollTop`.
    fn scroll_top(&self) -> f64;
    /// `scrollBottom`.
    fn scroll_bottom(&self) -> f64;
    /// `setScrollTop(scrollTop)`.
    fn set_scroll_top(&self, scroll_top: f64);
    /// `getLayoutInfo()`.
    fn get_layout_info(&self) -> NotebookLayoutInfo;
    /// `getAbsoluteTopOfElement(cell)`.
    fn get_absolute_top_of_element(&self, cell: &CellViewModel) -> f64;
    /// `getAbsoluteBottomOfElement(cell)`.
    fn get_absolute_bottom_of_element(&self, cell: &CellViewModel) -> f64;
    /// `getHeightOfElement(cell)`.
    fn get_height_of_element(&self, cell: &CellViewModel) -> f64;
    /// `isReadOnly`.
    fn is_read_only(&self) -> bool;
    /// `onDidScroll`.
    fn on_did_scroll(&self) -> Event<()>;
    /// `onDidChangeLayout`.
    fn on_did_change_layout(&self) -> Event<()>;
    /// `getActiveCell()`.
    fn get_active_cell(&self) -> Option<CellViewModel>;
    /// `hasEditorFocus()`.
    fn has_editor_focus(&self) -> bool;
    /// `revealRangeInViewAsync(cell, range)`.
    fn reveal_range_in_view_async(&self, cell: &CellViewModel, range: &Range);
}

/// `CodeCellRenderTemplate` (`notebookRenderingCommon.ts`), só com os campos que `CodeCell` lê. O `cellRenderer.ts` cria os nós, o
/// editor e as partes.
pub struct CodeCellTemplateData {
    /// O que as saídas leem do template (`outputContainer`, `outputShowMoreContainer`, `templateDisposables`).
    pub output: Rc<CodeCellRenderTemplate>,
    /// `container` (`.cell-inner-container`).
    pub container: NodeId,
    /// `editorPart`.
    pub editor_part: NodeId,
    /// `editor`.
    pub editor: Rc<dyn ICodeCellEditor>,
    /// `editorOverflowWidgets`.
    pub editor_overflow_widgets: Rc<CellEditorOverflowWidgets>,
    /// `cellInputCollapsedContainer`.
    pub cell_input_collapsed_container: NodeId,
    /// `cellOutputCollapsedContainer`.
    pub cell_output_collapsed_container: NodeId,
    /// `focusSinkElement`.
    pub focus_sink_element: NodeId,
    /// `cellParts`.
    pub cell_parts: Rc<CellPartsCollection>,
}

/// As cores, as fontes e as medidas que o CSS dá aos nós que a célula de código desenha, montadas por quem tem o tema.
#[derive(Clone)]
pub struct CodeCellStyles {
    /// O estilo do rótulo do `CollapsedCodeCellExecutionIcon`.
    pub collapsed_execution_icon: HighlightedLabelStyles,
    /// As cores, a fonte e as medidas da prévia recolhida da entrada (`.cell-collapse-preview`).
    pub collapsed_preview: CollapsedPreviewStyles,
}

/// Os serviços que o `createInstance` do original injetava em `CodeCell`, mais o pool de editores (argumento `editorPool`).
pub struct CodeCellServices {
    /// `@IInstantiationService`, no que `CodeCell` usa dele: criar o `CellOutputContainer`.
    pub instantiation_service: Rc<InstantiationService>,
    /// `@IKeybindingService`.
    pub keybinding_service: Rc<dyn IKeybindingService>,
    /// `@ILanguageService`.
    pub language_service: Rc<dyn ILanguageService>,
    /// `@IConfigurationService`.
    pub configuration_service: Rc<dyn IConfigurationService>,
    /// `@INotebookExecutionStateService`.
    pub execution_state_service: Rc<dyn INotebookExecutionStateService>,
    /// `@INotebookLoggingService`.
    pub log_service: Rc<dyn INotebookLoggingService>,
    /// A ligação com o view model do editor da célula.
    pub editor_host: Rc<dyn ICodeCellEditorHost>,
    /// `editorPool`.
    pub editor_pool: Rc<dyn ICodeCellEditorPool>,
    /// As cores e medidas dos nós que a célula desenha.
    pub styles: CodeCellStyles,
}

/// Os cinco colaboradores de [`CodeCellLayout`] sobre o delegate, o template e o view model da célula, e o `_debug` da classe.
pub struct CodeCellLayoutHost {
    ui: Ui,
    notebook_editor: Rc<dyn ICodeCellNotebookEditor>,
    cell: CellViewModel,
    code: Rc<CodeCellViewModel>,
    template_data: Rc<CodeCellTemplateData>,
    log_service: Rc<dyn INotebookLoggingService>,
    debug_prefix: String,
}

impl CodeCellLayoutHost {
    /// `debug(output)`: `notebookLogService.debug('CellLayout', `${debugPrefix} ${output}`)`.
    pub fn log_debug(&self, output: &str) {
        self.log_service.debug(LAYOUT_LOG_CATEGORY, &format!("{} {}", self.debug_prefix, output));
    }

    /// `templateData.editorPart.style.top = ...`: `None` é o `''`, o `top` zero do fluxo normal.
    pub fn set_editor_part_top(&self, top: Option<f64>) {
        let node = self.template_data.editor_part;
        let mut tree = self.ui.tree_mut();
        let bounds = tree.bounds(node);
        let y = top.unwrap_or(0.0);
        tree.set_bounds(node, Rect::new(bounds.x0, y, bounds.x1, y + bounds.height()));
    }
}

impl ICodeCellLayoutNotebook for CodeCellLayoutHost {
    fn scroll_top(&self) -> f64 {
        self.notebook_editor.scroll_top()
    }

    fn scroll_bottom(&self) -> f64 {
        self.notebook_editor.scroll_bottom()
    }

    fn layout_line_height(&self) -> f64 {
        self.notebook_editor.get_layout_info().font_info.line_height
    }

    fn layout_height(&self) -> f64 {
        self.notebook_editor.get_layout_info().height
    }

    fn get_absolute_top_of_element(&self) -> f64 {
        self.notebook_editor.get_absolute_top_of_element(&self.cell)
    }

    fn get_absolute_bottom_of_element(&self) -> f64 {
        self.notebook_editor.get_absolute_bottom_of_element(&self.cell)
    }

    fn get_height_of_element(&self) -> f64 {
        self.notebook_editor.get_height_of_element(&self.cell)
    }

    fn editor_top_padding(&self) -> f64 {
        self.notebook_editor.notebook_options().get_layout_configuration().editor_top_padding
    }
}

impl ICodeCellLayoutCell for CodeCellLayoutHost {
    fn is_input_collapsed(&self) -> bool {
        self.code.is_input_collapsed()
    }

    fn layout_info(&self) -> CodeCellLayoutInfo {
        self.code.layout_info()
    }
}

impl ICodeCellLayoutEditorPart for CodeCellLayoutHost {
    fn set_top(&self, top: Option<f64>) {
        self.set_editor_part_top(top);
    }
}

impl ICodeCellLayoutEditor for CodeCellLayoutHost {
    fn get_layout_info(&self) -> CellEditorDimension {
        self.template_data.editor.get_layout_info()
    }

    fn get_content_height(&self) -> f64 {
        self.template_data.editor.get_content_height()
    }

    fn layout(&self, dimension: CellEditorDimension) {
        self.template_data.editor.layout(dimension);
    }

    fn set_scroll_top(&self, scroll_top: f64) {
        self.template_data.editor.set_scroll_top(scroll_top);
    }
}

impl ICodeCellLayoutLogService for CodeCellLayoutHost {
    fn debug(&self, output: &str) {
        self.log_debug(output);
    }
}

/// `CodeCell`.
pub struct CodeCell {
    base: Rc<Disposable>,
    ui: Ui,
    notebook_editor: Rc<dyn ICodeCellNotebookEditor>,
    notebook_options: Rc<NotebookOptions>,
    view_cell: CellViewModel,
    code: Rc<CodeCellViewModel>,
    template_data: Rc<CodeCellTemplateData>,
    services: Rc<CodeCellServices>,

    output_container_renderer: Rc<CellOutputContainer>,
    /// `_inputCollapseElement`.
    input_collapse_element: RefCell<Option<NodeId>>,

    /// `_renderedInputCollapseState`.
    rendered_input_collapse_state: Cell<Option<bool>>,
    /// `_renderedOutputCollapseState`.
    rendered_output_collapse_state: Cell<Option<bool>>,
    is_disposed: Cell<bool>,
    /// `cellParts`: as partes do template mais o editor e as saídas desta célula.
    cell_parts: Rc<CellPartsCollection>,

    collapsed_execution_icon: OnceCell<Rc<CollapsedCodeCellExecutionIcon>>,
    cell_editor_options: Rc<CellEditorOptions>,
    pointer_down_in_editor: Cell<bool>,
    pointer_dragging_in_editor: Cell<bool>,
    cell_layout: CodeCellLayout<'static>,
    layout_host: Rc<CodeCellLayoutHost>,
    /// `_pendingLayout`.
    pending_layout: Rc<MutableDisposable<dyn IDisposable>>,
}

impl CodeCell {
    pub fn new(
        ui: &Ui,
        notebook_editor: Rc<dyn ICodeCellNotebookEditor>,
        view_cell: CellViewModel,
        template_data: Rc<CodeCellTemplateData>,
        services: Rc<CodeCellServices>,
    ) -> Rc<Self> {
        let code = view_cell.as_code().cloned().expect("CodeCell recebe uma célula de código");
        let notebook_options = notebook_editor.notebook_options();

        let cell_index = notebook_editor.get_cell_index(&view_cell);
        let layout_host = Rc::new(CodeCellLayoutHost {
            ui: ui.clone(),
            notebook_editor: notebook_editor.clone(),
            cell: view_cell.clone(),
            code: code.clone(),
            template_data: template_data.clone(),
            log_service: services.log_service.clone(),
            debug_prefix: format!("[Cell {cell_index}]"),
        });

        let base = Disposable::new();
        let pending_layout = MutableDisposable::<dyn IDisposable>::new();

        let cell_editor_options =
            CellEditorOptions::new(notebook_editor.get_base_cell_editor_options(&code.language()), notebook_options.clone(), services.configuration_service.clone());
        base.register(cell_editor_options.clone());
        let output_container_renderer =
            CellOutputContainer::new(notebook_editor.clone(), code.clone(), template_data.output.clone(), OUTPUT_DISPLAY_LIMIT, services.instantiation_service.clone());
        let content_parts: [Rc<dyn CellContentPart>; 2] = [cell_editor_options.clone(), output_container_renderer.clone()];
        let cell_parts = template_data.cell_parts.concat_content_part(&content_parts, notebook_editor.target_window());
        base.register(cell_parts.clone());

        let initial_editor_dimension = CellEditorDimension { height: Self::calculate_init_editor_height(&notebook_options, &code), width: code.layout_info().editor_width };
        let cell_layout = CodeCellLayout::new_shared(
            USE_NEW_APPROACH_FOR_EDITOR_LAYOUT,
            layout_host.clone(),
            layout_host.clone(),
            layout_host.clone(),
            layout_host.clone(),
            layout_host.clone(),
            initial_editor_dimension,
        );
        base.register(pending_layout.clone());

        let this = Rc::new(Self {
            base,
            ui: ui.clone(),
            notebook_editor,
            notebook_options,
            view_cell,
            code,
            template_data,
            services,
            output_container_renderer,
            input_collapse_element: RefCell::new(None),
            rendered_input_collapse_state: Cell::new(None),
            rendered_output_collapse_state: Cell::new(None),
            is_disposed: Cell::new(false),
            cell_parts,
            collapsed_execution_icon: OnceCell::new(),
            cell_editor_options,
            pointer_down_in_editor: Cell::new(false),
            pointer_dragging_in_editor: Cell::new(false),
            cell_layout,
            layout_host,
            pending_layout,
        });

        this.initialize_editor(initial_editor_dimension);
        this.template_data.editor_overflow_widgets.set_visible(!this.code.is_input_collapsed());
        this.rendered_input_collapse_state.set(Some(false)); // editor is always expanded initially

        this.register_notebook_editor_listeners();
        this.register_view_cell_layout_change();
        this.register_cell_editor_event_listeners();
        this.register_mouse_listener();

        let execution_events = any_event(vec![this.code.on_did_start_execution(), this.code.on_did_stop_execution()]);
        this.listen(execution_events, |this, e| this.cell_parts.update_for_execution_state(&this.view_cell, e));

        this.listen(this.code.on_did_change_state(), |this, e| {
            this.cell_parts.update_state(&this.view_cell, e);

            if e.output_is_hovered_changed {
                this.update_for_output_hover();
            }

            if e.output_is_focused_changed {
                this.update_for_output_focus();
            }

            if e.metadata_changed || e.internal_metadata_changed {
                this.update_editor_options();
            }

            if e.input_collapsed_changed || e.output_collapsed_changed {
                this.code.pause_layout();
                let updated = this.update_for_collapse_state();
                this.code.resume_layout();
                if updated {
                    this.relayout_cell();
                }
            }

            if e.focus_mode_changed {
                this.update_editor_for_focus_mode_change(true);
            }
        });

        this.update_editor_options();
        this.update_editor_for_focus_mode_change(false);
        this.update_for_output_hover();
        this.update_for_output_focus();

        this.cell_parts.schedule_render_cell(&this.view_cell);

        {
            let (cell_parts, view_cell) = (this.cell_parts.clone(), this.view_cell.clone());
            this.base.register(to_disposable(move || cell_parts.unrender_cell(&view_cell)));
        }

        // Render Outputs
        this.code.set_editor_height(initial_editor_dimension.height);
        this.output_container_renderer.render();
        this.rendered_output_collapse_state.set(Some(false)); // the output is always rendered initially
        // Need to do this after the intial renderOutput
        this.initial_view_update_expanded();

        this.listen(this.code.on_layout_info_read(), |this, _| this.cell_parts.prepare_layout());

        let execution_item_element = create_el(&mut this.ui.tree_mut(), this.template_data.cell_input_collapsed_container, Role::Generic, "collapsed-execution-icon");
        {
            let ui = this.ui.clone();
            this.base.register(to_disposable(move || ui.tree_mut().remove(execution_item_element)));
        }
        let collapsed_execution_icon = CollapsedCodeCellExecutionIcon::new(
            &this.ui,
            this.view_cell.clone(),
            execution_item_element,
            this.services.styles.collapsed_execution_icon,
            this.services.execution_state_service.clone(),
        );
        this.base.register(collapsed_execution_icon.clone());
        let _ = this.collapsed_execution_icon.set(collapsed_execution_icon);
        this.update_for_collapse_state();

        {
            let me = Rc::downgrade(&this);
            this.base.register(this.code.on_did_change_outputs().run_and_subscribe(
                move |_| {
                    if let Some(this) = me.upgrade() {
                        this.update_for_outputs();
                    }
                },
                None,
            ));
        }
        {
            let me = Rc::downgrade(&this);
            this.base.register(this.code.on_did_change_layout().run_and_subscribe(
                move |_| {
                    if let Some(this) = me.upgrade() {
                        this.update_for_layout();
                    }
                },
                None,
            ));
        }

        this.cell_editor_options.set_line_numbers(this.code.line_numbers());
        this.template_data.editor.update_options(&this.cell_editor_options.get_updated_value(&this.code.internal_metadata(), &this.code.uri()));

        this
    }

    /// `this._register(event(handler))` com o `this` do original: o ouvinte guarda um `Weak` da célula.
    fn listen<T: 'static>(self: &Rc<Self>, event: Event<T>, handler: impl Fn(&Rc<CodeCell>, &T) + 'static) {
        let me: Weak<CodeCell> = Rc::downgrade(self);
        self.base.register(event.listen(move |value| {
            if let Some(this) = me.upgrade() {
                handler(&this, value);
            }
        }));
    }

    /// `this._debug(output)`.
    fn debug(&self, output: &str) {
        self.layout_host.log_debug(output);
    }

    fn toggle_container_class(&self, class_name: &str, force: bool) {
        self.ui.tree_mut().toggle_class(self.template_data.container, class_name, force);
    }

    /// Nenhum nó focado é o `document.activeElement === document.body`.
    fn body_has_focus(&self) -> bool {
        self.ui.focused().is_none()
    }

    /// `raceCancellation(this.viewCell.resolveTextModel(), cts.token)`.
    fn resolve_text_model_until_cancelled(&self, cts: &CancellationTokenSource) -> Promise<Option<Rc<TextModel>>> {
        race_cancellation(&self.services.editor_host.resolve_text_model(&self.view_cell), &cts.token())
    }

    /// `model.updateOptions({ indentSize, tabSize, insertSpaces })`.
    fn apply_model_options(&self, model: &TextModel) {
        model.update_options(self.cell_editor_options.text_model_update_options());
    }

    fn update_code_cell_options(self: &Rc<Self>) {
        self.template_data.editor.update_options(&self.cell_editor_options.get_updated_value(&self.code.internal_metadata(), &self.code.uri()));

        let cts = CancellationTokenSource::new(None);
        {
            let cts = cts.clone();
            self.base.register(to_disposable(move || cts.dispose_with(true)));
        }
        let me = Rc::downgrade(self);
        let _ = self.resolve_text_model_until_cancelled(&cts).then(move |model| {
            let Some(this) = me.upgrade() else { return };
            if this.is_disposed.get() {
                return;
            }

            if let Some(model) = model {
                this.apply_model_options(&model);
            }
        });
    }

    /// `updateForLayout`: `_pendingLayout?.dispose()` e `DOM.modify(window, () => cellParts.updateInternalLayoutNow(viewCell))`.
    fn update_for_layout(&self) {
        let (cell_parts, view_cell) = (self.cell_parts.clone(), self.view_cell.clone());
        let scheduled = self.notebook_editor.target_window().schedule_at_next_animation_frame(move || cell_parts.update_internal_layout_now(&view_cell), MODIFY_PRIORITY);
        self.pending_layout.set(Some(scheduled));
    }

    fn update_for_output_hover(&self) {
        self.toggle_container_class("cell-output-hover", self.code.output_is_hovered());
    }

    fn update_for_output_focus(&self) {
        self.toggle_container_class("cell-output-focus", self.code.output_is_focused());
    }

    fn calculate_init_editor_height(notebook_options: &NotebookOptions, code: &CodeCellViewModel) -> f64 {
        let line_num = code.line_count();
        let line_height = code.layout_info().font_info.as_ref().map(|font_info| font_info.line_height).filter(|height| *height != 0.0).unwrap_or(FALLBACK_LINE_HEIGHT);
        let editor_padding = notebook_options.compute_editor_padding(&code.internal_metadata(), &code.uri());
        if code.layout_info().editor_height == 0.0 {
            line_num as f64 * line_height + editor_padding.top + editor_padding.bottom
        } else {
            code.layout_info().editor_height
        }
    }

    fn initialize_editor(self: &Rc<Self>, dimension: CellEditorDimension) {
        self.debug(&format!("Initialize Editor {} x {}, Scroll Top = {}", dimension.height, dimension.width, self.notebook_editor.scroll_top()));
        self.cell_layout.layout_editor(CellLayoutChangeReason::Init);
        self.layout_editor(dimension);

        let cts = CancellationTokenSource::new(None);
        {
            let cts = cts.clone();
            self.base.register(to_disposable(move || cts.dispose_with(true)));
        }
        let me = Rc::downgrade(self);
        let _ = self.resolve_text_model_until_cancelled(&cts).then(move |model| {
            let Some(this) = me.upgrade() else { return };
            if this.is_disposed.get() || model.as_ref().is_some_and(|model| model.is_disposed()) {
                return;
            }

            if let Some(model) = model {
                this.register_model_listeners(&model);

                // set model can trigger view update, which can lead to dispose of this cell
                this.template_data.editor.set_model(model.clone());

                if this.is_disposed.get() {
                    return;
                }

                this.apply_model_options(&model);
                this.services.editor_host.attach_text_editor(&this.view_cell, this.template_data.editor.clone(), this.code.layout_info().estimated_has_horizontal_scrolling);
                this.focus_editor_if_needed();

                let real_content_height = this.template_data.editor.get_content_height();
                if real_content_height != dimension.height {
                    this.on_cell_editor_height_change(CellLayoutChangeReason::OnDidResolveTextModel);
                }

                if this.is_disposed.get() {
                    return;
                }

                this.focus_editor_if_needed();
            }

            this.listen(this.cell_editor_options.on_did_change(), |this, _| this.update_code_cell_options());
        });
    }

    /// `focusEditorIfNeeded` de `initializeEditor`: mesma condição de `shouldPreserveEditor` (ver as divergências do módulo).
    fn focus_editor_if_needed(&self) {
        if self.should_preserve_editor() {
            self.template_data.editor.focus();
        }
    }

    fn update_for_outputs(&self) {
        set_visibility(&mut self.ui.tree_mut(), !self.code.outputs_view_models().is_empty(), self.template_data.focus_sink_element);
    }

    fn update_editor_options(&self) {
        let editor = &self.template_data.editor;

        let is_readonly = self.notebook_editor.is_read_only();
        let padding = self.notebook_options.compute_editor_padding(&self.code.internal_metadata(), &self.code.uri());
        if editor.read_only_option() != is_readonly || editor.padding_option() != padding {
            editor.update_options(&Dynamic::object([
                ("readOnly", Dynamic::Bool(is_readonly)),
                ("padding", Dynamic::object([("top", Dynamic::Number(padding.top)), ("bottom", Dynamic::Number(padding.bottom))])),
            ]));
        }
    }

    fn register_notebook_editor_listeners(self: &Rc<Self>) {
        self.listen(self.notebook_editor.on_did_scroll(), |this, _| {
            this.adjust_editor_position();
            this.cell_layout.layout_editor(CellLayoutChangeReason::NbDidScroll);
        });

        self.listen(self.notebook_editor.on_did_change_layout(), |this, _| {
            this.adjust_editor_position();
            this.on_cell_width_change(CellLayoutChangeReason::NbLayoutChange);
        });
    }

    fn adjust_editor_position(&self) {
        if USE_NEW_APPROACH_FOR_EDITOR_LAYOUT {
            return;
        }
        let min = 0.0_f64;

        let scroll_top = self.notebook_editor.scroll_top();
        let element_top = self.notebook_editor.get_absolute_top_of_element(&self.view_cell);
        let diff = scroll_top - element_top + EDITOR_TOP_EXTRA_OFFSET;

        let notebook_editor_layout = self.notebook_editor.get_layout_info();

        // we should stop adjusting the top when users are viewing the bottom of the cell editor
        let editor_max_height = notebook_editor_layout.height - notebook_editor_layout.sticky_height - NOTEBOOK_TOOLBAR_HEIGHT;

        let max_top = self.code.layout_info().editor_height - editor_max_height;
        let top = if max_top > 20.0 { min.max(diff).min(max_top) } else { min };
        self.layout_host.set_editor_part_top(Some(top));
        // scroll the editor with top
        self.template_data.editor.set_scroll_top(top);
    }

    fn register_view_cell_layout_change(self: &Rc<Self>) {
        self.listen(self.code.on_did_change_layout(), |this, e| {
            if e.outer_width.is_some() {
                let layout_info = this.template_data.editor.get_layout_info();
                if layout_info.width != this.code.layout_info().editor_width {
                    this.on_cell_width_change(CellLayoutChangeReason::ViewCellLayoutChange);
                    this.adjust_editor_position();
                }
            }
        });
    }

    fn register_cell_editor_event_listeners(self: &Rc<Self>) {
        let editor = self.template_data.editor.clone();

        self.listen(editor.on_did_content_size_change(), |this, e| {
            if e.content_height_changed && this.code.layout_info().editor_height != e.content_height {
                this.on_cell_editor_height_change(CellLayoutChangeReason::OnDidContentSizeChange);
                this.adjust_editor_position();
            }
        });

        if USE_NEW_APPROACH_FOR_EDITOR_LAYOUT {
            self.listen(editor.on_did_scroll_change(), |this, e| {
                // Option 4: Gate scroll-driven reactions during active drag-selection
                if this.pointer_down_in_editor.get() || this.pointer_dragging_in_editor.get() {
                    return;
                }
                let visibility = this.cell_layout.editor_visibility();
                if visibility == Some(CodeCellEditorVisibility::Invisible) || !this.template_data.editor.has_text_focus() {
                    return;
                }
                let last_changed = this.cell_layout.last_changed_editor_scrolltop();
                if last_changed == Some(e.scroll_top) || this.cell_layout.is_updating_layout() {
                    return;
                }
                let scroll_top = this.notebook_editor.scroll_top();
                let diff = e.scroll_top - last_changed.unwrap_or(0.0);
                if visibility == Some(CodeCellEditorVisibility::FullSmallViewport) && last_changed.is_some() {
                    this.debug(&format!("Scroll Change (1) = {} changed by {} (notebook scrollTop: {}, setEditorScrollTop: {})", e.scroll_top, diff, scroll_top, e.scroll_top));
                    // this.templateData.editor.setScrollTop(e.scrollTop);
                } else if visibility == Some(CodeCellEditorVisibility::BottomClipped) && last_changed.is_some() {
                    this.debug(&format!(
                        "Scroll Change (2) = {} changed by {} (notebook scrollTop: {}, setNotebookScrollTop: {})",
                        e.scroll_top,
                        diff,
                        scroll_top,
                        scroll_top + e.scroll_top
                    ));
                    this.notebook_editor.set_scroll_top(scroll_top + e.scroll_top);
                } else if visibility == Some(CodeCellEditorVisibility::TopClipped) && last_changed.is_some() {
                    let new_scroll_top = scroll_top + diff - 1.0;
                    this.debug(&format!("Scroll Change (3) = {} changed by {} (notebook scrollTop: {}, setNotebookScrollTop?: {})", e.scroll_top, diff, scroll_top, new_scroll_top));
                    if scroll_top != new_scroll_top {
                        this.notebook_editor.set_scroll_top(new_scroll_top);
                    }
                } else {
                    this.debug(&format!("Scroll Change (4) = {} changed by {} (notebook scrollTop: {})", e.scroll_top, diff, scroll_top));
                    this.cell_layout.set_last_changed_editor_scrolltop(None);
                }
            });
        }

        self.listen(editor.on_did_change_cursor_selection(), |this, e| {
            // do not reveal the cell into view if this selection change was caused by restoring editors
            // nor if the text editor is not actually focused (e.g. inline chat is focused and modifying the cell content)
            if e.source == "restoreState" || e.old_model_version_id == 0 || !this.template_data.editor.has_text_focus() {
                return;
            }

            // Option 3: Avoid relayouts during active pointer drag to prevent stuck selection mode
            if (this.pointer_down_in_editor.get() || this.pointer_dragging_in_editor.get()) && USE_NEW_APPROACH_FOR_EDITOR_LAYOUT {
                return;
            }

            let selections = this.template_data.editor.get_selections();

            if let Some(selections) = selections.filter(|selections| !selections.is_empty()) {
                let content_height = this.template_data.editor.get_content_height();
                let layout_content_height = this.code.layout_info().editor_height;

                if content_height != layout_content_height {
                    if !USE_NEW_APPROACH_FOR_EDITOR_LAYOUT {
                        this.debug("onDidChangeCursorSelection");
                        this.on_cell_editor_height_change(CellLayoutChangeReason::OnDidChangeCursorSelection);
                    }

                    if this.is_disposed.get() {
                        return;
                    }
                }
                let last_selection = selections[selections.len() - 1];
                this.notebook_editor.reveal_range_in_view_async(&this.view_cell, &last_selection);
            }
        });

        self.listen(editor.on_did_blur_editor_widget(), |this, _| this.template_data.editor.hide_light_bulb_widget());
    }

    /// `_reigsterModelListeners`.
    fn register_model_listeners(self: &Rc<Self>, model: &Rc<TextModel>) {
        let tokens_changed = model.on_did_change_tokens();
        let model = Rc::downgrade(model);
        self.listen(tokens_changed, move |this, _: &ModelTokensChangedEvent| {
            if this.code.is_input_collapsed()
                && this.input_collapse_element.borrow().is_some()
                && let Some(model) = model.upgrade()
            {
                // flush the collapsed input with the latest tokens
                this.flush_collapsed_input_preview(&model);
            }
        });
    }

    /// `resetPointerState`.
    fn reset_pointer_state(&self) {
        self.pointer_down_in_editor.set(false);
        self.pointer_dragging_in_editor.set(false);
        self.cell_layout.set_pointer_down(false);
    }

    fn register_mouse_listener(self: &Rc<Self>) {
        // Pointer-state handling in notebook cell editors has a couple of easy-to-regress edge cases:
        // 1) Holding the left mouse button while wheel/trackpad scrolling should scroll as usual.
        //    We therefore only treat the interaction as an "active drag selection" after actual pointer movement.
        // 2) "Stuck selection mode" can occur if we miss the corresponding mouseup (e.g. releasing outside the window,
        //    focus loss, or ESC cancelling Monaco selection/drag). When this happens, leaving any of our drag/pointer
        //    flags set will incorrectly gate scroll/layout syncing and make the editor feel stuck.
        //    To avoid that, we reset state on multiple cancellation paths and also self-heal on mousemove.
        let editor = self.template_data.editor.clone();

        self.listen(editor.on_mouse_down(), |this, e| {
            // prevent default on right mouse click, otherwise it will trigger unexpected focus changes
            // the catch is, it means we don't allow customization of right button mouse down handlers other than the built in ones.
            if e.event.right_button {
                this.template_data.editor.prevent_mouse_event_default(e);
            }

            if USE_NEW_APPROACH_FOR_EDITOR_LAYOUT {
                // Track pointer-down and pointer-drag separately.
                // Holding the left button while wheel/trackpad scrolling should behave like normal scrolling.
                if e.event.left_button {
                    this.pointer_down_in_editor.set(true);
                    this.pointer_dragging_in_editor.set(false);
                    this.cell_layout.set_pointer_down(false);
                }
            }
        });

        if USE_NEW_APPROACH_FOR_EDITOR_LAYOUT {
            self.listen(editor.on_mouse_move(), |this, e| {
                if !this.pointer_down_in_editor.get() {
                    return;
                }

                // Self-heal: if we missed a mouseup (e.g. focus loss), clear the drag state as soon as we can observe it.
                if !e.event.left_button {
                    this.reset_pointer_state();
                    return;
                }

                if !this.pointer_dragging_in_editor.get() {
                    // Only consider it a drag-selection once the pointer actually moves with the left button down.
                    this.pointer_dragging_in_editor.set(true);
                    this.cell_layout.set_pointer_down(true);
                }
            });

            // Ensure we reset pointer-down even if mouseup lands outside the editor
            let root = self.ui.root();
            for (kind, capture) in [(EventKind::PointerUp, false), (EventKind::Blur, true)] {
                let me = Rc::downgrade(self);
                self.base.register(self.ui.listen(root, kind, capture, move |_: &mut UiEvent| {
                    if let Some(this) = me.upgrade() {
                        this.reset_pointer_state();
                    }
                }));
            }
            let me = Rc::downgrade(self);
            self.base.register(self.ui.listen(root, EventKind::KeyDown, false, move |e: &mut UiEvent| {
                let Some(this) = me.upgrade() else { return };
                if e.key.is_some_and(|key| key.key_code == KeyCode::Escape) && (this.pointer_down_in_editor.get() || this.pointer_dragging_in_editor.get()) {
                    this.reset_pointer_state();
                }
            }));
        }
    }

    /// `shouldPreserveEditor`: the DOM focus needs to be adjusted when a cell editor should be focused and the document active element is
    /// inside the notebook editor or the document body (cell editor being disposed previously).
    fn should_preserve_editor(&self) -> bool {
        self.notebook_editor.get_active_cell().is_some_and(|active| active.ptr_eq(&self.view_cell))
            && self.code.focus_mode() == CellFocusMode::Editor
            && (self.notebook_editor.has_editor_focus() || self.body_has_focus())
    }

    fn update_editor_for_focus_mode_change(&self, sync: bool) {
        if self.should_preserve_editor() {
            if sync {
                self.template_data.editor.focus();
            } else {
                let editor = self.template_data.editor.clone();
                self.base.register(self.notebook_editor.target_window().run_at_this_or_schedule_at_next_animation_frame(move || editor.focus(), 0));
            }
        }

        self.toggle_container_class("cell-editor-focus", self.code.focus_mode() == CellFocusMode::Editor);
        self.toggle_container_class("cell-output-focus", self.code.focus_mode() == CellFocusMode::Output);
    }
}

/// Acrescenta o texto ao HTML em UTF-16.
fn push_html(out: &mut JsString, text: &str) {
    out.push_units(&text.encode_utf16().collect::<Vec<u16>>());
}

impl CodeCell {
    fn update_for_collapse_state(&self) -> bool {
        if Some(self.code.is_output_collapsed()) == self.rendered_output_collapse_state.get() && Some(self.code.is_input_collapsed()) == self.rendered_input_collapse_state.get() {
            return false;
        }

        self.code.layout_change(CellLayoutChangeEvent { editor_height: true, ..Default::default() });

        if self.code.is_input_collapsed() {
            self.collapse_input();
        } else {
            self.show_input();
        }

        if self.code.is_output_collapsed() {
            self.collapse_output();
        } else {
            self.show_output(false);
        }

        self.relayout_cell();

        self.rendered_output_collapse_state.set(Some(self.code.is_output_collapsed()));
        self.rendered_input_collapse_state.set(Some(self.code.is_input_collapsed()));

        true
    }

    /// `this._collapsedExecutionIcon.setVisibility(visible)`.
    fn set_collapsed_execution_icon_visibility(&self, visible: bool) {
        if let Some(icon) = self.collapsed_execution_icon.get() {
            icon.set_visibility(visible);
        }
    }

    /// `this.keybindingService.lookupKeybinding(EXPAND_CELL_INPUT_COMMAND_ID)?.getLabel()`.
    fn expand_input_keybinding_label(&self) -> Option<String> {
        self.services.keybinding_service.lookup_keybinding(EXPAND_CELL_INPUT_COMMAND_ID, None, false).and_then(|keybinding| keybinding.get_label())
    }

    /// A prévia fica depois dos ícones que o contêiner recolhido tem antes dela (o ícone de execução, quando está visível).
    fn layout_collapse_preview(&self, tree: &mut Tree, nodes: &CollapsedPreviewNodes) {
        let container = self.template_data.cell_input_collapsed_container;
        let left = tree
            .children_with_class(container, "collapsed-execution-icon")
            .into_iter()
            .filter(|icon| tree.style(*icon).visible)
            .map(|icon| tree.bounds(icon).x1)
            .fold(0.0, f64::max);
        layout_collapsed_preview(tree, container, nodes, left, self.code.layout_info().editor_width, &self.services.styles.collapsed_preview);
    }

    fn collapse_input(&self) {
        // hide the editor and execution label, keep the run button
        hide(&mut self.ui.tree_mut(), self.template_data.editor_part);
        self.template_data.editor_overflow_widgets.set_visible(false);
        self.toggle_container_class("input-collapsed", true);

        // remove input preview
        self.remove_input_collapse_preview();

        self.set_collapsed_execution_icon_visibility(true);

        // update preview
        let editor = &self.template_data.editor;
        let rich_editor_text = match editor.get_model() {
            Some(model) if editor.has_model() => Self::rich_text_from_line_tokens(&model),
            _ => self.rich_text(),
        };
        let spans = collapsed_preview_spans_from_html(&rich_editor_text, &self.services.styles.collapsed_preview);
        let keybinding_label = self.expand_input_keybinding_label();

        let container = self.template_data.cell_input_collapsed_container;
        let mut tree = self.ui.tree_mut();
        let element = create_el(&mut tree, container, Role::Generic, "cell-collapse-preview");
        *self.input_collapse_element.borrow_mut() = Some(element);
        let nodes = fill_collapsed_preview(&mut tree, element, spans, &self.services.styles.collapsed_preview, keybinding_label.as_deref());
        self.layout_collapse_preview(&mut tree, &nodes);

        show(&mut tree, container);
    }

    /// O corpo do ouvinte de `onDidChangeTokens`: reescreve a prévia recolhida com os tokens mais recentes (`innerHTML = richText` e
    /// `_attachInputExpandButton`).
    fn flush_collapsed_input_preview(&self, model: &TextModel) {
        let Some(element) = *self.input_collapse_element.borrow() else { return };

        let content = Self::rich_text_from_line_tokens(model);
        let spans = collapsed_preview_spans_from_html(&content, &self.services.styles.collapsed_preview);
        let keybinding_label = self.expand_input_keybinding_label();

        let mut tree = self.ui.tree_mut();
        if !tree.is_alive(element) {
            return;
        }
        clear_node(&mut tree, element);
        let nodes = fill_collapsed_preview(&mut tree, element, spans, &self.services.styles.collapsed_preview, keybinding_label.as_deref());
        self.layout_collapse_preview(&mut tree, &nodes);
    }

    fn show_input(&self) {
        self.set_collapsed_execution_icon_visibility(false);
        show(&mut self.ui.tree_mut(), self.template_data.editor_part);
        self.template_data.editor_overflow_widgets.set_visible(true);
        hide(&mut self.ui.tree_mut(), self.template_data.cell_input_collapsed_container);
    }

    /// `_getRichText(this.viewCell.textBuffer, this.viewCell.language)`: o HTML da primeira linha do buffer.
    fn rich_text(&self) -> JsString {
        let line = self.code.text_buffer().borrow().get_line_content(1);
        tokenize_line_to_html(&line, &*self.services.language_service, &self.code.language())
    }

    /// `_getRichTextFromLineTokens(model)`: o HTML da primeira linha com as classes dos tokens que o modelo já calculou.
    fn rich_text_from_line_tokens(model: &TextModel) -> JsString {
        let mut result = JsString::new();
        push_html(&mut result, "<div class=\"monaco-tokenized-source\">");

        let first_line_tokens = model.tokenization().get_line_tokens(1);
        let view_line_tokens = first_line_tokens.inflate();
        let line = view_line_tokens.get_line_content();
        let mut start_offset = 0usize;
        for j in 0..view_line_tokens.get_count() {
            let class_name = view_line_tokens.get_class_name(j);
            let end_index = view_line_tokens.get_end_offset(j).max(0) as usize;
            let end = end_index.min(line.len());
            let start = start_offset.min(end);
            push_html(&mut result, &format!("<span class=\"{class_name}\">"));
            result.push_units(escape(line.slice(start, end).units()).units());
            push_html(&mut result, "</span>");
            start_offset = end_index;
        }

        push_html(&mut result, "</div>");
        result
    }

    fn remove_input_collapse_preview(&self) {
        let container = self.template_data.cell_input_collapsed_container;
        let mut tree = self.ui.tree_mut();
        if tree.is_alive(container) {
            for element in tree.children_with_class(container, "cell-collapse-preview") {
                tree.remove(element);
            }
        }
        *self.input_collapse_element.borrow_mut() = None;
    }

    /// `_updateOutputInnerContainer(hide)`.
    fn update_output_inner_container(&self, hide: bool) {
        for child in self.template_data.output.output_container.children() {
            if child.class_name().split_whitespace().any(|class_name| class_name == "output-inner-container") {
                if hide {
                    child.hide();
                } else {
                    child.show();
                }
            }
        }
    }

    fn collapse_output(&self) {
        self.toggle_container_class("output-collapsed", true);
        show(&mut self.ui.tree_mut(), self.template_data.cell_output_collapsed_container);
        self.update_output_inner_container(true);
        self.output_container_renderer.view_update_hide_ouputs();
    }

    fn show_output(&self, init_rendering: bool) {
        self.toggle_container_class("output-collapsed", false);
        hide(&mut self.ui.tree_mut(), self.template_data.cell_output_collapsed_container);
        self.update_output_inner_container(false);
        self.output_container_renderer.view_update_show_outputs(init_rendering);
    }

    fn initial_view_update_expanded(&self) {
        self.toggle_container_class("input-collapsed", false);
        show(&mut self.ui.tree_mut(), self.template_data.editor_part);
        hide(&mut self.ui.tree_mut(), self.template_data.cell_input_collapsed_container);
        self.toggle_container_class("output-collapsed", false);
        self.show_output(true);
    }

    fn layout_editor(&self, dimension: CellEditorDimension) {
        if USE_NEW_APPROACH_FOR_EDITOR_LAYOUT {
            return;
        }
        let editor_layout = self.notebook_editor.get_layout_info();
        let max_height = (editor_layout.height - editor_layout.sticky_height - NOTEBOOK_TOOLBAR_HEIGHT).min(dimension.height);
        self.debug(&format!(
            "Layout Editor: Width = {}, Height = {} (Requested: {}, Editor Layout Height: {}, Sticky: {})",
            dimension.width, max_height, dimension.height, editor_layout.height, editor_layout.sticky_height
        ));
        self.template_data.editor.layout(CellEditorDimension { width: dimension.width, height: max_height });
    }

    fn on_cell_width_change(&self, reason: CellLayoutChangeReason) {
        let editor = &self.template_data.editor;
        self.debug(&format!("Cell Editor Width Change, {}, Content Height = {}", reason.as_str(), editor.get_content_height()));
        let height = editor.get_content_height();
        let editor_width = self.code.layout_info().editor_width;
        if editor.has_model() {
            self.debug(&format!(
                "**** Updating Cell Editor Height (1), ContentHeight: {height}, CodeCellLayoutInfo.EditorWidth {editor_width}, EditorLayoutInfo {} ****",
                editor.get_layout_info().height
            ));
            self.code.set_editor_height(height);
            self.relayout_cell();
            self.layout_editor(CellEditorDimension { width: self.code.layout_info().editor_width, height });
        } else {
            self.debug(&format!(
                "Cell Editor Width Change without model, return (1), ContentHeight: {height}, CodeCellLayoutInfo.EditorWidth {editor_width}, EditorLayoutInfo {}",
                editor.get_layout_info().height
            ));
        }
        self.cell_layout.layout_editor(reason);
    }

    fn on_cell_editor_height_change(&self, reason: CellLayoutChangeReason) {
        let editor = &self.template_data.editor;
        let height = editor.get_content_height();
        let editor_width = self.code.layout_info().editor_width;
        if !editor.has_model() {
            // O original interpola o objeto inteiro do `getLayoutInfo()`, que vira "[object Object]".
            self.debug(&format!("Cell Editor Height Change without model, return (2), ContentHeight: {height}, CodeCellLayoutInfo.EditorWidth {editor_width}, EditorLayoutInfo [object Object]"));
        }
        self.debug(&format!("Cell Editor Height Change ({}): {height}", reason.as_str()));
        self.debug(&format!(
            "**** Updating Cell Editor Height (2), ContentHeight: {height}, CodeCellLayoutInfo.EditorWidth {editor_width}, EditorLayoutInfo {} ****",
            editor.get_layout_info().height
        ));
        let view_layout = editor.get_layout_info();
        self.code.set_editor_height(height);
        self.relayout_cell();
        self.layout_editor(CellEditorDimension { width: view_layout.width, height });
        self.cell_layout.layout_editor(reason);
    }

    pub fn relayout_cell(&self) {
        self.notebook_editor.layout_notebook_cell(&self.code, self.code.layout_info().total_height);
    }
}

impl IDisposable for CodeCell {
    fn dispose(&self) {
        self.is_disposed.set(true);
        self.template_data.editor_overflow_widgets.set_visible(false);

        // move focus back to the cell list otherwise the focus goes to body
        if self.should_preserve_editor() {
            // now the focus is on the monaco editor for the cell but detached from the rows.
            self.services.editor_pool.preserve_focused_editor(&self.view_cell);
        }

        self.services.editor_host.detach_text_editor(&self.view_cell);
        self.remove_input_collapse_preview();
        self.output_container_renderer.dispose();
        self.pending_layout.dispose();

        self.base.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "CodeCell"
    }
}
