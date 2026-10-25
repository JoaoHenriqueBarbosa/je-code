//! Porte de `src/vs/editor/contrib/gotoSymbol/browser/peek/referencesTree.ts`: o que a árvore do peek de referências
//! precisa (a fonte de dados, o delegate, os provedores de identidade, de rótulo de navegação e de acessibilidade e os
//! renderers de arquivo e de referência).
//!
//! Representação:
//! - `TreeElement = FileReferences | OneReference` é o enum [`TreeElement`], com igualdade e hash pelo endereço do `Rc`
//!   (o `Map` do original compara por identidade de objeto). A entrada da árvore, `ReferencesModel | FileReferences`, é o
//!   enum [`ReferencesTreeInput`]; os `instanceof` de `DataSource.hasChildren` e `getChildren` são o `match` sobre os dois
//!   (a entrada e o elemento têm os mesmos casos, que o `InputOrElement` da árvore separa).
//! - **Os renderers** são [`IAsyncDataTreeRenderer`] sobre `FuzzyScore` (`Vec<i32>`, o `FuzzyScore` de `je_base::filters`).
//!   `@IInstantiationService`, `@ILabelService` e o `defaultCountBadgeStyles` chegam em [`ReferencesTreeRendererServices`].
//!   O DOM (`div.reference-file`, `.count`) é a árvore de `je-ui`, com as mesmas classes. O `display: inline-flex` de
//!   `.reference-file` e a margem `auto` à esquerda e de 12px à direita de `.count` (`referencesWidget.css`) são
//!   [`FileReferencesTemplate::layout`]: o rótulo ocupa o que sobra da largura da linha e o contador fica à direita.
//! - **Cores.** As regras de `referencesWidget.css` que pintam a árvore (`.ref-tree`, `.reference-file`, o realce de
//!   `.referenceMatch`, a seleção de uma lista com foco e o negrito do contraste alto) viram [`ReferencesTreeStyles`],
//!   calculado de um tema por [`references_tree_styles`] e relido a cada renderização: o `IColorTheme` do editor é um
//!   trait e os `default*Styles` do `je-widgets` pedem o `ColorTheme` concreto, então o mapeamento de cores da lista
//!   (`defaultListStyles`) é refeito aqui com o fundo e a seleção do peek por cima. O rótulo de uma linha selecionada
//!   mantém a cor do arquivo (o `color: inherit` de `.monaco-list:focus .selected .reference-file` herda a cor da
//!   linha, que o rótulo não lê) e o realce de `.referenceMatch .highlight` pinta só o texto (o `HighlightedLabel` não
//!   tem fundo de realce).
//! - `StringRepresentationProvider.mightProducePrintableCharacter` e o `@IKeybindingService` dele não existem: o
//!   `IKeyboardNavigationLabelProvider` do `je-widgets` só tem `getKeyboardNavigationLabel`.
//! - `AccessibilityProvider` é o `IListAccessibilityProvider` do `je-widgets`; as opções da árvore de dados assíncrona
//!   ainda não têm o campo `accessibilityProvider` (`// TODO(integração)`), então o provedor fica pronto para quando elas
//!   o tiverem.
//! - `DataSource.getChildren` de um `FileReferences` é o `resolve` do modelo (uma [`Promise`]); `throw new Error('bad
//!   tree')` de uma referência é o `throw` de `je_base::errors`.

use std::any::Any;
use std::hash::{Hash, Hasher};
use std::rc::Rc;

use je_base::errors::{Error, throw};
use je_base::filters::{FuzzyScore, Match, create_matches, is_default_fuzzy_score};
use je_base::lifecycle::IDisposable;
use je_base::nls::localize;
use je_base::promise::MaybePromise;
use je_base::uri::URI;
use je_render::Rgba;
use je_render::kurbo::Rect;
use je_theme::ThemeType;
use je_theme::to_rgba8;
use je_ui::{NodeId, Role, Ui};
use je_widgets::count_badge::{CountBadge, ICountBadgeOptions, ICountBadgeStyles};
use je_widgets::dom::create_el;
use je_widgets::highlighted_label::{HighlightedLabel, HighlightedLabelStyles, IHighlight};
use je_widgets::icon_label::{IIconLabelCreationOptions, IIconLabelValueOptions, IconLabel, IconLabelStyles, IconLabelText, IconLabelTitle};
use je_widgets::list::list_styles::{IListStyles, LIST_FALLBACK_FOREGROUND};
use je_widgets::list::list_types::{IIdentityProvider, IKeyboardNavigationLabelProvider, IListAccessibilityProvider, IListVirtualDelegate, TemplateData};
use je_widgets::text::UI_FONT_SIZE;
use je_widgets::tree::async_data_tree_node::{AsyncDataTreeNodeWrapper, IAsyncDataTreeRenderer};
use je_widgets::tree::tree_types::{IAsyncDataSource, InputOrElement, TreeElementRenderDetails, TreeNodeApi};

