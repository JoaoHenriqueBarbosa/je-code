//! Porte parcial de `src/vs/platform/quickinput/common/quickAccess.ts`: o que os provedores de acesso rápido leem
//! (`IQuickAccessProviderRunOptions`, `IQuickAccessProvider` e `DefaultQuickAccessFilterValue`).
//!
//! Fora do porte: o `IQuickAccessController`, o `IQuickAccessRegistry` e `IQuickAccessProviderDescriptor`, que dependem do
//! serviço de quick input visual (o controlador monta o picker e escolhe o provedor pelo prefixo).
//!
//! Notas de tradução:
//! - **`provide(picker, token, options)`.** Cada provedor define o próprio tipo de opções de execução (o original estende
//!   `IQuickAccessProviderRunOptions`), então o trait é genérico em `O`.
//! - **`handleAccept`.** É o `Rc<dyn Fn(&IQuickPickItem, bool)>` compartilhado.

use std::rc::Rc;

use je_base::cancellation::CancellationToken;
use je_base::lifecycle::DisposableRef;

use super::quick_input::{IQuickPick, IQuickPickItem};

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

/// `IQuickAccessProvider`.
pub trait IQuickAccessProvider<O> {
    /// What to do when the user changes the quick access value (mostly for the default filter value).
    fn default_filter_value(&self) -> Option<DefaultQuickAccessFilterValue> {
        None
    }

    /// Called whenever a quick access provider is needed to populate the picker.
    ///
    /// Returns a disposable that will automatically be disposed when the picker closes or is replaced by another picker.
    fn provide(&self, picker: Rc<dyn IQuickPick>, token: CancellationToken, options: Option<Rc<O>>) -> DisposableRef;
}
