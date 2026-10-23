//! Porte de `src/vs/base/browser/ui/scrollbar/scrollbarState.ts`: o cálculo do tamanho e da posição do
//! slider a partir do tamanho visível, do tamanho rolável e da posição de rolagem.
//!
//! Todos os números são `f64` (o `number` do JavaScript) e o arredondamento é o `Math.round` do JS
//! (metade para cima), não o `f64::round` do Rust (metade para longe do zero).

use crate::common::js_round;

/// O menor slider que dá para agarrar com o mouse.
const MINIMUM_SLIDER_SIZE: f64 = 20.0;

/// O estado de uma barra de rolagem. Como no original, os tamanhos e a posição são guardados como o
/// chamador os passa no construtor e arredondados nos `set_*`.
#[derive(Clone, Debug)]
pub struct ScrollbarState {
    /// Espessura da barra (a largura da vertical, a altura da horizontal).
    scrollbar_size: f64,
    /// Espessura da barra da outra direção, para descontar o canto que ela ocupa.
    opposite_scrollbar_size: f64,
    /// Tamanho das setas nas pontas (0 sem setas).
    arrow_size: f64,

    visible_size: f64,
    scroll_size: f64,
    scroll_position: f64,

    computed_available_size: f64,
    computed_is_needed: bool,
    computed_slider_size: f64,
    computed_slider_ratio: f64,
    computed_slider_position: f64,
}

struct Computed {
    available_size: f64,
    is_needed: bool,
    slider_size: f64,
    slider_ratio: f64,
    slider_position: f64,
}

impl ScrollbarState {
    pub fn new(arrow_size: f64, scrollbar_size: f64, opposite_scrollbar_size: f64, visible_size: f64, scroll_size: f64, scroll_position: f64) -> Self {
        let mut state = Self {
            scrollbar_size: js_round(scrollbar_size),
            opposite_scrollbar_size: js_round(opposite_scrollbar_size),
            arrow_size: js_round(arrow_size),
            visible_size,
            scroll_size,
            scroll_position,
            computed_available_size: 0.0,
            computed_is_needed: false,
            computed_slider_size: 0.0,
            computed_slider_ratio: 0.0,
            computed_slider_position: 0.0,
        };
        state.refresh_computed_values();
        state
    }

    /// Devolve `true` se o valor mudou.
    pub fn set_visible_size(&mut self, visible_size: f64) -> bool {
        let rounded = js_round(visible_size);
        if self.visible_size != rounded {
            self.visible_size = rounded;
            self.refresh_computed_values();
            return true;
        }
        false
    }

    /// Devolve `true` se o valor mudou.
    pub fn set_scroll_size(&mut self, scroll_size: f64) -> bool {
        let rounded = js_round(scroll_size);
        if self.scroll_size != rounded {
            self.scroll_size = rounded;
            self.refresh_computed_values();
            return true;
        }
        false
    }

    /// Devolve `true` se o valor mudou.
    pub fn set_scroll_position(&mut self, scroll_position: f64) -> bool {
        let rounded = js_round(scroll_position);
        if self.scroll_position != rounded {
            self.scroll_position = rounded;
            self.refresh_computed_values();
            return true;
        }
        false
    }

    pub fn set_scrollbar_size(&mut self, scrollbar_size: f64) {
        self.scrollbar_size = js_round(scrollbar_size);
    }

    pub fn set_opposite_scrollbar_size(&mut self, opposite_scrollbar_size: f64) {
        self.opposite_scrollbar_size = js_round(opposite_scrollbar_size);
    }

    fn compute_values(opposite_scrollbar_size: f64, arrow_size: f64, visible_size: f64, scroll_size: f64, scroll_position: f64) -> Computed {
        let available_size = f64::max(0.0, visible_size - opposite_scrollbar_size);
        let representable_size = f64::max(0.0, available_size - 2.0 * arrow_size);
        let is_needed = scroll_size > 0.0 && scroll_size > visible_size;

        if !is_needed {
            // não precisa de slider
            return Computed { available_size: js_round(available_size), is_needed, slider_size: js_round(representable_size), slider_ratio: 0.0, slider_position: 0.0 };
        }

        // O slider é aumentado de propósito quando seria pequeno demais para agarrar com o mouse.
        let slider_size = js_round(f64::max(MINIMUM_SLIDER_SIZE, (visible_size * representable_size / scroll_size).floor()));

        // O slider anda de 0 a `representable_size - slider_size`, assim como `scroll_position` anda de
        // 0 a `scroll_size - visible_size`.
        let slider_ratio = (representable_size - slider_size) / (scroll_size - visible_size);
        let slider_position = scroll_position * slider_ratio;

        Computed { available_size: js_round(available_size), is_needed, slider_size: js_round(slider_size), slider_ratio, slider_position: js_round(slider_position) }
    }

    fn refresh_computed_values(&mut self) {
        let r = Self::compute_values(self.opposite_scrollbar_size, self.arrow_size, self.visible_size, self.scroll_size, self.scroll_position);
        self.computed_available_size = r.available_size;
        self.computed_is_needed = r.is_needed;
        self.computed_slider_size = r.slider_size;
        self.computed_slider_ratio = r.slider_ratio;
        self.computed_slider_position = r.slider_position;
    }

    pub fn get_arrow_size(&self) -> f64 {
        self.arrow_size
    }

    pub fn get_scroll_position(&self) -> f64 {
        self.scroll_position
    }

    pub fn get_rectangle_large_size(&self) -> f64 {
        self.computed_available_size
    }

    pub fn get_rectangle_small_size(&self) -> f64 {
        self.scrollbar_size
    }

    pub fn is_needed(&self) -> bool {
        self.computed_is_needed
    }

    pub fn get_slider_size(&self) -> f64 {
        self.computed_slider_size
    }

    pub fn get_slider_position(&self) -> f64 {
        self.computed_slider_position
    }

    /// A posição de rolagem que leva o centro do slider até `offset` (a distância ao início da barra).
    pub fn get_desired_scroll_position_from_offset(&self, offset: f64) -> f64 {
        if !self.computed_is_needed {
            // não precisa de slider
            return 0.0;
        }
        let desired_slider_position = offset - self.arrow_size - self.computed_slider_size / 2.0;
        js_round(desired_slider_position / self.computed_slider_ratio)
    }

    /// Como [`Self::get_desired_scroll_position_from_offset`], mas de página em página: o clique antes do
    /// slider sobe uma página, o clique depois desce uma.
    pub fn get_desired_scroll_position_from_offset_paged(&self, offset: f64) -> f64 {
        if !self.computed_is_needed {
            // não precisa de slider
            return 0.0;
        }
        let corrected_offset = offset - self.arrow_size; // compensa as setas
        let mut desired_scroll_position = self.scroll_position;
        if corrected_offset < self.computed_slider_position {
            desired_scroll_position -= self.visible_size; // página para cima ou para a esquerda
        } else {
            desired_scroll_position += self.visible_size; // página para baixo ou para a direita
        }
        desired_scroll_position
    }

    /// A posição de rolagem depois de arrastar o slider `delta` pixels.
    pub fn get_desired_scroll_position_from_delta(&self, delta: f64) -> f64 {
        if !self.computed_is_needed {
            // não precisa de slider
            return 0.0;
        }
        let desired_slider_position = self.computed_slider_position + delta;
        js_round(desired_slider_position / self.computed_slider_ratio)
    }
}
