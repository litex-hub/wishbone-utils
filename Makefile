all:
	gcc -g -O0 -Wall vexriscv-etherbone-bridge.c -o vexriscv-etherbone-bridge
	gcc -g -O0 -Wall devmem2-netv2.c -o devmem2-netv2
