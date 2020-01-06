CFLAGS = -O0 -Wall -ggdb2
CC?= gcc

all: litex-devmem2

etherbone.o: etherbone.c etherbone.h
	${CC} -c $(CFLAGS) etherbone.c -o etherbone.o

litex-devmem2.o: litex-devmem2.c etherbone.h
	${CC} -c $(CFLAGS) litex-devmem2.c -o litex-devmem2.o

litex-devmem2: etherbone.o litex-devmem2.o
	${CC} $(CFLAGS) etherbone.o litex-devmem2.o -o litex-devmem2

clean:
	rm -f etherbone.o litex-devmem2.o