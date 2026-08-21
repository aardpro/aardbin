/* aardbin — JavaScript only for what the browser must own (PRD §28):
   clipboard, file drag&drop, SSE/EventSource, relative times, toasts. */
(() => {
  "use strict";

  // ---------- toasts ----------
  function toast(msg, kind) {
    const box = document.getElementById("toast-container");
    if (!box) return;
    const el = document.createElement("div");
    el.textContent = msg;
    el.className =
      "pointer-events-auto rounded-md px-3 py-1.5 text-sm shadow-md " +
      (kind === "error" ? "bg-red-600 text-white" : "bg-neutral-900 text-white");
    box.appendChild(el);
    setTimeout(() => el.remove(), 2500);
  }

  // ---------- copy (PRD §15) ----------
  document.addEventListener("click", async (e) => {
    const btn = e.target.closest("[data-copy]");
    if (!btn) return;
    try {
      const resp = await fetch(`/records/${btn.dataset.copy}/copy`, {
        credentials: "same-origin",
      });
      if (!resp.ok) throw new Error(`HTTP ${resp.status}`);
      const text = await resp.text();
      await navigator.clipboard.writeText(text);
      toast("Copied");
    } catch {
      toast("Copy failed", "error");
    }
  });

  // ---------- SSE real-time sync (PRD §18–§20) ----------
  // Events carry no payload; any message → reload the list region.
  // EventSource reconnects automatically on network loss.
  const region = document.getElementById("records-region");
  if (region) {
    const es = new EventSource("/events");
    let timer = null;
    es.addEventListener("data_changed", () => {
      clearTimeout(timer);
      timer = setTimeout(() => {
        if (document.getElementById("records-region")) {
          document.body.dispatchEvent(new Event("aardbin:refresh"));
        }
      }, 300);
    });
  }

  // ---------- relative times (PRD §12) ----------
  function pad(n) {
    return String(n).padStart(2, "0");
  }
  function relativeText(tsSec, nowMs) {
    const diff = Math.max(0, nowMs / 1000 - tsSec);
    if (diff < 45) return "just now";
    const min = Math.floor(diff / 60);
    if (min < 2) return "1 minute ago";
    if (min < 60) return `${min} minutes ago`;
    const hr = Math.floor(min / 60);
    if (hr < 2) return "1 hour ago";
    if (hr < 24) return `${hr} hours ago`;
    const d = new Date(tsSec * 1000);
    const now = new Date(nowMs);
    const yesterday = new Date(nowMs - 86400000);
    if (
      d.getFullYear() === yesterday.getFullYear() &&
      d.getMonth() === yesterday.getMonth() &&
      d.getDate() === yesterday.getDate()
    ) {
      return `yesterday ${pad(d.getHours())}:${pad(d.getMinutes())}`;
    }
    if (d.getFullYear() === now.getFullYear()) {
      return `${d.getMonth() + 1}/${d.getDate()}`;
    }
    return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
  }
  function renderTimes(root) {
    const nowMs = Date.now();
    root.querySelectorAll("time[data-ts]").forEach((el) => {
      const ts = Number(el.dataset.ts);
      if (!Number.isFinite(ts)) return;
      el.textContent = relativeText(ts, nowMs);
      const d = new Date(ts * 1000);
      el.title = `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ` +
        `${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`;
    });
  }
  renderTimes(document);
  document.body.addEventListener("htmx:afterSwap", (e) => renderTimes(e.target));
  setInterval(() => renderTimes(document), 30000);

  // ---------- drag & drop upload (PRD §26) ----------
  const dz = document.getElementById("dropzone");
  const input = document.getElementById("file-input");
  if (dz && input) {
    const form = dz.closest("form");
    const maxBytes = Number(form?.dataset.maxAttachmentBytes || 0);

    function humanSize(bytes) {
      if (bytes >= 1048576) return (bytes / 1048576).toFixed(1) + " MB";
      if (bytes >= 1024) return Math.round(bytes / 1024) + " KB";
      return bytes + " B";
    }
    function renderPending() {
      const ul = document.getElementById("pending-files");
      if (!ul) return;
      ul.innerHTML = "";
      for (const f of input.files) {
        const li = document.createElement("li");
        li.textContent = `📎 ${f.name} · ${humanSize(f.size)}`;
        li.className =
          "rounded-md border border-neutral-200 bg-neutral-50 px-2.5 py-1.5 text-sm text-neutral-600";
        ul.appendChild(li);
      }
    }
    function addFiles(fileList) {
      const dt = new DataTransfer();
      for (const f of input.files) dt.items.add(f);
      for (const f of fileList) {
        if (maxBytes && f.size > maxBytes) {
          toast(`"${f.name}" exceeds ${humanSize(maxBytes)} — skipped`, "error");
          continue;
        }
        dt.items.add(f);
      }
      input.files = dt.files;
      renderPending();
    }

    dz.addEventListener("click", () => input.click());
    ["dragover", "dragenter"].forEach((ev) =>
      dz.addEventListener(ev, (e) => {
        e.preventDefault();
        dz.classList.add("border-neutral-500", "bg-neutral-100");
      })
    );
    ["dragleave", "drop"].forEach((ev) =>
      dz.addEventListener(ev, (e) => {
        e.preventDefault();
        dz.classList.remove("border-neutral-500", "bg-neutral-100");
      })
    );
    dz.addEventListener("drop", (e) => addFiles(e.dataTransfer.files));
    input.addEventListener("change", renderPending);
  }
})();
