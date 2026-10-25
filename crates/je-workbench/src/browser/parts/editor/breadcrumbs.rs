//! Porte de `src/vs/workbench/browser/parts/editor/breadcrumbs.ts`: a configuração das migalhas
//! (`BreadcrumbsConfig`) e o registro das suas definições (`breadcrumbs.*`).
//!
//! Notas de tradução:
//! - **`BreadcrumbsConfig<T>` e os `static readonly X = BreadcrumbsConfig._stub<T>(name)`.** Cada chave estática é um
//!   [`BreadcrumbsConfigKey<T>`] (as constantes de [`BreadcrumbsConfigKeys`]), cujo `bindTo(service)` é
//!   [`BreadcrumbsConfigKey::bind_to`] e devolve o [`BreadcrumbsConfig<T>`] ligado ao serviço. O `T` do original é só
//!   anotação de tipo; aqui é o [`ConfigValue`], que converte de e para o valor dinâmico da configuração. Como o serviço
//!   devolve `undefined` quando a chave não existe, `getValue` é `Option<T>` (e um valor de outro tipo também é `None`).
//! - **Textos de união.** `'on' | 'off' | 'last'` é [`ShowPath`], e `'position' | 'name' | 'type'` é
//!   [`SymbolSortOrder`]. `IEditorPartOptions['titleScrollbarSizing']` e `['titleScrollbarVisibility']` (que o
//!   `IEditorPartOptions` ainda não tem aqui) são o texto, e `glob.IExpression` (`files.exclude`) é o valor dinâmico.
//! - **`IBreadcrumbsService` e `BreadcrumbsService`.** Guardam `BreadcrumbsWidget` por grupo de editor, e o widget é a
//!   árvore de widgets nativa (`je-widgets`): ficam com o desenho das migalhas, não com o modelo.
//! - **`Registry.as<IConfigurationRegistry>(...).registerConfiguration(...)`** é [`register_configuration`]. As 26
//!   definições `breadcrumbs.show*` são um laço sobre os tipos de símbolo (a mesma mensagem com o nome do tipo), na ordem
//!   do original.

use std::marker::PhantomData;
use std::rc::Rc;

use je_base::dynamic::Dynamic;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{DisposableRef, IDisposable};
use je_base::nls::localize;
use je_base::promise::Promise;
use je_platform::configuration::models::ConfigurationChangeEvent;
use je_platform::configuration::registry::{ConfigurationNode, ConfigurationNodeData, ConfigurationScope, PropertySchema, configuration_registry};
use je_platform::configuration::service::{IConfigurationService, update_overrides_of};
use je_platform::configuration::ConfigurationOverrides;

/// O que uma chave de configuração das migalhas sabe converter: de e para o valor dinâmico do serviço.
pub trait ConfigValue: Sized {
    /// O valor tipado, ou `None` se o dinâmico não for do tipo (ou for `undefined`).
    fn from_dynamic(value: &Dynamic) -> Option<Self>;

    fn to_dynamic(&self) -> Dynamic;
}

impl ConfigValue for bool {
    fn from_dynamic(value: &Dynamic) -> Option<Self> {
        match value {
            Dynamic::Bool(flag) => Some(*flag),
            _ => None,
        }
    }

    fn to_dynamic(&self) -> Dynamic {
        Dynamic::Bool(*self)
    }
}

impl ConfigValue for String {
    fn from_dynamic(value: &Dynamic) -> Option<Self> {
        match value {
            Dynamic::String(text) => Some(text.to_string_lossy()),
            _ => None,
        }
    }

    fn to_dynamic(&self) -> Dynamic {
        Dynamic::string(self)
    }
}

/// `glob.IExpression` (`files.exclude`): o valor como veio, `None` só se for `undefined`.
impl ConfigValue for Dynamic {
    fn from_dynamic(value: &Dynamic) -> Option<Self> {
        (!matches!(value, Dynamic::Undefined)).then(|| value.clone())
    }

    fn to_dynamic(&self) -> Dynamic {
        self.clone()
    }
}

/// Declara um tipo de união de textos (`'a' | 'b'`) e o liga ao [`ConfigValue`].
macro_rules! string_union {
    ($(#[$meta:meta])* $name:ident { $($variant:ident => $text:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum $name {
            $($variant),+
        }

        impl $name {
            /// O texto do original.
            pub fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $text),+
                }
            }
        }

        impl ConfigValue for $name {
            fn from_dynamic(value: &Dynamic) -> Option<Self> {
                match value {
                    Dynamic::String(text) => match text.to_string_lossy().as_str() {
                        $($text => Some(Self::$variant),)+
                        _ => None,
                    },
                    _ => None,
                }
            }

            fn to_dynamic(&self) -> Dynamic {
                Dynamic::string(self.as_str())
            }
        }
    };
}

