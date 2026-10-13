// Interface strings in English and French.
//
// `t(key)` returns the string for the current language; `{name}` placeholders
// are filled from `vars`. Counted strings are [one, other] pairs read with
// `tn(key, n)`: French treats 0 and 1 as singular, English only 1.
//
// Copy rule: say it in as few words as the control needs. Explanations go in
// tooltips (`*_tip`), never in the layout.

import { app } from "./state.svelte";

const NB = " "; // French puts a non-breaking space before : ? ! and %

const en = {
  nav_lighting: "Lighting",
  nav_room: "Room",
  nav_bulbs: "Bulbs",
  nav_settings: "Settings",
  state_on: "On",
  state_off: "Off",
  state_no_answer: "No bulb answering",
  state_none: "No bulbs yet",
  start: "Start",
  stop: "Stop",
  wait: "One moment",
  unbound_badge: "Lights without a bulb",

  setup_title: "Getting started",
  step1_title: "Add your bulbs",
  step1_btn: "Search",
  step2_title: "Place them in the room",
  step2_btn: "Open the plan",
  step3_title: "Start the ambilight",
  response: "Response",
  feel_instant: "Instant",
  feel_balanced: "Balanced",
  feel_calm: "Calm",
  feel_instant_tip: "Follows every cut. For fast games.",
  feel_balanced_tip: "Quick, without flicker.",
  feel_calm_tip: "Slow, soft changes. For films.",
  brightness: "Brightness",
  intensity: "Colour",
  intensity_tip: "How far colours are pushed beyond the screen's.",
  lights: "Lights",
  all_on: "All on",
  all_off: "All off",
  all_off_tip: "Also stops the ambilight",
  follows: "Follows the screen",
  no_bulb: "No bulb",
  assign: "Assign",
  turn_on: "Turn on",
  turn_off: "Turn off",
  show_on_plan: "Show on the plan",

  map_whole: "Whole picture",
  map_broad: "Broad region",
  map_left: "Left edge",
  map_right: "Right edge",
  map_slice: "Facing slice",

  add_light: "Add light",
  add_corner: "Add corner",
  reset_shape: "Reset shape",
  reset_done: "Shape reset.",
  coach: "Drag each light to where it stands, and You to your seat.",
  coach_done: "Done",
  plan_help: "Double-click a wall to add a corner",
  plan_you: "You",
  plan_screen: "Screen",
  seat_title: "Your seat. Drag to turn the screen.",
  resize_title: "Drag to resize",
  all_objects: "All objects",
  objects: "Objects",
  light_name: "Light name",
  mirrors: "What it mirrors",
  height: "Height",
  h_floor: "Floor",
  h_low: "Low",
  h_level: "Mid-height",
  h_high: "High",
  h_ceiling: "Ceiling",
  fact_brightness: "Brightness",
  fact_response: "Response",
  facts_tip: "Further from the screen, light is softer and slower, like real bounced light.",
  identify: "Identify",
  identify_tip: "Flash it warm white",
  remove_light: "Remove light",
  facing: "Facing",
  width: "Width",
  corner_n: "Corner {n}",
  remove_corner: "Remove corner",
  min_corners: "A room needs three corners.",
  no_lights: "No lights yet.",
  restart_text: "The room changed since the ambilight started.",
  apply: "Apply",

  search: "Search",
  searching: "Searching",
  search_again: "Search again",
  on_network: "On your network",
  listening: "Listening for bulbs…",
  none_title: "No bulbs answered",
  first_title: "No bulbs yet",
  first_text: "Turn on LAN Control for each bulb in the Yeelight app, then search.",
  no_music: "cannot follow the screen",
  use_for: "Use for…",
  new_light: "A new light",
  add: "Add",
  add_all: "Add all",
  in_room: "In the room",
  unassign: "Unassign",
  unassign_tip: "Keep the light, forget the bulb",
  pick_above: "No bulb",
  tips_title: "A bulb is missing?",
  tip_1: "Turn on LAN Control for the bulb in the Yeelight app.",
  tip_2: "Use the same network as this computer, not a guest network.",
  tip_3: "Allow Lueur through the firewall (UDP 1982).",
  tip_4: "Supported: Yeelight Color.",
  light_n: "Light {n}",

  picture: "Picture",
  display: "Display",
  display_main: "main",
  main_display: "Main display",
  fps: "Frame rate",
  fps_tip: "Higher is snappier and costs more CPU. 40 to 60 suits most bulbs.",
  sampling: "Sampling",
  sampling_tip: "Reading fewer pixels saves CPU and rarely shows.",
  sampling_all: "Every pixel",
  sampling_n: "1 in {n}",
  colour: "Colour",
  smoothing: "Smoothing",
  smoothing_tip: "Lower is calmer, higher reacts at once.",
  white_grey: "White on grey scenes",
  white_grey_tip: "Below this much colour, bulbs show a true white instead of a pinkish guess.",
  off: "Off",
  warmth: "White warmth",
  dark_scenes: "Dark cutoff",
  dark_scenes_tip: "Below this level, bulbs dim right down instead of glowing muddy.",
  transitions: "Transitions",
  fade: "Fade",
  fade_tip: "Off jumps instantly: lowest latency, can look stepped.",
  fade_len: "Fade length",
  application: "Application",
  appearance: "Appearance",
  automatic: "Auto",
  theme_light: "Light",
  theme_dark: "Dark",
  language: "Language",
  open_login: "Open at login",
  start_tray: "Start hidden in the tray",
  autostart: "Start the ambilight on launch",
  keep_running: "Keep running when closed",
  reset_to: "Reset to {v}",

  dismiss: "Dismiss",
  undo: "Undo",
  added: "{name} added.",
  added_many: "{n} bulbs added.",
  removed: "{name} removed.",
  readdressed: "{name} found at a new address.",
  no_bulb_yet: "This light has no bulb yet.",
  started_silent: "Started, but no bulb answered.",
  partial_power: "{ok} of {total} answered. Not answering: {names}.",
  err_save: "Settings could not be saved.",
  err_load: "Settings could not be loaded.",
  err_displays: "Displays could not be listed.",

  // Errors from the backend, by code (see src-tauri/src/error.rs)
  e_internal: "Something went wrong.",
  e_no_monitors: "No display found.",
  e_monitor_missing: "Display {subject} is no longer connected. Pick another in Settings.",
  e_monitors_unavailable: "Displays cannot be read right now.",
  e_bulb_unreachable: "The bulb at {subject} is not answering.",
  e_light_unbound: "This light has no bulb yet.",
  e_no_bindings: "No bulbs added yet.",
  e_no_active_lights: "No light with a bulb follows the screen.",
  e_music_mode_failed: "No bulb answered: {subject}. Are they powered and is LAN Control on?",
  e_discovery_failed: "The network could not be searched.",
  e_invalid_ip: "Invalid address: {subject}.",
  e_login_item_failed: "Open at login could not be changed.",
  e_save_failed: "Settings could not be saved.",
};

