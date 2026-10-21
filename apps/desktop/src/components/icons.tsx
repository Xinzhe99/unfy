import type { SVGProps } from "react";

/**
 * Inline icon set.
 *
 * Hand-drawn 16px strokes on a 24px grid, 1.5px wide, so they sit at the same
 * optical weight as the text. A component library would bring its own visual
 * language and a large dependency for six glyphs.
 */
type IconProps = SVGProps<SVGSVGElement>;

function Base({ children, ...rest }: IconProps) {
  return (
    <svg
      viewBox="0 0 24 24"
      width="16"
      height="16"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.5"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden
      {...rest}
    >
      {children}
    </svg>
  );
}

export function IconHome(props: IconProps) {
  return (
    <Base {...props}>
      <path d="M4 10.5 12 4l8 6.5" />
      <path d="M6 9.5V19a1 1 0 0 0 1 1h3v-5h4v5h3a1 1 0 0 0 1-1V9.5" />
    </Base>
  );
}

export function IconDisplays(props: IconProps) {
  return (
    <Base {...props}>
      <rect x="2.5" y="5" width="12" height="9" rx="1.5" />
      <rect x="14.5" y="9.5" width="7" height="8" rx="1.5" />
      <path d="M8.5 17.5v2M15 20.5h5" />
    </Base>
  );
}

export function IconShield(props: IconProps) {
  return (
    <Base {...props}>
      <path d="M12 3.5 19 6v6c0 4-3 7-7 8.5C8 19 5 16 5 12V6l7-2.5Z" />
      <path d="M9.5 12l1.8 1.8L15 10" />
    </Base>
  );
}

export function IconSettings(props: IconProps) {
  return (
    <Base {...props}>
      <circle cx="12" cy="12" r="2.8" />
      <path d="M12 3.5v2M12 18.5v2M4.9 7.5l1.7 1M17.4 15.5l1.7 1M4.9 16.5l1.7-1M17.4 8.5l1.7-1" />
    </Base>
  );
}

export function IconRefresh(props: IconProps) {
  return (
    <Base {...props}>
      <path d="M20 12a8 8 0 1 1-2.6-5.9" />
      <path d="M20 4v4h-4" />
    </Base>
  );
}

export function IconLayout(props: IconProps) {
  return (
    <Base {...props}>
      <rect x="3.5" y="4.5" width="17" height="15" rx="1.5" />
      <path d="M3.5 12h17M11 12v7.5" />
    </Base>
  );
}

export function IconEye(props: IconProps) {
  return (
    <Base {...props}>
      <path d="M2.5 12S6 6.5 12 6.5 21.5 12 21.5 12 18 17.5 12 17.5 2.5 12 2.5 12Z" />
      <circle cx="12" cy="12" r="2.5" />
    </Base>
  );
}

export function IconDownload(props: IconProps) {
  return (
    <Base {...props}>
      <path d="M12 4v10M8 11l4 4 4-4" />
      <path d="M4.5 18.5h15" />
    </Base>
  );
}

export function IconInfo(props: IconProps) {
  return (
    <Base {...props}>
      <circle cx="12" cy="12" r="8.5" />
      <path d="M12 11v5M12 8h.01" />
    </Base>
  );
}

export function IconArrowRight(props: IconProps) {
  return (
    <Base {...props}>
      <path d="M5 12h13M13 7l5 5-5 5" />
    </Base>
  );
}

export function IconChevron(props: IconProps) {
  return (
    <Base {...props}>
      <path d="M9 6l6 6-6 6" />
    </Base>
  );
}