use je_platform::label::label::{ILabelService, UriLabelOptions};

use crate::editor_extensions::ITextModelService;
use crate::goto_symbol::references_model::{FileReferences, OneReference, ReferencesModel, ext_uri};
use crate::peek_view::{
    PEEK_VIEW_RESULTS_BACKGROUND, PEEK_VIEW_RESULTS_FILE_FOREGROUND, PEEK_VIEW_RESULTS_MATCH_FOREGROUND, PEEK_VIEW_RESULTS_MATCH_HIGHLIGHT,
    PEEK_VIEW_RESULTS_SELECTION_BACKGROUND, PEEK_VIEW_RESULTS_SELECTION_FOREGROUND,
};
use crate::services::theme_service::{IColorTheme, IThemeService};

// #region data source

/// `TreeElement = FileReferences | OneReference`. A igualdade e o hash são pelo endereço do objeto.
#[derive(Clone)]
pub enum TreeElement {
    File(Rc<FileReferences>),
    Reference(Rc<OneReference>),
}

impl TreeElement {
    /// `element.uri`.
    pub fn uri(&self) -> &URI {
        match self {
            TreeElement::File(file) => &file.uri,
            TreeElement::Reference(reference) => reference.uri(),
        }
    }

    /// `element.ariaMessage`.
    pub fn aria_message(&self) -> String {
        match self {
            TreeElement::File(file) => file.aria_message(),
            TreeElement::Reference(reference) => reference.aria_message(),
        }
    }

    /// O elemento como o `IListService` o entrega aos comandos (`Rc<FileReferences>` ou `Rc<OneReference>`, que eles
    /// reconhecem por `downcast`).
    pub fn as_any(&self) -> Rc<dyn Any> {
        match self {
            TreeElement::File(file) => Rc::clone(file) as Rc<dyn Any>,
            TreeElement::Reference(reference) => Rc::clone(reference) as Rc<dyn Any>,
        }
    }
}

impl PartialEq for TreeElement {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (TreeElement::File(a), TreeElement::File(b)) => Rc::ptr_eq(a, b),
            (TreeElement::Reference(a), TreeElement::Reference(b)) => Rc::ptr_eq(a, b),
            _ => false,
        }
    }
}

impl Eq for TreeElement {}

impl Hash for TreeElement {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self {
            TreeElement::File(file) => {
                0u8.hash(state);
                (Rc::as_ptr(file) as usize).hash(state);
            }
            TreeElement::Reference(reference) => {
                1u8.hash(state);
                (Rc::as_ptr(reference) as usize).hash(state);
            }
        }
    }
}

/// `ReferencesModel | FileReferences`: a entrada da árvore.
#[derive(Clone)]
pub enum ReferencesTreeInput {
    Model(Rc<ReferencesModel>),
    File(Rc<FileReferences>),
}

impl ReferencesTreeInput {
    /// A entrada como o `IListService` a entrega (`getInput()`).
    pub fn as_any(&self) -> Rc<dyn Any> {
        match self {
            ReferencesTreeInput::Model(model) => Rc::clone(model) as Rc<dyn Any>,
            ReferencesTreeInput::File(file) => Rc::clone(file) as Rc<dyn Any>,
        }
    }

    /// `input === group`: a entrada é este grupo de arquivo.
    pub fn is_file(&self, group: &Rc<FileReferences>) -> bool {
        matches!(self, ReferencesTreeInput::File(file) if Rc::ptr_eq(file, group))
    }
}

/// `DataSource`.
pub struct DataSource {
    resolver_service: Rc<dyn ITextModelService>,
}

impl DataSource {
    pub fn new(resolver_service: Rc<dyn ITextModelService>) -> Self {
        Self { resolver_service }
    }
}

