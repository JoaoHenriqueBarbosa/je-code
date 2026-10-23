//! Galeria visual do `Menu` e da `MenuBar`: itens normais, com atalho, desabilitado, checado, separadores,
//! submenu (aberto), item em foco pelo teclado (contorno de seleção), item sob o ponteiro, rolagem (menu
//! mais alto que a janela) e a barra de menus (mnemônico sublinhado com Alt, e um menu aberto).
//! Grava `wip/scratch/gallery/menu-dark.png` e `menu-light.png`.
#[path = "support/mod.rs"]
mod support;

use std::rc::Rc;

use je_base::actions::{Action, ActionRef, IAction, Separator, SubmenuAction};
use je_base::dynamic::Dynamic;
use je_base::key_codes::KeyCode;
use je_base::keybindings::{ResolvedChord, ResolvedKeybinding, SingleModifierChord};
use je_render::Rgba;
use je_render::kurbo::Rect;
use je_theme::ColorTheme;
use je_ui::{KeyData, Modifiers, NodeId, PointerInput, PointerInputKind, Role, Ui};
use je_widgets::menu::{IMenuOptions, Menu, default_menu_styles};
use je_widgets::menu_bar::{IMenuBarOptions, MenuBar, MenuBarMenu, MenuBarStyles, default_menu_bar_styles};
use je_widgets::text::{set_node_text, ui_font};

const WIDTH: u16 = 1180;
const HEIGHT: u16 = 860;

/// O `ResolvedKeybinding` da galeria: só o rótulo.
struct Label(String);

impl ResolvedKeybinding for Label {
    fn get_label(&self) -> Option<String> {
        Some(self.0.clone())
    }
    fn get_aria_label(&self) -> Option<String> {
        None
    }
    fn get_electron_accelerator(&self) -> Option<String> {
        None
    }
    fn get_user_settings_label(&self) -> Option<String> {
        None
    }
    fn is_wysiwyg(&self) -> bool {
        false
    }
    fn has_multiple_chords(&self) -> bool {
        false
    }
    fn get_chords(&self) -> Vec<ResolvedChord> {
        Vec::new()
    }
    fn get_dispatch_chords(&self) -> Vec<Option<String>> {
        Vec::new()
    }
    fn get_single_modifier_dispatch_chords(&self) -> Vec<Option<SingleModifierChord>> {
        Vec::new()
    }
}

fn shortcuts() -> je_widgets::menu::GetKeyBinding {
    Rc::new(|action: &ActionRef| {
        let label = match action.id().as_str() {
            "new" => "Ctrl+N",
            "open" => "Ctrl+O",
            "save" => "Ctrl+S",
            "wrap" => "Alt+Z",
            "palette" => "Ctrl+Shift+P",
            _ => return None,
        };
        Some(Rc::new(Label(label.to_owned())) as Rc<dyn ResolvedKeybinding>)
    })
}

fn action(id: &str, label: &str, enabled: bool, checked: Option<bool>) -> ActionRef {
    let action = Action::new(id, label, "", enabled, Some(Rc::new(|_| Dynamic::Undefined.into())));
    if checked.is_some() {
        action.set_checked(checked);
    }
    action
}

/// Um nó na janela, nas coordenadas dadas, para ser o contêiner de um menu.
fn slot(ui: &Ui, x: f64, y: f64) -> NodeId {
    let mut tree = ui.tree_mut();
    let root = tree.root();
    let node = tree.create(root, Role::Generic);
    tree.set_bounds(node, Rect::new(x, y, x, y));
    node
}

/// Um texto solto na janela.
fn caption(ui: &Ui, x: f64, y: f64, text: &str, fg: Rgba) {
    let mut tree = ui.tree_mut();
    let root = tree.root();
    let node = tree.create(root, Role::Generic);
    let width = set_node_text(&mut tree, node, text, &ui_font(12.0, 400), fg, 18.0);
    tree.set_bounds(node, Rect::new(x, y, x + width, y + 18.0));
}

