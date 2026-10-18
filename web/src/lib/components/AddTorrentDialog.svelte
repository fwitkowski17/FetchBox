<script lang="ts">
  // "Add a torrent": magnet links (one per line) and/or `.torrent` files, chosen or dropped.
  // Everything is sent one after the other; what fails stays in the dialog with its reason.
  import { i18n, t } from '../i18n';
  import { addTorrentDialog } from '../stores/addTorrentDialog.svelte';
  import { disks } from '../stores/disks.svelte';
  import { settings } from '../stores/settings.svelte';
  import { toast } from '../stores/toast.svelte';
  import { torrents } from '../stores/torrents.svelte';
  import { startingTorrentDisk } from '../util/disks';
  import { MAX_TORRENT_FILE, magnetsIn } from '../util/torrentInput';
  import DiskSelect from './DiskSelect.svelte';
  import Icon from './Icon.svelte';
  import Modal from './Modal.svelte';

  let magnet = $state('');
  let files = $state<File[]>([]);
  let errors = $state<string[]>([]);
  let busy = $state(false);
  let over = $state(false);
  let input: HTMLInputElement | undefined = $state();
  let field: HTMLTextAreaElement | undefined = $state();
  // The disk to save on (SPEC 6.14). Untouched, it is the default for torrents.
  let diskChoice = $state<string | null>(null);
  const disk = $derived(diskChoice ?? startingTorrentDisk(disks.value, settings.value?.torrent_default_disk));
  /** What the add request says about the disk: nothing where there is nothing to choose from. */
  const diskArg = $derived(disks.choosable ? disk : undefined);

  // Every opening starts clean (with a link when the one who opened it had one).
  $effect(() => {
    if (addTorrentDialog.open) {
      magnet = addTorrentDialog.magnet;
      files = [];
      errors = [];
      busy = false;
      over = false;
      diskChoice = null;
      void disks.load();
      queueMicrotask(() => field?.focus());
    }
  });

  const parsed = $derived(magnetsIn(magnet));
  const count = $derived(parsed.valid.length + files.length);

  function close(): void {
    if (!busy) addTorrentDialog.hide();
  }

  function take(list: FileList | File[] | null): void {
    if (!list) return;
    const known = new Set(files.map((f) => `${f.name}:${f.size}:${f.lastModified}`));
    const next = [...files];
    for (const f of Array.from(list)) {
      if (!known.has(`${f.name}:${f.size}:${f.lastModified}`)) next.push(f);
    }
    files = next;
  }

  const why = (e: unknown): string => i18n.errorText(e);

  async function submit(e?: Event): Promise<void> {
    e?.preventDefault();
    if (busy) return;
    errors = [];
    if (parsed.invalid.length > 0) {
      errors = [t('torrent.add.notMagnet')];
      return;
    }
    if (count === 0) {
      errors = [t('torrent.add.nothing')];
      return;
    }
    busy = true;
    const failed: string[] = [];
    const keepFiles: File[] = [];
    const keepMagnets: string[] = [];
    let added = 0;
    try {
      for (const m of parsed.valid) {
        try {
          await torrents.addMagnet(m, diskArg);
          added++;
        } catch (err) {
          keepMagnets.push(m);
          failed.push(t('torrent.add.failed', { name: m.slice(0, 60), error: why(err) }));
        }
      }
      for (const f of files) {
        if (f.size > MAX_TORRENT_FILE) {
          keepFiles.push(f);
          failed.push(t('torrent.add.tooBig', { name: f.name }));
          continue;
        }
        try {
          await torrents.addFile(await f.arrayBuffer(), diskArg);
          added++;
        } catch (err) {
          keepFiles.push(f);
          failed.push(t('torrent.add.failed', { name: f.name, error: why(err) }));
        }
      }
    } finally {
      busy = false;
    }
    if (added > 0) toast.success(t('torrent.add.added', { n: added }));
    if (failed.length === 0) {
      addTorrentDialog.hide();
      return;
    }
    // What was added is gone from the form; what was not stays, with the reasons.
    magnet = keepMagnets.join('\n');
    files = keepFiles;
    errors = failed;
  }
