import type {
  ActionConnectorDto,
  ActionExecutionDto,
  ActionProposalDto,
  ActionRuleDto,
  AdminUserDto,
  AuditLogDto,
  ConnectionDto,
  DashboardDto,
  LoginResponse,
  OverviewResponse,
  ProviderDto,
  QueryHistoryItem,
  QueryPipelineResponse,
  RbacPolicy,
  RuleEvaluationSummary,
  SavedQueryDto,
  SchemaResponse,
  SemanticMetric,
  UserProfile,
  ValidationReport,
} from "./types";

const API_BASE =
  process.env.NEXT_PUBLIC_API_URL || "http://127.0.0.1:8420";

const USER_KEY = "dta_auth_user";

export function getStoredToken(): string | null {
  return null;
}

export function getStoredUser(): UserProfile | null {
  if (typeof window === "undefined") return null;
  const raw = window.localStorage.getItem(USER_KEY);
  if (!raw) return null;
  try {
    return JSON.parse(raw) as UserProfile;
  } catch {
    return null;
  }
}

export function setAuthSession(resp: LoginResponse): void {
  if (typeof window === "undefined") return;
  // Session JWT lives in an HttpOnly; SameSite=Lax cookie set by /api/v1/auth/login.
  window.localStorage.setItem(USER_KEY, JSON.stringify(resp.user));
}

export function clearAuthSession(): void {
  if (typeof window === "undefined") return;
  window.localStorage.removeItem(USER_KEY);
}

async function apiRequest<T>(path: string, init: RequestInit = {}): Promise<T> {
  const headers = new Headers(init.headers);
  if (!headers.has("Content-Type") && !(init.body instanceof FormData)) {
    headers.set("Content-Type", "application/json");
  }

  const res = await fetch(`${API_BASE}/api/v1${path}`, {
    ...init,
    credentials: "include",
    headers,
  });

  if (res.status === 401 && typeof window !== "undefined") {
    clearAuthSession();
  }

  if (!res.ok) {
    let errMsg = `HTTP ${res.status}`;
    try {
      const payload = (await res.json()) as { error?: string };
      if (payload?.error) errMsg = payload.error;
    } catch {
      // keep HTTP code
    }
    throw new Error(errMsg);
  }

  return (await res.json()) as T;
}

