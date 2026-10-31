//! The right half's Magic-layer Bootloader and Reset keys.
//!
//! RMK runs keyboard actions on the central, but ZMK's `&bootloader` and `&sys_reset`
//! act on the half they are pressed on. Without this the right half couldn't reach
//! its bootloader from the keyboard.

use cortex_m::peripheral::SCB;
use rmk::boot::jump_to_bootloader;
use rmk::event::{KeyboardEvent, KeyboardEventSource, KeyPos, LayerChangeEvent};
use rmk::macros::processor;

const MAGIC_LAYER: u8 = 3;
/// Columns are local to this half: its outer column is 6.
const BOOTLOADER_KEY: KeyPos = KeyPos { row: 1, col: 6 };
const RESET_KEY: KeyPos = KeyPos { row: 2, col: 6 };

#[processor(subscribe = [KeyboardEvent, LayerChangeEvent])]
pub struct RightKeys {
    layer: u8,
}

impl RightKeys {
    pub fn new() -> Self {
        Self { layer: 0 }
    }

    async fn on_layer_change_event(&mut self, event: LayerChangeEvent) {
        self.layer = event.0;
    }

    async fn on_keyboard_event(&mut self, event: KeyboardEvent) {
        let KeyboardEventSource::Key(position) = event.source else {
            return;
        };
        if !event.pressed || self.layer != MAGIC_LAYER {
            return;
        }
        if position == BOOTLOADER_KEY {
            jump_to_bootloader();
        } else if position == RESET_KEY {
            SCB::sys_reset();
        }
    }
}
