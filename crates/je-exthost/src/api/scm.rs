//! `vscode.scm.createSourceControl` e as classes `vscode.SourceControl`, `vscode.SourceControlResourceGroup` e
//! `vscode.SourceControlInputBox` que ela entrega.
//!
//! É a ponte para o `ExtHostSCM` portado (`extHostSCM.ts`): `createSourceControl(id, label, rootUri?)` cria o `ExtHostSourceControl`,
//! que fala com o cliente `MainThreadSCM` do workbench (`$registerSourceControl`, `$updateSourceControl`, `$registerGroups`,
//! `$spliceResourceStates`...). O objeto que a extensão segura carrega só o `_id` (o `handle` do original); as propriedades são
//! acessores do protótipo sobre o serviço vivo. O `SourceControl` tem `id`, `label`, `rootUri`, `inputBox`, `count`,
//! `commitTemplate`, `statusBarCommands`, `acceptInputCommand`, `quickDiffProvider`, `createResourceGroup` e `dispose`; o grupo tem
//! `id`, `label`, `hideWhenEmpty`, `contextValue`, `resourceStates` e `dispose`; a caixa de entrada tem `value`, `placeholder`,
//! `enabled` e `visible`.
//!
//! Falta: as propriedades que o original só libera com uma proposta de API (`SourceControl#actionButton`, `historyProvider`,
//! `artifactProvider`, `secondaryQuickDiffProvider`, `activeRepositoryName`, `contextValue`, `SourceControlInputBox#validateInput`,
//! `showValidationMessage`, `document`, `createResourceGroup(..., options)`), `SourceControl#selected` e `onDidChangeSelection`,
//! `vscode.scm.inputBox` (obsoleto), o `parent` e o `iconPath`/`isHidden` do `createSourceControl` (propostas), e os itens de
//! `SourceControlResourceState` das propostas `scmMultiDiffEditor`.
//!
//! Divergências:
//! - O `vscode` é um só para todas as extensões: o `ExtHostSCM` registra a fonte com a extensão `je.exthost` (a proposta de API nunca
//!   está habilitada, como em `languages` e `tree_views`).
//! - O `quickDiffProvider` chega como cópia (a atribuição a uma propriedade não é por referência): vale um objeto com a função
//!   `provideOriginalResource` como propriedade PRÓPRIA (um objeto literal ou o atribuído a campos); os métodos de protótipo de uma
//!   classe e o `this` do provedor não chegam. A função roda sem `this`.
//! - Os `resourceStates` que `resourceStates` devolve são os objetos que a extensão atribuiu (com as propriedades extras dela, num array
//!   novo a cada leitura), e a ordem é a atribuída (a ordenação por caminho é só a da lista que vai ao workbench).
//! - Um comando recebe a `SourceControl` e o `SourceControlResourceGroup` como a mesma instância que a extensão criou (`===`,
//!   [`register_arguments`]); o `SourceControlResourceState` é o MESMO objeto que a extensão atribuiu a `resourceStates` (o host observa
//!   a atribuição com [`JsObjects::observe`] e guarda cada item por referência).

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use je_base::cancellation::CancellationToken;
use je_base::errors::Error;
use je_base::lifecycle::IDisposable;
use je_base::themables::ThemeIcon;
use je_base::uri::URI;
use je_workbench::api::common::ext_host_scm::{
    DecorationIconPath, ExtHostSCM, ExtHostSCMInputBox, ExtHostSourceControl, ExtHostSourceControlResourceGroup, ProviderResult, QuickDiffProvider, SourceControlResourceDecorations,
    SourceControlResourceState, SourceControlResourceThemableDecorations,
};
use je_workbench::api::common::ext_host_types::Command;
use je_workbench::api::common::ext_host_values::ApiValue;
use zjsc::api::embed_node::{ClassSpec, JsData, JsHandle, MethodCallback, NodeRunner};

