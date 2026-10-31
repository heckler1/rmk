//! Underglow and status LEDs, ported from MoErgo's `rgb_underglow.c`, with the settings in
//! their go60_lh_defconfig. RMK has no RGB support of its own.
//!
//! The left half owns the settings and sends them to the right. Each half animates its
//! own strip, and only the left half shows status, as on ZMK. Settings travel in a
//! `CustomMessage`, which `wired.rs` also delivers when the halves are on the cable.

// Each half's binary uses only its own side of this module.
#![allow(dead_code)]

use embassy_nrf::Peri;
use embassy_nrf::gpio::{Level, Output, OutputDrive};
use embassy_nrf::peripherals::{P0_27, P1_11, P1_15, PWM0, PWM1};
use embassy_nrf::pwm::{
    self, DutyCycle, Prescaler, SequenceConfig, SequencePwm, SimpleConfig, SimplePwm, SingleSequenceMode,
    SingleSequencer,
};
use embassy_time::{Duration, Instant, Timer};
use rmk::custom_message::{CustomMessage, CustomMessageTarget, send};
use rmk::event::{
    ActionEvent, BatteryStatusEvent, ConnectionStatusChangeEvent, ConnectionType, LayerChangeEvent,
    LedIndicatorEvent, PeripheralBatteryEvent, PeripheralConnectedEvent, SleepStateEvent,
};
use rmk::macros::processor;
use rmk::processor::DeadlineProcessor;
use rmk::state::current_connection_status;
use rmk::storage::{read_user_data, store_user_data};
use rmk::types::action::{Action, LightAction};
use rmk::types::battery::BatteryStatus;
use rmk::types::ble::{BleState, BleStatus};
use rmk::types::connection::UsbState;
use rmk::types::led_indicator::LedIndicator;

use crate::wired;

const NUM_PIXELS: usize = 30;
const HUE_MAX: u16 = 360;
const SAT_MAX: u8 = 100;
const BRT_MAX: u8 = 100;
/// MoErgo: above 40%, the LEDs can draw over 500mA, which can damage some computers
/// and voids the warranty. Every colour sent to the strip is scaled by this.
const BRT_LIMIT: u8 = 40;
const _: () = assert!(BRT_LIMIT <= 40);
const BRT_MIN: u8 = 2;
const BRT_STEP: u8 = 2;
const HUE_STEP: u16 = 10;
const SAT_STEP: u8 = 10;
const SPEED_MAX: u8 = 5;
const NUM_EFFECTS: u8 = 4;
const EFFECT_SOLID: u8 = 0;
const EFFECT_BREATHE: u8 = 1;
const EFFECT_SPECTRUM: u8 = 2;
const EFFECT_SWIRL: u8 = 3;

const TICK: Duration = Duration::from_millis(25);
const POWER_ON_DELAY: Duration = Duration::from_millis(100);
const SAVE_DELAY: Duration = Duration::from_secs(60);
/// Catches up a right half that restarted or reconnected. A write sent just after it
/// reconnects can be lost, because RMK subscribes to the custom messages after that.
const SETTINGS_RESEND_INTERVAL: Duration = Duration::from_secs(30);

/// Below these battery levels the LEDs dim and then turn off.
const DIM_LEVEL: u8 = 20;
const OFF_LEVEL: u8 = 10;

const STATUS_FADE_IN_STEPS: u16 = 500 / 25;
const STATUS_FADE_OUT_START: u16 = 8000 / 25;
const STATUS_FADE_OUT_STEPS: u16 = 2000 / 25;
const STATUS_END: u16 = 10000 / 25;

/// `User21`: ZMK's `&rgb_ug RGB_STATUS`.
const STATUS_KEY_ID: u8 = 21;

const LEFT_BATTERY_PIXELS: [usize; 6] = [27, 23, 18, 13, 8, 4];
const RIGHT_BATTERY_PIXELS: [usize; 6] = [28, 24, 19, 14, 9, 5];
const BLE_PIXELS: [usize; 4] = [20, 15, 10, 6];
const LAYER_PIXELS: [usize; 6] = [26, 22, 17, 12, 7, 3];
const CAPS_LOCK_PIXEL: usize = 0;
const NUM_LOCK_PIXEL: usize = 1;
const SCROLL_LOCK_PIXEL: usize = 2;
const OUTPUT_FALLBACK_PIXEL: usize = 11;
const USB_PIXEL: usize = 21;

