//! Porte de `CodeCellLayout` e `CellLayoutChangeReason` de `src/vs/workbench/contrib/notebook/browser/view/cellParts/codeCell.ts`:
//! a conta que decide o `top` da parte do editor, a altura do editor, a rolagem interna e a classificação de visibilidade
//! (`Full`, `Top Clipped`, `Bottom Clipped`, `Full (Small Viewport)`, `Invisible`) a cada relayout de uma célula de código.
//!
//! Divergências:
//! - O `CodeCellLayout` do original fala com o `CodeCellRenderTemplate` (DOM), o `ICodeEditor` e o
//!   `IActiveNotebookEditorDelegate`. A UI do porte é nativa, então ele fala com os contratos mínimos que a conta lê:
//!   [`ICodeCellLayoutEditorPart`] (`templateData.editorPart.style.top`), [`ICodeCellLayoutEditor`] (`templateData.editor`),
//!   [`ICodeCellLayoutNotebook`] (o delegate do notebook) e [`ICodeCellLayoutCell`] (o `CodeCellViewModel`).
//! - `element.style.top = ''` é `set_top(None)` e `` `${top}px` `` é `set_top(Some(top))`.
//! - `editor.layout(dim, true)` é `layout(dim)` (o `true` é `postponeRendering`, que a camada lógica não tem).
//! - `_logService.debug` é [`ICodeCellLayoutLogService`].
//! - `DOM.IDimension` é [`CellEditorDimension`].
//! - O `try/finally` do original (`_isUpdatingLayout` e `_initialized` ao fim) é a sequência das atribuições finais: a camada
//!   lógica não propaga exceção do editor.
//! - O estado da conta (`_initialized`, `_pointerDown`, `_isUpdatingLayout`...) é de células (`Cell`) e os métodos recebem `&self`: o
//!   `layout(...)` do editor dispara, de forma síncrona, ouvintes da `CodeCell` que releem `isUpdatingLayout` e chamam
//!   `layoutEditor` de novo, como no original, onde o objeto é compartilhado. Os campos públicos `_previousScrollBottom` e
//!   `_lastChangedEditorScrolltop` são os acessores [`CodeCellLayout::previous_scroll_bottom`], [`CodeCellLayout::last_changed_editor_scrolltop`]
//!   e [`CodeCellLayout::set_last_changed_editor_scrolltop`].
//! - Os colaboradores são [`Collaborator`]: emprestados (a conta isolada, como nos casos de `codeCell.test.ts`) ou compartilhados
//!   (`Rc`, a `CodeCell` que guarda a conta junto de si).

use std::cell::Cell;
use std::ops::Deref;
use std::rc::Rc;

use crate::contrib::notebook::browser::view_model::cell_view_model_types::CodeCellLayoutInfo;

/// `CellLayoutChangeReason`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellLayoutChangeReason {
    NbLayoutChange,
    NbDidScroll,
    ViewCellLayoutChange,
    Init,
    OnDidChangeCursorSelection,
    OnDidContentSizeChange,
    OnDidResolveTextModel,
}

impl CellLayoutChangeReason {
    /// O literal do `type CellLayoutChangeReason` do original.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NbLayoutChange => "nbLayoutChange",
            Self::NbDidScroll => "nbDidScroll",
            Self::ViewCellLayoutChange => "viewCellLayoutChange",
            Self::Init => "init",
            Self::OnDidChangeCursorSelection => "onDidChangeCursorSelection",
            Self::OnDidContentSizeChange => "onDidContentSizeChange",
            Self::OnDidResolveTextModel => "onDidResolveTextModel",
        }
    }
}

/// `CodeCellLayout.editorVisibility`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CodeCellEditorVisibility {
    Full,
    TopClipped,
    BottomClipped,
    FullSmallViewport,
    Invisible,
}

impl CodeCellEditorVisibility {
    /// O literal do original (`'Full (Small Viewport)'`...).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Full => "Full",
            Self::TopClipped => "Top Clipped",
            Self::BottomClipped => "Bottom Clipped",
            Self::FullSmallViewport => "Full (Small Viewport)",
            Self::Invisible => "Invisible",
        }
    }
}

/// `IDimension`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CellEditorDimension {
    pub width: f64,
    pub height: f64,
}

/// `templateData.editorPart` (só o `style.top`).
pub trait ICodeCellLayoutEditorPart {
    /// `None` é o `style.top = ''`; `Some(n)` é `` `${n}px` ``.
    fn set_top(&self, top: Option<f64>);
}

