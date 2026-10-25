//! Teste de ponta a ponta dos provedores de `vscode.languages` da terceira leva (não é porte do oráculo): uma extensão de exemplo em
//! JavaScript, ativada pelo host local (`AbstractExtensionService` + `LocalExtensionHost` + `RPCProtocol`), registra links de documento
//! (com `resolveDocumentLink`), cores (com `provideColorPresentations`), hierarquia de chamadas, hierarquia de tipos e símbolos do espaço de
//! trabalho (com `resolveWorkspaceSymbol`), tudo com classes de usuário (o `this` do provedor funciona).
//!
//! O lado do workbench é o do `support/extension_host_api_languages_support.rs` (o mesmo de `extension_host_api_languages_providers2.rs`):
//! os documentos de texto reais e um `MainThreadLanguageFeatures` que registra o handle de cada `$register*`. O teste faz o papel do
//! `MainThreadLanguageFeatures` real: chama os `$provide*` e `$resolve*` pelo proxy `ExtHostLanguageFeaturesShape` e confere o resultado
//! convertido para o formato do editor. As suítes de unidade de `ext_host_language_features.rs` cobrem os adaptadores; aqui entram a ponte
//! (`vscode.languages.register*Provider`), as classes de valor, a leitura do que o JavaScript devolve e o `typeof provider.resolveX ===
//! 'function'` lido no objeto do provedor.
//!
//! - **Registro**: cada `register*Provider` chega ao cliente com o seu handle e o `Disposable` desfaz o registro (`$unregister`).
//! - **Links**: a lista com `DocumentLink` e o `resolveDocumentLink` que recebe o objeto original (com a propriedade extra).
//! - **Cores**: `ColorInformation` com `Color` e as `ColorPresentation` (rótulo, edição e edições adicionais).
//! - **Hierarquia de chamadas**: `prepareCallHierarchy` e as chamadas de entrada e de saída a partir do item preparado.
//! - **Hierarquia de tipos**: `prepareTypeHierarchy` (lista e item único), supertipos e subtipos.
//! - **Símbolos do espaço de trabalho**: a consulta, as duas formas do construtor de `SymbolInformation` e o `resolveWorkspaceSymbol`.
//! - **Classes**: os valores de construtor.

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
use je_editor::position::Position as EditorPosition;
use je_editor::range::Range as EditorRange;
use je_exthost::in_process::IMainThreadBinder;
use je_workbench::api::common::ext_host_protocol::IRawColorInfo;
use languages_support::{create_document, languages_setup, project_file, Observed};

/// O texto do documento de exemplo (cinco linhas).
const DOCUMENT: &str = "linha um\nlinha dois\nterceira\nquarta\nquinta";

