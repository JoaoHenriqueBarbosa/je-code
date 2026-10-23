//! Lado do je-code da verificação visual dos widgets contra o VS Code real (linhas H26 e H11 de
//! `docs/bench-report.md`).
//!
//! `oracle-harness/widget_parity.js` monta no oráculo o DOM exato de cada widget e grava, por caso e tema, o PNG e
//! um TSV com os retângulos (`wip/scratch/parity/<caso>-<tema>.{png,tsv}`). Este exemplo lê esse TSV (as MESMAS
//! posições e larguras), desenha os mesmos widgets do je-widgets, nos temas 2026 Dark e 2026 Light, e grava
//! `wip/scratch/parity-je/<caso>-<tema>.{png,tsv}` com o mesmo tamanho; o TSV daqui traz os retângulos que o je-code
//! calculou para cada widget, que o `oracle-harness/parity_report.py` compara com os do oráculo (tamanho e posição)
//! além da diferença de pixels.
//!
//! Uso: `node oracle-harness/widget_parity.js 9333 wip/scratch/parity both` e depois
//! `cargo run -p je-widgets --example gallery_parity -- [--oracle=pasta] [--out=pasta] [caso ...]`.
#[path = "support/mod.rs"]
mod support;
#[path = "../tests/keybinding_fixture/mod.rs"]
mod keybinding_fixture;

use std::any::Any;
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use je_base::actions::{Action, ActionRef, IAction, Separator, SubmenuAction};
use je_base::dynamic::Dynamic;
use je_base::filters::Match;
use je_base::platform::OperatingSystem;
use je_base::themables::ThemeIcon;
use je_render::kurbo::Rect;
use je_render::{DisplayList, render_rgba};
use je_theme::ColorTheme;
use je_ui::{Event, EventKind, NodeId, PointerInput, PointerInputKind, Role, Ui};
use je_widgets::action_bar::{ActionBar, ActionsOrientation, IActionBarOptions, IActionOptions};
use je_widgets::action_view_items::{IActionViewItemOptions, default_action_bar_styles};
use je_widgets::breadcrumbs_widget::{BreadcrumbsItem, BreadcrumbsItemRef, BreadcrumbsWidget, default_breadcrumbs_widget_styles};
use je_widgets::button::{Button, ButtonOptions, IButton, default_button_styles};
use je_widgets::count_badge::{CountBadge, ICountBadgeOptions, default_count_badge_styles};
use je_widgets::dom::create_el;
use je_widgets::context_view::ContextView;
use je_widgets::context_view_api::{ContextViewDOMPosition, IContextViewProvider};
use je_widgets::find_input::{FindInput, FindInputOptions, default_find_input_styles};
use je_widgets::find_input_toggles::{FindInputToggleOpts, regex_toggle};
use je_widgets::highlighted_label::{HighlightedLabel, IHighlight, default_highlighted_label_styles};
use je_widgets::hover_api::{HoverContent, HoverTarget, IHoverAppearanceOptions, IHoverOptions};
use je_widgets::hover_widget::{WorkbenchHoverWidget, default_hover_styles};
use je_widgets::action_view_items::IActionViewItem as _;
use je_widgets::menu::{IMenuOptions, Menu, MenuItem, default_menu_styles};
use je_widgets::menu_bar::{IMenuBarOptions, MenuBar, MenuBarMenu, default_menu_bar_styles};
use je_widgets::icon_label::{IIconLabelCreationOptions, IIconLabelValueOptions, IconLabel, IconPath, default_icon_label_styles};
use je_widgets::input_box::{InlineContextViewProvider, InputBox, InputBoxOptions, Message, MessageType, default_input_box_styles};
use je_widgets::keybinding_label::{ChordMatches, KeybindingLabel, Matches, default_keybinding_label_options};
use je_widgets::progress_bar::{IProgressBarOptions, ProgressBar, default_progress_bar_styles};
use je_widgets::select_box::{ISelectBoxDelegate, ISelectBoxOptions, ISelectOptionItem, SelectBox, default_select_box_styles};
use je_widgets::toggle::{Checkbox, Toggle, ToggleOpts, default_checkbox_styles, default_toggle_styles};
use keybinding_fixture::FixtureKeybinding;

/// Os retângulos que o oráculo mediu (`nome -> x, y, largura, altura`) e o tamanho da imagem.
struct Layout {
    size: (u16, u16),
    rects: BTreeMap<String, Rect>,
}

impl Layout {
    fn load(dir: &Path, case: &str, theme: &str) -> Layout {
        let path = dir.join(format!("{case}-{theme}.tsv"));
        let text = std::fs::read_to_string(&path).unwrap_or_else(|_| panic!("sem {}: rode oracle-harness/widget_parity.js antes", path.display()));
        let mut size = (0, 0);
        let mut rects = BTreeMap::new();
        for line in text.lines() {
            let fields: Vec<&str> = line.split('\t').collect();
            let number = |i: usize| fields[i].parse::<f64>().expect("número do TSV");
            if fields[0] == "#size" {
                size = (number(1) as u16, number(2) as u16);
            } else if !fields[0].starts_with('#') {
                rects.insert(fields[0].to_owned(), Rect::new(number(1), number(2), number(1) + number(3), number(2) + number(4)));
            }
        }
        Layout { size, rects }
    }
}

