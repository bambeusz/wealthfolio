import i18n from "i18next";
import { isActivityEditCancelled } from "@/features/spending/components/activity-split-review-provider";
import { getTransferPairForActivity, logger } from "@/adapters";
import { buildAssetResolutionInput } from "@/lib/asset-resolution-input";
import { ACTIVITY_SUBTYPES, ActivityStatus, ActivityType } from "@/lib/constants";
import { isCashSymbol } from "@/lib/activity-utils";
import { generateId } from "@/lib/id";
import type { ActivityCreate, ActivityDetails, ActivityUpdate } from "@/lib/types";
import { useCallback, useMemo } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import type { AccountSelectOption } from "../components/forms/fields";
import type { NewActivityFormValues } from "../components/forms/schemas";
import type { TransferFormValues } from "../components/forms/transfer-form";
import type { AdjustmentFormValues } from "../components/forms/adjustment-form";
import type { ConversionFormValues } from "../components/forms/conversion-form";
import {
  ACTIVITY_FORM_CONFIG,
  hasActivityForm,
  type ActivityFormValues,
  type PickerActivityType,
} from "../config/activity-form-config";
import { isPureCashActivity, mapActivityTypeToPicker } from "../utils/activity-form-utils";
import { useActivityMutations } from "./use-activity-mutations";
import { conversionFirstDate, isLeavingConversionRow } from "../utils/conversion-counterpart";

function generateSourceGroupId(): string {
  return generateId("wf-transfer");
}

/**
 * A conversion's two rows (engine rules R7.5): the leaving units as
 * CONVERSION_OUT and the arriving units as CONVERSION_IN, in one account on
 * one day, with no cash.
 */
function conversionRows(data: ConversionFormValues, accountCurrency: string | undefined) {
  const side = (
    subtype: string,
    asset: {
      id?: string | null;
      symbol: string;
      exchangeMic?: string | null;
      quoteCcy?: string | null;
      instrumentType?: string | null;
      quoteMode?: string | null;
      metadata?: ConversionFormValues["fromAssetMetadata"];
    },
    quantity: number,
    currency: string | null | undefined,
  ) => ({
    accountId: data.accountId,
    activityType: ActivityType.ADJUSTMENT,
    subtype,
    activityDate: data.activityDate,
    quantity,
    currency: currency?.trim() || asset.quoteCcy?.trim() || accountCurrency,
    comment: data.comment ?? null,
    // An existing asset is named by its id alone: with a symbol, its identity
    // would be inferred again, which can drop the id (a bond read as equity).
    asset: asset.id
      ? buildAssetResolutionInput({ id: asset.id })
      : buildAssetResolutionInput({
          symbol: asset.symbol,
          exchangeMic: asset.exchangeMic ?? asset.metadata?.exchangeMic,
          quoteCcy: asset.quoteCcy,
          instrumentType: asset.instrumentType,
          quoteMode: asset.quoteMode,
          name: asset.metadata?.name,
          kind: asset.metadata?.kind,
          providerId: asset.metadata?.providerId,
          providerSymbol: asset.metadata?.providerSymbol,
        }),
  });
  return {
    leaving: side(
      ACTIVITY_SUBTYPES.CONVERSION_OUT,
      {
        id: data.fromExistingAssetId,
        symbol: data.fromAssetId,
        exchangeMic: data.fromExchangeMic,
        quoteCcy: data.fromSymbolQuoteCcy,
        instrumentType: data.fromSymbolInstrumentType,
        quoteMode: data.fromQuoteMode,
        metadata: data.fromAssetMetadata,
      },
      data.fromQuantity,
      data.fromCurrency,
    ),
    arriving: side(
      ACTIVITY_SUBTYPES.CONVERSION_IN,
      {
        id: data.toExistingAssetId,
        symbol: data.toAssetId,
        exchangeMic: data.toExchangeMic,
        quoteCcy: data.toSymbolQuoteCcy,
        instrumentType: data.toSymbolInstrumentType,
        quoteMode: data.toQuoteMode,
        metadata: data.toAssetMetadata,
      },
      data.toQuantity,
      data.toCurrency,
    ),
  };
}

function extractErrorMessage(error: unknown): string {
  if (typeof error === "string" && error.trim()) return error;
  if (error instanceof Error && error.message.trim()) return error.message;
  if (error && typeof error === "object") {
    const raw = error as Record<string, unknown>;
    if (typeof raw.error === "string" && raw.error.trim()) return raw.error;
    if (typeof raw.message === "string" && raw.message.trim()) return raw.message;
  }
  return i18n.t("activity:mobile_form.save_failed_desc");
}

