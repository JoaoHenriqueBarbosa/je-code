//! Sonda H08: a cena do `scene-bench` (a mesma `Workbench`, os mesmos pintores) num `<canvas>`, por
//! dois caminhos: `vello` sobre wgpu com WebGPU, e `vello_cpu` num buffer com `putImageData`.
//!
//! Um `requestAnimationFrame` rola a cena e mede, depois do aquecimento, o intervalo entre frames
//! (fps de ponta a ponta, preso ao vsync da tela), o tempo de CPU por frame (pintar e enviar) e, na
//! GPU, a latência entre o envio e a conclusão do trabalho (`on_submitted_work_done`).

// Só existe no alvo wasm: o canvas e o WebGPU não têm equivalente nativo (o nativo é o `scene-bench`).
#![cfg(target_arch = "wasm32")]

#[path = "../../scene-bench/src/painters.rs"]
mod painters;
#[path = "../../scene-bench/src/scene.rs"]
mod scene;

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use painters::{CpuPainter, GpuPainter};
use scene::{HEIGHT, WIDTH, Workbench};
use vello::peniko::Color;
use wasm_bindgen::prelude::*;
use wasm_bindgen::Clamped;

const WARMUP: u64 = 30;

fn now() -> f64 {
    web_sys::window().expect("window").performance().expect("performance").now()
}

fn err(e: impl std::fmt::Debug) -> JsValue {
    JsValue::from_str(&format!("{e:?}"))
}

/// Média, p50, p95, p99 e máximo de uma série em milissegundos.
fn stats(label: &str, mut v: Vec<f64>) -> String {
    if v.is_empty() {
        return format!("{label}: sem amostras");
    }
    v.sort_by(f64::total_cmp);
    let mean = v.iter().sum::<f64>() / v.len() as f64;
    let pick = |q: f64| v[((v.len() - 1) as f64 * q) as usize];
    format!(
        "{label}: {} amostras | média {mean:.2} ms ({:.1} fps) | p50 {:.2} | p95 {:.2} | p99 {:.2} | máx {:.2}",
        v.len(),
        1000.0 / mean,
        pick(0.5),
        pick(0.95),
        pick(0.99),
        pick(1.0)
    )
}

struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    renderer: vello::Renderer,
    scene: vello::Scene,
    view: wgpu::TextureView,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    blitter: wgpu::util::TextureBlitter,
    latencies: Arc<Mutex<Vec<f64>>>,
    in_flight: Arc<AtomicU32>,
}

impl Gpu {
    async fn new(canvas: web_sys::HtmlCanvasElement, log: &mut String) -> Result<Self, JsValue> {
        let instance = wgpu::Instance::default();
        let surface = instance.create_surface(wgpu::SurfaceTarget::Canvas(canvas)).map_err(err)?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                ..Default::default()
            })
            .await
            .map_err(err)?;
        let info = adapter.get_info();
        log.push_str(&format!("adapter: {:?} | {} | {:?} | {}\n", info.backend, info.name, info.device_type, info.driver));
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor { required_limits: adapter.limits(), ..Default::default() })
            .await
            .map_err(err)?;
        let mut config = surface.get_default_config(&adapter, WIDTH, HEIGHT).ok_or("sem config de surface")?;
        config.present_mode = wgpu::PresentMode::AutoVsync;
        surface.configure(&device, &config);
        let renderer = vello::Renderer::new(
            &device,
            vello::RendererOptions {
                use_cpu: false,
                antialiasing_support: vello::AaSupport::area_only(),
                num_init_threads: None,
                pipeline_cache: None,
            },
        )
        .map_err(err)?;
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("cena"),
            size: wgpu::Extent3d { width: WIDTH, height: HEIGHT, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        let blitter = wgpu::util::TextureBlitter::new(&device, config.format);
        Ok(Self {
            device,
            queue,
            renderer,
            scene: vello::Scene::new(),
            view,
            surface,
            config,
            blitter,
            latencies: Arc::default(),
            in_flight: Arc::default(),
        })
    }

    /// Frames enviados e ainda não concluídos pela GPU.
    fn in_flight(&self) -> u32 {
        self.in_flight.load(Ordering::Relaxed)
    }

    fn frame(&mut self, workbench: &Workbench, frame: u64) {
        let texture = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t) | wgpu::CurrentSurfaceTexture::Suboptimal(t) => t,
            _ => {
                self.surface.configure(&self.device, &self.config);
                return;
            }
        };
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
        let target = texture.texture.create_view(&Default::default());
        let mut encoder = self.device.create_command_encoder(&Default::default());
        self.blitter.copy(&self.device, &mut encoder, &self.view, &target);
        self.queue.submit([encoder.finish()]);
        let (sent, latencies, in_flight) = (now(), self.latencies.clone(), self.in_flight.clone());
        in_flight.fetch_add(1, Ordering::Relaxed);
        self.queue.on_submitted_work_done(move || {
            in_flight.fetch_sub(1, Ordering::Relaxed);
            if frame >= WARMUP {
                latencies.lock().expect("lock").push(now() - sent);
            }
        });
        self.queue.present(texture);
    }
}

