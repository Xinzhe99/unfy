import type { ButtonHTMLAttributes, ReactNode } from "react";

/**
 * Control primitives.
 *
 * Visual rules enforced here, so no page has to repeat them:
 *
 *  * only the primary button is filled; everything else is a hairline outline or
 *    plain text;
 *  * semantic colour is text or a dot, never a filled background — a UI full of
 *    amber and green panels reads as an alarm console;
 *  * one radius and one control height scale.
 */

type Variant = "primary" | "secondary" | "quiet" | "danger";
type Size = "sm" | "md";

const variantClass: Record<Variant, string> = {
  primary:
    "bg-[var(--color-accent)] text-[var(--color-accent-fg)] hover:bg-[var(--color-accent-hover)] disabled:bg-[var(--color-accent-soft)] disabled:text-[var(--color-muted)]",
  secondary:
    "border border-[var(--color-border-strong)] bg-[var(--color-surface)] text-[var(--color-fg)] hover:bg-[var(--color-surface-hover)] disabled:text-[var(--color-muted)]",
  quiet:
    "text-[var(--color-fg-soft)] hover:bg-[var(--color-surface-sunken)] disabled:text-[var(--color-muted)]",
  danger:
    // `disabled:` matters here as much as on the others: three destructive buttons
    // take `disabled={busy}`, and without this they kept their full red label while
    // silently ignoring clicks. Muted rather than faded, for the contrast reason
    // spelled out on the button below.
    "border border-[var(--color-border-strong)] text-[var(--color-danger)] hover:bg-[var(--color-surface-hover)] disabled:text-[var(--color-muted)]",
};

const sizeClass: Record<Size, string> = {
  sm: "h-8 px-2.5 text-xs gap-1.5",
  md: "h-8 px-3 text-[13px] gap-1.5",
};

export function Button({
  children,
  variant = "secondary",
  size = "md",
  loading,
  icon,
  ...rest
}: {
  children: ReactNode;
  variant?: Variant;
  size?: Size;
  loading?: boolean;
  icon?: ReactNode;
} & ButtonHTMLAttributes<HTMLButtonElement>) {
  return (
    <button
      type="button"
      {...rest}
      disabled={rest.disabled || loading}
      // A disabled button states a fact ("not yet"), it is not a disabled control
      // that vanished. Whole-element opacity made its label fade with the fill: at
      // 55 % the guide's "back" button on step one measured about 2.9:1 against a
      // dark surface, below the 4.5:1 floor, and read as missing rather than as
      // unavailable. The fill is dimmed instead, and the label keeps the muted
      // colour, which clears the floor in both themes.
      className={`inline-flex select-none items-center justify-center whitespace-nowrap rounded-[var(--radius-control)] font-medium transition-colors disabled:pointer-events-none ${sizeClass[size]} ${variantClass[variant]} ${rest.className ?? ""}`}
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
      className="inline-block size-3 animate-spin rounded-full border-[1.5px] border-current border-t-transparent opacity-60"
    />
  );
}

/** A white panel with a hairline border. The only container in the app. */
export function Card({
  title,
  description,
  children,
  aside,
  footer,
  padding = "normal",
}: {
  title?: string;
  description?: string;
  children?: ReactNode;
  aside?: ReactNode;
  footer?: ReactNode;
  padding?: "normal" | "none";
}) {
  return (
    <section className="overflow-hidden rounded-[var(--radius-card)] border border-[var(--color-border)] bg-[var(--color-surface)] shadow-[var(--shadow-card)]">
      {title ? (
        <header className="flex items-start justify-between gap-4 px-4 pt-3.5">
          <div className="min-w-0">
            <h2 className="text-[13px] font-semibold leading-5">{title}</h2>
            {description ? (
              <p className="mt-0.5 text-xs leading-relaxed text-[var(--color-muted)]">
                {description}
              </p>
            ) : null}
          </div>
          {aside ? <div className="shrink-0">{aside}</div> : null}
        </header>
      ) : null}
      <div className={padding === "none" ? "" : "px-4 pb-4 pt-3"}>
        {children}
      </div>
      {footer ? (
        <footer className="border-t border-[var(--color-border)] px-4 py-2.5">
          {footer}
        </footer>
      ) : null}
    </section>
  );
}

