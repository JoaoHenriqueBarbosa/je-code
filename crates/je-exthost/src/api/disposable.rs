//! `vscode.Disposable` e a tabela que dá identidade aos descartáveis do Rust que o JavaScript segura.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use je_base::lifecycle::IDisposable;
use zjsc::api::embed_node::{ClassSpec, JsData, NodeRunner, CLASS_KEY};

use crate::convert::field;

/// Os descartáveis vivos, por id. O objeto JavaScript carrega só o id (`_id`).
#[derive(Default)]
pub struct DisposableTable {
    next: Cell<u64>,
    items: RefCell<HashMap<u64, Rc<dyn IDisposable>>>,
}

impl DisposableTable {
    pub fn new() -> Rc<Self> {
        Rc::new(Self::default())
    }

    pub fn add(&self, disposable: Rc<dyn IDisposable>) -> u64 {
        let id = self.next.get() + 1;
        self.next.set(id);
        self.items.borrow_mut().insert(id, disposable);
        id
    }

    /// Descarta e esquece `id`; uma segunda chamada não faz nada.
    pub fn dispose(&self, id: u64) {
        let taken = self.items.borrow_mut().remove(&id);
        if let Some(disposable) = taken {
            disposable.dispose();
        }
    }

    /// O objeto `Disposable` que o JavaScript recebe para `disposable`.
    pub fn to_js(&self, disposable: Rc<dyn IDisposable>) -> JsData {
        disposable_object(self.add(disposable))
    }
}

fn disposable_object(id: u64) -> JsData {
    JsData::Object(vec![(CLASS_KEY.to_owned(), JsData::Str("Disposable".to_owned())), ("_id".to_owned(), JsData::Num(id as f64))])
}

fn id_of(value: &JsData) -> Option<u64> {
    match field(value, "_id") {
        Some(JsData::Num(id)) => Some(*id as u64),
        _ => None,
    }
}

/// O `callOnDispose` do `new Disposable(fn)`: chama a função JavaScript uma vez.
struct CallOnDispose {
    runner: NodeRunner,
    callback: Rc<zjsc::api::embed_node::JsHandle>,
    called: Cell<bool>,
}

impl IDisposable for CallOnDispose {
    fn dispose(&self) {
        if !self.called.replace(true) {
            let _ = self.runner.call_data(&self.callback, &[]);
        }
    }
}

/// Define a classe `Disposable` (construtor, `dispose`, `Disposable.from`) em `runner`.
pub fn define(runner: &NodeRunner, table: &Rc<DisposableTable>) -> JsData {
    let (construct_runner, construct_table) = (runner.clone(), table.clone());
    let (dispose_table, from_table) = (table.clone(), table.clone());
    let constructor = runner.define_class(ClassSpec {
        name: "Disposable".to_owned(),
        length: 1,
        construct: Rc::new(move |args| {
            let JsData::Handle(callback) = args.first().cloned().unwrap_or(JsData::Undefined) else {
                return Err("Disposable: o argumento precisa ser uma função".to_owned());
            };
            let id = construct_table.add(Rc::new(CallOnDispose { runner: construct_runner.clone(), callback, called: Cell::new(false) }));
            Ok(JsData::Object(vec![("_id".to_owned(), JsData::Num(id as f64))]))
        }),
        methods: vec![(
            "dispose".to_owned(),
            0,
            Rc::new(move |this, _| {
                if let Some(id) = id_of(this) {
                    dispose_table.dispose(id);
                }
                Ok(JsData::Undefined)
            }),
        )],
        statics: vec![(
            "from".to_owned(),
            1,
            Rc::new(move |args| {
                let ids: Vec<u64> = args.iter().filter_map(id_of).collect();
                let table = from_table.clone();
                let all = Rc::new(je_workbench::api::common::ext_host_types::Disposable::new(move || {
                    for id in &ids {
                        table.dispose(*id);
                    }
                }));
                Ok(from_table.to_js(all))
            }),
        )],
    });
    JsData::Handle(Rc::new(constructor))
}
