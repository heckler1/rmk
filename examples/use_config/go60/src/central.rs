#![no_main]
#![no_std]

mod cirque;
mod go60_keys;
mod trackpad_speed;
mod underglow;
mod wired;

use embassy_nrf::{buffered_uarte, peripherals, spim};
use rmk::macros::rmk_central;

embassy_nrf::bind_interrupts!(struct BoardIrqs {
    SPI2 => spim::InterruptHandler<peripherals::SPI2>;
    UARTE0 => buffered_uarte::InterruptHandler<peripherals::UARTE0>;
});

#[rmk_central]
mod keyboard_central {
    #[register_processor]
    fn left_underglow() -> crate::underglow::LeftUnderglow<'_> {
        crate::underglow::LeftUnderglow::new(crate::underglow::Strip::new(p.PWM1, p.P0_27, p.P1_11), p.PWM0, p.P1_15)
    }

    #[register_processor]
    fn go60_keys() -> crate::go60_keys::Go60Keys {
        crate::go60_keys::Go60Keys::new()
    }

    #[register_processor]
    fn trackpad_speed() -> crate::trackpad_speed::TrackpadSpeed {
        crate::trackpad_speed::TrackpadSpeed::new()
    }

    #[register_processor]
    fn pointing() -> ::rmk::input_device::pointing::PointingProcessor<'_> {
        use ::rmk::input_device::pointing::{PointingProcessor, PointingProcessorConfig, ScrollAxis, ScrollConfig};

        // ZMK's `zip_xy_scaler 1 8` on the left pad. ZMK's wheel rises for +Y, RMK's falls.
        let pan = ScrollAxis {
            multiplier: 1,
            divisor: 8,
            invert: false,
        };
        let wheel = ScrollAxis { invert: true, ..pan };
        let config = PointingProcessorConfig {
            device_scroll: ScrollConfig { wheel, pan },
            ..Default::default()
        };
        let mut processor = PointingProcessor::new(&keymap, config);
        processor.set_pointing_mode(crate::trackpad_speed::default_mode());
        processor
    }

    #[register_processor]
    fn wired_link() -> crate::wired::WiredCentral<'_> {
        crate::wired::WiredCentral::new(
            crate::wired::BusParts::new(
                p.UARTE0,
                p.TIMER1,
                p.PPI_CH0,
                p.PPI_CH1,
                p.PPI_GROUP0,
                p.P1_09,
                p.P1_08,
                p.P0_11,
            ),
            p.P1_10,
        )
    }

    // The left pad scrolls, and its tap is a right click.
    #[register_processor]
    fn left_trackpad() -> crate::cirque::Cirque<'_> {
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
                device_id: 0,
                axes: (Axis::H, Axis::V),
                buttons: [HidKeyCode::MouseBtn2, HidKeyCode::MouseBtn2, HidKeyCode::MouseBtn3],
                // Factory, as `layer_4` on `cirque_lh_listener` in MoErgo's default keymap.
                cursor_layer: Some(4),
            },
        )
    }
}
