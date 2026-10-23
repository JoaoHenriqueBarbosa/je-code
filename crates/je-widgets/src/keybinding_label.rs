//! Porte de `src/vs/base/browser/ui/keybindingLabel/keybindingLabel.ts` e `keybindingLabel.css`: o
//! [`KeybindingLabel`], as teclas desenhadas como "keycaps" (`Ctrl + Shift + P`) do watermark, dos menus e da
//! paleta de comandos.
//!
//! Do DOM e do CSS para o `je-ui`:
//!
//! - A estrutura é a do original: `.monaco-keybinding` com `span.monaco-keybinding-key` (mais `.highlight` quando
//!   o trecho casou), `span.monaco-keybinding-key-separator` (o `+`) e `span.monaco-keybinding-key-chord-separator`
//!   (o espaço entre acordes). Os rótulos dos modificadores vêm do `UILabelProvider` de
//!   [`je_base::keybinding_labels`] para o sistema operacional dado, e as teclas do `ResolvedKeybinding`.
//! - A regra de CSS (`display: flex; align-items: center; line-height: 10px` no contêiner; na tecla `border:
//!   1px solid`, `border-radius: 3px`, `min-width: 12px`, `font-size: 11px`, `padding: 3px 5px`, `margin: 0 2px`,
//!   `:first-child` sem margem à esquerda e `:last-child` sem margem à direita; o separador de acordes com `width:
//!   6px`) é [`KeybindingLabel::render`], que mede o texto e posiciona os nós da esquerda para a direita, na altura
//!   do `align-items: center`. A posição do `.monaco-keybinding` fica com o dono; ele só ganha o tamanho.
//! - A cor de fundo, a da borda, a da borda de baixo e a sombra interna (`inset 0 -1px 0`) das teclas são os
//!   estilos de [`IKeybindingLabelStyles`]. A tecla é um nó com fundo e raio; o contorno é um filho (`frame`) e
//!   a borda de baixo e a sombra são tiras de 1 px por cima dele, porque o `je-ui` só tem uma cor de borda.
//!   Sem cor de borda o CSS usa `currentColor`, aqui [`KeybindingLabelOptions::foreground`].
//! - O `title` é o `aria-label` do atalho num `IManagedHover` ([`crate::hover_api`]).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::keybinding_labels::ModifierLabelProvider;
use je_base::keybindings::{ResolvedChord, ResolvedKeybinding};
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::nls::localize;
use je_base::platform::OperatingSystem;
use je_render::Rgba;
use je_render::kurbo::Rect;
use je_theme::ColorTheme;
use je_ui::{Border, Content, NodeId, Role, Tree, Ui};

use crate::dom::{clear_node, create_el};
use crate::hover_api::{HoverPlacement, IManagedHover, ManagedHoverContent, ManagedHoverContentOrFactory, get_base_layer_hover_delegate, get_default_hover_delegate};
use crate::icon_labels::resize;
use crate::text::{UI_FONT_SIZE, measure_text, ui_font};

/// O `ChordMatches`: quais partes do acorde casaram com a busca.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ChordMatches {
    pub ctrl_key: bool,
    pub shift_key: bool,
    pub alt_key: bool,
    pub meta_key: bool,
    pub key_code: bool,
}

/// O `Matches`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Matches {
    pub first_part: ChordMatches,
    pub chord_part: ChordMatches,
}

/// O `IKeybindingLabelStyles`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IKeybindingLabelStyles {
    pub keybinding_label_background: Option<Rgba>,
    pub keybinding_label_foreground: Option<Rgba>,
    pub keybinding_label_border: Option<Rgba>,
    pub keybinding_label_bottom_border: Option<Rgba>,
    pub keybinding_label_shadow: Option<Rgba>,
}

/// O `KeybindingLabelOptions` (estende os estilos).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeybindingLabelOptions {
    pub styles: IKeybindingLabelStyles,
    pub render_unbound_keybindings: bool,
    /// Não mostra o atalho como `title` (hover) do rótulo.
    pub disable_title: bool,
    /// A cor do texto que o elemento herdaria do dono (o `color` do CSS sem `keybindingLabelForeground`).
    pub foreground: Rgba,
}

