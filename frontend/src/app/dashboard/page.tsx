"use client";

import { useEffect, useState } from "react";
import Link from "next/link";
import {
  getMe,
  listAgents,
  createAgent,
  listPlans,
  type Agent,
  type User,
  type Plan,
} from "@/lib/api-client";

export default function DashboardPage() {
  const [user, setUser] = useState<User | null>(null);
  const [agents, setAgents] = useState<Agent[]>([]);
  const [plans, setPlans] = useState<Plan[]>([]);
  const [newAgentName, setNewAgentName] = useState("");
  const [creating, setCreating] = useState(false);
  const [error, setError] = useState("");

  useEffect(() => {
    getMe().then((u) => {
      setUser(u);
      if (u.status === "pending") {
        window.location.href = "/pending";
      }
    });
    listAgents().then(setAgents);
    listPlans().then(setPlans);
  }, []);

  const handleCreate = async () => {
    if (!newAgentName.trim()) return;
    setCreating(true);
    setError("");
    try {
      const agent = await createAgent(newAgentName.trim());
      setAgents((prev) => [...prev, agent]);
      setNewAgentName("");
    } catch (e) {
      setError(e instanceof Error ? e.message : "Failed to create agent");
    } finally {
      setCreating(false);
    }
  };

  return (
    <div className="mx-auto max-w-4xl p-8">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-bold">Dashboard</h1>
          <p className="text-sm text-zinc-400">
            {user?.email} &middot; {user?.plan?.name || "no plan"} plan
            {user?.role === "admin" && (
              <>
                {" "}
                &middot;{" "}
                <Link href="/admin/users" className="text-blue-400 hover:underline">
                  Admin Panel
                </Link>
              </>
            )}
          </p>
        </div>
      </div>

      <div className="mt-8">
        <h2 className="text-lg font-semibold">Agents</h2>

        <div className="mt-4 flex gap-2">
          <input
            type="text"
            value={newAgentName}
            onChange={(e) => setNewAgentName(e.target.value)}
            placeholder="Agent name..."
            className="flex-1 rounded-md border border-zinc-700 bg-zinc-800 px-3 py-2 text-sm placeholder:text-zinc-500 focus:border-zinc-500 focus:outline-none"
            onKeyDown={(e) => e.key === "Enter" && handleCreate()}
          />
          <button
            onClick={handleCreate}
            disabled={creating || !newAgentName.trim()}
            className="rounded-md bg-blue-600 px-4 py-2 text-sm font-medium hover:bg-blue-500 disabled:opacity-50 transition-colors"
          >
            {creating ? "Creating..." : "Create Agent"}
          </button>
        </div>

        {error && (
          <p className="mt-2 text-sm text-red-400">{error}</p>
        )}

        <div className="mt-4 space-y-2">
          {agents.length === 0 ? (
            <p className="text-sm text-zinc-500">
              No agents yet. Create one to get started.
            </p>
          ) : (
            agents.map((agent) => (
              <Link
                key={agent.id}
                href={`/dashboard/agents/${agent.id}`}
                className="flex items-center justify-between rounded-md border border-zinc-800 bg-zinc-900 p-4 hover:border-zinc-700 transition-colors"
              >
                <div>
                  <p className="font-medium">{agent.name}</p>
                  <p className="text-xs text-zinc-500">
                    {agent.vps
                      ? `${agent.vps.provider} \u00B7 ${agent.vps.state}`
                      : "No VPS provisioned"}
                  </p>
                </div>
                <div className="flex items-center gap-2">
                  {agent.vps && (
                    <span
                      className={`inline-block h-2 w-2 rounded-full ${
                        agent.vps.state === "running"
                          ? "bg-green-400"
                          : agent.vps.state === "stopped"
                            ? "bg-yellow-400"
                            : "bg-zinc-500"
                      }`}
                    />
                  )}
                  <span className="text-xs text-zinc-500">&rarr;</span>
                </div>
              </Link>
            ))
          )}
        </div>
      </div>

      {plans.length > 0 && (
        <div className="mt-8 text-xs text-zinc-600">
          Available plans: {plans.map((p) => p.name).join(", ")}
        </div>
      )}
    </div>
  );
}
