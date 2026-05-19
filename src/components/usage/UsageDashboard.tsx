import { useState } from "react";
import { useTranslation } from "react-i18next";
import { UsageHero } from "./UsageHero";
import { UsageTrendChart } from "./UsageTrendChart";
import { RequestLogTable } from "./RequestLogTable";
import { ProviderStatsTable } from "./ProviderStatsTable";
import { ModelStatsTable } from "./ModelStatsTable";
import { type UsageRangeSelection } from "@/types/usage";
import { motion } from "framer-motion";
import { BarChart3, ListFilter, Activity, Coins } from "lucide-react";
import {
  Accordion,
  AccordionContent,
  AccordionItem,
  AccordionTrigger,
} from "@/components/ui/accordion";
import { PricingConfigPanel } from "@/components/usage/PricingConfigPanel";
import { cn } from "@/lib/utils";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";

interface UsageDashboardProps {
  compact?: boolean;
}

export function UsageDashboard({ compact = false }: UsageDashboardProps) {
  const { t } = useTranslation();
  const [range, setRange] = useState<UsageRangeSelection>({ preset: "today" });
  const rangeLabel = "今天";

  return (
    <motion.div
      initial={{ opacity: 0, y: 10 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ duration: 0.4 }}
      className={cn("pb-8", compact ? "space-y-4" : "space-y-8")}
    >
      <div className="flex flex-col gap-4">
        <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-4">
          <div className="flex flex-col gap-1">
            <h2 className={cn("font-bold", compact ? "text-lg" : "text-2xl")}>
              使用统计概览
            </h2>
            <p className="text-sm text-muted-foreground">
              展示今天的云端用量，并对比官方价格与 BistroCode 平台价格。
            </p>
          </div>
        </div>
      </div>

      <UsageHero range={range} refreshIntervalMs={0} />

      {compact ? null : (
        <UsageTrendChart
          range={range}
          rangeLabel={rangeLabel}
          refreshIntervalMs={0}
        />
      )}

      {!compact && (
        <div className="space-y-4">
          <Tabs defaultValue="logs" className="w-full">
            <div className="flex items-center justify-between mb-4">
              <TabsList className="bg-muted/50">
                <TabsTrigger value="logs" className="gap-2">
                  <ListFilter className="h-4 w-4" />
                  {t("usage.requestLogs")}
                </TabsTrigger>
                <TabsTrigger value="providers" className="gap-2">
                  <Activity className="h-4 w-4" />
                  {t("usage.providerStats")}
                </TabsTrigger>
                <TabsTrigger value="models" className="gap-2">
                  <BarChart3 className="h-4 w-4" />
                  {t("usage.modelStats")}
                </TabsTrigger>
              </TabsList>
            </div>

            <motion.div
              initial={{ opacity: 0, y: 10 }}
              animate={{ opacity: 1, y: 0 }}
              transition={{ delay: 0.2 }}
            >
              <TabsContent value="logs" className="mt-0">
                <RequestLogTable
                  range={range}
                  rangeLabel={rangeLabel}
                  appType="all"
                  refreshIntervalMs={0}
                  onRangeChange={setRange}
                />
              </TabsContent>

              <TabsContent value="providers" className="mt-0">
                <ProviderStatsTable
                  range={range}
                  appType="all"
                  refreshIntervalMs={0}
                />
              </TabsContent>

              <TabsContent value="models" className="mt-0">
                <ModelStatsTable
                  range={range}
                  appType="all"
                  refreshIntervalMs={0}
                />
              </TabsContent>
            </motion.div>
          </Tabs>
        </div>
      )}

      {!compact && (
        <Accordion
          type="multiple"
          defaultValue={[]}
          className="w-full space-y-4"
        >
          <AccordionItem
            value="pricing"
            className="rounded-xl glass-card overflow-hidden"
          >
            <AccordionTrigger className="px-6 py-4 hover:no-underline hover:bg-muted/50 data-[state=open]:bg-muted/50">
              <div className="flex items-center gap-3">
                <Coins className="h-5 w-5 text-yellow-500" />
                <div className="text-left">
                  <h3 className="text-base font-semibold">
                    {t("settings.advanced.pricing.title")}
                  </h3>
                  <p className="text-sm text-muted-foreground font-normal">
                    {t("settings.advanced.pricing.description")}
                  </p>
                </div>
              </div>
            </AccordionTrigger>
            <AccordionContent className="px-6 pb-6 pt-4 border-t border-border/50">
              <PricingConfigPanel />
            </AccordionContent>
          </AccordionItem>
        </Accordion>
      )}
    </motion.div>
  );
}