struct Cpu {
    ctx: vello_cpu::RenderContext,
    resources: vello_cpu::Resources,
    pixmap: vello_cpu::Pixmap,
    canvas: web_sys::CanvasRenderingContext2d,
}

impl Cpu {
    fn new(canvas: &web_sys::HtmlCanvasElement, log: &mut String) -> Result<Self, JsValue> {
        let settings = vello_cpu::RenderSettings::default();
        log.push_str(&format!("vello_cpu: {} threads, nível SIMD {:?}\n", settings.num_threads, settings.level));
        let context = canvas
            .get_context("2d")?
            .ok_or("sem contexto 2d")?
            .dyn_into::<web_sys::CanvasRenderingContext2d>()?;
        Ok(Self {
            ctx: vello_cpu::RenderContext::new_with(WIDTH as u16, HEIGHT as u16, settings),
            resources: vello_cpu::Resources::new(),
            pixmap: vello_cpu::Pixmap::new(WIDTH as u16, HEIGHT as u16),
            canvas: context,
        })
    }

    fn frame(&mut self, workbench: &Workbench, frame: u64) -> Result<(), JsValue> {
        self.ctx.reset();
        workbench.paint(&mut CpuPainter { ctx: &mut self.ctx, resources: &mut self.resources }, frame);
        self.ctx.flush();
        self.ctx.render(&mut self.pixmap, &mut self.resources);
        let image = web_sys::ImageData::new_with_u8_clamped_array_and_sh(
            Clamped(self.pixmap.data_as_u8_slice()),
            WIDTH,
            HEIGHT,
        )?;
        self.canvas.put_image_data(&image, 0.0, 0.0)
    }
}

enum Backend {
    Gpu(Gpu),
    Cpu(Cpu),
}

struct Run {
    workbench: Workbench,
    backend: Backend,
    frame: u64,
    frames: u64,
    last: Option<f64>,
    measure_start: Option<f64>,
    intervals: Vec<f64>,
    work: Vec<f64>,
    log: String,
    done: Option<js_sys::Function>,
}

impl Run {
    /// Um frame de `requestAnimationFrame`; devolve `true` quando a medição acabou.
    fn step(&mut self, timestamp: f64) -> bool {
        // No máximo dois frames na GPU ao mesmo tempo (contrapressão): sem isso o rAF segue a 60 Hz
        // enquanto a fila cresce e o intervalo não mede nada.
        if let Backend::Gpu(gpu) = &self.backend {
            if gpu.in_flight() >= 2 {
                return false;
            }
        }
        let start = now();
        let result = match &mut self.backend {
            Backend::Gpu(gpu) => {
                gpu.frame(&self.workbench, self.frame);
                Ok(())
            }
            Backend::Cpu(cpu) => cpu.frame(&self.workbench, self.frame),
        };
        if let Err(e) = result {
            self.log.push_str(&format!("erro: {e:?}\n"));
            return true;
        }
        let work = now() - start;
        if self.frame == WARMUP {
            self.measure_start = Some(start);
        }
        if self.frame >= WARMUP {
            if let Some(last) = self.last {
                self.intervals.push(timestamp - last);
            }
            self.work.push(work);
        }
        if self.frame >= WARMUP - 1 {
            self.last = Some(timestamp);
        }
        self.frame += 1;
        false
    }

