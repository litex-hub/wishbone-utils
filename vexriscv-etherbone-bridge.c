#include <stdio.h>
#include <unistd.h>
#include <stdint.h>
#include <fcntl.h>
#include <sys/types.h>
#include <endian.h>

#include <errno.h>
#include <string.h>
#include <netdb.h>
#include <sys/socket.h>
#include <netinet/in.h>

#include <sys/uio.h>

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
	int write_fd;
	int read_fd;
	struct addrinfo* addr;
};

int wb_connect(struct wb_connection *conn, const char *addr, const char *port) {

	struct sockaddr_in si_me;
	struct addrinfo hints;
	struct addrinfo* res = 0;
	int err;

	// Rx half
     
	// zero out the structure
	memset((char *) &si_me, 0, sizeof(si_me));
     
	si_me.sin_family = AF_INET;
	si_me.sin_port = htobe16(1234);
	si_me.sin_addr.s_addr = htobe32(INADDR_ANY);

	int rx_socket;
	if ((rx_socket = socket(AF_INET, SOCK_DGRAM, IPPROTO_UDP)) == -1) {
		fprintf(stderr, "Unable to create Rx socket: %s\n", strerror(errno));
		return 10;
	}
	if (bind(rx_socket, (struct sockaddr*)&si_me, sizeof(si_me)) == -1) {
		fprintf(stderr, "Unable to bind Rx socket to port: %s\n", strerror(errno));
		return 11;
	}
	//freeaddrinfo(res);

	// Tx half
	memset(&hints, 0, sizeof(hints));
	hints.ai_family = AF_UNSPEC;
	hints.ai_socktype = SOCK_DGRAM;
	hints.ai_protocol = 0;
	hints.ai_flags = AI_ADDRCONFIG;
	err = getaddrinfo(addr, port, &hints, &res);
	if (err != 0) {
		fprintf(stderr, "failed to resolve remote socket address (err=%d / %s)\n", err, gai_strerror(err));
		return 1;
	}

	int tx_socket = socket(res->ai_family, res->ai_socktype, res->ai_protocol);
	if (tx_socket == -1) {
		fprintf(stderr, "Unable to create socket: %s\n", strerror(errno));
		return 2;
	}


	conn->read_fd = rx_socket;
	conn->write_fd = tx_socket;
	conn->addr = res;

	return 0;
}

int wb_free(struct wb_connection *conn) {
	freeaddrinfo(conn->addr);
	close(conn->read_fd);
	close(conn->write_fd);
	return 0;
}

int wb_send(struct wb_connection *conn, const void *bytes, size_t len) {
	return sendto(conn->write_fd, bytes, len, 0, conn->addr->ai_addr, conn->addr->ai_addrlen);
}

