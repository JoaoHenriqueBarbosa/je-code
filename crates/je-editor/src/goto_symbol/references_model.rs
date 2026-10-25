//! Porte de `src/vs/editor/contrib/gotoSymbol/browser/referencesModel.ts`: o modelo dos resultados de "ir para
//! referências", agrupados por arquivo ([`ReferencesModel`], [`FileReferences`], [`OneReference`]) e a prévia de uma
//! referência ([`FilePreview`]).
//!
//! Representação:
//! - O original é uma árvore com ciclos (`OneReference.parent`, `FileReferences.parent`, sustentados pelo coletor de
//!   lixo). Aqui o modelo é um `Rc<ReferencesModel>` criado por `Rc::new_cyclic`, os grupos são `Rc<FileReferences>` e as
//!   referências são `Rc<OneReference>`; o caminho de volta (`parent`) é um `Weak`, que as funções `parent` promovem. O
//!   modelo precisa estar vivo enquanto uma de suas referências é usada, como no original (onde o pai mantém o filho vivo
//!   e vice-versa).
//! - `links.sort(...)` ordena a lista de quem chama no lugar e `_links` guarda essa lista já ordenada (que o `clone()`
//!   repassa de novo ao construtor); o porte recebe o `Vec` por valor e guarda a ordenada. `providersFirst === link`
//!   (identidade do primeiro `LocationLink` da ordem original) é o índice original igual a zero, e a ordenação é estável
//!   como a de `Array.prototype.sort`.
//! - `OneReference.range` lê `_range ?? link.targetSelectionRange ?? link.range`; o comparador `_compareReferences`
//!   recebe `(uri, range)` e é chamado com `link.range` (do `LocationLink` que entra) contra `child.range` (da referência
//!   já criada), como o original (que passa o `LocationLink` e o `OneReference` como `Location`).
//! - `readonly _onDidChangeReferenceRange` é privado: o evento sai por [`ReferencesModel::on_did_change_reference_range`].
//! - `clone()` é [`ReferencesModel::clone_model`] (`ReferencesModel` não é `Clone`; o `Rc` seria o clone de ponteiro).
//! - `preview(range, n = 8)` tem `n: Option<i32>`. `ITextEditorModel.textEditorModel` pode ser nulo no original; o
//!   [`ModelReference`] do porte sempre traz o `TextModel`, então `preview` devolve `Some`, mas o tipo segue `Option`.
//! - `IMatch` é o [`Match`] de `je_base::filters` (índices em unidades UTF-16, que é como `before.length` conta), e o
//!   texto da prévia é um [`JsString`] pelo mesmo motivo. `replace(/^\s+/, '')` e `replace(/\s+$/, '')` são
//!   `trim_start` e `trim_end`, que usam o mesmo conjunto de espaços de `\s`.
//! - `extUri` é o `ExtUri` não enviesado do hospedeiro ([`ext_uri`]) e `uri.fsPath` usa a plataforma do hospedeiro.
//! - `async resolve(...)` é um [`async_fn`]; a rejeição de `createModelReference` vai para `onUnexpectedError` e a
//!   iteração segue.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::JsString;
use je_base::async_::async_fn;
use je_base::errors;
use je_base::event::{Emitter, Event};
use je_base::filters::Match;
use je_base::id_generator::default_generator_next_id;
use je_base::js_string::{trim_end, trim_start};
use je_base::lifecycle::IDisposable;
use je_base::map::ResourceMap;
use je_base::nls::localize;
use je_base::promise::Promise;
use je_base::resources::ExtUri;
use je_base::strings;
use je_base::uint::constants::MAX_SAFE_SMALL_INTEGER;
use je_base::uri::URI;
use je_platform::resources::{host_environment, host_platform};

use crate::editor_extensions::{ITextModelService, ModelReference};
use crate::language_features_service::LocationLink;
use crate::position::Position;
use crate::range::Range;

thread_local! {
    static EXT_URI: ExtUri = ExtUri::unbiased(host_environment());
}

/// `extUri` de `resources.ts`: a identidade e a ordem de URIs, tomados como estão.
pub fn ext_uri() -> ExtUri {
    EXT_URI.with(ExtUri::clone)
}

/// `uri.fsPath`.
fn fs_path(uri: &URI) -> String {
    uri.fs_path(host_platform()).to_string_lossy()
}

/// `basename(uri)` de `resources.ts`.
fn basename(uri: &URI) -> String {
    ext_uri().basename(uri, None)
}

