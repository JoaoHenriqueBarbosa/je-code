//! Porte de `src/vs/base/browser/ui/list/listPaging.ts`: a [`PagedList`], uma [`List`] de índices sobre um
//! `IPagedModel` (os elementos chegam por página, sob demanda), e o [`PagedRenderer`] que desenha o
//! placeholder enquanto a página do elemento não resolveu.
//!
//! Do TypeScript para o Rust:
//! - O `IPagedModel` é o de `je_base::paging`. O `ITemplateData<T>` do original é o [`PagedTemplateData`]
//!   (o dado do renderer interno e a fonte de cancelamento da resolução em andamento), guardado como
//!   [`TemplateData`] da lista.
//! - O `PagedAccessibilityProvider` e o `ariaLabel` não existem: a [`List`] não tem provedor de
//!   acessibilidade (o `je-ui` só tem `role` e `label`). O `typeNavigationEnabled`/`Mode` e o
//!   `updateOptions` também ficam de fora, pelos mesmos motivos da `List`.
//! - Os repasses puros do original (`layout`, `setFocus`, `focusNext`, `setSelection`, `reveal`, `scrollTop`,
//!   ...) não são reescritos: [`PagedList::widget`] devolve a `List<usize>` e quem precisar chama nela
//!   (o original também expõe `widget`). Ficam aqui só os membros que acrescentam algo: o `model`, os
//!   eventos que trocam o índice pelo elemento, `getSelectedElements` e o descarte.
//! - O `onTap` e o `onContextMenu` não existem na `List` portada (toque e menu de contexto, ver
//!   `list_types.rs`).
//! - Divergência deliberada: o `then` que desenha o elemento resolvido ignora a resposta quando o token da
//!   linha já foi cancelado. No original, um pager que ignora o cancelamento ainda desenharia o elemento
//!   velho numa linha reaproveitada para outro índice.

use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;

use je_base::cancellation::CancellationTokenSource;
use je_base::event::Event as BaseEvent;
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::paging::IPagedModel;
use je_ui::{NodeId, Ui};

use crate::common::throw;
use crate::list::list_types::{IListRenderer, IListVirtualDelegate, ListElementRenderDetails, ListEvent, ListMouseEvent, TemplateData};
use crate::list::list_view::ListViewOptions;
use crate::list::list_widget::{List, ListOptions};

/// `IPagedRenderer<TElement, TTemplateData>`: um renderer comum mais o desenho do estado "carregando".
pub trait IPagedRenderer<T>: IListRenderer<T> {
    fn render_placeholder(&self, index: usize, template_data: &TemplateData);
}

/// `ITemplateData<T>`: o dado do renderer interno (vazio depois de `disposeTemplate`) e a fonte que cancela
/// a resolução do elemento da linha.
pub struct PagedTemplateData {
    data: RefCell<Option<TemplateData>>,
    disposable: RefCell<Option<CancellationTokenSource>>,
}

impl PagedTemplateData {
    /// `data.disposable?.dispose()`: cancela a resolução em andamento, se houver.
    fn cancel_pending(&self) {
        let pending = self.disposable.borrow_mut().take();
        if let Some(cts) = pending {
            cts.cancel();
        }
    }
}

/// O provedor do modelo corrente (o `modelProvider` do original).
type ModelProvider<T> = Rc<dyn Fn() -> Rc<dyn IPagedModel<T>>>;

/// O modelo corrente, compartilhado entre a lista e os renderers (vazio até o `model` ser definido).
type ModelCell<T> = Rc<RefCell<Option<Rc<dyn IPagedModel<T>>>>>;

/// `PagedRenderer<TElement, TTemplateData>`: uma lista de índices vira uma lista de elementos.
pub struct PagedRenderer<T: Clone + 'static> {
    renderer: Rc<dyn IPagedRenderer<T>>,
    model_provider: ModelProvider<T>,
}

impl<T: Clone + 'static> PagedRenderer<T> {
    pub fn new(renderer: Rc<dyn IPagedRenderer<T>>, model_provider: ModelProvider<T>) -> Self {
        Self { renderer, model_provider }
    }
}

fn paged_data(template_data: &TemplateData) -> Rc<PagedTemplateData> {
    match template_data.clone().downcast::<PagedTemplateData>() {
        Ok(data) => data,
        Err(_) => throw("PagedRenderer recebeu um template que não é o dele"),
    }
}

impl<T: Clone + 'static> IListRenderer<usize> for PagedRenderer<T> {
    fn template_id(&self) -> String {
        self.renderer.template_id()
    }

    fn render_template(&self, container: NodeId) -> TemplateData {
        let data = self.renderer.render_template(container);
        Rc::new(PagedTemplateData { data: RefCell::new(Some(data)), disposable: RefCell::new(None) }) as Rc<dyn Any>
    }

    fn render_element(&self, index: &usize, _: usize, template_data: &TemplateData, details: Option<&ListElementRenderDetails>) {
        let index = *index;
        let data = paged_data(template_data);
        data.cancel_pending();

        let Some(inner) = data.data.borrow().clone() else { return };

        let model = (self.model_provider)();

        if model.is_resolved(index) {
            return self.renderer.render_element(&model.get(index), index, &inner, details);
        }

        let cts = CancellationTokenSource::new(None);
        let token = cts.token();
        let promise = model.resolve(index, token.clone());
        *data.disposable.borrow_mut() = Some(cts);

        self.renderer.render_placeholder(index, &inner);

        let renderer = self.renderer.clone();
        let details = details.copied();
        // A rejeição (cancelamento) passa adiante sem tratador, como no original.
        drop(promise.then_result(move |result| {
            let entry = result?;
            if token.is_cancellation_requested() {
                return Ok(());
            }
            if let Some(inner) = data.data.borrow().clone() {
                renderer.render_element(&entry, index, &inner, details.as_ref());
            }
            Ok(())
        }));
    }

    fn dispose_template(&self, template_data: &TemplateData) {
        let data = paged_data(template_data);
        data.cancel_pending();
        let inner = data.data.borrow_mut().take();
        if let Some(inner) = inner {
            self.renderer.dispose_template(&inner);
        }
    }
}