int wb_recv(struct wb_connection *conn, void *bytes, size_t max_len) {
	return recvfrom(conn->read_fd, bytes, max_len, 0, NULL, NULL);
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

void wb_write8(struct wb_connection *conn, uint32_t addr, uint8_t val) {
	//fprintf(stderr, "Writing 0x%08x = 0x%02x\n", addr, val);
	uint8_t raw_pkt[20];
	struct etherbone_packet *pkt = (struct etherbone_packet *)raw_pkt;
	wb_fillpkt(raw_pkt, 0, 1);

	pkt->records[0].write_addr = htobe32(addr);
	pkt->records[0].value = htobe32(val & 0xff);

	wb_send(conn, raw_pkt, 20);
}

void wb_write16(struct wb_connection *conn, uint32_t addr, uint16_t val) {
	val = htole16(val);
	int i;
	for (i = 0; i < 2; i++)
		wb_write8(conn, addr + (i * 4), val >> ((1 - i) * 8));
}

void wb_write32(struct wb_connection *conn, uint32_t addr, uint32_t val) {
	val = htole32(val);
	int i;
	for (i = 0; i < 4; i++)
		wb_write8(conn, addr + (i * 4), val >> ((3 - i) * 8));
}

void wb_write64(struct wb_connection *conn, uint32_t addr, uint64_t val) {
	val = htole64(val);
	int i;
	for (i = 0; i < 8; i++)
		wb_write8(conn, addr + (i * 4), val >> ((7 - i) * 8));
}

uint8_t wb_read8(struct wb_connection *conn, uint32_t addr) {
	uint8_t raw_pkt[68];
	struct etherbone_packet *pkt = (struct etherbone_packet *)raw_pkt;
	wb_fillpkt(raw_pkt, 1, 0);
	pkt->records[0].value = htobe32(addr);

	wb_send(conn, raw_pkt, 20);

	int count = wb_recv(conn, raw_pkt, sizeof(raw_pkt));
	if (count != 20) {
		fprintf(stderr, "Unexpected read length: %d\n", count);
		return -1;
	}
	return be32toh(pkt->records[0].value) & 0xff;
}

uint16_t wb_read16(struct wb_connection *conn, uint32_t addr) {
	uint16_t val = 0;
	int i;
	for (i = 0; i < 2; i++)
		val |= ((uint16_t)wb_read8(conn, addr + (i * 4))) << ((1 - i) * 8);
	return le16toh(val);
}

uint32_t wb_read32(struct wb_connection *conn, uint32_t addr) {
	uint32_t val = 0;
	int i;
	for (i = 0; i < 4; i++)
		val |= ((uint32_t)wb_read8(conn, addr + (i * 4))) << ((3 - i) * 8);
	return le32toh(val);
}

uint64_t wb_read64(struct wb_connection *conn, uint32_t addr) {
	uint64_t val = 0;
	int i;
	for (i = 0; i < 8; i++)
		val |= ((uint64_t)wb_read8(conn, addr + (i * 4))) << ((7 - i) * 8);
	return le64toh(val);
}

#include "debug.h"

void riscv_debug_write32(struct wb_connection *conn, uint8_t addr, uint32_t value) {
	wb_write8(conn, CSR_CPU_OR_BRIDGE_I_DEBUG_BUS_CMD_VALID_ADDR, 0);
//	(void)wb_read8(conn, CSR_CPU_OR_BRIDGE_I_DEBUG_BUS_CMD_VALID_ADDR);

	wb_write8(conn, CSR_CPU_OR_BRIDGE_I_DEBUG_BUS_CMD_PAYLOAD_WR_ADDR, 1);
//	(void)wb_read8(conn, CSR_CPU_OR_BRIDGE_I_DEBUG_BUS_CMD_PAYLOAD_WR_ADDR);

	wb_write8(conn, CSR_CPU_OR_BRIDGE_I_DEBUG_BUS_CMD_PAYLOAD_ADDRESS_ADDR, addr);
//	(void)wb_read8(conn, CSR_CPU_OR_BRIDGE_I_DEBUG_BUS_CMD_PAYLOAD_ADDRESS_ADDR);

	wb_write32(conn, CSR_CPU_OR_BRIDGE_I_DEBUG_BUS_CMD_PAYLOAD_DATA_ADDR, value);
	(void)wb_read32(conn, CSR_CPU_OR_BRIDGE_I_DEBUG_BUS_CMD_PAYLOAD_DATA_ADDR);

	wb_write8(conn, CSR_CPU_OR_BRIDGE_I_DEBUG_BUS_CMD_VALID_ADDR, 1);
//	(void)wb_read8(conn, CSR_CPU_OR_BRIDGE_I_DEBUG_BUS_CMD_VALID_ADDR);
}

uint32_t riscv_debug_read32(struct wb_connection *conn, uint8_t addr) {
	wb_write8(conn, CSR_CPU_OR_BRIDGE_I_DEBUG_BUS_CMD_VALID_ADDR, 0);
//	(void)wb_read8(conn, CSR_CPU_OR_BRIDGE_I_DEBUG_BUS_CMD_VALID_ADDR);

	wb_write8(conn, CSR_CPU_OR_BRIDGE_I_DEBUG_BUS_CMD_PAYLOAD_WR_ADDR, 0);
//	(void)wb_read8(conn, CSR_CPU_OR_BRIDGE_I_DEBUG_BUS_CMD_PAYLOAD_WR_ADDR);


	wb_write8(conn, CSR_CPU_OR_BRIDGE_I_DEBUG_BUS_CMD_PAYLOAD_ADDRESS_ADDR, addr);
//	(void)wb_read8(conn, CSR_CPU_OR_BRIDGE_I_DEBUG_BUS_CMD_PAYLOAD_ADDRESS_ADDR);

	wb_write32(conn, CSR_CPU_OR_BRIDGE_I_DEBUG_BUS_CMD_PAYLOAD_DATA_ADDR, 0);
	(void)wb_read32(conn, CSR_CPU_OR_BRIDGE_I_DEBUG_BUS_CMD_PAYLOAD_DATA_ADDR);


	wb_write8(conn, CSR_CPU_OR_BRIDGE_I_DEBUG_BUS_CMD_VALID_ADDR, 1);
//	(void)wb_read8(conn, CSR_CPU_OR_BRIDGE_I_DEBUG_BUS_CMD_VALID_ADDR);

	while (!(wb_read8(conn, CSR_CPU_OR_BRIDGE_O_DEBUG_BUS_CMD_READY_ADDR) & 1))
		;

	return wb_read32(conn, CSR_CPU_OR_BRIDGE_O_DEBUG_BUS_RSP_DATA_ADDR);
}

#define VRV_RW_READ 0
#define VRV_RW_WRITE 1

struct vexriscv_req {
	uint8_t readwrite;
	uint8_t size;
	uint32_t address;
	uint32_t data;
} __attribute__((packed));

struct vexriscv_server {
	int socket_fd;
	int connect_fd;
};

int vrv_init(struct vexriscv_server *server) {

	memset(server, 0, sizeof(*server));

	struct sockaddr_in sa;
    server->socket_fd = socket(PF_INET, SOCK_STREAM, IPPROTO_TCP);
    if (server->socket_fd == -1) {
		perror("cannot create socket");
		return 1;
    }
  
    memset(&sa, 0, sizeof sa);
  
    sa.sin_family = AF_INET;
    sa.sin_port = htons(7893);
    sa.sin_addr.s_addr = htonl(INADDR_ANY);
  
    if (bind(server->socket_fd, (struct sockaddr *)&sa, sizeof sa) == -1) {
		perror("bind failed");
		close(server->socket_fd);
		return 1;
    }
  
    if (listen(server->socket_fd, 10) == -1) {
		perror("listen failed");
		close(server->socket_fd);
		return 1;
    }
	return 0;
}

int vrv_accept(struct vexriscv_server *server) {
	server->connect_fd = accept(server->socket_fd, NULL, NULL);
  
	if (0 > server->connect_fd) {
		perror("accept failed");
		close(server->socket_fd);
		return 1;
	}
	return 0;
}

size_t vrv_read(struct vexriscv_server *server, void *bfr, size_t max_size) {
	return read(server->connect_fd, bfr, max_size);
}

size_t vrv_write(struct vexriscv_server *server, void *bfr, size_t size) {
	return write(server->connect_fd, bfr, size);
}

int vrv_shutdown(struct vexriscv_server *server) {
	if (server->connect_fd) {
		if (shutdown(server->connect_fd, SHUT_RDWR) == -1) {
			perror("shutdown failed");
			return 1;
		}
		server->connect_fd = 0;
	}
	return 0;
}

int main(int argc, char **argv) {

	struct vexriscv_server server;
	struct wb_connection conn;

	fprintf(stderr, "Setting up VexRiscV debug server...\n");
	if (vrv_init(&server)) {
		fprintf(stderr, "Unable to set up VexRiscV server\n");
		return 1;
	}

	fprintf(stderr, "Connecting to Wishbone Bridge...\n");
	if (wb_connect(&conn, "10.0.11.2", "1234") != 0) {
		fprintf(stderr, "Unable to create connection\n");
		return 1;
	}

	uint32_t temperature = wb_read16(&conn, 0xe0005800);
	fprintf(stderr, "Temperature: %g (0x%04x)\n", temperature * 503.975 / 4096 - 273.15, temperature);

	while (1) {
		uint8_t vrv_bfr[10];
		size_t vrv_read_size;
		struct vexriscv_req *req;

		if (server.connect_fd <= 0) {
			printf("Accepting new server connection...\n");
			if (vrv_accept(&server)) {
				return 1;
			}
			fprintf(stderr, "Accepted connection from openocd\n");
		}

		vrv_read_size = vrv_read(&server, vrv_bfr, sizeof(vrv_bfr));
		if (vrv_read_size <= 0) {
			if (vrv_shutdown(&server)) {
				fprintf(stderr, "Unable to disconnect\n");
				return 1;
			}
			continue;
		}

		if (vrv_read_size != 10) {
			fprintf(stderr, "Unrecognized read size: %lu\n", vrv_read_size);
			continue;
		}

		req = (struct vexriscv_req *)vrv_bfr;

		uint32_t resp;

		if ((req->address >= 0xf00f0000) && (req->address < 0xf00f0008)) {
			req->address -= 0xf00f0000;
			if (req->readwrite == VRV_RW_WRITE) {
				switch (req->size) {
				case 0:
					fprintf(stderr, "Unrecognized size for writing: 0 (8-bits)\n");
					break;
				case 1:
					fprintf(stderr, "Unrecognized size for writing: 1 (16-bits)\n");
					break;
				case 2:
					fprintf(stderr, "32-bit debug write 0x%08x = 0x%08x\n", req->address, req->data);
					riscv_debug_write32(&conn, req->address, req->data);
					break;
				default:
					fprintf(stderr, "Unrecognized size for writing: %d\n", req->size);
					break;
				}
			}
			else if (req->readwrite == VRV_RW_READ) {
				switch (req->size) {
				case 0:
					fprintf(stderr, "Unrecognized size for reading: 0 (8-bits)\n");
					break;
				case 1:
					fprintf(stderr, "Unrecognized size for reading: 1 (16-bits)\n");
					break;
				case 2:
					resp = riscv_debug_read32(&conn, req->address);
					fprintf(stderr, "32-bit debug read 0x%08x = 0x%08x [0x%08x]\n", req->address, resp, req->data);
					break;
				default:
					fprintf(stderr, "Unrecognized size for reading: %d\n", req->size);
					break;
				}
			}
			else {
				fprintf(stderr, "Unrecognized readwrite command: %d\n", req->readwrite);
			}
		}
		else {
			if (req->readwrite == VRV_RW_WRITE) {
				switch (req->size) {
				case 0:
					fprintf(stderr, "8-bit normal write 0x%08x = 0x%02x\n", req->address, req->data & 0xff);
					wb_write8(&conn, req->address, req->data);
					break;
				case 1:
					fprintf(stderr, "16-bit normal write 0x%08x = 0x%04x\n", req->address, req->data & 0xffff);
					wb_write16(&conn, req->address, req->data);
					break;
				case 2:
					fprintf(stderr, "32-bit debug write 0x%08x = 0x%08x\n", req->address, req->data);
					wb_write32(&conn, req->address, req->data);
					break;
				default:
					fprintf(stderr, "Unrecognized size for writing: %d\n", req->size);
					break;
				}
			}
			else if (req->readwrite == VRV_RW_READ) {
				switch (req->size) {
				case 0:
					resp = wb_read8(&conn, req->address);
					fprintf(stderr, "8-bit normal read 0x%08x = 0%02x\n", req->address, resp & 0xff);
					break;
				case 1:
					resp = wb_read16(&conn, req->address);
					fprintf(stderr, "16-bit normal read 0x%08x = 0%04x\n", req->address, resp & 0xffff);
					break;
				case 2:
					resp = wb_read32(&conn, req->address);
					fprintf(stderr, "32-bit normal read 0x%08x = 0%08x\n", req->address, resp);
					break;
				default:
					fprintf(stderr, "Unrecognized size for reading: %d\n", req->size);
					break;
				}
			}
			else {
				fprintf(stderr, "Unrecognized readwrite: %d\n", req->readwrite);
			}
		}

		// Send a response, which is always a 4-byte value.
		if (req->readwrite == VRV_RW_READ) {
			vrv_write(&server, &resp, sizeof(resp));
		}
	}
	/*
	riscv_write32(&conn, 0, (1 << 16));
	fprintf(stderr, "CPU state: 0x%08x\n", riscv_read32(&conn, 0));
	riscv_write32(&conn, 0, (1 << 24));
	fprintf(stderr, "CPU state: 0x%08x\n", riscv_read32(&conn, 0));
	fprintf(stderr, "Value at 0xe000a020: %d\n", wb_read8(&conn, 0xe000a020));
	wb_write8(&conn, 0xe000a020, 0);
	fprintf(stderr, "Value at 0xe000a020: %d\n", wb_read8(&conn, 0xe000a020));
	wb_write8(&conn, 0xe000a020, 1);
	fprintf(stderr, "Value at 0xe000a020: %d\n", wb_read8(&conn, 0xe000a020));

	wb_write8(&conn, CSR_CPU_OR_BRIDGE_I_DEBUG_BUS_CMD_VALID_ADDR, 0);
	wb_write8(&conn, CSR_CPU_OR_BRIDGE_I_DEBUG_BUS_CMD_PAYLOAD_WR_ADDR, 1);
	wb_write8(&conn, CSR_CPU_OR_BRIDGE_I_DEBUG_BUS_CMD_PAYLOAD_ADDRESS_ADDR, 0);
	wb_write32(&conn, CSR_CPU_OR_BRIDGE_I_DEBUG_BUS_CMD_PAYLOAD_DATA_ADDR, (1 << 24) | (1 << 25));
	//wb_write32(&conn, CSR_CPU_OR_BRIDGE_I_DEBUG_BUS_CMD_PAYLOAD_DATA_ADDR, 0);
	wb_write8(&conn, CSR_CPU_OR_BRIDGE_I_DEBUG_BUS_CMD_VALID_ADDR, 1);

	fprintf(stderr, "CSR_CPU_OR_BRIDGE_I_DEBUG_BUS_CMD_VALID: 0x%02x\n", wb_read8(&conn, CSR_CPU_OR_BRIDGE_I_DEBUG_BUS_CMD_VALID_ADDR));
	fprintf(stderr, "CSR_CPU_OR_BRIDGE_I_DEBUG_BUS_CMD_PAYLOAD_WR: 0x%02x\n", wb_read8(&conn, CSR_CPU_OR_BRIDGE_I_DEBUG_BUS_CMD_PAYLOAD_WR_ADDR));
	fprintf(stderr, "CSR_CPU_OR_BRIDGE_O_DEBUG_BUS_CMD_READY: 0x%02x\n", wb_read8(&conn, CSR_CPU_OR_BRIDGE_O_DEBUG_BUS_CMD_READY_ADDR));
	fprintf(stderr, "CSR_CPU_OR_BRIDGE_O_DEBUG_BUS_RSP_DATA: 0x%08x\n", wb_read32(&conn, CSR_CPU_OR_BRIDGE_O_DEBUG_BUS_RSP_DATA_ADDR));
	*/
	wb_free(&conn);
	return 0;
}
