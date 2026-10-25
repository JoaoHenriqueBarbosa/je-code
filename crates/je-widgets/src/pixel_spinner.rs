//! Porte de `src/vs/base/browser/ui/pixelSpinner/pixelSpinner.ts` e `pixelSpinner.css`: o [`PixelSpinner`], um
//! spinner pequeno de pixels (seis pontos de `2px` numa grade de duas colunas por três linhas, ou em anel), com
//! [`create_pixel_spinner`] e [`IPixelSpinnerOptions`].
//!
//! Como o DOM e o CSS viram [`je_ui`]:
//!
//! - `span.monaco-pixel-spinner` (mais `monaco-pixel-spinner-ring` na variante em anel) é o nó de `16px` por `16px`
//!   com `contain: paint` (`clip`); cada `span.monaco-pixel-spinner-dot` é um nó de `2px` com raio `50%` (`1px`),
//!   posto na grade `repeat(2, 2px) / repeat(3, 2px)` com `gap` de `2px`, centrada (`place-content: center`).
//! - O original mostra `aria-hidden="true"` sem `ariaLabel` (nó de papel `Presentation`) e `role="status"` com
//!   `aria-label` com ele (o rótulo do nó). Os atributos vivem no nó: [`PixelSpinner::attribute`] os lê de lá.
//! - A cor é `currentColor`: [`PixelSpinner::set_color`] é o `color` herdado do pai (sem valor, a cor do texto
//!   herdada do botão, [`INHERITED_FOREGROUND`](crate::button)).
//!
//! **Divergência (animação).** O `je-ui` não tem animação de CSS (`@keyframes`, `animation-delay`, `steps()`): o
//! spinner nasce no quadro que o CSS mostra com `prefers-reduced-motion: reduce` (`animation: none`, os pontos
//! com `opacity: 1` e sem `transform`), que é o quadro estático que o próprio CSS define. O
//! `pauseCSSAnimationsWhenHidden` (que só alterna a classe `monaco-pixel-spinner-paused` de um
//! `IntersectionObserver`) não existe porque não há animação a pausar, e a classe nunca é escrita.

use std::cell::Cell;
use std::rc::Rc;

use je_base::lifecycle::IDisposable;
use je_render::Rgba;
use je_render::kurbo::Rect;
use je_ui::{NodeId, Role, Ui};

use crate::button::INHERITED_FOREGROUND;
use crate::dom::{create_detached_el, create_el};

/// A largura e a altura do spinner (`width: 16px; height: 16px`).
pub const PIXEL_SPINNER_SIZE: f64 = 16.0;
/// O lado de um ponto (`width: 2px; height: 2px`) e o `gap` da grade (`column-gap` e `row-gap`).
const DOT_SIZE: f64 = 2.0;
const DOT_GAP: f64 = 2.0;
/// Quantos pontos o spinner tem.
const DOT_COUNT: usize = 6;
/// As colunas da grade (`grid-template-columns: repeat(2, 2px)`).
const GRID_COLUMNS: usize = 2;

/// `variant?: 'grid' | 'ring'`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PixelSpinnerVariant {
    /// Seis pontos numa grade de duas colunas por três linhas que descem em cascata.
    #[default]
    Grid,
    /// Seis pontos em círculo, com um destaque que orbita o anel.
    Ring,
}

/// `IPixelSpinnerOptions`.
#[derive(Clone, Debug, Default)]
pub struct IPixelSpinnerOptions {
    /// Rótulo de acessibilidade: com ele o spinner é `role="status"`, sem ele é decorativo (`aria-hidden`).
    pub aria_label: Option<String>,
    pub variant: PixelSpinnerVariant,
}

/// `IPixelSpinner`: o spinner e o elemento raiz.
pub struct PixelSpinner {
    ui: Ui,
    element: NodeId,
    dots: Vec<NodeId>,
    disposed: Cell<bool>,
}

impl PixelSpinner {
    /// `element`: o `span.monaco-pixel-spinner`.
    pub fn element(&self) -> NodeId {
        self.element
    }

