/*
 * Cortex-M3 startup for mps2-an385: vector table (FreeRTOS SVC/PendSV/SysTick
 * installed directly), .data copy, .bss zero, then main().
 */
#include <stdint.h>

extern int main(void);
void _estack(void);
void vPortSVCHandler(void);
void xPortPendSVHandler(void);
void xPortSysTickHandler(void);

extern uint32_t _sidata;
extern uint32_t _sdata;
extern uint32_t _edata;
extern uint32_t _sbss;
extern uint32_t _ebss;

void reset_handler(void)
{
	uint32_t *src = &_sidata;
	uint32_t *dst = &_sdata;

	while (dst < &_edata) {
		*dst++ = *src++;
	}
	for (dst = &_sbss; dst < &_ebss; dst++) {
		*dst = 0;
	}
	main();
	for (;;) {
	}
}

void default_handler(void)
{
	for (;;) {
	}
}

__attribute__((section(".vectors"), used))
void (*const vectors[16])(void) = {
	(void (*)(void))&_estack,
	reset_handler,
	default_handler, /* NMI */
	default_handler, /* HardFault */
	default_handler, /* MemManage */
	default_handler, /* BusFault */
	default_handler, /* UsageFault */
	0, 0, 0, 0,
	vPortSVCHandler,     /* SVCall */
	default_handler,     /* DebugMonitor */
	0,                   /* reserved */
	xPortPendSVHandler,  /* PendSV */
	xPortSysTickHandler, /* SysTick */
};
