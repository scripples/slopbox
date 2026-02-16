"use client";

import { useEffect, useState } from "react";
import { useParams, useRouter } from "next/navigation";
import Link from "next/link";
import {
  getAgent,
  deleteAgent,
  provisionVps,
  startVps,
  stopVps,
  destroyVps,
  restartAgent,
  agentHealth,
  listPlans,
  getMe,
  type Agent,
  type HealthResponse,
} from "@/lib/api-client";

export default function AgentDetailPage() {
  const params = useParams();
  const router = useRouter();
  const agentId = params.id as string;

  const [agent, setAgent] = useState<Agent | null>(null);
  const [health, setHealth] = useState<HealthResponse | null>(null);
  const [loading, setLoading] = useState(true);
  const [actionLoading, setActionLoading] = useState("");
  const [error, setError] = useState("");
  const [vpsConfigId, setVpsConfigId] = useState("");

  const refresh = async () => {
    try {
      const a = await getAgent(agentId);
      setAgent(a);
      if (a.vps?.state === "running") {
        agentHealth(agentId).then(setHealth).catch(() => {});
      }
    } catch {
      setError("Agent not found");
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    refresh();

    // Fetch default VPS config
    getMe().then((user) => {
      if (user.plan) {
        // Use the first available VPS config from the plan
        // In practice, we'd fetch vps_configs for the plan
        // For demo, we'll set a placeholder
      }
    });
  }, [agentId]);

  const doAction = async (
    name: string,
    fn: () => Promise<unknown>
  ) => {
    setActionLoading(name);
    setError("");
    try {
      await fn();
      await refresh();
    } catch (e) {
      setError(e instanceof Error ? e.message : `Failed: ${name}`);
    } finally {
      setActionLoading("");
    }
  };

  if (loading) {
    return (
      <div className="flex min-h-screen items-center justify-center">
        <p className="text-zinc-500">Loading...</p>
      </div>
    );
  }

  if (!agent) {
    return (
      <div className="flex min-h-screen items-center justify-center">
        <p className="text-red-400">{error || "Agent not found"}</p>
      </div>
    );
  }

  return (
    <div className="mx-auto max-w-4xl p-8">
      <div className="flex items-center gap-4">
        <Link
          href="/dashboard"
          className="text-sm text-zinc-500 hover:text-zinc-300"
        >
          &larr; Dashboard
        </Link>
      </div>

      <div className="mt-6">
        <h1 className="text-2xl font-bold">{agent.name}</h1>
        <p className="text-xs text-zinc-500 font-mono">{agent.id}</p>
      </div>

      {error && (
        <div className="mt-4 rounded-md border border-red-900 bg-red-950 p-3 text-sm text-red-300">
          {error}
        </div>
      )}

      {/* VPS Section */}
      <div className="mt-8 rounded-lg border border-zinc-800 bg-zinc-900 p-6">
        <h2 className="text-lg font-semibold">VPS</h2>

        {agent.vps ? (
          <div className="mt-4 space-y-4">
            <div className="grid grid-cols-2 gap-4 text-sm">
              <div>
                <span className="text-zinc-500">Provider:</span>{" "}
                {agent.vps.provider}
              </div>
              <div>
                <span className="text-zinc-500">State:</span>{" "}
                <span
                  className={
                    agent.vps.state === "running"
                      ? "text-green-400"
                      : agent.vps.state === "stopped"
                        ? "text-yellow-400"
                        : "text-zinc-400"
                  }
                >
                  {agent.vps.state}
                </span>
              </div>
              {agent.vps.address && (
                <div>
                  <span className="text-zinc-500">Address:</span>{" "}
                  <span className="font-mono text-xs">{agent.vps.address}</span>
                </div>
              )}
              {health && (
                <div>
                  <span className="text-zinc-500">Gateway:</span>{" "}
                  <span
                    className={
                      health.gateway_reachable
                        ? "text-green-400"
                        : "text-red-400"
                    }
                  >
                    {health.gateway_reachable ? "reachable" : "unreachable"}
                  </span>
                </div>
              )}
            </div>

            <div className="flex flex-wrap gap-2">
              {agent.vps.state === "running" && (
                <>
                  <Link
                    href={`/dashboard/agents/${agentId}/chat`}
                    className="rounded-md bg-blue-600 px-3 py-1.5 text-sm font-medium hover:bg-blue-500 transition-colors"
                  >
                    Open Chat
                  </Link>
                  <button
                    onClick={() => doAction("stop", () => stopVps(agentId))}
                    disabled={!!actionLoading}
                    className="rounded-md border border-zinc-700 px-3 py-1.5 text-sm hover:bg-zinc-800 disabled:opacity-50 transition-colors"
                  >
                    {actionLoading === "stop" ? "Stopping..." : "Stop"}
                  </button>
                  <button
                    onClick={() =>
                      doAction("restart", () => restartAgent(agentId))
                    }
                    disabled={!!actionLoading}
                    className="rounded-md border border-zinc-700 px-3 py-1.5 text-sm hover:bg-zinc-800 disabled:opacity-50 transition-colors"
                  >
                    {actionLoading === "restart"
                      ? "Restarting..."
                      : "Restart Gateway"}
                  </button>
                </>
              )}
              {agent.vps.state === "stopped" && (
                <button
                  onClick={() => doAction("start", () => startVps(agentId))}
                  disabled={!!actionLoading}
                  className="rounded-md bg-green-700 px-3 py-1.5 text-sm font-medium hover:bg-green-600 disabled:opacity-50 transition-colors"
                >
                  {actionLoading === "start" ? "Starting..." : "Start"}
                </button>
              )}
              <button
                onClick={() =>
                  doAction("destroy", () => destroyVps(agentId))
                }
                disabled={!!actionLoading}
                className="rounded-md border border-red-900 px-3 py-1.5 text-sm text-red-400 hover:bg-red-950 disabled:opacity-50 transition-colors"
              >
                {actionLoading === "destroy" ? "Destroying..." : "Destroy VPS"}
              </button>
            </div>
          </div>
        ) : (
          <div className="mt-4 space-y-3">
            <p className="text-sm text-zinc-500">No VPS provisioned.</p>
            <div className="flex gap-2">
              <input
                type="text"
                value={vpsConfigId}
                onChange={(e) => setVpsConfigId(e.target.value)}
                placeholder="VPS Config ID (from plan)"
                className="flex-1 rounded-md border border-zinc-700 bg-zinc-800 px-3 py-2 text-sm font-mono placeholder:text-zinc-500 focus:border-zinc-500 focus:outline-none"
              />
              <button
                onClick={() =>
                  doAction("provision", () =>
                    provisionVps(agentId, vpsConfigId)
                  )
                }
                disabled={!!actionLoading || !vpsConfigId}
                className="rounded-md bg-green-700 px-4 py-2 text-sm font-medium hover:bg-green-600 disabled:opacity-50 transition-colors"
              >
                {actionLoading === "provision"
                  ? "Provisioning..."
                  : "Provision VPS"}
              </button>
            </div>
          </div>
        )}
      </div>

      {/* Danger Zone */}
      <div className="mt-8 rounded-lg border border-red-900/50 bg-zinc-900 p-6">
        <h2 className="text-lg font-semibold text-red-400">Danger Zone</h2>
        <div className="mt-4">
          <button
            onClick={async () => {
              if (
                !confirm(
                  "Are you sure? This will delete the agent and destroy its VPS."
                )
              )
                return;
              await doAction("delete", () => deleteAgent(agentId));
              router.push("/dashboard");
            }}
            disabled={!!actionLoading}
            className="rounded-md border border-red-900 px-4 py-2 text-sm text-red-400 hover:bg-red-950 disabled:opacity-50 transition-colors"
          >
            Delete Agent
          </button>
        </div>
      </div>
    </div>
  );
}
