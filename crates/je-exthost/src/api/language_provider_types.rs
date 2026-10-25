//! As classes de valor e a leitura do que devolvem os provedores de `vscode.languages` da segunda leva (lentes de código, destaque de
//! ocorrências, referências, assinatura, renomeação, dobra, seleção, tokens semânticos e dicas embutidas): `CodeLens`,
//! `DocumentHighlight`, `SignatureHelp`, `SignatureInformation`, `ParameterInformation`, `FoldingRange`, `SelectionRange`, `InlayHint`,
//! `InlayHintLabelPart`, `SemanticTokensLegend`, `SemanticTokens`, `SemanticTokensEdit`, `SemanticTokensEdits` e
//! `SemanticTokensBuilder`, mais os leitores que transformam o valor do motor (copiado) nos tipos de `ext_host_types` que os adaptadores
//! do `ExtHostLanguageFeatures` esperam. A base (os leitores olham a FORMA do valor, não a marca `$$class`) é a de `language_types`.
//!
//! Divergências:
//! - `SemanticTokensBuilder` guarda o estado do lado Rust, numa tabela por id (como `WorkspaceEdit`), porque um método de classe não
//!   altera o receptor que o motor copia. `build` devolve um objeto comum `{ resultId, data }`.
//! - `Uint32Array` não atravessa o motor: `SemanticTokens#data` e `SemanticTokensEdit#data` são listas de números (o leitor também aceita
//!   um objeto com `length` e índices numéricos), então `new SemanticTokens(new Uint32Array(...))` só funciona se o motor entregar o
//!   array tipado como um desses dois formatos.
//! - `CodeLens#isResolved` é um acessor de leitura do protótipo (vale `command` definido).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use je_base::errors::Error;
use je_workbench::api::common::ext_host_language_features_api::{ApiPrepareRename, ApiSemanticTokensResult, ApiSignatureHelpRef, CodeLensRef, InlayHintRef};
use je_workbench::api::common::ext_host_types::{
    CodeLens, Command, DocumentHighlight, DocumentHighlightKind, Documentation, FoldingRange, FoldingRangeKind, InlayHint, InlayHintKind, InlayHintLabel, InlayHintLabelPart,
    Location, ParameterInformation, ParameterLabel, SelectionRange, SemanticTokens, SemanticTokensBuilder, SemanticTokensEdit, SemanticTokensEdits, SemanticTokensLegend,
    SignatureHelp, SignatureInformation,
};
use zjsc::api::embed_node::{ClassSpec, JsData, MethodCallback, NodeRunner};

use crate::api::language_types::{
    class_fields, command_from_js, define, define_with_getters, documentation_from_js, location_from_js, number_arg, range_field, required, strings_of, text_arg,
    text_edits_from_js,
};
use crate::api::types::{position_data, range_data, to_position, to_range};
use crate::api::{handle_field, handle_object};
use crate::convert::{api_to_js, field, is_nullish, opt_bool, opt_number, opt_string, string_arg};

fn error(message: impl Into<String>) -> Error {
    Error::new(message.into())
}

fn shared<T>(value: T) -> Rc<RefCell<T>> {
    Rc::new(RefCell::new(value))
}

/// `undefined` e `null` são "sem resultado"; qualquer outro valor passa por `read`.
fn one_of<T>(value: &JsData, read: impl Fn(&JsData) -> Result<T, Error>) -> Result<Option<T>, Error> {
    if is_nullish(Some(value)) { Ok(None) } else { read(value).map(Some) }
}

/// Uma lista lida elemento a elemento; qualquer outro valor (`undefined`, `null`) é "sem resultado".
fn list_of<T>(value: &JsData, read: impl Fn(&JsData) -> Result<T, Error>) -> Result<Option<Vec<T>>, Error> {
    match value {
        JsData::Array(items) => items.iter().map(read).collect::<Result<Vec<_>, _>>().map(Some),
        _ => Ok(None),
    }
}

fn command_field(value: &JsData) -> Result<Option<Command>, Error> {
    match field(value, "command") {
        Some(command @ JsData::Object(_)) => command_from_js(command).map(Some),
        _ => Ok(None),
    }
}

fn kind_field<K>(value: &JsData, from_value: fn(i32) -> Option<K>) -> Option<K> {
    opt_number(field(value, "kind")).and_then(|kind| from_value(kind as i32))
}

