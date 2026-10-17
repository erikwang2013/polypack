/*
 * mps2-an385 console: CMSDK APB UART0 at 0x40004000 (STATE bit0 = TXBF),
 * plus ARM semihosting SYS_EXIT_EXTENDED for the QEMU exit code.
 */
#include <stdint.h>

#include "board.h"

#define UART0 ((volatile uint32_t *)0x40004000)

static void uart_putc(char c)
{
	while (UART0[1] & 1u) {
		/* wait while the transmit buffer is full */
	}
	*(volatile uint8_t *)&UART0[0] = (uint8_t)c;
}

void board_init(void)
{
	UART0[2] = 3;  /* CTRL: TXEN | RXEN */
	UART0[4] = 16; /* BAUDDIV */
}

void board_console(const char *text)
{
	for (; *text != '\0'; text++) {
		uart_putc(*text);
	}
}

void board_uint(unsigned value)
{
	char digits[12];
	int count = 0;

	do {
		digits[count++] = (char)('0' + value % 10u);
		value /= 10u;
	} while (value != 0u);
	while (count > 0) {
		uart_putc(digits[--count]);
	}
}

void board_int(int value)
{
	if (value < 0) {
		uart_putc('-');
		board_uint((unsigned)(-(value + 1)) + 1u);
		return;
	}
	board_uint((unsigned)value);
}

void board_exit(unsigned code)
{
	/* ARM semihosting: SYS_EXIT_EXTENDED (0x20) with
	 * { ADP_Stopped_ApplicationExit (0x20026), exit code }.
	 * Plain SYS_EXIT makes QEMU always exit 1. */
	unsigned words[2] = {0x20026u, code};
	register unsigned r0 __asm__("r0") = 0x20u;
	register unsigned *r1 __asm__("r1") = words;

	__asm__ volatile("bkpt 0xab" : "+r"(r0) : "r"(r1) : "memory");
	for (;;) {
	}
}
