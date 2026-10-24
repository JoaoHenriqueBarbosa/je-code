//! Teste de ponta a ponta de `vscode.languages` (diagnósticos, lista de linguagens, troca de linguagem e `match`; não é porte do oráculo):
//! uma extensão de exemplo em JavaScript, ativada pelo host local (`AbstractExtensionService` + `LocalExtensionHost` + `RPCProtocol`), cria
//! coleções de diagnósticos, publica, lê, apaga e limpa, assina `onDidChangeDiagnostics`, lê as linguagens, troca a linguagem de um
//! documento e pontua seletores.
//!
//! O lado do workbench é o `MainThreadDiagnostics` REAL sobre o `MarkerService` e o `UriIdentityService` reais (os marcadores que a
//! extensão publica aparecem no `MarkerService`, e os que o workbench publica voltam por `getDiagnostics`), mais os documentos de texto reais
//! (`MainThreadDocumentsAndEditors` sobre o `ModelService`). O `MainThreadLanguages` é um dublê que registra as chamadas (o real pede o
//! `ILanguageService` e o `ITextModelService` completos), ver `support/extension_host_api_languages_support.rs`.
//!
//! - **Coleção**: `createDiagnosticCollection` (com nome e sem nome), `set` (uma Uri, lista de entradas e `undefined`), `get`, `has`, `delete`,
//!   `clear`, `forEach`, `dispose` e o erro de usar a coleção depois do `dispose`; `Diagnostic` com `source`, `code`, `relatedInformation` e
//!   `tags`, e a conversão para `IMarkerData` no `MarkerService`.
//! - **Leitura**: `getDiagnostics()` e `getDiagnostics(uri)`, inclusive com marcadores de outro dono vindos do workbench.
//! - **Evento**: `onDidChangeDiagnostics` com as Uris alteradas (depois do `DebounceEmitter` de 50 ms).
//! - **Linguagens**: `getLanguages`, `setTextDocumentLanguage` e `match`.

#[allow(dead_code)]
#[path = "../../je-base/tests/common/mod.rs"]
mod common;
#[path = "support/extension_host_api_harness.rs"]
mod harness;
#[allow(dead_code, unused_imports)]
#[path = "support/main_thread_support.rs"]
mod main_thread_support;

/// O suporte de editor do `je-editor/tests/support` que o `TestCodeEditorService` precisa, no mesmo módulo pai (os arquivos se referem uns
/// aos outros por `super::`).
#[allow(dead_code, unused_imports)]
#[path = "../../je-editor/tests/support"]
mod editor_support {
    pub mod editor_test_services;
    pub mod test_clipboard_service;
    pub mod test_code_editor;
    pub mod test_code_editor_service;
    pub mod test_color_theme_service;
    pub mod test_configuration;
    pub mod test_instantiation_service;
    pub mod test_language_configuration_service;
    pub mod test_notification_service;
    pub mod test_text_model;
    pub mod test_theme_service;
}
#[path = "support/extension_host_api_languages_support.rs"]
mod languages_support;

use std::time::Duration;

use harness::{run_extension, Workbench};
use je_base::dynamic::Dynamic;
use languages_support::{create_document, languages_setup, project_file, Observed, LANGUAGE_IDS};
use je_platform::markers::markers::{IMarkerService, MarkerData, MarkerReadOptions, MarkerSeverity};

