export interface MetricProps {
  label: string;
  value: string;
  hint?: string;
}

export function Metric({ label, value, hint }: MetricProps) {
  return (
    <div style={{ display: "flex", flexDirection: "column" }}>
      <span style={{ fontSize: "0.75rem", color: "#8aa0c8" }}>{label}</span>
      <strong style={{ fontSize: "1.4rem", color: "#f8fbff" }}>{value}</strong>
      {hint && <span style={{ fontSize: "0.7rem", color: "#6c819f" }}>{hint}</span>}
    </div>
  );
}
