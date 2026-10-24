//! Porte da geração das regras CSS de decoração de `src/vs/editor/browser/services/abstractCodeEditorService.ts`
//! (`registerDecorationType`, `resolveDecorationOptions`, `DecorationCSSRules`, os provedores de opções e o
//! `GlobalStyleSheet`) com os tipos de render de `IDecorationRenderOptions` de `src/vs/editor/common/editorCommon.ts`.
//!
//! O CSS gerado é a especificação (a UI é nativa): as regras saem como dados, um [`CssRule`] com seletor e texto
//! das propriedades, na [`GlobalStyleSheet`], que faz o papel do `CSSStyleSheet` do DOM (com a mesma ordem:
//! cada regra nova entra na frente). Quem pinta lê as regras de lá.
//!
//! Diferenças do original:
//! - Só a parte de decorações do `AbstractCodeEditorService` existe, no [`DecorationRenderService`]. A lista de
//!   editores, os editores de diferença, o foco, as propriedades de modelo, a abertura de editor e os
//!   `ModelTransientSettingWatcher` pertencem ao serviço de editores de código e ficam para o porte dele;
//!   por isso o `removeDecorationType` não chama `removeDecorationsByType` nos editores.
//! - Não existe folha de estilos por editor (`RefCountedStyleSheet`): ela só existe para um editor dentro de
//!   Shadow DOM, e a UI nativa tem uma folha só. `registerDecorationType` não recebe o `editor`, e
//!   `ref`/`unref` da folha (vazios na global) não existem.
//! - `IDecorationRenderOptions` e `IThemeDecorationRenderOptions` são os structs [`DecorationRenderOptions`] e
//!   [`ThemeDecorationRenderOptions`] (este achatado no primeiro por `base`, que é a herança do original); a
//!   propriedade dinâmica `opts[property]` do `collectCSSText` é o [`CssProperty`], lido por
//!   [`CssPropertySource`]. `string | ThemeColor` é o [`DecorationColor`] do modelo, e `UriComponents` é a
//!   [`URI`] (o `URI.revive` do original já está feito).
//! - `IModelDecorationOptions` é o [`ModelDecorationOptionsInit`]. O `getOptions(writable)` com `writable`
//!   falso devolve o próprio provedor no original (que também leva `inlineClassNameAffectsLetterSpacing`); aqui
//!   devolve um valor com esse campo preenchido, e com `writable` verdadeiro um valor sem ele.
//! - `throw new Error(...)` é `throw(Error)` (pânico que `errors::catch` captura).
//! - `asCSSUrl` precisa do `FileAccess.uriToBrowserUri`: o serviço o recebe em [`DecorationRenderService::new`].
//! - O `CSSRuleList` de `resolveDecorationCSSRules` é um `Vec<CssRule>`.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::errors::{Error, throw};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{DisposableRef, IDisposable};
use je_base::map::OrderedMap;
use je_base::uri::URI;

use crate::model::{
    DecorationColor, InjectedTextOptions, ModelDecorationOptionsInit, ModelDecorationOverviewRulerInit, OverviewRulerLane,
    TrackedRangeStickiness,
};
use crate::services::css_value::as_css_url;
use crate::services::theme_service::{IColorTheme, IThemeService};

// ---- IDecorationRenderOptions

/// `IContentDecorationRenderOptions`.
#[derive(Clone, Debug, Default)]
pub struct ContentDecorationRenderOptions {
    pub content_text: Option<String>,
    pub content_icon_path: Option<URI>,

    pub border: Option<String>,
    pub border_color: Option<DecorationColor>,
    pub border_radius: Option<String>,
    pub font_style: Option<String>,
    pub font_weight: Option<String>,
    pub font_size: Option<String>,
    pub font_family: Option<String>,
    pub text_decoration: Option<String>,
    pub color: Option<DecorationColor>,
    pub background_color: Option<DecorationColor>,
    pub opacity: Option<String>,
    pub vertical_align: Option<String>,

    pub margin: Option<String>,
    pub padding: Option<String>,
    pub width: Option<String>,
    pub height: Option<String>,
}

/// `IContentDecorationRenderOptions & { affectsLetterSpacing?: boolean }`: o tipo de `beforeInjectedText` e
/// `afterInjectedText`.
#[derive(Clone, Debug, Default)]
pub struct InjectedTextRenderOptions {
    pub content: ContentDecorationRenderOptions,
    pub affects_letter_spacing: Option<bool>,
}

/// `IThemeDecorationRenderOptions`.
#[derive(Clone, Debug, Default)]
pub struct ThemeDecorationRenderOptions {
    pub background_color: Option<DecorationColor>,

    pub outline: Option<String>,
    pub outline_color: Option<DecorationColor>,
    pub outline_style: Option<String>,
    pub outline_width: Option<String>,

    pub border: Option<String>,
    pub border_color: Option<DecorationColor>,
    pub border_radius: Option<String>,
    pub border_spacing: Option<String>,
    pub border_style: Option<String>,
    pub border_width: Option<String>,

    pub font_style: Option<String>,
    pub font_weight: Option<String>,
    pub font_family: Option<String>,
    pub font_size: Option<String>,
    pub line_height: Option<f64>,
    pub text_decoration: Option<String>,
    pub cursor: Option<String>,
    pub color: Option<DecorationColor>,
    pub opacity: Option<String>,
    pub letter_spacing: Option<String>,

