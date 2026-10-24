//! `vscode.window.createQuickPick` e `vscode.window.createInputBox`: os objetos de sessão do `ExtHostQuickOpen` portado
//! (`extHostQuickOpen.ts`: `ExtHostQuickInput`, `ExtHostQuickPick` e `ExtHostInputBox`) como as classes `vscode.QuickPick` e
//! `vscode.InputBox`.
//!
//! Cada objeto que a extensão segura carrega só o `_id` da sessão (o `id` do `ExtHostQuickInput`); as propriedades são acessores do
//! protótipo (leitura e escrita) sobre a sessão viva, e as funções `show`, `hide` e `dispose` e os eventos (`onDidAccept`,
//! `onDidChangeValue`, `onDidChangeActive`, `onDidChangeSelection`, `onDidTriggerButton`, `onDidTriggerItemButton`, `onDidHide`) são
//! métodos e acessores do mesmo protótipo. O `ExtHostQuickOpen` envia ao `MainThreadQuickOpen` as mudanças acumuladas (`$createOrUpdate`),
//! e as respostas do usuário voltam por `ExtHostQuickOpenShape` (`$onDidAccept`, `$onDidChangeValue`...).
//!
//! Divergências:
//! - Os itens e os botões da extensão atravessam o motor como cópia, então o original `activeItems = [items[0]]` (comparação por
//!   identidade) é feito por igualdade estrutural entre a cópia que a extensão passou e a cópia guardada em `items`: dois itens
//!   estruturalmente iguais são o mesmo item para esta ponte (o primeiro ainda não usado vale). Os itens que `items`, `activeItems`,
//!   `selectedItems` e os eventos devolvem são as cópias que a extensão passou (com as propriedades extras dela), nunca o mesmo objeto.
//! - `QuickInputButtons.Back` também é uma cópia: o botão cujo `iconPath` é o `ThemeIcon` `arrow-left` vale como o botão de voltar
//!   (handle -1), como o `button === QuickInputButtons.Back` do original.
//! - `QuickPickItem#tooltip` aceita só texto (a `MarkdownString` ainda não é classe do `vscode`), como em `showQuickPick`.
//! - `QuickPick#keepScrollPosition` e `InputBox#validationMessage` seguem a forma do `ExtHostQuickOpen`; `QuickInputButton#location` e
//!   `toggle` são lidos como números e `{ checked }`, e o `checked` que o usuário muda volta para o botão que a extensão guarda no
//!   host (a cópia da extensão não é atualizada).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use je_base::equals::structural_equals;
use je_base::event::Event;
use je_base::lifecycle::IDisposable;
use je_platform::quickinput::quick_input::{QuickInputButtonLocation, QuickInputButtonToggle};
use je_workbench::api::common::ext_host_quick_open::{
    ExtHostInputBox, ExtHostQuickInput, ExtHostQuickOpen, ExtHostQuickPick, InputBoxValidation, QuickInputButton, QuickPickEntry, QuickPickItem,
};
use je_workbench::api::common::ext_host_type_converters::icon_path::IconPathInput;
use zjsc::api::embed_node::{ClassSpec, JsData, MethodCallback, NodeRunner, CLASS_KEY};

use crate::api::disposable::DisposableTable;
use crate::api::events::event_function;
use crate::api::quick_input::{icon_path_of, quick_pick_entry_of, validation_of};
use crate::convert::{field, js_to_dynamic, opt_bool, opt_number, opt_string};

/// Os botões da sessão: o do host e a cópia que a extensão passou.
type Buttons = RefCell<Vec<(Rc<QuickInputButton>, JsData)>>;

/// Uma sessão de `createQuickPick`: o `ExtHostQuickPick`, os itens (o do host e a cópia da extensão) e os botões.
struct PickSession {
    pick: Rc<ExtHostQuickPick>,
    items: RefCell<Vec<(Rc<QuickPickItem>, JsData)>>,
    buttons: Buttons,
}

/// Uma sessão de `createInputBox`.
struct InputSession {
    input_box: Rc<ExtHostInputBox>,
    buttons: Buttons,
}

