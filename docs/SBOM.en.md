# SBOM and build provenance (M9-E1)

> Status: T-M9-01/02/03 delivered; a sample lives in `docs/samples/sbom/web/`. Chinese: [SBOM.md](SBOM.md).

## 1. What it is / is not

| Is | Is not |
| --- | --- |
| One SPDX 2.3 document, one CycloneDX 1.5 document, and one in-toto Statement provenance per artifact | Not SLSA certification and claims no SLSA level |
| Dependency graph comes from `cargo metadata --locked --offline` and is aligned with `Cargo.lock` | No network, Sigstore / Rekor, or remote attestation |
| Deterministic output: the same input produces byte-identical files on a second run | Not a substitute for artifact signing; `polypack sign` / `polypack verify` still own signatures and checksums |

## 2. Generate

Build first, then let `verify` generate SBOMs for the staged artifacts:

```bash
polypack build --target web
polypack verify --sbom --target web
# JSON event stream for automation
polypack verify --sbom --target web --json
```

`--sbom` can run alongside other verifiers; a target with no staged artifact warns and is skipped rather than emitting an empty SBOM. Generation failures return `polypack::E_VERIFY_0003` and never degrade silently.

## 3. Output layout

```text
.polypack/sbom/<target>/
  <artifact>.spdx.json
  <artifact>.cdx.json
  <artifact>.intoto.jsonl
```

For example the `site` artifact yields `site.spdx.json`, `site.cdx.json`, and `site.intoto.jsonl`. Files are sorted by artifact name and the directory belongs only to that target.

## 4. Determinism and privacy

- The timestamp is fixed at `1970-01-01T00:00:00Z`; neither the system clock nor `SOURCE_DATE_EPOCH` is read.
- Packages, JSON keys, and artifact digests are sorted; the same project and artifact produce byte-identical files on a second run.
- The source-tree digest excludes `target`, `.polypack`, `.git`, and `node_modules`, so build caches never enter provenance.
- Provenance contains only build metadata (toolchain, target declaration, materials digest, artifact digests); it executes or uploads nothing.

## 5. Release receipts

`polypack release` computes `sbom_digest` with the same artifact-set digest algorithm used for SBOMs and writes it into the receipt. The field is optional and old receipts without it are treated as absent, so release records and SBOMs reference the same artifact digests.

## 6. Sample and tests

Committed sample (generated from a minimal web static-site fixture; byte-identical on a second run):

```bash
find docs/samples/sbom/web -type f
jq -r .spdxVersion docs/samples/sbom/web/site.spdx.json   # SPDX-2.3
jq -r .specVersion docs/samples/sbom/web/site.cdx.json    # 1.5
jq -r '._type' docs/samples/sbom/web/site.intoto.jsonl     # https://in-toto.io/Statement/v1
```

Acceptance tests:

```bash
cargo test -p polypack-cli --test sbom
node tools/check-docs.mjs
```

## 7. Diagnostics

| Code | Meaning |
| --- | --- |
| `polypack::E_VERIFY_0003` | SBOM generation failed (missing `Cargo.lock`, offline `cargo metadata` failure, or artifact digest failure); run `polypack build` and retry |
