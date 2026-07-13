import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import { App } from "../App";

describe("App", () => {
  it("renders shell ready", () => {
    render(<App />);
    expect(screen.getByText("shell ready")).toBeDefined();
  });

  it("renders kruon heading", () => {
    render(<App />);
    expect(screen.getByText("kruon")).toBeDefined();
  });

  it("renders alpha tag", () => {
    render(<App />);
    expect(screen.getByText("alpha")).toBeDefined();
  });
});
