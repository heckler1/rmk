//! The abstracted driver layer of the split keyboard.
//!
use core::cell::{Cell, RefCell};

use embassy_futures::select::{Either, select};
use embassy_sync::blocking_mutex::Mutex as BlockingMutex;
use futures::FutureExt;
use rmk_types::battery::BatteryStatus;
#[cfg(feature = "rynk")]
use rmk_types::protocol::rynk::PeripheralStatus;

use super::{PeripheralMatrixConfig, SplitMessage};
#[cfg(feature = "dfu_split")]
use crate::event::DfuCmdEvent;
#[cfg(feature = "_ble")]
use crate::event::{BatteryStatusEvent, PeripheralBatteryEvent};
use crate::event::{
    KeyboardEvent, KeyboardEventSource, PeripheralConnectedEvent, SubscribableEvent, publish_event, publish_event_async,
};

#[derive(Debug, Clone, Copy)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub(crate) enum SplitDriverError {
    SerialError,
    EmptyMessage,
    DeserializeError,
    SerializeError,
    BleError(u8),
    Disconnected,
}

/// Split message reader from other split devices
pub(crate) trait SplitReader {
    async fn read(&mut self) -> Result<SplitMessage, SplitDriverError>;
}

/// Split message writer to other split devices
pub(crate) trait SplitWriter {
    async fn write(&mut self, message: &SplitMessage) -> Result<usize, SplitDriverError>;
}

/// Live per-peripheral status. Latched here in the transport-agnostic split
/// layer so host services can read a current snapshot at any time, even when
/// no host session was active when the change happened. Wired peripherals
/// never report a battery, so theirs stays `Unavailable`.
#[derive(Copy, Clone, PartialEq, Eq)]
struct PeripheralSlot {
    connected: bool,
    battery: BatteryStatus,
}

static PERIPHERAL_SLOTS: BlockingMutex<crate::RawMutex, Cell<[PeripheralSlot; crate::SPLIT_PERIPHERALS_NUM]>> =
    BlockingMutex::new(Cell::new(
        [PeripheralSlot {
            connected: false,
            battery: BatteryStatus::Unavailable,
        }; crate::SPLIT_PERIPHERALS_NUM],
    ));

/// How many keys one peripheral can hold down at once.
const MAX_HELD_KEYS: usize = 16;

type HeldKeys = heapless::Vec<KeyboardEventSource, MAX_HELD_KEYS>;

/// What each peripheral holds down, in the central's coordinates. It lives here
/// rather than in `PeripheralManager` because a link can be reported lost from
/// outside the manager, which also ends before the BLE path reports it.
static HELD_KEYS: BlockingMutex<crate::RawMutex, RefCell<[HeldKeys; crate::SPLIT_PERIPHERALS_NUM]>> =
    BlockingMutex::new(RefCell::new([const { HeldKeys::new() }; crate::SPLIT_PERIPHERALS_NUM]));

/// Read-modify-write peripheral `id`'s slot. Returns `false` when `id` is out
/// of range or the slot didn't change, so callers skip publishing.
fn update_slot(id: usize, f: impl FnOnce(&mut PeripheralSlot)) -> bool {
    PERIPHERAL_SLOTS.lock(|slots| {
        let mut all = slots.get();
        let Some(slot) = all.get_mut(id) else {
            return false;
        };
        let prev = *slot;
        f(slot);
        if *slot == prev {
            return false;
        }
        slots.set(all);
        true
    })
}

/// Latch peripheral `id`'s connected state and broadcast the change.
///
/// A lost link can't deliver the releases of what the peripheral holds, so they
/// are synthesized here. That runs on every `false`, not just the transition, so
/// a press read just after the link was reported lost is released by the next
/// report from a transport that repeats them.
pub(crate) fn set_peripheral_connected(id: usize, connected: bool) {
    if update_slot(id, |s| s.connected = connected) {
        publish_event(PeripheralConnectedEvent { id, connected });
    }
    if !connected {
        let held = HELD_KEYS.lock(|held| held.borrow_mut().get_mut(id).map(core::mem::take));
        for &source in held.iter().flatten() {
            publish_event(KeyboardEvent { pressed: false, source });
        }
    }
}

