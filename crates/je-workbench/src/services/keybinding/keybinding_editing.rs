//! Porte de `src/vs/workbench/services/keybinding/common/keybindingEditing.ts`: o `IKeybindingEditingService` e o
//! `KeybindingsEditingService`, que adicionam, editam, removem e restauram atalhos no `keybindings.json` do perfil
//! corrente, editando o texto por um modelo de texto e salvando em seguida.
//!
//! Decisões de tradução:
//! - **Injeção.** Os quatro parâmetros decorados (`ITextModelService`, `ITextFileService`, `IFileService` e
//!   `IUserDataProfileService`) são o `Deps` do [`Injectable`]; `registerSingleton(..., InstantiationType.Delayed)` é
//!   [`register`], com o [`KeybindingEditingServiceProxy`] como o proxy da instanciação tardia.
//! - **`queue.queue(() => ...)`** é a [`Queue`] do `je-base`; os `async` são [`Promise`] (`async_fn`).
//! - **JSON.** `json.parse(model.getValue())` é o [`json::parse`] sobre as unidades UTF-16 do modelo (os erros de
//!   leitura são descartados, como no original), e `setProperty(...)[0]` é o primeiro [`Edit`] de [`set_property`]; sem
//!   nenhuma edição o original lê `offset` de `undefined`, e aqui o erro é o `TypeError` equivalente.
//! - **Entradas do usuário.** `IUserFriendlyKeybinding` é o [`Value`] da entrada: `command`, `key` e `when` são lidos só
//!   quando são strings (o original os leria sem checar o tipo), e `args` entra em [`objects::equals`] como valor
//!   dinâmico.
//! - **`when` vazio.** `if (when)` e `!keybinding.when` testam a verdade de uma string, ou seja, o texto vazio conta como
//!   ausente.
//! - **Edições no buffer.** `applyEditsToBuffer` converte offsets UTF-16 em posições do [`TextModel`] e empurra a
//!   operação sem estado de cursor de volta (`() => []`).

use std::rc::{Rc, Weak};

use je_base::JsString;
use je_base::async_::{Queue, async_fn};
use je_base::dynamic::{Dynamic, Truthy};
use je_base::errors::Error;
use je_base::json::{self, ParseError, ParseOptions, Value};
use je_base::json_edit::{EditSegment, set_property};
use je_base::json_formatter::{Edit, FormattingOptions};
use je_base::keybindings::ResolvedKeybinding;
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::nls::localize;
use je_base::objects;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_editor::edit_operation::EditOperation;
use je_editor::model::{Eol, ValidEditOperation};
use je_editor::range::Range;
use je_editor::selection::Selection;
use je_editor::text_model::TextModel;
use je_instantiation::{Injectable, InstantiationType, delayed_proxy, register_singleton, service_id, service_impl};
use je_platform::contextkey::ContextKeyExpr;
use je_platform::files::i_file_service::IFileService;
use je_platform::js::from_json;
use je_platform::keybinding::resolved_keybinding_item::ResolvedKeybindingItem;

use crate::services::textfile::textfiles::{ITextFileService, WriteTextFileOptions};
use crate::services::textmodel_resolver::resolver_service::{ITextModelService, TextModelReference};
use crate::services::user_data_profile::user_data_profile::IUserDataProfileService;

/// `IKeybindingEditingService`.
pub trait IKeybindingEditingService {
    fn add_keybinding(&self, keybinding_item: Rc<ResolvedKeybindingItem>, key: &str, when: Option<&str>) -> Promise<()>;

    fn edit_keybinding(&self, keybinding_item: Rc<ResolvedKeybindingItem>, key: &str, when: Option<&str>) -> Promise<()>;

    fn remove_keybinding(&self, keybinding_item: Rc<ResolvedKeybindingItem>) -> Promise<()>;

    fn reset_keybinding(&self, keybinding_item: Rc<ResolvedKeybindingItem>) -> Promise<()>;
}

delayed_proxy!(pub KeybindingEditingServiceProxy for dyn IKeybindingEditingService {
    methods {
        fn add_keybinding(&self, keybinding_item: Rc<ResolvedKeybindingItem>, key: &str, when: Option<&str>) -> Promise<()>;
        fn edit_keybinding(&self, keybinding_item: Rc<ResolvedKeybindingItem>, key: &str, when: Option<&str>) -> Promise<()>;
        fn remove_keybinding(&self, keybinding_item: Rc<ResolvedKeybindingItem>) -> Promise<()>;
        fn reset_keybinding(&self, keybinding_item: Rc<ResolvedKeybindingItem>) -> Promise<()>;
    }
    events {}
});