use crate::api::commands::{call_settled, convert_maybe, HostArguments};
use crate::api::events::EventTables;
use crate::api::instances::{hold_by_reference, Instances};
use crate::api::js_object::{JsObjects, Observed};
use crate::api::languages::host_extension;
use crate::api::{getter, handle_field, handle_object, setter};
use crate::convert::{class_name, dynamic_to_js, field, is_nullish, js_to_dynamic, opt_bool, opt_number, opt_string, string_arg, text_or_undefined, uri_from_js, uri_to_js};

/// O que o namespace `scm` guarda dos objetos que a extensão segura (os objetos JavaScript carregam só o `_id`, o `handle` do host).
#[derive(Default)]
pub struct ScmObjects {
    controls: RefCell<HashMap<u32, Rc<ExtHostSourceControl>>>,
    groups: RefCell<HashMap<u32, Rc<ExtHostSourceControlResourceGroup>>>,
    /// O dono (o `handle` da fonte) de cada grupo, para soltar os grupos com a fonte.
    group_owners: RefCell<HashMap<u32, u32>>,
    /// O `quickDiffProvider` que a extensão atribuiu, por fonte (a leitura devolve o que ela passou).
    providers: RefCell<HashMap<u32, JsData>>,
    /// Os `resourceStates` que a extensão atribuiu, por grupo: o estado do host e o objeto que ela passou.
    states: RefCell<HashMap<u32, Vec<(Rc<SourceControlResourceState>, JsData)>>>,
    /// As instâncias de `SourceControl` e `SourceControlResourceGroup` que a extensão segura (a mesma em toda leitura).
    instances: Instances,
}

impl ScmObjects {
    /// O `SourceControl` que a extensão segura (o `value` do original), criado pela extensão ou entregue a um comando.
    fn control_value(&self, runner: &NodeRunner, control: &Rc<ExtHostSourceControl>) -> JsData {
        self.controls.borrow_mut().entry(control.handle()).or_insert_with(|| control.clone());
        self.instances.get(runner, "SourceControl", control.handle())
    }

    fn group_value(&self, runner: &NodeRunner, group: &Rc<ExtHostSourceControlResourceGroup>) -> JsData {
        self.groups.borrow_mut().entry(group.handle()).or_insert_with(|| group.clone());
        self.instances.get(runner, "SourceControlResourceGroup", group.handle())
    }

    /// O `SourceControlResourceState` que a extensão atribuiu a um grupo, se o host o conhece; senão o estado do host.
    fn state_value(&self, state: &Rc<SourceControlResourceState>) -> JsData {
        let assigned = self.states.borrow().values().flatten().find(|(candidate, _)| Rc::ptr_eq(candidate, state)).map(|(_, value)| value.clone());
        assigned.unwrap_or_else(|| dynamic_to_js(&state.to_dynamic()))
    }

    fn control_of(&self, this: &JsData) -> Result<Rc<ExtHostSourceControl>, String> {
        self.controls.borrow().get(&handle_field(this, "SourceControl")?).cloned().ok_or_else(|| "SourceControl: a fonte já foi descartada".to_owned())
    }

    fn input_box_of(&self, this: &JsData) -> Result<Rc<ExtHostSCMInputBox>, String> {
        Ok(self.control_of(this)?.input_box())
    }

    fn group_of(&self, this: &JsData) -> Result<Rc<ExtHostSourceControlResourceGroup>, String> {
        self.groups.borrow().get(&handle_field(this, "SourceControlResourceGroup")?).cloned().ok_or_else(|| "SourceControlResourceGroup: o grupo já foi descartado".to_owned())
    }

    fn forget_group(&self, handle: u32) {
        self.groups.borrow_mut().remove(&handle);
        self.group_owners.borrow_mut().remove(&handle);
        self.states.borrow_mut().remove(&handle);
        self.instances.forget("SourceControlResourceGroup", handle);
    }

    fn forget_control(&self, handle: u32) {
        self.controls.borrow_mut().remove(&handle);
        self.providers.borrow_mut().remove(&handle);
        self.instances.forget("SourceControl", handle);
        let owned: Vec<u32> = self.group_owners.borrow().iter().filter(|(_, owner)| **owner == handle).map(|(group, _)| *group).collect();
        for group in owned {
            self.forget_group(group);
        }
    }
}

