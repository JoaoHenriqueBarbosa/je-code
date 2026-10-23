//! Galeria visual da base dos widgets de ação: `ActionBar` (horizontal com ícones, horizontal com rótulo e
//! atalho, vertical; normal, hover, foco, desabilitado, checado, separador), `ContextView` ancorada em cada
//! posição e `HoverWidget` (texto, markdown com ações, ponteiro, dica de teclado).
//! Grava `wip/scratch/gallery/actionbar-dark.png` e `actionbar-light.png`.
#[path = "support/mod.rs"]
mod support;

use std::rc::Rc;

use je_base::actions::{Action, ActionRef, Separator};
use je_base::dynamic::Dynamic;
use je_base::layout::{AnchorAlignment, AnchorAxisAlignment, AnchorPosition, IAnchor};
use je_base::lifecycle::DisposableRef;
use je_render::Rgba;
use je_render::kurbo::Rect;
use je_theme::ColorTheme;
use je_ui::{Border, NodeId, PointerInput, PointerInputKind, Role, Ui};
use je_widgets::action_bar::{ActionBar, ActionsOrientation, IActionBarOptions, IActionOptions};
use je_widgets::action_view_items::{IActionViewItemOptions, default_action_bar_styles};
use je_widgets::context_view::ContextView;
use je_widgets::context_view_api::{Anchor, ContextViewDOMPosition, IContextViewDelegate};
use je_widgets::hover_api::{HoverContent, HoverPosition, HoverStyle, HoverTarget, IHoverAction, IHoverAppearanceOptions, IHoverOptions, IHoverPositionOptions};
use je_widgets::hover_widget::{WorkbenchHoverWidget, default_hover_styles};
use je_widgets::text::{set_node_text, ui_font};

const WIDTH: u16 = 1000;
const HEIGHT: u16 = 600;

fn markdown(value: &str) -> je_base::html_content::IMarkdownString {
    je_base::html_content::IMarkdownString { value: value.into(), is_trusted: None, support_theme_icons: None, support_html: None, support_alert_syntax: None, base_uri: None, uris: None }
}

fn color(theme: &ColorTheme, id: &str) -> Rgba {
    theme.get_color(id).unwrap_or_else(|| panic!("o tema não define {id}"))
}

/// Um texto solto na janela.
fn label(ui: &Ui, x: f64, y: f64, text: &str, fg: Rgba, size: f32, weight: u16) {
    let mut tree = ui.tree_mut();
    let root = tree.root();
    let node = tree.create(root, Role::Generic);
    let width = set_node_text(&mut tree, node, text, &ui_font(size, weight), fg, 20.0);
    tree.set_bounds(node, Rect::new(x, y, x + width, y + 20.0));
}

/// Uma caixa preenchida com borda opcional.
fn panel(ui: &Ui, rect: Rect, background: Rgba, border: Option<Rgba>) -> NodeId {
    let mut tree = ui.tree_mut();
    let root = tree.root();
    let node = tree.create(root, Role::Generic);
    tree.set_bounds(node, rect);
    let style = tree.style_mut(node);
    style.background = Some(background);
    style.border = border.map(|color| Border { width: 1.0, color });
    node
}

fn action(id: &str, label: &str, class: &str, enabled: bool, checked: Option<bool>) -> ActionRef {
    let action = Action::new(id, label, class, enabled, Some(Rc::new(|_| Dynamic::Undefined.into())));
    if checked.is_some() {
        je_base::actions::IAction::set_checked(&*action, checked);
    }
    action
}

/// O centro de um nó em coordenadas da janela.
fn center(ui: &Ui, node: NodeId) -> (f64, f64) {
    let r = ui.tree().absolute_bounds(node);
    ((r.x0 + r.x1) / 2.0, (r.y0 + r.y1) / 2.0)
}

fn label_node(ui: &Ui, bar: &ActionBar, index: usize) -> NodeId {
    let tree = ui.tree();
    tree.children_with_class(tree.children(bar.actions_list())[index], "action-label")[0]
}

/// Uma visão de contexto ancorada, com uma caixa de 130x56 e um texto.
struct BoxDelegate {
    ui: Ui,
    anchor: IAnchor,
    alignment: AnchorAlignment,
    position: AnchorPosition,
    axis: AnchorAxisAlignment,
    text: &'static str,
    background: Rgba,
    border: Rgba,
    fg: Rgba,
}

impl IContextViewDelegate for BoxDelegate {
    fn get_anchor(&self) -> Anchor {
        Anchor::Rect(self.anchor)
    }
    fn render(&self, container: NodeId) -> Option<DisposableRef> {
        let mut tree = self.ui.tree_mut();
        tree.set_bounds(container, Rect::new(0.0, 0.0, 130.0, 56.0));
        let style = tree.style_mut(container);
        style.background = Some(self.background);
        style.border = Some(Border { width: 1.0, color: self.border });
        style.radius = 6.0;
        let text = tree.create(container, Role::Generic);
        let width = set_node_text(&mut tree, text, self.text, &ui_font(12.0, 400), self.fg, 18.0);
        tree.set_bounds(text, Rect::new(10.0, 19.0, 10.0 + width, 37.0));
        None
    }
    fn anchor_alignment(&self) -> Option<AnchorAlignment> {
        Some(self.alignment)
    }
    fn anchor_position(&self) -> Option<AnchorPosition> {
        Some(self.position)
    }
    fn anchor_axis_alignment(&self) -> Option<AnchorAxisAlignment> {
        Some(self.axis)
    }
}

