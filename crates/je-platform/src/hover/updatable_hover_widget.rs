//! Porte de `src/vs/platform/hover/browser/updatableHoverWidget.ts`: o `ManagedHoverWidget`, que resolve o conteúdo de um hover
//! gerenciável, cancela a resolução anterior quando chega uma nova e mostra o hover pelo delegate.
//!
//! Divergências de tradução:
//! - **`IHoverDelegate.showHover`.** O contrato [`IHoverDelegate`] do `hover_service.rs` não carrega `showHover`; o original o
//!   chama no delegate. Aqui entra como [`IHoverShower`] (o `HoverService` ou o delegate que o embrulha), dado à
//!   [`ManagedHoverWidgetFactory`] na construção.
//! - **Conteúdo síncrono.** O `markdown` do [`ManagedHoverTooltip`] é um valor e o `element` devolve o elemento direto, então o
//!   ramo `managedContent instanceof Promise` (com o "Loading..." provisório) não existe: a resolução é síncrona, e o
//!   `CancellationTokenSource` continua criado e cancelado como no original, porque o `element(token)` o recebe.
//! - **`IHoverDelegateTarget.dispose`.** O `dispose` do alvo do original (que encerra a resolução quando este é o hover atual)
//!   é o `on_did_hide` das opções, que o `HoverService` roda quando o hover é descartado.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::cancellation::CancellationTokenSource;
use je_base::lifecycle::IDisposable;
use je_base::promise::Promise;

use crate::contextview::view_element::ViewElement;
use crate::hover::hover_service::{
    HoverContent, HoverDelegateTarget, HoverPlacement, HoverPosition, HoverPositionOption, HoverTarget, IHoverAppearanceOptions, IHoverDelegate, IHoverOptions,
    IHoverPositionOptions, IHoverWidget, IManagedHoverOptions, IManagedHoverWidget, IManagedHoverWidgetFactory, ManagedHoverContent, StructuredHoverTarget,
};

/// `hoverDelegate.showHover(options, focus)`.
pub trait IHoverShower {
    fn show_hover(&self, options: IHoverOptions, focus: bool) -> Option<Rc<dyn IHoverWidget>>;
}

/// `IManagedHoverResolvedContent`: `IMarkdownString | string | HTMLElement | undefined`.
type ManagedHoverResolvedContent = Option<HoverContent>;

/// `ManagedHoverWidget`.
pub struct ManagedHoverWidget {
    this: Weak<ManagedHoverWidget>,
    hover_delegate: Rc<dyn IHoverDelegate>,
    shower: Rc<dyn IHoverShower>,
    target: Rc<HoverDelegateTarget>,
    fade_in_animation: bool,
    hover_widget: RefCell<Option<Rc<dyn IHoverWidget>>>,
    cancellation_token_source: RefCell<Option<CancellationTokenSource>>,
    focus: Cell<Option<bool>>,
}

impl ManagedHoverWidget {
    pub fn new(hover_delegate: Rc<dyn IHoverDelegate>, shower: Rc<dyn IHoverShower>, target: Rc<HoverDelegateTarget>, fade_in_animation: bool) -> Rc<Self> {
        Rc::new_cyclic(|this| Self {
            this: this.clone(),
            hover_delegate,
            shower,
            target,
            fade_in_animation,
            hover_widget: RefCell::new(None),
            cancellation_token_source: RefCell::new(None),
            focus: Cell::new(None),
        })
    }

    /// `onDidHide()`: há uma computação em andamento, cancela.
    pub fn on_did_hide(&self) {
        self.cancel_ongoing();
    }

    fn cancel_ongoing(&self) {
        let source = self.cancellation_token_source.borrow_mut().take();
        if let Some(source) = source {
            source.dispose_with(true);
        }
    }

    /// `isDisposed` como o original: `undefined` (aqui `false`) enquanto não há hover.
    pub fn is_disposed(&self) -> bool {
        self.hover_widget.borrow().as_ref().is_some_and(|widget| widget.is_disposed())
    }

    /// `update(content, focus, options)`.
    pub fn update(&self, content: ManagedHoverContent, focus: Option<bool>, options: Option<IManagedHoverOptions>) -> Promise<()> {
        // there's an computation ongoing, cancel it
        self.cancel_ongoing();
        if self.is_disposed() {
            return Promise::resolved(());
        }

        let mut content_owns_padding = false;
        let resolved_content: ManagedHoverResolvedContent = match content {
            ManagedHoverContent::Undefined => None,
            ManagedHoverContent::Text(text) => Some(HoverContent::Text(text)),
            ManagedHoverContent::Tooltip(tooltip) => {
                // compute the content
                let source = CancellationTokenSource::new(None);
                let token = source.token();
                *self.cancellation_token_source.borrow_mut() = Some(source);

                let resolved = match &tooltip.element {
                    Some(element) => {
                        content_owns_padding = tooltip.content_owns_padding;
                        Some(HoverContent::Element(element(token.clone())))
                    }
                    None => tooltip.markdown.clone().or_else(|| tooltip.markdown_not_supported_fallback.clone().map(HoverContent::Text)),
                };

                if self.is_disposed() || token.is_cancellation_requested() {
                    // either the widget has been closed in the meantime or there has been a new call to `update`
                    return Promise::resolved(());
                }
                resolved
            }
        };

        self.show(resolved_content, focus, options, content_owns_padding);
        Promise::resolved(())
    }

