//! Teste de ponta a ponta dos provedores de `vscode.languages` (não é porte do oráculo): uma extensão de exemplo em JavaScript, ativada pelo
//! host local (`AbstractExtensionService` + `LocalExtensionHost` + `RPCProtocol`), registra provedores de completar, hover, definição, ação de
//! código, formatação (documento e intervalo) e símbolos, com classes de usuário (o `this` do provedor funciona) e com objetos literais.
//!
//! O lado do workbench é o do `support/extension_host_api_languages_support.rs`: os documentos de texto reais e um
//! `MainThreadLanguageFeatures` que registra o handle de cada `$register*`. O teste faz o papel do `MainThreadLanguageFeatures` real: chama os
//! `$provide*` pelo proxy `ExtHostLanguageFeaturesShape` que o contexto em processo entrega (o encaminhador `Late`, que repassa ao
//! `ExtHostLanguageFeatures` do host) e confere o resultado convertido para o formato do editor. Os adaptadores e os DTOs têm a suíte de
//! unidade própria em `ext_host_language_features.rs`; aqui entram a ponte (`vscode.languages.register*Provider`), as classes de valor e a
//! conversão do que o JavaScript devolve.
//!
//! - **Registro**: cada `register*Provider` chega ao cliente com o seu handle, o `Disposable` devolvido desfaz o registro (`$unregister`) e os
//!   metadados de `registerCodeActionsProvider` e `registerDocumentSymbolProvider` são lidos.
//! - **Completar**: `CompletionList` com `CompletionItem` (rótulo texto e rótulo com descrição, `SnippetString`, `MarkdownString`), o contexto
//!   (`triggerKind`, `triggerCharacter`) e `isIncomplete`.
//! - **Hover**: `Hover` com `MarkdownString`, texto e bloco de código, e a exceção do provedor.
//! - **Definição**: `Location[]`. **Formatação**: `TextEdit.replace` e `TextEdit.setEndOfLine`, e a versão `async` do intervalo.
//! - **Símbolos**: `DocumentSymbol` e a ação de código (só o registro: o `CodeActionContext` do editor pede o serviço de ações).
//! - **Classes**: `CodeActionKind` (`append`, `contains`, `intersects`) e as divergências de `language_types`.

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

use std::rc::Rc;

use harness::{run_extension, settled_ok, Workbench};
use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_editor::languages::{CompletionContext, CompletionTriggerKind};
use je_editor::model::EndOfLineSequence;
use je_editor::position::Position as EditorPosition;
use je_editor::range::Range as EditorRange;
use je_exthost::in_process::IMainThreadBinder;
use je_workbench::api::common::ext_host_protocol::FormattingOptions;
use languages_support::{create_document, languages_setup, project_file, Observed};

