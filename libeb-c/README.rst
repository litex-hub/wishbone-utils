Etherbone C Library
===================

A library that can be used for communicating with a remote device.

Using the etherbone “library”
=============================

You can take the ``csr.h`` file out from LiteX and use it directly on
your PC for development.

1. Create an empty directory where the program will live.
2. Create a directory called ``generated``, and copy ``csr.h`` from
   ``build/software/include/generated/csr.h`` into this directory.
3. Create a ``main.c`` with the following output:

   .. code:: cpp

      #include <stdint.h>
      #include <stdio.h>
      #include <stdlib.h>

      #include "etherbone.h"
      #include "generated/csr.h"

      static struct eb_connection *eb;

      uint32_t csr_read_simple(unsigned long addr) {
          return eb_read32(eb, addr);
      }

      void csr_write_simple(uint32_t val, unsigned long addr) {
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

4. Compile this with
   ``gcc -ggdb3 etherbone.c main.c -o test-program -DCSR_ACCESSORS_DEFINED -I. -Wall``
