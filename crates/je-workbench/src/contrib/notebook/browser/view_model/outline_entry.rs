//! Porte de `src/vs/workbench/contrib/notebook/browser/viewModel/OutlineEntry.ts`: uma entrada do outline do notebook (um
//! cabeçalho de markdown, a primeira linha de uma célula ou um símbolo de célula de código), com os filhos, o pai e a
//! informação de marcadores (erros e avisos).
//!
//! Divergências:
//! - `cell: ICellViewModel` é o [`CellViewModel`] (a única forma de `ICellViewModel` na camada lógica); `cell.uri` é `uri()`.
//! - `children`/`parent` do original são campos mutáveis: aqui `RefCell` com `Weak` para o pai (sem ciclo de contagem), e
//!   `addChild` exige o `Rc` da entrada (`self: &Rc<Self>`) para registrar o pai.
//! - `range?: IRange` é `Option<Range>` e `symbolKind?: SymbolKind` é `Option<SymbolKind>`.
//! - `SymbolKinds.toIcon(kind)` (de `editor/common/languages.ts`) não tem porte: [`symbol_kind_to_icon`] é a tabela do original.
//! - `Codicon.markdown`/`Codicon.code` e `executingStateIcon` (`Codicon.sync`, ver `notebookIcons.ts`) são `ThemeIcon::from_id`;
//!   `ThemeIcon.modify(icon, 'spin')` é `ThemeIcon::modify`.
//! - `this.symbolKind` como teste de verdade é [`OutlineEntry::has_symbol_kind`] (`SymbolKind.File` é `0`, falso em JavaScript).
//! - `get position()` devolve o [`Position`] do início do `range`.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use je_base::themables::{IconModifier, ThemeIcon};
use je_editor::language_providers::SymbolKind;
use je_editor::position::Position;
use je_editor::range::Range;
use je_platform::markers::markers::{IMarkerService, MarkerReadOptions, MarkerSeverity};

use crate::contrib::notebook::browser::view_model::notebook_view_model_impl::CellViewModel;
use crate::contrib::notebook::common::notebook_common::CellKind;

/// `IOutlineMarkerInfo`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IOutlineMarkerInfo {
    pub count: usize,
    pub top_sev: MarkerSeverity,
}

/// `SymbolKinds.toIcon(kind)`.
pub fn symbol_kind_to_icon(kind: SymbolKind) -> ThemeIcon {
    let id = match kind {
        SymbolKind::File => "symbol-file",
        SymbolKind::Module => "symbol-module",
        SymbolKind::Namespace => "symbol-namespace",
        SymbolKind::Package => "symbol-package",
        SymbolKind::Class => "symbol-class",
        SymbolKind::Method => "symbol-method",
        SymbolKind::Property => "symbol-property",
        SymbolKind::Field => "symbol-field",
        SymbolKind::Constructor => "symbol-constructor",
        SymbolKind::Enum => "symbol-enum",
        SymbolKind::Interface => "symbol-interface",
        SymbolKind::Function => "symbol-function",
        SymbolKind::Variable => "symbol-variable",
        SymbolKind::Constant => "symbol-constant",
        SymbolKind::String => "symbol-string",
        SymbolKind::Number => "symbol-number",
        SymbolKind::Boolean => "symbol-boolean",
        SymbolKind::Array => "symbol-array",
        SymbolKind::Object => "symbol-object",
        SymbolKind::Key => "symbol-key",
        SymbolKind::Null => "symbol-null",
        SymbolKind::EnumMember => "symbol-enum-member",
        SymbolKind::Struct => "symbol-struct",
        SymbolKind::Event => "symbol-event",
        SymbolKind::Operator => "symbol-operator",
        SymbolKind::TypeParameter => "symbol-type-parameter",
    };
    ThemeIcon::from_id(id)
}

/// `OutlineEntry`.
pub struct OutlineEntry {
    pub index: i64,
    pub level: i64,
    pub cell: CellViewModel,
    pub label: String,
    pub is_executing: bool,
    pub is_paused: bool,
    pub range: Option<Range>,
    pub symbol_kind: Option<SymbolKind>,
    children: RefCell<Vec<Rc<OutlineEntry>>>,
    parent: RefCell<Weak<OutlineEntry>>,
    marker_info: RefCell<Option<IOutlineMarkerInfo>>,
}

