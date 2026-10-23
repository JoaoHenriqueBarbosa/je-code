//! O executável do je-code. Mostra a moldura do workbench (título, activity bar, editor e barra de status) com o
//! tema 2026 Dark do VS Code, os codicons, e o arquivo dado na linha de comando num editor de verdade
//! (`je-editor-view`): cores de token do tema, cursor, seleção, edição, desfazer e salvar.
//!
//! Uso: `je-code [arquivo]` (sem argumento abre um arquivo vazio). `JE_RENDERER=gpu|cpu` força o caminho de
//! pintura; `JE_THEME` aponta outro tema JSON do VS Code; `JE_BUILTIN_EXTENSIONS` aponta a pasta `extensions`
//! do VS Code (as gramáticas e linguagens); sem ela o editor procura ao lado do executável e, sem achar, abre
//! o arquivo sem cores de token. `je-code --screenshot saida.png [arquivo]` pinta um quadro de 1200x800 pelo
//! caminho de CPU e sai (usado nas comparações com o oráculo, H26). `je-code --bench [arquivo]` rola e digita
//! na janela com quadros de verdade e imprime o tempo por quadro (H28).
//!
//! Salvar (Ctrl+S) grava com `std::fs::write`: o `je-platform::files::FileService` exige o grafo de serviços
//! do workbench (URI, provedores, watcher), que entra na Fase 4 do `roadmap.md`.
// TODO(integração): salvar pelo `FileService` (escrita atômica, `files.encoding`, `files.eol`) quando o
// workbench existir.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use je_base::runtime;
use je_editor::model::EndOfLinePreference;
use je_editor::text_model::{TextModel, TextSource};
use je_editor::undo_redo_service::UndoRedoService;
use je_editor_view::environment::find_builtin_extensions;
use je_editor_view::{EditorColors, EditorEnvironment, EditorKey, EditorView};
use je_render::kurbo::{Rect, RoundedRect};
use je_render::{Backend, DisplayList, Preference, Presenter, Rgba, Shape};
use je_text::{TextStyle, with_text_system};
use je_theme::ColorTheme;
use je_ui::{Border, Content, Modifiers, PointerButton, Role, Tree};
use web_time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalPosition, LogicalSize};
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::{CursorIcon, Window, WindowId};

const UI_FONT: &str = "system-ui, \"Ubuntu\", \"Droid Sans\", sans-serif";
/// O tema padrão relativo à pasta de extensões embutidas.
const BUILTIN_DEFAULT_THEME: &str = "theme-defaults/themes/2026-dark.json";
#[cfg(test)]
const DEFAULT_THEME: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../je-theme/tests/golden/themes/2026-dark.json");
/// O intervalo máximo entre cliques para contar como duplo e triplo, e a distância máxima entre eles.
const MULTI_CLICK: Duration = Duration::from_millis(500);
const MULTI_CLICK_DISTANCE: f64 = 4.0;

/// O espaço do editor dentro da janela: o painel flutuante, com a borda de 1 px dentro dele.
fn editor_panel(width: f64, height: f64) -> Rect {
    Rect::new(52.0, 35.0, width - 4.0, height - 32.0)
}

fn editor_view_rect(width: f64, height: f64) -> Rect {
    editor_panel(width, height).inset(-1.0)
}

struct Workbench {
    theme: ColorTheme,
    editor: EditorView,
    path: Option<PathBuf>,
    file_name: String,
    language_name: String,
    /// A versão do modelo no último salvamento (para o indicador de arquivo modificado).
    saved_version: i64,
    list: DisplayList,
    /// Falha do último salvamento, mostrada na barra de status.
    notice: Option<String>,
    // Mantém o ambiente de linguagens vivo (gramáticas, configuração, tokenização).
    _environment: Option<EditorEnvironment>,
}