/// Uma sessão viva.
#[derive(Clone)]
enum Session {
    Pick(Rc<PickSession>),
    Input(Rc<InputSession>),
}

impl Session {
    /// O núcleo comum (`title`, `value`, `show`, `hide`, `dispose`...).
    fn core(&self) -> &Rc<ExtHostQuickInput> {
        match self {
            Session::Pick(pick) => pick.pick.input(),
            Session::Input(input) => input.input_box.input(),
        }
    }

    fn buttons(&self) -> &Buttons {
        match self {
            Session::Pick(pick) => &pick.buttons,
            Session::Input(input) => &input.buttons,
        }
    }

    fn pick(&self) -> Result<&Rc<PickSession>, String> {
        match self {
            Session::Pick(pick) => Ok(pick),
            Session::Input(_) => Err("a propriedade existe só em QuickPick".to_owned()),
        }
    }

    fn input_box(&self) -> Result<&Rc<ExtHostInputBox>, String> {
        match self {
            Session::Input(input) => Ok(&input.input_box),
            Session::Pick(_) => Err("a propriedade existe só em InputBox".to_owned()),
        }
    }
}

/// As sessões vivas, por id (o objeto JavaScript carrega só o `_id`).
#[derive(Default)]
pub struct QuickSessions {
    items: RefCell<HashMap<u32, Session>>,
}

impl QuickSessions {
    fn add(&self, session: Session, class: &str) -> JsData {
        let id = session.core().id();
        self.items.borrow_mut().insert(id, session);
        JsData::Object(vec![(CLASS_KEY.to_owned(), JsData::Str(class.to_owned())), ("_id".to_owned(), JsData::Num(f64::from(id)))])
    }

    fn session_of(&self, this: &JsData) -> Result<Session, String> {
        match field(this, "_id") {
            Some(JsData::Num(id)) => self.items.borrow().get(&(*id as u32)).cloned().ok_or_else(|| "QuickInput: a sessão já foi descartada".to_owned()),
            _ => Err("QuickInput: this inválido".to_owned()),
        }
    }
}

// ---- a conversão de itens e botões

/// `QuickInputButton`: `iconPath`, `tooltip`, `location` e `toggle`.
fn button_of(value: &JsData) -> Result<Rc<QuickInputButton>, String> {
    let icon_path = match field(value, "iconPath") {
        Some(icon_path) => icon_path_of(icon_path)?,
        None => None,
    }
    .ok_or("QuickInputButton: falta o iconPath")?;
    let is_back = matches!(&icon_path, IconPathInput::ThemeIcon(icon) if icon.id == "arrow-left");
    let location = match opt_number(field(value, "location")).map(|location| location as i32) {
        Some(1) => Some(QuickInputButtonLocation::Title),
        Some(2) => Some(QuickInputButtonLocation::Inline),
        Some(3) => Some(QuickInputButtonLocation::Input),
        _ => None,
    };
    let toggle = match field(value, "toggle") {
        Some(toggle @ JsData::Object(_)) => Some(QuickInputButtonToggle { checked: Cell::new(opt_bool(field(toggle, "checked")).unwrap_or(false)) }),
        _ => None,
    };
    Ok(Rc::new(QuickInputButton { icon_path, tooltip: opt_string(field(value, "tooltip")), location, toggle, is_back }))
}

/// A lista de botões da extensão (um array de `QuickInputButton`).
fn buttons_of(value: &JsData) -> Result<Vec<(Rc<QuickInputButton>, JsData)>, String> {
    match value {
        JsData::Array(list) => list.iter().map(|button| Ok((button_of(button)?, button.clone()))).collect(),
        JsData::Undefined | JsData::Null => Ok(Vec::new()),
        _ => Err("buttons precisa ser uma lista de QuickInputButton".to_owned()),
    }
}

/// `QuickPickItem` de `createQuickPick`: o item de `showQuickPick` mais os `buttons` do item.
fn session_item_of(value: &JsData) -> Result<Rc<QuickPickItem>, String> {
    let QuickPickEntry::Item(item) = quick_pick_entry_of(value)? else {
        return Err("createQuickPick: cada item precisa ser um objeto com label".to_owned());
    };
    let mut item = Rc::try_unwrap(item).map_err(|_| "createQuickPick: item compartilhado".to_owned())?;
    if let Some(JsData::Array(buttons)) = field(value, "buttons") {
        item.buttons = Some(buttons.iter().map(button_of).collect::<Result<Vec<_>, String>>()?);
    }
    Ok(Rc::new(item))
}

