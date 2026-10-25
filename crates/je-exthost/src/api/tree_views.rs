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
//! `TreeView#badge` (`{ value, tooltip }`), `message` como texto ou `MarkdownString` (esta pede a proposta `treeViewMarkdownMessage`, que o
//! `ExtHostTreeViews` confere), `TreeItem#checkboxState` (lido do item pelo `ExtHostTreeViews`), `onDidChangeCheckboxState` e a opção
//! `dragAndDropController` (`dragMimeTypes`, `dropMimeTypes`, `handleDrag`, `handleDrop`) com as classes `DataTransfer` e
//! `DataTransferItem` de texto, que embrulham o `DataTransfer` portado.
//!
//! Falta: `TreeView#activeItem` e `onDidChangeActiveItem` (propostas de API), `DataTransferItem#asFile` (o item de arquivo só nasce do
//! lado da interface; aqui devolve `undefined`) e `DataTransfer[Symbol.iterator]` (`forEach` cobre a travessia).
//!
//! Divergências:
//! - O `DataTransferItem#value` atravessa o motor como cópia (o valor é lido por dados, sem identidade de objeto); o `DataTransfer` que
//!   o `handleDrag` e o `handleDrop` recebem fica vivo só enquanto o manipulador não assenta.
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
    ExtHostTreeViews, TreeCheckboxChangeEvent, TreeDataProvider, TreeDragAndDropController, TreeHandleDrag, TreeHandleDrop, TreeViewExpansionEvent, TreeViewHandle,
    TreeViewOptions, TreeViewSelectionChangeEvent, TreeViewVisibilityChangeEvent,
};
use je_workbench::api::common::ext_host_types::{DataTransfer, DataTransferItem, Documentation, ViewBadge};
use je_workbench::common::views::{IRevealOptions, RevealExpand};
use zjsc::api::embed_node::{ClassSpec, JsData, JsHandle, MethodCallback, NativeCallback, NodeRunner, CLASS_KEY};

use crate::api::commands::promise_to_js;
use crate::api::disposable::DisposableTable;
use crate::api::events::{event_function, EventTables};
use crate::api::js_object::{JsObjects, JsResult};
use crate::api::language_types::{documentation_from_js, strings_of};
use crate::api::languages::host_extension;
use crate::convert::{api_to_js, class_name, dynamic_to_js, exported, field, is_nullish, js_to_dynamic, opt_bool, opt_number, opt_string};

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

/// As `TreeView` e os `DataTransfer` que o JavaScript segura, por id (o objeto carrega só o `_id`).
#[derive(Default)]
pub struct TreeViewObjects {
    next: Cell<u64>,
    items: RefCell<HashMap<u64, Rc<TreeViewHandle>>>,
    transfers: RefCell<HashMap<u64, Rc<DataTransfer>>>,
}

impl TreeViewObjects {
    fn next_id(&self) -> u64 {
        let id = self.next.get() + 1;
        self.next.set(id);
        id
    }

    fn add(&self, view: Rc<TreeViewHandle>) -> u64 {
        let id = self.next_id();
        self.items.borrow_mut().insert(id, view);
        id
    }

    /// O objeto `DataTransfer` da extensão para `transfer`, com o id para soltá-lo depois.
    fn add_transfer(&self, transfer: Rc<DataTransfer>) -> (u64, JsData) {
        let id = self.next_id();
        self.transfers.borrow_mut().insert(id, transfer);
        (id, JsData::Object(vec![(CLASS_KEY.to_owned(), JsData::Str("DataTransfer".to_owned())), ("_id".to_owned(), JsData::Num(id as f64))]))
    }

    fn remove_transfer(&self, id: u64) {
        self.transfers.borrow_mut().remove(&id);
    }

    fn transfer_of(&self, this: &JsData) -> Result<Rc<DataTransfer>, String> {
        match field(this, "_id") {
            Some(JsData::Num(id)) => self.transfers.borrow().get(&(*id as u64)).cloned().ok_or_else(|| "DataTransfer: objeto desconhecido".to_owned()),
            _ => Err("DataTransfer: this inválido".to_owned()),
        }
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
        if let JsData::Handle(controller) = self.js.property(options, "dragAndDropController") {
            tree_options.drag_and_drop_controller = Some(self.drag_and_drop_controller(&controller));
        }
        let view = self.views.create_tree_view(&view_id, tree_options, &host_extension()).map_err(|error| error.message)?;
        Ok(self.view_object(view))
    }

