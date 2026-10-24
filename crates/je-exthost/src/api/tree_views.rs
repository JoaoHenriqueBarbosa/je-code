//! `vscode.window.createTreeView`, `registerTreeDataProvider`, a classe `vscode.TreeView` (que `createTreeView` devolve) e a classe
//! `vscode.TreeItem`.
//!
//! É a ponte para o `ExtHostTreeViews` portado (`extHostTreeViews.ts`): o `TreeDataProvider` da extensão é adaptado a um
//! [`TreeDataProvider`] de fechos (`getTreeItem`, `getChildren`, `getParent`, `resolveTreeItem` e `onDidChangeTreeData` chamam o objeto
//! da extensão com o `this` dele), e a `TreeView` entrega `selection`, `visible`, `message`, `title`, `description`, os eventos de
//! seleção, expansão e visibilidade, `reveal` e `dispose`.
//!
//! Os elementos da árvore são objetos da extensão e o `ExtHostTreeViews` os compara por identidade: o provedor é chamado por
//! referência (`JsObjects::invoke`), e cada elemento recebe um número estável (`JsObjects::identify`) que o [`Elements`] mapeia para o
//! mesmo [`Dynamic`], então `reveal(element)` e `getParent` enxergam o mesmo elemento que `getChildren` entregou.
//!
//! Falta: `TreeView#badge`, `onDidChangeCheckboxState` e `manageCheckboxState` além da opção, `TreeViewOptions#dragAndDropController`,
//! `TreeView#activeItem` e `onDidChangeActiveItem` (propostas de API), e a mensagem em markdown (`MarkdownString`).
//!
//! Divergências:
//! - O `vscode` é um só para todas as extensões: os registros usam a extensão `je.exthost` (sem `contributes.views`, então o título
//!   inicial da visão é vazio até o `title` ser definido).
//! - O `TreeItem` que `getTreeItem` devolve atravessa o motor como cópia (o `ExtHostTreeViews` lê as propriedades dele, não o
//!   identifica); por isso `resolveTreeItem` recebe uma cópia do item, e o que ele muta no argumento só vale se for devolvido.
//! - O objeto `TreeView` que `createTreeView` devolve tem `reveal` por referência (o elemento precisa manter a identidade); o resto
//!   são acessores do protótipo.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use je_base::cancellation::CancellationToken;
use je_base::dynamic::{Dynamic, ObjectRef};
use je_base::errors::on_unexpected_error;
use je_base::errors::Error;
use je_base::event::{Emitter, Event};
use je_base::promise::MaybePromise;
use je_workbench::api::common::ext_host_tree_views::{
    ExtHostTreeViews, TreeDataProvider, TreeViewExpansionEvent, TreeViewHandle, TreeViewOptions, TreeViewSelectionChangeEvent, TreeViewVisibilityChangeEvent,
};
use je_workbench::api::common::ext_host_types::Documentation;
use je_workbench::common::views::{IRevealOptions, RevealExpand};
use zjsc::api::embed_node::{ClassSpec, JsData, JsHandle, MethodCallback, NativeCallback, NodeRunner, CLASS_KEY};

use crate::api::commands::promise_to_js;
use crate::api::disposable::DisposableTable;
use crate::api::events::{event_function, EventTables};
use crate::api::js_object::{JsObjects, JsResult};
use crate::api::languages::host_extension;
use crate::convert::{class_name, dynamic_to_js, field, is_nullish, js_to_dynamic, opt_bool, opt_string};

/// `MaybePromise<T>` com `convert` aplicado ao valor quando ele chegar.
fn map_maybe<T: Clone + 'static, U: Clone + 'static>(value: MaybePromise<T>, convert: impl Fn(T) -> U + 'static) -> MaybePromise<U> {
    match value {
        MaybePromise::Value(value) => MaybePromise::Value(convert(value)),
        MaybePromise::Promise(promise) => MaybePromise::Promise(promise.then(convert)),
    }
}

// ---- os elementos