/// O JavaScript da extensão de exemplo.
const EXTENSION_SOURCE: &str = r#"
const vscode = require('vscode');
exports.activate = (context) => {
  globalThis.__activated = true;
  globalThis.__events = [];
  const log = (text) => globalThis.__events.push(text);
  const subscribe = (disposable) => context.subscriptions.push(disposable);
  const command = (id, handler) => subscribe(vscode.commands.registerCommand(id, handler));
  const pos = (position) => position.line + ',' + position.character;
  const describe = (range) => pos(range.start) + '-' + pos(range.end);
  const fileA = vscode.Uri.file('/proj/a.txt');
  const fileB = vscode.Uri.file('/proj/b.txt');
  let collection;

  subscribe(vscode.languages.onDidChangeDiagnostics((event) => log('diagnostics:' + event.uris.map((uri) => uri.path).sort().join(','))));

  const sample = (message, severity) => new vscode.Diagnostic(new vscode.Range(0, 0, 0, 5), message, severity);

  command('t.create', () => {
    collection = vscode.languages.createDiagnosticCollection('exemplo');
    return collection.name;
  });
  command('t.createAnonymous', () => {
    const anonymous = vscode.languages.createDiagnosticCollection();
    const name = anonymous.name;
    anonymous.dispose();
    return name;
  });
  command('t.set', () => {
    const diagnostic = sample('erro de exemplo', vscode.DiagnosticSeverity.Warning);
    diagnostic.source = 'exemplo';
    diagnostic.code = 'E1';
    diagnostic.relatedInformation = [new vscode.DiagnosticRelatedInformation(new vscode.Location(fileB, new vscode.Range(1, 0, 1, 3)), 'relacionado')];
    diagnostic.tags = [vscode.DiagnosticTag.Unnecessary];
    collection.set(fileA, [diagnostic]);
    return JSON.stringify(collection.get(fileA).map((item) => ({
      message: item.message,
      severity: item.severity,
      source: item.source,
      code: item.code,
      range: describe(item.range),
      related: item.relatedInformation.map((related) => related.message + '@' + related.location.uri.path + ':' + describe(related.location.range)),
      tags: item.tags,
    })));
  });
  command('t.setDefaults', () => {
    collection.set(fileA, [sample('padrão', undefined)]);
    const item = collection.get(fileA)[0];
    return item.severity + '|' + String(item.source) + '|' + String(item.code);
  });
  command('t.setMany', () => {
    collection.set([[fileA, [sample('um', vscode.DiagnosticSeverity.Error)]], [fileB, [sample('dois', vscode.DiagnosticSeverity.Hint), sample('três', vscode.DiagnosticSeverity.Information)]]]);
    return collection.get(fileA).length + ',' + collection.get(fileB).length;
  });
  command('t.has', () => String(collection.has(fileA)) + ',' + String(collection.has(fileB)));
  command('t.forEach', () => {
    const seen = [];
    collection.forEach((uri, diagnostics, owner) => seen.push(uri.path + ':' + diagnostics.length + ':' + (owner.name === collection.name)));
    return seen.join(';');
  });
  command('t.getDiagnostics', () => {
    const all = vscode.languages.getDiagnostics().map((entry) => entry[0].path + '=' + entry[1].map((item) => item.message).join('+'));
    return all.sort().join(';') + '|' + vscode.languages.getDiagnostics(fileA).map((item) => item.message).join('+');
  });
  command('t.delete', () => {
    collection.delete(fileA);
    return collection.get(fileA).length + ',' + String(collection.has(fileA));
  });
  command('t.clear', () => {
    collection.clear();
    return String(vscode.languages.getDiagnostics().length);
  });
  command('t.clearByUndefined', () => {
    collection.set(undefined);
    return String(collection.has(fileA)) + ',' + String(collection.has(fileB));
  });
  command('t.dispose', () => {
    collection.dispose();
    try {
      collection.set(fileA, []);
      return 'sem erro';
    } catch (error) {
      return error.message;
    }
  });
  command('t.invalidDiagnostic', () => {
    try {
      new vscode.Diagnostic(new vscode.Range(0, 0, 0, 1), '');
      return 'sem erro';
    } catch (error) {
      return error.message;
    }
  });
  command('t.languages', async () => JSON.stringify(await vscode.languages.getLanguages()));
  command('t.match', async () => {
    const document = await vscode.workspace.openTextDocument(fileA);
    return JSON.stringify([
      vscode.languages.match('plaintext', document),
      vscode.languages.match('typescript', document),
      vscode.languages.match('*', document),
      vscode.languages.match({ language: 'plaintext', scheme: 'file' }, document),
      vscode.languages.match({ scheme: 'untitled' }, document),
      vscode.languages.match(['typescript', { scheme: 'file' }], document),
      vscode.languages.match({ pattern: '**/*.txt' }, document),
    ]);
  });
  command('t.setLanguage', async () => {
    const document = await vscode.workspace.openTextDocument(fileA);
    const changed = await vscode.languages.setTextDocumentLanguage(document, 'typescript');
    return changed.uri.path;
  });
};
"#;

