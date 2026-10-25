//! Porte de `src/vs/platform/theme/common/iconRegistry.ts`: o registro de ícones (`registerIcon`), as fontes de ícones
//! (`registerIconFont`), as definições (`IconDefinition`, `IconFontDefinition`), os esquemas JSON dos ícones e os ícones
//! comuns do fim do arquivo (`widgetClose`, `gotoPreviousLocation`, `gotoNextLocation`, `syncing`, `spinningLoading`).
//!
//! Divergências:
//! - **Singleton.** O original cria `iconRegistry` no carregamento do módulo e o põe no `Registry` de plataforma
//!   (`Registry.add(Extensions.IconContribution, ...)`). Aqui o singleton é [`get_icon_registry`], preguiçoso e por
//!   thread (o `Event`, o `URI` e o resto do porte são de uma thread só), e o registro de plataforma não existe;
//!   [`ICON_CONTRIBUTION_EXTENSION_ID`] guarda o identificador para quem o montar. O `initialize()` (um ícone por codicon)
//!   e os ícones comuns são registrados na criação do singleton, então quem o obtém já os encontra, como depois de
//!   importar o módulo. O registro não é `Disposable` (o singleton nunca é descartado e não entra no rastreador de
//!   vazamentos).
//! - **Constantes de ícone.** As `export const x = registerIcon(...)` do arquivo são funções ([`widget_close`] e irmãs): o
//!   registro roda na primeira chamada que obtém o singleton, e elas devolvem o [`ThemeIcon`] do id.
//! - **`IIconRegistry`.** A interface e a classe `IconRegistry` são o mesmo tipo, [`IconRegistry`] (apelido
//!   [`IIconRegistry`]). `Codicon.x` é `ThemeIcon::from_id("x")` (o `je-codicons` guarda só ids e pontos de código).
//! - **Esquemas JSON.** `IJSONSchema` é um `serde_json::Value` (como em `token_classification_registry`).
//!   [`IconRegistry::get_icon_schema`] e [`IconRegistry::get_icon_reference_schema`] devolvem uma cópia montada na hora,
//!   não o objeto vivo, porque o registro guarda as propriedades e as enumerações à parte. O registro do esquema
//!   `vscode://schemas/icons` no `jsonContributionRegistry` e o `RunOnceScheduler` de 200 ms que reage a
//!   `onDidChange` existem: o singleton registra [`ICONS_SCHEMA_ID`] em [`je_base::json_contribution_registry`] na criação e um
//!   `RunOnceScheduler` de 200 ms, agendado por `onDidChange`, chama `notifySchemaChanged`. Como o esquema do registro é uma
//!   cópia montada na hora (acima), o objeto registrado é refeito no próprio lugar (a mesma identidade, como o objeto vivo
//!   do original) quando o agendador dispara, antes do aviso; entre uma mudança e o disparo ele fica defasado em até 200 ms.
//!   O esquema é `serde_json::Value` aqui e vira [`Dynamic`] ao ser registrado ([`schema_from_json`]).
//! - **Expressões regulares.** `fontIdRegex` e irmãs são os textos dos padrões (o `.source`), nas constantes `FONT_*_REGEX`;
//!   o `fontWeightRegex` mantém o `\d{0-1000}` do original, que não é um quantificador válido.
//! - **`IconContribution.getDefinition(contribution, registry)`.** O original ignora o `registry` e lê o singleton; aqui o
//!   parâmetro é usado (os chamadores sempre passam o singleton).
//! - **`description` e `deprecationMessage`** ausentes (`undefined`) são `None`. O registro devolve cópias das contribuições
//!   (o original devolve o próprio objeto, mutável por `registerIcon`), então ler uma contribuição não vê uma descrição
//!   acrescentada depois.
//! - **`toString()`** é o [`std::fmt::Display`] do registro.

use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;

use je_base::async_::RunOnceScheduler;
use je_base::collator::locale_compare;
use je_base::dynamic::{Dynamic, ObjectRef};
use je_base::event::{Emitter, Event};
use je_base::json_contribution_registry::{get_json_contribution_registry, schema_from_json};
use je_base::lifecycle::without_disposable_tracker;
use je_base::nls::localize;
use je_base::themables::{ICON_NAME_EXPRESSION, IconModifier, ThemeIcon};
use je_base::uri::URI;
use serde_json::{Map, Value, json};

