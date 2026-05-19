import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { listen } from "@tauri-apps/api/event";
import { toast } from "sonner";
import { invoke } from "@tauri-apps/api/core";
import type {
  BistroCodeAuthUser,
  BistroCodeDesktopTokenConfig,
} from "@/lib/api/bistrocode";

const ACCESS_TOKEN_STORAGE_KEY = "bistrocode-access-token";
const USER_ID_STORAGE_KEY = "bistrocode-user-id";
const AUTH_STATE_STORAGE_KEY = "bistrocode-auth-state";
const ACCOUNT_STORAGE_KEY = "bistrocode-account";

export interface BistroCodeAccountState {
  id: number;
  username: string;
  displayName?: string;
  email?: string;
  group?: string;
  quota: number;
  usedQuota: number;
  requestCount: number;
  quotaPerUsd?: number;
  accessToken: string;
  desktopTokens?: BistroCodeDesktopTokenConfig[];
}

interface BistroCodeAuthContextValue {
  authState: string;
  accessToken: string;
  userId: string;
  account: BistroCodeAccountState | null;
  authStatus: "idle" | "authorizing" | "connected" | "error";
  authError: string | null;
  isAuthenticated: boolean;
  syncAccount: (next: BistroCodeAccountState | null) => void;
  refreshFromStorage: () => void;
  rotateAuthState: () => string;
  clearAccount: () => void;
}

const BistroCodeAuthContext = createContext<
  BistroCodeAuthContextValue | undefined
>(undefined);

function createAuthState() {
  const value = crypto.getRandomValues(new Uint32Array(2)).join("-");
  localStorage.setItem(AUTH_STATE_STORAGE_KEY, value);
  return value;
}

function normalizeAccount(value: unknown): BistroCodeAccountState | null {
  if (!value || typeof value !== "object") return null;
  const data = value as Record<string, unknown>;
  const id = Number(data.id);
  if (!Number.isFinite(id) || id <= 0) return null;
  return {
    id,
    username: String(data.username ?? ""),
    displayName:
      typeof data.displayName === "string"
        ? data.displayName
        : typeof data.display_name === "string"
          ? data.display_name
          : undefined,
    email: typeof data.email === "string" ? data.email : undefined,
    group: typeof data.group === "string" ? data.group : undefined,
    quota: Number(data.quota ?? 0),
    usedQuota: Number(data.usedQuota ?? data.used_quota ?? 0),
    requestCount: Number(data.requestCount ?? data.request_count ?? 0),
    quotaPerUsd:
      Number(data.quotaPerUsd ?? data.quota_per_usd ?? 0) || undefined,
    accessToken: String(data.accessToken ?? data.access_token ?? ""),
    desktopTokens: Array.isArray(data.desktopTokens)
      ? data.desktopTokens
          .map((item) => normalizeDesktopToken(item))
          .filter((item): item is BistroCodeDesktopTokenConfig => Boolean(item))
      : Array.isArray(data.desktop_tokens)
        ? data.desktop_tokens
            .map((item) => normalizeDesktopToken(item))
            .filter((item): item is BistroCodeDesktopTokenConfig =>
              Boolean(item),
            )
        : undefined,
  };
}

function normalizeDesktopToken(
  value: unknown,
): BistroCodeDesktopTokenConfig | null {
  if (!value || typeof value !== "object") return null;
  const data = value as Record<string, unknown>;
  const id = Number(data.id);
  const key = String(data.key ?? "");
  if (!Number.isFinite(id) || id <= 0 || !key.trim()) return null;
  return {
    id,
    name: String(data.name ?? ""),
    key,
    group: String(data.group ?? ""),
    purpose: String(data.purpose ?? ""),
    modelMatch: String(data.modelMatch ?? data.model_match ?? ""),
    baseUrl: String(data.baseUrl ?? data.base_url ?? ""),
    createdTime: Number(data.createdTime ?? data.created_time ?? 0),
  };
}

