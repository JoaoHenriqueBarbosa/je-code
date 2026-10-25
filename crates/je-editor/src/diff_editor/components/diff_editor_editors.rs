//! Porte de `src/vs/editor/browser/widget/diffEditor/components/diffEditorEditors.ts`: os dois editores de código do editor de
//! diff, o original (esquerda) e o modificado (direita), as opções que cada lado recebe e os observáveis do estado deles.
//!
//! Divergências:
//! - **Contêineres e fábrica.** O original recebe os dois `HTMLElement` e um `_createInnerEditor(instantiationService,
//!   container, options, editorWidgetOptions)`. O contêiner de cada lado é do mundo nativo e o `IInstantiationService` é de quem
//!   monta o widget, então os dois entram na [`CreateInnerEditor`], que recebe o lado ([`DiffEditorSide`]), as opções cruas e as
//!   `ICodeEditorWidgetOptions` e devolve o editor. Dos serviços injetados ficam o `@IContextKeyService` e o
//!   `@IKeybindingService` (por [`Injectable`]); o `@IInstantiationService` não é usado pelo próprio componente.
//! - **Opções cruas são [`Dynamic`].** `IDiffEditorConstructionOptions` e `IEditorConstructionOptions` são objetos
//!   [`Dynamic`]; `{ ...options }` é uma cópia rasa do objeto e `Object.assign` é [`assign_options`]. As opções do diff que o
//!   objeto cru ainda traga ficam nele e o editor de código as ignora, como no original.
//! - **`_createLeftHandSideEditor` e `_createRightHandSideEditor`** só diferem no lado (os ajustes de opções, a chave de contexto
//!   `isInDiffLeftEditor` ou `isInDiffRightEditor` e o contêiner), e viram uma função só com o lado por parâmetro
//!   ([`InnerEditorFactory::create`]). O `_constructInnerEditor` é o começo dela. Os dois ajustes ([`SubEditorOptions`]) seguem
//!   separados, porque divergem em quase tudo.
//! - **`IContentSizeChangedEvent`.** O evento de tamanho de conteúdo é o `ContentSizeChangedEvent` do porte, cujo construtor
//!   deriva os `*Changed` de valores antigos e novos; o original monta o objeto literal com os `*Changed` do evento do editor
//!   interno, e aqui os valores antigos são escolhidos para dar as mesmas marcas (eles são privados e nada os lê).
//! - **Referência circular.** O ouvinte de tamanho de cada editor lê os dois; ele guarda `Weak` dos editores (o par é preenchido
//!   conforme os editores nascem), porque um `Rc` formaria um ciclo com o emissor do próprio editor.
//! - `lookupKeybinding(...)?.getAriaLabel()` vira o argumento de `localize` conforme o original: ausente é `undefined`, e um
//!   atalho sem rótulo é `null`.
//! - O `autorunHandleChanges` com o `changeTracker` literal usa o `FnChangeTracker`; o resumo de mudanças é um objeto
//!   [`Dynamic`] novo a cada rodada, em que o `handleChange` espalha as `changedOptions` da mudança das opções.

use std::cell::OnceCell;
use std::rc::{Rc, Weak};

use je_base::JsString;
use je_base::dynamic::{Dynamic, ObjectRef, Truthy};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::nls::{NlsArg, localize_args};
use je_base::observable::change_tracker::FnChangeTracker;
use je_base::observable::observables::derived::derived_owned;
use je_base::observable::observables::observable_from_event::observable_from_event_owned;
use je_base::observable::{DebugNameOptions, DebugOwner, DerivedOptions, IObservable, Observable, autorun_handle_changes, derived_opts};
use je_instantiation::Injectable;
use je_platform::contextkey::IContextKeyService;
use je_platform::keybinding::keybinding::IKeybindingService;

use crate::code_editor_widget::{CodeEditorWidget, CodeEditorWidgetOptions, ICodeEditor};
use crate::config::editor_options::registry::{REVEAL_HORIZONTAL_RIGHT_PADDING, editor_options_registry};
use crate::diff_editor::{DiffCodeEditorWidgetOptions, ENTIRE_DIFF_OVERVIEW_WIDTH};
use crate::diff_editor_options::{DiffEditorOptions, DiffEditorOptionsChange};
use crate::observable_code_editor::{ObservableCodeEditor, observable_code_editor};
use crate::position::Position;
use crate::selection::Selection;
use crate::text_model::TextModel;
use crate::view_model_event_dispatcher::ContentSizeChangedEvent;

