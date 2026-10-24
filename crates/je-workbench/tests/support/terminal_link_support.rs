//! Infraestrutura de teste de `src/vs/workbench/contrib/terminalContrib/links/test/browser/terminalLinkOpeners.test.ts` e
//! `terminalLocalLinkDetector.test.ts`: os dublês dos serviços que os dois testes injetam.
//!
//! Divergências:
//! - **`TestInstantiationService` e `instantiationService.stub(...)`.** O contêiner de injeção não entra: cada teste monta os
//!   objetos com os dublês deste arquivo.
//! - **`TestFileService extends FileService` com `stat` sobrescrito.** É o [`LinkTestFileService`]: um `IFileService` que só
//!   responde `stat` (existe, como arquivo, se `_files` é `'*'` ou contém o recurso; senão `ENOENT`) e `hasCapability`/`hasProvider`
//!   (nenhum provedor), que o `UriIdentityService` lê. Os demais membros lançam `TypeError`.
//! - **`TestSearchService extends SearchService`.** É o [`TestSearchService`]: `fileSearch` devolve o resultado guardado ou
//!   rejeita com o `TypeError` que o `this._searchResult!` indefinido causa quando o código lê `results.results`.
//! - **`instantiationService.stub(IQuickInputService, { quickAccess: { show } })` e `stub(IEditorService, { openEditor })`.** São
//!   o [`RecordingQuickAccess`] e o [`RecordingEditorService`], que gravam o `activationResult` compartilhado.
//! - **`opener.setFileQueryBuilder({ file: () => null! })`.** É o [`StubFileQueryBuilder`], que devolve uma consulta vazia: o
//!   `TestSearchService` ignora a consulta, então o `QueryBuilder` real que o original usa nos demais casos não muda o
//!   resultado.

use std::cell::RefCell;
use std::rc::Rc;

use je_base::cancellation::CancellationToken;
use je_base::errors::{self, Error};
use je_base::event::Event;
use je_base::lifecycle::DisposableRef;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_platform::files::file_service::FileContentSource;
use je_platform::files::files::{
    CreateFileOptions, FileChangesEvent, FileContent, FileOperationEvent, FileStat, FileStatWithMetadata, FileStreamContent, FileSystemProviderActivationEvent,
    FileSystemProviderCapabilities, FileSystemProviderCapabilitiesChangeEvent, FileSystemProviderRef, FileSystemProviderRegistrationEvent, PartialFileDeleteOptions,
    ReadFileOptions, ResolveFileOptions, WatchOptions, WriteFileOptions,
};
use je_platform::files::i_file_service::IFileService;
use je_platform::opener::editor_types::TextEditorSelection;
use je_workbench::common::editor::{EditorIdentifier, EditorInputOrUntyped, EditorInputRef, EditorInputWithOptions, EditorsOrder, IEditorPane, UntypedEditorInput};
use je_workbench::contrib::terminal::xterm_buffer::{IBuffer, IBufferCellPosition, IBufferLine, IBufferRange};
use je_workbench::contrib::terminal_contrib::links::links::{ITerminalLinkDetector, TerminalBuiltinLinkType};
use je_workbench::contrib::terminal_contrib::links::terminal_link_openers::{IFileQueryBuilder, IQuickAccessController};

use super::fake_xterm::FakeXterm;
use je_workbench::services::editor::editor_service::{
    ActiveTextEditorControl, FindEditorOptions, IEditorService, SaveAllEditorsOptions, SaveEditorsOptions, SaveEditorsResult,
};
use je_workbench::services::search::query_builder::{IFileQueryBuilderOptions, WorkspaceFolderOrUri};
use je_workbench::services::search::search::{
    IAITextQuery, IFileQuery, IFileQueryProps, ISearchComplete, ISearchResultProvider, ISearchService, ITextQuery, SearchProgressCallback, SearchProviderType,
    SharedResourceSet, TextSearchSplitSyncAsync,
};
use je_workbench::test::common::workbench_test_services::{FileStatOptions, create_file_stat};

/// `fileService.x is not a function`.
fn not_mocked(method: &str) -> ! {
    errors::throw(Error::type_error(format!("fileService.{method} is not a function")))
}

/// `TestFileService`: o `stat` do teste.
pub struct LinkTestFileService {
    /// `_files: URI[] | '*'`: `None` é o `'*'`.
    files: RefCell<Option<Vec<URI>>>,
    /// `true` no `stat` do `terminalUriLinkDetector.test.ts` (`validResources.map(e => e.path).includes(resource.path)`): só o
    /// caminho conta. `false` no dos demais (`_files.some(e => e.toString() === resource.toString())`).
    match_by_path: bool,
}

