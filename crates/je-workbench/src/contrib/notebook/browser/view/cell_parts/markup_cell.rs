//! Porte de `src/vs/workbench/contrib/notebook/browser/view/cellParts/markupCell.ts`: `MarkupCell`, que liga uma célula de markdown ao
//! template dela e alterna entre os três modos: entrada recolhida, edição (com o editor de código da célula) e prévia (o markdown
//! renderizado, que o `notebookEditor` desenha em `createMarkupPreview`).
//!
//! Divergências:
//! - **DOM é a árvore do `je-ui`.** Os elementos do template são [`NodeId`]s do [`MarkdownCellRenderTemplate`]; `classList.toggle` é
//!   `toggle_class`, `style.display` é `show` e `hide`, `ariaHidden` e `title` são atributos do nó, `innerText = ''` é `clear_node` e
//!   `DOM.reset(el, renderIcon(icon))` é `clear_node` mais o ícone de `build_parts`. O `style` de `markdownAccessibilityContainer`
//!   (`height: 1px; overflow: hidden; position: absolute; top: 100000px; left: 10000px`) são os `bounds` fora da tela e o `clip` do nó.
//! - **`document.activeElement === body`** (o corpo tem o foco) é nenhum nó focado na [`Ui`]; `activeElement.contains(container)` é
//!   `is_ancestor(focado, container)`. `getDomNode()` deixa de ser necessário por isso.
//! - **Serviços injetados chegam como argumentos** ([`MarkupCellServices`]). O `IInstantiationService.createChild(...)` mais
//!   `createInstance(CodeEditorWidget, ...)` é o [`IMarkupCellEditorFactory`]: ele recebe o serviço de chaves de contexto escopado
//!   (com `inCompositeEditor` ligado), as opções, o nó dos widgets de overflow e as contribuições de `creationOptions`, e devolve o
//!   editor, que carrega o descarte do filho de instanciação. Como o `CodeEditorWidget`, `WordHighlighterContribution` e a ligação
//!   com o view model do editor da célula (`attachTextEditor`, `detachTextEditor` e `resolveTextModel` de `BaseCellViewModel`, que
//!   o view model do porte ainda não tem) não têm contrato único, entram como contratos estreitos: [`IMarkupCellEditor`] (o
//!   `CodeEditorWidget` mais o `WordHighlighterContribution.get(editor)?.stopHighlighting()` e `restoreViewState(true)`, que sem a
//!   contribuição não fazem nada) e [`IMarkupCellEditorHost`].
//! - **`IActiveNotebookEditorDelegate` é o contrato estreito [`IMarkupCellNotebookEditor`]**, só com o que a classe chama. O mapa
//!   `renderedEditors: Map<ICellViewModel, ICodeEditor | undefined>` é o [`IRenderedEditors`], que o `cellRenderer.ts` implementa.
//! - **`EditorContextKeys.inCompositeEditor`** (`editorContextKeys.ts` ainda sem porte em `je-editor`) é [`in_composite_editor`], a
//!   chave com o mesmo nome e a mesma descrição; o `codeCell.ts` a reaproveita daqui.
//! - **Ícones de seção** (`notebookIcons.ts`, sem porte): `registerIcon('notebook-collapsed', Codicon.chevronRight)` e
//!   `registerIcon('notebook-expanded', Codicon.chevronDown)` resolvem para o glifo padrão, usado em [`collapsed_icon`] e
//!   [`expanded_icon`] (uma troca do ícone pelo tema não é honrada).
//! - **Entrada recolhida.** `tokenizeToStringSync` (não portado em `je-editor`) é `tokenize_to_string_with_support` sobre o suporte
//!   de tokenização já resolvido, e o HTML dele (`<div class="monaco-tokenized-source">` com um `<span class="mtkN ...">` por token)
//!   vira os trechos de um [`Paragraph`] com a cor do `colorMap` do registro e o peso e o itálico de `mtkb` e `mtki`; o sublinhado
//!   (`mtku`) e o tachado (`mtks`) não são desenhados (o parágrafo do `je-ui` não tem decoração de texto). O `white-space: normal`
//!   do CSS é aplicado ao montar os trechos. O `display: flex; align-items: center` de `.input-collapse-container` e de
//!   `.cell-collapse-preview` é a posição calculada em `layout_collapsed_children` (ícone do markdown, prévia com `padding: 0 8px` e
//!   o botão `expandInputIcon` de 16px com `padding: 2px`). O `collapsedCellTTPolicy` (Trusted Types) não existe.
//! - **Prévia para leitor de tela.** `safeSetInnerHtml(container, renderedHtml)` é `sanitize_html` mais
//!   [`MarkdownView::from_nodes`], o mesmo parágrafo com trechos e quebra do markdown das configurações, anexado ao contêiner de
//!   acessibilidade. O `DOM.clearNode` do ramo sem leitor de tela repete o `innerText = ''` logo acima e não é repetido.
//! - **`IEditorOptions`** é um [`Dynamic`] (o mesmo modelo de `cell_editor_options.rs`), e `{ ...editorOptions, overflowWidgetsDomNode,
//!   dimension, allowVariableLineHeights }` é [`merge_options`] mais o campo `overflow_widgets_dom_node` do pedido.
//! - **Partes são `Rc`.** Os ouvintes de evento guardam um `Weak` da célula (o `this` do original), então o construtor devolve
//!   `Rc<Self>`. `_register(x)` é o `register` do [`Disposable`] da célula, e o `dispose()` sobrescrito é o `IDisposable` dela.
//! - `raceCancellation(...).then(...)` mais `new CancellationTokenSource()` e `cts.dispose(true)` são `race_cancellation`,
//!   `CancellationTokenSource::dispose_with(true)` e `Promise::then`. `model.updateOptions({ indentSize, tabSize, insertSpaces })` é
//!   [`CellEditorOptions::text_model_update_options`].
//! - O `layoutInfo.fontInfo?.lineHeight || 17` trata `0` como ausente, como o `||`.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::JsString;
use je_base::async_::{disposable_timeout, race_cancellation};
use je_base::browser::dom_sanitize::{DomSanitizerConfig, sanitize_html};
use je_base::browser::html_parser::{Node, parse_body};
use je_base::cancellation::CancellationTokenSource;
use je_base::dynamic::Dynamic;
use je_base::event::Event;
use je_base::lifecycle::{Disposable, DisposableStore, IDisposable, MutableDisposable, to_disposable};
use je_base::nls::localize;
use je_base::promise::Promise;
use je_base::themables::ThemeIcon;
use je_editor::code_editor_widget::EditorContributionDescription;
use je_editor::cursor_events::ICursorSelectionChangedEvent;
use je_editor::language_service::ILanguageService;
use je_editor::range::Range;
use je_editor::selection::Selection;
use je_editor::text_model::TextModel;
use je_editor::text_to_html_tokenizer::tokenize_to_string_with_support;
use je_editor::tokenization_registry::tokenization_registry;
use je_editor::view_model_event_dispatcher::ContentSizeChangedEvent;
use je_platform::accessibility::IAccessibilityService;
use je_platform::configuration::service::IConfigurationService;
use je_platform::contextkey::contextkey::{ContextKeyMeta, IContextKeyService, IScopedContextKeyService, RawContextKey};
use je_platform::keybinding::keybinding::IKeybindingService;
use je_render::Rgba;
use je_render::kurbo::Rect;
use je_text::TextStyle;
use je_ui::{Content, Cursor, NodeId, Paragraph, Role, Span, Ui};
use je_widgets::dom::{clear_node, create_el, hide, show};
use je_widgets::icon_labels::{InlineStyle, LabelPart, Rendered, build_parts, render_icon};
use je_widgets::text::measure_text;

