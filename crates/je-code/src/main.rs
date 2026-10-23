//! O executável do je-code. Por enquanto mostra a moldura do workbench (título, activity bar, editor e
//! barra de status) com o tema 2026 Dark do VS Code, os codicons, e um arquivo de texto no editor com
//! rolagem; o workbench de verdade entra na Fase 4 do `roadmap.md`.
//!
//! Uso: `je-code [arquivo]` (sem argumento mostra o próprio código-fonte). `JE_RENDERER=gpu|cpu` força
//! o caminho de pintura; `JE_THEME` aponta outro tema JSON do VS Code.

use std::path::PathBuf;
use std::sync::Arc;

use je_render::kurbo::Rect;
use je_render::{Backend, DisplayList, Preference, Presenter, Rgba};
use je_text::TextStyle;
use je_theme::ColorTheme;
use je_ui::{Border, Content, Role, Tree};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowId};

const UI_FONT: &str = "system-ui, \"Ubuntu\", \"Droid Sans\", sans-serif";
const EDITOR_FONT: &str = "\"Droid Sans Mono\", \"monospace\", monospace";
const EDITOR_LINE_HEIGHT: f64 = 19.0;
const DEFAULT_THEME: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../je-theme/tests/golden/themes/2026-dark.json");

struct Workbench {
    theme: ColorTheme,
    lines: Vec<String>,
    file_name: String,
    scroll: f64,
    list: DisplayList,
}

impl Workbench {
    fn color(&self, id: &str) -> Rgba {
        self.theme.get_color(id).unwrap_or(Rgba::opaque(0xff, 0x00, 0xff))
    }

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
        style.content = Content::Text { text: format!("  je-code  /  {}", self.file_name), style: ui(12.0), color: foreground, line_height: 25.0 };

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

        // Editor: painel flutuante com números de linha e texto, recortado, com rolagem.
        let editor = tree.create(root, Role::Region);
        tree.set_bounds(editor, Rect::new(52.0, 35.0, width - 4.0, height - 32.0));
        let style = tree.style_mut(editor);
        style.background = Some(self.color("editor.background"));
        style.border = Some(Border { width: 1.0, color: self.color("editorGroup.border") });
        style.radius = 8.0;
        style.clip = true;
        let viewport = height - 32.0 - 35.0;
        let max_scroll = (self.lines.len() as f64 * EDITOR_LINE_HEIGHT - viewport + 20.0).max(0.0);
        self.scroll = self.scroll.clamp(0.0, max_scroll);
        let first = (self.scroll / EDITOR_LINE_HEIGHT) as usize;
        let visible = (viewport / EDITOR_LINE_HEIGHT) as usize + 2;
        let code = TextStyle::new(EDITOR_FONT, 14.0, 400);
        let number_color = self.color("editorLineNumber.foreground");
        let editor_foreground = self.color("editor.foreground");
        for row in 0..visible {
            let index = first + row;
            let Some(text) = self.lines.get(index) else { break };
            let y = row as f64 * EDITOR_LINE_HEIGHT - (self.scroll - first as f64 * EDITOR_LINE_HEIGHT);
            let number = tree.create(editor, Role::Generic);
            tree.set_bounds(number, Rect::new(0.0, y, 56.0, y + EDITOR_LINE_HEIGHT));
            tree.style_mut(number).content = Content::Text { text: format!("{:>5}", index + 1), style: code.clone(), color: number_color, line_height: EDITOR_LINE_HEIGHT };
            if !text.trim().is_empty() {
                let line = tree.create(editor, Role::Generic);
                tree.set_bounds(line, Rect::new(72.0, y, width, y + EDITOR_LINE_HEIGHT));
                tree.style_mut(line).content = Content::Text { text: text.replace('\t', "    "), style: code.clone(), color: editor_foreground, line_height: EDITOR_LINE_HEIGHT };
            }
        }

        // Barra de status.
        let status = tree.create(root, Role::Status);
        tree.set_bounds(status, Rect::new(0.0, height - 28.0, width, height));
        tree.style_mut(status).background = Some(self.color("statusBar.background"));
        let info = tree.create(status, Role::Generic);
        tree.set_bounds(info, Rect::new(12.0, 0.0, width / 2.0, 28.0));
        tree.style_mut(info).content = Content::Text {
            text: format!("Ln {}, {} linhas   |   pintura: {backend}", first + 1, self.lines.len()),
            style: ui(12.0),
            color: muted,
            line_height: 28.0,
        };
        tree
    }
}

struct App {
    workbench: Workbench,
    window: Option<Arc<Window>>,
    presenter: Option<Presenter>,
    backend_label: String,
}

impl App {
    fn frame(&mut self) {
        let (Some(window), Some(presenter)) = (&self.window, &mut self.presenter) else { return };
        let scale = window.scale_factor();
        let size = window.inner_size().to_logical::<f64>(scale);
        let tree = self.workbench.build(size.width, size.height, &self.backend_label);
        self.workbench.list.clear();
        tree.paint(&mut self.workbench.list, scale);
        let background = self.workbench.color("editor.background");
        if let Err(e) = presenter.present(&self.workbench.list, background) {
            eprintln!("je-code: frame descartado: {e}");
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attrs = Window::default_attributes().with_title("je-code").with_inner_size(LogicalSize::new(1200.0, 800.0));
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
            WindowEvent::MouseWheel { delta, .. } => {
                let pixels = match delta {
                    MouseScrollDelta::LineDelta(_, y) => -f64::from(y) * 3.0 * EDITOR_LINE_HEIGHT,
                    MouseScrollDelta::PixelDelta(p) => -p.y,
                };
                self.workbench.scroll += pixels;
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
            }
            WindowEvent::RedrawRequested => self.frame(),
            _ => {}
        }
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

fn main() {
    // No WSL a GPU só aparece com o driver d3d12 do Mesa; relança com o ambiente certo (H27).
    if let Some(e) = je_render::wsl::relaunch_with_gpu() {
        eprintln!("je-code: relançamento com a GPU do WSL falhou ({e}); seguindo sem ele");
    }
    let theme_path = std::env::var_os("JE_THEME").map_or_else(|| PathBuf::from(DEFAULT_THEME), PathBuf::from);
    let theme = match ColorTheme::load(&theme_path) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("je-code: tema inválido: {e}");
            std::process::exit(1);
        }
    };
    let (file_name, text) = match std::env::args().nth(1) {
        Some(path) => match std::fs::read_to_string(&path) {
            Ok(text) => (path.rsplit(['/', '\\']).next().unwrap_or(&path).to_owned(), text),
            Err(e) => {
                eprintln!("je-code: {path}: {e}");
                std::process::exit(1);
            }
        },
        None => ("main.rs".to_owned(), include_str!("main.rs").to_owned()),
    };
    let lines = text.lines().map(str::to_owned).collect();
    let event_loop = match build_event_loop() {
        Ok(l) => l,
        Err(e) => {
            eprintln!("je-code: sem event loop: {e}");
            std::process::exit(1);
        }
    };
    let mut app = App {
        workbench: Workbench { theme, lines, file_name, scroll: 0.0, list: DisplayList::new() },
        window: None,
        presenter: None,
        backend_label: String::new(),
    };
    if let Err(e) = event_loop.run_app(&mut app) {
        eprintln!("je-code: {e}");
        std::process::exit(1);
    }
}
