//! Porte de `src/vs/platform/opener/browser/link.ts` (e de `link.css`): o `Link`, uma âncora (`a.monaco-link`) que abre o
//! endereço pelo `IOpenerService` com `allowCommands` (ou pelo `opener` das opções) ao clicar, ou ao teclar `Enter` ou
//! `Space`, com dica gerenciada e o estado habilitado.
//!
//! Notas de tradução:
//! - **Injeção.** O `@IHoverService` do construtor é o hover de base de `je-widgets` (o serviço de hover do porte é global,
//!   como nas outras peças) e o `@IOpenerService` é o parâmetro `opener_service`.
//! - **`label: string | HTMLElement`** é [`LinkLabel`]. O texto vira o conteúdo do nó da âncora, na fonte da interface e na cor
//!   `textLink.foreground` das opções. Um nó vira filho da âncora; ao trocar o rótulo o nó antigo sai da âncora sem ser
//!   destruído (o `clearNode` do original só o desanexa, e quem o criou o destrói).
//! - **`href`, `tabIndex`, `aria-disabled`.** São atributos do nó (`href`, `tabindex`, `aria-disabled`), e o `tabindex` entra
//!   na ordem de foco da árvore.
//! - **`role="button"`.** O papel da âncora é [`Role::Button`], como o original o escreve.
//! - **`link.css`.** A cor (`textLink.foreground`) e o cursor `pointer` do `.monaco-link`; o sublinhado e o contorno de foco
//!   não existem no nó. O `Gesture` de toque (`Tap`) não existe: o `je-ui` ainda não tem toque.
//! - **`enabled`.** O setter do original grava `_enabled` duas vezes (a última, `enabled`, vale); aqui é só [`Link::set_enabled`].

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::JsString;
use je_base::key_codes::KeyCode;
use je_base::lifecycle::{Disposable, IDisposable, to_disposable};
use je_platform::opener::opener::{AllowCommands, IOpenerService, OpenOptions, OpenResource};
use je_render::Rgba;
use je_ui::{Content, Cursor, EventKind, NodeId, PointerEvents, Role, Ui};
use je_widgets::dom::{StandardKeyboardEvent, create_el};
use je_widgets::hover_api::{HoverPlacement, IHoverDelegate, IManagedHover, ManagedHoverContentOrFactory, get_base_layer_hover_delegate, get_default_hover_delegate};
use je_widgets::text::{UI_FONT_SIZE, WORKBENCH_LINE_HEIGHT, ui_font};

/// O `textLink.foreground` do tema escuro, quando as opções não dão cor para um rótulo de texto.
const DEFAULT_TEXT_LINK_FOREGROUND: Rgba = Rgba::new(0x37, 0x94, 0xff, 255);

/// `label: string | HTMLElement` do `ILinkDescriptor`.
pub enum LinkLabel {
    Text(String),
    Node(NodeId),
}

/// `ILinkDescriptor`.
pub struct ILinkDescriptor {
    pub label: LinkLabel,
    pub href: String,
    pub title: Option<String>,
    pub tab_index: Option<i32>,
}

/// `ILinkOptions`.
#[derive(Default)]
pub struct ILinkOptions {
    /// `opener`: no lugar do `IOpenerService`.
    pub opener: Option<Rc<dyn Fn(&str)>>,
    pub hover_delegate: Option<Rc<dyn IHoverDelegate>>,
    /// `textLinkForeground`: a cor do rótulo de texto.
    pub text_link_foreground: Option<Rgba>,
}

/// `Link`.
pub struct Link {
    base: Rc<Disposable>,
    ui: Ui,
    el: NodeId,
    hover: RefCell<Option<Rc<dyn IManagedHover>>>,
    hover_delegate: Rc<dyn IHoverDelegate>,
    link: RefCell<ILinkDescriptor>,
    enabled: Cell<bool>,
    opener: Option<Rc<dyn Fn(&str)>>,
    opener_service: Rc<dyn IOpenerService>,
    text_link_foreground: Option<Rgba>,
}

