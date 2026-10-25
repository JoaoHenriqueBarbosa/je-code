//! Utilitários do editor de diff (`src/vs/editor/browser/widget/diffEditor/utils.ts`) que os view models e os componentes usam:
//! `filterWithPrevious` e `applyObservableDecorations`. O `RefCounted` do mesmo arquivo vive em `je_base::lifecycle` ao lado do
//! `RefCountedDisposable`. O restante do arquivo (observáveis de DOM, `ObservableElementSizeObserver`, `animatedObservable`...)
//! pertence ao widget de diff e entra com ele.

use std::rc::Rc;

use je_base::lifecycle::{DisposableRef, DisposableStore, to_disposable};
use je_base::observable::{AnyObservable, DebugNameOptions, IObservable, IReader, Observable, autorun_opts};

use crate::code_editor_widget::CodeEditorWidget;
use crate::model::ModelDeltaDecoration;

/// `filterWithPrevious(arr, filter)`: filtra os elementos dando a cada um o elemento anterior da lista original (e não o anterior
/// que passou no filtro).
pub fn filter_with_previous<T: Clone>(arr: &[T], mut filter: impl FnMut(&T, Option<&T>) -> bool) -> Vec<T> {
    let mut prev: Option<&T> = None;
    arr.iter()
        .filter(|cur| {
            let result = filter(cur, prev);
            prev = Some(*cur);
            result
        })
        .cloned()
        .collect()
}

/// `applyObservableDecorations(editor, decorations)`: mantém as decorações do editor iguais ao valor do observável e as remove ao
/// descartar. A lista é um `Rc` porque o `===` do original compara a identidade do array.
pub fn apply_observable_decorations(editor: &Rc<CodeEditorWidget>, decorations: IObservable<Rc<Vec<ModelDeltaDecoration>>>) -> DisposableRef {
    let d = DisposableStore::new();
    let decorations_collection = Rc::new(editor.create_decorations_collection(None));
    let debug_name = format!("Apply decorations from {}", decorations.debug_name());
    {
        let decorations_collection = decorations_collection.clone();
        d.add(autorun_opts(DebugNameOptions::named(&debug_name), move |reader| {
            let reader: &dyn IReader = reader;
            let d = decorations.read(reader);
            decorations_collection.set(&d);
        }));
    }
    d.add(to_disposable(move || decorations_collection.clear()));
    d
}
