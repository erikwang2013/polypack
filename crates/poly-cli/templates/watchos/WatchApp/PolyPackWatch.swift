// PolyPack watchOS 伴生模板（实验，T-M7-05）。
// 手表端不直接运行 Rust 核心：核心调用经 iOS 伴生 App 代理（见 CoreBridge / WatchConnectivity）。

import SwiftUI

@main
struct PolyPackWatchApp: App {
    var body: some Scene {
        WindowGroup {
            ContentView()
        }
    }
}
