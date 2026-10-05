import { useTranslation } from "react-i18next";

/** Marks a row the user excluded from Spending; it still moves the balance. */
export function ExcludedBadge() {
  const { t } = useTranslation();
  return (
    <span
      className="bg-muted text-muted-foreground shrink-0 rounded px-1.5 py-0.5 text-[10px] font-medium uppercase tracking-wide"
      title={t("spending:exclusion.badgeHint")}
    >
      {t("spending:exclusion.badge")}
    </span>
  );
}