impl OutlineEntry {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        index: i64,
        level: i64,
        cell: CellViewModel,
        label: &str,
        is_executing: bool,
        is_paused: bool,
        range: Option<Range>,
        symbol_kind: Option<SymbolKind>,
    ) -> Rc<OutlineEntry> {
        Rc::new(OutlineEntry {
            index,
            level,
            cell,
            label: label.to_string(),
            is_executing,
            is_paused,
            range,
            symbol_kind,
            children: RefCell::new(Vec::new()),
            parent: RefCell::new(Weak::new()),
            marker_info: RefCell::new(None),
        })
    }

    /// O teste de verdade de `this.symbolKind` / `!!element.symbolKind`: `SymbolKind.File` vale `0`, que em JavaScript é falso
    /// (como a ausência de tipo).
    pub fn has_symbol_kind(&self) -> bool {
        self.symbol_kind.is_some_and(|kind| kind != SymbolKind::File)
    }

    /// `get icon()`.
    pub fn icon(&self) -> ThemeIcon {
        if self.has_symbol_kind()
            && let Some(symbol_kind) = self.symbol_kind
        {
            return symbol_kind_to_icon(symbol_kind);
        }
        let executing_state_icon = ThemeIcon::from_id("sync");
        if self.is_executing && self.is_paused {
            executing_state_icon
        } else if self.is_executing {
            ThemeIcon::modify(&executing_state_icon, Some(IconModifier::Spin))
        } else if self.cell.cell_kind() == CellKind::Markup {
            ThemeIcon::from_id("markdown")
        } else {
            ThemeIcon::from_id("code")
        }
    }

    /// `addChild(entry)`.
    pub fn add_child(self: &Rc<Self>, entry: Rc<OutlineEntry>) {
        *entry.parent.borrow_mut() = Rc::downgrade(self);
        self.children.borrow_mut().push(entry);
    }

    /// `get parent()`.
    pub fn parent(&self) -> Option<Rc<OutlineEntry>> {
        self.parent.borrow().upgrade()
    }

    /// `get children()`.
    pub fn children(&self) -> Vec<Rc<OutlineEntry>> {
        self.children.borrow().clone()
    }

    /// `get markerInfo()`.
    pub fn marker_info(&self) -> Option<IOutlineMarkerInfo> {
        *self.marker_info.borrow()
    }

    /// `get position()`.
    pub fn position(&self) -> Option<Position> {
        self.range.map(|range| Position { line_number: range.start_line_number, column: range.start_column })
    }

    /// `updateMarkers(markerService)`.
    pub fn update_markers(&self, marker_service: &dyn IMarkerService) {
        if self.cell.cell_kind() == CellKind::Code {
            // a code cell can have marker
            let marker = marker_service.read(Some(&MarkerReadOptions {
                resource: Some(self.cell.uri()),
                severities: Some(MarkerSeverity::ERROR.0 | MarkerSeverity::WARNING.0),
                ..MarkerReadOptions::default()
            }));
            if marker.is_empty() {
                *self.marker_info.borrow_mut() = None;
            } else {
                let top_sev = marker.iter().find(|a| a.severity == MarkerSeverity::ERROR).map(|a| a.severity).unwrap_or(MarkerSeverity::WARNING);
                *self.marker_info.borrow_mut() = Some(IOutlineMarkerInfo { top_sev, count: marker.len() });
            }
        } else {
            // a markdown cell can inherit markers from its children
            let mut top_child: Option<MarkerSeverity> = None;
            for child in self.children() {
                child.update_markers(marker_service);
                if let Some(info) = child.marker_info() {
                    top_child = Some(match top_child {
                        None => info.top_sev,
                        Some(current) => MarkerSeverity(info.top_sev.0.max(current.0)),
                    });
                }
            }
            *self.marker_info.borrow_mut() = top_child.map(|top_sev| IOutlineMarkerInfo { top_sev, count: 0 });
        }
    }

    /// `clearMarkers()`.
    pub fn clear_markers(&self) {
        *self.marker_info.borrow_mut() = None;
        for child in self.children() {
            child.clear_markers();
        }
    }

    /// `find(cell, parents)`.
    pub fn find(self: &Rc<Self>, cell: &CellViewModel, parents: &mut Vec<Rc<OutlineEntry>>) -> Option<Rc<OutlineEntry>> {
        if cell.id() == self.cell.id() {
            return Some(self.clone());
        }
        parents.push(self.clone());
        for child in self.children() {
            if let Some(result) = child.find(cell, parents) {
                return Some(result);
            }
        }
        parents.pop();
        None
    }

    /// `asFlatList(bucket)`.
    pub fn as_flat_list(self: &Rc<Self>, bucket: &mut Vec<Rc<OutlineEntry>>) {
        bucket.push(self.clone());
        for child in self.children() {
            child.as_flat_list(bucket);
        }
    }
}
