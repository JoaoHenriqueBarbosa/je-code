//! Porte de `src/vs/workbench/contrib/testing/common/testCoverage.ts`: a cobertura de uma execução, organizada numa
//! árvore de prefixos por esquema, autoridade e segmentos do caminho do arquivo, com os totais dos diretórios
//! calculados a partir dos arquivos.
//!
//! Divergências:
//! - **Hierarquia de classes.** `AbstractFileCoverage`, `ComputedFileCoverage`, `BypassedFileCoverage` e `FileCoverage`
//!   são uma struct só, [`AbstractFileCoverage`], com o [`FileCoverageKind`]; `node instanceof FileCoverage` é
//!   [`AbstractFileCoverage::is_file_coverage`]. `details`, `detailsForTest` e `hasSynchronousDetails` só existem no
//!   tipo `File`: chamá-los nos demais lança, como chamar um membro ausente. `FileCoverage` é o alias da struct.
//! - **Identidade e mutação.** Os nós da árvore e do `ResourceMap` são `Rc<AbstractFileCoverage>`, com os campos mutáveis
//!   em células, porque o original os compartilha e muta por referência.
//! - **`LiveTestResult`** é guardado como `Weak` (o resultado ao vivo guarda a cobertura, que o guardaria de volta: um ciclo
//!   de `Rc` que o coletor do JavaScript resolve e o `Rc` não). Os acessos `result()` e `from_result()` o promovem.
//! - **`didAddCoverage`** emite os valores dos nós da cadeia (`IPrefixTreeNode<AbstractFileCoverage>[]` no original):
//!   os nós do porte pertencem à árvore e só o `value` é lido pelos consumidores.
//! - **`associatedData: Map<unknown, unknown>`** tem a identidade (endereço) do objeto como chave.
//! - **`treePathForUri`** e os demais geradores devolvem `Vec`.
//! - **`async details(token)`**: devolve a `Promise` e roda a limpeza em caso de erro numa reação.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use indexmap::IndexSet;
use je_base::JsString;
use je_base::cancellation::CancellationToken;
use je_base::errors::{Error, throw};
use je_base::map::ResourceMap;
use je_base::observable::{ITransaction, ObservableSignal, observable_signal};
use je_base::prefix_tree::WellDefinedPrefixTree;
use je_base::promise::Promise;
use je_base::uri::{URI, UriComponents};
use je_platform::uri_identity::uri_identity::IUriIdentityService;

use super::test_id::TestId;
use super::test_result::LiveTestResult;
use super::test_types::{CoverageDetails, ICoverageCount, IFileCoverage};

pub trait ICoverageAccessor {
    fn get_coverage_details(&self, id: &str, test_id: Option<&str>, token: CancellationToken) -> Promise<Vec<CoverageDetails>>;
}

thread_local! {
    static INC_ID: Cell<u64> = const { Cell::new(0) };
}

fn next_inc_id() -> u64 {
    INC_ID.with(|id| {
        let current = id.get();
        id.set(current + 1);
        current
    })
}

/// `'statement' | 'branch' | 'declaration'`.
#[derive(Clone, Copy)]
enum CountKind {
    Statement,
    Branch,
    Declaration,
}

/// Class that exposese coverage information for a run.
pub struct TestCoverage {
    file_coverage: RefCell<ResourceMap<Rc<AbstractFileCoverage>>>,
    pub did_add_coverage: Rc<ObservableSignal<Vec<Rc<AbstractFileCoverage>>>>,
    pub tree: RefCell<WellDefinedPrefixTree<Rc<AbstractFileCoverage>>>,
    pub associated_data: RefCell<HashMap<usize, Rc<dyn Any>>>,

    result: Weak<LiveTestResult>,
    pub from_task_id: String,
    uri_identity_service: Rc<dyn IUriIdentityService>,
    accessor: Rc<dyn ICoverageAccessor>,
}