type Key = keyof typeof en;

const fr: Record<Key, string> = {
  nav_lighting: "Éclairage",
  nav_room: "Pièce",
  nav_bulbs: "Ampoules",
  nav_settings: "Réglages",
  state_on: "Allumé",
  state_off: "Éteint",
  state_no_answer: "Aucune ampoule ne répond",
  state_none: "Aucune ampoule",
  start: "Démarrer",
  stop: "Arrêter",
  wait: "Un instant",
  unbound_badge: "Lumières sans ampoule",

  setup_title: "Pour commencer",
  step1_title: "Ajouter vos ampoules",
  step1_btn: "Rechercher",
  step2_title: "Les placer dans la pièce",
  step2_btn: "Ouvrir le plan",
  step3_title: "Lancer l'ambilight",
  response: "Réactivité",
  feel_instant: "Immédiate",
  feel_balanced: "Équilibrée",
  feel_calm: "Douce",
  feel_instant_tip: "Suit chaque changement de plan. Pour les jeux rapides.",
  feel_balanced_tip: "Rapide, sans scintillement.",
  feel_calm_tip: "Changements lents et doux. Pour les films.",
  brightness: "Luminosité",
  intensity: "Couleur",
  intensity_tip: "À quel point les couleurs sont renforcées par rapport à l'écran.",
  lights: "Lumières",
  all_on: "Tout allumer",
  all_off: "Tout éteindre",
  all_off_tip: "Arrête aussi l'ambilight",
  follows: "Suit l'écran",
  no_bulb: "Sans ampoule",
  assign: "Associer",
  turn_on: "Allumer",
  turn_off: "Éteindre",
  show_on_plan: "Voir sur le plan",

  map_whole: "Toute l'image",
  map_broad: "Large zone",
  map_left: "Bord gauche",
  map_right: "Bord droit",
  map_slice: "Zone en face",

  add_light: "Ajouter une lumière",
  add_corner: "Ajouter un coin",
  reset_shape: "Réinitialiser",
  reset_done: "Forme réinitialisée.",
  coach: "Placez chaque lumière où elle se trouve, et « Vous » à votre place.",
  coach_done: "Terminé",
  plan_help: "Double-clic sur un mur pour ajouter un coin",
  plan_you: "Vous",
  plan_screen: "Écran",
  seat_title: "Votre place. Faites glisser pour orienter l'écran.",
  resize_title: "Faites glisser pour redimensionner",
  all_objects: "Tous les éléments",
  objects: "Éléments",
  light_name: "Nom de la lumière",
  mirrors: "Ce qu'elle reprend",
  height: "Hauteur",
  h_floor: "Sol",
  h_low: "Basse",
  h_level: "Mi-hauteur",
  h_high: "Haute",
  h_ceiling: "Plafond",
  fact_brightness: "Luminosité",
  fact_response: "Réactivité",
  facts_tip: "Loin de l'écran, la lumière est plus douce et plus lente, comme un vrai reflet.",
  identify: "Identifier",
  identify_tip: "La faire clignoter en blanc chaud",
  remove_light: "Supprimer la lumière",
  facing: "Orientation",
  width: "Largeur",
  corner_n: "Coin {n}",
  remove_corner: "Supprimer le coin",
  min_corners: "Une pièce a besoin de trois coins.",
  no_lights: "Aucune lumière.",
  restart_text: "La pièce a changé depuis le lancement.",
  apply: "Appliquer",

  search: "Rechercher",
  searching: "Recherche",
  search_again: "Relancer",
  on_network: "Sur votre réseau",
  listening: "Recherche des ampoules…",
  none_title: "Aucune ampoule n'a répondu",
  first_title: "Aucune ampoule",
  first_text: "Activez le contrôle LAN de chaque ampoule dans l'app Yeelight, puis lancez la recherche.",
  no_music: "ne peut pas suivre l'écran",
  use_for: "Utiliser pour…",
  new_light: "Une nouvelle lumière",
  add: "Ajouter",
  add_all: "Tout ajouter",
  in_room: "Dans la pièce",
  unassign: "Dissocier",
  unassign_tip: "Garder la lumière, oublier l'ampoule",
  pick_above: "Sans ampoule",
  tips_title: `Une ampoule manque${NB}?`,
  tip_1: "Activez le contrôle LAN de l'ampoule dans l'app Yeelight.",
  tip_2: "Utilisez le même réseau que cet ordinateur, pas un réseau invité.",
  tip_3: "Autorisez Lueur dans le pare-feu (UDP 1982).",
  tip_4: `Compatibles${NB}: Yeelight Color.`,
  light_n: "Lumière {n}",

  picture: "Image",
  display: "Écran",
  display_main: "principal",
  main_display: "Écran principal",
  fps: "Fréquence",
  fps_tip: "Plus élevée, plus vive, mais plus de processeur. 40 à 60 conviennent à la plupart des ampoules.",
  sampling: "Échantillonnage",
  sampling_tip: "Lire moins de pixels économise le processeur, sans différence visible le plus souvent.",
  sampling_all: "Tous les pixels",
  sampling_n: "1 sur {n}",
  colour: "Couleur",
  smoothing: "Lissage",
  smoothing_tip: "Plus bas, plus calme. Plus haut, réaction immédiate.",
  white_grey: "Blanc sur scènes grises",
  white_grey_tip: "Sous ce niveau de couleur, les ampoules passent en vrai blanc au lieu d'une teinte rosée.",
  off: "Désactivé",
  warmth: "Température du blanc",
  dark_scenes: "Seuil du noir",
  dark_scenes_tip: "Sous ce niveau, les ampoules baissent au minimum au lieu d'une lueur terne.",
  transitions: "Transitions",
  fade: "Fondu",
  fade_tip: `Désactivé, le changement est instantané${NB}: latence minimale, rendu parfois saccadé.`,
  fade_len: "Durée du fondu",
  application: "Application",
  appearance: "Apparence",
  automatic: "Auto",
  theme_light: "Clair",
  theme_dark: "Sombre",
  language: "Langue",
  open_login: "Ouvrir à la connexion",
  start_tray: "Démarrer masqué",
  autostart: "Lancer l'ambilight au démarrage",
  keep_running: "Rester actif une fois fermé",
  reset_to: "Rétablir {v}",

  dismiss: "Fermer",
  undo: "Annuler",
  added: "{name} ajoutée.",
  added_many: "{n} ampoules ajoutées.",
  removed: "{name} supprimée.",
  readdressed: "{name} retrouvée à une nouvelle adresse.",
  no_bulb_yet: "Cette lumière n'a pas encore d'ampoule.",
  started_silent: "Démarré, mais aucune ampoule ne répond.",
  partial_power: `{ok} sur {total} ont répondu. Sans réponse${NB}: {names}.`,
  err_save: "Impossible d'enregistrer les réglages.",
  err_load: "Impossible de charger les réglages.",
  err_displays: "Impossible de lister les écrans.",

  e_internal: "Une erreur inattendue s'est produite.",
  e_no_monitors: "Aucun écran détecté.",
  e_monitor_missing: "L'écran {subject} n'est plus branché. Choisissez-en un autre dans les Réglages.",
  e_monitors_unavailable: "Impossible de lire les écrans pour le moment.",
  e_bulb_unreachable: "L'ampoule {subject} ne répond pas.",
  e_light_unbound: "Cette lumière n'a pas encore d'ampoule.",
  e_no_bindings: "Aucune ampoule ajoutée.",
  e_no_active_lights: "Aucune lumière avec ampoule ne suit l'écran.",
  e_music_mode_failed: `Aucune ampoule ne répond${NB}: {subject}. Sont-elles alimentées, avec le contrôle LAN activé${NB}?`,
  e_discovery_failed: "Impossible de rechercher sur le réseau.",
  e_invalid_ip: `Adresse invalide${NB}: {subject}.`,
  e_login_item_failed: "Impossible de modifier l'ouverture à la connexion.",
  e_save_failed: "Impossible d'enregistrer les réglages.",
};