</script>

<Modal open={addTorrentDialog.open} title={t('torrent.add.title')} size="lg" onclose={close}>
  <form id="add-torrent" class="grid grid-cols-[minmax(0,1fr)] gap-4" onsubmit={submit} data-part="add-torrent">
    <div class="field">
      <label for="torrent-magnet">{t('torrent.add.magnet')}</label>
      <textarea
        id="torrent-magnet"
        bind:this={field}
        class="input min-h-24 resize-y font-mono text-sm"
        rows="3"
        spellcheck="false"
        autocomplete="off"
        placeholder={t('torrent.add.magnet.placeholder')}
        bind:value={magnet}
        onkeydown={(e) => {
          if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) void submit(e);
        }}
      ></textarea>
      <p class="hint">{t('torrent.add.magnets')}</p>
    </div>

    <div class="field">
      <span class="label">{t('torrent.add.file')}</span>
      <!-- A drop target for mouse users; the button inside does the same for the keyboard. -->
      <div
        role="presentation"
        class={[
          'grid justify-items-center gap-2 rounded-xl border-2 border-dashed p-4 text-center transition-colors',
          over ? 'border-accent bg-accent-soft' : 'border-border',
        ]}
        ondragover={(e) => {
          e.preventDefault();
          over = true;
        }}
        ondragleave={() => (over = false)}
        ondrop={(e) => {
          e.preventDefault();
          over = false;
          take(e.dataTransfer?.files ?? null);
        }}
        data-part="drop"
      >
        <span class="text-sm text-muted">{t('torrent.add.drop')}</span>
        <button type="button" class="btn" onclick={() => input?.click()}>
          <Icon name="plus" size={16} />{t('torrent.add.choose')}
        </button>
        <input
          bind:this={input}
          class="sr-only"
          type="file"
          multiple
          accept=".torrent,application/x-bittorrent"
          tabindex="-1"
          aria-label={t('torrent.add.file')}
          data-part="file-input"
          onchange={(e) => {
            take(e.currentTarget.files);
            e.currentTarget.value = '';
          }}
        />
      </div>
      {#if files.length > 0}
        <ul class="mt-2 grid gap-1" data-part="chosen">
          {#each files as f (`${f.name}:${f.size}:${f.lastModified}`)}
            <li class="flex items-center gap-2 rounded-lg bg-surface-2 py-1 pr-1 pl-3 text-sm">
              <span class="min-w-0 flex-1 truncate" title={f.name}>{f.name}</span>
              <span class="shrink-0 text-muted tabular-nums">{i18n.bytes(f.size)}</span>
              <button
                type="button"
                class="btn icon ghost"
                aria-label={t('torrent.add.remove', { name: f.name })}
                onclick={() => (files = files.filter((x) => x !== f))}
              >
                <Icon name="x" size={16} />
              </button>
            </li>
          {/each}
        </ul>
      {/if}
    </div>

    {#if disks.choosable && disks.value}
      <DiskSelect id="add-torrent-disk" disks={disks.value} value={disk} onchange={(name) => (diskChoice = name)} />
    {/if}

    {#if errors.length > 0}
      <ul class="grid gap-1 text-sm text-danger" role="alert" data-part="errors">
        {#each errors as message, i (i)}
          <li class="[overflow-wrap:anywhere]">{message}</li>
        {/each}
      </ul>
    {/if}
  </form>

  {#snippet footer()}
    <button class="btn" type="button" disabled={busy} onclick={close}>{t('common.cancel')}</button>
    <button class="btn primary" type="submit" form="add-torrent" disabled={busy || count === 0} data-part="add-torrent-submit">
      {busy ? t('torrent.add.adding') : t('torrent.add.start')}
    </button>
  {/snippet}
</Modal>
