//! Porte de `src/vs/base/browser/ui/iconLabel/iconLabel.ts` e `iconlabel.css`: o [`IconLabel`], o rótulo de
//! um item de lista, de árvore ou de menu: nome (com realces opcionais), sufixo e descrição, com um ícone
//! à esquerda e o `title` como hover. `iconLabels.ts` está em [`crate::icon_labels`] e `simpleIconLabel.ts`
//! em [`crate::simple_icon_label`].
//!
//! Como o DOM e o CSS viram `je-ui`:
//!
//! - A estrutura é a do original, com as mesmas classes: `.monaco-icon-label` > `.monaco-icon-label-container` >
//!   (`span.monaco-icon-name-container` > `a.label-name`, `span.monaco-icon-suffix-container` > `span.label-suffix`,
//!   `span.monaco-icon-description-container` > `span.label-description`), e o `.monaco-icon-label-iconpath`
//!   antes de tudo. Cada texto é um nó filho com [`je_ui::Content::Text`] (ver [`crate::icon_labels`]).
//! - O fluxo em linha do contêiner (`display: flex` e `text-overflow: ellipsis`) é [`IconLabel::relayout`]:
//!   os pedaços entram da esquerda para a direita, a descrição com `margin-left: 0.5em` e `font-size: 0.9em`,
//!   e o que passa da largura é cortado com `…`, primeiro a descrição. A largura é a de [`IconLabel::layout`]
//!   (`None`: o rótulo cresce até o texto, como um `display: flex` solto).
//! - As regras de `iconlabel.css` viram estilo: `opacity` de .7 (sufixo e descrição) e .5 (separador), `bold` em
//!   peso 700 nos três textos, `strikethrough` numa linha de 1 px, `.disabled` na cor `disabledForeground`.
//!   [`computed_text_state`] lê as classes e devolve o estado que o navegador devolveria como estilo
//!   computado (é o que o teste do original consulta).
//! - O `title` vira um `IManagedHover` do delegado de hover da camada de base ([`crate::hover_api`]).
//! - **Cor herdada.** Com [`HighlightedLabelStyles::inherit_foreground`] em [`IconLabelStyles::text`], o nome, o sufixo, a
//!   descrição e o ícone não fixam a cor: herdam a do antepassado (o `.reference-file` do peek, a linha da lista), como o
//!   CSS. O `.disabled` declara a cor `disabledForeground` e por isso não herda.
//!
//! Fica de fora: o `::before` do ícone de arquivo (a classe de tema de ícones que o dono põe em
//! `extraClasses`: a pintura do ícone de arquivo é do serviço de temas de ícone, TODO(integração)), a imagem
//! de um `iconPath` que é URI (o `je-ui` não tem conteúdo de imagem, TODO(integração)), o itálico (o `je-text`
//! não tem, ver [`crate::icon_labels`]) e o `equals` das opções que o original usa para pular um `setLabel`
//! idêntico (aqui o rótulo sempre é refeito, com o mesmo resultado). O `domId` (o atributo `id` dos
//! `a.label-name`, para o `aria-activedescendant`) também fica de fora: a árvore do `je-ui` não tem `id`.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use je_base::errors::{self, Error};
use je_base::filters::Match;
use je_base::lifecycle::{Disposable, DisposableStore, IDisposable};
use je_base::themables::ThemeIcon;
use je_base::uri::URI;
use je_render::Rgba;
use je_render::kurbo::Rect;
use je_theme::ColorTheme;
use je_ui::{Content, Foreground, NodeId, Role, Tree, Ui};

use crate::dom::{clear_node, create_el, is_ancestor};
use crate::highlighted_label::{HighlightedLabel, HighlightedLabelStyles, IHighlight, default_highlighted_label_styles};
use crate::hover_api::{HoverPlacement, IHoverDelegate, IManagedHover, ManagedHoverContent, ManagedHoverContentOrFactory, get_base_layer_hover_delegate, get_default_hover_delegate};
use crate::icon_labels::{InlineStyle, LabelPart, Rendered, build_parts_with, layout_parts, resize};
use crate::text::measure_text;