/// `pwm-leds` in go60_lh.dts: a 20us period, which is 320 ticks at 16MHz.
const REAR_LED_MAX_DUTY: u16 = 320;
/// CONFIG_ZMK_BACKLIGHT_BRT_START in MoErgo's defconfigs: 5%. Inverted polarity keeps
/// the pin high until the counter reaches the duty value, so 0 is off.
const REAR_LED_ON: DutyCycle = DutyCycle::inverted(REAR_LED_MAX_DUTY / 20);
const REAR_LED_OFF: DutyCycle = DutyCycle::inverted(0);

const fn status_channel(c: u8) -> u8 {
    ((BRT_LIMIT as u16 * c as u16) / 0xFF) as u8
}

const fn status_color(r: u8, g: u8, b: u8) -> Rgb {
    Rgb {
        r: status_channel(r),
        g: status_channel(g),
        b: status_channel(b),
    }
}
const RED: Rgb = status_color(0xFF, 0x00, 0x00);
const YELLOW: Rgb = status_color(0xFF, 0xFF, 0x00);
const GREEN: Rgb = status_color(0x00, 0xFF, 0x00);
const DULL_GREEN: Rgb = status_color(0x00, 0xFF, 0x68);
const WHITE: Rgb = status_color(0xFF, 0xFF, 0xFF);
const MAGENTA: Rgb = status_color(0xFF, 0x00, 0xFF);
const LILAC: Rgb = status_color(0x6B, 0x1F, 0xCE);

const MESSAGE_SETTINGS: u8 = 1;
const SETTINGS_SLOT: u8 = 0;

#[derive(Clone, Copy, Default)]
struct Rgb {
    r: u8,
    g: u8,
    b: u8,
}

fn hsb_to_rgb(h: u16, s: u8, b: u8) -> Rgb {
    let i = h / 60;
    let v = b as f32 / BRT_MAX as f32;
    let s = s as f32 / SAT_MAX as f32;
    let f = h as f32 / HUE_MAX as f32 * 6.0 - i as f32;
    let p = v * (1.0 - s);
    let q = v * (1.0 - f * s);
    let t = v * (1.0 - (1.0 - f) * s);
    let (r, g, b) = match i % 6 {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    };
    Rgb {
        r: (r * 255.0) as u8,
        g: (g * 255.0) as u8,
        b: (b * 255.0) as u8,
    }
}

fn scale_min_max(b: u8) -> u8 {
    (BRT_MIN as u16 + (BRT_LIMIT - BRT_MIN) as u16 * b as u16 / BRT_MAX as u16) as u8
}

fn scale_zero_max(b: u16) -> u8 {
    (b * BRT_LIMIT as u16 / BRT_MAX as u16) as u8
}

#[derive(Clone, Copy, PartialEq)]
struct Settings {
    on: bool,
    hue: u16,
    saturation: u8,
    brightness: u8,
    speed: u8,
    effect: u8,
}

impl Settings {
    const DEFAULT: Self = Self {
        on: false,
        hue: 285,
        saturation: 75,
        brightness: 16,
        speed: 3,
        effect: EFFECT_SWIRL,
    };

    fn to_bytes(self) -> [u8; 7] {
        let [hue_low, hue_high] = self.hue.to_le_bytes();
        [
            self.on as u8,
            hue_low,
            hue_high,
            self.saturation,
            self.brightness,
            self.speed,
            self.effect,
        ]
    }

    /// Stored and received bytes pass through here, so nothing out of range reaches the strip.
    fn from_bytes(b: &[u8]) -> Option<Self> {
        let &[on, hue_low, hue_high, saturation, brightness, speed, effect, ..] = b else {
            return None;
        };
        Some(Self {
            on: on != 0,
            hue: u16::from_le_bytes([hue_low, hue_high]).min(HUE_MAX - 1),
            saturation: saturation.min(SAT_MAX),
            brightness: brightness.min(BRT_MAX),
            speed: speed.clamp(1, SPEED_MAX),
            effect: effect % NUM_EFFECTS,
        })
    }
}