/// `templateData.editor`: o que a conta lê e escreve no editor da célula.
pub trait ICodeCellLayoutEditor {
    fn get_layout_info(&self) -> CellEditorDimension;
    fn get_content_height(&self) -> f64;
    fn layout(&self, dimension: CellEditorDimension);
    fn set_scroll_top(&self, scroll_top: f64);
}

/// O que a conta lê do `IActiveNotebookEditorDelegate`.
pub trait ICodeCellLayoutNotebook {
    fn scroll_top(&self) -> f64;
    fn scroll_bottom(&self) -> f64;
    /// `getLayoutInfo().fontInfo.lineHeight`.
    fn layout_line_height(&self) -> f64;
    /// `getLayoutInfo().height`.
    fn layout_height(&self) -> f64;
    fn get_absolute_top_of_element(&self) -> f64;
    fn get_absolute_bottom_of_element(&self) -> f64;
    fn get_height_of_element(&self) -> f64;
    /// `notebookOptions.getLayoutConfiguration().editorTopPadding`.
    fn editor_top_padding(&self) -> f64;
}

/// O que a conta lê do `CodeCellViewModel`.
pub trait ICodeCellLayoutCell {
    fn is_input_collapsed(&self) -> bool;
    fn layout_info(&self) -> CodeCellLayoutInfo;
}

/// `_logService`.
pub trait ICodeCellLayoutLogService {
    fn debug(&self, output: &str);
}

fn clamp(value: f64, min: f64, max: f64) -> f64 {
    value.max(min).min(max)
}

/// Um colaborador da conta: emprestado ou compartilhado (ver as divergências do módulo).
pub enum Collaborator<'a, T: ?Sized + 'a> {
    Borrowed(&'a T),
    Shared(Rc<T>),
}

impl<'a, T: ?Sized + 'a> Deref for Collaborator<'a, T> {
    type Target = T;

    fn deref(&self) -> &T {
        match self {
            Self::Borrowed(value) => value,
            Self::Shared(value) => value,
        }
    }
}

/// `CodeCellLayout`.
pub struct CodeCellLayout<'a> {
    enabled: bool,
    notebook_editor: Collaborator<'a, dyn ICodeCellLayoutNotebook + 'a>,
    view_cell: Collaborator<'a, dyn ICodeCellLayoutCell + 'a>,
    editor_part: Collaborator<'a, dyn ICodeCellLayoutEditorPart + 'a>,
    editor: Collaborator<'a, dyn ICodeCellLayoutEditor + 'a>,
    log_service: Collaborator<'a, dyn ICodeCellLayoutLogService + 'a>,
    initial_editor_dimension: CellEditorDimension,
    editor_visibility: Cell<Option<CodeCellEditorVisibility>>,
    is_updating_layout: Cell<bool>,
    previous_scroll_bottom: Cell<Option<f64>>,
    last_changed_editor_scrolltop: Cell<Option<f64>>,
    initialized: Cell<bool>,
    pointer_down: Cell<bool>,
    established_content_height: Cell<Option<f64>>,
}