// --- lente de código

fn code_lens_value(value: &JsData) -> Result<CodeLens, Error> {
    Ok(CodeLens::new(range_field(value, "range")?, command_field(value)?))
}

/// `CodeLens | undefined | null` (o resultado de `resolveCodeLens`).
pub fn code_lens_from_js(value: &JsData) -> Result<Option<CodeLensRef>, Error> {
    one_of(value, |value| code_lens_value(value).map(shared))
}

/// `CodeLens[] | undefined | null`.
pub fn code_lenses_from_js(value: &JsData) -> Result<Option<Vec<CodeLensRef>>, Error> {
    list_of(value, |value| code_lens_value(value).map(shared))
}

// --- destaque, referências

fn document_highlight_from_js(value: &JsData) -> Result<DocumentHighlight, Error> {
    Ok(DocumentHighlight::new(range_field(value, "range")?, kind_field(value, DocumentHighlightKind::from_value)))
}

/// `DocumentHighlight[] | undefined | null`.
pub fn document_highlights_from_js(value: &JsData) -> Result<Option<Vec<DocumentHighlight>>, Error> {
    list_of(value, document_highlight_from_js)
}

/// `Location[] | undefined | null`.
pub fn locations_from_js(value: &JsData) -> Result<Option<Vec<Location>>, Error> {
    list_of(value, location_from_js)
}

// --- assinatura

fn parameter_from_js(value: &JsData) -> Result<ParameterInformation, Error> {
    let label = match field(value, "label") {
        Some(JsData::Str(text)) => ParameterLabel::Text(text.clone()),
        Some(JsData::Array(pair)) => match (opt_number(pair.first()), opt_number(pair.get(1))) {
            (Some(start), Some(end)) => ParameterLabel::Offsets(start, end),
            _ => return Err(error("ParameterInformation.label must be a string or [number, number]")),
        },
        _ => return Err(error("ParameterInformation.label must be a string or [number, number]")),
    };
    Ok(ParameterInformation::new(label, documentation_from_js(field(value, "documentation"))?))
}

fn signature_information_from_js(value: &JsData) -> Result<SignatureInformation, Error> {
    let label = opt_string(field(value, "label")).ok_or_else(|| error("SignatureInformation.label must be a string"))?;
    let mut signature = SignatureInformation::new(label, documentation_from_js(field(value, "documentation"))?);
    signature.parameters = match field(value, "parameters") {
        Some(JsData::Array(items)) => items.iter().map(parameter_from_js).collect::<Result<Vec<_>, _>>()?,
        _ => Vec::new(),
    };
    signature.active_parameter = opt_number(field(value, "activeParameter"));
    Ok(signature)
}

fn signature_help_value(value: &JsData) -> Result<SignatureHelp, Error> {
    let Some(JsData::Array(items)) = field(value, "signatures") else { return Err(error("SignatureHelp.signatures must be an array")) };
    Ok(SignatureHelp {
        signatures: items.iter().map(signature_information_from_js).collect::<Result<Vec<_>, _>>()?,
        active_signature: opt_number(field(value, "activeSignature")).unwrap_or(0.0),
        active_parameter: opt_number(field(value, "activeParameter")).unwrap_or(0.0),
    })
}

/// `SignatureHelp | undefined | null`.
pub fn signature_help_from_js(value: &JsData) -> Result<Option<ApiSignatureHelpRef>, Error> {
    one_of(value, |value| signature_help_value(value).map(shared))
}

fn documentation_to_js(documentation: &Documentation) -> JsData {
    match documentation {
        Documentation::Text(text) => JsData::Str(text.clone()),
        Documentation::Markdown(markdown) => api_to_js(markdown),
    }
}

fn parameter_to_js(parameter: &ParameterInformation) -> JsData {
    let label = match &parameter.label {
        ParameterLabel::Text(text) => JsData::Str(text.clone()),
        ParameterLabel::Offsets(start, end) => JsData::Array(vec![JsData::Num(*start), JsData::Num(*end)]),
    };
    class_fields(vec![("label", Some(label)), ("documentation", parameter.documentation.as_ref().map(documentation_to_js))])
}