/// `unthemedKeybindingLabelOptions`: sem cores (o texto herda; aqui, preto, o `canvastext` inicial).
pub fn unthemed_keybinding_label_options() -> KeybindingLabelOptions {
    KeybindingLabelOptions { styles: IKeybindingLabelStyles::default(), render_unbound_keybindings: false, disable_title: false, foreground: Rgba::opaque(0, 0, 0) }
}

/// `defaultKeybindingLabelStyles` (`defaultStyles.ts`): `keybindingLabel.background`, `.foreground`, `.border`,
/// `.bottomBorder` e `widgetShadow`.
pub fn default_keybinding_label_styles(theme: &ColorTheme) -> IKeybindingLabelStyles {
    IKeybindingLabelStyles {
        keybinding_label_background: theme.get_color("keybindingLabel.background"),
        keybinding_label_foreground: theme.get_color("keybindingLabel.foreground"),
        keybinding_label_border: theme.get_color("keybindingLabel.border"),
        keybinding_label_bottom_border: theme.get_color("keybindingLabel.bottomBorder"),
        keybinding_label_shadow: theme.get_color("widget.shadow"),
    }
}

/// As opções com os estilos padrão do tema, e o `foreground` do tema como cor herdada.
pub fn default_keybinding_label_options(theme: &ColorTheme) -> KeybindingLabelOptions {
    KeybindingLabelOptions { styles: default_keybinding_label_styles(theme), foreground: theme.get_color("foreground").unwrap_or(Rgba::opaque(0xcc, 0xcc, 0xcc)), ..unthemed_keybinding_label_options() }
}

const LINE_HEIGHT: f64 = 10.0;
const KEY_FONT_SIZE: f32 = 11.0;
const KEY_MIN_WIDTH: f64 = 12.0;
const KEY_PADDING_X: f64 = 5.0;
const KEY_PADDING_Y: f64 = 3.0;
const KEY_BORDER: f64 = 1.0;
const KEY_MARGIN: f64 = 2.0;
const KEY_RADIUS: f64 = 3.0;
const CHORD_SEPARATOR_WIDTH: f64 = 6.0;
const KEY_HEIGHT: f64 = LINE_HEIGHT + 2.0 * (KEY_PADDING_Y + KEY_BORDER);

/// Um filho do `.monaco-keybinding` ainda sem posição.
enum Item {
    Key(NodeId, f64),
    Separator(NodeId, f64),
}

pub struct KeybindingLabel {
    base: Rc<Disposable>,
    ui: Ui,
    os: OperatingSystem,
    dom_node: NodeId,
    options: KeybindingLabelOptions,
    key_elements: RefCell<Vec<NodeId>>,
    hover: Rc<dyn IManagedHover>,
    keybinding: RefCell<Option<Rc<dyn ResolvedKeybinding>>>,
    matches: Cell<Option<Matches>>,
    did_ever_render: Cell<bool>,
}

impl KeybindingLabel {
    /// Cria o rótulo como último filho de `container`.
    pub fn new(ui: &Ui, container: NodeId, os: OperatingSystem, options: Option<KeybindingLabelOptions>) -> Rc<Self> {
        let options = options.unwrap_or_else(unthemed_keybinding_label_options);
        let dom_node = create_el(&mut ui.tree_mut(), container, Role::Generic, "monaco-keybinding");
        let hover = get_base_layer_hover_delegate().setup_managed_hover(get_default_hover_delegate(HoverPlacement::Mouse), dom_node, ManagedHoverContentOrFactory::Content(ManagedHoverContent::Text(String::new())), None);
        Rc::new(Self { base: Disposable::new(), ui: ui.clone(), os, dom_node, options, key_elements: RefCell::new(Vec::new()), hover, keybinding: RefCell::new(None), matches: Cell::new(None), did_ever_render: Cell::new(false) })
    }

    /// O elemento `.monaco-keybinding`.
    pub fn element(&self) -> NodeId {
        self.dom_node
    }