fn battery_level(status: BatteryStatus) -> Option<u8> {
    match status {
        BatteryStatus::Available { level, .. } => level,
        BatteryStatus::Unavailable => None,
    }
}

/// The LED strip, driven by a PWM sequence. MoErgo's ZMK uses SPI on these pins, but here
/// the SPI transfer never reported finishing and later frames didn't reach the strip.
pub struct Strip<'d> {
    pwm: SequencePwm<'d>,
    power: Output<'d>,
    buffer: [u16; NUM_PIXELS * 24 + RESET_WORDS],
}

/// PWM periods of 20 ticks at 16MHz give WS2812's 1.25us bit. The top bit makes each
/// period start high, so the duty is the high time.
const PWM_MAX_DUTY: u16 = 20;
const ONE_DUTY: u16 = 0x8000 | 13;
const ZERO_DUTY: u16 = 0x8000 | 7;
const RESET_DUTY: u16 = 0x8000;
/// A 300us low at the end latches the colours.
const RESET_WORDS: usize = 240;
/// Long enough for the whole sequence, which takes 1.2ms.
const FRAME_TIME: Duration = Duration::from_millis(2);

/// Each colour bit becomes one PWM period, green first, as `color-mapping` in go60_lh.dts.
fn encode(pixels: &[Rgb; NUM_PIXELS], buffer: &mut [u16]) {
    for (pixel, words) in pixels.iter().zip(buffer.chunks_exact_mut(24)) {
        let grb = (pixel.g as u32) << 16 | (pixel.r as u32) << 8 | pixel.b as u32;
        for (bit, word) in words.iter_mut().enumerate() {
            *word = if grb & (1 << (23 - bit)) != 0 { ONE_DUTY } else { ZERO_DUTY };
        }
    }
}

impl<'d> Strip<'d> {
    pub fn new(pwm: Peri<'d, PWM1>, data: Peri<'d, P0_27>, power: Peri<'d, P1_11>) -> Self {
        let mut config = pwm::Config::default();
        config.prescaler = Prescaler::Div1;
        config.max_duty = PWM_MAX_DUTY;
        config.ch0_drive = OutputDrive::HighDrive;
        Self {
            pwm: defmt::unwrap!(SequencePwm::new_1ch(pwm, data, config)),
            power: Output::new(power, Level::Low, OutputDrive::Standard),
            buffer: [RESET_DUTY; NUM_PIXELS * 24 + RESET_WORDS],
        }
    }

    async fn write(&mut self, pixels: &[Rgb; NUM_PIXELS]) {
        encode(pixels, &mut self.buffer);
        let sequencer = SingleSequencer::new(&mut self.pwm, &self.buffer, SequenceConfig::default());
        if sequencer.start(SingleSequenceMode::Times(1)).is_err() {
            return;
        }
        Timer::after(FRAME_TIME).await;
    }
}

/// The animation and power state both halves share.
struct Lighting<'d> {
    strip: Strip<'d>,
    settings: Settings,
    step: u16,
    pixels: [Rgb; NUM_PIXELS],
    powered: bool,
    next_tick: Option<Instant>,
}

impl<'d> Lighting<'d> {
    fn new(strip: Strip<'d>) -> Self {
        Self {
            strip,
            settings: Settings::DEFAULT,
            step: 0,
            pixels: [Rgb::default(); NUM_PIXELS],
            powered: false,
            next_tick: None,
        }
    }