fn file_actions() -> Vec<ActionRef> {
    vec![
        action("new", "&New File", true, None),
        action("open", "&Open File...", true, None),
        action("save", "&Save", true, None),
        Separator::new(),
        action("disabled", "Revert File", false, None),
        Separator::new(),
        action("autosave", "Auto Save", true, Some(true)),
        action("wrap", "Word Wrap", true, Some(true)),
        action("minimap", "Minimap", true, Some(false)),
        Separator::new(),
        SubmenuAction::new("theme", "Color Theme", vec![action("dark", "Dark 2026", true, Some(true)), action("light", "Light 2026", true, Some(false)), Separator::new(), action("palette", "Browse Additional Themes...", true, None)], None),
        SubmenuAction::new("empty", "Recent (empty)", Vec::new(), None),
    ]
}

fn key(ui: &Ui, key_code: KeyCode) {
    ui.key_input(true, KeyData { key_code, modifiers: Modifiers::default() });
}

fn pointer(ui: &Ui, kind: PointerInputKind, x: f64, y: f64) {
    ui.pointer_input(PointerInput::new(kind, x, y));
}

fn center(ui: &Ui, node: NodeId) -> (f64, f64) {
    let r = ui.tree().absolute_bounds(node);
    ((r.x0 + r.x1) / 2.0, (r.y0 + r.y1) / 2.0)
}