/// `Extensions.IconContribution`.
pub const ICON_CONTRIBUTION_EXTENSION_ID: &str = "base.contributions.icons";

/// `iconsSchemaId`.
pub const ICONS_SCHEMA_ID: &str = "vscode://schemas/icons";

// regexes for validation of font properties

/// `fontIdRegex.source`.
pub const FONT_ID_REGEX: &str = r"^([\w_-]+)$";
/// `fontStyleRegex.source`.
pub const FONT_STYLE_REGEX: &str = r"^(normal|italic|(oblique[ \w\s-]+))$";
/// `fontWeightRegex.source`.
pub const FONT_WEIGHT_REGEX: &str = r"^(normal|bold|lighter|bolder|(\d{0-1000}))$";
/// `fontSizeRegex.source`.
pub const FONT_SIZE_REGEX: &str = r"^([\w_.%+-]+)$";
/// `fontFormatRegex.source`.
pub const FONT_FORMAT_REGEX: &str = r"^woff|woff2|truetype|opentype|embedded-opentype|svg$";
/// `fontColorRegex.source`.
pub const FONT_COLOR_REGEX: &str = r"^#[0-9a-fA-F]{0,6}$";

/// `fontIdErrorMessage`.
pub fn font_id_error_message() -> String {
    localize("schema.fontId.formatError", "The font ID must only contain letters, numbers, underscores and dashes.", &[])
}

const WIDGET_CLOSE_ID: &str = "widget-close";
const GOTO_PREVIOUS_LOCATION_ID: &str = "goto-previous-location";
const GOTO_NEXT_LOCATION_ID: &str = "goto-next-location";

// ------ API types

/// `IconDefaults`: o ícone de que este herda a definição, ou a definição em si.
#[derive(Clone)]
pub enum IconDefaults {
    ThemeIcon(ThemeIcon),
    Definition(IconDefinition),
}

impl From<ThemeIcon> for IconDefaults {
    fn from(icon: ThemeIcon) -> Self {
        Self::ThemeIcon(icon)
    }
}

impl From<IconDefinition> for IconDefaults {
    fn from(definition: IconDefinition) -> Self {
        Self::Definition(definition)
    }
}

/// `IconDefinition`.
#[derive(Clone)]
pub struct IconDefinition {
    /// `None` é a fonte padrão (codicon).
    pub font: Option<IconFontContribution>,
    pub font_character: String,
}

/// `IconContribution`.
#[derive(Clone)]
pub struct IconContribution {
    pub id: String,
    pub description: Option<String>,
    pub deprecation_message: Option<String>,
    pub defaults: IconDefaults,
}

impl IconContribution {
    /// `IconContribution.getDefinition`: segue os ícones de que este herda até uma definição; `None` se algum
    /// deles não está registrado.
    pub fn get_definition(&self, registry: &IconRegistry) -> Option<IconDefinition> {
        let mut definition = self.defaults.clone();
        loop {
            match definition {
                IconDefaults::Definition(definition) => return Some(definition),
                IconDefaults::ThemeIcon(icon) => definition = registry.get_icon(&icon.id)?.defaults,
            }
        }
    }
}

/// `IconFontContribution`.
#[derive(Clone)]
pub struct IconFontContribution {
    pub id: String,
    pub definition: IconFontDefinition,
}

/// `IconFontSource`.
#[derive(Clone)]
pub struct IconFontSource {
    pub location: URI,
    pub format: String,
}

/// `IconFontDefinition`.
#[derive(Clone)]
pub struct IconFontDefinition {
    pub weight: Option<String>,
    pub style: Option<String>,
    pub src: Vec<IconFontSource>,
}

