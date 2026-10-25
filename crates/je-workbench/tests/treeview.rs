//! Porte de `src/vs/workbench/test/browser/treeview.test.ts`.
//!
//! Notas de tradução:
//! - `new TreeView('testTree', 'Test Title')` é o [`CustomTreeView::new_without_extension`]: a variante `TreeView` do original
//!   (a que ao ativar só cria a árvore) é a mesma struct sem identificador de extensão.
//! - `workbenchInstantiationService` entrega ao original os serviços que o [`CustomTreeViewServices`] reúne (duplos em
//!   `support/tree_view_services.rs`). O `ViewDescriptorService` (`IViewDescriptorService`) só serve à interface do original
//!   (local e cores da view) e não entra no `CustomTreeView` do porte.
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado no começo do caso (o `setup` do `suite` roda dentro dele).
//! - O `getChildrenBatch` do provedor é o [`ITreeViewDataProvider::get_children_batch`]; o `largestBatchSize` é uma célula
//!   compartilhada com ele.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
#[path = "support/tree_view_services.rs"]
mod tree_view_services;

use std::cell::Cell;
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_real_timers};
use je_base::data_transfer::VSDataTransfer;
use je_base::promise::Promise;
use je_editor::tree_views_dnd::TreeViewsDnDService;
use je_workbench::browser::parts::views::tree_view::CustomTreeView;
use je_workbench::common::tree_view_contracts::{ITreeView, ITreeViewDataProvider};
use je_workbench::common::views::{ITreeItem, TreeItemCollapsibleState};
use tree_view_services::{TestExtensionService, custom_tree_view_services};

/// `getChildrenOfItem(element?)`.
fn get_children_of_item(element: Option<&ITreeItem>) -> Option<Vec<ITreeItem>> {
    if element.is_some() {
        None
    } else {
        Some((0..100).map(|i| ITreeItem { handle: format!("item_{i}"), collapsible_state: TreeItemCollapsibleState::Expanded, ..ITreeItem::default() }).collect())
    }
}

/// O objeto literal `{ getChildren, getChildrenBatch }` do `setup`.
struct BatchCountingDataProvider {
    largest_batch_size: Rc<Cell<usize>>,
}

impl ITreeViewDataProvider for BatchCountingDataProvider {
    fn get_children(&self, element: Option<&ITreeItem>) -> Promise<Option<Vec<ITreeItem>>> {
        Promise::resolved(get_children_of_item(element))
    }

    fn get_children_batch(&self, elements: Option<&[ITreeItem]>) -> Option<Promise<Option<Vec<Vec<ITreeItem>>>>> {
        if let Some(elements) = elements
            && elements.len() > self.largest_batch_size.get()
        {
            self.largest_batch_size.set(elements.len());
        }
        Some(Promise::resolved(Some(match elements {
            Some(elements) => vec![Vec::new(); elements.len()],
            None => vec![get_children_of_item(None).unwrap_or_default()],
        })))
    }
}

#[test]
fn children_are_batched() {
    run_with_real_timers(async {
        // `ensureNoDisposablesAreLeakedInTestSuite()` e o `setup`.
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let largest_batch_size = Rc::new(Cell::new(0_usize));
        let drag_service: Rc<TreeViewsDnDService<Rc<VSDataTransfer>>> = Rc::new(TreeViewsDnDService::new());
        let services = custom_tree_view_services(Rc::new(TestExtensionService), drag_service);
        let tree_view = disposables.add_rc(CustomTreeView::new_without_extension("testTree", "Test Title", services));
        tree_view.set_data_provider(Some(Rc::new(BatchCountingDataProvider { largest_batch_size: largest_batch_size.clone() })));

        assert_eq!(largest_batch_size.get(), 0);
        tree_view.set_visibility(true);
        tree_view.refresh(None, None).await.unwrap();
        assert_eq!(largest_batch_size.get(), 100);
    });
}