/// O JavaScript da extensão de exemplo.
const EXTENSION_SOURCE: &str = r#"
const vscode = require('vscode');
exports.activate = (context) => {
  globalThis.__calls = [];
  const note = (text) => globalThis.__calls.push(text);
  const command = (id, handler) => context.subscriptions.push(vscode.commands.registerCommand(id, handler));
  const selector = { scheme: 'file' };
  const pos = (position) => position.line + ',' + position.character;
  const describe = (range) => pos(range.start) + '-' + pos(range.end);

  const item = (document, name, kind) =>
    new vscode.CallHierarchyItem(kind, name, 'detalhe', document.uri, new vscode.Range(0, 0, 1, 0), new vscode.Range(0, 0, 0, 4));

  // uma classe de usuário: os métodos moram no protótipo e usam `this`
  class Named {
    constructor(name) {
      this.name = name;
    }
    provideDocumentLinks(document, token) {
      note('links:' + this.name + ':' + document.uri.path + ':' + typeof token.isCancellationRequested);
      const ready = new vscode.DocumentLink(new vscode.Range(0, 0, 0, 5), vscode.Uri.parse('https://exemplo.com/um'));
      ready.tooltip = 'abrir';
      const lazy = new vscode.DocumentLink(new vscode.Range(1, 0, 1, 5));
      lazy.tag = 'tag-' + this.name;
      return [ready, lazy];
    }
    resolveDocumentLink(link, token) {
      note('resolveLink:' + this.name + ':' + link.tag + ':' + describe(link.range));
      link.target = vscode.Uri.parse('https://exemplo.com/resolvido');
      return link;
    }
    provideDocumentColors(document, token) {
      note('colors:' + this.name + ':' + document.uri.path);
      return [new vscode.ColorInformation(new vscode.Range(0, 0, 0, 5), new vscode.Color(0.1, 0.2, 0.3, 0.4))];
    }
    provideColorPresentations(color, context, token) {
      note('presentations:' + this.name + ':' + [color.red, color.green, color.blue, color.alpha].join('|') + ':' + context.document.uri.path + ':' + describe(context.range));
      const presentation = new vscode.ColorPresentation('#1a334d');
      presentation.textEdit = vscode.TextEdit.replace(context.range, '#1a334d');
      presentation.additionalTextEdits = [vscode.TextEdit.insert(new vscode.Position(0, 0), '/* cor */')];
      return [presentation, new vscode.ColorPresentation('rgb(26, 51, 77)')];
    }
    prepareCallHierarchy(document, position, token) {
      note('prepareCall:' + this.name + ':' + document.uri.path + '@' + pos(position));
      return item(document, 'alvo', vscode.SymbolKind.Function);
    }
    provideCallHierarchyIncomingCalls(callItem, token) {
      note('incoming:' + this.name + ':' + callItem.name + ':' + callItem.detail + ':' + callItem.kind + ':' + describe(callItem.selectionRange));
      return [new vscode.CallHierarchyIncomingCall(item({ uri: callItem.uri }, 'chamador', vscode.SymbolKind.Method), [new vscode.Range(2, 0, 2, 3)])];
    }
    provideCallHierarchyOutgoingCalls(callItem, token) {
      note('outgoing:' + this.name + ':' + callItem.name);
      return [new vscode.CallHierarchyOutgoingCall(item({ uri: callItem.uri }, 'chamado', vscode.SymbolKind.Function), [new vscode.Range(3, 0, 3, 2), new vscode.Range(4, 0, 4, 2)])];
    }
    prepareTypeHierarchy(document, position, token) {
      note('prepareType:' + this.name + ':' + document.uri.path + '@' + pos(position));
      return [new vscode.TypeHierarchyItem(vscode.SymbolKind.Class, 'Base', 'tipo', document.uri, new vscode.Range(0, 0, 1, 0), new vscode.Range(0, 0, 0, 4))];
    }
    provideTypeHierarchySupertypes(typeItem, token) {
      note('supertypes:' + this.name + ':' + typeItem.name);
      return [new vscode.TypeHierarchyItem(vscode.SymbolKind.Interface, 'Super', 'super', typeItem.uri, new vscode.Range(2, 0, 3, 0), new vscode.Range(2, 0, 2, 5))];
    }
    provideTypeHierarchySubtypes(typeItem, token) {
      note('subtypes:' + this.name + ':' + typeItem.name);
      return [];
    }
    provideWorkspaceSymbols(query, token) {
      note('symbols:' + this.name + ':' + query);
      const byLocation = new vscode.SymbolInformation('alfa', vscode.SymbolKind.Function, 'modulo', new vscode.Location(vscode.Uri.file('/proj/a.txt'), new vscode.Range(0, 0, 0, 4)));
      const byRange = new vscode.SymbolInformation('beta', vscode.SymbolKind.Class, new vscode.Range(2, 0, 2, 4), vscode.Uri.file('/proj/a.txt'));
      byRange.tag = 'tag-' + this.name;
      return [byLocation, byRange];
    }
    resolveWorkspaceSymbol(symbol, token) {
      note('resolveSymbol:' + this.name + ':' + symbol.name + ':' + symbol.kind + ':' + describe(symbol.location.range));
      return symbol;
    }
  }

  const registrations = {
    links: vscode.languages.registerDocumentLinkProvider(selector, new Named('lnk')),
    colors: vscode.languages.registerColorProvider(selector, new Named('col')),
    calls: vscode.languages.registerCallHierarchyProvider(selector, new Named('cal')),
    types: vscode.languages.registerTypeHierarchyProvider(selector, new Named('typ')),
    symbols: vscode.languages.registerWorkspaceSymbolProvider(new Named('sym')),
    plainSymbols: vscode.languages.registerWorkspaceSymbolProvider({ provideWorkspaceSymbols() { return undefined; } }),
  };

  command('t.disposeLinks', () => {
    registrations.links.dispose();
    return 'ok';
  });
  command('t.valueClasses', () => {
    const uri = vscode.Uri.file('/proj/a.txt');
    const link = new vscode.DocumentLink(new vscode.Range(0, 1, 0, 4), uri);
    const color = new vscode.Color(1, 0.5, 0, 1);
    const information = new vscode.ColorInformation(new vscode.Range(1, 0, 1, 3), color);
    const presentation = new vscode.ColorPresentation('#ff8000');
    const callItem = new vscode.CallHierarchyItem(vscode.SymbolKind.Function, 'f', 'd', uri, new vscode.Range(0, 0, 2, 0), new vscode.Range(0, 0, 0, 1));
    const incoming = new vscode.CallHierarchyIncomingCall(callItem, [new vscode.Range(1, 0, 1, 1)]);
    const outgoing = new vscode.CallHierarchyOutgoingCall(callItem, []);
    const typeItem = new vscode.TypeHierarchyItem(vscode.SymbolKind.Class, 'C', 'd', uri, new vscode.Range(0, 0, 2, 0), new vscode.Range(0, 0, 0, 1));
    const byLocation = new vscode.SymbolInformation('s', vscode.SymbolKind.Variable, 'c', new vscode.Location(uri, new vscode.Range(0, 0, 0, 1)));
    const byRange = new vscode.SymbolInformation('r', vscode.SymbolKind.Variable, new vscode.Range(1, 0, 1, 2), uri, 'k');
    let emptyLink = 'sem erro';
    try {
      new vscode.DocumentLink(new vscode.Range(0, 0, 0, 0), uri);
    } catch (error) {
      emptyLink = 'erro';
    }
    return JSON.stringify({
      link: describe(link.range) + ':' + link.target.path,
      color: [color.red, color.green, color.blue, color.alpha].join(','),
      information: describe(information.range) + ':' + information.color.green,
      presentation: presentation.label + ':' + String(presentation.textEdit),
      callItem: callItem.name + ':' + callItem.kind + ':' + describe(callItem.range) + ':' + describe(callItem.selectionRange),
      incoming: incoming.from.name + ':' + incoming.fromRanges.length,
      outgoing: outgoing.to.name + ':' + outgoing.fromRanges.length,
      typeItem: typeItem.name + ':' + typeItem.detail + ':' + typeItem.kind,
      byLocation: byLocation.name + ':' + byLocation.containerName + ':' + describe(byLocation.location.range),
      byRange: byRange.name + ':' + byRange.containerName + ':' + describe(byRange.location.range) + ':' + byRange.location.uri.path,
      emptyLink,
    });
  });
  globalThis.__activated = true;
};
"#;

