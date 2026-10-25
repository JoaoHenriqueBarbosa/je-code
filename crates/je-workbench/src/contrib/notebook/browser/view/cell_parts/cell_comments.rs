//! Porte de `src/vs/workbench/contrib/notebook/browser/view/cellParts/cellComments.ts`: `CellComments`, a parte que mostra, abaixo da
//! célula, os widgets das threads de comentário dela e informa à célula a altura que eles ocupam.
//!
//! Divergências:
//! - **O `CommentThreadWidget` não tem porte.** `commentThreadWidget.ts` entra como contrato estreito, só com o que a parte chama:
//!   [`ICellCommentThreadWidget`] e a fábrica [`ICellCommentThreadWidgetFactory`], que é o
//!   `instantiationService.createInstance(CommentThreadWidget, ...)` com os argumentos fixos do original (o `actionRunner` vazio e o
//!   `collapse` que devolve `true` pertencem à fábrica). O `ICommentService`, o `INotebookCommentInfo` e o
//!   `languages.CommentThread<ICellRange>` são os de `contrib/comments` (`ICommentService`, `INotebookCommentInfo` e
//!   `CommentThread<ICellRange>`).
//! - **Funções assíncronas são [`Promise`].** `initialize`, `_createCommentTheadWidget`, `_updateThread` e
//!   `_getCommentThreadsForCell` devolvem a promessa; os `await` em sequência de `_updateThread` (um por thread, na ordem) são
//!   `process_threads`, que encadeia uma thread por vez. `coalesce` é o `flatten` sobre as listas de `Option`.
//! - `INotebookEditorDelegate` é o contrato estreito [`ICellCommentsEditor`] (`hasModel()`, `textModel.uri` e
//!   `getLayoutInfo().fontInfo`). `IThemeService.onDidColorThemeChange` chega como o evento `on_did_color_theme_change`,
//!   `IContextKeyService` e `IInstantiationService` só serviam à criação do widget e ficam com a fábrica.
//! - `this.container.style.top = '...px'` é o [`StickyTopNode`]; `classList.add('review-widget')` é a classe do nó.
//! - `currentElement.commentHeight = h` e `layoutInfo.commentOffset` são os métodos e campos de cada forma de [`CellViewModel`]
//!   (`set_comment_height` e `layout_info().comment_offset` do código e do markup). `commentOffset` não é um dos campos que as duas
//!   formas compartilham em `common_layout_info`, então é lido por forma, aqui.
//! - `this.notebookEditor.textModel!.uri` sem modelo lança no original; aqui a promessa é rejeitada com o erro.
//! - `CellContentPart.prepareLayout` é sobrescrito, então o trait `CellContentPart` é implementado à mão (a macro
//!   `impl_cell_content_part!` só gera a versão com os métodos padrão).
//! - **`DisposableMap`.** O valor `{ widget, dispose }` é o [`CommentThreadWidgetEntry`], que descarta as `widgetDisposables`.

use std::cell::RefCell;
use std::collections::{HashSet, VecDeque};
use std::rc::{Rc, Weak};

use je_base::errors::Error;
use je_base::event::Event;
use je_base::lifecycle::{DisposableMap, DisposableRef, DisposableStore, IDisposable};
use je_base::promise::Promise;
use je_base::uri::URI;
use je_editor::config::font_info::FontInfo;
use je_editor::languages::CommentThread;
use je_ui::{NodeId, Ui};

use crate::contrib::comments::browser::comment_service::{ICommentService, INotebookCommentInfo};
use crate::contrib::notebook::browser::view::cell_part::{CellContentPart, CellPart, CellPartState};
use crate::contrib::notebook::browser::view::cell_parts::cell_toolbar_sticky_scroll::StickyTopNode;
use crate::contrib::notebook::browser::view_model::notebook_view_model_impl::CellViewModel;
use crate::contrib::notebook::common::notebook_range::ICellRange;

/// `CommentThreadWidget<ICellRange>`, só com o que `CellComments` chama.
pub trait ICellCommentThreadWidget: IDisposable {
    /// `display(lineHeight, focus)`.
    fn display(&self, line_height: f64, focus: bool) -> Promise<()>;
    /// `updateCommentThread(commentThread)`.
    fn update_comment_thread(&self, comment_thread: &Rc<dyn CommentThread<ICellRange>>) -> Promise<()>;
    /// `getDimensions().height`.
    fn get_dimensions_height(&self) -> f64;
    /// `onDidResize`.
    fn on_did_resize(&self) -> Event<()>;
    /// `applyTheme(fontInfo)`.
    fn apply_theme(&self, font_info: &FontInfo);
}