/// O lado de um editor do diff: o original (esquerda) ou o modificado (direita).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiffEditorSide {
    Original,
    Modified,
}

impl DiffEditorSide {
    /// A chave de contexto que diz se o foco está neste lado.
    fn context_key(self) -> &'static str {
        match self {
            Self::Original => "isInDiffLeftEditor",
            Self::Modified => "isInDiffRightEditor",
        }
    }
}

/// O `_createInnerEditor` do original: cria o editor de código do lado dado, com as opções cruas e as do widget.
pub type CreateInnerEditor = Rc<dyn Fn(DiffEditorSide, &Dynamic, CodeEditorWidgetOptions) -> Rc<CodeEditorWidget>>;

/// Os argumentos estáticos do construtor do `DiffEditorEditors`.
pub struct DiffEditorEditorsArgs {
    pub options: Rc<DiffEditorOptions>,
    pub widget_options: DiffCodeEditorWidgetOptions,
    pub create_inner_editor: CreateInnerEditor,
}

/// `Object.assign(target, source)`: as propriedades próprias de `source` entram em `target`.
fn assign_options(target: &ObjectRef, source: &Dynamic) {
    for (key, value) in source.own_entries() {
        target.set(&key, value);
    }
}

/// `target[key] = value`.
fn set(target: &ObjectRef, key: &str, value: impl Into<Dynamic>) {
    target.set(&JsString::from(key), value.into());
}

/// `{ ...value }`: a cópia rasa. Um valor que não é objeto (ou ausente) dá o objeto vazio, como o `...undefined`.
fn spread(value: &Dynamic) -> ObjectRef {
    ObjectRef::from_entries(value.own_entries())
}

/// `{ enabled }`.
fn enabled_option(enabled: bool) -> ObjectRef {
    ObjectRef::from_entries([("enabled", Dynamic::Bool(enabled))])
}

/// `EditorOptions.revealHorizontalRightPadding.defaultValue`.
fn reveal_horizontal_right_padding_default() -> f64 {
    let registry = editor_options_registry();
    let default_value = registry[REVEAL_HORIZONTAL_RIGHT_PADDING.id() as usize].default_value();
    *default_value.downcast_ref::<f64>().expect("revealHorizontalRightPadding é numérica")
}

/// Os ajustes de opções de cada lado (`_adjustOptionsForLeftHandSide`, `_adjustOptionsForRightHandSide`,
/// `_adjustOptionsForSubEditor` e `_updateAriaLabel`), com o que eles leem do componente.
struct SubEditorOptions {
    options: Rc<DiffEditorOptions>,
    keybinding_service: Rc<dyn IKeybindingService>,
}

impl SubEditorOptions {
    fn for_left_hand_side(&self, changed_options: &Dynamic) -> ObjectRef {
        let result = self.for_sub_editor(changed_options);
        let render_side_by_side = self.options.render_side_by_side.get();
        if !render_side_by_side {
            // never wrap hidden editor
            set(&result, "wordWrapOverride1", "off");
            set(&result, "wordWrapOverride2", "off");
            set(&result, "stickyScroll", enabled_option(false));

            // Disable unicode highlighting for the original side in inline mode, as they are not shown anyway.
            set(
                &result,
                "unicodeHighlight",
                ObjectRef::from_entries([
                    ("nonBasicASCII", Dynamic::Bool(false)),
                    ("ambiguousCharacters", Dynamic::Bool(false)),
                    ("invisibleCharacters", Dynamic::Bool(false)),
                ]),
            );
        } else {
            let unicode_highlight = self.options.editor_options().get().editor_options.optional_get_str("unicodeHighlight");
            set(&result, "unicodeHighlight", if unicode_highlight.is_truthy() { unicode_highlight } else { Dynamic::from(ObjectRef::new()) });
            set(&result, "wordWrapOverride1", self.options.diff_word_wrap.get().as_str());
        }
        set(&result, "glyphMargin", render_side_by_side);

        let original_aria_label = changed_options.optional_get_str("originalAriaLabel");
        if original_aria_label.is_truthy() {
            set(&result, "ariaLabel", original_aria_label);
        }
        self.update_aria_label_option(&result);
        let read_only = !self.options.original_editable.get();
        set(&result, "readOnly", read_only);
        set(&result, "dropIntoEditor", enabled_option(!read_only));
        set(&result, "extraEditorClassName", "original-in-monaco-diff-editor");
        result
    }

