//! T-M5-10 验收：ChromeOS 双轨（Crostini deb + PWA）mock 契约（每提交，任意宿主）。
//!
//! ①两轨在 `polypack-targets` 内建注册表中的声明（L5 / 宿主 / 格式 / 打包器 / 校验器）与
//! T-M5-10 口径一致；②复用证明：`polypack_bundle::find("chromeos", "deb")` 与
//! `find("chromeos-pwa", "static-site")` 解析到的打包器 id 分别是既有的 `linux` / `web`
//! ——双轨没有第二实现；③verify 腿真跑：临时工程 + 合法站点 → `pwa-audit` 通过并给
//! `Verified`；拆掉 service worker 注册 → `polypack::E_VERIFY_0003` 硬失败（非空转证明）。
//!
//! 真实链路（同一 JobSpec 双轨出包 / deb 安装 / Chrome 三腿）见
//! `contract_m5_chromeos_real.rs`（`POLYPACK_CONTRACT_REAL=1`）。

mod common;

use common::{combined, init_project, polypack, write_site};
use polypack_targets::{Host, Registry};

/// 注册表声明：两轨均为 L5、同 Linux 宿主可达（同一 JobSpec 的前提），verify 各自挂对应腿。
#[test]
fn dual_track_declarations_match_roadmap() {
    let registry = Registry::builtin();
    let chromeos = registry
        .get("chromeos")
        .cloned()
        .expect("内建注册表缺少 chromeos");
    let pwa = registry
        .get("chromeos-pwa")
        .cloned()
        .expect("内建注册表缺少 chromeos-pwa");

    for spec in [&chromeos, &pwa] {
        assert_eq!(spec.tier, 5, "{} 应为 L5（ROADMAP §3.6）", spec.id);
        assert!(
            spec.hosts.contains(&Host::Linux),
            "{} 应在 Linux 宿主可达（ChromeOS 双轨同机出包）",
            spec.id
        );
        assert!(!spec.experimental, "{} 不应标 experimental", spec.id);
    }

    assert_eq!(chromeos.package.format, "deb");
    assert_eq!(chromeos.package.builder, "linux-deb");
    assert_eq!(chromeos.verify, vec!["lintian".to_string()]);
    let notes = chromeos.notes.clone().unwrap_or_default();
    assert!(
        notes.contains("Crostini") && notes.contains("复用") && notes.contains("SKIP-with-print"),
        "chromeos notes 应写明复用 deb 链与安装冒烟跳过口径：{notes}"
    );

    assert_eq!(pwa.package.format, "static-site");
    assert_eq!(pwa.package.builder, "web-static");
    assert_eq!(pwa.verify, vec!["pwa-audit".to_string()]);
    let notes = pwa.notes.clone().unwrap_or_default();
    assert!(
        notes.contains("service worker") && notes.contains("SKIP-with-print"),
        "chromeos-pwa notes 应写明 SW 由应用自带与真浏览器腿跳过口径：{notes}"
    );
}

/// 两轨复用成熟打包器：解析到的 id 恒为既有 `linux`（deb）/ `web`（static-site），无第二实现。
#[test]
fn both_tracks_reuse_matured_packagers() {
    let deb = polypack_bundle::find("chromeos", "deb").expect("chromeos 应有 deb 打包器");
    assert_eq!(deb.id(), "linux", "chromeos deb 必须复用 Linux deb 打包器");
    assert_eq!(deb.format(), "deb");
    assert_eq!(
        polypack_bundle::find("linux", "deb").map(|packager| packager.id()),
        Some(deb.id()),
        "与 linux 目标解析到同一实现"
    );

    let site =
        polypack_bundle::find("chromeos-pwa", "static-site").expect("chromeos-pwa 应有静态站打包器");
    assert_eq!(site.id(), "web", "chromeos-pwa 必须复用 Web 静态站打包器");
    assert_eq!(site.format(), "static-site");
    assert_eq!(
        polypack_bundle::find("web", "static-site").map(|packager| packager.id()),
        Some(site.id()),
        "与 web 目标解析到同一实现"
    );
}

/// verify 腿真跑：合法站点过三腿；拆掉 SW 注册后 E_VERIFY_0003 硬失败（证明不是空转）。
#[test]
fn pwa_audit_verify_leg_passes_valid_site_and_fails_missing_sw() {
    let dir = init_project("verify");
    write_site(&dir, true);

    let ok = polypack(&["verify", "--target", "chromeos-pwa", "--json"], &dir);
    let log = combined(&ok);
    assert!(ok.status.success(), "合法站点应通过 pwa-audit：{log}");
    for marker in [
        "pwa-audit",
        "PWA manifest 审计",
        "PWA 离线能力审计",
        "PWA 安装性审计",
    ] {
        assert!(log.contains(marker), "verify 输出缺 {marker}：{log}");
    }
    assert!(
        log.contains("manifest.webmanifest"),
        "审计证据应点名 manifest：{log}"
    );

    // 同目录拆掉 SW 注册：离线能力腿缺陷必须硬失败并给定位字段。
    write_site(&dir, false);
    let bad = polypack(&["verify", "--target", "chromeos-pwa", "--json"], &dir);
    let log = combined(&bad);
    assert!(!bad.status.success(), "缺 SW 时 verify 必须失败：{log}");
    assert!(log.contains("E_VERIFY_0003"), "应报 E_VERIFY_0003：{log}");
    assert!(
        log.contains("service-worker"),
        "诊断应点名 service-worker：{log}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
