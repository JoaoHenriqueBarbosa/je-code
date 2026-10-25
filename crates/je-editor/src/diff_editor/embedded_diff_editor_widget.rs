//! Porte de `src/vs/editor/browser/widget/diffEditor/embeddedDiffEditorWidget.ts`: o `EmbeddedDiffEditorWidget`, um editor de diff
//! que vive dentro de um editor de código (o editor pai) e herda as opções dele, com as suas por cima.
//!
//! Divergências (as mesmas do [`crate::embedded_code_editor_widget`], que é o par de código):
//! - `class EmbeddedDiffEditorWidget extends DiffEditorWidget` é a struct [`EmbeddedDiffEditorWidget`], que guarda o widget de
//!   diff em [`EmbeddedDiffEditorWidget::widget`]; quem usava o embutido como `IDiffEditor` usa o widget interno. O que o
//!   `super` oferece ao embutido (`updateOptions` e o descarte) é o trait [`EmbeddedDiffEditorBase`], que o `DiffEditorWidget`
//!   da fatia 2 implementa; assim este módulo não depende do widget, que ainda não foi portado.
//! - O construtor do original recebe o `domElement` e os cinco serviços injetados (`@IContextKeyService`,
//!   `@IInstantiationService`, `@ICodeEditorService`, `@IAccessibilitySignalService`, `@IEditorProgressService`) só para
//!   repassá-los ao `super(...)`. Aqui quem monta o widget com esses serviços e o contêiner é a [`IEmbeddedDiffEditorFactory`],
//!   que recebe as opções cruas do pai e as `IDiffCodeEditorWidgetOptions`.
//! - `IDiffEditorOptions` (as opções cruas) é um objeto [`Dynamic`]; `objects.mixin(this._overwriteOptions, newOptions, true)` é o
//!   [`mixin`] de `je_base::objects`, que muda o objeto em lugar (o `_overwriteOptions` é a referência recebida, como no
//!   original).
//! - `parentEditor: ICodeEditor` é o `Rc<CodeEditorWidget>`, e `this._register(parentEditor.onDidChangeConfiguration(...))` e o
//!   widget interno ficam no `base` do embutido, que os descarta junto.

use std::rc::Rc;

use je_base::dynamic::Dynamic;
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::objects::mixin;

use crate::code_editor_widget::{CodeEditorWidget, ICodeEditor};
use crate::diff_editor::DiffCodeEditorWidgetOptions;

/// O que o `EmbeddedDiffEditorWidget` usa do `DiffEditorWidget` que ele estende: o `super.updateOptions` e o descarte.
pub trait EmbeddedDiffEditorBase: IDisposable + 'static {
    /// `updateOptions(changedOptions: IDiffEditorOptions)`.
    fn update_options(&self, new_options: &Dynamic);
}

/// O que cria o widget de diff de dentro de um `EmbeddedDiffEditorWidget`: o papel do contêiner e dos serviços injetados no
/// construtor do original (ver as notas do módulo).
pub trait IEmbeddedDiffEditorFactory {
    type Widget: EmbeddedDiffEditorBase;

    /// Cria o widget com as opções cruas do pai (`parentEditor.getRawOptions()`) e as opções dos dois editores.
    fn create_diff_editor(&self, raw_options: Dynamic, widget_options: DiffCodeEditorWidgetOptions) -> Rc<Self::Widget>;
}

/// `EmbeddedDiffEditorWidget`.
pub struct EmbeddedDiffEditorWidget<W: EmbeddedDiffEditorBase> {
    base: Rc<Disposable>,
    widget: Rc<W>,
    parent_editor: Rc<CodeEditorWidget>,
    overwrite_options: Dynamic,
}

impl<W: EmbeddedDiffEditorBase> EmbeddedDiffEditorWidget<W> {
    /// O construtor: `options` são as opções que valem por cima das do pai (`_overwriteOptions`).
    pub fn new<F: IEmbeddedDiffEditorFactory<Widget = W>>(
        options: Dynamic,
        code_editor_widget_options: DiffCodeEditorWidgetOptions,
        parent_editor: Rc<CodeEditorWidget>,
        factory: &F,
    ) -> Rc<Self> {
        let widget = factory.create_diff_editor(parent_editor.get_raw_options(), code_editor_widget_options);

        // Overwrite parent's options
        widget.update_options(&options);

        let base = Disposable::new();
        base.register(widget.clone());
        base.register(parent_editor.on_did_change_configuration().listen({
            let (widget, parent_editor, overwrite_options) = (Rc::clone(&widget), Rc::clone(&parent_editor), options.clone());
            move |_| {
                // `_onParentConfigurationChanged`
                widget.update_options(&parent_editor.get_raw_options());
                widget.update_options(&overwrite_options);
            }
        }));

        Rc::new(Self { base, widget, parent_editor, overwrite_options: options })
    }

    /// O widget de diff que o embutido é (o `this` do original nas chamadas herdadas).
    pub fn widget(&self) -> &Rc<W> {
        &self.widget
    }

    /// `getParentEditor()`.
    pub fn get_parent_editor(&self) -> Rc<CodeEditorWidget> {
        Rc::clone(&self.parent_editor)
    }

    /// `updateOptions(newOptions)`: as opções novas entram nas que valem por cima das do pai.
    pub fn update_options(&self, new_options: &Dynamic) {
        mixin(&self.overwrite_options, new_options, true);
        self.widget.update_options(&self.overwrite_options);
    }
}

impl<W: EmbeddedDiffEditorBase> IDisposable for EmbeddedDiffEditorWidget<W> {
    fn dispose(&self) {
        self.base.dispose();
    }
}
