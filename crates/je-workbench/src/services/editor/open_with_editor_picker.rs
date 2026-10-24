//! Porte de `src/vs/workbench/services/editor/browser/openWithEditorPicker.ts`: o seletor "Abrir com" dos editores
//! registrados no `IEditorResolverService`, e o comando interno `_workbench.editor.configureDefaultEditor` que o
//! resolvedor chama quando um recurso tem editores padrão em conflito.
//!
//! Notas de tradução:
//! - **`@IQuickInputService`, `@IEditorResolverService`, `@IEditorGroupsService` e `@IExtensionService`.** São as
//!   dependências do [`Injectable`] [`OpenWithEditorPicker`], na ordem dos parâmetros decorados.
//! - **`IUntypedEditorInput` e `IEditorOptions`.** São o [`UntypedEditorInput`] e as [`TextEditorOptions`] (que incluem as
//!   `IEditorOptions`); `{ ...editor.options }` é a cópia das opções, ou as opções vazias quando não há.
//! - **`new Promise(resolve => ...)`.** É o par `Promise::new()`: o resolvedor é copiado para os três ouvintes (`onDidAccept`,
//!   `onDidHide` e `onDidTriggerItemButton`), e resolver uma promessa já resolvida não faz nada, como no JavaScript. Os
//!   ouvintes seguram o seletor por `Weak` para não formar um ciclo de `Rc` com os emissores dele.
//! - **`CommandsRegistry.registerCommand(...)`.** O registro roda na carga do módulo do original; aqui é
//!   [`register_configure_default_editor_command`], que o arranque do workbench chama uma vez. Os argumentos do comando
//!   (`unknown[]`) são [`Dynamic`]: o `IUntypedEditorInput` e o `EditorInput`, que o JavaScript passa como objetos vivos,
//!   chegam como objetos de carga nativa ([`ObjectRef::with_host`]) com um `Rc<UntypedEditorInput>` e um `Rc<EditorInputRef>`,
//!   e o `groupId` como número. O retorno `async` do original é um objeto de carga nativa com a `Promise<()>`
//!   ([`configure_default_editor_command_result`] a recupera).
//! - **`options: replacementEditor.options ?? picked`.** O `IEditorReplacement.options` do porte são as `IEditorOptions`
//!   ([`EditorOptions`]), então as `ITextEditorOptions` do resolvido e do escolhido entram pelo campo `base`.
//! - **`editorPicks` e `quickPickEntries`.** `quickPickEntries.push({ ... })` com `QuickPickItem` é o [`QuickPickInput`], e o
//!   `description: undefined` é a ausência do campo.

use std::rc::Rc;

use je_base::JsString;
use je_base::async_::async_fn;
use je_base::dynamic::{Dynamic, ObjectRef};
use je_base::lifecycle::{DisposableRef, DisposableStore};
use je_base::network::schemas;
use je_base::nls::localize;
use je_base::promise::Promise;
use je_base::uri::{URI, UriComponents};
use je_instantiation::{I_INSTANTIATION_SERVICE, Injectable};
use je_platform::commands::commands::commands_registry;
use je_platform::opener::editor_types::{EditorOptions, EditorOverride};
use je_platform::quickinput::quick_input::{
    IQuickInputService, IQuickItem, IQuickPick, IQuickPickItem, IQuickPickSeparator, QuickPickCreateOptions, QuickPickInput, QuickPickItem,
};
use je_platform::resources::ext_uri;

use crate::common::editor::{
    DEFAULT_EDITOR_ASSOCIATION_ID, EditorInputOrUntyped, EditorInputRef, EditorResourceAccessor, EditorResourceAccessorOptions, SideBySideEditor, TextEditorOptions,
    UntypedEditorInput,
};
use crate::services::editor::editor_groups_service::{EditorReplacement, IEditorGroupsService, I_EDITOR_GROUPS_SERVICE, PreferredGroup};
use crate::services::editor::editor_resolver_service::{
    EditorResolverServiceGetAllEditorsOptions, EditorResolverServiceGetEditorMatchesOptions, IEditorResolverService, I_EDITOR_RESOLVER_SERVICE, RegisteredEditorInfo,
    ResolvedEditor, priority_to_rank,
};
use crate::services::extensions::extensions::IExtensionService;

/// `IOpenWithEditorPickerOptions`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OpenWithEditorPickerOptions {
    /// Asks the user for a new default editor for the resource and persists the choice.
    pub configure_default: Option<bool>,
}

