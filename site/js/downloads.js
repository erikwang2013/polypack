/* Downloads page: bilingual toggle, copy buttons and pet back-to-top. */
(() => {
  'use strict';
  const html = document.documentElement;
  const body = document.body;
  const $ = (sel) => document.querySelector(sel);
  const $$ = (sel) => [...document.querySelectorAll(sel)];
  const langToggle = $('#langToggle');
  const nav = $('#nav');
  const navToggle = $('#navToggle');
  const toTop = $('#toTop');
  const platformCta = $('#platformDownload');
  const platformHint = $('#platformHint');
  let lang = 'zh';

  const TARGETS = {
    windows: {
      href: 'polypack-2.0.1-x86_64.msi',
      zh: '下载 Windows 安装包', en: 'Download for Windows',
      labelZh: 'Windows x86_64 · MSI', labelEn: 'Windows x86_64 · MSI',
    },
    macos: {
      href: 'polypack-2.0.1-aarch64.pkg',
      zh: '下载 macOS 安装包', en: 'Download for macOS',
      labelZh: 'macOS Apple Silicon · pkg', labelEn: 'macOS Apple Silicon · pkg',
    },
    'linux-deb': {
      href: 'polypack_2.0.1_amd64.deb',
      zh: '下载 Debian / Ubuntu 包', en: 'Download for Debian / Ubuntu',
      labelZh: 'Debian / Ubuntu x86_64 · deb', labelEn: 'Debian / Ubuntu x86_64 · deb',
    },
    linux: {
      href: 'polypack-2.0.1-x86_64.AppImage',
      zh: '下载 AppImage', en: 'Download AppImage',
      labelZh: 'Linux x86_64 · AppImage', labelEn: 'Linux x86_64 · AppImage',
    },
  };

  function detectTarget() {
    const ua = navigator.userAgent || '';
    const platform = (navigator.userAgentData && navigator.userAgentData.platform) || navigator.platform || '';
    if (/win/i.test(platform) || /windows/i.test(ua)) return 'windows';
    if (/mac/i.test(platform) || /macintosh|mac os x/i.test(ua)) return 'macos';
    if (/linux|x11/i.test(platform) || /linux/i.test(ua)) {
      return /ubuntu|debian|linux mint|pop!_os/i.test(ua) ? 'linux-deb' : 'linux';
    }
    return '';
  }

  const detected = detectTarget();

  function syncPlatform() {
    const target = TARGETS[detected];
    for (const card of $$('.download-card[data-platform]')) {
      card.classList.toggle('detected', card.dataset.platform === detected);
    }
    if (!platformCta) return;
    if (!target) {
      platformCta.removeAttribute('download');
      platformCta.href = '#installers';
      platformCta.textContent = lang === 'en' ? 'Choose your platform' : '选择你的平台';
      if (platformHint) platformHint.hidden = true;
      return;
    }
    platformCta.href = target.href;
    platformCta.setAttribute('download', '');
    platformCta.textContent = lang === 'en' ? target.en : target.zh;
    if (platformHint) {
      platformHint.textContent = lang === 'en'
        ? `Detected ${target.labelEn} — this button downloads that file.`
        : `检测到 ${target.labelZh}，按钮直接下载该文件。`;
      platformHint.hidden = false;
    }
  }

  function pickLang() {
    const query = new URLSearchParams(location.search).get('lang');
    if (query === 'en' || query === 'zh') return query;
    const saved = localStorage.getItem('polypack-lang');
    if (saved === 'en' || saved === 'zh') return saved;
    return (navigator.language || '').toLowerCase().startsWith('en') ? 'en' : 'zh';
  }

  function applyLang(next) {
    lang = next;
    html.lang = next === 'en' ? 'en' : 'zh-CN';
    html.dataset.lang = next;
    const firstZh = next === 'zh' && html.dataset.langApplied !== '1';
    if (!firstZh) {
      for (const node of $$('[data-zh]')) {
        const value = node.dataset[next];
        if (value !== undefined) node.textContent = value;
      }
      for (const node of $$('[data-zh-html]')) {
        const value = node.dataset[next + 'Html'];
        if (value !== undefined) node.innerHTML = value;
      }
      for (const node of $$('[data-zh-aria]')) {
        const value = node.dataset[next + 'Aria'];
        if (value !== undefined) node.setAttribute('aria-label', value);
      }
    }
    html.dataset.langApplied = '1';
    const title = next === 'en' ? body.dataset.titleEn : body.dataset.titleZh;
    const desc = next === 'en' ? body.dataset.descEn : body.dataset.descZh;
    document.title = title;
    const meta = document.querySelector('meta[name="description"]');
    if (meta) meta.setAttribute('content', desc);
    langToggle.textContent = next === 'en' ? '中文' : 'EN';
    langToggle.setAttribute('aria-label', next === 'en' ? '切换到中文' : 'Switch to English');
    localStorage.setItem('polypack-lang', next);
    const url = new URL(location.href);
    if (next === 'en') url.searchParams.set('lang', 'en');
    else url.searchParams.delete('lang');
    history.replaceState(null, '', url);
    syncPlatform();
  }

  langToggle.addEventListener('click', () => applyLang(lang === 'zh' ? 'en' : 'zh'));
  navToggle.addEventListener('click', () => {
    const open = nav.classList.toggle('open');
    navToggle.setAttribute('aria-expanded', String(open));
  });
  nav.addEventListener('click', (event) => {
    if (event.target.closest('a')) {
      nav.classList.remove('open');
      navToggle.setAttribute('aria-expanded', 'false');
    }
  });

  for (const button of $$('.copy')) {
    button.addEventListener('click', () => {
      const text = button.closest('.code-card')?.querySelector('pre')?.innerText ?? '';
      const done = () => {
        button.textContent = lang === 'en' ? 'Copied' : '已复制';
        button.classList.add('done');
        setTimeout(() => {
          button.textContent = 'Copy';
          button.classList.remove('done');
        }, 1200);
      };
      if (navigator.clipboard && window.isSecureContext) {
        navigator.clipboard.writeText(text).then(done).catch(() => {});
      } else {
        const area = document.createElement('textarea');
        area.value = text;
        area.style.position = 'fixed';
        area.style.opacity = '0';
        document.body.appendChild(area);
        area.select();
        document.execCommand('copy');
        area.remove();
        done();
      }
    });
  }

  let ticking = false;
  function onScroll() {
    if (ticking) return;
    ticking = true;
    requestAnimationFrame(() => {
      toTop.classList.toggle('show', window.scrollY > 420);
      ticking = false;
    });
  }
  window.addEventListener('scroll', onScroll, { passive: true });
  onScroll();
  toTop.addEventListener('click', () => window.scrollTo({ top: 0, behavior: 'smooth' }));

  applyLang(pickLang());
})();