/// A largura e a altura do `::before` e do `.monaco-icon-label-iconpath` (`width: 16px; height: 22px`) e o
/// `padding-right`/`margin-right` de 6 px depois dele.
const ICON_WIDTH: f64 = 16.0;
const ICON_HEIGHT: f64 = 22.0;
const ICON_GAP: f64 = 6.0;
/// `margin: 0 2px` do `.label-separator`: as duas margens somadas.
const SEPARATOR_MARGIN: f64 = 4.0;

/// O `IIconLabelCreationOptions`.
#[derive(Clone, Default)]
pub struct IIconLabelCreationOptions {
    pub support_highlights: bool,
    pub support_description_highlights: bool,
    pub support_icons: bool,
    pub hover_delegate: Option<Rc<dyn IHoverDelegate>>,
    pub hover_target_override: Option<NodeId>,
}

/// `string | IManagedHoverTooltipMarkdownString`: o `title` e o `descriptionTitle`.
#[derive(Clone)]
pub enum IconLabelTitle {
    Text(String),
    Markdown(ManagedHoverContent),
}

impl IconLabelTitle {
    /// O valor é "verdadeiro" no JavaScript: texto não vazio, ou um markdown.
    fn is_truthy(&self) -> bool {
        !matches!(self, Self::Text(text) if text.is_empty())
    }

    fn to_content(&self) -> ManagedHoverContentOrFactory {
        ManagedHoverContentOrFactory::Content(match self {
            Self::Text(text) => ManagedHoverContent::Text(text.clone()),
            Self::Markdown(content) => content.clone(),
        })
    }
}

impl From<&str> for IconLabelTitle {
    fn from(text: &str) -> Self {
        Self::Text(text.to_owned())
    }
}

/// `URI | ThemeIcon`.
#[derive(Clone)]
pub enum IconPath {
    Uri(URI),
    ThemeIcon(ThemeIcon),
}

/// O `IIconLabelValueOptions`.
#[derive(Clone, Default)]
pub struct IIconLabelValueOptions {
    pub title: Option<IconLabelTitle>,
    pub description_title: Option<IconLabelTitle>,
    pub suffix: Option<String>,
    pub hide_icon: bool,
    pub extra_classes: Vec<String>,
    pub bold: bool,
    pub italic: bool,
    pub strikethrough: bool,
    pub matches: Option<Vec<Match>>,
    pub label_escape_new_lines: bool,
    pub description_matches: Option<Vec<Match>>,
    pub disabled_command: bool,
    pub separator: Option<String>,
    pub dom_id: Option<String>,
    pub icon_path: Option<IconPath>,
    pub support_icons: Option<bool>,
}

/// `string | string[]`: o nome do rótulo.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IconLabelText {
    Single(String),
    Multiple(Vec<String>),
}

impl From<&str> for IconLabelText {
    fn from(text: &str) -> Self {
        Self::Single(text.to_owned())
    }
}

impl From<Vec<String>> for IconLabelText {
    fn from(labels: Vec<String>) -> Self {
        Self::Multiple(labels)
    }
}

/// O estilo de um [`IconLabel`]: o do texto, mais a cor do `.monaco-icon-label-container.disabled`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IconLabelStyles {
    pub text: HighlightedLabelStyles,
    pub disabled_foreground: Option<Rgba>,
}

/// O estilo de uma linha de lista: texto em `foreground`, realce em `list.highlightForeground`, linha de
/// 22 px (`--vscode-list-row-height`) e `disabledForeground`.
pub fn default_icon_label_styles(theme: &ColorTheme) -> IconLabelStyles {
    IconLabelStyles { text: HighlightedLabelStyles { line_height: Some(ICON_HEIGHT), ..default_highlighted_label_styles(theme) }, disabled_foreground: theme.get_color("disabledForeground") }
}

/// O estado de texto de um nó: o que as regras `.bold`, `.italic` e `.strikethrough` do `iconlabel.css`
/// decidem para `font-weight >= 600`, `font-style: italic` e `text-decoration-line: line-through`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextState {
    pub italic: bool,
    pub strikethrough: bool,
    pub bold: bool,
}

