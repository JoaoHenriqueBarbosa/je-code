//! Porte de `src/vs/workbench/browser/parts/banner/bannerPart.ts`: a `BannerPart` (o banner com o item de maior prioridade, os
//! itens pendentes, a visibilidade, o foco das ações e o `IBannerService`).
//!
//! Divergências:
//! - **Herança vira composição.** `BannerPart extends Part` é a [`BannerPart`], que guarda a [`Part`] base e o
//!   [`BannerBehavior`] (o que a subclasse sobrescreve: `createContentArea`, os limites de tamanho e o `toJSON`). O
//!   `onDidChange` sobrescrito (`_onDidChangeSize`) é [`BannerPart::on_did_change`].
//! - **DOM vira estado nativo.** O elemento é o [`ViewElement`] (classes e pai). O que o original faz com o DOM e o `ViewElement`
//!   não modela (limpar os filhos, `tabIndex`, `aria-label`, o ouvinte de foco, o ícone, a mensagem (texto ou `MarkdownString`
//!   renderizado pelo `IMarkdownRendererService`), os `Link`s criados pelo `IInstantiationService`, o `ActionBar` da ação de
//!   fechar, o foco e o `isHTMLElement(children[i])`) entra pelo trait [`BannerPartView`], passado no construtor.
//! - **Identidade do item.** `item === this.item` do original compara objetos; aqui o banner guarda cada item mostrado num
//!   `Rc` e compara por [`Rc::ptr_eq`]. Como `show(item)` recebe o item por valor, mostrar de novo o mesmo item enquanto ele já
//!   é o ativo o desenha de novo (o original não faria nada); o foco é preservado do mesmo jeito (`hadFocus`).
//! - **`Map<string, IBannerItem>`.** É um `Vec` na ordem de inserção: `delete` seguido de `set` põe o item no fim.
//! - **Fica de fora** o registro das regras de teclado (`workbench.banner.focusBanner`, `workbench.banner.focusNextAction`,
//!   `workbench.banner.focusPreviousAction`), da `FocusBannerAction` e do `registerSingleton(IBannerService, ...)`: são registros
//!   globais de comandos e de serviços, feitos pelo código que monta o workbench, e chamam estes mesmos métodos.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::dynamic::Dynamic;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, DisposableRef, DisposableStore, IDisposable};
use je_base::nls::localize;
use je_editor::services::theme_service::IThemeService;
use je_platform::contextkey::contextkey::IContextKeyService;
use je_platform::contextview::view_element::ViewElement;
use je_platform::storage::StorageService;
use je_widgets::gridview::ViewDimensions;

use crate::browser::part::{Part, PartBehavior, PartOptions};
use crate::browser::parts::part_support::ViewElementTarget;
use crate::common::contextkeys::banner_focused;
use crate::services::banner::banner_service::{BannerIcon, BannerMessage, IBannerItem, IBannerService};
use crate::services::layout::layout_service::{IWorkbenchLayoutService, Parts};

/// `readonly height: number = 26`.
pub const BANNER_HEIGHT: f64 = 26.0;

/// Tudo que o banner faz com o DOM além do que o [`ViewElement`] guarda.
pub trait BannerPartView {
    /// `element.contains(element.ownerDocument.activeElement)`.
    fn has_focus_within(&self, element: &Rc<ViewElement>) -> bool;

    /// `element.focus()`.
    fn focus(&self, element: &Rc<ViewElement>);

    /// `clearNode(element)`.
    fn clear_node(&self, element: &Rc<ViewElement>);

    /// `element.tabIndex = index`.
    fn set_tab_index(&self, element: &Rc<ViewElement>, index: i32);

    /// `element.setAttribute('aria-label', label)` ou, sem rótulo, `element.removeAttribute('aria-label')`.
    fn set_aria_label(&self, element: &Rc<ViewElement>, label: Option<&str>);

    /// `addDisposableListener(element, EventType.FOCUS, handler)`.
    fn add_focus_listener(&self, element: &Rc<ViewElement>, handler: Rc<dyn Fn()>) -> DisposableRef;

