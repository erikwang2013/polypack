/*
 * PolyPack FreeRTOS engine self-check (T-M6-07): one task calls the T-M6-02
 * C ABI (version + target validation + cache key) and exits QEMU via
 * semihosting. No polypack_engine_* signature is redefined here.
 */
#include <stdint.h>
#include <string.h>

#include "FreeRTOS.h"
#include "task.h"
#include "board.h"
#include "polypack_engine.h"

/* serde_json 解析 / 格式化在任务栈上运行，1 KB 的 configMINIMAL_STACK_SIZE 不够。 */
#define ENGINE_TASK_STACK_WORDS 4096

static const char TARGET_JSON[] =
	"{\"id\":\"freertos\",\"tier\":6,\"hosts\":[\"linux\"],"
	"\"runtime\":\"staticlib\",\"bridge\":\"cffi\",\"ui\":[],"
	"\"package\":{\"format\":\"board\",\"builder\":\"freertos-qemu\"},"
	"\"verify\":[],\"publish\":[],\"experimental\":false}";

/* Wrapper crate panic hook (see polypack-bundle embedded wrapper). */
void polypack_engine_rt_panic(void)
{
	board_console("polypack_engine panic\n");
	board_exit(1);
}

static void engine_task(void *argument)
{
	static const char sources_hex[] =
		"0000000000000000000000000000000000000000000000000000000000000000";
	static const char lockfile_hex[] =
		"1111111111111111111111111111111111111111111111111111111111111111";
	int32_t rc;
	char *key;

	(void)argument;

	board_console("polypack_engine version=");
	board_console(polypack_engine_version());
	board_console(" abi=");
	board_uint(polypack_engine_abi_version());
	board_console("\n");

	rc = polypack_engine_validate_target_json(
		(const uint8_t *)TARGET_JSON, strlen(TARGET_JSON));
	board_console("polypack_engine validate_target=");
	board_int((int)rc);
	board_console("\n");

	key = polypack_engine_cache_key(sources_hex, lockfile_hex, "freertos",
				    "thumbv7m-none-eabi", "release");
	board_console("polypack_engine cache_key=");
	board_console(key != NULL ? key : "(null)");
	board_console("\n");
	polypack_engine_string_free(key);

	board_exit(rc == 0 && key != NULL ? 0u : 1u);
}

int main(void)
{
	board_init();
	if (xTaskCreate(engine_task, "engine", ENGINE_TASK_STACK_WORDS, NULL, 2, NULL) != pdPASS) {
		board_console("polypack_engine panic: xTaskCreate failed\n");
		board_exit(1);
	}
	vTaskStartScheduler();

	board_console("polypack_engine panic: scheduler returned\n");
	board_exit(1);

	return 1;
}