fn flag(value: &JsData) -> bool {
    matches!(value, JsData::Bool(true))
}

fn optional_text(value: &JsData) -> Option<String> {
    opt_string(Some(value))
}

fn optional_count(value: &JsData) -> Option<u32> {
    opt_number(Some(value)).map(|count| count as u32)
}

fn text_or_undefined(value: Option<String>) -> JsData {
    value.map_or(JsData::Undefined, JsData::Str)
}

fn count_or_undefined(value: Option<u32>) -> JsData {
    value.map_or(JsData::Undefined, |count| JsData::Num(f64::from(count)))
}

impl PickSession {
    /// A cópia da extensão de um item do host (`None` se o item saiu da lista).
    fn copy_of_item(&self, item: &Rc<QuickPickItem>) -> Option<JsData> {
        self.items.borrow().iter().find(|(candidate, _)| Rc::ptr_eq(candidate, item)).map(|(_, copy)| copy.clone())
    }

    fn js_of_item(&self, item: &Rc<QuickPickItem>) -> JsData {
        self.copy_of_item(item).unwrap_or(JsData::Undefined)
    }

    /// As cópias da extensão dos itens do host.
    fn js_of_items(&self, items: &[Rc<QuickPickItem>]) -> JsData {
        JsData::Array(items.iter().filter_map(|item| self.copy_of_item(item)).collect())
    }

    /// Os itens do host que as cópias da extensão representam (ver as divergências do módulo).
    fn items_of(&self, value: &JsData) -> Result<Vec<Rc<QuickPickItem>>, String> {
        let JsData::Array(list) = value else { return Err("a lista de itens precisa ser um array".to_owned()) };
        let known = self.items.borrow();
        let mut used = vec![false; known.len()];
        let mut found = Vec::new();
        for copy in list {
            let wanted = js_to_dynamic(copy);
            let position = known.iter().enumerate().position(|(index, (_, candidate))| !used[index] && structural_equals(&js_to_dynamic(candidate), &wanted));
            if let Some(index) = position {
                used[index] = true;
                found.push(known[index].0.clone());
            }
        }
        Ok(found)
    }

    /// O botão de um item como a extensão o passou (o `buttons[i]` do item).
    fn js_of_item_button(&self, item: &Rc<QuickPickItem>, button: &Rc<QuickInputButton>) -> JsData {
        let index = item.buttons.as_ref().and_then(|buttons| buttons.iter().position(|candidate| Rc::ptr_eq(candidate, button)));
        let copy = self.copy_of_item(item);
        match (index, copy.as_ref().and_then(|copy| field(copy, "buttons"))) {
            (Some(index), Some(JsData::Array(buttons))) => buttons.get(index).cloned().unwrap_or(JsData::Undefined),
            _ => JsData::Undefined,
        }
    }
}

// ---- a montagem das classes

/// Os acessores e métodos de uma classe de sessão, montados por um [`Members::read`], [`Members::write`], [`Members::event`].
struct Members {
    runner: NodeRunner,
    table: Rc<DisposableTable>,
    sessions: Rc<QuickSessions>,
    getters: Vec<(String, MethodCallback)>,
    setters: Vec<(String, MethodCallback)>,
    methods: Vec<(String, u32, MethodCallback)>,
}

impl Members {
    fn new(runner: &NodeRunner, table: &Rc<DisposableTable>, sessions: &Rc<QuickSessions>) -> Self {
        Self { runner: runner.clone(), table: table.clone(), sessions: sessions.clone(), getters: Vec::new(), setters: Vec::new(), methods: Vec::new() }
    }

    fn read(&mut self, name: &str, read: impl Fn(&Session) -> Result<JsData, String> + 'static) {
        let sessions = self.sessions.clone();
        self.getters.push((name.to_owned(), Rc::new(move |this, _| read(&sessions.session_of(this)?))));
    }