/// Os itens que o oráculo mediu num menu real: `(tipo, rótulo, atalho, desabilitado, marcado)`.
type MenuItemSpec = (String, String, String, bool, Option<bool>);

fn load_menu_items(dir: &Path, case: &str, theme: &str) -> Vec<MenuItemSpec> {
    let path = dir.join(format!("{case}-{theme}.items.tsv"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|_| panic!("sem {}: rode oracle-harness/widget_parity.js antes", path.display()));
    text.lines()
        .map(|line| {
            let f: Vec<&str> = line.split('\t').collect();
            let checked = match f.get(5).copied() {
                Some("1") => Some(true),
                Some("0") => Some(false),
                _ => None,
            };
            (f[1].to_owned(), f[2].to_owned(), f[3].to_owned(), f[4] == "1", checked)
        })
        .collect()
}

/// O que um caso monta: a árvore, os retângulos do oráculo e o que mantém os widgets vivos.
struct Ctx<'a> {
    oracle_dir: PathBuf,
    theme_name: String,
    ui: Ui,
    theme: &'a ColorTheme,
    layout: Layout,
    got: Vec<(String, Rect)>,
    keep: Vec<Box<dyn Any>>,
}

impl Ctx<'_> {
    /// O retângulo que o oráculo mediu para `name`.
    fn rect(&self, name: &str) -> Rect {
        *self.layout.rects.get(name).unwrap_or_else(|| panic!("o oráculo não mediu {name}"))
    }

    /// Um nó genérico com os limites do oráculo para `name` (o contêiner onde o widget é criado).
    fn host(&self, name: &str) -> NodeId {
        let rect = self.rect(name);
        let mut tree = self.ui.tree_mut();
        let root = tree.root();
        let node = create_el(&mut tree, root, Role::Generic, "");
        tree.set_bounds(node, rect);
        node
    }

    /// Um nó genérico sem tamanho na raiz, para widgets que se posicionam por conta própria.
    fn loose_host(&self) -> NodeId {
        let mut tree = self.ui.tree_mut();
        let root = tree.root();
        create_el(&mut tree, root, Role::Generic, "")
    }

    /// Move o nó para o canto do retângulo do oráculo de `name`, mantendo o tamanho que ele já tem.
    fn move_to_rect(&self, node: NodeId, name: &str) {
        let rect = self.rect(name);
        let mut tree = self.ui.tree_mut();
        let size = tree.bounds(node);
        tree.set_bounds(node, Rect::new(rect.x0, rect.y0, rect.x0 + size.width(), rect.y0 + size.height()));
    }

    /// Guarda o retângulo absoluto de `node` para o TSV.
    fn record(&mut self, name: &str, node: NodeId) {
        let rect = self.ui.tree().absolute_bounds(node);
        self.got.push((name.to_owned(), rect));
    }

    fn color(&self, id: &str) -> je_render::Rgba {
        self.theme.get_color(id).unwrap_or_else(|| panic!("o tema não define {id}"))
    }

    fn append_root(&self, node: NodeId) {
        let root = self.ui.root();
        self.ui.tree_mut().append_child(root, node);
    }

    fn dispatch(&self, kind: EventKind, node: NodeId) {
        self.ui.dispatch(&mut Event::new(kind, node));
    }
}

type Build = fn(&mut Ctx);

/// Os casos: nome (o mesmo do JS), chave de tema do fundo e a montagem.
const CASES: &[(&str, &str, Build)] = &[
    ("button", "editor.background", button),
    ("toggle", "editor.background", toggle),
    ("inputbox", "editor.background", inputbox),
    ("countbadge", "editor.background", countbadge),
    ("keybinding", "editor.background", keybinding),
    ("progressbar", "editor.background", progressbar),
    ("labels", "editor.background", labels),
    ("actionbar", "sideBar.background", actionbar),
    ("findinput", "sideBar.background", findinput),
    ("breadcrumbs", "editor.background", breadcrumbs),
    ("selectbox", "editor.background", selectbox),
    ("hover", "editor.background", hover),
    ("menubar", "titleBar.activeBackground", menubar),
    ("menu", "titleBar.activeBackground", menu),
];

/// O `ResolvedKeybinding` da galeria: só o rótulo (o atalho já vem formatado do oráculo).
struct LabelOnly(String);

impl je_base::keybindings::ResolvedKeybinding for LabelOnly {
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
    fn get_chords(&self) -> Vec<je_base::keybindings::ResolvedChord> {
        Vec::new()
    }
    fn get_dispatch_chords(&self) -> Vec<Option<String>> {
        Vec::new()
    }
    fn get_single_modifier_dispatch_chords(&self) -> Vec<Option<je_base::keybindings::SingleModifierChord>> {
        Vec::new()
    }
}