    fn animate(&mut self) {
        let Settings {
            hue,
            saturation,
            brightness,
            speed,
            effect,
            ..
        } = self.settings;
        let speed = speed as u16;
        match effect {
            EFFECT_SOLID => self.pixels = [hsb_to_rgb(hue, saturation, scale_min_max(brightness)); NUM_PIXELS],
            EFFECT_BREATHE => {
                let level = self.step.abs_diff(1200) / 12;
                self.pixels = [hsb_to_rgb(hue, saturation, scale_zero_max(level)); NUM_PIXELS];
                self.step += speed * 10;
                if self.step > 2400 {
                    self.step = 0;
                }
            }
            EFFECT_SPECTRUM => {
                self.pixels = [hsb_to_rgb(self.step, saturation, scale_min_max(brightness)); NUM_PIXELS];
                self.step = (self.step + speed) % HUE_MAX;
            }
            _ => {
                for (i, pixel) in self.pixels.iter_mut().enumerate() {
                    let hue = (HUE_MAX / NUM_PIXELS as u16 * i as u16 + self.step) % HUE_MAX;
                    *pixel = hsb_to_rgb(hue, saturation, scale_min_max(brightness));
                }
                self.step = (self.step + speed * 2) % HUE_MAX;
            }
        }
    }

    /// Start or stop the LEDs to match `wanted`. Returns once any strip power change is done.
    async fn set_active(&mut self, wanted: bool) {
        if wanted && !self.powered {
            self.strip.power.set_high();
            self.powered = true;
            self.next_tick = Some(Instant::now() + POWER_ON_DELAY);
        } else if !wanted && self.powered {
            self.strip.write(&[Rgb::default(); NUM_PIXELS]).await;
            self.strip.power.set_low();
            self.powered = false;
            self.next_tick = None;
        }
    }

    /// Dimmed to half on a low battery, blank on an empty one.
    async fn show(&mut self, mut pixels: [Rgb; NUM_PIXELS], level: u8) {
        if level < DIM_LEVEL {
            for pixel in &mut pixels {
                *pixel = Rgb {
                    r: pixel.r >> 1,
                    g: pixel.g >> 1,
                    b: pixel.b >> 1,
                };
            }
        }
        self.strip.write(&pixels).await;
        self.next_tick = Some(Instant::now() + TICK);
    }
}

fn apply_battery_bar(pixels: &mut [Rgb; NUM_PIXELS], level: u8, addresses: &[usize; 6]) {
    let color = if level >= 40 {
        GREEN
    } else if level >= 20 {
        YELLOW
    } else {
        RED
    };
    for (i, address) in addresses.iter().enumerate() {
        if level as usize >= i * 100 / (addresses.len() - 1) {
            pixels[*address] = color;
        }
    }
}

/// `output` is where keystrokes go. The other slots' colours come from `bonded`,
/// because RMK reports only the active profile.
fn apply_ble_state(pixels: &mut [Rgb; NUM_PIXELS], ble: BleStatus, output: Option<ConnectionType>, bonded: u8) {
    for (slot, address) in BLE_PIXELS.iter().enumerate() {
        let active = slot as u8 == ble.profile;
        let paired = if active { ble.bonded } else { bonded & (1 << slot) != 0 };
        pixels[*address] = if active && ble.state == BleState::Connected {
            if output == Some(ConnectionType::Ble) { WHITE } else { DULL_GREEN }
        } else if paired {
            RED
        } else {
            LILAC
        };
    }
}

/// How far the status display has faded in, out of 256.
fn status_blend(step: u16) -> u16 {
    if step < STATUS_FADE_IN_STEPS {
        step * 256 / STATUS_FADE_IN_STEPS
    } else if step > STATUS_FADE_OUT_START {
        256_u16.saturating_sub((step - STATUS_FADE_OUT_START) * 256 / STATUS_FADE_OUT_STEPS)
    } else {
        256
    }
}

fn blend(status: &[Rgb; NUM_PIXELS], underglow: &[Rgb; NUM_PIXELS], amount: u16) -> [Rgb; NUM_PIXELS] {
    let mix = |s: u8, u: u8| (((s as u16 * amount) >> 8) + ((u as u16 * (256 - amount)) >> 8)) as u8;
    core::array::from_fn(|i| Rgb {
        r: mix(status[i].r, underglow[i].r),
        g: mix(status[i].g, underglow[i].g),
        b: mix(status[i].b, underglow[i].b),
    })
}

