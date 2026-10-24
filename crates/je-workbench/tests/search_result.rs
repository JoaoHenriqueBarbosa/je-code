//! Porte de `src/vs/workbench/contrib/search/test/browser/searchResult.test.ts` (suíte `SearchResult`), caso a caso e na mesma ordem.
//!
//! Divergências:
//! - **`setup` e `teardown`.** O guarda de vazamentos (`ensureNoDisposablesAreLeakedInTestSuite`) é o [`LeakGuard`] criado no
//!   começo de cada caso. O `TestInstantiationService` do original é o [`InstantiationService`] sobre um [`ServiceCollection`] com os
//!   mesmos serviços: telemetria nula, `ModelService` real (com `search.searchOnType` ligado), serviço de editores de
//!   notebook, `UriIdentityService` real sobre um `FileService`, `IReplaceService` (dublê), `MockLabelService` e log nulo. O
//!   original deixa `ISearchService` e `INotebookSearchService` sem registro (`undefined`); aqui eles são dublês que
//!   falham se usados (nenhum caso os usa), porque o contrato do porte os exige. O `IThemeService` do `stubModelService` não
//!   existe no `ModelService` do porte, e o `NotebookEditorWidgetService` real (que exige os grupos de editores, o contexto e
//!   o serviço de editores) é um dublê de `INotebookEditorService` que não tem editores abertos, que é o que o real devolve
//!   sem editores.
//! - **`sinon`.** `sinon.spy()` de ouvinte vira um ouvinte que registra os eventos ou conta as chamadas. `instantiationService.stub(IReplaceService, 'replace', ...)`
//!   troca o comportamento do [`TestReplaceService`] (`stub_replace`), cujo padrão é `Promise.resolve(null)`.
//!   `sinon.spy(folderMatch, 'dispose')` é [`ISearchTreeFolderMatch::dispose_count_for_testing`]. `sinon.stub(CellMatch.prototype,
//!   'addContext')` é a célula falsa cujo `resolveTextModel()` nunca resolve (o mesmo efeito: o contexto não é preenchido), e
//!   `sinon.spy(FolderMatchImpl.prototype, 'addFileMatch')` (cujos argumentos o original compara com os da entrada) é a conferência
//!   do estado que o `addFileMatch` produziu: os casamentos de cada célula têm o texto e o intervalo dos resultados de entrada.
//! - **`{ cellKind: CellKind.Code } as ICellViewModel`.** É o [`FakeCellViewModel`]; o `id` indefinido do original (que faz o
//!   `CellMatch.id` cair em `rawCell#<índice>`) é o texto `rawCell#0`.
//! - **`Match -> FileMatch -> SearchResult hierarchy exists`.** O `SearchResultImpl` criado ali não tem consulta, então
//!   `folderMatches()[0]` é `undefined` no original e o arquivo nasce com pai `undefined` (e o `assert` compara `undefined ===
//!   undefined`). O arquivo do porte exige a pasta, então o caso define a consulta do resultado (a mesma de `aSearchResult`) e compara
//!   o pai com a pasta raiz de verdade.
//! - **`deepStrictEqual`** de eventos e de elementos da árvore compara os campos (`Option` ausente é diferente de `false`) e a
//!   identidade dos elementos (o `deepStrictEqual` sobre o mesmo objeto).
//! - **Casos `async`** esvaziam as microtarefas (`runtime::run_microtasks`) e conferem que a promessa não foi rejeitada; os que
//!   retornam `voidPromise.then(...)` conferem o estado depois das microtarefas.
//! - **`isWindows`.** `getRootName` e `createFileUriFromPathFromRoot` seguem a plataforma.

// O `setup`, os dublês dos serviços e os `aSearchResult`, `aFileMatch` e `aFolderMatch` estão em `support/search_support.rs`
// (a suíte `Search - Viewlet` os reaproveita).
mod support;

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::js_string::JsString;
use je_base::lifecycle::id_of;
use je_base::promise::Promise;
use je_base::runtime;
use je_editor::range::Range;
use je_platform::resources::uri_file;
use je_workbench::contrib::notebook::browser::notebook_browser::{CellEditState, ICellViewModel};
use je_workbench::contrib::notebook::common::notebook_common::{CellKind, TextBufferRef};
use je_workbench::contrib::search::notebook_search::notebook_search_model_base::INotebookFileInstanceMatch;
use je_workbench::contrib::search::notebook_search::search_notebook_helpers::{INotebookCellMatch, INotebookCellMatchWithModel};
use je_workbench::contrib::search::search_tree_model::r#match::MatchImpl;
use je_workbench::contrib::search::search_tree_model::search_model::SearchModelImpl;
use je_workbench::contrib::search::search_tree_model::search_result::SearchResultImpl;
use je_workbench::contrib::search::search_tree_model::search_tree_common::{
    FileOrFolderMatch, FolderMatchParent, IChangeEvent, ISearchModel, ISearchResult, ISearchTreeFileMatch, ISearchTreeFolderMatch, ISearchTreeMatch,
    MATCH_PREFIX, RenderableMatch, is_search_tree_folder_match_no_root,
};
use je_workbench::services::search::search::{IFileMatch, IFolderQuery, ITextQuery, ITextQueryProps, OneLineRange, PatternInfo, SearchRange, TextSearchMatch};
use support::search_support::{
    Fixture, LeakGuard, ReplaceCall, a_raw_match, create_file_uri_from_path_from_root, ensure_no_disposables_are_leaked_in_test_suite, get_root_name, root_query, setup,
    text_match, unused,
};

