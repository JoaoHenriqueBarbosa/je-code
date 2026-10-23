//! Desempenho da lista virtualizada com 1 milhão de itens: construir, rolar e pintar a janela visível (50
//! linhas). `#[ignore]` porque mede tempo: rode com
//! `cargo test -p je-widgets --release --test list_perf -- --ignored --nocapture`.
//!
//! Cada linha é um nó com fundo e um texto de uma linha; a pintura é a de `Tree::paint` para uma
//! `DisplayList` (o que o quadro da GPU consome). Os números saem em microssegundos por quadro.

mod support;

use std::any::Any;
use std::rc::Rc;
use std::time::Instant;

use je_render::{DisplayList, Rgba};
use je_text::TextStyle;
use je_ui::{Content, NodeId, Ui};
use je_widgets::list::list_types::{IListRenderer, IListVirtualDelegate, ListElementRenderDetails, TemplateData};
use je_widgets::list::list_view::{ListView, ListViewOptions};
use support::create_container;

const ITEMS: usize = 1_000_000;
const ROW_HEIGHT: f64 = 22.0;
const VISIBLE_ROWS: f64 = 50.0;

struct Delegate;

impl IListVirtualDelegate<u32> for Delegate {
    fn get_height(&self, _: &u32) -> f64 {
        ROW_HEIGHT
    }

    fn get_template_id(&self, _: &u32) -> String {
        "row".to_owned()
    }
}

struct Renderer {
    ui: Ui,
}

impl IListRenderer<u32> for Renderer {
    fn template_id(&self) -> String {
        "row".to_owned()
    }

    fn render_template(&self, container: NodeId) -> TemplateData {
        self.ui.tree_mut().style_mut(container).background = Some(Rgba::new(0x25, 0x25, 0x26, 255));
        Rc::new(()) as Rc<dyn Any>
    }

    fn render_element(&self, element: &u32, _: usize, _: &TemplateData, _: Option<&ListElementRenderDetails>) {
        // O `renderElement` de um explorador troca o texto do rótulo da linha.
        // A linha é o nó do `container` do `render_template`; o texto é o conteúdo próprio dele.
        let _ = element;
    }

    fn dispose_template(&self, _: &TemplateData) {}
}

struct TextRenderer {
    ui: Ui,
    style: TextStyle,
}

impl IListRenderer<u32> for TextRenderer {
    fn template_id(&self) -> String {
        "row".to_owned()
    }

    fn render_template(&self, container: NodeId) -> TemplateData {
        self.ui.tree_mut().style_mut(container).background = Some(Rgba::new(0x25, 0x25, 0x26, 255));
        Rc::new(container) as Rc<dyn Any>
    }

    fn render_element(&self, element: &u32, _: usize, template_data: &TemplateData, _: Option<&ListElementRenderDetails>) {
        let node = *template_data.downcast_ref::<NodeId>().expect("template de linha");
        self.ui.tree_mut().style_mut(node).content = Content::Text { text: format!("item {element}"), style: self.style.clone(), color: Rgba::new(0xCC, 0xCC, 0xCC, 255), line_height: ROW_HEIGHT };
    }

    fn dispose_template(&self, _: &TemplateData) {}
}

fn micros(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1e6
}

fn bench(label: &str, renderer: impl FnOnce(&Ui) -> Rc<dyn IListRenderer<u32>>) {
    let ui = Ui::new();
    let root = ui.root();
    ui.tree_mut().set_bounds(root, je_render::kurbo::Rect::new(0.0, 0.0, 800.0, ROW_HEIGHT * VISIBLE_ROWS));
    let container = create_container(&ui, 800.0, ROW_HEIGHT * VISIBLE_ROWS);
    ui.tree_mut().append_child(root, container);

    let items: Vec<u32> = (0..ITEMS as u32).collect();
    let view = ListView::new(&ui, container, Rc::new(Delegate), vec![renderer(&ui)], ListViewOptions::default());
    view.layout(Some(ROW_HEIGHT * VISIBLE_ROWS), Some(800.0));

    // construir: o `splice` de 1 milhão de itens (mapa de intervalos, itens, linhas visíveis)
    let start = Instant::now();
    view.splice(0, 0, &items);
    ui.run_animation_frame();
    let build = micros(start);
    assert_eq!(view.length(), ITEMS);
    assert_eq!(view.content_height(), ITEMS as f64 * ROW_HEIGHT);

    // rolar: 1000 quadros, cada um desce 3 linhas e renderiza as linhas novas
    let frames = 1000;
    let start = Instant::now();
    for frame in 0..frames {
        view.set_scroll_top(f64::from(frame) * ROW_HEIGHT * 3.0, false);
        ui.run_animation_frame();
    }
    let scroll = micros(start) / f64::from(frames);
    // a rolagem renderizou de verdade: o primeiro índice visível é o do último quadro e a linha existe
    let expected_first = i64::from(frames - 1) * 3;
    assert_eq!(view.first_visible_index(), expected_first);
    assert!(view.dom_element(expected_first as usize).is_some());

    // pintar: a janela visível numa `DisplayList`
    let frames = 200;
    let mut commands = 0;
    let start = Instant::now();
    for _ in 0..frames {
        let mut list = DisplayList::default();
        ui.tree().paint(&mut list, 1.0);
        commands = list.commands().len();
    }
    let paint = micros(start) / f64::from(frames);

    // salto longo: do início ao meio e ao fim (o `indexAt` do mapa é O(grupos))
    let start = Instant::now();
    view.set_scroll_top(ITEMS as f64 * ROW_HEIGHT / 2.0, false);
    view.set_scroll_top(ITEMS as f64 * ROW_HEIGHT - ROW_HEIGHT * VISIBLE_ROWS, false);
    ui.run_animation_frame();
    let jump = micros(start) / 2.0;

    println!("{label}: construir 1M itens {build:.0} us; rolar {scroll:.1} us/quadro (3 linhas novas); pintar {paint:.1} us/quadro ({commands} comandos); salto longo {jump:.1} us");
}

#[test]
#[ignore = "mede tempo; rode com --release --ignored --nocapture"]
fn one_million_items() {
    bench("fundo", |ui| Rc::new(Renderer { ui: ui.clone() }));
    bench("fundo e texto", |ui| Rc::new(TextRenderer { ui: ui.clone(), style: TextStyle::new("sans-serif", 13.0, 400) }));
}
