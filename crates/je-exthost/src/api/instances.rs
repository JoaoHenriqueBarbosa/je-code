//! Os objetos da API que a extensão segura por referência, com a identidade preservada.
//!
//! Um [`JsData::Object`] que atravessa a fronteira vira um objeto NOVO a cada travessia: o objeto que `createCommentThread` devolve
//! não seria o mesmo que o comando recebe depois. [`Instances`] cria a instância da classe nativa uma vez, guarda o [`JsHandle`] dela
//! (o coletor não o solta) e entrega sempre o mesmo, como o original devolve `commentThread.value`.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use zjsc::api::embed_node::{JsData, JsHandle, NodeRunner};

use crate::api::handle_object;

/// Troca, na lista `table[owner]` de pares (item do host, objeto da extensão), os objetos que chegaram por cópia pelos `items` por
/// referência (na mesma ordem): a extensão passa a receber de volta o objeto que ela atribuiu, com `===`.
pub fn hold_by_reference<T>(table: &RefCell<HashMap<u32, Vec<(Rc<T>, JsData)>>>, owner: u32, items: Vec<JsData>) {
    if let Some(pairs) = table.borrow_mut().get_mut(&owner) {
        for (pair, item) in pairs.iter_mut().zip(items) {
            pair.1 = item;
        }
    }
}

/// As instâncias vivas das classes de objetos com `_id`, por (classe, id).
#[derive(Default)]
pub struct Instances {
    held: RefCell<HashMap<(&'static str, u32), Rc<JsHandle>>>,
}

impl Instances {
    /// A instância da classe `class` com o `_id` `id`: a mesma em toda chamada, até [`Instances::forget`].
    pub fn get(&self, runner: &NodeRunner, class: &'static str, id: u32) -> JsData {
        let held = self.held.borrow().get(&(class, id)).cloned();
        let handle = held.unwrap_or_else(|| {
            let created = Rc::new(runner.import(&handle_object(class, id)));
            self.held.borrow_mut().insert((class, id), created.clone());
            created
        });
        JsData::Handle(handle)
    }

    /// Solta a instância (o objeto foi descartado).
    pub fn forget(&self, class: &'static str, id: u32) {
        self.held.borrow_mut().remove(&(class, id));
    }
}