    pub gutter_icon_path: Option<URI>,
    pub gutter_icon_size: Option<String>,

    pub overview_ruler_color: Option<DecorationColor>,

    pub before: Option<ContentDecorationRenderOptions>,
    pub after: Option<ContentDecorationRenderOptions>,

    pub before_injected_text: Option<InjectedTextRenderOptions>,
    pub after_injected_text: Option<InjectedTextRenderOptions>,
}

/// `IDecorationRenderOptions`: as opções de tema (`base`) mais as de decoração e as variantes `light` e `dark`.
#[derive(Clone, Debug, Default)]
pub struct DecorationRenderOptions {
    pub base: ThemeDecorationRenderOptions,
    pub is_whole_line: Option<bool>,
    pub range_behavior: Option<TrackedRangeStickiness>,
    pub overview_ruler_lane: Option<OverviewRulerLane>,

    pub light: Option<ThemeDecorationRenderOptions>,
    pub dark: Option<ThemeDecorationRenderOptions>,
}

// ---- _CSS_MAP

/// As chaves de `_CSS_MAP`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CssProperty {
    Color,
    Opacity,
    BackgroundColor,
    Outline,
    OutlineColor,
    OutlineStyle,
    OutlineWidth,
    Border,
    BorderColor,
    BorderRadius,
    BorderSpacing,
    BorderStyle,
    BorderWidth,
    FontStyle,
    FontWeight,
    FontSize,
    FontFamily,
    TextDecoration,
    Cursor,
    LetterSpacing,
    GutterIconPath,
    GutterIconSize,
    ContentText,
    ContentIconPath,
    Margin,
    Padding,
    Width,
    Height,
    VerticalAlign,
}

impl CssProperty {
    /// O valor de `_CSS_MAP[property]`: o modelo da regra, com `{0}` no lugar do valor.
    pub fn template(self) -> &'static str {
        match self {
            CssProperty::Color => "color:{0} !important;",
            CssProperty::Opacity => "opacity:{0};",
            CssProperty::BackgroundColor => "background-color:{0};",
            CssProperty::Outline => "outline:{0};",
            CssProperty::OutlineColor => "outline-color:{0};",
            CssProperty::OutlineStyle => "outline-style:{0};",
            CssProperty::OutlineWidth => "outline-width:{0};",
            CssProperty::Border => "border:{0};",
            CssProperty::BorderColor => "border-color:{0};",
            CssProperty::BorderRadius => "border-radius:{0};",
            CssProperty::BorderSpacing => "border-spacing:{0};",
            CssProperty::BorderStyle => "border-style:{0};",
            CssProperty::BorderWidth => "border-width:{0};",
            CssProperty::FontStyle => "font-style:{0};",
            CssProperty::FontWeight => "font-weight:{0};",
            CssProperty::FontSize => "font-size:{0};",
            CssProperty::FontFamily => "font-family:{0};",
            CssProperty::TextDecoration => "text-decoration:{0};",
            CssProperty::Cursor => "cursor:{0};",
            CssProperty::LetterSpacing => "letter-spacing:{0};",
            CssProperty::GutterIconPath => "background:{0} center center no-repeat;",
            CssProperty::GutterIconSize => "background-size:{0};",
            CssProperty::ContentText => "content:'{0}';",
            CssProperty::ContentIconPath => "content:{0};",
            CssProperty::Margin => "margin:{0};",
            CssProperty::Padding => "padding:{0};",
            CssProperty::Width => "width:{0};",
            CssProperty::Height => "height:{0};",
            CssProperty::VerticalAlign => "vertical-align:{0};",
        }
    }

    /// `strings.format(_CSS_MAP[property], value)`.
    fn format(self, value: &str) -> String {
        self.template().replace("{0}", value)
    }
}

/// O `opts[property]` de `collectCSSText`: o valor (texto ou cor do tema) da propriedade, quando o objeto de
/// opções a tem preenchida.
pub trait CssPropertySource {
    fn property(&self, property: CssProperty) -> Option<DecorationColor>;
}

fn plain(value: &Option<String>) -> Option<DecorationColor> {
    value.clone().map(DecorationColor::Color)
}

impl CssPropertySource for ThemeDecorationRenderOptions {
    fn property(&self, property: CssProperty) -> Option<DecorationColor> {
        match property {
            CssProperty::BackgroundColor => self.background_color.clone(),
            CssProperty::Outline => plain(&self.outline),
            CssProperty::OutlineColor => self.outline_color.clone(),
            CssProperty::OutlineStyle => plain(&self.outline_style),
            CssProperty::OutlineWidth => plain(&self.outline_width),
            CssProperty::Border => plain(&self.border),
            CssProperty::BorderColor => self.border_color.clone(),
            CssProperty::BorderRadius => plain(&self.border_radius),
            CssProperty::BorderSpacing => plain(&self.border_spacing),
            CssProperty::BorderStyle => plain(&self.border_style),
            CssProperty::BorderWidth => plain(&self.border_width),
            CssProperty::FontStyle => plain(&self.font_style),
            CssProperty::FontWeight => plain(&self.font_weight),
            CssProperty::FontFamily => plain(&self.font_family),
            CssProperty::FontSize => plain(&self.font_size),
            CssProperty::TextDecoration => plain(&self.text_decoration),
            CssProperty::Cursor => plain(&self.cursor),
            CssProperty::Color => self.color.clone(),
            CssProperty::Opacity => plain(&self.opacity),
            CssProperty::LetterSpacing => plain(&self.letter_spacing),
            _ => None,
        }
    }
}

