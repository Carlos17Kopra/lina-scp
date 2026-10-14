import { t } from "../../i18n/de";
import { ScreenPlaceholder } from "../../shell/ScreenPlaceholder";

/** SFTP view in TP1: the file manager arrives in TP3. */
export function SftpPlaceholder() {
  return <ScreenPlaceholder title={t.stubs.sftp} text={t.stubs.sftpText} />;
}
