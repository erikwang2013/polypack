# Store metadata pipeline (M9-E5)

> Status: implemented (T-M9-13/14/15). `store/metadata/<platform>/<locale>.toml` is validated by `polypack publish --metadata-dir` and deterministically packed into submission materials; `--check-metadata` validates without publishing. Screenshots are stored as paths only; image bytes are not read.

## 1. Directory and schema

```text
store/metadata/
  wechat/
    default.toml
    zh-CN.toml      # optional; used for zh-CN requests, falls back to default with a warning
  alipay/
    default.toml
```

Every file must declare `schema = 1`. Unknown versions, a missing `default.toml`, and duplicate locales fail with stable errors rather than silently degrading. Screenshot paths are relative to the platform directory (e.g. `screenshots/home.png`), and extensions are checked against the platform rules.

## 2. Platform rules

Lengths use the platform convention: CJK / full-width characters count as 2, everything else as 1.

| Field | WeChat (`wechat`) | Alipay (`alipay`) |
| --- | --- | --- |
| title | required, 4–30 | required, 3–20 |
| short_description | required, 4–120 | required, 10–32 |
| full_description | required, 4–2000 | required, 20–500 |
| keywords | required, 1–10 items, each 1–20 | required, 1–10 items, each 1–20 |
| screenshots | 0–10 paths, png/jpg/jpeg/gif | required, 2–5 paths, png/jpg/jpeg |
| privacy_url / support_url | required, absolute http(s) URL | required, absolute http(s) URL |

Rule sources and the retrieval date live in the `crates/polypack-publish/src/metadata.rs` module docs; when platform rules change, bump `schema` and update that table.

## 3. Complete example

```toml
# store/metadata/wechat/default.toml
schema = 1
title = "Pax Notes"
short_description = "Offline-first packaging notes"
full_description = "Pax Notes is an offline-first notes app with multi-target packaging and local-first storage."
keywords = ["notes", "offline", "packaging"]
screenshots = ["screenshots/home.png", "screenshots/editor.png"]
privacy_url = "https://example.com/privacy"
support_url = "https://example.com/support"
```

## 4. Commands

```bash
# Validate only: no artifact/credential parsing, no network
polypack publish --check-metadata --metadata-dir store/metadata --channel wechat-open-platform

# Validate and publish as submission materials
polypack publish --metadata-dir store/metadata --channel wechat-open-platform \
  --appid wx0000000000000000 --version 1.2.3 --credential env:WECHAT_KEY --package dist
```

`--materials <file>` and `--metadata-dir` are mutually exclusive; passing both is rejected by clap. `--metadata-locale` selects the requested locale; when missing it falls back to `default` with a warning.

## 5. Error codes

| Code | Meaning |
| --- | --- |
| `metadata.default_missing` | platform directory has no `default.toml` |
| `metadata.locale_duplicate` | normalized locale is duplicated |
| `metadata.schema_missing` / `metadata.schema_unsupported` | missing or unsupported `schema` |
| `metadata.field_missing` / `metadata.field_empty` | required field missing / empty |
| `metadata.field_too_short` / `metadata.field_too_long` | field length out of range |
| `metadata.keywords_too_few` / `metadata.keywords_too_many` | keyword count out of range |
| `metadata.keyword_empty` / `metadata.keyword_too_long` | individual keyword invalid |
| `metadata.screenshots_required` / `metadata.screenshots_too_few` / `metadata.screenshots_too_many` | screenshot count invalid |
| `metadata.screenshot_path_invalid` / `metadata.screenshot_path_missing` / `metadata.screenshot_extension` | screenshot path escapes / missing / bad extension |
| `metadata.url_invalid` | `privacy_url` / `support_url` is not an absolute http(s) URL |

Validation reports every violation, not just the first. Material objects are key-sorted; the digest is blake3(canonical JSON) and is identical across processes / platforms for the same input; the receipt records it as `materials_digest`.
