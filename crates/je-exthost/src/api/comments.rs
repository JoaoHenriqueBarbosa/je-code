//! `vscode.comments.createCommentController` e as classes `vscode.CommentController` e `vscode.CommentThread` que ela entrega.
//!
//! É a ponte para o `ExtHostComments` portado (`extHostComments.ts`): `createCommentController(id, label)` cria o
//! `ExtHostCommentController`, que fala com o cliente `MainThreadComments` do workbench (`$registerCommentController`,
//! `$createCommentThread`, `$updateCommentThread`, `$updateCommentingRanges`...). O objeto que a extensão segura carrega só o `_id` (o
//! `handle` do controlador ou da thread); as propriedades são acessores do protótipo sobre o objeto vivo do `ExtHostComments`. O
//! `CommentController` tem `id`, `label`, `options`, `commentingRangeProvider`, `reactionHandler`, `createCommentThread` e `dispose`; a
//! `CommentThread` tem `uri`, `range`, `comments`, `collapsibleState`, `canReply`, `contextValue`, `label`, `state` e `dispose`.
//!
//! O `commentingRangeProvider` é chamado com um `vscode.TextDocument` do host (o mesmo objeto que `workspace.textDocuments` devolve) e
//! um token de cancelamento; devolve `Range[]` ou `{ ranges, enableFileComments }`, direto ou por promessa. O `reactionHandler` recebe
//! o comentário e a reação.
//!
//! Falta: `CommentThread#reveal` e `hide` (a proposta `commentReveal`, que o host nunca habilita: o original rejeitaria), `CommentThread#
//! applicability`, `CommentController#activeComment` e `activeCommentThread` (a proposta `activeComment`), `commentingRangeProvider#
//! resourceHints` (a proposta `commentingRangeHint`) e o `timestamp` de `Comment` (um `Date` não atravessa a fronteira do motor: só
//! um número, em milissegundos). Os argumentos de comando criados pelo `ExtHostCommentsImpl` (`CommentThread`, `CommentNode`...) chegam
//! ao comando da extensão como objetos de dados, sem o `this` da thread.
//!
//! Divergências:
//! - O `vscode` é um só para todas as extensões: o controlador é criado em nome da extensão `je.exthost`, e as propostas de API nunca
//!   estão habilitadas (o `CommentState`, o `reactors` de uma reação e a forma `{ resolved, applicability }` de `state` rejeitam, como o
//!   original rejeitaria para uma extensão sem a proposta).
//! - O `commentingRangeProvider` e o `reactionHandler` chegam como cópia (a atribuição a uma propriedade não é por referência): o
//!   provedor vale com `provideCommentingRanges` como propriedade PRÓPRIA (um objeto literal), e roda sem `this`; o comentário que o
//!   `reactionHandler` recebe é uma cópia do comentário da thread.
//! - Um comentário é um objeto de dados: a identidade que o original compara em `ExtHostCommentThread#comments` (um `Map` por objeto) é
//!   a das cópias que a thread guardou. A leitura de `comments` devolve o que a extensão atribuiu (com as propriedades extras dela).
//! - As tabelas de controladores e de threads não têm coletor além de `dispose` (o motor não avisa quando o objeto é descartado).

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use je_base::cancellation::CancellationToken;
use je_base::errors::Error;
use je_base::lifecycle::IDisposable;
use je_base::uri::URI;
use je_workbench::api::common::ext_host_comments::{ExtHostCommentController, ExtHostCommentThread, ExtHostComments, ReactionHandler};
use je_workbench::api::common::ext_host_document_data::TextDocument;
use je_workbench::api::common::ext_host_protocol::{CommentAuthorInformation, CommentCanReply, CommentOptions};
use je_workbench::api::common::ext_host_types::{
    Comment, CommentBody, CommentData, CommentMode, CommentReaction, CommentReactors, CommentState, CommentThreadApplicability, CommentThreadCollapsibleState,
    CommentThreadState, CommentThreadStateValue, CommentingRangeProvider, CommentingRanges, CommentingRangesResult, PathOrUri, Range,
};
use zjsc::api::embed_node::{ClassSpec, JsData, JsHandle, MethodCallback, NodeRunner};

