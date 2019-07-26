# Wishbone Utilities

A collection of utilities for working with Wishbone

## Usage

To build the utilities, type `make`.  Precompiled versions of `wishbone-tool` can be found in the [releases/](https://github.com/xobs/wishbone-utils/releases) section.

## Utilities

* `wishbone-tool`: All-in-one Wishbone binary, available for a variety of platforms.

* `litex-devmem2`: An implementation of the classic `devmem2` command for litex.  Supports direct connections via Ethernet, or going through the `litex_server` binary to support PCIe and UART.

* `etherbone`: A library that can be used for communicating with a remote device.

## Wishbone Tool

`wishbone-tool` is useful for interacting with the internal Wishbone bridge on a device.  Some of the things you can use `wishbone-tool` for:

* Peeking and poking memory, such as with `devmem2`
* Testing memory and bridge consistency
* Exposing a Wishbone bridge to Ethernet
* Attaching a GDB server to a softcore

Currently-supported Wishbone bridges include:

* **usb** - For use with Valentyusb such as on Fomu
* **serial** - Generic UART, nominally running at 115200

### Wishbone Tool Usage

By default, `wishbone-tool` will communicate via USB, attempting to open a device with PID `0x5bf0`.  It will wait until it finds such a device.  To use a serial device instead, specify `wishbone-tool --serial /dev/ttyUSB0`.

To read from an area of memory (such as 0x10000000), run:

```sh
$ wishbone-tool 0x10000000
INFO [wishbone_tool::usb_bridge] waiting for target device
INFO [wishbone_tool::usb_bridge] opened USB device device 019 on bus 001
Value at 00000000: 6f80106f
$
```

To write a value to memory, add an additional parameter:

```sh
$ wishbone-tool 0x10000000 0x12345678
INFO [wishbone_tool::usb_bridge] opened USB device device 019 on bus 001
$ wishbone-tool 0x10000000
INFO [wishbone_tool::usb_bridge] opened USB device device 019 on bus 001
Value at 00000000: 12345678
$
```

You can connect to a serial port by specifying the `--serial` argument:

```sh
$ wishbone-tool --serial COM4: 0
```