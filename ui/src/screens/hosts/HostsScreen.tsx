import { t } from "../../i18n/de";
import { ScreenPlaceholder } from "../../shell/ScreenPlaceholder";

/** Stub — replaced by the host overview in Task 19 (must keep rendering an h1 "Server-Übersicht" or equivalent heading). */
export function HostsScreen() {
  return <ScreenPlaceholder title={t.stubs.hosts} />;
}