fn setup(_workbench: &Workbench) -> (Rc<dyn IMainThreadBinder>, Observed) {
    languages_setup()
}

fn calls(workbench: &Workbench) -> String {
    workbench.probe("JSON.stringify(globalThis.__calls)")
}

#[test]
fn every_new_registration_reaches_the_workbench_and_dispose_unregisters_it() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, observed| {
        let features = &observed.language_features;
        for kind in [
            "rpc_register_document_link_provider",
            "rpc_register_document_color_provider",
            "rpc_register_call_hierarchy_provider",
            "rpc_register_type_hierarchy_provider",
            "rpc_register_navigate_type_support",
        ] {
            assert!(!features.handles_of(kind).is_empty(), "o registro {kind} deveria ter chegado: {:?}", features.registered.borrow());
        }
        assert_eq!(features.handles_of("rpc_register_navigate_type_support").len(), 2, "o provedor com classe e o objeto simples");

        let links = features.handle_of("rpc_register_document_link_provider");
        assert!(features.unregistered.borrow().is_empty());
        assert_eq!(workbench.execute("t.disposeLinks", Vec::new()), Dynamic::string("ok"));
        assert_eq!(*features.unregistered.borrow(), vec![links], "o Disposable do registro desfaz só aquele registro");
    });
}

