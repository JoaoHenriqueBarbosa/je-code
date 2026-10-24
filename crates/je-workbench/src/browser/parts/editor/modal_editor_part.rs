//! Porte de `src/vs/workbench/browser/parts/editor/modalEditorPart.ts`, no que ele tem de lógica sem DOM: as
//! constantes de medida, os comandos permitidos dentro do modal, o estado da parte (maximizar, tamanho, posição,
//! navegação e barra lateral, com os eventos), e a aritmética de layout (tamanho padrão, posição, arrasto, redimensionamento
//! e barra lateral) como funções puras.
//!
//! Notas de tradução:
//! - **O que o original faz com `HTMLElement`** (criar o bloco modal, cabeçalho, botões, `Sash`, `ResizableHTMLElement`,
//!   escutar mouse e teclado, `ResourceLabel`, barras de ferramentas) é desenho e eventos da árvore de widgets nativa, e
//!   fica com ela. As decisões que esses trechos tomam são as funções deste módulo: quem desenha chama `default_modal_size`,
//!   `modal_layout_size`, `modal_position`, `drag_position`, `clamp_resize`, e os `sidebar_*`, e aplica o resultado.
//! - **`ModalEditorPartImpl`** estende `EditorPart`, que ainda não foi portada: o que ele guarda de dados e de eventos é o
//!   [`ModalEditorPartState`] (o construtor, `updateOptions`, `toggleMaximized`, `toggleSidebar` e `handleHeaderDoubleClick`).
//!   `enforceModalPartOptions`, `removeGroup`, `close` e `mergeGroupsToMainPart` agem sobre os grupos da `EditorPart`.
//! - **`IDimension`** é o [`EditorSize`], e `IPosition` é o [`EditorPosition`], ambos de `je_platform::opener::editor_types`.
//! - **Eventos.** `Emitter<IModalEditorNavigation | undefined>` é `Emitter<Option<ModalEditorNavigation>>`.
//! - **`defaultModalEditorAllowableCommands`** é [`DEFAULT_MODAL_EDITOR_ALLOWABLE_COMMANDS`], e a decisão do `keydown`
//!   (`useModalMode !== 'all'`, `startsWith('workbench.')`, fora do conjunto) é [`is_command_blocked_in_modal_editor`].
//! - **Constantes de outros arquivos.** `CLOSE_MODAL_EDITOR_COMMAND_ID` e os demais ids de comando são de
//!   `editorCommands.ts`; `USE_MODAL_EDITOR_SETTING` e `UseModalEditorMode` são de `editorService.ts`. Entram aqui
//!   porque este módulo é quem os lê, com o valor do original.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::nls::localize;
use je_platform::opener::editor_types::{
    EditorPosition, EditorSize, LayoutDimension, ModalEditorNavigation, ModalEditorOptionsProvider, ModalEditorPartOptions,
};

pub const MODAL_MIN_WIDTH: f64 = 400.0;
pub const MODAL_MIN_HEIGHT: f64 = 300.0;
pub const MODAL_MAX_DEFAULT_WIDTH: f64 = 1400.0;
pub const MODAL_MAX_DEFAULT_HEIGHT: f64 = 900.0;
/// 1px border on each side.
pub const MODAL_BORDER_WIDTH: f64 = 1.0;
pub const MODAL_BORDER_SIZE: f64 = MODAL_BORDER_WIDTH * 2.0;
/// Fallback only: actual height is measured from the rendered header element to account for the compact-header variant.
pub const MODAL_HEADER_HEIGHT: f64 = 33.0;
pub const MODAL_SNAP_THRESHOLD: f64 = 20.0;
pub const MODAL_MAXIMIZED_PADDING: f64 = 16.0;
pub const MODAL_SIDEBAR_MIN_WIDTH: f64 = 160.0;
pub const MODAL_SIDEBAR_DEFAULT_WIDTH: f64 = 260.0;
/// Matches CSS padding on sidebar container.
pub const MODAL_SIDEBAR_PADDING: f64 = 8.0;
/// Matches CSS border-right on sidebar container.
pub const MODAL_SIDEBAR_BORDER_RIGHT: f64 = 1.0;