/// Send to the right half on whichever link is up.
fn send_to_peripheral(data: &[u8; wired::SIDEBAND_LEN]) {
    wired::set_central_sideband(*data);
    if let Ok(message) = CustomMessage::new(data, CustomMessageTarget::Peripherals) {
        send(message);
    }
}

#[processor(
    subscribe = [ActionEvent, BatteryStatusEvent, ConnectionStatusChangeEvent, LayerChangeEvent, LedIndicatorEvent, PeripheralBatteryEvent, PeripheralConnectedEvent, SleepStateEvent],
    deadline
)]
pub struct LeftUnderglow<'d> {
    lighting: Lighting<'d>,
    rear_led: SimplePwm<'d>,
    loaded: bool,
    sleeping: bool,
    leds: LedIndicator,
    layer: u8,
    left_level: Option<u8>,
    right_level: Option<u8>,
    right_connected: bool,
    next_settings: Instant,
    /// Which BLE profiles were bonded when last active.
    bonded: u8,
    status_step: Option<u16>,
    save_at: Option<Instant>,
}

impl<'d> LeftUnderglow<'d> {
    pub fn new(strip: Strip<'d>, rear_pwm: Peri<'d, PWM0>, rear_led: Peri<'d, P1_15>) -> Self {
        let mut config = SimpleConfig::default();
        config.prescaler = Prescaler::Div1;
        config.max_duty = REAR_LED_MAX_DUTY;
        let rear_led = SimplePwm::new_1ch(rear_pwm, rear_led, &config);
        rear_led.disable();
        Self {
            lighting: Lighting::new(strip),
            rear_led,
            loaded: false,
            sleeping: false,
            leds: LedIndicator::new(),
            layer: 0,
            left_level: None,
            right_level: None,
            right_connected: false,
            next_settings: Instant::MIN,
            bonded: 0,
            status_step: None,
            save_at: None,
        }
    }

    fn save_later(&mut self) {
        self.save_at = Some(Instant::now() + SAVE_DELAY);
    }

    fn send_settings(&self) {
        let [on, hue_low, hue_high, saturation, brightness, speed, effect] = self.lighting.settings.to_bytes();
        send_to_peripheral(&[MESSAGE_SETTINGS, on, hue_low, hue_high, saturation, brightness, speed, effect]);
    }

    /// Lit while on USB power and awake, as on ZMK. The PWM only stays enabled while lit,
    /// because it keeps the fast clock running. Disabling leaves the pin where the last
    /// duty put it, so the duty has to reach 0 first.
    fn update_rear_led(&mut self) {
        let on = !self.sleeping && current_connection_status().usb != UsbState::Disabled;
        if on == self.rear_led.is_enabled() {
            return;
        }
        if on {
            self.rear_led.enable();
            self.rear_led.set_duty(0, REAR_LED_ON);
        } else {
            self.rear_led.set_duty(0, REAR_LED_OFF);
            self.rear_led.disable();
        }
    }

    fn underglow_on(&self) -> bool {
        self.lighting.settings.on && !self.sleeping && self.left_level.unwrap_or(100) >= OFF_LEVEL
    }

    fn should_run(&self) -> bool {
        self.status_step.is_some() || self.underglow_on()
    }

