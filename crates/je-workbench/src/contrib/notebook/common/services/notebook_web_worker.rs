//! Porte de `src/vs/workbench/contrib/notebook/common/services/notebookWebWorker.ts`: o `NotebookWorker`, que espelha os
//! notebooks abertos (`$acceptNewModel`, `$acceptModelChanged`, `$acceptCellModelChanged`, `$acceptRemovedModel`) e
//! calcula a diferença entre dois deles (`$computeDiff`), além de `$canPromptRecommendation`.
//!
//! Divergências:
//! - O worker roda no mesmo thread (como o `EditorWorker` do porte): os métodos `$...` viram métodos comuns em
//!   `snake_case` sem o `$`. `$computeDiff` (`async` sem nenhum `await` no corpo) devolve o resultado direto, e as URLs
//!   de modelo são `&str`. `IWebWorkerServerRequestHandler` (a marca do protocolo de IPC) e o `_requestHandlerBrand`
//!   não existem; [`create`] devolve o [`NotebookWorker`].
//! - `MirrorCell`, `MirrorNotebookDocument` e as células são compartilhadas por `Rc`, e os campos que o original muta
//!   (`language`, `outputs`, `metadata`, `internalMetadata`, `cells`) ficam em `RefCell`: o diff troca o
//!   `internalMetadata` das células depois de a fachada (`NotebookTextModelFacade`) já as ter lido, e o hash em cache
//!   (`_hash`) não é invalidado por isso, como no original (só `onEvents` o apaga).
//! - `CellDiffInfo` e `ICell` do diff são os de [`crate::contrib::notebook::common::notebook_diff`] (o original os
//!   repete por cópia); a fachada é `NotebookCellTextModelFacade` implementando o `ICell` de lá, e o `ICell` do
//!   casamento por similaridade é implementado pelo [`MirrorCell`].
//! - `metadata?.id as string || ''`: só um `id` de texto conta; um `id` de outro tipo (número, objeto) vale `''`.
//! - `acceptModelChanged` com um evento de `Output`, `ChangeCellLanguage` etc. sobre um índice fora da lista lança
//!   (`_assertIndex` ou o acesso a `undefined`); o porte lança com a mesma mensagem do `_assertIndex`.
//! - `splices.reverse()` do original inverte o array do evento no lugar; o porte inverte uma cópia (a leitura do
//!   evento não muda o valor que o chamador tem).
//! - `Array.prototype.slice(start, -1)` (o `findIndex` sem resultado) é [`js_slice`].

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use je_base::JsString;
use je_base::diff::{DiffChange, DiffResult, ISequence, LcsDiff, SequenceElements};
use je_base::dynamic::{Dynamic, json_stringify};
use je_base::errors::{self, Error};
use je_base::hash::{do_hash, hash, number_hash};
use je_base::objects::filter;
use je_base::uri::URI;
use je_base::uuid::generate_uuid;
use je_editor::editor_web_worker::MirrorModel;
use je_editor::model::{DefaultEndOfLine, Eol};
use je_editor::piece_tree_text_buffer_builder::PieceTreeTextBufferBuilder;
use je_editor::range::Range;
use je_editor::text_model_events::ModelContentChangedEvent;
use je_editor::text_model_search::SearchParams;

use crate::contrib::notebook::common::notebook_common::{
    CellKind, IMainCellDto, INotebookDiffResult, IOutputDto, NotebookCellInternalMetadata, NotebookCellMetadata, NotebookCellsChangedEventDto,
    NotebookDocumentMetadata, NotebookRawContentChange, NotebookCellTextModelSplice, TransientDocumentMetadata,
};
use crate::contrib::notebook::common::notebook_diff::{CellDiffInfo, ICell as DiffCell, compute_diff};
use crate::contrib::notebook::common::services::notebook_cell_matching::{CellMatch, ICell as MatchCell, match_cell_based_on_similarties};