impl<'a> CodeCellLayout<'a> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        enabled: bool,
        notebook_editor: &'a dyn ICodeCellLayoutNotebook,
        view_cell: &'a dyn ICodeCellLayoutCell,
        editor_part: &'a dyn ICodeCellLayoutEditorPart,
        editor: &'a dyn ICodeCellLayoutEditor,
        log_service: &'a dyn ICodeCellLayoutLogService,
        initial_editor_dimension: CellEditorDimension,
    ) -> Self {
        Self::with_collaborators(
            enabled,
            Collaborator::Borrowed(notebook_editor),
            Collaborator::Borrowed(view_cell),
            Collaborator::Borrowed(editor_part),
            Collaborator::Borrowed(editor),
            Collaborator::Borrowed(log_service),
            initial_editor_dimension,
        )
    }

    /// O mesmo de [`CodeCellLayout::new`] com colaboradores compartilhados, para a conta que a `CodeCell` guarda junto de si.
    #[allow(clippy::too_many_arguments)]
    pub fn new_shared(
        enabled: bool,
        notebook_editor: Rc<dyn ICodeCellLayoutNotebook>,
        view_cell: Rc<dyn ICodeCellLayoutCell>,
        editor_part: Rc<dyn ICodeCellLayoutEditorPart>,
        editor: Rc<dyn ICodeCellLayoutEditor>,
        log_service: Rc<dyn ICodeCellLayoutLogService>,
        initial_editor_dimension: CellEditorDimension,
    ) -> CodeCellLayout<'static> {
        CodeCellLayout::with_collaborators(
            enabled,
            Collaborator::Shared(notebook_editor),
            Collaborator::Shared(view_cell),
            Collaborator::Shared(editor_part),
            Collaborator::Shared(editor),
            Collaborator::Shared(log_service),
            initial_editor_dimension,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn with_collaborators(
        enabled: bool,
        notebook_editor: Collaborator<'a, dyn ICodeCellLayoutNotebook + 'a>,
        view_cell: Collaborator<'a, dyn ICodeCellLayoutCell + 'a>,
        editor_part: Collaborator<'a, dyn ICodeCellLayoutEditorPart + 'a>,
        editor: Collaborator<'a, dyn ICodeCellLayoutEditor + 'a>,
        log_service: Collaborator<'a, dyn ICodeCellLayoutLogService + 'a>,
        initial_editor_dimension: CellEditorDimension,
    ) -> Self {
        Self {
            enabled,
            notebook_editor,
            view_cell,
            editor_part,
            editor,
            log_service,
            initial_editor_dimension,
            editor_visibility: Cell::new(None),
            is_updating_layout: Cell::new(false),
            previous_scroll_bottom: Cell::new(None),
            last_changed_editor_scrolltop: Cell::new(None),
            initialized: Cell::new(false),
            pointer_down: Cell::new(false),
            established_content_height: Cell::new(None),
        }
    }

    /// `editorVisibility`.
    pub fn editor_visibility(&self) -> Option<CodeCellEditorVisibility> {
        self.editor_visibility.get()
    }

    /// `isUpdatingLayout`.
    pub fn is_updating_layout(&self) -> bool {
        self.is_updating_layout.get()
    }

    /// `_previousScrollBottom`.
    pub fn previous_scroll_bottom(&self) -> Option<f64> {
        self.previous_scroll_bottom.get()
    }

    /// `_lastChangedEditorScrolltop`.
    pub fn last_changed_editor_scrolltop(&self) -> Option<f64> {
        self.last_changed_editor_scrolltop.get()
    }

    /// `_lastChangedEditorScrolltop = value`.
    pub fn set_last_changed_editor_scrolltop(&self, value: Option<f64>) {
        self.last_changed_editor_scrolltop.set(value);
    }

    /// `setPointerDown`.
    pub fn set_pointer_down(&self, is_down: bool) {
        self.pointer_down.set(is_down);
    }

    /// `layoutEditor`: posiciona a parte do editor e dimensiona o editor para o viewport (ver o comentário longo do original).
    pub fn layout_editor(&self, reason: CellLayoutChangeReason) {
        if !self.enabled {
            return;
        }
        if self.view_cell.is_input_collapsed() {
            self.editor_part.set_top(None);
            return;
        }

        let cell_layout = self.view_cell.layout_info();
        let line_height = self.notebook_editor.layout_line_height();
        let cell_top_margin = cell_layout.top_margin;
        // 1 px extra de borda: a borda da célula não pode aparecer ao rolar para cima.
        let cell_outline_width = cell_layout.outline_width;
        let status_bar_height = cell_layout.status_bar_height;

        let editor = &*self.editor;
        let initialized = self.initialized.get();
        let editor_layout = editor.get_layout_info();
        // Depois da primeira vez, a largura vem do layout da célula (a janela ou o notebook mudou de largura).
        let editor_width = if initialized && matches!(reason, CellLayoutChangeReason::NbLayoutChange | CellLayoutChangeReason::ViewCellLayoutChange) {
            cell_layout.editor_width
        } else {
            editor_layout.width
        };
        let editor_height = cell_layout.editor_height;
        let scroll_top = self.notebook_editor.scroll_top();
        let element_top = self.notebook_editor.get_absolute_top_of_element();
        let element_bottom = self.notebook_editor.get_absolute_bottom_of_element();
        let element_height = self.notebook_editor.get_height_of_element();
        let is_init = !initialized && reason == CellLayoutChangeReason::Init;
        let editor_content_height = if is_init {
            // Editores são reaproveitados: na primeira vez o `getContentHeight()` pode ser o da célula anterior. O init usa
            // só a dimensão inicial da própria célula.
            let height = self.initial_editor_dimension.height;
            self.established_content_height.set(Some(height));
            height
        } else {
            let got_content_height = editor.get_content_height();
            let fallback = if got_content_height == -1.0 { editor.get_layout_info().height.max(self.initial_editor_dimension.height) } else { got_content_height };
            let should_refresh_content_height = !initialized
                || matches!(
                    reason,
                    CellLayoutChangeReason::OnDidContentSizeChange | CellLayoutChangeReason::ViewCellLayoutChange | CellLayoutChangeReason::NbLayoutChange
                );
            if should_refresh_content_height {
                self.established_content_height.set(Some(fallback));
                fallback
            } else {
                // A rolagem reaproveita a altura estabelecida, para um valor transitório menor não encolher a linha.
                self.established_content_height.get().unwrap_or(fallback)
            }
        };
        let editor_bottom = element_top + cell_layout.output_container_offset;
        let scroll_bottom = self.notebook_editor.scroll_bottom();
        // No carregamento, `scrollBottom - scrollTop === 0`.
        let viewport_height = if scroll_bottom - scroll_top == 0.0 { self.notebook_editor.layout_height() } else { scroll_bottom - scroll_top };
        let output_container_offset = cell_layout.output_container_offset;
        let scroll_direction = match self.previous_scroll_bottom.get() {
            Some(previous) if scroll_bottom < previous => "up",
            _ => "down",
        };
        self.previous_scroll_bottom.set(Some(scroll_bottom));

        let mut top = (scroll_top - element_top - cell_top_margin - cell_outline_width).max(0.0);
        let possible_editor_height = editor_height - top;
        if possible_editor_height < line_height {
            top = top - (line_height - possible_editor_height) - cell_outline_width;
        }

        let height;
        let mut editor_scroll_top = 0.0;
        let visibility;
        if scroll_top <= element_top + cell_top_margin {
            let minimum_editor_height = line_height + self.notebook_editor.editor_top_padding();
            if scroll_bottom >= editor_bottom {
                height = clamp(editor_content_height, minimum_editor_height, editor_content_height);
                visibility = CodeCellEditorVisibility::Full;
            } else {
                // A borda de baixo não pode aparecer.
                height = clamp(scroll_bottom - (element_top + cell_top_margin) - status_bar_height, minimum_editor_height, editor_content_height)
                    + 2.0 * cell_outline_width;
                visibility = CodeCellEditorVisibility::BottomClipped;
                editor_scroll_top = 0.0;
            }
        } else if viewport_height <= editor_content_height && scroll_bottom <= editor_bottom {
            let minimum_editor_height = line_height + self.notebook_editor.editor_top_padding();
            height = clamp(viewport_height - status_bar_height, minimum_editor_height, editor_content_height - status_bar_height) + 2.0 * cell_outline_width;
            visibility = CodeCellEditorVisibility::FullSmallViewport;
            editor_scroll_top = top;
        } else {
            let minimum_editor_height = line_height;
            height = clamp(editor_content_height - (scroll_top - (element_top + cell_top_margin)), minimum_editor_height, editor_content_height);
            visibility = if scroll_top > editor_bottom { CodeCellEditorVisibility::Invisible } else { CodeCellEditorVisibility::TopClipped };
            editor_scroll_top = editor_content_height - height;
        }
        self.editor_visibility.set(Some(visibility));

        let log = &*self.log_service;
        log.debug(&format!("{} ({}, {})", reason.as_str(), visibility.as_str(), initialized));
        log.debug(&format!("=> Editor Top = {top}px (editHeight = {editor_height}, editContentHeight: {editor_content_height})"));
        log.debug(&format!("=> eleTop = {element_top}, eleBottom = {element_bottom}, eleHeight = {element_height}"));
        log.debug(&format!("=> scrollTop = {scroll_top}, top = {top}"));
        log.debug(&format!("=> cellTopMargin = {cell_top_margin}, cellBottomMargin = {}, cellOutline = {cell_outline_width}", cell_layout.top_margin));
        log.debug(&format!(
            "=> scrollBottom: {scroll_bottom}, editBottom: {editor_bottom}, viewport: {viewport_height}, scroll: {scroll_direction}, contOffset: {output_container_offset})"
        ));
        log.debug(&format!(
            "=> Editor Height = {height}px, Width: {editor_width}px, Initial Width: {}, EditorScrollTop = {editor_scroll_top}px, StatusbarHeight = {status_bar_height}, lineHeight = {line_height}",
            self.initial_editor_dimension.width
        ));

        self.is_updating_layout.set(true);
        self.editor_part.set_top(Some(top));
        editor.layout(CellEditorDimension { width: if initialized { editor_width } else { self.initial_editor_dimension.width }, height });
        // Opção 3: sem rolagem programática enquanto o usuário arrasta a seleção.
        if !self.pointer_down.get() && editor_scroll_top >= 0.0 {
            self.last_changed_editor_scrolltop.set(Some(editor_scroll_top));
            editor.set_scroll_top(editor_scroll_top);
        }
        self.initialized.set(true);
        self.is_updating_layout.set(false);
        log.debug("Updated Editor Layout");
    }
}