service_id!(pub I_KEYBINDING_EDITING_SERVICE: dyn IKeybindingEditingService = "keybindingEditingService", proxy KeybindingEditingServiceProxy);

/// `KeybindingsEditingService`.
pub struct KeybindingsEditingService {
    this: Weak<KeybindingsEditingService>,
    base: Rc<Disposable>,
    queue: Queue<()>,
    text_model_resolver_service: Rc<dyn ITextModelService>,
    text_file_service: Rc<dyn ITextFileService>,
    file_service: Rc<dyn IFileService>,
    user_data_profile_service: Rc<dyn IUserDataProfileService>,
}

impl Injectable for KeybindingsEditingService {
    type Args = ();
    type Deps = (Rc<dyn ITextModelService>, Rc<dyn ITextFileService>, Rc<dyn IFileService>, Rc<dyn IUserDataProfileService>);

    fn create(_: (), (text_model_resolver_service, text_file_service, file_service, user_data_profile_service): Self::Deps) -> Rc<Self> {
        Rc::new_cyclic(|this| Self {
            this: this.clone(),
            base: Disposable::new(),
            queue: Queue::new(),
            text_model_resolver_service,
            text_file_service,
            file_service,
            user_data_profile_service,
        })
    }
}

service_impl!(KeybindingsEditingService => dyn IKeybindingEditingService, disposable);

/// `registerSingleton(IKeybindingEditingService, KeybindingsEditingService, InstantiationType.Delayed)`.
pub fn register() {
    register_singleton::<KeybindingsEditingService>(I_KEYBINDING_EDITING_SERVICE, InstantiationType::Delayed);
}

impl IDisposable for KeybindingsEditingService {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl IKeybindingEditingService for KeybindingsEditingService {
    fn add_keybinding(&self, keybinding_item: Rc<ResolvedKeybindingItem>, key: &str, when: Option<&str>) -> Promise<()> {
        let (this, key, when) = (self.rc(), key.to_owned(), when.map(str::to_owned));
        // queue up writes to prevent race conditions
        self.queue.queue(move || this.do_edit_keybinding(keybinding_item, key, when, true))
    }

    fn edit_keybinding(&self, keybinding_item: Rc<ResolvedKeybindingItem>, key: &str, when: Option<&str>) -> Promise<()> {
        let (this, key, when) = (self.rc(), key.to_owned(), when.map(str::to_owned));
        // queue up writes to prevent race conditions
        self.queue.queue(move || this.do_edit_keybinding(keybinding_item, key, when, false))
    }

    fn remove_keybinding(&self, keybinding_item: Rc<ResolvedKeybindingItem>) -> Promise<()> {
        let this = self.rc();
        // queue up writes to prevent race conditions
        self.queue.queue(move || this.do_remove_keybinding(keybinding_item))
    }