    /// As teclas (`span.monaco-keybinding-key`) renderizadas, na ordem.
    pub fn key_elements(&self) -> Vec<NodeId> {
        self.key_elements.borrow().clone()
    }

    /// `set(keybinding, matches)`.
    pub fn set(&self, keybinding: Option<Rc<dyn ResolvedKeybinding>>, matches: Option<Matches>) {
        let same_keybinding = match (&*self.keybinding.borrow(), &keybinding) {
            (Some(a), Some(b)) => Rc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        };
        if self.did_ever_render.get() && same_keybinding && self.matches.get() == matches {
            return;
        }

        *self.keybinding.borrow_mut() = keybinding;
        self.matches.set(matches);
        self.render();
    }

    fn render(&self) {
        self.clear();

        let mut items = Vec::new();
        let keybinding = self.keybinding.borrow().clone();
        if let Some(keybinding) = keybinding {
            let chords = keybinding.get_chords();
            let matches = self.matches.get();
            if let Some(first) = chords.first() {
                self.render_chord(&mut items, first, matches.map(|m| m.first_part));
            }
            for chord in chords.iter().skip(1) {
                let node = create_el(&mut self.ui.tree_mut(), self.dom_node, Role::Generic, "monaco-keybinding-key-chord-separator");
                items.push(Item::Separator(node, CHORD_SEPARATOR_WIDTH));
                self.render_chord(&mut items, chord, matches.map(|m| m.chord_part));
            }
            let title = if self.options.disable_title { None } else { keybinding.get_aria_label().filter(|l| !l.is_empty()) };
            self.hover.update(ManagedHoverContentOrFactory::Content(title.clone().map_or(ManagedHoverContent::Undefined, ManagedHoverContent::Text)), None);
            self.ui.tree_mut().set_label(self.dom_node, title.unwrap_or_default());
        } else if self.options.render_unbound_keybindings {
            let key = self.create_key_element(&localize("unbound", "Unbound", &[]), false);
            items.push(key);
        }

        self.layout(&items);
        self.did_ever_render.set(true);
    }

    fn clear(&self) {
        clear_node(&mut self.ui.tree_mut(), self.dom_node);
        self.key_elements.borrow_mut().clear();
    }

    fn render_chord(&self, items: &mut Vec<Item>, chord: &ResolvedChord, matched: Option<ChordMatches>) {
        let labels = ModifierLabelProvider::ui().modifier_labels(self.os).clone();
        let matched = matched.unwrap_or_default();
        for (on, label, highlight) in [(chord.ctrl_key, &labels.ctrl_key, matched.ctrl_key), (chord.shift_key, &labels.shift_key, matched.shift_key), (chord.alt_key, &labels.alt_key, matched.alt_key), (chord.meta_key, &labels.meta_key, matched.meta_key)] {
            if on {
                self.render_key(items, label, highlight, &labels.separator);
            }
        }
        if let Some(key_label) = chord.key_label.as_deref().filter(|l| !l.is_empty()) {
            self.render_key(items, key_label, matched.key_code, "");
        }
    }

    fn render_key(&self, items: &mut Vec<Item>, label: &str, highlight: bool, separator: &str) {
        items.push(self.create_key_element(label, highlight));
        if !separator.is_empty() {
            let font = ui_font(UI_FONT_SIZE, 400);
            let mut tree = self.ui.tree_mut();
            let node = create_el(&mut tree, self.dom_node, Role::Generic, "monaco-keybinding-key-separator");
            tree.style_mut(node).content = Content::Text { text: separator.to_owned(), style: font.clone(), color: self.text_color(), line_height: LINE_HEIGHT };
            items.push(Item::Separator(node, measure_text(separator, &font)));
        }
    }

    fn text_color(&self) -> Rgba {
        self.options.styles.keybinding_label_foreground.unwrap_or(self.options.foreground)
    }