/// O estilo computado de um dos três textos (`.label-name`, `.label-suffix`, `.label-description`): as
/// classes do `.monaco-icon-label` antepassado, como os seletores de `iconlabel.css`.
pub fn computed_text_state(tree: &Tree, node: NodeId) -> TextState {
    let mut current = Some(node);
    while let Some(n) = current {
        if tree.has_class(n, "monaco-icon-label") {
            return TextState { italic: tree.has_class(n, "italic"), strikethrough: tree.has_class(n, "strikethrough") || tree.has_class(n, "deprecated"), bold: tree.has_class(n, "bold") };
        }
        current = tree.parent(n);
    }
    TextState { italic: false, strikethrough: false, bold: false }
}

/// Um nó cujo conteúdo é um texto só (o `textContent` de um `FastLabelNode`).
struct TextSlot {
    holder: NodeId,
    rendered: Vec<Rendered>,
}

impl TextSlot {
    fn new(holder: NodeId) -> Self {
        Self { holder, rendered: Vec::new() }
    }

    fn set(&mut self, tree: &mut Tree, text: &str, style: &InlineStyle, inherit: bool) {
        clear_node(tree, self.holder);
        self.rendered = if text.is_empty() { Vec::new() } else { build_parts_with(tree, self.holder, &[LabelPart::Text(text.to_owned())], style, inherit) };
    }

    /// A largura do texto inteiro, sem corte.
    fn natural_width(&self) -> f64 {
        self.rendered
            .iter()
            .map(|r| match r {
                Rendered::Run { text, style, .. } => measure_text(text, &style.text_style()),
                _ => 0.0,
            })
            .sum()
    }

    /// Posiciona o texto e dá ao nó o tamanho dele.
    fn layout(&self, tree: &mut Tree, line_height: f64, max_width: f64) -> f64 {
        let width = layout_parts(tree, &self.rendered, line_height, Some(max_width.max(0.0)));
        resize(tree, self.holder, width, line_height);
        width
    }
}

enum SlotContent {
    Plain(TextSlot),
    Highlighted(Rc<HighlightedLabel>),
}

/// Um `a.label-name`.
struct NameSlot {
    anchor: NodeId,
    content: SlotContent,
    /// O `span.label-separator` que vem depois (dentro do `a` com realces, ao lado dele sem).
    separator: Option<(NodeId, TextSlot)>,
}

#[derive(Default)]
struct NameNode {
    multiple: bool,
    slots: Vec<NameSlot>,
}

enum DescriptionNode {
    Plain(TextSlot, NodeId),
    Highlighted(Rc<HighlightedLabel>, NodeId),
}

pub struct IconLabel {
    base: Rc<Disposable>,
    ui: Ui,
    creation_options: IIconLabelCreationOptions,
    styles: Cell<IconLabelStyles>,
    dom_node: NodeId,
    label_container: NodeId,
    name_container: NodeId,
    name: RefCell<NameNode>,
    name_disposables: Rc<DisposableStore>,
    description: RefCell<Option<DescriptionNode>>,
    suffix: RefCell<Option<(NodeId, TextSlot)>>,
    icon_path_node: Cell<Option<NodeId>>,
    hover_delegate: Rc<dyn IHoverDelegate>,
    custom_hovers: RefCell<HashMap<NodeId, Rc<dyn IManagedHover>>>,
    /// A largura do `.monaco-icon-label`; `None` cresce até o conteúdo.
    width: Cell<Option<f64>>,
    state: Cell<TextState>,
    disabled: Cell<bool>,
    disposed: Cell<bool>,
}

impl IconLabel {
    /// Cria o rótulo como último filho de `container`.
    pub fn new(ui: &Ui, container: NodeId, options: Option<IIconLabelCreationOptions>, styles: IconLabelStyles) -> Rc<Self> {
        let creation_options = options.unwrap_or_default();
        let (dom_node, label_container, name_container) = {
            let mut tree = ui.tree_mut();
            let dom_node = create_el(&mut tree, container, Role::Generic, "monaco-icon-label");
            tree.style_mut(dom_node).clip = true;
            let label_container = create_el(&mut tree, dom_node, Role::Generic, "monaco-icon-label-container");
            let name_container = create_el(&mut tree, label_container, Role::Generic, "monaco-icon-name-container");
            (dom_node, label_container, name_container)
        };
        let hover_delegate = creation_options.hover_delegate.clone().unwrap_or_else(|| get_default_hover_delegate(HoverPlacement::Mouse));
        let base = Disposable::new();
        let name_disposables = DisposableStore::new();
        base.register(name_disposables.clone());
        Rc::new(Self {
            base,
            ui: ui.clone(),
            creation_options,
            styles: Cell::new(styles),
            dom_node,
            label_container,
            name_container,
            name: RefCell::new(NameNode::default()),
            name_disposables,
            description: RefCell::new(None),
            suffix: RefCell::new(None),
            icon_path_node: Cell::new(None),
            hover_delegate,
            custom_hovers: RefCell::new(HashMap::new()),
            width: Cell::new(None),
            state: Cell::new(TextState { italic: false, strikethrough: false, bold: false }),
            disabled: Cell::new(false),
            disposed: Cell::new(false),
        })
    }

