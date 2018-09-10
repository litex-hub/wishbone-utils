#include <stdio.h>
#include <stdlib.h>
#include <getopt.h>
#include <string.h>

#include "etherbone.h"

static void print_help(const char *progname) {
    printf("Usage: %s [-t|--target target] [-p|--port port]\n"
           "                  [-a|--address address] [-v|--value value]\n"
           "                  [-d|--direct]\n", progname);
    printf("Target address defaults to 127.0.0.1, and port defaults to 1234.");
    printf("Connects to a device over Etherbone or the LiteX bridge, and accesses Wishbone.\n");
    printf("To connect directly over Etherbone without using litex_server, use --direct.\n");
    printf("If --value is omitted, then a read is performed.  Otherwise, a write is performed.\n");

    return;
}

int main(int argc, char **argv) {

    struct eb_connection *conn;
    const char *host_address = "127.0.0.1";
    const char *host_port = "1234";
    int direct_connection = 0;
    int have_value = 0;
    int have_address = 0;
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
            {"help", no_argument, 0, 'h'},
            {0, 0, 0, 0},
        };

        c = getopt_long(argc, argv, "a:v:t:p:dh", long_options, &option_index);
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
            have_address = 1;
            break;

        case 'v':
            fprintf(stderr, "Setting value: %s\n", optarg);
            value = strtoul(optarg, NULL, 0);
            have_value = 1;
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

        case 'h':
            print_help(argv[0]);
            return 0;

        default:
            printf("Unrecognized option\n");
            print_help(argv[0]);
            return 1;
        }
    }

    if (optind < argc) {
        if (!have_address) {
            address = strtoul(argv[optind++], NULL, 0);
            have_address = 1;
        }

        if (!have_value && (optind < argc)) {
            value = strtoul(argv[optind++], NULL, 0);
            have_value = 1;
        }
    }

    if (!have_address) {
        printf("%s: Must specify an address\n", argv[0]);
        return 1;
    }

    conn = eb_connect(host_address, host_port, direct_connection);
    if (!conn) {
        fprintf(stderr, "Unable to create connection\n");
        return 1;
    }

    if (have_value) {
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
