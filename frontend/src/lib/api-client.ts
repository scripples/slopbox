const API_URL = process.env.NEXT_PUBLIC_API_URL || "http://localhost:8080";

let cachedToken: string | null = null;
let tokenExpiry = 0;

export async function getToken(): Promise<string> {
  if (cachedToken && Date.now() < tokenExpiry) {
    return cachedToken;
  }

  const res = await fetch("/api/token");
  if (!res.ok) throw new Error("Failed to get token");
  const data = await res.json();
  cachedToken = data.token;
  // Cache for 50 minutes (tokens last 1 hour)
  tokenExpiry = Date.now() + 50 * 60 * 1000;
  return cachedToken!;
}

async function apiFetch<T>(path: string, options: RequestInit = {}): Promise<T> {
  const token = await getToken();
  const res = await fetch(`${API_URL}${path}`, {
    ...options,
    headers: {
      "Content-Type": "application/json",
      Authorization: `Bearer ${token}`,
      ...options.headers,
    },
  });

  if (!res.ok) {
    const body = await res.json().catch(() => ({ error: res.statusText }));
    throw new Error(body.error || `API error: ${res.status}`);
  }

  if (res.status === 204) return undefined as T;
  return res.json();
}

// ── Types ─────────────────────────────────────────────────────────

export interface Agent {
  id: string;
  user_id: string;
  name: string;
  vps: Vps | null;
  created_at: string;
  updated_at: string;
}

export interface Vps {
  id: string;
  vps_config_id: string;
  name: string;
  provider: string;
  state: "provisioning" | "running" | "stopped" | "destroyed";
  address: string | null;
  storage_used_bytes: number;
  created_at: string;
  updated_at: string;
}

export interface User {
  id: string;
  email: string;
  name: string | null;
  role: "user" | "admin";
  status: "pending" | "active" | "frozen";
  plan: Plan | null;
  created_at: string;
  updated_at: string;
}

export interface Plan {
  id: string;
  name: string;
  max_agents: number;
  max_vpses: number;
}

export interface AdminUser {
  id: string;
  email: string;
  name: string | null;
  role: "user" | "admin";
  status: "pending" | "active" | "frozen";
  plan_id: string | null;
  created_at: string;
  updated_at: string;
}

export interface AdminVps {
  id: string;
  user_id: string;
  name: string;
  provider: string;
  state: string;
  address: string | null;
  created_at: string;
  updated_at: string;
}

export interface VpsConfig {
  id: string;
  name: string;
  provider: string;
}

export interface HealthResponse {
  gateway_reachable: boolean;
}

// ── User ──────────────────────────────────────────────────────────

export const getMe = () => apiFetch<User>("/users/me");
export const listPlans = () => apiFetch<Plan[]>("/plans");

// ── Agents ────────────────────────────────────────────────────────

export const listAgents = () => apiFetch<Agent[]>("/agents");
export const getAgent = (id: string) => apiFetch<Agent>(`/agents/${id}`);
export const createAgent = (name: string) =>
  apiFetch<Agent>("/agents", { method: "POST", body: JSON.stringify({ name }) });
export const deleteAgent = (id: string) =>
  apiFetch<void>(`/agents/${id}`, { method: "DELETE" });

// ── VPS ───────────────────────────────────────────────────────────

export const provisionVps = (agentId: string, vpsConfigId: string) =>
  apiFetch<Vps>(`/agents/${agentId}/vps`, {
    method: "POST",
    body: JSON.stringify({ vps_config_id: vpsConfigId }),
  });
export const startVps = (agentId: string) =>
  apiFetch<Vps>(`/agents/${agentId}/vps/start`, { method: "POST" });
export const stopVps = (agentId: string) =>
  apiFetch<Vps>(`/agents/${agentId}/vps/stop`, { method: "POST" });
export const destroyVps = (agentId: string) =>
  apiFetch<void>(`/agents/${agentId}/vps`, { method: "DELETE" });
export const agentHealth = (agentId: string) =>
  apiFetch<HealthResponse>(`/agents/${agentId}/health`);

// ── Config ────────────────────────────────────────────────────────

export const restartAgent = (agentId: string) =>
  apiFetch<void>(`/agents/${agentId}/restart`, { method: "POST" });

// ── Admin ─────────────────────────────────────────────────────────

export const adminListUsers = () => apiFetch<AdminUser[]>("/admin/users");
export const adminSetUserStatus = (userId: string, status: string) =>
  apiFetch<void>(`/admin/users/${userId}/status`, {
    method: "PUT",
    body: JSON.stringify({ status }),
  });
export const adminSetUserRole = (userId: string, role: string) =>
  apiFetch<void>(`/admin/users/${userId}/role`, {
    method: "PUT",
    body: JSON.stringify({ role }),
  });
export const adminListVpses = () => apiFetch<AdminVps[]>("/admin/vpses");
export const adminStopVps = (vpsId: string) =>
  apiFetch<void>(`/admin/vpses/${vpsId}/stop`, { method: "POST" });
export const adminDestroyVps = (vpsId: string) =>
  apiFetch<void>(`/admin/vpses/${vpsId}/destroy`, { method: "POST" });

// ── WebSocket ─────────────────────────────────────────────────────

export function gatewayWsUrl(agentId: string, token: string): string {
  const base = API_URL.replace(/^http/, "ws");
  return `${base}/agents/${agentId}/gateway/ws?token=${encodeURIComponent(token)}`;
}
