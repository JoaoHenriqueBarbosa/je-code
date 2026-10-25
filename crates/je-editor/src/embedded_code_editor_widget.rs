//! Porte de `src/vs/editor/browser/widget/codeEditor/embeddedCodeEditorWidget.ts`: o `EmbeddedCodeEditorWidget`, um
//! editor de código que vive dentro de outro (o editor pai) e herda as opções dele, com as suas por cima.
//!
//! Notas de tradução:
//! - `class EmbeddedCodeEditorWidget extends CodeEditorWidget` é a struct [`EmbeddedCodeEditorWidget`] que guarda o
//!   `Rc<CodeEditorWidget>` em [`EmbeddedCodeEditorWidget::editor`]: o `CodeEditorWidget` não tem subclasse em Rust, e quem
//!   usava o embutido como `ICodeEditor` usa o editor interno;
//! - o construtor do original recebe o `domElement` e os dez serviços injetados (`@IInstantiationService`,
//!   `@ICodeEditorService`, `@ICommandService`, `@IContextKeyService`, `@IThemeService`, `@INotificationService`,
//!   `@IAccessibilityService`, `@ILanguageConfigurationService`, `@ILanguageFeaturesService`,
//!   `@IUserInteractionService`) só para repassá-los ao `super(...)`. Aqui quem monta o widget com esses serviços e a
//!   configuração é a [`IEmbeddedCodeEditorFactory`] (o dono do contêiner nativo e dos serviços, o mesmo que cria os
//!   editores de código comuns), que recebe as opções cruas já mescladas (`{...parentEditor.getRawOptions(),
//!   overflowWidgetsDomNode}`), as `ICodeEditorWidgetOptions` e os `contextKeyValues` (que no porte não fazem parte do
//!   [`CodeEditorWidgetOptions`]: pertencem ao serviço de chaves de contexto, que a fábrica possui);
//! - `overflowWidgetsDomNode: parentEditor.getOverflowWidgetsDomNode()` é um nó DOM: não existe aqui, o contêiner nativo dos
//!   widgets de overflow é da fábrica, que o liga ao editor novo;
//! - `IEditorOptions` (as opções cruas) é um objeto [`Dynamic`]; `objects.mixin(this._overwriteOptions, newOptions, true)`
//!   é o [`mixin`] de `je_base::objects`, que muda o objeto em lugar (o `_overwriteOptions` é uma referência, como no
//!   original);
//! - `super.updateOptions(...)` é o `update_options` do editor interno. O `override updateOptions` é
//!   [`EmbeddedCodeEditorWidget::update_options`], que quem usava o embutido por essa porta continua chamando;
//! - `instanceof EmbeddedCodeEditorWidget` do `getOuterEditor` é a consulta a um registro dos editores embutidos vivos
//!   (pelo endereço do editor interno), mantido pelo construtor e pelo `dispose`;
//! - `this._register(parentEditor.onDidChangeConfiguration(...))` e o próprio editor interno ficam no `base` do embutido,
//!   que o descarta junto.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use je_base::dynamic::Dynamic;
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::objects::mixin;
use je_instantiation::ServicesAccessor;

use crate::code_editor_widget::{CodeEditorWidget, CodeEditorWidgetOptions, ICodeEditor};
use crate::editor_extensions::I_CODE_EDITOR_LOOKUP;

/// O que cria o `CodeEditorWidget` de dentro de um `EmbeddedCodeEditorWidget`: o papel dos serviços injetados no
/// construtor do original (ver as notas do módulo).
pub trait IEmbeddedCodeEditorFactory {
    /// Cria o editor com as opções cruas já mescladas com as do pai, as opções do widget e os valores iniciais das chaves
    /// de contexto (`contextKeyValues`: o nome da chave e o valor).
    fn create_code_editor(
        &self,
        raw_options: Dynamic,
        widget_options: CodeEditorWidgetOptions,
        context_key_values: Vec<(String, Dynamic)>,
    ) -> Rc<CodeEditorWidget>;
}

