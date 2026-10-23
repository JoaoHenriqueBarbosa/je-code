//! Galeria dos widgets de rótulo: CountBadge, ProgressBar, KeybindingLabel, HighlightedLabel, IconLabel e
//! BreadcrumbsWidget, nos dois temas. Grava `wip/scratch/gallery/labels-<tema>.png`.
#[path = "support/mod.rs"]
mod support;
#[path = "../tests/keybinding_fixture/mod.rs"]
mod keybinding_fixture;

use std::any::Any;
use std::rc::Rc;

use je_base::filters::Match;
use je_base::platform::OperatingSystem;
use je_base::runtime;
use je_base::themables::ThemeIcon;
use je_render::Rgba;
use je_render::kurbo::Rect;
use je_theme::ColorTheme;
use je_ui::{NodeId, Role, Ui};
use je_widgets::breadcrumbs_widget::{BreadcrumbsItem, BreadcrumbsItemRef, BreadcrumbsWidget, default_breadcrumbs_widget_styles};
use je_widgets::count_badge::{CountBadge, ICountBadgeOptions, default_count_badge_styles};
use je_widgets::dom::create_el;
use je_widgets::highlighted_label::{HighlightedLabel, IHighlight, default_highlighted_label_styles};
use je_widgets::icon_label::{IIconLabelCreationOptions, IIconLabelValueOptions, IconLabel, IconPath, default_icon_label_styles};
use je_widgets::keybinding_label::{ChordMatches, KeybindingLabel, Matches, default_keybinding_label_options};
use je_widgets::progress_bar::{IProgressBarOptions, ProgressBar, default_progress_bar_styles};
use je_widgets::text::{UI_FONT_SIZE, set_node_text, ui_font};
use keybinding_fixture::FixtureKeybinding;

