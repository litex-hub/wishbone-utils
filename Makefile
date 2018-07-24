all:
	gcc -g -O0 -Wall vexriscv-etherbone-bridge.c -o vexriscv-etherbone-bridge
	gcc -g -O0 -Wall netv2-devmem2.c -o netv2-devmem2
	gcc -g -O0 -Wall litex-devmem2.c -o litex-devmem2
