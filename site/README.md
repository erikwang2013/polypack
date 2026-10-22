# PolyPack static site

A dependency-free static site (HTML + CSS + vanilla JS + local three.js) for project introduction, features, iterations, changelog, install/usage docs, downloads, stats and donation.

## Preview

```bash
python3 -m http.server 8080 -d site
# open http://127.0.0.1:8080/
```

Use `?lang=en` for English and `?perf=1` for the built-in interaction probe.

## Performance budget

- No framework, web fonts, or remote CSS/JS; three.js is vendored and runs in a worker with OffscreenCanvas after idle, keeping parsing and WebGL setup off the main thread.
- Images are local SVGs/PNG with lazy loading below the fold, explicit dimensions and `content-visibility` on sections.
- Event handlers are synchronous, CSS-driven, and passive where applicable. `?perf=1` writes a hidden JSON block with language-toggle, QR-modal and scroll-toggle timings plus navigation/FCP marks. Target: **each UI response < 50 ms** on a local static server.
- The hero canvas pauses when off-screen or the tab is hidden, caps DPR at 1.75, and uses fewer particles on mobile; the worker is optional and the CSS grid/glow is the fallback.

Measured on 2026-10-22 with Python 3.12 `http.server`, headless Chrome 151, localhost:

| Metric | Value |
| --- | --- |
| TTFB (curl) | ~10 ms |
| Nav TTFB (Chrome) | 7–29 ms |
| DOMContentLoaded | 249.0 ms |
| Load | 254.0 ms |
| First Contentful Paint | 400 ms |
| Language toggle | 10.0 ms |
| QR modal open + close | 11.2 ms |
| Nav / tab / back-to-top | 0.0–3.2 ms |
| Main-thread long tasks | 0 in the measured run |

The interaction budget is < 50 ms; the paint/load numbers are reported for context, not claimed as interaction latency.

## SEO

`index.html` includes canonical, hreflang, Open Graph/Twitter cards, JSON-LD `SoftwareApplication`, semantic headings, alt text and a skip link. `robots.txt` and `sitemap.xml` point at `https://package.erik.xyz/polypack/`; change the canonical host if the site is deployed elsewhere.

## Donation QR codes

`assets/img/weixinpay.png` and `assets/img/alipay.png` are the maintainer-provided public receive codes. The modal only zooms the local image and sends no payment data.

## Stats

The footer uses the Busuanzi page-view snippet. It requires the site to be served over HTTP(S); it is intentionally not loaded from local files.

## Deploy

Upload the contents of `site/` to any static host (GitHub Pages, nginx, object storage). Keep the directory structure intact. If hosting at a subpath, update the canonical/OG URLs in `index.html` and `sitemap.xml`; relative asset paths already work at any subpath.
