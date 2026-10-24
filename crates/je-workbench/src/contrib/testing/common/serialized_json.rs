//! A forma JSON de `ISerializedTestResults`: o que o `JSON.stringify` grava no disco e o `JSON.parse` lê de volta em
//! `testResultStorage.ts`. Não é um arquivo do VS Code: o original usa o próprio objeto serializado como JSON, e o porte
//! tem tipos (e `Range`, `URI`, `IMarkdownString`) que precisam de conversão explícita para a mesma forma de texto.
//!
//! A forma segue a do original: as chaves na ordem em que as funções `serialize` do `testTypes.ts` as criam, `undefined`
//! omitido e `null` mantido. O tamanho do texto importa: o orçamento de bytes do armazenamento
//! (`RETAIN_MAX_BYTES`) conta o `byteLength` do `JSON.stringify`.

use je_base::JsString;
use je_base::errors::{Error, throw};
use je_base::html_content::{IMarkdownString, IsTrusted, MarkdownStringTrustedOptions};
use je_base::map::OrderedMap;
use je_base::uri::{URI, UriComponents, UriState};
use je_editor::position::Position;
use je_editor::range::Range;
use serde_json::{Map, Value};

use super::test_types::{
    ISerializedTestResults, ITestErrorMessageSerialized, ITestItemSerialized, ITestMessageSerialized, ITestMessageStackFrameSerialized,
    ITestOutputMessageSerialized, ITestTaskStateSerialized, IRichLocationSerialized, ResolvedTestRunRequest, SerializedTestResultsTask,
    TestItemExpandState, TestMessageText, TestResultItemSerialized, TestResultState, TestRunTarget,
};

type Obj = Map<String, Value>;

fn invalid(what: &str) -> Error {
    Error::new(format!("invalid serialized test results: {what}"))
}

/// Um `number` do JavaScript como JSON: inteiro quando é inteiro, `null` quando é `NaN` ou infinito (como o
/// `JSON.stringify`).
fn number(n: f64) -> Value {
    if n.is_finite() && n.fract() == 0.0 && n.abs() < 9_007_199_254_740_992.0 {
        Value::from(n as i64)
    } else {
        serde_json::Number::from_f64(n).map_or(Value::Null, Value::Number)
    }
}

fn js(units: &JsString) -> Value {
    Value::String(units.to_string_lossy())
}

fn opt_string(value: &Option<String>) -> Option<Value> {
    value.as_ref().map(|s| Value::String(s.clone()))
}

fn put(object: &mut Obj, key: &str, value: Option<Value>) {
    if let Some(value) = value {
        object.insert(key.to_string(), value);
    }
}

// region: leitura

fn as_obj<'a>(value: &'a Value, what: &str) -> Result<&'a Obj, Error> {
    value.as_object().ok_or_else(|| invalid(what))
}

fn field<'a>(object: &'a Obj, key: &str) -> Option<&'a Value> {
    object.get(key).filter(|value| !value.is_null())
}

fn req<'a>(object: &'a Obj, key: &str) -> Result<&'a Value, Error> {
    field(object, key).ok_or_else(|| invalid(key))
}

fn req_str(object: &Obj, key: &str) -> Result<String, Error> {
    req(object, key)?.as_str().map(str::to_string).ok_or_else(|| invalid(key))
}

fn opt_str(object: &Obj, key: &str) -> Result<Option<String>, Error> {
    field(object, key).map(|v| v.as_str().map(str::to_string).ok_or_else(|| invalid(key))).transpose()
}

fn req_num(object: &Obj, key: &str) -> Result<f64, Error> {
    req(object, key)?.as_f64().ok_or_else(|| invalid(key))
}

fn opt_num(object: &Obj, key: &str) -> Result<Option<f64>, Error> {
    field(object, key).map(|v| v.as_f64().ok_or_else(|| invalid(key))).transpose()
}

fn req_bool(object: &Obj, key: &str) -> Result<bool, Error> {
    req(object, key)?.as_bool().ok_or_else(|| invalid(key))
}

fn req_arr<'a>(object: &'a Obj, key: &str) -> Result<&'a Vec<Value>, Error> {
    req(object, key)?.as_array().ok_or_else(|| invalid(key))
}

fn string_list(value: &Value, what: &str) -> Result<Vec<String>, Error> {
    value.as_array().ok_or_else(|| invalid(what))?.iter().map(|v| v.as_str().map(str::to_string).ok_or_else(|| invalid(what))).collect()
}

// endregion

// region: URI, Range, Position, markdown