/// Os elementos da árvore que o JavaScript entregou: o mesmo objeto da extensão é sempre o mesmo [`Dynamic`].
struct Elements {
    js: JsObjects,
    known: RefCell<HashMap<u64, Dynamic>>,
}

impl Elements {
    fn to_dynamic(&self, value: &JsData) -> Dynamic {
        let JsData::Handle(handle) = value else { return js_to_dynamic(value) };
        match self.js.identify(value) {
            Some(id) => self.known.borrow_mut().entry(id).or_insert_with(|| Dynamic::Object(ObjectRef::with_host(handle.clone()))).clone(),
            None => js_to_dynamic(value),
        }
    }

    /// `T | T[] | undefined | null`.
    fn of_result(&self, result: JsResult) -> Dynamic {
        match result {
            JsResult::Nullish => Dynamic::Undefined,
            JsResult::Single(value) => self.to_dynamic(&value),
            JsResult::Many(values) => Dynamic::array(values.iter().map(|value| self.to_dynamic(value))),
        }
    }

    /// Uma cópia (por valor) do objeto, para o que o `ExtHostTreeViews` só lê (o `TreeItem`).
    fn copy_of(&self, value: &JsData) -> Dynamic {
        match value {
            JsData::Handle(handle) => js_to_dynamic(&self.js.runner().export(handle)),
            other => js_to_dynamic(other),
        }
    }
}

// ---- as visões vivas

/// As `TreeView` que o JavaScript segura, por id (o objeto carrega só o `_id`).
#[derive(Default)]
pub struct TreeViewObjects {
    next: Cell<u64>,
    items: RefCell<HashMap<u64, Rc<TreeViewHandle>>>,
}

impl TreeViewObjects {
    fn add(&self, view: Rc<TreeViewHandle>) -> u64 {
        let id = self.next.get() + 1;
        self.next.set(id);
        self.items.borrow_mut().insert(id, view);
        id
    }

    fn view_of(&self, this: &JsData) -> Result<Rc<TreeViewHandle>, String> {
        match field(this, "_id") {
            Some(JsData::Num(id)) => self.items.borrow().get(&(*id as u64)).cloned().ok_or_else(|| "TreeView: visão desconhecida".to_owned()),
            _ => Err("TreeView: this inválido".to_owned()),
        }
    }
}

// ---- o contexto do namespace

#[derive(Clone)]
struct TreeContext {
    js: JsObjects,
    table: Rc<DisposableTable>,
    events: Rc<EventTables>,
    views: Rc<ExtHostTreeViews>,
    objects: Rc<TreeViewObjects>,
    elements: Rc<Elements>,
}