    /// O elemento `.monaco-icon-label`.
    pub fn element(&self) -> NodeId {
        self.dom_node
    }

    /// A largura do conteúdo, depois do último layout.
    pub fn content_width(&self) -> f64 {
        let tree = self.ui.tree();
        let r = tree.bounds(self.dom_node);
        r.width()
    }

    /// A largura disponível do rótulo (o `flex: 1` do contêiner dentro dele); `None` cresce até o texto.
    pub fn layout(&self, width: Option<f64>) {
        self.width.set(width);
        self.relayout();
    }

    /// Troca o estilo (e refaz o rótulo).
    pub fn set_styles(&self, styles: IconLabelStyles) {
        if styles != self.styles.get() {
            self.styles.set(styles);
            self.refresh_text_styles();
            self.relayout();
        }
    }

    fn name_styles(&self) -> HighlightedLabelStyles {
        let (styles, state) = (self.styles.get(), self.state.get());
        // `.monaco-icon-label-container.disabled` declara a própria cor: o rótulo desabilitado não herda a do antepassado.
        let disabled_foreground = if self.disabled.get() { styles.disabled_foreground } else { None };
        let foreground = disabled_foreground.unwrap_or(styles.text.foreground);
        HighlightedLabelStyles {
            foreground,
            font_weight: if state.bold { 700 } else { styles.text.font_weight },
            italic: state.italic,
            strikethrough: state.strikethrough,
            inherit_foreground: styles.text.inherit_foreground && disabled_foreground.is_none(),
            ..styles.text
        }
    }

    /// O texto da descrição: 0.9em do tamanho do nome.
    fn description_styles(&self) -> HighlightedLabelStyles {
        let name = self.name_styles();
        HighlightedLabelStyles { font_size: name.font_size * 0.9, ..name }
    }

    fn inline_of(styles: &HighlightedLabelStyles) -> InlineStyle {
        styles.inline_style()
    }

    /// Reaplica as regras de estado (`bold`, `italic`, `strikethrough`, `disabled`) aos três textos.
    fn refresh_text_styles(&self) {
        let (name_styles, description_styles) = (self.name_styles(), self.description_styles());
        for slot in &self.name.borrow().slots {
            if let SlotContent::Highlighted(label) = &slot.content {
                label.set_styles(name_styles);
            }
        }
        if let Some(DescriptionNode::Highlighted(label, _)) = &*self.description.borrow() {
            label.set_styles(description_styles);
        }
    }

