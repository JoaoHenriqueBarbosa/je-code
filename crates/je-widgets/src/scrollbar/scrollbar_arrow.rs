//! Porte de `src/vs/base/browser/ui/scrollbar/scrollbarArrow.ts`: o botão de seta nas pontas da barra.
//! Ativa ao apertar e repete (24 vezes por segundo, depois de 200 ms) enquanto o botão ficar apertado.

use std::rc::Rc;

use je_base::async_::{IntervalTimer, TimeoutTimer};
use je_base::lifecycle::{Disposable, IDisposable};
use je_render::kurbo::Rect;
use je_ui::{Content, EventKind, NodeId, PointerEvents, Role, Ui};

use crate::global_pointer_move_monitor::GlobalPointerMoveMonitor;
use crate::scrollbar::scrollbar_colors;

/// O tamanho da imagem da seta.
pub const ARROW_IMG_SIZE: f64 = 11.0;

pub struct ScrollbarArrowOptions {
    pub on_activate: Rc<dyn Fn()>,
    pub class_name: &'static str,
    /// O id do codicon (o `ThemeIcon`).
    pub icon: &'static str,

    pub bg_width: f64,
    pub bg_height: f64,

    pub top: Option<f64>,
    pub left: Option<f64>,
    pub bottom: Option<f64>,
    pub right: Option<f64>,
}

pub struct ScrollbarArrow {
    base: Rc<Disposable>,
    ui: Ui,
    /// O `bgDomNode`.
    pub bg_dom_node: NodeId,
    /// O `domNode`.
    pub dom_node: NodeId,
}

/// O `top/left/bottom/right` do `style` (os ausentes ficam de fora).
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Edges {
    pub top: Option<f64>,
    pub left: Option<f64>,
    pub bottom: Option<f64>,
    pub right: Option<f64>,
}

/// Posiciona o nó com os lados dados e o tamanho.
pub(crate) fn place(ui: &Ui, node: NodeId, edges: Edges, width: f64, height: f64) {
    let mut tree = ui.tree_mut();
    let (x, y) = (edges.left.unwrap_or(0.0), edges.top.unwrap_or(0.0));
    tree.set_bounds(node, Rect::new(x, y, x + width, y + height));
    tree.set_right(node, edges.right);
    tree.set_bottom(node, edges.bottom);
}

impl ScrollbarArrow {
    /// Cria os dois nós (ainda soltos) e os ouvintes; quem chama os põe na barra.
    pub fn new(ui: &Ui, opts: ScrollbarArrowOptions) -> Rc<Self> {
        let base = Disposable::new();
        let (bg_dom_node, dom_node) = {
            let mut tree = ui.tree_mut();
            (tree.create_detached(Role::Generic), tree.create_detached(Role::Generic))
        };
        // O fundo ocupa a ponta inteira: `top/left/bottom/right: 0px` para os lados dados.
        let edges = Edges { top: opts.top, left: opts.left, bottom: opts.bottom, right: opts.right };
        let zero = Edges { top: opts.top.map(|_| 0.0), left: opts.left.map(|_| 0.0), bottom: opts.bottom.map(|_| 0.0), right: opts.right.map(|_| 0.0) };
        place(ui, bg_dom_node, zero, opts.bg_width, opts.bg_height);
        place(ui, dom_node, edges, ARROW_IMG_SIZE, ARROW_IMG_SIZE);
        {
            let mut tree = ui.tree_mut();
            tree.add_class(bg_dom_node, "arrow-background");
            tree.add_class(dom_node, opts.class_name);
            tree.add_class(dom_node, "codicon");
            tree.add_class(dom_node, &format!("codicon-{}", opts.icon));
            // `.monaco-scrollable-element > .scrollbar > .scra { cursor: pointer; font-size: 11px }`
            tree.style_mut(bg_dom_node).pointer_events = PointerEvents::Auto;
            tree.style_mut(dom_node).cursor = Some(je_ui::Cursor::Pointer);
            tree.style_mut(dom_node).content = Content::Codicon { id: opts.icon.to_owned(), size: 11.0, color: scrollbar_colors().arrow_foreground };
        }

        let monitor = GlobalPointerMoveMonitor::new(ui);
        base.register(monitor.clone());
        let repeat_timer = IntervalTimer::new();
        base.register(repeat_timer.clone());
        let schedule_repeat_timer = TimeoutTimer::new();
        base.register(schedule_repeat_timer.clone());

        for node in [bg_dom_node, dom_node] {
            let on_activate = opts.on_activate.clone();
            let monitor = monitor.clone();
            let repeat_timer = repeat_timer.clone();
            let schedule_repeat_timer = schedule_repeat_timer.clone();
            base.register(ui.listen(node, EventKind::PointerDown, false, move |e| {
                let scheduler_activate = on_activate.clone();
                let scheduler_timer = repeat_timer.clone();
                let schedule_repeater = move || {
                    let activate = scheduler_activate.clone();
                    scheduler_timer.cancel_and_set(move || activate(), 1000 / 24);
                };

                on_activate();
                repeat_timer.cancel();
                schedule_repeat_timer.cancel_and_set(schedule_repeater, 200);

                let (stop_repeat, stop_schedule) = (repeat_timer.clone(), schedule_repeat_timer.clone());
                monitor.start_monitoring(
                    e.target,
                    |_| { /* intencionalmente vazio */ },
                    move || {
                        stop_repeat.cancel();
                        stop_schedule.cancel();
                    },
                );

                e.prevent_default();
            }));
        }

        Rc::new(Self { base, ui: ui.clone(), bg_dom_node, dom_node })
    }
}

impl IDisposable for ScrollbarArrow {
    fn dispose(&self) {
        self.base.dispose();
        // Os nós normalmente já saíram junto com a barra; remover de novo é inofensivo.
        self.ui.remove(self.bg_dom_node);
        self.ui.remove(self.dom_node);
    }

    fn constructor_name(&self) -> &'static str {
        "ScrollbarArrow"
    }
}
