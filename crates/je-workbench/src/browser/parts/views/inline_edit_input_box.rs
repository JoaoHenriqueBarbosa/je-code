//! Porte de `src/vs/workbench/browser/parts/views/inlineEditInputBox.ts`: um campo para editar um item em linha (por
//! exemplo, renomear um item de lista ou de árvore), sobre o [`InputBox`] nativo.
//!
//! Erros de `IEditableData.validationMessage` bloqueiam a aceitação do valor; as outras mensagens aparecem como
//! informação. Enter aceita o valor, Escape cancela e mover o foco para outro lugar aceita o valor se ele é válido.
//! Terminada a edição, o campo é removido e o `onFinish` é chamado; descartar o campo antes de terminar remove-o sem
//! chamar o `onFinish`.
//!
//! Divergências:
//! - **Enter e Escape.** O `InputBox` nativo entrega o `keydown` como [`StandardKeyboardEvent`] (um valor), sem
//!   `stopPropagation`/`preventDefault`; a árvore que contém o campo não recebe o evento porque o campo tem o foco.
//! - **Blur.** O original espera o `setTimeout(0)`, deixa a edição aberta enquanto o elemento ativo está dentro de um
//!   `context-view` (menu de contexto do campo) e só termina se o foco saiu de fato. Aqui o "menu de contexto aberto"
//!   é o estado de `onDidShowContextMenu`/`onDidHideContextMenu` e "o foco continua no campo" é `ui.focused()`; o
//!   `ownerDocument.hasFocus()` não tem equivalente porque a janela nativa não separa o foco do documento do da janela.

use std::cell::Cell;
use std::rc::{Rc, Weak};

use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::key_codes::KeyCode;
use je_base::runtime;
use je_base::severity::Severity;
use je_platform::contextview::context_view::IContextMenuService;
use je_ui::{NodeId, Ui};
use je_widgets::context_view_api::IContextViewProvider;
use je_widgets::input_box::{InputBox, InputBoxOptions, InputBoxStyles, InputValidationOptions, Message, MessageType, Range};

use crate::contrib::files::browser_files::EditableData;

/// `IInlineEditInputBoxOptions`.
#[derive(Clone, Debug)]
pub struct InlineEditInputBoxOptions {
    /// The aria label of the input box.
    pub aria_label: String,
    /// The initial value of the input box. Defaults to `IEditableData.startingValue`.
    pub value: Option<String>,
    /// The initially selected range of the value. Defaults to the whole value.
    pub selection: Option<Range>,
}

/// `InlineEditInputBox`.
pub struct InlineEditInputBox {
    base: Rc<DisposableStore>,
    ui: Ui,
    input_box: Rc<InputBox>,
    editable_data: EditableData,
    /// Há um menu de contexto do campo aberto: a edição continua mesmo sem foco no campo.
    context_menu_open: Cell<bool>,
    /// O campo perdeu o foco enquanto o menu de contexto estava aberto: reavalia quando o menu fechar.
    blur_pending: Cell<bool>,
    weak_self: Weak<InlineEditInputBox>,
}

