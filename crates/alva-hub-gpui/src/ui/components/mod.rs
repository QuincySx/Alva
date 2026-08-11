// INPUT:  Shared Hub view data and gpui-component primitives
// OUTPUT: Reusable ALVA Hub presentation components
// POS:    Component-first layer between product state and page composition

mod cards;
mod chat;
mod forms;
mod navigation;
mod surface;

pub use cards::{
    HubExtensionCard, HubExtensionDetail, HubQuickStartCard, HubTemplateCard, HubToolPlaceholder,
};
pub use chat::{HubActivityRow, HubComposer};
pub use forms::{HubAutomationForm, HubProviderForm};
pub use navigation::{HubProjectPicker, HubSessionItem, HubWorkbenchTabMenu};
pub use surface::{HubEmptyState, HubSurfaceHeader};
