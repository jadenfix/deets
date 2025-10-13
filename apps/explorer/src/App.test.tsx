import "@testing-library/jest-dom";
import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import React from "react";

import { App } from "./App.js";

describe("@aether/explorer", () => {
  it("renders chain overview metrics and validators", () => {
    render(<App />);

    expect(screen.getByText("Network Overview")).toBeInTheDocument();
    expect(screen.getByText("12,450 TPS")).toBeInTheDocument();
    expect(screen.getByText("Validator Performance")).toBeInTheDocument();
    expect(screen.getByText("Atlas One")).toBeInTheDocument();
    expect(screen.getByText("Active AI Jobs")).toBeInTheDocument();
  });
});