const PREFIX_FOR_UNMATCHED_ORIGINAL_CELLS: &str = "unmatchedOriginalCell";

/// `MirrorCell`.
pub struct MirrorCell {
    pub handle: i64,
    text_model: RefCell<MirrorModel>,
    hash: Cell<Option<i32>>,
    eol: Eol,
    pub language: RefCell<String>,
    pub cell_kind: CellKind,
    pub outputs: RefCell<Vec<IOutputDto>>,
    pub metadata: RefCell<Option<NotebookCellMetadata>>,
    pub internal_metadata: RefCell<Option<NotebookCellInternalMetadata>>,
}

impl MirrorCell {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        handle: i64,
        uri: URI,
        source: &[String],
        eol: &str,
        version_id: i64,
        language: &str,
        cell_kind: CellKind,
        outputs: Vec<IOutputDto>,
        metadata: Option<NotebookCellMetadata>,
        internal_metadata: Option<NotebookCellInternalMetadata>,
    ) -> Self {
        let eol = if eol == "\r\n" { Eol::CrLf } else { Eol::Lf };
        let lines = source.iter().map(|line| JsString::from(line.as_str())).collect();
        Self {
            handle,
            text_model: RefCell::new(MirrorModel::new(uri, lines, eol, version_id)),
            hash: Cell::new(None),
            eol,
            language: RefCell::new(language.to_owned()),
            cell_kind,
            outputs: RefCell::new(outputs),
            metadata: RefCell::new(metadata),
            internal_metadata: RefCell::new(internal_metadata),
        }
    }

    /// `eol`.
    pub fn eol(&self) -> DefaultEndOfLine {
        if self.eol == Eol::CrLf { DefaultEndOfLine::CrLf } else { DefaultEndOfLine::Lf }
    }

    pub fn on_events(&self, e: &ModelContentChangedEvent) {
        self.text_model.borrow_mut().on_events(e);
        self.hash.set(None);
    }

    pub fn get_value(&self) -> JsString {
        self.text_model.borrow().get_value()
    }

    pub fn get_lines_content(&self) -> Vec<JsString> {
        self.text_model.borrow().get_lines_content()
    }

    /// `internalMetadata?.internalId`.
    fn internal_id(&self) -> Option<String> {
        self.internal_metadata.borrow().as_ref().and_then(|metadata| metadata.internal_id.clone())
    }

    /// `metadata?.id as string || ''`.
    fn metadata_id(&self) -> String {
        match self.metadata.borrow().as_ref().map(|metadata| metadata.get_str("id")) {
            Some(Dynamic::String(id)) => id.to_string_lossy(),
            _ => String::new(),
        }
    }

    /// `getComparisonValue()`: o hash fica em cache até o próximo `onEvents`.
    pub fn get_comparison_value(&self) -> i32 {
        if let Some(hash) = self.hash.get() {
            return hash;
        }
        let hash = self.compute_hash();
        self.hash.set(Some(hash));
        hash
    }

    fn compute_hash(&self) -> i32 {
        let mut hash_value = number_hash(104_579.0, 0);

        hash_value = do_hash(&Dynamic::string(&self.language.borrow()), hash_value);
        hash_value = do_hash(&Dynamic::from(self.get_value()), hash_value);
        hash_value = do_hash(&self.metadata.borrow().clone().unwrap_or(Dynamic::Undefined), hash_value);
        // For purpose of diffing only cellId matters, rest do not
        hash_value = do_hash(&Dynamic::string(&self.internal_id().unwrap_or_default()), hash_value);
        let outputs = self.outputs.borrow();
        for op in outputs.iter() {
            hash_value = do_hash(&op.metadata.clone().unwrap_or(Dynamic::Undefined), hash_value);
            for output in &op.outputs {
                hash_value = do_hash(&Dynamic::string(&output.mime), hash_value);
            }
        }

        let digests: Vec<i32> = outputs
            .iter()
            .flat_map(|op| op.outputs.iter())
            .map(|o| hash(&Dynamic::array(o.data.with_bytes(|bytes| bytes.iter().map(|&byte| Dynamic::from(f64::from(byte))).collect::<Vec<_>>()))))
            .collect();
        for digest in digests {
            hash_value = number_hash(f64::from(digest), hash_value);
        }

        hash_value
    }
}

