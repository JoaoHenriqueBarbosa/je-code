//! Porte de `src/vs/workbench/contrib/customEditor/common/customTextEditorNavigation.ts`: o contrato
//! `ICustomTextEditorNavigation`, a navegação de um editor de texto personalizado (revelar um intervalo, capturar e
//! restaurar o estado de visão).
//!
//! Divergências: `ITextModel` é o [`TextModel`]; `IRange` e `ISelection` são o [`Range`] e o [`Selection`];
//! `readonly model`, `selection`, `onDidChangeSelection` e `onDidDispose` são métodos de leitura; `revealRange` devolve
//! uma [`Promise`].

use std::rc::Rc;

use je_base::cancellation::CancellationToken;
use je_base::event::Event;
use je_base::lifecycle::{DisposableRef, IDisposable};
use je_base::promise::Promise;
use je_editor::range::Range;
use je_editor::selection::Selection;
use je_editor::text_model::TextModel;

/// `ICustomTextEditorNavigation`.
pub trait ICustomTextEditorNavigation: IDisposable {
    fn model(&self) -> Rc<TextModel>;
    fn selection(&self) -> Option<Selection>;
    fn on_did_change_selection(&self) -> Event<()>;
    fn on_did_dispose(&self) -> Event<()>;
    fn reveal_range(&self, range: Range, selection: Option<Selection>, preserve_focus: bool, token: CancellationToken) -> Promise<()>;
    fn capture_view_state(&self) -> DisposableRef;
}
