#ifndef POLYPACK_BOARD_H
#define POLYPACK_BOARD_H

/* Board glue for the mps2-an385 sample: UART console + semihosting exit. */

void board_init(void);
void board_console(const char *text);
void board_uint(unsigned value);
void board_int(int value);
void board_exit(unsigned code) __attribute__((noreturn));

#endif /* POLYPACK_BOARD_H */