    /// O `div.icon-container` (`aria-hidden`) com o ícone do tema ou o ícone personalizado (a imagem de fundo de um `URI`).
    fn render_icon(&self, element: &Rc<ViewElement>, icon: Option<&BannerIcon>);

    /// O `div.message-container` (`aria-hidden`) com a mensagem (`getBannerMessage`): o texto num `span` ou o `MarkdownString`
    /// renderizado. O que for descartado ao trocar de item é o `DisposableRef` devolvido.
    fn render_message(&self, element: &Rc<ViewElement>, message: &BannerMessage) -> DisposableRef;

    /// O `div.message-actions-container` (sempre criado) com um `Link` de `tabIndex: -1` por ação, e o descarte deles.
    fn render_actions(&self, element: &Rc<ViewElement>, actions: &[crate::services::banner::banner_service::ILinkDescriptor]) -> DisposableRef;

    /// O `div.action-container` com o `ActionBar` da ação `banner.close` (rótulo `label`, só ícone, sem foco por tabulação), que
    /// roda `on_run` ao ser acionada, e o descarte dele.
    fn render_close_action(&self, element: &Rc<ViewElement>, label: &str, on_run: Rc<dyn Fn()>) -> DisposableRef;

    /// `children[index]` do `messageActionsContainer`: se é um elemento HTML, tira o foco por tabulação do `ActionBar`
    /// (`actionBar.setFocusable(false)`) e o foca (`actionLink.focus()`).
    fn focus_action_link(&self, index: usize);

    /// `actionBar.focus(0)`.
    fn focus_close_action(&self);
}

/// O que `BannerPart` sobrescreve de `Part`, e o estado do banner.
pub struct BannerBehavior {
    this: Weak<BannerBehavior>,
    view: Rc<dyn BannerPartView>,
    layout_service: Rc<dyn IWorkbenchLayoutService>,
    context_key_service: Rc<dyn IContextKeyService>,

    base: Rc<Disposable>,
    on_did_change_size: Rc<Emitter<Option<ViewDimensions>>>,

    element: RefCell<Option<Rc<ViewElement>>>,
    item: RefCell<Option<Rc<IBannerItem>>>,
    items: RefCell<Vec<Rc<IBannerItem>>>,
    item_disposables: Rc<DisposableStore>,
    visible: Cell<bool>,
    focused_action_index: Cell<i32>,
}

impl BannerBehavior {
    fn new(view: Rc<dyn BannerPartView>, layout_service: Rc<dyn IWorkbenchLayoutService>, context_key_service: Rc<dyn IContextKeyService>) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change_size: Rc<Emitter<Option<ViewDimensions>>> = Emitter::new();
        base.register(on_did_change_size.clone());
        let item_disposables = DisposableStore::new();
        base.register(item_disposables.clone());