string_union! {
    /// `'on' | 'off' | 'last'` (`breadcrumbs.filePath` e `breadcrumbs.symbolPath`).
    ShowPath { On => "on", Off => "off", Last => "last" }
}

string_union! {
    /// `'position' | 'name' | 'type'` (`breadcrumbs.symbolSortOrder`).
    SymbolSortOrder { Position => "position", Name => "name", Type => "type" }
}

/// `BreadcrumbsConfig._stub<T>(name)`: a chave estática, que se liga a um serviço de configuração.
pub struct BreadcrumbsConfigKey<T> {
    name: &'static str,
    value_type: PhantomData<fn() -> T>,
}

impl<T: ConfigValue + 'static> BreadcrumbsConfigKey<T> {
    pub const fn new(name: &'static str) -> Self {
        Self { name, value_type: PhantomData }
    }

    /// `bindTo(service)`: o [`BreadcrumbsConfig`] que dispara `onDidChange` quando a chave muda no serviço.
    pub fn bind_to(&self, service: Rc<dyn IConfigurationService>) -> Rc<BreadcrumbsConfig<T>> {
        let on_did_change: Rc<Emitter<()>> = Emitter::new();
        let name = self.name;

        let notify = on_did_change.clone();
        let listener = service.on_did_change_configuration().listen(move |event: &Rc<ConfigurationChangeEvent>| {
            if event.affects_configuration(name, None) {
                notify.fire(());
            }
        });

        Rc::new(BreadcrumbsConfig { name, service, on_did_change, listener, value_type: PhantomData })
    }
}

/// `BreadcrumbsConfig<T>`: uma chave ligada a um serviço de configuração.
pub struct BreadcrumbsConfig<T> {
    name: &'static str,
    service: Rc<dyn IConfigurationService>,
    on_did_change: Rc<Emitter<()>>,
    listener: DisposableRef,
    value_type: PhantomData<fn() -> T>,
}

impl<T: ConfigValue> BreadcrumbsConfig<T> {
    /// `name`.
    pub fn name(&self) -> &'static str {
        self.name
    }

    /// `onDidChange`.
    pub fn on_did_change(&self) -> Event<()> {
        self.on_did_change.event()
    }

    /// `getValue(overrides?)`.
    pub fn get_value(&self, overrides: Option<&ConfigurationOverrides>) -> Option<T> {
        T::from_dynamic(&self.service.get_value(Some(self.name), overrides))
    }

    /// `updateValue(value, overrides?)`.
    pub fn update_value(&self, value: &T, overrides: Option<&ConfigurationOverrides>) -> Promise<()> {
        self.service.update_value(self.name, value.to_dynamic(), overrides.map(update_overrides_of), None)
    }
}

impl<T> IDisposable for BreadcrumbsConfig<T> {
    fn dispose(&self) {
        self.listener.dispose();
        self.on_did_change.dispose();
    }
}

/// Os `static readonly` de `BreadcrumbsConfig`.
pub struct BreadcrumbsConfigKeys;

impl BreadcrumbsConfigKeys {
    pub const IS_ENABLED: BreadcrumbsConfigKey<bool> = BreadcrumbsConfigKey::new("breadcrumbs.enabled");
    pub const USE_QUICK_PICK: BreadcrumbsConfigKey<bool> = BreadcrumbsConfigKey::new("breadcrumbs.useQuickPick");
    pub const FILE_PATH: BreadcrumbsConfigKey<ShowPath> = BreadcrumbsConfigKey::new("breadcrumbs.filePath");
    pub const SYMBOL_PATH: BreadcrumbsConfigKey<ShowPath> = BreadcrumbsConfigKey::new("breadcrumbs.symbolPath");
    pub const SYMBOL_SORT_ORDER: BreadcrumbsConfigKey<SymbolSortOrder> = BreadcrumbsConfigKey::new("breadcrumbs.symbolSortOrder");
    pub const SYMBOL_PATH_SEPARATOR: BreadcrumbsConfigKey<String> = BreadcrumbsConfigKey::new("breadcrumbs.symbolPathSeparator");
    pub const ICONS: BreadcrumbsConfigKey<bool> = BreadcrumbsConfigKey::new("breadcrumbs.icons");
    pub const SHOW_EDITOR_TYPE: BreadcrumbsConfigKey<bool> = BreadcrumbsConfigKey::new("breadcrumbs.showEditorType");
    /// `IEditorPartOptions['titleScrollbarSizing']`.
    pub const TITLE_SCROLLBAR_SIZING: BreadcrumbsConfigKey<String> = BreadcrumbsConfigKey::new("workbench.editor.titleScrollbarSizing");
    /// `IEditorPartOptions['titleScrollbarVisibility']`.
    pub const TITLE_SCROLLBAR_VISIBILITY: BreadcrumbsConfigKey<String> = BreadcrumbsConfigKey::new("workbench.editor.titleScrollbarVisibility");
    /// `glob.IExpression`.
    pub const FILE_EXCLUDES: BreadcrumbsConfigKey<Dynamic> = BreadcrumbsConfigKey::new("files.exclude");
}