    fn write(&mut self, name: &str, write: impl Fn(&Session, &JsData) -> Result<(), String> + 'static) {
        let sessions = self.sessions.clone();
        self.setters.push((
            name.to_owned(),
            Rc::new(move |this, args| {
                write(&sessions.session_of(this)?, args.first().unwrap_or(&JsData::Undefined))?;
                Ok(JsData::Undefined)
            }),
        ));
    }

    fn event<T: 'static>(
        &mut self,
        name: &'static str,
        source: impl Fn(&Session) -> Result<Event<T>, String> + 'static,
        to_js: impl Fn(&Session, &T) -> JsData + 'static,
    ) {
        let (sessions, runner, table, to_js) = (self.sessions.clone(), self.runner.clone(), self.table.clone(), Rc::new(to_js));
        self.getters.push((
            name.to_owned(),
            Rc::new(move |this, _| {
                let session = sessions.session_of(this)?;
                let event = source(&session)?;
                let (to_js, payload_session) = (to_js.clone(), session);
                Ok(event_function(&runner, &table, name, event, move |value| to_js(&payload_session, value)))
            }),
        ));
    }

    fn method(&mut self, name: &str, body: impl Fn(&Session) -> Result<(), String> + 'static) {
        let sessions = self.sessions.clone();
        self.methods.push((
            name.to_owned(),
            0,
            Rc::new(move |this, _| {
                body(&sessions.session_of(this)?)?;
                Ok(JsData::Undefined)
            }),
        ));
    }

    /// Os membros do `ExtHostQuickInput` que `QuickPick` e `InputBox` compartilham.
    fn common(&mut self) {
        self.read("title", |session| Ok(text_or_undefined(session.core().title())));
        self.write("title", |session, value| {
            session.core().set_title(optional_text(value));
            Ok(())
        });
        self.read("step", |session| Ok(count_or_undefined(session.core().step())));
        self.write("step", |session, value| {
            session.core().set_step(optional_count(value));
            Ok(())
        });
        self.read("totalSteps", |session| Ok(count_or_undefined(session.core().total_steps())));
        self.write("totalSteps", |session, value| {
            session.core().set_total_steps(optional_count(value));
            Ok(())
        });
        self.read("enabled", |session| Ok(JsData::Bool(session.core().enabled())));
        self.write("enabled", |session, value| {
            session.core().set_enabled(flag(value));
            Ok(())
        });
        self.read("busy", |session| Ok(JsData::Bool(session.core().busy())));
        self.write("busy", |session, value| {
            session.core().set_busy(flag(value));
            Ok(())
        });
        self.read("ignoreFocusOut", |session| Ok(JsData::Bool(session.core().ignore_focus_out())));
        self.write("ignoreFocusOut", |session, value| {
            session.core().set_ignore_focus_out(flag(value));
            Ok(())
        });
        self.read("value", |session| Ok(JsData::Str(session.core().value())));
        self.write("value", |session, value| {
            session.core().set_value(&optional_text(value).unwrap_or_default());
            Ok(())
        });
        self.read("placeholder", |session| Ok(text_or_undefined(session.core().placeholder())));
        self.write("placeholder", |session, value| {
            session.core().set_placeholder(optional_text(value));
            Ok(())
        });
        self.read("buttons", |session| Ok(JsData::Array(session.buttons().borrow().iter().map(|(_, copy)| copy.clone()).collect())));
        self.write("buttons", |session, value| {
            let buttons = buttons_of(value)?;
            session.core().set_buttons(buttons.iter().map(|(button, _)| button.clone()).collect());
            *session.buttons().borrow_mut() = buttons;
            Ok(())
        });

        self.event("onDidAccept", |session| Ok(session.core().on_did_accept()), |_, ()| JsData::Undefined);
        self.event("onDidChangeValue", |session| Ok(session.core().on_did_change_value()), |_, value: &String| JsData::Str(value.clone()));
        self.event("onDidHide", |session| Ok(session.core().on_did_hide()), |_, ()| JsData::Undefined);
        self.event(
            "onDidTriggerButton",
            |session| Ok(session.core().on_did_trigger_button()),
            |session, button: &Rc<QuickInputButton>| {
                session.buttons().borrow().iter().find(|(candidate, _)| Rc::ptr_eq(candidate, button)).map_or(JsData::Undefined, |(_, copy)| copy.clone())
            },
        );

        self.method("show", |session| {
            session.core().show();
            Ok(())
        });
        self.method("hide", |session| {
            session.core().hide();
            Ok(())
        });
        let sessions = self.sessions.clone();
        self.methods.push((
            "dispose".to_owned(),
            0,
            Rc::new(move |this, _| {
                let session = sessions.session_of(this)?;
                let id = session.core().id();
                session.core().dispose();
                sessions.items.borrow_mut().remove(&id);
                Ok(JsData::Undefined)
            }),
        ));
    }

    /// Os membros que só o `QuickPick` tem.
    fn quick_pick(&mut self) {
        self.read("items", |session| Ok(JsData::Array(session.pick()?.items.borrow().iter().map(|(_, copy)| copy.clone()).collect())));
        self.write("items", |session, value| {
            let pick = session.pick()?;
            let list = match value {
                JsData::Array(list) => list.clone(),
                JsData::Undefined | JsData::Null => Vec::new(),
                _ => return Err("items precisa ser uma lista de QuickPickItem".to_owned()),
            };
            let items = list.iter().map(|copy| Ok((session_item_of(copy)?, copy.clone()))).collect::<Result<Vec<_>, String>>()?;
            pick.pick.set_items(items.iter().map(|(item, _)| item.clone()).collect());
            *pick.items.borrow_mut() = items;
            Ok(())
        });
        self.read("canSelectMany", |session| Ok(JsData::Bool(session.pick()?.pick.can_select_many())));
        self.write("canSelectMany", |session, value| {
            session.pick()?.pick.set_can_select_many(flag(value));
            Ok(())
        });
        self.read("matchOnDescription", |session| Ok(JsData::Bool(session.pick()?.pick.match_on_description())));
        self.write("matchOnDescription", |session, value| {
            session.pick()?.pick.set_match_on_description(flag(value));
            Ok(())
        });
        self.read("matchOnDetail", |session| Ok(JsData::Bool(session.pick()?.pick.match_on_detail())));
        self.write("matchOnDetail", |session, value| {
            session.pick()?.pick.set_match_on_detail(flag(value));
            Ok(())
        });
        self.read("sortByLabel", |session| Ok(JsData::Bool(session.pick()?.pick.sort_by_label())));
        self.write("sortByLabel", |session, value| {
            session.pick()?.pick.set_sort_by_label(flag(value));
            Ok(())
        });
        self.read("keepScrollPosition", |session| Ok(JsData::Bool(session.pick()?.pick.keep_scroll_position())));
        self.write("keepScrollPosition", |session, value| {
            session.pick()?.pick.set_keep_scroll_position(flag(value));
            Ok(())
        });
        self.read("activeItems", |session| {
            let pick = session.pick()?;
            Ok(pick.js_of_items(&pick.pick.active_items()))
        });
        self.write("activeItems", |session, value| {
            let pick = session.pick()?;
            pick.pick.set_active_items(pick.items_of(value)?);
            Ok(())
        });
        self.read("selectedItems", |session| {
            let pick = session.pick()?;
            Ok(pick.js_of_items(&pick.pick.selected_items()))
        });
        self.write("selectedItems", |session, value| {
            let pick = session.pick()?;
            pick.pick.set_selected_items(pick.items_of(value)?);
            Ok(())
        });

        self.event(
            "onDidChangeActive",
            |session| Ok(session.pick()?.pick.on_did_change_active()),
            |session, items: &Vec<Rc<QuickPickItem>>| session.pick().map_or(JsData::Undefined, |pick| pick.js_of_items(items)),
        );
        self.event(
            "onDidChangeSelection",
            |session| Ok(session.pick()?.pick.on_did_change_selection()),
            |session, items: &Vec<Rc<QuickPickItem>>| session.pick().map_or(JsData::Undefined, |pick| pick.js_of_items(items)),
        );
        self.event(
            "onDidTriggerItemButton",
            |session| Ok(session.pick()?.pick.on_did_trigger_item_button()),
            |session, (button, item): &(Rc<QuickInputButton>, Rc<QuickPickItem>)| {
                let Ok(pick) = session.pick() else { return JsData::Undefined };
                JsData::Object(vec![
                    ("button".to_owned(), pick.js_of_item_button(item, button)),
                    ("item".to_owned(), pick.js_of_item(item)),
                ])
            },
        );
    }

    /// Os membros que só a `InputBox` tem.
    fn input_box(&mut self) {
        self.read("password", |session| Ok(JsData::Bool(session.input_box()?.password())));
        self.write("password", |session, value| {
            session.input_box()?.set_password(flag(value));
            Ok(())
        });
        self.read("prompt", |session| Ok(text_or_undefined(session.input_box()?.prompt())));
        self.write("prompt", |session, value| {
            session.input_box()?.set_prompt(optional_text(value));
            Ok(())
        });
        self.read("valueSelection", |session| {
            Ok(session.core().value_selection().map_or(JsData::Undefined, |(start, end)| JsData::Array(vec![JsData::Num(f64::from(start)), JsData::Num(f64::from(end))])))
        });
        self.write("valueSelection", |session, value| {
            let selection = match value {
                JsData::Array(range) => match (opt_number(range.first()), opt_number(range.get(1))) {
                    (Some(start), Some(end)) => Some((start as u32, end as u32)),
                    _ => None,
                },
                _ => None,
            };
            session.core().set_value_selection(selection);
            Ok(())
        });
        self.read("validationMessage", |session| {
            Ok(match session.input_box()?.validation_message() {
                Some(InputBoxValidation::Text(text)) => JsData::Str(text),
                Some(InputBoxValidation::Message { message, severity }) => {
                    let mut fields = vec![("message".to_owned(), JsData::Str(message))];
                    if let Some(severity) = severity {
                        fields.push(("severity".to_owned(), JsData::Num(f64::from(severity.value()))));
                    }
                    JsData::Object(fields)
                }
                Some(InputBoxValidation::None) | None => JsData::Undefined,
            })
        });
        self.write("validationMessage", |session, value| {
            session.input_box()?.set_validation_message(Some(validation_of(value)));
            Ok(())
        });
    }

    fn define(self, runner: &NodeRunner, name: &str, construct_hint: &str) {
        let message = format!("{name} não tem construtor público: use {construct_hint}");
        runner.define_accessor_class(
            ClassSpec {
                name: name.to_owned(),
                length: 0,
                construct: Rc::new(move |_| Err(message.clone())),
                methods: self.methods,
                getters: self.getters,
                statics: vec![],
                parent: None,
            },
            self.setters,
        );
    }
}

