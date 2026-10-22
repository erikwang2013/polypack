# PolyPack Zephyr west 工程模板（T-M6-06）——最小应用工程。
cmake_minimum_required(VERSION 3.20.0)

# 引擎层以 Zephyr module 形式链入：模块由本工程随附，无需 ZEPHYR_EXTRA_MODULES 环境变量。
set(ZEPHYR_EXTRA_MODULES ${CMAKE_CURRENT_SOURCE_DIR}/modules/polypack_engine)
find_package(Zephyr REQUIRED HINTS $ENV{ZEPHYR_BASE})
project(polypack_zephyr_engine)

target_sources(app PRIVATE src/main.c)
