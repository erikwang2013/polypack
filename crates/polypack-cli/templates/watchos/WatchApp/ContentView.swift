// 手表主界面：演示「核心 API 调用桩 + 连接状态」的最小形态。

import SwiftUI

struct ContentView: View {
    @State private var status = "尚未查询"
    @State private var reachable = false

    var body: some View {
        VStack(spacing: 8) {
            Text("PolyPack Watch")
                .font(.headline)
            Text(status)
                .font(.footnote)
                .foregroundStyle(.secondary)
            Text(reachable ? "iOS 伴生 App 可达" : "iOS 伴生 App 不可达")
                .font(.caption2)
                .foregroundStyle(reachable ? .green : .orange)
            Button("查询核心状态") {
                Task {
                    status = await CoreBridge.fetchStatus()
                    reachable = Connectivity.shared.isReachable
                }
            }
        }
        .padding()
        .onAppear {
            Connectivity.shared.activate()
        }
    }
}

#Preview {
    ContentView()
}
