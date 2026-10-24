//! Porte de `src/vs/workbench/api/test/browser/extHostWorkspace.test.ts` (suíte `ExtHostWorkspace`, com as subsuítes
//! `findFiles`, `findFiles2`, `findTextInFiles` e `findTextInFiles2`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `new TestRPCProtocol()`, `createExtHostWorkspace(...)` e os `mock<...>()` são o `TestMainContext` e os dublês de
//!   `support::ext_host_workspace_support`: um método sem resposta falha com `panic!`, como o método ausente do objeto
//!   vazio do original lançaria `TypeError`. O `getProxy: () => undefined!` do caso `valid arguments` é o dublê padrão
//!   (o `ExtHostWorkspace` de Rust sempre tem proxy, e o padrão de `$updateWorkspaceFolders` resolve).
//! - `ensureNoDisposablesAreLeakedInTestSuite()` não é usado: o `ExtHostWorkspace` do original não é descartável e os
//!   emissores dele nunca são descartados.
//! - O `done` com `try/catch` dos casos de evento vira a captura do evento ([`accept`]), conferido depois do
//!   `$acceptWorkspaceData` que o dispara; o ouvinte do emissor roda dentro de um `catch` (um `assert!` nele seria engolido).
//!   O que o original confere dentro do ouvinte é conferido no evento capturado, que tem de ser um só.
//! - `Multiroot change event is immutable`: `(<any>e).added = []` lança no original. O evento de Rust não tem
//!   setter (a escrita seria erro de compilação), então o caso confere que o evento chega com as listas dadas.
//! - `Multiroot change keeps existing workspaces live`: o último `notStrictEqual(firstFolder, ws.workspace.folders[0])`
//!   compara a pasta de extensão com a pasta de plataforma, que são tipos diferentes; o caso confere o que ele quer
//!   dizer: a pasta antiga não voltou à vida (`Rc::ptr_eq` falso) e a `folders[0]` é a `foo:bar3`.
//! - `updateWorkspaceFolders(extensionDescriptor, null!, null!)` é `(None, None)`.
//! - `assert.strictEqual(e.added[0], firstAddedFolder)` é [`Rc::ptr_eq`].
//! - Os casos de busca rodam em `run_with_faked_timers`; as asserções dentro do dublê de `$startFileSearch` e
//!   `$startTextSearch` viram rejeição da busca (o `expect` do caso a mostra), como no original.
//! - `assert.deepStrictEqual(URI.from(_includeFolder).toJSON(), URI.file('/other/folder').toJSON())` é a igualdade
//!   de [`URI`]; `assert.strictEqual(options.excludePattern?.length, 1)` e `options.excludePattern[0].pattern` são a
//!   lista de [`SearchPatternBuilder`].
//! - `assert.strictEqual(options.includePattern, 'foo')` é `Some(SearchPathPatternBuilder::Single("foo"))`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::run_with_faked_timers;
use je_base::cancellation::CancellationToken;
use je_base::lifecycle::IDisposable;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_extensions::manifest::{ExtensionIdentifier, ExtensionManifest, TargetPlatform};
use je_extensions::scanner::ExtensionDescription;
use je_platform::resources::uri_file;
use je_workbench::api::common::ext_host_protocol::{
    ExtHostWorkspaceShape, ITextSearchComplete, IWorkspaceData, MainContext, MainThreadWorkspaceShape, WorkspaceFolderDataDto, WorkspaceFolderToAdd,
};
use je_workbench::api::common::ext_host_workspace::{
    ExtHostWorkspace, FindFiles2Options, FindFilesExclude, FindTextInFilesOptions, FindTextInFilesOptions2, GlobPattern, IExtHostWorkspaceProvider,
    RelativePattern, RelativePathInput, UseIgnoreFilesOptions, WorkspaceFolder, WorkspaceFoldersChangeEvent,
};
use je_workbench::services::search::query_builder::{SearchPathPatternBuilder, SearchPatternBuilder};
use je_workbench::services::search::search::PatternInfo;
use je_workbench::services::search::search_ext_types::{ExcludeSettingOptions, TextSearchQuery2};
use support::ext_host_workspace_support::{
    MockMainThreadWorkspace, TestMainContext, a_workspace_folder_data, create_ext_host_workspace, new_ext_host_workspace,
};

/// `nullExtensionDescription` de `services/extensions/common/extensions.ts`.
fn extension_descriptor() -> ExtensionDescription {
    ExtensionDescription {
        id: "nullExtensionDescription".to_owned(),
        identifier: ExtensionIdentifier::new("nullExtensionDescription"),
        is_builtin: false,
        is_user_builtin: false,
        is_under_development: false,
        extension_location: "void:location".to_owned(),
        uuid: None,
        target_platform: TargetPlatform::Undefined,
        publisher_display_name: None,
        pre_release: false,
        manifest: ExtensionManifest {
            name: "Null Extension Description".to_owned(),
            version: "0.0.0".to_owned(),
            publisher: "vscode".to_owned(),
            ..ExtensionManifest::default()
        },
    }
}

fn parse(text: &str) -> URI {
    URI::parse(text, false)
}

/// `{ id: 'foo', name: 'Test', folders }`.
fn data(folders: Vec<WorkspaceFolderDataDto>) -> IWorkspaceData {
    IWorkspaceData { id: "foo".to_owned(), name: "Test".to_owned(), folders, ..IWorkspaceData::default() }
}

/// `aWorkspaceFolderData(uri, index, name = '')`.
fn folder_data(uri: URI, index: usize, name: &str) -> WorkspaceFolderDataDto {
    a_workspace_folder_data(uri, index, name)
}

/// `asUpdateWorkspaceFolderData(uri, name?)`.
fn as_update_workspace_folder_data(uri: URI, name: Option<&str>) -> WorkspaceFolderToAdd {
    WorkspaceFolderToAdd { uri, name: name.map(str::to_owned) }
}

fn create(data: Option<IWorkspaceData>) -> Rc<ExtHostWorkspace> {
    create_ext_host_workspace(&TestMainContext::new(), data)
}

/// `ws.$acceptWorkspaceData(data)` e o evento que ele dispara (um só).
fn accept(ws: &Rc<ExtHostWorkspace>, data: IWorkspaceData) -> WorkspaceFoldersChangeEvent {
    let events: Rc<RefCell<Vec<WorkspaceFoldersChangeEvent>>> = Rc::default();
    let sink = events.clone();
    let sub = ws.on_did_change_workspace().listen(move |e| sink.borrow_mut().push(e.clone()));
    ws.rpc_accept_workspace_data(Some(data));
    sub.dispose();
    let mut events = events.borrow_mut();
    assert_eq!(events.len(), 1, "gotEvent");
    events.remove(0)
}

fn uri_texts(folders: &[Rc<WorkspaceFolder>]) -> Vec<String> {
    folders.iter().map(|folder| folder.uri().to_string(false).to_string_lossy()).collect()
}

fn assert_as_relative_path(workspace: &Rc<ExtHostWorkspace>, input: &str, expected: &str, include_workspace: Option<bool>) {
    let actual = workspace.get_relative_path(RelativePathInput::from(input), include_workspace);
    assert_eq!(actual, expected);
}

#[test]
fn as_relative_path() {
    let ws = create(Some(data(vec![folder_data(uri_file("/Coding/Applications/NewsWoWBot"), 0, "")])));

    assert_as_relative_path(&ws, "/Coding/Applications/NewsWoWBot/bernd/das/brot", "bernd/das/brot", None);
    assert_as_relative_path(
        &ws,
        "/Apps/DartPubCache/hosted/pub.dartlang.org/convert-2.0.1/lib/src/hex.dart",
        "/Apps/DartPubCache/hosted/pub.dartlang.org/convert-2.0.1/lib/src/hex.dart",
        None,
    );

    assert_as_relative_path(&ws, "", "", None);
    assert_as_relative_path(&ws, "/foo/bar", "/foo/bar", None);
    assert_as_relative_path(&ws, "in/out", "in/out", None);
}

