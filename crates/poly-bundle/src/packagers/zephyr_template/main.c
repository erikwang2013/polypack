/*
 * PolyPack Zephyr 引擎自检（T-M6-06）：只调用 T-M6-02 生成的 C ABI，
 * 不重写 / 不扩展任何 poly_engine_* 签名。
 */
#include <zephyr/kernel.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

#include "polypack_engine.h"

static const char TARGET_JSON[] =
	"{\"id\":\"zephyr\",\"tier\":6,\"hosts\":[\"linux\"],"
	"\"runtime\":\"staticlib\",\"bridge\":\"cffi\",\"ui\":[],"
	"\"package\":{\"format\":\"west\",\"builder\":\"zephyr-west\"},"
	"\"verify\":[],\"publish\":[],\"experimental\":false}";

int main(void)
{
	int32_t rc;
	char *key;
	static const char sources_hex[] =
		"0000000000000000000000000000000000000000000000000000000000000000";
	static const char lockfile_hex[] =
		"1111111111111111111111111111111111111111111111111111111111111111";

	printf("poly_engine version=%s abi=%u\n",
	       poly_engine_version(), (unsigned)poly_engine_abi_version());

	rc = poly_engine_validate_target_json(
		(const uint8_t *)TARGET_JSON, strlen(TARGET_JSON));
	printf("poly_engine validate_target=%d\n", (int)rc);

	key = poly_engine_cache_key(sources_hex, lockfile_hex, "zephyr",
				    "thumbv7m-none-eabi", "release");
	printf("poly_engine cache_key=%s\n", key != NULL ? key : "(null)");
	poly_engine_string_free(key);

	return rc == 0 ? 0 : 1;
}
