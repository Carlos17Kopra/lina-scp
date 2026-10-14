import { ConnectScreen } from "../screens/connect/ConnectScreen";
import { EditorModal } from "../screens/editor/EditorModal";
import { ExportDialog } from "../screens/settings/ExportDialog";
import { FirstRunDialog } from "../screens/settings/FirstRunDialog";
import { ImportDialog } from "../screens/settings/ImportDialog";
import { SettingsDialog } from "../screens/settings/SettingsDialog";
import { useUiStore } from "../stores/ui";

/** Renders the single open modal (`useUiStore().modal`). */
export function ModalLayer() {
  const modal = useUiStore((state) => state.modal);
  if (!modal) return null;
  switch (modal.type) {
    case "editor":
      return <EditorModal target={modal.target} />;
    case "settings":
      return <SettingsDialog />;
    case "firstRun":
      return <FirstRunDialog />;
    case "export":
      return <ExportDialog />;
    case "import":
      return <ImportDialog />;
    case "connect":
      return <ConnectScreen />;
  }
}
