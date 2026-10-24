//! Teste de ponta a ponta dos provedores de `vscode.languages` da segunda leva (não é porte do oráculo): uma extensão de exemplo em
//! JavaScript, ativada pelo host local (`AbstractExtensionService` + `LocalExtensionHost` + `RPCProtocol`), registra lentes de código (com
//! `resolveCodeLens`), referências, destaque de ocorrências, renomeação (com `prepareRename`), ajuda de assinatura (caracteres soltos e
//! `SignatureHelpProviderMetadata`), formatação de vários intervalos e ao digitar, dobra, seleção inteligente, tokens semânticos (documento e
//! intervalo), dicas embutidas (com `resolveInlayHint`) e completar com `resolveCompletionItem`, tudo com classes de usuário (o `this` do
//! provedor funciona).
//!
//! O lado do workbench é o do `support/extension_host_api_languages_support.rs` (o mesmo de `extension_host_api_languages_providers.rs`): os
//! documentos de texto reais e um `MainThreadLanguageFeatures` que registra o handle de cada `$register*`. O teste faz o papel do
//! `MainThreadLanguageFeatures` real: chama os `$provide*` e `$resolve*` pelo proxy `ExtHostLanguageFeaturesShape` e confere o resultado
//! convertido para o formato do editor. As suítes de unidade de `ext_host_language_features.rs` cobrem os adaptadores; aqui entram a ponte
//! (`vscode.languages.register*Provider`), as classes de valor, a leitura do que o JavaScript devolve e o `typeof provider.resolveX ===
//! 'function'` lido no objeto do provedor.
//!
//! - **Registro**: cada `register*Provider` chega ao cliente com o seu handle e o `Disposable` desfaz o registro (`$unregister`).
//! - **Lente de código**: a lente pronta e a preguiçosa, e o `resolveCodeLens` que recebe o objeto original (com a propriedade extra).
//! - **Referências e destaque**: o contexto `{ includeDeclaration }` e as listas de `Location` e de `DocumentHighlight`.
//! - **Renomeação**: a `WorkspaceEdit` criada pela extensão e o `prepareRename` com `{ range, placeholder }`.
//! - **Assinatura**: `SignatureHelp` com `SignatureInformation` e `ParameterInformation`, e o contexto do disparo.
//! - **Formatação**: `provideDocumentRangesFormattingEdits` (só chamado porque o provedor o tem) e `provideOnTypeFormattingEdits`.
//! - **Dobra e seleção**: `FoldingRange` e a cadeia de `SelectionRange` com pai.
//! - **Tokens semânticos**: `SemanticTokensBuilder` com e sem legenda, e o resultado nos dois provedores.
//! - **Dicas embutidas e completar**: rótulo em texto e em partes, e os `resolve` com o objeto original.
//! - **Classes**: os valores de construtor e os enums novos.

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
use je_editor::folding::syntax_range_provider::FoldingContext;
use je_editor::languages::{CompletionContext, CompletionTriggerKind};
use je_editor::parameter_hints::languages::SignatureHelpTriggerKind;
use je_editor::position::Position as EditorPosition;
use je_editor::range::Range as EditorRange;
use je_exthost::in_process::IMainThreadBinder;
use je_workbench::api::common::ext_host_protocol::{FormattingOptions, ISignatureHelpContextDto, InlayHintLabelDto, ReferenceContext};
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
  const at = (document, position) => document.uri.path + '@' + pos(position);
  const legend = new vscode.SemanticTokensLegend(['keyword', 'variable'], ['declaration']);

  const buildTokens = () => {
    const builder = new vscode.SemanticTokensBuilder(legend);
    builder.push(new vscode.Range(0, 0, 0, 5), 'keyword', ['declaration']);
    builder.push(1, 0, 3, 1);
    return builder.build('r1');
  };

  // uma classe de usuário: os métodos moram no protótipo e usam `this`
  class Named {
    constructor(name) {
      this.name = name;
      this.changed = new vscode.EventEmitter();
      this.onDidChangeCodeLenses = this.changed.event;
    }
    provideCodeLenses(document, token) {
      note('lens:' + this.name + ':' + document.uri.path + ':' + typeof token.isCancellationRequested);
      const ready = new vscode.CodeLens(new vscode.Range(0, 0, 0, 4), { title: 'pronto', command: 'noop' });
      const lazy = new vscode.CodeLens(new vscode.Range(1, 0, 1, 4));
      lazy.tag = 'tag-' + this.name;
      return [ready, lazy];
    }
    resolveCodeLens(lens, token) {
      note('resolveLens:' + this.name + ':' + lens.tag + ':' + describe(lens.range));
      lens.command = { title: 'resolvido ' + lens.tag, command: 'noop' };
      return lens;
    }
    provideReferences(document, position, context, token) {
      note('references:' + this.name + ':' + at(document, position) + ':' + context.includeDeclaration);
      return [new vscode.Location(document.uri, new vscode.Range(0, 0, 0, 5)), new vscode.Location(document.uri, new vscode.Range(2, 1, 2, 4))];
    }
    provideDocumentHighlights(document, position, token) {
      note('highlights:' + this.name + ':' + at(document, position));
      return [new vscode.DocumentHighlight(new vscode.Range(0, 0, 0, 5), vscode.DocumentHighlightKind.Write), new vscode.DocumentHighlight(new vscode.Range(2, 0, 2, 3))];
    }
    provideRenameEdits(document, position, newName, token) {
      note('rename:' + this.name + ':' + at(document, position) + ':' + newName);
      const edit = new vscode.WorkspaceEdit();
      edit.replace(document.uri, new vscode.Range(0, 0, 0, 5), newName);
      return edit;
    }
    prepareRename(document, position, token) {
      note('prepare:' + this.name + ':' + at(document, position));
      return { range: new vscode.Range(1, 0, 1, 5), placeholder: 'dois' };
    }
    provideSignatureHelp(document, position, token, context) {
      note('signature:' + this.name + ':' + at(document, position) + ':' + context.triggerKind + ':' + String(context.triggerCharacter) + ':' + context.isRetrigger + ':' + String(context.activeSignatureHelp));
      const help = new vscode.SignatureHelp();
      const signature = new vscode.SignatureInformation('f(a, b)', new vscode.MarkdownString('doc'));
      signature.parameters = [new vscode.ParameterInformation('a'), new vscode.ParameterInformation([3, 4], 'segundo')];
      help.signatures = [signature];
      help.activeParameter = 1;
      return help;
    }
    provideDocumentRangesFormattingEdits(document, ranges, options, token) {
      note('ranges:' + this.name + ':' + ranges.length + ':' + ranges.map((range) => describe(range)).join('|') + ':' + options.tabSize);
      return ranges.map((range) => vscode.TextEdit.insert(range.start, '>'));
    }
    provideOnTypeFormattingEdits(document, position, ch, options, token) {
      note('ontype:' + this.name + ':' + at(document, position) + ':' + ch + ':' + options.tabSize);
      return [vscode.TextEdit.insert(position, ch)];
    }
    provideFoldingRanges(document, context, token) {
      note('folding:' + this.name + ':' + typeof context);
      return [new vscode.FoldingRange(0, 2, vscode.FoldingRangeKind.Region), new vscode.FoldingRange(3, 4)];
    }
    provideSelectionRanges(document, positions, token) {
      note('selection:' + this.name + ':' + positions.map((position) => pos(position)).join('|'));
      return positions.map((position) => new vscode.SelectionRange(new vscode.Range(position.line, 0, position.line, 4), new vscode.SelectionRange(new vscode.Range(0, 0, 3, 0))));
    }
    provideDocumentSemanticTokens(document, token) {
      note('tokens:' + this.name + ':' + document.uri.path);
      return buildTokens();
    }
    provideDocumentRangeSemanticTokens(document, range, token) {
      note('rangeTokens:' + this.name + ':' + describe(range));
      return buildTokens();
    }
    provideInlayHints(document, range, token) {
      note('hints:' + this.name + ':' + describe(range));
      const typed = new vscode.InlayHint(new vscode.Position(0, 3), ': tipo', vscode.InlayHintKind.Type);
      typed.paddingLeft = true;
      const parts = new vscode.InlayHint(new vscode.Position(1, 2), [new vscode.InlayHintLabelPart('a'), new vscode.InlayHintLabelPart('b')]);
      parts.tag = 'x';
      return [typed, parts];
    }
    resolveInlayHint(hint, token) {
      note('resolveHint:' + this.name + ':' + hint.tag);
      hint.tooltip = 'dica';
      return hint;
    }
    provideCompletionItems(document, position, token, completionContext) {
      note('completion:' + this.name + ':' + at(document, position));
      const first = new vscode.CompletionItem('alfa', vscode.CompletionItemKind.Keyword);
      first.data = 'carga-alfa';
      return [first];
    }
    resolveCompletionItem(item, token) {
      note('resolveCompletion:' + this.name + ':' + item.label + ':' + item.data);
      item.detail = 'resolvido';
      return item;
    }
  }

  const registrations = {
    lens: vscode.languages.registerCodeLensProvider(selector, new Named('code')),
    references: vscode.languages.registerReferenceProvider(selector, new Named('ref')),
    highlight: vscode.languages.registerDocumentHighlightProvider(selector, new Named('hig')),
    rename: vscode.languages.registerRenameProvider(selector, new Named('ren')),
    signature: vscode.languages.registerSignatureHelpProvider(selector, new Named('sig'), '(', ','),
    signatureMetadata: vscode.languages.registerSignatureHelpProvider({ scheme: 'file', language: 'rust' }, { provideSignatureHelp() { return undefined; } }, {
      triggerCharacters: ['('],
      retriggerCharacters: [','],
    }),
    rangesFormat: vscode.languages.registerDocumentRangeFormattingEditProvider(selector, new Named('rngs')),
    onType: vscode.languages.registerOnTypeFormattingEditProvider(selector, new Named('typ'), '}', ';'),
    folding: vscode.languages.registerFoldingRangeProvider(selector, new Named('fol')),
    selection: vscode.languages.registerSelectionRangeProvider(selector, new Named('sel')),
    tokens: vscode.languages.registerDocumentSemanticTokensProvider(selector, new Named('tok'), legend),
    rangeTokens: vscode.languages.registerDocumentRangeSemanticTokensProvider(selector, new Named('rtk'), legend),
    hints: vscode.languages.registerInlayHintsProvider(selector, new Named('hin')),
    completion: vscode.languages.registerCompletionItemProvider(selector, new Named('comp'), '.'),
  };

  command('t.disposeRename', () => {
    registrations.rename.dispose();
    return 'ok';
  });
  command('t.builder', () => {
    // fora de ordem: o construtor ordena e codifica por diferença
    const builder = new vscode.SemanticTokensBuilder();
    builder.push(2, 1, 3, 0, 0);
    builder.push(0, 0, 2, 1, 1);
    const tokens = builder.build('rid');
    return JSON.stringify({ data: Array.from(tokens.data), resultId: tokens.resultId });
  });
  command('t.valueClasses', () => {
    const fold = new vscode.FoldingRange(1, 2, vscode.FoldingRangeKind.Comment);
    const lens = new vscode.CodeLens(new vscode.Range(0, 1, 2, 3));
    const tokenLegend = new vscode.SemanticTokensLegend(['a', 'b']);
    const help = new vscode.SignatureHelp();
    const signature = new vscode.SignatureInformation('f()');
    return JSON.stringify({
      fold: fold.start + '-' + fold.end + ':' + vscode.FoldingRangeKind[fold.kind],
      lens: describe(lens.range) + ':' + String(lens.command),
      legend: tokenLegend.tokenTypes.join(',') + '|' + tokenLegend.tokenModifiers.length,
      help: help.signatures.length + ':' + help.activeSignature + ':' + help.activeParameter,
      signature: signature.label + ':' + signature.parameters.length,
      kinds: vscode.DocumentHighlightKind[1] + ':' + vscode.SignatureHelpTriggerKind.TriggerCharacter + ':' + vscode.InlayHintKind[2],
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
fn every_new_registration_reaches_the_workbench_and_dispose_unregisters_it() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, observed| {
        let features = &observed.language_features;
        for kind in [
            "rpc_register_code_lens_support",
            "rpc_register_reference_support",
            "rpc_register_document_highlight_provider",
            "rpc_register_rename_support",
            "rpc_register_signature_help_provider",
            "rpc_register_range_formatting_support",
            "rpc_register_on_type_formatting_support",
            "rpc_register_folding_range_provider",
            "rpc_register_selection_range_provider",
            "rpc_register_document_semantic_tokens_provider",
            "rpc_register_document_range_semantic_tokens_provider",
            "rpc_register_inlay_hints_provider",
            "rpc_register_completions_provider",
        ] {
            assert!(!features.handles_of(kind).is_empty(), "o registro {kind} deveria ter chegado: {:?}", features.registered.borrow());
        }
        assert_eq!(features.handles_of("rpc_register_signature_help_provider").len(), 2, "os caracteres soltos e o objeto de metadados");

        let rename = features.handle_of("rpc_register_rename_support");
        assert!(features.unregistered.borrow().is_empty());
        assert_eq!(workbench.execute("t.disposeRename", Vec::new()), Dynamic::string("ok"));
        assert_eq!(*features.unregistered.borrow(), vec![rename], "o Disposable do registro desfaz só aquele registro");
    });
}

#[test]
fn code_lens_provider_resolves_the_lazy_lens_with_its_original_object() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, observed| {
        let model = create_document(&observed, "a.txt", DOCUMENT);
        let handle = observed.language_features.handle_of("rpc_register_code_lens_support");

        let pending = observed.features_proxy().rpc_provide_code_lenses(handle, &project_file("a.txt"), CancellationToken::None);
        workbench.pump();
        let list = settled_ok(&pending, "provideCodeLenses").expect("a lista de lentes");
        assert_eq!(list.lenses.len(), 2);
        assert!(list.lenses[0].command.is_some(), "a lente pronta já traz o comando");
        assert!(list.lenses[1].command.is_none(), "a lente preguiçosa espera o resolve");

        let lazy = list.lenses.into_iter().nth(1).expect("a segunda lente");
        let pending = observed.features_proxy().rpc_resolve_code_lens(handle, lazy, CancellationToken::None);
        workbench.pump();
        let resolved = settled_ok(&pending, "resolveCodeLens").expect("a lente resolvida");
        assert!(resolved.command.is_some(), "o resolveCodeLens recebeu o objeto original (com a propriedade tag) e devolveu o comando");
        assert_eq!(calls(workbench), r#"["lens:code:/proj/a.txt:boolean","resolveLens:code:tag-code:1,0-1,4"]"#);
        model.dispose();
    });
}

#[test]
fn reference_and_highlight_providers_convert_their_lists() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, observed| {
        let model = create_document(&observed, "a.txt", DOCUMENT);
        let references = observed.language_features.handle_of("rpc_register_reference_support");
        let highlight = observed.language_features.handle_of("rpc_register_document_highlight_provider");

        let pending = observed.features_proxy().rpc_provide_references(
            references,
            &project_file("a.txt"),
            EditorPosition::new(2, 3),
            ReferenceContext { include_declaration: true },
            CancellationToken::None,
        );
        workbench.pump();
        let locations = settled_ok(&pending, "provideReferences").expect("as referências");
        assert_eq!(locations.len(), 2);
        assert_eq!(locations[1].range, Some(EditorRange::new(3, 2, 3, 5)), "o intervalo vira base 1");

        let pending = observed.features_proxy().rpc_provide_document_highlights(highlight, &project_file("a.txt"), EditorPosition::new(2, 3), CancellationToken::None);
        workbench.pump();
        let highlights = settled_ok(&pending, "provideDocumentHighlights").expect("os destaques");
        assert_eq!(highlights.len(), 2);
        assert_eq!(highlights[0].range, EditorRange::new(1, 1, 1, 6));

        assert_eq!(calls(workbench), r#"["references:ref:/proj/a.txt@1,2:true","highlights:hig:/proj/a.txt@1,2"]"#);
        model.dispose();
    });
}

