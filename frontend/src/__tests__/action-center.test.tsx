import React from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import ActionCenterPage from "@/app/actions/page";
import * as apiMod from "@/lib/api";

describe("ActionCenterPage", () => {
  beforeEach(() => {
    vi.restoreAllMocks();
    window.localStorage.setItem(
      "dta_auth_user",
      JSON.stringify({
        user_id: "u-admin",
        tenant_id: "t1",
        tenant_slug: "acme",
        email: "admin@enterprise.local",
        display_name: "System Admin",
        role: "administrator",
        policy: {
          role: "administrator",
          allowed_databases: ["data_to_action_analytics"],
          allowed_schemas: ["analytics", "finance"],
          allowed_tables: [],
          denied_columns: [],
          allow_export: true,
          allow_schema_refresh: true,
          allow_admin_config: true,
          row_limit: 2000,
          region_filter: "ALL",
        },
      })
    );
  });

  it("renders Human-in-the-Loop proposal approval and connector dispatch", async () => {
    vi.spyOn(apiMod.api, "actionConnectors").mockResolvedValue([
      {
        id: "conn-1",
        name: "Ops Webhook",
        connector_type: "webhook",
        endpoint_url: "https://hooks.example.test/alerts",
        default_headers: {},
        has_secret: true,
        is_active: true,
        created_at: "2026-11-25T00:00:00Z",
      },
    ]);
    vi.spyOn(apiMod.api, "actionRules").mockResolvedValue([]);
    vi.spyOn(apiMod.api, "actionExecutions").mockResolvedValue([]);
    vi.spyOn(apiMod.api, "actionProposals").mockResolvedValue([
      {
        id: "prop-1",
        rule_id: "rule-1",
        connector_id: "conn-1",
        source_query_id: "11111111-2222-3333-4444-555555555555",
        title: "Alert: High Order Volume",
        rationale: "Observed 98 completed orders (> 10).",
        evidence_json: { observed_value: 98 },
        payload_json: {},
        status: "pending",
        idempotency_key: "rule-1-q-1",
        proposed_by: "u-admin",
        reviewed_by: null,
        review_note: "",
        reviewed_at: null,
        executed_at: null,
        created_at: "2026-11-25T00:00:00Z",
      },
    ]);

    const approveSpy = vi.spyOn(apiMod.api, "approveActionProposal").mockResolvedValue({
      id: "prop-1",
      rule_id: "rule-1",
      connector_id: "conn-1",
      source_query_id: "11111111-2222-3333-4444-555555555555",
      title: "Alert: High Order Volume",
      rationale: "Observed 98 completed orders (> 10).",
      evidence_json: { observed_value: 98 },
      payload_json: {},
      status: "approved",
      idempotency_key: "rule-1-q-1",
      proposed_by: "u-admin",
      reviewed_by: "u-admin",
      review_note: "Approved",
      reviewed_at: "2026-11-25T00:01:00Z",
      executed_at: null,
      created_at: "2026-11-25T00:00:00Z",
    });

    render(<ActionCenterPage />);

    await waitFor(() => {
      expect(screen.getByText(/Alert: High Order Volume/i)).toBeInTheDocument();
      expect(screen.getByText(/11111111-2222-3333-4444-555555555555/i)).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole("button", { name: /Approve/i }));

    await waitFor(() => {
      expect(approveSpy).toHaveBeenCalledWith("prop-1", expect.objectContaining({ connector_id: "conn-1" }));
    });
  });
});
