/*
 * PolyPack Zephyr 引擎自检（T-M6-06 / T-M6-10）：只调用 T-M6-02 生成的 C ABI，
 * 不重写 / 不扩展任何 polypack_engine_* 签名。
 *
 * 输出契约（与 QNX 冒烟共用同一份结果解析，见 polypack-cli 的 smoke 模块）：
 *   每项自检打印 `[POLYPACK-STEP <name>] PASS|FAIL`；
 *   全部通过时打印 `SMOKE PASS` 并以板级约定退出码结束。
 *
 * 退出码约定（`west build -t run` 透传 QEMU 退出码）：
 *   - qemu_cortex_m3：CONFIG_SEMIHOST=y（boards/qemu_cortex_m3.conf），
 *     ARM semihosting SYS_EXIT_EXTENDED，成功 = 0；
 *   - qemu_x86：isa-debug-exit（board.cmake 固定 iobase 0xf4），QEMU 退出码为
 *     (code << 1) | 1，成功 code = 0 → QEMU 退出 1（isa-debug-exit 只产奇数）。
 */
#include <zephyr/kernel.h>
#include <stdint.h>
#include <string.h>

#include "polypack_engine.h"

static const char TARGET_JSON[] =
	"{\"id\":\"zephyr\",\"tier\":6,\"hosts\":[\"linux\"],"
	"\"runtime\":\"staticlib\",\"bridge\":\"cffi\",\"ui\":[],"
	"\"package\":{\"format\":\"west\",\"builder\":\"zephyr-west\"},"
	"\"verify\":[],\"publish\":[],\"experimental\":false}";

static void step(const char *name, int ok)
{
	printk("[POLYPACK-STEP %s] %s\n", name, ok ? "PASS" : "FAIL");
}

#if defined(CONFIG_SEMIHOST)
static void smoke_exit(unsigned code)
{
	/* ARM semihosting: SYS_EXIT_EXTENDED (0x20) with
	 * { ADP_Stopped_ApplicationExit (0x20026), exit code }. */
	unsigned words[2] = {0x20026u, code};
	register unsigned r0 __asm__("r0") = 0x20u;
	register unsigned *r1 __asm__("r1") = words;

	__asm__ volatile("bkpt 0xab" : "+r"(r0) : "r"(r1) : "memory");
	for (;;) {
	}
}
#else
static void smoke_exit(unsigned code)
{
	/* isa-debug-exit: outl(code, 0xf4) -> QEMU exit (code << 1) | 1. */
	__asm__ volatile("outl %0, %1" : : "a"(code), "Nd"(0xf4));
	for (;;) {
	}
}
#endif

int main(void)
{
	int32_t rc;
	char *key;
	int ok = 1;
	static const char sources_hex[] =
		"0000000000000000000000000000000000000000000000000000000000000000";
	static const char lockfile_hex[] =
		"1111111111111111111111111111111111111111111111111111111111111111";

	printk("polypack_engine version=%s abi=%u\n",
	       polypack_engine_version(), (unsigned)polypack_engine_abi_version());
	step("version", polypack_engine_version() != NULL);
	ok &= polypack_engine_version() != NULL;

	rc = polypack_engine_validate_target_json(
		(const uint8_t *)TARGET_JSON, strlen(TARGET_JSON));
	step("validate-target", rc == 0);
	ok &= rc == 0;

	key = polypack_engine_cache_key(sources_hex, lockfile_hex, "zephyr",
				    "thumbv7m-none-eabi", "release");
	step("cache-key", key != NULL);
	ok &= key != NULL;
	polypack_engine_string_free(key);

	/* isa-debug-exit 会立刻终止 QEMU，控制台 FIFO 里尚未排空的标记会丢失
	 * （CI qemu_x86 实证：其余步骤全过、退出码也正确，但日志只剩 version 行）。
	 * 先让出 CPU 一小段时间给 UART 驱动排空，再按板级约定退出。 */
	k_sleep(K_MSEC(100));

	if (ok) {
		printk("SMOKE PASS\n");
		k_sleep(K_MSEC(100));
		smoke_exit(0);
	}
	printk("SMOKE FAIL\n");
	k_sleep(K_MSEC(100));
	smoke_exit(1);
	return 1;
}