    fn reset_keybinding(&self, keybinding_item: Rc<ResolvedKeybindingItem>) -> Promise<()> {
        let this = self.rc();
        // queue up writes to prevent race conditions
        self.queue.queue(move || this.do_reset_keybinding(keybinding_item))
    }
}

/// O `TypeError` que `setProperty(...)[0]` seguido de `.offset` lança quando não há edição.
fn missing_edit_error() -> Error {
    Error::named("TypeError", "Cannot read properties of undefined (reading 'offset')")
}

/// `setProperty(model.getValue(), path, value, { tabSize, insertSpaces, eol })`.
fn set_property_edits(model: &TextModel, path: &[EditSegment], value: Option<Value>) -> Result<Vec<Edit>, Error> {
    let options = model.get_options();
    let formatting_options = FormattingOptions {
        tab_size: Some(usize::try_from(options.tab_size).unwrap_or_default()),
        insert_spaces: Some(options.insert_spaces),
        eol: Some(JsString::from(eol_text(model.get_eol()))),
    };
    set_property(model.get_value().units(), path, value, &formatting_options, None).map_err(|error| Error::new(error.0))
}

/// `model.getEOL()`.
fn eol_text(eol: Eol) -> String {
    String::from_utf16_lossy(eol.as_units())
}

/// `json.parse(model.getValue())`, sem os erros e sem as opções.
fn parse_entries(model: &TextModel) -> Vec<Value> {
    match json::parse(model.get_value().units(), &mut Vec::new(), ParseOptions::default()) {
        Some(Value::Array(entries)) => entries,
        _ => Vec::new(),
    }
}

/// `entry[name]` quando é uma string (`undefined` para o resto).
fn string_property(entry: &Value, name: &str) -> Option<String> {
    match entry {
        Value::Object(object) => match object.get(name) {
            Some(Value::String(text)) => Some(text.to_string_lossy()),
            _ => None,
        },
        _ => None,
    }
}

/// `entry.args`.
fn args_property(entry: &Value) -> Dynamic {
    match entry {
        Value::Object(object) => object.get("args").map_or(Dynamic::Undefined, from_json),
        _ => Dynamic::Undefined,
    }
}

/// A verdade (`if (value)`) de um texto opcional: `undefined` e o texto vazio são falsos.
fn is_non_empty(text: Option<&str>) -> bool {
    text.is_some_and(|text| !text.is_empty())
}

/// `asObject(key, command, when, negate)`.
fn as_object(key: &str, command: Option<&str>, when: Option<&str>, negate: bool) -> Value {
    let mut object = json::Object::new();
    object.insert(JsString::from("key"), Value::String(JsString::from(key)));
    if let Some(command) = command.filter(|command| !command.is_empty()) {
        let command = if negate { format!("-{command}") } else { command.to_owned() };
        object.insert(JsString::from("command"), Value::String(JsString::from(command)));
    }
    if is_non_empty(when) {
        object.insert(JsString::from("when"), Value::String(JsString::from(when.unwrap_or_default())));
    }
    Value::Object(object)
}

/// `areSame(a, b)`.
fn are_same(a: &Value, b: &Value) -> bool {
    if string_property(a, "command") != string_property(b, "command") {
        return false;
    }
    if string_property(a, "key") != string_property(b, "key") {
        return false;
    }
    let when_a = ContextKeyExpr::deserialize(string_property(a, "when").as_deref());
    let when_b = ContextKeyExpr::deserialize(string_property(b, "when").as_deref());
    match (&when_a, &when_b) {
        (Some(_), None) | (None, Some(_)) => return false,
        (Some(when_a), Some(when_b)) if !when_a.equals(when_b) => return false,
        _ => {}
    }
    objects::equals(&args_property(a), &args_property(b))
}

/// `findUserKeybindingEntryIndex(keybindingItem, userKeybindingEntries)`.
fn find_user_keybinding_entry_index(keybinding_item: &ResolvedKeybindingItem, user_keybinding_entries: &[Value]) -> Option<usize> {
    user_keybinding_entries.iter().position(|keybinding| {
        if string_property(keybinding, "command") != keybinding_item.command {
            return false;
        }
        let when = string_property(keybinding, "when");
        match (is_non_empty(when.as_deref()), &keybinding_item.when) {
            (false, None) => true,
            (true, Some(item_when)) => ContextKeyExpr::deserialize(when.as_deref()).is_some_and(|expression| expression.serialize() == item_when.serialize()),
            _ => false,
        }
    })
}

/// `findUnassignedDefaultKeybindingEntryIndex(keybindingItem, userKeybindingEntries)`.
fn find_unassigned_default_keybinding_entry_index(keybinding_item: &ResolvedKeybindingItem, user_keybinding_entries: &[Value]) -> Vec<usize> {
    let unassigned = keybinding_item.command.as_ref().map(|command| format!("-{command}"));
    user_keybinding_entries
        .iter()
        .enumerate()
        .filter(|(_, entry)| string_property(entry, "command") == unassigned)
        .map(|(index, _)| index)
        .collect()
}

/// `applyEditsToBuffer(edit, model)`.
fn apply_edits_to_buffer(edit: &Edit, model: &TextModel) {
    let start_position = model.get_position_at(edit.offset as i64);
    let end_position = model.get_position_at((edit.offset + edit.length) as i64);
    let range = Range::new(start_position.line_number, start_position.column, end_position.line_number, end_position.column);
    let current_text = model.get_value_in_range(&range);
    let edit_operation = if current_text.is_empty() {
        EditOperation::insert(start_position, edit.content.clone())
    } else {
        EditOperation::replace(range, Some(edit.content.clone()))
    };
    let mut cursor_state_computer = |_: &[ValidEditOperation]| Some(Vec::<Selection>::new());
    model.push_edit_operations(
        Some(vec![Selection::new(start_position.line_number, start_position.column, start_position.line_number, start_position.column)]),
        &[edit_operation],
        Some(&mut cursor_state_computer),
        None,
        None,
    );
}

/// O primeiro `setProperty(...)[0]` aplicado ao buffer.
fn apply_first_edit(edits: Vec<Edit>, model: &TextModel) -> Result<(), Error> {
    let edit = edits.into_iter().next().ok_or_else(missing_edit_error)?;
    apply_edits_to_buffer(&edit, model);
    Ok(())
}

/// O modelo de texto de uma referência (`reference.object.textEditorModel`, sempre presente num modelo resolvido).
fn text_model_of(reference: &TextModelReference) -> Result<Rc<TextModel>, Error> {
    reference.object.text_editor_model().ok_or_else(|| Error::new("Unable to resolve the keybindings configuration file."))
}

impl KeybindingsEditingService {
    fn rc(&self) -> Rc<Self> {
        self.this.upgrade().expect("o serviço está vivo enquanto é usado")
    }

