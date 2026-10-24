//! Porte de `src/vs/platform/configuration/common/configurationModels.ts`: `ConfigurationModel` (o
//! conteúdo de uma camada), `ConfigurationModelParser` (do texto JSON ao modelo), `Configuration` (as
//! camadas juntas), `ConfigurationChangeEvent` e `mergeChanges`.
//!
//! Representação:
//! - Um [`ConfigurationModel`] é um handle `Rc` (identidade como no original: `current === previous`
//!   vira [`ConfigurationModel::ptr_eq`]); `setValue`, `addValue` e `removeValue` mudam o modelo por
//!   `&self`, com mutabilidade interior.
//! - O `inspect` do modelo devolve um [`InspectValue`] preguiçoso: `value`, `override`, `merged` e
//!   `overrides` são métodos que calculam (e congelam, como o original) só quando chamados.
//! - `IConfigurationValue` é o trait [`ConfigurationValue`]; o [`ConfigurationInspectValue`] o
//!   implementa e expõe `default`, `user`, `workspace`... como métodos do trait (`default` é `default_inspect`).
//!   `policy` devolve só o valor (`{ value }` no original).
//! - `UserSettings` (lê o `settings.json` pelo `IFileService` e observa mudanças) está em
//!   [`super::user_settings`].
//! - `ConfigurationModelParser#doParseRaw` era `protected` (as subclasses do workbench o trocam); aqui
//!   o parser é uma struct final.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ops::ControlFlow;
use std::rc::Rc;

use je_base::JsString;
use je_base::arrays;
use je_base::dynamic::{Dynamic, ObjectRef, Truthy};
use je_base::errors::{self, Error};
use je_base::js_string;
use je_base::json::{self, ParseError, ParseErrorCode, ParseOptions, Value, Visitor};
use je_base::map::{OrderedMap, ResourceMap};
use je_base::objects;
use je_base::types;
use je_base::uri::URI;

use super::registry::{ConfigurationScope, PropertyDictionary, PropertySchema, configuration_registry, is_override_property, override_identifiers_from_key};
use super::{
    ConfigurationChange, ConfigurationCompareResult, ConfigurationData, ConfigurationModelData, ConfigurationOverrides, ConfigurationUpdateOverrides, ConfigurationValue,
    Overrides, ConfigurationTarget, add_to_value_tree, get_configuration_value, remove_from_value_tree, to_values_tree,
};
use crate::js::{Key, for_in_entries, from_json, is_frozen, object_keys, set_property};
use crate::log::LogServiceRef;
use crate::workspace::Workspace;

/// `freeze(data)`: congela em profundidade o que ainda não está congelado.
fn freeze(data: Dynamic) -> Dynamic {
    if is_frozen(&data) { data } else { objects::deep_freeze(&data) }
}

fn empty_object() -> Dynamic {
    Dynamic::Object(ObjectRef::new())
}

/// O `raw` de um modelo: o dado cru antes do filtro, ou a lista de dados crus (e modelos) que a
/// mesclagem acumulou.
#[derive(Clone)]
pub enum Raw {
    Dictionary(Dynamic),
    List(Vec<RawItem>),
}

/// Um elemento de `raw`: um dicionário cru ou um modelo já pronto.
#[derive(Clone)]
pub enum RawItem {
    Dictionary(Dynamic),
    Model(ConfigurationModel),
}

/// `{ identifiers, value }` de `inspect().overrides`.
#[derive(Clone, Debug)]
pub struct OverrideValue {
    pub identifiers: Vec<JsString>,
    pub value: Dynamic,
}

/// O que `ConfigurationModel#inspect` devolve (`IInspectValue` mais `merged`). Os quatro campos são
/// métodos porque, no original, são `getter`s que calculam e congelam a cada leitura.
#[derive(Clone)]
pub struct InspectValue {
    model: ConfigurationModel,
    section: Option<JsString>,
    override_identifier: Option<JsString>,
}

impl InspectValue {
    pub fn value(&self) -> Dynamic {
        freeze(self.model.raw_configuration().get_value_opt(self.section.as_ref()))
    }

    /// `override`.
    pub fn override_value(&self) -> Dynamic {
        match &self.override_identifier {
            Some(identifier) => freeze(self.model.raw_configuration().get_override_value_opt(self.section.as_ref(), identifier)),
            None => Dynamic::Undefined,
        }
    }

    pub fn merged(&self) -> Dynamic {
        let raw = self.model.raw_configuration();
        freeze(match &self.override_identifier {
            Some(identifier) => raw.override_for(identifier).get_value_opt(self.section.as_ref()),
            None => raw.get_value_opt(self.section.as_ref()),
        })
    }

    pub fn overrides(&self) -> Option<Vec<OverrideValue>> {
        let mut overrides = Vec::new();
        for Overrides { contents, identifiers, keys } in self.model.raw_configuration().overrides() {
            let value = ConfigurationModel::new(contents, keys, Vec::new(), None, self.model.0.log.clone()).get_value_opt(self.section.as_ref());
            if !value.is_undefined() {
                overrides.push(OverrideValue { identifiers, value });
            }
        }
        if overrides.is_empty() {
            return None;
        }
        for item in &overrides {
            freeze(item.value.clone());
        }
        Some(overrides)
    }

    /// O objeto `{ value, override, merged, overrides }` (o que `deepStrictEqual` enxerga).
    pub fn to_dynamic(&self) -> Dynamic {
        Dynamic::object([
            ("value", self.value()),
            ("override", self.override_value()),
            ("merged", self.merged()),
            ("overrides", overrides_to_dynamic(self.overrides())),
        ])
    }

    fn is_configured(&self) -> bool {
        !self.value().is_undefined() || !self.override_value().is_undefined() || self.overrides().is_some()
    }
}

/// `overrides` de um `inspect` como valor dinâmico (`undefined` se não há).
pub fn overrides_to_dynamic(overrides: Option<Vec<OverrideValue>>) -> Dynamic {
    match overrides {
        None => Dynamic::Undefined,
        Some(overrides) => Dynamic::array(overrides.into_iter().map(|o| Dynamic::object([("identifiers", super::strings_to_dynamic(&o.identifiers)), ("value", o.value)]))),
    }
}

enum OverrideCache {
    SelfModel,
    Model(ConfigurationModel),
}

struct ModelInner {
    contents: Dynamic,
    keys: RefCell<Vec<JsString>>,
    overrides: RefCell<Vec<Overrides>>,
    raw: Option<Raw>,
    log: LogServiceRef,
    override_configurations: RefCell<HashMap<JsString, OverrideCache>>,
    raw_configuration: RefCell<Option<ConfigurationModel>>,
}

/// `ConfigurationModel`.
#[derive(Clone)]
pub struct ConfigurationModel(Rc<ModelInner>);

impl ConfigurationModel {
    pub fn new(contents: Dynamic, keys: Vec<JsString>, overrides: Vec<Overrides>, raw: Option<Raw>, log: LogServiceRef) -> Self {
        Self(Rc::new(ModelInner {
            contents,
            keys: RefCell::new(keys),
            overrides: RefCell::new(overrides),
            raw,
            log,
            override_configurations: RefCell::new(HashMap::new()),
            raw_configuration: RefCell::new(None),
        }))
    }

    /// `ConfigurationModel.createEmptyModel`.
    pub fn create_empty_model(log: LogServiceRef) -> Self {
        Self::new(empty_object(), Vec::new(), Vec::new(), None, log)
    }