/// O `createInstance(CommentThreadWidget, container, notebookEditor, owner, uri, contextKeyService, instantiationService,
/// commentThread, undefined, undefined, {}, undefined, { actionRunner, collapse })` do original.
pub trait ICellCommentThreadWidgetFactory {
    fn create(&self, container: NodeId, owner: &str, resource: &URI, comment_thread: &Rc<dyn CommentThread<ICellRange>>) -> Rc<dyn ICellCommentThreadWidget>;
}

/// O que `CellComments` chama do `INotebookEditorDelegate`.
pub trait ICellCommentsEditor {
    /// `hasModel()`.
    fn has_model(&self) -> bool;
    /// `textModel.uri`.
    fn text_model_uri(&self) -> Option<URI>;
    /// `getLayoutInfo().fontInfo`.
    fn font_info(&self) -> Rc<FontInfo>;
}

/// O valor `{ widget, dispose }` do `DisposableMap`.
struct CommentThreadWidgetEntry {
    widget: Rc<dyn ICellCommentThreadWidget>,
    widget_disposables: Rc<DisposableStore>,
}

impl IDisposable for CommentThreadWidgetEntry {
    fn dispose(&self) {
        self.widget_disposables.dispose();
    }
}

/// `layoutInfo.commentOffset` da célula.
fn comment_offset(cell: &CellViewModel) -> f64 {
    match cell {
        CellViewModel::Code(cell) => cell.layout_info().comment_offset,
        CellViewModel::Markup(cell) => cell.layout_info().comment_offset,
    }
}

/// `cell.commentHeight = height`.
fn set_comment_height(cell: &CellViewModel, height: f64) {
    match cell {
        CellViewModel::Code(cell) => cell.set_comment_height(height),
        CellViewModel::Markup(cell) => cell.set_comment_height(height),
    }
}

/// `CellComments`.
pub struct CellComments {
    state: CellPartState,
    me: Weak<CellComments>,
    notebook_editor: Rc<dyn ICellCommentsEditor>,
    container: Rc<StickyTopNode>,
    comment_service: Rc<dyn ICommentService>,
    widget_factory: Rc<dyn ICellCommentThreadWidgetFactory>,
    /// keyed by threadId
    comment_thread_widgets: Rc<DisposableMap<String, CommentThreadWidgetEntry>>,
    current_element: RefCell<Option<CellViewModel>>,
}

impl CellComments {
    pub fn new(
        ui: &Ui,
        notebook_editor: Rc<dyn ICellCommentsEditor>,
        container: NodeId,
        on_did_color_theme_change: Event<()>,
        comment_service: Rc<dyn ICommentService>,
        widget_factory: Rc<dyn ICellCommentThreadWidgetFactory>,
    ) -> Rc<Self> {
        ui.tree_mut().add_class(container, "review-widget");

        let state = CellPartState::new();
        let comment_thread_widgets = DisposableMap::<String, CommentThreadWidgetEntry>::new();
        state.register(comment_thread_widgets.clone());

        let part = Rc::new_cyclic(|me: &Weak<Self>| {
            {
                let me = me.clone();
                state.register(on_did_color_theme_change.listen(move |_| {
                    if let Some(part) = me.upgrade() {
                        part.apply_theme();
                    }
                }));
            }
            Self {
                state,
                me: me.clone(),
                notebook_editor,
                container: StickyTopNode::new(ui, container),
                comment_service,
                widget_factory,
                comment_thread_widgets,
                current_element: RefCell::new(None),
            }
        });
        // TODO @rebornix onDidChangeLayout (font change)
        // this._register(this.notebookEditor.onDidchangeLa)
        part.apply_theme();
        part
    }

    fn initialize(&self, element: &CellViewModel) -> Promise<()> {
        if self.current_element.borrow().as_ref().is_some_and(|current| current.ptr_eq(element)) {
            return Promise::resolved(());
        }

        *self.current_element.borrow_mut() = Some(element.clone());
        self.update_thread()
    }

    /// `_createCommentTheadWidget(owner, commentThread)`.
    fn create_comment_thread_widget(&self, owner: &str, comment_thread: &Rc<dyn CommentThread<ICellRange>>) -> Promise<()> {
        let Some(text_model_uri) = self.notebook_editor.text_model_uri() else {
            return Promise::rejected(Error::new("CellComments: o editor do notebook não tem modelo de texto"));
        };

        let widget_disposables = DisposableStore::new();
        let widget = self.widget_factory.create(self.container.node(), owner, &text_model_uri, comment_thread);
        let widget_disposable: DisposableRef = widget.clone();
        widget_disposables.add(widget_disposable);
        self.comment_thread_widgets.set(comment_thread.thread_id(), Rc::new(CommentThreadWidgetEntry { widget: widget.clone(), widget_disposables: widget_disposables.clone() }), false);

        let line_height = self.notebook_editor.font_info().line_height;
        let me = self.me.clone();
        widget.display(line_height, true).then(move |_| {
            let Some(part) = me.upgrade() else { return };
            part.apply_theme();

            let (me, weak_widget) = (me.clone(), Rc::downgrade(&widget));
            widget_disposables.add(widget.on_did_resize().listen(move |_| {
                let (Some(part), Some(widget)) = (me.upgrade(), weak_widget.upgrade()) else { return };
                let current_element = part.current_element.borrow().clone();
                if let Some(current_element) = current_element {
                    set_comment_height(&current_element, part.calculate_comment_thread_height(widget.get_dimensions_height()));
                }
            }));
        })
    }