    /// `setLabel(label, description, options)`.
    pub fn set_label(&self, label: IconLabelText, description: Option<&str>, options: Option<&IIconLabelValueOptions>) {
        let default_options = IIconLabelValueOptions::default();
        let has_options = options.is_some();
        let opts = options.unwrap_or(&default_options);
        let mut label_classes = vec!["monaco-icon-label".to_owned()];
        let mut container_classes = vec!["monaco-icon-label-container".to_owned()];
        let mut aria_label = String::new();
        if has_options {
            label_classes.extend(opts.extra_classes.iter().cloned());
            for (on, class) in [(opts.bold, "bold"), (opts.italic, "italic"), (opts.strikethrough, "strikethrough")] {
                if on {
                    label_classes.push(class.to_owned());
                }
            }
            if opts.disabled_command {
                container_classes.push("disabled".to_owned());
            }
            if let Some(title) = opts.title.as_ref().filter(|t| t.is_truthy()) {
                match title {
                    IconLabelTitle::Text(text) => aria_label.push_str(text),
                    IconLabelTitle::Markdown(_) => aria_label.push_str(&match &label {
                        IconLabelText::Single(l) => l.clone(),
                        IconLabelText::Multiple(l) => l.join(","),
                    }),
                }
            }
        }

        self.update_icon_path(opts.icon_path.as_ref());

        {
            let mut tree = self.ui.tree_mut();
            tree.set_class_name(self.dom_node, &label_classes.join(" "));
            tree.set_label(self.dom_node, aria_label);
            tree.set_class_name(self.label_container, &container_classes.join(" "));
        }
        self.state.set(TextState { italic: opts.italic, strikethrough: opts.strikethrough, bold: opts.bold });
        self.disabled.set(opts.disabled_command);
        self.setup_hover(if opts.description_title.is_some() { self.label_container } else { self.dom_node }, opts.title.as_ref());

        let (name_styles, description_styles) = (self.name_styles(), self.description_styles());
        self.set_name(&label, opts, &name_styles);

        let has_description = description.is_some_and(|d| !d.is_empty());
        if has_description || self.description.borrow().is_some() {
            self.get_or_create_description_node();
            let text = description.unwrap_or_default();
            let mut slot = self.description.borrow_mut();
            match slot.as_mut().expect("descrição criada") {
                DescriptionNode::Highlighted(label, holder) => {
                    let support_icons = opts.support_icons.unwrap_or(self.creation_options.support_icons);
                    label.set_styles(description_styles);
                    let highlights: Vec<IHighlight> = opts.description_matches.iter().flatten().map(|m| IHighlight::new(m.start, m.end)).collect();
                    label.set(Some(text), &highlights, "", opts.label_escape_new_lines, support_icons);
                    let holder = *holder;
                    drop(slot);
                    self.setup_hover(holder, opts.description_title.as_ref());
                }
                DescriptionNode::Plain(text_slot, holder) => {
                    let shown = if has_description && opts.label_escape_new_lines { HighlightedLabel::escape_new_lines(text, &mut []) } else { text.to_owned() };
                    text_slot.set(&mut self.ui.tree_mut(), &shown, &Self::inline_of(&description_styles), description_styles.inherit_foreground);
                    let holder = *holder;
                    drop(slot);
                    self.setup_hover(holder, Some(&opts.description_title.clone().unwrap_or(IconLabelTitle::Text(String::new()))));
                }
            }
        }

        if opts.suffix.as_deref().is_some_and(|s| !s.is_empty()) || self.suffix.borrow().is_some() {
            self.get_or_create_suffix_node();
            let mut suffix = self.suffix.borrow_mut();
            let (_, slot) = suffix.as_mut().expect("sufixo criado");
            slot.set(&mut self.ui.tree_mut(), opts.suffix.as_deref().unwrap_or_default(), &Self::inline_of(&name_styles), name_styles.inherit_foreground);
        }

        self.relayout();
    }

    /// O `.monaco-icon-label-iconpath`, antes de tudo no `.monaco-icon-label`.
    fn update_icon_path(&self, icon_path: Option<&IconPath>) {
        let mut tree = self.ui.tree_mut();
        let Some(icon_path) = icon_path else {
            if let Some(node) = self.icon_path_node.take() {
                tree.remove(node);
            }
            return;
        };
        let node = match self.icon_path_node.get() {
            Some(node) => node,
            None => {
                let node = tree.create_detached(Role::Generic);
                let first = tree.children(self.dom_node).first().copied();
                tree.insert_before(self.dom_node, node, first);
                self.icon_path_node.set(Some(node));
                node
            }
        };
        let styles = self.styles.get();
        let color = styles.text.foreground;
        let foreground = if styles.text.inherit_foreground { Foreground::Inherit } else { Foreground::Content };
        tree.style_mut(node).foreground = foreground;
        match icon_path {
            IconPath::ThemeIcon(icon) => {
                tree.set_class_name(node, &format!("monaco-icon-label-iconpath {}", icon.as_class_name()));
                tree.style_mut(node).content = Content::Codicon { id: icon.id.split('~').next().unwrap_or_default().to_owned(), size: 16.0, color };
            }
            // TODO(integração): pintar a imagem do URI (`background-image` com `contain`) quando o `je-ui` tiver
            // conteúdo de imagem; por ora o nó fica reservado, vazio.
            IconPath::Uri(_) => {
                tree.set_class_name(node, "monaco-icon-label-iconpath");
                tree.style_mut(node).content = Content::None;
            }
        }
    }