/// `IPagedListOptions<T>`: as opções da lista que se aplicam (ver o `//!` do módulo).
#[derive(Clone, Default)]
pub struct PagedListOptions {
    pub view: ListViewOptions,
    pub multiple_selection_support: Option<bool>,
    pub keyboard_support: Option<bool>,
}

/// `PagedList<T>`.
pub struct PagedList<T: Clone + 'static> {
    list: Rc<List<usize>>,
    model: ModelCell<T>,
    model_disposables: Rc<DisposableStore>,
}

impl<T: Clone + 'static> PagedList<T> {
    pub fn new(user: &str, ui: &Ui, container: NodeId, virtual_delegate: Rc<dyn IListVirtualDelegate<usize>>, renderers: Vec<Rc<dyn IPagedRenderer<T>>>, options: PagedListOptions) -> Self {
        let model: ModelCell<T> = Rc::default();
        let provider_model = model.clone();
        let model_provider: ModelProvider<T> = Rc::new(move || match provider_model.borrow().clone() {
            Some(model) => model,
            None => throw("PagedList sem modelo"),
        });
        let paged_renderers: Vec<Rc<dyn IListRenderer<usize>>> = renderers.into_iter().map(|r| Rc::new(PagedRenderer::new(r, model_provider.clone())) as Rc<dyn IListRenderer<usize>>).collect();
        let list_options = ListOptions {
            view: options.view,
            multiple_selection_support: options.multiple_selection_support,
            keyboard_support: options.keyboard_support,
            ..Default::default()
        };
        let list = List::new(user, ui, container, virtual_delegate, paged_renderers, list_options);
        Self { list, model, model_disposables: DisposableStore::new() }
    }

    /// `get widget()`.
    pub fn widget(&self) -> &Rc<List<usize>> {
        &self.list
    }

    /// `get model()`: sem modelo ainda, é erro (o original devolve `undefined` e quebra no primeiro uso).
    pub fn model(&self) -> Rc<dyn IPagedModel<T>> {
        match self.model.borrow().clone() {
            Some(model) => model,
            None => throw("PagedList sem modelo"),
        }
    }

    /// `set model(model)`: a lista passa a ter um índice por elemento e cresce com `onDidIncrementLength`.
    pub fn set_model(&self, model: Rc<dyn IPagedModel<T>>) {
        self.model_disposables.clear();
        *self.model.borrow_mut() = Some(model.clone());
        self.list.splice(0, self.list.length(), &(0..model.length()).collect::<Vec<usize>>());
        let list = Rc::downgrade(&self.list);
        self.model_disposables.add(model.on_did_increment_length().listen(move |new_length| {
            if let Some(list) = list.upgrade() {
                let length = list.length();
                list.splice(length, 0, &(length..*new_length).collect::<Vec<usize>>());
            }
        }));
    }

    pub fn get_selected_elements(&self) -> Vec<T> {
        let model = self.model();
        self.list.get_selection().into_iter().map(|i| model.get(i)).collect()
    }

    /// O elemento do modelo para um evento da lista de índices.
    fn map_list_event(model: &ModelCell<T>, event: &ListEvent<usize>) -> ListEvent<T> {
        let model = model.borrow().clone().unwrap_or_else(|| throw("PagedList sem modelo"));
        ListEvent { elements: event.elements.iter().map(|e| model.get(*e)).collect(), indexes: event.indexes.clone(), browser_event: event.browser_event }
    }

    fn map_mouse_event(&self, event: BaseEvent<ListMouseEvent<usize>>) -> BaseEvent<ListMouseEvent<T>> {
        let model = self.model.clone();
        event.map(
            move |e| {
                let model = model.borrow().clone().unwrap_or_else(|| throw("PagedList sem modelo"));
                ListMouseEvent { browser_event: e.browser_event, is_handled_by_list: e.is_handled_by_list.clone(), element: e.element.map(|i| model.get(i)), index: e.index }
            },
            None,
        )
    }

    pub fn on_mouse_click(&self) -> BaseEvent<ListMouseEvent<T>> {
        self.map_mouse_event(self.list.view.on_mouse_click())
    }

    pub fn on_mouse_dbl_click(&self) -> BaseEvent<ListMouseEvent<T>> {
        self.map_mouse_event(self.list.view.on_mouse_dbl_click())
    }

    pub fn on_pointer(&self) -> BaseEvent<ListMouseEvent<T>> {
        self.map_mouse_event(self.list.on_pointer())
    }

    pub fn on_did_change_focus(&self) -> BaseEvent<ListEvent<T>> {
        let model = self.model.clone();
        self.list.on_did_change_focus().map(move |e| Self::map_list_event(&model, e), None)
    }

    pub fn on_did_change_selection(&self) -> BaseEvent<ListEvent<T>> {
        let model = self.model.clone();
        self.list.on_did_change_selection().map(move |e| Self::map_list_event(&model, e), None)
    }
}

impl<T: Clone + 'static> IDisposable for PagedList<T> {
    fn dispose(&self) {
        self.list.dispose();
        self.model_disposables.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "PagedList"
    }
}
