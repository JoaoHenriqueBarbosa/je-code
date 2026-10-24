//! Porte de `src/vs/workbench/services/configurationResolver/common/configurationResolverExpression.ts`: a expressão
//! analisada de um objeto (ou string) de configuração, que acha cada `${nome:argumento}` e substitui os valores resolvidos
//! no lugar, inclusive nas chaves dos objetos e dentro dos próprios valores resolvidos.
//!
//! Notas de tradução:
//! - **O objeto é um [`Dynamic`].** `structuredClone(object)` é o `deep_clone` do `je-base`; os objetos e arrays têm
//!   identidade (`Rc`), então um local (`PropertyLocation`) guarda o objeto pai e o nome da propriedade e a substituição
//!   escreve nele, como no original. Nome de propriedade de array é o índice em texto (`"0"`), que o [`Dynamic`] aceita.
//! - **Textos em UTF-16.** O analisador de variáveis do `je-platform` trabalha em unidades UTF-16 (`JsString`), e os
//!   deslocamentos de `indexOf('${', pos)` também.
//! - **`replaceAll(id, valor)`** é a troca do JavaScript, inclusive os padrões especiais do texto de troca (`$$`, `$&`,
//!   `` $` `` e `$'`): um valor resolvido com `$&` não vira literal, como no original.
//! - **`unresolved()`** é um gerador: [`Unresolved`] é o iterador, que só começa (monta a lista inicial e se registra para
//!   saber das variáveis novas) no primeiro `next()`, e que descarta a variável entregue só quando o consumidor volta
//!   para pedir a próxima. O `Set<(r) => void>` de notificadores é a lista `new_replacement_notifiers`, onde cada
//!   iterador ativo se registra; o iterador se desregistra ao terminar ou ao ser descartado.
//! - **`parse(object)`** devolve `Rc<Self>`: a expressão tem estado mutável (`&self` + `RefCell`), como o objeto do
//!   original. Passar uma expressão já analisada (`object instanceof ConfigurationResolverExpression`) não existe aqui,
//!   porque o `Dynamic` não pode ser uma expressão: quem já tem a expressão a usa direto.
//! - **`resolve(replacement, data)`** recebe `string | IResolvedValue`: qualquer coisa que vira [`IResolvedValue`]
//!   (`&str`, `String`, `JsString`, o próprio valor, ou um [`Dynamic`], que segue o `typeof data !== 'object'` do original).
//!   O `value` nulo (`null`) vira o texto `"null"`, que é o que o `replaceAll` do original escreve.
//! - **Raiz nula.** `delete config.windows` sobre `null` ou `undefined` lança `TypeError` no original; aqui também.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::JsString;
use je_base::dynamic::Dynamic;
use je_base::errors::{self, Error};
use je_base::js_string::index_of;
use je_base::map::OrderedMap;
use je_base::objects::deep_clone;
use je_base::platform::Platform;
use je_platform::configuration::variables::{ConfigurationVariable, parse_configuration_variable};
use je_platform::js::{delete_property, object_keys, set_property, to_js_string};

use super::configuration_resolver::ConfiguredInput;

const DOLLAR: u16 = b'$' as u16;
const OPEN: u16 = b'{' as u16;
const AMPERSAND: u16 = b'&' as u16;
const BACKTICK: u16 = b'`' as u16;
const QUOTE: u16 = b'\'' as u16;

/// A replacement found in the object, as ${name} or ${name:arg}.
pub type Replacement = ConfigurationVariable;

/// `IResolvedValue`.
#[derive(Clone, Debug)]
pub struct IResolvedValue {
    pub value: Option<JsString>,

    /// Present when the variable is resolved from an input field.
    pub input: Option<ConfiguredInput>,
}

impl From<&str> for IResolvedValue {
    fn from(value: &str) -> Self {
        IResolvedValue { value: Some(JsString::from(value)), input: None }
    }
}

impl From<String> for IResolvedValue {
    fn from(value: String) -> Self {
        IResolvedValue { value: Some(JsString::from(value)), input: None }
    }
}

impl From<JsString> for IResolvedValue {
    fn from(value: JsString) -> Self {
        IResolvedValue { value: Some(value), input: None }
    }
}

