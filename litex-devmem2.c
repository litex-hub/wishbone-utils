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

#ifdef DEBUG
#define DEBUG_RISCV(str, ...) fprintf(stderr, str, __VA_ARGS__)
#else
#define DEBUG_RISCV(str, ...)
#endif

// Default to a width of 8, if undefined.  This is because upstream defaults to 8.
#ifndef CSR_WIDTH
#define CSR_WIDTH 8
#endif

// 4e 6f                // magic
// 10                   // VVVV?nRF (V: Version, n: No reads, R: Probe reply, F: Probe flag)
// 44                   // AAAApppp (A: Address size, p: Port size)
// 00 00 00 00          // Padding
// 00 0f 01 00 e0 00 a0 24 00 00 00 00

// 4e 6f
// 10
// 44
// 00 00 00 00
// 00 0f 00 01 00 00 00 00
// e0 00 58 18

// Size of the specified address, in bits
#define ADDR_SIZE_8 1
#define ADDR_SIZE_16 2
#define ADDR_SIZE_32 4
#define ADDR_SIZE_64 8

// Size of the specified address, in bits
#define PORT_SIZE_8 1
#define PORT_SIZE_16 2
#define PORT_SIZE_32 4
#define PORT_SIZE_64 8

struct etherbone_record {
	// 1...
#if __BYTE_ORDER == __BIG_ENDIAN
	uint8_t bca : 1;
	uint8_t rca : 1;
	uint8_t rff : 1;
	uint8_t ign1 : 1;
	uint8_t cyc : 1;
	uint8_t wca : 1;
	uint8_t wff : 1;
	uint8_t ign2 : 1;
#elif __BYTE_ORDER == __LITTLE_ENDIAN
	uint8_t ign2 : 1;
	uint8_t wff : 1;
	uint8_t wca : 1;
	uint8_t cyc : 1;
	uint8_t ign1 : 1;
	uint8_t rff : 1;
	uint8_t rca : 1;
	uint8_t bca : 1;
#else
#pragma error "Unrecognized byte order"
#endif

	uint8_t byte_enable;

	uint8_t wcount;

	uint8_t rcount;

	uint32_t write_addr;
	uint32_t value;
} __attribute__((packed));

struct etherbone_packet {
	uint8_t magic[2]; // 0x4e 0x6f

#if __BYTE_ORDER == __BIG_ENDIAN
	uint8_t version : 4;
	uint8_t ign : 1;
	uint8_t no_reads : 1;
	uint8_t probe_reply : 1;
	uint8_t probe_flag : 1;
#elif __BYTE_ORDER == __LITTLE_ENDIAN
	uint8_t probe_flag : 1;
	uint8_t probe_reply : 1;
	uint8_t no_reads : 1;
	uint8_t ign : 1;
	uint8_t version : 4;
#else
#pragma error "Unrecognized byte order"
#endif

#if __BYTE_ORDER == __BIG_ENDIAN
	uint8_t port_size : 4;
	uint8_t addr_size : 4;
#elif __BYTE_ORDER == __LITTLE_ENDIAN
	uint8_t addr_size : 4;
	uint8_t port_size : 4;
#else
#pragma error "Unrecognized byte order"
#endif
	uint8_t padding[4];

	struct etherbone_record records[0];
} __attribute__((packed));

struct wb_connection {
	int fd;
	struct addrinfo* addr;
};

int wb_connect(struct wb_connection *conn, const char *addr, const char *port) {

	struct addrinfo hints;
	struct addrinfo* res = 0;
	int err;
	int sock;

	sock = socket(AF_INET, SOCK_STREAM, 0);
	if (sock == -1) {
		fprintf(stderr, "failed to create socket: %s\n", strerror(errno));
		return 1;
	}

	memset(&hints, 0, sizeof(hints));
	hints.ai_family = AF_UNSPEC;
	hints.ai_socktype = SOCK_STREAM;
	hints.ai_protocol = IPPROTO_TCP;
	hints.ai_flags = AI_ADDRCONFIG;
	err = getaddrinfo(addr, port, &hints, &res);
	if (err != 0) {
		close(sock);
		fprintf(stderr, "failed to resolve remote socket address (err=%d / %s)\n", err, gai_strerror(err));
		return 1;
	}

	int connection = connect(sock, res->ai_addr, res->ai_addrlen);
	if (connection == -1) {
		close(sock);
		freeaddrinfo(res);
		fprintf(stderr, "Unable to create socket: %s\n", strerror(errno));
		return 2;
	}

	fprintf(stderr, "FD handle: %d\n", connection);

	conn->fd = sock;
	conn->addr = res;

	return 0;
}