impl CssPropertySource for ContentDecorationRenderOptions {
    fn property(&self, property: CssProperty) -> Option<DecorationColor> {
        match property {
            CssProperty::Border => plain(&self.border),
            CssProperty::BorderColor => self.border_color.clone(),
            CssProperty::BorderRadius => plain(&self.border_radius),
            CssProperty::FontStyle => plain(&self.font_style),
            CssProperty::FontWeight => plain(&self.font_weight),
            CssProperty::FontSize => plain(&self.font_size),
            CssProperty::FontFamily => plain(&self.font_family),
            CssProperty::TextDecoration => plain(&self.text_decoration),
            CssProperty::Color => self.color.clone(),
            CssProperty::BackgroundColor => self.background_color.clone(),
            CssProperty::Opacity => plain(&self.opacity),
            CssProperty::VerticalAlign => plain(&self.vertical_align),
            CssProperty::Margin => plain(&self.margin),
            CssProperty::Padding => plain(&self.padding),
            CssProperty::Width => plain(&self.width),
            CssProperty::Height => plain(&self.height),
            _ => None,
        }
    }
}

// ---- GlobalStyleSheet

/// Uma regra da folha: o seletor e o texto das propriedades (`selector {cssText}`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CssRule {
    pub selector: String,
    pub css_text: String,
}

impl CssRule {
    /// A regra em texto CSS: `selector {cssText}`.
    pub fn to_css_text(&self) -> String {
        format!("{} {{{}}}", self.selector, self.css_text)
    }
}

/// `GlobalStyleSheet`: a coleção de regras, a mais nova na frente.
#[derive(Default)]
pub struct GlobalStyleSheet {
    rules: RefCell<Vec<CssRule>>,
}

impl GlobalStyleSheet {
    pub fn new() -> Self {
        Self::default()
    }

    /// `insertRule(selector, rule)` (o `createCSSRule` do DOM: texto vazio não gera regra).
    pub fn insert_rule(&self, selector: &str, css_text: &str) {
        if css_text.is_empty() {
            return;
        }
        self.rules.borrow_mut().insert(0, CssRule { selector: selector.to_string(), css_text: css_text.to_string() });
    }

    /// `removeRulesContainingSelector(ruleName)`: tira toda regra cujo seletor contém o texto.
    pub fn remove_rules_containing_selector(&self, rule_name: &str) {
        self.rules.borrow_mut().retain(|rule| !rule.selector.contains(rule_name));
    }

    /// `sheet.cssRules`.
    pub fn css_rules(&self) -> Vec<CssRule> {
        self.rules.borrow().clone()
    }
}

// ---- providers

struct ProviderArguments {
    style_sheet: Rc<GlobalStyleSheet>,
    key: String,
    parent_type_key: Option<String>,
    options: DecorationRenderOptions,
    uri_to_browser_uri: Rc<dyn Fn(&URI) -> URI>,
}

/// `ModelDecorationCSSRuleType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ModelDecorationCssRuleType {
    ClassName = 0,
    InlineClassName = 1,
    GlyphMarginClassName = 2,
    BeforeContentClassName = 3,
    AfterContentClassName = 4,
    BeforeInjectedTextClassName = 5,
    AfterInjectedTextClassName = 6,
}

/// `CSSNameHelper`.
mod css_name_helper {
    use super::ModelDecorationCssRuleType;

    pub fn get_class_name(key: &str, rule_type: ModelDecorationCssRuleType) -> String {
        format!("ced-{key}-{}", rule_type as i32)
    }

    pub fn get_selector(key: &str, parent_key: Option<&str>, rule_type: ModelDecorationCssRuleType) -> String {
        let mut selector = format!(".monaco-editor .{}", get_class_name(key, rule_type));
        if let Some(parent_key) = parent_key.filter(|parent| !parent.is_empty()) {
            selector = format!("{selector}.{}", get_class_name(parent_key, rule_type));
        }
        match rule_type {
            ModelDecorationCssRuleType::BeforeContentClassName => selector.push_str("::before"),
            ModelDecorationCssRuleType::AfterContentClassName => selector.push_str("::after"),
            _ => {}
        }
        selector
    }
}

/// O estado de um `DecorationCSSRules` que o ouvinte de mudança de tema também lê e muda.
struct RulesState {
    theme: RefCell<Rc<dyn IColorTheme>>,
    class_name: String,
    un_themed_selector: String,
    has_content: Cell<bool>,
    has_letter_spacing: Cell<bool>,
    rule_type: ModelDecorationCssRuleType,
    provider_args: Rc<ProviderArguments>,
    uses_theme_colors: Cell<bool>,
}

/// `DecorationCSSRules`.
struct DecorationCssRules {
    state: Rc<RulesState>,
    theme_listener: RefCell<Option<DisposableRef>>,
}