/// Os tipos de símbolo que as definições `breadcrumbs.show*` ligam e desligam: o sufixo do nome da definição e o nome do
/// tipo na mensagem, na ordem do original.
const SYMBOL_KINDS: [(&str, &str); 26] = [
    ("Files", "file"),
    ("Modules", "module"),
    ("Namespaces", "namespace"),
    ("Packages", "package"),
    ("Classes", "class"),
    ("Methods", "method"),
    ("Properties", "property"),
    ("Fields", "field"),
    ("Constructors", "constructor"),
    ("Enums", "enum"),
    ("Interfaces", "interface"),
    ("Functions", "function"),
    ("Variables", "variable"),
    ("Constants", "constant"),
    ("Strings", "string"),
    ("Numbers", "number"),
    ("Booleans", "boolean"),
    ("Arrays", "array"),
    ("Objects", "object"),
    ("Keys", "key"),
    ("Null", "null"),
    ("EnumMembers", "enumMember"),
    ("Structs", "struct"),
    ("Events", "event"),
    ("Operators", "operator"),
    ("TypeParameters", "typeParameter"),
];

fn texts(items: &[&str]) -> Dynamic {
    Dynamic::array(items.iter().map(|item| Dynamic::string(item)))
}

/// `{ description, type, default, ...extra }`, com os campos de cada definição na ordem em que o original os escreve.
fn property(entries: Vec<(&str, Dynamic)>) -> PropertySchema {
    PropertySchema::from_entries(entries)
}

/// A definição `breadcrumbs.show<Kind>`: um booleano ligado por padrão, configurável por linguagem.
fn show_symbols_property(kind: &str) -> PropertySchema {
    let message = format!("When enabled breadcrumbs show `{kind}`-symbols.");
    property(vec![
        ("type", Dynamic::string("boolean")),
        ("default", Dynamic::Bool(true)),
        ("scope", ConfigurationScope::LanguageOverridable.to_dynamic()),
        ("markdownDescription", Dynamic::string(&localize(&format!("filteredTypes.{kind}"), &message, &[]))),
    ])
}