impl Workbench {
    fn open(theme_path: &Path, file: Option<&Path>, width: f64, height: f64) -> Result<Self, String> {
        let theme = ColorTheme::load(theme_path).map_err(|e| format!("tema inválido: {e}"))?;
        let (file_name, text) = match file {
            Some(path) => {
                let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
                let name = path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
                (name, String::from_utf8_lossy(&bytes).into_owned())
            }
            None => ("untitled".to_owned(), String::new()),
        };
        let environment = match find_builtin_extensions() {
            Some(dir) => match EditorEnvironment::load(&dir, theme_path) {
                Ok(env) => Some(env),
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
        let (model, language_name) = match &environment {
            Some(env) => {
                let model = env.open(&file_name, &text);
                let name = env.language_name(&model.get_language_id());
                (model, name)
            }
            None => {
                let mut options = TextModel::default_creation_options();
                options.detect_indentation = true;
                (TextModel::new(TextSource::from(text.as_str()), "plaintext", options, None, UndoRedoService::for_tests()), "Plain Text".to_owned())
            }
        };
        let rect = editor_view_rect(width, height);
        let mut editor = EditorView::new(model.clone(), rect.width(), rect.height(), EditorColors::from_theme(&theme));
        if let Some(env) = &environment {
            editor.set_token_colors(&env.color_map());
        }
        let saved_version = model.get_alternative_version_id();
        Ok(Self { theme, editor, path: file.map(Path::to_path_buf), file_name, language_name, saved_version, list: DisplayList::new(), notice: None, _environment: environment })
    }

    fn color(&self, id: &str) -> Rgba {
        self.theme.get_color(id).unwrap_or(Rgba::opaque(0xff, 0x00, 0xff))
    }

    fn is_modified(&self) -> bool {
        self.editor.model().get_alternative_version_id() != self.saved_version
    }

    fn save(&mut self) {
        let Some(path) = self.path.clone() else {
            self.notice = Some("sem arquivo: nada a salvar".to_owned());
            return;
        };
        let model = self.editor.model().clone();
        let text = model.get_value_with(EndOfLinePreference::TextDefined, true).to_string_lossy();
        match std::fs::write(&path, text.as_bytes()) {
            Ok(()) => {
                self.saved_version = model.get_alternative_version_id();
                self.notice = None;
            }
            Err(e) => self.notice = Some(format!("falha ao salvar: {e}")),
        }
        self.editor.mark_dirty();
    }

    /// Monta a moldura (título, activity bar, painel do editor e barra de status).
    fn build(&mut self, width: f64, height: f64, backend: &str) -> Tree {
        let mut tree = Tree::new();
        let root = tree.root();
        tree.set_bounds(root, Rect::new(0.0, 0.0, width, height));
        let foreground = self.color("foreground");
        let muted = self.color("statusBar.foreground");
        let ui = |size: f32| TextStyle::new(UI_FONT, size, 400);

        // Barra de título.
        let title = tree.create(root, Role::Region);
        tree.set_bounds(title, Rect::new(0.0, 0.0, width, 35.0));
        tree.style_mut(title).background = Some(self.color("titleBar.activeBackground"));
        let center = tree.create(title, Role::Generic);
        let cw = 460.0_f64.min(width - 360.0).max(120.0);
        tree.set_bounds(center, Rect::new((width - cw) / 2.0, 5.0, (width + cw) / 2.0, 30.0));
        let style = tree.style_mut(center);
        style.background = Some(self.color("commandCenter.background"));
        style.border = Some(Border { width: 1.0, color: self.color("commandCenter.border") });
        style.radius = 6.0;
        let marker = if self.is_modified() { " \u{25CF}" } else { "" };
        style.content = Content::Text { text: format!("  je-code  /  {}{marker}", self.file_name), style: ui(12.0), color: foreground, line_height: 25.0 };

        // Activity bar: painel flutuante com os ícones (ids do iconRegistry resolvidos para codicons).
        let activity = tree.create(root, Role::Toolbar);
        tree.set_bounds(activity, Rect::new(4.0, 35.0, 48.0, height - 32.0));
        let style = tree.style_mut(activity);
        style.background = Some(self.color("activityBar.background"));
        style.border = Some(Border { width: 1.0, color: Rgba::opaque(0x25, 0x25, 0x26) });
        style.radius = 8.0;
        let inactive = self.color("activityBar.inactiveForeground");
        let top = ["files", "search-large", "source-control", "debug-alt", "extensions"];
        for (i, id) in top.iter().enumerate() {
            let button = tree.create(activity, Role::Button);
            tree.set_bounds(button, Rect::new(4.0, 4.0 + 44.0 * i as f64, 40.0, 40.0 + 44.0 * i as f64));
            tree.style_mut(button).content = Content::Codicon { id: (*id).into(), size: 24.0, color: if i == 0 { foreground } else { inactive } };
        }
        for (i, id) in ["account", "settings-gear"].iter().enumerate() {
            let button = tree.create(activity, Role::Button);
            let y = height - 32.0 - 35.0 - 44.0 * (2 - i) as f64 - 4.0;
            tree.set_bounds(button, Rect::new(4.0, y, 40.0, y + 36.0));
            tree.style_mut(button).content = Content::Codicon { id: (*id).into(), size: 24.0, color: inactive };
        }

        // Painel do editor: fundo, borda e raio; o conteúdo é do `EditorView`, pintado por cima.
        let editor = tree.create(root, Role::Region);
        let panel = editor_panel(width, height);
        tree.set_bounds(editor, panel);
        let style = tree.style_mut(editor);
        style.background = Some(self.color("editor.background"));
        style.border = Some(Border { width: 1.0, color: self.color("editorGroup.border") });
        style.radius = 8.0;

        // Barra de status: o caminho de pintura à esquerda; posição, indentação, codificação, fim de linha e
        // linguagem à direita, cada item com o respiro de 10 px de cada lado.
        let status = tree.create(root, Role::Status);
        tree.set_bounds(status, Rect::new(0.0, height - 28.0, width, height));
        tree.style_mut(status).background = Some(self.color("statusBar.background"));
        let left = self.notice.clone().unwrap_or_else(|| format!("pintura: {backend}"));
        let info = tree.create(status, Role::Generic);
        tree.set_bounds(info, Rect::new(12.0, 0.0, width / 3.0, 28.0));
        tree.style_mut(info).content = Content::Text { text: left, style: ui(12.0), color: muted, line_height: 28.0 };
        let options = self.editor.model().get_options();
        let eol = if self.editor.model().get_eol() == je_editor::model::Eol::CrLf { "CRLF" } else { "LF" };
        let indent = if options.insert_spaces { format!("Spaces: {}", options.indent_size) } else { format!("Tab Size: {}", options.tab_size) };
        let items = [self.editor.status_text(), indent, "UTF-8".to_owned(), eol.to_owned(), self.language_name.clone()];
        let mut right = width - 12.0;
        for text in items.iter().rev() {
            let style = ui(12.0);
            let item_width = f64::from(with_text_system(|t| t.line(text, &style)).width);
            let node = tree.create(status, Role::Generic);
            tree.set_bounds(node, Rect::new(right - item_width, 0.0, right, 28.0));
            tree.style_mut(node).content = Content::Text { text: text.clone(), style, color: muted, line_height: 28.0 };
            right -= item_width + 20.0;
        }
        tree
    }

    /// Pinta um quadro completo em `self.list` (moldura e editor) e devolve a cor de fundo.
    fn paint(&mut self, width: f64, height: f64, scale: f64, backend: &str, now: Instant) -> Rgba {
        let rect = editor_view_rect(width, height);
        self.editor.resize(rect.width(), rect.height());
        let tree = self.build(width, height, backend);
        self.list.clear();
        tree.paint(&mut self.list, scale);
        let device = Rect::new(rect.x0 * scale, rect.y0 * scale, rect.x1 * scale, rect.y1 * scale);
        self.list.push_clip(Shape::RoundedRect(RoundedRect::from_rect(device, 7.0 * scale)));
        self.editor.paint(&mut self.list, (rect.x0, rect.y0), scale, now);
        self.list.pop_clip();
        self.color("editor.background")
    }
}

/// O botão do mouse em andamento e o rastreio de cliques múltiplos.
#[derive(Default)]
struct PointerState {
    position: (f64, f64),
    pressed: bool,
    last_click: Option<(Instant, (f64, f64))>,
    click_count: u32,
}

struct App {
    workbench: Workbench,
    window: Option<Arc<Window>>,
    presenter: Option<Presenter>,
    backend_label: String,
    modifiers: Modifiers,
    pointer: PointerState,
    /// `--bench`: mede o tempo por quadro de rolagem e de digitação na janela e sai.
    bench: bool,
    /// Pintura e `present` de cada quadro medido.
    split: Vec<(f64, f64)>,
}

/// Mínimo, mediana, p95, p99 e máximo de uma lista de tempos em ms.
fn summarize(label: &str, mut samples: Vec<f64>) {
    samples.sort_by(f64::total_cmp);
    let at = |q: f64| samples[((samples.len() - 1) as f64 * q).round() as usize];
    let mean = samples.iter().sum::<f64>() / samples.len() as f64;
    println!(
        "{label:<34} n={:<4} média {mean:6.2} ms ({:5.0} fps)  p50 {:6.2}  p95 {:6.2}  p99 {:6.2}  máx {:6.2}",
        samples.len(),
        1000.0 / mean,
        at(0.5),
        at(0.95),
        at(0.99),
        samples[samples.len() - 1]
    );
}

fn resident_mb() -> f64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| s.lines().find(|l| l.starts_with("VmRSS:")).and_then(|l| l.split_whitespace().nth(1)?.parse::<f64>().ok()))
        .map_or(f64::NAN, |kb| kb / 1024.0)
}

