//! Galeria visual da `SelectBox`: fechada (normal, foco, desabilitada, texto cortado) e aberta com a lista
//! (item selecionado, mouse sobre uma linha, separador, `detail`, `decoratorRight`, opção desabilitada, painel de
//! detalhes, lista para cima com separador rotulado), nos dois temas. Cada cena aberta é um `Ui` próprio
//! (duas listas abertas não coexistem: a segunda tira o foco da primeira, que se fecha) e as cenas são
//! empilhadas na mesma imagem. Grava `wip/scratch/gallery/selectbox-dark.png` e `selectbox-light.png`.
#[path = "support/mod.rs"]
mod support;

use std::any::Any;
use std::rc::Rc;

use je_base::layout::AnchorPosition;
use je_render::Rgba;
use je_render::kurbo::Rect;
use je_theme::ColorTheme;
use je_ui::{Content, PointerInput, PointerInputKind, Role, Ui};
use je_widgets::context_view::ContextView;
use je_widgets::context_view_api::{ContextViewDOMPosition, IContextViewProvider};
use je_widgets::dom::create_el;
use je_widgets::select_box::{ISelectBoxDelegate, ISelectBoxOptions, ISelectOptionItem, SelectBox, default_select_box_styles, separator_select_option};
use je_widgets::text::{set_node_text, ui_font};

const WIDTH: u16 = 1000;
const HEIGHT: u16 = 560;
const BOX_WIDTH: f64 = 200.0;

fn color(theme: &ColorTheme, id: &str) -> Rgba {
    theme.get_color(id).unwrap_or_else(|| panic!("o tema não define {id}"))
}

/// Uma janela vazia do tamanho da imagem.
fn new_scene() -> Ui {
    let ui = Ui::new();
    let root = ui.root();
    ui.tree_mut().set_bounds(root, Rect::new(0.0, 0.0, f64::from(WIDTH), f64::from(HEIGHT)));
    ui
}

/// Um texto solto na janela.
fn label(ui: &Ui, x: f64, y: f64, text: &str, fg: Rgba, size: f32, weight: u16) {
    let mut tree = ui.tree_mut();
    let root = tree.root();
    let node = tree.create(root, Role::Generic);
    let width = set_node_text(&mut tree, node, text, &ui_font(size, weight), fg, 20.0);
    tree.set_bounds(node, Rect::new(x, y, x + width, y + 20.0));
}

fn fruits() -> Vec<ISelectOptionItem> {
    vec![
        ISelectOptionItem::new("Apple"),
        ISelectOptionItem::new("Banana"),
        separator_select_option(),
        ISelectOptionItem { detail: Some("variedade ácida".to_owned()), ..ISelectOptionItem::new("Cherry") },
        ISelectOptionItem { decorator_right: Some("Ctrl+D".to_owned()), ..ISelectOptionItem::new("Date") },
        ISelectOptionItem { is_disabled: true, ..ISelectOptionItem::new("Elderberry (indisponível)") },
    ]
}

/// A caixa de seleção posicionada em `(x, y)`, com a largura fixa da galeria.
struct Placed {
    select_box: Rc<SelectBox>,
    _context_view: Rc<ContextView>,
}

#[allow(clippy::too_many_arguments)]
fn place(ui: &Ui, theme: &ColorTheme, x: f64, y: f64, width: f64, options: Vec<ISelectOptionItem>, selected: usize, select_box_options: ISelectBoxOptions) -> Placed {
    let root = ui.root();
    let context_view = ContextView::new(ui, root, ContextViewDOMPosition::Absolute);
    let provider: Rc<dyn IContextViewProvider> = context_view.clone();
    let select_box = SelectBox::new(ui, options, selected, provider, default_select_box_styles(theme), Some(select_box_options));
    let host = create_el(&mut ui.tree_mut(), root, Role::Generic, "host");
    let height = select_box.natural_size().1;
    ui.tree_mut().set_bounds(host, Rect::new(x, y, x + width, y + height));
    select_box.render(host);
    Placed { select_box, _context_view: context_view }
}

fn open(placed: &Placed, ui: &Ui) {
    ui.dispatch(&mut je_ui::Event::new(je_ui::EventKind::Click, placed.select_box.select_element()));
}

/// O centro da linha da lista cujo texto é `text`.
fn row_center(ui: &Ui, placed: &Placed, text: &str) -> (f64, f64) {
    let tree = ui.tree();
    let row = tree
        .subtree(placed.select_box.drop_down_node())
        .into_iter()
        .filter(|n| tree.has_class(*n, "monaco-list-row"))
        .find(|n| tree.children_with_class(*n, "option-text").first().is_some_and(|t| matches!(&tree.style(*t).content, Content::Text { text: t, .. } if t == text)))
        .unwrap_or_else(|| panic!("sem a linha {text}"));
    let r = tree.absolute_bounds(row);
    ((r.x0 + r.x1) / 2.0, (r.y0 + r.y1) / 2.0)
}

