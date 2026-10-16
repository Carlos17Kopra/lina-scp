import { useState } from "react";
import { Button, Dialog, Select } from "../../components";
import { t } from "../../i18n/de";
import { type KeyInfo, keyCopyId } from "../../ipc/api";
import { useHostsStore } from "../../stores/hosts";
import { errorTitle } from "../connect/steps";
import { ConnectionTestPanel } from "../editor/ConnectionTest";
import { useConnectionTest } from "../editor/useConnectionTest";
import styles from "./Keychain.module.css";

/** "Auf Host eintragen" (ssh-copy-id) with the host's own login; prompts show inline like "Verbindung testen". */
export function CopyIdDialog({ keyInfo, onClose }: { keyInfo: KeyInfo; onClose: () => void }) {
  const texts = t.keychain.copyIdDialog;
  const hosts = useHostsStore((state) => state.hosts);
  const [hostId, setHostId] = useState(hosts[0]?.id ?? "");
  const { state, run, answer, cancel } = useConnectionTest();
  const host = hosts.find((h) => h.id === hostId);
  const running = state.status === "running";

  const close = () => {
    cancel();
    onClose();
  };
  const start = () => {
    if (host) void run(host, null, (connectId, events) => keyCopyId(connectId, host.id, keyInfo.publicKey, events));
  };

  return (
    <Dialog
      open
      size="md"
      title={texts.title(keyInfo.name)}
      onClose={close}
      footer={
        <>
          <Button variant="ghost" onClick={close}>
            {t.common.close}
          </Button>
          <Button variant="primary" disabled={running || !host} onClick={start}>
            {texts.submit}
          </Button>
        </>
      }
    >
      <div className={styles.form}>
        <p className={styles.hint}>{texts.intro}</p>
        <Select label={texts.host} disabled={running} value={hostId} options={hosts.map((h) => ({ value: h.id, label: h.name }))} onChange={setHostId} />
        <ConnectionTestPanel state={state} onAnswer={answer} />
        {state.status === "ok" ? <p className={styles.ok}>{state.result === false ? texts.present : texts.added}</p> : null}
        {state.status === "failed" && state.error ? (
          <p role="alert" className={styles.error}>
            {texts.failed("message" in state.error ? state.error.message : errorTitle(state.error))}
          </p>
        ) : null}
      </div>
    </Dialog>
  );
}