/// O `registerConfiguration` do fim do arquivo original: as definições `breadcrumbs.*`.
pub fn register_configuration() {
    let on_off_last = || texts(&["on", "off", "last"]);

    let mut properties: Vec<(String, PropertySchema)> = vec![
        (
            "breadcrumbs.enabled".to_owned(),
            property(vec![
                ("description", Dynamic::string(&localize("enabled", "Enable/disable navigation breadcrumbs.", &[]))),
                ("type", Dynamic::string("boolean")),
                ("default", Dynamic::Bool(true)),
                ("agentsWindow", Dynamic::object([("default", Dynamic::Bool(true))])),
            ]),
        ),
        (
            "breadcrumbs.filePath".to_owned(),
            property(vec![
                ("description", Dynamic::string(&localize("filepath", "Controls whether and how file paths are shown in the breadcrumbs view.", &[]))),
                ("type", Dynamic::string("string")),
                ("default", Dynamic::string("on")),
                ("enum", on_off_last()),
                (
                    "enumDescriptions",
                    Dynamic::array([
                        Dynamic::string(&localize("filepath.on", "Show the file path in the breadcrumbs view.", &[])),
                        Dynamic::string(&localize("filepath.off", "Do not show the file path in the breadcrumbs view.", &[])),
                        Dynamic::string(&localize("filepath.last", "Only show the last element of the file path in the breadcrumbs view.", &[])),
                    ]),
                ),
            ]),
        ),
        (
            "breadcrumbs.symbolPath".to_owned(),
            property(vec![
                ("description", Dynamic::string(&localize("symbolpath", "Controls whether and how symbols are shown in the breadcrumbs view.", &[]))),
                ("type", Dynamic::string("string")),
                ("default", Dynamic::string("on")),
                ("enum", on_off_last()),
                (
                    "enumDescriptions",
                    Dynamic::array([
                        Dynamic::string(&localize("symbolpath.on", "Show all symbols in the breadcrumbs view.", &[])),
                        Dynamic::string(&localize("symbolpath.off", "Do not show symbols in the breadcrumbs view.", &[])),
                        Dynamic::string(&localize("symbolpath.last", "Only show the current symbol in the breadcrumbs view.", &[])),
                    ]),
                ),
            ]),
        ),
        (
            "breadcrumbs.symbolSortOrder".to_owned(),
            property(vec![
                ("description", Dynamic::string(&localize("symbolSortOrder", "Controls how symbols are sorted in the breadcrumbs outline view.", &[]))),
                ("type", Dynamic::string("string")),
                ("default", Dynamic::string("position")),
                ("scope", ConfigurationScope::LanguageOverridable.to_dynamic()),
                ("enum", texts(&["position", "name", "type"])),
                (
                    "enumDescriptions",
                    Dynamic::array([
                        Dynamic::string(&localize("symbolSortOrder.position", "Show symbol outline in file position order.", &[])),
                        Dynamic::string(&localize("symbolSortOrder.name", "Show symbol outline in alphabetical order.", &[])),
                        Dynamic::string(&localize("symbolSortOrder.type", "Show symbol outline in symbol type order.", &[])),
                    ]),
                ),
            ]),
        ),
        (
            "breadcrumbs.icons".to_owned(),
            property(vec![
                ("description", Dynamic::string(&localize("icons", "Render breadcrumb items with icons.", &[]))),
                ("type", Dynamic::string("boolean")),
                ("default", Dynamic::Bool(true)),
            ]),
        ),
        (
            "breadcrumbs.showEditorType".to_owned(),
            property(vec![
                (
                    "markdownDescription",
                    Dynamic::string(&localize(
                        "showEditorType",
                        "Controls whether the breadcrumbs bar shows a dropdown to switch between the editors that can open the current file (for example the text editor and a custom editor). The dropdown only appears when a more specialized editor is available.",
                        &[],
                    )),
                ),
                ("type", Dynamic::string("boolean")),
                ("default", Dynamic::Bool(true)),
                ("tags", texts(&["experimental"])),
            ]),
        ),
        (
            "breadcrumbs.symbolPathSeparator".to_owned(),
            property(vec![
                ("description", Dynamic::string(&localize("symbolPathSeparator", "The separator used when copying the breadcrumb symbol path.", &[]))),
                ("type", Dynamic::string("string")),
                ("default", Dynamic::string(".")),
                ("scope", ConfigurationScope::LanguageOverridable.to_dynamic()),
            ]),
        ),
    ];

    properties.extend(SYMBOL_KINDS.iter().map(|(suffix, kind)| (format!("breadcrumbs.show{suffix}"), show_symbols_property(kind))));

    configuration_registry().register_configuration(ConfigurationNode::new(
        ConfigurationNodeData {
            id: Some("breadcrumbs".to_owned()),
            title: Some(localize("title", "Breadcrumb Navigation", &[])),
            order: Some(101.0),
            type_: Some(Dynamic::string("object")),
            ..Default::default()
        }
        .with_properties(properties),
    ));
}

/// `IBreadcrumbsService`: o widget de migalhas de cada grupo de editor.
pub trait IBreadcrumbsService {
    /// `register(group, widget)`: falha se o grupo já tem um widget.
    fn register(&self, group: crate::common::editor::GroupIdentifier, widget: Rc<je_widgets::breadcrumbs_widget::BreadcrumbsWidget>) -> DisposableRef;

    /// `getWidget(group)`.
    fn get_widget(&self, group: crate::common::editor::GroupIdentifier) -> Option<Rc<je_widgets::breadcrumbs_widget::BreadcrumbsWidget>>;
}

je_instantiation::service_id!(pub I_BREADCRUMBS_SERVICE: dyn IBreadcrumbsService = "IEditorBreadcrumbsService");

type WidgetMap = std::cell::RefCell<std::collections::HashMap<crate::common::editor::GroupIdentifier, Rc<je_widgets::breadcrumbs_widget::BreadcrumbsWidget>>>;

/// `BreadcrumbsService`.
#[derive(Default)]
pub struct BreadcrumbsService {
    map: Rc<WidgetMap>,
}

impl BreadcrumbsService {
    pub fn new() -> Self {
        Self::default()
    }
}

impl IBreadcrumbsService for BreadcrumbsService {
    fn register(&self, group: crate::common::editor::GroupIdentifier, widget: Rc<je_widgets::breadcrumbs_widget::BreadcrumbsWidget>) -> DisposableRef {
        if self.map.borrow().contains_key(&group) {
            je_base::errors::throw(je_base::errors::Error::new(format!("group ({group}) has already a widget")));
        }
        self.map.borrow_mut().insert(group, widget);
        let map = self.map.clone();
        je_base::lifecycle::to_disposable(move || {
            map.borrow_mut().remove(&group);
        })
    }

    fn get_widget(&self, group: crate::common::editor::GroupIdentifier) -> Option<Rc<je_widgets::breadcrumbs_widget::BreadcrumbsWidget>> {
        self.map.borrow().get(&group).cloned()
    }
}
