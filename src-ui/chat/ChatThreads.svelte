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

<header class="ui-panel-header chat-threads">
  <DropdownMenu.Root>
    <DropdownMenu.Trigger class="ui-quiet-trigger thread-trigger" aria-label="Chats: {title}" title="Past chats">
      <MessagesSquare size={14} strokeWidth={1.75} />
      <span>{title}</span>
      <ChevronDown size={12} strokeWidth={1.75} />
    </DropdownMenu.Trigger>
    <DropdownMenu.Portal>
      <DropdownMenu.Content class="ui-menu thread-menu" align="start" sideOffset={4}>
        <div class="ui-menu-heading">Chats</div>
        <DropdownMenu.RadioGroup value={history.currentId} onValueChange={(id) => history.open(id)}>
          {#each history.listed as thread (thread.id)}
            <DropdownMenu.RadioItem class="ui-menu-item thread-item" value={thread.id} data-thread={thread.id}>
              {#snippet children({ checked })}
                <span class="ui-menu-check">{#if checked}<Check size={14} strokeWidth={1.75} />{/if}</span>
                <span class="label">{threadTitle(thread)}</span>
                <span class="ui-hint">{thread.turns.length ? when(thread.updated) : 'empty'}</span>
              {/snippet}
            </DropdownMenu.RadioItem>
          {/each}
        </DropdownMenu.RadioGroup>
        <DropdownMenu.Separator class="ui-menu-separator" />
        <DropdownMenu.Item class="ui-menu-item" disabled={!history.current.turns.length} onSelect={() => history.newChat()}>
          <span class="ui-menu-check"><SquarePen size={14} strokeWidth={1.75} /></span><span>New chat</span>
        </DropdownMenu.Item>
        <DropdownMenu.Item class="ui-menu-item is-danger" disabled={busy || !history.current.turns.length} onSelect={() => confirming = true}>
          <span class="ui-menu-check"><Trash size={14} strokeWidth={1.75} /></span><span>Delete this chat…</span>
        </DropdownMenu.Item>
      </DropdownMenu.Content>
    </DropdownMenu.Portal>
  </DropdownMenu.Root>
  <span class="ui-spacer"></span>
  <IconButton icon={SquarePen} label="New chat" disabled={!history.current.turns.length} onclick={() => history.newChat()} />
</header>

<AlertDialog.Root bind:open={confirming}>
  <AlertDialog.Portal>
    <AlertDialog.Overlay class="ui-dialog-overlay" />
    <AlertDialog.Content class="ui-dialog">
      <AlertDialog.Title class="ui-dialog-title">Delete this chat?</AlertDialog.Title>
      <div class="ui-dialog-body">
        <AlertDialog.Description class="ui-dialog-description thread-dialog-body">“{title}” and its {history.current.turns.length} message{history.current.turns.length === 1 ? '' : 's'} are removed from this scope. Changes the chat made to the document stay.</AlertDialog.Description>
      </div>
      <div class="ui-dialog-actions">
        <AlertDialog.Cancel class="ui-button is-outline">Cancel</AlertDialog.Cancel>
        <AlertDialog.Action class="ui-button is-danger is-outline" onclick={() => { history.remove(history.currentId); confirming = false; }}>Delete chat</AlertDialog.Action>
      </div>
    </AlertDialog.Content>
  </AlertDialog.Portal>
</AlertDialog.Root>

<style>
  .chat-threads { min-width: 0; }
  :global(.thread-menu) { width: min(320px, calc(100vw - 32px)); }
  :global(.thread-menu .thread-item .label) { flex: 1; }
  :global(.thread-dialog-body) { overflow-wrap: anywhere; }
</style>