impl DecorationCssRules {
    fn new(rule_type: ModelDecorationCssRuleType, provider_args: Rc<ProviderArguments>, theme_service: &Rc<dyn IThemeService>) -> Self {
        let mut class_name = css_name_helper::get_class_name(&provider_args.key, rule_type);
        if let Some(parent_type_key) = provider_args.parent_type_key.as_deref().filter(|parent| !parent.is_empty()) {
            class_name = format!("{class_name} {}", css_name_helper::get_class_name(parent_type_key, rule_type));
        }
        let un_themed_selector = css_name_helper::get_selector(&provider_args.key, provider_args.parent_type_key.as_deref(), rule_type);

        let state = Rc::new(RulesState {
            theme: RefCell::new(theme_service.get_color_theme()),
            class_name,
            un_themed_selector,
            has_content: Cell::new(false),
            has_letter_spacing: Cell::new(false),
            rule_type,
            provider_args,
            uses_theme_colors: Cell::new(false),
        });

        state.build_css();

        let theme_listener = if state.uses_theme_colors.get() {
            let listener_state = state.clone();
            let listener_theme_service = theme_service.clone();
            Some(theme_service.on_did_color_theme_change().listen(move |_| {
                *listener_state.theme.borrow_mut() = listener_theme_service.get_color_theme();
                listener_state.remove_css();
                listener_state.build_css();
            }))
        } else {
            None
        };

        Self { state, theme_listener: RefCell::new(theme_listener) }
    }

    fn has_content(&self) -> bool {
        self.state.has_content.get()
    }

    fn has_letter_spacing(&self) -> bool {
        self.state.has_letter_spacing.get()
    }

    fn class_name(&self) -> &str {
        &self.state.class_name
    }
}

impl IDisposable for DecorationCssRules {
    fn dispose(&self) {
        if self.state.has_content.get() {
            self.state.remove_css();
            self.state.has_content.set(false);
        }
        if let Some(listener) = self.theme_listener.borrow_mut().take() {
            listener.dispose();
        }
    }
}

impl RulesState {
    fn build_css(&self) {
        use ModelDecorationCssRuleType as RuleType;

        let options = &self.provider_args.options;
        let light = options.light.as_ref();
        let dark = options.dark.as_ref();
        let (unthemed_css, light_css, dark_css) = match self.rule_type {
            RuleType::ClassName => (
                self.css_text_for_class_name(Some(&options.base)),
                self.css_text_for_class_name(light),
                self.css_text_for_class_name(dark),
            ),
            RuleType::InlineClassName => (
                self.css_text_for_inline_class_name(Some(&options.base)),
                self.css_text_for_inline_class_name(light),
                self.css_text_for_inline_class_name(dark),
            ),
            RuleType::GlyphMarginClassName => (
                self.css_text_for_glyph_margin_class_name(Some(&options.base)),
                self.css_text_for_glyph_margin_class_name(light),
                self.css_text_for_glyph_margin_class_name(dark),
            ),
            RuleType::BeforeContentClassName => (
                self.css_text_for_content_class_name(options.base.before.as_ref()),
                self.css_text_for_content_class_name(light.and_then(|theme| theme.before.as_ref())),
                self.css_text_for_content_class_name(dark.and_then(|theme| theme.before.as_ref())),
            ),
            RuleType::AfterContentClassName => (
                self.css_text_for_content_class_name(options.base.after.as_ref()),
                self.css_text_for_content_class_name(light.and_then(|theme| theme.after.as_ref())),
                self.css_text_for_content_class_name(dark.and_then(|theme| theme.after.as_ref())),
            ),
            RuleType::BeforeInjectedTextClassName => (
                self.css_text_for_content_class_name(options.base.before_injected_text.as_ref().map(|text| &text.content)),
                self.css_text_for_content_class_name(light.and_then(|theme| theme.before_injected_text.as_ref()).map(|text| &text.content)),
                self.css_text_for_content_class_name(dark.and_then(|theme| theme.before_injected_text.as_ref()).map(|text| &text.content)),
            ),
            RuleType::AfterInjectedTextClassName => (
                self.css_text_for_content_class_name(options.base.after_injected_text.as_ref().map(|text| &text.content)),
                self.css_text_for_content_class_name(light.and_then(|theme| theme.after_injected_text.as_ref()).map(|text| &text.content)),
                self.css_text_for_content_class_name(dark.and_then(|theme| theme.after_injected_text.as_ref()).map(|text| &text.content)),
            ),
        };
        let sheet = &self.provider_args.style_sheet;

        let mut has_content = false;
        if !unthemed_css.is_empty() {
            sheet.insert_rule(&self.un_themed_selector, &unthemed_css);
            has_content = true;
        }
        if !light_css.is_empty() {
            sheet.insert_rule(&format!(".vs{0}, .hc-light{0}", self.un_themed_selector), &light_css);
            has_content = true;
        }
        if !dark_css.is_empty() {
            sheet.insert_rule(&format!(".vs-dark{0}, .hc-black{0}", self.un_themed_selector), &dark_css);
            has_content = true;
        }
        self.has_content.set(has_content);
    }

    fn remove_css(&self) {
        self.provider_args.style_sheet.remove_rules_containing_selector(&self.un_themed_selector);
    }