    pub fn ptr_eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }

    pub fn log(&self) -> LogServiceRef {
        self.0.log.clone()
    }

    /// `rawConfiguration`: o modelo do dado cru (sem o filtro do parser), ou o próprio modelo.
    pub fn raw_configuration(&self) -> ConfigurationModel {
        if let Some(cached) = self.0.raw_configuration.borrow().as_ref() {
            return cached.clone();
        }
        let Some(raw) = &self.0.raw else {
            // raw is same as current
            return self.clone();
        };
        let items: Vec<RawItem> = match raw {
            Raw::List(items) => items.clone(),
            Raw::Dictionary(dictionary) => vec![RawItem::Dictionary(dictionary.clone())],
        };
        let models: Vec<ConfigurationModel> = items
            .into_iter()
            .map(|item| match item {
                RawItem::Model(model) => model,
                RawItem::Dictionary(dictionary) => {
                    let mut parser = ConfigurationModelParser::new("", self.0.log.clone());
                    parser.parse_raw(&dictionary, None);
                    parser.configuration_model()
                }
            })
            .collect();
        let Some(first) = models.first().cloned() else {
            errors::throw(Error::type_error("Cannot read properties of undefined"));
        };
        let result = models.into_iter().fold(first, |previous, current| if current.ptr_eq(&previous) { current } else { previous.merge(&[current]) });
        *self.0.raw_configuration.borrow_mut() = Some(result.clone());
        result
    }

    /// `contents`.
    pub fn contents(&self) -> Dynamic {
        self.0.contents.clone()
    }

    /// `overrides` (os blocos compartilham `contents` com o modelo, como no original).
    pub fn overrides(&self) -> Vec<Overrides> {
        self.0.overrides.borrow().clone()
    }

    /// `keys`.
    pub fn keys(&self) -> Vec<JsString> {
        self.0.keys.borrow().clone()
    }

    /// `raw`: `undefined` quando o modelo é o próprio dado ou só reúne modelos.
    pub fn raw(&self) -> Option<Raw> {
        let raw = self.0.raw.as_ref()?;
        if let Raw::List(items) = raw
            && items.iter().all(|item| matches!(item, RawItem::Model(_)))
        {
            return None;
        }
        Some(raw.clone())
    }

    pub fn is_empty(&self) -> bool {
        self.0.keys.borrow().is_empty() && self.0.contents.own_keys().is_empty() && self.0.overrides.borrow().is_empty()
    }

    /// `getValue(section)`: `section` vazio ou ausente devolve todo o conteúdo.
    pub fn get_value(&self, section: impl Key) -> Dynamic {
        self.get_value_opt(Some(&section.to_js()))
    }

    /// `getValue(section)` com a seção opcional.
    pub fn get_value_opt(&self, section: Option<&JsString>) -> Dynamic {
        match section {
            Some(section) if !section.is_empty() => get_configuration_value(&self.0.contents, section, None),
            _ => self.0.contents.clone(),
        }
    }

    /// `inspect(section, overrideIdentifier)`.
    pub fn inspect(&self, section: Option<&JsString>, override_identifier: Option<&JsString>) -> InspectValue {
        InspectValue {
            model: self.clone(),
            section: section.cloned(),
            override_identifier: override_identifier.filter(|i| !i.is_empty()).cloned(),
        }
    }

    /// `getOverrideValue(section, overrideIdentifier)`.
    pub fn get_override_value(&self, section: Option<&str>, override_identifier: impl Key) -> Dynamic {
        self.get_override_value_opt(section.map(JsString::from).as_ref(), &override_identifier.to_js())
    }

    fn get_override_value_opt(&self, section: Option<&JsString>, override_identifier: &JsString) -> Dynamic {
        match self.get_contents_for_override_identifer(override_identifier) {
            Some(override_contents) => match section {
                Some(section) if !section.is_empty() => get_configuration_value(&override_contents, section, None),
                _ => override_contents,
            },
            None => Dynamic::Undefined,
        }
    }

    /// `getKeysForOverrideIdentifier`.
    pub fn get_keys_for_override_identifier(&self, identifier: impl Key) -> Vec<JsString> {
        let identifier = identifier.to_js();
        let mut keys = Vec::new();
        for override_ in self.0.overrides.borrow().iter() {
            if override_.identifiers.contains(&identifier) {
                keys.extend(override_.keys.iter().cloned());
            }
        }
        arrays::distinct(&keys, Clone::clone)
    }

    /// `getAllOverrideIdentifiers`.
    pub fn get_all_override_identifiers(&self) -> Vec<JsString> {
        let mut result = Vec::new();
        for override_ in self.0.overrides.borrow().iter() {
            result.extend(override_.identifiers.iter().cloned());
        }
        arrays::distinct(&result, Clone::clone)
    }

    /// `override(identifier)`: o modelo com o bloco `[identifier]` aplicado sobre o conteúdo.
    pub fn override_for(&self, identifier: impl Key) -> ConfigurationModel {
        let identifier = identifier.to_js();
        match self.0.override_configurations.borrow().get(&identifier) {
            Some(OverrideCache::SelfModel) => return self.clone(),
            Some(OverrideCache::Model(model)) => return model.clone(),
            None => {}
        }
        let model = self.create_override_configuration_model(&identifier);
        let entry = if model.ptr_eq(self) { OverrideCache::SelfModel } else { OverrideCache::Model(model.clone()) };
        self.0.override_configurations.borrow_mut().insert(identifier, entry);
        model
    }

    /// `merge(...others)`.
    pub fn merge(&self, others: &[ConfigurationModel]) -> ConfigurationModel {
        let contents = objects::deep_clone(&self.0.contents);
        let mut overrides: Vec<Overrides> = self.0.overrides.borrow().iter().map(Overrides::deep_clone).collect();
        let mut keys = self.keys();
        let mut raws = Self::raw_items_or_self(self);

        for other in others {
            raws.extend(Self::raw_items_or_self(other));
            if other.is_empty() {
                continue;
            }
            self.merge_contents(&contents, &other.0.contents);

            for other_override in other.overrides() {
                match overrides.iter_mut().find(|o| o.identifiers == other_override.identifiers) {
                    Some(override_) => {
                        self.merge_contents(&override_.contents, &other_override.contents);
                        override_.keys.extend(other_override.keys.iter().cloned());
                        override_.keys = arrays::distinct(&override_.keys, Clone::clone);
                    }
                    None => overrides.push(other_override.deep_clone()),
                }
            }
            for key in other.keys() {
                if !keys.contains(&key) {
                    keys.push(key);
                }
            }
        }
        let raw = if raws.is_empty() || raws.iter().all(|r| matches!(r, RawItem::Model(_))) { None } else { Some(Raw::List(raws)) };
        ConfigurationModel::new(contents, keys, overrides, raw, self.0.log.clone())
    }

    /// `other._raw ? (Array.isArray ? other._raw : [other._raw]) : [other]`.
    fn raw_items_or_self(model: &ConfigurationModel) -> Vec<RawItem> {
        match &model.0.raw {
            Some(Raw::List(items)) => items.clone(),
            Some(Raw::Dictionary(dictionary)) => vec![RawItem::Dictionary(dictionary.clone())],
            None => vec![RawItem::Model(model.clone())],
        }
    }

    fn create_override_configuration_model(&self, identifier: &JsString) -> ConfigurationModel {
        let override_contents = self.get_contents_for_override_identifer(identifier);

        let Some(override_contents) = override_contents.filter(|c| c.is_truthy() && c.type_of() == "object" && !object_keys(c).is_empty()) else {
            // If there are no valid overrides, return self
            return self.clone();
        };

        let contents = ObjectRef::new();
        let mut all_keys = object_keys(&self.0.contents);
        all_keys.extend(object_keys(&override_contents));
        for key in arrays::distinct(&all_keys, Clone::clone) {
            let mut contents_for_key = self.0.contents.get(&key);
            let override_contents_for_key = override_contents.get(&key);

            // If there are override contents for the key, clone and merge otherwise use base contents
            if override_contents_for_key.is_truthy() {
                // Clone and merge only if base contents and override contents are of type object otherwise just override
                if contents_for_key.type_of() == "object" && override_contents_for_key.type_of() == "object" {
                    contents_for_key = objects::deep_clone(&contents_for_key);
                    self.merge_contents(&contents_for_key, &override_contents_for_key);
                } else {
                    contents_for_key = override_contents_for_key;
                }
            }

            contents.set(&key, contents_for_key);
        }

        ConfigurationModel::new(Dynamic::Object(contents), self.keys(), self.overrides(), None, self.0.log.clone())
    }

    fn merge_contents(&self, source: &Dynamic, target: &Dynamic) {
        for key in object_keys(target) {
            if source.has_property(&key) {
                let (source_value, target_value) = (source.get(&key), target.get(&key));
                if types::is_object(&source_value) && types::is_object(&target_value) {
                    self.merge_contents(&source_value, &target_value);
                    continue;
                }
            }
            set_property(source, &key, objects::deep_clone(&target.get(&key)));
        }
    }

    fn get_contents_for_override_identifer(&self, identifier: &JsString) -> Option<Dynamic> {
        let mut contents_for_identifier_only: Option<Dynamic> = None;
        let mut contents: Option<Dynamic> = None;
        let mut merge_contents = |contents_to_merge: &Dynamic| {
            if contents_to_merge.is_truthy() {
                match &contents {
                    Some(existing) => self.merge_contents(existing, contents_to_merge),
                    None => contents = Some(objects::deep_clone(contents_to_merge)),
                }
            }
        };
        let overrides = self.overrides();
        for override_ in &overrides {
            if override_.identifiers.len() == 1 && &override_.identifiers[0] == identifier {
                contents_for_identifier_only = Some(override_.contents.clone());
            } else if override_.identifiers.contains(identifier) {
                merge_contents(&override_.contents);
            }
        }
        // Merge contents of the identifier only at the end to take precedence.
        if let Some(only) = &contents_for_identifier_only {
            merge_contents(only);
        }
        contents
    }

    /// `toJSON()`.
    pub fn to_json(&self) -> ConfigurationModelData {
        ConfigurationModelData { contents: self.contents(), keys: self.keys(), overrides: self.overrides(), raw: None }
    }

    // Update methods

    /// `addValue(key, value)`.
    pub fn add_value(&self, key: impl Key, value: Dynamic) {
        self.update_value(&key.to_js(), value, true);
    }

    /// `setValue(key, value)`.
    pub fn set_value(&self, key: impl Key, value: Dynamic) {
        self.update_value(&key.to_js(), value, false);
    }

    /// `removeValue(key)`.
    pub fn remove_value(&self, key: impl Key) {
        let key = key.to_js();
        let index = self.0.keys.borrow().iter().position(|k| *k == key);
        let Some(index) = index else {
            return;
        };
        self.0.keys.borrow_mut().remove(index);
        remove_from_value_tree(&self.0.contents, &key);
        if is_override_property(&key) {
            let identifiers = override_identifiers_from_key(&key);
            let position = self.0.overrides.borrow().iter().position(|o| o.identifiers == identifiers);
            // `splice(-1, 1)` do original remove o último quando não acha.
            let mut overrides = self.0.overrides.borrow_mut();
            match position {
                Some(position) => {
                    overrides.remove(position);
                }
                None => {
                    overrides.pop();
                }
            }
        }
    }

    fn update_value(&self, key: &JsString, value: Dynamic, add: bool) {
        let log = self.0.log.clone();
        add_to_value_tree(&self.0.contents, key, value, &|e| log.error(e));
        let add = add || !self.0.keys.borrow().contains(key);
        if add {
            self.0.keys.borrow_mut().push(key.clone());
        }
        if is_override_property(key) {
            let override_contents = self.0.contents.get(key);
            let identifiers = override_identifiers_from_key(key);
            let log = self.0.log.clone();
            let override_ = Overrides {
                identifiers: identifiers.clone(),
                keys: object_keys(&override_contents),
                contents: to_values_tree(&override_contents, &|message| log.error(message)),
            };
            let mut overrides = self.0.overrides.borrow_mut();
            match overrides.iter().position(|o| o.identifiers == identifiers) {
                Some(index) => overrides[index] = override_,
                None => overrides.push(override_),
            }
        }
    }
}

