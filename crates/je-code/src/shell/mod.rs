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
mod menus;

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use je_base::key_codes::KeyCode;
use je_editor::model::EndOfLinePreference;
use je_editor::text_model::{TextModel, TextSource};
use je_editor::undo_redo_service::UndoRedoService;
use je_editor_view::environment::find_builtin_extensions;
use je_editor_view::{EditorColors, EditorEnvironment, EditorKey, EditorView};
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
use menus::CommandQueue;

/// O tema padrão relativo à pasta de extensões embutidas.
pub const BUILTIN_DEFAULT_THEME: &str = "theme-defaults/themes/2026-dark.json";

/// O que os menus, botões e atalhos pedem ao shell. Roda depois do evento que o gerou, fora dos callbacks.
#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    NewUntitled,
    OpenFile(PathBuf),
    Save,
    SaveAll,
    Revert,
    CloseActiveEditor,
    CloseEditor(usize),
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
    ShowExplorer,
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
}

impl OpenEditor {
    pub fn is_modified(&self) -> bool {
        self.view.model().get_alternative_version_id() != self.saved_version
    }
}

/// Os nós retidos e os que [`Shell::render`] remonta.
struct Nodes {
    /// Remontado a cada `render`: tudo menos a barra de menus e o explorador.
    chrome: NodeId,
    menubar: NodeId,
    explorer: NodeId,
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
    nodes: Nodes,
    editors: Vec<OpenEditor>,
    active: Option<usize>,
    workspace: Option<PathBuf>,
    sidebar_visible: bool,
    sidebar_width: f64,
    statusbar_visible: bool,
    size: (f64, f64),
    /// O caminho de pintura, para a barra de status (`GPU ...` ou `CPU ...`).
    backend: String,
    /// Falha da última operação de arquivo, mostrada na barra de status.
    notice: Option<String>,
    /// O botão do mouse foi apertado no editor e o arraste segue para ele até soltar.
    editor_drag: bool,
    /// O arraste do sash da side bar: a largura no início e o x do ponteiro.
    sash_drag: Option<(f64, f64)>,
    /// O índice da aba sob o ponteiro (mostra o botão de fechar).
    hovered_tab: Rc<RefCell<Option<usize>>>,
    exit_requested: bool,
    fullscreen_requested: bool,
    needs_render: bool,
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
        let (chrome, menubar_node, explorer_node) = {
            let mut tree = ui.tree_mut();
            tree.set_bounds(root, Rect::new(0.0, 0.0, width, height));
            let chrome = tree.create(root, Role::Generic);
            let explorer = tree.create(root, Role::Tree);
            let menubar = tree.create(root, Role::MenuBar);
            tree.add_class(menubar, "menubar");
            (chrome, menubar, explorer)
        };
        let queue: CommandQueue = Rc::new(RefCell::new(Vec::new()));
        let (menus, get_keybinding) = menus::menubar_menus(&queue);
        let menu_styles = default_menu_styles(&theme);
        let bar_options = IMenuBarOptions {
            enable_mnemonics: true,
            visibility: Some("classic".to_owned()),
            get_keybinding: Some(get_keybinding),
            styles: Some(Rc::new(default_menu_bar_styles(&theme))),
            ..Default::default()
        };
        let menubar = MenuBar::new(&ui, menubar_node, bar_options, menu_styles);
        menubar.push(&menus);
        let language_of: explorer::LanguageOf = match &environment {
            Some(env) => {
                let env = env.clone();
                Rc::new(move |name: &str| Some(env.language_id_for(name, None)))
            }
            None => Rc::new(|_: &str| None),
        };
        let explorer = workspace.as_ref().map(|root| Explorer::new(&ui, explorer_node, root, &theme, icons.clone(), language_of, queue.clone()));
        let mut shell = Self {
            ui,
            theme,
            environment,
            icons,
            queue,
            menubar,
            explorer,
            nodes: Nodes { chrome, menubar: menubar_node, explorer: explorer_node, editor_host: None },
            editors: Vec::new(),
            active: None,
            sidebar_visible: workspace.is_some(),
            sidebar_width: DEFAULT_SIDEBAR_WIDTH,
            workspace,
            statusbar_visible: true,
            size: (width, height),
            backend: String::new(),
            notice: None,
            editor_drag: false,
            sash_drag: None,
            hovered_tab: Rc::new(RefCell::new(None)),
            exit_requested: false,
            fullscreen_requested: false,
            needs_render: true,
        };
        for file in files {
            shell.open_file(file)?;
        }
        if shell.editors.is_empty() && shell.workspace.is_none() {
            shell.new_untitled();
        }
        shell.render();
        Ok(shell)
    }

    fn color(&self, id: &str) -> Rgba {
        color_or(&self.theme, id, Rgba::opaque(0xff, 0x00, 0xff))
    }

    pub fn set_backend(&mut self, backend: String) {
        self.backend = backend;
        self.needs_render = true;
    }

    /// Os caminhos selecionados no explorador (para os testes).
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
        let saved_version = model.get_alternative_version_id();
        let index = self.active.map_or(self.editors.len(), |i| i + 1);
        self.editors.insert(index, OpenEditor { path, name, language_name, view, saved_version });
        self.activate(index);
    }

    /// Abre `path` numa aba (ou ativa a aba que já o mostra).
    pub fn open_file(&mut self, path: &Path) -> Result<(), String> {
        if let Some(index) = self.editors.iter().position(|e| e.path.as_deref() == Some(path)) {
            self.activate(index);
            return Ok(());
        }
        let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let name = path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
        self.push_editor(Some(path.to_path_buf()), name, &String::from_utf8_lossy(&bytes));
        Ok(())
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
        editor.view.resize(rect.width(), rect.height());
        editor.view.set_focus(true);
        editor.view.mark_dirty();
        if let Some(explorer) = &self.explorer {
            explorer.reveal(editor.path.as_deref());
        }
        self.needs_render = true;
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
        self.needs_render = true;
    }

    fn save(&mut self, index: usize) {
        let editor = &mut self.editors[index];
        let Some(path) = editor.path.clone() else {
            self.notice = Some("Save As ainda não existe: o arquivo sem título não foi salvo".to_owned());
            return;
        };
        let model = editor.view.model().clone();
        let text = model.get_value_with(EndOfLinePreference::TextDefined, true).to_string_lossy();
        match std::fs::write(&path, text.as_bytes()) {
            Ok(()) => {
                editor.saved_version = model.get_alternative_version_id();
                self.notice = None;
            }
            Err(e) => self.notice = Some(format!("falha ao salvar: {e}")),
        }
        editor.view.mark_dirty();
        self.needs_render = true;
    }

    fn revert(&mut self, index: usize) {
        let Some(path) = self.editors[index].path.clone() else { return };
        match std::fs::read(&path) {
            Ok(bytes) => {
                let name = self.editors[index].name.clone();
                let (model, language_name) = self.model_for(&name, &String::from_utf8_lossy(&bytes));
                let rect = self.editor_rect();
                let mut view = EditorView::new(model.clone(), rect.width(), rect.height(), EditorColors::from_theme(&self.theme));
                if let Some(env) = &self.environment {
                    view.set_token_colors(&env.color_map());
                }
                self.editors[index] = OpenEditor { path: Some(path), name, language_name, view, saved_version: model.get_alternative_version_id() };
                self.activate(index);
            }
            Err(e) => self.notice = Some(format!("falha ao reverter: {e}")),
        }
    }

    /// Roda os comandos enfileirados pelos menus e botões.
    fn run_commands(&mut self) {
        loop {
            let commands = std::mem::take(&mut *self.queue.borrow_mut());
            if commands.is_empty() {
                return;
            }
            for command in commands {
                self.run(command);
            }
        }
    }

    pub fn run(&mut self, command: Command) {
        let active = self.active;
        match command {
            Command::NewUntitled => self.new_untitled(),
            Command::OpenFile(path) => {
                if let Err(e) = self.open_file(&path) {
                    self.notice = Some(e);
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
                    self.close_editor(i);
                }
            }
            Command::CloseEditor(i) => self.close_editor(i),
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
                    let view = &mut editor.view;
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
            Command::ToggleSidebar => self.sidebar_visible = !self.sidebar_visible,
            Command::ShowExplorer => self.sidebar_visible = !self.sidebar_visible || self.explorer.is_none(),
            Command::ToggleStatusBar => self.statusbar_visible = !self.statusbar_visible,
            Command::ToggleFullScreen => self.fullscreen_requested = true,
            Command::Exit => self.exit_requested = true,
        }
        self.needs_render = true;
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

    /// O painel do grupo de editores, com a borda de 1 px.
    fn editor_part_rect(&self) -> Rect {
        let left = self.sidebar_rect().map_or(ACTIVITY_LEFT + ACTIVITY_WIDTH + SASH, |r| r.x1 + SASH);
        Rect::new(left, TITLE_HEIGHT, self.size.0 - 4.0, self.parts_bottom())
    }

    /// A área do editor de texto: abaixo das abas e dos breadcrumbs, dentro da borda.
    fn editor_rect(&self) -> Rect {
        let part = self.editor_part_rect().inset(-1.0);
        Rect::new(part.x0, part.y0 + TABS_HEIGHT + BREADCRUMBS_HEIGHT, part.x1, part.y1)
    }

    fn listen_command(&self, node: NodeId, command: Command) {
        let queue = self.queue.clone();
        // Os nós da moldura morrem no próximo `render`, e com eles o ouvinte.
        let _ = self.ui.listen(node, EventKind::Click, false, move |_| queue.borrow_mut().push(command.clone()));
    }

    /// Remonta a moldura para o estado atual.
    pub fn render(&mut self) {
        self.needs_render = false;
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
        if self.statusbar_visible {
            self.render_status_bar(chrome);
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
        let x = ((width - center_width) / 2.0).max(0.0).round();
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
        let ids = ["layout", if self.sidebar_visible { "layout-sidebar-left" } else { "layout-sidebar-left-off" }, "layout-panel-off", "layout-sidebar-right-off"];
        let right = width - 100.0 - 26.0 * ids.len() as f64 + 4.0;
        for (i, id) in ids.iter().enumerate() {
            let button = tree.create(title, Role::Button);
            let bx = right + 26.0 * i as f64;
            tree.set_bounds(button, Rect::new(bx, 6.5, bx + 22.0, 28.5));
            let style = tree.style_mut(button);
            style.content = Content::Codicon { id: (*id).into(), size: 16.0, color: fg };
            style.radius = 5.0;
            style.cursor = Some(Cursor::Pointer);
            if i == 1 {
                drop(tree);
                self.listen_command(button, Command::ToggleSidebar);
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
        let top = ["files", "search", "source-control", "debug-alt", "extensions"];
        let mut explorer_button = None;
        for (i, id) in top.iter().enumerate() {
            let button = tree.create(bar, Role::Button);
            tree.set_bounds(button, Rect::new(4.0, 4.0 + 44.0 * i as f64, 40.0, 40.0 + 44.0 * i as f64));
            let checked = i == 0 && self.sidebar_visible;
            let style = tree.style_mut(button);
            style.content = Content::Codicon { id: (*id).into(), size: 24.0, color: if checked { foreground } else { inactive } };
            style.radius = 4.0;
            if checked {
                style.background = Some(self.color("activityBar.activeBackground"));
            }
            if i == 0 {
                explorer_button = Some(button);
            }
        }
        let height = bottom - TITLE_HEIGHT;
        for (i, id) in ["account", "settings-gear"].iter().enumerate() {
            let button = tree.create(bar, Role::Button);
            let y = height - 4.0 - 44.0 * (2 - i) as f64 + 4.0;
            tree.set_bounds(button, Rect::new(4.0, y, 40.0, y + 36.0));
            tree.style_mut(button).content = Content::Codicon { id: (*id).into(), size: 24.0, color: inactive };
        }
        drop(tree);
        if let Some(button) = explorer_button {
            self.listen_command(button, Command::ShowExplorer);
        }
    }

    fn render_sidebar(&mut self, chrome: NodeId) {
        let Some(rect) = self.sidebar_rect() else {
            self.ui.tree_mut().style_mut(self.nodes.explorer).visible = false;
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
        tree.style_mut(title).content = Content::Text { text: "Explorer".to_owned(), style: ui_font(12.0, 600), color: fg, line_height: 32.0 };
        let more = tree.create(bar, Role::Button);
        tree.set_bounds(more, Rect::new(rect.width() - 27.0, 6.0, rect.width() - 5.0, 28.0));
        tree.style_mut(more).content = Content::Codicon { id: "ellipsis".into(), size: 16.0, color: fg };
        // A seção da pasta e, embaixo, Outline e Timeline recolhidas (`.pane-header`, 28 px, 11 px negrito).
        let section = |tree: &mut Tree, y: f64, label: &str, expanded: bool| {
            let header = tree.create(bar, Role::Button);
            tree.set_bounds(header, Rect::new(5.0, y, rect.width() - 6.0, y + 28.0));
            let chevron = tree.create(header, Role::Generic);
            tree.set_bounds(chevron, Rect::new(2.0, 6.0, 18.0, 22.0));
            tree.style_mut(chevron).content = Content::Codicon { id: if expanded { "chevron-down" } else { "chevron-right" }.into(), size: 16.0, color: fg };
            let text = tree.create(header, Role::Generic);
            tree.set_bounds(text, Rect::new(20.0, 0.0, rect.width() - 30.0, 28.0));
            tree.style_mut(text).content = Content::Text { text: label.to_owned(), style: ui_font(11.0, 700), color: fg, line_height: 28.0 };
        };
        let bottom = rect.height() - 6.0;
        if let Some(name) = self.workspace_name() {
            section(&mut tree, 33.0, &name, true);
        }
        section(&mut tree, bottom - 56.0, "Outline", false);
        section(&mut tree, bottom - 28.0, "Timeline", false);
        // O explorador retido fica entre o cabeçalho da pasta e o Outline.
        let list = Rect::new(rect.x0 + 5.0, rect.y0 + 61.0, rect.x1 - 6.0, rect.y0 + bottom - 56.0 - 1.0);
        tree.set_bounds(self.nodes.explorer, list);
        tree.style_mut(self.nodes.explorer).visible = self.explorer.is_some();
        drop(tree);
        if let Some(explorer) = &self.explorer {
            explorer.layout(list.width(), list.height());
        }
    }

    fn render_editor_part(&mut self, chrome: NodeId) {
        let part = self.editor_part_rect();
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
            tree.style_mut(label).content = Content::Text { text: editor.name.clone(), style: tab_font.clone(), color: fg, line_height: 24.0 };
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
        for (i, id) in ["split-horizontal", "ellipsis"].iter().enumerate() {
            let button = tree.create(strip, Role::Button);
            let bx = inner_width - 4.0 - 22.0 * (2 - i) as f64 - 4.0 * (1 - i) as f64;
            tree.set_bounds(button, Rect::new(bx, 4.5, bx + 22.0, 26.5));
            tree.style_mut(button).content = Content::Codicon { id: (*id).into(), size: 16.0, color: active_fg };
        }
        // Os breadcrumbs do editor ativo: ícone, nome e o `…` dos símbolos.
        let active = &self.editors[self.active.unwrap_or(0)];
        let crumbs = tree.create(frame, Role::Group);
        tree.set_bounds(crumbs, Rect::new(1.0, 1.0 + TABS_HEIGHT, 1.0 + inner_width, 1.0 + TABS_HEIGHT + BREADCRUMBS_HEIGHT));
        tree.style_mut(crumbs).background = Some(self.color("breadcrumb.background"));
        let crumb_fg = self.color("breadcrumb.foreground");
        let mut cx = 20.0;
        if let Some(icons) = &self.icons {
            let language = active.view.model().get_language_id();
            icons.decorate(&mut tree, crumbs, Rect::new(cx - 2.0, 0.0, cx + 16.0, BREADCRUMBS_HEIGHT), &active.name, Some(language.as_str()));
        }
        cx += 18.0;
        let crumb_font = ui_font(13.0, 400);
        let name_width = measure_text(&active.name, &crumb_font);
        let name = tree.create(crumbs, Role::Link);
        tree.set_bounds(name, Rect::new(cx, 0.0, cx + name_width, BREADCRUMBS_HEIGHT));
        tree.style_mut(name).content = Content::Text { text: active.name.clone(), style: crumb_font.clone(), color: crumb_fg, line_height: BREADCRUMBS_HEIGHT };
        cx += name_width + 1.0;
        let chevron = tree.create(crumbs, Role::Generic);
        tree.set_bounds(chevron, Rect::new(cx, 3.0, cx + 16.0, 19.0));
        tree.style_mut(chevron).content = Content::Codicon { id: "chevron-right".into(), size: 16.0, color: crumb_fg };
        let more = tree.create(crumbs, Role::Generic);
        tree.set_bounds(more, Rect::new(cx + 17.0, 0.0, cx + 40.0, BREADCRUMBS_HEIGHT));
        tree.style_mut(more).content = Content::Text { text: "\u{2026}".to_owned(), style: crumb_font, color: crumb_fg, line_height: BREADCRUMBS_HEIGHT };
        // A superfície do editor.
        let host = tree.create(frame, Role::TextInput);
        tree.set_bounds(host, Rect::new(1.0, 1.0 + TABS_HEIGHT + BREADCRUMBS_HEIGHT, 1.0 + inner_width, part.height() - 1.0));
        let style = tree.style_mut(host);
        style.content = Content::Host(0);
        style.cursor = Some(Cursor::Text);
        drop(tree);
        self.nodes.editor_host = Some(host);
        for (index, tab, close) in tab_nodes {
            let queue = self.queue.clone();
            let _ = self.ui.listen(tab, EventKind::PointerDown, false, move |e| {
                if e.pointer.is_some_and(|p| p.button == PointerButton::Auxiliary) {
                    queue.borrow_mut().push(Command::CloseEditor(index));
                } else {
                    queue.borrow_mut().push(Command::ActivateEditor(index));
                }
            });
            let queue = self.queue.clone();
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
        let measure = |pieces: &[Piece]| -> f64 {
            pieces.iter().map(|p| match p {
                Piece::Icon(_) => 16.0,
                Piece::Text(t) => measure_text(t, &font),
            }).sum::<f64>() + 14.0
        };
        let place = |tree: &mut Tree, x: f64, pieces: &[Piece]| -> f64 {
            let item_width = measure(pieces);
            let item = tree.create(bar, Role::Button);
            tree.set_bounds(item, Rect::new(x, 2.0, x + item_width, 26.0));
            tree.style_mut(item).radius = 4.0;
            let mut px = 7.0;
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
        left += place(&mut tree, left, &[Piece::Icon("remote")]);
        left += place(&mut tree, left, &[Piece::Icon("error"), Piece::Text(" 0 ".to_owned()), Piece::Icon("warning"), Piece::Text(" 0".to_owned())]);
        if let Some(notice) = &self.notice {
            place(&mut tree, left, &[Piece::Text(notice.clone())]);
        }
        let mut items: Vec<Vec<Piece>> = Vec::new();
        if let Some(editor) = self.active_editor() {
            let model = editor.view.model();
            let options = model.get_options();
            let eol = if model.get_eol() == je_editor::model::Eol::CrLf { "CRLF" } else { "LF" };
            let indent = if options.insert_spaces { format!("Spaces: {}", options.indent_size) } else { format!("Tab Size: {}", options.tab_size) };
            items.push(vec![Piece::Text(editor.view.status_text())]);
            items.push(vec![Piece::Text(indent)]);
            items.push(vec![Piece::Text("UTF-8".to_owned())]);
            items.push(vec![Piece::Text(eol.to_owned())]);
            items.push(vec![Piece::Icon("json")]);
            items.push(vec![Piece::Text(editor.language_name.clone())]);
        }
        items.push(vec![Piece::Icon("bell")]);
        let mut right = width - 6.0;
        for pieces in items.iter().rev() {
            right -= measure(pieces);
            place(&mut tree, right, pieces);
        }
    }

    /// Pinta um quadro em `list` e devolve a cor de fundo da janela.
    pub fn paint(&mut self, list: &mut DisplayList, scale: f64, now: Instant) -> Rgba {
        self.run_commands();
        self.ui.run_animation_frame();
        if self.needs_render {
            self.render();
        }
        let rect = self.editor_rect();
        let host = self.nodes.editor_host;
        let tree = self.ui.tree();
        let editor = self.active.map(|i| &mut self.editors[i]);
        let mut editor = editor;
        if let Some(e) = editor.as_deref_mut() {
            e.view.resize(rect.width(), rect.height());
        }
        tree.paint_hosted(list, scale, &mut |_, area, list| {
            let Some(e) = editor.as_deref_mut() else { return };
            let device = Rect::new(area.x0 * scale, area.y0 * scale, area.x1 * scale, area.y1 * scale);
            let radii = RoundedRectRadii::new(0.0, 0.0, 7.0 * scale, 7.0 * scale);
            list.push_clip(Shape::RoundedRect(RoundedRect::from_rect(device, radii)));
            e.view.paint(list, (area.x0, area.y0), scale, now);
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

    fn sash_hit(&self, x: f64, y: f64) -> bool {
        self.sidebar_rect().is_some_and(|r| x >= r.x1 && x < r.x1 + SASH && y >= r.y0 && y < r.y1)
    }

    /// Entrega uma entrada de ponteiro: ao editor quando ele é o alvo (ou está no meio de um arraste), à árvore
    /// do `Ui` sempre (menus fecham no clique fora, abas reagem).
    pub fn pointer(&mut self, input: PointerInput) {
        let (x, y) = (input.x, input.y);
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
                    PointerInputKind::Move => editor.view.mouse_drag(x - rect.x0, y - rect.y0),
                    PointerInputKind::Up => editor.view.mouse_up(),
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
        let target = self.ui.pointer_input(input);
        if input.kind == PointerInputKind::Down && Some(target) == self.nodes.editor_host && input.button == PointerButton::Primary {
            self.ui.blur();
            if let Some((lx, ly)) = self.editor_local(x, y)
                && let Some(editor) = self.active_editor_mut()
            {
                editor.view.set_focus(true);
                editor.view.mouse_down(lx, ly, input.button, input.click_count, input.modifiers);
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
        self.ui.cursor_at(x, y)
    }

    /// A roda do mouse: rola o editor se o ponteiro estiver nele.
    pub fn wheel(&mut self, x: f64, y: f64, dx: f64, dy: f64) {
        if let Some(explorer) = &self.explorer
            && self.sidebar_rect().is_some_and(|r| r.contains(je_render::kurbo::Point::new(x, y)))
        {
            explorer.scroll_by(dy);
            return;
        }
        if self.editor_local(x, y).is_some()
            && let Some(editor) = self.active_editor_mut()
        {
            editor.view.scroll_by(dx, dy);
        }
    }

    /// Os atalhos do workbench que valem com o foco no editor (os `KeybindingsRegistry` globais).
    fn workbench_shortcut(&self, key: KeyCode, mods: Modifiers) -> Option<Command> {
        let ctrl = mods.ctrl || mods.meta;
        Some(match (ctrl, mods.shift, mods.alt, key) {
            (true, false, false, KeyCode::KeyN) => Command::NewUntitled,
            (true, false, false, KeyCode::KeyS) => Command::Save,
            (true, false, false, KeyCode::KeyW) | (true, false, false, KeyCode::F4) => Command::CloseActiveEditor,
            (true, false, false, KeyCode::KeyQ) => Command::Exit,
            (true, false, false, KeyCode::KeyB) => Command::ToggleSidebar,
            (true, true, false, KeyCode::KeyE) => Command::ShowExplorer,
            (true, false, false, KeyCode::PageDown) => Command::NextEditor,
            (true, false, false, KeyCode::PageUp) => Command::PreviousEditor,
            (false, false, false, KeyCode::F11) => Command::ToggleFullScreen,
            _ => return None,
        })
    }

    /// Entrega uma tecla: primeiro à árvore do `Ui` (a barra de menus escuta Alt e os mnemônicos na raiz); se nenhum
    /// widget tem o foco e ninguém consumiu, aos atalhos do workbench e então ao editor.
    pub fn key(&mut self, down: bool, key_code: KeyCode, mods: Modifiers, text: Option<&str>, editor_key: Option<EditorKey>) {
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
        if let Some(command) = self.workbench_shortcut(key_code, mods) {
            self.run(command);
            return;
        }
        let Some(editor) = self.active_editor_mut() else { return };
        if let Some(key) = editor_key {
            editor.view.key_down(key, mods);
            return;
        }
        let ctrl = mods.ctrl || mods.meta;
        if (!ctrl || mods.alt)
            && let Some(text) = text
        {
            editor.view.type_text(text);
        }
    }

    pub fn set_window_focus(&mut self, focused: bool) {
        if let Some(editor) = self.active_editor_mut() {
            editor.view.set_focus(focused);
        }
        if !focused {
            self.menubar.blur();
        }
    }

    /// Avança o que é assíncrono (comandos, quadros de animação) e diz se precisa de quadro novo.
    pub fn take_dirty(&mut self) -> bool {
        self.run_commands();
        let editor = self.active.is_some_and(|i| self.editors[i].view.take_dirty());
        editor || self.needs_render || self.ui.has_pending_animation_frame()
    }

    pub fn next_blink_deadline(&self, now: Instant) -> Option<Instant> {
        self.active_editor().map(|e| e.view.next_blink_deadline(now))
    }
}
