import { useCallback, useEffect, useState } from "react";
import { Button } from "../../components";
import { t } from "../../i18n/de";
import { type KeyInfo, keysList } from "../../ipc/api";
import { errorMessage } from "../../ipc/errors";
import { useHostsStore } from "../../stores/hosts";
import { pushNotice } from "../../stores/notices";
import { copyText } from "../terminal/clipboard";
import { CopyIdDialog } from "./CopyIdDialog";
import { GenerateKeyDialog } from "./GenerateKeyDialog";
import styles from "./Keychain.module.css";

/** Whether a stored key path names this file. */
// ponytail: `~/…` is matched by suffix (the UI does not know $HOME); another user's home would match too.
export function pointsTo(keyPath: string | null, path: string): boolean {
  if (!keyPath) return false;
  return keyPath === path || (keyPath.startsWith("~/") && path.endsWith(keyPath.slice(1)));
}

/** Rail screen "Schlüsselbund": the key files in ~/.ssh, generate new ones, enter them on hosts. */
export function KeychainScreen() {
  const texts = t.keychain;
  const hosts = useHostsStore((state) => state.hosts);
  const profiles = useHostsStore((state) => state.profiles);
  const [keys, setKeys] = useState<KeyInfo[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [generating, setGenerating] = useState(false);
  const [copying, setCopying] = useState<KeyInfo | null>(null);

  const load = useCallback(async () => {
    try {
      setKeys(await keysList());
      setError(null);
    } catch (e) {
      setError(errorMessage(e));
    }
  }, []);
  useEffect(() => {
    void load();
  }, [load]);

  const users = (key: KeyInfo) => [
    ...hosts.filter((h) => h.auth.kind === "custom" && pointsTo(h.auth.auth.keyPath, key.path)).map((h) => h.name),
    ...profiles.filter(({ profile }) => pointsTo(profile.auth.keyPath, key.path)).map(({ profile }) => texts.profile(profile.name)),
  ];
  const copy = async (key: KeyInfo) => {
    await copyText(key.publicKey);
    pushNotice({ tone: "green", message: texts.copied });
  };

  return (
    <div className={styles.screen}>
      <header className={styles.header}>
        <h1 className={styles.title}>{texts.title}</h1>
        <Button variant="primary" onClick={() => setGenerating(true)}>
          {texts.add}
        </Button>
      </header>
      {error ? (
        <p role="alert" className={styles.error}>
          {texts.loadFailed(error)}
        </p>
      ) : null}
      {keys && keys.length === 0 ? <p className={styles.empty}>{texts.empty}</p> : null}
      {keys && keys.length > 0 ? (
        <ul className={styles.cards}>
          {keys.map((key) => {
            const usedBy = users(key);
            return (
              <li key={key.path} className={styles.card}>
                <h2 className={styles.name}>{key.name}</h2>
                <span className={styles.meta}>{key.algorithm}</span>
                <span className={styles.mono}>{key.fingerprint}</span>
                {key.comment ? <span className={styles.meta}>{key.comment}</span> : null}
                <span className={styles.meta}>
                  {key.encrypted === null ? texts.pem : key.encrypted ? texts.encrypted : texts.plain}
                </span>
                <span className={styles.mono}>{key.path}</span>
                <span className={styles.meta}>{usedBy.length > 0 ? texts.usedBy(usedBy.join(", ")) : texts.unused}</span>
                <div className={styles.actions}>
                  <Button size="sm" onClick={() => void copy(key)}>
                    {texts.copy}
                  </Button>
                  <Button size="sm" onClick={() => setCopying(key)}>
                    {texts.copyId}
                  </Button>
                </div>
              </li>
            );
          })}
        </ul>
      ) : null}
      {generating ? (
        <GenerateKeyDialog
          onClose={() => setGenerating(false)}
          onGenerated={() => {
            setGenerating(false);
            void load();
          }}
        />
      ) : null}
      {copying ? <CopyIdDialog keyInfo={copying} onClose={() => setCopying(null)} /> : null}
    </div>
  );
}