fn line_one_range() -> SearchRange {
    OneLineRange::new(1, 0, 1)
}

/// `addToSearchResult(searchResult, allRaw, searchInstanceID = '')`.
fn add_to_search_result(search_result: &Rc<dyn ISearchResult>, all_raw: &[IFileMatch]) {
    search_result.add(all_raw, "", false, false);
}

// ---------------------------------------------------------------------------------------------------------
// Dublês dos serviços
// ---------------------------------------------------------------------------------------------------------

/// `{ cellKind: CellKind.Code } as ICellViewModel`: o `id` é o do `CellMatch` sem id e o texto do modelo nunca resolve.
struct FakeCellViewModel {
    cell_kind: CellKind,
}

impl ICellViewModel for FakeCellViewModel {
    fn id(&self) -> String {
        "rawCell#0".to_string()
    }

    fn cell_kind(&self) -> CellKind {
        self.cell_kind
    }

    fn text_buffer(&self) -> TextBufferRef {
        unused("ICellViewModel.textBuffer")
    }

    fn resolve_text_model(&self) -> Promise<Rc<je_editor::text_model::TextModel>> {
        Promise::new().0
    }

    fn get_edit_state(&self) -> CellEditState {
        unused("ICellViewModel.getEditState")
    }

    fn update_edit_state(&self, _new_state: CellEditState, _source: &str) {
        unused("ICellViewModel.updateEditState")
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

// ---------------------------------------------------------------------------------------------------------
// Auxiliares do original
// ---------------------------------------------------------------------------------------------------------

/// `aRawFileMatchWithCells(resource, ...cellMatches)`.
fn a_raw_file_match_with_cells(resource: &str, cell_matches: Vec<INotebookCellMatchWithModel>) -> IFileMatch {
    IFileMatch {
        resource: create_file_uri_from_path_from_root(Some(resource)),
        results: None,
        cell_results: Some(cell_matches.into_iter().map(INotebookCellMatch::WithModel).collect()),
    }
}

/// Uma consulta de texto com as pastas dadas e o padrão `pattern`.
fn text_query(folders: &[&str], pattern: &str) -> ITextQuery {
    ITextQueryProps::new(
        folders.iter().map(|folder| IFolderQuery::new(create_file_uri_from_path_from_root(Some(folder)))).collect(),
        PatternInfo::new(pattern),
    )
}

impl Fixture {
    /// `getPopulatedSearchResult()`.
    fn get_populated_search_result(&self, guard: &LeakGuard) -> Rc<dyn ISearchResult> {
        let test_object = self.a_search_result(guard);

        test_object.set_query(Some(text_query(&["/voo", "/with"], "foo")));

        add_to_search_result(
            &test_object,
            &[
                a_raw_match("/voo/foo.a", vec![text_match("preview 1", line_one_range()), text_match("preview 2", line_one_range())]),
                a_raw_match("/with/path/bar.b", vec![text_match("preview 3", line_one_range())]),
                a_raw_match("/with/path.c", vec![text_match("preview 4", line_one_range()), text_match("preview 5", line_one_range())]),
            ],
        );
        test_object
    }

    /// `getPopulatedSearchResultForTreeTesting()`.
    fn get_populated_search_result_for_tree_testing(&self, guard: &LeakGuard) -> Rc<dyn ISearchResult> {
        let test_object = self.a_search_result(guard);

        test_object.set_query(Some(text_query(&["/voo", "/with", "/with/test", "/eep"], "foo")));
        // file structure looks like:
        // *voo/
        // |- foo.a
        // |- beep
        //    |- foo.c
        //    |- boop.c
        // *with/
        // |- path
        //    |- bar.b
        // |- path.c
        // |- *test/
        //    |- woo.c
        // eep/
        //    |- bar
        //       |- goo
        //           |- foo
        //              |- here.txt
        //           |- ooo
        //              |- there.txt
        //    |- eyy.y

        add_to_search_result(
            &test_object,
            &[
                a_raw_match("/voo/foo.a", vec![text_match("preview 1", line_one_range()), text_match("preview 2", line_one_range())]),
                a_raw_match("/voo/beep/foo.c", vec![text_match("preview 1", line_one_range()), text_match("preview 2", line_one_range())]),
                a_raw_match("/voo/beep/boop.c", vec![text_match("preview 3", line_one_range())]),
                a_raw_match("/with/path.c", vec![text_match("preview 4", line_one_range()), text_match("preview 5", line_one_range())]),
                a_raw_match("/with/path/bar.b", vec![text_match("preview 3", line_one_range())]),
                a_raw_match("/with/test/woo.c", vec![text_match("preview 3", line_one_range())]),
                a_raw_match("/eep/bar/goo/foo/here.txt", vec![text_match("preview 6", line_one_range()), text_match("preview 7", line_one_range())]),
                a_raw_match("/eep/bar/goo/ooo/there.txt", vec![text_match("preview 6", line_one_range()), text_match("preview 7", line_one_range())]),
                a_raw_match("/eep/eyy.y", vec![text_match("preview 6", line_one_range()), text_match("preview 7", line_one_range())]),
            ],
        );
        test_object
    }
}

/// `getFolderMatchAtIndex(parent, index)`.
fn get_folder_match_at_index(parent: &Rc<dyn ISearchTreeFolderMatch>, index: usize) -> Rc<dyn ISearchTreeFolderMatch> {
    parent.folder_matches_iterator()[index].clone()
}

/// `getFileMatchAtIndex(parent, index)`.
fn get_file_match_at_index(parent: &Rc<dyn ISearchTreeFolderMatch>, index: usize) -> Rc<dyn ISearchTreeFileMatch> {
    parent.file_matches_iterator()[index].clone()
}

/// O pai de uma pasta, que nos casos é sempre outra pasta.
fn folder_parent(folder_match: &Rc<dyn ISearchTreeFolderMatch>) -> Rc<dyn ISearchTreeFolderMatch> {
    match folder_match.parent() {
        FolderMatchParent::FolderMatch(parent) => parent,
        FolderMatchParent::Heading(_) => panic!("o pai da pasta é um cabeçalho"),
    }
}

/// O `deepStrictEqual` entre listas de arquivos: os mesmos objetos, na mesma ordem.
fn assert_same_files(actual: &[Rc<dyn ISearchTreeFileMatch>], expected: &[Rc<dyn ISearchTreeFileMatch>]) {
    let actual_ids: Vec<usize> = actual.iter().map(id_of).collect();
    let expected_ids: Vec<usize> = expected.iter().map(id_of).collect();
    assert_eq!(actual_ids, expected_ids);
}

fn assert_same_folder(actual: &Rc<dyn ISearchTreeFolderMatch>, expected: &Rc<dyn ISearchTreeFolderMatch>) {
    assert_eq!(id_of(actual), id_of(expected));
}

fn assert_closest_root(file_match: &Rc<dyn ISearchTreeFileMatch>, root: &Rc<dyn ISearchTreeFolderMatch>) {
    let closest_root = file_match.closest_root().expect("o arquivo tem raiz mais próxima");
    assert_same_folder(&closest_root, root);
}

/// O `deepStrictEqual([{ elements, added?, removed?, clearingAll? }], target.args[0])`.
fn assert_change_event(
    actual: &IChangeEvent,
    elements: &[Rc<dyn ISearchTreeFileMatch>],
    added: Option<bool>,
    removed: Option<bool>,
    clearing_all: Option<bool>,
) {
    assert_same_files(&actual.elements, elements);
    assert_eq!(actual.added, added);
    assert_eq!(actual.removed, removed);
    assert_eq!(actual.clearing_all, clearing_all);
}

/// `const target = sinon.spy(); store.add(testObject.onChange(target))`.
fn spy_on_change(guard: &LeakGuard, test_object: &Rc<dyn ISearchResult>) -> Rc<RefCell<Vec<IChangeEvent>>> {
    let events: Rc<RefCell<Vec<IChangeEvent>>> = Rc::new(RefCell::new(Vec::new()));
    let sink = events.clone();
    guard.add(test_object.on_change().listen(move |event| sink.borrow_mut().push(event.clone())));
    events
}

fn matches_of(test_object: &Rc<dyn ISearchResult>) -> Vec<Rc<dyn ISearchTreeFileMatch>> {
    test_object.matches(None)
}

fn file_match_dyn(file_match: &Rc<dyn INotebookFileInstanceMatch>) -> Rc<dyn ISearchTreeFileMatch> {
    file_match.clone()
}

// ---------------------------------------------------------------------------------------------------------
// Os casos
// ---------------------------------------------------------------------------------------------------------

#[test]
fn line_match() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let t = setup(&guard);
    let file_match = t.a_file_match(&guard, "folder/file.txt", None, Vec::new());
    let line_match = MatchImpl::new(&file_match_dyn(&file_match), vec![JsString::from("0 foo bar")], OneLineRange::new(0, 2, 5), OneLineRange::new(1, 0, 5), false);
    assert_eq!(line_match.text(), "0 foo bar");
    assert_eq!(line_match.range().start_line_number, 2);
    assert_eq!(line_match.range().end_line_number, 2);
    assert_eq!(line_match.range().start_column, 1);
    assert_eq!(line_match.range().end_column, 6);
    assert_eq!(line_match.id(), format!("{MATCH_PREFIX}file:///folder/file.txt>[2,1 -> 2,6]foo"));

    assert_eq!(line_match.full_match_text(false), "foo");
    assert_eq!(line_match.full_match_text(true), "0 foo bar");
}

#[test]
fn line_match_remove() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let t = setup(&guard);
    let search_result = t.a_search_result(&guard);
    let file_match = t.a_file_match(&guard, "folder/file.txt", Some(search_result), vec![text_match("foo bar", OneLineRange::new(1, 0, 3))]);
    let line_match = file_match.matches()[0].clone();
    file_match.remove(&[line_match]);
    assert_eq!(file_match.matches().len(), 0);
}

