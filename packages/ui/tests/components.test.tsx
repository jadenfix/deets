import "@testing-library/jest-dom";
import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import React from "react";

import { Card, Metric, Section } from "../src/index.js";

describe("@aether/ui primitives", () => {
  it("renders a metric with hint", () => {
    render(<Metric label="Finality" value="950 ms" hint="p95" />);
    expect(screen.getByText("Finality")).toBeInTheDocument();
    expect(screen.getByText("950 ms")).toBeInTheDocument();
  });

  it("composes card and section", () => {
    render(
      <Card title="Chain Status" subtitle="Phase 7">
        <Section title="Performance">
          <Metric label="TPS" value="12,450" />
        </Section>
      </Card>
    );

    expect(screen.getByRole("heading", { name: "Chain Status" })).toBeInTheDocument();
    expect(screen.getByText("Phase 7")).toBeInTheDocument();
    expect(screen.getByText("TPS")).toBeInTheDocument();
  });
});
