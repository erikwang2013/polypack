// T-M7-03 mock 腿（零 adb / 零安装）：用例集完备性、adb 动作映射（含圆屏坐标派生）、
// 期望匹配与失败定位格式。真跑腿见 drivers/android.mjs（设备 / APK 缺席 SKIP-with-print）。

import assert from "node:assert/strict";
import { test } from "node:test";

import {
  buildActionCommands,
  extractTexts,
  localizeFailure,
  missingExpectations,
  parseWmSize,
} from "../drivers/android.mjs";
import { FORM_ANCHORS, WEAR_FORM_CASES } from "../forms.mjs";

test("用例集：恰好 input / swipe / back，步骤 id 唯一且每步都有可观测期望", () => {
  assert.deepEqual(
    WEAR_FORM_CASES.map((wearCase) => wearCase.id),
    ["input", "swipe", "back"],
  );
  for (const wearCase of WEAR_FORM_CASES) {
    assert.ok(wearCase.purpose.length > 0, `${wearCase.id} 缺用途说明`);
    assert.ok(wearCase.steps.length >= 2, `${wearCase.id} 步骤过少`);
    const stepIds = wearCase.steps.map((step) => step.id);
    assert.equal(new Set(stepIds).size, stepIds.length, `${wearCase.id} 步骤 id 重复`);
    for (const step of wearCase.steps) {
      assert.ok(step.expect.length > 0, `${wearCase.id}/${step.id} 缺期望锚点`);
    }
  }
  // 三个用例都必须从「冷启动 + 金额页」开始（case 间隔离由 launch 的 force-stop 保证）。
  for (const wearCase of WEAR_FORM_CASES) {
    assert.equal(wearCase.steps[0].action.type, "launch");
    assert.ok(wearCase.steps[0].expect.includes(FORM_ANCHORS.amountStep));
  }
});

test("adb 动作映射：输入 / 左滑（按屏幕比例） / BACK 键", () => {
  assert.deepEqual(buildActionCommands({ type: "launch" }, { width: 320, height: 320 }), []);
  assert.deepEqual(buildActionCommands({ type: "text", value: "12" }, { width: 320, height: 320 }), [
    ["shell", "input", "text", "12"],
  ]);
  assert.deepEqual(buildActionCommands({ type: "back" }, { width: 320, height: 320 }), [
    ["shell", "input", "keyevent", "4"],
  ]);
  const [swipe] = buildActionCommands({ type: "swipe", direction: "left" }, { width: 320, height: 320 });
  assert.equal(swipe[0], "shell");
  assert.deepEqual(swipe.slice(1, 3), ["input", "swipe"]);
  assert.equal(swipe[3], "240");
  // 左滑：起点 x > 终点 x；y 取屏幕中线；坐标保持在屏内。
  const [, , , x1, y1, x2, y2, duration] = swipe;
  assert.ok(Number(x1) > Number(x2), `左滑方向反了：${x1} → ${x2}`);
  assert.equal(y1, "160");
  assert.equal(y2, "160");
  assert.equal(duration, "200");
  assert.throws(() => buildActionCommands({ type: "shake" }, { width: 1, height: 1 }), /未知表单动作/);
});

test("wm size 解析：物理尺寸行；缺省 / 异常回退 null", () => {
  assert.deepEqual(parseWmSize("Physical size: 320x320\n"), { width: 320, height: 320 });
  assert.deepEqual(parseWmSize("Physical size: 454x454\nOverride size: 320x320"), {
    width: 454,
    height: 454,
  });
  assert.equal(parseWmSize(""), null);
  assert.equal(parseWmSize(undefined), null);
});

test("观测匹配：text / content-desc 都算，缺锚点逐一列出", () => {
  const xml =
    '<node text="Wear Form" content-desc="amount-field" /><node text="step: amount" /><node text="amount: 12" />';
  const texts = extractTexts(xml);
  assert.deepEqual(texts, ["Wear Form", "amount-field", "step: amount", "amount: 12"]);
  assert.deepEqual(missingExpectations(texts, ["Wear Form", "amount: 12"]), []);
  assert.deepEqual(missingExpectations(texts, ["step: note"]), ["step: note"]);
  // 子串匹配是契约（宿主可在同一节点拼接文案），不做整串全等。
  assert.deepEqual(missingExpectations(["step: amount · amount: 12"], ["step: amount"]), []);
});

test("失败定位：case/step + 期望 + 实际 + dump 指针", () => {
  const text = localizeFailure({
    case: "swipe",
    step: "swipe-to-note",
    expect: ["step: note", "amount: 12"],
    observed: ["step: amount"],
    dump: "/tmp/swipe-swipe-to-note.xml",
  });
  assert.match(text, /^\[swipe\/swipe-to-note\]/);
  assert.match(text, /step: note/);
  assert.match(text, /step: amount/);
  assert.match(text, /dump: \/tmp\/swipe-swipe-to-note\.xml/);
  assert.match(
    localizeFailure({ case: "back", step: "back-to-amount", expect: ["step: amount"], observed: [], dump: null }),
    /实际 （空）/,
  );
});
