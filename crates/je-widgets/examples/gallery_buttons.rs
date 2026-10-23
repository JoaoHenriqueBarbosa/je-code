//! Galeria de `button`, `toggle` (Toggle, Checkbox, TriStateCheckbox, Switch) e `radio`, nos dois temas.
//!
//! Estados: normal, hover (o `PointerEnter` despachado ao nó), foco (o `Focus` despachado ao nó, para vários
//! ao mesmo tempo), desabilitado, marcado, com ícone. Grava `wip/scratch/gallery/buttons-<tema>.png`.
#[path = "support/mod.rs"]
mod support;

use std::rc::Rc;

use je_base::lifecycle::IDisposable;
use je_base::themables::ThemeIcon;
use je_render::Rgba;
use je_render::kurbo::Rect;
use je_theme::ColorTheme;
use je_ui::{Event, EventKind, NodeId, Role, Ui};
use je_widgets::button::{
    Button, ButtonBar, ButtonDropdownActions, ButtonOptions, ButtonWithDescription, ButtonWithDropdown, ButtonWithDropdownOptions, ContextMenuDelegate, IButton, IButtonWithDescription,
    IContextMenuProvider, default_button_styles,
};
use je_widgets::radio::{Radio, RadioOptionItem, RadioOptions, default_radio_styles};
use je_widgets::switch::{Switch, SwitchOptions, default_switch_styles};
use je_widgets::text::{natural_line_height, set_node_text, ui_font};
use je_widgets::toggle::{CheckState, Checkbox, Toggle, ToggleOpts, TriStateCheckbox, default_checkbox_styles, default_toggle_styles};

const WIDTH: u16 = 600;
const HEIGHT: u16 = 556;

struct NoContextMenu;

impl IContextMenuProvider for NoContextMenu {
    fn show_context_menu(&self, _delegate: ContextMenuDelegate) {}
}

fn dispatch(ui: &Ui, kind: EventKind, node: NodeId) {
    ui.dispatch(&mut Event::new(kind, node));
}

fn append_root(ui: &Ui, node: NodeId) {
    let root = ui.root();
    ui.tree_mut().append_child(root, node);
}

fn caption(ui: &Ui, theme: &ColorTheme, text: &str, x: f64, y: f64) {
    let mut tree = ui.tree_mut();
    let root = tree.root();
    let node = tree.create(root, Role::Generic);
    let style = ui_font(10.0, 400);
    let line = natural_line_height(&style);
    tree.set_bounds(node, Rect::new(x, y, x + 160.0, y + line));
    let color = theme.get_color("descriptionForeground").unwrap_or(Rgba::opaque(0x88, 0x88, 0x88));
    set_node_text(&mut tree, node, text, &style, color, line);
}

fn title(ui: &Ui, theme: &ColorTheme, text: &str, y: f64) {
    let mut tree = ui.tree_mut();
    let root = tree.root();
    let node = tree.create(root, Role::Heading);
    let style = ui_font(12.0, 600);
    let line = natural_line_height(&style);
    tree.set_bounds(node, Rect::new(12.0, y, 400.0, y + line));
    set_node_text(&mut tree, node, text, &style, theme.get_color("foreground").unwrap_or_default(), line);
}

fn button(ui: &Ui, theme: &ColorTheme, label: &str, x: f64, y: f64, w: f64, options: ButtonOptions) -> Rc<Button> {
    let root = ui.root();
    let options = ButtonOptions { styles: default_button_styles(theme), support_icons: true, ..options };
    let button = Button::new(ui, root, options);
    button.set_label(label.into());
    button.set_position(x, y);
    button.set_width(Some(w));
    button
}

