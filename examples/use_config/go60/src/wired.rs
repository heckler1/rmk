//! The TRRS link between the halves, on the half-duplex RS-422 bus in go60.dtsi.
//!
//! Only one side of the bus can drive it, but RMK's serial split expects a port that
//! either side can write at any time. So the link runs RMK's split messages through a
//! pair of pipes, and this module owns the bus. As in MoErgo's ZMK, the left half polls
//! and the right half speaks only when polled. A frame failing its CRC is dropped.
//!
//! Every frame also carries a small sideband, because RMK's `CustomMessage` only travels
//! over BLE. A sideband that has changed is delivered as a local `CustomMessage`, so
//! `underglow.rs` works the same on either link.
//!
//! RMK can't see this link fail, so the left half reports the right half's connection
//! from whether it's answering polls.
//!
//! The UART only runs while a half is using the cable. `BufferedUarte` starts receiving
//! as soon as it's built, and dropping it is the only way to stop it. So each half keeps
//! `BusParts`, which leaves the UART off, and builds a `Bus` from it while wired.

// Each half's binary uses only its own side of the link.
#![allow(dead_code)]

use core::cell::Cell;
use core::convert::Infallible;
use core::task::Poll;

use cortex_m::peripheral::SCB;
use embassy_futures::join::join;
use embassy_futures::poll_once;
use embassy_futures::select::select;
use embassy_nrf::Peri;
use embassy_nrf::buffered_uarte::BufferedUarte;
use embassy_nrf::gpio::{Input, Level, Output, OutputDrive, Pull};
use embassy_nrf::peripherals::{P0_11, P1_08, P1_09, P1_10, PPI_CH0, PPI_CH1, PPI_GROUP0, TIMER1, UARTE0};
use embassy_nrf::uarte::{self, Baudrate};
use embassy_sync::blocking_mutex::Mutex;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::pipe::Pipe;
use embassy_time::{Duration, Instant, Timer, with_timeout};
use embedded_io_async::{ErrorType, Read, Write};
use rmk::core_traits::Runnable;
use rmk::custom_message::{CustomMessage, CustomMessageTarget};
use rmk::event::{ConnectionStatusChangeEvent, publish_event};
use rmk::split::PeripheralMatrixConfig;
use rmk::split::central::{run_peripheral_manager, set_ble_split_enabled, set_peripheral_connected};
use rmk::state::{current_connection_status, current_sleep_state};
use static_cell::StaticCell;

const CENTRAL_MAGIC: u8 = 0xC5;
const PERIPHERAL_MAGIC: u8 = 0x5C;
const MAX_PAYLOAD: usize = 64;
pub const SIDEBAND_LEN: usize = 8;
/// Magic, length, sideband, and CRC.
const FRAME_OVERHEAD: usize = 2 + SIDEBAND_LEN + 2;
const PAYLOAD_START: usize = 2 + SIDEBAND_LEN;
const POLL_INTERVAL: Duration = Duration::from_millis(1);
/// Adds at most this much delay to the first right-half key after the keyboard sleeps.
const SLEEP_POLL_INTERVAL: Duration = Duration::from_millis(20);
/// Long enough for a full frame from the right half at 921600 baud, plus its turnaround.
const RESPONSE_TIMEOUT: Duration = Duration::from_millis(3);
/// Lets the last byte leave the shift register before the transceiver is turned off.
const DRIVE_HOLD: Duration = Duration::from_micros(30);
/// Lets the left half release the bus first.
const TURNAROUND: Duration = Duration::from_micros(150);
const CABLE_SETTLE: Duration = Duration::from_millis(200);
/// Survives a few bad frames without the status LEDs showing the right half as lost.
const ANSWER_TIMEOUT: Duration = Duration::from_millis(100);

type LinkPipe = Pipe<CriticalSectionRawMutex, 256>;

static CENTRAL_TO_PERIPHERAL: LinkPipe = Pipe::new();
static PERIPHERAL_TO_CENTRAL: LinkPipe = Pipe::new();

type Sideband = Mutex<CriticalSectionRawMutex, Cell<[u8; SIDEBAND_LEN]>>;

static CENTRAL_SIDEBAND: Sideband = Mutex::new(Cell::new([0; SIDEBAND_LEN]));

