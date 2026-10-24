//! Porte de `src/vs/workbench/contrib/modernUI/test/browser/frostedGlass.test.ts` (suíte `Frosted glass styles`), caso a
//! caso e na mesma ordem, sobre o resolvedor de estilo nativo do material (`frosted_glass.rs`, a especificação lida de
//! `frostedGlass.css`). O `frostedGlassTestUtils.ts` vira os construtores deste arquivo.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o guarda criado no começo de cada caso.
//! - `supportsGlass(root)` (que consulta `CSS.supports` e `matchMedia`) é o campo `capabilities_supported` da raiz. No
//!   navegador o ambiente decidia o ramo; aqui cada caso roda os dois ramos (`for supported in [true, false]`), e o
//!   ramo sem suporte confere o `supportsGlass ? X : Y` do original.
//! - `readColor(surface, '::before').rgba.a` (arredondado em porcentagem) é o alfa do `GlassPaint::tint`, e `0` quando
//!   a camada não existe (um `::before` sem `content` não pinta). `readColor(element)` do fundo de um elemento é
//!   [`FrostedGlassRoot::effective_token`]: `None` é o `RGBA(0, 0, 0, 0)` de `transparent`.
//! - `getComputedStyle(surface).backdropFilter` é `none` quando `creates_fixed_containing_block` é falso (o vidro
//!   filtra só a camada de material), e `paint.backdropFilter` é o desfoque de `GlassPaint` (`blur(12px)`) ou `none`.
//! - `animationName`, `willChange` e `transform` de uma superfície são `GlassStyle::animation`, `will_change` e a
//!   ausência de animação. Um valor que o vidro não impõe (`None`) é o que a superfície declara fora do `.css`
//!   (`quick-input-widget-open`, `transform, opacity`), então nesses ramos o caso confere `None`.
//! - `new Menu`, `ContextView`, `Action`, `defaultMenuStyles` e os `*.css` importados não têm equivalente: o desenho é
//!   nativo.
//!
//! Recortes (casos que o original decide por geometria, linha de tempo de animação ou pelo DOM de menus, fora do
//! `frostedGlass.css`):
//! - `scales every ${kind} picker in ${windowClass} without an opacity or tint transition` (8 casos), os 4 casos de
//!   `closing motion ...`, `scales the complete ... menu without fading`, `shares the ... menu anchor ...`,
//!   `closes the ... glass and content from the same in-flight scale`, `stops both ... scale animations when motion is
//!   reduced`, `keeps ... nested menu glass stable ...`, `finishes the ... glass entrance before positioning a
//!   submenu`, `keeps ... submenus tinted during closing ...`, `opens ... submenus without animation ...`,
//!   `scales glass menus when the editor Modern UI motion styles are unavailable` e `rounds the menubar menu holder ...`
//!   dependem de `Animation.currentTime`, `getBoundingClientRect`, `transformOrigin` e dos `@keyframes`, ou do vidro dos
//!   menus (`base/browser/ui/menu` e `contextview`), e voltam com esses widgets.
//! - `keeps picker and nested picker tint steady while opening and uses a solid closing fallback` confere a abertura e o
//!   estado assentado; o ramo de fechamento (tint 100 por causa do `@keyframes`) fica de fora.
//! - No `cancelling picker motion ...` só o estado final, depois de trocar para `monaco-reduce-motion`, é conferido.
//! - Os `opacity` de superfícies e conteúdos (`'1'`) não existem no modelo: o vidro nunca anima opacidade.
//! - O `overlays.menu.focus(true)` e a seleção do menu de `preserves solid sticky headings, ...` ficam com o `Menu`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_theme::color::Color;
use je_workbench::contrib::modern_ui::frosted_glass::{
    FrostedGlassRoot, GlassAnimation, GlassStyle, GlassSurface, GlassToken, GlassTokens, MotionMode,
};

const QUICK_INPUT: GlassSurface = GlassSurface::QuickInput { overlay: false };
const HOVER: GlassSurface = GlassSurface::Hover;
const DIALOG: GlassSurface = GlassSurface::Dialog;
const CENTER: GlassSurface = GlassSurface::NotificationsCenter;
const TOAST: GlassSurface = GlassSurface::Toast { fade_in_done: true };

/// O `action-widget action-widget-animated` do `createFrostedGlassOverlays`.
fn picker() -> GlassSurface {
    GlassSurface::ActionWidget {
        animated: true,
        dropdown: false,
        closing: false,
        submenu_panel: false,
        context_view_bottom: false,
        menu_motion_backdrop_percent: 0.0,
    }
}

