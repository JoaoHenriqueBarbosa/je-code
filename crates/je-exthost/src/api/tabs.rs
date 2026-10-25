//! `vscode.window.tabGroups` e as classes `vscode.TabGroup`, `vscode.Tab` e `vscode.TabInput*`.
//!
//! É a ponte para o `ExtHostEditorTabs` portado (`extHostEditorTabs.ts`), que recebe `$acceptEditorTabModel`, `$acceptTabGroupUpdate` e
//! `$acceptTabOperation` do cliente `MainThreadEditorTabs` do workbench e fala com ele em `moveTab`, `closeTab` e `closeGroup`.
//! `tabGroups` tem `all`, `activeTabGroup`, `onDidChangeTabGroups`, `onDidChangeTabs` e `close(tab | tab[] | group | group[],
//! preserveFocus?)`; o `TabGroup`, `isActive`, `viewColumn`, `activeTab` e `tabs`; o `Tab`, `label`, `group`, `input`, `isActive`,
//! `isDirty`, `isPinned` e `isPreview`. As classes de entrada são `TabInputText`, `TabInputTextDiff`, `TabInputTextMerge`,
//! `TabInputCustom`, `TabInputNotebook`, `TabInputNotebookDiff`, `TabInputWebview`, `TabInputTerminal`, `TabInputInteractiveWindow`,
//! `TabInputChat` e `TabInputTextMultiDiff`.
//!
//! Divergências:
//! - O objeto que a extensão segura carrega só o `_id` (um número da [`TabObjects`], um só contador para abas e grupos), com os
//!   acessores no protótipo, como nas outras pontes. A identidade que o original mantém (`apiObject`) é preservada: a primeira vez que
//!   uma aba ou um grupo atravessa a fronteira, o objeto é pego POR REFERÊNCIA ([`JsObjects::property`]) e guardado, e toda leitura
//!   seguinte devolve esse mesmo objeto (`tab === tabGroups.activeTabGroup.activeTab` vale, e serve de chave de `Map` e `WeakMap`).
//!   O argumento de `close` volta pelo `_id`, que é o que `_findExtHostTabFromApi` faz por ponteiro no porte.
//! - A [`TabObjects`] não solta as abas e os grupos que já atravessaram a fronteira (o motor não avisa quando o objeto é descartado):
//!   uma aba guardada pela extensão depois de fechada continua legível, como no original, ao custo de a memória só ser liberada com o
//!   host.
//! - `Tab#input` devolve uma instância nova da classe `TabInput*` a cada leitura (no original é o mesmo objeto até o DTO da aba mudar):
//!   comparar `tab.input === tab.input` falha, `tab.input instanceof vscode.TabInputText` e os campos valem.
//! - Onde o original lança (`activeTabGroup` sem grupo ativo, aba de grupo descartado) o porte levanta uma exceção com a mesma
//!   mensagem.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_workbench::api::common::ext_host_editor_tabs::{AnyTabInput, CloseTarget, ExtHostEditorTabs, Tab, TabChangeEvent, TabGroup, TabGroupChangeEvent, TabGroups};
use zjsc::api::embed_node::{ClassSpec, JsData, MethodCallback, NodeRunner};

use crate::api::commands::promise_to_js;
use crate::api::disposable::DisposableTable;
use crate::api::events::event_function;
use crate::api::js_object::JsObjects;
use crate::api::types::marked;
use crate::api::{accessor_namespace, getter, handle_field, handle_object};
use crate::convert::{field, opt_bool, string_arg, uri_from_js, uri_to_js};

/// O que uma entrada da tabela guarda: a aba ou o grupo do host.
#[derive(Clone)]
enum Target {
    Tab(Tab),
    Group(TabGroup),
}

impl Target {
    fn same(&self, other: &Target) -> bool {
        match (self, other) {
            (Target::Tab(a), Target::Tab(b)) => Rc::ptr_eq(a, b),
            (Target::Group(a), Target::Group(b)) => Rc::ptr_eq(a, b),
            _ => false,
        }
    }
}