    /// Build the CSS for decorations styled via `className`.
    fn css_text_for_class_name(&self, opts: Option<&ThemeDecorationRenderOptions>) -> String {
        let Some(opts) = opts else {
            return String::new();
        };
        let mut css_text_arr: Vec<String> = Vec::new();
        self.collect_css_text(opts, &[CssProperty::BackgroundColor], &mut css_text_arr);
        self.collect_css_text(
            opts,
            &[CssProperty::Outline, CssProperty::OutlineColor, CssProperty::OutlineStyle, CssProperty::OutlineWidth],
            &mut css_text_arr,
        );
        self.collect_border_settings_css_text(opts, &mut css_text_arr);
        css_text_arr.join("")
    }

    /// Build the CSS for decorations styled via `inlineClassName`.
    fn css_text_for_inline_class_name(&self, opts: Option<&ThemeDecorationRenderOptions>) -> String {
        let Some(opts) = opts else {
            return String::new();
        };
        let mut css_text_arr: Vec<String> = Vec::new();
        self.collect_css_text(
            opts,
            &[
                CssProperty::FontStyle,
                CssProperty::FontWeight,
                CssProperty::FontFamily,
                CssProperty::FontSize,
                CssProperty::TextDecoration,
                CssProperty::Cursor,
                CssProperty::Color,
                CssProperty::Opacity,
                CssProperty::LetterSpacing,
            ],
            &mut css_text_arr,
        );
        if opts.letter_spacing.as_deref().is_some_and(|spacing| !spacing.is_empty()) {
            self.has_letter_spacing.set(true);
        }
        css_text_arr.join("")
    }

    /// Build the CSS for decorations styled before or after content.
    fn css_text_for_content_class_name(&self, opts: Option<&ContentDecorationRenderOptions>) -> String {
        let Some(opts) = opts else {
            return String::new();
        };
        let mut css_text_arr: Vec<String> = Vec::new();

        self.collect_border_settings_css_text(opts, &mut css_text_arr);
        if let Some(content_icon_path) = &opts.content_icon_path {
            let url = as_css_url(Some(content_icon_path), &*self.provider_args.uri_to_browser_uri);
            css_text_arr.push(CssProperty::ContentIconPath.format(&url));
        }
        if let Some(content_text) = &opts.content_text {
            // only take first line
            let truncated: String = content_text.chars().take_while(|ch| !matches!(ch, '\n' | '\r' | '\u{2028}' | '\u{2029}')).collect();
            let mut escaped = String::with_capacity(truncated.len());
            for ch in truncated.chars() {
                if ch == '\'' || ch == '\\' {
                    escaped.push('\\');
                }
                escaped.push(ch);
            }

            css_text_arr.push(CssProperty::ContentText.format(&escaped));
        }
        self.collect_css_text(
            opts,
            &[
                CssProperty::VerticalAlign,
                CssProperty::FontStyle,
                CssProperty::FontWeight,
                CssProperty::FontSize,
                CssProperty::FontFamily,
                CssProperty::TextDecoration,
                CssProperty::Color,
                CssProperty::Opacity,
                CssProperty::BackgroundColor,
                CssProperty::Margin,
                CssProperty::Padding,
            ],
            &mut css_text_arr,
        );
        if self.collect_css_text(opts, &[CssProperty::Width, CssProperty::Height], &mut css_text_arr) {
            css_text_arr.push("display:inline-block;".to_string());
        }

        css_text_arr.join("")
    }

    /// Build the CSS for decorations styled via `glyphMarginClassName`.
    fn css_text_for_glyph_margin_class_name(&self, opts: Option<&ThemeDecorationRenderOptions>) -> String {
        let Some(opts) = opts else {
            return String::new();
        };
        let mut css_text_arr: Vec<String> = Vec::new();

        if let Some(gutter_icon_path) = &opts.gutter_icon_path {
            let url = as_css_url(Some(gutter_icon_path), &*self.provider_args.uri_to_browser_uri);
            css_text_arr.push(CssProperty::GutterIconPath.format(&url));
            if let Some(gutter_icon_size) = &opts.gutter_icon_size {
                css_text_arr.push(CssProperty::GutterIconSize.format(gutter_icon_size));
            }
        }

        css_text_arr.join("")
    }

    fn collect_border_settings_css_text(&self, opts: &dyn CssPropertySource, css_text_arr: &mut Vec<String>) -> bool {
        if self.collect_css_text(
            opts,
            &[
                CssProperty::Border,
                CssProperty::BorderColor,
                CssProperty::BorderRadius,
                CssProperty::BorderSpacing,
                CssProperty::BorderStyle,
                CssProperty::BorderWidth,
            ],
            css_text_arr,
        ) {
            css_text_arr.push("box-sizing: border-box;".to_string());
            return true;
        }
        false
    }

    fn collect_css_text(&self, opts: &dyn CssPropertySource, properties: &[CssProperty], css_text_arr: &mut Vec<String>) -> bool {
        let len_before = css_text_arr.len();
        for &property in properties {
            if let Some(value) = self.resolve_value(opts.property(property)) {
                css_text_arr.push(property.format(&value));
            }
        }
        css_text_arr.len() != len_before
    }

