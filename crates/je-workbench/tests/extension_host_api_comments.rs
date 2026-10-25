//! Teste de ponta a ponta de `vscode.comments.createCommentController` (não é porte do oráculo): uma extensão de exemplo em JavaScript,
//! ativada pelo host local (`AbstractExtensionService` + `LocalExtensionHost` + `RPCProtocol`), cria um controlador de comentários com
//! opções, provedor de ranges comentáveis e tratador de reações, cria threads de comentário (texto e markdown, estado, rótulo, valor de
//! contexto, resposta, expansão) e descarta thread e controlador; o efeito aparece no cliente `MainThreadComments` do workbench.
//!
//! O lado do ExtHost é o `ExtHostComments` REAL (portado de `extHostComments.ts`), ligado pelo host em processo. Não existe ainda um
//! `MainThreadComments` em `crates/je-workbench/src/api/browser/` (ele depende da camada de comentários da interface), então o lado do
//! workbench é um dublê do `MainThreadCommentsShape` ([`Panel`]) que anota cada chamada (`$registerCommentController`,
//! `$updateCommentControllerFeatures`, `$createCommentThread`, `$updateCommentThread`, `$deleteCommentThread`, `$updateCommentingRanges`,
//! `$unregisterCommentController`) e faz o papel da thread principal quando o teste manda: pede ao ExtHost o que a visão real pediria
//! (`$provideCommentingRanges`, `$toggleReaction`) pelo proxy `ExtHostCommentsShape` que o contexto em processo entrega. Os documentos de
//! texto vêm do mesmo suporte dos testes de `languages` (o `commentingRangeProvider` recebe um `TextDocument` do host).

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

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use harness::{run_extension, settled_ok, Workbench};
use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::lifecycle::IDisposable;
use je_base::marshalling_ids::MarshalledId;
use je_base::uri::URI;
use je_editor::languages::StringOrMarkdown;
use je_editor::range::Range as EditorRange;
use je_exthost::in_process::IMainThreadBinder;
use je_extensions::manifest::ExtensionIdentifier;
use je_workbench::common::comments::MarshalledCommentThread;
use je_workbench::api::common::ext_host_protocol::{
    CommentCanReply, CommentChanges, CommentProviderFeatures, CommentReaction, CommentThreadChanges, CommentThreadRevealOptions, CommentingRangeResourceHint, ExtHostCommentsShape,
    ExtHostContext, IMainContext, MainContext, MainThreadCommentsShape,
};
use languages_support::{create_document, languages_setup, project_file, Observed};