/// `ConfigurationParseOptions`.
#[derive(Clone, Debug, Default)]
pub struct ConfigurationParseOptions {
    pub skip_unregistered: bool,
    pub scopes: Option<Vec<ConfigurationScope>>,
    pub skip_restricted: bool,
    pub include: Option<Vec<JsString>>,
    pub exclude: Option<Vec<JsString>>,
}

struct FilterResult {
    raw: Dynamic,
    restricted: Vec<JsString>,
    has_excluded_properties: bool,
}

struct FilterPropertyResult {
    included: bool,
    value: Dynamic,
    restricted: Vec<JsString>,
    has_excluded_properties: bool,
}

/// O `JSONVisitor` do `doParseContent`: monta os valores como o original, pendurando cada contêiner no
/// pai assim que ele abre (um texto truncado ainda deixa o que já foi lido).
struct ContentVisitor {
    current_property: Option<JsString>,
    current_parent: Dynamic,
    previous_parents: Vec<Dynamic>,
    errors: Vec<ParseError>,
}

impl ContentVisitor {
    fn on_value(&self, value: Dynamic) {
        match &self.current_parent {
            Dynamic::Array(array) => array.push(value),
            parent => {
                if let Some(property) = &self.current_property {
                    set_property(parent, property, value);
                }
            }
        }
    }
}

impl Visitor for ContentVisitor {
    fn on_object_begin(&mut self, _offset: usize, _length: usize) -> ControlFlow<()> {
        let object = empty_object();
        self.on_value(object.clone());
        self.previous_parents.push(std::mem::replace(&mut self.current_parent, object));
        self.current_property = None;
        ControlFlow::Continue(())
    }

    fn on_object_property(&mut self, property: &JsString, _offset: usize, _length: usize) -> ControlFlow<()> {
        self.current_property = Some(property.clone());
        ControlFlow::Continue(())
    }

    fn on_object_end(&mut self, _offset: usize, _length: usize) -> ControlFlow<()> {
        if let Some(parent) = self.previous_parents.pop() {
            self.current_parent = parent;
        }
        ControlFlow::Continue(())
    }

    fn on_array_begin(&mut self, _offset: usize, _length: usize) -> ControlFlow<()> {
        let array = Dynamic::array([]);
        self.on_value(array.clone());
        self.previous_parents.push(std::mem::replace(&mut self.current_parent, array));
        self.current_property = None;
        ControlFlow::Continue(())
    }

    fn on_array_end(&mut self, _offset: usize, _length: usize) -> ControlFlow<()> {
        if let Some(parent) = self.previous_parents.pop() {
            self.current_parent = parent;
        }
        ControlFlow::Continue(())
    }

    fn on_literal_value(&mut self, value: &Value, _offset: usize, _length: usize) -> ControlFlow<()> {
        self.on_value(from_json(value));
        ControlFlow::Continue(())
    }

    fn on_error(&mut self, error: ParseErrorCode, offset: usize, length: usize) -> ControlFlow<()> {
        self.errors.push(ParseError { error, offset, length });
        ControlFlow::Continue(())
    }
}

/// O que `ConfigurationModelParser#doParseRaw` devolve: o conteúdo, as chaves e os blocos de linguagem do modelo,
/// mais as chaves restritas e se alguma propriedade foi excluída pelo filtro.
pub struct ParsedRaw {
    pub contents: Dynamic,
    pub keys: Vec<JsString>,
    pub overrides: Vec<Overrides>,
    pub restricted: Vec<JsString>,
    pub has_excluded_properties: bool,
}

/// `doParseRaw` era `protected` no original: as subclasses do workbench (`WorkspaceConfigurationModelParser` e
/// `StandaloneConfigurationModelParser`) o trocam. Aqui a subclasse é um objeto que o parser consulta no lugar do
/// corpo padrão; ela chama o corpo do original (o `super.doParseRaw`) com [`ConfigurationModelParser::do_parse_raw_base`].
pub trait ParseRawOverride {
    fn do_parse_raw(&self, parser: &ConfigurationModelParser, raw: &Dynamic, options: Option<&ConfigurationParseOptions>) -> ParsedRaw;
}

/// `ConfigurationModelParser`.
pub struct ConfigurationModelParser {
    name: String,
    log: LogServiceRef,
    raw: Option<Dynamic>,
    configuration_model: Option<ConfigurationModel>,
    restricted_configurations: Vec<JsString>,
    parse_errors: Vec<ParseError>,
    syntax_errors: Vec<ParseError>,
    parse_raw_override: Option<Rc<dyn ParseRawOverride>>,
}

impl ConfigurationModelParser {
    pub fn new(name: impl Into<String>, log: LogServiceRef) -> Self {
        Self {
            name: name.into(),
            log,
            raw: None,
            configuration_model: None,
            restricted_configurations: Vec::new(),
            parse_errors: Vec::new(),
            syntax_errors: Vec::new(),
            parse_raw_override: None,
        }
    }

    /// Um parser cuja subclasse troca o `doParseRaw` (veja [`ParseRawOverride`]).
    pub fn with_override(name: impl Into<String>, log: LogServiceRef, parse_raw_override: Rc<dyn ParseRawOverride>) -> Self {
        let mut parser = Self::new(name, log);
        parser.parse_raw_override = Some(parse_raw_override);
        parser
    }

    /// `_name`.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// `logService`.
    pub fn log(&self) -> LogServiceRef {
        self.log.clone()
    }

    /// `_raw`: o último dado cru lido.
    pub fn raw_content(&self) -> Option<Dynamic> {
        self.raw.clone()
    }

    /// `configurationModel`: o último modelo lido, ou um modelo vazio.
    pub fn configuration_model(&self) -> ConfigurationModel {
        self.configuration_model.clone().unwrap_or_else(|| ConfigurationModel::create_empty_model(self.log.clone()))
    }

    pub fn restricted_configurations(&self) -> &[JsString] {
        &self.restricted_configurations
    }

    /// `errors`: no original só recebe o erro que o `visit` lançar, e o `visit` não lança; por isso fica
    /// vazio mesmo com JSON malformado.
    pub fn errors(&self) -> &[ParseError] {
        &self.parse_errors
    }

    /// Os erros de sintaxe que o `visit` do último texto relatou. NÃO existe no original (ele junta
    /// estes erros num `Array` local e os descarta); fica aqui para quem quer saber se o texto era
    /// JSON válido.
    pub fn syntax_errors(&self) -> &[ParseError] {
        &self.syntax_errors
    }