/// O JavaScript da extensão de exemplo.
const EXTENSION_SOURCE: &str = r#"
const vscode = require('vscode');
exports.activate = (context) => {
  globalThis.__activated = true;
  globalThis.__calls = [];
  const note = (text) => globalThis.__calls.push(text);
  const command = (id, handler) => context.subscriptions.push(vscode.commands.registerCommand(id, handler));
  const selector = { scheme: 'file' };
  const pos = (position) => position.line + ',' + position.character;
  const describe = (range) => pos(range.start) + '-' + pos(range.end);
  const at = (document, position) => document.uri.path + '@' + pos(position);

  // uma classe de usuário: os métodos moram no protótipo e usam `this`
  class Named {
    constructor(name) { this.name = name; }
    provideHover(document, position, token) {
      note('hover:' + this.name + ':' + at(document, position) + ':' + typeof token.isCancellationRequested);
      return new vscode.Hover([new vscode.MarkdownString('**' + this.name + '**'), 'texto', { language: 'rust', value: 'fn main() {}' }], new vscode.Range(position, position));
    }
    provideDefinition(document, position, token) {
      note('definition:' + this.name + ':' + at(document, position));
      return [new vscode.Location(document.uri, new vscode.Range(1, 0, 1, 5))];
    }
    provideCompletionItems(document, position, token, completionContext) {
      note('completion:' + this.name + ':' + at(document, position) + ':' + completionContext.triggerKind + ':' + String(completionContext.triggerCharacter));
      const first = new vscode.CompletionItem('alfa', vscode.CompletionItemKind.Keyword);
      first.detail = 'detalhe';
      first.insertText = new vscode.SnippetString('alfa($0)');
      first.documentation = new vscode.MarkdownString('documentação');
      const second = new vscode.CompletionItem({ label: 'beta', description: 'pacote' }, vscode.CompletionItemKind.Function);
      second.sortText = 'z';
      return new vscode.CompletionList([first, second], true);
    }
    provideDocumentFormattingEdits(document, options, token) {
      note('format:' + this.name + ':' + document.uri.path + ':' + options.tabSize + ':' + options.insertSpaces);
      return [vscode.TextEdit.replace(new vscode.Range(0, 0, 0, 5), 'LINHA'), vscode.TextEdit.setEndOfLine(vscode.EndOfLine.LF)];
    }
    async provideDocumentRangeFormattingEdits(document, range, options, token) {
      note('range:' + this.name + ':' + describe(range) + ':' + options.tabSize);
      return [vscode.TextEdit.insert(range.start, '>> ')];
    }
    provideDocumentSymbols(document, token) {
      note('symbols:' + this.name + ':' + document.uri.path);
      const symbol = new vscode.DocumentSymbol('main', 'fn', vscode.SymbolKind.Function, new vscode.Range(0, 0, 1, 0), new vscode.Range(0, 0, 0, 4));
      symbol.children.push(new vscode.DocumentSymbol('interno', '', vscode.SymbolKind.Variable, new vscode.Range(0, 1, 0, 3), new vscode.Range(0, 1, 0, 3)));
      return [symbol];
    }
  }

  const registrations = {
    completion: vscode.languages.registerCompletionItemProvider(selector, new Named('comp'), '.', ':'),
    hover: vscode.languages.registerHoverProvider(selector, new Named('hov')),
    evilHover: vscode.languages.registerHoverProvider({ scheme: 'file', language: 'rust' }, { provideHover() { throw new Error('hover ruim'); } }),
    definition: vscode.languages.registerDefinitionProvider(selector, new Named('def')),
    codeActions: vscode.languages.registerCodeActionsProvider(selector, { provideCodeActions() { return []; } }, {
      providedCodeActionKinds: [vscode.CodeActionKind.QuickFix, vscode.CodeActionKind.Refactor.append('extract')],
    }),
    format: vscode.languages.registerDocumentFormattingEditProvider(selector, new Named('fmt')),
    rangeFormat: vscode.languages.registerDocumentRangeFormattingEditProvider(selector, new Named('rng')),
    symbols: vscode.languages.registerDocumentSymbolProvider(selector, new Named('sym'), { label: 'Símbolos de teste' }),
  };

  command('t.disposeHover', () => {
    registrations.hover.dispose();
    return 'ok';
  });
  command('t.codeActionKind', () => {
    const kinds = vscode.CodeActionKind;
    return JSON.stringify({
      quickFix: kinds.QuickFix.value,
      extract: kinds.Refactor.append('extract').value,
      contains: kinds.Refactor.contains(kinds.RefactorExtract),
      intersects: kinds.RefactorExtract.intersects(kinds.QuickFix),
      fixAll: kinds.SourceFixAll.value,
    });
  });
  command('t.valueClasses', () => {
    const edit = vscode.TextEdit.insert(new vscode.Position(1, 2), 'x');
    const location = new vscode.Location(vscode.Uri.file('/proj/a.txt'), new vscode.Position(3, 4));
    const markdown = new vscode.MarkdownString('texto', true);
    let missingMethod;
    try { markdown.appendText('mais'); missingMethod = 'sem erro'; } catch (error) { missingMethod = error.constructor.name; }
    return JSON.stringify({
      edit: describe(edit.range) + ':' + edit.newText,
      location: location.uri.path + ':' + describe(location.range),
      markdown: markdown.value + ':' + markdown.supportThemeIcons,
      missingMethod,
      severity: vscode.DiagnosticSeverity[vscode.DiagnosticSeverity.Hint],
      kind: vscode.SymbolKind[vscode.SymbolKind.Function] + ':' + vscode.CompletionItemKind[vscode.CompletionItemKind.Snippet],
    });
  });
};
"#;

