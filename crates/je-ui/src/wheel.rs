//! A roda do mouse: o `WheelEvent` do DOM e o `StandardWheelEvent` de `src/vs/base/browser/mouseEvent.ts`,
//! que normaliza os deltas para "entalhes da roda" (um entalhe vale 1).
//!
//! A janela nativa entrega a roda de duas formas: em linhas (a roda física, `DeltaMode::Line`, um entalhe
//! vale uma linha) ou em pixels (o touchpad, `DeltaMode::Pixel`). O sinal segue o `WheelEvent` do DOM:
//! `delta_y` positivo é rolar para baixo. Os campos legados `wheel_delta_*` (o `wheelDeltaX`/`wheelDeltaY`
//! do WebKit e do Chromium, em múltiplos de 120 por entalhe e com o sinal oposto) só existem para quem os
//! tem; sem eles vale a regra moderna do `deltaMode`, que é o caso do porte nativo.
//!
//! Divergências do original, todas por não haver navegador: o ramo do Firefox fora do macOS
//! (`-deltaY / 3` em linhas) e o fator do `devicePixelRatio` do Chrome até a versão 122 não existem, porque
//! o motor nativo reporta os deltas já no formato do Chrome atual; o ramo do Gecko (`axis`/`detail`) idem.

use crate::events::Modifiers;
use crate::tree::NodeId;

/// O `deltaMode` do `WheelEvent`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DeltaMode {
    /// `DOM_DELTA_PIXEL`: os deltas são pixels.
    #[default]
    Pixel,
    /// `DOM_DELTA_LINE`: os deltas são linhas.
    Line,
    /// `DOM_DELTA_PAGE`: os deltas são páginas.
    Page,
}

/// Os dados de um evento de roda do mouse. `x` e `y` são coordenadas da janela.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WheelData {
    pub x: f64,
    pub y: f64,
    pub delta_x: f64,
    pub delta_y: f64,
    pub delta_mode: DeltaMode,
    pub modifiers: Modifiers,
    /// O `wheelDeltaX` legado, se o sistema o informa.
    pub wheel_delta_x: Option<f64>,
    /// O `wheelDeltaY` legado, se o sistema o informa.
    pub wheel_delta_y: Option<f64>,
    /// O `wheelDelta` legado, usado só quando todo o resto resulta em zero.
    pub wheel_delta: Option<f64>,
}

impl WheelData {
    /// Uma roda moderna, só com `deltaX`, `deltaY` e `deltaMode`.
    pub fn new(x: f64, y: f64, delta_x: f64, delta_y: f64, delta_mode: DeltaMode) -> Self {
        Self { x, y, delta_x, delta_y, delta_mode, ..Self::default() }
    }
}

/// O `StandardWheelEvent`: os deltas em entalhes da roda (positivo é rolar para cima e para a esquerda,
/// como no original), mais o evento de origem.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StandardWheelEvent {
    pub delta_y: f64,
    pub delta_x: f64,
    /// O evento de origem (o `browserEvent`), nulo quando o evento é sintético (os botões de seta).
    pub browser_event: Option<WheelData>,
    /// O `browserEvent.defaultPrevented` no momento da criação.
    pub default_prevented: bool,
    /// O `target`.
    pub target: Option<NodeId>,
}

impl StandardWheelEvent {
    /// `new StandardWheelEvent(null, deltaX, deltaY)`: um evento sintético.
    pub fn synthetic(delta_x: f64, delta_y: f64) -> Self {
        Self { delta_y, delta_x, browser_event: None, default_prevented: false, target: None }
    }

    /// `new StandardWheelEvent(e)`.
    pub fn new(e: &WheelData, target: Option<NodeId>, default_prevented: bool) -> Self {
        // vertical: o legado do WebKit, senão o evento moderno `wheel`
        let mut delta_y = match e.wheel_delta_y {
            Some(wheel_delta_y) => wheel_delta_y / 120.0,
            None if e.delta_mode == DeltaMode::Line => -e.delta_y,
            None => -e.delta_y / 40.0,
        };
        // horizontal
        let delta_x = match e.wheel_delta_x {
            Some(wheel_delta_x) => wheel_delta_x / 120.0,
            None if e.delta_mode == DeltaMode::Line => -e.delta_x,
            None => -e.delta_x / 40.0,
        };

        // assume uma rolagem vertical se nada funcionou
        if delta_y == 0.0
            && delta_x == 0.0
            && let Some(wheel_delta) = e.wheel_delta.filter(|d| *d != 0.0)
        {
            delta_y = wheel_delta / 120.0;
        }
        Self { delta_y, delta_x, browser_event: Some(*e), default_prevented, target }
    }
}