#[test]
fn file_match() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let t = setup(&guard);
    let search_result = t.a_search_result(&guard);
    let mut file_match = t.a_file_match(&guard, "folder/file.txt", Some(search_result), Vec::new());
    assert_eq!(file_match.matches().len(), 0);
    assert_eq!(file_match.resource().to_string(false), "file:///folder/file.txt");
    assert_eq!(file_match.name(), "file.txt");

    let search_result = t.a_search_result(&guard);
    file_match = t.a_file_match(&guard, "file.txt", Some(search_result), Vec::new());
    assert_eq!(file_match.matches().len(), 0);
    assert_eq!(file_match.resource().to_string(false), "file:///file.txt");
    assert_eq!(file_match.name(), "file.txt");
}

/// O arquivo `folder/file.txt` com os casamentos `foo` e `bar` dos quatro casos de seleção.
fn a_selectable_file_match(t: &Fixture, guard: &LeakGuard) -> Rc<dyn INotebookFileInstanceMatch> {
    let search_result = t.a_search_result(guard);
    t.a_file_match(
        guard,
        "folder/file.txt",
        Some(search_result),
        vec![text_match("foo", OneLineRange::new(1, 0, 3)), text_match("bar", OneLineRange::new(1, 5, 3))],
    )
}

#[test]
fn file_match_select_an_existing_match() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let t = setup(&guard);
    let test_object = a_selectable_file_match(&t, &guard);

    test_object.set_selected_match(Some(test_object.matches()[0].clone()));

    assert_eq!(id_of(&test_object.matches()[0]), id_of(&test_object.get_selected_match().expect("há casamento selecionado")));
}