impl MatchCell for MirrorCell {
    fn internal_id(&self) -> Option<String> {
        MirrorCell::internal_id(self)
    }

    fn get_value(&self) -> JsString {
        MirrorCell::get_value(self)
    }

    fn get_lines_content(&self) -> Vec<JsString> {
        MirrorCell::get_lines_content(self)
    }

    fn cell_kind(&self) -> CellKind {
        self.cell_kind
    }
}

/// `NotebookCellTextModelFacade`.
struct NotebookCellTextModelFacade {
    cell: Rc<MirrorCell>,
}

impl DiffCell for NotebookCellTextModelFacade {
    fn cell_kind(&self) -> CellKind {
        self.cell.cell_kind
    }

    fn get_hash_value(&self) -> i32 {
        self.cell.get_comparison_value()
    }

    fn equal(&self, cell: &dyn DiffCell) -> bool {
        if cell.cell_kind() != self.cell_kind() {
            return false;
        }
        self.get_hash_value() == cell.get_hash_value()
    }
}

/// `NotebookTextModelFacade.cells`.
fn facade_cells(notebook: &MirrorNotebookDocument) -> Vec<Rc<dyn DiffCell>> {
    notebook.cells.borrow().iter().map(|cell| Rc::new(NotebookCellTextModelFacade { cell: cell.clone() }) as Rc<dyn DiffCell>).collect()
}

/// `MirrorNotebookDocument`.
pub struct MirrorNotebookDocument {
    pub uri: URI,
    pub cells: RefCell<Vec<Rc<MirrorCell>>>,
    pub metadata: RefCell<NotebookDocumentMetadata>,
    pub transient_document_metadata: TransientDocumentMetadata,
}

impl MirrorNotebookDocument {
    pub fn new(uri: URI, cells: Vec<Rc<MirrorCell>>, metadata: NotebookDocumentMetadata, transient_document_metadata: TransientDocumentMetadata) -> Self {
        Self { uri, cells: RefCell::new(cells), metadata: RefCell::new(metadata), transient_document_metadata }
    }

    pub fn accept_model_changed(&self, event: &NotebookCellsChangedEventDto) {
        // note that the cell content change is not applied to the MirrorCell
        // but it's fine as if a cell content is modified after the first diff, its position will not change any more
        // TODO@rebornix, but it might lead to interesting bugs in the future.
        for e in &event.raw_events {
            match e {
                NotebookRawContentChange::ModelChange(e) => self.splice_notebook_cells(&e.changes),
                NotebookRawContentChange::Move(e) => {
                    let mut cells = self.cells.borrow_mut();
                    let moved: Vec<Rc<MirrorCell>> = if e.index < cells.len() { vec![cells.remove(e.index)] } else { Vec::new() };
                    let at = e.new_idx.min(cells.len());
                    drop(cells.splice(at..at, moved));
                }
                NotebookRawContentChange::Output(e) => {
                    let cell = self.cells.borrow()[e.index].clone();
                    *cell.outputs.borrow_mut() = e.outputs.clone();
                }
                NotebookRawContentChange::ChangeCellLanguage(e) => {
                    self.assert_index(e.index);
                    let cell = self.cells.borrow()[e.index].clone();
                    *cell.language.borrow_mut() = e.language.clone();
                }
                NotebookRawContentChange::ChangeCellMetadata(e) => {
                    self.assert_index(e.index);
                    let cell = self.cells.borrow()[e.index].clone();
                    *cell.metadata.borrow_mut() = Some(e.metadata.clone());
                }
                NotebookRawContentChange::ChangeCellInternalMetadata(e) => {
                    self.assert_index(e.index);
                    let cell = self.cells.borrow()[e.index].clone();
                    *cell.internal_metadata.borrow_mut() = Some(e.internal_metadata.clone());
                }
                NotebookRawContentChange::ChangeDocumentMetadata(e) => {
                    *self.metadata.borrow_mut() = e.metadata.clone();
                }
                _ => {}
            }
        }
    }