    async fn on_action_event(&mut self, event: ActionEvent) {
        if !event.keyboard_event.pressed {
            return;
        }
        let settings = &mut self.lighting.settings;
        match event.action {
            Action::User(STATUS_KEY_ID) => {
                self.status_step = Some(self.status_step.map_or(0, |step| step.min(STATUS_FADE_IN_STEPS)));
                self.lighting.set_active(true).await;
                return;
            }
            Action::Light(LightAction::RgbTog) => settings.on = !settings.on,
            Action::Light(LightAction::RgbModeForward) => {
                settings.effect = (settings.effect + 1) % NUM_EFFECTS;
                self.lighting.step = 0;
            }
            Action::Light(LightAction::RgbHui) => settings.hue = (settings.hue + HUE_STEP) % HUE_MAX,
            Action::Light(LightAction::RgbHud) => settings.hue = (settings.hue + HUE_MAX - HUE_STEP) % HUE_MAX,
            Action::Light(LightAction::RgbSai) => settings.saturation = (settings.saturation + SAT_STEP).min(SAT_MAX),
            Action::Light(LightAction::RgbSad) => settings.saturation = settings.saturation.saturating_sub(SAT_STEP),
            Action::Light(LightAction::RgbVai) => settings.brightness = (settings.brightness + BRT_STEP).min(BRT_MAX),
            Action::Light(LightAction::RgbVad) => settings.brightness = settings.brightness.saturating_sub(BRT_STEP),
            Action::Light(LightAction::RgbSpi) => settings.speed = (settings.speed + 1).min(SPEED_MAX),
            Action::Light(LightAction::RgbSpd) => settings.speed = settings.speed.saturating_sub(1).max(1),
            _ => return,
        }
        self.send_settings();
        self.save_later();
        let run = self.should_run();
        self.lighting.set_active(run).await;
    }

    async fn on_battery_status_event(&mut self, event: BatteryStatusEvent) {
        self.left_level = battery_level(event.0);
        let run = self.should_run();
        self.lighting.set_active(run).await;
    }

    async fn on_sleep_state_event(&mut self, event: SleepStateEvent) {
        self.sleeping = event.0;
        self.update_rear_led();
        let run = self.should_run();
        self.lighting.set_active(run).await;
    }

    async fn on_layer_change_event(&mut self, event: LayerChangeEvent) {
        self.layer = event.0;
    }

    async fn on_led_indicator_event(&mut self, event: LedIndicatorEvent) {
        self.leds = event.0;
    }

    async fn on_connection_status_change_event(&mut self, event: ConnectionStatusChangeEvent) {
        self.update_rear_led();
        let ble = event.0.ble;
        let bonded = (self.bonded & !(1 << ble.profile)) | ((ble.bonded as u8) << ble.profile);
        if bonded != self.bonded {
            self.bonded = bonded;
            self.save_later();
        }
    }

    async fn on_peripheral_battery_event(&mut self, event: PeripheralBatteryEvent) {
        self.right_level = battery_level(event.state.0);
    }

    async fn on_peripheral_connected_event(&mut self, event: PeripheralConnectedEvent) {
        self.right_connected = event.connected;
    }

    fn status_pixels(&self) -> [Rgb; NUM_PIXELS] {
        let mut pixels = [Rgb::default(); NUM_PIXELS];
        apply_battery_bar(&mut pixels, self.left_level.unwrap_or(0), &LEFT_BATTERY_PIXELS);
        match self.right_level.filter(|_| self.right_connected) {
            Some(level) => apply_battery_bar(&mut pixels, level, &RIGHT_BATTERY_PIXELS),
            None => RIGHT_BATTERY_PIXELS.iter().for_each(|address| pixels[*address] = RED),
        }
        let connection = current_connection_status();
        let active = connection.decide_active();
        apply_ble_state(&mut pixels, connection.ble, active, self.bonded);
        pixels[USB_PIXEL] = match connection.usb {
            UsbState::Configured if active == Some(ConnectionType::Usb) => WHITE,
            UsbState::Configured => DULL_GREEN,
            UsbState::Enabled | UsbState::Suspended => RED,
            UsbState::Disabled => LILAC,
        };
        if active != Some(connection.preferred) {
            pixels[OUTPUT_FALLBACK_PIXEL] = RED;
        }
        for (on, address) in [
            (self.leds.caps_lock(), CAPS_LOCK_PIXEL),
            (self.leds.num_lock(), NUM_LOCK_PIXEL),
            (self.leds.scroll_lock(), SCROLL_LOCK_PIXEL),
        ] {
            if on {
                pixels[address] = RED;
            }
        }
        // ZMK lights every active layer, but RMK reports only the highest one.
        for layer in [0, self.layer] {
            if let Some(address) = LAYER_PIXELS.get(layer as usize) {
                pixels[*address] = MAGENTA;
            }
        }
        pixels
    }

