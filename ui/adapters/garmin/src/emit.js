// Monkey C 子集发射器（T-M7-09 spike）：Garmin 页模型 → Connect IQ 工程文件树。
//
// 产物（确定性文本，同输入二跑字节一致）：
//   manifest.xml · monkey.jungle · resources/strings/strings.xml ·
//   source/{PolypackApp,PolypackView,PolypackDelegate}.mc
//
// 设计约束（能力边界见 CAPABILITY-BOUNDARY-DRAFT.md）：
// - 语言子集：class / extends / var / static const / function / if / else / 数组 / 字典 /
//   while 以外一律不发射（子集断言见 subset.js）；
// - API 白名单：Application.AppBase、WatchUi.View / BehaviorDelegate / requestUpdate、
//   Graphics.Dc 7 个绘制方法 + 常量、System.println（见 subset.js 的同一份表）；
// - 布局：盒坐标来自 adapter-wear `layoutPage(tokens)`（构建期已知），运行时按
//   dc.getWidth()/getHeight() 线性缩放（参考画布 = wear 参考视口）；
// - 交互：焦点序 = adapter-wear 共享焦点内核的 engine.order（线性化，三键设备可达全部
//   四个按钮）；Back 记忆语义与内核同形（历史栈，空历史交宿主退出）。

import { createHash } from "node:crypto";

import { platformStyle } from "../../wear/src/index.js";

/** 示例产品元数据（与 T-M7-09 验收示例同口径；构建脚本可覆盖）。 */
export const DEFAULT_PRODUCT = Object.freeze({
  name: "Pax Notes",
  version: "1.2.3",
  identifier: "com.polypack.paxnotes",
});

const ENTRY_CLASS = "PolypackApp";
const VIEW_CLASS = "PolypackView";
const DELEGATE_CLASS = "PolypackDelegate";
const MIN_SDK_VERSION = "3.1.0";

const assertProduct = (product) => {
  if (!product || typeof product !== "object") {
    throw new TypeError("adapter-garmin: product 须为对象");
  }
  const { name, version, identifier } = product;
  if (typeof name !== "string" || name.trim() === "" || name.length > 64) {
    throw new TypeError("adapter-garmin: product.name 须为非空字符串（≤ 64 字符）");
  }
  if (/[\u0000-\u001f\u007f]/.test(name)) {
    throw new TypeError("adapter-garmin: product.name 不得含控制字符");
  }
  if (typeof version !== "string" || !/^\d+\.\d+\.\d+$/.test(version)) {
    throw new TypeError("adapter-garmin: product.version 须为 `主.次.补丁` 数字三段");
  }
  if (
    typeof identifier !== "string" ||
    !/^[a-z0-9-]+(\.[a-z0-9-]+)+$/.test(identifier)
  ) {
    throw new TypeError("adapter-garmin: product.identifier 须为小写反向域名");
  }
  return { name, version, identifier };
};

const xmlEscape = (text) =>
  text.replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll(">", "&gt;").replaceAll('"', "&quot;");

const mcString = (text) => JSON.stringify(text);

const colorLiteral = (hex) => {
  if (typeof hex !== "string" || !/^#[0-9a-fA-F]{6}$/.test(hex)) {
    throw new TypeError(`adapter-garmin: token 颜色须为 #rrggbb: ${hex}`);
  }
  return `0x${hex.slice(1).toUpperCase()}`;
};

/** Garmin 应用 id：identifier 的 sha256 前 32 hex（确定性；manifest 允许 32 hex 形态）。 */
export function appIdFor(identifier) {
  return createHash("sha256").update(identifier).digest("hex").slice(0, 32);
}

const center = (box, axis) => Math.round(box[axis] + box[axis === "x" ? "width" : "height"] / 2);

function manifestXml(page, product) {
  return `<?xml version="1.0" encoding="UTF-8"?>
<iq:manifest xmlns:iq="http://www.garmin.com/xml/connectiq" version="3">
  <iq:application id="${appIdFor(product.identifier)}" type="watch-app" entry="${ENTRY_CLASS}" name="@Strings.AppName" version="${product.version}" minSdkVersion="${MIN_SDK_VERSION}">
    <iq:products>
      <iq:product id="${page.product}"/>
    </iq:products>
  </iq:application>
</iq:manifest>
`;
}

const jungle = `# PolyPack 生成（T-M7-09 spike）——jungle 语法见官方 Jungle Reference。
project.manifest = manifest.xml
base.sourcePath = source
base.resourcePath = resources
`;