/// `CLOSE_MODAL_EDITOR_COMMAND_ID` (`editorCommands.ts`).
pub const CLOSE_MODAL_EDITOR_COMMAND_ID: &str = "workbench.action.closeModalEditor";
/// `MOVE_MODAL_EDITOR_TO_MAIN_COMMAND_ID` (`editorCommands.ts`).
pub const MOVE_MODAL_EDITOR_TO_MAIN_COMMAND_ID: &str = "workbench.action.moveModalEditorToMain";
/// `MOVE_MODAL_EDITOR_TO_WINDOW_COMMAND_ID` (`editorCommands.ts`).
pub const MOVE_MODAL_EDITOR_TO_WINDOW_COMMAND_ID: &str = "workbench.action.moveModalEditorToWindow";
/// `TOGGLE_MODAL_EDITOR_MAXIMIZED_COMMAND_ID` (`editorCommands.ts`).
pub const TOGGLE_MODAL_EDITOR_MAXIMIZED_COMMAND_ID: &str = "workbench.action.toggleModalEditorMaximized";
/// `NAVIGATE_MODAL_EDITOR_PREVIOUS_COMMAND_ID` (`editorCommands.ts`).
pub const NAVIGATE_MODAL_EDITOR_PREVIOUS_COMMAND_ID: &str = "workbench.action.navigateModalEditorPrevious";
/// `NAVIGATE_MODAL_EDITOR_NEXT_COMMAND_ID` (`editorCommands.ts`).
pub const NAVIGATE_MODAL_EDITOR_NEXT_COMMAND_ID: &str = "workbench.action.navigateModalEditorNext";
/// `TOGGLE_MODAL_EDITOR_SIDEBAR_COMMAND_ID` (`editorCommands.ts`).
pub const TOGGLE_MODAL_EDITOR_SIDEBAR_COMMAND_ID: &str = "workbench.action.toggleModalEditorSidebar";

/// `USE_MODAL_EDITOR_SETTING` (`editorService.ts`).
pub const USE_MODAL_EDITOR_SETTING: &str = "workbench.editor.useModal";

/// `UseModalEditorMode` (`editorService.ts`): `'off' | 'some' | 'all'`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UseModalEditorMode {
    Off,
    Some,
    All,
}

impl UseModalEditorMode {
    /// O modo que o texto da configuração representa.
    pub fn from_text(text: &str) -> Option<Self> {
        match text {
            "off" => Some(Self::Off),
            "some" => Some(Self::Some),
            "all" => Some(Self::All),
            _ => None,
        }
    }
}

/// `defaultModalEditorAllowableCommands`.
pub const DEFAULT_MODAL_EDITOR_ALLOWABLE_COMMANDS: &[&str] = &[
    // Application
    "workbench.action.quit",
    "workbench.action.reloadWindow",
    "workbench.action.toggleFullScreen",
    // Quick access
    "workbench.action.gotoSymbol",
    "workbench.action.gotoLine",
    // Zoom
    "workbench.action.zoomIn",
    "workbench.action.zoomOut",
    "workbench.action.zoomReset",
    // File operations
    "workbench.action.files.save",
    "workbench.action.files.saveAll",
    "workbench.action.files.revert",
    // Close editors
    "workbench.action.closeActiveEditor",
    "workbench.action.closeAllEditors",
    "workbench.action.closeEditorsInGroup",
    "workbench.action.closeUnmodifiedEditors",
    // Settings
    "workbench.action.openSettings",
    "workbench.action.openSettings2",
    "workbench.action.openSettingsJson",
    "workbench.action.openGlobalSettings",
    "workbench.action.openApplicationSettingsJson",
    "workbench.action.openRawDefaultSettings",
    "workbench.action.openWorkspaceSettings",
    "workbench.action.openWorkspaceSettingsFile",
    "workbench.action.openFolderSettings",
    "workbench.action.openFolderSettingsFile",
    "workbench.action.openRemoteSettings",
    "workbench.action.openRemoteSettingsFile",
    "workbench.action.openAccessibilitySettings",
    "workbench.action.configureLanguageBasedSettings",
    // Keybindings
    "workbench.action.openGlobalKeybindings",
    "workbench.action.openDefaultKeybindingsFile",
    "workbench.action.openGlobalKeybindingsFile",
    "workbench.action.openKeyboardLayoutPicker",
    // Modal editor
    CLOSE_MODAL_EDITOR_COMMAND_ID,
    MOVE_MODAL_EDITOR_TO_MAIN_COMMAND_ID,
    MOVE_MODAL_EDITOR_TO_WINDOW_COMMAND_ID,
    TOGGLE_MODAL_EDITOR_MAXIMIZED_COMMAND_ID,
    NAVIGATE_MODAL_EDITOR_PREVIOUS_COMMAND_ID,
    NAVIGATE_MODAL_EDITOR_NEXT_COMMAND_ID,
    TOGGLE_MODAL_EDITOR_SIDEBAR_COMMAND_ID,
];

