// SPDX-License-Identifier: AGPL-3.0-only

/**
 * i18n — every visible string is a translation key (SPEC §1.4: the product is
 * language-agnostic by construction; only French ships in v0). Paying 1 % now
 * beats 20 % later.
 */

/** The French string table. Keys are stable; values are localized. */
export const FR = {
  "app.title": "Fluence",
  "connect.title": "Connexion à Fluence",
  "connect.tokenLabel": "Jeton de l'appareil",
  "connect.submit": "Se connecter",
  "connect.hint": "Collez un jeton « control » (appairez via fluencectl ou l'écran principal).",
  "connect.error": "Connexion impossible. Vérifiez le jeton et le hub.",
  "compose.speak": "PARLER",
  "compose.space": "Espace",
  "compose.backspace": "Effacer",
  "compose.clear": "Tout effacer",
  "compose.emergency": "Urgence",
  "compose.emergencyConfirm": "Confirmer l'urgence",
  "compose.emergencyCancel": "Annuler",
  "compose.gaze": "Regard",
  "compose.gazeOn": "Regard activé",
  "compose.calibrate": "Calibrer",
  "compose.record": "Enregistrer le regard",
  "compose.draftPlaceholder": "Composez votre message…",
  "suggest.slotEmpty": "—",
  "banner.emergencyActive": "⚠ URGENCE déclenchée",
  "banner.emergencyCleared": "Urgence levée",
  "banner.emergencyFailed": "Échec de l'envoi de l'urgence — réessayez",
  "status.connected": "Connecté",
  "status.reconnecting": "Reconnexion…",
  "status.degraded": "Mode dégradé",
  "metrics.wpm": "Mots/min",
  "metrics.ks": "Éco. frappe",
  "connect.careTitle": "Espace aidant",
  "connect.careHint": "Collez un jeton « care » (appairez via fluencectl ou l'écran principal).",
  "care.title": "Espace aidant",
  "care.health": "Santé du système",
  "care.noWorkers": "Aucun composant supervisé",
  "care.devices": "Appareils appairés",
  "care.noDevices": "Aucun appareil appairé",
  "care.revoke": "Révoquer",
  "care.revokeConfirm": "Confirmer la révocation",
  "care.cancel": "Annuler",
  "care.revoked": "révoqué",
  "care.journal": "Journal d'accès",
  "care.noJournal": "Aucune entrée",
  "care.revokeFailed": "La révocation a échoué — vérifiez le hub puis rechargez.",
} as const;

/** A valid translation key. */
export type StringKey = keyof typeof FR;

const DICTIONARY: Record<StringKey, string> = FR;

/** Resolves a translation key to its localized string. */
export function t(key: StringKey): string {
  return DICTIONARY[key];
}