    fn resolve_value(&self, value: Option<DecorationColor>) -> Option<String> {
        match value? {
            DecorationColor::Theme(id) => {
                self.uses_theme_colors.set(true);
                let color = self.theme.borrow().get_color(&id, None);
                Some(color.map_or_else(|| "transparent".to_string(), |color| color.to_string()))
            }
            DecorationColor::Color(value) => Some(value),
        }
    }
}

/// `DecorationSubTypeOptionsProvider`.
struct DecorationSubTypeOptionsProvider {
    style_sheet: Rc<GlobalStyleSheet>,
    parent_type_key: String,
    before_content_rules: RefCell<Option<DecorationCssRules>>,
    after_content_rules: RefCell<Option<DecorationCssRules>>,
}

impl DecorationSubTypeOptionsProvider {
    fn new(theme_service: &Rc<dyn IThemeService>, style_sheet: Rc<GlobalStyleSheet>, provider_args: Rc<ProviderArguments>) -> Self {
        let before_content_rules =
            DecorationCssRules::new(ModelDecorationCssRuleType::BeforeContentClassName, provider_args.clone(), theme_service);
        let after_content_rules = DecorationCssRules::new(ModelDecorationCssRuleType::AfterContentClassName, provider_args.clone(), theme_service);
        Self {
            style_sheet,
            parent_type_key: provider_args.parent_type_key.clone().unwrap_or_default(),
            before_content_rules: RefCell::new(Some(before_content_rules)),
            after_content_rules: RefCell::new(Some(after_content_rules)),
        }
    }

    fn dispose(&self) {
        if let Some(rules) = self.before_content_rules.borrow_mut().take() {
            rules.dispose();
        }
        if let Some(rules) = self.after_content_rules.borrow_mut().take() {
            rules.dispose();
        }
    }
}

/// `DecorationTypeOptionsProvider`.
struct DecorationTypeOptionsProvider {
    disposables: Vec<DecorationCssRules>,
    style_sheet: Rc<GlobalStyleSheet>,

    description: String,
    class_name: Option<String>,
    inline_class_name: Option<String>,
    inline_class_name_affects_letter_spacing: Option<bool>,
    before_content_class_name: Option<String>,
    after_content_class_name: Option<String>,
    glyph_margin_class_name: Option<String>,
    is_whole_line: bool,
    line_height: Option<f64>,
    font_size: Option<String>,
    font_family: Option<String>,
    font_weight: Option<String>,
    font_style: Option<String>,
    overview_ruler: Option<ModelDecorationOverviewRulerInit>,
    stickiness: Option<TrackedRangeStickiness>,
    before_injected_text: Option<InjectedTextOptions>,
    after_injected_text: Option<InjectedTextOptions>,
}

/// `!!color`: o texto vazio é falso, uma cor de tema é verdadeira.
fn is_truthy_color(color: &DecorationColor) -> bool {
    !matches!(color, DecorationColor::Color(value) if value.is_empty())
}

/// `a || b` entre cores opcionais.
fn color_or(a: Option<DecorationColor>, b: Option<DecorationColor>) -> Option<DecorationColor> {
    if a.as_ref().is_some_and(is_truthy_color) { a } else { b }
}

impl DecorationTypeOptionsProvider {
    fn new(description: &str, theme_service: &Rc<dyn IThemeService>, style_sheet: Rc<GlobalStyleSheet>, provider_args: Rc<ProviderArguments>) -> Self {
        let disposables: RefCell<Vec<DecorationCssRules>> = RefCell::new(Vec::new());

        let create_css_rules = |rule_type: ModelDecorationCssRuleType| -> Option<String> {
            let rules = DecorationCssRules::new(rule_type, provider_args.clone(), theme_service);
            let class_name = rules.has_content().then(|| rules.class_name().to_string());
            disposables.borrow_mut().push(rules);
            class_name
        };
        let create_inline_css_rules = |rule_type: ModelDecorationCssRuleType| -> Option<(String, bool)> {
            let rules = DecorationCssRules::new(rule_type, provider_args.clone(), theme_service);
            let data = rules.has_content().then(|| (rules.class_name().to_string(), rules.has_letter_spacing()));
            disposables.borrow_mut().push(rules);
            data
        };

        let class_name = create_css_rules(ModelDecorationCssRuleType::ClassName);
        let inline_data = create_inline_css_rules(ModelDecorationCssRuleType::InlineClassName);
        let (inline_class_name, inline_class_name_affects_letter_spacing) = match inline_data {
            Some((class_name, has_letter_spacing)) => (Some(class_name), Some(has_letter_spacing)),
            None => (None, None),
        };
        let before_content_class_name = create_css_rules(ModelDecorationCssRuleType::BeforeContentClassName);
        let after_content_class_name = create_css_rules(ModelDecorationCssRuleType::AfterContentClassName);

        let options = &provider_args.options;
        let before_injected_text = if injected_content_text(&options.base.before_injected_text).is_some() {
            let before_inline_data = create_inline_css_rules(ModelDecorationCssRuleType::BeforeInjectedTextClassName);
            injected_text_options(&options.base.before_injected_text, before_inline_data)
        } else {
            None
        };

        let after_injected_text = if injected_content_text(&options.base.after_injected_text).is_some() {
            let after_inline_data = create_inline_css_rules(ModelDecorationCssRuleType::AfterInjectedTextClassName);
            injected_text_options(&options.base.after_injected_text, after_inline_data)
        } else {
            None
        };

        let glyph_margin_class_name = create_css_rules(ModelDecorationCssRuleType::GlyphMarginClassName);

        let light_overview_ruler_color =
            color_or(options.light.as_ref().and_then(|theme| theme.overview_ruler_color.clone()), options.base.overview_ruler_color.clone());
        let dark_overview_ruler_color =
            color_or(options.dark.as_ref().and_then(|theme| theme.overview_ruler_color.clone()), options.base.overview_ruler_color.clone());
        let overview_ruler = if light_overview_ruler_color.is_some() || dark_overview_ruler_color.is_some() {
            Some(ModelDecorationOverviewRulerInit {
                color: color_or(light_overview_ruler_color.clone(), dark_overview_ruler_color.clone()),
                dark_color: color_or(dark_overview_ruler_color, light_overview_ruler_color),
                position: Some(options.overview_ruler_lane.unwrap_or(OverviewRulerLane::Center)),
            })
        } else {
            None
        };

        Self {
            disposables: disposables.into_inner(),
            style_sheet,
            description: description.to_string(),
            class_name,
            inline_class_name,
            inline_class_name_affects_letter_spacing,
            before_content_class_name,
            after_content_class_name,
            glyph_margin_class_name,
            is_whole_line: options.is_whole_line.unwrap_or(false),
            line_height: options.base.line_height,
            font_size: options.base.font_size.clone(),
            font_family: options.base.font_family.clone(),
            font_weight: options.base.font_weight.clone(),
            font_style: options.base.font_style.clone(),
            overview_ruler,
            stickiness: options.range_behavior,
            before_injected_text,
            after_injected_text,
        }
    }

