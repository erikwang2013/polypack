# polypack-cli

`polypack` 命令行入口。本文件只记录 `polypack release` 的自托管仓库发布面（T-M5-05）：目录布局、
客户端消费、签名凭据契约与幂等口径。仓库元数据实现见 `src/release/repo/`，签名抽象见
`polypack-sign::repo`。

## 仓库布局

`--repo <root>` 是同一棵目录树，deb 与 rpm 可并存（各自的 `dists/`、`repodata/` 不冲突）：

```text
<root>/
  polypack-repo.gpg                    # 公钥（二进制；apt signed-by）
  polypack-repo.asc                    # 公钥（ASCII armored；dnf gpgkey / 人读）
  pool/main/<prefix>/<package>/*.deb   # prefix：lib* 取前 4 字符，其余取首字符
  dists/<suite>/main/binary-<arch>/Packages
  dists/<suite>/{Release,InRelease,Release.gpg}
  rpm/*.rpm
  repodata/{primary,filelists,other}.xml.gz
  repodata/repomd.xml
  repodata/repomd.xml.asc
```

`<suite>` = 发布目标 id（`ubuntu` / `mint` / `fedora`）；rpm 的 `location href` 为
`rpm/<file>.rpm`。旧版本产物不会自动清理（pool / rpm 目录按文件保留，元数据只列本次发布
批次）；需要回收时按回执与产物清单人工删除。

## 发布

```bash
polypack release --target ubuntu,fedora --repo .polypack/repo \
  --credential env:POLYPACK_REPO_GPG
```

- `--credential` 只接受凭据**引用**（`env:VAR` / `keyring[:条目]` / `vault[:条目]`）；
  缺省用目标声明的 `signing` 档案（`linux-repo-gpg`），来源为系统 keyring。
- 载荷是 base64(JSON)：`{"keystore":"<base64(export-secret-keys 字节)>",
  "store_password":"<口令>","key_alias":"<可选指纹/UID>","key_password":"<可选>"}`。
  `key_alias` 为空时可用仓库里唯一签名密钥。
- 私钥只在内存与一次性 GNUPGHOME（0700，退出即删）中存在；仓库里只落公钥。测试与日志
  不回显载荷；`polypack scan` 对生成仓库应为绿。
- `--dry-run` 只解析产物与回执锚点，不写仓库、不要求签名工具。

## 客户端消费

apt（`signed-by` 固定公钥，`file:` 路径可用于本机 / 内网镜像）：

```text
deb [signed-by=/srv/polypack/polypack-repo.gpg] file:/srv/polypack ubuntu main
```

```bash
apt-get update
apt-get install fixture-app
```

dnf（`repo_gpgcheck=1` 校验 repomd 签名；公钥用于导入）：

```ini
[polypack]
name=PolyPack
baseurl=file:///srv/polypack
enabled=1
repo_gpgcheck=1
gpgcheck=0
gpgkey=file:///srv/polypack/polypack-repo.asc
```

```bash
dnf makecache
dnf install polypack-repo-probe
```

宿主没有 dnf 时，`cargo test` 中 `dnf_makecache_consumes_signed_repository_when_available`
打印 SKIP；真跑口径是在 Debian 12 容器内 `apt-get install dnf` 后执行上面的配置
（`repo_gpgcheck=1` 会触发 GPG key 导入并输出 `Metadata cache created.`）。

## 幂等与时间

- 输入排序、gzip 固定 `mtime=0`、产物复制保留源 mtime：同一批产物重复生成时
  `Packages` / `Release` / `repomd.xml` 与 `*.xml.gz` 字节一致；差异只可能来自 GPG
  签名包内的时间戳。
- 元数据时刻优先取 `SOURCE_DATE_EPOCH`，否则取本批产物 mtime 的最大值；`Release` 的
  `Date` 为 RFC 2822 UTC，`repomd` 的 `revision` / `timestamp` 为 unix 秒。
- deb 解析复用 `polypack-bundle::arwrite`（与打包器 / verify 腿同一 control 解析）；当前只
  接受 gzip control（`polypack` 的 deb 产物固定 `-Zgzip`），xz/zst 会响亮报
  `E_RELEASE_0005` 而不是静默漏字段。
- rpm 的 repodata 只读解析在 `src/release/repo/rpm/header.rs` 内（`rpm_header.rs` 仍保持
  verify 自检的最小公开面）；后续可把共享解析上移。

## 密钥轮换

先部署新的 `polypack-repo.gpg` / `.asc` 与整批新元数据（`InRelease` /
`Release.gpg` / `repomd.xml.asc`），再让客户端切换；旧公钥可保留一个发布周期以便
平滑过渡。不要把私钥、口令文件或 payload 写进仓库 / 工程目录。
