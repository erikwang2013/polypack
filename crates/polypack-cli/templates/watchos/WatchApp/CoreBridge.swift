// 核心 API 调用桩：手表端 runtime = none，Rust 核心不直接运行。
//
// 真实链路（TODO）：WatchConnectivity `sendMessage(["api": "core.status"])`
// → iOS 伴生 App 调用 PolyPack 核心 → `replyHandler(["status": ...])` 回传。
// 本桩只保证模板可构建 / 可预览；不含任何凭据，凭据只在 iOS 宿主侧以 CredentialRef 解析。

import Foundation

enum CoreBridge {
    /// 核心 API 桩：读取核心状态（真实实现见 WatchConnectivity.swift 的通道说明）。
    static func fetchStatus() async -> String {
        // TODO(T-M7-05): 接入 iOS 伴生 App 的真实核心调用。
        return "核心桩：经 iOS 伴生 App 代理"
    }
}