    /// `parse(content, options)`; `None` (`null` ou `undefined`) não lê nada.
    pub fn parse(&mut self, content: Option<&str>, options: Option<&ConfigurationParseOptions>) {
        self.parse_js(content.map(JsString::from).as_ref(), options);
    }

    /// `parse(content, options)` com o texto em UTF-16.
    pub fn parse_js(&mut self, content: Option<&JsString>, options: Option<&ConfigurationParseOptions>) {
        if let Some(content) = content {
            self.syntax_errors.clear();
            let raw = self.do_parse_content(content);
            self.parse_raw(&raw, options);
        }
    }

    /// `reparse(options)`.
    pub fn reparse(&mut self, options: &ConfigurationParseOptions) {
        if let Some(raw) = self.raw.clone().filter(Truthy::is_truthy) {
            self.parse_raw(&raw, Some(options));
        }
    }

    /// `parseRaw(raw, options)`.
    pub fn parse_raw(&mut self, raw: &Dynamic, options: Option<&ConfigurationParseOptions>) {
        self.raw = Some(raw.clone());
        let ParsedRaw { contents, keys, overrides, restricted, has_excluded_properties } = match self.parse_raw_override.clone() {
            Some(parse_raw_override) => parse_raw_override.do_parse_raw(self, raw, options),
            None => self.do_parse_raw_base(raw, options),
        };
        // `raw has not changed` quando nada foi excluído.
        let model_raw = has_excluded_properties.then(|| Raw::List(vec![RawItem::Dictionary(raw.clone())]));
        self.configuration_model = Some(ConfigurationModel::new(contents, keys, overrides, model_raw, self.log.clone()));
        self.restricted_configurations = restricted;
    }

    fn do_parse_content(&mut self, content: &JsString) -> Dynamic {
        let mut raw = empty_object();
        if content.is_empty() {
            return raw;
        }
        let mut visitor = ContentVisitor { current_property: None, current_parent: Dynamic::array([]), previous_parents: Vec::new(), errors: Vec::new() };
        json::visit(content.units(), &mut visitor, ParseOptions::default());
        self.syntax_errors = std::mem::take(&mut visitor.errors);
        let first = visitor.current_parent.get(&JsString::from("0"));
        if first.is_truthy() {
            raw = first;
        }
        raw
    }

    /// O corpo de `doParseRaw` da classe base (o `super.doParseRaw` das subclasses).
    pub fn do_parse_raw_base(&self, raw: &Dynamic, options: Option<&ConfigurationParseOptions>) -> ParsedRaw {
        let registry = configuration_registry();
        let configuration_properties = registry.get_configuration_properties();
        let excluded_configuration_properties = registry.get_excluded_configuration_properties();
        let filtered = self.filter(raw, &configuration_properties, &excluded_configuration_properties, true, options);
        let raw = filtered.raw;
        let conflict = |message: &str| self.log.error(&format!("Conflict in settings file {}: {message}", self.name));
        let contents = to_values_tree(&raw, &conflict);
        let keys = object_keys(&raw);
        let overrides = Self::to_overrides(&raw, &conflict);
        ParsedRaw { contents, keys, overrides, restricted: filtered.restricted, has_excluded_properties: filtered.has_excluded_properties }
    }

    /// `toValuesTree` de um dado cru com as mensagens de conflito do nome deste parser.
    pub fn to_values_tree_reporting_conflicts(&self, raw: &Dynamic) -> Dynamic {
        let name = self.name.clone();
        to_values_tree(raw, &move |message| eprintln!("Conflict in settings file {name}: {message}"))
    }

    fn filter(
        &self,
        properties: &Dynamic,
        configuration_properties: &PropertyDictionary,
        excluded_configuration_properties: &PropertyDictionary,
        filter_overridden_properties: bool,
        options: Option<&ConfigurationParseOptions>,
    ) -> FilterResult {
        let mut has_excluded_properties = false;
        let has_exclude = options.is_some_and(|o| o.exclude.as_ref().is_some_and(|e| !e.is_empty()));
        let Some(options) = options.filter(|o| o.scopes.is_some() || o.skip_restricted || o.skip_unregistered || has_exclude) else {
            return FilterResult { raw: properties.clone(), restricted: Vec::new(), has_excluded_properties };
        };
        let raw = empty_object();
        let mut restricted = Vec::new();
        for (key, value) in for_in_entries(properties) {
            if is_override_property(&key) && filter_overridden_properties {
                let result = self.filter(&value, configuration_properties, excluded_configuration_properties, false, Some(options));
                set_property(&raw, &key, result.raw);
                has_excluded_properties = has_excluded_properties || result.has_excluded_properties;
                restricted.extend(result.restricted);
            } else {
                let result = self.filter_property(&key, &value, configuration_properties, excluded_configuration_properties, options);
                restricted.extend(result.restricted);
                has_excluded_properties = has_excluded_properties || result.has_excluded_properties;
                if result.included {
                    set_property(&raw, &key, result.value);
                } else {
                    has_excluded_properties = true;
                }
            }
        }
        FilterResult { raw, restricted, has_excluded_properties }
    }

    fn filter_property(
        &self,
        path: &JsString,
        value: &Dynamic,
        configuration_properties: &PropertyDictionary,
        excluded_configuration_properties: &PropertyDictionary,
        options: &ConfigurationParseOptions,
    ) -> FilterPropertyResult {
        let property_schema = configuration_properties.get(path);
        let excluded_property_schema = excluded_configuration_properties.get(path);
        if let Some(schema) = &property_schema {
            return FilterPropertyResult {
                included: Self::should_include(path, property_schema.as_ref(), excluded_property_schema.as_ref(), options),
                value: value.clone(),
                restricted: if schema.restricted() { vec![path.clone()] } else { Vec::new() },
                has_excluded_properties: false,
            };
        }
        let leaf = |included: bool| FilterPropertyResult { included, value: value.clone(), restricted: Vec::new(), has_excluded_properties: false };
        if !types::is_object(value) || value.is_array() {
            return leaf(Self::should_include(path, property_schema.as_ref(), excluded_property_schema.as_ref(), options));
        }
        // Excluded properties can themselves be object-typed (e.g. `mcp.enterpriseManagedAuth.idp`), so treat them as a leaf too.
        if excluded_property_schema.is_some() {
            return leaf(Self::should_include(path, property_schema.as_ref(), excluded_property_schema.as_ref(), options));
        }
        // If the whole object is explicitly excluded, do not recurse into its children.
        if options.exclude.as_ref().is_some_and(|e| e.contains(path)) {
            return leaf(false);
        }
        let keys = object_keys(value);
        if keys.is_empty() {
            return leaf(Self::should_include(path, property_schema.as_ref(), excluded_property_schema.as_ref(), options));
        }
        let nested = empty_object();
        let mut restricted = Vec::new();
        let mut included = false;
        let mut has_excluded_properties = false;
        for key in keys {
            let result = self.filter_property(
                &JsString::from(format!("{path}.{key}")),
                &value.get(&key),
                configuration_properties,
                excluded_configuration_properties,
                options,
            );
            restricted.extend(result.restricted);
            has_excluded_properties = has_excluded_properties || result.has_excluded_properties;
            if result.included {
                set_property(&nested, &key, result.value);
                included = true;
            } else {
                has_excluded_properties = true;
            }
        }
        FilterPropertyResult { included, value: nested, restricted, has_excluded_properties }
    }

    fn should_include(key: &JsString, property_schema: Option<&PropertySchema>, excluded_property_schema: Option<&PropertySchema>, options: &ConfigurationParseOptions) -> bool {
        if options.exclude.as_ref().is_some_and(|e| e.contains(key)) {
            return false;
        }
        if options.include.as_ref().is_some_and(|i| i.contains(key)) {
            return true;
        }
        if options.skip_restricted && property_schema.is_some_and(PropertySchema::restricted) {
            return false;
        }
        if options.skip_unregistered && property_schema.is_none() {
            return false;
        }
        let schema = property_schema.or(excluded_property_schema);
        let scope = schema.map(|schema| schema.scope().unwrap_or(ConfigurationScope::Window));
        match (scope, &options.scopes) {
            (Some(scope), Some(scopes)) => scopes.contains(&scope),
            _ => true,
        }
    }

    fn to_overrides(raw: &Dynamic, conflict_reporter: &dyn Fn(&str)) -> Vec<Overrides> {
        let mut overrides = Vec::new();
        for key in object_keys(raw) {
            if is_override_property(&key) {
                let override_raw = ObjectRef::new();
                let raw_key = raw.get(&key);
                for (key_in_override_raw, value) in for_in_entries(&raw_key) {
                    override_raw.set(&key_in_override_raw, value);
                }
                let override_raw = Dynamic::Object(override_raw);
                overrides.push(Overrides {
                    identifiers: override_identifiers_from_key(&key),
                    keys: object_keys(&override_raw),
                    contents: to_values_tree(&override_raw, conflict_reporter),
                });
            }
        }
        overrides
    }
}

