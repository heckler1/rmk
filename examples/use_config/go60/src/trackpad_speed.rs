//! The right pad's speed on each layer, as in `cirque_rh_listener` in MoErgo's default
//! keymap. RMK's pointing modes aren't per layer, so this switches them as layers change.

use rmk::event::{LayerChangeEvent, PointingProcessorEvent, publish_event};
use rmk::input_device::pointing::{ALL_POINTING_DEVICES, CursorConfig, PointingMode};
use rmk::macros::processor;

const SYMBOL_NAV_LAYER: u8 = 2;
/// ZMK's `zip_xy_scaler 3 1`.
const CURSOR_SPEED: u8 = 3;
/// ZMK's `zip_xy_scaler 9 1` in `layer_2`.
const SYMBOL_NAV_SPEED: u8 = 9;

fn cursor(speed: u8) -> PointingMode {
    PointingMode::Cursor(CursorConfig {
        multiplier_x: speed,
        multiplier_y: speed,
        ..Default::default()
    })
}

pub fn default_mode() -> PointingMode {
    cursor(CURSOR_SPEED)
}

#[processor(subscribe = [LayerChangeEvent])]
pub struct TrackpadSpeed;

impl TrackpadSpeed {
    pub fn new() -> Self {
        Self
    }

    async fn on_layer_change_event(&mut self, event: LayerChangeEvent) {
        let speed = if event.0 == SYMBOL_NAV_LAYER {
            SYMBOL_NAV_SPEED
        } else {
            CURSOR_SPEED
        };
        publish_event(PointingProcessorEvent {
            device_id: ALL_POINTING_DEVICES,
            mode: cursor(speed),
        });
    }
}
