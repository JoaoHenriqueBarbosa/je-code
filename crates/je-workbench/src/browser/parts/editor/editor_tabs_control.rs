//! Porte de `src/vs/workbench/browser/parts/editor/editorTabsControl.ts`: o contrato [`IEditorTabsControl`], o
//! [`EditorCommandsContextActionRunner`] e a base [`EditorTabsControlBase`] (o estado e a lógica que o `MultiEditorTabsControl`
//! e o `SingleEditorTabsControl` herdam): altura da aba, visibilidade das barras de ferramentas de ações do editor e de
//! layout, contexto do menu de uma aba, o título do hover, e as decisões do arrastar e soltar (mover ou copiar, abrir em
//! janela nova, onde posicionar a janela auxiliar, o que o arrastar de um grupo carrega).
//!
//! Divergências:
//! - **Sem DOM.** A UI é nativa (`je-ui`, `je-widgets`). Tudo que o original lê do `HTMLElement` pai (`classList.contains('tabs')`,
//!   `closest('.modern-ui-tabs')`, `closest(CONNECTED_EDITOR_TABS_SELECTOR)`) chega como [`TabsParentTraits`], e o que o
//!   original escreve no pai (`--editor-group-tab-height`, `compact-height`, `tab-actions-reserve-space`) fica em
//!   [`ParentStyle`], que o desenho nativo lê. A criação dos widgets (`WorkbenchToolBar`, `EditorAddTabControl`) e os
//!   listeners do `DragAndDropObserver` vêm com a renderização nativa; aqui ficam as decisões que eles tomam.
//! - **`groupView`, `groupsView`, `editorPartsView`** são o trait [`TabsControlHost`]: só o que a base lê dos três.
//! - **`Themable`** (`getColor`, `updateStyles`) é o `ThemeColors` do chamador; a base não guarda cor.
//! - **`ResourceContextKey` e `applyAvailableEditorIds`** dependem do `ILanguageService` e do `IModelService`, que ainda não
//!   estão portados (ver `PORT_PLAN.md`): [`TabContextKeys`] calcula as nove chaves booleanas do menu de contexto da aba, e o
//!   recurso e os ids de editores disponíveis ficam com quem mostra o menu.
//! - **`getHoverTitle`** devolve [`HoverTitle`], com o texto de markdown já montado (o `appendText` escapa os caracteres
//!   especiais do título como o original).
//! - **`DragEvent`** é [`DragModifiers`] (`altKey`, `ctrlKey`); `hostService.getCursorScreenPoint` e `getActiveWindow` chegam
//!   como [`CursorScreenPoint`] e [`ActiveWindowBounds`].
//! - **`moveEditor(editor, fromIndex, targetIndex, stickyStateChange)`** da interface tem quatro parâmetros e a classe
//!   abstrata três; o contrato segue a interface e as duas implementações ignoram o quarto, como no original.
//!
//! Fatias seguintes: a fatia 2 traz o resto de `multiEditorTabsControl.ts` (os listeners de cada aba, o arrastar e soltar de
//! uma aba, a seleção múltipla com Ctrl e Shift, o `onDrop`, o layout com quebra de linha `doLayoutTabsWrapping`, o recorte
//! das abas conectadas e a medição `getFitTabWidth`); a fatia 3 traz o teste `multiEditorTabsControl.test.ts` (86 casos).

use std::rc::Rc;

use je_base::actions::{ActionRef, ActionRunner, IActionRunner, RunAction};
use je_base::dynamic::{Dynamic, ObjectRef};
use je_base::lifecycle::IDisposable;
use je_editor::core_2d::dimension::Dimension;

use crate::browser::parts::editor::editor::{CONNECTED_EDITOR_TABS_CLASS, CONNECTED_EDITOR_TABS_SELECTOR};
use crate::common::editor::editor_group_model::{EditorOrIndex, IReadonlyEditorGroupModel};
use crate::common::editor::side_by_side_editor_input::SideBySideEditorInput;
use crate::common::editor::{EditorCommandsContext, EditorInputCapabilities, EditorInputRef, EditorsOrder, GroupIdentifier, Verbosity};
use crate::services::editor::editor_groups_service::{EditorActionsLocation, EditorPartOptions, ShowTabs, TabHeight, ToolbarActions};