/// Registra o resolvedor dos argumentos de comando que o `ArgumentProcessor` de `ExtHostSCM` produz: a fonte, o grupo e o estado de
/// recurso chegam ao comando da extensão como os objetos que ela segura.
pub fn register_arguments(runner: &NodeRunner, objects: &Rc<ScmObjects>, arguments: &HostArguments) {
    let (runner, objects) = (runner.clone(), objects.clone());
    arguments.register(move |value| {
        if let Some(control) = ExtHostSourceControl::from_dynamic(value) {
            return Some(objects.control_value(&runner, &control));
        }
        if let Some(group) = ExtHostSourceControlResourceGroup::from_dynamic(value) {
            return Some(objects.group_value(&runner, &group));
        }
        SourceControlResourceState::from_dynamic(value).map(|state| objects.state_value(&state))
    });
}

// ---- a conversão dos estados de recurso

fn command_of(value: &JsData) -> Result<Command, String> {
    Command::from_dynamic(&js_to_dynamic(value)).map_err(|error| error.message)
}

/// `iconPath` de uma decoração: `string | Uri | ThemeIcon`.
fn decoration_icon_of(value: Option<&JsData>) -> Result<Option<DecorationIconPath>, String> {
    match value {
        None | Some(JsData::Undefined) | Some(JsData::Null) => Ok(None),
        Some(JsData::Str(path)) => Ok(Some(DecorationIconPath::Path(path.clone()))),
        Some(icon @ JsData::Object(_)) => match class_name(icon) {
            Some("ThemeIcon") => Ok(Some(DecorationIconPath::ThemeIcon(ThemeIcon::from_dynamic(&js_to_dynamic(icon)).map_err(|error| error.message)?))),
            _ => Ok(Some(DecorationIconPath::Uri(uri_from_js(icon)?))),
        },
        Some(_) => Err("decorations.iconPath precisa ser texto, Uri ou ThemeIcon".to_owned()),
    }
}

fn themable_of(value: Option<&JsData>) -> Result<Option<SourceControlResourceThemableDecorations>, String> {
    match value {
        Some(themable @ JsData::Object(_)) => Ok(Some(SourceControlResourceThemableDecorations { icon_path: decoration_icon_of(field(themable, "iconPath"))? })),
        _ => Ok(None),
    }
}

fn decorations_of(value: &JsData) -> Result<SourceControlResourceDecorations, String> {
    Ok(SourceControlResourceDecorations {
        icon_path: decoration_icon_of(field(value, "iconPath"))?,
        light: themable_of(field(value, "light"))?,
        dark: themable_of(field(value, "dark"))?,
        strike_through: opt_bool(field(value, "strikeThrough")),
        faded: opt_bool(field(value, "faded")),
        tooltip: opt_string(field(value, "tooltip")),
    })
}

/// `vscode.SourceControlResourceState`.
fn resource_state_of(value: &JsData) -> Result<Rc<SourceControlResourceState>, String> {
    let resource_uri = uri_from_js(field(value, "resourceUri").ok_or("SourceControlResourceState: falta o resourceUri")?)?;
    let command = match field(value, "command") {
        Some(command @ JsData::Object(_)) => Some(command_of(command)?),
        _ => None,
    };
    let decorations = match field(value, "decorations") {
        Some(decorations @ JsData::Object(_)) => Some(decorations_of(decorations)?),
        _ => None,
    };
    Ok(Rc::new(SourceControlResourceState {
        resource_uri,
        command,
        decorations,
        context_value: opt_string(field(value, "contextValue")),
        multi_diff_editor_original_uri: None,
        multi_file_diff_editor_modified_uri: None,
    }))
}

// ---- o provedor de quick diff da extensão

/// O `vscode.QuickDiffProvider` da extensão: a função `provideOriginalResource` (se tiver) como o provedor do `ExtHostSourceControl`.
struct JsQuickDiffProvider {
    runner: NodeRunner,
    events: Rc<EventTables>,
    provide_original_resource: Option<Rc<JsHandle>>,
}

