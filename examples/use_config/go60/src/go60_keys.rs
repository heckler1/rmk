//! Absolute output selection, which has no keycode.

use rmk::event::{ActionEvent, ConnectionType, KeyboardEvent, publish_event_async};
use rmk::macros::processor;

use rmk::types::action::Action;

/// ZMK's `&out OUT_USB`.
const OUTPUT_USB_ID: u8 = 20;
/// The profile keys `User0..=User3` also select BLE output, like ZMK's `&bt_0..3` macros.
const BLE_PROFILES: u8 = 4;
/// The hidden keymap position holding the output toggle.
const OUTPUT_TOGGLE_POS: (u8, u8) = (4, 0);

#[processor(subscribe = [ActionEvent])]
pub struct Go60Keys {}

impl Go60Keys {
    pub fn new() -> Self {
        Self {}
    }

    async fn on_action_event(&mut self, event: ActionEvent) {
        let Action::User(id) = event.action else {
            return;
        };
        if !event.keyboard_event.pressed {
            return;
        }
        match id {
            0..BLE_PROFILES => select_output(ConnectionType::Ble).await,
            OUTPUT_USB_ID => select_output(ConnectionType::Usb).await,
            _ => {}
        }
    }
}

async fn select_output(wanted: ConnectionType) {
    if rmk::state::current_connection_status().preferred != wanted {
        let (row, col) = OUTPUT_TOGGLE_POS;
        publish_event_async(KeyboardEvent::key(row, col, true)).await;
        publish_event_async(KeyboardEvent::key(row, col, false)).await;
    }
}