/// `EditorPick`: o item escolhido e se o seletor aceitou em segundo plano.
#[derive(Clone)]
struct EditorPick {
    item: IQuickPickItem,
    open_in_background: bool,
}

const CONFIGURE_DEFAULT_ID: &str = "promptOpenWith.configureDefault";
const CONFIGURE_DEFAULT_DIFF_ID: &str = "promptOpenWith.configureDefaultDiff";

/// Lets the user pick which of the editors registered with the [`IEditorResolverService`] to open an editor with.
pub struct OpenWithEditorPicker {
    quick_input_service: Rc<dyn IQuickInputService>,
    editor_resolver_service: Rc<dyn IEditorResolverService>,
    editor_groups_service: Rc<dyn IEditorGroupsService>,
    extension_service: Rc<dyn IExtensionService>,
}

impl Injectable for OpenWithEditorPicker {
    type Args = ();
    type Deps = (Rc<dyn IQuickInputService>, Rc<dyn IEditorResolverService>, Rc<dyn IEditorGroupsService>, Rc<dyn IExtensionService>);

    fn create(_: (), (quick_input_service, editor_resolver_service, editor_groups_service, extension_service): Self::Deps) -> Rc<Self> {
        Rc::new(Self { quick_input_service, editor_resolver_service, editor_groups_service, extension_service })
    }
}

impl OpenWithEditorPicker {
    /// Shows a picker of the editors that can open the given editor.
    ///
    /// Resolves to the options to open the editor with, where `override` is the picked editor, or `None` if the picker was
    /// cancelled.
    pub fn pick(self: &Rc<Self>, editor: &UntypedEditorInput, options: Option<OpenWithEditorPickerOptions>) -> Promise<Option<TextEditorOptions>> {
        // Side by side editors resolve both sides independently and do not use an override
        if editor.is_resource_side_by_side_editor_input() {
            return Promise::resolved(Some(editor.options.clone().unwrap_or_default()));
        }

        let this = self.clone();
        let editor = editor.clone();
        let configure_default = options.and_then(|options| options.configure_default);
        async_fn(async move {
            // Make sure editors contributed by extensions are part of the picker
            this.extension_service.when_installed_extensions_registered().await?;

            this.do_pick(editor, configure_default, None).await
        })
    }