impl From<&Dynamic> for IResolvedValue {
    /// `if (typeof data !== 'object') data = { value: String(data) }`; um objeto é lido como `IResolvedValue`
    /// (`data.value` e `data.input`).
    fn from(data: &Dynamic) -> Self {
        if data.type_of() != "object" {
            return IResolvedValue { value: Some(to_js_string(data)), input: None };
        }
        let value = match data.get_str("value") {
            Dynamic::Undefined => None,
            Dynamic::String(text) => Some(text),
            other => Some(to_js_string(&other)),
        };
        let input = match data.get_str("input") {
            Dynamic::Undefined => None,
            other => Some(ConfiguredInput::new(other)),
        };
        IResolvedValue { value, input }
    }
}

/// `PropertyLocation`: o objeto pai e o nome da propriedade (o índice, em texto, nos arrays).
#[derive(Clone)]
struct PropertyLocation {
    object: Dynamic,
    property_name: JsString,
    replace_key_name: bool,
}

/// `IReplacementLocation`.
struct ReplacementLocation {
    replacement: Replacement,
    locations: Vec<PropertyLocation>,
    resolved: Option<IResolvedValue>,
}

/// O `newReplacements` de um `unresolved()` em andamento: um `Map<string, Replacement>` na ordem de inserção.
type PendingReplacements = Rc<RefCell<Vec<Replacement>>>;

/// `newReplacements.set(replacement.id, replacement)`: troca o valor onde a chave já está, senão acrescenta.
fn set_pending(pending: &RefCell<Vec<Replacement>>, replacement: Replacement) {
    let mut pending = pending.borrow_mut();
    match pending.iter().position(|existing| existing.id == replacement.id) {
        Some(index) => pending[index] = replacement,
        None => pending.push(replacement),
    }
}

/// `text.replaceAll(search, replacement)` com um texto de busca e a substituição de padrões (`GetSubstitution`).
fn replace_all(text: &JsString, search: &JsString, replacement: &JsString) -> JsString {
    let units = text.units();
    let search_units = search.units();
    let step = search_units.len().max(1);

    let mut positions = Vec::new();
    let mut from = 0;
    while from <= units.len() {
        let Some(position) = index_of(units, search_units, from) else { break };
        positions.push(position);
        from = position + step;
    }

    let mut out: Vec<u16> = Vec::with_capacity(units.len());
    let mut end_of_previous = 0;
    for position in positions {
        out.extend_from_slice(&units[end_of_previous..position]);
        let tail_start = (position + search_units.len()).min(units.len());
        let replacement_units = replacement.units();
        let mut i = 0;
        while i < replacement_units.len() {
            if replacement_units[i] == DOLLAR && i + 1 < replacement_units.len() {
                let special = match replacement_units[i + 1] {
                    DOLLAR => Some(&[DOLLAR][..]),
                    AMPERSAND => Some(search_units),
                    BACKTICK => Some(&units[..position]),
                    QUOTE => Some(&units[tail_start..]),
                    _ => None,
                };
                if let Some(special) = special {
                    out.extend_from_slice(special);
                    i += 2;
                    continue;
                }
            }
            out.push(replacement_units[i]);
            i += 1;
        }
        end_of_previous = tail_start;
    }
    out.extend_from_slice(&units[end_of_previous..]);
    JsString::from_units(out)
}

/// `ConfigurationResolverExpression`.
pub struct ConfigurationResolverExpression {
    locations: RefCell<OrderedMap<JsString, ReplacementLocation>>,
    root: Dynamic,
    string_root: bool,
    /// Callbacks when a new replacement is made, so that nested resolutions from `expr.unresolved()` can be fulfilled in
    /// the same iteration.
    new_replacement_notifiers: RefCell<Vec<(usize, PendingReplacements)>>,
    next_notifier_id: Cell<usize>,
}

impl ConfigurationResolverExpression {
    pub const VARIABLE_LHS: &'static str = "${";

    fn new(object: &Dynamic) -> Self {
        // If the input is a string, wrap it in an object so we can use the same logic
        let (root, string_root) = match object {
            Dynamic::String(_) => (Dynamic::object([("value", object.clone())]), true),
            _ => (deep_clone(object), false),
        };
        Self {
            locations: RefCell::new(OrderedMap::new()),
            root,
            string_root,
            new_replacement_notifiers: RefCell::new(Vec::new()),
            next_notifier_id: Cell::new(0),
        }
    }