/// `Uri | undefined | null` que o provedor devolveu.
fn original_uri_of(value: &JsData) -> Result<Option<URI>, Error> {
    if is_nullish(Some(value)) {
        return Ok(None);
    }
    uri_from_js(value).map(Some).map_err(Error::new)
}

impl QuickDiffProvider for JsQuickDiffProvider {
    fn provide_original_resource(&self, uri: &URI, token: &CancellationToken) -> Option<ProviderResult<URI>> {
        let function = self.provide_original_resource.as_ref()?;
        let result = call_settled(&self.runner, function, &[uri_to_js(uri), self.events.token_to_js(token.clone())]);
        Some(convert_maybe(result, |value| original_uri_of(&dynamic_to_js(&value))))
    }
}

// ---- as classes

fn command_to_js(command: &Command) -> JsData {
    dynamic_to_js(&command.to_dynamic())
}

fn define_source_control(runner: &NodeRunner, objects: &Rc<ScmObjects>, events: &Rc<EventTables>) {
    let get = |name: &str, read: fn(&ScmObjects, &Rc<ExtHostSourceControl>) -> JsData| getter(objects, name, ScmObjects::control_of, read);
    let getters: Vec<(String, MethodCallback)> = vec![
        get("id", |_, control| JsData::Str(control.id().to_owned())),
        get("label", |_, control| JsData::Str(control.label().to_owned())),
        get("rootUri", |_, control| control.root_uri().map_or(JsData::Undefined, uri_to_js)),
        get("inputBox", |_, control| handle_object("SourceControlInputBox", control.handle())),
        get("count", |_, control| control.count().map_or(JsData::Undefined, JsData::Num)),
        get("commitTemplate", |_, control| text_or_undefined(control.commit_template())),
        get("acceptInputCommand", |_, control| control.accept_input_command().as_ref().map_or(JsData::Undefined, command_to_js)),
        get("statusBarCommands", |_, control| {
            control.status_bar_commands().map_or(JsData::Undefined, |commands| JsData::Array(commands.iter().map(command_to_js).collect()))
        }),
        get("quickDiffProvider", |objects, control| objects.providers.borrow().get(&control.handle()).cloned().unwrap_or(JsData::Undefined)),
    ];

    let set = |name: &str, write: fn(&ScmObjects, &Rc<ExtHostSourceControl>, &JsData) -> Result<(), String>| setter(objects, name, ScmObjects::control_of, write);
    let quick_diff_runner = runner.clone();
    let quick_diff_events = events.clone();
    let setters: Vec<(String, MethodCallback)> = vec![
        set("count", |_, control, value| {
            control.set_count(opt_number(Some(value)));
            Ok(())
        }),
        set("commitTemplate", |_, control, value| {
            control.set_commit_template(opt_string(Some(value)));
            Ok(())
        }),
        set("acceptInputCommand", |_, control, value| {
            let command = if is_nullish(Some(value)) { None } else { Some(command_of(value)?) };
            control.set_accept_input_command(command).map_err(|error| error.message)
        }),
        set("statusBarCommands", |_, control, value| {
            let commands = match value {
                JsData::Array(list) => Some(list.iter().map(command_of).collect::<Result<Vec<_>, String>>()?),
                _ => None,
            };
            control.set_status_bar_commands(commands).map_err(|error| error.message)
        }),
        setter(objects, "quickDiffProvider", ScmObjects::control_of, move |objects, control, value| {
            if is_nullish(Some(value)) {
                objects.providers.borrow_mut().remove(&control.handle());
                control.set_quick_diff_provider(None);
                return Ok(());
            }
            let provide_original_resource = match field(value, "provideOriginalResource") {
                Some(JsData::Handle(function)) => Some(function.clone()),
                _ => None,
            };
            objects.providers.borrow_mut().insert(control.handle(), value.clone());
            control.set_quick_diff_provider(Some(Rc::new(JsQuickDiffProvider {
                runner: quick_diff_runner.clone(),
                events: quick_diff_events.clone(),
                provide_original_resource,
            })));
            Ok(())
        }),
    ];

    let (group_objects, dispose_objects) = (objects.clone(), objects.clone());
    let group_runner = runner.clone();
    let methods: Vec<(String, u32, MethodCallback)> = vec![
        (
            "createResourceGroup".to_owned(),
            2,
            Rc::new(move |this, args| {
                let control = group_objects.control_of(this)?;
                let id = string_arg(args, 0, "createResourceGroup: o id")?;
                let label = string_arg(args, 1, "createResourceGroup: o label")?;
                let group = control.create_resource_group(&id, &label, None);
                group_objects.group_owners.borrow_mut().insert(group.handle(), control.handle());
                Ok(group_objects.group_value(&group_runner, &group))
            }),
        ),
        (
            "dispose".to_owned(),
            0,
            Rc::new(move |this, _| {
                let control = dispose_objects.control_of(this)?;
                control.dispose();
                dispose_objects.forget_control(control.handle());
                Ok(JsData::Undefined)
            }),
        ),
    ];
    runner.define_accessor_class(
        ClassSpec {
            name: "SourceControl".to_owned(),
            length: 0,
            construct: Rc::new(|_| Err("SourceControl não tem construtor público: use scm.createSourceControl".to_owned())),
            methods,
            getters,
            statics: vec![],
            parent: None,
        },
        setters,
    );
}

