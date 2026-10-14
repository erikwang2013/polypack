# Security Policy / 安全策略

## 报告漏洞 / Reporting a Vulnerability

请通过 **GitHub Security Advisories**（仓库 Security 标签页 → Report a vulnerability）私密报告。请勿在公开 Issue 中披露未修复的漏洞。
Please report privately via **GitHub Security Advisories** (repo Security tab → Report a vulnerability). Do not disclose unfixed issues in public.

报告时请尽量包含：影响范围、复现步骤、受影响的文件/版本、建议修复（如有）。

## 支持范围 / Scope

本项目当前处于规划阶段（文档与图示流水线）；以下资产在保护范围内：

- `tools/` 下的生成与校验脚本（含 CI workflow）——供应链与生成产物完整性；
- `docs/assets/*.svg` 与文档——内容注入 / 结构完整性；
- 未来代码阶段的以下面（承诺）：**凭据与签名密钥的处理路径、secret-scan 绕过、WASM 插件沙箱逃逸、构建产物篡改与可复现性缺陷**（见 [ROADMAP §7](docs/ROADMAP.md) 与风险登记册 R5）。

Currently in the planning phase (docs + diagram pipelines). In scope: the generator/checker scripts and CI workflow, the committed SVG/doc assets, and — as a forward commitment — credential/signing-key handling, secret-scan bypasses, WASM plugin sandbox escapes, and artifact tampering/reproducibility defects once code lands.

## 支持版本 / Supported Versions

安全修复仅应用于最新的 `vX.Y.Z` 发布（当前 `main` 与最新 tag）。
Security fixes apply to the latest `vX.Y.Z` release only (current `main` and the latest tag).

## 响应预期 / Expectations

- 初次响应：3 个工作日内确认收到；
- 修复或缓解方案：在确认后尽力于 30 天内提供（复杂度高的将同步进展）；
- 修复发布后经你同意在 Advisory 中致谢。
