//! WSL: a GPU chega ao Linux pelo Mesa `d3d12` sobre `/dev/dxg`, mas o Mesa do Debian 13 não
//! escolhe esse driver sozinho e cai no llvmpipe (H01). A correção é ambiente na partida:
//! `GALLIUM_DRIVER=d3d12` e o backend GL do wgpu (o Vulkan disponível ali é só o lavapipe).
//!
//! Ambiente não se muda no próprio processo sem `unsafe` (`set_var` é `unsafe` na edição 2024),
//! então o executável se relança com o ambiente certo (H27).

use std::ffi::OsString;
use std::path::Path;

const DXG: &str = "/dev/dxg";
const DRI_DIRS: [&str; 3] = ["/usr/lib/x86_64-linux-gnu/dri", "/usr/lib/aarch64-linux-gnu/dri", "/usr/lib/dri"];

/// O ambiente que falta para a GPU do WSL chegar ao wgpu, ou `None` quando não há o que fazer
/// (fora do WSL, sem o driver `d3d12` instalado, ou com o ambiente já definido pelo user).
pub fn missing_gpu_environment() -> Option<Vec<(&'static str, &'static str)>> {
    if !Path::new(DXG).exists() || !DRI_DIRS.iter().any(|d| Path::new(d).join("d3d12_dri.so").exists()) {
        return None;
    }
    if std::env::var_os("GALLIUM_DRIVER").is_some() || std::env::var_os("JE_NO_WSL_GPU").is_some() {
        return None;
    }
    let mut vars = vec![("GALLIUM_DRIVER", "d3d12")];
    if std::env::var_os("WGPU_BACKEND").is_none() {
        vars.push(("WGPU_BACKEND", "gl"));
    }
    Some(vars)
}

/// Relança o executável atual com o ambiente da GPU do WSL, substituindo o processo. Volta só se
/// não há o que fazer ou se o relançamento falhou (aí a partida segue no caminho de CPU).
#[cfg(unix)]
pub fn relaunch_with_gpu() -> Option<std::io::Error> {
    use std::os::unix::process::CommandExt;
    let vars = missing_gpu_environment()?;
    let exe = std::env::current_exe().ok()?;
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    Some(std::process::Command::new(exe).args(args).envs(vars).exec())
}

#[cfg(not(unix))]
pub fn relaunch_with_gpu() -> Option<std::io::Error> {
    None
}

/// No WSLg, o X11 (XWayland) entrega 60 fps com vsync onde o Wayland do compositor RDP fica em 53 a 59
/// (H04, H02: o próprio clear sem pintura fica em 58,9 fps no Wayland). Dentro do WSL, com `DISPLAY`
/// definido, a janela vai pelo X11, salvo `JE_WSL_WAYLAND=1`.
pub fn prefer_x11() -> bool {
    Path::new(DXG).exists() && std::env::var_os("DISPLAY").is_some() && std::env::var_os("JE_WSL_WAYLAND").is_none()
}
