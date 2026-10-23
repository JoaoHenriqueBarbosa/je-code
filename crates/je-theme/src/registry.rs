//! Porte de `src/vs/platform/theme/common/colorUtils.ts` e `colorRegistry.ts`: o registro de cores
//! (`registerColor`), os valores de cor (literal, referência a outra cor, transformação) e a
//! resolução (`resolveColorValue`, `executeTransform`) no contexto de um tema.
//!
//! As cores registradas pelo VS Code vêm de [`crate::registered_colors`], gerado por
//! `scripts/gen_color_registry.py`. O que o original guarda em objetos TypeScript com referências
//! entre constantes aqui são dados estáticos: um id referido por outro é um [`ColorValue::Id`].

use std::collections::HashMap;
use std::sync::OnceLock;

use crate::ThemeType;
use crate::color::{Color, Srgba};
use crate::registered_colors::REGISTERED_COLORS;

/// `DEFAULT_COLOR_CONFIG_VALUE`: o valor `"default"` num tema ou em `workbench.colorCustomizations`
/// manda usar a cor padrão do registro.
pub const DEFAULT_COLOR_CONFIG_VALUE: &str = "default";

/// `asCssVariableName`: `editorSuggestWidget.background` vira `--vscode-editorSuggestWidget-background`.
pub fn as_css_variable_name(color_ident: &str) -> String {
    format!("--vscode-{}", color_ident.replace('.', "-"))
}

/// `ColorValue`: um literal de cor, uma referência a outra cor ou uma cor derivada.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ColorValue {
    /// Literal `#rgb`, `#rgba`, `#rrggbb` ou `#rrggbbaa` (o original: string que começa com `#`).
    Hex(&'static str),
    /// Referência a outra cor pelo id (o original: string que não começa com `#`).
    Id(&'static str),
    /// `new Color(new RGBA(r, g, b, a))`, e `Color.white`, `Color.black`, `Color.transparent`...
    Rgba(u8, u8, u8, f64),
    /// Cor derivada.
    Transform(&'static ColorTransform),
}

/// `ColorTransform`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ColorTransform {
    Darken {
        value: ColorValue,
        factor: f64,
    },
    Lighten {
        value: ColorValue,
        factor: f64,
    },
    Transparent {
        value: ColorValue,
        factor: f64,
    },
    Opaque {
        value: ColorValue,
        background: ColorValue,
    },
    OneOf(&'static [ColorValue]),
    LessProminent {
        value: ColorValue,
        background: ColorValue,
        factor: f64,
        transparency: f64,
    },
    IfDefinedThenElse {
        if_defined: &'static str,
        then: ColorValue,
        otherwise: ColorValue,
    },
    Mix {
        color: ColorValue,
        with: ColorValue,
        ratio: Option<f64>,
    },
}

/// `ColorDefaults`: o padrão para cada tipo de tema (`None` é o `null` do original: sem cor).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ColorDefaults {
    pub light: Option<ColorValue>,
    pub dark: Option<ColorValue>,
    pub hc_dark: Option<ColorValue>,
    pub hc_light: Option<ColorValue>,
}

impl ColorDefaults {
    fn for_theme(&self, theme_type: ThemeType) -> Option<ColorValue> {
        match theme_type {
            ThemeType::Light => self.light,
            ThemeType::Dark => self.dark,
            ThemeType::HighContrastDark => self.hc_dark,
            ThemeType::HighContrastLight => self.hc_light,
        }
    }
}

/// O `ColorDefaults | ColorValue` do segundo argumento de `registerColor`: padrões por tipo de tema
/// ou um único valor para todos os tipos. `Option<Defaults>` com `None` é o `null`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Defaults {
    PerTheme(ColorDefaults),
    Single(ColorValue),
}

/// `ColorContribution`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ColorContribution {
    pub id: &'static str,
    pub description: &'static str,
    pub defaults: Option<Defaults>,
    pub needs_transparency: bool,
    pub deprecation_message: Option<&'static str>,
}

/// O que a resolução pede de um tema (`IColorTheme`).
pub trait ColorThemeContext {
    fn theme_type(&self) -> ThemeType;

    /// `getColor(color, useDefault)`; `use_default` falso desliga o padrão do registro.
    fn get_color(&self, id: &str, use_default: bool) -> Option<Color>;

    /// `defines(color)`: o tema (não o padrão do registro) define um valor para a cor.
    fn defines(&self, id: &str) -> bool;
}

/// `resolveColorValue`: `None` como valor é o `null` do original.
pub fn resolve_color_value(
    color_value: Option<&ColorValue>,
    theme: &dyn ColorThemeContext,
) -> Option<Color> {
    match color_value? {
        ColorValue::Hex(hex) => Some(Color::from_hex(hex)),
        ColorValue::Id(id) => theme.get_color(id, true),
        ColorValue::Rgba(r, g, b, a) => Some(Color::from(Srgba::new(*r, *g, *b, *a))),
        ColorValue::Transform(transform) => execute_transform(transform, theme),
    }
}

