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

use je_base::filters::Match;
use je_base::platform::OperatingSystem;
use je_base::themables::ThemeIcon;
use je_render::kurbo::Rect;
use je_render::{DisplayList, render_rgba};
use je_theme::ColorTheme;
use je_ui::{Event, EventKind, NodeId, Role, Ui};
use je_widgets::button::{Button, ButtonOptions, IButton, default_button_styles};
use je_widgets::count_badge::{CountBadge, ICountBadgeOptions, default_count_badge_styles};
use je_widgets::dom::create_el;
use je_widgets::highlighted_label::{HighlightedLabel, IHighlight, default_highlighted_label_styles};
use je_widgets::icon_label::{IIconLabelCreationOptions, IIconLabelValueOptions, IconLabel, IconPath, default_icon_label_styles};
use je_widgets::input_box::{InlineContextViewProvider, InputBox, InputBoxOptions, Message, MessageType, default_input_box_styles};
use je_widgets::keybinding_label::{ChordMatches, KeybindingLabel, Matches, default_keybinding_label_options};
use je_widgets::progress_bar::{IProgressBarOptions, ProgressBar, default_progress_bar_styles};
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
            } else {
                rects.insert(fields[0].to_owned(), Rect::new(number(1), number(2), number(1) + number(3), number(2) + number(4)));
            }
        }
        Layout { size, rects }
    }
}

/// O que um caso monta: a árvore, os retângulos do oráculo e o que mantém os widgets vivos.
struct Ctx<'a> {
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
];

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
    let host = ctx.host("highlighted");
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
    ctx.keep.push(Box::new((first, second)));

    let styles = default_icon_label_styles(ctx.theme);
    let rows: [(&str, IIconLabelValueOptions, &str, &str); 4] = [
        ("iconlabel", IIconLabelValueOptions { matches: Some(vec![m(0, 4)]), suffix: Some(".ts".to_owned()), description_matches: Some(vec![m(0, 3)]), support_icons: Some(true), ..Default::default() }, "filename", "src/description"),
        ("iconlabel-italic", IIconLabelValueOptions { italic: true, ..Default::default() }, "filename", "description"),
        ("iconlabel-strike", IIconLabelValueOptions { strikethrough: true, ..Default::default() }, "filename", "description"),
        ("iconlabel-icon", IIconLabelValueOptions { icon_path: Some(IconPath::ThemeIcon(ThemeIcon::from_id("symbol-method"))), support_icons: Some(true), ..Default::default() }, "$(sync) Sincronizar", "agora"),
    ];
    for (name, options, label_text, description) in rows {
        let host = ctx.host(name);
        let label = IconLabel::new(&ctx.ui, host, Some(IIconLabelCreationOptions { support_highlights: true, support_description_highlights: true, support_icons: true, ..Default::default() }), styles);
        label.set_label(label_text.into(), Some(description), Some(&options));
        ctx.record(name, label.element());
        ctx.keep.push(Box::new(label));
    }
}

/// Monta, pinta e grava um caso num tema.
fn render(dirs: &Dirs, case: &str, background_key: &str, build: Build, theme_name: &str, theme: &ColorTheme) -> PathBuf {
    let layout = Layout::load(&dirs.oracle, case, theme_name);
    let (width, height) = layout.size;
    let ui = Ui::new();
    let root = ui.root();
    ui.tree_mut().set_bounds(root, Rect::new(0.0, 0.0, f64::from(width), f64::from(height)));
    let mut ctx = Ctx { ui, theme, layout, got: Vec::new(), keep: Vec::new() };
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
