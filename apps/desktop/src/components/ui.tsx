import type { ButtonHTMLAttributes, ReactNode } from "react";

/**
 * Control primitives.
 *
 * Each one exists because the same shape appears in at least three places; none
 * of them is a one-line wrapper around a DOM element. Variants are exhaustive so
 * a new tone cannot be invented ad hoc and break the palette.
 */

type Tone = "accent" | "neutral" | "quiet" | "danger";
type Size = "sm" | "md";

const toneClass: Record<Tone, string> = {
  accent:
    "bg-[var(--color-accent)] text-[var(--color-accent-fg)] hover:bg-[var(--color-accent-hover)] shadow-sm",
  neutral:
    "bg-[var(--color-surface-2)] text-[var(--color-fg)] border border-[var(--color-border)] hover:bg-[var(--color-surface-3)]",
  quiet:
    "text-[var(--color-muted)] hover:text-[var(--color-fg)] hover:bg-[var(--color-surface-2)]",
  danger:
    "border border-[var(--color-danger)]/35 bg-[var(--color-danger-soft)] text-[var(--color-danger)] hover:border-[var(--color-danger)]/60",
};

const sizeClass: Record<Size, string> = {
  sm: "h-8 px-2.5 text-xs gap-1.5 rounded-[var(--radius-control)]",
  md: "h-9 px-3.5 text-[13px] gap-2 rounded-[var(--radius-control)]",
};

export function Button({
  children,
  tone = "neutral",
  size = "md",
  loading,
  icon,
  ...rest
}: {
  children: ReactNode;
  tone?: Tone;
  size?: Size;
  loading?: boolean;
  icon?: ReactNode;
} & ButtonHTMLAttributes<HTMLButtonElement>) {
  return (
    <button
      type="button"
      {...rest}
      disabled={rest.disabled || loading}
      className={`inline-flex select-none items-center justify-center font-medium transition-colors disabled:pointer-events-none disabled:opacity-45 ${sizeClass[size]} ${toneClass[tone]} ${rest.className ?? ""}`}
    >
      {loading ? <Spinner /> : icon}
      {children}
    </button>
  );
}

export function Spinner() {
  return (
    <span
      aria-hidden
      className="inline-block size-3.5 animate-spin rounded-full border-2 border-current border-t-transparent opacity-70"
    />
  );
}

export function Card({
  title,
  description,
  children,
  aside,
  padded = true,
}: {
  title?: string;
  description?: string;
  children?: ReactNode;
  aside?: ReactNode;
  padded?: boolean;
}) {
  return (
    <section className="overflow-hidden rounded-[var(--radius-card)] border border-[var(--color-border)] bg-[var(--color-surface-2)] shadow-[var(--shadow-card)]">
      {title ? (
        <header className="flex items-start justify-between gap-4 px-5 pt-4">
          <div className="min-w-0">
            <h2 className="text-[13px] font-semibold tracking-tight">{title}</h2>
            {description ? (
              <p className="mt-1 text-xs leading-relaxed text-[var(--color-muted)]">
                {description}
              </p>
            ) : null}
          </div>
          {aside ? <div className="shrink-0">{aside}</div> : null}
        </header>
      ) : null}
      <div className={padded ? "px-5 pb-5 pt-3" : ""}>{children}</div>
    </section>
  );
}

/** A labelled switch. The whole row is clickable, not just the knob. */
export function Toggle({
  checked,
  onChange,
  disabled,
  label,
  hint,
}: {
  checked: boolean;
  onChange: (next: boolean) => void;
  disabled?: boolean;
  label: string;
  hint?: string;
}) {
  return (
    <label
      className={`flex cursor-pointer items-start justify-between gap-6 py-3 ${disabled ? "cursor-not-allowed opacity-60" : ""}`}
    >
      <span className="min-w-0">
        <span className="block text-[13px] font-medium">{label}</span>
        {hint ? (
          <span className="mt-0.5 block text-xs leading-relaxed text-[var(--color-muted)]">
            {hint}
          </span>
        ) : null}
      </span>
      <button
        type="button"
        role="switch"
        aria-checked={checked}
        aria-label={label}
        disabled={disabled}
        onClick={() => onChange(!checked)}
        className={`relative mt-0.5 h-5 w-9 shrink-0 rounded-full transition-colors ${
          checked ? "bg-[var(--color-accent)]" : "bg-[var(--color-border-strong)]"
        }`}
      >
        {/*
         * Positioned with `left`, not `translate`: an absolutely positioned span
         * with no `left` keeps its static position, and adding a translate on top
         * of that pushed the knob outside the track entirely.
         * Track is 36px, knob 16px, so on = 36 - 16 - 2.
         */}
        <span
          aria-hidden
          className={`absolute top-0.5 size-4 rounded-full bg-white shadow transition-[left] duration-150 ${
            checked ? "left-[18px]" : "left-0.5"
          }`}
        />
      </button>
    </label>
  );
}

