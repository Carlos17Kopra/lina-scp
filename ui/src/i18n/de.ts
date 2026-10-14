/**
 * All German UI strings, grouped by screen. Later tasks add one top-level group per screen
 * (e.g. `hosts`, `editor`, `connect`, `terminal`, `settings`). Never inline UI text in components.
 */
export const t = {
  app: {
    name: "LinaSCP",
  },
  common: {
    cancel: "Abbrechen",
    close: "Schließen",
    save: "Speichern",
    back: "Zurück",
    retry: "Erneut versuchen",
    copy: "Kopieren",
    copied: "Kopiert",
    delete: "Löschen",
    loading: "Wird geladen …",
    comingSoon: "Folgt in einer späteren Version",
  },
  rail: {
    nav: "Hauptnavigation",
    hosts: "Hosts",
    terminal: "Terminal",
    sftp: "SFTP",
    snippets: "Snippets",
    keychain: "Schlüsselbund",
    portForwarding: "Port-Forwarding",
    settings: "Einstellungen",
    toLight: "Helles Design",
    toDark: "Dunkles Design",
  },
  toast: {
    region: "Benachrichtigungen",
    copyDetails: "Details kopieren",
    dismiss: "Meldung schließen",
  },
  steps: {
    pending: "ausstehend",
    active: "läuft",
    done: "erledigt",
    failed: "fehlgeschlagen",
  },
  shell: {
    loadFailed: "Daten konnten nicht geladen werden",
    saveFailed: "Speichern fehlgeschlagen",
  },
  stubs: {
    hosts: "Server-Übersicht",
    terminal: "Terminal",
    sftp: "SFTP",
    sftpText: "Der SFTP-Dateimanager folgt in einer späteren Version.",
    editorHost: "Host bearbeiten",
    editorProfile: "Login-Profil bearbeiten",
    settings: "Einstellungen",
    firstRun: "Willkommen bei LinaSCP",
    export: "Exportieren",
    import: "Importieren",
    connect: "Verbindung",
  },
} as const;