    fn do_pick(self: &Rc<Self>, editor: UntypedEditorInput, show_default_picker: Option<bool>, update_diff_editor_default: Option<bool>) -> Promise<Option<TextEditorOptions>> {
        let this = self.clone();
        async_fn(async move {
            let resource = EditorResourceAccessor::get_original_uri(
                Some(&EditorInputOrUntyped::Untyped(editor.clone())),
                Some(&EditorResourceAccessorOptions { support_side_by_side: Some(SideBySideEditor::Primary), filter_by_scheme: None }),
            )
            .unwrap_or_else(|| URI::from(&UriComponents { scheme: Some(JsString::from(schemas::UNTITLED)), ..UriComponents::default() }, false));
            let is_diff_editor = editor.is_resource_diff_editor_input();
            // Which setting the default picker should write to. Defaults to the resource's association type
            // so that the per-item gear button keeps writing to the matching setting, but the "Configure
            // default editor" entries can target a specific setting (general vs. diff-only).
            let is_diff_editor_default = update_diff_editor_default.unwrap_or(is_diff_editor);
            let default_rule = this
                .editor_resolver_service
                .get_editor_matches(&resource, Some(&EditorResolverServiceGetEditorMatchesOptions { is_diff_editor: Some(is_diff_editor_default) }))
                .default_rule()
                .clone();

            // Persists the picked editor as the default for this resource's glob. When the user configures
            // the general default from a diff context, any diff-only override for the same glob is cleared
            // so that the general default also takes effect for diffs.
            let persist_default_association: Rc<dyn Fn(&str)> = {
                let editor_resolver_service = this.editor_resolver_service.clone();
                let resource = resource.clone();
                let association_pattern = default_rule.association_pattern.clone();
                Rc::new(move |editor_id: &str| {
                    editor_resolver_service.set_default_editor(&resource, editor_id, Some(is_diff_editor_default));
                    if !is_diff_editor_default && is_diff_editor {
                        editor_resolver_service.remove_diff_editor_association(&resource, &association_pattern);
                    }
                })
            };

            // Get all the editors for the resource as quickpick entries
            let editor_picks = this.get_editor_picks(&resource, show_default_picker, is_diff_editor, is_diff_editor_default);

            // Create the editor picker
            let disposables = DisposableStore::new();
            let editor_picker = this.quick_input_service.create_quick_pick(Some(QuickPickCreateOptions { use_separators: true }));
            let picker_disposable: DisposableRef = editor_picker.clone();
            disposables.add(picker_disposable);
            let place_holder_message = if show_default_picker == Some(true) {
                if is_diff_editor_default {
                    localize("promptOpenWith.updateDefaultDiffPlaceHolder", "Select new default editor (diff only) for '{0}'", &[default_rule.association_pattern.as_str()])
                } else {
                    localize("promptOpenWith.updateDefaultPlaceHolder", "Select new default editor for '{0}'", &[default_rule.association_pattern.as_str()])
                }
            } else {
                localize("promptOpenWith.placeHolder", "Select editor for '{0}'", &[ext_uri().basename(&resource, None).to_string().as_str()])
            };
            editor_picker.set_placeholder(Some(place_holder_message));
            editor_picker.set_can_accept_in_background(true);
            editor_picker.set_items(editor_picks);
            let first_item = editor_picker.items().iter().find_map(|item| item.as_item().cloned());
            if let Some(first_item) = first_item {
                editor_picker.set_selected_items(vec![first_item]);
            }

            // Prompt the user to select an editor
            let (picked_promise, resolver) = Promise::<Option<EditorPick>>::new();
            {
                let picker = Rc::downgrade(&editor_picker);
                let resolver = resolver.clone();
                let persist_default_association = persist_default_association.clone();
                disposables.add(editor_picker.on_did_accept().listen(move |e| {
                    let Some(picker) = picker.upgrade() else { return };
                    let mut result: Option<EditorPick> = None;

                    let selected_items = picker.selected_items();
                    if selected_items.len() == 1 {
                        result = Some(EditorPick { item: selected_items[0].clone(), open_in_background: e.in_background });
                    }

                    // If asked to always update the setting then update it even if the gear isn't clicked
                    if show_default_picker == Some(true)
                        && let Some(id) = result.as_ref().and_then(|result| result.item.base.id.as_deref())
                    {
                        persist_default_association(id);
                    }

                    resolver.resolve(result);
                }));
            }
            {
                let resolver = resolver.clone();
                let disposables_to_dispose = disposables.clone();
                disposables.add(editor_picker.on_did_hide().listen(move |_| {
                    disposables_to_dispose.dispose();
                    resolver.resolve(None);
                }));
            }
            {
                let resolver = resolver.clone();
                let persist_default_association = persist_default_association.clone();
                disposables.add(editor_picker.on_did_trigger_item_button().listen(move |e| {
                    // Trigger opening and close picker
                    resolver.resolve(Some(EditorPick { item: e.item.clone(), open_in_background: false }));

                    // Persist setting
                    if let Some(id) = e.item.base.id.as_deref() {
                        persist_default_association(id);
                    }
                }));
            }
            editor_picker.show();
            let picked = picked_promise.await?;

            // Close picker
            editor_picker.dispose();

            // If the user picked an editor, look at how the picker was
            // used (e.g. open in background) and create the options to use accordingly
            if let Some(picked) = picked {
                // If the user selected to configure default we trigger this picker again and tell it to show the default picker
                let picked_id = picked.item.base.id.clone();
                if picked_id.as_deref() == Some(CONFIGURE_DEFAULT_ID) {
                    return this.do_pick(editor, Some(true), Some(false)).await;
                }
                // The diff-only variant writes to `diffEditorAssociations` so it does not change how the
                // resource opens as a normal editor.
                if picked_id.as_deref() == Some(CONFIGURE_DEFAULT_DIFF_ID) {
                    return this.do_pick(editor, Some(true), Some(true)).await;
                }

                let mut result = editor.options.clone().unwrap_or_default();
                result.base.override_ = picked_id.map(|id| EditorOverride::Id(JsString::from(id.as_str())));
                result.base.preserve_focus = if picked.open_in_background { Some(true) } else { editor.options.as_ref().and_then(|options| options.preserve_focus) };
                return Ok(Some(result));
            }

            Ok(None)
        })
    }

