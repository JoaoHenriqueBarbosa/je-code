//! `vscode.languages.createLanguageStatusItem(id, selector)`: o `LanguageStatusItem` do `ExtHostLanguages` portado
//! (`extHostLanguages.ts`) como a classe `vscode.LanguageStatusItem`.
//!
//! O objeto que a extensão segura carrega só o `_id` da entrada; as propriedades (`id`, `name`, `selector`, `text`, `detail`,
//! `severity`, `command`, `accessibilityInformation`, `busy`) são acessores do protótipo (leitura e escrita) sobre o item vivo, e
//! `dispose` desfaz o item. O `ExtHostLanguages` junta as mudanças de um mesmo turno e envia o `$setLanguageStatus` ao
//! `MainThreadLanguages`.
//!
//! Divergências:
//! - `text2` (proposta `languageStatusText`) não existe, como no `ExtHostLanguages` portado.
//! - `selector`, `command` e `accessibilityInformation` devolvem a cópia que a extensão passou (o item guarda os tipos do host), nunca o
//!   mesmo objeto.
//! - Um item descartado segue legível e gravável (a gravação só registra o aviso do original); a entrada fica na tabela até o fim do host.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use je_base::lifecycle::IDisposable;
use je_extensions::scanner::ExtensionDescription;
use je_platform::accessibility::IAccessibilityInformation;
use je_workbench::api::common::ext_host_languages::{ExtHostLanguages, LanguageStatusItem};
use je_workbench::api::common::ext_host_types::LanguageStatusSeverity;
use zjsc::api::embed_node::{ClassSpec, JsData, MethodCallback, NodeRunner, CLASS_KEY};

use crate::api::language_types::command_from_js;
use crate::api::languages::selector_from;
use crate::convert::{field, is_nullish, opt_bool, opt_number, opt_string, text_or_undefined};

/// Um item vivo e as cópias que a extensão passou.
struct StatusEntry {
    item: Rc<LanguageStatusItem>,
    selector: RefCell<JsData>,
    command: RefCell<JsData>,
    accessibility_information: RefCell<JsData>,
}

/// Os itens de estado vivos, por id (o objeto JavaScript carrega só o id).
#[derive(Default)]
pub struct LanguageStatusItems {
    next: Cell<u64>,
    entries: RefCell<HashMap<u64, Rc<StatusEntry>>>,
}

impl LanguageStatusItems {
    /// `createLanguageStatusItem(id, selector)`: o objeto que a extensão recebe.
    pub fn create(&self, languages: &ExtHostLanguages, extension: &ExtensionDescription, id: &str, selector: &JsData) -> Result<JsData, String> {
        let item = languages.create_language_status_item(extension, id, selector_from(selector)?).map_err(|error| error.message)?;
        let key = self.next.get() + 1;
        self.next.set(key);
        let entry = StatusEntry {
            item,
            selector: RefCell::new(selector.clone()),
            command: RefCell::new(JsData::Undefined),
            accessibility_information: RefCell::new(JsData::Undefined),
        };
        self.entries.borrow_mut().insert(key, Rc::new(entry));
        Ok(JsData::Object(vec![(CLASS_KEY.to_owned(), JsData::Str("LanguageStatusItem".to_owned())), ("_id".to_owned(), JsData::Num(key as f64))]))
    }

    fn entry_of(&self, this: &JsData) -> Result<Rc<StatusEntry>, String> {
        match field(this, "_id") {
            Some(JsData::Num(key)) => self.entries.borrow().get(&(*key as u64)).cloned().ok_or_else(|| "LanguageStatusItem: item desconhecido".to_owned()),
            _ => Err("LanguageStatusItem: this inválido".to_owned()),
        }
    }
}

/// Os acessores e os métodos do protótipo.
struct Members {
    items: Rc<LanguageStatusItems>,
    getters: Vec<(String, MethodCallback)>,
    setters: Vec<(String, MethodCallback)>,
}

impl Members {
    fn read(&mut self, name: &str, read: impl Fn(&StatusEntry) -> JsData + 'static) {
        let items = self.items.clone();
        self.getters.push((name.to_owned(), Rc::new(move |this, _| Ok(read(&*items.entry_of(this)?)))));
    }