/** Segmented control for 2–4 mutually exclusive options. */
export function Segmented<T extends string>({
  value,
  options,
  onChange,
  disabled,
  label,
}: {
  value: T;
  options: { value: T; label: string }[];
  onChange: (next: T) => void;
  disabled?: boolean;
  label: string;
}) {
  return (
    <div
      role="radiogroup"
      aria-label={label}
      className="inline-flex rounded-[var(--radius-control)] border border-[var(--color-border)] bg-[var(--color-surface)] p-0.5"
    >
      {options.map((option) => {
        const active = option.value === value;
        return (
          <button
            key={option.value}
            type="button"
            role="radio"
            aria-checked={active}
            disabled={disabled}
            onClick={() => onChange(option.value)}
            className={`rounded-[7px] px-3 py-1.5 text-xs font-medium transition-colors ${
              active
                ? "bg-[var(--color-surface)] text-[var(--color-accent)] shadow-sm ring-1 ring-[var(--color-accent)]/25"
                : "text-[var(--color-muted)] hover:text-[var(--color-fg)]"
            }`}
          >
            {option.label}
          </button>
        );
      })}
    </div>
  );
}

export function Badge({
  tone = "neutral",
  children,
  dot,
}: {
  tone?: "neutral" | "accent" | "success" | "warning" | "danger";
  children: ReactNode;
  dot?: boolean;
}) {
  const tones: Record<string, string> = {
    neutral:
      "bg-[var(--color-surface-3)] text-[var(--color-fg-soft)] border-transparent",
    accent:
      "bg-[var(--color-accent-soft)] text-[var(--color-accent)] border-transparent",
    success:
      "bg-[var(--color-success-soft)] text-[var(--color-success)] border-transparent",
    warning:
      "bg-[var(--color-warning-soft)] text-[var(--color-warning)] border-transparent",
    danger:
      "bg-[var(--color-danger-soft)] text-[var(--color-danger)] border-transparent",
  };
  return (
    <span
      className={`inline-flex items-center gap-1.5 rounded-full border px-2 py-0.5 text-[11px] font-medium ${tones[tone]}`}
    >
      {dot ? (
        <span
          aria-hidden
          className="inline-block size-1.5 rounded-full bg-current"
        />
      ) : null}
      {children}
    </span>
  );
}

export function Callout({
  tone = "neutral",
  title,
  children,
  actions,
}: {
  tone?: "neutral" | "warning" | "danger" | "accent";
  title: string;
  children?: ReactNode;
  actions?: ReactNode;
}) {
  const tones: Record<string, string> = {
    neutral:
      "border-[var(--color-border)] bg-[var(--color-surface-2)] text-[var(--color-fg)]",
    accent:
      "border-[var(--color-accent)]/30 bg-[var(--color-accent-soft)] text-[var(--color-fg)]",
    warning:
      "border-[var(--color-warning)]/35 bg-[var(--color-warning-soft)] text-[var(--color-fg)]",
    danger:
      "border-[var(--color-danger)]/35 bg-[var(--color-danger-soft)] text-[var(--color-fg)]",
  };
  return (
    <div
      role={tone === "danger" ? "alert" : "status"}
      className={`rounded-[var(--radius-control)] border px-4 py-3 ${tones[tone]}`}
    >
      <p className="text-[13px] font-medium">{title}</p>
      {children ? (
        <div className="mt-1 text-xs leading-relaxed text-[var(--color-fg-soft)]">
          {children}
        </div>
      ) : null}
      {actions ? (
        <div className="mt-3 flex flex-wrap gap-2">{actions}</div>
      ) : null}
    </div>
  );
}

export function EmptyState({
  title,
  children,
}: {
  title: string;
  children?: ReactNode;
}) {
  return (
    <div className="rounded-[var(--radius-control)] border border-dashed border-[var(--color-border-strong)] px-4 py-8 text-center">
      <p className="text-[13px] font-medium text-[var(--color-fg-soft)]">
        {title}
      </p>
      {children ? (
        <p className="mx-auto mt-1 max-w-sm text-xs leading-relaxed text-[var(--color-muted)]">
          {children}
        </p>
      ) : null}
    </div>
  );
}
