//! Sonda H26, lado do je-code: pinta a moldura do workbench vazio (só as caixas das partes, com as
//! cores do tema 2026 Dark e a geometria medida no oráculo por CDP) e grava um PNG para comparar com
//! a captura do oráculo em `oracle-harness/imgdiff.py`. A geometria aqui é medida, não portada: o
//! layout de verdade vem com o porte do `layout.ts` e do grid (Fase 4).
//! Uso: screenshot-probe <tema.json> <saida.png>

use je_render::kurbo::{Rect, RoundedRect};
use je_render::{DisplayList, Rgba, Shape, render_rgba};
use je_theme::ColorTheme;
use je_ui::{Content, Role, Tree};

const W: u16 = 1200;
const H: u16 = 800;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let theme = ColorTheme::load(std::path::Path::new(&args[0])).expect("tema");
    let c = |id: &str| theme.color(id).unwrap_or_else(|| panic!("cor {id} ausente no tema"));
    let mut list = DisplayList::new();
    let (w, h) = (f64::from(W), f64::from(H));
    // Medido no oráculo 1.143.1 (1200x800, dpr 1): titlebar 0,0 1200x35; activitybar 4,35 44x733
    // raio 8 borda 1px; editor 52,35 1144x733 raio 8 borda 1px; statusbar 0,772 1200x28.
    list.fill_rect(Rect::new(0.0, 0.0, w, 35.0), c("titleBar.activeBackground"));
    list.fill_rect(Rect::new(0.0, 772.0, w, h), c("statusBar.background"));
    let activity = RoundedRect::new(4.0, 35.0, 48.0, 768.0, 8.0);
    list.fill(Shape::RoundedRect(activity), c("activityBar.background"));
    // A borda do painel flutuante medida no oráculo é #252526, não o `activityBar.border` do tema:
    // vem de outra regra de CSS, a ser achada no porte do layout.
    list.stroke(Shape::RoundedRect(RoundedRect::new(4.5, 35.5, 47.5, 767.5, 7.5)), 1.0, Rgba::opaque(0x25, 0x25, 0x26));
    let editor = RoundedRect::new(52.0, 35.0, 1196.0, 768.0, 8.0);
    list.fill(Shape::RoundedRect(editor), c("editor.background"));
    list.stroke(Shape::RoundedRect(RoundedRect::new(52.5, 35.5, 1195.5, 767.5, 7.5)), 1.0, c("editorGroup.border"));
    // Ícones da activity bar pela árvore do je-ui: rótulo 36x36 em x=8, y=39 + 44*i (medido no oráculo),
    // codicon de 24 px na cor `activityBar.inactiveForeground`. Os ids são os que o iconRegistry do
    // oráculo resolve (explorer-view-icon = files, search-view-icon = search-large...).
    let mut tree = Tree::new();
    let root = tree.root();
    tree.set_bounds(root, Rect::new(0.0, 0.0, w, h));
    let inactive = c("activityBar.inactiveForeground");
    let icons = [("files", 39.0), ("search-large", 83.0), ("source-control", 127.0), ("debug-alt", 171.0), ("extensions", 215.0), ("account", 684.0), ("settings-gear", 728.0)];
    for (id, y) in icons {
        let node = tree.create(root, Role::Button);
        tree.set_bounds(node, Rect::new(8.0, y, 44.0, y + 36.0));
        tree.style_mut(node).content = Content::Codicon { id: id.into(), size: 24.0, color: inactive };
    }
    tree.paint(&mut list, 1.0);
    let rgba = render_rgba(&list, c("titleBar.activeBackground"), W, H);
    let file = std::fs::File::create(&args[1]).expect("saída");
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), u32::from(W), u32::from(H));
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header().expect("png").write_image_data(&rgba).expect("png");
}
