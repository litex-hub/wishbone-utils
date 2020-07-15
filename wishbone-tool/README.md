# `wishbone-tool` - All-in-one Wishbone Binary and Library

`wishbone-tool` is useful for interacting with the internal Wishbone
bridge on a device.

Some of the things you can use `wishbone-tool` for:

- Peeking and poking memory, similar to using `devmem2`
- Testing memory and bridge link quality
- Exposing a Wishbone bridge to Ethernet
- Attaching a GDB server to a softcore

Currently-supported Wishbone bridges include:

- **USB** - For use with Valentyusb such as on Fomu
- **Serial** - Generic UART, nominally running at 115200 (but can be
  changed with ``--baud``)
- **SPI** - Using 2-, 3-, or 4-wire SPI from
  [spibone](https://github.com/litex-hub/spibone)
- **Ethernet** - Both TCP (e.g. a remote copy of `wishbone-tool`) or UDP (via Etherbone)
- **PCI Express** - Using a PCIe softcore with the CSR register bank exposed

## Binaries

Precompiled versions of `wishbone-tool` can be found in the
[Releases](https://github.com/litex-hub/wishbone-utils/releases)
section.

## Building

To build `wishbone-tool`:

1. Install Rust and Cargo. The easiest way to do this is to go to
   <https://rustup.rs/> and follow the instructions.
2. Enter the ``wishbone-tool`` directory.
3. Run `cargo build` or `cargo build --release`

The `wishbone-tool` binary will be located under `target/debug/` or
`target/release/`.

## Usage

By default, `wishbone-tool` will communicate via USB, attempting to
open a device with PID `0x5bf0`. It will also run the `peek/poke` server, allowing basic manipulation of memory addresses on the target device.

### USB Bridge

Simply run `wishbone-tool [ADDRESS]` to peek at a particular address.
To specify a particular vendor ID, pass `--vid [ID]`, for example `--vid 0xb0f1`.
To read from an area of memory (such as 0x10000000), run:

```shell
$ # Read from address 0x10000000 via USB
$ wishbone-tool 0x10000000
INFO [wishbone_tool::usb_bridge] waiting for target device
INFO [wishbone_tool::usb_bridge] opened USB device device 019 on bus 001
Value at 00000000: 6f80106f
$
```

To write a value to memory, add an additional parameter:

```shell
$ wishbone-tool 0x10000000 0x12345678
INFO [wishbone_tool::usb_bridge] opened USB device device 019 on bus 001
$ wishbone-tool 0x10000000
INFO [wishbone_tool::usb_bridge] opened USB device device 019 on bus 001
Value at 00000000: 12345678
$
```

### Serial Bridge

You can connect to a serial port by specifying the `--serial`
argument:

```shell
$ wishbone-tool --serial COM4: 0x00000000
Value at 00000000: ffffffff
$ wishbone-tool --serial /dev/ttyUSB0 0x00000000
Value at 00000000: ffffffff
```

Ensure that you have write permission to the serial port. On some Linux
systems you may need to add your user to the `dialout` group.

### Ethernet Bridge

To connect to an Ethernet device, pass the `--ethernet-host` parameter:

```sh
$ wishbone-tool --ethernet 192.168.100.50 0x00000000
Value at 00000000: ffffffff
```

To connect to a different port, add `--ethernet-port PORT_NUMBER`. Finally,
if you would like to connect to another copy of `wishbone-tool` or to a copy of `lxserver`, add `--ethernet-tcp` to switch the connection from Etherbone to TCP.

### PCIe Bridge

If your device is connected via PCI Express, you can specify a PCIe BAR with `--pcie-bar FILE_PATH`. This will be a device under `/sys/bus`.

Note that when running in PCIe mode, only a small portion of the memory space
is exposed. This means that you may need to specify `--register-offset OFFSET`, because e.g. address 0 in the PCIe BAR may actually correspond to address 0xe0000000, and `wishbone-tool` needs to know how to perform the translation.

### SPI Bridge

If you specify `--spi-pins`, `wishbone-tool` will communicate with the target device via SPI. This is currently only supported on Raspberry Pi. Specify the physical Broadcom Pin numbers. Consult [Pinout.xyz](https://pinout.xyz/) for more details. For example, assume you want to connect COPI,CPIO,CLK, and CS_N to pins 3,5,7, and 12 on the Raspberry Pi header. If you consult that website, you'll see pin 3 is BCM2, pin 5 is BCM3, pin 7 is BCM4, and pin 12 is BCM18. Therefore, the argument you would provide to `wishbone-tool` is `--spi-pins 2,3,4,18`

## Crossover UART

If your bridge is over a UART, then that means your UART is already in use,
and isn't available for use as a console. Or if you're connecting via some
other medium and you only have a single cable connecting the two devices. LiteX supports creating a
"crossover" UART that `wishbone-tool` can interact with and present a
local terminal on.

To add a UART bridge and a crossover UART to your design, instantiate the
main SoC object with `uart_name="crossover"` and add a separate Wishbone
bridge.

```python
   class MySoC(SoCCore):
      def __init__(self, platform, sys_clk_freq):
         SoCCore.__init__(self, platform, sys_clk_freq, uart_name="crossover")

         # Add a bridge with the real UART pins
         self.submodules.uart_bridge = UARTWishboneBridge(
               platform.request("serial"),
               sys_clk_freq,
               baudrate=115200)
         self.add_wb_master(self.uart_bridge.wishbone)
```

Then, to interact with the terminal, run `wishbone-tool` and provide it
with the `csr.csv` file from your build, and add the `-s terminal` flag:

```shell
$ wishbone-tool -s terminal --csr-csv build/csr.csv
$
```

Note that you can run multiple `wishbone-tool` servers at the same time.
For example, to run the `gdb` server as well, run:

```shell
$ wishbone-tool -s gdb -s terminal --csr-csv build/csr.csv
$
```

To exit the session, press `Ctrl-C`.

## GDB Server

If your softcore has a Vexriscv CPU in it, you can enable debug mode
and use `wishbone-tool` to act as a gdbserver.

## Command line Auto-Completion

You can generate auto-completion for `wishbone-tool` with the `-c`
option. For example, to generate auto-completion for bash, run:

```shell
$ wishbone-tool -c bash > wishbone-tool.bash
$ . wishbone-tool.bash
$
```

Auto-completion is available for zsh, bash, fish, powershell, and
elvish.

## `wishbone-bridge` as a Library

You can also use `wishbone-bridge` as a library from within your own program.
For more information, see the [wishbone-bridge documentation](https://docs.rs/wishbone-bridge/1.0.1/wishbone_bridge/).
