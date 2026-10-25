//! Porte de `src/vs/editor/standalone/common/standaloneTheme.ts`: o tema do editor standalone ([`IStandaloneTheme`]) e o
//! serviço que o guarda ([`IStandaloneThemeService`]).
//!
//! Divergências de forma, nenhuma de comportamento:
//! - **`tokenTheme: TokenTheme`.** O original usa a classe concreta, que os testes estendem sobrescrevendo `match`. Aqui o
//!   campo é o trait [`ITokenTheme`] (só o `match`, o que o suporte de tokenização lê), implementado pelo [`TokenTheme`]
//!   real e por qualquer dublê.
//! - **`getColorTheme(): IStandaloneTheme`.** O [`IThemeService`] já tem um `get_color_theme` que devolve o `IColorTheme`;
//!   o do standalone, que devolve o tema com o `tokenTheme`, é [`IStandaloneThemeService::get_standalone_color_theme`].
//! - **`IStandaloneThemeService extends IThemeService`** é o supertrait, e `IStandaloneTheme extends IColorTheme` também.
//! - **`IColors`** (`{ [colorId: string]: string }`) é a lista de pares na ordem de inserção.
//! - **`setColorMapOverride(Color[] | null)`** recebe `Option<Vec<Option<Color>>>`: o primeiro elemento da lista do original
//!   é `null` (o id 0 é "sem cor"), que aqui é `None`.
//! - O `IStandaloneThemeService` do `monarch/lexer.rs` é o recorte que o lexer lê (`token_theme()` sobre o [`TokenTheme`]
//!   concreto) e continua separado: o lexer só roda sobre o tema real.

use std::rc::Rc;

use je_theme::color::Color;

use crate::services::theme_service::{IColorTheme, IThemeService};
use crate::token_theme::{ITokenThemeRule, TokenTheme};

/// `BuiltinTheme`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuiltinTheme {
    Vs,
    VsDark,
    HcBlack,
    HcLight,
}

/// `IStandaloneThemeData`.
#[derive(Clone, Debug)]
pub struct StandaloneThemeData {
    pub base: BuiltinTheme,
    pub inherit: bool,
    pub rules: Vec<ITokenThemeRule>,
    pub encoded_tokens_colors: Option<Vec<String>>,
    pub colors: Vec<(String, String)>,
}

/// O que o suporte de tokenização lê de `TokenTheme`: o metadado de um token numa linguagem.
pub trait ITokenTheme {
    /// `match(languageId, token)`.
    fn match_token(&self, language_id: u32, token: &str) -> u32;
}

impl ITokenTheme for TokenTheme {
    fn match_token(&self, language_id: u32, token: &str) -> u32 {
        // O método do mesmo nome do tipo concreto (o caminho `Tipo::método` prefere o inerente ao do trait).
        TokenTheme::match_token(self, language_id, token)
    }
}

/// `IStandaloneTheme`.
pub trait IStandaloneTheme: IColorTheme {
    /// `tokenTheme`.
    fn token_theme(&self) -> Rc<dyn ITokenTheme>;

    /// `themeName`.
    fn theme_name(&self) -> String;
}

/// `IStandaloneThemeService`.
pub trait IStandaloneThemeService: IThemeService {
    /// `setTheme(themeName)`.
    fn set_theme(&self, theme_name: &str);

    /// `setAutoDetectHighContrast(autoDetectHighContrast)`.
    fn set_auto_detect_high_contrast(&self, auto_detect_high_contrast: bool);

    /// `defineTheme(themeName, themeData)`.
    fn define_theme(&self, theme_name: &str, theme_data: StandaloneThemeData);

    /// `getColorTheme(): IStandaloneTheme` (ver as divergências no topo do módulo).
    fn get_standalone_color_theme(&self) -> Rc<dyn IStandaloneTheme>;

    /// `setColorMapOverride(colorMapOverride)`.
    fn set_color_map_override(&self, color_map_override: Option<Vec<Option<Color>>>);
}
