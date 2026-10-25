//! Porte de `src/vs/editor/browser/widget/diffEditor/delegatingEditorImpl.ts`: a base `DelegatingEditor` do editor de diff, que
//! entrega ao editor de código alvo (o modificado) a parte da API de editor que o diff não reimplementa.
//!
//! Divergências:
//! - **`abstract class` é o trait [`DelegatingEditor`] mais o estado [`DelegatingEditorBase`].** O estado do original (o `_id`
//!   sequencial, o `_onDidDispose` e a herança de `Disposable`) é o [`DelegatingEditorBase`], que o implementador guarda e devolve
//!   em [`DelegatingEditor::delegating_base`]. O `_onDidDispose` é privado no original e nenhuma subclasse o dispara, então
//!   `onDidDispose` nunca dispara; o porte mantém isso.
//! - **Os métodos delegantes não são reescritos.** `getVisibleColumnFromPosition`, `getStatusbarColumn`, `getPosition`,
//!   `setPosition`, `revealLine*`, `revealPosition*`, `getSelection(s)`, `setSelection(s)`, `revealLines*`, `revealRange*`,
//!   `getSupportedActions`, `focus`, `trigger`, `createDecorationsCollection` e `changeDecorations` têm todos o corpo
//!   `this._targetEditor.<método>(mesmos argumentos)`, e uma função de repasse não existe neste projeto (ver "DRY" no
//!   `CLAUDE.md`). O conceito que as une é o `_targetEditor`, e é ele que o trait expõe: [`DelegatingEditor::target_editor`]
//!   devolve o `Rc<CodeEditorWidget>` e a chamada é a de `ICodeEditor` nele
//!   (`diff_editor.target_editor().reveal_line(n, None)`), com o mesmo efeito do método delegante do original.
//! - **`IEditorModel` e `IEditorViewState`** (`editorCommon.ts`) são as enums [`EditorModel`] e [`EditorViewState`], e
//!   `IDiffEditorViewState` é [`DiffEditorViewState`]. O `setModel(model: IEditorModel | null | IDiffEditorViewModel)` aceita as
//!   três formas pela enum (o `IDiffEditorViewModel` já faz parte de `IEditorModel` na versão do oráculo).
//! - **`layout(dimension?: IDimension)`** recebe `Option<Dimension>` (`IDimension` e `Dimension` são o mesmo tipo).
//! - **`getId()`** devolve `getEditorType() + ':v2:' + id`, com o `id` do contador compartilhado pelas subclasses.

use std::cell::Cell;
use std::rc::Rc;

use je_base::dynamic::Dynamic;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, DisposableRef, IDisposable};

use crate::code_editor_widget::{CodeEditorWidget, ICodeEditorViewState};
use crate::core_2d::dimension::Dimension;
use crate::diff_editor_view_model::{DiffEditorModel, IDiffEditorViewModel};
use crate::text_model::TextModel;

thread_local! {
    /// `private static idCounter = 0`.
    static ID_COUNTER: Cell<i32> = const { Cell::new(0) };
}

/// `++DelegatingEditor.idCounter`.
fn next_id() -> i32 {
    ID_COUNTER.with(|counter| {
        counter.set(counter.get() + 1);
        counter.get()
    })
}

/// O estado de um `DelegatingEditor`: o `_id`, o `_onDidDispose` e o `Disposable` de onde o editor registra o que cria.
pub struct DelegatingEditorBase {
    id: i32,
    base: Rc<Disposable>,
    on_did_dispose: Rc<Emitter<()>>,
}

impl Default for DelegatingEditorBase {
    fn default() -> Self {
        let base = Disposable::new();
        let on_did_dispose = Emitter::<()>::new();
        base.register(on_did_dispose.clone());
        Self { id: next_id(), base, on_did_dispose }
    }
}

impl DelegatingEditorBase {
    /// O `_id` sequencial do editor.
    pub fn id(&self) -> i32 {
        self.id
    }

    /// `onDidDispose`.
    pub fn on_did_dispose(&self) -> Event<()> {
        self.on_did_dispose.event()
    }

    /// `this._register(disposable)`.
    pub fn register(&self, disposable: DisposableRef) -> DisposableRef {
        self.base.register(disposable)
    }
}

impl IDisposable for DelegatingEditorBase {
    fn dispose(&self) {
        self.base.dispose();
    }
}

/// `IEditorModel`: `ITextModel | IDiffEditorModel | IDiffEditorViewModel`.
#[derive(Clone)]
pub enum EditorModel {
    Text(Rc<TextModel>),
    Diff(DiffEditorModel),
    DiffViewModel(Rc<dyn IDiffEditorViewModel>),
}

/// `IDiffEditorViewState`.
#[derive(Clone)]
pub struct DiffEditorViewState {
    pub original: Option<ICodeEditorViewState>,
    pub modified: Option<ICodeEditorViewState>,
    /// `modelState?: unknown`.
    pub model_state: Option<Dynamic>,
}

/// `IEditorViewState`: `ICodeEditorViewState | IDiffEditorViewState`.
#[derive(Clone)]
pub enum EditorViewState {
    Code(ICodeEditorViewState),
    Diff(DiffEditorViewState),
}

/// `DelegatingEditor`.
pub trait DelegatingEditor {
    /// O estado da base (o `_id`, o `_onDidDispose`, o `Disposable`).
    fn delegating_base(&self) -> &DelegatingEditorBase;

    /// `protected abstract get _targetEditor(): CodeEditorWidget`: o editor a que a API delegante do original é repassada.
    fn target_editor(&self) -> Rc<CodeEditorWidget>;

    fn get_editor_type(&self) -> &'static str;
    fn update_options(&self, new_options: &Dynamic);
    fn on_visible(&self);
    fn on_hide(&self);
    fn layout(&self, dimension: Option<Dimension>);
    fn has_text_focus(&self) -> bool;
    fn save_view_state(&self) -> Option<EditorViewState>;
    fn restore_view_state(&self, state: Option<&EditorViewState>);
    fn get_model(&self) -> Option<EditorModel>;
    fn set_model(&self, model: Option<EditorModel>);

    /// `getId()`.
    fn get_id(&self) -> String {
        format!("{}:v2:{}", self.get_editor_type(), self.delegating_base().id())
    }

    /// `onDidDispose`.
    fn on_did_dispose(&self) -> Event<()> {
        self.delegating_base().on_did_dispose()
    }
}