/// `OneReference`: uma referência dentro de um arquivo.
pub struct OneReference {
    /// `id`: gerado por `defaultGenerator.nextId()`.
    pub id: String,
    pub is_provider_first: bool,
    parent: Weak<FileReferences>,
    pub link: LocationLink,
    range: Cell<Option<Range>>,
    range_callback: Rc<dyn Fn(&Rc<OneReference>)>,
}

impl OneReference {
    /// `parent`.
    pub fn parent(&self) -> Rc<FileReferences> {
        self.parent.upgrade().expect("o modelo de referências está vivo enquanto uma referência dele é usada")
    }

    /// `uri`.
    pub fn uri(&self) -> &URI {
        &self.link.uri
    }

    /// `get range()`: `_range ?? link.targetSelectionRange ?? link.range`.
    pub fn range(&self) -> Range {
        self.range.get().or(self.link.target_selection_range).unwrap_or(self.link.range)
    }

    /// `set range(value)`: guarda o intervalo e avisa o modelo.
    pub fn set_range(self: &Rc<Self>, value: Range) {
        self.range.set(Some(value));
        (self.range_callback)(self);
    }

    /// `get ariaMessage()`.
    pub fn aria_message(&self) -> String {
        let range = self.range();
        let preview = self.parent().get_preview(self).and_then(|preview| preview.preview(&range, None));
        let name = basename(self.uri());
        let line = range.start_line_number.to_string();
        let column = range.start_column.to_string();

        match preview {
            None => localize("aria.oneReference", "in {0} on line {1} at column {2}", &[name.as_str(), line.as_str(), column.as_str()]),
            Some(preview) => localize(
                "aria.oneReference.preview",
                "{0} in {1} on line {2} at column {3}",
                &[preview.value.to_string_lossy().as_str(), name.as_str(), line.as_str(), column.as_str()],
            ),
        }
    }
}

/// O resultado de [`FilePreview::preview`]: `{ value: string; highlight: IMatch }`.
#[derive(Clone, Debug, PartialEq)]
pub struct ReferencePreview {
    pub value: JsString,
    pub highlight: Match,
}

/// `FilePreview`: guarda o modelo do arquivo para mostrar o trecho de código de uma referência.
pub struct FilePreview {
    model_reference: ModelReference,
}

impl FilePreview {
    pub fn new(model_reference: ModelReference) -> Self {
        Self { model_reference }
    }

    /// `preview(range, n = 8)`.
    pub fn preview(&self, range: &Range, n: Option<i32>) -> Option<ReferencePreview> {
        let n = n.unwrap_or(8);
        let model = &self.model_reference.object;

        let Range { start_line_number, start_column, end_line_number, end_column } = *range;
        let word = model.get_word_until_position(Position::new(start_line_number, start_column - n));
        let before_range = Range::new(start_line_number, word.start_column, start_line_number, start_column);
        let after_range = Range::new(end_line_number, end_column, end_line_number, MAX_SAFE_SMALL_INTEGER);

        let before_text = model.get_value_in_range(&before_range);
        let before = trim_start(before_text.units());
        let inside = model.get_value_in_range(range);
        let after_text = model.get_value_in_range(&after_range);
        let after = trim_end(after_text.units());

        let value = [before, inside.units(), after].concat();
        Some(ReferencePreview { value: JsString::from_units(value), highlight: Match { start: before.len(), end: before.len() + inside.len() } })
    }
}

impl IDisposable for FilePreview {
    fn dispose(&self) {
        self.model_reference.dispose();
    }
}

/// `FileReferences`: as referências de um arquivo.
pub struct FileReferences {
    parent: Weak<ReferencesModel>,
    pub uri: URI,
    children: RefCell<Vec<Rc<OneReference>>>,
    previews: RefCell<ResourceMap<Rc<FilePreview>>>,
}

impl FileReferences {
    /// `parent`.
    pub fn parent(&self) -> Rc<ReferencesModel> {
        self.parent.upgrade().expect("o modelo de referências está vivo enquanto um grupo dele é usado")
    }

    /// `children`.
    pub fn children(&self) -> Vec<Rc<OneReference>> {
        self.children.borrow().clone()
    }

    /// `getPreview(child)`.
    pub fn get_preview(&self, child: &OneReference) -> Option<Rc<FilePreview>> {
        self.previews.borrow().get(child.uri()).cloned()
    }

    /// `get ariaMessage()`.
    pub fn aria_message(&self) -> String {
        let len = self.children.borrow().len();
        let name = basename(&self.uri);
        let path = fs_path(&self.uri);
        if len == 1 {
            localize("aria.fileReferences.1", "1 symbol in {0}, full path {1}", &[name.as_str(), path.as_str()])
        } else {
            localize("aria.fileReferences.N", "{0} symbols in {1}, full path {2}", &[len.to_string().as_str(), name.as_str(), path.as_str()])
        }
    }