impl IconFontDefinition {
    /// `IconFontDefinition.toJSONObject`: `weight` e `style` ausentes não entram (o `JSON.stringify` omite `undefined`).
    pub fn to_json_object(&self) -> Value {
        let mut object = Map::new();
        if let Some(weight) = &self.weight {
            object.insert("weight".to_owned(), json!(weight));
        }
        if let Some(style) = &self.style {
            object.insert("style".to_owned(), json!(style));
        }
        let src = self.src.iter().map(|source| json!({ "format": source.format, "location": source.location.to_string(false).to_string_lossy() })).collect();
        object.insert("src".to_owned(), Value::Array(src));
        Value::Object(object)
    }

    /// `IconFontDefinition.fromJSONObject`: `None` se `src` não é uma lista de `{ format, location }` de textos.
    pub fn from_json_object(json: &Value) -> Option<IconFontDefinition> {
        let string_or_undef = |key: &str| json.get(key).and_then(Value::as_str).map(str::to_owned);
        let src = json.get("src")?.as_array()?;
        let text = |source: &Value, key: &str| source.get(key).and_then(Value::as_str).map(str::to_owned);
        let src = src
            .iter()
            .map(|source| Some(IconFontSource { format: text(source, "format")?, location: URI::parse(text(source, "location")?.as_str(), false) }))
            .collect::<Option<Vec<_>>>()?;
        Some(IconFontDefinition { weight: string_or_undef("weight"), style: string_or_undef("style"), src })
    }
}

/// `IIconRegistry`: a interface e a classe do original são o mesmo tipo (ver as divergências do módulo).
pub type IIconRegistry = IconRegistry;

/// O estado do registro, sob um `RefCell` (os ouvintes de `onDidChange` são chamados com ele solto).
struct State {
    /// `iconsById`, na ordem de registro.
    icons: Vec<IconContribution>,
    index_by_id: HashMap<String, usize>,
    /// `iconSchema.properties`.
    schema_properties: Map<String, Value>,
    /// `iconReferenceSchema.enum` e `enumDescriptions`.
    reference_enum: Vec<String>,
    reference_enum_descriptions: Vec<String>,
    /// `iconFontsById`.
    icon_fonts_by_id: HashMap<String, IconFontDefinition>,
}

impl State {
    fn get(&self, id: &str) -> Option<&IconContribution> {
        self.index_by_id.get(id).map(|&index| &self.icons[index])
    }

    /// O `classNames` de `toString`: segue os ícones herdados até uma definição própria.
    fn class_names(&self, contribution: &IconContribution) -> String {
        let mut current = Some(contribution);
        while let Some(icon) = current {
            match &icon.defaults {
                IconDefaults::ThemeIcon(inherited) => current = self.get(&inherited.id),
                IconDefaults::Definition(_) => return format!("codicon codicon-{}", icon.id),
            }
        }
        "codicon codicon-".to_owned()
    }
}

/// `IconRegistry`.
pub struct IconRegistry {
    on_did_change: Rc<Emitter<()>>,
    state: RefCell<State>,
}

/// Veracidade de uma cadeia em JavaScript (`if (description)`).
fn non_empty(text: Option<&str>) -> Option<&str> {
    text.filter(|text| !text.is_empty())
}

impl Default for IconRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl IconRegistry {
    /// Um registro vazio: sem os codicons nem os ícones comuns, que só o singleton ([`get_icon_registry`]) traz.
    pub fn new() -> Self {
        Self {
            on_did_change: without_disposable_tracker(Emitter::new),
            state: RefCell::new(State {
                icons: Vec::new(),
                index_by_id: HashMap::new(),
                schema_properties: Map::new(),
                reference_enum: Vec::new(),
                reference_enum_descriptions: Vec::new(),
                icon_fonts_by_id: HashMap::new(),
            }),
        }
    }

    pub fn on_did_change(&self) -> Event<()> {
        self.on_did_change.event()
    }