    fn bind_listeners(&self) {
        let me = self.me.clone();
        self.state.cell_disposables().add(self.comment_service.on_did_update_comment_threads().listen(move |_| {
            if let Some(part) = me.upgrade() {
                // O original devolve a promessa ao emissor, que a ignora.
                let _ = part.update_thread();
            }
        }));
    }

    /// `_updateThread()`.
    fn update_thread(&self) -> Promise<()> {
        let Some(current_element) = self.current_element.borrow().clone() else {
            return Promise::resolved(());
        };

        let me = self.me.clone();
        self.get_comment_threads_for_cell(&current_element).then_promise(move |infos| {
            let Some(part) = me.upgrade() else {
                return Promise::resolved(());
            };
            let mut widgets_to_delete: HashSet<String> = part.comment_thread_widgets.keys().into_iter().collect();
            part.container.set_top(Some(comment_offset(&current_element)));

            let mut work: VecDeque<(String, Rc<dyn CommentThread<ICellRange>>)> = VecDeque::new();
            for info in infos {
                for thread in info.threads {
                    widgets_to_delete.remove(&thread.thread_id());
                    work.push_back((info.unique_owner.clone(), thread));
                }
            }

            Self::process_threads(part.clone(), work).then(move |_| {
                for thread_id in widgets_to_delete {
                    part.comment_thread_widgets.delete_and_dispose(&thread_id);
                }
                part.update_height();
            })
        })
    }

    /// O laço `for (const thread of info.threads) { await ... }` de `_updateThread`: uma thread por vez, na ordem.
    fn process_threads(part: Rc<Self>, mut work: VecDeque<(String, Rc<dyn CommentThread<ICellRange>>)>) -> Promise<()> {
        let Some((owner, thread)) = work.pop_front() else {
            return Promise::resolved(());
        };

        let widget = part.comment_thread_widgets.get(&thread.thread_id()).map(|entry| entry.widget.clone());
        let step = match widget {
            Some(widget) => widget.update_comment_thread(&thread),
            None => part.create_comment_thread_widget(&owner, &thread),
        };
        step.then_promise(move |_| Self::process_threads(part, work))
    }

    fn calculate_comment_thread_height(&self, body_height: f64) -> f64 {
        let line_height = self.notebook_editor.font_info().line_height;

        let head_height = (line_height * 1.2).ceil();
        let arrow_height = (line_height / 3.0).round();
        let frame_thickness = (line_height / 9.0).round() * 2.0;

        head_height + body_height + arrow_height + frame_thickness + 8.0 /* margin bottom to avoid margin collapse */
    }

    fn update_height(&self) {
        let Some(current_element) = self.current_element.borrow().clone() else {
            return;
        };
        let height: f64 = self.comment_thread_widgets.values().iter().map(|entry| self.calculate_comment_thread_height(entry.widget.get_dimensions_height())).sum();
        set_comment_height(&current_element, height);
    }

    /// `_getCommentThreadsForCell(element)`.
    fn get_comment_threads_for_cell(&self, element: &CellViewModel) -> Promise<Vec<INotebookCommentInfo>> {
        if self.notebook_editor.has_model() {
            return self.comment_service.get_notebook_comments(&element.uri()).then(|infos| infos.into_iter().flatten().collect());
        }

        Promise::resolved(Vec::new())
    }

    fn apply_theme(&self) {
        let font_info = self.notebook_editor.font_info();
        for entry in self.comment_thread_widgets.values() {
            entry.widget.apply_theme(&font_info);
        }
    }
}

impl CellPart for CellComments {
    fn state(&self) -> &CellPartState {
        &self.state
    }

    fn did_render_cell(&self, element: &CellViewModel) {
        // O original não espera a promessa.
        let _ = self.initialize(element);
        self.bind_listeners();
    }

    fn update_internal_layout_now(&self, element: &CellViewModel) {
        if self.current_element.borrow().is_some() {
            self.container.set_top(Some(comment_offset(element)));
        }
    }
}

impl_cell_part_disposable!(CellComments, "CellComments");

impl CellContentPart for CellComments {
    fn prepare_layout(&self) {
        self.update_height();
    }
}
