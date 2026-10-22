/* PolyPack hero effect in a worker + OffscreenCanvas.
   Keeps three.js parsing/WebGL setup off the main thread so UI responses stay fast. */
/* global THREE, importScripts */
importScripts('../vendor/three.min.js');

let renderer;
let scene;
let camera;
let points;
let group;
let boxes = [];
let visible = true;
let firstFrame = false;
let pointerX = 0;
let pointerY = 0;
let raf = 0;
let width = 1;
let height = 1;

function makeScene(canvas) {
  scene = new THREE.Scene();
  scene.fog = new THREE.FogExp2(0x07111f, 0.055);
  camera = new THREE.PerspectiveCamera(52, width / height, 0.1, 120);
  camera.position.set(0, 0, 17);

  try {
    renderer = new THREE.WebGLRenderer({
      canvas,
      antialias: true,
      alpha: true,
      powerPreference: 'low-power',
    });
  } catch (_) {
    return false;
  }
  renderer.setClearColor(0x000000, 0);
  renderer.setPixelRatio(Math.min(self.devicePixelRatio || 1, 1.75));

  const palette = [0x22d3ee, 0x34d399, 0xfde68a, 0x38bdf8];
  const compact = width < 720;
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
  const material = new THREE.PointsMaterial({
    size: compact ? 0.4 : 0.34,
    vertexColors: true,
    transparent: true,
    opacity: 1,
    depthWrite: false,
    blending: THREE.AdditiveBlending,
    sizeAttenuation: true,
  });
  points = new THREE.Points(geometry, material);
  scene.add(points);

  group = new THREE.Group();
  const boxGeometry = new THREE.BoxGeometry(1, 1, 1);
  boxes = [];
  for (let i = 0; i < 4; i += 1) {
    const edges = new THREE.LineSegments(
      new THREE.EdgesGeometry(boxGeometry),
      new THREE.LineBasicMaterial({
        color: palette[i % palette.length],
        transparent: true,
        opacity: 0.5,
      })
    );
    const scale = 1.1 + Math.random() * 2.1;
    edges.scale.setScalar(scale);
    edges.position.set((Math.random() - 0.5) * 20, (Math.random() - 0.5) * 12, -2 - Math.random() * 8);
    edges.userData.spin = 0.0015 + Math.random() * 0.004;
    group.add(edges);
    boxes.push(edges);
  }
  scene.add(group);
  resize(width, height);
  renderer.render(scene, camera);
  self.postMessage({ type: 'rendered', width, height, count: points.geometry.attributes.position.count });
  return true;
}

function resize(w, h, dpr) {
  if (!renderer) return;
  width = Math.max(1, w);
  height = Math.max(1, h);
  renderer.setPixelRatio(Math.min(dpr || self.devicePixelRatio || 1, 1.75));
  renderer.setSize(width, height, false);
  camera.aspect = width / height;
  camera.updateProjectionMatrix();
  points?.scale.setScalar(Math.max(1, width / 900));
}

function frame(t) {
  raf = requestAnimationFrame(frame);
  if (!visible || !renderer) return;
  points.rotation.y = t * 0.000035;
  points.rotation.x = Math.sin(t * 0.00002) * 0.06;
  group.rotation.y = pointerX * 0.08 + Math.sin(t * 0.00003) * 0.05;
  group.rotation.x = pointerY * 0.05;
  for (const box of boxes) {
    box.rotation.x += box.userData.spin;
    box.rotation.y += box.userData.spin * 0.7;
  }
  renderer.render(scene, camera);
  if (!firstFrame) {
    firstFrame = true;
    self.postMessage({ type: 'rendered', width, height, count: points.geometry.attributes.position.count });
  }
}

self.onmessage = (event) => {
  const msg = event.data || {};
  if (msg.type === 'init') {
    width = msg.width || 1;
    height = msg.height || 1;
    if (!makeScene(msg.canvas)) return self.postMessage({ type: 'unsupported' });
    raf = requestAnimationFrame(frame);
    self.postMessage({ type: 'ready' });
  } else if (msg.type === 'resize') {
    resize(msg.width, msg.height, msg.dpr);
  } else if (msg.type === 'pointer') {
    pointerX = msg.x || 0;
    pointerY = msg.y || 0;
  } else if (msg.type === 'visible') {
    visible = Boolean(msg.visible);
  } else if (msg.type === 'stop') {
    cancelAnimationFrame(raf);
    renderer?.dispose();
    self.close();
  }
};