    /// `registerIcon`: registrar de novo um id existente só acrescenta a descrição, se ele não tinha. Devolve o
    /// [`ThemeIcon`] do id.
    pub fn register_icon(&self, id: &str, defaults: IconDefaults, description: Option<&str>, deprecation_message: Option<&str>) -> ThemeIcon {
        let changed = {
            let mut state = self.state.borrow_mut();
            match state.index_by_id.get(id).copied() {
                Some(index) => match non_empty(description) {
                    Some(description) if non_empty(state.icons[index].description.as_deref()).is_none() => {
                        state.icons[index].description = Some(description.to_owned());
                        if let Some(property) = state.schema_properties.get_mut(id) {
                            property["markdownDescription"] = json!(format!("{description} $({id})"));
                        }
                        if let Some(enum_index) = state.reference_enum.iter().position(|candidate| candidate == id) {
                            state.reference_enum_descriptions[enum_index] = description.to_owned();
                        }
                        true
                    }
                    _ => false,
                },
                None => {
                    let index = state.icons.len();
                    state.icons.push(IconContribution {
                        id: id.to_owned(),
                        description: description.map(str::to_owned),
                        deprecation_message: deprecation_message.map(str::to_owned),
                        defaults,
                    });
                    state.index_by_id.insert(id.to_owned(), index);
                    let mut property = json!({ "$ref": "#/definitions/icons" });
                    if let Some(deprecation_message) = non_empty(deprecation_message) {
                        property["deprecationMessage"] = json!(deprecation_message);
                    }
                    if let Some(description) = non_empty(description) {
                        property["markdownDescription"] = json!(format!("{description}: $({id})"));
                    }
                    state.schema_properties.insert(id.to_owned(), property);
                    state.reference_enum.push(id.to_owned());
                    state.reference_enum_descriptions.push(description.unwrap_or_default().to_owned());
                    true
                }
            }
        };
        if changed {
            self.on_did_change.fire(());
        }
        ThemeIcon::from_id(id)
    }

    /// `deregisterIcon`: dispara `onDidChange` mesmo que o id não exista.
    pub fn deregister_icon(&self, id: &str) {
        {
            let mut state = self.state.borrow_mut();
            if let Some(index) = state.index_by_id.remove(id) {
                state.icons.remove(index);
                for slot in state.index_by_id.values_mut().filter(|slot| **slot > index) {
                    *slot -= 1;
                }
            }
            state.schema_properties.shift_remove(id);
            if let Some(index) = state.reference_enum.iter().position(|candidate| candidate == id) {
                state.reference_enum.remove(index);
                state.reference_enum_descriptions.remove(index);
            }
        }
        self.on_did_change.fire(());
    }

    /// `getIcons`: todas as contribuições, na ordem de registro.
    pub fn get_icons(&self) -> Vec<IconContribution> {
        self.state.borrow().icons.clone()
    }

    /// `getIcon`.
    pub fn get_icon(&self, id: &str) -> Option<IconContribution> {
        self.state.borrow().get(id).cloned()
    }

    /// `getIconSchema`: o esquema do objeto que atribui valores a um dos ícones.
    pub fn get_icon_schema(&self) -> Value {
        json!({
            "definitions": {
                "icons": {
                    "type": "object",
                    "properties": {
                        "fontId": {
                            "type": "string",
                            "description": localize("iconDefinition.fontId", "The id of the font to use. If not set, the font that is defined first is used.", &[]),
                            "pattern": FONT_ID_REGEX,
                            "patternErrorMessage": font_id_error_message(),
                        },
                        "fontCharacter": {
                            "type": "string",
                            "description": localize("iconDefinition.fontCharacter", "The font character associated with the icon definition.", &[]),
                        },
                    },
                    "additionalProperties": false,
                    "defaultSnippets": [{ "body": { "fontCharacter": "\\\\e030" } }],
                },
            },
            "type": "object",
            "properties": Value::Object(self.state.borrow().schema_properties.clone()),
        })
    }

    /// `getIconReferenceSchema`: o esquema de uma referência a um ícone.
    pub fn get_icon_reference_schema(&self) -> Value {
        let state = self.state.borrow();
        json!({
            "type": "string",
            "pattern": format!("^{ICON_NAME_EXPRESSION}$"),
            "enum": state.reference_enum,
            "enumDescriptions": state.reference_enum_descriptions,
        })
    }

    /// `registerIconFont`: registrar de novo um id existente devolve a definição já registrada.
    pub fn register_icon_font(&self, id: &str, definition: IconFontDefinition) -> IconFontDefinition {
        {
            let mut state = self.state.borrow_mut();
            if let Some(existing) = state.icon_fonts_by_id.get(id) {
                return existing.clone();
            }
            state.icon_fonts_by_id.insert(id.to_owned(), definition.clone());
        }
        self.on_did_change.fire(());
        definition
    }