#[test]
fn as_relative_path_same_paths_11402() {
    let root = "/home/aeschli/workspaces/samples/docker";
    let input = "/home/aeschli/workspaces/samples/docker";
    let ws = create(Some(data(vec![folder_data(uri_file(root), 0, "")])));

    assert_as_relative_path(&ws, input, input, None);

    let input2 = "/home/aeschli/workspaces/samples/docker/a.file";
    assert_as_relative_path(&ws, input2, "a.file", None);
}

#[test]
fn as_relative_path_no_workspace() {
    let ws = create(None);
    assert_as_relative_path(&ws, "", "", None);
    assert_as_relative_path(&ws, "/foo/bar", "/foo/bar", None);
}

#[test]
fn as_relative_path_multiple_folders() {
    let ws = create(Some(data(vec![folder_data(uri_file("/Coding/One"), 0, ""), folder_data(uri_file("/Coding/Two"), 1, "")])));
    assert_as_relative_path(&ws, "/Coding/One/file.txt", "One/file.txt", None);
    assert_as_relative_path(&ws, "/Coding/Two/files/out.txt", "Two/files/out.txt", None);
    assert_as_relative_path(&ws, "/Coding/Two2/files/out.txt", "/Coding/Two2/files/out.txt", None);
}

#[test]
fn slightly_inconsistent_behaviour_of_as_relative_path_and_get_workspace_folder_31553() {
    let mrws = create(Some(data(vec![folder_data(uri_file("/Coding/One"), 0, ""), folder_data(uri_file("/Coding/Two"), 1, "")])));

    assert_as_relative_path(&mrws, "/Coding/One/file.txt", "One/file.txt", None);
    assert_as_relative_path(&mrws, "/Coding/One/file.txt", "One/file.txt", Some(true));
    assert_as_relative_path(&mrws, "/Coding/One/file.txt", "file.txt", Some(false));
    assert_as_relative_path(&mrws, "/Coding/Two/files/out.txt", "Two/files/out.txt", None);
    assert_as_relative_path(&mrws, "/Coding/Two/files/out.txt", "Two/files/out.txt", Some(true));
    assert_as_relative_path(&mrws, "/Coding/Two/files/out.txt", "files/out.txt", Some(false));
    assert_as_relative_path(&mrws, "/Coding/Two2/files/out.txt", "/Coding/Two2/files/out.txt", None);
    assert_as_relative_path(&mrws, "/Coding/Two2/files/out.txt", "/Coding/Two2/files/out.txt", Some(true));
    assert_as_relative_path(&mrws, "/Coding/Two2/files/out.txt", "/Coding/Two2/files/out.txt", Some(false));

    let srws = create(Some(data(vec![folder_data(uri_file("/Coding/One"), 0, "")])));
    assert_as_relative_path(&srws, "/Coding/One/file.txt", "file.txt", None);
    assert_as_relative_path(&srws, "/Coding/One/file.txt", "file.txt", Some(false));
    assert_as_relative_path(&srws, "/Coding/One/file.txt", "One/file.txt", Some(true));
    assert_as_relative_path(&srws, "/Coding/Two2/files/out.txt", "/Coding/Two2/files/out.txt", None);
    assert_as_relative_path(&srws, "/Coding/Two2/files/out.txt", "/Coding/Two2/files/out.txt", Some(true));
    assert_as_relative_path(&srws, "/Coding/Two2/files/out.txt", "/Coding/Two2/files/out.txt", Some(false));
}

#[test]
fn get_path_legacy() {
    let mut ws = create(Some(data(Vec::new())));
    assert_eq!(ws.get_path(), None);

    // `null!` e `undefined!`
    ws = create(None);
    assert_eq!(ws.get_path(), None);

    ws = create(None);
    assert_eq!(ws.get_path(), None);

    ws = create(Some(data(vec![folder_data(uri_file("Folder"), 0, ""), folder_data(uri_file("Another/Folder"), 1, "")])));
    assert_eq!(ws.get_path().unwrap().replace('\\', "/"), "/Folder");

    ws = create(Some(data(vec![folder_data(uri_file("/Folder"), 0, "")])));
    assert_eq!(ws.get_path().unwrap().replace('\\', "/"), "/Folder");
}

#[test]
fn workspace_folder_has_name_and_index() {
    let ws = create(Some(data(vec![folder_data(uri_file("/Coding/One"), 0, ""), folder_data(uri_file("/Coding/Two"), 1, "")])));

    let folders = ws.get_workspace_folders().unwrap();
    let (one, two) = (&folders[0], &folders[1]);

    assert_eq!(one.name(), "One");
    assert_eq!(one.index(), 0);
    assert_eq!(two.name(), "Two");
    assert_eq!(two.index(), 1);
}

#[test]
fn get_containing_workspace_folder() {
    let ws = create(Some(data(vec![
        folder_data(uri_file("/Coding/One"), 0, ""),
        folder_data(uri_file("/Coding/Two"), 1, ""),
        folder_data(uri_file("/Coding/Two/Nested"), 2, ""),
    ])));

    let mut folder = ws.get_workspace_folder(&uri_file("/foo/bar"), None);
    assert!(folder.is_none());

    folder = ws.get_workspace_folder(&uri_file("/Coding/One/file/path.txt"), None);
    assert_eq!(folder.unwrap().name(), "One");

    folder = ws.get_workspace_folder(&uri_file("/Coding/Two/file/path.txt"), None);
    assert_eq!(folder.unwrap().name(), "Two");

    folder = ws.get_workspace_folder(&uri_file("/Coding/Two/Nest"), None);
    assert_eq!(folder.unwrap().name(), "Two");

    folder = ws.get_workspace_folder(&uri_file("/Coding/Two/Nested/file"), None);
    assert_eq!(folder.unwrap().name(), "Nested");

    folder = ws.get_workspace_folder(&uri_file("/Coding/Two/Nested/f"), None);
    assert_eq!(folder.unwrap().name(), "Nested");

    folder = ws.get_workspace_folder(&uri_file("/Coding/Two/Nested"), Some(true));
    assert_eq!(folder.unwrap().name(), "Two");

    folder = ws.get_workspace_folder(&uri_file("/Coding/Two/Nested/"), Some(true));
    assert_eq!(folder.unwrap().name(), "Two");

    folder = ws.get_workspace_folder(&uri_file("/Coding/Two/Nested"), None);
    assert_eq!(folder.unwrap().name(), "Nested");

    folder = ws.get_workspace_folder(&uri_file("/Coding/Two/Nested/"), None);
    assert_eq!(folder.unwrap().name(), "Nested");

    folder = ws.get_workspace_folder(&uri_file("/Coding/Two"), Some(true));
    assert!(folder.is_none());

    folder = ws.get_workspace_folder(&uri_file("/Coding/Two"), Some(false));
    assert_eq!(folder.unwrap().name(), "Two");
}