/// A decisão do `keydown` do bloco modal: "prevent unsupported commands unless all editors open in modal". O comando
/// resolvido para a tecla é barrado quando o modo não é `all`, o id começa com `workbench.` e não está entre os
/// permitidos.
pub fn is_command_blocked_in_modal_editor(use_modal_mode: UseModalEditorMode, command_id: &str) -> bool {
    use_modal_mode != UseModalEditorMode::All && command_id.starts_with("workbench.") && !DEFAULT_MODAL_EDITOR_ALLOWABLE_COMMANDS.contains(&command_id)
}

// ---------------------------------------------------------------------------------------------------------------------
// Estado da parte (`ModalEditorPartImpl`)
// ---------------------------------------------------------------------------------------------------------------------

/// O que `ModalEditorPartImpl` guarda e dispara, sem a `EditorPart` de que ele herda.
pub struct ModalEditorPartState {
    base: Rc<Disposable>,
    maximized: Cell<bool>,
    size: Cell<Option<EditorSize>>,
    position: Cell<Option<EditorPosition>>,
    sidebar_width: Cell<Option<f64>>,
    sidebar_hidden: Cell<bool>,
    has_sidebar: Cell<bool>,
    saved_size: Cell<Option<EditorSize>>,
    saved_position: Cell<Option<EditorPosition>>,
    navigation: RefCell<Option<ModalEditorNavigation>>,
    on_did_change_maximized: Rc<Emitter<bool>>,
    on_did_request_layout: Rc<Emitter<()>>,
    on_did_change_navigation: Rc<Emitter<Option<ModalEditorNavigation>>>,
    on_did_toggle_sidebar: Rc<Emitter<()>>,
}

impl ModalEditorPartState {
    /// O trecho do construtor que lê as `options`.
    pub fn new(options: Option<&ModalEditorPartOptions>) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change_maximized: Rc<Emitter<bool>> = Emitter::new();
        let on_did_request_layout: Rc<Emitter<()>> = Emitter::new();
        let on_did_change_navigation: Rc<Emitter<Option<ModalEditorNavigation>>> = Emitter::new();
        let on_did_toggle_sidebar: Rc<Emitter<()>> = Emitter::new();
        base.register(on_did_change_maximized.clone());
        base.register(on_did_request_layout.clone());
        base.register(on_did_change_navigation.clone());
        base.register(on_did_toggle_sidebar.clone());

        let maximized = options.and_then(|options| options.maximized).unwrap_or(false);
        let size = options.and_then(|options| options.size);
        let position = options.and_then(|options| options.position);
        let sidebar = options.and_then(|options| options.sidebar.as_ref());

        // When restoring a maximized state with custom layout,
        // initialize saved state so un-maximize can restore it
        let (saved_size, saved_position) = if maximized { (size, position) } else { (None, None) };