fn uri_to_json(state: &UriState) -> Value {
    let mut object = Obj::new();
    put(&mut object, "$mid", state.mid.map(|mid| number(f64::from(mid))));
    put(&mut object, "fsPath", state.fs_path.as_ref().map(js));
    put(&mut object, "_sep", state.sep.map(|sep| number(f64::from(sep))));
    put(&mut object, "external", state.external.as_ref().map(js));
    put(&mut object, "path", state.components.path.as_ref().map(js));
    put(&mut object, "scheme", state.components.scheme.as_ref().map(js));
    put(&mut object, "authority", state.components.authority.as_ref().map(js));
    put(&mut object, "query", state.components.query.as_ref().map(js));
    put(&mut object, "fragment", state.components.fragment.as_ref().map(js));
    Value::Object(object)
}

fn uri_from_json(value: &Value) -> Result<UriState, Error> {
    let object = as_obj(value, "uri")?;
    let text = |key: &str| -> Result<Option<JsString>, Error> { Ok(opt_str(object, key)?.map(JsString::from)) };
    Ok(UriState {
        mid: opt_num(object, "$mid")?.map(|mid| mid as i32),
        external: text("external")?,
        fs_path: text("fsPath")?,
        sep: opt_num(object, "_sep")?.map(|sep| sep as u8),
        components: UriComponents { scheme: text("scheme")?, authority: text("authority")?, path: text("path")?, query: text("query")?, fragment: text("fragment")? },
    })
}

fn range_to_json(range: &Range) -> Value {
    let mut object = Obj::new();
    object.insert("startLineNumber".into(), number(f64::from(range.start_line_number)));
    object.insert("startColumn".into(), number(f64::from(range.start_column)));
    object.insert("endLineNumber".into(), number(f64::from(range.end_line_number)));
    object.insert("endColumn".into(), number(f64::from(range.end_column)));
    Value::Object(object)
}

fn range_from_json(value: &Value) -> Result<Range, Error> {
    let object = as_obj(value, "range")?;
    Ok(Range::new(
        req_num(object, "startLineNumber")? as i32,
        req_num(object, "startColumn")? as i32,
        req_num(object, "endLineNumber")? as i32,
        req_num(object, "endColumn")? as i32,
    ))
}

fn position_to_json(position: &Position) -> Value {
    let mut object = Obj::new();
    object.insert("lineNumber".into(), number(f64::from(position.line_number)));
    object.insert("column".into(), number(f64::from(position.column)));
    Value::Object(object)
}

fn position_from_json(value: &Value) -> Result<Position, Error> {
    let object = as_obj(value, "position")?;
    Ok(Position::new(req_num(object, "lineNumber")? as i32, req_num(object, "column")? as i32))
}

fn markdown_to_json(markdown: &IMarkdownString) -> Value {
    let mut object = Obj::new();
    object.insert("value".into(), js(&markdown.value));
    put(
        &mut object,
        "isTrusted",
        markdown.is_trusted.as_ref().map(|trusted| match trusted {
            IsTrusted::Bool(b) => Value::Bool(*b),
            IsTrusted::Options(options) => {
                let mut inner = Obj::new();
                inner.insert("enabledCommands".into(), Value::Array(options.enabled_commands.iter().map(|c| Value::String(c.clone())).collect()));
                Value::Object(inner)
            }
        }),
    );
    put(&mut object, "supportThemeIcons", markdown.support_theme_icons.map(Value::Bool));
    put(&mut object, "supportHtml", markdown.support_html.map(Value::Bool));
    put(&mut object, "supportAlertSyntax", markdown.support_alert_syntax.map(Value::Bool));
    put(&mut object, "baseUri", markdown.base_uri.as_ref().map(|uri| uri_to_json(&uri.to_json())));
    put(
        &mut object,
        "uris",
        markdown.uris.as_ref().map(|uris| {
            let mut inner = Obj::new();
            for (key, uri) in uris.iter() {
                inner.insert(key.to_string_lossy(), uri_to_json(&uri.to_json()));
            }
            Value::Object(inner)
        }),
    );
    Value::Object(object)
}

