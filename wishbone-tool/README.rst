``wishbone-tool`` - All-in-one Wishbone binary, available for a variety of platforms.
=====================================================================================

``wishbone-tool`` is useful for interacting with the internal Wishbone
bridge on a device.

Some of the things you can use ``wishbone-tool`` for:

-  Peeking and poking memory, such as with ``devmem2``
-  Testing memory and bridge consistency
-  Exposing a Wishbone bridge to Ethernet
-  Attaching a GDB server to a softcore

Currently-supported Wishbone bridges include:

-  **usb** - For use with Valentyusb such as on Fomu
-  **serial** - Generic UART, nominally running at 115200 (but can be
   changed with ``--baud``)
-  **spi** - Using 2-, 3-, or 4-wire SPI from
   `spibone <https://github.com/litex-hub/spibone>`__

Binaries
--------

Precompiled versions of ``wishbone-tool`` can be found in the
`releases/ <https://github.com/litex-hub/wishbone-utils/releases>`__
section.

Building
--------

To build ``wishbone-tool``:

1. Install Rust and Cargo. The easiest way to do this is to go to
   https://rustup.rs/ and follow the instructions.
2. Enter the ``wishbone-tool`` directory.
3. Run ``cargo build`` or ``cargo build --release``

The ``wishbone-tool`` binary will be located under ``target/debug/`` or
``target/release/``.

Usage
-----

By default, ``wishbone-tool`` will communicate via USB, attempting to
open a device with PID ``0x5bf0``. It will wait until it finds such a
device. To use a serial device instead, specify
``wishbone-tool --serial /dev/ttyUSB0``.

To read from an area of memory (such as 0x10000000), run:

.. session:: shell-session

   $ wishbone-tool 0x10000000
   INFO [wishbone_tool::usb_bridge] waiting for target device
   INFO [wishbone_tool::usb_bridge] opened USB device device 019 on bus 001
   Value at 00000000: 6f80106f
   $

To write a value to memory, add an additional parameter:

.. session:: shell-session

   $ wishbone-tool 0x10000000 0x12345678
   INFO [wishbone_tool::usb_bridge] opened USB device device 019 on bus 001
   $ wishbone-tool 0x10000000
   INFO [wishbone_tool::usb_bridge] opened USB device device 019 on bus 001
   Value at 00000000: 12345678
   $

You can connect to a serial port by specifying the ``--serial``
argument:

.. session:: shell-session

   $ wishbone-tool --serial COM4: 0

Command line Auto-Completion
----------------------------

You can generate auto-completion for ``wishbone-tool`` with the ``-c``
option. For example, to generate auto-completion for bash, run:

.. session:: shell-session

   $ wishbone-tool -c bash > wishbone-tool.bash
   $ . wishbone-tool.bash
   $

Auto-completion is available for zsh, bash, fish, powershell, and
elvish.
