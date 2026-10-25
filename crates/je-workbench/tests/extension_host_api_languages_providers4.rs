//! Teste de ponta a ponta dos provedores de `vscode.languages` da quarta leva (não é porte do oráculo): uma extensão de exemplo em
//! JavaScript, ativada pelo host local (`AbstractExtensionService` + `LocalExtensionHost` + `RPCProtocol`), registra uma expressão
//! avaliável, valores embutidos, edição vinculada e destaque em vários documentos, configura uma linguagem
//! (`setLanguageConfiguration`) e cria um item de estado de linguagem (`createLanguageStatusItem`), tudo com classes de usuário (o
//! `this` do provedor funciona).
//!
//! O lado do workbench é o do `support/extension_host_api_languages_support.rs` (o mesmo de `extension_host_api_languages_providers3.rs`):
//! os documentos de texto reais e um `MainThreadLanguageFeatures` que registra o handle de cada `$register*` e a configuração de cada
//! `$setLanguageConfiguration`. O teste faz o papel do `MainThreadLanguageFeatures` real: chama os `$provide*` pelo proxy
//! `ExtHostLanguageFeaturesShape` e confere o resultado convertido para o formato do editor. As suítes de unidade de
//! `ext_host_language_features.rs` cobrem os adaptadores; aqui entram a ponte (`vscode.languages.*`), as classes de valor e a leitura do
//! que o JavaScript devolve.
//!
//! - **Registro**: cada `register*Provider` chega ao cliente com o seu handle e o `Disposable` desfaz o registro (`$unregister`).
//! - **Expressão avaliável**: a posição do cursor e o `EvaluatableExpression` devolvido.
//! - **Valores embutidos**: o intervalo visível, o `InlineValueContext` e as três formas de `InlineValue`.
//! - **Edição vinculada**: os intervalos e o `wordPattern` (uma `RegExp`).
//! - **Destaque em vários documentos**: os outros documentos e o `MultiDocumentHighlight`.
//! - **Configuração de linguagem**: comentários, colchetes, `wordPattern`, regras de indentação e de nova linha, e o erro do padrão que
//!   casa a cadeia vazia.
//! - **Item de estado**: as propriedades, o id repetido e o `dispose`.
//! - **Classes**: os valores de construtor e os enums.

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
use je_editor::language_configuration::{IndentAction, LineComment};
use je_editor::position::Position as EditorPosition;
use je_editor::range::Range as EditorRange;
use je_exthost::in_process::IMainThreadBinder;
use je_workbench::api::common::ext_host_protocol::{IInlineValueContextDto, InlineValueDto};
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

  // uma classe de usuário: os métodos moram no protótipo e usam `this`
  class Named {
    constructor(name) {
      this.name = name;
    }
    provideEvaluatableExpression(document, position, token) {
      note('eval:' + this.name + ':' + document.uri.path + '@' + pos(position) + ':' + typeof token.isCancellationRequested);
      return new vscode.EvaluatableExpression(new vscode.Range(0, 0, 0, 5), 'expr-' + this.name);
    }
    provideInlineValues(document, viewPort, context, token) {
      note('inline:' + this.name + ':' + document.uri.path + ':' + describe(viewPort) + ':' + context.frameId + ':' + describe(context.stoppedLocation));
      return [
        new vscode.InlineValueText(new vscode.Range(0, 0, 0, 5), 'texto'),
        new vscode.InlineValueVariableLookup(new vscode.Range(1, 0, 1, 4), 'nome', false),
        new vscode.InlineValueEvaluatableExpression(new vscode.Range(2, 0, 2, 3), 'a+b'),
      ];
    }
    provideLinkedEditingRanges(document, position, token) {
      note('linked:' + this.name + ':' + document.uri.path + '@' + pos(position));
      return new vscode.LinkedEditingRanges([new vscode.Range(0, 0, 0, 4), new vscode.Range(2, 0, 2, 4)], /[a-z]+/i);
    }
    provideMultiDocumentHighlights(document, position, otherDocuments, token) {
      note('multi:' + this.name + ':' + document.uri.path + '@' + pos(position) + ':' + otherDocuments.map((other) => other.uri.path).join(','));
      return [new vscode.MultiDocumentHighlight(document.uri, [new vscode.DocumentHighlight(new vscode.Range(0, 0, 0, 4), vscode.DocumentHighlightKind.Write)])];
    }
  }

  const registrations = {
    evaluatable: vscode.languages.registerEvaluatableExpressionProvider(selector, new Named('eva')),
    inline: vscode.languages.registerInlineValuesProvider(selector, new Named('inl')),
    linked: vscode.languages.registerLinkedEditingRangeProvider(selector, new Named('lin')),
    multi: vscode.languages.registerMultiDocumentHighlightProvider(selector, new Named('mul')),
  };

  command('t.disposeEvaluatable', () => {
    registrations.evaluatable.dispose();
    return 'ok';
  });
  command('t.configure', () => {
    const disposable = vscode.languages.setLanguageConfiguration('typescript', {
      comments: { lineComment: '//', blockComment: ['/*', '*/'] },
      brackets: [['{', '}'], ['(', ')']],
      wordPattern: /[a-z]+/g,
      indentationRules: { increaseIndentPattern: /\{$/, decreaseIndentPattern: /^\s*\}/ },
      onEnterRules: [
        { beforeText: /^\s*\/\*\*/, afterText: /^\s*\*\//, action: { indentAction: vscode.IndentAction.IndentOutdent, appendText: ' * ' } },
        { beforeText: /^\s*\/\/\s/, action: { indentAction: vscode.IndentAction.None, removeText: 1 } },
      ],
    });
    globalThis.__configuration = disposable;
    return typeof disposable.dispose;
  });
  command('t.disposeConfiguration', () => {
    globalThis.__configuration.dispose();
    return 'ok';
  });
  command('t.badConfiguration', () => {
    try {
      vscode.languages.setLanguageConfiguration('typescript', { wordPattern: /(?:)/ });
      return 'sem erro';
    } catch (error) {
      return error.message;
    }
  });
  command('t.statusItem', () => {
    const item = vscode.languages.createLanguageStatusItem('estado', selector);
    const initial = [item.id, item.name, item.text, item.detail, item.severity, item.busy].join('|');
    item.name = 'Nome';
    item.text = 'pronto';
    item.detail = 'tudo certo';
    item.severity = vscode.LanguageStatusSeverity.Warning;
    item.busy = true;
    item.command = { title: 'abrir', command: 't.disposeEvaluatable' };
    item.accessibilityInformation = { label: 'acessível', role: 'button' };
    const updated = [item.name, item.text, item.detail, item.severity, item.busy, item.command.title, item.accessibilityInformation.label, item.selector.scheme].join('|');
    let duplicate;
    try {
      vscode.languages.createLanguageStatusItem('estado', selector);
      duplicate = 'sem erro';
    } catch (error) {
      duplicate = error.message;
    }
    item.dispose();
    const again = vscode.languages.createLanguageStatusItem('estado', selector);
    again.dispose();
    return JSON.stringify({ initial, updated, duplicate, recreated: again.id });
  });
  command('t.valueClasses', () => {
    const range = new vscode.Range(0, 1, 0, 4);
    const expression = new vscode.EvaluatableExpression(range, 'x');
    const bare = new vscode.EvaluatableExpression(range);
    const text = new vscode.InlineValueText(range, 'v');
    const lookup = new vscode.InlineValueVariableLookup(range, 'n');
    const caseless = new vscode.InlineValueVariableLookup(range, 'n', false);
    const evaluatable = new vscode.InlineValueEvaluatableExpression(range, 'a+b');
    const inlineContext = new vscode.InlineValueContext(3, range);
    const linked = new vscode.LinkedEditingRanges([range], /x+/);
    const plainLinked = new vscode.LinkedEditingRanges([range]);
    const multi = new vscode.MultiDocumentHighlight(vscode.Uri.file('/proj/a.txt'), [new vscode.DocumentHighlight(range)]);
    return JSON.stringify({
      expression: describe(expression.range) + ':' + expression.expression,
      bare: String(bare.expression),
      text: describe(text.range) + ':' + text.text,
      lookup: lookup.variableName + ':' + lookup.caseSensitiveLookup,
      caseless: String(caseless.caseSensitiveLookup),
      evaluatable: evaluatable.expression,
      inlineContext: inlineContext.frameId + ':' + describe(inlineContext.stoppedLocation),
      linked: linked.ranges.length + ':' + String(linked.wordPattern),
      plainLinked: String(plainLinked.wordPattern),
      multi: multi.uri.path + ':' + multi.highlights.length,
      enums: [vscode.LanguageStatusSeverity.Error, vscode.LanguageStatusSeverity[1], vscode.IndentAction.IndentOutdent, vscode.IndentAction[3]].join(','),
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
            "rpc_register_evaluatable_expression_provider",
            "rpc_register_inline_values_provider",
            "rpc_register_linked_editing_range_provider",
            "rpc_register_multi_document_highlight_provider",
        ] {
            assert_eq!(features.handles_of(kind).len(), 1, "o registro {kind} deveria ter chegado: {:?}", features.registered.borrow());
        }

        let evaluatable = features.handle_of("rpc_register_evaluatable_expression_provider");
        assert!(features.unregistered.borrow().is_empty());
        assert_eq!(workbench.execute("t.disposeEvaluatable", Vec::new()), Dynamic::string("ok"));
        assert_eq!(*features.unregistered.borrow(), vec![evaluatable], "o Disposable do registro desfaz só aquele registro");
    });
}

#[test]
fn evaluatable_expression_provider_receives_the_position_and_returns_the_expression() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, observed| {
        let model = create_document(&observed, "a.txt", DOCUMENT);
        let handle = observed.language_features.handle_of("rpc_register_evaluatable_expression_provider");

        let pending = observed.features_proxy().rpc_provide_evaluatable_expression(handle, &project_file("a.txt"), EditorPosition::new(2, 3), CancellationToken::None);
        workbench.pump();
        let expression = settled_ok(&pending, "provideEvaluatableExpression").expect("a expressão avaliável");
        assert_eq!(expression.range, EditorRange::new(1, 1, 1, 6), "o intervalo vira base 1");
        assert_eq!(expression.expression.as_deref(), Some("expr-eva"));

        assert_eq!(calls(workbench), r#"["eval:eva:/proj/a.txt@1,2:boolean"]"#);
        model.dispose();
    });
}