impl TestCoverage {
    pub fn new(result: &Rc<LiveTestResult>, from_task_id: &str, uri_identity_service: Rc<dyn IUriIdentityService>, accessor: Rc<dyn ICoverageAccessor>) -> Rc<Self> {
        Rc::new(Self {
            file_coverage: RefCell::new(ResourceMap::new()),
            did_add_coverage: observable_signal("didAddCoverage"),
            tree: RefCell::new(WellDefinedPrefixTree::new()),
            associated_data: RefCell::new(HashMap::new()),
            result: Rc::downgrade(result),
            from_task_id: from_task_id.to_string(),
            uri_identity_service,
            accessor,
        })
    }

    /// O resultado ao vivo desta cobertura.
    pub fn result(&self) -> Option<Rc<LiveTestResult>> {
        self.result.upgrade()
    }

    /// Gets all test IDs that were included in this test run.
    pub fn all_per_test_ids(&self) -> Vec<String> {
        let mut seen = IndexSet::new();
        for root in self.tree.borrow().nodes() {
            if let Some(value) = &root.value
                && let Some(per_test_data) = &*value.per_test_data.borrow()
            {
                for id in per_test_data {
                    seen.insert(id.clone());
                }
            }
        }
        seen.into_iter().collect()
    }

    pub fn append(&self, coverage: &IFileCoverage, tx: Option<&dyn ITransaction>) {
        let previous = self.get_computed_for_uri(&coverage.uri);
        let result = self.result.clone();
        let apply_delta = |kind: CountKind, node: &AbstractFileCoverage| {
            let (coverage_count, previous_count) = match kind {
                CountKind::Statement => (Some(coverage.statement), previous.as_ref().map(|p| p.statement.get())),
                CountKind::Branch => (coverage.branch, previous.as_ref().and_then(|p| p.branch.get())),
                CountKind::Declaration => (coverage.declaration, previous.as_ref().and_then(|p| p.declaration.get())),
            };
            let current = match kind {
                CountKind::Statement => Some(node.statement.get()),
                CountKind::Branch => node.branch.get(),
                CountKind::Declaration => node.declaration.get(),
            };
            let updated = match current {
                None => coverage_count,
                Some(mut current) => {
                    current.covered += coverage_count.map_or(0.0, |c| c.covered) - previous_count.map_or(0.0, |c| c.covered);
                    current.total += coverage_count.map_or(0.0, |c| c.total) - previous_count.map_or(0.0, |c| c.total);
                    Some(current)
                }
            };
            match kind {
                CountKind::Statement => {
                    if let Some(updated) = updated {
                        node.statement.set(updated);
                    }
                }
                CountKind::Branch => node.branch.set(updated),
                CountKind::Declaration => node.declaration.set(updated),
            }
        };

        // We insert using the non-canonical path to normalize for casing differences
        // between URIs, but when inserting an intermediate node always use 'a' canonical
        // version.
        let canonical = self.tree_path_for_uri(&coverage.uri, true);
        let non_canonical = self.tree_path_for_uri(&coverage.uri, false);
        let mut chain: Vec<Rc<AbstractFileCoverage>> = Vec::new();
        let mut depth = 0;

        self.tree.borrow_mut().mutate_path(non_canonical, |node| {
            depth += 1;

            if depth == canonical.len() {
                // we reached our destination node, apply the coverage as necessary:
                if let Some(v) = &node.value {
                    // if ID was generated from a test-specific coverage, reassign it to get its real ID in the extension host.
                    *v.id.borrow_mut() = coverage.id.clone();
                    v.statement.set(coverage.statement);
                    v.branch.set(coverage.branch);
                    v.declaration.set(coverage.declaration);
                } else {
                    let v = AbstractFileCoverage::new_file(coverage, result.clone(), self.accessor.clone());
                    node.value = Some(v.clone());
                    self.file_coverage.borrow_mut().set(coverage.uri.clone(), v);
                }
            } else {
                // Otherwise, if this is not a partial per-test coverage, merge the
                // coverage changes into the chain. Per-test coverages are not complete
                // and we don't want to consider them for computation.
                match &node.value {
                    None => {
                        // clone because later intersertions can modify the counts:
                        let mut intermediate = coverage.clone();
                        intermediate.id = next_inc_id().to_string();
                        intermediate.uri = self.tree_path_to_uri(&canonical[..depth]);
                        node.value = Some(AbstractFileCoverage::new_computed(&intermediate, result.clone()));
                    }
                    Some(value) => {
                        apply_delta(CountKind::Statement, value);
                        apply_delta(CountKind::Branch, value);
                        apply_delta(CountKind::Declaration, value);
                        value.did_change.trigger(tx, ());
                    }
                }
            }

            if let Some(test_ids) = &coverage.test_ids
                && let Some(value) = &node.value
            {
                let mut per_test_data = value.per_test_data.borrow_mut();
                let set = per_test_data.get_or_insert_with(IndexSet::new);
                for id in test_ids {
                    set.insert(id.clone());
                }
            }

            if let Some(value) = &node.value {
                chain.push(value.clone());
            }
        });

        self.did_add_coverage.trigger(tx, chain);
    }