pub fn set_central_sideband(data: [u8; SIDEBAND_LEN]) {
    CENTRAL_SIDEBAND.lock(|sideband| sideband.set(data));
}

/// What RMK's split driver sees as its serial port.
pub struct LinkPort {
    rx: &'static LinkPipe,
    tx: &'static LinkPipe,
}

impl ErrorType for LinkPort {
    type Error = Infallible;
}

impl Read for LinkPort {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Infallible> {
        Ok(self.rx.read(buf).await)
    }
}

impl Write for LinkPort {
    async fn write(&mut self, buf: &[u8]) -> Result<usize, Infallible> {
        Ok(self.tx.write(buf).await)
    }

    async fn flush(&mut self) -> Result<(), Infallible> {
        Ok(())
    }
}

pub fn peripheral_port() -> LinkPort {
    LinkPort {
        rx: &CENTRAL_TO_PERIPHERAL,
        tx: &PERIPHERAL_TO_CENTRAL,
    }
}

fn crc16(data: &[u8]) -> u16 {
    let mut crc = 0xFFFF_u16;
    for byte in data {
        crc ^= (*byte as u16) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 { (crc << 1) ^ 0x1021 } else { crc << 1 };
        }
    }
    crc
}

/// What a `Bus` is built from. Holding this leaves the UART off and the transceiver
/// listening, as MoErgo's ZMK does while idle.
pub struct BusParts<'d> {
    uart: Peri<'d, UARTE0>,
    timer: Peri<'d, TIMER1>,
    ppi_ch1: Peri<'d, PPI_CH0>,
    ppi_ch2: Peri<'d, PPI_CH1>,
    ppi_group: Peri<'d, PPI_GROUP0>,
    rxd: Peri<'d, P1_09>,
    txd: Peri<'d, P1_08>,
    drive: Output<'d>,
    rx_buffer: &'d mut [u8; 256],
    tx_buffer: &'d mut [u8; 256],
}

impl<'d> BusParts<'d> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        uart: Peri<'d, UARTE0>,
        timer: Peri<'d, TIMER1>,
        ppi_ch1: Peri<'d, PPI_CH0>,
        ppi_ch2: Peri<'d, PPI_CH1>,
        ppi_group: Peri<'d, PPI_GROUP0>,
        rxd: Peri<'d, P1_09>,
        txd: Peri<'d, P1_08>,
        drive: Peri<'d, P0_11>,
    ) -> Self {
        static RX_BUFFER: StaticCell<[u8; 256]> = StaticCell::new();
        static TX_BUFFER: StaticCell<[u8; 256]> = StaticCell::new();
        Self {
            uart,
            timer,
            ppi_ch1,
            ppi_ch2,
            ppi_group,
            rxd,
            txd,
            drive: Output::new(drive, Level::Low, OutputDrive::Standard),
            rx_buffer: RX_BUFFER.init([0; 256]),
            tx_buffer: TX_BUFFER.init([0; 256]),
        }
    }

    /// Starts the UART. Dropping the `Bus` stops it and releases the UART pins.
    pub fn bus(&mut self) -> Bus<'_, 'd> {
        let mut config = uarte::Config::default();
        config.baudrate = Baudrate::Baud921600;
        Bus {
            uart: BufferedUarte::new(
                self.uart.reborrow(),
                self.timer.reborrow(),
                self.ppi_ch1.reborrow(),
                self.ppi_ch2.reborrow(),
                self.ppi_group.reborrow(),
                self.rxd.reborrow(),
                self.txd.reborrow(),
                crate::BoardIrqs,
                config,
                self.rx_buffer,
                self.tx_buffer,
            ),
            drive: &mut self.drive,
            last_sideband: [0; SIDEBAND_LEN],
        }
    }
}

/// The running UART and transceiver direction pin, shared by the two halves' board files.
pub struct Bus<'a, 'd> {
    uart: BufferedUarte<'a>,
    drive: &'a mut Output<'d>,
    last_sideband: [u8; SIDEBAND_LEN],
}

