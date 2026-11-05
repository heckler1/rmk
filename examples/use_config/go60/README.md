# MoErgo Go60

RMK firmware for the MoErgo Go60, translated from a ZMK "Moonlander AutoShift" keymap.

This firmware intentionally differs from MoErgo's ZMK in two ways:

- **No Bluetooth disconnect:** RMK has no action for it, so double-tapping a profile key doesn't disconnect that profile.
- **The right half restarts when the cable changes:** RMK can't stop its BLE stack once it's running, so the right half reboots to switch links instead of switching live.

The left half is the central and the USB side. The halves talk over the TRRS cable when it's plugged in, and over BLE when it isn't. For a fully wired setup, connect the TRRS cable and a USB cable from the left half to your computer.

Plugging or unplugging the TRRS cable restarts the right half, which takes a moment on the cable and a few seconds to reconnect over BLE. Plug the cable into both halves: with one end loose, the halves can't reach each other.

While the cable is in the left half, it stops looking for the right half over BLE to save battery, and starts again a second or so after you unplug. Its BLE connection to your computer carries on either way.

## Build

Build with Docker from this directory:

```bash
./package.sh
```

This writes `go60_lh.uf2` and `go60_rh.uf2`. Use these files, not the output of `cargo make uf2` alone: `package.sh` sets the UF2 family IDs the Go60 bootloader requires.

## Flash

1. Put the left half in bootloader mode and copy `go60_lh.uf2` to the drive that appears.
2. Put the right half in bootloader mode and copy `go60_rh.uf2` to its drive.
3. Remove the old Go60 entry from your computer's Bluetooth settings and pair again.

On this firmware, the Bootloader key is the outer key on the second row of the Magic layer, on each half. The Magic layer is on the second left thumb key.

## Go back to ZMK

Enter bootloader mode the same way and copy your MoErgo UF2 to each half. This firmware leaves the bootloader, SoftDevice, and `UICR.REGOUT0` untouched.

## LEDs

Tap the third left thumb key to show status on the left half for 10 seconds. Rows count down from the top of the left half:

| LEDs | Shows |
| --- | --- |
| Second row | Left battery, filling from the outer edge |
| Third row | Right battery, all red if the right half is unreachable |
| Inner four LEDs of the fourth row | BLE profiles 1 to 4, from the third column inward |

Battery bars are green at 40% or more, yellow at 20% or more, and red below that.

Each BLE LED is white when connected and in use, green when connected while output is on USB, red when paired but not connected, and lilac when unused. A profile other than the active one shows as unused until it has been selected once.

The right battery can take a few seconds to appear after the right half starts or reconnects. Underglow changes made while it was disconnected can take up to 30 seconds to reach it.

The RGB keys on the Magic layer control the underglow on both halves. Settings are saved after a minute. Brightness is capped at 40% of full power, as MoErgo requires.
