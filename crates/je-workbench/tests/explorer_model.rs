//! Porte de `src/vs/workbench/contrib/files/test/browser/explorerModel.test.ts` (suíte `Files - View Model`), caso a caso e na
//! mesma ordem.
//!
//! Notas de tradução:
//! - **`ensureNoDisposablesAreLeakedInTestSuite()`.** Declarado no fim da suíte e válido para cada caso: o `LeakGuard` no
//!   começo de cada um. O `TestFileService` e o `TestConfigurationService` são `const` do corpo da suíte (criados antes do
//!   `setup` do guarda, então não contam como vazamento); aqui são criados antes do guarda, e o guarda é solto primeiro.
//! - **`toResource.call(this, path)`.** `URI.file(join('/', btoa(this.test.fullTitle()), path))`; o título completo do caso
//!   do mocha é o primeiro argumento de [`to_resource`] (cada caso passa o seu).
//! - **`s.move(s1)`.** `move` é palavra reservada: `move_to`. **`s.rename(renamed)`.** Recebe o nome e o `mtime` do item.
//! - **`assert.strictEqual(a, b)` sobre objetos.** É a identidade: `Rc::ptr_eq`. **`isLinux` e `isWindows`.** São os campos
//!   do `Platform::detect()`.
//! - **`validateFileName(pathService, s, null!, OS)`.** O nome `null` é `None`.

use std::rc::Rc;

use je_base::path::{PathModule, ProcessEnvironment};
use je_base::platform::Platform;
use je_base::runtime::date_now;
use je_base::uri::URI;
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::files::i_file_service::IFileService;
use je_platform::resources::{fs_path, uri_file};
use je_workbench::contrib::files::explorer_model::ExplorerItem;
use je_workbench::contrib::files::file_actions::validate_file_name;
use je_workbench::services::files_configuration::files_configuration_service::IFilesConfigurationService;
use je_workbench::test::browser::workbench_test_services::TestPathService;
use je_workbench::test::common::workbench_test_services::{NullFilesConfigurationService, TestFileService};

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;

/// `join` do `path` nativo.
fn join(paths: &[&str]) -> String {
    PathModule::native(&ProcessEnvironment).join(paths)
}

