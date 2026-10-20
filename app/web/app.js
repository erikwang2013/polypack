// Static frontend for the PolyPack build-history shell. All history text is
// inserted with textContent (never innerHTML) so a hand-edited JSONL file
// cannot inject markup into the dashboard.
"use strict";

const invoke =
  window.__TAURI__ && window.__TAURI__.core
    ? window.__TAURI__.core.invoke
    : null;

const els = {
  form: document.getElementById("load-form"),
  root: document.getElementById("root"),
  target: document.getElementById("target"),
  limit: document.getElementById("limit"),
  error: document.getElementById("error"),
  sourcePath: document.getElementById("source-path"),
  skipped: document.getElementById("skipped"),
  matrix: document.getElementById("matrix-view"),
  matrixBody: document.getElementById("matrix-body"),
  matrixMeta: document.getElementById("matrix-meta"),
  detail: document.getElementById("detail-view"),
  detailBody: document.getElementById("detail-body"),
  detailTitle: document.getElementById("detail-title"),
  detailMeta: document.getElementById("detail-meta"),
  back: document.getElementById("back"),
  empty: document.getElementById("empty-state"),
  emptyTitle: document.getElementById("empty-title"),
  emptyCopy: document.getElementById("empty-copy"),
};

const state = {
  targets: [],
  skippedLines: 0,
  selected: null,
  targetFilter: "",
};

function text(tag, className, value) {
  const node = document.createElement(tag);
  if (className) node.className = className;
  node.textContent = value == null ? "" : String(value);
  return node;
}

function showError(message) {
  els.error.textContent = message;
  els.error.hidden = false;
}

function clearError() {
  els.error.textContent = "";
  els.error.hidden = true;
}

function statusBadge(status) {
  const label = status || "unknown";
  return text("span", `status status-${label}`, label);
}

function codeCell(value) {
  if (!value) return text("span", "muted", "-");
  return text("code", value === "<redacted>" ? "muted" : "", value);
}

function formatTime(ts) {
  const value = Number(ts);
  if (!Number.isFinite(value)) return String(ts);
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return String(ts);
  return date.toISOString().replace(".000Z", "Z");
}

function formatDuration(ms) {
  const value = Number(ms);
  if (!Number.isFinite(value)) return String(ms);
  return value < 1000 ? `${value}ms` : `${(value / 1000).toFixed(2)}s`;
}

function matrixRow(target) {
  const row = document.createElement("tr");
  row.dataset.target = target.target;
  row.setAttribute("data-testid", "target-row");

  const name = document.createElement("td");
  const open = text("button", "target", target.target);
  open.type = "button";
  open.addEventListener("click", (event) => {
    event.stopPropagation();
    openDetail(target.target);
  });
  name.append(open);
  row.append(name);

  const status = document.createElement("td");
  status.append(statusBadge(target.latest.status));
  row.append(status);
  row.append(text("td", "", formatTime(target.latest.ts)));
  row.append(text("td", "", formatDuration(target.latest.duration_ms)));

  const artifact = document.createElement("td");
  artifact.append(codeCell(target.latest.artifact));
  row.append(artifact);
  const error = document.createElement("td");
  error.append(codeCell(target.latest.error_code));
  row.append(error);

  row.addEventListener("click", () => openDetail(target.target));
  return row;
}

function detailRow(record) {
  const row = document.createElement("tr");
  row.setAttribute("data-testid", "detail-record");

  const build = document.createElement("td");
  build.append(codeCell(record.build_id));
  row.append(build);
  const status = document.createElement("td");
  status.append(statusBadge(record.status));
  row.append(status);
  row.append(text("td", "", formatTime(record.ts)));
  row.append(text("td", "", formatDuration(record.duration_ms)));

  const artifact = document.createElement("td");
  artifact.append(codeCell(record.artifact));
  row.append(artifact);
  const digest = document.createElement("td");
  digest.append(codeCell(record.artifact_digest));
  row.append(digest);
  const error = document.createElement("td");
  error.append(codeCell(record.error_code));
  row.append(error);
  return row;
}

function openDetail(target) {
  state.selected = target;
  render();
}

function render() {
  els.skipped.hidden = state.skippedLines === 0;
  els.skipped.textContent = `${state.skippedLines} malformed line(s) skipped`;

  const targets = state.targets;
  const shown = targets.reduce((sum, item) => sum + item.records.length, 0);
  els.matrixMeta.textContent = targets.length
    ? `${targets.length} target(s) · ${shown} record(s) shown`
    : "";

  if (!targets.length) {
    state.selected = null;
    els.matrix.hidden = true;
    els.detail.hidden = true;
    els.empty.hidden = false;
    if (state.targetFilter) {
      els.emptyTitle.textContent = `No history for target "${state.targetFilter}"`;
      els.emptyCopy.textContent =
        "Clear the target filter to see all built targets, or run a build for this target.";
    } else {
      els.emptyTitle.textContent = "No build history yet";
      els.emptyCopy.textContent =
        "Run a build in the project root, then refresh this view.";
    }
    return;
  }

  els.empty.hidden = true;
  const selected =
    state.selected && targets.find((item) => item.target === state.selected);
  els.matrix.hidden = Boolean(selected);
  els.detail.hidden = !selected;

  if (selected) {
    els.detailTitle.textContent = selected.target;
    els.detailMeta.textContent = `${selected.records.length} most recent record(s) · latest ${formatTime(
      selected.latest.ts,
    )}`;
    els.detailBody.replaceChildren(...selected.records.map(detailRow));
  } else {
    els.matrixBody.replaceChildren(...targets.map(matrixRow));
  }
}

async function load() {
  clearError();
  if (!invoke) {
    showError(
      "Tauri IPC is unavailable. Open this page through the PolyPack shell: " +
        "cargo run --manifest-path app/src-tauri/Cargo.toml",
    );
    els.matrix.hidden = true;
    els.detail.hidden = true;
    els.empty.hidden = true;
    return;
  }

  const limit = Math.min(200, Math.max(1, Number(els.limit.value) || 20));
  state.targetFilter = els.target.value.trim();
  try {
    const view = await invoke("history_records", {
      root: els.root.value.trim() || null,
      target: state.targetFilter || null,
      limit,
    });
    state.targets = Array.isArray(view.targets) ? view.targets : [];
    state.skippedLines = Number(view.skipped_lines) || 0;
    state.selected = null;
    if (view.source) els.sourcePath.textContent = view.source;
    render();
  } catch (error) {
    state.targets = [];
    state.skippedLines = 0;
    render();
    els.empty.hidden = true;
    showError(
      typeof error === "string" ? error : (error && error.message) || String(error),
    );
  }
}

els.form.addEventListener("submit", (event) => {
  event.preventDefault();
  load();
});

els.back.addEventListener("click", () => {
  state.selected = null;
  render();
});

load();