    fn setup_hover(&self, html_element: NodeId, tooltip: Option<&IconLabelTitle>) {
        let previous = self.custom_hovers.borrow_mut().remove(&html_element);
        if let Some(previous) = previous {
            previous.dispose();
        }

        let Some(tooltip) = tooltip.filter(|t| t.is_truthy()) else {
            return;
        };

        let mut hover_target = html_element;
        if let Some(target_override) = self.creation_options.hover_target_override {
            if !is_ancestor(&self.ui, html_element, target_override) {
                errors::throw(Error::new("hoverTargetOverrride must be an ancestor of the htmlElement"));
            }
            hover_target = target_override;
        }

        let hover = get_base_layer_hover_delegate().setup_managed_hover(self.hover_delegate.clone(), hover_target, tooltip.to_content(), None);
        self.custom_hovers.borrow_mut().insert(html_element, hover);
    }

    fn get_or_create_suffix_node(&self) {
        if self.suffix.borrow().is_some() {
            return;
        }
        let mut tree = self.ui.tree_mut();
        let container = tree.create_detached(Role::Generic);
        tree.set_class_name(container, "monaco-icon-suffix-container");
        let next = tree.children(self.label_container).iter().position(|c| *c == self.name_container).and_then(|i| tree.children(self.label_container).get(i + 1).copied());
        tree.insert_before(self.label_container, container, next);
        let holder = create_el(&mut tree, container, Role::Generic, "label-suffix");
        tree.style_mut(holder).opacity = 0.7;
        *self.suffix.borrow_mut() = Some((container, TextSlot::new(holder)));
    }

    fn get_or_create_description_node(&self) {
        if self.description.borrow().is_some() {
            return;
        }
        let mut tree = self.ui.tree_mut();
        let container = create_el(&mut tree, self.label_container, Role::Generic, "monaco-icon-description-container");
        let holder = create_el(&mut tree, container, Role::Generic, "label-description");
        tree.style_mut(holder).opacity = 0.7;
        let node = if self.creation_options.support_description_highlights {
            drop(tree);
            let label = HighlightedLabel::new(&self.ui, holder, None, self.description_styles());
            self.base.register(je_base::lifecycle::to_disposable({
                let label = label.clone();
                move || label.dispose()
            }));
            DescriptionNode::Highlighted(label, holder)
        } else {
            DescriptionNode::Plain(TextSlot::new(holder), holder)
        };
        *self.description.borrow_mut() = Some(node);
    }