    fn assert_index(&self, index: usize) {
        let len = self.cells.borrow().len();
        if index >= len {
            errors::throw(Error::new(format!("Illegal index {index}. Cells length: {len}")));
        }
    }

    pub fn splice_notebook_cells(&self, splices: &[NotebookCellTextModelSplice<IMainCellDto>]) {
        for splice in splices.iter().rev() {
            let cell_dtos = &splice.2;
            let new_cells: Vec<Rc<MirrorCell>> = cell_dtos
                .iter()
                .map(|cell| {
                    Rc::new(MirrorCell::new(
                        cell.handle,
                        URI::parse(cell.url.as_str(), false),
                        &cell.source,
                        &cell.eol,
                        cell.version_id,
                        &cell.language,
                        cell.cell_kind,
                        cell.outputs.clone(),
                        cell.metadata.clone(),
                        None,
                    ))
                })
                .collect();

            let mut cells = self.cells.borrow_mut();
            let start = splice.0.min(cells.len());
            let end = (start + splice.1).min(cells.len());
            drop(cells.splice(start..end, new_cells));
        }
    }
}

/// `CellSequence`.
struct CellSequence {
    hash_value: SequenceElements,
}

impl CellSequence {
    fn create(text_model: &MirrorNotebookDocument) -> Self {
        let hash_value = text_model.cells.borrow().iter().map(|c| c.get_comparison_value()).collect();
        Self { hash_value: SequenceElements::Ints(hash_value) }
    }

    fn create_with_cell_id(cells: &[Rc<MirrorCell>], include_cell_contents: bool) -> Self {
        let hash_value = cells
            .iter()
            .map(|c| {
                let id_hash = do_hash(&c.internal_id().map_or(Dynamic::Undefined, |id| Dynamic::string(&id)), number_hash(104_579.0, 0));
                if include_cell_contents {
                    JsString::from(format!("{id_hash}#{}", c.get_comparison_value()))
                } else {
                    JsString::from(format!("{id_hash}}}"))
                }
            })
            .collect();
        Self { hash_value: SequenceElements::Strings(hash_value) }
    }
}

impl ISequence for CellSequence {
    fn get_elements(&self) -> SequenceElements {
        match &self.hash_value {
            SequenceElements::Ints(values) => SequenceElements::Ints(values.clone()),
            SequenceElements::Strings(values) => SequenceElements::Strings(values.clone()),
        }
    }
}

/// `Array.prototype.slice(start, end)` com o `end` que pode ser `-1` (o `findIndex` sem resultado): negativo conta do fim.
fn js_slice<T>(items: &[T], start: i64, end: i64) -> &[T] {
    let len = items.len() as i64;
    let resolve = |index: i64| if index < 0 { (len + index).max(0) } else { index.min(len) };
    let (start, end) = (resolve(start), resolve(end));
    if start >= end { &[] } else { &items[start as usize..end as usize] }
}

