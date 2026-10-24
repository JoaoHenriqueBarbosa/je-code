//! `vscode.EventEmitter`, `vscode.CancellationTokenSource` e as funções de evento (`onDidX`) que os namespaces entregam.
//!
//! Uma função de evento (`Event<T>` do `vscode.d.ts`) é `(listener, thisArgs?, disposables?) => Disposable`: [`event_function`] a
//! cria sobre um [`Event`] do Rust, e o `Disposable` devolvido passa pela [`DisposableTable`].
//!
//! Divergências:
//! - O motor entrega um objeto comum ou array por CÓPIA (`JsData`), e só função, promessa e demais células por identidade. Por
//!   isso `thisArgs` só vale quando é um [`JsData::Handle`] (uma função, por exemplo); um objeto comum como `thisArgs` é ignorado e
//!   o listener roda sem `this`. O argumento `disposables` (a lista onde o original empurra o `Disposable`) também chegaria copiado,
//!   então não é preenchido: o chamador guarda o `Disposable` devolvido.
//! - O valor de `EventEmitter#fire(data)` chega ao listener como a mesma cópia que atravessou a fronteira (objeto comum e array
//!   perdem a identidade; função e instância de classe nativa ficam).
//! - `emitter.event` devolve uma função nova a cada leitura (no original é uma só por emitter, mas ela é só um atalho para o
//!   `Emitter`, então escutar por qualquer das duas é o mesmo).
//! - `CancellationToken` não é exportada em `vscode` (é uma interface no `vscode.d.ts`): a classe nativa existe para o protótipo
//!   dos tokens que o host entrega (`isCancellationRequested` e `onCancellationRequested` vivos, não uma cópia).
//! - Um erro lançado por um listener vai ao tratador de erros inesperados (`onUnexpectedError`), como o `Emitter` do original; o
//!   `EventEmitter` do original relata o erro pelo `console.error` da extensão, e aqui o destino é o mesmo stderr do console.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use je_base::cancellation::{CancellationToken, CancellationTokenSource};
use je_base::errors::{on_unexpected_error, Error};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::IDisposable;
use zjsc::api::embed_node::{ClassSpec, JsData, MethodCallback, NodeRunner, CLASS_KEY};

use crate::api::disposable::DisposableTable;
use crate::convert::{field, js_to_error};

/// Os emissores, as fontes de cancelamento e os tokens vivos, por id. Os objetos JavaScript carregam só o id (`_id`).
#[derive(Default)]
pub struct EventTables {
    next: Cell<u64>,
    emitters: RefCell<HashMap<u64, Rc<Emitter<JsData>>>>,
    sources: RefCell<HashMap<u64, CancellationTokenSource>>,
    tokens: RefCell<HashMap<u64, CancellationToken>>,
}

impl EventTables {
    pub fn new() -> Rc<Self> {
        Rc::new(Self::default())
    }

    fn next_id(&self) -> u64 {
        let id = self.next.get() + 1;
        self.next.set(id);
        id
    }

    /// O token do Rust como o objeto `CancellationToken` que o JavaScript lê (o mesmo token vira sempre o mesmo id).
    pub fn token_to_js(&self, token: CancellationToken) -> JsData {
        let existing = self.tokens.borrow().iter().find(|(_, known)| known.is_same(&token)).map(|(id, _)| *id);
        let id = existing.unwrap_or_else(|| {
            let id = self.next_id();
            self.tokens.borrow_mut().insert(id, token);
            id
        });
        JsData::Object(vec![(CLASS_KEY.to_owned(), JsData::Str("CancellationToken".to_owned())), ("_id".to_owned(), JsData::Num(id as f64))])
    }

    /// O token do Rust que um objeto `CancellationToken` do JavaScript representa; `None` se não é um dos que o host entregou.
    pub fn token_of(&self, value: &JsData) -> Option<CancellationToken> {
        self.tokens.borrow().get(&id_of(value)?).cloned()
    }
}