fn dropdown_picker() -> GlassSurface {
    match picker() {
        GlassSurface::ActionWidget { animated, closing, submenu_panel, context_view_bottom, menu_motion_backdrop_percent, .. } => {
            GlassSurface::ActionWidget { animated, dropdown: true, closing, submenu_panel, context_view_bottom, menu_motion_backdrop_percent }
        }
        _ => unreachable!(),
    }
}

/// As superfícies de `overlays.surfaces` que o `frostedGlass.css` estiliza (os menus ficam de fora, ver o cabeçalho).
fn surfaces() -> [GlassSurface; 6] {
    [QUICK_INPUT, picker(), HOVER, DIALOG, CENTER, TOAST]
}

/// `createFrostedGlassOverlays(store, background)`: a raiz `monaco-workbench modern-ui` com todos os tokens de fundo.
fn create_root(background: &str, supported: bool) -> FrostedGlassRoot {
    let mut root = FrostedGlassRoot::new(GlassTokens::uniform(&Color::from_hex(background)));
    root.capabilities_supported = supported;
    root
}

fn tint_percent(style: &GlassStyle) -> i64 {
    style.paint.as_ref().map_or(0, |paint| (paint.tint.rgba().a * 100.0).round() as i64)
}

fn tints(root: &FrostedGlassRoot, surfaces: &[GlassSurface]) -> Vec<i64> {
    surfaces.iter().map(|surface| tint_percent(&root.style_of(*surface))).collect()
}

fn blur_of(style: &GlassStyle) -> Option<f64> {
    style.paint.as_ref().map(|paint| paint.backdrop_blur_px)
}

fn filter_of(style: &GlassStyle) -> &'static str {
    if style.creates_fixed_containing_block { "blur" } else { "none" }
}

fn alpha_of(color: &Option<Color>) -> f64 {
    color.as_ref().map_or(0.0, |color| color.rgba().a)
}

#[test]
fn keeps_picker_and_nested_picker_tint_steady_while_opening_and_uses_a_solid_closing_fallback() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    for supported in [true, false] {
        let mut root = create_root("#242424", supported);
        root.glass_enabled = true;
        root.motion = MotionMode::Enabled;
        root.opacity_percent = Some(50.0);
        let picker = dropdown_picker();
        let submenu = GlassSurface::ActionWidget {
            animated: false,
            dropdown: false,
            closing: false,
            submenu_panel: true,
            context_view_bottom: false,
            menu_motion_backdrop_percent: 0.0,
        };
        let surfaces = [picker, submenu];
        let picker_style = root.style_of(picker);
        if supported {
            assert!(
                matches!(picker_style.animation, Some(GlassAnimation::ActionWidgetScaleOpen { .. })),
                "o picker precisa de uma animação de abertura de verdade"
            );
        }
        let opening = tints(&root, &surfaces);
        let settled = (
            tints(&root, &surfaces),
            picker_style.will_change,
            // `getComputedStyle(submenu).position`: o painel de submenu mantém o `absolute` da própria regra.
            !root.style_of(submenu).position_relative,
        );
        let expected_tint = if supported { 50 } else { 0 };
        assert_eq!(
            (opening, settled),
            (
                surfaces.iter().map(|_| expected_tint).collect::<Vec<_>>(),
                (surfaces.iter().map(|_| expected_tint).collect::<Vec<_>>(), if supported { Some("transform") } else { None }, true),
            )
        );
    }
}

#[test]
fn cancelling_picker_motion_restores_tint_without_adding_a_transform_containing_block() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    for supported in [true, false] {
        let mut root = create_root("#242424", supported);
        root.glass_enabled = true;
        root.motion = MotionMode::Enabled;
        root.opacity_percent = Some(50.0);
        let picker = dropdown_picker();
        // `root.classList.replace('monaco-enable-motion', 'monaco-reduce-motion')`
        root.motion = MotionMode::Reduced;
        let style = root.style_of(picker);
        assert_eq!(
            (tint_percent(&style), style.will_change.unwrap_or("auto"), style.animation.is_none() && !style.creates_fixed_containing_block),
            (if supported { 50 } else { 0 }, "auto", true)
        );
    }
}

