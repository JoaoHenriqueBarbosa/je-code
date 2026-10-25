//! Porte de `src/vs/platform/jsonschemas/common/jsonContributionRegistry.ts`: o registro de esquemas JSON
//! (`IJSONContributionRegistry`), onde cada parte do produto registra o esquema que valida e completa um arquivo
//! JSON (`vscode://schemas/icons`, `vscode://schemas/workbench-sizes`, `vscode://schemas/settings/resourceLanguage`,
//! `vscode://schemas/launch`...) e as associações de esquema com globs de arquivo.
//!
//! Mora no `je-base`, e não no `je-platform` onde fica o original, porque o `je-theme` (ícones, tamanhos, tokens)
//! também registra esquemas e não depende do `je-platform`; o `je-base` é a crate que as duas enxergam.
//!
//! Divergências:
//! - **Singleton.** O original cria `jsonContributionRegistry` no carregamento do módulo e o põe no `Registry` de
//!   plataforma (`Registry.add(Extensions.JSONContribution, ...)`). Aqui o singleton é [`get_json_contribution_registry`],
//!   preguiçoso e por thread (o `Emitter` e o `Dynamic` do porte são de uma thread só, e cada thread de teste enxerga um
//!   registro próprio), como em `iconRegistry` e `sizeUtils` do `je-theme`. O registro de plataforma não existe;
//!   [`JSON_CONTRIBUTION_EXTENSION_ID`] guarda o identificador (`Extensions.JSONContribution`) para quem o montar.
//! - **`IJSONSchema`.** É um [`Dynamic`] (convenção de [`crate::json_schema`]), porque `getCompressedContent` compara os
//!   nós por identidade. O mapa `schemasById` é um [`ObjectRef`] vivo, o mesmo objeto que [`SchemaContributions::schemas`]
//!   devolve, então um esquema registrado e mudado depois (como o do `SizeRegistry`) aparece nele, como no original.
//!   [`schema_from_json`] converte um `serde_json::Value` (a representação de esquema de alguns registros) em `Dynamic`.
//! - **`schemaAssociations`.** O original devolve o objeto vivo; [`JsonContributionRegistry::get_schema_associations`]
//!   devolve uma cópia (`IndexMap`, na ordem de inserção), porque o mapa fica sob um `RefCell`.
//! - **`Disposable`.** O registro não é `Disposable` (o singleton nunca é descartado e os emissores são marcados como
//!   singleton, fora do rastreador de vazamentos).
//! - **`registerSchema(..., store)`.** `store` é `Option<&DisposableStore>`; quando presente, o descarte dele remove o
//!   esquema e dispara `onDidChangeSchema` com o `uri` original (não normalizado), como no original.

use std::cell::RefCell;
use std::rc::Rc;

use indexmap::IndexMap;
use serde_json::Value;

use crate::JsString;
use crate::dynamic::{Dynamic, ObjectRef, Truthy};
use crate::event::{Emitter, Event};
use crate::json_schema::get_compressed_content;
use crate::lifecycle::{DisposableRef, DisposableStore, mark_as_singleton, to_disposable};

/// `Extensions.JSONContribution`.
pub const JSON_CONTRIBUTION_EXTENSION_ID: &str = "base.contributions.json";

/// `ISchemaContributions`: os esquemas registrados por id (o objeto vivo do registro).
#[derive(Clone)]
pub struct SchemaContributions {
    pub schemas: ObjectRef,
}

/// `IJSONContributionRegistry`: a interface e a classe `JSONContributionRegistry` do original são o mesmo tipo.
pub type IJSONContributionRegistry = JsonContributionRegistry;

/// `normalizeId`: tira o `#` final do id.
fn normalize_id(id: &str) -> &str {
    id.strip_suffix('#').unwrap_or(id)
}

/// Um valor JSON (`serde_json`) como [`Dynamic`], para registrar um esquema montado como `serde_json::Value`.
/// Números que o `f64` não representa viram `NaN`, o que não acontece em esquemas reais.
pub fn schema_from_json(value: &Value) -> Dynamic {
    match value {
        Value::Null => Dynamic::Null,
        Value::Bool(b) => Dynamic::Bool(*b),
        Value::Number(n) => Dynamic::Number(n.as_f64().unwrap_or(f64::NAN)),
        Value::String(s) => Dynamic::string(s),
        Value::Array(items) => Dynamic::array(items.iter().map(schema_from_json)),
        Value::Object(object) => Dynamic::object(object.iter().map(|(key, value)| (key.clone(), schema_from_json(value)))),
    }
}