impl IAsyncDataSource<ReferencesTreeInput, TreeElement> for DataSource {
    fn has_children(&self, element: &InputOrElement<ReferencesTreeInput, TreeElement>) -> bool {
        match element {
            // `element instanceof ReferencesModel` e `element instanceof FileReferences`
            InputOrElement::Input(_) | InputOrElement::Element(TreeElement::File(_)) => true,
            InputOrElement::Element(TreeElement::Reference(_)) => false,
        }
    }

    fn get_children(&self, element: &InputOrElement<ReferencesTreeInput, TreeElement>) -> MaybePromise<Vec<TreeElement>> {
        match element {
            InputOrElement::Input(ReferencesTreeInput::Model(model)) => {
                MaybePromise::Value(model.groups().into_iter().map(TreeElement::File).collect())
            }
            InputOrElement::Input(ReferencesTreeInput::File(file)) | InputOrElement::Element(TreeElement::File(file)) => {
                // if (element.failure) {
                // 	// refresh the element on failure so that
                // 	// we can update its rendering
                // 	return tree.refresh(element).then(() => val.children);
                // }
                MaybePromise::Promise(
                    file.resolve(&self.resolver_service).then(|resolved| resolved.children().into_iter().map(TreeElement::Reference).collect::<Vec<_>>()),
                )
            }
            InputOrElement::Element(TreeElement::Reference(_)) => throw(Error::new("bad tree")),
        }
    }
}

// #endregion

/// `Delegate`.
pub struct Delegate;

impl IListVirtualDelegate<TreeElement> for Delegate {
    fn get_height(&self, _element: &TreeElement) -> f64 {
        ROW_HEIGHT
    }

    fn get_template_id(&self, element: &TreeElement) -> String {
        match element {
            TreeElement::File(_) => FileReferencesRenderer::ID.to_owned(),
            TreeElement::Reference(_) => OneReferenceRenderer::ID.to_owned(),
        }
    }
}

/// `getHeight()` do `Delegate` (e o `line-height: 23px` de `.ref-tree`).
const ROW_HEIGHT: f64 = 23.0;

/// `StringRepresentationProvider`.
pub struct StringRepresentationProvider;

impl IKeyboardNavigationLabelProvider<TreeElement> for StringRepresentationProvider {
    fn get_keyboard_navigation_label(&self, element: &TreeElement) -> Vec<Option<String>> {
        if let TreeElement::Reference(reference) = element {
            let parts = reference.parent().get_preview(reference).and_then(|preview| preview.preview(&reference.range(), None));
            if let Some(parts) = parts {
                return vec![Some(parts.value.to_string_lossy())];
            }
        }
        // FileReferences or unresolved OneReference
        vec![Some(ext_uri().basename(element.uri(), None))]
    }
}

/// `IdentityProvider`.
pub struct IdentityProvider;

impl IIdentityProvider<TreeElement> for IdentityProvider {
    fn get_id(&self, element: &TreeElement) -> String {
        match element {
            TreeElement::Reference(reference) => reference.id.clone(),
            TreeElement::File(file) => file.uri.to_string(false).to_string_lossy(),
        }
    }
}

/// `AccessibilityProvider`.
pub struct AccessibilityProvider;

impl IListAccessibilityProvider<TreeElement> for AccessibilityProvider {
    fn get_aria_label(&self, element: &TreeElement) -> Option<String> {
        Some(element.aria_message())
    }

    fn get_widget_aria_label(&self) -> String {
        localize("treeAriaLabel", "References", &[])
    }
}

// #region styles

/// As regras de `referencesWidget.css` que pintam a árvore (ver "Cores" no topo do módulo).
#[derive(Clone, Debug)]
pub struct ReferencesTreeStyles {
    /// `overrideStyles: { listBackground: peekViewResultsBackground }` mais a seleção de uma lista com foco.
    pub list: IListStyles,
    /// `.reference-file`: o rótulo do arquivo.
    pub file: IconLabelStyles,
    /// `defaultCountBadgeStyles`.
    pub badge: ICountBadgeStyles,
    /// `.ref-tree`: o texto de uma referência, com o realce de busca da lista.
    pub reference: HighlightedLabelStyles,
    /// `.referenceMatch`: o texto de uma referência com o trecho da ocorrência realçado.
    pub reference_match: HighlightedLabelStyles,
}

/// `theme.getColor(id)` como a cor de 8 bits da pintura.
fn theme_rgba(theme: &dyn IColorTheme, id: &str) -> Option<Rgba> {
    theme.get_color(id, None).map(|color| to_rgba8(&color))
}