fn render(name: &str, theme: &ColorTheme) {
    let ui = Ui::new();
    let root = ui.root();
    ui.tree_mut().set_bounds(root, Rect::new(0.0, 0.0, f64::from(WIDTH), f64::from(HEIGHT)));
    let bg = color(theme, "editor.background");
    let fg = color(theme, "foreground");
    let panel_bg = color(theme, "sideBar.background");
    let border = theme.get_color("contrastBorder").or_else(|| theme.get_color("panel.border")).unwrap_or(fg);
    let styles = Rc::new(default_action_bar_styles(theme));
    let hover_styles = default_hover_styles(theme);

    // ---- ActionBar ----
    label(&ui, 20.0, 8.0, "ActionBar", fg, 13.0, 700);
    let mut stores: Vec<Rc<ActionBar>> = Vec::new();

    label(&ui, 20.0, 34.0, "horizontal, ícones: normal, hover, desabilitado, checado, separador", fg, 11.0, 400);
    let icons_panel = panel(&ui, Rect::new(20.0, 56.0, 330.0, 90.0), panel_bg, Some(border));
    let icons = ActionBar::new(&ui, icons_panel, IActionBarOptions { styles: Some(styles.clone()), highlight_toggled_items: true, aria_label: Some("ícones".to_owned()), ..Default::default() });
    let icon_item = IActionViewItemOptions { icon: Some(true), label: Some(false), ..Default::default() };
    let push_icon = |bar: &ActionBar, action: ActionRef| bar.push_action(action, IActionOptions { item: icon_item.clone(), index: None });
    push_icon(&icons, action("add", "Adicionar", "codicon codicon-add", true, None));
    push_icon(&icons, action("refresh", "Atualizar", "codicon codicon-refresh", true, None));
    let separator: ActionRef = Separator::new();
    push_icon(&icons, separator);
    push_icon(&icons, action("trash", "Excluir", "codicon codicon-trash", false, None));
    push_icon(&icons, action("edit", "Editar", "codicon codicon-edit", true, Some(true)));
    push_icon(&icons, action("gear", "Configurar", "codicon codicon-gear", true, None));
    stores.push(icons.clone());

    label(&ui, 20.0, 104.0, "horizontal, rótulo e atalho: foco, desabilitado, checado", fg, 11.0, 400);
    let text_panel = panel(&ui, Rect::new(20.0, 126.0, 330.0, 190.0), panel_bg, Some(border));
    let text_bar = ActionBar::new(&ui, text_panel, IActionBarOptions { styles: Some(styles.clone()), highlight_toggled_items: true, ..Default::default() });
    for (id, text, enabled, checked, keybinding) in [("run", "Executar", true, None, Some("Ctrl+R")), ("debug", "Depurar", true, None, Some("F5")), ("stop", "Parar", false, None, None), ("pin", "Fixar", true, Some(true), None)] {
        text_bar.push_action(action(id, text, "", enabled, checked), IActionOptions { item: IActionViewItemOptions { keybinding: keybinding.map(str::to_owned), ..Default::default() }, index: None });
    }
    stores.push(text_bar.clone());

    label(&ui, 20.0, 204.0, "vertical", fg, 11.0, 400);
    let vertical_panel = panel(&ui, Rect::new(20.0, 226.0, 200.0, 360.0), panel_bg, Some(border));
    let vertical = ActionBar::new(&ui, vertical_panel, IActionBarOptions { styles: Some(styles.clone()), orientation: Some(ActionsOrientation::Vertical), highlight_toggled_items: true, ..Default::default() });
    vertical.push_action(action("new", "Novo arquivo", "", true, None), IActionOptions::default());
    vertical.push_action(action("open", "Abrir...", "", true, None), IActionOptions::default());
    let separator: ActionRef = Separator::new();
    vertical.push_action(separator, IActionOptions::default());
    vertical.push_action(action("pinned", "Fixado", "", true, Some(true)), IActionOptions::default());
    vertical.push_action(action("exit", "Sair", "", false, None), IActionOptions::default());
    stores.push(vertical.clone());

    // ---- ContextView ----
    label(&ui, 400.0, 8.0, "ContextView", fg, 13.0, 700);
    let cases: [(f64, f64, AnchorAlignment, AnchorPosition, AnchorAxisAlignment, &'static str); 5] = [
        (420.0, 60.0, AnchorAlignment::Left, AnchorPosition::Below, AnchorAxisAlignment::Vertical, "abaixo, à esquerda"),
        (620.0, 60.0, AnchorAlignment::Right, AnchorPosition::Below, AnchorAxisAlignment::Vertical, "abaixo, à direita"),
        (420.0, 230.0, AnchorAlignment::Left, AnchorPosition::Above, AnchorAxisAlignment::Vertical, "acima, à esquerda"),
        (620.0, 230.0, AnchorAlignment::Right, AnchorPosition::Above, AnchorAxisAlignment::Vertical, "acima, à direita"),
        (830.0, 150.0, AnchorAlignment::Left, AnchorPosition::Below, AnchorAxisAlignment::Horizontal, "eixo horizontal"),
    ];
    let mut context_views = Vec::new();
    for (x, y, alignment, position, axis, text) in cases {
        panel(&ui, Rect::new(x, y, x + 60.0, y + 24.0), color(theme, "button.background"), None);
        let context_view = ContextView::new(&ui, root, ContextViewDOMPosition::Absolute);
        context_view.show(Rc::new(BoxDelegate {
            ui: ui.clone(),
            anchor: IAnchor { x, y, width: Some(60.0), height: Some(24.0) },
            alignment,
            position,
            axis,
            text,
            background: color(theme, "editorWidget.background"),
            border: color(theme, "editorWidget.border"),
            fg,
        }));
        context_views.push(context_view);
    }

    // ---- HoverWidget ----
    label(&ui, 30.0, 394.0, "HoverWidget: texto, ações na barra de status, ponteiro, markdown com a dica de teclado", fg, 13.0, 700);
    let target = |x: f64, text: &str| {
        let node = panel(&ui, Rect::new(x, 462.0, x + 110.0, 486.0), color(theme, "button.secondaryBackground"), None);
        let mut tree = ui.tree_mut();
        let t = tree.create(node, Role::Generic);
        let width = set_node_text(&mut tree, t, text, &ui_font(12.0, 400), color(theme, "button.secondaryForeground"), 24.0);
        tree.set_bounds(t, Rect::new((110.0 - width) / 2.0, 0.0, (110.0 + width) / 2.0, 24.0));
        node
    };
    let mut hovers = Vec::new();
    let mut show = |x: f64, caption: &str, mut options: IHoverOptions| {
        let node = target(x, caption);
        options.target = HoverTarget::Node(node);
        let hover = WorkbenchHoverWidget::new(&ui, options, hover_styles, Some(&|command: &str| (command == "workbench.action.showCommands").then(|| "Ctrl+Shift+P".to_owned())));
        let context_view = ContextView::new(&ui, root, ContextViewDOMPosition::Absolute);
        context_view.show(hover.context_view_delegate(false, 1));
        hovers.push((hover, context_view));
    };
    let base = |content: HoverContent| IHoverOptions::new(content, HoverTarget::Node(root));
    show(30.0, "texto (acima)", base(HoverContent::Text("Dica de texto.\nSegunda linha.".to_owned())));
    let mut with_actions = base(HoverContent::Text("Dica com ações.".to_owned()));
    with_actions.actions = vec![
        IHoverAction { label: "Comandos".to_owned(), command_id: "workbench.action.showCommands".to_owned(), icon_class: Some("codicon codicon-terminal".to_owned()), run: Rc::new(|_| {}) },
        IHoverAction { label: "Ignorar".to_owned(), command_id: "ignore".to_owned(), icon_class: None, run: Rc::new(|_| {}) },
    ];
    with_actions.position = Some(IHoverPositionOptions { hover_position: Some(HoverPosition::Below), ..Default::default() });
    show(275.0, "ações (abaixo)", with_actions);
    let mut pointer = base(HoverContent::Text("Com ponteiro.".to_owned()));
    pointer.style = Some(HoverStyle::Pointer);
    show(520.0, "ponteiro (acima)", pointer);
    let mut hint = base(HoverContent::Markdown(markdown("Markdown em **dois**\n\nparágrafos, com dica.")));
    hint.appearance = Some(IHoverAppearanceOptions { show_hover_hint: Some(true), ..Default::default() });
    hint.position = Some(IHoverPositionOptions { hover_position: Some(HoverPosition::Below), ..Default::default() });
    show(765.0, "markdown e dica", hint);

    // ---- estados: hover do item de ícone "Adicionar" e foco no rótulo "Depurar" ----
    let hovered = label_node(&ui, &icons, 1);
    let (x, y) = center(&ui, hovered);
    ui.pointer_input(PointerInput::new(PointerInputKind::Move, x, y));
    text_bar.focus_index(1);
    label(&ui, 340.0, 62.0, "<- hover em Atualizar", fg, 11.0, 400);
    label(&ui, 340.0, 132.0, "<- foco em Depurar", fg, 11.0, 400);
    let _ = (&stores, &context_views, &hovers);

    println!("{}", support::save_png(&ui, bg, WIDTH, HEIGHT, &format!("actionbar-{name}")).display());
    for (hover, context_view) in hovers {
        je_base::lifecycle::IDisposable::dispose(&*hover);
        je_base::lifecycle::IDisposable::dispose(&*context_view);
    }
}

fn main() {
    for (name, theme) in support::themes() {
        render(name, &theme);
    }
}