impl LinkTestFileService {
    pub fn new() -> Rc<Self> {
        Rc::new(Self { files: RefCell::new(None), match_by_path: false })
    }

    /// O `IFileService` de `terminalUriLinkDetector.test.ts`: existe o recurso cujo `path` está na lista (a lista começa vazia).
    pub fn with_path_matching() -> Rc<Self> {
        Rc::new(Self { files: RefCell::new(Some(Vec::new())), match_by_path: true })
    }

    /// `setFiles(files)`.
    pub fn set_files(&self, files: Vec<URI>) {
        *self.files.borrow_mut() = Some(files);
    }
}

impl IFileService for LinkTestFileService {
    fn on_did_files_change(&self) -> Event<Rc<FileChangesEvent>> {
        not_mocked("onDidFilesChange")
    }

    fn get_provider(&self, _scheme: &str) -> Option<FileSystemProviderRef> {
        not_mocked("getProvider")
    }

    fn resolve_with_metadata(&self, _resource: &URI, _options: Option<ResolveFileOptions>) -> Promise<FileStatWithMetadata> {
        not_mocked("resolve")
    }

    /// `override async stat(resource)`.
    fn stat(&self, resource: &URI) -> Promise<FileStatWithMetadata> {
        let exists = match &*self.files.borrow() {
            None => true,
            Some(files) if self.match_by_path => files.iter().any(|file| file.path() == resource.path()),
            Some(files) => files.iter().any(|file| file.to_string(false) == resource.to_string(false)),
        };
        if exists {
            Promise::resolved(create_file_stat(
                resource,
                &FileStatOptions { is_file: Some(true), is_directory: Some(false), is_symbolic_link: Some(false), ..FileStatOptions::default() },
            ))
        } else {
            Promise::rejected(Error::new("ENOENT"))
        }
    }

    fn exists(&self, _resource: &URI) -> Promise<bool> {
        not_mocked("exists")
    }

    fn on_did_change_file_system_provider_registrations(&self) -> Event<FileSystemProviderRegistrationEvent> {
        Event::none()
    }

    fn on_did_change_file_system_provider_capabilities(&self) -> Event<FileSystemProviderCapabilitiesChangeEvent> {
        Event::none()
    }

    fn has_provider(&self, _resource: &URI) -> bool {
        false
    }

    fn has_capability(&self, _resource: &URI, _capability: FileSystemProviderCapabilities) -> bool {
        false
    }

    fn read_file(&self, _resource: &URI, _options: Option<ReadFileOptions>, _token: Option<CancellationToken>) -> Promise<FileContent> {
        not_mocked("readFile")
    }

    fn resolve(&self, _resource: &URI, _options: Option<ResolveFileOptions>) -> Promise<FileStat> {
        not_mocked("resolve")
    }

    fn create_folder(&self, _resource: &URI) -> Promise<FileStatWithMetadata> {
        not_mocked("createFolder")
    }

    fn watch(&self, _resource: &URI, _options: Option<WatchOptions>) -> DisposableRef {
        not_mocked("watch")
    }

    fn read_file_stream(&self, _resource: &URI, _options: Option<ReadFileOptions>, _token: Option<CancellationToken>) -> Promise<FileStreamContent> {
        not_mocked("readFileStream")
    }

    fn write_file(&self, _resource: &URI, _content: FileContentSource, _options: Option<WriteFileOptions>) -> Promise<FileStatWithMetadata> {
        not_mocked("writeFile")
    }

    fn del(&self, _resource: &URI, _options: Option<PartialFileDeleteOptions>) -> Promise<()> {
        not_mocked("del")
    }

    fn on_did_run_operation(&self) -> Event<FileOperationEvent> {
        not_mocked("onDidRunOperation")
    }

    fn create_file(&self, _resource: &URI, _content: Option<FileContentSource>, _options: Option<CreateFileOptions>) -> Promise<FileStatWithMetadata> {
        not_mocked("createFile")
    }

    fn on_will_activate_file_system_provider(&self) -> Event<FileSystemProviderActivationEvent> {
        not_mocked("onWillActivateFileSystemProvider")
    }

    fn register_provider(&self, _scheme: &str, _provider: FileSystemProviderRef) -> Result<DisposableRef, Error> {
        not_mocked("registerProvider")
    }
}

