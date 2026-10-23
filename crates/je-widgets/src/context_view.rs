//! Porte de `src/vs/base/browser/ui/contextview/contextview.ts` (e do `contextview.css`): a [`ContextView`],
//! a caixa flutuante que menus, listas de seleção, dicas e ações de widget usam, posicionada ao lado de uma
//! âncora dentro da janela com o `layout2d` de `je_base::layout`.
//!
//! # API para quem consome (menu, selectBox, dropdown, hover, dialog)
//!
//! - [`ContextView::new`]`(ui, container, dom_position)`; [`ContextView::show`]`(delegate)` mostra a visão
//!   para um [`IContextViewDelegate`] (âncora, `render`, alinhamento, posição, camada, animação de
//!   fechamento); [`ContextView::hide`]/[`ContextView::hide_with`], [`ContextView::layout`] (reposiciona),
//!   [`ContextView::get_view_element`] (o nó `.context-view`), [`ContextView::set_container`].
//! - Ela implementa [`IContextViewProvider`] (`show_context_view(delegate, container)` troca o contêiner se
//!   vier um).
//! - **Tamanho.** No DOM o tamanho da visão vem do conteúdo. Aqui o `render` do delegado dimensiona o nó que
//!   recebe (`tree.set_bounds(view, ...)`, o `view.style.width/height` do teste do original); se ele não
//!   mexer e o nó ficar com tamanho zero, a visão adota a caixa que envolve os filhos (a partir de 0,0).
//!   O `layout2d` usa esse tamanho; a posição é escrita em `x0`/`y0` e o tamanho é preservado (o
//!   `width = 'initial'` do original).
//!
//! Como o DOM e o CSS viram [`je_ui`]:
//!
//! - `div.context-view` é um nó filho do contêiner (ou do `.shadow-root-host` em [`ContextViewDOMPosition::FixedShadow`]),
//!   escondido (`visible = false`) quando não há delegado. A `z-index` é `2575 + layer`. As classes `fixed`,
//!   `top`/`bottom`/`left`/`right` (o resultado do `layout2d`) e `monaco-component` são as do original.
//! - Não há `position`: absoluto e fixo escrevem as mesmas coordenadas relativas ao pai, calculadas como o
//!   original calcula (`posição de página da âncora` menos `posição de página da visão em (0, 0)`), então o
//!   `getDomNodePagePosition` do teste é [`je_ui::Tree::absolute_bounds`]. Fixo só liga a classe `fixed`
//!   ([`ContextView::is_fixed`]); o `.shadow-root-host` (`position: fixed`, 0x0, `z-index` da visão) fica
//!   com a origem da janela. A raiz de sombra de verdade (isolamento de CSS e a folha de estilo
//!   `SHADOW_ROOT_CSS`) não existe: não há CSS.
//! - `inert` é [`ContextView::is_inert`] (e `pointer-events: none` no nó); a variável de CSS
//!   `--vscode-context-view-close-animation-duration` é [`ContextView::close_animation_duration_variable`].
//! - `setTimeout`/`clearTimeout` são os do `je-base`.
//!
//! Fica de fora: a animação em si (`animation`/`transform` do CSS de `context-view-menu-motion`, e as
//! variáveis `--vscode-context-view-menu-motion-*` do fechamento), a folha de estilo do movimento do menu,
//! o `BrowserFeatures.pointerEvents` do iOS no `layout()`, o zoom (`getDomNodeZoomLevel`) e o deslocamento de
//! rolagem da janela (`pageXOffset`).

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::layout::{ILayout2DOptions, IRect, ISize, layout2d};
use je_base::lifecycle::{Disposable, DisposableRef, DisposableStore, IDisposable, none, to_disposable};
use je_base::runtime::{self, TimerId};
use je_render::kurbo::Rect;
use je_ui::{Event, EventKind, NodeId, PointerEvents, Role, Ui};

use crate::context_view_api::{Anchor, ContextViewDOMPosition, IContextViewDelegate, IContextViewProvider};
use crate::dom::{create_detached_el, is_ancestor, window_size};

/// `isAnchor(obj)`: um retângulo de âncora tem `x` e `y` (todo [`Anchor::Rect`] tem).
pub fn is_anchor(anchor: &Anchor) -> bool {
    matches!(anchor, Anchor::Rect(_))
}

/// `hasRequiredAncestorClasses(element, classNames)`: algum antepassado (ou o próprio nó) satisfaz todos os
/// requisitos; um requisito com várias classes aceita qualquer uma delas. Sem requisitos, vale.
pub fn has_required_ancestor_classes(ui: &Ui, element: NodeId, class_names: &[Vec<String>]) -> bool {
    if class_names.is_empty() {
        return true;
    }
    let tree = ui.tree();
    let mut candidate = Some(element);
    while let Some(current) = candidate {
        if class_names.iter().all(|requirement| requirement.iter().any(|class| tree.has_class(current, class))) {
            return true;
        }
        candidate = tree.parent(current);
    }
    false
}

