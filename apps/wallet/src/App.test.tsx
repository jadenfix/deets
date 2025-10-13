import "@testing-library/jest-dom";
import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import React from "react";

import { App } from "./App.js";

describe("@aether/wallet", () => {
  it("renders transfer and job previews", () => {
    render(<App />);

    expect(screen.getByText("Wallet Overview")).toBeInTheDocument();
    expect(screen.getByText(/Phase 7 SDK wiring/)).toBeInTheDocument();
    expect(screen.getByText("Transfer Preview")).toBeInTheDocument();
    expect(screen.getByText("Job Submission")).toBeInTheDocument();
    expect(screen.getByText(/wallet-hello-job/)).toBeInTheDocument();
  });
});
