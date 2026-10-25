//! Porte de `src/vs/platform/theme/common/sizeUtils.ts` (novo na 1.143): o registro de tamanhos do
//! tema (`registerSize`, `getSizeRegistry`), os valores de tamanho com unidade e os nomes de variável CSS.
//!
//! Segue o desenho de [`crate::registry`] (o registro de cores) e as mesmas omissões:
//! - O registro global é o singleton do módulo (`const sizeRegistry = new SizeRegistry()`), aqui um
//!   `thread_local` de `Rc<SizeRegistry>`, porque o `Emitter` de `je_base::event` é `Rc` e não cruza
//!   threads. Cada thread vê o seu próprio registro (os testes ficam isolados). No original os tamanhos
//!   base entram porque `sizeRegistry.ts` importa `sizes/baseSizes.ts`; aqui o singleton nasce já com
//!   [`crate::sizes::base_sizes::register_base_sizes`] aplicado, e [`SizeRegistry::new`] é um registro vazio.
//! - `platform.Registry.add(Extensions.SizeContribution, ...)` é da camada de plataforma, que `je-theme` não tem (o
//!   registro de cores também não o tem): ficam [`EXTENSIONS_SIZE_CONTRIBUTION`] e [`WORKBENCH_SIZES_SCHEMA_ID`].
//!   O `IJSONContributionRegistry.registerSchema(workbenchSizesSchemaId, ...)` e o `RunOnceScheduler` de 200 ms que
//!   chama `notifySchemaChanged` existem: o singleton registra o schema vivo ([`SizeRegistry::get_size_schema`], o mesmo
//!   objeto atualizado a cada registro) em `je_base::json_contribution_registry` e agenda o aviso a cada
//!   [`SizeRegistry::on_did_change_schema`]. Como no original, a subscrição vem antes dos tamanhos base, que então já
//!   agendam o primeiro aviso.
//! - `IColorTheme` é [`ColorThemeContext`], como no registro de cores (só o `type` é lido).
//!
//! Divergências de tradução:
//! - Os dois schemas são objetos [`Dynamic`] (convenção de `je_base::json_schema`). `ArrayRef` não tem
//!   `splice`, então `deregisterSize` troca as listas `enum` e `enumDescriptions` por listas novas sem o
//!   item, dentro do mesmo objeto de schema (que mantém a identidade).
//! - `sizesById` é um `Vec` na ordem de registro (re-registrar um id mantém a posição). O original é um
//!   objeto JS, que levaria chaves que são índices de array para a frente; nenhum id real é assim.
//! - `localeCompare` do `toString` é a ordenação ICU para ASCII (pontuação, dígitos, letras sem diferenciar
//!   caixa, minúscula antes de maiúscula no empate), que é o que os ids de tamanho usam.
//! - `unit` com valor padrão `'px'` vira `impl Into<Option<SizeUnit>>`: passe a unidade direto
//!   (`SizeUnit::Px`); [`SizeUnit::default`] é `px`.

use std::cell::RefCell;

use je_base::collator::locale_compare;
use std::fmt;
use std::rc::Rc;

use je_base::JsString;
use je_base::async_::RunOnceScheduler;
use je_base::dynamic::{ArrayRef, Dynamic, ObjectRef, number_to_js_string};
use je_base::event::{Emitter, Event};
use je_base::json_contribution_registry::get_json_contribution_registry;
use je_base::lifecycle::IDisposable;

use crate::ThemeType;
use crate::registry::ColorThemeContext;
use crate::sizes::base_sizes::register_base_sizes;

/// `asCssVariableName` é o mesmo do registro de cores: `editor.fontSize` vira `--vscode-editor-fontSize`.
pub use crate::registry::as_css_variable_name;

//  ------ API types

/// `SizeIdentifier`.
pub type SizeIdentifier = String;

/// `SizeUnit`: a unidade de um valor. [`SizeUnit::Unitless`] (`''` no original) é para valores sem
/// unidade, como pesos de fonte.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SizeUnit {
    #[default]
    Px,
    Rem,
    Em,
    Percent,
    Unitless,
}

impl SizeUnit {
    /// O texto da unidade no CSS (`''` para [`SizeUnit::Unitless`]).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Px => "px",
            Self::Rem => "rem",
            Self::Em => "em",
            Self::Percent => "%",
            Self::Unitless => "",
        }
    }
}

/// `SizeValue`: uma quantidade com unidade.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SizeValue {
    pub value: f64,
    pub unit: SizeUnit,
}

/// `SizeDefaults`: o valor para cada tipo de tema (`None` é o `null` do original).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SizeDefaults {
    pub light: Option<SizeValue>,
    pub dark: Option<SizeValue>,
    pub hc_dark: Option<SizeValue>,
    pub hc_light: Option<SizeValue>,
}