impl TreeContext {
    /// O `vscode.TreeDataProvider` da extensão como [`TreeDataProvider`].
    fn tree_data_provider(&self, provider: &Rc<JsHandle>) -> Result<Rc<TreeDataProvider>, String> {
        let target = JsData::Handle(provider.clone());
        for required in ["getTreeItem", "getChildren"] {
            if !self.js.is_function(&target, required) {
                return Err(format!("treeDataProvider precisa de {required}"));
            }
        }

        let get_tree_item = {
            let (context, target) = (self.clone(), target.clone());
            Rc::new(move |element: &Dynamic| {
                let result = context.js.invoke(&target, "getTreeItem", vec![dynamic_to_js(element)]);
                let elements = context.elements.clone();
                map_maybe(result, move |result| match result {
                    JsResult::Nullish => Dynamic::Undefined,
                    JsResult::Single(item) => elements.copy_of(&item),
                    JsResult::Many(items) => Dynamic::array(items.iter().map(|item| elements.copy_of(item))),
                })
            })
        };

        let get_children = {
            let (context, target) = (self.clone(), target.clone());
            Rc::new(move |element: Option<&Dynamic>| {
                let result = context.js.invoke(&target, "getChildren", vec![element.map_or(JsData::Undefined, dynamic_to_js)]);
                let elements = context.elements.clone();
                map_maybe(result, move |result| match result {
                    JsResult::Nullish => None,
                    JsResult::Single(value) => Some(vec![elements.to_dynamic(&value)]),
                    JsResult::Many(values) => Some(values.iter().map(|value| elements.to_dynamic(value)).collect::<Vec<Dynamic>>()),
                })
            })
        };

        let get_parent = self.js.is_function(&target, "getParent").then(|| {
            let (context, target) = (self.clone(), target.clone());
            Rc::new(move |element: &Dynamic| {
                let result = context.js.invoke(&target, "getParent", vec![dynamic_to_js(element)]);
                let elements = context.elements.clone();
                map_maybe(result, move |result| elements.of_result(result))
            }) as Rc<dyn Fn(&Dynamic) -> MaybePromise<Dynamic>>
        });

        let resolve_tree_item = self.js.is_function(&target, "resolveTreeItem").then(|| {
            let (context, target) = (self.clone(), target.clone());
            Rc::new(move |item: &Dynamic, element: &Dynamic, token: &CancellationToken| {
                let args = vec![dynamic_to_js(item), dynamic_to_js(element), context.events.token_to_js(token.clone())];
                let result = context.js.invoke(&target, "resolveTreeItem", args);
                let elements = context.elements.clone();
                map_maybe(result, move |result| match result {
                    JsResult::Nullish => Dynamic::Undefined,
                    JsResult::Single(item) => elements.copy_of(&item),
                    JsResult::Many(_) => Dynamic::Undefined,
                })
            }) as Rc<dyn Fn(&Dynamic, &Dynamic, &CancellationToken) -> MaybePromise<Dynamic>>
        });

        let on_did_change_tree_data = match self.js.property(&target, "onDidChangeTreeData") {
            JsData::Handle(event) if self.js.is_function(&target, "onDidChangeTreeData") => {
                let emitter: Rc<Emitter<Dynamic>> = Emitter::new();
                let (fire, elements) = (emitter.clone(), self.elements.clone());
                match self.js.listen(&event, move |result| fire.fire(elements.of_result(result))) {
                    Ok(_) => Some(emitter.event()),
                    Err(message) => {
                        on_unexpected_error(&Error::new(message));
                        None
                    }
                }
            }
            _ => None,
        };

        Ok(Rc::new(TreeDataProvider { get_tree_item, get_children, get_parent, resolve_tree_item, on_did_change_tree_data }))
    }

    /// `createTreeView(viewId, options)`.
    fn create_tree_view(&self, args: &[JsData]) -> Result<JsData, String> {
        let view_id = opt_string(args.first()).ok_or("createTreeView: o viewId precisa ser texto")?;
        let options = args.get(1).filter(|options| !is_nullish(Some(*options))).ok_or("Options with treeDataProvider is mandatory")?;
        let JsData::Handle(provider) = self.js.property(options, "treeDataProvider") else { return Err("Options with treeDataProvider is mandatory".to_owned()) };
        let mut tree_options = TreeViewOptions::new(self.tree_data_provider(&provider)?);
        let flag = |name: &str| opt_bool(Some(&self.js.property(options, name))).unwrap_or(false);
        tree_options.show_collapse_all = flag("showCollapseAll");
        tree_options.can_select_many = flag("canSelectMany");
        tree_options.manage_checkbox_state_manually = flag("manageCheckboxStateManually");
        let view = self.views.create_tree_view(&view_id, tree_options, &host_extension()).map_err(|error| error.message)?;
        Ok(self.view_object(view))
    }

    /// O objeto `TreeView` da extensão: o `_id` e o `reveal` por referência.
    fn view_object(&self, view: Rc<TreeViewHandle>) -> JsData {
        let id = self.objects.add(view.clone());
        let context = self.clone();
        let reveal = self.js.runner().make_function_by_reference("reveal", 2, Rc::new(move |args| context.reveal(&view, args)));
        JsData::Object(vec![
            (CLASS_KEY.to_owned(), JsData::Str("TreeView".to_owned())),
            ("_id".to_owned(), JsData::Num(id as f64)),
            ("reveal".to_owned(), JsData::Handle(Rc::new(reveal))),
        ])
    }