/// `JSONContributionRegistry`.
pub struct JsonContributionRegistry {
    schemas_by_id: ObjectRef,
    schema_associations: Rc<RefCell<IndexMap<String, Vec<String>>>>,
    on_did_change_schema: Rc<Emitter<String>>,
    on_did_change_schema_associations: Rc<Emitter<()>>,
}

impl Default for JsonContributionRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl JsonContributionRegistry {
    /// Um registro vazio. O registro global é [`get_json_contribution_registry`].
    pub fn new() -> Self {
        Self {
            schemas_by_id: ObjectRef::new(),
            schema_associations: Rc::new(RefCell::new(IndexMap::new())),
            on_did_change_schema: mark_as_singleton(Emitter::new()),
            on_did_change_schema_associations: mark_as_singleton(Emitter::new()),
        }
    }

    /// `onDidChangeSchema`: o `uri` do esquema que mudou (como foi passado, sem normalizar).
    pub fn on_did_change_schema(&self) -> Event<String> {
        self.on_did_change_schema.event()
    }

    /// `onDidChangeSchemaAssociations`.
    pub fn on_did_change_schema_associations(&self) -> Event<()> {
        self.on_did_change_schema_associations.event()
    }

    /// `registerSchema`: registrar de novo um id substitui o esquema. O id é guardado sem o `#` final.
    pub fn register_schema(&self, uri: &str, unresolved_schema_content: Dynamic, store: Option<&DisposableStore>) {
        let normalized_uri = JsString::from(normalize_id(uri));
        self.schemas_by_id.set(&normalized_uri, unresolved_schema_content);
        self.on_did_change_schema.fire(uri.to_owned());

        if let Some(store) = store {
            let schemas_by_id = self.schemas_by_id.clone();
            let on_did_change_schema = self.on_did_change_schema.clone();
            let uri = uri.to_owned();
            store.add(to_disposable(move || {
                schemas_by_id.delete(&normalized_uri);
                on_did_change_schema.fire(uri);
            }));
        }
    }

    /// `registerSchemaAssociation`: associa o glob ao esquema; repetir o par não dispara o evento. O descarte do
    /// retorno desfaz a associação (e tira o esquema do mapa quando ele fica sem globs).
    pub fn register_schema_association(&self, uri: &str, glob: &str) -> DisposableRef {
        let normalized_uri = normalize_id(uri).to_owned();
        let added = {
            let mut associations = self.schema_associations.borrow_mut();
            let globs = associations.entry(normalized_uri.clone()).or_default();
            if globs.iter().any(|existing| existing == glob) {
                false
            } else {
                globs.push(glob.to_owned());
                true
            }
        };
        if added {
            self.on_did_change_schema_associations.fire(());
        }

        let schema_associations = self.schema_associations.clone();
        let on_did_change_schema_associations = self.on_did_change_schema_associations.clone();
        let glob = glob.to_owned();
        to_disposable(move || {
            let removed = {
                let mut associations = schema_associations.borrow_mut();
                let Some(globs) = associations.get_mut(&normalized_uri) else {
                    return;
                };
                let Some(index) = globs.iter().position(|existing| *existing == glob) else {
                    return;
                };
                globs.remove(index);
                if globs.is_empty() {
                    associations.shift_remove(&normalized_uri);
                }
                true
            };
            if removed {
                on_did_change_schema_associations.fire(());
            }
        })
    }

    /// `notifySchemaChanged`: avisa os ouvintes de que o conteúdo do esquema mudou.
    pub fn notify_schema_changed(&self, uri: &str) {
        self.on_did_change_schema.fire(uri.to_owned());
    }

    /// `getSchemaContributions`: o mapa vivo dos esquemas.
    pub fn get_schema_contributions(&self) -> SchemaContributions {
        SchemaContributions { schemas: self.schemas_by_id.clone() }
    }

    /// `getSchemaContent`: o conteúdo (comprimido) do esquema com o id, se houver. O id não é normalizado.
    pub fn get_schema_content(&self, uri: &str) -> Option<String> {
        let schema = self.schemas_by_id.get(&JsString::from(uri))?;
        schema.is_truthy().then(|| get_compressed_content(&schema))
    }

