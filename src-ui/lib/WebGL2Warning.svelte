<script lang="ts">
  /**
   * Shown when the browser cannot give this build a WebGL2 context.
   *
   * This build targets WebGL2 rather than WebGPU, so the guidance here is
   * deliberately short. WebGL2 has been baseline in every major browser since
   * 2017, which means a failure here is almost never "your browser is too old" —
   * it is hardware acceleration being off, a blocklisted GPU, or a lost context.
   * The long browser-by-browser enablement advice the WebGPU build needs would
   * be misleading in this one.
   *
   * The app's dialog (bits-ui Dialog, `.ui-dialog*` in ui.css), its Button and
   * Disclosure.
   */
  import { Dialog } from "bits-ui";
  import Button from "./ui/Button.svelte";
  import Disclosure from "./ui/Disclosure.svelte";

  let { show, onDismiss }: { show: boolean; onDismiss: () => void } = $props();

  const mobile = /android|webos|iphone|ipad|ipod|blackberry|iemobile|opera mini/i.test(navigator.userAgent);
</script>

<Dialog.Root open={show} onOpenChange={(open) => { if (!open) onDismiss(); }}>
  <Dialog.Portal>
    <Dialog.Overlay class="ui-dialog-overlay" />
    <Dialog.Content class="ui-dialog webgl2-warning">
      <Dialog.Title class="ui-dialog-title">WebGL2 not available</Dialog.Title>
      <div class="ui-dialog-body">
        <Dialog.Description class="ui-dialog-description">
          This build renders with WebGL2, and the browser did not return a WebGL2 context. Every current browser
          supports WebGL2, so this usually means it has been turned off rather than that it is missing.
        </Dialog.Description>
        <div>
          <p class="ui-subheading">How to fix this</p>
          <ol>
            <li>Turn on hardware acceleration in the browser's settings.</li>
            <li>Update the browser to its latest version.</li>
            <li>Update the graphics drivers, then restart the browser.</li>
            {#if mobile}<li>On mobile, close other tabs: WebGL2 contexts are limited and can be dropped under memory pressure.</li>{/if}
          </ol>
        </div>
        <Disclosure label="More information">
          <ul>
            <li>Check the browser's GPU status page (chrome://gpu, or about:support in Firefox) for a blocklisted or disabled driver.</li>
            <li>Confirm WebGL2 on its own at <a href="https://get.webgl.org/webgl2/" target="_blank" rel="noopener noreferrer">get.webgl.org/webgl2</a>.</li>
            <li>This build runs the renderer in a worker on an OffscreenCanvas; a browser with OffscreenCanvas turned off fails here too.</li>
          </ul>
        </Disclosure>
        <p class="detected">Detected: {mobile ? "mobile" : "desktop"}</p>
      </div>
      <div class="ui-dialog-actions">
        <Dialog.Close>{#snippet child({ props })}<Button variant="primary" {...props}>Dismiss</Button>{/snippet}</Dialog.Close>
      </div>
    </Dialog.Content>
  </Dialog.Portal>
</Dialog.Root>

<style>
  :global(.ui-dialog.webgl2-warning) { width: min(480px, calc(100vw - 32px)); }
  ol, ul { margin: 4px 0 0; padding-left: 18px; color: var(--fg); line-height: 1.5; }
  ul { margin-top: 6px; color: var(--fg-muted); font-size: var(--text-sm); }
  li + li { margin-top: 2px; }
  a { color: var(--accent); }
  .detected { margin: 0; color: var(--fg-muted); font-size: var(--text-sm); }
</style>