        Rc::new(Self {
            base,
            maximized: Cell::new(maximized),
            size: Cell::new(size),
            position: Cell::new(position),
            sidebar_width: Cell::new(sidebar.and_then(|sidebar| sidebar.sidebar_width)),
            sidebar_hidden: Cell::new(sidebar.and_then(|sidebar| sidebar.sidebar_hidden).unwrap_or(false)),
            has_sidebar: Cell::new(sidebar.is_some()),
            saved_size: Cell::new(saved_size),
            saved_position: Cell::new(saved_position),
            navigation: RefCell::new(options.and_then(|options| options.navigation.clone())),
            on_did_change_maximized,
            on_did_request_layout,
            on_did_change_navigation,
            on_did_toggle_sidebar,
        })
    }

    pub fn on_did_change_maximized(&self) -> Event<bool> {
        self.on_did_change_maximized.event()
    }

    pub fn on_did_request_layout(&self) -> Event<()> {
        self.on_did_request_layout.event()
    }

    pub fn on_did_change_navigation(&self) -> Event<Option<ModalEditorNavigation>> {
        self.on_did_change_navigation.event()
    }

    pub fn on_did_toggle_sidebar(&self) -> Event<()> {
        self.on_did_toggle_sidebar.event()
    }

    pub fn maximized(&self) -> bool {
        self.maximized.get()
    }

    pub fn size(&self) -> Option<EditorSize> {
        self.size.get()
    }

    pub fn set_size(&self, value: Option<EditorSize>) {
        self.size.set(value);
    }

    pub fn position(&self) -> Option<EditorPosition> {
        self.position.get()
    }

    pub fn set_position(&self, value: Option<EditorPosition>) {
        self.position.set(value);
    }

    pub fn sidebar_width(&self) -> Option<f64> {
        self.sidebar_width.get()
    }

    pub fn set_sidebar_width(&self, value: Option<f64>) {
        self.sidebar_width.set(value);
    }

    pub fn sidebar_hidden(&self) -> bool {
        self.sidebar_hidden.get()
    }

    pub fn set_sidebar_hidden(&self, value: bool) {
        self.sidebar_hidden.set(value);
    }

    pub fn has_sidebar(&self) -> bool {
        self.has_sidebar.get()
    }

    pub fn set_has_sidebar(&self, value: bool) {
        self.has_sidebar.set(value);
    }

    pub fn navigation(&self) -> Option<ModalEditorNavigation> {
        self.navigation.borrow().clone()
    }

    /// O valor das chaves de contexto `editorPartModalNavigation`: `!!navigation && navigation.total > 1`.
    pub fn has_navigation_context(&self) -> bool {
        is_navigation_visible(self.navigation.borrow().as_ref())
    }

    /// O valor de `editorPartModalSidebarVisible`: `hasSidebar && !sidebarHidden`.
    pub fn is_sidebar_visible(&self) -> bool {
        self.has_sidebar.get() && !self.sidebar_hidden.get()
    }

    /// `updateOptions(options?)`.
    pub fn update_options(&self, options: Option<&ModalEditorPartOptions>) {
        if let Some(maximized) = options.and_then(|options| options.maximized) {
            if maximized != self.maximized.get() {
                self.toggle_maximized();
            }
        }

        let navigation = options.and_then(|options| options.navigation.clone());
        *self.navigation.borrow_mut() = navigation.clone();

        self.on_did_change_navigation.fire(navigation);
    }

    /// `toggleMaximized()`.
    pub fn toggle_maximized(&self) {
        self.maximized.set(!self.maximized.get());

        if self.maximized.get() {
            self.saved_size.set(self.size.get());
            self.saved_position.set(self.position.get());
        } else {
            self.size.set(self.saved_size.get());
            self.position.set(self.saved_position.get());
            self.saved_size.set(None);
            self.saved_position.set(None);
        }

        self.on_did_change_maximized.fire(self.maximized.get());
    }

    /// `toggleSidebar()`.
    pub fn toggle_sidebar(&self) {
        self.sidebar_hidden.set(!self.sidebar_hidden.get());

        self.on_did_toggle_sidebar.fire(());
    }

    /// `handleHeaderDoubleClick()`.
    pub fn handle_header_double_click(&self) {
        if self.maximized.get() {
            // Clear saved state so that toggleMaximized restores to default
            self.saved_size.set(None);
            self.saved_position.set(None);
            self.toggle_maximized();
        } else if self.size.get().is_some() {
            self.size.set(None);
            self.position.set(None);
            self.on_did_request_layout.fire(());
        } else {
            self.toggle_maximized(); // maximize
        }
    }
}

