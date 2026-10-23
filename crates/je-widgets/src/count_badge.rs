//! Porte de `src/vs/base/browser/ui/countBadge/countBadge.ts` e `countBadge.css`: o [`CountBadge`], o
//! emblema arredondado com um número (as pendências de uma visão, as notificações).
//!
//! Do DOM e do CSS para o `je-ui`:
//!
//! - O `.monaco-count-badge` é um nó com as mesmas classes; o número é um nó filho de texto. A regra de CSS
//!   (`padding: 3px 5px`, `border-radius: 11px`, `font-size: 11px`, `min-width`/`min-height: 18px`,
//!   `line-height: 11px`, `box-sizing: border-box`, e a variante `.long`: `padding: 2px 3px`, `border-radius: 2px`,
//!   `min-height: auto`, `line-height: normal`) é [`CountBadge::refresh`], que mede o texto e dá o tamanho ao
//!   nó, mantendo a posição (o `display: inline-block` é o dono que posiciona). `.long` é uma classe que o dono
//!   põe no elemento (o original não a conhece): chame [`CountBadge::refresh`] depois de trocá-la.
//! - `style.backgroundColor`, `color` e a borda de 1 px viram o estilo do nó ([`ICountBadgeStyles`]).
//! - O hover do título é `getBaseLayerHoverDelegate().setupDelayedHoverAtMouse` de [`crate::hover_api`].

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::lifecycle::{Disposable, IDisposable, MutableDisposable, to_disposable};
use je_base::strings::format;
use je_render::Rgba;
use je_render::kurbo::Rect;
use je_theme::ColorTheme;
use je_ui::{Border, Content, NodeId, Role, Ui};

use crate::dom::create_el;
use crate::hover_api::{HoverContent, HoverTarget, IHoverOptions, IHoverAppearanceOptions, get_base_layer_hover_delegate};
use crate::text::{measure_text, natural_line_height, ui_font};

/// O `ICountBadgeOptions`.
#[derive(Clone, Debug, Default)]
pub struct ICountBadgeOptions {
    pub count: Option<i64>,
    pub count_format: Option<String>,
    pub title_format: Option<String>,
}

/// O `ICountBadgeStyles`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ICountBadgeStyles {
    pub badge_background: Option<Rgba>,
    pub badge_foreground: Option<Rgba>,
    pub badge_border: Option<Rgba>,
}

/// `unthemedCountStyles`.
pub const UNTHEMED_COUNT_STYLES: ICountBadgeStyles = ICountBadgeStyles { badge_background: Some(Rgba::opaque(0x4D, 0x4D, 0x4D)), badge_foreground: Some(Rgba::opaque(0xFF, 0xFF, 0xFF)), badge_border: None };

/// `defaultCountBadgeStyles` (`defaultStyles.ts`): `badge.background`, `badge.foreground` e `contrastBorder`.
pub fn default_count_badge_styles(theme: &ColorTheme) -> ICountBadgeStyles {
    ICountBadgeStyles { badge_background: theme.get_color("badge.background"), badge_foreground: theme.get_color("badge.foreground"), badge_border: theme.get_color("contrastBorder") }
}

const FONT_SIZE: f32 = 11.0;
const MIN_SIZE: f64 = 18.0;

pub struct CountBadge {
    base: Rc<Disposable>,
    weak_self: Weak<CountBadge>,
    ui: Ui,
    element: NodeId,
    text_node: NodeId,
    styles: ICountBadgeStyles,
    count: Cell<i64>,
    count_format: RefCell<String>,
    title_format: RefCell<String>,
    hover: Rc<MutableDisposable<dyn IDisposable>>,
}