fn id_of(value: &JsData) -> Option<u64> {
    match field(value, "_id") {
        Some(JsData::Num(id)) => Some(*id as u64),
        _ => None,
    }
}

/// A função de evento `(listener, thisArgs?, disposables?) => Disposable` sobre `event`; `to_js` converte o valor do evento.
pub fn event_function<T: 'static>(runner: &NodeRunner, table: &Rc<DisposableTable>, name: &str, event: Event<T>, to_js: impl Fn(&T) -> JsData + 'static) -> JsData {
    let (call_runner, table) = (runner.clone(), table.clone());
    let to_js = Rc::new(to_js);
    let label = name.to_owned();
    let function = runner.make_function(
        name,
        3,
        Rc::new(move |args| {
            let Some(JsData::Handle(listener)) = args.first().cloned() else {
                return Err(format!("{label}: o listener precisa ser uma função"));
            };
            let this_arg = match args.get(1) {
                Some(handle @ JsData::Handle(_)) => Some(handle.clone()),
                _ => None,
            };
            let (runner, convert) = (call_runner.clone(), to_js.clone());
            let subscription = event.listen(move |value| {
                let payload = convert(value);
                let outcome = match &this_arg {
                    Some(this_arg) => runner.call_method(&listener, "call", &[this_arg.clone(), payload]).map(|_| ()).map_err(Error::new),
                    None => runner.call_data_throwing(&listener, &[payload]).map(|_| ()).map_err(|thrown| js_to_error(&thrown)),
                };
                if let Err(error) = outcome {
                    on_unexpected_error(&error);
                }
            });
            Ok(table.to_js(subscription))
        }),
    );
    JsData::Handle(Rc::new(function))
}

/// `vscode.EventEmitter`.
fn define_event_emitter(runner: &NodeRunner, table: &Rc<DisposableTable>, tables: &Rc<EventTables>) -> JsData {
    let emitter_of = {
        let tables = tables.clone();
        move |this: &JsData| -> Result<Rc<Emitter<JsData>>, String> {
            id_of(this).and_then(|id| tables.emitters.borrow().get(&id).cloned()).ok_or_else(|| "EventEmitter: a instância já foi descartada".to_owned())
        }
    };
    let construct_tables = tables.clone();
    let fire_of = emitter_of.clone();
    let dispose_of = emitter_of.clone();
    let dispose_tables = tables.clone();
    let (event_runner, event_table) = (runner.clone(), table.clone());
    let methods: Vec<(String, u32, MethodCallback)> = vec![
        (
            "fire".to_owned(),
            1,
            Rc::new(move |this, args| {
                fire_of(this)?.fire(args.first().cloned().unwrap_or(JsData::Undefined));
                Ok(JsData::Undefined)
            }),
        ),
        (
            "dispose".to_owned(),
            0,
            Rc::new(move |this, _| {
                if let Some(id) = id_of(this) {
                    let _ = dispose_of(this).map(|emitter| emitter.dispose());
                    dispose_tables.emitters.borrow_mut().remove(&id);
                }
                Ok(JsData::Undefined)
            }),
        ),
    ];
    let getters: Vec<(String, MethodCallback)> = vec![(
        "event".to_owned(),
        Rc::new(move |this, _| Ok(event_function(&event_runner, &event_table, "event", emitter_of(this)?.event(), |data: &JsData| data.clone()))),
    )];
    let constructor = runner.define_class(ClassSpec {
        name: "EventEmitter".to_owned(),
        length: 0,
        construct: Rc::new(move |_| {
            let id = construct_tables.next_id();
            construct_tables.emitters.borrow_mut().insert(id, Emitter::new());
            Ok(JsData::Object(vec![("_id".to_owned(), JsData::Num(id as f64))]))
        }),
        methods,
        getters,
        statics: vec![],
        parent: None,
    });
    JsData::Handle(Rc::new(constructor))
}