fn setup(_workbench: &Workbench) -> (Rc<dyn IMainThreadBinder>, Observed) {
    languages_setup()
}

fn calls(workbench: &Workbench) -> String {
    workbench.probe("JSON.stringify(globalThis.__calls)")
}

fn formatting_options() -> FormattingOptions {
    FormattingOptions { tab_size: 4 as _, insert_spaces: true }
}

#[test]
fn every_provider_registration_reaches_the_workbench_and_dispose_unregisters_it() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, observed| {
        let features = &observed.language_features;
        for kind in [
            "rpc_register_completions_provider",
            "rpc_register_hover_provider",
            "rpc_register_definition_support",
            "rpc_register_code_action_support",
            "rpc_register_document_formatting_support",
            "rpc_register_range_formatting_support",
            "rpc_register_document_symbol_provider",
        ] {
            assert!(!features.handles_of(kind).is_empty(), "o registro {kind} deveria ter chegado: {:?}", features.registered.borrow());
        }
        assert_eq!(features.handles_of("rpc_register_hover_provider").len(), 2, "os dois provedores de hover (a classe e o objeto literal)");

        let hover = features.handle_of("rpc_register_hover_provider");
        assert!(features.unregistered.borrow().is_empty());
        assert_eq!(workbench.execute("t.disposeHover", Vec::new()), Dynamic::string("ok"));
        assert_eq!(*features.unregistered.borrow(), vec![hover], "o Disposable do registro desfaz só aquele registro");
    });
}

#[test]
fn hover_provider_keeps_this_and_its_rich_result_converts() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, observed| {
        let model = create_document(&observed, "a.txt", "linha um\nlinha dois\nterceira");
        let hover = observed.language_features.handle_of("rpc_register_hover_provider");

        let pending = observed.features_proxy().rpc_provide_hover(hover, &project_file("a.txt"), EditorPosition::new(2, 3), None, CancellationToken::None);
        workbench.pump();
        assert!(settled_ok(&pending, "provideHover").is_some(), "o Hover com MarkdownString, texto e bloco de código converte");
        assert_eq!(calls(workbench), r#"["hover:hov:/proj/a.txt@1,2:boolean"]"#, "o `this` da classe e o documento e a posição chegam ao provedor");
        model.dispose();
    });
}

#[test]
fn a_provider_that_throws_rejects_the_request() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, observed| {
        let model = create_document(&observed, "a.txt", "linha um");
        let evil = observed.language_features.handles_of("rpc_register_hover_provider")[1];

        let pending = observed.features_proxy().rpc_provide_hover(evil, &project_file("a.txt"), EditorPosition::new(1, 1), None, CancellationToken::None);
        workbench.pump();
        match pending.result() {
            Some(Err(error)) => assert!(error.message.contains("hover ruim"), "{}", error.message),
            other => panic!("a exceção do provedor deveria rejeitar a promessa: {:?}", other.map(|result| result.is_ok())),
        }
        model.dispose();
    });
}

#[test]
fn definition_provider_converts_a_location_list() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, observed| {
        let model = create_document(&observed, "a.txt", "linha um\nlinha dois\nterceira");
        let definition = observed.language_features.handle_of("rpc_register_definition_support");

        let pending = observed.features_proxy().rpc_provide_definition(definition, &project_file("a.txt"), EditorPosition::new(2, 3), CancellationToken::None);
        workbench.pump();
        let links = settled_ok(&pending, "provideDefinition");
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].uri.path().to_string(), "/proj/a.txt");
        assert_eq!(links[0].range, EditorRange::new(2, 1, 2, 6), "o intervalo vira base 1");
        assert_eq!(calls(workbench), r#"["definition:def:/proj/a.txt@1,2"]"#);
        model.dispose();
    });
}

