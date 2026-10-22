/* PolyPack static site: i18n, navigation, tabs, QR modal, back-to-top, perf probes.
   No framework, no network reads; handlers are synchronous and CSS-driven. */
(() => {
  'use strict';

  const html = document.documentElement;
  const body = document.body;
  const $ = (sel, root = document) => root.querySelector(sel);
  const $$ = (sel, root = document) => [...root.querySelectorAll(sel)];
  const langToggle = $('#langToggle');
  const nav = $('#nav');
  const navToggle = $('#navToggle');
  const toTop = $('#toTop');
  const modal = $('#qrModal');
  const modalImage = $('#qrImage');
  const modalTitle = $('#qrTitle');
  const modalClose = $('#qrClose');
  const reduceMotion = window.matchMedia('(prefers-reduced-motion: reduce)').matches;
  let lastFocus = null;
  let lang = 'zh';
  const longTasks = [];
  if ('PerformanceObserver' in window) {
    try {
      new PerformanceObserver((list) => {
        for (const entry of list.getEntries()) longTasks.push({ duration: entry.duration, start: entry.startTime });
      }).observe({ type: 'longtask', buffered: true });
    } catch (_) { /* longtask unsupported */ }
  }

  const dict = {
    'post.readA': { zh: '阅读', en: 'Read' },
    'post.readB': { zh: '次 👀', en: 'views 👀' },
  };

  function pickLang() {
    const query = new URLSearchParams(location.search).get('lang');
    if (query === 'en' || query === 'zh') return query;
    const saved = localStorage.getItem('polypack-lang');
    if (saved === 'en' || saved === 'zh') return saved;
    return (navigator.language || '').toLowerCase().startsWith('en') ? 'en' : 'zh';
  }

  function setMeta(name, value, attr = 'content') {
    const node = document.querySelector(`meta[${name}]`);
    if (node) node.setAttribute(attr, value);
  }

  function applyLang(next) {
    lang = next;
    html.lang = next === 'en' ? 'en' : 'zh-CN';
    html.dataset.lang = next;

    // The document ships in Chinese; on the first zh application the DOM already
    // matches, so skip 120+ text writes. Switching languages always updates.
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
      for (const node of $$('img[data-en-src]')) {
        if (!node.dataset.zhSrc) node.dataset.zhSrc = node.getAttribute('src');
        node.src = next === 'en' ? node.dataset.enSrc : node.dataset.zhSrc;
      }
      for (const node of $$('[data-i18n]')) {
        const entry = dict[node.dataset.i18n];
        if (entry) node.textContent = entry[next];
      }
    }
    html.dataset.langApplied = '1';

    const title = next === 'en' ? body.dataset.titleEn : body.dataset.titleZh;
    const desc = next === 'en' ? body.dataset.descEn : body.dataset.descZh;
    document.title = title;
    setMeta('name="description"', desc);
    setMeta('property="og:title"', title);
    setMeta('property="og:description"', desc);
    setMeta('property="og:locale"', next === 'en' ? 'en_US' : 'zh_CN');
    setMeta('name="twitter:title"', title);
    setMeta('name="twitter:description"', desc);
    langToggle.textContent = next === 'en' ? '中文' : 'EN';
    langToggle.setAttribute('aria-label', next === 'en' ? '切换到中文' : 'Switch to English');
    localStorage.setItem('polypack-lang', next);

    const url = new URL(location.href);
    if (next === 'en') url.searchParams.set('lang', 'en');
    else url.searchParams.delete('lang');
    history.replaceState(null, '', url);
  }

  /* ---- language ------------------------------------------------------- */
  langToggle.addEventListener('click', () => {
    applyLang(lang === 'zh' ? 'en' : 'zh');
  });

  /* ---- mobile navigation --------------------------------------------- */
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

  /* ---- code copy ------------------------------------------------------ */
  async function copyText(text) {
    if (navigator.clipboard && window.isSecureContext) {
      await navigator.clipboard.writeText(text);
      return;
    }
    const area = document.createElement('textarea');
    area.value = text;
    area.setAttribute('readonly', '');
    area.style.position = 'fixed';
    area.style.opacity = '0';
    document.body.appendChild(area);
    area.select();
    document.execCommand('copy');
    area.remove();
  }

  for (const button of $$('.copy')) {
    button.addEventListener('click', () => {
      const block = button.closest('.code-card');
      const text = block?.querySelector('pre')?.innerText ?? '';
      copyText(text).then(() => {
        button.textContent = lang === 'en' ? 'Copied' : '已复制';
        button.classList.add('done');
        setTimeout(() => {
          button.textContent = 'Copy';
          button.classList.remove('done');
        }, 1200);
      }).catch(() => {
        button.textContent = 'Copy failed';
      });
    });
  }

  /* ---- install tabs --------------------------------------------------- */
  const tabs = $$('.tab');
  for (const tab of tabs) {
    tab.addEventListener('click', () => {
      for (const other of tabs) {
        const selected = other === tab;
        other.setAttribute('aria-selected', String(selected));
        const panel = document.getElementById(other.getAttribute('aria-controls'));
        if (panel) panel.hidden = !selected;
      }
    });
  }

  /* ---- QR modal ------------------------------------------------------- */
  function openQR(button) {
    lastFocus = button;
    modalImage.src = button.dataset.qr;
    modalTitle.textContent = lang === 'en' ? button.dataset.titleEn : button.dataset.titleZh;
    modal.hidden = false;
    modal.classList.add('open');
    document.body.style.overflow = 'hidden';
    modalClose.focus({ preventScroll: true });
  }
  function closeQR() {
    modal.classList.remove('open');
    document.body.style.overflow = '';
    modal.hidden = true;
    if (lastFocus) lastFocus.focus({ preventScroll: true });
  }
  for (const card of $$('.qr-card')) card.addEventListener('click', () => openQR(card));
  modalClose.addEventListener('click', closeQR);
  modal.addEventListener('click', (event) => {
    if (event.target === modal) closeQR();
  });
  document.addEventListener('keydown', (event) => {
    if (event.key === 'Escape' && modal.classList.contains('open')) closeQR();
  });

  /* ---- back to top ---------------------------------------------------- */
  let ticking = false;
  function onScroll() {
    if (ticking) return;
    ticking = true;
    requestAnimationFrame(() => {
      toTop.classList.toggle('show', window.scrollY > 520);
      ticking = false;
    });
  }
  window.addEventListener('scroll', onScroll, { passive: true });
  onScroll();
  toTop.addEventListener('click', () => {
    window.scrollTo({ top: 0, behavior: reduceMotion ? 'auto' : 'smooth' });
  });

  /* ---- reveal --------------------------------------------------------- */
  if ('IntersectionObserver' in window) {
    const io = new IntersectionObserver((entries) => {
      for (const entry of entries) {
        if (entry.isIntersecting) {
          entry.target.classList.add('in');
          io.unobserve(entry.target);
        }
      }
    }, { rootMargin: '0px 0px -8% 0px', threshold: 0.08 });
    for (const node of $$('.reveal')) io.observe(node);
  } else {
    for (const node of $$('.reveal')) node.classList.add('in');
  }

  /* ---- three.js: worker + OffscreenCanvas, deferred and optional ------- */
  function bootThree() {
    const canvas = $('#bg-canvas');
    if (reduceMotion || !canvas) return;
    if (typeof canvas.transferControlToOffscreen !== 'function') return;
    let worker;
    let offscreen;
    try {
      offscreen = canvas.transferControlToOffscreen();
      worker = new Worker('js/three-worker.js');
    } catch (_) {
      return;
    }
    const host = canvas.parentElement;
    const postSize = () => worker.postMessage({
      type: 'resize',
      width: host.clientWidth,
      height: host.clientHeight,
      dpr: window.devicePixelRatio || 1,
    });
    worker.onerror = () => { canvas.dataset.three = 'error'; worker.terminate(); };
    worker.onmessage = (event) => {
      if (event.data?.type === 'ready' && !canvas.dataset.three) canvas.dataset.three = 'ready';
      if (event.data?.type === 'rendered') {
        canvas.dataset.three = 'rendered';
        canvas.dataset.threeSize = `${event.data.width}x${event.data.height}/${event.data.count}`;
      }
      if (event.data?.type === 'unsupported') { canvas.dataset.three = 'fallback'; worker.terminate(); }
    };
    worker.postMessage({
      type: 'init',
      canvas: offscreen,
      width: host.clientWidth,
      height: host.clientHeight,
      dpr: window.devicePixelRatio || 1,
    }, [offscreen]);

    if ('ResizeObserver' in window) new ResizeObserver(postSize).observe(host);
    else window.addEventListener('resize', postSize, { passive: true });

    let pointerTick = false;
    window.addEventListener('pointermove', (event) => {
      if (pointerTick) return;
      pointerTick = true;
      requestAnimationFrame(() => {
        worker.postMessage({
          type: 'pointer',
          x: (event.clientX / window.innerWidth - 0.5) * 2,
          y: (event.clientY / window.innerHeight - 0.5) * 2,
        });
        pointerTick = false;
      });
    }, { passive: true });

    const setVisible = () => worker.postMessage({
      type: 'visible',
      visible: !document.hidden,
    });
    document.addEventListener('visibilitychange', setVisible);
    if ('IntersectionObserver' in window) {
      new IntersectionObserver((entries) => {
        worker.postMessage({ type: 'visible', visible: entries.some((entry) => entry.isIntersecting) && !document.hidden });
      }, { threshold: 0 }).observe(host);
    }
    window.addEventListener('pagehide', () => {
      worker.postMessage({ type: 'stop' });
      worker.terminate();
    }, { once: true });
  }
  const idle = window.requestIdleCallback || ((fn) => setTimeout(fn, 800));
  document.addEventListener('DOMContentLoaded', () => idle(bootThree, { timeout: 2000 }), { once: true });

  /* ---- Busuanzi stats: load after first paint so a slow third party cannot
     hold the window load event (and therefore the UI probes) hostage. */
  const statsHost = $('#busuanzi');
  if (statsHost?.dataset.busuanziSrc) {
    setTimeout(() => {
      const script = document.createElement('script');
      script.async = true;
      script.src = statsHost.dataset.busuanziSrc;
      statsHost.appendChild(script);
    }, 1200);
  }

  /* ---- perf probe (?perf=1) ------------------------------------------ */
  function perfProbe() {
    if (!new URLSearchParams(location.search).has('perf')) return;
    const navEntry = performance.getEntriesByType('navigation')[0];
    const fcp = performance.getEntriesByName('first-contentful-paint')[0]?.startTime ?? null;
    const measure = (fn) => {
      const start = performance.now();
      fn();
      return Math.round((performance.now() - start) * 1000) / 1000;
    };
    const langMs = measure(() => {
      applyLang(lang === 'zh' ? 'en' : 'zh');
      applyLang(lang === 'zh' ? 'en' : 'zh');
    });
    const modalMs = measure(() => {
      const card = $('.qr-card');
      openQR(card);
      closeQR();
    });
    const scrollMs = measure(() => {
      toTop.classList.toggle('show', true);
      toTop.classList.toggle('show', false);
    });
    const navMs = measure(() => {
      nav.classList.add('open');
      nav.classList.remove('open');
    });
    const tabMs = measure(() => {
      const windows = document.getElementById('tab-windows');
      const linux = document.getElementById('tab-linux');
      windows.click();
      linux.click();
    });
    const homeMs = measure(() => {
      const home = document.querySelector('#nav a[href="#top"]');
      const previous = html.style.scrollBehavior;
      html.style.scrollBehavior = 'auto';
      if (home) home.click();
      window.scrollTo(0, 0);
      html.style.scrollBehavior = previous;
    });
    const maxLong = longTasks.length ? Math.max(...longTasks.map((entry) => entry.duration)) : 0;
    const payload = {
      ttfb_ms: navEntry ? Math.round(navEntry.responseStart * 10) / 10 : null,
      dom_content_loaded_ms: navEntry ? Math.round(navEntry.domContentLoadedEventEnd * 10) / 10 : null,
      load_ms: navEntry && navEntry.loadEventEnd ? Math.round(navEntry.loadEventEnd * 10) / 10 : null,
      fcp_ms: fcp === null ? null : Math.round(fcp * 10) / 10,
      lang_toggle_ms: langMs,
      qr_modal_open_close_ms: modalMs,
      scroll_toggle_ms: scrollMs,
      nav_toggle_ms: navMs,
      tab_switch_ms: tabMs,
      home_nav_ms: homeMs,
      long_task_max_ms: maxLong === 0 ? null : Math.round(maxLong * 10) / 10,
      long_tasks: longTasks.map((entry) => ({ start_ms: Math.round(entry.start * 10) / 10, duration_ms: Math.round(entry.duration * 10) / 10 })),
      budget_50ms: [langMs, modalMs, scrollMs, navMs, tabMs, homeMs].every((value) => value < 50),
    };
    const out = document.createElement('pre');
    out.id = 'perf-output';
    out.hidden = true;
    out.textContent = JSON.stringify(payload);
    document.body.appendChild(out);
  }
  document.addEventListener('DOMContentLoaded', () => setTimeout(perfProbe, 1200), { once: true });

  applyLang(pickLang());
})();
