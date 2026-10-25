//! Porte de `src/vs/editor/contrib/inlineCompletions/browser/view/inlineEdits/inlineEditsViews/longDistanceHint/`: a
//! dica de longa distância (`InlineEditsLongDistanceHint`), o editor de prévia que ela mostra
//! (`LongDistancePreviewEditor`), e o posicionamento do widget (`longDistnaceWidgetPlacement`, em
//! `inline_completions::long_distance_widget_placement`, e `scrollToReveal`, em `inline_completions::long_distance_hint`).
//!
//! O que o original pega do ambiente sem um serviço portado entra pelo [`ILongDistanceHintHost`], no mesmo molde dos
//! `ISideBySideViewHost` e `IGutterIndicatorHost` das views irmãs:
//! - `HideUnchangedRegionsFeature._breadcrumbsSourceFactory` (a fábrica da fonte da trilha de símbolos, que a camada de
//!   breadcrumbs registra) e a [`IDiffEditorBreadcrumbsSource`] que ela devolve (a do diff editor, sem cópia):
//!   [`ILongDistanceHintHost::outline_source`]. O ícone do símbolo é `SymbolKind::to_icon` do `kind` do `BreadcrumbItem`;
//! - `basename(uri)` e `getIconClasses(modelService, languageService, uri, FileKind.FILE)` (o `@IModelService` e o
//!   `@ILanguageService` do construtor só serviam a isso): [`ILongDistanceHintHost::target_file`].

use std::rc::Rc;

use je_base::observable::IReader;
use je_base::uri::URI;
use je_platform::accessibility::IAccessibilityService;
use je_platform::keybinding::IKeybindingService;

use crate::diff_editor::features::hide_unchanged_regions_feature::IDiffEditorBreadcrumbsSource;
use crate::embedded_code_editor_widget::IEmbeddedCodeEditorFactory;
use crate::services::theme_service::IThemeService;
use crate::text_model::TextModel;

use super::super::components::gutter_indicator_view::IGutterIndicatorHost;

pub mod inline_edits_long_distance_hint;
pub mod long_distance_preview_editor;

/// O nome do arquivo e as classes de ícone dele (`basename(uri)` e `getIconClasses(...)`).
#[derive(Clone, Debug, PartialEq)]
pub struct TargetFileInfo {
    pub file_name: String,
    pub icon_classes: Vec<String>,
}

/// O que a dica de longa distância pede ao ambiente (ver as notas do módulo).
pub trait ILongDistanceHintHost {
    /// `HideUnchangedRegionsFeature._breadcrumbsSourceFactory.read(reader)` aplicada ao modelo: `None` sem fábrica.
    fn outline_source(&self, text_model: &Rc<TextModel>, reader: &dyn IReader) -> Option<Rc<dyn IDiffEditorBreadcrumbsSource>>;

    /// `basename(uri)` e `getIconClasses(...)` de um arquivo.
    fn target_file(&self, uri: &URI) -> TargetFileInfo;
}

/// Os serviços da dica de longa distância e do editor de prévia dela: os `@I...` do construtor do original.
#[derive(Clone)]
pub struct LongDistanceHintServices {
    pub theme_service: Rc<dyn IThemeService>,
    pub keybinding_service: Rc<dyn IKeybindingService>,
    pub accessibility_service: Rc<dyn IAccessibilityService>,
    /// O que cria o `CodeEditorWidget` do `EmbeddedCodeEditorWidget` (o `@IInstantiationService` do editor de prévia).
    pub embedded_code_editor_factory: Rc<dyn IEmbeddedCodeEditorFactory>,
    /// O menu e o sticky scroll do indicador da margem que o editor de prévia mostra.
    pub gutter_indicator_host: Rc<dyn IGutterIndicatorHost>,
    pub host: Rc<dyn ILongDistanceHintHost>,
}
