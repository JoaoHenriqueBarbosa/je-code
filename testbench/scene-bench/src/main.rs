//! Sondas H03 e H04: a mesma cena de workbench pintada pelo `vello_cpu` (CPU) e pelo `vello` (GPU).
//!
//! Modos:
//! - `headless-cpu <frames>` e `headless-gpu <frames>`: só o custo de pintar, sem janela.
//! - `window-cpu <segundos>` e `window-gpu <segundos>`: janela winit, apresentando pelo softbuffer
//!   ou pelo wgpu, sem vsync quando o backend permite; mede fps de ponta a ponta.
//!
//! Variáveis: `SCENE_MONO` e `SCENE_UI` (arquivos de fonte), `SCENE_SOURCE` (texto do editor).

mod scene;

use std::sync::Arc;
use std::time::{Duration, Instant};

use scene::{HEIGHT, Painter, Rgba, Run, WIDTH, Workbench};
use vello::kurbo::{Affine, Rect};
use vello::peniko::{Color, Fill};
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

const WARMUP: u64 = 30;

fn color(c: Rgba) -> Color {
    Color::from_rgba8(c[0], c[1], c[2], c[3])
}

struct GpuPainter<'a>(&'a mut vello::Scene);

impl Painter for GpuPainter<'_> {
    fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, c: Rgba) {
        let r = Rect::new(f64::from(x), f64::from(y), f64::from(x + w), f64::from(y + h));
        self.0.fill(Fill::NonZero, Affine::IDENTITY, color(c), None, &r);
    }

    fn glyphs(&mut self, run: &Run, dx: f32, dy: f32) {
        self.0
            .draw_glyphs(&run.font)
            .font_size(run.size)
            .brush(color(run.color))
            .draw(Fill::NonZero, run.glyphs.iter().map(|&(id, x, y)| vello::Glyph { id, x: x + dx, y: y + dy }));
    }
}

struct CpuPainter<'a> {
    ctx: &'a mut vello_cpu::RenderContext,
    resources: &'a mut vello_cpu::Resources,
}

impl Painter for CpuPainter<'_> {
    fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, c: Rgba) {
        self.ctx.set_paint(color(c));
        self.ctx.fill_rect(&Rect::new(f64::from(x), f64::from(y), f64::from(x + w), f64::from(y + h)));
    }

    fn glyphs(&mut self, run: &Run, dx: f32, dy: f32) {
        self.ctx.set_paint(color(run.color));
        let glyphs = run.glyphs.iter().map(|&(id, x, y)| vello_cpu::Glyph { id, x: x + dx, y: y + dy });
        let _ = self.ctx.glyph_run(self.resources, &run.font).font_size(run.size).fill_glyphs(glyphs);
    }
}

/// Estado de pintura em CPU: contexto do vello_cpu e o pixmap de destino.
struct Cpu {
    ctx: vello_cpu::RenderContext,
    resources: vello_cpu::Resources,
    pixmap: vello_cpu::Pixmap,
}

impl Cpu {
    fn new() -> Self {
        let settings = vello_cpu::RenderSettings::default();
        println!("vello_cpu: {} threads, nível SIMD {:?}", settings.num_threads, settings.level);
        Self {
            ctx: vello_cpu::RenderContext::new_with(WIDTH as u16, HEIGHT as u16, settings),
            resources: vello_cpu::Resources::new(),
            pixmap: vello_cpu::Pixmap::new(WIDTH as u16, HEIGHT as u16),
        }
    }

    fn frame(&mut self, workbench: &Workbench, frame: u64) {
        self.ctx.reset();
        workbench.paint(&mut CpuPainter { ctx: &mut self.ctx, resources: &mut self.resources }, frame);
        self.ctx.flush();
        self.ctx.render(&mut self.pixmap, &mut self.resources);
    }
}