/// `EditorCommandsContextActionRunner`: o contexto de comandos de editor é fixo, mas aceita o `preserveFocus` do contexto que
/// recebe. O `context` do original é um objeto (pode ter getters, como o `editorIndex` de uma aba), por isso é um provedor
/// chamado a cada `run`.
pub struct EditorCommandsContextActionRunner {
    runner: Rc<ActionRunner>,
}

impl EditorCommandsContextActionRunner {
    /// Contexto fixo.
    pub fn new(context: EditorCommandsContext) -> Self {
        Self::with_provider(Rc::new(move || context))
    }

    /// Contexto lido a cada execução (o `get editorIndex()` do original).
    pub fn with_provider(provider: Rc<dyn Fn() -> EditorCommandsContext>) -> Self {
        let run_action: RunAction = Rc::new(move |action, context| {
            // Even though we have a fixed context for editor commands, allow to preserve the context that is given to us in
            // case it applies.
            let mut merged = provider();
            if preserve_focus_of(context.as_ref()) {
                merged.preserve_focus = Some(true);
            }

            action.run(&[editor_commands_context_to_dynamic(&merged)])
        });

        Self { runner: ActionRunner::with_run_action(run_action) }
    }

    /// O [`ActionRunner`] por baixo, para passar a uma barra de ações.
    pub fn runner(&self) -> &Rc<ActionRunner> {
        &self.runner
    }

    /// `run(action, context?)`.
    pub fn run(&self, action: &ActionRef, context: Option<Dynamic>) -> je_base::promise::Promise<()> {
        self.runner.run(action, context)
    }
}

impl IDisposable for EditorCommandsContextActionRunner {
    fn dispose(&self) {
        self.runner.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "EditorCommandsContextActionRunner"
    }
}

/// `context?.preserveFocus`.
fn preserve_focus_of(context: Option<&Dynamic>) -> bool {
    let Some(Dynamic::Object(object)) = context else { return false };

    object.entries().into_iter().any(|(key, value)| key.to_string() == "preserveFocus" && matches!(value, Dynamic::Bool(true)))
}

/// O `IEditorCommandsContext` como o objeto que a ação recebe.
pub fn editor_commands_context_to_dynamic(context: &EditorCommandsContext) -> Dynamic {
    let mut entries: Vec<(&str, Dynamic)> = vec![("groupId", Dynamic::Number(context.group_id as f64))];
    if let Some(editor_index) = context.editor_index {
        entries.push(("editorIndex", Dynamic::Number(editor_index as f64)));
    }
    if let Some(preserve_focus) = context.preserve_focus {
        entries.push(("preserveFocus", Dynamic::Bool(preserve_focus)));
    }

    Dynamic::Object(ObjectRef::from_entries(entries))
}

/// `IEditorTitleControlDimensions` (de `editorTitleControl.ts`): o contêiner do título e o espaço disponível para as abas.
/// `None` é o `Dimension.None` do original (ainda sem medida).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EditorTitleControlDimensions {
    /// The size of the parent container the title control is layed out in.
    pub container: Option<Dimension>,
    /// The maximum size the title control is allowed to consume based on other controls that are positioned inside the
    /// container.
    pub available: Option<Dimension>,
}

/// `IInternalEditorOpenOptions.focusTabControl` (a única opção interna que o controle de abas lê).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TabsOpenOptions {
    pub focus_tab_control: bool,
}

