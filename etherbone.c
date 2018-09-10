#include <endian.h>
#include <string.h>

#include "etherbone.h"

int eb_unfill_read32(uint8_t wb_buffer[20]) {
    int buffer;
    uint32_t intermediate;
    memcpy(&intermediate, &wb_buffer[16], sizeof(intermediate));
    intermediate = be32toh(intermediate);
    memcpy(&buffer, &intermediate, sizeof(intermediate));
    return buffer;
}

int eb_fill_readwrite32(uint8_t wb_buffer[20], uint32_t address, uint32_t data, int is_read) {
    wb_buffer[0] = 0x4e;	// Magic byte 0
    wb_buffer[1] = 0x6f;	// Magic byte 1
    wb_buffer[2] = 0x10;	// Version 1, all other flags 0
    wb_buffer[3] = 0x44;	// Address is 32-bits, port is 32-bits
    wb_buffer[4] = 0;		// Padding
    wb_buffer[5] = 0;		// Padding
    wb_buffer[6] = 0;		// Padding
    wb_buffer[7] = 0;		// Padding

    // Record
    wb_buffer[8] = 0;		// No Wishbone flags are set (cyc, wca, wff, etc.)
    wb_buffer[9] = 0x0f;	// Byte enable

    if (is_read) {
        wb_buffer[10] = 0;  // Write count
        wb_buffer[11] = 1;	// Read count
        data = htobe32(address);
        memcpy(&wb_buffer[16], &data, sizeof(data));
    }
    else {
        wb_buffer[10] = 1;	// Write count
        wb_buffer[11] = 0;  // Read count
        address = htobe32(address);
        memcpy(&wb_buffer[12], &address, sizeof(address));

        data = htobe32(data);
        memcpy(&wb_buffer[16], &data, sizeof(data));
    }
    return 20;
}

int eb_fill_write32(uint8_t wb_buffer[20], uint32_t address, uint32_t data) {
    return eb_fill_readwrite32(wb_buffer, address, data, 0);
}

int eb_fill_read32(uint8_t wb_buffer[20], uint32_t address) {
    return eb_fill_readwrite32(wb_buffer, address, 0, 1);
}

#if 0
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
#endif