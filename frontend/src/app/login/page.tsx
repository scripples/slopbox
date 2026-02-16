import { signIn, auth } from "@/lib/auth";
import { redirect } from "next/navigation";

export default async function LoginPage() {
  const session = await auth();
  if (session) redirect("/dashboard");

  return (
    <div className="flex min-h-screen items-center justify-center">
      <div className="w-full max-w-sm space-y-6 rounded-lg border border-zinc-800 bg-zinc-900 p-8">
        <div className="text-center">
          <h1 className="text-2xl font-bold">Slopbox</h1>
          <p className="mt-2 text-sm text-zinc-400">
            Sign in to manage your AI agents
          </p>
        </div>

        <div className="space-y-3">
          <form
            action={async () => {
              "use server";
              await signIn("google", { redirectTo: "/dashboard" });
            }}
          >
            <button
              type="submit"
              className="flex w-full items-center justify-center gap-2 rounded-md border border-zinc-700 bg-zinc-800 px-4 py-2.5 text-sm font-medium hover:bg-zinc-700 transition-colors"
            >
              Continue with Google
            </button>
          </form>

          <form
            action={async () => {
              "use server";
              await signIn("github", { redirectTo: "/dashboard" });
            }}
          >
            <button
              type="submit"
              className="flex w-full items-center justify-center gap-2 rounded-md border border-zinc-700 bg-zinc-800 px-4 py-2.5 text-sm font-medium hover:bg-zinc-700 transition-colors"
            >
              Continue with GitHub
            </button>
          </form>
        </div>
      </div>
    </div>
  );
}
