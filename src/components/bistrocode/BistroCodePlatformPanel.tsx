import { useEffect, useRef, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import {
  AlertTriangle,
  BarChart3,
  CreditCard,
  Edit,
  ExternalLink,
  Loader2,
  LogOut,
  Play,
  RefreshCw,
  TestTube2,
  UserRound,
} from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { ProviderIcon } from "@/components/ProviderIcon";
import {
  BISTROCODE_BASE_URL,
  bistrocodeApi,
  providersApi,
  settingsApi,
  universalProvidersApi,
} from "@/lib/api";
import { cn } from "@/lib/utils";
import { fmtInt, fmtUsd } from "@/components/usage/format";
import { useBistroCodeAuth } from "@/contexts/BistroCodeAuthContext";
import type { BistroCodeDesktopTokenConfig } from "@/lib/api/bistrocode";
import type { AppId } from "@/lib/api/types";
import type {
  Provider,
  UniversalProvider,
  UniversalProviderModels,
} from "@/types";

const DESKTOP_AUTHORIZE_PATH = "/desktop/authorize";
const DEFAULT_QUOTA_PER_USD = 500_000;
const BISTROCODE_PROVIDER_IDS: Record<string, string> = {
  claude: "bistrocode-claude",
  gpt: "bistrocode-gpt",
  gemini: "bistrocode-gemini",
};

export const BISTROCODE_MANAGED_PROVIDER_IDS = new Set(
  Object.values(BISTROCODE_PROVIDER_IDS).flatMap((id) => [
    `universal-claude-${id}`,
    `universal-cursor-${id}`,
    `universal-codex-${id}`,
    `universal-gemini-${id}`,
  ]),
);

const BISTROCODE_PROVIDER_APPS: Record<string, UniversalProvider["apps"]> = {
  claude: { claude: true, codex: false, gemini: false },
  gpt: { claude: false, codex: true, gemini: false },
  gemini: { claude: false, codex: false, gemini: true },
};

const BISTROCODE_MANAGED_APP_IDS = new Set<AppId>([
  "claude",
  "cursor",
  "codex",
  "gemini",
]);

const BISTROCODE_PROVIDER_MODELS: Record<string, UniversalProviderModels> = {
  claude: {
    claude: {
      model: "claude-sonnet-4-6",
      haikuModel: "claude-haiku-4-5-20251001",
      sonnetModel: "claude-sonnet-4-6",
      opusModel: "claude-opus-4-7",
    },
  },
  gpt: {
    codex: {
      model: "gpt-5.5",
      reasoningEffort: "high",
    },
  },
  gemini: {
    gemini: {
      model: "gemini-3.1-pro",
    },
  },
};

const getBistroCodeUrl = (path = "/") =>
  new URL(path, `${BISTROCODE_BASE_URL}/`).toString();

const openBistroCode = async (path = "/") => {
  const url = getBistroCodeUrl(path);

  try {
    await settingsApi.openExternal(url);
    return;
  } catch (error) {
    console.error("Failed to open BistroCode link", error);
  }

  const popup = window.open(url, "_blank", "noopener,noreferrer");
  if (!popup) {
    toast.error(`无法打开浏览器，请手动访问 ${url}`);
  }
};

function formatQuotaUsd(
  quota: number | undefined,
  quotaPerUsd = DEFAULT_QUOTA_PER_USD,
) {
  if (!Number.isFinite(quota) || quota == null) return "--";
  const divisor =
    Number.isFinite(quotaPerUsd) && quotaPerUsd > 0
      ? quotaPerUsd
      : DEFAULT_QUOTA_PER_USD;
  return fmtUsd(quota / divisor, 4);
}

function tokenConfigSignature(
  tokens: BistroCodeDesktopTokenConfig[] | undefined,
) {
  return (tokens ?? [])
    .map(
      (token) =>
        `${token.id}:${token.group}:${token.key}:${token.purpose}:${JSON.stringify(token.models ?? {})}`,
    )
    .sort()
    .join("|");
}

function createManagedProvider(
  token: BistroCodeDesktopTokenConfig,
): UniversalProvider | null {
  const purpose = token.purpose || "";
  const id = BISTROCODE_PROVIDER_IDS[purpose];
  const apps = BISTROCODE_PROVIDER_APPS[purpose];
  if (!id || !apps) return null;

  return {
    id,
    name: token.name,
    providerType: "bistrocode",
    apps,
    baseUrl: token.baseUrl || BISTROCODE_BASE_URL,
    apiKey: token.key,
    models: token.models ?? BISTROCODE_PROVIDER_MODELS[purpose] ?? {},
    websiteUrl: BISTROCODE_BASE_URL,
    icon: "bistrocode",
    iconColor: "#16A34A",
    notes: `BistroCode 自动配置，分组：${token.group || "default"}，模型规则：${token.modelMatch || "--"}`,
    meta: {
      providerType: "bistrocode",
      usage_script: {
        enabled: false,
        language: "javascript",
        code: "",
        timeout: 10,
        templateType: "newapi",
        autoQueryInterval: 0,
      },
    },
    createdAt: token.createdTime ? token.createdTime * 1000 : Date.now(),
  };
}

function appProviderPrefix(appId: string) {
  if (appId === "cursor") return "cursor";
  if (appId === "codex") return "codex";
  if (appId === "gemini") return "gemini";
  return "claude";
}

function modelConfigKeyForApp(appId: string) {
  if (appId === "cursor") return "claude";
  if (appId === "codex") return "codex";
  if (appId === "gemini") return "gemini";
  return "claude";
}

function purposeForApp(appId: string) {
  if (appId === "codex") return "gpt";
  if (appId === "gemini") return "gemini";
  return "claude";
}

function isBistroCodeManagedProviderId(appId: string, providerId?: string) {
  if (!providerId) return false;
  const expectedId = BISTROCODE_PROVIDER_IDS[purposeForApp(appId)];
  return providerId === `universal-${appProviderPrefix(appId)}-${expectedId}`;
}

export function selectBistroCodeManagedProvider(
  providers: Record<string, Provider>,
  appId: string,
): Provider | null {
  const id = BISTROCODE_PROVIDER_IDS[purposeForApp(appId)];
  if (!id) return null;
  return providers[`universal-${appProviderPrefix(appId)}-${id}`] ?? null;
}

export function filterBistroCodeManagedProviders(
  providers: Record<string, Provider>,
): Record<string, Provider> {
  return Object.fromEntries(
    Object.entries(providers).filter(
      ([id]) => !BISTROCODE_MANAGED_PROVIDER_IDS.has(id),
    ),
  );
}

function providerFromTokenForApp(
  tokens: BistroCodeDesktopTokenConfig[],
  appId: string,
): Provider | null {
  const purpose = purposeForApp(appId);
  const token = tokens.find((item) => item.purpose === purpose);
  const universal = token ? createManagedProvider(token) : null;
  if (!token || !universal) return null;

  const childApp = appProviderPrefix(appId);
  const modelKey = modelConfigKeyForApp(appId);
  const model = universal.models[modelKey as keyof UniversalProviderModels];
  let settingsConfig: Provider["settingsConfig"];

  if (childApp === "codex") {
    const codexBaseUrl = universal.baseUrl.endsWith("/v1")
      ? universal.baseUrl
      : `${universal.baseUrl.replace(/\/+$/, "")}/v1`;
    const codexModel =
      model && "model" in model ? (model.model ?? "gpt-5.5") : "gpt-5.5";
    const reasoningEffort =
      model && "reasoningEffort" in model
        ? (model.reasoningEffort ?? "high")
        : "high";
    settingsConfig = {
      auth: { OPENAI_API_KEY: token.key },
      config: `model_provider = "newapi"
model = "${codexModel}"
model_reasoning_effort = "${reasoningEffort}"
disable_response_storage = true

[model_providers.newapi]
name = "NewAPI"
base_url = "${codexBaseUrl}"
wire_api = "responses"
requires_openai_auth = true`,
    };
  } else if (childApp === "gemini") {
    settingsConfig = {
      env: {
        GOOGLE_GEMINI_BASE_URL: universal.baseUrl,
        GEMINI_API_KEY: token.key,
        GEMINI_MODEL:
          model && "model" in model
            ? (model.model ?? "gemini-3.1-pro")
            : "gemini-3.1-pro",
      },
    };
  } else {
    settingsConfig = {
      env: {
        ANTHROPIC_BASE_URL: universal.baseUrl,
        ANTHROPIC_AUTH_TOKEN: token.key,
        ANTHROPIC_MODEL:
          model && "model" in model
            ? (model.model ?? "claude-sonnet-4-6")
            : "claude-sonnet-4-6",
        ANTHROPIC_DEFAULT_HAIKU_MODEL:
          model && "haikuModel" in model
            ? (model.haikuModel ?? "claude-haiku-4-5-20251001")
            : "claude-haiku-4-5-20251001",
        ANTHROPIC_DEFAULT_SONNET_MODEL:
          model && "sonnetModel" in model
            ? (model.sonnetModel ?? "claude-sonnet-4-6")
            : "claude-sonnet-4-6",
        ANTHROPIC_DEFAULT_OPUS_MODEL:
          model && "opusModel" in model
            ? (model.opusModel ?? "claude-opus-4-7")
            : "claude-opus-4-7",
      },
    };
  }

  return {
    id: `universal-${childApp}-${universal.id}`,
    name: universal.name,
    settingsConfig,
    websiteUrl: universal.websiteUrl,
    category: "aggregator",
    createdAt: universal.createdAt,
    notes: universal.notes,
    meta: {
      ...universal.meta,
      apiFormat: modelKey === "claude" ? "openai_responses" : undefined,
    },
    icon: universal.icon,
    iconColor: universal.iconColor,
  };
}

export interface BistroCodePlatformPanelProps {
  appId?: string;
  providers?: Record<string, Provider>;
  currentProviderId?: string;
  onSwitch?: (provider: Provider) => void;
  onEdit?: (provider: Provider) => void;
  onTest?: (provider: Provider) => void;
  isTesting?: boolean;
  onConfigureUsage?: (provider: Provider) => void;
}

export function BistroCodePlatformPanel({
  appId = "claude",
  providers = {},
  currentProviderId,
  onSwitch,
  onEdit,
  onTest,
  isTesting = false,
  onConfigureUsage,
}: BistroCodePlatformPanelProps) {
  const {
    accessToken,
    userId,
    account: linkedAccount,
    authStatus,
    authError,
    isAuthenticated,
    syncAccount,
    clearAccount,
    rotateAuthState,
  } = useBistroCodeAuth();
  const queryClient = useQueryClient();
  const syncedTokenSignatureRef = useRef("");
  const reappliedCurrentSignatureRef = useRef("");
  const [providerSyncError, setProviderSyncError] = useState<string | null>(
    null,
  );

  const accountQuery = useQuery({
    queryKey: ["bistrocode", "account", userId],
    queryFn: () => bistrocodeApi.getAccount(accessToken, userId),
    enabled: isAuthenticated,
    staleTime: 60_000,
    retry: false,
  });

  const tokenConfigsQuery = useQuery({
    queryKey: ["bistrocode", "desktop-tokens", userId],
    queryFn: () => bistrocodeApi.ensureDefaultTokens(accessToken, userId),
    enabled: isAuthenticated,
    staleTime: 5 * 60_000,
    retry: false,
  });

  useEffect(() => {
    const data = accountQuery.data;
    if (!data?.loggedIn || !data.account || !accessToken.trim()) return;
    syncAccount({
      id: Number(data.account.id ?? Number(userId)),
      username: data.account.username ?? "",
      displayName: data.account.displayName,
      email: data.account.email,
      group: data.account.group,
      quota: Number(data.account.quota ?? 0),
      usedQuota: Number(data.account.usedQuota ?? 0),
      requestCount: Number(data.account.requestCount ?? 0),
      quotaPerUsd: data.account.quotaPerUsd,
      accessToken: accessToken.trim(),
      desktopTokens: linkedAccount?.desktopTokens,
    });
    void queryClient.invalidateQueries({ queryKey: ["usage"] });
  }, [
    accountQuery.data,
    accessToken,
    linkedAccount?.desktopTokens,
    queryClient,
    syncAccount,
    userId,
  ]);

  useEffect(() => {
    if (!linkedAccount || !tokenConfigsQuery.data?.length) return;
    if (
      tokenConfigSignature(linkedAccount.desktopTokens) ===
      tokenConfigSignature(tokenConfigsQuery.data)
    ) {
      return;
    }
    syncAccount({
      ...linkedAccount,
      desktopTokens: tokenConfigsQuery.data,
    });
  }, [linkedAccount, syncAccount, tokenConfigsQuery.data]);

  useEffect(() => {
    const tokens = tokenConfigsQuery.data ?? linkedAccount?.desktopTokens ?? [];
    if (!isAuthenticated || !tokens.length) return;

    const signature = tokenConfigSignature(tokens);
    if (!signature || syncedTokenSignatureRef.current === signature) return;

    syncedTokenSignatureRef.current = signature;
    let cancelled = false;

    const syncManagedProviders = async () => {
      try {
        for (const token of tokens) {
          const provider = createManagedProvider(token);
          if (!provider) continue;
          await universalProvidersApi.upsert(provider);
          await universalProvidersApi.sync(provider.id);
        }
        const cursorProvider = providerFromTokenForApp(tokens, "cursor");
        if (cursorProvider) {
          await providersApi.add(cursorProvider, "cursor", false);
        }
        const shouldReapplyCurrent =
          BISTROCODE_MANAGED_APP_IDS.has(appId as AppId) &&
          isBistroCodeManagedProviderId(appId, currentProviderId);
        const currentAppProvider = shouldReapplyCurrent
          ? providerFromTokenForApp(tokens, appId)
          : null;
        const reapplySignature = `${appId}:${currentProviderId}:${signature}`;
        if (
          currentAppProvider &&
          reappliedCurrentSignatureRef.current !== reapplySignature
        ) {
          reappliedCurrentSignatureRef.current = reapplySignature;
          console.info("[BistroAuth] reapplying current managed provider", {
            appId,
            providerId: currentAppProvider.id,
          });
          await providersApi.add(currentAppProvider, appId as AppId, false);
          await providersApi.switch(currentAppProvider.id, appId as AppId);
        }
        if (cancelled) return;
        setProviderSyncError(null);
        await Promise.all([
          queryClient.invalidateQueries({ queryKey: ["providers", "claude"] }),
          queryClient.invalidateQueries({ queryKey: ["providers", "cursor"] }),
          queryClient.invalidateQueries({ queryKey: ["providers", "codex"] }),
          queryClient.invalidateQueries({ queryKey: ["providers", "gemini"] }),
        ]);
        await universalProvidersApi.getAll().catch(() => undefined);
      } catch (error) {
        if (cancelled) return;
        syncedTokenSignatureRef.current = "";
        setProviderSyncError(
          error instanceof Error ? error.message : String(error),
        );
      }
    };

    void syncManagedProviders();

    return () => {
      cancelled = true;
    };
  }, [
    isAuthenticated,
    appId,
    currentProviderId,
    linkedAccount?.desktopTokens,
    queryClient,
    tokenConfigsQuery.data,
  ]);

  const account = accountQuery.data?.account ?? linkedAccount ?? undefined;
  const isConnected =
    (accountQuery.data?.loggedIn ?? false) ||
    isAuthenticated ||
    Boolean(account);
  const isConnecting = authStatus === "authorizing";
  const isRefreshing = accountQuery.isFetching;
  const isTokenRefreshing = tokenConfigsQuery.isFetching;
  const quotaPerUsd = account?.quotaPerUsd ?? DEFAULT_QUOTA_PER_USD;
  const currentQuota = Math.max(0, account?.quota ?? 0);
  const desktopTokens = linkedAccount?.desktopTokens ?? [];
  const activeManagedProvider =
    selectBistroCodeManagedProvider(providers, appId) ??
    providerFromTokenForApp(desktopTokens, appId);
  const managedProviderReady = Boolean(activeManagedProvider);
  const isManagedCurrent =
    Boolean(activeManagedProvider?.id) &&
    activeManagedProvider?.id === currentProviderId;

  const ensureManagedProviderForSwitch = async (provider: Provider) => {
    if (appId !== "cursor") return;
    if (providers[provider.id]) return;

    await providersApi.add(provider, appId, false);
    await queryClient.invalidateQueries({ queryKey: ["providers", appId] });
  };

  const handlePrimaryAction = async () => {
    if (!isConnected) {
      await startAuthorize();
      return;
    }

    if (!activeManagedProvider || !onSwitch) return;

    try {
      await ensureManagedProviderForSwitch(activeManagedProvider);
      await onSwitch(activeManagedProvider);
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      toast.error(`切换 BistroCode 默认配置失败：${message}`);
    }
  };

  useEffect(() => {
    if (!isAuthenticated || accountQuery.isFetching) return;
    if (accountQuery.data?.loggedIn === false) {
      const message =
        accountQuery.data.message || "BistroCode 授权已失效，请重新连接账号";
      clearAccount();
      toast.error(message);
    }
  }, [
    accountQuery.data?.loggedIn,
    accountQuery.data?.message,
    accountQuery.isFetching,
    clearAccount,
    isAuthenticated,
  ]);

  const startAuthorize = () => {
    const nextState = rotateAuthState();
    const params = new URLSearchParams({
      state: nextState,
      ts: String(Date.now()),
      return_to: "/console",
    });
    console.info("[BistroAuth] opening desktop authorize URL", {
      hasState: Boolean(nextState),
    });
    return openBistroCode(`${DESKTOP_AUTHORIZE_PATH}?${params.toString()}`);
  };

  const refreshAccount = () => {
    console.info("[BistroAuth] refresh account requested");
    void accountQuery.refetch();
    void tokenConfigsQuery.refetch();
    void queryClient.invalidateQueries({ queryKey: ["usage"] });
  };

  return (
    <section
      className={cn(
        "rounded-lg border bg-background/70 p-4",
        isManagedCurrent ? "border-emerald-500/50" : "border-border/70",
      )}
    >
      <div className="flex flex-col gap-4 lg:flex-row lg:items-start lg:justify-between">
        <div className="flex min-w-0 items-start gap-3">
          <ProviderIcon icon="bistrocode" name="BistroCode" size={40} />
          <div className="min-w-0">
            <div className="flex flex-wrap items-center gap-2">
              <h2 className="text-base font-semibold text-foreground">
                BistroCode 平台
              </h2>
              <Badge
                variant="outline"
                className={cn(
                  "border-amber-500/30 bg-amber-500/10 text-amber-700 dark:text-amber-300",
                  isConnected &&
                    "border-emerald-500/30 bg-emerald-500/10 text-emerald-700 dark:text-emerald-300",
                  isConnecting &&
                    "border-sky-500/30 bg-sky-500/10 text-sky-700 dark:text-sky-300",
                )}
              >
                {isConnecting
                  ? "正在连接"
                  : isConnected
                    ? "账号已连接"
                    : "未连接账号"}
              </Badge>
            </div>
            <p className="mt-1 text-xs text-muted-foreground">
              通过网页授权连接账号，应用会自动保存查询所需凭据。余额和用量按
              BistroCode 云端数据看板展示。
            </p>
            {account && (
              <div className="mt-2 flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
                <span className="inline-flex items-center gap-1 font-medium text-foreground">
                  <UserRound className="h-3.5 w-3.5" />
                  {account.displayName ||
                    account.username ||
                    `用户 ${account.id}`}
                  <span className="text-muted-foreground">ID {account.id}</span>
                </span>
                {account.email && <span>{account.email}</span>}
              </div>
            )}
          </div>
        </div>

        <div className="flex flex-wrap items-center gap-1.5">
          <Button
            size="sm"
            onClick={() => void handlePrimaryAction()}
            disabled={isConnecting || (isConnected && !activeManagedProvider)}
          >
            {isConnecting ? (
              <Loader2 className="h-4 w-4 animate-spin" />
            ) : (
              <Play className="h-4 w-4" />
            )}
            {isConnected
              ? isManagedCurrent
                ? "默认使用中"
                : "设为默认"
              : "连接账号"}
          </Button>
          {isConnected && (
            <>
              <Button
                size="icon"
                variant="ghost"
                className="h-8 w-8"
                title="编辑配置"
                disabled={!managedProviderReady}
                onClick={() =>
                  activeManagedProvider && onEdit?.(activeManagedProvider)
                }
              >
                <Edit className="h-4 w-4" />
              </Button>
              <Button
                size="icon"
                variant="ghost"
                className="h-8 w-8"
                title="测试模型"
                disabled={!managedProviderReady || isTesting}
                onClick={() =>
                  activeManagedProvider && onTest?.(activeManagedProvider)
                }
              >
                {isTesting ? (
                  <Loader2 className="h-4 w-4 animate-spin" />
                ) : (
                  <TestTube2 className="h-4 w-4" />
                )}
              </Button>
              <Button
                size="icon"
                variant="ghost"
                className="h-8 w-8"
                title="配置用量"
                disabled={!managedProviderReady}
                onClick={() =>
                  activeManagedProvider &&
                  onConfigureUsage?.(activeManagedProvider)
                }
              >
                <BarChart3 className="h-4 w-4" />
              </Button>
              <Button
                size="icon"
                variant="ghost"
                className="h-8 w-8"
                title="更多数据"
                onClick={() => void openBistroCode("/console")}
              >
                <ExternalLink className="h-4 w-4" />
              </Button>
            </>
          )}
          <Button
            size="sm"
            variant="outline"
            onClick={refreshAccount}
            disabled={!isConnected || isRefreshing || isTokenRefreshing}
          >
            {isRefreshing || isTokenRefreshing ? (
              <Loader2 className="h-4 w-4 animate-spin" />
            ) : (
              <RefreshCw className="h-4 w-4" />
            )}
            刷新
          </Button>
          <Button
            size="sm"
            variant="outline"
            onClick={() => void openBistroCode("/console/topup")}
          >
            <CreditCard className="h-4 w-4" />
            充值
          </Button>
          {isConnected && (
            <Button
              size="sm"
              variant="ghost"
              onClick={() => {
                console.info("[BistroAuth] disconnect requested from panel");
                clearAccount();
              }}
            >
              <LogOut className="h-4 w-4" />
              断开
            </Button>
          )}
        </div>
      </div>

      <div className="mt-4 grid gap-2 md:grid-cols-3">
        <Metric
          label="当前余额"
          value={formatQuotaUsd(currentQuota, quotaPerUsd)}
          subValue={`${fmtInt(currentQuota)} 额度`}
        />
        <Metric
          label="累计已用"
          value={formatQuotaUsd(account?.usedQuota, quotaPerUsd)}
          subValue={`${fmtInt(account?.usedQuota ?? 0)} 额度`}
        />
        <Metric
          label="请求次数"
          value={fmtInt(account?.requestCount ?? 0)}
          subValue={account ? `账号 ID ${account.id ?? userId}` : "--"}
        />
      </div>

      {desktopTokens.length ? (
        <>
          <div className="mt-4 text-xs font-medium text-muted-foreground">
            默认 API Key 配置项
          </div>
          <div className="mt-3 grid gap-2 md:grid-cols-3">
            {desktopTokens.map((token) => (
              <TokenConfigCard key={token.id} token={token} />
            ))}
          </div>
        </>
      ) : isConnected ? (
        <div className="mt-3 rounded-md border border-border/70 bg-muted/20 px-3 py-2 text-xs text-muted-foreground">
          正在准备默认 API Key 配置项。点击“刷新”可重新同步。
        </div>
      ) : null}

      {(authError ||
        accountQuery.data?.message ||
        accountQuery.error ||
        providerSyncError) && (
        <div className="mt-3 rounded-md border border-amber-500/20 bg-amber-500/10 px-3 py-2 text-xs text-amber-700 dark:text-amber-300">
          <div className="flex items-center gap-2 font-medium">
            <AlertTriangle className="h-4 w-4" />
            BistroCode 提示
          </div>
          <div className="mt-1 space-y-1">
            {authError && <div>{authError}</div>}
            {accountQuery.data?.message && (
              <div>{accountQuery.data.message}</div>
            )}
            {accountQuery.error && <div>{String(accountQuery.error)}</div>}
            {providerSyncError && (
              <div>同步默认工具配置失败：{providerSyncError}</div>
            )}
          </div>
        </div>
      )}
    </section>
  );
}

function maskKey(key: string) {
  const clean = key.trim();
  if (clean.length <= 12) return clean ? "********" : "--";
  return `${clean.slice(0, 6)}...${clean.slice(-4)}`;
}

function tokenPurposeLabel(purpose: string) {
  if (purpose === "claude") return "Claude 分组";
  if (purpose === "gpt") return "GPT/Codex 分组";
  if (purpose === "gemini") return "Gemini 分组";
  return "平台分组";
}

function TokenConfigCard({ token }: { token: BistroCodeDesktopTokenConfig }) {
  return (
    <div className="rounded-md border border-border/70 bg-muted/20 px-3 py-2">
      <div className="flex items-center justify-between gap-2">
        <div className="truncate text-[11px] text-muted-foreground">
          {tokenPurposeLabel(token.purpose)}
        </div>
        <Badge variant="outline" className="h-5 px-1.5 text-[10px]">
          {token.group || "default"}
        </Badge>
      </div>
      <div className="mt-1 truncate text-sm font-semibold text-foreground">
        {token.name}
      </div>
      <div className="mt-0.5 truncate font-mono text-[11px] text-muted-foreground">
        {maskKey(token.key)}
      </div>
      <div className="mt-1 truncate text-[11px] text-muted-foreground">
        模型规则 {token.modelMatch || "--"}
      </div>
    </div>
  );
}

function Metric({
  label,
  value,
  subValue,
  loading,
}: {
  label: string;
  value: string;
  subValue?: string;
  loading?: boolean;
}) {
  return (
    <div className="rounded-md border border-border/70 bg-muted/20 px-3 py-2">
      <div className="text-[11px] text-muted-foreground">{label}</div>
      <div className="mt-1 flex min-h-5 items-center gap-2 truncate text-sm font-semibold text-foreground">
        {loading && (
          <Loader2 className="h-3.5 w-3.5 animate-spin text-muted-foreground" />
        )}
        <span className="truncate">{value}</span>
      </div>
      {subValue && (
        <div className="mt-0.5 truncate text-[11px] text-muted-foreground">
          {subValue}
        </div>
      )}
    </div>
  );
}