impl IDisposable for ModalEditorPartState {
    fn dispose(&self) {
        // ensure to free the reference to the navigation closure
        *self.navigation.borrow_mut() = None;

        self.base.dispose();
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Navegação (botões e rótulo do cabeçalho)
// ---------------------------------------------------------------------------------------------------------------------

/// `navigation && navigation.total > 1`: a navegação só aparece (e só vale como contexto) com mais de um item.
pub fn is_navigation_visible(navigation: Option<&ModalEditorNavigation>) -> bool {
    navigation.is_some_and(|navigation| navigation.total > 1.0)
}

/// `navigation.current > 0`: o botão "anterior" está habilitado.
pub fn can_navigate_previous(navigation: &ModalEditorNavigation) -> bool {
    navigation.current > 0.0
}

/// `navigation.current < navigation.total - 1`: o botão "próximo" está habilitado.
pub fn can_navigate_next(navigation: &ModalEditorNavigation) -> bool {
    navigation.current < navigation.total - 1.0
}

/// O clique em "anterior": navega para `current - 1` se houver.
pub fn navigate_previous(navigation: Option<&ModalEditorNavigation>) {
    if let Some(navigation) = navigation.filter(|navigation| can_navigate_previous(navigation)) {
        (navigation.navigate)(navigation.current - 1.0);
    }
}

/// O clique em "próximo": navega para `current + 1` se houver.
pub fn navigate_next(navigation: Option<&ModalEditorNavigation>) {
    if let Some(navigation) = navigation.filter(|navigation| can_navigate_next(navigation)) {
        (navigation.navigate)(navigation.current + 1.0);
    }
}

/// O rótulo do contador: `localize('navigationCounter', "{0} of {1}", current + 1, total)`.
pub fn navigation_label(navigation: &ModalEditorNavigation) -> String {
    let (position, total) = ((navigation.current + 1.0).to_string(), navigation.total.to_string());
    localize("navigationCounter", "{0} of {1}", &[position.as_str(), total.as_str()])
}

// ---------------------------------------------------------------------------------------------------------------------
// Cabeçalho
// ---------------------------------------------------------------------------------------------------------------------

/// `!!editorModalOptions?.compactHeader`: o editor ativo pede o cabeçalho compacto (a classe `compact-header` do bloco).
pub fn is_compact_header(active_editor: Option<&dyn ModalEditorOptionsProvider>) -> bool {
    active_editor.and_then(|provider| provider.get_modal_editor_options()).and_then(|options| options.compact_header).unwrap_or(false)
}

/// `headerElement.offsetHeight || MODAL_HEADER_HEIGHT`: a altura medida do cabeçalho, ou a constante antes do layout
/// (`offsetHeight` zero).
pub fn resolve_header_height(measured_height: f64) -> f64 {
    if measured_height == 0.0 || measured_height.is_nan() { MODAL_HEADER_HEIGHT } else { measured_height }
}

// ---------------------------------------------------------------------------------------------------------------------
// Layout do modal (`create`)
// ---------------------------------------------------------------------------------------------------------------------

/// `MODAL_MIN_WIDTH + (options?.sidebar ? MODAL_SIDEBAR_MIN_WIDTH : 0)`: a largura mínima do modal.
pub fn effective_min_width(has_sidebar: bool) -> f64 {
    MODAL_MIN_WIDTH + if has_sidebar { MODAL_SIDEBAR_MIN_WIDTH } else { 0.0 }
}

/// `Math.max(containerDimension.height - titleBarOffset, 0)`: a altura abaixo da barra de título.
pub fn available_height(container: EditorSize, title_bar_offset: f64) -> f64 {
    (container.height - title_bar_offset).max(0.0)
}

/// `getDefaultSize()`: o tamanho do modal que não é maximizado nem personalizado.
pub fn default_modal_size(container: EditorSize, title_bar_offset: f64) -> EditorSize {
    let available_height = available_height(container, title_bar_offset);
    let target_width = container.width * 0.8;
    let target_height = available_height * 0.8;
    let width = target_width.min(MODAL_MAX_DEFAULT_WIDTH).min(container.width);
    let height = target_height.min(MODAL_MAX_DEFAULT_HEIGHT).min(available_height);

    EditorSize { width, height }
}

/// O tamanho que o `layoutModal` escolhe: maximizado (com o respiro das bordas), o personalizado limitado ao contêiner,
/// ou o padrão. A altura nunca passa do que há abaixo da barra de título.
pub fn modal_layout_size(container: EditorSize, title_bar_offset: f64, maximized: bool, custom_size: Option<EditorSize>) -> EditorSize {
    let available_height = available_height(container, title_bar_offset);

    let EditorSize { width, height } = if maximized {
        let vertical_padding = title_bar_offset /* keep away from title bar to prevent clipping issues with WCO */.max(MODAL_MAXIMIZED_PADDING);
        EditorSize { width: (container.width - MODAL_MAXIMIZED_PADDING).max(0.0), height: (available_height - vertical_padding).max(0.0) }
    } else if let Some(size) = custom_size {
        EditorSize { width: size.width.min(container.width), height: size.height.min(available_height) }
    } else {
        default_modal_size(container, title_bar_offset)
    };

    // Ensure the modal never exceeds available height (below the title bar)
    EditorSize { width, height: height.min(available_height) }
}

/// A posição que o `layoutModal` escolhe: a personalizada (limitada ao contêiner) quando não maximizado, senão
/// centralizada, mas sempre abaixo da barra de título.
pub fn modal_position(container: EditorSize, title_bar_offset: f64, modal: EditorSize, maximized: bool, custom_position: Option<EditorPosition>) -> EditorPosition {
    match custom_position.filter(|_| !maximized) {
        Some(position) => EditorPosition {
            left: position.left.min(container.width - modal.width).max(0.0),
            top: position.top.min(title_bar_offset + available_height(container, title_bar_offset) - modal.height).max(title_bar_offset),
        },
        None => EditorPosition {
            left: (container.width - modal.width) / 2.0,
            // center in full window, but clamp to stay below the title bar
            top: title_bar_offset.max((container.height - modal.height) / 2.0),
        },
    }
}

/// A posição central do diálogo (`centerLeft` e `centerTop`) usada para encaixar o arrasto.
fn center_position(container: EditorSize, title_bar_offset: f64, dialog: EditorSize) -> EditorPosition {
    EditorPosition { left: (container.width - dialog.width) / 2.0, top: title_bar_offset.max((container.height - dialog.height) / 2.0) }
}

/// O `onPointerMove` do arrasto do cabeçalho: a posição inicial mais o deslocamento do ponteiro, limitada à janela, e
/// encaixada no centro quando chega perto (`MODAL_SNAP_THRESHOLD`).
pub fn drag_position(start: EditorPosition, delta_x: f64, delta_y: f64, container: EditorSize, title_bar_offset: f64, dialog: EditorSize) -> EditorPosition {
    // Clamp to window bounds
    let min_left = 0.0_f64;
    let min_top = title_bar_offset;
    let max_left = min_left.max(container.width - dialog.width);
    let max_top = min_top.max(container.height - dialog.height);

    let new_left = min_left.max(max_left.min(start.left + delta_x));
    let new_top = min_top.max(max_top.min(start.top + delta_y));

    // Snap to center position when close
    let center = center_position(container, title_bar_offset, dialog);
    if (new_left - center.left).abs() < MODAL_SNAP_THRESHOLD && (new_top - center.top).abs() < MODAL_SNAP_THRESHOLD {
        return center;
    }
    EditorPosition { left: new_left, top: new_top }
}

/// O `onStop` do arrasto: a posição a guardar na parte (`None` se encaixou no centro, que limpa a posição personalizada).
pub fn position_after_drag(current: EditorPosition, container: EditorSize, title_bar_offset: f64, dialog: EditorSize) -> Option<EditorPosition> {
    // Check if snapped to center, if so clear custom position
    let center = center_position(container, title_bar_offset, dialog);
    if (current.left - center.left).abs() < 1.0 && (current.top - center.top).abs() < 1.0 { None } else { Some(current) }
}

/// O que `onDidWillResize` guarda quando o redimensionamento começa.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ResizeStart {
    pub left: f64,
    pub top: f64,
    pub size: EditorSize,
}

/// O resultado de [`clamp_resize`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClampedResize {
    pub size: EditorSize,
    pub left: f64,
    pub top: f64,
}

