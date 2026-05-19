import { useQuery } from "@tanstack/react-query";
import { dashboardApi } from "@/lib/api/dashboard";
import { resolveUsageRange } from "@/lib/usageRange";
import type { UsageRangeSelection } from "@/types/usage";

export const dashboardKeys = {
  all: ["dashboard"] as const,
  userQuota: (
    preset: UsageRangeSelection["preset"],
    customStartDate: number | undefined,
    customEndDate: number | undefined,
    userId: string,
  ) =>
    [
      ...dashboardKeys.all,
      "user-quota",
      preset,
      customStartDate ?? 0,
      customEndDate ?? 0,
      userId || "anonymous",
    ] as const,
};

export function useCloudUserQuotaDates(
  range: UsageRangeSelection,
  options: {
    accessToken?: string;
    userId?: string;
    enabled?: boolean;
    refetchInterval?: number | false;
    refetchIntervalInBackground?: boolean;
  },
) {
  return useQuery({
    queryKey: dashboardKeys.userQuota(
      range.preset,
      range.customStartDate,
      range.customEndDate,
      options.userId ?? "",
    ),
    queryFn: () => {
      const { startDate, endDate } = resolveUsageRange(range);
      return dashboardApi.getUserQuotaDates(startDate, endDate, {
        accessToken: options.accessToken,
        userId: options.userId,
      });
    },
    enabled: options.enabled ?? true,
    refetchInterval: options.refetchInterval ?? false,
    refetchIntervalInBackground: options.refetchIntervalInBackground ?? false,
  });
}
