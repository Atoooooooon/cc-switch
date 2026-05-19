import { describe, expect, it } from "vitest";

import {
  createUniversalProviderFromPreset,
  findPresetByType,
} from "./universalProviderPresets";

describe("BistroCode universal provider preset", () => {
  it("defaults to bistrocode.online and enables NewAPI usage", () => {
    const preset = findPresetByType("bistrocode");

    expect(preset).toBeDefined();

    const provider = createUniversalProviderFromPreset(
      preset!,
      "provider-bistrocode",
      "",
      "sk-test",
    );

    expect(provider.name).toBe("BistroCode");
    expect(provider.baseUrl).toBe("https://bistrocode.online");
    expect(provider.apps).toEqual({
      claude: true,
      codex: true,
      gemini: true,
    });
    expect(provider.meta?.providerType).toBe("bistrocode");
    expect(provider.meta?.usage_script?.enabled).toBe(true);
    expect(provider.meta?.usage_script?.templateType).toBe("newapi");
    expect(provider.meta?.usage_script?.code).toContain("/api/usage/token/");
  });
});