impl CountBadge {
    /// Cria o emblema como último filho de `container`.
    pub fn new(ui: &Ui, container: NodeId, options: &ICountBadgeOptions, styles: ICountBadgeStyles) -> Rc<Self> {
        let (element, text_node) = {
            let mut tree = ui.tree_mut();
            let element = create_el(&mut tree, container, Role::Generic, "monaco-count-badge");
            let text_node = create_el(&mut tree, element, Role::Generic, "");
            (element, text_node)
        };
        let base = Disposable::new();
        let hover = MutableDisposable::<dyn IDisposable>::new();
        base.register(hover.clone());
        let this = Rc::new_cyclic(|weak| Self {
            base,
            weak_self: weak.clone(),
            ui: ui.clone(),
            element,
            text_node,
            styles,
            count: Cell::new(0),
            count_format: RefCell::new(options.count_format.clone().filter(|f| !f.is_empty()).unwrap_or_else(|| "{0}".to_owned())),
            title_format: RefCell::new(options.title_format.clone().unwrap_or_default()),
            hover,
        });
        {
            let ui = ui.clone();
            this.base.register(to_disposable(move || ui.remove(element)));
        }
        this.set_count(options.count.unwrap_or(0));
        this.update_hover();
        this
    }

    /// O elemento `.monaco-count-badge`.
    pub fn element(&self) -> NodeId {
        self.element
    }

    pub fn set_count(&self, count: i64) {
        self.count.set(count);
        self.refresh();
    }

    pub fn set_count_format(&self, count_format: &str) {
        *self.count_format.borrow_mut() = count_format.to_owned();
        self.refresh();
    }

    pub fn set_title_format(&self, title_format: &str) {
        *self.title_format.borrow_mut() = title_format.to_owned();
        self.update_hover();
        self.refresh();
    }

    /// `format(pattern, count)`.
    fn formatted(pattern: &str, count: i64) -> String {
        let pattern: Vec<u16> = pattern.encode_utf16().collect();
        format(&pattern, &[count.to_string().encode_utf16().collect::<Vec<u16>>()]).to_string_lossy()
    }

    fn update_hover(&self) {
        let has_title = !self.title_format.borrow().is_empty();
        if has_title && self.hover.value().is_none() {
            let (weak, element) = (self.weak_self.clone(), self.element);
            let options = Rc::new(move || {
                let content = weak.upgrade().map(|badge| Self::formatted(&badge.title_format.borrow(), badge.count.get())).unwrap_or_default();
                let mut options = IHoverOptions::new(HoverContent::Text(content), HoverTarget::Node(element));
                options.appearance = Some(IHoverAppearanceOptions { compact: Some(true), ..Default::default() });
                options
            });
            self.hover.set(Some(get_base_layer_hover_delegate().setup_delayed_hover_at_mouse(self.element, options, None)));
        } else if !has_title && self.hover.value().is_some() {
            self.hover.set(None);
        }
    }

    /// `render()` mais a regra de CSS: o texto, as cores e o tamanho do nó (a posição fica com o dono).
    pub fn refresh(&self) {
        let text = Self::formatted(&self.count_format.borrow(), self.count.get());
        let font = ui_font(FONT_SIZE, 400);
        let mut tree = self.ui.tree_mut();
        let long = tree.has_class(self.element, "long");
        let (pad_x, pad_y, radius, line_height, min_height) = if long { (3.0, 2.0, 2.0, natural_line_height(&font), 0.0) } else { (5.0, 3.0, 11.0, 11.0, MIN_SIZE) };
        let border = self.styles.badge_border.map(|color| Border { width: 1.0, color });
        let border_width = border.map_or(0.0, |b| b.width);
        let text_width = measure_text(&text, &font);
        let width = MIN_SIZE.max(text_width + 2.0 * (pad_x + border_width));
        let height = min_height.max(line_height + 2.0 * (pad_y + border_width));

        let origin = tree.bounds(self.element);
        tree.set_bounds(self.element, Rect::new(origin.x0, origin.y0, origin.x0 + width, origin.y0 + height));
        {
            let style = tree.style_mut(self.element);
            style.background = self.styles.badge_background;
            style.radius = radius;
            if border.is_some() {
                style.border = border;
            }
        }
        let color = self.styles.badge_foreground.unwrap_or(Rgba::opaque(0, 0, 0));
        let left = (width - text_width) / 2.0;
        tree.set_bounds(self.text_node, Rect::new(left, border_width + pad_y, left + text_width, border_width + pad_y + line_height));
        tree.style_mut(self.text_node).content = Content::Text { text, style: font, color, line_height };
    }
}

impl IDisposable for CountBadge {
    fn dispose(&self) {
        self.base.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "CountBadge"
    }
}
