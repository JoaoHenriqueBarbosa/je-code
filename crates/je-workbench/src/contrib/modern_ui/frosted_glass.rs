//! Porte de `src/vs/workbench/contrib/modernUI/browser/media/frostedGlass.css`: o material de vidro fosco das
//! sobreposições do workbench (Quick Input, pickers, hovers, diálogos e notificações), como um resolvedor de estilo
//! nativo. O `.css` é a especificação (CLAUDE.md do projeto): aqui cada regra vira uma decisão sobre o estado do
//! contêiner raiz e da superfície, e o resultado é o que o rasterizador pinta.
//!
//! Recortes:
//! - Os menus de contexto (`ContextView` e `Menu`) recebem o vidro em `base/browser/ui/menu/menu.ts` e
//!   `contextview.ts`, não neste `.css`: ficam com o porte desses widgets.
//! - As linhas de tempo das animações (`@keyframes action-widget-scale-open`, `action-widget-dropdown-close`,
//!   `quick-input-widget-open`) moram em `platform/actionWidget` e `platform/quickinput`. Este módulo decide só o que
//!   o `.css` decide: qual animação a superfície usa, sem avançar o relógio. A variável
//!   `--vscode-context-view-menu-motion-backdrop-opacity`, que as animações movem, é a entrada
//!   [`GlassSurface::ActionWidget::menu_motion_backdrop_percent`].
//! - `isolation: isolate` e `z-index: -1` do `::before` são do empilhamento do DOM: a árvore nativa pinta a camada de
//!   material antes dos filhos, e [`GlassPaint`] só diz que ela existe.
//!
//! Divergências:
//! - O `@supports (...)` e o `@media (prefers-reduced-transparency: no-preference) and (forced-colors: none)` são a
//!   capacidade [`FrostedGlassRoot::capabilities_supported`], decidida por quem desenha (GPU com desfoque do fundo, sem
//!   transparência reduzida e sem cores forçadas).
//! - `.monaco-workbench.modern-ui-frosted-glass:not(.hc-black):not(.hc-light)` são `glass_enabled` e `high_contrast`.
//! - `color-mix(in srgb, X p%, transparent)` é a cor `X` com o alfa multiplicado por `p/100`; o
//!   `color-mix(in srgb, solid b%, glass)` do action widget soma `b + (100 - b) * glass / 100` ao alfa.
//! - As variáveis `--vscode-*-background: transparent` que a superfície redefine para os filhos são a lista
//!   [`GlassStyle::transparent_tokens`], lida por [`FrostedGlassRoot::effective_token`].

use je_theme::color::{Color, Srgba};

/// `backdrop-filter: blur(12px)`.
pub const FROSTED_GLASS_BLUR_PX: f64 = 12.0;

/// O padrão do `var(--modern-ui-glass-opacity, 92%)`.
pub const DEFAULT_GLASS_OPACITY_PERCENT: f64 = 92.0;

/// A duração da animação de abertura do picker (`250ms`).
pub const ACTION_WIDGET_OPEN_DURATION_MS: u32 = 250;

/// A duração da animação de fechamento do dropdown (`150ms`).
pub const ACTION_WIDGET_DROPDOWN_CLOSE_DURATION_MS: u32 = 150;

/// As classes `monaco-enable-motion` e `monaco-reduce-motion` do contêiner raiz.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MotionMode {
    Enabled,
    Reduced,
    Unspecified,
}

/// Os tokens de tema que o vidro substitui ou lê (`--vscode-<token>`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GlassToken {
    QuickInputBackground,
    MenuBackground,
    EditorHoverWidgetBackground,
    EditorWidgetBackground,
    NotificationsBackground,
    EditorBackground,
    InputBackground,
}

/// Os valores dos tokens no contêiner raiz.
#[derive(Clone, Debug, PartialEq)]
pub struct GlassTokens {
    pub quick_input_background: Color,
    pub menu_background: Color,
    pub editor_hover_widget_background: Color,
    pub editor_widget_background: Color,
    pub notifications_background: Color,
    pub editor_background: Color,
    pub input_background: Color,
}

