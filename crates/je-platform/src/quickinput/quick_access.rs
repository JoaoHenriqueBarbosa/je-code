//! Porte de `src/vs/platform/quickinput/common/quickAccess.ts`: o que os provedores de acesso rápido leem
//! (`IQuickAccessProviderRunOptions`, `IQuickAccessProvider` e `DefaultQuickAccessFilterValue`), as opções de uso do
//! controlador (`IQuickAccessOptions`), o contrato do controlador (`IQuickAccessController`) e o registro de provedores
//! (`IQuickAccessProviderDescriptor`, `IQuickAccessRegistry`, `QuickAccessRegistry` e `Extensions.Quickaccess`). O controlador
//! em si (`browser/quickAccess.ts`) está em [`super::quick_access_controller`].
//!
//! Notas de tradução:
//! - **`provide(picker, token, options)`.** Cada provedor define o próprio tipo de opções de execução (o original estende
//!   `IQuickAccessProviderRunOptions`), então o trait é genérico em `O`, que pode ser `dyn Any` (o registro e o controlador
//!   só repassam as opções, sem lê-las).
//! - **`handleAccept`.** É o `Rc<dyn Fn(&IQuickPickItem, bool)>` compartilhado.
//! - **`defaultFilterValue?: string | DefaultQuickAccessFilterValue`.** A união vira dois métodos do provedor:
//!   [`IQuickAccessProvider::default_filter_value`] (o enum) e [`IQuickAccessProvider::default_filter_text`] (a `string`).
//!   Cada provedor devolve no máximo um dos dois, como o original devolve um valor só.
//! - **`isDisposable(provider)`.** É [`IQuickAccessProvider::as_disposable`], que um provedor com `dispose` sobrescreve.
//! - **`ctor`.** O construtor do descritor (`new(...services)`) é o [`SyncDescriptor`], que o `createInstance` do controlador
//!   instancia com os serviços injetados. Descritores são compartilhados por identidade (`Rc`), porque o controlador e o
//!   registro os comparam com `===`.
//! - **`anchor`.** É só o `{ x, y }` (não há `HTMLElement`), como em [`QuickInputAnchor`].
//! - **`Registry.add(Extensions.Quickaccess, new QuickAccessRegistry())`.** Roda na primeira chamada de
//!   [`quick_access_registry`], que devolve o `Registry.as<IQuickAccessRegistry>(Extensions.Quickaccess)` da thread.
//! - **`clear()`.** Devolve o `Function` que restaura o estado anterior; pode ser chamado mais de uma vez.
//! - **`providers.splice(providers.indexOf(provider), 1)`.** Ao descartar o provedor padrão (que não está na lista), o
//!   `indexOf` dá `-1` e o `splice(-1, 1)` do original remove o último provedor da lista. O porte mantém esse efeito.

use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;

use je_base::cancellation::CancellationToken;
use je_base::lifecycle::{DisposableRef, IDisposable, to_disposable};
use je_base::promise::Promise;
use je_instantiation::SyncDescriptor;

use super::quick_input::{IQuickNavigateConfiguration, IQuickPick, IQuickPickItem, ItemActivation, QuickInputAnchor, QuickInputAnchorPosition};
use crate::contextkey::contextkey::{ContextKeyExpression, IContextKeyService};
use crate::registry::registry;

/// `DefaultQuickAccessFilterValue`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DefaultQuickAccessFilterValue {
    /// Keep the value as it is given to quick access.
    Preserve = 0,
    /// Use the value that was used last time something was accepted from the picker.
    Last = 1,
}

/// `IQuickAccessProviderRunOptions`.
#[derive(Clone, Default)]
pub struct IQuickAccessProviderRunOptions {
    pub from: Option<String>,
    pub placeholder: Option<String>,
    /// A handler to invoke when an item is accepted for this particular showing of the quick access.
    pub handle_accept: Option<Rc<dyn Fn(&IQuickPickItem, bool)>>,
}

/// O acesso às opções base de uma estrutura de opções que a estende.
pub trait AsQuickAccessRunOptions {
    fn base(&self) -> &IQuickAccessProviderRunOptions;
}

impl AsQuickAccessRunOptions for IQuickAccessProviderRunOptions {
    fn base(&self) -> &IQuickAccessProviderRunOptions {
        self
    }
}