#[test]
fn file_match_select_non_existing_match() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let t = setup(&guard);
    let test_object = a_selectable_file_match(&t, &guard);
    let target = test_object.matches()[0].clone();
    test_object.remove(&[target.clone()]);

    test_object.set_selected_match(Some(target));

    assert!(test_object.get_selected_match().is_none());
}

#[test]
fn file_match_is_selected_return_true_for_selected_match() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let t = setup(&guard);
    let test_object = a_selectable_file_match(&t, &guard);
    let target = test_object.matches()[0].clone();
    test_object.set_selected_match(Some(target.clone()));

    assert!(test_object.is_match_selected(&target));
}

#[test]
fn file_match_is_selected_return_false_for_un_selected_match() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let t = setup(&guard);
    let test_object = a_selectable_file_match(&t, &guard);
    test_object.set_selected_match(Some(test_object.matches()[0].clone()));
    assert!(!test_object.is_match_selected(&test_object.matches()[1]));
}

#[test]
fn file_match_unselect() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let t = setup(&guard);
    let test_object = a_selectable_file_match(&t, &guard);
    test_object.set_selected_match(Some(test_object.matches()[0].clone()));
    test_object.set_selected_match(None);

    assert!(test_object.get_selected_match().is_none());
}

#[test]
fn file_match_unselect_when_not_selected() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let t = setup(&guard);
    let test_object = a_selectable_file_match(&t, &guard);
    test_object.set_selected_match(None);

    assert!(test_object.get_selected_match().is_none());
}

#[test]
fn match_file_match_search_result_hierarchy_exists() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let t = setup(&guard);

    let search_model = t.instantiation_service.create_instance::<SearchModelImpl>(());
    guard.add_rc(search_model.clone());
    let weak_search_model = Rc::downgrade(&search_model);
    let weak_search_model: Weak<dyn ISearchModel> = weak_search_model;
    let search_result = t.instantiation_service.create_instance::<SearchResultImpl>(weak_search_model);
    guard.add_rc(search_result.clone());
    // O original deixa o resultado sem consulta (ver as divergências do módulo).
    search_result.set_query(Some(root_query()));
    let search_result_dyn: Rc<dyn ISearchResult> = search_result.clone();
    let file_match = t.a_file_match(&guard, "far/boo", Some(search_result_dyn), Vec::new());
    let file_match_as_dyn = file_match_dyn(&file_match);
    let line_match = MatchImpl::new(&file_match_as_dyn, vec![JsString::from("foo bar")], OneLineRange::new(0, 0, 3), OneLineRange::new(1, 0, 3), false);

    assert!(id_of(&line_match.parent()) == id_of(&file_match_as_dyn));
    assert!(id_of(&file_match.parent()) == id_of(&search_result.folder_matches(false)[0]));
}