    fn get_editor_picks(&self, resource: &URI, show_default_picker: Option<bool>, is_diff_editor: bool, is_diff_editor_default: bool) -> Vec<QuickPickItem> {
        let current_editor = self.editor_groups_service.active_group().find_editors(resource, None).into_iter().next();
        // If untitled, we want all registered editors
        let mut registered_editors: Vec<RegisteredEditorInfo> = if *resource.scheme() == schemas::UNTITLED {
            self.editor_resolver_service
                .get_all_editors(Some(&EditorResolverServiceGetAllEditorsOptions { exclude_exclusive_editors: Some(true), is_diff_editor: Some(is_diff_editor) }))
        } else {
            self.editor_resolver_service
                .get_editor_matches(resource, Some(&EditorResolverServiceGetEditorMatchesOptions { is_diff_editor: Some(is_diff_editor) }))
                .matches()
                .iter()
                .map(|rule| rule.editor.clone())
                .collect()
        };
        let default_rule = self
            .editor_resolver_service
            .get_editor_matches(resource, Some(&EditorResolverServiceGetEditorMatchesOptions { is_diff_editor: Some(is_diff_editor_default) }))
            .default_rule()
            .clone();
        let get_priority = |editor: &RegisteredEditorInfo| if is_diff_editor { editor.priority.diff } else { editor.priority.editor };
        // Not the most efficient way to do this, but we want to ensure the text editor is at the top of the quickpick
        registered_editors.sort_by(|a, b| {
            if a.id == DEFAULT_EDITOR_ASSOCIATION_ID {
                std::cmp::Ordering::Less
            } else if b.id == DEFAULT_EDITOR_ASSOCIATION_ID {
                std::cmp::Ordering::Greater
            } else {
                priority_to_rank(get_priority(b)).cmp(&priority_to_rank(get_priority(a)))
            }
        });
        let mut quick_pick_entries: Vec<QuickPickItem> = Vec::new();
        let currently_active_label = localize("promptOpenWith.currentlyActive", "Active", &[]);
        let current_default_label = localize("promptOpenWith.currentDefault", "Default", &[]);
        let current_default_and_active_label = localize("promptOpenWith.currentDefaultAndActive", "Active and Default", &[]);
        // Map the editors to quickpick entries
        for editor in &registered_editors {
            let current_view_type = current_editor.as_ref().and_then(|current_editor| current_editor.editor_id()).unwrap_or_else(|| DEFAULT_EDITOR_ASSOCIATION_ID.to_string());
            let is_active = current_editor.is_some() && editor.id == current_view_type;
            let is_default = editor.id == default_rule.editor.id;
            let description = match (is_active, is_default) {
                (true, true) => Some(current_default_and_active_label.as_str()),
                (true, false) => Some(currently_active_label.as_str()),
                (false, true) => Some(current_default_label.as_str()),
                (false, false) => None,
            };
            quick_pick_entries.push(QuickPickInput::Item(IQuickPickItem {
                base: IQuickItem {
                    id: Some(editor.id.clone()),
                    label: JsString::from(editor.label.as_str()),
                    description: description.map(JsString::from),
                    ..IQuickItem::default()
                },
                detail: Some(editor.detail.clone().unwrap_or_else(|| editor.priority.editor.as_str().to_string())),
                ..IQuickPickItem::default()
            }));
        }
        if show_default_picker != Some(true) && !ext_uri().extname(resource).is_empty() {
            quick_pick_entries.push(QuickPickInput::Separator(IQuickPickSeparator::default()));
            let editor_default = self.editor_resolver_service.get_editor_matches(resource, None).default_rule().clone();
            quick_pick_entries.push(QuickPickInput::Item(IQuickPickItem {
                base: IQuickItem {
                    id: Some(CONFIGURE_DEFAULT_ID.to_string()),
                    label: JsString::from(
                        localize("promptOpenWith.configureDefault", "Configure default editor for '{0}'...", &[editor_default.association_pattern.as_str()]).as_str(),
                    ),
                    ..IQuickItem::default()
                },
                ..IQuickPickItem::default()
            }));
            // For diffs, additionally offer to configure a diff-only default so the choice does not
            // affect how the resource opens as a normal editor (writes to `diffEditorAssociations`).
            if is_diff_editor {
                let diff_editor_default = self
                    .editor_resolver_service
                    .get_editor_matches(resource, Some(&EditorResolverServiceGetEditorMatchesOptions { is_diff_editor: Some(true) }))
                    .default_rule()
                    .clone();
                quick_pick_entries.push(QuickPickInput::Item(IQuickPickItem {
                    base: IQuickItem {
                        id: Some(CONFIGURE_DEFAULT_DIFF_ID.to_string()),
                        label: JsString::from(
                            localize("promptOpenWith.configureDefaultDiff", "Configure default editor (diff only) for '{0}'...", &[diff_editor_default.association_pattern.as_str()])
                                .as_str(),
                        ),
                        ..IQuickItem::default()
                    },
                    ..IQuickPickItem::default()
                }));
            }
        }
        quick_pick_entries
    }
}