/// `ConfigurationInspectValue`: o valor de uma chave em cada camada.
///
/// Os campos são públicos para quem simula o `IConfigurationValue` (o objeto literal que os testes do
/// `textResourceConfigurationService` devolvem de `inspect`) montar o valor a partir de modelos por camada.
pub struct ConfigurationInspectValue {
    pub key: JsString,
    pub overrides: ConfigurationOverrides,
    pub value: Dynamic,
    pub override_identifiers: Option<Vec<JsString>>,
    pub default_configuration: ConfigurationModel,
    pub policy_configuration: Option<ConfigurationModel>,
    pub application_configuration: Option<ConfigurationModel>,
    pub user_configuration: ConfigurationModel,
    pub local_user_configuration: ConfigurationModel,
    pub remote_user_configuration: ConfigurationModel,
    pub workspace_configuration: Option<ConfigurationModel>,
    pub folder_configuration_model: Option<ConfigurationModel>,
    pub memory_configuration_model: ConfigurationModel,
}

impl ConfigurationInspectValue {
    fn inspect_in(&self, model: &ConfigurationModel, with_override: bool) -> InspectValue {
        model.inspect(Some(&self.key), if with_override { self.overrides.override_identifier.as_ref() } else { None })
    }

    fn inspect_optional(&self, model: Option<&ConfigurationModel>, with_override: bool) -> Option<InspectValue> {
        model.map(|m| self.inspect_in(m, with_override))
    }

    /// `toInspectValue`: o `inspect` só se tem valor, `override` ou `overrides`.
    fn to_inspect_value(inspect_value: Option<InspectValue>) -> Option<InspectValue> {
        inspect_value.filter(InspectValue::is_configured)
    }
}

impl ConfigurationValue for ConfigurationInspectValue {
    fn default_inspect(&self) -> Option<InspectValue> {
        Self::to_inspect_value(Some(self.inspect_in(&self.default_configuration, true)))
    }

    /// `policy`: só o valor (`{ value }` no original), se definido.
    fn policy(&self) -> Option<Dynamic> {
        let value = self.inspect_optional(self.policy_configuration.as_ref(), false)?.value();
        (!value.is_undefined()).then_some(value)
    }

    fn application(&self) -> Option<InspectValue> {
        Self::to_inspect_value(self.inspect_optional(self.application_configuration.as_ref(), false))
    }

    fn user(&self) -> Option<InspectValue> {
        Self::to_inspect_value(Some(self.inspect_in(&self.user_configuration, true)))
    }

    fn user_local(&self) -> Option<InspectValue> {
        Self::to_inspect_value(Some(self.inspect_in(&self.local_user_configuration, true)))
    }

    fn user_remote(&self) -> Option<InspectValue> {
        Self::to_inspect_value(Some(self.inspect_in(&self.remote_user_configuration, true)))
    }

    fn workspace(&self) -> Option<InspectValue> {
        Self::to_inspect_value(self.inspect_optional(self.workspace_configuration.as_ref(), true))
    }

    fn workspace_folder(&self) -> Option<InspectValue> {
        Self::to_inspect_value(self.inspect_optional(self.folder_configuration_model.as_ref(), true))
    }

    fn memory(&self) -> Option<InspectValue> {
        Self::to_inspect_value(Some(self.inspect_in(&self.memory_configuration_model, true)))
    }

    fn default_value(&self) -> Dynamic {
        self.inspect_in(&self.default_configuration, true).merged()
    }

    fn application_value(&self) -> Dynamic {
        self.inspect_optional(self.application_configuration.as_ref(), false).map_or(Dynamic::Undefined, |i| i.merged())
    }

    fn user_value(&self) -> Dynamic {
        self.inspect_in(&self.user_configuration, true).merged()
    }

    fn user_local_value(&self) -> Dynamic {
        self.inspect_in(&self.local_user_configuration, true).merged()
    }

    fn user_remote_value(&self) -> Dynamic {
        self.inspect_in(&self.remote_user_configuration, true).merged()
    }

    fn workspace_value(&self) -> Dynamic {
        self.inspect_optional(self.workspace_configuration.as_ref(), true).map_or(Dynamic::Undefined, |i| i.merged())
    }

    fn workspace_folder_value(&self) -> Dynamic {
        self.inspect_optional(self.folder_configuration_model.as_ref(), true).map_or(Dynamic::Undefined, |i| i.merged())
    }

    fn memory_value(&self) -> Dynamic {
        self.inspect_in(&self.memory_configuration_model, true).merged()
    }

    fn policy_value(&self) -> Dynamic {
        self.inspect_optional(self.policy_configuration.as_ref(), false).map_or(Dynamic::Undefined, |i| i.merged())
    }

    fn value(&self) -> Dynamic {
        freeze(self.value.clone())
    }

    fn override_identifiers(&self) -> Option<Vec<JsString>> {
        self.override_identifiers.clone()
    }
}

/// O retorno de `Configuration#keys`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ConfigurationKeys {
    pub default: Vec<JsString>,
    pub policy: Vec<JsString>,
    pub user: Vec<JsString>,
    pub workspace: Vec<JsString>,
    pub workspace_folder: Vec<JsString>,
}

/// `Configuration`: as camadas default, política, aplicação, usuário (local e remoto), workspace, pasta
/// e memória. Todos os métodos recebem `&self`, e as camadas e caches ficam em células, porque o
/// objeto é compartilhado (o evento de mudança guarda a configuração viva).
pub struct Configuration {
    default_configuration: RefCell<ConfigurationModel>,
    policy_configuration: RefCell<ConfigurationModel>,
    application_configuration: RefCell<ConfigurationModel>,
    local_user_configuration: RefCell<ConfigurationModel>,
    remote_user_configuration: RefCell<ConfigurationModel>,
    workspace_configuration: RefCell<ConfigurationModel>,
    folder_configurations: RefCell<ResourceMap<ConfigurationModel>>,
    memory_configuration: ConfigurationModel,
    memory_configuration_by_resource: RefCell<ResourceMap<ConfigurationModel>>,
    log: LogServiceRef,
    workspace_consolidated_configuration: RefCell<Option<ConfigurationModel>>,
    folders_consolidated_configurations: RefCell<ResourceMap<ConfigurationModel>>,
    user_configuration: RefCell<Option<ConfigurationModel>>,
}

