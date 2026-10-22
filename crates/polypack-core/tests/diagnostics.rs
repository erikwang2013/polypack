//! T-M0-07 验收：诊断框架（错误码 / 退出码 / 修复提示）与事件渲染。

use polypack_core::{
    BuildEvent, Code, Diagnostic, EventSink, HumanSink, JsonSink, Severity, codes,
};

/// 全部已登记错误码：新增码时同步登记，使格式与唯一性校验保持全覆盖。
const ALL_CODES: [Code; 16] = [
    codes::E_CFG_0001,
    codes::E_CFG_0002,
    codes::E_TGT_0001,
    codes::E_TGT_0002,
    codes::E_TGT_0003,
    codes::E_PROBE_0001,
    codes::E_PLAN_0001,
    codes::E_PLAN_0002,
    codes::E_PLAN_0003,
    codes::E_COMPILE_0001,
    codes::E_WEB_0001,
    codes::E_WEB_0002,
    codes::E_CACHE_0001,
    codes::E_INIT_0001,
    codes::E_DEV_0001,
    codes::E_IO_0001,
];

/// 取走 sink 的输出并按 UTF-8 解码。
fn text_of(bytes: Vec<u8>) -> String {
    String::from_utf8(bytes).expect("事件输出为 UTF-8")
}

#[test]
fn code_display_follows_stable_format() {
    for code in ALL_CODES {
        let text = code.to_string();
        let rest = text
            .strip_prefix("polypack::E_")
            .unwrap_or_else(|| panic!("缺 polypack::E_ 前缀：{text}"));
        let (module, seq) = rest
            .split_once('_')
            .unwrap_or_else(|| panic!("缺模块 / 序号分隔符：{text}"));
        assert!(
            !module.is_empty() && module.chars().all(|c| c.is_ascii_uppercase()),
            "模块名应为大写字母：{text}"
        );
        assert_eq!(seq.len(), 4, "序号应为四位：{text}");
        assert!(
            seq.chars().all(|c| c.is_ascii_digit()),
            "序号应为数字：{text}"
        );
    }
}

#[test]
fn codes_are_unique() {
    let mut texts: Vec<String> = ALL_CODES.iter().map(ToString::to_string).collect();
    texts.sort();
    texts.dedup();
    assert_eq!(texts.len(), ALL_CODES.len(), "错误码字符串不得重复");
}

#[test]
fn exit_code_maps_error_to_one_others_to_zero() {
    assert_eq!(
        Diagnostic::error(codes::E_IO_0001, "写文件失败").exit_code(),
        1
    );
    assert_eq!(
        Diagnostic::warn(codes::E_TGT_0002, "宿主不可达").exit_code(),
        0
    );
    let info = Diagnostic {
        code: codes::E_IO_0001,
        severity: Severity::Info,
        message: "仅供参考".to_string(),
        hint: None,
    };
    assert_eq!(info.exit_code(), 0);
}

#[test]
fn with_hint_chains_and_renders() {
    let diag = Diagnostic::error(codes::E_PROBE_0001, "缺少 android-sdk");
    assert!(diag.hint.is_none(), "构造时无提示");
    let diag = diag.with_hint("运行 sdkmanager \"platform-tools\"");
    assert_eq!(
        diag.hint.as_deref(),
        Some("运行 sdkmanager \"platform-tools\"")
    );
    let text = diag.to_string();
    assert!(
        text.contains("[polypack::E_PROBE_0001]"),
        "含错误码：{text}"
    );
    assert!(text.contains("缺少 android-sdk"), "含描述：{text}");
    assert!(text.contains("运行 sdkmanager"), "含提示：{text}");
}

#[test]
fn human_sink_renders_failed_with_code_and_hint() {
    let mut sink = HumanSink { out: Vec::new() };
    sink.emit(&BuildEvent::Failed {
        stage: "compile".to_string(),
        code: codes::E_PROBE_0001.to_string(),
        hint: None,
    });
    sink.emit(&BuildEvent::Failed {
        stage: "sign".to_string(),
        code: "polypack::E_TGT_0002".to_string(),
        hint: Some("配置凭据档案".to_string()),
    });
    let text = text_of(sink.out);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 2, "每个事件一行：{text}");
    assert!(
        lines[0].contains('✗')
            && lines[0].contains("compile")
            && lines[0].contains("polypack::E_PROBE_0001"),
        "失败行含标记 / 阶段 / 错误码：{}",
        lines[0]
    );
    assert!(
        !lines[0].contains('（'),
        "无提示时不渲染提示括号：{}",
        lines[0]
    );
    assert!(
        lines[1].contains("sign")
            && lines[1].contains("polypack::E_TGT_0002")
            && lines[1].contains("配置凭据档案"),
        "含阶段 / 错误码 / 提示：{}",
        lines[1]
    );
}

#[test]
fn json_sink_emits_json_lines_with_event_tag() {
    let mut sink = JsonSink { out: Vec::new() };
    sink.emit(&BuildEvent::Planned {
        targets: vec!["linux".to_string()],
        excluded: vec![],
    });
    sink.emit(&BuildEvent::Probed {
        found: vec!["rust".to_string()],
        missing: vec!["emsdk".to_string()],
    });
    sink.emit(&BuildEvent::Failed {
        stage: "compile".to_string(),
        code: "polypack::E_PROBE_0001".to_string(),
        hint: Some("安装 NDK".to_string()),
    });
    sink.emit(&BuildEvent::Failed {
        stage: "bundle".to_string(),
        code: "polypack::E_IO_0001".to_string(),
        hint: None,
    });
    let text = text_of(sink.out);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 4, "每事件一行 JSON：{text}");
    let values: Vec<serde_json::Value> = lines
        .iter()
        .map(|line| {
            serde_json::from_str(line).unwrap_or_else(|e| panic!("行不是合法 JSON：{line}（{e}）"))
        })
        .collect();
    let tags: Vec<&str> = values
        .iter()
        .map(|v| v["event"].as_str().expect("event tag 为字符串"))
        .collect();
    assert_eq!(tags, ["planned", "probed", "failed", "failed"]);
    assert_eq!(values[0]["targets"][0], "linux");
    assert_eq!(values[1]["missing"][0], "emsdk");
    assert_eq!(values[2]["code"], "polypack::E_PROBE_0001");
    assert_eq!(values[2]["hint"], "安装 NDK");
    assert!(values[3].get("hint").is_none(), "无提示时省略 hint 字段");
}

#[test]
fn diagnostic_serializes_with_stable_field_shape() {
    let value = serde_json::to_value(
        Diagnostic::error(codes::E_CFG_0001, "schema 版本不受支持").with_hint("升级 polypack.toml"),
    )
    .expect("Diagnostic 可序列化");
    assert_eq!(value["code"], "polypack::E_CFG_0001");
    assert_eq!(value["severity"], "error");
    assert_eq!(value["message"], "schema 版本不受支持");
    assert_eq!(value["hint"], "升级 polypack.toml");
    let plain = serde_json::to_value(Diagnostic::warn(codes::E_TGT_0002, "宿主不可达"))
        .expect("Diagnostic 可序列化");
    assert!(plain.get("hint").is_none(), "无提示时省略 hint 字段");
}