fn signature_to_js(signature: &SignatureInformation) -> JsData {
    class_fields(vec![
        ("label", Some(JsData::Str(signature.label.clone()))),
        ("documentation", signature.documentation.as_ref().map(documentation_to_js)),
        ("parameters", Some(JsData::Array(signature.parameters.iter().map(parameter_to_js).collect()))),
        ("activeParameter", signature.active_parameter.map(JsData::Num)),
    ])
}

/// O `SignatureHelp` como o objeto que o provedor lê em `SignatureHelpContext#activeSignatureHelp`.
pub fn signature_help_to_js(help: &SignatureHelp) -> JsData {
    class_fields(vec![
        ("signatures", Some(JsData::Array(help.signatures.iter().map(signature_to_js).collect()))),
        ("activeSignature", Some(JsData::Num(help.active_signature))),
        ("activeParameter", Some(JsData::Num(help.active_parameter))),
    ])
}

// --- renomeação

/// `Range | { range, placeholder } | undefined | null`: o resultado de `prepareRename`.
pub fn prepare_rename_from_js(value: &JsData) -> Result<Option<ApiPrepareRename>, Error> {
    one_of(value, |value| {
        if field(value, "start").is_some() {
            return to_range(value).map(ApiPrepareRename::Range).map_err(error);
        }
        match field(value, "range") {
            Some(range @ JsData::Object(_)) => {
                Ok(ApiPrepareRename::RangeWithPlaceholder { range: to_range(range).map_err(error)?, placeholder: opt_string(field(value, "placeholder")) })
            }
            _ => Err(error("prepareRename must return a Range or { range, placeholder }")),
        }
    })
}

// --- dobra, seleção

fn folding_range_from_js(value: &JsData) -> Result<FoldingRange, Error> {
    let number = |name: &str| opt_number(field(value, name)).ok_or_else(|| error(format!("FoldingRange.{name} must be a number")));
    Ok(FoldingRange::new(number("start")?, number("end")?, kind_field(value, FoldingRangeKind::from_value)))
}

/// `FoldingRange[] | undefined | null`.
pub fn folding_ranges_from_js(value: &JsData) -> Result<Option<Vec<FoldingRange>>, Error> {
    list_of(value, folding_range_from_js)
}

fn selection_range_from_js(value: &JsData) -> Result<SelectionRange, Error> {
    let parent = match field(value, "parent") {
        Some(parent @ JsData::Object(_)) => Some(selection_range_from_js(parent)?),
        _ => None,
    };
    SelectionRange::new(range_field(value, "range")?, parent)
}

/// `SelectionRange[] | undefined | null`.
pub fn selection_ranges_from_js(value: &JsData) -> Result<Option<Vec<SelectionRange>>, Error> {
    list_of(value, selection_range_from_js)
}

// --- tokens semânticos

/// Uma lista de números sem sinal: um array, ou o objeto de índices numéricos de um array tipado.
fn number_list(value: Option<&JsData>) -> Option<Vec<u32>> {
    match value {
        Some(JsData::Array(items)) => items.iter().map(|item| opt_number(Some(item)).map(|number| number as u32)).collect(),
        Some(object @ JsData::Object(_)) => {
            let length = opt_number(field(object, "length"))? as usize;
            (0..length).map(|index| opt_number(field(object, &index.to_string())).map(|number| number as u32)).collect()
        }
        _ => None,
    }
}

fn semantic_tokens_value(value: &JsData) -> Result<SemanticTokens, Error> {
    let data = number_list(field(value, "data")).ok_or_else(|| error("SemanticTokens.data must be a Uint32Array"))?;
    Ok(SemanticTokens::new(data, opt_string(field(value, "resultId"))))
}

/// `SemanticTokens | undefined | null`.
pub fn semantic_tokens_from_js(value: &JsData) -> Result<Option<SemanticTokens>, Error> {
    one_of(value, semantic_tokens_value)
}

fn semantic_tokens_edit_from_js(value: &JsData) -> Result<SemanticTokensEdit, Error> {
    let number = |name: &str| opt_number(field(value, name)).ok_or_else(|| error(format!("SemanticTokensEdit.{name} must be a number")));
    Ok(SemanticTokensEdit::new(number("start")?, number("deleteCount")?, number_list(field(value, "data"))))
}

