// Static, offline-only asset: no network calls, no external services.
const status = document.getElementById("status");
const ping = document.getElementById("ping");
let count = 0;

ping.addEventListener("click", () => {
  count += 1;
  status.textContent = `Pong #${count} (offline, no external services).`;
});