impl Configuration {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        default_configuration: ConfigurationModel,
        policy_configuration: ConfigurationModel,
        application_configuration: ConfigurationModel,
        local_user_configuration: ConfigurationModel,
        remote_user_configuration: ConfigurationModel,
        workspace_configuration: ConfigurationModel,
        folder_configurations: ResourceMap<ConfigurationModel>,
        memory_configuration: ConfigurationModel,
        memory_configuration_by_resource: ResourceMap<ConfigurationModel>,
        log: LogServiceRef,
    ) -> Self {
        Self {
            default_configuration: RefCell::new(default_configuration),
            policy_configuration: RefCell::new(policy_configuration),
            application_configuration: RefCell::new(application_configuration),
            local_user_configuration: RefCell::new(local_user_configuration),
            remote_user_configuration: RefCell::new(remote_user_configuration),
            workspace_configuration: RefCell::new(workspace_configuration),
            folder_configurations: RefCell::new(folder_configurations),
            memory_configuration,
            memory_configuration_by_resource: RefCell::new(memory_configuration_by_resource),
            log,
            workspace_consolidated_configuration: RefCell::new(None),
            folders_consolidated_configurations: RefCell::new(ResourceMap::new()),
            user_configuration: RefCell::new(None),
        }
    }

    /// `getValue(section, overrides, workspace)`.
    pub fn get_value(&self, section: Option<&JsString>, overrides: &ConfigurationOverrides, workspace: Option<&Workspace>) -> Dynamic {
        let consolidate_configuration_model = self.get_consolidated_configuration_model(section, overrides, workspace);
        consolidate_configuration_model.get_value_opt(section)
    }

    /// `updateValue(key, value, overrides)`: escreve na camada de memória (`undefined` remove a chave).
    pub fn update_value(&self, key: impl Key, value: Dynamic, overrides: &ConfigurationUpdateOverrides) {
        let key = key.to_js();
        let memory_configuration = match &overrides.resource {
            Some(resource) => {
                let existing = self.memory_configuration_by_resource.borrow().get(resource).cloned();
                existing.unwrap_or_else(|| {
                    let model = ConfigurationModel::create_empty_model(self.log.clone());
                    self.memory_configuration_by_resource.borrow_mut().set(resource.clone(), model.clone());
                    model
                })
            }
            None => self.memory_configuration.clone(),
        };

        if value.is_undefined() {
            memory_configuration.remove_value(&key);
        } else {
            memory_configuration.set_value(&key, value);
        }

        if overrides.resource.is_none() {
            *self.workspace_consolidated_configuration.borrow_mut() = None;
        }
    }

    /// `inspect(key, overrides, workspace)`.
    pub fn inspect(&self, key: impl Key, overrides: &ConfigurationOverrides, workspace: Option<&Workspace>) -> ConfigurationInspectValue {
        let key = key.to_js();
        let consolidate_configuration_model = self.get_consolidated_configuration_model(Some(&key), overrides, workspace);
        let folder_configuration_model = self.get_folder_configuration_model_for_resource(overrides.resource.as_ref(), workspace);
        let memory_configuration_model = overrides
            .resource
            .as_ref()
            .and_then(|r| self.memory_configuration_by_resource.borrow().get(r).cloned())
            .unwrap_or_else(|| self.memory_configuration.clone());
        let mut override_identifiers: Vec<JsString> = Vec::new();
        for override_ in consolidate_configuration_model.overrides() {
            for override_identifier in &override_.identifiers {
                if !consolidate_configuration_model.get_override_value_opt(Some(&key), override_identifier).is_undefined() && !override_identifiers.contains(override_identifier) {
                    override_identifiers.push(override_identifier.clone());
                }
            }
        }

        let policy = self.policy_configuration.borrow().clone();
        let application = self.application_configuration();
        ConfigurationInspectValue {
            value: consolidate_configuration_model.get_value_opt(Some(&key)),
            overrides: overrides.clone(),
            override_identifiers: (!override_identifiers.is_empty()).then_some(override_identifiers),
            default_configuration: self.default_configuration.borrow().clone(),
            policy_configuration: (!policy.is_empty()).then_some(policy),
            application_configuration: (!application.is_empty()).then_some(application),
            user_configuration: self.user_configuration(),
            local_user_configuration: self.local_user_configuration(),
            remote_user_configuration: self.remote_user_configuration(),
            workspace_configuration: workspace.map(|_| self.workspace_configuration.borrow().clone()),
            folder_configuration_model,
            memory_configuration_model,
            key,
        }
    }

    /// `keys(workspace)`.
    pub fn keys(&self, workspace: Option<&Workspace>) -> ConfigurationKeys {
        let folder_configuration_model = self.get_folder_configuration_model_for_resource(None, workspace);
        ConfigurationKeys {
            default: self.default_configuration.borrow().keys(),
            policy: self.policy_configuration.borrow().keys(),
            user: self.user_configuration().keys(),
            workspace: self.workspace_configuration.borrow().keys(),
            workspace_folder: folder_configuration_model.map_or_else(Vec::new, |m| m.keys()),
        }
    }

    fn clear_consolidated(&self) {
        *self.workspace_consolidated_configuration.borrow_mut() = None;
        self.folders_consolidated_configurations.borrow_mut().clear();
    }

    pub fn update_default_configuration(&self, default_configuration: ConfigurationModel) {
        *self.default_configuration.borrow_mut() = default_configuration;
        self.clear_consolidated();
    }

    pub fn update_policy_configuration(&self, policy_configuration: ConfigurationModel) {
        *self.policy_configuration.borrow_mut() = policy_configuration;
    }

    pub fn update_application_configuration(&self, application_configuration: ConfigurationModel) {
        *self.application_configuration.borrow_mut() = application_configuration;
        self.clear_consolidated();
    }

    pub fn update_local_user_configuration(&self, local_user_configuration: ConfigurationModel) {
        *self.local_user_configuration.borrow_mut() = local_user_configuration;
        *self.user_configuration.borrow_mut() = None;
        self.clear_consolidated();
    }

    pub fn update_remote_user_configuration(&self, remote_user_configuration: ConfigurationModel) {
        *self.remote_user_configuration.borrow_mut() = remote_user_configuration;
        *self.user_configuration.borrow_mut() = None;
        self.clear_consolidated();
    }

    pub fn update_workspace_configuration(&self, workspace_configuration: ConfigurationModel) {
        *self.workspace_configuration.borrow_mut() = workspace_configuration;
        self.clear_consolidated();
    }

    pub fn update_folder_configuration(&self, resource: &URI, configuration: ConfigurationModel) {
        self.folder_configurations.borrow_mut().set(resource.clone(), configuration);
        self.folders_consolidated_configurations.borrow_mut().delete(resource);
    }

    pub fn delete_folder_configuration(&self, resource: &URI) {
        self.folder_configurations.borrow_mut().delete(resource);
        self.folders_consolidated_configurations.borrow_mut().delete(resource);
    }

    /// `compareAndUpdateDefaultConfiguration(defaults, keys?)`.
    pub fn compare_and_update_default_configuration(&self, defaults: ConfigurationModel, keys: Option<Vec<JsString>>) -> ConfigurationChange {
        let mut overrides: Vec<(JsString, Vec<JsString>)> = Vec::new();
        let current = self.default_configuration.borrow().clone();
        let keys = keys.unwrap_or_else(|| {
            let result = compare(Some(&current), Some(&defaults));
            [result.added, result.updated, result.removed].concat()
        });
        for key in &keys {
            for override_identifier in override_identifiers_from_key(key) {
                let from_keys = current.get_keys_for_override_identifier(&override_identifier);
                let to_keys = defaults.get_keys_for_override_identifier(&override_identifier);
                let mut changed: Vec<JsString> = Vec::new();
                changed.extend(to_keys.iter().filter(|k| !from_keys.contains(k)).cloned());
                changed.extend(from_keys.iter().filter(|k| !to_keys.contains(k)).cloned());
                changed.extend(
                    from_keys
                        .iter()
                        .filter(|k| !objects::equals(&current.override_for(&override_identifier).get_value(k), &defaults.override_for(&override_identifier).get_value(k)))
                        .cloned(),
                );
                overrides.push((override_identifier, changed));
            }
        }
        self.update_default_configuration(defaults);
        ConfigurationChange { keys, overrides }
    }

    /// `compareAndUpdatePolicyConfiguration`.
    pub fn compare_and_update_policy_configuration(&self, policy_configuration: ConfigurationModel) -> ConfigurationChange {
        let current = self.policy_configuration.borrow().clone();
        let result = compare(Some(&current), Some(&policy_configuration));
        let keys = [result.added, result.updated, result.removed].concat();
        if !keys.is_empty() {
            self.update_policy_configuration(policy_configuration);
        }
        ConfigurationChange { keys, overrides: Vec::new() }
    }

    /// `compareAndUpdateApplicationConfiguration`.
    pub fn compare_and_update_application_configuration(&self, application: ConfigurationModel) -> ConfigurationChange {
        let result = compare(Some(&self.application_configuration()), Some(&application));
        let keys = [result.added, result.updated, result.removed].concat();
        if !keys.is_empty() {
            self.update_application_configuration(application);
        }
        ConfigurationChange { keys, overrides: result.overrides }
    }

    /// `compareAndUpdateLocalUserConfiguration`.
    pub fn compare_and_update_local_user_configuration(&self, user: ConfigurationModel) -> ConfigurationChange {
        let result = compare(Some(&self.local_user_configuration()), Some(&user));
        let keys = [result.added, result.updated, result.removed].concat();
        if !keys.is_empty() {
            self.update_local_user_configuration(user);
        }
        ConfigurationChange { keys, overrides: result.overrides }
    }

    /// `compareAndUpdateRemoteUserConfiguration`.
    pub fn compare_and_update_remote_user_configuration(&self, user: ConfigurationModel) -> ConfigurationChange {
        let result = compare(Some(&self.remote_user_configuration()), Some(&user));
        let keys = [result.added, result.updated, result.removed].concat();
        if !keys.is_empty() {
            self.update_remote_user_configuration(user);
        }
        ConfigurationChange { keys, overrides: result.overrides }
    }

    /// `compareAndUpdateWorkspaceConfiguration`.
    pub fn compare_and_update_workspace_configuration(&self, workspace_configuration: ConfigurationModel) -> ConfigurationChange {
        let result = compare(Some(&self.workspace_configuration()), Some(&workspace_configuration));
        let keys = [result.added, result.updated, result.removed].concat();
        if !keys.is_empty() {
            self.update_workspace_configuration(workspace_configuration);
        }
        ConfigurationChange { keys, overrides: result.overrides }
    }

    /// `compareAndUpdateFolderConfiguration`.
    pub fn compare_and_update_folder_configuration(&self, resource: &URI, folder_configuration: ConfigurationModel) -> ConfigurationChange {
        let current_folder_configuration = self.folder_configurations.borrow().get(resource).cloned();
        let result = compare(current_folder_configuration.as_ref(), Some(&folder_configuration));
        let keys = [result.added, result.updated, result.removed].concat();
        if !keys.is_empty() || current_folder_configuration.is_none() {
            self.update_folder_configuration(resource, folder_configuration);
        }
        ConfigurationChange { keys, overrides: result.overrides }
    }

    /// `compareAndDeleteFolderConfiguration`: lança `Error('Unknown folder')` se a pasta não existe.
    pub fn compare_and_delete_folder_configuration(&self, folder: &URI) -> ConfigurationChange {
        let Some(folder_config) = self.folder_configurations.borrow().get(folder).cloned() else {
            errors::throw(Error::new("Unknown folder"));
        };
        self.delete_folder_configuration(folder);
        let result = compare(Some(&folder_config), None);
        ConfigurationChange { keys: [result.added, result.updated, result.removed].concat(), overrides: result.overrides }
    }

    pub fn defaults(&self) -> ConfigurationModel {
        self.default_configuration.borrow().clone()
    }

    pub fn application_configuration(&self) -> ConfigurationModel {
        self.application_configuration.borrow().clone()
    }

    /// `userConfiguration`: a local, ou a local mesclada com a remota.
    pub fn user_configuration(&self) -> ConfigurationModel {
        if let Some(user) = self.user_configuration.borrow().as_ref() {
            return user.clone();
        }
        let local = self.local_user_configuration();
        let remote = self.remote_user_configuration();
        let user = if remote.is_empty() {
            local
        } else {
            let merged = local.merge(&[remote]);
            ConfigurationModel::new(merged.contents(), merged.keys(), merged.overrides(), None, self.log.clone())
        };
        *self.user_configuration.borrow_mut() = Some(user.clone());
        user
    }

    pub fn local_user_configuration(&self) -> ConfigurationModel {
        self.local_user_configuration.borrow().clone()
    }

    pub fn remote_user_configuration(&self) -> ConfigurationModel {
        self.remote_user_configuration.borrow().clone()
    }

    pub fn workspace_configuration(&self) -> ConfigurationModel {
        self.workspace_configuration.borrow().clone()
    }

    /// `folderConfigurations`: o mapa vivo.
    pub fn folder_configurations(&self) -> &RefCell<ResourceMap<ConfigurationModel>> {
        &self.folder_configurations
    }

    fn get_consolidated_configuration_model(&self, section: Option<&JsString>, overrides: &ConfigurationOverrides, workspace: Option<&Workspace>) -> ConfigurationModel {
        let mut configuration_model = self.get_consolidated_configuration_model_for_resource(overrides, workspace);
        if let Some(override_identifier) = overrides.override_identifier.as_ref().filter(|i| !i.is_empty()) {
            configuration_model = configuration_model.override_for(override_identifier);
        }
        let policy = self.policy_configuration.borrow().clone();
        if !policy.is_empty() && !policy.get_value_opt(section).is_undefined() {
            // clone by merging
            configuration_model = configuration_model.merge(&[]);
            for key in policy.keys() {
                configuration_model.set_value(&key, policy.get_value(&key));
            }
        }
        configuration_model
    }

    fn get_consolidated_configuration_model_for_resource(&self, overrides: &ConfigurationOverrides, workspace: Option<&Workspace>) -> ConfigurationModel {
        let mut consolidate_configuration = self.get_workspace_consolidated_configuration();

        if let (Some(workspace), Some(resource)) = (workspace, overrides.resource.as_ref()) {
            if let Some(root) = workspace.get_folder(resource) {
                consolidate_configuration = self.get_folder_consolidated_configuration(&root.uri);
            }
            let memory_configuration_for_resource = self.memory_configuration_by_resource.borrow().get(resource).cloned();
            if let Some(memory) = memory_configuration_for_resource {
                consolidate_configuration = consolidate_configuration.merge(&[memory]);
            }
        }

        consolidate_configuration
    }

    fn get_workspace_consolidated_configuration(&self) -> ConfigurationModel {
        if let Some(consolidated) = self.workspace_consolidated_configuration.borrow().as_ref() {
            return consolidated.clone();
        }
        let consolidated = self.default_configuration.borrow().merge(&[
            self.application_configuration(),
            self.user_configuration(),
            self.workspace_configuration(),
            self.memory_configuration.clone(),
        ]);
        *self.workspace_consolidated_configuration.borrow_mut() = Some(consolidated.clone());
        consolidated
    }

    fn get_folder_consolidated_configuration(&self, folder: &URI) -> ConfigurationModel {
        if let Some(consolidated) = self.folders_consolidated_configurations.borrow().get(folder) {
            return consolidated.clone();
        }
        let workspace_consolidate_configuration = self.get_workspace_consolidated_configuration();
        let folder_configuration = self.folder_configurations.borrow().get(folder).cloned();
        match folder_configuration {
            Some(folder_configuration) => {
                let consolidated = workspace_consolidate_configuration.merge(&[folder_configuration]);
                self.folders_consolidated_configurations.borrow_mut().set(folder.clone(), consolidated.clone());
                consolidated
            }
            None => workspace_consolidate_configuration,
        }
    }

    fn get_folder_configuration_model_for_resource(&self, resource: Option<&URI>, workspace: Option<&Workspace>) -> Option<ConfigurationModel> {
        let (workspace, resource) = (workspace?, resource?);
        let root = workspace.get_folder(resource)?;
        self.folder_configurations.borrow().get(&root.uri).cloned()
    }

    /// `toData()`.
    pub fn to_data(&self) -> ConfigurationData {
        let data_of = |model: &ConfigurationModel, with_raw: bool| ConfigurationModelData {
            contents: model.contents(),
            keys: model.keys(),
            overrides: model.overrides(),
            raw: if with_raw { match model.raw() { Some(Raw::List(_)) | None => None, other => other } } else { None },
        };
        let folders = self.folder_configurations.borrow();
        ConfigurationData {
            defaults: data_of(&self.default_configuration.borrow(), false),
            policy: data_of(&self.policy_configuration.borrow(), false),
            application: data_of(&self.application_configuration(), true),
            user_local: data_of(&self.local_user_configuration(), true),
            user_remote: data_of(&self.remote_user_configuration(), true),
            workspace: data_of(&self.workspace_configuration(), false),
            folders: folders.entries().map(|(folder, model)| (folder.clone(), data_of(model, false))).collect(),
        }
    }

    /// `allKeys()`.
    pub fn all_keys(&self) -> Vec<JsString> {
        let mut keys: OrderedMap<JsString, ()> = OrderedMap::new();
        let mut add = |list: Vec<JsString>| {
            for key in list {
                keys.insert(key, ());
            }
        };
        add(self.default_configuration.borrow().keys());
        add(self.user_configuration().keys());
        add(self.workspace_configuration().keys());
        for folder_configuration in self.folder_configurations.borrow().values() {
            add(folder_configuration.keys());
        }
        keys.keys().cloned().collect()
    }

    /// `allOverrideIdentifiers()`.
    pub fn all_override_identifiers(&self) -> Vec<JsString> {
        let mut keys: OrderedMap<JsString, ()> = OrderedMap::new();
        let mut add = |list: Vec<JsString>| {
            for key in list {
                keys.insert(key, ());
            }
        };
        add(self.default_configuration.borrow().get_all_override_identifiers());
        add(self.user_configuration().get_all_override_identifiers());
        add(self.workspace_configuration().get_all_override_identifiers());
        for folder_configuration in self.folder_configurations.borrow().values() {
            add(folder_configuration.get_all_override_identifiers());
        }
        keys.keys().cloned().collect()
    }

    /// `getAllKeysForOverrideIdentifier(overrideIdentifier)`.
    pub fn get_all_keys_for_override_identifier(&self, override_identifier: impl Key) -> Vec<JsString> {
        let override_identifier = override_identifier.to_js();
        let mut keys: OrderedMap<JsString, ()> = OrderedMap::new();
        let mut add = |list: Vec<JsString>| {
            for key in list {
                keys.insert(key, ());
            }
        };
        add(self.default_configuration.borrow().get_keys_for_override_identifier(&override_identifier));
        add(self.user_configuration().get_keys_for_override_identifier(&override_identifier));
        add(self.workspace_configuration().get_keys_for_override_identifier(&override_identifier));
        for folder_configuration in self.folder_configurations.borrow().values() {
            add(folder_configuration.get_keys_for_override_identifier(&override_identifier));
        }
        keys.keys().cloned().collect()
    }

    /// `Configuration.parse(data, logService)`.
    pub fn parse(data: &ConfigurationData, log: LogServiceRef) -> Configuration {
        let parse_model = |model: &ConfigurationModelData| ConfigurationModel::new(model.contents.clone(), model.keys.clone(), model.overrides.clone(), model.raw.clone(), log.clone());
        let mut folders: ResourceMap<ConfigurationModel> = ResourceMap::new();
        for (uri, model) in &data.folders {
            folders.set(uri.clone(), parse_model(model));
        }
        Configuration::new(
            parse_model(&data.defaults),
            parse_model(&data.policy),
            parse_model(&data.application),
            parse_model(&data.user_local),
            parse_model(&data.user_remote),
            parse_model(&data.workspace),
            folders,
            ConfigurationModel::create_empty_model(log.clone()),
            ResourceMap::new(),
            log,
        )
    }
}