/// As cores da árvore no tema (ver "Cores" no topo do módulo).
pub fn references_tree_styles(theme: &dyn IColorTheme) -> ReferencesTreeStyles {
    let color = |id: &str| theme_rgba(theme, id);
    let high_contrast = matches!(theme.theme_type(), ThemeType::HighContrastDark | ThemeType::HighContrastLight);
    let active_contrast_border = color("contrastActiveBorder");
    let selection_background = color(PEEK_VIEW_RESULTS_SELECTION_BACKGROUND);
    let selection_foreground = color(PEEK_VIEW_RESULTS_SELECTION_FOREGROUND);
    let fallback_foreground = LIST_FALLBACK_FOREGROUND;
    let line_foreground = color(PEEK_VIEW_RESULTS_MATCH_FOREGROUND);
    let file_foreground = color(PEEK_VIEW_RESULTS_FILE_FOREGROUND);

    // `defaultListStyles` (`defaultStyles.ts`), com o fundo do peek e a seleção de `.monaco-list:focus`.
    let list = IListStyles {
        list_background: color(PEEK_VIEW_RESULTS_BACKGROUND),
        list_inactive_focus_foreground: None,
        list_focus_background: color("list.focusBackground"),
        list_focus_foreground: color("list.focusForeground"),
        list_focus_outline: color("list.focusOutline"),
        list_active_selection_background: selection_background,
        list_active_selection_foreground: selection_foreground,
        list_active_selection_icon_foreground: color("list.activeSelectionIconForeground"),
        list_focus_and_selection_outline: color("list.focusAndSelectionOutline"),
        list_focus_and_selection_background: selection_background,
        list_focus_and_selection_foreground: selection_foreground,
        list_inactive_selection_background: color("list.inactiveSelectionBackground"),
        list_inactive_selection_icon_foreground: color("list.inactiveSelectionIconForeground"),
        list_inactive_selection_foreground: color("list.inactiveSelectionForeground"),
        list_inactive_focus_background: color("list.inactiveFocusBackground"),
        list_inactive_focus_outline: color("list.inactiveFocusOutline"),
        list_hover_background: color("list.hoverBackground"),
        list_hover_foreground: color("list.hoverForeground"),
        list_drop_over_background: color("list.dropBackground"),
        list_drop_between_background: color("list.dropBetweenBackground"),
        list_selection_outline: active_contrast_border,
        list_hover_outline: active_contrast_border,
        tree_indent_guides_stroke: color("tree.indentGuidesStroke"),
        tree_inactive_indent_guides_stroke: color("tree.inactiveIndentGuidesStroke"),
        tree_sticky_scroll_background: None,
        tree_sticky_scroll_border: None,
        tree_sticky_scroll_shadow: color("scrollbar.shadow"),
        table_columns_border: color("tree.tableColumnsBorder"),
        table_odd_rows_background_color: color("tree.tableOddRowsBackground"),
        foreground: line_foreground,
        row_radius: 0.0,
    };

    let highlight_foreground = color("list.highlightForeground");
    let reference = HighlightedLabelStyles {
        foreground: line_foreground.unwrap_or(fallback_foreground),
        highlight_foreground,
        highlight_weight: 400,
        font_size: UI_FONT_SIZE,
        font_weight: 400,
        line_height: Some(ROW_HEIGHT),
        italic: false,
        strikethrough: false,
    };
    // `.referenceMatch .highlight { color: peekViewResult.fileForeground }`
    let reference_match = HighlightedLabelStyles { highlight_foreground: file_foreground, ..reference };
    // `.reference-file { color: peekViewResult.fileForeground }` e, no contraste alto, `font-weight: bold`
    let file = IconLabelStyles {
        text: HighlightedLabelStyles {
            foreground: file_foreground.unwrap_or(fallback_foreground),
            font_weight: if high_contrast { 700 } else { 400 },
            ..reference
        },
        disabled_foreground: color("disabledForeground"),
    };
    let badge = ICountBadgeStyles { badge_background: color("badge.background"), badge_foreground: color("badge.foreground"), badge_border: color("contrastBorder") };

    ReferencesTreeStyles { list, file, badge, reference, reference_match }
}

// #endregion

/// Os serviços que os renderers recebem por injeção.
#[derive(Clone)]
pub struct ReferencesTreeRendererServices {
    pub ui: Ui,
    pub label_service: Rc<dyn ILabelService>,
    pub theme_service: Rc<dyn IThemeService>,
}