    /// `resolve(textModelResolverService)`: abre o modelo de cada arquivo para as prévias.
    pub fn resolve(self: &Rc<Self>, text_model_resolver_service: &Rc<dyn ITextModelService>) -> Promise<Rc<FileReferences>> {
        let this = Rc::clone(self);
        let service = Rc::clone(text_model_resolver_service);
        async_fn(async move {
            if this.previews.borrow().size() != 0 {
                return Ok(this);
            }
            for child in this.children() {
                if this.previews.borrow().has(child.uri()) {
                    continue;
                }
                match service.create_model_reference(child.uri()).await {
                    Ok(reference) => {
                        this.previews.borrow_mut().set(child.uri().clone(), Rc::new(FilePreview::new(reference)));
                    }
                    Err(err) => errors::on_unexpected_error(&err),
                }
            }
            Ok(this)
        })
    }
}

impl IDisposable for FileReferences {
    fn dispose(&self) {
        let previews: Vec<Rc<FilePreview>> = self.previews.borrow().values().cloned().collect();
        for preview in &previews {
            preview.dispose();
        }
        self.previews.borrow_mut().clear();
    }
}

/// `ReferencesModel`: os resultados agrupados por arquivo, ordenados por URI e por posição.
pub struct ReferencesModel {
    links: Vec<LocationLink>,
    title: String,
    groups: RefCell<Vec<Rc<FileReferences>>>,
    references: Vec<Rc<OneReference>>,
    on_did_change_reference_range: Rc<Emitter<Rc<OneReference>>>,
}

impl ReferencesModel {
    pub fn new(links: Vec<LocationLink>, title: impl Into<String>) -> Rc<Self> {
        let ext_uri = ext_uri();

        // grouping and sorting
        let mut indexed: Vec<(usize, LocationLink)> = links.into_iter().enumerate().collect();
        indexed.sort_by(|(_, a), (_, b)| Self::compare_references(&ext_uri, (&a.uri, &a.range), (&b.uri, &b.range)).cmp(&0));
        let sorted_links: Vec<LocationLink> = indexed.iter().map(|(_, link)| link.clone()).collect();

        let on_did_change_reference_range: Rc<Emitter<Rc<OneReference>>> = Emitter::new();

        Rc::new_cyclic(|me: &Weak<ReferencesModel>| {
            let mut groups: Vec<Rc<FileReferences>> = Vec::new();
            let mut references: Vec<Rc<OneReference>> = Vec::new();

            let mut current: Option<Rc<FileReferences>> = None;
            for (original_index, link) in indexed {
                let group = match &current {
                    Some(group) if ext_uri.is_equal(Some(&group.uri), Some(&link.uri), true) => Rc::clone(group),
                    _ => {
                        // new group
                        let group = Rc::new(FileReferences {
                            parent: me.clone(),
                            uri: link.uri.clone(),
                            children: RefCell::new(Vec::new()),
                            previews: RefCell::new(ResourceMap::new()),
                        });
                        groups.push(Rc::clone(&group));
                        group
                    }
                };
                current = Some(Rc::clone(&group));

                // append, check for equality first!
                let is_new = match group.children.borrow().last() {
                    None => true,
                    Some(last) => Self::compare_references(&ext_uri, (&link.uri, &link.range), (last.uri(), &last.range())) != 0,
                };
                if is_new {
                    let emitter = Rc::clone(&on_did_change_reference_range);
                    let one_ref = Rc::new(OneReference {
                        id: default_generator_next_id(),
                        is_provider_first: original_index == 0,
                        parent: Rc::downgrade(&group),
                        link,
                        range: Cell::new(None),
                        range_callback: Rc::new(move |reference: &Rc<OneReference>| emitter.fire(Rc::clone(reference))),
                    });
                    references.push(Rc::clone(&one_ref));
                    group.children.borrow_mut().push(one_ref);
                }
            }

            Self { links: sorted_links, title: title.into(), groups: RefCell::new(groups), references, on_did_change_reference_range }
        })
    }

    /// `onDidChangeReferenceRange`.
    pub fn on_did_change_reference_range(&self) -> Event<Rc<OneReference>> {
        self.on_did_change_reference_range.event()
    }

    /// `groups`.
    pub fn groups(&self) -> Vec<Rc<FileReferences>> {
        self.groups.borrow().clone()
    }