/// `IQuickAccessOptions`.
#[derive(Clone, Default)]
pub struct IQuickAccessOptions {
    /// Allows to enable quick navigate support in quick input.
    pub provider_prefix: Option<String>,
    /// An anchor for the quick access.
    pub anchor: Option<QuickInputAnchor>,
    /// The position of the quick access relative to the anchor.
    pub anchor_position: Option<QuickInputAnchorPosition>,
    /// Allows to enable quick navigate support in quick input.
    pub quick_navigate_configuration: Option<IQuickNavigateConfiguration>,
    /// Allows to configure a different item activation strategy. By default the first item of the list will get activated.
    pub item_activation: Option<ItemActivation>,
    /// Whether to take the input value as is and not restore it from any existing value if quick access is visible.
    pub preserve_value: Option<bool>,
    /// Provider specific options for this particular showing of the quick access.
    pub provider_options: Option<Rc<dyn Any>>,
    /// An array of provider prefixes to enable. If undefined, all providers are enabled.
    pub enabled_provider_prefixes: Option<Vec<String>>,
    /// A placeholder to use for the input field when quick access is opened.
    pub placeholder: Option<String>,
}

/// `IQuickAccessController`.
pub trait IQuickAccessController {
    /// Open the quick access picker with the optional value prefilled.
    fn show(&self, value: &str, options: Option<IQuickAccessOptions>);

    /// Same as `show()` but instead of executing the selected pick item, it will be returned. May return `None` in case
    /// of cancellation or when the quick access was already showing for the prefix (o `undefined` do original).
    fn pick(&self, value: &str, options: Option<IQuickAccessOptions>) -> Promise<Option<Vec<IQuickPickItem>>>;
}

/// `IQuickAccessProvider`.
pub trait IQuickAccessProvider<O: ?Sized> {
    /// What to do when the user changes the quick access value (mostly for the default filter value).
    fn default_filter_value(&self) -> Option<DefaultQuickAccessFilterValue> {
        None
    }

    /// A `string` como `defaultFilterValue`: o valor padrão do filtro, que o controlador escreve depois do prefixo.
    fn default_filter_text(&self) -> Option<String> {
        None
    }

    /// O provedor como `IDisposable`, se tem `dispose` (`isDisposable(provider)`); o controlador o descarta junto com ele.
    fn as_disposable(&self) -> Option<&dyn IDisposable> {
        None
    }

    /// Called whenever a quick access provider is needed to populate the picker.
    ///
    /// Returns a disposable that will automatically be disposed when the picker closes or is replaced by another picker.
    fn provide(&self, picker: Rc<dyn IQuickPick>, token: CancellationToken, options: Option<Rc<O>>) -> DisposableRef;
}

/// `IQuickAccessProviderHelp`.
#[derive(Clone, Default)]
pub struct IQuickAccessProviderHelp {
    /// The prefix to show for the help entry. If not provided, the prefix used for registration will be taken.
    pub prefix: Option<String>,
    /// A description text to help understand the intent of the provider.
    pub description: String,
    /// The command to bring up this quick access provider.
    pub command_id: Option<String>,
    /// Order of the command in the command center.
    pub command_center_order: Option<f64>,
    /// Label of the command in the command center.
    pub command_center_label: Option<String>,
}

/// `IQuickAccessProviderDescriptor`.
#[derive(Clone)]
pub struct IQuickAccessProviderDescriptor {
    /// The actual provider that will be instantiated as needed.
    pub ctor: SyncDescriptor<dyn IQuickAccessProvider<dyn Any>>,
    /// The prefix for quick access picks to use the provider for.
    pub prefix: String,
    /// A placeholder to use for the input field when the provider is active.
    pub placeholder: Option<String>,
    /// Documentation for the provider in the quick access help.
    pub help_entries: Vec<IQuickAccessProviderHelp>,
    /// A context key that will be set automatically when the picker for the provider is showing.
    pub context_key: Option<String>,
    /// A condition that must be met for the provider to be available.
    pub when: Option<ContextKeyExpression>,
}

/// `Extensions.Quickaccess`.
pub const EXTENSIONS_QUICKACCESS: &str = "workbench.contributions.quickaccess";

/// `IQuickAccessRegistry`.
pub trait IQuickAccessRegistry {
    /// Registers a quick access provider to the platform.
    fn register_quick_access_provider(&self, provider: Rc<IQuickAccessProviderDescriptor>) -> DisposableRef;

    /// Get all registered quick access providers.
    fn get_quick_access_providers(&self, context_key_service: &dyn IContextKeyService) -> Vec<Rc<IQuickAccessProviderDescriptor>>;