/// O JavaScript da extensão de exemplo.
const EXTENSION_SOURCE: &str = r#"
const vscode = require('vscode');
exports.activate = (context) => {
  globalThis.__activated = true;
  globalThis.__events = [];
  const log = (text) => globalThis.__events.push(text);
  const command = (id, handler) => context.subscriptions.push(vscode.commands.registerCommand(id, handler));
  const bodyOf = (comment) => (typeof comment.body === 'string' ? comment.body : comment.body.value);
  const describeRange = (range) => range.start.line + ',' + range.start.character + '-' + range.end.line + ',' + range.end.character;

  command('t.commentsCreate', () => {
    const controller = vscode.comments.createCommentController('je-review', 'Je Review');
    globalThis.__controller = controller;
    controller.options = { prompt: 'Comente', placeHolder: 'Escreva aqui' };
    controller.commentingRangeProvider = {
      provideCommentingRanges: (document, token) => {
        log('ranges:' + document.uri.path + ':' + typeof token.isCancellationRequested);
        return [new vscode.Range(0, 0, 1, 0)];
      },
    };
    controller.reactionHandler = async (comment, reaction) => {
      log('reaction:' + bodyOf(comment) + ':' + reaction.label + ':' + reaction.count + ':' + reaction.authorHasReacted);
    };
    return JSON.stringify({
      id: controller.id,
      label: controller.label,
      prompt: controller.options.prompt,
      placeHolder: controller.options.placeHolder,
      provider: typeof controller.commentingRangeProvider,
      handler: typeof controller.reactionHandler,
    });
  });

  command('t.commentsInfoProvider', () => {
    globalThis.__controller.commentingRangeProvider = {
      provideCommentingRanges: async () => ({ ranges: [new vscode.Range(2, 0, 2, 3)], enableFileComments: true }),
    };
    return 'ok';
  });

  command('t.commentsThread', () => {
    const controller = globalThis.__controller;
    const first = { body: 'primeiro', mode: vscode.CommentMode.Preview, author: { name: 'Ana' }, contextValue: 'inicial' };
    const thread = controller.createCommentThread(vscode.Uri.file('/proj/a.txt'), new vscode.Range(1, 0, 1, 4), [first]);
    globalThis.__thread = thread;
    thread.label = 'Revisão';
    thread.contextValue = 'aberta';
    thread.collapsibleState = vscode.CommentThreadCollapsibleState.Expanded;
    thread.canReply = false;
    thread.state = vscode.CommentThreadState.Resolved;
    thread.comments = [first, { body: new vscode.MarkdownString('**dois**'), mode: vscode.CommentMode.Editing, author: { name: 'Bia' } }];
    return JSON.stringify({
      uri: thread.uri.path,
      range: describeRange(thread.range),
      label: thread.label,
      contextValue: thread.contextValue,
      collapsibleState: thread.collapsibleState,
      canReply: thread.canReply,
      state: thread.state,
      comments: thread.comments.map(bodyOf),
      sameFirst: thread.comments[0] === first,
    });
  });

  command('t.commentsSimpleThread', () => {
    const thread = globalThis.__controller.createCommentThread(vscode.Uri.file('/proj/a.txt'), new vscode.Range(0, 0, 0, 5), [
      { body: 'primeiro', mode: vscode.CommentMode.Preview, author: { name: 'Ana' } },
    ]);
    globalThis.__thread = thread;
    return 'ok';
  });

  command('t.commentsKeptComment', () => {
    const comment = { body: 'primeiro', mode: vscode.CommentMode.Preview, author: { name: 'Ana' }, extra: 'mantido' };
    globalThis.__comment = comment;
    globalThis.__thread = globalThis.__controller.createCommentThread(vscode.Uri.file('/proj/a.txt'), new vscode.Range(0, 0, 0, 5), [comment]);
    return 'ok';
  });
  command('t.commentsOnThread', (thread) => JSON.stringify({ same: thread === globalThis.__thread, body: thread.comments.map(bodyOf) }));
  command('t.commentsOnController', (controller) => JSON.stringify({ same: controller === globalThis.__controller, id: controller.id }));
  command('t.commentsOnReply', (reply) => JSON.stringify({ same: reply.thread === globalThis.__thread, text: reply.text }));
  command('t.commentsOnComment', (comment) => JSON.stringify({ extra: comment.extra, body: bodyOf(comment), same: comment === globalThis.__comment }));

  command('t.commentsDispose', () => {
    const controller = globalThis.__controller;
    globalThis.__thread.dispose();
    controller.dispose();
    let afterwards;
    try { afterwards = controller.id; } catch (error) { afterwards = error.message; }
    return afterwards;
  });
};
"#;

/// O que o `$createCommentThread` carregou.
struct CreatedThread {
    controller_handle: u32,
    thread_handle: u32,
    thread_id: String,
    uri: String,
    range: Option<EditorRange>,
    bodies: Vec<String>,
    unique_ids: Vec<u32>,
}

/// O texto do corpo de um comentário que chegou à thread principal.
fn describe_body(body: &StringOrMarkdown) -> String {
    match body {
        StringOrMarkdown::String(text) => text.clone(),
        StringOrMarkdown::Markdown(markdown) => format!("md:{}", markdown.value.to_string_lossy()),
    }
}

/// Um resumo (em texto) de um `CommentProviderFeatures`: só os campos que o `$updateCommentControllerFeatures` carrega.
fn describe_features(features: &CommentProviderFeatures) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(options) = &features.options {
        parts.push(format!("options={}/{}", options.prompt.clone().unwrap_or_default(), options.place_holder.clone().unwrap_or_default()));
    }
    if let Some(has_handler) = features.reaction_handler {
        parts.push(format!("reactionHandler={has_handler}"));
    }
    parts.join(",")
}

/// Um resumo (em texto) de um `CommentThreadChanges`: só as chaves presentes, na ordem do protocolo.
fn describe_changes(changes: &CommentThreadChanges) -> String {
    let mut parts: Vec<String> = Vec::new();
    if changes.range.is_some() {
        parts.push("range".to_owned());
    }
    if let Some(label) = &changes.label {
        parts.push(format!("label={}", label.clone().unwrap_or_default()));
    }
    if let Some(context_value) = &changes.context_value {
        parts.push(format!("contextValue={}", context_value.clone().unwrap_or_default()));
    }
    if let Some(comments) = &changes.comments {
        parts.push(format!("comments={}", comments.iter().map(|comment| describe_body(&comment.body)).collect::<Vec<_>>().join("|")));
    }
    if let Some(state) = changes.collapse_state {
        parts.push(format!("collapseState={}", state.value()));
    }
    if let Some(can_reply) = &changes.can_reply {
        parts.push(format!(
            "canReply={}",
            match can_reply {
                CommentCanReply::Bool(flag) => flag.to_string(),
                CommentCanReply::Author(author) => author.name.clone(),
            }
        ));
    }
    if let Some(state) = changes.state {
        parts.push(format!("state={}", state.value()));
    }
    parts.join(",")
}

