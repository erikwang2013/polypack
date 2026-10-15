// 原生模块注册位（T-M2-04）：桥产物挂载点，字段口径以 T-M2-02 真跑产物为准。
//   artifact    = AAR 路径 <工程根>/bridge/android/<核心库名>.aar（`poly bridge aar` 产出；
//                 不是 maven 坐标、不放 libs/）
//   entryClass  = Kotlin 门面类 uniffi.<UDL namespace>.<Ns>Kt（namespace pax → uniffi.pax.PaxKt）
//   libraryName = 可选，.so 基名 = System.loadLibrary 入参（= 核心 crate 名下划线形态）；缺省 null
// v0 只登记与校验（可注入描述符），RN 侧包装是宿主的事；真加载为阻塞项（无 Android SDK/模拟器）。

/** 创建注册位（每次 createAdapter 一个实例）。 */
export function createNativeRegistry() {
  let descriptor = null;
  return Object.freeze({
    /** 登记桥产物描述符；重复登记即覆盖（宿主热重载场景）。 */
    register(desc) {
      const artifact = desc?.artifact;
      const entryClass = desc?.entryClass;
      const libraryName = desc?.libraryName ?? null;
      if (typeof artifact !== "string" || artifact.trim() === "") {
        throw new TypeError(
          "native: artifact 须为非空字符串（AAR 路径 <工程根>/bridge/android/<核心库名>.aar）",
        );
      }
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