/// Título, sistema operacional, acordes e realces de um caso da galeria do `KeybindingLabel`.
type KeybindingCase = (&'static str, OperatingSystem, Vec<keybinding_fixture::ChordSpec>, Option<Matches>);

const WIDTH: u16 = 800;
const ROW: f64 = 30.0;
const LABEL_X: f64 = 12.0;
const WIDGET_X: f64 = 280.0;

/// Um item de breadcrumbs: um `IconLabel` com o ícone de arquivo.
struct FileItem {
    name: &'static str,
    theme: ColorTheme,
}

impl BreadcrumbsItem for FileItem {
    fn dispose(&self) {}

    fn equals(&self, other: &dyn BreadcrumbsItem) -> bool {
        other.as_any().downcast_ref::<FileItem>().is_some_and(|o| o.name == self.name)
    }

    fn render(&self, ui: &Ui, container: NodeId, foreground: Rgba) {
        let mut styles = default_icon_label_styles(&self.theme);
        styles.text.foreground = foreground;
        let label = IconLabel::new(ui, container, None, styles);
        label.set_label(self.name.into(), None, Some(&IIconLabelValueOptions { icon_path: Some(IconPath::ThemeIcon(ThemeIcon::from_id("symbol-file"))), ..Default::default() }));
        // O rótulo vive enquanto a árvore: o `IconLabel` do item de breadcrumbs só é descartado com o item.
        std::mem::forget(label);
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

fn move_to(ui: &Ui, node: NodeId, x: f64, y: f64) {
    let mut tree = ui.tree_mut();
    let r = tree.bounds(node);
    tree.set_bounds(node, Rect::new(x, y, x + r.width(), y + r.height()));
}

fn title(ui: &Ui, theme: &ColorTheme, row: usize, text: &str) {
    let mut tree = ui.tree_mut();
    let root = tree.root();
    let node = create_el(&mut tree, root, Role::Generic, "");
    let font = ui_font(UI_FONT_SIZE - 1.0, 400);
    let width = set_node_text(&mut tree, node, text, &font, theme.get_color("descriptionForeground").expect("descriptionForeground"), ROW);
    tree.set_bounds(node, Rect::new(LABEL_X, row as f64 * ROW, LABEL_X + width, (row + 1) as f64 * ROW));
}

fn row_node(ui: &Ui, row: usize, width: f64) -> NodeId {
    let mut tree = ui.tree_mut();
    let root = tree.root();
    let node = create_el(&mut tree, root, Role::Generic, "");
    tree.set_bounds(node, Rect::new(WIDGET_X, row as f64 * ROW, WIDGET_X + width, (row + 1) as f64 * ROW));
    node
}

fn main() {
    for (name, theme) in support::themes() {
        let ui = Ui::new();
        let background = theme.get_color("editor.background").expect("editor.background");
        let height = 20 * ROW as u16;
        {
            let mut tree = ui.tree_mut();
            let root = tree.root();
            tree.set_bounds(root, Rect::new(0.0, 0.0, f64::from(WIDTH), f64::from(height)));
        }
        let mut row = 0;

        // CountBadge
        title(&ui, &theme, row, "CountBadge: 0, 7, 42, 1234, long");
        let host = row_node(&ui, row, 400.0);
        let mut x = 0.0;
        for (count, long) in [(0, false), (7, false), (42, false), (1234, false), (3, true)] {
            let badge = CountBadge::new(&ui, host, &ICountBadgeOptions { count: Some(count), ..Default::default() }, default_count_badge_styles(&theme));
            if long {
                ui.tree_mut().add_class(badge.element(), "long");
                badge.refresh();
            }
            move_to(&ui, badge.element(), x, 6.0);
            x += ui.tree().bounds(badge.element()).width() + 10.0;
            std::mem::forget(badge);
        }
        row += 1;

        // ProgressBar
        let styles = default_progress_bar_styles(&theme);
        let options = IProgressBarOptions { styles, aria_label: None };
        let mut bars = Vec::new();
        for (text, setup) in [("ProgressBar: parado em 0", 0), ("ProgressBar: 50 %", 1), ("ProgressBar: 100 %", 2)] {
            title(&ui, &theme, row, text);
            let host = row_node(&ui, row, 400.0);
            let bar = ProgressBar::new(&ui, host, Some(&options));
            match setup {
                1 => {
                    bar.set_worked(50.0);
                }
                2 => {
                    bar.set_worked(100.0);
                }
                _ => {}
            }
            move_to(&ui, bar.get_container(), 0.0, 14.0);
            bars.push(bar);
            row += 1;
        }
        title(&ui, &theme, row, "ProgressBar: infinito (1 s e 2 s)");
        let host = row_node(&ui, row, 400.0);
        runtime::with_faked_timers(|| {
            let (first, second) = (ProgressBar::new(&ui, host, Some(&options)), ProgressBar::new(&ui, host, Some(&options)));
            move_to(&ui, first.get_container(), 0.0, 8.0);
            move_to(&ui, second.get_container(), 0.0, 18.0);
            first.infinite();
            second.infinite();
            runtime::advance_fake_time(1000);
            ui.run_animation_frame();
            // A primeira fica parada no quadro de 1 s: descarta o laço de quadros dela.
            je_base::lifecycle::IDisposable::dispose(&*first);
            runtime::advance_fake_time(1000);
            ui.run_animation_frame();
            // O laço de quadros da segunda também para: os quadros do resto da galeria rodam com o relógio real.
            je_base::lifecycle::IDisposable::dispose(&*second);
            bars.push(first);
            bars.push(second);
        });
        row += 1;
        std::mem::forget(bars);

        // KeybindingLabel
        let options = default_keybinding_label_options(&theme);
        let cases: [KeybindingCase; 5] = [
            ("KeybindingLabel: Linux, um acorde", OperatingSystem::Linux, vec![(true, true, false, false, "P")], None),
            ("KeybindingLabel: Linux, dois acordes", OperatingSystem::Linux, vec![(true, false, false, false, "K"), (true, false, false, false, "S")], None),
            ("KeybindingLabel: macOS", OperatingSystem::Macintosh, vec![(false, true, false, true, "P")], None),
            ("KeybindingLabel: Windows, realce", OperatingSystem::Windows, vec![(true, false, true, false, "Delete")], Some(Matches { first_part: ChordMatches { ctrl_key: true, key_code: true, ..Default::default() }, chord_part: ChordMatches::default() })),
            ("KeybindingLabel: sem atalho", OperatingSystem::Linux, vec![], None),
        ];
        for (text, os, chords, matches) in cases {
            title(&ui, &theme, row, text);
            let host = row_node(&ui, row, 400.0);
            let label = KeybindingLabel::new(&ui, host, os, Some(options_with_unbound(options)));
            let keybinding: Option<Rc<dyn je_base::keybindings::ResolvedKeybinding>> = (!chords.is_empty()).then(|| Rc::new(FixtureKeybinding::new(os, &chords)) as Rc<_>);
            label.set(keybinding, matches);
            move_to(&ui, label.element(), 0.0, 6.0);
            std::mem::forget(label);
            row += 1;
        }

        // HighlightedLabel
        let hl_styles = default_highlighted_label_styles(&theme);
        title(&ui, &theme, row, "HighlightedLabel: matches e ícone");
        let host = row_node(&ui, row, 480.0);
        let first = HighlightedLabel::new(&ui, host, None, hl_styles);
        first.set(Some("Hello World"), &[IHighlight::new(0, 2), IHighlight::new(6, 9)], "", false, false);
        move_to(&ui, first.element(), 0.0, 4.0);
        let second = HighlightedLabel::new(&ui, host, None, hl_styles);
        second.set(Some("$(symbol-class) MyClass"), &[IHighlight::new(16, 18)], "", false, true);
        move_to(&ui, second.element(), 160.0, 4.0);
        std::mem::forget((first, second));
        row += 1;

        // IconLabel
        let il_styles = default_icon_label_styles(&theme);
        let m = |start, end| Match { start, end };
        let icon_cases: Vec<(&str, IIconLabelValueOptions, &str, Option<&str>)> = vec![
            ("IconLabel: matches e descrição", IIconLabelValueOptions { matches: Some(vec![m(0, 4)]), suffix: Some(".ts".to_owned()), description_matches: Some(vec![m(0, 3)]), ..Default::default() }, "filename", Some("src/description")),
            ("IconLabel: itálico", IIconLabelValueOptions { italic: true, ..Default::default() }, "filename", Some("description")),
            ("IconLabel: tachado", IIconLabelValueOptions { strikethrough: true, ..Default::default() }, "filename", Some("description")),
            ("IconLabel: negrito", IIconLabelValueOptions { bold: true, ..Default::default() }, "filename", Some("description")),
            ("IconLabel: ícone $(...)", IIconLabelValueOptions { icon_path: Some(IconPath::ThemeIcon(ThemeIcon::from_id("symbol-method"))), support_icons: Some(true), ..Default::default() }, "$(sync) Sincronizar", Some("agora")),
            ("IconLabel: desabilitado", IIconLabelValueOptions { disabled_command: true, ..Default::default() }, "filename", Some("description")),
        ];
        for (text, options, label_text, description) in icon_cases {
            title(&ui, &theme, row, text);
            let host = row_node(&ui, row, 480.0);
            let label = IconLabel::new(&ui, host, Some(IIconLabelCreationOptions { support_highlights: true, support_description_highlights: true, support_icons: true, ..Default::default() }), il_styles);
            label.set_label(label_text.into(), description, Some(&options));
            move_to(&ui, label.element(), 0.0, 4.0);
            std::mem::forget(label);
            row += 1;
        }
        title(&ui, &theme, row, "IconLabel: nome em partes");
        let host = row_node(&ui, row, 480.0);
        let label = IconLabel::new(&ui, host, Some(IIconLabelCreationOptions { support_highlights: true, ..Default::default() }), il_styles);
        label.set_label(vec!["src".to_owned(), "base".to_owned(), "ui".to_owned()].into(), None, Some(&IIconLabelValueOptions { matches: Some(vec![m(4, 6)]), ..Default::default() }));
        move_to(&ui, label.element(), 0.0, 4.0);
        std::mem::forget(label);
        row += 1;

        // Breadcrumbs
        title(&ui, &theme, row, "Breadcrumbs: selecionado e focado");
        let host = row_node(&ui, row, 420.0);
        let widget = BreadcrumbsWidget::new(&ui, host, 3.0, None, ThemeIcon::from_id("chevron-right"), default_breadcrumbs_widget_styles(&theme), None);
        widget.set_inherited_foreground(theme.get_color("foreground").expect("foreground"));
        let items: Vec<BreadcrumbsItemRef> = ["src", "components", "button.ts", "render"].into_iter().map(|name| Rc::new(FileItem { name, theme: theme.clone() }) as BreadcrumbsItemRef).collect();
        widget.set_items(items.clone());
        widget.layout(Some((420.0, 30.0)));
        for _ in 0..4 {
            ui.run_animation_frame();
        }
        widget.set_selection(Some(&items[1]), None);
        widget.set_focused(Some(&items[2]), None);
        move_to(&ui, widget.element(), 0.0, 0.0);
        std::mem::forget(widget);

        println!("{}", support::save_png(&ui, background, WIDTH, height, &format!("labels-{name}")).display());
    }
}

fn options_with_unbound(options: je_widgets::keybinding_label::KeybindingLabelOptions) -> je_widgets::keybinding_label::KeybindingLabelOptions {
    je_widgets::keybinding_label::KeybindingLabelOptions { render_unbound_keybindings: true, ..options }
}
