//! As classes de posição da API: `Position`, `Range` e `Selection`, com a lógica e as validações dos tipos Rust de
//! `ext_host_types` (`extHostTypes.ts`). A instância carrega os campos públicos do original (`line`, `character`, `start`,
//! `end`, `anchor`, `active`); métodos e acessores reconstroem o valor Rust a partir do `this`.

use std::rc::Rc;

use je_workbench::api::common::ext_host_types::{Position, Range, Selection};
use zjsc::api::embed_node::{ClassSpec, JsData, NodeRunner, CLASS_KEY};

use crate::convert::{class_name, field};

fn number(value: Option<&JsData>) -> Option<f64> {
    match value {
        Some(JsData::Num(number)) => Some(*number),
        _ => None,
    }
}

fn marked(class: &str, mut fields: Vec<(String, JsData)>) -> JsData {
    fields.insert(0, (CLASS_KEY.to_owned(), JsData::Str(class.to_owned())));
    JsData::Object(fields)
}

pub(crate) fn position_data(position: Position) -> JsData {
    marked("Position", vec![("line".to_owned(), JsData::Num(position.line())), ("character".to_owned(), JsData::Num(position.character()))])
}

pub(crate) fn range_data(range: Range) -> JsData {
    marked("Range", vec![("start".to_owned(), position_data(range.start())), ("end".to_owned(), position_data(range.end()))])
}

pub(crate) fn selection_data(selection: Selection) -> JsData {
    marked(
        "Selection",
        vec![
            ("start".to_owned(), position_data(selection.start())),
            ("end".to_owned(), position_data(selection.end())),
            ("anchor".to_owned(), position_data(selection.anchor())),
            ("active".to_owned(), position_data(selection.active())),
        ],
    )
}

/// O `Position` de um valor com `line` e `character` numéricos (uma instância ou um literal, como o original aceita).
pub(crate) fn to_position(value: &JsData) -> Result<Position, String> {
    match (number(field(value, "line")), number(field(value, "character"))) {
        (Some(line), Some(character)) => Position::new(line, character).map_err(|error| error.message),
        _ => Err("Illegal argument: esperava uma Position".to_owned()),
    }
}

pub(crate) fn to_range(value: &JsData) -> Result<Range, String> {
    match (field(value, "start"), field(value, "end")) {
        (Some(start), Some(end)) => Ok(Range::new(to_position(start)?, to_position(end)?)),
        _ => Err("Illegal argument: esperava um Range".to_owned()),
    }
}

/// A `Selection` de um valor com `anchor` e `active` (uma instância ou um literal).
pub(crate) fn to_selection(value: &JsData) -> Result<Selection, String> {
    match (field(value, "anchor"), field(value, "active")) {
        (Some(anchor), Some(active)) => Ok(Selection::new(to_position(anchor)?, to_position(active)?)),
        _ => Err("Illegal argument: esperava uma Selection".to_owned()),
    }
}

fn opt_number(value: Option<&JsData>) -> Option<f64> {
    number(value)
}

/// O `Position` dos argumentos (`new X(position, position)` ou `new X(l, c, l, c)`): devolve os dois pontos.
fn two_points(args: &[JsData]) -> Result<(Position, Position), String> {
    match args {
        [first, second] => Ok((to_position(first)?, to_position(second)?)),
        [a, b, c, d] => {
            let numbers = [number(Some(a)), number(Some(b)), number(Some(c)), number(Some(d))];
            let [Some(a), Some(b), Some(c), Some(d)] = numbers else { return Err("Illegal argument: esperava quatro números".to_owned()) };
            Ok((Position::new(a, b).map_err(|e| e.message)?, Position::new(c, d).map_err(|e| e.message)?))
        }
        _ => Err("Illegal argument: esperava duas posições ou quatro números".to_owned()),
    }
}

fn define_position(runner: &NodeRunner) -> JsData {
    let compare = |name: &'static str, f: fn(&Position, &Position) -> bool| {
        (
            name.to_owned(),
            1,
            Rc::new(move |this: &JsData, args: &[JsData]| {
                let other = to_position(args.first().unwrap_or(&JsData::Undefined))?;
                Ok(JsData::Bool(f(&to_position(this)?, &other)))
            }) as zjsc::api::embed_node::MethodCallback,
        )
    };
    let constructor = runner.define_class(ClassSpec {
        name: "Position".to_owned(),
        length: 2,
        construct: Rc::new(|args| {
            let (Some(line), Some(character)) = (number(args.first()), number(args.get(1))) else {
                return Err("Illegal argument: line e character precisam ser números".to_owned());
            };
            let position = Position::new(line, character).map_err(|error| error.message)?;
            Ok(JsData::Object(vec![("line".to_owned(), JsData::Num(position.line())), ("character".to_owned(), JsData::Num(position.character()))]))
        }),
        methods: vec![
            compare("isBefore", Position::is_before),
            compare("isBeforeOrEqual", Position::is_before_or_equal),
            compare("isAfter", Position::is_after),
            compare("isAfterOrEqual", Position::is_after_or_equal),
            compare("isEqual", Position::is_equal),
            (
                "compareTo".to_owned(),
                1,
                Rc::new(|this, args| Ok(JsData::Num(f64::from(to_position(this)?.compare_to(&to_position(args.first().unwrap_or(&JsData::Undefined))?))))),
            ),
            (
                "translate".to_owned(),
                2,
                Rc::new(|this, args| {
                    let (line, character) = match args.first() {
                        Some(change @ JsData::Object(_)) => (opt_number(field(change, "lineDelta")), opt_number(field(change, "characterDelta"))),
                        _ => (number(args.first()), number(args.get(1))),
                    };
                    Ok(position_data(to_position(this)?.translate(line, character).map_err(|error| error.message)?))
                }),
            ),
            (
                "with".to_owned(),
                2,
                Rc::new(|this, args| {
                    let (line, character) = match args.first() {
                        Some(change @ JsData::Object(_)) => (opt_number(field(change, "line")), opt_number(field(change, "character"))),
                        _ => (number(args.first()), number(args.get(1))),
                    };
                    Ok(position_data(to_position(this)?.with(line, character).map_err(|error| error.message)?))
                }),
            ),
        ],
        getters: vec![],
        statics: vec![],
        parent: None,
    });
    JsData::Handle(Rc::new(constructor))
}