impl App {
    /// Rola e digita com quadros de verdade (pintura e `present`) e imprime o tempo por quadro.
    fn run_bench(&mut self) {
        let lines = self.workbench.editor.model().get_line_count();
        println!("arquivo: {lines} linhas, tokenização {}, pintura {}", if self.workbench.editor.model().is_too_large_for_tokenization() { "desligada (arquivo grande)" } else { "ligada" }, self.backend_label);
        let timed = |app: &mut App| {
            let (paint, present) = app.frame_timed();
            app.split.push((paint, present));
            paint + present
        };
        for _ in 0..10 {
            timed(self);
            runtime::turn();
        }
        // Rolagem: 3 linhas por quadro, como a roda do mouse, do topo para baixo. O loop de eventos do runtime
        // roda entre os quadros (a tokenização em fatias disputa o quadro, como no uso real).
        let step = 3.0 * self.workbench.editor.layout().line_height();
        let (mut frames, mut turns) = (Vec::new(), Vec::new());
        for _ in 0..600 {
            self.workbench.editor.scroll_by(0.0, step);
            frames.push(timed(self));
            let started = Instant::now();
            runtime::turn();
            turns.push(started.elapsed().as_secs_f64() * 1000.0);
        }
        summarize("quadro de rolagem (pintura+present)", frames);
        let paints: Vec<f64> = self.split[10..610].iter().map(|s| s.0).collect();
        let presents: Vec<f64> = self.split[10..610].iter().map(|s| s.1).collect();
        summarize("  montar a lista de exibição", paints);
        summarize("  present (rasterizar e trocar)", presents);
        self.split.clear();
        summarize("  loop do runtime entre quadros", turns);
        // Saltos: pula metade do arquivo de uma vez (cache frio).
        let mut jumps = Vec::new();
        for i in 0..40 {
            let top = f64::from(lines) * 19.0 * (f64::from(i % 20) / 20.0);
            self.workbench.editor.set_scroll_top(top);
            jumps.push(timed(self));
        }
        summarize("quadro após salto (cache frio)", jumps);
        // Digitação no meio do arquivo: a edição sozinha e o quadro seguinte.
        self.workbench.editor.set_cursor(je_editor::position::Position::new(lines / 2, 1));
        self.workbench.editor.reveal_cursor();
        timed(self);
        let (mut edits, mut after) = (Vec::new(), Vec::new());
        for _ in 0..300 {
            let started = Instant::now();
            self.workbench.editor.type_text("a");
            edits.push(started.elapsed().as_secs_f64() * 1000.0);
            after.push(timed(self));
            runtime::turn();
        }
        summarize("edição (type_text)", edits);
        summarize("quadro após digitar", after);
        let paints: Vec<f64> = self.split.iter().rev().take(300).map(|s| s.0).collect();
        let presents: Vec<f64> = self.split.iter().rev().take(300).map(|s| s.1).collect();
        summarize("  montar a lista de exibição", paints);
        summarize("  present (rasterizar e trocar)", presents);
        println!("memória residente: {:.0} MB", resident_mb());
    }

