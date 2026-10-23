//! Porte de `src/vs/platform/state/node/state.ts`: `IStateReadService` e `IStateService`.
//!
//! O valor de um item (`object | string | number | boolean | undefined | null`) é um [`Dynamic`]; um item
//! ausente é `Undefined`.

use je_base::dynamic::Dynamic;
use je_base::promise::Promise;

/// `IStateReadService`.
pub trait StateReadService {
    /// `getItem(key, defaultValue)`: `default_value` é `Undefined` quando o chamador não passa um.
    fn get_item(&self, key: &str, default_value: Dynamic) -> Dynamic;
}

/// `IStateService`.
pub trait StateService: StateReadService {
    fn set_item(&self, key: &str, data: Dynamic);
    fn set_items(&self, items: &[(String, Dynamic)]);
    fn remove_item(&self, key: &str);
    fn close(&self) -> Promise<()>;
}