impl InlineEditInputBox {
    /// `constructor(container, editableData, options, contextViewService, contextMenuService)`.
    pub fn new(
        ui: &Ui,
        container: NodeId,
        context_view_provider: Option<Rc<dyn IContextViewProvider>>,
        context_menu_service: Rc<dyn IContextMenuService>,
        input_box_styles: InputBoxStyles,
        editable_data: EditableData,
        options: InlineEditInputBoxOptions,
    ) -> Rc<Self> {
        let validation_data = editable_data.clone();
        let mut input_options = InputBoxOptions::new(input_box_styles);
        input_options.validation_options = Some(InputValidationOptions {
            validation: Some(Rc::new(move |value: &str| {
                let message = (validation_data.validation_message)(value)?;
                if message.severity != Severity::Error {
                    return None;
                }

                Some(Message { content: Some(message.content), format_content: true, r#type: Some(MessageType::Error) })
            })),
        });
        input_options.aria_label = Some(options.aria_label.clone());
        input_options.placeholder = editable_data.placeholder.clone();

        let input_box = InputBox::new(ui, container, context_view_provider, input_options);
        let base = DisposableStore::new();
        base.add(input_box.clone());

        let value = options.value.clone().or_else(|| editable_data.starting_value.clone()).unwrap_or_default();
        input_box.set_value(&value);
        input_box.focus();
        input_box.select(Some(options.selection.unwrap_or(Range { start: 0, end: value.encode_utf16().count() })));

        let this = Rc::new_cyclic(|weak_self| Self {
            base,
            ui: ui.clone(),
            input_box: input_box.clone(),
            editable_data,
            context_menu_open: Cell::new(false),
            blur_pending: Cell::new(false),
            weak_self: weak_self.clone(),
        });
        this.show_message();

        // Enter and Escape are handled here and must not trigger keybindings of the containing view
        this.base.add(input_box.on_key_down().listen({
            let weak = this.weak_self.clone();
            move |event| {
                let Some(this) = weak.upgrade() else { return };
                if event.equals(KeyCode::Enter.bits()) {
                    if this.input_box.validate().is_none() {
                        this.finish(true);
                    }
                } else if event.equals(KeyCode::Escape.bits()) {
                    this.finish(false);
                }
            }
        }));

        // Refresh non-error messages on every value change, including paste, drop and IME input. This listener
        // is registered after the input box's own one so it runs after validation, which may hide the message.
        this.base.add(input_box.on_input().listen({
            let weak = this.weak_self.clone();
            move |_| {
                if let Some(this) = weak.upgrade() {
                    this.show_message();
                }
            }
        }));

        this.base.add(context_menu_service.on_did_show_context_menu().listen({
            let weak = this.weak_self.clone();
            move |_| {
                if let Some(this) = weak.upgrade() {
                    this.context_menu_open.set(true);
                }
            }
        }));
        this.base.add(context_menu_service.on_did_hide_context_menu().listen({
            let weak = this.weak_self.clone();
            move |_| {
                let Some(this) = weak.upgrade() else { return };
                this.context_menu_open.set(false);
                if this.blur_pending.replace(false) {
                    Self::check_blur(this.weak_self.clone());
                }
            }
        }));

        this.base.add(input_box.on_did_blur().listen({
            let weak = this.weak_self.clone();
            move |_| Self::check_blur(weak.clone())
        }));

        this
    }

    /// `inputBox`.
    pub fn input_box(&self) -> &Rc<InputBox> {
        &self.input_box
    }

    /// O laço de `BLUR`: espera o próximo ciclo; com um menu de contexto aberto, adia até ele fechar; se o foco voltou
    /// ao campo, mantém a edição; senão termina com o valor se ele é válido.
    fn check_blur(weak: Weak<InlineEditInputBox>) {
        runtime::set_timeout(
            move || {
                let Some(this) = weak.upgrade() else { return };
                if this.base.is_disposed() {
                    return;
                }

                if this.context_menu_open.get() {
                    // Keep editing while a context menu of the input box is open
                    this.blur_pending.set(true);
                } else if this.ui.focused() != Some(this.input_box.input_element()) {
                    let valid = this.input_box.is_input_valid();
                    this.finish(valid);
                }
            },
            0,
        );
    }

    /// Shows non-error validation messages, errors are shown by the input box validation.
    fn show_message(&self) {
        if !self.input_box.is_input_valid() {
            return;
        }

        match (self.editable_data.validation_message)(&self.input_box.value()) {
            Some(message) => {
                let message_type = match message.severity {
                    Severity::Info => MessageType::Info,
                    Severity::Warning => MessageType::Warning,
                    _ => MessageType::Error,
                };
                self.input_box.show_message(Message { content: Some(message.content), format_content: true, r#type: Some(message_type) }, false);
            }
            None => self.input_box.hide_message(),
        }
    }

    fn finish(&self, success: bool) {
        if self.base.is_disposed() {
            return;
        }

        let value = self.input_box.value();
        self.dispose();
        // O resultado do `onFinish` não é esperado, como no original.
        let _ = (self.editable_data.on_finish)(value, success);
    }
}

impl IDisposable for InlineEditInputBox {
    fn dispose(&self) {
        if self.base.is_disposed() {
            return;
        }

        let element = self.input_box.element();
        self.base.dispose();
        self.ui.tree_mut().remove(element);
    }
}
