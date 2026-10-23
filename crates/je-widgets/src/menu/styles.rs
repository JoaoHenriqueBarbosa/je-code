//! Os estilos do menu (`IMenuStyles`, `unthemedMenuStyles`, `defaultMenuStyles`) e as medidas da folha de
//! estilo de `menu.ts` (`getMenuWidgetCSS`), que o menu aplica direto nos nós.

use je_base::themables::ThemeIcon;
use je_render::Rgba;
use je_theme::ColorTheme;

/// O tamanho da fonte do menu (`.monaco-menu { font-size: 13px }`).
pub const MENU_FONT_SIZE: f32 = 13.0;
/// A altura de um item (`.action-menu-item { height: 24px }`).
pub const MENU_ITEM_HEIGHT: f64 = 24.0;
/// A margem lateral do item (`margin: 0 4px`).
pub const MENU_ITEM_MARGIN: f64 = 4.0;
/// O `padding: 4px 0` da barra de ações vertical.
pub const MENU_VERTICAL_PADDING: f64 = 4.0;
/// O `padding: 0 2em` do rótulo, do atalho e a largura (`2em`) do `.menu-item-check`, em px (a fonte é 13px).
pub const MENU_LABEL_PADDING: f64 = 2.0 * MENU_FONT_SIZE as f64;
/// O `font-size: 16px !important` do `.submenu-indicator.codicon`.
pub const MENU_SUBMENU_INDICATOR_SIZE: f64 = 16.0;
/// O `padding: 0 1.8em` do `.submenu-indicator`, com o `em` do indicador (16px).
pub const MENU_SUBMENU_INDICATOR_PADDING: f64 = 1.8 * MENU_SUBMENU_INDICATOR_SIZE;
/// O `min-width: 160px` do `.monaco-menu`.
pub const MENU_MIN_WIDTH: f64 = 160.0;
/// O `border` de `var(--vscode-strokeThickness)`.
pub const MENU_BORDER_WIDTH: f64 = 1.0;
/// O `border-radius: var(--vscode-cornerRadius-large)`.
pub const MENU_CORNER_RADIUS_LARGE: f64 = 8.0;
/// O `border-radius: var(--vscode-cornerRadius-medium)` do item.
pub const MENU_CORNER_RADIUS_MEDIUM: f64 = 6.0;
/// O `verticalScrollbarSize: 7` do `DomScrollableElement` do menu.
pub const MENU_SCROLLBAR_SIZE: f64 = 7.0;
/// O `opacity: 0.7` do atalho e do indicador de submenu.
pub const MENU_SECONDARY_OPACITY: f32 = 0.7;
/// O `opacity: 0.4` do atalho e do indicador de um item desabilitado.
pub const MENU_DISABLED_SECONDARY_OPACITY: f32 = 0.4;
/// A cor do texto quando o tema não dá nenhuma (a mesma que os outros widgets usam de reserva).
pub const MENU_FALLBACK_FOREGROUND: Rgba = Rgba::new(204, 204, 204, 255);

/// `IMenuStyles`: `string | undefined` do original é `Option<Rgba>`. O último campo
/// (`disabled_foreground`) não está no original: é a variável `--vscode-disabledForeground` que a folha de
/// estilo do menu lê.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct IMenuStyles {
    pub shadow_color: Option<Rgba>,
    pub border_color: Option<Rgba>,
    pub foreground_color: Option<Rgba>,
    pub background_color: Option<Rgba>,
    pub selection_foreground_color: Option<Rgba>,
    pub selection_background_color: Option<Rgba>,
    pub selection_border_color: Option<Rgba>,
    pub separator_color: Option<Rgba>,
    pub scrollbar_shadow: Option<Rgba>,
    pub scrollbar_slider_background: Option<Rgba>,
    pub scrollbar_slider_hover_background: Option<Rgba>,
    pub scrollbar_slider_active_background: Option<Rgba>,
    pub disabled_foreground: Option<Rgba>,
}

/// `unthemedMenuStyles`: nenhuma cor.
pub fn unthemed_menu_styles() -> IMenuStyles {
    IMenuStyles::default()
}

/// `defaultMenuStyles` (`defaultStyles.ts`) com as cores resolvidas no tema.
pub fn default_menu_styles(theme: &ColorTheme) -> IMenuStyles {
    IMenuStyles {
        shadow_color: theme.get_color("widget.shadow"),
        border_color: theme.get_color("menu.border").or_else(|| theme.get_color("editorWidget.border")),
        foreground_color: theme.get_color("menu.foreground"),
        background_color: theme.get_color("menu.background"),
        selection_foreground_color: theme.get_color("list.hoverForeground"),
        selection_background_color: theme.get_color("list.hoverBackground"),
        selection_border_color: theme.get_color("menu.selectionBorder"),
        separator_color: theme.get_color("menu.separatorBackground"),
        scrollbar_shadow: theme.get_color("scrollbar.shadow"),
        scrollbar_slider_background: theme.get_color("scrollbarSlider.background"),
        scrollbar_slider_hover_background: theme.get_color("scrollbarSlider.hoverBackground"),
        scrollbar_slider_active_background: theme.get_color("scrollbarSlider.activeBackground"),
        disabled_foreground: theme.get_color("disabledForeground"),
    }
}

/// `formatRule(c)`: a regra de CSS que dá ao `.codicon-<id>:before` o caractere da fonte de ícones.
/// `None` se o ícone não existe na fonte (o original lança).
pub fn format_rule(icon: &ThemeIcon) -> Option<String> {
    let character = je_codicons::code_point(&icon.id)?;
    Some(format!(".codicon-{}:before {{ content: '\\{:x}'; }}", icon.id, u32::from(character)))
}