/// O papel da thread principal: o `MainThreadCommentsShape` que anota as chamadas.
struct Panel {
    ext_host: RefCell<Option<Rc<dyn ExtHostCommentsShape>>>,
    log: RefCell<Vec<String>>,
    /// Os `handle` dos controladores registrados.
    controllers: RefCell<Vec<u32>>,
    created: RefCell<Vec<CreatedThread>>,
}

impl Panel {
    fn new() -> Rc<Self> {
        Rc::new(Self { ext_host: RefCell::new(None), log: RefCell::new(Vec::new()), controllers: RefCell::new(Vec::new()), created: RefCell::new(Vec::new()) })
    }

    fn ext_host(&self) -> Rc<dyn ExtHostCommentsShape> {
        self.ext_host.borrow().clone().expect("o host já subiu e ligou o dublê")
    }

    fn note(&self, line: String) {
        self.log.borrow_mut().push(line);
    }

    fn lines(&self) -> Vec<String> {
        self.log.borrow().clone()
    }

    fn controller_handle(&self) -> u32 {
        *self.controllers.borrow().first().expect("a extensão criou um controlador")
    }

    fn last_thread(&self) -> (u32, u32, Vec<u32>) {
        let created = self.created.borrow();
        let thread = created.last().expect("a extensão criou uma thread");
        (thread.controller_handle, thread.thread_handle, thread.unique_ids.clone())
    }
}

impl IDisposable for Panel {
    fn dispose(&self) {}
}

impl MainThreadCommentsShape for Panel {
    fn rpc_register_comment_controller(&self, handle: u32, id: &str, label: &str, extension_id: &str) {
        self.controllers.borrow_mut().push(handle);
        self.note(format!("register:{id}:{label}:{extension_id}"));
    }

    fn rpc_unregister_comment_controller(&self, _handle: u32) {
        self.note("unregister".to_owned());
    }

    fn rpc_update_comment_controller_features(&self, _handle: u32, features: CommentProviderFeatures) {
        self.note(format!("features:{}", describe_features(&features)));
    }

    fn rpc_create_comment_thread(
        &self,
        handle: u32,
        comment_thread_handle: u32,
        thread_id: &str,
        resource: &URI,
        range: Option<EditorRange>,
        comments: Vec<CommentChanges>,
        _extension_id: &ExtensionIdentifier,
        _is_template: bool,
        _editor_id: Option<&str>,
    ) {
        let bodies: Vec<String> = comments.iter().map(|comment| describe_body(&comment.body)).collect();
        self.note(format!("createThread:{thread_id}:{}:{}", resource.to_string(false).to_string_lossy(), bodies.join("|")));
        self.created.borrow_mut().push(CreatedThread {
            controller_handle: handle,
            thread_handle: comment_thread_handle,
            thread_id: thread_id.to_owned(),
            uri: resource.to_string(false).to_string_lossy(),
            range,
            bodies,
            unique_ids: comments.iter().map(|comment| comment.unique_id_in_thread).collect(),
        });
    }

    fn rpc_update_comment_thread(&self, _handle: u32, _comment_thread_handle: u32, thread_id: &str, _resource: &URI, changes: CommentThreadChanges) {
        self.note(format!("updateThread:{thread_id}:{}", describe_changes(&changes)));
    }

    fn rpc_delete_comment_thread(&self, _handle: u32, _comment_thread_handle: u32) {
        self.note("deleteThread".to_owned());
    }

    fn rpc_update_commenting_ranges(&self, _handle: u32, resource_hints: Option<CommentingRangeResourceHint>) {
        self.note(format!("commentingRanges:{}", resource_hints.map_or_else(|| "none".to_owned(), |hints| hints.schemes.join(","))));
    }

    fn rpc_reveal_comment_thread(&self, _handle: u32, _comment_thread_handle: u32, _comment_unique_id_in_thread: u32, _options: CommentThreadRevealOptions) -> je_base::promise::Promise<()> {
        panic!("o teste não revela threads")
    }

    fn rpc_hide_comment_thread(&self, _handle: u32, _comment_thread_handle: u32) {
        panic!("o teste não esconde threads")
    }
}