export const api = {
  login: (email: string, password: string) =>
    apiRequest<LoginResponse>("/auth/login", {
      method: "POST",
      body: JSON.stringify({ email, password }),
    }),
  logout: () =>
    apiRequest<{ ok: boolean }>("/auth/logout", {
      method: "POST",
    }),
  me: () => apiRequest<UserProfile>("/auth/me"),
  overview: () => apiRequest<OverviewResponse>("/overview"),
  askQuery: (payload: {
    question: string;
    connection_id?: string;
    preferred_metric?: string;
    clarify_ambiguity?: boolean;
    requested_row_limit?: number;
    session_id?: string;
    conversation_history?: Array<{
      question: string;
      sql: string;
      explanation: string;
    }>;
  }) =>
    apiRequest<QueryPipelineResponse>("/queries/ask", {
      method: "POST",
      body: JSON.stringify(payload),
    }),
  validateSql: (sql: string, requested_row_limit?: number, connection_id?: string) =>
    apiRequest<ValidationReport>("/queries/validate", {
      method: "POST",
      body: JSON.stringify({ sql, requested_row_limit, connection_id }),
    }),
  executeValidatedSql: (
    sql: string,
    question?: string,
    requested_row_limit?: number,
    connection_id?: string
  ) =>
    apiRequest<QueryPipelineResponse>("/queries/execute-validated", {
      method: "POST",
      body: JSON.stringify({ sql, question, requested_row_limit, connection_id }),
    }),
  queryHistory: () => apiRequest<QueryHistoryItem[]>("/queries/history"),
  exportCsv: async (query_id: string): Promise<string> => {
    const res = await fetch(`${API_BASE}/api/v1/queries/export`, {
      method: "POST",
      credentials: "include",
      headers: {
        "Content-Type": "application/json",
      },
      body: JSON.stringify({ query_id }),
    });
    if (!res.ok) {
      const err = (await res.json().catch(() => ({}))) as { error?: string };
      throw new Error(err.error || `Export failed (HTTP ${res.status})`);
    }
    return await res.text();
  },
  schema: (connectionId?: string) =>
    apiRequest<SchemaResponse>(
      connectionId ? `/schema?connection_id=${encodeURIComponent(connectionId)}` : "/schema"
    ),
  refreshSchema: (connectionId?: string) =>
    apiRequest<SchemaResponse>(
      connectionId
        ? `/schema/refresh?connection_id=${encodeURIComponent(connectionId)}`
        : "/schema/refresh",
      { method: "POST" }
    ),
  addManualRelationship: (payload: Record<string, string>) =>
    apiRequest("/schema/relationships", {
      method: "POST",
      body: JSON.stringify(payload),
    }),
  semantics: () => apiRequest<SemanticMetric[]>("/semantics"),
  semanticVersions: () => apiRequest<SemanticMetric[]>("/semantics/versions"),
  upsertSemantic: (payload: Record<string, unknown>) =>
    apiRequest<SemanticMetric>("/semantics", {
      method: "POST",
      body: JSON.stringify(payload),
    }),
  savedQueries: () => apiRequest<SavedQueryDto[]>("/saved-queries"),
  createSavedQuery: (payload: {
    query_execution_id?: string;
    title: string;
    question: string;
    sql_text: string;
    notes?: string;
    is_shared?: boolean;
  }) =>
    apiRequest<SavedQueryDto>("/saved-queries", {
      method: "POST",
      body: JSON.stringify(payload),
    }),
  deleteSavedQuery: (id: string) =>
    apiRequest(`/saved-queries/${id}`, { method: "DELETE" }),
  dashboards: () => apiRequest<DashboardDto[]>("/dashboards"),
  createDashboard: (payload: {
    title: string;
    description: string;
    widgets: unknown[];
    is_shared?: boolean;
  }) =>
    apiRequest<DashboardDto>("/dashboards", {
      method: "POST",
      body: JSON.stringify(payload),
    }),
  addDashboardWidget: (dashboardId: string, widget: unknown) =>
    apiRequest<DashboardDto>(`/dashboards/${dashboardId}/widgets`, {
      method: "POST",
      body: JSON.stringify({ widget }),
    }),
  refreshDashboard: (dashboardId: string, connectionId?: string) =>
    apiRequest<DashboardDto>(`/dashboards/${dashboardId}/refresh`, {
      method: "POST",
      body: JSON.stringify({ connection_id: connectionId || null }),
    }),
  deleteDashboard: (id: string) =>
    apiRequest(`/dashboards/${id}`, { method: "DELETE" }),
  connections: () => apiRequest<ConnectionDto[]>("/connections"),
  createConnection: (payload: Record<string, unknown>) =>
    apiRequest<ConnectionDto>("/connections", {
      method: "POST",
      body: JSON.stringify(payload),
    }),
  testConnection: (id: string) =>
    apiRequest<{ connection_id: string; identity: string; status: string }>(
      `/connections/${id}/test`,
      { method: "POST" }
    ),
  providers: () => apiRequest<ProviderDto[]>("/providers"),
  createProvider: (payload: Record<string, unknown>) =>
    apiRequest<ProviderDto>("/providers", {
      method: "POST",
      body: JSON.stringify(payload),
    }),
  activateProvider: (id: string) =>
    apiRequest(`/providers/${id}/activate`, { method: "POST" }),
  adminUsers: () => apiRequest<AdminUserDto[]>("/admin/users"),
  updateAdminUser: (id: string, payload: { role?: string; is_active?: boolean }) =>
    apiRequest<AdminUserDto>(`/admin/users/${id}`, {
      method: "PUT",
      body: JSON.stringify(payload),
    }),
  rbacPolicies: () => apiRequest<RbacPolicy[]>("/admin/rbac"),
  updateRbacPolicy: (role: string, payload: Record<string, unknown>) =>
    apiRequest<RbacPolicy>(`/admin/rbac/${role}`, {
      method: "PUT",
      body: JSON.stringify(payload),
    }),
  auditLogs: () => apiRequest<AuditLogDto[]>("/audit"),
  actionConnectors: () => apiRequest<ActionConnectorDto[]>("/actions/connectors"),
  createActionConnector: (payload: Record<string, unknown>) =>
    apiRequest<ActionConnectorDto>("/actions/connectors", {
      method: "POST",
      body: JSON.stringify(payload),
    }),
  deleteActionConnector: (id: string) =>
    apiRequest(`/actions/connectors/${id}`, { method: "DELETE" }),
  actionRules: () => apiRequest<ActionRuleDto[]>("/actions/rules"),
  createActionRule: (payload: Record<string, unknown>) =>
    apiRequest<ActionRuleDto>("/actions/rules", {
      method: "POST",
      body: JSON.stringify(payload),
    }),
  evaluateActionRule: (id: string) =>
    apiRequest<RuleEvaluationSummary>(`/actions/rules/${id}/evaluate`, {
      method: "POST",
    }),
  deleteActionRule: (id: string) =>
    apiRequest(`/actions/rules/${id}`, { method: "DELETE" }),
  actionProposals: () => apiRequest<ActionProposalDto[]>("/actions/proposals"),
  createActionProposal: (payload: {
    source_query_id: string;
    connector_id?: string;
    title: string;
    rationale: string;
    evidence_json?: Record<string, unknown>;
    payload_json?: Record<string, unknown>;
  }) =>
    apiRequest<ActionProposalDto>("/actions/proposals", {
      method: "POST",
      body: JSON.stringify(payload),
    }),
  approveActionProposal: (
    id: string,
    payload: { review_note?: string; connector_id?: string } = {}
  ) =>
    apiRequest<ActionProposalDto>(`/actions/proposals/${id}/approve`, {
      method: "POST",
      body: JSON.stringify(payload),
    }),
  rejectActionProposal: (id: string, payload: { review_note?: string } = {}) =>
    apiRequest<ActionProposalDto>(`/actions/proposals/${id}/reject`, {
      method: "POST",
      body: JSON.stringify(payload),
    }),
  executeActionProposal: (id: string) =>
    apiRequest<{ proposal: ActionProposalDto; execution: ActionExecutionDto }>(
      `/actions/proposals/${id}/execute`,
      { method: "POST" }
    ),
  actionExecutions: () => apiRequest<ActionExecutionDto[]>("/actions/executions"),
};
