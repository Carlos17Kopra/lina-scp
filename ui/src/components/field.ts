/** Ids and `aria-describedby` for a form control with optional hint and error text. */
export function fieldDescription(inputId: string, hint?: string | null, error?: string | null) {
  const hintId = `${inputId}-hint`;
  const errorId = `${inputId}-error`;
  const ids = [hint ? hintId : null, error ? errorId : null].filter((id): id is string => id !== null);
  return { hintId, errorId, describedBy: ids.length > 0 ? ids.join(" ") : undefined };
}
