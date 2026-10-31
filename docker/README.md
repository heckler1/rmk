# Build firmware with Docker

Build RMK firmware with only Docker installed. You don't need Rust, Cargo, or an embedded toolchain on your computer.

## Prerequisites

- [Docker Desktop](https://www.docker.com/products/docker-desktop/) or another Docker engine, running. The examples build with 8 GB of memory assigned to Docker.
- About 4 GB of disk space for the image, plus 2 GB more for ESP32-S3. Each example you build adds about 2 GB of build cache, and all examples share about 1 GB of downloaded crates.

## Build firmware

Run this command from anywhere in the repository, using the path to any example:

```bash
scripts/docker.sh build examples/use_config/nrf52840_ble
```

The first run builds the image, which takes a few minutes. Later runs reuse the image and the compiled dependencies.

The firmware is written to the example directory, and Git ignores it:

| File | Use |
| --- | --- |
| `*.uf2` | Copy to a board's UF2 bootloader drive. |
| `*.hex` | Flash with a tool that accepts Intel HEX. |
| `*.elf` | Flash with `probe-rs` or `espflash`, or use for debugging. |

The script only creates `.hex` and `.uf2` files when the example's `Makefile.toml` has a `uf2` task. Other examples, including all ESP32 examples, produce `.elf` files only.

Flash the firmware from your computer, because Docker Desktop on macOS can't access USB devices. Copying a `.uf2` file needs no extra tools, while `.elf` and `.hex` files need a flashing tool on your computer.

## Run other commands

Open a shell in the container, or run a single command in a directory:

```bash
scripts/docker.sh run examples/use_config/rp2040
```

```bash
scripts/docker.sh run examples/use_config/rp2040 cargo make uf2 --release
```

Use `run` for examples that `build` can't handle. For example, `esp32_ble_split` needs a different target for each half:

```bash
scripts/docker.sh run examples/use_config/esp32_ble_split cargo build-central --release
```

Build output from `run` stays in the Docker volume unless the command writes it to the example directory. In a `run` shell, the output is under `$CARGO_TARGET_DIR`.

## Update or clean up

To update Rust and the build tools, rebuild the image without its cache:

```bash
scripts/docker.sh image --no-cache
```

To delete cached build output and downloaded crates:

```bash
scripts/docker.sh clean
```

## How it works

- The repository is mounted at `/workspace`, so your edits are used immediately.
- Build output is stored in a Docker volume, not `target/`, because macOS bind mounts are slow. Each example has its own build directory.
- Examples with `channel = "esp"` in `rust-toolchain.toml` automatically use the larger `esp` image.
- The image installs the toolchain listed in the repository's `rust-toolchain.toml`.