/// `IEditorTabsControl`.
pub trait IEditorTabsControl: IDisposable {
    fn update_options(&self, old_options: &EditorPartOptions, new_options: &EditorPartOptions);
    fn open_editor(&self, editor: &EditorInputRef, options: Option<TabsOpenOptions>) -> bool;
    fn open_editors(&self, editors: &[EditorInputRef]) -> bool;
    fn before_close_editor(&self, editor: &EditorInputRef);
    fn close_editor(&self, editor: &EditorInputRef);
    fn close_editors(&self, editors: &[EditorInputRef]);
    fn move_editor(&self, editor: &EditorInputRef, from_index: usize, target_index: usize, sticky_state_change: bool);
    fn pin_editor(&self, editor: &EditorInputRef);
    fn stick_editor(&self, editor: &EditorInputRef);
    fn unstick_editor(&self, editor: &EditorInputRef);
    fn set_active(&self, is_active: bool);
    fn update_editor_selections(&self);
    fn update_editor_label(&self, editor: &EditorInputRef);
    fn update_editor_capabilities(&self, editor: &EditorInputRef);
    fn update_editor_dirty(&self, editor: &EditorInputRef);
    fn layout(&self, dimensions: EditorTitleControlDimensions) -> Dimension;
    fn get_height(&self) -> f64;
}

/// O que a base lê de `groupView`, `groupsView` e `editorPartsView`.
pub trait TabsControlHost {
    /// `groupView.id`.
    fn group_id(&self) -> GroupIdentifier;
    /// `groupView.label`.
    fn group_label(&self) -> String;
    /// `groupView.ariaLabel`.
    fn group_aria_label(&self) -> String;
    /// `groupView.count`.
    fn group_count(&self) -> usize;
    /// `groupView.activeEditor`.
    fn active_editor(&self) -> Option<EditorInputRef>;
    /// `groupView.selectedEditors`.
    fn selected_editors(&self) -> Vec<EditorInputRef>;
    /// `groupView.getEditors(EditorsOrder.SEQUENTIAL)`.
    fn group_editors(&self) -> Vec<EditorInputRef>;
    /// `groupView.getIndexOfEditor(editor)`.
    fn index_of_editor(&self, editor: &EditorInputRef) -> i64;
    /// `groupView.isActive(editor)`.
    fn is_active(&self, editor: &EditorInputRef) -> bool;
    /// `groupView.isPinned(editor)`.
    fn is_pinned(&self, editor: &EditorInputRef) -> bool;
    /// `groupView.isSticky(index)`.
    fn is_sticky_at(&self, index: usize) -> bool;
    /// `groupsView.activeGroup === groupView`.
    fn is_group_active(&self) -> bool;
    /// `groupsView.partOptions`.
    fn part_options(&self) -> EditorPartOptions;
    /// `editorPartsView.count`.
    fn editor_parts_count(&self) -> usize;
    /// `groupView.relayout()`.
    fn relayout(&self);
}

/// Os atributos do `HTMLElement` pai que a base consulta.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TabsParentTraits {
    /// `parent.classList.contains('tabs')`: só existe quando `showTabs === 'multiple'`.
    pub has_tabs_class: bool,
    /// `parent.closest('.modern-ui-tabs')`.
    pub in_modern_ui_tabs: bool,
    /// `parent.closest(CONNECTED_EDITOR_TABS_SELECTOR)`.
    pub in_connected_editor_tabs: bool,
}

impl TabsParentTraits {
    /// O seletor de `closest` equivalente, para o desenho nativo achar o contêiner conectado.
    pub const CONNECTED_SELECTOR: &'static str = CONNECTED_EDITOR_TABS_SELECTOR;
    /// A classe do contêiner conectado.
    pub const CONNECTED_CLASS: &'static str = CONNECTED_EDITOR_TABS_CLASS;
}

/// O que `updateTabHeight` e `updateTabActionSpaceReservation` escrevem no elemento pai e no pai do pai.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ParentStyle {
    /// `--editor-group-tab-height`, em pixels.
    pub editor_group_tab_height: f64,
    /// `compact-height` no pai.
    pub compact_height: bool,
    /// `editor-tabs-compact-height` no pai do pai.
    pub grandparent_compact_height: bool,
    /// `tab-actions-reserve-space`.
    pub tab_actions_reserve_space: bool,
}

/// `EditorTabsControl.EDITOR_TAB_HEIGHT`.
pub struct EditorTabHeight;

impl EditorTabHeight {
    pub const NORMAL: f64 = 35.0;
    pub const COMPACT: f64 = 22.0;
    /// Modern UI multi-tab mode adds 4px top + 4px bottom padding to the tabs-and-actions-container (tabs.css), so the total
    /// title-bar height is the --editor-group-tab-height CSS value (24px / 20px) plus that 8px padding.
    pub const MODERN_UI: f64 = 32.0;
    pub const MODERN_UI_COMPACT: f64 = 28.0;
    pub const CONNECTED: f64 = 28.0;
    pub const CONNECTED_COMPACT: f64 = 24.0;
}

