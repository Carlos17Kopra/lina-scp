import { Plus, RefreshCw, Search, Zap } from "lucide-react";
import { type FormEvent, useEffect, useId, useRef, useState } from "react";
import { Button, cx, useToast } from "../../components";
import { t } from "../../i18n/de";
import { errorDetails } from "../../ipc/errors";
import { useHostsStore } from "../../stores/hosts";
import { useUiStore } from "../../stores/ui";
import styles from "./HostsToolbar.module.css";
import { parseQuickConnect } from "./quickConnect";
import { startQuickConnect } from "./useHostActions";

export function HostsToolbar() {
  const query = useHostsStore((state) => state.query);
  const setQuery = useHostsStore((state) => state.setQuery);
  const reloadSshConfig = useHostsStore((state) => state.reloadSshConfig);
  const searchFocusToken = useUiStore((state) => state.searchFocusToken);
  const openModal = useUiStore((state) => state.openModal);
  const toast = useToast();
  const searchRef = useRef<HTMLInputElement>(null);
  const [quick, setQuick] = useState("");
  const [quickInvalid, setQuickInvalid] = useState(false);
  const quickErrorId = useId();

  useEffect(() => {
    if (searchFocusToken === 0) return;
    searchRef.current?.focus();
    searchRef.current?.select();
  }, [searchFocusToken]);

  const submitQuick = async (event: FormEvent) => {
    event.preventDefault();
    const target = parseQuickConnect(quick);
    if (!target) {
      setQuickInvalid(true);
      return;
    }
    setQuickInvalid(false);
    try {
      await startQuickConnect(target);
      setQuick("");
    } catch (error) {
      toast.show({ tone: "red", message: t.shell.saveFailed, details: errorDetails(error) });
    }
  };

  const reload = () => {
    reloadSshConfig().then(
      () => toast.show({ tone: "green", message: t.hosts.sshConfigReloaded }),
      (error: unknown) => toast.show({ tone: "red", message: t.shell.loadFailed, details: errorDetails(error) }),
    );
  };

  return (
    <header className={styles.toolbar}>
      <div className={cx(styles.field, styles.search)}>
        <Search size={16} aria-hidden="true" />
        <input
          ref={searchRef}
          type="search"
          className={styles.input}
          aria-label={t.hosts.search}
          placeholder={t.hosts.search}
          value={query}
          onChange={(event) => setQuery(event.target.value)}
        />
        <kbd className={styles.kbd}>{t.hosts.searchShortcut}</kbd>
      </div>
      <form className={cx(styles.field, styles.quick, quickInvalid && styles.invalid)} onSubmit={submitQuick}>
        <Zap size={16} aria-hidden="true" />
        <input
          className={cx(styles.input, "mono")}
          aria-label={t.hosts.quickConnectLabel}
          placeholder={t.hosts.quickConnect}
          value={quick}
          onChange={(event) => {
            setQuick(event.target.value);
            setQuickInvalid(false);
          }}
          aria-invalid={quickInvalid ? true : undefined}
          aria-describedby={quickInvalid ? quickErrorId : undefined}
          spellCheck={false}
          autoCapitalize="off"
        />
        {quickInvalid ? (
          <span id={quickErrorId} className={styles.error}>
            {t.hosts.quickConnectInvalid}
          </span>
        ) : null}
      </form>
      <Button icon={RefreshCw} onClick={reload}>
        {t.hosts.reloadSshConfig}
      </Button>
      <Button
        variant="primary"
        icon={Plus}
        title={t.hosts.newHostTitle}
        onClick={() => openModal({ type: "editor", target: { kind: "host" } })}
      >
        {t.hosts.newHost}
      </Button>
    </header>
  );
}