fn markdown_from_json(value: &Value) -> Result<IMarkdownString, Error> {
    let object = as_obj(value, "markdown")?;
    let revive = |state: &UriState| URI::revive(state, je_platform::resources::host_platform());
    let is_trusted = match field(object, "isTrusted") {
        None => None,
        Some(Value::Bool(b)) => Some(IsTrusted::Bool(*b)),
        Some(other) => {
            let inner = as_obj(other, "isTrusted")?;
            Some(IsTrusted::Options(MarkdownStringTrustedOptions { enabled_commands: string_list(req(inner, "enabledCommands")?, "enabledCommands")? }))
        }
    };
    let uris = match field(object, "uris") {
        None => None,
        Some(value) => {
            let mut map = OrderedMap::new();
            for (key, uri) in as_obj(value, "uris")? {
                map.insert(JsString::from(key.as_str()), revive(&uri_from_json(uri)?));
            }
            Some(map)
        }
    };
    Ok(IMarkdownString {
        value: JsString::from(req_str(object, "value")?),
        is_trusted,
        support_theme_icons: field(object, "supportThemeIcons").and_then(Value::as_bool),
        support_html: field(object, "supportHtml").and_then(Value::as_bool),
        support_alert_syntax: field(object, "supportAlertSyntax").and_then(Value::as_bool),
        base_uri: field(object, "baseUri").map(|v| uri_from_json(v).map(|state| revive(&state))).transpose()?,
        uris,
    })
}

fn text_to_json(text: &TestMessageText) -> Value {
    match text {
        TestMessageText::Text(text) => Value::String(text.clone()),
        TestMessageText::Markdown(markdown) => markdown_to_json(markdown),
    }
}

fn text_from_json(value: &Value) -> Result<TestMessageText, Error> {
    match value {
        Value::String(text) => Ok(TestMessageText::Text(text.clone())),
        other => Ok(TestMessageText::Markdown(markdown_from_json(other)?)),
    }
}

// endregion

// region: mensagens, itens

fn location_to_json(location: &IRichLocationSerialized) -> Value {
    let mut object = Obj::new();
    object.insert("range".into(), range_to_json(&location.range));
    object.insert("uri".into(), uri_to_json(&location.uri));
    Value::Object(object)
}

fn location_from_json(value: &Value) -> Result<IRichLocationSerialized, Error> {
    let object = as_obj(value, "location")?;
    Ok(IRichLocationSerialized { range: range_from_json(req(object, "range")?)?, uri: uri_from_json(req(object, "uri")?)? })
}

fn frame_to_json(frame: &ITestMessageStackFrameSerialized) -> Value {
    let mut object = Obj::new();
    object.insert("label".into(), Value::String(frame.label.clone()));
    put(&mut object, "uri", frame.uri.as_ref().map(uri_to_json));
    put(&mut object, "position", frame.position.as_ref().map(position_to_json));
    Value::Object(object)
}

fn frame_from_json(value: &Value) -> Result<ITestMessageStackFrameSerialized, Error> {
    let object = as_obj(value, "stack frame")?;
    Ok(ITestMessageStackFrameSerialized {
        label: req_str(object, "label")?,
        uri: field(object, "uri").map(uri_from_json).transpose()?,
        position: field(object, "position").map(position_from_json).transpose()?,
    })
}

const MESSAGE_TYPE_ERROR: f64 = 0.0;
const MESSAGE_TYPE_OUTPUT: f64 = 1.0;

fn message_to_json(message: &ITestMessageSerialized) -> Value {
    let mut object = Obj::new();
    match message {
        ITestMessageSerialized::Error(error) => {
            object.insert("message".into(), text_to_json(&error.message));
            object.insert("type".into(), number(MESSAGE_TYPE_ERROR));
            put(&mut object, "expected", opt_string(&error.expected));
            put(&mut object, "actual", opt_string(&error.actual));
            put(&mut object, "contextValue", opt_string(&error.context_value));
            put(&mut object, "location", error.location.as_ref().map(location_to_json));
            put(&mut object, "stackTrace", error.stack_trace.as_ref().map(|frames| Value::Array(frames.iter().map(frame_to_json).collect())));
        }
        ITestMessageSerialized::Output(output) => {
            object.insert("message".into(), Value::String(output.message.clone()));
            object.insert("type".into(), number(MESSAGE_TYPE_OUTPUT));
            object.insert("offset".into(), number(output.offset as f64));
            object.insert("length".into(), number(output.length as f64));
            put(&mut object, "location", output.location.as_ref().map(location_to_json));
        }
    }
    Value::Object(object)
}