/// As ações de um menu real, na ordem do oráculo; o id é o rótulo, que a função de atalho usa.
fn menu_actions(items: &[MenuItemSpec]) -> (Vec<ActionRef>, je_widgets::menu::GetKeyBinding) {
    let mut shortcuts: BTreeMap<String, String> = BTreeMap::new();
    let actions = items
        .iter()
        .enumerate()
        .map(|(index, (kind, label, keybinding, disabled, checked))| -> ActionRef {
            let id = format!("item{index}");
            if !keybinding.is_empty() {
                shortcuts.insert(id.clone(), keybinding.clone());
            }
            match kind.as_str() {
                "separator" => Separator::new(),
                "submenu" => SubmenuAction::new(&id, label, vec![action("x", "x", "", true, None)], None),
                _ => action(&id, label, "", !disabled, *checked),
            }
        })
        .collect();
    let lookup: je_widgets::menu::GetKeyBinding = Rc::new(move |a: &ActionRef| shortcuts.get(a.id().as_str()).map(|label| Rc::new(LabelOnly(label.clone())) as Rc<dyn je_base::keybindings::ResolvedKeybinding>));
    (actions, lookup)
}

/// Um item de breadcrumbs: um `IconLabel` só com o nome (o DOM do editor: `.monaco-icon-label > ... > a.label-name`).
struct NameItem {
    name: &'static str,
    theme: ColorTheme,
}

impl BreadcrumbsItem for NameItem {
    fn dispose(&self) {}

    fn equals(&self, other: &dyn BreadcrumbsItem) -> bool {
        other.as_any().downcast_ref::<NameItem>().is_some_and(|o| o.name == self.name)
    }