/// `SemanticTokens | SemanticTokensEdits | undefined | null`: as edições têm a lista `edits`.
pub fn semantic_tokens_result_from_js(value: &JsData) -> Result<Option<ApiSemanticTokensResult>, Error> {
    one_of(value, |value| match field(value, "edits") {
        Some(JsData::Array(edits)) => Ok(ApiSemanticTokensResult::Edits(SemanticTokensEdits::new(
            edits.iter().map(semantic_tokens_edit_from_js).collect::<Result<Vec<_>, _>>()?,
            opt_string(field(value, "resultId")),
        ))),
        _ => semantic_tokens_value(value).map(ApiSemanticTokensResult::Tokens),
    })
}

/// `SemanticTokensLegend`: `tokenTypes` e `tokenModifiers`.
pub fn semantic_tokens_legend_from_js(value: &JsData) -> Result<SemanticTokensLegend, String> {
    let token_types = strings_of(field(value, "tokenTypes")).ok_or("SemanticTokensLegend.tokenTypes precisa ser uma lista de textos")?;
    Ok(SemanticTokensLegend::new(token_types, strings_of(field(value, "tokenModifiers"))))
}

// --- dicas embutidas

fn inlay_hint_label_part_from_js(value: &JsData) -> Result<InlayHintLabelPart, Error> {
    let mut part = InlayHintLabelPart::new(opt_string(field(value, "value")).ok_or_else(|| error("InlayHintLabelPart.value must be a string"))?);
    part.tooltip = documentation_from_js(field(value, "tooltip"))?;
    part.location = match field(value, "location") {
        Some(location @ JsData::Object(_)) => Some(location_from_js(location)?),
        _ => None,
    };
    part.command = command_field(value)?;
    Ok(part)
}

fn inlay_hint_value(value: &JsData) -> Result<InlayHint, Error> {
    let position = to_position(field(value, "position").ok_or_else(|| error("InlayHint.position must be a Position"))?).map_err(error)?;
    let label = match field(value, "label") {
        Some(JsData::Str(text)) => InlayHintLabel::Text(text.clone()),
        Some(JsData::Array(parts)) => InlayHintLabel::Parts(parts.iter().map(inlay_hint_label_part_from_js).collect::<Result<Vec<_>, _>>()?),
        _ => return Err(error("InlayHint.label must be a string or an array of InlayHintLabelPart")),
    };
    let mut hint = InlayHint::new(position, label, kind_field(value, InlayHintKind::from_value));
    hint.tooltip = documentation_from_js(field(value, "tooltip"))?;
    hint.text_edits = match field(value, "textEdits") {
        Some(edits) => text_edits_from_js(edits)?,
        None => None,
    };
    hint.padding_left = opt_bool(field(value, "paddingLeft"));
    hint.padding_right = opt_bool(field(value, "paddingRight"));
    Ok(hint)
}

/// `InlayHint | undefined | null` (o resultado de `resolveInlayHint`).
pub fn inlay_hint_from_js(value: &JsData) -> Result<Option<InlayHintRef>, Error> {
    one_of(value, |value| inlay_hint_value(value).map(shared))
}

/// `InlayHint[] | undefined | null`.
pub fn inlay_hints_from_js(value: &JsData) -> Result<Option<Vec<InlayHintRef>>, Error> {
    list_of(value, |value| inlay_hint_value(value).map(shared))
}

// --- SemanticTokensBuilder

/// Os `SemanticTokensBuilder` que as extensões criaram, por id (o objeto JavaScript carrega só o `_id`).
#[derive(Default)]
pub struct SemanticTokensBuilders {
    next: Cell<u32>,
    items: RefCell<HashMap<u32, SemanticTokensBuilder>>,
}

impl SemanticTokensBuilders {
    fn create(&self, legend: Option<&SemanticTokensLegend>) -> JsData {
        let id = self.next.get() + 1;
        self.next.set(id);
        self.items.borrow_mut().insert(id, SemanticTokensBuilder::new(legend));
        handle_object("SemanticTokensBuilder", id)
    }

    fn with<T>(&self, this: &JsData, run: impl FnOnce(&mut SemanticTokensBuilder) -> Result<T, String>) -> Result<T, String> {
        let id = handle_field(this, "SemanticTokensBuilder")?;
        run(self.items.borrow_mut().get_mut(&id).ok_or("SemanticTokensBuilder: o construtor não existe neste host")?)
    }
}