    /// `hasSchemaContent`: existe um esquema com o id? O id não é normalizado.
    pub fn has_schema_content(&self, uri: &str) -> bool {
        self.schemas_by_id.get(&JsString::from(uri)).is_truthy()
    }

    /// `getSchemaAssociations`: uma cópia das associações (esquema para globs).
    pub fn get_schema_associations(&self) -> IndexMap<String, Vec<String>> {
        self.schema_associations.borrow().clone()
    }
}

thread_local! {
    static JSON_CONTRIBUTION_REGISTRY: Rc<JsonContributionRegistry> = Rc::new(JsonContributionRegistry::new());
}

/// `Registry.as<IJSONContributionRegistry>(Extensions.JSONContribution)`: o registro global da thread.
pub fn get_json_contribution_registry() -> Rc<JsonContributionRegistry> {
    JSON_CONTRIBUTION_REGISTRY.with(Rc::clone)
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use serde_json::json;

    use super::*;
    use crate::lifecycle::IDisposable;

    fn object_schema() -> Dynamic {
        Dynamic::object([("type", Dynamic::string("object"))])
    }

    fn record_schema_changes(registry: &JsonContributionRegistry) -> Rc<RefCell<Vec<String>>> {
        let changes = Rc::new(RefCell::new(Vec::new()));
        let sink = changes.clone();
        registry.on_did_change_schema().listen(move |uri| sink.borrow_mut().push(uri.clone()));
        changes
    }

    #[test]
    fn register_schema_normalizes_the_id_but_notifies_the_original() {
        let registry = JsonContributionRegistry::new();
        let changes = record_schema_changes(&registry);
        registry.register_schema("vscode://schemas/a#", object_schema(), None);
        assert_eq!(*changes.borrow(), vec!["vscode://schemas/a#".to_owned()]);
        assert!(registry.has_schema_content("vscode://schemas/a"));
        assert!(!registry.has_schema_content("vscode://schemas/a#"));
        assert!(registry.get_schema_content("vscode://schemas/a").is_some_and(|content| content.contains("object")));
        assert!(registry.get_schema_content("vscode://schemas/missing").is_none());
    }

    #[test]
    fn disposing_the_store_removes_the_schema_and_notifies() {
        let registry = JsonContributionRegistry::new();
        let changes = record_schema_changes(&registry);
        let store = DisposableStore::new();
        registry.register_schema("vscode://schemas/b", object_schema(), Some(store.as_ref()));
        assert!(registry.has_schema_content("vscode://schemas/b"));
        store.clear();
        assert!(!registry.has_schema_content("vscode://schemas/b"));
        assert_eq!(changes.borrow().len(), 2);
    }

    #[test]
    fn contributions_are_the_live_map() {
        let registry = JsonContributionRegistry::new();
        let contributions = registry.get_schema_contributions();
        registry.register_schema("vscode://schemas/c", object_schema(), None);
        assert!(contributions.schemas.has_own(&JsString::from("vscode://schemas/c")));
    }

    #[test]
    fn associations_notify_once_and_are_removed_on_dispose() {
        let registry = JsonContributionRegistry::new();
        let count = Rc::new(RefCell::new(0));
        let counter = count.clone();
        registry.on_did_change_schema_associations().listen(move |_| *counter.borrow_mut() += 1);

        let first = registry.register_schema_association("vscode://schemas/d#", "*.d.json");
        registry.register_schema_association("vscode://schemas/d", "*.d.json");
        let second = registry.register_schema_association("vscode://schemas/d", "*.e.json");
        assert_eq!(*count.borrow(), 2);
        assert_eq!(registry.get_schema_associations()["vscode://schemas/d"], vec!["*.d.json".to_owned(), "*.e.json".to_owned()]);

        first.dispose();
        assert_eq!(registry.get_schema_associations()["vscode://schemas/d"], vec!["*.e.json".to_owned()]);
        second.dispose();
        assert!(registry.get_schema_associations().is_empty());
        assert_eq!(*count.borrow(), 4);
    }

    #[test]
    fn schema_from_json_keeps_the_structure() {
        let schema = schema_from_json(&json!({ "type": "object", "enum": ["a", 1, null, true] }));
        assert_eq!(schema.get_str("type"), Dynamic::string("object"));
        assert!(matches!(schema.get_str("enum"), Dynamic::Array(items) if items.len() == 4));
    }
}