use super::cell_editor_options::{CellEditorOptions, ICellEditorBaseOptions, merge_options};
use super::cell_editor_overflow_widgets::{CellEditorOverflowWidgets, ICellEditorOverflowEditor, UiNodeTarget};
use super::code_cell_layout::CellEditorDimension;
use crate::contrib::notebook::browser::notebook_browser::CellEditState;
use crate::contrib::notebook::browser::notebook_options::NotebookOptions;
use crate::contrib::notebook::browser::view::cell_part::CellPartsCollection;
use crate::contrib::notebook::browser::view_model::cell_view_model_types::{CellFocusMode, CellFoldingState, CellLayoutChangeEvent};
use crate::contrib::notebook::browser::view_model::markup_cell_view_model::MarkupCellViewModel;
use crate::contrib::notebook::browser::view_model::notebook_view_model_impl::CellViewModel;
use crate::contrib::notebook::common::notebook_common::ICell;
use crate::contrib::preferences::settings_markdown_view::{MarkdownView, MarkdownViewStyles};

/// `EXPAND_CELL_INPUT_COMMAND_ID` (`notebookBrowser.ts`).
pub const EXPAND_CELL_INPUT_COMMAND_ID: &str = "notebook.cell.expandCellInput";

/// O `left: 10000px` do `markdownAccessibilityContainer`.
const ACCESSIBILITY_CONTAINER_LEFT: f64 = 10000.0;
/// O `top: 100000px` do `markdownAccessibilityContainer`.
const ACCESSIBILITY_CONTAINER_TOP: f64 = 100000.0;
/// O `height: 1px` do `markdownAccessibilityContainer`.
const ACCESSIBILITY_CONTAINER_HEIGHT: f64 = 1.0;
/// A altura de linha que o `||` assume quando a célula ainda não tem `fontInfo`.
const FALLBACK_LINE_HEIGHT: f64 = 17.0;
/// O lado de um ícone (`codicon`) da entrada recolhida e do indicador de dobra.
const ICON_SIZE: f64 = 16.0;
/// O `padding: 0px 8px` de `.cell-collapse-preview`.
const PREVIEW_PADDING_X: f64 = 8.0;
/// O lado do botão `.expandInputIcon`: 16px mais o `padding: 2px` dos dois lados.
const EXPAND_BUTTON_SIZE: f64 = 20.0;
/// O `font-size: 12px` do glifo do `.expandInputIcon:before`.
const EXPAND_GLYPH_SIZE: f32 = 12.0;
/// O atraso do `disposableTimeout` do desfoque do editor.
const BLUR_FOCUS_DELAY_MS: u64 = 300;

thread_local! {
    static IN_COMPOSITE_EDITOR: RawContextKey = RawContextKey::new(
        "inCompositeEditor",
        Dynamic::Undefined,
        ContextKeyMeta::Description(localize("inCompositeEditor", "Whether the editor is part of a larger editor (e.g. notebooks)", &[])),
    );
}

/// `EditorContextKeys.inCompositeEditor`.
pub fn in_composite_editor() -> RawContextKey {
    IN_COMPOSITE_EDITOR.with(Clone::clone)
}

/// `collapsedIcon` (ver as divergências do módulo): `Codicon.chevronRight`.
pub fn collapsed_icon() -> ThemeIcon {
    ThemeIcon::from_id("chevron-right")
}

/// `expandedIcon` (ver as divergências do módulo): `Codicon.chevronDown`.
pub fn expanded_icon() -> ThemeIcon {
    ThemeIcon::from_id("chevron-down")
}

/// O `CodeEditorWidget` da célula, só com o que `MarkupCell` chama (ver as divergências do módulo). O editor também é o
/// [`ICellEditorOverflowEditor`] que `CellEditorOverflowWidgets.bindEditor` recebe, e `dispose` descarta o editor e o filho de
/// instanciação que o criou.
pub trait IMarkupCellEditor: ICellEditorOverflowEditor + IDisposable {
    /// `hasModel()`.
    fn has_model(&self) -> bool;
    /// `getContentHeight()`.
    fn get_content_height(&self) -> f64;
    /// `layout(dimension)`.
    fn layout(&self, dimension: CellEditorDimension);
    /// `getLayoutInfo().width`.
    fn get_layout_width(&self) -> f64;
    /// `updateOptions(newOptions)`.
    fn update_options(&self, options: &Dynamic);
    /// `setModel(model)`.
    fn set_model(&self, model: Rc<TextModel>);
    /// `focus()`.
    fn focus(&self);
    /// `hasWidgetFocus()`.
    fn has_widget_focus(&self) -> bool;
    /// `getSelection()`.
    fn get_selection(&self) -> Option<Selection>;
    /// `getSelections()`.
    fn get_selections(&self) -> Option<Vec<Selection>>;
    /// `onDidContentSizeChange`.
    fn on_did_content_size_change(&self) -> Event<ContentSizeChangedEvent>;
    /// `onDidChangeCursorSelection`.
    fn on_did_change_cursor_selection(&self) -> Event<ICursorSelectionChangedEvent>;
    /// `onDidFocusEditorWidget`.
    fn on_did_focus_editor_widget(&self) -> Event<()>;
    /// `onDidBlurEditorWidget`.
    fn on_did_blur_editor_widget(&self) -> Event<()>;
    /// `WordHighlighterContribution.get(editor)?.stopHighlighting()`: sem a contribuição não faz nada.
    fn stop_word_highlighting(&self);
    /// `WordHighlighterContribution.get(editor)?.restoreViewState(state)`: sem a contribuição não faz nada.
    fn restore_word_highlighter_view_state(&self, state: bool);
}