#[test]
fn rename_provider_converts_the_workspace_edit_and_resolves_the_prepared_location() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, observed| {
        let model = create_document(&observed, "a.txt", DOCUMENT);
        let rename = observed.language_features.handle_of("rpc_register_rename_support");

        let pending = observed.features_proxy().rpc_provide_rename_edits(rename, &project_file("a.txt"), EditorPosition::new(2, 3), "novo", CancellationToken::None);
        workbench.pump();
        assert!(settled_ok(&pending, "provideRenameEdits").is_some(), "a WorkspaceEdit criada pela extensão chega como edições de renomeação");

        let pending = observed.features_proxy().rpc_resolve_rename_location(rename, &project_file("a.txt"), EditorPosition::new(2, 3), CancellationToken::None);
        workbench.pump();
        assert!(settled_ok(&pending, "prepareRename").is_some(), "o prepareRename só roda porque o objeto do provedor o tem");

        assert_eq!(calls(workbench), r#"["rename:ren:/proj/a.txt@1,2:novo","prepare:ren:/proj/a.txt@1,2"]"#);
        model.dispose();
    });
}

#[test]
fn signature_help_provider_receives_the_context_and_its_help_converts() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, observed| {
        let model = create_document(&observed, "a.txt", DOCUMENT);
        let signature = observed.language_features.handle_of("rpc_register_signature_help_provider");

        let context = ISignatureHelpContextDto { trigger_kind: SignatureHelpTriggerKind::Invoke, trigger_character: None, is_retrigger: false, active_signature_help: None };
        let pending = observed.features_proxy().rpc_provide_signature_help(signature, &project_file("a.txt"), EditorPosition::new(2, 3), context, CancellationToken::None);
        workbench.pump();
        let help = settled_ok(&pending, "provideSignatureHelp").expect("a ajuda de assinatura");
        assert_eq!(help.signatures.len(), 1);
        assert_eq!(help.active_parameter, 1);
        assert_eq!(calls(workbench), r#"["signature:sig:/proj/a.txt@1,2:1:undefined:false:undefined"]"#);
        model.dispose();
    });
}