    /// `deregisterIconFont`: não dispara `onDidChange`.
    pub fn deregister_icon_font(&self, id: &str) {
        self.state.borrow_mut().icon_fonts_by_id.remove(id);
    }

    /// `getIconFont`.
    pub fn get_icon_font(&self, id: &str) -> Option<IconFontDefinition> {
        self.state.borrow().icon_fonts_by_id.get(id).cloned()
    }
}

/// `toString()`: a tabela de referência dos ícones em Markdown.
impl fmt::Display for IconRegistry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let state = self.state.borrow();
        fn sorted(mut contributions: Vec<&IconContribution>) -> Vec<&IconContribution> {
            contributions.sort_by(|a, b| locale_compare(&a.id, &b.id));
            contributions
        }

        let mut reference = vec![
            "| preview     | identifier                        | default codicon ID                | description".to_owned(),
            "| ----------- | --------------------------------- | --------------------------------- | --------------------------------- |".to_owned(),
        ];

        for icon in sorted(state.icons.iter().filter(|icon| non_empty(icon.description.as_deref()).is_some()).collect()) {
            let default_id = match &icon.defaults {
                IconDefaults::ThemeIcon(inherited) => inherited.id.as_str(),
                IconDefaults::Definition(_) => icon.id.as_str(),
            };
            reference.push(format!("|<i class=\"{}\"></i>|{}|{}|{}|", state.class_names(icon), icon.id, default_id, icon.description.as_deref().unwrap_or_default()));
        }

        reference.push("| preview     | identifier                        ".to_owned());
        reference.push("| ----------- | --------------------------------- |".to_owned());

        for icon in sorted(state.icons.iter().filter(|icon| matches!(icon.defaults, IconDefaults::Definition(_))).collect()) {
            reference.push(format!("|<i class=\"{}\"></i>|{}|", state.class_names(icon), icon.id));
        }

        f.write_str(&reference.join("\n"))
    }
}

/// `initialize()`: um ícone por codicon, com a definição `\<code point em hexadecimal>` na fonte padrão.
fn initialize(registry: &IconRegistry) {
    for id in je_codicons::ids() {
        if let Some(code_point) = je_codicons::code_point(id) {
            let font_character = format!("\\{:x}", u32::from(code_point));
            registry.register_icon(id, IconDefaults::Definition(IconDefinition { font: None, font_character }), None, None);
        }
    }
}

/// `common icons`: os ícones registrados no fim do arquivo.
fn register_common_icons(registry: &IconRegistry) {
    let register = |id: &str, codicon: &str, description: String| {
        registry.register_icon(id, IconDefaults::ThemeIcon(ThemeIcon::from_id(codicon)), Some(&description), None);
    };
    register(WIDGET_CLOSE_ID, "close", localize("widgetClose", "Icon for the close action in widgets.", &[]));
    register(GOTO_PREVIOUS_LOCATION_ID, "arrow-up", localize("previousChangeIcon", "Icon for goto previous editor location.", &[]));
    register(GOTO_NEXT_LOCATION_ID, "arrow-down", localize("nextChangeIcon", "Icon for goto next editor location.", &[]));
}

/// Atraso do `RunOnceScheduler` que avisa a mudança do esquema de ícones.
const ICONS_SCHEMA_NOTIFY_DELAY_MS: u64 = 200;

/// Refaz o objeto de esquema registrado a partir do esquema atual do registro, no próprio lugar.
fn refresh_icons_schema(target: &ObjectRef, registry: &IconRegistry) {
    if let Dynamic::Object(fresh) = schema_from_json(&registry.get_icon_schema()) {
        for (key, value) in fresh.entries() {
            target.set(&key, value);
        }
    }
}