/// O pedido de criação do editor da célula: os argumentos do `createInstance(CodeEditorWidget, parent, options, widgetOptions)`.
pub struct MarkupCellEditorOptions {
    /// `templateData.editorContainer`.
    pub parent: NodeId,
    /// O serviço de chaves de contexto escopado, que o filho de instanciação (`createChild`) usa no lugar do `IContextKeyService`.
    pub context_key_service: Rc<dyn IScopedContextKeyService>,
    /// `{ ...editorOptions, dimension, allowVariableLineHeights: false }`.
    pub options: Dynamic,
    /// `overflowWidgetsDomNode`.
    pub overflow_widgets_dom_node: NodeId,
    /// `{ contributions: notebookEditor.creationOptions.cellEditorContributions }`.
    pub contributions: Option<Vec<EditorContributionDescription>>,
}

/// O `IInstantiationService` filho que cria o editor da célula.
pub trait IMarkupCellEditorFactory {
    /// `editorInstaService.createInstance(CodeEditorWidget, ...)`.
    fn create_editor(&self, request: MarkupCellEditorOptions) -> Rc<dyn IMarkupCellEditor>;
}

/// O que `MarkupCell` chama do `MarkupCellViewModel` e que o view model do porte ainda não tem (ver as divergências do módulo).
pub trait IMarkupCellEditorHost {
    /// `attachTextEditor(editor)`.
    fn attach_text_editor(&self, cell: &CellViewModel, editor: Rc<dyn IMarkupCellEditor>);
    /// `detachTextEditor()`.
    fn detach_text_editor(&self, cell: &CellViewModel);
    /// `resolveTextModel()`.
    fn resolve_text_model(&self, cell: &CellViewModel) -> Promise<Rc<TextModel>>;
}

/// `renderedEditors: Map<ICellViewModel, ICodeEditor | undefined>`, só com `set` e `delete`.
pub trait IRenderedEditors {
    /// `renderedEditors.set(cell, editor)`.
    fn set(&self, cell: &CellViewModel, editor: Option<Rc<dyn IMarkupCellEditor>>);
    /// `renderedEditors.delete(cell)`.
    fn delete(&self, cell: &CellViewModel);
}

/// O que `MarkupCell` chama do `IActiveNotebookEditorDelegate`.
pub trait IMarkupCellNotebookEditor {
    /// `getBaseCellEditorOptions(language)`.
    fn get_base_cell_editor_options(&self, language: &str) -> Rc<dyn ICellEditorBaseOptions>;
    /// `notebookOptions`.
    fn notebook_options(&self) -> Rc<NotebookOptions>;
    /// `getLayoutInfo().width`.
    fn layout_width(&self) -> f64;
    /// `creationOptions.cellEditorContributions`.
    fn cell_editor_contributions(&self) -> Option<Vec<EditorContributionDescription>>;
    /// `getOverflowContainerDomNode()`.
    fn overflow_container_dom_node(&self) -> NodeId;
    /// `hasEditorFocus()`.
    fn has_editor_focus(&self) -> bool;
    /// `getActiveCell()`.
    fn get_active_cell(&self) -> Option<CellViewModel>;
    /// `focusContainer()`.
    fn focus_container(&self);
    /// `hideMarkupPreviews(cells)`.
    fn hide_markup_previews(&self, cells: &[CellViewModel]);
    /// `unhideMarkupPreviews(cells)`.
    fn unhide_markup_previews(&self, cells: &[CellViewModel]);
    /// `createMarkupPreview(cell)`.
    fn create_markup_preview(&self, cell: &CellViewModel);
    /// `revealRangeInViewAsync(cell, range)`.
    fn reveal_range_in_view_async(&self, cell: &CellViewModel, range: &Range);
    /// `layoutNotebookCell(cell, height)`.
    fn layout_notebook_cell(&self, cell: &CellViewModel, height: f64);
}

/// `MarkdownCellRenderTemplate` (`notebookRenderingCommon.ts`), só com os campos que `MarkupCell` lê e escreve. O
/// `cellRenderer.ts` cria os nós e as partes.
pub struct MarkdownCellRenderTemplate {
    /// `rootContainer`.
    pub root_container: NodeId,
    /// `container` (`.cell-inner-container`).
    pub container: NodeId,
    /// `cellContainer` (`.cell.markdown`), o contêiner só para leitores de tela.
    pub cell_container: NodeId,
    /// `editorPart`.
    pub editor_part: NodeId,
    /// `editorContainer`.
    pub editor_container: NodeId,
    /// `cellInputCollapsedContainer`.
    pub cell_input_collapsed_container: NodeId,
    /// `foldingIndicator`.
    pub folding_indicator: NodeId,
    /// `cellParts`.
    pub cell_parts: Rc<CellPartsCollection>,
    /// `currentEditor`.
    pub current_editor: RefCell<Option<Rc<dyn IMarkupCellEditor>>>,
}

/// As cores, as fontes e as medidas que o CSS dá à entrada recolhida, ao indicador de dobra e ao markdown de leitor de tela,
/// montadas por quem tem o tema.
#[derive(Clone)]
pub struct MarkupCellStyles {
    /// O estilo dos codicons (o ícone do markdown e o indicador de dobra).
    pub icon: InlineStyle,
    /// A cor do glifo `expandInputIcon` (`color: grey`).
    pub expand_icon_color: Rgba,
    /// A cor do texto da prévia recolhida.
    pub collapse_preview_foreground: Rgba,
    /// `--notebook-cell-input-preview-font-family`.
    pub collapse_preview_font_family: String,
    /// `--notebook-cell-input-preview-font-size`.
    pub collapse_preview_font_size: f32,
    /// A altura da linha do texto da prévia recolhida.
    pub collapse_preview_line_height: f64,
    /// O estilo do markdown renderizado para leitor de tela.
    pub markdown: MarkdownViewStyles,
}

/// Os serviços que o `createInstance` do original injetava em `MarkupCell`.
pub struct MarkupCellServices {
    /// `@IAccessibilityService`.
    pub accessibility_service: Rc<dyn IAccessibilityService>,
    /// `@IContextKeyService`.
    pub context_key_service: Rc<dyn IContextKeyService>,
    /// `@IInstantiationService`, no que `MarkupCell` usa dele: criar o editor da célula.
    pub editor_factory: Rc<dyn IMarkupCellEditorFactory>,
    /// `@ILanguageService`.
    pub language_service: Rc<dyn ILanguageService>,
    /// `@IConfigurationService`.
    pub configuration_service: Rc<dyn IConfigurationService>,
    /// `@IKeybindingService`.
    pub keybinding_service: Rc<dyn IKeybindingService>,
    /// A ligação com o view model do editor da célula.
    pub editor_host: Rc<dyn IMarkupCellEditorHost>,
    /// As cores e medidas dos nós que a célula desenha.
    pub styles: MarkupCellStyles,
}