    fn for_right_hand_side(&self, changed_options: &Dynamic) -> ObjectRef {
        let result = self.for_sub_editor(changed_options);
        let modified_aria_label = changed_options.optional_get_str("modifiedAriaLabel");
        if modified_aria_label.is_truthy() {
            set(&result, "ariaLabel", modified_aria_label);
        }
        self.update_aria_label_option(&result);
        set(&result, "wordWrapOverride1", self.options.diff_word_wrap.get().as_str());
        set(&result, "revealHorizontalRightPadding", reveal_horizontal_right_padding_default() + ENTIRE_DIFF_OVERVIEW_WIDTH);
        if let Some(Dynamic::Object(scrollbar)) = result.get(&JsString::from("scrollbar")) {
            set(&scrollbar, "verticalHasArrows", false);
        }
        set(&result, "extraEditorClassName", "modified-in-monaco-diff-editor");
        result
    }

    fn for_sub_editor(&self, options: &Dynamic) -> ObjectRef {
        let cloned_options = spread(options);
        set(&cloned_options, "dimension", ObjectRef::from_entries([("height", Dynamic::from(0.0)), ("width", Dynamic::from(0.0))]));
        set(&cloned_options, "inDiffEditor", true);
        set(&cloned_options, "automaticLayout", false);
        set(&cloned_options, "allowVariableLineHeights", false);
        set(&cloned_options, "allowVariableFonts", false);
        set(&cloned_options, "allowVariableFontsInAccessibilityMode", false);

        // Clone scrollbar options before changing them
        set(&cloned_options, "scrollbar", spread(&options.optional_get_str("scrollbar")));
        set(&cloned_options, "folding", false);
        set(&cloned_options, "codeLens", self.options.diff_code_lens.get());
        set(&cloned_options, "fixedOverflowWidgets", true);

        // Clone minimap options before changing them
        let minimap = spread(&options.optional_get_str("minimap"));
        set(&minimap, "enabled", false);
        set(&cloned_options, "minimap", minimap);

        if self.options.hide_unchanged_regions.get() {
            set(&cloned_options, "stickyScroll", enabled_option(false));
        } else {
            set(&cloned_options, "stickyScroll", self.options.editor_options().get().editor_options.optional_get_str("stickyScroll"));
        }
        cloned_options
    }

    /// `result.ariaLabel = this._updateAriaLabel(result.ariaLabel)`.
    fn update_aria_label_option(&self, result: &ObjectRef) {
        let aria_label = result.get(&JsString::from("ariaLabel")).unwrap_or(Dynamic::Undefined);
        set(result, "ariaLabel", self.update_aria_label(&aria_label));
    }

    fn update_aria_label(&self, aria_label: &Dynamic) -> String {
        let aria_label = match aria_label {
            Dynamic::String(text) => text.to_string_lossy(),
            _ => String::new(),
        };
        let shortcut = self.keybinding_service.lookup_keybinding("editor.action.accessibilityHelp", None, false);
        let shortcut_label = match shortcut {
            None => NlsArg::Undefined,
            Some(keybinding) => keybinding.get_aria_label().map_or(NlsArg::Null, NlsArg::Str),
        };
        let aria_navigation_tip = localize_args("diff-aria-navigation-tip", " use {0} to open the accessibility help.", &[shortcut_label]);
        if self.options.accessibility_verbose.get() {
            format!("{aria_label}{aria_navigation_tip}")
        } else if !aria_label.is_empty() {
            aria_label.replace(&aria_navigation_tip, "")
        } else {
            String::new()
        }
    }
}

/// Os dois editores, preenchidos conforme nascem (o `this.original` e o `this.modified` que os ouvintes do original leem).
#[derive(Default)]
struct EditorPair {
    original: OnceCell<Weak<CodeEditorWidget>>,
    modified: OnceCell<Weak<CodeEditorWidget>>,
}

impl EditorPair {
    fn slot(&self, side: DiffEditorSide) -> &OnceCell<Weak<CodeEditorWidget>> {
        match side {
            DiffEditorSide::Original => &self.original,
            DiffEditorSide::Modified => &self.modified,
        }
    }

    /// Os dois editores, se já nasceram e continuam vivos.
    fn both(&self) -> Option<(Rc<CodeEditorWidget>, Rc<CodeEditorWidget>)> {
        Some((self.original.get()?.upgrade()?, self.modified.get()?.upgrade()?))
    }
}