/// O trecho de `iconRegistry.ts` que registra `iconsSchemaId` no `jsonContributionRegistry` e agenda o
/// `notifySchemaChanged` a cada `onDidChange`.
fn register_icons_schema(registry: &IconRegistry) {
    let schema = ObjectRef::new();
    refresh_icons_schema(&schema, registry);
    get_json_contribution_registry().register_schema(ICONS_SCHEMA_ID, Dynamic::Object(schema.clone()), None);

    let delayer = RunOnceScheduler::new(
        move || {
            refresh_icons_schema(&schema, &get_icon_registry());
            get_json_contribution_registry().notify_schema_changed(ICONS_SCHEMA_ID);
        },
        ICONS_SCHEMA_NOTIFY_DELAY_MS,
    );
    registry.on_did_change().listen(move |_| {
        if !delayer.is_scheduled() {
            delayer.schedule(None);
        }
    });
}

thread_local! {
    static ICON_REGISTRY: Rc<IconRegistry> = {
        let registry = IconRegistry::new();
        initialize(&registry);
        register_icons_schema(&registry);
        register_common_icons(&registry);
        Rc::new(registry)
    };
}

/// `registerIcon(id, defaults, description, deprecationMessage)` do módulo: registra no singleton.
pub fn register_icon(id: &str, defaults: impl Into<IconDefaults>, description: &str, deprecation_message: Option<&str>) -> ThemeIcon {
    get_icon_registry().register_icon(id, defaults.into(), Some(description), deprecation_message)
}

/// `getIconRegistry()`.
pub fn get_icon_registry() -> Rc<IconRegistry> {
    ICON_REGISTRY.with(Rc::clone)
}

/// O ícone de um id que o singleton registra ao ser criado.
fn common_icon(id: &str) -> ThemeIcon {
    get_icon_registry();
    ThemeIcon::from_id(id)
}

/// `widgetClose`.
pub fn widget_close() -> ThemeIcon {
    common_icon(WIDGET_CLOSE_ID)
}

/// `gotoPreviousLocation`.
pub fn goto_previous_location() -> ThemeIcon {
    common_icon(GOTO_PREVIOUS_LOCATION_ID)
}

/// `gotoNextLocation`.
pub fn goto_next_location() -> ThemeIcon {
    common_icon(GOTO_NEXT_LOCATION_ID)
}

/// `syncing`.
pub fn syncing() -> ThemeIcon {
    ThemeIcon::modify(&ThemeIcon::from_id("sync"), Some(IconModifier::Spin))
}

