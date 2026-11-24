import React from "react";
import { describe, expect, it } from "vitest";
import { render, screen } from "@testing-library/react";
import { SecurityStateBadge } from "@/components/SecurityStateBadge";

describe("SecurityStateBadge", () => {
  it("never shows completed state while loading", () => {
    render(<SecurityStateBadge isLoading={true} executionStatus="completed_with_rows" />);
    expect(screen.getByText(/Validation Pending \/ Execution Running/i)).toBeInTheDocument();
    expect(screen.queryByText(/Execution Completed/i)).not.toBeInTheDocument();
  });

  it("renders Validation Rejected explicitly on security rejection", () => {
    render(
      <SecurityStateBadge
        validationStatus="validation_rejected"
        executionStatus="validation_rejected"
      />
    );
    expect(screen.getByText("Validation Rejected")).toBeInTheDocument();
    expect(screen.queryByText("Execution Completed")).not.toBeInTheDocument();
  });

  it("distinguishes Empty Result and Results Truncated states", () => {
    const { rerender } = render(
      <SecurityStateBadge
        validationStatus="validation_passed"
        executionStatus="completed_empty"
      />
    );
    expect(screen.getByText("Execution Completed (0 Rows)")).toBeInTheDocument();

    rerender(
      <SecurityStateBadge
        validationStatus="validation_passed"
        executionStatus="completed_truncated"
      />
    );
    expect(screen.getByText("Results Truncated")).toBeInTheDocument();
  });
});