    /// A medição acaba quando os frames pedidos foram enviados e a GPU concluiu todos.
    fn finished(&self) -> bool {
        let drained = matches!(&self.backend, Backend::Gpu(gpu) if gpu.in_flight() == 0) || matches!(self.backend, Backend::Cpu(_));
        self.frame >= WARMUP + self.frames && drained
    }

    fn report(&mut self) -> String {
        let mut text = std::mem::take(&mut self.log);
        let elapsed = now() - self.measure_start.unwrap_or_else(now);
        text.push_str(&format!(
            "vazão: {} frames em {:.0} ms = {:.2} ms por frame ({:.1} fps)\n",
            self.frames,
            elapsed,
            elapsed / self.frames as f64,
            self.frames as f64 * 1000.0 / elapsed
        ));
        text.push_str(&stats("intervalo entre frames (rAF)", std::mem::take(&mut self.intervals)));
        text.push('\n');
        text.push_str(&stats("CPU por frame (pintar e enviar)", std::mem::take(&mut self.work)));
        if let Backend::Gpu(gpu) = &self.backend {
            text.push('\n');
            text.push_str(&stats("GPU, envio até conclusão", gpu.latencies.lock().expect("lock").clone()));
        }
        text
    }
}

fn request_frame(callback: &Closure<dyn FnMut(f64)>) {
    web_sys::window().expect("window").request_animation_frame(callback.as_ref().unchecked_ref()).expect("rAF");
}

/// Roda a medição: `mode` é `"gpu"` ou `"cpu"`; devolve o relatório em texto.
#[wasm_bindgen]
pub async fn run(mode: String, mono: Vec<u8>, ui: Vec<u8>, source: String, dense: u32, frames: u32) -> Result<String, JsValue> {
    std::panic::set_hook(Box::new(|info| web_sys::console::error_1(&JsValue::from_str(&info.to_string()))));
    let document = web_sys::window().ok_or("sem window")?.document().ok_or("sem document")?;
    let canvas = document
        .get_element_by_id("scene")
        .ok_or("sem canvas #scene")?
        .dyn_into::<web_sys::HtmlCanvasElement>()?;
    canvas.set_width(WIDTH);
    canvas.set_height(HEIGHT);
    let mut log = String::new();
    let source = source.lines().map(|l| l.repeat(dense.max(1) as usize)).collect::<Vec<_>>().join("\n");
    let t = now();
    let workbench = Workbench::new(mono, ui, &source);
    log.push_str(&format!(
        "cena: {} linhas diagramadas em {:.1} ms, {} glifos por frame\n",
        source.lines().count(),
        now() - t,
        workbench.glyph_count_per_frame()
    ));
    let t = now();
    let backend = match mode.as_str() {
        "gpu" => Backend::Gpu(Gpu::new(canvas, &mut log).await?),
        "cpu" => Backend::Cpu(Cpu::new(&canvas, &mut log)?),
        other => return Err(err(format!("modo desconhecido: {other}"))),
    };
    log.push_str(&format!("inicialização do backend: {:.1} ms\n", now() - t));

    let state = Rc::new(RefCell::new(Run {
        workbench,
        backend,
        frame: 0,
        frames: u64::from(frames),
        last: None,
        measure_start: None,
        intervals: Vec::new(),
        work: Vec::new(),
        log,
        done: None,
    }));
    let promise = js_sys::Promise::new(&mut |resolve, _| {
        state.borrow_mut().done = Some(resolve);
        // O fecho se referencia para se reagendar; o ciclo é vazado de propósito (sonda de vida curta).
        let slot: Rc<RefCell<Option<Closure<dyn FnMut(f64)>>>> = Rc::default();
        let again = slot.clone();
        let state = state.clone();
        *slot.borrow_mut() = Some(Closure::new(move |timestamp: f64| {
            let mut run = state.borrow_mut();
            if run.finished() || run.step(timestamp) {
                let report = JsValue::from_str(&run.report());
                if let Some(done) = run.done.take() {
                    let _ = done.call1(&JsValue::NULL, &report);
                }
            } else {
                request_frame(again.borrow().as_ref().expect("fecho"));
            }
        }));
        request_frame(slot.borrow().as_ref().expect("fecho"));
    });
    let report = wasm_bindgen_futures::JsFuture::from(promise).await?;
    Ok(report.as_string().unwrap_or_default())
}
