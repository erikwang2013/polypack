/* PolyPack hero effect: local three.js, no textures fetched, pauses off-screen. */
(function () {
  'use strict';
  window.initPolyPackThree = function initPolyPackThree(canvas) {
    if (!canvas || !window.THREE) return;
    const THREE = window.THREE;
    const reduce = window.matchMedia('(prefers-reduced-motion: reduce)').matches;
    const host = canvas.parentElement;
    const scene = new THREE.Scene();
    scene.fog = new THREE.FogExp2(0x07111f, 0.055);

    const camera = new THREE.PerspectiveCamera(52, 1, 0.1, 120);
    camera.position.set(0, 0, 17);

    let renderer;
    try {
      renderer = new THREE.WebGLRenderer({ canvas, antialias: true, alpha: true, powerPreference: 'low-power' });
    } catch (_) {
      return;
    }
    renderer.setClearColor(0x000000, 0);
    renderer.setPixelRatio(Math.min(window.devicePixelRatio || 1, 1.75));

    const palette = [0x22d3ee, 0x34d399, 0xfde68a, 0x38bdf8];
    const compact = window.matchMedia('(max-width: 720px)').matches;
    const count = compact ? 90 : 180;
    const positions = new Float32Array(count * 3);
    const colors = new Float32Array(count * 3);
    const color = new THREE.Color();

    for (let i = 0; i < count; i += 1) {
      positions[i * 3] = (Math.random() - 0.5) * 34;
      positions[i * 3 + 1] = (Math.random() - 0.5) * 20;
      positions[i * 3 + 2] = (Math.random() - 0.5) * 18;
      color.setHex(palette[i % palette.length]);
      colors[i * 3] = color.r;
      colors[i * 3 + 1] = color.g;
      colors[i * 3 + 2] = color.b;
    }
    const geometry = new THREE.BufferGeometry();
    geometry.setAttribute('position', new THREE.BufferAttribute(positions, 3));
    geometry.setAttribute('color', new THREE.BufferAttribute(colors, 3));

    const sprite = document.createElement('canvas');
    sprite.width = 32;
    sprite.height = 32;
    const ctx = sprite.getContext('2d');
    const gradient = ctx.createRadialGradient(16, 16, 0, 16, 16, 16);
    gradient.addColorStop(0, 'rgba(255,255,255,1)');
    gradient.addColorStop(0.35, 'rgba(125,243,255,.9)');
    gradient.addColorStop(1, 'rgba(34,211,238,0)');
    ctx.fillStyle = gradient;
    ctx.fillRect(0, 0, 32, 32);
    const texture = new THREE.CanvasTexture(sprite);
    const material = new THREE.PointsMaterial({
      size: compact ? 0.22 : 0.18,
      map: texture,
      vertexColors: true,
      transparent: true,
      opacity: 0.9,
      depthWrite: false,
      blending: THREE.AdditiveBlending,
      sizeAttenuation: true,
    });
    const points = new THREE.Points(geometry, material);
    scene.add(points);

    // A few wireframe "packages" drifting behind the headline.
    const group = new THREE.Group();
    const boxes = [];
    const boxGeometry = new THREE.BoxGeometry(1, 1, 1);
    for (let i = 0; i < 4; i += 1) {
      const edges = new THREE.LineSegments(
        new THREE.EdgesGeometry(boxGeometry),
        new THREE.LineBasicMaterial({ color: palette[i % palette.length], transparent: true, opacity: 0.28 })
      );
      const scale = 1.1 + Math.random() * 2.1;
      edges.scale.setScalar(scale);
      edges.position.set((Math.random() - 0.5) * 20, (Math.random() - 0.5) * 12, -2 - Math.random() * 8);
      edges.userData.spin = 0.0015 + Math.random() * 0.004;
      group.add(edges);
      boxes.push(edges);
    }
    scene.add(group);

    const pointer = { x: 0, y: 0 };
    function onPointer(event) {
      const touch = event.touches?.[0];
      const x = touch ? touch.clientX : event.clientX;
      const y = touch ? touch.clientY : event.clientY;
      pointer.x = (x / window.innerWidth - 0.5) * 2;
      pointer.y = (y / window.innerHeight - 0.5) * 2;
    }
    window.addEventListener('pointermove', onPointer, { passive: true });
    window.addEventListener('touchmove', onPointer, { passive: true });

    let width = 0;
    let height = 0;
    function resize() {
      width = host.clientWidth;
      height = host.clientHeight;
      if (!width || !height) return;
      renderer.setSize(width, height, false);
      camera.aspect = width / height;
      camera.updateProjectionMatrix();
      const spread = Math.max(1, width / 900);
      points.scale.setScalar(spread);
    }
    const ro = 'ResizeObserver' in window ? new ResizeObserver(resize) : null;
    if (ro) ro.observe(host);
    window.addEventListener('resize', resize, { passive: true });
    resize();

    let visible = true;
    document.addEventListener('visibilitychange', () => { visible = !document.hidden; });
    if ('IntersectionObserver' in window) {
      new IntersectionObserver((entries) => {
        visible = entries.some((entry) => entry.isIntersecting) && !document.hidden;
      }, { threshold: 0 }).observe(host);
    }

    let raf = 0;
    function frame(t) {
      raf = requestAnimationFrame(frame);
      if (!visible || reduce) return;
      points.rotation.y = t * 0.000035;
      points.rotation.x = Math.sin(t * 0.00002) * 0.06;
      group.rotation.y = pointer.x * 0.08 + Math.sin(t * 0.00003) * 0.05;
      group.rotation.x = pointer.y * 0.05;
      for (const box of boxes) {
        box.rotation.x += box.userData.spin;
        box.rotation.y += box.userData.spin * 0.7;
      }
      renderer.render(scene, camera);
    }
    raf = requestAnimationFrame(frame);

    window.addEventListener('pagehide', () => {
      cancelAnimationFrame(raf);
      renderer.dispose();
      geometry.dispose();
      material.dispose();
      texture.dispose();
      boxGeometry.dispose();
    }, { once: true });
  };
})();