/// A classe nativa dos tokens que o host entrega (`isCancellationRequested` e `onCancellationRequested` vivos).
fn define_cancellation_token(runner: &NodeRunner, table: &Rc<DisposableTable>, tables: &Rc<EventTables>) -> JsData {
    let (flag_tables, event_tables) = (tables.clone(), tables.clone());
    let (event_runner, event_table) = (runner.clone(), table.clone());
    let constructor = runner.define_class(ClassSpec {
        name: "CancellationToken".to_owned(),
        length: 0,
        construct: Rc::new(|_| Err("CancellationToken não tem construtor público".to_owned())),
        methods: vec![],
        getters: vec![
            (
                "isCancellationRequested".to_owned(),
                Rc::new(move |this, _| Ok(JsData::Bool(flag_tables.token_of(this).is_some_and(|token| token.is_cancellation_requested())))),
            ),
            (
                "onCancellationRequested".to_owned(),
                Rc::new(move |this, _| {
                    let event = event_tables.token_of(this).map_or_else(Event::none, |token| token.on_cancellation_requested());
                    Ok(event_function(&event_runner, &event_table, "onCancellationRequested", event, |()| JsData::Undefined))
                }),
            ),
        ],
        statics: vec![],
        parent: None,
    });
    JsData::Handle(Rc::new(constructor))
}

/// `vscode.CancellationTokenSource`.
fn define_cancellation_token_source(runner: &NodeRunner, tables: &Rc<EventTables>) -> JsData {
    let source_of = {
        let tables = tables.clone();
        move |this: &JsData| -> Result<CancellationTokenSource, String> {
            id_of(this).and_then(|id| tables.sources.borrow().get(&id).cloned()).ok_or_else(|| "CancellationTokenSource: a instância já foi descartada".to_owned())
        }
    };
    let construct_tables = tables.clone();
    let token_tables = tables.clone();
    let (cancel_of, dispose_of, token_of) = (source_of.clone(), source_of.clone(), source_of);
    let methods: Vec<(String, u32, MethodCallback)> = vec![
        (
            "cancel".to_owned(),
            0,
            Rc::new(move |this, _| {
                cancel_of(this)?.cancel();
                Ok(JsData::Undefined)
            }),
        ),
        (
            "dispose".to_owned(),
            0,
            Rc::new(move |this, _| {
                dispose_of(this)?.dispose_with(false);
                Ok(JsData::Undefined)
            }),
        ),
    ];
    let getters: Vec<(String, MethodCallback)> = vec![("token".to_owned(), Rc::new(move |this, _| Ok(token_tables.token_to_js(token_of(this)?.token()))))];
    let constructor = runner.define_class(ClassSpec {
        name: "CancellationTokenSource".to_owned(),
        length: 0,
        construct: Rc::new(move |_| {
            let id = construct_tables.next_id();
            construct_tables.sources.borrow_mut().insert(id, CancellationTokenSource::new(None));
            Ok(JsData::Object(vec![("_id".to_owned(), JsData::Num(id as f64))]))
        }),
        methods,
        getters,
        statics: vec![],
        parent: None,
    });
    JsData::Handle(Rc::new(constructor))
}

/// As classes deste módulo como entradas do objeto `vscode` (`CancellationToken` fica de fora: ver as divergências do módulo,
/// mas o protótipo precisa ser definido, e o é aqui).
pub fn classes(runner: &NodeRunner, table: &Rc<DisposableTable>, tables: &Rc<EventTables>) -> Vec<(String, JsData)> {
    let _internal_token_class = define_cancellation_token(runner, table, tables);
    vec![
        ("EventEmitter".to_owned(), define_event_emitter(runner, table, tables)),
        ("CancellationTokenSource".to_owned(), define_cancellation_token_source(runner, tables)),
    ]
}