/// Liga o dublê da thread principal ao host, junto dos clientes de documentos e linguagens do suporte de `languages` (o
/// `commentingRangeProvider` recebe um `TextDocument` real).
struct Binder {
    documents: Rc<dyn IMainThreadBinder>,
    panel: Rc<Panel>,
}

impl IMainThreadBinder for Binder {
    fn bind(&self, context: Rc<dyn IMainContext>) -> Vec<Rc<dyn IDisposable>> {
        let disposables = self.documents.bind(context.clone());
        *self.panel.ext_host.borrow_mut() = Some(context.get_proxy(&ExtHostContext::EXT_HOST_COMMENTS));
        let panel: Rc<dyn MainThreadCommentsShape> = self.panel.clone();
        context.set(&MainContext::MAIN_THREAD_COMMENTS, panel);
        disposables
    }
}

fn setup(_workbench: &Workbench) -> (Rc<dyn IMainThreadBinder>, (Observed, Rc<Panel>)) {
    let (documents, observed) = languages_setup();
    let panel = Panel::new();
    (Rc::new(Binder { documents, panel: panel.clone() }), (observed, panel))
}

fn run(workbench: &Workbench, command: &str) -> Dynamic {
    workbench.execute(command, Vec::new())
}

fn events(workbench: &Workbench) -> String {
    workbench.probe("JSON.stringify(globalThis.__events)")
}

/// Deixa vencer o `debounce` de 100 ms de `eventuallyUpdateCommentThread` e esvazia o laço.
fn settle(workbench: &Workbench) {
    std::thread::sleep(Duration::from_millis(200));
    workbench.pump();
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|item| (*item).to_owned()).collect()
}

#[test]
fn create_comment_controller_registers_it_and_forwards_options_ranges_and_reaction_handler() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, (_observed, panel)| {
        assert_eq!(
            run(workbench, "t.commentsCreate"),
            Dynamic::string(r#"{"id":"je-review","label":"Je Review","prompt":"Comente","placeHolder":"Escreva aqui","provider":"object","handler":"function"}"#)
        );
        settle(workbench);

        assert_eq!(
            panel.lines(),
            strings(&["register:je-review:Je Review:je.exthost", "features:options=Comente/Escreva aqui", "commentingRanges:none", "features:reactionHandler=true"])
        );
    });
}

#[test]
fn create_comment_thread_forwards_the_thread_and_one_coalesced_update() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, (_observed, panel)| {
        run(workbench, "t.commentsCreate");
        assert_eq!(
            run(workbench, "t.commentsThread"),
            Dynamic::string(
                r#"{"uri":"/proj/a.txt","range":"1,0-1,4","label":"Revisão","contextValue":"aberta","collapsibleState":1,"canReply":false,"state":1,"comments":["primeiro","**dois**"],"sameFirst":true}"#
            )
        );
        settle(workbench);

        let created = panel.created.borrow();
        assert_eq!(created.len(), 1);
        assert_eq!(created[0].thread_id, "je-review.0");
        assert_eq!(created[0].uri, "file:///proj/a.txt");
        assert_eq!(created[0].range, Some(EditorRange::new(2, 1, 2, 5)), "o intervalo vira base 1");
        assert_eq!(created[0].bodies, strings(&["primeiro"]));

        let lines = panel.lines();
        assert_eq!(lines[4], "createThread:je-review.0:file:///proj/a.txt:primeiro");
        assert_eq!(
            lines[5..].to_vec(),
            strings(&["updateThread:je-review.0:label=Revisão,contextValue=aberta,comments=primeiro|md:**dois**,collapseState=1,canReply=false,state=1"]),
            "as atribuições da mesma rodada viram um só $updateCommentThread"
        );
    });
}

#[test]
fn commenting_range_provider_answers_the_main_thread_with_ranges_and_file_comments() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, (observed, panel)| {
        run(workbench, "t.commentsCreate");
        let model = create_document(&observed, "a.txt", "linha um\nlinha dois\nterceira");
        let handle = panel.controller_handle();

        let plain = panel.ext_host().rpc_provide_commenting_ranges(handle, &project_file("a.txt"), CancellationToken::None);
        workbench.pump();
        let plain = settled_ok(&plain, "provideCommentingRanges").expect("o provedor devolveu ranges");
        assert_eq!(plain.ranges, vec![EditorRange::new(1, 1, 2, 1)]);
        assert!(!plain.file_comments);
        assert_eq!(events(workbench), r#"["ranges:/proj/a.txt:boolean"]"#, "o documento e o token chegam ao provedor");

        run(workbench, "t.commentsInfoProvider");
        let info = panel.ext_host().rpc_provide_commenting_ranges(handle, &project_file("a.txt"), CancellationToken::None);
        workbench.pump();
        let info = settled_ok(&info, "provideCommentingRanges").expect("o provedor assíncrono devolveu ranges");
        assert_eq!(info.ranges, vec![EditorRange::new(3, 1, 3, 4)]);
        assert!(info.file_comments, "enableFileComments chega à thread principal");
        model.dispose();
    });
}