#[test]
fn inline_values_provider_receives_the_view_port_and_the_context_and_returns_the_three_forms() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, observed| {
        let model = create_document(&observed, "a.txt", DOCUMENT);
        let handle = observed.language_features.handle_of("rpc_register_inline_values_provider");

        let context = IInlineValueContextDto { frame_id: 7.0, stopped_location: EditorRange::new(2, 1, 2, 5) };
        let pending = observed.features_proxy().rpc_provide_inline_values(handle, &project_file("a.txt"), EditorRange::new(1, 1, 3, 1), context, CancellationToken::None);
        workbench.pump();
        let values = settled_ok(&pending, "provideInlineValues").expect("os valores embutidos");
        assert_eq!(
            values,
            vec![
                InlineValueDto::Text { range: EditorRange::new(1, 1, 1, 6), text: "texto".to_owned() },
                InlineValueDto::Variable { range: EditorRange::new(2, 1, 2, 5), variable_name: Some("nome".to_owned()), case_sensitive_lookup: false },
                InlineValueDto::Expression { range: EditorRange::new(3, 1, 3, 4), expression: Some("a+b".to_owned()) },
            ]
        );

        assert_eq!(calls(workbench), r#"["inline:inl:/proj/a.txt:0,0-2,0:7:1,0-1,4"]"#);
        model.dispose();
    });
}