/// As abas e os grupos que o JavaScript segura, por `_id`, com o objeto de cada um (para a identidade estável).
pub struct TabObjects {
    js: JsObjects,
    next: Cell<u32>,
    entries: RefCell<Vec<(u32, Target, JsData)>>,
}

impl TabObjects {
    pub fn new(runner: &NodeRunner) -> Rc<Self> {
        Rc::new(Self { js: JsObjects::new(runner), next: Cell::new(0), entries: RefCell::new(Vec::new()) })
    }

    /// O objeto do `target`: o mesmo objeto JavaScript a cada chamada.
    fn reference(&self, class: &str, target: Target) -> JsData {
        let known = self.entries.borrow().iter().find(|(_, candidate, _)| candidate.same(&target)).map(|(_, _, object)| object.clone());
        if let Some(object) = known {
            return object;
        }
        let id = self.next.get() + 1;
        self.next.set(id);
        let instance = handle_object(class, id);
        // O motor copia o objeto que atravessa a fronteira; lido de volta por referência, ele vira o objeto vivo que se guarda.
        let object = match self.js.property(&JsData::Object(vec![("value".to_owned(), instance.clone())]), "value") {
            held @ JsData::Handle(_) => held,
            _ => instance,
        };
        self.entries.borrow_mut().push((id, target, object.clone()));
        object
    }

    fn tab_to_js(&self, tab: &Tab) -> JsData {
        self.reference("Tab", Target::Tab(tab.clone()))
    }

    fn group_to_js(&self, group: &TabGroup) -> JsData {
        self.reference("TabGroup", Target::Group(group.clone()))
    }

    fn target_of(&self, value: &JsData, class: &str) -> Result<Target, String> {
        let id = handle_field(value, class)?;
        self.entries
            .borrow()
            .iter()
            .find(|(candidate, _, _)| *candidate == id)
            .map(|(_, target, _)| target.clone())
            .ok_or_else(|| format!("{class}: objeto desconhecido"))
    }

    fn tab_of(&self, this: &JsData) -> Result<Tab, String> {
        match self.target_of(this, "Tab")? {
            Target::Tab(tab) => Ok(tab),
            Target::Group(_) => Err("Tab: this inválido".to_owned()),
        }
    }

    fn group_of(&self, this: &JsData) -> Result<TabGroup, String> {
        match self.target_of(this, "TabGroup")? {
            Target::Group(group) => Ok(group),
            Target::Tab(_) => Err("TabGroup: this inválido".to_owned()),
        }
    }

    fn tabs_to_js(&self, tabs: &[Tab]) -> JsData {
        JsData::Array(tabs.iter().map(|tab| self.tab_to_js(tab)).collect())
    }

    fn groups_to_js(&self, groups: &[TabGroup]) -> JsData {
        JsData::Array(groups.iter().map(|group| self.group_to_js(group)).collect())
    }

    /// O argumento de `close`: uma aba, um grupo, ou uma lista só de abas ou só de grupos.
    fn close_target_of(&self, value: &JsData) -> Result<CloseTarget, String> {
        let single = |item: &JsData| -> Result<Target, String> {
            if field(item, "_id").is_none() {
                return Err("Illegal argument: close espera Tab, TabGroup ou listas deles".to_owned());
            }
            let id = handle_field(item, "Tab ou TabGroup")?;
            self.entries
                .borrow()
                .iter()
                .find(|(candidate, _, _)| *candidate == id)
                .map(|(_, target, _)| target.clone())
                .ok_or_else(|| "Illegal argument: close recebeu um objeto que não é uma aba nem um grupo do host".to_owned())
        };
        match value {
            JsData::Array(items) => {
                let targets = items.iter().map(single).collect::<Result<Vec<_>, String>>()?;
                let tabs: Vec<Tab> = targets.iter().filter_map(|target| if let Target::Tab(tab) = target { Some(tab.clone()) } else { None }).collect();
                if tabs.len() == targets.len() {
                    return Ok(CloseTarget::Tabs(tabs));
                }
                let groups: Vec<TabGroup> = targets.iter().filter_map(|target| if let Target::Group(group) = target { Some(group.clone()) } else { None }).collect();
                if groups.len() == targets.len() {
                    return Ok(CloseTarget::TabGroups(groups));
                }
                Err("Illegal argument: close não aceita uma lista que mistura abas e grupos".to_owned())
            }
            item => Ok(match single(item)? {
                Target::Tab(tab) => CloseTarget::Tab(tab),
                Target::Group(group) => CloseTarget::TabGroup(group),
            }),
        }
    }
}