/// `searchService.fileSearch(...)` do teste.
pub struct TestSearchService {
    search_result: RefCell<Option<ISearchComplete>>,
}

impl TestSearchService {
    pub fn new() -> Rc<Self> {
        Rc::new(Self { search_result: RefCell::new(None) })
    }

    /// `setSearchResult(result)`.
    pub fn set_search_result(&self, result: ISearchComplete) {
        *self.search_result.borrow_mut() = Some(result);
    }
}

fn search_not_mocked(method: &str) -> ! {
    errors::throw(Error::type_error(format!("searchService.{method} is not a function")))
}

impl ISearchService for TestSearchService {
    fn text_search(&self, _query: &ITextQuery, _token: Option<CancellationToken>, _on_progress: Option<SearchProgressCallback>) -> Promise<ISearchComplete> {
        search_not_mocked("textSearch")
    }

    fn ai_text_search(&self, _query: &IAITextQuery, _token: Option<CancellationToken>, _on_progress: Option<SearchProgressCallback>) -> Promise<ISearchComplete> {
        search_not_mocked("aiTextSearch")
    }

    fn get_ai_name(&self) -> Promise<Option<String>> {
        search_not_mocked("getAIName")
    }

    fn text_search_split_sync_async(
        &self,
        _query: &ITextQuery,
        _token: Option<CancellationToken>,
        _on_progress: Option<SearchProgressCallback>,
        _notebook_files_to_ignore: Option<SharedResourceSet>,
        _async_notebook_files_to_ignore: Option<Promise<SharedResourceSet>>,
    ) -> TextSearchSplitSyncAsync {
        search_not_mocked("textSearchSplitSyncAsync")
    }

    /// `override async fileSearch(query) { return this._searchResult!; }`: sem resultado, o chamador lê `results` de
    /// `undefined` e leva um `TypeError`.
    fn file_search(&self, _query: &IFileQuery, _token: Option<CancellationToken>) -> Promise<ISearchComplete> {
        match &*self.search_result.borrow() {
            Some(result) => Promise::resolved(result.clone()),
            None => Promise::rejected(Error::type_error("Cannot read properties of undefined (reading 'results')".to_string())),
        }
    }

    fn scheme_has_file_search_provider(&self, _scheme: &str) -> bool {
        search_not_mocked("schemeHasFileSearchProvider")
    }

    fn clear_cache(&self, _cache_key: &str) -> Promise<()> {
        search_not_mocked("clearCache")
    }

    fn register_search_result_provider(&self, _scheme: &str, _provider_type: SearchProviderType, _provider: Rc<dyn ISearchResultProvider>) -> DisposableRef {
        search_not_mocked("registerSearchResultProvider")
    }
}

/// `{ file: () => null! }`: o `QueryBuilder.file` que devolve uma consulta vazia.
pub struct StubFileQueryBuilder;

impl IFileQueryBuilder for StubFileQueryBuilder {
    fn file(&self, _folders: &[WorkspaceFolderOrUri], _options: &IFileQueryBuilderOptions) -> IFileQuery {
        IFileQueryProps::new(Vec::new())
    }
}

/// `ITerminalLinkActivationResult`.
#[derive(Clone, Debug, PartialEq)]
pub struct TerminalLinkActivationResult {
    /// `'editor' | 'search'`.
    pub source: &'static str,
    pub link: String,
    pub selection: Option<TextEditorSelection>,
}

/// O `activationResult` compartilhado entre o teste e os dublês.
pub type ActivationResult = Rc<RefCell<Option<TerminalLinkActivationResult>>>;

/// `{ quickAccess: { show(link) { activationResult = { link, source: 'search' } } } }`.
pub struct RecordingQuickAccess {
    pub activation_result: ActivationResult,
}

impl IQuickAccessController for RecordingQuickAccess {
    fn show(&self, value: Option<&str>) {
        *self.activation_result.borrow_mut() =
            Some(TerminalLinkActivationResult { link: value.unwrap_or_default().to_owned(), source: "search", selection: None });
    }
}

/// `{ async openEditor(editor) { activationResult = { source: 'editor', link: editor.resource?.toString() } ... } }`.
pub struct RecordingEditorService {
    pub activation_result: ActivationResult,
}

impl IEditorService for RecordingEditorService {
    fn visible_editor_panes(&self) -> Vec<Rc<dyn IEditorPane>> {
        unreachable!("o stub do IEditorService só define openEditor")
    }