#[test]
fn adding_a_raw_match_will_add_a_file_match_with_line_matches() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let t = setup(&guard);
    let test_object = t.a_search_result(&guard);
    let target = [a_raw_match(
        "/1",
        vec![
            text_match("preview 1", OneLineRange::new(1, 1, 4)),
            text_match("preview 1", OneLineRange::new(1, 4, 11)),
            text_match("preview 2", line_one_range()),
        ],
    )];

    add_to_search_result(&test_object, &target);

    assert_eq!(3, test_object.count(false));

    let actual = matches_of(&test_object);
    assert_eq!(1, actual.len());
    assert_eq!(uri_file(&format!("{}/1", get_root_name())).to_string(false), actual[0].resource().to_string(false));

    let actua_matches = actual[0].matches();
    assert_eq!(3, actua_matches.len());

    assert_eq!(actua_matches[0].text(), "preview 1");
    assert_eq!(Range::new(2, 2, 2, 5), actua_matches[0].range());

    assert_eq!(actua_matches[1].text(), "preview 1");
    assert_eq!(Range::new(2, 5, 2, 12), actua_matches[1].range());

    assert_eq!(actua_matches[2].text(), "preview 2");
    assert_eq!(Range::new(2, 1, 2, 2), actua_matches[2].range());
}

#[test]
fn adding_multiple_raw_matches() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let t = setup(&guard);
    let test_object = t.a_search_result(&guard);
    let target = [
        a_raw_match("/1", vec![text_match("preview 1", OneLineRange::new(1, 1, 4)), text_match("preview 1", OneLineRange::new(1, 4, 11))]),
        a_raw_match("/2", vec![text_match("preview 2", line_one_range())]),
    ];

    add_to_search_result(&test_object, &target);

    assert_eq!(3, test_object.count(false));

    let actual = matches_of(&test_object);
    assert_eq!(2, actual.len());
    assert_eq!(uri_file(&format!("{}/1", get_root_name())).to_string(false), actual[0].resource().to_string(false));

    let mut actua_matches = actual[0].matches();
    assert_eq!(2, actua_matches.len());
    assert_eq!(actua_matches[0].text(), "preview 1");
    assert_eq!(Range::new(2, 2, 2, 5), actua_matches[0].range());
    assert_eq!(actua_matches[1].text(), "preview 1");
    assert_eq!(Range::new(2, 5, 2, 12), actua_matches[1].range());

    actua_matches = actual[1].matches();
    assert_eq!(1, actua_matches.len());
    assert_eq!(actua_matches[0].text(), "preview 2");
    assert_eq!(Range::new(2, 1, 2, 2), actua_matches[0].range());
}

/// A conferência do estado que `addFileMatch` produziu para as células de um arquivo de notebook: os casamentos de cada
/// célula têm o texto e o intervalo dos resultados de entrada.
fn assert_cell_matches_equal_raw(file_match: &Rc<dyn ISearchTreeFileMatch>, raw_cell: &INotebookCellMatchWithModel) {
    let notebook_file_match = file_match.as_notebook_file_match().expect("o arquivo é um arquivo de notebook");
    let cell_matches = notebook_file_match.cell_matches();
    assert_eq!(1, cell_matches.len());

    let to_expected = |results: &[TextSearchMatch]| -> Vec<(JsString, Range)> {
        results
            .iter()
            .map(|result| {
                let source = result.range_locations[0].source;
                (
                    result.preview_text.clone(),
                    Range::new(source.start_line_number as i32 + 1, source.start_column as i32 + 1, source.end_line_number as i32 + 1, source.end_column as i32 + 1),
                )
            })
            .collect()
    };
    let to_actual = |matches: Vec<Rc<dyn je_workbench::contrib::search::notebook_search::notebook_search_model_base::IMatchInNotebook>>| -> Vec<(JsString, Range)> {
        matches.iter().map(|match_| (match_.text(), match_.range())).collect()
    };

    assert_eq!(to_expected(&raw_cell.content_results), to_actual(cell_matches[0].content_matches()));
    assert_eq!(to_expected(&raw_cell.webview_results), to_actual(cell_matches[0].webview_matches()));
}

#[test]
fn test_that_notebook_matches_get_added_correctly() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let t = setup(&guard);
    let test_object = t.a_search_result(&guard);
    let cell1: Rc<dyn ICellViewModel> = Rc::new(FakeCellViewModel { cell_kind: CellKind::Code });
    let cell2: Rc<dyn ICellViewModel> = Rc::new(FakeCellViewModel { cell_kind: CellKind::Code });

    // `sinon.stub(CellMatch.prototype, 'addContext')`: o `resolveTextModel()` da célula falsa nunca resolve.
    let cell_results = |cell: Rc<dyn ICellViewModel>| INotebookCellMatchWithModel {
        cell,
        index: 0,
        content_results: vec![text_match("preview 1", OneLineRange::new(1, 1, 4))],
        webview_results: vec![text_match("preview 1", OneLineRange::new(1, 4, 11)), text_match("preview 2", line_one_range())],
    };
    let raw_cell1 = cell_results(cell1);
    let raw_cell2 = cell_results(cell2);
    let file_match1 = a_raw_file_match_with_cells("/1", vec![raw_cell1.clone()]);
    let file_match2 = a_raw_file_match_with_cells("/2", vec![raw_cell2.clone()]);
    let target = [file_match1, file_match2];

    add_to_search_result(&test_object, &target);
    assert_eq!(6, test_object.count(false));
    let actual = matches_of(&test_object);
    assert_cell_matches_equal_raw(&actual[0], &raw_cell1);
    assert_cell_matches_equal_raw(&actual[1], &raw_cell2);
}