#[test]
fn document_link_provider_resolves_the_lazy_link_with_its_original_object() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, observed| {
        let model = create_document(&observed, "a.txt", DOCUMENT);
        let handle = observed.language_features.handle_of("rpc_register_document_link_provider");

        let pending = observed.features_proxy().rpc_provide_document_links(handle, &project_file("a.txt"), CancellationToken::None);
        workbench.pump();
        let list = settled_ok(&pending, "provideDocumentLinks").expect("a lista de links");
        assert_eq!(list.links.len(), 2);
        assert!(list.links[0].url.is_some(), "o link pronto já traz o destino");
        assert_eq!(list.links[0].tooltip.as_deref(), Some("abrir"));
        assert_eq!(list.links[0].range, EditorRange::new(1, 1, 1, 6), "o intervalo vira base 1");
        assert!(list.links[1].url.is_none(), "o link preguiçoso espera o resolve");

        let id = list.links[1].cache_id.expect("o link guardado no cache");
        let pending = observed.features_proxy().rpc_resolve_document_link(handle, id, CancellationToken::None);
        workbench.pump();
        let resolved = settled_ok(&pending, "resolveDocumentLink").expect("o link resolvido");
        assert!(resolved.url.is_some(), "o resolveDocumentLink recebeu o objeto original (com a propriedade tag) e devolveu o destino");
        assert_eq!(calls(workbench), r#"["links:lnk:/proj/a.txt:boolean","resolveLink:lnk:tag-lnk:1,0-1,5"]"#);
        model.dispose();
    });
}

#[test]
fn color_provider_lists_colors_and_presentations() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, observed| {
        let model = create_document(&observed, "a.txt", DOCUMENT);
        let handle = observed.language_features.handle_of("rpc_register_document_color_provider");

        let pending = observed.features_proxy().rpc_provide_document_colors(handle, &project_file("a.txt"), CancellationToken::None);
        workbench.pump();
        let colors = settled_ok(&pending, "provideDocumentColors");
        assert_eq!(colors.len(), 1);
        assert_eq!(colors[0].color, [0.1, 0.2, 0.3, 0.4]);
        assert_eq!(colors[0].range, EditorRange::new(1, 1, 1, 6));

        let info = IRawColorInfo { color: [0.1, 0.2, 0.3, 0.4], range: EditorRange::new(1, 1, 1, 6) };
        let pending = observed.features_proxy().rpc_provide_color_presentations(handle, &project_file("a.txt"), info, CancellationToken::None);
        workbench.pump();
        let presentations = settled_ok(&pending, "provideColorPresentations").expect("as apresentações");
        assert_eq!(presentations.len(), 2);
        assert_eq!(presentations[0].label, "#1a334d");
        assert!(presentations[0].text_edit.is_some());
        assert_eq!(presentations[0].additional_text_edits.as_ref().map(Vec::len), Some(1));
        assert!(presentations[1].text_edit.is_none());

        assert_eq!(calls(workbench), r#"["colors:col:/proj/a.txt","presentations:col:0.1|0.2|0.3|0.4:/proj/a.txt:0,0-0,5"]"#);
        model.dispose();
    });
}

#[test]
fn call_hierarchy_provider_prepares_the_item_and_lists_both_directions() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, observed| {
        let model = create_document(&observed, "a.txt", DOCUMENT);
        let handle = observed.language_features.handle_of("rpc_register_call_hierarchy_provider");

        let pending = observed.features_proxy().rpc_prepare_call_hierarchy(handle, &project_file("a.txt"), EditorPosition::new(2, 3), CancellationToken::None);
        workbench.pump();
        let items = settled_ok(&pending, "prepareCallHierarchy").expect("os itens preparados");
        assert_eq!(items.len(), 1, "um item único vira lista de um");
        assert_eq!(items[0].name, "alvo");
        assert_eq!(items[0].selection_range, EditorRange::new(1, 1, 1, 5));

        let pending = observed.features_proxy().rpc_provide_call_hierarchy_incoming_calls(handle, &items[0].session_id, &items[0].item_id, CancellationToken::None);
        workbench.pump();
        let incoming = settled_ok(&pending, "provideCallHierarchyIncomingCalls").expect("as chamadas de entrada");
        assert_eq!(incoming.len(), 1);
        assert_eq!(incoming[0].from.name, "chamador");
        assert_eq!(incoming[0].from_ranges, vec![EditorRange::new(3, 1, 3, 4)]);

        let pending = observed.features_proxy().rpc_provide_call_hierarchy_outgoing_calls(handle, &items[0].session_id, &items[0].item_id, CancellationToken::None);
        workbench.pump();
        let outgoing = settled_ok(&pending, "provideCallHierarchyOutgoingCalls").expect("as chamadas de saída");
        assert_eq!(outgoing.len(), 1);
        assert_eq!(outgoing[0].to.name, "chamado");
        assert_eq!(outgoing[0].from_ranges.len(), 2);

        assert_eq!(
            calls(workbench),
            r#"["prepareCall:cal:/proj/a.txt@1,2","incoming:cal:alvo:detalhe:11:0,0-0,4","outgoing:cal:alvo"]"#,
            "o provedor recebe o item preparado, com os campos da classe"
        );
        model.dispose();
    });
}