impl ReferencesTreeRendererServices {
    /// As cores da árvore no tema de agora.
    fn styles(&self) -> ReferencesTreeStyles {
        references_tree_styles(&*self.theme_service.get_color_theme())
    }
}

// #region render: File

/// A margem direita de `.reference-file .count`.
const COUNT_MARGIN_RIGHT: f64 = 12.0;

/// `FileReferencesTemplate`.
pub struct FileReferencesTemplate {
    services: ReferencesTreeRendererServices,
    container: NodeId,
    count: NodeId,
    file: Rc<IconLabel>,
    badge: Rc<CountBadge>,
}

impl FileReferencesTemplate {
    fn new(services: &ReferencesTreeRendererServices, container: NodeId) -> Self {
        let styles = services.styles();
        let parent = create_el(&mut services.ui.tree_mut(), container, Role::Generic, "reference-file");
        let file = IconLabel::new(&services.ui, parent, Some(IIconLabelCreationOptions { support_highlights: true, ..IIconLabelCreationOptions::default() }), styles.file);
        let count = create_el(&mut services.ui.tree_mut(), parent, Role::Generic, "count");
        let badge = CountBadge::new(&services.ui, count, &ICountBadgeOptions::default(), styles.badge);
        Self { services: services.clone(), container: parent, count, file, badge }
    }

    /// `set(element, matches)`.
    fn set(&self, element: &FileReferences, matches: Vec<Match>) {
        let label_service = &self.services.label_service;
        let parent = ext_uri().dirname(&element.uri);
        let parent_label = label_service.get_uri_label(&parent, &UriLabelOptions { relative: Some(true), ..UriLabelOptions::default() });
        self.file.set_label(
            IconLabelText::Single(label_service.get_uri_basename_label(&element.uri)),
            Some(parent_label.as_str()),
            Some(&IIconLabelValueOptions {
                title: Some(IconLabelTitle::Text(label_service.get_uri_label(&element.uri, &UriLabelOptions::default()))),
                matches: Some(matches),
                ..IIconLabelValueOptions::default()
            }),
        );
        let len = element.children().len();
        self.badge.set_count(len as i64);
        let len_text = len.to_string();
        if len > 1 {
            self.badge.set_title_format(&localize("referencesCount", "{0} references", &[len_text.as_str()]));
        } else {
            self.badge.set_title_format(&localize("referenceCount", "{0} reference", &[len_text.as_str()]));
        }
        self.layout();
    }

    /// `.reference-file { display: inline-flex; width: 100%; height: 100% }` e `.count { margin-left: auto;
    /// margin-right: 12px }`: o rótulo ocupa o que sobra da largura da linha e o contador fica à direita.
    pub fn layout(&self) {
        let ui = &self.services.ui;
        let row_width = ui.tree().bounds(self.container).width();
        let badge = ui.tree().bounds(self.badge.element());
        let (badge_width, badge_height) = (badge.width(), badge.height());
        if row_width <= 0.0 {
            // a linha ainda não tem largura: o rótulo cresce até o texto
            self.file.layout(None);
            return;
        }
        let count_left = (row_width - COUNT_MARGIN_RIGHT - badge_width).max(0.0);
        let count_top = ((ROW_HEIGHT - badge_height) / 2.0).max(0.0);
        {
            let mut tree = ui.tree_mut();
            tree.set_bounds(self.count, Rect::new(count_left, count_top, count_left + badge_width, count_top + badge_height));
            tree.set_bounds(self.badge.element(), Rect::new(0.0, 0.0, badge_width, badge_height));
        }
        self.file.layout(Some(count_left));
    }
}

impl IDisposable for FileReferencesTemplate {
    fn dispose(&self) {
        self.file.dispose();
        self.badge.dispose();
    }
}

/// `FileReferencesRenderer`.
pub struct FileReferencesRenderer {
    services: ReferencesTreeRendererServices,
}

impl FileReferencesRenderer {
    pub const ID: &'static str = "FileReferencesRenderer";

    pub fn new(services: ReferencesTreeRendererServices) -> Self {
        Self { services }
    }
}

impl IAsyncDataTreeRenderer<ReferencesTreeInput, TreeElement, FuzzyScore> for FileReferencesRenderer {
    fn template_id(&self) -> String {
        Self::ID.to_owned()
    }

