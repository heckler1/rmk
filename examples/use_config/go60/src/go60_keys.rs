//! Behaviors the keymap needs that have no keycode: timed modifier latches and
//! absolute output selection.

use embassy_time::{Duration, Instant};
use rmk::event::{ActionEvent, ConnectionType, KeyboardEvent, publish_event_async};
use rmk::macros::processor;
use rmk::processor::DeadlineProcessor;

use rmk::types::action::Action;
use rmk::types::keycode::HidKeyCode;

const LATCH_TIME: Duration = Duration::from_secs(10);
/// `User16..=User19`, in the order of `TD(1..=4)` in keyboard.toml.
const LATCH_FIRST_ID: u8 = 16;
const LATCHED_MODIFIERS: [HidKeyCode; 4] = [
    HidKeyCode::LCtrl,
    HidKeyCode::LAlt,
    HidKeyCode::LGui,
    HidKeyCode::LShift,
];
/// ZMK's `&out OUT_USB`.
const OUTPUT_USB_ID: u8 = 20;
/// The profile keys `User0..=User3` also select BLE output, like ZMK's `&bt_0..3` macros.
const BLE_PROFILES: u8 = 4;
/// The hidden keymap position holding the output toggle.
const OUTPUT_TOGGLE_POS: (u8, u8) = (4, 0);

#[processor(subscribe = [ActionEvent], deadline)]
pub struct Go60Keys {
    latched_until: [Option<Instant>; LATCHED_MODIFIERS.len()],
}

impl Go60Keys {
    pub fn new() -> Self {
        Self {
            latched_until: [None; LATCHED_MODIFIERS.len()],
        }
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
            _ if (LATCH_FIRST_ID..LATCH_FIRST_ID + LATCHED_MODIFIERS.len() as u8).contains(&id) => {
                let index = (id - LATCH_FIRST_ID) as usize;
                let latching = self.latched_until[index].is_none();
                self.latched_until[index] = latching.then(|| Instant::now() + LATCH_TIME);
                publish_event_async(KeyboardEvent::keycode(LATCHED_MODIFIERS[index], latching)).await;
            }
            _ => {}
        }
    }
}

impl DeadlineProcessor for Go60Keys {
    fn deadline(&self) -> Option<Instant> {
        self.latched_until.iter().flatten().min().copied()
    }

    async fn on_deadline(&mut self) {
        let now = Instant::now();
        for (until, modifier) in self.latched_until.iter_mut().zip(LATCHED_MODIFIERS) {
            if until.is_some_and(|until| until <= now) {
                *until = None;
                publish_event_async(KeyboardEvent::keycode(modifier, false)).await;
            }
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