/// O bloco que o `$computeDiff` repete: as células não inseridas nem removidas antes de uma mudança, que mudaram de
/// conteúdo (`modified`, ou o hash diferente) viram uma mudança de uma célula.
fn push_changed_cells(infos: &[CellDiffInfo], original: &MirrorNotebookDocument, modified: &MirrorNotebookDocument, changes: &mut Vec<DiffChange>) {
    for c in infos {
        let (original_cell_index, modified_cell_index, is_modified) = match *c {
            CellDiffInfo::Unchanged { original_cell_index, modified_cell_index } => (original_cell_index, modified_cell_index, false),
            CellDiffInfo::Modified { original_cell_index, modified_cell_index } => (original_cell_index, modified_cell_index, true),
            CellDiffInfo::Delete { .. } | CellDiffInfo::Insert { .. } => continue,
        };
        let original_cell = original.cells.borrow()[original_cell_index].clone();
        let modified_cell = modified.cells.borrow()[modified_cell_index].clone();
        let changed = is_modified || original_cell.get_comparison_value() != modified_cell.get_comparison_value();
        if changed {
            changes.push(DiffChange::new(original_cell_index as i32, 1, modified_cell_index as i32, 1));
        }
    }
}

/// `NotebookWorker`.
#[derive(Default)]
pub struct NotebookWorker {
    models: RefCell<HashMap<String, Rc<MirrorNotebookDocument>>>,
}