    fn keybindings_resource(&self) -> URI {
        self.user_data_profile_service.current_profile().keybindings_resource
    }

    fn do_edit_keybinding(self: &Rc<Self>, keybinding_item: Rc<ResolvedKeybindingItem>, key: String, when: Option<String>, add: bool) -> Promise<()> {
        let this = self.clone();
        async_fn(async move {
            let reference = this.resolve_and_validate().await?;
            let model = text_model_of(&reference)?;
            if add {
                this.update_keybinding(&keybinding_item, &key, when.as_deref(), &model, None)?;
            } else {
                let user_keybinding_entries = parse_entries(&model);
                let user_keybinding_entry_index = find_user_keybinding_entry_index(&keybinding_item, &user_keybinding_entries);
                this.update_keybinding(&keybinding_item, &key, when.as_deref(), &model, user_keybinding_entry_index)?;
                if keybinding_item.is_default && keybinding_item.resolved_keybinding.is_some() {
                    this.remove_default_keybinding(&keybinding_item, &model)?;
                }
            }
            let result = this.save().await;
            reference.dispose();
            result
        })
    }

    fn do_remove_keybinding(self: &Rc<Self>, keybinding_item: Rc<ResolvedKeybindingItem>) -> Promise<()> {
        let this = self.clone();
        async_fn(async move {
            let reference = this.resolve_and_validate().await?;
            let model = text_model_of(&reference)?;
            if keybinding_item.is_default {
                this.remove_default_keybinding(&keybinding_item, &model)?;
            } else {
                this.remove_user_keybinding(&keybinding_item, &model)?;
            }
            let result = this.save().await;
            reference.dispose();
            result
        })
    }

    fn do_reset_keybinding(self: &Rc<Self>, keybinding_item: Rc<ResolvedKeybindingItem>) -> Promise<()> {
        let this = self.clone();
        async_fn(async move {
            let reference = this.resolve_and_validate().await?;
            let model = text_model_of(&reference)?;
            if !keybinding_item.is_default {
                this.remove_user_keybinding(&keybinding_item, &model)?;
                this.remove_unassigned_default_keybinding(&keybinding_item, &model)?;
            }
            let result = this.save().await;
            reference.dispose();
            result
        })
    }

    fn save(&self) -> Promise<()> {
        let save = self.text_file_service.save(&self.keybindings_resource(), None);
        async_fn(async move {
            save.await?;
            Ok(())
        })
    }

    fn update_keybinding(
        &self,
        keybinding_item: &ResolvedKeybindingItem,
        new_key: &str,
        when: Option<&str>,
        model: &TextModel,
        user_keybinding_entry_index: Option<usize>,
    ) -> Result<(), Error> {
        match user_keybinding_entry_index {
            Some(index) => {
                let index = index as i64;
                // Update the keybinding with new key
                let key_edits = set_property_edits(model, &[EditSegment::Index(index), EditSegment::from("key")], Some(Value::String(JsString::from(new_key))))?;
                apply_first_edit(key_edits, model)?;
                let when_value = when.filter(|when| !when.is_empty()).map(|when| Value::String(JsString::from(when)));
                let when_edits = set_property_edits(model, &[EditSegment::Index(index), EditSegment::from("when")], when_value)?;
                if let Some(edit) = when_edits.first() {
                    apply_edits_to_buffer(edit, model);
                }
                Ok(())
            }
            None => {
                // Add the new keybinding with new key
                let entry = as_object(new_key, keybinding_item.command.as_deref(), when, false);
                apply_first_edit(set_property_edits(model, &[EditSegment::Index(-1)], Some(entry))?, model)
            }
        }
    }

    fn remove_user_keybinding(&self, keybinding_item: &ResolvedKeybindingItem, model: &TextModel) -> Result<(), Error> {
        let user_keybinding_entries = parse_entries(model);
        if let Some(index) = find_user_keybinding_entry_index(keybinding_item, &user_keybinding_entries) {
            apply_first_edit(set_property_edits(model, &[EditSegment::Index(index as i64)], None)?, model)?;
        }
        Ok(())
    }