    /// Get a specific quick access provider for a given prefix.
    fn get_quick_access_provider(&self, prefix: &str, context_key_service: &dyn IContextKeyService) -> Option<Rc<IQuickAccessProviderDescriptor>>;
}

/// O `.length` de JavaScript (unidades UTF-16).
fn js_length(text: &str) -> usize {
    text.encode_utf16().count()
}

type Providers = Rc<RefCell<Vec<Rc<IQuickAccessProviderDescriptor>>>>;
type DefaultProvider = Rc<RefCell<Option<Rc<IQuickAccessProviderDescriptor>>>>;

/// `QuickAccessRegistry`.
#[derive(Default)]
pub struct QuickAccessRegistry {
    providers: Providers,
    default_provider: DefaultProvider,
}

impl QuickAccessRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// `clear()`: esvazia o registro e devolve a função que restaura o estado de antes.
    pub fn clear(&self) -> Box<dyn Fn()> {
        let providers = self.providers.borrow().clone();
        let default_provider = self.default_provider.borrow().clone();

        self.providers.borrow_mut().clear();
        *self.default_provider.borrow_mut() = None;

        let (providers_cell, default_provider_cell) = (self.providers.clone(), self.default_provider.clone());
        Box::new(move || {
            *providers_cell.borrow_mut() = providers.clone();
            *default_provider_cell.borrow_mut() = default_provider.clone();
        })
    }
}

impl IQuickAccessRegistry for QuickAccessRegistry {
    fn register_quick_access_provider(&self, provider: Rc<IQuickAccessProviderDescriptor>) -> DisposableRef {
        // Extract the default provider when no prefix is present
        if provider.prefix.is_empty() {
            *self.default_provider.borrow_mut() = Some(provider.clone());
        } else {
            self.providers.borrow_mut().push(provider.clone());
        }

        // sort the providers by decreasing prefix length, such that longer
        // prefixes take priority: 'ext' vs 'ext install' - the latter should win
        self.providers.borrow_mut().sort_by(|provider_a, provider_b| js_length(&provider_b.prefix).cmp(&js_length(&provider_a.prefix)));

        let (providers, default_provider) = (self.providers.clone(), self.default_provider.clone());
        to_disposable(move || {
            let mut providers = providers.borrow_mut();
            // `splice(indexOf(provider), 1)`: o `-1` de um provedor fora da lista remove o último.
            match providers.iter().position(|candidate| Rc::ptr_eq(candidate, &provider)) {
                Some(index) => {
                    providers.remove(index);
                }
                None => {
                    providers.pop();
                }
            }

            let mut default_provider = default_provider.borrow_mut();
            if default_provider.as_ref().is_some_and(|candidate| Rc::ptr_eq(candidate, &provider)) {
                *default_provider = None;
            }
        })
    }

    fn get_quick_access_providers(&self, context_key_service: &dyn IContextKeyService) -> Vec<Rc<IQuickAccessProviderDescriptor>> {
        self.default_provider
            .borrow()
            .iter()
            .cloned()
            .chain(self.providers.borrow().iter().cloned())
            .filter(|provider| provider.when.is_none() || context_key_service.context_matches_rules(provider.when.as_ref()))
            .collect()
    }

    fn get_quick_access_provider(&self, prefix: &str, context_key_service: &dyn IContextKeyService) -> Option<Rc<IQuickAccessProviderDescriptor>> {
        let result = if prefix.is_empty() {
            None
        } else {
            self.providers
                .borrow()
                .iter()
                .find(|provider| prefix.starts_with(provider.prefix.as_str()) && (provider.when.is_none() || context_key_service.context_matches_rules(provider.when.as_ref())))
                .cloned()
        };

        result.or_else(|| self.default_provider.borrow().clone())
    }
}

/// `Registry.as<IQuickAccessRegistry>(Extensions.Quickaccess)`, registrando o `QuickAccessRegistry` na primeira chamada.
pub fn quick_access_registry() -> Rc<QuickAccessRegistry> {
    let registry = registry();
    if !registry.knows(EXTENSIONS_QUICKACCESS) {
        registry.add(EXTENSIONS_QUICKACCESS, Rc::new(QuickAccessRegistry::new()));
    }

    registry.as_::<QuickAccessRegistry>(EXTENSIONS_QUICKACCESS).expect("o registro de acesso rápido acabou de ser adicionado")
}