/// `btoa(text)` para o texto ASCII do título de um caso.
fn btoa(text: &str) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in text.as_bytes().chunks(3) {
        let n = chunk.iter().enumerate().fold(0u32, |acc, (i, byte)| acc | (u32::from(*byte) << (16 - 8 * i)));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ALPHABET[((n >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// `toResource.call(this, path)`.
fn to_resource(full_title: &str, path: &str) -> URI {
    uri_file(&join(&["/", &btoa(full_title), path]))
}

/// Os serviços do corpo da suíte.
struct Services {
    file_service: Rc<dyn IFileService>,
    config_service: Rc<dyn IConfigurationService>,
    files_config_service: Rc<dyn IFilesConfigurationService>,
}

impl Services {
    fn new() -> Self {
        Self { file_service: TestFileService::new(), config_service: TestConfigurationService::new(None), files_config_service: Rc::new(NullFilesConfigurationService) }
    }

    /// `createStat.call(this, path, name, isFolder, hasChildren, size, mtime)`.
    fn create_stat(&self, title: &str, path: &str, name: &str, is_folder: bool, mtime: f64) -> Rc<ExplorerItem> {
        ExplorerItem::new(
            to_resource(title, path),
            self.file_service.clone(),
            self.config_service.clone(),
            self.files_config_service.clone(),
            None,
            Some(is_folder),
            Some(false),
            Some(false),
            Some(false),
            Some(name.to_string()),
            Some(mtime),
            false,
        )
    }

    /// `new ExplorerItem(URI.file(join('C:\\', path)), ..., undefined, true, false, false, false, name, Date.now())`.
    fn create_disk_item(&self, path: &str, name: &str) -> Rc<ExplorerItem> {
        ExplorerItem::new(
            uri_file(&join(&["C:\\", path])),
            self.file_service.clone(),
            self.config_service.clone(),
            self.files_config_service.clone(),
            None,
            Some(true),
            Some(false),
            Some(false),
            Some(false),
            Some(name.to_string()),
            Some(date_now() as f64),
            false,
        )
    }
}

#[test]
fn properties() {
    const TITLE: &str = "Files - View Model Properties";
    let services = Services::new();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let d = date_now() as f64;
    let s = services.create_stat(TITLE, "/path/to/stat", "sName", true, d);

    assert!(!s.is_directory_resolved());
    assert_eq!(fs_path(&s.resource()), fs_path(&to_resource(TITLE, "/path/to/stat")));
    assert_eq!(s.name(), "sName");
    assert!(s.is_directory());
    assert_eq!(s.mtime(), Some(d));

    let _s = services.create_stat(TITLE, "/path/to/stat", "sName", false, d);
}

#[test]
fn add_and_remove_child_check_for_has_child() {
    const TITLE: &str = "Files - View Model Add and Remove Child, check for hasChild";
    let services = Services::new();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let d = date_now() as f64;
    let s = services.create_stat(TITLE, "/path/to/stat", "sName", true, d);

    let child1 = services.create_stat(TITLE, "/path/to/stat/foo", "foo", true, d);
    let child4 = services.create_stat(TITLE, "/otherpath/to/other/otherbar.html", "otherbar.html", false, d);

    s.add_child(&child1);

    assert!(s.get_child(&child1.name()).is_some());

    s.remove_child(&child1);
    s.add_child(&child1);
    assert!(s.get_child(&child1.name()).is_some());

    s.remove_child(&child1);
    assert!(s.get_child(&child1.name()).is_none());

    // Assert that adding a child updates its path properly
    s.add_child(&child4);
    assert_eq!(fs_path(&child4.resource()), fs_path(&to_resource(TITLE, &format!("/path/to/stat/{}", child4.name()))));
}

#[test]
fn move_() {
    const TITLE: &str = "Files - View Model Move";
    let services = Services::new();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let d = date_now() as f64;

    let s1 = services.create_stat(TITLE, "/", "/", true, d);
    let s2 = services.create_stat(TITLE, "/path", "path", true, d);
    let s3 = services.create_stat(TITLE, "/path/to", "to", true, d);
    let s4 = services.create_stat(TITLE, "/path/to/stat", "stat", false, d);

    s1.add_child(&s2);
    s2.add_child(&s3);
    s3.add_child(&s4);

    s4.move_to(&s1);

    // Assert the new path of the moved element
    assert_eq!(fs_path(&s4.resource()), fs_path(&to_resource(TITLE, &format!("/{}", s4.name()))));

    // Move a subtree with children
    let leaf = services.create_stat(TITLE, "/leaf", "leaf", true, d);
    let leaf_c1 = services.create_stat(TITLE, "/leaf/folder", "folder", true, d);
    let leaf_cc2 = services.create_stat(TITLE, "/leaf/folder/index.html", "index.html", true, d);

    leaf.add_child(&leaf_c1);
    leaf_c1.add_child(&leaf_cc2);
    s1.add_child(&leaf);

    leaf_c1.move_to(&s3);
    assert_eq!(fs_path(&leaf_c1.resource()), fs_path(&uri_file(&format!("{}/{}", fs_path(&s3.resource()), leaf_c1.name()))));
    assert_eq!(fs_path(&leaf_cc2.resource()), fs_path(&uri_file(&format!("{}/{}", fs_path(&leaf_c1.resource()), leaf_cc2.name()))));
}

#[test]
fn rename() {
    const TITLE: &str = "Files - View Model Rename";
    let services = Services::new();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let d = date_now() as f64;

    let s1 = services.create_stat(TITLE, "/", "/", true, d);
    let s2 = services.create_stat(TITLE, "/path", "path", true, d);
    let s3 = services.create_stat(TITLE, "/path/to", "to", true, d);
    let s4 = services.create_stat(TITLE, "/path/to/stat", "stat", true, d);

    s1.add_child(&s2);
    s2.add_child(&s3);
    s3.add_child(&s4);

    assert!(Rc::ptr_eq(&s1.get_child(&s2.name()).unwrap(), &s2));
    let s2renamed = services.create_stat(TITLE, "/otherpath", "otherpath", true, d);
    s2.rename(&s2renamed.name(), s2renamed.mtime());
    assert!(Rc::ptr_eq(&s1.get_child(&s2.name()).unwrap(), &s2));

    // Verify the paths have changed including children
    assert_eq!(s2.name(), s2renamed.name());
    assert_eq!(fs_path(&s2.resource()), fs_path(&s2renamed.resource()));
    assert_eq!(fs_path(&s3.resource()), fs_path(&to_resource(TITLE, "/otherpath/to")));
    assert_eq!(fs_path(&s4.resource()), fs_path(&to_resource(TITLE, "/otherpath/to/stat")));

    let s4renamed = services.create_stat(TITLE, "/otherpath/to/statother.js", "statother.js", true, d);
    s4.rename(&s4renamed.name(), s4renamed.mtime());
    assert!(Rc::ptr_eq(&s3.get_child(&s4.name()).unwrap(), &s4));
    assert_eq!(s4.name(), s4renamed.name());
    assert_eq!(fs_path(&s4.resource()), fs_path(&s4renamed.resource()));
}

#[test]
fn find() {
    const TITLE: &str = "Files - View Model Find";
    let services = Services::new();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let d = date_now() as f64;

    let s1 = services.create_stat(TITLE, "/", "/", true, d);
    let s2 = services.create_stat(TITLE, "/path", "path", true, d);
    let s3 = services.create_stat(TITLE, "/path/to", "to", true, d);
    let s4 = services.create_stat(TITLE, "/path/to/stat", "stat", true, d);
    let s4_upper = services.create_stat(TITLE, "/path/to/STAT", "stat", true, d);

    let child1 = services.create_stat(TITLE, "/path/to/stat/foo", "foo", true, d);
    let child2 = services.create_stat(TITLE, "/path/to/stat/foo/bar.html", "bar.html", false, d);

    s1.add_child(&s2);
    s2.add_child(&s3);
    s3.add_child(&s4);
    s4.add_child(&child1);
    child1.add_child(&child2);

    assert!(Rc::ptr_eq(&s1.find(&child2.resource()).unwrap(), &child2));
    assert!(Rc::ptr_eq(&s1.find(&child1.resource()).unwrap(), &child1));
    assert!(Rc::ptr_eq(&s1.find(&s4.resource()).unwrap(), &s4));
    assert!(Rc::ptr_eq(&s1.find(&s3.resource()).unwrap(), &s3));
    assert!(Rc::ptr_eq(&s1.find(&s2.resource()).unwrap(), &s2));

    if Platform::detect().is_linux {
        assert!(s1.find(&s4_upper.resource()).is_none());
    } else {
        assert!(Rc::ptr_eq(&s1.find(&s4_upper.resource()).unwrap(), &s4));
    }

    assert!(s1.find(&to_resource(TITLE, "foobar")).is_none());

    assert!(Rc::ptr_eq(&s1.find(&to_resource(TITLE, "/")).unwrap(), &s1));
}

#[test]
fn find_with_mixed_case() {
    const TITLE: &str = "Files - View Model Find with mixed case";
    let services = Services::new();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let d = date_now() as f64;

    let s1 = services.create_stat(TITLE, "/", "/", true, d);
    let s2 = services.create_stat(TITLE, "/path", "path", true, d);
    let s3 = services.create_stat(TITLE, "/path/to", "to", true, d);
    let s4 = services.create_stat(TITLE, "/path/to/stat", "stat", true, d);

    let child1 = services.create_stat(TITLE, "/path/to/stat/foo", "foo", true, d);
    let child2 = services.create_stat(TITLE, "/path/to/stat/foo/bar.html", "bar.html", false, d);

    s1.add_child(&s2);
    s2.add_child(&s3);
    s3.add_child(&s4);
    s4.add_child(&child1);
    child1.add_child(&child2);

    if Platform::detect().is_linux {
        // linux is case sensitive
        assert!(s1.find(&to_resource(TITLE, "/path/to/stat/Foo")).is_none());
        assert!(s1.find(&to_resource(TITLE, "/Path/to/stat/foo/bar.html")).is_none());
    } else {
        assert!(s1.find(&to_resource(TITLE, "/path/to/stat/Foo")).is_some());
        assert!(s1.find(&to_resource(TITLE, "/Path/to/stat/foo/bar.html")).is_some());
    }
}

#[test]
fn validate_file_name_for_create() {
    const TITLE: &str = "Files - View Model Validate File Name (For Create)";
    let services = Services::new();
    let path_service = TestPathService::new(None, None);
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let os = Platform::detect().os;

    let d = date_now() as f64;
    let s = services.create_stat(TITLE, "/path/to/stat", "sName", true, d);
    let s_child = services.create_stat(TITLE, "/path/to/stat/alles.klar", "alles.klar", true, d);
    s.add_child(&s_child);

    assert!(validate_file_name(&*path_service, &s, None, os).is_some());
    assert!(validate_file_name(&*path_service, &s, Some(""), os).is_some());
    assert!(validate_file_name(&*path_service, &s, Some("  "), os).is_some());
    assert!(validate_file_name(&*path_service, &s, Some("Read Me"), os).is_none(), "name containing space");

    if Platform::detect().is_windows {
        assert!(validate_file_name(&*path_service, &s, Some("foo:bar"), os).is_some());
        assert!(validate_file_name(&*path_service, &s, Some("foo*bar"), os).is_some());
        assert!(validate_file_name(&*path_service, &s, Some("foo?bar"), os).is_some());
        assert!(validate_file_name(&*path_service, &s, Some("foo<bar"), os).is_some());
        assert!(validate_file_name(&*path_service, &s, Some("foo>bar"), os).is_some());
        assert!(validate_file_name(&*path_service, &s, Some("foo|bar"), os).is_some());
    }
    assert!(validate_file_name(&*path_service, &s, Some("alles.klar"), os).is_none());
    assert!(validate_file_name(&*path_service, &s, Some(".foo"), os).is_none());
    assert!(validate_file_name(&*path_service, &s, Some("foo.bar"), os).is_none());
    assert!(validate_file_name(&*path_service, &s, Some("foo"), os).is_none());
}

#[test]
fn validate_file_name_for_rename() {
    const TITLE: &str = "Files - View Model Validate File Name (For Rename)";
    let services = Services::new();
    let path_service = TestPathService::new(None, None);
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let os = Platform::detect().os;

    let d = date_now() as f64;
    let s = services.create_stat(TITLE, "/path/to/stat", "sName", true, d);
    let s_child = services.create_stat(TITLE, "/path/to/stat/alles.klar", "alles.klar", true, d);
    s.add_child(&s_child);

    assert!(validate_file_name(&*path_service, &s, Some("alles.klar"), os).is_none());

    assert!(validate_file_name(&*path_service, &s, Some("Alles.klar"), os).is_none());
    assert!(validate_file_name(&*path_service, &s, Some("Alles.Klar"), os).is_none());

    assert!(validate_file_name(&*path_service, &s, Some(".foo"), os).is_none());
    assert!(validate_file_name(&*path_service, &s, Some("foo.bar"), os).is_none());
    assert!(validate_file_name(&*path_service, &s, Some("foo"), os).is_none());
}

#[test]
fn validate_multi_path_file_names() {
    const TITLE: &str = "Files - View Model Validate Multi-Path File Names";
    let services = Services::new();
    let path_service = TestPathService::new(None, None);
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let os = Platform::detect().os;

    let d = date_now() as f64;
    let ws_folder = services.create_stat(TITLE, "/", "workspaceFolder", true, d);

    assert!(validate_file_name(&*path_service, &ws_folder, Some("foo/bar"), os).is_none());
    assert!(validate_file_name(&*path_service, &ws_folder, Some("foo\\bar"), os).is_none());
    assert!(validate_file_name(&*path_service, &ws_folder, Some("all/slashes/are/same"), os).is_none());
    assert!(validate_file_name(&*path_service, &ws_folder, Some("theres/one/different\\slash"), os).is_none());
    assert!(validate_file_name(&*path_service, &ws_folder, Some("/slashAtBeginning"), os).is_some());

    // attempting to add a child to a deeply nested file
    let s1 = services.create_stat(TITLE, "/path", "path", true, d);
    let s2 = services.create_stat(TITLE, "/path/to", "to", true, d);
    let s3 = services.create_stat(TITLE, "/path/to/stat", "stat", true, d);
    ws_folder.add_child(&s1);
    s1.add_child(&s2);
    s2.add_child(&s3);
    let file_deeply_nested = services.create_stat(TITLE, "/path/to/stat/fileNested", "fileNested", false, d);
    s3.add_child(&file_deeply_nested);
    assert!(validate_file_name(&*path_service, &ws_folder, Some("/path/to/stat/fileNested/aChild"), os).is_some());

    // detect if path already exists
    assert!(validate_file_name(&*path_service, &ws_folder, Some("/path/to/stat/fileNested"), os).is_some());
    assert!(validate_file_name(&*path_service, &ws_folder, Some("/path/to/stat/"), os).is_some());
}

#[test]
fn merge_local_with_disk() {
    let services = Services::new();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let merge1 = services.create_disk_item("/path/to", "to");
    let merge2 = services.create_disk_item("/path/to", "to");

    // Merge Properties
    ExplorerItem::merge_local_with_disk(&merge2, &merge1);
    assert_eq!(merge1.mtime(), merge2.mtime());

    // Merge Child when isDirectoryResolved=false is a no-op
    merge2.add_child(&services.create_disk_item("/path/to/foo.html", "foo.html"));
    ExplorerItem::merge_local_with_disk(&merge2, &merge1);

    // Merge Child with isDirectoryResolved=true
    let child = services.create_disk_item("/path/to/foo.html", "foo.html");
    merge2.remove_child(&child);
    merge2.add_child(&child);
    merge2.set_directory_resolved(true);
    ExplorerItem::merge_local_with_disk(&merge2, &merge1);
    assert_eq!(merge1.get_child("foo.html").unwrap().name(), "foo.html");
    assert!(Rc::ptr_eq(&merge1.get_child("foo.html").unwrap().parent().unwrap(), &merge1), "Check parent");

    // Verify that merge does not replace existing children, but updates properties in that case
    let existing_child = merge1.get_child("foo.html").unwrap();
    ExplorerItem::merge_local_with_disk(&merge2, &merge1);
    assert!(Rc::ptr_eq(&existing_child, &merge1.get_child(&existing_child.name()).unwrap()));
}