/// O `onDidResize` (sem `done`) do redimensionamento: limita posição e tamanho à janela, mantendo a borda oposta fixa
/// quando a alça é a oeste ou a norte.
pub fn clamp_resize(start: ResizeStart, dimension: EditorSize, west: bool, north: bool, container: EditorSize, title_bar_offset: f64) -> ClampedResize {
    let delta_width = dimension.width - start.size.width;
    let delta_height = dimension.height - start.size.height;

    let mut new_left = if west { start.left - delta_width } else { start.left };
    let mut new_top = if north { start.top - delta_height } else { start.top };
    let mut new_width = dimension.width;
    let mut new_height = dimension.height;

    if new_left < 0.0 {
        new_width += new_left;
        new_left = 0.0;
    }
    if new_top < title_bar_offset {
        new_height += new_top - title_bar_offset;
        new_top = title_bar_offset;
    }
    if new_left + new_width > container.width {
        new_width = container.width - new_left;
    }
    if new_top + new_height > container.height {
        new_height = container.height - new_top;
    }

    ClampedResize { size: EditorSize { width: new_width, height: new_height }, left: new_left, top: new_top }
}

/// Onde a parte de editor fica dentro do modal (o `editorPart.layout(...)` do `layout`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EditorPartLayout {
    pub width: f64,
    pub height: f64,
    pub top: f64,
    pub left: f64,
}