#[test]
fn multiroot_change_event_should_have_a_delta_29641() {
    let ws = create(Some(data(Vec::new())));

    let e = accept(&ws, data(Vec::new()));
    assert!(e.added.is_empty());
    assert!(e.removed.is_empty());

    let e = accept(&ws, data(vec![folder_data(parse("foo:bar"), 0, "")]));
    assert!(e.removed.is_empty());
    assert_eq!(e.added.len(), 1);
    assert_eq!(e.added[0].uri().to_string(false), "foo:bar");

    let e = accept(&ws, data(vec![folder_data(parse("foo:bar"), 0, ""), folder_data(parse("foo:bar2"), 1, "")]));
    assert!(e.removed.is_empty());
    assert_eq!(e.added.len(), 1);
    assert_eq!(e.added[0].uri().to_string(false), "foo:bar2");

    let e = accept(&ws, data(vec![folder_data(parse("foo:bar3"), 0, "")]));
    assert_eq!(e.removed.len(), 2);
    assert_eq!(e.removed[0].uri().to_string(false), "foo:bar");
    assert_eq!(e.removed[1].uri().to_string(false), "foo:bar2");

    assert_eq!(e.added.len(), 1);
    assert_eq!(e.added[0].uri().to_string(false), "foo:bar3");
}

#[test]
fn multiroot_change_keeps_existing_workspaces_live() {
    let ws = create(Some(data(vec![folder_data(parse("foo:bar"), 0, "")])));

    let first_folder = ws.get_workspace_folders().unwrap()[0].clone();
    ws.rpc_accept_workspace_data(Some(data(vec![folder_data(parse("foo:bar2"), 0, ""), folder_data(parse("foo:bar"), 1, "renamed")])));

    assert!(Rc::ptr_eq(&ws.get_workspace_folders().unwrap()[1], &first_folder));
    assert_eq!(first_folder.index(), 1);
    assert_eq!(first_folder.name(), "renamed");

    ws.rpc_accept_workspace_data(Some(data(vec![folder_data(parse("foo:bar3"), 0, ""), folder_data(parse("foo:bar2"), 1, ""), folder_data(parse("foo:bar"), 2, "")])));
    assert!(Rc::ptr_eq(&ws.get_workspace_folders().unwrap()[2], &first_folder));
    assert_eq!(first_folder.index(), 2);

    ws.rpc_accept_workspace_data(Some(data(vec![folder_data(parse("foo:bar3"), 0, "")])));
    ws.rpc_accept_workspace_data(Some(data(vec![folder_data(parse("foo:bar3"), 0, ""), folder_data(parse("foo:bar"), 1, "")])));

    assert!(!Rc::ptr_eq(&first_folder, &ws.get_workspace_folders().unwrap()[1]));
    assert_eq!(ws.workspace().unwrap().folders()[0].uri.to_string(false), "foo:bar3");
}

#[test]
fn update_workspace_folders_invalid_arguments() {
    let extension = extension_descriptor();
    let mut ws = create(Some(data(Vec::new())));

    assert!(!ws.update_workspace_folders(&extension, None, None, Vec::new()));
    assert!(!ws.update_workspace_folders(&extension, Some(0.0), Some(0.0), Vec::new()));
    assert!(!ws.update_workspace_folders(&extension, Some(0.0), Some(1.0), Vec::new()));
    assert!(!ws.update_workspace_folders(&extension, Some(1.0), Some(0.0), Vec::new()));
    assert!(!ws.update_workspace_folders(&extension, Some(-1.0), Some(0.0), Vec::new()));
    assert!(!ws.update_workspace_folders(&extension, Some(-1.0), Some(-1.0), Vec::new()));

    ws = create(Some(data(vec![folder_data(parse("foo:bar"), 0, "")])));

    assert!(!ws.update_workspace_folders(&extension, Some(1.0), Some(1.0), Vec::new()));
    assert!(!ws.update_workspace_folders(&extension, Some(0.0), Some(2.0), Vec::new()));
    assert!(!ws.update_workspace_folders(&extension, Some(0.0), Some(1.0), vec![as_update_workspace_folder_data(parse("foo:bar"), None)]));
}