/// Define as classes `QuickPick` e `InputBox` (protótipos dos objetos que `createQuickPick` e `createInputBox` entregam).
pub fn define_classes(runner: &NodeRunner, sessions: &Rc<QuickSessions>, table: &Rc<DisposableTable>) {
    let mut quick_pick = Members::new(runner, table, sessions);
    quick_pick.common();
    quick_pick.quick_pick();
    quick_pick.define(runner, "QuickPick", "window.createQuickPick");

    let mut input_box = Members::new(runner, table, sessions);
    input_box.common();
    input_box.input_box();
    input_box.define(runner, "InputBox", "window.createInputBox");
}

/// `window.createQuickPick()`.
pub fn create_quick_pick(quick_open: &ExtHostQuickOpen, sessions: &QuickSessions) -> Result<JsData, String> {
    let pick = quick_open.create_quick_pick();
    Ok(sessions.add(Session::Pick(Rc::new(PickSession { pick, items: RefCell::new(Vec::new()), buttons: RefCell::new(Vec::new()) })), "QuickPick"))
}

/// `window.createInputBox()`.
pub fn create_input_box(quick_open: &ExtHostQuickOpen, sessions: &QuickSessions) -> Result<JsData, String> {
    let input_box = quick_open.create_input_box();
    Ok(sessions.add(Session::Input(Rc::new(InputSession { input_box, buttons: RefCell::new(Vec::new()) })), "InputBox"))
}
