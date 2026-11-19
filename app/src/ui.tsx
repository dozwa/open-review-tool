// Bausteine der Oberfläche: Icons, Kopfzeilen, Gruppen, Statuspillen, Kennzahlen.

import React from "react";

const ICONS: Record<string, React.ReactNode> = {
  stack: <><path d="m12 3 9 5-9 5-9-5 9-5Z" /><path d="m3 13 9 5 9-5" /></>,
  exchange: <><path d="M7 4 3 8l4 4" /><path d="M3 8h13" /><path d="m17 20 4-4-4-4" /><path d="M21 16H8" /></>,
  people: <><circle cx="9" cy="8" r="3.2" /><path d="M3 20c0-3.3 2.7-5.5 6-5.5s6 2.2 6 5.5" /><path d="M16 4.6a3.2 3.2 0 0 1 0 6.3" /><path d="M18 14.8c1.9.7 3 2.6 3 5.2" /></>,
  upload: <><path d="M12 15V4" /><path d="m7 9 5-5 5 5" /><path d="M5 20h14" /></>,
  download: <><path d="M12 4v11" /><path d="m7 10 5 5 5-5" /><path d="M5 20h14" /></>,
  list: <><path d="M9 6h11" /><path d="M9 12h11" /><path d="M9 18h11" /><circle cx="4.5" cy="6" r="1" /><circle cx="4.5" cy="12" r="1" /><circle cx="4.5" cy="18" r="1" /></>,
  shield: <><path d="M12 3 5 6v5c0 4.4 3 8.3 7 10 4-1.7 7-5.6 7-10V6l-7-3Z" /><path d="m9 12 2 2 4-4" /></>,
  plus: <><path d="M12 5v14" /><path d="M5 12h14" /></>,
  search: <><circle cx="11" cy="11" r="6.5" /><path d="m16 16 4.5 4.5" /></>,
  back: <path d="m15 5-7 7 7 7" />,
  chart: <><path d="M4 20V10" /><path d="M10 20V4" /><path d="M16 20v-7" /><path d="M21 20H3" /></>,
  play: <path d="M8 5v14l11-7L8 5Z" />,
  lock: <><rect x="5" y="11" width="14" height="9" rx="2" /><path d="M8 11V8a4 4 0 0 1 8 0v3" /></>,
  eye: <><path d="M2.5 12S6 5.5 12 5.5 21.5 12 21.5 12 18 18.5 12 18.5 2.5 12 2.5 12Z" /><circle cx="12" cy="12" r="2.8" /></>,
  flag: <><path d="M5 21V4" /><path d="M5 4h11l-2 4 2 4H5" /></>,
  key: <><circle cx="8" cy="15" r="4" /><path d="m11 12 9-9" /><path d="m17 6 3 3" /></>,
  inbox: <><path d="M4 13 6.5 5h11L20 13" /><path d="M4 13v6h16v-6h-5l-1.5 2h-3L9 13H4Z" /></>,
  file: <><path d="M14 3H6v18h12V7l-4-4Z" /><path d="M14 3v4h4" /></>,
  check: <path d="m5 12 4.5 4.5L19 7" />,
  x: <><path d="M6 6l12 12" /><path d="M18 6 6 18" /></>,
  help: <><circle cx="12" cy="12" r="9" /><path d="M9.5 9.5a2.5 2.5 0 1 1 3.5 2.3c-.6.3-1 .9-1 1.6v.6" /><circle cx="12" cy="17" r=".6" /></>,
  csv: <><path d="M14 3H6v18h12V7l-4-4Z" /><path d="M9 13h6" /><path d="M9 17h6" /></>,
};

export function Icon({ name, size = 16 }: { name: keyof typeof ICONS | string; size?: number }) {
  return (
    <svg
      className="icon"
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={1.8}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      {ICONS[name]}
    </svg>
  );
}

export function PageHeader({ title, subtitle, children, back }: {
  title: string;
  subtitle?: React.ReactNode;
  children?: React.ReactNode;
  back?: React.ReactNode;
}) {
  return (
    <header className="page-header">
      {back && <div className="back">{back}</div>}
      <div className="page-header-row">
        <div>
          <h1>{title}</h1>
          {subtitle && <p className="subtitle">{subtitle}</p>}
        </div>
        {children && <div className="toolbar">{children}</div>}
      </div>
    </header>
  );
}

export function Group({ title, footer, children, action }: {
  title?: string;
  footer?: React.ReactNode;
  children: React.ReactNode;
  action?: React.ReactNode;
}) {
  return (
    <section className="group">
      {(title || action) && (
        <div className="group-head">
          {title && <h2>{title}</h2>}
          {action}
        </div>
      )}
      <div className="group-body">{children}</div>
      {footer && <p className="group-foot">{footer}</p>}
    </section>
  );
}

