// What the buttons of a download do, shared by the card and the table row.
import type { Download } from '../api/types';
import type { IconName } from '../components/Icon.svelte';
import { t } from '../i18n';
import type { PrimaryKind } from '../util/downloads';
import { confirmAction } from './confirm.svelte';
import { downloads } from './downloads.svelte';

/** The label and icon of the main button. */
export function primaryUi(kind: PrimaryKind): { label: string; icon: IconName } {
  switch (kind) {
    case 'startNow':
      return { label: t('card.startNow'), icon: 'down' };
    case 'pause':
      return { label: t('card.pause'), icon: 'pause' };
    case 'skipVerify':
      return { label: t('card.skipVerify'), icon: 'x' };
    case 'resume':
      return { label: t('card.resume'), icon: 'play' };
    case 'retry':
      return { label: t('card.retry'), icon: 'retry' };
    case 'downloadAgain':
      return { label: t('card.downloadAgain'), icon: 'retry' };
  }
}

export function runPrimary(kind: PrimaryKind, id: number): Promise<boolean> {
  switch (kind) {
    case 'startNow':
      return downloads.startNow(id);
    case 'pause':
      return downloads.pause(id);
    case 'skipVerify':
      return downloads.keep(id);
    case 'resume':
    case 'retry':
    case 'downloadAgain':
      return downloads.resume(id);
  }
}

/** Asks first: cancelling throws the partial file away. */
export function confirmCancel(d: Download): Promise<boolean> {
  return confirmAction({
    title: t('confirm.cancel.title'),
    message: t('confirm.cancel.message', { name: d.filename }),
    confirmLabel: t('confirm.cancel.yes'),
    danger: true,
  });
}