#[test]
fn formatting_providers_use_the_ranges_method_and_the_on_type_method() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, observed| {
        let model = create_document(&observed, "a.txt", DOCUMENT);
        let range_format = observed.language_features.handle_of("rpc_register_range_formatting_support");
        let on_type = observed.language_features.handle_of("rpc_register_on_type_formatting_support");

        let pending = observed.features_proxy().rpc_provide_document_ranges_formatting_edits(
            range_format,
            &project_file("a.txt"),
            vec![EditorRange::new(1, 1, 1, 3), EditorRange::new(2, 1, 2, 3)],
            formatting_options(),
            CancellationToken::None,
        );
        workbench.pump();
        let edits = settled_ok(&pending, "provideDocumentRangesFormattingEdits").expect("as edições");
        assert_eq!(edits.len(), 2);
        assert_eq!(edits[0].text, ">");

        let pending =
            observed.features_proxy().rpc_provide_on_type_formatting_edits(on_type, &project_file("a.txt"), EditorPosition::new(1, 3), "}", formatting_options(), CancellationToken::None);
        workbench.pump();
        let edits = settled_ok(&pending, "provideOnTypeFormattingEdits").expect("as edições");
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].text, "}");

        assert_eq!(calls(workbench), r#"["ranges:rngs:2:0,0-0,2|1,0-1,2:4","ontype:typ:/proj/a.txt@0,2:}:4"]"#);
        model.dispose();
    });
}