fn push_tokens(builder: &mut SemanticTokensBuilder, args: &[JsData]) -> Result<JsData, String> {
    match args.first() {
        Some(JsData::Num(_)) => {
            let number = |index: usize| opt_number(args.get(index)).map(|number| number as u32).ok_or_else(|| "Illegal argument".to_owned());
            builder.push(number(0)?, number(1)?, number(2)?, number(3)?, opt_number(args.get(4)).map(|number| number as u32));
        }
        Some(range @ JsData::Object(_)) => {
            let token_type = string_arg(args, 1, "SemanticTokensBuilder#push: o tokenType")?;
            let modifiers = strings_of(args.get(2));
            let modifiers: Option<Vec<&str>> = modifiers.as_ref().map(|modifiers| modifiers.iter().map(String::as_str).collect());
            builder.push_range(&to_range(range)?, &token_type, modifiers.as_deref()).map_err(|error| error.message)?;
        }
        _ => return Err("Illegal argument".to_owned()),
    }
    Ok(JsData::Undefined)
}

fn define_builder_class(runner: &NodeRunner, builders: &Rc<SemanticTokensBuilders>) -> JsData {
    let (push_builders, build_builders, create_builders) = (builders.clone(), builders.clone(), builders.clone());
    let methods: Vec<(String, u32, MethodCallback)> = vec![
        ("push".to_owned(), 5, Rc::new(move |this, args| push_builders.with(this, |builder| push_tokens(builder, args)))),
        (
            "build".to_owned(),
            1,
            Rc::new(move |this, args| {
                build_builders.with(this, |builder| {
                    let tokens = builder.build(opt_string(args.first()).as_deref());
                    Ok(class_fields(vec![
                        ("resultId", tokens.result_id.map(JsData::Str)),
                        ("data", Some(JsData::Array(tokens.data.iter().map(|value| JsData::Num(f64::from(*value))).collect()))),
                    ]))
                })
            }),
        ),
    ];
    JsData::Handle(Rc::new(runner.define_class(ClassSpec {
        name: "SemanticTokensBuilder".to_owned(),
        length: 1,
        construct: Rc::new(move |args| {
            let legend = match args.first() {
                Some(legend @ JsData::Object(_)) => Some(semantic_tokens_legend_from_js(legend)?),
                _ => None,
            };
            Ok(create_builders.create(legend.as_ref()))
        }),
        methods,
        getters: vec![],
        statics: vec![],
        parent: None,
    })))
}

// --- classes

fn array_or_empty(args: &[JsData], index: usize) -> JsData {
    match args.get(index) {
        Some(items @ JsData::Array(_)) => items.clone(),
        _ => JsData::Array(vec![]),
    }
}

fn optional_arg(args: &[JsData], index: usize) -> Option<JsData> {
    args.get(index).filter(|value| !is_nullish(Some(*value))).cloned()
}

