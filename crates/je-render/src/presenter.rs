//! Apresentação da lista de exibição numa janela winit, pela GPU ou pela CPU.
//!
//! A escolha segue a medida da bancada: GPU de verdade vai para o `vello` (H04); sem GPU, ou com só
//! um adapter de software (llvmpipe, lavapipe, WARP), vai para o `vello_cpu`, que no mesmo
//! hardware pinta mais rápido que o `vello` sobre GPU emulada (H03 contra H04: 10,7 ms contra
//! 16 ms por frame na cena de referência).

use std::num::NonZeroU32;
use std::sync::Arc;

use vello::wgpu;
use winit::window::Window;

use crate::display_list::{DisplayList, Rgba};
use crate::{cpu, gpu};

/// Preferência de backend. `Auto` é o padrão; `JE_RENDERER=gpu|cpu` força um dos dois.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Preference {
    Auto,
    Gpu,
    Cpu,
}

impl Preference {
    pub fn from_env() -> Self {
        match std::env::var("JE_RENDERER").as_deref() {
            Ok("gpu") => Preference::Gpu,
            Ok("cpu") => Preference::Cpu,
            _ => Preference::Auto,
        }
    }
}

/// O backend em uso, para diagnóstico (`Developer: Show Renderer` no futuro).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Backend {
    Gpu { adapter: String, api: String },
    Cpu { threads: u16 },
}

struct GpuState {
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    device: wgpu::Device,
    queue: wgpu::Queue,
    renderer: vello::Renderer,
    scene: vello::Scene,
    target: Option<(wgpu::Texture, wgpu::TextureView)>,
    blitter: wgpu::util::TextureBlitter,
}

struct CpuState {
    surface: softbuffer::Surface<Arc<Window>, Arc<Window>>,
    ctx: vello_cpu::RenderContext,
    resources: vello_cpu::Resources,
    pixmap: vello_cpu::Pixmap,
}

enum State {
    Gpu(Box<GpuState>),
    Cpu(Box<CpuState>),
}

/// Pinta listas de exibição numa janela.
pub struct Presenter {
    window: Arc<Window>,
    state: State,
    backend: Backend,
}

#[derive(Debug)]
pub struct PresentError(pub String);

impl std::fmt::Display for PresentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for PresentError {}

fn err(context: &str, e: impl std::fmt::Display) -> PresentError {
    PresentError(format!("{context}: {e}"))
}

fn size_of(window: &Window) -> (u32, u32) {
    let size = window.inner_size();
    (size.width.max(1), size.height.max(1))
}

impl Presenter {
    pub fn new(window: Arc<Window>, preference: Preference) -> Result<Self, PresentError> {
        if preference != Preference::Cpu {
            match Self::try_gpu(&window, preference == Preference::Gpu) {
                Ok((state, backend)) => return Ok(Self { window, state: State::Gpu(Box::new(state)), backend }),
                Err(e) if preference == Preference::Gpu => return Err(e),
                Err(_) => {}
            }
        }
        let (state, backend) = Self::cpu(&window)?;
        Ok(Self { window, state: State::Cpu(Box::new(state)), backend })
    }

    pub fn backend(&self) -> &Backend {
        &self.backend
    }

    fn try_gpu(window: &Arc<Window>, accept_software: bool) -> Result<(GpuState, Backend), PresentError> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_with_display_handle_from_env(Box::new(window.clone())));
        let surface = instance.create_surface(window.clone()).map_err(|e| err("surface", e))?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            ..Default::default()
        }))
        .map_err(|e| err("adapter", e))?;
        let info = adapter.get_info();
        if info.device_type == wgpu::DeviceType::Cpu && !accept_software {
            return Err(PresentError(format!("adapter de software: {}", info.name)));
        }
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            required_limits: adapter.limits(),
            ..Default::default()
        }))
        .map_err(|e| err("device", e))?;
        let (width, height) = size_of(window);
        let mut config = surface.get_default_config(&adapter, width, height).ok_or_else(|| PresentError("surface sem formato".into()))?;
        // Sem tearing: o editor apresenta com vsync, como o Chromium do VS Code.
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
        .map_err(|e| err("renderer", e))?;
        let blitter = wgpu::util::TextureBlitter::new(&device, config.format);
        let backend = Backend::Gpu { adapter: info.name.clone(), api: format!("{:?}", info.backend) };
        Ok((GpuState { surface, config, device, queue, renderer, scene: vello::Scene::new(), target: None, blitter }, backend))
    }

    fn cpu(window: &Arc<Window>) -> Result<(CpuState, Backend), PresentError> {
        let context = softbuffer::Context::new(window.clone()).map_err(|e| err("softbuffer", e))?;
        let surface = softbuffer::Surface::new(&context, window.clone()).map_err(|e| err("softbuffer", e))?;
        let settings = vello_cpu::RenderSettings::default();
        let threads = settings.num_threads;
        let (width, height) = size_of(window);
        let (w, h) = (clamp16(width), clamp16(height));
        Ok((
            CpuState {
                surface,
                ctx: vello_cpu::RenderContext::new_with(w, h, settings),
                resources: vello_cpu::Resources::new(),
                pixmap: vello_cpu::Pixmap::new(w, h),
            },
            Backend::Cpu { threads },
        ))
    }

    /// Pinta `list` sobre `background` no tamanho atual da janela.
    pub fn present(&mut self, list: &DisplayList, background: Rgba) -> Result<(), PresentError> {
        let (width, height) = size_of(&self.window);
        match &mut self.state {
            State::Gpu(gpu) => present_gpu(gpu, list, background, width, height),
            State::Cpu(cpu) => present_cpu(cpu, list, background, width, height),
        }
    }
}

