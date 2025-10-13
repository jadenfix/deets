import type { PropsWithChildren, ReactNode } from "react";

export interface CardProps {
  title: string;
  subtitle?: ReactNode;
  action?: ReactNode;
}

export function Card({ title, subtitle, action, children }: PropsWithChildren<CardProps>) {
  return (
    <section
      aria-label={typeof title === "string" ? title : undefined}
      style={{
        border: "1px solid #232932",
        borderRadius: "12px",
        padding: "16px",
        background: "#0b111c",
        display: "flex",
        flexDirection: "column",
        gap: "12px"
      }}
    >
      <header style={{ display: "flex", justifyContent: "space-between", alignItems: "center" }}>
        <div>
          <h2 style={{ margin: 0, color: "#e5ecff" }}>{title}</h2>
          {subtitle && (
            <p style={{ margin: "4px 0 0", color: "#8aa0c8", fontSize: "0.85rem" }}>{subtitle}</p>
          )}
        </div>
        {action}
      </header>
      <div>{children}</div>
    </section>
  );
}