thread_local! {
    /// Os editores embutidos vivos: o endereço do editor interno e o pai dele.
    static EMBEDDED_EDITORS: RefCell<Vec<(usize, Weak<CodeEditorWidget>)>> = const { RefCell::new(Vec::new()) };
}

/// O endereço que identifica um editor no registro dos embutidos.
fn editor_key(editor: &Rc<CodeEditorWidget>) -> usize {
    Rc::as_ptr(editor) as usize
}

/// `EmbeddedCodeEditorWidget`.
pub struct EmbeddedCodeEditorWidget {
    base: Rc<Disposable>,
    editor: Rc<CodeEditorWidget>,
    parent_editor: Rc<CodeEditorWidget>,
    overwrite_options: Dynamic,
}

impl EmbeddedCodeEditorWidget {
    /// O construtor: `options` são as opções que valem por cima das do pai (`_overwriteOptions`).
    pub fn new(
        options: Dynamic,
        code_editor_widget_options: CodeEditorWidgetOptions,
        context_key_values: Vec<(String, Dynamic)>,
        parent_editor: Rc<CodeEditorWidget>,
        factory: &dyn IEmbeddedCodeEditorFactory,
    ) -> Rc<Self> {
        let parent_options = Dynamic::object(parent_editor.get_raw_options().own_entries());
        let editor = factory.create_code_editor(parent_options, code_editor_widget_options, context_key_values);

        // Overwrite parent's options
        editor.update_options(Some(&options));

        let base = Disposable::new();
        base.register(editor.clone());
        EMBEDDED_EDITORS.with(|editors| editors.borrow_mut().push((editor_key(&editor), Rc::downgrade(&parent_editor))));

        base.register(parent_editor.on_did_change_configuration().listen({
            let (editor, parent_editor, overwrite_options) = (Rc::clone(&editor), Rc::clone(&parent_editor), options.clone());
            move |_| {
                // `_onParentConfigurationChanged`
                editor.update_options(Some(&parent_editor.get_raw_options()));
                editor.update_options(Some(&overwrite_options));
            }
        }));

        Rc::new(Self { base, editor, parent_editor, overwrite_options: options })
    }

    /// O `CodeEditorWidget` que o embutido é (o `this` do original nas chamadas herdadas).
    pub fn editor(&self) -> &Rc<CodeEditorWidget> {
        &self.editor
    }

    /// `getParentEditor()`.
    pub fn get_parent_editor(&self) -> Rc<CodeEditorWidget> {
        Rc::clone(&self.parent_editor)
    }

    /// `updateOptions(newOptions)`: as opções novas entram nas que valem por cima das do pai.
    pub fn update_options(&self, new_options: &Dynamic) {
        mixin(&self.overwrite_options, new_options, true);
        self.editor.update_options(Some(&self.overwrite_options));
    }
}

impl IDisposable for EmbeddedCodeEditorWidget {
    fn dispose(&self) {
        let key = editor_key(&self.editor);
        EMBEDDED_EDITORS.with(|editors| editors.borrow_mut().retain(|(entry, _)| *entry != key));
        self.base.dispose();
    }
}

/// `getOuterEditor(accessor)`: o editor com foco, ou o pai dele se for um editor embutido.
pub fn get_outer_editor(accessor: &ServicesAccessor) -> Option<Rc<CodeEditorWidget>> {
    let editor = accessor.get(I_CODE_EDITOR_LOOKUP).get_focused_code_editor()?;
    let key = editor_key(&editor);
    let parent = EMBEDDED_EDITORS.with(|editors| editors.borrow().iter().find(|(entry, _)| *entry == key).map(|(_, parent)| parent.clone()));
    match parent.and_then(|parent| parent.upgrade()) {
        Some(parent) => Some(parent),
        None => Some(editor),
    }
}