/// O `get tabHeight()`.
pub fn compute_tab_height(is_compact: bool, parent: TabsParentTraits, use_modern_ui_tabs: bool) -> f64 {
    // In modern multi-tab mode the tabs-and-actions-container gains extra padding (tabs.css), so the total height differs
    // from the base values. The `.tabs` class is present only when showTabs === 'multiple'; single-tab hosts using modern UI
    // tabs opt into the same aligned height.
    let uses_modern_multi_tab_height = parent.has_tabs_class && parent.in_modern_ui_tabs;
    if uses_modern_multi_tab_height || use_modern_ui_tabs {
        if uses_modern_multi_tab_height && parent.in_connected_editor_tabs {
            return (if is_compact { EditorTabHeight::CONNECTED_COMPACT } else { EditorTabHeight::CONNECTED }) + 1.0;
        }

        return if is_compact { EditorTabHeight::MODERN_UI_COMPACT } else { EditorTabHeight::MODERN_UI };
    }

    if is_compact { EditorTabHeight::COMPACT } else { EditorTabHeight::NORMAL }
}

/// `getHoverTitle`: o texto simples, ou o texto de markdown de uma aba em modo de pré-visualização.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HoverTitle {
    Plain(String),
    Preview {
        /// O markdown (`isTrusted`, `supportThemeIcons`).
        markdown: String,
        /// `markdownNotSupportedFallback`.
        fallback: String,
    },
}

