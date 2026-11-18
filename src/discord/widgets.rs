//! Building blocks for panels and forms: buttons, rows, labelled fields, and reading a form back.

use twilight_model::application::interaction::modal::ModalInteractionComponent;
use twilight_model::channel::message::component::{ActionRow, Button, ButtonStyle, Component, Label, TextInput, TextInputStyle};

pub(crate) fn button(custom_id: String, label: &str, style: ButtonStyle) -> Component {
    Component::Button(Button { custom_id: Some(custom_id), disabled: false, emoji: None, id: None, label: Some(label.to_owned()), sku_id: None, style, url: None })
}

pub(crate) const fn row(components: Vec<Component>) -> Component {
    Component::ActionRow(ActionRow { components, id: None })
}

/// A form field: a label over one control.
pub(crate) fn labelled(label: &str, description: Option<&str>, control: Component) -> Component {
    Component::Label(Label { component: Box::new(control), description: description.map(str::to_owned), id: None, label: label.to_owned() })
}

/// How much a text field may hold, and whether it must be filled.
pub(crate) struct TextSpec<'a> {
    pub(crate) custom_id: &'a str,
    pub(crate) style: TextInputStyle,
    pub(crate) max: u16,
    pub(crate) value: Option<String>,
    pub(crate) placeholder: Option<&'a str>,
    pub(crate) required: bool,
}

// TextInput's own label is deprecated in favour of the Label around it; it is left empty.
#[allow(deprecated)]
pub(crate) fn text_input(spec: TextSpec) -> Component {
    Component::TextInput(TextInput {
        custom_id: spec.custom_id.to_owned(),
        id: None,
        label: None,
        max_length: Some(spec.max),
        min_length: Some(u16::from(spec.required)),
        placeholder: spec.placeholder.map(str::to_owned),
        required: Some(spec.required),
        style: spec.style,
        value: spec.value,
    })
}

/// A submitted field by its custom id, inside labels or rows: a text input's text, or a menu's first pick.
pub(crate) fn field(components: &[ModalInteractionComponent], id: &str) -> Option<String> {
    components.iter().find_map(|component| match component {
        ModalInteractionComponent::Label(label) => field(std::slice::from_ref(&label.component), id),
        ModalInteractionComponent::ActionRow(row) => field(&row.components, id),
        ModalInteractionComponent::TextInput(input) if input.custom_id == id => Some(input.value.clone()),
        ModalInteractionComponent::StringSelect(menu) if menu.custom_id == id => menu.values.first().cloned(),
        ModalInteractionComponent::ChannelSelect(menu) if menu.custom_id == id => menu.values.first().map(ToString::to_string),
        ModalInteractionComponent::UserSelect(menu) if menu.custom_id == id => menu.values.first().map(ToString::to_string),
        _ => None,
    })
}

