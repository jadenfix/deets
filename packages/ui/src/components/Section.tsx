import type { PropsWithChildren } from "react";

export interface SectionProps {
  title: string;
}

export function Section({ title, children }: PropsWithChildren<SectionProps>) {
  return (
    <section style={{ display: "flex", flexDirection: "column", gap: "16px" }}>
      <h3 style={{ color: "#bfcce8", margin: 0 }}>{title}</h3>
      {children}
    </section>
  );
}