/// O `layout(sizeChanged)` do modal: a área da parte de editor descontadas as bordas, o cabeçalho e a barra lateral. `top`
/// e `left` são os do elemento redimensionável.
pub fn editor_part_layout(modal: EditorSize, top: f64, left: f64, sidebar_width: f64, header_height: f64) -> EditorPartLayout {
    EditorPartLayout {
        width: (modal.width - MODAL_BORDER_SIZE - sidebar_width).max(0.0),
        height: modal.height - MODAL_BORDER_SIZE - header_height,
        top: top + MODAL_BORDER_WIDTH + header_height,
        left: left + MODAL_BORDER_WIDTH + sidebar_width,
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Barra lateral (`createSidebar`)
// ---------------------------------------------------------------------------------------------------------------------

/// `content.sidebarWidth && content.sidebarWidth > 0 ? content.sidebarWidth : MODAL_SIDEBAR_DEFAULT_WIDTH`: a largura com
/// que a barra lateral nasce (a lembrada de outra sessão, se houver).
pub fn initial_sidebar_width(remembered_width: Option<f64>) -> f64 {
    remembered_width.filter(|width| *width > 0.0).unwrap_or(MODAL_SIDEBAR_DEFAULT_WIDTH)
}

/// `Math.max(MODAL_SIDEBAR_MIN_WIDTH, container.clientWidth - MODAL_MIN_WIDTH)`: a largura máxima da barra lateral.
pub fn sidebar_max_width(container_width: f64) -> f64 {
    MODAL_SIDEBAR_MIN_WIDTH.max(container_width - MODAL_MIN_WIDTH)
}

/// O `sash.onDidChange` da barra lateral: a largura de quando o arrasto começou mais o deslocamento, entre o mínimo e o
/// máximo.
pub fn resized_sidebar_width(start_width: f64, delta: f64, container_width: f64) -> f64 {
    sidebar_max_width(container_width).min(MODAL_SIDEBAR_MIN_WIDTH.max(start_width + delta))
}

/// O `sash.onDidReset` da barra lateral: a largura padrão, limitada ao máximo.
pub fn reset_sidebar_width(container_width: f64) -> f64 {
    sidebar_max_width(container_width).min(MODAL_SIDEBAR_DEFAULT_WIDTH)
}

/// O `clampWidth(modalWidth)` da barra lateral: `Some(nova largura)` quando a barra deixaria o editor estreito demais
/// (e então a largura personalizada é esquecida), `None` quando cabe.
pub fn clamped_sidebar_width(sidebar_width: f64, modal_width: f64) -> Option<f64> {
    (sidebar_width + MODAL_MIN_WIDTH > modal_width)
        .then(|| MODAL_SIDEBAR_DEFAULT_WIDTH.min(MODAL_SIDEBAR_MIN_WIDTH.max(modal_width - MODAL_MIN_WIDTH)))
}

/// O `layout(height)` da barra lateral: as dimensões do conteúdo (descontados o respiro e a borda), ou `None` se a
/// barra está escondida.
pub fn sidebar_layout_dimension(height: f64, sidebar_width: f64, visible: bool) -> Option<LayoutDimension> {
    visible.then(|| LayoutDimension {
        height: height - MODAL_SIDEBAR_PADDING * 2.0,
        width: sidebar_width - MODAL_SIDEBAR_PADDING * 2.0 - MODAL_SIDEBAR_BORDER_RIGHT,
    })
}