#[test]
fn completion_provider_receives_the_context_and_its_list_converts() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, observed| {
        let model = create_document(&observed, "a.txt", "linha um\nlinha dois\nterceira");
        let completion = observed.language_features.handle_of("rpc_register_completions_provider");

        let context = CompletionContext { trigger_kind: CompletionTriggerKind::Invoke, trigger_character: None };
        let pending = observed.features_proxy().rpc_provide_completion_items(completion, &project_file("a.txt"), EditorPosition::new(2, 3), context, CancellationToken::None);
        workbench.pump();
        let result = settled_ok(&pending, "provideCompletionItems").expect("a lista de completar");
        assert_eq!(result.completions.len(), 2);
        assert_eq!(result.is_incomplete, Some(true), "isIncomplete da CompletionList chega ao editor");
        assert_eq!(calls(workbench), r#"["completion:comp:/proj/a.txt@1,2:0:undefined"]"#);
        model.dispose();
    });
}

#[test]
fn formatting_providers_convert_the_edits_for_the_document_and_for_the_range() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, observed| {
        let model = create_document(&observed, "a.txt", "linha um\nlinha dois");
        let format = observed.language_features.handle_of("rpc_register_document_formatting_support");
        let range_format = observed.language_features.handle_of("rpc_register_range_formatting_support");

        let pending = observed.features_proxy().rpc_provide_document_formatting_edits(format, &project_file("a.txt"), formatting_options(), CancellationToken::None);
        workbench.pump();
        let edits = settled_ok(&pending, "provideDocumentFormattingEdits").expect("as edições");
        assert_eq!(edits.len(), 2);
        assert_eq!(edits[0].text, "LINHA");
        assert_eq!(edits[0].range, EditorRange::new(1, 1, 1, 6));
        assert_eq!(edits[1].eol, Some(EndOfLineSequence::Lf), "TextEdit.setEndOfLine");

        let pending = observed.features_proxy().rpc_provide_document_range_formatting_edits(
            range_format,
            &project_file("a.txt"),
            EditorRange::new(2, 2, 2, 6),
            formatting_options(),
            CancellationToken::None,
        );
        workbench.pump();
        let edits = settled_ok(&pending, "provideDocumentRangeFormattingEdits").expect("as edições");
        assert_eq!(edits.len(), 1, "o provedor async devolve uma promessa que o host adota");
        assert_eq!(edits[0].text, ">> ");
        assert_eq!(edits[0].range, EditorRange::new(2, 2, 2, 2), "TextEdit.insert é um intervalo vazio na posição");

        assert_eq!(calls(workbench), r#"["format:fmt:/proj/a.txt:4:true","range:rng:1,1-1,5:4"]"#);
        model.dispose();
    });
}

#[test]
fn document_symbol_provider_converts_the_tree() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, observed| {
        let model = create_document(&observed, "a.txt", "linha um\nlinha dois");
        let symbols = observed.language_features.handle_of("rpc_register_document_symbol_provider");

        let pending = observed.features_proxy().rpc_provide_document_symbols(symbols, &project_file("a.txt"), CancellationToken::None);
        workbench.pump();
        let symbols = settled_ok(&pending, "provideDocumentSymbols").expect("os símbolos");
        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].name, "main");
        assert_eq!(symbols[0].range, EditorRange::new(1, 1, 2, 1));
        assert_eq!(calls(workbench), r#"["symbols:sym:/proj/a.txt"]"#);
        model.dispose();
    });
}

#[test]
fn code_action_kind_has_the_static_members_and_the_value_helpers() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, _observed| {
        assert_eq!(
            workbench.execute("t.codeActionKind", Vec::new()),
            Dynamic::string(r#"{"quickFix":"quickfix","extract":"refactor.extract","contains":true,"intersects":false,"fixAll":"source.fixAll"}"#)
        );
    });
}

#[test]
fn value_classes_build_the_objects_of_the_original() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, _observed| {
        assert_eq!(
            workbench.execute("t.valueClasses", Vec::new()),
            Dynamic::string(concat!(
                r#"{"edit":"1,2-1,2:x","location":"/proj/a.txt:3,4-3,4","markdown":"texto:true","missingMethod":"TypeError","#,
                r#""severity":"Hint","kind":"Function:Snippet"}"#,
            ))
        );
    });
}
