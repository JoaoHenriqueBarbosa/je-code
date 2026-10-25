//! O shell do workbench: as partes do VS Code (barra de título com a barra de menus e o command center, activity
//! bar, side bar, grupo de editores com abas e breadcrumbs, e barra de status) numa árvore retida do `je-ui`.
//!
//! As medidas e cores são as do oráculo (o VS Code 1.143.1 com o tema 2026 Dark), lidas do DOM dele por CDP
//! (`oracle-harness/cdp_eval.js`) e conferidas por screenshot. A barra de menus e o explorador são widgets
//! retidos (`je-widgets`); o resto da moldura é remontado por [`Shell::render`] a cada mudança de estado, como
//! o `layout()` das partes do VS Code reescreve o estilo dos elementos. O editor é uma superfície
//! [`Content::Host`] pintada pelo `je-editor-view` na ordem da árvore, então menus abertos ficam por cima dele.

mod explorer;
mod file_icons;
mod find;
mod menus;
mod search_backend;
pub mod scm_view;
mod search_view;
mod excludes;
mod extensions_view;
mod file_ops;
mod input;
pub mod git;
mod quick_pick;
pub mod recent;
mod terminal_panel;
pub mod themes;

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use je_base::key_codes::KeyCode;
use je_base::lifecycle::IDisposable;
use je_editor::cursor_common::CursorSimpleModel;
use je_editor::model::EndOfLinePreference;
use je_editor::text_model::{TextModel, TextSource};
use je_editor::undo_redo_service::UndoRedoService;
use je_editor_view::environment::find_builtin_extensions;
use je_editor_view::{DiffColors, DiffEditor, EditorColors, EditorEnvironment, EditorKey, EditorView, GutterChange, GutterKind, Side};
use je_render::kurbo::{Rect, RoundedRect, RoundedRectRadii};
use je_render::{DisplayList, Rgba, Shape};
use je_theme::ColorTheme;
use je_ui::{Border, Content, Cursor, EventKind, KeyData, Modifiers, NodeId, PointerButton, PointerInput, PointerInputKind, Role, Tree, Ui};
use je_widgets::menu::default_menu_styles;
use je_widgets::menu_bar::{IMenuBarOptions, MenuBar, default_menu_bar_styles};
use je_widgets::text::{measure_text, ui_font};
use web_time::Instant;

use explorer::Explorer;
use file_icons::FileIconTheme;
use find::FindWidget;
use menus::CommandQueue;
use quick_pick::{PickItem, QuickPick};
use scm_view::ScmView;
use search_view::SearchView;

/// O tema padrão relativo à pasta de extensões embutidas.
pub const BUILTIN_DEFAULT_THEME: &str = "theme-defaults/themes/2026-dark.json";

/// Os contêineres de vista da barra lateral (a activity bar).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SideView {
    Explorer,
    Search,
    SourceControl,
    Debug,
    Extensions,
}

impl SideView {
    pub const ALL: [SideView; 5] = [Self::Explorer, Self::Search, Self::SourceControl, Self::Debug, Self::Extensions];

    fn title(self) -> &'static str {
        match self {
            Self::Explorer => "Explorer",
            Self::Search => "Search",
            Self::SourceControl => "Source Control",
            Self::Debug => "Run and Debug",
            Self::Extensions => "Extensions",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Self::Explorer => "files",
            Self::Search => "search",
            Self::SourceControl => "source-control",
            Self::Debug => "debug-alt",
            Self::Extensions => "extensions",
        }
    }
}

/// O pedido de dica pendente: geração, nó alvo, texto e posição.
type HoverRequest = Rc<RefCell<Option<(u64, NodeId, String, je_widgets::hover_api::HoverPosition)>>>;

/// A gravidade de uma notificação (`Severity`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
// `Info` e `Warning` são as gravidades que o serviço de notificações do workbench vai usar (extensões, git).
#[allow(dead_code)]
pub enum Severity {
    Info,
    Warning,
    Error,
}

/// Uma notificação na tela (`NotificationsToasts`).
struct Toast {
    id: u64,
    severity: Severity,
    message: String,
}

/// As abas do painel inferior.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PanelTab {
    Problems,
    Output,
    DebugConsole,
    Terminal,
    Ports,
}

impl PanelTab {
    const ALL: [PanelTab; 5] = [Self::Problems, Self::Output, Self::DebugConsole, Self::Terminal, Self::Ports];

    fn title(self) -> &'static str {
        match self {
            Self::Problems => "Problems",
            Self::Output => "Output",
            Self::DebugConsole => "Debug Console",
            Self::Terminal => "Terminal",
            Self::Ports => "Ports",
        }
    }

    /// O texto do painel vazio, o de cada vista do VS Code sem conteúdo.
    fn empty_message(self) -> &'static str {
        match self {
            Self::Problems => "No problems have been detected in the workspace.",
            Self::Output => "",
            Self::DebugConsole => "",
            Self::Terminal => "",
            Self::Ports => "No forwarded ports. Forward a port to access your running services locally.",
        }
    }
}

/// O que um prompt de nome faz com o texto digitado.
#[derive(Clone, Debug, PartialEq)]
pub enum PromptAction {
    NewFile(PathBuf),
    NewFolder(PathBuf),
    Rename(PathBuf),
    OpenFile,
    OpenFolder,
    SaveAs(usize),
    GoToLine,
}

/// O que os menus, botões e atalhos pedem ao shell. Roda depois do evento que o gerou, fora dos callbacks.
#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    NewUntitled,
    OpenFile(PathBuf),
    /// Abre como pré-visualização (o clique único do explorador e do Quick Open em modo preview).
    PreviewFile(PathBuf),
    PinEditor(usize),
    Save,
    SaveAll,
    Revert,
    CloseActiveEditor,
    CloseEditor(usize),
    SplitEditor,
    FocusGroup(usize),
    /// Clique numa aba de um grupo fora de foco: foca o grupo e ativa a aba.
    FocusGroupEditor(usize, usize),
    TabContextMenu(usize),
    CloseOthers(usize),
    CloseToTheRight(usize),
    CloseSaved,
    CloseAll,
    /// Confere o disco: arquivos abertos mudados por fora e a árvore do explorador.
    CheckDisk,
    RevealInExplorer(PathBuf),
    SaveAndClose(usize),
    DiscardAndClose(usize),
    CancelExit,
    CloseDialog,
    ActivateEditor(usize),
    NextEditor,
    PreviousEditor,
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    SelectAll,
    ToggleSidebar,
    ToggleStatusBar,
    ToggleFullScreen,
    ToggleWordWrap,
    ShowExplorer,
    ShowManageMenu,
    ToggleTimeline,
    PickTheme,
    SetTheme(usize),
    OpenSettingsJson,
    OpenKeybindingsJson,
    PickEol,
    PickIndentation,
    /// `Indent Using Spaces` (true) ou `Tabs` (false): pede o tamanho em seguida.
    PickIndentSize(bool),
    SetIndentation(bool, i32),
    DetectIndentation,
    ToggleMaximizedPanel,
    SetEol(bool),
    PickLanguage,
    SetLanguage(String),
    /// O botão direito num item do explorador.
    ExplorerContextMenu(PathBuf),
    ShowView(SideView),
    ShowSearch,
    SearchFocus,
    SearchTick(u64),
    SearchNow,
    SearchClear,
    SearchCollapse,
    SearchToggleCase,
    SearchToggleWord,
    SearchToggleRegex,
    SearchToggleReplace,
    ShowSearchReplace,
    SearchFocusReplace,
    SearchReplaceAll,
    ShowScm,
    ShowExtensions,
    OpenRecent,
    /// `workbench.action.zoomIn`/`zoomOut`/`zoomReset`: muda o `window.zoomLevel` em um passo.
    Zoom(i32),
    AutoFetch,
    /// O `files.autoSave: afterDelay` venceu o prazo para a versão dada da aba.
    AutoSave(usize, i64),
    /// Mostra a dica do nó depois da espera (`workbench.hover.delay`), se o ponteiro ainda estiver nele.
    ShowHover(u64),
    HideHover,
    OpenFolderPath(PathBuf),
    ExtensionsFocus,
    ScmFocus,
    ScmCommit,
    ScmRefresh,
    ScmOpen(String),
    /// Clicar num arquivo da vista Source Control: abre o diff dele (o arquivo, se não há o que comparar).
    ScmOpenChanges(scm_view::Group, String),
    ScmContextMenu(scm_view::Entry),
    ScmMoreMenu,
    ScmOp(git::Operation),
    DismissToast(u64),
    /// Abre o arquivo na linha e seleciona de `column` a `end_column`.
    OpenAt(PathBuf, i32, i32, i32),
    /// Um nome digitado no prompt (Enter) para a ação dele.
    Prompt(PromptAction, String),
    DeletePath(PathBuf),
    RevealInFileManager(PathBuf),
    OpenInTerminal(PathBuf),
    CopyPath(PathBuf, bool),
    CloseContextMenu,
    PromptOpenFile,
    PromptOpenFolder,
    PromptSaveAs,
    PromptGoToLine,
    PromptNewFile(PathBuf),
    PromptNewFolder(PathBuf),
    PromptRename(PathBuf),
    ConfirmDelete(PathBuf),
    QuickOpen,
    ShowCommands,
    TogglePanel,
    ShowPanelTab(PanelTab),
    KillTerminal,
    ToggleTerminal,
    NewTerminal,
    Find,
    FindNext,
    FindPrevious,
    FindClose,
    FindToggleCase,
    FindToggleWord,
    FindToggleRegex,
    FindToggleReplace,
    /// Ctrl+H: abre a busca com a linha de substituição.
    FindAndReplace,
    FindFocusReplace,
    FindReplace,
    FindReplaceAll,
    FindInSelection,
    /// Aceita a linha de índice dado do Quick Open ou da paleta.
    QuickPickAccept(usize),
    Exit,
}

/// As medidas fixas das partes no tema 2026 (pixels lógicos), do DOM do oráculo.
mod metrics {
    pub const TITLE_HEIGHT: f64 = 35.0;
    pub const STATUS_HEIGHT: f64 = 28.0;
    /// A activity bar flutuante: de x = 4 a 48, com 4 px de folga até a barra de status.
    pub const ACTIVITY_LEFT: f64 = 4.0;
    pub const ACTIVITY_WIDTH: f64 = 44.0;
    /// O espaço entre a borda de baixo das partes e a barra de status.
    pub const PART_BOTTOM_GAP: f64 = 4.0;
    pub const SASH: f64 = 4.0;
    pub const DEFAULT_SIDEBAR_WIDTH: f64 = 300.0;
    pub const TABS_HEIGHT: f64 = 32.0;
    pub const BREADCRUMBS_HEIGHT: f64 = 22.0;
    pub const RADIUS: f64 = 8.0;
    pub const DEFAULT_PANEL_HEIGHT: f64 = 258.0;
    pub const PANEL_TITLE_HEIGHT: f64 = 32.0;
}

use metrics::*;

/// Um editor aberto (uma aba).
pub struct OpenEditor {
    pub path: Option<PathBuf>,
    pub name: String,
    pub language_name: String,
    pub view: EditorView,
    /// A versão do modelo no último salvamento (para o indicador de arquivo modificado).
    saved_version: i64,
    /// As linhas do arquivo no HEAD do git (para as barras de diff da margem) e a versão do modelo calculada.
    head: Option<Vec<je_base::JsString>>,
    diff_version: i64,
    /// A data de modificação do arquivo no disco quando foi lido ou gravado (para notar mudanças de fora).
    disk_mtime: Option<std::time::SystemTime>,
    /// A aba de pré-visualização (`workbench.editor.enablePreview`): título em itálico, trocada pela próxima
    /// aberta do mesmo jeito; editar o texto ou clicar duas vezes a fixa.
    preview: bool,
    /// O editor de diff lado a lado quando a aba compara o arquivo com a versão do git; o `view` é o modificado.
    diff: Option<DiffEditor>,
}

impl OpenEditor {
    pub fn is_modified(&self) -> bool {
        self.view.model().get_alternative_version_id() != self.saved_version
    }

    /// O editor de diff da aba, se ela compara o arquivo com o git.
    #[cfg(test)]
    pub fn diff(&self) -> Option<&DiffEditor> {
        self.diff.as_ref()
    }

    /// Ajusta o editor (e o diff, se for o caso) à área `width` x `height`.
    fn resize(&mut self, width: f64, height: f64) {
        match &mut self.diff {
            Some(diff) => diff.resize(&mut self.view, width, height),
            None => self.view.resize(width, height),
        }
    }

    fn paint(&mut self, list: &mut DisplayList, origin: (f64, f64), scale: f64, now: Instant) {
        match &mut self.diff {
            Some(diff) => diff.paint(&mut self.view, list, origin, scale, now),
            None => self.view.paint(list, origin, scale, now),
        }
    }

    fn mouse_down(&mut self, x: f64, y: f64, input: &PointerInput) {
        match &mut self.diff {
            Some(diff) => diff.mouse_down(&mut self.view, x, y, input.button, input.click_count, input.modifiers),
            None => self.view.mouse_down(x, y, input.button, input.click_count, input.modifiers),
        }
    }

    fn mouse_drag(&mut self, x: f64, y: f64) {
        match &mut self.diff {
            Some(diff) => diff.mouse_drag(&mut self.view, x, y),
            None => self.view.mouse_drag(x, y),
        }
    }

    fn mouse_up(&mut self) {
        match &mut self.diff {
            Some(diff) => diff.mouse_up(&mut self.view),
            None => self.view.mouse_up(),
        }
    }

    fn mouse_move(&mut self, x: f64, y: f64) {
        match &mut self.diff {
            Some(diff) => diff.mouse_move(&mut self.view, x, y),
            None => self.view.mouse_move(x, y),
        }
    }

    fn scroll_by(&mut self, x: f64, dx: f64, dy: f64) {
        match &mut self.diff {
            Some(diff) => diff.scroll_by(&mut self.view, x, dx, dy),
            None => self.view.scroll_by(dx, dy),
        }
    }

    fn key_down(&mut self, key: EditorKey, mods: Modifiers) -> bool {
        match &mut self.diff {
            Some(diff) => diff.key_down(&mut self.view, key, mods),
            None => self.view.key_down(key, mods),
        }
    }

    fn type_text(&mut self, text: &str) {
        match &mut self.diff {
            Some(diff) => diff.type_text(&mut self.view, text),
            None => self.view.type_text(text),
        }
    }

    /// A view que recebe desfazer e a área de transferência: o lado focado do diff, ou o próprio editor.
    fn focused_view_mut(&mut self) -> &mut EditorView {
        match &mut self.diff {
            Some(diff) if diff.focused_side() == Side::Original => diff.original_mut(),
            _ => &mut self.view,
        }
    }

    /// O próximo comando de atalho do `keybindings.json` que não é do editor, do lado que recebeu a tecla.
    fn take_workbench_command(&mut self) -> Option<(String, je_base::dynamic::Dynamic)> {
        let diff = self.diff.as_mut().map(DiffEditor::original_mut);
        self.view.take_workbench_command().or_else(|| diff.and_then(EditorView::take_workbench_command))
    }
}

/// Um grupo de editores fora de foco (o grupo em foco vive em `Shell::editors` e `Shell::active`).
pub struct Group {
    editors: Vec<OpenEditor>,
    active: Option<usize>,
}

/// Os nós retidos e os que [`Shell::render`] remonta.
struct Nodes {
    /// Remontado a cada `render`: tudo menos a barra de menus e o explorador.
    chrome: NodeId,
    menubar: NodeId,
    explorer: NodeId,
    search: NodeId,
    scm: NodeId,
    /// A superfície do editor ativo dentro de `chrome` (muda a cada `render`).
    editor_host: Option<NodeId>,
}

pub struct Shell {
    ui: Ui,
    theme: ColorTheme,
    environment: Option<Rc<EditorEnvironment>>,
    icons: Option<FileIconTheme>,
    queue: CommandQueue,
    menubar: Rc<MenuBar>,
    explorer: Option<Explorer>,
    side_view: SideView,
    search: SearchView,
    scm: ScmView,
    scm_focused: bool,
    git_root: Option<PathBuf>,
    /// A Timeline aberta e as entradas dela (assunto e data relativa) para o arquivo ativo.
    timeline_open: bool,
    timeline_entries: Vec<(String, String)>,
    timeline_path: Option<PathBuf>,
    /// As extensões instaladas (lidas na primeira abertura da vista Extensions) e o filtro e a rolagem dela.
    extensions: Option<Vec<extensions_view::ExtensionEntry>>,
    extensions_filter: je_widgets::text_field::TextField,
    extensions_scroll: f64,
    extensions_focused: bool,
    git_rx: Option<std::sync::mpsc::Receiver<Result<git::Status, String>>>,
    search_focused: bool,
    search_generation: u64,
    nodes: Nodes,
    editors: Vec<OpenEditor>,
    active: Option<usize>,
    /// Os grupos da esquerda para a direita; a vaga do grupo em foco fica `None` (o conteúdo dele está em
    /// `editors` e `active`).
    groups: Vec<Option<Group>>,
    group_index: usize,
    workspace: Option<PathBuf>,
    sidebar_visible: bool,
    sidebar_width: f64,
    statusbar_visible: bool,
    size: (f64, f64),
    /// O caminho de pintura, para a barra de status (`GPU ...` ou `CPU ...`).
    backend: String,
    /// Falha da última operação de arquivo, mostrada na barra de status.
    toasts: Vec<Toast>,
    next_toast_id: u64,
    /// O botão do mouse foi apertado no editor e o arraste segue para ele até soltar.
    editor_drag: bool,
    /// O arraste do sash da side bar: a largura no início e o x do ponteiro.
    sash_drag: Option<(f64, f64)>,
    /// O índice da aba sob o ponteiro (mostra o botão de fechar).
    hovered_tab: Rc<RefCell<Option<usize>>>,
    exit_requested: bool,
    /// Saindo: cada aba modificada pergunta antes; termina quando não sobra nenhuma.
    exiting: bool,
    /// O Save As veio do `Save` de um fechamento: a aba fecha depois de gravar.
    close_after_save_as: bool,
    /// A dica (hover) na tela e o pedido pendente: geração, nó alvo, texto e posição.
    hover: Option<Rc<je_widgets::hover_widget::WorkbenchHoverWidget>>,
    context_view: Option<Rc<je_widgets::context_view::ContextView>>,
    hover_request: HoverRequest,
    /// O diálogo modal aberto: enquanto existe, teclado e ponteiro vão só para ele.
    dialog: Option<Rc<je_widgets::dialog::Dialog>>,
    fullscreen_requested: bool,
    needs_render: bool,
    /// O `window.zoomLevel` (cada nível é 20%).
    zoom_level: f64,
    /// O `files.autoSave` (`off`, `afterDelay`, `onFocusChange`, `onWindowChange`) e o `files.autoSaveDelay`.
    auto_save: (String, u64),
    /// A versão de cada aba já agendada para salvar (evita um timer por tecla).
    auto_save_scheduled: Option<(usize, i64)>,
    /// Pede um quadro sem remontar a moldura.
    redraw: bool,
    quick_pick: Option<QuickPick>,
    /// O menu de contexto aberto (o widget `Menu`) e o nó onde ele pinta.
    context_menu: Option<(Rc<je_widgets::menu::Menu>, NodeId)>,
    /// A última posição do ponteiro (onde o menu de contexto abre).
    last_pointer: (f64, f64),
    find: Option<FindWidget>,
    panel_visible: bool,
    panel_tab: PanelTab,
    panel_height: f64,
    /// A altura de antes de maximizar o painel.
    panel_restore_height: Option<f64>,
    terminal: Option<je_terminal::Terminal>,
    terminal_focused: bool,
    /// A pasta em que o próximo terminal abre (`Open in Integrated Terminal`).
    workspace_for_terminal: Option<PathBuf>,
    terminal_palette: je_terminal::Palette,
    /// Acorda o laço de eventos da janela (o pty do terminal chama quando há saída).
    wake: je_terminal::Wake,
    sash_panel_drag: Option<(f64, f64)>,
    /// O campo da busca tem o foco do teclado.
    find_focused: bool,
    /// O último valor da busca, para reabrir o widget com ele.
    last_find: Option<String>,
    palette_items: Vec<PickItem>,
    themes: Vec<themes::ThemeEntry>,
    theme_index: Option<usize>,
    /// O arquivo e o `uiTheme` do tema em uso.
    theme_file: (PathBuf, String),
    /// O tema de antes da pré-visualização do seletor (Esc volta a ele).
    theme_before_preview: Option<(PathBuf, String, Option<usize>)>,
    /// Os arquivos do workspace, lidos na primeira abertura do Quick Open.
    workspace_files: Option<Vec<PickItem>>,
    language_of: explorer::LanguageOf,
}

fn color_or(theme: &ColorTheme, id: &str, fallback: Rgba) -> Rgba {
    theme.get_color(id).unwrap_or(fallback)
}

impl Shell {
    /// Abre o workbench com a pasta `workspace` no explorador (se houver) e `files` em abas.
    pub fn open(theme_path: &Path, workspace: Option<PathBuf>, files: &[PathBuf], width: f64, height: f64) -> Result<Self, String> {
        let theme = ColorTheme::load(theme_path).map_err(|e| format!("tema inválido: {e}"))?;
        let extensions = find_builtin_extensions();
        let environment = match &extensions {
            Some(dir) => match EditorEnvironment::load(dir, theme_path) {
                Ok(env) => Some(Rc::new(env)),
                Err(e) => {
                    eprintln!("je-code: sem linguagens ({}): {e}", dir.display());
                    None
                }
            },
            None => {
                eprintln!("je-code: pasta de extensões não encontrada (defina JE_BUILTIN_EXTENSIONS); abrindo sem cores de token");
                None
            }
        };
        let icons = extensions.as_deref().and_then(|dir| match FileIconTheme::load(dir) {
            Ok(icons) => Some(icons),
            Err(e) => {
                eprintln!("je-code: sem ícones de arquivo: {e}");
                None
            }
        });
        let ui = Ui::new();
        let root = ui.root();
        let (chrome, menubar_node, explorer_node, search_node, scm_node) = {
            let mut tree = ui.tree_mut();
            tree.set_bounds(root, Rect::new(0.0, 0.0, width, height));
            let chrome = tree.create(root, Role::Generic);
            let explorer = tree.create(root, Role::Tree);
            let search = tree.create(root, Role::Tree);
            let scm = tree.create(root, Role::Tree);
            let menubar = tree.create(root, Role::MenuBar);
            tree.add_class(menubar, "menubar");
            (chrome, menubar, explorer, search, scm)
        };
        let queue: CommandQueue = Rc::new(RefCell::new(Vec::new()));
        let (menubar, mut palette_items) = Self::build_menubar(&ui, menubar_node, &theme, &queue);
        for (label, keybinding, command) in [
            ("Preferences: Color Theme", Some("Ctrl+K Ctrl+T"), Command::PickTheme),
            ("Preferences: Open User Settings (JSON)", Some("Ctrl+,"), Command::OpenSettingsJson),
            ("Preferences: Open Keyboard Shortcuts (JSON)", None, Command::OpenKeybindingsJson),
        ] {
            palette_items.push(PickItem { label: label.to_owned(), description: None, keybinding: keybinding.map(str::to_owned), command, recent: false });
        }
        let themes = extensions.as_deref().map(themes::catalog).unwrap_or_default();
        let theme_index = themes.iter().position(|t| t.path == theme_path);
        let language_of: explorer::LanguageOf = match &environment {
            Some(env) => {
                let env = env.clone();
                Rc::new(move |name: &str| Some(env.language_id_for(name, None)))
            }
            None => Rc::new(|_: &str| None),
        };
        let user_settings = themes::user_settings();
        let explorer = workspace.as_ref().map(|root| Explorer::new(&ui, explorer_node, root, &theme, icons.clone(), language_of.clone(), queue.clone(), excludes::Excludes::files(&user_settings).matcher()));
        let search = SearchView::new(&ui, search_node, &theme, icons.clone(), language_of.clone(), queue.clone());
        let scm = ScmView::new(&ui, scm_node, &theme, icons.clone(), language_of.clone(), queue.clone());
        let terminal_palette = terminal_panel::palette(&theme);
        let mut shell = Self {
            ui,
            theme,
            environment,
            icons,
            queue,
            menubar,
            explorer,
            side_view: SideView::Explorer,
            search,
            scm,
            scm_focused: false,
            git_root: None,
            timeline_open: false,
            timeline_entries: Vec::new(),
            timeline_path: None,
            extensions: None,
            extensions_filter: je_widgets::text_field::TextField::new(),
            extensions_scroll: 0.0,
            extensions_focused: false,
            git_rx: None,
            search_focused: false,
            search_generation: 0,
            nodes: Nodes { chrome, menubar: menubar_node, explorer: explorer_node, search: search_node, scm: scm_node, editor_host: None },
            editors: Vec::new(),
            active: None,
            groups: vec![None],
            group_index: 0,
            sidebar_visible: workspace.is_some(),
            sidebar_width: DEFAULT_SIDEBAR_WIDTH,
            workspace,
            statusbar_visible: true,
            size: (width, height),
            backend: String::new(),
            toasts: Vec::new(),
            next_toast_id: 0,
            editor_drag: false,
            sash_drag: None,
            hovered_tab: Rc::new(RefCell::new(None)),
            exit_requested: false,
            exiting: false,
            close_after_save_as: false,
            dialog: None,
            hover: None,
            hover_request: Rc::new(RefCell::new(None)),
            context_view: None,
            fullscreen_requested: false,
            needs_render: true,
            zoom_level: 0.0,
            auto_save: ("off".to_owned(), 1000),
            auto_save_scheduled: None,
            redraw: false,
            quick_pick: None,
            context_menu: None,
            last_pointer: (0.0, 0.0),
            find: None,
            panel_visible: false,
            panel_tab: PanelTab::Terminal,
            panel_height: DEFAULT_PANEL_HEIGHT,
            panel_restore_height: None,
            terminal: None,
            terminal_focused: false,
            workspace_for_terminal: None,
            terminal_palette,
            wake: std::sync::Arc::new(|| {}),
            sash_panel_drag: None,
            find_focused: false,
            last_find: None,
            palette_items,
            themes,
            theme_index,
            theme_file: (theme_path.to_path_buf(), "vs-dark".to_owned()),
            theme_before_preview: None,
            workspace_files: None,
            language_of,
        };
        if let Some(entry) = shell.theme_index.and_then(|i| shell.themes.get(i)) {
            shell.theme_file.1 = entry.ui_theme.clone();
        }
        shell.detect_git();
        shell.schedule_disk_check();
        shell.apply_user_settings();
        for file in files {
            shell.open_file(file)?;
        }
        if shell.editors.is_empty() && shell.workspace.is_none() {
            shell.new_untitled();
        }
        shell.render();
        Ok(shell)
    }

