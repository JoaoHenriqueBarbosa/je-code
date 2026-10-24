//! Porte de `src/vs/workbench/contrib/share/common/share.ts`: o item compartilhável, o provedor e o contrato do serviço de
//! compartilhamento.
//!
//! Divergências:
//! - `URI | string | undefined` (o resultado de `provideShare`) é `Option<`[`ShareResult`]`>`.
//! - `Thenable<T>` é [`Promise<T>`]; `prepareShare?` é um método com corpo padrão `None` (o membro opcional ausente).
//! - `ISubmenuItem` é o [`SubmenuItem`] de `je-platform`; o provedor é guardado como `Rc<dyn IShareProvider>`.
//! - O `createDecorator<IShareService>('shareService')` é o `service_id!`.

use std::rc::Rc;

use je_base::cancellation::CancellationToken;
use je_base::lifecycle::DisposableRef;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_editor::language_selector::LanguageSelector;
use je_editor::selection::Selection;
use je_platform::actions::actions::SubmenuItem;

/// `IShareableItem`.
#[derive(Clone, Debug, PartialEq)]
pub struct IShareableItem {
    pub resource_uri: URI,
    pub selection: Option<Selection>,
}

/// O `URI | string` que um provedor de compartilhamento devolve.
#[derive(Clone, Debug, PartialEq)]
pub enum ShareResult {
    Uri(URI),
    Text(String),
}

/// `IShareProvider`.
pub trait IShareProvider {
    fn id(&self) -> String;
    fn label(&self) -> String;
    fn priority(&self) -> i32;
    fn selector(&self) -> LanguageSelector;

    /// `prepareShare?(item, token)`: o membro opcional que o provedor não tem é `None`.
    fn prepare_share(&self, _item: &IShareableItem, _token: CancellationToken) -> Option<Promise<Option<bool>>> {
        None
    }

    fn provide_share(&self, item: &IShareableItem, token: CancellationToken) -> Promise<Option<ShareResult>>;
}

/// `IShareService`.
pub trait IShareService {
    fn register_share_provider(&self, provider: Rc<dyn IShareProvider>) -> DisposableRef;
    fn get_share_actions(&self) -> Vec<SubmenuItem>;
    fn provide_share(&self, item: &IShareableItem, token: CancellationToken) -> Promise<Option<ShareResult>>;
}

je_instantiation::service_id!(pub I_SHARE_SERVICE: dyn IShareService = "shareService");