impl Link {
    /// `new Link(container, link, options, hoverService, openerService)`.
    pub fn new(ui: &Ui, container: NodeId, link: ILinkDescriptor, options: ILinkOptions, opener_service: Rc<dyn IOpenerService>) -> Rc<Self> {
        let el = create_el(&mut ui.tree_mut(), container, Role::Generic, "monaco-link");
        let hover_delegate = options.hover_delegate.unwrap_or_else(|| get_default_hover_delegate(HoverPlacement::Mouse));
        let title = link.title.clone();
        let this = Rc::new(Self {
            base: Disposable::new(),
            ui: ui.clone(),
            el,
            hover: RefCell::new(None),
            hover_delegate,
            link: RefCell::new(link),
            enabled: Cell::new(true),
            opener: options.opener,
            opener_service,
            text_link_foreground: options.text_link_foreground,
        });

        {
            let link = this.link.borrow();
            let tab_index = link.tab_index.unwrap_or(0);
            this.set_label(&link.label);
            let mut tree = ui.tree_mut();
            tree.set_attribute(el, "href", link.href.clone());
            tree.set_attribute(el, "tabindex", tab_index.to_string());
        }
        this.set_tooltip(title.as_deref());

        // this.el.setAttribute('role', 'button')
        ui.tree_mut().set_role(el, Role::Button);

        let weak = Rc::downgrade(&this);
        let on_click = weak.clone();
        this.base.register(ui.listen(el, EventKind::Click, false, move |e| {
            if let Some(this) = on_click.upgrade() {
                this.open(e);
            }
        }));
        this.base.register(ui.listen(el, EventKind::KeyDown, false, move |e| {
            if let Some(this) = weak.upgrade()
                && let Some(event) = StandardKeyboardEvent::from_event(e)
                && (event.equals(KeyCode::Enter.0 as u32) || event.equals(KeyCode::Space.0 as u32))
            {
                this.open(e);
            }
        }));

        this.set_enabled(true);
        this
    }

    /// O nó da âncora (`a.monaco-link`).
    pub fn element(&self) -> NodeId {
        self.el
    }

    /// `get enabled()`.
    pub fn enabled(&self) -> bool {
        self.enabled.get()
    }

    /// `set enabled(enabled)`.
    pub fn set_enabled(&self, enabled: bool) {
        let mut tree = self.ui.tree_mut();
        tree.set_attribute(self.el, "aria-disabled", if enabled { "false" } else { "true" });
        tree.set_attribute(self.el, "tabindex", if enabled { "0" } else { "-1" });
        let style = tree.style_mut(self.el);
        style.pointer_events = if enabled { PointerEvents::Auto } else { PointerEvents::None };
        style.opacity = if enabled { 1.0 } else { 0.4 };
        style.cursor = Some(if enabled { Cursor::Pointer } else { Cursor::Default });
        self.enabled.set(enabled);
    }

    /// `set link(link)`.
    pub fn set_link(&self, link: ILinkDescriptor) {
        self.set_label(&link.label);
        self.ui.tree_mut().set_attribute(self.el, "href", link.href.clone());

        if let Some(tab_index) = link.tab_index {
            self.ui.tree_mut().set_attribute(self.el, "tabindex", tab_index.to_string());
        }

        self.set_tooltip(link.title.as_deref());
        *self.link.borrow_mut() = link;
    }

    /// O rótulo: o texto vira o conteúdo da âncora, um nó vira o único filho dela.
    fn set_label(&self, label: &LinkLabel) {
        let mut tree = self.ui.tree_mut();
        match label {
            LinkLabel::Text(text) => {
                let color = self.text_link_foreground.unwrap_or(DEFAULT_TEXT_LINK_FOREGROUND);
                tree.style_mut(self.el).content = Content::Text { text: text.clone(), style: ui_font(UI_FONT_SIZE, 400), color, line_height: WORKBENCH_LINE_HEIGHT };
            }
            LinkLabel::Node(node) => {
                tree.style_mut(self.el).content = Content::None;
                for child in tree.children(self.el).to_vec() {
                    tree.detach(child);
                }
                tree.append_child(self.el, *node);
            }
        }
    }

    /// `setTooltip(title)`: cria a dica na primeira vez com título; depois só a atualiza.
    fn set_tooltip(&self, title: Option<&str>) {
        let existing = self.hover.borrow().clone();
        match (existing, title.filter(|title| !title.is_empty())) {
            (None, Some(title)) => {
                let hover = get_base_layer_hover_delegate().setup_managed_hover(self.hover_delegate.clone(), self.el, ManagedHoverContentOrFactory::from(title), None);
                *self.hover.borrow_mut() = Some(hover.clone());
                self.base.register(to_disposable(move || hover.dispose()));
            }
            (Some(hover), title) => hover.update(ManagedHoverContentOrFactory::from(title.unwrap_or("")), None),
            (None, None) => {}
        }
    }

    /// O `onOpen` do original: sem efeito se desabilitado; senão para o evento e abre.
    fn open(&self, e: &mut je_ui::Event) {
        if !self.enabled.get() {
            return;
        }

        e.stop(true);

        let href = self.link.borrow().href.clone();
        if let Some(opener) = &self.opener {
            opener(&href);
        } else {
            let options = OpenOptions { allow_commands: Some(AllowCommands::Bool(true)), ..OpenOptions::default() };
            let _ = self.opener_service.open(&OpenResource::Text(JsString::from(href.as_str())), Some(&options));
        }
    }
}

impl IDisposable for Link {
    fn dispose(&self) {
        self.base.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "Link"
    }
}
