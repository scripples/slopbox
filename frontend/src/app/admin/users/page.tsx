"use client";

import { useEffect, useState } from "react";
import Link from "next/link";
import {
  adminListUsers,
  adminSetUserStatus,
  adminSetUserRole,
  type AdminUser,
} from "@/lib/api-client";

export default function AdminUsersPage() {
  const [users, setUsers] = useState<AdminUser[]>([]);
  const [loading, setLoading] = useState(true);
  const [actionLoading, setActionLoading] = useState("");

  const refresh = async () => {
    try {
      const data = await adminListUsers();
      setUsers(data);
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
        <h1 className="text-2xl font-bold">Admin: Users</h1>
        <Link
          href="/admin/vpses"
          className="ml-auto text-sm text-blue-400 hover:underline"
        >
          VPSes &rarr;
        </Link>
      </div>

      <div className="mt-6 overflow-x-auto">
        <table className="w-full text-sm">
          <thead>
            <tr className="border-b border-zinc-800 text-left text-zinc-400">
              <th className="pb-2 pr-4">Email</th>
              <th className="pb-2 pr-4">Name</th>
              <th className="pb-2 pr-4">Role</th>
              <th className="pb-2 pr-4">Status</th>
              <th className="pb-2 pr-4">Created</th>
              <th className="pb-2">Actions</th>
            </tr>
          </thead>
          <tbody>
            {users.map((user) => (
              <tr
                key={user.id}
                className="border-b border-zinc-800/50 hover:bg-zinc-900"
              >
                <td className="py-3 pr-4 font-mono text-xs">{user.email}</td>
                <td className="py-3 pr-4">{user.name || "\u2014"}</td>
                <td className="py-3 pr-4">
                  <span
                    className={
                      user.role === "admin"
                        ? "text-purple-400"
                        : "text-zinc-400"
                    }
                  >
                    {user.role}
                  </span>
                </td>
                <td className="py-3 pr-4">
                  <span
                    className={
                      user.status === "active"
                        ? "text-green-400"
                        : user.status === "pending"
                          ? "text-amber-400"
                          : "text-red-400"
                    }
                  >
                    {user.status}
                  </span>
                </td>
                <td className="py-3 pr-4 text-xs text-zinc-500">
                  {new Date(user.created_at).toLocaleDateString()}
                </td>
                <td className="py-3">
                  <div className="flex flex-wrap gap-1">
                    {user.status === "pending" && (
                      <button
                        onClick={() =>
                          doAction(`approve-${user.id}`, () =>
                            adminSetUserStatus(user.id, "active")
                          )
                        }
                        disabled={!!actionLoading}
                        className="rounded bg-green-800 px-2 py-1 text-xs text-green-200 hover:bg-green-700 disabled:opacity-50"
                      >
                        {actionLoading === `approve-${user.id}`
                          ? "..."
                          : "Approve"}
                      </button>
                    )}
                    {user.status === "active" && (
                      <button
                        onClick={() =>
                          doAction(`freeze-${user.id}`, () =>
                            adminSetUserStatus(user.id, "frozen")
                          )
                        }
                        disabled={!!actionLoading}
                        className="rounded bg-red-900 px-2 py-1 text-xs text-red-200 hover:bg-red-800 disabled:opacity-50"
                      >
                        Freeze
                      </button>
                    )}
                    {user.status === "frozen" && (
                      <button
                        onClick={() =>
                          doAction(`activate-${user.id}`, () =>
                            adminSetUserStatus(user.id, "active")
                          )
                        }
                        disabled={!!actionLoading}
                        className="rounded bg-green-800 px-2 py-1 text-xs text-green-200 hover:bg-green-700 disabled:opacity-50"
                      >
                        Activate
                      </button>
                    )}
                    {user.role === "user" && (
                      <button
                        onClick={() =>
                          doAction(`promote-${user.id}`, () =>
                            adminSetUserRole(user.id, "admin")
                          )
                        }
                        disabled={!!actionLoading}
                        className="rounded bg-purple-900 px-2 py-1 text-xs text-purple-200 hover:bg-purple-800 disabled:opacity-50"
                      >
                        Make Admin
                      </button>
                    )}
                    {user.role === "admin" && (
                      <button
                        onClick={() =>
                          doAction(`demote-${user.id}`, () =>
                            adminSetUserRole(user.id, "user")
                          )
                        }
                        disabled={!!actionLoading}
                        className="rounded bg-zinc-700 px-2 py-1 text-xs text-zinc-300 hover:bg-zinc-600 disabled:opacity-50"
                      >
                        Remove Admin
                      </button>
                    )}
                  </div>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  );
}