#[test]
fn update_workspace_folders_valid_arguments() {
    let extension = extension_descriptor();
    let ws = create(Some(data(Vec::new())));

    //
    // Add one folder
    //

    assert!(ws.update_workspace_folders(&extension, Some(0.0), Some(0.0), vec![as_update_workspace_folder_data(parse("foo:bar"), None)]));
    assert_eq!(1, ws.workspace().unwrap().folders().len());
    assert_eq!(ws.workspace().unwrap().folders()[0].uri.to_string(false), parse("foo:bar").to_string(false));

    let first_added_folder = ws.get_workspace_folders().unwrap()[0].clone();

    // simulate acknowledgement from main side
    let e = accept(&ws, data(vec![folder_data(parse("foo:bar"), 0, "")]));
    assert!(e.removed.is_empty());
    assert_eq!(e.added.len(), 1);
    assert_eq!(e.added[0].uri().to_string(false), "foo:bar");
    assert!(Rc::ptr_eq(&e.added[0], &first_added_folder)); // verify object is still live
    assert!(Rc::ptr_eq(&ws.get_workspace_folders().unwrap()[0], &first_added_folder)); // verify object is still live

    //
    // Add two more folders
    //

    assert!(ws.update_workspace_folders(
        &extension,
        Some(1.0),
        Some(0.0),
        vec![as_update_workspace_folder_data(parse("foo:bar1"), None), as_update_workspace_folder_data(parse("foo:bar2"), None)]
    ));
    assert_eq!(3, ws.workspace().unwrap().folders().len());
    let folders = ws.workspace().unwrap().folders();
    assert_eq!(folders[0].uri.to_string(false), parse("foo:bar").to_string(false));
    assert_eq!(folders[1].uri.to_string(false), parse("foo:bar1").to_string(false));
    assert_eq!(folders[2].uri.to_string(false), parse("foo:bar2").to_string(false));

    let second_added_folder = ws.get_workspace_folders().unwrap()[1].clone();
    let third_added_folder = ws.get_workspace_folders().unwrap()[2].clone();

    // simulate acknowledgement from main side
    let e = accept(
        &ws,
        data(vec![folder_data(parse("foo:bar"), 0, ""), folder_data(parse("foo:bar1"), 1, ""), folder_data(parse("foo:bar2"), 2, "")]),
    );
    assert!(e.removed.is_empty());
    assert_eq!(e.added.len(), 2);
    assert_eq!(e.added[0].uri().to_string(false), "foo:bar1");
    assert_eq!(e.added[1].uri().to_string(false), "foo:bar2");
    assert!(Rc::ptr_eq(&e.added[0], &second_added_folder));
    assert!(Rc::ptr_eq(&e.added[1], &third_added_folder));
    assert!(Rc::ptr_eq(&ws.get_workspace_folders().unwrap()[0], &first_added_folder)); // verify object is still live
    assert!(Rc::ptr_eq(&ws.get_workspace_folders().unwrap()[1], &second_added_folder)); // verify object is still live
    assert!(Rc::ptr_eq(&ws.get_workspace_folders().unwrap()[2], &third_added_folder)); // verify object is still live

    //
    // Remove one folder
    //

    assert!(ws.update_workspace_folders(&extension, Some(2.0), Some(1.0), Vec::new()));
    assert_eq!(2, ws.workspace().unwrap().folders().len());
    let folders = ws.workspace().unwrap().folders();
    assert_eq!(folders[0].uri.to_string(false), parse("foo:bar").to_string(false));
    assert_eq!(folders[1].uri.to_string(false), parse("foo:bar1").to_string(false));

    // simulate acknowledgement from main side
    let e = accept(&ws, data(vec![folder_data(parse("foo:bar"), 0, ""), folder_data(parse("foo:bar1"), 1, "")]));
    assert!(e.added.is_empty());
    assert_eq!(e.removed.len(), 1);
    assert!(Rc::ptr_eq(&e.removed[0], &third_added_folder));
    assert!(Rc::ptr_eq(&ws.get_workspace_folders().unwrap()[0], &first_added_folder)); // verify object is still live
    assert!(Rc::ptr_eq(&ws.get_workspace_folders().unwrap()[1], &second_added_folder)); // verify object is still live

    //
    // Rename folder
    //

    assert!(ws.update_workspace_folders(
        &extension,
        Some(0.0),
        Some(2.0),
        vec![as_update_workspace_folder_data(parse("foo:bar"), Some("renamed 1")), as_update_workspace_folder_data(parse("foo:bar1"), Some("renamed 2"))]
    ));
    assert_eq!(2, ws.workspace().unwrap().folders().len());
    let folders = ws.workspace().unwrap().folders();
    assert_eq!(folders[0].uri.to_string(false), parse("foo:bar").to_string(false));
    assert_eq!(folders[1].uri.to_string(false), parse("foo:bar1").to_string(false));
    assert_eq!(folders[0].name, "renamed 1");
    assert_eq!(folders[1].name, "renamed 2");
    assert_eq!(ws.get_workspace_folders().unwrap()[0].name(), "renamed 1");
    assert_eq!(ws.get_workspace_folders().unwrap()[1].name(), "renamed 2");

    // simulate acknowledgement from main side
    let e = accept(&ws, data(vec![folder_data(parse("foo:bar"), 0, "renamed 1"), folder_data(parse("foo:bar1"), 1, "renamed 2")]));
    assert!(e.added.is_empty());
    assert_eq!(e.removed.len(), 0);
    assert!(Rc::ptr_eq(&ws.get_workspace_folders().unwrap()[0], &first_added_folder)); // verify object is still live
    assert!(Rc::ptr_eq(&ws.get_workspace_folders().unwrap()[1], &second_added_folder)); // verify object is still live
    assert_eq!(ws.workspace().unwrap().folders()[0].name, "renamed 1");
    assert_eq!(ws.workspace().unwrap().folders()[1].name, "renamed 2");
    assert_eq!(ws.get_workspace_folders().unwrap()[0].name(), "renamed 1");
    assert_eq!(ws.get_workspace_folders().unwrap()[1].name(), "renamed 2");

    //
    // Add and remove folders
    //

    assert!(ws.update_workspace_folders(
        &extension,
        Some(0.0),
        Some(2.0),
        vec![as_update_workspace_folder_data(parse("foo:bar3"), None), as_update_workspace_folder_data(parse("foo:bar4"), None)]
    ));
    assert_eq!(2, ws.workspace().unwrap().folders().len());
    let folders = ws.workspace().unwrap().folders();
    assert_eq!(folders[0].uri.to_string(false), parse("foo:bar3").to_string(false));
    assert_eq!(folders[1].uri.to_string(false), parse("foo:bar4").to_string(false));

    let fourth_added_folder = ws.get_workspace_folders().unwrap()[0].clone();
    let fifth_added_folder = ws.get_workspace_folders().unwrap()[1].clone();

    // simulate acknowledgement from main side
    let e = accept(&ws, data(vec![folder_data(parse("foo:bar3"), 0, ""), folder_data(parse("foo:bar4"), 1, "")]));
    assert_eq!(e.added.len(), 2);
    assert!(Rc::ptr_eq(&e.added[0], &fourth_added_folder));
    assert!(Rc::ptr_eq(&e.added[1], &fifth_added_folder));
    assert_eq!(e.removed.len(), 2);
    assert!(Rc::ptr_eq(&e.removed[0], &first_added_folder));
    assert!(Rc::ptr_eq(&e.removed[1], &second_added_folder));
    assert!(Rc::ptr_eq(&ws.get_workspace_folders().unwrap()[0], &fourth_added_folder)); // verify object is still live
    assert!(Rc::ptr_eq(&ws.get_workspace_folders().unwrap()[1], &fifth_added_folder)); // verify object is still live

    //
    // Swap folders
    //

    assert!(ws.update_workspace_folders(
        &extension,
        Some(0.0),
        Some(2.0),
        vec![as_update_workspace_folder_data(parse("foo:bar4"), None), as_update_workspace_folder_data(parse("foo:bar3"), None)]
    ));
    assert_eq!(2, ws.workspace().unwrap().folders().len());
    let folders = ws.workspace().unwrap().folders();
    assert_eq!(folders[0].uri.to_string(false), parse("foo:bar4").to_string(false));
    assert_eq!(folders[1].uri.to_string(false), parse("foo:bar3").to_string(false));

    assert!(Rc::ptr_eq(&ws.get_workspace_folders().unwrap()[0], &fifth_added_folder)); // verify object is still live
    assert!(Rc::ptr_eq(&ws.get_workspace_folders().unwrap()[1], &fourth_added_folder)); // verify object is still live

    // simulate acknowledgement from main side
    let e = accept(&ws, data(vec![folder_data(parse("foo:bar4"), 0, ""), folder_data(parse("foo:bar3"), 1, "")]));
    assert_eq!(e.added.len(), 0);
    assert_eq!(e.removed.len(), 0);
    assert!(Rc::ptr_eq(&ws.get_workspace_folders().unwrap()[0], &fifth_added_folder)); // verify object is still live
    assert!(Rc::ptr_eq(&ws.get_workspace_folders().unwrap()[1], &fourth_added_folder)); // verify object is still live
    assert_eq!(fifth_added_folder.index(), 0);
    assert_eq!(fourth_added_folder.index(), 1);

    //
    // Add one folder after the other without waiting for confirmation (not supported currently)
    //

    assert!(ws.update_workspace_folders(&extension, Some(2.0), Some(0.0), vec![as_update_workspace_folder_data(parse("foo:bar5"), None)]));

    assert_eq!(3, ws.workspace().unwrap().folders().len());
    let folders = ws.workspace().unwrap().folders();
    assert_eq!(folders[0].uri.to_string(false), parse("foo:bar4").to_string(false));
    assert_eq!(folders[1].uri.to_string(false), parse("foo:bar3").to_string(false));
    assert_eq!(folders[2].uri.to_string(false), parse("foo:bar5").to_string(false));

    let sixth_added_folder = ws.get_workspace_folders().unwrap()[2].clone();

    // simulate acknowledgement from main side
    let e = accept(
        &ws,
        data(vec![folder_data(parse("foo:bar4"), 0, ""), folder_data(parse("foo:bar3"), 1, ""), folder_data(parse("foo:bar5"), 2, "")]),
    );
    assert_eq!(e.added.len(), 1);
    assert!(Rc::ptr_eq(&e.added[0], &sixth_added_folder));

    assert!(Rc::ptr_eq(&ws.get_workspace_folders().unwrap()[0], &fifth_added_folder)); // verify object is still live
    assert!(Rc::ptr_eq(&ws.get_workspace_folders().unwrap()[1], &fourth_added_folder)); // verify object is still live
    assert!(Rc::ptr_eq(&ws.get_workspace_folders().unwrap()[2], &sixth_added_folder)); // verify object is still live
}

#[test]
fn multiroot_change_event_is_immutable() {
    let ws = create(Some(data(Vec::new())));
    let e = accept(&ws, data(Vec::new()));
    // `(<any>e).added = []` lança no original: o evento de Rust não tem setter
    assert!(e.added.is_empty());
    assert!(e.removed.is_empty());
}