impl NotebookWorker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn dispose(&self) {}

    pub fn accept_new_model(&self, uri: &str, metadata: NotebookDocumentMetadata, transient_document_metadata: TransientDocumentMetadata, cells: Vec<IMainCellDto>) {
        let cells = cells
            .into_iter()
            .map(|dto| {
                Rc::new(MirrorCell::new(
                    dto.handle,
                    URI::parse(dto.url.as_str(), false),
                    &dto.source,
                    &dto.eol,
                    dto.version_id,
                    &dto.language,
                    dto.cell_kind,
                    dto.outputs,
                    dto.metadata,
                    dto.internal_metadata,
                ))
            })
            .collect();
        self.models.borrow_mut().insert(uri.to_owned(), Rc::new(MirrorNotebookDocument::new(URI::parse(uri, false), cells, metadata, transient_document_metadata)));
    }

    pub fn accept_model_changed(&self, str_url: &str, event: &NotebookCellsChangedEventDto) {
        let model = self.models.borrow().get(str_url).cloned();
        if let Some(model) = model {
            model.accept_model_changed(event);
        }
    }

    pub fn accept_cell_model_changed(&self, str_url: &str, handle: i64, event: &ModelContentChangedEvent) {
        let model = self.get_model(str_url);
        let cell = model.cells.borrow().iter().find(|cell| cell.handle == handle).cloned();
        if let Some(cell) = cell {
            cell.on_events(event);
        }
    }

    pub fn accept_removed_model(&self, str_url: &str) {
        self.models.borrow_mut().remove(str_url);
    }

    pub fn compute_diff(&self, original_url: &str, modified_url: &str) -> INotebookDiffResult {
        let original = self.get_model(original_url);
        let modified = self.get_model(modified_url);

        let original_model = facade_cells(&original);
        let modified_model = facade_cells(&modified);

        let non_transient = |notebook: &MirrorNotebookDocument| {
            filter(&notebook.metadata.borrow(), |key, _| !notebook.transient_document_metadata.get(&key.to_string_lossy()).copied().unwrap_or(false))
        };
        let original_metadata = non_transient(&original);
        let modified_metadata = non_transient(&modified);
        let metadata_changed = json_stringify(&original_metadata) != json_stringify(&modified_metadata);
        // TODO@DonJayamanne
        // In the future we might want to avoid computing LCS of outputs
        // That will make this faster.
        let original_diff = LcsDiff::new(Rc::new(CellSequence::create(&original)), Rc::new(CellSequence::create(&modified)), None).compute_diff(false);
        if original_diff.changes.is_empty() {
            return INotebookDiffResult { metadata_changed, cells_diff: original_diff };
        }

        // This will return the mapping of the cells and what cells were inserted/deleted.
        // We do not care much about accuracy of the diff, but care about the mapping of unmodified cells.
        // That can be used as anchor points to find the cells that have changed.
        // And on cells that have changed, we can use similarity algorithms to find the mapping.
        // Eg as mentioned earlier, its possible after similarity algorithms we find that cells weren't inserted/deleted but were just modified.
        let cell_mapping = compute_diff(
            &original_model,
            &modified_model,
            &INotebookDiffResult { cells_diff: DiffResult { quit_early: false, changes: original_diff.changes.clone() }, metadata_changed: false },
        )
        .cell_diff_info;

        // If we have no insertions/deletions, then this is a good diffing.
        if cell_mapping.iter().all(|c| matches!(c, CellDiffInfo::Modified { .. } | CellDiffInfo::Unchanged { .. })) {
            return INotebookDiffResult { metadata_changed, cells_diff: original_diff };
        }

        let mut diff_using_cell_ids = self.can_compute_diff_with_cell_ids(&original, &modified);
        if !diff_using_cell_ids {
            // Assume we have cells as follows
            // Original   Modified
            // A	  		A
            // B			B
            // C			e
            // D			F
            // E
            // F
            //
            // Using LCS we know easily that A, B cells match.
            // Using LCS it would look like C changed to e
            // Using LCS D & E were removed.
            //
            // A human would be able to tell that cell C, D were removed.
            // A human can tell that E changed to e because the code in the cells are very similar.
            // Note the words `similar`, humans try to match cells based on certain heuristics.
            // & the most obvious one is the similarity of the code in the cells.
            //
            // LCS has no notion of similarity, it only knows about equality.
            // We can use other algorithms to find similarity.
            // So if we eliminate A, B, we are left with C, D, E, F and we need to find what they map to in `e, F` in modifed document.
            // We can use a similarity algorithm to find that.
            //
            // The purpose of using LCS first is to find the cells that have not changed.
            // This avoids the need to use similarity algorithms on all cells.
            //
            // At the end of the day what we need is as follows
            // A <=> A
            // B <=> B
            // C => Deleted
            // D => Deleted
            // E => e
            // F => F

            // Note, if cells are swapped, then this compilicates things
            // Trying to solve diff manually is not easy.
            // Lets instead use LCS find the cells that haven't changed,
            // & the cells that have.
            // For the range of cells that have change, lets see if we can get better results using similarity algorithms.
            // Assume we have
            // Code Cell = print("Hello World")
            // Code Cell = print("Foo Bar")
            // We now change this to
            // MD Cell = # Description
            // Code Cell = print("Hello WorldZ")
            // Code Cell = print("Foo BarZ")
            // LCS will tell us that everything changed.
            // But using similarity algorithms we can tell that the first cell is new and last 2 changed.

            // Lets try the similarity algorithms on all cells.
            // We might fare better.
            let as_match_cells = |notebook: &MirrorNotebookDocument| -> Vec<Rc<dyn MatchCell>> {
                notebook.cells.borrow().iter().map(|cell| cell.clone() as Rc<dyn MatchCell>).collect()
            };
            let result = match_cell_based_on_similarties(&as_match_cells(&modified), &as_match_cells(&original));
            // If we have at least one match, then great.
            if result.iter().any(|c| c.original != -1) {
                // We have managed to find similarities between cells.
                // Now we can definitely find what cell is new/removed.
                self.update_cell_ids_based_on_mappings(&result, &original.cells.borrow(), &modified.cells.borrow());
                diff_using_cell_ids = true;
            }
        }

        if !diff_using_cell_ids {
            return INotebookDiffResult { metadata_changed, cells_diff: original_diff };
        }

        // At this stage we can use internalMetadata.cellId for tracking changes.
        // I.e. we compute LCS diff and the hashes of some cells from original will be equal to that in modified as we're using cellId.
        // Thus we can find what cells are new/deleted.
        // After that we can find whether the contents of the cells changed.
        let cells_inserted_or_deleted_diff = LcsDiff::new(
            Rc::new(CellSequence::create_with_cell_id(&original.cells.borrow(), false)),
            Rc::new(CellSequence::create_with_cell_id(&modified.cells.borrow(), false)),
            None,
        )
        .compute_diff(false);
        let cell_diff_info = compute_diff(
            &original_model,
            &modified_model,
            &INotebookDiffResult { cells_diff: DiffResult { quit_early: false, changes: cells_inserted_or_deleted_diff.changes.clone() }, metadata_changed: false },
        )
        .cell_diff_info;

        let mut processed_index: i64 = 0;
        let mut changes: Vec<DiffChange> = Vec::new();
        let find_index = |predicate: &dyn Fn(&CellDiffInfo) -> bool| cell_diff_info.iter().position(predicate).map_or(-1, |index| index as i64);
        for change in &cells_inserted_or_deleted_diff.changes {
            let change_index = if change.original_length == 0 && change.modified_length != 0 {
                // Inserted.
                // Find all modified cells before this.
                find_index(&|c| matches!(c, CellDiffInfo::Insert { modified_cell_index } if *modified_cell_index as i32 == change.modified_start))
            } else if change.original_length != 0 && change.modified_length == 0 {
                // Deleted.
                // Find all modified cells before this.
                find_index(&|c| matches!(c, CellDiffInfo::Delete { original_cell_index } if *original_cell_index as i32 == change.original_start))
            } else {
                // This could be a situation where a cell has been deleted on left and inserted on the right.
                // E.g. markdown cell deleted and code cell inserted.
                // But LCS shows them as a modification.
                find_index(&|c| match c {
                    CellDiffInfo::Delete { original_cell_index } => *original_cell_index as i32 == change.original_start,
                    CellDiffInfo::Insert { modified_cell_index } => *modified_cell_index as i32 == change.modified_start,
                    _ => false,
                })
            };
            push_changed_cells(js_slice(&cell_diff_info, processed_index, change_index), &original, &modified, &mut changes);
            changes.push(*change);
            processed_index = change_index + 1;
        }
        push_changed_cells(js_slice(&cell_diff_info, processed_index, cell_diff_info.len() as i64), &original, &modified, &mut changes);

        INotebookDiffResult { metadata_changed, cells_diff: DiffResult { changes, quit_early: false } }
    }

    pub fn can_compute_diff_with_cell_ids(&self, original: &MirrorNotebookDocument, modified: &MirrorNotebookDocument) -> bool {
        self.can_compute_diff_with_cell_internal_ids(original, modified) || self.can_compute_diff_with_cell_metadata_ids(original, modified)
    }

    pub fn can_compute_diff_with_cell_internal_ids(&self, original: &MirrorNotebookDocument, modified: &MirrorNotebookDocument) -> bool {
        let ids = |notebook: &MirrorNotebookDocument| -> Vec<String> { notebook.cells.borrow().iter().map(|cell| cell.internal_id().unwrap_or_default()).collect() };
        let original_cell_index_ids = ids(original);
        let modified_cell_index_ids = ids(modified);
        // If we have a cell without an id, do not use metadata.id for diffing.
        if original_cell_index_ids.iter().any(String::is_empty) || modified_cell_index_ids.iter().any(String::is_empty) {
            return false;
        }
        // If none of the ids in original can be found in modified, then we can't use metadata.id for diffing.
        // I.e. everything is new, no point trying.
        original_cell_index_ids.iter().any(|c| modified_cell_index_ids.iter().any(|m| m == c))
    }

    pub fn can_compute_diff_with_cell_metadata_ids(&self, original: &MirrorNotebookDocument, modified: &MirrorNotebookDocument) -> bool {
        let ids = |notebook: &MirrorNotebookDocument| -> Vec<String> { notebook.cells.borrow().iter().map(|cell| cell.metadata_id()).collect() };
        let original_cell_index_ids = ids(original);
        let modified_cell_index_ids = ids(modified);
        // If we have a cell without an id, do not use metadata.id for diffing.
        if original_cell_index_ids.iter().any(String::is_empty) || modified_cell_index_ids.iter().any(String::is_empty) {
            return false;
        }
        // If none of the ids in original can be found in modified, then we can't use metadata.id for diffing.
        // I.e. everything is new, no point trying.
        if original_cell_index_ids.iter().all(|c| !modified_cell_index_ids.iter().any(|m| m == c)) {
            return false;
        }

        // Internally we use internalMetadata.cellId for diffing, hence update the internalMetadata.cellId
        for notebook in [original, modified] {
            for cell in notebook.cells.borrow().iter() {
                let id = cell.metadata_id();
                cell.internal_metadata.borrow_mut().get_or_insert_with(NotebookCellInternalMetadata::default).internal_id = Some(id);
            }
        }
        true
    }

    pub fn is_original_cell_matched_with_modified_cell(&self, original_cell: &MirrorCell) -> bool {
        original_cell.internal_id().unwrap_or_default().starts_with(PREFIX_FOR_UNMATCHED_ORIGINAL_CELLS)
    }

    pub fn update_cell_ids_based_on_mappings(&self, mappings: &[CellMatch], original_cells: &[Rc<MirrorCell>], modified_cells: &[Rc<MirrorCell>]) -> bool {
        let mut uuids: HashMap<i64, String> = HashMap::new();
        for (index, cell) in original_cells.iter().enumerate() {
            let mut internal_metadata = cell.internal_metadata.borrow_mut();
            let internal_metadata = internal_metadata.get_or_insert_with(|| NotebookCellInternalMetadata { internal_id: Some(String::new()), ..Default::default() });
            internal_metadata.internal_id = Some(format!("{PREFIX_FOR_UNMATCHED_ORIGINAL_CELLS}{}", generate_uuid()));
            if let Some(found) = mappings.iter().find(|r| r.original == index as i64) {
                // Do not use the indexes as ids.
                // If we do, then the hashes will be very similar except for last digit.
                let id = generate_uuid();
                internal_metadata.internal_id = Some(id.clone());
                uuids.insert(found.modified, id);
            }
        }
        for (index, cell) in modified_cells.iter().enumerate() {
            let mut internal_metadata = cell.internal_metadata.borrow_mut();
            let internal_metadata = internal_metadata.get_or_insert_with(|| NotebookCellInternalMetadata { internal_id: Some(String::new()), ..Default::default() });
            internal_metadata.internal_id = Some(uuids.get(&(index as i64)).cloned().unwrap_or_else(generate_uuid));
        }
        true
    }

    pub fn can_prompt_recommendation(&self, model_url: &str) -> bool {
        let model = self.get_model(model_url);
        let cells = model.cells.borrow();

        for cell in cells.iter() {
            if cell.cell_kind == CellKind::Markup {
                continue;
            }

            if *cell.language.borrow() != "python" {
                continue;
            }

            let search_params = SearchParams::new(JsString::from("import\\s*pandas|from\\s*pandas"), true, false, None);
            let Some(search_data) = search_params.parse_search_request() else {
                continue;
            };

            let mut builder = PieceTreeTextBufferBuilder::new();
            builder.accept_chunk(cell.get_value().units());
            let buffer_factory = builder.finish(true);
            let text_buffer = buffer_factory.create(cell.eol());

            let line_count = text_buffer.get_line_count();
            let max_line_count = line_count.min(20) as i32;
            let range = Range::new(1, 1, max_line_count, text_buffer.get_line_length(max_line_count) as i32 + 1);
            let cell_matches = text_buffer.find_matches_line_by_line(&range, &search_data, true, 1);
            if !cell_matches.is_empty() {
                return true;
            }
        }

        false
    }

    /// `_getModel(uri)`.
    fn get_model(&self, uri: &str) -> Rc<MirrorNotebookDocument> {
        match self.models.borrow().get(uri) {
            Some(model) => model.clone(),
            None => errors::throw(Error::new(format!("Cannot read properties of undefined: notebook {uri} não foi aceito pelo worker"))),
        }
    }
}

/// `create()`.
pub fn create() -> NotebookWorker {
    NotebookWorker::new()
}