impl SizeDefaults {
    /// `defaults[theme.type]`.
    pub fn for_theme(&self, theme_type: ThemeType) -> Option<SizeValue> {
        match theme_type {
            ThemeType::Light => self.light,
            ThemeType::Dark => self.dark,
            ThemeType::HighContrastDark => self.hc_dark,
            ThemeType::HighContrastLight => self.hc_light,
        }
    }
}

/// O `SizeDefaults | SizeValue` do segundo argumento de `registerSize`: padrões por tipo de tema ou um
/// único valor. `Option<SizeDefaultsOrValue>` com `None` é o `null`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SizeDefaultsOrValue {
    Defaults(SizeDefaults),
    Value(SizeValue),
}

impl From<SizeDefaults> for SizeDefaultsOrValue {
    fn from(defaults: SizeDefaults) -> Self {
        Self::Defaults(defaults)
    }
}

impl From<SizeValue> for SizeDefaultsOrValue {
    fn from(value: SizeValue) -> Self {
        Self::Value(value)
    }
}

/// `SizeContribution`.
#[derive(Clone, Debug, PartialEq)]
pub struct SizeContribution {
    pub id: SizeIdentifier,
    pub description: String,
    pub defaults: Option<SizeDefaultsOrValue>,
    pub deprecation_message: Option<String>,
}

/// `asCssVariable`: `fontSize` vira `var(--vscode-fontSize)`.
pub fn as_css_variable(size: &str) -> String {
    format!("var({})", as_css_variable_name(size))
}

/// `asCssVariableWithDefault`.
pub fn as_css_variable_with_default(size: &str, default_css_value: &str) -> String {
    format!("var({}, {default_css_value})", as_css_variable_name(size))
}

/// `isSizeDefaults`: o valor é um `SizeDefaults` (e não um `SizeValue`).
pub fn is_size_defaults(value: &SizeDefaultsOrValue) -> bool {
    matches!(value, SizeDefaultsOrValue::Defaults(_))
}

/// `size`: cria um valor de tamanho. A unidade padrão é `px`.
pub fn size(value: f64, unit: impl Into<Option<SizeUnit>>) -> SizeValue {
    SizeValue {
        value,
        unit: unit.into().unwrap_or_default(),
    }
}

/// `sizeForAllThemes`: o mesmo valor para todos os tipos de tema. A unidade padrão é `px`.
pub fn size_for_all_themes(value: f64, unit: impl Into<Option<SizeUnit>>) -> SizeDefaults {
    let size_value = Some(size(value, unit));
    SizeDefaults {
        light: size_value,
        dark: size_value,
        hc_dark: size_value,
        hc_light: size_value,
    }
}

/// `sizeValueToCss`: `10px`, `1.5rem`; sem unidade, só o número (pesos de fonte).
pub fn size_value_to_css(size_value: &SizeValue) -> String {
    format!(
        "{}{}",
        number_to_js_string(size_value.value),
        size_value.unit.as_str()
    )
}

// size registry

/// `Extensions.SizeContribution`: o id do registro na plataforma.
pub const EXTENSIONS_SIZE_CONTRIBUTION: &str = "base.contributions.sizes";

/// `DEFAULT_SIZE_CONFIG_VALUE`.
pub const DEFAULT_SIZE_CONFIG_VALUE: &str = "default";

/// `workbenchSizesSchemaId`.
pub const WORKBENCH_SIZES_SCHEMA_ID: &str = "vscode://schemas/workbench-sizes";

const UNITLESS_PATTERN: &str = r"^(?:\d+|default)$";
const UNITLESS_PATTERN_ERROR_MESSAGE: &str = "Value must be an integer (e.g., \"400\") or \"default\"";
const LENGTH_PATTERN: &str = r"^(?:\d+(\.\d+)?(px|rem|em|%)|default)$";
const LENGTH_PATTERN_ERROR_MESSAGE: &str = "Size must be a number followed by px, rem, em, or % (e.g., \"12px\", \"1.5rem\") or \"default\"";

/// O token usa valores sem unidade (pesos de fonte)? O padrão do schema é escolhido por token, para os
/// tokens de comprimento continuarem exigindo unidade.
fn is_unitless(defaults: Option<&SizeDefaultsOrValue>) -> bool {
    match defaults {
        None => false,
        Some(SizeDefaultsOrValue::Defaults(defaults)) => {
            let sample = defaults
                .dark
                .or(defaults.light)
                .or(defaults.hc_dark)
                .or(defaults.hc_light);
            sample.is_some_and(|sample| sample.unit == SizeUnit::Unitless)
        }
        Some(SizeDefaultsOrValue::Value(value)) => value.unit == SizeUnit::Unitless,
    }
}