fn define_resource_group(runner: &NodeRunner, objects: &Rc<ScmObjects>) {
    let get = |name: &str, read: fn(&ScmObjects, &Rc<ExtHostSourceControlResourceGroup>) -> JsData| getter(objects, name, ScmObjects::group_of, read);
    let getters: Vec<(String, MethodCallback)> = vec![
        get("id", |_, group| JsData::Str(group.id().to_owned())),
        get("label", |_, group| JsData::Str(group.label())),
        get("hideWhenEmpty", |_, group| group.hide_when_empty().map_or(JsData::Undefined, JsData::Bool)),
        get("contextValue", |_, group| text_or_undefined(group.context_value())),
        get("resourceStates", |objects, group| {
            JsData::Array(objects.states.borrow().get(&group.handle()).map(|pairs| pairs.iter().map(|(_, value)| value.clone()).collect()).unwrap_or_default())
        }),
    ];

    let set = |name: &str, write: fn(&ScmObjects, &Rc<ExtHostSourceControlResourceGroup>, &JsData) -> Result<(), String>| setter(objects, name, ScmObjects::group_of, write);
    let setters: Vec<(String, MethodCallback)> = vec![
        set("label", |_, group, value| {
            group.set_label(&opt_string(Some(value)).unwrap_or_default());
            Ok(())
        }),
        set("hideWhenEmpty", |_, group, value| {
            group.set_hide_when_empty(opt_bool(Some(value)));
            Ok(())
        }),
        set("contextValue", |_, group, value| {
            group.set_context_value(opt_string(Some(value)));
            Ok(())
        }),
        set("resourceStates", |objects, group, value| {
            let list = match value {
                JsData::Array(list) => list.clone(),
                JsData::Undefined | JsData::Null => Vec::new(),
                _ => return Err("resourceStates precisa ser uma lista de SourceControlResourceState".to_owned()),
            };
            let states = list.iter().map(resource_state_of).collect::<Result<Vec<_>, String>>()?;
            objects.states.borrow_mut().insert(group.handle(), states.iter().cloned().zip(list).collect());
            group.set_resource_states(states);
            Ok(())
        }),
    ];

    let dispose_objects = objects.clone();
    let methods: Vec<(String, u32, MethodCallback)> = vec![(
        "dispose".to_owned(),
        0,
        Rc::new(move |this, _| {
            let group = dispose_objects.group_of(this)?;
            group.dispose();
            dispose_objects.forget_group(group.handle());
            Ok(JsData::Undefined)
        }),
    )];
    let class = Rc::new(runner.define_accessor_class(
        ClassSpec {
            name: "SourceControlResourceGroup".to_owned(),
            length: 0,
            construct: Rc::new(|_| Err("SourceControlResourceGroup não tem construtor público: use SourceControl#createResourceGroup".to_owned())),
            methods,
            getters,
            statics: vec![],
            parent: None,
        },
        setters,
    ));
    let (hold_js, hold_objects) = (JsObjects::new(runner), objects.clone());
    let observer = hold_js.clone();
    observer.observe(&class, "resourceStates", Observed::Setter, move |this, _, items| {
        if let Some(group) = hold_js.handle_id(this) {
            hold_by_reference(&hold_objects.states, group, items);
        }
    });
}