        Rc::new_cyclic(|this| Self {
            this: this.clone(),
            view,
            layout_service,
            context_key_service,
            base,
            on_did_change_size,
            element: RefCell::new(None),
            item: RefCell::new(None),
            items: RefCell::new(Vec::new()),
            item_disposables,
            visible: Cell::new(false),
            focused_action_index: Cell::new(-1),
        })
    }

    /// `this.element`: só existe depois do `create`.
    fn element(&self) -> Rc<ViewElement> {
        self.element.borrow().clone().expect("o banner ainda não foi criado")
    }

    fn actions_length(&self) -> i32 {
        let item = self.item.borrow().clone();
        item.and_then(|item| item.actions.as_ref().map(Vec::len)).unwrap_or(0) as i32
    }

    fn close(&self, item: &IBannerItem) {
        self.hide(&item.id);

        // Remember choice
        if let Some(on_close) = &item.on_close {
            on_close();
        }
    }

    fn focus_action_link(&self) {
        let length = self.actions_length();
        let index = self.focused_action_index.get();

        if index < length {
            if index >= 0 {
                self.view.focus_action_link(index as usize);
            }
        } else {
            self.view.focus_close_action();
        }
    }

    fn get_aria_label(item: &IBannerItem) -> Option<String> {
        if let Some(aria_label) = item.aria_label.as_ref().filter(|label| !label.is_empty()) {
            return Some(aria_label.clone());
        }
        if let BannerMessage::Text(message) = &item.message {
            return Some(message.clone());
        }

        None
    }

    fn set_visibility(&self, visible: bool) {
        if visible != self.visible.get() {
            self.visible.set(visible);
            self.focused_action_index.set(-1);

            self.layout_service.set_part_hidden(!visible, Parts::BannerPart);
            self.on_did_change_size.fire(None);
        }
    }

    fn focus(&self) {
        self.focused_action_index.set(-1);
        self.view.focus(&self.element());
    }

    fn focus_next_action(&self) {
        let length = self.actions_length();
        let index = self.focused_action_index.get();
        self.focused_action_index.set(if index < length { index + 1 } else { 0 });

        self.focus_action_link();
    }

    fn focus_previous_action(&self) {
        let length = self.actions_length();
        let index = self.focused_action_index.get();
        self.focused_action_index.set(if index > 0 { index - 1 } else { length });

        self.focus_action_link();
    }

    fn hide(&self, id: &str) {
        self.items.borrow_mut().retain(|item| item.id != id);
        self.render_active_item();
    }

    fn show(&self, item: IBannerItem) {
        {
            let mut items = self.items.borrow_mut();
            items.retain(|candidate| candidate.id != item.id);
            items.push(Rc::new(item));
        }
        self.render_active_item();
    }

    fn render_active_item(&self) {
        let element = self.element();

        let mut item: Option<Rc<IBannerItem>> = None;
        for candidate in self.items.borrow().iter() {
            let is_better = match &item {
                None => true,
                Some(current) => candidate.priority.unwrap_or(0) >= current.priority.unwrap_or(0),
            };
            if is_better {
                item = Some(candidate.clone());
            }
        }

        let same_item = match (&item, &*self.item.borrow()) {
            (None, None) => true,
            (Some(next), Some(current)) => Rc::ptr_eq(next, current),
            _ => false,
        };
        if same_item {
            return;
        }

        let had_focus = self.view.has_focus_within(&element);
        self.item_disposables.clear();
        self.view.clear_node(&element);
        *self.item.borrow_mut() = item.clone();
        self.focused_action_index.set(-1);
        let Some(item) = item else {
            self.view.set_aria_label(&element, None);
            self.set_visibility(false);
            return;
        };
        if item.neutral.unwrap_or(false) {
            element.class_list_add("neutral");
        } else {
            element.class_list_remove("neutral");
        }

        // Banner aria label
        let aria_label = Self::get_aria_label(&item).filter(|label| !label.is_empty());
        self.view.set_aria_label(&element, aria_label.as_deref());

        // Icon
        self.view.render_icon(&element, item.icon.as_ref());

        // Message
        self.item_disposables.add(self.view.render_message(&element, &item.message));

        // Message Actions
        self.item_disposables.add(self.view.render_actions(&element, item.actions.as_deref().unwrap_or(&[])));

        // Action
        let label = item.close_label.clone().unwrap_or_else(|| localize("closeBanner", "Close Banner", &[]));
        let (this, active_item) = (self.this.clone(), item.clone());
        let close: Rc<dyn Fn()> = Rc::new(move || {
            if let Some(this) = this.upgrade() {
                this.close(&active_item);
            }
        });
        self.item_disposables.add(self.view.render_close_action(&element, &label, close));

        self.set_visibility(true);
        if had_focus {
            self.focus();
        }
    }
}