/// `Math.max(this.modified.getContentHeight(), this.original.getContentHeight())`.
fn max_content_height(original: &CodeEditorWidget, modified: &CodeEditorWidget) -> f64 {
    modified.get_content_height().max(original.get_content_height())
}

/// O `IContentSizeChangedEvent` literal do original: os `*Changed` vêm do evento do editor interno (ver as notas do módulo).
fn content_size_event(content_width: f64, content_height: f64, content_width_changed: bool, content_height_changed: bool) -> ContentSizeChangedEvent {
    let old = |value: f64, changed: bool| if changed { value - 1.0 } else { value };
    ContentSizeChangedEvent::new(old(content_width, content_width_changed), old(content_height, content_height_changed), content_width, content_height)
}

/// Cria os editores internos (a parte do original que os dois `_create...HandSideEditor` e o `_constructInnerEditor` dividem).
struct InnerEditorFactory {
    adjust: Rc<SubEditorOptions>,
    create_inner_editor: CreateInnerEditor,
    context_key_service: Rc<dyn IContextKeyService>,
    on_did_content_size_change: Rc<Emitter<ContentSizeChangedEvent>>,
    pair: Rc<EditorPair>,
}

impl InnerEditorFactory {
    /// `_createLeftHandSideEditor` e `_createRightHandSideEditor`, com o `_constructInnerEditor`; o editor fica registrado em `base`.
    fn create(&self, base: &Disposable, side: DiffEditorSide, options: &Dynamic, widget_options: CodeEditorWidgetOptions) -> Rc<CodeEditorWidget> {
        let side_options = Dynamic::from(match side {
            DiffEditorSide::Original => self.adjust.for_left_hand_side(options),
            DiffEditorSide::Modified => self.adjust.for_right_hand_side(options),
        });
        let editor = (self.create_inner_editor)(side, &side_options, widget_options);

        let (pair, on_did_content_size_change) = (self.pair.clone(), self.on_did_content_size_change.clone());
        base.register(editor.on_did_content_size_change().listen(move |e| {
            let Some((original, modified)) = pair.both() else { return };
            let width = original.get_content_width() + modified.get_content_width() + ENTIRE_DIFF_OVERVIEW_WIDTH;
            on_did_content_size_change.fire(content_size_event(width, max_content_height(&original, &modified), e.content_width_changed, e.content_height_changed));
        }));

        let is_in_diff_editor_key = self.context_key_service.create_key(side.context_key(), Dynamic::Bool(editor.has_widget_focus()));
        base.register(editor.on_did_focus_editor_widget().listen({
            let key = is_in_diff_editor_key.clone();
            move |_| key.set(Dynamic::Bool(true))
        }));
        base.register(editor.on_did_blur_editor_widget().listen(move |_| is_in_diff_editor_key.set(Dynamic::Bool(false))));

        base.register(editor.clone());
        self.pair.slot(side).set(Rc::downgrade(&editor)).expect("cada lado nasce uma vez");
        editor
    }
}

/// `DiffEditorEditors`.
pub struct DiffEditorEditors {
    base: Rc<Disposable>,
    pub original: Rc<CodeEditorWidget>,
    pub modified: Rc<CodeEditorWidget>,

    on_did_content_size_change: Rc<Emitter<ContentSizeChangedEvent>>,

    pub modified_scroll_top: IObservable<f64>,
    pub modified_scroll_height: IObservable<f64>,

    pub modified_obs: Rc<ObservableCodeEditor>,
    pub original_obs: Rc<ObservableCodeEditor>,

    pub modified_model: IObservable<Option<Rc<TextModel>>>,

    pub modified_selections: IObservable<Rc<Vec<Selection>>>,
    pub modified_cursor: IObservable<Position>,

    pub original_cursor: IObservable<Position>,

    pub is_original_focused: IObservable<bool>,
    pub is_modified_focused: IObservable<bool>,

    pub is_focused: IObservable<bool>,
}

impl Injectable for DiffEditorEditors {
    type Args = DiffEditorEditorsArgs;
    type Deps = (Rc<dyn IContextKeyService>, Rc<dyn IKeybindingService>);

    fn create(args: Self::Args, (context_key_service, keybinding_service): Self::Deps) -> Rc<Self> {
        Self::new(args, context_key_service, keybinding_service)
    }
}