int wb_free(struct wb_connection *conn) {
	freeaddrinfo(conn->addr);
	close(conn->fd);
	return 0;
}

int wb_send(struct wb_connection *conn, const void *bytes, size_t len) {
	return write(conn->fd, bytes, len);
}

int wb_recv(struct wb_connection *conn, void *bytes, size_t max_len) {
	return read(conn->fd, bytes, max_len);
}

void wb_fillpkt(uint8_t raw_pkt[16], uint32_t read_count, uint32_t write_count) {
	struct etherbone_packet *pkt = (struct etherbone_packet *)raw_pkt;
	memset(pkt, 0, sizeof(*pkt));

	pkt->magic[0] = 0x4e;
	pkt->magic[1] = 0x6f;
	pkt->version = 1;
	pkt->addr_size = ADDR_SIZE_32; // 32-byte address
	pkt->port_size = PORT_SIZE_32;
	pkt->records[0].rcount = read_count;
	pkt->records[0].wcount = write_count;
}

void wb_write32(struct wb_connection *conn, uint32_t addr, uint32_t val) {
	uint8_t raw_pkt[20];
	struct etherbone_packet *pkt = (struct etherbone_packet *)raw_pkt;
	wb_fillpkt(raw_pkt, 0, 1);

	pkt->records[0].write_addr = htobe32(addr);
	pkt->records[0].value = htobe32(val);

	wb_send(conn, raw_pkt, sizeof(raw_pkt));
}

uint32_t wb_read32(struct wb_connection *conn, uint32_t addr) {
	uint8_t raw_pkt[20];
	struct etherbone_packet *pkt = (struct etherbone_packet *)raw_pkt;
	wb_fillpkt(raw_pkt, 1, 0);
	pkt->records[0].value = htobe32(addr);

	wb_send(conn, raw_pkt, sizeof(raw_pkt));

	int count = wb_recv(conn, raw_pkt, sizeof(raw_pkt));
	if (count != sizeof(raw_pkt)) {
		fprintf(stderr, "Unexpected read length: %d\n", count);
		return -1;
	}
	return be32toh(pkt->records[0].value);
}

#if CSR_WIDTH == 8
void csr_write8(struct wb_connection *conn, uint32_t addr, uint8_t val) {
	uint8_t raw_pkt[20];
	struct etherbone_packet *pkt = (struct etherbone_packet *)raw_pkt;
	wb_fillpkt(raw_pkt, 0, 1);

	pkt->records[0].write_addr = htobe32(addr);
	pkt->records[0].value = htobe32(val & 0xff);

	wb_send(conn, raw_pkt, 20);
}

void csr_write16(struct wb_connection *conn, uint32_t addr, uint16_t val) {
	val = htole16(val);
	int i;
	for (i = 0; i < 2; i++)
		csr_write8(conn, addr + (i * 4), val >> ((1 - i) * 8));
}

void csr_write32(struct wb_connection *conn, uint32_t addr, uint32_t val) {
	val = htole32(val);
	int i;
	for (i = 0; i < 4; i++)
		csr_write8(conn, addr + (i * 4), val >> ((3 - i) * 8));
}

void csr_write64(struct wb_connection *conn, uint32_t addr, uint64_t val) {
	val = htole64(val);
	int i;
	for (i = 0; i < 8; i++)
		csr_write8(conn, addr + (i * 4), val >> ((7 - i) * 8));
}

uint8_t csr_read8(struct wb_connection *conn, uint32_t addr) {
	uint8_t raw_pkt[20];
	struct etherbone_packet *pkt = (struct etherbone_packet *)raw_pkt;
	wb_fillpkt(raw_pkt, 1, 0);
	pkt->records[0].value = htobe32(addr);

	wb_send(conn, raw_pkt, sizeof(raw_pkt));

	int count = wb_recv(conn, raw_pkt, sizeof(raw_pkt));
	if (count != sizeof(raw_pkt)) {
		fprintf(stderr, "Unexpected read length: %d\n", count);
		return -1;
	}
	return be32toh(pkt->records[0].value) & 0xff;
}

