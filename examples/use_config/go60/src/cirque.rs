//! Cirque Pinnacle trackpad on SPI, ported from the driver MoErgo's ZMK uses
//! (petejohanson/cirque-input-module) with the settings from their go60_lh.dts and
//! go60_rh.dts: 1x sensitivity, rotated 90 degrees, Y inverted, no secondary tap.
//!
//! A pad that fails to initialize stops reporting; the keyboard keeps working.

use embassy_futures::select::{Either3, select3};
use embassy_nrf::gpio::{Input, Output};
use embassy_nrf::spim::Spim;
use embassy_time::Timer;
use rmk::event::{
    Axis, AxisEvent, AxisValType, EventSubscriber, KeyboardEvent, LayerChangeEvent, PointingEvent, SleepStateEvent,
    SubscribableEvent, publish_event_async,
};
use rmk::macros::input_device;
use rmk::types::keycode::HidKeyCode;

const READ: u8 = 0xA0;
const WRITE: u8 = 0x80;
const AUTOINC: u8 = 0xFC;
const FILLER: u8 = 0xFB;

const REG_STATUS1: u8 = 0x02;
const REG_SYS_CFG: u8 = 0x03;
const REG_FEED_CFG1: u8 = 0x04;
const REG_FEED_CFG2: u8 = 0x05;
const REG_CAL_CFG: u8 = 0x07;
const REG_Z_IDLE: u8 = 0x0A;
const REG_PACKET0: u8 = 0x12;
const REG_ERA_VALUE: u8 = 0x1B;
const REG_ERA_HIGH_BYTE: u8 = 0x1C;
const REG_ERA_LOW_BYTE: u8 = 0x1D;
const REG_ERA_CONTROL: u8 = 0x1E;

const ERA_CONTROL_READ: u8 = 0x01;
const ERA_CONTROL_WRITE: u8 = 0x02;
const ERA_X_AXIS_WIDE_Z_MIN: u16 = 0x0149;
const ERA_Y_AXIS_WIDE_Z_MIN: u16 = 0x0168;
const ERA_TRACKING_ADC_CONFIG: u16 = 0x0187;

const STATUS1_SW_DR: u8 = 1 << 2;
const SYS_CFG_RESET: u8 = 1 << 0;
const SYS_CFG_EN_SLEEP: u8 = 1 << 2;
const FEED_CFG1_EN_FEED: u8 = 1 << 0;
const FEED_CFG1_INV_Y: u8 = 1 << 7;
const FEED_CFG2_EN_IM: u8 = 1 << 0;
const FEED_CFG2_DIS_SEC: u8 = 1 << 2;
const FEED_CFG2_EN_BTN_SCRL: u8 = 1 << 6;
const FEED_CFG2_ROTATE_90: u8 = 1 << 7;
const PACKET0_BUTTONS: u8 = 0b111;
const PACKET0_X_SIGN: u8 = 1 << 4;
const PACKET0_Y_SIGN: u8 = 1 << 5;

/// How many times to poll a register the pad clears when it finishes.
const POLL_ATTEMPTS: usize = 200;

#[derive(Debug)]
struct Error;

/// What a pad reports. Scrolling goes out on the H/V axes, which `PointingProcessor`
/// scales through `device_scroll`.
pub struct CirqueRole {
    pub device_id: u8,
    pub axes: (Axis, Axis),
    /// The keycodes for the primary, secondary and auxiliary tap.
    pub buttons: [HidKeyCode; 3],
    /// A layer where this pad moves the cursor, whatever `axes` says.
    pub cursor_layer: Option<u8>,
}

#[input_device(publish = PointingEvent)]
pub struct Cirque<'d> {
    spi: Spim<'d>,
    cs: Output<'d>,
    data_ready: Input<'d>,
    role: CirqueRole,
    initialized: bool,
    pressed_buttons: u8,
    sleep_events: <SleepStateEvent as SubscribableEvent>::Subscriber,
    layer_events: <LayerChangeEvent as SubscribableEvent>::Subscriber,
    on_cursor_layer: bool,
}

impl<'d> Cirque<'d> {
    pub fn new(spi: Spim<'d>, cs: Output<'d>, data_ready: Input<'d>, role: CirqueRole) -> Self {
        Self {
            spi,
            cs,
            data_ready,
            role,
            initialized: false,
            pressed_buttons: 0,
            sleep_events: SleepStateEvent::subscriber(),
            layer_events: LayerChangeEvent::subscriber(),
            on_cursor_layer: false,
        }
    }

    async fn read<const N: usize>(&mut self, register: u8) -> Result<[u8; N], Error> {
        // The first three bytes clocked back are the command and two filler bytes.
        let mut tx = [AUTOINC; 32];
        let mut rx = [0; 32];
        tx[0] = READ | register;
        let len = N + 3;
        self.cs.set_low();
        let result = self.spi.transfer(&mut rx[..len], &tx[..len]).await;
        self.cs.set_high();
        result.map_err(|_| Error)?;
        let mut value = [0; N];
        value.copy_from_slice(&rx[3..len]);
        Ok(value)
    }

    async fn read_byte(&mut self, register: u8) -> Result<u8, Error> {
        Ok(self.read::<1>(register).await?[0])
    }

    async fn write(&mut self, register: u8, value: u8) -> Result<(), Error> {
        let tx = [WRITE | register, value];
        let mut rx = [0; 2];
        self.cs.set_low();
        let result = self.spi.transfer(&mut rx, &tx).await;
        self.cs.set_high();
        result.map_err(|_| Error)?;
        Timer::after_micros(50).await;
        if rx[1] == FILLER { Ok(()) } else { Err(Error) }
    }