impl DiffEditorEditors {
    pub fn new(args: DiffEditorEditorsArgs, context_key_service: Rc<dyn IContextKeyService>, keybinding_service: Rc<dyn IKeybindingService>) -> Rc<Self> {
        let DiffEditorEditorsArgs { options, widget_options, create_inner_editor } = args;
        let DiffCodeEditorWidgetOptions { original_editor, modified_editor, .. } = widget_options;
        let owner = DebugOwner::new("DiffEditorEditors");
        let base = Disposable::new();

        let on_did_content_size_change = Emitter::<ContentSizeChangedEvent>::new();
        base.register(on_did_content_size_change.clone());

        let adjust = Rc::new(SubEditorOptions { options: options.clone(), keybinding_service });
        let pair = Rc::new(EditorPair::default());
        let factory = InnerEditorFactory {
            adjust: adjust.clone(),
            create_inner_editor,
            context_key_service,
            on_did_content_size_change: on_did_content_size_change.clone(),
            pair,
        };

        let initial_options = options.editor_options().get().editor_options.clone();
        let original = factory.create(&base, DiffEditorSide::Original, &initial_options, original_editor.unwrap_or_default());
        let modified = factory.create(&base, DiffEditorSide::Modified, &initial_options, modified_editor.unwrap_or_default());

        let modified_scroll_top = {
            let modified = modified.clone();
            observable_from_event_owned(owner.clone(), modified.on_did_scroll_change(), move |_| modified.get_scroll_top())
        };
        let modified_scroll_height = {
            let modified = modified.clone();
            observable_from_event_owned(owner.clone(), modified.on_did_scroll_change(), move |_| modified.get_scroll_height())
        };
        let modified_obs = observable_code_editor(&modified);
        let original_obs = observable_code_editor(&original);
        let modified_model = modified_obs.model.clone();
        let modified_selections = {
            let modified = modified.clone();
            observable_from_event_owned(owner.clone(), modified.on_did_change_cursor_selection(), move |_| Rc::new(modified.get_selections().unwrap_or_default()))
        };
        let modified_cursor = {
            let selections = modified_selections.clone();
            derived_opts(
                DerivedOptions { owner: Some(owner.clone()), equals_fn: Some(Rc::new(|a: &Position, b: &Position| a == b)), ..DerivedOptions::default() },
                move |reader| selections.read(reader).first().map(Selection::get_position).unwrap_or(Position::new(1, 1)),
            )
        };
        let original_cursor = {
            let original = original.clone();
            observable_from_event_owned(owner.clone(), original.on_did_change_cursor_position(), move |_| original.get_position().unwrap_or(Position::new(1, 1)))
        };
        let is_original_focused = original_obs.is_focused.clone();
        let is_modified_focused = modified_obs.is_focused.clone();
        let is_focused = {
            let (original_focused, modified_focused) = (is_original_focused.clone(), is_modified_focused.clone());
            derived_owned(owner, move |reader| original_focused.read(reader) || modified_focused.read(reader))
        };

        let change_tracker = {
            let editor_options = options.editor_options().to_any_observable();
            FnChangeTracker::<Dynamic>::new(
                |_| Dynamic::Object(ObjectRef::new()),
                move |ctx, change_summary| {
                    if ctx.did_change(&*editor_options)
                        && let (Dynamic::Object(summary), Some(change)) = (change_summary, ctx.change::<DiffEditorOptionsChange>())
                    {
                        assign_options(summary, &change.changed_options);
                    }
                    true
                },
            )
        };
        {
            let (original, modified) = (original.clone(), modified.clone());
            base.register(autorun_handle_changes::<Dynamic>(DebugNameOptions::named("update editor options"), change_tracker, move |reader, change_summary| {
                options.editor_options().read(reader);
                options.render_side_by_side.read(reader);

                modified.update_options(Some(&Dynamic::from(adjust.for_right_hand_side(change_summary))));
                original.update_options(Some(&Dynamic::from(adjust.for_left_hand_side(change_summary))));
            }));
        }

        Rc::new(Self {
            base,
            original,
            modified,
            on_did_content_size_change,
            modified_scroll_top,
            modified_scroll_height,
            modified_obs,
            original_obs,
            modified_model,
            modified_selections,
            modified_cursor,
            original_cursor,
            is_original_focused,
            is_modified_focused,
            is_focused,
        })
    }

    /// `onDidContentSizeChange`.
    pub fn on_did_content_size_change(&self) -> Event<ContentSizeChangedEvent> {
        self.on_did_content_size_change.event()
    }

    pub fn get_content_height(&self) -> f64 {
        max_content_height(&self.original, &self.modified)
    }
}

impl IDisposable for DiffEditorEditors {
    fn dispose(&self) {
        self.base.dispose();
    }
}