    fn render_template(&self, container: NodeId) -> TemplateData {
        Rc::new(FileReferencesTemplate::new(&self.services, container))
    }

    fn render_element(
        &self,
        node: &Rc<AsyncDataTreeNodeWrapper<ReferencesTreeInput, TreeElement, FuzzyScore>>,
        _index: usize,
        template_data: &TemplateData,
        _details: Option<&TreeElementRenderDetails>,
    ) {
        let (Some(template), InputOrElement::Element(TreeElement::File(file))) = (template_data.downcast_ref::<FileReferencesTemplate>(), node.element()) else {
            return;
        };
        template.set(&file, create_matches(node.filter_data().as_deref()));
    }

    fn dispose_template(&self, template_data: &TemplateData) {
        if let Some(template) = template_data.downcast_ref::<FileReferencesTemplate>() {
            template.dispose();
        }
    }
}

// #endregion

// #region render: Reference

/// `OneReferenceTemplate`.
pub struct OneReferenceTemplate {
    ui: Ui,
    label: Rc<HighlightedLabel>,
}

impl OneReferenceTemplate {
    fn new(services: &ReferencesTreeRendererServices, container: NodeId) -> Self {
        let label = HighlightedLabel::new(&services.ui, container, None, services.styles().reference);
        Self { ui: services.ui.clone(), label }
    }

    /// `set(element, score?)`.
    fn set(&self, element: &OneReference, score: Option<&[i32]>, styles: &ReferencesTreeStyles) {
        let range = element.range();
        let preview = element.parent().get_preview(element).and_then(|preview| preview.preview(&range, None)).filter(|preview| !preview.value.is_empty());
        let Some(preview) = preview else {
            // this means we FAILED to resolve the document or the value is the empty string
            self.label.set_styles(styles.reference);
            self.label.set(
                Some(&format!("{}:{}:{}", ext_uri().basename(element.uri(), None), range.start_line_number + 1, range.start_column + 1)),
                &[],
                "",
                false,
                false,
            );
            return;
        };

        // render search match as highlight unless
        // we have score, then render the score
        let value = preview.value.to_string_lossy();
        if score.is_some() && !is_default_fuzzy_score(score) {
            self.ui.tree_mut().remove_class(self.label.element(), "referenceMatch");
            self.label.set_styles(styles.reference);
            let highlights: Vec<IHighlight> = create_matches(score).into_iter().map(|m| IHighlight::new(m.start, m.end)).collect();
            self.label.set(Some(&value), &highlights, "", false, false);
        } else {
            self.ui.tree_mut().add_class(self.label.element(), "referenceMatch");
            self.label.set_styles(styles.reference_match);
            self.label.set(Some(&value), &[IHighlight::new(preview.highlight.start, preview.highlight.end)], "", false, false);
        }
    }
}

impl IDisposable for OneReferenceTemplate {
    fn dispose(&self) {
        self.label.dispose();
    }
}

/// `OneReferenceRenderer`.
pub struct OneReferenceRenderer {
    services: ReferencesTreeRendererServices,
}

impl OneReferenceRenderer {
    pub const ID: &'static str = "OneReferenceRenderer";

    pub fn new(services: ReferencesTreeRendererServices) -> Self {
        Self { services }
    }
}

impl IAsyncDataTreeRenderer<ReferencesTreeInput, TreeElement, FuzzyScore> for OneReferenceRenderer {
    fn template_id(&self) -> String {
        Self::ID.to_owned()
    }

    fn render_template(&self, container: NodeId) -> TemplateData {
        Rc::new(OneReferenceTemplate::new(&self.services, container))
    }

    fn render_element(
        &self,
        node: &Rc<AsyncDataTreeNodeWrapper<ReferencesTreeInput, TreeElement, FuzzyScore>>,
        _index: usize,
        template_data: &TemplateData,
        _details: Option<&TreeElementRenderDetails>,
    ) {
        let (Some(template), InputOrElement::Element(TreeElement::Reference(reference))) = (template_data.downcast_ref::<OneReferenceTemplate>(), node.element()) else {
            return;
        };
        template.set(&reference, node.filter_data().as_deref(), &self.services.styles());
    }

    fn dispose_template(&self, template_data: &TemplateData) {
        if let Some(template) = template_data.downcast_ref::<OneReferenceTemplate>() {
            template.dispose();
        }
    }
}

// #endregion