impl GlassTokens {
    /// Todos os tokens com a mesma cor.
    pub fn uniform(color: &Color) -> Self {
        Self {
            quick_input_background: color.clone(),
            menu_background: color.clone(),
            editor_hover_widget_background: color.clone(),
            editor_widget_background: color.clone(),
            notifications_background: color.clone(),
            editor_background: color.clone(),
            input_background: color.clone(),
        }
    }

    pub fn get(&self, token: GlassToken) -> &Color {
        match token {
            GlassToken::QuickInputBackground => &self.quick_input_background,
            GlassToken::MenuBackground => &self.menu_background,
            GlassToken::EditorHoverWidgetBackground => &self.editor_hover_widget_background,
            GlassToken::EditorWidgetBackground => &self.editor_widget_background,
            GlassToken::NotificationsBackground => &self.notifications_background,
            GlassToken::EditorBackground => &self.editor_background,
            GlassToken::InputBackground => &self.input_background,
        }
    }

    pub fn set(&mut self, token: GlassToken, color: Color) {
        match token {
            GlassToken::QuickInputBackground => self.quick_input_background = color,
            GlassToken::MenuBackground => self.menu_background = color,
            GlassToken::EditorHoverWidgetBackground => self.editor_hover_widget_background = color,
            GlassToken::EditorWidgetBackground => self.editor_widget_background = color,
            GlassToken::NotificationsBackground => self.notifications_background = color,
            GlassToken::EditorBackground => self.editor_background = color,
            GlassToken::InputBackground => self.input_background = color,
        }
    }
}

/// O estado do contêiner raiz (`.monaco-workbench`) que as regras do vidro leem.
#[derive(Clone, Debug, PartialEq)]
pub struct FrostedGlassRoot {
    /// A classe `modern-ui-frosted-glass`.
    pub glass_enabled: bool,
    /// `hc-black` ou `hc-light`.
    pub high_contrast: bool,
    pub motion: MotionMode,
    /// Desfoque do fundo e `color-mix` disponíveis, transparência não reduzida e sem cores forçadas.
    pub capabilities_supported: bool,
    /// `--modern-ui-glass-opacity` em porcentagem; `None` usa [`DEFAULT_GLASS_OPACITY_PERCENT`].
    pub opacity_percent: Option<f64>,
    pub tokens: GlassTokens,
}

/// Cada superfície que o `.css` estiliza.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GlassSurface {
    /// `.quick-input-widget`; `overlay` é `.quick-input-widget-overlay` (widgets ricos, que ficam opacos).
    QuickInput { overlay: bool },
    /// `.action-widget`.
    ActionWidget {
        /// `.action-widget-animated`.
        animated: bool,
        /// `.action-widget-dropdown`.
        dropdown: bool,
        /// `.action-widget-dropdown-closing`.
        closing: bool,
        /// `.action-list-submenu-panel`.
        submenu_panel: bool,
        /// `.context-view.bottom` como ancestral.
        context_view_bottom: bool,
        /// `--vscode-context-view-menu-motion-backdrop-opacity`, em porcentagem (0 por padrão).
        menu_motion_backdrop_percent: f64,
    },
    /// `.monaco-hover` e `.monaco-resizable-hover`.
    Hover,
    /// `.monaco-dialog-box`.
    Dialog,
    /// `.notifications-center`.
    NotificationsCenter,
    /// `.notifications-toasts .notification-toast`; `fade_in_done` é `.notification-fade-in-done`, e `false` é
    /// `.notification-fade-in` (a entrada em andamento).
    Toast { fade_in_done: bool },
}

impl GlassSurface {
    /// Um action widget sem nenhuma das classes opcionais.
    pub const fn plain_action_widget() -> Self {
        Self::ActionWidget {
            animated: false,
            dropdown: false,
            closing: false,
            submenu_panel: false,
            context_view_bottom: false,
            menu_motion_backdrop_percent: 0.0,
        }
    }
}