    fn render(&self, ui: &Ui, container: NodeId, foreground: je_render::Rgba) {
        let mut styles = default_icon_label_styles(&self.theme);
        styles.text.foreground = foreground;
        let label = IconLabel::new(ui, container, None, styles);
        label.set_label(self.name.into(), None, None);
        // O rótulo vive enquanto o item de breadcrumbs.
        std::mem::forget(label);
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

fn menu(ctx: &mut Ctx) {
    let items = load_menu_items(&ctx.oracle_dir, "menu", &ctx.theme_name);
    let (actions, lookup) = menu_actions(&items);
    let holder = ctx.rect("holder");
    let styles = Rc::new(default_menu_styles(ctx.theme));
    let slot = {
        let mut tree = ctx.ui.tree_mut();
        let root = tree.root();
        let node = create_el(&mut tree, root, Role::Generic, "");
        tree.set_bounds(node, Rect::new(holder.x0, holder.y0, holder.x0, holder.y0));
        node
    };
    let menu = Menu::new(&ctx.ui, slot, &actions, IMenuOptions { enable_mnemonics: true, get_key_binding: Some(lookup), ..Default::default() }, styles);
    ctx.record("holder", menu.container());
    let nodes: Vec<NodeId> = menu.menu_items().iter().filter_map(|item| match item {
        MenuItem::Action(a) => a.container(),
        MenuItem::Separator(s) => s.base().element(),
    }).collect();
    for (index, node) in nodes.into_iter().enumerate() {
        ctx.record(&format!("item-{index}"), node);
    }
    ctx.keep.push(Box::new(menu));
}

fn menubar(ctx: &mut Ctx) {
    let bar = ctx.rect("bar");
    let styles = Rc::new(default_menu_bar_styles(ctx.theme));
    let menu_styles = default_menu_styles(ctx.theme);
    let node = {
        let mut tree = ctx.ui.tree_mut();
        let root = tree.root();
        let node = create_el(&mut tree, root, Role::Generic, "menubar");
        tree.set_bounds(node, bar);
        node
    };
    let options = IMenuBarOptions { enable_mnemonics: true, visibility: Some("classic".to_owned()), styles: Some(styles), ..Default::default() };
    let menubar = MenuBar::new(&ctx.ui, node, options, menu_styles);
    let titles = ["&File", "&Edit", "&Selection", "&View", "&Go", "&Run", "&Terminal", "&Help"];
    let menus: Vec<MenuBarMenu> = titles.iter().map(|label| MenuBarMenu { label: (*label).to_owned(), actions: vec![action("x", "x", "", true, None)] }).collect();
    menubar.push(&menus);
    menubar.update(None);
    ctx.ui.run_animation_frame();
    ctx.record("bar", node);
    let buttons: Vec<NodeId> = ctx.ui.tree().children(node).to_vec();
    for (title, button) in titles.iter().zip(buttons) {
        ctx.record(&format!("button-{}", title.replace('&', "")), button);
    }
    ctx.keep.push(Box::new(menubar));
}

fn hover(ctx: &mut Ctx) {
    let cases = [
        ("text", "Dica de texto.\nSegunda linha.", false, None),
        ("compact", "Dica compacta", true, None),
        ("long", "Um texto de dica comprido o bastante para quebrar em mais de uma linha quando passa da largura máxima do conteúdo, que é de quinhentos pixels, e depois continua por mais uma frase.", false, None),
    ];
    for (name, text, compact, width) in cases {
        let rect = ctx.rect(name);
        let root = ctx.ui.root();
        let mut options = IHoverOptions::new(HoverContent::Text(text.to_owned()), HoverTarget::Node(root));
        options.appearance = Some(IHoverAppearanceOptions { compact: Some(compact), skip_fade_in_animation: Some(true), ..Default::default() });
        let hover = WorkbenchHoverWidget::new(&ctx.ui, options, default_hover_styles(ctx.theme), None);
        hover.render(root);
        let node = hover.hover_container();
        {
            // O hover se posiciona por conta própria em volta do alvo; a captura o põe no retângulo do oráculo
            // (o posicionamento é outro assunto, medido nos testes do porte).
            let mut tree = ctx.ui.tree_mut();
            let size = tree.bounds(node);
            let w = width.unwrap_or(size.width());
            tree.set_bounds(node, Rect::new(rect.x0, rect.y0, rect.x0 + w, rect.y0 + size.height()));
        }
        ctx.record(name, node);
        ctx.keep.push(Box::new(hover));
    }
}

fn selectbox(ctx: &mut Ctx) {
    let fruits = || ["Apple", "Banana", "Cherry", "Date"].map(ISelectOptionItem::new).to_vec();
    let long = || vec![ISelectOptionItem::new("Uma opção com um texto bem comprido que não cabe"), ISelectOptionItem::new("Curta")];
    let cases: [(&str, Vec<ISelectOptionItem>, usize, bool, bool); 4] = [
        ("normal", fruits(), 1, false, false),
        ("focus", fruits(), 0, false, true),
        ("disabled", fruits(), 0, true, false),
        ("cut", long(), 0, false, false),
    ];
    for (name, options, selected, disabled, focus) in cases {
        let root = ctx.ui.root();
        let context_view = ContextView::new(&ctx.ui, root, ContextViewDOMPosition::Absolute);
        let provider: Rc<dyn IContextViewProvider> = context_view.clone();
        let select_box = SelectBox::new(&ctx.ui, options, selected, provider, default_select_box_styles(ctx.theme), Some(ISelectBoxOptions::default()));
        // O contexto que `selectBox.css` pede para o preenchimento: `.monaco-action-bar .action-item`.
        let host = {
            let rect = ctx.rect(name);
            let height = select_box.natural_size().1;
            let mut tree = ctx.ui.tree_mut();
            let bar = create_el(&mut tree, root, Role::Generic, "monaco-action-bar");
            let item = create_el(&mut tree, bar, Role::Generic, "action-item");
            let host = create_el(&mut tree, item, Role::Generic, "host");
            tree.set_bounds(bar, rect);
            tree.set_bounds(item, Rect::new(0.0, 0.0, rect.width(), height));
            tree.set_bounds(host, Rect::new(0.0, 0.0, rect.width(), height));
            host
        };
        select_box.render(host);
        select_box.set_enabled(!disabled);
        if focus {
            select_box.focus();
        }
        let _ = host;
        ctx.record(&format!("{name}-select"), select_box.select_element());
        ctx.keep.push(Box::new((select_box, context_view)));
    }
}

fn breadcrumbs(ctx: &mut Ctx) {
    let names = ["home", "john", "projects", "je-code", "sample.ts", "render"];
    let theme = ctx.theme.clone();
    let build = |ctx: &mut Ctx, host_name: &str, count: usize, state: bool| {
        let host = ctx.host(host_name);
        let widget = BreadcrumbsWidget::new(&ctx.ui, host, 3.0, None, ThemeIcon::from_id("chevron-right"), default_breadcrumbs_widget_styles(&theme), None);
        widget.set_inherited_foreground(theme.get_color("foreground").expect("foreground"));
        let items: Vec<BreadcrumbsItemRef> = names[..count].iter().map(|name| Rc::new(NameItem { name, theme: theme.clone() }) as BreadcrumbsItemRef).collect();
        widget.set_items(items.clone());
        widget.layout(Some((436.0, 22.0)));
        for _ in 0..4 {
            ctx.ui.run_animation_frame();
        }
        if state {
            widget.set_selection(Some(&items[1]), None);
            widget.set_focused(Some(&items[2]), None);
        }
        ctx.record(host_name, host);
        let nodes: Vec<NodeId> = {
            let tree = ctx.ui.tree();
            tree.subtree(widget.element()).into_iter().filter(|n| tree.has_class(*n, "monaco-breadcrumb-item")).collect()
        };
        for (index, node) in nodes.into_iter().enumerate() {
            let name = if host_name == "bar" { format!("item-{index}") } else { format!("plain-item-{index}") };
            ctx.record(&name, node);
        }
        widget
    };
    let bar = build(ctx, "bar", 6, true);
    let plain = build(ctx, "plain", 4, false);
    // `:hover` no quinto item.
    let target = ctx.rect("item-4");
    for dx in [-1.0, 0.0] {
        ctx.ui.pointer_input(PointerInput::new(PointerInputKind::Move, (target.x0 + target.x1) / 2.0 + dx, (target.y0 + target.y1) / 2.0));
    }
    ctx.keep.push(Box::new((bar, plain)));
}

fn button(ctx: &mut Ctx) {
    let secondary = ButtonOptions { secondary: true, ..ButtonOptions::default() };
    let samples: [(&str, &str, bool, ButtonOptions); 9] = [
        ("primary", "Primary", false, ButtonOptions::default()),
        ("secondary", "Secondary", false, secondary.clone()),
        ("small", "Small", false, ButtonOptions { small: true, ..ButtonOptions::default() }),
        ("icon", "$(add) With icon", false, ButtonOptions::default()),
        ("disabled", "Disabled", false, ButtonOptions { disabled: true, ..ButtonOptions::default() }),
        ("hover", "Hover", true, ButtonOptions::default()),
        ("secondary-hover", "Hover", true, secondary.clone()),
        ("disabled-secondary", "Disabled", false, ButtonOptions { disabled: true, secondary: true, ..ButtonOptions::default() }),
        ("small-icon", "$(check) Small", false, ButtonOptions { small: true, ..ButtonOptions::default() }),
    ];
    for (name, label, hover, options) in samples {
        let rect = ctx.rect(name);
        let options = ButtonOptions { styles: default_button_styles(ctx.theme), support_icons: true, ..options };
        let button = Button::new(&ctx.ui, ctx.ui.root(), options);
        button.set_label(label.into());
        button.set_position(rect.x0, rect.y0);
        button.set_width(Some(rect.width()));
        if hover {
            ctx.dispatch(EventKind::PointerEnter, button.element());
        }
        ctx.record(name, button.element());
        ctx.keep.push(Box::new(button));
    }
}

fn toggle(ctx: &mut Ctx) {
    let styles = default_toggle_styles(ctx.theme);
    for (name, icon, checked, disabled) in [
        ("toggle-off", "case-sensitive", false, false),
        ("toggle-on", "case-sensitive", true, false),
        ("toggle-disabled-on", "case-sensitive", true, true),
        ("toggle-disabled-off", "case-sensitive", false, true),
        ("toggle-hover", "regex", false, false),
        ("toggle-on-b", "whole-word", true, false),
    ] {
        let rect = ctx.rect(name);
        let toggle = Toggle::new(&ctx.ui, ToggleOpts { icon: Some(ThemeIcon::from_id(icon)), ..ToggleOpts::new("Toggle", checked, styles) });
        ctx.append_root(toggle.dom_node());
        toggle.set_position(rect.x0, rect.y0);
        if disabled {
            toggle.disable();
        }
        if name == "toggle-hover" {
            ctx.dispatch(EventKind::PointerEnter, toggle.dom_node());
        }
        ctx.record(name, toggle.dom_node());
        ctx.keep.push(Box::new(toggle));
    }
    let styles = default_checkbox_styles(ctx.theme);
    for (name, checked, disabled) in [("checkbox-unchecked", false, false), ("checkbox-checked", true, false), ("checkbox-disabled-checked", true, true), ("checkbox-disabled-unchecked", false, true)] {
        let rect = ctx.rect(name);
        let checkbox = Checkbox::new(&ctx.ui, "Option", checked, styles.clone());
        ctx.append_root(checkbox.dom_node());
        checkbox.set_position(rect.x0, rect.y0);
        if disabled {
            checkbox.disable();
        }
        ctx.record(name, checkbox.dom_node());
        ctx.keep.push(Box::new(checkbox));
    }
}

fn inputbox(ctx: &mut Ctx) {
    for (name, placeholder, value, focus, message) in [
        ("placeholder", Some("Search"), "", false, None),
        ("text", None, "src/vs/base/browser/ui", false, None),
        ("focus", None, "typing here", true, None),
        ("info", None, "info", false, Some(MessageType::Info)),
        ("warning", None, "warning", false, Some(MessageType::Warning)),
        ("error", None, "**/*.{rs,toml", false, Some(MessageType::Error)),
    ] {
        let host = ctx.host(name);
        let mut options = InputBoxOptions::new(default_input_box_styles(ctx.theme));
        options.caret_blink = false;
        options.placeholder = placeholder.map(str::to_owned);
        let provider = (message == Some(MessageType::Error)).then(|| InlineContextViewProvider::new(&ctx.ui));
        let as_provider = provider.clone().map(|p| p as Rc<dyn je_widgets::context_view_api::IContextViewProvider>);
        let input = InputBox::new(&ctx.ui, host, as_provider, options);
        input.set_value(value);
        if let Some(message_type) = message {
            let content = (message_type == MessageType::Error).then(|| "The value must be a valid pattern.".to_owned());
            input.show_message(Message { content, format_content: false, r#type: Some(message_type) }, true);
        }
        if focus {
            input.focus();
        }
        ctx.record(name, host);
        ctx.record(&format!("{name}-box"), input.element());
        ctx.record(&format!("{name}-input"), input.input_element());
        ctx.keep.push(Box::new(input));
        ctx.keep.push(Box::new(provider));
    }
}

fn countbadge(ctx: &mut Ctx) {
    let host = ctx.loose_host();
    for (name, count, long) in [("badge-0", 0, false), ("badge-7", 7, false), ("badge-42", 42, false), ("badge-1234", 1234, false), ("badge-long", 3, true)] {
        let badge = CountBadge::new(&ctx.ui, host, &ICountBadgeOptions { count: Some(count), ..Default::default() }, default_count_badge_styles(ctx.theme));
        if long {
            ctx.ui.tree_mut().add_class(badge.element(), "long");
            badge.refresh();
        }
        ctx.move_to_rect(badge.element(), name);
        ctx.record(name, badge.element());
        ctx.keep.push(Box::new(badge));
    }
}

fn keybinding(ctx: &mut Ctx) {
    type Spec = (&'static str, OperatingSystem, Vec<keybinding_fixture::ChordSpec>, Option<Matches>);
    let cases: [Spec; 5] = [
        ("kb-linux-one", OperatingSystem::Linux, vec![(true, true, false, false, "P")], None),
        ("kb-linux-chord", OperatingSystem::Linux, vec![(true, false, false, false, "K"), (true, false, false, false, "S")], None),
        ("kb-mac", OperatingSystem::Macintosh, vec![(false, true, false, true, "P")], None),
        ("kb-windows-match", OperatingSystem::Windows, vec![(true, false, true, false, "Delete")], Some(Matches { first_part: ChordMatches { ctrl_key: true, key_code: true, ..Default::default() }, chord_part: ChordMatches::default() })),
        ("kb-unbound", OperatingSystem::Linux, vec![], None),
    ];
    let host = ctx.loose_host();
    for (name, os, chords, matches) in cases {
        let options = je_widgets::keybinding_label::KeybindingLabelOptions { render_unbound_keybindings: true, ..default_keybinding_label_options(ctx.theme) };
        let label = KeybindingLabel::new(&ctx.ui, host, os, Some(options));
        let keybinding: Option<Rc<dyn je_base::keybindings::ResolvedKeybinding>> = (!chords.is_empty()).then(|| Rc::new(FixtureKeybinding::new(os, &chords)) as Rc<_>);
        label.set(keybinding, matches);
        ctx.move_to_rect(label.element(), name);
        ctx.record(name, label.element());
        ctx.keep.push(Box::new(label));
    }
}

fn progressbar(ctx: &mut Ctx) {
    let options = IProgressBarOptions { styles: default_progress_bar_styles(ctx.theme), aria_label: None };
    for (name, worked) in [("idle", None), ("half", Some(50.0)), ("full", Some(100.0))] {
        let host = ctx.host(name);
        let bar = ProgressBar::new(&ctx.ui, host, Some(&options));
        if let Some(worked) = worked {
            bar.set_worked(worked);
        }
        ctx.record(name, bar.get_container());
        ctx.keep.push(Box::new(bar));
    }
}

fn labels(ctx: &mut Ctx) {
    let m = |start, end| Match { start, end };
    let hl_styles = default_highlighted_label_styles(ctx.theme);
    let host = ctx.host("highlighted-row");
    let first = HighlightedLabel::new(&ctx.ui, host, None, hl_styles);
    first.set(Some("Hello World"), &[IHighlight::new(0, 2), IHighlight::new(6, 9)], "", false, false);
    let first_width = ctx.ui.tree().bounds(first.element()).width();
    let second = HighlightedLabel::new(&ctx.ui, host, None, hl_styles);
    second.set(Some("$(symbol-class) MyClass"), &[IHighlight::new(16, 18)], "", false, true);
    {
        let mut tree = ctx.ui.tree_mut();
        let size = tree.bounds(second.element());
        let x = first_width + 20.0;
        tree.set_bounds(second.element(), Rect::new(x, 0.0, x + size.width(), size.height()));
    }
    ctx.record("highlighted", first.element());
    ctx.record("highlighted-icon", second.element());
    ctx.keep.push(Box::new((first, second)));

    let styles = default_icon_label_styles(ctx.theme);
    let rows: [(&str, IIconLabelValueOptions, &str, &str); 4] = [
        ("iconlabel", IIconLabelValueOptions { matches: Some(vec![m(0, 4)]), suffix: Some(".ts".to_owned()), description_matches: Some(vec![m(0, 3)]), support_icons: Some(true), ..Default::default() }, "filename", "src/description"),
        ("iconlabel-italic", IIconLabelValueOptions { italic: true, ..Default::default() }, "filename", "description"),
        ("iconlabel-strike", IIconLabelValueOptions { strikethrough: true, ..Default::default() }, "filename", "description"),
        ("iconlabel-icon", IIconLabelValueOptions { icon_path: Some(IconPath::ThemeIcon(ThemeIcon::from_id("symbol-method"))), support_icons: Some(true), ..Default::default() }, "$(sync) Sincronizar", "agora"),
    ];
    for (name, options, label_text, description) in rows {
        let host = ctx.host(&format!("{name}-row"));
        let label = IconLabel::new(&ctx.ui, host, Some(IIconLabelCreationOptions { support_highlights: true, support_description_highlights: true, support_icons: true, ..Default::default() }), styles);
        label.set_label(label_text.into(), Some(description), Some(&options));
        ctx.record(name, label.element());
        ctx.keep.push(Box::new(label));
    }
}

fn action(id: &str, label: &str, class: &str, enabled: bool, checked: Option<bool>) -> ActionRef {
    let action = Action::new(id, label, class, enabled, Some(Rc::new(|_| Dynamic::Undefined.into())));
    if checked.is_some() {
        action.set_checked(checked);
    }
    action
}

/// Um item de barra de ações: ícone codicon (ou só rótulo), atalho, estado.
struct BarItem {
    icon: Option<&'static str>,
    label: &'static str,
    keybinding: Option<&'static str>,
    enabled: bool,
    checked: Option<bool>,
    separator: bool,
}

impl BarItem {
    const fn new(label: &'static str) -> Self {
        Self { icon: None, label, keybinding: None, enabled: true, checked: None, separator: false }
    }
}

/// Monta uma barra de ações no contêiner do oráculo `name` e devolve o nó de cada rótulo (`a.action-label`).
fn action_bar(ctx: &mut Ctx, name: &str, items: &[BarItem], vertical: bool, with_icons: bool) -> Rc<ActionBar> {
    let host = ctx.host(name);
    let styles = Rc::new(default_action_bar_styles(ctx.theme));
    let orientation = vertical.then_some(ActionsOrientation::Vertical);
    let bar = ActionBar::new(&ctx.ui, host, IActionBarOptions { styles: Some(styles), orientation, highlight_toggled_items: true, ..Default::default() });
    let icon_item = IActionViewItemOptions { icon: Some(true), label: Some(false), ..Default::default() };
    for (index, item) in items.iter().enumerate() {
        let (action, options): (ActionRef, IActionOptions) = if item.separator {
            (Separator::new(), IActionOptions::default())
        } else if with_icons {
            let class = format!("codicon codicon-{}", item.icon.expect("ícone"));
            (action(&index.to_string(), item.label, &class, item.enabled, item.checked), IActionOptions { item: icon_item.clone(), index: None })
        } else {
            (action(&index.to_string(), item.label, "", item.enabled, item.checked), IActionOptions { item: IActionViewItemOptions { keybinding: item.keybinding.map(str::to_owned), ..Default::default() }, index: None })
        };
        bar.push_action(action, options);
    }
    ctx.record(name, host);
    let labels: Vec<NodeId> = {
        let tree = ctx.ui.tree();
        tree.children(bar.actions_list()).iter().map(|li| tree.children_with_class(*li, "action-label")[0]).collect()
    };
    for (index, label) in labels.into_iter().enumerate() {
        ctx.record(&format!("{name}-item-{index}"), label);
    }
    bar
}

fn actionbar(ctx: &mut Ctx) {
    let icon = |icon, label| BarItem { icon: Some(icon), ..BarItem::new(label) };
    let icons = [
        icon("add", "Adicionar"),
        icon("refresh", "Atualizar"),
        BarItem { separator: true, ..BarItem::new("") },
        BarItem { enabled: false, ..icon("trash", "Excluir") },
        BarItem { checked: Some(true), ..icon("edit", "Editar") },
        icon("gear", "Configurar"),
    ];
    let icons_bar = action_bar(ctx, "bar-icons", &icons, false, true);
    // Hover no item "Atualizar": o ponteiro sobre o rótulo.
    let refresh = ctx.rect("bar-icons-item-1");
    ctx.ui.pointer_input(PointerInput::new(PointerInputKind::Move, (refresh.x0 + refresh.x1) / 2.0 - 1.0, (refresh.y0 + refresh.y1) / 2.0));
    ctx.ui.pointer_input(PointerInput::new(PointerInputKind::Move, (refresh.x0 + refresh.x1) / 2.0, (refresh.y0 + refresh.y1) / 2.0));
    let text = [
        BarItem { keybinding: Some("Ctrl+R"), ..BarItem::new("Executar") },
        BarItem { keybinding: Some("F5"), ..BarItem::new("Depurar") },
        BarItem { enabled: false, ..BarItem::new("Parar") },
        BarItem { checked: Some(true), ..BarItem::new("Fixar") },
    ];
    let text_bar = action_bar(ctx, "bar-text", &text, false, false);
    let vertical = [
        BarItem::new("Novo arquivo"),
        BarItem::new("Abrir..."),
        BarItem { separator: true, ..BarItem::new("") },
        BarItem { checked: Some(true), ..BarItem::new("Fixado") },
        BarItem { enabled: false, ..BarItem::new("Sair") },
    ];
    let vertical_bar = action_bar(ctx, "bar-vertical", &vertical, true, false);
    ctx.keep.push(Box::new((icons_bar, text_bar, vertical_bar)));
}

fn findinput(ctx: &mut Ctx) {
    let styles = default_find_input_styles(ctx.theme);
    let options = |common: bool| FindInputOptions { placeholder: Some("Find".to_owned()), show_common_find_toggles: common, caret_blink: false, ..FindInputOptions::new("Find", styles.clone()) };
    struct Row {
        name: &'static str,
        value: &'static str,
        case: bool,
        words: bool,
        regex: bool,
        common: bool,
        focus: bool,
        extra: bool,
        error: bool,
    }
    let row = |name, value, case, words, regex| Row { name, value, case, words, regex, common: true, focus: false, extra: false, error: false };
    let rows = [
        row("empty", "", false, false, false),
        row("text", "src/vs/base/browser", false, false, false),
        row("case", "TextModel", true, false, false),
        row("words-regex", "^import .* from", false, true, true),
        row("three", "(foo|bar)+", true, true, true),
        Row { focus: true, ..row("focus", "typing here", false, false, true) },
        Row { common: false, ..row("plain", "plain input", false, false, false) },
        Row { extra: true, ..row("extra", "with an extra toggle", false, false, false) },
        Row { error: true, ..row("error", "(unclosed", false, false, true) },
    ];
    for r in rows {
        let host = ctx.host(r.name);
        let provider = r.error.then(|| InlineContextViewProvider::new(&ctx.ui));
        let as_provider = provider.clone().map(|p| p as Rc<dyn je_widgets::context_view_api::IContextViewProvider>);
        let additional_toggles = if r.extra { vec![regex_toggle(&ctx.ui, FindInputToggleOpts { is_checked: true, ..FindInputToggleOpts::new(styles.toggle_styles) })] } else { Vec::new() };
        let find = FindInput::new(&ctx.ui, Some(host), as_provider, FindInputOptions { additional_toggles, ..options(r.common) });
        find.set_value(r.value);
        find.set_case_sensitive(r.case);
        find.set_whole_words(r.words);
        find.set_regex(r.regex);
        if r.error {
            find.input_box.show_message(Message { content: Some("Invalid regular expression: missing )".to_owned()), format_content: false, r#type: Some(MessageType::Error) }, true);
        }
        if r.focus {
            find.focus();
        }
        ctx.record(r.name, host);
        ctx.record(&format!("{}-find", r.name), find.dom_node());
        ctx.record(&format!("{}-box", r.name), find.input_box.element());
        ctx.keep.push(Box::new(find));
        ctx.keep.push(Box::new(provider));
    }
}

/// Monta, pinta e grava um caso num tema.
fn render(dirs: &Dirs, case: &str, background_key: &str, build: Build, theme_name: &str, theme: &ColorTheme) -> PathBuf {
    let layout = Layout::load(&dirs.oracle, case, theme_name);
    let (width, height) = layout.size;
    let ui = Ui::new();
    let root = ui.root();
    // A janela do oráculo é de 1200x800 (a captura recorta a região do caso): o que depende do tamanho da janela
    // (altura máxima de um menu, posição de uma dica) vê o mesmo espaço.
    ui.tree_mut().set_bounds(root, Rect::new(0.0, 0.0, 1200.0_f64.max(f64::from(width)), 800.0_f64.max(f64::from(height))));
    let mut ctx = Ctx { oracle_dir: dirs.oracle.clone(), theme_name: theme_name.to_owned(), ui, theme, layout, got: Vec::new(), keep: Vec::new() };
    build(&mut ctx);
    let background = ctx.color(background_key);
    let mut list = DisplayList::new();
    ctx.ui.tree().paint(&mut list, 1.0);
    let rgba = render_rgba(&list, background, width, height);
    let dir = dirs.out.clone();
    std::fs::create_dir_all(&dir).expect("pasta de saída");
    let path = dir.join(format!("{case}-{theme_name}.png"));
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(std::fs::File::create(&path).expect("saída")), u32::from(width), u32::from(height));
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header().expect("png").write_image_data(&rgba).expect("png");
    let mut tsv = format!("#size\t{width}\t{height}\n");
    for (name, rect) in &ctx.got {
        writeln!(tsv, "{name}\t{}\t{}\t{}\t{}", rect.x0, rect.y0, rect.width(), rect.height()).expect("string");
    }
    std::fs::write(dir.join(format!("{case}-{theme_name}.tsv")), tsv).expect("tsv");
    path
}

/// As pastas: a do oráculo (`--oracle=`) e a de saída (`--out=`), relativas à raiz do repositório.
struct Dirs {
    oracle: PathBuf,
    out: PathBuf,
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let option = |name: &str, default: &str| support::repo_root().join(args.iter().find_map(|a| a.strip_prefix(name)).unwrap_or(default));
    let dirs = Dirs { oracle: option("--oracle=", "wip/scratch/parity"), out: option("--out=", "wip/scratch/parity-je") };
    let only: Vec<&String> = args.iter().filter(|a| !a.starts_with("--")).collect();
    for (theme_name, theme) in support::themes() {
        for (case, background, build) in CASES {
            if only.is_empty() || only.iter().any(|o| o.as_str() == *case) {
                println!("{}", render(&dirs, case, background, *build, theme_name, &theme).display());
            }
        }
    }
}