    fn logical_size(&self) -> Option<(f64, f64)> {
        let window = self.window.as_ref()?;
        let size = window.inner_size().to_logical::<f64>(window.scale_factor());
        Some((size.width, size.height))
    }

    fn frame(&mut self) {
        self.frame_timed();
    }

    /// Pinta um quadro e devolve o tempo de montar a lista de exibição e o de `present` (rasterização e troca de
    /// buffer), em ms.
    fn frame_timed(&mut self) -> (f64, f64) {
        let (Some(window), Some(presenter)) = (&self.window, &mut self.presenter) else { return (0.0, 0.0) };
        let scale = window.scale_factor();
        let size = window.inner_size().to_logical::<f64>(scale);
        let started = Instant::now();
        let background = self.workbench.paint(size.width, size.height, scale, &self.backend_label, started);
        let painted = Instant::now();
        if let Err(e) = presenter.present(&self.workbench.list, background) {
            eprintln!("je-code: frame descartado: {e}");
        }
        (painted.duration_since(started).as_secs_f64() * 1000.0, painted.elapsed().as_secs_f64() * 1000.0)
    }

    /// Posição do ponteiro relativa ao editor, se estiver sobre ele.
    fn editor_local(&self, x: f64, y: f64) -> Option<(f64, f64)> {
        let (width, height) = self.logical_size()?;
        let rect = editor_view_rect(width, height);
        (x >= rect.x0 && x < rect.x1 && y >= rect.y0 && y < rect.y1).then_some((x - rect.x0, y - rect.y0))
    }

