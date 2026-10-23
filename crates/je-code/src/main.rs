//! O executável do je-code. Por enquanto abre a janela com a moldura vazia do workbench e mostra
//! qual backend de pintura foi escolhido; o workbench entra na Fase 4 do `roadmap.md`.

use std::sync::Arc;

use je_render::kurbo::Rect;
use je_render::{DisplayList, Presenter, Preference, Rgba};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowId};

// Cores do tema Dark Modern (`extensions/theme-defaults/themes/dark_modern.json`).
const EDITOR_BACKGROUND: Rgba = Rgba::opaque(0x1f, 0x1f, 0x1f);
const SIDE_BAR_BACKGROUND: Rgba = Rgba::opaque(0x18, 0x18, 0x18);
const BORDER: Rgba = Rgba::opaque(0x2b, 0x2b, 0x2b);
const STATUS_BAR_BACKGROUND: Rgba = Rgba::opaque(0x18, 0x18, 0x18);

#[derive(Default)]
struct App {
    window: Option<Arc<Window>>,
    presenter: Option<Presenter>,
    list: DisplayList,
}

impl App {
    fn frame(&mut self) {
        let (Some(window), Some(presenter)) = (&self.window, &mut self.presenter) else {
            return;
        };
        let size = window.inner_size().to_logical::<f64>(window.scale_factor());
        let scale = window.scale_factor();
        let px = |v: f64| v * scale;
        let (w, h) = (size.width, size.height);
        self.list.clear();
        self.list.fill_rect(Rect::new(0.0, 0.0, px(48.0), px(h - 22.0)), SIDE_BAR_BACKGROUND);
        self.list.fill_rect(Rect::new(px(48.0), 0.0, px(48.0 + 300.0), px(h - 22.0)), SIDE_BAR_BACKGROUND);
        self.list.fill_rect(Rect::new(px(348.0), 0.0, px(349.0), px(h - 22.0)), BORDER);
        self.list.fill_rect(Rect::new(0.0, px(h - 22.0), px(w), px(h)), STATUS_BAR_BACKGROUND);
        self.list.fill_rect(Rect::new(0.0, px(h - 22.0), px(w), px(h - 21.0)), BORDER);
        if let Err(e) = presenter.present(&self.list, EDITOR_BACKGROUND) {
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
            WindowEvent::RedrawRequested => self.frame(),
            _ => {}
        }
    }
}

fn main() {
    // No WSL a GPU só aparece com o driver d3d12 do Mesa; relança com o ambiente certo (H27).
    if let Some(e) = je_render::wsl::relaunch_with_gpu() {
        eprintln!("je-code: relançamento com a GPU do WSL falhou ({e}); seguindo sem ele");
    }
    let event_loop = match EventLoop::new() {
        Ok(l) => l,
        Err(e) => {
            eprintln!("je-code: sem event loop: {e}");
            std::process::exit(1);
        }
    };
    let mut app = App::default();
    if let Err(e) = event_loop.run_app(&mut app) {
        eprintln!("je-code: {e}");
        std::process::exit(1);
    }
}
