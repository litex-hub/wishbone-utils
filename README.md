# Wishbone Utilities

A collection of utilities for working with Wishbone

## Contents

* `wishbone-tool`: All-in-one Wishbone binary, available for a variety of platforms.

* `litex-devmem2`: An implementation of the classic `devmem2` command for litex.  Supports direct connections via Ethernet, or going through the `litex_server` or `wishbone-tool` binaries to support PCIe, USB, SPI, and UART bridges.

* `etherbone`: A library that can be used for communicating with a remote device.

## Usage

Precompiled versions of `wishbone-tool` can be found in the [releases/](https://github.com/xobs/wishbone-utils/releases) section.

## Building

To build `wishbone-tool`:

1. Install Rust and Cargo. The easiest way to do this is to go to [https://rustup.rs/](https://rustup.rs/) and follow the instructions.
2. Enter the `wishbone-tool` directory.
3. Run `cargo build` or `cargo build --release`

The `wishbone-tool` binary will be located under `target/debug/` or `target/release/`.

To build `litex-devmem2`:

1. Install a compiler.  This varies from system to system.
2. Go to the `litex-devmem2/` directory.
3. Type `make`.

## Wishbone Tool

`wishbone-tool` is useful for interacting with the internal Wishbone bridge on a device.  Some of the things you can use `wishbone-tool` for:

* Peeking and poking memory, such as with `devmem2`
* Testing memory and bridge consistency
* Exposing a Wishbone bridge to Ethernet
* Attaching a GDB server to a softcore

Currently-supported Wishbone bridges include:

* **usb** - For use with Valentyusb such as on Fomu
* **serial** - Generic UART, nominally running at 115200
* **spi** - Using 2-, 3-, or 4-wire SPI from [spibone](https://github.com/xobs/spibone)

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

## Using the etherbone "library"

You can take the `csr.h` file out from LiteX and use it directly
on your PC for development.

1. Create an empty directory where the program will live.
2. Create a directory called `generated`, and copy `csr.h` from `build/software/include/generated/csr.h` into this directory.
3. Create a `main.c` with the following output:

    ```cpp
    #include <stdint.h>
    #include <stdio.h>
    #include <stdlib.h>

    #include "etherbone.h"
    #include "generated/csr.h"

    static struct eb_connection *eb;

    uint32_t csr_readl(unsigned long addr) {
        return eb_read32(eb, addr);
    }

    void csr_writel(uint32_t val, unsigned long addr) {
        eb_write32(eb, val, addr);
    }

    int main(int argc, char **argv) {
        eb = eb_connect("127.0.0.1", "1234", 0);
        if (!eb) {
            fprintf(stderr, "Couldn't connect\n");
            exit(1);
        }

        // You can now access registers from csr.h.  E.g.:
        fprintf(stderr, "Version: %d\n", version_major_read());
        return 0;
    }
    ```

4. Compile this with `gcc -ggdb3 etherbone.c main.c -o test-program -DCSR_ACCESSORS_DEFINED -I. -Wall`

You can add more files to the command line, or generate a Makefile to do it for you.

You can then run `wishbone-tool -s wishbone` and it will bridge memory accesses for you.