    /// Creates a new [`ConfigurationResolverExpression`] from an object. Note that platform-specific keys (i.e. `windows`,
    /// `osx`, `linux`) are applied during parsing.
    pub fn parse(object: &Dynamic) -> Rc<Self> {
        let expr = Rc::new(Self::new(object));
        expr.apply_platform_specific_keys();
        expr.parse_object(&expr.root);
        expr
    }

    fn apply_platform_specific_keys(&self) {
        let config = &self.root; // already cloned by ctor, safe to change
        let host = Platform::detect();
        let key = if host.is_windows {
            Some("windows")
        } else if host.is_macintosh {
            Some("osx")
        } else if host.is_linux {
            Some("linux")
        } else {
            None
        };

        if config.is_nullish() {
            errors::throw(Error::type_error("Cannot convert undefined or null to object"));
        }

        if let Some(key) = key
            && let Dynamic::Object(object) = config
            && object.has_own(&JsString::from(key))
        {
            let source = config.get_str(key);
            if source.is_nullish() {
                errors::throw(Error::type_error("Cannot convert undefined or null to object"));
            }
            for name in object_keys(&source) {
                set_property(config, &name, source.get(&name));
            }
        }

        delete_property(config, &JsString::from("windows"));
        delete_property(config, &JsString::from("osx"));
        delete_property(config, &JsString::from("linux"));
    }

    fn parse_object(&self, obj: &Dynamic) {
        match obj {
            Dynamic::Array(array) => {
                for index in 0..array.len() {
                    let value = array.get(index);
                    if let Dynamic::String(text) = &value {
                        self.parse_string(obj, &JsString::from(index.to_string()), text, false, &mut Vec::new());
                    } else {
                        self.parse_object(&value);
                    }
                }
            }
            Dynamic::Object(object) => {
                for (key, value) in object.entries() {
                    self.parse_string(obj, &key, &key, true, &mut Vec::new()); // parse key

                    if let Dynamic::String(text) = &value {
                        self.parse_string(obj, &key, text, false, &mut Vec::new());
                    } else {
                        self.parse_object(&value);
                    }
                }
            }
            _ => {}
        }
    }

    fn parse_string(&self, object: &Dynamic, property_name: &JsString, value: &JsString, replace_key_name: bool, replacement_path: &mut Vec<JsString>) {
        let mut pos = 0;
        while pos < value.len() {
            let Some(found) = index_of(value.units(), &[DOLLAR, OPEN], pos) else { break };
            match parse_configuration_variable(value, found) {
                Some(parsed) => {
                    pos = parsed.end + 1;
                    if replacement_path.contains(&parsed.replacement.id) {
                        continue;
                    }

                    let new_location = PropertyLocation { object: object.clone(), property_name: property_name.clone(), replace_key_name };
                    let resolved = {
                        let mut locations = self.locations.borrow_mut();
                        match locations.get_mut(&parsed.replacement.id) {
                            Some(entry) => {
                                entry.locations.push(new_location.clone());
                                entry.resolved.clone()
                            }
                            None => {
                                locations.insert(
                                    parsed.replacement.id.clone(),
                                    ReplacementLocation { replacement: parsed.replacement.clone(), locations: vec![new_location.clone()], resolved: None },
                                );
                                None
                            }
                        }
                    };

                    match resolved {
                        Some(resolved) => self.resolve_at_location(&parsed.replacement, &new_location, &resolved, replacement_path),
                        None => self.notify_new_replacement(&parsed.replacement),
                    }
                }
                None => pos = found + 2,
            }
        }
    }

    fn notify_new_replacement(&self, replacement: &Replacement) {
        let notifiers: Vec<PendingReplacements> = self.new_replacement_notifiers.borrow().iter().map(|(_, pending)| pending.clone()).collect();
        for pending in notifiers {
            set_pending(&pending, replacement.clone());
        }
    }

    /// `unresolved()`: the replacements which have not yet been resolved.
    pub fn unresolved(self: &Rc<Self>) -> Unresolved {
        Unresolved { expression: self.clone(), pending: Rc::new(RefCell::new(Vec::new())), notifier_id: None, last_key: None, done: false }
    }

    /// `resolved()`: the replacements which have been resolved.
    pub fn resolved(&self) -> Vec<(Replacement, IResolvedValue)> {
        self.locations
            .borrow()
            .values()
            .filter_map(|location| location.resolved.as_ref().map(|resolved| (location.replacement.clone(), resolved.clone())))
            .collect()
    }

