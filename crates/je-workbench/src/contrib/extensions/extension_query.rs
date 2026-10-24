//! Porte de `src/vs/workbench/contrib/extensions/common/extensionQuery.ts`: a consulta da caixa de busca de
//! extensões (`@sort:installs foo`), com o `parse`, a serialização e as sugestões de autocompletar.
//!
//! Notas de tradução:
//! - `value` e `sortBy` são [`JsString`] (UTF-16), como o `string` do original; o `trim()` é o de
//!   [`je_base::js_string::trim`] (o mesmo espaço do `String.prototype.trim`).
//! - `toString()` vira [`Query::to_js_string`], que devolve o texto UTF-16 sem perda.
//! - `suggestions` recebe a consulta como `&str`: ela só é procurada por `indexOf` de trechos ASCII.
//! - `IExtensionGalleryManifest` é o [`GalleryManifest`] de `je-extensions`, que guarda `filtering` e `sorting`
//!   como pares `(nome, valor)`; `capabilities.extensionQuery?.filtering?.some(c => c.name === X)` vira a busca
//!   pelo nome de `FilterType`/`SortBy` nesses pares (manifesto ausente ou sem a lista não tem a capacidade).
//! - `equals(other)` é o `==` derivado (`value` e `sortBy` iguais); não há método de repasse.
//! - `Registry.as<IExtensionFeaturesRegistry>(...)` é [`extension_features_registry`].
//! - A regex `/@sort:(\w+)(-\w*)?/g` do `parse` roda no `zyarr`, com o laço `replace` global de `je_base::js_regexp`.

use std::rc::Rc;

use je_base::js_regexp::replace_all;
use je_base::js_string::{JsString, index_of, trim};
use je_base::lazy::Lazy;
use je_extensions::gallery::GalleryManifest;
use je_extensions::management::{FilterType, SortBy};
use je_extensions::manifest::EXTENSION_CATEGORIES;
use zyarr::RegExp;

use crate::services::extension_management::extension_features::extension_features_registry;

thread_local! {
    /// `/@sort:(\w+)(-\w*)?/g`. Fica em `thread_local!` dentro de um [`Lazy`], porque o `Lazy` não é `Sync`.
    static SORT_REGEX: Lazy<Rc<RegExp>> = Lazy::new(|| {
        Rc::new(RegExp::from_str(r"@sort:(\w+)(-\w*)?", "g").expect("a regex do extensionQuery é válida"))
    });
}

fn units(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Query {
    pub value: JsString,
    pub sort_by: JsString,
}

impl Query {
    /// `new Query(value, sortBy)`: o `value` é guardado sem os espaços das pontas.
    pub fn new(value: impl Into<JsString>, sort_by: impl Into<JsString>) -> Self {
        let value = value.into();
        Self { value: JsString::from(trim(value.units())), sort_by: sort_by.into() }
    }

    pub fn suggestions(query: &str, gallery_manifest: Option<&GalleryManifest>) -> Vec<String> {
        let supports_filter = |filter: FilterType| {
            gallery_manifest.is_some_and(|manifest| manifest.filtering.iter().any(|(name, _)| name == filter.as_str()))
        };
        let supports_sort = |sort: SortBy| {
            gallery_manifest.is_some_and(|manifest| manifest.sorting.iter().any(|(name, _)| name == sort.as_str()))
        };

        let mut commands: Vec<&str> = vec!["installed", "updates", "enabled", "disabled", "builtin", "contribute"];
        if supports_filter(FilterType::Featured) {
            commands.push("featured");
        }

        commands.extend(["mcp", "agentPlugins", "popular", "recommended", "recentlyPublished", "workspaceUnsupported", "deprecated", "sort"]);
        let is_categories_enabled = supports_filter(FilterType::Category);
        if is_categories_enabled {
            commands.push("category");
        }

        commands.extend(["tag", "ext", "id", "outdated", "recentlyUpdated", "restartRequired"]);
        let mut sort_commands: Vec<String> = Vec::new();
        if supports_sort(SortBy::InstallCount) {
            sort_commands.push("installs".to_owned());
        }
        if supports_sort(SortBy::WeightedRating) {
            sort_commands.push("rating".to_owned());
        }
        sort_commands.extend(["name", "publishedDate", "updateDate"].map(str::to_owned));

        let contribute_commands: Vec<String> =
            extension_features_registry().get_extension_features().iter().map(|feature| feature.id.clone()).collect();

        let category_commands: Vec<String> = if is_categories_enabled {
            EXTENSION_CATEGORIES.iter().map(|category| format!("\"{}\"", category.to_lowercase())).collect()
        } else {
            Vec::new()
        };
        let free_text_commands = [String::new()];

        let query_contains = |substr: &str| query.contains(substr);
        let has_sort = sort_commands.iter().any(|subcommand| query_contains(&format!("@sort:{subcommand}")));
        let has_category = category_commands.iter().any(|subcommand| query_contains(&format!("@category:{subcommand}")));

        commands
            .into_iter()
            .flat_map(|command| -> Vec<String> {
                if has_sort && command == "sort" || has_category && command == "category" {
                    return Vec::new();
                }
                // `subcommands` do original: `sort`, `category`, `tag`, `ext`, `id` e `contribute`.
                let subcommands: Option<&[String]> = match command {
                    "sort" => Some(&sort_commands),
                    "category" => Some(&category_commands),
                    "tag" | "ext" | "id" => Some(&free_text_commands),
                    "contribute" => Some(&contribute_commands),
                    _ => None,
                };
                match subcommands {
                    Some(subcommands) => subcommands
                        .iter()
                        .map(|subcommand| format!("@{command}:{subcommand}{}", if subcommand.is_empty() { "" } else { " " }))
                        .collect(),
                    None if query_contains(&format!("@{command}")) => Vec::new(),
                    None => vec![format!("@{command} ")],
                }
            })
            .collect()
    }

    pub fn parse(value: impl Into<JsString>) -> Query {
        let value = value.into();
        let mut sort_by = JsString::new();
        let regex = SORT_REGEX.with(Lazy::value);
        let rest = replace_all(&regex, value.units(), |found| {
            sort_by = JsString::from(found.group(1).unwrap_or_default());
            JsString::new()
        });
        Query::new(rest, sort_by)
    }

    /// `toString()`.
    pub fn to_js_string(&self) -> JsString {
        let mut result = self.value.clone();

        if !self.sort_by.is_empty() {
            if !result.is_empty() {
                result.push_unit(u16::from(b' '));
            }
            result.push_units(&units("@sort:"));
            result.push_units(self.sort_by.units());
        }
        result
    }

    /// `!/@outdated/.test(this.value)`.
    pub fn is_valid(&self) -> bool {
        index_of(self.value.units(), &units("@outdated"), 0).is_none()
    }
}