use crate::api::commands::{call_settled, convert_maybe};
use crate::api::events::EventTables;
use crate::api::language_types::markdown_from_js;
use crate::api::languages::host_extension;
use crate::api::text_editors::TextObjects;
use crate::api::types::{range_data, to_range};
use crate::api::{getter, handle_field, handle_object, setter};
use crate::convert::{api_to_js, dynamic_to_js, field, is_nullish, opt_bool, opt_number, opt_string, string_arg, text_or_undefined, uri_from_js, uri_to_js};

/// O que o namespace `comments` guarda dos objetos que a extensão segura (os objetos JavaScript carregam só o `_id`, o `handle` do host).
#[derive(Default)]
pub struct CommentObjects {
    controllers: RefCell<HashMap<u32, Rc<ExtHostCommentController>>>,
    threads: RefCell<HashMap<u32, Rc<ExtHostCommentThread>>>,
    /// O dono (o `handle` do controlador) de cada thread, para soltar as threads com o controlador.
    thread_owners: RefCell<HashMap<u32, u32>>,
    /// O `commentingRangeProvider` que a extensão atribuiu, por controlador (a leitura devolve o que ela passou).
    providers: RefCell<HashMap<u32, JsData>>,
    /// O `reactionHandler` que a extensão atribuiu, por controlador.
    reaction_handlers: RefCell<HashMap<u32, JsData>>,
    /// Os comentários que a extensão atribuiu, por thread.
    comments: RefCell<HashMap<u32, Vec<JsData>>>,
}

impl CommentObjects {
    fn controller_of(&self, this: &JsData) -> Result<Rc<ExtHostCommentController>, String> {
        self.controllers.borrow().get(&handle_field(this, "CommentController")?).cloned().ok_or_else(|| "CommentController: o controlador já foi descartado".to_owned())
    }

    fn thread_of(&self, this: &JsData) -> Result<Rc<ExtHostCommentThread>, String> {
        self.threads.borrow().get(&handle_field(this, "CommentThread")?).cloned().ok_or_else(|| "CommentThread: a thread já foi descartada".to_owned())
    }

    fn forget_thread(&self, handle: u32) {
        self.threads.borrow_mut().remove(&handle);
        self.thread_owners.borrow_mut().remove(&handle);
        self.comments.borrow_mut().remove(&handle);
    }

    fn forget_controller(&self, handle: u32) {
        self.controllers.borrow_mut().remove(&handle);
        self.providers.borrow_mut().remove(&handle);
        self.reaction_handlers.borrow_mut().remove(&handle);
        let owned: Vec<u32> = self.thread_owners.borrow().iter().filter(|(_, owner)| **owner == handle).map(|(thread, _)| *thread).collect();
        for thread in owned {
            self.forget_thread(thread);
        }
    }
}

// ---- os valores de comentário

fn author_of(value: Option<&JsData>) -> Result<CommentAuthorInformation, String> {
    let author = value.filter(|author| matches!(author, JsData::Object(_))).ok_or("Illegal argument: o autor precisa ser { name, iconPath? }")?;
    let icon_path = match field(author, "iconPath") {
        Some(icon) if !is_nullish(Some(icon)) => Some(uri_from_js(icon)?),
        _ => None,
    };
    Ok(CommentAuthorInformation { name: opt_string(field(author, "name")).ok_or("Illegal argument: author.name")?, icon_path })
}

fn author_to_js(author: &CommentAuthorInformation) -> JsData {
    let mut fields = vec![("name".to_owned(), JsData::Str(author.name.clone()))];
    if let Some(icon) = &author.icon_path {
        fields.push(("iconPath".to_owned(), uri_to_js(icon)));
    }
    JsData::Object(fields)
}