export function Row({ children, className = "" }: { children: React.ReactNode; className?: string }) {
  return <div className={`row ${className}`}>{children}</div>;
}

const TONE: Record<string, string> = {
  verblindet: "violet", entblindet: "blue", offen: "green", geschlossen: "gray",
  owner: "blue", admin: "blue", reviewer: "green", viewer: "gray",
  fertig: "green", fehlgeschlagen: "red", läuft: "yellow",
};

export function Pill({ children, tone }: { children: React.ReactNode; tone?: string }) {
  const t = tone ?? TONE[String(children).toLowerCase()] ?? "gray";
  return <span className={`pill pill-${t}`}>{children}</span>;
}

export function Banner({ kind, children, onClose }: { kind: "error" | "notice"; children: React.ReactNode; onClose?: () => void }) {
  return (
    <div className={`banner banner-${kind}`} role={kind === "error" ? "alert" : "status"}>
      <Icon name={kind === "error" ? "x" : "check"} />
      <span>{children}</span>
      {onClose && (
        <button className="plain" onClick={onClose} aria-label="Schließen">
          <Icon name="x" size={14} />
        </button>
      )}
    </div>
  );
}

export function Empty({ icon, title, children }: { icon: string; title: string; children?: React.ReactNode }) {
  return (
    <div className="empty">
      <div className="empty-icon"><Icon name={icon} size={22} /></div>
      <strong>{title}</strong>
      {children && <div className="empty-text">{children}</div>}
    </div>
  );
}

export function Segmented<T extends string>({ value, options, onChange, label }: {
  value: T;
  options: { value: T; label: string; icon?: string }[];
  onChange: (value: T) => void;
  label: string;
}) {
  return (
    <div className="segmented" role="tablist" aria-label={label}>
      {options.map((o) => (
        <button
          key={o.value}
          role="tab"
          aria-selected={value === o.value}
          className={value === o.value ? "active" : ""}
          onClick={() => onChange(o.value)}
        >
          {o.icon && <Icon name={o.icon} size={14} />}
          {o.label}
        </button>
      ))}
    </div>
  );
}

export function Avatar({ name, size = 28 }: { name: string; size?: number }) {
  const initials = name.split(/\s+/).filter(Boolean).slice(0, 2).map((p) => p[0]?.toUpperCase()).join("") || "?";
  let hash = 0;
  for (const c of name) hash = (hash * 31 + c.charCodeAt(0)) % 360;
  return (
    <span className="avatar" style={{ width: size, height: size, fontSize: size * 0.4, background: `hsl(${hash} 45% 52%)` }}>
      {initials}
    </span>
  );
}

/** Verteilung der Entscheidungen. Reihenfolge und Farben sind mit dem Paletten-Validator geprüft
 *  (hell und dunkel, benachbarte Paare, Farbsehschwächen): positiv → unsicher → negativ, offen grau. */
export const DECISION_PARTS = [
  { key: "include", label: "Include", tone: "inc" },
  { key: "maybe", label: "Maybe", tone: "may" },
  { key: "needs_clarification", label: "Zur Klärung", tone: "cla" },
  { key: "disagreement", label: "Widerspruch", tone: "dis" },
  { key: "exclude", label: "Exclude", tone: "exc" },
  { key: "pending", label: "Offen", tone: "pen" },
] as const;

export function DecisionBar({ counts }: { counts: Record<string, number> }) {
  const total = DECISION_PARTS.reduce((s, p) => s + (counts[p.key] ?? 0), 0);
  const label = DECISION_PARTS.map((p) => `${p.label} ${counts[p.key] ?? 0}`).join(", ");
  return (
    <div className="decision-bar" role="img" aria-label={`Entscheidungen: ${label}`}>
      {total > 0 &&
        DECISION_PARTS.filter((p) => (counts[p.key] ?? 0) > 0).map((p) => {
          const n = counts[p.key] ?? 0;
          return (
            <span
              key={p.key}
              className={`seg seg-${p.tone}`}
              style={{ flexGrow: n }}
              title={`${p.label}: ${n} (${Math.round((n / total) * 100)} %)`}
            />
          );
        })}
    </div>
  );
}

export function Stat({ label, value, detail, tone }: { label: string; value: React.ReactNode; detail?: React.ReactNode; tone?: string }) {
  return (
    <div className="stat">
      <span className="stat-label">
        {tone && <span className={`dot dot-${tone}`} />}
        {label}
      </span>
      <span className="stat-value">{value}</span>
      {detail && <span className="stat-detail">{detail}</span>}
    </div>
  );
}

export function Kbd({ children }: { children: React.ReactNode }) {
  return <kbd>{children}</kbd>;
}