#[test]
fn suppresses_quick_input_motion_without_retaining_backdrop_isolation() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    for supported in [true, false] {
        let mut root = create_root("#242424", supported);
        root.glass_enabled = true;
        root.motion = MotionMode::Enabled;
        let state = |root: &FrostedGlassRoot| {
            let style = root.style_of(QUICK_INPUT);
            (style.animation, style.will_change, filter_of(&style), blur_of(&style))
        };
        // `quick-input-widget-closing` não muda nenhuma decisão do vidro: o estado de fechamento é o de abertura.
        let opening = state(&root);
        let closing = state(&root);
        root.motion = MotionMode::Reduced;
        let reduced_motion = state(&root);

        let blur = if supported { Some(12.0) } else { None };
        let suppressed = (if supported { Some(GlassAnimation::None) } else { None }, if supported { Some("auto") } else { None }, "none", blur);
        assert_eq!((opening, closing, reduced_motion), (suppressed, suppressed, suppressed));
    }
}

#[test]
fn paints_one_non_interactive_material_layer_per_overlay_over_242424() {
    paints_one_non_interactive_material_layer_per_overlay("#242424", "#cccccc", Color::white());
}

#[test]
fn paints_one_non_interactive_material_layer_per_overlay_over_f8f8f8() {
    paints_one_non_interactive_material_layer_per_overlay("#f8f8f8", "#333333", Color::black());
}

fn paints_one_non_interactive_material_layer_per_overlay(background: &str, foreground: &str, backdrop: Color) {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    for supported in [true, false] {
        let mut root = create_root(background, supported);
        root.glass_enabled = true;
        let actual: Vec<_> = surfaces()
            .iter()
            .map(|surface| {
                let style = root.style_of(*surface);
                let tint_is_readable = !supported
                    || style.paint.as_ref().is_some_and(|paint| {
                        paint.tint.rgba().a >= 0.9 && Color::from_hex(foreground).get_contrast_ratio(&paint.tint.make_opaque(&backdrop)) >= 4.5
                    });
                (
                    filter_of(&style),
                    blur_of(&style),
                    // `pointer-events` de um `::before` inexistente é o padrão `auto`.
                    style.paint.as_ref().is_none_or(|paint| !paint.non_interactive),
                    tint_is_readable,
                )
            })
            .collect();
        let expected: Vec<_> = surfaces().iter().map(|_| ("none", if supported { Some(12.0) } else { None }, !supported, true)).collect();
        assert_eq!(actual, expected);
    }
}

#[test]
fn updates_the_tint_across_overlays_and_shadow_menus_without_fading_content() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    for supported in [true, false] {
        let mut root = create_root("#242424", supported);
        root.glass_enabled = true;
        let percentages = [50, 75, 100];
        let actual: Vec<_> = percentages
            .iter()
            .map(|percentage| {
                root.opacity_percent = Some(*percentage as f64);
                tints(&root, &surfaces())
            })
            .collect();
        let expected: Vec<_> = percentages.iter().map(|percentage| surfaces().iter().map(|_| if supported { *percentage } else { 0 }).collect::<Vec<_>>()).collect();
        assert_eq!(actual, expected);
    }
}

/// Os elementos de `overlays.backgrounds` que o `.css` alcança: o token que cada um lê e as superfícies ancestrais
/// (a própria superfície inclusive), da mais externa para a mais interna.
fn background_elements() -> Vec<(GlassToken, Vec<GlassSurface>)> {
    vec![
        (GlassToken::QuickInputBackground, vec![QUICK_INPUT]),
        (GlassToken::MenuBackground, vec![picker()]),
        (GlassToken::MenuBackground, vec![picker()]),
        (GlassToken::EditorHoverWidgetBackground, vec![HOVER]),
        (GlassToken::EditorWidgetBackground, vec![DIALOG]),
        (GlassToken::NotificationsBackground, vec![CENTER]),
        (GlassToken::NotificationsBackground, vec![TOAST]),
        (GlassToken::NotificationsBackground, vec![TOAST]),
    ]
}

fn read_backgrounds(root: &FrostedGlassRoot) -> Vec<Option<Color>> {
    background_elements()
        .into_iter()
        .map(|(token, chain)| {
            let styles: Vec<GlassStyle> = chain.iter().map(|surface| root.style_of(*surface)).collect();
            let refs: Vec<&GlassStyle> = styles.iter().collect();
            root.effective_token(token, &refs, false)
        })
        .collect()
}

#[test]
fn replaces_all_background_layers_together_and_restores_the_exact_theme_backgrounds() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    for supported in [true, false] {
        let mut root = create_root("#242424", supported);
        root.opacity_percent = Some(50.0);
        let original = read_backgrounds(&root);
        root.glass_enabled = true;
        let glass = read_backgrounds(&root);
        root.glass_enabled = false;
        let restored = read_backgrounds(&root);
        let expected_glass: Vec<Option<Color>> = if supported { original.iter().map(|_| None).collect() } else { original.clone() };
        assert_eq!((glass, restored), (expected_glass, original));
    }
}