impl PartBehavior for BannerBehavior {
    fn create_content_area(&self, parent: &Rc<ViewElement>, _options: Option<&Dynamic>) -> Option<Rc<ViewElement>> {
        *self.element.borrow_mut() = Some(parent.clone());
        self.view.set_tab_index(parent, 0);

        // Restore focused action if needed
        let this = self.this.clone();
        let on_focus: Rc<dyn Fn()> = Rc::new(move || {
            if let Some(this) = this.upgrade()
                && this.focused_action_index.get() != -1
            {
                this.focus_action_link();
            }
        });
        self.base.register(self.view.add_focus_listener(parent, on_focus));

        // Track focus
        let scoped_context_key_service = self.context_key_service.create_scoped(ViewElementTarget::new(parent.clone()));
        self.base.register(scoped_context_key_service.clone());
        banner_focused().bind_to(&*scoped_context_key_service).set(Dynamic::Bool(true));

        Some(parent.clone())
    }

    fn minimum_width(&self) -> f64 {
        0.0
    }

    fn maximum_width(&self) -> f64 {
        f64::INFINITY
    }

    fn minimum_height(&self) -> f64 {
        if self.visible.get() { BANNER_HEIGHT } else { 0.0 }
    }

    fn maximum_height(&self) -> f64 {
        if self.visible.get() { BANNER_HEIGHT } else { 0.0 }
    }

    fn to_json(&self) -> Dynamic {
        Dynamic::object([("type", Dynamic::string(Parts::BannerPart.as_str()))])
    }
}

/// `BannerPart`.
pub struct BannerPart {
    part: Rc<Part>,
    behavior: Rc<BannerBehavior>,
}

impl BannerPart {
    /// `constructor(themeService, layoutService, storageService, contextKeyService, instantiationService, markdownRendererService)`:
    /// os dois últimos serviços são os da `view`.
    pub fn new(
        theme_service: Rc<dyn IThemeService>,
        layout_service: Rc<dyn IWorkbenchLayoutService>,
        storage_service: Rc<dyn StorageService>,
        context_key_service: Rc<dyn IContextKeyService>,
        view: Rc<dyn BannerPartView>,
    ) -> Rc<Self> {
        let behavior = BannerBehavior::new(view, layout_service.clone(), context_key_service);
        let part = Part::new(Parts::BannerPart.as_str(), PartOptions { has_title: false, ..PartOptions::default() }, theme_service, storage_service, layout_service, behavior.clone());

        Rc::new(Self { part, behavior })
    }

    /// A [`Part`] base.
    pub fn part(&self) -> &Rc<Part> {
        &self.part
    }

    /// `create(parent)`.
    pub fn create(&self, parent: &Rc<ViewElement>) {
        self.part.create(parent, None);
    }

    /// `height`.
    pub fn height(&self) -> f64 {
        BANNER_HEIGHT
    }

    /// `minimumWidth`.
    pub fn minimum_width(&self) -> f64 {
        self.part.minimum_width()
    }

    /// `maximumWidth`.
    pub fn maximum_width(&self) -> f64 {
        self.part.maximum_width()
    }

    /// `minimumHeight`.
    pub fn minimum_height(&self) -> f64 {
        self.part.minimum_height()
    }

    /// `maximumHeight`.
    pub fn maximum_height(&self) -> f64 {
        self.part.maximum_height()
    }

    /// `onDidChange` (o `_onDidChangeSize`).
    pub fn on_did_change(&self) -> Event<Option<ViewDimensions>> {
        self.behavior.on_did_change_size.event()
    }

    /// `toJSON()`.
    pub fn to_json(&self) -> Dynamic {
        self.part.to_json()
    }
}

impl IBannerService for BannerPart {
    fn focus(&self) {
        self.behavior.focus();
    }

    fn focus_next_action(&self) {
        self.behavior.focus_next_action();
    }

    fn focus_previous_action(&self) {
        self.behavior.focus_previous_action();
    }

    fn hide(&self, id: &str) {
        self.behavior.hide(id);
    }

    fn show(&self, item: IBannerItem) {
        self.behavior.show(item);
    }
}

impl IDisposable for BannerPart {
    fn dispose(&self) {
        self.part.dispose();
        self.behavior.base.dispose();
    }
}
