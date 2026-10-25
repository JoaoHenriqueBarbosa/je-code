//! Porte de `src/vs/editor/contrib/hover/browser/resizableContentWidget.ts`: o [`ResizableContentWidget`], a base do
//! widget de conteúdo do hover (e de outros): um [`IContentWidget`] com um [`ResizableHtmlElement`] dentro, que o usuário
//! redimensiona pelas bordas e que escolhe se aparece acima ou abaixo da posição conforme o espaço que há no editor.
//!
//! Notas de tradução:
//! - `abstract class ResizableContentWidget extends Disposable implements IContentWidget` é a struct com um [`Disposable`]
//!   dentro. O membro abstrato `getId()` é o argumento `id` do construtor (todo filho devolve uma constante). O único
//!   membro que os filhos sobrescrevem, `_resize(dimension)`, é o gancho [`ResizableContentWidgetHooks::resize`], registrado
//!   com [`ResizableContentWidget::set_hooks`] (uma referência fraca, para não formar ciclo); o corpo padrão e o
//!   `super._resize` são [`ResizableContentWidget::resize_base`].
//! - `protected _contentPosition` é lido e escrito pelos filhos por [`ResizableContentWidget::content_position`] e
//!   [`ResizableContentWidget::set_content_position`]; `protected _resizableNode` é [`ResizableContentWidget::resizable_node`].
//! - `getDomNode()` devolve o [`NodeId`] do nó redimensionável (o `IContentWidget` do porte não tem `getDomNode`, ver
//!   `editor_browser`).
//! - **O que o editor nativo não expõe.** O editor do porte não tem `getDomNode()`, `getScrolledVisiblePosition(position)`
//!   (a `View` e o DOM ficaram fora do `CodeEditorWidget`) nem a área do `body` da janela. As três leituras que
//!   `_availableVerticalSpaceAbove` e `_availableVerticalSpaceBelow` fazem entram pelo [`IResizableContentWidgetHost`]:
//!   quem tem a `View` (ou o host nativo que a substitui) o implementa. `editorBox.top` é
//!   [`IResizableContentWidgetHost::editor_page_top`], onde `None` é o `getDomNode()` indefinido, e
//!   `dom.getClientArea(body).height` é [`IResizableContentWidgetHost::client_area_height`].
//! - `_findPositionPreference` tem o tipo de retorno `ContentWidgetPositionPreference | undefined`, mas o corpo sempre
//!   devolve uma preferência; aqui o retorno é a preferência, e o `?? ABOVE` dos chamadores deixa de existir.
//! - `style.position = 'absolute'` não tem tradução: a posição do nó na árvore já é absoluta (os limites que o dono do nó
//!   escreve).

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::lifecycle::{Disposable, IDisposable};
use je_ui::{NodeId, Ui};
use je_widgets::resizable::{Dimension, IResizeEvent, ResizableHtmlElement};

use crate::code_editor_widget::{CodeEditorWidget, ICodeEditor};
use crate::config::editor_options::hover::HOVER;
use crate::editor_browser::{ContentWidgetPosition, ContentWidgetPositionPreference, IContentWidget};
use crate::position::Position;

const TOP_HEIGHT: f64 = 30.0;
const BOTTOM_HEIGHT: f64 = 24.0;

/// O resultado de `editor.getScrolledVisiblePosition(position)`: `{ top, left, height }`, relativo ao editor.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrolledVisiblePosition {
    pub top: f64,
    pub left: f64,
    pub height: f64,
}

/// O que o widget lê do editor e da janela e que o `CodeEditorWidget` não tem (ver as notas do módulo).
pub trait IResizableContentWidgetHost {
    /// `dom.getDomNodePagePosition(editor.getDomNode()).top`; `None` quando o editor não tem nó (`!editorDomNode`).
    fn editor_page_top(&self) -> Option<f64>;

    /// `editor.getScrolledVisiblePosition(position)`; `None` quando a posição não está visível.
    fn get_scrolled_visible_position(&self, position: Position) -> Option<ScrolledVisiblePosition>;

    /// `dom.getClientArea(editorDomNode.ownerDocument.body).height`.
    fn client_area_height(&self) -> f64;
}

/// Os membros de `ResizableContentWidget` que as subclasses sobrescrevem.
pub trait ResizableContentWidgetHooks {
    /// `_resize(dimension)`: o corpo padrão é [`ResizableContentWidget::resize_base`].
    fn resize(&self, widget: &ResizableContentWidget, dimension: Dimension) {
        widget.resize_base(dimension);
    }
}

/// `ResizableContentWidget`.
pub struct ResizableContentWidget {
    base: Rc<Disposable>,
    id: String,
    editor: Rc<CodeEditorWidget>,
    host: Rc<dyn IResizableContentWidgetHost>,
    resizable_node: Rc<ResizableHtmlElement>,
    content_position: RefCell<Option<ContentWidgetPosition>>,
    is_resizing: Cell<bool>,
    hooks: RefCell<Option<Weak<dyn ResizableContentWidgetHooks>>>,
}