    /// `reveal(element, options?)`.
    fn reveal(&self, view: &Rc<TreeViewHandle>, args: &[JsData]) -> Result<JsData, String> {
        let element = self.elements.to_dynamic(args.first().ok_or("reveal: falta o elemento")?);
        let options = args.get(1).filter(|options| !is_nullish(Some(*options))).map(|options| IRevealOptions {
            select: opt_bool(Some(&self.js.property(options, "select"))),
            focus: opt_bool(Some(&self.js.property(options, "focus"))),
            expand: match self.js.property(options, "expand") {
                JsData::Bool(flag) => Some(RevealExpand::Bool(flag)),
                JsData::Num(depth) => Some(RevealExpand::Depth(depth)),
                _ => None,
            },
        });
        Ok(promise_to_js(self.js.runner(), view.reveal(element, options), |()| JsData::Undefined))
    }
}

fn view_event<T: 'static>(
    runner: &NodeRunner,
    table: &Rc<DisposableTable>,
    objects: &Rc<TreeViewObjects>,
    name: &'static str,
    source: impl Fn(&TreeViewHandle) -> Event<T> + 'static,
    to_js: impl Fn(&T) -> JsData + Clone + 'static,
) -> (String, MethodCallback) {
    let (runner, table, objects) = (runner.clone(), table.clone(), objects.clone());
    (
        name.to_owned(),
        Rc::new(move |this, _| {
            let view = objects.view_of(this)?;
            Ok(event_function(&runner, &table, name, source(&view), to_js.clone()))
        }),
    )
}

/// Define a classe `TreeView` (protótipo dos objetos que `createTreeView` entrega).
pub fn define_classes(runner: &NodeRunner, objects: &Rc<TreeViewObjects>, table: &Rc<DisposableTable>) {
    let getter = |name: &str, read: Rc<dyn Fn(&TreeViewHandle) -> JsData>| -> (String, MethodCallback) {
        let objects = objects.clone();
        (name.to_owned(), Rc::new(move |this, _| Ok(read(&*objects.view_of(this)?))))
    };
    let setter = |name: &str, write: Rc<dyn Fn(&TreeViewHandle, Option<&JsData>) -> Result<(), Error>>| -> (String, MethodCallback) {
        let objects = objects.clone();
        (
            name.to_owned(),
            Rc::new(move |this, args| {
                write(&*objects.view_of(this)?, args.first()).map_err(|error| error.message)?;
                Ok(JsData::Undefined)
            }),
        )
    };
    let dispose_objects = objects.clone();
    let element_event = |name: &'static str, source: fn(&TreeViewHandle) -> Event<TreeViewExpansionEvent>| {
        view_event(runner, table, objects, name, source, |event: &TreeViewExpansionEvent| JsData::Object(vec![("element".to_owned(), dynamic_to_js(&event.element))]))
    };
    let getters: Vec<(String, MethodCallback)> = vec![
        getter("selection", Rc::new(|view| JsData::Array(view.selection().iter().map(dynamic_to_js).collect()))),
        getter("visible", Rc::new(|view| JsData::Bool(view.visible()))),
        getter(
            "message",
            Rc::new(|view| match view.message() {
                Documentation::Text(text) => JsData::Str(text),
                _ => JsData::Undefined,
            }),
        ),
        getter("title", Rc::new(|view| JsData::Str(view.title()))),
        getter("description", Rc::new(|view| view.description().map_or(JsData::Undefined, JsData::Str))),
        element_event("onDidExpandElement", TreeViewHandle::on_did_expand_element),
        element_event("onDidCollapseElement", TreeViewHandle::on_did_collapse_element),
        view_event(runner, table, objects, "onDidChangeSelection", TreeViewHandle::on_did_change_selection, |event: &TreeViewSelectionChangeEvent| {
            JsData::Object(vec![("selection".to_owned(), JsData::Array(event.selection.iter().map(dynamic_to_js).collect()))])
        }),
        view_event(runner, table, objects, "onDidChangeVisibility", TreeViewHandle::on_did_change_visibility, |event: &TreeViewVisibilityChangeEvent| {
            JsData::Object(vec![("visible".to_owned(), JsData::Bool(event.visible))])
        }),
    ];
    let setters: Vec<(String, MethodCallback)> = vec![
        setter("message", Rc::new(|view, value| view.set_message(Documentation::Text(opt_string(value).unwrap_or_default())))),
        setter(
            "title",
            Rc::new(|view, value| {
                view.set_title(&opt_string(value).unwrap_or_default());
                Ok(())
            }),
        ),
        setter(
            "description",
            Rc::new(|view, value| {
                view.set_description(opt_string(value).as_deref());
                Ok(())
            }),
        ),
    ];
    let methods: Vec<(String, u32, MethodCallback)> = vec![(
        "dispose".to_owned(),
        0,
        Rc::new(move |this, _| {
            je_base::lifecycle::IDisposable::dispose(&*dispose_objects.view_of(this)?);
            Ok(JsData::Undefined)
        }),
    )];
    runner.define_accessor_class(
        ClassSpec {
            name: "TreeView".to_owned(),
            length: 0,
            construct: Rc::new(|_| Err("TreeView não tem construtor público: use window.createTreeView".to_owned())),
            methods,
            getters,
            statics: vec![],
            parent: None,
        },
        setters,
    );
}

