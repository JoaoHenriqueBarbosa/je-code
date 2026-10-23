//! Galeria visual do `ToolBar` e dos dropdowns: barras com poucas ações e com o botão "mais", barras
//! responsivas (várias larguras, `kind` `all` e `last`, com rótulos de texto), estados de hover, foco, desabilitado
//! e separador final, `BaseDropdown` (rótulo de texto e de ícone com texto), `DropdownMenu` fechado e aberto,
//! `DropdownMenuActionViewItem` fechado e aberto e o botão dividido (`ActionWithDropdownActionViewItem`).
//! O menu aberto é o do módulo `menu` (outro agente): aqui um provedor de menus de contexto desenha uma lista
//! simples de linhas sobre uma `ContextView`, só para mostrar a âncora e o estado `active`.
//! Grava `wip/scratch/gallery/toolbar-dark.png` e `toolbar-light.png`.
#[path = "support/mod.rs"]
mod support;

use std::cell::RefCell;
use std::rc::Rc;

use je_base::actions::{Action, ActionRef, Separator};
use je_base::dynamic::Dynamic;
use je_base::lifecycle::{DisposableRef, IDisposable};
use je_render::Rgba;
use je_render::kurbo::Rect;
use je_theme::ColorTheme;
use je_ui::{Border, NodeId, PointerInput, PointerInputKind, Role, Ui};
use je_widgets::action_bar::{ActionBar, IActionBarOptions, IActionOptions, IActionViewItemProvider};
use je_widgets::action_view_items::{ActionBarStyles, IActionViewItem, IActionViewItemOptions, default_action_bar_styles};
use je_widgets::context_view::ContextView;
use je_widgets::context_view_api::{Anchor, ContextViewDOMPosition, IContextMenuDelegate, IContextMenuProvider, IContextViewDelegate};
use je_widgets::dom::create_el;
use je_widgets::dropdown::{BaseDropdown, DropdownMenu, IBaseDropdownOptions, IDropdown, IDropdownMenuOptions};
use je_widgets::dropdown_action_view_item::{ActionWithDropdownActionViewItem, DropdownMenuActionViewItem, IActionWithDropdownActionViewItemOptions, IDropdownMenuActionViewItemOptions, MenuActions};
use je_widgets::text::{measure_text, natural_line_height, set_node_text, ui_font};
use je_widgets::toolbar::{IToolBarOptions, IToolBarResponsiveBehaviorOptions, OverflowFrom, ResponsiveKind, ToolBar};

const WIDTH: u16 = 1000;
const HEIGHT: u16 = 760;

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

fn action(id: &str, label: &str, class: &str, enabled: bool) -> ActionRef {
    Action::new(id, label, class, enabled, Some(Rc::new(|_| Dynamic::Undefined.into())))
}

fn center(ui: &Ui, node: NodeId) -> (f64, f64) {
    let r = ui.tree().absolute_bounds(node);
    ((r.x0 + r.x1) / 2.0, (r.y0 + r.y1) / 2.0)
}

fn hover(ui: &Ui, node: NodeId) {
    let (x, y) = center(ui, node);
    ui.pointer_input(PointerInput::new(PointerInputKind::Move, x, y));
}

/// O menu de contexto de mentira da galeria: uma lista de linhas numa `ContextView`, ancorada onde o dropdown pede.
struct StandInMenuProvider {
    ui: Ui,
    theme: ColorTheme,
    shown: RefCell<Vec<Rc<ContextView>>>,
}

struct StandInMenu {
    ui: Ui,
    anchor: Anchor,
    rows: Vec<String>,
    background: Rgba,
    border: Rgba,
    fg: Rgba,
}

impl IContextViewDelegate for StandInMenu {
    fn get_anchor(&self) -> Anchor {
        self.anchor
    }

    fn render(&self, container: NodeId) -> Option<DisposableRef> {
        let font = ui_font(13.0, 400);
        let width = self.rows.iter().map(|row| measure_text(row, &font)).fold(0.0, f64::max) + 28.0;
        let line_height = natural_line_height(&font);
        let mut tree = self.ui.tree_mut();
        tree.set_bounds(container, Rect::new(0.0, 0.0, width, 8.0 + 26.0 * self.rows.len() as f64));
        let style = tree.style_mut(container);
        style.background = Some(self.background);
        style.border = Some(Border { width: 1.0, color: self.border });
        style.radius = 8.0;
        for (index, row) in self.rows.iter().enumerate() {
            let node = create_el(&mut tree, container, Role::MenuItem, "menu-row");
            let text_width = set_node_text(&mut tree, node, row, &font, self.fg, line_height);
            let y = 4.0 + 26.0 * index as f64 + (26.0 - line_height) / 2.0;
            tree.set_bounds(node, Rect::new(14.0, y, 14.0 + text_width, y + line_height));
        }
        None
    }
}