/** Label on the left, control on the right, hairline between rows. */
export function SettingRow({
  label,
  hint,
  children,
}: {
  label: string;
  hint?: string;
  children: ReactNode;
}) {
  return (
    <div className="flex items-start justify-between gap-6 py-3">
      <div className="min-w-0">
        {/* The row is not faded. Two rows use `disabled` for a long-lived reason -
            the automation switches while detection is off - and `opacity-50` on the
            row took the hint from 5.03:1 to 2.21:1, so the sentence explaining why
            the control is unavailable was the hardest thing on the page to read.
            The label keeps its colour, the hint keeps `muted`, and the control inside
            shows its own disabled state. */}
        <span className="block text-[13px] font-medium leading-5">{label}</span>
        {hint ? (
          <span className="mt-0.5 block text-xs leading-relaxed text-[var(--color-muted)]">
            {hint}
          </span>
        ) : null}
      </div>
      <div className="shrink-0 pt-0.5">{children}</div>
    </div>
  );
}

/** Switch. The knob is positioned with `left`, not a transform. */
export function Switch({
  checked,
  onChange,
  disabled,
  label,
}: {
  checked: boolean;
  onChange: (next: boolean) => void;
  disabled?: boolean;
  label: string;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      disabled={disabled}
      onClick={() => onChange(!checked)}
      className={`relative h-[18px] w-[32px] rounded-full transition-colors ${
        checked
          ? "bg-[var(--color-accent)]"
          : "bg-[var(--color-border-strong)]"
      }`}
    >
      <span
        aria-hidden
        className={`absolute top-[2px] size-[14px] rounded-full bg-white shadow-sm transition-[left] duration-150 ${
          checked ? "left-[16px]" : "left-[2px]"
        }`}
      />
    </button>
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
      onKeyDown={(event) => {
        if (disabled || !["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown", "Home", "End"].includes(event.key)) return;
        event.preventDefault();
        const index = Math.max(0, options.findIndex((option) => option.value === value));
        const nextIndex = event.key === "Home" ? 0 : event.key === "End" ? options.length - 1 : (index + (event.key === "ArrowLeft" || event.key === "ArrowUp" ? -1 : 1) + options.length) % options.length;
        const next = options[nextIndex];
        if (!next) return;
        onChange(next.value);
        event.currentTarget.querySelectorAll<HTMLButtonElement>("button")[nextIndex]?.focus();
      }}
      // Only the selected option changes when disabled: it drops from `fg` to
      // `muted`. Unselected options are muted already and stay as they are.
      //
      // Fading the whole group was the obvious move and the wrong one - muted text
      // on this surface is 5.44:1, and 60 % opacity drops it to 2.71:1, under the
      // floor. The border is already the faintest token in the palette, so there is
      // no dimmer-but-legal state to reach for. The result is that a disabled group
      // reads as "the current choice is inactive" rather than "the whole control is
      // unavailable"; `disabled` still blocks the click, and `busy` lasts about
      // 11 ms, so this is a smaller problem than an unreadable label would be.
      className="inline-flex rounded-[var(--radius-control)] border border-[var(--color-border)] bg-[var(--color-surface-sunken)] p-[2px]"
    >
      {options.map((option) => {
        const active = option.value === value;
        return (
          <button
            key={option.value}
            type="button"
            role="radio"
            aria-checked={active}
            tabIndex={active ? 0 : -1}
            disabled={disabled}
            onClick={() => onChange(option.value)}
            className={`rounded-[5px] px-2.5 py-1 text-xs font-medium transition-colors ${
              active
                ? `bg-[var(--color-surface)] shadow-sm ${
                    disabled ? "text-[var(--color-muted)]" : "text-[var(--color-fg)]"
                  }`
                : "text-[var(--color-muted)] hover:text-[var(--color-fg-soft)]"
            }`}
          >
            {option.label}
          </button>
        );
      })}
    </div>
  );
}