function transferPairIds(activity: Partial<ActivityDetails> | undefined): {
  transferOutId?: string;
  transferInId?: string;
} {
  return {
    transferOutId:
      activity?.transferOutId ??
      (activity?.activityType === ActivityType.TRANSFER_OUT
        ? activity.id
        : activity?.counterpartActivityId),
    transferInId:
      activity?.transferInId ??
      (activity?.activityType === ActivityType.TRANSFER_IN
        ? activity.id
        : activity?.counterpartActivityId),
  };
}

export interface UseActivityFormParams {
  accounts: AccountSelectOption[];
  activity?: Partial<ActivityDetails>;
  selectedType: PickerActivityType | undefined;
  onSuccess?: () => void;
}

export interface UseActivityFormReturn {
  /** Default values for the selected form type */
  defaultValues: Partial<ActivityFormValues> | undefined;
  /** Whether we're editing an existing activity */
  isEditing: boolean;
  /** Whether a mutation is in progress */
  isLoading: boolean;
  /** Error from the last mutation */
  error: Error | string | null;
  /** Whether the last mutation resulted in an error */
  isError: boolean;
  /** Submit handler for the selected form type */
  handleSubmit: (data: ActivityFormValues) => Promise<void>;
}

/**
 * Hook that provides all form logic for activity forms.
 * Uses configuration-driven approach for optimal performance.
 */