    /// O `vscode.TreeDragAndDropController` da extensão como [`TreeDragAndDropController`].
    fn drag_and_drop_controller(&self, controller: &Rc<JsHandle>) -> Rc<TreeDragAndDropController> {
        let target = JsData::Handle(controller.clone());
        // a lista chega por referência: a cópia do motor é que se lê como lista de textos
        let mime_types = |name: &str| strings_of(Some(&exported(self.js.runner(), &self.js.property(&target, name)))).unwrap_or_default();
        let handle_drag = self.js.is_function(&target, "handleDrag").then(|| {
            let (context, target) = (self.clone(), target.clone());
            Rc::new(move |sources: &[Dynamic], transfer: &Rc<DataTransfer>, token: &CancellationToken| {
                let sources = JsData::Array(sources.iter().map(dynamic_to_js).collect());
                context.invoke_with_transfer(&target, "handleDrag", sources, transfer, token)
            }) as TreeHandleDrag
        });
        let handle_drop = self.js.is_function(&target, "handleDrop").then(|| {
            let (context, target) = (self.clone(), target.clone());
            Rc::new(move |element: Option<&Dynamic>, transfer: &Rc<DataTransfer>, token: &CancellationToken| {
                let element = element.map_or(JsData::Undefined, dynamic_to_js);
                context.invoke_with_transfer(&target, "handleDrop", element, transfer, token)
            }) as TreeHandleDrop
        });
        Rc::new(TreeDragAndDropController { drop_mime_types: mime_types("dropMimeTypes"), drag_mime_types: mime_types("dragMimeTypes"), handle_drag, handle_drop })
    }

    /// `target[method](first, dataTransfer, token)`: o `DataTransfer` fica vivo para a extensão até o manipulador assentar.
    fn invoke_with_transfer(&self, target: &JsData, method: &str, first: JsData, transfer: &Rc<DataTransfer>, token: &CancellationToken) -> MaybePromise<()> {
        let (id, object) = self.objects.add_transfer(transfer.clone());
        let result = self.js.invoke(target, method, vec![first, object, self.events.token_to_js(token.clone())]);
        let objects = self.objects.clone();
        match result {
            MaybePromise::Value(_) => {
                objects.remove_transfer(id);
                MaybePromise::Value(())
            }
            MaybePromise::Promise(promise) => MaybePromise::Promise(promise.then_result(move |outcome| {
                objects.remove_transfer(id);
                outcome.map(|_| ())
            })),
        }
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

/// Define a classe `TreeView` (protótipo dos objetos que `createTreeView` entrega) e devolve `DataTransferItem` e `DataTransfer`.
pub fn define_classes(runner: &NodeRunner, objects: &Rc<TreeViewObjects>, table: &Rc<DisposableTable>) -> Vec<(String, JsData)> {
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
                Documentation::Markdown(markdown) => api_to_js(&markdown),
            }),
        ),
        getter(
            "badge",
            Rc::new(|view| {
                view.badge().map_or(JsData::Undefined, |badge| {
                    JsData::Object(vec![("value".to_owned(), JsData::Num(badge.value)), ("tooltip".to_owned(), JsData::Str(badge.tooltip))])
                })
            }),
        ),
        getter("title", Rc::new(|view| JsData::Str(view.title()))),
        getter("description", Rc::new(|view| view.description().map_or(JsData::Undefined, JsData::Str))),
        element_event("onDidExpandElement", TreeViewHandle::on_did_expand_element),
        element_event("onDidCollapseElement", TreeViewHandle::on_did_collapse_element),
        view_event(runner, table, objects, "onDidChangeCheckboxState", TreeViewHandle::on_did_change_checkbox_state, |event: &TreeCheckboxChangeEvent| {
            let items = event.items.iter().map(|(element, state)| JsData::Array(vec![dynamic_to_js(element), JsData::Num(f64::from(state.value()))])).collect();
            JsData::Object(vec![("items".to_owned(), JsData::Array(items))])
        }),
        view_event(runner, table, objects, "onDidChangeSelection", TreeViewHandle::on_did_change_selection, |event: &TreeViewSelectionChangeEvent| {
            JsData::Object(vec![("selection".to_owned(), JsData::Array(event.selection.iter().map(dynamic_to_js).collect()))])
        }),
        view_event(runner, table, objects, "onDidChangeVisibility", TreeViewHandle::on_did_change_visibility, |event: &TreeViewVisibilityChangeEvent| {
            JsData::Object(vec![("visible".to_owned(), JsData::Bool(event.visible))])
        }),
    ];
    let (message_runner, badge_runner) = (runner.clone(), runner.clone());
    let setters: Vec<(String, MethodCallback)> = vec![
        setter(
            "message",
            Rc::new(move |view, value| {
                let value = value.map(|value| exported(&message_runner, value));
                view.set_message(documentation_from_js(value.as_ref())?.unwrap_or_else(|| Documentation::Text(String::new())))
            }),
        ),
        setter(
            "badge",
            Rc::new(move |view, value| {
                let badge = value.map(|value| exported(&badge_runner, value)).filter(|badge| !is_nullish(Some(badge)));
                view.set_badge(badge.map(|badge| ViewBadge {
                    value: opt_number(field(&badge, "value")).unwrap_or(0.0),
                    tooltip: opt_string(field(&badge, "tooltip")).unwrap_or_default(),
                }));
                Ok(())
            }),
        ),
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
    vec![("DataTransferItem".to_owned(), data_transfer_item_class(runner)), ("DataTransfer".to_owned(), data_transfer_class(runner, objects))]
}