fn reaction_of(value: &JsData) -> Result<CommentReaction, String> {
    let icon_path = match field(value, "iconPath") {
        Some(JsData::Str(path)) => Some(PathOrUri::Path(path.clone())),
        Some(icon @ JsData::Object(_)) => Some(PathOrUri::Uri(uri_from_js(icon)?)),
        _ => None,
    };
    let reactors = match field(value, "reactors") {
        Some(JsData::Array(items)) if items.iter().all(|item| matches!(item, JsData::Str(_))) => {
            Some(CommentReactors::Names(items.iter().filter_map(|item| opt_string(Some(item))).collect()))
        }
        Some(JsData::Array(items)) => Some(CommentReactors::Authors(items.iter().map(|item| author_of(Some(item))).collect::<Result<Vec<_>, String>>()?)),
        _ => None,
    };
    Ok(CommentReaction {
        label: opt_string(field(value, "label")),
        icon_path,
        count: opt_number(field(value, "count")).unwrap_or(0.0),
        author_has_reacted: opt_bool(field(value, "authorHasReacted")).unwrap_or(false),
        reactors,
    })
}

fn reaction_to_js(reaction: &CommentReaction) -> JsData {
    let mut fields: Vec<(String, JsData)> = Vec::new();
    if let Some(label) = &reaction.label {
        fields.push(("label".to_owned(), JsData::Str(label.clone())));
    }
    match &reaction.icon_path {
        Some(PathOrUri::Path(path)) => fields.push(("iconPath".to_owned(), JsData::Str(path.clone()))),
        Some(PathOrUri::Uri(uri)) => fields.push(("iconPath".to_owned(), uri_to_js(uri))),
        None => {}
    }
    fields.push(("count".to_owned(), JsData::Num(reaction.count)));
    fields.push(("authorHasReacted".to_owned(), JsData::Bool(reaction.author_has_reacted)));
    match &reaction.reactors {
        Some(CommentReactors::Names(names)) => fields.push(("reactors".to_owned(), JsData::Array(names.iter().cloned().map(JsData::Str).collect()))),
        Some(CommentReactors::Authors(authors)) => fields.push(("reactors".to_owned(), JsData::Array(authors.iter().map(author_to_js).collect()))),
        None => {}
    }
    JsData::Object(fields)
}

/// `vscode.Comment`: `body` (texto ou `MarkdownString`), `mode`, `author` e, opcionais, `contextValue`, `reactions`, `label`, `state` e
/// `timestamp`.
fn comment_of(value: &JsData) -> Result<Rc<Comment>, String> {
    let body = match field(value, "body") {
        Some(JsData::Str(text)) => CommentBody::Text(text.clone()),
        Some(markdown @ JsData::Object(_)) => CommentBody::Markdown(markdown_from_js(markdown).map_err(|error| error.message)?),
        _ => return Err("Illegal argument: Comment.body precisa ser texto ou MarkdownString".to_owned()),
    };
    let mode = match opt_number(field(value, "mode")) {
        Some(mode) => CommentMode::from_value(mode as i32).ok_or("Illegal argument: Comment.mode precisa ser um CommentMode")?,
        None => CommentMode::Editing,
    };
    let mut data = CommentData::new(body, mode, author_of(field(value, "author"))?);
    data.context_value = opt_string(field(value, "contextValue"));
    data.label = opt_string(field(value, "label"));
    data.state = match opt_number(field(value, "state")) {
        Some(state) => Some(CommentState::from_value(state as i32).ok_or("Illegal argument: Comment.state precisa ser um CommentState")?),
        None => None,
    };
    data.timestamp = opt_number(field(value, "timestamp"));
    data.reactions = match field(value, "reactions") {
        Some(JsData::Array(items)) => Some(items.iter().map(reaction_of).collect::<Result<Vec<_>, String>>()?),
        _ => None,
    };
    Ok(Comment::new(data))
}