#[test]
fn dispose_disposes_matches() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let t = setup(&guard);
    let target1 = Rc::new(Cell::new(0));
    let target2 = Rc::new(Cell::new(0));

    let test_object = t.a_search_result(&guard);
    add_to_search_result(
        &test_object,
        &[a_raw_match("/1", vec![text_match("preview 1", line_one_range())]), a_raw_match("/2", vec![text_match("preview 2", line_one_range())])],
    );

    let counter = target1.clone();
    guard.add(matches_of(&test_object)[0].on_dispose().listen(move |_| counter.set(counter.get() + 1)));
    let counter = target2.clone();
    guard.add(matches_of(&test_object)[1].on_dispose().listen(move |_| counter.set(counter.get() + 1)));

    test_object.dispose();

    assert!(test_object.is_empty());
    assert_eq!(target1.get(), 1);
    assert_eq!(target2.get(), 1);
}

#[test]
fn remove_triggers_change_event() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let t = setup(&guard);
    let test_object = t.a_search_result(&guard);
    add_to_search_result(&test_object, &[a_raw_match("/1", vec![text_match("preview 1", line_one_range())])]);
    let object_to_remove = matches_of(&test_object)[0].clone();
    let target = spy_on_change(&guard, &test_object);

    test_object.remove(&[FileOrFolderMatch::File(object_to_remove.clone())], false);

    assert_eq!(target.borrow().len(), 1);
    assert_change_event(&target.borrow()[0], &[object_to_remove], None, Some(true), None);
}

#[test]
fn remove_array_triggers_change_event() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let t = setup(&guard);
    let test_object = t.a_search_result(&guard);
    add_to_search_result(
        &test_object,
        &[a_raw_match("/1", vec![text_match("preview 1", line_one_range())]), a_raw_match("/2", vec![text_match("preview 2", line_one_range())])],
    );
    let array_to_remove = matches_of(&test_object);
    let target = spy_on_change(&guard, &test_object);

    let elements: Vec<FileOrFolderMatch> = array_to_remove.iter().cloned().map(FileOrFolderMatch::File).collect();
    test_object.remove(&elements, false);

    assert_eq!(target.borrow().len(), 1);
    assert_change_event(&target.borrow()[0], &array_to_remove, None, Some(true), None);
}

#[test]
fn removing_all_line_matches_and_adding_back_will_add_file_back_to_result() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let t = setup(&guard);
    let test_object = t.a_search_result(&guard);
    add_to_search_result(&test_object, &[a_raw_match("/1", vec![text_match("preview 1", line_one_range())])]);
    let target = matches_of(&test_object)[0].clone();
    let match_to_remove = target.matches()[0].clone();
    target.remove(&[match_to_remove.clone()]);

    assert!(test_object.is_empty());
    target.add(match_to_remove, true);

    assert_eq!(1, test_object.file_count(false));
    assert_eq!(id_of(&target), id_of(&matches_of(&test_object)[0]));
}

#[test]
fn replace_should_remove_the_file_match() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let t = setup(&guard);
    let void_promise = Promise::resolved(());
    let promise = void_promise.clone();
    t.replace_service.stub_replace(move |_| promise.clone());
    let test_object = t.a_search_result(&guard);
    add_to_search_result(&test_object, &[a_raw_match("/1", vec![text_match("preview 1", line_one_range())])]);

    let _ = test_object.replace(&matches_of(&test_object)[0]);

    // `return voidPromise.then(() => assert.ok(testObject.isEmpty()))`
    runtime::run_microtasks();
    assert!(test_object.is_empty());
}

#[test]
fn replace_should_trigger_the_change_event() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let t = setup(&guard);
    let void_promise = Promise::resolved(());
    let promise = void_promise.clone();
    t.replace_service.stub_replace(move |_| promise.clone());
    let test_object = t.a_search_result(&guard);
    add_to_search_result(&test_object, &[a_raw_match("/1", vec![text_match("preview 1", line_one_range())])]);

    let target = spy_on_change(&guard, &test_object);
    let object_to_remove = matches_of(&test_object)[0].clone();

    let _ = test_object.replace(&object_to_remove);

    // `return voidPromise.then(() => { ... })`
    runtime::run_microtasks();
    assert_eq!(target.borrow().len(), 1);
    assert_change_event(&target.borrow()[0], &[object_to_remove], None, Some(true), None);
}

#[test]
fn replace_all_should_remove_all_file_matches() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let t = setup(&guard);
    let void_promise = Promise::resolved(());
    let promise = void_promise.clone();
    t.replace_service.stub_replace(move |_| promise.clone());
    let test_object = t.a_search_result(&guard);
    add_to_search_result(
        &test_object,
        &[a_raw_match("/1", vec![text_match("preview 1", line_one_range())]), a_raw_match("/2", vec![text_match("preview 2", line_one_range())])],
    );

    let _ = test_object.replace_all(None);

    // `return voidPromise.then(() => assert.ok(testObject.isEmpty()))`
    runtime::run_microtasks();
    assert!(test_object.is_empty());
}