    /// A barra de menus do tema dado (e as entradas da paleta que vêm dos itens).
    fn build_menubar(ui: &Ui, node: NodeId, theme: &ColorTheme, queue: &CommandQueue) -> (Rc<MenuBar>, Vec<PickItem>) {
        let (menus, get_keybinding, palette_items) = menus::menubar_menus(queue);
        let bar_options = IMenuBarOptions {
            enable_mnemonics: true,
            visibility: Some("classic".to_owned()),
            get_keybinding: Some(get_keybinding),
            styles: Some(Rc::new(default_menu_bar_styles(theme))),
            ..Default::default()
        };
        let menubar = MenuBar::new(ui, node, bar_options, default_menu_styles(theme));
        menubar.push(&menus);
        (menubar, palette_items)
    }

    /// Troca o tema de cor: o editor, os tokens, o terminal, o explorador e a barra de menus.
    fn apply_theme(&mut self, index: usize) {
        let Some(entry) = self.themes.get(index).cloned() else { return };
        self.apply_theme_file(&entry.path, &entry.ui_theme, Some(index));
    }

    fn apply_theme_file(&mut self, path: &Path, ui_theme: &str, index: Option<usize>) {
        let theme = match ColorTheme::load(path) {
            Ok(theme) => theme,
            Err(e) => {
                self.notify(Severity::Error, format!("tema inválido: {e}"));
                return;
            }
        };
        if let Some(env) = &self.environment
            && let Err(e) = env.set_theme(path, ui_theme)
        {
            self.notify(Severity::Error, format!("tema de tokens inválido: {e}"));
        }
        self.theme = theme;
        self.theme_index = index;
        self.theme_file = (path.to_path_buf(), ui_theme.to_owned());
        let colors = EditorColors::from_theme(&self.theme);
        let token_colors = self.environment.as_ref().map(|env| env.color_map());
        let diff_colors = DiffColors::from_theme(&self.theme);
        for editor in self.all_editors_mut() {
            match &mut editor.diff {
                Some(diff) => diff.set_colors(&mut editor.view, colors.clone(), diff_colors.clone()),
                None => editor.view.set_colors(colors.clone()),
            }
            if let Some(map) = &token_colors {
                editor.view.set_token_colors(map);
                if let Some(diff) = &mut editor.diff {
                    diff.set_token_colors(map);
                }
            }
        }
        self.terminal_palette = terminal_panel::palette(&self.theme);
        if let Some(explorer) = &self.explorer {
            explorer.restyle(&self.theme);
        }
        self.search.restyle(&self.theme);
        self.scm.restyle(&self.theme);
        // A barra de menus guarda as cores na criação: recria no mesmo nó.
        IDisposable::dispose(&*self.menubar);
        let children = self.ui.tree().children(self.nodes.menubar).to_vec();
        for child in children {
            self.ui.remove(child);
        }
        let (menubar, _) = Self::build_menubar(&self.ui, self.nodes.menubar, &self.theme, &self.queue);
        self.menubar = menubar;
        self.needs_render = true;
    }

    /// Abre o seletor de tema (`Preferences: Color Theme`) com a pré-visualização ao navegar.
    fn open_theme_picker(&mut self) {
        let items = self.themes.iter().enumerate().map(|(i, t)| PickItem { label: t.label.clone(), description: None, keybinding: None, command: Command::SetTheme(i), recent: false }).collect();
        let mut pick = QuickPick::list(items, "Select Color Theme (Up/Down Keys to Preview)");
        // Começa no tema atual.
        if let Some(current) = self.theme_index {
            pick.move_focus(current as isize);
        }
        self.theme_before_preview = Some((self.theme_file.0.clone(), self.theme_file.1.clone(), self.theme_index));
        self.quick_pick = Some(pick);
        self.ui.blur();
    }

    /// Com o seletor de tema aberto, mostra o tema do item em foco.
    fn preview_theme(&mut self) {
        if let Some(Command::SetTheme(index)) = self.quick_pick.as_ref().and_then(QuickPick::focused_command)
            && self.theme_index != Some(index)
        {
            self.apply_theme(index);
        }
    }

    fn color(&self, id: &str) -> Rgba {
        color_or(&self.theme, id, Rgba::opaque(0xff, 0x00, 0xff))
    }

    pub fn set_backend(&mut self, backend: String) {
        self.backend = backend;
        self.needs_render = true;
    }

    /// Os caminhos selecionados no explorador (para os testes).
    #[cfg(test)]
    pub fn explorer_selection(&self) -> Vec<PathBuf> {
        self.explorer.as_ref().map(Explorer::selection).unwrap_or_default()
    }

    pub fn active_editor(&self) -> Option<&OpenEditor> {
        self.active.map(|i| &self.editors[i])
    }

    pub fn active_editor_mut(&mut self) -> Option<&mut OpenEditor> {
        self.active.map(|i| &mut self.editors[i])
    }

    /// O título da janela (`window.title`: `${dirty}${activeEditorShort}${separator}${rootName}${separator}${appName}`).
    pub fn window_title(&self) -> String {
        let mut parts = Vec::new();
        if let Some(editor) = self.active_editor() {
            parts.push(format!("{}{}", if editor.is_modified() { "\u{25CF} " } else { "" }, editor.name));
        }
        if let Some(name) = self.workspace_name() {
            parts.push(name);
        }
        parts.push("je-code".to_owned());
        parts.join(" - ")
    }

    fn workspace_name(&self) -> Option<String> {
        self.workspace.as_ref().map(|p| p.file_name().map_or_else(|| p.display().to_string(), |n| n.to_string_lossy().into_owned()))
    }

    fn model_for(&self, file_name: &str, text: &str) -> (Rc<TextModel>, String) {
        match &self.environment {
            Some(env) => {
                let model = env.open(file_name, text);
                let name = env.language_name(&model.get_language_id());
                (model, name)
            }
            None => {
                let mut options = TextModel::default_creation_options();
                options.detect_indentation = true;
                (TextModel::new(TextSource::from(text), "plaintext", options, None, UndoRedoService::for_tests()), "Plain Text".to_owned())
            }
        }
    }

    fn push_editor(&mut self, path: Option<PathBuf>, name: String, text: &str) {
        let (model, language_name) = self.model_for(&name, text);
        let rect = self.editor_rect();
        let mut view = EditorView::new(model.clone(), rect.width(), rect.height(), EditorColors::from_theme(&self.theme));
        if let Some(env) = &self.environment {
            view.set_token_colors(&env.color_map());
        }
        view.apply_configuration(&themes::user_settings());
        let saved_version = model.get_alternative_version_id();
        let index = self.active.map_or(self.editors.len(), |i| i + 1);
        let needs_head = path.is_some();
        self.editors.insert(index, OpenEditor { path, name, language_name, view, saved_version, head: None, diff_version: -1, disk_mtime: None, preview: false, diff: None });
        if needs_head {
            self.reload_heads();
        }
        self.activate(index);
    }

    /// Abre `path` numa aba (ou ativa a aba que já o mostra).
    pub fn open_file(&mut self, path: &Path) -> Result<(), String> {
        if let Some(index) = self.editors.iter().position(|e| e.diff.is_none() && e.path.as_deref() == Some(path)) {
            self.activate(index);
            return Ok(());
        }
        let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let name = path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
        self.push_editor(Some(path.to_path_buf()), name, &String::from_utf8_lossy(&bytes));
        if let Some(editor) = self.active_editor_mut() {
            editor.disk_mtime = Self::mtime(path);
        }
        Ok(())
    }

    /// Abre `path` como pré-visualização: troca a aba de pré-visualização que houver (se ela não foi editada).
    fn preview_file(&mut self, path: &Path) -> Result<(), String> {
        if self.editors.iter().any(|e| e.path.as_deref() == Some(path)) {
            return self.open_file(path);
        }
        let old = self.editors.iter().position(|e| e.preview && !e.is_modified());
        self.open_file(path)?;
        if let Some(active) = self.active {
            self.editors[active].preview = true;
            if let Some(old) = old.filter(|o| *o != active) {
                self.close_editor(old);
            }
        }
        Ok(())
    }

    /// O editor de diff do `original` contra o `modified` (o modelo da aba), nas medidas e cores do shell.
    fn new_diff(&self, original: Rc<TextModel>, modified: &mut EditorView) -> DiffEditor {
        let rect = self.editor_rect();
        let mut diff = DiffEditor::new(original, modified, rect.width(), rect.height(), EditorColors::from_theme(&self.theme), DiffColors::from_theme(&self.theme));
        if let Some(env) = &self.environment {
            diff.set_token_colors(&env.color_map());
        }
        diff.original_mut().apply_configuration(&themes::user_settings());
        diff
    }

    /// Abre o diff de um arquivo da vista Source Control: a árvore de trabalho contra o índice (`arquivo (Working Tree)`,
    /// o lado direito é o arquivo, editável) ou o índice contra o HEAD (`arquivo (Index)`, somente leitura). Os arquivos
    /// sem o que comparar (novos e em conflito) abrem como arquivo.
    fn open_changes(&mut self, group: scm_view::Group, relative: &str) {
        let Some(root) = self.git_root.clone() else { return };
        let full = root.join(relative);
        let staged = group == scm_view::Group::Staged;
        let kind = self.scm.changes_of(group).into_iter().find(|c| c.path == relative).map(|c| c.kind);
        if matches!(kind, Some(git::Kind::Untracked | git::Kind::Conflict)) || (!staged && !full.is_file()) {
            if full.is_file() {
                let _ = self.open_file(&full);
            }
            return;
        }
        let file_name = full.file_name().map_or_else(|| relative.to_owned(), |n| n.to_string_lossy().into_owned());
        let title = format!("{file_name} ({})", if staged { "Index" } else { "Working Tree" });
        if let Some(index) = self.editors.iter().position(|e| e.diff.is_some() && e.name == title && (staged || e.path.as_deref() == Some(full.as_path()))) {
            self.activate(index);
            return;
        }
        let index_text = git::index_text(&root, relative);
        let (original_text, modified_text) = if staged {
            (git::head_text(&root, relative).unwrap_or_default(), index_text.unwrap_or_default())
        } else {
            let Ok(bytes) = std::fs::read(&full) else { return };
            (index_text.or_else(|| git::head_text(&root, relative)).unwrap_or_default(), String::from_utf8_lossy(&bytes).into_owned())
        };
        // O arquivo já aberto empresta o modelo: o diff e a aba comum editam o mesmo texto.
        let shared = (!staged).then(|| self.editors.iter().find(|e| e.diff.is_none() && e.path.as_deref() == Some(full.as_path()))).flatten().map(|e| (e.view.model().clone(), e.language_name.clone(), e.saved_version));
        let (model, language_name, saved_version) = match shared {
            Some(shared) => shared,
            None => {
                let (model, language_name) = self.model_for(&file_name, &modified_text.replace("\r\n", "\n"));
                let version = model.get_alternative_version_id();
                (model, language_name, version)
            }
        };
        let (original, _) = self.model_for(&file_name, &original_text);
        let rect = self.editor_rect();
        let mut view = EditorView::new(model, rect.width(), rect.height(), EditorColors::from_theme(&self.theme));
        if let Some(env) = &self.environment {
            view.set_token_colors(&env.color_map());
        }
        view.apply_configuration(&themes::user_settings());
        if staged {
            view.set_read_only(true);
        }
        let diff = self.new_diff(original, &mut view);
        let editor = OpenEditor { path: (!staged).then(|| full.clone()), name: title, language_name, view, saved_version, head: None, diff_version: -1, disk_mtime: (!staged).then(|| Self::mtime(&full)).flatten(), preview: false, diff: Some(diff) };
        let position = self.active.map_or(self.editors.len(), |i| i + 1);
        self.editors.insert(position, editor);
        self.activate(position);
    }

    fn new_untitled(&mut self) {
        let n = (1..).find(|n| !self.editors.iter().any(|e| e.path.is_none() && e.name == format!("Untitled-{n}"))).unwrap_or(1);
        self.push_editor(None, format!("Untitled-{n}"), "");
    }

    fn activate(&mut self, index: usize) {
        if let Some(previous) = self.active_editor_mut() {
            previous.view.set_focus(false);
        }
        self.active = Some(index);
        let rect = self.editor_rect();
        let editor = &mut self.editors[index];
        editor.resize(rect.width(), rect.height());
        editor.view.set_focus(true);
        editor.view.mark_dirty();
        if let Some(explorer) = &self.explorer {
            explorer.reveal(editor.path.as_deref());
        }
        self.needs_render = true;
        self.refresh_timeline();
        if self.find.is_some() {
            self.find_changed(false);
            if self.find_focused {
                self.editors[index].view.set_focus(false);
            }
        }
    }

    fn mtime(path: &Path) -> Option<std::time::SystemTime> {
        std::fs::metadata(path).and_then(|m| m.modified()).ok()
    }

    /// Relê os arquivos abertos que mudaram no disco e não têm edição pendente (o `TextFileEditorModel` faz o
    /// mesmo ao receber o evento do watcher); os com edição ficam como estão até salvar.
    fn check_disk(&mut self) {
        for index in 0..self.editors.len() {
            let editor = &self.editors[index];
            let Some(path) = editor.path.clone() else { continue };
            let now = Self::mtime(&path);
            if now.is_none() || now == editor.disk_mtime || editor.is_modified() {
                continue;
            }
            let Ok(bytes) = std::fs::read(&path) else { continue };
            let text = String::from_utf8_lossy(&bytes).into_owned();
            let model = self.editors[index].view.model().clone();
            if model.get_value_with(EndOfLinePreference::TextDefined, true).to_string_lossy() != text {
                // Troca o conteúdo como uma edição (o desfazer volta ao texto anterior), mantendo o cursor.
                self.editors[index].view.replace_all(&text);
                self.editors[index].saved_version = model.get_alternative_version_id();
                self.editors[index].diff_version = -1;
            }
            self.editors[index].disk_mtime = now;
            self.needs_render = true;
        }
        if let Some(explorer) = &self.explorer {
            explorer.refresh_if_changed();
        }
    }

    /// A próxima conferência do disco em 2 s (o watcher do VS Code avisa na hora; aqui é por consulta).
    fn schedule_disk_check(&self) {
        let queue = self.queue.clone();
        je_base::runtime::set_timeout(move || queue.borrow_mut().push(Command::CheckDisk), 2000);
    }

    /// Fecha as abas `targets` (em ordem decrescente): as salvas fecham já; a primeira modificada pergunta.
    fn close_many(&mut self, targets: Vec<usize>) {
        for index in targets {
            if index >= self.editors.len() {
                continue;
            }
            if self.editors[index].is_modified() {
                self.request_close(index);
                return;
            }
            self.close_editor(index);
        }
    }

    /// O menu de contexto de uma aba (`editor/title/context`).
    fn tab_menu(&mut self, index: usize) {
        use je_base::actions::{Action, ActionCallback, ActionRef, Separator};
        let Some(editor) = self.editors.get(index) else { return };
        let path = editor.path.clone();
        let queue = self.queue.clone();
        let item = |label: &str, command: Command, enabled: bool| -> ActionRef {
            let queue = queue.clone();
            let callback: ActionCallback = Rc::new(move |_| {
                queue.borrow_mut().push(command.clone());
                je_base::dynamic::Dynamic::Undefined.into()
            });
            Action::new(label, label, "", enabled, Some(callback)) as ActionRef
        };
        let count = self.editors.len();
        let mut actions = vec![
            item("Close", Command::CloseEditor(index), true),
            item("Close Others", Command::CloseOthers(index), count > 1),
            item("Close to the Right", Command::CloseToTheRight(index), index + 1 < count),
            item("Close Saved", Command::CloseSaved, true),
            item("Close All", Command::CloseAll, true),
        ];
        if let Some(path) = path {
            actions.push(Separator::new() as ActionRef);
            actions.push(item("Copy Path", Command::CopyPath(path.clone(), false), true));
            actions.push(item("Copy Relative Path", Command::CopyPath(path.clone(), true), self.workspace.is_some()));
            actions.push(Separator::new() as ActionRef);
            actions.push(item("Reveal in Explorer View", Command::RevealInExplorer(path.clone()), self.explorer.is_some()));
            actions.push(item(if cfg!(windows) { "Reveal in File Explorer" } else if cfg!(target_os = "macos") { "Reveal in Finder" } else { "Open Containing Folder" }, Command::RevealInFileManager(path), true));
        }
        self.open_menu(actions);
    }

    /// Fechar uma aba modificada pergunta antes (`Do you want to save the changes you made to X?`).
    fn request_close(&mut self, index: usize) {
        let Some(editor) = self.editors.get(index) else { return };
        if !editor.is_modified() {
            self.close_editor(index);
            return;
        }
        let name = editor.name.clone();
        self.activate(index);
        // O diálogo modal do VS Code (`showSaveConfirm`): Save, Don't Save e Cancel, com Esc como Cancel.
        self.show_dialog(
            &format!("Do you want to save the changes you made to {name}?"),
            Some("Your changes will be lost if you don't save them."),
            je_widgets::dialog::DialogType::Warning,
            vec![("&&Save", Command::SaveAndClose(index)), ("Do&&n't Save", Command::DiscardAndClose(index)), ("Cancel", Command::CancelExit)],
        );
    }

    /// Mostra um diálogo modal (`IDialogService.show`): cada botão roda o comando dele; o último é o de cancelar (Esc).
    fn show_dialog(&mut self, message: &str, detail: Option<&str>, kind: je_widgets::dialog::DialogType, buttons: Vec<(&str, Command)>) {
        use je_widgets::dialog::{Dialog, DialogOptions, default_dialog_styles};
        let mut options = DialogOptions::new(
            je_widgets::button::default_button_styles(&self.theme),
            je_widgets::toggle::default_checkbox_styles(&self.theme),
            je_widgets::input_box::default_input_box_styles(&self.theme),
            default_dialog_styles(&self.theme),
        );
        options.r#type = Some(kind);
        options.cancel_id = Some(buttons.len() as i32 - 1);
        options.detail = detail.map(str::to_owned);
        let labels = buttons.iter().map(|(label, _)| (*label).to_owned()).collect();
        let commands: Vec<Command> = buttons.into_iter().map(|(_, command)| command).collect();
        let root = self.ui.root();
        let dialog = Dialog::new(&self.ui, root, message, Some(labels), options);
        let queue = self.queue.clone();
        let _ = dialog.show().then(move |result| {
            queue.borrow_mut().push(Command::CloseDialog);
            if let Some(command) = usize::try_from(result.button).ok().and_then(|i| commands.get(i)) {
                queue.borrow_mut().push(command.clone());
            }
        });
        self.dialog = Some(dialog);
        // O diálogo põe o foco no botão padrão no próximo quadro de animação: roda já, para o teclado chegar nele.
        self.ui.run_animation_frame();
        self.needs_render = true;
    }

    /// Continua a saída: pergunta pela próxima aba modificada ou encerra.
    fn continue_exit(&mut self) {
        if !self.exiting {
            return;
        }
        match self.editors.iter().position(OpenEditor::is_modified) {
            Some(index) => self.request_close(index),
            None => self.exit_requested = true,
        }
    }

    /// A janela pediu para fechar (o X do sistema): o mesmo caminho do `Exit`.
    pub fn request_exit(&mut self) {
        self.run(Command::Exit);
    }

    fn close_editor(&mut self, index: usize) {
        if index >= self.editors.len() {
            return;
        }
        self.editors.remove(index);
        self.active = match self.active {
            _ if self.editors.is_empty() => None,
            Some(active) if active > index || active == self.editors.len() => Some(active - 1),
            other => other,
        };
        if let Some(active) = self.active {
            self.activate(active);
        }
        *self.hovered_tab.borrow_mut() = None;
        self.close_empty_group();
        self.needs_render = true;
    }

    fn save(&mut self, index: usize) {
        let editor = &mut self.editors[index];
        if editor.diff.is_some() && editor.path.is_none() {
            // O lado do índice é somente leitura: não há arquivo para gravar.
            return;
        }
        let Some(path) = editor.path.clone() else {
            // Sem arquivo ainda: o Save vira o Save As.
            self.open_prompt(PromptAction::SaveAs(index));
            return;
        };
        // `files.trimTrailingWhitespace`, `files.insertFinalNewline` e `files.trimFinalNewlines`.
        editor.view.run_save_participants(false);
        let model = editor.view.model().clone();
        let text = model.get_value_with(EndOfLinePreference::TextDefined, true).to_string_lossy();
        let result = std::fs::write(&path, text.as_bytes());
        editor.view.mark_dirty();
        if result.is_ok() {
            // O mesmo arquivo aberto em outros grupos também fica salvo (o modelo é um só).
            let (version, mtime) = (model.get_alternative_version_id(), Self::mtime(&path));
            for other in self.all_editors_mut().filter(|e| e.path.as_deref() == Some(path.as_path())) {
                other.saved_version = version;
                other.disk_mtime = mtime;
            }
        }
        if result.is_ok() {
            self.git_op(git::Operation::Refresh);
            let user = themes::user_data_dir().map(|d| d.join("User"));
            if [user.as_ref().map(|u| u.join("settings.json")), user.map(|u| u.join("keybindings.json"))].iter().any(|p| p.as_deref() == Some(path.as_path())) {
                self.apply_user_settings();
            }
        }
        if let Err(e) = result {
            self.notify(Severity::Error, format!("Falha ao salvar '{}': {e}", path.display()));
        }
        self.needs_render = true;
    }

    fn revert(&mut self, index: usize) {
        let Some(path) = self.editors[index].path.clone() else { return };
        match std::fs::read(&path) {
            Ok(bytes) => {
                let name = self.editors[index].name.clone();
                let diff_original = self.editors[index].diff.as_ref().map(|d| d.original().model().clone());
                let (model, language_name) = self.model_for(&name, &String::from_utf8_lossy(&bytes));
                let rect = self.editor_rect();
                let mut view = EditorView::new(model.clone(), rect.width(), rect.height(), EditorColors::from_theme(&self.theme));
                if let Some(env) = &self.environment {
                    view.set_token_colors(&env.color_map());
                }
                view.apply_configuration(&themes::user_settings());
                let diff = diff_original.map(|original| self.new_diff(original, &mut view));
                self.editors[index] = OpenEditor { path: Some(path), name, language_name, view, saved_version: model.get_alternative_version_id(), head: None, diff_version: -1, disk_mtime: None, preview: false, diff };
                self.activate(index);
            }
            Err(e) => self.notify(Severity::Error, format!("falha ao reverter: {e}")),
        }
    }

    /// Roda os comandos enfileirados pelos menus e botões.
    fn run_commands(&mut self) {
        loop {
            let commands = std::mem::take(&mut *self.queue.borrow_mut());
            if commands.is_empty() {
                return;
            }
            let from_menu = self.context_menu.is_some();
            for command in commands {
                self.run(command);
            }
            // Um item do menu de contexto rodou: o menu se fecha.
            if from_menu {
                self.close_context_menu();
            }
        }
    }