fn comment_to_js(comment: &Comment) -> JsData {
    let data = comment.data.borrow();
    let body = match &data.body {
        CommentBody::Text(text) => JsData::Str(text.clone()),
        CommentBody::Markdown(markdown) => api_to_js(markdown),
    };
    let mut fields = vec![
        ("body".to_owned(), body),
        ("mode".to_owned(), JsData::Num(f64::from(data.mode.value()))),
        ("author".to_owned(), author_to_js(&data.author)),
    ];
    if let Some(context_value) = &data.context_value {
        fields.push(("contextValue".to_owned(), JsData::Str(context_value.clone())));
    }
    if let Some(reactions) = &data.reactions {
        fields.push(("reactions".to_owned(), JsData::Array(reactions.iter().map(reaction_to_js).collect())));
    }
    if let Some(label) = &data.label {
        fields.push(("label".to_owned(), JsData::Str(label.clone())));
    }
    if let Some(state) = data.state {
        fields.push(("state".to_owned(), JsData::Num(f64::from(state.value()))));
    }
    if let Some(timestamp) = data.timestamp {
        fields.push(("timestamp".to_owned(), JsData::Num(timestamp)));
    }
    JsData::Object(fields)
}

/// `Comment[]` (ou `undefined`, a lista vazia).
fn comments_of(value: &JsData) -> Result<Vec<Rc<Comment>>, String> {
    match value {
        JsData::Array(items) => items.iter().map(comment_of).collect(),
        JsData::Undefined | JsData::Null => Ok(Vec::new()),
        _ => Err("Illegal argument: os comentários precisam ser uma lista de Comment".to_owned()),
    }
}

fn range_of(value: &JsData) -> Result<Option<Range>, String> {
    if is_nullish(Some(value)) { Ok(None) } else { to_range(value).map(Some) }
}

fn can_reply_of(value: &JsData) -> Result<CommentCanReply, String> {
    match value {
        JsData::Object(_) => Ok(CommentCanReply::Author(Rc::new(author_of(Some(value))?))),
        other => Ok(CommentCanReply::Bool(opt_bool(Some(other)).unwrap_or(false))),
    }
}

fn can_reply_to_js(can_reply: &CommentCanReply) -> JsData {
    match can_reply {
        CommentCanReply::Bool(flag) => JsData::Bool(*flag),
        CommentCanReply::Author(author) => author_to_js(author),
    }
}

/// `CommentThreadState | { resolved?, applicability? }`.
fn state_of(value: &JsData) -> Result<CommentThreadStateValue, String> {
    let state = |value: Option<&JsData>| -> Result<Option<CommentThreadState>, String> {
        match opt_number(value) {
            Some(number) => CommentThreadState::from_value(number as i32).map(Some).ok_or_else(|| "Illegal argument: CommentThread.state inválido".to_owned()),
            None => Ok(None),
        }
    };
    match value {
        JsData::Object(_) => {
            let applicability = match opt_number(field(value, "applicability")) {
                Some(number) => Some(CommentThreadApplicability::from_value(number as i32).ok_or("Illegal argument: CommentThread.applicability inválido")?),
                None => None,
            };
            Ok(CommentThreadStateValue::Info { resolved: state(field(value, "resolved"))?, applicability })
        }
        other => state(Some(other))?.map(CommentThreadStateValue::State).ok_or_else(|| "Illegal argument: CommentThread.state precisa ser um CommentThreadState".to_owned()),
    }
}

fn state_to_js(state: CommentThreadStateValue) -> JsData {
    match state {
        CommentThreadStateValue::State(state) => JsData::Num(f64::from(state.value())),
        CommentThreadStateValue::Info { resolved, applicability } => {
            let mut fields: Vec<(String, JsData)> = Vec::new();
            if let Some(resolved) = resolved {
                fields.push(("resolved".to_owned(), JsData::Num(f64::from(resolved.value()))));
            }
            if let Some(applicability) = applicability {
                fields.push(("applicability".to_owned(), JsData::Num(f64::from(applicability.value()))));
            }
            JsData::Object(fields)
        }
    }
}

// ---- o provedor de ranges e o tratador de reações da extensão

fn ranges_of(items: &[JsData]) -> Result<Vec<Range>, Error> {
    items.iter().map(|item| to_range(item).map_err(Error::new)).collect()
}