/// `SizeRegistry` (e `ISizeRegistry`): os tamanhos registrados, na ordem de registro.
pub struct SizeRegistry {
    on_did_change_schema: Rc<Emitter<()>>,
    sizes_by_id: RefCell<Vec<SizeContribution>>,
    /// `sizeSchema`: `{ type: 'object', properties: {} }`.
    size_schema: ObjectRef,
    /// O objeto `properties` de `size_schema`.
    size_schema_properties: ObjectRef,
    /// `sizeReferenceSchema`: `{ type: 'string', enum: [], enumDescriptions: [] }`.
    size_reference_schema: ObjectRef,
}

impl Default for SizeRegistry {
    fn default() -> Self {
        let size_schema_properties = ObjectRef::new();
        Self {
            on_did_change_schema: Emitter::new(),
            sizes_by_id: RefCell::new(Vec::new()),
            size_schema: ObjectRef::from_entries([
                ("type", Dynamic::string("object")),
                ("properties", Dynamic::Object(size_schema_properties.clone())),
            ]),
            size_schema_properties,
            size_reference_schema: ObjectRef::from_entries([
                ("type", Dynamic::string("string")),
                ("enum", Dynamic::array([])),
                ("enumDescriptions", Dynamic::array([])),
            ]),
        }
    }
}

impl SizeRegistry {
    /// Um registro vazio.
    pub fn new() -> Self {
        Self::default()
    }

    /// Um registro com os tamanhos base do VS Code ([`crate::sizes::base_sizes`]).
    pub fn with_base_sizes() -> Self {
        let registry = Self::new();
        register_base_sizes(&registry);
        registry
    }

    /// `onDidChangeSchema`: dispara a cada registro, remoção e `notifyThemeUpdate`.
    pub fn on_did_change_schema(&self) -> Event<()> {
        self.on_did_change_schema.event()
    }

    /// A lista `enum` ou `enumDescriptions` do schema de referência.
    fn reference_list(&self, key: &str) -> ArrayRef {
        let Some(Dynamic::Array(list)) = self.size_reference_schema.get(&JsString::from(key))
        else {
            unreachable!("o schema de referência nasce com a lista `{key}`");
        };
        list
    }

    /// `notifyThemeUpdate`: o `default` de cada propriedade do schema passa a ser o padrão do tamanho
    /// para o tipo do tema.
    pub fn notify_theme_update(&self, theme: &dyn ColorThemeContext) {
        let ids: Vec<SizeIdentifier> = self
            .sizes_by_id
            .borrow()
            .iter()
            .map(|contribution| contribution.id.clone())
            .collect();
        for key in ids {
            let Some(size_val) = self.resolve_default_size(&key, theme) else {
                continue;
            };
            if let Some(Dynamic::Object(property)) =
                self.size_schema_properties.get(&JsString::from(key.as_str()))
            {
                property.set(
                    &JsString::from("default"),
                    Dynamic::string(&size_value_to_css(&size_val)),
                );
            }
        }
        self.on_did_change_schema.fire(());
    }

    /// `registerSize`: registrar de novo um id existente substitui a contribuição, mantendo a posição
    /// (a lista `enum` do schema de referência, como no original, ganha o id de novo).
    pub fn register_size(
        &self,
        id: &str,
        defaults: Option<SizeDefaultsOrValue>,
        description: &str,
        deprecation_message: Option<&str>,
    ) -> SizeIdentifier {
        let contribution = SizeContribution {
            id: id.to_owned(),
            description: description.to_owned(),
            defaults,
            deprecation_message: deprecation_message.map(str::to_owned),
        };
        {
            let mut sizes = self.sizes_by_id.borrow_mut();
            match sizes.iter().position(|existing| existing.id == id) {
                Some(index) => sizes[index] = contribution,
                None => sizes.push(contribution),
            }
        }

        let (pattern, pattern_error_message) = if is_unitless(defaults.as_ref()) {
            (UNITLESS_PATTERN, UNITLESS_PATTERN_ERROR_MESSAGE)
        } else {
            (LENGTH_PATTERN, LENGTH_PATTERN_ERROR_MESSAGE)
        };
        // `{ description, ...propertySchema }`: a descrição vem primeiro.
        let mut property_schema = vec![
            ("description", Dynamic::string(description)),
            ("type", Dynamic::string("string")),
            ("pattern", Dynamic::string(pattern)),
            ("patternErrorMessage", Dynamic::string(pattern_error_message)),
        ];
        if let Some(message) = deprecation_message.filter(|message| !message.is_empty()) {
            property_schema.push(("deprecationMessage", Dynamic::string(message)));
        }
        self.size_schema_properties
            .set(&JsString::from(id), Dynamic::object(property_schema));

        self.reference_list("enum").push(Dynamic::string(id));
        self.reference_list("enumDescriptions")
            .push(Dynamic::string(description));

        self.on_did_change_schema.fire(());
        id.to_owned()
    }