/// Estado de pintura em GPU: renderer do vello e a textura intermediária (o vello escreve em
/// textura de storage `Rgba8Unorm`; a apresentação copia para a surface pelo blitter do wgpu).
struct Gpu {
    device: vello::wgpu::Device,
    queue: vello::wgpu::Queue,
    renderer: vello::Renderer,
    scene: vello::Scene,
    view: vello::wgpu::TextureView,
}

impl Gpu {
    fn new(device: vello::wgpu::Device, queue: vello::wgpu::Queue) -> Self {
        let renderer = vello::Renderer::new(
            &device,
            vello::RendererOptions {
                use_cpu: false,
                antialiasing_support: vello::AaSupport::area_only(),
                num_init_threads: None,
                pipeline_cache: None,
            },
        )
        .expect("renderer do vello");
        let texture = device.create_texture(&vello::wgpu::TextureDescriptor {
            label: Some("cena"),
            size: vello::wgpu::Extent3d { width: WIDTH, height: HEIGHT, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: vello::wgpu::TextureDimension::D2,
            format: vello::wgpu::TextureFormat::Rgba8Unorm,
            usage: vello::wgpu::TextureUsages::STORAGE_BINDING | vello::wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        Self { device, queue, renderer, scene: vello::Scene::new(), view }
    }

    fn frame(&mut self, workbench: &Workbench, frame: u64) {
        self.scene.reset();
        workbench.paint(&mut GpuPainter(&mut self.scene), frame);
        self.renderer
            .render_to_texture(
                &self.device,
                &self.queue,
                &self.scene,
                &self.view,
                &vello::RenderParams {
                    base_color: Color::BLACK,
                    width: WIDTH,
                    height: HEIGHT,
                    antialiasing_method: vello::AaConfig::Area,
                },
            )
            .expect("render do vello");
    }

    fn wait(&self) {
        let _ = self.device.poll(vello::wgpu::PollType::wait_indefinitely());
    }
}

fn adapter(instance: &vello::wgpu::Instance, surface: Option<&vello::wgpu::Surface<'_>>) -> vello::wgpu::Adapter {
    let adapter = pollster::block_on(instance.request_adapter(&vello::wgpu::RequestAdapterOptions {
        power_preference: vello::wgpu::PowerPreference::HighPerformance,
        compatible_surface: surface,
        ..Default::default()
    }))
    .expect("adapter");
    let info = adapter.get_info();
    println!("adapter: {:?} | {} | {:?}", info.backend, info.name, info.device_type);
    adapter
}

fn device(adapter: &vello::wgpu::Adapter) -> (vello::wgpu::Device, vello::wgpu::Queue) {
    pollster::block_on(adapter.request_device(&vello::wgpu::DeviceDescriptor {
        required_limits: adapter.limits(),
        ..Default::default()
    }))
    .expect("device")
}

/// Estatística de tempos de frame em milissegundos.
fn report(label: &str, mut times: Vec<Duration>) {
    times.sort();
    let ms = |d: Duration| d.as_secs_f64() * 1000.0;
    let mean = times.iter().map(|d| ms(*d)).sum::<f64>() / times.len() as f64;
    let pick = |q: f64| ms(times[((times.len() - 1) as f64 * q) as usize]);
    println!(
        "{label}: {} frames | média {mean:.2} ms ({:.1} fps) | p50 {:.2} | p95 {:.2} | p99 {:.2} | máx {:.2}",
        times.len(),
        1000.0 / mean,
        pick(0.5),
        pick(0.95),
        pick(0.99),
        pick(1.0)
    );
}

fn headless(workbench: &Workbench, gpu: bool, frames: u64) {
    let mut times = Vec::with_capacity(frames as usize);
    if gpu {
        let instance = vello::wgpu::Instance::new(vello::wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        let (device, queue) = device(&adapter(&instance, None));
        let mut state = Gpu::new(device, queue);
        for f in 0..10 {
            state.frame(workbench, f);
            state.wait();
        }
        for f in 0..frames {
            let start = Instant::now();
            state.frame(workbench, f);
            state.wait();
            times.push(start.elapsed());
        }
        report("GPU sem janela (vello)", times);
    } else {
        let mut state = Cpu::new();
        for f in 0..10 {
            state.frame(workbench, f);
        }
        for f in 0..frames {
            let start = Instant::now();
            state.frame(workbench, f);
            times.push(start.elapsed());
        }
        report("CPU sem janela (vello_cpu)", times);
    }
}

enum Present {
    Cpu {
        state: Cpu,
        surface: softbuffer::Surface<Arc<Window>, Arc<Window>>,
    },
    Gpu {
        state: Gpu,
        surface: vello::wgpu::Surface<'static>,
        config: vello::wgpu::SurfaceConfiguration,
        blitter: vello::wgpu::util::TextureBlitter,
    },
}

struct App {
    workbench: Workbench,
    gpu: bool,
    duration: Duration,
    window: Option<Arc<Window>>,
    present: Option<Present>,
    frame: u64,
    start: Option<Instant>,
    times: Vec<Duration>,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attrs = Window::default_attributes()
            .with_title("je-code scene-bench")
            .with_resizable(false)
            .with_inner_size(PhysicalSize::new(WIDTH, HEIGHT));
        let window = Arc::new(event_loop.create_window(attrs).expect("janela"));
        let size = window.inner_size();
        println!("janela: {}x{} escala={}", size.width, size.height, window.scale_factor());
        self.present = Some(if self.gpu {
            let instance = vello::wgpu::Instance::new(
                vello::wgpu::InstanceDescriptor::new_with_display_handle_from_env(Box::new(window.clone())),
            );
            let surface = instance.create_surface(window.clone()).expect("surface");
            let adapter = adapter(&instance, Some(&surface));
            let (device, queue) = device(&adapter);
            let caps = surface.get_capabilities(&adapter);
            let mut config = surface.get_default_config(&adapter, WIDTH, HEIGHT).expect("config");
            config.present_mode = if caps.present_modes.contains(&vello::wgpu::PresentMode::Immediate) {
                vello::wgpu::PresentMode::Immediate
            } else {
                vello::wgpu::PresentMode::AutoVsync
            };
            println!("surface: {:?} modo {:?} (disponíveis {:?})", config.format, config.present_mode, caps.present_modes);
            surface.configure(&device, &config);
            let blitter = vello::wgpu::util::TextureBlitter::new(&device, config.format);
            Present::Gpu { state: Gpu::new(device, queue), surface, config, blitter }
        } else {
            let context = softbuffer::Context::new(window.clone()).expect("softbuffer");
            let mut surface = softbuffer::Surface::new(&context, window.clone()).expect("surface");
            surface
                .resize(WIDTH.try_into().expect("largura"), HEIGHT.try_into().expect("altura"))
                .expect("resize");
            Present::Cpu { state: Cpu::new(), surface }
        });
        self.start = Some(Instant::now());
        window.request_redraw();
        self.window = Some(window);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::RedrawRequested => self.redraw(event_loop),
            _ => {}
        }
    }
}

impl App {
    fn redraw(&mut self, event_loop: &ActiveEventLoop) {
        let (Some(present), Some(window), Some(start)) = (&mut self.present, &self.window, self.start) else {
            return;
        };
        let frame_start = Instant::now();
        match present {
            Present::Cpu { state, surface } => {
                state.frame(&self.workbench, self.frame);
                let mut buffer = surface.buffer_mut().expect("buffer");
                for (dst, src) in buffer.iter_mut().zip(state.pixmap.data()) {
                    *dst = (u32::from(src.r) << 16) | (u32::from(src.g) << 8) | u32::from(src.b);
                }
                buffer.present().expect("present");
            }
            Present::Gpu { state, surface, config, blitter } => {
                // Adquire a imagem da swapchain antes de pintar: a espera do vsync acontece aqui, e a
                // pintura e a cópia saem logo depois, dentro do mesmo intervalo.
                let texture = match surface.get_current_texture() {
                    vello::wgpu::CurrentSurfaceTexture::Success(t) | vello::wgpu::CurrentSurfaceTexture::Suboptimal(t) => t,
                    other => {
                        println!("surface sem frame: {other:?}");
                        surface.configure(&state.device, config);
                        window.request_redraw();
                        return;
                    }
                };
                state.frame(&self.workbench, self.frame);
                let target = texture.texture.create_view(&Default::default());
                let mut encoder = state.device.create_command_encoder(&Default::default());
                blitter.copy(&state.device, &mut encoder, &state.view, &target);
                state.queue.submit([encoder.finish()]);
                state.queue.present(texture);
            }
        }
        self.frame += 1;
        // Os primeiros frames compilam shaders e enchem caches: ficam fora da medida.
        if self.frame == WARMUP {
            self.start = Some(Instant::now());
            window.request_redraw();
            return;
        }
        if self.frame < WARMUP {
            window.request_redraw();
            return;
        }
        self.times.push(frame_start.elapsed());
        let elapsed = start.elapsed();
        if elapsed >= self.duration {
            println!(
                "janela {}: {} frames em {:.2} s = {:.1} fps de ponta a ponta (após {WARMUP} de aquecimento)",
                if self.gpu { "GPU" } else { "CPU" },
                self.times.len(),
                elapsed.as_secs_f64(),
                self.times.len() as f64 / elapsed.as_secs_f64()
            );
            report("tempo de CPU por frame (pintar e enviar)", std::mem::take(&mut self.times));
            event_loop.exit();
        } else {
            window.request_redraw();
        }
    }
}

fn read(var: &str, default: &str) -> Vec<u8> {
    let path = std::env::var(var).unwrap_or_else(|_| default.to_owned());
    std::fs::read(&path).unwrap_or_else(|e| panic!("{var}={path}: {e}"))
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mode = args.first().map_or("headless-cpu", String::as_str);
    let amount: u64 = args.get(1).and_then(|a| a.parse().ok()).unwrap_or(300);
    let source = String::from_utf8(read(
        "SCENE_SOURCE",
        "ref/vscode/src/vs/editor/common/model/pieceTreeTextBuffer/pieceTreeBase.ts",
    ))
    .expect("fonte UTF-8");
    // SCENE_DENSE=n repete cada linha n vezes na horizontal: o arquivo real tem linhas curtas, e o pior
    // caso do editor (minified, logs, CSV) enche a largura toda.
    let dense: usize = std::env::var("SCENE_DENSE").ok().and_then(|v| v.parse().ok()).unwrap_or(1);
    let source = source.lines().map(|l| l.repeat(dense)).collect::<Vec<_>>().join("\n");
    let t = Instant::now();
    let workbench = Workbench::new(
        read("SCENE_MONO", "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf"),
        read("SCENE_UI", "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf"),
        &source,
    );
    println!(
        "cena: {} linhas diagramadas em {:.1} ms, {} glifos por frame",
        source.lines().count(),
        t.elapsed().as_secs_f64() * 1000.0,
        workbench.glyph_count_per_frame()
    );
    match mode {
        "headless-cpu" => headless(&workbench, false, amount),
        "headless-gpu" => headless(&workbench, true, amount),
        "window-cpu" | "window-gpu" => {
            let event_loop = EventLoop::new().expect("event loop");
            event_loop.set_control_flow(ControlFlow::Poll);
            let mut app = App {
                workbench,
                gpu: mode == "window-gpu",
                duration: Duration::from_secs(amount),
                window: None,
                present: None,
                frame: 0,
                start: None,
                times: Vec::new(),
            };
            event_loop.run_app(&mut app).expect("run");
        }
        other => eprintln!("modo desconhecido: {other}"),
    }
}