#[test]
fn vscode_workspace_get_workspace_folder_file_does_not_return_workspace_folder_when_file_open_from_command_line_36221() {
    if cfg!(windows) {
        let ws = create(Some(data(vec![folder_data(uri_file("c:/Users/marek/Desktop/vsc_test/"), 0, "")])));

        assert!(ws.get_workspace_folder(&uri_file("c:/Users/marek/Desktop/vsc_test/a.txt"), None).is_some());
        assert!(ws.get_workspace_folder(&uri_file("C:/Users/marek/Desktop/vsc_test/b.txt"), None).is_some());
    }
}

// ---- findFiles -------------------------------------------------------------------------------------------

const ROOT: &str = "/project/foo";

fn test_extension() -> ExtensionIdentifier {
    ExtensionIdentifier::new("test")
}

/// `rpcProtocol.set(MainContext.MainThreadWorkspace, mock)` e o `createExtHostWorkspace` com a pasta `ROOT`.
fn workspace_with(mock: MockMainThreadWorkspace) -> Rc<ExtHostWorkspace> {
    let context = TestMainContext::new();
    context.set(&MainContext::MAIN_THREAD_WORKSPACE, Rc::new(mock) as Rc<dyn MainThreadWorkspaceShape>);
    create_ext_host_workspace(&context, Some(data(vec![folder_data(uri_file(ROOT), 0, "")])))
}

fn single(pattern: &str) -> Option<SearchPathPatternBuilder> {
    Some(SearchPathPatternBuilder::Single(pattern.to_owned()))
}

mod find_files {
    use super::*;

    #[test]
    fn string_include() {
        run_with_faked_timers(async move {
            let main_thread_called = Rc::new(Cell::new(false));
            let called = main_thread_called.clone();
            let ws = workspace_with(MockMainThreadWorkspace {
                start_file_search: Some(Box::new(move |include_folder, options, _token| {
                    called.set(true);
                    assert_eq!(options.common.include_pattern, single("foo"));
                    assert_eq!(include_folder, None);
                    assert_eq!(options.common.exclude_pattern, None);
                    assert_eq!(options.common.disregard_exclude_settings, Some(false));
                    assert_eq!(options.common.max_results, Some(10));
                    Promise::resolved(None)
                })),
                ..MockMainThreadWorkspace::default()
            });
            ws.find_files(Some(GlobPattern::from("foo")), FindFilesExclude::Undefined, Some(10), &test_extension(), CancellationToken::None).await.expect("busca");
            assert!(main_thread_called.get(), "mainThreadCalled");
        });
    }

    fn test_find_files_include(pattern: RelativePattern) {
        run_with_faked_timers(async move {
            let main_thread_called = Rc::new(Cell::new(false));
            let called = main_thread_called.clone();
            let ws = workspace_with(MockMainThreadWorkspace {
                start_file_search: Some(Box::new(move |include_folder, options, _token| {
                    called.set(true);
                    assert_eq!(options.common.include_pattern, single("glob/**"));
                    assert_eq!(include_folder, Some(uri_file("/other/folder")));
                    assert_eq!(options.common.exclude_pattern, None);
                    assert_eq!(options.common.disregard_exclude_settings, Some(false));
                    Promise::resolved(None)
                })),
                ..MockMainThreadWorkspace::default()
            });
            ws.find_files(Some(GlobPattern::from(pattern)), FindFilesExclude::Undefined, Some(10), &test_extension(), CancellationToken::None).await.expect("busca");
            assert!(main_thread_called.get(), "mainThreadCalled");
        });
    }

    #[test]
    fn relative_pattern_include_string() {
        test_find_files_include(RelativePattern::new("/other/folder", "glob/**"));
    }

    #[test]
    fn relative_pattern_include_uri() {
        test_find_files_include(RelativePattern::new(uri_file("/other/folder"), "glob/**"));
    }

    #[test]
    fn no_excludes() {
        run_with_faked_timers(async move {
            let main_thread_called = Rc::new(Cell::new(false));
            let called = main_thread_called.clone();
            let ws = workspace_with(MockMainThreadWorkspace {
                start_file_search: Some(Box::new(move |include_folder, options, _token| {
                    called.set(true);
                    assert_eq!(options.common.include_pattern, single("glob/**"));
                    assert_eq!(include_folder.unwrap().to_string(false), uri_file("/other/folder").to_string(false));
                    assert_eq!(options.common.exclude_pattern, None);
                    assert_eq!(options.common.disregard_exclude_settings, Some(true));
                    Promise::resolved(None)
                })),
                ..MockMainThreadWorkspace::default()
            });
            ws.find_files(
                Some(GlobPattern::from(RelativePattern::new("/other/folder", "glob/**"))),
                FindFilesExclude::Null,
                Some(10),
                &test_extension(),
                CancellationToken::None,
            )
            .await
            .expect("busca");
            assert!(main_thread_called.get(), "mainThreadCalled");
        });
    }

    #[test]
    fn with_cancelled_token() {
        run_with_faked_timers(async move {
            let main_thread_called = Rc::new(Cell::new(false));
            let called = main_thread_called.clone();
            let ws = workspace_with(MockMainThreadWorkspace {
                start_file_search: Some(Box::new(move |_include_folder, _options, _token| {
                    called.set(true);
                    Promise::resolved(None)
                })),
                ..MockMainThreadWorkspace::default()
            });

            let token = CancellationToken::Cancelled;
            ws.find_files(
                Some(GlobPattern::from(RelativePattern::new("/other/folder", "glob/**"))),
                FindFilesExclude::Null,
                Some(10),
                &test_extension(),
                token,
            )
            .await
            .expect("busca");
            assert!(!main_thread_called.get(), "!mainThreadCalled");
        });
    }

    #[test]
    fn relative_pattern_exclude() {
        run_with_faked_timers(async move {
            let main_thread_called = Rc::new(Cell::new(false));
            let called = main_thread_called.clone();
            let ws = workspace_with(MockMainThreadWorkspace {
                start_file_search: Some(Box::new(move |_include_folder, options, _token| {
                    called.set(true);
                    assert_eq!(options.common.disregard_exclude_settings, Some(false));
                    let exclude_pattern = options.common.exclude_pattern.expect("excludePattern");
                    assert_eq!(exclude_pattern.len(), 1);
                    assert_eq!(exclude_pattern[0].pattern, SearchPathPatternBuilder::Single("glob/**".to_owned())); // Note that the base portion is ignored, see #52651
                    Promise::resolved(None)
                })),
                ..MockMainThreadWorkspace::default()
            });
            ws.find_files(
                Some(GlobPattern::from("")),
                FindFilesExclude::Pattern(GlobPattern::from(RelativePattern::new(ROOT, "glob/**"))),
                Some(10),
                &test_extension(),
                CancellationToken::None,
            )
            .await
            .expect("busca");
            assert!(main_thread_called.get(), "mainThreadCalled");
        });
    }
}

// ---- findFiles2 ------------------------------------------------------------------------------------------

mod find_files2 {
    use super::*;

    #[test]
    fn string_include() {
        run_with_faked_timers(async move {
            let main_thread_called = Rc::new(Cell::new(false));
            let called = main_thread_called.clone();
            let ws = workspace_with(MockMainThreadWorkspace {
                start_file_search: Some(Box::new(move |include_folder, options, _token| {
                    called.set(true);
                    assert_eq!(options.file_pattern.as_deref(), Some("foo"));
                    assert_eq!(options.common.include_pattern, None);
                    assert_eq!(include_folder, None);
                    assert_eq!(options.common.exclude_pattern, None);
                    assert_eq!(options.common.disregard_exclude_settings, Some(false));
                    assert_eq!(options.common.max_results, Some(10));
                    Promise::resolved(None)
                })),
                ..MockMainThreadWorkspace::default()
            });
            ws.find_files2(
                vec![GlobPattern::from("foo")],
                FindFiles2Options { max_results: Some(10), use_exclude_settings: Some(ExcludeSettingOptions::FilesExclude), ..FindFiles2Options::default() },
                &test_extension(),
                CancellationToken::None,
            )
            .await
            .expect("busca");
            assert!(main_thread_called.get(), "mainThreadCalled");
        });
    }

