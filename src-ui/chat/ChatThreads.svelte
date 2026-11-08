<script lang="ts">
  // The chat panel's header: the current conversation's title, which opens
  // the panel's past chats (open one, start one, delete this one), and a New
  // chat button. History is per panel and saved with the scope
  // (sessions.svelte.ts).
  import { AlertDialog, DropdownMenu } from 'bits-ui';
  import Check from '@lucide/svelte/icons/check';
  import ChevronDown from '@lucide/svelte/icons/chevron-down';
  import MessagesSquare from '@lucide/svelte/icons/messages-square';
  import SquarePen from '@lucide/svelte/icons/square-pen';
  import Trash from '@lucide/svelte/icons/trash';
  import IconButton from '../lib/ui/IconButton.svelte';
  import { threadTitle } from './history';
  import type { ChatHistory } from './sessions.svelte';

  let { history, busy = false }: { history: ChatHistory; busy?: boolean } = $props();
  const title = $derived(threadTitle(history.current));
  let confirming = $state(false);

  /** "5 min ago", "Yesterday", "3 Nov". */
  function when(at: number): string {
    const minutes = Math.round((Date.now() - at) / 60000);
    if (minutes < 1) return 'Just now';
    if (minutes < 60) return `${minutes} min ago`;
    if (minutes < 60 * 24) return `${Math.round(minutes / 60)} h ago`;
    if (minutes < 60 * 48) return 'Yesterday';
    return new Date(at).toLocaleDateString(undefined, { day: 'numeric', month: 'short' });
  }
</script>

<header class="chat-threads">
  <DropdownMenu.Root>
    <DropdownMenu.Trigger class="thread-trigger" aria-label="Chats: {title}" title="Past chats">
      <MessagesSquare size={14} strokeWidth={1.75} />
      <span class="thread-title">{title}</span>
      <ChevronDown size={13} strokeWidth={1.75} />
    </DropdownMenu.Trigger>
    <DropdownMenu.Portal>
      <DropdownMenu.Content class="ui-menu thread-menu" align="start" sideOffset={4}>
        <div class="ui-menu-heading">Chats</div>
        <DropdownMenu.RadioGroup value={history.currentId} onValueChange={(id) => history.open(id)}>
          {#each history.listed as thread (thread.id)}
            <DropdownMenu.RadioItem class="ui-menu-item thread-item" value={thread.id} data-thread={thread.id}>
              {#snippet children({ checked })}
                <span class="check">{#if checked}<Check size={14} strokeWidth={2} />{/if}</span>
                <span class="label">{threadTitle(thread)}</span>
                <span class="ui-hint">{thread.turns.length ? when(thread.updated) : 'empty'}</span>
              {/snippet}
            </DropdownMenu.RadioItem>
          {/each}
        </DropdownMenu.RadioGroup>
        <DropdownMenu.Separator class="ui-menu-separator" />
        <DropdownMenu.Item class="ui-menu-item" disabled={!history.current.turns.length} onSelect={() => history.newChat()}>
          <span class="check"><SquarePen size={14} strokeWidth={1.75} /></span>New chat
        </DropdownMenu.Item>
        <DropdownMenu.Item class="ui-menu-item" disabled={busy || !history.current.turns.length} onSelect={() => confirming = true}>
          <span class="check"><Trash size={14} strokeWidth={1.75} /></span>Delete this chat…
        </DropdownMenu.Item>
      </DropdownMenu.Content>
    </DropdownMenu.Portal>
  </DropdownMenu.Root>
  <IconButton icon={SquarePen} label="New chat" disabled={!history.current.turns.length} onclick={() => history.newChat()} />
</header>

<AlertDialog.Root bind:open={confirming}>
  <AlertDialog.Portal>
    <AlertDialog.Overlay class="thread-overlay" />
    <AlertDialog.Content class="thread-dialog">
      <AlertDialog.Title class="thread-dialog-title">Delete this chat?</AlertDialog.Title>
      <AlertDialog.Description class="thread-dialog-body">“{title}” and its {history.current.turns.length} message{history.current.turns.length === 1 ? '' : 's'} are removed from this scope. Changes the chat made to the document stay.</AlertDialog.Description>
      <div class="thread-dialog-actions">
        <AlertDialog.Cancel class="thread-dialog-button">Cancel</AlertDialog.Cancel>
        <AlertDialog.Action class="thread-dialog-button danger" onclick={() => { history.remove(history.currentId); confirming = false; }}>Delete chat</AlertDialog.Action>
      </div>
    </AlertDialog.Content>
  </AlertDialog.Portal>
</AlertDialog.Root>

<style>
  .chat-threads { flex: none; display: flex; align-items: center; gap: 4px; padding: 6px 10px 4px; min-width: 0; }
  :global(.thread-trigger) {
    display: inline-flex; align-items: center; gap: 6px; min-width: 0; max-width: 100%; height: 26px; padding: 0 8px;
    border: 0; border-radius: 7px; background: transparent; color: var(--fg-muted); font: inherit; font-size: 12px; cursor: pointer;
  }
  :global(.thread-trigger:hover), :global(.thread-trigger[data-state='open']) { background: var(--hover); color: var(--fg); }
  :global(.thread-trigger:focus-visible) { outline: 2px solid var(--accent); outline-offset: 1px; }
  :global(.thread-trigger svg) { flex: none; color: var(--fg-faint); }
  .thread-title { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--fg); }
  .chat-threads :global(.property-icon) { margin-left: auto; flex: none; }
  :global(.thread-menu) { width: min(320px, calc(100vw - 32px)); }
  :global(.thread-menu .thread-item .label) { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; flex: 1; }
  .check { width: 14px; flex: none; display: flex; }
  .label { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; flex: 1; }
  :global(.thread-overlay) { position: fixed; inset: 0; z-index: 1000; background: var(--overlay); }
  :global(.thread-dialog) {
    position: fixed; left: 50%; top: 50%; transform: translate(-50%, -50%); z-index: 1001; width: min(400px, calc(100vw - 32px));
    padding: 18px 20px 16px; border: 1px solid var(--border-strong); border-radius: 10px; background: var(--bg); color: var(--fg);
    box-shadow: var(--menu-shadow); font-size: 13px;
  }
  :global(.thread-dialog-title) { margin: 0 0 8px; font-size: 14px; font-weight: 600; }
  :global(.thread-dialog-body) { margin: 0; color: var(--fg-muted); line-height: 1.45; overflow-wrap: anywhere; }
  .thread-dialog-actions { display: flex; justify-content: flex-end; gap: 8px; margin-top: 16px; }
  :global(.thread-dialog-button) { padding: 5px 12px; border: 1px solid var(--border-strong); border-radius: 6px; background: var(--bg-input); color: var(--fg); font: inherit; cursor: pointer; }
  :global(.thread-dialog-button:hover) { background: var(--hover); }
  :global(.thread-dialog-button:focus-visible) { outline: 2px solid var(--accent); outline-offset: 2px; }
  :global(.thread-dialog-button.danger) { border-color: var(--danger); background: var(--danger); color: var(--accent-fg); }
</style>
