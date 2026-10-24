//! Porte de `src/vs/workbench/services/decorations/common/decorations.ts`: os contratos do serviço de decorações de
//! recursos (`IDecorationsService`), do provedor (`IDecorationsProvider`) e dos dados de uma decoração.
//!
//! A implementação (`DecorationsService`, que calcula as classes de CSS de cada decoração) é o
//! [`decorations_service`](super::decorations_service).
//!
//! Divergências:
//! - **`letter`** (`string | ThemeIcon`) é o [`DecorationLetter`].
//! - **`provideDecorations`** devolve `IDecorationData | Promise<IDecorationData | undefined> | undefined`, que é o
//!   `MaybePromise<Option<DecorationData>>`.
//! - **`IDecoration`, `IResourceDecorationChangeEvent`** são traits (interfaces de leitura).
//! - **`_serviceBrand`** não existe: o identificador do serviço é o [`I_DECORATIONS_SERVICE`].

use std::rc::Rc;

use je_base::cancellation::CancellationToken;
use je_base::event::Event;
use je_base::lifecycle::{DisposableRef, IDisposable};
use je_base::promise::MaybePromise;
use je_base::themables::{ColorIdentifier, ThemeIcon};
use je_base::uri::URI;
use je_instantiation::service_id;

service_id!(pub I_DECORATIONS_SERVICE: dyn IDecorationsService = "IFileDecorationsService");

/// Stable marker class set on a label alongside the generated, per-color `IDecoration.labelClassName`.
pub const DECORATION_LABEL_COLOR_CLASS: &str = "monaco-decoration-itemColor";

/// Stable marker class set on a label with generated badge classes.
pub const DECORATION_BADGE_CLASS: &str = "monaco-decoration-badge";

/// Stable marker class set on a label with a text decoration badge.
pub const DECORATION_TEXT_BADGE_CLASS: &str = "monaco-decoration-text-badge";

/// `string | ThemeIcon` de `IDecorationData.letter`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DecorationLetter {
    Text(String),
    Icon(ThemeIcon),
}

/// `IDecorationData`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DecorationData {
    pub weight: Option<i32>,
    pub color: Option<ColorIdentifier>,
    pub letter: Option<DecorationLetter>,
    pub tooltip: Option<String>,
    pub strikethrough: Option<bool>,
    pub bubble: Option<bool>,
}

/// `IDecoration`.
pub trait IDecoration: IDisposable {
    fn tooltip(&self) -> String;
    fn strikethrough(&self) -> bool;
    fn label_class_name(&self) -> String;
    fn badge_class_name(&self) -> String;
    fn icon_class_name(&self) -> String;
    fn is_text_badge(&self) -> bool;
}

/// `IDecorationsProvider`.
pub trait IDecorationsProvider {
    fn label(&self) -> String;

    fn on_did_change(&self) -> Event<Vec<URI>>;

    fn provide_decorations(&self, uri: &URI, token: CancellationToken) -> MaybePromise<Option<DecorationData>>;
}

/// `IResourceDecorationChangeEvent`.
pub trait IResourceDecorationChangeEvent {
    fn affects_resource(&self, uri: &URI) -> bool;
}

/// `IDecorationsService`.
pub trait IDecorationsService {
    fn on_did_change_decorations(&self) -> Event<Rc<dyn IResourceDecorationChangeEvent>>;

    fn register_decorations_provider(&self, provider: Rc<dyn IDecorationsProvider>) -> DisposableRef;

    fn get_decoration(&self, uri: &URI, include_children: bool) -> Option<Rc<dyn IDecoration>>;
}