/**
 * How much colour a status dot carries.
 *
 * The dot is the only place semantic hue appears, so its value matters:
 *
 * * `success` - ready, nothing pending. Green, because "idle and healthy" is
 *   genuinely good news and a grey dot reads as disabled.
 * * `live` - windows are gathered right now. Blue, deliberately not the
 *   monochrome accent: an active layout is a state the user must recognise at a
 *   glance, and a near-black dot was indistinguishable from body text.
 * * `warning` / `danger` - something needs attention.
 * * `neutral` - genuinely unknown or inactive.
 */
export type StatusTone =
  | "neutral"
  | "accent"
  | "live"
  | "success"
  | "warning"
  | "danger";

/**
 * Status indicator: a coloured dot plus neutral text.
 *
 * Deliberately not a filled pill. The state is already named in words; the dot
 * only helps scanning, and a row of coloured blocks would compete with content.
 */
export function Status({
  tone = "neutral",
  children,
}: {
  tone?: StatusTone;
  children: ReactNode;
}) {
  const dot: Record<StatusTone, string> = {
    neutral: "bg-[var(--color-muted)]",
    accent: "bg-[var(--color-accent)]",
    live: "bg-[var(--color-active)]",
    success: "bg-[var(--color-success)]",
    warning: "bg-[var(--color-warning)]",
    danger: "bg-[var(--color-danger)]",
  };
  const text: Record<StatusTone, string> = {
    neutral: "text-[var(--color-muted)]",
    accent: "text-[var(--color-fg-soft)]",
    live: "text-[var(--color-fg)]",
    success: "text-[var(--color-fg-soft)]",
    warning: "text-[var(--color-fg-soft)]",
    danger: "text-[var(--color-danger)]",
  };
  return (
    <span
      className={`inline-flex items-center gap-1.5 text-xs font-medium ${text[tone]}`}
    >
      {/* 8 px rather than 6: at 6 the dot is legible in a zoomed screenshot but
          reads as a smudge at normal viewing size, which is how "ready" kept
          looking grey to a user even after it became green. */}
      <span aria-hidden className={`size-2 rounded-full ${dot[tone]}`} />
      {children}
    </span>
  );
}

/**
 * Notice: a hairline-bordered strip with an accent bar, not a tinted box.
 *
 * The left border carries the severity; the body stays on the normal surface, so
 * the text keeps its usual contrast.
 */
export function Notice({
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
  const bar: Record<string, string> = {
    neutral: "border-l-[var(--color-border-strong)]",
    accent: "border-l-[var(--color-accent)]",
    warning: "border-l-[var(--color-warning)]",
    danger: "border-l-[var(--color-danger)]",
  };
  return (
    <div
      role={tone === "danger" ? "alert" : "status"}
      className={`rounded-[var(--radius-control)] border border-[var(--color-border)] border-l-2 bg-[var(--color-surface)] px-3.5 py-2.5 ${bar[tone]}`}
    >
      <p className="text-[13px] font-medium leading-5">{title}</p>
      {children ? (
        <div className="mt-0.5 text-xs leading-relaxed text-[var(--color-fg-soft)]">
          {children}
        </div>
      ) : null}
      {actions ? (
        <div className="mt-2.5 flex flex-wrap gap-1.5">{actions}</div>
      ) : null}
    </div>
  );
}

export function EmptyState({
  title,
  children,
  action,
}: {
  title: string;
  children?: ReactNode;
  action?: ReactNode;
}) {
  return (
    <div className="px-4 py-10 text-center">
      <p className="text-[13px] font-medium text-[var(--color-fg-soft)]">
        {title}
      </p>
      {children ? (
        <p className="mx-auto mt-1 max-w-sm text-xs leading-relaxed text-[var(--color-muted)]">
          {children}
        </p>
      ) : null}
      {action ? <div className="mt-3 flex justify-center">{action}</div> : null}
    </div>
  );
}

/** Key/value line used in summaries. */
export function Detail({
  label,
  value,
  muted,
}: {
  label: string;
  value: ReactNode;
  muted?: boolean;
}) {
  return (
    <div className="flex min-w-0 items-baseline gap-2 py-1">
      <span className="shrink-0 text-xs text-[var(--color-muted)]">
        {label}
      </span>
      <span
        className={`min-w-0 flex-1 truncate text-right text-[13px] tabular-nums ${
          muted ? "text-[var(--color-muted)]" : ""
        }`}
      >
        {value}
      </span>
    </div>
  );
}