    async fn load(&mut self) {
        self.loaded = true;
        self.update_rear_led();
        if let Some(stored) = read_user_data(SETTINGS_SLOT).await {
            if let Some(settings) = Settings::from_bytes(&stored) {
                self.lighting.settings = settings;
            }
            self.bonded = stored.get(7).copied().unwrap_or(0);
        }
        let run = self.should_run();
        self.lighting.set_active(run).await;
    }

    async fn save(&mut self) {
        self.save_at = None;
        let mut stored = [0; 8];
        stored[..7].copy_from_slice(&self.lighting.settings.to_bytes());
        stored[7] = self.bonded;
        let _ = store_user_data(SETTINGS_SLOT, &stored).await;
    }

    async fn tick(&mut self) {
        self.lighting.animate();
        let level = self.left_level.unwrap_or(100);
        let underglow = if self.underglow_on() {
            self.lighting.pixels
        } else {
            [Rgb::default(); NUM_PIXELS]
        };
        let mut pixels = underglow;
        if let Some(step) = self.status_step {
            pixels = blend(&self.status_pixels(), &underglow, status_blend(step));
            self.status_step = (step < STATUS_END).then_some(step + 1);
        }
        self.lighting.show(pixels, level).await;
        if self.status_step.is_none() {
            let run = self.should_run();
            self.lighting.set_active(run).await;
        }
    }
}

impl DeadlineProcessor for LeftUnderglow<'_> {
    fn deadline(&self) -> Option<Instant> {
        if !self.loaded {
            return Some(Instant::MIN);
        }
        [self.lighting.next_tick, self.save_at, Some(self.next_settings)]
            .into_iter()
            .flatten()
            .min()
    }

    async fn on_deadline(&mut self) {
        if !self.loaded {
            self.load().await;
            return;
        }
        let now = Instant::now();
        if self.next_settings <= now {
            self.send_settings();
            self.next_settings = now + SETTINGS_RESEND_INTERVAL;
        }
        if self.save_at.is_some_and(|at| at <= now) {
            self.save().await;
        }
        if self.lighting.next_tick.is_some_and(|at| at <= now) {
            self.tick().await;
        }
    }
}

#[processor(subscribe = [BatteryStatusEvent, CustomMessage, SleepStateEvent], deadline)]
pub struct RightUnderglow<'d> {
    lighting: Lighting<'d>,
    /// Held low to keep the rear LED off, because it stays on otherwise. This half has no
    /// USB, so it never shows the left half's USB indicator.
    _rear_led: Output<'d>,
    level: Option<u8>,
    sleeping: bool,
}

impl<'d> RightUnderglow<'d> {
    pub fn new(strip: Strip<'d>, rear_led: Peri<'d, P1_15>) -> Self {
        Self {
            lighting: Lighting::new(strip),
            _rear_led: Output::new(rear_led, Level::Low, OutputDrive::Standard),
            level: None,
            sleeping: false,
        }
    }

    async fn apply(&mut self) {
        let run = self.lighting.settings.on && !self.sleeping && self.level.unwrap_or(100) >= OFF_LEVEL;
        self.lighting.set_active(run).await;
    }

    async fn on_sleep_state_event(&mut self, event: SleepStateEvent) {
        self.sleeping = event.0;
        self.apply().await;
    }

    async fn on_battery_status_event(&mut self, event: BatteryStatusEvent) {
        self.level = battery_level(event.0);
        self.apply().await;
    }

    async fn on_custom_message_event(&mut self, message: CustomMessage) {
        if let [MESSAGE_SETTINGS, ref settings @ ..] = message.data[..]
            && let Some(settings) = Settings::from_bytes(settings)
        {
            self.lighting.settings = settings;
            self.apply().await;
        }
    }
}

impl DeadlineProcessor for RightUnderglow<'_> {
    fn deadline(&self) -> Option<Instant> {
        self.lighting.next_tick
    }

    async fn on_deadline(&mut self) {
        self.lighting.animate();
        let pixels = self.lighting.pixels;
        self.lighting.show(pixels, self.level.unwrap_or(100)).await;
    }
}