    /// `createKeyElement(label, extraClass)`: o nó da tecla, com os filhos de pintura e o texto.
    fn create_key_element(&self, label: &str, highlight: bool) -> Item {
        let (styles, font) = (self.options.styles, ui_font(KEY_FONT_SIZE, 400));
        let text_width = measure_text(label, &font);
        let content_width = KEY_MIN_WIDTH.max(text_width);
        let width = content_width + 2.0 * (KEY_PADDING_X + KEY_BORDER);
        let mut tree = self.ui.tree_mut();
        let key = create_el(&mut tree, self.dom_node, Role::Generic, if highlight { "monaco-keybinding-key highlight" } else { "monaco-keybinding-key" });
        {
            let style = tree.style_mut(key);
            style.background = styles.keybinding_label_background;
            style.radius = KEY_RADIUS;
        }
        tree.set_bounds(key, Rect::new(0.0, 0.0, width, KEY_HEIGHT));
        let border_color = styles.keybinding_label_border.unwrap_or(self.text_color());

        // O contorno de 1 px (`border-style: solid`).
        let frame = create_el(&mut tree, key, Role::Presentation, "");
        tree.set_bounds(frame, Rect::new(0.0, 0.0, width, KEY_HEIGHT));
        {
            let style = tree.style_mut(frame);
            style.border = Some(Border { width: KEY_BORDER, color: border_color });
            style.radius = KEY_RADIUS;
        }
        // `box-shadow: inset 0 -1px 0 <sombra>`: uma tira de 1 px colada por dentro da borda de baixo.
        if let Some(shadow) = styles.keybinding_label_shadow {
            let strip = create_el(&mut tree, key, Role::Presentation, "");
            tree.set_bounds(strip, Rect::new(KEY_BORDER + 1.0, KEY_HEIGHT - KEY_BORDER - 1.0, width - KEY_BORDER - 1.0, KEY_HEIGHT - KEY_BORDER));
            tree.style_mut(strip).background = Some(shadow);
        }
        // `border-bottom-color`: a borda de baixo, por cima do contorno.
        if let Some(bottom) = styles.keybinding_label_bottom_border {
            let strip = create_el(&mut tree, key, Role::Presentation, "");
            tree.set_bounds(strip, Rect::new(KEY_RADIUS, KEY_HEIGHT - KEY_BORDER, width - KEY_RADIUS, KEY_HEIGHT));
            tree.style_mut(strip).background = Some(bottom);
        }
        let text = create_el(&mut tree, key, Role::Generic, "");
        let left = KEY_BORDER + KEY_PADDING_X + (content_width - text_width) / 2.0;
        let top = KEY_BORDER + KEY_PADDING_Y;
        tree.set_bounds(text, Rect::new(left, top, left + text_width, top + LINE_HEIGHT));
        tree.style_mut(text).content = Content::Text { text: label.to_owned(), style: font, color: self.text_color(), line_height: LINE_HEIGHT };
        self.key_elements.borrow_mut().push(key);
        Item::Key(key, width)
    }

    /// O flex: da esquerda para a direita, centrado na vertical, com as margens da tecla.
    fn layout(&self, items: &[Item]) {
        let mut tree = self.ui.tree_mut();
        let height = if items.iter().any(|i| matches!(i, Item::Key(..))) { KEY_HEIGHT } else if items.is_empty() { 0.0 } else { LINE_HEIGHT };
        let mut x = 0.0;
        for (i, item) in items.iter().enumerate() {
            match *item {
                Item::Key(node, width) => {
                    // `:first-child` e `:last-child` zeram a margem do lado de fora.
                    x += if i == 0 { 0.0 } else { KEY_MARGIN };
                    place(&mut tree, node, x, width, KEY_HEIGHT, height);
                    x += width + if i + 1 == items.len() { 0.0 } else { KEY_MARGIN };
                }
                Item::Separator(node, width) => {
                    place(&mut tree, node, x, width, LINE_HEIGHT, height);
                    x += width;
                }
            }
        }
        resize(&mut tree, self.dom_node, x, height);
    }
}

fn place(tree: &mut Tree, node: NodeId, x: f64, width: f64, node_height: f64, container_height: f64) {
    let top = (container_height - node_height) / 2.0;
    tree.set_bounds(node, Rect::new(x, top, x + width, top + node_height));
}

impl IDisposable for KeybindingLabel {
    fn dispose(&self) {
        self.base.dispose();
        self.hover.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "KeybindingLabel"
    }
}