/// O item de texto do `DataTransfer` portado como objeto da extensão (o `value` por dados).
fn item_to_js(item: &DataTransferItem) -> JsData {
    JsData::Object(vec![(CLASS_KEY.to_owned(), JsData::Str("DataTransferItem".to_owned())), ("value".to_owned(), dynamic_to_js(item.value()))])
}

/// O `DataTransferItem` portado de um objeto da extensão (`value`).
fn item_from_js(runner: &NodeRunner, value: Option<&JsData>) -> Result<Rc<DataTransferItem>, String> {
    let item = value.map(|value| exported(runner, value)).ok_or("DataTransferItem: falta o item")?;
    Ok(DataTransferItem::new(js_to_dynamic(field(&item, "value").unwrap_or(&JsData::Undefined))))
}

/// A classe `vscode.DataTransferItem`: `new DataTransferItem(value)`, `value`, `asString()` e `asFile()`.
fn data_transfer_item_class(runner: &NodeRunner) -> JsData {
    let as_string_runner = runner.clone();
    let constructor = runner.define_class(ClassSpec {
        name: "DataTransferItem".to_owned(),
        length: 1,
        construct: Rc::new(|args| Ok(JsData::Object(vec![("value".to_owned(), args.first().cloned().unwrap_or(JsData::Undefined))]))),
        methods: vec![
            (
                "asString".to_owned(),
                0,
                Rc::new(move |this, _| {
                    let item = DataTransferItem::new(js_to_dynamic(field(this, "value").unwrap_or(&JsData::Undefined)));
                    Ok(promise_to_js(&as_string_runner, item.as_string(), |text| JsData::Str(text.clone())))
                }),
            ),
            ("asFile".to_owned(), 0, Rc::new(|_, _| Ok(JsData::Undefined))),
        ],
        getters: vec![],
        statics: vec![],
        parent: None,
    });
    JsData::Handle(Rc::new(constructor))
}

/// A classe `vscode.DataTransfer`: `new DataTransfer(init?)`, `get`, `set` e `forEach`, sobre o `DataTransfer` portado.
fn data_transfer_class(runner: &NodeRunner, objects: &Rc<TreeViewObjects>) -> JsData {
    let (construct_objects, get_objects, set_objects, each_objects) = (objects.clone(), objects.clone(), objects.clone(), objects.clone());
    let (construct_runner, set_runner, each_runner) = (runner.clone(), runner.clone(), runner.clone());
    let constructor = runner.define_class(ClassSpec {
        name: "DataTransfer".to_owned(),
        length: 1,
        construct: Rc::new(move |args| {
            let mut init = Vec::new();
            if let Some(JsData::Array(entries)) = args.first() {
                for entry in entries {
                    let JsData::Array(pair) = exported(&construct_runner, entry) else { return Err("DataTransfer: cada entrada precisa ser [tipo, item]".to_owned()) };
                    let mime_type = opt_string(pair.first()).ok_or("DataTransfer: o tipo da entrada precisa ser texto")?;
                    init.push((mime_type, item_from_js(&construct_runner, pair.get(1))?));
                }
            }
            let (_, object) = construct_objects.add_transfer(DataTransfer::new(init));
            Ok(match object {
                JsData::Object(fields) => JsData::Object(fields.into_iter().filter(|(key, _)| key != CLASS_KEY).collect()),
                other => other,
            })
        }),
        methods: vec![
            (
                "get".to_owned(),
                1,
                Rc::new(move |this, args| {
                    let mime_type = opt_string(args.first()).ok_or("DataTransfer#get: o tipo precisa ser texto")?;
                    Ok(get_objects.transfer_of(this)?.get(&mime_type).map_or(JsData::Undefined, |item| item_to_js(&item)))
                }),
            ),
            (
                "set".to_owned(),
                2,
                Rc::new(move |this, args| {
                    let mime_type = opt_string(args.first()).ok_or("DataTransfer#set: o tipo precisa ser texto")?;
                    set_objects.transfer_of(this)?.set(&mime_type, item_from_js(&set_runner, args.get(1))?);
                    Ok(JsData::Undefined)
                }),
            ),
            (
                "forEach".to_owned(),
                2,
                Rc::new(move |this, args| {
                    let Some(JsData::Handle(callback)) = args.first() else { return Err("DataTransfer#forEach: o callback precisa ser função".to_owned()) };
                    for (mime_type, item) in each_objects.transfer_of(this)?.iter() {
                        each_runner.call_data(callback, &[item_to_js(&item), JsData::Str(mime_type), this.clone()])?;
                    }
                    Ok(JsData::Undefined)
                }),
            ),
        ],
        getters: vec![],
        statics: vec![],
        parent: None,
    });
    JsData::Handle(Rc::new(constructor))
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
