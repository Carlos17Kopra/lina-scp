import { t } from "../../../i18n/de";
import { errorDetails } from "../../../ipc/errors";
import { pushNotice } from "../../../stores/notices";
import { useSettingsStore } from "../../../stores/settings";

/** Opens/closes the terminal sidebar and persists the state; a failed save becomes a notice. */
export function toggleTerminalSidebar(): void {
  const { settings, save } = useSettingsStore.getState();
  save({ terminalSidebarOpen: !settings.terminalSidebarOpen }).catch((error: unknown) =>
    pushNotice({ tone: "red", message: t.shell.saveFailed, details: errorDetails(error) }),
  );
}