/// `executeTransform`.
pub fn execute_transform(
    transform: &ColorTransform,
    theme: &dyn ColorThemeContext,
) -> Option<Color> {
    match transform {
        ColorTransform::Darken { value, factor } => {
            resolve_color_value(Some(value), theme).map(|c| c.darken(*factor))
        }
        ColorTransform::Lighten { value, factor } => {
            resolve_color_value(Some(value), theme).map(|c| c.lighten(*factor))
        }
        ColorTransform::Transparent { value, factor } => {
            resolve_color_value(Some(value), theme).map(|c| c.transparent(*factor))
        }
        ColorTransform::Mix { color, with, ratio } => {
            let primary =
                resolve_color_value(Some(color), theme).unwrap_or_else(Color::transparent_color);
            let other =
                resolve_color_value(Some(with), theme).unwrap_or_else(Color::transparent_color);
            Some(primary.mix(&other, *ratio))
        }
        ColorTransform::Opaque { value, background } => {
            let Some(background) = resolve_color_value(Some(background), theme) else {
                return resolve_color_value(Some(value), theme);
            };
            resolve_color_value(Some(value), theme).map(|c| c.make_opaque(&background))
        }
        ColorTransform::OneOf(values) => values
            .iter()
            .find_map(|candidate| resolve_color_value(Some(candidate), theme)),
        ColorTransform::IfDefinedThenElse {
            if_defined,
            then,
            otherwise,
        } => resolve_color_value(
            Some(if theme.defines(if_defined) {
                then
            } else {
                otherwise
            }),
            theme,
        ),
        ColorTransform::LessProminent {
            value,
            background,
            factor,
            transparency,
        } => {
            let from = resolve_color_value(Some(value), theme)?;

            let Some(background) = resolve_color_value(Some(background), theme) else {
                return Some(from.transparent(factor * transparency));
            };

            Some(if from.is_darker_than(&background) {
                Color::get_lighter_color(&from, &background, Some(*factor))
                    .transparent(*transparency)
            } else {
                Color::get_darker_color(&from, &background, Some(*factor))
                    .transparent(*transparency)
            })
        }
    }
}

/// `ColorRegistry`: as cores registradas, na ordem de registro.
#[derive(Clone, Debug, Default)]
pub struct ColorRegistry {
    colors: Vec<ColorContribution>,
    index_by_id: HashMap<&'static str, usize>,
}

impl ColorRegistry {
    /// Um registro vazio.
    pub fn new() -> Self {
        Self::default()
    }

    /// Um registro com as cores que o VS Code registra ([`crate::registered_colors`]).
    pub fn with_registered_colors() -> Self {
        let mut registry = Self::new();
        for contribution in REGISTERED_COLORS {
            registry.register_color(*contribution);
        }
        registry
    }

    /// `registerColor`: registrar de novo um id existente substitui a contribuição, mantendo a posição.
    pub fn register_color(&mut self, contribution: ColorContribution) -> &'static str {
        match self.index_by_id.get(contribution.id) {
            Some(&index) => self.colors[index] = contribution,
            None => {
                self.index_by_id.insert(contribution.id, self.colors.len());
                self.colors.push(contribution);
            }
        }
        contribution.id
    }

    /// `deregisterColor`.
    pub fn deregister_color(&mut self, id: &str) {
        if let Some(index) = self.index_by_id.remove(id) {
            self.colors.remove(index);
            for slot in self.index_by_id.values_mut().filter(|slot| **slot > index) {
                *slot -= 1;
            }
        }
    }

    /// `getColors`.
    pub fn get_colors(&self) -> &[ColorContribution] {
        &self.colors
    }

    pub fn get(&self, id: &str) -> Option<&ColorContribution> {
        self.index_by_id.get(id).map(|&index| &self.colors[index])
    }

    /// `updateDefaultColor`: troca os padrões de uma cor já registrada.
    pub fn update_default_color(&mut self, id: &str, defaults: Option<Defaults>) {
        if let Some(&index) = self.index_by_id.get(id) {
            self.colors[index].defaults = defaults;
        }
    }

    /// `resolveDefaultColor`: o padrão da cor para o tipo do `theme`, resolvido no contexto dele.
    pub fn resolve_default_color(&self, id: &str, theme: &dyn ColorThemeContext) -> Option<Color> {
        let defaults = self.get(id)?.defaults.as_ref()?;
        let color_value = match defaults {
            Defaults::PerTheme(per_theme) => per_theme.for_theme(theme.theme_type()),
            Defaults::Single(value) => Some(*value),
        };
        resolve_color_value(color_value.as_ref(), theme)
    }
}

/// O registro global (`getColorRegistry()`), com as cores do VS Code.
pub fn color_registry() -> &'static ColorRegistry {
    static REGISTRY: OnceLock<ColorRegistry> = OnceLock::new();
    REGISTRY.get_or_init(ColorRegistry::with_registered_colors)
}
