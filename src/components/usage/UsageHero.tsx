import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import { motion } from "framer-motion";
import {
  Activity,
  BadgeDollarSign,
  ExternalLink,
  Loader2,
  PiggyBank,
  RefreshCw,
  TrendingUp,
  Wallet,
} from "lucide-react";
import { useQueryClient } from "@tanstack/react-query";
import { Card, CardContent } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { useBistroCodeAuth } from "@/contexts/BistroCodeAuthContext";
import { dashboardKeys, useCloudUserQuotaDates } from "@/lib/query/dashboard";
import { usageKeys, useBistroCodeUsageEstimate } from "@/lib/query/usage";
import { settingsApi } from "@/lib/api";
import type { QuotaDataItem } from "@/lib/api/dashboard";
import type { UsageRangeSelection } from "@/types/usage";
import { fmtInt, fmtUsd } from "./format";

interface UsageHeroProps {
  range: UsageRangeSelection;
  refreshIntervalMs?: number;
}

const SPARKLINE_BUCKETS = 12;

type SparklineKey = "balance" | "usage" | "requests";

function getBucketIndex(
  timestamp: number,
  start: number,
  end: number,
  bucketCount: number,
) {
  if (end <= start) return 0;
  const ratio = (timestamp - start) / (end - start);
  return Math.min(bucketCount - 1, Math.max(0, Math.floor(ratio * bucketCount)));
}

function buildSparklines(
  data: QuotaDataItem[],
  currentBalance: number,
): Record<SparklineKey, number[]> {
  const usage = Array.from({ length: SPARKLINE_BUCKETS }, () => 0);
  const requests = Array.from({ length: SPARKLINE_BUCKETS }, () => 0);
  const timestamps = data
    .map((item) => Number(item.created_at) || 0)
    .filter((value) => value > 0);
  const start = timestamps.length ? Math.min(...timestamps) : 0;
  const end = timestamps.length ? Math.max(...timestamps) : 1;

  for (const item of data) {
    const timestamp = Number(item.created_at) || start;
    const index = getBucketIndex(timestamp, start, end, SPARKLINE_BUCKETS);
    usage[index] += Number(item.quota) || 0;
    requests[index] += Number(item.count) || 0;
  }

  let balance = currentBalance;
  const balanceTrend = Array.from({ length: SPARKLINE_BUCKETS }, () => 0);
  for (let index = SPARKLINE_BUCKETS - 1; index >= 0; index--) {
    balanceTrend[index] = Math.max(0, balance);
    balance += usage[index];
  }

  return {
    balance: balanceTrend,
    usage,
    requests,
  };
}

function Sparkline({ values }: { values: number[] }) {
  const max = Math.max(...values, 1);

  return (
    <div className="mt-3 flex h-7 items-end gap-1">
      {values.map((value, index) => (
        <span
          key={index}
          className="flex-1 rounded-t-sm bg-primary/65"
          style={{ height: `${Math.max(10, (value / max) * 100)}%` }}
        />
      ))}
    </div>
  );
}

function MiniCard({
  label,
  value,
  description,
  values,
  icon: Icon,
}: {
  label: string;
  value: string;
  description: string;
  values: number[];
  icon: React.ComponentType<{ className?: string }>;
}) {
  return (
    <div className="rounded-lg border border-border/70 bg-background/60 p-3">
      <div className="flex items-center gap-1.5 text-xs text-muted-foreground">
        <Icon className="size-3.5" />
        <span>{label}</span>
      </div>
      <div className="mt-1 font-mono text-xl font-semibold tabular-nums">
        {value}
      </div>
      <div className="mt-0.5 text-xs text-muted-foreground">
        {description}
      </div>
      <Sparkline values={values} />
    </div>
  );
}