    fn test_find_files2_include(pattern: Vec<RelativePattern>) {
        run_with_faked_timers(async move {
            let main_thread_called = Rc::new(Cell::new(false));
            let called = main_thread_called.clone();
            let ws = workspace_with(MockMainThreadWorkspace {
                start_file_search: Some(Box::new(move |include_folder, options, _token| {
                    called.set(true);
                    assert_eq!(options.file_pattern.as_deref(), Some("glob/**"));
                    assert_eq!(options.common.include_pattern, None);
                    assert_eq!(include_folder, Some(uri_file("/other/folder")));
                    assert_eq!(options.common.exclude_pattern, None);
                    assert_eq!(options.common.disregard_exclude_settings, Some(false));
                    Promise::resolved(None)
                })),
                ..MockMainThreadWorkspace::default()
            });
            ws.find_files2(
                pattern.into_iter().map(GlobPattern::from).collect(),
                FindFiles2Options { max_results: Some(10), ..FindFiles2Options::default() },
                &test_extension(),
                CancellationToken::None,
            )
            .await
            .expect("busca");
            assert!(main_thread_called.get(), "mainThreadCalled");
        });
    }

    #[test]
    fn relative_pattern_include_string() {
        test_find_files2_include(vec![RelativePattern::new("/other/folder", "glob/**")]);
    }

    #[test]
    fn relative_pattern_include_uri() {
        test_find_files2_include(vec![RelativePattern::new(uri_file("/other/folder"), "glob/**")]);
    }

    #[test]
    fn no_excludes() {
        run_with_faked_timers(async move {
            let main_thread_called = Rc::new(Cell::new(false));
            let called = main_thread_called.clone();
            let ws = workspace_with(MockMainThreadWorkspace {
                start_file_search: Some(Box::new(move |include_folder, options, _token| {
                    called.set(true);
                    assert_eq!(options.file_pattern.as_deref(), Some("glob/**"));
                    assert_eq!(options.common.include_pattern, None);
                    assert_eq!(include_folder.unwrap().to_string(false), uri_file("/other/folder").to_string(false));
                    assert_eq!(options.common.exclude_pattern, None);
                    assert_eq!(options.common.disregard_exclude_settings, Some(false));
                    Promise::resolved(None)
                })),
                ..MockMainThreadWorkspace::default()
            });
            ws.find_files2(
                vec![GlobPattern::from(RelativePattern::new("/other/folder", "glob/**"))],
                FindFiles2Options::default(),
                &test_extension(),
                CancellationToken::None,
            )
            .await
            .expect("busca");
            assert!(main_thread_called.get(), "mainThreadCalled");
        });
    }

    #[test]
    fn no_dups() {
        run_with_faked_timers(async move {
            let main_thread_called = Rc::new(Cell::new(false));
            let called = main_thread_called.clone();
            let ws = workspace_with(MockMainThreadWorkspace {
                start_file_search: Some(Box::new(move |_include_folder, options, _token| {
                    called.set(true);
                    assert_eq!(options.common.include_pattern, None);
                    assert_eq!(options.common.exclude_pattern, None);
                    assert_eq!(options.common.disregard_exclude_settings, Some(false));
                    Promise::resolved(Some(vec![uri_file(&format!("{ROOT}/main.py"))]))
                })),
                ..MockMainThreadWorkspace::default()
            });

            let uris = ws
                .find_files2(vec![GlobPattern::from("**/main.py"), GlobPattern::from("**/main.py/**")], FindFiles2Options::default(), &test_extension(), CancellationToken::None)
                .await
                .expect("busca");
            assert!(main_thread_called.get(), "mainThreadCalled");
            assert_eq!(uris.len(), 1);
            assert_eq!(uris[0].to_string(false), uri_file(&format!("{ROOT}/main.py")).to_string(false));
        });
    }

    #[test]
    fn with_cancelled_token() {
        run_with_faked_timers(async move {
            let main_thread_called = Rc::new(Cell::new(false));
            let called = main_thread_called.clone();
            let ws = workspace_with(MockMainThreadWorkspace {
                start_file_search: Some(Box::new(move |_include_folder, _options, _token| {
                    called.set(true);
                    Promise::resolved(None)
                })),
                ..MockMainThreadWorkspace::default()
            });

            let token = CancellationToken::Cancelled;
            ws.find_files2(vec![GlobPattern::from(RelativePattern::new("/other/folder", "glob/**"))], FindFiles2Options::default(), &test_extension(), token)
                .await
                .expect("busca");
            assert!(!main_thread_called.get(), "!mainThreadCalled");
        });
    }

    #[test]
    fn relative_pattern_exclude() {
        run_with_faked_timers(async move {
            let main_thread_called = Rc::new(Cell::new(false));
            let called = main_thread_called.clone();
            let ws = workspace_with(MockMainThreadWorkspace {
                start_file_search: Some(Box::new(move |_include_folder, options, _token| {
                    called.set(true);
                    assert_eq!(options.common.disregard_exclude_settings, Some(false));
                    let exclude_pattern = options.common.exclude_pattern.expect("excludePattern");
                    assert_eq!(exclude_pattern.len(), 1);
                    assert_eq!(exclude_pattern[0].pattern, SearchPathPatternBuilder::Single("glob/**".to_owned())); // Note that the base portion is ignored, see #52651
                    Promise::resolved(None)
                })),
                ..MockMainThreadWorkspace::default()
            });
            ws.find_files2(
                vec![GlobPattern::from("")],
                FindFiles2Options { exclude: Some(vec![GlobPattern::from(RelativePattern::new(ROOT, "glob/**"))]), ..FindFiles2Options::default() },
                &test_extension(),
                CancellationToken::None,
            )
            .await
            .expect("busca");
            assert!(main_thread_called.get(), "mainThreadCalled");
        });
    }

    #[test]
    fn use_ignore_files() {
        run_with_faked_timers(async move {
            let main_thread_called = Rc::new(Cell::new(false));
            let called = main_thread_called.clone();
            let ws = workspace_with(MockMainThreadWorkspace {
                start_file_search: Some(Box::new(move |_include_folder, options, _token| {
                    called.set(true);
                    assert_eq!(options.common.disregard_exclude_settings, Some(false));
                    assert_eq!(options.common.disregard_ignore_files, Some(false));
                    assert_eq!(options.common.disregard_global_ignore_files, Some(false));
                    assert_eq!(options.common.disregard_parent_ignore_files, Some(false));
                    Promise::resolved(None)
                })),
                ..MockMainThreadWorkspace::default()
            });
            ws.find_files2(
                vec![GlobPattern::from("")],
                FindFiles2Options {
                    use_ignore_files: Some(UseIgnoreFilesOptions { local: Some(true), parent: Some(true), global: Some(true) }),
                    ..FindFiles2Options::default()
                },
                &test_extension(),
                CancellationToken::None,
            )
            .await
            .expect("busca");
            assert!(main_thread_called.get(), "mainThreadCalled");
        });
    }

