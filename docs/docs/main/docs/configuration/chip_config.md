# Chip-Specific Configuration

The `[chip.<chip_name>]` section allows you to configure chip-specific settings for different microcontrollers. These settings are optional and only needed when you want to override the default behavior.

## nRF52 low-frequency clock

By default, RMK runs the low-frequency clock from the internal RC oscillator and tells the BLE stack it is accurate to 500 ppm. If your board has a 32.768 kHz crystal, set `lfclk_xtal_ppm` to use it. The crystal needs no periodic calibration wakeups, and a smaller figure narrows the receive window the BLE peer has to keep open, so it can save power.

This setting works under `[chip.<chip_name>]` for every nRF52 chip, for example `[chip.nrf52840]` or `[chip.nrf52832]`.

```toml
[chip.nrf52840]
# Use the crystal, accurate to 150 ppm. Leave unset to use the internal RC oscillator.
lfclk_xtal_ppm = 150
```

Set the value to the crystal's total accuracy, in the range 1 to 500. Include its tolerance, temperature drift and aging, as well as the accuracy of your load capacitors. If you don't know these, choose a value that's too high: too low a value can drop BLE connections, while too high a value only costs a little power.

::: danger Hardware Requirement
Only set `lfclk_xtal_ppm` if the board has a 32.768 kHz crystal fitted on the XL1 and XL2 pins, which are P0.00 and P0.01. RMK waits for the crystal without a timeout, so without one the firmware can stop during startup, before USB, BLE and the RMK watchdog have started. The board then looks dead and no key works, including a Bootloader key. To recover, enter the bootloader by a method that doesn't need the firmware, such as your board's reset button or a debug probe, and flash firmware without this setting.
:::

## nRF52840

### DCDC Configuration

The nRF52840 has two DC/DC regulators that can improve power efficiency when enabled with proper hardware support.

#### Configuration Example

```toml
[chip.nrf52840]
# Enable DCDC regulator 0
dcdc_reg0 = true
# Enable DCDC regulator 1
dcdc_reg1 = true
# Set DCDC regulator 0 voltage
dcdc_reg0_voltage = "3V3"  # Options: "3V3", "1V8" or "unchanged" (leave UICR.REGOUT0 as it is)
```

::: danger Hardware Requirement
Do not enable DC/DC regulators without an external LC filter being connected, as this will inhibit device operation, including debug access, until an LC filter is connected.
:::
::: danger Bootloader Version
If your hardware is using the [Adafruit nRF52 Bootloader](https://github.com/adafruit/Adafruit_nRF52_Bootloader) (e.g. nice!nano) ensure that your bootloader is updated to version ≥ [0.10.0](https://github.com/adafruit/Adafruit_nRF52_Bootloader/releases/tag/0.10.0) when setting a voltage other than 3V3.
There is a bug in older versions that can lead to a boot loop which can only be fixed by re-flashing the bootloader via the debug interface.
:::

## nRF52833

### DCDC Configuration

The nRF52833 has one DC/DC regulator available for configuration. Its REG0 stage has no DC/DC mode, but when the chip is supplied through VDDH you can set REG0's output voltage, which is VDD and the GPIO level.

#### Configuration Example

```toml
[chip.nrf52833]
# Enable DCDC regulator 1
dcdc_reg1 = true
# REG0 output voltage (VDD) when supplied through VDDH, stored in UICR.REGOUT0.
dcdc_reg0_voltage = "3V3"  # Options: "3V3", "1V8" or "unchanged" (leave UICR.REGOUT0 as it is)
```

::: danger Hardware Requirement
Do not enable DC/DC regulator without an external LC filter being connected, as this will inhibit device operation, including debug access, until an LC filter is connected.
:::
::: danger Bootloader Version
If your hardware is using the [Adafruit nRF52 Bootloader](https://github.com/adafruit/Adafruit_nRF52_Bootloader) (e.g. nice!nano) ensure that your bootloader is updated to version ≥ [0.10.0](https://github.com/adafruit/Adafruit_nRF52_Bootloader/releases/tag/0.10.0) when setting a voltage other than 3V3.
There is a bug in older versions that can lead to a boot loop which can only be fixed by re-flashing the bootloader via the debug interface.
:::
