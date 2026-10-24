//! `extHostTypes.ts`: a região `Timeline` (`TimelineItem`; o `TimelineTriggerKind` está em [`super::enums`]).
//!
//! Divergências: `TimelineItem` é uma classe de propriedades públicas sem lógica (`constructor(public label, public
//! timestamp)`), e o host a trata como valor da extensão que cruza a fronteira: um objeto [`Dynamic`] de classe
//! `TimelineItem` (o `$$class` de [`crate::api::common::ext_host_values`]). A extensão atribui `id`, `iconPath`,
//! `description`, `tooltip`, `resourceUri`, `command`, `contextValue` e `accessibilityInformation` ao objeto, e o host
//! os lê pelo nome (o objeto é o mesmo que a extensão entregou, o que o `===` do original precisa). `label` e
//! `timestamp` entram como [`Dynamic`] porque o construtor do original não valida os tipos.

use je_base::dynamic::{Dynamic, ObjectRef};

use crate::api::common::ext_host_values::tag;

/// `new TimelineItem(label, timestamp)`.
pub fn new_timeline_item(label: impl Into<Dynamic>, timestamp: impl Into<Dynamic>) -> ObjectRef {
    let item = ObjectRef::from_entries([("label", label.into()), ("timestamp", timestamp.into())]);
    tag(Dynamic::Object(item.clone()), "TimelineItem");
    item
}