fn clamp16(v: u32) -> u16 {
    u16::try_from(v).unwrap_or(u16::MAX)
}

fn present_gpu(state: &mut GpuState, list: &DisplayList, background: Rgba, width: u32, height: u32) -> Result<(), PresentError> {
    if state.config.width != width || state.config.height != height {
        state.config.width = width;
        state.config.height = height;
        state.surface.configure(&state.device, &state.config);
        state.target = None;
    }
    // Adquirir antes de pintar: a espera do vsync fica aqui e a pintura cabe no mesmo intervalo (H04).
    let frame = match state.surface.get_current_texture() {
        wgpu::CurrentSurfaceTexture::Success(f) | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
        other => {
            state.surface.configure(&state.device, &state.config);
            return Err(PresentError(format!("surface sem frame: {other:?}")));
        }
    };
    let (_, view) = state.target.get_or_insert_with(|| {
        let texture = state.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("je-render"),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        (texture, view)
    });
    state.scene.reset();
    gpu::encode(list, &mut state.scene);
    state
        .renderer
        .render_to_texture(
            &state.device,
            &state.queue,
            &state.scene,
            view,
            &vello::RenderParams { base_color: background.to_peniko(), width, height, antialiasing_method: vello::AaConfig::Area },
        )
        .map_err(|e| err("render", e))?;
    let target = frame.texture.create_view(&Default::default());
    let mut encoder = state.device.create_command_encoder(&Default::default());
    state.blitter.copy(&state.device, &mut encoder, view, &target);
    state.queue.submit([encoder.finish()]);
    state.queue.present(frame);
    Ok(())
}

fn present_cpu(state: &mut CpuState, list: &DisplayList, background: Rgba, width: u32, height: u32) -> Result<(), PresentError> {
    let (w, h) = (clamp16(width), clamp16(height));
    if state.pixmap.width() != w || state.pixmap.height() != h {
        let settings = vello_cpu::RenderSettings::default();
        state.ctx = vello_cpu::RenderContext::new_with(w, h, settings);
        state.pixmap = vello_cpu::Pixmap::new(w, h);
    }
    let (Some(nw), Some(nh)) = (NonZeroU32::new(u32::from(w)), NonZeroU32::new(u32::from(h))) else {
        return Ok(());
    };
    state.surface.resize(nw, nh).map_err(|e| err("resize", e))?;
    state.ctx.reset();
    state.ctx.set_paint(background.to_peniko());
    state.ctx.fill_rect(&vello::kurbo::Rect::new(0.0, 0.0, f64::from(w), f64::from(h)));
    cpu::encode(list, &mut state.ctx, &mut state.resources);
    state.ctx.flush();
    state.ctx.render(&mut state.pixmap, &mut state.resources);
    let mut buffer = state.surface.buffer_mut().map_err(|e| err("buffer", e))?;
    // O pixmap é RGBA pré-multiplicado; o fundo é opaco, então a cor final já é a não
    // pré-multiplicada. O softbuffer quer 0x00RRGGBB.
    for (dst, src) in buffer.iter_mut().zip(state.pixmap.data()) {
        *dst = (u32::from(src.r) << 16) | (u32::from(src.g) << 8) | u32::from(src.b);
    }
    buffer.present().map_err(|e| err("present", e))
}
