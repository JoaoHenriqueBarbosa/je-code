//! Porte de `src/vs/platform/accessibility/common/accessibility.ts`: o contrato `IAccessibilityService` e o
//! `AccessibilitySupport`.
//!
//! Divergências:
//! - `CONTEXT_ACCESSIBILITY_MODE_ENABLED` (a `RawContextKey`), `isAccessibilityInformation` e
//!   `ACCESSIBLE_VIEW_SHOWN_STORAGE_PREFIX` não são portados aqui: nenhum consumidor portado os lê.
//!   `IAccessibilityInformation` é portado (o item da barra de status de célula de notebook o carrega).
//! - `alwaysUnderlineAccessKeys(): Promise<boolean>` é `Promise<bool>`.

use je_base::event::Event;
use je_base::promise::Promise;
use je_instantiation::service_id;

/// `IAccessibilityInformation`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IAccessibilityInformation {
    pub label: String,
    pub role: Option<String>,
}

/// `AccessibilitySupport`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccessibilitySupport {
    /// This should be the browser case where it is not known if a screen reader is attached or no.
    Unknown = 0,

    Disabled = 1,

    Enabled = 2,
}

/// `IAccessibilityService`.
pub trait IAccessibilityService {
    fn on_did_change_screen_reader_optimized(&self) -> Event<()>;
    fn on_did_change_reduced_motion(&self) -> Event<()>;
    fn on_did_change_reduced_transparency(&self) -> Event<()>;

    fn always_underline_access_keys(&self) -> Promise<bool>;
    fn is_screen_reader_optimized(&self) -> bool;
    fn is_motion_reduced(&self) -> bool;
    fn is_transparency_reduced(&self) -> bool;
    fn get_accessibility_support(&self) -> AccessibilitySupport;
    fn set_accessibility_support(&self, accessibility_support: AccessibilitySupport);
    fn alert(&self, message: &str);
    fn status(&self, message: &str);
}

service_id!(pub I_ACCESSIBILITY_SERVICE: dyn IAccessibilityService = "accessibilityService");