function stringsXml(product) {
  return `<?xml version="1.0" encoding="UTF-8"?>
<strings>
  <string id="AppName">${xmlEscape(product.name)}</string>
</strings>
`;
}

function appMc() {
  return `using Toybox.Application as App;

class ${ENTRY_CLASS} extends App.AppBase {
    function initialize() {
        AppBase.initialize();
    }

    function getInitialView() {
        var view = new ${VIEW_CLASS}();
        return [view, new ${DELEGATE_CLASS}(view)];
    }
}
`;
}

function viewMc(page, product) {
  const styles = platformStyle(page.tokens);
  const { boxes, reference } = page;
  const colors = {
    bg: colorLiteral(page.tokens.color.surface),
    card: colorLiteral(styles.card.backgroundColor),
    border: colorLiteral(styles.card.borderColor),
    brand: colorLiteral(styles.button.backgroundColor),
    label: colorLiteral(styles.buttonLabel.color),
    focus: colorLiteral(styles.focusRing.borderColor),
    text: colorLiteral(styles.title.color),
    muted: colorLiteral(styles.muted.color),
  };
  const orderLiteral = JSON.stringify(page.focusOrder);
  const indexLiteral = `{ ${page.focusOrder.map((id, i) => `"${id}" => ${i}`).join(", ")} }`;
  const button = (id, label) => {
    const box = boxes[id];
    return `        drawButton(dc, scaleX(${box.x}, width), scaleY(${box.y}, height), scaleX(${box.width}, width), scaleY(${box.height}, height), ${label}, mFocus.equals(${mcString(id)}));`;
  };
  const labels = {
    counter: `"计数：" + mClicks.toString()`,
    reset: mcString(BUTTON_LABELS.reset),
    detail: mcString(BUTTON_LABELS.detail),
    exit: mcString(BUTTON_LABELS.exit),
  };
  const versionLine = `${mcString(`版本 ${product.version}`)}`;

  return `using Toybox.WatchUi as Ui;
using Toybox.Graphics as Gfx;

// PolyPack 生成（T-M7-09 spike）：布局盒来自 @polypack/adapter-wear layoutPage(tokens)，
// 焦点序来自共享焦点内核 engine.order——本文件不含手写导航邻接。
class ${VIEW_CLASS} extends Ui.View {
    static const REF_WIDTH = ${reference.width};
    static const REF_HEIGHT = ${reference.height};
    static const COLOR_BG = ${colors.bg};
    static const COLOR_CARD = ${colors.card};
    static const COLOR_BORDER = ${colors.border};
    static const COLOR_BRAND = ${colors.brand};
    static const COLOR_LABEL = ${colors.label};
    static const COLOR_FOCUS = ${colors.focus};
    static const COLOR_TEXT = ${colors.text};
    static const COLOR_MUTED = ${colors.muted};
    static const FOCUS_COUNT = ${page.focusOrder.length};

    var mClicks = 0;
    var mDetail = false;
    var mExitRequested = false;
    var mFocus = ${mcString(page.initialFocus)};
    var mOrder = ${orderLiteral};
    var mIndex = ${indexLiteral};
    var mHistory = new[${page.focusOrder.length}];
    var mHistorySize = 0;

    function initialize() {
        View.initialize();
    }

    function scaleX(value, width) {
        return (value * width) / REF_WIDTH;
    }

    function scaleY(value, height) {
        return (value * height) / REF_HEIGHT;
    }

    function drawButton(dc, x, y, width, height, label, focused) {
        dc.setColor(COLOR_BRAND, COLOR_BRAND);
        dc.fillRectangle(x, y, width, height);
        if (focused) {
            dc.setColor(COLOR_FOCUS, COLOR_FOCUS);
            dc.setPenWidth(3);
            dc.drawRectangle(x, y, width, height);
            dc.setPenWidth(1);
        }
        dc.setColor(COLOR_LABEL, COLOR_LABEL);
        dc.drawText(x + (width / 2), y + (height / 2), Gfx.FONT_XTINY, label, Gfx.TEXT_JUSTIFY_CENTER | Gfx.TEXT_JUSTIFY_VCENTER);
    }

    function onUpdate(dc) {
        var width = dc.getWidth();
        var height = dc.getHeight();
        dc.setColor(COLOR_BG, COLOR_BG);
        dc.clear();
        dc.setColor(COLOR_CARD, COLOR_CARD);
        dc.fillRectangle(scaleX(${boxes.card.x}, width), scaleY(${boxes.card.y}, height), scaleX(${boxes.card.width}, width), scaleY(${boxes.card.height}, height));
        dc.setColor(COLOR_BORDER, COLOR_BORDER);
        dc.setPenWidth(1);
        dc.drawRectangle(scaleX(${boxes.card.x}, width), scaleY(${boxes.card.y}, height), scaleX(${boxes.card.width}, width), scaleY(${boxes.card.height}, height));
        dc.setColor(COLOR_TEXT, COLOR_TEXT);
        dc.drawText(scaleX(${center(boxes.title, "x")}, width), scaleY(${center(boxes.title, "y")}, height), Gfx.FONT_SMALL, ${mcString(product.name)}, Gfx.TEXT_JUSTIFY_CENTER | Gfx.TEXT_JUSTIFY_VCENTER);
        dc.setColor(COLOR_MUTED, COLOR_MUTED);
        dc.drawText(scaleX(${center(boxes.version, "x")}, width), scaleY(${center(boxes.version, "y")}, height), Gfx.FONT_XTINY, ${versionLine}, Gfx.TEXT_JUSTIFY_CENTER | Gfx.TEXT_JUSTIFY_VCENTER);
${["counter", "reset", "detail", "exit"].map((id) => button(id, labels[id])).join("\n")}
        var hintText = "上 / 下键移动，确定键激活";
        if (mExitRequested) {
            hintText = "已请求退出（宿主接管）";
        } else if (mDetail) {
            hintText = "详情：Wear 演示页";
        }
        dc.setColor(COLOR_MUTED, COLOR_MUTED);
        dc.drawText(scaleX(${center(boxes.hint, "x")}, width), scaleY(${center(boxes.hint, "y")}, height), Gfx.FONT_XTINY, hintText, Gfx.TEXT_JUSTIFY_CENTER | Gfx.TEXT_JUSTIFY_VCENTER);
    }

    function remember() {
        if (mHistorySize < FOCUS_COUNT) {
            mHistory[mHistorySize] = mFocus;
            mHistorySize += 1;
        }
    }

    function move(delta) {
        var index = mIndex[mFocus];
        var next = (index + delta + FOCUS_COUNT) % FOCUS_COUNT;
        var target = mOrder[next];
        if (!target.equals(mFocus)) {
            remember();
            mFocus = target;
        }
    }

    function activate(id) {
        if (id.equals("counter")) {
            mClicks += 1;
        } else if (id.equals("reset")) {
            mClicks = 0;
        } else if (id.equals("detail")) {
            mDetail = !mDetail;
        } else if (id.equals("exit")) {
            mExitRequested = true;
        }
    }

    function back() {
        if (mHistorySize > 0) {
            mHistorySize -= 1;
            mFocus = mHistory[mHistorySize];
            return true;
        }
        return false;
    }
}
`;
}