    #[test]
    fn use_symlinks() {
        run_with_faked_timers(async move {
            let main_thread_called = Rc::new(Cell::new(false));
            let called = main_thread_called.clone();
            let ws = workspace_with(MockMainThreadWorkspace {
                start_file_search: Some(Box::new(move |_include_folder, options, _token| {
                    called.set(true);
                    assert_eq!(options.common.ignore_symlinks, Some(false));
                    Promise::resolved(None)
                })),
                ..MockMainThreadWorkspace::default()
            });
            ws.find_files2(
                vec![GlobPattern::from("")],
                FindFiles2Options { follow_symlinks: Some(true), ..FindFiles2Options::default() },
                &test_extension(),
                CancellationToken::None,
            )
            .await
            .expect("busca");
            assert!(main_thread_called.get(), "mainThreadCalled");
        });
    }

    #[test]
    fn case_insensitive() {
        run_with_faked_timers(async move {
            let main_thread_called = Rc::new(Cell::new(false));
            let called = main_thread_called.clone();
            let ws = workspace_with(MockMainThreadWorkspace {
                start_file_search: Some(Box::new(move |_include_folder, options, _token| {
                    called.set(true);
                    assert_eq!(options.common.ignore_glob_case, Some(true));
                    Promise::resolved(None)
                })),
                ..MockMainThreadWorkspace::default()
            });
            ws.find_files2(
                vec![GlobPattern::from("")],
                FindFiles2Options { case_insensitive: Some(true), ..FindFiles2Options::default() },
                &test_extension(),
                CancellationToken::None,
            )
            .await
            .expect("busca");
            assert!(main_thread_called.get(), "mainThreadCalled");
        });
    }

    // todo: add tests with multiple filePatterns and excludes
}

// ---- findTextInFiles -------------------------------------------------------------------------------------

type TextSearchResultOf = Promise<Option<ITextSearchComplete>>;

mod find_text_in_files {
    use super::*;

    fn no_callback() -> Rc<dyn Fn(je_workbench::api::common::ext_host_workspace::TextSearchResult)> {
        Rc::new(|_| {})
    }

    #[test]
    fn no_include() {
        run_with_faked_timers(async move {
            let main_thread_called = Rc::new(Cell::new(false));
            let called = main_thread_called.clone();
            let ws = workspace_with(MockMainThreadWorkspace {
                start_text_search: Some(Box::new(move |query, folder, options, _request_id, _token| -> TextSearchResultOf {
                    called.set(true);
                    assert_eq!(query.pattern, "foo");
                    assert_eq!(folder, None);
                    assert_eq!(options.common.include_pattern, None);
                    assert_eq!(options.common.exclude_pattern, None);
                    Promise::resolved(None)
                })),
                ..MockMainThreadWorkspace::default()
            });
            ws.find_text_in_files(PatternInfo::new("foo"), FindTextInFilesOptions::default(), no_callback(), &test_extension(), CancellationToken::None).await.expect("busca");
            assert!(main_thread_called.get(), "mainThreadCalled");
        });
    }

    #[test]
    fn string_include() {
        run_with_faked_timers(async move {
            let main_thread_called = Rc::new(Cell::new(false));
            let called = main_thread_called.clone();
            let ws = workspace_with(MockMainThreadWorkspace {
                start_text_search: Some(Box::new(move |query, folder, options, _request_id, _token| -> TextSearchResultOf {
                    called.set(true);
                    assert_eq!(query.pattern, "foo");
                    assert_eq!(folder, None);
                    assert_eq!(options.common.include_pattern, single("**/files"));
                    assert_eq!(options.common.exclude_pattern, None);
                    Promise::resolved(None)
                })),
                ..MockMainThreadWorkspace::default()
            });
            ws.find_text_in_files(
                PatternInfo::new("foo"),
                FindTextInFilesOptions { include: Some(GlobPattern::from("**/files")), ..FindTextInFilesOptions::default() },
                no_callback(),
                &test_extension(),
                CancellationToken::None,
            )
            .await
            .expect("busca");
            assert!(main_thread_called.get(), "mainThreadCalled");
        });
    }

    #[test]
    fn relative_pattern_include() {
        run_with_faked_timers(async move {
            let main_thread_called = Rc::new(Cell::new(false));
            let called = main_thread_called.clone();
            let ws = workspace_with(MockMainThreadWorkspace {
                start_text_search: Some(Box::new(move |query, folder, options, _request_id, _token| -> TextSearchResultOf {
                    called.set(true);
                    assert_eq!(query.pattern, "foo");
                    assert_eq!(folder.unwrap().to_string(false), uri_file("/other/folder").to_string(false));
                    assert_eq!(options.common.include_pattern, single("glob/**"));
                    assert_eq!(options.common.exclude_pattern, None);
                    Promise::resolved(None)
                })),
                ..MockMainThreadWorkspace::default()
            });
            ws.find_text_in_files(
                PatternInfo::new("foo"),
                FindTextInFilesOptions { include: Some(GlobPattern::from(RelativePattern::new("/other/folder", "glob/**"))), ..FindTextInFilesOptions::default() },
                no_callback(),
                &test_extension(),
                CancellationToken::None,
            )
            .await
            .expect("busca");
            assert!(main_thread_called.get(), "mainThreadCalled");
        });
    }

    #[test]
    fn with_cancelled_token() {
        run_with_faked_timers(async move {
            let main_thread_called = Rc::new(Cell::new(false));
            let called = main_thread_called.clone();
            let ws = workspace_with(MockMainThreadWorkspace {
                start_text_search: Some(Box::new(move |_query, _folder, _options, _request_id, _token| -> TextSearchResultOf {
                    called.set(true);
                    Promise::resolved(None)
                })),
                ..MockMainThreadWorkspace::default()
            });
            let token = CancellationToken::Cancelled;
            ws.find_text_in_files(PatternInfo::new("foo"), FindTextInFilesOptions::default(), no_callback(), &test_extension(), token).await.expect("busca");
            assert!(!main_thread_called.get(), "!mainThreadCalled");
        });
    }

    #[test]
    fn relative_pattern_exclude() {
        run_with_faked_timers(async move {
            let main_thread_called = Rc::new(Cell::new(false));
            let called = main_thread_called.clone();
            let ws = workspace_with(MockMainThreadWorkspace {
                start_text_search: Some(Box::new(move |query, folder, options, _request_id, _token| -> TextSearchResultOf {
                    called.set(true);
                    assert_eq!(query.pattern, "foo");
                    assert_eq!(folder, None);
                    assert_eq!(options.common.include_pattern, None);
                    let exclude_pattern = options.common.exclude_pattern.expect("excludePattern");
                    assert_eq!(exclude_pattern.len(), 1);
                    assert_eq!(exclude_pattern[0], SearchPatternBuilder { uri: None, pattern: SearchPathPatternBuilder::Single("glob/**".to_owned()) }); // exclude folder is ignored...
                    Promise::resolved(None)
                })),
                ..MockMainThreadWorkspace::default()
            });
            ws.find_text_in_files(
                PatternInfo::new("foo"),
                FindTextInFilesOptions { exclude: Some(GlobPattern::from(RelativePattern::new("/other/folder", "glob/**"))), ..FindTextInFilesOptions::default() },
                no_callback(),
                &test_extension(),
                CancellationToken::None,
            )
            .await
            .expect("busca");
            assert!(main_thread_called.get(), "mainThreadCalled");
        });
    }
}

// ---- findTextInFiles2 ------------------------------------------------------------------------------------

mod find_text_in_files2 {
    use super::*;

