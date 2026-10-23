//! Porte de `src/vs/base/browser/ui/iconLabel/simpleIconLabel.ts`: o [`SimpleIconLabel`], um texto com
//! ícones `$(id)` que o dono dá a um contêiner, e o `title` como hover.
//!
//! Do DOM para o `je-ui`: `reset(container, ...renderLabelWithIcons(text))` é limpar o contêiner e
//! acrescentar os nós de [`crate::icon_labels::render_label_with_icons`], posicionados em fluxo a partir
//! de `x = 0` (o contêiner é do dono: o tamanho dele não muda). Os `set text(...)` e `set title(...)` do
//! original são [`SimpleIconLabel::set_text`] e [`SimpleIconLabel::set_title`].

use std::cell::RefCell;
use std::rc::Rc;

use je_base::lifecycle::IDisposable;
use je_ui::{NodeId, Ui};

use crate::dom::clear_node;
use crate::highlighted_label::HighlightedLabelStyles;
use crate::hover_api::{HoverPlacement, IManagedHover, ManagedHoverContent, ManagedHoverContentOrFactory, get_base_layer_hover_delegate, get_default_hover_delegate};
use crate::icon_labels::{Rendered, build_parts, layout_parts, render_label_with_icons};

pub struct SimpleIconLabel {
    ui: Ui,
    container: NodeId,
    styles: HighlightedLabelStyles,
    hover: RefCell<Option<Rc<dyn IManagedHover>>>,
}

impl SimpleIconLabel {
    pub fn new(ui: &Ui, container: NodeId, styles: HighlightedLabelStyles) -> Self {
        Self { ui: ui.clone(), container, styles, hover: RefCell::new(None) }
    }

    /// `set text(text)`.
    pub fn set_text(&self, text: &str) {
        let inline = self.styles.inline_style();
        let mut tree = self.ui.tree_mut();
        clear_node(&mut tree, self.container);
        let rendered: Vec<Rendered> = build_parts(&mut tree, self.container, &render_label_with_icons(text, false), &inline);
        layout_parts(&mut tree, &rendered, inline.line_height, None);
    }

    /// `set title(title)`: cria o hover na primeira vez com um título não vazio e depois só o atualiza.
    pub fn set_title(&self, title: &str) {
        let content = ManagedHoverContentOrFactory::Content(ManagedHoverContent::Text(title.to_owned()));
        let existing = self.hover.borrow().clone();
        match existing {
            None if !title.is_empty() => {
                let delegate = get_default_hover_delegate(HoverPlacement::Mouse);
                *self.hover.borrow_mut() = Some(get_base_layer_hover_delegate().setup_managed_hover(delegate, self.container, content, None));
            }
            Some(hover) => hover.update(content, None),
            None => {}
        }
    }
}

impl IDisposable for SimpleIconLabel {
    fn dispose(&self) {
        let hover = self.hover.borrow_mut().take();
        if let Some(hover) = hover {
            hover.dispose();
        }
    }
}