    fn write(&mut self, name: &str, write: impl Fn(&StatusEntry, &JsData) -> Result<(), String> + 'static) {
        let items = self.items.clone();
        self.setters.push((
            name.to_owned(),
            Rc::new(move |this, args| {
                write(&*items.entry_of(this)?, args.first().unwrap_or(&JsData::Undefined))?;
                Ok(JsData::Undefined)
            }),
        ));
    }
}

/// `Command | undefined`.
fn optional_command(value: &JsData) -> Result<Option<je_workbench::api::common::ext_host_types::Command>, String> {
    if is_nullish(Some(value)) { Ok(None) } else { command_from_js(value).map(Some).map_err(|error| error.message) }
}

/// `AccessibilityInformation | undefined`: `label` e `role`.
fn optional_accessibility_information(value: &JsData) -> Result<Option<IAccessibilityInformation>, String> {
    if is_nullish(Some(value)) {
        return Ok(None);
    }
    let label = opt_string(field(value, "label")).ok_or("AccessibilityInformation.label must be a string")?;
    Ok(Some(IAccessibilityInformation { label, role: opt_string(field(value, "role")) }))
}

/// Define a classe `LanguageStatusItem` (o protótipo do objeto que `createLanguageStatusItem` entrega).
pub fn define_class(runner: &NodeRunner, items: &Rc<LanguageStatusItems>) {
    let mut members = Members { items: items.clone(), getters: Vec::new(), setters: Vec::new() };

    members.read("id", |entry| JsData::Str(entry.item.id()));

    members.read("name", |entry| text_or_undefined(entry.item.name()));
    members.write("name", |entry, value| {
        entry.item.set_name(opt_string(Some(value)));
        Ok(())
    });

    members.read("selector", |entry| entry.selector.borrow().clone());
    members.write("selector", |entry, value| {
        entry.item.set_selector(selector_from(value)?);
        *entry.selector.borrow_mut() = value.clone();
        Ok(())
    });

    members.read("text", |entry| JsData::Str(entry.item.text()));
    members.write("text", |entry, value| {
        entry.item.set_text(&opt_string(Some(value)).unwrap_or_default());
        Ok(())
    });

    members.read("detail", |entry| text_or_undefined(entry.item.detail()));
    members.write("detail", |entry, value| {
        entry.item.set_detail(opt_string(Some(value)));
        Ok(())
    });

    members.read("severity", |entry| JsData::Num(f64::from(entry.item.severity().value())));
    members.write("severity", |entry, value| {
        let severity = opt_number(Some(value)).and_then(|number| LanguageStatusSeverity::from_value(number as i32)).ok_or("LanguageStatusItem.severity must be a LanguageStatusSeverity")?;
        entry.item.set_severity(severity);
        Ok(())
    });

    members.read("command", |entry| entry.command.borrow().clone());
    members.write("command", |entry, value| {
        entry.item.set_command(optional_command(value)?);
        *entry.command.borrow_mut() = value.clone();
        Ok(())
    });

    members.read("accessibilityInformation", |entry| entry.accessibility_information.borrow().clone());
    members.write("accessibilityInformation", |entry, value| {
        entry.item.set_accessibility_information(optional_accessibility_information(value)?);
        *entry.accessibility_information.borrow_mut() = value.clone();
        Ok(())
    });

    members.read("busy", |entry| JsData::Bool(entry.item.busy()));
    members.write("busy", |entry, value| {
        entry.item.set_busy(opt_bool(Some(value)).unwrap_or(false));
        Ok(())
    });

    let dispose_items = items.clone();
    let methods: Vec<(String, u32, MethodCallback)> = vec![(
        "dispose".to_owned(),
        0,
        Rc::new(move |this, _| {
            dispose_items.entry_of(this)?.item.dispose();
            Ok(JsData::Undefined)
        }),
    )];
    runner.define_accessor_class(
        ClassSpec {
            name: "LanguageStatusItem".to_owned(),
            length: 0,
            construct: Rc::new(|_| Err("LanguageStatusItem não tem construtor público: use languages.createLanguageStatusItem".to_owned())),
            methods,
            getters: members.getters,
            statics: vec![],
            parent: None,
        },
        members.setters,
    );
}
