import type { LucideIcon } from "lucide-react";
import type { ButtonHTMLAttributes, Ref } from "react";
import { cx } from "../cx";
import styles from "./IconButton.module.css";

export interface IconButtonProps extends Omit<ButtonHTMLAttributes<HTMLButtonElement>, "children"> {
  /** Accessible name and default tooltip. */
  label: string;
  icon: LucideIcon;
  size?: "sm" | "md" | "lg";
  ref?: Ref<HTMLButtonElement>;
}

const ICON_SIZE = { sm: 14, md: 16, lg: 20 } as const;

export function IconButton({ label, icon: Icon, size = "md", className, type = "button", ...rest }: IconButtonProps) {
  return (
    <button
      title={label}
      {...rest}
      type={type}
      aria-label={label}
      className={cx(styles.iconButton, styles[size], className)}
    >
      <Icon size={ICON_SIZE[size]} strokeWidth={2} aria-hidden="true" />
    </button>
  );
}
