//! Porte de `src/vs/editor/test/browser/viewModel/testViewModel.ts`: `testViewModel`, que cria um `ViewModel` sobre
//! um modelo novo, roda o caso e descarta tudo.
//!
//! Representação:
//! - `IEditorOptions` é o objeto [`Dynamic`] que a `TestConfiguration` lê (ver `test_configuration.rs`); `{}` é
//!   [`no_options`].
//! - O `callback` recebe `(viewModel, model, configuration)`, o terceiro como o `IEditorConfiguration` que o
//!   `TestConfiguration` implementa.
//! - O alvo transacional `{ batchChanges: (cb) => cb() }` é o [`TestBatchTarget`], o `IBatchableTarget` que só
//!   chama o callback (o `ViewModel` do porte é genérico no alvo, ver `view_model_impl.rs`).
//! - O `IAttachedView` `{ setVisibleLines(visibleLines, stabilized) {} }` é um [`AttachedView`] avulso (criado por um
//!   `AttachedViews` que não pertence a nenhum modelo, então o modelo não vê editor ligado), que também ignora
//!   o que o `ViewModel` informa.
//!
//! Divergências:
//! - `scheduleAtNextAnimationFrame` é `null!` no original (quem o chamasse lançaria). O `ViewModel` do porte exige um
//!   valor: é uma função que descarta o callback, sem agendar nada, o que os casos síncronos nunca distinguem.
//! - O `TestThemeService` do original não é descartado; aqui ele é, junto com o resto, no fim (descartar um serviço
//!   que ninguém mais usa não muda o que o caso observa).

use std::rc::Rc;

use je_base::dynamic::Dynamic;
use je_base::lifecycle::{IDisposable, plain_disposable};
use je_base::scrollable::ScheduleAtNextAnimationFrame;
use je_editor::attached_views::AttachedViews;
use je_editor::config::editor_configuration::IEditorConfiguration;
use je_editor::cursor_common::EditorConfiguration as ConfigurationOptions;
use je_editor::model_line_projection_data::LineBreaksComputerFactory;
use je_editor::monospace_line_breaks_computer::MonospaceLineBreaksComputerFactory;
use je_editor::text_model::TextModel;
use je_editor::view_model_impl::ViewModel;
use je_editor::view_model_impl_support::IBatchableTarget;

use super::test_configuration::TestConfiguration;
use super::test_language_configuration_service::TestLanguageConfigurationService;
use super::test_text_model::create_text_model;
use super::test_theme_service::TestThemeService;

/// O `{ batchChanges: (cb) => cb() }` do original.
pub struct TestBatchTarget;

impl IBatchableTarget for TestBatchTarget {
    fn batch_changes<T>(&self, cb: impl FnOnce() -> T) -> T {
        cb()
    }
}

/// O `ViewModel` que `testViewModel` cria.
pub type TestViewModel = ViewModel<TestBatchTarget>;

/// O `{}` das opções do editor.
pub fn no_options() -> Dynamic {
    Dynamic::object(std::iter::empty::<(&str, Dynamic)>())
}

/// `testViewModel(text, options, callback)`.
pub fn test_view_model(
    text: &[&str],
    options: &Dynamic,
    callback: impl FnOnce(&Rc<TestViewModel>, &Rc<TextModel>, &Rc<dyn IEditorConfiguration>),
) {
    const EDITOR_ID: i32 = 1;

    let configuration: Rc<dyn IEditorConfiguration> = TestConfiguration::new(options);
    let model = create_text_model(&text.join("\n"));
    let monospace_line_breaks_computer_factory: Rc<dyn LineBreaksComputerFactory> =
        Rc::new(MonospaceLineBreaksComputerFactory::create(ConfigurationOptions::options(&*configuration)));
    let test_language_configuration_service = TestLanguageConfigurationService::new();
    let theme_service = TestThemeService::new();
    let attached_views = AttachedViews::new();
    let schedule_at_next_animation_frame: ScheduleAtNextAnimationFrame =
        Rc::new(|_callback| plain_disposable(|| {}));
    let view_model = ViewModel::new(
        EDITOR_ID,
        configuration.clone(),
        model.clone(),
        monospace_line_breaks_computer_factory.clone(),
        monospace_line_breaks_computer_factory,
        schedule_at_next_animation_frame,
        test_language_configuration_service.clone(),
        theme_service.clone(),
        attached_views.attach_view(),
        TestBatchTarget,
    );

    callback(&view_model, &model, &configuration);

    view_model.dispose();
    model.dispose();
    configuration.dispose();
    test_language_configuration_service.dispose();
    theme_service.dispose();
    attached_views.dispose();
}