fn run(workbench: &Workbench, command: &str) -> Dynamic {
    workbench.execute(command, Vec::new())
}

fn text(value: &str) -> Dynamic {
    Dynamic::string(value)
}

fn events(workbench: &Workbench) -> String {
    workbench.probe("JSON.stringify(globalThis.__events)")
}

/// Os marcadores que o `MarkerService` guarda do dono `owner`.
fn markers_of(observed: &Observed, owner: &str) -> Vec<je_platform::markers::markers::Marker> {
    observed.marker_service.read(Some(&MarkerReadOptions { owner: Some(owner.to_owned()), ..Default::default() }))
}

fn setup(_workbench: &Workbench) -> (std::rc::Rc<dyn je_exthost::in_process::IMainThreadBinder>, Observed) {
    languages_setup()
}

#[test]
fn set_publishes_markers_and_get_returns_the_diagnostics_with_every_field() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, observed| {
        assert_eq!(run(workbench, "t.create"), text("exemplo"));
        assert_eq!(
            run(workbench, "t.set"),
            text(concat!(
                r#"[{"message":"erro de exemplo","severity":1,"source":"exemplo","code":"E1","range":"0,0-0,5","#,
                r#""related":["relacionado@/proj/b.txt:1,0-1,3"],"tags":[1]}]"#,
            ))
        );

        let markers = markers_of(&observed, "exemplo");
        assert_eq!(markers.len(), 1, "o MainThreadDiagnostics grava o marcador no MarkerService");
        let marker = &markers[0];
        assert_eq!(marker.resource.path().to_string(), "/proj/a.txt");
        assert_eq!(marker.message, "erro de exemplo");
        assert_eq!(marker.severity, MarkerSeverity::WARNING);
        assert_eq!((marker.start_line_number, marker.start_column, marker.end_line_number, marker.end_column), (1, 1, 1, 6), "o intervalo vira base 1");
        assert_eq!(marker.source.as_deref(), Some("exemplo"));
    });
}

#[test]
fn diagnostic_defaults_are_the_ones_of_the_original() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, _observed| {
        run(workbench, "t.create");
        assert_eq!(run(workbench, "t.setDefaults"), text("0|undefined|undefined"), "a severidade padrão é Error e source e code ficam ausentes");
        assert_eq!(run(workbench, "t.invalidDiagnostic"), text("message must be set"), "o construtor recusa a mensagem vazia");
    });
}

#[test]
fn set_many_has_for_each_and_get_diagnostics_see_every_entry() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, observed| {
        run(workbench, "t.create");
        assert_eq!(run(workbench, "t.setMany"), text("1,2"));
        assert_eq!(run(workbench, "t.has"), text("true,true"));
        assert_eq!(run(workbench, "t.forEach"), text("/proj/a.txt:1:true;/proj/b.txt:2:true"));
        assert_eq!(run(workbench, "t.getDiagnostics"), text("/proj/a.txt=um;/proj/b.txt=dois+três|um"));
        assert_eq!(markers_of(&observed, "exemplo").len(), 3, "as três entradas chegam ao MarkerService");
    });
}