    /// `Label.setLabel` e `LabelWithHighlights.setLabel`.
    fn set_name(&self, label: &IconLabelText, opts: &IIconLabelValueOptions, name_styles: &HighlightedLabelStyles) {
        let with_highlights = self.creation_options.support_highlights || self.creation_options.support_icons;
        let support_icons = opts.support_icons.unwrap_or(self.creation_options.support_icons);
        let mut name = self.name.borrow_mut();
        let make_label = |anchor: NodeId| {
            let hl = HighlightedLabel::new(&self.ui, anchor, None, *name_styles);
            self.name_disposables.add_rc(hl.clone());
            hl
        };
        match label {
            IconLabelText::Single(text) => {
                if name.multiple || name.slots.is_empty() {
                    self.name_disposables.clear();
                    let mut tree = self.ui.tree_mut();
                    clear_node(&mut tree, self.name_container);
                    tree.remove_class(self.name_container, "multiple");
                    let anchor = create_el(&mut tree, self.name_container, Role::Generic, "label-name");
                    drop(tree);
                    let content = if with_highlights { SlotContent::Highlighted(make_label(anchor)) } else { SlotContent::Plain(TextSlot::new(anchor)) };
                    *name = NameNode { multiple: false, slots: vec![NameSlot { anchor, content, separator: None }] };
                }
                let slot = &mut name.slots[0];
                match &mut slot.content {
                    SlotContent::Highlighted(hl) => {
                        let highlights: Vec<IHighlight> = opts.matches.iter().flatten().map(|m| IHighlight::new(m.start, m.end)).collect();
                        hl.set_styles(*name_styles);
                        hl.set(Some(text), &highlights, "", opts.label_escape_new_lines, support_icons);
                    }
                    SlotContent::Plain(text_slot) => text_slot.set(&mut self.ui.tree_mut(), text, &Self::inline_of(name_styles), name_styles.inherit_foreground),
                }
            }
            IconLabelText::Multiple(labels) => {
                self.name_disposables.clear();
                let separator = opts.separator.clone().filter(|s| !s.is_empty()).unwrap_or_else(|| "/".to_owned());
                let matches = split_matches(labels, &separator, opts.matches.as_deref());
                let mut slots = Vec::new();
                {
                    let mut tree = self.ui.tree_mut();
                    clear_node(&mut tree, self.name_container);
                    tree.add_class(self.name_container, "multiple");
                }
                for (i, l) in labels.iter().enumerate() {
                    let anchor = create_el(&mut self.ui.tree_mut(), self.name_container, Role::TreeItem, "label-name");
                    let is_last = i + 1 == labels.len();
                    let content = if with_highlights {
                        let hl = make_label(anchor);
                        let highlights: Vec<IHighlight> = matches.as_ref().map(|m| m[i].iter().map(|m| IHighlight::new(m.start, m.end)).collect()).unwrap_or_default();
                        hl.set(Some(l), &highlights, "", opts.label_escape_new_lines, support_icons);
                        SlotContent::Highlighted(hl)
                    } else {
                        let mut slot = TextSlot::new(anchor);
                        slot.set(&mut self.ui.tree_mut(), l, &Self::inline_of(name_styles), name_styles.inherit_foreground);
                        SlotContent::Plain(slot)
                    };
                    let separator_node = (!is_last).then(|| {
                        let mut tree = self.ui.tree_mut();
                        // Com realces o separador fica dentro do `a`; sem, ao lado dele.
                        let parent = if with_highlights { anchor } else { self.name_container };
                        let node = create_el(&mut tree, parent, Role::Generic, "label-separator");
                        tree.style_mut(node).opacity = 0.5;
                        let mut slot = TextSlot::new(node);
                        slot.set(&mut tree, &separator, &Self::inline_of(name_styles), name_styles.inherit_foreground);
                        (node, slot)
                    });
                    slots.push(NameSlot { anchor, content, separator: separator_node });
                }
                *name = NameNode { multiple: true, slots };
            }
        }
    }