    async fn clear_status(&mut self) -> Result<(), Error> {
        self.write(REG_STATUS1, 0).await
    }

    async fn wait_until_clear(&mut self, register: u8, mask: u8) -> Result<(), Error> {
        for _ in 0..POLL_ATTEMPTS {
            if self.read_byte(register).await? & mask == 0 {
                return Ok(());
            }
            Timer::after_millis(1).await;
        }
        Err(Error)
    }

    async fn era_start(&mut self, address: u16, control: u8) -> Result<(), Error> {
        self.write(REG_ERA_HIGH_BYTE, (address >> 8) as u8).await?;
        self.write(REG_ERA_LOW_BYTE, address as u8).await?;
        self.write(REG_ERA_CONTROL, control).await?;
        self.wait_until_clear(REG_ERA_CONTROL, 0xFF).await
    }

    async fn era_read(&mut self, address: u16) -> Result<u8, Error> {
        self.era_start(address, ERA_CONTROL_READ).await?;
        let value = self.read_byte(REG_ERA_VALUE).await?;
        self.clear_status().await?;
        Ok(value)
    }

    async fn era_write(&mut self, address: u16, value: u8) -> Result<(), Error> {
        self.write(REG_ERA_VALUE, value).await?;
        self.era_start(address, ERA_CONTROL_WRITE).await?;
        self.clear_status().await
    }

    async fn init(&mut self) -> Result<(), Error> {
        Timer::after_millis(10).await;
        self.clear_status().await?;
        self.write(REG_SYS_CFG, SYS_CFG_RESET).await?;
        Timer::after_millis(20).await;
        self.write(REG_Z_IDLE, 0x05).await?;

        // 1x tracking sensitivity
        let adc_config = self.era_read(ERA_TRACKING_ADC_CONFIG).await?;
        self.era_write(ERA_TRACKING_ADC_CONFIG, adc_config & 0x3F).await?;
        self.era_write(ERA_X_AXIS_WIDE_Z_MIN, 5).await?;
        self.era_write(ERA_Y_AXIS_WIDE_Z_MIN, 4).await?;

        let calibration = self.read_byte(REG_CAL_CFG).await?;
        self.write(REG_CAL_CFG, calibration | 0x01).await?;
        self.wait_until_clear(REG_CAL_CFG, 0x01).await?;

        self.write(
            REG_FEED_CFG2,
            FEED_CFG2_EN_IM | FEED_CFG2_EN_BTN_SCRL | FEED_CFG2_DIS_SEC | FEED_CFG2_ROTATE_90,
        )
        .await?;
        self.write(REG_FEED_CFG1, FEED_CFG1_EN_FEED | FEED_CFG1_INV_Y).await?;
        self.clear_status().await
    }

    async fn read_motion(&mut self) -> Result<Option<(i16, i16)>, Error> {
        let status = self.read_byte(REG_STATUS1).await?;
        if status == 0xFF || status & STATUS1_SW_DR == 0 {
            return Ok(None);
        }
        let [flags, x, y] = self.read::<3>(REG_PACKET0).await?;
        self.clear_status().await?;

        let buttons = flags & PACKET0_BUTTONS;
        for (bit, keycode) in self.role.buttons.into_iter().enumerate() {
            let mask = 1 << bit;
            if (buttons ^ self.pressed_buttons) & mask != 0 {
                publish_event_async(KeyboardEvent::keycode(keycode, buttons & mask != 0)).await;
            }
        }
        self.pressed_buttons = buttons;

        let signed = |value: u8, negative: bool| (if negative { value | 0x80 } else { value }) as i8 as i16;
        Ok(Some((
            signed(x, flags & PACKET0_X_SIGN != 0),
            signed(y, flags & PACKET0_Y_SIGN != 0),
        )))
    }

    async fn read_pointing_event(&mut self) -> PointingEvent {
        if !self.initialized {
            if self.init().await.is_err() {
                defmt::error!("Cirque {} failed to initialize", self.role.device_id);
                core::future::pending::<()>().await;
            }
            self.initialized = true;
        }
        loop {
            // MoErgo's ZMK never lets the pad sleep. Sleep slows its first touch, so it's
            // only allowed while the keyboard sleeps, not after each pause in use.
            let event = select3(
                self.data_ready.wait_for_high(),
                self.sleep_events.next_event(),
                self.layer_events.next_event(),
            );
            match event.await {
                Either3::First(()) => {}
                Either3::Second(sleep) => {
                    let sys_cfg = if sleep.0 { SYS_CFG_EN_SLEEP } else { 0 };
                    let _ = self.write(REG_SYS_CFG, sys_cfg).await;
                    continue;
                }
                Either3::Third(layer) => {
                    self.on_cursor_layer = self.role.cursor_layer == Some(layer.0);
                    continue;
                }
            }
            match self.read_motion().await {
                Ok(Some((x, y))) if (x, y) != (0, 0) => {
                    let rel = |axis, value| AxisEvent {
                        typ: AxisValType::Rel,
                        axis,
                        value,
                    };
                    let axes = if self.on_cursor_layer {
                        (Axis::X, Axis::Y)
                    } else {
                        self.role.axes
                    };
                    return PointingEvent {
                        device_id: self.role.device_id,
                        axes: [rel(axes.0, x), rel(axes.1, y), rel(Axis::Z, 0)],
                    };
                }
                // A tap has no motion. The pause keeps a pad that holds data ready high
                // from starving the other tasks.
                _ => Timer::after_millis(1).await,
            }
        }
    }
}