export function UsageHero({
  range,
  refreshIntervalMs = 0,
}: UsageHeroProps) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const { account, isAuthenticated, accessToken, userId } = useBistroCodeAuth();

  const { data, isLoading, error } = useCloudUserQuotaDates(range, {
    accessToken,
    userId,
    enabled: isAuthenticated,
    refetchInterval: refreshIntervalMs > 0 ? refreshIntervalMs : false,
  });
  const {
    data: estimate,
    isLoading: estimateLoading,
    error: estimateError,
  } = useBistroCodeUsageEstimate(range, {
    refetchInterval: false,
  });

  const refreshUsage = () => {
    queryClient.invalidateQueries({ queryKey: usageKeys.all });
    queryClient.invalidateQueries({ queryKey: dashboardKeys.all });
  };

  const openDashboard = async () => {
    try {
      await settingsApi.openExternal("https://bistrocode.online/console");
    } catch {
      window.open("https://bistrocode.online/console", "_blank", "noopener,noreferrer");
    }
  };

  const values = useMemo(() => {
    const rows = data ?? [];
    const currentBalance = Number(account?.quota ?? 0);
    const rangeUsedQuota = rows.reduce(
      (sum, item) => sum + (Number(item.quota) || 0),
      0,
    );
    const rangeRequests = rows.reduce(
      (sum, item) => sum + (Number(item.count) || 0),
      0,
    );
    const rangeTokens = rows.reduce(
      (sum, item) => sum + (Number(item.token_used) || 0),
      0,
    );

    return {
      currentBalance,
      usedQuota: Number(account?.usedQuota ?? rangeUsedQuota),
      requestCount: Number(account?.requestCount ?? rangeRequests),
      rangeUsedQuota,
      rangeRequests,
      rangeTokens,
      officialCost: Number(estimate?.officialCostUsd ?? 0),
      bistrocodeCost: Number(estimate?.bistrocodeCostUsd ?? 0),
      estimatedSavings: Number(estimate?.estimatedSavingsUsd ?? 0),
      sparklines: buildSparklines(rows, currentBalance),
    };
  }, [
    account?.quota,
    account?.requestCount,
    account?.usedQuota,
    data,
    estimate?.bistrocodeCostUsd,
    estimate?.estimatedSavingsUsd,
    estimate?.officialCostUsd,
  ]);

  if (!isAuthenticated) {
    return (
      <Card className="border border-border/50 bg-card/40 backdrop-blur-sm">
        <CardContent className="flex min-h-[200px] items-center justify-center">
          <div className="text-sm text-muted-foreground">
            请先连接 BistroCode 平台账号
          </div>
        </CardContent>
      </Card>
    );
  }

  if (isLoading) {
    return (
      <Card className="border border-border/50 bg-card/40 backdrop-blur-sm">
        <CardContent className="flex min-h-[200px] items-center justify-center">
          <Loader2 className="h-6 w-6 animate-spin text-muted-foreground/50" />
        </CardContent>
      </Card>
    );
  }

  return (
    <motion.div
      initial={{ opacity: 0, y: 10 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ duration: 0.4 }}
      className="grid gap-4"
    >
      <Card className="overflow-hidden border border-border/50 bg-card/60">
        <CardContent className="p-4 sm:p-5">
          <div className="flex flex-wrap items-start justify-between gap-3">
            <div className="flex flex-col gap-1">
              <h3 className="text-base font-semibold">
                今日使用概览
              </h3>
              <p className="text-sm text-muted-foreground">
                展示今天的真实用量和平台价格优惠。
              </p>
            </div>
            <div className="flex flex-wrap items-center gap-2">
              {account ? (
                <div className="rounded-full border border-border/70 bg-background/60 px-2.5 py-1 text-xs text-muted-foreground">
                  {account.displayName || account.username || `用户 ${account.id}`}
                  <span className="ml-1 text-muted-foreground">ID {account.id}</span>
                </div>
              ) : null}
              <Button
                type="button"
                variant="ghost"
                size="sm"
                className="h-8 px-2 text-xs text-muted-foreground"
                title={t("common.refresh", "刷新")}
                onClick={refreshUsage}
              >
                <RefreshCw className="mr-1 h-3.5 w-3.5" />
                刷新
              </Button>
              <Button
                type="button"
                variant="ghost"
                size="sm"
                className="h-8 px-2 text-xs text-muted-foreground"
                title="打开云端数据看板"
                onClick={() => void openDashboard()}
              >
                <ExternalLink className="mr-1 h-3.5 w-3.5" />
                更多
              </Button>
            </div>
          </div>

          <div className="mt-4 grid gap-3 md:grid-cols-3">
            <MiniCard
              label="当前余额"
              value={fmtInt(values.currentBalance)}
              description="云端账户当前剩余额度"
              values={values.sparklines.balance}
              icon={Wallet}
            />
            <MiniCard
              label="今日用量"
              value={fmtInt(values.rangeUsedQuota)}
              description={`累计已用 ${fmtInt(values.usedQuota)} 额度`}
              values={values.sparklines.usage}
              icon={TrendingUp}
            />
            <MiniCard
              label="请求次数"
              value={fmtInt(values.requestCount)}
              description={`当前范围 ${fmtInt(values.rangeRequests)} 次 / ${fmtInt(values.rangeTokens)} tokens`}
              values={values.sparklines.requests}
              icon={Activity}
            />
          </div>

          <div className="mt-3 grid gap-3 md:grid-cols-3">
            <MiniCard
              label="官方价格"
              value={estimateLoading ? "计算中" : fmtUsd(values.officialCost, 4)}
              description="按当前 token 用量套用官方模型价格"
              values={[values.officialCost]}
              icon={BadgeDollarSign}
            />
            <MiniCard
              label="BistroCode 价格"
              value={estimateLoading ? "计算中" : fmtUsd(values.bistrocodeCost, 4)}
              description="按 BistroCode 云端价格折算"
              values={[values.bistrocodeCost]}
              icon={Wallet}
            />
            <MiniCard
              label="已优惠"
              value={estimateLoading ? "计算中" : fmtUsd(values.estimatedSavings, 4)}
              description={`已匹配 ${fmtInt(estimate?.pricedRequests ?? 0)} 次请求`}
              values={[values.estimatedSavings]}
              icon={PiggyBank}
            />
          </div>

          {error ? (
            <div className="mt-3 rounded-md border border-amber-500/20 bg-amber-500/10 px-3 py-2 text-xs text-amber-700 dark:text-amber-300">
              {String(error)}
            </div>
          ) : null}
          {estimateError ? (
            <div className="mt-3 rounded-md border border-amber-500/20 bg-amber-500/10 px-3 py-2 text-xs text-amber-700 dark:text-amber-300">
              {String(estimateError)}
            </div>
          ) : null}
        </CardContent>
      </Card>
    </motion.div>
  );
}
