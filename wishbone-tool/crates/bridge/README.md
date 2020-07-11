# Wishbone Bridge

This crate enables writing code in Rust to manipulate a device via a Wishbone bridge. Various bridges may be specified, depending on the bridge type you need.

Supported bridges include:

* SPI
* Ethernet
* USB
* UART (Serial)
* PCI Express

## Example Usage

As an example, there is a kind of device that has a USB bridge with a small
random number generator at address 0xf001_7000. This device has a
simple API:

1. Write `1` to 0xf001_7000 to enable the device
2. When `0xf001_7008` is `1` there is data available
3. Read the data from `0xf001_7004`
4. Goto 2

We can turn this into a command that reads from this RNG and prints to stdout:

```rust
use std::io::{self, Write};
use wishbone_bridge::{UsbBridge, BridgeError};

fn main() -> Result<(), BridgeError> {
    let stdout = io::stdout();
    let mut handle = stdout.lock();

    // Create a configuration object with a USB bridge that
    // connects to a device with the product ID of 0x5bf0.
    let bridge = UsbBridge::new().pid(0x5bf0).create()?;

    // Enable the oscillator. Note that this address may change,
    // so consult the `csr.csv` for your device.
    bridge.poke(0xf001_7000, 1)?;

    loop {
        // Wait until the `Ready` flag is `1`
        while bridge.peek(0xf001_7008)? & 1 == 0 {}

        // Read the random word and write it to stdout
        handle
            .write_all(&bridge.peek(0xf001_7004)?.to_le_bytes())
            .unwrap();
    }
}
```

It is then possible to run this with `cargo run | hexdump -C` to
produce an endless stream of random numbers.

## Feature Support

Support for all bridges is enabled by default, however you may enable only certain bridges using cargo features.

For example, to enable only the "usb" bridge, add the following to your `Cargo.toml`:

```toml
[dependencies]
wishbone-bridge = { version = "1", default-features = false, features = ["usb"] }
```

This will result in a faster build, but you will only have access to the `UsbBridge`.
