//! Porte de `src/vs/editor/common/config/editorConfiguration.ts`: a interface `IEditorConfiguration`, a
//! configuração de um editor (as opções calculadas, os eventos de mudança e os ajustes que o editor faz nas
//! opções conforme o conteúdo).
//!
//! Decisões de tradução:
//! - `IEditorConfiguration extends IDisposable` é o supertrait [`IDisposable`]. As opções calculadas (`options`)
//!   vêm do supertrait [`ConfigurationOptions`] (o `EditorConfiguration` de `cursor_common.rs`, que só traz
//!   `options`), assim um `Rc<dyn IEditorConfiguration>` vira `Rc<dyn ConfigurationOptions>` por coerção, que é o que
//!   o `ViewLayout`, o `ViewModelDecorations` e o `CursorConfiguration` pedem.
//! - `IEditorOptions` (as opções cruas, que o chamador passa e que `updateOptions` mescla) é um objeto
//!   [`Dynamic`], como no registro de opções (`editor_options.rs`).
//! - `MenuId` é o de `je_platform::actions`; `IDimension?` é `Option<Dimension>`.
//! - Os `number` de contagem de linhas são `i32` (como `ViewModel` os passa) e os de altura e de faixas da margem
//!   de glifos são `f64`.
//! - `contextMenuId` devolve uma cópia do [`MenuId`] (o original devolve a mesma instância; o `MenuId` é só o
//!   identificador).

use je_base::dynamic::Dynamic;
use je_base::event::Event;
use je_base::lifecycle::IDisposable;
use je_platform::actions::MenuId;

use crate::config::editor_options::ConfigurationChangedEvent;
use crate::core_2d::dimension::Dimension;
use crate::cursor_common::EditorConfiguration as ConfigurationOptions;

/// `IEditorConfiguration`.
pub trait IEditorConfiguration: ConfigurationOptions + IDisposable {
    /// `isSimpleWidget`: é um widget simples (não um editor de código de verdade)?
    fn is_simple_widget(&self) -> bool;

    /// `contextMenuId`: o id do menu de contexto do editor.
    fn context_menu_id(&self) -> MenuId;

    /// `onDidChangeFast`: as opções mudaram (evento rápido).
    fn on_did_change_fast(&self) -> Event<ConfigurationChangedEvent>;

    /// `onDidChange`: as opções mudaram (evento lento).
    fn on_did_change(&self) -> Event<ConfigurationChangedEvent>;

    /// `getRawOptions()`: as opções cruas como foram passadas ao editor, mescladas com todas as chamadas de
    /// `updateOptions`. É o mesmo objeto (compartilhado), não uma cópia.
    fn get_raw_options(&self) -> Dynamic;

    /// `updateOptions(newOptions)`: atualiza as opções com opções parciais. As anteriores ficam, só as chaves
    /// presentes são sobrescritas.
    fn update_options(&self, new_options: &Dynamic);

    /// `observeContainer(dimension?)`: recalcula as opções com as novas dimensões do elemento de referência.
    fn observe_container(&self, dimension: Option<Dimension>);

    /// `setIsDominatedByLongLines`: o modelo atual é dominado por linhas longas?
    fn set_is_dominated_by_long_lines(&self, is_dominated_by_long_lines: bool);

    /// `setModelLineCount`: o número de linhas do modelo.
    fn set_model_line_count(&self, model_line_count: i32);

    /// `setViewLineCount`: o número de linhas do view model.
    fn set_view_line_count(&self, view_line_count: i32);

    /// `setReservedHeight`: a altura reservada em cima.
    fn set_reserved_height(&self, reserved_height: f64);

    /// `setGlyphMarginDecorationLaneCount`: o número de faixas de decoração a desenhar na margem de glifos.
    fn set_glyph_margin_decoration_lane_count(&self, decoration_lane_count: f64);
}
