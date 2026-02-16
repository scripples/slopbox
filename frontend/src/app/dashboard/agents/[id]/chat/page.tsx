"use client";

import { useEffect, useRef, useState, useCallback } from "react";
import { useParams } from "next/navigation";
import Link from "next/link";
import { getToken, gatewayWsUrl } from "@/lib/api-client";

interface ChatMessage {
  id: string;
  role: "user" | "assistant" | "system";
  content: string;
  timestamp: Date;
}

let msgIdCounter = 0;
function nextMsgId(): string {
  return `msg-${++msgIdCounter}`;
}

let rpcIdCounter = 0;
function nextRpcId(): number {
  return ++rpcIdCounter;
}

export default function ChatPage() {
  const params = useParams();
  const agentId = params.id as string;

  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [input, setInput] = useState("");
  const [connected, setConnected] = useState(false);
  const [connecting, setConnecting] = useState(true);

  const wsRef = useRef<WebSocket | null>(null);
  const messagesEndRef = useRef<HTMLDivElement>(null);

  const scrollToBottom = useCallback(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: "smooth" });
  }, []);

  useEffect(() => {
    scrollToBottom();
  }, [messages, scrollToBottom]);

  useEffect(() => {
    let ws: WebSocket | null = null;
    let disposed = false;

    async function connect() {
      try {
        const token = await getToken();
        const url = gatewayWsUrl(agentId, token);

        ws = new WebSocket(url);
        wsRef.current = ws;

        ws.onopen = () => {
          if (disposed) return;
          // Send connect handshake
          const connectMsg = {
            id: nextRpcId(),
            method: "connect",
            params: {
              auth: { token: "" }, // Token will be injected by proxy
              nonce: crypto.randomUUID(),
            },
          };
          ws!.send(JSON.stringify(connectMsg));
        };

        ws.onmessage = (event) => {
          if (disposed) return;
          try {
            const data = JSON.parse(event.data);

            // Handle connect response
            if (data.result?.connected) {
              setConnected(true);
              setConnecting(false);

              // Fetch chat history
              const historyMsg = {
                id: nextRpcId(),
                method: "chat.history",
                params: {},
              };
              ws!.send(JSON.stringify(historyMsg));
              return;
            }

            // Handle chat history response
            if (data.result?.messages && Array.isArray(data.result.messages)) {
              const historyMessages: ChatMessage[] = data.result.messages.map(
                (m: { role: string; content: string }) => ({
                  id: nextMsgId(),
                  role: m.role as ChatMessage["role"],
                  content: m.content,
                  timestamp: new Date(),
                })
              );
              setMessages(historyMessages);
              return;
            }

            // Handle chat.message notification (assistant response)
            if (data.method === "chat.message" && data.params) {
              setMessages((prev) => [
                ...prev,
                {
                  id: nextMsgId(),
                  role: data.params.role || "assistant",
                  content: data.params.content || "",
                  timestamp: new Date(),
                },
              ]);
              return;
            }

            // Handle chat.send response
            if (data.result?.ok) {
              return;
            }

            // Handle errors
            if (data.error) {
              setMessages((prev) => [
                ...prev,
                {
                  id: nextMsgId(),
                  role: "system",
                  content: `Error: ${data.error.message || JSON.stringify(data.error)}`,
                  timestamp: new Date(),
                },
              ]);
            }
          } catch {
            // Non-JSON message, ignore
          }
        };

        ws.onclose = () => {
          if (disposed) return;
          setConnected(false);
          setConnecting(false);
        };

        ws.onerror = () => {
          if (disposed) return;
          setConnected(false);
          setConnecting(false);
        };
      } catch {
        setConnecting(false);
      }
    }

    connect();

    return () => {
      disposed = true;
      ws?.close();
      wsRef.current = null;
    };
  }, [agentId]);

  const sendMessage = () => {
    const text = input.trim();
    if (!text || !wsRef.current || !connected) return;

    // Add user message to UI
    setMessages((prev) => [
      ...prev,
      {
        id: nextMsgId(),
        role: "user",
        content: text,
        timestamp: new Date(),
      },
    ]);

    // Send via RPC
    const rpcMsg = {
      id: nextRpcId(),
      method: "chat.send",
      params: { content: text },
    };
    wsRef.current.send(JSON.stringify(rpcMsg));
    setInput("");
  };

  return (
    <div className="flex h-screen flex-col">
      {/* Header */}
      <div className="flex items-center gap-4 border-b border-zinc-800 px-4 py-3">
        <Link
          href={`/dashboard/agents/${agentId}`}
          className="text-sm text-zinc-500 hover:text-zinc-300"
        >
          &larr; Back
        </Link>
        <h1 className="text-sm font-medium">Chat</h1>
        <span
          className={`ml-auto inline-block h-2 w-2 rounded-full ${
            connected
              ? "bg-green-400"
              : connecting
                ? "bg-yellow-400 animate-pulse"
                : "bg-red-400"
          }`}
        />
        <span className="text-xs text-zinc-500">
          {connected
            ? "Connected"
            : connecting
              ? "Connecting..."
              : "Disconnected"}
        </span>
      </div>

      {/* Messages */}
      <div className="flex-1 overflow-y-auto p-4 space-y-4">
        {messages.length === 0 && !connecting && (
          <p className="text-center text-sm text-zinc-500 mt-8">
            Send a message to start chatting with your agent.
          </p>
        )}
        {messages.map((msg) => (
          <div
            key={msg.id}
            className={`flex ${
              msg.role === "user" ? "justify-end" : "justify-start"
            }`}
          >
            <div
              className={`max-w-[75%] rounded-lg px-4 py-2 text-sm ${
                msg.role === "user"
                  ? "bg-blue-600 text-white"
                  : msg.role === "system"
                    ? "bg-red-950 text-red-300 border border-red-900"
                    : "bg-zinc-800 text-zinc-200"
              }`}
            >
              <pre className="whitespace-pre-wrap font-sans">{msg.content}</pre>
            </div>
          </div>
        ))}
        <div ref={messagesEndRef} />
      </div>

      {/* Input */}
      <div className="border-t border-zinc-800 p-4">
        <div className="flex gap-2">
          <input
            type="text"
            value={input}
            onChange={(e) => setInput(e.target.value)}
            placeholder={
              connected ? "Type a message..." : "Waiting for connection..."
            }
            disabled={!connected}
            className="flex-1 rounded-md border border-zinc-700 bg-zinc-800 px-3 py-2.5 text-sm placeholder:text-zinc-500 focus:border-zinc-500 focus:outline-none disabled:opacity-50"
            onKeyDown={(e) => e.key === "Enter" && !e.shiftKey && sendMessage()}
          />
          <button
            onClick={sendMessage}
            disabled={!connected || !input.trim()}
            className="rounded-md bg-blue-600 px-4 py-2.5 text-sm font-medium hover:bg-blue-500 disabled:opacity-50 transition-colors"
          >
            Send
          </button>
        </div>
      </div>
    </div>
  );
}