    fn editor_origin(&self) -> (f64, f64) {
        self.logical_size().map_or((0.0, 0.0), |(w, h)| {
            let rect = editor_view_rect(w, h);
            (rect.x0, rect.y0)
        })
    }

    /// Roda o loop de eventos do runtime (tokenização em fatias, timers) e pede quadro se algo mudou. Devolve
    /// até quando o loop da janela pode dormir.
    fn tick(&mut self, event_loop: &ActiveEventLoop) {
        runtime::turn();
        if self.workbench.editor.take_dirty()
            && let Some(w) = &self.window
        {
            w.request_redraw();
        }
        let now = Instant::now();
        let mut deadline = self.workbench.editor.next_blink_deadline(now);
        if let Some(wait) = runtime::time_to_next_timer() {
            deadline = deadline.min(now + wait);
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(deadline));
    }

    fn key(&mut self, event: &winit::event::KeyEvent) {
        let mods = self.modifiers;
        let ctrl = mods.ctrl || mods.meta;
        let editor = &mut self.workbench.editor;
        let key = match &event.logical_key {
            Key::Named(NamedKey::ArrowLeft) => Some(EditorKey::Left),
            Key::Named(NamedKey::ArrowRight) => Some(EditorKey::Right),
            Key::Named(NamedKey::ArrowUp) => Some(EditorKey::Up),
            Key::Named(NamedKey::ArrowDown) => Some(EditorKey::Down),
            Key::Named(NamedKey::Home) => Some(EditorKey::Home),
            Key::Named(NamedKey::End) => Some(EditorKey::End),
            Key::Named(NamedKey::PageUp) => Some(EditorKey::PageUp),
            Key::Named(NamedKey::PageDown) => Some(EditorKey::PageDown),
            Key::Named(NamedKey::Backspace) => Some(EditorKey::Backspace),
            Key::Named(NamedKey::Delete) => Some(EditorKey::Delete),
            Key::Named(NamedKey::Enter) => Some(EditorKey::Enter),
            Key::Named(NamedKey::Tab) => Some(EditorKey::Tab),
            Key::Named(NamedKey::Escape) => Some(EditorKey::Escape),
            Key::Character(s) if ctrl && !mods.alt => s.chars().next().map(|c| EditorKey::Char(c.to_ascii_lowercase())),
            _ => None,
        };
        if let Some(key) = key {
            if key == EditorKey::Char('s') {
                self.workbench.save();
            } else {
                editor.key_down(key, mods);
            }
            return;
        }
        // Texto digitado (inclusive espaço e AltGr, que no Windows chega como Ctrl+Alt).
        if (!ctrl || mods.alt)
            && let Some(text) = &event.text
        {
            editor.type_text(text);
        }
    }