#[test]
fn delete_clear_and_set_undefined_remove_the_markers() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, observed| {
        run(workbench, "t.create");
        run(workbench, "t.setMany");

        assert_eq!(run(workbench, "t.delete"), text("0,false"));
        assert_eq!(markers_of(&observed, "exemplo").len(), 2, "só as duas de b.txt sobram");

        assert_eq!(run(workbench, "t.clear"), text("0"));
        assert!(markers_of(&observed, "exemplo").is_empty(), "clear limpa o dono inteiro");

        run(workbench, "t.setMany");
        assert_eq!(run(workbench, "t.clearByUndefined"), text("false,false"), "set(undefined) é clear()");
        assert!(markers_of(&observed, "exemplo").is_empty());
    });
}

#[test]
fn dispose_clears_the_owner_and_the_collection_refuses_further_use() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, observed| {
        run(workbench, "t.create");
        run(workbench, "t.setMany");
        assert_eq!(markers_of(&observed, "exemplo").len(), 3);

        assert_eq!(run(workbench, "t.dispose"), text("illegal state - object is disposed"));
        assert!(markers_of(&observed, "exemplo").is_empty(), "o dispose limpa os marcadores do dono");
        assert_eq!(run(workbench, "t.getDiagnostics"), text("|"), "a coleção descartada sai do getDiagnostics");
    });
}

#[test]
fn a_collection_without_a_name_gets_a_generated_one() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, _observed| {
        let name = run(workbench, "t.createAnonymous");
        let Dynamic::String(name) = name else { panic!("o nome deveria ser texto: {name:?}") };
        assert!(name.to_string_lossy().starts_with("_generated_diagnostic_collection_name_#"), "{}", name.to_string_lossy());
    });
}

#[test]
fn markers_published_by_the_workbench_come_back_in_get_diagnostics() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, observed| {
        observed.marker_service.change_one("typescript", &project_file("a.txt"), vec![MarkerData::new(MarkerSeverity::ERROR, "do workbench".to_owned(), 1, 1, 1, 4)], None);
        workbench.pump();

        assert_eq!(run(workbench, "t.getDiagnostics"), text("/proj/a.txt=do workbench|do workbench"));
    });
}

#[test]
fn on_did_change_diagnostics_fires_with_the_changed_uris_after_the_debounce() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, _observed| {
        run(workbench, "t.create");
        run(workbench, "t.setMany");
        std::thread::sleep(Duration::from_millis(120));
        workbench.pump();

        let seen = events(workbench);
        assert!(seen.contains(r#""diagnostics:/proj/a.txt,/proj/b.txt""#), "o evento junta as Uris do mesmo lote: {seen}");
    });
}

#[test]
fn get_languages_comes_from_the_workbench() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, _observed| {
        let expected = format!("[{}]", LANGUAGE_IDS.iter().map(|id| format!("\"{id}\"")).collect::<Vec<_>>().join(","));
        assert_eq!(run(workbench, "t.languages"), text(&expected));
    });
}

#[test]
fn match_scores_selectors_against_a_document() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, observed| {
        let model = create_document(&observed, "a.txt", "linha um");
        // plaintext casa exato (10), typescript não casa (0), '*' casa por curinga (5), o filtro com linguagem e esquema casa exato (10), o
        // esquema errado não casa (0), a lista leva o maior valor (10 pelo esquema `file`) e o padrão de arquivo casa (10)
        assert_eq!(run(workbench, "t.match"), text("[10,0,5,10,0,10,10]"));
        model.dispose();
    });
}

#[test]
fn set_text_document_language_reaches_the_workbench_and_returns_the_document() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, observed| {
        let model = create_document(&observed, "a.txt", "linha um");
        assert_eq!(run(workbench, "t.setLanguage"), text("/proj/a.txt"));
        assert_eq!(*observed.languages.calls.borrow(), vec!["changeLanguage /proj/a.txt typescript".to_owned()]);
        model.dispose();
    });
}