    fn get_options(&self, writable: bool) -> ModelDecorationOptionsInit {
        ModelDecorationOptionsInit {
            description: self.description.clone(),
            inline_class_name: self.inline_class_name.clone(),
            // só o provedor em si (`writable` falso) leva este campo
            inline_class_name_affects_letter_spacing: !writable && self.inline_class_name_affects_letter_spacing.unwrap_or(false),
            before_content_class_name: self.before_content_class_name.clone(),
            after_content_class_name: self.after_content_class_name.clone(),
            class_name: self.class_name.clone(),
            glyph_margin_class_name: self.glyph_margin_class_name.clone(),
            is_whole_line: self.is_whole_line,
            line_height: self.line_height,
            font_family: self.font_family.clone(),
            font_size: self.font_size.clone(),
            font_weight: self.font_weight.clone(),
            font_style: self.font_style.clone(),
            overview_ruler: self.overview_ruler.clone(),
            stickiness: self.stickiness,
            before: self.before_injected_text.clone(),
            after: self.after_injected_text.clone(),
            ..ModelDecorationOptionsInit::default()
        }
    }

    fn dispose(&self) {
        for rules in &self.disposables {
            rules.dispose();
        }
    }
}

/// `options.beforeInjectedText && options.beforeInjectedText.contentText`: o texto, quando existe e não é vazio.
fn injected_content_text(text: &Option<InjectedTextRenderOptions>) -> Option<&str> {
    text.as_ref()?.content.content_text.as_deref().filter(|content| !content.is_empty())
}

/// O `{ content, inlineClassName, inlineClassNameAffectsLetterSpacing }` de `beforeInjectedText` e
/// `afterInjectedText`, a partir do texto (`contentText`) e dos dados da regra de classe inline.
fn injected_text_options(text: &Option<InjectedTextRenderOptions>, inline_data: Option<(String, bool)>) -> Option<InjectedTextOptions> {
    let content = injected_content_text(text)?;
    let affects_letter_spacing = text.as_ref().and_then(|text| text.affects_letter_spacing).unwrap_or(false);
    Some(InjectedTextOptions {
        content: content.to_string(),
        inline_class_name_affects_letter_spacing: inline_data.as_ref().is_some_and(|(_, has_letter_spacing)| *has_letter_spacing)
            || affects_letter_spacing,
        inline_class_name: inline_data.map(|(class_name, _)| class_name),
        ..InjectedTextOptions::default()
    })
}

/// `IModelDecorationOptionsProvider`.
enum OptionsProvider {
    Type(DecorationTypeOptionsProvider),
    SubType(DecorationSubTypeOptionsProvider),
}

struct ProviderEntry {
    ref_count: Cell<i32>,
    provider: OptionsProvider,
}

impl OptionsProvider {
    fn resolve_decoration_css_rules(&self) -> Vec<CssRule> {
        match self {
            OptionsProvider::Type(provider) => provider.style_sheet.css_rules(),
            OptionsProvider::SubType(provider) => provider.style_sheet.css_rules(),
        }
    }

    fn dispose(&self) {
        match self {
            OptionsProvider::Type(provider) => provider.dispose(),
            OptionsProvider::SubType(provider) => provider.dispose(),
        }
    }
}

// ---- the service

/// O `IDisposable` que `registerDecorationType` devolve: remove o tipo de decoração.
pub struct DecorationTypeRegistration {
    service: Weak<DecorationRenderService>,
    key: String,
}

impl IDisposable for DecorationTypeRegistration {
    fn dispose(&self) {
        if let Some(service) = self.service.upgrade() {
            service.remove_decoration_type(&self.key);
        }
    }
}