/// `getAnchorRect(anchor)`: o retângulo da âncora em coordenadas da janela. O ponteiro vira um bloco de
/// 2x2 (para a visão não nascer exatamente sob o mouse), a âncora avulsa tem `width || 1` e `height || 2`.
pub fn get_anchor_rect(ui: &Ui, anchor: &Anchor) -> IRect {
    match anchor {
        Anchor::Node(node) => {
            let r = ui.tree().absolute_bounds(*node);
            IRect { top: r.y0, left: r.x0, width: r.width(), height: r.height() }
        }
        Anchor::Rect(a) => IRect { top: a.y, left: a.x, width: a.width.filter(|w| *w != 0.0).unwrap_or(1.0), height: a.height.filter(|h| *h != 0.0).unwrap_or(2.0) },
        Anchor::Mouse { x, y } => IRect { top: *y, left: *x, width: 2.0, height: 2.0 },
    }
}

/// A visão em fechamento animado: o que cancela o timer, o que descartar no fim e a classe a tirar.
struct HidingContextView {
    timer: TimerId,
    to_dispose: DisposableRef,
    class_name: String,
}

/// A visão de contexto.
pub struct ContextView {
    base: Rc<Disposable>,
    weak_self: Weak<ContextView>,
    ui: Ui,
    container: Cell<Option<NodeId>>,
    view: NodeId,
    use_fixed_position: Cell<bool>,
    use_shadow_dom: Cell<bool>,
    delegate: RefCell<Option<Rc<dyn IContextViewDelegate>>>,
    to_dispose_on_clean: RefCell<DisposableRef>,
    to_dispose_on_set_container: RefCell<Option<Rc<DisposableStore>>>,
    hiding_context_view: RefCell<Option<HidingContextView>>,
    shadow_root_host: Cell<Option<NodeId>>,
    close_animation_duration: Cell<Option<f64>>,
    inert: Cell<bool>,
}

impl ContextView {
    /// `new ContextView(container, domPosition)`.
    pub fn new(ui: &Ui, container: NodeId, dom_position: ContextViewDOMPosition) -> Rc<Self> {
        let view = {
            let mut tree = ui.tree_mut();
            let view = create_detached_el(&mut tree, Role::Generic, "context-view");
            tree.style_mut(view).visible = false;
            view
        };
        let this = Rc::new_cyclic(|weak| Self {
            base: Disposable::new(),
            weak_self: weak.clone(),
            ui: ui.clone(),
            container: Cell::new(None),
            view,
            use_fixed_position: Cell::new(false),
            use_shadow_dom: Cell::new(false),
            delegate: RefCell::new(None),
            to_dispose_on_clean: RefCell::new(none()),
            to_dispose_on_set_container: RefCell::new(None),
            hiding_context_view: RefCell::new(None),
            shadow_root_host: Cell::new(None),
            close_animation_duration: Cell::new(None),
            inert: Cell::new(false),
        });
        this.set_container(Some(container), dom_position);
        let weak = this.weak_self.clone();
        this.base.register(to_disposable(move || {
            if let Some(this) = weak.upgrade() {
                this.set_container(None, ContextViewDOMPosition::Absolute);
            }
        }));
        this
    }

    /// `setContainer(container, domPosition)`.
    pub fn set_container(&self, container: Option<NodeId>, dom_position: ContextViewDOMPosition) {
        self.use_fixed_position.set(dom_position != ContextViewDOMPosition::Absolute);
        let used_shadow_dom = self.use_shadow_dom.get();
        self.use_shadow_dom.set(dom_position == ContextViewDOMPosition::FixedShadow);

        if container == self.container.get() && used_shadow_dom == self.use_shadow_dom.get() {
            return; // o contêiner é o mesmo e o uso da raiz de sombra não mudou
        }

        if self.container.get().is_some() {
            if let Some(store) = self.to_dispose_on_set_container.borrow_mut().take() {
                store.dispose();
            }
            self.ui.tree_mut().detach(self.view);
            if let Some(host) = self.shadow_root_host.take() {
                self.ui.remove(host);
            }
            self.container.set(None);
        }

        let Some(container) = container else { return };
        self.container.set(Some(container));
        {
            let mut tree = self.ui.tree_mut();
            if self.use_shadow_dom.get() {
                let host = create_detached_el(&mut tree, Role::Generic, "shadow-root-host");
                tree.append_child(container, host);
                tree.append_child(host, self.view);
                self.shadow_root_host.set(Some(host));
            } else {
                tree.append_child(container, self.view);
            }
        }
        self.position_shadow_root_host();

        let store = DisposableStore::new();
        // BUBBLE_UP_EVENTS: `click`, `keydown`, `focus` e `blur`, na fase de borbulha; BUBBLE_DOWN_EVENTS: `click` na captura.
        for (kind, capture) in [(EventKind::Click, false), (EventKind::KeyDown, false), (EventKind::Focus, false), (EventKind::Blur, false), (EventKind::Click, true)] {
            let weak = self.weak_self.clone();
            store.add(self.ui.listen(container, kind, capture, move |e| {
                if let Some(this) = weak.upgrade() {
                    this.on_dom_event(e, capture);
                }
            }));
        }
        *self.to_dispose_on_set_container.borrow_mut() = Some(store);
    }