    fn show(&self, content: ManagedHoverResolvedContent, focus: Option<bool>, options: Option<IManagedHoverOptions>, content_owns_padding: bool) {
        let old_hover_widget = self.hover_widget.borrow_mut().take();
        let focus = focus.or(self.focus.get());
        self.focus.set(focus);
        // A pinned hover must release its lock before its replacement is shown.
        if let Some(old) = &old_hover_widget {
            old.dispose();
        }

        let Some(content) = content.filter(has_content) else {
            return;
        };

        let options = options.unwrap_or_default();
        // Identifica o hover que este `show` cria, para o `on_did_hide` só encerrar a resolução se ele ainda for o atual.
        let created: Rc<RefCell<Option<Rc<dyn IHoverWidget>>>> = Rc::new(RefCell::new(None));

        let target = HoverTarget::Structured(StructuredHoverTarget { target_elements: self.target.target_elements.clone(), x: self.target.x.get(), y: None });
        let mut hover_options = IHoverOptions::new(content, target);
        hover_options.actions = options.actions;
        hover_options.link_handler = options.link_handler;
        hover_options.trap_focus = options.trap_focus.or(focus).unwrap_or(false);
        hover_options.on_did_show = options.on_did_show;
        hover_options.additional_classes = options.additional_classes;
        if content_owns_padding {
            hover_options.additional_classes.push("managed-hover-content-owns-padding".to_owned());
        }
        hover_options.appearance = Some(IHoverAppearanceOptions {
            show_pointer: Some(self.hover_delegate.placement() == Some(HoverPlacement::Element)),
            // do not fade in if the hover is already showing
            skip_fade_in_animation: Some(!self.fade_in_animation || old_hover_widget.is_some()),
            show_hover_hint: options.show_hover_hint,
            ..IHoverAppearanceOptions::default()
        });
        hover_options.position = Some(IHoverPositionOptions {
            hover_position: Some(HoverPositionOption::Position(HoverPosition::Below)),
            anchor_alignment: options.anchor_alignment,
            ..IHoverPositionOptions::default()
        });

        let this = self.this.clone();
        let created_ref = created.clone();
        hover_options.on_did_hide = Some(Rc::new(move || {
            let Some(widget) = this.upgrade() else {
                return;
            };
            let is_current = match (&*widget.hover_widget.borrow(), &*created_ref.borrow()) {
                (Some(current), Some(own)) => Rc::ptr_eq(current, own),
                (None, None) => true,
                _ => false,
            };
            if is_current {
                widget.on_did_hide();
            }
        }));

        let hover_widget = self.shower.show_hover(hover_options, focus.unwrap_or(false));
        *created.borrow_mut() = hover_widget.clone();
        *self.hover_widget.borrow_mut() = hover_widget;
    }
}

fn has_content(content: &HoverContent) -> bool {
    match content {
        HoverContent::Text(text) => !text.is_empty(),
        HoverContent::Markdown(markdown) => !markdown.value.to_string().is_empty(),
        HoverContent::Element(_) => true,
    }
}

impl IDisposable for ManagedHoverWidget {
    fn dispose(&self) {
        let widget = self.hover_widget.borrow().clone();
        if let Some(widget) = widget {
            widget.dispose();
        }
        self.cancel_ongoing();
    }
}

impl IManagedHoverWidget for ManagedHoverWidget {
    fn is_disposed(&self) -> bool {
        ManagedHoverWidget::is_disposed(self)
    }

    fn update(&self, content: ManagedHoverContent, focus: Option<bool>, options: Option<IManagedHoverOptions>) -> Promise<()> {
        ManagedHoverWidget::update(self, content, focus, options)
    }
}

/// `new ManagedHoverWidget(hoverDelegate, target, fadeInAnimation)` para o `HoverService`.
pub struct ManagedHoverWidgetFactory {
    shower: Rc<dyn IHoverShower>,
}

impl ManagedHoverWidgetFactory {
    pub fn new(shower: Rc<dyn IHoverShower>) -> Self {
        Self { shower }
    }
}

impl IManagedHoverWidgetFactory for ManagedHoverWidgetFactory {
    fn create(&self, delegate: &Rc<dyn IHoverDelegate>, target: Rc<HoverDelegateTarget>, fade_in_animation: bool) -> Rc<dyn IManagedHoverWidget> {
        ManagedHoverWidget::new(delegate.clone(), self.shower.clone(), target, fade_in_animation)
    }
}

/// Os elementos de alvo de um `HoverDelegateTarget` a partir de um elemento só (`isHTMLElement(target) ? [target] : ...`).
pub fn delegate_target_for_element(element: &Rc<ViewElement>) -> Rc<HoverDelegateTarget> {
    Rc::new(HoverDelegateTarget { target_elements: vec![element.clone()], x: Cell::new(None) })
}