/// O host de abas e a tabela de objetos, que `window` usa.
#[derive(Clone)]
pub struct TabsApi {
    pub tabs: Rc<ExtHostEditorTabs>,
    pub objects: Rc<TabObjects>,
}

// ---- as entradas de aba

fn input_to_js(input: &AnyTabInput) -> JsData {
    let uri = |name: &str, value: &je_base::uri::URI| (name.to_owned(), uri_to_js(value));
    let text = |name: &str, value: &str| (name.to_owned(), JsData::Str(value.to_owned()));
    match input {
        AnyTabInput::Text(input) => marked("TabInputText", vec![uri("uri", &input.uri)]),
        AnyTabInput::TextDiff(input) => text_diff_to_js(&input.original, &input.modified),
        AnyTabInput::TextMultiDiff(input) => marked(
            "TabInputTextMultiDiff",
            vec![("textDiffs".to_owned(), JsData::Array(input.text_diffs.iter().map(|diff| text_diff_to_js(&diff.original, &diff.modified)).collect()))],
        ),
        AnyTabInput::Custom(input) => marked("TabInputCustom", vec![uri("uri", &input.uri), text("viewType", &input.view_type)]),
        AnyTabInput::Notebook(input) => marked("TabInputNotebook", vec![uri("uri", &input.uri), text("notebookType", &input.notebook_type)]),
        AnyTabInput::NotebookDiff(input) => {
            marked("TabInputNotebookDiff", vec![uri("original", &input.original), uri("modified", &input.modified), text("notebookType", &input.notebook_type)])
        }
        AnyTabInput::Webview(input) => marked("TabInputWebview", vec![text("viewType", &input.view_type)]),
        AnyTabInput::Terminal(_) => marked("TabInputTerminal", vec![]),
        AnyTabInput::Interactive(input) => marked("TabInputInteractiveWindow", vec![uri("uri", &input.uri), uri("inputBoxUri", &input.input_box_uri)]),
        AnyTabInput::Chat(_) => marked("TabInputChat", vec![]),
        AnyTabInput::TextMerge(input) => {
            marked("TabInputTextMerge", vec![uri("base", &input.base), uri("input1", &input.input1), uri("input2", &input.input2), uri("result", &input.result)])
        }
    }
}

fn text_diff_to_js(original: &je_base::uri::URI, modified: &je_base::uri::URI) -> JsData {
    marked("TabInputTextDiff", vec![("original".to_owned(), uri_to_js(original)), ("modified".to_owned(), uri_to_js(modified))])
}

#[derive(Clone, Copy)]
enum Kind {
    Uri,
    Text,
    Raw,
}

/// Cada classe de entrada de aba com os campos do construtor, na ordem dos argumentos do original.
const INPUT_CLASSES: &[(&str, &[(&str, Kind)])] = &[
    ("TabInputText", &[("uri", Kind::Uri)]),
    ("TabInputTextDiff", &[("original", Kind::Uri), ("modified", Kind::Uri)]),
    ("TabInputTextMerge", &[("base", Kind::Uri), ("input1", Kind::Uri), ("input2", Kind::Uri), ("result", Kind::Uri)]),
    ("TabInputCustom", &[("uri", Kind::Uri), ("viewType", Kind::Text)]),
    ("TabInputNotebook", &[("uri", Kind::Uri), ("notebookType", Kind::Text)]),
    ("TabInputNotebookDiff", &[("original", Kind::Uri), ("modified", Kind::Uri), ("notebookType", Kind::Text)]),
    ("TabInputWebview", &[("viewType", Kind::Text)]),
    ("TabInputTerminal", &[]),
    ("TabInputInteractiveWindow", &[("uri", Kind::Uri), ("inputBoxUri", Kind::Uri)]),
    ("TabInputChat", &[]),
    ("TabInputTextMultiDiff", &[("textDiffs", Kind::Raw)]),
];

