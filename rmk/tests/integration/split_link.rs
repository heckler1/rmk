//! A split link going down while keys are held, which a scenario cannot express:
//! its input is the matrix, and these keys arrive from a peripheral.

use rmk::event::KeyboardEvent;
use rmk::split::central::set_peripheral_connected;
use rmk::test_support::{forward_peripheral_key, test_block_on};
use rmk::types::keycode::HidKeyCode;
use rmk::types::modifier::ModifierCombination;
use rmk::{k, mt};

use crate::simulator::SimKeyboard;

const PERIPHERAL: usize = 0;
const LSHIFT: u8 = ModifierCombination::LSHIFT.into_bits();

trait PeripheralSteps {
    fn peripheral_press(&mut self, row: u8, col: u8) -> &mut Self;
    fn peripheral_release(&mut self, row: u8, col: u8) -> &mut Self;
    fn link_up(&mut self) -> &mut Self;
    fn link_lost(&mut self) -> &mut Self;
}

impl PeripheralSteps for SimKeyboard {
    fn peripheral_press(&mut self, row: u8, col: u8) -> &mut Self {
        self.publish(forward_peripheral_key(PERIPHERAL, KeyboardEvent::key(row, col, true)))
    }

    fn peripheral_release(&mut self, row: u8, col: u8) -> &mut Self {
        self.publish(forward_peripheral_key(PERIPHERAL, KeyboardEvent::key(row, col, false)))
    }

    fn link_up(&mut self) -> &mut Self {
        self.publish(async { set_peripheral_connected(PERIPHERAL, true) })
    }

    fn link_lost(&mut self) -> &mut Self {
        self.publish(async { set_peripheral_connected(PERIPHERAL, false) })
    }
}

// Control for the link-loss cases: without one the key is released by the
// peripheral, and that release has to keep working.
#[test]
fn release_over_a_live_link_reaches_the_keyboard() {
    test_block_on(async {
        let mut keyboard = SimKeyboard::builder([[[k!(A)]]]).build().await;
        keyboard
            .link_up()
            .peripheral_press(0, 0)
            .expect_keys([HidKeyCode::A])
            .peripheral_release(0, 0)
            .expect_keys([])
            .run()
            .await;
    });
}

// The keys stayed in every report after the link dropped, repeating on the host
// until the peripheral happened to reconnect and release them.
#[test]
fn link_loss_releases_held_keys() {
    test_block_on(async {
        let mut keyboard = SimKeyboard::builder([[[k!(A), k!(B), k!(LShift)]]]).build().await;
        keyboard
            .link_up()
            .peripheral_press(0, 0)
            .expect_keys([HidKeyCode::A])
            .peripheral_press(0, 1)
            .expect_keys([HidKeyCode::A, HidKeyCode::B])
            .peripheral_press(0, 2)
            .expect_keys_with_mods(LSHIFT, [HidKeyCode::A, HidKeyCode::B])
            .peripheral_release(0, 2)
            .expect_keys([HidKeyCode::A, HidKeyCode::B])
            .link_lost()
            .expect_keys([HidKeyCode::B])
            .expect_keys([])
            .run()
            .await;
    });
}

// A modifier that stuck here applied to everything typed on the other half.
#[test]
fn link_loss_releases_a_decided_hold() {
    test_block_on(async {
        let mut keyboard = SimKeyboard::builder([[[mt!(A, ModifierCombination::LSHIFT)]]])
            .build()
            .await;
        keyboard
            .link_up()
            .peripheral_press(0, 0)
            .delay(300)
            .expect_keys_with_mods(LSHIFT, [])
            .link_lost()
            .expect_keys([])
            .run()
            .await;
    });
}

// The synthesized release is an ordinary release, so a key still undecided
// resolves the way letting go of it would: as a tap, matching ZMK.
#[test]
fn link_loss_taps_an_undecided_morse_key() {
    test_block_on(async {
        let mut keyboard = SimKeyboard::builder([[[mt!(A, ModifierCombination::LSHIFT)]]])
            .build()
            .await;
        keyboard
            .link_up()
            .peripheral_press(0, 0)
            .delay(20)
            .link_lost()
            .expect_keys([HidKeyCode::A])
            .expect_keys([])
            .run()
            .await;
    });
}

// A poll loop reports the same loss over and over.
#[test]
fn repeated_link_loss_releases_once() {
    test_block_on(async {
        let mut keyboard = SimKeyboard::builder([[[k!(A)]]]).build().await;
        keyboard
            .link_up()
            .peripheral_press(0, 0)
            .expect_keys([HidKeyCode::A])
            .link_lost()
            .expect_keys([])
            .link_lost()
            .link_lost()
            .expect_no_report(100)
            .run()
            .await;
    });
}

// The read racing the loss report: the press lands after the transition, so
// only a later report can release it.
#[test]
fn repeated_link_loss_releases_a_late_press() {
    test_block_on(async {
        let mut keyboard = SimKeyboard::builder([[[k!(A)]]]).build().await;
        keyboard
            .link_up()
            .link_lost()
            .peripheral_press(0, 0)
            .expect_keys([HidKeyCode::A])
            .link_lost()
            .expect_keys([])
            .run()
            .await;
    });
}

// The peripheral still owes this release after reconnecting. Delivered, it would
// take the key away from whatever pressed it since.
#[test]
fn release_of_a_key_lost_with_the_link_is_dropped() {
    test_block_on(async {
        let mut keyboard = SimKeyboard::builder([[[k!(A), k!(A)]]]).build().await;
        keyboard
            .link_up()
            .peripheral_press(0, 0)
            .expect_keys([HidKeyCode::A])
            .link_lost()
            .expect_keys([])
            .link_up()
            .press(0, 1)
            .expect_keys([HidKeyCode::A])
            .peripheral_release(0, 0)
            .expect_no_report(100)
            .release(0, 1)
            .expect_keys([])
            .run()
            .await;
    });
}
