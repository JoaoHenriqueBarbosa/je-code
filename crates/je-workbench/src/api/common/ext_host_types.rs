//! Porte parcial de `src/vs/workbench/api/common/extHostTypes.ts`.
//!
//! Divergências: só o que `extHostBrowsers.ts` usa. `ViewColumn` é a enumeração numérica da API
//! (`vscode.ViewColumn`), então os valores são [`f64`] (o `number` do original) e o tipo é um espaço de
//! nomes de constantes. A classe `ThemeIcon` do original (`id` e `color`) é o [`je_base::themables::ThemeIcon`],
//! que já tem os mesmos campos.

use je_base::themables::ThemeIcon;
use je_base::uri::URI;

/// `ViewColumn`: a coluna do editor na API de extensão.
pub struct ViewColumn;

impl ViewColumn {
    /// A coluna do editor ativo.
    pub const ACTIVE: f64 = -1.0;
    /// A coluna ao lado do editor ativo.
    pub const BESIDE: f64 = -2.0;
    pub const ONE: f64 = 1.0;
    pub const TWO: f64 = 2.0;
    pub const THREE: f64 = 3.0;
    pub const FOUR: f64 = 4.0;
    pub const FIVE: f64 = 5.0;
    pub const SIX: f64 = 6.0;
    pub const SEVEN: f64 = 7.0;
    pub const EIGHT: f64 = 8.0;
    pub const NINE: f64 = 9.0;
}

/// `vscode.IconPath`: `Uri | { light: Uri; dark: Uri } | ThemeIcon`.
#[derive(Clone)]
pub enum IconPath {
    Uri(URI),
    LightDark { light: URI, dark: URI },
    ThemeIcon(ThemeIcon),
}