#[test]
fn does_not_move_fixed_position_children_when_glass_is_enabled() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    for supported in [true, false] {
        let mut root = create_root("#242424", supported);
        // Um filho `position: fixed` só sai do lugar se a superfície cria um bloco de contenção.
        let containers = [QUICK_INPUT, picker()];
        let anchored = |root: &FrostedGlassRoot| containers.iter().map(|surface| !root.style_of(*surface).creates_fixed_containing_block).collect::<Vec<_>>();
        let before = anchored(&root);
        root.glass_enabled = true;
        assert_eq!((before, anchored(&root)), (vec![true, true], vec![true, true]));
    }
}

#[test]
fn preserves_solid_sticky_headings_embedded_editor_backgrounds_and_menu_selection() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    for supported in [true, false] {
        let mut root = create_root("#242424", supported);
        root.opacity_percent = Some(50.0);
        let read = |root: &FrostedGlassRoot| {
            let quick_input = root.style_of(QUICK_INPUT);
            let dialog = root.style_of(DIALOG);
            [
                // `.monaco-tree-sticky-container` dentro do Quick Input.
                root.effective_token(GlassToken::QuickInputBackground, &[&quick_input], true),
                // `.monaco-editor-background` dentro do diálogo.
                root.effective_token(GlassToken::EditorBackground, &[&dialog], false),
            ]
        };
        let before = read(&root);
        root.glass_enabled = true;
        assert_eq!(read(&root), before);
    }
}

#[test]
fn keeps_rich_quick_widgets_and_animating_toasts_on_their_original_backgrounds() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    for supported in [true, false] {
        let mut root = create_root("#242424", supported);
        root.glass_enabled = true;
        let surfaces = [
            (GlassSurface::QuickInput { overlay: true }, GlassToken::QuickInputBackground),
            (GlassSurface::Toast { fade_in_done: false }, GlassToken::NotificationsBackground),
        ];
        let actual: Vec<_> = surfaces
            .iter()
            .map(|(surface, token)| {
                let style = root.style_of(*surface);
                (root.effective_token(*token, &[&style], false), blur_of(&style))
            })
            .collect();
        let original = Some(Color::from_hex("#242424"));
        assert_eq!(actual, vec![(original.clone(), None::<f64>), (original, None::<f64>)]);
    }
}

#[test]
fn paints_motion_free_toasts_without_waiting_for_a_transition_event() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    for supported in [true, false] {
        let mut root = create_root("#242424", supported);
        root.glass_enabled = true;
        root.motion = MotionMode::Reduced;
        let style = root.style_of(GlassSurface::Toast { fade_in_done: false });
        let background = root.effective_token(GlassToken::NotificationsBackground, &[&style], false);
        assert_eq!(
            (filter_of(&style), blur_of(&style), alpha_of(&background)),
            ("none", if supported { Some(12.0) } else { None }, if supported { 0.0 } else { 1.0 })
        );
    }
}

#[test]
fn retains_every_original_background_in_hc_black() {
    retains_every_original_background("hc-black");
}

#[test]
fn retains_every_original_background_in_hc_light() {
    retains_every_original_background("hc-light");
}

fn retains_every_original_background(_theme_class: &str) {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    for supported in [true, false] {
        let mut root = create_root("#242424", supported);
        root.opacity_percent = Some(50.0);
        let before = read_backgrounds(&root);
        // `classList.add('modern-ui-frosted-glass', themeClass)`: `hc-black` e `hc-light` têm o mesmo efeito no seletor
        // `:not(.hc-black):not(.hc-light)`.
        root.glass_enabled = true;
        root.high_contrast = true;
        let filters: Vec<_> = surfaces().iter().map(|surface| blur_of(&root.style_of(*surface))).collect();
        assert_eq!((read_backgrounds(&root), filters), (before, surfaces().iter().map(|_| None).collect::<Vec<Option<f64>>>()));
    }
}

#[test]
fn updates_the_material_when_theme_tokens_change_without_changing_text_opacity() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    for supported in [true, false] {
        let mut root = create_root("#242424", supported);
        root.opacity_percent = Some(75.0);
        root.glass_enabled = true;
        root.tokens.set(GlassToken::QuickInputBackground, Color::from_hex("#ffffff"));
        let paint = root.style_of(QUICK_INPUT).paint.map(|paint| {
            let rgba = paint.tint.rgba();
            (rgba.r, rgba.g, rgba.b, (rgba.a * 100.0).round() as i64)
        });
        assert_eq!(paint, if supported { Some((255, 255, 255, 75)) } else { None });
    }
}
