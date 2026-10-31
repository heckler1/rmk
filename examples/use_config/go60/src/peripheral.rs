#![no_main]
#![no_std]

mod cirque;
mod right_keys;
mod underglow;
mod wired;

use embassy_nrf::{buffered_uarte, peripherals, spim};
use rmk::macros::rmk_peripheral;

embassy_nrf::bind_interrupts!(struct BoardIrqs {
    SPI2 => spim::InterruptHandler<peripherals::SPI2>;
    UARTE0 => buffered_uarte::InterruptHandler<peripherals::UARTE0>;
});

#[rmk_peripheral(id = 0)]
mod keyboard_peripheral {
    // The TRRS cable selects the link, as on MoErgo's ZMK.
    #[Override(split_transport)]
    fn split_transport() {
        let cable = ::embassy_nrf::gpio::Input::new(p.P1_10, ::embassy_nrf::gpio::Pull::None);
        let mut bus_parts = crate::wired::BusParts::new(
            p.UARTE0,
            p.TIMER1,
            p.PPI_CH0,
            p.PPI_CH1,
            p.PPI_GROUP0,
            p.P1_09,
            p.P1_08,
            p.P0_11,
        );
        async move {
            use ::embassy_futures::join::join3;

            ::embassy_time::Timer::after_millis(10).await;
            let wired = cable.is_high();
            let watch_cable = crate::wired::reboot_on_cable_change(cable, wired);
            if wired {
                join3(
                    watch_cable,
                    crate::wired::serve_peripheral(bus_parts.bus()),
                    ::rmk::split::peripheral::run_serial_split_peripheral(crate::wired::peripheral_port()),
                )
                .await;
            } else {
                ::embassy_futures::join::join(
                    watch_cable,
                    ::rmk::split::peripheral::run_rmk_split_peripheral(0, ble_controller, ble_addr),
                )
                .await;
            }
        }
    }

    #[register_processor]
    fn right_underglow() -> crate::underglow::RightUnderglow<'_> {
        crate::underglow::RightUnderglow::new(crate::underglow::Strip::new(p.PWM1, p.P0_27, p.P1_11), p.P1_15)
    }

    #[register_processor]
    fn right_keys() -> crate::right_keys::RightKeys {
        crate::right_keys::RightKeys::new()
    }

    // The right pad moves the cursor. Its events reach the central's PointingProcessor
    // over the split link.
    #[register_processor]
    fn right_trackpad() -> crate::cirque::Cirque<'_> {
        use ::embassy_nrf::gpio::{Input, Level, Output, OutputDrive, Pull};
        use ::embassy_nrf::spim::{self, Spim};
        use ::rmk::event::Axis;
        use ::rmk::types::keycode::HidKeyCode;

        let mut spi_config = spim::Config::default();
        spi_config.frequency = spim::Frequency::M1;
        spi_config.mode = spim::MODE_1;
        crate::cirque::Cirque::new(
            Spim::new(p.SPI2, crate::BoardIrqs, p.P0_19, p.P0_21, p.P0_22, spi_config),
            Output::new(p.P0_25, Level::High, OutputDrive::Standard),
            Input::new(p.P0_23, Pull::Down),
            crate::cirque::CirqueRole {
                device_id: 1,
                axes: (Axis::X, Axis::Y),
                buttons: [HidKeyCode::MouseBtn1, HidKeyCode::MouseBtn2, HidKeyCode::MouseBtn3],
                cursor_layer: None,
            },
        )
    }
}
