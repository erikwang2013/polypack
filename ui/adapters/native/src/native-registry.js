// 原生模块注册位（T-M2-04 / T-M3-02）：桥产物挂载点，字段口径以真跑产物为准，按宿主分族：
// Android AAR（T-M2-02）：
//   artifact    = AAR 路径 <工程根>/bridge/android/<核心库名>.aar（`polypack bridge aar` 产出；
//                 不是 maven 坐标、不放 libs/）
//   entryClass  = Kotlin 门面类 uniffi.<UDL namespace>.<Ns>Kt（namespace pax → uniffi.pax.PaxKt）
//   libraryName = 可选，.so 基名 = System.loadLibrary 入参（= 核心 crate 名下划线形态）；缺省 null
// 鸿蒙 NAPI（T-M3-01/T-M3-02，C ABI）：
//   artifact    = .so 路径 <工程根>/bridge/harmonyos/<abi>/lib<ns>.so（`polypack bridge ohos` 产出，自包含）
//   moduleName  = NAPI 模块名 = UDL namespace（ArkTS `import … from 'lib<ns>.so'`，示例 pax）
//   （不含 entryClass / libraryName——那是 Android AAR 口径，混用即抛错防跨宿主拷贝漂移）
// v0 只登记与校验（可注入描述符），RN 侧包装是宿主的事；真加载为阻塞项（无 Android/鸿蒙设备）。

/** 创建注册位（每次 createAdapter 一个实例；target 决定描述符口径）。 */
export function createNativeRegistry(target = "android") {
  const harmony = target === "harmonyos";
  let descriptor = null;
  return Object.freeze({
    /** 登记桥产物描述符；重复登记即覆盖（宿主热重载场景）。 */
    register(desc) {
      const artifact = desc?.artifact;
      if (typeof artifact !== "string" || artifact.trim() === "") {
        throw new TypeError(
          harmony
            ? "native: artifact 须为非空字符串（.so 路径 bridge/harmonyos/<abi>/lib<ns>.so）"
            : "native: artifact 须为非空字符串（AAR 路径 <工程根>/bridge/android/<核心库名>.aar）",
        );
      }
      if (harmony) {
        const moduleName = desc?.moduleName;
        if (typeof moduleName !== "string" || moduleName.trim() === "") {
          throw new TypeError("native: moduleName 须为非空字符串（NAPI 模块名 = UDL namespace）");
        }
        if (desc.entryClass !== undefined || desc.libraryName !== undefined) {
          throw new TypeError(
            "native: 鸿蒙 NAPI 描述符不含 entryClass / libraryName（Android AAR 口径，勿跨宿主混用）",
          );
        }
        descriptor = Object.freeze({ artifact, moduleName });
        return descriptor;
      }
      const entryClass = desc?.entryClass;
      const libraryName = desc?.libraryName ?? null;
      if (typeof entryClass !== "string" || entryClass.trim() === "") {
        throw new TypeError("native: entryClass 须为非空字符串（Kotlin 门面类 uniffi.<ns>.<Ns>Kt）");
      }
      if (libraryName !== null && (typeof libraryName !== "string" || libraryName.trim() === "")) {
        throw new TypeError("native: libraryName 须为非空字符串或省略（.so 基名）");
      }
      descriptor = Object.freeze({ artifact, entryClass, libraryName });
      return descriptor;
    },
    /** 当前描述符；未登记为 null。 */
    get() {
      return descriptor;
    },
    /** 是否已登记。 */
    has() {
      return descriptor !== null;
    },
  });
}