/// `#rrggbb` ou `#rrggbbaa`, o formato das entradas do `colorMap` do registro de tokenização.
fn parse_css_hex(text: &str) -> Option<Rgba> {
    let digits = text.strip_prefix('#')?;
    if !digits.is_ascii() || (digits.len() != 6 && digits.len() != 8) {
        return None;
    }
    let channel = |index: usize| u8::from_str_radix(&digits[index * 2..index * 2 + 2], 16).ok();
    let alpha = if digits.len() == 8 { channel(3)? } else { 0xff };
    Some(Rgba::new(channel(0)?, channel(1)?, channel(2)?, alpha))
}

/// O `white-space: normal`: sequências de espaço viram um, e o espaço no começo do bloco é descartado (`after_space` é verdadeiro
/// quando o texto montado até aqui termina em espaço ou está vazio).
fn collapse_white_space(text: &str, after_space: &mut bool) -> String {
    let mut collapsed = String::with_capacity(text.len());
    for ch in text.chars() {
        if matches!(ch, ' ' | '\t' | '\n' | '\r' | '\u{c}') {
            if !*after_space {
                collapsed.push(' ');
            }
            *after_space = true;
        } else {
            collapsed.push(ch);
            *after_space = false;
        }
    }
    collapsed
}

/// A célula e o editor, ambos fracos: o que os ouvintes do editor guardam no lugar do `this` e do `editor` do original.
#[derive(Clone)]
struct EditorHandles {
    cell: Weak<MarkupCell>,
    editor: Weak<dyn IMarkupCellEditor>,
}

impl EditorHandles {
    /// Os dois, enquanto ambos estão vivos.
    fn upgrade(&self) -> Option<(Rc<MarkupCell>, Rc<dyn IMarkupCellEditor>)> {
        Some((self.cell.upgrade()?, self.editor.upgrade()?))
    }
}

/// `MarkupCell`.
pub struct MarkupCell {
    base: Rc<Disposable>,
    ui: Ui,
    notebook_editor: Rc<dyn IMarkupCellNotebookEditor>,
    notebook_options: Rc<NotebookOptions>,
    view_cell: CellViewModel,
    markup: Rc<MarkupCellViewModel>,
    template_data: Rc<MarkdownCellRenderTemplate>,
    rendered_editors: Rc<dyn IRenderedEditors>,
    services: Rc<MarkupCellServices>,

    editor: RefCell<Option<Rc<dyn IMarkupCellEditor>>>,
    editor_overflow_widgets: RefCell<Option<Rc<CellEditorOverflowWidgets>>>,

    /// `markdownAccessibilityContainer`.
    markdown_accessibility_container: NodeId,
    /// O markdown renderizado que `safeSetInnerHtml` pôs no contêiner de acessibilidade.
    accessibility_view: RefCell<Option<Rc<MarkdownView>>>,
    editor_part: NodeId,

    local_disposables: Rc<DisposableStore>,
    focus_switch_disposable: Rc<MutableDisposable<dyn IDisposable>>,
    editor_disposables: Rc<DisposableStore>,
    folding_state: Cell<CellFoldingState>,
    cell_editor_options: Rc<CellEditorOptions>,
    editor_options: RefCell<Dynamic>,
    is_disposed: Cell<bool>,
}

impl MarkupCell {
    pub fn new(
        ui: &Ui,
        notebook_editor: Rc<dyn IMarkupCellNotebookEditor>,
        view_cell: CellViewModel,
        template_data: Rc<MarkdownCellRenderTemplate>,
        rendered_editors: Rc<dyn IRenderedEditors>,
        services: Rc<MarkupCellServices>,
    ) -> Rc<Self> {
        let markup = view_cell.as_markup().cloned().expect("MarkupCell recebe uma célula de markup");
        let notebook_options = notebook_editor.notebook_options();

        let base = Disposable::new();
        let local_disposables = DisposableStore::new();
        let focus_switch_disposable = MutableDisposable::<dyn IDisposable>::new();
        let editor_disposables = DisposableStore::new();
        base.register(local_disposables.clone());
        base.register(focus_switch_disposable.clone());
        base.register(editor_disposables.clone());

        let editor_part = template_data.editor_part;
        let cell_editor_options =
            CellEditorOptions::new(notebook_editor.get_base_cell_editor_options(&markup.language()), notebook_options.clone(), services.configuration_service.clone());
        base.register(cell_editor_options.clone());
        cell_editor_options.set_line_numbers(markup.line_numbers());
        let editor_options = cell_editor_options.get_value(&markup.internal_metadata(), &markup.uri());
        let folding_state = markup.folding_state(&view_cell);

        let this = Rc::new(Self {
            base,
            ui: ui.clone(),
            notebook_editor,
            notebook_options,
            markdown_accessibility_container: template_data.cell_container,
            view_cell,
            markup,
            template_data,
            rendered_editors,
            services,
            editor: RefCell::new(None),
            editor_overflow_widgets: RefCell::new(None),
            accessibility_view: RefCell::new(None),
            editor_part,
            local_disposables,
            focus_switch_disposable,
            editor_disposables,
            folding_state: Cell::new(folding_state),
            cell_editor_options,
            editor_options: RefCell::new(editor_options),
            is_disposed: Cell::new(false),
        });

        this.construct_dom();

        {
            let (rendered_editors, view_cell) = (this.rendered_editors.clone(), this.view_cell.clone());
            this.base.register(to_disposable(move || rendered_editors.delete(&view_cell)));
        }
        {
            let me = Rc::downgrade(&this);
            this.base.register(to_disposable(move || {
                if let Some(this) = me.upgrade() {
                    this.dispose_accessibility_view();
                }
            }));
        }
        this.register_listeners();

        // update for init state
        this.template_data.cell_parts.schedule_render_cell(&this.view_cell);

        {
            let (cell_parts, view_cell) = (this.template_data.cell_parts.clone(), this.view_cell.clone());
            this.base.register(to_disposable(move || cell_parts.unrender_cell(&view_cell)));
        }

        let screen_reader_changed = this.services.accessibility_service.on_did_change_screen_reader_optimized();
        this.listen(screen_reader_changed, |this, _| this.view_update());

        this.update_for_hover();
        this.update_for_focus_mode_change();
        this.layout_folding_indicator();
        this.update_folding_icon_show_class();

        // the markdown preview's height might already be updated after the renderer calls `element.getHeight()`
        if this.markup.layout_info().total_height > 0.0 {
            this.relayout_cell();
        }

        this.view_update();

        this.layout_cell_parts();
        this.listen(this.markup.on_did_change_layout(), |this, _| this.layout_cell_parts());

        this
    }

