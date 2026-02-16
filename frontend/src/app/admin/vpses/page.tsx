"use client";

import { useEffect, useState } from "react";
import Link from "next/link";
import {
  adminListVpses,
  adminStopVps,
  adminDestroyVps,
  type AdminVps,
} from "@/lib/api-client";

export default function AdminVpsesPage() {
  const [vpses, setVpses] = useState<AdminVps[]>([]);
  const [loading, setLoading] = useState(true);
  const [actionLoading, setActionLoading] = useState("");

  const refresh = async () => {
    try {
      const data = await adminListVpses();
      setVpses(data);
    } catch (e) {
      console.error(e);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    refresh();
  }, []);

  const doAction = async (key: string, fn: () => Promise<unknown>) => {
    setActionLoading(key);
    try {
      await fn();
      await refresh();
    } catch (e) {
      alert(e instanceof Error ? e.message : "Action failed");
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

  return (
    <div className="mx-auto max-w-5xl p-8">
      <div className="flex items-center gap-4">
        <Link
          href="/dashboard"
          className="text-sm text-zinc-500 hover:text-zinc-300"
        >
          &larr; Dashboard
        </Link>
        <h1 className="text-2xl font-bold">Admin: VPSes</h1>
        <Link
          href="/admin/users"
          className="ml-auto text-sm text-blue-400 hover:underline"
        >
          &larr; Users
        </Link>
      </div>

      <div className="mt-6 overflow-x-auto">
        <table className="w-full text-sm">
          <thead>
            <tr className="border-b border-zinc-800 text-left text-zinc-400">
              <th className="pb-2 pr-4">Name</th>
              <th className="pb-2 pr-4">Provider</th>
              <th className="pb-2 pr-4">State</th>
              <th className="pb-2 pr-4">Address</th>
              <th className="pb-2 pr-4">User</th>
              <th className="pb-2">Actions</th>
            </tr>
          </thead>
          <tbody>
            {vpses.length === 0 ? (
              <tr>
                <td colSpan={6} className="py-8 text-center text-zinc-500">
                  No VPSes found.
                </td>
              </tr>
            ) : (
              vpses.map((vps) => (
                <tr
                  key={vps.id}
                  className="border-b border-zinc-800/50 hover:bg-zinc-900"
                >
                  <td className="py-3 pr-4">{vps.name}</td>
                  <td className="py-3 pr-4 text-xs text-zinc-400">
                    {vps.provider}
                  </td>
                  <td className="py-3 pr-4">
                    <span
                      className={
                        vps.state === "running"
                          ? "text-green-400"
                          : vps.state === "stopped"
                            ? "text-yellow-400"
                            : "text-zinc-400"
                      }
                    >
                      {vps.state}
                    </span>
                  </td>
                  <td className="py-3 pr-4 font-mono text-xs text-zinc-500">
                    {vps.address || "\u2014"}
                  </td>
                  <td className="py-3 pr-4 font-mono text-xs text-zinc-500">
                    {vps.user_id.slice(0, 8)}...
                  </td>
                  <td className="py-3">
                    <div className="flex gap-1">
                      {vps.state === "running" && (
                        <button
                          onClick={() =>
                            doAction(`stop-${vps.id}`, () =>
                              adminStopVps(vps.id)
                            )
                          }
                          disabled={!!actionLoading}
                          className="rounded bg-yellow-900 px-2 py-1 text-xs text-yellow-200 hover:bg-yellow-800 disabled:opacity-50"
                        >
                          {actionLoading === `stop-${vps.id}`
                            ? "..."
                            : "Stop"}
                        </button>
                      )}
                      <button
                        onClick={() => {
                          if (!confirm("Destroy this VPS? This cannot be undone."))
                            return;
                          doAction(`destroy-${vps.id}`, () =>
                            adminDestroyVps(vps.id)
                          );
                        }}
                        disabled={!!actionLoading}
                        className="rounded bg-red-900 px-2 py-1 text-xs text-red-200 hover:bg-red-800 disabled:opacity-50"
                      >
                        {actionLoading === `destroy-${vps.id}`
                          ? "..."
                          : "Destroy"}
                      </button>
                    </div>
                  </td>
                </tr>
              ))
            )}
          </tbody>
        </table>
      </div>
    </div>
  );
}