/// As classes deste módulo como entradas do objeto `vscode`.
pub fn classes(runner: &NodeRunner, builders: &Rc<SemanticTokensBuilders>) -> Vec<(String, JsData)> {
    let entry = |name: &str, value: JsData| (name.to_owned(), value);
    let code_lens_getters: Vec<(String, MethodCallback)> =
        vec![("isResolved".to_owned(), Rc::new(|this, _| Ok(JsData::Bool(field(this, "command").is_some_and(|command| !is_nullish(Some(command)))))))];
    vec![
        entry(
            "CodeLens",
            define_with_getters(
                runner,
                "CodeLens",
                2,
                |args| Ok(class_fields(vec![("range", Some(range_data(to_range(required(args, 0, "CodeLens: o intervalo")?)?))), ("command", optional_arg(args, 1))])),
                vec![],
                code_lens_getters,
            ),
        ),
        entry(
            "DocumentHighlight",
            define(
                runner,
                "DocumentHighlight",
                2,
                |args| {
                    Ok(class_fields(vec![
                        ("range", Some(range_data(to_range(required(args, 0, "DocumentHighlight: o intervalo")?)?))),
                        ("kind", Some(number_arg(args, 1).unwrap_or(JsData::Num(f64::from(DocumentHighlightKind::Text.value()))))),
                    ]))
                },
                vec![],
            ),
        ),
        entry(
            "SignatureHelp",
            define(
                runner,
                "SignatureHelp",
                0,
                |_| {
                    Ok(class_fields(vec![
                        ("signatures", Some(JsData::Array(vec![]))),
                        ("activeSignature", Some(JsData::Num(0.0))),
                        ("activeParameter", Some(JsData::Num(0.0))),
                    ]))
                },
                vec![],
            ),
        ),
        entry(
            "SignatureInformation",
            define(
                runner,
                "SignatureInformation",
                2,
                |args| {
                    Ok(class_fields(vec![
                        ("label", Some(JsData::Str(string_arg(args, 0, "SignatureInformation: o rótulo")?))),
                        ("documentation", optional_arg(args, 1)),
                        ("parameters", Some(JsData::Array(vec![]))),
                    ]))
                },
                vec![],
            ),
        ),
        entry(
            "ParameterInformation",
            define(
                runner,
                "ParameterInformation",
                2,
                |args| Ok(class_fields(vec![("label", Some(required(args, 0, "ParameterInformation: o rótulo")?.clone())), ("documentation", optional_arg(args, 1))])),
                vec![],
            ),
        ),
        entry(
            "FoldingRange",
            define(
                runner,
                "FoldingRange",
                3,
                |args| {
                    Ok(class_fields(vec![
                        ("start", Some(number_arg(args, 0).ok_or("FoldingRange: o início é obrigatório")?)),
                        ("end", Some(number_arg(args, 1).ok_or("FoldingRange: o fim é obrigatório")?)),
                        ("kind", number_arg(args, 2)),
                    ]))
                },
                vec![],
            ),
        ),
        entry(
            "SelectionRange",
            define(
                runner,
                "SelectionRange",
                2,
                |args| Ok(class_fields(vec![("range", Some(range_data(to_range(required(args, 0, "SelectionRange: o intervalo")?)?))), ("parent", optional_arg(args, 1))])),
                vec![],
            ),
        ),
        entry(
            "InlayHint",
            define(
                runner,
                "InlayHint",
                3,
                |args| {
                    Ok(class_fields(vec![
                        ("position", Some(position_data(to_position(required(args, 0, "InlayHint: a posição")?)?))),
                        ("label", Some(required(args, 1, "InlayHint: o rótulo")?.clone())),
                        ("kind", number_arg(args, 2)),
                    ]))
                },
                vec![],
            ),
        ),
        entry(
            "InlayHintLabelPart",
            define(
                runner,
                "InlayHintLabelPart",
                1,
                |args| Ok(class_fields(vec![("value", Some(JsData::Str(string_arg(args, 0, "InlayHintLabelPart: o valor")?)))])),
                vec![],
            ),
        ),
        entry(
            "SemanticTokensLegend",
            define(
                runner,
                "SemanticTokensLegend",
                2,
                |args| Ok(class_fields(vec![("tokenTypes", Some(array_or_empty(args, 0))), ("tokenModifiers", Some(array_or_empty(args, 1)))])),
                vec![],
            ),
        ),
        entry(
            "SemanticTokens",
            define(
                runner,
                "SemanticTokens",
                2,
                |args| Ok(class_fields(vec![("data", Some(required(args, 0, "SemanticTokens: os dados")?.clone())), ("resultId", text_arg(args, 1).map(JsData::Str))])),
                vec![],
            ),
        ),
        entry(
            "SemanticTokensEdit",
            define(
                runner,
                "SemanticTokensEdit",
                3,
                |args| {
                    Ok(class_fields(vec![
                        ("start", Some(number_arg(args, 0).ok_or("SemanticTokensEdit: o início é obrigatório")?)),
                        ("deleteCount", Some(number_arg(args, 1).ok_or("SemanticTokensEdit: deleteCount é obrigatório")?)),
                        ("data", optional_arg(args, 2)),
                    ]))
                },
                vec![],
            ),
        ),
        entry(
            "SemanticTokensEdits",
            define(
                runner,
                "SemanticTokensEdits",
                2,
                |args| Ok(class_fields(vec![("edits", Some(array_or_empty(args, 0))), ("resultId", text_arg(args, 1).map(JsData::Str))])),
                vec![],
            ),
        ),
        entry("SemanticTokensBuilder", define_builder_class(runner, builders)),
    ]
}