/// `Range[] | CommentingRanges | undefined | null` que o provedor devolveu.
fn ranges_result_of(value: &JsData) -> Result<Option<CommentingRangesResult>, Error> {
    match value {
        JsData::Undefined | JsData::Null => Ok(None),
        JsData::Array(items) => Ok(Some(CommentingRangesResult::Ranges(ranges_of(items)?))),
        JsData::Object(_) => Ok(Some(CommentingRangesResult::Info(CommentingRanges {
            ranges: match field(value, "ranges") {
                Some(JsData::Array(items)) => Some(ranges_of(items)?),
                _ => None,
            },
            enable_file_comments: opt_bool(field(value, "enableFileComments")),
        }))),
        _ => Err(Error::new("provideCommentingRanges precisa devolver Range[] ou { ranges, enableFileComments }")),
    }
}

/// O `vscode.CommentingRangeProvider` da extensão: a função `provideCommentingRanges` (se tiver) como o provedor do controlador.
fn range_provider_of(runner: &NodeRunner, events: &Rc<EventTables>, documents: &Rc<TextObjects>, value: &JsData) -> Result<CommentingRangeProvider, String> {
    let Some(JsData::Handle(function)) = field(value, "provideCommentingRanges") else {
        return Err("commentingRangeProvider precisa ter a função provideCommentingRanges".to_owned());
    };
    let (runner, events, documents, function): (NodeRunner, Rc<EventTables>, Rc<TextObjects>, Rc<JsHandle>) =
        (runner.clone(), events.clone(), documents.clone(), function.clone());
    Ok(CommentingRangeProvider {
        provide_commenting_ranges: Rc::new(move |document: &TextDocument, token: &CancellationToken| {
            let result = call_settled(&runner, &function, &[documents.document_to_js(document), events.token_to_js(token.clone())]);
            convert_maybe(result, |value| ranges_result_of(&dynamic_to_js(&value)))
        }),
        resource_hints: None,
    })
}

/// O `reactionHandler(comment, reaction)` da extensão.
fn reaction_handler_of(runner: &NodeRunner, value: &JsData) -> Result<ReactionHandler, String> {
    let JsData::Handle(function) = value else { return Err("reactionHandler precisa ser uma função".to_owned()) };
    let (runner, function): (NodeRunner, Rc<JsHandle>) = (runner.clone(), function.clone());
    Ok(Rc::new(move |comment: &Rc<Comment>, reaction: &CommentReaction| {
        convert_maybe(call_settled(&runner, &function, &[comment_to_js(comment), reaction_to_js(reaction)]), |_| Ok(()))
    }))
}

// ---- as classes