/// `spinningLoading`.
pub fn spinning_loading() -> ThemeIcon {
    ThemeIcon::modify(&ThemeIcon::from_id("loading"), Some(IconModifier::Spin))
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    fn close() -> ThemeIcon {
        ThemeIcon::from_id("close")
    }

    #[test]
    fn codicons_are_registered_with_their_font_character() {
        let registry = get_icon_registry();
        let add = registry.get_icon("add").expect("o codicon add está registrado");
        match add.defaults {
            IconDefaults::Definition(definition) => {
                assert!(definition.font.is_none());
                assert_eq!(definition.font_character, "\\ea60");
            }
            IconDefaults::ThemeIcon(_) => panic!("o codicon tem definição própria"),
        }
    }

    #[test]
    fn common_icons_resolve_to_the_codicon_definition() {
        let registry = get_icon_registry();
        let contribution = registry.get_icon(&widget_close().id).expect("widget-close registrado");
        assert_eq!(contribution.description.as_deref(), Some("Icon for the close action in widgets."));
        let inherited = contribution.get_definition(&registry).expect("herda a definição de close");
        let direct = registry.get_icon("close").and_then(|c| c.get_definition(&registry)).expect("close tem definição");
        assert_eq!(inherited.font_character, direct.font_character);
        assert_eq!(goto_previous_location().id, "goto-previous-location");
        assert_eq!(goto_next_location().id, "goto-next-location");
    }

    #[test]
    fn spinning_icons_carry_the_spin_modifier() {
        assert_eq!(syncing().id, "sync~spin");
        assert_eq!(spinning_loading().id, "loading~spin");
    }

    #[test]
    fn registering_again_only_adds_a_missing_description() {
        let registry = IconRegistry::new();
        let changes = Rc::new(Cell::new(0));
        let counter = changes.clone();
        let _subscription = registry.on_did_change().listen(move |_| counter.set(counter.get() + 1));

        let icon = registry.register_icon("my-icon", IconDefaults::ThemeIcon(close()), None, None);
        assert_eq!(icon.id, "my-icon");
        assert_eq!(changes.get(), 1);

        registry.register_icon("my-icon", IconDefaults::ThemeIcon(close()), Some("Meu ícone"), None);
        assert_eq!(changes.get(), 2);
        assert_eq!(registry.get_icon("my-icon").and_then(|c| c.description).as_deref(), Some("Meu ícone"));

        registry.register_icon("my-icon", IconDefaults::ThemeIcon(close()), Some("Outra"), None);
        assert_eq!(changes.get(), 2);
        assert_eq!(registry.get_icon("my-icon").and_then(|c| c.description).as_deref(), Some("Meu ícone"));

        let schema = registry.get_icon_schema();
        assert_eq!(schema["properties"]["my-icon"]["markdownDescription"], "Meu ícone $(my-icon)");
        let reference = registry.get_icon_reference_schema();
        assert_eq!(reference["enum"], json!(["my-icon"]));
        assert_eq!(reference["enumDescriptions"], json!(["Meu ícone"]));
    }

    #[test]
    fn deregistering_removes_the_icon_from_the_schemas() {
        let registry = IconRegistry::new();
        registry.register_icon("a", IconDefaults::ThemeIcon(close()), Some("A"), None);
        registry.register_icon("b", IconDefaults::ThemeIcon(close()), Some("B"), Some("obsoleto"));
        assert_eq!(registry.get_icon_schema()["properties"]["b"]["deprecationMessage"], "obsoleto");
        registry.deregister_icon("a");
        assert!(registry.get_icon("a").is_none());
        assert_eq!(registry.get_icons().len(), 1);
        assert_eq!(registry.get_icon("b").map(|c| c.id).as_deref(), Some("b"));
        assert_eq!(registry.get_icon_reference_schema()["enum"], json!(["b"]));
        assert!(registry.get_icon_schema()["properties"].get("a").is_none());
    }

    #[test]
    fn icon_fonts_are_registered_once() {
        let registry = IconRegistry::new();
        let definition = IconFontDefinition {
            weight: Some("normal".to_owned()),
            style: None,
            src: vec![IconFontSource { location: URI::parse("https://example.com/font.woff", false), format: "woff".to_owned() }],
        };
        registry.register_icon_font("my-font", definition.clone());
        let other = IconFontDefinition { weight: None, ..definition.clone() };
        let existing = registry.register_icon_font("my-font", other);
        assert_eq!(existing.weight.as_deref(), Some("normal"));
        assert!(registry.get_icon_font("my-font").is_some());
        registry.deregister_icon_font("my-font");
        assert!(registry.get_icon_font("my-font").is_none());
    }

    #[test]
    fn icon_font_definition_round_trips_through_json() {
        let json = json!({ "weight": "bold", "src": [{ "format": "woff", "location": "https://example.com/font.woff" }] });
        let definition = IconFontDefinition::from_json_object(&json).expect("definição válida");
        assert_eq!(definition.weight.as_deref(), Some("bold"));
        assert_eq!(definition.style, None);
        assert_eq!(definition.to_json_object(), json);
        assert!(IconFontDefinition::from_json_object(&json!({ "src": [{ "format": "woff" }] })).is_none());
        assert!(IconFontDefinition::from_json_object(&json!({})).is_none());
    }

    #[test]
    fn display_lists_described_icons_first() {
        let registry = IconRegistry::new();
        registry.register_icon("b-icon", IconDefaults::ThemeIcon(close()), Some("B"), None);
        registry.register_icon("a-icon", IconDefaults::ThemeIcon(close()), Some("A"), None);
        registry.register_icon("close", IconDefaults::Definition(IconDefinition { font: None, font_character: "\\ea76".to_owned() }), None, None);
        let text = registry.to_string();
        let a = text.find("|a-icon|").expect("a-icon na tabela");
        let b = text.find("|b-icon|").expect("b-icon na tabela");
        assert!(a < b);
        assert!(text.contains("|<i class=\"codicon codicon-close\"></i>|a-icon|close|A|"));
        assert!(text.ends_with("|<i class=\"codicon codicon-close\"></i>|close|"));
    }
}
