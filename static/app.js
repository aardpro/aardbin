/* aardbin — JavaScript only for what the browser must own (SPEC §28):
   clipboard, file drag&drop, SSE/EventSource, relative times, toasts.
   Includes lightweight i18n table for client-side strings (E9). */
(() => {
  "use strict";

  // ---------- i18n ----------
  // Detect language from <html lang="…"> attribute set by the server.
  const LANG = (document.documentElement.lang || "en").startsWith("zh") ? "zh" : "en";
  const I18N = {
    en: {
      "js.copied": "Copied",
      "js.copy_failed": "Copy failed",
      "js.just_now": "just now",
      "js.minute_ago": "1 minute ago",
      "js.minutes_ago": (n) => `${n} minutes ago`,
      "js.hour_ago": "1 hour ago",
      "js.hours_ago": (n) => `${n} hours ago`,
      "js.yesterday": "yesterday",
      "js.exceeds_max": "exceeds max",
      "js.skipped": "skipped",
    },
    zh: {
      "js.copied": "已复制",
      "js.copy_failed": "复制失败",
      "js.just_now": "刚刚",
      "js.minute_ago": "1 分钟前",
      "js.minutes_ago": (n) => `${n} 分钟前`,
      "js.hour_ago": "1 小时前",
      "js.hours_ago": (n) => `${n} 小时前`,
      "js.yesterday": "昨天",
      "js.exceeds_max": "超过最大",
      "js.skipped": "已跳过",
    },
  };
  function t(key, ...args) {
    const val = I18N[LANG]?.[key] ?? I18N.en[key] ?? key;
    if (typeof val === "function") return val(...args);
    return val;
  }

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

  // ---------- copy (SPEC §15) ----------
  // Clicking [data-copy] (legacy button) or [data-copy-content] (content area)
  // fetches the record's plaintext and writes it to the clipboard.
  document.addEventListener("click", async (e) => {
    const btn = e.target.closest("[data-copy]") || e.target.closest("[data-copy-content]");
    if (!btn) return;
    const id = btn.dataset.copy || btn.dataset.copyContent;
    try {
      const resp = await fetch(`/records/${id}/copy`, {
        credentials: "same-origin",
      });
      if (!resp.ok) throw new Error(`HTTP ${resp.status}`);
      const text = await resp.text();
      await navigator.clipboard.writeText(text);
      toast(t("js.copied"));
    } catch {
      toast(t("js.copy_failed"), "error");
    }
  });

  // ---------- SSE real-time sync (SPEC §18–§20) ----------
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
    // After reconnect (network loss recovery), refresh the list to catch
    // any changes that happened while disconnected.
    es.onopen = () => {
      if (document.getElementById("records-region")) {
        document.body.dispatchEvent(new Event("aardbin:refresh"));
      }
    };
  }

  // ---------- relative times (SPEC §12) ----------
  function pad(n) {
    return String(n).padStart(2, "0");
  }
  function relativeText(tsSec, nowMs) {
    const diff = Math.max(0, nowMs / 1000 - tsSec);
    if (diff < 45) return t("js.just_now");
    const min = Math.floor(diff / 60);
    if (min < 2) return t("js.minute_ago");
    if (min < 60) return t("js.minutes_ago", min);
    const hr = Math.floor(min / 60);
    if (hr < 2) return t("js.hour_ago");
    if (hr < 24) return t("js.hours_ago", hr);
    const d = new Date(tsSec * 1000);
    const now = new Date(nowMs);
    const yesterday = new Date(nowMs - 86400000);
    if (
      d.getFullYear() === yesterday.getFullYear() &&
      d.getMonth() === yesterday.getMonth() &&
      d.getDate() === yesterday.getDate()
    ) {
      return `${t("js.yesterday")} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
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

  // ---------- drag & drop upload (SPEC §26) ----------
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
          toast(`"${f.name}" ${t("js.exceeds_max")} ${humanSize(maxBytes)} — ${t("js.skipped")}`, "error");
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