    /// `references`.
    pub fn references(&self) -> &[Rc<OneReference>] {
        &self.references
    }

    /// `clone()`.
    pub fn clone_model(&self) -> Rc<ReferencesModel> {
        ReferencesModel::new(self.links.clone(), self.title.clone())
    }

    /// `get title()`.
    pub fn title(&self) -> &str {
        &self.title
    }

    /// `get isEmpty()`.
    pub fn is_empty(&self) -> bool {
        self.groups.borrow().is_empty()
    }

    /// `get ariaMessage()`.
    pub fn aria_message(&self) -> String {
        let groups = self.groups.borrow();
        if groups.is_empty() {
            localize("aria.result.0", "No results found", &[])
        } else if self.references.len() == 1 {
            localize("aria.result.1", "Found 1 symbol in {0}", &[fs_path(self.references[0].uri()).as_str()])
        } else if groups.len() == 1 {
            localize("aria.result.n1", "Found {0} symbols in {1}", &[self.references.len().to_string().as_str(), fs_path(&groups[0].uri).as_str()])
        } else {
            localize("aria.result.nm", "Found {0} symbols in {1} files", &[self.references.len().to_string().as_str(), groups.len().to_string().as_str()])
        }
    }

    /// `nextOrPreviousReference(reference, next)`.
    pub fn next_or_previous_reference(&self, reference: &Rc<OneReference>, next: bool) -> Rc<OneReference> {
        let parent = reference.parent();
        let children = parent.children();
        let groups = parent.parent().groups();

        let mut idx = children.iter().position(|child| Rc::ptr_eq(child, reference)).map_or(-1, |i| i as isize);
        let child_count = children.len() as isize;
        let group_count = groups.len() as isize;

        if group_count == 1 || next && idx + 1 < child_count || !next && idx > 0 {
            // cycling within one file
            if next {
                idx = (idx + 1) % child_count;
            } else {
                idx = (idx + child_count - 1) % child_count;
            }
            return Rc::clone(&children[idx as usize]);
        }

        let mut idx = groups.iter().position(|group| Rc::ptr_eq(group, &parent)).map_or(-1, |i| i as isize);
        if next {
            idx = (idx + 1) % group_count;
            Rc::clone(&groups[idx as usize].children()[0])
        } else {
            idx = (idx + group_count - 1) % group_count;
            let siblings = groups[idx as usize].children();
            Rc::clone(&siblings[siblings.len() - 1])
        }
    }

    /// `nearestReference(resource, position)`: o maior prefixo comum do URI e, no empate, a menor distância.
    pub fn nearest_reference(&self, resource: &URI, position: &Position) -> Option<Rc<OneReference>> {
        let resource_key = resource.to_string(false);
        self.references
            .iter()
            .map(|reference| {
                let range = reference.range();
                let prefix_len = strings::common_prefix_length(reference.uri().to_string(false).units(), resource_key.units());
                let offset_dist = i64::from((range.start_line_number - position.line_number).abs()) * 100
                    + i64::from((range.start_column - position.column).abs());
                (reference, prefix_len, offset_dist)
            })
            .min_by(|a, b| b.1.cmp(&a.1).then(a.2.cmp(&b.2)))
            .map(|(reference, _, _)| Rc::clone(reference))
    }

    /// `referenceAt(resource, position)`.
    pub fn reference_at(&self, resource: &URI, position: &Position) -> Option<Rc<OneReference>> {
        let resource_key = resource.to_string(false);
        self.references
            .iter()
            .find(|reference| reference.uri().to_string(false) == resource_key && reference.range().contains_position(position))
            .cloned()
    }

    /// `firstReference()`.
    pub fn first_reference(&self) -> Option<Rc<OneReference>> {
        self.references.iter().find(|reference| reference.is_provider_first).or_else(|| self.references.first()).cloned()
    }

    /// `_compareReferences(a, b)`: `extUri.compare(a.uri, b.uri) || Range.compareRangesUsingStarts(a.range, b.range)`.
    fn compare_references(ext_uri: &ExtUri, a: (&URI, &Range), b: (&URI, &Range)) -> i32 {
        match ext_uri.compare(a.0, b.0, false) {
            0 => Range::compare_ranges_using_starts(Some(a.1), Some(b.1)),
            by_uri => by_uri,
        }
    }
}

impl IDisposable for ReferencesModel {
    fn dispose(&self) {
        let groups = self.groups();
        for group in &groups {
            group.dispose();
        }
        self.on_did_change_reference_range.dispose();
        self.groups.borrow_mut().clear();
    }
}
