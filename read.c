#include <stdio.h>
#include <unistd.h>
#include <stdint.h>
#include <fcntl.h>
#include <sys/types.h>
#include <endian.h>

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

	// 2...
	uint8_t byte_enable;

	// 3...
	uint8_t wcount;

	// 4...
	uint8_t rcount;

	// 5... 6... 7... 8...
	uint8_t padding[4];

	union {
		uint8_t bytes[0];
		uint8_t shorts[0];
		uint8_t longs[0];
	};
} __attribute__((packed));

struct etherbone_packet {
	// 1... 2...
	uint8_t magic[2]; // 0x4e 0x6f

	// 3...
#if __BYTE_ORDER == __BIG_ENDIAN
	uint8_t ver : 4;
	uint8_t ign : 1;
	uint8_t no_reads : 1;
	uint8_t probe_reply : 1;
	uint8_t probe_flag : 1;
#elif __BYTE_ORDER == __LITTLE_ENDIAN
	uint8_t probe_flag : 1;
	uint8_t probe_reply : 1;
	uint8_t no_reads : 1;
	uint8_t ign : 1;
	uint8_t ver : 4;
#else
#pragma error "Unrecognized byte order"
#endif

	// 4...
#if __BYTE_ORDER == __BIG_ENDIAN
	uint8_t port_size : 4;
	uint8_t addr_size : 4;
#elif __BYTE_ORDER == __LITTLE_ENDIAN
	uint8_t addr_size : 4;
	uint8_t port_size : 4;
#else
#pragma error "Unrecognized byte order"
#endif
	// 5... 6... 7... 8...
	uint8_t padding[4];

	struct etherbone_record records[0];
} __attribute__((packed));

int main(int argc, char **argv) {
	struct etherbone_packet *pkt;
	uint8_t tst[] = {0x4e, 0x6f,
                         0x10,
                         0x44,
                         0x00, 0x00, 0x00, 0x00,
// 00 0f 00 01 00 00 00 00
// e0 00 58 18
                         0x00, 0x0f, 0x00, 0x01,
                         0x00, 0x00, 0x00, 0x00,
                         0xe0, 0x00, 0xa0, 0x24};
	pkt = (struct etherbone_packet *)tst;
	printf("Hello - 0x%02x\n", pkt->magic[0]);
	return 0;
}