fn render(name: &str, theme: &ColorTheme) {
    let fg = color(theme, "foreground");
    let mut keep: Vec<Box<dyn Any>> = Vec::new();
    let mut layers: Vec<Ui> = Vec::new();

    // ---- Cena 1: fechada ----
    let closed = new_scene();
    label(&closed, 20.0, 8.0, "SelectBox: fechado (normal, foco, desabilitado, cortado) e aberto", fg, 13.0, 700);
    for (x, caption) in [(20.0, "normal"), (250.0, "foco"), (480.0, "desabilitado"), (710.0, "texto cortado (150px)")] {
        label(&closed, x, 34.0, caption, fg, 11.0, 400);
    }
    let normal = place(&closed, theme, 20.0, 58.0, BOX_WIDTH, fruits(), 1, ISelectBoxOptions::default());
    let focus = place(&closed, theme, 250.0, 58.0, BOX_WIDTH, fruits(), 0, ISelectBoxOptions::default());
    let disabled = place(&closed, theme, 480.0, 58.0, BOX_WIDTH, fruits(), 0, ISelectBoxOptions::default());
    disabled.select_box.set_enabled(false);
    let long_text = vec![ISelectOptionItem::new("Uma opção com um texto bem comprido que não cabe"), ISelectOptionItem::new("Curta")];
    let cut = place(&closed, theme, 710.0, 58.0, 150.0, long_text, 0, ISelectBoxOptions::default());
    focus.select_box.focus();
    keep.extend([Box::new(normal) as Box<dyn Any>, Box::new(focus), Box::new(disabled), Box::new(cut)]);
    layers.push(closed);

    // ---- Cena 2: aberta, item selecionado ----
    let selected = new_scene();
    label(&selected, 20.0, 100.0, "item selecionado (foco)", fg, 11.0, 400);
    let placed = place(&selected, theme, 20.0, 124.0, BOX_WIDTH, fruits(), 0, ISelectBoxOptions::default());
    open(&placed, &selected);
    keep.push(Box::new(placed));
    layers.push(selected);

    // ---- Cena 3: aberta, mouse sobre a terceira opção ----
    let hovered = new_scene();
    label(&hovered, 330.0, 100.0, "mouse sobre Cherry", fg, 11.0, 400);
    let placed = place(&hovered, theme, 330.0, 124.0, BOX_WIDTH, fruits(), 1, ISelectBoxOptions::default());
    open(&placed, &hovered);
    let (x, y) = row_center(&hovered, &placed, "Cherry");
    hovered.pointer_input(PointerInput::new(PointerInputKind::Move, x, y));
    keep.push(Box::new(placed));
    layers.push(hovered);

    // ---- Cena 4: aberta com o painel de detalhes ----
    let details = new_scene();
    label(&details, 640.0, 100.0, "painel de detalhes", fg, 11.0, 400);
    let described = vec![
        ISelectOptionItem { description: Some("Mostra só as sessões deste computador.".to_owned()), ..ISelectOptionItem::new("Local") },
        ISelectOptionItem { description: Some("Mostra as sessões de todos os computadores conectados, inclusive as que estão em segundo plano e as remotas.".to_owned()), ..ISelectOptionItem::new("Todas") },
        ISelectOptionItem::new("Nenhuma"),
    ];
    let placed = place(&details, theme, 640.0, 124.0, 220.0, described, 1, ISelectBoxOptions::default());
    open(&placed, &details);
    keep.push(Box::new(placed));
    layers.push(details);

    // ---- Cena 5: aberta para cima, separador rotulado ----
    let above = new_scene();
    label(&above, 20.0, 500.0, "lista para cima (anchorPosition: ABOVE) com separador rotulado", fg, 11.0, 400);
    let grouped = vec![
        ISelectOptionItem { decorator_right: Some("Frutas".to_owned()), is_separator: true, is_disabled: true, ..ISelectOptionItem::new("") },
        ISelectOptionItem::new("Apple"),
        ISelectOptionItem::new("Banana"),
        ISelectOptionItem { decorator_right: Some("Verduras".to_owned()), is_separator: true, is_disabled: true, ..ISelectOptionItem::new("") },
        ISelectOptionItem::new("Alface"),
        ISelectOptionItem::new("Couve"),
    ];
    let placed = place(&above, theme, 20.0, 470.0, BOX_WIDTH, grouped, 1, ISelectBoxOptions { anchor_position: Some(AnchorPosition::Above), ..Default::default() });
    open(&placed, &above);
    keep.push(Box::new(placed));
    layers.push(above);

    let path = support::save_png_layers(&layers, color(theme, "editor.background"), WIDTH, HEIGHT, &format!("selectbox-{name}"));
    println!("{}", path.display());
    drop(keep);
}

fn main() {
    for (name, theme) in support::themes() {
        render(name, &theme);
    }
}