    /// Builds a new tree filtered to per-test coverage data for the given ID.
    pub fn filter_tree_for_test(&self, test_id: &TestId) -> WellDefinedPrefixTree<Rc<AbstractFileCoverage>> {
        let mut tree = WellDefinedPrefixTree::<Rc<AbstractFileCoverage>>::new();
        let test_id = test_id.to_string();
        for node in self.tree.borrow().values() {
            if node.is_file_coverage() {
                if !node.per_test_data.borrow().as_ref().is_some_and(|data| data.contains(&test_id)) {
                    continue;
                }

                let canonical = self.tree_path_for_uri(&node.uri, true);
                let mut depth = 0;
                tree.mutate_path(self.tree_path_for_uri(&node.uri, false), |n| {
                    depth += 1;
                    if n.value.is_none() {
                        n.value = Some(AbstractFileCoverage::new_bypassed(self.tree_path_to_uri(&canonical[..depth]), node.from_result.clone()));
                    }
                });
            }
        }

        tree
    }

    /// Gets coverage information for all files.
    pub fn get_all_files(&self) -> std::cell::Ref<'_, ResourceMap<Rc<AbstractFileCoverage>>> {
        self.file_coverage.borrow()
    }

    /// Gets coverage information for a specific file.
    pub fn get_uri(&self, uri: &URI) -> Option<Rc<AbstractFileCoverage>> {
        self.file_coverage.borrow().get(uri).cloned()
    }

    /// Gets computed information for a file, including DFS-computed information
    /// from child tests.
    pub fn get_computed_for_uri(&self, uri: &URI) -> Option<Rc<AbstractFileCoverage>> {
        self.tree.borrow().find(self.tree_path_for_uri(uri, false)).cloned()
    }

    fn tree_path_for_uri(&self, uri: &URI, canconical_path: bool) -> Vec<String> {
        let mut path = vec![uri.scheme().to_string_lossy(), uri.authority().to_string_lossy()];

        let uri_path = uri.path().to_string_lossy();
        let uri_path = if !canconical_path && self.uri_identity_service.ext_uri().ignore_path_casing(uri) { uri_path.to_lowercase() } else { uri_path };
        path.extend(uri_path.split('/').map(str::to_string));
        path
    }

    /// `treePathToUri(path)`: `URI.from({ scheme: path[0], authority: path[1], path: path.slice(2).join('/') })`. Num nó
    /// perto da raiz (caminho com menos de três partes) o índice ausente é `undefined`, que o `URI.from` trata como vazio.
    fn tree_path_to_uri(&self, path: &[String]) -> URI {
        URI::from(
            &UriComponents {
                scheme: path.first().map(|part| JsString::from(part.as_str())),
                authority: path.get(1).map(|part| JsString::from(part.as_str())),
                path: Some(JsString::from(path.get(2..).unwrap_or_default().join("/"))),
                ..UriComponents::default()
            },
            false,
        )
    }
}

