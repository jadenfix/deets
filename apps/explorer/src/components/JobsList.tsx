import type { JobInfo } from "../data/mock.js";

interface Props {
  jobs: JobInfo[];
}

const badgeColors: Record<JobInfo["status"], string> = {
  pending: "#d4a72c",
  running: "#2cb1a3",
  settled: "#8aa0c8"
};

export function JobsList({ jobs }: Props) {
  return (
    <ul style={{ listStyle: "none", padding: 0, margin: 0, display: "flex", flexDirection: "column", gap: "8px" }}>
      {jobs.map((job) => (
        <li key={job.id} style={{ display: "flex", justifyContent: "space-between", alignItems: "center", color: "#e5ecff" }}>
          <div>
            <strong>{job.id}</strong>
            <span style={{ marginLeft: 8, color: "#8aa0c8" }}>{job.model}</span>
            <span style={{ marginLeft: 8, color: "#6c819f" }}>Provider: {job.provider}</span>
          </div>
          <div style={{ display: "flex", alignItems: "center", gap: 12 }}>
            <span style={{ color: "#bfcce8", fontSize: "0.85rem" }}>{job.maxFee.toLocaleString()} AIC</span>
            <span
              style={{
                background: badgeColors[job.status],
                color: "#0b111c",
                borderRadius: "999px",
                padding: "4px 10px",
                fontSize: "0.75rem",
                fontWeight: 600,
                textTransform: "uppercase"
              }}
            >
              {job.status}
            </span>
          </div>
        </li>
      ))}
    </ul>
  );
}