impl ResizableContentWidget {
    /// `constructor(editor, minimumSize = new dom.Dimension(10, 10))`; `minimum_size` ausente vale 10 por 10. `id` é o
    /// `getId()` da subclasse.
    pub fn new(
        editor: Rc<CodeEditorWidget>,
        host: Rc<dyn IResizableContentWidgetHost>,
        ui: &Ui,
        id: String,
        minimum_size: Option<Dimension>,
    ) -> Rc<Self> {
        let minimum_size = minimum_size.unwrap_or(Dimension::new(10.0, 10.0));
        let base = Disposable::new();
        let resizable_node = ResizableHtmlElement::new(ui);
        base.register(resizable_node.clone());
        resizable_node.set_min_size(minimum_size);
        resizable_node.layout(Some(minimum_size.height), Some(minimum_size.width));
        resizable_node.enable_sashes(true, true, true, true);

        let this = Rc::new(Self {
            base,
            id,
            editor,
            host,
            resizable_node,
            content_position: RefCell::new(None),
            is_resizing: Cell::new(false),
            hooks: RefCell::new(None),
        });

        this.base.register(this.resizable_node.on_did_resize().listen({
            let weak = Rc::downgrade(&this);
            move |event: &IResizeEvent| {
                let Some(this) = weak.upgrade() else {
                    return;
                };
                this.resize(Dimension::new(event.dimension.width, event.dimension.height));
                if event.done {
                    this.is_resizing.set(false);
                }
            }
        }));
        this.base.register(this.resizable_node.on_did_will_resize().listen({
            let weak = Rc::downgrade(&this);
            move |_| {
                if let Some(this) = weak.upgrade() {
                    this.is_resizing.set(true);
                }
            }
        }));
        this
    }

    /// Registra os ganchos que a subclasse sobrescreve.
    pub fn set_hooks(&self, hooks: Weak<dyn ResizableContentWidgetHooks>) {
        *self.hooks.borrow_mut() = Some(hooks);
    }

    /// `isResizing`.
    pub fn is_resizing(&self) -> bool {
        self.is_resizing.get()
    }

    /// `getDomNode()`.
    pub fn get_dom_node(&self) -> NodeId {
        self.resizable_node.dom_node()
    }

    /// `_resizableNode`.
    pub fn resizable_node(&self) -> &Rc<ResizableHtmlElement> {
        &self.resizable_node
    }

    /// `_contentPosition` (leitura).
    pub fn content_position(&self) -> Option<ContentWidgetPosition> {
        self.content_position.borrow().clone()
    }

    /// `_contentPosition` (escrita).
    pub fn set_content_position(&self, content_position: Option<ContentWidgetPosition>) {
        *self.content_position.borrow_mut() = content_position;
    }

    /// `get position()`: `Position.lift(this._contentPosition.position)`.
    pub fn position(&self) -> Option<Position> {
        self.content_position.borrow().as_ref().and_then(|content_position| content_position.position)
    }

    /// `_availableVerticalSpaceAbove(position)`.
    pub fn available_vertical_space_above(&self, position: Position) -> Option<f64> {
        let editor_top = self.host.editor_page_top()?;
        let mouse_box = self.host.get_scrolled_visible_position(position)?;
        Some(editor_top + mouse_box.top - TOP_HEIGHT)
    }

    /// `_availableVerticalSpaceBelow(position)`.
    pub fn available_vertical_space_below(&self, position: Position) -> Option<f64> {
        let editor_top = self.host.editor_page_top()?;
        let mouse_box = self.host.get_scrolled_visible_position(position)?;
        let mouse_bottom = editor_top + mouse_box.top + mouse_box.height;
        Some(self.host.client_area_height() - mouse_bottom - BOTTOM_HEIGHT)
    }

    /// `_findPositionPreference(widgetHeight, showAtPosition)`: acima ou abaixo, conforme a opção `hover.above` e o espaço
    /// livre, e liga as bordas do nó que fazem sentido para o lado escolhido.
    pub fn find_position_preference(&self, widget_height: f64, show_at_position: Position) -> ContentWidgetPositionPreference {
        let max_height_below = self.available_vertical_space_below(show_at_position).unwrap_or(f64::INFINITY).min(widget_height);
        let max_height_above = self.available_vertical_space_above(show_at_position).unwrap_or(f64::INFINITY).min(widget_height);
        let max_height = max_height_above.max(max_height_below).min(widget_height);
        let height = widget_height.min(max_height);
        let rendering_above = if self.editor.get_option(HOVER).above {
            if height <= max_height_above { ContentWidgetPositionPreference::Above } else { ContentWidgetPositionPreference::Below }
        } else if height <= max_height_below {
            ContentWidgetPositionPreference::Below
        } else {
            ContentWidgetPositionPreference::Above
        };
        if rendering_above == ContentWidgetPositionPreference::Above {
            self.resizable_node.enable_sashes(true, true, false, false);
        } else {
            self.resizable_node.enable_sashes(false, true, true, false);
        }
        rendering_above
    }

    /// `_resize(dimension)`: despacha para o gancho da subclasse, se houver.
    fn resize(&self, dimension: Dimension) {
        let hooks = self.hooks.borrow().as_ref().and_then(Weak::upgrade);
        match hooks {
            Some(hooks) => hooks.resize(self, dimension),
            None => self.resize_base(dimension),
        }
    }

    /// O corpo de `ResizableContentWidget._resize(dimension)`.
    pub fn resize_base(&self, dimension: Dimension) {
        self.resizable_node.layout(Some(dimension.height), Some(dimension.width));
    }
}

impl IContentWidget for ResizableContentWidget {
    fn get_id(&self) -> String {
        self.id.clone()
    }

    fn get_position(&self) -> Option<ContentWidgetPosition> {
        self.content_position()
    }

    /// `allowEditorOverflow = true`.
    fn allow_editor_overflow(&self) -> bool {
        true
    }

    /// `suppressMouseDown = false`.
    fn suppress_mouse_down(&self) -> bool {
        false
    }
}

impl IDisposable for ResizableContentWidget {
    fn dispose(&self) {
        self.base.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "ResizableContentWidget"
    }
}