fn message_from_json(value: &Value) -> Result<ITestMessageSerialized, Error> {
    let object = as_obj(value, "message")?;
    let kind = req_num(object, "type")?;
    if kind == MESSAGE_TYPE_ERROR {
        Ok(ITestMessageSerialized::Error(ITestErrorMessageSerialized {
            message: text_from_json(req(object, "message")?)?,
            expected: opt_str(object, "expected")?,
            actual: opt_str(object, "actual")?,
            context_value: opt_str(object, "contextValue")?,
            location: field(object, "location").map(location_from_json).transpose()?,
            stack_trace: field(object, "stackTrace")
                .map(|frames| frames.as_array().ok_or_else(|| invalid("stackTrace"))?.iter().map(frame_from_json).collect::<Result<Vec<_>, Error>>())
                .transpose()?,
        }))
    } else {
        Ok(ITestMessageSerialized::Output(ITestOutputMessageSerialized {
            message: req_str(object, "message")?,
            offset: req_num(object, "offset")? as usize,
            length: req_num(object, "length")? as usize,
            location: field(object, "location").map(location_from_json).transpose()?,
        }))
    }
}

fn task_state_to_json(state: &ITestTaskStateSerialized) -> Value {
    let mut object = Obj::new();
    object.insert("state".into(), number(state.state.as_number()));
    put(&mut object, "duration", state.duration.map(number));
    object.insert("messages".into(), Value::Array(state.messages.iter().map(message_to_json).collect()));
    Value::Object(object)
}

fn task_state_from_json(value: &Value) -> Result<ITestTaskStateSerialized, Error> {
    let object = as_obj(value, "task state")?;
    Ok(ITestTaskStateSerialized {
        state: TestResultState::from_number(req_num(object, "state")?).ok_or_else(|| invalid("state"))?,
        duration: opt_num(object, "duration")?,
        messages: req_arr(object, "messages")?.iter().map(message_from_json).collect::<Result<Vec<_>, Error>>()?,
    })
}

fn item_to_json(item: &ITestItemSerialized) -> Value {
    let mut object = Obj::new();
    object.insert("extId".into(), Value::String(item.ext_id.clone()));
    object.insert("label".into(), Value::String(item.label.clone()));
    object.insert("tags".into(), Value::Array(item.tags.iter().map(|t| Value::String(t.clone())).collect()));
    object.insert("busy".into(), Value::Bool(item.busy));
    put(&mut object, "uri", item.uri.as_ref().map(uri_to_json));
    object.insert("range".into(), item.range.as_ref().map_or(Value::Null, range_to_json));
    object.insert("description".into(), item.description.as_ref().map_or(Value::Null, |d| Value::String(d.clone())));
    object.insert("error".into(), item.error.as_ref().map_or(Value::Null, text_to_json));
    object.insert("sortText".into(), item.sort_text.as_ref().map_or(Value::Null, |s| Value::String(s.clone())));
    Value::Object(object)
}

fn item_from_json(value: &Value) -> Result<ITestItemSerialized, Error> {
    let object = as_obj(value, "item")?;
    Ok(ITestItemSerialized {
        ext_id: req_str(object, "extId")?,
        label: req_str(object, "label")?,
        tags: string_list(req(object, "tags")?, "tags")?,
        busy: req_bool(object, "busy")?,
        uri: field(object, "uri").map(uri_from_json).transpose()?,
        range: field(object, "range").map(range_from_json).transpose()?,
        description: opt_str(object, "description")?,
        error: field(object, "error").map(text_from_json).transpose()?,
        sort_text: opt_str(object, "sortText")?,
    })
}

fn result_item_to_json(item: &TestResultItemSerialized) -> Value {
    let mut object = Obj::new();
    object.insert("expand".into(), number(item.expand.as_number()));
    object.insert("item".into(), item_to_json(&item.item));
    object.insert("ownComputedState".into(), number(item.own_computed_state.as_number()));
    object.insert("computedState".into(), number(item.computed_state.as_number()));
    object.insert("tasks".into(), Value::Array(item.tasks.iter().map(task_state_to_json).collect()));
    Value::Object(object)
}

fn result_item_from_json(value: &Value) -> Result<TestResultItemSerialized, Error> {
    let object = as_obj(value, "result item")?;
    let state = |key: &str| TestResultState::from_number(req_num(object, key)?).ok_or_else(|| invalid(key));
    Ok(TestResultItemSerialized {
        expand: TestItemExpandState::from_number(req_num(object, "expand")?).ok_or_else(|| invalid("expand"))?,
        item: item_from_json(req(object, "item")?)?,
        own_computed_state: state("ownComputedState")?,
        computed_state: state("computedState")?,
        tasks: req_arr(object, "tasks")?.iter().map(task_state_from_json).collect::<Result<Vec<_>, Error>>()?,
    })
}

// endregion

// region: pedido de execução e resultados

