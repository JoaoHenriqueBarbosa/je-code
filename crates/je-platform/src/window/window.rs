//! Porte de `src/vs/platform/window/common/window.ts`, só o que o workbench já usa.
//!
//! Do arquivo original entra apenas `DEFAULT_CUSTOM_TITLEBAR_HEIGHT` (o teste de posição das notificações
//! calcula o deslocamento do topo com ela). Os demais tipos (`IWindowSettings`, `IOpenWindowOptions`,
//! `IPath`...) entram junto com quem os consumir.

/// `DEFAULT_CUSTOM_TITLEBAR_HEIGHT`: altura da barra de título customizada, em pixels, incluindo o espaço da
/// central de comandos.
pub const DEFAULT_CUSTOM_TITLEBAR_HEIGHT: u32 = 35;
