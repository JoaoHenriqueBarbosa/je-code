//! Porte de `src/vs/workbench/contrib/notebook/browser/services/notebookCellStatusBarServiceImpl.ts`: o `NotebookCellStatusBarService`, que
//! guarda os provedores de itens da barra de status das células e junta os itens que eles devolvem para uma célula.
//!
//! Divergências:
//! - O `onDidChangeStatusBarItems` opcional do provedor é `Option<Event<()>>`, e o `catch` do `await p.provideCellStatusBarItems(...)` é o
//!   tratamento do `Result` da promessa (`onUnexpectedExternalError`).
//! - `splice(idx, 1)` com `idx === -1` (provedor já removido) remove o último elemento no original; aqui o descarte repetido é inócuo
//!   porque o descarte de `to_disposable` roda uma vez só.
//! - O construtor devolve `Rc<Self>`.

use std::cell::RefCell;
use std::rc::Rc;

use je_base::cancellation::CancellationToken;
use je_base::errors::on_unexpected_external_error;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, DisposableRef, IDisposable, to_disposable};
use je_base::promise::Promise;
use je_base::uri::URI;

use crate::contrib::notebook::common::notebook_cell_status_bar_service::INotebookCellStatusBarService;
use crate::contrib::notebook::common::notebook_common::{INotebookCellStatusBarItemList, INotebookCellStatusBarItemProvider};

/// `NotebookCellStatusBarService`.
pub struct NotebookCellStatusBarService {
    base: Rc<Disposable>,
    on_did_change_providers: Rc<Emitter<()>>,
    on_did_change_items: Rc<Emitter<()>>,
    providers: Rc<RefCell<Vec<Rc<dyn INotebookCellStatusBarItemProvider>>>>,
}

impl NotebookCellStatusBarService {
    pub fn new() -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change_providers = Emitter::<()>::new();
        let on_did_change_items = Emitter::<()>::new();
        base.register(on_did_change_providers.clone());
        base.register(on_did_change_items.clone());
        Rc::new(Self { base, on_did_change_providers, on_did_change_items, providers: Rc::new(RefCell::new(Vec::new())) })
    }
}

impl INotebookCellStatusBarService for NotebookCellStatusBarService {
    fn on_did_change_providers(&self) -> Event<()> {
        self.on_did_change_providers.event()
    }

    fn on_did_change_items(&self) -> Event<()> {
        self.on_did_change_items.event()
    }

    fn register_cell_status_bar_item_provider(&self, provider: Rc<dyn INotebookCellStatusBarItemProvider>) -> DisposableRef {
        self.providers.borrow_mut().push(provider.clone());
        let change_listener = provider.on_did_change_status_bar_items().map(|event| {
            let on_did_change_items = self.on_did_change_items.clone();
            event.listen(move |_| on_did_change_items.fire(()))
        });

        self.on_did_change_providers.fire(());

        let providers = self.providers.clone();
        to_disposable(move || {
            if let Some(change_listener) = &change_listener {
                change_listener.dispose();
            }
            let mut providers = providers.borrow_mut();
            if let Some(idx) = providers.iter().position(|candidate| std::ptr::addr_eq(Rc::as_ptr(candidate), Rc::as_ptr(&provider))) {
                providers.remove(idx);
            }
        })
    }

    fn get_status_bar_items_for_cell(&self, doc_uri: &URI, cell_index: usize, view_type: &str, token: &CancellationToken) -> Promise<Vec<INotebookCellStatusBarItemList>> {
        let providers: Vec<Rc<dyn INotebookCellStatusBarItemProvider>> =
            self.providers.borrow().iter().filter(|p| p.view_type() == view_type || p.view_type() == "*").cloned().collect();
        let lists = providers
            .iter()
            .map(|p| {
                p.provide_cell_status_bar_items(doc_uri, cell_index, token).then_result(|result| match result {
                    Ok(list) => Ok(list.unwrap_or(INotebookCellStatusBarItemList { items: Vec::new(), dispose: None })),
                    Err(error) => {
                        on_unexpected_external_error(&error);
                        Ok(INotebookCellStatusBarItemList { items: Vec::new(), dispose: None })
                    }
                })
            })
            .collect();
        Promise::all(lists)
    }
}

impl IDisposable for NotebookCellStatusBarService {
    fn dispose(&self) {
        self.base.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "NotebookCellStatusBarService"
    }
}
