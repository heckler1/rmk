use embedded_io_async::{Read, Write};

#[cfg(feature = "dfu_split")]
pub use crate::split::dfu::UpdatePolicy;

/// Run the manager task of one serial split peripheral.
///
/// BLE split peripherals are managed inside the BLE transport — pass one
/// [`PeripheralMatrixConfig`](crate::split::PeripheralMatrixConfig) per
/// peripheral to `BleTransport::new`. A BLE build can also run this, for a board whose
/// halves have a wired link as well.
pub async fn run_peripheral_manager<S: Read + Write>(
    id: usize,
    receiver: S,
    matrix_config: crate::split::PeripheralMatrixConfig,
    #[cfg(feature = "dfu_split")] policy: crate::split::dfu::UpdatePolicy,
) {
    crate::split::serial::run_serial_peripheral_manager(
        id,
        receiver,
        matrix_config,
        #[cfg(feature = "dfu_split")]
        policy,
    )
    .await;
}

/// Report whether peripheral `id` is reachable, for a transport that can tell.
///
/// RMK can't detect a wired peripheral going away, so it stays connected otherwise.
pub fn set_peripheral_connected(id: usize, connected: bool) {
    crate::split::driver::set_peripheral_connected(id, connected);
}