    fn remove_default_keybinding(&self, keybinding_item: &ResolvedKeybindingItem, model: &TextModel) -> Result<(), Error> {
        let key = keybinding_item.resolved_keybinding.as_ref().and_then(|resolved| resolved.get_user_settings_label());
        if let Some(key) = key.filter(|key| !key.is_empty()) {
            let when = keybinding_item.when.as_ref().map(|when| when.serialize());
            let entry = as_object(&key, keybinding_item.command.as_deref(), when.as_deref(), true);
            let user_keybinding_entries = parse_entries(model);
            if user_keybinding_entries.iter().all(|existing| !are_same(existing, &entry)) {
                apply_first_edit(set_property_edits(model, &[EditSegment::Index(-1)], Some(entry))?, model)?;
            }
        }
        Ok(())
    }

    fn remove_unassigned_default_keybinding(&self, keybinding_item: &ResolvedKeybindingItem, model: &TextModel) -> Result<(), Error> {
        let user_keybinding_entries = parse_entries(model);
        let indices = find_unassigned_default_keybinding_entry_index(keybinding_item, &user_keybinding_entries);
        for index in indices.into_iter().rev() {
            apply_first_edit(set_property_edits(model, &[EditSegment::Index(index as i64)], None)?, model)?;
        }
        Ok(())
    }

    fn resolve_model_reference(self: &Rc<Self>) -> Promise<TextModelReference> {
        let this = self.clone();
        async_fn(async move {
            let resource = this.keybindings_resource();
            let exists = this.file_service.exists(&resource).await?;
            if !exists {
                let options = WriteTextFileOptions { encoding: Some("utf8".to_owned()), ..WriteTextFileOptions::default() };
                this.text_file_service.write(&resource, Self::get_empty_content().into(), Some(options)).await?;
            }
            this.text_model_resolver_service.create_model_reference(&resource).await
        })
    }

    fn resolve_and_validate(self: &Rc<Self>) -> Promise<TextModelReference> {
        let this = self.clone();
        async_fn(async move {
            // Target cannot be dirty if not writing into buffer
            if this.text_file_service.is_dirty(&this.keybindings_resource()) {
                return Err(Error::new(localize(
                    "errorKeybindingsFileDirty",
                    "Unable to write because the keybindings configuration file has unsaved changes. Please save it first and then try again.",
                    &[],
                )));
            }

            let reference = this.resolve_model_reference().await?;
            let model = text_model_of(&reference)?;
            let eol = eol_text(model.get_eol());
            let value = model.get_value();
            if !value.is_empty() {
                let (result, parse_errors) = Self::parse(&model);
                if !parse_errors.is_empty() {
                    reference.dispose();
                    return Err(Error::new(localize(
                        "parseErrors",
                        "Unable to write to the keybindings configuration file. Please open it to correct errors/warnings in the file and try again.",
                        &[],
                    )));
                }
                if result.as_ref().is_some_and(|result| from_json(result).is_truthy()) {
                    if !matches!(result, Some(Value::Array(_))) {
                        reference.dispose();
                        return Err(Error::new(localize(
                            "errorInvalidConfiguration",
                            "Unable to write to the keybindings configuration file. It has an object which is not of type Array. Please open the file to clean up and try again.",
                            &[],
                        )));
                    }
                } else {
                    let content = JsString::from(format!("{eol}[]"));
                    apply_edits_to_buffer(&Edit { length: content.len(), offset: value.len(), content }, &model);
                }
            } else {
                let content = JsString::from(Self::get_empty_content());
                apply_edits_to_buffer(&Edit { length: content.len(), offset: 0, content }, &model);
            }
            Ok(reference)
        })
    }

    /// `parse(model)`: o resultado e os erros de leitura (com vírgula final e conteúdo vazio permitidos).
    fn parse(model: &TextModel) -> (Option<Value>, Vec<ParseError>) {
        let mut parse_errors = Vec::new();
        let options = ParseOptions { allow_trailing_comma: true, allow_empty_content: true, ..ParseOptions::default() };
        let result = json::parse(model.get_value().units(), &mut parse_errors, options);
        (result, parse_errors)
    }

    fn get_empty_content() -> String {
        format!("// {}\n[\n]", localize("emptyKeybindingsHeader", "Place your key bindings in this file to override the defaults", &[]))
    }
}
