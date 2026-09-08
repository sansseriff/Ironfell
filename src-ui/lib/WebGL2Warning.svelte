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
   */
  interface Props {
    show: boolean;
    onDismiss: () => void;
  }

  let { show, onDismiss }: Props = $props();

  function isMobile() {
    return /android|webos|iphone|ipad|ipod|blackberry|iemobile|opera mini/i.test(
      navigator.userAgent
    );
  }

  const mobile = isMobile();
  let showTechnicalDetails = $state(false);

  function toggleTechnicalDetails() {
    showTechnicalDetails = !showTechnicalDetails;
  }
</script>

{#if show}
  <div id="backend-warning">
    <div class="warning-content">
      <div class="warning-text">
        <h3>WebGL2 Not Available</h3>
        <p>
          This build renders with WebGL2, and the browser did not return a WebGL2
          context. WebGL2 is supported by every current browser, so this usually
          means it has been disabled rather than that it is missing.
        </p>

        <div class="warning-details">
          <p><strong>How to fix this:</strong></p>
          <ol>
            <li>Enable hardware acceleration in your browser's settings</li>
            <li>Update your browser to the latest version</li>
            <li>Update your graphics drivers, then restart the browser</li>
            {#if mobile}
              <li>
                On mobile, close other tabs — WebGL2 contexts are limited and can
                be dropped under memory pressure
              </li>
            {/if}
          </ol>

          <button class="technical-toggle" onclick={toggleTechnicalDetails}>
            {showTechnicalDetails ? "\u25bc" : "\u25b6"} More Information
          </button>

          {#if showTechnicalDetails}
            <div class="technical-details">
              <p><strong>Advanced troubleshooting:</strong></p>
              <ul>
                <li>
                  Check your browser's GPU status page (chrome://gpu, or
                  about:support in Firefox) for a blocklisted or disabled driver
                </li>
                <li>
                  Confirm WebGL2 independently at
                  <a
                    href="https://get.webgl.org/webgl2/"
                    target="_blank"
                    rel="noopener noreferrer">get.webgl.org/webgl2</a
                  >
                </li>
                <li>
                  This build runs the renderer in a worker on an OffscreenCanvas;
                  a browser with OffscreenCanvas disabled will also fail here
                </li>
              </ul>
            </div>
          {/if}
        </div>

        <div class="browser-info">
          <p><strong>Detected:</strong> {mobile ? "Mobile" : "Desktop"}</p>
        </div>

        <button class="warning-dismiss" onclick={onDismiss}>Dismiss</button>
      </div>
    </div>
  </div>
{/if}

<style>
  #backend-warning {
    position: absolute;
    top: 0;
    left: 0;
    width: 100%;
    height: 100%;
    background-color: rgba(0, 0, 0, 0.95);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 1000;
    padding: 20px;
    box-sizing: border-box;
    background-color: white;
  }

  .warning-content {
    background: linear-gradient(135deg, #ffd2d2, #ffe8be);
    color: black;
    border-radius: 16px;
    padding: 30px;
    max-width: 600px;
    width: 100%;
    box-shadow: 0 20px 40px rgba(0, 0, 0, 0.1);
    animation: warningSlideIn 0.5s ease-out;
    font-family:
      -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
  }


  .warning-text h3 {
    font-size: 24px;
    margin: 0 0 15px 0;
    text-align: center;
    font-weight: 600;
  }

  .warning-text p {
    font-size: 16px;
    line-height: 1.6;
    margin: 0 0 20px 0;
    text-align: center;
  }

  .warning-details {
    background: rgba(255, 255, 255, 0.1);
    border-radius: 8px;
    padding: 20px;
    margin: 20px 0;
    backdrop-filter: blur(10px);
  }

  .warning-details p {
    margin: 0 0 15px 0;
    text-align: left;
  }

  .warning-details ol {
    margin: 10px 0;
    padding-left: 20px;
  }

  .warning-details li {
    margin: 8px 0;
    line-height: 1.4;
  }

  .technical-toggle {
    background: rgba(255, 255, 255, 0.1);
    border: 1px solid rgba(126, 126, 126, 0.2);
    color: rgb(46, 46, 46);
    padding: 8px 12px;
    border-radius: 6px;
    cursor: pointer;
    font-size: 14px;
    margin: 15px 0 0 0;
    transition: all 0.3s ease;
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .technical-toggle:hover {
    background: rgba(255, 255, 255, 0.2);
    border-color: rgba(72, 72, 72, 0.3);
  }

  .technical-details {
    margin-top: 15px;
    padding: 15px;
    background: rgba(255, 255, 255, 0.05);
    border-radius: 6px;
    border: 1px solid rgba(55, 55, 55, 0.1);
    animation: slideDown 0.3s ease-out;
  }

  .technical-details ul {
    margin: 10px 0;
    padding-left: 20px;
  }

  .technical-details li {
    margin: 6px 0;
    line-height: 1.3;
    font-size: 14px;
    opacity: 0.9;
  }



  .technical-details a {
    color: #2b6ea3;
    text-decoration: underline;
  }

  .technical-details a:hover {
    color: #1b4d73;
  }

  @keyframes slideDown {
    from {
      opacity: 0;
      transform: translateY(-10px);
    }
    to {
      opacity: 1;
      transform: translateY(0);
    }
  }


  .browser-info {
    background: rgba(255, 255, 255, 0.05);
    border-radius: 6px;
    padding: 10px;
    margin: 15px 0;
    text-align: center;
    font-size: 14px;
    opacity: 0.8;
  }

  .browser-info p {
    margin: 0;
  }

  .warning-dismiss {
    display: block;
    margin: 20px auto 0;
    padding: 12px 24px;
    background: rgba(255, 255, 255, 0.2);
    border: 2px solid rgba(95, 95, 95, 0.3);
    border-radius: 8px;
    color: rgb(44, 44, 44);
    font-size: 16px;
    font-weight: 500;
    cursor: pointer;
    transition: all 0.3s ease;
    backdrop-filter: blur(10px);
  }

  .warning-dismiss:hover {
    background: rgba(255, 255, 255, 0.3);
    border-color: rgba(204, 204, 204, 0.5);
    transform: translateY(-2px);
  }

  .warning-dismiss:active {
    transform: translateY(0);
  }

  @keyframes warningSlideIn {
    from {
      opacity: 0;
      transform: translateY(-50px) scale(0.9);
    }
    to {
      opacity: 1;
      transform: translateY(0) scale(1);
    }
  }


  /* Mobile responsiveness */
  @media (max-width: 768px) {
    #backend-warning {
      padding: 15px;
    }

    .warning-content {
      padding: 20px;
      border-radius: 12px;
    }


    .warning-text h3 {
      font-size: 20px;
    }

    .warning-text p {
      font-size: 14px;
    }

    .warning-details {
      padding: 15px;
    }

    .warning-details li {
      font-size: 14px;
    }

    .technical-toggle {
      font-size: 13px;
      padding: 6px 10px;
    }

    .technical-details {
      padding: 12px;
    }

    .technical-details li {
      font-size: 13px;
    }


  }

  @media (max-width: 480px) {
    .warning-content {
      padding: 15px;
      border-radius: 8px;
    }


    .warning-text h3 {
      font-size: 18px;
    }

    .warning-text p {
      font-size: 13px;
    }

    .warning-details {
      padding: 12px;
    }

    .warning-details li {
      font-size: 13px;
    }

    .technical-toggle {
      font-size: 12px;
      padding: 5px 8px;
    }

    .technical-details {
      padding: 10px;
    }

    .technical-details li {
      font-size: 12px;
    }



    .warning-dismiss {
      padding: 10px 20px;
      font-size: 14px;
    }
  }
</style>
