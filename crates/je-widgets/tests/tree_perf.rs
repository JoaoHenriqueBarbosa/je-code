//! Desempenho da árvore virtualizada com 200 mil nós (um explorador de arquivos grande): 10 mil pastas, todas
//! expandidas, com 19 arquivos cada. Mede construir, rolar e pintar a janela visível (50 linhas), o
//! `expand`/`collapse` de uma pasta no meio e o `set_children` (o `refresh` do `ObjectTree`) de uma pasta com
//! 10 mil filhos. `#[ignore]` porque mede tempo: rode com
//! `cargo test -p je-widgets --release --test tree_perf -- --ignored --nocapture`.
//!
//! Teste novo (o VS Code não tem este benchmark). A pintura é a de `Tree::paint` para uma `DisplayList` (o
//! que o quadro da GPU consome). Os números saem em microssegundos.

mod support;

use std::any::Any;
use std::rc::Rc;
use std::time::Instant;

use je_render::{DisplayList, Rgba};
use je_text::TextStyle;
use je_ui::{Content, NodeId, Ui};
use je_widgets::list::list_types::{IListVirtualDelegate, TemplateData};
use je_widgets::tree::object_tree::{ObjectTree, ObjectTreeMethods, ObjectTreeOptions};
use je_widgets::tree::object_tree_model::{ObjectTreeModel, ObjectTreeModelSetChildrenOptions};
use je_widgets::tree::tree_types::{IObjectTreeElement, ITreeRenderer, TreeElementRenderDetails};
use support::create_container;

const FOLDERS: u32 = 10_000;
const FILES_PER_FOLDER: u32 = 19;
const ROW_HEIGHT: f64 = 22.0;
const VISIBLE_ROWS: f64 = 50.0;
const BIG_FOLDER: u32 = 10_000;

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
    style: TextStyle,
}

impl ITreeRenderer<ObjectTreeModel<u32>> for Renderer {
    fn template_id(&self) -> String {
        "row".to_owned()
    }

    fn render_template(&self, container: NodeId) -> TemplateData {
        self.ui.tree_mut().style_mut(container).background = Some(Rgba::new(0x25, 0x25, 0x26, 255));
        Rc::new(container) as Rc<dyn Any>
    }

    fn render_element(&self, node: &Rc<je_widgets::tree::index_tree_model::TreeNode<Option<u32>>>, _: usize, template_data: &TemplateData, _: Option<&TreeElementRenderDetails>) {
        let container = *template_data.downcast_ref::<NodeId>().expect("template de linha");
        self.ui.tree_mut().style_mut(container).content =
            Content::Text { text: format!("item {}", node.element.expect("elemento")), style: self.style.clone(), color: Rgba::new(0xCC, 0xCC, 0xCC, 255), line_height: ROW_HEIGHT };
    }

    fn dispose_template(&self, _: &TemplateData) {}
}

fn micros(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1e6
}

fn folder(index: u32) -> IObjectTreeElement<u32> {
    let base = index * (FILES_PER_FOLDER + 1);
    IObjectTreeElement::with_children(base, (1..=FILES_PER_FOLDER).map(|i| IObjectTreeElement::new(base + i)).collect())
}

#[test]
#[ignore = "mede tempo: rode em --release com --ignored --nocapture"]
fn tree_perf_200k_nodes() {
    let ui = Ui::new();
    let root = ui.root();
    ui.tree_mut().set_bounds(root, je_render::kurbo::Rect::new(0.0, 0.0, 800.0, ROW_HEIGHT * VISIBLE_ROWS));
    let container = create_container(&ui, 800.0, ROW_HEIGHT * VISIBLE_ROWS);
    ui.tree_mut().append_child(root, container);

    let renderer = Renderer { ui: ui.clone(), style: TextStyle::new("sans-serif", 13.0, 400) };
    let tree = ObjectTree::<u32>::new("perf", &ui, container, Rc::new(Delegate), vec![Rc::new(renderer)], ObjectTreeOptions::default());
    tree.layout(Some(ROW_HEIGHT * VISIBLE_ROWS), Some(800.0));

    // construir: o `setChildren` de 200 mil nós, 10 mil pastas expandidas (o modelo e a lista)
    let children: Vec<IObjectTreeElement<u32>> = (0..FOLDERS).map(folder).collect();
    let start = Instant::now();
    tree.set_children(&None, children, ObjectTreeModelSetChildrenOptions::default());
    ui.run_animation_frame();
    let build = micros(start);
    let total = FOLDERS * (FILES_PER_FOLDER + 1);
    assert_eq!(tree.get_list_render_count(&None), total as usize);

    // rolar: 1000 quadros, cada um desce 3 linhas e renderiza as linhas novas
    let frames = 1000;
    let start = Instant::now();
    for frame in 0..frames {
        tree.set_scroll_top(f64::from(frame) * ROW_HEIGHT * 3.0);
        ui.run_animation_frame();
    }
    let scroll = micros(start) / f64::from(frames);
    assert_eq!(tree.first_visible_element(), Some(Some((frames as u32 - 1) * 3)));

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

    // recolher e expandir uma pasta do meio, com a janela rolada até lá
    let middle = Some((FOLDERS / 2) * (FILES_PER_FOLDER + 1));
    let start = Instant::now();
    tree.collapse(&middle, false);
    ui.run_animation_frame();
    let collapse = micros(start);
    let start = Instant::now();
    tree.expand(&middle, false);
    ui.run_animation_frame();
    let expand = micros(start);

    // o `refresh` de uma pasta com 10 mil filhos: `setChildren` do elemento com a lista nova
    let target = Some(0);
    let big: Vec<IObjectTreeElement<u32>> = (0..BIG_FOLDER).map(|i| IObjectTreeElement::new(total + i)).collect();
    let start = Instant::now();
    tree.set_children(&target, big, ObjectTreeModelSetChildrenOptions::default());
    ui.run_animation_frame();
    let refresh = micros(start);
    let big: Vec<IObjectTreeElement<u32>> = (0..BIG_FOLDER).map(|i| IObjectTreeElement::new(total + i)).collect();
    let start = Instant::now();
    tree.set_children(&target, big, ObjectTreeModelSetChildrenOptions::default());
    ui.run_animation_frame();
    let refresh_same = micros(start);

    println!(
        "tree 200k nos (10k pastas expandidas): construir {build:.0} us; rolar {scroll:.1} us/quadro (3 linhas novas); pintar {paint:.1} us/quadro ({commands} comandos); \
         recolher pasta {collapse:.0} us; expandir pasta {expand:.0} us; refresh de pasta com 10k filhos {refresh:.0} us (de novo, mesmos elementos {refresh_same:.0} us)"
    );
}