impl IContextMenuProvider for StandInMenuProvider {
    fn show_context_menu(&self, delegate: Rc<dyn IContextMenuDelegate>) {
        let rows = delegate.get_actions().iter().map(|a| a.label()).collect();
        let theme = &self.theme;
        let context_view = ContextView::new(&self.ui, self.ui.root(), ContextViewDOMPosition::Absolute);
        context_view.show(Rc::new(StandInMenu {
            ui: self.ui.clone(),
            anchor: delegate.get_anchor(),
            rows,
            background: color(theme, "menu.background"),
            border: theme.get_color("menu.border").or_else(|| theme.get_color("widget.border")).unwrap_or(color(theme, "foreground")),
            fg: color(theme, "menu.foreground"),
        }));
        self.shown.borrow_mut().push(context_view);
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
    let styles: Rc<ActionBarStyles> = Rc::new(default_action_bar_styles(theme));
    let provider = Rc::new(StandInMenuProvider { ui: ui.clone(), theme: theme.clone(), shown: RefCell::new(Vec::new()) });
    let mut toolbars: Vec<Rc<ToolBar>> = Vec::new();
    let mut disposables: Vec<Rc<dyn IDisposable>> = Vec::new();

    let toolbar_in = |x: f64, y: f64, width: f64, options: IToolBarOptions| {
        let container = panel(&ui, Rect::new(x, y, x + width, y + 30.0), panel_bg, Some(border));
        ToolBar::new(&ui, container, provider.clone(), IToolBarOptions { styles: Some(styles.clone()), ..options })
    };
    let icon_actions = |count: usize| -> Vec<ActionRef> {
        let icons = ["add", "refresh", "trash", "edit", "gear", "search", "save", "play"];
        (0..count).map(|i| action(&format!("a{i}"), icons[i % icons.len()], &format!("codicon codicon-{}", icons[i % icons.len()]), true)).collect()
    };
    let responsive = |kind: ResponsiveKind| IToolBarResponsiveBehaviorOptions { enabled: true, kind, ..Default::default() };

    // ---- ToolBar ----
    label(&ui, 20.0, 8.0, "ToolBar", fg, 13.0, 700);

    label(&ui, 20.0, 34.0, "poucas ações e o botão \"mais\" (secundárias)", fg, 11.0, 400);
    let few = toolbar_in(20.0, 56.0, 273.0, IToolBarOptions::default());
    few.set_actions(&icon_actions(3), &[action("s1", "Renomear", "", true), action("s2", "Excluir", "", true)]);
    toolbars.push(few.clone());

    label(&ui, 320.0, 34.0, "responsiva (all): 8 ações em 120px e em 360px", fg, 11.0, 400);
    let narrow = toolbar_in(320.0, 56.0, 120.0, IToolBarOptions { responsive_behavior: Some(responsive(ResponsiveKind::All)), ..Default::default() });
    narrow.set_actions(&icon_actions(8), &[]);
    toolbars.push(narrow.clone());
    let wide = toolbar_in(460.0, 56.0, 360.0, IToolBarOptions { responsive_behavior: Some(responsive(ResponsiveKind::All)), ..Default::default() });
    wide.set_actions(&icon_actions(8), &[]);
    toolbars.push(wide.clone());

    label(&ui, 20.0, 100.0, "all, minItems 2, a partir do início", fg, 11.0, 400);
    let from_start = toolbar_in(20.0, 122.0, 130.0, IToolBarOptions { responsive_behavior: Some(IToolBarResponsiveBehaviorOptions { min_items: Some(2), overflow_from: Some(OverflowFrom::Start), ..responsive(ResponsiveKind::All) }), ..Default::default() });
    from_start.set_actions(&icon_actions(8), &[action("history", "Histórico", "", true)]);
    toolbars.push(from_start.clone());

    label(&ui, 320.0, 100.0, "rótulos (last): 273px e 150px", fg, 11.0, 400);
    let labeled = |x: f64, width: f64| {
        let toolbar = toolbar_in(x, 122.0, width, IToolBarOptions { icon: Some(false), label: Some(true), responsive_behavior: Some(IToolBarResponsiveBehaviorOptions { min_items: Some(1), ..responsive(ResponsiveKind::Last) }), ..Default::default() });
        toolbar.set_actions(&[action("run", "Executar", "", true), action("debug", "Depurar", "", true), action("test", "Testar", "", true), action("deploy", "Publicar", "", true)], &[]);
        toolbar
    };
    toolbars.push(labeled(320.0, 273.0));
    toolbars.push(labeled(620.0, 150.0));

    label(&ui, 20.0, 166.0, "hover, foco, desabilitado, marcado, separador final", fg, 11.0, 400);
    let states = toolbar_in(20.0, 188.0, 273.0, IToolBarOptions { trailing_separator: true, highlight_toggled_items: true, ..Default::default() });
    let checked = action("pin", "Fixar", "codicon codicon-pinned", true);
    je_base::actions::IAction::set_checked(&*checked, Some(true));
    states.set_actions(&[action("add", "Adicionar", "codicon codicon-add", true), action("refresh", "Atualizar", "codicon codicon-refresh", true), action("trash", "Excluir", "codicon codicon-trash", true), action("edit", "Editar", "codicon codicon-edit", false), checked], &[]);
    toolbars.push(states.clone());
    states.focus(Some(2));

    // ---- BaseDropdown ----
    label(&ui, 20.0, 240.0, "BaseDropdown / DropdownMenu", fg, 13.0, 700);
    label(&ui, 20.0, 266.0, "rótulo de texto; ícone com texto; DropdownMenu aberto (active)", fg, 11.0, 400);
    let dropdown_box = |x: f64, width: f64| {
        let container = panel(&ui, Rect::new(x, 288.0, x + width, 318.0), panel_bg, Some(border));
        let dropdown_options = |options: IBaseDropdownOptions| IBaseDropdownOptions { foreground: Some(fg), ..options };
        (container, dropdown_options)
    };
    let (text_container, with_fg) = dropdown_box(20.0, 130.0);
    let text_dropdown = BaseDropdown::new(&ui, text_container, with_fg(IBaseDropdownOptions { label: Some("Menu".to_owned()), ..Default::default() }));
    {
        let mut tree = ui.tree_mut();
        tree.set_bounds(text_dropdown.element(), Rect::new(0.0, 0.0, 130.0, 30.0));
    }
    text_dropdown.layout();
    disposables.push(text_dropdown);

    let (icon_container, with_fg) = dropdown_box(170.0, 130.0);
    let renderer_ui = ui.clone();
    let icon_dropdown = BaseDropdown::new(
        &ui,
        icon_container,
        with_fg(IBaseDropdownOptions {
            label_renderer: Some(Rc::new(move |container: NodeId| {
                let font = ui_font(13.0, 400);
                let line_height = natural_line_height(&font);
                let mut tree = renderer_ui.tree_mut();
                let text = create_el(&mut tree, container, Role::Generic, "dropdown-label-text");
                let width = set_node_text(&mut tree, text, "Ordenar", &font, fg, line_height);
                tree.set_bounds(text, Rect::new(0.0, 0.0, width + 4.0, line_height));
                let icon = create_el(&mut tree, container, Role::Generic, "codicon");
                tree.style_mut(icon).content = je_ui::Content::Codicon { id: "chevron-down".to_owned(), size: 16.0, color: fg };
                tree.set_bounds(icon, Rect::new(0.0, 0.0, 16.0, 16.0));
                None
            })),
            ..Default::default()
        }),
    );
    ui.tree_mut().set_bounds(icon_dropdown.element(), Rect::new(0.0, 0.0, 130.0, 30.0));
    icon_dropdown.layout();
    disposables.push(icon_dropdown);

    let (open_container, with_fg) = dropdown_box(320.0, 130.0);
    let mut menu_options = IDropdownMenuOptions::new(provider.clone());
    menu_options.base = with_fg(IBaseDropdownOptions { label: Some("Arquivo".to_owned()), ..Default::default() });
    menu_options.actions = Some(vec![action("new", "Novo arquivo", "", true), action("open", "Abrir...", "", true), action("save", "Salvar", "", true)]);
    let dropdown_menu = DropdownMenu::new(&ui, open_container, menu_options);
    ui.tree_mut().set_bounds(dropdown_menu.element(), Rect::new(0.0, 0.0, 130.0, 30.0));
    dropdown_menu.layout();
    dropdown_menu.show();
    disposables.push(dropdown_menu);

    // ---- DropdownMenuActionViewItem e botão dividido, numa ActionBar ----
    label(&ui, 520.0, 266.0, "DropdownMenuActionViewItem: fechado, aberto; botão dividido", fg, 11.0, 400);
    let bar_container = panel(&ui, Rect::new(520.0, 288.0, 980.0, 318.0), panel_bg, Some(border));
    let item_provider: IActionViewItemProvider = {
        let (ui, provider, styles) = (ui.clone(), provider.clone(), styles.clone());
        Rc::new(move |action: &ActionRef, options: &IActionViewItemOptions| -> Option<Rc<dyn IActionViewItem>> {
            let menu = MenuActions::Actions(vec![je_widgets_menu_action("m1", "Primeira"), je_widgets_menu_action("m2", "Segunda")]);
            match action.id().as_str() {
                "closed" | "open" => Some(DropdownMenuActionViewItem::new(
                    &ui,
                    action.clone(),
                    menu,
                    provider.clone(),
                    IDropdownMenuActionViewItemOptions { base: IActionViewItemOptions { styles: Some(styles.clone()), ..options.clone() }, class_names: vec!["codicon".to_owned(), "codicon-toolbar-more".to_owned()], ..Default::default() },
                )),
                id if id.starts_with("split") => Some(ActionWithDropdownActionViewItem::new(
                    &ui,
                    None,
                    action.clone(),
                    IActionWithDropdownActionViewItemOptions { base: IActionViewItemOptions { icon: Some(true), label: Some(false), styles: Some(styles.clone()), ..options.clone() }, menu_actions_or_provider: menu, menu_action_class_names: Vec::new(), keybinding_provider: None },
                    provider.clone(),
                )),
                _ => None,
            }
        })
    };
    let bar = ActionBar::new(&ui, bar_container, IActionBarOptions { styles: Some(styles.clone()), action_view_item_provider: Some(item_provider), ..Default::default() });
    let push = |action: ActionRef| bar.push_action(action, IActionOptions { item: IActionViewItemOptions { icon: Some(true), label: Some(false), ..Default::default() }, index: None });
    push(action("closed", "Mais (fechado)", "", true));
    push(action("open", "Mais (aberto)", "", true));
    push(Separator::new());
    push(action("split-a", "Executar", "codicon codicon-play", true));
    push(action("split-b", "Depurar", "codicon codicon-debug-alt", true));
    push(action("split-c", "Parar", "codicon codicon-debug-stop", false));
    // aberto: o segundo item
    let open_item = bar.view_items()[1].clone();
    let open_label = open_item.base().element().unwrap();
    let (x, y) = center(&ui, open_label);
    ui.pointer_input(PointerInput::new(PointerInputKind::Down, x, y));
    ui.pointer_input(PointerInput::new(PointerInputKind::Up, x, y));

    // ---- o botão "mais" aberto numa ToolBar, com o menu das secundárias ----
    label(&ui, 20.0, 340.0, "ToolBar com o menu \"mais\" aberto (as secundárias)", fg, 11.0, 400);
    let open_toolbar = toolbar_in(20.0, 362.0, 273.0, IToolBarOptions::default());
    open_toolbar.set_actions(&icon_actions(3), &[action("s1", "Renomear", "", true), action("s2", "Excluir", "", true), action("s3", "Copiar caminho", "", true)]);
    let more = open_toolbar.get_item_view_item(3).unwrap().base().element().unwrap();
    let (x, y) = center(&ui, more);
    ui.pointer_input(PointerInput::new(PointerInputKind::Down, x, y));
    ui.pointer_input(PointerInput::new(PointerInputKind::Up, x, y));
    toolbars.push(open_toolbar);

    // hover (o ponteiro é um só, então por último): o segundo item da barra de estados
    let li = states.get_item_element(1).unwrap();
    let hovered = ui.tree().children_with_class(li, "action-label")[0];
    hover(&ui, hovered);
    label(&ui, 300.0, 194.0, "<- hover em Atualizar, foco em Excluir", fg, 11.0, 400);

    println!("{}", support::save_png(&ui, bg, WIDTH, HEIGHT, &format!("toolbar-{name}")).display());
    for context_view in provider.shown.borrow().iter() {
        context_view.dispose();
    }
    let _ = &toolbars;
}

/// Uma ação de menu qualquer, habilitada.
fn je_widgets_menu_action(id: &str, label: &str) -> ActionRef {
    action(id, label, "", true)
}

fn main() {
    for (name, theme) in support::themes() {
        render(name, &theme);
    }
}
