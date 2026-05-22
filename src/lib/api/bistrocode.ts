import type { UniversalProviderModels } from "@/types";
import { invoke } from "@tauri-apps/api/core";

export interface BistroCodeAccount {
  id?: number;
  username?: string;
  displayName?: string;
  email?: string;
  group?: string;
  quota: number;
  usedQuota: number;
  requestCount?: number;
  quotaPerUsd?: number;
}

export interface BistroCodeAccountResponse {
  loggedIn: boolean;
  account?: BistroCodeAccount;
  message?: string;
}

export interface BistroCodePricingModel {
  modelName: string;
  quotaType: number;
  modelRatio: number;
  modelPrice: number;
  completionRatio: number;
  cacheRatio?: number;
  createCacheRatio?: number;
  enableGroups: string[];
}

export interface BistroCodePricingResponse {
  success: boolean;
  models: BistroCodePricingModel[];
  groupRatio: Record<string, number>;
  defaultGroupRatio: number;
  message?: string;
}

export interface BistroCodeAuthUser {
  id: number;
  username: string;
  displayName?: string;
  email?: string;
  group?: string;
  quota: number;
  usedQuota: number;
  requestCount: number;
  accessToken: string;
}

export interface BistroCodeDesktopTokenConfig {
  id: number;
  name: string;
  key: string;
  group: string;
  purpose: string;
  modelMatch: string;
  models?: UniversalProviderModels;
  baseUrl: string;
  createdTime: number;
}

export interface BistroCodeAuthExchangeResponse {
  success: boolean;
  message?: string;
  data?: BistroCodeAuthUser;
}

export const BISTROCODE_BASE_URL = "https://bistrocode.online";

export const bistrocodeApi = {
  async getAccount(
    accessToken?: string,
    userId?: string,
  ): Promise<BistroCodeAccountResponse> {
    return await invoke("get_bistrocode_account", {
      accessToken: accessToken?.trim() || null,
      userId: userId?.trim() || null,
    });
  },

  async getPricing(): Promise<BistroCodePricingResponse> {
    return await invoke("get_bistrocode_pricing");
  },

  async ensureDefaultTokens(
    accessToken: string,
    userId: string,
  ): Promise<BistroCodeDesktopTokenConfig[]> {
    return await invoke("ensure_bistrocode_default_tokens", {
      accessToken: accessToken.trim(),
      userId: userId.trim(),
    });
  },
};
