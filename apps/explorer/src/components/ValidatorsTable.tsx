import type { ValidatorInfo } from "../data/mock.js";

interface Props {
  validators: ValidatorInfo[];
}

export function ValidatorsTable({ validators }: Props) {
  return (
    <table style={{ width: "100%", borderCollapse: "collapse" }}>
      <thead>
        <tr style={{ textAlign: "left", color: "#8aa0c8", fontSize: "0.8rem" }}>
          <th style={{ padding: "6px 0" }}>Validator</th>
          <th style={{ padding: "6px 0" }}>Identity</th>
          <th style={{ padding: "6px 0" }}>Perf.</th>
          <th style={{ padding: "6px 0" }}>Stake</th>
        </tr>
      </thead>
      <tbody>
        {validators.map((val) => (
          <tr key={val.identity} style={{ color: "#e5ecff" }}>
            <td style={{ padding: "6px 0" }}>{val.name}</td>
            <td style={{ padding: "6px 0", fontFamily: "monospace" }}>{val.identity}</td>
            <td style={{ padding: "6px 0" }}>{val.performanceScore.toFixed(1)}%</td>
            <td style={{ padding: "6px 0" }}>{val.stake.toLocaleString()} SWR</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}
