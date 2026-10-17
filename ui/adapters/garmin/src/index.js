// @polypack/adapter-garmin（T-M7-09 spike，实验档）：Monkey C 子集转译 + 结构自检。
// 复用 @polypack/adapter-wear 的紧凑 tokens / 安全区 / capabilities / 共享焦点内核；
// 本包不参与核心，也不新写导航系统。

export { GARMIN_FORMS, createGarminPage, describePage } from "./page.js";
export {
  appIdFor,
  DEFAULT_PRODUCT,
  transpileGarmin,
  ENTRY_CLASS,
  VIEW_CLASS,
  DELEGATE_CLASS,
  MIN_SDK_VERSION,
} from "./emit.js";
export {
  assertSubset,
  checkMonkeyC,
  sanitize,
  USING_ALLOWLIST,
  DC_METHODS,
  BASE_CLASSES,
} from "./subset.js";
export {
  assertGarminProject,
  assertIqEntries,
  checkGarminProject,
  checkIqEntries,
  isSafePath,
  parseXml,
  REQUIRED_FILES,
} from "./check.js";