    /// O `.shadow-root-host` é `position: fixed; top: 0; left: 0; width: 0; height: 0`: fica na origem da janela
    /// e com o tamanho zero.
    fn position_shadow_root_host(&self) {
        let Some(host) = self.shadow_root_host.get() else { return };
        let mut tree = self.ui.tree_mut();
        tree.set_bounds(host, Rect::ZERO);
        let origin = tree.absolute_bounds(host);
        tree.set_bounds(host, Rect::new(-origin.x0, -origin.y0, -origin.x0, -origin.y0));
    }

    /// O `.shadow-root-host`, na posição de raiz de sombra.
    pub fn get_shadow_root_host(&self) -> Option<NodeId> {
        self.shadow_root_host.get()
    }

    /// O nó `.context-view`.
    pub fn get_view_element(&self) -> NodeId {
        self.view
    }

    /// A visão usa posição fixa (a classe `fixed`).
    pub fn is_fixed(&self) -> bool {
        self.use_fixed_position.get()
    }

    /// `view.inert`: verdadeiro durante a animação de fechamento.
    pub fn is_inert(&self) -> bool {
        self.inert.get()
    }

    /// O valor da variável `--vscode-context-view-close-animation-duration` na visão (`100ms`), se definida.
    pub fn close_animation_duration_variable(&self) -> Option<String> {
        self.close_animation_duration.get().map(|ms| format!("{ms}ms"))
    }

    fn is_visible(&self) -> bool {
        self.delegate.borrow().is_some()
    }

    /// `show(delegate)`.
    pub fn show(&self, delegate: Rc<dyn IContextViewDelegate>) {
        self.complete_hide_animation();

        if self.is_visible() {
            self.hide_with(None, true);
        }

        // Mostra a caixa estática
        let children = self.ui.tree().children(self.view).to_vec();
        for child in children {
            self.ui.remove(child);
        }
        {
            let mut tree = self.ui.tree_mut();
            tree.set_class_name(self.view, "context-view monaco-component");
            tree.set_bounds(self.view, Rect::ZERO);
            let z_index = 2575 + delegate.layer();
            tree.style_mut(self.view).z_index = z_index;
            if let Some(host) = self.shadow_root_host.get() {
                tree.style_mut(host).z_index = z_index;
            }
            tree.style_mut(self.view).visible = true;
        }

        // Monta o conteúdo
        let rendered = delegate.render(self.view).unwrap_or_else(none);
        self.fit_to_children_if_unsized();
        *self.to_dispose_on_clean.borrow_mut() = rendered;

        // Delegado ativo
        *self.delegate.borrow_mut() = Some(delegate.clone());

        // Layout
        self.do_layout();

        // Foco
        delegate.focus();
    }

    /// Sem tamanho dado pelo `render`, a caixa que envolve os filhos (o que o CSS daria a um bloco sem largura).
    fn fit_to_children_if_unsized(&self) {
        let mut tree = self.ui.tree_mut();
        let own = tree.bounds(self.view);
        if own.width() > 0.0 || own.height() > 0.0 {
            return;
        }
        let (width, height) = tree.children(self.view).to_vec().into_iter().map(|c| tree.bounds(c)).fold((0.0_f64, 0.0_f64), |(w, h), r| (w.max(r.x1), h.max(r.y1)));
        tree.set_bounds(self.view, Rect::new(own.x0, own.y0, own.x0 + width, own.y0 + height));
    }

    /// `layout()`: reposiciona (ou esconde, se o delegado não aceita reposicionar).
    pub fn layout(&self) {
        let Some(delegate) = self.delegate.borrow().clone() else { return };
        if !delegate.can_relayout() {
            self.hide();
            return;
        }
        delegate.layout();
        self.do_layout();
    }