/// Forward a key event from peripheral `id`, already in the central's coordinates.
///
/// Every press that reaches the keyboard is in `HELD_KEYS`, so a link loss can
/// release it. That leaves two kinds of event to drop: a press past
/// `MAX_HELD_KEYS`, and a release with no press, which is a key whose release
/// was synthesized and that the peripheral let go of once the link came back.
pub(crate) async fn forward_peripheral_key(id: usize, event: KeyboardEvent) {
    let forward = HELD_KEYS.lock(|held| {
        let mut held = held.borrow_mut();
        let Some(keys) = held.get_mut(id) else {
            return false;
        };
        match keys.iter().position(|source| *source == event.source) {
            Some(_) if event.pressed => true,
            Some(index) => {
                keys.swap_remove(index);
                true
            }
            None => event.pressed && keys.push(event.source).is_ok(),
        }
    });
    if forward {
        publish_event_async(event).await;
    } else {
        debug!("Dropping key event from peripheral {}: {:?}", id, event);
    }
}

/// Latch peripheral `id`'s battery status and broadcast the change.
#[cfg(feature = "_ble")]
pub(crate) fn set_peripheral_battery(id: usize, battery: BatteryStatus) {
    if update_slot(id, |s| s.battery = battery) {
        publish_event(PeripheralBatteryEvent {
            id,
            state: BatteryStatusEvent(battery),
        });
    }
}

/// Latest battery status reported by peripheral `id`.
#[cfg(feature = "_ble")]
pub(crate) fn current_peripheral_battery_status(id: usize) -> Option<BatteryStatus> {
    PERIPHERAL_SLOTS.lock(|slots| slots.get().get(id).map(|slot| slot.battery))
}

/// Latest snapshot for peripheral `id`, or `None` when `id` is out of range.
#[cfg(feature = "rynk")]
pub(crate) fn current_peripheral_status(id: usize) -> Option<PeripheralStatus> {
    PERIPHERAL_SLOTS.lock(|slots| {
        slots.get().get(id).map(|s| PeripheralStatus {
            connected: s.connected,
            battery: s.battery,
        })
    })
}

/// PeripheralManager runs on the central and manages communication with one split peripheral.
/// It publishes input events from that peripheral and sends the central's state changes back.
pub(crate) struct PeripheralManager<T: SplitReader + SplitWriter> {
    /// Receiver
    pub(crate) transceiver: T,
    /// Peripheral id
    pub(crate) id: usize,
    /// This peripheral's matrix size and placement in the central's keymap
    matrix_config: PeripheralMatrixConfig,
    #[cfg(feature = "dfu_split")]
    pub(crate) passthrough_crc: crate::crc32::Crc32,
    /// Whether to skip hash comparison and always flash firmware.
    #[cfg(feature = "dfu_split")]
    pub(crate) policy: crate::split::dfu::UpdatePolicy,
    /// Set after a chunk fails all retries — aborts subsequent writes.
    #[cfg(feature = "dfu_split")]
    pub(crate) dfu_aborted: bool,
}

impl<T: SplitReader + SplitWriter> PeripheralManager<T> {
    pub(crate) fn new(
        transceiver: T,
        id: usize,
        matrix_config: PeripheralMatrixConfig,
        #[cfg(feature = "dfu_split")] policy: crate::split::dfu::UpdatePolicy,
    ) -> Self {
        Self {
            transceiver,
            matrix_config,
            id,
            #[cfg(feature = "dfu_split")]
            passthrough_crc: crate::crc32::Crc32::new(),
            #[cfg(feature = "dfu_split")]
            policy,
            #[cfg(feature = "dfu_split")]
            dfu_aborted: false,
        }
    }

    /// Send a message to the peripheral, returning Err on disconnect.
    pub(crate) async fn send(&mut self, msg: &SplitMessage) -> Result<(), ()> {
        debug!("Sending message to peripheral {}: {:?}", self.id, msg);
        match self.transceiver.write(msg).await {
            Ok(_) => Ok(()),
            Err(SplitDriverError::Disconnected) => Err(()),
            Err(e) => {
                error!("SplitDriver write error: {:?}", e);
                Err(())
            }
        }
    }