    /// `this._register(event(handler))` com o `this` do original: o ouvinte guarda um `Weak` da célula.
    fn listen<T: 'static>(self: &Rc<Self>, event: Event<T>, handler: impl Fn(&Rc<MarkupCell>, &T) + 'static) {
        let me = Rc::downgrade(self);
        self.base.register(event.listen(move |value| {
            if let Some(this) = me.upgrade() {
                handler(&this, value);
            }
        }));
    }

    /// O `Weak` da célula e o do editor, para os ouvintes do editor.
    fn handles(self: &Rc<Self>, editor: &Rc<dyn IMarkupCellEditor>) -> EditorHandles {
        EditorHandles { cell: Rc::downgrade(self), editor: Rc::downgrade(editor) }
    }

    fn current_editor(&self) -> Option<Rc<dyn IMarkupCellEditor>> {
        self.editor.borrow().clone()
    }

    fn set_overflow_widgets_visible(&self, visible: bool) {
        let widgets = self.editor_overflow_widgets.borrow().clone();
        if let Some(widgets) = widgets {
            widgets.set_visible(visible);
        }
    }

    fn toggle_container_class(&self, class_name: &str, force: bool) {
        self.ui.tree_mut().toggle_class(self.template_data.container, class_name, force);
    }

    /// `markdownAccessibilityContainer.ariaHidden = 'true' | 'false'`.
    fn set_aria_hidden(&self, hidden: bool) {
        self.ui.tree_mut().set_attribute(self.markdown_accessibility_container, "aria-hidden", if hidden { "true" } else { "false" });
    }

    /// Nenhum nó focado é o `document.activeElement === document.body`.
    fn body_has_focus(&self) -> bool {
        self.ui.focused().is_none()
    }

    fn layout_cell_parts(&self) {
        self.template_data.cell_parts.update_internal_layout_now(&self.view_cell);
    }

    fn construct_dom(&self) {
        // Create an element that is only used to announce markup cell content to screen readers
        let id = format!("aria-markup-cell-{}", self.markup.id);
        let container = self.markdown_accessibility_container;
        {
            let mut tree = self.ui.tree_mut();
            tree.set_attribute(container, "id", id.clone());
            // Hide the element from non-screen readers
            let width = tree.bounds(container).width();
            tree.set_bounds(
                container,
                Rect::new(ACCESSIBILITY_CONTAINER_LEFT, ACCESSIBILITY_CONTAINER_TOP, ACCESSIBILITY_CONTAINER_LEFT + width, ACCESSIBILITY_CONTAINER_TOP + ACCESSIBILITY_CONTAINER_HEIGHT),
            );
            tree.style_mut(container).clip = true;
            tree.set_attribute(container, "aria-hidden", "false");

            tree.set_attribute(self.template_data.root_container, "aria-describedby", id);
        }
        self.toggle_container_class("webview-backed-markdown-cell", true);
    }

    fn register_listeners(self: &Rc<Self>) {
        self.listen(self.markup.on_did_change_state(), |this, e| this.template_data.cell_parts.update_state(&this.view_cell, e));

        self.listen(self.markup.model.on_did_change_metadata(), |this, _| this.view_update());

        self.listen(self.markup.on_did_change_state(), |this, e| {
            if e.edit_state_changed || e.content_changed {
                this.view_update();
            }

            if e.focus_mode_changed {
                this.update_for_focus_mode_change();
            }

            if e.folding_state_changed {
                let folding_state = this.markup.folding_state(&this.view_cell);

                if folding_state != this.folding_state.get() {
                    this.folding_state.set(folding_state);
                    this.layout_folding_indicator();
                }
            }

            if e.cell_is_hovered_changed {
                this.update_for_hover();
            }

            if e.input_collapsed_changed {
                this.update_collapsed_state();
                this.view_update();
            }

            if e.cell_line_number_changed {
                this.cell_editor_options.set_line_numbers(this.markup.line_numbers());
            }
        });

        self.listen(self.notebook_options.on_did_change_options(), |this, e| {
            if e.show_folding_controls {
                this.update_folding_icon_show_class();
            }
        });

        self.listen(self.markup.on_did_change_layout(), |this, e: &CellLayoutChangeEvent| {
            let layout_width = this.current_editor().map(|editor| editor.get_layout_width());
            if e.outer_width.is_some_and(|width| width != 0.0)
                && this.markup.get_edit_state() == CellEditState::Editing
                && layout_width.is_some_and(|width| width != this.markup.layout_info().editor_width)
            {
                this.on_cell_editor_width_change();
            }
        });

        self.listen(self.cell_editor_options.on_did_change(), |this, _| this.update_markup_cell_options());
    }

    /// `model.updateOptions({ indentSize, tabSize, insertSpaces })`.
    fn apply_model_options(&self, model: &TextModel) {
        model.update_options(self.cell_editor_options.text_model_update_options());
    }

    /// `raceCancellation(this.viewCell.resolveTextModel(), cts.token)`.
    fn resolve_text_model_until_cancelled(&self, cts: &CancellationTokenSource) -> Promise<Option<Rc<TextModel>>> {
        race_cancellation(&self.services.editor_host.resolve_text_model(&self.view_cell), &cts.token())
    }

    fn update_markup_cell_options(self: &Rc<Self>) {
        let (internal_metadata, uri) = (self.markup.internal_metadata(), self.markup.uri());
        self.update_editor_options(self.cell_editor_options.get_updated_value(&internal_metadata, &uri));

        if let Some(editor) = self.current_editor() {
            editor.update_options(&self.cell_editor_options.get_updated_value(&internal_metadata, &uri));

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
    }

    fn update_collapsed_state(&self) {
        let cells = std::slice::from_ref(&self.view_cell);
        if self.markup.is_input_collapsed() {
            self.notebook_editor.hide_markup_previews(cells);
        } else {
            self.notebook_editor.unhide_markup_previews(cells);
        }
    }

    fn update_for_hover(&self) {
        self.toggle_container_class("markdown-cell-hover", self.markup.cell_is_hovered());
    }

    fn update_for_focus_mode_change(&self) {
        let editor_focused = self.markup.focus_mode() == CellFocusMode::Editor;
        if editor_focused {
            self.focus_editor_if_needed();
        }

        self.toggle_container_class("cell-editor-focus", editor_focused);
    }

    fn update_folding_icon_show_class(&self) {
        let show_folding_icon = self.notebook_options.get_display_options().show_folding_controls;
        let mut tree = self.ui.tree_mut();
        let indicator = self.template_data.folding_indicator;
        tree.remove_class(indicator, "mouseover");
        tree.remove_class(indicator, "always");
        tree.add_class(indicator, &show_folding_icon);
    }

    fn view_update(self: &Rc<Self>) {
        if self.markup.is_input_collapsed() {
            self.view_update_collapsed();
        } else if self.markup.get_edit_state() == CellEditState::Editing {
            self.view_update_editing();
        } else {
            self.view_update_preview();
        }
    }

    fn view_update_collapsed(&self) {
        let container = self.template_data.cell_input_collapsed_container;
        {
            let mut tree = self.ui.tree_mut();
            show(&mut tree, container);
            hide(&mut tree, self.editor_part);
        }
        self.set_overflow_widgets_visible(false);

        let preview_spans = self.collapsed_preview_spans();
        let styles = &self.services.styles;
        let keybinding_label = self.services.keybinding_service.lookup_keybinding(EXPAND_CELL_INPUT_COMMAND_ID, None, false).and_then(|keybinding| keybinding.get_label());

        {
            let mut tree = self.ui.tree_mut();
            clear_node(&mut tree, container);

            let rendered = build_parts(&mut tree, container, &[LabelPart::Span(render_icon(&ThemeIcon::from_id("markdown"), false))], &styles.icon);
            let markdown_icon = rendered.iter().find_map(|part| if let Rendered::Icon { node } = part { Some(*node) } else { None });

            let element = create_el(&mut tree, container, Role::Generic, "cell-collapse-preview");
            let source = create_el(&mut tree, element, Role::Generic, "monaco-tokenized-source");
            let mut paragraph = Paragraph::new(preview_spans, styles.collapse_preview_line_height);
            paragraph.wrap = true;
            tree.style_mut(source).content = Content::Paragraph(paragraph);
            tree.style_mut(source).cursor = Some(Cursor::Pointer);

            let expand_icon = create_el(&mut tree, element, Role::Generic, "expandInputIcon");
            tree.style_mut(expand_icon).content = Content::Codicon { id: "more".to_owned(), size: EXPAND_GLYPH_SIZE, color: styles.expand_icon_color };
            if let Some(label) = keybinding_label {
                tree.set_attribute(element, "title", localize("cellExpandInputButtonLabelWithDoubleClick", "Double-click to expand cell input ({0})", &[label.as_str()]));
                tree.set_attribute(expand_icon, "title", localize("cellExpandInputButtonLabel", "Expand Cell Input ({0})", &[label.as_str()]));
            }

            self.layout_collapsed_children(&mut tree, container, markdown_icon, element, source, expand_icon);
        }

        self.set_aria_hidden(true);

        self.toggle_container_class("input-collapsed", true);
        self.markup.set_rendered_markdown_height(0.0);
        self.markup.layout_change(CellLayoutChangeEvent::default());
    }

    /// O `display: flex; align-items: center` de `.input-collapse-container` e de `.cell-collapse-preview`: o ícone do markdown, a
    /// prévia (`padding: 0 8px`, com o texto e o botão de expandir logo depois dele), centrados na altura do contêiner.
    fn layout_collapsed_children(&self, tree: &mut je_ui::Tree, container: NodeId, markdown_icon: Option<NodeId>, element: NodeId, source: NodeId, expand_icon: NodeId) {
        let styles = &self.services.styles;
        let container_bounds = tree.bounds(container);
        let width = if container_bounds.width() > 0.0 { container_bounds.width() } else { self.markup.layout_info().editor_width };
        let height = if container_bounds.height() > 0.0 { container_bounds.height() } else { styles.collapse_preview_line_height.max(EXPAND_BUTTON_SIZE) };

        if let Some(icon) = markdown_icon {
            let y = (height - ICON_SIZE) / 2.0;
            tree.set_bounds(icon, Rect::new(0.0, y, ICON_SIZE, y + ICON_SIZE));
        }

        let (natural_width, line_height) = match &tree.style(source).content {
            Content::Paragraph(paragraph) => (paragraph.spans.iter().map(|span| measure_text(&span.text, &span.style)).sum::<f64>(), paragraph.line_height),
            _ => (0.0, styles.collapse_preview_line_height),
        };
        let available = (width - ICON_SIZE - 2.0 * PREVIEW_PADDING_X - EXPAND_BUTTON_SIZE).max(0.0);
        let text_width = natural_width.min(available);
        let text_height = match &tree.style(source).content {
            Content::Paragraph(paragraph) => paragraph.height(text_width).max(line_height),
            _ => line_height,
        };

        let element_width = PREVIEW_PADDING_X + text_width + EXPAND_BUTTON_SIZE + PREVIEW_PADDING_X;
        tree.set_bounds(element, Rect::new(ICON_SIZE, 0.0, ICON_SIZE + element_width, height));
        let text_y = (height - text_height) / 2.0;
        tree.set_bounds(source, Rect::new(PREVIEW_PADDING_X, text_y, PREVIEW_PADDING_X + text_width, text_y + text_height));
        let button_x = PREVIEW_PADDING_X + text_width;
        let button_y = (height - EXPAND_BUTTON_SIZE) / 2.0;
        tree.set_bounds(expand_icon, Rect::new(button_x, button_y, button_x + EXPAND_BUTTON_SIZE, button_y + EXPAND_BUTTON_SIZE));
    }

    /// `this.getRichText(this.viewCell.textBuffer, this.viewCell.language)`, como os trechos de texto colorido da primeira linha.
    fn collapsed_preview_spans(&self) -> Vec<Span> {
        let styles = &self.services.styles;
        let line = self.markup.text_buffer().borrow().get_line_content(1);
        let codec = self.services.language_service.language_id_codec();
        let support = tokenization_registry().get(&self.markup.language());
        let html = tokenize_to_string_with_support(&line, &codec, support.as_ref());
        let color_map = tokenization_registry().get_color_map().unwrap_or_default();

        let mut spans = Vec::new();
        let mut after_space = true;
        let source = parse_body(html.units());
        for node in &source.children {
            let Node::Element(div) = node else { continue };
            for token in &div.children {
                let Node::Element(token) = token else { continue };
                let text = collapse_white_space(&token.text_content().to_string_lossy(), &mut after_space);
                if text.is_empty() {
                    continue;
                }

                let mut color = styles.collapse_preview_foreground;
                let mut weight = 400;
                let mut italic = false;
                let classes = token.get_attribute("class").map(|classes| classes.to_string_lossy()).unwrap_or_default();
                for class in classes.split_whitespace() {
                    match class {
                        "mtkb" => weight = 700,
                        "mtki" => italic = true,
                        _ => {
                            let mapped = class.strip_prefix("mtk").and_then(|index| index.parse::<usize>().ok()).and_then(|index| color_map.get(index)).and_then(|css| parse_css_hex(css));
                            if let Some(mapped) = mapped {
                                color = mapped;
                            }
                        }
                    }
                }
                let style = TextStyle::new(styles.collapse_preview_font_family.clone(), styles.collapse_preview_font_size, weight).italic(italic);
                spans.push(Span::text(text, style, color));
            }
        }
        // `white-space: normal` descarta o espaço no fim da linha.
        if let Some(last) = spans.last_mut()
            && last.text.ends_with(' ')
        {
            last.text.pop();
            if last.text.is_empty() {
                spans.pop();
            }
        }
        spans
    }

    fn view_update_editing(self: &Rc<Self>) {
        // switch to editing mode
        {
            let mut tree = self.ui.tree_mut();
            show(&mut tree, self.editor_part);
            hide(&mut tree, self.template_data.cell_input_collapsed_container);
        }
        self.set_aria_hidden(true);

        self.notebook_editor.hide_markup_previews(std::slice::from_ref(&self.view_cell));

        self.toggle_container_class("input-collapsed", false);
        self.toggle_container_class("markdown-cell-edit-mode", true);

        let editor_height;
        if let Some(editor) = self.current_editor().filter(|editor| editor.has_model()) {
            editor_height = editor.get_content_height();

            // not first time, we don't need to create editor
            self.services.editor_host.attach_text_editor(&self.view_cell, editor.clone());
            self.focus_editor_if_needed();

            self.bind_editor_listeners(&editor);

            editor.layout(CellEditorDimension { width: self.markup.layout_info().editor_width, height: editor_height });
        } else {
            editor_height = self.create_editor();
        }

        self.set_overflow_widgets_visible(true);
        self.markup.set_editor_height(editor_height);
        self.focus_editor_if_needed();
        self.rendered_editors.set(&self.view_cell, self.current_editor());
    }

    /// O ramo "primeira vez" de `viewUpdateEditing`: cria o editor, o serviço de chaves de contexto escopado e os widgets de overflow.
    /// Devolve o `editorHeight` estimado.
    fn create_editor(self: &Rc<Self>) -> f64 {
        self.editor_disposables.clear();
        let width = self.notebook_options.compute_markdown_cell_editor_width(self.notebook_editor.layout_width());
        let line_num = self.markup.line_count();
        let line_height = self.markup.layout_info().font_info.as_ref().map(|font_info| font_info.line_height).filter(|height| *height != 0.0).unwrap_or(FALLBACK_LINE_HEIGHT);
        let editor_padding = self.notebook_options.compute_editor_padding(&self.markup.internal_metadata(), &self.markup.uri());
        let editor_height = line_num.max(1) as f64 * line_height + editor_padding.top + editor_padding.bottom;

        clear_node(&mut self.ui.tree_mut(), self.template_data.editor_container);

        // create a special context key service that set the inCompositeEditor-contextkey
        let editor_context_key_service = self.services.context_key_service.create_scoped(UiNodeTarget::new(&self.ui, self.editor_part));
        self.editor_disposables.add(editor_context_key_service.clone());
        in_composite_editor().bind_to(&*editor_context_key_service).set(Dynamic::Bool(true));

        let overflow_widgets = CellEditorOverflowWidgets::new(&self.ui, self.notebook_editor.overflow_container_dom_node());
        self.editor_disposables.add(overflow_widgets.clone());
        *self.editor_overflow_widgets.borrow_mut() = Some(overflow_widgets.clone());

        let options = merge_options(
            &self.editor_options.borrow(),
            [
                ("dimension", Dynamic::object([("width", Dynamic::Number(width)), ("height", Dynamic::Number(editor_height))])),
                ("allowVariableLineHeights", Dynamic::Bool(false)),
            ],
        );
        let editor = self.services.editor_factory.create_editor(MarkupCellEditorOptions {
            parent: self.template_data.editor_container,
            context_key_service: editor_context_key_service,
            options,
            overflow_widgets_dom_node: overflow_widgets.dom_node(),
            contributions: self.notebook_editor.cell_editor_contributions(),
        });
        self.editor_disposables.add(editor.clone());
        *self.editor.borrow_mut() = Some(editor.clone());
        overflow_widgets.bind_editor(editor.clone());
        *self.template_data.current_editor.borrow_mut() = Some(editor.clone());

        let weak_editor = Rc::downgrade(&editor);
        self.editor_disposables.add(editor.on_did_blur_editor_widget().listen({
            let weak_editor = weak_editor.clone();
            move |_| {
                if let Some(editor) = weak_editor.upgrade() {
                    editor.stop_word_highlighting();
                }
            }
        }));
        self.editor_disposables.add(editor.on_did_focus_editor_widget().listen(move |_| {
            if let Some(editor) = weak_editor.upgrade() {
                editor.restore_word_highlighter_view_state(true);
            }
        }));

        let cts = CancellationTokenSource::new(None);
        {
            let cts = cts.clone();
            self.editor_disposables.add(to_disposable(move || cts.dispose_with(true)));
        }
        let me = Rc::downgrade(self);
        let _ = self.resolve_text_model_until_cancelled(&cts).then(move |model| {
            let Some(model) = model else { return };
            let Some(this) = me.upgrade() else { return };
            let Some(editor) = this.current_editor() else { return };

            editor.set_model(model.clone());
            this.apply_model_options(&model);

            let mut editor_height = editor_height;
            let real_content_height = editor.get_content_height();
            if real_content_height != editor_height {
                editor.layout(CellEditorDimension { width, height: real_content_height });
                editor_height = real_content_height;
            }

            this.services.editor_host.attach_text_editor(&this.view_cell, editor.clone());

            if this.markup.get_edit_state() == CellEditState::Editing {
                this.focus_editor_if_needed();
            }

            this.bind_editor_listeners(&editor);

            this.markup.set_editor_height(editor_height);
        });

        editor_height
    }

    fn view_update_preview(&self) {
        self.services.editor_host.detach_text_editor(&self.view_cell);
        hide(&mut self.ui.tree_mut(), self.editor_part);
        self.set_overflow_widgets_visible(false);
        hide(&mut self.ui.tree_mut(), self.template_data.cell_input_collapsed_container);
        self.set_aria_hidden(false);
        self.toggle_container_class("input-collapsed", false);
        self.toggle_container_class("markdown-cell-edit-mode", false);

        self.rendered_editors.delete(&self.view_cell);

        self.dispose_accessibility_view();
        clear_node(&mut self.ui.tree_mut(), self.markdown_accessibility_container);
        if let Some(rendered_html) = self.markup.rendered_html().filter(|html| !html.is_empty())
            && self.services.accessibility_service.is_screen_reader_optimized()
        {
            self.safe_set_inner_html(rendered_html);
        }

        self.notebook_editor.create_markup_preview(&self.view_cell);
    }

    /// `domSanitize.safeSetInnerHtml(this.markdownAccessibilityContainer, html)`: o HTML sanitizado como o markdown renderizado
    /// (parágrafos com trechos e quebra) dentro do contêiner de acessibilidade.
    fn safe_set_inner_html(&self, html: String) {
        let nodes = sanitize_html(JsString::from(html).units(), &DomSanitizerConfig::default());
        let view = MarkdownView::from_nodes(&self.ui, &nodes, &self.services.styles.markdown);

        let container = self.markdown_accessibility_container;
        let container_width = self.ui.tree().bounds(container).width();
        let width = if container_width > 0.0 { container_width } else { self.markup.layout_info().editor_width };
        self.ui.tree_mut().append_child(container, view.root());
        view.layout(width);
        *self.accessibility_view.borrow_mut() = Some(view);
    }

    /// Descarta o markdown renderizado do contêiner de acessibilidade, se houver.
    fn dispose_accessibility_view(&self) {
        let view = self.accessibility_view.borrow_mut().take();
        if let Some(view) = view {
            view.dispose();
        }
    }

    fn focus_editor_if_needed(&self) {
        if self.markup.focus_mode() == CellFocusMode::Editor && (self.notebook_editor.has_editor_focus() || self.body_has_focus()) {
            // Don't steal focus from other workbench parts, but if body has focus, we can take it
            let Some(editor) = self.current_editor() else { return };

            editor.focus();

            let Some(primary_selection) = editor.get_selection() else { return };

            self.notebook_editor.reveal_range_in_view_async(&self.view_cell, &primary_selection);
        }
    }

    fn layout_editor(&self, dimension: CellEditorDimension) {
        if let Some(editor) = self.current_editor() {
            editor.layout(dimension);
        }
    }

    fn on_cell_editor_width_change(&self) {
        let Some(editor) = self.current_editor() else { return };
        let real_content_height = editor.get_content_height();
        self.layout_editor(CellEditorDimension { width: self.markup.layout_info().editor_width, height: real_content_height });

        // LET the content size observer to handle it
        // this.viewCell.editorHeight = realContentHeight;
        // this.relayoutCell();
    }

    pub fn relayout_cell(&self) {
        self.notebook_editor.layout_notebook_cell(&self.view_cell, self.markup.layout_info().total_height);
        self.layout_folding_indicator();
    }

    pub fn update_editor_options(&self, new_value: Dynamic) {
        *self.editor_options.borrow_mut() = new_value.clone();
        if let Some(editor) = self.current_editor() {
            editor.update_options(&new_value);
        }
    }

    fn layout_folding_indicator(&self) {
        let indicator = self.template_data.folding_indicator;
        let icon = match self.folding_state.get() {
            CellFoldingState::None => {
                let mut tree = self.ui.tree_mut();
                hide(&mut tree, indicator);
                clear_node(&mut tree, indicator);
                return;
            }
            CellFoldingState::Collapsed => collapsed_icon(),
            CellFoldingState::Expanded => expanded_icon(),
        };

        let mut tree = self.ui.tree_mut();
        show(&mut tree, indicator);
        clear_node(&mut tree, indicator);
        let rendered = build_parts(&mut tree, indicator, &[LabelPart::Span(render_icon(&icon, false))], &self.services.styles.icon);
        if let Some(Rendered::Icon { node }) = rendered.first() {
            tree.set_bounds(*node, Rect::new(0.0, 0.0, ICON_SIZE, ICON_SIZE));
        }
    }

    fn update_focus_mode(&self, editor: &Rc<dyn IMarkupCellEditor>) {
        self.markup.set_focus_mode(if editor.has_widget_focus() { CellFocusMode::Editor } else { CellFocusMode::Container });
    }

    fn bind_editor_listeners(self: &Rc<Self>, editor: &Rc<dyn IMarkupCellEditor>) {
        self.local_disposables.clear();
        self.focus_switch_disposable.clear();

        let handles = self.handles(editor);

        self.local_disposables.add(editor.on_did_content_size_change().listen({
            let handles = handles.clone();
            move |e| {
                if !e.content_height_changed {
                    return;
                }
                if let Some((this, editor)) = handles.upgrade() {
                    this.on_cell_editor_height_change(&editor, e.content_height);
                }
            }
        }));

        self.local_disposables.add(editor.on_did_change_cursor_selection().listen({
            let handles = handles.clone();
            move |e| {
                if e.source == "restoreState" {
                    // do not reveal the cell into view if this selection change was caused by restoring editors...
                    return;
                }
                let Some((this, editor)) = handles.upgrade() else { return };

                let Some(selections) = editor.get_selections().filter(|selections| !selections.is_empty()) else { return };
                let content_height = editor.get_content_height();
                let layout_content_height = this.markup.layout_info().editor_height;

                if content_height != layout_content_height {
                    this.on_cell_editor_height_change(&editor, content_height);
                }
                let last_selection = selections[selections.len() - 1];
                this.notebook_editor.reveal_range_in_view_async(&this.view_cell, &last_selection);
            }
        }));

        self.local_disposables.add(editor.on_did_focus_editor_widget().listen({
            let handles = handles.clone();
            move |_| {
                if let Some((this, editor)) = handles.upgrade() {
                    this.update_focus_mode(&editor);
                }
            }
        }));

        self.local_disposables.add(editor.on_did_blur_editor_widget().listen({
            let handles = handles.clone();
            move |_| {
                let Some((this, editor)) = handles.upgrade() else { return };
                // this is for a special case:
                // users click the status bar empty space, which we will then focus the editor
                // so we don't want to update the focus state too eagerly
                let container = this.template_data.container;
                let focus_inside = this.ui.focused().is_some_and(|active| this.ui.tree().is_ancestor(active, container));
                if focus_inside {
                    let handles_later = this.handles(&editor);
                    this.focus_switch_disposable.set(Some(disposable_timeout(
                        move || {
                            if let Some((this, editor)) = handles_later.upgrade() {
                                this.update_focus_mode(&editor);
                            }
                        },
                        BLUR_FOCUS_DELAY_MS,
                        None,
                    )));
                } else {
                    this.update_focus_mode(&editor);
                }
            }
        }));

        self.update_focus_mode(editor);
    }

    fn on_cell_editor_height_change(&self, editor: &Rc<dyn IMarkupCellEditor>, new_height: f64) {
        let width = editor.get_layout_width();
        self.markup.set_editor_height(new_height);
        editor.layout(CellEditorDimension { width, height: new_height });
    }
}

impl IDisposable for MarkupCell {
    fn dispose(&self) {
        self.is_disposed.set(true);
        self.set_overflow_widgets_visible(false);

        // move focus back to the cell list otherwise the focus goes to body
        let is_active_cell = self.notebook_editor.get_active_cell().is_some_and(|active| active.ptr_eq(&self.view_cell));
        if is_active_cell && self.markup.focus_mode() == CellFocusMode::Editor && (self.notebook_editor.has_editor_focus() || self.body_has_focus()) {
            self.notebook_editor.focus_container();
        }

        self.services.editor_host.detach_text_editor(&self.view_cell);
        self.base.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "MarkupCell"
    }
}