/// Internal command used by the [`IEditorResolverService`] when a resource has conflicting default editors: asks the user for
/// a new default editor and replaces `currentEditor` with it.
pub const CONFIGURE_DEFAULT_EDITOR_COMMAND_ID: &str = "_workbench.editor.configureDefaultEditor";

/// O objeto de carga nativa que o argumento `i` do comando traz, do tipo `T` (ver as notas do módulo).
fn host_argument<T: 'static>(args: &[Dynamic], index: usize) -> Option<Rc<T>> {
    match args.get(index)? {
        Dynamic::Object(object) => object.host::<T>(),
        _ => None,
    }
}

/// O retorno do comando como `Dynamic`: o objeto de carga nativa com a `Promise<()>` do corpo `async`.
fn configure_default_editor_command_result_object(promise: Promise<()>) -> Dynamic {
    Dynamic::Object(ObjectRef::with_host(Rc::new(promise)))
}

/// A `Promise<()>` que o comando devolveu (o `await handler(...)` do original).
pub fn configure_default_editor_command_result(result: &Dynamic) -> Option<Promise<()>> {
    match result {
        Dynamic::Object(object) => object.host::<Promise<()>>().map(|promise| (*promise).clone()),
        _ => None,
    }
}

/// O argumento `untypedEditor` do comando: o `IUntypedEditorInput` como objeto de carga nativa.
pub fn configure_default_editor_untyped_argument(editor: UntypedEditorInput) -> Dynamic {
    Dynamic::Object(ObjectRef::with_host(Rc::new(editor)))
}

/// O argumento `currentEditor` do comando: o `EditorInput` como objeto de carga nativa.
pub fn configure_default_editor_current_editor_argument(editor: EditorInputRef) -> Dynamic {
    Dynamic::Object(ObjectRef::with_host(Rc::new(editor)))
}

/// `CommandsRegistry.registerCommand(CONFIGURE_DEFAULT_EDITOR_COMMAND_ID, async (accessor, untypedEditor, currentEditor,
/// groupId) => ...)`.
pub fn register_configure_default_editor_command() -> DisposableRef {
    commands_registry().register_command_handler(
        CONFIGURE_DEFAULT_EDITOR_COMMAND_ID,
        Some(Rc::new(|accessor, args| {
            let editor_resolver_service = accessor.get(I_EDITOR_RESOLVER_SERVICE);
            let editor_groups_service = accessor.get(I_EDITOR_GROUPS_SERVICE);
            let instantiation_service = accessor.get(I_INSTANTIATION_SERVICE);

            let untyped_editor = host_argument::<UntypedEditorInput>(args, 0).map(|editor| (*editor).clone());
            let current_editor = host_argument::<EditorInputRef>(args, 1).map(|editor| (*editor).clone());
            let group_id = match args.get(2) {
                Some(Dynamic::Number(group_id)) => Some(*group_id as i64),
                _ => None,
            };

            let result = async_fn(async move {
                let (Some(untyped_editor), Some(current_editor), Some(group_id)) = (untyped_editor, current_editor, group_id) else {
                    return Err(je_base::errors::Error::new("Illegal arguments"));
                };

                let picked =
                    instantiation_service.create_instance::<OpenWithEditorPicker>(()).pick(&untyped_editor, Some(OpenWithEditorPickerOptions { configure_default: Some(true) })).await?;
                let group = editor_groups_service.get_group(group_id);
                let (Some(picked), Some(group)) = (picked, group) else {
                    return Ok(());
                };

                let mut resolve_input = untyped_editor.clone();
                resolve_input.options = Some(picked.clone());
                let replacement_editor = editor_resolver_service.resolve_editor(&resolve_input, Some(&PreferredGroup::Group(group.clone()))).await?;
                let ResolvedEditor::Editor(replacement_editor) = replacement_editor else {
                    return Ok(());
                };

                // Replace the current editor with the picked one
                let options: Option<EditorOptions> = Some(replacement_editor.options.map_or(picked.base, |options| options.base));
                group.replace_editors(vec![EditorReplacement { editor: current_editor, replacement: replacement_editor.editor, options, force_replace_dirty: None }]).await?;

                Ok(())
            });
            configure_default_editor_command_result_object(result)
        })),
    )
}