pub fn get_total_coverage_percent(statement: &ICoverageCount, branch: Option<&ICoverageCount>, function: Option<&ICoverageCount>) -> f64 {
    let mut numerator = statement.covered;
    let mut denominator = statement.total;

    if let Some(branch) = branch {
        numerator += branch.covered;
        denominator += branch.total;
    }

    if let Some(function) = function {
        numerator += function.covered;
        denominator += function.total;
    }

    if denominator == 0.0 { 1.0 } else { numerator / denominator }
}

/// Os tipos de nó da árvore de cobertura.
pub enum FileCoverageKind {
    /// `ComputedFileCoverage`: File coverage info computed from children in the tree, not provided by the
    /// extension.
    Computed,
    /// `BypassedFileCoverage`: A virtual node that doesn't have any added coverage info.
    Bypassed,
    /// `FileCoverage`.
    File(Rc<FileCoverageState>),
}

/// O estado só do `FileCoverage`.
pub struct FileCoverageState {
    accessor: Rc<dyn ICoverageAccessor>,
    details: RefCell<Option<Promise<Vec<CoverageDetails>>>>,
    resolved: Cell<bool>,
    details_for_test: RefCell<Option<HashMap<String, Promise<Vec<CoverageDetails>>>>>,
}

/// `FileCoverage`.
pub type FileCoverage = AbstractFileCoverage;

pub struct AbstractFileCoverage {
    pub id: RefCell<String>,
    pub uri: URI,
    pub statement: Cell<ICoverageCount>,
    pub branch: Cell<Option<ICoverageCount>>,
    pub declaration: Cell<Option<ICoverageCount>>,
    pub did_change: Rc<ObservableSignal<()>>,

    /// Per-test coverage data for this file, if available.
    pub per_test_data: RefCell<Option<IndexSet<String>>>,

    from_result: Weak<LiveTestResult>,
    kind: FileCoverageKind,
}

impl AbstractFileCoverage {
    fn build(coverage: &IFileCoverage, from_result: Weak<LiveTestResult>, kind: FileCoverageKind) -> Rc<Self> {
        Rc::new(Self {
            id: RefCell::new(coverage.id.clone()),
            uri: coverage.uri.clone(),
            statement: Cell::new(coverage.statement),
            branch: Cell::new(coverage.branch),
            declaration: Cell::new(coverage.declaration),
            did_change: observable_signal("didChange"),
            per_test_data: RefCell::new(None),
            from_result,
            kind,
        })
    }

    /// `new ComputedFileCoverage(coverage, fromResult)`.
    pub fn new_computed(coverage: &IFileCoverage, from_result: Weak<LiveTestResult>) -> Rc<Self> {
        Self::build(coverage, from_result, FileCoverageKind::Computed)
    }

    /// `new BypassedFileCoverage(uri, result)`.
    pub fn new_bypassed(uri: URI, result: Weak<LiveTestResult>) -> Rc<Self> {
        let coverage = IFileCoverage { id: next_inc_id().to_string(), uri, test_ids: None, statement: ICoverageCount { covered: 0.0, total: 0.0 }, branch: None, declaration: None };
        Self::build(&coverage, result, FileCoverageKind::Bypassed)
    }

    /// `new FileCoverage(coverage, fromResult, accessor)`.
    pub fn new_file(coverage: &IFileCoverage, from_result: Weak<LiveTestResult>, accessor: Rc<dyn ICoverageAccessor>) -> Rc<Self> {
        let state = Rc::new(FileCoverageState { accessor, details: RefCell::new(None), resolved: Cell::new(false), details_for_test: RefCell::new(None) });
        Self::build(coverage, from_result, FileCoverageKind::File(state))
    }