const enCounts = {
  n_bulbs_following: ["{n} bulb following", "{n} bulbs following"],
  n_bulbs_ready: ["{n} bulb ready", "{n} bulbs ready"],
  n_found: ["{n} found", "{n} found"],
};
type CountKey = keyof typeof enCounts;
const frCounts: Record<CountKey, string[]> = {
  n_bulbs_following: ["{n} ampoule active", "{n} ampoules actives"],
  n_bulbs_ready: ["{n} ampoule prête", "{n} ampoules prêtes"],
  n_found: ["{n} trouvée", "{n} trouvées"],
};

export type Lang = "en" | "fr";

/** The language in use: the user's choice, or the system's when automatic. */
export function lang(): Lang {
  const pref = app.config.interface?.language ?? "system";
  if (pref === "en" || pref === "fr") return pref;
  return typeof navigator !== "undefined" && navigator.language.toLowerCase().startsWith("fr")
    ? "fr"
    : "en";
}

function fill(s: string, vars?: Record<string, string | number>): string {
  if (!vars) return s;
  return s.replace(/\{(\w+)\}/g, (m, k) => (k in vars ? String(vars[k]) : m));
}

export function t(key: Key, vars?: Record<string, string | number>): string {
  return fill((lang() === "fr" ? fr : en)[key], vars);
}

export function tn(key: CountKey, n: number): string {
  const forms = (lang() === "fr" ? frCounts : enCounts)[key];
  const one = lang() === "fr" ? n <= 1 : n === 1;
  return fill(one ? forms[0] : forms[1], { n });
}

/** "45 %" in French, "45%" in English. */
export function pct(v: number): string {
  const n = Math.round(v * 100);
  return lang() === "fr" ? `${n}${NB}%` : `${n}%`;
}

/** An error from the backend: a code from src-tauri/src/error.rs. */
interface BackendError {
  code: string;
  subject?: string;
  detail?: string;
}

function isBackendError(e: unknown): e is BackendError {
  return typeof e === "object" && e !== null && typeof (e as BackendError).code === "string";
}

/** Any thrown value as a sentence in the interface language. */
export function errorText(e: unknown): string {
  if (isBackendError(e)) {
    const key = `e_${e.code}` as Key;
    return key in en ? t(key, { subject: e.subject ?? "" }) : t("e_internal");
  }
  if (e instanceof Error) return e.message;
  return typeof e === "string" ? e : t("e_internal");
}

/** The untranslated technical cause, if the backend sent one. */
export function errorDetail(e: unknown): string | undefined {
  return isBackendError(e) ? e.detail : undefined;
}