fn define_input_box(runner: &NodeRunner, objects: &Rc<ScmObjects>) {
    let get = |name: &str, read: fn(&ScmObjects, &Rc<ExtHostSCMInputBox>) -> JsData| getter(objects, name, ScmObjects::input_box_of, read);
    let getters: Vec<(String, MethodCallback)> = vec![
        get("value", |_, input_box| JsData::Str(input_box.value())),
        get("placeholder", |_, input_box| JsData::Str(input_box.placeholder())),
        get("enabled", |_, input_box| JsData::Bool(input_box.enabled())),
        get("visible", |_, input_box| JsData::Bool(input_box.visible())),
    ];

    let set = |name: &str, write: fn(&ScmObjects, &Rc<ExtHostSCMInputBox>, &JsData) -> Result<(), String>| setter(objects, name, ScmObjects::input_box_of, write);
    let setters: Vec<(String, MethodCallback)> = vec![
        set("value", |_, input_box, value| {
            input_box.set_value(&opt_string(Some(value)).unwrap_or_default());
            Ok(())
        }),
        set("placeholder", |_, input_box, value| {
            input_box.set_placeholder(&opt_string(Some(value)).unwrap_or_default());
            Ok(())
        }),
        set("enabled", |_, input_box, value| {
            input_box.set_enabled(opt_bool(Some(value)).unwrap_or(false));
            Ok(())
        }),
        set("visible", |_, input_box, value| {
            input_box.set_visible(opt_bool(Some(value)).unwrap_or(false));
            Ok(())
        }),
    ];
    runner.define_accessor_class(
        ClassSpec {
            name: "SourceControlInputBox".to_owned(),
            length: 0,
            construct: Rc::new(|_| Err("SourceControlInputBox não tem construtor público: use SourceControl#inputBox".to_owned())),
            methods: vec![],
            getters,
            statics: vec![],
            parent: None,
        },
        setters,
    );
}

/// Define as classes `SourceControl`, `SourceControlResourceGroup` e `SourceControlInputBox` (protótipos dos objetos que o host entrega).
pub fn define_classes(runner: &NodeRunner, objects: &Rc<ScmObjects>, events: &Rc<EventTables>) {
    define_source_control(runner, objects, events);
    define_resource_group(runner, objects);
    define_input_box(runner, objects);
}

/// O namespace `scm`: `createSourceControl(id, label, rootUri?)`.
pub fn namespace(runner: &NodeRunner, scm: &Rc<ExtHostSCM>, objects: &Rc<ScmObjects>) -> JsData {
    let (scm, objects, create_runner) = (scm.clone(), objects.clone(), runner.clone());
    let create = runner.make_function(
        "createSourceControl",
        3,
        Rc::new(move |args| {
            let id = string_arg(args, 0, "createSourceControl: o id")?;
            let label = string_arg(args, 1, "createSourceControl: o label")?;
            let root_uri = match args.get(2) {
                Some(uri @ JsData::Object(_)) => Some(uri_from_js(uri)?),
                _ => None,
            };
            let control = scm.create_source_control(&host_extension(), &id, &label, root_uri, None, None, None);
            Ok(objects.control_value(&create_runner, &control))
        }),
    );
    JsData::Object(vec![("createSourceControl".to_owned(), JsData::Handle(Rc::new(create)))])
}
