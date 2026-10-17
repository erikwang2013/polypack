// T-M7-03 §8「Wear 表单」用例集：输入（input）→ 滑动（swipe）→ 返回（back）。
//
// 用例是**观测契约**而非实现：每步列出 adb 动作 + uiautomator 可观测的文本锚点；驱动按序
// 执行，失败即定位到 case/step（期望 vs 实际 + dump 路径）。被测 APK 是 RN Wear 宿主
// （仓库外预构建，同 TV 冒烟 `vars.POLYPACK_TV_HOST_APK` 纪律），宿主须实现「表单契约」：
//   - 标题 "Wear Form"；状态行 "step: amount" / "step: note"；金额回显 "amount: <值>"
//   - 金额输入框启动即聚焦（无需按坐标点按，圆 / 方屏通用）
//   - 内容区水平滑动（左滑）从 amount 页进入 note 页；金额保留
//   - note 页按系统 BACK 返回 amount 页；金额保留（焦点记忆）

/** 表单契约锚点（宿主实现与用例共用的唯一来源）。 */
export const FORM_ANCHORS = Object.freeze({
  title: "Wear Form",
  amountStep: "step: amount",
  noteStep: "step: note",
  amount: (value) => `amount: ${value}`,
});

const step = (id, action, expect) => Object.freeze({ id, action: Object.freeze(action), expect: Object.freeze(expect) });

/** 三个表单用例；launch 步由驱动执行 force-stop + monkey 冷启动。 */
export const WEAR_FORM_CASES = Object.freeze([
  Object.freeze({
    id: "input",
    purpose: "输入：键入金额，状态行回显",
    steps: Object.freeze([
      step("launch", { type: "launch" }, [FORM_ANCHORS.title, FORM_ANCHORS.amountStep]),
      step("type-amount", { type: "text", value: "12" }, [FORM_ANCHORS.amount(12)]),
    ]),
  }),
  Object.freeze({
    id: "swipe",
    purpose: "滑动：左滑从金额页进入备注页，金额保留",
    steps: Object.freeze([
      step("launch", { type: "launch" }, [FORM_ANCHORS.title, FORM_ANCHORS.amountStep]),
      step("type-amount", { type: "text", value: "12" }, [FORM_ANCHORS.amount(12)]),
      step("swipe-to-note", { type: "swipe", direction: "left" }, [FORM_ANCHORS.noteStep, FORM_ANCHORS.amount(12)]),
    ]),
  }),
  Object.freeze({
    id: "back",
    purpose: "返回：备注页按 BACK 回到金额页，金额保留",
    steps: Object.freeze([
      step("launch", { type: "launch" }, [FORM_ANCHORS.title, FORM_ANCHORS.amountStep]),
      step("type-amount", { type: "text", value: "12" }, [FORM_ANCHORS.amount(12)]),
      step("swipe-to-note", { type: "swipe", direction: "left" }, [FORM_ANCHORS.noteStep]),
      step("back-to-amount", { type: "back" }, [FORM_ANCHORS.amountStep, FORM_ANCHORS.amount(12)]),
    ]),
  }),
]);
