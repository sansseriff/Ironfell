<script lang="ts">
  import { marked, type Token, type Tokens } from 'marked';
  import { safeChatUrl } from './conversation';
  import ChatImage from './ChatImage.svelte';

  let { text }: { text: string } = $props();
  const tokens = $derived(marked.lexer(text));
</script>

{#snippet inline(items: Token[])}
  {#each items as token}
    {#if token.type === 'strong'}<strong>{@render inline(token.tokens ?? [])}</strong>
    {:else if token.type === 'em'}<em>{@render inline(token.tokens ?? [])}</em>
    {:else if token.type === 'del'}<del>{@render inline(token.tokens ?? [])}</del>
    {:else if token.type === 'codespan'}<code>{token.text}</code>
    {:else if token.type === 'br'}<br />
    {:else if token.type === 'link'}
      {@const href = safeChatUrl(token.href)}
      {#if href}<a {href} target="_blank" rel="noopener noreferrer">{@render inline(token.tokens ?? [])}</a>{:else}<span>{@render inline(token.tokens ?? [])}</span>{/if}
    {:else if token.type === 'image'}<ChatImage src={token.href} alt={token.text} />
    {:else if 'tokens' in token && token.tokens}{@render inline(token.tokens)}
    {:else}{'text' in token ? token.text : token.raw}{/if}
  {/each}
{/snippet}

{#snippet blocks(items: Token[])}
  {#each items as token}
    {#if token.type === 'heading'}<svelte:element this={`h${Math.min(6, Math.max(1, token.depth))}`}>{@render inline(token.tokens ?? [])}</svelte:element>
    {:else if token.type === 'paragraph'}<p>{@render inline(token.tokens ?? [])}</p>
    {:else if token.type === 'code'}<figure class="code"><figcaption>{token.lang || 'Code'}</figcaption><pre><code>{token.text}</code></pre></figure>
    {:else if token.type === 'blockquote'}<blockquote>{@render blocks(token.tokens ?? [])}</blockquote>
    {:else if token.type === 'list'}
      {@const list = token as Tokens.List}
      <svelte:element this={list.ordered ? 'ol' : 'ul'} start={list.ordered ? Number(list.start) || 1 : undefined}>
        {#each list.items as item}<li>{#if item.task}<span aria-label={item.checked ? 'Completed' : 'Not completed'}>{item.checked ? '[x] ' : '[ ] '}</span>{/if}{@render blocks(item.tokens)}</li>{/each}
      </svelte:element>
    {:else if token.type === 'table'}
      {@const table = token as Tokens.Table}
      <div class="table"><table><thead><tr>{#each table.header as cell}<th>{@render inline(cell.tokens)}</th>{/each}</tr></thead><tbody>{#each table.rows as row}<tr>{#each row as cell}<td>{@render inline(cell.tokens)}</td>{/each}</tr>{/each}</tbody></table></div>
    {:else if token.type === 'hr'}<hr />
    {:else if token.type === 'html'}<p class="literal">{token.raw}</p>
    {:else if token.type === 'text'}{@render inline(token.tokens ?? [{ type: 'text', raw: token.raw, text: token.text }])}
    {:else if token.type !== 'space' && token.type !== 'def'}<p>{token.raw}</p>{/if}
  {/each}
{/snippet}

<div class="chat-markdown">{@render blocks(tokens)}</div>

<style>
  .chat-markdown { font-size: 13px; line-height: 1.65; overflow-wrap: anywhere; min-width: 0; }
  .chat-markdown :global(p) { margin: 0 0 10px; }
  .chat-markdown :global(p:last-child) { margin-bottom: 0; }
  .chat-markdown :global(h1), .chat-markdown :global(h2), .chat-markdown :global(h3), .chat-markdown :global(h4), .chat-markdown :global(h5), .chat-markdown :global(h6) { font-size: 1.1em; line-height: 1.4; margin: 16px 0 8px; }
  .chat-markdown :global(a) { color: var(--accent); text-underline-offset: 3px; }
  .chat-markdown :global(code) { font-size: .9em; background: var(--bg-sunken); padding: 1px 4px; border-radius: 3px; }
  .chat-markdown :global(pre) { margin: 0; padding: 10px 12px; overflow-x: auto; white-space: pre; }
  .chat-markdown :global(pre code) { padding: 0; background: none; }
  .chat-markdown :global(figure.code) { margin: 10px 0; border: 1px solid var(--border); border-radius: 7px; overflow: hidden; background: var(--bg-sunken); }
  .chat-markdown :global(figcaption) { padding: 4px 12px; border-bottom: 1px solid var(--border); color: var(--fg-muted); font-size: 10px; }
  .chat-markdown :global(blockquote) { border-left: 3px solid var(--border-strong); margin: 10px 0; padding-left: 12px; color: var(--fg-muted); }
  .chat-markdown :global(ul), .chat-markdown :global(ol) { padding-left: 22px; margin: 8px 0; }
  .table { overflow-x: auto; margin: 10px 0; }
  table { border-collapse: collapse; width: 100%; font-size: 12px; }
  th, td { border: 1px solid var(--border); padding: 6px 10px; text-align: left; }
  th { background: var(--bg-sunken); font-weight: 600; }
  .literal { white-space: pre-wrap; color: var(--fg-muted); }
</style>