    fn mouse_button(&mut self, state: ElementState, button: MouseButton) {
        let button = match button {
            MouseButton::Left => PointerButton::Primary,
            _ => return,
        };
        let (x, y) = self.pointer.position;
        match state {
            ElementState::Pressed => {
                let Some((lx, ly)) = self.editor_local(x, y) else { return };
                let now = Instant::now();
                let repeated = self.pointer.last_click.is_some_and(|(at, (px, py))| now.duration_since(at) < MULTI_CLICK && (px - x).abs() < MULTI_CLICK_DISTANCE && (py - y).abs() < MULTI_CLICK_DISTANCE);
                self.pointer.click_count = if repeated { self.pointer.click_count % 3 + 1 } else { 1 };
                self.pointer.last_click = Some((now, (x, y)));
                self.pointer.pressed = true;
                self.workbench.editor.mouse_down(lx, ly, button, self.pointer.click_count, self.modifiers);
            }
            ElementState::Released => {
                self.pointer.pressed = false;
                self.workbench.editor.mouse_up();
            }
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let name = self.workbench.file_name.clone();
        let attrs = Window::default_attributes().with_title(format!("{name} - je-code")).with_inner_size(LogicalSize::new(1200.0, 800.0));
        let window = match event_loop.create_window(attrs) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                eprintln!("je-code: não foi possível abrir a janela: {e}");
                event_loop.exit();
                return;
            }
        };
        match Presenter::new(window.clone(), Preference::from_env()) {
            Ok(presenter) => {
                self.backend_label = match presenter.backend() {
                    Backend::Gpu { adapter, api } => format!("GPU {adapter} ({api})"),
                    Backend::Cpu { threads } => format!("CPU, {threads} threads"),
                };
                eprintln!("je-code: pintando com {:?}", presenter.backend());
                self.presenter = Some(presenter);
            }
            Err(e) => {
                eprintln!("je-code: sem backend de pintura: {e}");
                event_loop.exit();
                return;
            }
        }
        window.request_redraw();
        self.window = Some(window);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => {
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
            }
            WindowEvent::Focused(focused) => self.workbench.editor.set_focus(focused),
            WindowEvent::ModifiersChanged(m) => {
                let s = m.state();
                self.modifiers = Modifiers { alt: s.alt_key(), ctrl: s.control_key(), shift: s.shift_key(), meta: s.super_key() };
            }
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => self.key(&event),
            WindowEvent::CursorMoved { position, .. } => {
                let scale = self.window.as_ref().map_or(1.0, |w| w.scale_factor());
                let LogicalPosition { x, y } = position.to_logical::<f64>(scale);
                self.pointer.position = (x, y);
                if self.pointer.pressed {
                    let (ox, oy) = self.editor_origin();
                    self.workbench.editor.mouse_drag(x - ox, y - oy);
                }
                if let Some(w) = &self.window {
                    w.set_cursor(if self.editor_local(x, y).is_some() { CursorIcon::Text } else { CursorIcon::Default });
                }
            }
            WindowEvent::MouseInput { state, button, .. } => self.mouse_button(state, button),
            WindowEvent::MouseWheel { delta, .. } => {
                let line_height = self.workbench.editor.layout().line_height();
                let (dx, dy) = match delta {
                    MouseScrollDelta::LineDelta(x, y) => (-f64::from(x) * 3.0 * line_height, -f64::from(y) * 3.0 * line_height),
                    MouseScrollDelta::PixelDelta(p) => (-p.x, -p.y),
                };
                // Shift + roda rola na horizontal, como no VS Code.
                if self.modifiers.shift { self.workbench.editor.scroll_by(dy, 0.0) } else { self.workbench.editor.scroll_by(dx, dy) }
            }
            WindowEvent::RedrawRequested => self.frame(),
            _ => {}
        }
        self.tick(event_loop);
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.bench && self.presenter.is_some() {
            self.bench = false;
            self.run_bench();
            event_loop.exit();
            return;
        }
        self.tick(event_loop);
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
fn build_event_loop() -> Result<EventLoop<()>, winit::error::EventLoopError> {
    use winit::platform::x11::EventLoopBuilderExtX11;
    let mut builder = EventLoop::builder();
    if je_render::wsl::prefer_x11() {
        builder.with_x11();
    }
    builder.build()
}