#[test]
fn folding_and_selection_range_providers_convert_their_results() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, observed| {
        let model = create_document(&observed, "a.txt", DOCUMENT);
        let folding = observed.language_features.handle_of("rpc_register_folding_range_provider");
        let selection = observed.language_features.handle_of("rpc_register_selection_range_provider");

        let pending = observed.features_proxy().rpc_provide_folding_ranges(folding, &project_file("a.txt"), FoldingContext {}, CancellationToken::None);
        workbench.pump();
        let ranges = settled_ok(&pending, "provideFoldingRanges").expect("os intervalos de dobra");
        assert_eq!(ranges.len(), 2);
        assert_eq!((ranges[0].start, ranges[0].end), (1, 3), "as linhas viram base 1");

        let pending = observed.features_proxy().rpc_provide_selection_ranges(selection, &project_file("a.txt"), vec![EditorPosition::new(2, 2)], CancellationToken::None);
        workbench.pump();
        let chains = settled_ok(&pending, "provideSelectionRanges");
        assert_eq!(chains.len(), 1);
        assert_eq!(chains[0].len(), 2, "o intervalo e o pai dele");
        assert_eq!(chains[0][0].range, EditorRange::new(2, 1, 2, 5));

        assert_eq!(calls(workbench), r#"["folding:fol:object","selection:sel:1,1"]"#);
        model.dispose();
    });
}

