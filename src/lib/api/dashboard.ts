import { invoke } from "@tauri-apps/api/core";

export interface QuotaDataItem {
  id?: number;
  user_id?: number;
  username?: string;
  model_name?: string;
  created_at: number;
  token_used?: number;
  count?: number;
  quota?: number;
}

export interface DashboardQuotaResponse {
  success: boolean;
  data: QuotaDataItem[];
  message?: string;
}

export interface DashboardApiOptions {
  accessToken?: string;
  userId?: string;
}

export const dashboardApi = {
  async getUserQuotaDates(
    startTimestamp: number,
    endTimestamp: number,
    options: DashboardApiOptions,
  ): Promise<QuotaDataItem[]> {
    return invoke("get_bistrocode_dashboard_quota", {
      accessToken: options.accessToken?.trim() ?? "",
      userId: options.userId?.trim() ?? "",
      startTimestamp,
      endTimestamp,
    });
  },

  async getUserQuotaSummary(
    startTimestamp: number,
    endTimestamp: number,
    options: DashboardApiOptions,
  ): Promise<QuotaDataItem[]> {
    return dashboardApi.getUserQuotaDates(startTimestamp, endTimestamp, options);
  },
};