impl Bus<'_, '_> {
    async fn send(&mut self, magic: u8, sideband: [u8; SIDEBAND_LEN], payload: &[u8]) {
        let mut frame = [0; MAX_PAYLOAD + FRAME_OVERHEAD];
        let len = payload.len();
        frame[0] = magic;
        frame[1] = len as u8;
        frame[2..PAYLOAD_START].copy_from_slice(&sideband);
        frame[PAYLOAD_START..PAYLOAD_START + len].copy_from_slice(payload);
        let crc = crc16(&frame[..PAYLOAD_START + len]);
        frame[PAYLOAD_START + len..PAYLOAD_START + len + 2].copy_from_slice(&crc.to_le_bytes());

        self.drive.set_high();
        let mut sent = 0;
        while sent < len + FRAME_OVERHEAD {
            match self.uart.write(&frame[sent..len + FRAME_OVERHEAD]).await {
                Ok(n) => sent += n,
                Err(_) => break,
            }
        }
        let _ = self.uart.flush().await;
        Timer::after(DRIVE_HOLD).await;
        self.drive.set_low();

        // Drop any echo of this frame. The other half is still waiting out TURNAROUND,
        // or for this frame's reply to start, so nothing of its is in the buffer yet.
        let mut discarded = [0; MAX_PAYLOAD];
        while let Poll::Ready(Ok(n)) = poll_once(self.uart.read(&mut discarded))
            && n > 0
        {}
    }

    async fn read_exact(&mut self, buf: &mut [u8]) -> Option<()> {
        let mut received = 0;
        while received < buf.len() {
            received += self.uart.read(&mut buf[received..]).await.ok()?;
        }
        Some(())
    }

    /// Wait for a frame from the other side, returning its payload length. A changed
    /// sideband is published for `target` to pick up.
    async fn receive(
        &mut self,
        magic: u8,
        payload: &mut [u8; MAX_PAYLOAD],
        target: CustomMessageTarget,
    ) -> Option<usize> {
        let mut frame = [0; MAX_PAYLOAD + FRAME_OVERHEAD];
        frame[0] = 0;
        while frame[0] != magic {
            self.read_exact(&mut frame[..1]).await?;
        }
        self.read_exact(&mut frame[1..2]).await?;
        let len = frame[1] as usize;
        if len > MAX_PAYLOAD {
            return None;
        }
        let crc_start = PAYLOAD_START + len;
        self.read_exact(&mut frame[2..crc_start + 2]).await?;
        if crc16(&frame[..crc_start]).to_le_bytes() != frame[crc_start..crc_start + 2] {
            return None;
        }
        payload[..len].copy_from_slice(&frame[PAYLOAD_START..crc_start]);
        if frame[2..PAYLOAD_START] != self.last_sideband {
            self.last_sideband.copy_from_slice(&frame[2..PAYLOAD_START]);
            if let Ok(message) = CustomMessage::new(&self.last_sideband, target) {
                publish_event(message);
            }
        }
        Some(len)
    }
}

/// The left half's end: runs alongside BLE, and polls while the cable is present.
pub struct WiredCentral<'d> {
    parts: BusParts<'d>,
    cable: Input<'d>,
}

impl<'d> WiredCentral<'d> {
    pub fn new(parts: BusParts<'d>, cable: Peri<'d, P1_10>) -> Self {
        // This runs before RMK's tasks start, so booting with the cable in never scans.
        // `run` turns BLE back on once it has seen the cable stay out.
        set_ble_split_enabled(false);
        Self {
            parts,
            cable: Input::new(cable, Pull::None),
        }
    }
}