#[test]
fn type_hierarchy_provider_prepares_the_item_and_lists_supertypes_and_subtypes() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, observed| {
        let model = create_document(&observed, "a.txt", DOCUMENT);
        let handle = observed.language_features.handle_of("rpc_register_type_hierarchy_provider");

        let pending = observed.features_proxy().rpc_prepare_type_hierarchy(handle, &project_file("a.txt"), EditorPosition::new(1, 2), CancellationToken::None);
        workbench.pump();
        let items = settled_ok(&pending, "prepareTypeHierarchy").expect("os itens preparados");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].name, "Base");

        let pending = observed.features_proxy().rpc_provide_type_hierarchy_supertypes(handle, &items[0].session_id, &items[0].item_id, CancellationToken::None);
        workbench.pump();
        let supertypes = settled_ok(&pending, "provideTypeHierarchySupertypes").expect("os supertipos");
        assert_eq!(supertypes.len(), 1);
        assert_eq!(supertypes[0].name, "Super");

        let pending = observed.features_proxy().rpc_provide_type_hierarchy_subtypes(handle, &items[0].session_id, &items[0].item_id, CancellationToken::None);
        workbench.pump();
        assert!(settled_ok(&pending, "provideTypeHierarchySubtypes").expect("os subtipos").is_empty());

        assert_eq!(calls(workbench), r#"["prepareType:typ:/proj/a.txt@0,1","supertypes:typ:Base","subtypes:typ:Base"]"#);
        model.dispose();
    });
}

#[test]
fn workspace_symbol_provider_converts_both_constructor_forms_and_resolves_the_symbol() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, observed| {
        let handle = observed.language_features.handles_of("rpc_register_navigate_type_support")[0];

        let pending = observed.features_proxy().rpc_provide_workspace_symbols(handle, "al", CancellationToken::None);
        workbench.pump();
        let result = settled_ok(&pending, "provideWorkspaceSymbols");
        assert_eq!(result.symbols.len(), 2);
        assert_eq!(result.symbols[0].name, "alfa");
        assert_eq!(result.symbols[0].container_name.as_deref(), Some("modulo"));
        assert_eq!(result.symbols[1].name, "beta");
        assert_eq!(result.symbols[1].location.range, Some(EditorRange::new(3, 1, 3, 5)), "o construtor com intervalo e uri");

        let symbol = result.symbols.into_iter().nth(1).expect("o segundo símbolo");
        let pending = observed.features_proxy().rpc_resolve_workspace_symbol(handle, symbol, CancellationToken::None);
        workbench.pump();
        let resolved = settled_ok(&pending, "resolveWorkspaceSymbol").expect("o símbolo resolvido");
        assert_eq!(resolved.name, "beta");

        let calls = calls(workbench);
        assert!(calls.starts_with(r#"["symbols:sym:al","resolveSymbol:sym:beta:"#), "o provedor recebeu a consulta e o símbolo: {calls}");
        assert!(calls.ends_with(r#":2,0-2,4"]"#), "o símbolo chega ao resolve com a localização em base 0: {calls}");
    });
}

#[test]
fn value_classes_build_the_objects_of_the_original() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, _observed| {
        assert_eq!(
            workbench.execute("t.valueClasses", Vec::new()),
            Dynamic::string(concat!(
                r##"{"link":"0,1-0,4:/proj/a.txt","color":"1,0.5,0,1","information":"1,0-1,3:0.5","presentation":"#ff8000:undefined","##,
                r#""callItem":"f:11:0,0-2,0:0,0-0,1","incoming":"f:1","outgoing":"f:0","typeItem":"C:d:4","#,
                r#""byLocation":"s:c:0,0-0,1","byRange":"r:k:1,0-1,2:/proj/a.txt","emptyLink":"erro"}"#,
            ))
        );
    });
}