fn render(name: &str, theme: &ColorTheme) {
    let ui = Ui::new();
    let root = ui.root();
    ui.tree_mut().set_bounds(root, Rect::new(0.0, 0.0, f64::from(WIDTH), f64::from(HEIGHT)));
    let color = |id: &str| theme.get_color(id).unwrap_or_else(|| panic!("o tema não define {id}"));
    let (background, fg) = (color("editor.background"), color("foreground"));
    let menu_styles = Rc::new(default_menu_styles(theme));
    let bar_styles = Rc::new(default_menu_bar_styles(theme));
    let _: &MenuBarStyles = &bar_styles;
    let options = || IMenuOptions { enable_mnemonics: true, get_key_binding: Some(shortcuts()), ..Default::default() };
    let mut keep = Vec::new();

    // A faixa da barra de título com duas barras de menus: Alt apertado e um menu aberto.
    let titlebar = {
        let mut tree = ui.tree_mut();
        let node = tree.create(root, Role::Generic);
        tree.set_bounds(node, Rect::new(0.0, 0.0, f64::from(WIDTH), 35.0));
        tree.style_mut(node).background = theme.get_color("titleBar.activeBackground");
        node
    };
    let _ = titlebar;
    let bar_node = |x: f64, y: f64, width: f64| {
        let mut tree = ui.tree_mut();
        let node = tree.create(root, Role::Generic);
        tree.add_class(node, "menubar");
        tree.set_bounds(node, Rect::new(x, y, x + width, y + 35.0));
        node
    };
    let bar_options = || IMenuBarOptions { enable_mnemonics: true, visibility: Some("classic".to_owned()), get_keybinding: Some(shortcuts()), styles: Some(bar_styles.clone()), ..Default::default() };
    let titles = ["&File", "&Edit", "&Selection", "&View", "&Go", "&Run", "&Terminal", "&Help"];
    let bar_menus = || titles.iter().map(|label| MenuBarMenu { label: (*label).to_owned(), actions: file_actions() }).collect::<Vec<_>>();
    let closed_bar = MenuBar::new(&ui, bar_node(10.0, 0.0, 560.0), bar_options(), (*menu_styles).clone());
    closed_bar.push(&bar_menus());
    closed_bar.update(None);
    ui.run_animation_frame();
    keep.push(closed_bar.clone());
    caption(&ui, 590.0, 8.0, "MenuBar com Alt apertado (mnemônicos sublinhados)", color("titleBar.activeForeground"));

    // Alt: sublinha os mnemônicos da barra fechada.
    ui.key_input(true, KeyData { key_code: KeyCode::Alt, modifiers: Modifiers { alt: true, ..Modifiers::default() } });

    // A segunda barra, com o primeiro menu aberto pelo clique.
    let open_bar_node = bar_node(360.0, 380.0, 560.0);
    ui.tree_mut().style_mut(open_bar_node).background = theme.get_color("titleBar.activeBackground");
    caption(&ui, 360.0, 358.0, "MenuBar com o menu File aberto (clique)", fg);
    let open_bar = MenuBar::new(&ui, open_bar_node, bar_options(), (*menu_styles).clone());
    open_bar.push(&bar_menus());
    open_bar.update(None);
    ui.run_animation_frame();
    keep.push(open_bar.clone());
    let first_button = ui.tree().children(open_bar_node)[0];
    let (x, y) = center(&ui, first_button);
    pointer(&ui, PointerInputKind::Move, x, y);
    pointer(&ui, PointerInputKind::Down, x, y);
    pointer(&ui, PointerInputKind::Up, x, y);

    // Menus soltos. Só um tem o foco do `Ui` por vez: os outros mostram o item que ficou com a classe `focused`
    // (o contorno de seleção aparece quando o item não está com o foco do DOM), então o do ponteiro vai por último.
    caption(&ui, 20.0, 46.0, "Menu: submenu aberto (Color Theme)", fg);
    let submenu_menu = Menu::new(&ui, slot(&ui, 20.0, 70.0), &file_actions(), options(), menu_styles.clone());
    submenu_menu.focus(true);
    let theme_index = submenu_menu.view_items().iter().position(|item| item.action().id() == "theme").expect("submenu");
    submenu_menu.trigger(theme_index);

    caption(&ui, 20.0, 358.0, "Menu mais alto que o espaço (rolagem)", fg);
    let many: Vec<ActionRef> = (0..40).map(|i| action(&format!("item{i}"), &format!("Item number {i}"), true, None)).collect();
    let scrolled = Menu::new(&ui, slot(&ui, 20.0, 382.0), &many, options(), menu_styles.clone());
    scrolled.focus(true);
    // Descer até o item sair da vista rola o menu (e mostra a barra de rolagem).
    for _ in 0..24 {
        key(&ui, KeyCode::DownArrow);
    }

    caption(&ui, 670.0, 46.0, "Menu: foco pelo teclado (Down)", fg);
    let keyboard_menu = Menu::new(&ui, slot(&ui, 670.0, 70.0), &file_actions(), options(), menu_styles.clone());
    keyboard_menu.focus(true);
    key(&ui, KeyCode::DownArrow);
    key(&ui, KeyCode::DownArrow);

    caption(&ui, 930.0, 46.0, "Menu: sob o ponteiro", fg);
    let hover_menu = Menu::new(&ui, slot(&ui, 930.0, 70.0), &file_actions(), options(), menu_styles.clone());
    let hover_target = hover_menu.menu_items().iter().filter_map(|i| if let je_widgets::menu::MenuItem::Action(a) = i { Some(a.clone()) } else { None }).nth(1).expect("segundo item");
    let (hx, hy) = center(&ui, hover_target.container().expect("li"));
    pointer(&ui, PointerInputKind::Move, hx - 4.0, hy);
    pointer(&ui, PointerInputKind::Move, hx, hy);

    // O `setTimeout(0)` que mostra a barra de rolagem.
    je_base::runtime::advance_fake_time(1);
    ui.run_animation_frame();
    let path = support::save_png(&ui, background, WIDTH, HEIGHT, &format!("menu-{name}"));
    println!("{}", path.display());
    drop((keyboard_menu, hover_menu, submenu_menu, scrolled, keep));
}

fn main() {
    for (name, theme) in support::themes() {
        je_base::runtime::with_faked_timers(|| render(name, &theme));
    }
}