fn define_thread(runner: &NodeRunner, objects: &Rc<CommentObjects>) {
    let get = |name: &str, read: fn(&CommentObjects, &Rc<ExtHostCommentThread>) -> JsData| getter(objects, name, CommentObjects::thread_of, read);
    let getters: Vec<(String, MethodCallback)> = vec![
        get("uri", |_, thread| uri_to_js(thread.uri())),
        get("range", |_, thread| thread.range().map_or(JsData::Undefined, range_data)),
        get("comments", |objects, thread| {
            let assigned = objects.comments.borrow().get(&thread.handle()).cloned();
            JsData::Array(assigned.unwrap_or_else(|| thread.comments().iter().map(|comment| comment_to_js(comment)).collect()))
        }),
        get("collapsibleState", |_, thread| thread.collapsible_state().map_or(JsData::Undefined, |state| JsData::Num(f64::from(state.value())))),
        get("canReply", |_, thread| can_reply_to_js(&thread.can_reply())),
        get("contextValue", |_, thread| text_or_undefined(thread.context_value())),
        get("label", |_, thread| text_or_undefined(thread.label())),
        get("state", |_, thread| thread.state().map_or(JsData::Undefined, state_to_js)),
    ];

    let set = |name: &str, write: fn(&CommentObjects, &Rc<ExtHostCommentThread>, &JsData) -> Result<(), String>| setter(objects, name, CommentObjects::thread_of, write);
    let setters: Vec<(String, MethodCallback)> = vec![
        set("range", |_, thread, value| {
            thread.set_range(range_of(value)?);
            Ok(())
        }),
        set("comments", |objects, thread, value| {
            let comments = comments_of(value)?;
            let assigned = match value {
                JsData::Array(items) => items.clone(),
                _ => Vec::new(),
            };
            objects.comments.borrow_mut().insert(thread.handle(), assigned);
            thread.set_comments(comments);
            Ok(())
        }),
        set("collapsibleState", |_, thread, value| {
            let state = opt_number(Some(value)).and_then(|number| CommentThreadCollapsibleState::from_value(number as i32));
            thread.set_collapsible_state(state.ok_or("Illegal argument: collapsibleState precisa ser um CommentThreadCollapsibleState")?);
            Ok(())
        }),
        set("canReply", |_, thread, value| {
            thread.set_can_reply(can_reply_of(value)?);
            Ok(())
        }),
        set("contextValue", |_, thread, value| {
            thread.set_context_value(opt_string(Some(value)));
            Ok(())
        }),
        set("label", |_, thread, value| {
            thread.set_label(opt_string(Some(value)));
            Ok(())
        }),
        set("state", |_, thread, value| thread.set_state(state_of(value)?).map_err(|error| error.message)),
    ];

    let dispose_objects = objects.clone();
    let methods: Vec<(String, u32, MethodCallback)> = vec![(
        "dispose".to_owned(),
        0,
        Rc::new(move |this, _| {
            let thread = dispose_objects.thread_of(this)?;
            thread.dispose();
            dispose_objects.forget_thread(thread.handle());
            Ok(JsData::Undefined)
        }),
    )];
    runner.define_accessor_class(
        ClassSpec {
            name: "CommentThread".to_owned(),
            length: 0,
            construct: Rc::new(|_| Err("CommentThread não tem construtor público: use CommentController#createCommentThread".to_owned())),
            methods,
            getters,
            statics: vec![],
            parent: None,
        },
        setters,
    );
}