#[test]
fn semantic_tokens_providers_build_tokens_with_the_builder() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, observed| {
        let model = create_document(&observed, "a.txt", DOCUMENT);
        let tokens = observed.language_features.handle_of("rpc_register_document_semantic_tokens_provider");
        let range_tokens = observed.language_features.handle_of("rpc_register_document_range_semantic_tokens_provider");

        let pending = observed.features_proxy().rpc_provide_document_semantic_tokens(tokens, &project_file("a.txt"), 0, CancellationToken::None);
        workbench.pump();
        assert!(settled_ok(&pending, "provideDocumentSemanticTokens").is_some(), "o SemanticTokens que o builder com legenda construiu converte");

        let pending = observed.features_proxy().rpc_provide_document_range_semantic_tokens(range_tokens, &project_file("a.txt"), EditorRange::new(1, 1, 2, 1), CancellationToken::None);
        workbench.pump();
        assert!(settled_ok(&pending, "provideDocumentRangeSemanticTokens").is_some());

        assert_eq!(calls(workbench), r#"["tokens:tok:/proj/a.txt","rangeTokens:rtk:0,0-1,0"]"#);
        assert_eq!(
            workbench.execute("t.builder", Vec::new()),
            Dynamic::string(r#"{"data":[0,0,2,1,1,2,1,3,0,0],"resultId":"rid"}"#),
            "tokens fora de ordem saem ordenados e codificados por diferença"
        );
        model.dispose();
    });
}