#[test]
fn linked_editing_range_provider_returns_the_ranges_and_the_word_pattern() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, observed| {
        let model = create_document(&observed, "a.txt", DOCUMENT);
        let handle = observed.language_features.handle_of("rpc_register_linked_editing_range_provider");

        let pending = observed.features_proxy().rpc_provide_linked_editing_ranges(handle, &project_file("a.txt"), EditorPosition::new(1, 2), CancellationToken::None);
        workbench.pump();
        let linked = settled_ok(&pending, "provideLinkedEditingRanges").expect("os intervalos vinculados");
        assert_eq!(linked.ranges, vec![EditorRange::new(1, 1, 1, 5), EditorRange::new(3, 1, 3, 5)]);
        let word_pattern = linked.word_pattern.expect("o wordPattern (uma RegExp lida por referência)");
        assert_eq!(word_pattern.pattern, "[a-z]+");
        assert!(word_pattern.flags.as_deref().unwrap_or("").contains('i'), "as flags da RegExp: {:?}", word_pattern.flags);

        assert_eq!(calls(workbench), r#"["linked:lin:/proj/a.txt@0,1"]"#);
        model.dispose();
    });
}

#[test]
fn multi_document_highlight_provider_receives_the_other_documents() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, observed| {
        let model = create_document(&observed, "a.txt", DOCUMENT);
        let other = create_document(&observed, "b.txt", DOCUMENT);
        let handle = observed.language_features.handle_of("rpc_register_multi_document_highlight_provider");

        let pending = observed.features_proxy().rpc_provide_multi_document_highlights(
            handle,
            &project_file("a.txt"),
            EditorPosition::new(1, 2),
            vec![project_file("b.txt")],
            CancellationToken::None,
        );
        workbench.pump();
        let result = settled_ok(&pending, "provideMultiDocumentHighlights").expect("os destaques");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].uri.path(), "/proj/a.txt");
        assert_eq!(result[0].highlights.len(), 1);
        assert_eq!(result[0].highlights[0].range, EditorRange::new(1, 1, 1, 5));

        assert_eq!(calls(workbench), r#"["multi:mul:/proj/a.txt@0,1:/proj/b.txt"]"#);
        model.dispose();
        other.dispose();
    });
}