fn define_controller(runner: &NodeRunner, objects: &Rc<CommentObjects>, events: &Rc<EventTables>, documents: &Rc<TextObjects>) {
    let get = |name: &str, read: fn(&CommentObjects, &Rc<ExtHostCommentController>) -> JsData| getter(objects, name, CommentObjects::controller_of, read);
    let getters: Vec<(String, MethodCallback)> = vec![
        get("id", |_, controller| JsData::Str(controller.id().to_owned())),
        get("label", |_, controller| JsData::Str(controller.label().to_owned())),
        get("options", |_, controller| {
            controller.options().map_or(JsData::Undefined, |options| {
                let mut fields: Vec<(String, JsData)> = Vec::new();
                if let Some(prompt) = options.prompt {
                    fields.push(("prompt".to_owned(), JsData::Str(prompt)));
                }
                if let Some(place_holder) = options.place_holder {
                    fields.push(("placeHolder".to_owned(), JsData::Str(place_holder)));
                }
                JsData::Object(fields)
            })
        }),
        get("commentingRangeProvider", |objects, controller| objects.providers.borrow().get(&controller.handle()).cloned().unwrap_or(JsData::Undefined)),
        get("reactionHandler", |objects, controller| objects.reaction_handlers.borrow().get(&controller.handle()).cloned().unwrap_or(JsData::Undefined)),
    ];

    let (provider_runner, provider_events, provider_documents) = (runner.clone(), events.clone(), documents.clone());
    let handler_runner = runner.clone();
    let setters: Vec<(String, MethodCallback)> = vec![
        setter(objects, "options", CommentObjects::controller_of, |_, controller, value| {
            let options = match value {
                JsData::Object(_) => Some(CommentOptions { prompt: opt_string(field(value, "prompt")), place_holder: opt_string(field(value, "placeHolder")) }),
                _ => None,
            };
            controller.set_options(options);
            Ok(())
        }),
        setter(objects, "commentingRangeProvider", CommentObjects::controller_of, move |objects, controller, value| {
            if is_nullish(Some(value)) {
                objects.providers.borrow_mut().remove(&controller.handle());
                return controller.set_commenting_range_provider(None).map_err(|error| error.message);
            }
            let provider = range_provider_of(&provider_runner, &provider_events, &provider_documents, value)?;
            objects.providers.borrow_mut().insert(controller.handle(), value.clone());
            controller.set_commenting_range_provider(Some(provider)).map_err(|error| error.message)
        }),
        setter(objects, "reactionHandler", CommentObjects::controller_of, move |objects, controller, value| {
            if is_nullish(Some(value)) {
                objects.reaction_handlers.borrow_mut().remove(&controller.handle());
                controller.set_reaction_handler(None);
                return Ok(());
            }
            let handler = reaction_handler_of(&handler_runner, value)?;
            objects.reaction_handlers.borrow_mut().insert(controller.handle(), value.clone());
            controller.set_reaction_handler(Some(handler));
            Ok(())
        }),
    ];

    let (create_objects, dispose_objects) = (objects.clone(), objects.clone());
    let methods: Vec<(String, u32, MethodCallback)> = vec![
        (
            "createCommentThread".to_owned(),
            3,
            Rc::new(move |this, args| {
                let controller = create_objects.controller_of(this)?;
                let uri: URI = uri_from_js(args.first().filter(|uri| !is_nullish(Some(*uri))).ok_or("Illegal argument: createCommentThread: a Uri")?)?;
                let range = range_of(args.get(1).unwrap_or(&JsData::Undefined))?;
                let comments = comments_of(args.get(2).unwrap_or(&JsData::Undefined))?;
                let thread = controller.create_comment_thread(uri, range, comments).map_err(|error| error.message)?;
                let handle = thread.handle();
                if let Some(JsData::Array(assigned)) = args.get(2) {
                    create_objects.comments.borrow_mut().insert(handle, assigned.clone());
                }
                create_objects.threads.borrow_mut().insert(handle, thread);
                create_objects.thread_owners.borrow_mut().insert(handle, controller.handle());
                Ok(handle_object("CommentThread", handle))
            }),
        ),
        (
            "dispose".to_owned(),
            0,
            Rc::new(move |this, _| {
                let controller = dispose_objects.controller_of(this)?;
                controller.dispose();
                dispose_objects.forget_controller(controller.handle());
                Ok(JsData::Undefined)
            }),
        ),
    ];
    runner.define_accessor_class(
        ClassSpec {
            name: "CommentController".to_owned(),
            length: 0,
            construct: Rc::new(|_| Err("CommentController não tem construtor público: use comments.createCommentController".to_owned())),
            methods,
            getters,
            statics: vec![],
            parent: None,
        },
        setters,
    );
}

/// Define as classes `CommentController` e `CommentThread` (protótipos dos objetos que o host entrega). `documents` é a tabela que dá o
/// `vscode.TextDocument` que o `commentingRangeProvider` recebe.
pub fn define_classes(runner: &NodeRunner, objects: &Rc<CommentObjects>, events: &Rc<EventTables>, documents: &Rc<TextObjects>) {
    define_thread(runner, objects);
    define_controller(runner, objects, events, documents);
}

/// O namespace `comments`: `createCommentController(id, label)`.
pub fn namespace(runner: &NodeRunner, comments: &Rc<ExtHostComments>, objects: &Rc<CommentObjects>) -> JsData {
    let (comments, objects) = (comments.clone(), objects.clone());
    let create = runner.make_function(
        "createCommentController",
        2,
        Rc::new(move |args| {
            let id = string_arg(args, 0, "createCommentController: o id")?;
            let label = string_arg(args, 1, "createCommentController: o label")?;
            let controller = comments.create_comment_controller(&host_extension(), &id, &label);
            let handle = controller.handle();
            objects.controllers.borrow_mut().insert(handle, controller);
            Ok(handle_object("CommentController", handle))
        }),
    );
    JsData::Object(vec![("createCommentController".to_owned(), JsData::Handle(Rc::new(create)))])
}