#[test]
fn inlay_hints_provider_converts_both_label_forms_and_resolves_with_the_original() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, observed| {
        let model = create_document(&observed, "a.txt", DOCUMENT);
        let hints = observed.language_features.handle_of("rpc_register_inlay_hints_provider");

        let pending = observed.features_proxy().rpc_provide_inlay_hints(hints, &project_file("a.txt"), EditorRange::new(1, 1, 3, 1), CancellationToken::None);
        workbench.pump();
        let result = settled_ok(&pending, "provideInlayHints").expect("as dicas");
        assert_eq!(result.hints.len(), 2);
        match &result.hints[0].label {
            InlayHintLabelDto::Text(text) => assert_eq!(text, ": tipo"),
            _ => panic!("o rótulo em texto deveria ficar em texto"),
        }
        assert_eq!(result.hints[0].padding_left, Some(true));
        assert!(matches!(&result.hints[1].label, InlayHintLabelDto::Parts(parts) if parts.len() == 2), "o rótulo em partes");

        let id = result.hints[1].cache_id.expect("a dica guardada no cache");
        let pending = observed.features_proxy().rpc_resolve_inlay_hint(hints, id, CancellationToken::None);
        workbench.pump();
        assert!(settled_ok(&pending, "resolveInlayHint").is_some(), "o resolveInlayHint só roda porque o objeto do provedor o tem");

        assert_eq!(calls(workbench), r#"["hints:hin:0,0-2,0","resolveHint:hin:x"]"#);
        model.dispose();
    });
}

#[test]
fn completion_provider_resolves_the_item_with_its_original_object() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, observed| {
        let model = create_document(&observed, "a.txt", DOCUMENT);
        let completion = observed.language_features.handle_of("rpc_register_completions_provider");

        let context = CompletionContext { trigger_kind: CompletionTriggerKind::Invoke, trigger_character: None };
        let pending = observed.features_proxy().rpc_provide_completion_items(completion, &project_file("a.txt"), EditorPosition::new(2, 3), context, CancellationToken::None);
        workbench.pump();
        let result = settled_ok(&pending, "provideCompletionItems").expect("a lista de completar");
        let cache_id = result.cache_id.expect("o cache do provedor que sabe resolver");

        let pending = observed.features_proxy().rpc_resolve_completion_item(completion, (cache_id, 0), CancellationToken::None);
        workbench.pump();
        assert!(settled_ok(&pending, "resolveCompletionItem").is_some(), "o resolveCompletionItem só roda porque o objeto do provedor o tem");

        assert_eq!(calls(workbench), r#"["completion:comp:/proj/a.txt@1,2","resolveCompletion:comp:alfa:carga-alfa"]"#, "o resolve recebe a propriedade extra do item original");
        model.dispose();
    });
}

#[test]
fn value_classes_build_the_objects_of_the_original() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, _observed| {
        assert_eq!(
            workbench.execute("t.valueClasses", Vec::new()),
            Dynamic::string(concat!(
                r#"{"fold":"1-2:Comment","lens":"0,1-2,3:undefined","legend":"a,b|0","help":"0:0:0","signature":"f():0","#,
                r#""kinds":"Read:2:Parameter"}"#,
            ))
        );
    });
}
