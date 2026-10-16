import { useState } from "react";
import { Button, Checkbox, Dialog, TextField } from "../../components";
import { t } from "../../i18n/de";
import type { ChmodReport, Entry, ModeChange } from "../../ipc/api";
import { isCoreError } from "../../ipc/errors";
import { formatMtime, formatSize } from "./entries";
import styles from "./PermissionsDialog.module.css";

type Bit = boolean | "mixed";

/** Index 0 = owner read (0o400) … 8 = others execute (0o001). */
const BITS = Array.from({ length: 9 }, (_, i) => 1 << (8 - i));

function initialBits(entries: readonly Entry[]): Bit[] {
  return BITS.map((bit) => {
    const set = entries.map((e) => (e.mode === null ? null : (e.mode & bit) !== 0));
    if (set.some((s) => s === null)) return "mixed";
    return set.every(Boolean) ? true : set.some(Boolean) ? "mixed" : false;
  });
}

function octalOf(bits: readonly Bit[]): string {
  if (bits.includes("mixed")) return "";
  const value = BITS.reduce((sum, bit, i) => (bits[i] === true ? sum | bit : sum), 0);
  return `0${value.toString(8).padStart(3, "0")}`;
}

function changeOf(bits: readonly Bit[]): ModeChange {
  const mask = (want: boolean) => BITS.reduce((sum, bit, i) => (bits[i] === want ? sum | bit : sum), 0);
  return { set: mask(true), clear: mask(false) };
}

const reasonOf = (error: unknown) => (isCoreError(error) && "message" in error ? error.message : t.sftp.failed);

export function PermissionsDialog({ entries, onSubmit, onClose }: {
  entries: readonly Entry[];
  onSubmit: (change: ModeChange, recursive: boolean) => Promise<ChmodReport>;
  onClose: () => void;
}) {
  const texts = t.sftp.permissions;
  const [initial] = useState(() => initialBits(entries));
  const [bits, setBits] = useState(initial);
  const [octal, setOctal] = useState(() => octalOf(initial));
  const [octalError, setOctalError] = useState(false);
  const [recursive, setRecursive] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const single = entries.length === 1 ? entries[0]! : null;
  const hasDir = entries.some((e) => e.kind.type === "dir");
  const dirty = recursive || bits.some((b, i) => b !== initial[i]);

  const toggle = (i: number) => {
    const next = bits.map((b, j) => (j === i ? b !== true : b));
    setBits(next);
    setOctal(octalOf(next));
    setOctalError(false);
  };
  const typeOctal = (text: string) => {
    setOctal(text);
    const valid = /^0?[0-7]{3}$/.test(text);
    setOctalError(text !== "" && !valid);
    if (valid) {
      const value = parseInt(text, 8);
      setBits(BITS.map((bit) => (value & bit) !== 0));
    }
  };
  const submit = async () => {
    setBusy(true);
    setError(null);
    try {
      const report = await onSubmit(changeOf(bits), recursive && hasDir);
      if (report.failed === 0 || !report.first) onClose();
      else setError(texts.failed(report.failed, report.first.path, reasonOf(report.first.error)));
    } catch (e) {
      setError(reasonOf(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog
      open
      size="sm"
      title={texts.title}
      onClose={onClose}
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>{t.common.cancel}</Button>
          <Button variant="primary" disabled={busy || !dirty || octalError} onClick={() => void submit()}>
            {texts.apply}
          </Button>
        </>
      }
    >
      <dl className={styles.facts}>
        <dt>{t.sftp.columns.name}</dt>
        <dd>{single ? single.name : texts.count(entries.length)}</dd>
        {single ? (
          <>
            <dt>{texts.kind}</dt>
            <dd>{texts.kinds[single.kind.type]}</dd>
            {single.kind.type === "dir" ? null : (
              <>
                <dt>{t.sftp.columns.size}</dt>
                <dd>{formatSize(single.size)}</dd>
              </>
            )}
            {single.owner ? (
              <>
                <dt>{t.sftp.columns.owner}</dt>
                <dd>{single.owner}</dd>
              </>
            ) : null}
            {single.mtime !== null ? (
              <>
                <dt>{t.sftp.columns.mtime}</dt>
                <dd>{formatMtime(single.mtime)}</dd>
              </>
            ) : null}
          </>
        ) : null}
      </dl>
      <table className={styles.grid} aria-label={texts.rights}>
        <thead>
          <tr>
            <td />
            {texts.what.map((what) => <th key={what} scope="col">{what}</th>)}
          </tr>
        </thead>
        <tbody>
          {texts.who.map((who, row) => (
            <tr key={who}>
              <th scope="row">{who}</th>
              {texts.what.map((what, col) => {
                const i = row * 3 + col;
                return (
                  <td key={what}>
                    <input
                      type="checkbox"
                      aria-label={`${who}: ${what}`}
                      checked={bits[i] === true}
                      ref={(el) => {
                        if (el) el.indeterminate = bits[i] === "mixed";
                      }}
                      onChange={() => toggle(i)}
                    />
                  </td>
                );
              })}
            </tr>
          ))}
        </tbody>
      </table>
      <TextField
        label={texts.octal}
        mono
        inputMode="numeric"
        spellCheck={false}
        value={octal}
        error={octalError ? texts.octalInvalid : undefined}
        onChange={typeOctal}
      />
      {hasDir ? (
        <Checkbox label={texts.recursive} hint={texts.recursiveHint} checked={recursive} onChange={setRecursive} />
      ) : null}
      {error ? <p role="alert" className={styles.error}>{error}</p> : null}
    </Dialog>
  );
}
