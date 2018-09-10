#include <stdio.h>
#include <unistd.h>
#include <stdint.h>
#include <fcntl.h>
#include <sys/types.h>
#include <endian.h>
#include <errno.h>
#include <string.h>
#include <stdlib.h>
#include <netdb.h>
#include <sys/socket.h>
#include <netinet/in.h>

#include <sys/uio.h>

#include "etherbone.h"

#ifdef DEBUG
#define DEBUG_RISCV(str, ...) fprintf(stderr, str, __VA_ARGS__)
#else
#define DEBUG_RISCV(str, ...)
#endif

int main(int argc, char **argv) {

	struct eb_connection *conn;
	uint32_t address;
	uint32_t value;
	int is_write = 0;

	conn = eb_connect("127.0.0.1", "1234");
	if (!conn) {
		fprintf(stderr, "Unable to create connection\n");
		return 1;
	}
	if (argc == 1) {
		fprintf(stderr, "Must specify an address\n");
		return 1;
	}
	else if (argc == 2) {
		address = strtoul(argv[1], NULL, 0);
	}
	else if (argc == 3) {
		address = strtoul(argv[1], NULL, 0);
		value = strtoul(argv[2], NULL, 0);
		is_write = 1;
	}
	else if (argc == 4) {
		fprintf(stderr, "not supported\n");
	}

	if (is_write) {
		uint32_t old_val = eb_read32(conn, address);
		eb_write32(conn, address, value);
		uint32_t new_val = eb_read32(conn, address);
		fprintf(stderr, "0x%08x 0x%08x -> 0x%08x (wanted: 0x%08x)\n", address, old_val, new_val, value);
	}
	else {
		fprintf(stderr, "0x%08x: 0x%08x\n", address, eb_read32(conn, address));
	}

	eb_disconnect(&conn);
	return 0;
}