    /// Run the manager.
    ///
    /// The manager receives from the peripheral and publishes input events.
    /// It also syncs the central's `ConnectionStatus` to the peripheral on every
    /// change as an informational signal
    pub(crate) async fn run(mut self) {
        use crate::event::EventSubscriber;

        let mut indicator_sub = crate::event::LedIndicatorEvent::subscriber();
        let mut layer_sub = crate::event::LayerChangeEvent::subscriber();
        // Subscribe before the initial send so any change racing past the
        // snapshot is still delivered to us.
        let mut connection_sub = crate::event::ConnectionStatusChangeEvent::subscriber();
        #[cfg(feature = "_ble")]
        let mut clear_peer_sub = crate::event::ClearPeerEvent::subscriber();
        #[cfg(feature = "display")]
        let mut wpm_sub = crate::event::WpmUpdateEvent::subscriber();
        #[cfg(feature = "display")]
        let mut modifier_sub = crate::event::ModifierEvent::subscriber();
        let mut sleep_sub = crate::event::SleepStateEvent::subscriber();
        #[cfg(feature = "dfu_split")]
        let mut dfu_sub = DfuCmdEvent::subscriber();

        // Send the current state when the session starts.
        for message in [
            SplitMessage::ConnectionStatus(crate::state::current_connection_status()),
            SplitMessage::KeyboardIndicator(crate::keyboard::current_led_indicator().into_bits()),
            SplitMessage::SleepState(crate::state::current_sleep_state()),
        ] {
            if self.send(&message).await.is_err() {
                return;
            }
        }

        #[cfg(feature = "dfu_split")]
        self.check_firmware_update().await;

        loop {
            // Use select_biased_with_feature to handle feature-gated subscriber arms
            let next_event_to_peri = async {
                crate::select_biased_with_feature! {
                    e = indicator_sub.next_event().fuse() => SplitMessage::KeyboardIndicator(e.0.into_bits()),
                    e = layer_sub.next_event().fuse() => SplitMessage::Layer(e.0),
                    e = connection_sub.next_event().fuse() => SplitMessage::ConnectionStatus(e.0),
                    with_feature("_ble"): _ = clear_peer_sub.next_event().fuse() => {
                        #[cfg(feature = "storage")]
                        {
                            use {crate::split::ble::PeerAddress, crate::storage::{StorageItem, store_unchecked}};
                            store_unchecked(StorageItem::PeerAddress(PeerAddress::new(self.id as u8, false, [0; 6]))).await;
                        }
                        SplitMessage::ClearPeer
                    },
                    e = sleep_sub.next_event().fuse() => SplitMessage::SleepState(e.0),
                    with_feature("display"): e = wpm_sub.next_event().fuse() => SplitMessage::Wpm(e.0),
                    with_feature("display"): e = modifier_sub.next_event().fuse() => SplitMessage::Modifier(e.modifier.into_bits()),
                }
            };

            #[cfg(feature = "dfu_split")]
            let event_or_signal = select(next_event_to_peri, dfu_sub.next_message_pure()).fuse();
            #[cfg(not(feature = "dfu_split"))]
            let event_or_signal = next_event_to_peri;

            match select(self.transceiver.read(), event_or_signal).await {
                Either::First(read_result) => match read_result {
                    #[cfg(feature = "dfu_split")]
                    Ok(SplitMessage::FirmwareHashResponse(hash)) => {
                        self.handle_proactive_hash(hash).await;
                    }
                    Ok(split_message) => self.process_peripheral_message(split_message).await,
                    Err(e) => error!("Peripheral message read error: {:?}", e),
                },
                #[cfg(feature = "dfu_split")]
                Either::Second(Either::First(msg)) => {
                    if self.send(&msg).await.is_err() {
                        return;
                    }
                }
                #[cfg(feature = "dfu_split")]
                Either::Second(Either::Second(cmd_event)) => {
                    self.handle_dfu_event(cmd_event).await;
                }
                #[cfg(not(feature = "dfu_split"))]
                Either::Second(msg) => {
                    if self.send(&msg).await.is_err() {
                        return;
                    }
                }
            }
        }
    }

