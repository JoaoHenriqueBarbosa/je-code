//! Porte de `src/vs/workbench/contrib/output/test/browser/outputLinkProvider.test.ts` (suíte `OutputLinkProvider`), caso a caso e na mesma ordem.
//!
//! Divergências:
//! - **`ensureNoDisposablesAreLeakedInTestSuite`.** É o [`LeakGuard`] criado no começo de cada caso.
//! - **`TestContextService` como `IResourceCreator`.** O original passa o serviço (que tem `toResource`) ao `detectLinks`; no
//!   porte o contrato é o trait [`IResourceCreator`], e o serviço é embrulhado em [`ContextResourceCreator`] (um tipo local, porque
//!   nem o trait nem o serviço são do teste).
//! - **Linhas.** As linhas do original são montadas com a raiz do workspace em barras invertidas (`WORKSPACE_BACKSLASH`) ou
//!   em barras (`WORKSPACE_SLASH`) e o resto escrito por extenso, depois de passar por [`to_os_path`], como o `toOSPath`.
//! - **`url`.** O `url` do link é o texto (`LinkUrl::Text`) que o `detectLinks` monta.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::js_string::JsString;
use je_base::platform::Platform;
use je_base::uri::URI;
use je_editor::languages::{ILink, LinkUrl};
use je_platform::resources::uri_file;
use je_workbench::contrib::output::common::output_link_computer::{IResourceCreator, OutputLinkComputer};
use je_workbench::test::common::workbench_test_services::TestContextService;

const WORKSPACE_BACKSLASH: &str = "C:\\Users\\someone\\AppData\\Local\\Temp\\_monacodata_9888\\workspaces\\mankala";
const WORKSPACE_SLASH: &str = "C:/Users/someone/AppData/Local/Temp/_monacodata_9888/workspaces/mankala";

fn is_windows() -> bool {
    Platform::detect().is_windows
}

fn to_os_path(p: &str) -> String {
    if !is_windows() {
        return p.replace('\\', "/");
    }

    p.to_string()
}

/// O `TestContextService` usado como `IResourceCreator`.
struct ContextResourceCreator(Rc<TestContextService>);

impl IResourceCreator for ContextResourceCreator {
    fn to_resource(&self, folder_relative_path: &str) -> Option<URI> {
        Some(self.0.to_resource(folder_relative_path))
    }
}

fn detect(line: &str, patterns: &[je_base::js_regexp::JsRegExp], context_service: &ContextResourceCreator) -> Vec<ILink> {
    let units: Vec<u16> = line.encode_utf16().collect();
    OutputLinkComputer::detect_links(&units, 1, patterns, context_service)
}

fn url_of(link: &ILink) -> JsString {
    match link.url.as_ref().expect("o link tem destino") {
        LinkUrl::Text(text) => text.clone(),
        LinkUrl::Uri(uri) => uri.to_string(false),
    }
}

/// `contextService.toResource(path).toString() + suffix`.
fn expected_url(context_service: &ContextResourceCreator, path: &str, suffix: &str) -> JsString {
    let mut url = context_service.0.to_resource(path).to_string(false);
    url.push_units(&suffix.encode_utf16().collect::<Vec<u16>>());
    url
}

