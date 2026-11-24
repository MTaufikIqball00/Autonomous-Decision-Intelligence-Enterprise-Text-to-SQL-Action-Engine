import type {
  AuditLogDto,
  ConnectionDto,
  DashboardDto,
  LoginResponse,
  OverviewResponse,
  ProviderDto,
  QueryHistoryItem,
  QueryPipelineResponse,
  RbacPolicy,
  SavedQueryDto,
  SchemaResponse,
  SemanticMetric,
  UserProfile,
  ValidationReport,
} from "./types";

const API_BASE =
  process.env.NEXT_PUBLIC_API_URL || "http://127.0.0.1:8420";

const TOKEN_KEY = "dta_auth_token";
const USER_KEY = "dta_auth_user";

export function getStoredToken(): string | null {
  if (typeof window === "undefined") return null;
  return window.localStorage.getItem(TOKEN_KEY);
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
  window.localStorage.setItem(TOKEN_KEY, resp.token);
  window.localStorage.setItem(USER_KEY, JSON.stringify(resp.user));
}

export function clearAuthSession(): void {
  if (typeof window === "undefined") return;
  window.localStorage.removeItem(TOKEN_KEY);
  window.localStorage.removeItem(USER_KEY);
}

async function apiRequest<T>(path: string, init: RequestInit = {}): Promise<T> {
  const token = getStoredToken();
  const headers = new Headers(init.headers);
  if (!headers.has("Content-Type") && !(init.body instanceof FormData)) {
    headers.set("Content-Type", "application/json");
  }
  if (token) {
    headers.set("Authorization", `Bearer ${token}`);
  }

  const res = await fetch(`${API_BASE}/api/v1${path}`, {
    ...init,
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
  me: () => apiRequest<UserProfile>("/auth/me"),
  overview: () => apiRequest<OverviewResponse>("/overview"),
  askQuery: (payload: {
    question: string;
    preferred_metric?: string;
    clarify_ambiguity?: boolean;
    requested_row_limit?: number;
  }) =>
    apiRequest<QueryPipelineResponse>("/queries/ask", {
      method: "POST",
      body: JSON.stringify(payload),
    }),
  validateSql: (sql: string, requested_row_limit?: number) =>
    apiRequest<ValidationReport>("/queries/validate", {
      method: "POST",
      body: JSON.stringify({ sql, requested_row_limit }),
    }),
  executeValidatedSql: (sql: string, question?: string, requested_row_limit?: number) =>
    apiRequest<QueryPipelineResponse>("/queries/execute-validated", {
      method: "POST",
      body: JSON.stringify({ sql, question, requested_row_limit }),
    }),
  queryHistory: () => apiRequest<QueryHistoryItem[]>("/queries/history"),
  exportCsv: async (query_id: string): Promise<string> => {
    const token = getStoredToken();
    const res = await fetch(`${API_BASE}/api/v1/queries/export`, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        ...(token ? { Authorization: `Bearer ${token}` } : {}),
      },
      body: JSON.stringify({ query_id }),
    });
    if (!res.ok) {
      const err = (await res.json().catch(() => ({}))) as { error?: string };
      throw new Error(err.error || `Export failed (HTTP ${res.status})`);
    }
    return await res.text();
  },
  schema: () => apiRequest<SchemaResponse>("/schema"),
  refreshSchema: () =>
    apiRequest<SchemaResponse>("/schema/refresh", { method: "POST" }),
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
  adminUsers: () =>
    apiRequest<
      Array<{
        id: string;
        email: string;
        display_name: string;
        role: string;
        is_active: boolean;
        created_at: string;
      }>
    >("/admin/users"),
  rbacPolicies: () => apiRequest<RbacPolicy[]>("/admin/rbac"),
  updateRbacPolicy: (role: string, payload: Record<string, unknown>) =>
    apiRequest<RbacPolicy>(`/admin/rbac/${role}`, {
      method: "PUT",
      body: JSON.stringify(payload),
    }),
  auditLogs: () => apiRequest<AuditLogDto[]>("/audit"),
};
