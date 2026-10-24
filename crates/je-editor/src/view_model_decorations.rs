//! Porte de `src/vs/editor/common/viewModel/viewModelDecorations.ts`: `ViewModelDecorations`, o cache das
//! decorações do viewport sobre o `InlineModelDecorationsComputer`.
//!
//! Divergências de tradução:
//! - O original guarda `IEditorConfiguration` e lê `configuration.options` a cada chamada; aqui é o trait
//!   [`EditorConfiguration`] de `cursor_common` em `Rc`, lido do mesmo modo, a cada chamada.
//! - O contexto do computador (um objeto literal com `getModelDecorations` que fecha sobre `this`) é a struct
//!   privada `LinesDecorationsContext`, que guarda o id do editor, a configuração e as linhas.
//! - `IViewDecorationsCollection` é [`ViewDecorationsCollection`] (`inline_decorations`). O cache guarda um
//!   `Rc` dela, porque o original devolve a mesma instância em chamadas seguidas; o cache fica em `RefCell`,
//!   para os métodos continuarem `&self` como no resto do crate (`IDisposable::dispose` é `&self`).
//! - `getDecorationsOnLine` tem `onlyMinimapDecorations` e `onlyMarginDecorations` com padrão `false`: aqui são
//!   parâmetros obrigatórios.
//! - `filterValidationDecorations` e `filterFontDecorations` (`config/editorOptions.ts`) ainda não têm porte
//!   no módulo de opções; ficam aqui como funções privadas, com o mesmo corpo do original.
//! - `getMinimapDecorationsInRange` devolve os `Rc<ViewModelDecoration>` do computador.

use std::cell::RefCell;
use std::rc::Rc;

use je_base::lifecycle::IDisposable;

use crate::config::editor_options::ComputedEditorOptions;
use crate::config::editor_options::effective_options::EFFECTIVE_ALLOW_VARIABLE_FONTS;
use crate::config::editor_options::registry::{READ_ONLY, RENDER_VALIDATION_DECORATIONS};
use crate::coordinates_converter::ICoordinatesConverter;
use crate::cursor_common::EditorConfiguration;
use crate::inline_decorations::{
    IInlineModelDecorationsComputerContext, InlineModelDecorationsComputer, ViewDecorationsCollection,
};
use crate::model::ModelDecoration;
use crate::range::Range;
use crate::text_model::TextModel;
use crate::view_model_decoration::ViewModelDecoration;
use crate::view_model_lines::IViewModelLines;

/// `filterValidationDecorations(options)`.
pub(crate) fn filter_validation_decorations(options: &ComputedEditorOptions) -> bool {
    let render_validation_decorations = options.get(RENDER_VALIDATION_DECORATIONS);
    if render_validation_decorations == "editable" {
        return *options.get(READ_ONLY);
    }
    render_validation_decorations != "on"
}

/// `filterFontDecorations(options)`.
pub(crate) fn filter_font_decorations(options: &ComputedEditorOptions) -> bool {
    !*options.get(EFFECTIVE_ALLOW_VARIABLE_FONTS)
}

/// O `IInlineModelDecorationsComputerContext` do original: pede as decorações às linhas do viewModel, com os
/// filtros que vêm das opções vigentes.
struct LinesDecorationsContext {
    editor_id: i32,
    configuration: Rc<dyn EditorConfiguration>,
    lines_collection: Rc<dyn IViewModelLines>,
}

impl IInlineModelDecorationsComputerContext for LinesDecorationsContext {
    fn get_model_decorations(&self, view_range: &Range, only_minimap_decorations: bool, only_margin_decorations: bool) -> Vec<ModelDecoration> {
        let options = self.configuration.options();
        self.lines_collection.get_decorations_in_range(
            *view_range,
            self.editor_id,
            filter_validation_decorations(options),
            filter_font_decorations(options),
            only_minimap_decorations,
            only_margin_decorations,
        )
    }
}

/// O resultado em cache de `getDecorationsViewportData` e o intervalo da visão a que ele corresponde.
struct CachedViewportDecorations {
    view_range: Range,
    collection: Rc<ViewDecorationsCollection>,
}

/// `ViewModelDecorations`.
pub struct ViewModelDecorations {
    lines_collection: Rc<dyn IViewModelLines>,
    inline_decorations_computer: InlineModelDecorationsComputer,
    cached_model_decorations_resolver: RefCell<Option<CachedViewportDecorations>>,
}

impl ViewModelDecorations {
    pub fn new(
        editor_id: i32,
        model: Rc<TextModel>,
        configuration: Rc<dyn EditorConfiguration>,
        lines_collection: Rc<dyn IViewModelLines>,
        coordinates_converter: Rc<dyn ICoordinatesConverter>,
    ) -> Self {
        let context = Rc::new(LinesDecorationsContext { editor_id, configuration, lines_collection: lines_collection.clone() });
        Self {
            lines_collection,
            inline_decorations_computer: InlineModelDecorationsComputer::new(context, model, coordinates_converter),
            cached_model_decorations_resolver: RefCell::new(None),
        }
    }

    fn clear_cached_model_decorations_resolver(&self) {
        *self.cached_model_decorations_resolver.borrow_mut() = None;
    }

    pub fn reset(&self) {
        self.inline_decorations_computer.reset();
        self.clear_cached_model_decorations_resolver();
    }

    pub fn on_model_decorations_changed(&self) {
        self.inline_decorations_computer.on_model_decorations_changed();
        self.clear_cached_model_decorations_resolver();
    }

    pub fn on_line_mapping_changed(&self) {
        self.inline_decorations_computer.on_line_mapping_changed();
        self.clear_cached_model_decorations_resolver();
    }

    pub fn get_minimap_decorations_in_range(&self, range: Range) -> Vec<Rc<ViewModelDecoration>> {
        self.inline_decorations_computer.get_decorations(range, true, false).decorations
    }

    pub fn get_decorations_viewport_data(&self, view_range: Range) -> Rc<ViewDecorationsCollection> {
        if let Some(cached) = self.cached_model_decorations_resolver.borrow().as_ref()
            && cached.view_range == view_range
        {
            return cached.collection.clone();
        }
        let collection = Rc::new(self.inline_decorations_computer.get_decorations(view_range, false, false));
        *self.cached_model_decorations_resolver.borrow_mut() =
            Some(CachedViewportDecorations { view_range, collection: collection.clone() });
        collection
    }

    pub fn get_decorations_on_line(&self, line_number: i32, only_minimap_decorations: bool, only_margin_decorations: bool) -> ViewDecorationsCollection {
        let range = Range::new(
            line_number,
            self.lines_collection.get_view_line_min_column(line_number),
            line_number,
            self.lines_collection.get_view_line_max_column(line_number),
        );
        self.inline_decorations_computer.get_decorations(range, only_minimap_decorations, only_margin_decorations)
    }
}

impl IDisposable for ViewModelDecorations {
    fn dispose(&self) {
        self.reset();
    }
}