/// A classe `vscode.TreeItem`: `new TreeItem(label | resourceUri, collapsibleState?)`.
pub fn tree_item_class(runner: &NodeRunner) -> JsData {
    let constructor = runner.define_class(ClassSpec {
        name: "TreeItem".to_owned(),
        length: 2,
        construct: Rc::new(|args| {
            let first = args.first().cloned().unwrap_or(JsData::Undefined);
            let mut fields = Vec::new();
            if class_name(&first) == Some("Uri") {
                fields.push(("resourceUri".to_owned(), first));
            } else {
                fields.push(("label".to_owned(), first));
            }
            let collapsible_state = match args.get(1) {
                Some(JsData::Num(state)) => JsData::Num(*state),
                _ => JsData::Num(0.0),
            };
            fields.push(("collapsibleState".to_owned(), collapsible_state));
            Ok(JsData::Object(fields))
        }),
        methods: vec![],
        getters: vec![],
        statics: vec![],
        parent: None,
    });
    JsData::Handle(Rc::new(constructor))
}

/// As funções de `window` que recebem objetos da extensão (`createTreeView` e `registerTreeDataProvider`, por referência).
pub fn window_members(
    runner: &NodeRunner,
    js: &JsObjects,
    views: &Rc<ExtHostTreeViews>,
    objects: &Rc<TreeViewObjects>,
    table: &Rc<DisposableTable>,
    events: &Rc<EventTables>,
) -> Vec<(String, JsData)> {
    let context = TreeContext {
        js: js.clone(),
        table: table.clone(),
        events: events.clone(),
        views: views.clone(),
        objects: objects.clone(),
        elements: Rc::new(Elements { js: js.clone(), known: RefCell::new(HashMap::new()) }),
    };
    let reference = |name: &str, length: u32, body: NativeCallback| (name.to_owned(), JsData::Handle(Rc::new(runner.make_function_by_reference(name, length, body))));
    let create_context = context.clone();
    let register_context = context;
    vec![
        reference("createTreeView", 2, Rc::new(move |args| create_context.create_tree_view(args))),
        reference(
            "registerTreeDataProvider",
            2,
            Rc::new(move |args| {
                let view_id = opt_string(args.first()).ok_or("registerTreeDataProvider: o viewId precisa ser texto")?;
                let Some(JsData::Handle(provider)) = args.get(1) else { return Err("registerTreeDataProvider: o provedor precisa ser um objeto".to_owned()) };
                let provider = register_context.tree_data_provider(provider)?;
                let registration = register_context.views.register_tree_data_provider(&view_id, provider, &host_extension()).map_err(|error| error.message)?;
                Ok(register_context.table.to_js(Rc::new(registration)))
            }),
        ),
    ]
}