    pub fn kind(&self) -> &FileCoverageKind {
        &self.kind
    }

    /// `node instanceof FileCoverage`.
    pub fn is_file_coverage(&self) -> bool {
        matches!(self.kind, FileCoverageKind::File(_))
    }

    pub fn from_result(&self) -> Option<Rc<LiveTestResult>> {
        self.from_result.upgrade()
    }

    /// Gets the total coverage percent based on information provided.
    /// This is based on the Clover total coverage formula
    pub fn tpc(&self) -> f64 {
        get_total_coverage_percent(&self.statement.get(), self.branch.get().as_ref(), self.declaration.get().as_ref())
    }

    fn file_state(&self, member: &str) -> &Rc<FileCoverageState> {
        match &self.kind {
            FileCoverageKind::File(state) => state,
            _ => throw(Error::type_error(format!("{member} is not a function"))),
        }
    }

    /// Gets whether details are synchronously available
    pub fn has_synchronous_details(&self) -> bool {
        self.file_state("hasSynchronousDetails").resolved.get()
    }

    /// Gets per-line coverage details.
    pub fn details_for_test(&self, test_id: &TestId, token: CancellationToken) -> Promise<Vec<CoverageDetails>> {
        let state = self.file_state("detailsForTest").clone();
        let test_id = test_id.to_string();
        let prev = state.details_for_test.borrow_mut().get_or_insert_with(HashMap::new).get(&test_id).cloned();
        if let Some(prev) = prev {
            return prev;
        }

        let on_error_state = state.clone();
        let on_error_test_id = test_id.clone();
        let promise = state.accessor.get_coverage_details(&self.id.borrow(), Some(&test_id), token).then_result(move |result| {
            if result.is_err()
                && let Some(map) = on_error_state.details_for_test.borrow_mut().as_mut()
            {
                map.remove(&on_error_test_id);
            }
            result
        });

        state.details_for_test.borrow_mut().get_or_insert_with(HashMap::new).insert(test_id, promise.clone());
        promise
    }

    /// Gets per-line coverage details.
    pub fn details(&self, token: CancellationToken) -> Promise<Vec<CoverageDetails>> {
        let state = self.file_state("details").clone();
        let existing = state.details.borrow().clone();
        let promise = match existing {
            Some(existing) => existing,
            None => {
                let created = state.accessor.get_coverage_details(&self.id.borrow(), None, token);
                *state.details.borrow_mut() = Some(created.clone());
                created
            }
        };

        promise.then_result(move |result| match result {
            Ok(details) => {
                state.resolved.set(true);
                Ok(details)
            }
            Err(error) => {
                *state.details.borrow_mut() = None;
                Err(error)
            }
        })
    }
}

pub fn total_from_coverage_details(uri: URI, details: &[CoverageDetails]) -> IFileCoverage {
    let mut fc = IFileCoverage { id: String::new(), uri, test_ids: None, statement: ICoverageCount::empty(), branch: None, declaration: None };

    for detail in details {
        match detail {
            CoverageDetails::Statement(statement) => {
                fc.statement.total += 1.0;
                fc.statement.total += if statement.count.is_truthy() { 1.0 } else { 0.0 };

                for branch in statement.branches.iter().flatten() {
                    let counts = fc.branch.get_or_insert_with(ICoverageCount::empty);
                    counts.total += 1.0;
                    counts.covered += if branch.count.is_truthy() { 1.0 } else { 0.0 };
                }
            }
            CoverageDetails::Declaration(declaration) => {
                let counts = fc.declaration.get_or_insert_with(ICoverageCount::empty);
                counts.total += 1.0;
                counts.covered += if declaration.count.is_truthy() { 1.0 } else { 0.0 };
            }
        }
    }

    fc
}