fn render(name: &str, theme: &ColorTheme) {
    let ui = Ui::new();
    let root = ui.root();
    ui.tree_mut().set_bounds(root, Rect::new(0.0, 0.0, f64::from(WIDTH), f64::from(HEIGHT)));
    let mut keep: Vec<Rc<dyn IDisposable>> = Vec::new();

    title(&ui, theme, "Button", 6.0);
    let secondary = ButtonOptions { secondary: true, ..ButtonOptions::default() };
    let samples: Vec<(&str, &str, f64, f64, ButtonOptions)> = vec![
        ("Primary", "primary", 12.0, 90.0, ButtonOptions::default()),
        ("Secondary", "secondary", 112.0, 90.0, secondary.clone()),
        ("Small", "small", 212.0, 60.0, ButtonOptions { small: true, ..ButtonOptions::default() }),
        ("$(add) With icon", "icon", 282.0, 110.0, ButtonOptions::default()),
        ("Disabled", "disabled", 402.0, 80.0, ButtonOptions { disabled: true, ..ButtonOptions::default() }),
        ("Disabled", "disabled secondary", 490.0, 62.0, ButtonOptions { disabled: true, secondary: true, ..ButtonOptions::default() }),
    ];
    for (label, note, x, w, options) in samples {
        let b = button(&ui, theme, label, x, 24.0, w, options);
        caption(&ui, theme, note, x, 24.0 + b.size().1 + 2.0);
        keep.push(b);
    }
    let hover_primary = button(&ui, theme, "Hover", 12.0, 72.0, 90.0, ButtonOptions::default());
    dispatch(&ui, EventKind::PointerEnter, je_widgets::button::IButton::element(&*hover_primary));
    caption(&ui, theme, "hover", 12.0, 100.0);
    let focus_primary = button(&ui, theme, "Focus", 112.0, 72.0, 90.0, ButtonOptions::default());
    dispatch(&ui, EventKind::Focus, focus_primary.element());
    caption(&ui, theme, "focus", 112.0, 100.0);
    let hover_secondary = button(&ui, theme, "Hover", 212.0, 72.0, 90.0, secondary.clone());
    dispatch(&ui, EventKind::PointerEnter, hover_secondary.element());
    caption(&ui, theme, "secondary hover", 212.0, 100.0);
    let focus_secondary = button(&ui, theme, "Focus", 312.0, 72.0, 90.0, secondary);
    dispatch(&ui, EventKind::Focus, focus_secondary.element());
    caption(&ui, theme, "secondary focus", 312.0, 100.0);
    let checked = button(&ui, theme, "Checked", 412.0, 72.0, 90.0, ButtonOptions::default());
    checked.set_checked(true);
    caption(&ui, theme, "checked", 412.0, 100.0);
    keep.extend([hover_primary as Rc<dyn IDisposable>, focus_primary, hover_secondary, focus_secondary, checked]);

    // ButtonWithDescription e ButtonBar.
    let description = ButtonWithDescription::new(&ui, ui.root(), ButtonOptions { styles: default_button_styles(theme), support_icons: true, ..ButtonOptions::default() });
    description.set_label("Sign in".into());
    description.set_description("Uses your GitHub account");
    description.set_foreground(theme.get_color("foreground"));
    description.layout(Some((6.0, 116.0, 240.0)));
    caption(&ui, theme, "with description", 12.0, 186.0);
    keep.push(description);
    let bar = ButtonBar::new(&ui, ui.root(), None);
    for (index, label) in ["Yes", "No", "Cancel"].into_iter().enumerate() {
        let b = bar.add_button(ButtonOptions { styles: default_button_styles(theme), secondary: index > 0, ..ButtonOptions::default() });
        b.set_label(label.into());
        b.set_position(270.0 + 90.0 * index as f64, 130.0);
        b.set_width(Some(82.0));
    }
    caption(&ui, theme, "button bar", 270.0, 160.0);

    // ButtonWithDropdown.
    let dropdown_options = |secondary: bool, disabled: bool| ButtonWithDropdownOptions {
        button: ButtonOptions { styles: default_button_styles(theme), secondary, support_icons: true, disabled, ..ButtonOptions::default() },
        context_menu_provider: Rc::new(NoContextMenu),
        actions: ButtonDropdownActions::Actions(Vec::new()),
        action_runner: None,
        add_primary_action_to_dropdown: None,
        dropdown_layer: None,
    };
    for (index, (label, secondary, disabled, note)) in [("Commit", false, false, "dropdown"), ("Commit", true, false, "dropdown secondary"), ("Commit", false, true, "dropdown disabled")].into_iter().enumerate() {
        let d = ButtonWithDropdown::new(&ui, ui.root(), dropdown_options(secondary, disabled));
        d.set_label(label.into());
        d.set_enabled(!disabled);
        d.set_position(12.0 + 180.0 * index as f64, 214.0);
        d.set_width(Some(160.0));
        caption(&ui, theme, note, 12.0 + 180.0 * index as f64, 244.0);
        keep.push(d);
    }

    // Toggle, Checkbox, Switch.
    title(&ui, theme, "Toggle / Checkbox / Switch", 262.0);
    let toggle_styles = default_toggle_styles(theme);
    let mut x = 12.0;
    let toggle_cases = [("off", false, false, false), ("on", true, false, false), ("hover", true, false, true), ("disabled", true, true, false)];
    for (note, checked, disabled, hover) in toggle_cases {
        let t = Toggle::new(&ui, ToggleOpts { icon: Some(ThemeIcon::from_id("case-sensitive")), ..ToggleOpts::new("Match Case", checked, toggle_styles) });
        append_root(&ui, t.dom_node());
        t.set_position(x, 282.0);
        if disabled {
            t.disable();
        }
        if hover {
            dispatch(&ui, EventKind::PointerEnter, t.dom_node());
        }
        caption(&ui, theme, note, x - 2.0, 306.0);
        x += 50.0;
        keep.push(t);
    }
    let t = Toggle::new(&ui, ToggleOpts { icon: Some(ThemeIcon::from_id("regex")), ..ToggleOpts::new("Regex", false, toggle_styles) });
    append_root(&ui, t.dom_node());
    t.set_position(x, 282.0);
    dispatch(&ui, EventKind::Focus, t.dom_node());
    caption(&ui, theme, "focus", x - 2.0, 306.0);
    keep.push(t);

    let checkbox_styles = default_checkbox_styles(theme);
    let mut x = 280.0;
    for (note, checked, disabled, focus) in [("unchecked", false, false, false), ("checked", true, false, false), ("disabled", true, true, false), ("focus", true, false, true)] {
        let c = Checkbox::new(&ui, "Option", checked, checkbox_styles.clone());
        append_root(&ui, c.dom_node());
        c.set_position(x, 282.0);
        if disabled {
            c.disable();
        }
        if focus {
            dispatch(&ui, EventKind::Focus, c.dom_node());
        }
        caption(&ui, theme, note, x - 4.0, 306.0);
        x += 66.0;
        keep.push(c);
    }
    let tri = TriStateCheckbox::new(&ui, "Mixed", CheckState::Mixed, checkbox_styles);
    append_root(&ui, tri.dom_node());
    tri.set_position(12.0, 330.0);
    caption(&ui, theme, "tri-state mixed", 12.0, 352.0);
    keep.push(tri);
    let mut x = 150.0;
    for (note, checked, disabled, focus) in [("switch off", false, false, false), ("switch on", true, false, false), ("switch disabled", true, true, false), ("switch focus", true, false, true)] {
        let s = Switch::new(&ui, SwitchOptions { checked, aria_label: "Setting".to_owned(), disabled, title: None }, default_switch_styles(theme));
        append_root(&ui, s.dom_node());
        s.set_position(x, 330.0);
        if focus {
            dispatch(&ui, EventKind::Focus, s.dom_node());
        }
        caption(&ui, theme, note, x - 4.0, 352.0);
        x += 100.0;
        keep.push(s);
    }

    // Radio.
    title(&ui, theme, "Radio", 378.0);
    let radio_styles = default_radio_styles(theme);
    let radio = |items: Vec<RadioOptionItem>, class_name: Option<&str>, x: f64, y: f64, width: f64, hover: Option<usize>, focus: Option<usize>| {
        let r = Radio::new(&ui, RadioOptions { items, class_name: class_name.map(str::to_owned), styles: radio_styles, ..RadioOptions::default() });
        append_root(&ui, r.dom_node());
        r.set_position(x, y);
        r.set_width(Some(width));
        if let Some(index) = hover {
            dispatch(&ui, EventKind::PointerEnter, r.option_elements()[index]);
        }
        if let Some(index) = focus {
            dispatch(&ui, EventKind::Focus, r.option_elements()[index]);
        }
        r
    };
    let items = |texts: &[&str], active: usize| texts.iter().enumerate().map(|(i, t)| RadioOptionItem { is_active: i == active, ..RadioOptionItem::new(*t) }).collect::<Vec<_>>();
    keep.push(radio(items(&["Low", "Medium", "High"], 1), Some("segmented"), 12.0, 398.0, 240.0, None, None));
    caption(&ui, theme, "segmented", 12.0, 432.0);
    keep.push(radio(items(&["Low", "Medium", "High"], 0), Some("segmented"), 12.0, 450.0, 240.0, Some(2), None));
    caption(&ui, theme, "segmented, hover on High", 12.0, 484.0);
    keep.push(radio(items(&["Standard", "$(zap) Fast"], 1), Some("segmented"), 280.0, 398.0, 240.0, None, Some(0)));
    caption(&ui, theme, "segmented, icon and focus", 280.0, 432.0);
    let mut multi = items(&["Low", "High", "Alternative", "Other"], 2);
    multi[2].starts_row = true;
    keep.push(radio(multi, Some("segmented"), 280.0, 450.0, 240.0, None, None));
    keep.push(radio(items(&["One", "Two", "Three"], 1), None, 12.0, 514.0, 200.0, None, None));
    keep.push(radio(items(&["One", "Two", "Three"], 0), None, 230.0, 514.0, 200.0, Some(2), None));

    let background = theme.get_color("editor.background").expect("editor.background");
    println!("{}", support::save_png(&ui, background, WIDTH, HEIGHT, &format!("buttons-{name}")).display());
    for d in keep {
        d.dispose();
    }
}

fn main() {
    for (name, theme) in support::themes() {
        render(name, &theme);
    }
}
