#include <stdio.h>
#include <stdlib.h>
#include <getopt.h>
#include <string.h>

#include "etherbone.h"

#ifdef DEBUG
#define DEBUG_RISCV(str, ...) fprintf(stderr, str, __VA_ARGS__)
#else
#define DEBUG_RISCV(str, ...)
#endif

int main(int argc, char **argv) {

    struct eb_connection *conn;
    const char *host_address = "127.0.0.1";
    const char *host_port = "1234";
    int direct_connection = 0;
    int is_write = 0;
    int c;
    uint32_t address = 0;
    uint32_t value = 0;

    while (1) {
        int this_option_optind = optind ? optind : 1;
        int option_index = 0;
        static struct option long_options[] = {
            {"address", required_argument, 0, 'a'},
            {"value", required_argument, 0, 'v'},
            {"target", required_argument, 0, 't'},
            {"port", required_argument, 0, 'p'},
            {"direct", no_argument, 0, 'd'},
            {0, 0, 0, 0},
        };

        c = getopt_long(argc, argv, "h:p:d", long_options, &option_index);
        if (c == -1)
            break;

        switch (c) {
        case 0:
            printf("option %s", long_options[option_index].name);
            if (optarg)
                printf(" with arg %s", optarg);
            printf("\n");
            break;

        case 'a':
            fprintf(stderr, "Setting host address: %s\n", optarg);
            address = strtoul(optarg, NULL, 0);
            break;

        case 'v':
            fprintf(stderr, "Setting value: %s\n", optarg);
            value = strtoul(optarg, NULL, 0);
            is_write = 1;
            break;

        case 't':
            fprintf(stderr, "Setting target address: %s\n", optarg);
            host_address = strdup(optarg);
            break;

        case 'p':
            fprintf(stderr, "Setting host port: %s\n", optarg);
            host_port = strdup(optarg);
            break;

        case 'd':
            fprintf(stderr, "Setting direct connection\n");
            direct_connection = 1;
            break;
        default:
            printf("?? getopt returned character code 0%o ??\n", c);
            return 1;
            break;
        }
    }

    conn = eb_connect(host_address, host_port);
    if (!conn) {
        fprintf(stderr, "Unable to create connection\n");
        return 1;
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
