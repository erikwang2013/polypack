# PolyPack FreeRTOS sample board (T-M6-07): mps2-an385 (Cortex-M3) under QEMU.
#
# Reproducible commands (also recorded in poly-template.json):
#   make fetch   fetch + sha256-verify the pinned FreeRTOS kernel (V11.1.0)
#   make         build build/freertos-sample.elf (fetches the kernel if absent)
#   make run     run in QEMU; the semihosting exit code is QEMU's exit code
#   make smoke   build + run + assert the engine version line and exit code 0

CROSS         ?= arm-none-eabi-
CC            := $(CROSS)gcc
SIZE          := $(CROSS)size
QEMU          ?= qemu-system-arm
BOARD         ?= mps2-an385
KERNEL_TAG    ?= V11.1.0
KERNEL_SHA256 ?= 0e21928b3bcc4f9bcaf7333fb1c8c0299d97e2ec9e13e3faa2c5a7ac8a3bc573
KERNEL_DIR    ?= $(CURDIR)/.freertos-kernel
BUILD_DIR     ?= $(CURDIR)/build
ENGINE_LIB    ?= ../lib/arm/libpolypack_engine.a
ENGINE_INC    ?= ../include

KERNEL_FILES := $(KERNEL_DIR)/tasks.c $(KERNEL_DIR)/queue.c $(KERNEL_DIR)/list.c \
                $(KERNEL_DIR)/portable/GCC/ARM_CM3/port.c \
                $(KERNEL_DIR)/portable/MemMang/heap_4.c
BOARD_SRC    := src/startup.c src/main.c src/uart.c
CFLAGS       := -mcpu=cortex-m3 -mthumb -Os -ffreestanding -fno-common \
                -nostartfiles -T linker.ld -Wl,--gc-sections \
                -I. -I$(ENGINE_INC) -I$(KERNEL_DIR)/include \
                -I$(KERNEL_DIR)/portable/GCC/ARM_CM3 \
                -Wall -Wextra -Werror
ELF          := $(BUILD_DIR)/freertos-sample.elf
SMOKE_LOG    := $(BUILD_DIR)/smoke.log

.PHONY: all fetch run smoke clean

all: $(ELF)

fetch:
	@KERNEL_DIR=$(KERNEL_DIR) KERNEL_TAG=$(KERNEL_TAG) \
	  KERNEL_SHA256=$(KERNEL_SHA256) sh fetch_kernel.sh

$(KERNEL_DIR)/tasks.c:
	@KERNEL_DIR=$(KERNEL_DIR) KERNEL_TAG=$(KERNEL_TAG) \
	  KERNEL_SHA256=$(KERNEL_SHA256) sh fetch_kernel.sh

$(ELF): $(BOARD_SRC) $(KERNEL_FILES) $(ENGINE_LIB) linker.ld FreeRTOSConfig.h
	@mkdir -p $(BUILD_DIR)
	$(CC) $(CFLAGS) $(BOARD_SRC) $(KERNEL_FILES) $(ENGINE_LIB) -o $@ -lgcc
	@$(SIZE) $@

run: $(ELF)
	$(QEMU) -M $(BOARD) -cpu cortex-m3 -nographic \
	  -semihosting-config enable=on,target=native -kernel $(ELF)

smoke: $(ELF)
	@mkdir -p $(BUILD_DIR)
	@$(QEMU) -M $(BOARD) -cpu cortex-m3 -nographic \
	  -semihosting-config enable=on,target=native -kernel $(ELF) >$(SMOKE_LOG) 2>&1; \
	  rc=$$?; cat $(SMOKE_LOG); \
	  if [ $$rc -ne 0 ]; then echo "SMOKE FAIL: qemu exit=$$rc" >&2; exit $$rc; fi; \
	  grep -q "poly_engine version=" $(SMOKE_LOG) || \
	    { echo "SMOKE FAIL: missing version line" >&2; exit 1; }; \
	  echo "SMOKE PASS: exit=0, version line present"

clean:
	rm -rf $(BUILD_DIR) $(KERNEL_DIR)
