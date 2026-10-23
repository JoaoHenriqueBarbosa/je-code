//! Teste do suporte das galerias: um nó de texto nos dois temas.
#[path = "support/mod.rs"]
mod support;

use je_render::kurbo::Rect;
use je_ui::{Role, Ui};
use je_widgets::text::{UI_FONT_SIZE, set_node_text, ui_font};

fn main() {
    for (name, theme) in support::themes() {
        let ui = Ui::new();
        let bg = theme.get_color("editor.background").expect("editor.background");
        {
            let mut tree = ui.tree_mut();
            let root = tree.root();
            tree.set_bounds(root, Rect::new(0.0, 0.0, 200.0, 40.0));
            let node = tree.create(root, Role::Generic);
            tree.set_bounds(node, Rect::new(10.0, 10.0, 190.0, 30.0));
            set_node_text(&mut tree, node, "Hello, je-widgets", &ui_font(UI_FONT_SIZE, 400), theme.get_color("foreground").expect("foreground"), 20.0);
        }
        println!("{}", support::save_png(&ui, bg, 200, 40, &format!("smoke-{name}")).display());
    }
}
