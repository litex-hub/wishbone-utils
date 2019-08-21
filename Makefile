PROG = litex-devmem2
OBJS = litex-devmem2.o etherbone.o

CFLAGS ?= -O2 -g
CFLAGS += -MMD -Wall -Wextra

PREFIX ?= /usr/local
BINDIR ?= ${PREFIX}/bin

${PROG}: ${OBJS}
	${CC} ${CFLAGS} -o $@ ${OBJS}

install:
	install -D -m 0755 -t ${DESTDIR}${BINDIR} ${PROG}

clean:
	rm -f ${PROG} ${OBJS} ${OBJS:.o=.d}

.PHONY: install clean

-include ${OBJS:.o=.d}
