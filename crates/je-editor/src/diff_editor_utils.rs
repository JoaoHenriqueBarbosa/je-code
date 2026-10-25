//! Utilitários do editor de diff (`src/vs/editor/browser/widget/diffEditor/utils.ts`) que os view models usam: `filterWithPrevious`.
//! O `RefCounted` do mesmo arquivo vive em `je_base::lifecycle` ao lado do `RefCountedDisposable`. O restante do arquivo
//! (observáveis de DOM, `ObservableElementSizeObserver`, `animatedObservable`...) pertence ao widget de diff e entra com ele.

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
