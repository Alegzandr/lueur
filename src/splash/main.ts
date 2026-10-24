// Startup update check, shown before the app starts (see updater.rs).
//
// A page of its own rather than part of the app: it must paint at once and
// does not load the config, so its few strings live here. The shell passes
// `?lang=` and `?theme=` when the settings force them; otherwise the system's
// are used.
// Outside Tauri (`npm run dev`, then /splash.html) it loops through the stages.

import { invoke } from "@tauri-apps/api/core";
import "../styles/app.css";
import "./splash.css";

type Stage = "checking" | "downloading" | "installing" | "restarting";
interface Progress {
  stage: Stage;
  percent: number | null;
}

const POLL_MS = 100;

const strings = {
  en: {
    checking: "Checking for updates…",
    downloading: "Downloading update…",
    downloadingPercent: "Downloading update, {percent}%",
    installing: "Installing update…",
    restarting: "Restarting…",
    download: "Download",
  },
  fr: {
    checking: "Recherche de mises à jour…",
    downloading: "Téléchargement de la mise à jour…",
    downloadingPercent: "Téléchargement de la mise à jour, {percent} %",
    installing: "Installation de la mise à jour…",
    restarting: "Redémarrage…",
    download: "Téléchargement",
  },
};

const params = new URLSearchParams(location.search);
const forced = params.get("lang");
const lang = forced === "en" || forced === "fr" ? forced : navigator.language.toLowerCase().startsWith("fr") ? "fr" : "en";
const s = strings[lang];
document.documentElement.lang = lang;
const theme = params.get("theme");
if (theme === "light" || theme === "dark") document.documentElement.setAttribute("data-theme", theme);

const inTauri = "__TAURI_INTERNALS__" in window;
const stage = document.getElementById("stage")!;
const bar = document.getElementById("bar")!;
const fill = bar.firstElementChild as HTMLElement;
bar.setAttribute("aria-label", s.download);
stage.textContent = s.checking;

async function poll() {
  let progress: Progress;
  try {
    progress = inTauri ? await invoke<Progress>("update_progress") : demo();
  } catch {
    return;
  }
  const percent = progress.stage === "downloading" ? progress.percent : null;
  const text = percent != null ? s.downloadingPercent.replace("{percent}", String(percent)) : s[progress.stage] ?? s.checking;
  if (stage.textContent !== text) stage.textContent = text;
  bar.hidden = percent == null;
  if (percent != null) {
    bar.setAttribute("aria-valuenow", String(percent));
    fill.style.transform = `scaleX(${percent / 100})`;
  }
}

/** Browser preview: loops through the stages. */
function demo(): Progress {
  const t = (performance.now() / 1000) % 8;
  if (t < 2) return { stage: "checking", percent: null };
  if (t < 6) return { stage: "downloading", percent: Math.round(((t - 2) / 4) * 100) };
  if (t < 7) return { stage: "installing", percent: null };
  return { stage: "restarting", percent: null };
}

document.addEventListener("contextmenu", (e) => e.preventDefault());

void poll();
setInterval(poll, POLL_MS);
