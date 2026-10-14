import styles from "./ScreenPlaceholder.module.css";

export function ScreenPlaceholder({ title, text }: { title: string; text?: string }) {
  return (
    <div className={styles.placeholder}>
      <h1 className={styles.title}>{title}</h1>
      {text ? <p className={styles.text}>{text}</p> : null}
    </div>
  );
}