#[cfg(not(all(unix, not(target_os = "macos"))))]
fn build_event_loop() -> Result<EventLoop<()>, winit::error::EventLoopError> {
    EventLoop::new()
}

/// Pinta um quadro de `width` x `height` pelo caminho de CPU e grava em PNG (sem janela).
fn screenshot(workbench: &mut Workbench, out: &Path, width: u16, height: u16) -> Result<(), String> {
    // Tokeniza o arquivo inteiro antes de pintar: a captura precisa ser determinística.
    let model = workbench.editor.model().clone();
    model.tokenization().force_tokenization(model.get_line_count().min(2000));
    runtime::turn();
    let background = workbench.paint(f64::from(width), f64::from(height), 1.0, "CPU", Instant::now());
    let rgba = je_render::render_rgba(&workbench.list, background, width, height);
    let file = std::fs::File::create(out).map_err(|e| format!("{}: {e}", out.display()))?;
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), u32::from(width), u32::from(height));
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header().and_then(|mut w| w.write_image_data(&rgba)).map_err(|e| e.to_string())
}

fn main() {
    // No WSL a GPU só aparece com o driver d3d12 do Mesa; relança com o ambiente certo (H27).
    if let Some(e) = je_render::wsl::relaunch_with_gpu() {
        eprintln!("je-code: relançamento com a GPU do WSL falhou ({e}); seguindo sem ele");
    }
    let mut args = std::env::args_os().skip(1).peekable();
    let bench = args.peek().is_some_and(|a| a == "--bench");
    if bench {
        args.next();
    }
    let screenshot_out = if args.peek().is_some_and(|a| a == "--screenshot") {
        args.next();
        args.next().map(PathBuf::from)
    } else {
        None
    };
    let file = args.next().map(PathBuf::from);
    // O tema padrão vem da extensão embutida `theme-defaults`, como no VS Code; `JE_THEME` troca o arquivo.
    let theme_path = match std::env::var_os("JE_THEME") {
        Some(path) => PathBuf::from(path),
        None => match find_builtin_extensions() {
            Some(dir) => dir.join(BUILTIN_DEFAULT_THEME),
            None => {
                eprintln!("je-code: pasta de extensões não encontrada (defina JE_BUILTIN_EXTENSIONS ou JE_THEME)");
                std::process::exit(1);
            }
        },
    };
    let mut workbench = match Workbench::open(&theme_path, file.as_deref(), 1200.0, 800.0) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("je-code: {e}");
            std::process::exit(1);
        }
    };
    if let Some(out) = screenshot_out {
        if let Err(e) = screenshot(&mut workbench, &out, 1200, 800) {
            eprintln!("je-code: {e}");
            std::process::exit(1);
        }
        return;
    }
    let event_loop = match build_event_loop() {
        Ok(l) => l,
        Err(e) => {
            eprintln!("je-code: sem event loop: {e}");
            std::process::exit(1);
        }
    };
    let mut app = App { workbench, window: None, presenter: None, backend_label: String::new(), modifiers: Modifiers::default(), pointer: PointerState::default(), bench, split: Vec::new() };
    if let Err(e) = event_loop.run_app(&mut app) {
        eprintln!("je-code: {e}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_file(name: &str, contents: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("je-code-{}-{name}", std::process::id()));
        std::fs::write(&path, contents).expect("arquivo temporário");
        path
    }

    #[test]
    fn editor_view_stays_inside_panel() {
        let panel = editor_panel(1200.0, 800.0);
        let view = editor_view_rect(1200.0, 800.0);
        assert_eq!((view.x0, view.y0, view.x1, view.y1), (panel.x0 + 1.0, panel.y0 + 1.0, panel.x1 - 1.0, panel.y1 - 1.0));
    }

    #[test]
    fn save_writes_edited_text_and_keeps_eol() {
        let path = temp_file("save.txt", "ab\r\ncd\r\n");
        let mut workbench = Workbench::open(Path::new(DEFAULT_THEME), Some(&path), 1200.0, 800.0).expect("abre");
        assert!(!workbench.is_modified());
        workbench.editor.set_cursor(je_editor::position::Position::new(1, 2));
        workbench.editor.type_text("X");
        assert!(workbench.is_modified(), "digitar marca o arquivo como modificado");
        workbench.editor.undo();
        assert!(!workbench.is_modified(), "desfazer até o texto salvo limpa a marca");
        workbench.editor.redo();
        workbench.save();
        assert!(!workbench.is_modified());
        assert_eq!(std::fs::read_to_string(&path).expect("lê"), "aXb\r\ncd\r\n");
        std::fs::remove_file(&path).expect("remove");
    }

    #[test]
    fn frame_paints_chrome_and_text() {
        let path = temp_file("frame.ts", "const answer = 42;\n");
        let mut workbench = Workbench::open(Path::new(DEFAULT_THEME), Some(&path), 1200.0, 800.0).expect("abre");
        let background = workbench.paint(1200.0, 800.0, 1.0, "CPU", Instant::now());
        assert_eq!(background, Rgba::opaque(0x12, 0x13, 0x14));
        let rgba = je_render::render_rgba(&workbench.list, background, 1200, 800);
        let pixel = |x: usize, y: usize| [rgba[(y * 1200 + x) * 4], rgba[(y * 1200 + x) * 4 + 1], rgba[(y * 1200 + x) * 4 + 2]];
        // O texto do editor está dentro do painel, e a barra de status tem a cor do tema.
        let text_region_has_ink = (36..56).any(|y| (123..260).any(|x| pixel(x, y) != pixel(900, 400) && pixel(x, y) != [0x24, 0x25, 0x26]));
        assert!(text_region_has_ink, "a primeira linha foi pintada");
        assert_ne!(pixel(600, 790), pixel(600, 400), "a barra de status tem fundo próprio");
        std::fs::remove_file(&path).expect("remove");
    }
}