    /// O fluxo em linha do contêiner: posiciona nome, sufixo e descrição e corta com `…` o que passa da
    /// largura (primeiro a descrição, que é a última).
    pub fn relayout(&self) {
        if self.disposed.get() {
            return;
        }
        let styles = self.styles.get();
        let line_height = styles.text.resolved_line_height();
        let has_icon = self.icon_path_node.get().is_some();
        let icon_extent = if has_icon { ICON_WIDTH + ICON_GAP } else { 0.0 };
        let available = self.width.get().map(|w| (w - icon_extent).max(0.0));
        let mut remaining = available.unwrap_or(f64::INFINITY);

        // O nome: cada `a.label-name` (e o separador depois dele, com `margin: 0 2px`).
        let mut x = 0.0;
        for slot in &self.name.borrow().slots {
            let separator = slot.separator.as_ref().map_or(0.0, |(_, s)| s.natural_width() + SEPARATOR_MARGIN);
            let budget = (remaining - separator).max(0.0);
            let width = match &slot.content {
                SlotContent::Highlighted(hl) => {
                    hl.set_max_width(Some(budget));
                    hl.width()
                }
                SlotContent::Plain(text) => text.layout(&mut self.ui.tree_mut(), line_height, budget),
            };
            let mut tree = self.ui.tree_mut();
            let mut anchor_width = width;
            if let Some((node, text)) = &slot.separator {
                let sep = text.layout(&mut tree, line_height, f64::INFINITY);
                match slot.content {
                    // Com realces o separador fica dentro do `a`, depois do texto.
                    SlotContent::Highlighted(_) => {
                        tree.set_bounds(*node, Rect::new(width + SEPARATOR_MARGIN / 2.0, 0.0, width + SEPARATOR_MARGIN / 2.0 + sep, line_height));
                        anchor_width = width + sep + SEPARATOR_MARGIN;
                    }
                    SlotContent::Plain(_) => {
                        let left = x + width + SEPARATOR_MARGIN / 2.0;
                        tree.set_bounds(*node, Rect::new(left, 0.0, left + sep, line_height));
                    }
                }
            }
            tree.set_bounds(slot.anchor, Rect::new(x, 0.0, x + anchor_width, line_height));
            let consumed = if matches!(slot.content, SlotContent::Highlighted(_)) { anchor_width } else { width + separator };
            x += consumed;
            remaining -= consumed;
        }
        self.ui.tree_mut().set_bounds(self.name_container, Rect::new(0.0, 0.0, x, line_height));

        let mut cursor = x;
        if let Some((container, slot)) = &*self.suffix.borrow() {
            let mut tree = self.ui.tree_mut();
            let w = slot.layout(&mut tree, line_height, remaining);
            tree.set_bounds(*container, Rect::new(cursor, 0.0, cursor + w, line_height));
            cursor += w;
            remaining -= w;
        }
        if let Some(description) = &*self.description.borrow() {
            // `margin-left: 0.5em` do `.label-description`, em do tamanho da própria descrição; o
            // `FastLabelNode.empty` zera a margem de uma descrição vazia (só a que não tem realces).
            let em = f64::from(self.description_styles().font_size);
            let (holder, margin, width) = match description {
                DescriptionNode::Plain(slot, holder) => {
                    let margin = if slot.rendered.is_empty() { 0.0 } else { em * 0.5 };
                    (*holder, margin, slot.layout(&mut self.ui.tree_mut(), line_height, (remaining - margin).max(0.0)))
                }
                DescriptionNode::Highlighted(label, holder) => {
                    let margin = em * 0.5;
                    label.set_max_width(Some((remaining - margin).max(0.0)));
                    (*holder, margin, label.width())
                }
            };
            let mut tree = self.ui.tree_mut();
            tree.set_bounds(holder, Rect::new(margin, 0.0, margin + width, line_height));
            if let Some(container) = tree.parent(holder) {
                tree.set_bounds(container, Rect::new(cursor, 0.0, cursor + margin + width, line_height));
            }
            cursor += margin + width;
        }

        let mut tree = self.ui.tree_mut();
        let container_width = available.unwrap_or(cursor);
        tree.set_bounds(self.label_container, Rect::new(icon_extent, 0.0, icon_extent + container_width, line_height));
        if let Some(icon) = self.icon_path_node.get() {
            tree.set_bounds(icon, Rect::new(0.0, 0.0, ICON_WIDTH, ICON_HEIGHT));
        }
        let height = if has_icon { line_height.max(ICON_HEIGHT) } else { line_height };
        resize(&mut tree, self.dom_node, self.width.get().unwrap_or(icon_extent + cursor), height);
    }
}

/// `splitMatches`: reparte os `matches` do texto inteiro (`a/b/c`) entre as partes, em posições de cada uma.
fn split_matches(labels: &[String], separator: &str, matches: Option<&[Match]>) -> Option<Vec<Vec<Match>>> {
    let matches = matches?;
    let mut label_start = 0usize;
    Some(
        labels
            .iter()
            .map(|label| {
                let label_end = label_start + label.encode_utf16().count();
                let result = matches
                    .iter()
                    .map(|m| (m.start.max(label_start), m.end.min(label_end)))
                    .filter(|(start, end)| start < end)
                    .map(|(start, end)| Match { start: start - label_start, end: end - label_start })
                    .collect();
                label_start = label_end + separator.encode_utf16().count();
                result
            })
            .collect(),
    )
}

impl IDisposable for IconLabel {
    fn dispose(&self) {
        if self.disposed.replace(true) {
            return;
        }
        self.base.dispose();
        let hovers: Vec<_> = self.custom_hovers.borrow_mut().drain().map(|(_, h)| h).collect();
        for hover in hovers {
            hover.dispose();
        }
    }

    fn constructor_name(&self) -> &'static str {
        "IconLabel"
    }
}