    #[test]
    fn no_include() {
        run_with_faked_timers(async move {
            let main_thread_called = Rc::new(Cell::new(false));
            let called = main_thread_called.clone();
            let ws = workspace_with(MockMainThreadWorkspace {
                start_text_search: Some(Box::new(move |query, folder, options, _request_id, _token| -> TextSearchResultOf {
                    called.set(true);
                    assert_eq!(query.pattern, "foo");
                    assert_eq!(folder, None);
                    assert_eq!(options.common.include_pattern, None);
                    assert_eq!(options.common.exclude_pattern, None);
                    Promise::resolved(None)
                })),
                ..MockMainThreadWorkspace::default()
            });
            ws.find_text_in_files2(TextSearchQuery2::new("foo"), Some(FindTextInFilesOptions2::default()), &test_extension(), CancellationToken::None).complete.await.expect("busca");
            assert!(main_thread_called.get(), "mainThreadCalled");
        });
    }

    #[test]
    fn string_include() {
        run_with_faked_timers(async move {
            let main_thread_called = Rc::new(Cell::new(false));
            let called = main_thread_called.clone();
            let ws = workspace_with(MockMainThreadWorkspace {
                start_text_search: Some(Box::new(move |query, folder, options, _request_id, _token| -> TextSearchResultOf {
                    called.set(true);
                    assert_eq!(query.pattern, "foo");
                    assert_eq!(folder, None);
                    assert_eq!(options.common.include_pattern, single("**/files"));
                    assert_eq!(options.common.exclude_pattern, None);
                    Promise::resolved(None)
                })),
                ..MockMainThreadWorkspace::default()
            });
            ws.find_text_in_files2(
                TextSearchQuery2::new("foo"),
                Some(FindTextInFilesOptions2 { include: Some(vec![GlobPattern::from("**/files")]), ..FindTextInFilesOptions2::default() }),
                &test_extension(),
                CancellationToken::None,
            )
            .complete
            .await
            .expect("busca");
            assert!(main_thread_called.get(), "mainThreadCalled");
        });
    }

    #[test]
    fn relative_pattern_include() {
        run_with_faked_timers(async move {
            let main_thread_called = Rc::new(Cell::new(false));
            let called = main_thread_called.clone();
            let ws = workspace_with(MockMainThreadWorkspace {
                start_text_search: Some(Box::new(move |query, folder, options, _request_id, _token| -> TextSearchResultOf {
                    called.set(true);
                    assert_eq!(query.pattern, "foo");
                    assert_eq!(folder.unwrap().to_string(false), uri_file("/other/folder").to_string(false));
                    assert_eq!(options.common.include_pattern, single("glob/**"));
                    assert_eq!(options.common.exclude_pattern, None);
                    Promise::resolved(None)
                })),
                ..MockMainThreadWorkspace::default()
            });
            ws.find_text_in_files2(
                TextSearchQuery2::new("foo"),
                Some(FindTextInFilesOptions2 { include: Some(vec![GlobPattern::from(RelativePattern::new("/other/folder", "glob/**"))]), ..FindTextInFilesOptions2::default() }),
                &test_extension(),
                CancellationToken::None,
            )
            .complete
            .await
            .expect("busca");
            assert!(main_thread_called.get(), "mainThreadCalled");
        });
    }

    #[test]
    fn with_cancelled_token() {
        run_with_faked_timers(async move {
            let main_thread_called = Rc::new(Cell::new(false));
            let called = main_thread_called.clone();
            let ws = workspace_with(MockMainThreadWorkspace {
                start_text_search: Some(Box::new(move |_query, _folder, _options, _request_id, _token| -> TextSearchResultOf {
                    called.set(true);
                    Promise::resolved(None)
                })),
                ..MockMainThreadWorkspace::default()
            });
            let token = CancellationToken::Cancelled;
            ws.find_text_in_files2(TextSearchQuery2::new("foo"), None, &test_extension(), token).complete.await.expect("busca");
            assert!(!main_thread_called.get(), "!mainThreadCalled");
        });
    }

    #[test]
    fn relative_pattern_exclude() {
        run_with_faked_timers(async move {
            let main_thread_called = Rc::new(Cell::new(false));
            let called = main_thread_called.clone();
            let ws = workspace_with(MockMainThreadWorkspace {
                start_text_search: Some(Box::new(move |query, folder, options, _request_id, _token| -> TextSearchResultOf {
                    called.set(true);
                    assert_eq!(query.pattern, "foo");
                    assert_eq!(folder, None);
                    assert_eq!(options.common.include_pattern, None);
                    let exclude_pattern = options.common.exclude_pattern.expect("excludePattern");
                    assert_eq!(exclude_pattern.len(), 1);
                    assert_eq!(exclude_pattern[0].pattern, SearchPathPatternBuilder::Single("glob/**".to_owned())); // exclude folder is ignored...
                    Promise::resolved(None)
                })),
                ..MockMainThreadWorkspace::default()
            });
            ws.find_text_in_files2(
                TextSearchQuery2::new("foo"),
                Some(FindTextInFilesOptions2 { exclude: Some(vec![GlobPattern::from(RelativePattern::new("/other/folder", "glob/**"))]), ..FindTextInFilesOptions2::default() }),
                &test_extension(),
                CancellationToken::None,
            )
            .complete
            .await
            .expect("busca");
            assert!(main_thread_called.get(), "mainThreadCalled");
        });
    }

    #[test]
    fn case_insensitive() {
        run_with_faked_timers(async move {
            let main_thread_called = Rc::new(Cell::new(false));
            let called = main_thread_called.clone();
            let ws = workspace_with(MockMainThreadWorkspace {
                start_text_search: Some(Box::new(move |_query, _folder, options, _request_id, _token| -> TextSearchResultOf {
                    called.set(true);
                    assert_eq!(options.common.ignore_glob_case, Some(true));
                    Promise::resolved(None)
                })),
                ..MockMainThreadWorkspace::default()
            });
            ws.find_text_in_files2(
                TextSearchQuery2::new("foo"),
                Some(FindTextInFilesOptions2 { case_insensitive: Some(true), ..FindTextInFilesOptions2::default() }),
                &test_extension(),
                CancellationToken::None,
            )
            .complete
            .await
            .expect("busca");
            assert!(main_thread_called.get(), "mainThreadCalled");
        });
    }

    // TODO: test multiple includes/excludess
}

// ---- Casos próprios --------------------------------------------------------------------------------------
//
// O que o original não cobre: o `IExtHostWorkspaceProvider` (que espera a barreira do `$initializeWorkspace`),
// o nome e o arquivo do workspace e a confiança.

#[test]
fn workspace_provider_methods_wait_for_initialization_and_resolve_folders() {
    run_with_faked_timers(async move {
        let context = TestMainContext::new();
        let initial = data(vec![folder_data(uri_file("/Coding/One"), 0, ""), folder_data(uri_file("/Coding/Two"), 1, "")]);
        let ws = new_ext_host_workspace(&context, Some(initial.clone()));

        // antes do `$initializeWorkspace` a barreira está fechada
        let pending = ws.get_workspace_folders2();
        assert!(!pending.is_settled());

        ws.rpc_initialize_workspace(Some(initial), false);
        assert!(!ws.trusted());
        let folders = pending.await.expect("pastas").expect("workspace");
        assert_eq!(uri_texts(&folders), vec!["file:///Coding/One".to_owned(), "file:///Coding/Two".to_owned()]);

        let folder = ws.get_workspace_folder2(&uri_file("/Coding/Two/a/b"), None).await.expect("pasta").expect("contém");
        assert_eq!(folder.name(), "Two");
        assert!(ws.resolve_workspace_folder(&uri_file("/Coding/Two/a/b")).await.expect("pasta").is_none());
        assert!(ws.resolve_workspace_folder(&uri_file("/Coding/Two")).await.expect("pasta").is_some());

        assert_eq!(ws.name().as_deref(), Some("Test"));
        assert_eq!(ws.workspace_file(), None);

        ws.rpc_on_did_grant_workspace_trust();
        assert!(ws.trusted());
    });
}