export function useActivityForm({
  accounts,
  activity,
  selectedType,
  onSuccess,
}: UseActivityFormParams): UseActivityFormReturn {
  const { t } = useTranslation();
  const {
    addActivityMutation,
    updateActivityMutation,
    saveActivitiesMutation,
    saveInternalTransferPairMutation,
    unlinkTransferActivitiesMutation,
  } = useActivityMutations(onSuccess);

  const isEditing = !!activity?.id;
  const isLoading =
    addActivityMutation.isPending ||
    updateActivityMutation.isPending ||
    saveActivitiesMutation.isPending ||
    saveInternalTransferPairMutation.isPending ||
    unlinkTransferActivitiesMutation.isPending;
  const error =
    [
      addActivityMutation.error,
      updateActivityMutation.error,
      saveActivitiesMutation.error,
      saveInternalTransferPairMutation.error,
      unlinkTransferActivitiesMutation.error,
    ].find((error) => error != null && !isActivityEditCancelled(error)) ?? null;
  const isError = error !== null;

  // Get config for selected type (undefined if no type selected)
  const config = selectedType ? ACTIVITY_FORM_CONFIG[selectedType] : undefined;

  // The user swapped an already-editable stored type for another one ("Change
  // type"). A row stored without an editor (UNKNOWN) being classified for the
  // first time is not a change of type, so it keeps the plain edit behaviour.
  const storedPickerType = mapActivityTypeToPicker(activity?.activityType);
  const typeChanged =
    isEditing &&
    !!selectedType &&
    hasActivityForm(storedPickerType) &&
    storedPickerType !== selectedType;

  // Compute default values only for selected type (lazy evaluation)
  const defaultValues = useMemo(() => {
    if (!config) return undefined;
    const defaults = config.getDefaults(activity, accounts);
    if (typeChanged && selectedType === "TRANSFER") {
      // A non-transfer row cannot become one leg of an internal transfer by
      // editing it: the other leg does not exist. Open it as an external
      // transfer on its own account; pairing is a separate "Link transfer" step.
      return {
        ...defaults,
        isExternal: true,
        direction: activity?.activityType === ActivityType.DEPOSIT ? "in" : "out",
        accountId: activity?.accountId ?? "",
        fromAccountId: "",
        toAccountId: "",
      } as Partial<ActivityFormValues>;
    }
    return defaults;
  }, [config, activity, accounts, typeChanged, selectedType]);

  // Single submit handler that uses config transform
  const handleSubmit = useCallback(
    async (formData: ActivityFormValues) => {
      if (!config) return;

      // Detach the edited row from its linked transfer pair, so the row can be
      // saved on its own. Both legs become standalone external transfers; the
      // other leg is kept, not deleted or retyped.
      const unlinkLinkedPair = async (linked: Partial<ActivityDetails> & { id: string }) => {
        let { transferOutId, transferInId } = transferPairIds(linked);
        if (linked.sourceGroupId && (!transferOutId || !transferInId)) {
          try {
            const pair = await getTransferPairForActivity(linked.id);
            if (pair) {
              transferOutId = pair.transferOut.id;
              transferInId = pair.transferIn.id;
            }
          } catch {
            // Invalid/orphan groups are cleared by the single-row update that follows.
          }
        }
        if (linked.sourceGroupId && transferOutId && transferInId) {
          await unlinkTransferActivitiesMutation.mutateAsync({
            activityAId: transferOutId,
            activityBId: transferInId,
          });
        }
      };

      // A type change to one without an asset must detach the stored asset
      // explicitly: the update keeps it unless told otherwise. Cash placeholders
      // are not assets.
      const dropsStoredAsset =
        typeChanged &&
        !!activity?.assetId &&
        !isCashSymbol(activity.assetId) &&
        (isPureCashActivity(config.activityType) ||
          (selectedType === "TRANSFER" &&
            (formData as TransferFormValues).transferMode !== "securities"));

      try {
        // A conversion saves its two rows together, created or edited.
        if (selectedType === "CONVERSION") {
          const data = formData as ConversionFormValues;
          const account = accounts.find((a) => a.value === data.accountId);
          const { leaving, arriving } = conversionRows(data, account?.currency);
          if (isEditing && activity?.id) {
            const editsLeaving = isLeavingConversionRow(activity);
            const leavingId = editsLeaving ? activity.id : activity.counterpartActivityId;
            const arrivingId = editsLeaving ? activity.counterpartActivityId : activity.id;
            if (!leavingId || !arrivingId) {
              throw new Error("Editing a conversion requires both of its rows.");
            }
            // A date left as the form opened keeps each row's own time, so the
            // pair keeps its place among activities at the same time; an
            // edited date moves both rows to it.
            const opened = conversionFirstDate(activity);
            const keepsTimes =
              !!opened && new Date(data.activityDate).getTime() === opened.getTime();
            const ownTime = (date: Date | string | null | undefined) =>
              keepsTimes && date ? { activityDate: date } : {};
            const [leavingTime, arrivingTime] = editsLeaving
              ? [activity.date, activity.counterpartDate]
              : [activity.counterpartDate, activity.date];
            const result = await saveActivitiesMutation.mutateAsync({
              updates: [
                { ...leaving, id: leavingId, ...ownTime(leavingTime) } as ActivityUpdate,
                { ...arriving, id: arrivingId, ...ownTime(arrivingTime) } as ActivityUpdate,
              ],
            });
            // The bulk save closes the sheet after a create; an edit closes it here.
            if (!result.errors?.length) onSuccess?.();
            return;
          }
          const sourceGroupId = generateId("wf-conversion");
          await saveActivitiesMutation.mutateAsync({
            creates: [
              { ...leaving, sourceGroupId } as ActivityCreate,
              { ...arriving, sourceGroupId } as ActivityCreate,
            ],
          });
          return;
        }

        // Handle internal transfers specially - need to create two activities
        if (selectedType === "TRANSFER") {
          const transferData = formData as TransferFormValues;

          if (typeChanged && !transferData.isExternal) {
            throw new Error(t("activity:type_change_to_transfer_hint"));
          }

          // Internal transfer: update or create both legs
          if (!transferData.isExternal && transferData.fromAccountId && transferData.toAccountId) {
            const fromAccount = accounts.find((a) => a.value === transferData.fromAccountId);
            const toAccount = accounts.find((a) => a.value === transferData.toAccountId);

            if (transferData.transferMode === "cash") {
              const sourceAmount = transferData.sourceAmount ?? transferData.amount;
              const sourceCurrency = transferData.sourceCurrency ?? fromAccount?.currency;
              const destinationCurrency =
                transferData.destinationCurrency ?? toAccount?.currency ?? sourceCurrency;
              const destinationAmount =
                sourceCurrency === destinationCurrency
                  ? sourceAmount
                  : (transferData.destinationAmount ??
                    (sourceAmount && transferData.transferRate
                      ? sourceAmount * transferData.transferRate
                      : undefined));

              if (!sourceAmount || !destinationAmount || !sourceCurrency || !destinationCurrency) {
                throw new Error("Transfer amount and currencies are required.");
              }

              const { transferOutId, transferInId } = transferPairIds(activity);

              if (isEditing && (!transferOutId || !transferInId)) {
                throw new Error(
                  "Use Link transfer... to pair this existing transfer before saving it as internal.",
                );
              }

              await saveInternalTransferPairMutation.mutateAsync({
                editedActivityId: isEditing ? activity?.id : undefined,
                transferOutId: isEditing ? transferOutId : undefined,
                transferInId: isEditing ? transferInId : undefined,
                fromAccountId: transferData.fromAccountId,
                toAccountId: transferData.toAccountId,
                activityDate: transferData.activityDate,
                sourceAmount,
                destinationAmount,
                sourceCurrency,
                destinationCurrency,
                notes: transferData.comment ?? null,
                transferMode: "cash",
              });
              return;
            }

            const formPayload = config.toPayload(formData);

            // Extract symbol-related and fxRate fields from payload
            const {
              assetId,
              existingAssetId,
              fxRate,
              exchangeMic,
              quoteMode,
              symbolQuoteCcy,
              symbolInstrumentType,
              assetMetadata,
              ...sharedFields
            } = formPayload as {
              assetId?: string;
              fxRate?: number;
              exchangeMic?: string;
              existingAssetId?: string;
              quoteMode?: string;
              symbolQuoteCcy?: string;
              symbolInstrumentType?: string;
              assetMetadata?: {
                name?: string;
                kind?: string;
                exchangeMic?: string;
                providerId?: string | null;
                providerSymbol?: string | null;
              };
            } & Record<string, unknown>;

            // Build the nested asset object with all metadata
            const assetInput: ActivityCreate["asset"] = assetId
              ? buildAssetResolutionInput({
                  id: existingAssetId,
                  symbol: assetId,
                  exchangeMic,
                  quoteMode,
                  quoteCcy: symbolQuoteCcy,
                  instrumentType: symbolInstrumentType,
                  name: assetMetadata?.name,
                  kind: assetMetadata?.kind,
                  providerId: assetMetadata?.providerId,
                  providerSymbol: assetMetadata?.providerSymbol,
                })
              : undefined;

            if (isEditing) {
              const { transferOutId, transferInId } = transferPairIds(activity);

              if (!transferOutId || !transferInId) {
                throw new Error("Editing an internal securities transfer requires both legs.");
              }

              const transferOutActivity: ActivityUpdate = {
                ...sharedFields,
                id: transferOutId,
                accountId: transferData.fromAccountId,
                activityType: ActivityType.TRANSFER_OUT,
                currency: fromAccount?.currency,
                asset: assetInput,
              } as ActivityUpdate;

              const transferInActivity: ActivityUpdate = {
                ...sharedFields,
                id: transferInId,
                accountId: transferData.toAccountId,
                activityType: ActivityType.TRANSFER_IN,
                currency: toAccount?.currency,
                asset: assetInput,
                fxRate,
              } as ActivityUpdate;

              await saveActivitiesMutation.mutateAsync({
                updates: [transferOutActivity, transferInActivity],
              });
              return;
            }

            const sourceGroupId = generateSourceGroupId();

            // Create TRANSFER_OUT on source account (no fxRate - activity currency = account currency)
            const transferOutActivity: ActivityCreate = {
              ...sharedFields,
              accountId: transferData.fromAccountId,
              activityType: ActivityType.TRANSFER_OUT,
              currency: fromAccount?.currency,
              sourceGroupId,
              asset: assetInput,
            } as ActivityCreate;

            // Create TRANSFER_IN on destination account (fxRate applies if currencies differ)
            const transferInActivity: ActivityCreate = {
              ...sharedFields,
              accountId: transferData.toAccountId,
              activityType: ActivityType.TRANSFER_IN,
              currency: toAccount?.currency,
              sourceGroupId,
              asset: assetInput,
              fxRate,
            } as ActivityCreate;

            await saveActivitiesMutation.mutateAsync({
              creates: [transferOutActivity, transferInActivity],
            });
            return;
          }

          // External transfer: determine activity type from direction
          const activityType =
            transferData.direction === "in" ? ActivityType.TRANSFER_IN : ActivityType.TRANSFER_OUT;
          const basePayload = config.toPayload(formData);
          const accountId = transferData.accountId;
          const account = accounts.find((a) => a.value === accountId);

          const submitData: NewActivityFormValues = {
            ...basePayload,
            activityType,
          } as NewActivityFormValues;

          if (!submitData.currency?.trim() && account?.currency) {
            submitData.currency = account.currency;
          }

          if (isEditing && activity?.id) {
            let { transferOutId, transferInId } = transferPairIds(activity);
            if (activity.sourceGroupId && (!transferOutId || !transferInId)) {
              try {
                const pair = await getTransferPairForActivity(activity.id);
                if (pair) {
                  transferOutId = pair.transferOut.id;
                  transferInId = pair.transferIn.id;
                }
              } catch {
                // Invalid/orphan groups are cleared by the single-row external update below.
              }
            }
            const unlinkTransferPairId =
              activity.sourceGroupId && transferOutId && transferInId
                ? activity.id === transferOutId
                  ? transferInId
                  : transferOutId
                : undefined;
            await updateActivityMutation.mutateAsync({
              id: activity.id,
              currentAssetId: dropsStoredAsset ? undefined : activity.assetId,
              clearAsset: dropsStoredAsset,
              unlinkTransferPairId,
              ...submitData,
            } as NewActivityFormValues & {
              id: string;
              currentAssetId?: string;
              clearAsset?: boolean;
              unlinkTransferPairId?: string;
            });
          } else {
            await addActivityMutation.mutateAsync(submitData);
          }
          return;
        }

        // Standard activity handling
        const basePayload = config.toPayload(formData);

        // Get account currency for pure cash activities
        const accountId = (formData as { accountId?: string }).accountId;
        const account = accounts.find((a) => a.value === accountId);

        const submitData: NewActivityFormValues = {
          ...basePayload,
          activityType: config.activityType as NewActivityFormValues["activityType"],
        } as NewActivityFormValues;

        if (!submitData.currency?.trim() && account?.currency) {
          submitData.currency = account.currency;
        }

        // A form save is a review: every field - including the total, typed
        // or calculated - is on screen when the user submits, so every form
        // submission attests. Drafts are the exception: they stay in their
        // review queue until explicitly approved and posted.
        if (isEditing && activity?.status === ActivityStatus.DRAFT) {
          delete (submitData as { needsReview?: boolean }).needsReview;
        } else {
          (submitData as { needsReview?: boolean }).needsReview = false;
        }

        if (isEditing && activity?.id) {
          // Leaving a transfer type while still linked: unlink first, exactly as
          // an external-transfer edit does, so the other leg is not left
          // pointing at a row that is no longer a transfer.
          const leavesTransfer =
            typeChanged &&
            (activity.activityType === ActivityType.TRANSFER_IN ||
              activity.activityType === ActivityType.TRANSFER_OUT);
          if (leavesTransfer) {
            await unlinkLinkedPair({ ...activity, id: activity.id });
            // The unlink marks the leg as an external transfer. That flag only
            // means something to transfers and CREDIT (where it makes the row an
            // external flow / contribution), so a leg retyped to CREDIT would
            // silently keep counting. The backend merges metadata patches over
            // the stored blob by top-level key, so an empty `flow` overwrites it.
            (submitData as { metadata?: Record<string, unknown> }).metadata = {
              ...(submitData as { metadata?: Record<string, unknown> }).metadata,
              flow: {},
            };
          }
          const adjustmentCash =
            selectedType === ActivityType.ADJUSTMENT &&
            (formData as AdjustmentFormValues).adjustmentMode === "cash";
          const currentAssetId = adjustmentCash || dropsStoredAsset ? undefined : activity.assetId;
          const clearAsset = Boolean(activity.assetId && (adjustmentCash || dropsStoredAsset));
          await updateActivityMutation.mutateAsync({
            id: activity.id,
            currentAssetId,
            clearAsset,
            ...submitData,
          } as NewActivityFormValues & {
            id: string;
            currentAssetId?: string;
            clearAsset?: boolean;
          });
        } else {
          await addActivityMutation.mutateAsync(submitData);
        }
      } catch (err) {
        if (isActivityEditCancelled(err)) return;
        const message = extractErrorMessage(err);
        toast.error(i18n.t("activity:mobile_form.save_failed"), { description: message });
        logger.error(`Activity Form Submit Error: ${JSON.stringify({ error: err, formData })}`);
      }
    },
    [
      config,
      accounts,
      isEditing,
      activity,
      selectedType,
      typeChanged,
      t,
      addActivityMutation,
      updateActivityMutation,
      saveActivitiesMutation,
      saveInternalTransferPairMutation,
      unlinkTransferActivitiesMutation,
      onSuccess,
    ],
  );

  return {
    defaultValues,
    isEditing,
    isLoading,
    error,
    isError,
    handleSubmit,
  };
}
