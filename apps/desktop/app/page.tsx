"use client";

import {
  ipc,
  type CoreStatusDto,
  type FeatureLifecycleStateDto,
} from "@qbit/ipc";
import { useI18n } from "@qbit/i18n";
import { AppShell } from "@qbit/ui";
import { Button, Paper, Stack, Typography } from "@mui/material";
import { useEffect, useState } from "react";

type CoreStatusState =
  | { status: "loading" }
  | { status: "success"; coreStatus: CoreStatusDto }
  | { status: "error" };

async function requestCoreStatus(): Promise<CoreStatusState> {
  try {
    const coreStatus = await ipc.getCoreStatus();
    return { status: "success", coreStatus };
  } catch {
    return { status: "error" };
  }
}

const lifecycleTranslationKeys: Record<FeatureLifecycleStateDto, string> = {
  unavailable: "lifecycle.unavailable",
  disabled: "lifecycle.disabled",
  starting: "lifecycle.starting",
  running: "lifecycle.running",
  degraded: "lifecycle.degraded",
  stopping: "lifecycle.stopping",
  failed: "lifecycle.failed",
};

export default function HomePage() {
  const { t } = useI18n();
  const [coreStatusState, setCoreStatusState] = useState<CoreStatusState>({
    status: "loading",
  });

  useEffect(() => {
    let active = true;

    void requestCoreStatus().then((state) => {
      if (active) {
        setCoreStatusState(state);
      }
    });

    return () => {
      active = false;
    };
  }, []);

  return (
    <AppShell>
      <Stack spacing={3}>
        <Typography component="h2" variant="h4">
          {t("core.status.heading")}
        </Typography>
        {coreStatusState.status === "loading" ? (
          <Typography role="status">{t("core.status.loading")}</Typography>
        ) : null}
        {coreStatusState.status === "error" ? (
          <Stack alignItems="flex-start" spacing={2}>
            <Typography role="alert">{t("core.status.load_error")}</Typography>
            <Button
              onClick={() => {
                setCoreStatusState({ status: "loading" });
                void requestCoreStatus().then(setCoreStatusState);
              }}
              variant="contained"
            >
              {t("core.status.retry")}
            </Button>
          </Stack>
        ) : null}
        {coreStatusState.status === "success" ? (
          <Paper
            component="section"
            elevation={0}
            sx={{ p: 3, borderRadius: 3 }}
          >
            <Stack spacing={2}>
              {coreStatusState.coreStatus.runtimeState ===
              "recoveryRequired" ? (
                <Stack component="section" role="alert" spacing={1}>
                  <Typography component="h3" variant="h6">
                    {t("core.recovery.title")}
                  </Typography>
                  <Typography>{t("core.recovery.message")}</Typography>
                  {coreStatusState.coreStatus.startupError !== null &&
                  t(coreStatusState.coreStatus.startupError.messageKey) !==
                    coreStatusState.coreStatus.startupError.messageKey ? (
                    <Typography>
                      {t(coreStatusState.coreStatus.startupError.messageKey)}
                    </Typography>
                  ) : null}
                </Stack>
              ) : null}
              <Typography>
                {t("core.app_version", {
                  version: coreStatusState.coreStatus.appVersion,
                })}
              </Typography>
              <Typography>
                {t("core.platform", {
                  platform: coreStatusState.coreStatus.platform,
                })}
              </Typography>
              <Typography>
                {t("core.schema_version", {
                  version:
                    coreStatusState.coreStatus.schemaVersion ??
                    t("core.schema_unavailable"),
                })}
              </Typography>
              {coreStatusState.coreStatus.features.length === 0 ? (
                <Typography>{t("core.features.empty")}</Typography>
              ) : (
                <Stack
                  component="ul"
                  aria-label={t("core.features.list_label")}
                  spacing={1}
                  sx={{ m: 0, pl: 3 }}
                >
                  {coreStatusState.coreStatus.features.map((feature) => {
                    const translatedName = t(feature.displayNameKey);
                    const name =
                      translatedName === feature.displayNameKey
                        ? feature.id
                        : translatedName;
                    return (
                      <Typography component="li" key={feature.id}>
                        {feature.route ? (
                          <Button href={feature.route}>{name}</Button>
                        ) : null}
                        {t("core.feature.format", {
                          name,
                          id: feature.id,
                          lifecycle: t(
                            lifecycleTranslationKeys[feature.lifecycleState],
                          ),
                        })}
                      </Typography>
                    );
                  })}
                </Stack>
              )}
            </Stack>
          </Paper>
        ) : null}
      </Stack>
    </AppShell>
  );
}