#[test]
fn set_language_configuration_sends_the_serialized_rules_and_dispose_unregisters_it() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, observed| {
        assert_eq!(workbench.execute("t.configure", Vec::new()), Dynamic::string("function"));
        let configurations = observed.language_features.configurations.borrow();
        assert_eq!(configurations.len(), 1);
        let (handle, language, configuration) = &configurations[0];
        assert_eq!(language, "typescript");

        let comments = configuration.comments.as_ref().expect("os comentários");
        assert_eq!(comments.line_comment, Some(LineComment::Token("//".to_owned())));
        assert_eq!(comments.block_comment, Some(("/*".to_owned(), "*/".to_owned())));
        assert_eq!(configuration.brackets, Some(vec![("{".to_owned(), "}".to_owned()), ("(".to_owned(), ")".to_owned())]));

        let word_pattern = configuration.word_pattern.as_ref().expect("o wordPattern");
        assert_eq!(word_pattern.pattern, "[a-z]+");
        assert!(word_pattern.flags.as_deref().unwrap_or("").contains('g'), "as flags da RegExp: {:?}", word_pattern.flags);

        let indentation = configuration.indentation_rules.as_ref().expect("as regras de indentação");
        assert_eq!(indentation.increase_indent_pattern.pattern, r"\{$");
        assert_eq!(indentation.decrease_indent_pattern.pattern, r"^\s*\}");
        assert!(indentation.indent_next_line_pattern.is_none());

        let rules = configuration.on_enter_rules.as_ref().expect("as regras de nova linha");
        assert_eq!(rules.len(), 2);
        assert_eq!(rules[0].before_text.pattern, r"^\s*\/\*\*");
        assert_eq!(rules[0].after_text.as_ref().map(|pattern| pattern.pattern.as_str()), Some(r"^\s*\*\/"));
        assert_eq!(rules[0].action.indent_action, IndentAction::IndentOutdent);
        assert_eq!(rules[0].action.append_text.as_deref(), Some(" * "));
        assert!(rules[1].after_text.is_none());
        assert_eq!(rules[1].action.indent_action, IndentAction::None);
        assert_eq!(rules[1].action.remove_text, Some(1));

        let handle = *handle;
        drop(configurations);
        assert!(observed.language_features.unregistered.borrow().is_empty());
        assert_eq!(workbench.execute("t.disposeConfiguration", Vec::new()), Dynamic::string("ok"));
        assert_eq!(*observed.language_features.unregistered.borrow(), vec![handle]);
    });
}

#[test]
fn set_language_configuration_throws_for_a_word_pattern_that_matches_the_empty_string() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, observed| {
        let message = workbench.execute("t.badConfiguration", Vec::new());
        let Dynamic::String(message) = message else { panic!("a mensagem do erro: {message:?}") };
        assert!(message.to_string_lossy().starts_with("Invalid language configuration: wordPattern '/"), "{}", message.to_string_lossy());
        assert!(observed.language_features.configurations.borrow().is_empty(), "nada chega ao cliente quando a configuração é inválida");
    });
}

#[test]
fn language_status_item_exposes_the_writable_properties_and_rejects_a_repeated_id() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, _observed| {
        assert_eq!(
            workbench.execute("t.statusItem", Vec::new()),
            Dynamic::string(concat!(
                r#"{"initial":"estado|exthost|||0|false","#,
                r#""updated":"Nome|pronto|tudo certo|1|true|abrir|acessível|file","#,
                r#""duplicate":"LanguageStatusItem with id 'estado' ALREADY exists","recreated":"estado"}"#,
            ))
        );
    });
}

#[test]
fn value_classes_build_the_objects_of_the_original() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, _observed| {
        assert_eq!(
            workbench.execute("t.valueClasses", Vec::new()),
            Dynamic::string(concat!(
                r#"{"expression":"0,1-0,4:x","bare":"undefined","text":"0,1-0,4:v","lookup":"n:true","caseless":"false","evaluatable":"a+b","#,
                r#""inlineContext":"3:0,1-0,4","linked":"1:/x+/","plainLinked":"undefined","multi":"/proj/a.txt:1","#,
                r#""enums":"2,Warning,2,Outdent"}"#,
            ))
        );
    });
}