#[test]
fn batch_remove_should_trigger_the_on_change_event_correctly() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let t = setup(&guard);
    let test_object = t.get_populated_search_result(&guard);

    let folder_match = test_object.folder_matches(false)[0].clone();
    let file_match = test_object.folder_matches(false)[1].all_downstream_file_matches()[0].clone();
    let match_ = test_object.folder_matches(false)[1].all_downstream_file_matches()[1].matches()[0].clone();

    let array_to_remove = [RenderableMatch::FolderMatch(folder_match.clone()), RenderableMatch::FileMatch(file_match.clone()), RenderableMatch::Match(match_.clone())];
    let mut expected_array_result = folder_match.all_downstream_file_matches();
    expected_array_result.push(file_match);
    expected_array_result.push(match_.parent());

    let target = spy_on_change(&guard, &test_object);
    test_object.batch_remove(&array_to_remove);

    assert_eq!(target.borrow().len(), 1);
    assert_change_event(&target.borrow()[0], &expected_array_result, Some(false), Some(true), None);
}

#[test]
fn batch_remove_should_remove_folder_match_no_root_other_files_correctly() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let t = setup(&guard);
    let test_object = t.a_search_result(&guard);

    test_object.set_query(Some(text_query(&["/workspace"], "foo")));

    // Add a file inside the workspace folder
    add_to_search_result(&test_object, &[a_raw_match("/workspace/file.txt", vec![text_match("preview 1", line_one_range())])]);

    // Add a file outside of the workspace folder (goes to "Other files")
    add_to_search_result(&test_object, &[a_raw_match("/other/outside.txt", vec![text_match("preview 2", line_one_range())])]);

    // Should have 2 folder matches: workspace root and "Other files"
    let folder_matches = test_object.folder_matches(false);
    assert_eq!(folder_matches.len(), 2);

    // Find the "Other files" folder match (FolderMatchNoRoot)
    let other_files_match = folder_matches.iter().find(|fm| is_search_tree_folder_match_no_root(fm.as_ref())).expect("Should have an Other files folder match").clone();
    assert_eq!(other_files_match.all_downstream_file_matches().len(), 1);

    let target = spy_on_change(&guard, &test_object);
    test_object.batch_remove(&[RenderableMatch::FolderMatch(other_files_match.clone())]);

    assert_eq!(target.borrow().len(), 1);
    // After removal, the Other files folder should be cleared
    assert_eq!(other_files_match.all_downstream_file_matches().len(), 0);
}

#[test]
fn folder_matches_are_not_retained_across_queries() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let t = setup(&guard);
    let test_object = t.a_search_result(&guard);
    // `sinon.spy(folderMatch, 'dispose')`: a contagem de chamadas de `dispose` de cada pasta.
    let spied_folder_matches = test_object.folder_matches(false);

    for folder in ["/second", "/third"] {
        test_object.set_query(Some(text_query(&[folder], "")));
        test_object.add(&[], "test", false, false);
    }

    test_object.dispose();

    let dispose_counts: Vec<usize> = spied_folder_matches.iter().map(|folder_match| folder_match.dispose_count_for_testing()).collect();
    assert_eq!(dispose_counts, vec![1, 1]);
}

#[test]
fn batch_replace_should_trigger_the_on_change_event_correctly() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let t = setup(&guard);
    // `replaceSpy`: a identidade do argumento de cada chamada de `replace`.
    let replace_spy: Rc<RefCell<Vec<usize>>> = Rc::new(RefCell::new(Vec::new()));
    let spy = replace_spy.clone();
    t.replace_service.stub_replace(move |call| {
        // `Array.isArray(arg) ? replaceSpy(arg[0]) : replaceSpy(arg)`
        match call {
            ReplaceCall::Files(files) => spy.borrow_mut().push(id_of(&files[0])),
            ReplaceCall::Match(match_) => spy.borrow_mut().push(id_of(&match_)),
        }
        Promise::resolved(())
    });

    let test_object = t.get_populated_search_result(&guard);

    let folder_match = test_object.folder_matches(false)[0].clone();
    let file_match = test_object.folder_matches(false)[1].all_downstream_file_matches()[0].clone();
    let match_ = test_object.folder_matches(false)[1].all_downstream_file_matches()[1].matches()[0].clone();

    let first_expected_match = folder_match.all_downstream_file_matches()[0].clone();

    let array_to_remove = vec![RenderableMatch::FolderMatch(folder_match), RenderableMatch::FileMatch(file_match.clone()), RenderableMatch::Match(match_.clone())];

    let target = spy_on_change(&guard, &test_object);
    let batch_replace = test_object.batch_replace(array_to_remove);
    // `await testObject.batchReplace(arrayToRemove)`
    runtime::run_microtasks();
    assert!(matches!(batch_replace.result(), Some(Ok(()))));

    assert_eq!(target.borrow().len(), 1);
    let calls = replace_spy.borrow();
    assert_eq!(calls.len(), 3);
    assert_eq!(calls[0], id_of(&first_expected_match));
    assert_eq!(calls[1], id_of(&file_match));
    assert_eq!(calls[2], id_of(&match_));
}