#[test]
fn toggle_reaction_reaches_the_reaction_handler_with_the_comment() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, (_observed, panel)| {
        run(workbench, "t.commentsCreate");
        run(workbench, "t.commentsSimpleThread");
        settle(workbench);
        let (controller, thread, unique_ids) = panel.last_thread();
        assert_eq!(unique_ids, vec![1], "o primeiro comentário da thread tem o id 1");

        let comment = CommentChanges {
            unique_id_in_thread: unique_ids[0],
            body: StringOrMarkdown::String("primeiro".to_owned()),
            user_name: "Ana".to_owned(),
            user_icon_path: None,
            context_value: None,
            comment_reactions: None,
            label: None,
            mode: None,
            state: None,
            timestamp: None,
        };
        let reaction = CommentReaction { label: Some("like".to_owned()), count: Some(2.0), has_reacted: Some(true), ..CommentReaction::default() };
        let toggled = panel.ext_host().rpc_toggle_reaction(controller, thread, &project_file("a.txt"), comment, reaction);
        workbench.pump();
        settled_ok(&toggled, "toggleReaction");
        assert_eq!(events(workbench), r#"["reaction:primeiro:like:2:true"]"#);
    });
}

#[test]
fn command_arguments_are_the_same_objects_the_extension_created() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, (_observed, panel)| {
        run(workbench, "t.commentsCreate");
        run(workbench, "t.commentsKeptComment");
        settle(workbench);
        let (controller, thread, unique_ids) = panel.last_thread();

        let number = |value: u32| Dynamic::from(f64::from(value));
        let marshalled_thread = MarshalledCommentThread { comment_control_handle: controller, comment_thread_handle: thread }.to_dynamic();
        let marshalled = |id: MarshalledId, fields: Vec<(&str, Dynamic)>| {
            let mut entries = vec![("$mid", Dynamic::from(id as i32))];
            entries.extend(fields);
            Dynamic::object(entries)
        };

        // `CommentThread`: a mesma thread que `createCommentThread` devolveu (`commentThread.value` no original).
        assert_eq!(workbench.execute("t.commentsOnThread", vec![marshalled_thread.clone()]), Dynamic::string(r#"{"same":true,"body":["primeiro"]}"#));
        // `CommentController`.
        let controller_arg = marshalled(MarshalledId::CommentController, vec![("handle", number(controller))]);
        assert_eq!(workbench.execute("t.commentsOnController", vec![controller_arg]), Dynamic::string(r#"{"same":true,"id":"je-review"}"#));
        // `CommentThreadReply`: o `{ thread, text }` com a thread da extensão.
        let reply = marshalled(MarshalledId::CommentThreadReply, vec![("thread", marshalled_thread.clone()), ("text", Dynamic::string("resposta"))]);
        assert_eq!(workbench.execute("t.commentsOnReply", vec![reply]), Dynamic::string(r#"{"same":true,"text":"resposta"}"#));
        // `CommentNode`: o comentário que a extensão atribuiu, com as propriedades extras dela.
        let node = marshalled(MarshalledId::CommentNode, vec![("thread", marshalled_thread), ("commentUniqueId", number(unique_ids[0]))]);
        assert_eq!(workbench.execute("t.commentsOnComment", vec![node]), Dynamic::string(r#"{"extra":"mantido","body":"primeiro","same":true}"#));
    });
}

#[test]
fn dispose_removes_the_thread_and_the_controller_and_the_object_stops_working() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, (_observed, panel)| {
        run(workbench, "t.commentsCreate");
        run(workbench, "t.commentsSimpleThread");
        settle(workbench);
        assert_eq!(
            run(workbench, "t.commentsDispose"),
            Dynamic::string("CommentController: o controlador já foi descartado"),
            "o objeto do controlador descartado rejeita o acesso"
        );
        settle(workbench);

        let lines = panel.lines();
        assert_eq!(lines[lines.len() - 2..].to_vec(), strings(&["deleteThread", "unregister"]), "a thread some antes do controlador");
        assert_eq!(panel.created.borrow()[0].controller_handle, panel.controller_handle());
    });
}