impl Runnable for WiredCentral<'_> {
    async fn run(&mut self) -> ! {
        let poll = async {
            let mut outgoing = [0; MAX_PAYLOAD];
            let mut incoming = [0; MAX_PAYLOAD];
            let mut last_answer = None;
            // RMK sets this as the manager starts, even with no cable, and subscribers
            // may not have been running yet. Clearing it means they see the first answer.
            Timer::after(POLL_INTERVAL).await;
            set_peripheral_connected(0, false);
            let mut answering = false;
            let mut wired = true;
            let mut cable_changed_at = None;
            // Polls once, returning whether the link still needs the bus: a cable that
            // has come out keeps it until the cable has settled, so a bounce doesn't
            // restart the UART.
            let mut poll_link = async |bus: &mut Bus<'_, '_>, cable_in: bool| {
                let len = CENTRAL_TO_PERIPHERAL.try_read(&mut outgoing).unwrap_or(0);
                if cable_in {
                    let sideband = CENTRAL_SIDEBAND.lock(Cell::get);
                    bus.send(CENTRAL_MAGIC, sideband, &outgoing[..len]).await;
                    let reply = with_timeout(
                        RESPONSE_TIMEOUT,
                        bus.receive(PERIPHERAL_MAGIC, &mut incoming, CustomMessageTarget::Central),
                    );
                    if let Ok(Some(len)) = reply.await {
                        PERIPHERAL_TO_CENTRAL.write_all(&incoming[..len]).await;
                        last_answer = Some(Instant::now());
                    }
                }
                let interval = if current_sleep_state() {
                    SLEEP_POLL_INTERVAL
                } else {
                    POLL_INTERVAL
                };
                Timer::after(interval).await;

                // The right half only answers BLE with the cable out, so the left half
                // stops looking for it. The BLE link to the host isn't touched.
                if cable_in == wired {
                    cable_changed_at = None;
                } else if cable_changed_at.get_or_insert_with(Instant::now).elapsed() >= CABLE_SETTLE {
                    wired = cable_in;
                    cable_changed_at = None;
                    set_ble_split_enabled(!wired);
                }

                let was_answering = answering;
                answering = last_answer.is_some_and(|at: Instant| at.elapsed() < ANSWER_TIMEOUT);
                if answering {
                    // Set on every poll, because a BLE link dropping late clears it too.
                    set_peripheral_connected(0, true);
                    if !was_answering {
                        // The right half missed this if it started after the left half,
                        // and it's what prompts it to send its battery level.
                        publish_event(ConnectionStatusChangeEvent(current_connection_status()));
                    }
                } else if was_answering {
                    set_peripheral_connected(0, false);
                }
                cable_in || wired
            };
            loop {
                let mut bus = self.parts.bus();
                while poll_link(&mut bus, self.cable.is_high()).await {}
                drop(bus);
                // Without a cable the data is dropped, so the split manager never
                // blocks. The right half starts fresh when it reboots into wired mode.
                let discard = async {
                    let mut unsent = [0; MAX_PAYLOAD];
                    loop {
                        CENTRAL_TO_PERIPHERAL.read(&mut unsent).await;
                    }
                };
                select(self.cable.wait_for_high(), discard).await;
            }
        };
        let manager = run_peripheral_manager(
            0,
            LinkPort {
                rx: &PERIPHERAL_TO_CENTRAL,
                tx: &CENTRAL_TO_PERIPHERAL,
            },
            PeripheralMatrixConfig {
                rows: 5,
                cols: 7,
                row_offset: 0,
                col_offset: 7,
            },
        );
        join(poll, manager).await;
        core::future::pending().await
    }
}

/// The right half's end of the bus: answers each poll with whatever is waiting.
pub async fn serve_peripheral(mut bus: Bus<'_, '_>) {
    let mut incoming = [0; MAX_PAYLOAD];
    let mut outgoing = [0; MAX_PAYLOAD];
    loop {
        let Some(len) = bus
            .receive(CENTRAL_MAGIC, &mut incoming, CustomMessageTarget::Peripherals)
            .await
        else {
            continue;
        };
        CENTRAL_TO_PERIPHERAL.write_all(&incoming[..len]).await;
        Timer::after(TURNAROUND).await;
        let len = PERIPHERAL_TO_CENTRAL.try_read(&mut outgoing).unwrap_or(0);
        bus.send(PERIPHERAL_MAGIC, [0; SIDEBAND_LEN], &outgoing[..len]).await;
    }
}

/// The right half uses one link at a time, so the left half never receives a key twice.
/// It chooses at boot and reboots when the cable changes, since the BLE stack can't be
/// stopped once it's running.
pub async fn reboot_on_cable_change(mut cable: Input<'_>, wired: bool) {
    loop {
        if wired {
            cable.wait_for_low().await;
        } else {
            cable.wait_for_high().await;
        }
        Timer::after(CABLE_SETTLE).await;
        if cable.is_high() != wired {
            SCB::sys_reset();
        }
    }
}