#[test]
fn creating_a_model_with_nested_folders_should_create_the_correct_structure() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let t = setup(&guard);
    let test_object = t.get_populated_search_result_for_tree_testing(&guard);

    let root0 = test_object.folder_matches(false)[0].clone();
    let root1 = test_object.folder_matches(false)[1].clone();
    let root2 = test_object.folder_matches(false)[2].clone();
    let root3 = test_object.folder_matches(false)[3].clone();

    let root0_downstream_files = root0.all_downstream_file_matches();
    let mut expected_root0_files = root0.file_matches_iterator();
    expected_root0_files.extend(get_folder_match_at_index(&root0, 0).file_matches_iterator());
    assert_same_files(&root0_downstream_files, &expected_root0_files);
    assert_same_files(&get_folder_match_at_index(&root0, 0).all_downstream_file_matches(), &get_folder_match_at_index(&root0, 0).file_matches_iterator());
    assert_same_folder(&get_file_match_at_index(&get_folder_match_at_index(&root0, 0), 0).parent(), &get_folder_match_at_index(&root0, 0));
    assert_same_folder(&folder_parent(&get_folder_match_at_index(&root0, 0)), &root0);
    assert_same_folder(&get_folder_match_at_index(&root0, 0).closest_root().expect("a pasta intermediária tem raiz mais próxima"), &root0);
    for e in &root0_downstream_files {
        assert_closest_root(e, &root0);
    }

    let root1_downstream_files = root1.all_downstream_file_matches();
    let mut expected_root1_files = root1.file_matches_iterator();
    expected_root1_files.extend(get_folder_match_at_index(&root1, 0).file_matches_iterator()); // excludes the matches from nested root
    assert_same_files(&root1.all_downstream_file_matches(), &expected_root1_files);
    assert_same_folder(&get_file_match_at_index(&get_folder_match_at_index(&root1, 0), 0).parent(), &get_folder_match_at_index(&root1, 0));
    for e in &root1_downstream_files {
        assert_closest_root(e, &root1);
    }

    let root2_downstream_files = root2.all_downstream_file_matches();
    assert_same_files(&root2_downstream_files, &root2.file_matches_iterator());
    assert_same_folder(&get_file_match_at_index(&root2, 0).parent(), &root2);
    assert_closest_root(&get_file_match_at_index(&root2, 0), &root2);

    let root3_downstream_files = root3.all_downstream_file_matches();
    let root3_level3_folder = get_folder_match_at_index(&get_folder_match_at_index(&root3, 0), 0);
    let mut expected_root3_files = root3.file_matches_iterator();
    expected_root3_files.extend(get_folder_match_at_index(&root3_level3_folder, 0).file_matches_iterator());
    expected_root3_files.extend(get_folder_match_at_index(&root3_level3_folder, 1).file_matches_iterator());
    assert_same_files(&root3_downstream_files, &expected_root3_files);
    assert_same_files(&root3_level3_folder.all_downstream_file_matches(), &get_folder_match_at_index(&root3, 0).all_downstream_file_matches());

    assert_same_folder(
        &get_file_match_at_index(&get_folder_match_at_index(&root3_level3_folder, 1), 0).parent(),
        &get_folder_match_at_index(&root3_level3_folder, 1),
    );
    assert_same_folder(&folder_parent(&get_folder_match_at_index(&root3_level3_folder, 1)), &root3_level3_folder);
    assert_same_folder(&folder_parent(&root3_level3_folder), &get_folder_match_at_index(&root3, 0));

    for e in &root3_downstream_files {
        assert_closest_root(e, &root3);
    }
}

#[test]
fn removing_an_intermediate_folder_should_call_on_change_on_all_downstream_file_matches() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let t = setup(&guard);
    let test_object = t.get_populated_search_result_for_tree_testing(&guard);

    let folder_match =
        get_folder_match_at_index(&get_folder_match_at_index(&get_folder_match_at_index(&test_object.folder_matches(false)[3], 0), 0), 0);

    let expected_array_result = folder_match.all_downstream_file_matches();

    let target = spy_on_change(&guard, &test_object);
    test_object.remove(&[FileOrFolderMatch::Folder(folder_match)], false);
    assert_eq!(target.borrow().len(), 1);
    assert_change_event(&target.borrow()[0], &expected_array_result, Some(false), Some(true), Some(false));
}

#[test]
fn replacing_an_intermediate_folder_should_remove_all_downstream_folders_and_file_matches() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let t = setup(&guard);
    let test_object = t.get_populated_search_result_for_tree_testing(&guard);

    let folder_match = get_folder_match_at_index(&test_object.folder_matches(false)[3], 0);

    let expected_array_result = folder_match.all_downstream_file_matches();

    let target = spy_on_change(&guard, &test_object);
    let batch_replace = test_object.batch_replace(vec![RenderableMatch::FolderMatch(folder_match)]);
    // `await testObject.batchReplace([folderMatch])`
    runtime::run_microtasks();
    assert!(matches!(batch_replace.result(), Some(Ok(()))));
    assert_change_event(&target.borrow()[0], &expected_array_result, Some(false), Some(true), None);
}