fn define_range(runner: &NodeRunner) -> JsData {
    let constructor = runner.define_class(ClassSpec {
        name: "Range".to_owned(),
        length: 2,
        construct: Rc::new(|args| {
            let (start, end) = two_points(args)?;
            let range = Range::new(start, end);
            Ok(JsData::Object(vec![("start".to_owned(), position_data(range.start())), ("end".to_owned(), position_data(range.end()))]))
        }),
        methods: vec![
            (
                "contains".to_owned(),
                1,
                Rc::new(|this, args| {
                    let range = to_range(this)?;
                    let target = args.first().unwrap_or(&JsData::Undefined);
                    match class_name(target) {
                        Some("Range" | "Selection") => Ok(JsData::Bool(range.contains_range(&to_range(target)?))),
                        _ => Ok(JsData::Bool(range.contains_position(&to_position(target)?))),
                    }
                }),
            ),
            ("isEqual".to_owned(), 1, Rc::new(|this, args| Ok(JsData::Bool(to_range(this)?.is_equal(&to_range(args.first().unwrap_or(&JsData::Undefined))?))))),
            (
                "intersection".to_owned(),
                1,
                Rc::new(|this, args| Ok(to_range(this)?.intersection(&to_range(args.first().unwrap_or(&JsData::Undefined))?).map_or(JsData::Undefined, range_data))),
            ),
            ("union".to_owned(), 1, Rc::new(|this, args| Ok(range_data(to_range(this)?.union(&to_range(args.first().unwrap_or(&JsData::Undefined))?))))),
            (
                "with".to_owned(),
                2,
                Rc::new(|this, args| {
                    let (start, end) = match args.first() {
                        Some(change @ JsData::Object(_)) if class_name(change).is_none() => (field(change, "start"), field(change, "end")),
                        _ => (args.first(), args.get(1)),
                    };
                    let start = start.filter(|value| !matches!(value, JsData::Undefined)).map(to_position).transpose()?;
                    let end = end.filter(|value| !matches!(value, JsData::Undefined)).map(to_position).transpose()?;
                    Ok(range_data(to_range(this)?.with(start, end)))
                }),
            ),
        ],
        getters: vec![
            ("isEmpty".to_owned(), Rc::new(|this, _| Ok(JsData::Bool(to_range(this)?.is_empty())))),
            ("isSingleLine".to_owned(), Rc::new(|this, _| Ok(JsData::Bool(to_range(this)?.is_single_line())))),
        ],
        statics: vec![],
        parent: None,
    });
    JsData::Handle(Rc::new(constructor))
}

fn define_selection(runner: &NodeRunner) -> JsData {
    let constructor = runner.define_class(ClassSpec {
        name: "Selection".to_owned(),
        length: 2,
        construct: Rc::new(|args| {
            let (anchor, active) = two_points(args)?;
            let JsData::Object(fields) = selection_data(Selection::new(anchor, active)) else { unreachable!() };
            Ok(JsData::Object(fields.into_iter().filter(|(key, _)| key != CLASS_KEY).collect()))
        }),
        methods: vec![],
        getters: vec![(
            "isReversed".to_owned(),
            Rc::new(|this, _| {
                let (anchor, active) = (field(this, "anchor"), field(this, "active"));
                match (anchor, active) {
                    (Some(anchor), Some(active)) => Ok(JsData::Bool(Selection::new(to_position(anchor)?, to_position(active)?).is_reversed())),
                    _ => Err("Illegal argument: esperava uma Selection".to_owned()),
                }
            }),
        )],
        statics: vec![],
        parent: Some("Range".to_owned()),
    });
    JsData::Handle(Rc::new(constructor))
}

/// As classes de posição como entradas do objeto `vscode`.
pub fn classes(runner: &NodeRunner) -> Vec<(String, JsData)> {
    vec![
        ("Position".to_owned(), define_position(runner)),
        ("Range".to_owned(), define_range(runner)),
        ("Selection".to_owned(), define_selection(runner)),
    ]
}
