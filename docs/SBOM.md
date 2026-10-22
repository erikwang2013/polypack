# SBOM 与构建溯源（M9-E1）

> 状态：T-M9-01/02/03 已交付；样例在 `docs/samples/sbom/web/`。英文版：[SBOM.en.md](SBOM.en.md)。

## 1. 是什么 / 不是什么

| 是 | 不是 |
| --- | --- |
| 每个产物一份 SPDX 2.3、一份 CycloneDX 1.5 与一份 in-toto Statement provenance | 不是 SLSA 认证，不声明任何 SLSA 等级 |
| 依赖图来自 `cargo metadata --locked --offline`，与 `Cargo.lock` 对齐 | 不联网、不接 Sigstore / Rekor、不做远程证明 |
| 确定性输出：同一输入二跑字节一致 | 不替代制品签名；签名 / 校验和仍由 `polypack sign` / `polypack verify` 负责 |

## 2. 生成

先构建，再让 `verify` 为暂存产物生成 SBOM：

```bash
polypack build --target web
polypack verify --sbom --target web
# JSON 事件流（自动化用）
polypack verify --sbom --target web --json
```

`--sbom` 可以与其他校验器同时使用；没有暂存产物的目标会明确告警并跳过，不会生成空 SBOM。生成失败返回 `polypack::E_VERIFY_0003`，不会静默降级。

## 3. 输出布局

```text
.polypack/sbom/<target>/
  <artifact>.spdx.json
  <artifact>.cdx.json
  <artifact>.intoto.jsonl
```

例如 `site` 产物会得到 `site.spdx.json`、`site.cdx.json`、`site.intoto.jsonl`。文件随产物名排序，目录内容只属于该目标。

## 4. 确定性与隐私

- 时间戳固定为 `1970-01-01T00:00:00Z`，不读系统时钟或 `SOURCE_DATE_EPOCH`。
- 包、JSON 键与产物摘要均排序；同一工程、同一产物二跑字节一致。
- 源树摘要排除 `target`、`.polypack`、`.git`、`node_modules`，不把构建缓存写进 provenance。
- provenance 只包含构建所需元数据（工具链、目标声明、材料摘要、产物摘要），不执行或上传任何内容。

## 5. 发布回执

`polypack release` 用与 SBOM 相同的产物集合摘要算法计算 `sbom_digest` 并写入回执；回执可选字段，旧回执没有该字段时按缺省处理。这样发布记录和 SBOM 引用同一组产物摘要。

## 6. 样例与测试

仓库内样例（用最小 web 静态站夹具生成，二跑字节一致）：

```bash
find docs/samples/sbom/web -type f
jq -r .spdxVersion docs/samples/sbom/web/site.spdx.json   # SPDX-2.3
jq -r .specVersion docs/samples/sbom/web/site.cdx.json    # 1.5
jq -r '._type' docs/samples/sbom/web/site.intoto.jsonl     # https://in-toto.io/Statement/v1
```

验收测试：

```bash
cargo test -p polypack-cli --test sbom
node tools/check-docs.mjs
```

## 7. 诊断

| 错误码 | 含义 |
| --- | --- |
| `polypack::E_VERIFY_0003` | SBOM 生成失败（缺 `Cargo.lock` / 离线 `cargo metadata` 失败 / 产物摘要失败）；先跑 `polypack build` 再重试 |