/// `mergeChanges(...changes)`.
pub fn merge_changes(changes: &[ConfigurationChange]) -> ConfigurationChange {
    match changes {
        [] => return ConfigurationChange::default(),
        [single] => return single.clone(),
        _ => {}
    }
    let mut keys_set: OrderedMap<JsString, ()> = OrderedMap::new();
    let mut overrides_map: OrderedMap<JsString, OrderedMap<JsString, ()>> = OrderedMap::new();
    for change in changes {
        for key in &change.keys {
            keys_set.insert(key.clone(), ());
        }
        for (identifier, keys) in &change.overrides {
            if !overrides_map.contains_key(identifier) {
                overrides_map.insert(identifier.clone(), OrderedMap::new());
            }
            let result = overrides_map.get_mut(identifier).expect("acabou de inserir");
            for key in keys {
                result.insert(key.clone(), ());
            }
        }
    }
    ConfigurationChange {
        keys: keys_set.keys().cloned().collect(),
        overrides: overrides_map.iter().map(|(identifier, keys)| (identifier.clone(), keys.keys().cloned().collect())).collect(),
    }
}

/// O estado anterior que um [`ConfigurationChangeEvent`] guarda para comparar valores.
pub struct PreviousConfiguration {
    pub workspace: Option<Rc<Workspace>>,
    pub data: ConfigurationData,
}