function delegateMc() {
  return `using Toybox.WatchUi as Ui;

class ${DELEGATE_CLASS} extends Ui.BehaviorDelegate {
    var mView;

    function initialize(view) {
        BehaviorDelegate.initialize();
        mView = view;
    }

    function onSelect() {
        mView.activate(mView.mFocus);
        Ui.requestUpdate();
        return true;
    }

    function onNextPage() {
        mView.move(1);
        Ui.requestUpdate();
        return true;
    }

    function onPreviousPage() {
        mView.move(-1);
        Ui.requestUpdate();
        return true;
    }

    function onBack() {
        if (mView.back()) {
            Ui.requestUpdate();
            return true;
        }
        return false;
    }
}
`;
}

const BUTTON_LABELS = Object.freeze({ reset: "重置", detail: "详情", exit: "退出" });

/**
 * 页模型 → Connect IQ 工程文件树。
 *
 * @param {ReturnType<import("./page.js").createGarminPage>} page
 * @param {{name: string, version: string, identifier: string}} [product]
 * @returns {{ files: Record<string, string>, meta: object }}
 */
export function transpileGarmin(page, product = DEFAULT_PRODUCT) {
  const info = assertProduct(product);
  const files = {
    "manifest.xml": manifestXml(page, info),
    "monkey.jungle": jungle,
    "resources/strings/strings.xml": stringsXml(info),
    "source/PolypackApp.mc": appMc(),
    "source/PolypackView.mc": viewMc(page, info),
    "source/PolypackDelegate.mc": delegateMc(),
  };
  return {
    files,
    meta: Object.freeze({
      target: "garmin",
      form: page.form,
      product: page.product,
      entry: ENTRY_CLASS,
      appId: appIdFor(info.identifier),
      minSdkVersion: MIN_SDK_VERSION,
      version: info.version,
    }),
  };
}

export { ENTRY_CLASS, VIEW_CLASS, DELEGATE_CLASS, MIN_SDK_VERSION };
