"use client";

import { useEffect, useState } from "react";
import { getMe, type User } from "@/lib/api-client";

export default function PendingPage() {
  const [user, setUser] = useState<User | null>(null);

  useEffect(() => {
    getMe()
      .then(setUser)
      .catch(() => {});
  }, []);

  return (
    <div className="flex min-h-screen items-center justify-center">
      <div className="w-full max-w-md space-y-4 rounded-lg border border-zinc-800 bg-zinc-900 p-8 text-center">
        <div className="mx-auto flex h-16 w-16 items-center justify-center rounded-full bg-amber-500/10">
          <span className="text-3xl">&#9203;</span>
        </div>
        <h1 className="text-xl font-bold">Account Pending Approval</h1>
        <p className="text-sm text-zinc-400">
          Hi {user?.name || user?.email || "there"}! Your account has been
          created but is awaiting admin approval. You&apos;ll get access once an
          administrator activates your account.
        </p>
        <p className="text-xs text-zinc-500">
          Status: <span className="font-mono text-amber-400">pending</span>
        </p>
      </div>
    </div>
  );
}
