//! O executável do je-code: o workbench (ver [`shell`]) com o tema 2026 Dark do VS Code, a barra de menus, o
//! explorador, as abas e os editores de texto (`je-editor-view`).
//!
//! Uso: `je-code [pasta] [arquivos...]`, como o `code`: a pasta vira o workspace do explorador e os arquivos abrem
//! em abas (sem nada, abre um arquivo sem título). `JE_RENDERER=gpu|cpu` força o caminho de
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

mod shell;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use je_base::key_codes::{KeyCode, KeyCodeUtils, ScanCodeUtils, immutable_code_to_key_code};
use je_base::runtime;
use je_editor_view::EditorKey;
use je_editor_view::environment::find_builtin_extensions;
use je_render::{Backend, DisplayList, Preference, Presenter};
use je_ui::{Cursor, Modifiers, PointerButton, PointerInput, PointerInputKind};
use shell::{BUILTIN_DEFAULT_THEME, Shell};
use web_time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalPosition, LogicalSize};
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey, PhysicalKey};
use winit::window::{CursorIcon, Fullscreen, Window, WindowId};

#[cfg(test)]
const DEFAULT_THEME: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../je-theme/tests/golden/themes/2026-dark.json");
/// O intervalo máximo entre cliques para contar como duplo e triplo, e a distância máxima entre eles.
const MULTI_CLICK: Duration = Duration::from_millis(500);
const MULTI_CLICK_DISTANCE: f64 = 4.0;

/// O clique em andamento e o rastreio de cliques múltiplos.
#[derive(Default)]
struct PointerState {
    position: (f64, f64),
    last_click: Option<(Instant, (f64, f64))>,
    click_count: u32,
}

struct App {
    /// O tamanho e a posição da janela na abertura (os da última janela do VS Code, sem argumentos).
    initial: shell::recent::LastWindow,
    shell: Shell,
    list: DisplayList,
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

/// A tecla do winit como `KeyCode` do VS Code: pela posição física (o `code` do DOM, que o `ScanCodeUtils` conhece)
/// e, para letras e dígitos, pelo caractere produzido (o `keyCode` do navegador segue o layout).
fn key_code_of(event: &winit::event::KeyEvent) -> KeyCode {
    if let Key::Character(text) = &event.logical_key
        && let Some(c) = text.chars().next().filter(char::is_ascii_alphanumeric)
    {
        return KeyCodeUtils::from_string(&c.to_ascii_uppercase().to_string());
    }
    match event.physical_key {
        PhysicalKey::Code(code) => immutable_code_to_key_code(ScanCodeUtils::to_enum(&format!("{code:?}"))).unwrap_or(KeyCode::Unknown),
        PhysicalKey::Unidentified(_) => KeyCode::Unknown,
    }
}

fn editor_key_of(event: &winit::event::KeyEvent, mods: Modifiers) -> Option<EditorKey> {
    let ctrl = mods.ctrl || mods.meta;
    Some(match &event.logical_key {
        Key::Named(NamedKey::ArrowLeft) => EditorKey::Left,
        Key::Named(NamedKey::ArrowRight) => EditorKey::Right,
        Key::Named(NamedKey::ArrowUp) => EditorKey::Up,
        Key::Named(NamedKey::ArrowDown) => EditorKey::Down,
        Key::Named(NamedKey::Home) => EditorKey::Home,
        Key::Named(NamedKey::End) => EditorKey::End,
        Key::Named(NamedKey::PageUp) => EditorKey::PageUp,
        Key::Named(NamedKey::PageDown) => EditorKey::PageDown,
        Key::Named(NamedKey::Backspace) => EditorKey::Backspace,
        Key::Named(NamedKey::Delete) => EditorKey::Delete,
        Key::Named(NamedKey::Enter) => EditorKey::Enter,
        Key::Named(NamedKey::Tab) => EditorKey::Tab,
        Key::Named(NamedKey::Escape) => EditorKey::Escape,
        Key::Character(s) if ctrl && !mods.alt => EditorKey::Char(s.chars().next()?.to_ascii_lowercase()),
        _ => return None,
    })
}

fn cursor_icon(cursor: Cursor) -> CursorIcon {
    match cursor {
        Cursor::Default => CursorIcon::Default,
        Cursor::Pointer => CursorIcon::Pointer,
        Cursor::Text => CursorIcon::Text,
        Cursor::ColResize => CursorIcon::ColResize,
        Cursor::RowResize => CursorIcon::RowResize,
        Cursor::EwResize => CursorIcon::EwResize,
        Cursor::NsResize => CursorIcon::NsResize,
        Cursor::WResize => CursorIcon::WResize,
        Cursor::EResize => CursorIcon::EResize,
        Cursor::NResize => CursorIcon::NResize,
        Cursor::SResize => CursorIcon::SResize,
        Cursor::NwseResize => CursorIcon::NwseResize,
        Cursor::NeswResize => CursorIcon::NeswResize,
        Cursor::AllScroll => CursorIcon::AllScroll,
    }
}

impl App {
    fn editor(&mut self) -> &mut je_editor_view::EditorView {
        &mut self.shell.active_editor_mut().expect("o bench abre um arquivo").view
    }