/// `ConfigurationChangeEvent`.
pub struct ConfigurationChangeEvent {
    affects_config_str: JsString,
    pub affected_keys: Vec<JsString>,
    pub change: ConfigurationChange,
    pub source: Cell<Option<ConfigurationTarget>>,
    previous: Option<PreviousConfiguration>,
    previous_configuration: RefCell<Option<Rc<Configuration>>>,
    current_configuration: Rc<Configuration>,
    current_workspace: Option<Rc<Workspace>>,
    log: LogServiceRef,
}

const MARKER: u16 = b'\n' as u16;
const MARKER_CODE_2: u16 = b'.' as u16;

impl ConfigurationChangeEvent {
    pub fn new(
        change: ConfigurationChange,
        previous: Option<PreviousConfiguration>,
        current_configuration: Rc<Configuration>,
        current_workspace: Option<Rc<Workspace>>,
        log: LogServiceRef,
    ) -> Self {
        let mut affected_keys: Vec<JsString> = Vec::new();
        let mut add = |key: &JsString| {
            if !affected_keys.contains(key) {
                affected_keys.push(key.clone());
            }
        };
        for key in &change.keys {
            add(key);
        }
        for (_, keys) in &change.overrides {
            for key in keys {
                add(key);
            }
        }

        // Example: '\nfoo.bar\nabc.def\n'
        let mut units = vec![MARKER];
        for key in &affected_keys {
            units.extend_from_slice(key.units());
            units.push(MARKER);
        }

        Self {
            affects_config_str: JsString::from_units(units),
            affected_keys,
            change,
            source: Cell::new(None),
            previous,
            previous_configuration: RefCell::new(None),
            current_configuration,
            current_workspace,
            log,
        }
    }

    /// `previousConfiguration`.
    pub fn previous_configuration(&self) -> Option<Rc<Configuration>> {
        if self.previous_configuration.borrow().is_none()
            && let Some(previous) = &self.previous
        {
            *self.previous_configuration.borrow_mut() = Some(Rc::new(Configuration::parse(&previous.data, self.log.clone())));
        }
        self.previous_configuration.borrow().clone()
    }

    /// `affectsConfiguration(section, overrides?)`.
    pub fn affects_configuration(&self, section: impl Key, overrides: Option<&ConfigurationOverrides>) -> bool {
        // we have one large string with all keys that have changed. we pad (marker) the section
        // and check that either find it padded or before a segment character
        let mut needle = vec![MARKER];
        let section = section.to_js();
        needle.extend_from_slice(section.units());
        let haystack = self.affects_config_str.units();
        let Some(idx) = js_string::index_of(haystack, &needle, 0) else {
            // NOT: (marker + section)
            return false;
        };
        let pos = idx + needle.len();
        if pos >= haystack.len() {
            return false;
        }
        let code = haystack[pos];
        if code != MARKER && code != MARKER_CODE_2 {
            // NOT: section + (marker | segment)
            return false;
        }
        if let Some(overrides) = overrides {
            let value1 = match self.previous_configuration() {
                Some(previous) => previous.get_value(Some(&section), overrides, self.previous.as_ref().and_then(|p| p.workspace.as_deref())),
                None => Dynamic::Undefined,
            };
            let value2 = self.current_configuration.get_value(Some(&section), overrides, self.current_workspace.as_deref());
            return !objects::equals(&value1, &value2);
        }
        true
    }
}

/// `compare(from, to)`.
fn compare(from: Option<&ConfigurationModel>, to: Option<&ConfigurationModel>) -> ConfigurationCompareResult {
    let raw_to = to.map(ConfigurationModel::raw_configuration);
    let raw_from = from.map(ConfigurationModel::raw_configuration);
    let (added, removed, updated) = compare_configuration_contents(
        raw_to.as_ref().map(|m| (m.keys(), m.contents())),
        raw_from.as_ref().map(|m| (m.keys(), m.contents())),
    );
    let mut overrides: Vec<(JsString, Vec<JsString>)> = Vec::new();

    let from_override_identifiers = from.map(ConfigurationModel::get_all_override_identifiers).unwrap_or_default();
    let to_override_identifiers = to.map(ConfigurationModel::get_all_override_identifiers).unwrap_or_default();

    if let Some(to) = to {
        for identifier in to_override_identifiers.iter().filter(|key| !from_override_identifiers.contains(key)) {
            overrides.push((identifier.clone(), to.get_keys_for_override_identifier(identifier)));
        }
    }

    if let Some(from) = from {
        for identifier in from_override_identifiers.iter().filter(|key| !to_override_identifiers.contains(key)) {
            overrides.push((identifier.clone(), from.get_keys_for_override_identifier(identifier)));
        }
    }

    if let (Some(to), Some(from)) = (to, from) {
        for identifier in &from_override_identifiers {
            if to_override_identifiers.contains(identifier) {
                let contents_of = |model: &ConfigurationModel| {
                    let value = model.get_override_value_opt(None, identifier);
                    (model.get_keys_for_override_identifier(identifier), if value.is_truthy() { value } else { empty_object() })
                };
                let (from_keys, from_contents) = contents_of(from);
                let (to_keys, to_contents) = contents_of(to);
                let (added, removed, updated) = compare_configuration_contents(Some((to_keys, to_contents)), Some((from_keys, from_contents)));
                overrides.push((identifier.clone(), [added, removed, updated].concat()));
            }
        }
    }

    ConfigurationCompareResult { added, removed, updated, overrides }
}

type KeysAndContents = (Vec<JsString>, Dynamic);

/// `compareConfigurationContents(to, from)`: devolve `(added, removed, updated)`.
fn compare_configuration_contents(to: Option<KeysAndContents>, from: Option<KeysAndContents>) -> (Vec<JsString>, Vec<JsString>, Vec<JsString>) {
    let added = match (&to, &from) {
        (Some((to_keys, _)), Some((from_keys, _))) => to_keys.iter().filter(|key| !from_keys.contains(key)).cloned().collect(),
        (Some((to_keys, _)), None) => to_keys.clone(),
        _ => Vec::new(),
    };
    let removed = match (&to, &from) {
        (Some((to_keys, _)), Some((from_keys, _))) => from_keys.iter().filter(|key| !to_keys.contains(key)).cloned().collect(),
        (None, Some((from_keys, _))) => from_keys.clone(),
        _ => Vec::new(),
    };
    let mut updated = Vec::new();

    if let (Some((to_keys, to_contents)), Some((from_keys, from_contents))) = (&to, &from) {
        for key in from_keys {
            if to_keys.contains(key) {
                let value1 = get_configuration_value(from_contents, key, None);
                let value2 = get_configuration_value(to_contents, key, None);
                if !objects::equals(&value1, &value2) {
                    updated.push(key.clone());
                }
            }
        }
    }
    (added, removed, updated)
}