/// A parte de decorações do `AbstractCodeEditorService` (ver as diferenças no topo do módulo).
pub struct DecorationRenderService {
    me: Weak<DecorationRenderService>,
    theme_service: Rc<dyn IThemeService>,
    uri_to_browser_uri: Rc<dyn Fn(&URI) -> URI>,
    global_style_sheet: Rc<GlobalStyleSheet>,
    decoration_option_providers: RefCell<OrderedMap<String, Rc<ProviderEntry>>>,
    on_decoration_type_registered: Rc<Emitter<String>>,
}

impl DecorationRenderService {
    pub fn new(theme_service: Rc<dyn IThemeService>, uri_to_browser_uri: Rc<dyn Fn(&URI) -> URI>) -> Rc<Self> {
        Rc::new_cyclic(|me| Self {
            me: me.clone(),
            theme_service,
            uri_to_browser_uri,
            global_style_sheet: Rc::new(GlobalStyleSheet::new()),
            decoration_option_providers: RefCell::new(OrderedMap::new()),
            on_decoration_type_registered: Emitter::new(),
        })
    }

    /// `globalStyleSheet`.
    pub fn global_style_sheet(&self) -> &Rc<GlobalStyleSheet> {
        &self.global_style_sheet
    }

    /// `onDecorationTypeRegistered`.
    pub fn on_decoration_type_registered(&self) -> Event<String> {
        self.on_decoration_type_registered.event()
    }

    /// `registerDecorationType(description, key, options, parentTypeKey?)`.
    pub fn register_decoration_type(
        &self,
        description: &str,
        key: &str,
        options: DecorationRenderOptions,
        parent_type_key: Option<&str>,
    ) -> Rc<DecorationTypeRegistration> {
        let existing = self.decoration_option_providers.borrow().get(&key.to_string()).cloned();
        let entry = match existing {
            Some(entry) => entry,
            None => {
                let style_sheet = self.global_style_sheet.clone();
                let provider_args = Rc::new(ProviderArguments {
                    style_sheet: style_sheet.clone(),
                    key: key.to_string(),
                    parent_type_key: parent_type_key.map(str::to_string),
                    options,
                    uri_to_browser_uri: self.uri_to_browser_uri.clone(),
                });
                let provider = if parent_type_key.is_none_or(str::is_empty) {
                    OptionsProvider::Type(DecorationTypeOptionsProvider::new(description, &self.theme_service, style_sheet, provider_args))
                } else {
                    OptionsProvider::SubType(DecorationSubTypeOptionsProvider::new(&self.theme_service, style_sheet, provider_args))
                };
                let entry = Rc::new(ProviderEntry { ref_count: Cell::new(0), provider });
                self.decoration_option_providers.borrow_mut().insert(key.to_string(), entry.clone());
                self.on_decoration_type_registered.fire(key.to_string());
                entry
            }
        };
        entry.ref_count.set(entry.ref_count.get() + 1);
        Rc::new(DecorationTypeRegistration { service: self.me.clone(), key: key.to_string() })
    }

    /// `listDecorationTypes()`.
    pub fn list_decoration_types(&self) -> Vec<String> {
        self.decoration_option_providers.borrow().keys().cloned().collect()
    }

    /// `removeDecorationType(key)`.
    pub fn remove_decoration_type(&self, key: &str) {
        let entry = self.decoration_option_providers.borrow().get(&key.to_string()).cloned();
        if let Some(entry) = entry {
            entry.ref_count.set(entry.ref_count.get() - 1);
            if entry.ref_count.get() <= 0 {
                self.decoration_option_providers.borrow_mut().remove(&key.to_string());
                entry.provider.dispose();
            }
        }
    }

    /// `resolveDecorationOptions(decorationTypeKey, writable)`: lança quando o tipo não existe.
    pub fn resolve_decoration_options(&self, decoration_type_key: &str, writable: bool) -> ModelDecorationOptionsInit {
        let entry = self.decoration_option_providers.borrow().get(&decoration_type_key.to_string()).cloned();
        let Some(entry) = entry else {
            throw(Error::new(format!("Unknown decoration type key: {decoration_type_key}")));
        };
        match &entry.provider {
            OptionsProvider::Type(provider) => provider.get_options(writable),
            OptionsProvider::SubType(provider) => {
                let mut options = self.resolve_decoration_options(&provider.parent_type_key, true);
                if let Some(rules) = provider.before_content_rules.borrow().as_ref() {
                    options.before_content_class_name = Some(rules.class_name().to_string());
                }
                if let Some(rules) = provider.after_content_rules.borrow().as_ref() {
                    options.after_content_class_name = Some(rules.class_name().to_string());
                }
                options
            }
        }
    }

    /// `resolveDecorationCSSRules(decorationTypeKey)`: `None` quando o tipo não existe.
    pub fn resolve_decoration_css_rules(&self, decoration_type_key: &str) -> Option<Vec<CssRule>> {
        let entry = self.decoration_option_providers.borrow().get(&decoration_type_key.to_string()).cloned();
        entry.map(|entry| entry.provider.resolve_decoration_css_rules())
    }
}

impl IDisposable for DecorationRenderService {
    fn dispose(&self) {
        self.on_decoration_type_registered.dispose();
    }
}
