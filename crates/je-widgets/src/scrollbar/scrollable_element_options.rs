//! Porte de `src/vs/base/browser/ui/scrollbar/scrollableElementOptions.ts`: as opções de criação, de
//! alteração e as resolvidas do elemento rolável. Os campos opcionais do TypeScript (`x?: T`) são
//! `Option<T>`; as descrições estão no original.

use je_base::scrollable::ScrollbarVisibility;
use je_ui::NodeId;

#[derive(Clone, Debug, Default)]
pub struct ScrollableElementCreationOptions {
    /// Desenha as barras só em `render_now`, não a cada mudança.
    pub lazy_render: Option<bool>,
    /// Classes extras do elemento rolável.
    pub class_name: Option<String>,
    /// Sombras que indicam que a vista está rolada. Padrão: ligado.
    pub use_shadows: Option<bool>,
    /// Trata a roda do mouse. Padrão: ligado.
    pub handle_mouse_wheel: Option<bool>,
    /// Rolagem suave da roda física. Padrão: ligado.
    pub mouse_wheel_smooth_scroll: Option<bool>,
    /// Inércia da rolagem de touchpad. Padrão: desligado.
    pub inertial_scroll: Option<bool>,
    /// Troca os eixos vertical e horizontal da roda.
    pub flip_axes: Option<bool>,
    /// Converte a rolagem vertical em horizontal.
    pub scroll_y_to_x: Option<bool>,
    /// Consome a roda se alguma barra é necessária, mesmo sem rolar.
    pub consume_mouse_wheel_if_scrollbar_is_needed: Option<bool>,
    /// Consome sempre a roda.
    pub always_consume_mouse_wheel: Option<bool>,
    /// Multiplicador do delta da roda. Padrão: 1.
    pub mouse_wheel_scroll_sensitivity: Option<f64>,
    /// Multiplicador do delta da roda com Alt. Padrão: 5.
    pub fast_scroll_sensitivity: Option<f64>,
    /// Só o eixo dominante rola. Padrão: ligado.
    pub scroll_predominant_axis: Option<bool>,
    /// Tamanho das setas. Padrão: 11.
    pub arrow_size: Option<f64>,
    /// O nó que escuta a roda e o hover. Padrão: o próprio elemento rolável.
    pub listen_on_dom_node: Option<NodeId>,
    pub horizontal: Option<ScrollbarVisibility>,
    pub horizontal_scrollbar_size: Option<f64>,
    pub horizontal_slider_size: Option<f64>,
    pub horizontal_has_arrows: Option<bool>,
    pub vertical: Option<ScrollbarVisibility>,
    pub vertical_scrollbar_size: Option<f64>,
    pub vertical_slider_size: Option<f64>,
    pub vertical_has_arrows: Option<bool>,
    /// O clique na barra rola de página em página.
    pub scroll_by_page: Option<bool>,
}

#[derive(Clone, Debug, Default)]
pub struct ScrollableElementChangeOptions {
    pub handle_mouse_wheel: Option<bool>,
    pub mouse_wheel_scroll_sensitivity: Option<f64>,
    pub fast_scroll_sensitivity: Option<f64>,
    pub scroll_predominant_axis: Option<bool>,
    pub horizontal: Option<ScrollbarVisibility>,
    pub horizontal_scrollbar_size: Option<f64>,
    pub vertical: Option<ScrollbarVisibility>,
    pub vertical_scrollbar_size: Option<f64>,
    pub scroll_by_page: Option<bool>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ScrollableElementResolvedOptions {
    pub lazy_render: bool,
    pub class_name: String,
    pub use_shadows: bool,
    pub handle_mouse_wheel: bool,
    pub flip_axes: bool,
    pub scroll_y_to_x: bool,
    pub consume_mouse_wheel_if_scrollbar_is_needed: bool,
    pub always_consume_mouse_wheel: bool,
    pub mouse_wheel_scroll_sensitivity: f64,
    pub fast_scroll_sensitivity: f64,
    pub scroll_predominant_axis: bool,
    pub mouse_wheel_smooth_scroll: bool,
    pub inertial_scroll: bool,
    pub arrow_size: f64,
    pub listen_on_dom_node: Option<NodeId>,
    pub horizontal: ScrollbarVisibility,
    pub horizontal_scrollbar_size: f64,
    pub horizontal_slider_size: f64,
    pub horizontal_has_arrows: bool,
    pub vertical: ScrollbarVisibility,
    pub vertical_scrollbar_size: f64,
    pub vertical_slider_size: f64,
    pub vertical_has_arrows: bool,
    pub scroll_by_page: bool,
}
