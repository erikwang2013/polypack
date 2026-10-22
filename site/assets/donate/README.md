# Donation QR codes

`wechat.svg` and `alipay.svg` are **placeholders** until the maintainer supplies real payment codes.

To replace them:

1. Export your WeChat Pay and Alipay receive-code images (PNG or SVG).
2. Save them as `site/assets/donate/wechat.png` / `alipay.png` (or overwrite the SVGs with the same filenames).
3. If you change the file extension, update the two `data-qr` attributes and the fallback `<img src>` in `site/index.html`.
4. Keep the modal and donation card paths relative; never commit private payment credentials, only the public receive-code image.

The site only displays these images locally; no payment data is sent anywhere.