/// A camada de material (`::before`).
#[derive(Clone, Debug, PartialEq)]
pub struct GlassPaint {
    pub backdrop_blur_px: f64,
    /// `background-color: var(--modern-ui-glass-background)`, já com o alfa do material.
    pub tint: Color,
    /// `pointer-events: none`: a camada nunca é interativa.
    pub non_interactive: bool,
}

/// `animation:` que o vidro impõe à superfície.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GlassAnimation {
    /// `animation: none`.
    None,
    /// `action-widget-scale-open 250ms cubic-bezier(0.22, 1, 0.36, 1) backwards`.
    ActionWidgetScaleOpen { duration_ms: u32, origin: TransformOrigin },
    /// `action-widget-dropdown-close 150ms cubic-bezier(0.22, 1, 0.36, 1) both`.
    ActionWidgetDropdownClose { duration_ms: u32 },
}

/// `transform-origin` do picker.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransformOrigin {
    BottomLeft,
    TopLeft,
}

/// O que as regras do vidro decidem para uma superfície.
#[derive(Clone, Debug, PartialEq)]
pub struct GlassStyle {
    /// `None` quando nenhuma regra casa (a superfície mantém o fundo original).
    pub paint: Option<GlassPaint>,
    /// Os tokens que a superfície redefine como `transparent` para os descendentes.
    pub transparent_tokens: Vec<GlassToken>,
    /// Os tokens que o contêiner fixo de títulos (`.monaco-tree-sticky-container`) restaura para o valor sólido.
    pub sticky_container_solid_tokens: Vec<GlassToken>,
    pub animation: Option<GlassAnimation>,
    /// `will-change` imposto; `None` deixa o valor da superfície.
    pub will_change: Option<&'static str>,
    /// `position: relative`.
    pub position_relative: bool,
    /// `pointer-events: none` na própria superfície (dropdown em fechamento).
    pub pointer_events_none: bool,
    /// Se a superfície cria um bloco de contenção para descendentes `position: fixed` (filtro, transformação ou
    /// `backdrop-filter` próprios). O vidro filtra só a camada de material, então nunca cria.
    pub creates_fixed_containing_block: bool,
}

impl GlassStyle {
    /// Nenhuma regra casou.
    pub fn inactive() -> Self {
        Self {
            paint: None,
            transparent_tokens: Vec::new(),
            sticky_container_solid_tokens: Vec::new(),
            animation: None,
            will_change: None,
            position_relative: false,
            pointer_events_none: false,
            creates_fixed_containing_block: false,
        }
    }
}

impl FrostedGlassRoot {
    pub fn new(tokens: GlassTokens) -> Self {
        Self { glass_enabled: false, high_contrast: false, motion: MotionMode::Unspecified, capabilities_supported: true, opacity_percent: None, tokens }
    }

    /// O seletor do contêiner raiz casa e as consultas `@supports` e `@media` passam.
    pub fn is_active(&self) -> bool {
        self.capabilities_supported && self.glass_enabled && !self.high_contrast
    }

    fn glass_opacity(&self) -> f64 {
        self.opacity_percent.unwrap_or(DEFAULT_GLASS_OPACITY_PERCENT)
    }

    /// `color-mix(in srgb, <token> <percent>%, transparent)`.
    fn mix_with_transparent(&self, color: &Color, percent: f64) -> Color {
        let rgba = color.rgba();
        Color::from(Srgba::new(rgba.r, rgba.g, rgba.b, rgba.a * percent / 100.0))
    }

    fn paint(&self, color: &Color, backdrop_percent: f64) -> GlassPaint {
        let glass = self.mix_with_transparent(color, self.glass_opacity());
        // `color-mix(in srgb, <solid> <backdrop>%, <glass>)`: o sólido cobre `backdrop`% e o vidro, o resto.
        let tint = if backdrop_percent == 0.0 {
            glass
        } else {
            let solid = color.rgba();
            let glass_alpha = glass.rgba().a;
            let alpha = backdrop_percent / 100.0 * solid.a + (1.0 - backdrop_percent / 100.0) * glass_alpha;
            Color::from(Srgba::new(solid.r, solid.g, solid.b, alpha))
        };
        GlassPaint { backdrop_blur_px: FROSTED_GLASS_BLUR_PX, tint, non_interactive: true }
    }