fn request_to_json(request: &ResolvedTestRunRequest) -> Value {
    let mut object = Obj::new();
    object.insert("group".into(), number(f64::from(request.group)));
    let targets = request
        .targets
        .iter()
        .map(|target| {
            let mut t = Obj::new();
            t.insert("profileId".into(), number(f64::from(target.profile_id)));
            t.insert("controllerId".into(), Value::String(target.controller_id.clone()));
            t.insert("testIds".into(), Value::Array(target.test_ids.iter().map(|id| Value::String(id.clone())).collect()));
            Value::Object(t)
        })
        .collect();
    object.insert("targets".into(), Value::Array(targets));
    put(&mut object, "exclude", request.exclude.as_ref().map(|ids| Value::Array(ids.iter().map(|id| Value::String(id.clone())).collect())));
    put(&mut object, "continuous", request.continuous.map(Value::Bool));
    put(&mut object, "preserveFocus", request.preserve_focus.map(Value::Bool));
    Value::Object(object)
}

fn request_from_json(value: &Value) -> Result<ResolvedTestRunRequest, Error> {
    let object = as_obj(value, "request")?;
    let targets = req_arr(object, "targets")?
        .iter()
        .map(|target| {
            let t = as_obj(target, "target")?;
            Ok(TestRunTarget {
                test_ids: string_list(req(t, "testIds")?, "testIds")?,
                controller_id: req_str(t, "controllerId")?,
                profile_id: req_num(t, "profileId")? as u32,
            })
        })
        .collect::<Result<Vec<_>, Error>>()?;
    Ok(ResolvedTestRunRequest {
        group: req_num(object, "group")? as u32,
        targets,
        exclude: field(object, "exclude").map(|ids| string_list(ids, "exclude")).transpose()?,
        continuous: field(object, "continuous").and_then(Value::as_bool),
        preserve_focus: field(object, "preserveFocus").and_then(Value::as_bool),
    })
}

impl ISerializedTestResults {
    /// O objeto que o `JSON.stringify` do original veria: `id`, `completedAt`, `tasks`, `name`, `request` e `items`.
    pub fn to_json_value(&self) -> Value {
        let mut object = Obj::new();
        object.insert("id".into(), Value::String(self.id.clone()));
        object.insert("completedAt".into(), number(self.completed_at));
        let tasks = self
            .tasks
            .iter()
            .map(|task| {
                let mut t = Obj::new();
                t.insert("id".into(), Value::String(task.id.clone()));
                put(&mut t, "name", opt_string(&task.name));
                t.insert("ctrlId".into(), Value::String(task.ctrl_id.clone()));
                t.insert("hasCoverage".into(), Value::Bool(task.has_coverage));
                Value::Object(t)
            })
            .collect();
        object.insert("tasks".into(), Value::Array(tasks));
        object.insert("name".into(), Value::String(self.name.clone()));
        object.insert("request".into(), request_to_json(&self.request));
        object.insert("items".into(), Value::Array(self.items.iter().map(result_item_to_json).collect()));
        Value::Object(object)
    }

    /// `JSON.stringify(serialized)`.
    pub fn to_json_string(&self) -> String {
        self.to_json_value().to_string()
    }

    /// O `JSON.parse` do texto como `ISerializedTestResults`. Um texto que não é JSON ou não tem a forma esperada lança,
    /// como o `JSON.parse` (ou o acesso a um campo ausente) do original.
    pub fn from_json_str(text: &str) -> ISerializedTestResults {
        match serde_json::from_str::<Value>(text).map_err(|e| Error::named("SyntaxError", e.to_string())).and_then(|value| ISerializedTestResults::from_json_value(&value)) {
            Ok(results) => results,
            Err(error) => throw(error),
        }
    }

    pub fn from_json_value(value: &Value) -> Result<ISerializedTestResults, Error> {
        let object = as_obj(value, "root")?;
        let tasks = req_arr(object, "tasks")?
            .iter()
            .map(|task| {
                let t = as_obj(task, "task")?;
                Ok(SerializedTestResultsTask {
                    id: req_str(t, "id")?,
                    name: opt_str(t, "name")?,
                    ctrl_id: req_str(t, "ctrlId")?,
                    has_coverage: req_bool(t, "hasCoverage")?,
                })
            })
            .collect::<Result<Vec<_>, Error>>()?;
        Ok(ISerializedTestResults {
            id: req_str(object, "id")?,
            completed_at: req_num(object, "completedAt")?,
            items: req_arr(object, "items")?.iter().map(result_item_from_json).collect::<Result<Vec<_>, Error>>()?,
            tasks,
            name: req_str(object, "name")?,
            request: request_from_json(req(object, "request")?)?,
        })
    }
}

// endregion
