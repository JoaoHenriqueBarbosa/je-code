//! Porte de `src/vs/workbench/services/languageStatus/common/languageStatusService.ts`: o `ILanguageStatusService`, o
//! registro dos itens de status de linguagem (o item da barra de status que as extensões criam com
//! `vscode.languages.createLanguageStatusItem`).
//!
//! Divergências:
//! - `ITextModel` é o [`TextModel`] concreto. O `LanguageFeatureRegistry<ILanguageStatus>` guarda cada status em `Rc`, e
//!   [`ILanguageStatusService::get_language_status`] devolve esses `Rc` (o objeto compartilhado do original).
//! - `label: string | { value: string; shortValue: string }` é o [`LanguageStatusLabel`].
//! - `Command` é o `Command` de `editor/common/languages`; `Severity` é o de `base/common/severity`.
//! - `Event.map(this._provider.onDidChange, () => undefined)` é `Event<()>`, montado a cada chamada de `on_did_change`
//!   (o `Event` do porte não é clonável; assinar qualquer um deles escuta o mesmo registro).
//! - `registerSingleton(ILanguageStatusService, LanguageStatusServiceImpl, InstantiationType.Delayed)` é [`register`],
//!   com o proxy de instanciação atrasada declarado junto do identificador.

use std::cmp::Ordering;
use std::rc::Rc;

use je_base::cancellation::CancellationToken;
use je_base::event::Event;
use je_base::lifecycle::DisposableRef;
use je_base::promise::Promise;
use je_base::severity::Severity;
use je_base::strings;
use je_editor::code_action::languages::Command;
use je_editor::language_feature_registry::LanguageFeatureRegistry;
use je_editor::language_selector::LanguageSelector;
use je_editor::text_model::TextModel;
use je_instantiation::{InstantiationType, Injectable, delayed_proxy, register_singleton, service_id, service_impl};
use je_platform::accessibility::IAccessibilityInformation;

/// `label: string | { value: string; shortValue: string }`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LanguageStatusLabel {
    Text(String),
    Short { value: String, short_value: String },
}

/// `ILanguageStatus`.
#[derive(Clone, Debug)]
pub struct ILanguageStatus {
    pub id: String,
    pub name: String,
    pub selector: LanguageSelector,
    pub severity: Severity,
    pub label: LanguageStatusLabel,
    pub detail: String,
    pub busy: bool,
    pub source: String,
    pub command: Option<Command>,
    pub accessibility_info: Option<IAccessibilityInformation>,
}

/// `ILanguageStatusProvider`.
pub trait ILanguageStatusProvider {
    fn provide_language_status(&self, lang_id: &str, token: CancellationToken) -> Promise<Option<ILanguageStatus>>;
}

/// `ILanguageStatusService`.
pub trait ILanguageStatusService {
    fn on_did_change(&self) -> Event<()>;

    fn add_status(&self, status: ILanguageStatus) -> DisposableRef;

    fn get_language_status(&self, model: &TextModel) -> Vec<Rc<ILanguageStatus>>;
}

delayed_proxy!(pub LanguageStatusServiceProxy for dyn ILanguageStatusService {
    methods {
        fn add_status(&self, status: ILanguageStatus) -> DisposableRef;
        fn get_language_status(&self, model: &TextModel) -> Vec<Rc<ILanguageStatus>>;
    }
    events {
        fn on_did_change(&self) -> ();
    }
});

service_id!(pub I_LANGUAGE_STATUS_SERVICE: dyn ILanguageStatusService = "ILanguageStatusService", proxy LanguageStatusServiceProxy);

/// `compare(a, b)` de `base/common/strings.ts`, por unidade UTF-16 como o `<` do JavaScript.
fn compare(a: &str, b: &str) -> Ordering {
    let (a, b): (Vec<u16>, Vec<u16>) = (a.encode_utf16().collect(), b.encode_utf16().collect());
    strings::compare(&a, &b).cmp(&0)
}

/// `LanguageStatusServiceImpl`.
pub struct LanguageStatusServiceImpl {
    provider: LanguageFeatureRegistry<ILanguageStatus>,
}

impl Injectable for LanguageStatusServiceImpl {
    type Args = ();
    type Deps = ();

    fn create(_args: Self::Args, _deps: Self::Deps) -> Rc<Self> {
        Rc::new(Self { provider: LanguageFeatureRegistry::new(None) })
    }
}

service_impl!(LanguageStatusServiceImpl => dyn ILanguageStatusService);

impl ILanguageStatusService for LanguageStatusServiceImpl {
    fn on_did_change(&self) -> Event<()> {
        self.provider.on_did_change().map(|_| (), None)
    }

    fn add_status(&self, status: ILanguageStatus) -> DisposableRef {
        let selector = status.selector.clone();
        self.provider.register(selector, Rc::new(status))
    }

    fn get_language_status(&self, model: &TextModel) -> Vec<Rc<ILanguageStatus>> {
        let mut statuses = self.provider.ordered(model, false);
        statuses.sort_by(|a, b| {
            (b.severity as i32)
                .cmp(&(a.severity as i32))
                .then_with(|| compare(&a.source, &b.source))
                .then_with(|| compare(&a.id, &b.id))
        });
        statuses
    }
}

/// `registerSingleton(ILanguageStatusService, LanguageStatusServiceImpl, InstantiationType.Delayed)`.
pub fn register() {
    register_singleton::<LanguageStatusServiceImpl>(I_LANGUAGE_STATUS_SERVICE, InstantiationType::Delayed);
}
