//! Porte de `src/vs/base/browser/ui/list/rowCache.ts`: o cache de linhas (nós) reaproveitáveis por
//! `templateId`, com a transação que adia a remoção dos nós para pular o trabalho quando uma linha sai e
//! volta no mesmo passe.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use je_ui::{NodeId, Role, Ui};

use crate::common::throw;
use crate::list::list_types::{IListRenderer, RendererMap, TemplateData};

/// `IRow`.
#[derive(Clone)]
pub struct Row {
    pub dom_node: NodeId,
    pub template_id: Rc<str>,
    pub template_data: TemplateData,
}

pub struct RowCache<T> {
    ui: Ui,
    renderers: RendererMap<T>,
    cache: RefCell<HashMap<Rc<str>, Vec<Row>>>,
    transaction_nodes_pending_removal: RefCell<HashSet<NodeId>>,
    in_transaction: Cell<bool>,
}

impl<T> RowCache<T> {
    pub fn new(ui: &Ui, renderers: RendererMap<T>) -> Self {
        Self { ui: ui.clone(), renderers, cache: RefCell::new(HashMap::new()), transaction_nodes_pending_removal: RefCell::new(HashSet::new()), in_transaction: Cell::new(false) }
    }

    /// Devolve uma linha: nova, ou uma liberada antes com o mesmo `templateId`. O segundo valor diz se o
    /// nó reaproveitado ainda está na árvore numa posição velha (`isReusingConnectedDomNode`).
    pub fn alloc(&self, template_id: &Rc<str>) -> (Row, bool) {
        let reused = self.cache.borrow_mut().entry(template_id.clone()).or_default().pop();
        match reused {
            Some(row) => {
                let is_stale = self.transaction_nodes_pending_removal.borrow_mut().remove(&row.dom_node);
                (row, is_stale)
            }
            None => {
                let dom_node = {
                    let mut tree = self.ui.tree_mut();
                    let node = tree.create_detached(Role::ListItem);
                    tree.add_class(node, "monaco-list-row");
                    node
                };
                let renderer = self.get_renderer(template_id);
                let template_data = renderer.render_template(dom_node);
                (Row { dom_node, template_id: template_id.clone(), template_data }, false)
            }
        }
    }

    /// Libera a linha para reuso.
    pub fn release(&self, row: &Row) {
        self.release_row(row);
    }

    /// Começa um conjunto de mudanças que usam o cache.
    pub fn transact(&self, make_changes: impl FnOnce()) {
        if self.in_transaction.get() {
            throw("Already in transaction");
        }

        self.in_transaction.set(true);

        struct Finish<'a, T>(&'a RowCache<T>);
        impl<T> Drop for Finish<'_, T> {
            fn drop(&mut self) {
                let pending: Vec<NodeId> = self.0.transaction_nodes_pending_removal.borrow().iter().copied().collect();
                for dom_node in pending {
                    self.0.do_remove_node(dom_node);
                }
                self.0.transaction_nodes_pending_removal.borrow_mut().clear();
                self.0.in_transaction.set(false);
            }
        }
        let _finish = Finish(self);
        make_changes();
    }

    fn release_row(&self, row: &Row) {
        if self.in_transaction.get() {
            self.transaction_nodes_pending_removal.borrow_mut().insert(row.dom_node);
        } else {
            self.do_remove_node(row.dom_node);
        }
        self.cache.borrow_mut().entry(row.template_id.clone()).or_default().push(row.clone());
    }

    fn do_remove_node(&self, dom_node: NodeId) {
        let mut tree = self.ui.tree_mut();
        tree.remove_class(dom_node, "scrolling");
        tree.detach(dom_node);
    }

    pub fn dispose(&self) {
        let cache = std::mem::take(&mut *self.cache.borrow_mut());
        for (template_id, rows) in cache {
            for row in rows {
                let renderer = self.get_renderer(&template_id);
                renderer.dispose_template(&row.template_data);
                self.ui.remove(row.dom_node);
            }
        }
        self.transaction_nodes_pending_removal.borrow_mut().clear();
    }

    fn get_renderer(&self, template_id: &str) -> Rc<dyn IListRenderer<T>> {
        match self.renderers.borrow().get(template_id) {
            Some(renderer) => renderer.clone(),
            None => throw(&format!("No renderer found for {template_id}")),
        }
    }
}