/// `MarkdownString.appendText`: escapa os caracteres que o markdown interpretaria.
fn escape_markdown_text(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\\' | '`' | '*' | '_' | '{' | '}' | '[' | ']' | '(' | ')' | '#' | '+' | '-' | '!' | '~' => {
                out.push('\\');
                out.push(ch);
            }
            '>' => out.push_str("\\>"),
            ' ' | '\t' => {
                let mut run = 1;
                while matches!(chars.peek(), Some(' ' | '\t')) {
                    chars.next();
                    run += 1;
                }
                out.push_str(&"&nbsp;".repeat(run));
            }
            '\n' => out.push_str("\n\\"),
            other => out.push(other),
        }
    }

    out
}

/// A barra de ferramentas de ações do editor e a de layout: quais existem e o que está visível.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EditorActionsToolbarState {
    /// `editorActionsToolbar` existe.
    pub toolbar_created: bool,
    /// O contêiner tem a classe `hidden`.
    pub container_hidden: bool,
    /// `editorActionsToolbarHasActions`.
    pub has_actions: bool,
    /// `editorLayoutActionsToolbar` existe.
    pub layout_toolbar_created: bool,
    /// O contêiner da barra de layout tem a classe `hidden`.
    pub layout_container_hidden: bool,
    /// `setVisibility(.., editorLayoutActionsSeparator)`.
    pub separator_visible: bool,
    /// `setVisibility(.., editorLayoutActionsToolbarContainer)`.
    pub layout_container_visible: bool,
}

/// A decisão de [`EditorActionsToolbarState::handle_visibility`]: o que o desenho nativo faz com a barra.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolbarVisibilityChange {
    /// A barra deve ser criada.
    Create,
    /// A barra deve ser removida (e as ações liberadas).
    Remove,
    /// Nada a criar ou remover.
    Keep,
}

impl EditorActionsToolbarState {
    /// `handleEditorActionToolBarVisibility`.
    pub fn handle_visibility(&mut self, editor_actions_enabled: bool) -> ToolbarVisibilityChange {
        let visible = self.toolbar_created;
        let change = if editor_actions_enabled && !visible {
            // Create toolbar if it is enabled (and not yet created)
            self.toolbar_created = true;
            ToolbarVisibilityChange::Create
        } else if !editor_actions_enabled && visible {
            // Remove toolbar if it is not enabled (and is visible)
            self.toolbar_created = false;
            ToolbarVisibilityChange::Remove
        } else {
            ToolbarVisibilityChange::Keep
        };

        self.container_hidden = !editor_actions_enabled;
        change
    }

    /// `handleEditorLayoutActionsToolBarVisibility`.
    pub fn handle_layout_visibility(&mut self, editor_actions_enabled: bool) -> ToolbarVisibilityChange {
        let visible = self.layout_toolbar_created;
        let change = if editor_actions_enabled && !visible {
            self.layout_toolbar_created = true;
            ToolbarVisibilityChange::Create
        } else if !editor_actions_enabled && visible {
            self.layout_toolbar_created = false;
            ToolbarVisibilityChange::Remove
        } else {
            ToolbarVisibilityChange::Keep
        };

        self.layout_container_hidden = !editor_actions_enabled;

        // Keep the sibling separator in sync with the toolbar. The separator lives outside the hidden containers so it must be
        // explicitly hidden whenever the layout toolbar is disabled/removed; otherwise it would remain visible as an orphan
        // line.
        if !editor_actions_enabled {
            self.separator_visible = false;
        }

        change
    }

    /// `updateEditorLayoutActionsSeparator`.
    pub fn update_layout_separator(&mut self, layout_items: usize) {
        self.separator_visible = layout_items > 0 && self.has_actions;
    }

    /// O fim de `updateEditorActionsToolbar`: registra se há ações.
    pub fn set_actions(&mut self, actions: &ToolbarActions) {
        self.has_actions = !actions.primary.is_empty() || !actions.secondary.is_empty();
    }

    /// O fim de `updateEditorLayoutActionsToolbar`: o separador e o contêiner só aparecem com ações de layout e ações do
    /// editor à esquerda para separar.
    pub fn set_layout_actions(&mut self, actions: &ToolbarActions) {
        let has_layout_actions = !actions.primary.is_empty() || !actions.secondary.is_empty();
        self.update_layout_separator(actions.primary.len() + actions.secondary.len());
        self.layout_container_visible = has_layout_actions;
    }

    /// `clearEditorActionsToolbar`.
    pub fn clear(&mut self) {
        self.has_actions = false;
        self.separator_visible = false;
        self.layout_container_visible = false;
    }
}

/// `editorActionsEnabled`.
pub fn editor_actions_enabled(options: &EditorPartOptions) -> bool {
    options.editor_actions_location == EditorActionsLocation::Default && options.show_tabs != ShowTabs::None
}

/// As chaves de contexto que o menu de contexto de uma aba vê (`onTabContextMenu`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TabContextKeys {
    pub pinned: bool,
    pub first: bool,
    pub last: bool,
    pub sticky: bool,
    pub dirty: bool,
    pub cannot_close: bool,
    pub group_locked: bool,
    pub can_split_in_group: bool,
    pub side_by_side: bool,
}

impl TabContextKeys {
    pub fn for_editor(tabs_model: &dyn IReadonlyEditorGroupModel, editor: &EditorInputRef) -> Self {
        let editor_or_index = EditorOrIndex::Editor(editor.clone());

        Self {
            pinned: tabs_model.is_pinned(&editor_or_index),
            first: tabs_model.is_first(editor, None),
            last: tabs_model.is_last(editor, None),
            sticky: tabs_model.is_sticky(&editor_or_index),
            dirty: editor.is_dirty() && !editor.is_saving(),
            cannot_close: editor.has_capability(EditorInputCapabilities::CANNOT_CLOSE),
            group_locked: tabs_model.is_locked(),
            can_split_in_group: editor.has_capability(EditorInputCapabilities::CAN_SPLIT_IN_GROUP),
            side_by_side: editor.editor_type_id() == SideBySideEditorInput::ID,
        }
    }
}

/// `DragEvent`: os modificadores que as decisões leem.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DragModifiers {
    pub alt_key: bool,
    pub ctrl_key: bool,
}

/// `hostService.getCursorScreenPoint()` (ou `{ x: e.screenX, y: e.screenY }` sem ele).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CursorScreenPoint {
    pub x: f64,
    pub y: f64,
    /// A tela sob o cursor, quando o host a conhece.
    pub display: Option<ScreenOrigin>,
}

/// `display.x` e `display.y`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScreenOrigin {
    pub x: f64,
    pub y: f64,
}

/// A janela ativa (`getActiveWindow()`): visível, com foco e seus limites na tela.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ActiveWindowBounds {
    pub visible_and_focused: bool,
    pub screen_x: f64,
    pub screen_y: f64,
    pub outer_width: f64,
    pub outer_height: f64,
}

/// `{ bounds }` do `createAuxiliaryEditorPart`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AuxiliaryPartBounds {
    pub x: f64,
    pub y: f64,
}

/// `maybeCreateAuxiliaryEditorPartAt`, até a criação: `None` se o mouse foi solto sobre a janela ativa e focada, senão a
/// posição da janela auxiliar. `offset_width` e `offset_height` são do elemento arrastado.
pub fn auxiliary_part_bounds(cursor: CursorScreenPoint, window: ActiveWindowBounds, offset_width: f64, offset_height: f64) -> Option<AuxiliaryPartBounds> {
    if window.visible_and_focused
        && cursor.x >= window.screen_x
        && cursor.x <= window.screen_x + window.outer_width
        && cursor.y >= window.screen_y
        && cursor.y <= window.screen_y + window.outer_height
    {
        return None; // refuse to create as long as the mouse was released over active focused window to reduce chance of opening by accident
    }

    let offset_x = offset_width / 2.0;
    let offset_y = 30.0 /* take title bar height into account (approximation) */ + offset_height / 2.0;

    let mut bounds = AuxiliaryPartBounds { x: cursor.x - offset_x, y: cursor.y - offset_y };
    if let Some(display) = cursor.display {
        if bounds.x < display.x {
            bounds.x = display.x; // prevent overflow to the left
        }

        if bounds.y < display.y {
            bounds.y = display.y; // prevent overflow to the top
        }
    }

    Some(bounds)
}

/// O que o arrastar de um grupo carrega (`onGroupDragStart`).
#[derive(Clone)]
pub struct GroupDragPlan {
    /// Os editores do `fillEditorsDragData` (vazio: nada a preencher).
    pub editors: Vec<EditorInputRef>,
    /// O rótulo da imagem do arrastar, quando há editor ativo.
    pub drag_image_label: Option<String>,
    /// O arrastar é uma operação de janela nova.
    pub is_new_window_operation: bool,
}

/// A base de `EditorTabsControl`: o estado e a lógica compartilhados, sem os widgets.
pub struct EditorTabsControlBase {
    pub host: Rc<dyn TabsControlHost>,
    pub tabs_model: Rc<dyn IReadonlyEditorGroupModel>,
    pub breadcrumbs_in_header: bool,
    pub use_modern_ui_tabs: bool,
    pub parent: TabsParentTraits,
    pub parent_style: ParentStyle,
    pub toolbars: EditorActionsToolbarState,
}

impl EditorTabsControlBase {
    /// O construtor e `create(parent)`: calcula a altura e a reserva de espaço das ações da aba.
    pub fn new(
        host: Rc<dyn TabsControlHost>,
        tabs_model: Rc<dyn IReadonlyEditorGroupModel>,
        parent: TabsParentTraits,
        breadcrumbs_in_header: bool,
        use_modern_ui_tabs: bool,
    ) -> Self {
        let mut base = Self { host, tabs_model, breadcrumbs_in_header, use_modern_ui_tabs, parent, parent_style: ParentStyle::default(), toolbars: EditorActionsToolbarState::default() };
        base.update_tab_height();
        base.update_tab_action_space_reservation();
        base
    }

    /// `editorActionsEnabled`.
    pub fn editor_actions_enabled(&self) -> bool {
        editor_actions_enabled(&self.host.part_options())
    }

    /// `tabHeight`.
    pub fn tab_height(&self) -> f64 {
        compute_tab_height(self.host.part_options().tab_height == TabHeight::Compact, self.parent, self.use_modern_ui_tabs)
    }

    /// `updateTabHeight`.
    pub fn update_tab_height(&mut self) {
        let is_compact = self.host.part_options().tab_height == TabHeight::Compact;
        self.parent_style.editor_group_tab_height = self.tab_height();
        // Signal compact mode via a CSS class so the modern tab rules in tabs.css can apply a proportionally smaller
        // --editor-group-tab-height value.
        self.parent_style.compact_height = is_compact;
        self.parent_style.grandparent_compact_height = is_compact;
    }

    /// `updateTabActionSpaceReservation`.
    pub fn update_tab_action_space_reservation(&mut self) {
        self.parent_style.tab_actions_reserve_space = self.host.part_options().tab_action_reserve_space;
    }

    /// O `updateOptions` da base. Devolve se as barras de ferramentas mudaram de visibilidade e precisam ser atualizadas.
    pub fn update_options(&mut self, old_options: &EditorPartOptions, new_options: &EditorPartOptions) -> bool {
        // Update tab height
        if old_options.tab_height != new_options.tab_height {
            self.update_tab_height();
        }

        if old_options.tab_action_reserve_space != new_options.tab_action_reserve_space {
            self.update_tab_action_space_reservation();
        }

        // Update Editor Actions Toolbar
        if old_options.editor_actions_location != new_options.editor_actions_location || old_options.show_tabs != new_options.show_tabs {
            let enabled = editor_actions_enabled(new_options);
            self.toolbars.handle_visibility(enabled);
            self.toolbars.handle_layout_visibility(enabled);
            return true;
        }

        false
    }

    /// `getHoverTitle`.
    pub fn get_hover_title(&self, editor: &EditorInputRef) -> HoverTitle {
        let title = editor.get_title(Some(Verbosity::Long));
        if !self.tabs_model.is_pinned(&EditorOrIndex::Editor(editor.clone())) {
            return HoverTitle::Preview {
                markdown: format!(
                    "{} (_preview_ [$(gear)](command:workbench.action.openSettings?%5B%22workbench.editor.enablePreview%22%5D \"Configure Preview Mode\"))",
                    escape_markdown_text(&title)
                ),
                fallback: format!("{title} (preview)"),
            };
        }

        HoverTitle::Plain(title)
    }

    /// `isNewWindowOperation`.
    pub fn is_new_window_operation(&self, modifiers: DragModifiers) -> bool {
        if self.host.part_options().drag_to_open_window {
            return !modifiers.alt_key;
        }

        modifiers.alt_key
    }

    /// `isMoveOperation`. `is_macintosh` é a plataforma (`isMacintosh`).
    pub fn is_move_operation(&self, modifiers: DragModifiers, source_group: GroupIdentifier, source_editor: Option<&EditorInputRef>, is_macintosh: bool) -> bool {
        if source_editor.is_some_and(|editor| editor.has_capability(EditorInputCapabilities::SINGLETON)) {
            return true; // Singleton editors cannot be split
        }

        let is_copy = (modifiers.ctrl_key && !is_macintosh) || (modifiers.alt_key && is_macintosh);

        !is_copy || source_group == self.host.group_id()
    }

    /// O planejamento de `onGroupDragStart` (o `target === element` é checado por quem escuta o evento).
    pub fn plan_group_drag(&self, modifiers: DragModifiers) -> GroupDragPlan {
        let is_new_window_operation = self.is_new_window_operation(modifiers);
        let options = self.host.part_options();
        let active_editor = self.host.active_editor();

        // Drag all tabs of the group if tabs are enabled
        let editors = if options.show_tabs == ShowTabs::Multiple {
            self.host.group_editors()
        }
        // Otherwise only drag the active editor
        else {
            active_editor.iter().cloned().collect()
        };

        // Drag Image
        let drag_image_label = active_editor.map(|editor| {
            let name = editor.get_name();
            if options.show_tabs == ShowTabs::Multiple && self.host.group_count() > 1 {
                format!("{} (+{})", name, self.host.group_count() - 1)
            } else {
                name
            }
        });

        GroupDragPlan { editors, drag_image_label, is_new_window_operation }
    }

    /// Todos os editores do grupo em ordem sequencial, do modelo de abas.
    pub fn tab_editors(&self) -> Vec<EditorInputRef> {
        self.tabs_model.get_editors(EditorsOrder::Sequential, None)
    }
}
