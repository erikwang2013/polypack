// 原生模块注册位（T-M2-04）：桥产物挂载点。
// T-M2-02 的 AAR（JNI .so + Kotlin 绑定）就位后在此接入加载与模块解析；
// v0 只登记与校验描述符。字段以 T-M2-02 报告为准，当前最小集：artifact / entryClass。
// 仓库当前无 Android SDK/模拟器，AAR 真加载为阻塞项（见 README「阻塞与挂载点」）。

/** 创建注册位（每次 createAdapter 一个实例）。 */
export function createNativeRegistry() {
  let descriptor = null;
  return Object.freeze({
    /** 登记桥产物描述符；重复登记即覆盖（宿主热重载场景）。 */
    register(desc) {
      const artifact = desc?.artifact;
      const entryClass = desc?.entryClass;
      if (typeof artifact !== "string" || artifact.trim() === "") {
        throw new TypeError("native: artifact 须为非空字符串（AAR 路径或坐标）");
      }
      if (typeof entryClass !== "string" || entryClass.trim() === "") {
        throw new TypeError("native: entryClass 须为非空字符串（Kotlin 入口类）");
      }
      descriptor = Object.freeze({ artifact, entryClass });
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