    /// Os seis `span.monaco-pixel-spinner-dot`, em ordem.
    pub fn dots(&self) -> &[NodeId] {
        &self.dots
    }

    /// `getAttribute`: `aria-hidden`, ou `role` e `aria-label`.
    pub fn attribute(&self, name: &str) -> Option<String> {
        self.ui.tree().attribute(self.element, name).map(str::to_owned)
    }

    /// `style.color`: a cor dos pontos (`currentColor`).
    pub fn set_color(&self, color: Rgba) {
        let mut tree = self.ui.tree_mut();
        for dot in &self.dots {
            tree.style_mut(*dot).background = Some(color);
        }
    }
}

impl IDisposable for PixelSpinner {
    fn dispose(&self) {
        if self.disposed.replace(true) {
            return;
        }
        self.ui.remove(self.element);
    }

    fn constructor_name(&self) -> &'static str {
        "PixelSpinner"
    }
}

/// `createPixelSpinner(parent?, options?)`: o spinner, anexado a `parent` se houver.
pub fn create_pixel_spinner(ui: &Ui, parent: Option<NodeId>, options: Option<IPixelSpinnerOptions>) -> Rc<PixelSpinner> {
    let options = options.unwrap_or_default();
    let root_class = if options.variant == PixelSpinnerVariant::Ring { "monaco-pixel-spinner monaco-pixel-spinner-ring" } else { "monaco-pixel-spinner" };
    let decorative = options.aria_label.as_deref().is_none_or(str::is_empty);
    let (element, dots) = {
        let mut tree = ui.tree_mut();
        let element = create_detached_el(&mut tree, if decorative { Role::Presentation } else { Role::Generic }, root_class);
        let origin_x = (PIXEL_SPINNER_SIZE - (GRID_COLUMNS as f64 * DOT_SIZE + (GRID_COLUMNS as f64 - 1.0) * DOT_GAP)) / 2.0;
        let rows = DOT_COUNT / GRID_COLUMNS;
        let origin_y = (PIXEL_SPINNER_SIZE - (rows as f64 * DOT_SIZE + (rows as f64 - 1.0) * DOT_GAP)) / 2.0;
        tree.set_bounds(element, Rect::new(0.0, 0.0, PIXEL_SPINNER_SIZE, PIXEL_SPINNER_SIZE));
        {
            let style = tree.style_mut(element);
            style.clip = true; // `contain: paint`
            style.pointer_events = je_ui::PointerEvents::None;
        }
        let mut dots = Vec::with_capacity(DOT_COUNT);
        for index in 0..DOT_COUNT {
            let dot = create_el(&mut tree, element, Role::Generic, "monaco-pixel-spinner-dot");
            let (column, row) = ((index % GRID_COLUMNS) as f64, (index / GRID_COLUMNS) as f64);
            let (x, y) = (origin_x + column * (DOT_SIZE + DOT_GAP), origin_y + row * (DOT_SIZE + DOT_GAP));
            tree.set_bounds(dot, Rect::new(x, y, x + DOT_SIZE, y + DOT_SIZE));
            let style = tree.style_mut(dot);
            style.radius = DOT_SIZE / 2.0; // `border-radius: 50%`
            style.background = Some(INHERITED_FOREGROUND);
            style.opacity = 1.0; // o quadro de `prefers-reduced-motion: reduce`
            dots.push(dot);
        }
        match options.aria_label.as_deref().filter(|label| !label.is_empty()) {
            Some(label) => {
                tree.set_attribute(element, "role", "status");
                tree.set_attribute(element, "aria-label", label);
            }
            None => tree.set_attribute(element, "aria-hidden", "true"),
        }
        (element, dots)
    };
    if let Some(parent) = parent {
        ui.tree_mut().append_child(parent, element);
    }
    Rc::new(PixelSpinner { ui: ui.clone(), element, dots, disposed: Cell::new(false) })
}
