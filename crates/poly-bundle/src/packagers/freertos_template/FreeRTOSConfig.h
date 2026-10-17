#ifndef FREERTOS_CONFIG_H
#define FREERTOS_CONFIG_H

/* PolyPack FreeRTOS sample board (T-M6-07): mps2-an385 / Cortex-M3 / QEMU. */

/* 断言失败走板级 hook：打印后以 semihosting 退出码 1 结束（不静默挂死）。 */
void poly_engine_rt_panic(void);
#define configASSERT(x) \
    do { \
        if (!(x)) { \
            poly_engine_rt_panic(); \
        } \
    } while (0)

#define configUSE_PREEMPTION 1
#define configUSE_TIME_SLICING 1
#define configUSE_PORT_OPTIMISED_TASK_SELECTION 0
#define configUSE_TICKLESS_IDLE 0
#define configCPU_CLOCK_HZ 25000000
#define configTICK_RATE_HZ 100
#define configMAX_PRIORITIES 5
#define configMINIMAL_STACK_SIZE 256
#define configMAX_TASK_NAME_LEN 16
#define configUSE_16_BIT_TICKS 0
#define configIDLE_SHOULD_YIELD 1
#define configUSE_TASK_NOTIFICATIONS 1
#define configUSE_MUTEXES 0
#define configUSE_RECURSIVE_MUTEXES 0
#define configUSE_COUNTING_SEMAPHORES 0
#define configUSE_QUEUE_SETS 0
#define configQUEUE_REGISTRY_SIZE 0
#define configUSE_TRACE_FACILITY 0
#define configUSE_STATS_FORMATTING_FUNCTIONS 0
#define configSUPPORT_DYNAMIC_ALLOCATION 1
#define configSUPPORT_STATIC_ALLOCATION 0
#define configTOTAL_HEAP_SIZE (64 * 1024)
#define configUSE_IDLE_HOOK 0
#define configUSE_TICK_HOOK 0
#define configUSE_MALLOC_FAILED_HOOK 0
#define configCHECK_FOR_STACK_OVERFLOW 0
#define configUSE_TIMERS 0
#define configUSE_NEWLIB_REENTRANT 0
#define INCLUDE_vTaskDelay 1
#define INCLUDE_vTaskDelete 1
#define INCLUDE_xTaskGetSchedulerState 1

/* MPS2 AN385 实现 3 位 NVIC 优先级。 */
#define configPRIO_BITS 3
#define configKERNEL_INTERRUPT_PRIORITY (7 << (8 - configPRIO_BITS))
#define configMAX_SYSCALL_INTERRUPT_PRIORITY (5 << (8 - configPRIO_BITS))

#endif /* FREERTOS_CONFIG_H */
