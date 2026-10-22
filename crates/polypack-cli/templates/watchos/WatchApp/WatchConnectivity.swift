// WatchConnectivity 说明 / 骨架：手表 ⇄ iOS 伴生 App。
//
// 三条通道按需选择：
// - sendMessage(_:replyHandler:errorHandler:)：对端可达时即时往返，核心 API 调用首选；
// - transferUserInfo(_:)：后台排队补投，对端不可达时的可靠请求；
// - updateApplicationContext(_:)：只保留最新状态快照，用于状态同步。
//
// 配对宿主：包含本 watch target 的 iOS 工程（WKCompanionAppBundleIdentifier）。
// 传输内容不含任何明文凭据——凭据只在 iOS 宿主侧以 CredentialRef 解析。

import Foundation
import WatchConnectivity

final class Connectivity: NSObject, WCSessionDelegate {
    static let shared = Connectivity()

    private override init() {
        super.init()
    }

    func activate() {
        guard WCSession.isSupported() else { return }
        WCSession.default.delegate = self
        WCSession.default.activate()
    }

    var isReachable: Bool {
        WCSession.isSupported() && WCSession.default.isReachable
    }

    /// 请求核心状态：可达走 sendMessage，不可达转后台补投。
    func requestCoreStatus(reply: @escaping (String) -> Void) {
        let message = ["api": "core.status"]
        guard WCSession.default.isReachable else {
            WCSession.default.transferUserInfo(message)
            reply("iOS 伴生 App 不可达：请求已转入后台队列")
            return
        }
        WCSession.default.sendMessage(message) { response in
            reply(response["status"] as? String ?? "核心未返回状态")
        } errorHandler: { error in
            reply("核心调用失败：\(error.localizedDescription)")
        }
    }

    // MARK: - WCSessionDelegate（watchOS 必选回调）

    func session(
        _ session: WCSession,
        activationDidCompleteWith activationState: WCSessionActivationState,
        error: Error?
    ) {
        // 激活结果由 isReachable / 调用时错误处理反映；模板不额外记录状态。
    }
}