uint16_t csr_read16(struct wb_connection *conn, uint32_t addr) {
	uint16_t val = 0;
	int i;
	for (i = 0; i < 2; i++)
		val |= ((uint16_t)csr_read8(conn, addr + (i * 4))) << ((1 - i) * 8);
	return le16toh(val);
}

uint32_t csr_read32(struct wb_connection *conn, uint32_t addr) {
	uint32_t val = 0;
	int i;
	for (i = 0; i < 4; i++)
		val |= ((uint32_t)csr_read8(conn, addr + (i * 4))) << ((3 - i) * 8);
	return le32toh(val);
}

uint64_t csr_read64(struct wb_connection *conn, uint32_t addr) {
	uint64_t val = 0;
	int i;
	for (i = 0; i < 8; i++)
		val |= ((uint64_t)csr_read8(conn, addr + (i * 4))) << ((7 - i) * 8);
	return le64toh(val);
}

#elif CSR_WIDTH == 32

void csr_write32(struct wb_connection *conn, uint32_t addr, uint32_t val) {
	uint8_t raw_pkt[20];
	struct etherbone_packet *pkt = (struct etherbone_packet *)raw_pkt;
	wb_fillpkt(raw_pkt, 0, 1);

	pkt->records[0].write_addr = htobe32(addr);
	pkt->records[0].value = htobe32(val);

	wb_send(conn, raw_pkt, 20);
}

void csr_write16(struct wb_connection *conn, uint32_t addr, uint16_t val) {
	csr_write32(conn, addr, val & 0xffff);
}

void csr_write8(struct wb_connection *conn, uint32_t addr, uint8_t val) {
	csr_write32(conn, addr, val & 0xff);
}

void csr_write64(struct wb_connection *conn, uint32_t addr, uint64_t val) {
	val = htole64(val);
	int i;
	for (i = 0; i < 2; i++)
		csr_write32(conn, addr + (i * 4), val >> ((1 - i) * 8));
}

uint32_t csr_read32(struct wb_connection *conn, uint32_t addr) {
	uint8_t raw_pkt[20];
	struct etherbone_packet *pkt = (struct etherbone_packet *)raw_pkt;
	wb_fillpkt(raw_pkt, 1, 0);
	pkt->records[0].value = htobe32(addr);

	wb_send(conn, raw_pkt, sizeof(raw_pkt));

	int count = wb_recv(conn, raw_pkt, sizeof(raw_pkt));
	if (count != sizeof(raw_pkt)) {
		fprintf(stderr, "Unexpected read length: %d\n", count);
		return -1;
	}
	return be32toh(pkt->records[0].value);
}

uint16_t csr_read16(struct wb_connection *conn, uint32_t addr) {
	return csr_read32(conn, addr) & 0xffff;
}

uint8_t csr_read8(struct wb_connection *conn, uint32_t addr) {
	return csr_read32(conn, addr) & 0xff;
}

uint64_t csr_read64(struct wb_connection *conn, uint32_t addr) {
	uint64_t val = 0;
	int i;
	for (i = 0; i < 2; i++)
		val |= ((uint64_t)csr_read32(conn, addr + (i * 4))) << ((1 - i) * 8);
	return le64toh(val);
}

#else
#pragma error "Unrecognized CSR width"
#endif

int main(int argc, char **argv) {

	struct wb_connection conn;
	uint32_t address;
	uint32_t value;
	uint32_t width = 4;
	int is_write = 0;

	fprintf(stderr, "Connecting to Wishbone Bridge...\n");
	if (wb_connect(&conn, "127.0.0.1", "1234") != 0) {
		fprintf(stderr, "Unable to create connection\n");
		return 1;
	}

	fprintf(stderr, "Reading temperature: ");
	uint32_t temperature = csr_read16(&conn, 0xe0005800);
	fprintf(stderr, "Temperature: %g (0x%04x)\n", temperature * 503.975 / 4096 - 273.15, temperature);

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
		uint32_t old_val = wb_read32(&conn, address);
		wb_write32(&conn, address, value);
		uint32_t new_val = wb_read32(&conn, address);
		fprintf(stderr, "0x%08x 0x%08x -> 0x%08x (wanted: 0x%08x)\n", address, old_val, new_val, value);
	}
	else {
		fprintf(stderr, "0x%08x: 0x%08x\n", address, wb_read32(&conn, address));
	}

	wb_free(&conn);
	return 0;
}
