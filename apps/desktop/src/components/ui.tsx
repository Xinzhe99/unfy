import type { ReactNode } from "react";

export function Button({
  children,
  onClick,
  variant = "secondary",
  disabled,
  title,
}: {
  children: ReactNode;
  onClick?: () => void;
  variant?: "primary" | "secondary" | "danger" | "ghost";
  disabled?: boolean;
  title?: string;
}) {
  const base =
    "inline-flex items-center justify-center gap-2 rounded-lg px-3.5 py-2 text-sm font-medium transition-colors disabled:cursor-not-allowed disabled:opacity-50";
  const styles: Record<string, string> = {
    primary:
      "bg-[var(--color-accent)] text-white hover:brightness-110 active:brightness-95",
    secondary:
      "border border-[var(--color-border)] bg-[var(--color-surface-2)] text-[var(--color-fg)] hover:bg-[var(--color-surface)]",
    danger:
      "border border-[var(--color-danger)]/40 bg-[var(--color-danger)]/10 text-[var(--color-danger)] hover:bg-[var(--color-danger)]/15",
    ghost: "text-[var(--color-muted)] hover:text-[var(--color-fg)]",
  };
  return (
    <button
      type="button"
      className={`${base} ${styles[variant]}`}
      onClick={onClick}
      disabled={disabled}
      title={title}
    >
      {children}
    </button>
  );
}

export function Card({
  title,
  description,
  children,
  actions,
}: {
  title: string;
  description?: string;
  children?: ReactNode;
  actions?: ReactNode;
}) {
  return (
    <section className="rounded-xl border border-[var(--color-border)] bg-[var(--color-surface-2)] p-5">
      <div className="flex items-start justify-between gap-4">
        <div>
          <h2 className="text-sm font-semibold">{title}</h2>
          {description ? (
            <p className="mt-1 text-xs leading-relaxed text-[var(--color-muted)]">
              {description}
            </p>
          ) : null}
        </div>
        {actions ? <div className="flex shrink-0 gap-2">{actions}</div> : null}
      </div>
      {children ? <div className="mt-4">{children}</div> : null}
    </section>
  );
}

export function StatusPill({
  tone,
  children,
}: {
  tone: "neutral" | "accent" | "success" | "warning" | "danger";
  children: ReactNode;
}) {
  const tones: Record<string, string> = {
    neutral: "bg-[var(--color-surface)] text-[var(--color-muted)] border-[var(--color-border)]",
    accent: "bg-[var(--color-accent-soft)] text-[var(--color-accent)] border-transparent",
    success: "bg-[var(--color-success)]/12 text-[var(--color-success)] border-transparent",
    warning: "bg-[var(--color-warning)]/14 text-[var(--color-warning)] border-transparent",
    danger: "bg-[var(--color-danger)]/12 text-[var(--color-danger)] border-transparent",
  };
  return (
    <span
      className={`inline-flex items-center gap-1.5 rounded-full border px-2.5 py-1 text-xs font-medium ${tones[tone]}`}
    >
      {children}
    </span>
  );
}

export function Field({
  label,
  hint,
  children,
}: {
  label: string;
  hint?: string;
  children: ReactNode;
}) {
  return (
    <label className="block">
      <span className="text-xs font-medium text-[var(--color-fg)]">{label}</span>
      {hint ? (
        <span className="mt-0.5 block text-xs text-[var(--color-muted)]">{hint}</span>
      ) : null}
      <div className="mt-1.5">{children}</div>
    </label>
  );
}

export function Notice({
  tone = "neutral",
  title,
  children,
  actions,
}: {
  tone?: "neutral" | "warning" | "danger";
  title: string;
  children?: ReactNode;
  actions?: ReactNode;
}) {
  const tones: Record<string, string> = {
    neutral: "border-[var(--color-border)] bg-[var(--color-surface)]",
    warning:
      "border-[var(--color-warning)]/40 bg-[var(--color-warning)]/8",
    danger: "border-[var(--color-danger)]/40 bg-[var(--color-danger)]/8",
  };
  return (
    <div className={`rounded-lg border p-4 ${tones[tone]}`}>
      <p className="text-sm font-medium">{title}</p>
      {children ? (
        <div className="mt-1 text-xs leading-relaxed text-[var(--color-muted)]">
          {children}
        </div>
      ) : null}
      {actions ? <div className="mt-3 flex gap-2">{actions}</div> : null}
    </div>
  );
}