    pub fn run(&mut self, command: Command) {
        let active = self.active;
        match command {
            Command::NewUntitled => self.new_untitled(),
            Command::OpenFile(path) => {
                if let Err(e) = self.open_file(&path) {
                    self.notify(Severity::Error, e);
                }
            }
            Command::PreviewFile(path) => {
                if let Err(e) = self.preview_file(&path) {
                    self.notify(Severity::Error, e);
                }
            }
            Command::PinEditor(index) => {
                if let Some(editor) = self.editors.get_mut(index) {
                    editor.preview = false;
                }
            }
            Command::Save => {
                if let Some(i) = active {
                    self.save(i);
                }
            }
            Command::SaveAll => {
                for i in 0..self.editors.len() {
                    if self.editors[i].is_modified() && self.editors[i].path.is_some() {
                        self.save(i);
                    }
                }
            }
            Command::Revert => {
                if let Some(i) = active {
                    self.revert(i);
                }
            }
            Command::CloseActiveEditor => {
                if let Some(i) = active {
                    self.request_close(i);
                }
            }
            Command::CloseEditor(i) => self.request_close(i),
            Command::SplitEditor => self.split_editor(),
            Command::FocusGroup(i) => self.focus_group(i),
            Command::FocusGroupEditor(group, index) => {
                self.focus_group(group);
                if index < self.editors.len() {
                    self.activate(index);
                }
            }
            Command::TabContextMenu(index) => self.tab_menu(index),
            Command::CloseOthers(index) | Command::CloseToTheRight(index) => {
                // Fecha de trás para a frente para os índices não andarem; as modificadas perguntam uma por vez.
                let others = matches!(command, Command::CloseOthers(_));
                let targets: Vec<usize> = (0..self.editors.len()).filter(|i| if others { *i != index } else { *i > index }).rev().collect();
                self.close_many(targets);
            }
            Command::CloseSaved => {
                let targets: Vec<usize> = (0..self.editors.len()).filter(|i| !self.editors[*i].is_modified()).rev().collect();
                self.close_many(targets);
            }
            Command::CheckDisk => {
                self.check_disk();
                self.schedule_disk_check();
            }
            Command::CloseAll => {
                let targets: Vec<usize> = (0..self.editors.len()).rev().collect();
                self.close_many(targets);
            }
            Command::RevealInExplorer(path) => {
                self.sidebar_visible = true;
                self.side_view = SideView::Explorer;
                if let Some(explorer) = &self.explorer {
                    explorer.reveal(Some(&path));
                }
                self.resize_editors();
            }
            Command::SaveAndClose(i) => {
                if i < self.editors.len() {
                    if self.editors[i].path.is_none() {
                        // Sem arquivo: o Save As pergunta o caminho e a aba fecha depois de gravar.
                        self.close_after_save_as = true;
                        self.save(i);
                    } else {
                        self.save(i);
                        if !self.editors[i].is_modified() {
                            self.close_editor(i);
                            self.continue_exit();
                        }
                    }
                }
            }
            Command::DiscardAndClose(i) => {
                if i < self.editors.len() {
                    self.editors[i].saved_version = self.editors[i].view.model().get_alternative_version_id();
                    self.close_editor(i);
                    self.continue_exit();
                }
            }
            Command::CancelExit => self.exiting = false,
            Command::CloseDialog => {
                if let Some(dialog) = self.dialog.take() {
                    IDisposable::dispose(&*dialog);
                }
                self.needs_render = true;
            }
            Command::ActivateEditor(i) => {
                if i < self.editors.len() {
                    self.activate(i);
                }
            }
            Command::NextEditor | Command::PreviousEditor => {
                if let Some(i) = active {
                    let n = self.editors.len();
                    let next = if command == Command::NextEditor { (i + 1) % n } else { (i + n - 1) % n };
                    self.activate(next);
                }
            }
            Command::Undo | Command::Redo | Command::Cut | Command::Copy | Command::Paste | Command::SelectAll => {
                if let Some(editor) = self.active_editor_mut() {
                    let view = editor.focused_view_mut();
                    match command {
                        Command::Undo => view.undo(),
                        Command::Redo => view.redo(),
                        Command::Cut => view.cut(),
                        Command::Copy => view.copy(),
                        Command::Paste => view.paste(),
                        _ => view.select_all(),
                    }
                }
            }
            Command::ToggleSidebar => {
                self.sidebar_visible = !self.sidebar_visible;
                self.resize_editors();
            }
            Command::ShowExplorer => self.show_view(SideView::Explorer),
            Command::ShowView(view) => self.show_view(view),
            Command::ShowSearch => {
                // A semente é a seleção (ou a palavra) do editor; o campo vem selecionado.
                let seed = self.active_editor().map(|e| e.view.seed_search_string()).unwrap_or_default();
                self.show_view(SideView::Search);
                if !seed.is_empty() {
                    self.search.field = input::seeded(&seed);
                    self.start_search();
                } else {
                    self.search.field.select_all();
                }
                self.search_focused = true;
                self.set_terminal_focus_off();
                self.ui.blur();
                self.resize_editors();
            }
            Command::SearchFocus => {
                self.search_focused = true;
                self.set_terminal_focus_off();
                if let Some(editor) = self.active_editor_mut() {
                    editor.view.set_focus(false);
                }
            }
            Command::SearchTick(generation) => {
                if generation == self.search_generation {
                    self.start_search();
                }
            }
            Command::SearchNow => self.start_search(),
            Command::SearchClear => {
                self.search.clear();
                self.needs_render = true;
            }
            Command::SearchCollapse => {
                self.search.collapse_all();
                self.needs_render = true;
            }
            Command::SearchToggleReplace => {
                self.search.replace_visible = !self.search.replace_visible;
                self.search.in_replace = self.search.replace_visible;
                self.search_focused = true;
            }
            Command::SearchFocusReplace => {
                self.search.in_replace = true;
                self.search_focused = true;
                if let Some(editor) = self.active_editor_mut() {
                    editor.view.set_focus(false);
                }
            }
            Command::SearchReplaceAll => self.search_replace_all(),
            Command::ShowSearchReplace => {
                self.run(Command::ShowSearch);
                self.search.replace_visible = true;
                return;
            }
            Command::SearchToggleCase | Command::SearchToggleWord | Command::SearchToggleRegex => {
                match command {
                    Command::SearchToggleCase => self.search.match_case = !self.search.match_case,
                    Command::SearchToggleWord => self.search.whole_word = !self.search.whole_word,
                    _ => self.search.regex = !self.search.regex,
                }
                self.start_search();
            }
            // As dicas não mexem na moldura: saem antes do `needs_render` do fim.
            Command::ShowHover(generation) => {
                let request = self.hover_request.borrow().clone();
                if let Some((g, node, text, position)) = request
                    && g == generation
                    && self.ui.tree().is_alive(node)
                {
                    self.show_hover(node, &text, position);
                }
                return;
            }
            Command::HideHover => {
                *self.hover_request.borrow_mut() = None;
                self.hide_hover();
                return;
            }
            Command::AutoSave(index, version) => {
                self.auto_save_scheduled = None;
                if self.editors.get(index).is_some_and(|e| e.path.is_some() && e.is_modified() && e.view.model().get_version_id() == version) {
                    self.save(index);
                }
                return;
            }
            Command::AutoFetch => {
                if self.git_rx.is_none() {
                    self.git_op(git::Operation::Fetch);
                }
                self.schedule_autofetch();
                return;
            }
            Command::Zoom(step) => {
                self.zoom_level = if step == 0 { 0.0 } else { (self.zoom_level + f64::from(step)).clamp(-8.0, 8.0) };
                self.resize_editors();
            }
            Command::OpenRecent => {
                let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from);
                let tidy = |p: &Path| -> String {
                    match home.as_deref().and_then(|h| p.strip_prefix(h).ok()) {
                        Some(rest) => format!("~{}{}", std::path::MAIN_SEPARATOR, rest.display()),
                        None => p.display().to_string(),
                    }
                };
                let items: Vec<PickItem> = recent::load()
                    .into_iter()
                    .filter_map(|entry| {
                        let (path, command) = match entry {
                            recent::Recent::Folder(p) => (p.clone(), Command::OpenFolderPath(p)),
                            recent::Recent::File(p) => (p.clone(), Command::OpenFile(p)),
                        };
                        let label = path.file_name()?.to_string_lossy().into_owned();
                        let description = path.parent().map(tidy);
                        Some(PickItem { label, description, keybinding: None, command, recent: false })
                    })
                    .collect();
                if items.is_empty() {
                    self.notify(Severity::Info, "Não há itens recentes do VS Code para mostrar.".to_owned());
                } else {
                    self.quick_pick = Some(QuickPick::list(items, "Select to open (hold Ctrl-key to force new window or Alt-key for same window)"));
                    self.ui.blur();
                }
            }
            Command::OpenFolderPath(path) => {
                if path.is_dir() {
                    self.set_workspace(path);
                } else {
                    self.notify(Severity::Error, format!("A pasta '{}' não existe mais.", path.display()));
                }
            }
            Command::ShowExtensions => {
                self.show_view(SideView::Extensions);
                self.extensions_focused = self.sidebar_visible;
            }
            Command::ExtensionsFocus => {
                self.extensions_focused = true;
                self.search_focused = false;
                self.scm_focused = false;
                self.set_terminal_focus_off();
                if let Some(editor) = self.active_editor_mut() {
                    editor.view.set_focus(false);
                }
            }
            Command::ShowScm => {
                self.show_view(SideView::SourceControl);
                if self.sidebar_visible {
                    self.scm_focused = true;
                    self.search_focused = false;
                    self.set_terminal_focus_off();
                    self.ui.blur();
                    self.git_op(git::Operation::Refresh);
                }
            }
            Command::ScmFocus => {
                self.scm_focused = true;
                self.search_focused = false;
                self.set_terminal_focus_off();
                if let Some(editor) = self.active_editor_mut() {
                    editor.view.set_focus(false);
                }
            }
            Command::ScmRefresh => self.git_op(git::Operation::Refresh),
            Command::ScmOp(op) => {
                self.git_op(op);
            }
            Command::ScmCommit => {
                let message = self.scm.text();
                let staged = self.scm.status.borrow().staged.len();
                if message.trim().is_empty() {
                    self.notify(Severity::Warning, "Please provide a commit message.".to_owned());
                } else if staged == 0 {
                    self.notify(Severity::Warning, "There are no staged changes to commit. Stage some changes first.".to_owned());
                } else {
                    self.git_op(git::Operation::Commit(message));
                    self.scm.message.set_value("");
                }
            }
            Command::ScmOpen(path) => {
                if let Some(root) = self.git_root.clone() {
                    let full = root.join(&path);
                    if full.is_file() {
                        let _ = self.open_file(&full);
                    }
                }
            }
            Command::ScmOpenChanges(group, path) => self.open_changes(group, &path),
            Command::ScmContextMenu(entry) => self.scm_menu(entry),
            Command::ScmMoreMenu => {
                // O `...` da vista Source Control: as operações com o remoto.
                use je_base::actions::{Action, ActionCallback, ActionRef};
                let queue = self.queue.clone();
                let item = |label: &str, command: Command| -> ActionRef {
                    let queue = queue.clone();
                    let callback: ActionCallback = Rc::new(move |_| {
                        queue.borrow_mut().push(command.clone());
                        je_base::dynamic::Dynamic::Undefined.into()
                    });
                    Action::new(label, label, "", true, Some(callback)) as ActionRef
                };
                let actions = vec![
                    item("Pull", Command::ScmOp(git::Operation::Pull)),
                    item("Push", Command::ScmOp(git::Operation::Push)),
                    item("Fetch", Command::ScmOp(git::Operation::Fetch)),
                    item("Sync", Command::ScmOp(git::Operation::Sync)),
                ];
                self.open_menu(actions);
            }
            Command::DismissToast(id) => {
                self.toasts.retain(|t| t.id != id);
                self.needs_render = true;
            }
            Command::OpenAt(path, line, column, end_column) => {
                if let Err(e) = self.open_file(&path) {
                    self.notify(Severity::Error, e);
                } else if let Some(editor) = self.active_editor_mut() {
                    editor.view.select_range(je_editor::range::Range::new(line, column, line, end_column));
                    self.search_focused = false;
                    if let Some(editor) = self.active_editor_mut() {
                        editor.view.set_focus(true);
                    }
                }
            }
            Command::ToggleStatusBar => self.statusbar_visible = !self.statusbar_visible,
            Command::ToggleFullScreen => self.fullscreen_requested = true,
            Command::ToggleWordWrap => {
                if let Some(editor) = self.active_editor_mut() {
                    editor.view.toggle_word_wrap();
                }
            }
            Command::QuickOpen => self.open_quick_pick(""),
            Command::ShowCommands => self.open_quick_pick(">"),
            Command::ToggleTimeline => {
                self.timeline_open = !self.timeline_open;
                self.timeline_path = None;
                self.refresh_timeline();
            }
            Command::ShowManageMenu => {
                // O menu da engrenagem (`Manage`): comandos, configurações, atalhos e temas.
                use je_base::actions::{Action, ActionCallback, ActionRef, Separator};
                let queue = self.queue.clone();
                let item = |label: &str, command: Command| -> ActionRef {
                    let queue = queue.clone();
                    let callback: ActionCallback = Rc::new(move |_| {
                        queue.borrow_mut().push(command.clone());
                        je_base::dynamic::Dynamic::Undefined.into()
                    });
                    Action::new(label, label, "", true, Some(callback)) as ActionRef
                };
                let actions = vec![
                    item("Command Palette...", Command::ShowCommands),
                    Separator::new() as ActionRef,
                    item("Settings (JSON)", Command::OpenSettingsJson),
                    item("Keyboard Shortcuts (JSON)", Command::OpenKeybindingsJson),
                    Separator::new() as ActionRef,
                    item("Themes: Color Theme", Command::PickTheme),
                ];
                self.open_menu(actions);
            }
            Command::PickTheme => self.open_theme_picker(),
            Command::OpenSettingsJson | Command::OpenKeybindingsJson => {
                let file = if command == Command::OpenSettingsJson { "settings.json" } else { "keybindings.json" };
                match themes::user_data_dir().map(|d| d.join("User").join(file)) {
                    Some(path) => {
                        // O arquivo do usuário do VS Code: criado vazio se ainda não existe.
                        if !path.exists() {
                            let initial = if file == "settings.json" { "{\n}\n" } else { "[\n]\n" };
                            if let Err(e) = file_ops::write_new(&path, initial) {
                                self.notify(Severity::Error, e);
                                return;
                            }
                        }
                        if let Err(e) = self.open_file(&path) {
                            self.notify(Severity::Error, e);
                        }
                    }
                    None => self.notify(Severity::Error, "Não achei a pasta de dados do usuário.".to_owned()),
                }
            }
            Command::PickIndentation => {
                let items = vec![
                    PickItem { label: "Indent Using Spaces".to_owned(), description: Some("change view".to_owned()), keybinding: None, command: Command::PickIndentSize(true), recent: false },
                    PickItem { label: "Indent Using Tabs".to_owned(), description: Some("change view".to_owned()), keybinding: None, command: Command::PickIndentSize(false), recent: false },
                    PickItem { label: "Detect Indentation from Content".to_owned(), description: Some("change view".to_owned()), keybinding: None, command: Command::DetectIndentation, recent: false },
                ];
                self.quick_pick = Some(QuickPick::list(items, "Select Action"));
                self.ui.blur();
            }
            Command::PickIndentSize(spaces) => {
                let current = self.active_editor().map_or(4, |e| e.view.model().get_options().tab_size);
                let items: Vec<PickItem> = (1..=8).map(|n| PickItem { label: n.to_string(), description: (n == current).then(|| "Configured Tab Size".to_owned()), keybinding: None, command: Command::SetIndentation(spaces, n), recent: false }).collect();
                let at = items.iter().position(|i| i.description.is_some()).unwrap_or(3);
                let mut pick = QuickPick::list(items, "Select Tab Size for the current file");
                pick.move_focus(at as isize);
                self.quick_pick = Some(pick);
                self.ui.blur();
            }
            Command::SetIndentation(spaces, size) => {
                if let Some(editor) = self.active_editor_mut() {
                    editor.view.model().update_options(je_editor::model::TextModelUpdateOptions { tab_size: Some(size), indent_size: Some(je_editor::model::IndentSize::Size(size)), insert_spaces: Some(spaces), ..Default::default() });
                    editor.view.mark_dirty();
                }
            }
            Command::DetectIndentation => {
                if let Some(editor) = self.active_editor_mut() {
                    let options = editor.view.model().get_options();
                    editor.view.model().detect_indentation(options.insert_spaces, options.tab_size);
                    editor.view.mark_dirty();
                }
            }
            Command::ToggleMaximizedPanel => {
                // Maximizar o painel é dar a ele a altura toda das partes, menos o mínimo do grupo de editores.
                let full = (self.parts_bottom() - TITLE_HEIGHT - SASH - 80.0).max(80.0);
                if self.panel_height >= full - 1.0 {
                    self.panel_height = self.panel_restore_height.unwrap_or(DEFAULT_PANEL_HEIGHT);
                } else {
                    self.panel_restore_height = Some(self.panel_height);
                    self.panel_height = full;
                }
                self.panel_visible = true;
                self.resize_editors();
            }
            Command::PickEol => {
                let items = [("LF", false), ("CRLF", true)].map(|(label, crlf)| PickItem { label: label.to_owned(), description: None, keybinding: None, command: Command::SetEol(crlf), recent: false }).to_vec();
                self.quick_pick = Some(QuickPick::list(items, "Select End of Line Sequence"));
                self.ui.blur();
            }
            Command::SetEol(crlf) => {
                if let Some(editor) = self.active_editor_mut() {
                    editor.view.model().set_eol(if crlf { je_editor::model::EndOfLineSequence::CrLf } else { je_editor::model::EndOfLineSequence::Lf });
                    editor.view.mark_dirty();
                }
            }
            Command::PickLanguage => {
                let Some(env) = &self.environment else { return };
                let items = env.languages().into_iter().map(|(id, name)| PickItem { label: name, description: Some(format!("({id})")), keybinding: None, command: Command::SetLanguage(id), recent: false }).collect();
                self.quick_pick = Some(QuickPick::list(items, "Select Language Mode"));
                self.ui.blur();
            }
            Command::SetLanguage(id) => {
                if let (Some(env), Some(editor)) = (self.environment.clone(), self.active.map(|i| &mut self.editors[i])) {
                    env.set_language(editor.view.model(), &id);
                    editor.language_name = env.language_name(&id);
                    editor.view.mark_dirty();
                }
            }
            Command::ExplorerContextMenu(path) => self.open_explorer_menu(&path),
            Command::CloseContextMenu => self.close_context_menu(),
            Command::PromptGoToLine => {
                if self.active.is_some() {
                    self.open_prompt(PromptAction::GoToLine);
                }
            }
            Command::PromptSaveAs => {
                if let Some(i) = self.active {
                    self.open_prompt(PromptAction::SaveAs(i));
                }
            }
            Command::PromptOpenFile => self.open_prompt(PromptAction::OpenFile),
            Command::PromptOpenFolder => self.open_prompt(PromptAction::OpenFolder),
            Command::PromptNewFile(dir) => self.open_prompt(PromptAction::NewFile(dir)),
            Command::PromptNewFolder(dir) => self.open_prompt(PromptAction::NewFolder(dir)),
            Command::PromptRename(path) => self.open_prompt(PromptAction::Rename(path)),
            Command::ConfirmDelete(path) => self.confirm_delete(path),
            Command::Prompt(action, text) => self.run_prompt(action, &text),
            Command::DeletePath(path) => match file_ops::trash(&path) {
                Ok(()) => {
                    // Fecha as abas dos arquivos apagados.
                    while let Some(i) = self.editors.iter().position(|e| e.path.as_deref().is_some_and(|p| p.starts_with(&path))) {
                        self.close_editor(i);
                    }
                    self.refresh_explorer();
                }
                Err(e) => self.notify(Severity::Error, e),
            },
            Command::RevealInFileManager(path) => {
                if let Err(e) = file_ops::reveal(&path) {
                    self.notify(Severity::Error, e);
                }
            }
            Command::OpenInTerminal(path) => {
                let dir = if path.is_dir() { path } else { path.parent().map(Path::to_path_buf).unwrap_or(path) };
                self.terminal = None;
                self.workspace_for_terminal = Some(dir);
                self.panel_visible = true;
                self.set_terminal_focus(true);
                self.ensure_terminal();
                self.resize_editors();
            }
            Command::CopyPath(path, relative) => {
                let text = if relative { self.workspace.as_deref().and_then(|w| path.strip_prefix(w).ok()).unwrap_or(&path).to_string_lossy().into_owned() } else { path.to_string_lossy().into_owned() };
                je_editor_view::clipboard::Clipboard::new().set_text(&text);
            }
            Command::SetTheme(index) => {
                self.apply_theme(index);
                self.theme_before_preview = None;
            }
            Command::QuickPickAccept(index) => {
                let command = self.quick_pick.as_ref().and_then(|pick| pick.accept(Some(index)));
                self.quick_pick = None;
                if let Some(command) = command {
                    self.run(command);
                }
            }
            Command::TogglePanel => {
                self.panel_visible = !self.panel_visible;
                self.set_terminal_focus(self.panel_visible);
                self.ensure_terminal();
                self.resize_editors();
            }
            Command::KillTerminal => {
                self.terminal = None;
                self.panel_visible = false;
                self.set_terminal_focus(false);
                self.resize_editors();
            }
            Command::ShowPanelTab(tab) => {
                self.panel_tab = tab;
                self.panel_visible = true;
                self.set_terminal_focus(tab == PanelTab::Terminal);
                if tab == PanelTab::Terminal {
                    self.ensure_terminal();
                }
                self.resize_editors();
            }
            Command::ToggleTerminal | Command::NewTerminal => {
                self.panel_tab = PanelTab::Terminal;
                if command == Command::NewTerminal {
                    self.terminal = None;
                }
                if self.panel_visible && self.terminal_focused && command == Command::ToggleTerminal {
                    self.panel_visible = false;
                    self.set_terminal_focus(false);
                } else {
                    self.panel_visible = true;
                    self.set_terminal_focus(true);
                }
                self.ensure_terminal();
                self.resize_editors();
            }
            Command::Find => self.open_find(),
            Command::FindNext => self.find_step(1),
            Command::FindPrevious => self.find_step(-1),
            Command::FindClose => self.close_find(),
            Command::FindToggleCase | Command::FindToggleWord | Command::FindToggleRegex => {
                if let Some(find) = &mut self.find {
                    match command {
                        Command::FindToggleCase => find.match_case = !find.match_case,
                        Command::FindToggleWord => find.whole_word = !find.whole_word,
                        _ => find.regex = !find.regex,
                    }
                }
                self.find_changed(true);
            }
            // A substituição e a busca na seleção ainda não existem: o botão não faz nada.
            Command::FindToggleReplace => {
                if let Some(find) = &mut self.find {
                    find.replace_visible = !find.replace_visible;
                    find.in_replace = find.replace_visible;
                }
            }
            Command::FindAndReplace => {
                if self.find.is_none() {
                    self.open_find();
                }
                if let Some(find) = &mut self.find {
                    find.replace_visible = true;
                    find.in_replace = !find.field.is_empty();
                }
                self.find_focused = true;
            }
            Command::FindFocusReplace => {
                if let Some(find) = &mut self.find {
                    find.in_replace = true;
                }
                self.find_focused = true;
                if let Some(editor) = self.active_editor_mut() {
                    editor.view.set_focus(false);
                }
            }
            Command::FindReplace => self.find_replace(false),
            Command::FindReplaceAll => self.find_replace(true),
            // A busca na seleção ainda não existe: o botão não faz nada.
            Command::FindInSelection => {}
            Command::Exit => {
                self.exiting = true;
                self.continue_exit();
            }
        }
        self.needs_render = true;
    }

    #[cfg(test)]
    pub fn git_status(&self) -> git::Status {
        self.scm.status.borrow().clone()
    }

    #[cfg(test)]
    pub fn panel_is_visible(&self) -> bool {
        self.panel_visible
    }

    #[cfg(test)]
    pub fn tab_paths(&self) -> Vec<PathBuf> {
        self.editors.iter().filter_map(|e| e.path.clone()).collect()
    }

    #[cfg(test)]
    pub fn explorer_lists(&self, path: &Path) -> bool {
        self.explorer.as_ref().is_some_and(|e| e.contains(path))
    }

    #[cfg(test)]
    pub fn has_hover(&self) -> bool {
        self.hover.is_some()
    }

    #[cfg(test)]
    pub fn set_auto_save(&mut self, mode: &str, delay: u64) {
        self.auto_save = (mode.to_owned(), delay);
    }

    #[cfg(test)]
    pub fn group_count(&self) -> usize {
        self.groups.len()
    }

    #[cfg(test)]
    pub fn timeline_subjects(&self) -> Vec<String> {
        self.timeline_entries.iter().map(|e| e.0.clone()).collect()
    }

    #[cfg(test)]
    pub fn toast_count(&self) -> usize {
        self.toasts.len()
    }

    #[cfg(test)]
    pub fn search_message(&self) -> Option<String> {
        self.search.message()
    }

    #[cfg(test)]
    pub fn workspace_path(&self) -> Option<PathBuf> {
        self.workspace.clone()
    }

    #[cfg(test)]
    pub fn has_context_menu(&self) -> bool {
        self.context_menu.is_some()
    }

    /// Troca a pasta do workspace: o explorador, o Quick Open e o terminal passam a usá-la.
    fn set_workspace(&mut self, path: PathBuf) {
        let node = self.nodes.explorer;
        let children = self.ui.tree().children(node).to_vec();
        for child in children {
            self.ui.remove(child);
        }
        let language_of = self.language_of.clone();
        self.explorer = Some(Explorer::new(&self.ui, node, &path, &self.theme, self.icons.clone(), language_of, self.queue.clone(), excludes::Excludes::files(&themes::user_settings()).matcher()));
        self.workspace = Some(path);
        self.workspace_files = None;
        self.sidebar_visible = true;
        self.detect_git();
        let selected = self.active_editor().and_then(|e| e.path.clone());
        if let Some(explorer) = &self.explorer {
            explorer.reveal(selected.as_deref());
        }
        self.resize_editors();
    }

    /// Relê o conteúdo do HEAD de cada editor aberto (depois de um commit, de trocar de ramo ou ao abrir).
    fn reload_heads(&mut self) {
        let root = self.git_root.clone();
        for editor in &mut self.editors {
            editor.diff_version = -1;
            editor.head = match (&root, &editor.path) {
                (Some(root), Some(path)) => path.strip_prefix(root).ok().and_then(|rel| git::head_text(root, &rel.to_string_lossy().replace('\\', "/"))).map(|text| {
                    // O modelo conta a linha vazia depois do último `\n`; o HEAD é cortado do mesmo jeito.
                    text.replace("\r\n", "\n").split('\n').map(je_base::JsString::from).collect()
                }),
                _ => None,
            };
        }
    }

    /// As barras de diff do editor ativo contra o HEAD (`DirtyDiffModel`), quando o texto mudou.
    fn update_gutter_diff(&mut self) {
        use je_editor::diff::lines_diff_computer::{ILinesDiffComputer, LinesDiffComputerOptions};
        let Some(editor) = self.active.map(|i| &mut self.editors[i]) else { return };
        // O editor de diff mostra as próprias mudanças, sem as barras da margem.
        if editor.diff.is_some() {
            return;
        }
        let version = editor.view.model().get_version_id();
        if editor.diff_version == version {
            return;
        }
        editor.diff_version = version;
        let Some(head) = &editor.head else {
            editor.view.set_gutter_changes(Vec::new());
            return;
        };
        let modified = editor.view.model().get_lines_content();
        let diff = je_editor::diff::default_lines_diff_computer::default_lines_diff_computer::DefaultLinesDiffComputer::new().compute_diff(
            head,
            &modified,
            &LinesDiffComputerOptions { ignore_trim_whitespace: false, max_computation_time_ms: 200.0, compute_moves: false, extend_to_subwords: None },
        );
        let changes = diff
            .changes
            .iter()
            .map(|c| {
                let (original, modified) = (c.original, c.modified);
                if original.is_empty() {
                    GutterChange { start_line: modified.start_line_number, end_line: modified.end_line_number_exclusive - 1, kind: GutterKind::Added }
                } else if modified.is_empty() {
                    GutterChange { start_line: modified.start_line_number, end_line: modified.start_line_number, kind: GutterKind::Deleted }
                } else {
                    GutterChange { start_line: modified.start_line_number, end_line: modified.end_line_number_exclusive - 1, kind: GutterKind::Modified }
                }
            })
            .collect();
        editor.view.set_gutter_changes(changes);
    }

    /// Reaplica o `settings.json` do usuário: o tema de cor, o zoom e o salvamento automático.
    fn apply_user_settings(&mut self) {
        let settings = themes::user_settings();
        if let Some(explorer) = &self.explorer {
            explorer.set_exclude(excludes::Excludes::files(&settings).matcher());
        }
        self.search.excludes = excludes::Excludes::search(&settings);
        self.workspace_files = None;
        self.auto_save = (
            settings.get("files.autoSave").and_then(|v| v.as_str()).unwrap_or("off").to_owned(),
            settings.get("files.autoSaveDelay").and_then(serde_json::Value::as_u64).unwrap_or(1000),
        );
        if let Some(level) = settings.get("window.zoomLevel").and_then(serde_json::Value::as_f64) {
            self.zoom_level = level.clamp(-8.0, 8.0);
            self.resize_editors();
        }
        // As configurações do editor (`editor.*`, `files.*` e os blocos de linguagem) e os atalhos do usuário.
        for editor in &mut self.editors {
            editor.view.apply_configuration(&settings);
            if let Some(diff) = &mut editor.diff {
                diff.original_mut().apply_configuration(&settings);
            }
        }
        if let Some(text) = themes::user_data_dir().and_then(|d| std::fs::read_to_string(d.join("User/keybindings.json")).ok())
            && let Err(e) = je_editor_view::set_user_keybindings(&text)
        {
            self.notify(Severity::Error, format!("keybindings.json: {e}"));
        }
        let wanted = settings.get("workbench.colorTheme").and_then(|v| v.as_str());
        if let Some(index) = themes::resolve(&self.themes, wanted)
            && self.theme_index != Some(index)
        {
            self.apply_theme(index);
        }
    }

    /// O `git.autofetch`: um `git fetch` agora e a cada `git.autofetchPeriod` segundos (180 por padrão).
    fn schedule_autofetch(&self) {
        let settings = themes::user_settings();
        if settings.get("git.autofetch").is_none_or(|v| v == &serde_json::Value::Bool(false)) || self.git_root.is_none() {
            return;
        }
        let period = settings.get("git.autofetchPeriod").and_then(serde_json::Value::as_u64).unwrap_or(180).max(30);
        let queue = self.queue.clone();
        je_base::runtime::set_timeout(move || queue.borrow_mut().push(Command::AutoFetch), period * 1000);
    }

    /// Relê o histórico do arquivo ativo para a Timeline (`git log`), quando ela está aberta e o arquivo mudou.
    fn refresh_timeline(&mut self) {
        let path = self.active_editor().and_then(|e| e.path.clone());
        if !self.timeline_open || path == self.timeline_path {
            return;
        }
        self.timeline_path = path.clone();
        self.timeline_entries = match (&self.git_root, path) {
            (Some(root), Some(path)) => git::file_history(root, &path),
            _ => Vec::new(),
        };
        self.needs_render = true;
    }

    /// Detecta o repositório do workspace e pede o estado.
    fn detect_git(&mut self) {
        self.git_root = self.workspace.as_deref().and_then(git::find_root);
        self.scm.set_status(git::Status::default());
        self.git_rx = None;
        if self.git_root.is_some() {
            self.git_op(git::Operation::Refresh);
            self.schedule_autofetch();
        }
    }

    /// Roda uma operação do git numa thread; o estado novo chega depois (ver `take_dirty`).
    fn git_op(&mut self, op: git::Operation) {
        if let Some(root) = self.git_root.clone() {
            self.git_rx = Some(git::spawn(root, op, self.wake.clone()));
        }
    }

    /// O texto de `Ln, Col`... do ramo na barra de status: `main`, com `*` se a árvore está suja e `+` se há índice.
    fn branch_label(&self) -> Option<String> {
        self.git_root.as_ref()?;
        let status = self.scm.status.borrow();
        if status.branch.is_empty() {
            return None;
        }
        Some(format!("{}{}{}", status.branch, if status.changes.is_empty() { "" } else { "*" }, if status.staged.is_empty() { "" } else { "+" }))
    }

    fn scm_menu(&mut self, entry: scm_view::Entry) {
        use je_base::actions::{Action, ActionCallback, ActionRef};
        use scm_view::{Entry, Group};
        let queue = self.queue.clone();
        let item = |label: &str, command: Command| -> ActionRef {
            let queue = queue.clone();
            let callback: ActionCallback = Rc::new(move |_| {
                queue.borrow_mut().push(command.clone());
                je_base::dynamic::Dynamic::Undefined.into()
            });
            Action::new(label, label, "", true, Some(callback)) as ActionRef
        };
        let mut actions: Vec<ActionRef> = Vec::new();
        match &entry {
            Entry::File(group, path) => {
                actions.push(item("Open File", Command::ScmOpen(path.clone())));
                let change = self.scm.changes_of(*group).into_iter().find(|c| &c.path == path);
                match group {
                    Group::Staged => actions.push(item("Unstage Changes", Command::ScmOp(git::Operation::Unstage(vec![path.clone()])))),
                    Group::Changes => {
                        actions.push(item("Stage Changes", Command::ScmOp(git::Operation::Stage(vec![path.clone()]))));
                        if let Some(change) = change {
                            actions.push(item("Discard Changes", Command::ScmOp(git::Operation::Discard(vec![change]))));
                        }
                    }
                }
            }
            Entry::Group(Group::Staged) => {
                let paths = self.scm.changes_of(Group::Staged).into_iter().map(|c| c.path).collect();
                actions.push(item("Unstage All Changes", Command::ScmOp(git::Operation::Unstage(paths))));
            }
            Entry::Group(Group::Changes) => {
                let all = self.scm.changes_of(Group::Changes);
                actions.push(item("Stage All Changes", Command::ScmOp(git::Operation::Stage(all.iter().map(|c| c.path.clone()).collect()))));
                actions.push(item("Discard All Changes", Command::ScmOp(git::Operation::Discard(all))));
            }
        }
        self.open_menu(actions);
    }

    /// Trata uma tecla com a mensagem do commit focada. Devolve se consumiu.
    fn scm_key(&mut self, key_code: KeyCode, text: Option<&str>, key: Option<EditorKey>, mods: Modifiers) -> bool {
        let ctrl = mods.ctrl || mods.meta;
        match key {
            Some(EditorKey::Escape) => {
                self.scm_focused = false;
                if let Some(editor) = self.active_editor_mut() {
                    editor.view.set_focus(true);
                }
            }
            Some(EditorKey::Enter) if ctrl => self.run(Command::ScmCommit),
            _ => {
                input::edit(&mut self.scm.message, key_code, key, mods, text, &input::SystemClipboard::new());
            }
        }
        self.needs_render = true;
        true
    }

    /// Mostra a vista `view` na barra lateral (clicar na ativa recolhe a barra, como no VS Code).
    fn show_view(&mut self, view: SideView) {
        if self.sidebar_visible && self.side_view == view {
            self.sidebar_visible = false;
        } else {
            self.sidebar_visible = true;
            self.side_view = view;
        }
        if view != SideView::Search || !self.sidebar_visible {
            self.search_focused = false;
        }
        self.resize_editors();
    }

    fn set_terminal_focus_off(&mut self) {
        if self.terminal_focused {
            self.terminal_focused = false;
        }
    }

    /// Roda a busca com o campo atual no workspace.
    fn start_search(&mut self) {
        let wake = self.wake.clone();
        self.search.run(self.workspace.as_deref(), wake);
        self.needs_render = true;
    }

    /// Agenda a busca depois de uma pausa na digitação (o debounce de 300 ms do VS Code).
    fn schedule_search(&mut self) {
        self.search_generation += 1;
        let generation = self.search_generation;
        let queue = self.queue.clone();
        je_base::runtime::set_timeout(Box::new(move || queue.borrow_mut().push(Command::SearchTick(generation))), 300);
    }

    /// O `Replace All` da busca em arquivos: os abertos recebem a edição no editor (ficam modificados); os outros
    /// são reescritos no disco.
    fn search_replace_all(&mut self) {
        use je_editor::text_model::FindScope;
        let query = self.search.query();
        if query.text.is_empty() {
            return;
        }
        let template = self.search.replace.value().to_owned();
        let separators = query.whole_word.then_some("`~!@#$%^&*()-=+[{]}\\|;:'\",.<>/?");
        let edits_for = |model: &TextModel| -> Vec<(je_editor::range::Range, String)> {
            model
                .find_matches(je_base::JsString::from(query.text.as_str()), FindScope::Whole, query.regex, query.match_case, separators, false, 100_000)
                .into_iter()
                .map(|m| {
                    let matched = model.get_value_in_range(&m.range).to_string_lossy();
                    (m.range, find::expand_replacement(&query.text, query.regex, query.match_case, &template, &matched))
                })
                .collect()
        };
        let (mut total, mut files) = (0usize, 0usize);
        for path in self.search.result_files() {
            let open = self.all_editors_mut().find(|e| e.path.as_deref() == Some(path.as_path())).map(|e| e.view.model().clone());
            if let Some(model) = open {
                let edits = edits_for(&model);
                if let Some(editor) = self.all_editors_mut().find(|e| e.path.as_deref() == Some(path.as_path())) {
                    total += edits.len();
                    files += usize::from(!edits.is_empty());
                    editor.view.replace_ranges(edits);
                }
                continue;
            }
            let Ok(bytes) = std::fs::read(&path) else { continue };
            let text = String::from_utf8_lossy(&bytes).into_owned();
            let mut options = TextModel::default_creation_options();
            options.detect_indentation = false;
            let model = TextModel::new(TextSource::from(text.as_str()), "plaintext", options, None, UndoRedoService::for_tests());
            let edits = edits_for(&model);
            if edits.is_empty() {
                continue;
            }
            total += edits.len();
            files += 1;
            let ops: Vec<_> = edits.into_iter().map(|(r, t)| je_editor::edit_operation::EditOperation::replace(r, Some(je_base::JsString::from(t.as_str())))).collect();
            model.apply_edits(&ops);
            let new_text = model.get_value_with(EndOfLinePreference::TextDefined, true).to_string_lossy();
            if let Err(e) = std::fs::write(&path, new_text.as_bytes()) {
                self.notify(Severity::Error, format!("Falha ao gravar '{}': {e}", path.display()));
            }
        }
        self.notify(Severity::Info, format!("Replaced {total} occurrence{} across {files} file{}.", if total == 1 { "" } else { "s" }, if files == 1 { "" } else { "s" }));
        self.start_search();
    }

    /// Trata uma tecla com o campo da vista Search focado. Devolve se consumiu.
    fn search_key(&mut self, key_code: KeyCode, text: Option<&str>, key: Option<EditorKey>, mods: Modifiers) -> bool {
        let ctrl = mods.ctrl || mods.meta;
        let view = &mut self.search;
        if key == Some(EditorKey::Tab) && view.replace_visible {
            view.in_replace = !view.in_replace;
            self.needs_render = true;
            return true;
        }
        match key {
            Some(EditorKey::Escape) => {
                self.search_focused = false;
                if let Some(editor) = self.active_editor_mut() {
                    editor.view.set_focus(true);
                }
                self.needs_render = true;
                return true;
            }
            Some(EditorKey::Enter) if view.in_replace && ctrl && mods.alt => {
                self.search_replace_all();
                return true;
            }
            Some(EditorKey::Enter) if !view.in_replace => {
                self.start_search();
                return true;
            }
            _ => {}
        }
        let in_replace = view.in_replace;
        let field = if in_replace { &mut view.replace } else { &mut view.field };
        if input::edit(field, key_code, key, mods, text, &input::SystemClipboard::new()) == Some(true) && !in_replace {
            self.schedule_search();
        }
        self.needs_render = true;
        true
    }

    fn refresh_explorer(&mut self) {
        if let Some(explorer) = &self.explorer {
            explorer.refresh();
        }
        let selected = self.active_editor().and_then(|e| e.path.clone());
        if let Some(explorer) = &self.explorer {
            explorer.reveal(selected.as_deref());
        }
        self.workspace_files = None;
        self.needs_render = true;
    }

    /// A confirmação de apagar (`explorer.confirmDelete`): o diálogo de mover para a lixeira.
    fn confirm_delete(&mut self, path: PathBuf) {
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let (button, detail) = if cfg!(windows) { ("&&Move to Recycle Bin", "You can restore this file from the Recycle Bin.") } else { ("&&Move to Trash", "You can restore this file from the Trash.") };
        self.show_dialog(&format!("Are you sure you want to delete '{name}'?"), Some(detail), je_widgets::dialog::DialogType::Question, vec![(button, Command::DeletePath(path)), ("Cancel", Command::CloseDialog)]);
    }

    fn close_context_menu(&mut self) {
        if let Some((menu, slot)) = self.context_menu.take() {
            IDisposable::dispose(&*menu);
            self.ui.remove(slot);
            self.needs_render = true;
        }
    }

    /// O menu de contexto de um item do explorador (`explorer/context`): os comandos que o shell já tem.
    fn open_explorer_menu(&mut self, path: &Path) {
        use je_base::actions::{Action, ActionCallback, ActionRef, Separator};
        let queue = self.queue.clone();
        let item = |label: &str, command: Command| -> ActionRef {
            let queue = queue.clone();
            let callback: ActionCallback = Rc::new(move |_| {
                queue.borrow_mut().push(command.clone());
                je_base::dynamic::Dynamic::Undefined.into()
            });
            Action::new(label, label, "", true, Some(callback)) as ActionRef
        };
        let is_dir = path.is_dir();
        let parent = if is_dir { path.to_path_buf() } else { path.parent().map(Path::to_path_buf).unwrap_or_else(|| path.to_path_buf()) };
        let mut actions: Vec<ActionRef> = Vec::new();
        if is_dir {
            actions.push(item("New File...", Command::PromptNewFile(parent.clone())));
            actions.push(item("New Folder...", Command::PromptNewFolder(parent.clone())));
            actions.push(Separator::new() as ActionRef);
        }
        actions.push(item(if cfg!(windows) { "Reveal in File Explorer" } else if cfg!(target_os = "macos") { "Reveal in Finder" } else { "Open Containing Folder" }, Command::RevealInFileManager(path.to_path_buf())));
        actions.push(item("Open in Integrated Terminal", Command::OpenInTerminal(path.to_path_buf())));
        actions.push(Separator::new() as ActionRef);
        actions.push(item("Copy Path", Command::CopyPath(path.to_path_buf(), false)));
        actions.push(item("Copy Relative Path", Command::CopyPath(path.to_path_buf(), true)));
        actions.push(Separator::new() as ActionRef);
        actions.push(item("Rename...", Command::PromptRename(path.to_path_buf())));
        actions.push(item("Delete", Command::ConfirmDelete(path.to_path_buf())));
        self.open_menu(actions);
    }

    /// Abre um menu de contexto com `actions` na posição do ponteiro.
    fn open_menu(&mut self, actions: Vec<je_base::actions::ActionRef>) {
        self.close_context_menu();
        let (x, y) = self.last_pointer;
        let slot = {
            let mut tree = self.ui.tree_mut();
            let root = tree.root();
            let slot = tree.create(root, Role::Generic);
            tree.set_bounds(slot, Rect::new(x, y, x, y));
            slot
        };
        let queue = self.queue.clone();
        let menu = je_widgets::menu::Menu::new(&self.ui, slot, &actions, je_widgets::menu::IMenuOptions::default(), Rc::new(default_menu_styles(&self.theme)));
        let _ = menu.on_did_cancel().listen(move |_| queue.borrow_mut().push(Command::CloseContextMenu));
        // Cabe na janela: sobe ou recua o que passar da borda.
        let (width, height) = self.size;
        let bounds = self.ui.tree().children(slot).first().map(|c| self.ui.tree().absolute_bounds(*c));
        if let Some(b) = bounds {
            let (dx, dy) = ((width - 4.0 - b.x1).min(0.0), (height - 4.0 - b.y1).min(0.0));
            if dx < 0.0 || dy < 0.0 {
                self.ui.tree_mut().set_bounds(slot, Rect::new(x + dx, y + dy, x + dx, y + dy));
            }
        }
        menu.focus(true);
        self.context_menu = Some((menu, slot));
        self.needs_render = true;
    }

    /// O prompt de nome do explorador (novo arquivo, nova pasta, renomear).
    fn open_prompt(&mut self, action: PromptAction) {
        let (placeholder, initial) = match &action {
            PromptAction::NewFile(_) => ("Type file name. Press Enter to confirm or Escape to cancel.".to_owned(), String::new()),
            PromptAction::NewFolder(_) => ("Type folder name. Press Enter to confirm or Escape to cancel.".to_owned(), String::new()),
            PromptAction::Rename(path) => ("Type file name. Press Enter to confirm or Escape to cancel.".to_owned(), path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()),
            PromptAction::GoToLine => {
                let (line, column, last) = self.active_editor().map_or((1, 1, 1), |e| {
                    let p = e.view.cursor_position();
                    (p.line_number, p.column, e.view.model().get_line_count())
                });
                (format!("Current Line: {line}, Character: {column}. Type a line number between 1 and {last} to navigate to."), String::new())
            }
            PromptAction::SaveAs(index) => {
                let base = self.workspace.clone().or_else(|| std::env::current_dir().ok()).unwrap_or_default();
                let suggestion = self.editors.get(*index).map(|e| e.path.clone().unwrap_or_else(|| base.join(&e.name))).unwrap_or(base);
                ("Type the path to save the file as. Press Enter to confirm or Escape to cancel.".to_owned(), suggestion.display().to_string())
            }
            PromptAction::OpenFile => ("Type the path of a file to open. Press Enter to confirm or Escape to cancel.".to_owned(), self.workspace.as_deref().map(|w| format!("{}{}", w.display(), std::path::MAIN_SEPARATOR)).unwrap_or_default()),
            PromptAction::OpenFolder => ("Type the path of a folder to open. Press Enter to confirm or Escape to cancel.".to_owned(), self.workspace.as_deref().and_then(Path::parent).map(|w| format!("{}{}", w.display(), std::path::MAIN_SEPARATOR)).unwrap_or_default()),
        };
        self.quick_pick = Some(QuickPick::prompt(&placeholder, &initial, action));
        self.ui.blur();
    }

    fn run_prompt(&mut self, action: PromptAction, text: &str) {
        let text = text.trim();
        if text.is_empty() {
            return;
        }
        let result = match &action {
            PromptAction::GoToLine => {
                // `linha` ou `linha:coluna` (o `:` inicial do Go to Line do VS Code também vale).
                let mut parts = text.trim_start_matches(':').split(':').map(|p| p.trim().parse::<i32>());
                match (parts.next(), parts.next()) {
                    (Some(Ok(line)), column) => {
                        if let Some(editor) = self.active_editor_mut() {
                            let last = editor.view.model().get_line_count();
                            let line = line.clamp(1, last);
                            let max_column = editor.view.model().get_line_max_column(line);
                            let column = column.and_then(Result::ok).unwrap_or(1).clamp(1, max_column);
                            editor.view.select_range(je_editor::range::Range::new(line, column, line, column));
                        }
                        Ok(None)
                    }
                    _ => Err("Digite um número de linha válido.".to_owned()),
                }
            }
            PromptAction::SaveAs(index) => match file_ops::resolve(text, self.workspace.as_deref()) {
                Ok(path) if path.is_dir() => Err(format!("'{}' é uma pasta.", path.display())),
                Ok(path) => {
                    let model_text = self.editors.get(*index).map(|e| e.view.model().get_value_with(EndOfLinePreference::TextDefined, true).to_string_lossy());
                    match model_text {
                        Some(content) => file_ops::write_new(&path, &content).map(|()| {
                            let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                            self.editors[*index].path = Some(path.clone());
                            self.editors[*index].name = name;
                            // Relê do disco: a linguagem e os ícones seguem o nome novo.
                            self.revert(*index);
                            if std::mem::take(&mut self.close_after_save_as) {
                                self.close_editor(*index);
                                self.continue_exit();
                            }
                            None
                        }),
                        None => Ok(None),
                    }
                }
                Err(e) => Err(e),
            },
            PromptAction::OpenFile => match file_ops::resolve(text, self.workspace.as_deref()) {
                Ok(path) if path.is_file() => Ok(Some(path)),
                Ok(path) => Err(format!("O arquivo '{}' não existe.", path.display())),
                Err(e) => Err(e),
            },
            PromptAction::OpenFolder => match file_ops::resolve(text, self.workspace.as_deref()) {
                Ok(path) if path.is_dir() => {
                    self.set_workspace(path);
                    Ok(None)
                }
                Ok(path) => Err(format!("A pasta '{}' não existe.", path.display())),
                Err(e) => Err(e),
            },
            PromptAction::NewFile(dir) => file_ops::create_file(dir, text).map(Some),
            PromptAction::NewFolder(dir) => file_ops::create_folder(dir, text).map(|_| None),
            PromptAction::Rename(path) => file_ops::rename(path, text).map(|new| {
                // As abas dos arquivos renomeados seguem o novo caminho.
                for editor in &mut self.editors {
                    if let Some(old) = editor.path.clone()
                        && let Ok(rest) = old.strip_prefix(path)
                    {
                        let moved = new.join(rest);
                        editor.name = moved.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                        editor.path = Some(moved);
                    }
                }
                None
            }),
        };
        match result {
            Ok(opened) => {
                self.refresh_explorer();
                if let Some(path) = opened {
                    let _ = self.open_file(&path);
                }
            }
            Err(e) => self.notify(Severity::Error, e),
        }
        self.needs_render = true;
    }

    /// Abre o Quick Open (`initial` vazio) ou a paleta (`>`).
    fn open_quick_pick(&mut self, initial: &str) {
        let files = match &self.workspace_files {
            Some(files) => files.clone(),
            None => {
                let excludes = excludes::Excludes::search(&themes::user_settings());
                let files = self.workspace.as_deref().map(|w| quick_pick::scan_workspace(w, &excludes)).unwrap_or_default();
                self.workspace_files = Some(files.clone());
                files
            }
        };
        let open: Vec<PathBuf> = self.active.into_iter().chain((0..self.editors.len()).filter(|i| Some(*i) != self.active)).filter_map(|i| self.editors[i].path.clone()).collect();
        let recents = quick_pick::recent_items(&open, self.workspace.as_deref());
        self.quick_pick = Some(QuickPick::new(files, recents, self.palette_items.clone(), initial));
        self.ui.blur();
    }

    /// Trata uma tecla com o Quick Open aberto. Devolve se consumiu.
    fn quick_pick_key(&mut self, down: bool, key_code: KeyCode, text: Option<&str>, key: Option<EditorKey>, mods: Modifiers) -> bool {
        let Some(pick) = self.quick_pick.as_mut() else { return false };
        if !down {
            return true;
        }
        let ctrl = mods.ctrl || mods.meta;
        match key {
            Some(EditorKey::Escape) => {
                self.quick_pick = None;
                self.exiting = false;
                if let Some((path, ui_theme, index)) = self.theme_before_preview.take() {
                    self.apply_theme_file(&path, &ui_theme, index);
                }
            }
            Some(EditorKey::Enter) => {
                let command = pick.prompt_result().map(|(action, text)| Command::Prompt(action, text)).or_else(|| pick.accept(None));
                self.quick_pick = None;
                if let Some(command) = command {
                    self.run(command);
                }
            }
            Some(EditorKey::Up) => pick.move_focus(-1),
            Some(EditorKey::Down) => pick.move_focus(1),
            Some(EditorKey::PageUp) => pick.move_focus(-10),
            Some(EditorKey::PageDown) => pick.move_focus(10),
            _ => {
                pick.edit(key_code, key, mods, text);
            }
        }
        let _ = ctrl;
        self.needs_render = true;
        if self.theme_before_preview.is_some() {
            self.preview_theme();
        }
        true
    }

    fn open_find(&mut self) {
        let Some(editor) = self.active_editor() else { return };
        let seed = editor.view.seed_search_string();
        let origin = editor.view.cursor_position();
        let keep = self.find.as_ref().map(|f| (f.match_case, f.whole_word, f.regex));
        let mut find = FindWidget::new(&seed, origin);
        if let Some((case, word, regex)) = keep {
            (find.match_case, find.whole_word, find.regex) = (case, word, regex);
        } else if seed.is_empty() && let Some(previous) = self.last_find.clone() {
            find = FindWidget::new(&previous, origin);
        }
        self.find = Some(find);
        self.find_focused = true;
        self.ui.blur();
        self.find_changed(true);
    }

    fn close_find(&mut self) {
        if let Some(find) = self.find.take() {
            self.last_find = Some(find.text());
        }
        self.find_focused = false;
        if let Some(editor) = self.active_editor_mut() {
            editor.view.set_find_matches(Vec::new(), None);
            editor.view.set_focus(true);
        }
        self.needs_render = true;
    }

    /// O valor ou as chaves do campo mudaram: recalcula as ocorrências e seleciona a primeira a partir da origem.
    fn find_changed(&mut self, select: bool) {
        let (Some(find), Some(active)) = (self.find.as_mut(), self.active) else { return };
        let editor = &mut self.editors[active];
        find.search(editor.view.model());
        let found = if select { find.select_from(find.origin) } else { None };
        editor.view.set_find_matches(find.matches.clone(), find.current);
        if let Some(range) = found {
            editor.view.select_range(range);
        }
        self.needs_render = true;
    }

    fn find_step(&mut self, delta: isize) {
        if self.find.is_none() {
            self.open_find();
            return;
        }
        let (Some(find), Some(active)) = (self.find.as_mut(), self.active) else { return };
        let editor = &mut self.editors[active];
        if find.version != editor.view.model().get_version_id() {
            find.search(editor.view.model());
        }
        // A busca continua da seleção do editor, não da ocorrência anterior, se o cursor andou.
        let from = editor.view.cursor_selection_range();
        let range = match find.current.and_then(|i| find.matches.get(i)) {
            Some(current) if *current == from => find.step(delta),
            _ => {
                let start = if delta > 0 { from.get_end_position() } else { from.get_start_position() };
                let at = if delta > 0 { find.matches.iter().position(|m| !m.get_start_position().is_before(&start)) } else { find.matches.iter().rposition(|m| m.get_end_position().is_before(&start) || *m == from && false) };
                let index = at.or(if find.matches.is_empty() { None } else if delta > 0 { Some(0) } else { Some(find.matches.len() - 1) });
                find.current = index;
                index.map(|i| find.matches[i])
            }
        };
        editor.view.set_find_matches(find.matches.clone(), find.current);
        if let Some(range) = range {
            editor.view.select_range(range);
        }
        self.needs_render = true;
    }

    /// `Replace` troca a ocorrência atual e vai à próxima; `Replace All` troca todas numa edição só.
    fn find_replace(&mut self, all: bool) {
        let (Some(find), Some(active)) = (self.find.as_mut(), self.active) else { return };
        let editor = &mut self.editors[active];
        if find.version != editor.view.model().get_version_id() {
            find.search(editor.view.model());
        }
        let model = editor.view.model().clone();
        let text_of = |r: &je_editor::range::Range| model.get_value_in_range(r).to_string_lossy();
        if all {
            let edits: Vec<_> = find.matches.iter().map(|r| (*r, find.replacement_for(&text_of(r)))).collect();
            let count = edits.len();
            editor.view.replace_ranges(edits);
            find.search(editor.view.model());
            find.current = None;
            editor.view.set_find_matches(find.matches.clone(), None);
            if count > 0 {
                self.notify(Severity::Info, format!("Replaced {count} occurrence{}.", if count == 1 { "" } else { "s" }));
            }
        } else {
            // Sem ocorrência atual, a primeira a partir do cursor vira a atual antes de trocar.
            let current = find.current.and_then(|i| find.matches.get(i)).copied().or_else(|| find.select_from(editor.view.cursor_position()));
            if let Some(range) = current {
                let replacement = find.replacement_for(&text_of(&range));
                editor.view.replace_ranges(vec![(range, replacement)]);
                find.search(editor.view.model());
                let next = find.select_from(range.get_start_position());
                editor.view.set_find_matches(find.matches.clone(), find.current);
                if let Some(next) = next {
                    editor.view.select_range(next);
                }
            }
        }
        self.needs_render = true;
    }

    /// Trata uma tecla com o campo da busca focado. Devolve se consumiu.
    fn find_key(&mut self, key_code: KeyCode, text: Option<&str>, key: Option<EditorKey>, mods: Modifiers) -> bool {
        let Some(find) = self.find.as_mut() else { return false };
        // Tab alterna entre os campos da busca e da substituição.
        if key == Some(EditorKey::Tab) && find.replace_visible {
            find.in_replace = !find.in_replace;
            self.needs_render = true;
            return true;
        }
        let ctrl = mods.ctrl || mods.meta;
        match key {
            Some(EditorKey::Escape) => {
                self.close_find();
                return true;
            }
            Some(EditorKey::Enter) if find.in_replace => {
                self.find_replace(ctrl && mods.alt);
                return true;
            }
            Some(EditorKey::Enter) => {
                self.find_step(if mods.shift { -1 } else { 1 });
                return true;
            }
            _ => {}
        }
        let in_replace = find.in_replace;
        let field = if in_replace { &mut find.replace } else { &mut find.field };
        let changed = input::edit(field, key_code, key, mods, text, &input::SystemClipboard::new());
        if changed == Some(true) && !in_replace {
            self.find_origin_refresh();
            self.find_changed(true);
        }
        self.needs_render = true;
        true
    }

    /// A busca parte do cursor do editor.
    fn find_origin_refresh(&mut self) {
        if let (Some(editor), Some(find)) = (self.active.map(|i| &self.editors[i]), self.find.as_mut()) {
            find.origin = editor.view.cursor_selection_range().get_start_position();
        }
    }

    fn render_find(&mut self, chrome: NodeId) {
        let Some(minimap) = self.active_editor().map(|e| e.view.layout().minimap_width()) else { return };
        let editor_rect = self.editor_rect();
        let Some(find) = self.find.as_mut() else { return };
        let rect = find.rect(editor_rect, minimap);
        let hits = find.render(&mut self.ui.tree_mut(), chrome, rect, self.find_focused, &self.theme);
        for (node, command) in hits {
            self.listen_command(node, command);
        }
    }

    /// A área do editor mudou (painel, barra lateral): cada editor se ajusta.
    fn resize_editors(&mut self) {
        let rect = self.editor_rect();
        for editor in &mut self.editors {
            editor.resize(rect.width(), rect.height());
        }
        self.needs_render = true;
    }

    /// O fator de zoom da janela: `1.2 ^ zoomLevel`, como o Electron.
    pub fn zoom_factor(&self) -> f64 {
        1.2_f64.powf(self.zoom_level)
    }

    pub fn take_exit(&mut self) -> bool {
        std::mem::take(&mut self.exit_requested)
    }

    pub fn take_fullscreen(&mut self) -> bool {
        std::mem::take(&mut self.fullscreen_requested)
    }

    pub fn resize(&mut self, width: f64, height: f64) {
        if self.size != (width, height) {
            self.size = (width, height);
            self.ui.tree_mut().set_bounds(self.ui.root(), Rect::new(0.0, 0.0, width, height));
            self.needs_render = true;
        }
    }

    /// A borda de baixo das partes flutuantes (activity bar, side bar, editor).
    fn parts_bottom(&self) -> f64 {
        self.size.1 - if self.statusbar_visible { STATUS_HEIGHT + PART_BOTTOM_GAP } else { PART_BOTTOM_GAP }
    }

    fn sidebar_rect(&self) -> Option<Rect> {
        let left = ACTIVITY_LEFT + ACTIVITY_WIDTH;
        self.sidebar_visible.then(|| Rect::new(left, TITLE_HEIGHT, left + self.sidebar_width, self.parts_bottom()))
    }

    /// A área de todos os grupos de editores.
    fn editor_part_rect(&self) -> Rect {
        let left = self.sidebar_rect().map_or(ACTIVITY_LEFT + ACTIVITY_WIDTH + SASH, |r| r.x1 + SASH);
        let bottom = if self.panel_visible { self.parts_bottom() - self.panel_height - SASH } else { self.parts_bottom() };
        Rect::new(left, TITLE_HEIGHT, self.size.0 - 4.0, bottom)
    }

    /// O painel inferior (abas Problems, Output, Debug Console, Terminal, Ports), debaixo do grupo de editores.
    fn panel_rect(&self) -> Option<Rect> {
        self.panel_visible.then(|| {
            let editor = self.editor_part_rect();
            Rect::new(editor.x0, self.parts_bottom() - self.panel_height, editor.x1, self.parts_bottom())
        })
    }

    /// A área da grade do terminal: abaixo da faixa de título do painel, dentro da borda de 1 px.
    fn terminal_area(&self) -> Option<Rect> {
        self.panel_rect().map(|p| Rect::new(p.x0 + 1.0, p.y0 + 1.0 + PANEL_TITLE_HEIGHT, p.x1 - 1.0, p.y1 - 1.0))
    }

    /// O painel do grupo `index`, com a borda de 1 px: as colunas dividem a área por igual, com o sash entre elas.
    fn group_part_rect(&self, index: usize) -> Rect {
        let part = self.editor_part_rect();
        let n = self.groups.len().max(1) as f64;
        let width = (part.width() - SASH * (n - 1.0)) / n;
        let x0 = (part.x0 + (width + SASH) * index as f64).round();
        let x1 = if index + 1 == self.groups.len() { part.x1 } else { (part.x0 + (width + SASH) * index as f64 + width).round() };
        Rect::new(x0, part.y0, x1, part.y1)
    }

    /// A área de texto de um grupo: abaixo das abas e dos breadcrumbs, dentro da borda.
    fn group_editor_rect(&self, index: usize) -> Rect {
        let part = self.group_part_rect(index).inset(-1.0);
        Rect::new(part.x0, part.y0 + TABS_HEIGHT + BREADCRUMBS_HEIGHT, part.x1, part.y1)
    }

    /// A área do editor de texto do grupo em foco.
    fn editor_rect(&self) -> Rect {
        self.group_editor_rect(self.group_index)
    }

    /// Põe o foco no grupo `index`: o conteúdo do grupo em foco volta para a vaga dele e o do novo sai dela.
    fn focus_group(&mut self, index: usize) {
        if index == self.group_index || index >= self.groups.len() {
            return;
        }
        let Some(incoming) = self.groups[index].take() else { return };
        if let Some(editor) = self.active_editor_mut() {
            editor.view.set_focus(false);
        }
        let outgoing = Group { editors: std::mem::replace(&mut self.editors, incoming.editors), active: incoming.active };
        let outgoing = Group { active: std::mem::replace(&mut self.active, outgoing.active), editors: outgoing.editors };
        self.groups[self.group_index] = Some(outgoing);
        self.group_index = index;
        if let Some(active) = self.active {
            self.activate(active);
        }
        self.needs_render = true;
    }

    /// `workbench.action.splitEditor`: um grupo novo à direita com o mesmo documento (o mesmo modelo).
    fn split_editor(&mut self) {
        let Some(editor) = self.active_editor() else { return };
        let rect = self.editor_rect();
        let mut view = EditorView::new(editor.view.model().clone(), rect.width(), rect.height(), EditorColors::from_theme(&self.theme));
        if let Some(env) = &self.environment {
            view.set_token_colors(&env.color_map());
        }
        let diff = editor.diff.as_ref().map(|d| d.original().model().clone()).map(|original| self.new_diff(original, &mut view));
        let copy = OpenEditor { path: editor.path.clone(), name: editor.name.clone(), language_name: editor.language_name.clone(), view, saved_version: editor.saved_version, head: editor.head.clone(), diff_version: -1, disk_mtime: editor.disk_mtime, preview: false, diff };
        self.groups.insert(self.group_index + 1, Some(Group { editors: vec![copy], active: Some(0) }));
        self.focus_group(self.group_index + 1);
        self.resize_editors();
    }

    /// Fecha o grupo em foco quando ele fica sem editores e há outro (o `workbench.editor.closeEmptyGroups`).
    fn close_empty_group(&mut self) {
        if !self.editors.is_empty() || self.groups.len() < 2 {
            return;
        }
        let closing = self.group_index;
        let next = if closing + 1 < self.groups.len() { closing + 1 } else { closing - 1 };
        self.focus_group(next);
        self.groups.remove(closing);
        if self.group_index > closing {
            self.group_index -= 1;
        }
        self.resize_editors();
    }

    /// Todos os editores abertos, de todos os grupos.
    fn all_editors_mut(&mut self) -> impl Iterator<Item = &mut OpenEditor> {
        self.editors.iter_mut().chain(self.groups.iter_mut().flatten().flat_map(|g| g.editors.iter_mut()))
    }

    /// Mostra a dica de `text` junto de `node` (o `IHoverService.showDelayedHover` com o estilo de ponteiro).
    fn show_hover(&mut self, node: NodeId, text: &str, position: je_widgets::hover_api::HoverPosition) {
        use je_widgets::hover_api::{HoverContent, HoverStyle, HoverTarget, IHoverOptions, IHoverPositionOptions};
        self.hide_hover();
        let mut options = IHoverOptions::new(HoverContent::Text(text.to_owned()), HoverTarget::Node(node));
        options.style = Some(HoverStyle::Pointer);
        options.position = Some(IHoverPositionOptions { hover_position: Some(position), anchor_alignment: None, force_position: None, clipping_element: None });
        let hover = je_widgets::hover_widget::WorkbenchHoverWidget::new(&self.ui, options, je_widgets::hover_widget::default_hover_styles(&self.theme), None);
        // A dica vai numa `ContextView` sobre a janela, como o `HoverService` faz: ela posiciona a caixa pela âncora.
        let view = self.context_view.get_or_insert_with(|| {
            let root = self.ui.root();
            je_widgets::context_view::ContextView::new(&self.ui, root, je_widgets::context_view_api::ContextViewDOMPosition::Absolute)
        });
        view.show(je_widgets::hover_widget::HoverContextViewDelegate::new(hover.clone(), false, 0));
        self.hover = Some(hover);
        // Só um quadro novo: remontar a moldura mataria o nó alvo da dica.
        self.redraw = true;
    }

    fn hide_hover(&mut self) {
        if let Some(hover) = self.hover.take() {
            if let Some(view) = &self.context_view {
                view.hide();
            }
            IDisposable::dispose(&*hover);
            self.redraw = true;
        }
    }

    /// Liga a dica `text` ao nó: aparece depois de 500 ms com o ponteiro parado nele e some ao sair.
    fn listen_hover(&self, node: NodeId, text: String, position: je_widgets::hover_api::HoverPosition) {
        let (request, queue) = (self.hover_request.clone(), self.queue.clone());
        let _ = self.ui.listen(node, EventKind::PointerEnter, false, move |_| {
            let generation = request.borrow().as_ref().map_or(0, |r| r.0) + 1;
            *request.borrow_mut() = Some((generation, node, text.clone(), position));
            let queue = queue.clone();
            je_base::runtime::set_timeout(move || queue.borrow_mut().push(Command::ShowHover(generation)), 500);
        });
        let (request, queue) = (self.hover_request.clone(), self.queue.clone());
        let _ = self.ui.listen(node, EventKind::PointerLeave, false, move |_| {
            if request.borrow().as_ref().is_some_and(|r| r.1 == node) {
                queue.borrow_mut().push(Command::HideHover);
            }
        });
    }

    fn listen_command(&self, node: NodeId, command: Command) {
        let queue = self.queue.clone();
        // Os nós da moldura morrem no próximo `render`, e com eles o ouvinte.
        let _ = self.ui.listen(node, EventKind::Click, false, move |_| queue.borrow_mut().push(command.clone()));
    }

    /// Remonta a moldura para o estado atual.
    pub fn render(&mut self) {
        self.needs_render = false;
        // A moldura é remontada e o nó alvo da dica morre com ela: a dica some junto.
        *self.hover_request.borrow_mut() = None;
        self.hide_hover();
        let (width, height) = self.size;
        let old = self.nodes.chrome;
        let chrome = {
            let mut tree = self.ui.tree_mut();
            let root = tree.root();
            let chrome = tree.create_detached(Role::Generic);
            tree.insert_before(root, chrome, Some(old));
            tree.set_bounds(chrome, Rect::new(0.0, 0.0, width, height));
            chrome
        };
        self.ui.remove(old);
        self.nodes.chrome = chrome;
        self.render_title_bar(chrome);
        self.render_activity_bar(chrome);
        self.render_sidebar(chrome);
        self.render_editor_part(chrome);
        self.render_panel(chrome);
        if self.statusbar_visible {
            self.render_status_bar(chrome);
        }
        self.render_find(chrome);
        self.render_toasts(chrome);
        self.render_quick_pick(chrome);
    }

    fn render_quick_pick(&mut self, chrome: NodeId) {
        let Some(pick) = self.quick_pick.as_mut() else { return };
        let rows = pick.render(&mut self.ui.tree_mut(), chrome, self.size.0, &self.theme, self.icons.as_ref(), &self.language_of);
        for (row, index) in rows {
            self.listen_command(row, Command::QuickPickAccept(index));
        }
    }

    fn render_title_bar(&self, chrome: NodeId) {
        let (width, _) = self.size;
        let fg = self.color("titleBar.activeForeground");
        let mut tree = self.ui.tree_mut();
        let title = tree.create(chrome, Role::Region);
        tree.set_bounds(title, Rect::new(0.0, 0.0, width, TITLE_HEIGHT));
        tree.style_mut(title).background = Some(self.color("titleBar.activeBackground"));
        // O ícone do aplicativo (`.window-appicon`, 35 x 35).
        let icon = tree.create(title, Role::Image);
        tree.set_bounds(icon, Rect::new(0.0, 0.0, 35.0, 35.0));
        tree.style_mut(icon).content = Content::Codicon { id: "vscode".into(), size: 16.0, color: self.color("textLink.foreground") };
        // O centro: voltar, avançar e o command center, centrados na janela.
        let center_width = 26.0 * 2.0 + 6.0 + (width * 0.38).min(600.0) + 2.0;
        // O centro fica 3 px à esquerda do meio exato da janela (medido no oráculo a 1200 px).
        let x = ((width - center_width) / 2.0 - 3.0).max(0.0).round();
        for (i, id) in ["arrow-left", "arrow-right"].iter().enumerate() {
            let button = tree.create(title, Role::Button);
            tree.set_bounds(button, Rect::new(x + 26.0 * i as f64, 6.5, x + 26.0 * i as f64 + 22.0, 28.5));
            let disabled = Rgba { a: 0x66, ..fg };
            tree.style_mut(button).content = Content::Codicon { id: (*id).into(), size: 16.0, color: disabled };
        }
        let cc_left = x + 58.0;
        let cc = tree.create(title, Role::Button);
        tree.set_bounds(cc, Rect::new(cc_left, 5.5, x + center_width, 29.5));
        let style = tree.style_mut(cc);
        style.background = Some(self.color("commandCenter.background"));
        style.border = Some(Border { width: 1.0, color: self.color("commandCenter.border") });
        style.radius = 6.0;
        let label = tree.create(cc, Role::Generic);
        let text = self.workspace_name().or_else(|| self.active_editor().map(|e| e.name.clone())).unwrap_or_else(|| "Search".to_owned());
        tree.set_bounds(label, Rect::new(9.0, 0.0, center_width - 58.0 - 9.0, 24.0));
        tree.style_mut(label).content = Content::Text { text, style: ui_font(13.0, 400), color: self.color("commandCenter.foreground"), line_height: 24.0 };
        // À direita: os botões de layout (`.titlebar-right .action-item`, 22 x 22 a cada 26 px), antes dos 100 px
        // dos controles da janela.
        let ids = ["layout", if self.sidebar_visible { "layout-sidebar-left" } else { "layout-sidebar-left-off" }, if self.panel_visible { "layout-panel" } else { "layout-panel-off" }, "layout-sidebar-right-off"];
        let right = width - 100.0 - 26.0 * ids.len() as f64 + 4.0;
        for (i, id) in ids.iter().enumerate() {
            let button = tree.create(title, Role::Button);
            let bx = right + 26.0 * i as f64;
            tree.set_bounds(button, Rect::new(bx, 6.5, bx + 22.0, 28.5));
            let style = tree.style_mut(button);
            style.content = Content::Codicon { id: (*id).into(), size: 16.0, color: fg };
            style.radius = 5.0;
            style.cursor = Some(Cursor::Pointer);
            if i == 1 || i == 2 {
                drop(tree);
                self.listen_command(button, if i == 1 { Command::ToggleSidebar } else { Command::TogglePanel });
                tree = self.ui.tree_mut();
            }
        }
        // A barra de menus ocupa de x = 35 até o começo do centro.
        tree.set_bounds(self.nodes.menubar, Rect::new(35.0, 0.0, (x - 30.0).max(35.0), TITLE_HEIGHT));
        drop(tree);
        self.menubar.update(None);
    }

    fn render_activity_bar(&self, chrome: NodeId) {
        let bottom = self.parts_bottom();
        let foreground = self.color("activityBar.foreground");
        let inactive = self.color("activityBar.inactiveForeground");
        let mut tree = self.ui.tree_mut();
        let bar = tree.create(chrome, Role::Toolbar);
        tree.set_bounds(bar, Rect::new(ACTIVITY_LEFT, TITLE_HEIGHT, ACTIVITY_LEFT + ACTIVITY_WIDTH, bottom));
        let style = tree.style_mut(bar);
        style.background = Some(self.color("activityBar.background"));
        style.border = Some(Border { width: 1.0, color: self.color("sideBar.border") });
        style.corner_radii = Some(if self.sidebar_visible { [RADIUS, 0.0, 0.0, RADIUS] } else { [RADIUS; 4] });
        if self.sidebar_visible {
            style.border_widths = Some([1.0, 0.0, 1.0, 1.0]);
        }
        let mut view_buttons = Vec::new();
        for (i, view) in SideView::ALL.into_iter().enumerate() {
            let button = tree.create(bar, Role::Button);
            tree.set_bounds(button, Rect::new(4.0, 4.0 + 44.0 * i as f64, 40.0, 40.0 + 44.0 * i as f64));
            let checked = view == self.side_view && self.sidebar_visible;
            let style = tree.style_mut(button);
            style.content = Content::Codicon { id: view.icon().into(), size: 24.0, color: if checked { foreground } else { inactive } };
            style.radius = 4.0;
            style.cursor = Some(Cursor::Pointer);
            if checked {
                style.background = Some(self.color("activityBar.activeBackground"));
            }
            if view == SideView::SourceControl {
                let count = self.scm.status.borrow().count();
                if count > 0 && self.git_root.is_some() {
                    let text = if count > 99 { "99+".to_owned() } else { count.to_string() };
                    let badge_font = ui_font(9.0, 600);
                    let w = (measure_text(&text, &badge_font) + 8.0).max(16.0).round();
                    let badge = tree.create(button, Role::Generic);
                    tree.set_bounds(badge, Rect::new(18.0, 18.0, 18.0 + w, 34.0));
                    let style = tree.style_mut(badge);
                    style.background = Some(self.color("activityBarBadge.background"));
                    style.radius = 8.0;
                    let t = tree.create(badge, Role::Generic);
                    tree.set_bounds(t, Rect::new((w - measure_text(&text, &badge_font)) / 2.0, 0.0, w, 16.0));
                    tree.style_mut(t).content = Content::Text { text, style: badge_font, color: self.color("activityBarBadge.foreground"), line_height: 16.0 };
                }
            }
            view_buttons.push((button, view));
        }
        let height = bottom - TITLE_HEIGHT;
        let mut manage_button = None;
        for (i, id) in ["account", "settings-gear"].iter().enumerate() {
            let button = tree.create(bar, Role::Button);
            let y = height - 4.0 - 44.0 * (2 - i) as f64 + 4.0;
            tree.set_bounds(button, Rect::new(4.0, y, 40.0, y + 36.0));
            let style = tree.style_mut(button);
            style.content = Content::Codicon { id: (*id).into(), size: 24.0, color: inactive };
            style.cursor = Some(Cursor::Pointer);
            if i == 1 {
                manage_button = Some(button);
            }
        }
        drop(tree);
        if let Some(button) = manage_button {
            self.listen_command(button, Command::ShowManageMenu);
        }
        for (button, view) in view_buttons {
            self.listen_command(button, Command::ShowView(view));
            let shortcut = match view {
                SideView::Explorer => "Ctrl+Shift+E",
                SideView::Search => "Ctrl+Shift+F",
                SideView::SourceControl => "Ctrl+Shift+G",
                SideView::Debug => "Ctrl+Shift+D",
                SideView::Extensions => "Ctrl+Shift+X",
            };
            self.listen_hover(button, format!("{} ({shortcut})", view.title()), je_widgets::hover_api::HoverPosition::Right);
        }
        if let Some(button) = manage_button {
            self.listen_hover(button, "Manage".to_owned(), je_widgets::hover_api::HoverPosition::Right);
        }
    }

    fn render_sidebar(&mut self, chrome: NodeId) {
        let Some(rect) = self.sidebar_rect() else {
            let mut tree = self.ui.tree_mut();
            tree.style_mut(self.nodes.explorer).visible = false;
            tree.style_mut(self.nodes.search).visible = false;
            tree.style_mut(self.nodes.scm).visible = false;
            return;
        };
        let fg = self.color("sideBarTitle.foreground");
        let mut tree = self.ui.tree_mut();
        let bar = tree.create(chrome, Role::Region);
        tree.set_bounds(bar, rect);
        let style = tree.style_mut(bar);
        style.background = Some(self.color("sideBar.background"));
        style.border = Some(Border { width: 1.0, color: self.color("sideBar.border") });
        style.corner_radii = Some([0.0, RADIUS, RADIUS, 0.0]);
        // O título (`.composite.title`, 32 px) com o nome em 12 px semi-negrito e o `...` à direita.
        let title = tree.create(bar, Role::Heading);
        tree.set_bounds(title, Rect::new(13.0, 1.0, rect.width() - 40.0, 33.0));
        tree.style_mut(title).content = Content::Text { text: self.side_view.title().to_owned(), style: ui_font(12.0, 600), color: fg, line_height: 32.0 };
        let mut hits: Vec<(NodeId, Command)> = Vec::new();
        match self.side_view {
            SideView::Explorer => {
                let more = tree.create(bar, Role::Button);
                tree.set_bounds(more, Rect::new(rect.width() - 27.0, 6.0, rect.width() - 5.0, 28.0));
                tree.style_mut(more).content = Content::Codicon { id: "ellipsis".into(), size: 16.0, color: fg };
                // A seção da pasta e, embaixo, Outline e Timeline (`.pane-header`, 28 px, 11 px negrito); a Timeline aberta
                // ocupa um terço da altura com o histórico do git do arquivo ativo.
                let section = |tree: &mut Tree, y: f64, label: &str, expanded: bool| -> NodeId {
                    let header = tree.create(bar, Role::Button);
                    tree.set_bounds(header, Rect::new(5.0, y, rect.width() - 6.0, y + 28.0));
                    let chevron = tree.create(header, Role::Generic);
                    tree.set_bounds(chevron, Rect::new(7.0, 6.0, 23.0, 22.0));
                    tree.style_mut(chevron).content = Content::Codicon { id: if expanded { "chevron-down" } else { "chevron-right" }.into(), size: 16.0, color: fg };
                    let text = tree.create(header, Role::Generic);
                    tree.set_bounds(text, Rect::new(25.0, 0.0, rect.width() - 30.0, 28.0));
                    tree.style_mut(text).content = Content::Text { text: label.to_owned(), style: ui_font(11.0, 700), color: fg, line_height: 28.0 };
                    header
                };
                let bottom = rect.height() - 6.0;
                if let Some(name) = self.workspace_name() {
                    section(&mut tree, 33.0, &name, true);
                }
                let timeline_height = if self.timeline_open { (bottom / 3.0).round().max(80.0) } else { 0.0 };
                let timeline_top = bottom - 28.0 - timeline_height;
                section(&mut tree, timeline_top - 28.0, "Outline", false);
                let timeline_header = section(&mut tree, timeline_top, "Timeline", self.timeline_open);
                hits.push((timeline_header, Command::ToggleTimeline));
                if self.timeline_open {
                    let font = ui_font(13.0, 400);
                    let muted = self.color("descriptionForeground");
                    let entries = &self.timeline_entries;
                    if entries.is_empty() {
                        let note = tree.create(bar, Role::Status);
                        tree.set_bounds(note, Rect::new(25.0, timeline_top + 30.0, rect.width() - 12.0, timeline_top + 52.0));
                        tree.style_mut(note).content = Content::Text { text: "The active editor cannot provide timeline information.".to_owned(), style: ui_font(12.0, 400), color: muted, line_height: 22.0 };
                    }
                    let rows = ((timeline_height - 2.0) / 22.0).floor().max(0.0) as usize;
                    for (i, entry) in entries.iter().take(rows).enumerate() {
                        let y = timeline_top + 28.0 + 22.0 * i as f64;
                        let row = tree.create(bar, Role::ListItem);
                        tree.set_bounds(row, Rect::new(5.0, y, rect.width() - 6.0, y + 22.0));
                        let icon = tree.create(row, Role::Image);
                        tree.set_bounds(icon, Rect::new(14.0, 3.0, 30.0, 19.0));
                        tree.style_mut(icon).content = Content::Codicon { id: "git-commit".into(), size: 16.0, color: fg };
                        let when = je_widgets::text::ellipsize(&entry.1, &font, 90.0);
                        let when_width = measure_text(&when, &font);
                        let subject = je_widgets::text::ellipsize(&entry.0, &font, rect.width() - 60.0 - when_width - 8.0);
                        let w = measure_text(&subject, &font);
                        let label = tree.create(row, Role::Generic);
                        tree.set_bounds(label, Rect::new(36.0, 0.0, 36.0 + w, 22.0));
                        tree.style_mut(label).content = Content::Text { text: subject, style: font.clone(), color: fg, line_height: 22.0 };
                        let date = tree.create(row, Role::Generic);
                        tree.set_bounds(date, Rect::new(36.0 + w + 6.0, 0.0, 36.0 + w + 6.0 + when_width, 22.0));
                        tree.style_mut(date).content = Content::Text { text: when, style: font.clone(), color: muted, line_height: 22.0 };
                    }
                }
                // O explorador retido fica entre o cabeçalho da pasta e o Outline.
                let list = Rect::new(rect.x0 + 5.0, rect.y0 + 61.0, rect.x1 - 6.0, rect.y0 + timeline_top - 28.0 - 1.0);
                tree.set_bounds(self.nodes.explorer, list);
                tree.style_mut(self.nodes.explorer).visible = self.explorer.is_some();
                tree.style_mut(self.nodes.search).visible = false;
                tree.style_mut(self.nodes.scm).visible = false;
                drop(tree);
                if let Some(explorer) = &self.explorer {
                    explorer.layout(list.width(), list.height());
                }
            }
            SideView::Search => {
                // As ações do título: atualizar, limpar, nova busca no editor, lista e recolher tudo.
                let actions = [("refresh", Some(Command::SearchNow)), ("clear-all", Some(Command::SearchClear)), ("new-file", None), ("list-flat", None), ("collapse-all", Some(Command::SearchCollapse))];
                for (k, (icon, command)) in actions.into_iter().enumerate() {
                    let x = rect.width() - 27.0 - 26.0 * (4 - k) as f64;
                    let button = tree.create(bar, Role::Button);
                    tree.set_bounds(button, Rect::new(x, 5.0, x + 22.0, 27.0));
                    let style = tree.style_mut(button);
                    style.content = Content::Codicon { id: icon.into(), size: 16.0, color: fg };
                    style.radius = 5.0;
                    if let Some(command) = command {
                        style.cursor = Some(Cursor::Pointer);
                        hits.push((button, command));
                    }
                }
                let (header_hits, top) = self.search.render_header(&mut tree, bar, rect.width(), self.search_focused, &self.theme);
                hits.extend(header_hits);
                let list = Rect::new(rect.x0 + 5.0, rect.y0 + top, rect.x1 - 6.0, rect.y1 - 6.0);
                tree.set_bounds(self.nodes.search, list);
                tree.style_mut(self.nodes.search).visible = true;
                tree.style_mut(self.nodes.explorer).visible = false;
                tree.style_mut(self.nodes.scm).visible = false;
                drop(tree);
                self.search.layout(list.width(), list.height());
            }
            SideView::SourceControl => {
                if self.git_root.is_none() {
                    let note = tree.create(bar, Role::Status);
                    tree.set_bounds(note, Rect::new(13.0, 40.0, rect.width() - 13.0, 80.0));
                    let message = if self.workspace.is_some() { "The folder currently open doesn't have a Git repository." } else { "You have not yet opened a folder." };
                    tree.style_mut(note).content = Content::Text { text: message.to_owned(), style: ui_font(13.0, 400), color: fg, line_height: 20.0 };
                    let mut tree_ref = tree;
                    tree_ref.style_mut(self.nodes.explorer).visible = false;
                    tree_ref.style_mut(self.nodes.search).visible = false;
                    tree_ref.style_mut(self.nodes.scm).visible = false;
                } else {
                    let branch = self.scm.status.borrow().branch.clone();
                    let (header_hits, top) = self.scm.render_header(&mut tree, bar, rect.width(), self.scm_focused, &branch);
                    hits.extend(header_hits);
                    let list = Rect::new(rect.x0 + 5.0, rect.y0 + top + 5.0, rect.x1 - 6.0, rect.y1 - 6.0);
                    tree.set_bounds(self.nodes.scm, list);
                    tree.style_mut(self.nodes.scm).visible = true;
                    tree.style_mut(self.nodes.explorer).visible = false;
                    tree.style_mut(self.nodes.search).visible = false;
                    drop(tree);
                    self.scm.layout(list.width(), list.height());
                }
            }
            SideView::Extensions => {
                if self.extensions.is_none() {
                    self.extensions = Some(find_builtin_extensions().map(|d| extensions_view::scan(&d)).unwrap_or_default());
                }
                // O campo de busca do marketplace filtra as instaladas (o marketplace precisa de rede).
                let input = tree.create(bar, Role::TextInput);
                tree.set_bounds(input, Rect::new(13.0, 39.0, rect.width() - 12.0, 65.0));
                hits.push((input, Command::ExtensionsFocus));
                let style = tree.style_mut(input);
                style.background = Some(self.color("input.background"));
                style.border = Some(Border { width: 1.0, color: if self.extensions_focused { self.color("focusBorder") } else { self.color("input.border") } });
                style.radius = 4.0;
                let colors = input::InputColors { foreground: fg, placeholder: self.color("input.placeholderForeground"), selection: self.color("editor.selectionBackground") };
                input::paint(&mut tree, input, Rect::new(7.0, 1.0, rect.width() - 40.0, 24.0), &mut self.extensions_filter, "Search Extensions in Marketplace", self.extensions_focused, &ui_font(13.0, 400), &colors);
                let filter = self.extensions_filter.value().to_owned();
                let entries = self.extensions.clone().unwrap_or_default();
                let total = extensions_view::render(&mut tree, bar, &entries, &filter, rect.width(), 72.0, rect.height() - 6.0, self.extensions_scroll, &self.theme);
                self.extensions_scroll = self.extensions_scroll.min((total - (rect.height() - 78.0)).max(0.0));
                tree.style_mut(self.nodes.explorer).visible = false;
                tree.style_mut(self.nodes.search).visible = false;
                tree.style_mut(self.nodes.scm).visible = false;
            }
            other => {
                // As vistas ainda sem implementação mostram só o título e um aviso honesto.
                let note = tree.create(bar, Role::Status);
                tree.set_bounds(note, Rect::new(13.0, 40.0, rect.width() - 13.0, 60.0));
                tree.style_mut(note).content = Content::Text { text: format!("{} ainda não existe no je-code.", other.title()), style: ui_font(13.0, 400), color: fg, line_height: 20.0 };
                tree.style_mut(self.nodes.explorer).visible = false;
                tree.style_mut(self.nodes.search).visible = false;
                tree.style_mut(self.nodes.scm).visible = false;
            }
        }
        for (node, command) in hits {
            self.listen_command(node, command);
        }
    }

    fn render_panel(&mut self, chrome: NodeId) {
        let Some(rect) = self.panel_rect() else { return };
        let active_fg = self.color("panelTitle.activeForeground");
        let inactive_fg = self.color("panelTitle.inactiveForeground");
        let font = ui_font(13.0, 400);
        let label = self.terminal.as_ref().map_or("zsh".to_owned(), |t| t.shell_name().to_owned());
        let mut tree = self.ui.tree_mut();
        let panel = tree.create(chrome, Role::Region);
        tree.set_bounds(panel, rect);
        let style = tree.style_mut(panel);
        style.background = Some(self.color("panel.background"));
        style.border = Some(Border { width: 1.0, color: self.color("panel.border") });
        style.radius = RADIUS;
        style.clip = true;
        // As abas (`.composite-bar`): cada uma com 10 px de respiro de cada lado, 24 px de altura, a ativa com fundo.
        let mut x = 2.0;
        let mut tab_hits = Vec::new();
        for tab_kind in PanelTab::ALL {
            let name = tab_kind.title();
            let active = tab_kind == self.panel_tab;
            let text_width = measure_text(name, &font);
            let width = text_width + 20.0;
            let tab = tree.create(panel, Role::Tab);
            tree.set_bounds(tab, Rect::new(x + 2.0, 5.0, x + 2.0 + width, 29.0));
            tab_hits.push((tab, Command::ShowPanelTab(tab_kind)));
            let style = tree.style_mut(tab);
            style.radius = 5.0;
            style.cursor = Some(Cursor::Pointer);
            if active {
                style.background = Some(self.color("list.inactiveSelectionBackground"));
            }
            let text = tree.create(tab, Role::Generic);
            tree.set_bounds(text, Rect::new(10.0, 0.0, 10.0 + text_width, 24.0));
            tree.style_mut(text).content = Content::Text { text: name.to_owned(), style: font.clone(), color: if active { self.color("panelTitle.activeForeground") } else { inactive_fg }, line_height: 24.0 };
            x += width + 4.0;
        }
        // À direita: maximizar e fechar sempre; o terminal ativo e as ações dele só na aba Terminal.
        let inner = rect.width() - 2.0;
        let terminal_tab = self.panel_tab == PanelTab::Terminal;
        let mut icons: Vec<(&str, f64, Option<Command>)> = vec![("close", inner - 28.0, Some(Command::TogglePanel)), ("screen-full", inner - 54.0, Some(Command::ToggleMaximizedPanel))];
        if terminal_tab {
            icons.extend([("ellipsis", inner - 80.0, None), ("trash", inner - 106.0, Some(Command::KillTerminal)), ("split-horizontal", inner - 132.0, None)]);
        }
        for (icon, bx, command) in icons {
            let button = tree.create(panel, Role::Button);
            tree.set_bounds(button, Rect::new(bx, 5.0, bx + 22.0, 27.0));
            let style = tree.style_mut(button);
            style.content = Content::Codicon { id: icon.into(), size: 16.0, color: active_fg };
            style.radius = 5.0;
            style.cursor = Some(Cursor::Pointer);
            if let Some(command) = command {
                tab_hits.push((button, command));
            }
        }
        if terminal_tab {
            let new = tree.create(panel, Role::Button);
            tree.set_bounds(new, Rect::new(inner - 175.0, 5.0, inner - 140.0, 27.0));
            tree.style_mut(new).content = Content::Codicon { id: "add".into(), size: 16.0, color: active_fg };
            tab_hits.push((new, Command::NewTerminal));
            let label_width = measure_text(&label, &font);
            let name = tree.create(panel, Role::Generic);
            let name_x = inner - 175.0 - 8.0 - label_width;
            tree.set_bounds(name, Rect::new(name_x, 4.0, name_x + label_width, 28.0));
            tree.style_mut(name).content = Content::Text { text: label, style: font.clone(), color: active_fg, line_height: 24.0 };
            let icon = tree.create(panel, Role::Image);
            tree.set_bounds(icon, Rect::new(name_x - 26.0, 5.0, name_x - 6.0, 27.0));
            tree.style_mut(icon).content = Content::Codicon { id: "terminal".into(), size: 16.0, color: active_fg };
            // A superfície do terminal.
            let host = tree.create(panel, Role::Generic);
            tree.set_bounds(host, Rect::new(0.0, PANEL_TITLE_HEIGHT, inner, rect.height() - 2.0));
            let style = tree.style_mut(host);
            style.content = Content::Host(1);
            style.cursor = Some(Cursor::Text);
        } else {
            // As outras abas: a mensagem de vazio da vista (o conteúdo delas ainda não existe no je-code).
            let message = self.panel_tab.empty_message();
            if !message.is_empty() {
                let note = tree.create(panel, Role::Status);
                tree.set_bounds(note, Rect::new(20.0, PANEL_TITLE_HEIGHT + 4.0, inner - 20.0, PANEL_TITLE_HEIGHT + 26.0));
                tree.style_mut(note).content = Content::Text { text: message.to_owned(), style: font, color: inactive_fg, line_height: 22.0 };
            }
        }
        drop(tree);
        for (node, command) in tab_hits {
            self.listen_command(node, command);
        }
        if terminal_tab {
            self.ensure_terminal();
        }
    }

    /// Abre o terminal (um shell no workspace) na primeira vez que o painel aparece.
    fn ensure_terminal(&mut self) {
        let Some(area) = self.terminal_area() else { return };
        let (columns, rows) = terminal_panel::grid_size(area.width(), area.height());
        let cw = terminal_panel::cell_width();
        match &mut self.terminal {
            Some(terminal) => terminal.resize(columns, rows, cw.round() as u16, terminal_panel::ROW_HEIGHT as u16),
            None => {
                let (cell_width, cell_height) = (cw.round() as u16, terminal_panel::ROW_HEIGHT as u16);
                let working_directory = self.workspace_for_terminal.take().or_else(|| self.workspace.clone());
                // Fora do wasm o terminal abre um shell local no workspace.
                #[cfg(not(target_arch = "wasm32"))]
                match je_terminal::Terminal::new(working_directory, None, columns, rows, cell_width, cell_height, self.wake.clone()) {
                    Ok(terminal) => self.terminal = Some(terminal),
                    Err(e) => self.notify(Severity::Error, format!("não foi possível abrir o terminal: {e}")),
                }
                // No wasm não há processo local (o pty mora no servidor): a grade nasce vazia e sem backend, como o
                // terminal do VS Code web sem servidor, até uma conexão remota chamar `Terminal::connect`.
                #[cfg(target_arch = "wasm32")]
                {
                    drop(working_directory);
                    self.terminal = Some(je_terminal::Terminal::detached("terminal", columns, rows, cell_width, cell_height, self.wake.clone()));
                }
            }
        }
    }

    /// O texto da grade do terminal, linha a linha (para os testes).
    #[cfg(test)]
    pub fn terminal_text(&self) -> String {
        self.terminal.as_ref().map_or_else(String::new, |t| {
            let snapshot = t.snapshot(&self.terminal_palette);
            snapshot.rows.iter().map(|r| r.iter().map(|c| c.ch).collect::<String>().trim_end().to_owned()).collect::<Vec<_>>().join("\n")
        })
    }

    pub fn set_wake(&mut self, wake: je_terminal::Wake) {
        self.wake = wake;
    }

    fn set_terminal_focus(&mut self, focused: bool) {
        self.terminal_focused = focused;
        if focused {
            self.find_focused = false;
            if let Some(editor) = self.active_editor_mut() {
                editor.view.set_focus(false);
            }
        } else if let Some(editor) = self.active_editor_mut() {
            editor.view.set_focus(true);
        }
        self.needs_render = true;
    }

    /// O fundo do grupo sem editores (`editorGroupWatermark`): o logotipo apagado e os atalhos principais.
    fn render_watermark(tree: &mut Tree, frame: NodeId, part: Rect, theme: &ColorTheme, folder_open: bool) {
        let color = |id: &str| theme.get_color(id).unwrap_or(Rgba::opaque(0xff, 0, 0xff));
        let background = color("editor.background");
        // O logotipo é um tom mais escuro que o fundo (`letterpress`).
        let logo_color = Rgba { r: background.r.saturating_sub(1), g: background.g.saturating_sub(1), b: background.b.saturating_sub(1), ..background };
        let center_x = part.width() / 2.0;
        let top = part.height() / 2.0 - 200.0;
        let logo = tree.create(frame, Role::Image);
        tree.set_bounds(logo, Rect::new(center_x - 128.0, top, center_x + 128.0, top + 256.0));
        tree.style_mut(logo).content = Content::Codicon { id: "vscode".into(), size: 256.0, color: logo_color };
        let label_font = ui_font(13.0, 400);
        let key_font = ui_font(11.0, 400);
        let muted = color("descriptionForeground");
        let chip_bg = color("keybindingLabel.background");
        let chip_border = color("keybindingLabel.bottomBorder");
        let chip_fg = color("keybindingLabel.foreground");
        // Com uma pasta aberta o grupo sugere os comandos do projeto; sem pasta, os de abrir.
        let rows: Vec<(&str, Vec<Vec<&str>>)> = if folder_open {
            vec![("Show All Commands", vec![vec!["Ctrl", "Shift", "P"]]), ("Toggle Terminal", vec![vec!["Ctrl", "`"]]), ("Start Debugging", vec![vec!["F5"]])]
        } else {
            vec![
                ("Show All Commands", vec![vec!["Ctrl", "Shift", "P"]]),
                ("Open Recent", vec![vec!["Ctrl", "R"]]),
                ("Open File", vec![vec!["Ctrl", "O"]]),
                ("Open Folder", vec![vec!["Ctrl", "K"], vec!["Ctrl", "O"]]),
            ]
        };
        let left = center_x - 136.0;
        let mut y = top + 286.0;
        for (label, chords) in rows {
            // Largura dos atalhos: cada tecla é um chip com 6 px de respiro, separados por `+`.
            let chip_width = |key: &str| measure_text(key, &key_font) + 12.0;
            let chord_width = |chord: &Vec<&str>| chord.iter().map(|k| chip_width(k)).sum::<f64>() + 14.0 * (chord.len() as f64 - 1.0);
            let keys_width: f64 = chords.iter().map(chord_width).sum::<f64>() + 10.0 * (chords.len() as f64 - 1.0);
            let wraps = measure_text(label, &label_font) + 12.0 > 272.0 - keys_width - 12.0;
            let lines: Vec<&str> = if wraps { label.split_once(' ').map_or(vec![label], |_| vec!["Show All", "Commands"]) } else { vec![label] };
            for (i, line) in lines.iter().enumerate() {
                let node = tree.create(frame, Role::Generic);
                let w = measure_text(line, &label_font);
                tree.set_bounds(node, Rect::new(left, y + 18.0 * i as f64, left + w, y + 18.0 * (i + 1) as f64));
                tree.style_mut(node).content = Content::Text { text: (*line).to_owned(), style: label_font.clone(), color: muted, line_height: 18.0 };
            }
            let mut x = left + 272.0 - keys_width;
            for (c, chord) in chords.iter().enumerate() {
                if c > 0 {
                    x += 10.0;
                }
                for (k, key) in chord.iter().enumerate() {
                    if k > 0 {
                        let plus = tree.create(frame, Role::Generic);
                        tree.set_bounds(plus, Rect::new(x + 3.0, y, x + 11.0, y + 18.0));
                        tree.style_mut(plus).content = Content::Text { text: "+".to_owned(), style: label_font.clone(), color: muted, line_height: 18.0 };
                        x += 14.0;
                    }
                    let w = chip_width(key);
                    let chip = tree.create(frame, Role::Generic);
                    tree.set_bounds(chip, Rect::new(x, y, x + w, y + 17.0));
                    let style = tree.style_mut(chip);
                    style.background = Some(chip_bg);
                    style.radius = 3.0;
                    style.border_widths = Some([0.0, 0.0, 1.0, 0.0]);
                    style.border = Some(Border { width: 1.0, color: chip_border });
                    let text = tree.create(chip, Role::Generic);
                    tree.set_bounds(text, Rect::new(6.0, 0.0, w - 6.0, 16.0));
                    tree.style_mut(text).content = Content::Text { text: (*key).to_owned(), style: key_font.clone(), color: chip_fg, line_height: 16.0 };
                    x += w;
                }
            }
            y += 18.0 * lines.len() as f64 + 9.0;
        }
    }

    /// Um grupo fora de foco: a moldura, as abas (sem a de destaque do foco) e a superfície do editor ativo dele.
    /// Clicar nele põe o foco no grupo.
    fn render_other_group(&self, chrome: NodeId, index: usize) {
        let Some(group) = self.groups[index].as_ref() else { return };
        let part = self.group_part_rect(index);
        let tab_font = ui_font(13.0, 400);
        let inactive_fg = self.color("tab.inactiveForeground");
        let unfocused_active_fg = self.color("tab.unfocusedActiveForeground");
        let mut hits = Vec::new();
        let mut tree = self.ui.tree_mut();
        let frame = tree.create(chrome, Role::Region);
        tree.set_bounds(frame, part);
        let style = tree.style_mut(frame);
        style.background = Some(self.color("editor.background"));
        style.border = Some(Border { width: 1.0, color: self.color("editorGroup.border") });
        style.radius = RADIUS;
        style.clip = true;
        hits.push((frame, Command::FocusGroup(index)));
        let inner_width = part.width() - 2.0;
        let strip = tree.create(frame, Role::TabList);
        tree.set_bounds(strip, Rect::new(1.0, 1.0, 1.0 + inner_width, 1.0 + TABS_HEIGHT));
        tree.style_mut(strip).background = Some(self.color("editorGroupHeader.connectedTabsBackground"));
        let mut x = 0.0;
        for (i, editor) in group.editors.iter().enumerate() {
            let active = Some(i) == group.active;
            let text_width = measure_text(&editor.name, &tab_font);
            let width = (6.0 + 22.0 + text_width + 4.0 + 20.0 + 6.5).max(80.0).round();
            let tab = tree.create(strip, Role::Tab);
            tree.set_bounds(tab, Rect::new(x, 0.0, x + width, 31.0));
            let style = tree.style_mut(tab);
            style.corner_radii = Some([4.0, 4.0, 0.0, 0.0]);
            style.cursor = Some(Cursor::Pointer);
            if active {
                style.background = Some(self.color("tab.unfocusedActiveBackground"));
                let tail = tree.create(strip, Role::Generic);
                tree.set_bounds(tail, Rect::new(x, 31.0, x + width, TABS_HEIGHT));
                tree.style_mut(tail).background = Some(self.color("tab.unfocusedActiveBackground"));
            }
            if let Some(icons) = &self.icons {
                let language = editor.view.model().get_language_id();
                icons.decorate(&mut tree, tab, Rect::new(6.0, 3.5, 28.0, 27.5), &editor.name, Some(language.as_str()));
            }
            let label = tree.create(tab, Role::Generic);
            tree.set_bounds(label, Rect::new(28.0, 3.5, 28.0 + text_width, 27.5));
            let font = if editor.preview { je_text::TextStyle { italic: true, ..tab_font.clone() } } else { tab_font.clone() };
            tree.style_mut(label).content = Content::Text { text: editor.name.clone(), style: font, color: if active { unfocused_active_fg } else { inactive_fg }, line_height: 24.0 };
            if editor.is_modified() {
                let dot = tree.create(tab, Role::Generic);
                tree.set_bounds(dot, Rect::new(width - 26.5, 5.5, width - 6.5, 25.5));
                tree.style_mut(dot).content = Content::Codicon { id: "circle-filled".into(), size: 12.0, color: inactive_fg };
            }
            hits.push((tab, Command::FocusGroupEditor(index, i)));
            x += width;
        }
        if let Some(active) = group.active.and_then(|a| group.editors.get(a)) {
            let crumbs = tree.create(frame, Role::Group);
            tree.set_bounds(crumbs, Rect::new(1.0, 1.0 + TABS_HEIGHT, 1.0 + inner_width, 1.0 + TABS_HEIGHT + BREADCRUMBS_HEIGHT));
            tree.style_mut(crumbs).background = Some(self.color("breadcrumb.background"));
            let name = tree.create(crumbs, Role::Link);
            tree.set_bounds(name, Rect::new(38.0, 0.0, inner_width, BREADCRUMBS_HEIGHT));
            tree.style_mut(name).content = Content::Text { text: active.name.clone(), style: tab_font.clone(), color: self.color("breadcrumb.foreground"), line_height: BREADCRUMBS_HEIGHT };
            let host = tree.create(frame, Role::TextInput);
            tree.set_bounds(host, Rect::new(1.0, 1.0 + TABS_HEIGHT + BREADCRUMBS_HEIGHT, 1.0 + inner_width, part.height() - 1.0));
            let style = tree.style_mut(host);
            style.content = Content::Host(100 + index as u32);
            style.cursor = Some(Cursor::Text);
        }
        drop(tree);
        for (node, command) in hits {
            let queue = self.queue.clone();
            let _ = self.ui.listen(node, EventKind::PointerDown, false, move |_| queue.borrow_mut().push(command.clone()));
        }
    }

    fn render_editor_part(&mut self, chrome: NodeId) {
        for index in 0..self.groups.len() {
            if index != self.group_index {
                self.render_other_group(chrome, index);
            }
        }
        let part = self.group_part_rect(self.group_index);
        let tab_font = ui_font(13.0, 400);
        let active_fg = self.color("tab.activeForeground");
        let inactive_fg = self.color("tab.inactiveForeground");
        let hovered = *self.hovered_tab.borrow();
        let mut tree = self.ui.tree_mut();
        let frame = tree.create(chrome, Role::Region);
        tree.set_bounds(frame, part);
        let style = tree.style_mut(frame);
        style.background = Some(self.color("editor.background"));
        style.border = Some(Border { width: 1.0, color: self.color("editorGroup.border") });
        style.radius = RADIUS;
        style.clip = true;
        let inner_width = part.width() - 2.0;
        if self.editors.is_empty() {
            self.nodes.editor_host = None;
            Self::render_watermark(&mut tree, frame, part, &self.theme, self.workspace.is_some());
            return;
        }
        // A faixa das abas (`.tabs-and-actions-container`).
        let strip = tree.create(frame, Role::TabList);
        tree.set_bounds(strip, Rect::new(1.0, 1.0, 1.0 + inner_width, 1.0 + TABS_HEIGHT));
        tree.style_mut(strip).background = Some(self.color("editorGroupHeader.connectedTabsBackground"));
        let mut x = 0.0;
        let mut tab_nodes = Vec::new();
        for (index, editor) in self.editors.iter().enumerate() {
            let active = Some(index) == self.active;
            let fg = if active { active_fg } else { inactive_fg };
            let text_width = measure_text(&editor.name, &tab_font);
            let icon_width = 22.0;
            let width = (6.0 + icon_width + text_width + 4.0 + 20.0 + 6.5).max(80.0).round();
            let tab = tree.create(strip, Role::Tab);
            tree.set_bounds(tab, Rect::new(x, 0.0, x + width, 31.0));
            let style = tree.style_mut(tab);
            style.corner_radii = Some([4.0, 4.0, 0.0, 0.0]);
            style.cursor = Some(Cursor::Pointer);
            if active {
                style.background = Some(self.color("tab.activeBackground"));
                // O fundo da aba ativa desce até a borda de baixo da faixa, emendando no editor.
                let tail = tree.create(strip, Role::Generic);
                tree.set_bounds(tail, Rect::new(x, 31.0, x + width, TABS_HEIGHT));
                tree.style_mut(tail).background = Some(self.color("tab.activeBackground"));
            }
            if let Some(icons) = &self.icons {
                let language = editor.view.model().get_language_id();
                icons.decorate(&mut tree, tab, Rect::new(6.0, 3.5, 6.0 + icon_width, 27.5), &editor.name, Some(language.as_str()));
            }
            let label = tree.create(tab, Role::Generic);
            tree.set_bounds(label, Rect::new(6.0 + icon_width, 3.5, 6.0 + icon_width + text_width, 27.5));
            let font = if editor.preview { je_text::TextStyle { italic: true, ..tab_font.clone() } } else { tab_font.clone() };
            tree.style_mut(label).content = Content::Text { text: editor.name.clone(), style: font, color: fg, line_height: 24.0 };
            // O botão de fechar: visível na aba ativa e sob o ponteiro; o ponto de modificado no lugar dele.
            let close = tree.create(tab, Role::Button);
            let cx = width - 6.5 - 20.0;
            tree.set_bounds(close, Rect::new(cx, 5.5, cx + 20.0, 25.5));
            let show_close = active || hovered == Some(index);
            let icon = if editor.is_modified() && hovered != Some(index) { Some("circle-filled") } else if show_close { Some("close") } else { None };
            let style = tree.style_mut(close);
            style.radius = 5.0;
            if let Some(icon) = icon {
                style.content = Content::Codicon { id: icon.into(), size: if icon == "close" { 16.0 } else { 12.0 }, color: fg };
            }
            tab_nodes.push((index, tab, close));
            x += width;
        }
        // As ações do grupo à direita: dividir e mais.
        let mut split_button = None;
        for (i, id) in ["split-horizontal", "ellipsis"].iter().enumerate() {
            let button = tree.create(strip, Role::Button);
            let bx = inner_width - 4.0 - 22.0 * (2 - i) as f64 - 4.0 * (1 - i) as f64;
            tree.set_bounds(button, Rect::new(bx, 4.5, bx + 22.0, 26.5));
            let style = tree.style_mut(button);
            style.content = Content::Codicon { id: (*id).into(), size: 16.0, color: active_fg };
            style.radius = 5.0;
            if i == 0 {
                style.cursor = Some(Cursor::Pointer);
                split_button = Some(button);
            }
        }
        // Os breadcrumbs do editor ativo: ícone, nome e o `…` dos símbolos.
        let active = &self.editors[self.active.unwrap_or(0)];
        let crumbs = tree.create(frame, Role::Group);
        tree.set_bounds(crumbs, Rect::new(1.0, 1.0 + TABS_HEIGHT, 1.0 + inner_width, 1.0 + TABS_HEIGHT + BREADCRUMBS_HEIGHT));
        tree.style_mut(crumbs).background = Some(self.color("breadcrumb.background"));
        let crumb_fg = self.color("breadcrumb.foreground");
        let crumb_font = ui_font(13.0, 400);
        // As pastas do caminho: relativas ao workspace, ou o caminho inteiro para um arquivo de fora dele (o
        // `breadcrumbs.filePath: on` do VS Code), cada uma seguida da seta.
        let folders: Vec<String> = match &active.path {
            Some(path) => {
                let parent = path.parent().unwrap_or(path);
                let relative = self.workspace.as_deref().and_then(|w| parent.strip_prefix(w).ok());
                let shown = relative.unwrap_or(parent);
                shown
                    .components()
                    .filter_map(|c| match c {
                        std::path::Component::Normal(name) => Some(name.to_string_lossy().into_owned()),
                        _ => None,
                    })
                    .collect()
            }
            None => Vec::new(),
        };
        // Cada item é o texto seguido da seta de 16 px, sem respiro (`.monaco-breadcrumb-item`), a partir de 16 px.
        let mut cx = 16.0;
        let chevron = |tree: &mut Tree, cx: &mut f64| {
            let node = tree.create(crumbs, Role::Generic);
            tree.set_bounds(node, Rect::new(*cx, 3.0, *cx + 16.0, 19.0));
            tree.style_mut(node).content = Content::Codicon { id: "chevron-right".into(), size: 16.0, color: crumb_fg };
            *cx += 16.0;
        };
        for folder in &folders {
            let w = measure_text(folder, &crumb_font);
            let node = tree.create(crumbs, Role::Link);
            tree.set_bounds(node, Rect::new(cx, 0.0, cx + w, BREADCRUMBS_HEIGHT));
            tree.style_mut(node).content = Content::Text { text: folder.clone(), style: crumb_font.clone(), color: crumb_fg, line_height: BREADCRUMBS_HEIGHT };
            cx += w;
            chevron(&mut tree, &mut cx);
        }
        if let Some(icons) = &self.icons {
            let language = active.view.model().get_language_id();
            icons.decorate(&mut tree, crumbs, Rect::new(cx + 2.0, 0.0, cx + 20.0, BREADCRUMBS_HEIGHT), &active.name, Some(language.as_str()));
        }
        cx += 22.0;
        let name_width = measure_text(&active.name, &crumb_font);
        let name = tree.create(crumbs, Role::Link);
        tree.set_bounds(name, Rect::new(cx, 0.0, cx + name_width, BREADCRUMBS_HEIGHT));
        tree.style_mut(name).content = Content::Text { text: active.name.clone(), style: crumb_font.clone(), color: crumb_fg, line_height: BREADCRUMBS_HEIGHT };
        cx += name_width;
        chevron(&mut tree, &mut cx);
        let more = tree.create(crumbs, Role::Generic);
        tree.set_bounds(more, Rect::new(cx, 0.0, cx + 23.0, BREADCRUMBS_HEIGHT));
        tree.style_mut(more).content = Content::Text { text: "\u{2026}".to_owned(), style: crumb_font, color: crumb_fg, line_height: BREADCRUMBS_HEIGHT };
        // A superfície do editor.
        let host = tree.create(frame, Role::TextInput);
        tree.set_bounds(host, Rect::new(1.0, 1.0 + TABS_HEIGHT + BREADCRUMBS_HEIGHT, 1.0 + inner_width, part.height() - 1.0));
        let style = tree.style_mut(host);
        style.content = Content::Host(0);
        style.cursor = Some(Cursor::Text);
        drop(tree);
        self.nodes.editor_host = Some(host);
        if let Some(button) = split_button {
            self.listen_command(button, Command::SplitEditor);
        }
        for (index, tab, close) in tab_nodes {
            let title = self.editors[index].path.as_ref().map_or_else(|| self.editors[index].name.clone(), |p| p.display().to_string());
            self.listen_hover(tab, title, je_widgets::hover_api::HoverPosition::Below);
            let queue = self.queue.clone();
            let _ = self.ui.listen(tab, EventKind::PointerDown, false, move |e| {
                match e.pointer.map(|p| p.button) {
                    Some(PointerButton::Auxiliary) => queue.borrow_mut().push(Command::CloseEditor(index)),
                    Some(PointerButton::Secondary) => queue.borrow_mut().push(Command::TabContextMenu(index)),
                    _ => queue.borrow_mut().push(Command::ActivateEditor(index)),
                }
            });
            let queue = self.queue.clone();
            let queue_pin = self.queue.clone();
            let _ = self.ui.listen(tab, EventKind::DoubleClick, false, move |_| queue_pin.borrow_mut().push(Command::PinEditor(index)));
            let _ = self.ui.listen(close, EventKind::Click, false, move |e| {
                e.stop_propagation();
                queue.borrow_mut().push(Command::CloseEditor(index));
            });
            // Sob o ponteiro a aba mostra o botão de fechar (no lugar do ponto de modificado), sem remontar a moldura.
            let fg = if Some(index) == self.active { active_fg } else { inactive_fg };
            let (hovered, ui) = (self.hovered_tab.clone(), self.ui.clone());
            let _ = self.ui.listen(tab, EventKind::PointerEnter, false, move |_| {
                *hovered.borrow_mut() = Some(index);
                ui.tree_mut().style_mut(close).content = Content::Codicon { id: "close".into(), size: 16.0, color: fg };
            });
            let (hovered, ui) = (self.hovered_tab.clone(), self.ui.clone());
            let resting = self.ui.tree().style(close).content.clone();
            let _ = self.ui.listen(tab, EventKind::PointerLeave, false, move |_| {
                let mut h = hovered.borrow_mut();
                if *h == Some(index) {
                    *h = None;
                }
                ui.tree_mut().style_mut(close).content = resting.clone();
            });
        }
    }

    /// Mostra uma notificação no canto inferior direito; some sozinha em 15 s, exceto erro (fica até fechar).
    fn notify(&mut self, severity: Severity, message: String) {
        self.next_toast_id += 1;
        let id = self.next_toast_id;
        self.toasts.push(Toast { id, severity, message });
        if self.toasts.len() > 3 {
            self.toasts.remove(0);
        }
        if severity != Severity::Error {
            let queue = self.queue.clone();
            je_base::runtime::set_timeout(Box::new(move || queue.borrow_mut().push(Command::DismissToast(id))), 15_000);
        }
        self.needs_render = true;
    }

    /// As notificações empilhadas acima da barra de status (`.notifications-toasts`, 452 px, canto inferior direito).
    fn render_toasts(&self, chrome: NodeId) {
        const WIDTH: f64 = 452.0;
        let font = ui_font(13.0, 400);
        let (width, height) = self.size;
        let mut bottom = height - STATUS_HEIGHT - 8.0;
        let mut hits = Vec::new();
        let mut tree = self.ui.tree_mut();
        for toast in self.toasts.iter().rev() {
            // A mensagem quebrada em linhas de 22 px na largura útil (452 - 17 - 33 - 64).
            let max = WIDTH - 33.0 - 64.0;
            let mut lines: Vec<String> = Vec::new();
            for paragraph in toast.message.lines() {
                let mut current = String::new();
                for word in paragraph.split_whitespace() {
                    let candidate = if current.is_empty() { word.to_owned() } else { format!("{current} {word}") };
                    if measure_text(&candidate, &font) > max && !current.is_empty() {
                        lines.push(std::mem::take(&mut current));
                        current = word.to_owned();
                    } else {
                        current = candidate;
                    }
                }
                lines.push(current);
            }
            let rect_height = 14.0 + 22.0 * lines.len() as f64 + 14.0;
            let rect = Rect::new(width - 8.0 - WIDTH, bottom - rect_height, width - 8.0, bottom);
            let node = tree.create(chrome, Role::Alert);
            tree.set_bounds(node, rect);
            let style = tree.style_mut(node);
            style.background = Some(self.color("notifications.background"));
            style.border = Some(Border { width: 1.0, color: self.color("notificationToast.border") });
            style.radius = 8.0;
            let (icon, icon_id) = match toast.severity {
                Severity::Info => ("info", "notificationsInfoIcon.foreground"),
                Severity::Warning => ("warning", "notificationsWarningIcon.foreground"),
                Severity::Error => ("error", "notificationsErrorIcon.foreground"),
            };
            let icon_node = tree.create(node, Role::Image);
            tree.set_bounds(icon_node, Rect::new(8.0, 14.0, 26.0, 32.0));
            tree.style_mut(icon_node).content = Content::Codicon { id: icon.into(), size: 16.0, color: self.color(icon_id) };
            for (i, line) in lines.iter().enumerate() {
                let text = tree.create(node, Role::Generic);
                let w = measure_text(line, &font);
                tree.set_bounds(text, Rect::new(33.0, 14.0 + 22.0 * i as f64, 33.0 + w, 14.0 + 22.0 * (i + 1) as f64));
                tree.style_mut(text).content = Content::Text { text: line.clone(), style: font.clone(), color: self.color("notifications.foreground"), line_height: 22.0 };
            }
            let close = tree.create(node, Role::Button);
            tree.set_bounds(close, Rect::new(WIDTH - 32.0, 12.0, WIDTH - 10.0, 34.0));
            let style = tree.style_mut(close);
            style.content = Content::Codicon { id: "close".into(), size: 16.0, color: self.color("icon.foreground") };
            style.radius = 5.0;
            style.cursor = Some(Cursor::Pointer);
            hits.push((close, Command::DismissToast(toast.id)));
            bottom = rect.y0 - 8.0;
        }
        drop(tree);
        for (node, command) in hits {
            self.listen_command(node, command);
        }
    }

    fn render_status_bar(&self, chrome: NodeId) {
        let (width, height) = self.size;
        let fg = self.color("statusBar.foreground");
        let font = ui_font(12.0, 400);
        let mut tree = self.ui.tree_mut();
        let bar = tree.create(chrome, Role::Status);
        tree.set_bounds(bar, Rect::new(0.0, height - STATUS_HEIGHT, width, height));
        tree.style_mut(bar).background = Some(self.color("statusBar.background"));
        // Um item: ícones e textos lado a lado, 7 px de respiro de cada lado, 24 px de altura.
        enum Piece<'a> {
            Icon(&'a str),
            Text(String),
        }
        // O respiro de cada lado (`.statusbar-item a`, medido no oráculo): 7,5 px com texto, 7 px num ícone só e
        // 5,5 px no ícone de status da linguagem (`{ }`), que encosta no nome da linguagem.
        let padding = |pieces: &[Piece]| -> f64 {
            match pieces {
                [Piece::Icon("json")] => 5.5,
                [Piece::Icon(_)] => 7.0,
                _ => 7.5,
            }
        };
        let measure = |pieces: &[Piece]| -> f64 {
            pieces.iter().map(|p| match p {
                Piece::Icon(_) => 16.0,
                Piece::Text(t) => measure_text(t, &font),
            }).sum::<f64>() + 2.0 * padding(pieces)
        };
        let hits: RefCell<Vec<(NodeId, Command)>> = RefCell::new(Vec::new());
        let place = |tree: &mut Tree, x: f64, pieces: &[Piece], command: Option<Command>| -> f64 {
            let item_width = measure(pieces);
            let item = tree.create(bar, Role::Button);
            if let Some(command) = command {
                tree.style_mut(item).cursor = Some(Cursor::Pointer);
                hits.borrow_mut().push((item, command));
            }
            tree.set_bounds(item, Rect::new(x, 2.0, x + item_width, 26.0));
            tree.style_mut(item).radius = 4.0;
            let mut px = padding(pieces);
            for piece in pieces {
                let node = tree.create(item, Role::Generic);
                match piece {
                    Piece::Icon(id) => {
                        tree.set_bounds(node, Rect::new(px, 4.0, px + 16.0, 20.0));
                        tree.style_mut(node).content = Content::Codicon { id: (*id).into(), size: 16.0, color: fg };
                        px += 16.0;
                    }
                    Piece::Text(text) => {
                        let w = measure_text(text, &font);
                        tree.set_bounds(node, Rect::new(px, 0.0, px + w, 24.0));
                        tree.style_mut(node).content = Content::Text { text: text.clone(), style: font.clone(), color: fg, line_height: 24.0 };
                        px += w;
                    }
                }
            }
            item_width
        };
        let mut left = 6.0;
        left += place(&mut tree, left, &[Piece::Icon("remote")], None);
        if let Some(label) = self.branch_label() {
            left += place(&mut tree, left, &[Piece::Icon("git-branch"), Piece::Text(format!(" {label}"))], Some(Command::ShowScm));
            // O `Synchronize Changes`: as contagens de puxar e empurrar, ou só o ícone de publicar sem remoto.
            let (upstream, ahead, behind) = {
                let s = self.scm.status.borrow();
                (s.upstream, s.ahead, s.behind)
            };
            let busy = self.git_rx.is_some();
            if upstream {
                let text = if ahead == 0 && behind == 0 { String::new() } else { format!(" {behind}\u{2193} {ahead}\u{2191}") };
                left += place(&mut tree, left, &[Piece::Icon(if busy { "loading" } else { "sync" }), Piece::Text(text)], Some(Command::ScmOp(git::Operation::Sync)));
            } else {
                left += place(&mut tree, left, &[Piece::Icon("cloud-upload")], None);
            }
        }
        place(&mut tree, left, &[Piece::Icon("error"), Piece::Text(" 0 ".to_owned()), Piece::Icon("warning"), Piece::Text(" 0".to_owned())], None);
        let mut items: Vec<(Vec<Piece>, Option<Command>)> = Vec::new();
        if let Some(editor) = self.active_editor() {
            let model = editor.view.model();
            let options = model.get_options();
            let eol = if model.get_eol() == je_editor::model::Eol::CrLf { "CRLF" } else { "LF" };
            let indent = if options.insert_spaces { format!("Spaces: {}", options.indent_size) } else { format!("Tab Size: {}", options.tab_size) };
            items.push((vec![Piece::Text(editor.view.status_text())], Some(Command::PromptGoToLine)));
            items.push((vec![Piece::Text(indent)], Some(Command::PickIndentation)));
            items.push((vec![Piece::Text("UTF-8".to_owned())], None));
            items.push((vec![Piece::Text(eol.to_owned())], Some(Command::PickEol)));
            items.push((vec![Piece::Icon("json")], Some(Command::PickLanguage)));
            items.push((vec![Piece::Text(editor.language_name.clone())], Some(Command::PickLanguage)));
        }
        items.push((vec![Piece::Icon("bell")], None));
        let mut right = width - 6.0;
        for (pieces, command) in items.iter().rev() {
            right -= measure(pieces);
            place(&mut tree, right, pieces, command.clone());
        }
        drop(tree);
        for (node, command) in hits.into_inner() {
            self.listen_command(node, command);
        }
    }

    /// Pinta um quadro em `list` e devolve a cor de fundo da janela.
    pub fn paint(&mut self, list: &mut DisplayList, scale: f64, now: Instant) -> Rgba {
        self.run_commands();
        // O documento mudou com a busca aberta: as ocorrências acompanham (o `FindModelBoundToEditorModel`).
        if let (Some(find), Some(active)) = (&self.find, self.active)
            && find.version != self.editors[active].view.model().get_version_id()
        {
            self.find_changed(false);
        }
        self.ui.run_animation_frame();
        if self.needs_render {
            self.render();
            // Os widgets retidos (a barra de menus) agendam o layout para o próximo quadro de animação: roda já.
            self.ui.run_animation_frame();
        }
        self.update_gutter_diff();
        let rect = self.editor_rect();
        let host = self.nodes.editor_host;
        let tree = self.ui.tree();
        let editor = self.active.map(|i| &mut self.editors[i]);
        let mut editor = editor;
        if let Some(e) = editor.as_deref_mut() {
            e.resize(rect.width(), rect.height());
        }
        let (terminal, palette, terminal_focused) = (&self.terminal, &self.terminal_palette, self.terminal_focused);
        let groups = &mut self.groups;
        tree.paint_hosted(list, scale, &mut |surface, area, list| {
            if surface >= 100 {
                // O editor ativo de um grupo fora de foco, sem cursor piscando.
                let Some(view) = groups.get_mut((surface - 100) as usize).and_then(Option::as_mut).and_then(|g| g.active.and_then(|a| g.editors.get_mut(a))) else { return };
                view.resize(area.width(), area.height());
                view.view.set_focus(false);
                let device = Rect::new(area.x0 * scale, area.y0 * scale, area.x1 * scale, area.y1 * scale);
                list.push_clip(Shape::RoundedRect(RoundedRect::from_rect(device, RoundedRectRadii::new(0.0, 0.0, 7.0 * scale, 7.0 * scale))));
                view.paint(list, (area.x0, area.y0), scale, now);
                list.pop_clip();
                return;
            }
            if surface == 1 {
                if let Some(terminal) = terminal {
                    let device = Rect::new(area.x0 * scale, area.y0 * scale, area.x1 * scale, area.y1 * scale);
                    let radii = RoundedRectRadii::new(0.0, 0.0, 7.0 * scale, 7.0 * scale);
                    list.push_clip(Shape::RoundedRect(RoundedRect::from_rect(device, radii)));
                    terminal_panel::paint(list, area, scale, &terminal.snapshot(palette), palette, terminal_focused, true);
                    list.pop_clip();
                }
                return;
            }
            let Some(e) = editor.as_deref_mut() else { return };
            let device = Rect::new(area.x0 * scale, area.y0 * scale, area.x1 * scale, area.y1 * scale);
            let radii = RoundedRectRadii::new(0.0, 0.0, 7.0 * scale, 7.0 * scale);
            list.push_clip(Shape::RoundedRect(RoundedRect::from_rect(device, radii)));
            e.paint(list, (area.x0, area.y0), scale, now);
            list.pop_clip();
        });
        let _ = host;
        self.color("titleBar.activeBackground")
    }

    /// A posição relativa ao editor ativo, se `(x, y)` estiver na superfície dele.
    fn editor_local(&self, x: f64, y: f64) -> Option<(f64, f64)> {
        let rect = self.editor_rect();
        (self.active.is_some() && rect.contains(je_render::kurbo::Point::new(x, y))).then_some((x - rect.x0, y - rect.y0))
    }

    /// O sash entre o grupo de editores e o painel inferior.
    fn panel_sash_hit(&self, x: f64, y: f64) -> bool {
        let (Some(panel), true) = (self.panel_rect(), self.panel_visible) else { return false };
        x >= panel.x0 && x < panel.x1 && y >= panel.y0 - SASH && y < panel.y0
    }

    fn sash_hit(&self, x: f64, y: f64) -> bool {
        self.sidebar_rect().is_some_and(|r| x >= r.x1 && x < r.x1 + SASH && y >= r.y0 && y < r.y1)
    }

    /// Entrega uma entrada de ponteiro: ao editor quando ele é o alvo (ou está no meio de um arraste), à árvore
    /// do `Ui` sempre (menus fecham no clique fora, abas reagem).
    pub fn pointer(&mut self, input: PointerInput) {
        let (x, y) = (input.x, input.y);
        self.last_pointer = (x, y);
        if self.dialog.is_some() {
            // O bloco modal cobre a janela: o ponteiro só fala com o diálogo.
            self.ui.pointer_input(input);
            je_base::runtime::turn();
            self.run_commands();
            return;
        }
        // O menu de contexto aberto: o clique fora dele só o fecha.
        if input.kind == PointerInputKind::Down
            && let Some((_, slot)) = &self.context_menu
        {
            let inside = self.ui.tree().children(*slot).first().is_some_and(|c| self.ui.tree().absolute_bounds(*c).contains(je_render::kurbo::Point::new(x, y)));
            if !inside {
                self.close_context_menu();
                return;
            }
        }
        if let Some((start_height, start_y)) = self.sash_panel_drag {
            // O painel: do mínimo de 80 px até deixar 80 px ao grupo de editores.
            let max = (self.parts_bottom() - TITLE_HEIGHT - SASH - 80.0).max(80.0);
            self.panel_height = (start_height + start_y - y).clamp(80.0, max).round();
            if input.kind == PointerInputKind::Up {
                self.sash_panel_drag = None;
            }
            self.resize_editors();
            return;
        }
        if input.kind == PointerInputKind::Down && self.panel_sash_hit(x, y) {
            self.sash_panel_drag = Some((self.panel_height, y));
            return;
        }
        if let Some((start_width, start_x)) = self.sash_drag {
            // O sash da side bar: limites do `SidebarPart` (170 px a 75% da janela).
            self.sidebar_width = (start_width + x - start_x).clamp(170.0, (self.size.0 * 0.75).max(170.0)).round();
            if input.kind == PointerInputKind::Up {
                self.sash_drag = None;
            }
            self.needs_render = true;
            return;
        }
        if self.editor_drag {
            let rect = self.editor_rect();
            if let Some(editor) = self.active_editor_mut() {
                match input.kind {
                    PointerInputKind::Move => editor.mouse_drag(x - rect.x0, y - rect.y0),
                    PointerInputKind::Up => editor.mouse_up(),
                    PointerInputKind::Down => {}
                }
            }
            if input.kind == PointerInputKind::Up {
                self.editor_drag = false;
            }
            return;
        }
        if input.kind == PointerInputKind::Down && self.sash_hit(x, y) && self.ui.pointer_captured_by().is_none() {
            self.sash_drag = Some((self.sidebar_width, x));
            return;
        }
        if input.kind == PointerInputKind::Down
            && let Some(pick) = &self.quick_pick
            && !pick.widget_rect(self.size.0).contains(je_render::kurbo::Point::new(x, y))
        {
            self.quick_pick = None;
            self.needs_render = true;
            return;
        }
        if input.kind == PointerInputKind::Down
            && let Some(group) = (0..self.groups.len()).find(|i| *i != self.group_index && self.group_part_rect(*i).contains(je_render::kurbo::Point::new(x, y)))
        {
            // O clique num grupo fora de foco o foca antes; a moldura nova vale para o resto do clique.
            self.focus_group(group);
            self.render();
        }
        if input.kind == PointerInputKind::Down {
            let in_terminal = self.panel_tab == PanelTab::Terminal && self.terminal_area().is_some_and(|a| a.contains(je_render::kurbo::Point::new(x, y)));
            if in_terminal != self.terminal_focused {
                self.set_terminal_focus(in_terminal);
            }
        }
        // O ponteiro sobre o editor (os chevrons de dobra da margem aparecem com ele).
        if input.kind == PointerInputKind::Move {
            let local = self.editor_local(x, y);
            if let Some(editor) = self.active_editor_mut() {
                let (lx, ly) = local.unwrap_or((-1.0, -1.0));
                editor.mouse_move(lx, ly);
            }
        }
        let target = self.ui.pointer_input(input);
        if input.kind == PointerInputKind::Down && Some(target) == self.nodes.editor_host && input.button == PointerButton::Primary {
            self.ui.blur();
            self.find_focused = false;
            self.search_focused = false;
            self.scm_focused = false;
            self.extensions_focused = false;
            if let Some((lx, ly)) = self.editor_local(x, y)
                && let Some(editor) = self.active_editor_mut()
            {
                editor.view.set_focus(true);
                editor.mouse_down(lx, ly, &input);
                self.editor_drag = true;
            }
        }
        self.run_commands();
    }

    pub fn pointer_left(&mut self) {
        self.ui.pointer_left_window();
        self.run_commands();
    }

    pub fn cursor_at(&self, x: f64, y: f64) -> Cursor {
        if self.sash_drag.is_some() || self.sash_hit(x, y) {
            return Cursor::EwResize;
        }
        if self.sash_panel_drag.is_some() || self.panel_sash_hit(x, y) {
            return Cursor::NsResize;
        }
        self.ui.cursor_at(x, y)
    }

    /// A roda do mouse: rola o editor se o ponteiro estiver nele.
    pub fn wheel(&mut self, x: f64, y: f64, dx: f64, dy: f64) {
        if self.sidebar_rect().is_some_and(|r| r.contains(je_render::kurbo::Point::new(x, y))) {
            match (self.side_view, &self.explorer) {
                (SideView::Explorer, Some(explorer)) => explorer.scroll_by(dy),
                (SideView::Search, _) => self.search.scroll_by(dy),
                (SideView::SourceControl, _) => self.scm.scroll_by(dy),
                (SideView::Extensions, _) => {
                    self.extensions_scroll = (self.extensions_scroll + dy).max(0.0);
                    self.needs_render = true;
                }
                _ => {}
            }
            return;
        }
        if self.terminal_area().is_some_and(|a| a.contains(je_render::kurbo::Point::new(x, y))) {
            if let Some(terminal) = &self.terminal {
                terminal.scroll((-dy / terminal_panel::ROW_HEIGHT).round() as i32);
            }
            return;
        }
        if let Some((local_x, _)) = self.editor_local(x, y)
            && let Some(editor) = self.active_editor_mut()
        {
            editor.scroll_by(local_x, dx, dy);
        }
    }

    /// O comando do shell para um id do VS Code vindo de um atalho do `keybindings.json` (`workbench.action.quickOpen`...);
    /// `None` se o shell não tem esse comando.
    fn command_of_id(id: &str) -> Option<Command> {
        Some(match id {
            "workbench.action.files.newUntitledFile" => Command::NewUntitled,
            "workbench.action.files.save" => Command::Save,
            "workbench.action.files.saveAll" => Command::SaveAll,
            "workbench.action.files.saveAs" => Command::PromptSaveAs,
            "workbench.action.files.openFile" => Command::PromptOpenFile,
            "workbench.action.files.revert" => Command::Revert,
            "workbench.action.openRecent" => Command::OpenRecent,
            "workbench.action.zoomIn" => Command::Zoom(1),
            "workbench.action.zoomOut" => Command::Zoom(-1),
            "workbench.action.zoomReset" => Command::Zoom(0),
            "workbench.action.closeActiveEditor" => Command::CloseActiveEditor,
            "workbench.action.closeAllEditors" => Command::CloseAll,
            "workbench.action.quit" => Command::Exit,
            "workbench.action.toggleSidebarVisibility" => Command::ToggleSidebar,
            "workbench.action.toggleStatusbarVisibility" => Command::ToggleStatusBar,
            "workbench.action.toggleFullScreen" => Command::ToggleFullScreen,
            "workbench.action.quickOpen" | "workbench.action.quickOpenView" => Command::QuickOpen,
            "workbench.action.showCommands" => Command::ShowCommands,
            "actions.find" => Command::Find,
            "editor.action.startFindReplace" => Command::FindAndReplace,
            "editor.action.nextMatchFindAction" => Command::FindNext,
            "editor.action.previousMatchFindAction" => Command::FindPrevious,
            "workbench.action.togglePanel" => Command::TogglePanel,
            "workbench.action.splitEditor" => Command::SplitEditor,
            "workbench.action.focusFirstEditorGroup" => Command::FocusGroup(0),
            "workbench.action.focusSecondEditorGroup" => Command::FocusGroup(1),
            "workbench.action.focusThirdEditorGroup" => Command::FocusGroup(2),
            "workbench.action.openSettingsJson" | "workbench.action.openSettings" => Command::OpenSettingsJson,
            "workbench.action.openGlobalKeybindingsFile" | "workbench.action.openGlobalKeybindings" => Command::OpenKeybindingsJson,
            "workbench.action.gotoLine" => Command::PromptGoToLine,
            "workbench.action.terminal.toggleTerminal" => Command::ToggleTerminal,
            "workbench.action.terminal.new" => Command::NewTerminal,
            "workbench.view.explorer" => Command::ShowExplorer,
            "workbench.view.search" | "workbench.action.findInFiles" => Command::ShowSearch,
            "workbench.action.replaceInFiles" => Command::ShowSearchReplace,
            "workbench.view.scm" => Command::ShowScm,
            "workbench.view.extensions" => Command::ShowExtensions,
            "workbench.actions.view.problems" => Command::ShowPanelTab(PanelTab::Problems),
            "workbench.action.output.toggleOutput" => Command::ShowPanelTab(PanelTab::Output),
            "workbench.debug.action.toggleRepl" => Command::ShowPanelTab(PanelTab::DebugConsole),
            "workbench.action.nextEditor" => Command::NextEditor,
            "workbench.action.previousEditor" => Command::PreviousEditor,
            "workbench.action.selectTheme" => Command::PickTheme,
            "workbench.action.editor.changeLanguageMode" => Command::PickLanguage,
            "workbench.action.editor.changeEOL" => Command::PickEol,
            "workbench.action.editor.changeIndentation" => Command::PickIndentation,
            "editor.action.detectIndentation" => Command::DetectIndentation,
            _ => return None,
        })
    }

    /// Os atalhos do workbench que valem com o foco no editor (os `KeybindingsRegistry` globais).
    fn workbench_shortcut(&self, key: KeyCode, mods: Modifiers) -> Option<Command> {
        let ctrl = mods.ctrl || mods.meta;
        Some(match (ctrl, mods.shift, mods.alt, key) {
            (true, false, false, KeyCode::KeyN) => Command::NewUntitled,
            (true, false, false, KeyCode::KeyS) => Command::Save,
            (true, true, false, KeyCode::KeyS) => Command::PromptSaveAs,
            (true, false, false, KeyCode::KeyO) => Command::PromptOpenFile,
            (true, false, false, KeyCode::KeyR) => Command::OpenRecent,
            (true, _, false, KeyCode::Equal) | (true, false, false, KeyCode::NumpadAdd) => Command::Zoom(1),
            (true, false, false, KeyCode::Minus) | (true, false, false, KeyCode::NumpadSubtract) => Command::Zoom(-1),
            (true, false, false, KeyCode::Digit0) | (true, false, false, KeyCode::Numpad0) => Command::Zoom(0),
            (true, false, false, KeyCode::KeyW) | (true, false, false, KeyCode::F4) => Command::CloseActiveEditor,
            (true, false, false, KeyCode::KeyQ) => Command::Exit,
            (true, false, false, KeyCode::KeyB) => Command::ToggleSidebar,
            (true, false, false, KeyCode::KeyP) => Command::QuickOpen,
            (true, false, false, KeyCode::KeyF) => Command::Find,
            (true, false, false, KeyCode::KeyH) => Command::FindAndReplace,
            (true, false, false, KeyCode::KeyJ) => Command::TogglePanel,
            (true, false, false, KeyCode::Backslash) => Command::SplitEditor,
            (true, false, false, KeyCode::Digit1) => Command::FocusGroup(0),
            (true, false, false, KeyCode::Digit2) => Command::FocusGroup(1),
            (true, false, false, KeyCode::Digit3) => Command::FocusGroup(2),
            (true, false, false, KeyCode::Comma) => Command::OpenSettingsJson,
            (true, false, false, KeyCode::KeyG) => Command::PromptGoToLine,
            (true, false, false, KeyCode::Backquote) => Command::ToggleTerminal,
            (true, true, false, KeyCode::Backquote) => Command::NewTerminal,
            (false, false, false, KeyCode::F3) => Command::FindNext,
            (false, true, false, KeyCode::F3) => Command::FindPrevious,
            (true, true, false, KeyCode::KeyP) | (false, false, false, KeyCode::F1) => Command::ShowCommands,
            (true, true, false, KeyCode::KeyE) => Command::ShowExplorer,
            (true, true, false, KeyCode::KeyF) => Command::ShowSearch,
            (true, true, false, KeyCode::KeyH) => Command::ShowSearchReplace,
            (true, true, false, KeyCode::KeyG) => Command::ShowScm,
            (true, true, false, KeyCode::KeyX) => Command::ShowExtensions,
            (true, true, false, KeyCode::KeyM) => Command::ShowPanelTab(PanelTab::Problems),
            (true, true, false, KeyCode::KeyU) => Command::ShowPanelTab(PanelTab::Output),
            (true, true, false, KeyCode::KeyY) => Command::ShowPanelTab(PanelTab::DebugConsole),
            (true, false, false, KeyCode::PageDown) => Command::NextEditor,
            (true, false, false, KeyCode::PageUp) => Command::PreviousEditor,
            (false, false, false, KeyCode::F11) => Command::ToggleFullScreen,
            (false, false, true, KeyCode::KeyZ) => Command::ToggleWordWrap,
            _ => return None,
        })
    }

    /// Entrega uma tecla: primeiro à árvore do `Ui` (a barra de menus escuta Alt e os mnemônicos na raiz); se nenhum
    /// widget tem o foco e ninguém consumiu, aos atalhos do workbench e então ao editor.
    pub fn key(&mut self, down: bool, key_code: KeyCode, mods: Modifiers, text: Option<&str>, editor_key: Option<EditorKey>) {
        if self.dialog.is_some() {
            self.ui.key_input(down, KeyData { key_code, modifiers: mods });
            je_base::runtime::turn();
            self.run_commands();
            return;
        }
        if self.context_menu.is_some() {
            // O `Menu` escuta o teclado pelo `Ui` (setas, Enter, Esc, mnemônicos).
            self.ui.key_input(down, KeyData { key_code, modifiers: mods });
            self.run_commands();
            return;
        }
        if self.quick_pick.is_some() && !(down && matches!(self.workbench_shortcut(key_code, mods), Some(Command::QuickOpen | Command::ShowCommands))) && self.quick_pick_key(down, key_code, text, editor_key, mods) {
            return;
        }
        if self.terminal_focused && self.panel_visible && self.panel_tab == PanelTab::Terminal && self.terminal.is_some() && self.quick_pick.is_none() {
            if down {
                let allowed = self.workbench_shortcut(key_code, mods).filter(|c| matches!(c, Command::QuickOpen | Command::ShowCommands | Command::TogglePanel | Command::ToggleTerminal | Command::NewTerminal | Command::ToggleSidebar | Command::NewUntitled | Command::ShowExplorer | Command::ToggleFullScreen | Command::Exit));
                if let Some(command) = allowed {
                    self.run(command);
                    return;
                }
                let ctrl = mods.ctrl || mods.meta;
                // Colar e copiar do terminal: Ctrl+Shift+V / Ctrl+Shift+C (e Ctrl+V no Windows).
                if ctrl && key_code == KeyCode::KeyV && (mods.shift || cfg!(windows)) {
                    let text = je_editor_view::clipboard::Clipboard::new().text();
                    if let Some(terminal) = &self.terminal {
                        let bracketed = terminal.bracketed_paste();
                        let mut bytes = Vec::new();
                        if bracketed {
                            bytes.extend_from_slice(b"\x1b[200~");
                        }
                        bytes.extend_from_slice(text.replace("\r\n", "\r").replace('\n', "\r").as_bytes());
                        if bracketed {
                            bytes.extend_from_slice(b"\x1b[201~");
                        }
                        terminal.write(bytes);
                    }
                    return;
                }
                if let Some(terminal) = &self.terminal
                    && let Some(bytes) = terminal_panel::encode_key(terminal, editor_key, key_code, text, mods)
                {
                    terminal.scroll_to_bottom();
                    terminal.write(bytes);
                }
            }
            return;
        }
        if self.extensions_focused && self.sidebar_visible && self.side_view == SideView::Extensions && down {
            if let Some(command) = self.workbench_shortcut(key_code, mods) {
                self.run(command);
                return;
            }
            if editor_key == Some(EditorKey::Escape) {
                self.extensions_focused = false;
                if let Some(editor) = self.active_editor_mut() {
                    editor.view.set_focus(true);
                }
            } else {
                input::edit(&mut self.extensions_filter, key_code, editor_key, mods, text, &input::SystemClipboard::new());
            }
            self.extensions_scroll = 0.0;
            self.needs_render = true;
            return;
        }
        if self.scm_focused && self.sidebar_visible && self.side_view == SideView::SourceControl && down {
            let shortcut = self.workbench_shortcut(key_code, mods).filter(|c| matches!(c, Command::ShowScm | Command::ShowSearch | Command::QuickOpen | Command::ShowCommands | Command::TogglePanel | Command::ToggleTerminal | Command::ToggleSidebar | Command::NewUntitled | Command::ShowExplorer));
            if let Some(command) = shortcut {
                self.run(command);
                return;
            }
            if self.scm_key(key_code, text, editor_key, mods) {
                return;
            }
        }
        if self.search_focused && self.sidebar_visible && self.side_view == SideView::Search && down {
            let toggles = match (mods.alt, key_code) {
                (true, KeyCode::KeyC) => Some(Command::SearchToggleCase),
                (true, KeyCode::KeyW) => Some(Command::SearchToggleWord),
                (true, KeyCode::KeyR) => Some(Command::SearchToggleRegex),
                _ => None,
            };
            let shortcut = self.workbench_shortcut(key_code, mods).filter(|c| matches!(c, Command::ShowSearch | Command::QuickOpen | Command::ShowCommands | Command::Find | Command::TogglePanel | Command::ToggleTerminal | Command::ToggleSidebar | Command::NewUntitled | Command::Save | Command::CloseActiveEditor | Command::ShowExplorer));
            if let Some(command) = toggles.or(shortcut) {
                self.run(command);
                return;
            }
            if self.search_key(key_code, text, editor_key, mods) {
                return;
            }
        }
        if self.find_focused && self.find.is_some() && down {
            let toggles = match (mods.alt, key_code) {
                (true, KeyCode::KeyC) => Some(Command::FindToggleCase),
                (true, KeyCode::KeyW) => Some(Command::FindToggleWord),
                (true, KeyCode::KeyR) => Some(Command::FindToggleRegex),
                _ => None,
            };
            let shortcut = self.workbench_shortcut(key_code, mods).filter(|c| matches!(c, Command::Find | Command::FindAndReplace | Command::QuickOpen | Command::ShowCommands | Command::FindNext | Command::FindPrevious | Command::NewUntitled | Command::Save | Command::CloseActiveEditor | Command::ToggleSidebar));
            if let Some(command) = toggles.or(shortcut) {
                self.run(command);
                return;
            }
            if self.find_key(key_code, text, editor_key, mods) {
                return;
            }
        }
        let had_focus = self.ui.focused().is_some();
        let not_consumed = self.ui.key_input(down, KeyData { key_code, modifiers: mods });
        self.run_commands();
        if !down || had_focus || self.ui.focused().is_some() || !not_consumed {
            if let Some(text) = text
                && down
                && self.ui.focused().is_some()
            {
                self.ui.text_input(text);
            }
            return;
        }
        // Uma tecla que o `keybindings.json` religou a um comando do shell vence a tabela fixa dele.
        let rebound = editor_key.and_then(|key| self.active_editor().and_then(|e| e.view.user_keybinding(key, mods))).and_then(|(id, _)| Self::command_of_id(&id));
        if let Some(command) = rebound.or_else(|| self.workbench_shortcut(key_code, mods)) {
            self.run(command);
            return;
        }
        if editor_key == Some(EditorKey::Escape) && self.find.is_some() {
            self.close_find();
            return;
        }
        let Some(editor) = self.active_editor_mut() else { return };
        if let Some(key) = editor_key {
            editor.key_down(key, mods);
            // Os atalhos do `keybindings.json` que apontam para comandos do workbench ficam na fila do editor.
            let mut commands = Vec::new();
            while let Some((id, _args)) = editor.take_workbench_command() {
                commands.extend(Self::command_of_id(&id));
            }
            for command in commands {
                self.run(command);
            }
            return;
        }
        let ctrl = mods.ctrl || mods.meta;
        if (!ctrl || mods.alt)
            && let Some(text) = text
        {
            editor.type_text(text);
        }
    }

    pub fn set_window_focus(&mut self, focused: bool) {
        if focused {
            self.check_disk();
        }
        if focused && self.git_root.is_some() {
            // Ao voltar para a janela o estado do git pode ter mudado fora dela.
            self.git_op(git::Operation::Refresh);
        }
        if let Some(editor) = self.active_editor_mut() {
            editor.view.set_focus(focused);
        }
        if !focused {
            self.menubar.blur();
            if matches!(self.auto_save.0.as_str(), "onFocusChange" | "onWindowChange") {
                self.run(Command::SaveAll);
            }
        }
    }

    /// Avança o que é assíncrono (comandos, quadros de animação) e diz se precisa de quadro novo.
    pub fn take_dirty(&mut self) -> bool {
        self.run_commands();
        let editor = self.active.is_some_and(|i| self.editors[i].view.take_dirty());
        // Editar fixa a aba de pré-visualização.
        if let Some(i) = self.active
            && self.editors[i].preview
            && self.editors[i].is_modified()
        {
            self.editors[i].preview = false;
            self.needs_render = true;
        }
        // `files.autoSave: afterDelay`: a aba ativa modificada salva depois do prazo sem novas edições.
        if self.auto_save.0 == "afterDelay"
            && let Some(index) = self.active
            && self.editors[index].path.is_some()
            && self.editors[index].is_modified()
        {
            let version = self.editors[index].view.model().get_version_id();
            if self.auto_save_scheduled != Some((index, version)) {
                self.auto_save_scheduled = Some((index, version));
                let queue = self.queue.clone();
                je_base::runtime::set_timeout(move || queue.borrow_mut().push(Command::AutoSave(index, version)), self.auto_save.1);
            }
        }
        let terminal = self.terminal.as_ref().is_some_and(je_terminal::Terminal::take_dirty);
        if let Some(rx) = &self.git_rx
            && let Ok(result) = rx.try_recv()
        {
            self.git_rx = None;
            match result {
                Ok(status) => {
                    self.scm.set_status(status);
                    self.reload_heads();
                }
                Err(e) => self.notify(Severity::Error, e),
            }
            self.needs_render = true;
        }
        let search = self.search.poll();
        if search {
            self.needs_render = true;
        }
        editor || terminal || self.needs_render || std::mem::take(&mut self.redraw) || self.ui.has_pending_animation_frame()
    }

    pub fn next_blink_deadline(&self, now: Instant) -> Option<Instant> {
        self.active_editor().map(|e| e.view.next_blink_deadline(now))
    }
}