    fn active_editor(&self) -> Option<EditorInputOrUntyped> {
        unreachable!("o stub do IEditorService só define openEditor")
    }

    fn active_text_editor_control(&self) -> Option<ActiveTextEditorControl> {
        unreachable!("o stub do IEditorService só define openEditor")
    }

    fn active_text_editor_language_id(&self) -> Option<String> {
        unreachable!("o stub do IEditorService só define openEditor")
    }

    fn open_editor(&self, editor: UntypedEditorInput) -> Promise<Option<Rc<dyn IEditorPane>>> {
        let mut result = TerminalLinkActivationResult {
            source: "editor",
            link: editor.resource.as_ref().map(|resource| resource.to_string(false).to_string_lossy()).unwrap_or_default(),
            selection: None,
        };
        // Only assert on selection if it's not the default value
        if let Some(selection) = editor.options.as_ref().and_then(|options| options.selection)
            && (selection.start_column != 1.0 || selection.start_line_number != 1.0)
        {
            result.selection = Some(selection);
        }
        *self.activation_result.borrow_mut() = Some(result);
        Promise::resolved(None)
    }

    fn find_editors(&self, _resource: &URI, _options: Option<FindEditorOptions>) -> Vec<EditorIdentifier> {
        unreachable!("o stub do IEditorService só define openEditor")
    }

    fn save(&self, _editors: Vec<EditorIdentifier>, _options: Option<SaveEditorsOptions>) -> Promise<SaveEditorsResult> {
        unreachable!("o stub do IEditorService só define openEditor")
    }

    fn save_all(&self, _options: Option<SaveAllEditorsOptions>) -> Promise<SaveEditorsResult> {
        unreachable!("o stub do IEditorService só define openEditor")
    }

    fn get_editors(&self, _order: EditorsOrder) -> Vec<EditorIdentifier> {
        unreachable!("o stub do IEditorService só define openEditor")
    }

    fn open_editors(&self, _editors: Vec<EditorInputWithOptions>) -> Promise<Vec<Rc<dyn IEditorPane>>> {
        unreachable!("o stub do IEditorService só define openEditor")
    }

    fn is_visible(&self, _editor: &EditorInputRef) -> bool {
        unreachable!("o stub do IEditorService só define openEditor")
    }
}

/// `assertLinkHelper(text, expected, detector, expectedType)` de `linkTestUtils.ts`: escreve o texto no terminal, monta as linhas
/// até a do cursor, detecta sempre na última linha com conteúdo e compara `{ link, type, bufferRange }`. `expected` é a lista de
/// `(texto ou uri, [início, fim])`, com o intervalo em coordenadas (x, y) de 1.
pub async fn assert_link_helper(
    xterm: &FakeXterm,
    detector: &dyn ITerminalLinkDetector,
    text: &str,
    expected: &[(String, [(i32, i32); 2])],
    expected_type: TerminalBuiltinLinkType,
) {
    xterm.reset();

    // Write the text and wait for the parser to finish
    xterm.write(text);
    let text_split: Vec<&str> = text.split("\r\n").collect();
    let last_line_index: i32 = text_split[..text_split.len() - 1].iter().map(|c| ((c.encode_utf16().count() as f64 / 80.0).ceil() as i32).max(1)).sum();

    // Ensure all links are provided
    let cursor_y = xterm.cursor_y();
    let buffer = xterm.buffer();
    let lines: Vec<Rc<dyn IBufferLine>> = (0..=cursor_y).map(|i| IBuffer::get_line(&buffer, i).expect("a linha existe")).collect();

    // Detect links always on the last line with content
    let actual_links: Vec<(String, TerminalBuiltinLinkType, IBufferRange)> = detector
        .detect(&lines, last_line_index, cursor_y)
        .await
        .unwrap()
        .into_iter()
        .map(|e| (e.uri.as_ref().map_or_else(|| e.text.clone(), |uri| uri.to_string(false).to_string_lossy()), expected_type, e.buffer_range))
        .collect();
    let expected_links: Vec<(String, TerminalBuiltinLinkType, IBufferRange)> = expected
        .iter()
        .map(|(link, range)| {
            (
                link.clone(),
                expected_type,
                IBufferRange { start: IBufferCellPosition { x: range[0].0, y: range[0].1 }, end: IBufferCellPosition { x: range[1].0, y: range[1].1 } },
            )
        })
        .collect();
    assert_eq!(actual_links, expected_links, "links de \"{text}\"");
}