#[test]
fn output_link_provider_link_detection() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let root_folder = if is_windows() {
        uri_file("C:\\Users\\someone\\AppData\\Local\\Temp\\_monacodata_9888\\workspaces\\mankala")
    } else {
        uri_file("C:/Users/someone/AppData/Local/Temp/_monacodata_9888/workspaces/mankala")
    };

    let patterns = OutputLinkComputer::create_patterns(&root_folder);

    let context_service = ContextResourceCreator(TestContextService::new(None, None));
    let w = WORKSPACE_BACKSLASH;
    let s = WORKSPACE_SLASH;

    let mut line = to_os_path("Foo bar");
    let mut result = detect(&line, &patterns, &context_service);
    assert_eq!(result.len(), 0);

    // Example: at C:\\Users\\someone\\AppData\\Local\\Temp\\_monacodata_9888\\workspaces\\mankala\\Game.ts
    line = to_os_path(&format!(" at {w}\\Game.ts in"));
    result = detect(&line, &patterns, &context_service);
    assert_eq!(result.len(), 1);
    assert_eq!(url_of(&result[0]), expected_url(&context_service, "/Game.ts", ""));
    assert_eq!(result[0].range.start_column, 5);
    assert_eq!(result[0].range.end_column, 84);

    // Example: at C:\\Users\\someone\\AppData\\Local\\Temp\\_monacodata_9888\\workspaces\\mankala\\Game.ts:336
    line = to_os_path(&format!(" at {w}\\Game.ts:336 in"));
    result = detect(&line, &patterns, &context_service);
    assert_eq!(result.len(), 1);
    assert_eq!(url_of(&result[0]), expected_url(&context_service, "/Game.ts", "#336"));
    assert_eq!(result[0].range.start_column, 5);
    assert_eq!(result[0].range.end_column, 88);

    // Example: at C:\\Users\\someone\\AppData\\Local\\Temp\\_monacodata_9888\\workspaces\\mankala\\Game.ts:336:9
    line = to_os_path(&format!(" at {w}\\Game.ts:336:9 in"));
    result = detect(&line, &patterns, &context_service);
    assert_eq!(result.len(), 1);
    assert_eq!(url_of(&result[0]), expected_url(&context_service, "/Game.ts", "#336,9"));
    assert_eq!(result[0].range.start_column, 5);
    assert_eq!(result[0].range.end_column, 90);

    line = to_os_path(&format!(" at {w}\\Game.ts:336:9 in"));
    result = detect(&line, &patterns, &context_service);
    assert_eq!(result.len(), 1);
    assert_eq!(url_of(&result[0]), expected_url(&context_service, "/Game.ts", "#336,9"));
    assert_eq!(result[0].range.start_column, 5);
    assert_eq!(result[0].range.end_column, 90);

    // Example: at C:\\Users\\someone\\AppData\\Local\\Temp\\_monacodata_9888\\workspaces\\mankala\\Game.ts>dir
    line = to_os_path(&format!(" at {w}\\Game.ts>dir in"));
    result = detect(&line, &patterns, &context_service);
    assert_eq!(result.len(), 1);
    assert_eq!(url_of(&result[0]), expected_url(&context_service, "/Game.ts", ""));
    assert_eq!(result[0].range.start_column, 5);
    assert_eq!(result[0].range.end_column, 84);

    // Example: at [C:\\Users\\someone\\AppData\\Local\\Temp\\_monacodata_9888\\workspaces\\mankala\\Game.ts:336:9]
    line = to_os_path(&format!(" at {w}\\Game.ts:336:9] in"));
    result = detect(&line, &patterns, &context_service);
    assert_eq!(result.len(), 1);
    assert_eq!(url_of(&result[0]), expected_url(&context_service, "/Game.ts", "#336,9"));
    assert_eq!(result[0].range.start_column, 5);
    assert_eq!(result[0].range.end_column, 90);

    // Example: at [C:\\Users\\someone\\AppData\\Local\\Temp\\_monacodata_9888\\workspaces\\mankala\\Game.ts]
    line = to_os_path(&format!(" at {w}\\Game.ts] in"));
    result = detect(&line, &patterns, &context_service);
    assert_eq!(result.len(), 1);
    assert_eq!(url_of(&result[0]), expected_url(&context_service, "/Game.ts]", ""));

    // Example: C:\Users\someone\AppData\Local\Temp\_monacodata_9888\workspaces\express\server.js on line 8
    line = to_os_path(&format!("{w}\\Game.ts on line 8"));
    result = detect(&line, &patterns, &context_service);
    assert_eq!(result.len(), 1);
    assert_eq!(url_of(&result[0]), expected_url(&context_service, "/Game.ts", "#8"));
    assert_eq!(result[0].range.start_column, 1);
    assert_eq!(result[0].range.end_column, 90);

    // Example: C:\Users\someone\AppData\Local\Temp\_monacodata_9888\workspaces\express\server.js on line 8, column 13
    line = to_os_path(&format!("{w}\\Game.ts on line 8, column 13"));
    result = detect(&line, &patterns, &context_service);
    assert_eq!(result.len(), 1);
    assert_eq!(url_of(&result[0]), expected_url(&context_service, "/Game.ts", "#8,13"));
    assert_eq!(result[0].range.start_column, 1);
    assert_eq!(result[0].range.end_column, 101);

    line = to_os_path(&format!("{w}\\Game.ts on LINE 8, COLUMN 13"));
    result = detect(&line, &patterns, &context_service);
    assert_eq!(result.len(), 1);
    assert_eq!(url_of(&result[0]), expected_url(&context_service, "/Game.ts", "#8,13"));
    assert_eq!(result[0].range.start_column, 1);
    assert_eq!(result[0].range.end_column, 101);

    // Example: C:\Users\someone\AppData\Local\Temp\_monacodata_9888\workspaces\express\server.js:line 8
    line = to_os_path(&format!("{w}\\Game.ts:line 8"));
    result = detect(&line, &patterns, &context_service);
    assert_eq!(result.len(), 1);
    assert_eq!(url_of(&result[0]), expected_url(&context_service, "/Game.ts", "#8"));
    assert_eq!(result[0].range.start_column, 1);
    assert_eq!(result[0].range.end_column, 87);

    // Example: at File.put (C:/Users/someone/AppData/Local/Temp/_monacodata_9888/workspaces/mankala/Game.ts)
    line = to_os_path(&format!(" at File.put ({s}/Game.ts)"));
    result = detect(&line, &patterns, &context_service);
    assert_eq!(result.len(), 1);
    assert_eq!(url_of(&result[0]), expected_url(&context_service, "/Game.ts", ""));
    assert_eq!(result[0].range.start_column, 15);
    assert_eq!(result[0].range.end_column, 94);

    // Example: at File.put (C:/Users/someone/AppData/Local/Temp/_monacodata_9888/workspaces/mankala/Game.ts:278)
    line = to_os_path(&format!(" at File.put ({s}/Game.ts:278)"));
    result = detect(&line, &patterns, &context_service);
    assert_eq!(result.len(), 1);
    assert_eq!(url_of(&result[0]), expected_url(&context_service, "/Game.ts", "#278"));
    assert_eq!(result[0].range.start_column, 15);
    assert_eq!(result[0].range.end_column, 98);

    // Example: at File.put (C:/Users/someone/AppData/Local/Temp/_monacodata_9888/workspaces/mankala/Game.ts:278:34)
    line = to_os_path(&format!(" at File.put ({s}/Game.ts:278:34)"));
    result = detect(&line, &patterns, &context_service);
    assert_eq!(result.len(), 1);
    assert_eq!(url_of(&result[0]), expected_url(&context_service, "/Game.ts", "#278,34"));
    assert_eq!(result[0].range.start_column, 15);
    assert_eq!(result[0].range.end_column, 101);

    line = to_os_path(&format!(" at File.put ({s}/Game.ts:278:34)"));
    result = detect(&line, &patterns, &context_service);
    assert_eq!(result.len(), 1);
    assert_eq!(url_of(&result[0]), expected_url(&context_service, "/Game.ts", "#278,34"));
    assert_eq!(result[0].range.start_column, 15);
    assert_eq!(result[0].range.end_column, 101);

    let features = "/lib/something/Features.ts";

    // Example: C:/Users/someone/AppData/Local/Temp/_monacodata_9888/workspaces/mankala/Features.ts(45): error
    line = to_os_path(&format!("{s}/lib/something/Features.ts(45): error"));
    result = detect(&line, &patterns, &context_service);
    assert_eq!(result.len(), 1);
    assert_eq!(url_of(&result[0]), expected_url(&context_service, features, "#45"));
    assert_eq!(result[0].range.start_column, 1);
    assert_eq!(result[0].range.end_column, 102);

    // Example: C:/Users/someone/AppData/Local/Temp/_monacodata_9888/workspaces/mankala/Features.ts (45,18): error
    line = to_os_path(&format!("{s}/lib/something/Features.ts (45): error"));
    result = detect(&line, &patterns, &context_service);
    assert_eq!(result.len(), 1);
    assert_eq!(url_of(&result[0]), expected_url(&context_service, features, "#45"));
    assert_eq!(result[0].range.start_column, 1);
    assert_eq!(result[0].range.end_column, 103);

    // Example: C:/Users/someone/AppData/Local/Temp/_monacodata_9888/workspaces/mankala/Features.ts(45,18): error
    line = to_os_path(&format!("{s}/lib/something/Features.ts(45,18): error"));
    result = detect(&line, &patterns, &context_service);
    assert_eq!(result.len(), 1);
    assert_eq!(url_of(&result[0]), expected_url(&context_service, features, "#45,18"));
    assert_eq!(result[0].range.start_column, 1);
    assert_eq!(result[0].range.end_column, 105);

    line = to_os_path(&format!("{s}/lib/something/Features.ts(45,18): error"));
    result = detect(&line, &patterns, &context_service);
    assert_eq!(result.len(), 1);
    assert_eq!(url_of(&result[0]), expected_url(&context_service, features, "#45,18"));
    assert_eq!(result[0].range.start_column, 1);
    assert_eq!(result[0].range.end_column, 105);

    // Example: C:/Users/someone/AppData/Local/Temp/_monacodata_9888/workspaces/mankala/Features.ts (45,18): error
    line = to_os_path(&format!("{s}/lib/something/Features.ts (45,18): error"));
    result = detect(&line, &patterns, &context_service);
    assert_eq!(result.len(), 1);
    assert_eq!(url_of(&result[0]), expected_url(&context_service, features, "#45,18"));
    assert_eq!(result[0].range.start_column, 1);
    assert_eq!(result[0].range.end_column, 106);

    line = to_os_path(&format!("{s}/lib/something/Features.ts (45,18): error"));
    result = detect(&line, &patterns, &context_service);
    assert_eq!(result.len(), 1);
    assert_eq!(url_of(&result[0]), expected_url(&context_service, features, "#45,18"));
    assert_eq!(result[0].range.start_column, 1);
    assert_eq!(result[0].range.end_column, 106);

    // Example: C:/Users/someone/AppData/Local/Temp/_monacodata_9888/workspaces/mankala/Features.ts(45): error
    line = to_os_path(&format!("{w}\\lib\\something\\Features.ts(45): error"));
    result = detect(&line, &patterns, &context_service);
    assert_eq!(result.len(), 1);
    assert_eq!(url_of(&result[0]), expected_url(&context_service, features, "#45"));
    assert_eq!(result[0].range.start_column, 1);
    assert_eq!(result[0].range.end_column, 102);

    // Example: C:/Users/someone/AppData/Local/Temp/_monacodata_9888/workspaces/mankala/Features.ts (45,18): error
    line = to_os_path(&format!("{w}\\lib\\something\\Features.ts (45): error"));
    result = detect(&line, &patterns, &context_service);
    assert_eq!(result.len(), 1);
    assert_eq!(url_of(&result[0]), expected_url(&context_service, features, "#45"));
    assert_eq!(result[0].range.start_column, 1);
    assert_eq!(result[0].range.end_column, 103);

    // Example: C:/Users/someone/AppData/Local/Temp/_monacodata_9888/workspaces/mankala/Features.ts(45,18): error
    line = to_os_path(&format!("{w}\\lib\\something\\Features.ts(45,18): error"));
    result = detect(&line, &patterns, &context_service);
    assert_eq!(result.len(), 1);
    assert_eq!(url_of(&result[0]), expected_url(&context_service, features, "#45,18"));
    assert_eq!(result[0].range.start_column, 1);
    assert_eq!(result[0].range.end_column, 105);

    line = to_os_path(&format!("{w}\\lib\\something\\Features.ts(45,18): error"));
    result = detect(&line, &patterns, &context_service);
    assert_eq!(result.len(), 1);
    assert_eq!(url_of(&result[0]), expected_url(&context_service, features, "#45,18"));
    assert_eq!(result[0].range.start_column, 1);
    assert_eq!(result[0].range.end_column, 105);

    // Example: C:/Users/someone/AppData/Local/Temp/_monacodata_9888/workspaces/mankala/Features.ts (45,18): error
    line = to_os_path(&format!("{w}\\lib\\something\\Features.ts (45,18): error"));
    result = detect(&line, &patterns, &context_service);
    assert_eq!(result.len(), 1);
    assert_eq!(url_of(&result[0]), expected_url(&context_service, features, "#45,18"));
    assert_eq!(result[0].range.start_column, 1);
    assert_eq!(result[0].range.end_column, 106);

    line = to_os_path(&format!("{w}\\lib\\something\\Features.ts (45,18): error"));
    result = detect(&line, &patterns, &context_service);
    assert_eq!(result.len(), 1);
    assert_eq!(url_of(&result[0]), expected_url(&context_service, features, "#45,18"));
    assert_eq!(result[0].range.start_column, 1);
    assert_eq!(result[0].range.end_column, 106);

    // Example: C:\\Users\\someone\\AppData\\Local\\Temp\\_monacodata_9888\\workspaces\\mankala\\lib\\something\\Features Special.ts (45,18): error.
    line = to_os_path(&format!("{w}\\lib\\something\\Features Special.ts (45,18): error"));
    result = detect(&line, &patterns, &context_service);
    assert_eq!(result.len(), 1);
    assert_eq!(url_of(&result[0]), expected_url(&context_service, "/lib/something/Features Special.ts", "#45,18"));
    assert_eq!(result[0].range.start_column, 1);
    assert_eq!(result[0].range.end_column, 114);

    // Example: at C:\\Users\\someone\\AppData\\Local\\Temp\\_monacodata_9888\\workspaces\\mankala\\Game.ts.
    line = to_os_path(&format!(" at {w}\\Game.ts. in"));
    result = detect(&line, &patterns, &context_service);
    assert_eq!(result.len(), 1);
    assert_eq!(url_of(&result[0]), expected_url(&context_service, "/Game.ts", ""));
    assert_eq!(result[0].range.start_column, 5);
    assert_eq!(result[0].range.end_column, 84);

    // Example: at C:\\Users\\someone\\AppData\\Local\\Temp\\_monacodata_9888\\workspaces\\mankala\\Game
    line = to_os_path(&format!(" at {w}\\Game in"));
    result = detect(&line, &patterns, &context_service);
    assert_eq!(result.len(), 1);

    // Example: at C:\\Users\\someone\\AppData\\Local\\Temp\\_monacodata_9888\\workspaces\\mankala\\Game\\
    line = to_os_path(&format!(" at {w}\\Game\\ in"));
    result = detect(&line, &patterns, &context_service);
    assert_eq!(result.len(), 1);

    // Example: at "C:\\Users\\someone\\AppData\\Local\\Temp\\_monacodata_9888\\workspaces\\mankala\\Game.ts"
    line = to_os_path(&format!(" at \"{w}\\Game.ts\" in"));
    result = detect(&line, &patterns, &context_service);
    assert_eq!(result.len(), 1);
    assert_eq!(url_of(&result[0]), expected_url(&context_service, "/Game.ts", ""));
    assert_eq!(result[0].range.start_column, 6);
    assert_eq!(result[0].range.end_column, 85);

    // Example: at 'C:\\Users\\someone\\AppData\\Local\\Temp\\_monacodata_9888\\workspaces\\mankala\\Game.ts'
    line = to_os_path(&format!(" at '{w}\\Game.ts' in"));
    result = detect(&line, &patterns, &context_service);
    assert_eq!(result.len(), 1);
    assert_eq!(url_of(&result[0]), expected_url(&context_service, "/Game.ts", ""));
    assert_eq!(result[0].range.start_column, 6);
    assert_eq!(result[0].range.end_column, 85);
}

#[test]
fn output_link_provider_106847() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let root_folder = if is_windows() { uri_file("C:\\Users\\username\\Desktop\\test-ts") } else { uri_file("C:/Users/username/Desktop") };

    let patterns = OutputLinkComputer::create_patterns(&root_folder);

    let context_service = ContextResourceCreator(TestContextService::new(None, None));

    let line = to_os_path(
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa C:\\Users\\username\\Desktop\\test-ts\\prj.conf C:\\Users\\username\\Desktop\\test-ts\\prj.conf C:\\Users\\username\\Desktop\\test-ts\\prj.conf",
    );
    let result = detect(&line, &patterns, &context_service);
    assert_eq!(result.len(), 3);

    for res in &result {
        assert!(res.range.start_column > 0 && res.range.end_column > 0);
    }
}