    /// `deregisterSize`.
    pub fn deregister_size(&self, id: &str) {
        self.sizes_by_id
            .borrow_mut()
            .retain(|contribution| contribution.id != id);
        self.size_schema_properties.delete(&JsString::from(id));
        let id_value = Dynamic::string(id);
        let index = self
            .reference_list("enum")
            .to_vec()
            .iter()
            .position(|item| item.strict_equals(&id_value));
        if let Some(index) = index {
            for key in ["enum", "enumDescriptions"] {
                let mut items = self.reference_list(key).to_vec();
                if index < items.len() {
                    items.remove(index);
                }
                self.size_reference_schema
                    .set(&JsString::from(key), Dynamic::array(items));
            }
        }
        self.on_did_change_schema.fire(());
    }

    /// `getSizes`.
    pub fn get_sizes(&self) -> Vec<SizeContribution> {
        self.sizes_by_id.borrow().clone()
    }

    /// `resolveDefaultSize`: o padrão do tamanho para o tipo do `theme`.
    pub fn resolve_default_size(
        &self,
        id: &str,
        theme: &dyn ColorThemeContext,
    ) -> Option<SizeValue> {
        let defaults = self
            .sizes_by_id
            .borrow()
            .iter()
            .find(|contribution| contribution.id == id)?
            .defaults?;
        match defaults {
            SizeDefaultsOrValue::Defaults(defaults) => defaults.for_theme(theme.theme_type()),
            SizeDefaultsOrValue::Value(value) => Some(value),
        }
    }

    /// `getSizeSchema`: o schema de um objeto que atribui valores aos tamanhos. É sempre o mesmo objeto.
    pub fn get_size_schema(&self) -> Dynamic {
        Dynamic::Object(self.size_schema.clone())
    }

    /// `getSizeReferenceSchema`: o schema de uma referência a um tamanho. É sempre o mesmo objeto.
    pub fn get_size_reference_schema(&self) -> Dynamic {
        Dynamic::Object(self.size_reference_schema.clone())
    }
}

impl IDisposable for SizeRegistry {
    fn dispose(&self) {
        self.on_did_change_schema.dispose();
    }
}

/// `toString`: uma linha por tamanho, os ids sem ponto antes dos com ponto, cada grupo em ordem alfabética.
impl fmt::Display for SizeRegistry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut sizes = self.get_sizes();
        sizes.sort_by(|a, b| {
            let category = |id: &str| usize::from(id.contains('.'));
            category(&a.id)
                .cmp(&category(&b.id))
                .then_with(|| locale_compare(&a.id, &b.id))
        });
        let lines: Vec<String> = sizes
            .iter()
            .map(|contribution| format!("- `{}`: {}", contribution.id, contribution.description))
            .collect();
        f.write_str(&lines.join("\n"))
    }
}

/// Atraso do `RunOnceScheduler` que avisa a mudança do schema de tamanhos.
const SIZES_SCHEMA_NOTIFY_DELAY_MS: u64 = 200;

/// O trecho de `sizeUtils.ts` que registra `workbenchSizesSchemaId` no `jsonContributionRegistry` e agenda o
/// `notifySchemaChanged` a cada `onDidChangeSchema`.
fn register_sizes_schema(registry: &SizeRegistry) {
    let schema_registry = get_json_contribution_registry();
    schema_registry.register_schema(WORKBENCH_SIZES_SCHEMA_ID, registry.get_size_schema(), None);

    let delayer = RunOnceScheduler::new(
        || get_json_contribution_registry().notify_schema_changed(WORKBENCH_SIZES_SCHEMA_ID),
        SIZES_SCHEMA_NOTIFY_DELAY_MS,
    );
    registry.on_did_change_schema().listen(move |_| {
        if !delayer.is_scheduled() {
            delayer.schedule(None);
        }
    });
}

thread_local! {
    // O registro e as inscrições dele nascem na importação do módulo no original, fora do rastreio de descartáveis.
    static SIZE_REGISTRY: Rc<SizeRegistry> = je_base::lifecycle::without_disposable_tracker(|| {
        let registry = Rc::new(SizeRegistry::new());
        register_sizes_schema(&registry);
        register_base_sizes(&registry);
        registry
    });
}

/// `getSizeRegistry()`: o registro global, com os tamanhos base do VS Code.
pub fn get_size_registry() -> Rc<SizeRegistry> {
    SIZE_REGISTRY.with(Rc::clone)
}

/// `registerSize`: registra no registro global.
pub fn register_size(
    id: &str,
    defaults: Option<SizeDefaultsOrValue>,
    description: &str,
    deprecation_message: Option<&str>,
) -> SizeIdentifier {
    get_size_registry().register_size(id, defaults, description, deprecation_message)
}