fn input_class(runner: &NodeRunner, name: &'static str, fields: &'static [(&'static str, Kind)]) -> JsData {
    let constructor = runner.define_class(ClassSpec {
        name: name.to_owned(),
        length: fields.len() as u32,
        construct: Rc::new(move |args| {
            let mut values: Vec<(String, JsData)> = Vec::new();
            for (index, (field_name, kind)) in fields.iter().enumerate() {
                let value = match kind {
                    Kind::Uri => uri_to_js(&uri_from_js(args.get(index).ok_or_else(|| format!("{name}: falta o argumento {field_name}"))?)?),
                    Kind::Text => JsData::Str(string_arg(args, index, field_name)?),
                    Kind::Raw => args.get(index).cloned().unwrap_or(JsData::Array(Vec::new())),
                };
                values.push(((*field_name).to_owned(), value));
            }
            Ok(JsData::Object(values))
        }),
        methods: vec![],
        getters: vec![],
        statics: vec![],
        parent: None,
    });
    JsData::Handle(Rc::new(constructor))
}

// ---- as classes

fn define_tab(runner: &NodeRunner, objects: &Rc<TabObjects>) {
    let getters: Vec<(String, MethodCallback)> = vec![
        getter(objects, "label", TabObjects::tab_of, |_, tab| JsData::Str(tab.label())),
        getter(objects, "group", TabObjects::tab_of, |objects, tab| match tab.try_group() {
            Some(group) => objects.group_to_js(&group),
            None => JsData::Undefined,
        }),
        getter(objects, "input", TabObjects::tab_of, |_, tab| tab.input().map_or(JsData::Undefined, |input| input_to_js(&input))),
        getter(objects, "isActive", TabObjects::tab_of, |_, tab| JsData::Bool(tab.is_active())),
        getter(objects, "isDirty", TabObjects::tab_of, |_, tab| JsData::Bool(tab.is_dirty())),
        getter(objects, "isPinned", TabObjects::tab_of, |_, tab| JsData::Bool(tab.is_pinned())),
        getter(objects, "isPreview", TabObjects::tab_of, |_, tab| JsData::Bool(tab.is_preview())),
    ];
    runner.define_accessor_class(
        ClassSpec {
            name: "Tab".to_owned(),
            length: 0,
            construct: Rc::new(|_| Err("Tab não tem construtor público: use window.tabGroups".to_owned())),
            methods: vec![],
            getters,
            statics: vec![],
            parent: None,
        },
        vec![],
    );
}

fn define_tab_group(runner: &NodeRunner, objects: &Rc<TabObjects>) {
    let getters: Vec<(String, MethodCallback)> = vec![
        getter(objects, "isActive", TabObjects::group_of, |_, group| JsData::Bool(group.is_active())),
        getter(objects, "viewColumn", TabObjects::group_of, |_, group| JsData::Num(group.view_column())),
        getter(objects, "activeTab", TabObjects::group_of, |objects, group| group.active_tab().map_or(JsData::Undefined, |tab| objects.tab_to_js(&tab))),
        getter(objects, "tabs", TabObjects::group_of, |objects, group| objects.tabs_to_js(&group.tabs())),
    ];
    runner.define_accessor_class(
        ClassSpec {
            name: "TabGroup".to_owned(),
            length: 0,
            construct: Rc::new(|_| Err("TabGroup não tem construtor público: use window.tabGroups".to_owned())),
            methods: vec![],
            getters,
            statics: vec![],
            parent: None,
        },
        vec![],
    );
}

/// Define os protótipos de `Tab` e `TabGroup` e devolve as classes `TabInput*` (membros de `vscode`).
pub fn classes(runner: &NodeRunner, objects: &Rc<TabObjects>) -> Vec<(String, JsData)> {
    define_tab(runner, objects);
    define_tab_group(runner, objects);
    INPUT_CLASSES.iter().map(|(name, fields)| ((*name).to_owned(), input_class(runner, name, fields))).collect()
}

// ---- `window.tabGroups`

fn tab_change_to_js(objects: &TabObjects, event: &TabChangeEvent) -> JsData {
    JsData::Object(vec![
        ("opened".to_owned(), objects.tabs_to_js(&event.opened)),
        ("closed".to_owned(), objects.tabs_to_js(&event.closed)),
        ("changed".to_owned(), objects.tabs_to_js(&event.changed)),
    ])
}

fn group_change_to_js(objects: &TabObjects, event: &TabGroupChangeEvent) -> JsData {
    JsData::Object(vec![
        ("opened".to_owned(), objects.groups_to_js(&event.opened)),
        ("closed".to_owned(), objects.groups_to_js(&event.closed)),
        ("changed".to_owned(), objects.groups_to_js(&event.changed)),
    ])
}

/// O membro `tabGroups` de `vscode.window`.
pub fn window_member(runner: &NodeRunner, api: &TabsApi, table: &Rc<DisposableTable>) -> (String, JsData) {
    let groups: Rc<TabGroups> = api.tabs.tab_groups();
    let keep_alive = api.tabs.clone();
    let objects = &api.objects;

    let (all_groups, all_objects) = (groups.clone(), objects.clone());
    let (active_groups, active_objects) = (groups.clone(), objects.clone());
    let all_keep = keep_alive.clone();
    let active_keep = keep_alive.clone();

    let (tabs_objects, group_objects) = (objects.clone(), objects.clone());
    let on_did_change_tabs = event_function(runner, table, "onDidChangeTabs", groups.on_did_change_tabs(), move |event| tab_change_to_js(&tabs_objects, event));
    let on_did_change_tab_groups =
        event_function(runner, table, "onDidChangeTabGroups", groups.on_did_change_tab_groups(), move |event| group_change_to_js(&group_objects, event));

    let getters: Vec<(String, MethodCallback)> = vec![
        (
            "all".to_owned(),
            Rc::new(move |_, _| {
                let _ = &all_keep;
                Ok(all_objects.groups_to_js(&all_groups.all()))
            }),
        ),
        (
            "activeTabGroup".to_owned(),
            Rc::new(move |_, _| {
                let _ = &active_keep;
                // o erro do `assertReturnsDefined` do original
                let group = active_groups
                    .try_active_tab_group()
                    .ok_or_else(|| format!("Assertion Failed: {}", je_base::types::UNDEFINED_ARGUMENT_MESSAGE))?;
                Ok(active_objects.group_to_js(&group))
            }),
        ),
        ("onDidChangeTabs".to_owned(), Rc::new(move |_, _| Ok(on_did_change_tabs.clone()))),
        ("onDidChangeTabGroups".to_owned(), Rc::new(move |_, _| Ok(on_did_change_tab_groups.clone()))),
    ];

    let (close_runner, close_groups, close_objects, close_keep) = (runner.clone(), groups, objects.clone(), keep_alive);
    let methods: Vec<(String, u32, MethodCallback)> = vec![(
        "close".to_owned(),
        2,
        Rc::new(move |_, args| {
            let _ = &close_keep;
            let target = close_objects.close_target_of(args.first().unwrap_or(&JsData::Undefined))?;
            let closed = close_groups.close(target, opt_bool(args.get(1)));
            Ok(promise_to_js(&close_runner, closed, |closed| JsData::Bool(*closed)))
        }),
    )];
    ("tabGroups".to_owned(), accessor_namespace(runner, "tabGroups", methods, getters))
}