    /// As regras do `.css` para uma superfície.
    pub fn style_of(&self, surface: GlassSurface) -> GlassStyle {
        if !self.is_active() {
            return GlassStyle::inactive();
        }
        let tokens = &self.tokens;
        let mut style = GlassStyle::inactive();
        match surface {
            GlassSurface::QuickInput { overlay: true } => return style,
            GlassSurface::QuickInput { overlay: false } => {
                style.paint = Some(self.paint(&tokens.quick_input_background, 0.0));
                style.transparent_tokens = vec![GlassToken::QuickInputBackground];
                style.sticky_container_solid_tokens = vec![GlassToken::QuickInputBackground];
                // Uma animação de opacidade persistente isolaria o desfoque do fundo do workbench.
                style.animation = Some(GlassAnimation::None);
                style.will_change = Some("auto");
            }
            GlassSurface::ActionWidget { animated, dropdown, closing, submenu_panel, context_view_bottom, menu_motion_backdrop_percent } => {
                style.paint = Some(self.paint(&tokens.menu_background, menu_motion_backdrop_percent));
                style.transparent_tokens = vec![GlassToken::MenuBackground];
                style.position_relative = !submenu_panel;
                if self.motion == MotionMode::Enabled {
                    if animated && !closing {
                        let origin = if context_view_bottom { TransformOrigin::TopLeft } else { TransformOrigin::BottomLeft };
                        style.animation = Some(GlassAnimation::ActionWidgetScaleOpen { duration_ms: ACTION_WIDGET_OPEN_DURATION_MS, origin });
                    }
                    if dropdown {
                        // Mantém o bloco de contenção existente sem reter o isolamento de opacidade.
                        style.will_change = Some("transform");
                    }
                    if closing {
                        style.animation = Some(GlassAnimation::ActionWidgetDropdownClose { duration_ms: ACTION_WIDGET_DROPDOWN_CLOSE_DURATION_MS });
                        style.pointer_events_none = true;
                    }
                }
            }
            GlassSurface::Hover => {
                style.paint = Some(self.paint(&tokens.editor_hover_widget_background, 0.0));
                style.transparent_tokens = vec![GlassToken::EditorHoverWidgetBackground];
            }
            GlassSurface::Dialog => {
                style.paint = Some(self.paint(&tokens.editor_widget_background, 0.0));
                style.transparent_tokens = vec![GlassToken::EditorWidgetBackground];
            }
            GlassSurface::NotificationsCenter => {
                style.paint = Some(self.paint(&tokens.notifications_background, 0.0));
                style.transparent_tokens = vec![GlassToken::NotificationsBackground];
            }
            GlassSurface::Toast { fade_in_done } => {
                if !fade_in_done && self.motion != MotionMode::Reduced {
                    return style;
                }
                style.paint = Some(self.paint(&tokens.notifications_background, 0.0));
                style.transparent_tokens = vec![GlassToken::NotificationsBackground];
                style.position_relative = true;
            }
        }
        style
    }

    /// O valor de `--vscode-<token>` para um descendente das superfícies de `chain` (da mais externa para a mais
    /// interna). `None` é `transparent`. `in_sticky_container` é o descendente dentro de um
    /// `.monaco-tree-sticky-container`, que restaura o valor sólido.
    pub fn effective_token(&self, token: GlassToken, chain: &[&GlassStyle], in_sticky_container: bool) -> Option<Color> {
        let solid = Some(self.tokens.get(token).clone());
        if in_sticky_container && chain.iter().any(|style| style.sticky_container_solid_tokens.contains(&token)) {
            return solid;
        }
        if chain.iter().any(|style| style.transparent_tokens.contains(&token)) { None } else { solid }
    }
}