    fn do_layout(&self) {
        // Verifica se ainda há delegado: o `delegate.layout` pode ter escondido a visão
        let Some(delegate) = self.delegate.borrow().clone() else { return };
        let anchor = get_anchor_rect(&self.ui, &delegate.get_anchor());
        let (window_width, window_height) = window_size(&self.ui);
        let viewport = IRect { top: 0.0, left: 0.0, width: window_width, height: window_height };
        self.position_shadow_root_host();
        let (origin, size) = {
            let mut tree = self.ui.tree_mut();
            tree.toggle_class(self.view, "fixed", self.use_fixed_position.get());
            let size = tree.bounds(self.view);
            tree.set_bounds(self.view, Rect::new(0.0, 0.0, size.width(), size.height()));
            (tree.absolute_bounds(self.view), size)
        };
        let result = layout2d(
            viewport,
            ISize { width: size.width(), height: size.height() },
            anchor,
            Some(ILayout2DOptions { anchor_alignment: delegate.anchor_alignment(), anchor_position: delegate.anchor_position(), anchor_axis_alignment: delegate.anchor_axis_alignment() }),
        );
        let mut tree = self.ui.tree_mut();
        for class in ["top", "bottom", "left", "right"] {
            tree.remove_class(self.view, class);
        }
        tree.add_class(self.view, if result.anchor_position == je_base::layout::AnchorPosition::Below { "bottom" } else { "top" });
        tree.add_class(self.view, if result.anchor_alignment == je_base::layout::AnchorAlignment::Left { "left" } else { "right" });
        let (left, top) = (result.left - origin.x0, result.top - origin.y0);
        tree.set_bounds(self.view, Rect::new(left, top, left + size.width(), top + size.height()));
    }

    /// `hide()`.
    pub fn hide(&self) {
        self.hide_with(None, false);
    }

    /// `hide(data, skipAnimation)`: `data` é o `didCancel` que o delegado recebe em `on_hide`.
    pub fn hide_with(&self, data: Option<bool>, skip_animation: bool) {
        if self.hiding_context_view.borrow().is_some() {
            if skip_animation {
                self.complete_hide_animation();
            }
            return;
        }

        let Some(delegate) = self.delegate.borrow_mut().take() else { return };
        let to_dispose = std::mem::replace(&mut *self.to_dispose_on_clean.borrow_mut(), none());

        delegate.on_hide(data);

        if !skip_animation
            && let Some(close) = delegate.close_animation()
            && close.duration > 0.0
            && has_required_ancestor_classes(&self.ui, self.view, &close.required_ancestor_classes)
        {
            self.close_animation_duration.set(Some(close.duration));
            self.inert.set(true);
            {
                let mut tree = self.ui.tree_mut();
                tree.style_mut(self.view).pointer_events = PointerEvents::None;
                tree.add_class(self.view, &close.class_name);
            }
            let weak = self.weak_self.clone();
            let timer = runtime::set_timeout(
                move || {
                    if let Some(this) = weak.upgrade() {
                        this.complete_hide_animation();
                    }
                },
                close.duration as u64,
            );
            *self.hiding_context_view.borrow_mut() = Some(HidingContextView { timer, to_dispose, class_name: close.class_name });
            return;
        }

        to_dispose.dispose();
        self.ui.tree_mut().style_mut(self.view).visible = false;
    }

    fn complete_hide_animation(&self) {
        let Some(hiding) = self.hiding_context_view.borrow_mut().take() else { return };
        runtime::clear_timeout(hiding.timer);
        self.ui.tree_mut().remove_class(self.view, &hiding.class_name);
        self.close_animation_duration.set(None);
        hiding.to_dispose.dispose();
        {
            let mut tree = self.ui.tree_mut();
            tree.style_mut(self.view).visible = false;
            tree.style_mut(self.view).pointer_events = PointerEvents::Inherit;
        }
        self.inert.set(false);
    }

    fn on_dom_event(&self, event: &Event, on_capture: bool) {
        let Some(delegate) = self.delegate.borrow().clone() else { return };
        if delegate.handles_dom_events() {
            delegate.on_dom_event(event, self.ui.focused());
        } else if on_capture && !self.container.get().is_some_and(|container| is_ancestor(&self.ui, event.target, container)) {
            self.hide();
        }
    }
}

impl IContextViewProvider for ContextView {
    fn show_context_view(&self, delegate: Rc<dyn IContextViewDelegate>, container: Option<NodeId>) {
        if let Some(container) = container {
            self.set_container(Some(container), if self.use_shadow_dom.get() { ContextViewDOMPosition::FixedShadow } else if self.use_fixed_position.get() { ContextViewDOMPosition::Fixed } else { ContextViewDOMPosition::Absolute });
        }
        self.show(delegate);
    }

    fn hide_context_view(&self) {
        self.hide();
    }

    fn layout(&self) {
        ContextView::layout(self);
    }
}

impl IDisposable for ContextView {
    fn dispose(&self) {
        self.hide();
        self.complete_hide_animation();
        self.base.dispose();
    }
}