    /// Rola e digita com quadros de verdade (pintura e `present`) e imprime o tempo por quadro.
    fn run_bench(&mut self) {
        use je_editor::cursor_common::CursorSimpleModel;
        let lines = self.editor().model().get_line_count();
        let too_large = self.editor().model().is_too_large_for_tokenization();
        println!("arquivo: {lines} linhas, tokenização {}, pintura {}", if too_large { "desligada (arquivo grande)" } else { "ligada" }, self.backend_label);
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
        let step = 3.0 * self.editor().layout().line_height();
        let (mut frames, mut turns) = (Vec::new(), Vec::new());
        for _ in 0..600 {
            self.editor().scroll_by(0.0, step);
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
            self.editor().set_scroll_top(top);
            jumps.push(timed(self));
        }
        summarize("quadro após salto (cache frio)", jumps);
        // Digitação no meio do arquivo: a edição sozinha e o quadro seguinte.
        self.editor().set_cursor(je_editor::position::Position::new(lines / 2, 1));
        self.editor().reveal_cursor();
        timed(self);
        let (mut edits, mut after) = (Vec::new(), Vec::new());
        for _ in 0..300 {
            let started = Instant::now();
            self.editor().type_text("a");
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

    /// Pinta um quadro e devolve o tempo de montar a lista de exibição e o de `present` (rasterização e troca de
    /// buffer), em ms.
    fn frame_timed(&mut self) -> (f64, f64) {
        let (Some(window), Some(presenter)) = (&self.window, &mut self.presenter) else { return (0.0, 0.0) };
        // A escala da tela vezes o zoom da janela (`window.zoomLevel`, Ctrl+= e Ctrl+-).
        let scale = window.scale_factor() * self.shell.zoom_factor();
        let size = window.inner_size().to_logical::<f64>(scale);
        let started = Instant::now();
        self.shell.resize(size.width, size.height);
        self.list.clear();
        let background = self.shell.paint(&mut self.list, scale, started);
        let painted = Instant::now();
        if let Err(e) = presenter.present(&self.list, background) {
            eprintln!("je-code: frame descartado: {e}");
        }
        window.set_title(&self.shell.window_title());
        (painted.duration_since(started).as_secs_f64() * 1000.0, painted.elapsed().as_secs_f64() * 1000.0)
    }

    /// Roda o loop de eventos do runtime (tokenização em fatias, timers), aplica os pedidos do shell e pede quadro
    /// se algo mudou.
    fn tick(&mut self, event_loop: &ActiveEventLoop) {
        runtime::turn();
        if self.shell.take_exit() {
            event_loop.exit();
            return;
        }
        if let Some(w) = &self.window {
            if self.shell.take_fullscreen() {
                w.set_fullscreen(if w.fullscreen().is_some() { None } else { Some(Fullscreen::Borderless(None)) });
            }
            if self.shell.take_dirty() {
                w.request_redraw();
            }
        }
        let now = Instant::now();
        let mut deadline = self.shell.next_blink_deadline(now).unwrap_or(now + Duration::from_secs(3600));
        if let Some(wait) = runtime::time_to_next_timer() {
            deadline = deadline.min(now + wait);
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(deadline));
    }

    fn pointer_input(&mut self, kind: PointerInputKind, button: PointerButton) {
        let (x, y) = self.pointer.position;
        let mut input = PointerInput::new(kind, x, y);
        input.button = button;
        input.modifiers = self.modifiers;
        if kind == PointerInputKind::Down {
            let now = Instant::now();
            let repeated = self.pointer.last_click.is_some_and(|(at, (px, py))| now.duration_since(at) < MULTI_CLICK && (px - x).abs() < MULTI_CLICK_DISTANCE && (py - y).abs() < MULTI_CLICK_DISTANCE);
            self.pointer.click_count = if repeated { self.pointer.click_count % 3 + 1 } else { 1 };
            self.pointer.last_click = Some((now, (x, y)));
        }
        input.click_count = self.pointer.click_count.max(1);
        self.shell.pointer(input);
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let (width, height) = self.initial.bounds.map_or((1200.0, 800.0), |(w, h, _, _)| (w, h));
        let mut attrs = Window::default_attributes().with_title(self.shell.window_title()).with_inner_size(LogicalSize::new(width, height)).with_maximized(self.initial.maximized);
        if let Some((_, _, x, y)) = self.initial.bounds {
            attrs = attrs.with_position(LogicalPosition::new(x, y));
        }
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
                self.shell.set_backend(self.backend_label.clone());
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

    /// O pty do terminal avisa que há saída nova.
    fn user_event(&mut self, event_loop: &ActiveEventLoop, _: ()) {
        if let Some(w) = &self.window {
            w.request_redraw();
        }
        self.tick(event_loop);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                // Com abas modificadas a janela pergunta antes; sem elas, fecha já.
                self.shell.request_exit();
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
            }
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => {
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
            }
            WindowEvent::Focused(focused) => self.shell.set_window_focus(focused),
            WindowEvent::ModifiersChanged(m) => {
                let s = m.state();
                self.modifiers = Modifiers { alt: s.alt_key(), ctrl: s.control_key(), shift: s.shift_key(), meta: s.super_key() };
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let down = event.state == ElementState::Pressed;
                let key_code = key_code_of(&event);
                let editor_key = editor_key_of(&event, self.modifiers);
                self.shell.key(down, key_code, self.modifiers, event.text.as_deref(), editor_key);
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                let scale = self.window.as_ref().map_or(1.0, |w| w.scale_factor()) * self.shell.zoom_factor();
                let LogicalPosition { x, y } = position.to_logical::<f64>(scale);
                self.pointer.position = (x, y);
                self.pointer_input(PointerInputKind::Move, PointerButton::Primary);
                if let Some(w) = &self.window {
                    w.set_cursor(cursor_icon(self.shell.cursor_at(x, y)));
                }
            }
            WindowEvent::CursorLeft { .. } => self.shell.pointer_left(),
            WindowEvent::MouseInput { state, button, .. } => {
                let button = match button {
                    MouseButton::Left => PointerButton::Primary,
                    MouseButton::Middle => PointerButton::Auxiliary,
                    MouseButton::Right => PointerButton::Secondary,
                    _ => return,
                };
                let kind = if state == ElementState::Pressed { PointerInputKind::Down } else { PointerInputKind::Up };
                self.pointer_input(kind, button);
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let line_height = self.shell.active_editor().map_or(19.0, |e| e.view.layout().line_height());
                let (dx, dy) = match delta {
                    MouseScrollDelta::LineDelta(x, y) => (-f64::from(x) * 3.0 * line_height, -f64::from(y) * 3.0 * line_height),
                    MouseScrollDelta::PixelDelta(p) => (-p.x, -p.y),
                };
                let (x, y) = self.pointer.position;
                // Shift + roda rola na horizontal, como no VS Code.
                if self.modifiers.shift { self.shell.wheel(x, y, dy, 0.0) } else { self.shell.wheel(x, y, dx, dy) }
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
            }
            WindowEvent::RedrawRequested => {
                self.frame_timed();
            }
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
fn screenshot(shell: &mut Shell, out: &Path, width: u16, height: u16) -> Result<(), String> {
    use je_editor::cursor_common::CursorSimpleModel;
    // Tokeniza o arquivo inteiro antes de pintar: a captura precisa ser determinística.
    if let Some(editor) = shell.active_editor() {
        let model = editor.view.model().clone();
        model.tokenization().force_tokenization(model.get_line_count().min(2000));
    }
    runtime::turn();
    shell.set_backend("CPU".to_owned());
    shell.resize(f64::from(width), f64::from(height));
    let mut list = DisplayList::new();
    let background = shell.paint(&mut list, 1.0, Instant::now());
    let rgba = je_render::render_rgba(&list, background, width, height);
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
    // Como o `code`: pastas viram o workspace (a primeira), arquivos abrem em abas.
    let (mut workspace, mut files) = (None, Vec::new());
    for arg in args.map(PathBuf::from) {
        let absolute = std::path::absolute(&arg).unwrap_or(arg);
        if absolute.is_dir() {
            workspace.get_or_insert(absolute);
        } else {
            files.push(absolute);
        }
    }
    // O tema padrão vem da extensão embutida `theme-defaults`, como no VS Code; `JE_THEME` troca o arquivo.
    let theme_path = match std::env::var_os("JE_THEME") {
        Some(path) => PathBuf::from(path),
        None => match find_builtin_extensions() {
            Some(dir) => {
                // `workbench.colorTheme` do `settings.json` do usuário (o do VS Code), se houver; senão o padrão.
                let settings = shell::themes::user_settings();
                let themes = shell::themes::catalog(&dir);
                shell::themes::resolve(&themes, settings.get("workbench.colorTheme").and_then(|v| v.as_str())).map_or_else(|| dir.join(BUILTIN_DEFAULT_THEME), |i| themes[i].path.clone())
            }
            None => {
                eprintln!("je-code: pasta de extensões não encontrada (defina JE_BUILTIN_EXTENSIONS ou JE_THEME)");
                std::process::exit(1);
            }
        },
    };
    // Sem argumentos, como o VS Code: reabre a pasta e o tamanho da última janela (o `windowsState` dele).
    let last = if workspace.is_none() && files.is_empty() && screenshot_out.is_none() && !bench { shell::recent::last_window() } else { shell::recent::LastWindow::default() };
    if workspace.is_none() {
        workspace = last.folder.clone();
    }
    let mut shell = match Shell::open(&theme_path, workspace, &files, 1200.0, 800.0) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("je-code: {e}");
            std::process::exit(1);
        }
    };
    if let Some(out) = screenshot_out {
        if let Err(e) = screenshot(&mut shell, &out, 1200, 800) {
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
    let proxy = event_loop.create_proxy();
    shell.set_wake(Arc::new(move || {
        let _ = proxy.send_event(());
    }));
    let mut app = App { initial: last, shell, list: DisplayList::new(), window: None, presenter: None, backend_label: String::new(), modifiers: Modifiers::default(), pointer: PointerState::default(), bench, split: Vec::new() };
    if let Err(e) = event_loop.run_app(&mut app) {
        eprintln!("je-code: {e}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use je_render::Rgba;

    fn temp_file(name: &str, contents: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("je-code-{}-{name}", std::process::id()));
        std::fs::write(&path, contents).expect("arquivo temporário");
        path
    }

    #[test]
    fn save_writes_edited_text_and_keeps_eol() {
        let path = temp_file("save.txt", "ab\r\ncd\r\n");
        let mut shell = Shell::open(Path::new(DEFAULT_THEME), None, std::slice::from_ref(&path), 1200.0, 800.0).expect("abre");
        let editor = shell.active_editor_mut().expect("aba");
        assert!(!editor.is_modified());
        editor.view.set_cursor(je_editor::position::Position::new(1, 2));
        editor.view.type_text("X");
        assert!(editor.is_modified(), "digitar marca o arquivo como modificado");
        editor.view.undo();
        assert!(!editor.is_modified(), "desfazer até o texto salvo limpa a marca");
        editor.view.redo();
        shell.run(shell::Command::Save);
        assert!(!shell.active_editor().expect("aba").is_modified());
        assert_eq!(std::fs::read_to_string(&path).expect("lê"), "aXb\r\ncd\r\n");
        std::fs::remove_file(&path).expect("remove");
    }

    #[test]
    fn frame_paints_chrome_and_text() {
        let path = temp_file("frame.ts", "const answer = 42;\n");
        let mut shell = Shell::open(Path::new(DEFAULT_THEME), None, std::slice::from_ref(&path), 1200.0, 800.0).expect("abre");
        let mut list = DisplayList::new();
        let background = shell.paint(&mut list, 1.0, Instant::now());
        let rgba = je_render::render_rgba(&list, background, 1200, 800);
        let pixel = |x: usize, y: usize| [rgba[(y * 1200 + x) * 4], rgba[(y * 1200 + x) * 4 + 1], rgba[(y * 1200 + x) * 4 + 2]];
        let editor_bg = Rgba::opaque(0x12, 0x13, 0x14);
        assert_eq!(pixel(900, 400), [editor_bg.r, editor_bg.g, editor_bg.b], "o editor tem o fundo do tema");
        // O texto do editor começa abaixo das abas e dos breadcrumbs.
        let text_region_has_ink = (90..112).any(|y| (120..300).any(|x| pixel(x, y) != pixel(900, 400)));
        assert!(text_region_has_ink, "a primeira linha foi pintada");
        assert_ne!(pixel(600, 790), pixel(600, 400), "a barra de status tem fundo próprio");
        std::fs::remove_file(&path).expect("remove");
    }

    fn click(shell: &mut Shell, x: f64, y: f64) {
        for kind in [PointerInputKind::Move, PointerInputKind::Down, PointerInputKind::Up] {
            shell.pointer(PointerInput::new(kind, x, y));
        }
    }

    fn snapshot(shell: &mut Shell, name: &str) -> Vec<u8> {
        let mut list = DisplayList::new();
        let background = shell.paint(&mut list, 1.0, Instant::now());
        let rgba = je_render::render_rgba(&list, background, 1200, 800);
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../wip/scratch/shell");
        std::fs::create_dir_all(&dir).expect("pasta");
        let file = std::fs::File::create(dir.join(name)).expect("png");
        let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), 1200, 800);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.write_header().and_then(|mut w| w.write_image_data(&rgba)).expect("png");
        rgba
    }

    #[test]
    fn explorer_and_menus_respond_to_the_pointer() {
        let dir = std::env::temp_dir().join(format!("je-code-{}-ws", std::process::id()));
        std::fs::create_dir_all(dir.join("src")).expect("pasta");
        std::fs::write(dir.join("a.txt"), "alpha\n").expect("a");
        std::fs::write(dir.join("src/b.rs"), "fn main() {}\n").expect("b");
        let mut shell = Shell::open(Path::new(DEFAULT_THEME), Some(dir.clone()), &[], 1200.0, 800.0).expect("abre");
        snapshot(&mut shell, "0-empty.png");
        assert!(shell.active_editor().is_none(), "só a pasta: nenhum editor");
        // A segunda linha do explorador (a.txt; a primeira é a pasta src) abre o arquivo.
        click(&mut shell, 120.0, 96.0 + 22.0 + 11.0);
        snapshot(&mut shell, "1-open.png");
        assert_eq!(shell.active_editor().and_then(|e| e.path.clone()), Some(dir.join("a.txt")));
        // O clique único abriu em pré-visualização: fixar a aba mantém a.txt quando o próximo abrir.
        shell.run(shell::Command::PinEditor(0));
        // A pasta src abre e mostra b.rs, que abre numa segunda aba.
        click(&mut shell, 120.0, 96.0 + 11.0);
        snapshot(&mut shell, "2-folder.png");
        click(&mut shell, 120.0, 96.0 + 22.0 + 11.0);
        snapshot(&mut shell, "3-second.png");
        assert_eq!(shell.active_editor().and_then(|e| e.path.clone()), Some(dir.join("src/b.rs")));
        assert_eq!(shell.explorer_selection(), vec![dir.join("src/b.rs")], "o explorador seleciona o arquivo do editor ativo");
        // Trocar de aba pela faixa leva a seleção do explorador junto (o `explorer.autoReveal`).
        shell.run(shell::Command::ActivateEditor(0));
        assert_eq!(shell.explorer_selection(), vec![dir.join("a.txt")]);
        // O menu File abre com o clique e "Close Editor" fecha a aba ativa.
        click(&mut shell, 54.0, 17.0);
        snapshot(&mut shell, "4-menu.png");
        std::fs::remove_dir_all(&dir).expect("remove");
    }

    #[test]
    fn quick_open_and_palette_pick_by_fuzzy_score() {
        let dir = std::env::temp_dir().join(format!("je-code-{}-qo", std::process::id()));
        std::fs::create_dir_all(dir.join("src")).expect("pasta");
        for (name, text) in [("sample.ts", "x"), ("notes.txt", "n"), ("package.json", "{}"), ("src/index.ts", "i")] {
            std::fs::write(dir.join(name), text).expect("arquivo");
        }
        let mut shell = Shell::open(Path::new(DEFAULT_THEME), Some(dir.clone()), &[dir.join("sample.ts")], 1200.0, 800.0).expect("abre");
        let key = |shell: &mut Shell, key: Option<EditorKey>, text: Option<&str>| shell.key(true, KeyCode::Unknown, Modifiers::default(), text, key);
        shell.run(shell::Command::QuickOpen);
        snapshot(&mut shell, "q0-empty.png");
        key(&mut shell, None, Some("s"));
        snapshot(&mut shell, "q1-s.png");
        // `sample.ts` (aberto) vem primeiro como `recently opened`; Enter mantém o arquivo.
        key(&mut shell, Some(EditorKey::Enter), None);
        assert_eq!(shell.active_editor().and_then(|e| e.path.clone()), Some(dir.join("sample.ts")));
        // A busca por `idx` acha `src/index.ts`, que abre numa segunda aba.
        shell.run(shell::Command::QuickOpen);
        for c in ["i", "d", "x"] {
            key(&mut shell, None, Some(c));
        }
        key(&mut shell, Some(EditorKey::Enter), None);
        assert_eq!(shell.active_editor().and_then(|e| e.path.clone()), Some(dir.join("src/index.ts")));
        // A paleta lista os comandos do menu e roda o escolhido.
        shell.run(shell::Command::ShowCommands);
        snapshot(&mut shell, "q2-palette.png");
        for c in ["c", "l", "o", "s", "e"] {
            key(&mut shell, None, Some(c));
        }
        snapshot(&mut shell, "q3-palette-close.png");
        key(&mut shell, Some(EditorKey::Enter), None);
        assert_eq!(shell.active_editor().and_then(|e| e.path.clone()), Some(dir.join("sample.ts")), "File: Close Editor fecha a aba ativa");
        // Escape fecha sem rodar nada.
        shell.run(shell::Command::QuickOpen);
        key(&mut shell, Some(EditorKey::Escape), None);
        key(&mut shell, None, Some("z"));
        assert_eq!(shell.active_editor().expect("aba").view.model().get_value_length(), 2, "com o painel fechado a tecla volta ao editor");
        std::fs::remove_dir_all(&dir).expect("remove");
    }

    #[test]
    fn find_widget_selects_and_steps_through_matches() {
        use je_editor::cursor_common::CursorSimpleModel;
        let path = temp_file("find.ts", "const a = 1;\nconst b = a + a;\nlet C = 2;\n");
        let mut shell = Shell::open(Path::new(DEFAULT_THEME), None, std::slice::from_ref(&path), 1200.0, 800.0).expect("abre");
        let key = |shell: &mut Shell, key: Option<EditorKey>, text: Option<&str>| shell.key(true, KeyCode::Unknown, Modifiers::default(), text, key);
        let selection = |shell: &Shell| {
            let r = shell.active_editor().expect("aba").view.cursor_selection_range();
            (r.start_line_number, r.start_column, r.end_line_number, r.end_column)
        };
        shell.run(shell::Command::Find);
        for c in ["c", "o"] {
            key(&mut shell, None, Some(c));
        }
        snapshot(&mut shell, "f0-co.png");
        assert_eq!(selection(&shell), (1, 1, 1, 3), "a primeira ocorrência a partir do cursor");
        key(&mut shell, Some(EditorKey::Enter), None);
        assert_eq!(selection(&shell), (2, 1, 2, 3), "Enter vai para a próxima");
        key(&mut shell, Some(EditorKey::Enter), None);
        assert_eq!(selection(&shell), (1, 1, 1, 3), "e dá a volta no documento");
        shell.run(shell::Command::FindPrevious);
        assert_eq!(selection(&shell), (2, 1, 2, 3), "Shift+Enter volta");
        // Maiúsculas: `C` sem a chave acha `const` duas vezes e `C` de `let C`; com a chave, só o último.
        shell.run(shell::Command::FindClose);
        shell.run(shell::Command::Find);
        for _ in 0..2 {
            key(&mut shell, Some(EditorKey::Backspace), None);
        }
        key(&mut shell, None, Some("C"));
        snapshot(&mut shell, "f1-c.png");
        shell.run(shell::Command::FindToggleCase);
        assert_eq!(shell.active_editor().expect("aba").view.model().find_matches("C", je_editor::text_model::FindScope::Whole, false, true, None, false, 100).len(), 1);
        key(&mut shell, Some(EditorKey::Escape), None);
        key(&mut shell, None, Some("z"));
        assert_eq!(shell.active_editor().expect("aba").view.model().get_line_content(3).to_string_lossy().chars().filter(|c| *c == 'z').count(), 1, "Escape fecha a busca e devolve o teclado ao editor");
        std::fs::remove_file(&path).expect("remove");
    }

    #[cfg(unix)]
    #[test]
    fn terminal_panel_runs_a_shell_and_takes_the_keyboard() {
        let dir = std::env::temp_dir().join(format!("je-code-{}-term", std::process::id()));
        std::fs::create_dir_all(&dir).expect("pasta");
        std::fs::write(dir.join("a.txt"), "abc").expect("arquivo");
        let mut shell = Shell::open(Path::new(DEFAULT_THEME), Some(dir.clone()), &[dir.join("a.txt")], 1200.0, 800.0).expect("abre");
        shell.set_wake(Arc::new(|| {}));
        shell.run(shell::Command::TogglePanel);
        let key = |shell: &mut Shell, key: Option<EditorKey>, text: Option<&str>| shell.key(true, KeyCode::Unknown, Modifiers::default(), text, key);
        // O teclado vai ao terminal: digita um comando e dá Enter.
        for c in "echo ola-terminal".chars() {
            key(&mut shell, None, Some(&c.to_string()));
        }
        key(&mut shell, Some(EditorKey::Enter), None);
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        loop {
            snapshot(&mut shell, "t0-terminal.png");
            if shell.terminal_text().matches("ola-terminal").count() >= 2 || std::time::Instant::now() > deadline {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        assert!(shell.terminal_text().matches("ola-terminal").count() >= 2, "o comando e a saída dele estão na grade: {}", shell.terminal_text());
        assert_eq!(shell.active_editor().expect("aba").view.model().get_value_length(), 3, "o texto do editor não mudou");
        // Ctrl+J fecha o painel e devolve o teclado ao editor.
        shell.run(shell::Command::TogglePanel);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn theme_picker_previews_applies_and_reverts() {
        let path = temp_file("theme.ts", "const answer = 42;\n");
        let mut shell = Shell::open(Path::new(DEFAULT_THEME), None, std::slice::from_ref(&path), 1200.0, 800.0).expect("abre");
        let key = |shell: &mut Shell, key: Option<EditorKey>, text: Option<&str>| shell.key(true, KeyCode::Unknown, Modifiers::default(), text, key);
        // O cursor pisca com o relógio: o teste o apaga das imagens.
        let blank_cursor = |mut rgba: Vec<u8>| {
            for y in 88..112 {
                for x in 120..130 {
                    rgba[(y * 1200 + x) * 4..(y * 1200 + x) * 4 + 3].fill(0);
                }
            }
            rgba
        };
        let dark = blank_cursor(snapshot(&mut shell, "th0-dark.png"));
        shell.run(shell::Command::PickTheme);
        for c in "light modern".chars() {
            key(&mut shell, None, Some(&c.to_string()));
        }
        // Pré-visualiza o tema em foco sem aplicar de vez: o fundo do editor já é o do Light Modern.
        let preview = snapshot(&mut shell, "th1-preview.png");
        let at = (400 * 1200 + 900) * 4;
        assert_eq!(&preview[at..at + 3], &[0xff, 0xff, 0xff], "o tema em foco já aparece na janela");
        key(&mut shell, Some(EditorKey::Escape), None);
        assert!(blank_cursor(snapshot(&mut shell, "th2-reverted.png")) == dark, "Esc volta ao tema de antes");
        shell.run(shell::Command::PickTheme);
        for c in "light modern".chars() {
            key(&mut shell, None, Some(&c.to_string()));
        }
        key(&mut shell, Some(EditorKey::Enter), None);
        let applied = blank_cursor(snapshot(&mut shell, "th3-applied.png"));
        assert!(applied != dark, "Enter aplica o tema escolhido");
        assert_eq!(&applied[at..at + 3], &[0xff, 0xff, 0xff], "o fundo do editor é o do Light Modern");
        std::fs::remove_file(&path).expect("remove");
    }

    #[test]
    fn explorer_context_menu_renames_and_creates() {
        let dir = std::env::temp_dir().join(format!("je-code-{}-ctx", std::process::id()));
        std::fs::create_dir_all(&dir).expect("pasta");
        std::fs::write(dir.join("a.txt"), "alpha").expect("a");
        let mut shell = Shell::open(Path::new(DEFAULT_THEME), Some(dir.clone()), &[], 1200.0, 800.0).expect("abre");
        let key = |shell: &mut Shell, key: Option<EditorKey>, code: KeyCode, text: Option<&str>| shell.key(true, code, Modifiers::default(), text, key);
        let right_click = |shell: &mut Shell, x: f64, y: f64| {
            for kind in [PointerInputKind::Move, PointerInputKind::Down, PointerInputKind::Up] {
                let mut input = PointerInput::new(kind, x, y);
                input.button = PointerButton::Secondary;
                shell.pointer(input);
            }
        };
        snapshot(&mut shell, "c0-tree.png");
        // Botão direito na linha de a.txt abre o menu de contexto; Esc o fecha sem abrir o arquivo.
        right_click(&mut shell, 120.0, 96.0 + 11.0);
        snapshot(&mut shell, "c1-menu.png");
        assert!(shell.has_context_menu(), "o menu de contexto abriu");
        assert!(shell.active_editor().is_none(), "o botão direito não abre o arquivo");
        key(&mut shell, Some(EditorKey::Escape), KeyCode::Escape, None);
        assert!(!shell.has_context_menu(), "Esc fecha o menu");
        // Renomear: a quinta opção (Open Containing Folder já vem em foco; mais quatro Down).
        right_click(&mut shell, 120.0, 96.0 + 11.0);
        for _ in 0..4 {
            key(&mut shell, Some(EditorKey::Down), KeyCode::DownArrow, None);
        }
        key(&mut shell, Some(EditorKey::Enter), KeyCode::Enter, None);
        assert!(!shell.has_context_menu(), "o item rodou e o menu fechou");
        snapshot(&mut shell, "c2-prompt.png");
        for _ in 0..5 {
            key(&mut shell, Some(EditorKey::Backspace), KeyCode::Backspace, None);
        }
        for c in "b.txt".chars() {
            key(&mut shell, None, KeyCode::Unknown, Some(&c.to_string()));
        }
        key(&mut shell, Some(EditorKey::Enter), KeyCode::Enter, None);
        assert!(dir.join("b.txt").is_file() && !dir.join("a.txt").exists(), "o arquivo foi renomeado");
        snapshot(&mut shell, "c3-renamed.png");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn open_folder_and_save_as_prompts_work() {
        let dir = std::env::temp_dir().join(format!("je-code-{}-open", std::process::id()));
        std::fs::create_dir_all(dir.join("sub")).expect("pasta");
        std::fs::write(dir.join("sub/x.ts"), "export {}").expect("x");
        let mut shell = Shell::open(Path::new(DEFAULT_THEME), None, &[], 1200.0, 800.0).expect("abre");
        let key = |shell: &mut Shell, key: Option<EditorKey>, text: Option<&str>| shell.key(true, KeyCode::Unknown, Modifiers::default(), text, key);
        let type_text = |shell: &mut Shell, text: &str| {
            for c in text.chars() {
                key(shell, None, Some(&c.to_string()));
            }
        };
        // Open Folder: o explorador passa a mostrar a pasta digitada.
        shell.run(shell::Command::PromptOpenFolder);
        type_text(&mut shell, &dir.display().to_string());
        key(&mut shell, Some(EditorKey::Enter), None);
        assert_eq!(shell.workspace_path(), Some(dir.clone()));
        snapshot(&mut shell, "o0-folder.png");
        // Open File: relativo à pasta aberta.
        shell.run(shell::Command::PromptOpenFile);
        for _ in 0..dir.display().to_string().len() + 1 {
            key(&mut shell, Some(EditorKey::Backspace), None);
        }
        type_text(&mut shell, "sub/x.ts");
        key(&mut shell, Some(EditorKey::Enter), None);
        assert_eq!(shell.active_editor().and_then(|e| e.path.clone()), Some(dir.join("sub/x.ts")));
        // Save As de um arquivo novo: o texto vai para o disco e a aba segue o caminho.
        shell.run(shell::Command::NewUntitled);
        shell.active_editor_mut().expect("aba").view.type_text("hello");
        shell.run(shell::Command::Save);
        for _ in 0..200 {
            key(&mut shell, Some(EditorKey::Backspace), None);
        }
        type_text(&mut shell, &dir.join("novo.txt").display().to_string());
        key(&mut shell, Some(EditorKey::Enter), None);
        assert_eq!(std::fs::read_to_string(dir.join("novo.txt")).expect("gravou"), "hello");
        assert_eq!(shell.active_editor().and_then(|e| e.path.clone()), Some(dir.join("novo.txt")));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn go_to_line_prompt_moves_the_cursor() {
        let path = temp_file("goto.txt", "um\ndois\ntres\nquatro\n");
        let mut shell = Shell::open(Path::new(DEFAULT_THEME), None, std::slice::from_ref(&path), 1200.0, 800.0).expect("abre");
        let key = |shell: &mut Shell, key: Option<EditorKey>, text: Option<&str>| shell.key(true, KeyCode::Unknown, Modifiers::default(), text, key);
        shell.run(shell::Command::PromptGoToLine);
        for c in "3:2".chars() {
            key(&mut shell, None, Some(&c.to_string()));
        }
        key(&mut shell, Some(EditorKey::Enter), None);
        let p = shell.active_editor().expect("aba").view.cursor_position();
        assert_eq!((p.line_number, p.column), (3, 2));
        std::fs::remove_file(&path).expect("remove");
    }

    #[test]
    fn status_bar_pickers_change_language_and_eol() {
        use je_editor::cursor_common::CursorSimpleModel;
        let path = temp_file("mode.txt", "fn main() {}\n");
        let mut shell = Shell::open(Path::new(DEFAULT_THEME), None, std::slice::from_ref(&path), 1200.0, 800.0).expect("abre");
        let key = |shell: &mut Shell, key: Option<EditorKey>, text: Option<&str>| shell.key(true, KeyCode::Unknown, Modifiers::default(), text, key);
        shell.run(shell::Command::PickLanguage);
        for c in "rust".chars() {
            key(&mut shell, None, Some(&c.to_string()));
        }
        key(&mut shell, Some(EditorKey::Enter), None);
        let editor = shell.active_editor().expect("aba");
        assert_eq!(editor.view.model().get_language_id(), "rust");
        assert_eq!(editor.language_name, "Rust");
        // Indentação pela barra de status: tabulações de tamanho 2.
        shell.run(shell::Command::PickIndentation);
        key(&mut shell, Some(EditorKey::Down), None);
        key(&mut shell, Some(EditorKey::Enter), None);
        for _ in 0..2 {
            key(&mut shell, Some(EditorKey::Up), None);
        }
        key(&mut shell, Some(EditorKey::Enter), None);
        let options = shell.active_editor().expect("aba").view.model().get_options();
        assert_eq!((options.insert_spaces, options.tab_size), (false, 2));
        shell.run(shell::Command::PickEol);
        key(&mut shell, Some(EditorKey::Down), None);
        key(&mut shell, Some(EditorKey::Enter), None);
        assert_eq!(shell.active_editor().expect("aba").view.model().get_eol(), je_editor::model::Eol::CrLf);
        std::fs::remove_file(&path).expect("remove");
    }

    #[test]
    fn search_view_finds_in_files_and_opens_the_match() {
        let dir = std::env::temp_dir().join(format!("je-code-{}-sv", std::process::id()));
        std::fs::create_dir_all(dir.join("src")).expect("pasta");
        std::fs::write(dir.join("a.ts"), "export class Counter {\n  private count = 0;\n}\n").expect("a");
        std::fs::write(dir.join("src/b.ts"), "this.count += step;\nreturn this.count;\n").expect("b");
        let mut shell = Shell::open(Path::new(DEFAULT_THEME), Some(dir.clone()), &[], 1200.0, 800.0).expect("abre");
        let key = |shell: &mut Shell, key: Option<EditorKey>, text: Option<&str>| shell.key(true, KeyCode::Unknown, Modifiers::default(), text, key);
        shell.run(shell::Command::ShowSearch);
        for c in "count".chars() {
            key(&mut shell, None, Some(&c.to_string()));
        }
        key(&mut shell, Some(EditorKey::Enter), None);
        // A busca roda numa thread: espera os resultados chegarem.
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        loop {
            shell.take_dirty();
            snapshot(&mut shell, "s0-search.png");
            if shell.search_message().is_some_and(|m| m.starts_with("4 results in 2 files")) || std::time::Instant::now() > deadline {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        assert_eq!(shell.search_message().as_deref(), Some("4 results in 2 files"));
        // A primeira linha de resultado (depois da linha do arquivo a.ts) abre o arquivo na linha 1.
        click(&mut shell, 120.0, 35.0 + 105.0 + 22.0 + 11.0);
        snapshot(&mut shell, "s1-open.png");
        let editor = shell.active_editor().expect("abriu");
        assert_eq!(editor.path.as_deref(), Some(dir.join("a.ts").as_path()));
        let r = editor.view.cursor_selection_range();
        assert_eq!((r.start_line_number, r.start_column, r.end_column), (1, 14, 19), "a ocorrência fica selecionada");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn failed_operations_show_a_toast_that_can_be_closed() {
        let dir = std::env::temp_dir().join(format!("je-code-{}-toast", std::process::id()));
        std::fs::create_dir_all(&dir).expect("pasta");
        std::fs::write(dir.join("a.txt"), "a").expect("a");
        let mut shell = Shell::open(Path::new(DEFAULT_THEME), Some(dir.clone()), &[], 1200.0, 800.0).expect("abre");
        let key = |shell: &mut Shell, key: Option<EditorKey>, text: Option<&str>| shell.key(true, KeyCode::Unknown, Modifiers::default(), text, key);
        // Criar um arquivo que já existe falha com uma notificação de erro.
        shell.run(shell::Command::PromptNewFile(dir.clone()));
        for c in "a.txt".chars() {
            key(&mut shell, None, Some(&c.to_string()));
        }
        key(&mut shell, Some(EditorKey::Enter), None);
        assert_eq!(shell.toast_count(), 1);
        snapshot(&mut shell, "n0-toast.png");
        // O botão de fechar (canto superior direito do aviso) o dispensa.
        let (width, height) = (1200.0, 800.0);
        let toast_top = height - 28.0 - 8.0 - (14.0 + 22.0 * 2.0 + 14.0);
        click(&mut shell, width - 8.0 - 21.0, toast_top + 23.0);
        assert_eq!(shell.toast_count(), 0);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn source_control_view_stages_and_commits() {
        let dir = std::env::temp_dir().join(format!("je-code-{}-scm", std::process::id()));
        std::fs::create_dir_all(&dir).expect("pasta");
        let run_git = |args: &[&str]| {
            let out = std::process::Command::new("git").arg("-C").arg(&dir).args(args).output().expect("git existe");
            assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        };
        run_git(&["init", "-q", "-b", "main"]);
        run_git(&["config", "user.email", "a@b.c"]);
        run_git(&["config", "user.name", "teste"]);
        std::fs::write(dir.join("a.txt"), "um\n").expect("a");
        run_git(&["add", "."]);
        run_git(&["commit", "-q", "-m", "first"]);
        std::fs::write(dir.join("a.txt"), "um\ndois\n").expect("edita");
        std::fs::write(dir.join("b.txt"), "novo\n").expect("novo");
        let mut shell = Shell::open(Path::new(DEFAULT_THEME), Some(dir.clone()), &[], 1200.0, 800.0).expect("abre");
        shell.set_wake(Arc::new(|| {}));
        let wait = |shell: &mut Shell, ready: &dyn Fn(&shell::git::Status) -> bool| {
            let deadline = std::time::Instant::now() + Duration::from_secs(10);
            loop {
                shell.take_dirty();
                if ready(&shell.git_status()) || std::time::Instant::now() > deadline {
                    break;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        };
        shell.run(shell::Command::ShowScm);
        wait(&mut shell, &|s| s.count() == 2);
        snapshot(&mut shell, "g0-scm.png");
        let status = shell.git_status();
        assert_eq!((status.branch.as_str(), status.staged.len(), status.changes.len()), ("main", 0, 2));
        // O diff contra o HEAD aparece na margem do arquivo aberto.
        shell.run(shell::Command::ScmOpen("a.txt".to_owned()));
        snapshot(&mut shell, "g00-gutter.png");
        // A Timeline do arquivo aberto mostra o commit dele.
        shell.run(shell::Command::ShowExplorer);
        shell.run(shell::Command::ToggleTimeline);
        assert_eq!(shell.timeline_subjects(), vec!["first".to_owned()]);
        snapshot(&mut shell, "g01-timeline.png");
        shell.run(shell::Command::ToggleTimeline);
        shell.run(shell::Command::ShowScm);
        // Stage de tudo, mensagem e commit pela Ctrl+Enter.
        shell.run(shell::Command::ScmOp(shell::git::Operation::Stage(vec!["a.txt".to_owned(), "b.txt".to_owned()])));
        wait(&mut shell, &|s| s.staged.len() == 2);
        snapshot(&mut shell, "g1-staged.png");
        let key = |shell: &mut Shell, key: Option<EditorKey>, mods: Modifiers, text: Option<&str>| shell.key(true, KeyCode::Unknown, mods, text, key);
        shell.run(shell::Command::ScmFocus);
        for c in "segundo commit".chars() {
            key(&mut shell, None, Modifiers::default(), Some(&c.to_string()));
        }
        key(&mut shell, Some(EditorKey::Enter), Modifiers { ctrl: true, ..Modifiers::default() }, None);
        wait(&mut shell, &|s| s.count() == 0);
        assert_eq!(shell.git_status().count(), 0, "o commit levou os dois arquivos");
        // Um remoto local: o commit novo fica 1 à frente; o Sync empurra e zera.
        let remote = dir.with_extension("remote");
        let out = std::process::Command::new("git").args(["init", "-q", "--bare"]).arg(&remote).output().expect("bare");
        assert!(out.status.success());
        run_git(&["remote", "add", "origin", &remote.to_string_lossy()]);
        run_git(&["push", "-q", "-u", "origin", "main"]);
        std::fs::write(dir.join("c.txt"), "c\n").expect("c");
        run_git(&["add", "c.txt"]);
        run_git(&["commit", "-q", "-m", "terceiro"]);
        shell.run(shell::Command::ScmRefresh);
        wait(&mut shell, &|s| s.ahead == 1);
        assert!(shell.git_status().upstream);
        snapshot(&mut shell, "g2-ahead.png");
        shell.run(shell::Command::ScmOp(shell::git::Operation::Sync));
        wait(&mut shell, &|s| s.ahead == 0);
        assert_eq!(shell.git_status().ahead, 0, "o Sync empurrou o commit");
        std::fs::remove_dir_all(&remote).ok();
        let log = std::process::Command::new("git").arg("-C").arg(&dir).args(["log", "--oneline"]).output().expect("log");
        assert!(String::from_utf8_lossy(&log.stdout).contains("segundo commit"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn manage_menu_and_layout_buttons_respond() {
        let mut shell = Shell::open(Path::new(DEFAULT_THEME), None, &[], 1200.0, 800.0).expect("abre");
        // A engrenagem da activity bar abre o menu Manage, com o clique primário.
        click(&mut shell, 26.0, 742.0);
        assert!(shell.has_context_menu());
        snapshot(&mut shell, "m0-manage.png");
        shell.key(true, KeyCode::Escape, Modifiers::default(), None, Some(EditorKey::Escape));
        assert!(!shell.has_context_menu());
        // O botão de layout do painel (terceiro à direita) mostra e esconde o painel.
        let toggle = |shell: &mut Shell| click(shell, 1063.0, 17.0);
        toggle(&mut shell);
        assert!(shell.panel_is_visible());
        toggle(&mut shell);
        assert!(!shell.panel_is_visible());
        // Problems mostra a aba com a mensagem de vazio, sem abrir terminal.
        shell.run(shell::Command::ShowPanelTab(shell::PanelTab::Problems));
        assert!(shell.panel_is_visible());
        snapshot(&mut shell, "m1-problems.png");
    }

    #[test]
    fn closing_a_modified_tab_asks_to_save() {
        let path = temp_file("dirty.txt", "abc");
        let mut shell = Shell::open(Path::new(DEFAULT_THEME), None, std::slice::from_ref(&path), 1200.0, 800.0).expect("abre");
        // Tecla apertada e solta: o diálogo fecha com o Esc no `keyup`, como o original.
        let key = |shell: &mut Shell, key: Option<EditorKey>, code: KeyCode| {
            shell.key(true, code, Modifiers::default(), None, key);
            shell.key(false, code, Modifiers::default(), None, key);
        };
        shell.active_editor_mut().expect("aba").view.type_text("X");
        shell.run(shell::Command::CloseActiveEditor);
        snapshot(&mut shell, "d0-ask.png");
        assert!(shell.active_editor().is_some(), "a aba modificada não fecha sem perguntar");
        // Esc cancela; a aba e a edição continuam.
        key(&mut shell, Some(EditorKey::Escape), KeyCode::Escape);
        assert!(shell.active_editor().expect("aba").is_modified());
        // Save (o botão em foco) grava e fecha.
        shell.run(shell::Command::CloseActiveEditor);
        key(&mut shell, Some(EditorKey::Enter), KeyCode::Enter);
        assert!(shell.active_editor().is_none());
        assert_eq!(std::fs::read_to_string(&path).expect("lê"), "Xabc");
        // Sair com uma aba modificada também pergunta; Don't Save fecha e sai.
        let mut shell = Shell::open(Path::new(DEFAULT_THEME), None, std::slice::from_ref(&path), 1200.0, 800.0).expect("abre");
        shell.active_editor_mut().expect("aba").view.type_text("Y");
        shell.request_exit();
        assert!(!shell.take_exit(), "não sai antes da resposta");
        // Tab vai ao segundo botão (Don't Save) e Enter o aciona.
        key(&mut shell, Some(EditorKey::Tab), KeyCode::Tab);
        key(&mut shell, Some(EditorKey::Enter), KeyCode::Enter);
        assert!(shell.take_exit(), "Don't Save sai");
        assert_eq!(std::fs::read_to_string(&path).expect("lê"), "Xabc", "o texto descartado não foi gravado");
        std::fs::remove_file(&path).expect("remove");
    }

    #[test]
    fn tab_context_menu_closes_others_and_to_the_right() {
        let files: Vec<PathBuf> = ["t1.txt", "t2.txt", "t3.txt", "t4.txt"].iter().map(|n| temp_file(n, n)).collect();
        let mut shell = Shell::open(Path::new(DEFAULT_THEME), None, &files, 1200.0, 800.0).expect("abre");
        let names = |shell: &Shell| shell.tab_paths();
        assert_eq!(names(&shell).len(), 4);
        // Botão direito na segunda aba abre o menu.
        let mut input = PointerInput::new(PointerInputKind::Down, 200.0, 51.0);
        input.button = PointerButton::Secondary;
        shell.pointer(PointerInput::new(PointerInputKind::Move, 200.0, 51.0));
        shell.pointer(input);
        input.kind = PointerInputKind::Up;
        shell.pointer(input);
        assert!(shell.has_context_menu());
        snapshot(&mut shell, "tab0-menu.png");
        shell.key(true, KeyCode::Escape, Modifiers::default(), None, Some(EditorKey::Escape));
        shell.run(shell::Command::CloseToTheRight(1));
        assert_eq!(names(&shell), files[..2].to_vec());
        shell.run(shell::Command::CloseOthers(0));
        assert_eq!(names(&shell), files[..1].to_vec());
        for f in files {
            std::fs::remove_file(f).ok();
        }
    }

    #[test]
    fn external_changes_reload_clean_files_and_refresh_the_explorer() {
        use je_editor::cursor_common::CursorSimpleModel;
        let dir = std::env::temp_dir().join(format!("je-code-{}-watch", std::process::id()));
        std::fs::create_dir_all(&dir).expect("pasta");
        std::fs::write(dir.join("a.txt"), "antes").expect("a");
        let mut shell = Shell::open(Path::new(DEFAULT_THEME), Some(dir.clone()), &[dir.join("a.txt")], 1200.0, 800.0).expect("abre");
        // Outro programa grava o arquivo (com a data mudada) e cria um novo na pasta.
        std::thread::sleep(Duration::from_millis(20));
        std::fs::write(dir.join("a.txt"), "depois").expect("muda");
        std::fs::write(dir.join("b.txt"), "novo").expect("novo");
        shell.run(shell::Command::CheckDisk);
        let editor = shell.active_editor().expect("aba");
        assert_eq!(editor.view.model().get_line_content(1).to_string_lossy(), "depois", "o arquivo limpo foi relido");
        assert!(!editor.is_modified(), "o recarregar não deixa a aba suja");
        snapshot(&mut shell, "w0-reloaded.png");
        assert!(shell.explorer_lists(&dir.join("b.txt")), "o explorador mostra o arquivo novo");
        // Com edição pendente, a mudança de fora não sobrescreve.
        shell.active_editor_mut().expect("aba").view.type_text("X");
        std::thread::sleep(Duration::from_millis(20));
        std::fs::write(dir.join("a.txt"), "de novo").expect("muda");
        shell.run(shell::Command::CheckDisk);
        assert!(shell.active_editor().expect("aba").view.model().get_line_content(1).to_string_lossy().starts_with('X'));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn extensions_view_lists_and_filters_installed_extensions() {
        let mut shell = Shell::open(Path::new(DEFAULT_THEME), None, &[], 1200.0, 800.0).expect("abre");
        shell.run(shell::Command::ShowExtensions);
        snapshot(&mut shell, "x0-extensions.png");
        for c in "rust".chars() {
            shell.key(true, KeyCode::Unknown, Modifiers::default(), Some(&c.to_string()), None);
        }
        snapshot(&mut shell, "x1-filtered.png");
    }

    #[test]
    fn activity_bar_shows_a_hover_after_the_delay() {
        je_base::runtime::with_faked_timers(|| {
            let mut shell = Shell::open(Path::new(DEFAULT_THEME), None, &[], 1200.0, 800.0).expect("abre");
            snapshot(&mut shell, "h-before.png");
            shell.pointer(PointerInput::new(PointerInputKind::Move, 26.0, 100.0));
            je_base::runtime::advance_fake_time(600);
            shell.take_dirty();
            assert!(shell.has_hover(), "a dica aparece depois de 500 ms");
            snapshot(&mut shell, "h0-hover.png");
            shell.pointer(PointerInput::new(PointerInputKind::Move, 600.0, 400.0));
            shell.take_dirty();
            assert!(!shell.has_hover(), "e some ao sair");
        });
    }

    #[test]
    fn auto_save_after_delay_writes_the_file() {
        let path = temp_file("auto.txt", "abc");
        je_base::runtime::with_faked_timers(|| {
            let mut shell = Shell::open(Path::new(DEFAULT_THEME), None, std::slice::from_ref(&path), 1200.0, 800.0).expect("abre");
            shell.set_auto_save("afterDelay", 1000);
            shell.active_editor_mut().expect("aba").view.type_text("X");
            shell.take_dirty();
            je_base::runtime::advance_fake_time(500);
            shell.take_dirty();
            assert_eq!(std::fs::read_to_string(&path).expect("lê"), "abc", "antes do prazo nada é gravado");
            je_base::runtime::advance_fake_time(600);
            shell.take_dirty();
            assert_eq!(std::fs::read_to_string(&path).expect("lê"), "Xabc", "depois do prazo a aba foi salva");
            assert!(!shell.active_editor().expect("aba").is_modified());
        });
        std::fs::remove_file(&path).expect("remove");
    }

    #[test]
    fn single_click_opens_a_preview_tab_that_the_next_one_replaces() {
        let dir = std::env::temp_dir().join(format!("je-code-{}-preview", std::process::id()));
        std::fs::create_dir_all(&dir).expect("pasta");
        for n in ["a.txt", "b.txt", "c.txt"] {
            std::fs::write(dir.join(n), n).expect("arquivo");
        }
        let mut shell = Shell::open(Path::new(DEFAULT_THEME), Some(dir.clone()), &[], 1200.0, 800.0).expect("abre");
        snapshot(&mut shell, "p-0.png");
        // a.txt e depois b.txt pelo clique único: só uma aba, a de b.txt, em itálico.
        click(&mut shell, 120.0, 96.0 + 11.0);
        click(&mut shell, 120.0, 96.0 + 22.0 + 11.0);
        snapshot(&mut shell, "p-1.png");
        assert_eq!(shell.tab_paths(), vec![dir.join("b.txt")]);
        // Editar fixa a aba: c.txt abre ao lado.
        shell.active_editor_mut().expect("aba").view.type_text("X");
        shell.take_dirty();
        click(&mut shell, 120.0, 96.0 + 44.0 + 11.0);
        assert_eq!(shell.tab_paths(), vec![dir.join("b.txt"), dir.join("c.txt")]);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn split_editor_shows_two_groups_sharing_the_document() {
        use je_editor::cursor_common::CursorSimpleModel;
        let path = temp_file("split.ts", "const a = 1;\nconst b = 2;\n");
        let mut shell = Shell::open(Path::new(DEFAULT_THEME), None, std::slice::from_ref(&path), 1200.0, 800.0).expect("abre");
        shell.run(shell::Command::SplitEditor);
        snapshot(&mut shell, "sp0-split.png");
        assert_eq!(shell.group_count(), 2);
        // Digitar no grupo da direita aparece no da esquerda (o modelo é o mesmo).
        shell.active_editor_mut().expect("aba").view.type_text("X");
        shell.run(shell::Command::FocusGroup(0));
        assert!(shell.active_editor().expect("aba").view.model().get_line_content(1).to_string_lossy().starts_with('X'));
        // Clicar no grupo da direita o foca; fechar a aba dele fecha o grupo.
        click(&mut shell, 1000.0, 300.0);
        shell.run(shell::Command::CloseEditor(0));
        // A aba está modificada: o diálogo pergunta; Don't Save fecha.
        shell.run(shell::Command::DiscardAndClose(0));
        snapshot(&mut shell, "sp1-closed.png");
        assert_eq!(shell.group_count(), 1, "o grupo vazio fechou");
        assert!(shell.active_editor().is_some(), "o grupo que sobrou mantém a aba");
        std::fs::remove_file(&path).expect("remove");
    }

    #[test]
    fn find_and_replace_replaces_one_and_all() {
        use je_editor::cursor_common::CursorSimpleModel;
        let path = temp_file("replace.txt", "foo bar foo\nfoo-1 foo-22\n");
        let mut shell = Shell::open(Path::new(DEFAULT_THEME), None, std::slice::from_ref(&path), 1200.0, 800.0).expect("abre");
        let key = |shell: &mut Shell, key: Option<EditorKey>, mods: Modifiers, text: Option<&str>| shell.key(true, KeyCode::Unknown, mods, text, key);
        let type_text = |shell: &mut Shell, text: &str| {
            for c in text.chars() {
                key(shell, None, Modifiers::default(), Some(&c.to_string()));
            }
        };
        let line = |shell: &Shell, n: i32| shell.active_editor().expect("aba").view.model().get_line_content(n).to_string_lossy();
        // Ctrl+H com a palavra sob o cursor (`foo`) já semeada: o teclado vai direto para a substituição.
        shell.run(shell::Command::FindAndReplace);
        type_text(&mut shell, "baz");
        snapshot(&mut shell, "r0-replace.png");
        // Enter na substituição troca a ocorrência atual (a primeira) e vai para a próxima.
        key(&mut shell, Some(EditorKey::Enter), Modifiers::default(), None);
        assert_eq!(line(&shell, 1), "baz bar foo");
        // Ctrl+Alt+Enter troca todas as que sobraram.
        key(&mut shell, Some(EditorKey::Enter), Modifiers { ctrl: true, alt: true, ..Modifiers::default() }, None);
        assert_eq!((line(&shell, 1), line(&shell, 2)), ("baz bar baz".to_owned(), "baz-1 baz-22".to_owned()));
        // Regex com grupo: `baz-(\d+)` vira `n$1`.
        shell.run(shell::Command::FindClose);
        shell.run(shell::Command::Find);
        shell.run(shell::Command::FindToggleRegex);
        type_text(&mut shell, "baz-(\\d+)");
        shell.run(shell::Command::FindToggleReplace);
        for _ in 0..3 {
            key(&mut shell, Some(EditorKey::Backspace), Modifiers::default(), None);
        }
        type_text(&mut shell, "n$1");
        shell.run(shell::Command::FindReplaceAll);
        assert_eq!(line(&shell, 2), "n1 n22");
        std::fs::remove_file(&path).expect("remove");
    }

    #[test]
    fn replace_in_files_rewrites_closed_files_and_edits_open_ones() {
        use je_editor::cursor_common::CursorSimpleModel;
        let dir = std::env::temp_dir().join(format!("je-code-{}-rif", std::process::id()));
        std::fs::create_dir_all(&dir).expect("pasta");
        std::fs::write(dir.join("a.txt"), "old one\nold two\n").expect("a");
        std::fs::write(dir.join("b.txt"), "keep old\n").expect("b");
        let mut shell = Shell::open(Path::new(DEFAULT_THEME), Some(dir.clone()), &[dir.join("b.txt")], 1200.0, 800.0).expect("abre");
        let key = |shell: &mut Shell, key: Option<EditorKey>, text: Option<&str>| shell.key(true, KeyCode::Unknown, Modifiers::default(), text, key);
        shell.run(shell::Command::ShowSearchReplace);
        // A semente é a palavra sob o cursor de b.txt (`keep`): troca por `old`.
        for _ in 0..4 {
            key(&mut shell, Some(EditorKey::Backspace), None);
        }
        for c in "old".chars() {
            key(&mut shell, None, Some(&c.to_string()));
        }
        shell.run(shell::Command::SearchNow);
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while shell.search_message().as_deref() != Some("3 results in 2 files") && std::time::Instant::now() < deadline {
            shell.take_dirty();
            std::thread::sleep(Duration::from_millis(20));
        }
        key(&mut shell, Some(EditorKey::Tab), None);
        for c in "new".chars() {
            key(&mut shell, None, Some(&c.to_string()));
        }
        snapshot(&mut shell, "rif0.png");
        shell.run(shell::Command::SearchReplaceAll);
        assert_eq!(std::fs::read_to_string(dir.join("a.txt")).expect("a"), "new one\nnew two\n", "o arquivo fechado foi reescrito");
        let open = shell.active_editor().expect("aba");
        assert_eq!(open.view.model().get_line_content(1).to_string_lossy(), "keep new", "o aberto recebeu a edição");
        assert!(open.is_modified(), "e fica modificado até salvar");
        assert_eq!(std::fs::read_to_string(dir.join("b.txt")).expect("b"), "keep old\n");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn closing_tabs_moves_the_active_editor() {
        let a = temp_file("a.txt", "a");
        let b = temp_file("b.txt", "b");
        let mut shell = Shell::open(Path::new(DEFAULT_THEME), None, &[a.clone(), b.clone()], 1200.0, 800.0).expect("abre");
        assert_eq!(shell.active_editor().expect("aba").path.as_deref(), Some(b.as_path()));
        shell.run(shell::Command::CloseActiveEditor);
        assert_eq!(shell.active_editor().expect("aba").path.as_deref(), Some(a.as_path()));
        shell.run(shell::Command::CloseActiveEditor);
        assert!(shell.active_editor().is_none());
        for p in [a, b] {
            std::fs::remove_file(p).expect("remove");
        }
    }
}