    /// `resolve(replacement, data)`: resolves a replacement into the string value. If the value is undefined, the original
    /// variable text will be preserved.
    pub fn resolve(&self, replacement: &Replacement, data: impl Into<IResolvedValue>) {
        let data = data.into();

        {
            let mut locations = self.locations.borrow_mut();
            let Some(location) = locations.get_mut(&replacement.id) else { return };
            location.resolved = Some(data.clone());
        }

        if data.value.is_some() {
            // `for (const l of location.locations)` vê o que for acrescentado durante a troca: o índice relê a lista.
            let mut index = 0;
            loop {
                let location = {
                    let locations = self.locations.borrow();
                    locations.get(&replacement.id).and_then(|entry| entry.locations.get(index).cloned())
                };
                let Some(location) = location else { break };
                self.resolve_at_location(replacement, &location, &data, &mut Vec::new());
                index += 1;
            }
        }
    }

    fn resolve_at_location(&self, replacement: &Replacement, location: &PropertyLocation, data: &IResolvedValue, path: &mut Vec<JsString>) {
        let Some(value) = &data.value else { return };

        // avoid recursive resolution, e.g. ${env:FOO} -> ${env:BAR}=${env:FOO}
        path.push(replacement.id.clone());

        // note: in nested `self.parse_string`, parse only the new substring for any replacements, don't reparse the whole string
        let current = location.object.get(&location.property_name);
        if location.replace_key_name {
            let new_key = replace_all(&location.property_name, &replacement.id, value);
            delete_property(&location.object, &location.property_name);
            set_property(&location.object, &new_key, current);
            self.rename_key_in_locations(&location.object, &location.property_name, &new_key);
            self.parse_string(&location.object, &new_key, value, true, path);
        } else {
            let Dynamic::String(text) = current else {
                errors::throw(Error::type_error("object[propertyName].replaceAll is not a function"));
            };
            set_property(&location.object, &location.property_name, Dynamic::String(replace_all(&text, &replacement.id, value)));
            self.parse_string(&location.object, &location.property_name, value, false, path);
        }

        path.pop();
    }

    fn rename_key_in_locations(&self, obj: &Dynamic, old_key: &JsString, new_key: &JsString) {
        for location in self.locations.borrow_mut().values_mut() {
            for loc in &mut location.locations {
                if loc.object.strict_equals(obj) && loc.property_name == *old_key {
                    loc.property_name = new_key.clone();
                }
            }
        }
    }

    /// `toObject()`: returns the complete object. Any unresolved replacements are left intact.
    pub fn to_object(&self) -> Dynamic {
        // If we wrapped a string, unwrap it
        if self.string_root {
            return self.root.get_str("value");
        }

        self.root.clone()
    }
}

/// O gerador de `unresolved()`.
pub struct Unresolved {
    expression: Rc<ConfigurationResolverExpression>,
    pending: PendingReplacements,
    notifier_id: Option<usize>,
    last_key: Option<JsString>,
    done: bool,
}

impl Unresolved {
    /// O trecho do gerador antes do primeiro `yield`: a lista inicial e o registro do notificador.
    fn start(&mut self) {
        for location in self.expression.locations.borrow().values() {
            if location.resolved.is_none() {
                set_pending(&self.pending, location.replacement.clone());
            }
        }

        let id = self.expression.next_notifier_id.get();
        self.expression.next_notifier_id.set(id + 1);
        self.expression.new_replacement_notifiers.borrow_mut().push((id, self.pending.clone()));
        self.notifier_id = Some(id);
    }

    fn unregister(&mut self) {
        if let Some(id) = self.notifier_id.take() {
            self.expression.new_replacement_notifiers.borrow_mut().retain(|(registered, _)| *registered != id);
        }
    }
}

impl Iterator for Unresolved {
    type Item = Replacement;

    fn next(&mut self) -> Option<Replacement> {
        if self.done {
            return None;
        }

        if self.notifier_id.is_none() {
            self.start();
        } else if let Some(key) = self.last_key.take() {
            self.pending.borrow_mut().retain(|pending| pending.id != key);
        }

        let next = self.pending.borrow().first().cloned();
        match next {
            Some(replacement) => {
                self.last_key = Some(replacement.id.clone());
                Some(replacement)
            }
            None => {
                self.unregister();
                self.done = true;
                None
            }
        }
    }
}

impl Drop for Unresolved {
    fn drop(&mut self) {
        self.unregister();
    }
}
