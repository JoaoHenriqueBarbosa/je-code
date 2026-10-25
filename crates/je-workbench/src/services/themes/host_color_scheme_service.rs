//! Porte de `src/vs/workbench/services/themes/common/hostColorSchemeService.ts`: o contrato `IHostColorSchemeService`
//! (o esquema de cores do sistema hospedeiro) e o `IColorScheme` de `platform/window/common/window.ts`.

use je_base::event::Event;
use je_instantiation::service_id;

/// `IColorScheme`: o esquema de cores do sistema (escuro ou claro, com ou sem alto contraste).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IColorScheme {
    pub dark: bool,
    pub high_contrast: bool,
}

/// `IHostColorSchemeService`. Os campos `dark` e `highContrast` (somente leitura no original) são métodos, porque o
/// valor muda ao longo do tempo.
pub trait IHostColorSchemeService {
    fn dark(&self) -> bool;
    fn high_contrast(&self) -> bool;
    fn on_did_change_color_scheme(&self) -> Event<()>;
}

service_id!(pub I_HOST_COLOR_SCHEME_SERVICE: dyn IHostColorSchemeService = "hostColorSchemeService");
