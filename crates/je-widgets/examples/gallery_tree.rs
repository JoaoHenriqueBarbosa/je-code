//! Galeria visual da `ObjectTree` com a pintura da lista e da árvore: pasta expandida e colapsada, arquivos,
//! uma linha selecionada e focada, uma linha sob o ponteiro, e a mesma árvore com a lista em foco e sem foco.
//! Grava `wip/scratch/gallery/tree-dark.png` e `tree-light.png`.
#[path = "support/mod.rs"]
mod support;

use std::any::Any;
use std::rc::Rc;

use je_render::Rgba;
use je_render::kurbo::Rect;
use je_theme::ColorTheme;
use je_ui::{NodeId, PointerInput, PointerInputKind, Role, Ui};
use je_widgets::list::list_styles::default_list_styles;
use je_widgets::list::list_types::{IListVirtualDelegate, TemplateData};
use je_widgets::text::{set_node_text, ui_font};
use je_widgets::tree::index_tree_model::TreeNode;
use je_widgets::tree::object_tree::{ObjectTree, ObjectTreeOptions};
use je_widgets::tree::object_tree_model::{ObjectTreeModel, ObjectTreeModelSetChildrenOptions};
use je_widgets::tree::tree_types::{IObjectTreeElement, ITreeRenderer, TreeElementRenderDetails};

const WIDTH: u16 = 700;
const HEIGHT: u16 = 330;
const ROW_HEIGHT: f64 = 22.0;
const PANEL_WIDTH: f64 = 300.0;
const PANEL_HEIGHT: f64 = 242.0;

type Model = ObjectTreeModel<i64>;

/// Os nomes dos elementos, pelo id.
const NAMES: [&str; 10] = ["src", "main.rs", "lib.rs", "widgets", "button.rs", "target", "Cargo.toml", "README.md", "LICENSE", "debug"];

struct Delegate;

impl IListVirtualDelegate<i64> for Delegate {
    fn get_height(&self, _: &i64) -> f64 {
        ROW_HEIGHT
    }

    fn get_template_id(&self, _: &i64) -> String {
        "file".to_owned()
    }
}

/// O renderer da galeria: escreve o nome do arquivo no nó de conteúdo da linha.
struct FileRenderer {
    ui: Ui,
    foreground: Rgba,
}

impl ITreeRenderer<Model> for FileRenderer {
    fn template_id(&self) -> String {
        "file".to_owned()
    }

    fn render_template(&self, container: NodeId) -> TemplateData {
        Rc::new(container) as Rc<dyn Any>
    }

    fn render_element(&self, node: &Rc<TreeNode<Option<i64>>>, _index: usize, template_data: &TemplateData, _details: Option<&TreeElementRenderDetails>) {
        let contents = *template_data.downcast_ref::<NodeId>().expect("template");
        let name = NAMES[usize::try_from(node.element.expect("elemento")).expect("id")];
        set_node_text(&mut self.ui.tree_mut(), contents, name, &ui_font(13.0, 400), self.foreground, ROW_HEIGHT);
    }

    fn dispose_template(&self, _: &TemplateData) {}
}

fn el(id: i64) -> IObjectTreeElement<i64> {
    IObjectTreeElement::new(id)
}

fn folder(id: i64, collapsed: bool, children: Vec<IObjectTreeElement<i64>>) -> IObjectTreeElement<i64> {
    IObjectTreeElement::with_children(id, children).collapsed(collapsed)
}

/// Um painel com a árvore. `focused` põe o foco do DOM na lista (o `:focus` do `.monaco-list`); `hover_row` é
/// o índice da linha sob o ponteiro.
fn panel(x: f64, theme: &ColorTheme, focused: bool, hover_row: usize, caption: &str) -> (Ui, Rc<ObjectTree<i64>>) {
    let ui = Ui::new();
    let root = ui.root();
    ui.tree_mut().set_bounds(root, Rect::new(0.0, 0.0, f64::from(WIDTH), f64::from(HEIGHT)));
    let foreground = theme.get_color("foreground").expect("foreground");

    {
        let mut tree = ui.tree_mut();
        let node = tree.create(root, Role::Generic);
        let width = set_node_text(&mut tree, node, caption, &ui_font(12.0, 400), foreground, 18.0);
        tree.set_bounds(node, Rect::new(x, 12.0, x + width, 30.0));
    }

    let container = {
        let mut tree = ui.tree_mut();
        let node = tree.create(root, Role::Generic);
        tree.set_bounds(node, Rect::new(x, 40.0, x + PANEL_WIDTH, 40.0 + PANEL_HEIGHT));
        tree.style_mut(node).background = theme.get_color("sideBar.background");
        node
    };

    let renderer = Rc::new(FileRenderer { ui: ui.clone(), foreground });
    let tree = ObjectTree::new("gallery", &ui, container, Rc::new(Delegate), vec![renderer], ObjectTreeOptions::default());
    tree.layout(Some(PANEL_HEIGHT), Some(PANEL_WIDTH));
    tree.style(&default_list_styles(theme));
    tree.set_children(
        &None,
        vec![folder(0, false, vec![el(1), el(2), folder(3, true, vec![el(4)])]), folder(5, true, vec![el(9)]), el(6), el(7), el(8)],
        ObjectTreeModelSetChildrenOptions::default(),
    );
    ui.run_animation_frame();

    // `main.rs` fica selecionado e focado.
    tree.set_selection(&[Some(1)], None);
    tree.set_focus(&[Some(1)], None);
    if focused {
        tree.dom_focus();
    }

    // O ponteiro sobre a linha pedida.
    let mut rows = {
        let tree = ui.tree();
        support::query_all(&tree, container, &["monaco-list-row"])
    };
    rows.sort_by(|a, b| ui.tree().bounds(*a).y0.total_cmp(&ui.tree().bounds(*b).y0));
    let r = ui.tree().absolute_bounds(rows[hover_row]);
    let (hx, hy) = ((r.x0 + r.x1) / 2.0, (r.y0 + r.y1) / 2.0);
    ui.pointer_input(PointerInput::new(PointerInputKind::Move, hx - 4.0, hy));
    ui.pointer_input(PointerInput::new(PointerInputKind::Move, hx, hy));
    ui.run_animation_frame();
    (ui, tree)
}

fn render(name: &str, theme: &ColorTheme) {
    let background = theme.get_color("editor.background").expect("editor.background");
    let (focused_ui, focused_tree) = panel(20.0, theme, true, 3, "Lista com foco: main.rs selecionado e focado, widgets sob o ponteiro");
    let (blurred_ui, blurred_tree) = panel(360.0, theme, false, 3, "Lista sem foco: o mesmo estado");
    let path = support::save_png_layers(&[focused_ui, blurred_ui], background, WIDTH, HEIGHT, &format!("tree-{name}"));
    drop((focused_tree, blurred_tree));
    println!("{}", path.display());
}

fn main() {
    for (name, theme) in support::themes() {
        je_base::runtime::with_faked_timers(|| render(name, &theme));
    }
}