export function BistroCodeAuthProvider({
  children,
}: {
  children: React.ReactNode;
}) {
  const [authState, setAuthState] = useState(createAuthState);
  const handledCodesRef = useRef<Set<string>>(new Set());
  const [accessToken, setAccessToken] = useState(
    () => localStorage.getItem(ACCESS_TOKEN_STORAGE_KEY) ?? "",
  );
  const [userId, setUserId] = useState(
    () => localStorage.getItem(USER_ID_STORAGE_KEY) ?? "",
  );
  const [account, setAccount] = useState<BistroCodeAccountState | null>(() => {
    try {
      return normalizeAccount(
        JSON.parse(localStorage.getItem(ACCOUNT_STORAGE_KEY) ?? "null"),
      );
    } catch {
      return null;
    }
  });
  const [authStatus, setAuthStatus] = useState<
    "idle" | "authorizing" | "connected" | "error"
  >(() => (account ? "connected" : "idle"));
  const [authError, setAuthError] = useState<string | null>(null);
  const authStateRef = useRef(authState);

  useEffect(() => {
    authStateRef.current = authState;
  }, [authState]);

  const syncAccount = useCallback((next: BistroCodeAccountState | null) => {
    setAccount(next);
    const nextToken = next?.accessToken.trim() ?? "";
    const nextUserId = next ? String(next.id) : "";
    setAccessToken(nextToken);
    setUserId(nextUserId);

    if (nextToken) {
      localStorage.setItem(ACCESS_TOKEN_STORAGE_KEY, nextToken);
    } else {
      localStorage.removeItem(ACCESS_TOKEN_STORAGE_KEY);
    }

    if (nextUserId) {
      localStorage.setItem(USER_ID_STORAGE_KEY, nextUserId);
    } else {
      localStorage.removeItem(USER_ID_STORAGE_KEY);
    }

    if (next) {
      localStorage.setItem(ACCOUNT_STORAGE_KEY, JSON.stringify(next));
      setAuthStatus("connected");
      setAuthError(null);
    } else {
      localStorage.removeItem(ACCOUNT_STORAGE_KEY);
      setAuthStatus("idle");
    }
  }, []);

  const clearAccount = useCallback(() => {
    syncAccount(null);
    setAuthState(createAuthState());
    setAuthError(null);
  }, [syncAccount]);

  const rotateAuthState = useCallback(() => {
    const next = createAuthState();
    authStateRef.current = next;
    setAuthState(next);
    setAuthError(null);
    return next;
  }, []);

  const refreshFromStorage = useCallback(() => {
    setAccessToken(localStorage.getItem(ACCESS_TOKEN_STORAGE_KEY) ?? "");
    setUserId(localStorage.getItem(USER_ID_STORAGE_KEY) ?? "");
    try {
      setAccount(
        normalizeAccount(
          JSON.parse(localStorage.getItem(ACCOUNT_STORAGE_KEY) ?? "null"),
        ),
      );
    } catch {
      setAccount(null);
    }
  }, []);

  useEffect(() => {
    let unsubscribe: (() => void) | undefined;
    let active = true;

    const setup = async () => {
      const off = await listen<string>(
        "bistrocode-auth-link",
        async (event) => {
          const raw = event.payload;
          try {
            const url = new URL(raw);
            const code = url.searchParams.get("code") ?? "";
            const state = url.searchParams.get("state") ?? "";
            if (!code || state !== authStateRef.current) {
              setAuthStatus("error");
              setAuthError("授权链接无效或已过期");
              toast.error("授权链接无效或已过期");
              return;
            }
            if (handledCodesRef.current.has(code)) {
              return;
            }
            handledCodesRef.current.add(code);
            setAuthStatus("authorizing");
            setAuthError(null);
            const result = (await invoke("exchange_bistrocode_auth_code", {
              code,
              state,
            })) as {
              success?: boolean;
              data?: BistroCodeAuthUser;
              message?: string;
            };
            if (
              !result?.success ||
              !result.data?.accessToken ||
              !result.data?.id
            ) {
              const message = result?.message || "授权失败";
              setAuthStatus("error");
              setAuthError(message);
              setAuthState(createAuthState());
              toast.error(message);
              return;
            }
            syncAccount({
              id: Number(result.data.id),
              username: result.data.username ?? "",
              displayName: result.data.displayName,
              email: result.data.email,
              group: result.data.group,
              quota: Number(result.data.quota ?? 0),
              usedQuota: Number(result.data.usedQuota ?? 0),
              requestCount: Number(result.data.requestCount ?? 0),
              quotaPerUsd: 500_000,
              accessToken: result.data.accessToken,
            });
            toast.success("BistroCode 账号已连接");
          } catch (error) {
            console.error("Failed to handle BistroCode auth link", error);
            const message =
              error instanceof Error
                ? error.message
                : typeof error === "string"
                  ? error
                  : "处理授权回调失败";
            setAuthStatus("error");
            setAuthError(message);
            setAuthState(createAuthState());
            toast.error(message);
          }
        },
      );
      if (!active) {
        off();
        return;
      }
      unsubscribe = off;
    };

    void setup();
    return () => {
      active = false;
      unsubscribe?.();
    };
  }, [syncAccount]);

  const value = useMemo(
    () => ({
      authState,
      accessToken,
      userId,
      account,
      authStatus,
      authError,
      isAuthenticated: Boolean(accessToken.trim() && userId.trim()),
      syncAccount,
      refreshFromStorage,
      rotateAuthState,
      clearAccount,
    }),
    [
      account,
      accessToken,
      authError,
      authState,
      authStatus,
      clearAccount,
      refreshFromStorage,
      rotateAuthState,
      syncAccount,
      userId,
    ],
  );

  return (
    <BistroCodeAuthContext.Provider value={value}>
      {children}
    </BistroCodeAuthContext.Provider>
  );
}

export function useBistroCodeAuth() {
  const context = useContext(BistroCodeAuthContext);
  if (!context) {
    throw new Error(
      "useBistroCodeAuth must be used within BistroCodeAuthProvider",
    );
  }
  return context;
}