    /// Process a single message from the peripheral.
    async fn process_peripheral_message(&self, split_message: SplitMessage) {
        trace!("Got message from peripheral: {:?}", split_message);
        match split_message {
            SplitMessage::Key(e) => match e.source {
                KeyboardEventSource::Key(key_pos) => {
                    // Verify the row/col
                    if key_pos.row >= self.matrix_config.rows || key_pos.col >= self.matrix_config.cols {
                        error!("Invalid peripheral row/col: {} {}", key_pos.row, key_pos.col);
                        return;
                    }
                    forward_peripheral_key(
                        self.id,
                        KeyboardEvent::key(
                            key_pos.row + self.matrix_config.row_offset,
                            key_pos.col + self.matrix_config.col_offset,
                            e.pressed,
                        ),
                    )
                    .await;
                }
                _ => forward_peripheral_key(self.id, e).await,
            },
            // Non-key events are drop-on-full to keep the split read loop responsive.
            SplitMessage::Pointing(e) => publish_event(e),
            #[cfg(feature = "_ble")]
            SplitMessage::BatteryStatus(state) => set_peripheral_battery(self.id, state.0),
            #[cfg(feature = "dfu_split")]
            SplitMessage::FirmwareHashResponse(hash) => {
                info!("dfu_split: stale hash response ({:#x}) in event loop", hash);
            }
            #[cfg(feature = "dfu_split")]
            SplitMessage::FirmwareChunkAck { offset, crc: _ } => {
                info!("dfu_split: stale chunk ack (offset {}) in event loop, ignoring", offset);
            }
            #[cfg(feature = "dfu_split")]
            SplitMessage::FirmwareUpdateConfirm => {
                info!("dfu_split: stale update confirm in event loop, ignoring");
            }
            _ => warn!("{:?} should not come from peripheral", split_message),
        }
    }
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "_ble")]
    use rmk_types::battery::ChargeState;

    use super::{MAX_HELD_KEYS, forward_peripheral_key, set_peripheral_connected};
    #[cfg(feature = "_ble")]
    use super::{current_peripheral_battery_status, set_peripheral_battery};
    use crate::event::{KeyboardEvent, SubscribableEvent};
    use crate::test_support::test_block_on;

    // Tracked and untracked presses must stay paired: an untracked one reaching
    // the keyboard would stick, as its release is dropped.
    #[test]
    fn press_past_the_held_limit_is_dropped() {
        test_block_on(async {
            let mut sub = KeyboardEvent::subscriber();
            for col in 0..=MAX_HELD_KEYS as u8 {
                forward_peripheral_key(0, KeyboardEvent::key(0, col, true)).await;
            }
            for col in 0..MAX_HELD_KEYS as u8 {
                assert_eq!(sub.next_message_pure().await, KeyboardEvent::key(0, col, true));
            }
            assert_eq!(sub.try_next_message_pure(), None);

            set_peripheral_connected(0, false);
            let mut released = 0;
            while let Some(event) = sub.try_next_message_pure() {
                assert!(!event.pressed);
                released += 1;
            }
            assert_eq!(released, MAX_HELD_KEYS);
        });
    }

    #[test]
    fn link_loss_of_an_unknown_peripheral_is_ignored() {
        set_peripheral_connected(crate::SPLIT_PERIPHERALS_NUM, false);
    }

    #[cfg(feature = "_ble")]
    #[test]
    fn caches_latest_peripheral_battery_status() {
        let status = rmk_types::battery::BatteryStatus::Available {
            charge_state: ChargeState::Discharging,
            level: Some(73),
        };

        set_peripheral_battery(0, status);

        assert_eq!(current_peripheral_battery_status(0), Some(status));
        assert_eq!(current_peripheral_battery_status(crate::SPLIT_PERIPHERALS_NUM), None);
    }
}
